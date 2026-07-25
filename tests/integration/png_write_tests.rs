//! Integration tests for PNG metadata writer
//!
//! These tests verify PNG metadata modification with write operations.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::metadata_map::MetadataMap;
use oxidex::core::tag_value::TagValue;
use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::png::parse_png_metadata;
use oxidex::writers::png_writer::write_png_metadata;
use tempfile::TempDir;

/// PNG signature
const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Calculates CRC-32 for PNG chunks
fn calculate_png_crc(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
    use crc::{CRC_32_ISO_HDLC, Crc};
    let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    let mut digest = crc.digest();
    digest.update(chunk_type);
    digest.update(data);
    digest.finalize()
}

/// Creates a minimal valid PNG with IHDR, IDAT, and IEND chunks
fn create_minimal_png_with_idat() -> Vec<u8> {
    let mut data = Vec::new();

    // PNG signature
    data.extend_from_slice(&PNG_SIGNATURE);

    // IHDR chunk (13 bytes data)
    let ihdr_data = [
        0, 0, 0, 1, // Width: 1
        0, 0, 0, 1, // Height: 1
        8, // Bit depth
        2, // Color type: RGB
        0, // Compression
        0, // Filter
        0, // Interlace
    ];
    let ihdr_crc = calculate_png_crc(b"IHDR", &ihdr_data);
    data.extend_from_slice(&13u32.to_be_bytes()); // Length
    data.extend_from_slice(b"IHDR"); // Type
    data.extend_from_slice(&ihdr_data);
    data.extend_from_slice(&ihdr_crc.to_be_bytes());

    // IDAT chunk (minimal compressed image data)
    // This is a valid compressed data for a 1x1 RGB image (all black)
    let idat_data = [
        0x78, 0x9C, // zlib header
        0x62, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, // compressed data
    ];
    let idat_crc = calculate_png_crc(b"IDAT", &idat_data);
    data.extend_from_slice(&(idat_data.len() as u32).to_be_bytes());
    data.extend_from_slice(b"IDAT");
    data.extend_from_slice(&idat_data);
    data.extend_from_slice(&idat_crc.to_be_bytes());

    // IEND chunk (0 bytes data)
    let iend_crc = calculate_png_crc(b"IEND", &[]);
    data.extend_from_slice(&0u32.to_be_bytes()); // Length
    data.extend_from_slice(b"IEND"); // Type
    data.extend_from_slice(&iend_crc.to_be_bytes());

    data
}

/// Creates a PNG with tEXt chunks
fn create_png_with_text() -> Vec<u8> {
    let mut data = Vec::new();

    // PNG signature
    data.extend_from_slice(&PNG_SIGNATURE);

    // IHDR chunk
    let ihdr_data = [
        0, 0, 0, 1, // Width: 1
        0, 0, 0, 1, // Height: 1
        8, 2, 0, 0, 0, // bit depth, color type, compression, filter, interlace
    ];
    let ihdr_crc = calculate_png_crc(b"IHDR", &ihdr_data);
    data.extend_from_slice(&13u32.to_be_bytes());
    data.extend_from_slice(b"IHDR");
    data.extend_from_slice(&ihdr_data);
    data.extend_from_slice(&ihdr_crc.to_be_bytes());

    // tEXt chunk: Author
    let text_data = b"Author\0Original Author";
    let text_crc = calculate_png_crc(b"tEXt", text_data);
    data.extend_from_slice(&(text_data.len() as u32).to_be_bytes());
    data.extend_from_slice(b"tEXt");
    data.extend_from_slice(text_data);
    data.extend_from_slice(&text_crc.to_be_bytes());

    // IDAT chunk
    let idat_data = [0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01];
    let idat_crc = calculate_png_crc(b"IDAT", &idat_data);
    data.extend_from_slice(&(idat_data.len() as u32).to_be_bytes());
    data.extend_from_slice(b"IDAT");
    data.extend_from_slice(&idat_data);
    data.extend_from_slice(&idat_crc.to_be_bytes());

    // IEND chunk
    let iend_crc = calculate_png_crc(b"IEND", &[]);
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(b"IEND");
    data.extend_from_slice(&iend_crc.to_be_bytes());

    data
}

#[test]
fn test_write_text_chunk_to_new_png() {
    // Create a minimal PNG with IDAT
    let original_png = create_minimal_png_with_idat();
    let reader = TestReader::new(original_png);

    // Create metadata with tEXt tag
    let mut metadata = MetadataMap::new();
    metadata.insert("PNG:tEXt:Author", TagValue::new_string("Test Author"));
    metadata.insert("PNG:tEXt:Title", TagValue::new_string("Test Title"));

    // Write to temp file
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");

    write_png_metadata(&output_path, &reader, &metadata).unwrap();

    // Read back and verify
    let output_reader = BufferedReader::new(&output_path).unwrap();
    let parsed_metadata = parse_png_metadata(&output_reader).unwrap();

    assert_eq!(
        parsed_metadata.get_string("PNG:tEXt:Author"),
        Some("Test Author")
    );
    assert_eq!(
        parsed_metadata.get_string("PNG:tEXt:Title"),
        Some("Test Title")
    );
}

#[test]
fn test_modify_existing_text_chunk() {
    // Create PNG with existing text
    let original_png = create_png_with_text();
    let reader = TestReader::new(original_png.clone());

    // Parse original metadata
    let original_metadata = parse_png_metadata(&reader).unwrap();
    assert_eq!(
        original_metadata.get_string("PNG:tEXt:Author"),
        Some("Original Author")
    );

    // Modify metadata
    let mut modified_metadata = MetadataMap::new();
    modified_metadata.insert("PNG:tEXt:Author", TagValue::new_string("Modified Author"));

    // Write to temp file
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");

    write_png_metadata(&output_path, &reader, &modified_metadata).unwrap();

    // Read back and verify change
    let output_reader = BufferedReader::new(&output_path).unwrap();
    let parsed_metadata = parse_png_metadata(&output_reader).unwrap();

    assert_eq!(
        parsed_metadata.get_string("PNG:tEXt:Author"),
        Some("Modified Author")
    );
}

#[test]
fn test_write_itxt_chunk() {
    // Create a minimal PNG
    let original_png = create_minimal_png_with_idat();
    let reader = TestReader::new(original_png);

    // Create metadata with iTXt tag (UTF-8)
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "PNG:iTXt:Description",
        TagValue::new_string("UTF-8 Text: 你好世界"),
    );
    metadata.insert("PNG:iTXt:Comment", TagValue::new_string("Testing iTXt"));

    // Write to temp file
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");

    write_png_metadata(&output_path, &reader, &metadata).unwrap();

    // Read back and verify
    let output_reader = BufferedReader::new(&output_path).unwrap();
    let parsed_metadata = parse_png_metadata(&output_reader).unwrap();

    assert_eq!(
        parsed_metadata.get_string("PNG:iTXt:Description"),
        Some("UTF-8 Text: 你好世界")
    );
    assert_eq!(
        parsed_metadata.get_string("PNG:iTXt:Comment"),
        Some("Testing iTXt")
    );
}

#[test]
fn test_write_exif_chunk() {
    // Create a minimal PNG
    let original_png = create_minimal_png_with_idat();
    let reader = TestReader::new(original_png);

    // Create metadata with EXIF tags
    let mut metadata = MetadataMap::new();
    metadata.insert("IFD0:Make", TagValue::new_string("Canon"));
    metadata.insert("IFD0:Model", TagValue::new_string("EOS R5"));
    metadata.insert("IFD0:ImageWidth", TagValue::new_integer(1920));

    // Write to temp file
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");

    write_png_metadata(&output_path, &reader, &metadata).unwrap();

    // Read back and verify
    // PNG parser returns EXIF tags with human-readable names (IFD0:Make)
    let output_reader = BufferedReader::new(&output_path).unwrap();
    let parsed_metadata = parse_png_metadata(&output_reader).unwrap();

    // Verify tags are present with IFD0: prefix
    assert_eq!(parsed_metadata.get_string("IFD0:Make"), Some("Canon"));
    assert_eq!(parsed_metadata.get_string("IFD0:Model"), Some("EOS R5"));
}

#[test]
fn test_preserve_idat_chunks() {
    // Create PNG with specific IDAT data
    let original_png = create_minimal_png_with_idat();
    let reader = TestReader::new(original_png.clone());

    // Extract original IDAT data for comparison
    let original_idat_data = extract_idat_data(&original_png);

    // Modify metadata (but not image data)
    let mut metadata = MetadataMap::new();
    metadata.insert("PNG:tEXt:Author", TagValue::new_string("Test Author"));

    // Write to temp file
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");

    write_png_metadata(&output_path, &reader, &metadata).unwrap();

    // Read output file and extract IDAT
    let output_data = std::fs::read(&output_path).unwrap();
    let output_idat_data = extract_idat_data(&output_data);

    // Verify IDAT data is unchanged
    assert_eq!(original_idat_data, output_idat_data);
}

#[test]
fn test_round_trip_preservation() {
    // Create PNG with multiple metadata types
    let mut data = Vec::new();
    data.extend_from_slice(&PNG_SIGNATURE);

    // IHDR
    let ihdr_data = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
    let ihdr_crc = calculate_png_crc(b"IHDR", &ihdr_data);
    data.extend_from_slice(&13u32.to_be_bytes());
    data.extend_from_slice(b"IHDR");
    data.extend_from_slice(&ihdr_data);
    data.extend_from_slice(&ihdr_crc.to_be_bytes());

    // tEXt
    let text_data = b"Author\0Test";
    let text_crc = calculate_png_crc(b"tEXt", text_data);
    data.extend_from_slice(&(text_data.len() as u32).to_be_bytes());
    data.extend_from_slice(b"tEXt");
    data.extend_from_slice(text_data);
    data.extend_from_slice(&text_crc.to_be_bytes());

    // IDAT
    let idat_data = [0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01];
    let idat_crc = calculate_png_crc(b"IDAT", &idat_data);
    data.extend_from_slice(&(idat_data.len() as u32).to_be_bytes());
    data.extend_from_slice(b"IDAT");
    data.extend_from_slice(&idat_data);
    data.extend_from_slice(&idat_crc.to_be_bytes());

    // IEND
    let iend_crc = calculate_png_crc(b"IEND", &[]);
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(b"IEND");
    data.extend_from_slice(&iend_crc.to_be_bytes());

    let reader = TestReader::new(data);

    // Parse original
    let original_metadata = parse_png_metadata(&reader).unwrap();

    // Write to file
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");
    write_png_metadata(&output_path, &reader, &original_metadata).unwrap();

    // Read back and verify
    let output_reader = BufferedReader::new(&output_path).unwrap();
    let roundtrip_metadata = parse_png_metadata(&output_reader).unwrap();

    assert_eq!(
        original_metadata.get_string("PNG:tEXt:Author"),
        roundtrip_metadata.get_string("PNG:tEXt:Author")
    );
}

#[test]
fn test_remove_metadata_chunk() {
    // Create PNG with text chunk
    let original_png = create_png_with_text();
    let reader = TestReader::new(original_png);

    // Parse original to verify it has metadata
    let original_metadata = parse_png_metadata(&reader).unwrap();
    assert_eq!(
        original_metadata.get_string("PNG:tEXt:Author"),
        Some("Original Author")
    );

    // Write with empty metadata (should remove the tEXt chunk)
    let empty_metadata = MetadataMap::new();
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");

    write_png_metadata(&output_path, &reader, &empty_metadata).unwrap();

    // Read back and verify metadata is gone
    let output_reader = BufferedReader::new(&output_path).unwrap();
    let parsed_metadata = parse_png_metadata(&output_reader).unwrap();

    assert!(parsed_metadata.get_string("PNG:tEXt:Author").is_none());
}

#[test]
fn test_mixed_metadata_types() {
    // Create a minimal PNG
    let original_png = create_minimal_png_with_idat();
    let reader = TestReader::new(original_png);

    // Create metadata with mixed types
    let mut metadata = MetadataMap::new();
    metadata.insert("PNG:tEXt:Author", TagValue::new_string("John Doe"));
    metadata.insert("PNG:iTXt:Description", TagValue::new_string("Test 测试"));
    metadata.insert("IFD0:Make", TagValue::new_string("TestMake"));

    // Write to temp file
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");

    write_png_metadata(&output_path, &reader, &metadata).unwrap();

    // Read back and verify all types preserved
    let output_reader = BufferedReader::new(&output_path).unwrap();
    let parsed_metadata = parse_png_metadata(&output_reader).unwrap();

    assert_eq!(
        parsed_metadata.get_string("PNG:tEXt:Author"),
        Some("John Doe")
    );
    assert_eq!(
        parsed_metadata.get_string("PNG:iTXt:Description"),
        Some("Test 测试")
    );
    // EXIF tags are returned with IFD0: prefix
    assert_eq!(parsed_metadata.get_string("IFD0:Make"), Some("TestMake"));
}

#[test]
fn test_crc_recalculation() {
    // Create a minimal PNG
    let original_png = create_minimal_png_with_idat();
    let reader = TestReader::new(original_png);

    // Add metadata
    let mut metadata = MetadataMap::new();
    metadata.insert("PNG:tEXt:Test", TagValue::new_string("Value"));

    // Write to temp file
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.png");

    write_png_metadata(&output_path, &reader, &metadata).unwrap();

    // Read the raw file and verify CRC is correct for tEXt chunk
    let output_data = std::fs::read(&output_path).unwrap();

    // Find tEXt chunk and verify its CRC
    let mut offset = 8; // After signature
    while offset < output_data.len() {
        let length = u32::from_be_bytes([
            output_data[offset],
            output_data[offset + 1],
            output_data[offset + 2],
            output_data[offset + 3],
        ]) as usize;

        let chunk_type = &output_data[offset + 4..offset + 8];

        if chunk_type == b"tEXt" {
            let data = &output_data[offset + 8..offset + 8 + length];
            let stored_crc = u32::from_be_bytes([
                output_data[offset + 8 + length],
                output_data[offset + 8 + length + 1],
                output_data[offset + 8 + length + 2],
                output_data[offset + 8 + length + 3],
            ]);

            let calculated_crc = calculate_png_crc(chunk_type.try_into().unwrap(), data);

            assert_eq!(stored_crc, calculated_crc, "CRC mismatch for tEXt chunk");
            break;
        }

        offset += 12 + length; // length(4) + type(4) + data(length) + crc(4)
    }
}

/// Helper function to extract IDAT chunk data from PNG bytes
fn extract_idat_data(png_data: &[u8]) -> Vec<u8> {
    let mut idat_data = Vec::new();
    let mut offset = 8; // Skip signature

    while offset < png_data.len() {
        let length = u32::from_be_bytes([
            png_data[offset],
            png_data[offset + 1],
            png_data[offset + 2],
            png_data[offset + 3],
        ]) as usize;

        let chunk_type = &png_data[offset + 4..offset + 8];

        if chunk_type == b"IDAT" {
            idat_data.extend_from_slice(&png_data[offset + 8..offset + 8 + length]);
        }

        if chunk_type == b"IEND" {
            break;
        }

        offset += 12 + length;
    }

    idat_data
}
