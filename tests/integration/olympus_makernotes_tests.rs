//! Integration tests for Olympus MakerNotes parser
//!
//! Tests the Olympus MakerNotes parsing functionality including:
//! - Lens database lookups (Four Thirds and Micro Four Thirds)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

const OM_1: &str = "Olympus/OlympusOM-1.jpg";
const U20D: &str = "Olympus/Olympus_u20D.jpg";
const U1040: &str = "Olympus/Olympus_u1040.jpg";
const E_P5: &str = "Olympus/OlympusE-P5.jpg";
const TG_870: &str = "Olympus/OlympusTG-870.jpg";

/// The legacy u20D MakerNote stores `WBMode` in Olympus::Main 0x1015.  The
/// exact display value is pinned from ExifTool 13.59 on the real fixture.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn u20d_reports_main_white_balance_mode() {
    use oxidex::core::operations::read_metadata;

    let metadata =
        read_metadata(&crate::fixtures::required_combined_fixture_path(U20D)).expect("u20D parses");
    assert_eq!(metadata.get_string("Olympus:WBMode"), Some("Auto"));
}

/// Olympus::Main 0x0f04/0x0f05 declare a ZoomedPreviewImage outside
/// this 19,778-byte file. Never invent its bytes from the native display
/// placeholder: targeted native extraction warns that the image cannot be read.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn u1040_refuses_out_of_bounds_zoomed_preview_image() {
    use oxidex::core::operations::read_metadata;

    let metadata = read_metadata(&crate::fixtures::required_combined_fixture_path(U1040))
        .expect("u1040 parses");
    assert_eq!(
        metadata.get_integer("Olympus:ZoomedPreviewStart"),
        Some(4_184_638)
    );
    assert_eq!(
        metadata.get_integer("Olympus:ZoomedPreviewLength"),
        Some(92_592)
    );
    assert!(
        4_184_638
            > std::fs::metadata(crate::fixtures::required_combined_fixture_path(U1040))
                .unwrap()
                .len()
    );
    assert!(metadata.get("Olympus:ZoomedPreviewImage").is_none());
}

/// The E-P5 is an older-model `AFPointDetails` variant, which ExifTool 13.59
/// deliberately reports as raw `0`.  Its cached-empty Extender record is the
/// one fully certain composite path: `None` means `Not attached`.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn e_p5_reports_legacy_af_point_details_and_empty_extender_status() {
    use oxidex::core::operations::read_metadata;

    let metadata =
        read_metadata(&crate::fixtures::required_combined_fixture_path(E_P5)).expect("E-P5 parses");
    assert_eq!(metadata.get_integer("Olympus:AFPointDetails"), Some(0));
    assert_eq!(
        metadata.get_string("Composite:ExtenderStatus"),
        Some("Not attached")
    );
}

/// ExifTool 13.59's full two-column `StackedImage` conversion reports this
/// TG-870 record as `No`, not two independently printed zeroes.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn tg_870_reports_stacked_image() {
    use oxidex::core::operations::read_metadata;

    let metadata = read_metadata(&crate::fixtures::required_combined_fixture_path(TG_870))
        .expect("TG-870 parses");
    assert_eq!(metadata.get_string("Olympus:StackedImage"), Some("No"));
}

/// OM-1 stores these fields in CameraSettings' nested AFTargetInfo and
/// SubjectDetectInfo binary records.  Expectations are pinned from ExifTool
/// 13.59 with `-G1 -s` on the real corpus fixture.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn om_1_reports_nested_af_and_subject_detection_tags() {
    use oxidex::core::operations::read_metadata;

    let metadata =
        read_metadata(&crate::fixtures::required_combined_fixture_path(OM_1)).expect("OM-1 parses");
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

/// ExifTool 13.59 reads this real OM-1 fixture's FocusInfo sub-IFD and its
/// nested AFInfo binary record.  The two values are scalar fields, not
/// display conversions: `AntiShockWaitingTime` is FocusInfo 0x2100 and
/// `CAFSensitivity` is signed byte 0x062c of AFInfo.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn om_1_reports_focus_info_wait_and_caf_sensitivity() {
    use oxidex::core::operations::read_metadata;

    let metadata =
        read_metadata(&crate::fixtures::required_combined_fixture_path(OM_1)).expect("OM-1 parses");
    assert_eq!(
        metadata.get_integer("Olympus:AntiShockWaitingTime"),
        Some(0)
    );
    assert_eq!(metadata.get_integer("Olympus:CAFSensitivity"), Some(0));
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
