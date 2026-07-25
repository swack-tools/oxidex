//! APP12 Agfa Picture Info parser
//!
//! This module parses JPEG APP12 segments from Agfa cameras. The Agfa Picture Info
//! format uses key=value pairs containing camera and image metadata. There are two
//! variants of this format:
//!
//! 1. **Standard format**: Begins with "AGFA" identifier (4 bytes), followed by a
//!    null terminator, then newline-separated key=value pairs.
//!
//! 2. **Legacy format**: Older Agfa cameras (like the SR84) wrote APP12 segments
//!    that start directly with key=value pairs without an "AGFA" prefix.
//!
//! # Format Structure
//!
//! ## Standard Format
//! ```text
//! AGFA\0CameraType=DC-1033\nVersion=1.0\n...
//! ```
//!
//! ## Legacy Format (no identifier)
//! ```text
//! Type=SR84\nVersion=v84-71\nID=AGFA DIGITAL CAMERA\n...
//! ```
//!
//! # Supported Tags
//!
//! - `ID` - Unique identifier for the image
//! - `Type` / `CameraType` - Model name of the Agfa camera
//! - `Version` - Firmware or software version
//! - `DateTimeOriginal` - Original capture date and time
//! - `ExposureTime` - Shutter speed / exposure duration
//! - `FNumber` - Aperture value (f-stop)
//! - `Flash` - Flash status (fired, not fired, etc.)
//! - And other proprietary Agfa tags
//!
//! # Example
//!
//! ```ignore
//! use oxidex::parsers::jpeg::app_segments::app12_agfa::parse_app12_agfa;
//!
//! // Standard format with AGFA identifier
//! let data = b"AGFA\0ID=12345\nCameraType=AgfaPhoto DC-1033\nVersion=1.0\n";
//! let result = parse_app12_agfa(data)?;
//! assert_eq!(result.get_string("Agfa:CameraType"), Some("AgfaPhoto DC-1033"));
//!
//! // Legacy format without identifier
//! let data = b"Type=SR84\nVersion=v84-71\nID=AGFA DIGITAL CAMERA\n";
//! let result = parse_app12_agfa(data)?;
//! assert_eq!(result.get_string("Agfa:Type"), Some("SR84"));
//! ```

use crate::core::MetadataMap;
use crate::core::TagValue;
use crate::error::Result;

/// Identifier bytes that mark a standard Agfa Picture Info APP12 segment.
/// Some older cameras omit this identifier and start directly with key=value pairs.
const AGFA_IDENTIFIER: &[u8; 4] = b"AGFA";

/// Minimum length required for a valid Agfa APP12 segment.
/// This is the minimum for key=value format (e.g., "A=B" is 3 bytes).
const MIN_AGFA_LENGTH: usize = 3;

/// Parses an APP12 Agfa Picture Info segment from raw JPEG data.
///
/// This function extracts metadata from Agfa camera APP12 segments,
/// which store information as key=value pairs. The function supports both:
/// - Standard format with "AGFA" identifier prefix
/// - Legacy format that starts directly with key=value pairs
///
/// # Arguments
///
/// * `data` - Raw bytes of the APP12 segment payload (after the APP12 marker and length)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed metadata with tags prefixed by "Agfa:"
/// * `Err(ExifToolError)` - If the segment is too short or cannot be parsed
///
/// # Format Details
///
/// The standard Agfa APP12 format consists of:
/// 1. 4-byte identifier: "AGFA" (optional in legacy format)
/// 2. Null terminator (0x00) if identifier present
/// 3. Key=value pairs separated by newlines or carriage returns
///
/// Legacy format from older cameras:
/// 1. Key=value pairs starting immediately (no identifier)
/// 2. Pairs separated by newlines or carriage returns
///
/// Each key=value pair is parsed and stored in the MetadataMap with
/// the "Agfa:" prefix (e.g., "Agfa:CameraType", "Agfa:Type").
///
/// # Example
///
/// ```ignore
/// // Standard format
/// let segment_data = b"AGFA\0CameraType=DC-1033\nExposureTime=1/125\n";
/// let metadata = parse_app12_agfa(segment_data)?;
/// assert!(metadata.contains_key("Agfa:CameraType"));
///
/// // Legacy format (no identifier)
/// let segment_data = b"Type=SR84\nVersion=v84-71\n";
/// let metadata = parse_app12_agfa(segment_data)?;
/// assert!(metadata.contains_key("Agfa:Type"));
/// ```
pub fn parse_app12_agfa(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Validate minimum segment length
    // Need at least a few bytes for any valid key=value content
    if data.len() < MIN_AGFA_LENGTH {
        return Err(crate::error::ExifToolError::parse_error(format!(
            "APP12 Agfa segment too short: {} bytes (minimum {} required)",
            data.len(),
            MIN_AGFA_LENGTH
        )));
    }

    // Determine the content start offset based on whether the AGFA identifier is present.
    // Standard format: "AGFA\0" followed by key=value pairs
    // Legacy format: key=value pairs start immediately
    let content_offset = if data.len() >= 4 && &data[0..4] == AGFA_IDENTIFIER {
        // Standard format with AGFA identifier
        // Skip the identifier and find where content starts
        let content_start = find_content_start(&data[4..]);
        4 + content_start
    } else {
        // Legacy format without identifier - content starts at beginning
        // Skip any leading whitespace or null bytes
        find_content_start(data)
    };

    // If no content after the identifier/whitespace, return empty metadata
    // This is not an error - some cameras may write empty Agfa segments
    if content_offset >= data.len() {
        return Ok(metadata);
    }

    // Parse the key=value pairs from the remaining content
    let content = &data[content_offset..];
    parse_key_value_pairs(content, &mut metadata);

    Ok(metadata)
}

/// Finds the start of actual content after the identifier.
///
/// Skips null terminators and whitespace to find where the key=value
/// content begins. This handles variations in how different Agfa
/// cameras format the segment.
///
/// # Arguments
///
/// * `data` - Slice of bytes after the "AGFA" identifier
///
/// # Returns
///
/// The offset where content begins (relative to the input slice)
fn find_content_start(data: &[u8]) -> usize {
    let mut offset = 0;

    // Skip null terminators that may follow the identifier
    while offset < data.len() && data[offset] == 0x00 {
        offset += 1;
    }

    // Skip any leading whitespace (CR, LF, space, tab)
    while offset < data.len() && is_whitespace(data[offset]) {
        offset += 1;
    }

    offset
}

/// Checks if a byte is considered whitespace in this context.
///
/// # Arguments
///
/// * `byte` - The byte to check
///
/// # Returns
///
/// True if the byte is a space, tab, carriage return, or newline
#[inline]
fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// Parses key=value pairs from the content section and populates the metadata map.
///
/// The content is expected to contain lines in the format "Key=Value",
/// separated by newlines (LF) or carriage returns (CR). Empty lines and
/// lines without an equals sign are skipped.
///
/// # Arguments
///
/// * `content` - Raw bytes containing the key=value pairs
/// * `metadata` - MetadataMap to populate with parsed values
///
/// # Tag Handling
///
/// - Keys are trimmed of whitespace
/// - Values are trimmed of whitespace
/// - Empty keys or values are skipped
/// - All tags are prefixed with "Agfa:" in the metadata map
/// - Numeric values are detected and stored as appropriate types
fn parse_key_value_pairs(content: &[u8], metadata: &mut MetadataMap) {
    // Convert to string, handling potential encoding issues gracefully
    // Agfa data is typically ASCII/Latin-1, but we use lossy conversion
    // to ensure robustness against malformed data
    let content_str = String::from_utf8_lossy(content);

    // Split on common line terminators (handles CR, LF, and CRLF)
    for line in content_str.split(['\n', '\r']) {
        // Skip empty lines
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Find the equals sign that separates key from value
        if let Some(equals_pos) = line.find('=') {
            let key = line[..equals_pos].trim();
            let value = line[equals_pos + 1..].trim();

            // Skip if either key or value is empty
            if key.is_empty() || value.is_empty() {
                continue;
            }

            // Create the tag name with Agfa prefix
            let tag_name = format!("Agfa:{}", key);

            // Attempt to parse as numeric value, falling back to string
            let tag_value = parse_value(value);

            // Picture Info fields are exposed by ExifTool in the APP12 group.
            // Preserve the display-ready fraction rather than converting it
            // to a floating-point value.
            if key.eq_ignore_ascii_case("ExposureTime") {
                metadata.insert(
                    "APP12:ExposureTime".to_string(),
                    TagValue::String(value.to_string()),
                );
            }

            // Flash is a standard Picture Info field. ExifTool exposes the
            // display-ready value (for example, "Off") in the APP12 group.
            if key.eq_ignore_ascii_case("Flash") {
                metadata.insert(
                    "APP12:Flash".to_string(),
                    TagValue::String(value.to_string()),
                );
            }

            metadata.insert(tag_name, tag_value);
        }
    }
}

/// Parses a string value and returns the appropriate TagValue type.
///
/// This function attempts to detect the type of value and convert it
/// to the most appropriate TagValue variant:
/// - Integer for whole numbers
/// - Float for decimal numbers (including rational notation like "1/125")
/// - String for everything else
///
/// # Arguments
///
/// * `value` - The string value to parse
///
/// # Returns
///
/// A TagValue of the appropriate type for the value
fn parse_value(value: &str) -> TagValue {
    // Try to parse as integer first (most specific)
    if let Ok(int_val) = value.parse::<i64>() {
        return TagValue::Integer(int_val);
    }

    // Try to parse as floating point
    if let Ok(float_val) = value.parse::<f64>() {
        return TagValue::Float(float_val);
    }

    // Check for rational notation (e.g., "1/125" for exposure time)
    // This is common for ExposureTime values in camera metadata
    if let Some(rational_value) = try_parse_rational(value) {
        return rational_value;
    }

    // Default to string for everything else
    TagValue::String(value.to_string())
}

/// Attempts to parse a value in rational notation (e.g., "1/125").
///
/// This handles common camera metadata formats for exposure time,
/// aperture, and other values that are expressed as fractions.
///
/// # Arguments
///
/// * `value` - The string to parse (e.g., "1/125", "f/2.8")
///
/// # Returns
///
/// Some(TagValue::Rational) if parsing succeeds, None otherwise
fn try_parse_rational(value: &str) -> Option<TagValue> {
    // Look for the "/" separator
    let parts: Vec<&str> = value.split('/').collect();
    if parts.len() != 2 {
        return None;
    }

    // Parse numerator and denominator
    let numerator = parts[0].trim().parse::<i32>().ok()?;
    let denominator = parts[1].trim().parse::<i32>().ok()?;

    // Avoid division by zero
    if denominator == 0 {
        return None;
    }

    Some(TagValue::Rational {
        numerator,
        denominator,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test parsing a basic Agfa APP12 segment with common tags.
    #[test]
    fn test_parse_basic_agfa_segment() {
        // Simulate an Agfa APP12 segment with typical metadata
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00); // Null terminator
        data.extend_from_slice(b"ID=IMG12345\nCameraType=AgfaPhoto DC-1033\nVersion=v1.0\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        // ID contains non-numeric prefix so it's stored as string
        assert_eq!(metadata.get_string("Agfa:ID"), Some("IMG12345"));
        assert_eq!(
            metadata.get_string("Agfa:CameraType"),
            Some("AgfaPhoto DC-1033")
        );
        // Version has 'v' prefix so it's stored as string
        assert_eq!(metadata.get_string("Agfa:Version"), Some("v1.0"));
    }

    /// Test parsing segment with exposure and aperture values.
    #[test]
    fn test_parse_exposure_settings() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"ExposureTime=1/125\nFNumber=5.6\nISO=200\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();

        assert_eq!(metadata.get_string("APP12:ExposureTime"), Some("1/125"));

        // ExposureTime should be parsed as a rational
        match metadata.get("Agfa:ExposureTime") {
            Some(TagValue::Rational {
                numerator,
                denominator,
            }) => {
                assert_eq!(*numerator, 1);
                assert_eq!(*denominator, 125);
            }
            other => panic!("Expected Rational, got {:?}", other),
        }

        // FNumber should be parsed as a float
        assert_eq!(metadata.get_float("Agfa:FNumber"), Some(5.6));

        // ISO should be parsed as an integer
        assert_eq!(metadata.get_integer("Agfa:ISO"), Some(200));
    }

    /// Test parsing segment with DateTimeOriginal.
    #[test]
    fn test_parse_datetime() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"DateTimeOriginal=2024:03:15 10:30:00\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        // DateTime is stored as string since we don't do automatic date parsing
        assert_eq!(
            metadata.get_string("Agfa:DateTimeOriginal"),
            Some("2024:03:15 10:30:00")
        );
    }

    /// Test parsing segment with Flash tag.
    #[test]
    fn test_parse_flash_tag() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"Flash=Fired\nFlashMode=Auto\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:Flash"), Some("Fired"));
        assert_eq!(metadata.get_string("APP12:Flash"), Some("Fired"));
        assert_eq!(metadata.get_string("Agfa:FlashMode"), Some("Auto"));
    }

    /// Test that segment too short returns an error.
    #[test]
    fn test_segment_too_short() {
        let data = b"AB"; // Only 2 bytes, less than minimum of 3
        let result = parse_app12_agfa(data);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("APP12 Agfa segment too short")
        );
    }

    /// Test parsing legacy format without AGFA identifier.
    /// Older Agfa cameras like SR84 wrote APP12 segments that start directly
    /// with key=value pairs without the "AGFA" prefix.
    #[test]
    fn test_legacy_format_no_identifier() {
        let data = b"Type=SR84\nVersion=v84-71\nID=AGFA DIGITAL CAMERA\n";
        let result = parse_app12_agfa(data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:Type"), Some("SR84"));
        assert_eq!(metadata.get_string("Agfa:Version"), Some("v84-71"));
        assert_eq!(metadata.get_string("Agfa:ID"), Some("AGFA DIGITAL CAMERA"));
    }

    /// Test that data without key=value format returns empty metadata.
    /// If there's no equals sign, no tags will be extracted but it's not an error.
    #[test]
    fn test_data_without_keyvalue_format() {
        let data = b"XYZW\0SomeDataWithoutEquals\n";
        let result = parse_app12_agfa(data);
        // This should succeed but return empty metadata since there are no key=value pairs
        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert!(metadata.is_empty());
    }

    /// Test parsing segment with only the identifier (no content).
    #[test]
    fn test_empty_content() {
        let data = b"AGFA\0";
        let result = parse_app12_agfa(&data[..]);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert!(metadata.is_empty());
    }

    /// Test parsing segment with CRLF line endings (Windows style).
    #[test]
    fn test_crlf_line_endings() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"Key1=Value1\r\nKey2=Value2\r\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:Key1"), Some("Value1"));
        assert_eq!(metadata.get_string("Agfa:Key2"), Some("Value2"));
    }

    /// Test parsing segment with CR-only line endings (old Mac style).
    #[test]
    fn test_cr_line_endings() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"Key1=Value1\rKey2=Value2\r");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:Key1"), Some("Value1"));
        assert_eq!(metadata.get_string("Agfa:Key2"), Some("Value2"));
    }

    /// Test that lines without equals sign are skipped.
    #[test]
    fn test_malformed_lines_skipped() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"ValidKey=ValidValue\nNoEqualsHere\nAnotherValid=Value\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata.get_string("Agfa:ValidKey"), Some("ValidValue"));
        assert_eq!(metadata.get_string("Agfa:AnotherValid"), Some("Value"));
    }

    /// Test that empty keys and values are skipped.
    #[test]
    fn test_empty_key_or_value_skipped() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"=EmptyKey\nEmptyValue=\nValid=Data\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata.get_string("Agfa:Valid"), Some("Data"));
    }

    /// Test parsing values with spaces.
    #[test]
    fn test_values_with_spaces() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"Description=This is a test photo\nCameraType=Agfa Photo 1234\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("Agfa:Description"),
            Some("This is a test photo")
        );
        assert_eq!(
            metadata.get_string("Agfa:CameraType"),
            Some("Agfa Photo 1234")
        );
    }

    /// Test parsing negative integer values.
    #[test]
    fn test_negative_integer() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"ExposureCompensation=-2\n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_integer("Agfa:ExposureCompensation"), Some(-2));
    }

    /// Test the rational parsing helper function directly.
    #[test]
    fn test_try_parse_rational() {
        // Valid rational
        let result = try_parse_rational("1/125");
        assert!(result.is_some());
        match result.unwrap() {
            TagValue::Rational {
                numerator,
                denominator,
            } => {
                assert_eq!(numerator, 1);
                assert_eq!(denominator, 125);
            }
            _ => panic!("Expected Rational"),
        }

        // Invalid: not a rational
        assert!(try_parse_rational("not-a-rational").is_none());

        // Invalid: division by zero
        assert!(try_parse_rational("1/0").is_none());

        // Invalid: non-numeric parts
        assert!(try_parse_rational("abc/def").is_none());
    }

    /// Test the find_content_start helper function.
    #[test]
    fn test_find_content_start() {
        // Multiple null terminators
        let data = [0x00, 0x00, 0x00, b'A', b'B', b'C'];
        assert_eq!(find_content_start(&data), 3);

        // Null followed by whitespace
        let data = [0x00, b' ', b'\t', b'A'];
        assert_eq!(find_content_start(&data), 3);

        // No null or whitespace
        let data = [b'A', b'B', b'C'];
        assert_eq!(find_content_start(&data), 0);

        // All nulls
        let data = [0x00, 0x00, 0x00];
        assert_eq!(find_content_start(&data), 3);
    }

    /// Test the is_whitespace helper function.
    #[test]
    fn test_is_whitespace() {
        assert!(is_whitespace(b' '));
        assert!(is_whitespace(b'\t'));
        assert!(is_whitespace(b'\r'));
        assert!(is_whitespace(b'\n'));
        assert!(!is_whitespace(b'A'));
        assert!(!is_whitespace(b'0'));
        assert!(!is_whitespace(0x00));
    }

    /// Test parsing a comprehensive Agfa segment with all tag types.
    #[test]
    fn test_comprehensive_agfa_segment() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(
            b"ID=IMG12345\n\
              CameraType=AgfaPhoto DC-1033\n\
              Version=1.0.5\n\
              DateTimeOriginal=2024:03:15 10:30:00\n\
              ExposureTime=1/125\n\
              FNumber=5.6\n\
              ISO=400\n\
              Flash=Fired\n\
              FocalLength=35\n\
              WhiteBalance=Auto\n",
        );

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();

        // Verify all expected tags are present (ID is non-numeric so stored as string)
        assert_eq!(metadata.get_string("Agfa:ID"), Some("IMG12345"));
        assert_eq!(
            metadata.get_string("Agfa:CameraType"),
            Some("AgfaPhoto DC-1033")
        );
        assert_eq!(metadata.get_string("Agfa:Version"), Some("1.0.5"));
        assert_eq!(
            metadata.get_string("Agfa:DateTimeOriginal"),
            Some("2024:03:15 10:30:00")
        );
        assert_eq!(metadata.get_float("Agfa:FNumber"), Some(5.6));
        assert_eq!(metadata.get_integer("Agfa:ISO"), Some(400));
        assert_eq!(metadata.get_string("Agfa:Flash"), Some("Fired"));
        assert_eq!(metadata.get_integer("Agfa:FocalLength"), Some(35));
        assert_eq!(metadata.get_string("Agfa:WhiteBalance"), Some("Auto"));

        // Verify ExposureTime is a rational
        match metadata.get("Agfa:ExposureTime") {
            Some(TagValue::Rational {
                numerator,
                denominator,
            }) => {
                assert_eq!(*numerator, 1);
                assert_eq!(*denominator, 125);
            }
            other => panic!("Expected Rational for ExposureTime, got {:?}", other),
        }
    }

    /// Test that whitespace around keys and values is properly trimmed.
    #[test]
    fn test_whitespace_trimming() {
        let mut data = Vec::new();
        data.extend_from_slice(b"AGFA");
        data.push(0x00);
        data.extend_from_slice(b"  Key1  =  Value1  \n  Key2=Value2  \n");

        let result = parse_app12_agfa(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:Key1"), Some("Value1"));
        assert_eq!(metadata.get_string("Agfa:Key2"), Some("Value2"));
    }

    /// Test legacy format with CameraType at start (another common variant).
    #[test]
    fn test_legacy_format_cameratype_start() {
        let data = b"CameraType=SR84\nVersion=v84-71\n";
        let result = parse_app12_agfa(data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:CameraType"), Some("SR84"));
        assert_eq!(metadata.get_string("Agfa:Version"), Some("v84-71"));
    }

    /// Test legacy format with ID at start containing camera identifier string.
    #[test]
    fn test_legacy_format_id_start() {
        let data = b"ID=AGFA DIGITAL CAMERA\nType=ePhoto780\nVersion=v1.0\n";
        let result = parse_app12_agfa(data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:ID"), Some("AGFA DIGITAL CAMERA"));
        assert_eq!(metadata.get_string("Agfa:Type"), Some("ePhoto780"));
        // Version has 'v' prefix so it's stored as string
        assert_eq!(metadata.get_string("Agfa:Version"), Some("v1.0"));
    }

    /// Test legacy format with Version at start.
    #[test]
    fn test_legacy_format_version_start() {
        let data = b"Version=v84-71\nType=SR84\nID=AGFA DIGITAL CAMERA\n";
        let result = parse_app12_agfa(data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:Version"), Some("v84-71"));
        assert_eq!(metadata.get_string("Agfa:Type"), Some("SR84"));
        assert_eq!(metadata.get_string("Agfa:ID"), Some("AGFA DIGITAL CAMERA"));
    }

    /// Test legacy format with leading whitespace.
    #[test]
    fn test_legacy_format_leading_whitespace() {
        let data = b"  \n\rType=SR84\nVersion=v1.0\n";
        let result = parse_app12_agfa(data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("Agfa:Type"), Some("SR84"));
        // Version has 'v' prefix so it's stored as string
        assert_eq!(metadata.get_string("Agfa:Version"), Some("v1.0"));
    }

    /// Test that both standard and legacy formats parse the same tags correctly.
    /// This ensures backward compatibility with the standard format.
    #[test]
    fn test_format_compatibility() {
        // Standard format with AGFA identifier
        let standard_data = b"AGFA\0Type=SR84\nVersion=v84-71\n";
        let standard_result = parse_app12_agfa(standard_data);
        assert!(standard_result.is_ok());
        let standard_metadata = standard_result.unwrap();

        // Legacy format without identifier
        let legacy_data = b"Type=SR84\nVersion=v84-71\n";
        let legacy_result = parse_app12_agfa(legacy_data);
        assert!(legacy_result.is_ok());
        let legacy_metadata = legacy_result.unwrap();

        // Both should produce the same tags
        assert_eq!(
            standard_metadata.get_string("Agfa:Type"),
            legacy_metadata.get_string("Agfa:Type")
        );
        assert_eq!(
            standard_metadata.get_string("Agfa:Version"),
            legacy_metadata.get_string("Agfa:Version")
        );
    }
}
