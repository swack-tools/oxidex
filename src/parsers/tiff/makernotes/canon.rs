//! Canon MakerNote parser
//!
//! Parses Canon-specific EXIF MakerNote tags containing camera settings,
//! lens information, focus data, and other proprietary metadata.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use nom::{
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
    IResult,
};
use std::collections::HashMap;

// Canon MakerNote Tag IDs
const CANON_CAMERA_SETTINGS: u16 = 0x0001;
const CANON_FOCAL_LENGTH: u16 = 0x0002;
const CANON_SHOT_INFO: u16 = 0x0004;
const CANON_PANORAMA: u16 = 0x0005;
const CANON_IMAGE_TYPE: u16 = 0x0006;
const CANON_FIRMWARE_VERSION: u16 = 0x0007;
const CANON_FILE_NUMBER: u16 = 0x0008;
const CANON_OWNER_NAME: u16 = 0x0009;
const CANON_SERIAL_NUMBER: u16 = 0x000C;
const CANON_CAMERA_INFO: u16 = 0x000D;
const CANON_CUSTOM_FUNCTIONS: u16 = 0x000F;
const CANON_MODEL_ID: u16 = 0x0010;

// Canon signature (not always present)
const CANON_SIGNATURE: &[u8] = b"Canon";

/// Represents a Canon MakerNote tag value
#[derive(Debug, Clone, PartialEq)]
pub enum CanonTagValue {
    /// Single integer value
    Integer(i32),
    /// String value (model name, firmware, etc.)
    String(String),
    /// Array of integers (camera settings, shot info)
    IntArray(Vec<i16>),
}

/// Maps Canon MakerNote tag IDs to human-readable tag names.
///
/// # Parameters
/// - `tag_id`: The Canon-specific tag ID
///
/// # Returns
/// Tag name in the format "Canon:TagName"
///
/// # Example
/// ```
/// use exiftool_rs::parsers::tiff::makernotes::canon::canon_tag_to_name;
/// assert_eq!(canon_tag_to_name(0x0001), "Canon:CameraSettings");
/// ```
pub fn canon_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        CANON_CAMERA_SETTINGS => "CameraSettings",
        CANON_FOCAL_LENGTH => "FocalLength",
        CANON_SHOT_INFO => "ShotInfo",
        CANON_PANORAMA => "Panorama",
        CANON_IMAGE_TYPE => "ImageType",
        CANON_FIRMWARE_VERSION => "FirmwareVersion",
        CANON_FILE_NUMBER => "FileNumber",
        CANON_OWNER_NAME => "OwnerName",
        CANON_SERIAL_NUMBER => "SerialNumber",
        CANON_CAMERA_INFO => "CameraInfo",
        CANON_CUSTOM_FUNCTIONS => "CustomFunctions",
        CANON_MODEL_ID => "CanonModelID",
        _ => return format!("Canon:Unknown-{:#06X}", tag_id),
    };

    format!("Canon:{}", tag_name)
}

/// Parses a single IFD entry (12 bytes) in little-endian byte order.
///
/// This is a helper function for parsing Canon MakerNote IFD entries.
fn parse_ifd_entry_le(input: &[u8]) -> IResult<&[u8], IfdEntry> {
    map(
        |input| {
            let (input, tag_id) = le_u16(input)?;
            let (input, field_type) = le_u16(input)?;
            let (input, value_count) = le_u32(input)?;
            let (input, value_offset) = le_u32(input)?;
            Ok((input, (tag_id, field_type, value_count, value_offset)))
        },
        |(tag_id, field_type, value_count, value_offset)| IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        },
    )(input)
}

/// Parses a single IFD entry (12 bytes) in big-endian byte order.
///
/// This is a helper function for parsing Canon MakerNote IFD entries.
fn parse_ifd_entry_be(input: &[u8]) -> IResult<&[u8], IfdEntry> {
    map(
        |input| {
            let (input, tag_id) = be_u16(input)?;
            let (input, field_type) = be_u16(input)?;
            let (input, value_count) = be_u32(input)?;
            let (input, value_offset) = be_u32(input)?;
            Ok((input, (tag_id, field_type, value_count, value_offset)))
        },
        |(tag_id, field_type, value_count, value_offset)| IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        },
    )(input)
}

/// Parses IFD entries in the specified byte order.
///
/// # Parameters
/// - `input`: Input byte slice containing IFD entries
/// - `entry_count`: Number of entries to parse
/// - `byte_order`: Byte order for parsing
///
/// # Returns
/// IResult with remaining input and vector of IFD entries
fn parse_ifd_entries(
    input: &[u8],
    entry_count: u16,
    byte_order: ByteOrder,
) -> IResult<&[u8], Vec<IfdEntry>> {
    match byte_order {
        ByteOrder::LittleEndian => count(parse_ifd_entry_le, entry_count as usize)(input),
        ByteOrder::BigEndian => count(parse_ifd_entry_be, entry_count as usize)(input),
    }
}

/// Checks if data appears to be a Canon MakerNote.
///
/// Canon MakerNotes may optionally start with "Canon" signature,
/// but always contain a valid IFD structure.
///
/// # Parameters
/// - `data`: Raw byte data to check
///
/// # Returns
/// `true` if the data appears to be a Canon MakerNote, `false` otherwise
pub fn is_canon_makernote(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // Check for optional Canon signature
    if data.starts_with(CANON_SIGNATURE) {
        return true;
    }

    // Check if it looks like an IFD (starts with entry count)
    // Valid IFD has at least 2 bytes for entry count
    // Try both little-endian and big-endian interpretations
    if data.len() >= 2 {
        let entry_count_le = u16::from_le_bytes([data[0], data[1]]);
        let entry_count_be = u16::from_be_bytes([data[0], data[1]]);

        // Reasonable entry count (Canon typically has 1-100 entries)
        // Accept if either byte order yields a reasonable count
        let is_reasonable = |count: u16| count > 0 && count < 200;

        return is_reasonable(entry_count_le) || is_reasonable(entry_count_be);
    }

    false
}

/// Parses Canon MakerNote data into a map of tag names to values.
///
/// This parser extracts simple tags (strings and integers) from Canon MakerNotes.
/// Complex array tags (CameraSettings, ShotInfo, etc.) are deferred to Phase 2.
///
/// # Parameters
/// - `data`: Raw MakerNote data (may include Canon signature)
/// - `byte_order`: Byte order for parsing (usually matches TIFF header)
///
/// # Returns
/// HashMap of tag names to string values
///
/// # Errors
/// Returns error if IFD parsing fails or data is invalid
pub fn parse_canon_makernote(
    data: &[u8],
    byte_order: ByteOrder,
) -> Result<HashMap<String, String>> {
    if data.is_empty() {
        return Ok(HashMap::new());
    }

    // Skip Canon signature if present
    let ifd_data = if data.starts_with(CANON_SIGNATURE) {
        &data[CANON_SIGNATURE.len()..]
    } else {
        data
    };

    // Parse IFD entry count
    if ifd_data.len() < 2 {
        return Ok(HashMap::new());
    }

    let entry_count = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([ifd_data[0], ifd_data[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([ifd_data[0], ifd_data[1]]),
    };

    // Parse IFD entries
    let entries_start = &ifd_data[2..];
    let entries = match parse_ifd_entries(entries_start, entry_count, byte_order) {
        Ok((_, entries)) => entries,
        Err(_) => {
            // If parsing fails, return empty map rather than failing entire extraction
            return Ok(HashMap::new());
        }
    };

    let mut tags = HashMap::new();

    // Extract simple values from entries
    // Phase 1: Only extract simple string and integer tags
    for entry in entries {
        let tag_name = canon_tag_to_name(entry.tag_id);

        // For Phase 1, extract simple values only
        // Skip complex arrays (CameraSettings, ShotInfo, etc.)
        let value = match entry.tag_id {
            CANON_IMAGE_TYPE | CANON_FIRMWARE_VERSION | CANON_OWNER_NAME | CANON_SERIAL_NUMBER => {
                // String tags (EXIF type 2 = ASCII)
                // Pass full data (including Canon signature) for offset resolution
                extract_string_value(&entry, data)
            }
            CANON_MODEL_ID | CANON_FILE_NUMBER => {
                // Integer tags (EXIF type 4 = LONG)
                extract_integer_value(&entry)
            }
            _ => {
                // Skip complex arrays and unknown tags for Phase 1
                continue;
            }
        };

        if let Some(v) = value {
            tags.insert(tag_name, v);
        }
    }

    Ok(tags)
}

/// Extracts string value from IFD entry.
///
/// Handles both inline strings (≤4 bytes stored in value_offset field)
/// and offset-based strings (>4 bytes stored at specified offset).
///
/// # Parameters
/// - `entry`: IFD entry containing string data
/// - `full_data`: Complete MakerNote data (including Canon signature if present)
///
/// # Returns
/// Optional string value, trimmed and null-terminated
fn extract_string_value(entry: &IfdEntry, full_data: &[u8]) -> Option<String> {
    // Calculate the byte size based on value count
    let byte_count = entry.value_count as usize;

    // For inline strings (≤4 bytes), value is in value_offset field
    if byte_count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = std::str::from_utf8(&bytes[0..byte_count])
            .ok()?
            .trim_end_matches('\0')
            .trim();
        return Some(s.to_string());
    }

    // For longer strings, read from offset
    // Canon MakerNote offsets are relative to the start of the IFD data
    // (after the Canon signature if present)
    let offset = entry.value_offset as usize;

    // Calculate the IFD start position in full_data
    let ifd_start = if full_data.starts_with(CANON_SIGNATURE) {
        CANON_SIGNATURE.len()
    } else {
        0
    };

    // Calculate absolute offset in full_data
    let abs_offset = ifd_start + offset;

    // Bounds check and extract string
    if abs_offset + byte_count <= full_data.len() {
        let bytes = &full_data[abs_offset..abs_offset + byte_count];
        let s = std::str::from_utf8(bytes)
            .ok()?
            .trim_end_matches('\0')
            .trim();
        return Some(s.to_string());
    }

    None
}

/// Extracts integer value from IFD entry.
///
/// For simple integer tags, the value is stored directly in the value_offset field.
///
/// # Parameters
/// - `entry`: IFD entry containing integer data
///
/// # Returns
/// Optional string representation of the integer value
fn extract_integer_value(entry: &IfdEntry) -> Option<String> {
    // For simple integer tags (LONG type), value is in value_offset field
    Some(entry.value_offset.to_string())
}

/// Extracts an array of signed 16-bit integers from an IFD entry.
///
/// Handles both inline arrays (≤2 values fitting in 4-byte value_offset)
/// and offset-based arrays (>2 values stored elsewhere in data).
///
/// # Parameters
/// - `entry`: The IFD entry containing the array data
/// - `ifd_data`: The complete IFD data buffer for offset-based reads
/// - `byte_order`: Byte order for parsing (little or big endian)
///
/// # Returns
/// Optional vector of i16 values, or None if the data is invalid or wrong type
fn extract_i16_array(entry: &IfdEntry, ifd_data: &[u8], byte_order: ByteOrder) -> Option<Vec<i16>> {
    // Canon array tags use SHORT type (field_type = 3)
    if entry.field_type != 3 {
        return None;
    }

    let count = entry.value_count as usize;
    let bytes_needed = count * 2; // 2 bytes per i16

    // Inline: ≤2 shorts fit in 4-byte value_offset field
    if bytes_needed <= 4 {
        let mut result = Vec::with_capacity(count);
        let bytes = entry.value_offset.to_le_bytes();

        for i in 0..count {
            let offset = i * 2;
            let value = match byte_order {
                ByteOrder::LittleEndian => i16::from_le_bytes([bytes[offset], bytes[offset + 1]]),
                ByteOrder::BigEndian => i16::from_be_bytes([bytes[offset], bytes[offset + 1]]),
            };
            result.push(value);
        }

        return Some(result);
    }

    // Offset-based: read from ifd_data at specified offset
    let offset = entry.value_offset as usize;

    // Bounds check
    if offset + bytes_needed > ifd_data.len() {
        return None;
    }

    let mut result = Vec::with_capacity(count);
    let array_data = &ifd_data[offset..offset + bytes_needed];

    for i in 0..count {
        let byte_offset = i * 2;
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                i16::from_le_bytes([array_data[byte_offset], array_data[byte_offset + 1]])
            }
            ByteOrder::BigEndian => {
                i16::from_be_bytes([array_data[byte_offset], array_data[byte_offset + 1]])
            }
        };
        result.push(value);
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canon_tag_ids() {
        assert_eq!(CANON_CAMERA_SETTINGS, 0x0001);
        assert_eq!(CANON_FOCAL_LENGTH, 0x0002);
        assert_eq!(CANON_SHOT_INFO, 0x0004);
        assert_eq!(CANON_MODEL_ID, 0x0010);
    }

    #[test]
    fn test_canon_signature() {
        assert_eq!(CANON_SIGNATURE, b"Canon");
    }

    #[test]
    fn test_canon_tag_to_name() {
        assert_eq!(canon_tag_to_name(0x0001), "Canon:CameraSettings");
        assert_eq!(canon_tag_to_name(0x0002), "Canon:FocalLength");
        assert_eq!(canon_tag_to_name(0x0004), "Canon:ShotInfo");
        assert_eq!(canon_tag_to_name(0x0006), "Canon:ImageType");
        assert_eq!(canon_tag_to_name(0x0007), "Canon:FirmwareVersion");
        assert_eq!(canon_tag_to_name(0x0010), "Canon:CanonModelID");

        // Unknown tag
        assert_eq!(canon_tag_to_name(0xFFFF), "Canon:Unknown-0xFFFF");
    }

    #[test]
    fn test_is_canon_makernote() {
        // With Canon signature
        let data_with_sig = b"Canon\x00\x01\x00\x02\x00";
        assert!(is_canon_makernote(data_with_sig));

        // Without signature (starts with IFD)
        let data_without_sig = b"\x00\x01\x00\x02\x00";
        assert!(is_canon_makernote(data_without_sig));

        // Invalid data
        let invalid_data = b"Nikon";
        assert!(!is_canon_makernote(invalid_data));
    }

    #[test]
    fn test_parse_canon_makernote_basic() {
        // Create minimal Canon MakerNote with signature
        let mut data = Vec::new();

        // Canon signature (optional)
        data.extend_from_slice(b"Canon");

        // Simple IFD with one entry (little-endian format)
        data.extend_from_slice(&[
            0x01, 0x00, // Number of entries: 1 (little-endian)
            // Entry 1: ImageType (0x0006)
            0x06, 0x00, // Tag ID: 0x0006 (little-endian)
            0x02, 0x00, // Type: 2 = ASCII string (little-endian)
            0x0B, 0x00, 0x00, 0x00, // Count: 11 bytes (little-endian)
            0x12, 0x00, 0x00, 0x00, // Offset to data: 0x12 (18 bytes from IFD start)
            // Next IFD offset
            0x00, 0x00, 0x00, 0x00,
            // String data at offset 0x12 from IFD start (= byte 23 from data start)
            b'I', b'M', b'G', b':', b'E', b'O', b'S', b' ', b'R', b'5', 0x00,
        ]);

        let result = parse_canon_makernote(&data, ByteOrder::LittleEndian);
        assert!(result.is_ok());

        let tags = result.unwrap();
        assert!(tags.len() > 0);
        assert_eq!(tags.get("Canon:ImageType"), Some(&"IMG:EOS R5".to_string()));
    }

    #[test]
    fn test_extract_i16_array_inline() {
        // Test inline array (count * 2 <= 4 bytes)
        let entry = IfdEntry {
            tag_id: CANON_FOCAL_LENGTH,
            field_type: 3, // SHORT
            value_count: 2,
            value_offset: 0x0064_0032, // Two shorts: 50, 100 (little-endian)
        };

        let result = extract_i16_array(&entry, &[], ByteOrder::LittleEndian);
        assert_eq!(result, Some(vec![50, 100]));
    }

    #[test]
    fn test_extract_i16_array_offset() {
        // Test offset-based array (count * 2 > 4 bytes)
        let entry = IfdEntry {
            tag_id: CANON_CAMERA_SETTINGS,
            field_type: 3, // SHORT
            value_count: 4,
            value_offset: 0, // Offset to data
        };

        // Data at offset 0: [1, 2, 3, 4] as little-endian shorts
        let data = vec![
            0x01, 0x00, // 1
            0x02, 0x00, // 2
            0x03, 0x00, // 3
            0x04, 0x00, // 4
        ];

        let result = extract_i16_array(&entry, &data, ByteOrder::LittleEndian);
        assert_eq!(result, Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn test_extract_i16_array_big_endian() {
        let entry = IfdEntry {
            tag_id: CANON_CAMERA_SETTINGS,
            field_type: 3,
            value_count: 3, // Use 3 values to force offset-based reading (>4 bytes)
            value_offset: 0,
        };

        // Big-endian data: [256, 512, 768]
        let data = vec![
            0x01, 0x00, // 256 (big-endian)
            0x02, 0x00, // 512 (big-endian)
            0x03, 0x00, // 768 (big-endian)
        ];

        let result = extract_i16_array(&entry, &data, ByteOrder::BigEndian);
        assert_eq!(result, Some(vec![256, 512, 768]));
    }
}
