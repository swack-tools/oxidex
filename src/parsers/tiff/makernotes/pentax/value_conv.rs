//! The `ValueConv` computations of `%Pentax`'s binary sub-tables.
//!
//! A `ValueConv` is arithmetic, not a lookup, so unlike a `PrintConv` hash it
//! cannot be carried across as data -- it has to be ported. Each function here
//! quotes the ExifTool source it was written against, and
//! `codegen_subdirs.py` binds it by that exact text: if the Perl changes
//! upstream the generator stops rather than leaving one of these behind a real
//! tag name.

/// ExifTool's `PentaxEv()` (Pentax.pm:6822): converts a raw hex-based EV code
/// (modulo 8) into an EV value, correcting for the fact that 1/3-stop
/// increments don't divide evenly by 8.
///
/// Shared by the four `ValueConv`s below and by `%Pentax::AEInfo`'s
/// `FlashExposureCompSet`, which is still hand-written in `pentax.rs`.
pub(super) fn pentax_ev(val: i32) -> f64 {
    let mut v = val as f64;
    if val & 1 != 0 {
        let sign: f64 = if val < 0 { -1.0 } else { 1.0 };
        let frac = ((val as f64) * sign) as i64 & 0x07;
        if frac == 3 {
            v += sign * (8.0 / 3.0 - frac as f64);
        } else if frac == 5 {
            v += sign * (16.0 / 3.0 - frac as f64);
        }
    }
    v / 8.0
}

/// `ValueConv => 'int(100*exp(Image::ExifTool::Pentax::PentaxEv($val-32)*log(2))+0.5)'`
/// (Pentax.pm:3499 `ISOFloor`, :3756 `SvISOSetting`).
///
/// Perl's `int()` truncates toward zero; the result is always positive here,
/// so `.trunc()` matches it and leaves the shared `render()` float path's
/// `fract() == 0.0` check true, printing a plain integer rather than a
/// long decimal.
pub(super) fn iso_from_pentax_ev(value: f64) -> f64 {
    (100.0 * (pentax_ev(value as i32 - 32) * std::f64::consts::LN_2).exp() + 0.5).trunc()
}

/// `ValueConv => 'exp(-Image::ExifTool::Pentax::PentaxEv($val-68)*log(2))'`
/// (Pentax.pm:3732 `TvExposureTimeSetting`), an exposure time in seconds.
pub(super) fn tv_from_pentax_ev(value: f64) -> f64 {
    (-pentax_ev(value as i32 - 68) * std::f64::consts::LN_2).exp()
}

/// `ValueConv => 'exp(Image::ExifTool::Pentax::PentaxEv($val-68)*log(2)/2)'`
/// (Pentax.pm:3741 `AvApertureSetting`), an f-number.
pub(super) fn av_from_pentax_ev(value: f64) -> f64 {
    (pentax_ev(value as i32 - 68) * std::f64::consts::LN_2 / 2.0).exp()
}

/// `ValueConv => 'Image::ExifTool::Pentax::PentaxEv(64-$val)'` (Pentax.pm:3763
/// `BaseExposureCompensation`).
pub(super) fn base_exposure_comp_from_pentax_ev(value: f64) -> f64 {
    pentax_ev(64 - value as i32)
}

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

/// `ValueConv => '$val / 100'` (Pentax.pm:4866 `BodyBatteryVoltage1`, :4921
/// `BodyBatteryVoltage2`, :4946 `BodyBatteryVoltage3`, :4956
/// `BodyBatteryVoltage4`).
///
/// The record holds centivolts, so a raw 686 is 6.86 V.
pub(super) fn div_100(value: f64) -> f64 {
    value / 100.0
}

/// `ValueConv => '$val * 4e-8 + 0.27219'` (Pentax.pm:4930
/// `BodyBatteryVoltage`, :4984 `GripBatteryVoltage`).
///
/// The K-3 Mark III reports a raw 32-bit ADC count rather than centivolts.
pub(super) fn k3_iii_voltage(value: f64) -> f64 {
    value * 4e-8 + 0.27219
}

/// `ValueConv => '$val / 10'` (Pentax.pm:6129 `SensorTemperature`, :6138
/// `SensorTemperature2`, :6162 the K-3 III's `SensorTemperature`).
pub(super) fn div_10(value: f64) -> f64 {
    value / 10.0
}

/// `ValueConv => '$val * 2'` (Pentax.pm:5062 `AFIntegrationTime`).
///
/// "effective exposure time for AF sensors in 2 ms increments"
/// (Pentax.pm:5059), which is why a sub-2 ms exposure reports 0.
pub(super) fn times_2(value: f64) -> f64 {
    value * 2.0
}

/// `ValueConv => '$val+1'` (Pentax.pm:6118 `ShotNumber`).
///
/// "Internal representation starts at 0 for the 1st shot" (Pentax.pm:6117).
pub(super) fn plus_1(value: f64) -> f64 {
    value + 1.0
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

    /// A raw byte of 32 is `PentaxEv(0)`, exactly 0 EV, so `exp(0)` collapses
    /// the whole formula to `100 + 0.5` truncated -- the one input where the
    /// odd/even correction inside `PentaxEv` cannot matter.
    #[test]
    fn iso_from_pentax_ev_matches_the_zero_ev_point() {
        assert_eq!(iso_from_pentax_ev(32.0), 100.0);
        // 40 - 32 = 8, an even code: no 1/3-stop correction, EV = 1 exactly.
        assert_eq!(iso_from_pentax_ev(40.0), 200.0);
    }

    /// A raw byte of 68 is `PentaxEv(0)` for both formulas, so each collapses
    /// to `exp(0)` -- 1 second and f/1.0 respectively.
    #[test]
    fn tv_and_av_from_pentax_ev_match_the_zero_ev_point() {
        assert_eq!(tv_from_pentax_ev(68.0), 1.0);
        assert_eq!(av_from_pentax_ev(68.0), 1.0);
    }

    #[test]
    fn base_exposure_comp_from_pentax_ev_matches_the_zero_ev_point() {
        assert_eq!(base_exposure_comp_from_pentax_ev(64.0), 0.0);
        // 64 - 72 = -8, an even code: EV = -1 exactly.
        assert_eq!(base_exposure_comp_from_pentax_ev(72.0), -1.0);
    }
}
