//! Shared ports of the `Image::ExifTool::Exif` PrintConv subroutines.
//!
//! These are the conversions ExifTool defines once in `Exif.pm` and then
//! references by name from dozens of tag tables -- including MakerNote tables
//! that have nothing else in common. Porting one per parser is how the copies
//! drift: before this module existed there were sixteen `print_exposure_time`
//! functions in this crate and four of them printed a different string than
//! ExifTool for the same seconds.

/// Port of `Image::ExifTool::Exif::PrintExposureTime` (Exif.pm:5606).
///
/// ```text
/// sub PrintExposureTime($)
/// {
///     my $secs = shift;
///     return $secs unless Image::ExifTool::IsFloat($secs);
///     if ($secs < 0.25001 and $secs > 0) {
///         return sprintf("1/%d",int(0.5 + 1/$secs));
///     }
///     $_ = sprintf("%.1f",$secs);
///     s/\.0$//;
///     return $_;
/// }
/// ```
///
/// Three details are load-bearing, and each one was gotten wrong by at least
/// one of the copies this replaces:
///
/// * The slow branch is `sprintf("%.1f")` -- **one** decimal place. A copy that
///   printed the quotient at full precision turned Sony's `2**(6-$val/8)`
///   shutter speed into `58.6882587651` where ExifTool prints `58.7`.
/// * The trailing `.0` is stripped by a regex that only fires when `%.1f`
///   actually produced one, so `29.96 s` prints as `30`, not `30.0`. Checking
///   `secs == secs.trunc()` before formatting is not the same test: 29.96 is
///   not an integer but rounds to one.
/// * `int()` truncates toward zero, so the reciprocal is `int(0.5 + 1/$secs)`,
///   not a rounding function applied to `1/$secs`.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::exif_print_conv::print_exposure_time;
///
/// assert_eq!(print_exposure_time(1.0 / 250.0), "1/250");
/// assert_eq!(print_exposure_time(0.25), "1/4");
/// assert_eq!(print_exposure_time(0.4), "0.4");
/// assert_eq!(print_exposure_time(2.0), "2");
/// // Rounds to one decimal, then drops a trailing ".0"
/// assert_eq!(print_exposure_time(58.688258765), "58.7");
/// assert_eq!(print_exposure_time(29.96), "30");
/// ```
pub fn print_exposure_time(secs: f64) -> String {
    if secs < 0.25001 && secs > 0.0 {
        return format!("1/{}", (0.5 + 1.0 / secs) as i64);
    }
    let formatted = format!("{:.1}", secs);
    formatted
        .strip_suffix(".0")
        .map(str::to_string)
        .unwrap_or(formatted)
}

/// `PrintExposureTime` over a string that ExifTool has not yet numified,
/// scaling microseconds to seconds first.
///
/// `Exif.pm:5609` -- `return $secs unless Image::ExifTool::IsFloat($secs)` --
/// hands back anything that is not a number untouched, which is what the APP12
/// text segments need: their shutter fields are decimal strings in
/// microseconds, and a malformed one has to survive as itself.
///
/// `Image::ExifTool::IsFloat` (ExifTool.pm:5924) is
/// `/^[+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?$/` -- anchored at both ends
/// with no whitespace tolerance -- so the string is parsed as-is. Both callers
/// already trimmed the value off its `Key=Value` line before getting here.
pub fn print_exposure_time_micros_str(value: &str) -> String {
    match value.parse::<f64>() {
        Ok(micros) => print_exposure_time(micros * 1e-6),
        Err(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fast branch: `sprintf("1/%d", int(0.5 + 1/$secs))`.
    #[test]
    fn reciprocal_branch_matches_perl() {
        assert_eq!(print_exposure_time(1.0 / 8000.0), "1/8000");
        assert_eq!(print_exposure_time(1.0 / 250.0), "1/250");
        assert_eq!(print_exposure_time(1.0 / 13.0), "1/13");
        assert_eq!(print_exposure_time(0.1), "1/10");
        // 0.25 is inside the `< 0.25001` window; 0.2501 is not.
        assert_eq!(print_exposure_time(0.25), "1/4");
        assert_eq!(print_exposure_time(0.2501), "0.3");
    }

    /// `int()` truncates, so a reciprocal of 5.4 is 5 and not 6.
    #[test]
    fn reciprocal_truncates_rather_than_rounds() {
        // 1/0.2049 = 4.8804..., +0.5 = 5.3804..., int() -> 5
        assert_eq!(print_exposure_time(0.2049), "1/5");
    }

    /// The slow branch is one decimal place, not full precision. These are the
    /// Sony `2**(6 - $val/8)` shutter speeds that the pre-consolidation
    /// `sony::binary` copy printed as `58.6882587651` and `4.7568284600`.
    #[test]
    fn slow_branch_rounds_to_one_decimal() {
        assert_eq!(print_exposure_time(2f64.powf(6.0 - 1.0 / 8.0)), "58.7");
        assert_eq!(print_exposure_time(2f64.powf(6.0 - 30.0 / 8.0)), "4.8");
        assert_eq!(print_exposure_time(2f64.powf(6.0 - 9.0 / 8.0)), "29.3");
    }

    /// `s/\.0$//` fires on the *formatted* string, so a non-integer that
    /// rounds to a whole tenth loses the decimal too. The copies that tested
    /// `secs == secs.trunc()` instead printed `1.0` and `19.0` here.
    #[test]
    fn trailing_zero_strip_follows_the_rounding() {
        assert_eq!(print_exposure_time(2.0), "2");
        assert_eq!(print_exposure_time(30.0), "30");
        assert_eq!(print_exposure_time(0.96), "1");
        assert_eq!(print_exposure_time(29.96), "30");
        // 2**4.25 and 2**6.375, both real 1/8-stop APEX shutter speeds.
        assert_eq!(print_exposure_time(19.0273138400435), "19");
        assert_eq!(print_exposure_time(82.9977314976646), "83");
    }

    /// `IsFloat` is anchored, so a padded number is not a number.
    #[test]
    fn non_numeric_string_survives_untouched() {
        assert_eq!(print_exposure_time_micros_str("n/a"), "n/a");
        assert_eq!(print_exposure_time_micros_str(""), "");
        assert_eq!(print_exposure_time_micros_str(" 2000000 "), " 2000000 ");
    }

    /// APP12 shutter fields are microseconds in a decimal string.
    #[test]
    fn micros_string_is_scaled_then_printed() {
        assert_eq!(print_exposure_time_micros_str("6460"), "1/155");
        assert_eq!(print_exposure_time_micros_str("500000"), "0.5");
        assert_eq!(print_exposure_time_micros_str("2000000"), "2");
    }
}
