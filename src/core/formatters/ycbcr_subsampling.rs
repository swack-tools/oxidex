//! YCbCrSubSampling formatter
//!
//! This module provides formatting for the YCbCrSubSampling EXIF tag (tag ID 0x0212).
//! The tag describes the subsampling ratio of chrominance components (Cb and Cr)
//! relative to the luminance component (Y) in YCbCr color space.
//!
//! # Background
//!
//! YCbCr subsampling is used in JPEG and other image formats to reduce file size
//! by storing less color information than brightness information (human eyes are
//! less sensitive to color detail).
//!
//! # Format
//!
//! The tag stores two values: [horizontal_ratio, vertical_ratio]
//! ExifTool displays these as: "YCbCr4:X:Y (H V)" format where:
//! - X and Y depend on the ratios
//! - H V are the actual ratio values
//!
//! Common patterns:
//! - [2, 1] -> "YCbCr4:2:2 (2 1)" - 4:2:2 subsampling (horizontal only)
//! - [2, 2] -> "YCbCr4:2:0 (2 2)" - 4:2:0 subsampling (both directions)
//! - [1, 1] -> "YCbCr4:4:4 (1 1)" - No subsampling (full resolution)
//! - [4, 1] -> "YCbCr4:1:1 (4 1)" - 4:1:1 subsampling (aggressive horizontal)

/// Formats YCbCrSubSampling values to match ExifTool output.
///
/// Converts the raw horizontal and vertical subsampling ratios into the
/// standard "YCbCr4:X:Y (H V)" format.
///
/// # Arguments
///
/// * `h` - Horizontal subsampling ratio (typically 1, 2, or 4)
/// * `v` - Vertical subsampling ratio (typically 1 or 2)
///
/// # Returns
///
/// A formatted string like "YCbCr4:2:2 (2 1)" matching ExifTool's output.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::ycbcr_subsampling::format_ycbcr_subsampling;
///
/// // Most common: 4:2:2 (horizontal subsampling only)
/// assert_eq!(format_ycbcr_subsampling(2, 1), "YCbCr4:2:2 (2 1)");
///
/// // 4:2:0 (both horizontal and vertical subsampling)
/// assert_eq!(format_ycbcr_subsampling(2, 2), "YCbCr4:2:0 (2 2)");
///
/// // 4:4:4 (no subsampling)
/// assert_eq!(format_ycbcr_subsampling(1, 1), "YCbCr4:4:4 (1 1)");
///
/// // 4:1:1 (aggressive horizontal subsampling)
/// assert_eq!(format_ycbcr_subsampling(4, 1), "YCbCr4:1:1 (4 1)");
/// ```
pub fn format_ycbcr_subsampling(h: i64, v: i64) -> String {
    // Calculate the chroma subsampling notation
    // The pattern is YCbCr4:X:Y where:
    // - 4 represents the Y sample rate (always 4 in standard notation)
    // - X represents horizontal chroma samples for 4 luma samples
    // - Y represents vertical chroma samples (0 if v=2, equals X if v=1)

    let x = 4 / h; // Horizontal chroma samples per 4 luma samples
    let y = if v == 2 { 0 } else { x }; // Vertical: 0 if v=2, same as x if v=1

    format!("YCbCr4:{}:{} ({} {})", x, y, h, v)
}

/// Parses a string containing two space-separated integers for YCbCr subsampling.
///
/// # Arguments
///
/// * `value` - String containing two integers separated by space (e.g., "2 1")
///
/// # Returns
///
/// Formatted YCbCr string, or original value if parsing fails.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::ycbcr_subsampling::format_ycbcr_subsampling_string;
///
/// assert_eq!(format_ycbcr_subsampling_string("2 1"), "YCbCr4:2:2 (2 1)");
/// assert_eq!(format_ycbcr_subsampling_string("2 2"), "YCbCr4:2:0 (2 2)");
/// assert_eq!(format_ycbcr_subsampling_string("invalid"), "invalid");
/// ```
pub fn format_ycbcr_subsampling_string(value: &str) -> String {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() == 2 {
        if let (Ok(h), Ok(v)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
            return format_ycbcr_subsampling(h, v);
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ycbcr_422() {
        // 4:2:2 - Horizontal subsampling only (most common in cameras)
        assert_eq!(format_ycbcr_subsampling(2, 1), "YCbCr4:2:2 (2 1)");
    }

    #[test]
    fn test_ycbcr_420() {
        // 4:2:0 - Both horizontal and vertical subsampling (common in video)
        assert_eq!(format_ycbcr_subsampling(2, 2), "YCbCr4:2:0 (2 2)");
    }

    #[test]
    fn test_ycbcr_444() {
        // 4:4:4 - No subsampling (full chroma resolution)
        assert_eq!(format_ycbcr_subsampling(1, 1), "YCbCr4:4:4 (1 1)");
    }

    #[test]
    fn test_ycbcr_411() {
        // 4:1:1 - Aggressive horizontal subsampling
        assert_eq!(format_ycbcr_subsampling(4, 1), "YCbCr4:1:1 (4 1)");
    }

    #[test]
    fn test_format_from_string() {
        assert_eq!(format_ycbcr_subsampling_string("2 1"), "YCbCr4:2:2 (2 1)");
        assert_eq!(format_ycbcr_subsampling_string("2 2"), "YCbCr4:2:0 (2 2)");
        assert_eq!(format_ycbcr_subsampling_string("1 1"), "YCbCr4:4:4 (1 1)");
        assert_eq!(format_ycbcr_subsampling_string("4 1"), "YCbCr4:1:1 (4 1)");
    }

    #[test]
    fn test_invalid_input() {
        // Invalid inputs should return original value
        assert_eq!(format_ycbcr_subsampling_string("invalid"), "invalid");
        assert_eq!(format_ycbcr_subsampling_string("2"), "2");
        assert_eq!(format_ycbcr_subsampling_string("2 1 3"), "2 1 3");
        assert_eq!(format_ycbcr_subsampling_string(""), "");
    }
}
