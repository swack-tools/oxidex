//! Integration tests for Panasonic MakerNotes parser
//!
//! Tests the Panasonic MakerNotes parsing functionality including:
//! - Lens database lookups (M43 and L-mount)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_panasonic_parser_trait() {
    use oxidex::parsers::tiff::makernotes::panasonic::PanasonicParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = PanasonicParser;

    // Test trait methods
    assert_eq!(parser.manufacturer_name(), "Panasonic");
    assert_eq!(parser.tag_prefix(), "Panasonic:");

    // Test header validation
    let valid_header = b"Panasonic\0\0\0extra data here";
    assert!(parser.validate_header(valid_header));

    let invalid_header = b"Nikon\0\x00\x00";
    assert!(!parser.validate_header(invalid_header));

    let too_short = b"Panasonic";
    assert!(!parser.validate_header(too_short));
}

#[test]
fn test_panasonic_is_panasonic_makernote() {
    use oxidex::parsers::tiff::makernotes::panasonic::is_panasonic_makernote;

    // Valid Panasonic header
    assert!(is_panasonic_makernote(b"Panasonic\0\0\0"));

    // Valid with extra data
    assert!(is_panasonic_makernote(b"Panasonic\0\0\0extra data"));

    // Invalid - Nikon header
    assert!(!is_panasonic_makernote(b"Nikon\0"));

    // Invalid - too short
    assert!(!is_panasonic_makernote(b"Panasonic"));

    // Invalid - wrong signature
    assert!(!is_panasonic_makernote(b"Canon\0\0\0"));
}

#[test]
fn test_panasonic_parse_basic_tags() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    // Create minimal Panasonic MakerNote
    let mut data = Vec::new();

    // Panasonic header (12 bytes)
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: entry count (little-endian)
    data.extend_from_slice(&[0x02, 0x00]); // 2 entries

    // Entry 1: WhiteBalance (tag 0x0003) = Cloudy (value 3)
    // Registry: 0x0003 = WhiteBalance with WHITE_BALANCE decoder
    data.extend_from_slice(&[0x03, 0x00]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // Value: 3 (Cloudy)

    // Entry 2: MacroMode (tag 0x001C) = On (value 1)
    // Registry: 0x001C = MacroMode with MACRO_MODE decoder
    data.extend_from_slice(&[0x1C, 0x00]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

    // Next IFD offset
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify extracted tags using correct registry tag names
    assert!(tags.contains_key("Panasonic:WhiteBalance"));
    assert_eq!(
        tags.get("Panasonic:WhiteBalance"),
        Some(&"Cloudy".to_string())
    );

    assert!(tags.contains_key("Panasonic:MacroMode"));
    assert_eq!(tags.get("Panasonic:MacroMode"), Some(&"On".to_string()));
}

#[test]
fn test_panasonic_parse_enumerated_values() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 4 entries
    data.extend_from_slice(&[0x04, 0x00]);

    // Entry 1: WhiteBalance (tag 0x0003) = Daylight (value 2)
    // Registry: 0x0003 = WhiteBalance, WHITE_BALANCE decoder: 2 = "Daylight"
    data.extend_from_slice(&[0x03, 0x00]); // Tag ID = 0x0003
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Daylight)

    // Entry 2: FocusMode (tag 0x0007) = AF-S (value 4)
    // Registry: 0x0007 = FocusMode, FOCUS_MODE decoder: 4 = "AF-S (Single)"
    data.extend_from_slice(&[0x07, 0x00]); // Tag ID = 0x0007
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // AF-S

    // Entry 3: ShootingMode (tag 0x001F) = Aperture Priority (value 7)
    // Registry: 0x001F = ShootingMode
    data.extend_from_slice(&[0x1F, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]);

    // Entry 4: FilmMode (tag 0x0042) = Dynamic (color) (value 2)
    // Registry: 0x0042 = FilmMode (table from ExifTool Panasonic.pm)
    data.extend_from_slice(&[0x42, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // 2

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify decoded values
    assert_eq!(
        tags.get("Panasonic:WhiteBalance"),
        Some(&"Daylight".to_string())
    );
    assert_eq!(
        tags.get("Panasonic:FocusMode"),
        Some(&"AF-S (Single)".to_string())
    );
    assert_eq!(
        tags.get("Panasonic:ShootingMode"),
        Some(&"Aperture Priority".to_string())
    );
    assert_eq!(
        tags.get("Panasonic:FilmMode"),
        Some(&"Dynamic (color)".to_string())
    );
}

#[test]
fn test_panasonic_parse_photo_style() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 2 entries
    data.extend_from_slice(&[0x02, 0x00]);

    // Entry 1: PhotoStyle (tag 0x0089) = V-Log (value 10)
    // Registry: 0x0089 = PhotoStyle with PHOTO_STYLE decoder
    data.extend_from_slice(&[0x89, 0x00]); // Tag ID = 0x0089
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x0A, 0x00, 0x00, 0x00]); // 10 (V-Log)

    // Entry 2: HDR (tag 0x009E) = HDR Auto (value 100)
    // Registry: 0x009E = HDR with HDR decoder
    data.extend_from_slice(&[0x9E, 0x00]); // Tag ID = 0x009E
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x64, 0x00, 0x00, 0x00]); // 100 (HDR Auto)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(tags.get("Panasonic:PhotoStyle"), Some(&"V-Log".to_string()));
    assert_eq!(tags.get("Panasonic:HDR"), Some(&"HDR Auto".to_string()));
}

#[test]
fn test_panasonic_parse_empty_data() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut tags = HashMap::new();

    // Empty data should not crash
    parse_panasonic_makernotes(&[], ByteOrder::LittleEndian, &mut tags);
    assert!(tags.is_empty());

    // Invalid header should not crash
    let invalid_data = b"Nikon\0\x00\x00";
    parse_panasonic_makernotes(invalid_data, ByteOrder::LittleEndian, &mut tags);
    // Should have no tags extracted (error case)
}

#[test]
fn test_panasonic_parser_big_endian() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 1 entry (big-endian)
    data.extend_from_slice(&[0x00, 0x01]); // Entry count (BE)

    // Entry: WhiteBalance (tag 0x0003) = Daylight (value 2)
    // Registry: 0x0003 = WhiteBalance, WHITE_BALANCE decoder: 2 = "Daylight"
    data.extend_from_slice(&[0x00, 0x03]); // Tag ID (BE) = 0x0003
    data.extend_from_slice(&[0x00, 0x03]); // Type: SHORT (BE)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // Count: 1 (BE)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]); // Value: 2 (BE) = Daylight

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD (BE)

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::BigEndian, &mut tags);

    assert_eq!(
        tags.get("Panasonic:WhiteBalance"),
        Some(&"Daylight".to_string())
    );
}

#[test]
fn test_panasonic_intelligent_features() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 3 entries
    data.extend_from_slice(&[0x03, 0x00]);

    // Entry 1: IntelligentExposure (tag 0x005D) = Standard (value 2)
    // Registry: 0x005D = IntelligentExposure with INTELLIGENT_EXPOSURE decoder
    data.extend_from_slice(&[0x5D, 0x00]); // Tag ID = 0x005D
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Standard)

    // Entry 2: IntelligentResolution (tag 0x0070) = High (value 3)
    // Registry: 0x0070 = IntelligentResolution with INTELLIGENT_RESOLUTION decoder
    data.extend_from_slice(&[0x70, 0x00]); // Tag ID = 0x0070
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // Value: 3 (High)

    // Entry 3: IntelligentD-Range (tag 0x0079) = Low (value 1)
    // Registry: 0x0079 = IntelligentD-Range with INTELLIGENT_D_RANGE decoder
    data.extend_from_slice(&[0x79, 0x00]); // Tag ID = 0x0079
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Low)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(
        tags.get("Panasonic:IntelligentExposure"),
        Some(&"Standard".to_string())
    );
    assert_eq!(
        tags.get("Panasonic:IntelligentResolution"),
        Some(&"High".to_string())
    );
    assert_eq!(
        tags.get("Panasonic:IntelligentD-Range"),
        Some(&"Low".to_string())
    );
}

/// `%Panasonic::Main` 0x51 `LensType` is `Writable => 'string'`
/// (Panasonic.pm:943): the tag holds the lens name itself, and ExifTool ends
/// the string at its first NUL and then trims trailing spaces
/// (`ValueConv => '$val=~s/ +$//'`).  There is no Panasonic lens-id table.
#[test]
fn test_panasonic_lens_type_is_a_string() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    // "LEICA DG 12-60/F2.8-4.0" is what ExifTool prints for
    // Panasonic/PanasonicDC-G9.jpg, padded here the way that file pads it.
    let text: &[u8] = b"LEICA DG 12-60/F2.8-4.0 \0\0\0\0\0\0 \0\0";

    let mut data = Vec::new();
    data.extend_from_slice(b"Panasonic\0\0\0"); // header
    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x51, 0x00]); // tag 0x0051
    data.extend_from_slice(&[0x02, 0x00]); // type: ASCII
    data.extend_from_slice(&(text.len() as u32).to_le_bytes()); // count
    // value offset, measured from the IFD start: count(2) + entry(12) + next(4)
    data.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // next IFD
    data.extend_from_slice(text);

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(
        tags.get("Panasonic:LensType"),
        Some(&"LEICA DG 12-60/F2.8-4.0".to_string())
    );
}
