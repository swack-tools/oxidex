//! GPS Altitude Reference Formatter
//!
//! This module provides formatting functions for GPS altitude reference values
//! to match ExifTool's output format. The GPS altitude reference indicates
//! whether the altitude is above or below sea level.
//!
//! ## EXIF Standard
//!
//! According to the EXIF specification (JEITA CP-3451C):
//! - GPSAltitudeRef (tag 0x0005) is stored as a single BYTE value:
//!   - 0 = Above sea level
//!   - 1 = Below sea level
//!
//! ## Input Handling
//!
//! This formatter handles multiple input representations:
//! - Raw byte values: `0x00`, `0x01`
//! - Byte arrays: `[0]`, `[1]`
//! - String representations: `"0"`, `"1"`
//! - Null byte strings: `"\x00"`, `"\x01"`
//!
//! ## ExifTool Compatibility
//!
//! ExifTool outputs these values as human-readable strings:
//! - `0` / `0x00` / `[0]` -> "Above Sea Level"
//! - `1` / `0x01` / `[1]` -> "Below Sea Level"

/// The formatted string for altitude above sea level (reference value 0).
pub const ABOVE_SEA_LEVEL: &str = "Above Sea Level";

/// The formatted string for altitude below sea level (reference value 1).
pub const BELOW_SEA_LEVEL: &str = "Below Sea Level";

/// Formats a GPS altitude reference value to a human-readable description.
///
/// This function converts the raw GPS altitude reference value (as stored in EXIF)
/// to the human-readable format that ExifTool outputs. It handles multiple input
/// formats to accommodate different parsing scenarios.
///
/// # Arguments
///
/// * `value` - The raw altitude reference value, which can be:
///   - A string containing "0" or "1"
///   - A string containing a null byte (`"\x00"` or `"\x01"`)
///   - Any other string representation of the reference value
///
/// # Returns
///
/// - `Some("Above Sea Level")` for reference value 0
/// - `Some("Below Sea Level")` for reference value 1
/// - `None` for unrecognized or invalid values
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_altitude_ref::format_gps_altitude_ref;
///
/// // String numeric values
/// assert_eq!(format_gps_altitude_ref("0"), Some("Above Sea Level".to_string()));
/// assert_eq!(format_gps_altitude_ref("1"), Some("Below Sea Level".to_string()));
///
/// // Null byte representations
/// assert_eq!(format_gps_altitude_ref("\x00"), Some("Above Sea Level".to_string()));
/// assert_eq!(format_gps_altitude_ref("\x01"), Some("Below Sea Level".to_string()));
///
/// // Invalid input
/// assert_eq!(format_gps_altitude_ref("invalid"), None);
/// assert_eq!(format_gps_altitude_ref("2"), None);
/// ```
pub fn format_gps_altitude_ref(value: &str) -> Option<String> {
    // Trim whitespace to handle cases like " 0 " or "0 "
    let trimmed = value.trim();

    match trimmed {
        // String numeric representation: "0" or "1"
        "0" => Some(ABOVE_SEA_LEVEL.to_string()),
        "1" => Some(BELOW_SEA_LEVEL.to_string()),

        // Null byte representation: "\x00" or "\x01"
        // These may appear when binary data is converted to string
        "\x00" => Some(ABOVE_SEA_LEVEL.to_string()),
        "\x01" => Some(BELOW_SEA_LEVEL.to_string()),

        // Unrecognized value - return None to allow fallback handling
        _ => None,
    }
}

/// Formats a GPS altitude reference from a byte value.
///
/// This is a convenience function for when the reference value is available
/// as a raw byte rather than a string. This is the most direct representation
/// of how the value is stored in EXIF data.
///
/// # Arguments
///
/// * `value` - The raw byte value (0x00 or 0x01)
///
/// # Returns
///
/// - `Some("Above Sea Level")` for byte value 0
/// - `Some("Below Sea Level")` for byte value 1
/// - `None` for any other byte value
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_altitude_ref::format_gps_altitude_ref_byte;
///
/// assert_eq!(format_gps_altitude_ref_byte(0x00), Some("Above Sea Level".to_string()));
/// assert_eq!(format_gps_altitude_ref_byte(0x01), Some("Below Sea Level".to_string()));
/// assert_eq!(format_gps_altitude_ref_byte(0x02), None);
/// assert_eq!(format_gps_altitude_ref_byte(0xFF), None);
/// ```
pub fn format_gps_altitude_ref_byte(value: u8) -> Option<String> {
    match value {
        0x00 => Some(ABOVE_SEA_LEVEL.to_string()),
        0x01 => Some(BELOW_SEA_LEVEL.to_string()),
        _ => None,
    }
}

/// Formats a GPS altitude reference from a byte slice (array).
///
/// This function handles the case where GPS altitude reference is provided
/// as a byte array, which can occur when reading raw EXIF data. The function
/// expects a single-element array containing the reference byte.
///
/// # Arguments
///
/// * `bytes` - A byte slice, expected to contain a single element
///
/// # Returns
///
/// - `Some("Above Sea Level")` for `[0]` or `[0x00]`
/// - `Some("Below Sea Level")` for `[1]` or `[0x01]`
/// - `None` for empty arrays, multi-element arrays, or invalid values
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_altitude_ref::format_gps_altitude_ref_bytes;
///
/// // Single-element arrays
/// assert_eq!(format_gps_altitude_ref_bytes(&[0]), Some("Above Sea Level".to_string()));
/// assert_eq!(format_gps_altitude_ref_bytes(&[1]), Some("Below Sea Level".to_string()));
///
/// // Invalid inputs
/// assert_eq!(format_gps_altitude_ref_bytes(&[]), None);
/// assert_eq!(format_gps_altitude_ref_bytes(&[0, 1]), None);
/// assert_eq!(format_gps_altitude_ref_bytes(&[2]), None);
/// ```
pub fn format_gps_altitude_ref_bytes(bytes: &[u8]) -> Option<String> {
    // GPS altitude reference should be exactly one byte per EXIF specification.
    // We only process single-byte arrays to avoid misinterpreting data.
    if bytes.len() != 1 {
        return None;
    }

    format_gps_altitude_ref_byte(bytes[0])
}

/// Checks if a value represents "above sea level" reference.
///
/// This is a convenience function for conditional logic based on the
/// altitude reference without needing to format the full string.
///
/// # Arguments
///
/// * `value` - The raw reference value as a string
///
/// # Returns
///
/// `true` if the value represents above sea level, `false` otherwise.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_altitude_ref::is_above_sea_level;
///
/// assert!(is_above_sea_level("0"));
/// assert!(is_above_sea_level("\x00"));
/// assert!(!is_above_sea_level("1"));
/// assert!(!is_above_sea_level("invalid"));
/// ```
pub fn is_above_sea_level(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed == "0" || trimmed == "\x00"
}

/// Checks if a value represents "below sea level" reference.
///
/// This is a convenience function for conditional logic based on the
/// altitude reference without needing to format the full string.
///
/// # Arguments
///
/// * `value` - The raw reference value as a string
///
/// # Returns
///
/// `true` if the value represents below sea level, `false` otherwise.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_altitude_ref::is_below_sea_level;
///
/// assert!(is_below_sea_level("1"));
/// assert!(is_below_sea_level("\x01"));
/// assert!(!is_below_sea_level("0"));
/// assert!(!is_below_sea_level("invalid"));
/// ```
pub fn is_below_sea_level(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed == "1" || trimmed == "\x01"
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Tests for format_gps_altitude_ref (string input)
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_altitude_ref_string_zero() {
        // String "0" should map to "Above Sea Level"
        assert_eq!(
            format_gps_altitude_ref("0"),
            Some(ABOVE_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_string_one() {
        // String "1" should map to "Below Sea Level"
        assert_eq!(
            format_gps_altitude_ref("1"),
            Some(BELOW_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_null_byte_zero() {
        // Null byte "\x00" should map to "Above Sea Level"
        assert_eq!(
            format_gps_altitude_ref("\x00"),
            Some(ABOVE_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_null_byte_one() {
        // Null byte "\x01" should map to "Below Sea Level"
        assert_eq!(
            format_gps_altitude_ref("\x01"),
            Some(BELOW_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_with_whitespace() {
        // Values with leading/trailing whitespace should be trimmed
        assert_eq!(
            format_gps_altitude_ref(" 0 "),
            Some(ABOVE_SEA_LEVEL.to_string())
        );
        assert_eq!(
            format_gps_altitude_ref(" 1 "),
            Some(BELOW_SEA_LEVEL.to_string())
        );
        assert_eq!(
            format_gps_altitude_ref("\t0\n"),
            Some(ABOVE_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_invalid_values() {
        // Invalid string values should return None
        assert_eq!(format_gps_altitude_ref("2"), None);
        assert_eq!(format_gps_altitude_ref("-1"), None);
        assert_eq!(format_gps_altitude_ref("above"), None);
        assert_eq!(format_gps_altitude_ref("below"), None);
        assert_eq!(format_gps_altitude_ref("Above Sea Level"), None);
        assert_eq!(format_gps_altitude_ref("invalid"), None);
        assert_eq!(format_gps_altitude_ref(""), None);
    }

    #[test]
    fn test_format_gps_altitude_ref_numeric_edge_cases() {
        // Multi-digit numbers should be invalid
        assert_eq!(format_gps_altitude_ref("00"), None);
        assert_eq!(format_gps_altitude_ref("01"), None);
        assert_eq!(format_gps_altitude_ref("10"), None);
        assert_eq!(format_gps_altitude_ref("255"), None);
    }

    // -------------------------------------------------------------------------
    // Tests for format_gps_altitude_ref_byte (single byte input)
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_altitude_ref_byte_zero() {
        // Byte 0x00 should map to "Above Sea Level"
        assert_eq!(
            format_gps_altitude_ref_byte(0x00),
            Some(ABOVE_SEA_LEVEL.to_string())
        );
        // Alternative representation
        assert_eq!(
            format_gps_altitude_ref_byte(0),
            Some(ABOVE_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_byte_one() {
        // Byte 0x01 should map to "Below Sea Level"
        assert_eq!(
            format_gps_altitude_ref_byte(0x01),
            Some(BELOW_SEA_LEVEL.to_string())
        );
        // Alternative representation
        assert_eq!(
            format_gps_altitude_ref_byte(1),
            Some(BELOW_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_byte_invalid() {
        // Any byte value other than 0 or 1 should return None
        assert_eq!(format_gps_altitude_ref_byte(2), None);
        assert_eq!(format_gps_altitude_ref_byte(0x02), None);
        assert_eq!(format_gps_altitude_ref_byte(0xFF), None);
        assert_eq!(format_gps_altitude_ref_byte(128), None);
        assert_eq!(format_gps_altitude_ref_byte(255), None);
    }

    // -------------------------------------------------------------------------
    // Tests for format_gps_altitude_ref_bytes (byte slice input)
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_altitude_ref_bytes_single_zero() {
        // Single-element array [0] should map to "Above Sea Level"
        assert_eq!(
            format_gps_altitude_ref_bytes(&[0]),
            Some(ABOVE_SEA_LEVEL.to_string())
        );
        assert_eq!(
            format_gps_altitude_ref_bytes(&[0x00]),
            Some(ABOVE_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_bytes_single_one() {
        // Single-element array [1] should map to "Below Sea Level"
        assert_eq!(
            format_gps_altitude_ref_bytes(&[1]),
            Some(BELOW_SEA_LEVEL.to_string())
        );
        assert_eq!(
            format_gps_altitude_ref_bytes(&[0x01]),
            Some(BELOW_SEA_LEVEL.to_string())
        );
    }

    #[test]
    fn test_format_gps_altitude_ref_bytes_empty() {
        // Empty array should return None
        assert_eq!(format_gps_altitude_ref_bytes(&[]), None);
    }

    #[test]
    fn test_format_gps_altitude_ref_bytes_multiple_elements() {
        // Multi-element arrays should return None (invalid per EXIF spec)
        assert_eq!(format_gps_altitude_ref_bytes(&[0, 0]), None);
        assert_eq!(format_gps_altitude_ref_bytes(&[0, 1]), None);
        assert_eq!(format_gps_altitude_ref_bytes(&[1, 0]), None);
        assert_eq!(format_gps_altitude_ref_bytes(&[0, 0, 0]), None);
    }

    #[test]
    fn test_format_gps_altitude_ref_bytes_invalid_value() {
        // Single-element array with invalid value should return None
        assert_eq!(format_gps_altitude_ref_bytes(&[2]), None);
        assert_eq!(format_gps_altitude_ref_bytes(&[0xFF]), None);
    }

    // -------------------------------------------------------------------------
    // Tests for is_above_sea_level
    // -------------------------------------------------------------------------

    #[test]
    fn test_is_above_sea_level_true() {
        assert!(is_above_sea_level("0"));
        assert!(is_above_sea_level("\x00"));
        assert!(is_above_sea_level(" 0 ")); // With whitespace
    }

    #[test]
    fn test_is_above_sea_level_false() {
        assert!(!is_above_sea_level("1"));
        assert!(!is_above_sea_level("\x01"));
        assert!(!is_above_sea_level("2"));
        assert!(!is_above_sea_level("invalid"));
        assert!(!is_above_sea_level(""));
    }

    // -------------------------------------------------------------------------
    // Tests for is_below_sea_level
    // -------------------------------------------------------------------------

    #[test]
    fn test_is_below_sea_level_true() {
        assert!(is_below_sea_level("1"));
        assert!(is_below_sea_level("\x01"));
        assert!(is_below_sea_level(" 1 ")); // With whitespace
    }

    #[test]
    fn test_is_below_sea_level_false() {
        assert!(!is_below_sea_level("0"));
        assert!(!is_below_sea_level("\x00"));
        assert!(!is_below_sea_level("2"));
        assert!(!is_below_sea_level("invalid"));
        assert!(!is_below_sea_level(""));
    }

    // -------------------------------------------------------------------------
    // Tests for constants
    // -------------------------------------------------------------------------

    #[test]
    fn test_constants_match_exiftool_output() {
        // Verify constants match ExifTool's exact output strings
        assert_eq!(ABOVE_SEA_LEVEL, "Above Sea Level");
        assert_eq!(BELOW_SEA_LEVEL, "Below Sea Level");
    }

    // -------------------------------------------------------------------------
    // Integration-style tests combining multiple functions
    // -------------------------------------------------------------------------

    #[test]
    fn test_consistency_across_functions() {
        // All three input methods should produce the same result for above sea level
        let expected_above = Some(ABOVE_SEA_LEVEL.to_string());
        assert_eq!(format_gps_altitude_ref("0"), expected_above);
        assert_eq!(format_gps_altitude_ref_byte(0), expected_above);
        assert_eq!(format_gps_altitude_ref_bytes(&[0]), expected_above);

        // All three input methods should produce the same result for below sea level
        let expected_below = Some(BELOW_SEA_LEVEL.to_string());
        assert_eq!(format_gps_altitude_ref("1"), expected_below);
        assert_eq!(format_gps_altitude_ref_byte(1), expected_below);
        assert_eq!(format_gps_altitude_ref_bytes(&[1]), expected_below);
    }

    #[test]
    fn test_helper_functions_consistency_with_formatter() {
        // is_above_sea_level should return true exactly when formatter returns ABOVE_SEA_LEVEL
        for value in ["0", "\x00", " 0 "] {
            let is_above = is_above_sea_level(value);
            let formatted = format_gps_altitude_ref(value);
            if is_above {
                assert_eq!(formatted, Some(ABOVE_SEA_LEVEL.to_string()));
            }
        }

        // is_below_sea_level should return true exactly when formatter returns BELOW_SEA_LEVEL
        for value in ["1", "\x01", " 1 "] {
            let is_below = is_below_sea_level(value);
            let formatted = format_gps_altitude_ref(value);
            if is_below {
                assert_eq!(formatted, Some(BELOW_SEA_LEVEL.to_string()));
            }
        }
    }
}
