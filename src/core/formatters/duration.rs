//! ExifTool's `ConvertDuration`, the PrintConv shared by every composite that
//! reports an elapsed time in seconds.

/// `Image::ExifTool::ConvertDuration` (ExifTool.pm:6877-6895).
///
/// ```text
/// return '0 s' if $time == 0;
/// my $sign = ($time > 0 ? '' : (($time = -$time), '-'));
/// return sprintf("$sign%.2f s", $time) if $time < 30;
/// $time += 0.5;   # to round off to nearest second
/// my $h = int($time / 3600);  $time -= $h * 3600;
/// my $m = int($time / 60);    $time -= $m * 60;
/// if ($h > 24) { my $d = int($h / 24); $h -= $d * 24; $sign = "$sign$d days "; }
/// return sprintf("$sign%d:%.2d:%.2d", $h, $m, int($time));
/// ```
///
/// Note the `$h > 24` -- not `>= 24` -- so exactly one day of runtime prints as
/// `24:00:00` rather than `1 days 0:00:00`.
pub fn convert_duration(seconds: f64) -> String {
    if seconds == 0.0 {
        return "0 s".to_string();
    }
    let (sign, mut time) = if seconds > 0.0 {
        ("", seconds)
    } else {
        ("-", -seconds)
    };
    if time < 30.0 {
        return format!("{sign}{time:.2} s");
    }
    time += 0.5; // round off to the nearest second
    let mut hours = (time / 3600.0) as i64;
    time -= hours as f64 * 3600.0;
    let minutes = (time / 60.0) as i64;
    time -= minutes as f64 * 60.0;

    let mut prefix = sign.to_string();
    if hours > 24 {
        let days = hours / 24;
        hours -= days * 24;
        prefix = format!("{sign}{days} days ");
    }
    format!("{prefix}{hours}:{minutes:02}:{:02}", time as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exiftools_apple_runtime_output() {
        // Values quoted from `exiftool -a -G1 -s` over the sample corpus, with
        // the RunTimeValue/RunTimeScale pair each was computed from.
        // Apple_iPhone13Pro.jpg   235706184764708 / 1e9
        assert_eq!(convert_duration(235_706.184_764_708), "2 days 17:28:26");
        // Apple_iPhone11Pro.jpg     5805702950166 / 1e9
        assert_eq!(convert_duration(5_805.702_950_166), "1:36:46");
        // Apple_iPhone6s.jpg        1481457911333 / 1e9
        assert_eq!(convert_duration(1_481.457_911_333), "0:24:41");
        // Apple_iPadPro_12.9-inch_4th_generation.jpg 166430433427583 / 1e9
        assert_eq!(convert_duration(166_430.433_427_583), "1 days 22:13:50");
    }

    #[test]
    fn handles_the_short_and_zero_branches() {
        assert_eq!(convert_duration(0.0), "0 s");
        assert_eq!(convert_duration(12.345), "12.35 s");
        assert_eq!(convert_duration(-12.345), "-12.35 s");
        assert_eq!(convert_duration(-3600.0), "-1:00:00");
    }

    #[test]
    fn one_full_day_stays_in_hours() {
        // `$h > 24`, so 24 hours exactly does not become "1 days".
        assert_eq!(convert_duration(24.0 * 3600.0), "24:00:00");
        assert_eq!(convert_duration(25.0 * 3600.0), "1 days 1:00:00");
    }
}
