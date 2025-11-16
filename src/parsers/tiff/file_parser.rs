//! TIFF file parser for standalone TIFF files
//!
//! This module handles parsing of complete TIFF file structures, including:
//! - 8-byte TIFF header (byte order marker, magic number, first IFD offset)
//! - IFD chain navigation (IFD0 → IFD1 → ... via next IFD offsets)
//! - Sub-IFD recursion (EXIF, GPS, Interoperability sub-IFDs)
//! - Multi-page TIFF support
//!
//! Unlike the IFD parser which handles individual IFD structures, this parser
//! handles the complete TIFF file format including header parsing and IFD chain
//! traversal.
//!
//! # TIFF File Structure
//!
//! A TIFF file consists of:
//!
//! ```text
//! Bytes 0-1:   Byte Order Marker
//!              0x4949 ("II") = Little-Endian
//!              0x4D4D ("MM") = Big-Endian
//! Bytes 2-3:   Magic Number 42 (0x002A in detected byte order)
//! Bytes 4-7:   Offset to first IFD (typically 8 for files with header-adjacent IFD)
//!
//! At each IFD offset:
//!   2 bytes:     Entry count (number of tags in this IFD)
//!   N×12 bytes:  Tag entries (12 bytes each)
//!   4 bytes:     Next IFD offset (0 if last IFD)
//! ```
//!
//! # IFD Chain
//!
//! TIFF files organize metadata in a chain of IFDs:
//! - **IFD0**: Main image metadata
//! - **IFD1**: Thumbnail metadata (optional)
//! - **IFDn**: Additional pages for multi-page TIFF
//!
//! Each IFD may also reference sub-IFDs via special pointer tags:
//! - **Tag 0x8769 (ExifIFDPointer)**: Points to EXIF sub-IFD
//! - **Tag 0x8825 (GPSInfoIFDPointer)**: Points to GPS sub-IFD
//! - **Tag 0xA005 (InteroperabilityIFDPointer)**: Points to Interoperability sub-IFD
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::parsers::tiff::file_parser::parse_tiff_file;
//! use exiftool_rs::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("sample.tif"))?;
//! let all_tags = parse_tiff_file(&reader)?;
//!
//! println!("Extracted {} tags from all IFDs", all_tags.len());
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder, IfdEntries};
use std::collections::HashSet;

/// TIFF header structure
///
/// The first 8 bytes of every TIFF file contain:
/// - Byte order marker (2 bytes)
/// - Magic number 42 (2 bytes)
/// - Offset to first IFD (4 bytes)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiffHeader {
    /// Byte order (endianness) for all multi-byte values in the file
    pub byte_order: ByteOrder,
    /// File offset to the first IFD (typically 8)
    pub first_ifd_offset: u32,
}

/// Special tag IDs that reference sub-IFDs
const EXIF_IFD_POINTER: u16 = 0x8769;
const GPS_INFO_IFD_POINTER: u16 = 0x8825;
const INTEROPERABILITY_IFD_POINTER: u16 = 0xA005;
const SUB_IFDS: u16 = 0x014A;

/// Parses the 8-byte TIFF file header.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing file data
///
/// # Returns
///
/// - `Ok(TiffHeader)`: Parsed header with byte order and first IFD offset
/// - `Err(ExifToolError)`: Invalid header format
///
/// # Errors
///
/// Returns an error if:
/// - File is smaller than 8 bytes
/// - Byte order marker is invalid (not 0x4949 or 0x4D4D)
/// - Magic number is not 42
///
/// # Example
///
/// ```no_run
/// # use exiftool_rs::parsers::tiff::file_parser::parse_tiff_header;
/// # use exiftool_rs::io::buffered_reader::BufferedReader;
/// # use std::path::Path;
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("sample.tif"))?;
/// let header = parse_tiff_header(&reader)?;
/// println!("First IFD at offset: {}", header.first_ifd_offset);
/// # Ok(())
/// # }
/// ```
pub fn parse_tiff_header(reader: &dyn FileReader) -> Result<TiffHeader> {
    // Ensure file is at least 8 bytes
    if reader.size() < 8 {
        return Err(ExifToolError::parse_error(
            "File too small to contain TIFF header (minimum 8 bytes)",
        ));
    }

    // Read 8-byte header
    let header = reader.read(0, 8)?;

    // Parse byte order marker (bytes 0-1)
    let byte_order = match &header[0..2] {
        [0x49, 0x49] => ByteOrder::LittleEndian,
        [0x4D, 0x4D] => ByteOrder::BigEndian,
        _ => {
            return Err(ExifToolError::parse_error(format!(
                "Invalid TIFF byte order marker: 0x{:02X}{:02X}",
                header[0], header[1]
            )))
        }
    };

    // Parse magic number (bytes 2-3) - should be 42
    let magic = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([header[2], header[3]]),
        ByteOrder::BigEndian => u16::from_be_bytes([header[2], header[3]]),
    };

    if magic != 42 {
        return Err(ExifToolError::parse_error(format!(
            "Invalid TIFF magic number: {} (expected 42)",
            magic
        )));
    }

    // Parse first IFD offset (bytes 4-7)
    let first_ifd_offset = match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([header[4], header[5], header[6], header[7]]),
        ByteOrder::BigEndian => u32::from_be_bytes([header[4], header[5], header[6], header[7]]),
    };

    Ok(TiffHeader {
        byte_order,
        first_ifd_offset,
    })
}

/// Reads the entry count at the start of an IFD.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing file data
/// - `ifd_offset`: Byte offset to the IFD
/// - `byte_order`: Endianness for parsing the 2-byte count
///
/// # Returns
///
/// - `Ok(u16)`: Number of entries in this IFD
/// - `Err(ExifToolError)`: I/O error or offset out of bounds
fn read_entry_count(
    reader: &dyn FileReader,
    ifd_offset: u64,
    byte_order: ByteOrder,
) -> Result<u16> {
    let data = reader.read(ifd_offset, 2)?;
    let count = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([data[0], data[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([data[0], data[1]]),
    };
    Ok(count)
}

/// Reads the "next IFD offset" field after an IFD's tag entries.
///
/// The next IFD offset is located immediately after the last tag entry:
/// `ifd_offset + 2 (entry count) + (entry_count × 12)`
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing file data
/// - `ifd_offset`: Byte offset to the IFD
/// - `entry_count`: Number of tag entries in this IFD
/// - `byte_order`: Endianness for parsing the 4-byte offset
///
/// # Returns
///
/// - `Ok(u32)`: Offset to next IFD, or 0 if this is the last IFD
/// - `Err(ExifToolError)`: I/O error or offset out of bounds
fn read_next_ifd_offset(
    reader: &dyn FileReader,
    ifd_offset: u64,
    entry_count: u16,
    byte_order: ByteOrder,
) -> Result<u32> {
    let next_offset_location = ifd_offset + 2 + (entry_count as u64 * 12);
    let data = reader.read(next_offset_location, 4)?;
    let offset = match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        ByteOrder::BigEndian => u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
    };
    Ok(offset)
}

/// Extracts a u32 value from tag value bytes.
///
/// Used to extract sub-IFD pointer offsets from tag values.
///
/// # Parameters
///
/// - `value`: Raw value bytes from a tag
/// - `byte_order`: Endianness for parsing multi-byte values
///
/// # Returns
///
/// - `Some(u32)`: Extracted offset value
/// - `None`: Value is too small to contain a u32
fn extract_u32_from_tag_value(value: &[u8], byte_order: ByteOrder) -> Option<u32> {
    if value.len() < 4 {
        return None;
    }

    let offset = match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([value[0], value[1], value[2], value[3]]),
        ByteOrder::BigEndian => u32::from_be_bytes([value[0], value[1], value[2], value[3]]),
    };

    Some(offset)
}

/// Parses a complete TIFF file and extracts all metadata tags.
///
/// This is the main entry point for TIFF file parsing. It:
/// 1. Parses the 8-byte TIFF header to determine byte order and first IFD location
/// 2. Walks the IFD chain (IFD0 → IFD1 → ... → IFDn) following next IFD offsets
/// 3. For each IFD, parses all tag entries
/// 4. Recursively parses sub-IFDs (EXIF, GPS, Interoperability) referenced by special tags
/// 5. Returns all tags from all IFDs as a flat vector
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing file data
///
/// # Returns
///
/// - `Ok(Vec<(u16, u16, u32, Cow<'static, [u8]>)>)`: Vector of (tag_id, field_type, value_count, raw_value_bytes) from all IFDs
/// - `Err(ExifToolError)`: Parse error, I/O error, or invalid file structure
///
/// # Errors
///
/// Returns an error if:
/// - TIFF header is invalid
/// - IFD offsets point beyond file size
/// - Circular IFD references detected
/// - Tag data is malformed
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::tiff::file_parser::parse_tiff_file;
/// use exiftool_rs::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("multi_page.tif"))?;
/// let all_tags = parse_tiff_file(&reader)?;
///
/// // Process all tags from all IFDs
/// for (tag_id, _, _, value) in all_tags {
///     println!("Tag 0x{:04X}: {} bytes", tag_id, value.len());
/// }
/// # Ok(())
/// # }
/// ```
pub fn parse_tiff_file(reader: &dyn FileReader) -> Result<IfdEntries> {
    // Parse header
    let header = parse_tiff_header(reader)?;
    let byte_order = header.byte_order;
    let mut current_offset = header.first_ifd_offset as u64;

    // Track visited IFD offsets to prevent infinite loops from circular references
    let mut visited_offsets = HashSet::new();

    // Collect all tags from all IFDs
    let mut all_tags = Vec::new();

    // Walk the IFD chain
    loop {
        // Check for circular reference
        if visited_offsets.contains(&current_offset) {
            return Err(ExifToolError::parse_error_at(
                "Circular IFD reference detected",
                current_offset as usize,
            ));
        }

        // Validate offset is within file bounds
        if current_offset >= reader.size() {
            return Err(ExifToolError::parse_error_at(
                format!(
                    "IFD offset {} exceeds file size {}",
                    current_offset,
                    reader.size()
                ),
                current_offset as usize,
            ));
        }

        // Mark this offset as visited
        visited_offsets.insert(current_offset);

        // Read entry count to calculate next IFD offset location
        let entry_count = read_entry_count(reader, current_offset, byte_order)?;

        // Parse this IFD and collect tags
        let tags = parse_ifd(reader, current_offset, byte_order)?;

        // Check for sub-IFD pointers and recursively parse them
        for (tag_id, _field_type, _value_count, value) in &tags {
            match *tag_id {
                EXIF_IFD_POINTER | GPS_INFO_IFD_POINTER | INTEROPERABILITY_IFD_POINTER => {
                    // Convert Cow<[u8]> to &[u8] using as_ref()
                    if let Some(sub_ifd_offset) = extract_u32_from_tag_value(value.as_ref(), byte_order) {
                        // Skip if we've already visited this offset
                        if !visited_offsets.contains(&(sub_ifd_offset as u64)) {
                            // Parse sub-IFD
                            match parse_ifd(reader, sub_ifd_offset as u64, byte_order) {
                                Ok(sub_tags) => {
                                    all_tags.extend(sub_tags);
                                    visited_offsets.insert(sub_ifd_offset as u64);
                                }
                                Err(e) => {
                                    // Log but don't fail - some files have invalid sub-IFD pointers
                                    eprintln!(
                                        "Warning: Failed to parse sub-IFD at offset {}: {}",
                                        sub_ifd_offset, e
                                    );
                                }
                            }
                        }
                    }
                }
                SUB_IFDS => {
                    // SubIFDs tag can contain multiple offsets
                    // Each offset is 4 bytes (u32)
                    // Convert Cow<[u8]> to &[u8] using as_ref()
                    let value_bytes = value.as_ref();
                    let offset_count = value_bytes.len() / 4;
                    for i in 0..offset_count {
                        let offset_bytes = &value_bytes[i * 4..(i + 1) * 4];
                        if let Some(sub_ifd_offset) =
                            extract_u32_from_tag_value(offset_bytes, byte_order)
                        {
                            if !visited_offsets.contains(&(sub_ifd_offset as u64)) {
                                match parse_ifd(reader, sub_ifd_offset as u64, byte_order) {
                                    Ok(sub_tags) => {
                                        all_tags.extend(sub_tags);
                                        visited_offsets.insert(sub_ifd_offset as u64);
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "Warning: Failed to parse sub-IFD at offset {}: {}",
                                            sub_ifd_offset, e
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Add tags from main IFD to result
        all_tags.extend(tags);

        // Read next IFD offset
        let next_offset = read_next_ifd_offset(reader, current_offset, entry_count, byte_order)?;

        // If next offset is 0, we've reached the end of the IFD chain
        if next_offset == 0 {
            break;
        }

        // Move to next IFD
        current_offset = next_offset as u64;
    }

    Ok(all_tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// Simple in-memory FileReader for testing
    struct TestReader {
        data: Vec<u8>,
    }

    impl TestReader {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    impl FileReader for TestReader {
        fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
            let start = offset as usize;
            let end = start + length;

            if end > self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "read beyond end of file",
                ));
            }

            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    /// Creates a minimal valid TIFF file with little-endian byte order
    fn create_minimal_tiff_le() -> Vec<u8> {
        let mut data = vec![0u8; 200];

        // === TIFF Header (8 bytes) ===
        // Byte order: "II" (little-endian)
        data[0] = 0x49;
        data[1] = 0x49;
        // Magic number: 42
        data[2] = 0x2A;
        data[3] = 0x00;
        // First IFD offset: 8 (points right after header)
        data[4] = 0x08;
        data[5] = 0x00;
        data[6] = 0x00;
        data[7] = 0x00;

        // === IFD0 at offset 8 ===
        // Entry count: 2 tags
        data[8] = 0x02;
        data[9] = 0x00;

        // Tag 1: ImageWidth (0x0100) = 100 (SHORT, inline)
        data[10] = 0x00;
        data[11] = 0x01;
        data[12] = 0x03; // Type: SHORT
        data[13] = 0x00;
        data[14] = 0x01; // Count: 1
        data[15] = 0x00;
        data[16] = 0x00;
        data[17] = 0x00;
        data[18] = 0x64; // Value: 100
        data[19] = 0x00;
        data[20] = 0x00;
        data[21] = 0x00;

        // Tag 2: ImageLength (0x0101) = 100 (SHORT, inline)
        data[22] = 0x01;
        data[23] = 0x01;
        data[24] = 0x03; // Type: SHORT
        data[25] = 0x00;
        data[26] = 0x01; // Count: 1
        data[27] = 0x00;
        data[28] = 0x00;
        data[29] = 0x00;
        data[30] = 0x64; // Value: 100
        data[31] = 0x00;
        data[32] = 0x00;
        data[33] = 0x00;

        // Next IFD offset: 0 (no more IFDs)
        data[34] = 0x00;
        data[35] = 0x00;
        data[36] = 0x00;
        data[37] = 0x00;

        data
    }

    /// Creates a TIFF file with big-endian byte order
    fn create_minimal_tiff_be() -> Vec<u8> {
        let mut data = vec![0u8; 200];

        // === TIFF Header ===
        // Byte order: "MM" (big-endian)
        data[0] = 0x4D;
        data[1] = 0x4D;
        // Magic number: 42
        data[2] = 0x00;
        data[3] = 0x2A;
        // First IFD offset: 8
        data[4] = 0x00;
        data[5] = 0x00;
        data[6] = 0x00;
        data[7] = 0x08;

        // === IFD0 at offset 8 ===
        // Entry count: 2
        data[8] = 0x00;
        data[9] = 0x02;

        // Tag 1: ImageWidth
        data[10] = 0x01;
        data[11] = 0x00;
        data[12] = 0x00;
        data[13] = 0x03; // Type: SHORT
        data[14] = 0x00;
        data[15] = 0x00;
        data[16] = 0x00;
        data[17] = 0x01; // Count: 1
        data[18] = 0x00;
        data[19] = 0x64; // Value: 100
        data[20] = 0x00;
        data[21] = 0x00;

        // Tag 2: ImageLength
        data[22] = 0x01;
        data[23] = 0x01;
        data[24] = 0x00;
        data[25] = 0x03;
        data[26] = 0x00;
        data[27] = 0x00;
        data[28] = 0x00;
        data[29] = 0x01;
        data[30] = 0x00;
        data[31] = 0x64;
        data[32] = 0x00;
        data[33] = 0x00;

        // Next IFD offset: 0
        data[34] = 0x00;
        data[35] = 0x00;
        data[36] = 0x00;
        data[37] = 0x00;

        data
    }

    /// Creates a multi-page TIFF (IFD0 → IFD1)
    fn create_multi_page_tiff() -> Vec<u8> {
        let mut data = vec![0u8; 300];

        // === Header ===
        data[0] = 0x49;
        data[1] = 0x49; // Little-endian
        data[2] = 0x2A;
        data[3] = 0x00; // Magic 42
        data[4] = 0x08;
        data[5] = 0x00;
        data[6] = 0x00;
        data[7] = 0x00; // First IFD at 8

        // === IFD0 at offset 8 ===
        data[8] = 0x01;
        data[9] = 0x00; // 1 tag

        // ImageWidth tag
        data[10] = 0x00;
        data[11] = 0x01;
        data[12] = 0x03;
        data[13] = 0x00;
        data[14] = 0x01;
        data[15] = 0x00;
        data[16] = 0x00;
        data[17] = 0x00;
        data[18] = 0x64;
        data[19] = 0x00; // Value: 100
        data[20] = 0x00;
        data[21] = 0x00;

        // Next IFD offset: 100 (points to IFD1)
        data[22] = 0x64;
        data[23] = 0x00;
        data[24] = 0x00;
        data[25] = 0x00;

        // === IFD1 at offset 100 ===
        data[100] = 0x01;
        data[101] = 0x00; // 1 tag

        // ImageLength tag
        data[102] = 0x01;
        data[103] = 0x01;
        data[104] = 0x03;
        data[105] = 0x00;
        data[106] = 0x01;
        data[107] = 0x00;
        data[108] = 0x00;
        data[109] = 0x00;
        data[110] = 0xC8;
        data[111] = 0x00; // Value: 200
        data[112] = 0x00;
        data[113] = 0x00;

        // Next IFD offset: 0 (last IFD)
        data[114] = 0x00;
        data[115] = 0x00;
        data[116] = 0x00;
        data[117] = 0x00;

        data
    }

    #[test]
    fn test_parse_tiff_header_little_endian() {
        let data = create_minimal_tiff_le();
        let reader = TestReader::new(data);

        let header = parse_tiff_header(&reader).expect("Failed to parse header");

        assert_eq!(header.byte_order, ByteOrder::LittleEndian);
        assert_eq!(header.first_ifd_offset, 8);
    }

    #[test]
    fn test_parse_tiff_header_big_endian() {
        let data = create_minimal_tiff_be();
        let reader = TestReader::new(data);

        let header = parse_tiff_header(&reader).expect("Failed to parse header");

        assert_eq!(header.byte_order, ByteOrder::BigEndian);
        assert_eq!(header.first_ifd_offset, 8);
    }

    #[test]
    fn test_parse_tiff_header_too_small() {
        let data = vec![0u8; 4]; // Only 4 bytes
        let reader = TestReader::new(data);

        let result = parse_tiff_header(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tiff_header_invalid_byte_order() {
        let mut data = vec![0u8; 8];
        data[0] = 0xFF;
        data[1] = 0xFF; // Invalid byte order
        data[2] = 0x2A;
        data[3] = 0x00;

        let reader = TestReader::new(data);
        let result = parse_tiff_header(&reader);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tiff_header_invalid_magic() {
        let mut data = vec![0u8; 8];
        data[0] = 0x49;
        data[1] = 0x49; // Little-endian
        data[2] = 0xFF;
        data[3] = 0xFF; // Invalid magic (not 42)

        let reader = TestReader::new(data);
        let result = parse_tiff_header(&reader);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tiff_file_single_ifd() {
        let data = create_minimal_tiff_le();
        let reader = TestReader::new(data);

        let tags = parse_tiff_file(&reader).expect("Failed to parse TIFF file");

        // Should have 2 tags: ImageWidth and ImageLength
        assert_eq!(tags.len(), 2);

        // Verify ImageWidth (0x0100)
        let width = tags.iter().find(|(id, _, _, _)| *id == 0x0100);
        assert!(width.is_some());

        // Verify ImageLength (0x0101)
        let length = tags.iter().find(|(id, _, _, _)| *id == 0x0101);
        assert!(length.is_some());
    }

    #[test]
    fn test_parse_tiff_file_multi_page() {
        let data = create_multi_page_tiff();
        let reader = TestReader::new(data);

        let tags = parse_tiff_file(&reader).expect("Failed to parse multi-page TIFF");

        // Should have 2 tags: one from IFD0, one from IFD1
        assert_eq!(tags.len(), 2);

        // Should have both ImageWidth and ImageLength
        let width = tags.iter().find(|(id, _, _, _)| *id == 0x0100);
        assert!(width.is_some(), "Should have ImageWidth from IFD0");

        let length = tags.iter().find(|(id, _, _, _)| *id == 0x0101);
        assert!(length.is_some(), "Should have ImageLength from IFD1");
    }

    #[test]
    fn test_parse_tiff_file_big_endian() {
        let data = create_minimal_tiff_be();
        let reader = TestReader::new(data);

        let tags = parse_tiff_file(&reader).expect("Failed to parse big-endian TIFF");

        assert_eq!(tags.len(), 2);
    }

    #[test]
    fn test_circular_ifd_reference() {
        let mut data = vec![0u8; 200];

        // Header
        data[0] = 0x49;
        data[1] = 0x49;
        data[2] = 0x2A;
        data[3] = 0x00;
        data[4] = 0x08;
        data[5] = 0x00;
        data[6] = 0x00;
        data[7] = 0x00;

        // IFD0 at offset 8
        data[8] = 0x00;
        data[9] = 0x00; // 0 tags

        // Next IFD offset: 8 (points back to itself - circular!)
        data[10] = 0x08;
        data[11] = 0x00;
        data[12] = 0x00;
        data[13] = 0x00;

        let reader = TestReader::new(data);
        let result = parse_tiff_file(&reader);

        // Should detect circular reference
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_u32_from_tag_value() {
        // Little-endian
        let value = vec![0x12, 0x34, 0x56, 0x78];
        let result = extract_u32_from_tag_value(&value, ByteOrder::LittleEndian);
        assert_eq!(result, Some(0x78563412));

        // Big-endian
        let result = extract_u32_from_tag_value(&value, ByteOrder::BigEndian);
        assert_eq!(result, Some(0x12345678));

        // Too short
        let value = vec![0x12, 0x34];
        let result = extract_u32_from_tag_value(&value, ByteOrder::LittleEndian);
        assert_eq!(result, None);
    }
}
