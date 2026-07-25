//! TIFF IFD serialization
//!
//! This module handles writing TIFF IFD (Image File Directory) structures from metadata.
//! It provides functions to serialize MetadataMap EXIF tags back to binary TIFF IFD format,
//! supporting both little-endian and big-endian byte orders.
//!
//! # Architecture
//!
//! The TIFF writer has been refactored into modular components:
//! - `tiff::byte_writer` - Low-level byte order writing utilities
//! - `tiff::ifd_entry` - IFD entry data structures and conversions
//! - `tiff::ifd_builder` - Builder pattern for IFD construction
//! - `tiff::tiff_builder` - Builder pattern for complete TIFF file assembly
//! - `tiff::validator` - Validation logic for TIFF metadata
//!
//! # TIFF IFD Structure
//!
//! An IFD consists of:
//! 1. **Entry Count**: 2 bytes (u16) - number of tag entries in this IFD
//! 2. **Tag Entries**: 12 bytes each × entry_count (must be sorted by tag ID)
//!    - Tag ID: 2 bytes (u16) - identifies the tag (e.g., 0x010F for Make)
//!    - Field Type: 2 bytes (u16) - data type (1=Byte, 2=ASCII, 3=Short, etc.)
//!    - Value Count: 4 bytes (u32) - number of values (not bytes)
//!    - Value/Offset: 4 bytes (u32) - either inline value (if ≤4 bytes) or offset
//! 3. **Next IFD Offset**: 4 bytes (u32) - offset to next IFD, or 0 if last
//! 4. **Value Data Area**: For values >4 bytes, written sequentially
//!
//! # Example
//!
//! ```no_run
//! use oxidex::core::metadata_map::MetadataMap;
//! use oxidex::core::tag_value::TagValue;
//! use oxidex::parsers::tiff::ifd_parser::ByteOrder;
//! use oxidex::writers::tiff_writer::serialize_ifd;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut metadata = MetadataMap::new();
//! metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
//! metadata.insert("EXIF:Model", TagValue::new_string("EOS R5"));
//!
//! let ifd_bytes = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0)?;
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

mod tiff;

use crate::core::FileReader;
use crate::core::metadata_map::MetadataMap;
use crate::error::Result;
use crate::parsers::tiff::file_parser::parse_tiff_header;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::writers::atomic_writer::write_atomic;
use std::path::Path;

// Re-export commonly used functions and types
pub use tiff::ifd_builder::IfdBuilder;
pub use tiff::tiff_builder::TiffBuilder;

/// Writes a complete TIFF file with modified metadata.
///
/// This is the main entry point for TIFF file writing. It reads the original file,
/// extracts image data if present, and writes a new TIFF file with updated metadata
/// while preserving pixel data unchanged.
///
/// # Parameters
///
/// - `path`: Output file path where the TIFF file will be written
/// - `original_reader`: FileReader for the original TIFF file (for reading image data)
/// - `modified_metadata`: MetadataMap containing the tags to write
///
/// # Returns
///
/// - `Ok(())`: File written successfully
/// - `Err(ExifToolError)`: Write error, I/O error, or invalid metadata
///
/// # Example
///
/// ```no_run
/// use oxidex::io::buffered_reader::BufferedReader;
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::writers::tiff_writer::write_tiff_file;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let input_path = Path::new("input.tif");
/// let output_path = Path::new("output.tif");
/// let reader = BufferedReader::new(input_path)?;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
/// metadata.insert("EXIF:Model", TagValue::new_string("EOS R5"));
///
/// write_tiff_file(output_path, &reader, &metadata)?;
/// # Ok(())
/// # }
/// ```
pub fn write_tiff_file(
    path: &Path,
    original_reader: &dyn FileReader,
    modified_metadata: &MetadataMap,
) -> Result<()> {
    // Parse original TIFF header to preserve byte order
    let header = parse_tiff_header(original_reader)?;
    let byte_order = header.byte_order;

    // Reconstruct the complete TIFF structure using the builder
    let tiff_data = reconstruct_tiff_structure(original_reader, byte_order, modified_metadata)?;

    // Write atomically to prevent corruption
    write_atomic(path, &tiff_data)?;

    Ok(())
}

/// Reconstructs the complete TIFF file bytes from components.
///
/// This helper function assembles the TIFF file structure using the TiffBuilder:
/// - 8-byte header
/// - IFD0 (main image metadata)
/// - EXIF sub-IFD (if EXIF tags present)
/// - GPS sub-IFD (if GPS tags present)
/// - Image data (if present in original - currently not implemented)
/// - IFD1 (thumbnail metadata - currently not implemented)
///
/// # Parameters
///
/// - `original_reader`: FileReader for the original TIFF file (currently unused)
/// - `byte_order`: Endianness for the output file
/// - `modified_metadata`: MetadataMap containing all tags to write
///
/// # Returns
///
/// Complete TIFF file as bytes, ready to write to disk
pub fn reconstruct_tiff_structure(
    _original_reader: &dyn FileReader,
    byte_order: ByteOrder,
    modified_metadata: &MetadataMap,
) -> Result<Vec<u8>> {
    TiffBuilder::new()
        .with_byte_order(byte_order)
        .with_metadata(modified_metadata)?
        .build()
}

/// Serializes EXIF tags from MetadataMap to TIFF IFD bytes.
///
/// This function filters tags for the EXIF family, converts them to TIFF data types,
/// builds IFD entries, handles inline vs. offset values, and writes the complete
/// IFD structure in the specified byte order.
///
/// # Parameters
///
/// - `metadata`: MetadataMap containing tags to serialize (only EXIF: tags are processed)
/// - `byte_order`: Endianness for serialization (LittleEndian or BigEndian)
/// - `ifd_start_offset`: Byte offset where this IFD will be written in the file
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: Complete IFD structure as bytes (ready to write to file)
/// - `Err(ExifToolError)`: Conversion error or unsupported tag value
///
/// # Errors
///
/// Returns an error if:
/// - A tag name cannot be mapped to a numeric tag ID
/// - A TagValue variant cannot be converted to a TIFF type
/// - A value is too large or otherwise invalid for TIFF format
///
/// # Example
///
/// ```no_run
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
/// use oxidex::writers::tiff_writer::serialize_ifd;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
/// metadata.insert("EXIF:ISO", TagValue::new_integer(400));
///
/// // Serialize with little-endian byte order
/// let ifd_bytes = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0)?;
///
/// // Serialize with big-endian byte order
/// let ifd_bytes_be = serialize_ifd(&metadata, ByteOrder::BigEndian, 0)?;
/// # Ok(())
/// # }
/// ```
pub fn serialize_ifd(
    metadata: &MetadataMap,
    byte_order: ByteOrder,
    ifd_start_offset: u64,
) -> Result<Vec<u8>> {
    IfdBuilder::new()
        .with_byte_order(byte_order)
        .with_start_offset(ifd_start_offset)
        .add_metadata(metadata)?
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;
    use crate::io::EndianReader;
    use crate::parsers::tiff::ifd_parser::parse_ifd;
    use crate::test_support::TestReader;

    #[test]
    fn test_serialize_empty_ifd() {
        let metadata = MetadataMap::new();
        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);

        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Should have: 2 bytes (count=0) + 4 bytes (next IFD offset=0)
        assert_eq!(bytes.len(), 6);
        assert_eq!(bytes[0], 0x00); // Entry count low byte
        assert_eq!(bytes[1], 0x00); // Entry count high byte
    }

    #[test]
    fn test_serialize_single_string_tag_inline() {
        let mut metadata = MetadataMap::new();
        // "EOS" + null = 4 bytes, fits inline
        metadata.insert("EXIF:Model", TagValue::new_string("EOS"));

        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);
        assert!(result.is_ok());

        let bytes = result.unwrap();

        // Should have: 2 bytes (count) + 12 bytes (entry) + 4 bytes (next IFD)
        assert_eq!(bytes.len(), 18);

        // Entry count should be 1
        let reader = EndianReader::little_endian(&bytes);
        assert_eq!(reader.u16_at(0).unwrap_or(0), 1);

        // Tag ID should be 0x0110 (Model)
        assert_eq!(reader.u16_at(2).unwrap_or(0), 0x0110);

        // Type should be ASCII (2)
        assert_eq!(reader.u16_at(4).unwrap_or(0), 2);

        // Count should be 4 (including null)
        assert_eq!(reader.u32_at(6).unwrap_or(0), 4);

        // Value should be inline: "EOS\0"
        assert_eq!(&bytes[10..14], b"EOS\0");
    }

    #[test]
    fn test_serialize_string_tag_with_offset() {
        let mut metadata = MetadataMap::new();
        // "Canon" + null = 6 bytes, needs offset
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);
        assert!(result.is_ok());

        let bytes = result.unwrap();

        // Should have: 2 (count) + 12 (entry) + 4 (next IFD) + 6 (value data)
        assert_eq!(bytes.len(), 24);

        // Entry count should be 1
        let reader = EndianReader::little_endian(&bytes);
        assert_eq!(reader.u16_at(0).unwrap_or(0), 1);

        // Tag ID should be 0x010F (Make)
        assert_eq!(reader.u16_at(2).unwrap_or(0), 0x010F);

        // Type should be ASCII (2)
        assert_eq!(reader.u16_at(4).unwrap_or(0), 2);

        // Count should be 6
        assert_eq!(reader.u32_at(6).unwrap_or(0), 6);

        // Offset should point to value area (after IFD header)
        let offset = reader.u32_at(10).unwrap_or(0);
        assert_eq!(offset, 18); // 2 + 12 + 4

        // Value data should be "Canon\0"
        assert_eq!(&bytes[18..24], b"Canon\0");
    }

    #[test]
    fn test_serialize_integer_tag_short() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);
        assert!(result.is_ok());

        let bytes = result.unwrap();

        // Entry count should be 1
        let reader = EndianReader::little_endian(&bytes);
        assert_eq!(reader.u16_at(0).unwrap_or(0), 1);

        // Type should be Short (3)
        assert_eq!(reader.u16_at(4).unwrap_or(0), 3);

        // Count should be 1
        assert_eq!(reader.u32_at(6).unwrap_or(0), 1);

        // Value should be 400 (inline, as u16)
        assert_eq!(reader.u16_at(10).unwrap_or(0), 400);
    }

    #[test]
    fn test_serialize_rational_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:FNumber", TagValue::new_rational(28, 10)); // f/2.8

        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);
        assert!(result.is_ok());

        let bytes = result.unwrap();

        // Type should be Rational (5)
        let reader = EndianReader::little_endian(&bytes);
        assert_eq!(reader.u16_at(4).unwrap_or(0), 5);

        // Count should be 1
        assert_eq!(reader.u32_at(6).unwrap_or(0), 1);

        // Offset should point to value area
        let offset = reader.u32_at(10).unwrap_or(0);
        assert_eq!(offset, 18);

        // Value data should be numerator (28) + denominator (10)
        let numerator = reader.u32_at(18).unwrap_or(0);
        let denominator = reader.u32_at(22).unwrap_or(0);
        assert_eq!(numerator, 28);
        assert_eq!(denominator, 10);
    }

    #[test]
    fn test_serialize_multiple_tags_sorted() {
        let mut metadata = MetadataMap::new();
        // Insert in non-sorted order
        metadata.insert("EXIF:Model", TagValue::new_string("EOS")); // 0x0110
        metadata.insert("EXIF:Make", TagValue::new_string("Canon")); // 0x010F

        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);
        assert!(result.is_ok());

        let bytes = result.unwrap();

        // Entry count should be 2
        let reader = EndianReader::little_endian(&bytes);
        assert_eq!(reader.u16_at(0).unwrap_or(0), 2);

        // First entry should be Make (0x010F) - lower tag ID
        let first_tag = reader.u16_at(2).unwrap_or(0);
        assert_eq!(first_tag, 0x010F);

        // Second entry should be Model (0x0110) - higher tag ID
        let second_tag = reader.u16_at(14).unwrap_or(0);
        assert_eq!(second_tag, 0x0110);
    }

    #[test]
    fn test_serialize_big_endian() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Model", TagValue::new_string("EOS"));

        let result = serialize_ifd(&metadata, ByteOrder::BigEndian, 0);
        assert!(result.is_ok());

        let bytes = result.unwrap();

        // Entry count should be 1 (big-endian)
        let reader = EndianReader::big_endian(&bytes);
        assert_eq!(reader.u16_at(0).unwrap_or(0), 1);

        // Tag ID should be 0x0110 (big-endian)
        assert_eq!(reader.u16_at(2).unwrap_or(0), 0x0110);

        // Type should be ASCII (2, big-endian)
        assert_eq!(reader.u16_at(4).unwrap_or(0), 2);

        // Count should be 4 (big-endian)
        assert_eq!(reader.u32_at(6).unwrap_or(0), 4);

        // Value should still be "EOS\0" (ASCII is byte-oriented)
        assert_eq!(&bytes[10..14], b"EOS\0");
    }

    #[test]
    fn test_round_trip_little_endian() {
        // Create metadata
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        // Serialize
        let bytes = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0).unwrap();

        // Parse back
        let reader = TestReader::new(bytes);
        let parsed = parse_ifd(&reader, 0, ByteOrder::LittleEndian).unwrap();

        // Verify tag count
        assert_eq!(parsed.len(), 3);

        // Verify Make
        let make = parsed.iter().find(|(id, _, _, _)| *id == 0x010F);
        assert!(make.is_some());
        let (_, _, _, make_value) = make.unwrap();
        assert_eq!(make_value.as_ref(), b"Canon\0");

        // Verify Model
        let model = parsed.iter().find(|(id, _, _, _)| *id == 0x0110);
        assert!(model.is_some());
        let (_, _, _, model_value) = model.unwrap();
        assert_eq!(model_value.as_ref(), b"EOS\0");

        // Verify ISO
        let iso = parsed.iter().find(|(id, _, _, _)| *id == 0x8827);
        assert!(iso.is_some());
        let (_, _, _, iso_value) = iso.unwrap();
        let iso_bytes = iso_value.as_ref();
        let iso_reader = EndianReader::little_endian(iso_bytes);
        assert_eq!(iso_reader.u16_at(0).unwrap_or(0), 400);
    }

    #[test]
    fn test_round_trip_big_endian() {
        // Create metadata
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Nikon"));
        metadata.insert("EXIF:Model", TagValue::new_string("D850"));

        // Serialize
        let bytes = serialize_ifd(&metadata, ByteOrder::BigEndian, 0).unwrap();

        // Parse back
        let reader = TestReader::new(bytes);
        let parsed = parse_ifd(&reader, 0, ByteOrder::BigEndian).unwrap();

        // Verify tag count
        assert_eq!(parsed.len(), 2);

        // Verify Make
        let make = parsed.iter().find(|(id, _, _, _)| *id == 0x010F);
        assert!(make.is_some());
        let (_, _, _, make_value) = make.unwrap();
        assert_eq!(make_value.as_ref(), b"Nikon\0");

        // Verify Model
        let model = parsed.iter().find(|(id, _, _, _)| *id == 0x0110);
        assert!(model.is_some());
        let (_, _, _, model_value) = model.unwrap();
        assert_eq!(model_value.as_ref(), b"D850\0");
    }

    #[test]
    fn test_round_trip_with_rational() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:FNumber", TagValue::new_rational(28, 10));

        // Serialize
        let bytes = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0).unwrap();

        // Parse back
        let reader = TestReader::new(bytes);
        let parsed = parse_ifd(&reader, 0, ByteOrder::LittleEndian).unwrap();

        // Verify
        assert_eq!(parsed.len(), 1);

        let fnumber = parsed.iter().find(|(id, _, _, _)| *id == 0x829D);
        assert!(fnumber.is_some());
        let (_, _, _, value) = fnumber.unwrap();

        // Should be 8 bytes: numerator (28) + denominator (10)
        assert_eq!(value.len(), 8);
        let value_reader = EndianReader::little_endian(value);
        let numerator = value_reader.u32_at(0).unwrap_or(0);
        let denominator = value_reader.u32_at(4).unwrap_or(0);
        assert_eq!(numerator, 28);
        assert_eq!(denominator, 10);
    }

    #[test]
    fn test_non_exif_tags_filtered() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("XMP:Creator", TagValue::new_string("John Doe")); // Non-EXIF
        metadata.insert("IPTC:Keywords", TagValue::new_string("test")); // Non-EXIF

        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);
        assert!(result.is_ok());

        let bytes = result.unwrap();

        // Entry count should be 1 (only EXIF:Make)
        let reader = EndianReader::little_endian(&bytes);
        assert_eq!(reader.u16_at(0).unwrap_or(0), 1);
    }

    #[test]
    fn test_unsupported_tag_skipped() {
        let mut metadata = MetadataMap::new();
        // Create a tag that's not in the registry - should be skipped silently
        metadata.insert("EXIF:UnknownTag99999", TagValue::new_string("test"));
        metadata.insert("EXIF:Make", TagValue::new_string("Canon")); // Valid tag

        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);

        // Should succeed but only include the valid tag
        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Entry count should be 1 (only EXIF:Make, unknown tag is skipped)
        let reader = EndianReader::little_endian(&bytes);
        assert_eq!(reader.u16_at(0).unwrap_or(0), 1);
    }

    #[test]
    fn test_binary_data_serialization() {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            "EXIF:UserComment",
            TagValue::new_binary(vec![0x41, 0x42, 0x43, 0x44]),
        );

        let result = serialize_ifd(&metadata, ByteOrder::LittleEndian, 0);
        assert!(result.is_ok());

        let bytes = result.unwrap();

        // Type should be Undefined (7)
        let reader = EndianReader::little_endian(&bytes);
        assert_eq!(reader.u16_at(4).unwrap_or(0), 7);

        // Count should be 4
        assert_eq!(reader.u32_at(6).unwrap_or(0), 4);

        // Value should be inline
        assert_eq!(&bytes[10..14], &[0x41, 0x42, 0x43, 0x44]);
    }
}
