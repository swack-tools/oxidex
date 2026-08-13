//! Integration tests for Olympus MakerNotes parser
//!
//! Tests the Olympus MakerNotes parsing functionality including:
//! - Lens database lookups (Four Thirds and Micro Four Thirds)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

const OM_1: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Olympus/OlympusOM-1.jpg";

/// OM-1 stores these fields in CameraSettings' nested AFTargetInfo and
/// SubjectDetectInfo binary records.  Expectations are pinned from ExifTool
/// 13.59 with `-G1 -s` on the real corpus fixture.
#[test]
fn om_1_reports_nested_af_and_subject_detection_tags() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(OM_1).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(OM_1)).expect("OM-1 parses");
    assert_eq!(metadata.get_string("Olympus:AFFrameSize"), Some("640 480"));
    assert_eq!(metadata.get_string("Olympus:AFFocusArea"), Some("0 0 0 0"));
    assert_eq!(
        metadata.get_string("Olympus:SubjectDetectFrameSize"),
        Some("640 480")
    );
    assert_eq!(
        metadata.get_string("Olympus:SubjectDetectArea"),
        Some("0 0 0 0")
    );
    assert_eq!(
        metadata.get_string("Olympus:SubjectDetectStatus"),
        Some("No Subject or Face Detected")
    );
}

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
