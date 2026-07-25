//! Panasonic Extended Tags Parser
//!
//! This module provides parsing for Panasonic-specific extended MakerNote tags
//! that require additional processing beyond the standard registry-based parsing.
//!
//! ## Extended Tags Supported
//!
//! This parser handles the following Panasonic extended tags:
//!
//! - **IntelligentExposure** (0x005D): Adaptive exposure compensation mode
//! - **IntelligentResolution** (0x0070): Smart resolution enhancement mode
//! - **IntelligentD-Range** (0x0079): Dynamic range optimization mode
//! - **PhotoStyle** (0x0089): Picture style/color profile preset
//! - **HDR** (0x009E): High Dynamic Range mode settings
//! - **AccelerometerX** (0x008D): Camera orientation X-axis acceleration
//! - **AccelerometerY** (0x008E): Camera orientation Y-axis acceleration
//! - **AccelerometerZ** (0x008C): Camera orientation Z-axis acceleration
//!
//! ## Tag ID Reference
//!
//! Tag IDs are based on ExifTool's Panasonic.pm module:
//! - <https://exiftool.org/TagNames/Panasonic.html>
//!
//! ## Usage
//!
//! ```ignore
//! use crate::core::MetadataMap;
//! use crate::parsers::tiff::makernotes::panasonic::extended::parse_panasonic_extended;
//!
//! let data: &[u8] = &[/* MakerNote data */];
//! let byte_order = true; // true = little-endian, false = big-endian
//! let metadata = parse_panasonic_extended(data, byte_order);
//! ```

use crate::core::MetadataMap;
use crate::core::TagValue;

// =============================================================================
// CONSTANTS - Tag IDs from ExifTool's Panasonic.pm
// =============================================================================

/// IntelligentExposure tag ID - adaptive exposure mode (Off/Low/Standard/High)
const TAG_INTELLIGENT_EXPOSURE: u16 = 0x005D;

/// IntelligentResolution tag ID - resolution enhancement (Off/Low/Standard/High/Extended)
const TAG_INTELLIGENT_RESOLUTION: u16 = 0x0070;

/// IntelligentD-Range tag ID - dynamic range optimization (Off/Low/Standard/High)
const TAG_INTELLIGENT_D_RANGE: u16 = 0x0079;

/// PhotoStyle tag ID - picture style preset
const TAG_PHOTO_STYLE: u16 = 0x0089;

/// HDR mode tag ID - high dynamic range settings
const TAG_HDR: u16 = 0x009E;

/// AccelerometerZ tag ID - Z-axis acceleration value
const TAG_ACCELEROMETER_Z: u16 = 0x008C;

/// AccelerometerX tag ID - X-axis acceleration value (also used for RollAngle)
const TAG_ACCELEROMETER_X: u16 = 0x008D;

/// AccelerometerY tag ID - Y-axis acceleration value (also used for PitchAngle)
const TAG_ACCELEROMETER_Y: u16 = 0x008E;

/// Panasonic MakerNote header - identifies Panasonic MakerNotes
const PANASONIC_HEADER: &[u8] = b"Panasonic\0\0\0";

/// Length of the Panasonic header in bytes
const PANASONIC_HEADER_LEN: usize = 12;

/// Size of a single IFD entry in bytes (2 + 2 + 4 + 4 = 12)
const IFD_ENTRY_SIZE: usize = 12;

// =============================================================================
// VALUE DECODERS
// =============================================================================
// These decoders map raw integer values to human-readable strings.
// Values are documented in ExifTool's Panasonic.pm source.

/// Decodes IntelligentExposure values to human-readable strings.
///
/// IntelligentExposure (iExposure) automatically adjusts exposure to prevent
/// blown highlights and blocked shadows.
///
/// # Arguments
///
/// * `value` - Raw tag value from MakerNote
///
/// # Returns
///
/// Human-readable string describing the iExposure mode
fn decode_intelligent_exposure(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Low",
        2 => "Standard",
        3 => "High",
        _ => "Unknown",
    }
}

/// Decodes IntelligentResolution values to human-readable strings.
///
/// IntelligentResolution (iResolution) enhances detail and sharpness
/// through intelligent processing.
///
/// # Arguments
///
/// * `value` - Raw tag value from MakerNote
///
/// # Returns
///
/// Human-readable string describing the iResolution mode
fn decode_intelligent_resolution(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Low",
        2 => "Standard",
        3 => "High",
        4 => "Extended",
        _ => "Unknown",
    }
}

/// Decodes IntelligentD-Range values to human-readable strings.
///
/// IntelligentD-Range (iDynamic) optimizes dynamic range to preserve
/// detail in highlights and shadows.
///
/// # Arguments
///
/// * `value` - Raw tag value from MakerNote
///
/// # Returns
///
/// Human-readable string describing the iDynamic mode
fn decode_intelligent_d_range(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Low",
        2 => "Standard",
        3 => "High",
        _ => "Unknown",
    }
}

/// Decodes PhotoStyle values to human-readable strings.
///
/// PhotoStyle controls the overall look and color rendering of images,
/// similar to film simulation modes on other cameras.
///
/// # Arguments
///
/// * `value` - Raw tag value from MakerNote
///
/// # Returns
///
/// Human-readable string describing the PhotoStyle preset
fn decode_photo_style(value: i32) -> &'static str {
    match value {
        0 => "Standard",
        1 => "Vivid",
        2 => "Natural",
        3 => "Monochrome",
        4 => "Scenery",
        5 => "Portrait",
        6 => "Custom",
        7 => "Cinelike D",
        8 => "Cinelike V",
        9 => "Like 709",
        10 => "V-Log",
        11 => "V-Log L",
        _ => "Unknown",
    }
}

/// Decodes HDR mode values to human-readable strings.
///
/// HDR mode captures multiple exposures and combines them for
/// extended dynamic range.
///
/// # Arguments
///
/// * `value` - Raw tag value from MakerNote
///
/// # Returns
///
/// Human-readable string describing the HDR mode
fn decode_hdr(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "HDR (1 EV)",
        2 => "HDR (2 EV)",
        3 => "HDR (3 EV)",
        100 => "HDR Auto",
        _ => "Unknown",
    }
}

// =============================================================================
// BYTE READING UTILITIES
// =============================================================================

/// Reads a 16-bit unsigned integer from a byte slice with specified endianness.
///
/// # Arguments
///
/// * `data` - Byte slice containing at least 2 bytes
/// * `offset` - Byte offset to read from
/// * `little_endian` - true for little-endian, false for big-endian
///
/// # Returns
///
/// The 16-bit value, or None if insufficient data
fn read_u16(data: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }
    let bytes = [data[offset], data[offset + 1]];
    Some(if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

/// Reads a 32-bit unsigned integer from a byte slice with specified endianness.
///
/// # Arguments
///
/// * `data` - Byte slice containing at least 4 bytes
/// * `offset` - Byte offset to read from
/// * `little_endian` - true for little-endian, false for big-endian
///
/// # Returns
///
/// The 32-bit value, or None if insufficient data
fn read_u32(data: &[u8], offset: usize, little_endian: bool) -> Option<u32> {
    if offset + 4 > data.len() {
        return None;
    }
    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ];
    Some(if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}

/// Reads a 16-bit signed integer from a byte slice with specified endianness.
///
/// Used for accelerometer values which can be negative.
///
/// # Arguments
///
/// * `data` - Byte slice containing at least 2 bytes
/// * `offset` - Byte offset to read from
/// * `little_endian` - true for little-endian, false for big-endian
///
/// # Returns
///
/// The 16-bit signed value, or None if insufficient data
fn read_i16(data: &[u8], offset: usize, little_endian: bool) -> Option<i16> {
    if offset + 2 > data.len() {
        return None;
    }
    let bytes = [data[offset], data[offset + 1]];
    Some(if little_endian {
        i16::from_le_bytes(bytes)
    } else {
        i16::from_be_bytes(bytes)
    })
}

// =============================================================================
// IFD ENTRY STRUCTURE
// =============================================================================

/// Represents a single IFD (Image File Directory) entry.
///
/// Each IFD entry contains metadata about a single tag, including its
/// identifier, data type, count, and value/offset.
#[derive(Debug, Clone)]
struct IfdEntry {
    /// Tag identifier (e.g., 0x005D for IntelligentExposure)
    tag_id: u16,
    /// Field type (1=BYTE, 2=ASCII, 3=SHORT, 4=LONG, etc.)
    #[allow(dead_code)]
    field_type: u16,
    /// Number of values (not bytes)
    #[allow(dead_code)]
    value_count: u32,
    /// Value if it fits in 4 bytes, otherwise offset to value
    value_offset: u32,
}

/// Parses a single IFD entry from raw bytes.
///
/// # Arguments
///
/// * `data` - Byte slice containing the IFD entry (must be at least 12 bytes)
/// * `little_endian` - true for little-endian byte order
///
/// # Returns
///
/// Parsed IFD entry, or None if insufficient data
fn parse_ifd_entry(data: &[u8], little_endian: bool) -> Option<IfdEntry> {
    if data.len() < IFD_ENTRY_SIZE {
        return None;
    }

    Some(IfdEntry {
        tag_id: read_u16(data, 0, little_endian)?,
        field_type: read_u16(data, 2, little_endian)?,
        value_count: read_u32(data, 4, little_endian)?,
        value_offset: read_u32(data, 8, little_endian)?,
    })
}

// =============================================================================
// MAIN PARSER FUNCTION
// =============================================================================

/// Parses Panasonic extended MakerNote tags from raw byte data.
///
/// This function extracts extended Panasonic-specific metadata tags that provide
/// information about intelligent processing modes, picture styles, HDR settings,
/// and camera orientation (accelerometer data).
///
/// ## Supported Tags
///
/// | Tag Name | Tag ID | Description |
/// |----------|--------|-------------|
/// | IntelligentExposure | 0x005D | Adaptive exposure compensation |
/// | IntelligentResolution | 0x0070 | Smart resolution enhancement |
/// | IntelligentD-Range | 0x0079 | Dynamic range optimization |
/// | PhotoStyle | 0x0089 | Picture style preset |
/// | HDR | 0x009E | High dynamic range mode |
/// | AccelerometerX | 0x008D | X-axis acceleration |
/// | AccelerometerY | 0x008E | Y-axis acceleration |
/// | AccelerometerZ | 0x008C | Z-axis acceleration |
///
/// ## Data Format
///
/// Panasonic MakerNotes use a 12-byte header ("Panasonic\0\0\0") followed by
/// a standard TIFF IFD structure. The byte order parameter determines how
/// multi-byte values are interpreted.
///
/// ## Accelerometer Values
///
/// Accelerometer values are stored as signed 16-bit integers and represent
/// the gravitational force on each axis. Values are typically in the range
/// of -1000 to +1000, where 1000 represents 1g of acceleration.
///
/// # Arguments
///
/// * `data` - Raw MakerNote data bytes (including header)
/// * `byte_order` - true for little-endian, false for big-endian
///
/// # Returns
///
/// A `MetadataMap` containing all successfully parsed extended tags.
/// Tags that cannot be parsed (missing, corrupted, etc.) are silently skipped.
///
/// # Examples
///
/// ```ignore
/// use crate::core::MetadataMap;
///
/// // Parse little-endian Panasonic MakerNote
/// let metadata = parse_panasonic_extended(&makernote_data, true);
///
/// // Access parsed values
/// if let Some(photo_style) = metadata.get_string("Panasonic:PhotoStyle") {
///     println!("Photo Style: {}", photo_style);
/// }
///
/// // Accelerometer values are stored as integers
/// if let Some(accel_x) = metadata.get_integer("Panasonic:AccelerometerX") {
///     println!("X-axis acceleration: {}", accel_x);
/// }
/// ```
pub fn parse_panasonic_extended(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Validate minimum data length for header check
    if data.len() < PANASONIC_HEADER_LEN {
        return metadata;
    }

    // Verify Panasonic MakerNote header
    // The header must match "Panasonic\0\0\0" exactly
    if &data[..PANASONIC_HEADER_LEN] != PANASONIC_HEADER {
        return metadata;
    }

    // Skip header to reach IFD data
    let ifd_data = &data[PANASONIC_HEADER_LEN..];

    // Read IFD entry count (first 2 bytes after header)
    let entry_count = match read_u16(ifd_data, 0, byte_order) {
        Some(count) => count as usize,
        None => return metadata,
    };

    // Validate that we have enough data for all entries
    // Each entry is 12 bytes, plus 2 bytes for the count
    let required_len = 2 + (entry_count * IFD_ENTRY_SIZE);
    if ifd_data.len() < required_len {
        return metadata;
    }

    // Parse each IFD entry looking for our target tags
    for i in 0..entry_count {
        let entry_offset = 2 + (i * IFD_ENTRY_SIZE);
        let entry_data = &ifd_data[entry_offset..];

        let entry = match parse_ifd_entry(entry_data, byte_order) {
            Some(e) => e,
            None => continue,
        };

        // Process only the extended tags we're interested in
        match entry.tag_id {
            TAG_INTELLIGENT_EXPOSURE => {
                let decoded = decode_intelligent_exposure(entry.value_offset as i32);
                metadata.insert(
                    "Panasonic:IntelligentExposure",
                    TagValue::new_string(decoded),
                );
            }

            TAG_INTELLIGENT_RESOLUTION => {
                let decoded = decode_intelligent_resolution(entry.value_offset as i32);
                metadata.insert(
                    "Panasonic:IntelligentResolution",
                    TagValue::new_string(decoded),
                );
            }

            TAG_INTELLIGENT_D_RANGE => {
                let decoded = decode_intelligent_d_range(entry.value_offset as i32);
                metadata.insert(
                    "Panasonic:IntelligentD-Range",
                    TagValue::new_string(decoded),
                );
            }

            TAG_PHOTO_STYLE => {
                let decoded = decode_photo_style(entry.value_offset as i32);
                metadata.insert("Panasonic:PhotoStyle", TagValue::new_string(decoded));
            }

            TAG_HDR => {
                let decoded = decode_hdr(entry.value_offset as i32);
                metadata.insert("Panasonic:HDR", TagValue::new_string(decoded));
            }

            TAG_ACCELEROMETER_X => {
                // Accelerometer values are signed 16-bit integers
                // The value is stored in the lower 16 bits of value_offset
                let value = (entry.value_offset & 0xFFFF) as i16;
                metadata.insert(
                    "Panasonic:AccelerometerX",
                    TagValue::new_integer(value as i64),
                );
            }

            TAG_ACCELEROMETER_Y => {
                let value = (entry.value_offset & 0xFFFF) as i16;
                metadata.insert(
                    "Panasonic:AccelerometerY",
                    TagValue::new_integer(value as i64),
                );
            }

            TAG_ACCELEROMETER_Z => {
                let value = (entry.value_offset & 0xFFFF) as i16;
                metadata.insert(
                    "Panasonic:AccelerometerZ",
                    TagValue::new_integer(value as i64),
                );
            }

            // Skip all other tags - they're handled by the main parser
            _ => {}
        }
    }

    metadata
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Decoder Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_decode_intelligent_exposure() {
        assert_eq!(decode_intelligent_exposure(0), "Off");
        assert_eq!(decode_intelligent_exposure(1), "Low");
        assert_eq!(decode_intelligent_exposure(2), "Standard");
        assert_eq!(decode_intelligent_exposure(3), "High");
        assert_eq!(decode_intelligent_exposure(99), "Unknown");
    }

    #[test]
    fn test_decode_intelligent_resolution() {
        assert_eq!(decode_intelligent_resolution(0), "Off");
        assert_eq!(decode_intelligent_resolution(1), "Low");
        assert_eq!(decode_intelligent_resolution(2), "Standard");
        assert_eq!(decode_intelligent_resolution(3), "High");
        assert_eq!(decode_intelligent_resolution(4), "Extended");
        assert_eq!(decode_intelligent_resolution(99), "Unknown");
    }

    #[test]
    fn test_decode_intelligent_d_range() {
        assert_eq!(decode_intelligent_d_range(0), "Off");
        assert_eq!(decode_intelligent_d_range(1), "Low");
        assert_eq!(decode_intelligent_d_range(2), "Standard");
        assert_eq!(decode_intelligent_d_range(3), "High");
        assert_eq!(decode_intelligent_d_range(99), "Unknown");
    }

    #[test]
    fn test_decode_photo_style() {
        assert_eq!(decode_photo_style(0), "Standard");
        assert_eq!(decode_photo_style(1), "Vivid");
        assert_eq!(decode_photo_style(2), "Natural");
        assert_eq!(decode_photo_style(3), "Monochrome");
        assert_eq!(decode_photo_style(4), "Scenery");
        assert_eq!(decode_photo_style(5), "Portrait");
        assert_eq!(decode_photo_style(6), "Custom");
        assert_eq!(decode_photo_style(7), "Cinelike D");
        assert_eq!(decode_photo_style(8), "Cinelike V");
        assert_eq!(decode_photo_style(9), "Like 709");
        assert_eq!(decode_photo_style(10), "V-Log");
        assert_eq!(decode_photo_style(11), "V-Log L");
        assert_eq!(decode_photo_style(99), "Unknown");
    }

    #[test]
    fn test_decode_hdr() {
        assert_eq!(decode_hdr(0), "Off");
        assert_eq!(decode_hdr(1), "HDR (1 EV)");
        assert_eq!(decode_hdr(2), "HDR (2 EV)");
        assert_eq!(decode_hdr(3), "HDR (3 EV)");
        assert_eq!(decode_hdr(100), "HDR Auto");
        assert_eq!(decode_hdr(99), "Unknown");
    }

    // -------------------------------------------------------------------------
    // Byte Reading Utility Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_read_u16_little_endian() {
        let data = [0x34, 0x12];
        assert_eq!(read_u16(&data, 0, true), Some(0x1234));
    }

    #[test]
    fn test_read_u16_big_endian() {
        let data = [0x12, 0x34];
        assert_eq!(read_u16(&data, 0, false), Some(0x1234));
    }

    #[test]
    fn test_read_u16_insufficient_data() {
        let data = [0x12];
        assert_eq!(read_u16(&data, 0, true), None);
    }

    #[test]
    fn test_read_u32_little_endian() {
        let data = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_u32(&data, 0, true), Some(0x12345678));
    }

    #[test]
    fn test_read_u32_big_endian() {
        let data = [0x12, 0x34, 0x56, 0x78];
        assert_eq!(read_u32(&data, 0, false), Some(0x12345678));
    }

    #[test]
    fn test_read_i16_positive() {
        let data = [0x01, 0x00]; // 256 in big-endian (0x0100)
        assert_eq!(read_i16(&data, 0, false), Some(256));
    }

    #[test]
    fn test_read_i16_negative() {
        // -100 in little-endian: 0x9C, 0xFF
        let data = [0x9C, 0xFF];
        assert_eq!(read_i16(&data, 0, true), Some(-100));
    }

    // -------------------------------------------------------------------------
    // IFD Entry Parsing Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_ifd_entry_little_endian() {
        // Create a mock IFD entry for IntelligentExposure (0x005D)
        // Tag ID: 0x005D, Type: SHORT (3), Count: 1, Value: 2 (Standard)
        let entry_data = [
            0x5D, 0x00, // Tag ID (little-endian)
            0x03, 0x00, // Field type (SHORT)
            0x01, 0x00, 0x00, 0x00, // Count
            0x02, 0x00, 0x00, 0x00, // Value (2 = Standard)
        ];

        let entry = parse_ifd_entry(&entry_data, true).unwrap();
        assert_eq!(entry.tag_id, 0x005D);
        assert_eq!(entry.field_type, 3);
        assert_eq!(entry.value_count, 1);
        assert_eq!(entry.value_offset, 2);
    }

    #[test]
    fn test_parse_ifd_entry_insufficient_data() {
        let short_data = [0x5D, 0x00, 0x03, 0x00]; // Only 4 bytes
        assert!(parse_ifd_entry(&short_data, true).is_none());
    }

    // -------------------------------------------------------------------------
    // Main Parser Function Tests
    // -------------------------------------------------------------------------

    /// Helper function to create a valid Panasonic MakerNote with specified entries.
    ///
    /// This builds a complete MakerNote structure with:
    /// - 12-byte Panasonic header
    /// - 2-byte entry count
    /// - Variable number of 12-byte IFD entries
    fn create_test_makernote(entries: &[(u16, u16, u32, u32)]) -> Vec<u8> {
        let mut data = Vec::new();

        // Add Panasonic header
        data.extend_from_slice(PANASONIC_HEADER);

        // Add entry count (little-endian)
        let count = entries.len() as u16;
        data.extend_from_slice(&count.to_le_bytes());

        // Add IFD entries
        for (tag_id, field_type, value_count, value_offset) in entries {
            data.extend_from_slice(&tag_id.to_le_bytes());
            data.extend_from_slice(&field_type.to_le_bytes());
            data.extend_from_slice(&value_count.to_le_bytes());
            data.extend_from_slice(&value_offset.to_le_bytes());
        }

        data
    }

    #[test]
    fn test_parse_empty_data() {
        let metadata = parse_panasonic_extended(&[], true);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_invalid_header() {
        // Data with wrong header
        let data = b"NotPanasonic";
        let metadata = parse_panasonic_extended(data, true);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_too_short_header() {
        let data = b"Panasonic";
        let metadata = parse_panasonic_extended(data, true);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_intelligent_exposure() {
        let entries = vec![
            (TAG_INTELLIGENT_EXPOSURE, 3, 1, 2), // Standard
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        assert_eq!(
            metadata.get_string("Panasonic:IntelligentExposure"),
            Some("Standard")
        );
    }

    #[test]
    fn test_parse_intelligent_resolution() {
        let entries = vec![
            (TAG_INTELLIGENT_RESOLUTION, 3, 1, 4), // Extended
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        assert_eq!(
            metadata.get_string("Panasonic:IntelligentResolution"),
            Some("Extended")
        );
    }

    #[test]
    fn test_parse_intelligent_d_range() {
        let entries = vec![
            (TAG_INTELLIGENT_D_RANGE, 3, 1, 3), // High
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        assert_eq!(
            metadata.get_string("Panasonic:IntelligentD-Range"),
            Some("High")
        );
    }

    #[test]
    fn test_parse_photo_style() {
        let entries = vec![
            (TAG_PHOTO_STYLE, 3, 1, 7), // Cinelike D
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        assert_eq!(
            metadata.get_string("Panasonic:PhotoStyle"),
            Some("Cinelike D")
        );
    }

    #[test]
    fn test_parse_hdr() {
        let entries = vec![
            (TAG_HDR, 3, 1, 100), // HDR Auto
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        assert_eq!(metadata.get_string("Panasonic:HDR"), Some("HDR Auto"));
    }

    #[test]
    fn test_parse_accelerometer_positive() {
        let entries = vec![
            (TAG_ACCELEROMETER_X, 4, 1, 500), // Positive X acceleration
            (TAG_ACCELEROMETER_Y, 4, 1, 300), // Positive Y acceleration
            (TAG_ACCELEROMETER_Z, 4, 1, 980), // Near 1g Z acceleration
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        assert_eq!(metadata.get_integer("Panasonic:AccelerometerX"), Some(500));
        assert_eq!(metadata.get_integer("Panasonic:AccelerometerY"), Some(300));
        assert_eq!(metadata.get_integer("Panasonic:AccelerometerZ"), Some(980));
    }

    #[test]
    fn test_parse_accelerometer_negative() {
        // Test negative accelerometer value
        // -100 as u16 = 65436, stored in lower 16 bits
        let neg_value: u16 = (-100_i16) as u16;
        let entries = vec![(TAG_ACCELEROMETER_X, 4, 1, neg_value as u32)];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        assert_eq!(metadata.get_integer("Panasonic:AccelerometerX"), Some(-100));
    }

    #[test]
    fn test_parse_multiple_tags() {
        // Test parsing multiple extended tags at once
        let entries = vec![
            (TAG_INTELLIGENT_EXPOSURE, 3, 1, 1),   // Low
            (TAG_INTELLIGENT_RESOLUTION, 3, 1, 2), // Standard
            (TAG_INTELLIGENT_D_RANGE, 3, 1, 0),    // Off
            (TAG_PHOTO_STYLE, 3, 1, 0),            // Standard
            (TAG_HDR, 3, 1, 0),                    // Off
            (TAG_ACCELEROMETER_X, 4, 1, 100),
            (TAG_ACCELEROMETER_Y, 4, 1, 200),
            (TAG_ACCELEROMETER_Z, 4, 1, 950),
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        // Verify all tags were parsed
        assert_eq!(metadata.len(), 8);

        assert_eq!(
            metadata.get_string("Panasonic:IntelligentExposure"),
            Some("Low")
        );
        assert_eq!(
            metadata.get_string("Panasonic:IntelligentResolution"),
            Some("Standard")
        );
        assert_eq!(
            metadata.get_string("Panasonic:IntelligentD-Range"),
            Some("Off")
        );
        assert_eq!(
            metadata.get_string("Panasonic:PhotoStyle"),
            Some("Standard")
        );
        assert_eq!(metadata.get_string("Panasonic:HDR"), Some("Off"));
        assert_eq!(metadata.get_integer("Panasonic:AccelerometerX"), Some(100));
        assert_eq!(metadata.get_integer("Panasonic:AccelerometerY"), Some(200));
        assert_eq!(metadata.get_integer("Panasonic:AccelerometerZ"), Some(950));
    }

    #[test]
    fn test_parse_ignores_unknown_tags() {
        // Include some tags that should be ignored
        let entries = vec![
            (0x0001, 2, 10, 0),         // ImageQuality (string tag, not extended)
            (TAG_PHOTO_STYLE, 3, 1, 5), // Portrait
            (0x0003, 3, 1, 1),          // WhiteBalance (not extended)
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        // Only PhotoStyle should be parsed
        assert_eq!(metadata.len(), 1);
        assert_eq!(
            metadata.get_string("Panasonic:PhotoStyle"),
            Some("Portrait")
        );
    }

    #[test]
    fn test_parse_header_only() {
        // MakerNote with header but no entries
        let mut data = PANASONIC_HEADER.to_vec();
        data.extend_from_slice(&0u16.to_le_bytes()); // Zero entries

        let metadata = parse_panasonic_extended(&data, true);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_big_endian() {
        // Create big-endian MakerNote data manually
        let mut data = PANASONIC_HEADER.to_vec();

        // Entry count (big-endian)
        data.extend_from_slice(&1u16.to_be_bytes());

        // Single entry: PhotoStyle = Vivid
        data.extend_from_slice(&TAG_PHOTO_STYLE.to_be_bytes()); // Tag ID
        data.extend_from_slice(&3u16.to_be_bytes()); // Type: SHORT
        data.extend_from_slice(&1u32.to_be_bytes()); // Count
        data.extend_from_slice(&1u32.to_be_bytes()); // Value: 1 = Vivid

        let metadata = parse_panasonic_extended(&data, false);

        assert_eq!(metadata.get_string("Panasonic:PhotoStyle"), Some("Vivid"));
    }

    #[test]
    fn test_parse_unknown_decoder_values() {
        // Test that unknown values decode gracefully
        let entries = vec![
            (TAG_INTELLIGENT_EXPOSURE, 3, 1, 255), // Unknown value
            (TAG_PHOTO_STYLE, 3, 1, 999),          // Unknown value
            (TAG_HDR, 3, 1, 50),                   // Unknown value
        ];
        let data = create_test_makernote(&entries);

        let metadata = parse_panasonic_extended(&data, true);

        assert_eq!(
            metadata.get_string("Panasonic:IntelligentExposure"),
            Some("Unknown")
        );
        assert_eq!(metadata.get_string("Panasonic:PhotoStyle"), Some("Unknown"));
        assert_eq!(metadata.get_string("Panasonic:HDR"), Some("Unknown"));
    }

    // -------------------------------------------------------------------------
    // Edge Case Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_truncated_entry_data() {
        // Create MakerNote with entry count claiming more entries than data
        let mut data = PANASONIC_HEADER.to_vec();
        data.extend_from_slice(&10u16.to_le_bytes()); // Claims 10 entries

        // Only add 1 entry worth of data
        data.extend_from_slice(&TAG_PHOTO_STYLE.to_le_bytes());
        data.extend_from_slice(&3u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        // Should return empty because validation fails
        let metadata = parse_panasonic_extended(&data, true);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_exact_minimum_length() {
        // Test with exactly 12-byte header (minimum valid length for header check)
        let metadata = parse_panasonic_extended(PANASONIC_HEADER, true);
        // Should fail due to missing entry count
        assert!(metadata.is_empty());
    }
}
