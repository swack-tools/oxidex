//! Numeric precision formatting for EXIF rational values
//!
//! This module provides formatting functions to match ExifTool's numeric precision
//! for various EXIF rational tags. Different tags use different decimal precisions:
//!
//! - **High precision (9 decimals)**: BrightnessValue, DigitalZoomRatio, CompressedBitsPerPixel
//! - **GPS precision (2 decimals with trailing zeros)**: GPSDestBearing, GPSImgDirection, GPSSpeed, GPSTrack
//! - **Resolution precision (up to 10 decimals)**: XResolution, YResolution
//!
//! # Background
//!
//! OxiDex was originally outputting 6 decimal places for all rational values,
//! but ExifTool uses tag-specific precision. This module provides the logic to
//! format EXIF rationals to match ExifTool's output exactly.
//!
//! # Examples
//!
//! ```
//! use oxidex::core::formatters::numeric_precision::format_exif_rational;
//!
//! // High precision tags (9 decimal places)
//! assert_eq!(format_exif_rational("BrightnessValue", 3.617254236), "3.617254236");
//!
//! // GPS tags (whole numbers without decimals, fractional with minimal precision)
//! assert_eq!(format_exif_rational("GPSImgDirection", 45.5), "45.5");
//! assert_eq!(format_exif_rational("GPSImgDirection", 45.0), "45");
//!
//! // Resolution tags (only as many decimals as needed, up to 10)
//! assert_eq!(format_exif_rational("XResolution", 72.0), "72");
//! ```

// ============================================================================
// PRECISION CONSTANTS
// ============================================================================

/// Tags that require 9 decimal places of precision.
///
/// These tags store computed APEX values or ratios where high precision
/// is meaningful for accuracy in exposure calculations or zoom factors.
pub const HIGH_PRECISION_9_TAGS: &[&str] = &[
    "BrightnessValue",
    "CompressedBitsPerPixel",
    "DigitalZoomRatio",
];

/// Tags that require GPS-style numeric formatting.
///
/// GPS directional and speed tags are displayed with ExifTool-compatible formatting:
/// - Whole numbers (no fractional part) are displayed without decimals: "20"
/// - Numbers with fractional parts use minimal required precision
///
/// This matches ExifTool's behavior where integer GPS values don't show ".00" suffix.
pub const GPS_NUMERIC_TAGS: &[&str] = &[
    "GPSDestBearing",
    "GPSImgDirection",
    "GPSSpeed",
    "GPSTrack",
    "GPSDestDistance",
];

/// Tags that may require up to 10 decimal places if precision is needed.
///
/// Resolution values typically display as integers when they are whole numbers,
/// but can have up to 10 decimal places for fractional DPI values.
pub const RESOLUTION_TAGS: &[&str] = &["XResolution", "YResolution"];

/// Tags that should format integers without decimal places (integer precision).
///
/// These tags store values as rational numbers but ExifTool displays them as
/// integers when they are whole numbers. ReferenceBlackWhite contains Y, Cb, Cr
/// black and white reference values which are typically 0, 255, or 128.
pub const INTEGER_PRECISION_TAGS: &[&str] = &["ReferenceBlackWhite"];

/// Tags that should use 3 decimal places of precision.
///
/// YCbCrCoefficients contains the matrix coefficients for converting RGB to YCbCr.
/// Standard values are 0.299, 0.587, and 0.114 (ITU-R BT.601 standard).
/// ExifTool displays these with 3 decimal places.
pub const THREE_DECIMAL_PRECISION_TAGS: &[&str] = &["YCbCrCoefficients"];

/// Tags for ICC profile matrix values requiring 5 decimal places maximum.
///
/// ICC profile color transformation matrix values (ChromaticAdaptation, ColorMatrix, etc.)
/// are displayed with up to 5 decimal places to match ExifTool output.
///
/// This list includes:
/// - Color matrix columns (RedMatrixColumn, GreenMatrixColumn, BlueMatrixColumn)
/// - White point and illuminant values (MediaWhitePoint, ConnectionSpaceIlluminant)
/// - Viewing condition values (Luminance, ViewingCondIlluminant, ViewingCondSurround)
/// - TRC (Tone Reproduction Curve) tags
/// - DNG camera calibration matrices
pub const ICC_MATRIX_TAGS: &[&str] = &[
    "ChromaticAdaptation",
    "ColorMatrix1",
    "ColorMatrix2",
    "CameraCalibration1",
    "CameraCalibration2",
    "ProfileCalibrationSignature",
    "RedMatrixColumn",
    "GreenMatrixColumn",
    "BlueMatrixColumn",
    "RedTRC",
    "GreenTRC",
    "BlueTRC",
    // ICC profile white point and illuminant values
    "MediaWhitePoint",
    "ConnectionSpaceIlluminant",
    // ICC profile viewing condition values
    "Luminance",
    "ViewingCondIlluminant",
    "ViewingCondSurround",
    // MeasurementFlare is formatted as a percentage (handled separately with "%" suffix)
    "MeasurementFlare",
];

// ============================================================================
// MAIN FORMATTING FUNCTION
// ============================================================================

/// Format an EXIF rational value with tag-specific precision to match ExifTool output.
///
/// This function applies different decimal precision rules based on the tag name:
///
/// | Tag Category | Decimal Places | Trailing Zeros | Example |
/// |--------------|----------------|----------------|---------|
/// | BrightnessValue, DigitalZoomRatio, CompressedBitsPerPixel | 9 | Trimmed | "3.617254236" |
/// | GPSDestBearing, GPSImgDirection, GPSSpeed, GPSTrack | 2 | Preserved | "45.50" |
/// | XResolution, YResolution | Up to 10 | Trimmed | "72" or "300.5" |
/// | All other tags | 6 (default) | Trimmed | "3.5" |
///
/// # Arguments
///
/// * `tag_name` - The EXIF tag name. Supports both simple names ("BrightnessValue")
///   and fully-qualified names with group prefix ("EXIF:BrightnessValue").
/// * `value` - The floating-point value to format (typically computed from rational numerator/denominator).
///
/// # Returns
///
/// A string representation of the value with appropriate precision for the tag.
///
/// # Precision Rules
///
/// - **High precision tags**: Format with up to 9 decimal places, trimming trailing zeros
///   except when the result would be an integer (which displays without decimal point).
/// - **GPS tags**: Always format with exactly 2 decimal places, including trailing zeros.
/// - **Resolution tags**: Format with up to 10 decimal places, trimming trailing zeros.
///   Integer values display without decimal point.
/// - **Default**: Format with up to 6 decimal places, trimming trailing zeros.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::format_exif_rational;
///
/// // High precision (9 decimals, trailing zeros trimmed)
/// assert_eq!(format_exif_rational("BrightnessValue", 3.617254236), "3.617254236");
/// assert_eq!(format_exif_rational("DigitalZoomRatio", 1.0), "1");
/// assert_eq!(format_exif_rational("CompressedBitsPerPixel", 2.5), "2.5");
///
/// // GPS tags (whole numbers without decimals, fractional with minimal precision)
/// assert_eq!(format_exif_rational("GPSDestBearing", 123.45), "123.45");
/// assert_eq!(format_exif_rational("GPSImgDirection", 45.0), "45");
/// assert_eq!(format_exif_rational("GPSSpeed", 50.5), "50.5");
/// assert_eq!(format_exif_rational("GPSTrack", 180.0), "180");
///
/// // Resolution (up to 10 decimals, trailing zeros trimmed)
/// assert_eq!(format_exif_rational("XResolution", 72.0), "72");
/// assert_eq!(format_exif_rational("YResolution", 300.5), "300.5");
///
/// // Default behavior (6 decimals, trailing zeros trimmed)
/// assert_eq!(format_exif_rational("SomeOtherTag", 3.5), "3.5");
///
/// // Works with fully-qualified tag names
/// assert_eq!(format_exif_rational("EXIF:BrightnessValue", 3.617254236), "3.617254236");
/// assert_eq!(format_exif_rational("GPS:GPSImgDirection", 45.0), "45");
/// ```
pub fn format_exif_rational(tag_name: &str, value: f64) -> String {
    // Extract the base tag name (after the last colon if present).
    // This handles fully-qualified names like "EXIF:BrightnessValue" or "GPS:GPSSpeed".
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);

    // Apply tag-specific formatting rules based on the base tag name
    if HIGH_PRECISION_9_TAGS.contains(&base_name) {
        format_with_precision(value, 9)
    } else if GPS_NUMERIC_TAGS.contains(&base_name) {
        // GPS numeric tags: whole numbers without decimals, fractional with minimal precision
        // ExifTool shows "20" not "20.00" for integer GPS values
        format_gps_numeric(value)
    } else if RESOLUTION_TAGS.contains(&base_name) {
        format_with_precision(value, 10)
    } else if ICC_MATRIX_TAGS.contains(&base_name) {
        // ICC profile matrix values: limit to 5 decimal places
        format_with_precision(value, 5)
    } else {
        // Default: 6 decimal places with trailing zeros trimmed
        format_with_precision(value, 6)
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Format a value with a specified maximum precision, trimming trailing zeros.
///
/// This internal helper formats a floating-point value with up to `precision`
/// decimal places, then removes unnecessary trailing zeros and decimal points
/// to produce clean output.
///
/// # Arguments
///
/// * `value` - The floating-point value to format
/// * `precision` - Maximum number of decimal places to display
///
/// # Returns
///
/// A string representation with trailing zeros trimmed. Integer values
/// are displayed without a decimal point (e.g., "72" not "72.0").
///
/// # Examples
///
/// ```ignore
/// assert_eq!(format_with_precision(3.5, 9), "3.5");
/// assert_eq!(format_with_precision(72.0, 10), "72");
/// assert_eq!(format_with_precision(3.617254236, 9), "3.617254236");
/// ```
fn format_with_precision(value: f64, precision: usize) -> String {
    // Format with the specified precision
    let formatted = format!("{:.prec$}", value, prec = precision);

    // Trim trailing zeros and unnecessary decimal point for cleaner output.
    // This produces "3.5" instead of "3.500000000" and "72" instead of "72.0000000000".
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Format GPS numeric values with ExifTool-compatible precision.
///
/// GPS numeric values (direction, speed, distance) are formatted to match ExifTool:
/// - Whole numbers are displayed without decimals: "20" not "20.00"
/// - Fractional values use minimal required precision (trimmed trailing zeros)
///
/// This function uses a small epsilon (1e-9) to detect near-integer values,
/// accounting for floating-point representation of values like 20.0.
///
/// # Arguments
///
/// * `value` - The floating-point value to format
///
/// # Returns
///
/// A string formatted to match ExifTool's GPS numeric output.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(format_gps_numeric(20.0), "20");
/// assert_eq!(format_gps_numeric(45.5), "45.5");
/// assert_eq!(format_gps_numeric(123.456), "123.456");
/// ```
fn format_gps_numeric(value: f64) -> String {
    // Use a small epsilon to detect near-integer values
    // This handles floating-point representation issues
    const EPSILON: f64 = 1e-9;

    if (value.fract().abs()) < EPSILON {
        // Whole number - format without decimals
        format!("{:.0}", value)
    } else {
        // Fractional value - use up to 6 decimal places and trim trailing zeros
        format_with_precision(value, 6)
    }
}

/// Check if a tag requires high precision (9 decimal places).
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally with group prefix
///
/// # Returns
///
/// `true` if the tag is in the high precision category.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::is_high_precision_tag;
///
/// assert!(is_high_precision_tag("BrightnessValue"));
/// assert!(is_high_precision_tag("EXIF:DigitalZoomRatio"));
/// assert!(!is_high_precision_tag("FocalLength"));
/// ```
pub fn is_high_precision_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    HIGH_PRECISION_9_TAGS.contains(&base_name)
}

/// Check if a tag requires GPS-style numeric formatting.
///
/// GPS numeric tags display whole numbers without decimals and fractional
/// values with minimal precision (trailing zeros trimmed).
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally with group prefix
///
/// # Returns
///
/// `true` if the tag is a GPS numeric tag (direction, speed, distance).
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::is_gps_numeric_tag;
///
/// assert!(is_gps_numeric_tag("GPSImgDirection"));
/// assert!(is_gps_numeric_tag("GPS:GPSSpeed"));
/// assert!(!is_gps_numeric_tag("GPSLatitude"));
/// ```
pub fn is_gps_numeric_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    GPS_NUMERIC_TAGS.contains(&base_name)
}

/// Check if a tag is an ICC profile matrix tag (5 decimal places).
///
/// ICC profile matrix values are formatted with up to 5 decimal places
/// to match ExifTool output.
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally with group prefix
///
/// # Returns
///
/// `true` if the tag is an ICC profile matrix tag.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::is_icc_matrix_tag;
///
/// assert!(is_icc_matrix_tag("ChromaticAdaptation"));
/// assert!(is_icc_matrix_tag("ICC_Profile:RedMatrixColumn"));
/// assert!(!is_icc_matrix_tag("GPSSpeed"));
/// ```
pub fn is_icc_matrix_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    ICC_MATRIX_TAGS.contains(&base_name)
}

/// Format an ICC profile value with 5 decimal places maximum, trimming trailing zeros.
///
/// ICC profile matrix values (color matrices, white points, illuminants) are displayed
/// with up to 5 decimal places to match ExifTool output.
///
/// # Arguments
///
/// * `value` - The floating-point value to format
///
/// # Returns
///
/// A string formatted with up to 5 decimal places, trailing zeros trimmed.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::format_icc_value;
///
/// assert_eq!(format_icc_value(0.14919), "0.14919");
/// assert_eq!(format_icc_value(0.5), "0.5");
/// assert_eq!(format_icc_value(1.0), "1");
/// ```
pub fn format_icc_value(value: f64) -> String {
    format_with_precision(value, 5)
}

/// Check if a tag is a resolution tag (XResolution, YResolution).
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally with group prefix
///
/// # Returns
///
/// `true` if the tag is a resolution tag.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::is_resolution_tag;
///
/// assert!(is_resolution_tag("XResolution"));
/// assert!(is_resolution_tag("EXIF:YResolution"));
/// assert!(!is_resolution_tag("FocalPlaneXResolution"));
/// ```
pub fn is_resolution_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    RESOLUTION_TAGS.contains(&base_name)
}

/// Check if a tag requires integer precision (no decimal places for whole numbers).
///
/// Integer precision tags like ReferenceBlackWhite contain values that should
/// be displayed as integers when they are whole numbers (0, 128, 255).
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally with group prefix
///
/// # Returns
///
/// `true` if the tag should use integer precision formatting.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::is_integer_precision_tag;
///
/// assert!(is_integer_precision_tag("ReferenceBlackWhite"));
/// assert!(is_integer_precision_tag("EXIF:ReferenceBlackWhite"));
/// assert!(!is_integer_precision_tag("YCbCrCoefficients"));
/// ```
pub fn is_integer_precision_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    INTEGER_PRECISION_TAGS.contains(&base_name)
}

/// Check if a tag requires 3 decimal places of precision.
///
/// Three-decimal precision is used for tags like YCbCrCoefficients which
/// contain standard values like 0.299, 0.587, and 0.114.
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally with group prefix
///
/// # Returns
///
/// `true` if the tag should use 3 decimal precision formatting.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::is_three_decimal_tag;
///
/// assert!(is_three_decimal_tag("YCbCrCoefficients"));
/// assert!(is_three_decimal_tag("EXIF:YCbCrCoefficients"));
/// assert!(!is_three_decimal_tag("ReferenceBlackWhite"));
/// ```
pub fn is_three_decimal_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    THREE_DECIMAL_PRECISION_TAGS.contains(&base_name)
}

/// Format a space-separated list of values with integer precision.
///
/// Each value in the space-separated string is reformatted to display
/// as an integer when it's a whole number, matching ExifTool output.
///
/// # Arguments
///
/// * `value` - Space-separated string of numeric values (e.g., "0.0 255.0 128.0")
///
/// # Returns
///
/// A string with each value formatted as integer if whole, or minimal decimals if not.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::format_integer_precision_values;
///
/// assert_eq!(format_integer_precision_values("0.0 255.0 128.0"), "0 255 128");
/// assert_eq!(format_integer_precision_values("0.5 255.0 128.5"), "0.5 255 128.5");
/// ```
pub fn format_integer_precision_values(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            if let Ok(f) = part.parse::<f64>() {
                // Check if value is effectively an integer
                if (f.fract().abs()) < 1e-9 {
                    format!("{:.0}", f)
                } else {
                    // Non-integer: use minimal precision (trim trailing zeros)
                    format_with_precision(f, 6)
                }
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format a space-separated list of values with 3 decimal precision.
///
/// Each value in the space-separated string is reformatted to display
/// with up to 3 decimal places, matching ExifTool output for tags like
/// YCbCrCoefficients.
///
/// # Arguments
///
/// * `value` - Space-separated string of numeric values (e.g., "0.299 0.587 0.114")
///
/// # Returns
///
/// A string with each value formatted with up to 3 decimal places.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::numeric_precision::format_three_decimal_values;
///
/// assert_eq!(format_three_decimal_values("0.2990000000 0.5870000000 0.1140000000"), "0.299 0.587 0.114");
/// assert_eq!(format_three_decimal_values("0.5 1.0 0.25"), "0.5 1 0.25");
/// ```
pub fn format_three_decimal_values(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            if let Ok(f) = part.parse::<f64>() {
                format_with_precision(f, 3)
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // High Precision Tags (9 decimal places)
    // ------------------------------------------------------------------------

    #[test]
    fn test_brightness_value_high_precision() {
        // BrightnessValue should show up to 9 decimal places
        assert_eq!(
            format_exif_rational("BrightnessValue", 3.617254236),
            "3.617254236"
        );
        // Trailing zeros should be trimmed
        assert_eq!(format_exif_rational("BrightnessValue", 3.5), "3.5");
        // Integer values should not show decimal point
        assert_eq!(format_exif_rational("BrightnessValue", 4.0), "4");
    }

    #[test]
    fn test_digital_zoom_ratio_high_precision() {
        // DigitalZoomRatio should show up to 9 decimal places
        assert_eq!(
            format_exif_rational("DigitalZoomRatio", 1.234567891),
            "1.234567891"
        );
        // Typical values
        assert_eq!(format_exif_rational("DigitalZoomRatio", 1.0), "1");
        assert_eq!(format_exif_rational("DigitalZoomRatio", 2.5), "2.5");
    }

    #[test]
    fn test_compressed_bits_per_pixel_high_precision() {
        // CompressedBitsPerPixel should show up to 9 decimal places
        assert_eq!(
            format_exif_rational("CompressedBitsPerPixel", 2.123456789),
            "2.123456789"
        );
        assert_eq!(format_exif_rational("CompressedBitsPerPixel", 3.0), "3");
    }

    // ------------------------------------------------------------------------
    // GPS Numeric Tags (whole numbers without decimals, fractional with minimal precision)
    // ------------------------------------------------------------------------

    #[test]
    fn test_gps_dest_bearing_formatting() {
        // GPSDestBearing: fractional values show minimal precision
        assert_eq!(format_exif_rational("GPSDestBearing", 123.45), "123.45");
        // Whole numbers display without decimals (ExifTool compatible)
        assert_eq!(format_exif_rational("GPSDestBearing", 90.0), "90");
        // Trailing zeros trimmed from fractional values
        assert_eq!(format_exif_rational("GPSDestBearing", 45.5), "45.5");
    }

    #[test]
    fn test_gps_img_direction_formatting() {
        // GPSImgDirection: whole numbers without decimals
        assert_eq!(format_exif_rational("GPSImgDirection", 180.0), "180");
        assert_eq!(format_exif_rational("GPSImgDirection", 20.0), "20");
        // Fractional values with minimal precision
        assert_eq!(format_exif_rational("GPSImgDirection", 270.75), "270.75");
    }

    #[test]
    fn test_gps_speed_formatting() {
        // GPSSpeed: whole numbers without decimals
        assert_eq!(format_exif_rational("GPSSpeed", 50.0), "50");
        // Fractional values with minimal precision
        assert_eq!(format_exif_rational("GPSSpeed", 65.5), "65.5");
        assert_eq!(format_exif_rational("GPSSpeed", 100.25), "100.25");
    }

    #[test]
    fn test_gps_track_formatting() {
        // GPSTrack: whole numbers without decimals
        assert_eq!(format_exif_rational("GPSTrack", 0.0), "0");
        // Fractional values preserved
        assert_eq!(format_exif_rational("GPSTrack", 359.99), "359.99");
    }

    #[test]
    fn test_gps_dest_distance_formatting() {
        // GPSDestDistance: whole numbers without decimals
        assert_eq!(format_exif_rational("GPSDestDistance", 100.0), "100");
        // Fractional values with minimal precision
        assert_eq!(format_exif_rational("GPSDestDistance", 12.345), "12.345");
    }

    // ------------------------------------------------------------------------
    // Resolution Tags (up to 10 decimal places)
    // ------------------------------------------------------------------------

    #[test]
    fn test_x_resolution_formatting() {
        // Integer resolution values should not show decimal point
        assert_eq!(format_exif_rational("XResolution", 72.0), "72");
        assert_eq!(format_exif_rational("XResolution", 300.0), "300");
        // Fractional values should show necessary precision
        assert_eq!(format_exif_rational("XResolution", 300.5), "300.5");
        // High precision if needed (up to 10 decimals)
        assert_eq!(
            format_exif_rational("XResolution", 72.1234567891),
            "72.1234567891"
        );
    }

    #[test]
    fn test_y_resolution_formatting() {
        // Same rules as XResolution
        assert_eq!(format_exif_rational("YResolution", 72.0), "72");
        assert_eq!(format_exif_rational("YResolution", 300.25), "300.25");
    }

    // ------------------------------------------------------------------------
    // Default Precision (6 decimal places)
    // ------------------------------------------------------------------------

    #[test]
    fn test_default_precision_for_unknown_tags() {
        // Unknown tags should use default 6 decimal places with trailing zeros trimmed
        assert_eq!(format_exif_rational("SomeUnknownTag", 3.5), "3.5");
        assert_eq!(format_exif_rational("AnotherTag", 3.123456), "3.123456");
        // Integer values should not show decimal point
        assert_eq!(format_exif_rational("AnyTag", 42.0), "42");
    }

    #[test]
    fn test_default_precision_truncates_beyond_6_decimals() {
        // Default should only show up to 6 decimal places
        // Note: values are rounded, not truncated
        assert_eq!(
            format_exif_rational("UnknownTag", 1.1234567890),
            "1.123457" // Rounded to 6 decimal places
        );
    }

    // ------------------------------------------------------------------------
    // Fully-Qualified Tag Names (with group prefix)
    // ------------------------------------------------------------------------

    #[test]
    fn test_fully_qualified_tag_names() {
        // Should extract base name and apply correct formatting
        assert_eq!(
            format_exif_rational("EXIF:BrightnessValue", 3.617254236),
            "3.617254236"
        );
        // GPS tags now show whole numbers without decimals
        assert_eq!(format_exif_rational("GPS:GPSImgDirection", 45.0), "45");
        assert_eq!(format_exif_rational("EXIF:XResolution", 72.0), "72");
        // Multiple colons (edge case)
        assert_eq!(
            format_exif_rational("Group:SubGroup:BrightnessValue", 5.0),
            "5"
        );
    }

    // ------------------------------------------------------------------------
    // Edge Cases
    // ------------------------------------------------------------------------

    #[test]
    fn test_zero_values() {
        // Zero should format correctly for each tag type
        assert_eq!(format_exif_rational("BrightnessValue", 0.0), "0");
        // GPS numeric tags: zero is displayed without decimals
        assert_eq!(format_exif_rational("GPSSpeed", 0.0), "0");
        assert_eq!(format_exif_rational("XResolution", 0.0), "0");
        assert_eq!(format_exif_rational("UnknownTag", 0.0), "0");
    }

    #[test]
    fn test_negative_values() {
        // BrightnessValue can be negative
        assert_eq!(format_exif_rational("BrightnessValue", -2.5), "-2.5");
        assert_eq!(
            format_exif_rational("BrightnessValue", -3.617254236),
            "-3.617254236"
        );
    }

    #[test]
    fn test_very_small_values() {
        // Very small values should preserve precision where possible
        assert_eq!(
            format_exif_rational("BrightnessValue", 0.000000001),
            "0.000000001"
        );
        assert_eq!(format_exif_rational("GPSSpeed", 0.01), "0.01");
    }

    #[test]
    fn test_very_large_values() {
        // Large values should format correctly
        assert_eq!(
            format_exif_rational("BrightnessValue", 999999.123456789),
            "999999.123456789"
        );
        assert_eq!(format_exif_rational("GPSTrack", 999.99), "999.99");
    }

    // ------------------------------------------------------------------------
    // Helper Function Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_is_high_precision_tag() {
        assert!(is_high_precision_tag("BrightnessValue"));
        assert!(is_high_precision_tag("DigitalZoomRatio"));
        assert!(is_high_precision_tag("CompressedBitsPerPixel"));
        assert!(is_high_precision_tag("EXIF:BrightnessValue"));
        assert!(!is_high_precision_tag("FocalLength"));
        assert!(!is_high_precision_tag("GPSSpeed"));
    }

    #[test]
    fn test_is_gps_numeric_tag() {
        assert!(is_gps_numeric_tag("GPSDestBearing"));
        assert!(is_gps_numeric_tag("GPSImgDirection"));
        assert!(is_gps_numeric_tag("GPSSpeed"));
        assert!(is_gps_numeric_tag("GPSTrack"));
        assert!(is_gps_numeric_tag("GPSDestDistance"));
        assert!(is_gps_numeric_tag("GPS:GPSSpeed"));
        assert!(!is_gps_numeric_tag("GPSLatitude"));
        assert!(!is_gps_numeric_tag("BrightnessValue"));
    }

    #[test]
    fn test_is_icc_matrix_tag() {
        assert!(is_icc_matrix_tag("ChromaticAdaptation"));
        assert!(is_icc_matrix_tag("RedMatrixColumn"));
        assert!(is_icc_matrix_tag("ICC_Profile:BlueMatrixColumn"));
        assert!(!is_icc_matrix_tag("GPSSpeed"));
        assert!(!is_icc_matrix_tag("XResolution"));
    }

    #[test]
    fn test_is_resolution_tag() {
        assert!(is_resolution_tag("XResolution"));
        assert!(is_resolution_tag("YResolution"));
        assert!(is_resolution_tag("EXIF:XResolution"));
        assert!(!is_resolution_tag("FocalPlaneXResolution"));
        assert!(!is_resolution_tag("GPSSpeed"));
    }

    // ------------------------------------------------------------------------
    // Tag List Verification
    // ------------------------------------------------------------------------

    #[test]
    fn test_high_precision_tags_list() {
        assert_eq!(HIGH_PRECISION_9_TAGS.len(), 3);
        assert!(HIGH_PRECISION_9_TAGS.contains(&"BrightnessValue"));
        assert!(HIGH_PRECISION_9_TAGS.contains(&"CompressedBitsPerPixel"));
        assert!(HIGH_PRECISION_9_TAGS.contains(&"DigitalZoomRatio"));
    }

    #[test]
    fn test_gps_numeric_tags_list() {
        assert_eq!(GPS_NUMERIC_TAGS.len(), 5);
        assert!(GPS_NUMERIC_TAGS.contains(&"GPSDestBearing"));
        assert!(GPS_NUMERIC_TAGS.contains(&"GPSImgDirection"));
        assert!(GPS_NUMERIC_TAGS.contains(&"GPSSpeed"));
        assert!(GPS_NUMERIC_TAGS.contains(&"GPSTrack"));
        assert!(GPS_NUMERIC_TAGS.contains(&"GPSDestDistance"));
    }

    #[test]
    fn test_icc_matrix_tags_list() {
        // Verify key ICC matrix tags are in the list
        assert!(ICC_MATRIX_TAGS.contains(&"ChromaticAdaptation"));
        assert!(ICC_MATRIX_TAGS.contains(&"RedMatrixColumn"));
        assert!(ICC_MATRIX_TAGS.contains(&"GreenMatrixColumn"));
        assert!(ICC_MATRIX_TAGS.contains(&"BlueMatrixColumn"));
    }

    #[test]
    fn test_resolution_tags_list() {
        assert_eq!(RESOLUTION_TAGS.len(), 2);
        assert!(RESOLUTION_TAGS.contains(&"XResolution"));
        assert!(RESOLUTION_TAGS.contains(&"YResolution"));
    }

    // ------------------------------------------------------------------------
    // Internal Helper Function Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_format_with_precision() {
        // Verify trailing zero trimming behavior
        assert_eq!(format_with_precision(3.5, 9), "3.5");
        assert_eq!(format_with_precision(72.0, 10), "72");
        assert_eq!(format_with_precision(3.617254236, 9), "3.617254236");
        assert_eq!(format_with_precision(1.0, 6), "1");
        assert_eq!(format_with_precision(0.0, 6), "0");
        assert_eq!(format_with_precision(-1.5, 6), "-1.5");
    }

    #[test]
    fn test_format_gps_numeric() {
        // Whole numbers display without decimals
        assert_eq!(format_gps_numeric(20.0), "20");
        assert_eq!(format_gps_numeric(180.0), "180");
        assert_eq!(format_gps_numeric(0.0), "0");

        // Fractional values use minimal precision
        assert_eq!(format_gps_numeric(45.5), "45.5");
        assert_eq!(format_gps_numeric(123.456), "123.456");
        assert_eq!(format_gps_numeric(90.25), "90.25");

        // Very small fractions preserved
        assert_eq!(format_gps_numeric(0.01), "0.01");
    }

    // ------------------------------------------------------------------------
    // ICC Profile Matrix Tag Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_icc_matrix_formatting() {
        // ICC profile values are limited to 5 decimal places
        assert_eq!(
            format_exif_rational("ChromaticAdaptation", 1.04788),
            "1.04788"
        );
        // Trailing zeros trimmed
        assert_eq!(format_exif_rational("RedMatrixColumn", 0.436), "0.436");
        // Whole numbers display without decimals
        assert_eq!(format_exif_rational("BlueMatrixColumn", 1.0), "1");
        // Values beyond 5 decimals are rounded
        assert_eq!(
            format_exif_rational("GreenMatrixColumn", 0.3851234567),
            "0.38512"
        );
    }
}
