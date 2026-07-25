//! Olympus CameraSettings parser
//!
//! Parses Olympus-specific camera settings from MakerNote data.
//! These settings contain detailed exposure, focus, and image quality
//! information specific to Olympus cameras.
//!
//! ## Supported Settings
//! - ExposureMode: Manual, Program, Aperture Priority, Shutter Priority, etc.
//! - MeteringMode: Center Weighted, Spot, ESP (Evaluative)
//! - MacroMode: Off, On, Super Macro
//! - FocusMode: Single AF, Continuous AF, Manual Focus, etc.
//! - WhiteBalance: Auto, various color temperatures, custom presets
//! - ImageQuality: SQ, HQ, SHQ, RAW
//! - ImageSize: Various resolution presets
//!
//! ## Data Format
//! Camera settings are stored as an IFD-like structure within the MakerNote.
//! Values are extracted based on tag IDs within this sub-structure.
//!
//! Based on ExifTool's Olympus.pm tag definitions.

#![allow(dead_code)]

use crate::core::{MetadataMap, TagValue};

// ============================================================================
// Tag ID Constants for CameraSettings IFD
// ============================================================================

/// Exposure mode tag (0x0200 in CameraSettings sub-IFD)
const TAG_EXPOSURE_MODE: u16 = 0x0200;
/// Metering mode tag (0x0202 in CameraSettings sub-IFD)
const TAG_METERING_MODE: u16 = 0x0202;
/// Macro mode tag (0x0300 in CameraSettings sub-IFD)
const TAG_MACRO_MODE: u16 = 0x0300;
/// Focus mode tag (0x0301 in CameraSettings sub-IFD)
const TAG_FOCUS_MODE: u16 = 0x0301;
/// White balance tag (0x0500 in CameraSettings sub-IFD)
const TAG_WHITE_BALANCE: u16 = 0x0500;
/// Image quality tag (0x0201 in main MakerNote)
const TAG_IMAGE_QUALITY: u16 = 0x0201;
/// Image size tag
const TAG_IMAGE_SIZE: u16 = 0x0103;

// ============================================================================
// Value Decoders
// ============================================================================

/// Decodes the exposure mode value to a human-readable string.
///
/// # Arguments
/// * `value` - Raw exposure mode value from the tag
///
/// # Returns
/// Human-readable exposure mode description
fn decode_exposure_mode(value: u16) -> &'static str {
    match value {
        1 => "Manual",
        2 => "Program",
        3 => "Aperture Priority",
        4 => "Shutter Priority",
        5 => "Program Shift",
        6 => "Auto Bracketing",
        _ => "Unknown",
    }
}

/// Decodes the metering mode value to a human-readable string.
///
/// Olympus cameras support several metering modes for exposure calculation.
///
/// # Arguments
/// * `value` - Raw metering mode value from the tag
///
/// # Returns
/// Human-readable metering mode description
fn decode_metering_mode(value: u16) -> &'static str {
    match value {
        2 => "Center Weighted",
        3 => "Spot",
        5 => "ESP (Evaluative)",
        261 => "Pattern+AF",
        515 => "Spot+Highlight Control",
        1027 => "Spot+Shadow Control",
        _ => "Unknown",
    }
}

/// Decodes the macro mode value to a human-readable string.
///
/// # Arguments
/// * `value` - Raw macro mode value from the tag
///
/// # Returns
/// Human-readable macro mode description
fn decode_macro_mode(value: u16) -> &'static str {
    match value {
        0 => "Off",
        1 => "On",
        2 => "Super Macro",
        _ => "Unknown",
    }
}

/// Decodes the focus mode value to a human-readable string.
///
/// Olympus cameras support various autofocus and manual focus modes.
///
/// # Arguments
/// * `value` - Raw focus mode value from the tag
///
/// # Returns
/// Human-readable focus mode description
fn decode_focus_mode(value: u16) -> &'static str {
    match value {
        0 => "Single AF",
        1 => "Sequential Shooting AF",
        2 => "Continuous AF",
        3 => "Manual Focus",
        4 => "Super AF",
        5 => "AF-C",
        10 => "MF",
        _ => "Unknown",
    }
}

/// Decodes the white balance value to a human-readable string.
///
/// Olympus cameras support many white balance presets including
/// color temperature-based settings and custom presets.
///
/// # Arguments
/// * `value` - Raw white balance value from the tag
///
/// # Returns
/// Human-readable white balance description
fn decode_white_balance(value: u16) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Auto (Keep Warm Color Off)",
        16 => "7500K (Fine Weather with Shade)",
        17 => "6000K (Cloudy)",
        18 => "5300K (Fine Weather)",
        20 => "3000K (Tungsten)",
        21 => "3600K (Evening Sunlight)",
        22 => "Auto Setup",
        23 => "5500K (Flash)",
        33 => "6600K (Daylight Fluorescent)",
        34 => "4500K (Neutral White Fluorescent)",
        35 => "4000K (Cool White Fluorescent)",
        36 => "White Fluorescent",
        48 => "3600K (Tungsten)",
        67 => "Underwater",
        256 => "One Touch WB 1",
        257 => "One Touch WB 2",
        258 => "One Touch WB 3",
        259 => "One Touch WB 4",
        512 => "Custom WB 1",
        513 => "Custom WB 2",
        514 => "Custom WB 3",
        515 => "Custom WB 4",
        _ => "Unknown",
    }
}

/// Decodes the image quality value to a human-readable string.
///
/// Olympus cameras encode quality settings as numeric values.
///
/// # Arguments
/// * `value` - Raw image quality value from the tag
///
/// # Returns
/// Human-readable image quality description
fn decode_image_quality(value: u16) -> &'static str {
    match value {
        1 => "SQ (Standard Quality)",
        2 => "HQ (High Quality)",
        3 => "SHQ (Super High Quality)",
        4 => "RAW",
        5 => "SQ (Low)",
        6 => "SQ (Medium)",
        _ => "Unknown",
    }
}

/// Decodes the image size value to a human-readable string.
///
/// # Arguments
/// * `value` - Raw image size value from the tag
///
/// # Returns
/// Human-readable image size description
fn decode_image_size(value: u16) -> &'static str {
    match value {
        0 => "2560x1920",  // Full
        1 => "1600x1200",  // 1600
        2 => "1280x960",   // 1280
        3 => "640x480",    // 640
        4 => "3200x2400",  // 3200 (E-1)
        5 => "4080x3048",  // Super Large
        22 => "2048x1536", // 2048
        _ => "Unknown",
    }
}

// ============================================================================
// Byte Reading Utilities
// ============================================================================

/// Reads a u16 value from a byte slice at the specified offset.
///
/// # Arguments
/// * `data` - Source byte slice
/// * `offset` - Byte offset to read from
/// * `big_endian` - If true, read as big-endian; if false, read as little-endian
///
/// # Returns
/// The u16 value if the read is within bounds, None otherwise
#[inline]
fn read_u16(data: &[u8], offset: usize, big_endian: bool) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }

    let bytes = [data[offset], data[offset + 1]];
    if big_endian {
        Some(u16::from_be_bytes(bytes))
    } else {
        Some(u16::from_le_bytes(bytes))
    }
}

/// Reads a u32 value from a byte slice at the specified offset.
///
/// # Arguments
/// * `data` - Source byte slice
/// * `offset` - Byte offset to read from
/// * `big_endian` - If true, read as big-endian; if false, read as little-endian
///
/// # Returns
/// The u32 value if the read is within bounds, None otherwise
#[inline]
fn read_u32(data: &[u8], offset: usize, big_endian: bool) -> Option<u32> {
    if offset + 4 > data.len() {
        return None;
    }

    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ];
    if big_endian {
        Some(u32::from_be_bytes(bytes))
    } else {
        Some(u32::from_le_bytes(bytes))
    }
}

// ============================================================================
// IFD Entry Structure
// ============================================================================

/// Represents a single IFD entry parsed from the camera settings data.
///
/// Each entry contains:
/// - tag_id: 2-byte identifier for the setting
/// - field_type: 2-byte type indicator (1=BYTE, 2=ASCII, 3=SHORT, 4=LONG, etc.)
/// - value_count: Number of values
/// - value_offset: Either the value itself (if small) or offset to value data
#[derive(Debug, Clone)]
struct IfdEntry {
    tag_id: u16,
    field_type: u16,
    value_count: u32,
    value_offset: u32,
}

/// Parses IFD entries from the camera settings data.
///
/// # Arguments
/// * `data` - Raw camera settings byte data
/// * `big_endian` - Byte order flag (true = big-endian, false = little-endian)
///
/// # Returns
/// Vector of parsed IFD entries, or empty vector if parsing fails
fn parse_ifd_entries(data: &[u8], big_endian: bool) -> Vec<IfdEntry> {
    let mut entries = Vec::new();

    // Need at least 2 bytes for entry count
    if data.len() < 2 {
        return entries;
    }

    // Read entry count from first 2 bytes
    let entry_count = match read_u16(data, 0, big_endian) {
        Some(count) => count as usize,
        None => return entries,
    };

    // Sanity check: avoid parsing huge numbers of entries
    if entry_count > 500 || entry_count == 0 {
        return entries;
    }

    // Each IFD entry is 12 bytes, starting at offset 2
    let entries_start = 2;
    let required_len = entries_start + (entry_count * 12);

    if data.len() < required_len {
        return entries;
    }

    // Parse each entry
    for i in 0..entry_count {
        let entry_offset = entries_start + (i * 12);

        let tag_id = match read_u16(data, entry_offset, big_endian) {
            Some(v) => v,
            None => continue,
        };
        let field_type = match read_u16(data, entry_offset + 2, big_endian) {
            Some(v) => v,
            None => continue,
        };
        let value_count = match read_u32(data, entry_offset + 4, big_endian) {
            Some(v) => v,
            None => continue,
        };
        let value_offset = match read_u32(data, entry_offset + 8, big_endian) {
            Some(v) => v,
            None => continue,
        };

        entries.push(IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        });
    }

    entries
}

/// Extracts a u16 value from an IFD entry.
///
/// For SHORT (type 3) fields with count 1, the value is stored
/// directly in the value_offset field (lower 16 bits for LE).
///
/// # Arguments
/// * `entry` - The IFD entry to extract from
/// * `big_endian` - Byte order flag
///
/// # Returns
/// The u16 value if applicable, None otherwise
fn extract_u16_value(entry: &IfdEntry, big_endian: bool) -> Option<u16> {
    // Type 3 = SHORT (u16), count must be 1 for direct value
    if entry.field_type != 3 || entry.value_count != 1 {
        return None;
    }

    // Value is stored in the offset field; extract correctly based on endianness
    if big_endian {
        // For big-endian, value is in upper 16 bits
        Some(((entry.value_offset >> 16) & 0xFFFF) as u16)
    } else {
        // For little-endian, value is in lower 16 bits
        Some((entry.value_offset & 0xFFFF) as u16)
    }
}

// ============================================================================
// Main Parser Function
// ============================================================================

/// Parses Olympus camera settings from raw MakerNote data.
///
/// This function extracts key camera settings from Olympus MakerNote data,
/// including exposure mode, metering mode, macro mode, focus mode,
/// white balance, image quality, and image size.
///
/// # Arguments
/// * `data` - Raw byte slice containing the camera settings IFD data
/// * `byte_order` - Byte order flag: true for big-endian, false for little-endian
///
/// # Returns
/// A `MetadataMap` containing the extracted camera settings with keys prefixed
/// by "Olympus:" (e.g., "Olympus:ExposureMode", "Olympus:FocusMode")
///
/// # Examples
///
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::olympus::camera_settings::parse_olympus_camera_settings;
///
/// let raw_data: &[u8] = &[/* camera settings bytes */];
/// let metadata = parse_olympus_camera_settings(raw_data, false);
///
/// if let Some(exposure_mode) = metadata.get_string("Olympus:ExposureMode") {
///     println!("Exposure Mode: {}", exposure_mode);
/// }
/// ```
///
/// # Tag Output
/// The following tags may be present in the returned MetadataMap:
/// - `Olympus:ExposureMode` - Camera exposure mode setting
/// - `Olympus:MeteringMode` - Light metering method
/// - `Olympus:MacroMode` - Macro/close-up mode status
/// - `Olympus:FocusMode` - Autofocus or manual focus mode
/// - `Olympus:WhiteBalance` - White balance preset or color temperature
/// - `Olympus:ImageQuality` - JPEG quality or RAW format
/// - `Olympus:ImageSize` - Image resolution preset
pub fn parse_olympus_camera_settings(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Parse IFD entries from the data
    let entries = parse_ifd_entries(data, byte_order);

    // Process each entry and extract relevant camera settings
    for entry in entries {
        // Extract u16 value if applicable
        if let Some(value) = extract_u16_value(&entry, byte_order) {
            match entry.tag_id {
                TAG_EXPOSURE_MODE => {
                    let decoded = decode_exposure_mode(value);
                    metadata.insert(
                        "Olympus:ExposureMode",
                        TagValue::new_string(decoded.to_string()),
                    );
                }
                TAG_METERING_MODE => {
                    let decoded = decode_metering_mode(value);
                    metadata.insert(
                        "Olympus:MeteringMode",
                        TagValue::new_string(decoded.to_string()),
                    );
                }
                TAG_MACRO_MODE => {
                    let decoded = decode_macro_mode(value);
                    metadata.insert(
                        "Olympus:MacroMode",
                        TagValue::new_string(decoded.to_string()),
                    );
                }
                TAG_FOCUS_MODE => {
                    let decoded = decode_focus_mode(value);
                    metadata.insert(
                        "Olympus:FocusMode",
                        TagValue::new_string(decoded.to_string()),
                    );
                }
                TAG_WHITE_BALANCE => {
                    let decoded = decode_white_balance(value);
                    metadata.insert(
                        "Olympus:WhiteBalance",
                        TagValue::new_string(decoded.to_string()),
                    );
                }
                TAG_IMAGE_QUALITY => {
                    let decoded = decode_image_quality(value);
                    metadata.insert(
                        "Olympus:ImageQuality",
                        TagValue::new_string(decoded.to_string()),
                    );
                }
                TAG_IMAGE_SIZE => {
                    let decoded = decode_image_size(value);
                    metadata.insert(
                        "Olympus:ImageSize",
                        TagValue::new_string(decoded.to_string()),
                    );
                }
                _ => {
                    // Other tags are not processed in this focused parser
                }
            }
        }
    }

    metadata
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Decoder Tests
    // ========================================================================

    #[test]
    fn test_decode_exposure_mode() {
        assert_eq!(decode_exposure_mode(1), "Manual");
        assert_eq!(decode_exposure_mode(2), "Program");
        assert_eq!(decode_exposure_mode(3), "Aperture Priority");
        assert_eq!(decode_exposure_mode(4), "Shutter Priority");
        assert_eq!(decode_exposure_mode(5), "Program Shift");
        assert_eq!(decode_exposure_mode(6), "Auto Bracketing");
        assert_eq!(decode_exposure_mode(255), "Unknown");
    }

    #[test]
    fn test_decode_metering_mode() {
        assert_eq!(decode_metering_mode(2), "Center Weighted");
        assert_eq!(decode_metering_mode(3), "Spot");
        assert_eq!(decode_metering_mode(5), "ESP (Evaluative)");
        assert_eq!(decode_metering_mode(261), "Pattern+AF");
        assert_eq!(decode_metering_mode(999), "Unknown");
    }

    #[test]
    fn test_decode_macro_mode() {
        assert_eq!(decode_macro_mode(0), "Off");
        assert_eq!(decode_macro_mode(1), "On");
        assert_eq!(decode_macro_mode(2), "Super Macro");
        assert_eq!(decode_macro_mode(99), "Unknown");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(0), "Single AF");
        assert_eq!(decode_focus_mode(1), "Sequential Shooting AF");
        assert_eq!(decode_focus_mode(2), "Continuous AF");
        assert_eq!(decode_focus_mode(3), "Manual Focus");
        assert_eq!(decode_focus_mode(4), "Super AF");
        assert_eq!(decode_focus_mode(5), "AF-C");
        assert_eq!(decode_focus_mode(10), "MF");
        assert_eq!(decode_focus_mode(255), "Unknown");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0), "Auto");
        assert_eq!(decode_white_balance(18), "5300K (Fine Weather)");
        assert_eq!(decode_white_balance(23), "5500K (Flash)");
        assert_eq!(decode_white_balance(67), "Underwater");
        assert_eq!(decode_white_balance(256), "One Touch WB 1");
        assert_eq!(decode_white_balance(512), "Custom WB 1");
        assert_eq!(decode_white_balance(9999), "Unknown");
    }

    #[test]
    fn test_decode_image_quality() {
        assert_eq!(decode_image_quality(1), "SQ (Standard Quality)");
        assert_eq!(decode_image_quality(2), "HQ (High Quality)");
        assert_eq!(decode_image_quality(3), "SHQ (Super High Quality)");
        assert_eq!(decode_image_quality(4), "RAW");
        assert_eq!(decode_image_quality(5), "SQ (Low)");
        assert_eq!(decode_image_quality(6), "SQ (Medium)");
        assert_eq!(decode_image_quality(99), "Unknown");
    }

    #[test]
    fn test_decode_image_size() {
        assert_eq!(decode_image_size(0), "2560x1920");
        assert_eq!(decode_image_size(1), "1600x1200");
        assert_eq!(decode_image_size(2), "1280x960");
        assert_eq!(decode_image_size(3), "640x480");
        assert_eq!(decode_image_size(4), "3200x2400");
        assert_eq!(decode_image_size(5), "4080x3048");
        assert_eq!(decode_image_size(22), "2048x1536");
        assert_eq!(decode_image_size(100), "Unknown");
    }

    // ========================================================================
    // Byte Reading Tests
    // ========================================================================

    #[test]
    fn test_read_u16_little_endian() {
        let data = [0x01, 0x02, 0x03, 0x04];
        // Little-endian: 0x01, 0x02 -> 0x0201
        assert_eq!(read_u16(&data, 0, false), Some(0x0201));
        // Little-endian: 0x03, 0x04 -> 0x0403
        assert_eq!(read_u16(&data, 2, false), Some(0x0403));
    }

    #[test]
    fn test_read_u16_big_endian() {
        let data = [0x01, 0x02, 0x03, 0x04];
        // Big-endian: 0x01, 0x02 -> 0x0102
        assert_eq!(read_u16(&data, 0, true), Some(0x0102));
        // Big-endian: 0x03, 0x04 -> 0x0304
        assert_eq!(read_u16(&data, 2, true), Some(0x0304));
    }

    #[test]
    fn test_read_u16_out_of_bounds() {
        let data = [0x01];
        assert_eq!(read_u16(&data, 0, false), None);
        assert_eq!(read_u16(&data, 0, true), None);
    }

    #[test]
    fn test_read_u32_little_endian() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        // Little-endian: 0x01, 0x02, 0x03, 0x04 -> 0x04030201
        assert_eq!(read_u32(&data, 0, false), Some(0x04030201));
    }

    #[test]
    fn test_read_u32_big_endian() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        // Big-endian: 0x01, 0x02, 0x03, 0x04 -> 0x01020304
        assert_eq!(read_u32(&data, 0, true), Some(0x01020304));
    }

    #[test]
    fn test_read_u32_out_of_bounds() {
        let data = [0x01, 0x02, 0x03];
        assert_eq!(read_u32(&data, 0, false), None);
    }

    // ========================================================================
    // IFD Parsing Tests
    // ========================================================================

    #[test]
    fn test_parse_ifd_entries_empty_data() {
        let entries = parse_ifd_entries(&[], false);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_ifd_entries_insufficient_data() {
        // Only 1 byte, need at least 2 for entry count
        let entries = parse_ifd_entries(&[0x01], false);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_ifd_entries_zero_count() {
        // Entry count = 0 (little-endian)
        let data = [0x00, 0x00];
        let entries = parse_ifd_entries(&data, false);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_ifd_entries_single_entry_le() {
        // Build a minimal IFD with 1 entry (little-endian)
        // Entry count: 1 (0x0001 LE = [0x01, 0x00])
        // Entry: tag=0x0200, type=3 (SHORT), count=1, value=2 (Program mode)
        let mut data = Vec::new();

        // Entry count (LE)
        data.extend_from_slice(&[0x01, 0x00]);

        // Tag ID (0x0200 LE = [0x00, 0x02])
        data.extend_from_slice(&[0x00, 0x02]);
        // Field type (3 = SHORT, LE)
        data.extend_from_slice(&[0x03, 0x00]);
        // Value count (1, LE)
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        // Value (2 = Program mode, in u32 LE)
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

        let entries = parse_ifd_entries(&data, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tag_id, 0x0200);
        assert_eq!(entries[0].field_type, 3);
        assert_eq!(entries[0].value_count, 1);
        assert_eq!(entries[0].value_offset, 2);
    }

    #[test]
    fn test_parse_ifd_entries_single_entry_be() {
        // Build a minimal IFD with 1 entry (big-endian)
        let mut data = Vec::new();

        // Entry count (BE)
        data.extend_from_slice(&[0x00, 0x01]);

        // Tag ID (0x0200 BE)
        data.extend_from_slice(&[0x02, 0x00]);
        // Field type (3 = SHORT, BE)
        data.extend_from_slice(&[0x00, 0x03]);
        // Value count (1, BE)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        // Value (2 = Program mode, in u32 BE: value in upper 16 bits)
        data.extend_from_slice(&[0x00, 0x02, 0x00, 0x00]);

        let entries = parse_ifd_entries(&data, true);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tag_id, 0x0200);
        assert_eq!(entries[0].field_type, 3);
        assert_eq!(entries[0].value_count, 1);
    }

    #[test]
    fn test_extract_u16_value() {
        // Entry with field_type=3 (SHORT), value_count=1
        let entry = IfdEntry {
            tag_id: 0x0200,
            field_type: 3,
            value_count: 1,
            value_offset: 0x00020000, // Value 2 in upper 16 bits for BE
        };

        // Big-endian: value in upper 16 bits
        assert_eq!(extract_u16_value(&entry, true), Some(2));

        // Little-endian entry
        let entry_le = IfdEntry {
            tag_id: 0x0200,
            field_type: 3,
            value_count: 1,
            value_offset: 0x00000002, // Value 2 in lower 16 bits for LE
        };

        assert_eq!(extract_u16_value(&entry_le, false), Some(2));
    }

    #[test]
    fn test_extract_u16_value_wrong_type() {
        // Entry with wrong field type (1 = BYTE instead of 3 = SHORT)
        let entry = IfdEntry {
            tag_id: 0x0200,
            field_type: 1,
            value_count: 1,
            value_offset: 0x00000002,
        };

        assert_eq!(extract_u16_value(&entry, false), None);
    }

    #[test]
    fn test_extract_u16_value_wrong_count() {
        // Entry with wrong value count (2 instead of 1)
        let entry = IfdEntry {
            tag_id: 0x0200,
            field_type: 3,
            value_count: 2,
            value_offset: 0x00000002,
        };

        assert_eq!(extract_u16_value(&entry, false), None);
    }

    // ========================================================================
    // Full Parser Integration Tests
    // ========================================================================

    #[test]
    fn test_parse_olympus_camera_settings_empty() {
        let metadata = parse_olympus_camera_settings(&[], false);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_olympus_camera_settings_exposure_mode() {
        // Build IFD with exposure mode = Program (2)
        let mut data = Vec::new();

        // Entry count = 1 (LE)
        data.extend_from_slice(&[0x01, 0x00]);

        // Tag 0x0200 (ExposureMode), type 3 (SHORT), count 1, value 2 (Program)
        data.extend_from_slice(&[0x00, 0x02]); // tag
        data.extend_from_slice(&[0x03, 0x00]); // type
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // value

        let metadata = parse_olympus_camera_settings(&data, false);

        assert_eq!(metadata.get_string("Olympus:ExposureMode"), Some("Program"));
    }

    #[test]
    fn test_parse_olympus_camera_settings_multiple_tags() {
        // Build IFD with multiple camera setting tags
        let mut data = Vec::new();

        // Entry count = 3 (LE)
        data.extend_from_slice(&[0x03, 0x00]);

        // Entry 1: ExposureMode (0x0200) = 3 (Aperture Priority)
        data.extend_from_slice(&[0x00, 0x02]); // tag
        data.extend_from_slice(&[0x03, 0x00]); // type
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count
        data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // value

        // Entry 2: MacroMode (0x0300) = 1 (On)
        data.extend_from_slice(&[0x00, 0x03]); // tag
        data.extend_from_slice(&[0x03, 0x00]); // type
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // value

        // Entry 3: FocusMode (0x0301) = 2 (Continuous AF)
        data.extend_from_slice(&[0x01, 0x03]); // tag
        data.extend_from_slice(&[0x03, 0x00]); // type
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // value

        let metadata = parse_olympus_camera_settings(&data, false);

        assert_eq!(
            metadata.get_string("Olympus:ExposureMode"),
            Some("Aperture Priority")
        );
        assert_eq!(metadata.get_string("Olympus:MacroMode"), Some("On"));
        assert_eq!(
            metadata.get_string("Olympus:FocusMode"),
            Some("Continuous AF")
        );
    }

    #[test]
    fn test_parse_olympus_camera_settings_big_endian() {
        // Build IFD with metering mode in big-endian
        let mut data = Vec::new();

        // Entry count = 1 (BE)
        data.extend_from_slice(&[0x00, 0x01]);

        // Tag 0x0202 (MeteringMode), type 3, count 1, value 5 (ESP)
        data.extend_from_slice(&[0x02, 0x02]); // tag (BE)
        data.extend_from_slice(&[0x00, 0x03]); // type (BE)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // count (BE)
        data.extend_from_slice(&[0x00, 0x05, 0x00, 0x00]); // value (BE, in upper 16 bits)

        let metadata = parse_olympus_camera_settings(&data, true);

        assert_eq!(
            metadata.get_string("Olympus:MeteringMode"),
            Some("ESP (Evaluative)")
        );
    }

    #[test]
    fn test_parse_olympus_camera_settings_white_balance() {
        // Build IFD with white balance
        let mut data = Vec::new();

        // Entry count = 1 (LE)
        data.extend_from_slice(&[0x01, 0x00]);

        // Tag 0x0500 (WhiteBalance), type 3, count 1, value 0 (Auto)
        data.extend_from_slice(&[0x00, 0x05]); // tag
        data.extend_from_slice(&[0x03, 0x00]); // type
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // value

        let metadata = parse_olympus_camera_settings(&data, false);

        assert_eq!(metadata.get_string("Olympus:WhiteBalance"), Some("Auto"));
    }

    #[test]
    fn test_parse_olympus_camera_settings_image_quality() {
        // Build IFD with image quality = RAW (4)
        let mut data = Vec::new();

        // Entry count = 1 (LE)
        data.extend_from_slice(&[0x01, 0x00]);

        // Tag 0x0201 (ImageQuality), type 3, count 1, value 4 (RAW)
        data.extend_from_slice(&[0x01, 0x02]); // tag
        data.extend_from_slice(&[0x03, 0x00]); // type
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // value

        let metadata = parse_olympus_camera_settings(&data, false);

        assert_eq!(metadata.get_string("Olympus:ImageQuality"), Some("RAW"));
    }

    #[test]
    fn test_parse_olympus_camera_settings_unknown_tag() {
        // Build IFD with an unrecognized tag
        let mut data = Vec::new();

        // Entry count = 1 (LE)
        data.extend_from_slice(&[0x01, 0x00]);

        // Tag 0xFFFF (unknown), type 3, count 1, value 999
        data.extend_from_slice(&[0xFF, 0xFF]); // tag
        data.extend_from_slice(&[0x03, 0x00]); // type
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count
        data.extend_from_slice(&[0xE7, 0x03, 0x00, 0x00]); // value (999)

        let metadata = parse_olympus_camera_settings(&data, false);

        // Unknown tag should not be added to metadata
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_olympus_camera_settings_all_tags() {
        // Build IFD with all supported tags
        let mut data = Vec::new();

        // Entry count = 7 (LE)
        data.extend_from_slice(&[0x07, 0x00]);

        // ExposureMode (0x0200) = 1 (Manual)
        data.extend_from_slice(&[
            0x00, 0x02, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        ]);
        // ImageQuality (0x0201) = 3 (SHQ)
        data.extend_from_slice(&[
            0x01, 0x02, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00,
        ]);
        // MeteringMode (0x0202) = 3 (Spot)
        data.extend_from_slice(&[
            0x02, 0x02, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00,
        ]);
        // MacroMode (0x0300) = 2 (Super Macro)
        data.extend_from_slice(&[
            0x00, 0x03, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
        ]);
        // FocusMode (0x0301) = 3 (Manual Focus)
        data.extend_from_slice(&[
            0x01, 0x03, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00,
        ]);
        // ImageSize (0x0103) = 0 (2560x1920)
        data.extend_from_slice(&[
            0x03, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        // WhiteBalance (0x0500) = 18 (5300K Fine Weather)
        data.extend_from_slice(&[
            0x00, 0x05, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00,
        ]);

        let metadata = parse_olympus_camera_settings(&data, false);

        assert_eq!(metadata.len(), 7);
        assert_eq!(metadata.get_string("Olympus:ExposureMode"), Some("Manual"));
        assert_eq!(
            metadata.get_string("Olympus:ImageQuality"),
            Some("SHQ (Super High Quality)")
        );
        assert_eq!(metadata.get_string("Olympus:MeteringMode"), Some("Spot"));
        assert_eq!(
            metadata.get_string("Olympus:MacroMode"),
            Some("Super Macro")
        );
        assert_eq!(
            metadata.get_string("Olympus:FocusMode"),
            Some("Manual Focus")
        );
        assert_eq!(metadata.get_string("Olympus:ImageSize"), Some("2560x1920"));
        assert_eq!(
            metadata.get_string("Olympus:WhiteBalance"),
            Some("5300K (Fine Weather)")
        );
    }
}
