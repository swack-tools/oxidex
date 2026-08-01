//! ExifTool's `ProcessBinaryData`, restricted to the shapes Nikon's encrypted
//! tables actually use.
//!
//! The generated [`super::encrypted_tables`] holds one row per ExifTool tag;
//! this module is the interpreter for those rows. It reproduces
//! `ProcessBinaryData` in `Image/ExifTool.pm`:
//!
//! * tags are visited in ascending index order, and the byte offset of a tag is
//!   `index * FORMAT_SIZE + varSize` -- `varSize` only ever moves here through a
//!   `Hook`, since none of these tables uses a variable-length format;
//! * a tag id with several `Condition` variants resolves to the first variant
//!   whose Condition holds, and to nothing at all if none does;
//! * `Mask` is applied as `($val & mask) >> BitShift`;
//! * `RawConv` runs first (and can suppress the tag or set a data member that
//!   later Conditions read), then `ValueConv`, then `PrintConv`;
//! * a `SubDirectory` recurses instead of producing a value, and a
//!   `Start => '$val'` subdirectory with a zero offset is skipped.
//!
//! Tables carrying `VARS => { NIKON_OFFSETS => n }` (the Z bodies) keep their
//! sub-directory offsets in a table at `n`; `PrepareNikonOffsets` derives each
//! sub-directory's length from the gap to the next offset, which this module
//! does in [`nikon_offset_lengths`].

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use super::encrypted_tables::TABLES;
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
    Fixed32u,
    Str,
    Undef,
}

impl Fmt {
    fn size(self) -> usize {
        match self {
            Fmt::U8 | Fmt::I8 | Fmt::Str | Fmt::Undef | Fmt::Default => 1,
            Fmt::U16 | Fmt::I16 => 2,
            Fmt::U32 | Fmt::I32 | Fmt::Fixed32u => 4,
        }
    }
}

/// The `$$self{...}` slots ExifTool threads between Nikon tables.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dm {
    AFAreaMode,
    AfAreaInitialHeight,
    AfAreaInitialWidth,
    AutoCapturedFrame,
    BracketSet,
    CmdDialsReverseRotExposureComp,
    DynamicAFAreaSize,
    FirmwareVersion,
    FlashControlBuiltin,
    FlashControlMode,
    FlashGroupOptionsMasterMode,
    FocusDistanceRangeWidth,
    FocusMode,
    FocusShiftNumberShots,
    FocusShiftShooting,
    FocusStepsFromInfinity,
    HDMIBitDepth,
    HDMIOutputNLog,
    HDR,
    ImageArea,
    IntervalFrame,
    IntervalShooting,
    IntervalShootingIntervals,
    IntervalShootingShotsPerInterval,
    LensDriveEnd,
    LensID,
    MovieType,
    MultipleExposureMode,
    NewLensData,
    OldLensData,
    PixelShiftActive,
    PixelShiftShooting,
    ShotInfoVersion,
    ShutterMode,
    SingleFrame,
    ZebraPatternToneRange,
}

#[derive(Clone, Copy)]
pub enum NumCmp {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

/// Perl's string-comparison operators, as used on firmware versions.
#[derive(Clone, Copy)]
pub enum StrCmp {
    Ge,
    Le,
    Gt,
    Lt,
}

/// A `Condition`. Only the forms ExifTool's Nikon tables actually use.
#[derive(Clone, Copy)]
pub enum Cond {
    Always,
    /// `$$self{FILE_TYPE} eq/ne "X"`
    FileType(bool, &'static str),
    /// `$$self{Model} =~ /RE/` (third field is the `/i` flag)
    Model(bool, &'static str, bool),
    /// `$$self{Model} eq "X"`
    ModelEq(&'static str),
    /// `$$self{Model} eq "X" and $$self{FirmwareVersion} ge "Y"`
    ModelEqAndFirmwareGe(&'static str, &'static str),
    /// `$$self{Model} =~ /RE/ and $$self{FirmwareVersion} and ... <op> "Y"`
    ModelAndFirmware(bool, &'static str, bool, StrCmp, &'static str),
    /// `[$$self{FirmwareVersion} and] $$self{FirmwareVersion} <op> "X"`
    FirmwareCmp(StrCmp, &'static str, bool),
    /// `$$self{FirmwareVersion} eq/ne "X"`
    FirmwareEq(&'static str, bool),
    /// `$$self{FirmwareVersion} =~ /RE/`
    FirmwareRe(&'static str, bool),
    /// `$$self{X}` on its own
    Truthy(Dm),
    /// `$$self{X} <op> n`
    Num(Dm, NumCmp, f64),
    /// `$$self{X} eq/ne "s"`
    StrEq(Dm, &'static str, bool),
    /// `$$self{X} and $$self{X} <op> n`
    TruthyNum(Dm, NumCmp, f64),
    /// `$$self{X} and $$self{X} ne "s"`
    TruthyStrNe(Dm, &'static str),
    /// `$$self{A} and $$self{A} ne "s" and $$self{B} <op> n`
    TruthyStrNeAndNum(Dm, &'static str, Dm, NumCmp, f64),
    /// `$$self{A} and $$self{A} <op> n and $$self{B} ne "s"`
    TruthyNumAndStrNe(Dm, NumCmp, f64, Dm, &'static str),
}

/// `RawConv` -- runs before ValueConv and may suppress the tag.
#[derive(Clone, Copy)]
pub enum Raw {
    None,
    /// `$$self{X} = $val`
    Store(Dm),
    /// `$val = $val/256`
    Div256,
    /// `$val || undef`
    NonZero,
    /// `$val =~ /^\d\.\d+.$/ ? $val : undef`
    FirmwareLike,
    /// `$$self{X} = 1 unless $val =~ /^.\0+$/s; undef`
    StoreFlagIfNotPadding(Dm),
    /// `$$self{X} = 1 + int($val / n)`
    StoreScaled(Dm, f64),
}

/// An extra guard applied after [`Raw`], for the compound RawConvs.
#[derive(Clone, Copy)]
pub enum Filter {
    None,
    /// `$val =~ /^\d+$/ ? $val : undef`
    DigitsOnly,
}

/// `ValueConv`.
#[derive(Clone, Copy)]
pub enum Vc {
    None,
    /// `2**($val/n)`
    Pow2Div(f64),
    /// `2**($val/384-1)`
    Pow2Div384Minus1,
    /// `5 * 2**($val/24)`
    FiveTimesPow2Div24,
    /// `2**(($val-a)/b)`
    Pow2SubDiv(f64, f64),
    /// `($val > 0x7 ? $val - 0x10 : $val) / 6`
    Nibble4Div6,
    /// `2 ** (-$val/n)`
    Pow2NegDiv(f64),
    /// `2 ** (-$val-n)`
    Pow2NegSub(f64),
    /// `2 ** ($val - n)`
    Pow2Sub(f64),
    /// `($val-a)/b`
    SubDiv(f64, f64),
    /// `$val / n`
    Div(f64),
    /// `-$val/n`
    NegDiv(f64),
    /// `$val + n`
    Add(f64),
    /// `100*exp(($val/12-5)*log(2))`
    Iso100Exp,
    /// `0.01 * 10**($val/40)`
    Pow10Div40,
    /// `$val ? 2048 / $val : $val`
    Recip2048,
    /// `$val <= 180 ? $val : $val - 360`
    Signed360,
    /// `$val eq -1 ? 'No Limit' : $val`
    NoLimit,
    /// `$val < 10 ? $val + 1 : a * ($val - b)`
    SmallOrScaled(f64, f64),
    /// `my $t = ($val - 16) % 24; $t ? $val / 24 : 2 + ($val - 16) / 24`
    D3bShutterSpeed,
    /// `$$self{SingleFrame} == 0 ? 5 : $val`
    SingleFrameOrFive,
    /// `unpack("n", $val)`
    UnpackBigEndian16,
}

/// `PrintConv`.
#[derive(Clone, Copy)]
pub enum Pc {
    None,
    /// A plain lookup hash. Keys are Perl-stringified numbers.
    Map(&'static [(&'static str, &'static str)]),
    /// A hash with a `BITMASK` fallback: exact matches first, then `DecodeBits`.
    Bitmask(
        &'static [(&'static str, &'static str)],
        &'static [(u32, &'static str)],
    ),
    /// `sprintf("%.1fmm",$val/n)`
    MmDiv(f64),
    /// `sprintf("f/%.1f",$val/100)`
    FNumberDiv100,
    /// `$val ? sprintf("%+.Nf", $val) : 0`
    SignedOrZero(usize),
    /// `sprintf("%+.Nf",$val)`
    Signed(usize),
    /// `"$val <suffix>"`
    Suffix(&'static str),
    /// `"<prefix>$val"`
    Prefix(&'static str),
    /// `return 'Full' if $val > 0.99; PrintExposureTime($val)`
    FullOrExposureTime,
    /// `sprintf("%.Nf",$val)`
    Fixed(usize),
    /// `sprintf("%.Nf <suffix>",$val)`
    FixedSuffix(usize, &'static str),
    /// `sprintf("%.1f m", $val/10)`
    MetersDiv10,
    /// `int($val + 0.5)`
    RoundHalfUp,
    /// `$val>0.99 ? "Full" : sprintf("%.1f%%",$val*100)`
    FullOrPercent,
    /// `$val == 1 ? "1 Second" : sprintf("%.0f Seconds",$val)`
    Seconds,
    /// `PrintExposureTime($val)`
    ExposureTime,
    /// `PrintFraction($val)`
    Fraction,
    /// `$val ? sprintf("%.2f m",$val) : "inf"`
    MetersOrInf,
    /// `sprintf("0x%02x", $val)`
    Hex2,
    /// `$val == 0 ? "No Delay" : sprintf("%.0f sec",$val)`
    NoDelayOrSeconds,
    /// `$val ? sprintf("%.1f sec",$val/1000) : "Off"`
    SecondsDiv1000OrOff,
    /// `$val > 0 ? sprintf("%.0f", $val) : ""`
    PositiveOrBlank,
    /// `LensFirmwareVersion`: `$val` split into version/release/modification.
    LensFirmwareVersion,
    /// The Z-body `FocusDistance` ladder.
    FocusDistanceZ,
    /// `BlockShotAFResponse` bits.
    BlockShotBits,
    /// `AutoCaptureCriteria` bits, with 255 meaning `All`.
    AutoCaptureCriteriaBits,
    /// `IntervalShooting`, which reads three other data members.
    IntervalShooting,
    /// `FocusShiftShooting`, which reads `FocusShiftNumberShots`.
    FocusShiftShooting,
}

/// A `Hook`, which shifts every later tag in the table.
#[derive(Clone, Copy)]
pub enum Hook {
    None,
    /// `$varSize += n if $$self{FirmwareVersion} and $$self{FirmwareVersion} ge "X"`
    AddIfFirmwareGe(usize, &'static str),
    /// the same, additionally gated on `$$self{Model} =~ /RE/`
    AddIfModelAndFirmwareGe(usize, &'static str, &'static str),
    /// `MenuSettingsZ8v2`: +4 on firmware 02.10, +8 from 3.0 on.
    MenuSettingsZ8v2,
}

/// Where a `SubDirectory` begins.
#[derive(Clone, Copy)]
pub enum SubStart {
    /// A literal `Start` (always 0 here): `$dirStart + $entry`.
    Fixed(u32),
    /// `Start => '$val'`
    Val,
    /// `Start => '$dirStart + $val'`
    DirStartPlusVal,
}

#[derive(Clone, Copy)]
pub struct SubDir {
    pub table: usize,
    pub start: SubStart,
}

pub struct BinTag {
    pub index: i32,
    pub frac: u16,
    pub name: &'static str,
    pub cond: Cond,
    pub fmt: Fmt,
    pub count: u32,
    pub mask: u32,
    pub shift: u32,
    pub raw: Raw,
    pub filter: Filter,
    pub vc: Vc,
    pub pc: Pc,
    pub hook: Hook,
    pub print_hex: bool,
    pub unknown: bool,
    /// ExifTool's `Priority => 0`: never displace a value already found under
    /// this name.
    pub low_priority: bool,
    pub subdir: Option<SubDir>,
}

pub struct BinTable {
    pub name: &'static str,
    pub increment: u8,
    pub nikon_offsets: Option<u32>,
    pub tags: &'static [BinTag],
}

/// The decryption parameters attached to a `Nikon::Main` sub-directory.
#[derive(Clone, Copy)]
pub struct Encrypted {
    pub table: usize,
    pub decrypt_start: usize,
    pub dir_offset: usize,
    /// `Some(true)` big-endian, `Some(false)` little-endian, `None` inherits
    /// the MakerNote's own order.
    pub byte_order: Option<bool>,
}

/// One `Condition` variant of `Nikon::Main` 0x0091 / 0x0097 / 0x0098.
pub struct Root {
    pub name: &'static str,
    /// Anchored regex matched against the block's leading version bytes.
    pub version_re: &'static str,
    /// `$1 < n` guard (`ColorBalance02`).
    pub cap_lt: Option<u32>,
    /// `$count == ...`; empty means any count.
    pub counts: &'static [u32],
    pub encrypted: Option<Encrypted>,
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
    let mut seen_exp = false;
    while end < bytes.len() {
        let c = bytes[end] as char;
        match c {
            '+' | '-' if end == 0 => {}
            '+' | '-' if seen_exp && matches!(bytes[end - 1], b'e' | b'E') => {}
            '0'..='9' => seen_digit = true,
            '.' if !seen_dot && !seen_exp => seen_dot = true,
            'e' | 'E' if seen_digit && !seen_exp => seen_exp = true,
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
        // Trim the mantissa's trailing zeros, then normalise the exponent to
        // Perl's e+NN / e-NN form.
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

// ===========================================================================
// Evaluation context
// ===========================================================================

/// The `$$self{...}` state shared by every Nikon directory in one file.
#[derive(Default)]
pub struct Ctx {
    members: HashMap<Dm, Scalar>,
    value_forms: HashMap<String, String>,
    pub model: Option<String>,
    /// `$$self{FILE_TYPE}` -- "JPEG" for JPEGs, "TIFF" for NEF and TIFF.
    ///
    /// The MakerNote parser interface carries no file type, so this is `None`
    /// in practice and [`Cond::FileType`] is treated as false. That costs the
    /// three tags ExifTool gates on it (`AEBracketingSteps`,
    /// `WBBracketingSteps`, `PhotoShootingMenuBank`) rather than risk emitting
    /// them on the format where ExifTool suppresses them.
    pub file_type: Option<&'static str>,
}

impl Ctx {
    pub fn new(model: Option<&str>, file_type: Option<&'static str>) -> Self {
        Ctx {
            members: HashMap::new(),
            value_forms: HashMap::new(),
            model: model.map(str::to_string),
            file_type,
        }
    }

    pub fn set(&mut self, dm: Dm, value: Scalar) {
        self.members.insert(dm, value);
    }

    pub fn get(&self, dm: Dm) -> Option<&Scalar> {
        self.members.get(&dm)
    }

    pub fn set_value_form(&mut self, key: String, value: String) {
        self.value_forms.insert(key, value);
    }

    pub fn take_value_forms(&mut self) -> HashMap<String, String> {
        std::mem::take(&mut self.value_forms)
    }

    fn num(&self, dm: Dm) -> f64 {
        self.members.get(&dm).map_or(0.0, Scalar::num)
    }

    fn text(&self, dm: Dm) -> String {
        self.members.get(&dm).map_or(String::new(), Scalar::text)
    }

    fn truthy(&self, dm: Dm) -> bool {
        self.members.get(&dm).is_some_and(Scalar::truthy)
    }
}

static RE_CACHE: Lazy<std::sync::Mutex<HashMap<(&'static str, bool), Option<Regex>>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

/// Compile a Perl regex from an ExifTool Condition. Every pattern in these
/// tables is plain enough that the `regex` crate accepts it verbatim.
fn matches(pattern: &'static str, case_insensitive: bool, subject: &str) -> bool {
    let mut cache = match RE_CACHE.lock() {
        Ok(c) => c,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entry = cache.entry((pattern, case_insensitive)).or_insert_with(|| {
        let src = if case_insensitive {
            format!("(?i){pattern}")
        } else {
            pattern.to_string()
        };
        Regex::new(&src).ok()
    });
    entry.as_ref().is_some_and(|re| re.is_match(subject))
}

fn num_cmp(a: f64, op: NumCmp, b: f64) -> bool {
    match op {
        NumCmp::Eq => a == b,
        NumCmp::Ne => a != b,
        NumCmp::Ge => a >= b,
        NumCmp::Le => a <= b,
        NumCmp::Gt => a > b,
        NumCmp::Lt => a < b,
    }
}

fn str_cmp(a: &str, op: StrCmp, b: &str) -> bool {
    match op {
        StrCmp::Ge => a >= b,
        StrCmp::Le => a <= b,
        StrCmp::Gt => a > b,
        StrCmp::Lt => a < b,
    }
}

fn cond_holds(cond: &Cond, ctx: &Ctx) -> bool {
    let model = ctx.model.as_deref().unwrap_or("");
    match *cond {
        Cond::Always => true,
        Cond::FileType(eq, want) => ctx.file_type.is_some_and(|ft| (ft == want) == eq),
        Cond::Model(want, re, ci) => matches(re, ci, model) == want,
        Cond::ModelEq(want) => model == want,
        Cond::ModelEqAndFirmwareGe(m, fw) => {
            model == m && str_cmp(&ctx.text(Dm::FirmwareVersion), StrCmp::Ge, fw)
        }
        Cond::ModelAndFirmware(want, re, ci, op, fw) => {
            matches(re, ci, model) == want
                && ctx.truthy(Dm::FirmwareVersion)
                && str_cmp(&ctx.text(Dm::FirmwareVersion), op, fw)
        }
        Cond::FirmwareCmp(op, fw, guarded) => {
            (!guarded || ctx.truthy(Dm::FirmwareVersion))
                && str_cmp(&ctx.text(Dm::FirmwareVersion), op, fw)
        }
        Cond::FirmwareEq(fw, eq) => (ctx.text(Dm::FirmwareVersion) == fw) == eq,
        Cond::FirmwareRe(re, want) => matches(re, false, &ctx.text(Dm::FirmwareVersion)) == want,
        Cond::Truthy(dm) => ctx.truthy(dm),
        Cond::Num(dm, op, n) => num_cmp(ctx.num(dm), op, n),
        Cond::StrEq(dm, s, eq) => (ctx.text(dm) == s) == eq,
        Cond::TruthyNum(dm, op, n) => ctx.truthy(dm) && num_cmp(ctx.num(dm), op, n),
        Cond::TruthyStrNe(dm, s) => ctx.truthy(dm) && ctx.text(dm) != s,
        Cond::TruthyStrNeAndNum(a, s, b, op, n) => {
            ctx.truthy(a) && ctx.text(a) != s && num_cmp(ctx.num(b), op, n)
        }
        Cond::TruthyNumAndStrNe(a, op, n, b, s) => {
            ctx.truthy(a) && num_cmp(ctx.num(a), op, n) && ctx.text(b) != s
        }
    }
}

// ===========================================================================
// Value reading
// ===========================================================================

fn read_scalar(data: &[u8], at: usize, fmt: Fmt, big: bool) -> Option<f64> {
    let size = fmt.size();
    let bytes = data.get(at..at + size)?;
    let raw_u32 = |b: &[u8]| -> u32 {
        let mut v: u32 = 0;
        if big {
            for &x in b {
                v = (v << 8) | u32::from(x);
            }
        } else {
            for &x in b.iter().rev() {
                v = (v << 8) | u32::from(x);
            }
        }
        v
    };
    Some(match fmt {
        Fmt::U8 | Fmt::Default | Fmt::Str | Fmt::Undef => f64::from(bytes[0]),
        Fmt::I8 => f64::from(bytes[0] as i8),
        Fmt::U16 => f64::from(raw_u32(bytes) as u16),
        Fmt::I16 => f64::from(raw_u32(bytes) as u16 as i16),
        Fmt::U32 => f64::from(raw_u32(bytes)),
        Fmt::I32 => f64::from(raw_u32(bytes) as i32),
        Fmt::Fixed32u => f64::from(raw_u32(bytes)) / 65536.0,
    })
}

/// ExifTool's `ReadValue`: `count` elements joined by a space, or the raw
/// bytes (truncated at the first NUL for `string`).
fn read_value(data: &[u8], at: usize, fmt: Fmt, count: usize, avail: usize) -> Option<Scalar> {
    match fmt {
        Fmt::Str | Fmt::Undef => {
            let n = count.min(avail);
            let bytes = data.get(at..at + n)?;
            let bytes = if fmt == Fmt::Str {
                let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                &bytes[..end]
            } else {
                bytes
            };
            Some(Scalar::Text(
                bytes.iter().map(|&b| b as char).collect::<String>(),
            ))
        }
        _ => None,
    }
}

// ===========================================================================
// Conversions
// ===========================================================================

fn apply_value_conv(vc: Vc, val: &Scalar, ctx: &Ctx, big: bool) -> Option<Scalar> {
    let v = val.num();
    Some(match vc {
        Vc::None => val.clone(),
        Vc::Pow2Div(n) => Scalar::Num(2f64.powf(v / n)),
        Vc::Pow2Div384Minus1 => Scalar::Num(2f64.powf(v / 384.0 - 1.0)),
        Vc::FiveTimesPow2Div24 => Scalar::Num(5.0 * 2f64.powf(v / 24.0)),
        Vc::Pow2SubDiv(a, b) => Scalar::Num(2f64.powf((v - a) / b)),
        Vc::Nibble4Div6 => Scalar::Num(if v > 7.0 { v - 16.0 } else { v } / 6.0),
        Vc::Pow2NegDiv(n) => Scalar::Num(2f64.powf(-v / n)),
        Vc::Pow2NegSub(n) => Scalar::Num(2f64.powf(-v - n)),
        Vc::Pow2Sub(n) => Scalar::Num(2f64.powf(v - n)),
        Vc::SubDiv(a, b) => Scalar::Num((v - a) / b),
        Vc::Div(n) => Scalar::Num(v / n),
        Vc::NegDiv(n) => Scalar::Num(-v / n),
        Vc::Add(n) => Scalar::Num(v + n),
        Vc::Iso100Exp => Scalar::Num(100.0 * ((v / 12.0 - 5.0) * 2f64.ln()).exp()),
        Vc::Pow10Div40 => Scalar::Num(0.01 * 10f64.powf(v / 40.0)),
        Vc::Recip2048 => Scalar::Num(if v != 0.0 { 2048.0 / v } else { v }),
        Vc::Signed360 => Scalar::Num(if v <= 180.0 { v } else { v - 360.0 }),
        // `$val eq -1` is a *string* comparison against "-1".
        Vc::NoLimit => {
            if val.text() == "-1" {
                Scalar::Text("No Limit".to_string())
            } else {
                val.clone()
            }
        }
        Vc::SmallOrScaled(a, b) => Scalar::Num(if v < 10.0 { v + 1.0 } else { a * (v - b) }),
        Vc::D3bShutterSpeed => {
            // Perl's % on a negative left operand still returns a
            // non-negative result, which is what rem_euclid gives.
            let t = (v - 16.0).rem_euclid(24.0);
            Scalar::Num(if t != 0.0 {
                v / 24.0
            } else {
                2.0 + (v - 16.0) / 24.0
            })
        }
        Vc::SingleFrameOrFive => {
            if ctx.num(Dm::SingleFrame) == 0.0 {
                Scalar::Num(5.0)
            } else {
                val.clone()
            }
        }
        Vc::UnpackBigEndian16 => {
            let _ = big;
            let bytes: Vec<u8> = val.text().chars().map(|c| c as u8).collect();
            if bytes.len() < 2 {
                return None;
            }
            Scalar::Num(f64::from(u16::from_be_bytes([bytes[0], bytes[1]])))
        }
    })
}

/// `Image::ExifTool::Exif::PrintExposureTime`.
fn print_exposure_time(secs: f64) -> String {
    if secs < 0.25001 && secs > 0.0 {
        let inv = (1.0 / secs).round();
        return format!("1/{}", perl_num_to_string(inv));
    }
    let s = format!("{secs:.1}");
    if let Some(stripped) = s.strip_suffix(".0") {
        return stripped.to_string();
    }
    s
}

/// `Image::ExifTool::Exif::PrintFraction`.
fn print_fraction(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else if (v * 2.0).fract() == 0.0 {
        format!("{:+.0}/2", v * 2.0)
    } else if (v * 3.0).fract() == 0.0 {
        format!("{:+.0}/3", v * 3.0)
    } else {
        format!("{:+.3}", v)
    }
}

fn decode_bits(value: f64, lookup: &[(u32, &str)]) -> String {
    let bits = value as u64;
    let mut out: Vec<String> = Vec::new();
    for i in 0..32u32 {
        if bits & (1 << i) == 0 {
            continue;
        }
        match lookup.iter().find(|(n, _)| *n == i) {
            Some((_, label)) => out.push((*label).to_string()),
            None => out.push(format!("[{i}]")),
        }
    }
    if out.is_empty() {
        "(none)".to_string()
    } else {
        out.join(", ")
    }
}

fn map_lookup(map: &[(&str, &str)], key: &str, val: &Scalar, print_hex: bool) -> Option<String> {
    if let Some((_, v)) = map.iter().find(|(k, _)| *k == key) {
        return Some((*v).to_string());
    }
    let n = val.num();
    if print_hex && n.fract() == 0.0 && n >= 0.0 {
        return Some(format!("Unknown (0x{:x})", n as u64));
    }
    Some(format!("Unknown ({key})"))
}

fn apply_print_conv(tag: &BinTag, val: &Scalar, ctx: &Ctx) -> Option<String> {
    let v = val.num();
    let text = val.text();
    Some(match tag.pc {
        Pc::None => text,
        Pc::Map(map) => map_lookup(map, &text, val, tag.print_hex)?,
        Pc::Bitmask(map, bits) => match map.iter().find(|(k, _)| *k == text) {
            Some((_, s)) => (*s).to_string(),
            None => decode_bits(v, bits),
        },
        Pc::MmDiv(n) => format!("{:.1}mm", v / n),
        Pc::FNumberDiv100 => format!("f/{:.1}", v / 100.0),
        Pc::SignedOrZero(p) => {
            if v != 0.0 {
                format!("{:+.*}", p, v)
            } else {
                "0".to_string()
            }
        }
        Pc::Signed(p) => format!("{:+.*}", p, v),
        Pc::Suffix(s) => format!("{text}{s}"),
        Pc::Prefix(s) => format!("{s}{text}"),
        Pc::FullOrExposureTime => {
            if v > 0.99 {
                "Full".to_string()
            } else {
                print_exposure_time(v)
            }
        }
        Pc::Fixed(p) => format!("{:.*}", p, v),
        Pc::FixedSuffix(p, s) => format!("{:.*}{}", p, v, s),
        Pc::MetersDiv10 => format!("{:.1} m", v / 10.0),
        Pc::RoundHalfUp => perl_num_to_string((v + 0.5).floor()),
        Pc::FullOrPercent => {
            if v > 0.99 {
                "Full".to_string()
            } else {
                format!("{:.1}%", v * 100.0)
            }
        }
        Pc::Seconds => {
            if v == 1.0 {
                "1 Second".to_string()
            } else {
                format!("{v:.0} Seconds")
            }
        }
        Pc::ExposureTime => print_exposure_time(v),
        Pc::Fraction => print_fraction(v),
        Pc::MetersOrInf => {
            if v != 0.0 {
                format!("{v:.2} m")
            } else {
                "inf".to_string()
            }
        }
        Pc::Hex2 => format!("0x{:02x}", v as i64),
        Pc::NoDelayOrSeconds => {
            if v == 0.0 {
                "No Delay".to_string()
            } else {
                format!("{v:.0} sec")
            }
        }
        Pc::SecondsDiv1000OrOff => {
            if v != 0.0 {
                format!("{:.1} sec", v / 1000.0)
            } else {
                "Off".to_string()
            }
        }
        Pc::PositiveOrBlank => {
            if v > 0.0 {
                format!("{v:.0}")
            } else {
                String::new()
            }
        }
        Pc::LensFirmwareVersion => {
            let version = (v / 256.0).floor();
            let release = ((v - 256.0 * version) / 16.0).floor();
            let modification = v - (256.0 * version + 16.0 * release);
            format!("{version:.0}.{release:.0}.{modification:.0}")
        }
        Pc::FocusDistanceZ => {
            if ctx.get(Dm::FocusStepsFromInfinity).is_some()
                && ctx.text(Dm::FocusStepsFromInfinity) == "0"
            {
                "Inf".to_string()
            } else if v < 100.0 {
                if v < 10.0 {
                    if v < 1.0 {
                        if v < 0.35 {
                            format!("{v:.4} m")
                        } else {
                            format!("{v:.3} m")
                        }
                    } else {
                        format!("{v:.2} m")
                    }
                } else {
                    format!("{v:.1} m")
                }
            } else {
                format!("{v:.0} m")
            }
        }
        Pc::BlockShotBits => decode_bits(
            v,
            &[(0, "Distance"), (1, "Motion"), (2, "Subject Detection")],
        ),
        Pc::AutoCaptureCriteriaBits => {
            if text == "255" {
                "All".to_string()
            } else {
                decode_bits(
                    v,
                    &[
                        (0, "Top Left"),
                        (1, "Top Right"),
                        (2, "Bottom Left"),
                        (3, "Bottom Right"),
                        (4, "Left"),
                        (5, "Right"),
                        (6, "Top Center"),
                        (7, "Bottom Center"),
                    ],
                )
            }
        }
        Pc::IntervalShooting => {
            if v == 0.0 {
                "Off".to_string()
            } else {
                let intervals = ctx.num(Dm::IntervalShootingIntervals);
                let per = ctx.num(Dm::IntervalShootingShotsPerInterval);
                let frame = ctx.num(Dm::IntervalFrame);
                let mut s = format!("On: Interval {v:.0} of {intervals:.0}");
                if per > 1.0 {
                    s.push_str(&format!(" Frame {frame:.0} of {per:.0}"));
                }
                s
            }
        }
        Pc::FocusShiftShooting => {
            if v == 0.0 {
                "Off".to_string()
            } else if ctx.get(Dm::PixelShiftActive).is_some()
                && ctx.text(Dm::PixelShiftActive) == "1"
            {
                format!("On: Frame {v:.0}")
            } else {
                let shots = ctx.num(Dm::FocusShiftNumberShots);
                format!("On: Frame {v:.0} of {shots:.0}")
            }
        }
    })
}

// ===========================================================================
// The walk
// ===========================================================================

/// Sub-directory lengths derived from a `NIKON_OFFSETS` table, keyed by the
/// tag index (which is the position of the offset within the table).
fn nikon_offset_lengths(data: &[u8], offsets_at: usize, big: bool) -> HashMap<i32, usize> {
    let mut out = HashMap::new();
    let Some(count) = read_scalar(data, offsets_at, Fmt::U32, big) else {
        return out;
    };
    let count = count as usize;
    if offsets_at + 4 + count * 4 > data.len() {
        return out;
    }
    let mut entries: Vec<(usize, usize)> = Vec::new();
    for i in 0..count {
        let pos = offsets_at + 4 + 4 * i;
        let Some(off) = read_scalar(data, pos, Fmt::U32, big) else {
            continue;
        };
        if off == 0.0 {
            continue;
        }
        entries.push((pos, off as usize));
    }
    entries.sort_by_key(|&(pos, off)| (off, pos));
    for i in 0..entries.len() {
        let next = entries.get(i + 1).map_or(data.len(), |&(_, off)| off);
        out.insert(entries[i].0 as i32, next.saturating_sub(entries[i].1));
    }
    out
}

/// Walk one binary-data table over `data[dir_start .. dir_start + dir_len]`.
#[allow(clippy::too_many_arguments)]
pub fn process(
    table_idx: usize,
    data: &[u8],
    dir_start: usize,
    dir_len: usize,
    big: bool,
    ctx: &mut Ctx,
    out: &mut HashMap<String, String>,
    depth: u32,
) {
    if depth > 6 || dir_start > data.len() {
        return;
    }
    let table = &TABLES[table_idx];
    let size = dir_len.min(data.len() - dir_start);
    let increment = usize::from(table.increment);

    let sub_lens = table
        .nikon_offsets
        .map(|at| nikon_offset_lengths(data, dir_start + at as usize, big))
        .unwrap_or_default();

    let mut var_size: isize = 0;
    let mut i = 0usize;
    while i < table.tags.len() {
        // Gather the Condition variants of this id and take the first that holds.
        let id = (table.tags[i].index, table.tags[i].frac);
        let mut j = i;
        while j < table.tags.len() && (table.tags[j].index, table.tags[j].frac) == id {
            j += 1;
        }
        let chosen = table.tags[i..j].iter().find(|t| cond_holds(&t.cond, ctx));
        i = j;
        let Some(tag) = chosen else { continue };

        let entry_rel = tag.index as isize * increment as isize + var_size;
        if entry_rel < 0 {
            continue;
        }
        let entry = entry_rel as usize;
        if entry >= size {
            // `last if $more <= 0` -- the rest of the table is past the data.
            break;
        }
        let more = size - entry;
        let at = dir_start + entry;

        let fmt = if tag.fmt == Fmt::Default {
            match increment {
                2 => Fmt::U16,
                4 => Fmt::U32,
                _ => Fmt::U8,
            }
        } else {
            tag.fmt
        };
        let count = if tag.count == 0 {
            more
        } else {
            tag.count as usize
        };

        // Hook -- shifts every later tag in this table.
        match tag.hook {
            Hook::None => {}
            Hook::AddIfFirmwareGe(n, fw) => {
                if ctx.truthy(Dm::FirmwareVersion)
                    && str_cmp(&ctx.text(Dm::FirmwareVersion), StrCmp::Ge, fw)
                {
                    var_size += n as isize;
                }
            }
            Hook::AddIfModelAndFirmwareGe(n, re, fw) => {
                let model = ctx.model.clone().unwrap_or_default();
                if matches(re, false, &model)
                    && ctx.truthy(Dm::FirmwareVersion)
                    && str_cmp(&ctx.text(Dm::FirmwareVersion), StrCmp::Ge, fw)
                {
                    var_size += n as isize;
                }
            }
            Hook::MenuSettingsZ8v2 => {
                if ctx.truthy(Dm::FirmwareVersion) {
                    let fw = ctx.text(Dm::FirmwareVersion);
                    if fw.starts_with("02.10") {
                        var_size += 4;
                    } else if str_cmp(&fw, StrCmp::Ge, "3.0") {
                        var_size += 8;
                    }
                }
            }
        }

        // Read the value.
        let mut val = match fmt {
            Fmt::Str | Fmt::Undef => {
                let Some(v) = read_value(data, at, fmt, count, more) else {
                    continue;
                };
                v
            }
            _ => {
                let elems = count.min(more / fmt.size());
                if elems == 0 {
                    continue;
                }
                if elems == 1 {
                    let Some(n) = read_scalar(data, at, fmt, big) else {
                        continue;
                    };
                    Scalar::Num(n)
                } else {
                    let mut parts = Vec::with_capacity(elems);
                    for k in 0..elems {
                        let Some(n) = read_scalar(data, at + k * fmt.size(), fmt, big) else {
                            break;
                        };
                        parts.push(perl_num_to_string(n));
                    }
                    Scalar::Text(parts.join(" "))
                }
            }
        };
        if tag.mask != 0 {
            let masked = (val.num() as i64 as u64 & u64::from(tag.mask)) >> tag.shift;
            val = Scalar::Num(masked as f64);
        }

        // SubDirectory: recurse instead of producing a value.
        if let Some(sub) = tag.subdir {
            let (sub_start, sub_len) = match sub.start {
                // `$len = $count * $formatSize{$format}` when the tag names a
                // Format, and the rest of the block when it does not. That is
                // what stops `CustomSettingsD3`'s `undef[17]` from running on
                // past its 17 bytes and inventing a PlaybackMonitorOffTime.
                SubStart::Fixed(s) => {
                    let len = if tag.fmt == Fmt::Default {
                        more
                    } else {
                        (count * fmt.size()).min(more)
                    };
                    (dir_start + entry + s as usize, len)
                }
                SubStart::Val | SubStart::DirStartPlusVal => {
                    if !val.truthy() {
                        continue;
                    }
                    let base = if matches!(sub.start, SubStart::DirStartPlusVal) {
                        dir_start
                    } else {
                        0
                    };
                    let start = base + val.num().max(0.0) as usize;
                    if start < dir_start || start > data.len() {
                        continue;
                    }
                    let avail = data.len() - start;
                    let len = sub_lens
                        .get(&tag.index)
                        .copied()
                        .filter(|&l| l > 0 && l <= avail)
                        .unwrap_or(avail);
                    (start, len)
                }
            };
            process(
                sub.table,
                data,
                sub_start,
                sub_len,
                big,
                ctx,
                out,
                depth + 1,
            );
            continue;
        }

        if tag.unknown {
            continue;
        }

        // RawConv.
        match tag.raw {
            Raw::None => {}
            Raw::Store(dm) => ctx.set(dm, val.clone()),
            Raw::Div256 => val = Scalar::Num(val.num() / 256.0),
            Raw::NonZero => {
                if !val.truthy() {
                    continue;
                }
            }
            Raw::FirmwareLike => {
                if !firmware_like(&val.text()) {
                    continue;
                }
            }
            Raw::StoreFlagIfNotPadding(dm) => {
                if !is_single_byte_then_nuls(&val.text()) {
                    ctx.set(dm, Scalar::Num(1.0));
                }
                continue; // the RawConv returns undef, so no tag is stored
            }
            Raw::StoreScaled(dm, n) => {
                ctx.set(dm, Scalar::Num(1.0 + (val.num() / n).floor()));
            }
        }
        if matches!(tag.filter, Filter::DigitsOnly) {
            let t = val.text();
            if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
        }

        let Some(converted) = apply_value_conv(tag.vc, &val, ctx, big) else {
            continue;
        };
        let Some(printed) = apply_print_conv(tag, &converted, ctx) else {
            continue;
        };
        // ExifTool's `FoundTag`: a later tag of equal-or-higher priority
        // becomes the reported one, which is why `NikonCustom`'s copies of
        // `CLModeShootingSpeed`, `PreviewButton` and friends displace
        // `NikonSettings`'. `Priority => 0` never displaces anything.
        let key = format!("Nikon:{}", tag.name);
        if tag.low_priority {
            if let std::collections::hash_map::Entry::Vacant(entry) = out.entry(key.clone()) {
                entry.insert(printed);
                if tag.name == "FocusDistance" {
                    ctx.set_value_form(key, converted.text());
                }
            }
        } else {
            out.insert(key.clone(), printed);
            if tag.name == "FocusDistance" {
                ctx.set_value_form(key, converted.text());
            }
        }
    }
}

/// `$val =~ /^\d\.\d+.$/`
fn firmware_like(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 4 {
        return false;
    }
    if !b[0].is_ascii_digit() || b[1] != b'.' {
        return false;
    }
    // one or more digits, then exactly one more character (any, but not a
    // newline -- Perl's `.` without /s)
    let tail = &b[2..b.len() - 1];
    !tail.is_empty() && tail.iter().all(u8::is_ascii_digit) && b[b.len() - 1] != b'\n'
}

/// `$val =~ /^.\0+$/s` -- one byte followed only by NULs.
fn is_single_byte_then_nuls(s: &str) -> bool {
    let b: Vec<u8> = s.chars().map(|c| c as u8).collect();
    b.len() >= 2 && b[1..].iter().all(|&x| x == 0)
}

/// Pick the `Nikon::Main` variant for an encrypted block, ExifTool-style: the
/// first `Condition` that matches the value's leading bytes and count.
pub fn select_root(roots: &'static [Root], value: &[u8], count: usize) -> Option<&'static Root> {
    let head: String = value
        .iter()
        .take(8)
        .map(|&b| if b == 0 { '\u{0}' } else { b as char })
        .collect();
    roots.iter().find(|r| {
        if !r.counts.is_empty() && !r.counts.contains(&(count as u32)) {
            return false;
        }
        if r.version_re == "^" {
            return true;
        }
        let mut cache = match RE_CACHE.lock() {
            Ok(c) => c,
            Err(poisoned) => poisoned.into_inner(),
        };
        let re = cache
            .entry((r.version_re, false))
            .or_insert_with(|| Regex::new(r.version_re).ok());
        let Some(re) = re.as_ref() else {
            return false;
        };
        match re.captures(&head) {
            None => false,
            Some(caps) => match r.cap_lt {
                None => true,
                Some(limit) => caps
                    .get(1)
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .is_some_and(|n| n < limit),
            },
        }
    })
}

/// Look up a table index by name (used by the ShotInfo dispatcher's tests).
pub fn table_index(name: &str) -> Option<usize> {
    TABLES.iter().position(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perl_number_stringification() {
        assert_eq!(perl_num_to_string(0.0), "0");
        assert_eq!(perl_num_to_string(2.0), "2");
        assert_eq!(perl_num_to_string(-3.0), "-3");
        assert_eq!(perl_num_to_string(0.5), "0.5");
        assert_eq!(perl_num_to_string(37.5), "37.5");
        assert_eq!(perl_num_to_string(1.0 / 3.0), "0.333333333333333");
        assert_eq!(perl_num_to_string(2048.0 / 3.0), "682.666666666667");
    }

    #[test]
    fn perl_numification_is_leading_prefix() {
        assert_eq!(perl_numify("12abc"), 12.0);
        assert_eq!(perl_numify("abc"), 0.0);
        assert_eq!(perl_numify("-2.5"), -2.5);
        assert_eq!(perl_numify("0204"), 204.0);
    }

    #[test]
    fn exposure_time_matches_exiftool() {
        assert_eq!(print_exposure_time(0.004), "1/250");
        assert_eq!(print_exposure_time(0.25), "1/4");
        assert_eq!(print_exposure_time(0.5), "0.5");
        assert_eq!(print_exposure_time(2.0), "2");
        assert_eq!(print_exposure_time(1.3), "1.3");
    }

    #[test]
    fn decode_bits_names_unknown_positions() {
        assert_eq!(decode_bits(0.0, &[(0, "A")]), "(none)");
        assert_eq!(decode_bits(5.0, &[(0, "A"), (2, "C")]), "A, C");
        assert_eq!(decode_bits(2.0, &[(0, "A")]), "[1]");
    }

    #[test]
    fn every_subdir_index_is_in_range() {
        for table in TABLES {
            for tag in table.tags {
                if let Some(sub) = tag.subdir {
                    assert!(
                        sub.table < TABLES.len(),
                        "{}::{} points at table {}",
                        table.name,
                        tag.name,
                        sub.table
                    );
                }
            }
        }
    }

    #[test]
    fn tags_are_in_ascending_index_order() {
        for table in TABLES {
            let mut last = (i32::MIN, 0u16);
            for tag in table.tags {
                let key = (tag.index, tag.frac);
                assert!(
                    key >= last,
                    "{} is out of order at {}",
                    table.name,
                    tag.name
                );
                last = key;
            }
        }
    }
}
