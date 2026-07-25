//! GPS Status and Mode Value Formatters
//!
//! This module provides formatting functions for GPS-related status and mode values
//! to convert raw EXIF values into human-readable strings matching ExifTool's output.
//!
//! # Overview
//!
//! GPS tags in EXIF metadata store status and mode information as single characters
//! or numeric codes. ExifTool displays these as human-readable descriptions. This
//! module provides the conversion functions to achieve parity with ExifTool output.
//!
//! # Supported Tags
//!
//! - **GPSStatus**: Indicates whether GPS measurement is active
//!   - "A" -> "Measurement Active"
//!   - "V" -> "Measurement Void"
//!
//! - **GPSMeasureMode**: Indicates the GPS measurement dimensionality
//!   - "2" -> "2-Dimensional Measurement"
//!   - "3" -> "3-Dimensional Measurement"
//!
//! - **GPSDifferential**: Indicates if differential GPS correction was applied
//!   - "0" or 0 -> "No Correction"
//!   - "1" or 1 -> "Differential Corrected"
//!
//! # Example
//!
//! ```
//! use oxidex::core::formatters::gps_status::{
//!     format_gps_status, format_gps_measure_mode, format_gps_differential
//! };
//!
//! assert_eq!(format_gps_status("A"), Some("Measurement Active".to_string()));
//! assert_eq!(format_gps_measure_mode("3"), Some("3-Dimensional Measurement".to_string()));
//! assert_eq!(format_gps_differential("0"), Some("No Correction".to_string()));
//! ```

// =============================================================================
// GPS STATUS FORMATTER
// =============================================================================

/// Format GPSStatus value to human-readable description.
///
/// The GPSStatus tag indicates the status of the GPS receiver when the image
/// was recorded. Per the EXIF specification:
/// - "A" indicates measurement is active (GPS receiver is actively tracking)
/// - "V" indicates measurement is void (GPS receiver is not tracking)
///
/// # Arguments
///
/// * `value` - The raw GPSStatus value (typically "A" or "V")
///
/// # Returns
///
/// - `Some("Measurement Active")` if value is "A"
/// - `Some("Measurement Void")` if value is "V"
/// - `None` if the value is unrecognized
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_status::format_gps_status;
///
/// assert_eq!(format_gps_status("A"), Some("Measurement Active".to_string()));
/// assert_eq!(format_gps_status("V"), Some("Measurement Void".to_string()));
/// assert_eq!(format_gps_status("X"), None);
/// assert_eq!(format_gps_status(""), None);
/// ```
pub fn format_gps_status(value: &str) -> Option<String> {
    // Trim whitespace to handle values with trailing spaces or null bytes
    match value.trim() {
        "A" => Some("Measurement Active".to_string()),
        "V" => Some("Measurement Void".to_string()),
        _ => None,
    }
}

// =============================================================================
// GPS MEASURE MODE FORMATTER
// =============================================================================

/// Format GPSMeasureMode value to human-readable description.
///
/// The GPSMeasureMode tag indicates the GPS measurement mode. Per the EXIF
/// specification:
/// - "2" indicates 2-dimensional measurement (latitude and longitude only)
/// - "3" indicates 3-dimensional measurement (latitude, longitude, and altitude)
///
/// # Arguments
///
/// * `value` - The raw GPSMeasureMode value (typically "2" or "3")
///
/// # Returns
///
/// - `Some("2-Dimensional Measurement")` if value is "2"
/// - `Some("3-Dimensional Measurement")` if value is "3"
/// - `None` if the value is unrecognized
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_status::format_gps_measure_mode;
///
/// assert_eq!(format_gps_measure_mode("2"), Some("2-Dimensional Measurement".to_string()));
/// assert_eq!(format_gps_measure_mode("3"), Some("3-Dimensional Measurement".to_string()));
/// assert_eq!(format_gps_measure_mode("1"), None);
/// assert_eq!(format_gps_measure_mode(""), None);
/// ```
pub fn format_gps_measure_mode(value: &str) -> Option<String> {
    // Trim whitespace to handle values with trailing spaces or null bytes
    match value.trim() {
        "2" => Some("2-Dimensional Measurement".to_string()),
        "3" => Some("3-Dimensional Measurement".to_string()),
        _ => None,
    }
}

// =============================================================================
// GPS DIFFERENTIAL FORMATTER
// =============================================================================

/// Format GPSDifferential value to human-readable description.
///
/// The GPSDifferential tag indicates whether differential correction is applied
/// to the GPS receiver. Per the EXIF specification:
/// - 0 indicates no differential correction is applied
/// - 1 indicates differential correction is applied (DGPS)
///
/// This function accepts both string and numeric representations since EXIF
/// data may be parsed as either format depending on the source.
///
/// # Arguments
///
/// * `value` - The raw GPSDifferential value (string: "0"/"1", or byte: "\x00"/"\x01")
///
/// # Returns
///
/// - `Some("No Correction")` if value represents 0
/// - `Some("Differential Corrected")` if value represents 1
/// - `None` if the value is unrecognized
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_status::format_gps_differential;
///
/// // String numeric values
/// assert_eq!(format_gps_differential("0"), Some("No Correction".to_string()));
/// assert_eq!(format_gps_differential("1"), Some("Differential Corrected".to_string()));
///
/// // Binary byte values (null byte for 0, SOH byte for 1)
/// assert_eq!(format_gps_differential("\x00"), Some("No Correction".to_string()));
/// assert_eq!(format_gps_differential("\x01"), Some("Differential Corrected".to_string()));
///
/// // Unknown values
/// assert_eq!(format_gps_differential("2"), None);
/// assert_eq!(format_gps_differential(""), None);
/// ```
pub fn format_gps_differential(value: &str) -> Option<String> {
    // Handle both string representations ("0", "1") and binary representations
    // ("\x00", "\x01") since EXIF data may come in either format depending
    // on how it was parsed from the binary file.
    match value.trim() {
        "0" | "\x00" => Some("No Correction".to_string()),
        "1" | "\x01" => Some("Differential Corrected".to_string()),
        _ => None,
    }
}

/// Format GPSDifferential from a numeric value.
///
/// This is a convenience function for formatting GPSDifferential when the
/// value is already parsed as a numeric type.
///
/// # Arguments
///
/// * `value` - The numeric GPSDifferential value (0 or 1)
///
/// # Returns
///
/// - `Some("No Correction")` if value is 0
/// - `Some("Differential Corrected")` if value is 1
/// - `None` if the value is unrecognized
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_status::format_gps_differential_numeric;
///
/// assert_eq!(format_gps_differential_numeric(0), Some("No Correction".to_string()));
/// assert_eq!(format_gps_differential_numeric(1), Some("Differential Corrected".to_string()));
/// assert_eq!(format_gps_differential_numeric(2), None);
/// assert_eq!(format_gps_differential_numeric(-1), None);
/// ```
pub fn format_gps_differential_numeric(value: i32) -> Option<String> {
    match value {
        0 => Some("No Correction".to_string()),
        1 => Some("Differential Corrected".to_string()),
        _ => None,
    }
}

// =============================================================================
// COMBINED GPS STATUS/MODE FORMATTER
// =============================================================================

/// Format any GPS status/mode tag value based on the tag name.
///
/// This is a convenience function that routes to the appropriate formatter
/// based on the tag name, handling fully-qualified tag names with group
/// prefixes (e.g., "GPS:GPSStatus").
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally prefixed with group (e.g., "GPSStatus", "GPS:GPSMeasureMode")
/// * `value` - The raw value string to format
///
/// # Returns
///
/// The human-readable description, or `None` if the tag is not recognized
/// or the value cannot be formatted.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_status::format_gps_status_tag;
///
/// // Without group prefix
/// assert_eq!(format_gps_status_tag("GPSStatus", "A"), Some("Measurement Active".to_string()));
/// assert_eq!(format_gps_status_tag("GPSMeasureMode", "3"), Some("3-Dimensional Measurement".to_string()));
/// assert_eq!(format_gps_status_tag("GPSDifferential", "1"), Some("Differential Corrected".to_string()));
///
/// // With group prefix
/// assert_eq!(format_gps_status_tag("GPS:GPSStatus", "V"), Some("Measurement Void".to_string()));
/// assert_eq!(format_gps_status_tag("EXIF:GPSMeasureMode", "2"), Some("2-Dimensional Measurement".to_string()));
///
/// // Unknown tag or value
/// assert_eq!(format_gps_status_tag("SomeOtherTag", "A"), None);
/// assert_eq!(format_gps_status_tag("GPSStatus", "X"), None);
/// ```
pub fn format_gps_status_tag(tag_name: &str, value: &str) -> Option<String> {
    // Extract the base tag name by taking the last segment after any colons.
    // This handles fully-qualified names like "GPS:GPSStatus" or "EXIF:GPSMeasureMode".
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);

    match base_name {
        "GPSStatus" => format_gps_status(value),
        "GPSMeasureMode" => format_gps_measure_mode(value),
        "GPSDifferential" => format_gps_differential(value),
        _ => None,
    }
}

/// List of GPS status/mode tag names handled by this module.
///
/// This can be used to quickly check if a tag should be routed to this
/// module's formatters.
pub const GPS_STATUS_MODE_TAGS: &[&str] = &["GPSStatus", "GPSMeasureMode", "GPSDifferential"];

/// Check if a tag name is a GPS status/mode tag handled by this module.
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally prefixed with group
///
/// # Returns
///
/// `true` if the tag is handled by this module, `false` otherwise.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_status::is_gps_status_mode_tag;
///
/// assert!(is_gps_status_mode_tag("GPSStatus"));
/// assert!(is_gps_status_mode_tag("GPS:GPSMeasureMode"));
/// assert!(is_gps_status_mode_tag("EXIF:GPSDifferential"));
/// assert!(!is_gps_status_mode_tag("GPSLatitude"));
/// assert!(!is_gps_status_mode_tag("Make"));
/// ```
pub fn is_gps_status_mode_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    GPS_STATUS_MODE_TAGS.contains(&base_name)
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // GPSStatus Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_status_valid_values() {
        // Active measurement
        assert_eq!(
            format_gps_status("A"),
            Some("Measurement Active".to_string())
        );

        // Void measurement
        assert_eq!(format_gps_status("V"), Some("Measurement Void".to_string()));
    }

    #[test]
    fn test_format_gps_status_with_whitespace() {
        // Values with leading/trailing whitespace should be trimmed
        assert_eq!(
            format_gps_status(" A"),
            Some("Measurement Active".to_string())
        );
        assert_eq!(
            format_gps_status("A "),
            Some("Measurement Active".to_string())
        );
        assert_eq!(
            format_gps_status(" A "),
            Some("Measurement Active".to_string())
        );
        assert_eq!(
            format_gps_status("\tV\n"),
            Some("Measurement Void".to_string())
        );
    }

    #[test]
    fn test_format_gps_status_invalid_values() {
        // Unknown values should return None
        assert_eq!(format_gps_status("X"), None);
        assert_eq!(format_gps_status("a"), None); // lowercase not valid
        assert_eq!(format_gps_status("v"), None); // lowercase not valid
        assert_eq!(format_gps_status(""), None);
        assert_eq!(format_gps_status("Active"), None);
        assert_eq!(format_gps_status("0"), None);
        assert_eq!(format_gps_status("1"), None);
    }

    // -------------------------------------------------------------------------
    // GPSMeasureMode Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_measure_mode_valid_values() {
        // 2-dimensional measurement
        assert_eq!(
            format_gps_measure_mode("2"),
            Some("2-Dimensional Measurement".to_string())
        );

        // 3-dimensional measurement
        assert_eq!(
            format_gps_measure_mode("3"),
            Some("3-Dimensional Measurement".to_string())
        );
    }

    #[test]
    fn test_format_gps_measure_mode_with_whitespace() {
        // Values with leading/trailing whitespace should be trimmed
        assert_eq!(
            format_gps_measure_mode(" 2"),
            Some("2-Dimensional Measurement".to_string())
        );
        assert_eq!(
            format_gps_measure_mode("3 "),
            Some("3-Dimensional Measurement".to_string())
        );
        assert_eq!(
            format_gps_measure_mode(" 2 "),
            Some("2-Dimensional Measurement".to_string())
        );
    }

    #[test]
    fn test_format_gps_measure_mode_invalid_values() {
        // Unknown values should return None
        assert_eq!(format_gps_measure_mode("1"), None);
        assert_eq!(format_gps_measure_mode("0"), None);
        assert_eq!(format_gps_measure_mode("4"), None);
        assert_eq!(format_gps_measure_mode(""), None);
        assert_eq!(format_gps_measure_mode("two"), None);
        assert_eq!(format_gps_measure_mode("2D"), None);
    }

    // -------------------------------------------------------------------------
    // GPSDifferential Tests (String Input)
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_differential_string_values() {
        // String "0" for no correction
        assert_eq!(
            format_gps_differential("0"),
            Some("No Correction".to_string())
        );

        // String "1" for differential corrected
        assert_eq!(
            format_gps_differential("1"),
            Some("Differential Corrected".to_string())
        );
    }

    #[test]
    fn test_format_gps_differential_binary_values() {
        // Binary null byte (0x00) for no correction
        assert_eq!(
            format_gps_differential("\x00"),
            Some("No Correction".to_string())
        );

        // Binary SOH byte (0x01) for differential corrected
        assert_eq!(
            format_gps_differential("\x01"),
            Some("Differential Corrected".to_string())
        );
    }

    #[test]
    fn test_format_gps_differential_with_whitespace() {
        // Values with leading/trailing whitespace should be trimmed
        assert_eq!(
            format_gps_differential(" 0"),
            Some("No Correction".to_string())
        );
        assert_eq!(
            format_gps_differential("1 "),
            Some("Differential Corrected".to_string())
        );
        assert_eq!(
            format_gps_differential(" 0 "),
            Some("No Correction".to_string())
        );
    }

    #[test]
    fn test_format_gps_differential_invalid_values() {
        // Unknown values should return None
        assert_eq!(format_gps_differential("2"), None);
        assert_eq!(format_gps_differential("-1"), None);
        assert_eq!(format_gps_differential(""), None);
        assert_eq!(format_gps_differential("yes"), None);
        assert_eq!(format_gps_differential("no"), None);
        assert_eq!(format_gps_differential("true"), None);
    }

    // -------------------------------------------------------------------------
    // GPSDifferential Tests (Numeric Input)
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_differential_numeric_valid_values() {
        // 0 for no correction
        assert_eq!(
            format_gps_differential_numeric(0),
            Some("No Correction".to_string())
        );

        // 1 for differential corrected
        assert_eq!(
            format_gps_differential_numeric(1),
            Some("Differential Corrected".to_string())
        );
    }

    #[test]
    fn test_format_gps_differential_numeric_invalid_values() {
        // Values outside 0-1 range should return None
        assert_eq!(format_gps_differential_numeric(2), None);
        assert_eq!(format_gps_differential_numeric(-1), None);
        assert_eq!(format_gps_differential_numeric(100), None);
        assert_eq!(format_gps_differential_numeric(i32::MAX), None);
        assert_eq!(format_gps_differential_numeric(i32::MIN), None);
    }

    // -------------------------------------------------------------------------
    // Combined Format Function Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_status_tag_without_prefix() {
        // GPSStatus
        assert_eq!(
            format_gps_status_tag("GPSStatus", "A"),
            Some("Measurement Active".to_string())
        );
        assert_eq!(
            format_gps_status_tag("GPSStatus", "V"),
            Some("Measurement Void".to_string())
        );

        // GPSMeasureMode
        assert_eq!(
            format_gps_status_tag("GPSMeasureMode", "2"),
            Some("2-Dimensional Measurement".to_string())
        );
        assert_eq!(
            format_gps_status_tag("GPSMeasureMode", "3"),
            Some("3-Dimensional Measurement".to_string())
        );

        // GPSDifferential
        assert_eq!(
            format_gps_status_tag("GPSDifferential", "0"),
            Some("No Correction".to_string())
        );
        assert_eq!(
            format_gps_status_tag("GPSDifferential", "1"),
            Some("Differential Corrected".to_string())
        );
    }

    #[test]
    fn test_format_gps_status_tag_with_prefix() {
        // With GPS: prefix
        assert_eq!(
            format_gps_status_tag("GPS:GPSStatus", "A"),
            Some("Measurement Active".to_string())
        );
        assert_eq!(
            format_gps_status_tag("GPS:GPSMeasureMode", "3"),
            Some("3-Dimensional Measurement".to_string())
        );
        assert_eq!(
            format_gps_status_tag("GPS:GPSDifferential", "0"),
            Some("No Correction".to_string())
        );

        // With EXIF: prefix (some systems may use this)
        assert_eq!(
            format_gps_status_tag("EXIF:GPSStatus", "V"),
            Some("Measurement Void".to_string())
        );
    }

    #[test]
    fn test_format_gps_status_tag_unknown_tags() {
        // Unknown tags should return None even with valid-looking values
        assert_eq!(format_gps_status_tag("GPSLatitude", "A"), None);
        assert_eq!(format_gps_status_tag("GPSLongitude", "V"), None);
        assert_eq!(format_gps_status_tag("Make", "A"), None);
        assert_eq!(format_gps_status_tag("Model", "2"), None);
        assert_eq!(format_gps_status_tag("", "A"), None);
    }

    #[test]
    fn test_format_gps_status_tag_invalid_values() {
        // Valid tags with invalid values should return None
        assert_eq!(format_gps_status_tag("GPSStatus", "X"), None);
        assert_eq!(format_gps_status_tag("GPSMeasureMode", "4"), None);
        assert_eq!(format_gps_status_tag("GPSDifferential", "2"), None);
    }

    // -------------------------------------------------------------------------
    // Tag List and Helper Function Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_status_mode_tags_list() {
        // Verify the list contains expected entries
        assert!(GPS_STATUS_MODE_TAGS.contains(&"GPSStatus"));
        assert!(GPS_STATUS_MODE_TAGS.contains(&"GPSMeasureMode"));
        assert!(GPS_STATUS_MODE_TAGS.contains(&"GPSDifferential"));

        // Verify expected count
        assert_eq!(GPS_STATUS_MODE_TAGS.len(), 3);

        // Verify it does NOT contain other GPS tags
        assert!(!GPS_STATUS_MODE_TAGS.contains(&"GPSLatitude"));
        assert!(!GPS_STATUS_MODE_TAGS.contains(&"GPSLongitude"));
        assert!(!GPS_STATUS_MODE_TAGS.contains(&"GPSAltitude"));
    }

    #[test]
    fn test_is_gps_status_mode_tag_valid() {
        // Tags handled by this module
        assert!(is_gps_status_mode_tag("GPSStatus"));
        assert!(is_gps_status_mode_tag("GPSMeasureMode"));
        assert!(is_gps_status_mode_tag("GPSDifferential"));
    }

    #[test]
    fn test_is_gps_status_mode_tag_with_prefix() {
        // With various prefixes
        assert!(is_gps_status_mode_tag("GPS:GPSStatus"));
        assert!(is_gps_status_mode_tag("EXIF:GPSMeasureMode"));
        assert!(is_gps_status_mode_tag("IFD0:GPSDifferential"));
    }

    #[test]
    fn test_is_gps_status_mode_tag_invalid() {
        // Tags not handled by this module
        assert!(!is_gps_status_mode_tag("GPSLatitude"));
        assert!(!is_gps_status_mode_tag("GPSLongitude"));
        assert!(!is_gps_status_mode_tag("GPSAltitude"));
        assert!(!is_gps_status_mode_tag("GPSLatitudeRef"));
        assert!(!is_gps_status_mode_tag("Make"));
        assert!(!is_gps_status_mode_tag("Model"));
        assert!(!is_gps_status_mode_tag(""));
    }

    // -------------------------------------------------------------------------
    // Edge Case Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_edge_case_empty_strings() {
        assert_eq!(format_gps_status(""), None);
        assert_eq!(format_gps_measure_mode(""), None);
        assert_eq!(format_gps_differential(""), None);
        assert_eq!(format_gps_status_tag("", ""), None);
    }

    #[test]
    fn test_edge_case_whitespace_only() {
        assert_eq!(format_gps_status("   "), None);
        assert_eq!(format_gps_measure_mode("\t\n"), None);
        assert_eq!(format_gps_differential("  "), None);
    }

    #[test]
    fn test_edge_case_multiple_colons_in_tag_name() {
        // Should use the last segment after splitting by ':'
        assert_eq!(
            format_gps_status_tag("Foo:Bar:GPSStatus", "A"),
            Some("Measurement Active".to_string())
        );
    }
}
