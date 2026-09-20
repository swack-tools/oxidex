//! `Session`: the `$self` an ExifTool conversion reads (Autogeneration v2,
//! `docs/AUTOGENERATION-V2-DESIGN.md` section 2).
//!
//! # Why this exists
//!
//! ExifTool evaluates every `Condition`, `RawConv`, `ValueConv` and
//! `PrintConv` with its per-file object in scope as `$self`/`$et`. The
//! runtime [`super::cond::Ctx`] carries only same-table members, `$$valPt`,
//! `$format` and `$count`, so an expression that reads `$$self{Model}` (734
//! uses in the 13.59 dump), `$$self{OPTIONS}{...}` or another directory's
//! state cannot be generated through it, however capable the translator.
//! `Session` is that object: a few typed fields for the high-traffic keys,
//! plus a map of Perl scalars for the rest, and the ExifTool options a helper
//! sub reads.
//!
//! `Ctx` is not replaced here. The file-level reader owns one `Session` and
//! each IFD walk enters a [`DirectoryScope`] for the generated conversion
//! arms (`conv`), which the helper library ([`super::helpers`]) reads and
//! mutates as ExifTool's subs mutate `$self`.
//!
//! # Perl scalar semantics
//!
//! A `$$self{...}` slot is a dynamically typed Perl scalar, modelled as
//! [`MemberVal`]. Its three contexts are methods, never Rust coercions:
//!
//! - [`MemberVal::is_truthy`] -- boolean context. False is exactly `undef`,
//!   `""`, `"0"` and a numeric zero (including `-0.0`); `"0.0"`, `"00"`,
//!   `"0E0"`, `" 0"`, `"0\n"` and `NaN` are TRUE. Pinned against the pinned
//!   perl 5.38.2 by `tests::truthiness_matches_the_pinned_perl_capture`.
//! - [`MemberVal::perl_string`] -- string context: an integer prints its
//!   digits, a float `%.15g` (`-0.0` prints `0`, infinities `Inf`/`-Inf`),
//!   `undef` prints the empty string.
//! - [`MemberVal::perl_num`] -- numeric context, Perl's `grok_number`: leading
//!   whitespace, a sign, digits, a fraction and an exponent, then anything
//!   else ignored (`"12abc"` is 12, `"0x1A"` is 0, `"1e"` is 1); `inf`,
//!   `infinity` and `nan` in any case; nothing numeric at all is 0.
//!
//! # Byte strings
//!
//! A Perl scalar is a BYTE string; a Rust `String` is UTF-8 text. A value
//! that is not valid UTF-8 (a UCS-2 `XPTitle`, a Latin-1 `UserComment`, the
//! output of `Decode` into a 1-byte charset) is [`MemberVal::Bytes`]. Build
//! one with [`MemberVal::from_bytes`], which keeps valid UTF-8 as
//! [`MemberVal::Str`] so that one Perl string has one representation;
//! `Str` and `Bytes` holding the same bytes also compare equal. Every
//! context above is defined on the bytes, and [`MemberVal::perl_bytes`] is
//! the exact string context for both variants.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use super::engine::Guard;
use super::exprs::perl_num;

/// One Perl scalar: a `$$self{...}` data member, an ExifTool option, or a
/// helper argument/result. `Bool` is Perl's `PL_sv_yes`/`PL_sv_no` (what a
/// comparison or `IsInt` returns): it prints `"1"`/`""`. `Str` is a byte
/// string that is valid UTF-8, `Bytes` one that is not (see the module doc).
#[derive(Clone, Debug)]
pub enum MemberVal {
    Str(String),
    Bytes(Vec<u8>),
    Int(i64),
    Float(f64),
    Bool(bool),
    Undef,
}

/// Structural equality, except that `Str` and `Bytes` are one Perl type and
/// compare by their bytes (a `Bytes` built directly rather than through
/// [`MemberVal::from_bytes`] may hold valid UTF-8).
impl PartialEq for MemberVal {
    fn eq(&self, other: &Self) -> bool {
        use MemberVal::{Bool, Bytes, Float, Int, Str, Undef};
        match (self, other) {
            (Str(_) | Bytes(_), Str(_) | Bytes(_)) => self.perl_bytes() == other.perl_bytes(),
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Bool(a), Bool(b)) => a == b,
            (Undef, Undef) => true,
            _ => false,
        }
    }
}

/// A scalar in numeric context: Perl keeps an integer (IV) where it can and a
/// double (NV) otherwise; the distinction decides how the result prints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PerlNum {
    Int(i64),
    Float(f64),
}

impl PerlNum {
    #[must_use]
    pub fn as_f64(self) -> f64 {
        match self {
            PerlNum::Int(i) => i as f64,
            PerlNum::Float(f) => f,
        }
    }
}

/// Perl's `isSPACE` for a non-UTF-8 string: ASCII space, `\t`, `\n`, `\r`,
/// `\f` and (since 5.18) `\v`. Not `u8::is_ascii_whitespace`, which omits
/// `\v`.
#[must_use]
pub const fn is_perl_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Numify a string exactly as Perl's `sv_2nv`/`grok_number` does for a
/// scalar that is not already numeric.
#[must_use]
pub fn numify_str(s: &str) -> PerlNum {
    numify_bytes(s.as_bytes())
}

/// [`numify_str`] over a byte string: only an ASCII prefix is ever numeric,
/// so any bytes after it (UTF-8 or not) are ignored exactly as Perl does.
#[must_use]
pub fn numify_bytes(b: &[u8]) -> PerlNum {
    // Every slice taken below is of ASCII digits, signs, points and `e`.
    let ascii = |r: std::ops::Range<usize>| std::str::from_utf8(&b[r]).expect("ASCII");
    let mut i = 0;
    while i < b.len() && is_perl_space(b[i]) {
        i += 1;
    }
    let start = i;
    let neg = if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
        b[i - 1] == b'-'
    } else {
        false
    };
    let lower = b.get(i..i + 3).map(<[u8]>::to_ascii_lowercase);
    if lower.as_deref() == Some(b"inf".as_slice()) {
        return PerlNum::Float(if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if lower.as_deref() == Some(b"nan".as_slice()) {
        return PerlNum::Float(f64::NAN);
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_end = i;
    let mut frac = (i, i);
    let mut has_point = false;
    if i < b.len() && b[i] == b'.' {
        let fs = i + 1;
        let mut j = fs;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if int_end > int_start || j > fs {
            has_point = true;
            frac = (fs, j);
            i = j;
        }
    }
    if int_end == int_start && frac.0 == frac.1 {
        return PerlNum::Int(0);
    }
    let mut exp = None;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let ds = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > ds {
            exp = Some(ascii(i + 1..j));
        }
    }
    let int_digits = ascii(int_start..int_end);
    if !has_point && exp.is_none() {
        if let Ok(v) = ascii(start..int_end).parse::<i64>() {
            return PerlNum::Int(v);
        }
    }
    let text = format!(
        "{}{}.{}e{}",
        if neg { "-" } else { "" },
        if int_digits.is_empty() {
            "0"
        } else {
            int_digits
        },
        if frac.0 == frac.1 {
            "0"
        } else {
            ascii(frac.0..frac.1)
        },
        exp.unwrap_or("0"),
    );
    PerlNum::Float(text.parse().unwrap_or(0.0))
}

impl MemberVal {
    /// A Perl byte string: [`MemberVal::Str`] when the bytes are valid
    /// UTF-8, [`MemberVal::Bytes`] otherwise.
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        match String::from_utf8(bytes) {
            Ok(s) => MemberVal::Str(s),
            Err(e) => MemberVal::Bytes(e.into_bytes()),
        }
    }

    /// Perl boolean context (`if ($x)`, `$x and ...`, `not $x`).
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            MemberVal::Str(s) => !(s.is_empty() || s == "0"),
            MemberVal::Bytes(b) => !(b.is_empty() || b == b"0"),
            MemberVal::Int(i) => *i != 0,
            // NaN != 0.0, so NaN is true -- as in Perl.
            MemberVal::Float(f) => *f != 0.0,
            MemberVal::Bool(b) => *b,
            MemberVal::Undef => false,
        }
    }

    #[must_use]
    pub fn is_defined(&self) -> bool {
        !matches!(self, MemberVal::Undef)
    }

    /// Perl string context (`"$x"`, `eq`, a regex match target) as bytes:
    /// exact for every variant.
    #[must_use]
    pub fn perl_bytes(&self) -> Cow<'_, [u8]> {
        match self {
            MemberVal::Str(s) => Cow::Borrowed(s.as_bytes()),
            MemberVal::Bytes(b) => Cow::Borrowed(b),
            other => Cow::Owned(other.perl_string().into_bytes()),
        }
    }

    /// Perl string context as UTF-8 text. Exact for every variant except a
    /// non-UTF-8 [`MemberVal::Bytes`], which has no `String` form: there the
    /// invalid sequences become U+FFFD, fit for display only. Anything that
    /// must be exact (every helper port) reads [`MemberVal::perl_bytes`].
    #[must_use]
    pub fn perl_string(&self) -> String {
        match self {
            MemberVal::Str(s) => s.clone(),
            MemberVal::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            MemberVal::Int(i) => i.to_string(),
            MemberVal::Float(f) => perl_num(*f),
            MemberVal::Bool(true) => "1".to_string(),
            MemberVal::Bool(false) | MemberVal::Undef => String::new(),
        }
    }

    /// Perl numeric context (`$x + 0`, `$x < 1`).
    #[must_use]
    pub fn perl_num(&self) -> PerlNum {
        match self {
            MemberVal::Str(s) => numify_str(s),
            MemberVal::Bytes(b) => numify_bytes(b),
            MemberVal::Int(i) => PerlNum::Int(*i),
            MemberVal::Float(f) => PerlNum::Float(*f),
            MemberVal::Bool(b) => PerlNum::Int(i64::from(*b)),
            MemberVal::Undef => PerlNum::Int(0),
        }
    }

    /// Perl NV context (`sv_2nv`, what `sprintf("%f")`, a mixed IV/NV
    /// operator or a comparison against a double reads): [`Self::perl_num`]
    /// as a double, except that a STRING spelling a negative zero in integer
    /// form (`"-0"`, `" -00"`, `"-0abc"`) is `-0.0` -- `grok_number` gives the
    /// IV 0 but `Atof` keeps the sign, so `sprintf("%.1f", "-0")` is `-0.0`
    /// while `"-0" + 0` is `0`. A signed rational 0/-1 reaches a conversion as
    /// exactly that string (`RoundFloat` prints `-0`).
    #[must_use]
    pub fn perl_nv(&self) -> f64 {
        match self {
            MemberVal::Str(s) => match numify_str(s) {
                PerlNum::Int(0)
                    if s.trim_start_matches(|c: char| c.is_ascii() && is_perl_space(c as u8))
                        .starts_with('-') =>
                {
                    -0.0
                }
                n => n.as_f64(),
            },
            other => other.perl_num().as_f64(),
        }
    }

    /// `length $x`: `None` for `undef` (Perl's `length undef` is `undef`).
    #[must_use]
    pub fn perl_length(&self) -> Option<usize> {
        match self {
            MemberVal::Undef => None,
            other => Some(other.perl_bytes().len()),
        }
    }
}

impl From<PerlNum> for MemberVal {
    fn from(n: PerlNum) -> Self {
        match n {
            PerlNum::Int(i) => MemberVal::Int(i),
            PerlNum::Float(f) => MemberVal::Float(f),
        }
    }
}

/// `GetByteOrder()`: `'II'` (little-endian) or `'MM'` (big-endian).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrder {
    LittleEndian,
    BigEndian,
}

impl ByteOrder {
    #[must_use]
    pub const fn as_perl(self) -> &'static str {
        match self {
            ByteOrder::LittleEndian => "II",
            ByteOrder::BigEndian => "MM",
        }
    }
}

/// A member key typed on [`Session`] was set with a value of the wrong Perl
/// type. The generator infers one type per key from every use site and
/// refuses a key used inconsistently (design s2); this is the run-time end of
/// the same rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeMismatch {
    pub key: String,
    pub expected: &'static str,
}

/// The ExifTool object (`$self`) as generated conversions and ported helpers
/// see it.
///
/// Typed: `make`/`model` (`$$self{Make}`/`$$self{Model}`, together 851 of the
/// dump's session reads), `byte_order`, and the eval-site lexicals `$count`
/// and `$format`. Everything else is a [`MemberVal`] in `members`, and
/// ExifTool's `OPTIONS` hash is `options`, seeded with the defaults of the
/// options a ported helper reads (ExifTool.pm `@availableOptions`).
#[derive(Clone, Debug)]
pub struct Session {
    pub make: Option<String>,
    pub model: Option<String>,
    pub byte_order: Option<ByteOrder>,
    /// `$count`: the value count of the entry under conversion.
    pub count: Option<i64>,
    /// `$format`: the entry's ExifTool format name (`int16u`, `string`, ...).
    pub format: Option<String>,
    members: HashMap<String, MemberVal>,
    options: HashMap<String, MemberVal>,
    warnings: Vec<Warning>,
    processed: Guard,
}

/// One `ProcessDirectory` frame over a file-scoped [`Session`].
///
/// ExifTool temporarily replaces directory identity and entry-evaluation
/// fields while it walks a directory, then restores their exact prior state.
/// Everything else in the session is deliberately left alone: DataMembers,
/// options, warnings, values and processed-directory state belong to the
/// input file rather than to one IFD. The guard is intentionally not
/// cloneable, so a scope has one LIFO exit and [`Drop`] covers every return
/// path.
pub struct DirectoryScope<'a> {
    session: &'a mut Session,
    saved_dir_name: Option<MemberVal>,
    saved_compression: Option<MemberVal>,
    saved_subfile_type: Option<MemberVal>,
    saved_byte_order: Option<ByteOrder>,
    saved_count: Option<i64>,
    saved_format: Option<String>,
}

impl Deref for DirectoryScope<'_> {
    type Target = Session;

    fn deref(&self) -> &Self::Target {
        self.session
    }
}

impl DerefMut for DirectoryScope<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.session
    }
}

impl Drop for DirectoryScope<'_> {
    fn drop(&mut self) {
        restore_member(self.session, "DIR_NAME", self.saved_dir_name.take());
        restore_member(self.session, "Compression", self.saved_compression.take());
        restore_member(self.session, "SubfileType", self.saved_subfile_type.take());
        self.session.byte_order = self.saved_byte_order;
        self.session.count = self.saved_count;
        self.session.format = self.saved_format.take();
    }
}

fn saved_member(session: &Session, key: &str) -> Option<MemberVal> {
    session.has_member(key).then(|| session.member(key))
}

fn restore_member(session: &mut Session, key: &str, saved: Option<MemberVal>) {
    match saved {
        Some(value) => {
            session
                .set_member(key, value)
                .expect("directory-local members accept every Perl scalar");
        }
        None => session.remove_member(key),
    }
}

/// One `$self->Warn($str [, $ignorable])` request a helper made, recorded
/// as asked: `ignorable` is the call's second argument (0 when absent).
/// What `Warn` itself then does with it -- the `[minor] ` prefix, the
/// `IgnoreMinorErrors`/`NoWarning`/`Validate` options, de-duplication, the
/// `Warning` tag -- belongs to whoever drains the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Warning {
    pub message: MemberVal,
    pub ignorable: i64,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// `@availableOptions` defaults (ExifTool.pm, pinned 13.59) for the options
/// the ported helpers and their call sites read. An option absent here reads
/// as `undef`, which is also its ExifTool default (`CoordFormat`,
/// `DateFormat`, `GlobalTimeShift`, `KeepUTCTime`, `StrictDate`,
/// `CharsetEXIF`, `CharsetFileName`, `Validate`). `CharsetRIFF`,
/// `SystemTimeRes` and `Verbose` default to the integer 0 (see
/// [`Session::new`]). The helper oracle
/// captures `Image::ExifTool->new`'s own values for all of these, and
/// `helpers::tests::session_option_defaults_match_the_pinned_perl` holds
/// this table to them.
const DEFAULT_OPTIONS: &[(&str, &str)] = &[
    ("ByteUnit", "SI"),
    ("Charset", "UTF8"),
    ("CharsetID3", "Latin"),
    ("CharsetIPTC", "Latin"),
    ("CharsetPhotoshop", "Latin"),
    ("CharsetQuickTime", "MacRoman"),
];

/// Options whose `@availableOptions` default is the integer 0.
const DEFAULT_ZERO_OPTIONS: &[&str] = &["CharsetRIFF", "SystemTimeRes", "Verbose"];

/// The scalar members `ExifTool::Init` (ExifTool.pm:4330-4376, pinned 13.59)
/// sets before any file is read, with the value it sets. They are
/// "always set" only in the sense that ExifTool never leaves them undefined:
/// `Make`, `Model`, `CameraType`, `FileType` and `TIFF_TYPE` are then
/// OVERWRITTEN as the file is read (Make's `RawConv`, `DoProcessTIFF`'s
/// `$$self{TIFF_TYPE} = $fileType`, ...), so a reader that does not know the
/// current value must leave the member unsupplied rather than seed this
/// initial one. [`Session::for_ifd`] supplies only what the IFD walk knows.
pub const INIT_MEMBERS: &[(&str, &str)] = &[
    ("TIFF_TYPE", ""),
    ("Make", ""),
    ("Model", ""),
    ("CameraType", ""),
    ("FileType", ""),
    ("PRIORITY_DIR", ""),
];

/// The members `ProcessExif` and `ProcessDirectory` set for EVERY IFD before
/// its first entry is converted: `DIR_NAME` (ExifTool.pm:9078, the
/// directory's own name; `PATH` gains it too, but a Perl array is not a
/// [`MemberVal`]), and `Compression`/`SubfileType`, reset to `''`
/// (Exif.pm:6446-6447 "make sure that Compression and SubfileType are
/// defined for this IFD (for Condition's)"; ProcessDirectory saves and
/// restores all three around the directory, ExifTool.pm:9075/9088).
pub const IFD_MEMBERS: &[&str] = &["DIR_NAME", "Compression", "SubfileType"];

/// Members the file-level readers set before an IFD is walked and never
/// leave undefined afterwards: `TIFF_TYPE` (ExifTool.pm:4369 `''`, then
/// `DoProcessTIFF` 8715) and `FILE_TYPE` (ExifTool.pm:2985/3048). A walk
/// supplies them only when its caller says what they are.
pub const FILE_MEMBERS: &[&str] = &["TIFF_TYPE", "FILE_TYPE"];

impl Session {
    #[must_use]
    pub fn new() -> Self {
        let mut options: HashMap<String, MemberVal> = DEFAULT_OPTIONS
            .iter()
            .map(|(k, v)| ((*k).to_string(), MemberVal::Str((*v).to_string())))
            .collect();
        // ExifTool.pm: [ 'CharsetRIFF', 0, ... ], [ 'SystemTimeRes', 0, ... ]
        for name in DEFAULT_ZERO_OPTIONS {
            options.insert((*name).to_string(), MemberVal::Int(0));
        }
        Self {
            make: None,
            model: None,
            byte_order: None,
            count: None,
            format: None,
            members: HashMap::new(),
            options,
            warnings: Vec::new(),
            processed: Guard::new(),
        }
    }

    /// Enter one IFD directory while retaining the surrounding file state.
    ///
    /// `DIR_NAME`, `Compression`, `SubfileType`, byte order and the current
    /// entry's `$count`/`$format` are directory/evaluation-local. Their exact
    /// prior presence and values are restored when the returned guard drops.
    /// All other members and side effects remain file-scoped.
    pub fn enter_directory(
        &mut self,
        byte_order: ByteOrder,
        dir_name: Option<&str>,
    ) -> DirectoryScope<'_> {
        let saved_dir_name = saved_member(self, "DIR_NAME");
        let saved_compression = saved_member(self, "Compression");
        let saved_subfile_type = saved_member(self, "SubfileType");
        let saved_byte_order = self.byte_order;
        let saved_count = self.count;
        let saved_format = self.format.take();

        self.byte_order = Some(byte_order);
        self.count = None;
        match dir_name {
            Some(name) => {
                self.members
                    .insert("DIR_NAME".to_string(), MemberVal::Str(name.to_string()));
            }
            None => {
                self.members.remove("DIR_NAME");
            }
        }
        for key in ["Compression", "SubfileType"] {
            self.members
                .insert(key.to_string(), MemberVal::Str(String::new()));
        }

        DirectoryScope {
            session: self,
            saved_dir_name,
            saved_compression,
            saved_subfile_type,
            saved_byte_order,
            saved_count,
            saved_format,
        }
    }

    /// The session one `ProcessExif` directory's conversions run in: its
    /// byte order (`GetByteOrder()`), the [`IFD_MEMBERS`] -- `DIR_NAME` when
    /// the walk knows the directory's name, `Compression` and `SubfileType`
    /// reset to `''` -- and whatever per-file members the caller already
    /// knows (`known`: `Make`, `Model`, [`FILE_MEMBERS`], ...), exactly as
    /// given. A member the caller does not know stays unsupplied, so an arm
    /// that reads it declines ([`Session::has_member`]) instead of reading
    /// `Init`'s placeholder.
    #[must_use]
    pub fn for_ifd<'a>(
        byte_order: ByteOrder,
        dir_name: Option<&str>,
        known: impl IntoIterator<Item = (&'a str, MemberVal)>,
    ) -> Self {
        let mut s = Self::new();
        s.byte_order = Some(byte_order);
        for (key, value) in known {
            // A non-UTF-8 Make/Model leaves the typed slot unsupplied.
            let _ = s.set_member(key, value);
        }
        if let Some(name) = dir_name {
            s.members
                .insert("DIR_NAME".to_string(), MemberVal::Str(name.to_string()));
        }
        for key in ["Compression", "SubfileType"] {
            s.members
                .insert(key.to_string(), MemberVal::Str(String::new()));
        }
        s
    }

    /// `$$self{key}`. The typed keys read through their fields; an absent
    /// key is `undef`, as in Perl.
    #[must_use]
    pub fn member(&self, key: &str) -> MemberVal {
        let typed = |v: &Option<String>| v.clone().map_or(MemberVal::Undef, MemberVal::Str);
        match key {
            "Make" => typed(&self.make),
            "Model" => typed(&self.model),
            _ => self.members.get(key).cloned().unwrap_or(MemberVal::Undef),
        }
    }

    /// Whether `$$self{key}` was supplied (set, even to `undef`). A generated
    /// conversion that reads a member the caller never supplied declines
    /// rather than read it as `undef` -- ExifTool always has, e.g.,
    /// `TIFF_TYPE` set, and "absent here" is not "undef there".
    #[must_use]
    pub fn has_member(&self, key: &str) -> bool {
        match key {
            "Make" => self.make.is_some(),
            "Model" => self.model.is_some(),
            _ => self.members.contains_key(key),
        }
    }

    /// `$$self{key} = value`. `Make`/`Model` are UTF-8 strings (or `undef`,
    /// which clears them); any other value for them is a [`TypeMismatch`],
    /// and leaves the slot UNSUPPLIED (never the stale previous value), so a
    /// later read declines.
    pub fn set_member(&mut self, key: &str, value: MemberVal) -> Result<(), TypeMismatch> {
        let slot = match key {
            "Make" => &mut self.make,
            "Model" => &mut self.model,
            _ => {
                self.members.insert(key.to_string(), value);
                return Ok(());
            }
        };
        match value {
            MemberVal::Str(s) => *slot = Some(s),
            MemberVal::Undef => *slot = None,
            _ => {
                *slot = None;
                return Err(TypeMismatch {
                    key: key.to_string(),
                    expected: "Str",
                });
            }
        }
        Ok(())
    }

    /// `delete $$self{key}`: afterwards the member is unsupplied, as it is
    /// absent (not `undef`) in Perl.
    pub fn remove_member(&mut self, key: &str) {
        match key {
            "Make" => self.make = None,
            "Model" => self.model = None,
            _ => {
                self.members.remove(key);
            }
        }
    }

    /// The keys of every untyped member set so far (`Make`/`Model` live in
    /// their fields), in no particular order.
    pub fn member_names(&self) -> impl Iterator<Item = &str> {
        self.members.keys().map(String::as_str)
    }

    /// `$$self{OPTIONS}{name}` (what `$self->Options(name)` returns).
    #[must_use]
    pub fn option(&self, name: &str) -> MemberVal {
        self.options.get(name).cloned().unwrap_or(MemberVal::Undef)
    }

    /// `$$self{OPTIONS}{name} = value`, stored as given. This is NOT
    /// `$self->Options(name, value)`: no alias is resolved (a charset option
    /// must already be canonical, `Latin` not `cp1252`).
    pub fn set_option(&mut self, name: &str, value: MemberVal) {
        self.options.insert(name.to_string(), value);
    }

    /// Record a `$self->Warn($str)` call a helper made (no `$ignorable`
    /// argument). The message is kept exactly as the helper passed it;
    /// `Warn`'s own engine behaviour (`NoWarning`, de-duplication, the
    /// `Warning` tag) belongs to whoever drains the list, not to the port.
    pub fn warn(&mut self, message: MemberVal) {
        self.warn_ignorable(message, 0);
    }

    /// Record a `$self->Warn($str, $ignorable)` call (see [`Warning`]).
    pub fn warn_ignorable(&mut self, message: MemberVal, ignorable: i64) {
        self.warnings.push(Warning { message, ignorable });
    }

    /// Every `Warn` request recorded so far, oldest first.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// The file's `$$self{PROCESSED}` state. It is deliberately not part of
    /// [`DirectoryScope`]'s snapshot: a directory reached once stays reached
    /// when its caller resumes or a later root walk starts.
    pub(super) fn processed(&mut self) -> &mut Guard {
        &mut self.processed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthiness_follows_perl_not_rust() {
        for (v, want) in [
            (MemberVal::Undef, false),
            (MemberVal::Str(String::new()), false),
            (MemberVal::Str("0".into()), false),
            (MemberVal::Str("0.0".into()), true),
            (MemberVal::Str("00".into()), true),
            (MemberVal::Str("0E0".into()), true),
            (MemberVal::Str(" 0".into()), true),
            (MemberVal::Str("0\n".into()), true),
            (MemberVal::Int(0), false),
            (MemberVal::Int(-1), true),
            (MemberVal::Float(0.0), false),
            (MemberVal::Float(-0.0), false),
            (MemberVal::Float(f64::NAN), true),
            (MemberVal::Bool(false), false),
            (MemberVal::Bool(true), true),
        ] {
            assert_eq!(v.is_truthy(), want, "{v:?}");
        }
    }

    #[test]
    fn numify_follows_grok_number() {
        for (s, want) in [
            ("12abc", PerlNum::Int(12)),
            (" 12", PerlNum::Int(12)),
            ("0x1A", PerlNum::Int(0)),
            ("1_000", PerlNum::Int(1)),
            (".5", PerlNum::Float(0.5)),
            ("5.", PerlNum::Float(5.0)),
            ("1e5", PerlNum::Float(100_000.0)),
            ("1e", PerlNum::Int(1)),
            ("+3", PerlNum::Int(3)),
            ("-", PerlNum::Int(0)),
            (".", PerlNum::Int(0)),
            ("abc", PerlNum::Int(0)),
            ("\x0b7", PerlNum::Int(7)),
            ("-inf", PerlNum::Float(f64::NEG_INFINITY)),
            ("Infinity", PerlNum::Float(f64::INFINITY)),
        ] {
            assert_eq!(numify_str(s), want, "{s:?}");
        }
        assert!(matches!(numify_str("nan"), PerlNum::Float(f) if f.is_nan()));
    }

    #[test]
    fn typed_members_route_through_their_fields() {
        let mut s = Session::new();
        assert_eq!(s.member("Model"), MemberVal::Undef);
        s.set_member("Model", MemberVal::Str("EOS R5".into()))
            .unwrap();
        assert_eq!(s.model.as_deref(), Some("EOS R5"));
        assert_eq!(s.member("Model"), MemberVal::Str("EOS R5".into()));
        assert!(s.set_member("Make", MemberVal::Int(1)).is_err());
        s.set_member("FacesDetected", MemberVal::Int(2)).unwrap();
        assert_eq!(s.member("FacesDetected"), MemberVal::Int(2));
        assert_eq!(s.option("ByteUnit"), MemberVal::Str("SI".into()));
        assert_eq!(s.option("DateFormat"), MemberVal::Undef);
    }

    #[test]
    fn a_non_utf8_make_leaves_the_slot_unsupplied_not_stale() {
        let mut s = Session::new();
        s.set_member("Make", MemberVal::Str("Canon".into()))
            .unwrap();
        assert!(
            s.set_member("Make", MemberVal::Bytes(b"Can\xf3n".to_vec()))
                .is_err()
        );
        assert!(!s.has_member("Make"));
        s.set_member("X", MemberVal::Int(1)).unwrap();
        s.remove_member("X");
        assert!(!s.has_member("X"));
        assert_eq!(s.member("X"), MemberVal::Undef);
    }

    #[test]
    fn an_ifd_session_carries_what_process_exif_always_sets() {
        let s = Session::for_ifd(
            ByteOrder::BigEndian,
            Some("ExifIFD"),
            [("TIFF_TYPE", MemberVal::Str("APP1".into()))],
        );
        assert_eq!(s.byte_order, Some(ByteOrder::BigEndian));
        assert_eq!(s.member("DIR_NAME"), MemberVal::Str("ExifIFD".into()));
        for key in ["Compression", "SubfileType"] {
            assert!(s.has_member(key), "{key}");
            assert_eq!(s.member(key), MemberVal::Str(String::new()));
        }
        assert_eq!(s.member("TIFF_TYPE"), MemberVal::Str("APP1".into()));
        // What the walk does not know stays unsupplied.
        for key in ["FILE_TYPE", "Make", "Model", "PRIORITY_DIR"] {
            assert!(!s.has_member(key), "{key}");
        }
        assert!(!Session::for_ifd(ByteOrder::LittleEndian, None, []).has_member("DIR_NAME"));
        for key in IFD_MEMBERS {
            assert!(s.has_member(key), "{key}");
        }
        assert_eq!(INIT_MEMBERS.len(), 6);
        assert_eq!(FILE_MEMBERS, &["TIFF_TYPE", "FILE_TYPE"]);
    }

    #[test]
    fn warn_requests_keep_their_ignorable_argument() {
        let mut s = Session::new();
        s.warn(MemberVal::Str("a".into()));
        s.warn_ignorable(MemberVal::Str("b".into()), 1);
        let got: Vec<(MemberVal, i64)> = s
            .warnings()
            .iter()
            .map(|w| (w.message.clone(), w.ignorable))
            .collect();
        assert_eq!(
            got,
            vec![
                (MemberVal::Str("a".into()), 0),
                (MemberVal::Str("b".into()), 1)
            ]
        );
    }
}
