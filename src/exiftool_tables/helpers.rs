//! Autogeneration v2 helper library (`docs/AUTOGENERATION-V2-DESIGN.md`
//! section 3): ExifTool's helper subs, each ported ONCE over Perl scalars
//! ([`MemberVal`]) and a [`Session`], for generated conversions to call.
//!
//! # Admission rule
//!
//! A port is listed in [`PORTS`] only when both proofs hold, against the
//! pinned 13.59 tree and the pinned perl 5.38.2 only:
//!
//! 1. **Exact source.** [`HelperPort::source_sha256`] is the sha256 of the
//!    sub's text in the pinned tree, comments and whitespace folded -- the
//!    same fold #805 uses to select a `ConvertUnixTime` port. It is checked
//!    against `tools/exiftool-tables/testdata/helper_oracle_outputs.json`,
//!    which `tools/exiftool-tables/helper_oracle.py` captures from the tree.
//!    A release whose sub differs proves nothing about this port.
//! 2. **Byte-identical behaviour.** The same capture holds the pinned sub's
//!    own output for every probe (zero, negatives, `undef`, huge magnitudes,
//!    non-numeric strings, rationals, each option branch).
//!    `tests::every_port_matches_the_pinned_perl_capture` replays every probe
//!    here and requires the same bytes.
//!
//! A port may REFUSE a branch it does not model ([`HelperError::Refused`]):
//! an ExifTool option this library cannot honour (`DateFormat`,
//! `CoordFormat`, sub-second `SystemTimeRes`), local time, or a magnitude
//! where Perl's integer/unsigned arithmetic would print differently from a
//! double. A refusal is never an answer: the caller keeps the value it has
//! (or the hand parser's), and the refusal is counted. Where Perl DIES, the
//! port says so ([`HelperError::Dies`]) and the test requires the capture to
//! show the same death. Nothing here approximates.
//!
//! Helpers not ported, with the reason, are [`REFUSED_HELPERS`].
//!
//! Called by the generated conversion arms (`conv`), which the IFD walk runs
//! per field in mixed mode.

use std::sync::LazyLock;

use regex::bytes::Regex;

use super::charset;
use super::session::{MemberVal, PerlNum, Session};

/// Why a port did not produce a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperError {
    /// A branch of the Perl sub this port does not model. Never a guess.
    Refused(&'static str),
    /// The Perl sub dies on this input (e.g. `Illegal division by zero`).
    Dies(&'static str),
}

pub type HelperResult = Result<MemberVal, HelperError>;

/// One admitted port: which Perl sub, proven against which source text.
#[derive(Clone, Copy, Debug)]
pub struct HelperPort {
    /// Fully qualified Perl sub, e.g. `Image::ExifTool::Exif::ConvertFraction`.
    pub perl: &'static str,
    /// The module file under the pinned `lib/` that defines it.
    pub module: &'static str,
    /// sha256 of the folded `sub ... }` text in the pinned 13.59 tree.
    pub source_sha256: &'static str,
    /// The Rust function in this module that ports it.
    pub rust: &'static str,
}

/// Every admitted port, in the spike's use-count order (PR #817,
/// `spike/COVERAGE.md` section 5, Frame B).
pub const PORTS: &[HelperPort] = &[
    HelperPort {
        perl: "Image::ExifTool::ConvertDateTime",
        module: "Image/ExifTool.pm",
        source_sha256: "22a3fea0970c697c98d4002710c50ed9fe89b91ed1e314c65220c26c99e9028d",
        rust: "convert_date_time",
    },
    HelperPort {
        perl: "Image::ExifTool::Exif::ConvertFraction",
        module: "Image/ExifTool/Exif.pm",
        source_sha256: "0b253da5d95ce5383233ab715e9d8f61b3d50feb49258cd6a3ed914004d762f3",
        rust: "convert_fraction",
    },
    HelperPort {
        perl: "Image::ExifTool::GPS::ToDMS",
        module: "Image/ExifTool/GPS.pm",
        source_sha256: "b27f5e39644a3b90c81c4ca36a0baf06c3eaae9039f9e6220d816ab7039216dc",
        rust: "to_dms",
    },
    HelperPort {
        perl: "Image::ExifTool::Exif::PrintExposureTime",
        module: "Image/ExifTool/Exif.pm",
        source_sha256: "2326b19ea785fd27ee9d13c58954613f7e5c5330ff287e53c509bc8c1fba89bb",
        rust: "print_exposure_time",
    },
    HelperPort {
        perl: "Image::ExifTool::ConvertUnixTime",
        module: "Image/ExifTool.pm",
        source_sha256: "ee1e09f50ee91f3b67b9d6166af39cdaed3e3ce375080ff51b5465cedad4f7ac",
        rust: "convert_unix_time",
    },
    HelperPort {
        perl: "Image::ExifTool::ConvertDuration",
        module: "Image/ExifTool.pm",
        source_sha256: "c9858a26c63fe1e6b6b33a39f1df435e965d2ee7389dd07f8de3608bc841c9fd",
        rust: "convert_duration",
    },
    HelperPort {
        perl: "Image::ExifTool::IsFloat",
        module: "Image/ExifTool.pm",
        source_sha256: "62c94077616ef958b83ee189cf9cf3668cef3c8898c09b4dc504ecc6dccccbc4",
        rust: "is_float",
    },
    HelperPort {
        perl: "Image::ExifTool::ConvertBitrate",
        module: "Image/ExifTool.pm",
        source_sha256: "b9185a50af4c0ca8b9272faa64ba164e195fb65df04eda9f328864c35cc388e6",
        rust: "convert_bitrate",
    },
    HelperPort {
        perl: "Image::ExifTool::Exif::PrintFraction",
        module: "Image/ExifTool/Exif.pm",
        source_sha256: "1434a8152adaded43710fff1f2d76c333164f1ca661efedff7ad9e501f20cf30",
        rust: "print_fraction",
    },
    HelperPort {
        perl: "Image::ExifTool::GPS::ToDegrees",
        module: "Image/ExifTool/GPS.pm",
        source_sha256: "c9137f3bf19849ae0b73ab1d2eb49444e845d264a6eafeaf8ca17362e4a8c54d",
        rust: "to_degrees",
    },
    HelperPort {
        perl: "Image::ExifTool::Canon::CanonEv",
        module: "Image/ExifTool/Canon.pm",
        source_sha256: "86f043b507abd050b4500e837a98304da948a9e1ec31aaec5696a37666c6aac4",
        rust: "canon_ev",
    },
    HelperPort {
        perl: "Image::ExifTool::Canon::CanonEvInv",
        module: "Image/ExifTool/Canon.pm",
        source_sha256: "030bdb43a00c691a921a937ef8952112d1175c43774c5b4d256aed0bab86902f",
        rust: "canon_ev_inv",
    },
    HelperPort {
        perl: "Image::ExifTool::GetUnixTime",
        module: "Image/ExifTool.pm",
        source_sha256: "7c4b9ade78e619553b6b82a15a31af83cad4ae24325ad21d41a3af0e822fdaff",
        rust: "get_unix_time",
    },
    HelperPort {
        perl: "Image::ExifTool::Exif::PrintFNumber",
        module: "Image/ExifTool/Exif.pm",
        source_sha256: "435ec0e48687d1db8852f04e4559d7bd8607ec68d74bcfcffbf723975c370e67",
        rust: "print_f_number",
    },
    HelperPort {
        perl: "Image::ExifTool::IsInt",
        module: "Image/ExifTool.pm",
        source_sha256: "2bbb4873f5f2adca822e551a7b6f206a7af307dabfc0e1ef6002b8f19e4a30a6",
        rust: "is_int",
    },
    HelperPort {
        perl: "Image::ExifTool::XMP::ConvertXMPDate",
        module: "Image/ExifTool/XMP.pm",
        source_sha256: "1a31d0f220c8e4e31898bb76193eccfa97993c3105f7409c49173a7af0e9273c",
        rust: "convert_xmp_date",
    },
    HelperPort {
        perl: "Image::ExifTool::ConvertFileSize",
        module: "Image/ExifTool.pm",
        source_sha256: "887af9c8aba0dc68e93b90e9d4c30dc80edf813d73050b9b6917a85e83e7679e",
        rust: "convert_file_size",
    },
    HelperPort {
        perl: "Image::ExifTool::Decode",
        module: "Image/ExifTool.pm",
        source_sha256: "e2a3b6777541f21a4a61f655c84a1f5443337e085453018565a04e43aa33e6f3",
        rust: "decode",
    },
    HelperPort {
        perl: "Image::ExifTool::Encode",
        module: "Image/ExifTool.pm",
        source_sha256: "c0d7e605eaf26f64da3c2a81f9a95e876498264b9a48c142c1532f7a526f5fdc",
        rust: "encode",
    },
    // The helpers Exif::Main's remaining conversions call (#838's refusals).
    HelperPort {
        perl: "Image::ExifTool::Exif::ConvertExifText",
        module: "Image/ExifTool/Exif.pm",
        source_sha256: "b2f1c8b9ddd47be014021186d9631a1e423d69111b4ac11790e3fae74e3c4a47",
        rust: "convert_exif_text",
    },
    HelperPort {
        perl: "Image::ExifTool::Exif::DecodeCFAPattern",
        module: "Image/ExifTool/Exif.pm",
        source_sha256: "dad638b4ece00df6621095396005f079a4c67aed865fb9773c382a8b4ca00c3a",
        rust: "decode_cfa_pattern",
    },
    HelperPort {
        perl: "Image::ExifTool::Exif::PrintCFAPattern",
        module: "Image/ExifTool/Exif.pm",
        source_sha256: "b06783693c00220de5e26849c7f92e39a938708f16a4577ac4d8319fb89d4ca1",
        rust: "print_cfa_pattern",
    },
    HelperPort {
        perl: "Image::ExifTool::Exif::PrintSFR",
        module: "Image/ExifTool/Exif.pm",
        source_sha256: "d7138eca99e4d776cfd859bcca19a25159643df4bd9d0714a2745a76628cc81b",
        rust: "print_sfr",
    },
    HelperPort {
        perl: "Image::ExifTool::ASF::GetGUID",
        module: "Image/ExifTool/ASF.pm",
        source_sha256: "c425526f5a3a74cf975e8f4b22d4f6574d3d1533d5c4aaa7392cddfa752e8264",
        rust: "asf_get_guid",
    },
    HelperPort {
        perl: "Image::ExifTool::Printable",
        module: "Image/ExifTool.pm",
        source_sha256: "6caee76604604fbae0e661f43770b5dc29ddb8e060298cefce244fbfa10a979d",
        rust: "printable",
    },
];

/// The engine subs a port above reproduces inline rather than calls, by
/// folded-source sha256 in the pinned tree: `(port, module, sub, digest)`.
/// A port is proven only for a tree where these match too; the helper
/// oracle records them beside the port and
/// `tests::port_dependencies_match_the_capture` holds this list to it.
pub const PORT_DEPENDENCIES: &[(&str, &str, &str, &str)] = &[
    (
        "Image::ExifTool::Exif::ConvertExifText",
        "Image/ExifTool.pm",
        "Options",
        "80bb02af485fb9245d080e679e0caeaa994c4e2e23192f346c5ffae8fac5cdbf",
    ),
    (
        "Image::ExifTool::Exif::ConvertExifText",
        "Image/ExifTool.pm",
        "Decode",
        "e2a3b6777541f21a4a61f655c84a1f5443337e085453018565a04e43aa33e6f3",
    ),
    (
        "Image::ExifTool::Exif::DecodeCFAPattern",
        "Image/ExifTool.pm",
        "GetByteOrder",
        "c783f08627e820902d577934f2373415f8254d60155f254c36cadb1c487947c8",
    ),
    (
        "Image::ExifTool::Exif::PrintSFR",
        "Image/ExifTool.pm",
        "Get16u",
        "8559bff49b577fe8d266dbfce4ba379a917873aec92dc73c7271f32b55c103e0",
    ),
    (
        "Image::ExifTool::Exif::PrintSFR",
        "Image/ExifTool.pm",
        "Get32u",
        "75c842dfc8732163515232c472d3bd07b6c8c0299dfbfe27510ca8c5ab211de0",
    ),
    (
        "Image::ExifTool::Exif::PrintSFR",
        "Image/ExifTool.pm",
        "DoUnpackStd",
        "acfb8926e3e0780079171119d0bd7eda852801d46d677569157c474c3f280424",
    ),
    (
        "Image::ExifTool::Exif::PrintSFR",
        "Image/ExifTool.pm",
        "GetRational64u",
        "7e5fdb0e775ecb4103fa6bfba1cc4136468eb95004f3173bdb1abb18e58a3520",
    ),
    (
        "Image::ExifTool::Exif::PrintSFR",
        "Image/ExifTool.pm",
        "RoundFloat",
        "d36661b4c4a37646cbbbf3bf81a763b52b0e1d8fd55996e205d904954561a418",
    ),
];

/// The subs `Decode` reaches beyond its own body, by folded-source sha256 in
/// the pinned tree (module, sub, digest). The Decode/Encode ports are proven
/// only for a tree where every one of these matches; the helper oracle
/// records them and `tests::every_port_names_the_pinned_source_it_was_proven_against`
/// holds this list to the capture. The data those subs read (`%csType`, the
/// `Charset/*.pm` tables, `%unicode2byte`) is generated, and tied to the same
/// tree by [`super::charset_tables::SOURCES`].
pub const DECODE_DEPENDENCIES: &[(&str, &str, &str)] = &[
    (
        "Image/ExifTool/Charset.pm",
        "Decompose",
        "dc0bb7b02055a4ae63a33d16337a780ff46a200b4556079d7b223af7a0350ed5",
    ),
    (
        "Image/ExifTool/Charset.pm",
        "Recompose",
        "9095f69c6b79ca635234369ff8a4fcf519420836bb9e7bff48cbd7ad280513fe",
    ),
    (
        "Image/ExifTool/Charset.pm",
        "LoadCharset",
        "4f36c876756b736fbdcc215d38d1ab1a168f26bd2b631cd98c696ca2f90be90f",
    ),
];

/// Top-22 helpers (by the spike's use count) this library does NOT port,
/// and why (`Decode` and `Encode`, once here, are ported below). Each is a sub whose exact behaviour needs something this slice
/// does not have; none is approximated.
pub const REFUSED_HELPERS: &[(&str, &str)] = &[
    (
        "Image::ExifTool::InverseDateTime",
        "write-side inverse: reads DateFormat/StrictDate, strptime-style parsing and Time::Local",
    ),
    (
        "Image::ExifTool::ValidateImage",
        "mutates its image argument and warns via REQ_TAG_LOOKUP; needs the session warning sink",
    ),
    (
        "Image::ExifTool::Warn",
        "engine side effect (warning list, IgnoreMinorErrors); needs the session warning sink",
    ),
    (
        "Image::ExifTool::Samsung::Crypt",
        "reads the array-valued EncryptionKey member, $tagInfo and module-level %formatMinMax",
    ),
];

// ---------------------------------------------------------------------------
// Perl primitives the ports share
// ---------------------------------------------------------------------------

const TWO_63: f64 = 9_223_372_036_854_775_808.0;
const TWO_64: f64 = 18_446_744_073_709_551_616.0;
/// Past 2**53 a double no longer holds every integer, and Perl's IV/UV
/// arithmetic (exact) and NV arithmetic (rounded) print differently.
const TWO_53: f64 = 9_007_199_254_740_992.0;

const UV_RANGE: HelperError =
    HelperError::Refused("integer in Perl's unsigned (UV) range; prints unlike a double");
const BEYOND_2_53: HelperError =
    HelperError::Refused("integer beyond 2**53: Perl's exact IV arithmetic is not modelled");

/// Perl's `int()` (pp_int): an IV when the truncated value fits one, a UV
/// when it fits that (refused -- a UV prints its digits, a double `%.15g`),
/// otherwise the truncated double.
pub(crate) fn perl_int(n: PerlNum) -> Result<PerlNum, HelperError> {
    match n {
        PerlNum::Int(_) => Ok(n),
        PerlNum::Float(v) if !v.is_finite() => Ok(n),
        PerlNum::Float(v) => {
            if v > -TWO_63 && v < TWO_63 {
                Ok(PerlNum::Int(v.trunc() as i64))
            } else if v >= TWO_63 && v < TWO_64 {
                Err(UV_RANGE)
            } else {
                Ok(PerlNum::Float(v.trunc()))
            }
        }
    }
}

/// `sprintf("%d")` / `sprintf("%+d")` of a Perl number, including Perl's own
/// `Inf`/`NaN` rendering and its IV cast of an out-of-range double (a UV
/// reinterpreted as signed, `UV_MAX` as -1 beyond that).
pub(crate) fn sprintf_d(n: PerlNum, plus: bool) -> String {
    let i = match n {
        PerlNum::Int(i) => i,
        PerlNum::Float(v) if v.is_nan() => return "NaN".to_string(),
        PerlNum::Float(v) if v.is_infinite() => {
            return if v < 0.0 {
                "-Inf".to_string()
            } else if plus {
                "+Inf".to_string()
            } else {
                "Inf".to_string()
            };
        }
        PerlNum::Float(v) => {
            if v < -TWO_63 {
                i64::MIN
            } else if v < TWO_63 {
                v as i64
            } else if v < TWO_64 {
                (v as u64) as i64
            } else {
                -1
            }
        }
    };
    if plus && i >= 0 {
        format!("+{i}")
    } else {
        i.to_string()
    }
}

/// `sprintf("%.<prec>f")`, with Perl's `Inf`/`-Inf`/`NaN`.
pub(crate) fn sprintf_f(prec: usize, v: f64) -> String {
    if v.is_nan() {
        "NaN".to_string()
    } else if v.is_infinite() {
        if v < 0.0 { "-Inf" } else { "Inf" }.to_string()
    } else {
        format!("{v:.prec$}")
    }
}

/// Perl's `looks_like_number` for a string: optional surrounding whitespace
/// around one complete decimal number (or `Inf`/`Infinity`/`NaN`).
fn looks_like_number(s: &[u8]) -> bool {
    LOOKS_LIKE_NUMBER.is_match(s)
}

/// Unary minus on a scalar that is negative in numeric context. On a string
/// that does not look like a number (`"-3/4"`, `"-1,5"`), Perl's pp_negate
/// flips the sign CHARACTER (`"+3/4"`) and leaves the numeric value it cached
/// during the `< 0` test attached to the new string -- whether that stale
/// cache is then used depends on whether the scalar was copy-on-write, which
/// a Perl-level value does not carry (the pinned perl prints
/// `CanonEv("-1,5")` as -0.03125 for a literal and 0.03125 for a packed
/// string). Such an input is refused; a real number is negated normally.
fn guard_string_negation(val: &MemberVal) -> Result<(), HelperError> {
    match val {
        MemberVal::Str(_) | MemberVal::Bytes(_) if !looks_like_number(&val.perl_bytes()) => {
            Err(HelperError::Refused(
                "unary minus on a non-numeric string (pp_negate string form, stale numeric cache)",
            ))
        }
        _ => Ok(()),
    }
}

fn num_val(n: PerlNum) -> MemberVal {
    MemberVal::from(n)
}

/// `(?-u)` byte regexes. Perl's `\d`, `\s`, `\S`, `\b` and `/i` on a
/// non-UTF-8 string are ASCII-only, and its `$` matches at the end or before
/// a final newline -- spelled out below as `[0-9]`, `[\t\n\v\f\r ]` and
/// `\n?\z` so no Unicode class or Rust `$` semantics leak in.
macro_rules! perl_re {
    ($name:ident, $re:expr) => {
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($re).expect("helper regex compiles"));
    };
}

// IsFloat: /^[+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?$/ -- the
// lookahead folded into the alternation `\d+(\.\d*)?|\.\d+`, which accepts
// exactly the same strings.
perl_re!(
    IS_FLOAT,
    r"(?-u)^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[Ee][+-]?[0-9]+)?\n?\z"
);
perl_re!(
    IS_FLOAT_COMMA,
    r"(?-u)^[+-]?(?:[0-9]+(?:,[0-9]*)?|,[0-9]+)(?:[Ee][+-]?[0-9]+)?\n?\z"
);
perl_re!(IS_INT, r"(?-u)^[+-]?[0-9]+\n?\z");
perl_re!(
    LOOKS_LIKE_NUMBER,
    r"(?i-u)^[\t\n\x0B\x0C\r ]*[+-]?(?:(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:e[+-]?[0-9]+)?|inf(?:inity)?|nan)[\t\n\x0B\x0C\r ]*\z"
);
perl_re!(FRACTION, r"(?-u)([-+]?[0-9]+)/([0-9]+)");
perl_re!(TO_DEG_INVALID, r"(?-u)\b(?:inf|undef)\b");
perl_re!(
    TO_DEG_PAIR,
    r"(?i-u)^(.*(?:N(?:orth)?|S(?:outh)?)),[\t\n\x0B\x0C\r ]*(.*(?:E(?:ast)?|W(?:est)?))\n?\z"
);
perl_re!(
    TO_DEG_NUM,
    r"(?-u)[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[Ee][+-][0-9]+)?"
);
perl_re!(
    TO_DEG_SOUTH_WEST,
    r"(?i-u)[^A-Z](?:S(?:outh)?|W(?:est)?)[\t\n\x0B\x0C\r ]*\n?\z"
);
perl_re!(
    XMP_DATE,
    r"(?-u)^([0-9]{4})-([0-9]{2})-([0-9]{2})[T ]([0-9]{2}:[0-9]{2})(:[0-9]{2})?[\t\n\x0B\x0C\r ]*([^\t\n\x0B\x0C\r ]*)\n?\z"
);
perl_re!(XMP_DATE_PREFIX, r"(?-u)^[0-9]{4}(?:-[0-9]{2}){0,2}");
perl_re!(
    UNIX_TIME_STR,
    r"(?-u)^([0-9]+)[-:]([0-9]+)[-:]([0-9]+)[\t\n\x0B\x0C\r ]+([0-9]+):([0-9]+):([0-9]+)(.*)"
);
perl_re!(UNIX_TIME_ZONE, r"(?i-u)(?:Z|([-+])([0-9]+):([0-9]+))");
perl_re!(UNIX_TIME_FRAC, r"(?-u)^(\.[0-9]+)");

fn bytes_str(b: &[u8]) -> &str {
    // Only captures of ASCII-only groups (digits, signs, `:`) come here.
    std::str::from_utf8(b).expect("ASCII capture")
}

/// `$s =~ tr/<from>/<to>/` for one byte.
fn tr_byte(s: &[u8], from: u8, to: u8) -> Vec<u8> {
    s.iter().map(|&b| if b == from { to } else { b }).collect()
}

// ---------------------------------------------------------------------------
// The ports
// ---------------------------------------------------------------------------

/// `Image::ExifTool::IsFloat($val)` (ExifTool.pm, pinned 13.59):
///
/// ```perl
/// return 1 if $_[0] =~ /^[+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?$/;
/// return 0 unless $_[0] =~ /^[+-]?(?=\d|,\d)\d*(,\d*)?([Ee]([+-]?\d+))?$/;
/// $_[0] =~ tr/,/./;   # but translate ',' to '.'
/// return 1;
/// ```
///
/// Returns `(result, argument afterwards)`: a comma-decimal string is
/// rewritten IN PLACE, and every caller that goes on to use `$val` sees the
/// rewritten form (`PrintFNumber("-1,5")` returns `-1.5`).
#[must_use]
pub fn is_float(val: &MemberVal) -> (MemberVal, MemberVal) {
    let s = val.perl_bytes();
    if IS_FLOAT.is_match(&s) {
        return (MemberVal::Int(1), val.clone());
    }
    if !IS_FLOAT_COMMA.is_match(&s) {
        return (MemberVal::Int(0), val.clone());
    }
    (
        MemberVal::Int(1),
        MemberVal::from_bytes(tr_byte(&s, b',', b'.')),
    )
}

/// `Image::ExifTool::IsInt($val)`: `scalar($_[0] =~ /^[+-]?\d+$/)` -- Perl's
/// yes (`1`) or no (`""`).
#[must_use]
pub fn is_int(val: &MemberVal) -> MemberVal {
    MemberVal::Bool(IS_INT.is_match(&val.perl_bytes()))
}

/// `Image::ExifTool::Exif::ConvertFraction($val)`:
///
/// ```perl
/// if ($val =~ m{([-+]?\d+)/(\d+)}) {
///     $val = $2 ? $1 / $2 : ($1 ? 'inf' : 'undef');
/// }
/// return $val;
/// ```
///
/// `$2` is tested as a STRING: `"00"` is true, so `"5/00"` divides by zero
/// and dies, exactly as the Perl does.
pub fn convert_fraction(val: &MemberVal) -> HelperResult {
    let s = val.perl_bytes();
    let Some(c) = FRACTION.captures(&s) else {
        return Ok(val.clone());
    };
    let num_s = MemberVal::Str(bytes_str(&c[1]).to_string());
    let den_s = MemberVal::Str(bytes_str(&c[2]).to_string());
    if !den_s.is_truthy() {
        let r = if num_s.is_truthy() { "inf" } else { "undef" };
        return Ok(MemberVal::Str(r.to_string()));
    }
    let (num, den) = (num_s.perl_num(), den_s.perl_num());
    if num.as_f64().abs() > TWO_53 || den.as_f64().abs() > TWO_53 {
        return Err(BEYOND_2_53);
    }
    if den.as_f64() == 0.0 {
        return Err(HelperError::Dies("Illegal division by zero"));
    }
    // pp_divide keeps an exact integer quotient of two integers as an IV;
    // below 2**53 that prints the same as the double, so one path serves.
    Ok(MemberVal::Float(num.as_f64() / den.as_f64()))
}

/// `Image::ExifTool::Exif::PrintExposureTime($secs)`: the `IsFloat` gate
/// (non-numbers returned as they are, comma decimals rewritten), then the
/// existing numeric port [`crate::core::formatters::exif_print_conv::print_exposure_time`]
/// -- except where `int(0.5 + 1/$secs)` leaves `i64`, where Perl's `%d`
/// wraps and the delegated port saturates, which this port reproduces itself.
pub fn print_exposure_time(val: &MemberVal) -> HelperResult {
    let (ok, secs) = is_float(val);
    if !ok.is_truthy() {
        return Ok(secs);
    }
    // NV context: `sprintf("%.1f", "-0")` is `-0.0` (see `MemberVal::perl_nv`).
    let v = secs.perl_nv();
    if v < 0.25001 && v > 0.0 {
        let q = 0.5 + 1.0 / v;
        if q >= TWO_63 {
            let i = perl_int(PerlNum::Float(q))?;
            return Ok(MemberVal::Str(format!("1/{}", sprintf_d(i, false))));
        }
    }
    Ok(MemberVal::Str(
        crate::core::formatters::exif_print_conv::print_exposure_time(v),
    ))
}

/// `Image::ExifTool::Exif::PrintFNumber($val)`:
///
/// ```perl
/// if (Image::ExifTool::IsFloat($val) and $val > 0) {
///     $val = sprintf(($val<1 ? "%.2f" : "%.1f"), $val);
/// }
/// return $val;
/// ```
///
/// `$val` is returned as `IsFloat` left it, so `"-1,5"` comes back `-1.5`.
pub fn print_f_number(val: &MemberVal) -> HelperResult {
    let (ok, v) = is_float(val);
    if ok.is_truthy() {
        let n = v.perl_num().as_f64();
        if n > 0.0 {
            return Ok(MemberVal::Str(sprintf_f(if n < 1.0 { 2 } else { 1 }, n)));
        }
    }
    Ok(v)
}

/// `Image::ExifTool::Exif::PrintFraction($val)`:
///
/// ```perl
/// if (defined $val) {
///     $val *= 1.00001;
///     if (not $val) { $str = '0'; }
///     elsif (int($val)/$val > 0.999) { $str = sprintf("%+d", int($val)); }
///     elsif ((int($val*2))/($val*2) > 0.999) { $str = sprintf("%+d/2", int($val * 2)); }
///     elsif ((int($val*3))/($val*3) > 0.999) { $str = sprintf("%+d/3", int($val * 3)); }
///     else { $str = sprintf("%+.3g", $val); }
/// }
/// ```
///
/// The finite, in-`i64` domain delegates to the existing
/// [`crate::core::formatters::exif_print_conv::print_fraction`]. `NaN` is
/// TRUE in Perl (`not $val` is false), so it reaches `%+.3g` and prints
/// `NaN`; the delegated port treats it as zero, so this port handles it (and
/// the infinities) itself. Beyond `i64` the `int()` quotients are UVs or
/// wrapped IVs: refused.
pub fn print_fraction(val: &MemberVal) -> HelperResult {
    if !val.is_defined() {
        return Ok(MemberVal::Undef);
    }
    let v = val.perl_num().as_f64() * 1.00001;
    if v.is_nan() {
        return Ok(MemberVal::Str("NaN".to_string()));
    }
    if v.is_infinite() {
        return Ok(MemberVal::Str(
            if v < 0.0 { "-Inf" } else { "+Inf" }.to_string(),
        ));
    }
    if v.abs() * 3.0 >= TWO_63 {
        return Err(UV_RANGE);
    }
    Ok(MemberVal::Str(
        crate::core::formatters::exif_print_conv::print_fraction(v / 1.00001),
    ))
}

/// `Image::ExifTool::ConvertDuration($time)`: the `IsFloat` gate, then the
/// existing [`crate::core::formatters::duration::convert_duration`] for
/// magnitudes whose `int($time / 3600)` is an exact `i64` in both; larger
/// ones refuse.
pub fn convert_duration(val: &MemberVal) -> HelperResult {
    let (ok, t) = is_float(val);
    if !ok.is_truthy() {
        return Ok(t);
    }
    let v = t.perl_num().as_f64();
    if v.abs() >= TWO_53 {
        return Err(BEYOND_2_53);
    }
    Ok(MemberVal::Str(
        crate::core::formatters::duration::convert_duration(v),
    ))
}

/// `Image::ExifTool::ConvertBitrate($bitrate)`: the `IsFloat` gate, then the
/// existing [`crate::core::formatters::bitrate::convert_bitrate`] -- except a
/// negative zero, which Perl's `%.3g` prints `-0`.
pub fn convert_bitrate(val: &MemberVal) -> HelperResult {
    let (ok, b) = is_float(val);
    if !ok.is_truthy() {
        return Ok(b);
    }
    let v = b.perl_num().as_f64();
    if v == 0.0 && v.is_sign_negative() {
        // `sprintf('%.3g', -0.0)` is `-0`; the delegated port's `%g` prints
        // `0` for a negative zero (a disagreement with Perl in that port,
        // reached only by a float field holding -0.0).
        return Ok(MemberVal::Str("-0 bps".to_string()));
    }
    Ok(MemberVal::Str(
        crate::core::formatters::bitrate::convert_bitrate(b.perl_num().as_f64()),
    ))
}

/// `Image::ExifTool::Canon::CanonEv($val)` over a Perl scalar: numified, then
/// the existing [`super::exprs::canon_ev`] (whose doc carries the Perl). An
/// integral result below 2**53 prints the same as Perl's IV would.
pub fn canon_ev(val: &MemberVal) -> HelperResult {
    let v = val.perl_num().as_f64();
    if v < 0.0 {
        guard_string_negation(val)?;
    }
    if v.abs() >= TWO_53 {
        return Err(BEYOND_2_53);
    }
    Ok(MemberVal::Float(super::exprs::canon_ev(v)))
}

/// `Image::ExifTool::Canon::CanonEvInv($num)`:
///
/// ```perl
/// if ($num < 0) { $num = -$num; $sign = -1; } else { $sign = 1; }
/// my $val = int($num);
/// my $frac = $num - $val;
/// if (abs($frac - 0.33) < 0.05) { $frac = 0x0c }
/// elsif (abs($frac - 0.67) < 0.05) { $frac = 0x14; }
/// else { $frac = int($frac * 0x20 + 0.5); }
/// return $sign * ($val * 0x20 + $frac);
/// ```
pub fn canon_ev_inv(val: &MemberVal) -> HelperResult {
    let n = val.perl_num().as_f64();
    if n.is_nan() {
        return Err(HelperError::Refused("NaN through int() and abs()"));
    }
    if n < 0.0 {
        guard_string_negation(val)?;
    }
    let (sign, num) = if n < 0.0 { (-1.0, -n) } else { (1.0, n) };
    if num * 32.0 >= TWO_53 {
        return Err(BEYOND_2_53);
    }
    let whole = num.trunc();
    let frac = num - whole;
    let frac = if (frac - 0.33).abs() < 0.05 {
        12.0
    } else if (frac - 0.67).abs() < 0.05 {
        20.0
    } else {
        (frac * 32.0 + 0.5).trunc()
    };
    Ok(MemberVal::Float(sign * (whole * 32.0 + frac)))
}

/// `Image::ExifTool::GPS::ToDegrees($val [, $doSign [, $coord]])`:
///
/// ```perl
/// return '' if $val =~ /\b(inf|undef)\b/;
/// if ($coord and ($coord eq 'lat' or $coord eq 'lon') and
///     $val =~ /^(.*(?:N(?:orth)?|S(?:outh)?)),\s*(.*(?:E(?:ast)?|W(?:est)?))$/i)
/// {
///     $val = $coord eq 'lat' ? $1 : $2;
/// }
/// my ($d, $m, $s) = ($val =~ /((?:[+-]?)(?=\d|\.\d)\d*(?:\.\d*)?(?:[Ee][+-]\d+)?)/g);
/// return '' unless defined $d;
/// my $deg = $d + (($m || 0) + ($s || 0)/60) / 60;
/// $deg = -$deg if $doSign ? $val =~ /[^A-Z](S(outh)?|W(est)?)\s*$/i : $deg < 0;
/// return $deg;
/// ```
pub fn to_degrees(val: &MemberVal, do_sign: &MemberVal, coord: &MemberVal) -> HelperResult {
    let mut s = val.perl_bytes().into_owned();
    if TO_DEG_INVALID.is_match(&s) {
        return Ok(MemberVal::Str(String::new()));
    }
    if coord.is_truthy() {
        let c = coord.perl_bytes();
        if c.as_ref() == b"lat" || c.as_ref() == b"lon" {
            if let Some(m) = TO_DEG_PAIR.captures(&s) {
                let pick = if c.as_ref() == b"lat" { &m[1] } else { &m[2] };
                s = pick.to_vec();
            }
        }
    }
    let nums: Vec<String> = TO_DEG_NUM
        .find_iter(&s)
        .take(3)
        .map(|m| bytes_str(m.as_bytes()).to_string())
        .collect();
    let Some(d) = nums.first() else {
        return Ok(MemberVal::Str(String::new()));
    };
    let part = |i: usize| {
        nums.get(i)
            .map(|x| MemberVal::Str(x.clone()))
            .filter(MemberVal::is_truthy)
            .map_or(0.0, |x| x.perl_num().as_f64())
    };
    let dn = MemberVal::Str(d.clone()).perl_num();
    let deg = dn.as_f64() + (part(1) + part(2) / 60.0) / 60.0;
    // An integral result that Perl may hold as an exact IV prints its digits;
    // a double prints `%.15g`. They agree below 1e15.
    if deg.is_finite() && deg.abs() >= 1e15 {
        return Err(BEYOND_2_53);
    }
    let negate = if do_sign.is_truthy() {
        TO_DEG_SOUTH_WEST.is_match(&s)
    } else {
        deg < 0.0
    };
    Ok(MemberVal::Float(if negate { -deg } else { deg }))
}

/// `Image::ExifTool::GPS::ToDMS($et, $val [, $doPrintConv [, $ref]])`
/// (GPS.pm, pinned 13.59) -- every branch except `$doPrintConv eq '1'` with
/// a Perl-true `CoordFormat` option (a user `sprintf` format), which refuses.
/// A `$ref` carrying a `%` or a regex metacharacter would change the format
/// or the XMP trailing-zero substitution it is interpolated into; refused.
pub fn to_dms(
    session: &Session,
    val: &MemberVal,
    do_print_conv: &MemberVal,
    ref_: &MemberVal,
) -> HelperResult {
    let dpc_bytes = do_print_conv.perl_bytes();
    let dpc_is = |s: &str| do_print_conv.is_truthy() && dpc_bytes.as_ref() == s.as_bytes();
    // unless (length $val)
    if val.perl_length().unwrap_or(0) == 0 {
        return Ok(if dpc_is("1") {
            val.clone()
        } else {
            MemberVal::Undef
        });
    }
    let mut v = val.perl_num().as_f64();
    let mut do_print = do_print_conv.is_truthy();
    let mut neg = false;
    // `$ref` after the sign logic: None is Perl's undef.
    let ref_s: Option<Vec<u8>>;
    if ref_.is_truthy() {
        let r = ref_.perl_bytes().into_owned();
        if v < 0.0 {
            guard_string_negation(val)?;
        }
        let mapped = if v < 0.0 {
            v = -v;
            match r.as_slice() {
                b"N" => Some(b"S".to_vec()),
                b"E" => Some(b"W".to_vec()),
                _ => None,
            }
        } else {
            Some(r)
        };
        ref_s = if dpc_is("2") {
            mapped
        } else {
            let mut spaced = b" ".to_vec();
            spaced.extend(mapped.unwrap_or_default());
            Some(spaced)
        };
    } else {
        if dpc_is("3") {
            neg = v < 0.0;
            do_print = false;
        }
        v = v.abs();
        ref_s = Some(Vec::new());
    }
    let ref_bytes = ref_s.unwrap_or_default();
    if ref_bytes
        .iter()
        .any(|&b| !(b.is_ascii_alphanumeric() || b == b' '))
    {
        return Err(HelperError::Refused(
            "$ref with a format or regex metacharacter",
        ));
    }
    let ref_text = bytes_str(&ref_bytes).to_string();
    // With CoordFormat unset the format is fixed: three specs for the
    // degree form (%d %d %.2f), two for XMP (%d %.8f).
    let xmp = do_print && dpc_bytes.as_ref() != b"1";
    if do_print && !xmp && session.option("CoordFormat").is_truthy() {
        return Err(HelperError::Refused(
            "CoordFormat option (user sprintf format)",
        ));
    }
    let num = if do_print && xmp { 2 } else { 3 };
    // @c: each coordinate as the Perl scalar it is at that point.
    let c0 = perl_int(PerlNum::Float(v))?;
    let mut c: Vec<MemberVal> = Vec::with_capacity(3);
    let c1f = (v - c0.as_f64()) * 60.0;
    c.push(num_val(c0));
    if num > 2 {
        let c1 = perl_int(PerlNum::Float(c1f))?;
        let c2f = (v - c0.as_f64() - c1.as_f64() / 60.0) * 3600.0;
        c.push(num_val(c1));
        c.push(MemberVal::Float(c2f));
    } else {
        c.push(MemberVal::Float(c1f));
    }
    // $c[-1] = $doPrintConv ? sprintf($fmt[-1], $c[-1]) : ($c[-1] . '');
    let last_f = c
        .last()
        .expect("two or three coordinates")
        .perl_num()
        .as_f64();
    let last = if do_print {
        MemberVal::Str(sprintf_f(if xmp { 8 } else { 2 }, last_f))
    } else {
        MemberVal::Str(c.last().expect("coordinate").perl_string())
    };
    let n = c.len();
    c[n - 1] = last;
    if c[n - 1].perl_num().as_f64() >= 60.0 {
        c[n - 1] = MemberVal::Float(c[n - 1].perl_num().as_f64() - 60.0);
        // ($c[-2] += 1) >= 60 and $num > 2 and $c[-2] -= 60, $c[-3] += 1;
        let bumped = c[n - 2].perl_num().as_f64() + 1.0;
        c[n - 2] = MemberVal::Float(bumped);
        if bumped >= 60.0 && num > 2 {
            c[n - 2] = MemberVal::Float(bumped - 60.0);
            c[n - 3] = MemberVal::Float(c[n - 3].perl_num().as_f64() + 1.0);
        }
    }
    if c.iter().any(|x| x.perl_num().as_f64().abs() >= TWO_53) {
        return Err(BEYOND_2_53);
    }
    if do_print {
        let d = |x: &MemberVal| sprintf_d(x.perl_num(), false);
        let f = |p: usize, x: &MemberVal| sprintf_f(p, x.perl_num().as_f64());
        if xmp {
            let mut out = format!("{},{}{}", d(&c[0]), f(8, &c[1]), ref_text);
            if dpc_bytes.as_ref() == b"2" {
                out = trim_xmp_zeros(&out, &ref_text);
            }
            Ok(MemberVal::Str(out))
        } else {
            Ok(MemberVal::Str(format!(
                "{} deg {}' {}\"{}",
                d(&c[0]),
                d(&c[1]),
                f(2, &c[2]),
                ref_text
            )))
        }
    } else {
        let parts: Vec<String> = c
            .iter()
            .map(|x| {
                if neg {
                    let n = x.perl_num();
                    match n {
                        PerlNum::Int(i) => (-i).to_string(),
                        PerlNum::Float(f) => MemberVal::Float(-f).perl_string(),
                    }
                } else {
                    x.perl_string()
                }
            })
            .collect();
        Ok(MemberVal::Str(format!("{}{}", parts.join(" "), ref_text)))
    }
}

/// `$rtnVal =~ s/(\d)0+$ref$/$1$ref/` for an alphanumeric `$ref`: the
/// leftmost digit followed by a run of zeros that ends where `$ref` ends the
/// string keeps the digit and drops the zeros.
fn trim_xmp_zeros(s: &str, suffix: &str) -> String {
    let Some(body) = s.strip_suffix(suffix) else {
        return s.to_string();
    };
    let b = body.as_bytes();
    let zeros = b.iter().rev().take_while(|&&x| x == b'0').count();
    if zeros == 0 {
        return s.to_string();
    }
    let run_start = b.len() - zeros;
    // Leftmost start: the digit just before the run, else the run's own
    // first zero (then at least one more zero must follow it).
    let keep_to = if run_start > 0 && b[run_start - 1].is_ascii_digit() {
        run_start
    } else if zeros >= 2 {
        run_start + 1
    } else {
        return s.to_string();
    };
    format!("{}{}", &body[..keep_to], suffix)
}

/// `$self->ConvertDateTime($date)` (ExifTool.pm, pinned 13.59): the date
/// unchanged, unless `$$self{OPTIONS}{GlobalTimeShift}` or
/// `$$self{OPTIONS}{DateFormat}` is Perl-TRUE -- a `"0"` of either leaves the
/// identity branch, exactly as the Perl's `if ($shift)` / `if ($fmt)` do.
/// Those branches (ShiftTime, strftime, StrictDate) refuse.
pub fn convert_date_time(session: &Session, date: &MemberVal) -> HelperResult {
    if session.option("GlobalTimeShift").is_truthy() {
        return Err(HelperError::Refused("GlobalTimeShift option"));
    }
    if session.option("DateFormat").is_truthy() {
        return Err(HelperError::Refused("DateFormat option"));
    }
    Ok(date.clone())
}

/// `Image::ExifTool::ConvertUnixTime($time [, $toLocal [, $dec]])` for an
/// effective `$dec` of 0: no `$dec` argument and a Perl-false
/// `SystemTimeRes` (ExifTool::Init copies it into `%static_vars`). The
/// calendar work is the existing [`super::exprs::convert_unix_time`];
/// `KeepUTCTime` (`gmtime` plus `Z`) is the one branch added here. A
/// non-finite time refuses.
pub fn convert_unix_time(
    session: &Session,
    time: &MemberVal,
    to_local: &MemberVal,
    dec: &MemberVal,
) -> HelperResult {
    let t = time.perl_num().as_f64();
    if t == 0.0 {
        return Ok(MemberVal::Str("0000:00:00 00:00:00".to_string()));
    }
    if dec.is_defined() || session.option("SystemTimeRes").is_truthy() {
        return Err(HelperError::Refused("sub-second $dec / SystemTimeRes"));
    }
    if !t.is_finite() {
        return Err(HelperError::Refused("non-finite time"));
    }
    if !to_local.is_truthy() {
        return Ok(MemberVal::Str(super::exprs::convert_unix_time(t, false)));
    }
    if session.option("KeepUTCTime").is_truthy() {
        return Ok(MemberVal::Str(format!(
            "{}Z",
            super::exprs::convert_unix_time(t, false)
        )));
    }
    Ok(MemberVal::Str(super::exprs::convert_unix_time(t, true)))
}

/// Days in each month, for Time::Local's range check.
fn days_in_month(year: i64, month0: i64) -> i64 {
    const DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    if month0 == 1 && leap {
        29
    } else {
        DAYS[month0 as usize]
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `Image::ExifTool::GetUnixTime($timeStr [, $isLocal])` (ExifTool.pm,
/// pinned 13.59), on its UTC path: `$isLocal` false, `'2'` ("assume UTC"),
/// or a time string carrying an explicit `Z` / `+-HH:MM` zone. A true
/// `$isLocal` with no zone calls `TimeLocal` (the host zone): refused. So is
/// any field `Time::Local::timegm` would croak on (month, day, hour, minute,
/// second out of range) and any year outside 1000-9999 (its two-digit-year
/// window depends on the current date).
pub fn get_unix_time(time_str: &MemberVal, is_local: &MemberVal) -> HelperResult {
    let s = time_str.perl_bytes();
    if s.as_ref() == b"0000:00:00 00:00:00" {
        return Ok(MemberVal::Int(0));
    }
    let Some(c) = UNIX_TIME_STR.captures(&s) else {
        return Ok(MemberVal::Undef);
    };
    let field = |i: usize| bytes_str(&c[i]).to_string();
    let tz_str = c[7].to_vec();
    let mut tz_sec: i64 = 0;
    let mut local = is_local.is_truthy();
    if local {
        if let Some(z) = UNIX_TIME_ZONE.captures(&tz_str) {
            if let (Some(sign), Some(h), Some(m)) = (z.get(1), z.get(2), z.get(3)) {
                let (h, m) = (bytes_str(h.as_bytes()), bytes_str(m.as_bytes()));
                if h.len() > 6 || m.len() > 6 {
                    return Err(HelperError::Refused("zone offset digits"));
                }
                let (h, m): (i64, i64) = (h.parse().unwrap_or(0), m.parse().unwrap_or(0));
                tz_sec = (h * 60 + m) * if sign.as_bytes() == b"-" { -60 } else { 60 };
            }
            local = false;
        } else if is_local.perl_bytes().as_ref() == b"2" {
            local = false;
        }
    }
    if local {
        return Err(HelperError::Refused(
            "local time (TimeLocal, host time zone)",
        ));
    }
    let mut nums = [0i64; 6];
    for (i, n) in nums.iter_mut().enumerate() {
        let f = field(i + 1);
        if f.len() > 9 {
            return Err(HelperError::Refused(
                "date field beyond Time::Local's range",
            ));
        }
        *n = f.parse().expect("digits");
    }
    let [year, month, day, hour, min, sec] = nums;
    let month0 = month - 1;
    if !(1000..=9999).contains(&year)
        || !(0..12).contains(&month0)
        || day < 1
        || day > days_in_month(year, month0)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&min)
        || !(0..60).contains(&sec)
    {
        return Err(HelperError::Refused(
            "Time::Local range check / two-digit-year window",
        ));
    }
    let t = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + min * 60 + sec - tz_sec;
    if let Some(f) = UNIX_TIME_FRAC.captures(&tz_str) {
        let frac = MemberVal::Str(bytes_str(&f[1]).to_string())
            .perl_num()
            .as_f64();
        return Ok(MemberVal::Float(t as f64 + frac));
    }
    Ok(MemberVal::Int(t))
}

/// `Image::ExifTool::XMP::ConvertXMPDate($val [, $unsure])` in scalar
/// context (every table call site):
///
/// ```perl
/// if ($val =~ /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$/) {
///     my $s = $5 || '';
///     $val = "$1:$2:$3 $4$s$6";
/// } elsif (not $unsure and $val =~ /^(\d{4})(-\d{2}){0,2}/) {
///     $val =~ tr/-/:/;
/// }
/// return $val;
/// ```
#[must_use]
pub fn convert_xmp_date(val: &MemberVal, unsure: &MemberVal) -> MemberVal {
    let s = val.perl_bytes();
    if let Some(c) = XMP_DATE.captures(&s) {
        let g = |i: usize| c.get(i).map_or(&b""[..], |m| m.as_bytes());
        let secs = MemberVal::from_bytes(g(5).to_vec());
        let secs = if secs.is_truthy() { g(5) } else { b"" };
        let mut out = Vec::with_capacity(s.len());
        for (part, sep) in [
            (g(1), &b":"[..]),
            (g(2), b":"),
            (g(3), b" "),
            (g(4), b""),
            (secs, b""),
            (g(6), b""),
        ] {
            out.extend_from_slice(part);
            out.extend_from_slice(sep);
        }
        return MemberVal::from_bytes(out);
    }
    if !unsure.is_truthy() && XMP_DATE_PREFIX.is_match(&s) {
        return MemberVal::from_bytes(tr_byte(&s, b'-', b':'));
    }
    val.clone()
}

/// `Image::ExifTool::ConvertFileSize($val [, $et])`: SI units, or binary
/// ones when `$et` is given and its `ByteUnit` option is exactly `Binary`.
/// Both branches are ported (the v1 `exprs.rs` port has the SI one only).
pub fn convert_file_size(val: &MemberVal, session: Option<&Session>) -> HelperResult {
    let binary = session.is_some_and(|s| s.option("ByteUnit").perl_bytes().as_ref() == b"Binary");
    let v = val.perl_num().as_f64();
    let bytes = || {
        let mut out = val.perl_bytes().into_owned();
        out.extend_from_slice(b" bytes");
        MemberVal::from_bytes(out)
    };
    let unit =
        |p: usize, div: f64, u: &str| MemberVal::Str(format!("{} {u}", sprintf_f(p, v / div)));
    let out = if binary {
        if v < 2048.0 {
            bytes()
        } else if v < 10240.0 {
            unit(1, 1024.0, "KiB")
        } else if v < 2_097_152.0 {
            unit(0, 1024.0, "KiB")
        } else if v < 10_485_760.0 {
            unit(1, 1_048_576.0, "MiB")
        } else if v < 2_147_483_648.0 {
            unit(0, 1_048_576.0, "MiB")
        } else if v < 10_737_418_240.0 {
            unit(1, 1_073_741_824.0, "GiB")
        } else {
            unit(0, 1_073_741_824.0, "GiB")
        }
    } else if v < 2000.0 {
        bytes()
    } else if v < 10_000.0 {
        unit(1, 1000.0, "kB")
    } else if v < 2_000_000.0 {
        unit(0, 1000.0, "kB")
    } else if v < 10_000_000.0 {
        unit(1, 1_000_000.0, "MB")
    } else if v < 2_000_000_000.0 {
        unit(0, 1_000_000.0, "MB")
    } else if v < 10_000_000_000.0 {
        unit(1, 1_000_000_000.0, "GB")
    } else {
        unit(0, 1_000_000_000.0, "GB")
    };
    Ok(out)
}

/// `$self->Decode($val, $from [, $fromOrder [, $to [, $toOrder]]])`
/// (ExifTool.pm, pinned 13.59):
///
/// ```perl
/// $from or $from = $$self{OPTIONS}{Charset};
/// $to or $to = $$self{OPTIONS}{Charset};
/// if ($from ne $to and length $val) {
///     require Image::ExifTool::Charset;
///     my $cs1 = $Image::ExifTool::Charset::csType{$from};
///     my $cs2 = $Image::ExifTool::Charset::csType{$to};
///     if ($cs1 and $cs2 and not $cs2 & 0x002) {
///         # treat as straight ASCII if no character will need remapping
///         if (($cs1 | $cs2) & 0x680 or $val =~ /[\x80-\xff]/) {
///             my $uni = Image::ExifTool::Charset::Decompose($self, $val, $from, $fromOrder);
///             $val = Image::ExifTool::Charset::Recompose($self, $uni, $to, $toOrder);
///         }
///     } elsif ($self) {
///         my $set = $cs1 ? $to : $from;
///         unless ($$self{"DecodeWarn$set"}) {
///             $self->Warn("Unsupported character set ($set)");
///             $$self{"DecodeWarn$set"} = 1;
///         }
///     }
/// }
/// return $val;
/// ```
///
/// Every charset `%csType` names is ported, as a source and (where Perl
/// allows it, i.e. without the 0x002 flag) as a destination; a destination
/// Perl refuses takes the same `Unsupported character set` path here. The
/// value is a Perl byte string in and out ([`MemberVal::Bytes`] when it is
/// not UTF-8). Side effects are the Perl's: the `DecodeWarn<set>`,
/// `WarnBadUTF8`, `WrongByteOrder` and `EncodingError` members, and `Warn`
/// requests recorded on the Session ([`Session::warn`]).
///
/// Refused: a 2-/4-byte charset whose byte order comes from `GetByteOrder()`
/// (no `$fromOrder`/`$toOrder`, or `Unknown`) when the Session has no
/// `byte_order`; a charset name that is not UTF-8 on the unsupported path
/// (it would name a member). Not modelled: the global
/// `$Image::ExifTool::evalWarning` that Decompose's UTF-8 branch leaves set
/// after a malformation (the Perl's value does not depend on it, and every
/// ExifTool reader of it clears it first).
pub fn decode(
    session: &mut Session,
    val: &MemberVal,
    from: &MemberVal,
    from_order: &MemberVal,
    to: &MemberVal,
    to_order: &MemberVal,
) -> HelperResult {
    let from = if from.is_truthy() {
        from.clone()
    } else {
        session.option("Charset")
    };
    let to = if to.is_truthy() {
        to.clone()
    } else {
        session.option("Charset")
    };
    let (from_b, to_b) = (from.perl_bytes(), to.perl_bytes());
    if from_b == to_b || val.perl_length().unwrap_or(0) == 0 {
        return Ok(val.clone());
    }
    let (cs1, cs2) = (charset::cs_type(&from_b), charset::cs_type(&to_b));
    match (cs1, cs2) {
        (Some(t1), Some(t2)) if t2 & 0x002 == 0 => {
            let bytes = val.perl_bytes();
            if (t1 | t2) & 0x680 == 0 && !bytes.iter().any(|&b| b >= 0x80) {
                return Ok(val.clone());
            }
            // both names are %csType keys, so ASCII
            let (from_s, to_s) = (bytes_str(&from_b), bytes_str(&to_b));
            let uni = charset::decompose(session, &bytes, from_s, from_order)?;
            let out = charset::recompose(session, uni, to_s, to_order)?;
            Ok(MemberVal::from_bytes(out))
        }
        _ => {
            let set = if cs1.is_some() { &to_b } else { &from_b };
            let set = std::str::from_utf8(set).map_err(|_| {
                HelperError::Refused("an unsupported charset name that is not UTF-8")
            })?;
            let key = format!("DecodeWarn{set}");
            if !session.member(&key).is_truthy() {
                session.warn(MemberVal::Str(format!("Unsupported character set ({set})")));
                session
                    .set_member(&key, MemberVal::Int(1))
                    .map_err(|_| HelperError::Refused("member typed on Session"))?;
            }
            Ok(val.clone())
        }
    }
}

/// `$self->Encode($val, $to [, $toOrder])` (ExifTool.pm, pinned 13.59):
/// `return $self->Decode($val, undef, undef, $to, $toOrder);` -- from the
/// `Charset` option into `$to`.
pub fn encode(
    session: &mut Session,
    val: &MemberVal,
    to: &MemberVal,
    to_order: &MemberVal,
) -> HelperResult {
    decode(
        session,
        val,
        &MemberVal::Undef,
        &MemberVal::Undef,
        to,
        to_order,
    )
}

// ---------------------------------------------------------------------------
// Exif.pm / ASF.pm / ExifTool.pm helpers the Exif::Main arms call
// ---------------------------------------------------------------------------

/// `GetByteOrder()` of the session: `'II'`/`'MM'`, or a refusal when the
/// caller did not supply one (Perl's module default is never guessed).
fn session_order(session: &Session) -> Result<super::session::ByteOrder, HelperError> {
    session.byte_order.ok_or(HelperError::Refused(
        "GetByteOrder() with no Session byte order",
    ))
}

/// Whether Perl's `$` (no `/m`) holds at `p` in `s`: the end, or just
/// before a final newline.
fn dollar_at(s: &[u8], p: usize) -> bool {
    p == s.len() || (p + 1 == s.len() && s[p] == b'\n')
}

/// `$s =~ /[\0 ]+$/` anchored at the start of `s` (the rest of an id).
fn nul_space_run_to_dollar(s: &[u8]) -> bool {
    let k = s.iter().take_while(|&&b| b == 0 || b == b' ').count();
    // `[\0 ]+` may stop at any p in 1..=k; `$` holds there only at the
    // end or before a final newline, which the run itself cannot contain.
    (1..=k).any(|p| dollar_at(s, p))
}

/// `$str =~ s/ +$//` (Perl's `$`: a run of spaces at the end, or just
/// before a final newline, which stays).
fn trim_spaces_before_dollar(s: &[u8]) -> Vec<u8> {
    let (body, nl) = match s.strip_suffix(b"\n") {
        Some(body) => (body, true),
        None => (s, false),
    };
    let kept = body.len() - body.iter().rev().take_while(|&&b| b == b' ').count();
    if kept == body.len() {
        return s.to_vec();
    }
    let mut out = body[..kept].to_vec();
    if nl {
        out.push(b'\n');
    }
    out
}

/// `Image::ExifTool::Exif::ConvertExifText($et, $val, $asciiFlex, $tag)`
/// (Exif.pm:5554-5601, pinned 13.59): the 8-byte character-code header
/// (`ASCII\0\0\0`, `UNICODE\0`, `JIS\0\0\0\0\0`, or nulls/spaces) selects
/// the decode; the result has trailing blanks removed.
///
/// ```perl
/// return $val if length($val) < 8;
/// my $id = substr($val, 0, 8);
/// my $str = substr($val, 8);
/// my $type;
/// delete $$et{WrongByteOrder};
/// if ($$et{OPTIONS}{Validate} and $id =~ /^(ASCII|UNICODE|JIS)?\0* \0*$/) { ...Warn }
/// if ($id =~ /^(ASCII)?(\0|[\0 ]+$)/) {
///     $str =~ s/\0.*//s;
///     if ($asciiFlex and $asciiFlex eq '1') {
///         my $enc = $et->Options('CharsetEXIF');
///         $str = $et->Decode($str, $enc) if $enc;
///     }
/// } elsif ($id =~ /^(UNICODE)[\0 ]$/) {
///     $type = $1;
///     $str = $et->Decode($str, 'UTF16', 'Unknown');
/// } elsif ($id =~ /^(JIS)[\0 ]{5}$/) {
///     $type = $1;
///     $str = $et->Decode($str, 'JIS', 'Unknown');
/// } else {
///     $tag = $asciiFlex if $asciiFlex and $asciiFlex ne '1';
///     $et->Warn('Invalid EXIF text encoding' . ($tag ? " for $tag" : ''));
///     $str = $id . $str;
/// }
/// if ($$et{WrongByteOrder} and $$et{OPTIONS}{Validate}) { ...Warn }
/// $str =~ s/ +$//;
/// return $str;
/// ```
///
/// Refused: a Perl-true `Validate` option (its two extra warnings are not
/// modelled); whatever [`decode`] refuses.
pub fn convert_exif_text(
    session: &mut Session,
    val: &MemberVal,
    ascii_flex: &MemberVal,
    tag: &MemberVal,
) -> HelperResult {
    if val.perl_length().unwrap_or(0) < 8 {
        return Ok(val.clone());
    }
    if session.option("Validate").is_truthy() {
        return Err(HelperError::Refused("the Validate option's extra warnings"));
    }
    let bytes = val.perl_bytes();
    let (id, rest) = bytes.split_at(8);
    session.remove_member("WrongByteOrder");
    let ascii = (id.starts_with(b"ASCII") && {
        let r = &id[5..];
        r.first() == Some(&0) || nul_space_run_to_dollar(r)
    }) || id[0] == 0
        || nul_space_run_to_dollar(id);
    let str_val = if ascii {
        let cut = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let s = MemberVal::from_bytes(rest[..cut].to_vec());
        if ascii_flex.is_truthy() && ascii_flex.perl_bytes().as_ref() == b"1" {
            let enc = session.option("CharsetEXIF");
            if enc.is_truthy() {
                decode(
                    session,
                    &s,
                    &enc,
                    &MemberVal::Undef,
                    &MemberVal::Undef,
                    &MemberVal::Undef,
                )?
            } else {
                s
            }
        } else {
            s
        }
    } else if &id[..7] == b"UNICODE" && matches!(id[7], 0 | b' ') {
        let s = MemberVal::from_bytes(rest.to_vec());
        decode(
            session,
            &s,
            &MemberVal::Str("UTF16".into()),
            &MemberVal::Str("Unknown".into()),
            &MemberVal::Undef,
            &MemberVal::Undef,
        )?
    } else if &id[..3] == b"JIS" && id[3..].iter().all(|&b| b == 0 || b == b' ') {
        let s = MemberVal::from_bytes(rest.to_vec());
        decode(
            session,
            &s,
            &MemberVal::Str("JIS".into()),
            &MemberVal::Str("Unknown".into()),
            &MemberVal::Undef,
            &MemberVal::Undef,
        )?
    } else {
        let tag = if ascii_flex.is_truthy() && ascii_flex.perl_bytes().as_ref() != b"1" {
            ascii_flex
        } else {
            tag
        };
        let mut msg = b"Invalid EXIF text encoding".to_vec();
        if tag.is_truthy() {
            msg.extend_from_slice(b" for ");
            msg.extend_from_slice(&tag.perl_bytes());
        }
        session.warn(MemberVal::from_bytes(msg));
        MemberVal::from_bytes(bytes.to_vec())
    };
    Ok(MemberVal::from_bytes(trim_spaces_before_dollar(
        &str_val.perl_bytes(),
    )))
}

/// `Image::ExifTool::Exif::DecodeCFAPattern($self, $val)` (Exif.pm:5729-5751):
///
/// ```perl
/// if ($val =~ /^[0-6]+$/) {
///     $self->Warn('Incorrectly formatted CFAPattern', 1);
///     $val =~ tr/0-6/\x00-\x06/;
/// }
/// return $val unless length($val) >= 4;
/// my @a = unpack(GetByteOrder() eq 'II' ? 'v2C*' : 'n2C*', $val);
/// my $end = 2 + $a[0] * $a[1];
/// if ($end > @a) {
///     my ($x, $y) = unpack('n2',pack('v2',$a[0],$a[1]));
///     if (@a < 2 + $x * $y) {
///         $self->Warn('Invalid CFAPattern', 1);
///     } else {
///         ($a[0], $a[1]) = ($x, $y);
///     }
/// }
/// return "@a";
/// ```
pub fn decode_cfa_pattern(session: &mut Session, val: &MemberVal) -> HelperResult {
    let mut b = val.perl_bytes().into_owned();
    let digits = b.strip_suffix(b"\n").unwrap_or(&b);
    // `/^[0-6]+$/`: Perl's `$` also holds before a final newline.
    let ascii = !digits.is_empty() && digits.iter().all(|c| (b'0'..=b'6').contains(c));
    if ascii {
        session.warn_ignorable(MemberVal::Str("Incorrectly formatted CFAPattern".into()), 1);
        for c in &mut b {
            if (b'0'..=b'6').contains(c) {
                *c -= b'0';
            }
        }
    }
    if val.perl_length().unwrap_or(0) < 4 {
        // `$val` itself when `tr` did not touch it (an undef, or a number,
        // stays what it was).
        return Ok(if ascii {
            MemberVal::from_bytes(b)
        } else {
            val.clone()
        });
    }
    let order = session_order(session)?;
    let u16_at = |i: usize| match order {
        super::session::ByteOrder::LittleEndian => u16::from_le_bytes([b[i], b[i + 1]]),
        super::session::ByteOrder::BigEndian => u16::from_be_bytes([b[i], b[i + 1]]),
    };
    let mut a: Vec<i64> = vec![i64::from(u16_at(0)), i64::from(u16_at(2))];
    a.extend(b[4..].iter().map(|&c| i64::from(c)));
    let n = a.len() as i64;
    if 2 + a[0] * a[1] > n {
        let (x, y) = (
            i64::from((a[0] as u16).swap_bytes()),
            i64::from((a[1] as u16).swap_bytes()),
        );
        if n < 2 + x * y {
            session.warn_ignorable(MemberVal::Str("Invalid CFAPattern".into()), 1);
        } else {
            a[0] = x;
            a[1] = y;
        }
    }
    Ok(MemberVal::Str(
        a.iter().map(i64::to_string).collect::<Vec<_>>().join(" "),
    ))
}

/// `Image::ExifTool::Exif::PrintCFAPattern($val)` (Exif.pm:5756-5774):
///
/// ```perl
/// my @a = split ' ', $val;
/// return '<truncated data>' unless @a >= 2;
/// return '<zero pattern size>' unless $a[0] and $a[1];
/// my $end = 2 + $a[0] * $a[1];
/// return '<invalid pattern size>' if $end > @a;
/// my @cfaColor = qw(Red Green Blue Cyan Magenta Yellow White);
/// my ($pos, $rtnVal) = (2, '[');
/// for (;;) {
///     $rtnVal .= $cfaColor[$a[$pos]] || 'Unknown';
///     last if ++$pos >= $end;
///     ($pos - 2) % $a[1] and $rtnVal .= ',', next;
///     $rtnVal .= '][';
/// }
/// return $rtnVal . ']';
/// ```
///
/// Refused: a field that is not a canonical integer (`DecodeCFAPattern`,
/// the only producer of this `$val`, prints integers or returns a string
/// shorter than 4 bytes, which splits into single characters -- also
/// refused unless they are digits).
pub fn print_cfa_pattern(val: &MemberVal) -> HelperResult {
    let fields: Vec<Vec<u8>> = val
        .perl_bytes()
        .split(|&c| super::session::is_perl_space(c))
        .filter(|f| !f.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    if fields.len() < 2 {
        return Ok(MemberVal::Str("<truncated data>".into()));
    }
    let mut a = Vec::with_capacity(fields.len());
    for f in &fields {
        let s = std::str::from_utf8(f)
            .map_err(|_| HelperError::Refused("CFA field that is not an integer"))?;
        match s.parse::<i64>() {
            Ok(v) if v.to_string() == s => a.push(i128::from(v)),
            _ => return Err(HelperError::Refused("CFA field that is not an integer")),
        }
    }
    if a[0] == 0 || a[1] == 0 {
        return Ok(MemberVal::Str("<zero pattern size>".into()));
    }
    let end = 2 + a[0] * a[1];
    if end > a.len() as i128 {
        return Ok(MemberVal::Str("<invalid pattern size>".into()));
    }
    const COLORS: [&str; 7] = ["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"];
    let color = |pos: usize| -> &'static str {
        // `$a[$pos]` past the end is undef, whose index is 0.
        let i = a.get(pos).copied().unwrap_or(0);
        let k = if i < 0 { 7 + i } else { i };
        if (0..7).contains(&k) {
            COLORS[k as usize]
        } else {
            "Unknown"
        }
    };
    let mut out = String::from("[");
    let mut pos = 2usize;
    loop {
        out.push_str(color(pos));
        pos += 1;
        if pos as i128 >= end {
            break;
        }
        if (pos as i128 - 2) % a[1] != 0 {
            out.push(',');
            continue;
        }
        out.push_str("][");
    }
    out.push(']');
    Ok(MemberVal::Str(out))
}

/// `Image::ExifTool::Exif::PrintSFR($val)` (Exif.pm:5605-5623):
///
/// ```perl
/// return $val unless length $val > 4;
/// my ($n, $m) = (Get16u(\$val, 0), Get16u(\$val, 2));
/// my @cols = split /\0/, substr($val, 4), $n+1;
/// my $pos = length($val) - 8 * $n * $m;
/// return $val unless @cols == $n+1 and $pos >= 4;
/// pop @cols;
/// my ($i, $j);
/// for ($i=0; $i<$n; ++$i) {
///     my @rows;
///     for ($j=0; $j<$m; ++$j) {
///         push @rows, Image::ExifTool::GetRational64u(\$val, $pos + 8*($i+$j*$n));
///     }
///     $cols[$i] .= '=' . join(',',@rows) . '';
/// }
/// return join '; ', @cols;
/// ```
///
/// `Get16u`/`GetRational64u` read in `GetByteOrder()`, the session's.
pub fn print_sfr(session: &Session, val: &MemberVal) -> HelperResult {
    if val.perl_length().unwrap_or(0) <= 4 {
        return Ok(val.clone());
    }
    let order = session_order(session)?;
    let b = val.perl_bytes();
    let u16_at = |i: usize| match order {
        super::session::ByteOrder::LittleEndian => u16::from_le_bytes([b[i], b[i + 1]]),
        super::session::ByteOrder::BigEndian => u16::from_be_bytes([b[i], b[i + 1]]),
    };
    let u32_at = |i: usize| {
        let w = [b[i], b[i + 1], b[i + 2], b[i + 3]];
        match order {
            super::session::ByteOrder::LittleEndian => u32::from_le_bytes(w),
            super::session::ByteOrder::BigEndian => u32::from_be_bytes(w),
        }
    };
    let (n, m) = (usize::from(u16_at(0)), usize::from(u16_at(2)));
    // `split /\0/, $s, $n+1`: at most n+1 fields, trailing empties kept.
    let tail = &b[4..];
    let mut cols: Vec<Vec<u8>> = Vec::new();
    let mut at = 0;
    while cols.len() + 1 < n + 1 {
        match tail[at..].iter().position(|&c| c == 0) {
            Some(k) => {
                cols.push(tail[at..at + k].to_vec());
                at += k + 1;
            }
            None => break,
        }
    }
    cols.push(tail[at..].to_vec());
    let span = 8 * n * m;
    if cols.len() != n + 1 || span > b.len() || b.len() - span < 4 {
        return Ok(val.clone());
    }
    let pos = b.len() - span;
    cols.pop();
    for (i, col) in cols.iter_mut().enumerate() {
        let rows: Vec<String> = (0..m)
            .map(|j| {
                let p = pos + 8 * (i + j * n);
                super::runtime::perl_rational64(f64::from(u32_at(p)), f64::from(u32_at(p + 4)))
            })
            .collect();
        col.push(b'=');
        col.extend_from_slice(rows.join(",").as_bytes());
    }
    Ok(MemberVal::from_bytes(cols.join(&b"; "[..])))
}

/// `Image::ExifTool::ASF::GetGUID($val)` (ASF.pm, pinned 13.59):
///
/// ```perl
/// return $val unless length($val) == 16;
/// my $buff = unpack('H*',pack('NnnNN',unpack('VvvNN',$val)));
/// $buff =~ s/(.{8})(.{4})(.{4})(.{4})/$1-$2-$3-$4-/;
/// return uc($buff);
/// ```
#[must_use]
pub fn asf_get_guid(val: &MemberVal) -> MemberVal {
    if val.perl_length() != Some(16) {
        return val.clone();
    }
    let b = val.perl_bytes();
    let mut swapped = Vec::with_capacity(16);
    swapped.extend(b[0..4].iter().rev());
    swapped.extend(b[4..6].iter().rev());
    swapped.extend(b[6..8].iter().rev());
    swapped.extend_from_slice(&b[8..16]);
    let hex: String = swapped.iter().map(|c| format!("{c:02X}")).collect();
    MemberVal::Str(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

/// `$self->Printable($outStr [, $maxLen])` (ExifTool.pm, pinned 13.59) for
/// a plain scalar:
///
/// ```perl
/// return '(undef)' unless defined $outStr;
/// $outStr =~ tr/\x01-\x1f\x7f-\xff/./;
/// $outStr =~ s/\x00//g;
/// my $verbose = $$self{OPTIONS}{Verbose};
/// if ($verbose < 4) {
///     if ($maxLen) {
///         $maxLen = 20 if $maxLen < 20;   # minimum length is 20
///     } elsif (defined $maxLen) {
///         $maxLen = length $outStr;       # 0 is unlimited
///     } else {
///         $maxLen = 60;                   # default maximum is 60
///     }
/// } else {
///     $maxLen = length $outStr;
///     $maxLen = 2048 if $maxLen > 2048 and $verbose < 5;
/// }
/// $outStr = substr($outStr,0,$maxLen-6) . '[snip]' if length($outStr) > $maxLen;
/// return $outStr;
/// ```
///
/// (The SCALAR-reference branch is never reached by a conversion's
/// `$val`.) Refused: a `Verbose` option or `$maxLen` that is not an integer
/// in numeric context.
pub fn printable(session: &Session, val: &MemberVal, max_len: &MemberVal) -> HelperResult {
    if !val.is_defined() {
        return Ok(MemberVal::Str("(undef)".into()));
    }
    let s: Vec<u8> = val
        .perl_bytes()
        .iter()
        .map(|&c| {
            if (1..=0x1f).contains(&c) || c >= 0x7f {
                b'.'
            } else {
                c
            }
        })
        .filter(|&c| c != 0)
        .collect();
    let int = |v: &MemberVal| match v.perl_num() {
        PerlNum::Int(i) => Ok(i),
        PerlNum::Float(_) => Err(HelperError::Refused(
            "Printable: a non-integer length or Verbose",
        )),
    };
    let verbose = int(&session.option("Verbose"))?;
    let len = s.len() as i64;
    let max = if verbose < 4 {
        if max_len.is_truthy() {
            int(max_len)?.max(20)
        } else if max_len.is_defined() {
            len
        } else {
            60
        }
    } else if len > 2048 && verbose < 5 {
        2048
    } else {
        len
    };
    if len > max {
        let keep = usize::try_from(max - 6)
            .map_err(|_| HelperError::Refused("Printable: negative substr"))?;
        let mut out = s[..keep.min(s.len())].to_vec();
        out.extend_from_slice(b"[snip]");
        return Ok(MemberVal::from_bytes(out));
    }
    Ok(MemberVal::from_bytes(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};

    const CAPTURE: &str =
        include_str!("../../tools/exiftool-tables/testdata/helper_oracle_outputs.json");

    fn capture() -> Value {
        serde_json::from_str(CAPTURE).expect("capture is JSON")
    }

    fn hex_bytes(h: &str) -> Vec<u8> {
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex"))
            .collect()
    }

    fn arg(v: &Value) -> MemberVal {
        match v["t"].as_str().expect("type") {
            "undef" => MemberVal::Undef,
            "s" => MemberVal::from_bytes(hex_bytes(v["hex"].as_str().expect("hex"))),
            "i" => MemberVal::Int(v["v"].as_str().expect("v").parse().expect("int")),
            "f" => MemberVal::Float(v["v"].as_str().expect("v").parse().expect("float")),
            t => panic!("arg type {t}"),
        }
    }

    /// Perl's stringification of one output, as the harness recorded it.
    fn out_bytes(v: &MemberVal) -> Option<Vec<u8>> {
        v.is_defined().then(|| v.perl_bytes().into_owned())
    }

    fn expected(o: &Value) -> Option<Vec<u8>> {
        o.get("hex").map(|h| hex_bytes(h.as_str().expect("hex")))
    }

    /// What a port did to the Session besides returning: the members it
    /// created, changed or deleted (`Some(None)` for `undef`, `None` for
    /// deleted) and the `Warn` requests it made with their `$ignorable`, in
    /// the harness's shape (`set_members` sorted by key, `warnings` in
    /// order).
    type SideEffects = (
        BTreeMap<String, Option<Option<Vec<u8>>>>,
        Vec<(Option<Vec<u8>>, i64)>,
    );

    fn side_effects(before: &Session, after: &Session) -> SideEffects {
        let mut set: BTreeMap<String, Option<Option<Vec<u8>>>> = after
            .member_names()
            .filter(|k| !before.has_member(k) || before.member(k) != after.member(k))
            .map(|k| (k.to_string(), Some(out_bytes(&after.member(k)))))
            .collect();
        for k in before.member_names() {
            if !after.has_member(k) {
                set.insert(k.to_string(), None);
            }
        }
        let warned = after.warnings()[before.warnings().len()..]
            .iter()
            .map(|w| (out_bytes(&w.message), w.ignorable))
            .collect();
        (set, warned)
    }

    fn expected_side_effects(case: &Value) -> SideEffects {
        let set = case
            .get("set_members")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        let v = (v.get("t").and_then(Value::as_str) != Some("deleted"))
                            .then(|| expected(v));
                        (k.clone(), v)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let warned = case
            .get("warnings")
            .and_then(Value::as_array)
            .map(|w| {
                w.iter()
                    .map(|w| {
                        let ign = w
                            .get("ignorable")
                            .and_then(Value::as_str)
                            .map_or(0, |v| v.parse().expect("ignorable"));
                        (expected(w), ign)
                    })
                    .collect()
            })
            .unwrap_or_default();
        (set, warned)
    }

    /// Run one captured case through the port. `Ok(outputs, side effects)`
    /// or the port's refusal / death.
    fn run(helper: &str, case: &Value) -> Result<(Vec<MemberVal>, SideEffects), HelperError> {
        let args: Vec<MemberVal> = case["args"]
            .as_array()
            .expect("args")
            .iter()
            .map(arg)
            .collect();
        let a = |i: usize| args.get(i).cloned().unwrap_or(MemberVal::Undef);
        let mut session = Session::new();
        if let Some(opts) = case.get("options").and_then(Value::as_object) {
            for (k, v) in opts {
                session.set_option(k, arg(v));
            }
        }
        if let Some(members) = case.get("members").and_then(Value::as_object) {
            for (k, v) in members {
                session.set_member(k, arg(v)).expect("untyped member");
            }
        }
        session.byte_order = match case.get("byte_order").and_then(Value::as_str) {
            Some("II") => Some(super::super::session::ByteOrder::LittleEndian),
            Some("MM") => Some(super::super::session::ByteOrder::BigEndian),
            None => None,
            Some(o) => panic!("byte order {o}"),
        };
        let before = session.clone();
        let mut mutating = session.clone();
        let effects =
            |r: HelperResult, after: &Session| r.map(|v| (vec![v], side_effects(&before, after)));
        match helper {
            "Image::ExifTool::Decode" => {
                let r = decode(&mut mutating, &a(0), &a(1), &a(2), &a(3), &a(4));
                return effects(r, &mutating);
            }
            "Image::ExifTool::Encode" => {
                let r = encode(&mut mutating, &a(0), &a(1), &a(2));
                return effects(r, &mutating);
            }
            "Image::ExifTool::Exif::ConvertExifText" => {
                let r = convert_exif_text(&mut mutating, &a(0), &a(1), &a(2));
                return effects(r, &mutating);
            }
            "Image::ExifTool::Exif::DecodeCFAPattern" => {
                let r = decode_cfa_pattern(&mut mutating, &a(0));
                return effects(r, &mutating);
            }
            _ => {}
        }
        let none = || side_effects(&before, &before);
        let one = |r: HelperResult| r.map(|v| (vec![v], none()));
        match helper {
            "Image::ExifTool::ConvertDateTime" => one(convert_date_time(&session, &a(0))),
            "Image::ExifTool::Exif::ConvertFraction" => one(convert_fraction(&a(0))),
            "Image::ExifTool::GPS::ToDMS" => one(to_dms(&session, &a(0), &a(1), &a(2))),
            "Image::ExifTool::Exif::PrintExposureTime" => one(print_exposure_time(&a(0))),
            "Image::ExifTool::ConvertUnixTime" => {
                one(convert_unix_time(&session, &a(0), &a(1), &a(2)))
            }
            "Image::ExifTool::ConvertDuration" => one(convert_duration(&a(0))),
            "Image::ExifTool::IsFloat" => {
                let (r, after) = is_float(&a(0));
                Ok((vec![r, after], none()))
            }
            "Image::ExifTool::ConvertBitrate" => one(convert_bitrate(&a(0))),
            "Image::ExifTool::Exif::PrintFraction" => one(print_fraction(&a(0))),
            "Image::ExifTool::GPS::ToDegrees" => one(to_degrees(&a(0), &a(1), &a(2))),
            "Image::ExifTool::Canon::CanonEv" => one(canon_ev(&a(0))),
            "Image::ExifTool::Canon::CanonEvInv" => one(canon_ev_inv(&a(0))),
            "Image::ExifTool::GetUnixTime" => one(get_unix_time(&a(0), &a(1))),
            "Image::ExifTool::Exif::PrintFNumber" => one(print_f_number(&a(0))),
            "Image::ExifTool::IsInt" => Ok((vec![is_int(&a(0))], none())),
            "Image::ExifTool::XMP::ConvertXMPDate" => {
                Ok((vec![convert_xmp_date(&a(0), &a(1))], none()))
            }
            "Image::ExifTool::ConvertFileSize" => {
                let with = case.get("with_session").is_some();
                one(convert_file_size(&a(0), with.then_some(&session)))
            }
            "Image::ExifTool::Exif::PrintCFAPattern" => one(print_cfa_pattern(&a(0))),
            "Image::ExifTool::Exif::PrintSFR" => one(print_sfr(&session, &a(0))),
            "Image::ExifTool::ASF::GetGUID" => Ok((vec![asf_get_guid(&a(0))], none())),
            "Image::ExifTool::Printable" => {
                // `$self->Printable($str)` and `$self->Printable($str, $max)`:
                // an absent second argument is `undef` either way.
                one(printable(&session, &a(0), &a(1)))
            }
            other => panic!("no dispatch for {other}"),
        }
    }

    /// `$toLocal` renders in the host zone on both sides (localtime /
    /// chrono::Local); the capture was taken under TZ=UTC, so those probes
    /// are only comparable on a host whose zone is UTC (CI runners). They
    /// are counted, never silently passed.
    fn host_zone_is_utc() -> bool {
        use chrono::{Offset, TimeZone};
        [0_i64, 1_234_567_890, 1_593_561_600].iter().all(|&t| {
            chrono::Local
                .timestamp_opt(t, 0)
                .single()
                .is_some_and(|d| d.offset().fix().local_minus_utc() == 0)
        })
    }

    fn host_zone_dependent(helper: &str, case: &Value) -> bool {
        if helper != "Image::ExifTool::ConvertUnixTime" {
            return false;
        }
        let args = case["args"].as_array().expect("args");
        let local = args.get(1).map(arg).is_some_and(|v| v.is_truthy());
        let keep_utc = case
            .get("options")
            .and_then(|o| o.get("KeepUTCTime"))
            .map(arg)
            .is_some_and(|v| v.is_truthy());
        local && !keep_utc
    }

    /// The differential test: every captured probe of every admitted port
    /// reproduces the pinned Perl's bytes, or refuses, or dies where Perl
    /// died. A wrong answer is a failure with the probe printed.
    #[test]
    fn every_port_matches_the_pinned_perl_capture() {
        let cap = capture();
        let helpers = cap["helpers"].as_object().expect("helpers");
        let utc = host_zone_is_utc();
        let mut failures: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        let mut refused: BTreeMap<&str, (usize, BTreeSet<&str>)> = BTreeMap::new();
        let (mut checked, mut matched, mut zone_skipped) = (0usize, 0usize, 0usize);
        for port in PORTS {
            let h = &helpers[port.perl];
            for case in h["cases"].as_array().expect("cases") {
                checked += 1;
                if !utc && host_zone_dependent(port.perl, case) {
                    zone_skipped += 1;
                    continue;
                }
                let got = run(port.perl, case);
                let perl_died = case.get("die").is_some();
                let fail = |why: String| {
                    format!(
                        "args {} opts {} {why}",
                        case["args"],
                        case.get("options").unwrap_or(&Value::Null)
                    )
                };
                match got {
                    Err(HelperError::Refused(why)) => {
                        let e = refused.entry(port.perl).or_default();
                        e.0 += 1;
                        e.1.insert(why);
                    }
                    Err(HelperError::Dies(_)) if perl_died => matched += 1,
                    Err(HelperError::Dies(why)) => failures
                        .entry(port.perl)
                        .or_default()
                        .push(fail(format!("dies ({why}); perl did not"))),
                    Ok(_) if perl_died => failures
                        .entry(port.perl)
                        .or_default()
                        .push(fail("returned; perl died".to_string())),
                    Ok((outs, effects)) => {
                        let want: Vec<Option<Vec<u8>>> = case["out"]
                            .as_array()
                            .expect("out")
                            .iter()
                            .map(expected)
                            .collect();
                        let have: Vec<Option<Vec<u8>>> = outs.iter().map(out_bytes).collect();
                        let want_effects = expected_side_effects(case);
                        if want == have && want_effects == effects {
                            matched += 1;
                        } else {
                            failures.entry(port.perl).or_default().push(fail(format!(
                                "perl {want:02x?} {want_effects:02x?} rust {have:02x?} {effects:02x?}"
                            )));
                        }
                    }
                }
            }
        }
        for (h, (n, why)) in &refused {
            eprintln!("refused {n:5}  {h}  {why:?}");
        }
        let nfail: usize = failures.values().map(Vec::len).sum();
        eprintln!(
            "checked {checked} probes: {matched} byte-identical, {} refused, \
             {zone_skipped} host-zone-dependent skipped (host zone UTC: {utc}), {nfail} mismatches",
            refused.values().map(|r| r.0).sum::<usize>()
        );
        let report: Vec<String> = failures
            .iter()
            .flat_map(|(h, v)| {
                std::iter::once(format!("== {h}: {} mismatches", v.len()))
                    .chain(v.iter().take(12).cloned())
            })
            .collect();
        assert!(
            nfail == 0,
            "{nfail} probes differ from the pinned perl:\n{}",
            report.join("\n")
        );
    }

    /// Exact source: each port names the digest of the sub it was proven
    /// against, and the capture (taken from the pinned tree) agrees. A
    /// helper the capture marks ported must be in PORTS and vice versa.
    #[test]
    fn every_port_names_the_pinned_source_it_was_proven_against() {
        let cap = capture();
        let helpers = cap["helpers"].as_object().expect("helpers");
        for port in PORTS {
            let h = &helpers[port.perl];
            assert_eq!(h["status"], "ported", "{}", port.perl);
            assert_eq!(h["module"], port.module, "{}", port.perl);
            assert_eq!(
                h["source_sha256"].as_str(),
                Some(port.source_sha256),
                "{}: pinned source differs from the one this port was proven against",
                port.perl
            );
        }
        let ported: BTreeSet<&str> = PORTS.iter().map(|p| p.perl).collect();
        let refused: BTreeSet<&str> = REFUSED_HELPERS.iter().map(|(p, _)| *p).collect();
        for (name, h) in helpers {
            match h["status"].as_str() {
                Some("ported") => assert!(ported.contains(name.as_str()), "{name} not in PORTS"),
                Some("refused") => assert!(
                    refused.contains(name.as_str()),
                    "{name} not in REFUSED_HELPERS"
                ),
                s => panic!("{name}: status {s:?}"),
            }
        }
        assert_eq!(ported.len() + refused.len(), helpers.len());
    }

    #[test]
    fn truthiness_matches_the_pinned_perl_capture() {
        let cap = capture();
        for t in cap["truthiness"].as_array().expect("truthiness") {
            let v = arg(&t["value"]);
            let want = expected(&t["out"][0]).expect("defined") == b"1";
            assert_eq!(v.is_truthy(), want, "{v:?}");
        }
    }

    /// `Session::new()`'s options are `Image::ExifTool->new`'s, as the
    /// pinned perl reports them (string context; `undef` absent).
    #[test]
    fn session_option_defaults_match_the_pinned_perl() {
        let cap = capture();
        let session = Session::new();
        for (name, want) in cap["option_defaults"].as_object().expect("option_defaults") {
            assert_eq!(out_bytes(&session.option(name)), expected(want), "{name}");
        }
    }

    /// Every inline-reproduced engine sub a port names has the digest the
    /// capture recorded from the pinned tree.
    #[test]
    fn port_dependencies_match_the_capture() {
        let cap = capture();
        for (port, module, sub, digest) in PORT_DEPENDENCIES {
            assert!(PORTS.iter().any(|p| p.perl == *port), "{port} not in PORTS");
            assert_eq!(
                cap["helpers"][*port]["dependencies"][&format!("{module}::{sub}")].as_str(),
                Some(*digest),
                "{port}: {module}::{sub}"
            );
        }
    }

    /// Decode's dependencies (Charset.pm subs) and the generated charset
    /// tables name the same pinned sources as the capture.
    #[test]
    fn decode_dependencies_and_charset_tables_match_the_capture() {
        let cap = capture();
        for helper in ["Image::ExifTool::Decode", "Image::ExifTool::Encode"] {
            let deps = cap["helpers"][helper]["dependencies"]
                .as_object()
                .expect("dependencies");
            for (module, sub, digest) in DECODE_DEPENDENCIES {
                assert_eq!(
                    deps[&format!("{module}::{sub}")].as_str(),
                    Some(*digest),
                    "{helper}: {module}::{sub}"
                );
            }
        }
        let decode = PORTS
            .iter()
            .find(|p| p.perl == "Image::ExifTool::Decode")
            .expect("Decode");
        assert_eq!(
            cap["helpers"]["Image::ExifTool::Encode"]["dependencies"]["Image/ExifTool.pm::Decode"]
                .as_str(),
            Some(decode.source_sha256)
        );
        let sources = cap["charset_sources"].as_object().expect("charset_sources");
        assert_eq!(sources.len(), super::super::charset_tables::SOURCES.len());
        for (path, digest) in super::super::charset_tables::SOURCES {
            assert_eq!(sources[*path].as_str(), Some(*digest), "{path}");
        }
        assert_eq!(
            super::super::charset_tables::EXIFTOOL_VERSION,
            cap["capture"]["exiftool_version"]
        );
        let perl = cap["perl_sources"].as_object().expect("perl_sources");
        assert_eq!(perl.len(), super::super::charset_tables::PERL_SOURCES.len());
        for (path, digest) in super::super::charset_tables::PERL_SOURCES {
            assert_eq!(perl[*path].as_str(), Some(*digest), "{path}");
        }
    }
}
