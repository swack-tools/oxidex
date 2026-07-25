//! Sony FocusInfo parser
//!
//! Parses Sony MakerNotes focus information data block, which contains
//! autofocus-related settings and status information stored in a binary format.
//!
//! ## Supported Fields
//! - **FocusMode**: Manual, AF-S, AF-C, AF-A, DMF, etc.
//! - **AFAreaMode**: Wide, Spot, Local, Flexible Spot, Zone, etc.
//! - **FocusPosition**: Distance/position value from the focus system
//! - **AFPointSelected**: The AF point currently selected by the user or camera
//! - **AFPointsUsed**: Bitmask or count of AF points actively used during focus
//!
//! ## Data Format
//! Sony FocusInfo is typically stored as a binary array within MakerNotes.
//! The exact format and offsets can vary between camera models, but this
//! parser handles the common case found in most Sony Alpha/NEX cameras.
//!
//! ## Byte Order
//! The `byte_order` parameter indicates endianness:
//! - `true` = Big-endian (Motorola byte order)
//! - `false` = Little-endian (Intel byte order)

use crate::core::MetadataMap;
use crate::core::TagValue;

// =============================================================================
// FOCUS INFO FIELD OFFSETS
// =============================================================================
// These offsets are based on common Sony FocusInfo structures.
// Sony uses different offsets depending on camera model, but these represent
// the most common layout found in Alpha and NEX series cameras.

/// Offset for FocusMode field (2 bytes, i16)
const OFFSET_FOCUS_MODE: usize = 0;

/// Offset for AFAreaMode field (2 bytes, i16)
const OFFSET_AF_AREA_MODE: usize = 2;

/// Offset for FocusPosition field (2 bytes, u16)
const OFFSET_FOCUS_POSITION: usize = 4;

/// Offset for AFPointSelected field (2 bytes, i16)
const OFFSET_AF_POINT_SELECTED: usize = 6;

/// Offset for AFPointsUsed field (4 bytes, u32 bitmask)
const OFFSET_AF_POINTS_USED: usize = 8;

/// Minimum data length required for parsing all fields
const MIN_DATA_LENGTH: usize = 12;

// =============================================================================
// VALUE DECODERS
// =============================================================================
// These functions convert raw numeric values to human-readable strings.
// Based on ExifTool Sony.pm definitions.

/// Decodes FocusMode numeric value to human-readable string.
///
/// Sony focus modes follow a consistent numbering across most camera models.
/// Values are based on ExifTool Sony.pm tag definitions.
///
/// # Arguments
/// * `value` - Raw i16 value from FocusInfo data
///
/// # Returns
/// Human-readable focus mode string
fn decode_focus_mode(value: i16) -> &'static str {
    match value {
        0 => "Manual",
        1 => "AF-S",
        2 => "AF-C",
        3 => "AF-A",
        4 => "DMF",
        5 => "AF-D",
        6 => "AF-S (Continuous)",
        _ => "Unknown",
    }
}

/// Decodes AFAreaMode numeric value to human-readable string.
///
/// Sony AF area modes determine how the camera selects focus points.
/// Values are based on ExifTool Sony.pm tag definitions.
///
/// # Arguments
/// * `value` - Raw i16 value from FocusInfo data
///
/// # Returns
/// Human-readable AF area mode string
fn decode_af_area_mode(value: i16) -> &'static str {
    match value {
        0 => "Wide",
        1 => "Spot",
        2 => "Local",
        3 => "Flexible Spot",
        4 => "Zone",
        5 => "Expand Flexible Spot",
        6 => "Lock-on AF",
        7 => "Tracking",
        8 => "Eye AF",
        9 => "Flexible Spot (Small)",
        10 => "Flexible Spot (Medium)",
        11 => "Flexible Spot (Large)",
        _ => "Unknown",
    }
}

// =============================================================================
// BYTE READING UTILITIES
// =============================================================================
// These functions handle endian-aware reading of multi-byte values.

/// Reads a 16-bit signed integer from the data buffer.
///
/// # Arguments
/// * `data` - Raw byte slice
/// * `offset` - Byte offset to read from
/// * `big_endian` - If true, reads as big-endian; otherwise little-endian
///
/// # Returns
/// The i16 value, or None if offset is out of bounds
#[inline]
fn read_i16(data: &[u8], offset: usize, big_endian: bool) -> Option<i16> {
    if offset + 2 > data.len() {
        return None;
    }

    let bytes = [data[offset], data[offset + 1]];
    Some(if big_endian {
        i16::from_be_bytes(bytes)
    } else {
        i16::from_le_bytes(bytes)
    })
}

/// Reads a 16-bit unsigned integer from the data buffer.
///
/// # Arguments
/// * `data` - Raw byte slice
/// * `offset` - Byte offset to read from
/// * `big_endian` - If true, reads as big-endian; otherwise little-endian
///
/// # Returns
/// The u16 value, or None if offset is out of bounds
#[inline]
fn read_u16(data: &[u8], offset: usize, big_endian: bool) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }

    let bytes = [data[offset], data[offset + 1]];
    Some(if big_endian {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    })
}

/// Reads a 32-bit unsigned integer from the data buffer.
///
/// # Arguments
/// * `data` - Raw byte slice
/// * `offset` - Byte offset to read from
/// * `big_endian` - If true, reads as big-endian; otherwise little-endian
///
/// # Returns
/// The u32 value, or None if offset is out of bounds
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
    Some(if big_endian {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    })
}

// =============================================================================
// MAIN PARSER FUNCTION
// =============================================================================

/// Parses Sony FocusInfo binary data into a MetadataMap.
///
/// This function extracts focus-related metadata from the Sony FocusInfo
/// data block typically found within Sony MakerNotes. The FocusInfo block
/// contains autofocus settings and status information.
///
/// # Arguments
/// * `data` - Raw byte slice containing the FocusInfo data
/// * `byte_order` - Byte order flag: `true` for big-endian, `false` for little-endian
///
/// # Returns
/// A `MetadataMap` containing the extracted focus information tags.
/// Tags are prefixed with "Sony:" to indicate their source.
///
/// # Extracted Tags
/// - `Sony:FocusMode` - Focus mode setting (Manual, AF-S, AF-C, etc.)
/// - `Sony:AFAreaMode` - AF area selection mode (Wide, Spot, Zone, etc.)
/// - `Sony:FocusPosition` - Focus position/distance value
/// - `Sony:AFPointSelected` - Currently selected AF point index
/// - `Sony:AFPointsUsed` - Bitmask of AF points used during focus
///
/// # Example
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::sony::focus_info::parse_sony_focus_info;
///
/// // Example FocusInfo data (little-endian)
/// let data = vec![
///     0x02, 0x00,  // FocusMode: AF-C (2)
///     0x03, 0x00,  // AFAreaMode: Flexible Spot (3)
///     0x64, 0x00,  // FocusPosition: 100
///     0x05, 0x00,  // AFPointSelected: 5
///     0x1F, 0x00, 0x00, 0x00,  // AFPointsUsed: 0x0000001F (5 points)
/// ];
///
/// let metadata = parse_sony_focus_info(&data, false);
/// assert_eq!(metadata.get_string("Sony:FocusMode"), Some("AF-C"));
/// ```
///
/// # Notes
/// - Returns an empty MetadataMap if data is too short (< 12 bytes)
/// - Invalid field values result in "Unknown" strings for decoded fields
/// - Numeric fields (FocusPosition, AFPointSelected, AFPointsUsed) are stored as integers
pub fn parse_sony_focus_info(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Validate minimum data length required for all fields
    if data.len() < MIN_DATA_LENGTH {
        return metadata;
    }

    // Parse FocusMode (offset 0, i16)
    // Determines how the camera acquires focus (manual, single-shot, continuous, etc.)
    if let Some(focus_mode_raw) = read_i16(data, OFFSET_FOCUS_MODE, byte_order) {
        let focus_mode_decoded = decode_focus_mode(focus_mode_raw);
        metadata.insert(
            "Sony:FocusMode",
            TagValue::new_string(focus_mode_decoded.to_string()),
        );
    }

    // Parse AFAreaMode (offset 2, i16)
    // Determines the AF area selection method (wide, spot, zone, etc.)
    if let Some(af_area_mode_raw) = read_i16(data, OFFSET_AF_AREA_MODE, byte_order) {
        let af_area_mode_decoded = decode_af_area_mode(af_area_mode_raw);
        metadata.insert(
            "Sony:AFAreaMode",
            TagValue::new_string(af_area_mode_decoded.to_string()),
        );
    }

    // Parse FocusPosition (offset 4, u16)
    // Raw focus position value from the lens/focus system
    if let Some(focus_position) = read_u16(data, OFFSET_FOCUS_POSITION, byte_order) {
        metadata.insert(
            "Sony:FocusPosition",
            TagValue::new_integer(focus_position as i64),
        );
    }

    // Parse AFPointSelected (offset 6, i16)
    // Index of the AF point selected by user or camera (-1 often means none/auto)
    if let Some(af_point_selected) = read_i16(data, OFFSET_AF_POINT_SELECTED, byte_order) {
        metadata.insert(
            "Sony:AFPointSelected",
            TagValue::new_integer(af_point_selected as i64),
        );
    }

    // Parse AFPointsUsed (offset 8, u32)
    // Bitmask indicating which AF points were used during focus acquisition
    if let Some(af_points_used) = read_u32(data, OFFSET_AF_POINTS_USED, byte_order) {
        metadata.insert(
            "Sony:AFPointsUsed",
            TagValue::new_integer(af_points_used as i64),
        );
    }

    metadata
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test parsing with little-endian byte order
    #[test]
    fn test_parse_focus_info_little_endian() {
        // Construct test data in little-endian format:
        // FocusMode: 2 (AF-C)
        // AFAreaMode: 3 (Flexible Spot)
        // FocusPosition: 100
        // AFPointSelected: 5
        // AFPointsUsed: 0x0000001F (31 = 5 lowest bits set)
        let data: Vec<u8> = vec![
            0x02, 0x00, // FocusMode: 2 (little-endian)
            0x03, 0x00, // AFAreaMode: 3 (little-endian)
            0x64, 0x00, // FocusPosition: 100 (little-endian)
            0x05, 0x00, // AFPointSelected: 5 (little-endian)
            0x1F, 0x00, 0x00, 0x00, // AFPointsUsed: 31 (little-endian)
        ];

        let metadata = parse_sony_focus_info(&data, false);

        assert_eq!(metadata.get_string("Sony:FocusMode"), Some("AF-C"));
        assert_eq!(
            metadata.get_string("Sony:AFAreaMode"),
            Some("Flexible Spot")
        );
        assert_eq!(metadata.get_integer("Sony:FocusPosition"), Some(100));
        assert_eq!(metadata.get_integer("Sony:AFPointSelected"), Some(5));
        assert_eq!(metadata.get_integer("Sony:AFPointsUsed"), Some(31));
    }

    /// Test parsing with big-endian byte order
    #[test]
    fn test_parse_focus_info_big_endian() {
        // Construct test data in big-endian format:
        // FocusMode: 1 (AF-S)
        // AFAreaMode: 0 (Wide)
        // FocusPosition: 200
        // AFPointSelected: 10
        // AFPointsUsed: 0x000000FF (255)
        let data: Vec<u8> = vec![
            0x00, 0x01, // FocusMode: 1 (big-endian)
            0x00, 0x00, // AFAreaMode: 0 (big-endian)
            0x00, 0xC8, // FocusPosition: 200 (big-endian)
            0x00, 0x0A, // AFPointSelected: 10 (big-endian)
            0x00, 0x00, 0x00, 0xFF, // AFPointsUsed: 255 (big-endian)
        ];

        let metadata = parse_sony_focus_info(&data, true);

        assert_eq!(metadata.get_string("Sony:FocusMode"), Some("AF-S"));
        assert_eq!(metadata.get_string("Sony:AFAreaMode"), Some("Wide"));
        assert_eq!(metadata.get_integer("Sony:FocusPosition"), Some(200));
        assert_eq!(metadata.get_integer("Sony:AFPointSelected"), Some(10));
        assert_eq!(metadata.get_integer("Sony:AFPointsUsed"), Some(255));
    }

    /// Test parsing with insufficient data returns empty map
    #[test]
    fn test_parse_focus_info_insufficient_data() {
        // Only 8 bytes - not enough for all fields (minimum 12 required)
        let data: Vec<u8> = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];

        let metadata = parse_sony_focus_info(&data, false);

        // Should return empty map since data is too short
        assert!(metadata.is_empty());
    }

    /// Test parsing with empty data
    #[test]
    fn test_parse_focus_info_empty_data() {
        let data: Vec<u8> = vec![];

        let metadata = parse_sony_focus_info(&data, false);

        assert!(metadata.is_empty());
    }

    /// Test parsing with manual focus mode
    #[test]
    fn test_parse_focus_info_manual_mode() {
        let data: Vec<u8> = vec![
            0x00, 0x00, // FocusMode: 0 (Manual)
            0x01, 0x00, // AFAreaMode: 1 (Spot)
            0x00, 0x00, // FocusPosition: 0
            0xFF, 0xFF, // AFPointSelected: -1 (none selected in manual mode)
            0x00, 0x00, 0x00, 0x00, // AFPointsUsed: 0 (none used in manual mode)
        ];

        let metadata = parse_sony_focus_info(&data, false);

        assert_eq!(metadata.get_string("Sony:FocusMode"), Some("Manual"));
        assert_eq!(metadata.get_string("Sony:AFAreaMode"), Some("Spot"));
        assert_eq!(metadata.get_integer("Sony:FocusPosition"), Some(0));
        // -1 as i16 interpreted as i64
        assert_eq!(metadata.get_integer("Sony:AFPointSelected"), Some(-1));
        assert_eq!(metadata.get_integer("Sony:AFPointsUsed"), Some(0));
    }

    /// Test all focus mode decoder values
    #[test]
    fn test_decode_focus_mode_all_values() {
        assert_eq!(decode_focus_mode(0), "Manual");
        assert_eq!(decode_focus_mode(1), "AF-S");
        assert_eq!(decode_focus_mode(2), "AF-C");
        assert_eq!(decode_focus_mode(3), "AF-A");
        assert_eq!(decode_focus_mode(4), "DMF");
        assert_eq!(decode_focus_mode(5), "AF-D");
        assert_eq!(decode_focus_mode(6), "AF-S (Continuous)");
        assert_eq!(decode_focus_mode(99), "Unknown");
        assert_eq!(decode_focus_mode(-1), "Unknown");
    }

    /// Test all AF area mode decoder values
    #[test]
    fn test_decode_af_area_mode_all_values() {
        assert_eq!(decode_af_area_mode(0), "Wide");
        assert_eq!(decode_af_area_mode(1), "Spot");
        assert_eq!(decode_af_area_mode(2), "Local");
        assert_eq!(decode_af_area_mode(3), "Flexible Spot");
        assert_eq!(decode_af_area_mode(4), "Zone");
        assert_eq!(decode_af_area_mode(5), "Expand Flexible Spot");
        assert_eq!(decode_af_area_mode(6), "Lock-on AF");
        assert_eq!(decode_af_area_mode(7), "Tracking");
        assert_eq!(decode_af_area_mode(8), "Eye AF");
        assert_eq!(decode_af_area_mode(9), "Flexible Spot (Small)");
        assert_eq!(decode_af_area_mode(10), "Flexible Spot (Medium)");
        assert_eq!(decode_af_area_mode(11), "Flexible Spot (Large)");
        assert_eq!(decode_af_area_mode(99), "Unknown");
        assert_eq!(decode_af_area_mode(-1), "Unknown");
    }

    /// Test byte reading utilities with boundary conditions
    #[test]
    fn test_read_i16_boundary() {
        let data: Vec<u8> = vec![0x01, 0x02];

        // Valid read
        assert_eq!(read_i16(&data, 0, false), Some(0x0201));
        assert_eq!(read_i16(&data, 0, true), Some(0x0102));

        // Out of bounds
        assert_eq!(read_i16(&data, 1, false), None);
        assert_eq!(read_i16(&data, 2, false), None);
    }

    /// Test byte reading utilities with negative values
    #[test]
    fn test_read_i16_negative() {
        // -1 in little-endian
        let data_le: Vec<u8> = vec![0xFF, 0xFF];
        assert_eq!(read_i16(&data_le, 0, false), Some(-1));

        // -1 in big-endian
        let data_be: Vec<u8> = vec![0xFF, 0xFF];
        assert_eq!(read_i16(&data_be, 0, true), Some(-1));

        // -256 in little-endian (0xFF00)
        let data_256_le: Vec<u8> = vec![0x00, 0xFF];
        assert_eq!(read_i16(&data_256_le, 0, false), Some(-256));
    }

    /// Test u32 reading for AF points used field
    #[test]
    fn test_read_u32_values() {
        // Test little-endian max u32
        let data_max_le: Vec<u8> = vec![0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read_u32(&data_max_le, 0, false), Some(u32::MAX));

        // Test big-endian specific value
        let data_be: Vec<u8> = vec![0x00, 0x01, 0x02, 0x03];
        assert_eq!(read_u32(&data_be, 0, true), Some(0x00010203));
        assert_eq!(read_u32(&data_be, 0, false), Some(0x03020100));

        // Out of bounds
        assert_eq!(read_u32(&data_be, 1, false), None);
    }

    /// Test that metadata map contains expected number of entries
    #[test]
    fn test_metadata_map_entry_count() {
        let data: Vec<u8> = vec![
            0x00, 0x00, // FocusMode
            0x00, 0x00, // AFAreaMode
            0x00, 0x00, // FocusPosition
            0x00, 0x00, // AFPointSelected
            0x00, 0x00, 0x00, 0x00, // AFPointsUsed
        ];

        let metadata = parse_sony_focus_info(&data, false);

        // Should have exactly 5 entries
        assert_eq!(metadata.len(), 5);
        assert!(metadata.contains_key("Sony:FocusMode"));
        assert!(metadata.contains_key("Sony:AFAreaMode"));
        assert!(metadata.contains_key("Sony:FocusPosition"));
        assert!(metadata.contains_key("Sony:AFPointSelected"));
        assert!(metadata.contains_key("Sony:AFPointsUsed"));
    }

    /// Test parsing with exactly minimum data length
    #[test]
    fn test_parse_focus_info_exact_minimum_length() {
        // Exactly 12 bytes - the minimum required
        let data: Vec<u8> = vec![
            0x04, 0x00, // FocusMode: 4 (DMF)
            0x08, 0x00, // AFAreaMode: 8 (Eye AF)
            0x50, 0x00, // FocusPosition: 80
            0x03, 0x00, // AFPointSelected: 3
            0x07, 0x00, 0x00, 0x00, // AFPointsUsed: 7
        ];

        let metadata = parse_sony_focus_info(&data, false);

        assert_eq!(metadata.get_string("Sony:FocusMode"), Some("DMF"));
        assert_eq!(metadata.get_string("Sony:AFAreaMode"), Some("Eye AF"));
        assert_eq!(metadata.get_integer("Sony:FocusPosition"), Some(80));
        assert_eq!(metadata.get_integer("Sony:AFPointSelected"), Some(3));
        assert_eq!(metadata.get_integer("Sony:AFPointsUsed"), Some(7));
    }

    /// Test that extra data beyond minimum is ignored
    #[test]
    fn test_parse_focus_info_extra_data_ignored() {
        // More than 12 bytes - extra data should be ignored
        let data: Vec<u8> = vec![
            0x02, 0x00, // FocusMode: 2 (AF-C)
            0x04, 0x00, // AFAreaMode: 4 (Zone)
            0x32, 0x00, // FocusPosition: 50
            0x01, 0x00, // AFPointSelected: 1
            0x03, 0x00, 0x00, 0x00, // AFPointsUsed: 3
            0xAA, 0xBB, 0xCC, 0xDD, // Extra garbage data
        ];

        let metadata = parse_sony_focus_info(&data, false);

        // Should still parse correctly, ignoring extra data
        assert_eq!(metadata.len(), 5);
        assert_eq!(metadata.get_string("Sony:FocusMode"), Some("AF-C"));
        assert_eq!(metadata.get_string("Sony:AFAreaMode"), Some("Zone"));
        assert_eq!(metadata.get_integer("Sony:FocusPosition"), Some(50));
        assert_eq!(metadata.get_integer("Sony:AFPointSelected"), Some(1));
        assert_eq!(metadata.get_integer("Sony:AFPointsUsed"), Some(3));
    }
}
