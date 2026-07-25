//! EXIF Thumbnail Extractor
//!
//! This module extracts thumbnail data from EXIF IFD1 (the second Image File Directory
//! in a TIFF/EXIF structure). Many digital cameras embed a small JPEG thumbnail image
//! in the EXIF data for quick preview purposes.
//!
//! # EXIF Thumbnail Tags
//!
//! The thumbnail is typically stored as embedded JPEG data, referenced by two tags:
//! - **JPEGInterchangeFormat (0x0201)**: Offset to the thumbnail JPEG data
//! - **JPEGInterchangeFormatLength (0x0202)**: Length of the thumbnail JPEG data in bytes
//!
//! # Output Tags
//!
//! This extractor produces three metadata tags:
//! - `EXIF:ThumbnailImage`: The raw binary JPEG thumbnail data
//! - `EXIF:ThumbnailOffset`: The file offset where the thumbnail begins
//! - `EXIF:ThumbnailLength`: The size of the thumbnail in bytes
//!
//! # Example
//!
//! ```no_run
//! use oxidex::parsers::tiff::thumbnail_extractor::extract_thumbnail;
//!
//! # fn example() -> oxidex::core::MetadataMap {
//! // Example: Extract thumbnail from IFD1 at offset 1024
//! let file_data: &[u8] = &[/* TIFF/EXIF file data */];
//! let ifd1_offset: usize = 1024;
//! let is_little_endian: bool = true;
//!
//! let metadata = extract_thumbnail(file_data, ifd1_offset, is_little_endian);
//! # metadata
//! # }
//! ```
//!
//! # References
//!
//! - EXIF 2.3 Specification Section 4.6.3 (Thumbnail Data)
//! - TIFF 6.0 Specification (IFD structure)

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;

// =============================================================================
// CONSTANTS: EXIF Tag IDs for thumbnail-related tags
// =============================================================================

/// Tag ID for JPEGInterchangeFormat (0x0201)
/// Contains the offset from TIFF header to the start of thumbnail JPEG data
const TAG_JPEG_INTERCHANGE_FORMAT: u16 = 0x0201;

/// Tag ID for JPEGInterchangeFormatLength (0x0202)
/// Contains the byte length of the thumbnail JPEG data
const TAG_JPEG_INTERCHANGE_FORMAT_LENGTH: u16 = 0x0202;

// =============================================================================
// CONSTANTS: Minimum sizes for validation
// =============================================================================

/// Minimum size for a valid IFD (2-byte entry count + 4-byte next IFD pointer)
const MIN_IFD_SIZE: usize = 6;

/// Size of a single IFD entry (12 bytes)
/// Structure: tag_id(2) + type(2) + count(4) + value/offset(4)
const IFD_ENTRY_SIZE: usize = 12;

/// Minimum valid thumbnail size (JPEG SOI + EOI markers)
/// A valid JPEG must have at least 2 bytes for SOI and 2 bytes for EOI
const MIN_THUMBNAIL_SIZE: usize = 4;

/// Maximum reasonable thumbnail size (1 MB)
/// Thumbnails larger than this are likely corrupt or invalid data
const MAX_THUMBNAIL_SIZE: usize = 1024 * 1024;

// =============================================================================
// MAIN EXTRACTION FUNCTION
// =============================================================================

/// Extracts thumbnail data from an EXIF IFD1 structure.
///
/// This function parses the IFD at the specified offset to find the thumbnail
/// location tags (JPEGInterchangeFormat and JPEGInterchangeFormatLength), then
/// extracts the thumbnail binary data and returns it as metadata entries.
///
/// # Arguments
///
/// * `data` - Complete TIFF/EXIF file data as a byte slice. The offsets found
///   in the IFD are relative to the start of this data.
/// * `ifd1_offset` - Byte offset from the start of `data` to IFD1 (the thumbnail IFD).
///   This offset points to the 2-byte entry count field of the IFD.
/// * `byte_order` - Byte order (endianness) flag. Use `true` for little-endian (Intel, "II"),
///   `false` for big-endian (Motorola, "MM").
///
/// # Returns
///
/// A `MetadataMap` containing extracted thumbnail metadata:
/// - `EXIF:ThumbnailOffset` (Integer): File offset where thumbnail data begins
/// - `EXIF:ThumbnailLength` (Integer): Size of thumbnail data in bytes
/// - `EXIF:ThumbnailImage` (Binary): The raw thumbnail JPEG data
///
/// Returns an empty `MetadataMap` if:
/// - The IFD offset is invalid or beyond data bounds
/// - The required thumbnail tags are not found
/// - The thumbnail data is invalid (wrong size, out of bounds, etc.)
///
/// # Example
///
/// ```no_run
/// use oxidex::parsers::tiff::thumbnail_extractor::extract_thumbnail;
///
/// // Assuming we have TIFF data with IFD1 at offset 1024
/// let tiff_data: Vec<u8> = vec![/* ... */];
/// let metadata = extract_thumbnail(&tiff_data, 1024, true);
///
/// if let Some(thumb) = metadata.get("EXIF:ThumbnailImage") {
///     println!("Found thumbnail!");
/// }
/// ```
///
/// # Performance
///
/// This function performs a single scan through the IFD entries and a single
/// data copy for the thumbnail bytes. Memory allocation is proportional to
/// the thumbnail size (typically 5-30 KB for most cameras).
pub fn extract_thumbnail(data: &[u8], ifd1_offset: usize, byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Validate that we have enough data to read the IFD entry count
    if ifd1_offset + MIN_IFD_SIZE > data.len() {
        // IFD offset is beyond available data; return empty result
        return metadata;
    }

    // Parse the IFD entry count (first 2 bytes at the IFD offset)
    let entry_count = read_u16(&data[ifd1_offset..], byte_order);

    // Calculate total IFD size and validate it fits within data
    // IFD structure: entry_count(2) + entries(12 each) + next_ifd_offset(4)
    let ifd_total_size = 2 + (entry_count as usize * IFD_ENTRY_SIZE) + 4;
    if ifd1_offset + ifd_total_size > data.len() {
        // IFD extends beyond available data; return empty result
        return metadata;
    }

    // Variables to store the thumbnail location tags as we find them
    let mut thumbnail_offset: Option<u32> = None;
    let mut thumbnail_length: Option<u32> = None;

    // Parse each IFD entry looking for thumbnail location tags
    let entries_start = ifd1_offset + 2; // Skip the 2-byte entry count
    for i in 0..entry_count as usize {
        let entry_offset = entries_start + (i * IFD_ENTRY_SIZE);

        // Read the tag ID (first 2 bytes of the 12-byte entry)
        let tag_id = read_u16(&data[entry_offset..], byte_order);

        // Check if this is one of the thumbnail location tags
        match tag_id {
            TAG_JPEG_INTERCHANGE_FORMAT => {
                // Extract the offset value from the IFD entry
                thumbnail_offset = extract_long_value(data, entry_offset, byte_order);
            }
            TAG_JPEG_INTERCHANGE_FORMAT_LENGTH => {
                // Extract the length value from the IFD entry
                thumbnail_length = extract_long_value(data, entry_offset, byte_order);
            }
            _ => {
                // Other tags are ignored for thumbnail extraction
            }
        }

        // Early exit if we've found both required tags
        if thumbnail_offset.is_some() && thumbnail_length.is_some() {
            break;
        }
    }

    // Both offset and length must be present to extract the thumbnail
    let (offset, length) = match (thumbnail_offset, thumbnail_length) {
        (Some(o), Some(l)) => (o as usize, l as usize),
        _ => return metadata, // Missing one or both tags; return empty result
    };

    // Validate thumbnail parameters before attempting extraction
    if !is_valid_thumbnail(data, offset, length) {
        return metadata;
    }

    // Extract the thumbnail binary data
    let thumbnail_data = data[offset..offset + length].to_vec();

    // Add all three metadata entries to the result
    metadata.insert("EXIF:ThumbnailOffset", TagValue::new_integer(offset as i64));
    metadata.insert("EXIF:ThumbnailLength", TagValue::new_integer(length as i64));
    metadata.insert("EXIF:ThumbnailImage", TagValue::new_binary(thumbnail_data));

    metadata
}

// =============================================================================
// HELPER FUNCTIONS: Byte Order-Aware Integer Reading
// =============================================================================

/// Reads a 16-bit unsigned integer from a byte slice with specified byte order.
///
/// # Arguments
///
/// * `data` - Byte slice containing at least 2 bytes
/// * `little_endian` - `true` for little-endian, `false` for big-endian
///
/// # Returns
///
/// The 16-bit value interpreted according to the byte order.
///
/// # Panics
///
/// Panics if `data` contains fewer than 2 bytes. Callers must ensure
/// sufficient data is available before calling.
#[inline]
fn read_u16(data: &[u8], little_endian: bool) -> u16 {
    if little_endian {
        u16::from_le_bytes([data[0], data[1]])
    } else {
        u16::from_be_bytes([data[0], data[1]])
    }
}

/// Reads a 32-bit unsigned integer from a byte slice with specified byte order.
///
/// # Arguments
///
/// * `data` - Byte slice containing at least 4 bytes
/// * `little_endian` - `true` for little-endian, `false` for big-endian
///
/// # Returns
///
/// The 32-bit value interpreted according to the byte order.
///
/// # Panics
///
/// Panics if `data` contains fewer than 4 bytes. Callers must ensure
/// sufficient data is available before calling.
#[inline]
fn read_u32(data: &[u8], little_endian: bool) -> u32 {
    if little_endian {
        u32::from_le_bytes([data[0], data[1], data[2], data[3]])
    } else {
        u32::from_be_bytes([data[0], data[1], data[2], data[3]])
    }
}

// =============================================================================
// HELPER FUNCTIONS: IFD Value Extraction
// =============================================================================

/// Extracts a LONG (u32) value from an IFD entry.
///
/// IFD entries for thumbnail offset/length use LONG type (4 bytes).
/// The value is stored in bytes 8-11 of the 12-byte IFD entry structure.
///
/// # Arguments
///
/// * `data` - Complete file data
/// * `entry_offset` - Offset to the start of the IFD entry (12 bytes)
/// * `little_endian` - Byte order flag
///
/// # Returns
///
/// `Some(value)` if the entry contains a valid LONG type with count 1,
/// `None` if the type or count is unexpected.
///
/// # IFD Entry Structure
///
/// ```text
/// Bytes 0-1:  Tag ID (u16)
/// Bytes 2-3:  Type (u16) - should be 4 (LONG) for these tags
/// Bytes 4-7:  Count (u32) - should be 1
/// Bytes 8-11: Value (u32) - the actual offset or length
/// ```
fn extract_long_value(data: &[u8], entry_offset: usize, little_endian: bool) -> Option<u32> {
    // Ensure we have enough bytes for the full IFD entry
    if entry_offset + IFD_ENTRY_SIZE > data.len() {
        return None;
    }

    let entry = &data[entry_offset..entry_offset + IFD_ENTRY_SIZE];

    // Read the field type (bytes 2-3)
    // Type 4 = LONG (4-byte unsigned integer) per TIFF specification
    let field_type = read_u16(&entry[2..], little_endian);
    if field_type != 4 {
        // Also accept type 3 (SHORT) which some cameras use
        if field_type == 3 {
            // SHORT type: value is 2 bytes, stored in first 2 bytes of value field
            let value = read_u16(&entry[8..], little_endian) as u32;
            return Some(value);
        }
        return None; // Unexpected type
    }

    // Read the value count (bytes 4-7)
    // Should be 1 for offset and length tags
    let count = read_u32(&entry[4..], little_endian);
    if count != 1 {
        return None; // Unexpected count
    }

    // Read the actual value (bytes 8-11)
    let value = read_u32(&entry[8..], little_endian);
    Some(value)
}

// =============================================================================
// HELPER FUNCTIONS: Validation
// =============================================================================

/// Validates that thumbnail parameters are reasonable and the data is extractable.
///
/// # Validation Checks
///
/// 1. Length is within acceptable bounds (MIN_THUMBNAIL_SIZE to MAX_THUMBNAIL_SIZE)
/// 2. Offset + Length does not exceed available data
/// 3. (Optional) Data starts with JPEG SOI marker (0xFFD8)
///
/// # Arguments
///
/// * `data` - Complete file data
/// * `offset` - Claimed thumbnail offset
/// * `length` - Claimed thumbnail length
///
/// # Returns
///
/// `true` if the thumbnail appears valid and can be safely extracted,
/// `false` otherwise.
fn is_valid_thumbnail(data: &[u8], offset: usize, length: usize) -> bool {
    // Check minimum size
    if length < MIN_THUMBNAIL_SIZE {
        return false;
    }

    // Check maximum size (protection against corrupt data)
    if length > MAX_THUMBNAIL_SIZE {
        return false;
    }

    // Check bounds: offset + length must not exceed data size
    // Using checked_add to prevent overflow attacks
    let end = match offset.checked_add(length) {
        Some(e) => e,
        None => return false, // Overflow detected
    };

    if end > data.len() {
        return false;
    }

    // Optional: Verify JPEG magic bytes (SOI marker)
    // This provides additional confidence but is not strictly required
    // as some cameras may store other thumbnail formats
    if data.len() >= offset + 2 {
        let soi = &data[offset..offset + 2];
        if soi != [0xFF, 0xD8] {
            // Not a JPEG thumbnail; still might be valid (e.g., TIFF strip)
            // but we only extract JPEG thumbnails in this implementation
            // Return false for now; could be made configurable in the future
            return false;
        }
    }

    true
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Test Data Builders
    // -------------------------------------------------------------------------

    /// Creates a minimal JPEG file (just SOI and EOI markers).
    fn create_minimal_jpeg() -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xD9] // SOI + EOI
    }

    /// Creates a more realistic test JPEG with some content.
    fn create_test_jpeg() -> Vec<u8> {
        let jpeg = vec![
            0xFF, 0xD8, // SOI (Start Of Image)
            0xFF, 0xE0, // APP0 marker
            0x00, 0x10, // APP0 length (16 bytes)
            b'J', b'F', b'I', b'F', 0x00, // JFIF identifier
            0x01, 0x01, // Version 1.1
            0x00, // Aspect ratio units: no units
            0x00, 0x01, // X density: 1
            0x00, 0x01, // Y density: 1
            0x00, 0x00, // No thumbnail
            0xFF, 0xD9, // EOI (End Of Image)
        ];
        jpeg
    }

    /// Creates a sample IFD with thumbnail offset and length tags (little-endian).
    ///
    /// Layout:
    /// - Bytes 0-1: Entry count (2 entries)
    /// - Bytes 2-13: First entry (JPEGInterchangeFormat)
    /// - Bytes 14-25: Second entry (JPEGInterchangeFormatLength)
    /// - Bytes 26-29: Next IFD offset (0)
    fn create_test_ifd_le(thumbnail_offset: u32, thumbnail_length: u32) -> Vec<u8> {
        let mut ifd = Vec::new();

        // Entry count: 2 (little-endian)
        ifd.extend_from_slice(&2u16.to_le_bytes());

        // Entry 1: JPEGInterchangeFormat (0x0201)
        // Tag ID
        ifd.extend_from_slice(&TAG_JPEG_INTERCHANGE_FORMAT.to_le_bytes());
        // Type: LONG (4)
        ifd.extend_from_slice(&4u16.to_le_bytes());
        // Count: 1
        ifd.extend_from_slice(&1u32.to_le_bytes());
        // Value: offset
        ifd.extend_from_slice(&thumbnail_offset.to_le_bytes());

        // Entry 2: JPEGInterchangeFormatLength (0x0202)
        // Tag ID
        ifd.extend_from_slice(&TAG_JPEG_INTERCHANGE_FORMAT_LENGTH.to_le_bytes());
        // Type: LONG (4)
        ifd.extend_from_slice(&4u16.to_le_bytes());
        // Count: 1
        ifd.extend_from_slice(&1u32.to_le_bytes());
        // Value: length
        ifd.extend_from_slice(&thumbnail_length.to_le_bytes());

        // Next IFD offset: 0 (no next IFD)
        ifd.extend_from_slice(&0u32.to_le_bytes());

        ifd
    }

    /// Creates a sample IFD with thumbnail tags (big-endian).
    fn create_test_ifd_be(thumbnail_offset: u32, thumbnail_length: u32) -> Vec<u8> {
        let mut ifd = Vec::new();

        // Entry count: 2 (big-endian)
        ifd.extend_from_slice(&2u16.to_be_bytes());

        // Entry 1: JPEGInterchangeFormat
        ifd.extend_from_slice(&TAG_JPEG_INTERCHANGE_FORMAT.to_be_bytes());
        ifd.extend_from_slice(&4u16.to_be_bytes()); // Type: LONG
        ifd.extend_from_slice(&1u32.to_be_bytes()); // Count: 1
        ifd.extend_from_slice(&thumbnail_offset.to_be_bytes());

        // Entry 2: JPEGInterchangeFormatLength
        ifd.extend_from_slice(&TAG_JPEG_INTERCHANGE_FORMAT_LENGTH.to_be_bytes());
        ifd.extend_from_slice(&4u16.to_be_bytes()); // Type: LONG
        ifd.extend_from_slice(&1u32.to_be_bytes()); // Count: 1
        ifd.extend_from_slice(&thumbnail_length.to_be_bytes());

        // Next IFD offset: 0
        ifd.extend_from_slice(&0u32.to_be_bytes());

        ifd
    }

    // -------------------------------------------------------------------------
    // Core Extraction Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_thumbnail_little_endian() {
        // Create test JPEG thumbnail
        let jpeg = create_test_jpeg();
        let jpeg_len = jpeg.len() as u32;

        // Create IFD pointing to thumbnail at offset 100
        let ifd = create_test_ifd_le(100, jpeg_len);

        // Build complete test data: padding + IFD + more padding + JPEG
        let mut data = vec![0u8; 200];
        // Place IFD at offset 0
        data[..ifd.len()].copy_from_slice(&ifd);
        // Place JPEG at offset 100
        data[100..100 + jpeg.len()].copy_from_slice(&jpeg);

        // Extract thumbnail
        let metadata = extract_thumbnail(&data, 0, true);

        // Verify all three expected tags are present
        assert!(metadata.contains_key("EXIF:ThumbnailOffset"));
        assert!(metadata.contains_key("EXIF:ThumbnailLength"));
        assert!(metadata.contains_key("EXIF:ThumbnailImage"));

        // Verify offset value
        assert_eq!(metadata.get_integer("EXIF:ThumbnailOffset"), Some(100));

        // Verify length value
        assert_eq!(
            metadata.get_integer("EXIF:ThumbnailLength"),
            Some(jpeg_len as i64)
        );

        // Verify the actual thumbnail data
        if let Some(TagValue::Binary(thumb_data)) = metadata.get("EXIF:ThumbnailImage") {
            assert_eq!(thumb_data, &jpeg);
            // Verify JPEG markers
            assert_eq!(thumb_data[0], 0xFF);
            assert_eq!(thumb_data[1], 0xD8);
        } else {
            panic!("ThumbnailImage should be Binary type");
        }
    }

    #[test]
    fn test_extract_thumbnail_big_endian() {
        let jpeg = create_minimal_jpeg();
        let jpeg_len = jpeg.len() as u32;

        let ifd = create_test_ifd_be(50, jpeg_len);

        let mut data = vec![0u8; 100];
        data[..ifd.len()].copy_from_slice(&ifd);
        data[50..50 + jpeg.len()].copy_from_slice(&jpeg);

        let metadata = extract_thumbnail(&data, 0, false);

        assert_eq!(metadata.get_integer("EXIF:ThumbnailOffset"), Some(50));
        assert_eq!(
            metadata.get_integer("EXIF:ThumbnailLength"),
            Some(jpeg_len as i64)
        );

        if let Some(TagValue::Binary(thumb_data)) = metadata.get("EXIF:ThumbnailImage") {
            assert_eq!(thumb_data, &jpeg);
        } else {
            panic!("ThumbnailImage should be Binary type");
        }
    }

    #[test]
    fn test_extract_thumbnail_with_ifd_offset() {
        // Test extraction when IFD is not at offset 0
        let jpeg = create_minimal_jpeg();
        let jpeg_len = jpeg.len() as u32;

        // IFD at offset 50, thumbnail at offset 150
        let ifd = create_test_ifd_le(150, jpeg_len);

        let mut data = vec![0u8; 200];
        data[50..50 + ifd.len()].copy_from_slice(&ifd);
        data[150..150 + jpeg.len()].copy_from_slice(&jpeg);

        let metadata = extract_thumbnail(&data, 50, true);

        assert_eq!(metadata.get_integer("EXIF:ThumbnailOffset"), Some(150));
        assert!(metadata.contains_key("EXIF:ThumbnailImage"));
    }

    // -------------------------------------------------------------------------
    // Edge Case Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_thumbnail_missing_offset_tag() {
        // Create IFD with only the length tag (missing offset)
        let mut ifd = Vec::new();
        ifd.extend_from_slice(&1u16.to_le_bytes()); // 1 entry only

        // Only JPEGInterchangeFormatLength tag
        ifd.extend_from_slice(&TAG_JPEG_INTERCHANGE_FORMAT_LENGTH.to_le_bytes());
        ifd.extend_from_slice(&4u16.to_le_bytes());
        ifd.extend_from_slice(&1u32.to_le_bytes());
        ifd.extend_from_slice(&100u32.to_le_bytes());

        ifd.extend_from_slice(&0u32.to_le_bytes()); // Next IFD

        let mut data = vec![0u8; 50];
        data[..ifd.len()].copy_from_slice(&ifd);

        let metadata = extract_thumbnail(&data, 0, true);

        // Should return empty metadata when offset tag is missing
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_extract_thumbnail_missing_length_tag() {
        // Create IFD with only the offset tag (missing length)
        let mut ifd = Vec::new();
        ifd.extend_from_slice(&1u16.to_le_bytes()); // 1 entry only

        // Only JPEGInterchangeFormat tag
        ifd.extend_from_slice(&TAG_JPEG_INTERCHANGE_FORMAT.to_le_bytes());
        ifd.extend_from_slice(&4u16.to_le_bytes());
        ifd.extend_from_slice(&1u32.to_le_bytes());
        ifd.extend_from_slice(&50u32.to_le_bytes());

        ifd.extend_from_slice(&0u32.to_le_bytes()); // Next IFD

        let mut data = vec![0u8; 100];
        data[..ifd.len()].copy_from_slice(&ifd);

        let metadata = extract_thumbnail(&data, 0, true);

        // Should return empty metadata when length tag is missing
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_extract_thumbnail_empty_ifd() {
        // IFD with zero entries
        let mut ifd = Vec::new();
        ifd.extend_from_slice(&0u16.to_le_bytes()); // 0 entries
        ifd.extend_from_slice(&0u32.to_le_bytes()); // Next IFD

        let mut data = vec![0u8; 50];
        data[..ifd.len()].copy_from_slice(&ifd);

        let metadata = extract_thumbnail(&data, 0, true);

        assert!(metadata.is_empty());
    }

    #[test]
    fn test_extract_thumbnail_ifd_offset_beyond_data() {
        let data = vec![0u8; 50];

        // IFD offset beyond available data
        let metadata = extract_thumbnail(&data, 100, true);

        assert!(metadata.is_empty());
    }

    #[test]
    fn test_extract_thumbnail_out_of_bounds() {
        let jpeg = create_minimal_jpeg();

        // IFD claims thumbnail at offset 1000, but data is only 100 bytes
        let ifd = create_test_ifd_le(1000, jpeg.len() as u32);

        let mut data = vec![0u8; 100];
        data[..ifd.len()].copy_from_slice(&ifd);

        let metadata = extract_thumbnail(&data, 0, true);

        // Should return empty due to bounds check failure
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_extract_thumbnail_too_small() {
        // Thumbnail length less than minimum (4 bytes)
        let ifd = create_test_ifd_le(50, 2); // Only 2 bytes

        let mut data = vec![0u8; 100];
        // Place JPEG SOI marker at the expected thumbnail offset
        data[50] = 0xFF;
        data[51] = 0xD8;
        data[..ifd.len()].copy_from_slice(&ifd);

        let metadata = extract_thumbnail(&data, 0, true);

        assert!(metadata.is_empty());
    }

    #[test]
    fn test_extract_thumbnail_not_jpeg() {
        // Data that doesn't start with JPEG SOI marker
        let ifd = create_test_ifd_le(50, 10);

        let mut data = vec![0u8; 100];
        data[..ifd.len()].copy_from_slice(&ifd);
        // Put non-JPEG data at thumbnail offset
        data[50..60].copy_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09]);

        let metadata = extract_thumbnail(&data, 0, true);

        // Should reject non-JPEG data
        assert!(metadata.is_empty());
    }

    // -------------------------------------------------------------------------
    // Helper Function Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_read_u16_little_endian() {
        let data = [0x34, 0x12];
        assert_eq!(read_u16(&data, true), 0x1234);
    }

    #[test]
    fn test_read_u16_big_endian() {
        let data = [0x12, 0x34];
        assert_eq!(read_u16(&data, false), 0x1234);
    }

    #[test]
    fn test_read_u32_little_endian() {
        let data = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_u32(&data, true), 0x12345678);
    }

    #[test]
    fn test_read_u32_big_endian() {
        let data = [0x12, 0x34, 0x56, 0x78];
        assert_eq!(read_u32(&data, false), 0x12345678);
    }

    #[test]
    fn test_is_valid_thumbnail_valid() {
        let mut data = vec![0u8; 100];
        // Place JPEG SOI marker at offset 10
        data[10] = 0xFF;
        data[11] = 0xD8;

        assert!(is_valid_thumbnail(&data, 10, 50));
    }

    #[test]
    fn test_is_valid_thumbnail_too_small() {
        let mut data = vec![0u8; 100];
        data[0] = 0xFF;
        data[1] = 0xD8;
        assert!(!is_valid_thumbnail(&data, 0, 2)); // Below MIN_THUMBNAIL_SIZE
    }

    #[test]
    fn test_is_valid_thumbnail_out_of_bounds() {
        let mut data = vec![0u8; 50];
        data[40] = 0xFF;
        data[41] = 0xD8;
        assert!(!is_valid_thumbnail(&data, 40, 20)); // 40 + 20 > 50
    }

    #[test]
    fn test_is_valid_thumbnail_overflow_protection() {
        let mut data = vec![0u8; 100];
        data[0] = 0xFF;
        data[1] = 0xD8;
        // Try to trigger integer overflow with very large length
        assert!(!is_valid_thumbnail(&data, usize::MAX - 10, 100));
    }

    #[test]
    fn test_extract_long_value_valid() {
        // Build a valid IFD entry with LONG type
        let mut entry = vec![0u8; 12];
        // Tag ID (ignored for this test)
        entry[0..2].copy_from_slice(&0x0201u16.to_le_bytes());
        // Type: LONG (4)
        entry[2..4].copy_from_slice(&4u16.to_le_bytes());
        // Count: 1
        entry[4..8].copy_from_slice(&1u32.to_le_bytes());
        // Value: 12345
        entry[8..12].copy_from_slice(&12345u32.to_le_bytes());

        let result = extract_long_value(&entry, 0, true);
        assert_eq!(result, Some(12345));
    }

    #[test]
    fn test_extract_long_value_wrong_type() {
        // Build an entry with wrong type (ASCII instead of LONG)
        let mut entry = vec![0u8; 12];
        entry[0..2].copy_from_slice(&0x0201u16.to_le_bytes());
        entry[2..4].copy_from_slice(&2u16.to_le_bytes()); // Type: ASCII (2)
        entry[4..8].copy_from_slice(&1u32.to_le_bytes());
        entry[8..12].copy_from_slice(&12345u32.to_le_bytes());

        let result = extract_long_value(&entry, 0, true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_long_value_short_type() {
        // Some cameras use SHORT type instead of LONG
        let mut entry = vec![0u8; 12];
        entry[0..2].copy_from_slice(&0x0201u16.to_le_bytes());
        entry[2..4].copy_from_slice(&3u16.to_le_bytes()); // Type: SHORT (3)
        entry[4..8].copy_from_slice(&1u32.to_le_bytes());
        entry[8..10].copy_from_slice(&1000u16.to_le_bytes()); // Value as SHORT

        let result = extract_long_value(&entry, 0, true);
        assert_eq!(result, Some(1000));
    }

    // -------------------------------------------------------------------------
    // Integration-style Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_realistic_thumbnail_extraction() {
        // Simulate a more realistic TIFF structure:
        // - TIFF header at 0-7
        // - IFD0 at offset 8
        // - IFD1 at offset 100 (thumbnail IFD)
        // - Thumbnail JPEG at offset 200

        let jpeg = create_test_jpeg();
        let jpeg_len = jpeg.len() as u32;

        // Build IFD1 at offset 100
        let ifd1 = create_test_ifd_le(200, jpeg_len);

        let mut data = vec![0u8; 300];

        // Simple TIFF header (little-endian)
        data[0..2].copy_from_slice(&[0x49, 0x49]); // "II" = little-endian
        data[2..4].copy_from_slice(&42u16.to_le_bytes()); // Magic number
        data[4..8].copy_from_slice(&8u32.to_le_bytes()); // IFD0 offset

        // Place IFD1 at offset 100
        data[100..100 + ifd1.len()].copy_from_slice(&ifd1);

        // Place thumbnail JPEG at offset 200
        data[200..200 + jpeg.len()].copy_from_slice(&jpeg);

        // Extract thumbnail from IFD1
        let metadata = extract_thumbnail(&data, 100, true);

        assert_eq!(metadata.len(), 3);
        assert_eq!(metadata.get_integer("EXIF:ThumbnailOffset"), Some(200));
        assert_eq!(
            metadata.get_integer("EXIF:ThumbnailLength"),
            Some(jpeg_len as i64)
        );

        if let Some(TagValue::Binary(thumb)) = metadata.get("EXIF:ThumbnailImage") {
            assert_eq!(thumb.len(), jpeg.len());
            // Verify it's a valid JPEG
            assert_eq!(&thumb[0..2], &[0xFF, 0xD8]); // SOI
            assert_eq!(&thumb[thumb.len() - 2..], &[0xFF, 0xD9]); // EOI
        } else {
            panic!("Expected Binary thumbnail data");
        }
    }

    #[test]
    fn test_thumbnail_with_many_ifd_entries() {
        // Test that extraction works when thumbnail tags are among many other tags
        let jpeg = create_minimal_jpeg();
        let jpeg_len = jpeg.len() as u32;

        let mut ifd = Vec::new();
        // 5 entries
        ifd.extend_from_slice(&5u16.to_le_bytes());

        // Entry 1: Random tag (0x0100 - ImageWidth)
        ifd.extend_from_slice(&0x0100u16.to_le_bytes());
        ifd.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        ifd.extend_from_slice(&1u32.to_le_bytes());
        ifd.extend_from_slice(&640u32.to_le_bytes());

        // Entry 2: Random tag (0x0101 - ImageHeight)
        ifd.extend_from_slice(&0x0101u16.to_le_bytes());
        ifd.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        ifd.extend_from_slice(&1u32.to_le_bytes());
        ifd.extend_from_slice(&480u32.to_le_bytes());

        // Entry 3: JPEGInterchangeFormat
        ifd.extend_from_slice(&TAG_JPEG_INTERCHANGE_FORMAT.to_le_bytes());
        ifd.extend_from_slice(&4u16.to_le_bytes());
        ifd.extend_from_slice(&1u32.to_le_bytes());
        ifd.extend_from_slice(&150u32.to_le_bytes());

        // Entry 4: Random tag (0x0103 - Compression)
        ifd.extend_from_slice(&0x0103u16.to_le_bytes());
        ifd.extend_from_slice(&3u16.to_le_bytes());
        ifd.extend_from_slice(&1u32.to_le_bytes());
        ifd.extend_from_slice(&6u32.to_le_bytes()); // JPEG compression

        // Entry 5: JPEGInterchangeFormatLength
        ifd.extend_from_slice(&TAG_JPEG_INTERCHANGE_FORMAT_LENGTH.to_le_bytes());
        ifd.extend_from_slice(&4u16.to_le_bytes());
        ifd.extend_from_slice(&1u32.to_le_bytes());
        ifd.extend_from_slice(&jpeg_len.to_le_bytes());

        // Next IFD offset
        ifd.extend_from_slice(&0u32.to_le_bytes());

        let mut data = vec![0u8; 200];
        data[..ifd.len()].copy_from_slice(&ifd);
        data[150..150 + jpeg.len()].copy_from_slice(&jpeg);

        let metadata = extract_thumbnail(&data, 0, true);

        assert_eq!(metadata.get_integer("EXIF:ThumbnailOffset"), Some(150));
        assert!(metadata.contains_key("EXIF:ThumbnailImage"));
    }
}
