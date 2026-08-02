//! The expression `PrintConv`s of `%Pentax`'s binary sub-tables.
//!
//! A `PrintConv` that is a Perl expression rather than a hash is a computation,
//! so like a `ValueConv` it has to be ported rather than carried as data. Each
//! function quotes the ExifTool source it was written against and
//! `codegen_subdirs.py` binds it by that exact text, so an upstream edit stops
//! the generator instead of leaving one of these behind a real tag name.
//!
//! Every one of these runs on the value *after* `ValueConv`, which is what
//! `$val` is at `PrintConv` time.

use crate::core::formatters::numeric_precision::perl_number;

/// `PrintConv => 'sprintf("%.2f V", $val)'` (Pentax.pm:4868, :4923, :4932,
/// :4948, :4958, :4986 -- every `BodyBattery*`/`GripBatteryVoltage`).
pub(super) fn volts_2dp(value: f64) -> String {
    format!("{value:.2} V")
}

/// `PrintConv => 'sprintf("%d (%.1fV, %d%%)",$val,$val*8.18/186,($val-155)*100/35)'`
/// (Pentax.pm:4854 `BodyBatteryADNoLoad`, K10D/K20D).
///
/// ExifTool's own calibration note: "DVM readings: 8.18V=186, 8.42-8.40V=192
/// (full), 6.86V=155 (empty)" (Pentax.pm:4853). Perl's `%d` truncates toward
/// zero, so the percentage is not rounded.
pub(super) fn ad_no_load(value: f64) -> String {
    let volts = value * 8.18 / 186.0;
    let percent = (value - 155.0) * 100.0 / 35.0;
    format!("{} ({volts:.1}V, {}%)", value as i64, percent as i64)
}

/// `PrintConv => 'sprintf("%d (%.1fV, %d%%)",$val,$val*8.18/186,($val-152)*100/34)'`
/// (Pentax.pm:4898 `BodyBatteryADLoad`, K10D/K20D).
///
/// The same shape as [`ad_no_load`] against a different empty-battery reading,
/// which is why it is a second function and not a parameter: the constants are
/// ExifTool's, and a shared helper would invite editing one of them.
pub(super) fn ad_load(value: f64) -> String {
    let volts = value * 8.18 / 186.0;
    let percent = (value - 152.0) * 100.0 / 34.0;
    format!("{} ({volts:.1}V, {}%)", value as i64, percent as i64)
}

/// `PrintConv => '"$val ms"'` (Pentax.pm:5064 `AFIntegrationTime`).
///
/// Perl interpolates a number with its `%.15g` stringification, which is what
/// [`perl_number`] reproduces -- `format!("{value}")` would print `0` as `0`
/// but a computed `2.5` as `2.5` and a large one in a different exponent form.
pub(super) fn millis(value: f64) -> String {
    format!("{} ms", perl_number(value))
}

/// `PrintConv => '"$val C"'` (Pentax.pm:6147 `CameraTemperature4`, :6154
/// `CameraTemperature5`).
pub(super) fn celsius(value: f64) -> String {
    format!("{} C", perl_number(value))
}

/// `PrintConv => 'sprintf("%.1f C", $val)'` (Pentax.pm:6131
/// `SensorTemperature`, :6140 `SensorTemperature2`, :6164 the K-3 III's
/// `SensorTemperature`).
pub(super) fn celsius_1dp(value: f64) -> String {
    format!("{value:.1} C")
}

/// `PrintConv => '5 - $val'` (Pentax.pm:5188 `AFCSensitivity`).
///
/// A `PrintConv` that returns a number rather than text; ExifTool prints the
/// result as-is.
pub(super) fn five_minus(value: f64) -> String {
    perl_number(5.0 - value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `exiftool -a -G1 -s combined-samples/Pentax/PentaxK-5IIs.jpg` reports
    /// `BodyBatteryVoltage1 : 6.86 V` from a raw 686, and `BodyBatteryVoltage2
    /// : 6.55 V` from 655.
    #[test]
    fn voltages_print_two_decimals() {
        assert_eq!(volts_2dp(6.86), "6.86 V");
        assert_eq!(volts_2dp(6.55), "6.55 V");
        // A whole number still carries both decimals.
        assert_eq!(volts_2dp(7.0), "7.00 V");
    }

    /// ExifTool's own calibration point: a raw 155 is the empty reading, so it
    /// prints 0%, and 186 is 8.18 V.
    #[test]
    fn ad_readings_match_exiftools_calibration() {
        assert_eq!(ad_no_load(155.0), "155 (6.8V, 0%)");
        assert_eq!(ad_no_load(186.0), "186 (8.2V, 88%)");
        assert_eq!(ad_load(152.0), "152 (6.7V, 0%)");
    }

    /// Below one step the camera reports 0, which ExifTool prints as "0 ms",
    /// not "0.0 ms" -- the value is a Perl number, not a formatted one.
    #[test]
    fn integration_time_is_a_bare_number() {
        assert_eq!(millis(0.0), "0 ms");
        assert_eq!(millis(4.0), "4 ms");
    }

    #[test]
    fn temperatures_keep_their_declared_precision() {
        assert_eq!(celsius(31.0), "31 C");
        assert_eq!(celsius(-3.0), "-3 C");
        assert_eq!(celsius_1dp(31.4), "31.4 C");
        assert_eq!(celsius_1dp(-3.0), "-3.0 C");
    }

    #[test]
    fn af_c_sensitivity_counts_down_from_five() {
        assert_eq!(five_minus(0.0), "5");
        assert_eq!(five_minus(4.0), "1");
    }
}
