//! APP14 Adobe segment parser
//!
//! JPEG APP14 segments (marker 0xFFEE) contain Adobe-specific metadata when
//! they start with the "Adobe" identifier. This module provides parsing
//! functionality to extract Adobe DCT encoding metadata.
//!
//! # APP14 Adobe Segment Format
//!
//! The APP14 Adobe segment has the following structure:
//! - Identifier: "Adobe" (5 bytes)
//! - DCTEncodeVersion: Version number (2 bytes, big-endian)
//! - APP14Flags0: First set of flags (2 bytes, big-endian)
//! - APP14Flags1: Second set of flags (2 bytes, big-endian)
//! - ColorTransform: Color transformation code (1 byte)
//!   - 0 = Unknown (RGB or CMYK)
//!   - 1 = YCbCr (standard JPEG color space)
//!   - 2 = YCCK (CMYK encoded as YCCK)
//!
//! The ColorTransform value is critical for proper JPEG decoding as it
//! indicates how the color data should be interpreted.
//!
//! # Example
//!
//! ```ignore
//! use oxidex::parsers::jpeg::app_segments::app14_adobe::parse_app14_adobe;
//!
//! let data: &[u8] = &[/* APP14 Adobe segment data */];
//! let metadata = parse_app14_adobe(data)?;
//!
//! if let Some(transform) = metadata.get_string("APP14:ColorTransform") {
//!     println!("Color transform: {}", transform);
//! }
//! ```

use crate::core::MetadataMap;
use crate::core::TagValue;
use crate::error::{ExifToolError, Result};

/// The "Adobe" identifier that marks an APP14 segment as Adobe-format.
const ADOBE_IDENTIFIER: &[u8] = b"Adobe";

/// Minimum length for a valid APP14 Adobe segment.
///
/// Structure: "Adobe" (5) + Version (2) + Flags0 (2) + Flags1 (2) + ColorTransform (1) = 12 bytes
const MIN_ADOBE_SEGMENT_LENGTH: usize = 12;

/// Parses APP14 Adobe segment data and extracts DCT encoding metadata.
///
/// This function validates the "Adobe" identifier and extracts the following tags:
/// - `APP14:DCTEncodeVersion` - Version of the DCT encoder (Integer)
/// - `APP14:APP14Flags0` - First set of encoding flags (Integer)
/// - `APP14:APP14Flags1` - Second set of encoding flags (Integer)
/// - `APP14:ColorTransform` - Color transformation type (String)
///
/// # Arguments
///
/// * `data` - Raw APP14 segment data (excluding the APP14 marker and length bytes)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - A metadata map containing extracted APP14 tags
/// * `Err(ExifToolError)` - If the data is not a valid Adobe segment or is malformed
///
/// # Errors
///
/// Returns an error if:
/// - The segment does not start with "Adobe" identifier
/// - The segment is shorter than the minimum required length (12 bytes)
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app14_adobe::parse_app14_adobe;
///
/// // Construct a valid APP14 Adobe segment
/// let mut data = Vec::new();
/// data.extend_from_slice(b"Adobe");       // Identifier
/// data.extend_from_slice(&[0x00, 0x64]);  // DCTEncodeVersion = 100
/// data.extend_from_slice(&[0x80, 0x00]);  // Flags0 = 0x8000
/// data.extend_from_slice(&[0x00, 0x00]);  // Flags1 = 0
/// data.push(0x01);                        // ColorTransform = YCbCr
///
/// let metadata = parse_app14_adobe(&data)?;
/// assert_eq!(metadata.get_integer("APP14:DCTEncodeVersion"), Some(100));
/// assert_eq!(metadata.get_string("APP14:ColorTransform"), Some("YCbCr"));
/// ```
pub fn parse_app14_adobe(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Validate minimum segment length
    if data.len() < MIN_ADOBE_SEGMENT_LENGTH {
        return Err(ExifToolError::parse_error(format!(
            "APP14 Adobe segment too short: expected at least {} bytes, got {}",
            MIN_ADOBE_SEGMENT_LENGTH,
            data.len()
        )));
    }

    // Validate "Adobe" identifier at the start of the segment
    if &data[..ADOBE_IDENTIFIER.len()] != ADOBE_IDENTIFIER {
        return Err(ExifToolError::parse_error(
            "APP14 segment does not contain Adobe identifier",
        ));
    }

    // Parse fields after the "Adobe" identifier (big-endian byte order)
    // Offset 5: DCTEncodeVersion (2 bytes)
    let dct_version = u16::from_be_bytes([data[5], data[6]]);
    metadata.insert(
        "APP14:DCTEncodeVersion",
        TagValue::Integer(i64::from(dct_version)),
    );

    // Offset 7: APP14Flags0 (2 bytes)
    let flags0 = u16::from_be_bytes([data[7], data[8]]);
    metadata.insert("APP14:APP14Flags0", TagValue::Integer(i64::from(flags0)));

    // Offset 9: APP14Flags1 (2 bytes)
    let flags1 = u16::from_be_bytes([data[9], data[10]]);
    metadata.insert("APP14:APP14Flags1", TagValue::Integer(i64::from(flags1)));

    // Offset 11: ColorTransform (1 byte)
    let color_transform = data[11];
    let transform_string = match color_transform {
        0 => "Unknown",
        1 => "YCbCr",
        2 => "YCCK",
        other => {
            // For unknown values, we still record them but as a numeric string
            // This matches ExifTool's behavior of showing unexpected values
            metadata.insert(
                "APP14:ColorTransform",
                TagValue::String(format!("Unknown ({})", other)),
            );
            return Ok(metadata);
        }
    };

    metadata.insert(
        "APP14:ColorTransform",
        TagValue::String(transform_string.to_string()),
    );

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to construct a valid APP14 Adobe segment.
    ///
    /// Creates segment data with the specified values, making test construction
    /// cleaner and more readable.
    fn make_adobe_segment(
        dct_version: u16,
        flags0: u16,
        flags1: u16,
        color_transform: u8,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(MIN_ADOBE_SEGMENT_LENGTH);
        data.extend_from_slice(ADOBE_IDENTIFIER);
        data.extend_from_slice(&dct_version.to_be_bytes());
        data.extend_from_slice(&flags0.to_be_bytes());
        data.extend_from_slice(&flags1.to_be_bytes());
        data.push(color_transform);
        data
    }

    /// Tests parsing of a valid APP14 Adobe segment with YCbCr color transform.
    #[test]
    fn test_parse_valid_ycbcr_segment() {
        let data = make_adobe_segment(100, 0x8000, 0x0000, 1);

        let result = parse_app14_adobe(&data);
        assert!(
            result.is_ok(),
            "Parsing should succeed for valid YCbCr segment"
        );

        let metadata = result.unwrap();

        assert_eq!(
            metadata.get_integer("APP14:DCTEncodeVersion"),
            Some(100),
            "DCTEncodeVersion should be 100"
        );
        assert_eq!(
            metadata.get_integer("APP14:APP14Flags0"),
            Some(0x8000),
            "APP14Flags0 should be 0x8000"
        );
        assert_eq!(
            metadata.get_integer("APP14:APP14Flags1"),
            Some(0),
            "APP14Flags1 should be 0"
        );
        assert_eq!(
            metadata.get_string("APP14:ColorTransform"),
            Some("YCbCr"),
            "ColorTransform should be 'YCbCr'"
        );
    }

    /// Tests parsing with YCCK color transform (value 2).
    #[test]
    fn test_parse_ycck_color_transform() {
        let data = make_adobe_segment(100, 0, 0, 2);

        let result = parse_app14_adobe(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("APP14:ColorTransform"),
            Some("YCCK"),
            "ColorTransform should be 'YCCK'"
        );
    }

    /// Tests parsing with Unknown color transform (value 0).
    #[test]
    fn test_parse_unknown_color_transform() {
        let data = make_adobe_segment(100, 0, 0, 0);

        let result = parse_app14_adobe(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("APP14:ColorTransform"),
            Some("Unknown"),
            "ColorTransform should be 'Unknown'"
        );
    }

    /// Tests parsing with an unexpected color transform value (e.g., 3).
    #[test]
    fn test_parse_unexpected_color_transform() {
        let data = make_adobe_segment(100, 0, 0, 3);

        let result = parse_app14_adobe(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("APP14:ColorTransform"),
            Some("Unknown (3)"),
            "Unexpected value should be formatted as 'Unknown (3)'"
        );
    }

    /// Tests that a segment without Adobe identifier returns an error.
    #[test]
    fn test_invalid_identifier() {
        let mut data = Vec::new();
        data.extend_from_slice(b"NotAd"); // Wrong identifier
        data.extend_from_slice(&[0x00; 7]); // Padding to meet minimum length

        let result = parse_app14_adobe(&data);
        assert!(result.is_err(), "Should error on invalid identifier");

        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(
                message.contains("Adobe identifier"),
                "Error message should mention Adobe identifier"
            );
        } else {
            panic!("Expected ParseError variant");
        }
    }

    /// Tests that a segment shorter than minimum length returns an error.
    #[test]
    fn test_segment_too_short() {
        let data = b"Adobe".to_vec(); // Only identifier, missing data fields

        let result = parse_app14_adobe(&data);
        assert!(result.is_err(), "Should error when segment is too short");

        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(
                message.contains("too short"),
                "Error message should mention segment is too short"
            );
        } else {
            panic!("Expected ParseError variant");
        }
    }

    /// Tests parsing with maximum values for all fields.
    #[test]
    fn test_max_field_values() {
        let data = make_adobe_segment(0xFFFF, 0xFFFF, 0xFFFF, 1);

        let result = parse_app14_adobe(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_integer("APP14:DCTEncodeVersion"),
            Some(65535),
            "DCTEncodeVersion should handle max u16 value"
        );
        assert_eq!(
            metadata.get_integer("APP14:APP14Flags0"),
            Some(65535),
            "APP14Flags0 should handle max u16 value"
        );
        assert_eq!(
            metadata.get_integer("APP14:APP14Flags1"),
            Some(65535),
            "APP14Flags1 should handle max u16 value"
        );
    }

    /// Tests parsing with zero values for all fields.
    #[test]
    fn test_zero_field_values() {
        let data = make_adobe_segment(0, 0, 0, 0);

        let result = parse_app14_adobe(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_integer("APP14:DCTEncodeVersion"),
            Some(0),
            "DCTEncodeVersion should handle zero"
        );
        assert_eq!(
            metadata.get_integer("APP14:APP14Flags0"),
            Some(0),
            "APP14Flags0 should handle zero"
        );
        assert_eq!(
            metadata.get_integer("APP14:APP14Flags1"),
            Some(0),
            "APP14Flags1 should handle zero"
        );
        assert_eq!(
            metadata.get_string("APP14:ColorTransform"),
            Some("Unknown"),
            "ColorTransform 0 should be 'Unknown'"
        );
    }

    /// Tests that segment with extra trailing data is still parsed correctly.
    #[test]
    fn test_extra_trailing_data() {
        let mut data = make_adobe_segment(100, 0, 0, 1);
        // Add extra bytes after the standard structure
        data.extend_from_slice(&[0xFF, 0xAA, 0xBB, 0xCC]);

        let result = parse_app14_adobe(&data);
        assert!(result.is_ok(), "Extra trailing data should be ignored");

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_integer("APP14:DCTEncodeVersion"),
            Some(100),
            "Should parse correctly despite extra data"
        );
    }

    /// Tests the minimum segment length constant.
    #[test]
    fn test_minimum_segment_length_constant() {
        // "Adobe" (5) + Version (2) + Flags0 (2) + Flags1 (2) + ColorTransform (1) = 12
        assert_eq!(
            MIN_ADOBE_SEGMENT_LENGTH, 12,
            "Minimum length should be 12 bytes"
        );
    }

    /// Tests parsing with exactly the minimum required length.
    #[test]
    fn test_exact_minimum_length() {
        let data = make_adobe_segment(100, 0, 0, 1);
        assert_eq!(
            data.len(),
            MIN_ADOBE_SEGMENT_LENGTH,
            "Test data should be exactly minimum length"
        );

        let result = parse_app14_adobe(&data);
        assert!(result.is_ok(), "Should succeed with exactly minimum length");
    }

    /// Tests that empty data returns an appropriate error.
    #[test]
    fn test_empty_data() {
        let data: Vec<u8> = vec![];

        let result = parse_app14_adobe(&data);
        assert!(result.is_err(), "Should error on empty data");
    }

    /// Tests typical values seen in real-world JPEG files.
    #[test]
    fn test_typical_real_world_values() {
        // Common values from Photoshop-saved JPEGs
        let data = make_adobe_segment(100, 0, 0, 1);

        let result = parse_app14_adobe(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_integer("APP14:DCTEncodeVersion"), Some(100));
        assert_eq!(metadata.get_string("APP14:ColorTransform"), Some("YCbCr"));
    }

    /// Tests that Adobe identifier matching is case-sensitive.
    #[test]
    fn test_identifier_case_sensitivity() {
        // Lowercase "adobe" should not match
        let mut data = Vec::new();
        data.extend_from_slice(b"adobe"); // Lowercase
        data.extend_from_slice(&[0x00; 7]);

        let result = parse_app14_adobe(&data);
        assert!(result.is_err(), "Lowercase identifier should not match");
    }
}
