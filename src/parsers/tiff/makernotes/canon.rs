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

// CameraSettings array (tag 0x0001) indices
// Array contains ~50 values with camera settings
// Reference: ExifTool Canon.pm CameraSettings table
const CAMERA_SETTINGS_MACRO_MODE: usize = 1;
const CAMERA_SETTINGS_SELF_TIMER: usize = 2;
const CAMERA_SETTINGS_QUALITY: usize = 3;
const CAMERA_SETTINGS_FLASH_MODE: usize = 4;
const CAMERA_SETTINGS_DRIVE_MODE: usize = 5;
const CAMERA_SETTINGS_FOCUS_MODE: usize = 7;
const CAMERA_SETTINGS_IMAGE_SIZE: usize = 10;
const CAMERA_SETTINGS_EASY_MODE: usize = 11;
const CAMERA_SETTINGS_CONTRAST: usize = 13;
const CAMERA_SETTINGS_SATURATION: usize = 14;
const CAMERA_SETTINGS_SHARPNESS: usize = 15;
const CAMERA_SETTINGS_ISO: usize = 16;
const CAMERA_SETTINGS_METERING_MODE: usize = 17;
const CAMERA_SETTINGS_FOCUS_TYPE: usize = 18;
const CAMERA_SETTINGS_AF_POINT: usize = 19;
const CAMERA_SETTINGS_EXPOSURE_MODE: usize = 20;
const CAMERA_SETTINGS_FLASH_ACTIVITY: usize = 28;
const CAMERA_SETTINGS_FOCUS_CONTINUOUS: usize = 32;

// ShotInfo array (tag 0x0004) indices
const SHOT_INFO_AUTO_ISO: usize = 1;
const SHOT_INFO_BASE_ISO: usize = 2;
const SHOT_INFO_MEASURED_EV: usize = 3;
const SHOT_INFO_TARGET_APERTURE: usize = 4;
const SHOT_INFO_TARGET_SHUTTER_SPEED: usize = 5;
const SHOT_INFO_WHITE_BALANCE: usize = 7;
const SHOT_INFO_SLOW_SHUTTER: usize = 8;
const SHOT_INFO_SEQUENCE_NUMBER: usize = 9;
const SHOT_INFO_FLASH_GUIDE_NUMBER: usize = 13;
const SHOT_INFO_AF_POINTS_USED: usize = 14;
const SHOT_INFO_FLASH_EXPOSURE_COMP: usize = 15;
const SHOT_INFO_AUTO_EXPOSURE_BRACKETING: usize = 16;
const SHOT_INFO_SUBJECT_DISTANCE: usize = 19;

/// Decodes Canon macro mode value to human-readable string
fn decode_macro_mode(value: i16) -> String {
    match value {
        1 => "Macro".to_string(),
        2 => "Normal".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon quality setting to human-readable string
fn decode_quality(value: i16) -> String {
    match value {
        -1 => "n/a".to_string(),
        1 => "Economy".to_string(),
        2 => "Normal".to_string(),
        3 => "Fine".to_string(),
        4 => "RAW".to_string(),
        5 => "Superfine".to_string(),
        7 => "CRAW".to_string(),
        130 => "Normal Movie".to_string(),
        131 => "Movie (2)".to_string(),
        132 => "Movie (3)".to_string(),
        133 => "Movie (4)".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon flash mode to human-readable string
fn decode_flash_mode(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Auto".to_string(),
        2 => "On".to_string(),
        3 => "Red-eye Reduction".to_string(),
        4 => "Slow Sync".to_string(),
        5 => "Auto + Red-eye Reduction".to_string(),
        6 => "On + Red-eye Reduction".to_string(),
        16 => "External Flash".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon drive mode to human-readable string
fn decode_drive_mode(value: i16) -> String {
    match value {
        0 => "Single".to_string(),
        1 => "Continuous".to_string(),
        2 => "Movie".to_string(),
        4 => "Continuous, Speed Priority".to_string(),
        5 => "Continuous, Low".to_string(),
        6 => "Continuous, High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon focus mode to human-readable string
fn decode_focus_mode(value: i16) -> String {
    match value {
        0 => "One-shot AF".to_string(),
        1 => "AI Servo AF".to_string(),
        2 => "AI Focus AF".to_string(),
        3 => "Manual Focus (3)".to_string(),
        4 => "Single".to_string(),
        5 => "Continuous".to_string(),
        6 => "Manual Focus (6)".to_string(),
        16 => "Pan Focus".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon metering mode to human-readable string
fn decode_metering_mode(value: i16) -> String {
    match value {
        3 => "Evaluative".to_string(),
        4 => "Partial".to_string(),
        5 => "Center-weighted Average".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon exposure mode to human-readable string
fn decode_exposure_mode(value: i16) -> String {
    match value {
        0 => "Easy".to_string(),
        1 => "Program AE".to_string(),
        2 => "Shutter Priority".to_string(),
        3 => "Aperture Priority".to_string(),
        4 => "Manual".to_string(),
        5 => "Depth-of-field AE".to_string(),
        6 => "M-Dep".to_string(),
        7 => "Bulb".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

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

    // Extract values from entries
    for entry in entries {
        match entry.tag_id {
            // Simple string tags (Phase 1)
            CANON_IMAGE_TYPE | CANON_FIRMWARE_VERSION | CANON_OWNER_NAME | CANON_SERIAL_NUMBER => {
                if let Some(value) = extract_string_value(&entry, data) {
                    let tag_name = canon_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, value);
                }
            }

            // Simple integer tags (Phase 1)
            CANON_MODEL_ID | CANON_FILE_NUMBER => {
                if let Some(value) = extract_integer_value(&entry) {
                    let tag_name = canon_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, value);
                }
            }

            // CameraSettings array (Phase 2)
            CANON_CAMERA_SETTINGS => {
                if let Some(array) = extract_i16_array(&entry, data, byte_order) {
                    // Extract specific settings from array
                    if array.len() > CAMERA_SETTINGS_MACRO_MODE {
                        tags.insert(
                            "Canon:MacroMode".to_string(),
                            decode_macro_mode(array[CAMERA_SETTINGS_MACRO_MODE]),
                        );
                    }
                    if array.len() > CAMERA_SETTINGS_QUALITY {
                        tags.insert(
                            "Canon:Quality".to_string(),
                            decode_quality(array[CAMERA_SETTINGS_QUALITY]),
                        );
                    }
                    if array.len() > CAMERA_SETTINGS_FLASH_MODE {
                        tags.insert(
                            "Canon:FlashMode".to_string(),
                            decode_flash_mode(array[CAMERA_SETTINGS_FLASH_MODE]),
                        );
                    }
                    if array.len() > CAMERA_SETTINGS_DRIVE_MODE {
                        tags.insert(
                            "Canon:DriveMode".to_string(),
                            decode_drive_mode(array[CAMERA_SETTINGS_DRIVE_MODE]),
                        );
                    }
                    if array.len() > CAMERA_SETTINGS_FOCUS_MODE {
                        tags.insert(
                            "Canon:FocusMode".to_string(),
                            decode_focus_mode(array[CAMERA_SETTINGS_FOCUS_MODE]),
                        );
                    }
                    if array.len() > CAMERA_SETTINGS_ISO {
                        tags.insert(
                            "Canon:ISO".to_string(),
                            array[CAMERA_SETTINGS_ISO].to_string(),
                        );
                    }
                    if array.len() > CAMERA_SETTINGS_METERING_MODE {
                        tags.insert(
                            "Canon:MeteringMode".to_string(),
                            decode_metering_mode(array[CAMERA_SETTINGS_METERING_MODE]),
                        );
                    }
                    if array.len() > CAMERA_SETTINGS_EXPOSURE_MODE {
                        tags.insert(
                            "Canon:ExposureMode".to_string(),
                            decode_exposure_mode(array[CAMERA_SETTINGS_EXPOSURE_MODE]),
                        );
                    }
                }
            }

            // ShotInfo array (Phase 2)
            CANON_SHOT_INFO => {
                if let Some(array) = extract_i16_array(&entry, data, byte_order) {
                    if array.len() > SHOT_INFO_AUTO_ISO {
                        tags.insert(
                            "Canon:AutoISO".to_string(),
                            array[SHOT_INFO_AUTO_ISO].to_string(),
                        );
                    }
                    if array.len() > SHOT_INFO_BASE_ISO {
                        tags.insert(
                            "Canon:BaseISO".to_string(),
                            array[SHOT_INFO_BASE_ISO].to_string(),
                        );
                    }
                    if array.len() > SHOT_INFO_MEASURED_EV {
                        tags.insert(
                            "Canon:MeasuredEV".to_string(),
                            array[SHOT_INFO_MEASURED_EV].to_string(),
                        );
                    }
                    if array.len() > SHOT_INFO_TARGET_APERTURE {
                        tags.insert(
                            "Canon:TargetAperture".to_string(),
                            array[SHOT_INFO_TARGET_APERTURE].to_string(),
                        );
                    }
                    if array.len() > SHOT_INFO_TARGET_SHUTTER_SPEED {
                        tags.insert(
                            "Canon:TargetShutterSpeed".to_string(),
                            array[SHOT_INFO_TARGET_SHUTTER_SPEED].to_string(),
                        );
                    }
                    if array.len() > SHOT_INFO_SUBJECT_DISTANCE {
                        let distance = array[SHOT_INFO_SUBJECT_DISTANCE];
                        tags.insert(
                            "Canon:SubjectDistance".to_string(),
                            format!("{} mm", distance),
                        );
                    }
                }
            }

            // FocalLength array (Phase 2)
            CANON_FOCAL_LENGTH => {
                if let Some(array) = extract_i16_array(&entry, data, byte_order) {
                    // array[0] = focal type
                    // array[1] = focal length
                    if !array.is_empty() {
                        tags.insert("Canon:FocalType".to_string(), array[0].to_string());
                    }
                    if array.len() > 1 {
                        tags.insert("Canon:FocalLength".to_string(), format!("{} mm", array[1]));
                    }
                }
            }

            // Other array tags - skip for now (will add in future phases)
            _ => continue,
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

    #[test]
    fn test_camera_settings_indices() {
        // Verify key CameraSettings array indices are defined correctly
        assert_eq!(CAMERA_SETTINGS_MACRO_MODE, 1);
        assert_eq!(CAMERA_SETTINGS_SELF_TIMER, 2);
        assert_eq!(CAMERA_SETTINGS_QUALITY, 3);
        assert_eq!(CAMERA_SETTINGS_FLASH_MODE, 4);
        assert_eq!(CAMERA_SETTINGS_DRIVE_MODE, 5);
        assert_eq!(CAMERA_SETTINGS_FOCUS_MODE, 7);
        assert_eq!(CAMERA_SETTINGS_IMAGE_SIZE, 10);
        assert_eq!(CAMERA_SETTINGS_EASY_MODE, 11);
        assert_eq!(CAMERA_SETTINGS_CONTRAST, 13);
        assert_eq!(CAMERA_SETTINGS_SATURATION, 14);
        assert_eq!(CAMERA_SETTINGS_SHARPNESS, 15);
        assert_eq!(CAMERA_SETTINGS_ISO, 16);
        assert_eq!(CAMERA_SETTINGS_METERING_MODE, 17);
        assert_eq!(CAMERA_SETTINGS_FOCUS_TYPE, 18);
        assert_eq!(CAMERA_SETTINGS_AF_POINT, 19);
        assert_eq!(CAMERA_SETTINGS_EXPOSURE_MODE, 20);
        assert_eq!(CAMERA_SETTINGS_FLASH_ACTIVITY, 28);
        assert_eq!(CAMERA_SETTINGS_FOCUS_CONTINUOUS, 32);
    }

    #[test]
    fn test_decode_macro_mode() {
        assert_eq!(decode_macro_mode(1), "Macro");
        assert_eq!(decode_macro_mode(2), "Normal");
        assert_eq!(decode_macro_mode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(decode_quality(2), "Normal");
        assert_eq!(decode_quality(3), "Fine");
        assert_eq!(decode_quality(5), "Superfine");
        assert_eq!(decode_quality(130), "Normal Movie");
        assert_eq!(decode_quality(131), "Movie (2)");
        assert_eq!(decode_quality(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_flash_mode() {
        assert_eq!(decode_flash_mode(0), "Off");
        assert_eq!(decode_flash_mode(1), "Auto");
        assert_eq!(decode_flash_mode(2), "On");
        assert_eq!(decode_flash_mode(3), "Red-eye Reduction");
        assert_eq!(decode_flash_mode(4), "Slow Sync");
        assert_eq!(decode_flash_mode(5), "Auto + Red-eye Reduction");
        assert_eq!(decode_flash_mode(6), "On + Red-eye Reduction");
        assert_eq!(decode_flash_mode(16), "External Flash");
        assert_eq!(decode_flash_mode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_drive_mode() {
        assert_eq!(decode_drive_mode(0), "Single");
        assert_eq!(decode_drive_mode(1), "Continuous");
        assert_eq!(decode_drive_mode(2), "Movie");
        assert_eq!(decode_drive_mode(4), "Continuous, Speed Priority");
        assert_eq!(decode_drive_mode(5), "Continuous, Low");
        assert_eq!(decode_drive_mode(6), "Continuous, High");
        assert_eq!(decode_drive_mode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(0), "One-shot AF");
        assert_eq!(decode_focus_mode(1), "AI Servo AF");
        assert_eq!(decode_focus_mode(2), "AI Focus AF");
        assert_eq!(decode_focus_mode(3), "Manual Focus (3)");
        assert_eq!(decode_focus_mode(4), "Single");
        assert_eq!(decode_focus_mode(5), "Continuous");
        assert_eq!(decode_focus_mode(6), "Manual Focus (6)");
        assert_eq!(decode_focus_mode(16), "Pan Focus");
        assert_eq!(decode_focus_mode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_metering_mode() {
        assert_eq!(decode_metering_mode(3), "Evaluative");
        assert_eq!(decode_metering_mode(4), "Partial");
        assert_eq!(decode_metering_mode(5), "Center-weighted Average");
        assert_eq!(decode_metering_mode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_exposure_mode() {
        assert_eq!(decode_exposure_mode(0), "Easy");
        assert_eq!(decode_exposure_mode(1), "Program AE");
        assert_eq!(decode_exposure_mode(2), "Shutter Priority");
        assert_eq!(decode_exposure_mode(3), "Aperture Priority");
        assert_eq!(decode_exposure_mode(4), "Manual");
        assert_eq!(decode_exposure_mode(5), "Depth-of-field AE");
        assert_eq!(decode_exposure_mode(6), "M-Dep");
        assert_eq!(decode_exposure_mode(7), "Bulb");
        assert_eq!(decode_exposure_mode(99), "Unknown (99)");
    }

    #[test]
    fn test_parse_camera_settings_array() {
        // Create Canon MakerNote with CameraSettings array
        let mut data = Vec::new();

        // Canon signature
        data.extend_from_slice(b"Canon");

        // IFD: 1 entry (CameraSettings)
        data.extend_from_slice(&[0x01, 0x00]); // Entry count (LE)

        // IFD Entry for CameraSettings (tag 0x0001)
        data.extend_from_slice(&[0x01, 0x00]); // Tag: CameraSettings
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x15, 0x00, 0x00, 0x00]); // Count: 21 values
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23 (5 sig + 2 count + 12 entry + 4 next = 23)

        // Next IFD offset
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        // CameraSettings array data at offset 20 (21 i16 values)
        let settings: Vec<i16> = vec![
            21, // [0] Array length
            2,  // [1] Macro mode: Normal
            0,  // [2] Self-timer: Off
            3,  // [3] Quality: Fine
            2,  // [4] Flash mode: On
            0,  // [5] Drive mode: Single
            0,  // [6] (unused)
            0,  // [7] Focus mode: One-shot AF
            0,  // [8] (unused)
            0,  // [9] (unused)
            1,  // [10] Image size: Large
            0,  // [11] Easy mode: Off
            0,  // [12] (unused)
            0,  // [13] Contrast: Normal
            0,  // [14] Saturation: Normal
            0,  // [15] Sharpness: Normal
            80, // [16] ISO: 80
            3,  // [17] Metering mode: Evaluative
            0,  // [18] Focus type
            0,  // [19] AF point
            1,  // [20] Exposure mode: Program AE
        ];

        for value in settings {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote(&data, ByteOrder::LittleEndian).unwrap();

        // Verify extracted values
        assert_eq!(result.get("Canon:MacroMode"), Some(&"Normal".to_string()));
        assert_eq!(result.get("Canon:Quality"), Some(&"Fine".to_string()));
        assert_eq!(result.get("Canon:FlashMode"), Some(&"On".to_string()));
        assert_eq!(result.get("Canon:DriveMode"), Some(&"Single".to_string()));
        assert_eq!(
            result.get("Canon:FocusMode"),
            Some(&"One-shot AF".to_string())
        );
        assert_eq!(
            result.get("Canon:MeteringMode"),
            Some(&"Evaluative".to_string())
        );
        assert_eq!(
            result.get("Canon:ExposureMode"),
            Some(&"Program AE".to_string())
        );
        assert_eq!(result.get("Canon:ISO"), Some(&"80".to_string()));
    }

    #[test]
    fn test_shot_info_indices() {
        assert_eq!(SHOT_INFO_AUTO_ISO, 1);
        assert_eq!(SHOT_INFO_BASE_ISO, 2);
        assert_eq!(SHOT_INFO_MEASURED_EV, 3);
        assert_eq!(SHOT_INFO_TARGET_APERTURE, 4);
        assert_eq!(SHOT_INFO_TARGET_SHUTTER_SPEED, 5);
        assert_eq!(SHOT_INFO_WHITE_BALANCE, 7);
        assert_eq!(SHOT_INFO_SLOW_SHUTTER, 8);
        assert_eq!(SHOT_INFO_SEQUENCE_NUMBER, 9);
        assert_eq!(SHOT_INFO_FLASH_GUIDE_NUMBER, 13);
        assert_eq!(SHOT_INFO_AF_POINTS_USED, 14);
        assert_eq!(SHOT_INFO_FLASH_EXPOSURE_COMP, 15);
        assert_eq!(SHOT_INFO_AUTO_EXPOSURE_BRACKETING, 16);
        assert_eq!(SHOT_INFO_SUBJECT_DISTANCE, 19);
    }

    #[test]
    fn test_parse_shot_info_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // ShotInfo tag (0x0004)
        data.extend_from_slice(&[0x04, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Count: 20
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // ShotInfo array (20 values)
        let shot_info: Vec<i16> = vec![
            20,  // [0] Array length
            100, // [1] Auto ISO
            100, // [2] Base ISO
            128, // [3] Measured EV
            160, // [4] Target aperture (f/5.6)
            96,  // [5] Target shutter speed (1/60)
            0,   // [6] (unused)
            0,   // [7] White balance: Auto
            0,   // [8] Slow shutter: Off
            0,   // [9] Sequence number
            0, 0, 0, 0, // [10-13]
            0, // [14] AF points used
            0, // [15] Flash exposure comp
            0, // [16] Auto exposure bracketing
            0, 0,    // [17-18]
            1000, // [19] Subject distance (mm)
        ];

        for value in shot_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(result.get("Canon:AutoISO"), Some(&"100".to_string()));
        assert_eq!(result.get("Canon:BaseISO"), Some(&"100".to_string()));
        assert_eq!(result.get("Canon:MeasuredEV"), Some(&"128".to_string()));
        assert_eq!(result.get("Canon:TargetAperture"), Some(&"160".to_string()));
        assert_eq!(
            result.get("Canon:TargetShutterSpeed"),
            Some(&"96".to_string())
        );
        assert_eq!(
            result.get("Canon:SubjectDistance"),
            Some(&"1000 mm".to_string())
        );
    }

    #[test]
    fn test_parse_focal_length_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // FocalLength tag (0x0002)
        data.extend_from_slice(&[0x02, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // FocalLength array: [focal_type, focal_length, focal_plane_x_size, focal_plane_y_size]
        // focal_type: 2 (35mm equivalent available)
        // focal_length: 50mm (stored as 50)
        // focal_units: typically stored separately
        let focal_data: Vec<i16> = vec![2, 50, 0, 0];

        for value in focal_data {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(result.get("Canon:FocalType"), Some(&"2".to_string()));
        assert_eq!(result.get("Canon:FocalLength"), Some(&"50 mm".to_string()));
    }
}
