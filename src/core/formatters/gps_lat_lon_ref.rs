//! GPS Latitude/Longitude reference value formatter
//!
//! This module provides formatting functions to convert single-character GPS
//! reference values (or their ASCII byte equivalents) to human-readable strings
//! for ExifTool compatibility.
//!
//! # Background
//!
//! EXIF GPS metadata stores directional references as single ASCII characters:
//! - Latitude reference: "N" (North) or "S" (South)
//! - Longitude reference: "E" (East) or "W" (West)
//!
//! ExifTool displays these as full words ("North", "South", "East", "West"),
//! so this module provides the conversion to match that output format.
//!
//! # Supported Input Formats
//!
//! Both functions accept:
//! - Single character strings: "N", "S", "E", "W"
//! - ASCII byte values as strings: "78" (0x4E = 'N'), "83" (0x53 = 'S'), etc.
//!
//! Unknown or invalid values are returned unchanged.
//!
//! # Examples
//!
//! ```
//! use oxidex::core::formatters::gps_lat_lon_ref::{format_gps_lat_ref, format_gps_lon_ref};
//!
//! // Character inputs
//! assert_eq!(format_gps_lat_ref("N"), "North");
//! assert_eq!(format_gps_lat_ref("S"), "South");
//! assert_eq!(format_gps_lon_ref("E"), "East");
//! assert_eq!(format_gps_lon_ref("W"), "West");
//!
//! // Unknown values pass through unchanged
//! assert_eq!(format_gps_lat_ref("X"), "X");
//! ```

// -----------------------------------------------------------------------------
// ASCII byte values for GPS reference characters
// These are the decimal representations of the ASCII codes that may appear
// in raw EXIF data when bytes are interpreted as numeric strings.
// -----------------------------------------------------------------------------

/// ASCII code for 'N' (North) - 0x4E in hexadecimal
const ASCII_N: u8 = 0x4E; // 78 decimal

/// ASCII code for 'S' (South) - 0x53 in hexadecimal
const ASCII_S: u8 = 0x53; // 83 decimal

/// ASCII code for 'E' (East) - 0x45 in hexadecimal
const ASCII_E: u8 = 0x45; // 69 decimal

/// ASCII code for 'W' (West) - 0x57 in hexadecimal
const ASCII_W: u8 = 0x57; // 87 decimal

// -----------------------------------------------------------------------------
// Public API
// -----------------------------------------------------------------------------

/// Formats a GPS latitude reference value to a human-readable direction.
///
/// Converts single-character latitude references or their ASCII byte values
/// to the corresponding cardinal direction name for ExifTool compatibility.
///
/// # Arguments
///
/// * `value` - The raw latitude reference value. Can be:
///   - A single character: "N" or "S"
///   - An ASCII byte value as a string: "78" (N) or "83" (S)
///
/// # Returns
///
/// - `"North"` for "N" or 0x4E (78)
/// - `"South"` for "S" or 0x53 (83)
/// - The original value unchanged for any other input
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_lat_lon_ref::format_gps_lat_ref;
///
/// // Standard character inputs
/// assert_eq!(format_gps_lat_ref("N"), "North");
/// assert_eq!(format_gps_lat_ref("S"), "South");
///
/// // Handles whitespace trimming
/// assert_eq!(format_gps_lat_ref(" N "), "North");
///
/// // Unknown values are returned unchanged
/// assert_eq!(format_gps_lat_ref("E"), "E");
/// assert_eq!(format_gps_lat_ref("unknown"), "unknown");
/// ```
pub fn format_gps_lat_ref(value: &str) -> String {
    let trimmed = value.trim();

    // First, check for direct character match (most common case)
    match trimmed {
        "N" => return "North".to_string(),
        "S" => return "South".to_string(),
        _ => {}
    }

    // Check if the value is a single byte that matches our expected ASCII codes
    // This handles cases where raw bytes are passed as single-character strings
    if trimmed.len() == 1 {
        let byte = trimmed.as_bytes()[0];
        match byte {
            ASCII_N => return "North".to_string(),
            ASCII_S => return "South".to_string(),
            _ => {}
        }
    }

    // Check if the value is a numeric string representing an ASCII code
    // (e.g., "78" for 'N' or "83" for 'S')
    if let Ok(byte_val) = trimmed.parse::<u8>() {
        match byte_val {
            ASCII_N => return "North".to_string(),
            ASCII_S => return "South".to_string(),
            _ => {}
        }
    }

    // Unknown value - return the original unchanged
    value.to_string()
}

/// Formats a GPS longitude reference value to a human-readable direction.
///
/// Converts single-character longitude references or their ASCII byte values
/// to the corresponding cardinal direction name for ExifTool compatibility.
///
/// # Arguments
///
/// * `value` - The raw longitude reference value. Can be:
///   - A single character: "E" or "W"
///   - An ASCII byte value as a string: "69" (E) or "87" (W)
///
/// # Returns
///
/// - `"East"` for "E" or 0x45 (69)
/// - `"West"` for "W" or 0x57 (87)
/// - The original value unchanged for any other input
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_lat_lon_ref::format_gps_lon_ref;
///
/// // Standard character inputs
/// assert_eq!(format_gps_lon_ref("E"), "East");
/// assert_eq!(format_gps_lon_ref("W"), "West");
///
/// // Handles whitespace trimming
/// assert_eq!(format_gps_lon_ref(" W "), "West");
///
/// // Unknown values are returned unchanged
/// assert_eq!(format_gps_lon_ref("N"), "N");
/// assert_eq!(format_gps_lon_ref("unknown"), "unknown");
/// ```
pub fn format_gps_lon_ref(value: &str) -> String {
    let trimmed = value.trim();

    // First, check for direct character match (most common case)
    match trimmed {
        "E" => return "East".to_string(),
        "W" => return "West".to_string(),
        _ => {}
    }

    // Check if the value is a single byte that matches our expected ASCII codes
    // This handles cases where raw bytes are passed as single-character strings
    if trimmed.len() == 1 {
        let byte = trimmed.as_bytes()[0];
        match byte {
            ASCII_E => return "East".to_string(),
            ASCII_W => return "West".to_string(),
            _ => {}
        }
    }

    // Check if the value is a numeric string representing an ASCII code
    // (e.g., "69" for 'E' or "87" for 'W')
    if let Ok(byte_val) = trimmed.parse::<u8>() {
        match byte_val {
            ASCII_E => return "East".to_string(),
            ASCII_W => return "West".to_string(),
            _ => {}
        }
    }

    // Unknown value - return the original unchanged
    value.to_string()
}

// -----------------------------------------------------------------------------
// Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Latitude Reference Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_lat_ref_north_character() {
        // Standard "N" character should convert to "North"
        assert_eq!(format_gps_lat_ref("N"), "North");
    }

    #[test]
    fn test_format_gps_lat_ref_south_character() {
        // Standard "S" character should convert to "South"
        assert_eq!(format_gps_lat_ref("S"), "South");
    }

    #[test]
    fn test_format_gps_lat_ref_north_ascii_byte() {
        // ASCII byte value 0x4E (78 decimal) should convert to "North"
        assert_eq!(format_gps_lat_ref("78"), "North");
    }

    #[test]
    fn test_format_gps_lat_ref_south_ascii_byte() {
        // ASCII byte value 0x53 (83 decimal) should convert to "South"
        assert_eq!(format_gps_lat_ref("83"), "South");
    }

    #[test]
    fn test_format_gps_lat_ref_with_whitespace() {
        // Should handle leading/trailing whitespace
        assert_eq!(format_gps_lat_ref(" N "), "North");
        assert_eq!(format_gps_lat_ref("\tS\n"), "South");
        assert_eq!(format_gps_lat_ref("  78  "), "North");
    }

    #[test]
    fn test_format_gps_lat_ref_unknown_value() {
        // Unknown values should be returned unchanged
        assert_eq!(format_gps_lat_ref("E"), "E");
        assert_eq!(format_gps_lat_ref("W"), "W");
        assert_eq!(format_gps_lat_ref("X"), "X");
        assert_eq!(format_gps_lat_ref("unknown"), "unknown");
        assert_eq!(format_gps_lat_ref("North"), "North"); // Already expanded
        assert_eq!(format_gps_lat_ref(""), "");
    }

    #[test]
    fn test_format_gps_lat_ref_invalid_numeric() {
        // Invalid or out-of-range numeric values should pass through
        assert_eq!(format_gps_lat_ref("0"), "0");
        assert_eq!(format_gps_lat_ref("255"), "255");
        assert_eq!(format_gps_lat_ref("999"), "999"); // Too large for u8
        assert_eq!(format_gps_lat_ref("-1"), "-1");
    }

    // -------------------------------------------------------------------------
    // Longitude Reference Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_lon_ref_east_character() {
        // Standard "E" character should convert to "East"
        assert_eq!(format_gps_lon_ref("E"), "East");
    }

    #[test]
    fn test_format_gps_lon_ref_west_character() {
        // Standard "W" character should convert to "West"
        assert_eq!(format_gps_lon_ref("W"), "West");
    }

    #[test]
    fn test_format_gps_lon_ref_east_ascii_byte() {
        // ASCII byte value 0x45 (69 decimal) should convert to "East"
        assert_eq!(format_gps_lon_ref("69"), "East");
    }

    #[test]
    fn test_format_gps_lon_ref_west_ascii_byte() {
        // ASCII byte value 0x57 (87 decimal) should convert to "West"
        assert_eq!(format_gps_lon_ref("87"), "West");
    }

    #[test]
    fn test_format_gps_lon_ref_with_whitespace() {
        // Should handle leading/trailing whitespace
        assert_eq!(format_gps_lon_ref(" E "), "East");
        assert_eq!(format_gps_lon_ref("\tW\n"), "West");
        assert_eq!(format_gps_lon_ref("  69  "), "East");
    }

    #[test]
    fn test_format_gps_lon_ref_unknown_value() {
        // Unknown values should be returned unchanged
        assert_eq!(format_gps_lon_ref("N"), "N");
        assert_eq!(format_gps_lon_ref("S"), "S");
        assert_eq!(format_gps_lon_ref("X"), "X");
        assert_eq!(format_gps_lon_ref("unknown"), "unknown");
        assert_eq!(format_gps_lon_ref("East"), "East"); // Already expanded
        assert_eq!(format_gps_lon_ref(""), "");
    }

    #[test]
    fn test_format_gps_lon_ref_invalid_numeric() {
        // Invalid or out-of-range numeric values should pass through
        assert_eq!(format_gps_lon_ref("0"), "0");
        assert_eq!(format_gps_lon_ref("255"), "255");
        assert_eq!(format_gps_lon_ref("999"), "999"); // Too large for u8
        assert_eq!(format_gps_lon_ref("-1"), "-1");
    }

    // -------------------------------------------------------------------------
    // Edge Cases and Boundary Conditions
    // -------------------------------------------------------------------------

    #[test]
    fn test_case_sensitivity() {
        // The functions should be case-sensitive (matching ExifTool behavior)
        // Lowercase letters should NOT be converted
        assert_eq!(format_gps_lat_ref("n"), "n");
        assert_eq!(format_gps_lat_ref("s"), "s");
        assert_eq!(format_gps_lon_ref("e"), "e");
        assert_eq!(format_gps_lon_ref("w"), "w");
    }

    #[test]
    fn test_preserves_original_whitespace_on_unknown() {
        // Unknown values should preserve original input including whitespace
        assert_eq!(format_gps_lat_ref(" unknown "), " unknown ");
        assert_eq!(format_gps_lon_ref(" unknown "), " unknown ");
    }

    #[test]
    fn test_ascii_byte_values_are_correct() {
        // Verify our constants match actual ASCII values
        assert_eq!(ASCII_N, b'N');
        assert_eq!(ASCII_S, b'S');
        assert_eq!(ASCII_E, b'E');
        assert_eq!(ASCII_W, b'W');

        // Verify decimal string parsing works correctly
        assert_eq!(format!("{}", b'N'), "78");
        assert_eq!(format!("{}", b'S'), "83");
        assert_eq!(format!("{}", b'E'), "69");
        assert_eq!(format!("{}", b'W'), "87");
    }

    #[test]
    fn test_raw_byte_single_char_handling() {
        // Test that single raw bytes work correctly
        // This simulates cases where a byte is passed as a character
        let n_byte = String::from_utf8(vec![0x4E]).unwrap();
        let s_byte = String::from_utf8(vec![0x53]).unwrap();
        let e_byte = String::from_utf8(vec![0x45]).unwrap();
        let w_byte = String::from_utf8(vec![0x57]).unwrap();

        assert_eq!(format_gps_lat_ref(&n_byte), "North");
        assert_eq!(format_gps_lat_ref(&s_byte), "South");
        assert_eq!(format_gps_lon_ref(&e_byte), "East");
        assert_eq!(format_gps_lon_ref(&w_byte), "West");
    }
}
