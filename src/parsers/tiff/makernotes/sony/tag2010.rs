//! Sony Tag2010 (0x2010) Parser
//!
//! Parses Sony MakerNote Tag 0x2010, which contains comprehensive camera settings
//! encoded as a binary data block. This tag is found in modern Sony cameras and
//! includes exposure program, internal serial number, recommended exposure index,
//! compression settings, and quality mode information.
//!
//! ## Tag Structure
//!
//! Tag 0x2010 is a variable-length binary blob where specific bytes or byte ranges
//! encode different camera settings. The exact structure varies by camera model,
//! but common fields include:
//!
//! - **Offset 0-1**: Header/version identifier
//! - **Offset 2-3**: ExposureProgram (encoded as u16)
//! - **Offset 4-19**: InternalSerialNumber (16-byte ASCII string)
//! - **Offset 20-23**: RecommendedExposureIndex (u32)
//! - **Offset 24**: Compression mode
//! - **Offset 25**: Quality mode
//!
//! ## Usage
//!
//! ```ignore
//! use oxidex::parsers::tiff::makernotes::sony::tag2010::parse_sony_tag2010;
//! use oxidex::parsers::tiff::ifd_parser::ByteOrder;
//!
//! let data: &[u8] = &[/* tag 0x2010 data */];
//! let is_little_endian = true;
//! let metadata = parse_sony_tag2010(data, is_little_endian);
//!
//! // Access parsed values
//! if let Some(program) = metadata.get("Sony:ExposureProgram") {
//!     println!("Exposure Program: {:?}", program);
//! }
//! ```
//!
//! ## References
//!
//! - ExifTool Sony.pm Tag2010 table
//! - Sony camera internal documentation (reverse-engineered)

use crate::core::MetadataMap;
use crate::core::TagValue;

// ============================================================================
// CONSTANTS - Tag2010 Field Offsets
// ============================================================================
// These offsets define where specific fields are located within the Tag2010
// binary data structure. Offsets are based on ExifTool Sony.pm documentation.

/// Offset to ExposureProgram field (2 bytes, u16)
const OFFSET_EXPOSURE_PROGRAM: usize = 2;

/// Offset to InternalSerialNumber field (16 bytes, ASCII string)
const OFFSET_INTERNAL_SERIAL: usize = 4;

/// Length of InternalSerialNumber field in bytes
const INTERNAL_SERIAL_LENGTH: usize = 16;

/// Offset to RecommendedExposureIndex field (4 bytes, u32)
const OFFSET_RECOMMENDED_EXPOSURE_INDEX: usize = 20;

/// Offset to Compression mode field (1 byte)
const OFFSET_COMPRESSION: usize = 24;

/// Offset to Quality mode field (1 byte)
const OFFSET_QUALITY: usize = 25;

/// Minimum data length required to parse Tag2010
/// (must include at least the Quality field at offset 25 + 1 byte)
const MIN_TAG2010_LENGTH: usize = 26;

// ============================================================================
// EXPOSURE PROGRAM DECODER
// ============================================================================

/// Decodes the ExposureProgram value to a human-readable string.
///
/// Sony's ExposureProgram encoding follows EXIF standards with some
/// Sony-specific extensions for advanced shooting modes.
///
/// # Arguments
///
/// * `value` - The raw u16 exposure program value from Tag2010
///
/// # Returns
///
/// A string describing the exposure program mode
fn decode_exposure_program(value: u16) -> &'static str {
    match value {
        0 => "Not Defined",
        1 => "Manual",
        2 => "Program AE",
        3 => "Aperture Priority",
        4 => "Shutter Priority",
        5 => "Creative (Slow speed)",
        6 => "Action (High speed)",
        7 => "Portrait",
        8 => "Landscape",
        9 => "Bulb",
        // Sony-specific extended modes (values > 8)
        10 => "Panorama",
        11 => "Movie",
        12 => "Scene Selection",
        13 => "iAuto",
        14 => "iAuto+",
        15 => "Sweep Panorama",
        16 => "Superior Auto",
        _ => "Unknown",
    }
}

// ============================================================================
// COMPRESSION DECODER
// ============================================================================

/// Decodes the Compression setting value to a human-readable string.
///
/// This field indicates the compression type used for the image data.
///
/// # Arguments
///
/// * `value` - The raw u8 compression value from Tag2010
///
/// # Returns
///
/// A string describing the compression type
fn decode_compression(value: u8) -> &'static str {
    match value {
        0 => "Uncompressed",
        1 => "JPEG",
        2 => "HEIF",
        3 => "RAW",
        4 => "RAW + JPEG",
        5 => "RAW + HEIF",
        6 => "Compressed RAW",
        7 => "Compressed RAW + JPEG",
        8 => "Compressed RAW + HEIF",
        _ => "Unknown",
    }
}

// ============================================================================
// QUALITY DECODER
// ============================================================================

/// Decodes the Quality mode value to a human-readable string.
///
/// This field indicates the image quality setting (affects JPEG compression
/// quality and file size).
///
/// # Arguments
///
/// * `value` - The raw u8 quality value from Tag2010
///
/// # Returns
///
/// A string describing the quality mode
fn decode_quality(value: u8) -> &'static str {
    match value {
        0 => "RAW",
        1 => "Extra Fine",
        2 => "Fine",
        3 => "Standard",
        4 => "Economy",
        5 => "Light",
        _ => "Unknown",
    }
}

// ============================================================================
// BYTE ORDER HELPERS
// ============================================================================

/// Reads a u16 value from the data slice at the specified offset.
///
/// # Arguments
///
/// * `data` - The raw byte slice
/// * `offset` - Byte offset to read from
/// * `little_endian` - If true, read as little-endian; otherwise big-endian
///
/// # Returns
///
/// The u16 value, or None if the offset is out of bounds
#[inline]
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

/// Reads a u32 value from the data slice at the specified offset.
///
/// # Arguments
///
/// * `data` - The raw byte slice
/// * `offset` - Byte offset to read from
/// * `little_endian` - If true, read as little-endian; otherwise big-endian
///
/// # Returns
///
/// The u32 value, or None if the offset is out of bounds
#[inline]
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

/// Reads an ASCII string from the data slice at the specified offset.
///
/// The string is read up to the specified length or until a null terminator
/// is encountered, whichever comes first. Non-printable characters are
/// filtered out to ensure clean output.
///
/// # Arguments
///
/// * `data` - The raw byte slice
/// * `offset` - Byte offset to start reading from
/// * `length` - Maximum number of bytes to read
///
/// # Returns
///
/// The extracted string, or None if the offset is out of bounds
fn read_ascii_string(data: &[u8], offset: usize, length: usize) -> Option<String> {
    if offset + length > data.len() {
        return None;
    }

    let bytes = &data[offset..offset + length];

    // Find null terminator or use full length
    let end_pos = bytes.iter().position(|&b| b == 0).unwrap_or(length);

    // Convert to string, filtering non-printable ASCII characters
    // This handles cases where the field contains garbage data
    let s: String = bytes[..end_pos]
        .iter()
        .filter(|&&b| (0x20..0x7F).contains(&b)) // Printable ASCII range
        .map(|&b| b as char)
        .collect();

    // Return None if the resulting string is empty (all filtered out)
    if s.is_empty() { None } else { Some(s) }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Parses Sony Tag2010 (0x2010) data and extracts camera settings.
///
/// This function parses the binary data from Sony MakerNote Tag 0x2010 and
/// returns a MetadataMap containing the extracted values. The tag contains
/// comprehensive camera settings including:
///
/// - **ExposureProgram**: The shooting mode (Program AE, Aperture Priority, etc.)
/// - **InternalSerialNumber**: The camera's internal serial number
/// - **RecommendedExposureIndex**: The recommended ISO/exposure index
/// - **Compression**: The image compression type
/// - **Quality**: The image quality setting
///
/// # Arguments
///
/// * `data` - Raw byte data from Tag 0x2010
/// * `byte_order` - If true, data is little-endian; if false, big-endian.
///   Sony cameras typically use little-endian byte order.
///
/// # Returns
///
/// A `MetadataMap` containing all successfully parsed tag values.
/// Each tag is prefixed with "Sony:" namespace (e.g., "Sony:ExposureProgram").
/// If the data is too short or malformed, an empty MetadataMap is returned.
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::sony::tag2010::parse_sony_tag2010;
///
/// // Example Tag2010 data (simplified)
/// let data = vec![
///     0x00, 0x01,             // Header
///     0x02, 0x00,             // ExposureProgram: Program AE (2)
///     b'A', b'B', b'C', b'1', b'2', b'3', b'4', b'5',  // Serial part 1
///     b'6', b'7', b'8', b'9', b'0', 0, 0, 0,           // Serial part 2
///     0x90, 0x01, 0x00, 0x00, // RecommendedExposureIndex: 400
///     0x01,                   // Compression: JPEG
///     0x02,                   // Quality: Fine
/// ];
///
/// let metadata = parse_sony_tag2010(&data, true);
///
/// assert!(metadata.get("Sony:ExposureProgram").is_some());
/// assert!(metadata.get("Sony:InternalSerialNumber").is_some());
/// ```
pub fn parse_sony_tag2010(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut map = MetadataMap::new();

    // Validate minimum data length to avoid panics on short data
    if data.len() < MIN_TAG2010_LENGTH {
        // Data too short to contain all expected fields
        // Return empty map rather than partial data to maintain consistency
        return map;
    }

    // Parse ExposureProgram (u16 at offset 2)
    // This indicates the camera's shooting mode
    if let Some(exposure_program) = read_u16(data, OFFSET_EXPOSURE_PROGRAM, byte_order) {
        let decoded = decode_exposure_program(exposure_program);
        map.insert(
            "Sony:ExposureProgram",
            TagValue::new_string(decoded.to_string()),
        );
    }

    // Parse InternalSerialNumber (16-byte ASCII at offset 4)
    // This is the camera's internal serial number, distinct from the body serial
    if let Some(serial) = read_ascii_string(data, OFFSET_INTERNAL_SERIAL, INTERNAL_SERIAL_LENGTH) {
        map.insert("Sony:InternalSerialNumber", TagValue::new_string(serial));
    }

    // Parse RecommendedExposureIndex (u32 at offset 20)
    // This represents the recommended ISO setting for the exposure
    if let Some(exposure_index) = read_u32(data, OFFSET_RECOMMENDED_EXPOSURE_INDEX, byte_order) {
        // Only insert if the value is reasonable (non-zero, within typical ISO range)
        // ISO values typically range from 50 to 409600 in modern cameras
        if exposure_index > 0 && exposure_index <= 409600 {
            map.insert(
                "Sony:RecommendedExposureIndex",
                TagValue::new_integer(exposure_index as i64),
            );
        }
    }

    // Parse Compression mode (u8 at offset 24)
    // This indicates the file format/compression type
    if OFFSET_COMPRESSION < data.len() {
        let compression = data[OFFSET_COMPRESSION];
        let decoded = decode_compression(compression);
        map.insert(
            "Sony:Compression",
            TagValue::new_string(decoded.to_string()),
        );
    }

    // Parse Quality mode (u8 at offset 25)
    // This indicates the image quality/JPEG compression level
    if OFFSET_QUALITY < data.len() {
        let quality = data[OFFSET_QUALITY];
        let decoded = decode_quality(quality);
        map.insert("Sony:Quality", TagValue::new_string(decoded.to_string()));
    }

    map
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Helper Functions for Tests
    // -------------------------------------------------------------------------

    /// Creates a minimal valid Tag2010 data buffer for testing.
    ///
    /// The buffer is initialized with zeros and then populated with
    /// the specified field values.
    fn create_test_data(
        exposure_program: u16,
        serial: &str,
        exposure_index: u32,
        compression: u8,
        quality: u8,
        little_endian: bool,
    ) -> Vec<u8> {
        let mut data = vec![0u8; MIN_TAG2010_LENGTH];

        // Write exposure program at offset 2
        let ep_bytes = if little_endian {
            exposure_program.to_le_bytes()
        } else {
            exposure_program.to_be_bytes()
        };
        data[OFFSET_EXPOSURE_PROGRAM] = ep_bytes[0];
        data[OFFSET_EXPOSURE_PROGRAM + 1] = ep_bytes[1];

        // Write serial number at offset 4 (up to 16 bytes)
        let serial_bytes = serial.as_bytes();
        let copy_len = serial_bytes.len().min(INTERNAL_SERIAL_LENGTH);
        data[OFFSET_INTERNAL_SERIAL..OFFSET_INTERNAL_SERIAL + copy_len]
            .copy_from_slice(&serial_bytes[..copy_len]);

        // Write recommended exposure index at offset 20
        let ei_bytes = if little_endian {
            exposure_index.to_le_bytes()
        } else {
            exposure_index.to_be_bytes()
        };
        data[OFFSET_RECOMMENDED_EXPOSURE_INDEX] = ei_bytes[0];
        data[OFFSET_RECOMMENDED_EXPOSURE_INDEX + 1] = ei_bytes[1];
        data[OFFSET_RECOMMENDED_EXPOSURE_INDEX + 2] = ei_bytes[2];
        data[OFFSET_RECOMMENDED_EXPOSURE_INDEX + 3] = ei_bytes[3];

        // Write compression at offset 24
        data[OFFSET_COMPRESSION] = compression;

        // Write quality at offset 25
        data[OFFSET_QUALITY] = quality;

        data
    }

    // -------------------------------------------------------------------------
    // Exposure Program Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_exposure_program_decoding() {
        assert_eq!(decode_exposure_program(0), "Not Defined");
        assert_eq!(decode_exposure_program(1), "Manual");
        assert_eq!(decode_exposure_program(2), "Program AE");
        assert_eq!(decode_exposure_program(3), "Aperture Priority");
        assert_eq!(decode_exposure_program(4), "Shutter Priority");
        assert_eq!(decode_exposure_program(5), "Creative (Slow speed)");
        assert_eq!(decode_exposure_program(6), "Action (High speed)");
        assert_eq!(decode_exposure_program(7), "Portrait");
        assert_eq!(decode_exposure_program(8), "Landscape");
        assert_eq!(decode_exposure_program(9), "Bulb");
        assert_eq!(decode_exposure_program(10), "Panorama");
        assert_eq!(decode_exposure_program(11), "Movie");
        assert_eq!(decode_exposure_program(12), "Scene Selection");
        assert_eq!(decode_exposure_program(13), "iAuto");
        assert_eq!(decode_exposure_program(14), "iAuto+");
        assert_eq!(decode_exposure_program(15), "Sweep Panorama");
        assert_eq!(decode_exposure_program(16), "Superior Auto");
        assert_eq!(decode_exposure_program(100), "Unknown");
    }

    #[test]
    fn test_parse_exposure_program_little_endian() {
        let data = create_test_data(2, "SN12345678901234", 400, 1, 2, true);
        let result = parse_sony_tag2010(&data, true);

        let value = result.get("Sony:ExposureProgram");
        assert!(value.is_some());
        assert_eq!(value.unwrap().as_string(), Some("Program AE"));
    }

    #[test]
    fn test_parse_exposure_program_big_endian() {
        let data = create_test_data(3, "SN12345678901234", 800, 1, 2, false);
        let result = parse_sony_tag2010(&data, false);

        let value = result.get("Sony:ExposureProgram");
        assert!(value.is_some());
        assert_eq!(value.unwrap().as_string(), Some("Aperture Priority"));
    }

    // -------------------------------------------------------------------------
    // Internal Serial Number Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_internal_serial_number() {
        let data = create_test_data(2, "ABC123456789", 400, 1, 2, true);
        let result = parse_sony_tag2010(&data, true);

        let value = result.get("Sony:InternalSerialNumber");
        assert!(value.is_some());
        assert_eq!(value.unwrap().as_string(), Some("ABC123456789"));
    }

    #[test]
    fn test_parse_internal_serial_number_with_null() {
        // Serial number with embedded null - should truncate at null
        let mut data = create_test_data(2, "ABC123", 400, 1, 2, true);
        // Add explicit null after "ABC123"
        data[OFFSET_INTERNAL_SERIAL + 6] = 0;

        let result = parse_sony_tag2010(&data, true);

        let value = result.get("Sony:InternalSerialNumber");
        assert!(value.is_some());
        assert_eq!(value.unwrap().as_string(), Some("ABC123"));
    }

    #[test]
    fn test_parse_empty_serial_number() {
        // All zeros for serial number - should not insert tag
        let data = create_test_data(2, "", 400, 1, 2, true);
        let result = parse_sony_tag2010(&data, true);

        // Empty serial should not be inserted
        assert!(result.get("Sony:InternalSerialNumber").is_none());
    }

    // -------------------------------------------------------------------------
    // Recommended Exposure Index Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_recommended_exposure_index() {
        let data = create_test_data(2, "SN12345678901234", 800, 1, 2, true);
        let result = parse_sony_tag2010(&data, true);

        let value = result.get("Sony:RecommendedExposureIndex");
        assert!(value.is_some());
        assert_eq!(value.unwrap().as_integer(), Some(800));
    }

    #[test]
    fn test_parse_high_iso_exposure_index() {
        // Test with high ISO value (102400)
        let data = create_test_data(2, "SN12345678901234", 102400, 1, 2, true);
        let result = parse_sony_tag2010(&data, true);

        let value = result.get("Sony:RecommendedExposureIndex");
        assert!(value.is_some());
        assert_eq!(value.unwrap().as_integer(), Some(102400));
    }

    #[test]
    fn test_parse_zero_exposure_index_ignored() {
        // Zero exposure index should be ignored (invalid)
        let data = create_test_data(2, "SN12345678901234", 0, 1, 2, true);
        let result = parse_sony_tag2010(&data, true);

        // Zero value should not be inserted
        assert!(result.get("Sony:RecommendedExposureIndex").is_none());
    }

    #[test]
    fn test_parse_invalid_exposure_index_ignored() {
        // Unreasonably high exposure index should be ignored
        let data = create_test_data(2, "SN12345678901234", 500000, 1, 2, true);
        let result = parse_sony_tag2010(&data, true);

        // Value > 409600 should not be inserted
        assert!(result.get("Sony:RecommendedExposureIndex").is_none());
    }

    // -------------------------------------------------------------------------
    // Compression Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_compression_decoding() {
        assert_eq!(decode_compression(0), "Uncompressed");
        assert_eq!(decode_compression(1), "JPEG");
        assert_eq!(decode_compression(2), "HEIF");
        assert_eq!(decode_compression(3), "RAW");
        assert_eq!(decode_compression(4), "RAW + JPEG");
        assert_eq!(decode_compression(5), "RAW + HEIF");
        assert_eq!(decode_compression(6), "Compressed RAW");
        assert_eq!(decode_compression(7), "Compressed RAW + JPEG");
        assert_eq!(decode_compression(8), "Compressed RAW + HEIF");
        assert_eq!(decode_compression(255), "Unknown");
    }

    #[test]
    fn test_parse_compression() {
        let data = create_test_data(2, "SN12345678901234", 400, 4, 2, true);
        let result = parse_sony_tag2010(&data, true);

        let value = result.get("Sony:Compression");
        assert!(value.is_some());
        assert_eq!(value.unwrap().as_string(), Some("RAW + JPEG"));
    }

    // -------------------------------------------------------------------------
    // Quality Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_quality_decoding() {
        assert_eq!(decode_quality(0), "RAW");
        assert_eq!(decode_quality(1), "Extra Fine");
        assert_eq!(decode_quality(2), "Fine");
        assert_eq!(decode_quality(3), "Standard");
        assert_eq!(decode_quality(4), "Economy");
        assert_eq!(decode_quality(5), "Light");
        assert_eq!(decode_quality(255), "Unknown");
    }

    #[test]
    fn test_parse_quality() {
        let data = create_test_data(2, "SN12345678901234", 400, 1, 1, true);
        let result = parse_sony_tag2010(&data, true);

        let value = result.get("Sony:Quality");
        assert!(value.is_some());
        assert_eq!(value.unwrap().as_string(), Some("Extra Fine"));
    }

    // -------------------------------------------------------------------------
    // Edge Case Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_empty_data() {
        let data: &[u8] = &[];
        let result = parse_sony_tag2010(data, true);

        // Empty data should return empty map
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_short_data() {
        // Data shorter than minimum required length
        let data = vec![0u8; MIN_TAG2010_LENGTH - 1];
        let result = parse_sony_tag2010(&data, true);

        // Short data should return empty map
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_exact_minimum_length() {
        let data = create_test_data(2, "SN12345678901234", 400, 1, 2, true);
        assert_eq!(data.len(), MIN_TAG2010_LENGTH);

        let result = parse_sony_tag2010(&data, true);

        // All fields should be parsed
        assert!(result.get("Sony:ExposureProgram").is_some());
        assert!(result.get("Sony:InternalSerialNumber").is_some());
        assert!(result.get("Sony:RecommendedExposureIndex").is_some());
        assert!(result.get("Sony:Compression").is_some());
        assert!(result.get("Sony:Quality").is_some());
    }

    #[test]
    fn test_parse_all_fields() {
        let data = create_test_data(3, "TEST12345678", 1600, 4, 2, true);
        let result = parse_sony_tag2010(&data, true);

        // Verify all fields are correctly parsed
        assert_eq!(
            result.get("Sony:ExposureProgram").unwrap().as_string(),
            Some("Aperture Priority")
        );
        assert_eq!(
            result.get("Sony:InternalSerialNumber").unwrap().as_string(),
            Some("TEST12345678")
        );
        assert_eq!(
            result
                .get("Sony:RecommendedExposureIndex")
                .unwrap()
                .as_integer(),
            Some(1600)
        );
        assert_eq!(
            result.get("Sony:Compression").unwrap().as_string(),
            Some("RAW + JPEG")
        );
        assert_eq!(
            result.get("Sony:Quality").unwrap().as_string(),
            Some("Fine")
        );
    }

    // -------------------------------------------------------------------------
    // Byte Order Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_read_u16_little_endian() {
        let data = [0x01, 0x02, 0x03, 0x04];
        assert_eq!(read_u16(&data, 0, true), Some(0x0201)); // LE: 01 02 -> 0x0201
        assert_eq!(read_u16(&data, 2, true), Some(0x0403)); // LE: 03 04 -> 0x0403
    }

    #[test]
    fn test_read_u16_big_endian() {
        let data = [0x01, 0x02, 0x03, 0x04];
        assert_eq!(read_u16(&data, 0, false), Some(0x0102)); // BE: 01 02 -> 0x0102
        assert_eq!(read_u16(&data, 2, false), Some(0x0304)); // BE: 03 04 -> 0x0304
    }

    #[test]
    fn test_read_u16_out_of_bounds() {
        let data = [0x01, 0x02];
        assert_eq!(read_u16(&data, 1, true), None); // Only 1 byte available
        assert_eq!(read_u16(&data, 2, true), None); // 0 bytes available
    }

    #[test]
    fn test_read_u32_little_endian() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        assert_eq!(read_u32(&data, 0, true), Some(0x04030201)); // LE
        assert_eq!(read_u32(&data, 2, true), Some(0x06050403)); // LE
    }

    #[test]
    fn test_read_u32_big_endian() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        assert_eq!(read_u32(&data, 0, false), Some(0x01020304)); // BE
        assert_eq!(read_u32(&data, 2, false), Some(0x03040506)); // BE
    }

    #[test]
    fn test_read_u32_out_of_bounds() {
        let data = [0x01, 0x02, 0x03];
        assert_eq!(read_u32(&data, 0, true), None); // Only 3 bytes available
    }

    #[test]
    fn test_read_ascii_string_basic() {
        let data = b"Hello World";
        assert_eq!(read_ascii_string(data, 0, 5), Some("Hello".to_string()));
        assert_eq!(read_ascii_string(data, 6, 5), Some("World".to_string()));
    }

    #[test]
    fn test_read_ascii_string_with_null() {
        let data = b"Hello\0World";
        assert_eq!(
            read_ascii_string(data, 0, 11),
            Some("Hello".to_string()) // Stops at null
        );
    }

    #[test]
    fn test_read_ascii_string_filters_non_printable() {
        let data = [0x48, 0x65, 0x01, 0x6C, 0x6C, 0x6F]; // "He" + 0x01 + "llo"
        assert_eq!(
            read_ascii_string(&data, 0, 6),
            Some("Hello".to_string()) // 0x01 filtered out
        );
    }

    #[test]
    fn test_read_ascii_string_out_of_bounds() {
        let data = b"Hi";
        assert_eq!(read_ascii_string(data, 0, 10), None); // Requested length exceeds data
    }

    #[test]
    fn test_read_ascii_string_empty_result() {
        let data = [0x01, 0x02, 0x03]; // All non-printable
        assert_eq!(read_ascii_string(&data, 0, 3), None); // All filtered out
    }
}
