//! Integration tests for Nikon MakerNotes parser
//!
//! Tests the Nikon MakerNotes parsing functionality including:
//! - Lens database lookups
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_nikon_parser_trait() {
    use oxidex::parsers::tiff::makernotes::nikon::NikonParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = NikonParser;

    // Test trait methods
    assert_eq!(parser.manufacturer_name(), "Nikon");
    assert_eq!(parser.tag_prefix(), "Nikon:");

    // Test header validation
    let valid_header = b"Nikon\0\x02\x10\x00\x00extra data";
    assert!(parser.validate_header(valid_header));

    let invalid_header = b"Canon\0\x00\x00";
    assert!(!parser.validate_header(invalid_header));
}

#[test]
fn test_nikon_is_nikon_makernote() {
    use oxidex::parsers::tiff::makernotes::nikon::is_nikon_makernote;

    // Valid Nikon Type 2 header
    assert!(is_nikon_makernote(b"Nikon\0\x02\x10\x00\x00"));

    // Valid Nikon Type 3 header
    assert!(is_nikon_makernote(b"Nikon\0\x02\x00\x00\x00"));

    // Valid with extra data
    assert!(is_nikon_makernote(b"Nikon\0extra data here"));

    // Invalid - Canon header
    assert!(!is_nikon_makernote(b"Canon\0"));

    // Invalid - too short
    assert!(!is_nikon_makernote(b"Nikon"));

    // Invalid - wrong signature
    assert!(!is_nikon_makernote(b"Sony\0\0\0"));
}

#[test]
fn test_nikon_parse_basic_tags() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::nikon::parse_nikon_makernotes;
    use std::collections::HashMap;

    // Create minimal Nikon MakerNote with Type 2 header and embedded TIFF structure
    let mut data = Vec::new();

    // Nikon Type 2 header (10 bytes): "Nikon\0" + version info
    data.extend_from_slice(b"Nikon\0\x02\x10\x00\x00");

    // Embedded TIFF header (8 bytes at offset 10)
    data.extend_from_slice(b"II"); // Little-endian byte order marker
    data.extend_from_slice(&[0x2A, 0x00]); // TIFF magic number (42)
    data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // IFD offset (8 bytes from TIFF start)

    // IFD at offset 18 (10 + 8): entry count (little-endian)
    data.extend_from_slice(&[0x02, 0x00]); // 2 entries

    // Entry 1: ISO (tag 0x0002)
    data.extend_from_slice(&[0x02, 0x00]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x64, 0x00, 0x00, 0x00]); // Value: 100 (ISO 100)

    // Entry 2: Shutter Count (tag 0x00A7)
    data.extend_from_slice(&[0xA7, 0x00]); // Tag ID
    data.extend_from_slice(&[0x04, 0x00]); // Type: LONG
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x10, 0x27, 0x00, 0x00]); // Value: 10000 shutter count

    // Next IFD offset
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify extracted tags. Nikon.pm calls tag 0x0002 "ISO" (int16u), and
    // ExifTool prints the bare number -- there is no "ISOSpeed" tag and no
    // "ISO " prefix.
    assert!(tags.contains_key("Nikon:ISO"));
    assert_eq!(tags.get("Nikon:ISO"), Some(&"100".to_string()));

    assert!(tags.contains_key("Nikon:ShutterCount"));
    assert_eq!(tags.get("Nikon:ShutterCount"), Some(&"10000".to_string()));
}

#[test]
fn test_nikon_parse_enumerated_values() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::nikon::parse_nikon_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Nikon Type 2 header (10 bytes)
    data.extend_from_slice(b"Nikon\0\x02\x10\x00\x00");

    // Embedded TIFF header (8 bytes at offset 10)
    data.extend_from_slice(b"II"); // Little-endian byte order marker
    data.extend_from_slice(&[0x2A, 0x00]); // TIFF magic number (42)
    data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // IFD offset (8 bytes from TIFF start)

    // IFD: 3 entries
    data.extend_from_slice(&[0x03, 0x00]);

    // Nikon.pm declares 0x0004/0x0005/0x0007 as `Writable => 'string'` for
    // every model: the camera writes ASCII in all caps and the table's
    // PRINT_CONV (FormatString) fixes the case on the way out.

    // Entry 1: Quality (tag 0x0004) = "RAW"
    data.extend_from_slice(&[0x04, 0x00]); // Tag
    data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4 (fits inline)
    data.extend_from_slice(b"RAW\0");

    // Entry 2: White Balance (tag 0x0005) = "AUTO"
    data.extend_from_slice(&[0x05, 0x00]);
    data.extend_from_slice(&[0x02, 0x00]);
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]);
    data.extend_from_slice(b"AUTO");

    // Entry 3: Focus Mode (tag 0x0007) = "AF-S"
    data.extend_from_slice(&[0x07, 0x00]);
    data.extend_from_slice(&[0x02, 0x00]);
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]);
    data.extend_from_slice(b"AF-S");

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify decoded values
    assert_eq!(tags.get("Nikon:Quality"), Some(&"RAW".to_string()));
    assert_eq!(tags.get("Nikon:WhiteBalance"), Some(&"Auto".to_string()));
    assert_eq!(tags.get("Nikon:FocusMode"), Some(&"AF-S".to_string()));
}

#[test]
fn test_nikon_parse_empty_data() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::nikon::parse_nikon_makernotes;
    use std::collections::HashMap;

    let mut tags = HashMap::new();

    // Empty data should not crash
    parse_nikon_makernotes(&[], ByteOrder::LittleEndian, &mut tags);
    assert!(tags.is_empty());

    // Invalid header should not crash
    let invalid_data = b"Canon\0\x00\x00";
    parse_nikon_makernotes(invalid_data, ByteOrder::LittleEndian, &mut tags);
    // Should have no tags extracted (error case)
}

#[test]
fn test_nikon_parser_big_endian() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::nikon::parse_nikon_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Nikon Type 2 header (10 bytes)
    data.extend_from_slice(b"Nikon\0\x02\x10\x00\x00");

    // Embedded TIFF header (8 bytes at offset 10) - Big-endian
    data.extend_from_slice(b"MM"); // Big-endian byte order marker
    data.extend_from_slice(&[0x00, 0x2A]); // TIFF magic number (42) - BE
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // IFD offset (8 bytes from TIFF start) - BE

    // IFD: 1 entry (big-endian)
    data.extend_from_slice(&[0x00, 0x01]); // Entry count (BE)

    // Entry: ISO (tag 0x0002) = 200.
    // TIFF left-justifies a value that fits in the 4-byte field, so a
    // big-endian int16u of 200 is stored as 00 C8 00 00 -- not 00 00 00 C8.
    data.extend_from_slice(&[0x00, 0x02]); // Tag ID (BE)
    data.extend_from_slice(&[0x00, 0x03]); // Type: SHORT (BE)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // Count: 1 (BE)
    data.extend_from_slice(&[0x00, 0xC8, 0x00, 0x00]); // Value: 200 (BE)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD (BE)

    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::BigEndian, &mut tags);

    assert_eq!(tags.get("Nikon:ISO"), Some(&"200".to_string()));
}
