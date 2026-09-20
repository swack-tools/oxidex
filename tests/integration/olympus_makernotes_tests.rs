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

/// Pinned ExifTool 13.59 renders Olympus::CameraSettings 0x0804's two
/// `int32u` values `0 0` as `No`. This fails if the generated IFD row is
/// withheld or if its source-derived fixed-array PrintConv is not applied.
#[test]
fn tg_870_reports_generated_stacked_image() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    let path = Path::new(
        "/Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples/Olympus/OlympusTG-870.jpg",
    );
    assert!(path.is_file(), "pinned Olympus fixture must be available");

    let metadata = read_metadata(path).expect("Olympus TG-870 parses");
    assert_eq!(metadata.get_string("Olympus:StackedImage"), Some("No"));
}

/// MakerNotes.pm routes a `CAMER\\0` Olympus-layout note through
/// Olympus::Main at byte 8 and selects the binary CameraParameters variant.
/// This catches both a missing dispatcher route and a rejected Olympus header.
#[test]
fn camer_makernote_reports_generated_camera_parameters() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    let path = Path::new(
        "/Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples/Pentax/PentaxIQ_Digital59e.jpg",
    );
    assert!(path.is_file(), "pinned CAMER fixture must be available");

    let metadata = read_metadata(path).expect("Pentax IQ Digital 59e parses");
    assert_eq!(
        metadata.get_string("Olympus:CameraParameters"),
        Some("(Binary data 6058 bytes, use -b option to extract)")
    );
}
