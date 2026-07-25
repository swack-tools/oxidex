//! GPS direction reference formatter (ImgDirection, Track, DestBearing)
//!
//! This module provides formatting functions for GPS direction reference tags
//! (GPSImgDirectionRef, GPSTrackRef, GPSDestBearingRef) to convert raw values
//! to human-readable descriptions that match ExifTool's output format.
//!
//! # Background
//!
//! GPS direction reference tags store single-character codes indicating whether
//! a direction measurement is relative to true north or magnetic north:
//! - "T" = True North (geographic north pole)
//! - "M" = Magnetic North (direction a compass points)
//!
//! ExifTool displays these as "True North" and "Magnetic North" respectively,
//! so this formatter converts the raw values to match that format.
//!
//! # Applicable Tags
//!
//! This formatter applies to the following EXIF GPS tags:
//! - `GPSImgDirectionRef` - Reference for image direction
//! - `GPSTrackRef` - Reference for GPS track direction
//! - `GPSDestBearingRef` - Reference for destination bearing

/// Formats a GPS direction reference value to a human-readable description.
///
/// Converts the raw single-character GPS direction reference codes to their
/// full human-readable equivalents, matching ExifTool's output format:
/// - "T" -> "True North"
/// - "M" -> "Magnetic North"
/// - Any other value -> returned unchanged (preserves unknown values)
///
/// # Arguments
///
/// * `value` - The raw GPS direction reference value (typically "T" or "M")
///
/// # Returns
///
/// A `String` containing either the human-readable description for known values,
/// or the original value for unknown inputs.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_direction_ref::format_gps_direction_ref;
///
/// // Known direction references
/// assert_eq!(format_gps_direction_ref("T"), "True North");
/// assert_eq!(format_gps_direction_ref("M"), "Magnetic North");
///
/// // Unknown values are preserved as-is
/// assert_eq!(format_gps_direction_ref("X"), "X");
/// assert_eq!(format_gps_direction_ref("Unknown"), "Unknown");
/// assert_eq!(format_gps_direction_ref(""), "");
/// ```
///
/// # Notes
///
/// - The matching is case-sensitive ("T" matches, but "t" does not)
/// - Leading/trailing whitespace is NOT trimmed (caller should trim if needed)
/// - Empty strings are returned unchanged
pub fn format_gps_direction_ref(value: &str) -> String {
    match value {
        "T" => "True North".to_string(),
        "M" => "Magnetic North".to_string(),
        // Unknown values: return the original to preserve data integrity
        // This ensures we don't lose information for edge cases or future
        // extensions to the GPS direction reference specification
        _ => value.to_string(),
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test formatting of "T" (True North) reference
    #[test]
    fn test_format_true_north() {
        assert_eq!(format_gps_direction_ref("T"), "True North");
    }

    /// Test formatting of "M" (Magnetic North) reference
    #[test]
    fn test_format_magnetic_north() {
        assert_eq!(format_gps_direction_ref("M"), "Magnetic North");
    }

    /// Test that unknown single-character values are preserved unchanged
    #[test]
    fn test_format_unknown_single_char() {
        // Other letters that might appear in corrupted data
        assert_eq!(format_gps_direction_ref("X"), "X");
        assert_eq!(format_gps_direction_ref("N"), "N");
        assert_eq!(format_gps_direction_ref("S"), "S");
        assert_eq!(format_gps_direction_ref("E"), "E");
        assert_eq!(format_gps_direction_ref("W"), "W");
    }

    /// Test that unknown multi-character values are preserved unchanged
    #[test]
    fn test_format_unknown_multi_char() {
        // Arbitrary strings should pass through unchanged
        assert_eq!(format_gps_direction_ref("Unknown"), "Unknown");
        assert_eq!(format_gps_direction_ref("True"), "True");
        assert_eq!(format_gps_direction_ref("Magnetic"), "Magnetic");
        assert_eq!(format_gps_direction_ref("TN"), "TN");
        assert_eq!(format_gps_direction_ref("MN"), "MN");
    }

    /// Test that empty string is preserved unchanged
    #[test]
    fn test_format_empty_string() {
        assert_eq!(format_gps_direction_ref(""), "");
    }

    /// Test case sensitivity (lowercase should NOT match)
    #[test]
    fn test_case_sensitivity() {
        // Lowercase versions should NOT be converted
        // (EXIF spec uses uppercase only)
        assert_eq!(format_gps_direction_ref("t"), "t");
        assert_eq!(format_gps_direction_ref("m"), "m");
    }

    /// Test that whitespace is preserved (caller's responsibility to trim)
    #[test]
    fn test_whitespace_handling() {
        // Values with leading/trailing whitespace should NOT match
        // This is intentional - the caller should trim if needed
        assert_eq!(format_gps_direction_ref(" T"), " T");
        assert_eq!(format_gps_direction_ref("T "), "T ");
        assert_eq!(format_gps_direction_ref(" T "), " T ");
        assert_eq!(format_gps_direction_ref(" M"), " M");
        assert_eq!(format_gps_direction_ref("M "), "M ");
    }

    /// Test numeric values (should be preserved unchanged)
    #[test]
    fn test_numeric_values() {
        // Numeric strings should pass through unchanged
        assert_eq!(format_gps_direction_ref("0"), "0");
        assert_eq!(format_gps_direction_ref("1"), "1");
        assert_eq!(format_gps_direction_ref("123"), "123");
    }

    /// Test special characters (should be preserved unchanged)
    #[test]
    fn test_special_characters() {
        // Special characters should pass through unchanged
        assert_eq!(format_gps_direction_ref("-"), "-");
        assert_eq!(format_gps_direction_ref("/"), "/");
        assert_eq!(format_gps_direction_ref("\0"), "\0");
        assert_eq!(format_gps_direction_ref("\t"), "\t");
    }
}
