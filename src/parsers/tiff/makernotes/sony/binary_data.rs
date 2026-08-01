//! ExifTool's `ProcessBinaryData`, restricted to the shapes Sony's enciphered
//! tables actually use.
//!
//! The generated [`super::enciphered_tables`] holds one row per ExifTool tag;
//! this module is the interpreter for those rows. It reproduces
//! `ProcessBinaryData` in `Image/ExifTool.pm` (13.59):
//!
//! * tags are visited in ascending index order, and the byte offset of a tag is
//!   `index * FORMAT_SIZE + varSize` -- every Sony table's `FORMAT` is `int8u`,
//!   so the index *is* the byte offset until a `Hook` moves `varSize`;
//! * `varSize` is added when the entry offset is computed, which happens
//!   *before* the tag's own `Hook` runs, so a Hook shifts every later tag but
//!   not the tag carrying it;
//! * a tag id with several `Condition` variants resolves to the first variant
//!   whose Condition holds, and to nothing at all if none does;
//! * reading stops at the first index whose offset is at or past the end of the
//!   directory (ExifTool's `last if $more <= 0`), and a value that would run
//!   past the end has its count shortened by `ReadValue`, or is dropped if not
//!   even one element fits;
//! * `Mask` is applied as `$val & mask`;
//! * `RawConv` runs first (and can suppress the tag or set a data member that
//!   later Conditions read), then `ValueConv`, then `PrintConv`;
//! * a `SubDirectory` recurses instead of producing a value, over
//!   `count * FORMAT_SIZE` bytes when the tag declares a `Format` and over the
//!   rest of the directory otherwise.
//!
//! The `$$self{...}` data members ExifTool threads between Sony's directories
//! live in [`Ctx`], which one file's whole MakerNote shares -- `LensMount` is
//! set by 0x9050 and read by its own later tags, `FlashFired` by 0x9050 and
//! read by 0x9405, `Ver9401` by 0x9401 and read by 0x9405. Which of those
//! actually see a value depends on the order the body wrote its IFD entries in,
//! since a Sony MakerNote IFD is not sorted by tag id; the caller walks it in
//! file order, which is the order `ProcessExif` uses, so the dependence is
//! ExifTool's own.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::parsers::tiff::ifd_parser::ByteOrder;

// ===========================================================================
// Table shapes (constructed by the generator)
// ===========================================================================

/// A binary format code, or `Default` to use the table's `FORMAT`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    Default,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    Rat32u,
    Undef,
    /// `string[n]`, truncated at the first NUL.
    Str,
    /// `int16uRev`: an int16u stored in the opposite byte order.
    U16Rev,
}

impl Fmt {
    const fn size(self) -> usize {
        match self {
            Fmt::U8 | Fmt::I8 | Fmt::Undef | Fmt::Default | Fmt::Str => 1,
            Fmt::U16 | Fmt::I16 | Fmt::U16Rev => 2,
            // ExifTool's `rational32u` is 32 bits *total*: two int16u.
            Fmt::U32 | Fmt::I32 | Fmt::Rat32u => 4,
        }
    }
}

/// The `$$self{...}` slots ExifTool threads between Sony's tables.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dm {
    AFType,
    Battery2,
    BatteryStatus1,
    BatteryStatus2,
    FaceInfoLength,
    FaceInfoOffset,
    FacesDetected,
    FlashFired,
    LensMount,
    Locations,
    MetaVersion,
    TagVersion,
    TempTest1,
    TempTest2,
    Ver9401,
}

#[derive(Clone, Copy)]
pub enum NumCmp {
    Eq,
    Ne,
    Ge,
    Gt,
    Le,
    Lt,
}

impl NumCmp {
    fn holds(self, a: f64, b: f64) -> bool {
        match self {
            NumCmp::Eq => a == b,
            NumCmp::Ne => a != b,
            NumCmp::Ge => a >= b,
            NumCmp::Gt => a > b,
            NumCmp::Le => a <= b,
            NumCmp::Lt => a < b,
        }
    }
}

/// A `Condition`. Only the forms ExifTool's Sony tables actually use.
pub enum Cond {
    Always,
    /// `$$self{Model} =~ /RE/` (`true` for the negated `!~`)
    ModelRe(bool, &'static str),
    /// `$$self{Software} =~ /RE/`
    SoftwareRe(bool, &'static str),
    /// `$$self{X} <op> n`
    DmCmp(Dm, NumCmp, f64),
    /// `($$self{X} & mask) <op> n`
    DmBitCmp(Dm, u32, NumCmp, f64),
    /// `$$self{X} =~ /RE/`
    DmRe(Dm, bool, &'static str),
    /// `$$self{X}` on its own
    DmTruthy(Dm),
    All(&'static [Cond]),
    Any(&'static [Cond]),
}

/// `RawConv` -- runs before ValueConv and may suppress the tag.
#[derive(Clone, Copy)]
pub enum Raw {
    None,
    /// `$$self{X} = $val`
    Store(Dm),
    /// `$$self{X} = $val; undef` -- the member is the only point of the tag
    StoreThenUndef(Dm),
    /// `$$self{X} = $val; $$self{Model} =~ /RE/ ? undef : $val`
    StoreUnlessModel(Dm, &'static str),
    /// `$val & 0x00ffffff`
    MaskLow24,
    /// `$val || undef`
    NonZero,
    /// `($val and $val != 255) ? $val : undef`
    NonZeroNot255,
    /// `(($$self{LensMount} != 0) or ($val > 0 and $val < 32784)) ? $val : undef`
    LensMountOrSmall,
    /// `$$self{X} < n ? undef : $val` -- the Nth face is only present when the
    /// body says it detected at least N.
    DropIfDmLess(Dm, f64),
}

/// `ValueConv`.
#[derive(Clone, Copy)]
pub enum Vc {
    None,
    /// `$val / n`
    Div(f64),
    /// `$val + n`
    Add(f64),
    /// `-$val / n`
    NegDiv(f64),
    /// `($val - a) / b`
    SubDiv(f64, f64),
    /// `$val * n`
    Mul(f64),
    /// `$val / a - b`
    DivSub(f64, f64),
    /// `100 * 2**(16 - $val/256)`
    SonyIso,
    /// `16 - $val/256`
    ApexMinusDiv256,
    /// `2 ** (($val/a - b) / 2)`
    Pow2DivSubHalf(f64, f64),
    /// `2 ** (($val - a) / b)`
    Pow2SubDiv(f64, f64),
    /// `2 ** (($val - a) / b) * c`
    Pow2SubDivMul(f64, f64, f64),
    /// `$val ? 2 ** (a - $val/b) : 0`
    ExpTime(f64, f64),
    /// `($val - a) / b` applied to every element of a space-joined list
    EachSubDiv(f64, f64),
    /// `join " ", unpack "H2H2", $val`
    HexPair,
    /// `unpack('vC*')` -> `"%.4d:%.2d:%.2d %.2d:%.2d:%.2d"`
    DateTimeYear16,
    /// `unpack('C*')` -> `"20%.2d:%.2d:%.2d %.2d:%.2d:%.2d"`, dropped when the
    /// year byte is zero
    DateTime20xx,
    /// `unpack('C*')` -> `"%.2d:%.2d"`
    MinSec,
    /// `$val > 128 ? $val - 256 : $val` -- an int8u read as a signed offset
    Signed8Above128,
    /// `$val ? exp(($val/8-6)*log(2))*100 : $val`
    IsoExp,
    /// `($val and $val < 254) ? exp(($val/8-6)*log(2))*100 : $val`
    IsoExpBelow254,
    /// A `ValueConv` written as a lookup hash (Sony's ISO ladders).
    Map(&'static [(&'static str, &'static str)]),
}

/// The `OTHER` fallback of a `PrintConv` hash.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Other {
    None,
    /// `Minolta::afStatusInfo`: `Front Focus (n)` / `Back Focus (+n)`
    MinoltaFocus,
    /// `Minolta` LensType: Metabones/Sigma adapter ids, which need the Canon
    /// and Sigma lens tables. Not implemented, so the value falls through to
    /// `Unknown (n)` exactly as ExifTool does for an id neither table knows.
    MinoltaLens,
    /// `Sony::Tag9050*` Shutter: `Mechanical ($val)`
    ShutterMechanical,
    /// `Exif::PrintParameter`: a positive value prints `+n`, and an int16u
    /// above 0xfff0 is really a small negative.
    ExifParameter,
    /// `int($val + 0.5)`
    RoundHalfUp,
    /// `sub { shift }` -- print the value unchanged.
    Identity,
}

/// `PrintConv`.
#[derive(Clone, Copy)]
pub enum Pc {
    None,
    /// A plain lookup hash. Keys are Perl-stringified values.
    Map(&'static [(&'static str, &'static str)], Other),
    /// A hash with a `BITMASK` fallback: exact matches first, then
    /// `DecodeBits` over `BitsPerWord`-bit words (32 unless the tag says
    /// otherwise).
    Bitmask(
        &'static [(&'static str, &'static str)],
        &'static [(u32, &'static str)],
        u32,
        Other,
    ),
    /// `sprintf("%.1f mm",$val)`
    MmFixed1,
    /// `sprintf("%.Nf",$val)`
    Fixed(usize),
    /// `sprintf("%3d",$val)`
    Width3,
    /// `sprintf("%.Nd",$val)` -- Perl pads an integer to N digits with zeros.
    ZeroPad(usize),
    /// `$val ? sprintf("%.0f",$val) : "Auto"`
    Fixed0OrAuto,
    /// `$val ? sprintf("%.1f",$val) : $val`
    Fixed1OrVal,
    /// `$val ? sprintf("%+.1f",$val) : 0`
    Signed1OrZero,
    /// `$val ? PrintExposureTime($val) : "Bulb"`
    ExposureTimeOrBulb,
    /// `unpack "H*", pack "C*", split " ", $val`
    HexOfBytes,
    /// `$self->ConvertDateTime($val)` -- identity with no `DateFormat` option
    DateTime,
    /// `sprintf("%.0f%%",$val/10.24)`
    PercentOf1024,
    /// `"$val<suffix>"`
    Suffix(&'static str),
    /// `sprintf("20%.2d", $val)`
    Year20,
    /// `$val > 0 ? 8*$val : "n.a."`
    TimesEightOrNa,
    /// `sprintf("%.1f C",$val)`
    Fixed1Celsius,
    /// `sprintf("%x.%.2x",$val>>8,$val&0xff)`
    HexDotHex,
    /// `$val > 0 ? "+$val" : $val`
    PlusOrVal,
    /// `sprintf("Ver.%.2x.%.3d",$val>>8,$val&0xff)`
    VerHex,
    /// `Image::ExifTool::Exif::PrintFNumber($val)`
    FNumber,
    /// `int($val + 0.5)`
    RoundHalfUp,
    /// `$val > a ? "inf" : sprintf("%.2f m", $val)`
    InfAboveOrMeters(f64),
    /// `Sony::PrintLensSpec`-style feature decoding of `LensSpecFeatures`.
    LensSpecFeatures,
}

/// A `Hook`, which shifts every *later* tag in the table.
#[derive(Clone, Copy)]
pub enum Hook {
    None,
    /// `$varSize += n if $$self{Model} =~ /RE/`
    AddIfModel(i32, &'static str),
}

pub struct BinTag {
    pub index: u32,
    pub name: &'static str,
    pub cond: Cond,
    pub fmt: Fmt,
    pub count: u32,
    pub mask: u32,
    pub raw: Raw,
    pub vc: Vc,
    pub pc: Pc,
    pub hook: Hook,
    /// `PrintHex => 1`: an unmatched PrintConv key prints `Unknown (0xN)`.
    pub print_hex: bool,
    /// `Priority => 0`: never displaces a value already found under this name.
    pub low_priority: bool,
    pub subdir: Option<usize>,
}

pub struct BinTable {
    pub name: &'static str,
    /// The table's `FORMAT`, which fixes the index-to-offset increment.
    pub fmt: Fmt,
    pub tags: &'static [BinTag],
}

// ===========================================================================
// Perl scalar semantics
// ===========================================================================

/// A Perl scalar as these tables produce them: a number or a string.
#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    Num(f64),
    Text(String),
}

impl Scalar {
    pub fn num(&self) -> f64 {
        match self {
            Scalar::Num(n) => *n,
            Scalar::Text(s) => perl_numify(s),
        }
    }

    pub fn text(&self) -> String {
        match self {
            Scalar::Num(n) => perl_num_to_string(*n),
            Scalar::Text(s) => s.clone(),
        }
    }

    /// Perl truth: `0`, `"0"` and `""` are false, everything else is true.
    fn truthy(&self) -> bool {
        match self {
            Scalar::Num(n) => *n != 0.0,
            Scalar::Text(s) => !s.is_empty() && s != "0",
        }
    }
}

/// Perl's leading-numeric conversion (`"12abc"` is 12, `"abc"` is 0).
fn perl_numify(s: &str) -> f64 {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    while end < bytes.len() {
        let c = bytes[end] as char;
        match c {
            '+' | '-' if end == 0 => {}
            '0'..='9' => seen_digit = true,
            '.' if !seen_dot => seen_dot = true,
            _ => break,
        }
        end += 1;
    }
    if !seen_digit {
        return 0.0;
    }
    t[..end].parse::<f64>().unwrap_or(0.0)
}

/// Perl's default number stringification (`%.15g`).
pub fn perl_num_to_string(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "NaN".to_string()
        } else if v > 0.0 {
            "Inf".to_string()
        } else {
            "-Inf".to_string()
        };
    }
    if v == 0.0 {
        return "0".to_string();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let exp = v.abs().log10().floor() as i32;
    if !(-5..15).contains(&exp) {
        let mut s = format!("{:.*e}", 14, v);
        if let Some(pos) = s.find('e') {
            let (mantissa, e) = s.split_at(pos);
            let mut m = mantissa.to_string();
            if m.contains('.') {
                while m.ends_with('0') {
                    m.pop();
                }
                if m.ends_with('.') {
                    m.pop();
                }
            }
            let exp_val: i32 = e[1..].parse().unwrap_or(0);
            s = format!(
                "{}e{}{:02}",
                m,
                if exp_val < 0 { '-' } else { '+' },
                exp_val.abs()
            );
        }
        return s;
    }
    let decimals = (14 - exp).max(0) as usize;
    let mut s = format!("{:.*}", decimals, v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".to_string() } else { s }
}

/// Compiled once per distinct pattern; the tables hold a few dozen.
///
/// Patterns are `&'static str` because every one of them is a literal in the
/// generated tables, which is also what lets the compiled `Regex` be cached for
/// the process's lifetime.
fn regex_for(pattern: &'static str) -> Option<&'static Regex> {
    static CACHE: Lazy<std::sync::Mutex<HashMap<&'static str, Option<&'static Regex>>>> =
        Lazy::new(|| std::sync::Mutex::new(HashMap::new()));
    let mut cache = CACHE.lock().ok()?;
    *cache.entry(pattern).or_insert_with(|| {
        Regex::new(pattern)
            .ok()
            .map(|r| &*Box::leak(Box::new(r)) as &'static Regex)
    })
}

fn re_matches(pattern: &'static str, subject: &str) -> bool {
    regex_for(pattern).is_some_and(|r| r.is_match(subject))
}

/// `$$self{Model} =~ /RE/`, for the `Sony::Main` dispatch in
/// [`super::enciphered`].
pub fn model_matches(pattern: &'static str, model: &str) -> bool {
    re_matches(pattern, model)
}

// ===========================================================================
// Evaluation context
// ===========================================================================

/// The `$$self{...}` state shared by every Sony directory in one file.
#[derive(Default)]
pub struct Ctx {
    members: HashMap<Dm, Scalar>,
    pub model: String,
    /// `$$self{Software}` -- EXIF `Software`, which Sony bodies write as
    /// `"ILCE-9 v5.00"`. An absent Software is the empty string, which is what
    /// Perl's `undef` compares as: `undef !~ /RE/` is true and `undef =~ /RE/`
    /// is false for every anchored pattern these tables use.
    pub software: String,
    /// ExifTool's `$$et{DoubleCipher}`, set when 0x9400 announces a block that
    /// ExifTool 9.04-9.10 enciphered twice.
    pub double_cipher: bool,
}

impl Ctx {
    pub fn new(model: Option<&str>, software: Option<&str>) -> Self {
        Ctx {
            members: HashMap::new(),
            model: model.unwrap_or_default().to_string(),
            software: software.unwrap_or_default().to_string(),
            double_cipher: false,
        }
    }

    pub fn member(&self, dm: Dm) -> Option<&Scalar> {
        self.members.get(&dm)
    }

    fn set(&mut self, dm: Dm, v: Scalar) {
        self.members.insert(dm, v);
    }

    fn holds(&self, cond: &Cond) -> bool {
        match cond {
            Cond::Always => true,
            Cond::ModelRe(neg, re) => re_matches(re, &self.model) != *neg,
            Cond::SoftwareRe(neg, re) => re_matches(re, &self.software) != *neg,
            // An unset data member is Perl's undef: numerically 0, and false.
            Cond::DmCmp(dm, op, n) => op.holds(self.members.get(dm).map_or(0.0, Scalar::num), *n),
            Cond::DmBitCmp(dm, mask, op, n) => {
                let v = self.members.get(dm).map_or(0.0, Scalar::num);
                op.holds(((v as i64) & (*mask as i64)) as f64, *n)
            }
            Cond::DmRe(dm, neg, re) => {
                let text = self.members.get(dm).map(Scalar::text).unwrap_or_default();
                re_matches(re, &text) != *neg
            }
            Cond::DmTruthy(dm) => self.members.get(dm).is_some_and(Scalar::truthy),
            Cond::All(list) => list.iter().all(|c| self.holds(c)),
            Cond::Any(list) => list.iter().any(|c| self.holds(c)),
        }
    }
}

/// One extracted tag, before duplicate names are resolved.
pub struct Found {
    pub name: &'static str,
    pub value: String,
    /// `Priority => 0`
    pub low_priority: bool,
}

// ===========================================================================
// Reading values
// ===========================================================================

fn read_scalar(data: &[u8], off: usize, fmt: Fmt, order: ByteOrder) -> Option<Scalar> {
    let size = fmt.size();
    let b = data.get(off..off.checked_add(size)?)?;
    let le = order == ByteOrder::LittleEndian;
    let u16v = || {
        if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        }
    };
    let u32v = || {
        if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        }
    };
    Some(match fmt {
        Fmt::U8 | Fmt::Default => Scalar::Num(b[0] as f64),
        Fmt::I8 => Scalar::Num(b[0] as i8 as f64),
        Fmt::U16 => Scalar::Num(u16v() as f64),
        Fmt::I16 => Scalar::Num(u16v() as i16 as f64),
        Fmt::U32 => Scalar::Num(u32v() as f64),
        Fmt::I32 => Scalar::Num(u32v() as i32 as f64),
        Fmt::Rat32u => {
            // `GetRational32u`: two int16u, rounded to 7 significant digits,
            // with ExifTool's own words for a zero denominator.
            let n = u16v() as f64;
            let d = if le {
                u16::from_le_bytes([b[2], b[3]])
            } else {
                u16::from_be_bytes([b[2], b[3]])
            } as f64;
            if d == 0.0 {
                Scalar::Text(if n != 0.0 { "inf" } else { "undef" }.to_string())
            } else {
                Scalar::Num(round_float(n / d, 7))
            }
        }
        Fmt::Undef | Fmt::Str => Scalar::Num(b[0] as f64),
        Fmt::U16Rev => Scalar::Num(if le {
            u16::from_be_bytes([b[0], b[1]]) as f64
        } else {
            u16::from_le_bytes([b[0], b[1]]) as f64
        }),
    })
}

/// ExifTool's `ReadValue`: `count` values joined by spaces, the count shortened
/// to whatever fits in `avail`, and nothing at all when not even one fits.
fn read_value(
    data: &[u8],
    off: usize,
    fmt: Fmt,
    count: u32,
    avail: usize,
    order: ByteOrder,
) -> Option<(Scalar, Vec<u8>)> {
    let size = fmt.size();
    let mut count = count as usize;
    if size * count > avail {
        count = avail / size;
        if count < 1 {
            return None;
        }
    }
    let raw = data.get(off..off + size * count)?.to_vec();
    if fmt == Fmt::Str {
        // ExifTool truncates a string at its first NUL.
        let text = raw.split(|b| *b == 0).next().unwrap_or(&[]);
        return Some((
            Scalar::Text(String::from_utf8_lossy(text).into_owned()),
            raw,
        ));
    }
    if fmt == Fmt::Undef {
        // `undef` is the raw byte string, which only the byte-wise ValueConvs
        // ever look at.
        return Some((Scalar::Text(String::new()), raw));
    }
    let mut parts = Vec::with_capacity(count);
    for i in 0..count {
        parts.push(read_scalar(data, off + i * size, fmt, order)?);
    }
    let scalar = if parts.len() == 1 {
        parts.remove(0)
    } else {
        Scalar::Text(parts.iter().map(Scalar::text).collect::<Vec<_>>().join(" "))
    };
    Some((scalar, raw))
}

// ===========================================================================
// Conversions
// ===========================================================================

fn map_lookup(map: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    map.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn apply_raw(raw: Raw, val: Scalar, ctx: &mut Ctx) -> Option<Scalar> {
    Some(match raw {
        Raw::None => val,
        Raw::Store(dm) => {
            ctx.set(dm, val.clone());
            val
        }
        Raw::StoreThenUndef(dm) => {
            ctx.set(dm, val);
            return None;
        }
        Raw::StoreUnlessModel(dm, re) => {
            ctx.set(dm, val.clone());
            if re_matches(re, &ctx.model) {
                return None;
            }
            val
        }
        Raw::MaskLow24 => Scalar::Num(((val.num() as i64) & 0x00ff_ffff) as f64),
        Raw::NonZero => {
            if !val.truthy() {
                return None;
            }
            val
        }
        Raw::NonZeroNot255 => {
            if !val.truthy() || val.num() == 255.0 {
                return None;
            }
            val
        }
        Raw::DropIfDmLess(dm, n) => {
            if ctx.members.get(&dm).map_or(0.0, Scalar::num) < n {
                return None;
            }
            val
        }
        Raw::LensMountOrSmall => {
            let mount = ctx.members.get(&Dm::LensMount).map_or(0.0, Scalar::num);
            let n = val.num();
            if !(mount != 0.0 || (n > 0.0 && n < 32784.0)) {
                return None;
            }
            val
        }
    })
}

fn apply_vc(vc: Vc, val: Scalar, raw: &[u8]) -> Option<Scalar> {
    let n = || val.num();
    Some(match vc {
        Vc::None => val,
        Vc::Div(d) => Scalar::Num(n() / d),
        Vc::Add(a) => Scalar::Num(n() + a),
        Vc::NegDiv(d) => Scalar::Num(-n() / d),
        Vc::SubDiv(a, b) => Scalar::Num((n() - a) / b),
        Vc::Mul(m) => Scalar::Num(n() * m),
        Vc::DivSub(d, b) => Scalar::Num(n() / d - b),
        Vc::SonyIso => Scalar::Num(100.0 * 2f64.powf(16.0 - n() / 256.0)),
        Vc::ApexMinusDiv256 => Scalar::Num(16.0 - n() / 256.0),
        Vc::Pow2DivSubHalf(d, b) => Scalar::Num(2f64.powf((n() / d - b) / 2.0)),
        Vc::Pow2SubDiv(a, b) => Scalar::Num(2f64.powf((n() - a) / b)),
        Vc::Pow2SubDivMul(a, b, m) => Scalar::Num(2f64.powf((n() - a) / b) * m),
        Vc::ExpTime(a, b) => {
            let v = n();
            Scalar::Num(if v != 0.0 { 2f64.powf(a - v / b) } else { 0.0 })
        }
        Vc::EachSubDiv(a, b) => Scalar::Text(
            val.text()
                .split_whitespace()
                .map(|p| perl_num_to_string((perl_numify(p) - a) / b))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Vc::HexPair => {
            if raw.len() < 2 {
                return None;
            }
            Scalar::Text(format!("{:02x} {:02x}", raw[0], raw[1]))
        }
        Vc::DateTimeYear16 => {
            if raw.len() < 7 {
                return None;
            }
            // unpack('vC*'): a 16-bit little-endian year then five bytes.
            let year = u16::from_le_bytes([raw[0], raw[1]]);
            Scalar::Text(format!(
                "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
                year, raw[2], raw[3], raw[4], raw[5], raw[6]
            ))
        }
        Vc::DateTime20xx => {
            if raw.len() < 6 || raw[0] == 0 {
                return None;
            }
            Scalar::Text(format!(
                "20{:02}:{:02}:{:02} {:02}:{:02}:{:02}",
                raw[0], raw[1], raw[2], raw[3], raw[4], raw[5]
            ))
        }
        Vc::MinSec => {
            if raw.len() < 2 {
                return None;
            }
            Scalar::Text(format!("{:02}:{:02}", raw[0], raw[1]))
        }
        Vc::Signed8Above128 => {
            let v = n();
            Scalar::Num(if v > 128.0 { v - 256.0 } else { v })
        }
        Vc::IsoExp => {
            let v = n();
            Scalar::Num(if v != 0.0 {
                ((v / 8.0 - 6.0) * std::f64::consts::LN_2).exp() * 100.0
            } else {
                v
            })
        }
        Vc::IsoExpBelow254 => {
            let v = n();
            Scalar::Num(if v != 0.0 && v < 254.0 {
                ((v / 8.0 - 6.0) * std::f64::consts::LN_2).exp() * 100.0
            } else {
                v
            })
        }
        Vc::Map(map) => match map_lookup(map, &val.text()) {
            Some(v) => Scalar::Text(v.to_string()),
            // A ValueConv hash with no entry yields undef, so the tag is dropped.
            None => return None,
        },
    })
}

/// `Image::ExifTool::Exif::PrintExposureTime`.
fn print_exposure_time(seconds: f64) -> String {
    if seconds > 0.0 && seconds < 0.25001 {
        return format!("1/{}", (0.5 + 1.0 / seconds) as i64);
    }
    if seconds == seconds.trunc() {
        return format!("{}", seconds as i64);
    }
    format!("{:.1}", seconds)
}

/// ExifTool's `RoundFloat`: keep `sig` significant digits.
fn round_float(val: f64, sig: i32) -> f64 {
    if val == 0.0 {
        return 0.0;
    }
    let sign = if val < 0.0 { -1.0 } else { 1.0 };
    let val = val.abs();
    let log = val.log10();
    let exp = log.trunc() as i32 - i32::from(log < 0.0) - sig + 1;
    sign * ((10f64.powi(-exp) * val + 0.5).trunc()) * 10f64.powi(exp)
}

/// `Image::ExifTool::Exif::PrintFNumber` (Exif.pm), verbatim: one decimal
/// place, two below f/1.0, and anything not a positive number untouched.
fn print_f_number(v: f64) -> String {
    if v > 0.0 {
        if v < 1.0 {
            format!("{:.2}", v)
        } else {
            format!("{:.1}", v)
        }
    } else {
        perl_num_to_string(v)
    }
}

/// ExifTool's `DecodeBits`: each space-separated word of `val` contributes
/// `bits_per_word` bits, numbered from the low bit of the first word.
fn decode_bits(val: &str, bits: &[(u32, &str)], bits_per_word: u32) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut base = 0u32;
    for word in val.split_whitespace() {
        let w = perl_numify(word) as i64;
        for i in 0..bits_per_word {
            if w & (1i64 << i) == 0 {
                continue;
            }
            let n = i + base;
            match bits.iter().find(|(b, _)| *b == n) {
                Some((_, name)) => out.push((*name).to_string()),
                None => out.push(format!("[{}]", n)),
            }
        }
        base += bits_per_word;
    }
    if out.is_empty() {
        return "(none)".to_string();
    }
    out.join(", ")
}

/// `Minolta::afStatusInfo`'s `OTHER`.
fn minolta_focus(val: f64) -> String {
    if val < 0.0 {
        format!("Front Focus ({})", perl_num_to_string(val))
    } else {
        format!("Back Focus (+{})", perl_num_to_string(val))
    }
}

/// `@lensFeatures` from `Sony.pm`, in the order ExifTool applies them: mask,
/// the names its masked bits map to, and whether the name goes in front.
///
/// The high byte of each mask is byte 0 of a `LensSpec` and the low byte is
/// byte 7, which is why the two-byte `LensSpecFeatures` value can be read with
/// the same table.
#[rustfmt::skip]
const LENS_FEATURES: &[(u32, &[(u32, &str)], bool)] = &[
    (0x4000, &[(0x4000, "PZ")], true),
    (0x0300, &[(0x0100, "DT"), (0x0200, "FE"), (0x0300, "E")], true),
    (0x00e0, &[(0x0020, "STF"), (0x0040, "Reflex"), (0x0060, "Macro"), (0x0080, "Fisheye")], false),
    (0x000c, &[(0x0004, "ZA"), (0x0008, "G")], false),
    (0x0003, &[(0x0001, "SSM"), (0x0002, "SAM")], false),
    (0x8000, &[(0x8000, "OSS")], false),
    (0x2000, &[(0x2000, "LE")], false),
    (0x0800, &[(0x0800, "II")], false),
];

/// `Sony::PrintLensSpec` restricted to the two-byte `LensSpecFeatures` form.
fn print_lens_spec_features(val: &str) -> String {
    let parts: Vec<&str> = val.split_whitespace().collect();
    if parts.len() != 2 {
        return format!("Unknown ({})", val);
    }
    let flags = match u32::from_str_radix(&format!("{}{}", parts[0], parts[1]), 16) {
        Ok(f) => f,
        Err(_) => return format!("Unknown ({})", val),
    };
    let mut out = String::new();
    for (mask, names, prefix) in LENS_FEATURES {
        let bits = mask & flags;
        let name = match names.iter().find(|(b, _)| *b == bits) {
            Some((_, n)) => (*n).to_string(),
            None => {
                if bits == 0 {
                    continue;
                }
                format!("Unknown({:04x})", bits)
            }
        };
        out = if out.is_empty() {
            name
        } else if *prefix {
            format!("{} {}", name, out)
        } else {
            format!("{} {}", out, name)
        };
    }
    out
}

/// ExifTool's fallback for a PrintConv hash with no matching key.
fn unknown(val: &Scalar, print_hex: bool) -> String {
    let n = val.num();
    if print_hex && n >= 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64 {
        format!("Unknown (0x{:x})", n as u32)
    } else {
        format!("Unknown ({})", val.text())
    }
}

fn apply_other(other: Other, val: &Scalar) -> Option<String> {
    match other {
        Other::None => None,
        Other::MinoltaFocus => Some(minolta_focus(val.num())),
        // Needs the Canon and Sigma lens tables to name an adapter; without a
        // match ExifTool's own sub returns undef, and the value falls through
        // to `Unknown (n)`.
        Other::MinoltaLens => None,
        Other::ShutterMechanical => Some(format!("Mechanical ({})", val.text())),
        Other::RoundHalfUp => Some(perl_num_to_string((val.num() + 0.5).floor())),
        Other::Identity => Some(val.text()),
        Other::ExifParameter => Some({
            let n = val.num();
            if n > 65520.0 {
                perl_num_to_string(n - 65536.0)
            } else if n > 0.0 {
                format!("+{}", val.text())
            } else {
                val.text()
            }
        }),
    }
}

fn apply_pc(pc: Pc, val: Scalar, print_hex: bool) -> String {
    match pc {
        Pc::None => val.text(),
        Pc::Map(map, other) => match map_lookup(map, &val.text()) {
            Some(v) => v.to_string(),
            None => apply_other(other, &val).unwrap_or_else(|| unknown(&val, print_hex)),
        },
        // ExifTool tries the exact key first, then DecodeBits; a hash with a
        // BITMASK never falls through to OTHER or to `Unknown (n)`.
        Pc::Bitmask(map, bits, per_word, _other) => match map_lookup(map, &val.text()) {
            Some(v) => v.to_string(),
            None => decode_bits(&val.text(), bits, per_word),
        },
        Pc::MmFixed1 => format!("{:.1} mm", val.num()),
        Pc::Fixed(n) => format!("{:.*}", n, val.num()),
        Pc::Width3 => format!("{:3}", val.num() as i64),
        Pc::ZeroPad(n) => {
            let v = val.num() as i64;
            if v < 0 {
                format!("-{:0width$}", -v, width = n)
            } else {
                format!("{:0width$}", v, width = n)
            }
        }
        Pc::Fixed0OrAuto => {
            if val.truthy() {
                format!("{:.0}", val.num())
            } else {
                "Auto".to_string()
            }
        }
        Pc::Fixed1OrVal => {
            if val.truthy() {
                format!("{:.1}", val.num())
            } else {
                val.text()
            }
        }
        Pc::Signed1OrZero => {
            if val.truthy() {
                format!("{:+.1}", val.num())
            } else {
                "0".to_string()
            }
        }
        Pc::ExposureTimeOrBulb => {
            if val.truthy() {
                print_exposure_time(val.num())
            } else {
                "Bulb".to_string()
            }
        }
        Pc::HexOfBytes => val
            .text()
            .split_whitespace()
            .map(|p| format!("{:02x}", (perl_numify(p) as i64) as u8))
            .collect::<Vec<_>>()
            .join(""),
        Pc::DateTime => val.text(),
        Pc::PercentOf1024 => format!("{:.0}%", val.num() / 10.24),
        Pc::Suffix(suffix) => format!("{}{}", val.text(), suffix),
        Pc::Year20 => format!("20{:02}", val.num() as i64),
        Pc::TimesEightOrNa => {
            if val.num() > 0.0 {
                perl_num_to_string(8.0 * val.num())
            } else {
                "n.a.".to_string()
            }
        }
        Pc::Fixed1Celsius => format!("{:.1} C", val.num()),
        Pc::HexDotHex => {
            let v = val.num() as i64;
            format!("{:x}.{:02x}", v >> 8, v & 0xff)
        }
        Pc::PlusOrVal => {
            if val.num() > 0.0 {
                format!("+{}", val.text())
            } else {
                val.text()
            }
        }
        Pc::VerHex => {
            let v = val.num() as i64;
            format!("Ver.{:02x}.{:03}", v >> 8, v & 0xff)
        }
        Pc::FNumber => print_f_number(val.num()),
        Pc::RoundHalfUp => perl_num_to_string((val.num() + 0.5).floor()),
        Pc::InfAboveOrMeters(a) => {
            if val.num() > a {
                "inf".to_string()
            } else {
                format!("{:.2} m", val.num())
            }
        }
        Pc::LensSpecFeatures => print_lens_spec_features(&val.text()),
    }
}

// ===========================================================================
// ProcessBinaryData
// ===========================================================================

/// Walks one binary-data table over `data`, appending everything it yields.
///
/// `tables` is the generated table list a `SubDirectory` index refers into, and
/// `data` the directory's own bytes (already deciphered for the enciphered
/// blocks); index 0 is ExifTool's `$dirStart`.
pub fn process(
    tables: &'static [BinTable],
    table: usize,
    data: &[u8],
    order: ByteOrder,
    ctx: &mut Ctx,
    out: &mut Vec<Found>,
) {
    process_depth(tables, table, data, order, ctx, out, 0)
}

#[allow(clippy::too_many_arguments)]
fn process_depth(
    tables: &'static [BinTable],
    table: usize,
    data: &[u8],
    order: ByteOrder,
    ctx: &mut Ctx,
    out: &mut Vec<Found>,
    depth: u8,
) {
    // Sony's tables nest one level (AFInfo -> AFStatus*, Tag9401 -> ISOInfo);
    // the guard is against a future table cycling, not against these.
    if depth > 4 {
        return;
    }
    let Some(tbl) = tables.get(table) else {
        return;
    };
    let size = data.len();
    let increment = tbl.fmt.size();
    let mut var_size: i64 = 0;
    let mut index = 0usize;
    while index < tbl.tags.len() {
        // ExifTool resolves a tag id with several Condition variants to the
        // first variant that holds, and to nothing at all when none does.
        let id = tbl.tags[index].index;
        let mut end = index;
        while end < tbl.tags.len() && tbl.tags[end].index == id {
            end += 1;
        }
        let chosen = tbl.tags[index..end].iter().find(|t| ctx.holds(&t.cond));
        index = end;
        let Some(tag) = chosen else { continue };

        let entry = id as i64 * increment as i64 + var_size;
        if entry < 0 {
            continue;
        }
        let entry = entry as usize;
        // `last if $more <= 0`: the table stops at the end of the directory.
        if entry >= size {
            break;
        }
        let more = size - entry;

        // The Hook runs after this tag's offset is fixed, so it shifts only
        // the tags after it.
        if let Hook::AddIfModel(delta, re) = tag.hook {
            if re_matches(re, &ctx.model) {
                var_size += delta as i64;
            }
        }

        let fmt = if tag.fmt == Fmt::Default {
            tbl.fmt
        } else {
            tag.fmt
        };

        if let Some(sub) = tag.subdir {
            // A sub-directory spans `count * FORMAT_SIZE` bytes when the tag
            // declares a Format, and the rest of the directory otherwise.
            let len = if tag.fmt != Fmt::Default {
                (tag.count as usize * fmt.size()).min(more)
            } else {
                more
            };
            let bytes = &data[entry..entry + len];
            process_depth(tables, sub, bytes, order, ctx, out, depth + 1);
            continue;
        }

        let Some((mut val, raw_bytes)) = read_value(data, entry, fmt, tag.count, more, order)
        else {
            continue;
        };
        if tag.mask != 0 {
            // `($val & $mask) >> $BitShift`, where ExifTool derives BitShift as
            // the index of the mask's lowest set bit (ExifTool.pm:5917).
            let masked = (val.num() as i64) & tag.mask as i64;
            val = Scalar::Num((masked >> tag.mask.trailing_zeros()) as f64);
        }
        // ExifTool's `Hidden` only suppresses verbose output, never
        // extraction, so it is not represented here: every hidden tag in these
        // tables is a data-member carrier whose RawConv already returns undef.
        let Some(val) = apply_raw(tag.raw, val, ctx) else {
            continue;
        };
        let Some(val) = apply_vc(tag.vc, val, &raw_bytes) else {
            continue;
        };
        out.push(Found {
            name: tag.name,
            value: apply_pc(tag.pc, val, tag.print_hex),
            low_priority: tag.low_priority,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perl_stringification_matches_perl() {
        assert_eq!(perl_num_to_string(1.0), "1");
        assert_eq!(perl_num_to_string(-0.0), "0");
        assert_eq!(perl_num_to_string(0.5), "0.5");
        assert_eq!(perl_num_to_string(1.0 / 3.0), "0.333333333333333");
    }

    #[test]
    fn exposure_time_uses_exiftools_thresholds() {
        assert_eq!(print_exposure_time(1.0 / 250.0), "1/250");
        assert_eq!(print_exposure_time(2.0), "2");
        assert_eq!(print_exposure_time(0.4), "0.4");
    }

    #[test]
    fn minolta_focus_signs_match_exiftool() {
        assert_eq!(minolta_focus(-5.0), "Front Focus (-5)");
        assert_eq!(minolta_focus(7.0), "Back Focus (+7)");
    }

    #[test]
    fn an_unset_data_member_is_perls_undef() {
        let ctx = Ctx::new(Some("ILCE-9"), None);
        // undef == 0 is true, undef != 0 is false.
        assert!(ctx.holds(&Cond::DmCmp(Dm::LensMount, NumCmp::Eq, 0.0)));
        assert!(!ctx.holds(&Cond::DmTruthy(Dm::LensMount)));
        // An absent Software matches `!~` and fails `=~`, as Perl's undef does.
        assert!(ctx.holds(&Cond::SoftwareRe(true, r"^ILCE-9 (v5.0|v6.0)")));
        assert!(!ctx.holds(&Cond::SoftwareRe(false, r"^ILCE-9 (v5.0|v6.0)")));
    }

    #[test]
    fn lens_spec_features_decode_the_flag_groups() {
        // 0x0308: E-mount + G.
        assert_eq!(print_lens_spec_features("03 08"), "E G");
        assert_eq!(print_lens_spec_features("00 00"), "");
    }
}
