//! Integration tests for Phase One MakerNotes parser
//!
//! Tests the Phase One MakerNotes parsing functionality including:
//! - Lens database lookups (Schneider, Mamiya, Rodenstock lenses)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_phaseone_header_validation_with_header() {
    use oxidex::parsers::tiff::makernotes::phaseone::is_phaseone_makernote;

    // Test valid "Phase One" header
    let valid_header = b"Phase One\x00\x10\x00\x00";
    assert!(is_phaseone_makernote(valid_header));

    // Test invalid header
    let invalid_header = b"Canon\0\0\0";
    assert!(!is_phaseone_makernote(invalid_header));
}

#[test]
fn test_phaseone_header_validation_no_header() {
    use oxidex::parsers::tiff::makernotes::phaseone::is_phaseone_makernote;

    // Test data with no header but valid IFD entry count (8 entries)
    let no_header = b"\x08\x00\x00\x00\x00\x00\x00\x00";
    assert!(is_phaseone_makernote(no_header));

    // Test data with unreasonable entry count (should fail)
    let bad_count = b"\xFF\xFF\x00\x00\x00\x00\x00\x00";
    assert!(!is_phaseone_makernote(bad_count));

    // Test too short data
    let too_short = b"\x05";
    assert!(!is_phaseone_makernote(too_short));
}

#[test]
fn test_phaseone_makernote_parse_basic() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::phaseone::PhaseOneMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = PhaseOneMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic Phase One MakerNote data with 2 IFD entries (no header)
    // Entry count: 2 (little-endian u16)
    // Entry 1: System Type tag (0x0109) = 4 (XF Camera System)
    // Entry 2: ISO tag (0x0401) = 100
    let mut data = Vec::new();
    data.extend_from_slice(&[0x02, 0x00]); // 2 entries (little-endian)

    // Entry 1: System Type (0x0109), type SHORT (3), count 1, value 4
    data.extend_from_slice(&[0x09, 0x01]); // tag: 0x0109
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // value: 4

    // Entry 2: ISO (0x0401), type SHORT (3), count 1, value 100
    data.extend_from_slice(&[0x01, 0x04]); // tag: 0x0401
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x64, 0x00, 0x00, 0x00]); // value: 100

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(
        tags.get("PhaseOne:SystemType"),
        Some(&"XF Camera System".to_string())
    );
    assert_eq!(tags.get("PhaseOne:ISO"), Some(&"100".to_string()));
}

#[test]
fn test_phaseone_makernote_parse_exposure_settings() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::phaseone::PhaseOneMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = PhaseOneMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic data with exposure settings
    let mut data = Vec::new();
    data.extend_from_slice(&[0x03, 0x00]); // 3 entries

    // Entry 1: Exposure Mode (0x0405) = 2 (Aperture Priority)
    data.extend_from_slice(&[0x05, 0x04, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

    // Entry 2: Metering Mode (0x0406) = 1 (Multi-zone)
    data.extend_from_slice(&[0x06, 0x04, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 3: White Balance (0x0412) = 1 (Daylight)
    data.extend_from_slice(&[0x12, 0x04, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(
        tags.get("PhaseOne:ExposureMode"),
        Some(&"Aperture Priority".to_string())
    );
    assert_eq!(
        tags.get("PhaseOne:MeteringMode"),
        Some(&"Multi-zone".to_string())
    );
    assert_eq!(
        tags.get("PhaseOne:WhiteBalance"),
        Some(&"Daylight".to_string())
    );
}

#[test]
fn test_phaseone_makernote_parse_sensor_info() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::phaseone::PhaseOneMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = PhaseOneMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic data with sensor information
    let mut data = Vec::new();
    data.extend_from_slice(&[0x03, 0x00]); // 3 entries

    // Entry 1: Sensor Width (0x010E) = 8280 pixels
    data.extend_from_slice(&[0x0E, 0x01, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x58, 0x20, 0x00, 0x00]); // 8280 in little-endian

    // Entry 2: Sensor Height (0x010F) = 6208 pixels
    data.extend_from_slice(&[0x0F, 0x01, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x40, 0x18, 0x00, 0x00]); // 6208 in little-endian

    // Entry 3: Sensor Bit Depth (0x0110) = 16 bit
    data.extend_from_slice(&[0x10, 0x01, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]);

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(
        tags.get("PhaseOne:SensorWidth"),
        Some(&"8280 px".to_string())
    );
    assert_eq!(
        tags.get("PhaseOne:SensorHeight"),
        Some(&"6208 px".to_string())
    );
    assert_eq!(
        tags.get("PhaseOne:SensorBitDepth"),
        Some(&"16 bit".to_string())
    );
}

#[test]
fn test_phaseone_makernote_parse_capture_settings() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::phaseone::PhaseOneMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = PhaseOneMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic data with capture settings
    let mut data = Vec::new();
    data.extend_from_slice(&[0x04, 0x00]); // 4 entries

    // Entry 1: Drive Mode (0x0500) = 3 (Mirror Lock-up)
    data.extend_from_slice(&[0x00, 0x05, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);

    // Entry 2: Focus Mode (0x0501) = 0 (Manual)
    data.extend_from_slice(&[0x01, 0x05, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // Entry 3: Mirror Lockup (0x0502) = 1 (On)
    data.extend_from_slice(&[0x02, 0x05, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 4: Live View (0x0503) = 0 (Off)
    data.extend_from_slice(&[0x03, 0x05, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(
        tags.get("PhaseOne:DriveMode"),
        Some(&"Mirror Lock-up".to_string())
    );
    assert_eq!(tags.get("PhaseOne:FocusMode"), Some(&"Manual".to_string()));
    assert_eq!(tags.get("PhaseOne:MirrorLockup"), Some(&"On".to_string()));
    assert_eq!(tags.get("PhaseOne:LiveView"), Some(&"Off".to_string()));
}

#[test]
fn test_phaseone_makernote_parse_error_too_short() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::phaseone::PhaseOneMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = PhaseOneMakerNoteParser;
    let mut tags = HashMap::new();

    // Test with data that's too short (less than 2 bytes)
    let data = b"P";
    let result = parser.parse(data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}

#[test]
fn test_phaseone_makernote_parse_error_invalid_entry_count() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::phaseone::PhaseOneMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = PhaseOneMakerNoteParser;
    let mut tags = HashMap::new();

    // Create data with invalid entry count (200, exceeding limit of 150)
    let data = &[0xC8, 0x00]; // 200 entries (little-endian) - invalid

    let result = parser.parse(data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}

#[test]
fn test_phaseone_makernote_parser_trait_implementation() {
    use oxidex::parsers::tiff::makernotes::phaseone::PhaseOneMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = PhaseOneMakerNoteParser;
    assert_eq!(parser.manufacturer_name(), "PhaseOne");
    assert_eq!(parser.tag_prefix(), "PhaseOne:");
}
