//! Integration tests for JPEG EXIF writing operations
//!
//! These tests verify the end-to-end functionality of writing modified EXIF
//! metadata back to JPEG files.

use exiftool_rs::core::metadata_map::MetadataMap;
use exiftool_rs::core::tag_value::TagValue;
use exiftool_rs::io::buffered_reader::BufferedReader;
use exiftool_rs::parsers::jpeg::{parse_segments, Segment};
use exiftool_rs::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
use exiftool_rs::writers::jpeg_writer::write_exif_to_jpeg;
use std::io::{self, Write};
use tempfile::NamedTempFile;

/// Helper: Creates a FileReader from byte buffer
struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl exiftool_rs::core::FileReader for TestReader {
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

/// Helper: Creates a complete valid JPEG with EXIF metadata
fn create_test_jpeg_with_metadata() -> Vec<u8> {
    let mut data = Vec::new();

    // SOI marker
    data.extend_from_slice(&[0xFF, 0xD8]);

    // Build EXIF APP1 segment with Make="Canon" and Model="EOS R5"
    let mut exif_data = Vec::new();

    // EXIF identifier
    exif_data.extend_from_slice(b"Exif\0\0");

    // TIFF header (little-endian)
    exif_data.extend_from_slice(&[0x49, 0x49]); // Little-endian marker
    exif_data.extend_from_slice(&[0x2A, 0x00]); // TIFF magic (42)
    exif_data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // IFD offset = 8

    // IFD: 2 entries (Make, Model)
    // Entry count
    exif_data.extend_from_slice(&[0x02, 0x00]); // 2 entries

    // Entry 1: Make (0x010F)
    exif_data.extend_from_slice(&[0x0F, 0x01]); // Tag ID: Make
    exif_data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    exif_data.extend_from_slice(&[0x06, 0x00, 0x00, 0x00]); // Count: 6 bytes ("Canon\0")
    exif_data.extend_from_slice(&[0x26, 0x00, 0x00, 0x00]); // Offset to value (38 from TIFF header start)

    // Entry 2: Model (0x0110)
    exif_data.extend_from_slice(&[0x10, 0x01]); // Tag ID: Model
    exif_data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    exif_data.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]); // Count: 7 bytes ("EOS R5\0")
    exif_data.extend_from_slice(&[0x2C, 0x00, 0x00, 0x00]); // Offset to value (44 from TIFF header start)

    // Next IFD offset (0 = none)
    exif_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // Value area
    exif_data.extend_from_slice(b"Canon\0"); // Make value
    exif_data.extend_from_slice(b"EOS R5\0"); // Model value

    // Write APP1 segment
    data.extend_from_slice(&[0xFF, 0xE1]); // APP1 marker
    let length = 2 + exif_data.len();
    data.extend_from_slice(&(length as u16).to_be_bytes());
    data.extend_from_slice(&exif_data);

    // EOI marker
    data.extend_from_slice(&[0xFF, 0xD9]);

    data
}

/// Helper: Finds EXIF APP1 segment in segment list
fn find_exif_segment<'a>(segments: &'a [Segment]) -> Option<&'a Segment<'a>> {
    segments.iter().find(|seg| {
        seg.marker == 0xFFE1 && seg.data.starts_with(b"Exif\0\0")
    })
}

/// Helper: Extracts TIFF data from EXIF APP1 segment
fn extract_tiff_data<'a>(exif_segment: &Segment<'a>) -> &'a [u8] {
    // Skip "Exif\0\0" identifier (6 bytes)
    &exif_segment.data[6..]
}

/// Helper: Parses EXIF tags from TIFF data
#[allow(clippy::type_complexity)]
fn parse_exif_tags(tiff_data: &[u8]) -> Result<Vec<(u16, Vec<u8>)>, Box<dyn std::error::Error>> {
    // Create reader for TIFF data
    let reader = TestReader::new(tiff_data.to_vec());

    // Detect byte order from TIFF header
    let byte_order = if &tiff_data[0..2] == b"II" {
        ByteOrder::LittleEndian
    } else {
        ByteOrder::BigEndian
    };

    // IFD starts at offset 8 (after TIFF header)
    let tags = parse_ifd(&reader, 8, byte_order)?;

    Ok(tags)
}

#[test]
fn test_modify_exif_tag_in_jpeg() {
    // Create test JPEG with Make="Canon" and Model="EOS R5"
    let original_jpeg = create_test_jpeg_with_metadata();
    let reader = TestReader::new(original_jpeg);

    // Parse original EXIF to verify starting state
    let original_segments = parse_segments(&reader).unwrap();
    let original_exif = find_exif_segment(&original_segments).expect("Should have EXIF segment");
    let original_tiff = extract_tiff_data(original_exif);
    let original_tags = parse_exif_tags(original_tiff).unwrap();

    // Verify original Make value
    let original_make = original_tags.iter().find(|(id, _)| *id == 0x010F);
    assert!(original_make.is_some(), "Original should have Make tag");
    let (_, make_value) = original_make.unwrap();
    assert_eq!(make_value, b"Canon\0");

    // Create modified metadata with new Artist tag
    let mut metadata = MetadataMap::new();
    metadata.insert("EXIF:Artist", TagValue::new_string("TestArtist"));
    metadata.insert("EXIF:Make", TagValue::new_string("ModifiedMake"));

    // Write modified JPEG
    let modified_jpeg = write_exif_to_jpeg(&reader, &metadata).expect("Write should succeed");

    // Verify modified JPEG is valid
    assert_eq!(&modified_jpeg[0..2], &[0xFF, 0xD8], "Should start with SOI");
    assert_eq!(&modified_jpeg[modified_jpeg.len()-2..], &[0xFF, 0xD9], "Should end with EOI");

    // Parse modified JPEG
    let modified_reader = TestReader::new(modified_jpeg);
    let modified_segments = parse_segments(&modified_reader).unwrap();
    let modified_exif = find_exif_segment(&modified_segments).expect("Should have EXIF segment");
    let modified_tiff = extract_tiff_data(modified_exif);
    let modified_tags = parse_exif_tags(modified_tiff).unwrap();

    // Verify Artist tag was added
    let artist = modified_tags.iter().find(|(id, _)| *id == 0x013B); // Artist tag ID
    assert!(artist.is_some(), "Should have Artist tag");
    let (_, artist_value) = artist.unwrap();
    assert_eq!(artist_value, b"TestArtist\0");

    // Verify Make tag was modified
    let make = modified_tags.iter().find(|(id, _)| *id == 0x010F);
    assert!(make.is_some(), "Should have Make tag");
    let (_, make_value) = make.unwrap();
    assert_eq!(make_value, b"ModifiedMake\0");
}

#[test]
fn test_preserve_non_exif_segments() {
    // Create JPEG with APP0 (JFIF) + EXIF + APP2
    let mut jpeg = Vec::new();

    // SOI
    jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // APP0 (JFIF)
    jpeg.extend_from_slice(&[0xFF, 0xE0]);
    jpeg.extend_from_slice(&[0x00, 0x06]); // Length
    jpeg.extend_from_slice(&[0x4A, 0x46, 0x49, 0x46]); // "JFIF"

    // APP1 (EXIF) - minimal
    jpeg.extend_from_slice(&[0xFF, 0xE1]);
    jpeg.extend_from_slice(&[0x00, 0x10]); // Length
    jpeg.extend_from_slice(b"Exif\0\0");
    jpeg.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]); // TIFF header

    // APP2 (ICC profile marker)
    jpeg.extend_from_slice(&[0xFF, 0xE2]);
    jpeg.extend_from_slice(&[0x00, 0x04]); // Length
    jpeg.extend_from_slice(&[0xAA, 0xBB]); // Dummy data

    // EOI
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    let reader = TestReader::new(jpeg);

    // Modify EXIF
    let mut metadata = MetadataMap::new();
    metadata.insert("EXIF:Copyright", TagValue::new_string("Test"));

    let modified_jpeg = write_exif_to_jpeg(&reader, &metadata).expect("Write should succeed");

    // Parse modified JPEG
    let modified_reader = TestReader::new(modified_jpeg);
    let segments = parse_segments(&modified_reader).unwrap();

    // Should have: SOI, APP0, APP1 (EXIF), APP2, EOI
    assert_eq!(segments.len(), 5, "Should preserve all segments");
    assert_eq!(segments[0].marker, 0xFFD8, "SOI preserved");
    assert_eq!(segments[1].marker, 0xFFE0, "APP0 preserved");
    assert_eq!(segments[2].marker, 0xFFE1, "APP1 (EXIF) present");
    assert_eq!(segments[3].marker, 0xFFE2, "APP2 preserved");
    assert_eq!(segments[4].marker, 0xFFD9, "EOI preserved");

    // Verify APP0 data unchanged
    assert_eq!(&segments[1].data[0..4], b"JFIF");

    // Verify APP2 data unchanged
    assert_eq!(segments[3].data, &[0xAA, 0xBB]);
}

#[test]
fn test_insert_exif_when_missing() {
    // Create JPEG without EXIF
    let mut jpeg = Vec::new();
    jpeg.extend_from_slice(&[0xFF, 0xD8]); // SOI
    jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI

    let reader = TestReader::new(jpeg);

    // Add EXIF metadata
    let mut metadata = MetadataMap::new();
    metadata.insert("EXIF:Make", TagValue::new_string("NewCamera"));

    let modified_jpeg = write_exif_to_jpeg(&reader, &metadata).expect("Write should succeed");

    // Parse modified JPEG
    let modified_reader = TestReader::new(modified_jpeg);
    let segments = parse_segments(&modified_reader).unwrap();

    // Should now have EXIF segment
    let exif_seg = find_exif_segment(&segments);
    assert!(exif_seg.is_some(), "EXIF segment should be inserted");

    // Verify Make tag
    let tiff_data = extract_tiff_data(exif_seg.unwrap());
    let tags = parse_exif_tags(tiff_data).unwrap();
    let make = tags.iter().find(|(id, _)| *id == 0x010F);
    assert!(make.is_some());
    let (_, make_value) = make.unwrap();
    assert_eq!(make_value, b"NewCamera\0");
}

#[test]
fn test_handle_size_changes() {
    // Create JPEG with short EXIF
    let original_jpeg = create_test_jpeg_with_metadata();
    let reader = TestReader::new(original_jpeg.clone());

    // Get original size
    let original_size = original_jpeg.len();

    // Add large metadata (longer than original)
    let mut metadata = MetadataMap::new();
    metadata.insert("EXIF:Make", TagValue::new_string("VeryLongCameraManufacturerName"));
    metadata.insert("EXIF:Model", TagValue::new_string("VeryLongCameraModelNameHere"));
    metadata.insert("EXIF:Artist", TagValue::new_string("VeryLongArtistName"));
    metadata.insert("EXIF:Copyright", TagValue::new_string("VeryLongCopyrightNotice"));

    let larger_jpeg = write_exif_to_jpeg(&reader, &metadata).expect("Write should succeed");

    // Should be larger than original
    assert!(larger_jpeg.len() > original_size, "JPEG should be larger with more metadata");

    // Should still be valid
    let larger_reader = TestReader::new(larger_jpeg);
    let segments = parse_segments(&larger_reader);
    assert!(segments.is_ok(), "Larger JPEG should be valid");

    // Now write smaller metadata
    let mut small_metadata = MetadataMap::new();
    small_metadata.insert("EXIF:Make", TagValue::new_string("X"));

    let smaller_jpeg = write_exif_to_jpeg(&reader, &small_metadata).expect("Write should succeed");

    // Should be smaller than original
    assert!(smaller_jpeg.len() < original_size, "JPEG should be smaller with less metadata");

    // Should still be valid
    let smaller_reader = TestReader::new(smaller_jpeg);
    let segments = parse_segments(&smaller_reader);
    assert!(segments.is_ok(), "Smaller JPEG should be valid");
}

#[test]
fn test_write_to_real_file() -> Result<(), Box<dyn std::error::Error>> {
    // Create test JPEG
    let jpeg = create_test_jpeg_with_metadata();

    // Write to temp file
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(&jpeg)?;
    let temp_path = temp_file.path();

    // Read using BufferedReader
    let reader = BufferedReader::new(temp_path)?;

    // Modify metadata
    let mut metadata = MetadataMap::new();
    metadata.insert("EXIF:Artist", TagValue::new_string("FileTestArtist"));
    metadata.insert("EXIF:Software", TagValue::new_string("exiftool-rs"));

    // Write modified JPEG
    let modified_jpeg = write_exif_to_jpeg(&reader, &metadata)?;

    // Write modified to another temp file
    let mut output_file = NamedTempFile::new()?;
    output_file.write_all(&modified_jpeg)?;
    let output_path = output_file.path();

    // Read back and verify
    let verify_reader = BufferedReader::new(output_path)?;
    let segments = parse_segments(&verify_reader)?;
    let exif_seg = find_exif_segment(&segments).expect("Should have EXIF");
    let tiff_data = extract_tiff_data(exif_seg);
    let tags = parse_exif_tags(tiff_data)?;

    // Verify Artist
    let artist = tags.iter().find(|(id, _)| *id == 0x013B);
    assert!(artist.is_some());
    let (_, artist_value) = artist.unwrap();
    assert_eq!(artist_value, b"FileTestArtist\0");

    // Verify Software
    let software = tags.iter().find(|(id, _)| *id == 0x0131);
    assert!(software.is_some());
    let (_, software_value) = software.unwrap();
    assert_eq!(software_value, b"exiftool-rs\0");

    Ok(())
}

#[test]
fn test_preserve_xmp_alongside_exif() {
    // Create JPEG with both EXIF and XMP APP1 segments
    let mut jpeg = Vec::new();

    // SOI
    jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // APP1 (EXIF)
    jpeg.extend_from_slice(&[0xFF, 0xE1]);
    jpeg.extend_from_slice(&[0x00, 0x10]); // Length
    jpeg.extend_from_slice(b"Exif\0\0");
    jpeg.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);

    // APP1 (XMP)
    jpeg.extend_from_slice(&[0xFF, 0xE1]);
    let xmp_data = b"http://ns.adobe.com/xap/1.0/\0<xmp>test</xmp>";
    let xmp_length = 2 + xmp_data.len();
    jpeg.extend_from_slice(&(xmp_length as u16).to_be_bytes());
    jpeg.extend_from_slice(xmp_data);

    // EOI
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    let reader = TestReader::new(jpeg);

    // Modify EXIF
    let mut metadata = MetadataMap::new();
    metadata.insert("EXIF:Make", TagValue::new_string("TestCamera"));

    let modified_jpeg = write_exif_to_jpeg(&reader, &metadata).unwrap();

    // Parse modified JPEG
    let modified_reader = TestReader::new(modified_jpeg);
    let segments = parse_segments(&modified_reader).unwrap();

    // Should have both EXIF and XMP
    let exif_segments: Vec<_> = segments.iter().filter(|s| s.marker == 0xFFE1).collect();
    assert_eq!(exif_segments.len(), 2, "Should have 2 APP1 segments");

    // One should be EXIF
    let has_exif = exif_segments.iter().any(|s| s.data.starts_with(b"Exif\0\0"));
    assert!(has_exif, "Should have EXIF segment");

    // One should be XMP
    let has_xmp = exif_segments.iter().any(|s| s.data.starts_with(b"http://ns.adobe.com/xap/1.0/\0"));
    assert!(has_xmp, "Should have XMP segment");

    // Verify XMP content preserved
    let xmp_seg = exif_segments.iter().find(|s| s.data.starts_with(b"http://ns.adobe.com/xap/1.0/\0"));
    assert!(xmp_seg.is_some());
    let xmp_content = xmp_seg.unwrap().data;
    // XMP identifier is 29 bytes, then comes the XMP content
    let xmp_payload = &xmp_content[29..];
    assert_eq!(xmp_payload, b"<xmp>test</xmp>", "XMP content should be preserved");
}
