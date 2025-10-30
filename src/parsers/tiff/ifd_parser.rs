//! Image File Directory (IFD) parsing
//!
//! This module handles parsing of TIFF IFD structures using nom parser combinators.
//! IFDs are the core structural element of TIFF files, containing arrays of tag entries
//! that store image metadata.
//!
//! # TIFF IFD Structure
//!
//! An IFD consists of:
//! 1. **Entry Count**: 2 bytes (u16) - number of tag entries in this IFD
//! 2. **Tag Entries**: 12 bytes each × entry_count
//!    - Tag ID: 2 bytes (u16) - identifies the tag (e.g., 0x010F for Make)
//!    - Field Type: 2 bytes (u16) - data type (1=Byte, 2=ASCII, 3=Short, etc.)
//!    - Value Count: 4 bytes (u32) - number of values (not bytes)
//!    - Value/Offset: 4 bytes (u32) - either inline value (if ≤4 bytes) or offset
//! 3. **Next IFD Offset**: 4 bytes (u32) - offset to next IFD, or 0 if last
//!
//! # Byte Order
//!
//! TIFF files can be either little-endian (0x4949 "II") or big-endian (0x4D4D "MM").
//! The byte order marker appears at the start of the TIFF file and affects all
//! multi-byte values in the IFD structure.
//!
//! # Value Storage
//!
//! Values are stored either inline or via offset:
//! - If `(type_size × count) ≤ 4 bytes`: value stored inline in Value/Offset field
//! - Otherwise: Value/Offset contains absolute file offset to value data
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
//! use exiftool_rs::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("image.tif"))?;
//! let tags = parse_ifd(&reader, 8, ByteOrder::LittleEndian)?;
//!
//! for (tag_id, value) in tags {
//!     println!("Tag 0x{:04X}: {} bytes", tag_id, value.len());
//! }
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use crate::parsers::common::exif_types::ExifType;
use nom::{
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
    IResult,
};

/// Byte order (endianness) for TIFF data.
///
/// TIFF files begin with a 2-byte order marker:
/// - `0x4949` ("II") indicates little-endian (Intel byte order)
/// - `0x4D4D` ("MM") indicates big-endian (Motorola byte order)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Little-endian byte order (0x4949 "II")
    LittleEndian,
    /// Big-endian byte order (0x4D4D "MM")
    BigEndian,
}

/// Represents a single TIFF IFD tag entry.
///
/// Each entry is 12 bytes and contains the tag ID, type, count, and either
/// the value itself (if small enough) or an offset to the value data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfdEntry {
    /// Tag identifier (e.g., 0x010F for Make)
    pub tag_id: u16,
    /// EXIF data type (e.g., ASCII, Short, Long)
    pub field_type: u16,
    /// Number of values (not bytes)
    pub value_count: u32,
    /// Either inline value or offset to value data
    pub value_offset: u32,
}

/// Parses a TIFF Image File Directory (IFD) and extracts tag values.
///
/// This function reads an IFD structure at the specified offset and returns
/// a vector of (tag_id, raw_value) pairs. The raw values are returned as
/// owned byte vectors.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing file data
/// - `ifd_offset`: Byte offset from start of TIFF data to the IFD
/// - `byte_order`: Endianness for parsing multi-byte values
///
/// # Returns
///
/// - `Ok(Vec<(u16, u16, Vec<u8>)>)`: Vector of (tag_id, field_type, raw_value_bytes) tuples
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Errors
///
/// Returns an error if:
/// - IFD offset is beyond file size
/// - Entry count is invalid
/// - Tag entry data is truncated
/// - Value offset points beyond file size
/// - Unknown or invalid field type encountered
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
/// use exiftool_rs::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("image.tif"))?;
/// let tags = parse_ifd(&reader, 8, ByteOrder::LittleEndian)?;
///
/// // Find Make tag (0x010F)
/// for (tag_id, value) in &tags {
///     if *tag_id == 0x010F {
///         let make = String::from_utf8_lossy(value);
///         println!("Make: {}", make);
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn parse_ifd(
    reader: &dyn FileReader,
    ifd_offset: u64,
    byte_order: ByteOrder,
) -> Result<Vec<(u16, u16, Vec<u8>)>> {
    let file_size = reader.size();

    // Validate IFD offset
    if ifd_offset >= file_size {
        return Err(ExifToolError::parse_error_at(
            "IFD offset beyond file size",
            ifd_offset as usize,
        ));
    }

    // Read entry count (2 bytes)
    let entry_count_data = reader.read(ifd_offset, 2)?;
    let entry_count = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([entry_count_data[0], entry_count_data[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([entry_count_data[0], entry_count_data[1]]),
    };

    // Calculate IFD size: 2 bytes (count) + 12 bytes per entry + 4 bytes (next IFD offset)
    let ifd_size = 2 + (entry_count as usize * 12) + 4;

    // Validate IFD size doesn't exceed file
    if ifd_offset + ifd_size as u64 > file_size {
        return Err(ExifToolError::parse_error_at(
            format!("IFD size ({} bytes) exceeds file bounds", ifd_size),
            ifd_offset as usize,
        ));
    }

    // Read entire IFD (excluding the initial 2-byte count we already read)
    let entries_start = ifd_offset + 2;
    let entries_size = entry_count as usize * 12;
    let entries_data = reader.read(entries_start, entries_size)?;

    // Parse IFD entries based on byte order
    let ifd_entries = match byte_order {
        ByteOrder::LittleEndian => {
            parse_ifd_entries_le(entries_data, entry_count)
                .map_err(|e| {
                    ExifToolError::parse_error_at(
                        format!("Failed to parse IFD entries (LE): {}", e),
                        entries_start as usize,
                    )
                })?
                .1
        }
        ByteOrder::BigEndian => {
            parse_ifd_entries_be(entries_data, entry_count)
                .map_err(|e| {
                    ExifToolError::parse_error_at(
                        format!("Failed to parse IFD entries (BE): {}", e),
                        entries_start as usize,
                    )
                })?
                .1
        }
    };

    // Extract tag values
    let mut result = Vec::new();

    for entry in ifd_entries {
        // Get type information
        let exif_type = ExifType::from_u16(entry.field_type).ok_or_else(|| {
            ExifToolError::parse_error(format!("Unknown EXIF type code: {}", entry.field_type))
        })?;

        let type_size = exif_type.size_in_bytes();
        let total_size = type_size * entry.value_count as usize;

        // Extract value bytes
        let value_bytes = if total_size <= 4 {
            // Value is stored inline in the value_offset field
            extract_inline_value(entry.value_offset, total_size, byte_order)
        } else {
            // Value is stored at an offset
            let value_offset = entry.value_offset as u64;

            // Validate offset
            if value_offset + total_size as u64 > file_size {
                return Err(ExifToolError::parse_error_at(
                    format!(
                        "Tag 0x{:04X} value offset ({}) + size ({}) exceeds file size",
                        entry.tag_id, value_offset, total_size
                    ),
                    value_offset as usize,
                ));
            }

            // Read value data from offset
            reader.read(value_offset, total_size)?.to_vec()
        };

        result.push((entry.tag_id, entry.field_type, value_bytes));
    }

    Ok(result)
}

/// Extracts an inline value from the 4-byte value_offset field.
///
/// For values ≤4 bytes, TIFF stores them directly in the value_offset field.
/// Values are left-justified (stored in the first N bytes).
fn extract_inline_value(value_offset: u32, size: usize, byte_order: ByteOrder) -> Vec<u8> {
    let bytes = match byte_order {
        ByteOrder::LittleEndian => value_offset.to_le_bytes(),
        ByteOrder::BigEndian => value_offset.to_be_bytes(),
    };

    // For little-endian, values are in bytes[0..size]
    // For big-endian, values are also in bytes[0..size] when stored in the field
    // (TIFF spec says values are left-justified in the 4-byte field)
    bytes[0..size].to_vec()
}

/// Parses IFD entries in little-endian byte order.
fn parse_ifd_entries_le(input: &[u8], entry_count: u16) -> IResult<&[u8], Vec<IfdEntry>> {
    count(parse_ifd_entry_le, entry_count as usize)(input)
}

/// Parses IFD entries in big-endian byte order.
fn parse_ifd_entries_be(input: &[u8], entry_count: u16) -> IResult<&[u8], Vec<IfdEntry>> {
    count(parse_ifd_entry_be, entry_count as usize)(input)
}

/// Parses a single IFD entry (12 bytes) in little-endian byte order.
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

    /// Creates a minimal TIFF IFD with 3 tags in little-endian format.
    ///
    /// Tags included:
    /// - 0x010F (Make): "Canon" (5 bytes at offset 100)
    /// - 0x0110 (Model): "EOS" (3 bytes inline)
    /// - 0x0132 (DateTime): "2024:01:01 12:00:00" (19 bytes at offset 106)
    fn create_sample_ifd_le() -> Vec<u8> {
        let mut data = vec![0u8; 200];

        // === IFD at offset 0 ===

        // Entry count: 3 tags (little-endian)
        data[0] = 0x03;
        data[1] = 0x00;

        // === Tag 1: Make (0x010F) ===
        // Offset 2: Tag ID = 0x010F
        data[2] = 0x0F;
        data[3] = 0x01;
        // Offset 4: Type = ASCII (2)
        data[4] = 0x02;
        data[5] = 0x00;
        // Offset 6: Count = 6 (includes null terminator)
        data[6] = 0x06;
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x00;
        // Offset 10: Value offset = 100 (points to "Canon\0")
        data[10] = 0x64;
        data[11] = 0x00;
        data[12] = 0x00;
        data[13] = 0x00;

        // === Tag 2: Model (0x0110) ===
        // Offset 14: Tag ID = 0x0110
        data[14] = 0x10;
        data[15] = 0x01;
        // Offset 16: Type = ASCII (2)
        data[16] = 0x02;
        data[17] = 0x00;
        // Offset 18: Count = 4 (includes null terminator, fits inline)
        data[18] = 0x04;
        data[19] = 0x00;
        data[20] = 0x00;
        data[21] = 0x00;
        // Offset 22: Inline value = "EOS\0"
        data[22] = b'E';
        data[23] = b'O';
        data[24] = b'S';
        data[25] = 0x00;

        // === Tag 3: DateTime (0x0132) ===
        // Offset 26: Tag ID = 0x0132
        data[26] = 0x32;
        data[27] = 0x01;
        // Offset 28: Type = ASCII (2)
        data[28] = 0x02;
        data[29] = 0x00;
        // Offset 30: Count = 20 (includes null terminator)
        data[30] = 0x14;
        data[31] = 0x00;
        data[32] = 0x00;
        data[33] = 0x00;
        // Offset 34: Value offset = 106 (points to datetime string)
        data[34] = 0x6A;
        data[35] = 0x00;
        data[36] = 0x00;
        data[37] = 0x00;

        // Next IFD offset: 0 (no next IFD)
        data[38] = 0x00;
        data[39] = 0x00;
        data[40] = 0x00;
        data[41] = 0x00;

        // === Value data ===
        // Offset 100: "Canon\0"
        data[100..106].copy_from_slice(b"Canon\0");

        // Offset 106: "2024:01:01 12:00:00\0"
        data[106..126].copy_from_slice(b"2024:01:01 12:00:00\0");

        data
    }

    /// Creates a minimal TIFF IFD with 3 tags in big-endian format.
    fn create_sample_ifd_be() -> Vec<u8> {
        let mut data = vec![0u8; 200];

        // === IFD at offset 0 ===

        // Entry count: 3 tags (big-endian)
        data[0] = 0x00;
        data[1] = 0x03;

        // === Tag 1: Make (0x010F) ===
        data[2] = 0x01;
        data[3] = 0x0F;
        data[4] = 0x00;
        data[5] = 0x02; // ASCII
        data[6] = 0x00;
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x06; // Count = 6
        data[10] = 0x00;
        data[11] = 0x00;
        data[12] = 0x00;
        data[13] = 0x64; // Offset = 100

        // === Tag 2: Model (0x0110) - inline ===
        data[14] = 0x01;
        data[15] = 0x10;
        data[16] = 0x00;
        data[17] = 0x02; // ASCII
        data[18] = 0x00;
        data[19] = 0x00;
        data[20] = 0x00;
        data[21] = 0x04; // Count = 4
        data[22] = b'E';
        data[23] = b'O';
        data[24] = b'S';
        data[25] = 0x00;

        // === Tag 3: DateTime (0x0132) ===
        data[26] = 0x01;
        data[27] = 0x32;
        data[28] = 0x00;
        data[29] = 0x02; // ASCII
        data[30] = 0x00;
        data[31] = 0x00;
        data[32] = 0x00;
        data[33] = 0x14; // Count = 20
        data[34] = 0x00;
        data[35] = 0x00;
        data[36] = 0x00;
        data[37] = 0x6A; // Offset = 106

        // Next IFD offset: 0
        data[38] = 0x00;
        data[39] = 0x00;
        data[40] = 0x00;
        data[41] = 0x00;

        // === Value data ===
        data[100..106].copy_from_slice(b"Canon\0");
        data[106..126].copy_from_slice(b"2024:01:01 12:00:00\0");

        data
    }

    #[test]
    fn test_parse_ifd_little_endian() {
        let data = create_sample_ifd_le();
        let reader = TestReader::new(data);

        let tags = parse_ifd(&reader, 0, ByteOrder::LittleEndian)
            .expect("Failed to parse little-endian IFD");

        // Should have 3 tags
        assert_eq!(tags.len(), 3);

        // Check Make tag (0x010F)
        let make = tags.iter().find(|(id, _)| *id == 0x010F);
        assert!(make.is_some());
        let (_, make_value) = make.unwrap();
        assert_eq!(make_value, b"Canon\0");

        // Check Model tag (0x0110)
        let model = tags.iter().find(|(id, _)| *id == 0x0110);
        assert!(model.is_some());
        let (_, model_value) = model.unwrap();
        assert_eq!(model_value, b"EOS\0");

        // Check DateTime tag (0x0132)
        let datetime = tags.iter().find(|(id, _)| *id == 0x0132);
        assert!(datetime.is_some());
        let (_, datetime_value) = datetime.unwrap();
        assert_eq!(datetime_value, b"2024:01:01 12:00:00\0");
    }

    #[test]
    fn test_parse_ifd_big_endian() {
        let data = create_sample_ifd_be();
        let reader = TestReader::new(data);

        let tags =
            parse_ifd(&reader, 0, ByteOrder::BigEndian).expect("Failed to parse big-endian IFD");

        // Should have 3 tags
        assert_eq!(tags.len(), 3);

        // Check Make tag
        let make = tags.iter().find(|(id, _)| *id == 0x010F);
        assert!(make.is_some());
        let (_, make_value) = make.unwrap();
        assert_eq!(make_value, b"Canon\0");

        // Check Model tag
        let model = tags.iter().find(|(id, _)| *id == 0x0110);
        assert!(model.is_some());
        let (_, model_value) = model.unwrap();
        assert_eq!(model_value, b"EOS\0");

        // Check DateTime tag
        let datetime = tags.iter().find(|(id, _)| *id == 0x0132);
        assert!(datetime.is_some());
        let (_, datetime_value) = datetime.unwrap();
        assert_eq!(datetime_value, b"2024:01:01 12:00:00\0");
    }

    #[test]
    fn test_parse_empty_ifd() {
        let mut data = vec![0u8; 10];
        // Entry count: 0
        data[0] = 0x00;
        data[1] = 0x00;
        // Next IFD offset: 0
        data[2] = 0x00;
        data[3] = 0x00;
        data[4] = 0x00;
        data[5] = 0x00;

        let reader = TestReader::new(data);
        let tags =
            parse_ifd(&reader, 0, ByteOrder::LittleEndian).expect("Failed to parse empty IFD");

        assert_eq!(tags.len(), 0);
    }

    #[test]
    fn test_parse_truncated_ifd() {
        let mut data = vec![0u8; 10];
        // Entry count: 5 (but not enough data for 5 entries)
        data[0] = 0x05;
        data[1] = 0x00;

        let reader = TestReader::new(data);
        let result = parse_ifd(&reader, 0, ByteOrder::LittleEndian);

        // Should fail due to truncated IFD
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ifd_offset_beyond_file() {
        let data = vec![0u8; 50];
        let reader = TestReader::new(data);

        let result = parse_ifd(&reader, 100, ByteOrder::LittleEndian);

        // Should fail because offset is beyond file size
        assert!(result.is_err());
        if let Err(ExifToolError::ParseError { message, offset }) = result {
            assert!(message.contains("beyond file size"));
            assert_eq!(offset, Some(100));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_parse_ifd_with_invalid_value_offset() {
        let mut data = vec![0u8; 100];

        // Entry count: 1
        data[0] = 0x01;
        data[1] = 0x00;

        // Tag entry with invalid offset
        data[2] = 0xFF; // Tag ID
        data[3] = 0xFF;
        data[4] = 0x02; // Type = ASCII
        data[5] = 0x00;
        data[6] = 0x0A; // Count = 10
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x00;
        data[10] = 0xE8; // Offset = 1000 (beyond file)
        data[11] = 0x03;
        data[12] = 0x00;
        data[13] = 0x00;

        let reader = TestReader::new(data);
        let result = parse_ifd(&reader, 0, ByteOrder::LittleEndian);

        // Should fail due to invalid offset
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ifd_with_unknown_type() {
        let mut data = vec![0u8; 50];

        // Entry count: 1
        data[0] = 0x01;
        data[1] = 0x00;

        // Tag entry with unknown type
        data[2] = 0xFF; // Tag ID
        data[3] = 0xFF;
        data[4] = 0xFF; // Type = 255 (invalid)
        data[5] = 0x00;
        data[6] = 0x01; // Count = 1
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x00;
        data[10] = 0x00; // Value
        data[11] = 0x00;
        data[12] = 0x00;
        data[13] = 0x00;

        let reader = TestReader::new(data);
        let result = parse_ifd(&reader, 0, ByteOrder::LittleEndian);

        // Should fail due to unknown type
        assert!(result.is_err());
        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(message.contains("Unknown EXIF type"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_extract_inline_value_le() {
        // Test 1-byte inline value
        let value = extract_inline_value(0x12345678, 1, ByteOrder::LittleEndian);
        assert_eq!(value, vec![0x78]);

        // Test 2-byte inline value
        let value = extract_inline_value(0x12345678, 2, ByteOrder::LittleEndian);
        assert_eq!(value, vec![0x78, 0x56]);

        // Test 4-byte inline value
        let value = extract_inline_value(0x12345678, 4, ByteOrder::LittleEndian);
        assert_eq!(value, vec![0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn test_extract_inline_value_be() {
        // Test 1-byte inline value
        let value = extract_inline_value(0x12345678, 1, ByteOrder::BigEndian);
        assert_eq!(value, vec![0x12]);

        // Test 2-byte inline value
        let value = extract_inline_value(0x12345678, 2, ByteOrder::BigEndian);
        assert_eq!(value, vec![0x12, 0x34]);

        // Test 4-byte inline value
        let value = extract_inline_value(0x12345678, 4, ByteOrder::BigEndian);
        assert_eq!(value, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_byte_order_equality() {
        assert_eq!(ByteOrder::LittleEndian, ByteOrder::LittleEndian);
        assert_eq!(ByteOrder::BigEndian, ByteOrder::BigEndian);
        assert_ne!(ByteOrder::LittleEndian, ByteOrder::BigEndian);
    }
}
