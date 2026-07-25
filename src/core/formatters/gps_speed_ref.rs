//! GPS Speed and Distance Reference Value Formatters
//!
//! This module provides formatting functions to convert raw GPS reference codes
//! to human-readable strings that match ExifTool's output format.
//!
//! ## Background
//!
//! GPS metadata tags store speed and distance reference units as single-character codes:
//! - "K" = Kilometers (or km/h for speed)
//! - "M" = Miles (or mph for speed)
//! - "N" = Nautical Miles (or knots for speed)
//!
//! ExifTool displays these as human-readable descriptions rather than raw codes.
//! This module provides the conversion logic to match ExifTool's output format.
//!
//! ## Supported Tags
//!
//! - **GPSSpeedRef**: Speed unit reference (km/h, mph, knots)
//! - **GPSDestDistanceRef**: Distance unit reference (Kilometers, Miles, Nautical Miles)
//!
//! ## Usage
//!
//! ```rust
//! use oxidex::core::formatters::gps_speed_ref::{format_gps_speed_ref, format_gps_dest_distance_ref};
//!
//! // Speed reference formatting
//! assert_eq!(format_gps_speed_ref("K"), Some("km/h".to_string()));
//! assert_eq!(format_gps_speed_ref("M"), Some("mph".to_string()));
//! assert_eq!(format_gps_speed_ref("N"), Some("knots".to_string()));
//!
//! // Distance reference formatting
//! assert_eq!(format_gps_dest_distance_ref("K"), Some("Kilometers".to_string()));
//! assert_eq!(format_gps_dest_distance_ref("M"), Some("Miles".to_string()));
//! assert_eq!(format_gps_dest_distance_ref("N"), Some("Nautical Miles".to_string()));
//! ```

/// GPS Speed Reference unit codes as defined in the EXIF specification.
///
/// These are the valid single-character codes that can appear in the
/// GPSSpeedRef tag to indicate the unit of measurement for GPS speed.
pub mod speed_ref_codes {
    /// Kilometers per hour
    pub const KILOMETERS_PER_HOUR: &str = "K";
    /// Miles per hour
    pub const MILES_PER_HOUR: &str = "M";
    /// Knots (nautical miles per hour)
    pub const KNOTS: &str = "N";
}

/// GPS Destination Distance Reference unit codes as defined in the EXIF specification.
///
/// These are the valid single-character codes that can appear in the
/// GPSDestDistanceRef tag to indicate the unit of measurement for GPS distance.
pub mod distance_ref_codes {
    /// Kilometers
    pub const KILOMETERS: &str = "K";
    /// Miles (statute miles)
    pub const MILES: &str = "M";
    /// Nautical miles
    pub const NAUTICAL_MILES: &str = "N";
}

/// Format a GPSSpeedRef value to match ExifTool's output.
///
/// Converts the raw single-character speed reference code to a human-readable
/// speed unit string. This matches ExifTool's formatting for the GPSSpeedRef tag.
///
/// # Arguments
///
/// * `value` - The raw GPSSpeedRef value (typically "K", "M", or "N")
///
/// # Returns
///
/// * `Some(String)` - The formatted speed unit if the input is a valid code
/// * `None` - If the input is not a recognized speed reference code
///
/// # Mapping
///
/// | Input | Output  |
/// |-------|---------|
/// | "K"   | "km/h"  |
/// | "M"   | "mph"   |
/// | "N"   | "knots" |
///
/// # Examples
///
/// ```rust
/// use oxidex::core::formatters::gps_speed_ref::format_gps_speed_ref;
///
/// // Valid speed reference codes
/// assert_eq!(format_gps_speed_ref("K"), Some("km/h".to_string()));
/// assert_eq!(format_gps_speed_ref("M"), Some("mph".to_string()));
/// assert_eq!(format_gps_speed_ref("N"), Some("knots".to_string()));
///
/// // Handles whitespace in input
/// assert_eq!(format_gps_speed_ref(" K "), Some("km/h".to_string()));
///
/// // Returns None for unrecognized codes
/// assert_eq!(format_gps_speed_ref("X"), None);
/// assert_eq!(format_gps_speed_ref(""), None);
/// ```
pub fn format_gps_speed_ref(value: &str) -> Option<String> {
    // Trim whitespace to handle values that may have padding
    // (some parsers may include trailing spaces or null bytes)
    match value.trim() {
        speed_ref_codes::KILOMETERS_PER_HOUR => Some("km/h".to_string()),
        speed_ref_codes::MILES_PER_HOUR => Some("mph".to_string()),
        speed_ref_codes::KNOTS => Some("knots".to_string()),
        _ => None,
    }
}

/// Format a GPSDestDistanceRef value to match ExifTool's output.
///
/// Converts the raw single-character distance reference code to a human-readable
/// distance unit string. This matches ExifTool's formatting for the GPSDestDistanceRef tag.
///
/// # Arguments
///
/// * `value` - The raw GPSDestDistanceRef value (typically "K", "M", or "N")
///
/// # Returns
///
/// * `Some(String)` - The formatted distance unit if the input is a valid code
/// * `None` - If the input is not a recognized distance reference code
///
/// # Mapping
///
/// | Input | Output           |
/// |-------|------------------|
/// | "K"   | "Kilometers"     |
/// | "M"   | "Miles"          |
/// | "N"   | "Nautical Miles" |
///
/// # Examples
///
/// ```rust
/// use oxidex::core::formatters::gps_speed_ref::format_gps_dest_distance_ref;
///
/// // Valid distance reference codes
/// assert_eq!(format_gps_dest_distance_ref("K"), Some("Kilometers".to_string()));
/// assert_eq!(format_gps_dest_distance_ref("M"), Some("Miles".to_string()));
/// assert_eq!(format_gps_dest_distance_ref("N"), Some("Nautical Miles".to_string()));
///
/// // Handles whitespace in input
/// assert_eq!(format_gps_dest_distance_ref(" M "), Some("Miles".to_string()));
///
/// // Returns None for unrecognized codes
/// assert_eq!(format_gps_dest_distance_ref("X"), None);
/// assert_eq!(format_gps_dest_distance_ref(""), None);
/// ```
pub fn format_gps_dest_distance_ref(value: &str) -> Option<String> {
    // Trim whitespace to handle values that may have padding
    // (some parsers may include trailing spaces or null bytes)
    match value.trim() {
        distance_ref_codes::KILOMETERS => Some("Kilometers".to_string()),
        distance_ref_codes::MILES => Some("Miles".to_string()),
        distance_ref_codes::NAUTICAL_MILES => Some("Nautical Miles".to_string()),
        _ => None,
    }
}

/// Format a GPS speed or distance reference value based on the tag name.
///
/// This is a convenience function that dispatches to the appropriate formatter
/// based on the tag name. It handles both simple tag names and fully-qualified
/// tag names with group prefixes (e.g., "GPS:GPSSpeedRef").
///
/// # Arguments
///
/// * `tag_name` - The name of the GPS reference tag (e.g., "GPSSpeedRef", "GPS:GPSDestDistanceRef")
/// * `value` - The raw reference value (typically "K", "M", or "N")
///
/// # Returns
///
/// * `Some(String)` - The formatted value if the tag is recognized and the value is valid
/// * `None` - If the tag is not a speed/distance reference or the value is not recognized
///
/// # Supported Tags
///
/// - `GPSSpeedRef` - Formats using [`format_gps_speed_ref`]
/// - `GPSDestDistanceRef` - Formats using [`format_gps_dest_distance_ref`]
///
/// # Examples
///
/// ```rust
/// use oxidex::core::formatters::gps_speed_ref::format_gps_speed_or_distance_ref;
///
/// // Speed reference formatting
/// assert_eq!(format_gps_speed_or_distance_ref("GPSSpeedRef", "K"), Some("km/h".to_string()));
/// assert_eq!(format_gps_speed_or_distance_ref("GPS:GPSSpeedRef", "N"), Some("knots".to_string()));
///
/// // Distance reference formatting
/// assert_eq!(format_gps_speed_or_distance_ref("GPSDestDistanceRef", "M"), Some("Miles".to_string()));
/// assert_eq!(format_gps_speed_or_distance_ref("GPS:GPSDestDistanceRef", "K"), Some("Kilometers".to_string()));
///
/// // Unrecognized tag returns None
/// assert_eq!(format_gps_speed_or_distance_ref("GPSLatitudeRef", "N"), None);
/// ```
pub fn format_gps_speed_or_distance_ref(tag_name: &str, value: &str) -> Option<String> {
    // Extract the base tag name by taking the part after the last colon.
    // This handles fully-qualified names like "GPS:GPSSpeedRef" -> "GPSSpeedRef"
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);

    match base_name {
        "GPSSpeedRef" => format_gps_speed_ref(value),
        "GPSDestDistanceRef" => format_gps_dest_distance_ref(value),
        _ => None,
    }
}

/// Check if a tag name is a GPS speed or distance reference tag.
///
/// This function determines whether a given tag name should have its value
/// formatted using the GPS speed/distance reference formatters.
///
/// # Arguments
///
/// * `tag_name` - The tag name to check (supports group prefixes like "GPS:GPSSpeedRef")
///
/// # Returns
///
/// `true` if the tag is `GPSSpeedRef` or `GPSDestDistanceRef`, `false` otherwise.
///
/// # Examples
///
/// ```rust
/// use oxidex::core::formatters::gps_speed_ref::is_gps_speed_or_distance_ref_tag;
///
/// // Recognized tags
/// assert!(is_gps_speed_or_distance_ref_tag("GPSSpeedRef"));
/// assert!(is_gps_speed_or_distance_ref_tag("GPSDestDistanceRef"));
/// assert!(is_gps_speed_or_distance_ref_tag("GPS:GPSSpeedRef"));
///
/// // Other tags return false
/// assert!(!is_gps_speed_or_distance_ref_tag("GPSLatitudeRef"));
/// assert!(!is_gps_speed_or_distance_ref_tag("GPSAltitudeRef"));
/// ```
pub fn is_gps_speed_or_distance_ref_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    matches!(base_name, "GPSSpeedRef" | "GPSDestDistanceRef")
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Tests for format_gps_speed_ref
    // ------------------------------------------------------------------------

    #[test]
    fn test_format_gps_speed_ref_kilometers_per_hour() {
        // "K" should format to "km/h" to match ExifTool's output
        assert_eq!(format_gps_speed_ref("K"), Some("km/h".to_string()));
    }

    #[test]
    fn test_format_gps_speed_ref_miles_per_hour() {
        // "M" should format to "mph" to match ExifTool's output
        assert_eq!(format_gps_speed_ref("M"), Some("mph".to_string()));
    }

    #[test]
    fn test_format_gps_speed_ref_knots() {
        // "N" should format to "knots" to match ExifTool's output
        assert_eq!(format_gps_speed_ref("N"), Some("knots".to_string()));
    }

    #[test]
    fn test_format_gps_speed_ref_with_whitespace() {
        // Function should handle values with leading/trailing whitespace
        assert_eq!(format_gps_speed_ref(" K"), Some("km/h".to_string()));
        assert_eq!(format_gps_speed_ref("K "), Some("km/h".to_string()));
        assert_eq!(format_gps_speed_ref(" K "), Some("km/h".to_string()));
        assert_eq!(format_gps_speed_ref("\tM\t"), Some("mph".to_string()));
        assert_eq!(format_gps_speed_ref("  N  "), Some("knots".to_string()));
    }

    #[test]
    fn test_format_gps_speed_ref_invalid_codes() {
        // Invalid or unrecognized codes should return None
        assert_eq!(format_gps_speed_ref("X"), None);
        assert_eq!(format_gps_speed_ref(""), None);
        assert_eq!(format_gps_speed_ref("km/h"), None); // Already formatted
        assert_eq!(format_gps_speed_ref("mph"), None);
        assert_eq!(format_gps_speed_ref("knots"), None);
    }

    #[test]
    fn test_format_gps_speed_ref_case_sensitive() {
        // EXIF spec uses uppercase letters; lowercase should not match
        assert_eq!(format_gps_speed_ref("k"), None);
        assert_eq!(format_gps_speed_ref("m"), None);
        assert_eq!(format_gps_speed_ref("n"), None);
    }

    // ------------------------------------------------------------------------
    // Tests for format_gps_dest_distance_ref
    // ------------------------------------------------------------------------

    #[test]
    fn test_format_gps_dest_distance_ref_kilometers() {
        // "K" should format to "Kilometers" to match ExifTool's output
        assert_eq!(
            format_gps_dest_distance_ref("K"),
            Some("Kilometers".to_string())
        );
    }

    #[test]
    fn test_format_gps_dest_distance_ref_miles() {
        // "M" should format to "Miles" to match ExifTool's output
        assert_eq!(format_gps_dest_distance_ref("M"), Some("Miles".to_string()));
    }

    #[test]
    fn test_format_gps_dest_distance_ref_nautical_miles() {
        // "N" should format to "Nautical Miles" to match ExifTool's output
        assert_eq!(
            format_gps_dest_distance_ref("N"),
            Some("Nautical Miles".to_string())
        );
    }

    #[test]
    fn test_format_gps_dest_distance_ref_with_whitespace() {
        // Function should handle values with leading/trailing whitespace
        assert_eq!(
            format_gps_dest_distance_ref(" K"),
            Some("Kilometers".to_string())
        );
        assert_eq!(
            format_gps_dest_distance_ref("K "),
            Some("Kilometers".to_string())
        );
        assert_eq!(
            format_gps_dest_distance_ref(" M "),
            Some("Miles".to_string())
        );
        assert_eq!(
            format_gps_dest_distance_ref("\tN\n"),
            Some("Nautical Miles".to_string())
        );
    }

    #[test]
    fn test_format_gps_dest_distance_ref_invalid_codes() {
        // Invalid or unrecognized codes should return None
        assert_eq!(format_gps_dest_distance_ref("X"), None);
        assert_eq!(format_gps_dest_distance_ref(""), None);
        assert_eq!(format_gps_dest_distance_ref("Kilometers"), None); // Already formatted
        assert_eq!(format_gps_dest_distance_ref("Miles"), None);
    }

    #[test]
    fn test_format_gps_dest_distance_ref_case_sensitive() {
        // EXIF spec uses uppercase letters; lowercase should not match
        assert_eq!(format_gps_dest_distance_ref("k"), None);
        assert_eq!(format_gps_dest_distance_ref("m"), None);
        assert_eq!(format_gps_dest_distance_ref("n"), None);
    }

    // ------------------------------------------------------------------------
    // Tests for format_gps_speed_or_distance_ref
    // ------------------------------------------------------------------------

    #[test]
    fn test_format_gps_speed_or_distance_ref_speed_tags() {
        // GPSSpeedRef should use speed formatting
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSSpeedRef", "K"),
            Some("km/h".to_string())
        );
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSSpeedRef", "M"),
            Some("mph".to_string())
        );
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSSpeedRef", "N"),
            Some("knots".to_string())
        );
    }

    #[test]
    fn test_format_gps_speed_or_distance_ref_distance_tags() {
        // GPSDestDistanceRef should use distance formatting
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSDestDistanceRef", "K"),
            Some("Kilometers".to_string())
        );
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSDestDistanceRef", "M"),
            Some("Miles".to_string())
        );
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSDestDistanceRef", "N"),
            Some("Nautical Miles".to_string())
        );
    }

    #[test]
    fn test_format_gps_speed_or_distance_ref_with_group_prefix() {
        // Should handle fully-qualified tag names with group prefix
        assert_eq!(
            format_gps_speed_or_distance_ref("GPS:GPSSpeedRef", "K"),
            Some("km/h".to_string())
        );
        assert_eq!(
            format_gps_speed_or_distance_ref("GPS:GPSDestDistanceRef", "M"),
            Some("Miles".to_string())
        );
        assert_eq!(
            format_gps_speed_or_distance_ref("EXIF:GPSSpeedRef", "N"),
            Some("knots".to_string())
        );
    }

    #[test]
    fn test_format_gps_speed_or_distance_ref_unrecognized_tags() {
        // Other GPS reference tags should return None
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSLatitudeRef", "N"),
            None
        );
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSLongitudeRef", "E"),
            None
        );
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSAltitudeRef", "0"),
            None
        );
        assert_eq!(format_gps_speed_or_distance_ref("SomeOtherTag", "K"), None);
    }

    #[test]
    fn test_format_gps_speed_or_distance_ref_invalid_values() {
        // Valid tags with invalid values should return None
        assert_eq!(format_gps_speed_or_distance_ref("GPSSpeedRef", "X"), None);
        assert_eq!(
            format_gps_speed_or_distance_ref("GPSDestDistanceRef", ""),
            None
        );
    }

    // ------------------------------------------------------------------------
    // Tests for is_gps_speed_or_distance_ref_tag
    // ------------------------------------------------------------------------

    #[test]
    fn test_is_gps_speed_or_distance_ref_tag_recognized() {
        // GPSSpeedRef and GPSDestDistanceRef should return true
        assert!(is_gps_speed_or_distance_ref_tag("GPSSpeedRef"));
        assert!(is_gps_speed_or_distance_ref_tag("GPSDestDistanceRef"));
    }

    #[test]
    fn test_is_gps_speed_or_distance_ref_tag_with_prefix() {
        // Should handle group prefixes
        assert!(is_gps_speed_or_distance_ref_tag("GPS:GPSSpeedRef"));
        assert!(is_gps_speed_or_distance_ref_tag("GPS:GPSDestDistanceRef"));
        assert!(is_gps_speed_or_distance_ref_tag("EXIF:GPSSpeedRef"));
    }

    #[test]
    fn test_is_gps_speed_or_distance_ref_tag_other_gps_tags() {
        // Other GPS reference tags should return false
        assert!(!is_gps_speed_or_distance_ref_tag("GPSLatitudeRef"));
        assert!(!is_gps_speed_or_distance_ref_tag("GPSLongitudeRef"));
        assert!(!is_gps_speed_or_distance_ref_tag("GPSAltitudeRef"));
        assert!(!is_gps_speed_or_distance_ref_tag("GPSImgDirectionRef"));
        assert!(!is_gps_speed_or_distance_ref_tag("GPSDestBearingRef"));
        assert!(!is_gps_speed_or_distance_ref_tag("GPSTrackRef"));
    }

    #[test]
    fn test_is_gps_speed_or_distance_ref_tag_non_gps_tags() {
        // Non-GPS tags should return false
        assert!(!is_gps_speed_or_distance_ref_tag("Make"));
        assert!(!is_gps_speed_or_distance_ref_tag("Model"));
        assert!(!is_gps_speed_or_distance_ref_tag("FocalLength"));
        assert!(!is_gps_speed_or_distance_ref_tag(""));
    }

    // ------------------------------------------------------------------------
    // Tests for module constants
    // ------------------------------------------------------------------------

    #[test]
    fn test_speed_ref_codes_constants() {
        // Verify the speed reference code constants match EXIF spec
        assert_eq!(speed_ref_codes::KILOMETERS_PER_HOUR, "K");
        assert_eq!(speed_ref_codes::MILES_PER_HOUR, "M");
        assert_eq!(speed_ref_codes::KNOTS, "N");
    }

    #[test]
    fn test_distance_ref_codes_constants() {
        // Verify the distance reference code constants match EXIF spec
        assert_eq!(distance_ref_codes::KILOMETERS, "K");
        assert_eq!(distance_ref_codes::MILES, "M");
        assert_eq!(distance_ref_codes::NAUTICAL_MILES, "N");
    }
}
