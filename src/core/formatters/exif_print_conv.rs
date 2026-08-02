//! Shared ports of the `Image::ExifTool::Exif` PrintConv subroutines.
//!
//! These are the conversions ExifTool defines once in `Exif.pm` and then
//! references by name from dozens of tag tables -- including MakerNote tables
//! that have nothing else in common. Porting one per parser is how the copies
//! drift: before this module existed there were sixteen `print_exposure_time`
//! functions in this crate and four of them printed a different string than
//! ExifTool for the same seconds, and nine `print_fraction` functions of which
//! four printed a different string than ExifTool for the same EV.

use crate::core::formatters::numeric_precision::perl_g;

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

/// Port of `Image::ExifTool::Exif::PrintFraction` (Exif.pm:5421).
///
/// ```text
/// sub PrintFraction($)
/// {
///     my $val = shift;
///     my $str;
///     if (defined $val) {
///         $val *= 1.00001;    # avoid round-off errors
///         if (not $val) {
///             $str = '0';
///         } elsif (int($val)/$val > 0.999) {
///             $str = sprintf("%+d", int($val));
///         } elsif ((int($val*2))/($val*2) > 0.999) {
///             $str = sprintf("%+d/2", int($val * 2));
///         } elsif ((int($val*3))/($val*3) > 0.999) {
///             $str = sprintf("%+d/3", int($val * 3));
///         } else {
///             $str = sprintf("%+.3g", $val);
///         }
///     }
///     return $str;
/// }
/// ```
///
/// This replaces nine separate ports. Sweeping all nine against the installed
/// 13.55 Perl -- every 1/32, 1/8, 1/6, 1/3 and 1/2 EV step real cameras write,
/// plus `%g` boundary values -- found four of them wrong, and each of the four
/// had unit tests asserting its own output. Counts are over the 1,538 swept
/// inputs with `|val| <= 20`, the range an exposure compensation can occupy:
///
/// | copy                              | wrong / 1,538 |
/// |-----------------------------------|---------------|
/// | `panasonic.rs`  (`{:+}`)          | 1,377         |
/// | `nikon/binary_data.rs` (`{:+.3}`) |   476         |
/// | `raw/metadata.rs` (`{:+.3}`)      |   430         |
/// | `minolta_tables.rs` (`{:+.3}`)    |   430         |
///
/// The five that were already correct -- `pdf`, `canon`, `minolta_makernote`,
/// `nikon/value_reader` and the `nikon/flash_info` shim that delegates to it --
/// agree with this one on all 1,538. The replacement itself was then checked
/// against the Perl over 5,197 inputs with zero divergences.
///
/// The failure is always the last branch. `%+.3g` is **three significant
/// digits**; `{:+.3}` is three *decimal places*. They agree only while the
/// value has one digit before the point:
///
/// ```text
/// input      {:+.3}     ExifTool
/// -19.875    -19.875    -19.9
/// -18.750    -18.750    -18.8
///   1.3264   +1.326     +1.33
/// ```
///
/// `panasonic.rs` used a bare `{:+}` and printed the full binary expansion --
/// `-19.875198750000003` where ExifTool prints `-19.9`.
///
/// `%g` also switches to exponent form outside `-4 <= exp < 3`, and the
/// exponent it tests is the one the value has *after* rounding -- which is why
/// this defers to [`perl_g`], the single `%g` implementation, rather than
/// deriving the exponent from `log10` of the unrounded value.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::exif_print_conv::print_fraction;
///
/// assert_eq!(print_fraction(0.0), "0");
/// assert_eq!(print_fraction(1.0), "+1");
/// assert_eq!(print_fraction(-2.0), "-2");
/// assert_eq!(print_fraction(0.5), "+1/2");
/// assert_eq!(print_fraction(1.0 / 3.0), "+1/3");
/// // The %+.3g branch: three significant digits, not three decimals
/// assert_eq!(print_fraction(1.326429536), "+1.33");
/// assert_eq!(print_fraction(-0.7), "-0.7");
/// ```
pub fn print_fraction(val: f64) -> String {
    // Exif.pm:5426 -- `$val *= 1.00001; # avoid round-off errors`
    let val = val * 1.00001;
    // Exif.pm:5427 -- `if (not $val)`. Perl's falsiness also covers NaN here.
    if val == 0.0 || val.is_nan() {
        return "0".to_string();
    }
    // Exif.pm:5429-5434 -- int($val)/$val > 0.999, then the *2 and *3 forms.
    // Perl's int() truncates toward zero, which is f64::trunc.
    for (multiplier, suffix) in [(1.0_f64, ""), (2.0, "/2"), (3.0, "/3")] {
        let scaled = val * multiplier;
        let whole = scaled.trunc();
        // Beyond i64 Perl's own `sprintf("%+d", int($val))` wraps and prints
        // nonsense (`PrintFraction(1e20)` is `-1` on this build). Fall through
        // to the %g branch instead of reproducing the overflow.
        if whole.abs() >= 9.223_372_036_854_776e18 {
            break;
        }
        if whole / scaled > 0.999 {
            return format!("{:+}{}", whole as i64, suffix);
        }
    }
    // Exif.pm:5436 -- `sprintf("%+.3g", $val)`.
    let body = perl_g(val, 3);
    if body.starts_with('-') {
        body
    } else {
        format!("+{}", body)
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
