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
//! `Ctx` is not replaced here. This is the first slice of v2 step 1: the
//! type, its Perl scalar semantics, and the helper library
//! ([`super::helpers`]) that reads it. Nothing in the read path constructs a
//! `Session` yet, so no output changes.
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
    warnings: Vec<MemberVal>,
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
/// `CharsetEXIF`, `CharsetFileName`). `CharsetRIFF` and `SystemTimeRes`
/// default to the integer 0 (see [`Session::new`]). The helper oracle
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
const DEFAULT_ZERO_OPTIONS: &[&str] = &["CharsetRIFF", "SystemTimeRes"];

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
        }
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

    /// `$$self{key} = value`. `Make`/`Model` are strings (or `undef`, which
    /// clears them); any other type for them is a [`TypeMismatch`].
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
                return Err(TypeMismatch {
                    key: key.to_string(),
                    expected: "Str",
                });
            }
        }
        Ok(())
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
        self.warnings.push(message);
    }

    /// Every `Warn` request recorded so far, oldest first.
    #[must_use]
    pub fn warnings(&self) -> &[MemberVal] {
        &self.warnings
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
}
