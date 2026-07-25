//! SceneType decoder for EXIF tag 0xA301
//!
//! This module provides decoding functions for the SceneType EXIF tag, which
//! indicates how the scene was captured. According to the EXIF specification,
//! only one value is currently defined:
//!
//! - 1: Directly photographed
//!
//! ExifTool outputs "Directly photographed" for value 1, while unknown values
//! are displayed as "Unknown ({value})".
//!
//! # Tag Information
//!
//! | Property    | Value                    |
//! |-------------|--------------------------|
//! | Tag ID      | 0xA301 (41729)           |
//! | Tag Name    | SceneType                |
//! | Data Type   | UNDEFINED (1 byte)       |
//! | IFD         | EXIF                     |
//!
//! # Examples
//!
//! ```
//! use oxidex::core::formatters::scene_type::{decode_scene_type, decode_scene_type_value};
//!
//! // Decode from raw bytes
//! assert_eq!(decode_scene_type(&[1]), "Directly photographed");
//! assert_eq!(decode_scene_type(&[2]), "Unknown (2)");
//!
//! // Decode from a single value
//! assert_eq!(decode_scene_type_value(1), "Directly photographed");
//! assert_eq!(decode_scene_type_value(0), "Unknown (0)");
//! ```

/// The only defined SceneType value according to EXIF specification.
///
/// Value 1 means the image was directly photographed (as opposed to being
/// scanned, computer-generated, or otherwise derived).
const SCENE_TYPE_DIRECTLY_PHOTOGRAPHED: u8 = 1;

/// Human-readable description for directly photographed scenes.
///
/// This matches ExifTool's output format exactly.
const SCENE_TYPE_DIRECTLY_PHOTOGRAPHED_DESC: &str = "Directly photographed";

/// Decode SceneType from raw binary data.
///
/// The SceneType tag (0xA301) is stored as a single byte in the EXIF data.
/// This function extracts the first byte from the provided data slice and
/// decodes it to a human-readable string.
///
/// # Arguments
///
/// * `data` - Raw binary data containing the scene type byte. Only the first
///   byte is used; any additional bytes are ignored.
///
/// # Returns
///
/// A `String` containing the human-readable scene type description:
/// - "Directly photographed" for value 1 (or 0x01)
/// - "Unknown ({value})" for any other value
/// - Empty string if the input data is empty
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::scene_type::decode_scene_type;
///
/// // Standard case: directly photographed (value 1)
/// assert_eq!(decode_scene_type(&[1]), "Directly photographed");
/// assert_eq!(decode_scene_type(&[0x01]), "Directly photographed");
///
/// // Unknown values
/// assert_eq!(decode_scene_type(&[0]), "Unknown (0)");
/// assert_eq!(decode_scene_type(&[2]), "Unknown (2)");
/// assert_eq!(decode_scene_type(&[255]), "Unknown (255)");
///
/// // Empty data returns empty string
/// assert_eq!(decode_scene_type(&[]), "");
///
/// // Extra bytes are ignored
/// assert_eq!(decode_scene_type(&[1, 0, 0, 0]), "Directly photographed");
/// ```
pub fn decode_scene_type(data: &[u8]) -> String {
    // Return empty string for empty input data.
    // This handles edge cases where the tag data might be missing or truncated.
    if data.is_empty() {
        return String::new();
    }

    // Decode the first byte as the scene type value.
    // Additional bytes (if any) are ignored as the EXIF spec defines this
    // as a single-byte value.
    decode_scene_type_value(data[0])
}

/// Decode a SceneType value from a single byte.
///
/// This is the core decoding function that maps a numeric scene type value
/// to its human-readable description. Use this when you already have the
/// byte value extracted from the EXIF data.
///
/// # Arguments
///
/// * `value` - The scene type value as a single byte (u8).
///
/// # Returns
///
/// A `String` containing the human-readable scene type description:
/// - "Directly photographed" for value 1
/// - "Unknown ({value})" for any other value
///
/// # EXIF Specification Notes
///
/// According to the EXIF 2.32 specification, only value 1 is defined:
/// - 1 = A directly photographed image
///
/// Other values are reserved for future use. This function returns
/// "Unknown ({value})" for any undefined values to maintain forward
/// compatibility while clearly indicating the value is not recognized.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::scene_type::decode_scene_type_value;
///
/// // The only defined value
/// assert_eq!(decode_scene_type_value(1), "Directly photographed");
///
/// // Unknown/undefined values
/// assert_eq!(decode_scene_type_value(0), "Unknown (0)");
/// assert_eq!(decode_scene_type_value(2), "Unknown (2)");
/// assert_eq!(decode_scene_type_value(255), "Unknown (255)");
/// ```
pub fn decode_scene_type_value(value: u8) -> String {
    match value {
        SCENE_TYPE_DIRECTLY_PHOTOGRAPHED => SCENE_TYPE_DIRECTLY_PHOTOGRAPHED_DESC.to_string(),
        other => format!("Unknown ({})", other),
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Tests for decode_scene_type (byte slice input)
    // -------------------------------------------------------------------------

    #[test]
    fn test_decode_scene_type_directly_photographed() {
        // Value 1 should decode to "Directly photographed"
        assert_eq!(decode_scene_type(&[1]), "Directly photographed");
        // Also test with hex notation for clarity
        assert_eq!(decode_scene_type(&[0x01]), "Directly photographed");
    }

    #[test]
    fn test_decode_scene_type_unknown_values() {
        // Value 0 is not defined in the EXIF spec
        assert_eq!(decode_scene_type(&[0]), "Unknown (0)");

        // Value 2 and above are reserved/unknown
        assert_eq!(decode_scene_type(&[2]), "Unknown (2)");
        assert_eq!(decode_scene_type(&[3]), "Unknown (3)");
        assert_eq!(decode_scene_type(&[100]), "Unknown (100)");

        // Maximum byte value
        assert_eq!(decode_scene_type(&[255]), "Unknown (255)");
    }

    #[test]
    fn test_decode_scene_type_empty_data() {
        // Empty input should return empty string
        assert_eq!(decode_scene_type(&[]), "");
    }

    #[test]
    fn test_decode_scene_type_extra_bytes_ignored() {
        // Only the first byte should be used; extra bytes are ignored.
        // This matches how EXIF parsers handle the single-byte SceneType tag.
        assert_eq!(decode_scene_type(&[1, 0]), "Directly photographed");
        assert_eq!(decode_scene_type(&[1, 0, 0, 0]), "Directly photographed");
        assert_eq!(
            decode_scene_type(&[1, 255, 255, 255]),
            "Directly photographed"
        );

        // Even if extra bytes have different values, first byte determines result
        assert_eq!(decode_scene_type(&[2, 1]), "Unknown (2)");
    }

    // -------------------------------------------------------------------------
    // Tests for decode_scene_type_value (single byte input)
    // -------------------------------------------------------------------------

    #[test]
    fn test_decode_scene_type_value_directly_photographed() {
        // The only defined value in the EXIF specification
        assert_eq!(decode_scene_type_value(1), "Directly photographed");
    }

    #[test]
    fn test_decode_scene_type_value_unknown_values() {
        // Value 0 is undefined
        assert_eq!(decode_scene_type_value(0), "Unknown (0)");

        // Values 2-254 are reserved
        assert_eq!(decode_scene_type_value(2), "Unknown (2)");
        assert_eq!(decode_scene_type_value(128), "Unknown (128)");

        // Maximum byte value (255)
        assert_eq!(decode_scene_type_value(255), "Unknown (255)");
    }

    #[test]
    fn test_decode_scene_type_value_boundary_values() {
        // Test boundary values around the defined value (1)
        assert_eq!(decode_scene_type_value(0), "Unknown (0)");
        assert_eq!(decode_scene_type_value(1), "Directly photographed");
        assert_eq!(decode_scene_type_value(2), "Unknown (2)");
    }

    // -------------------------------------------------------------------------
    // Consistency tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_consistency_between_functions() {
        // Ensure decode_scene_type and decode_scene_type_value produce
        // consistent results for all possible byte values
        for value in 0..=255u8 {
            let from_slice = decode_scene_type(&[value]);
            let from_value = decode_scene_type_value(value);
            assert_eq!(
                from_slice, from_value,
                "Mismatch for value {}: slice='{}', value='{}'",
                value, from_slice, from_value
            );
        }
    }

    #[test]
    fn test_output_matches_exiftool_format() {
        // Verify our output matches ExifTool's exact format
        // ExifTool outputs "Directly photographed" for value 1
        let result = decode_scene_type(&[1]);
        assert_eq!(result, "Directly photographed");

        // ExifTool outputs "Unknown (N)" for unknown values
        // (Note: ExifTool may show different text for some cameras,
        // but for standard EXIF this is the expected format)
        let unknown_result = decode_scene_type(&[2]);
        assert!(unknown_result.starts_with("Unknown ("));
        assert!(unknown_result.ends_with(")"));
    }
}
