//! ExifTool's `ConvertBitrate`, the PrintConv shared by every tag that
//! reports a bit rate in bits per second.

/// `Image::ExifTool::ConvertBitrate` (ExifTool.pm:6900-6912).
///
/// ```text
/// my $units = [ 'bps', 'kbps', 'Mbps', 'Gbps' ];
/// ...
/// $bitrate /= 1000, $unit = shift @$units while $bitrate >= 1000 and @$units;
/// ...
/// return sprintf('%.3g %s', $bitrate, $unit) if $bitrate < 100;
/// return sprintf('%.0f %s', $bitrate, $unit);
/// ```
pub fn convert_bitrate(mut bitrate: f64) -> String {
    let units = ["bps", "kbps", "Mbps", "Gbps"];
    let mut unit_index = 0;
    while bitrate >= 1000.0 && unit_index < units.len() - 1 {
        bitrate /= 1000.0;
        unit_index += 1;
    }
    let number = if bitrate < 100.0 {
        // C's `%.3g`, exponent form included. A local "three significant
        // digits, fixed notation" helper used to live here; it agreed with
        // Perl on every realistic bitrate and disagreed on the tails the
        // differential oracle probes: `ConvertBitrate(-1000)` is `-1e+03 bps`
        // in Perl (a negative never scales past `bps`, and `%.3g` of -1000
        // is `%e` form), and `1e-6` is `1e-06 bps`. `perl_g` is the one
        // general `%g` in the crate; there is no reason for a second.
        crate::core::formatters::numeric_precision::perl_g(bitrate, 3)
    } else {
        format!("{:.0}", bitrate)
    };
    format!("{number} {}", units[unit_index])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two values `MOI::Main`'s VideoBitrate ValueConv hash declares
    /// (MOI.pm:82-89) and the DV profile total (DV.pm:191), each checked
    /// against the pinned oracle's rendering.
    #[test]
    fn renders_the_pinned_bitrates() {
        assert_eq!(convert_bitrate(8_500_000.0), "8.5 Mbps");
        assert_eq!(convert_bitrate(5_500_000.0), "5.5 Mbps");
        assert_eq!(convert_bitrate(28_800_000.0), "28.8 Mbps");
        assert_eq!(convert_bitrate(224_000.0), "224 kbps");
    }

    /// The `%.3g` tails, quoted from `Image::ExifTool::ConvertBitrate` under
    /// the pinned 13.59 Perl (`perl -MImage::ExifTool -e ...`). These are the
    /// probes the fixed-notation helper this replaced got wrong; the unit
    /// boundaries and the exhausted-units case are checked alongside them.
    #[test]
    fn matches_perl_on_the_g_tails_and_unit_boundaries() {
        assert_eq!(convert_bitrate(0.0), "0 bps");
        assert_eq!(convert_bitrate(999.0), "999 bps");
        assert_eq!(convert_bitrate(1000.0), "1 kbps");
        // 99.999 kbps rounds to "100" under %.3g, not "99.9" or "100.0".
        assert_eq!(convert_bitrate(99_999.0), "100 kbps");
        assert_eq!(convert_bitrate(1e6), "1 Mbps");
        // Exponent form: %.3g switches to %e below 1e-4 ...
        assert_eq!(convert_bitrate(1e-6), "1e-06 bps");
        // ... and a negative never scales, so -1000 stays in bps and %.3g
        // renders it in %e form too.
        assert_eq!(convert_bitrate(-1000.0), "-1e+03 bps");
        // Units run out at Gbps; from there it is %.0f of the Gbps figure.
        assert_eq!(convert_bitrate(1e18), "1000000000 Gbps");
    }
}
