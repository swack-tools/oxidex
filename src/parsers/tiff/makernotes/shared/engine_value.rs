//! The string an IFD-engine value is stored as by a MakerNote parser whose
//! output is a `HashMap<String, String>` (Olympus slice I-2, Canon slice
//! I-5). One mapping for every such call site, so two vendors routed through
//! the generated tables cannot render the same engine shape two ways.

use crate::core::TagValue;

use super::table_ifd;

/// The string the hand walks (`table_ifd::apply_conv`, Canon's scalar arms)
/// would have produced for an engine value of the same shape, so that
/// switching the producer changes no byte of the map:
///
/// * `String` -- itself. This is also every multi-count numeric entry (the
///   engine space-joins them, ExifTool.pm:6330), every rendered `PrintConv`,
///   and a `Binary` tag's `(Binary data N bytes, use -b option to extract)`
///   placeholder (`ifd_engine::walk` builds it as a `String`).
/// * `Integer` -- decimal, as `OlyVal::Int`'s `print_raw`.
/// * `Float` -- Perl's `%.15g` (`exprs::perl_num`), as `OlyVal::Float`'s
///   `fmt_g15`. This is every unconverted rational with a nonzero
///   denominator: the engine reads a 64-bit rational as the number Perl
///   parses from `RoundFloat($n/$d, 10)` = `sprintf("%.10g")`
///   (`GetRational64s`, ExifTool.pm:6107-6120, 5960-5964;
///   `ifd_engine::round_rationals`), and `%.15g` of that number prints the
///   same ten digits, which is what `table_ifd::print_rational` printed for
///   the Olympus hand rows (0x1003, 0x1006, 0x1023, 0x1025, 0x103d, 0x103e).
/// * `Rational` -- only a zero denominator reaches this arm now;
///   `table_ifd::print_rational` prints it as ExifTool does (`inf` for a
///   nonzero numerator, `undef` for zero, ExifTool.pm:6111/6118).
/// * anything else -- dropped. `Binary` is an `undef` run with no
///   conversion (Olympus::Main 0x0000 `MakerNoteVersion`, which no Olympus
///   note in the corpus carries): the string map cannot say whether its
///   bytes are text, and guessing is worse than the absent tag. `Array`
///   only arises for `List => 1` tags, of which neither `Olympus::Main` nor
///   `Canon::Main` has any; `DateTime`/`Structure` cannot come out of an IFD
///   walk.
#[must_use]
pub fn engine_value_text(value: &TagValue) -> Option<String> {
    match value {
        TagValue::String(s) => Some(s.clone()),
        TagValue::Integer(n) => Some(n.to_string()),
        TagValue::Float(f) => Some(crate::exiftool_tables::exprs::perl_num(*f)),
        TagValue::Rational {
            numerator,
            denominator,
        } => Some(table_ifd::print_rational(
            i64::from(*numerator),
            i64::from(*denominator),
        )),
        _ => None,
    }
}
