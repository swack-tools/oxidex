//! Integration tests for Olympus MakerNotes parser
//!
//! Tests the Olympus MakerNotes parsing functionality including:
//! - Lens database lookups (Four Thirds and Micro Four Thirds)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_olympus_header_validation() {
    use oxidex::parsers::tiff::makernotes::olympus::OlympusParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = OlympusParser;

    // Test valid little-endian header
    let header_le = b"OLYMPUS\0II\x03\x00extra data";
    assert!(parser.validate_header(header_le));

    // Test valid big-endian header
    let header_be = b"OLYMPUS\0MM\x00\x03extra data";
    assert!(parser.validate_header(header_be));

    // Test invalid header
    let invalid = b"NIKON\0\0\0";
    assert!(!parser.validate_header(invalid));

    // Test short data
    let short = b"OLYMP";
    assert!(!parser.validate_header(short));
}

#[test]
fn test_olympus_parser_empty_data() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::olympus::OlympusParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = OlympusParser;
    let mut tags = HashMap::new();

    // Empty data should return Ok without errors
    let result = parser.parse(&[], ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());
    assert!(tags.is_empty());
}

#[test]
fn test_olympus_parser_invalid_header() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::olympus::OlympusParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = OlympusParser;
    let mut tags = HashMap::new();

    // Invalid header should return error
    let data = b"NIKON\0\0\0invalid header";
    let result = parser.parse(data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}

#[test]
fn test_olympus_parser_trait_implementation() {
    use oxidex::parsers::tiff::makernotes::olympus::OlympusParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = OlympusParser;
    assert_eq!(parser.manufacturer_name(), "Olympus");
    assert_eq!(parser.tag_prefix(), "Olympus:");
}

fn little_endian_entry(tag: u16, data_type: u16, count: u32, value: u32) -> Vec<u8> {
    let mut entry = Vec::with_capacity(12);
    entry.extend_from_slice(&tag.to_le_bytes());
    entry.extend_from_slice(&data_type.to_le_bytes());
    entry.extend_from_slice(&count.to_le_bytes());
    entry.extend_from_slice(&value.to_le_bytes());
    entry
}

fn olympus_stacked_image_fixture(first: u32, second: u32) -> Vec<u8> {
    const CAMERA_SETTINGS_OFFSET: u32 = 40;
    const STACKED_IMAGE_VALUES_OFFSET: u32 = 64;

    let mut note = b"OLYMPUS\0II\x03\0".to_vec();
    note.extend_from_slice(&1u16.to_le_bytes());
    note.extend_from_slice(&little_endian_entry(0x2020, 4, 1, CAMERA_SETTINGS_OFFSET));
    note.extend_from_slice(&0u32.to_le_bytes());
    note.resize(CAMERA_SETTINGS_OFFSET as usize, 0);
    note.extend_from_slice(&1u16.to_le_bytes());
    note.extend_from_slice(&little_endian_entry(
        0x0804,
        4,
        2,
        STACKED_IMAGE_VALUES_OFFSET,
    ));
    note.extend_from_slice(&0u32.to_le_bytes());
    note.resize(STACKED_IMAGE_VALUES_OFFSET as usize, 0);
    note.extend_from_slice(&first.to_le_bytes());
    note.extend_from_slice(&second.to_le_bytes());
    note
}

fn camer_fixture(payload_len: usize, payload_tiff_offset: u32) -> Vec<u8> {
    const PAYLOAD_OFFSET: u32 = 32;

    let mut note = b"CAMER\0\0\0".to_vec();
    note.extend_from_slice(&1u16.to_le_bytes());
    note.extend_from_slice(&little_endian_entry(
        0x2050,
        7,
        payload_len as u32,
        payload_tiff_offset + PAYLOAD_OFFSET,
    ));
    note.extend_from_slice(&0u32.to_le_bytes());
    note.resize(PAYLOAD_OFFSET as usize, 0);
    note.resize(PAYLOAD_OFFSET as usize + payload_len, 0x5a);
    note
}

/// Pinned ExifTool 13.59 renders Olympus::CameraSettings 0x0804's two
/// `int32u` values `0 0` as `No`. The generated fixture exercises the same
/// dispatcher, nested IFD and generated-row path without a private corpus.
#[test]
fn generated_fixture_reports_stacked_image() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernote_dispatcher::dispatch_makernote;
    use std::collections::HashMap;

    let mut tags = HashMap::new();
    dispatch_makernote(
        "OLYMPUS CORPORATION",
        &olympus_stacked_image_fixture(0, 0),
        ByteOrder::LittleEndian,
        &mut tags,
    )
    .expect("synthetic Olympus MakerNote parses");
    assert_eq!(tags.get("Olympus:StackedImage"), Some(&"No".to_string()));
}

/// MakerNotes.pm routes a `CAMER\\0` Olympus-layout note through
/// Olympus::Main at byte 8 and selects the binary CameraParameters variant.
/// The generated fixture keeps this default regression independent of a
/// machine-specific Pentax sample path.
#[test]
fn generated_camer_fixture_reports_camera_parameters() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernote_dispatcher::dispatch_makernote_with_context;
    use oxidex::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
    use std::collections::HashMap;

    const MAKERNOTE_OFFSET: usize = 96;
    let note = camer_fixture(24, MAKERNOTE_OFFSET as u32);
    let mut tiff = vec![0u8; MAKERNOTE_OFFSET];
    tiff.extend_from_slice(&note);
    let context = MakerNoteContext::in_tiff(&tiff, MAKERNOTE_OFFSET, note.len(), 0);
    let mut tags = HashMap::new();
    dispatch_makernote_with_context(
        "PENTAX Corporation",
        None,
        &context,
        ByteOrder::LittleEndian,
        &mut tags,
    )
    .expect("synthetic CAMER MakerNote parses");
    assert_eq!(
        tags.get("Olympus:CameraParameters"),
        Some(&"(Binary data 24 bytes, use -b option to extract)".to_string())
    );
}
