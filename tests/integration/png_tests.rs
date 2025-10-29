//! Integration tests for PNG parser
//!
//! These tests verify PNG metadata extraction with real PNG files.

use exiftool_rs::core::FileReader;
use exiftool_rs::parsers::png::parse_png_metadata;
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

/// PNG signature
const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Creates a minimal valid PNG with IHDR and IEND chunks
fn create_minimal_png() -> Vec<u8> {
    let mut data = Vec::new();

    // PNG signature
    data.extend_from_slice(&PNG_SIGNATURE);

    // IHDR chunk (13 bytes data)
    data.extend_from_slice(&13u32.to_be_bytes()); // Length
    data.extend_from_slice(b"IHDR"); // Type
    data.extend_from_slice(&[
        0, 0, 0, 1, // Width: 1
        0, 0, 0, 1, // Height: 1
        8, // Bit depth
        2, // Color type: RGB
        0, // Compression
        0, // Filter
        0, // Interlace
    ]);
    data.extend_from_slice(&0u32.to_be_bytes()); // CRC (dummy)

    // IEND chunk (0 bytes data)
    data.extend_from_slice(&0u32.to_be_bytes()); // Length
    data.extend_from_slice(b"IEND"); // Type
    data.extend_from_slice(&0u32.to_be_bytes()); // CRC (dummy)

    data
}

/// Creates a PNG with multiple tEXt chunks
fn create_png_with_text() -> Vec<u8> {
    let mut data = create_minimal_png();

    // Insert multiple tEXt chunks before IEND
    let iend_pos = data.len() - 12; // IEND chunk is 12 bytes

    let mut chunks = Vec::new();

    // tEXt chunk 1: Author
    let text1_data = b"Author\0John Doe";
    chunks.extend_from_slice(&(text1_data.len() as u32).to_be_bytes());
    chunks.extend_from_slice(b"tEXt");
    chunks.extend_from_slice(text1_data);
    chunks.extend_from_slice(&0u32.to_be_bytes()); // CRC

    // tEXt chunk 2: Copyright
    let text2_data = b"Copyright\0(C) 2024 Test Corp";
    chunks.extend_from_slice(&(text2_data.len() as u32).to_be_bytes());
    chunks.extend_from_slice(b"tEXt");
    chunks.extend_from_slice(text2_data);
    chunks.extend_from_slice(&0u32.to_be_bytes()); // CRC

    // tEXt chunk 3: Description
    let text3_data = b"Description\0A test image with metadata";
    chunks.extend_from_slice(&(text3_data.len() as u32).to_be_bytes());
    chunks.extend_from_slice(b"tEXt");
    chunks.extend_from_slice(text3_data);
    chunks.extend_from_slice(&0u32.to_be_bytes()); // CRC

    data.splice(iend_pos..iend_pos, chunks);
    data
}

/// Creates a PNG with EXIF data
fn create_png_with_exif() -> Vec<u8> {
    let mut data = create_minimal_png();

    // Create EXIF data with multiple tags (little-endian)
    let mut exif_data = Vec::new();
    exif_data.extend_from_slice(b"II"); // Little-endian
    exif_data.extend_from_slice(&0x002Au16.to_le_bytes()); // Magic
    exif_data.extend_from_slice(&8u32.to_le_bytes()); // IFD offset

    // IFD with 3 entries
    exif_data.extend_from_slice(&3u16.to_le_bytes()); // Entry count

    // Tag 1: Make (0x010F) = "Canon" (at offset 50)
    exif_data.extend_from_slice(&0x010Fu16.to_le_bytes()); // Tag ID
    exif_data.extend_from_slice(&2u16.to_le_bytes()); // Type: ASCII
    exif_data.extend_from_slice(&6u32.to_le_bytes()); // Count: 6
    exif_data.extend_from_slice(&50u32.to_le_bytes()); // Offset

    // Tag 2: Model (0x0110) = "EOS" (inline)
    exif_data.extend_from_slice(&0x0110u16.to_le_bytes()); // Tag ID
    exif_data.extend_from_slice(&2u16.to_le_bytes()); // Type: ASCII
    exif_data.extend_from_slice(&4u32.to_le_bytes()); // Count: 4
    exif_data.extend_from_slice(b"EOS\0"); // Inline value

    // Tag 3: Software (0x0131) = "ExifTool" (at offset 56)
    exif_data.extend_from_slice(&0x0131u16.to_le_bytes()); // Tag ID
    exif_data.extend_from_slice(&2u16.to_le_bytes()); // Type: ASCII
    exif_data.extend_from_slice(&9u32.to_le_bytes()); // Count: 9
    exif_data.extend_from_slice(&56u32.to_le_bytes()); // Offset

    // Next IFD offset: 0
    exif_data.extend_from_slice(&0u32.to_le_bytes());

    // Value data at offset 50: "Canon\0"
    exif_data.extend_from_slice(b"Canon\0");

    // Value data at offset 56: "ExifTool\0"
    exif_data.extend_from_slice(b"ExifTool\0");

    // Insert eXIf chunk before IEND
    let iend_pos = data.len() - 12;
    let mut exif_chunk = Vec::new();
    exif_chunk.extend_from_slice(&(exif_data.len() as u32).to_be_bytes());
    exif_chunk.extend_from_slice(b"eXIf");
    exif_chunk.extend_from_slice(&exif_data);
    exif_chunk.extend_from_slice(&0u32.to_be_bytes());

    data.splice(iend_pos..iend_pos, exif_chunk);
    data
}

/// Creates a PNG with both text and EXIF metadata
fn create_png_with_mixed_metadata() -> Vec<u8> {
    let mut data = create_minimal_png();
    let iend_pos = data.len() - 12;

    let mut chunks = Vec::new();

    // Add tEXt chunk
    let text_data = b"Title\0Test Image";
    chunks.extend_from_slice(&(text_data.len() as u32).to_be_bytes());
    chunks.extend_from_slice(b"tEXt");
    chunks.extend_from_slice(text_data);
    chunks.extend_from_slice(&0u32.to_be_bytes());

    // Add iTXt chunk
    let mut itxt_data = Vec::new();
    itxt_data.extend_from_slice(b"Description");
    itxt_data.push(0);
    itxt_data.push(0); // uncompressed
    itxt_data.push(0);
    itxt_data.extend_from_slice(b"en-US");
    itxt_data.push(0);
    itxt_data.extend_from_slice(b"Description");
    itxt_data.push(0);
    itxt_data.extend_from_slice(b"A wonderful test image");

    chunks.extend_from_slice(&(itxt_data.len() as u32).to_be_bytes());
    chunks.extend_from_slice(b"iTXt");
    chunks.extend_from_slice(&itxt_data);
    chunks.extend_from_slice(&0u32.to_be_bytes());

    // Add EXIF chunk
    let mut exif_data = Vec::new();
    exif_data.extend_from_slice(b"II");
    exif_data.extend_from_slice(&0x002Au16.to_le_bytes());
    exif_data.extend_from_slice(&8u32.to_le_bytes());
    exif_data.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
    exif_data.extend_from_slice(&0x010Fu16.to_le_bytes()); // Make
    exif_data.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    exif_data.extend_from_slice(&4u32.to_le_bytes()); // Count
    exif_data.extend_from_slice(b"Tst\0"); // Inline
    exif_data.extend_from_slice(&0u32.to_le_bytes()); // Next IFD

    chunks.extend_from_slice(&(exif_data.len() as u32).to_be_bytes());
    chunks.extend_from_slice(b"eXIf");
    chunks.extend_from_slice(&exif_data);
    chunks.extend_from_slice(&0u32.to_be_bytes());

    data.splice(iend_pos..iend_pos, chunks);
    data
}

#[test]
fn test_png_with_text_chunks() {
    let png_data = create_png_with_text();
    let reader = TestReader::new(png_data);

    let result = parse_png_metadata(&reader);
    assert!(result.is_ok(), "Failed to parse PNG with text chunks");

    let metadata = result.unwrap();

    // Should have 3 text chunks
    assert_eq!(metadata.len(), 3, "Expected 3 metadata entries");

    // Verify each text chunk
    assert_eq!(
        metadata.get_string("PNG:tEXt:Author"),
        Some("John Doe"),
        "Author tag should be 'John Doe'"
    );

    assert_eq!(
        metadata.get_string("PNG:tEXt:Copyright"),
        Some("(C) 2024 Test Corp"),
        "Copyright tag should be '(C) 2024 Test Corp'"
    );

    assert_eq!(
        metadata.get_string("PNG:tEXt:Description"),
        Some("A test image with metadata"),
        "Description tag should be 'A test image with metadata'"
    );
}

#[test]
fn test_png_with_exif_chunk() {
    let png_data = create_png_with_exif();
    let reader = TestReader::new(png_data);

    let result = parse_png_metadata(&reader);
    assert!(result.is_ok(), "Failed to parse PNG with EXIF chunk");

    let metadata = result.unwrap();

    // Should have 3 EXIF tags
    assert_eq!(metadata.len(), 3, "Expected 3 EXIF tags");

    // Verify EXIF tags (using hex tag IDs)
    // 0x010F = Make, 0x0110 = Model, 0x0131 = Software
    assert_eq!(
        metadata.get_string("EXIF:0x010F"),
        Some("Canon"),
        "Make tag should be 'Canon'"
    );

    assert_eq!(
        metadata.get_string("EXIF:0x0110"),
        Some("EOS"),
        "Model tag should be 'EOS'"
    );

    assert_eq!(
        metadata.get_string("EXIF:0x0131"),
        Some("ExifTool"),
        "Software tag should be 'ExifTool'"
    );
}

#[test]
fn test_png_with_mixed_metadata() {
    let png_data = create_png_with_mixed_metadata();
    let reader = TestReader::new(png_data);

    let result = parse_png_metadata(&reader);
    assert!(result.is_ok(), "Failed to parse PNG with mixed metadata");

    let metadata = result.unwrap();

    // Should have text, iTXt, and EXIF entries
    assert!(
        metadata.len() >= 3,
        "Expected at least 3 metadata entries, got {}",
        metadata.len()
    );

    // Verify text chunk
    assert_eq!(
        metadata.get_string("PNG:tEXt:Title"),
        Some("Test Image"),
        "Title tag should be 'Test Image'"
    );

    // Verify iTXt chunk
    assert_eq!(
        metadata.get_string("PNG:iTXt:Description"),
        Some("A wonderful test image"),
        "Description iTXt tag should be 'A wonderful test image'"
    );

    // Verify EXIF tag
    assert_eq!(
        metadata.get_string("EXIF:0x010F"),
        Some("Tst"),
        "EXIF Make tag should be 'Tst'"
    );
}

#[test]
fn test_png_empty_metadata() {
    let png_data = create_minimal_png();
    let reader = TestReader::new(png_data);

    let result = parse_png_metadata(&reader);
    assert!(result.is_ok(), "Failed to parse minimal PNG: {:?}", result.err());

    let metadata = result.unwrap();
    assert_eq!(metadata.len(), 0, "Minimal PNG should have no metadata");
}

#[test]
fn test_png_invalid_signature() {
    let data = vec![0xFF; 100];
    let reader = TestReader::new(data);

    let result = parse_png_metadata(&reader);
    assert!(result.is_err(), "Should fail on invalid PNG signature");
}

#[test]
fn test_png_truncated_file() {
    let data = PNG_SIGNATURE.to_vec(); // Only signature, no chunks
    let reader = TestReader::new(data);

    let result = parse_png_metadata(&reader);
    // Should either succeed with no metadata or fail gracefully
    // (depending on implementation, both are acceptable for truncated files)
    if let Ok(metadata) = result {
        assert_eq!(metadata.len(), 0);
    }
}
