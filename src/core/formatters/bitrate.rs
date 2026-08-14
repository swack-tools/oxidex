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
        // Perl's "%.3g": three significant digits, trimmed of trailing zeros.
        format_significant(bitrate, 3)
    } else {
        format!("{:.0}", bitrate)
    };
    format!("{number} {}", units[unit_index])
}

/// Perl's `%.Ng` formatting: `N` significant digits, no trailing zeros or
/// decimal point, no exponent for the magnitudes this call site ever sees.
fn format_significant(value: f64, digits: i32) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (digits - 1 - magnitude).max(0);
    let formatted = format!("{value:.*}", decimals as usize);
    if formatted.contains('.') {
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        formatted
    }
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
}
