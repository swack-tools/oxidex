//! The `ValueConv` computations of `%Pentax`'s binary sub-tables.
//!
//! A `ValueConv` is arithmetic, not a lookup, so unlike a `PrintConv` hash it
//! cannot be carried across as data -- it has to be ported. Each function here
//! quotes the ExifTool source it was written against, and
//! `codegen_subdirs.py` binds it by that exact text: if the Perl changes
//! upstream the generator stops rather than leaving one of these behind a real
//! tag name.

/// `ValueConv => '-$val'` (Pentax.pm:5744 `CompositionAdjustX`, :5750
/// `CompositionAdjustY`).
///
/// The camera counts steps in the opposite sense to the direction the tag is
/// documented in ("steps to the right", "steps up").
pub(super) fn negate(value: f64) -> f64 {
    -value
}

/// `ValueConv => '-$val / 2'` (Pentax.pm:5734 `RollAngle`, :5740 `PitchAngle`,
/// :5760 `CompositionAdjustRotation`).
///
/// Half-degree steps, again in the opposite sense -- so an int8s of 3 is
/// -1.5 degrees, not 3.
pub(super) fn negate_half(value: f64) -> f64 {
    -value / 2.0
}

/// `%kelvinWB`'s `ValueConv` (Pentax.pm:837-840), shared by all 17
/// `KelvinWB_*` tags:
///
/// ```text
/// my @a = split ' ', shift;
/// (53190 - $a[0]) . ' ' . $a[1] . ' ' . ($a[2] / 8192) . ' ' . ($a[3] / 8192);
/// ```
///
/// The four `int16u` slots are not alike, which is why this is one conversion
/// over the list rather than four scalar ones: the first is a colour
/// temperature stored as its complement against 53190, the second passes
/// through, and the last two are 13-bit fixed-point multipliers.
pub(super) fn kelvin_wb(values: &[f64]) -> Vec<f64> {
    if values.len() < 4 {
        // ExifTool's `split` would yield fewer fields and the concatenation
        // would read past them; a short record reports nothing instead.
        return Vec::new();
    }
    vec![
        53190.0 - values[0],
        values[1],
        values[2] / 8192.0,
        values[3] / 8192.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negations_match_exiftool() {
        assert!((negate(-105.0) - 105.0).abs() < f64::EPSILON);
        assert!((negate_half(3.0) - -1.5).abs() < f64::EPSILON);
    }

    /// `exiftool -a -G1 -s` reports
    /// `KelvinWB_Daylight : 5205 0 1.647705078125 1.1788330078125` for a K-5 in
    /// the corpus, so the record holds 47985 and the two multipliers as 13-bit
    /// fixed point.
    #[test]
    fn kelvin_wb_matches_a_real_reading() {
        let out = kelvin_wb(&[47985.0, 0.0, 13498.0, 9657.0]);
        assert_eq!(out[0], 5205.0);
        assert_eq!(out[1], 0.0);
        assert_eq!(out[2], 1.647_705_078_125);
        assert_eq!(out[3], 1.178_833_007_812_5);
    }

    #[test]
    fn kelvin_wb_refuses_a_short_record() {
        assert!(kelvin_wb(&[1.0, 2.0, 3.0]).is_empty());
    }
}
