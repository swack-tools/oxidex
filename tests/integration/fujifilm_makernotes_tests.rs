//! Integration tests for Fujifilm MakerNotes parser
//!
//! Tests the Fujifilm MakerNotes parsing functionality including:
//! - Lens database lookups (XF, XC, GF lenses)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data
//! - Film simulation modes and dynamic range settings

#[test]
fn test_fujifilm_lens_database_xf_primes() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm_lens_database::lookup_lens_name;

    // Test popular XF prime lenses
    assert_eq!(lookup_lens_name(35), Some("XF 35mm f/1.4 R".to_string()));

    assert_eq!(lookup_lens_name(148), Some("XF 56mm f/1.2 R".to_string()));

    assert_eq!(
        lookup_lens_name(270),
        Some("XF 90mm f/2 R LM WR".to_string())
    );

    // Test wide angle prime
    assert_eq!(lookup_lens_name(23), Some("XF 14mm f/2.8 R".to_string()));
}

#[test]
fn test_fujifilm_lens_database_xf_zooms() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm_lens_database::lookup_lens_name;

    // Test kit lens
    assert_eq!(
        lookup_lens_name(1),
        Some("XF 18-55mm f/2.8-4 R LM OIS".to_string())
    );

    // Test professional zoom
    assert_eq!(
        lookup_lens_name(20),
        Some("XF 50-140mm f/2.8 R LM OIS WR".to_string())
    );

    // Test telephoto zoom
    assert_eq!(
        lookup_lens_name(272),
        Some("XF 100-400mm f/4.5-5.6 R LM OIS WR".to_string())
    );

    // Test ultra-wide zoom
    assert_eq!(
        lookup_lens_name(274),
        Some("XF 8-16mm f/2.8 R LM WR".to_string())
    );
}

#[test]
fn test_fujifilm_lens_database_xc_budget() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm_lens_database::lookup_lens_name;

    // Test budget XC lenses
    assert_eq!(
        lookup_lens_name(11),
        Some("XC 16-50mm f/3.5-5.6 OIS".to_string())
    );

    assert_eq!(
        lookup_lens_name(277),
        Some("XC 15-45mm f/3.5-5.6 OIS PZ".to_string())
    );

    assert_eq!(lookup_lens_name(278), Some("XC 35mm f/2".to_string()));
}

#[test]
fn test_fujifilm_lens_database_gfx_medium_format() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm_lens_database::lookup_lens_name;

    // Test GFX medium format lenses
    assert_eq!(lookup_lens_name(63), Some("GF 63mm f/2.8 R WR".to_string()));

    assert_eq!(
        lookup_lens_name(110),
        Some("GF 110mm f/2 R LM WR".to_string())
    );

    assert_eq!(
        lookup_lens_name(100),
        Some("GF 100-200mm f/5.6 R LM OIS WR".to_string())
    );

    // Test ultra-fast GF prime
    assert_eq!(
        lookup_lens_name(293),
        Some("GF 80mm f/1.7 R WR".to_string())
    );
}

#[test]
fn test_fujifilm_lens_database_teleconverters() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm_lens_database::lookup_lens_name;

    // Test teleconverters
    assert_eq!(lookup_lens_name(286), Some("GF 1.4X TC WR".to_string()));

    assert_eq!(lookup_lens_name(288), Some("XF 1.4X TC WR".to_string()));

    assert_eq!(lookup_lens_name(289), Some("XF 2X TC WR".to_string()));
}

#[test]
fn test_fujifilm_lens_database_unknown() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm_lens_database::lookup_lens_name;

    // Unknown lens IDs should return None
    assert_eq!(lookup_lens_name(65000), None);
    assert_eq!(lookup_lens_name(0), None);
    assert_eq!(lookup_lens_name(9999), None);
}

#[test]
fn test_fujifilm_parser_trait() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::FujifilmParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = FujifilmParser;

    // Test trait methods
    assert_eq!(parser.manufacturer_name(), "Fujifilm");
    assert_eq!(parser.tag_prefix(), "Fujifilm:");

    // Test header validation
    let valid_header = b"FUJIFILM\x0C\x00\x00\x00extra data";
    assert!(parser.validate_header(valid_header));

    let invalid_header = b"Canon\0\x00\x00";
    assert!(!parser.validate_header(invalid_header));

    // Too short
    let too_short = b"FUJIFILM\x0C";
    assert!(!parser.validate_header(too_short));
}

#[test]
fn test_fujifilm_parser_lens_lookup() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::FujifilmParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = FujifilmParser;

    // Test lens lookup through trait
    assert_eq!(parser.lookup_lens(35), Some("XF 35mm f/1.4 R".to_string()));

    assert_eq!(
        parser.lookup_lens(63),
        Some("GF 63mm f/2.8 R WR".to_string())
    );

    assert_eq!(parser.lookup_lens(65000), None);
}

#[test]
fn test_fujifilm_is_fujifilm_makernote() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::is_fujifilm_makernote;

    // Valid Fujifilm header
    assert!(is_fujifilm_makernote(b"FUJIFILM\x0C\x00\x00\x00test data"));

    // Valid with exact minimum length
    assert!(is_fujifilm_makernote(b"FUJIFILM\x0C\x00\x00\x00"));

    // Invalid - Canon header
    assert!(!is_fujifilm_makernote(b"Canon\0"));

    // Invalid - too short
    assert!(!is_fujifilm_makernote(b"FUJIFILM"));

    // Invalid - wrong signature
    assert!(!is_fujifilm_makernote(b"Nikon\0\0\0\0\0\0\0"));
}

#[test]
fn test_fujifilm_parse_basic_tags() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    // Create minimal Fujifilm MakerNote
    let mut data = Vec::new();

    // Fujifilm header: "FUJIFILM" + IFD offset (0x0000000C = 12)
    data.extend_from_slice(b"FUJIFILM");
    data.extend_from_slice(&[0x0C, 0x00, 0x00, 0x00]); // Offset to IFD (little-endian)

    // IFD: entry count (little-endian)
    data.extend_from_slice(&[0x02, 0x00]); // 2 entries

    // Entry 1: Quality (tag 0x1000) = Fine (value 3)
    data.extend_from_slice(&[0x00, 0x10]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // Value: 3 (Fine)

    // Entry 2: Sequence Number (tag 0x1103) = 42
    data.extend_from_slice(&[0x03, 0x11]); // Tag ID
    data.extend_from_slice(&[0x04, 0x00]); // Type: LONG
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x2A, 0x00, 0x00, 0x00]); // Value: 42

    // Next IFD offset
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify extracted tags
    assert!(tags.contains_key("Fujifilm:Quality"));
    assert_eq!(tags.get("Fujifilm:Quality"), Some(&"Fine".to_string()));

    assert!(tags.contains_key("Fujifilm:SequenceNumber"));
    assert_eq!(tags.get("Fujifilm:SequenceNumber"), Some(&"42".to_string()));
}

#[test]
fn test_fujifilm_parse_film_simulation() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Fujifilm header
    data.extend_from_slice(b"FUJIFILM");
    data.extend_from_slice(&[0x0C, 0x00, 0x00, 0x00]);

    // IFD: 3 entries for different film simulations
    data.extend_from_slice(&[0x03, 0x00]);

    // Entry 1: Film Mode (tag 0x1401) = Classic Chrome (0x0600)
    data.extend_from_slice(&[0x01, 0x14]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x00, 0x06, 0x00, 0x00]); // Value: 0x0600

    // Entry 2: Dynamic Range (tag 0x1402) = Wide 2 (400%) (value 3)
    data.extend_from_slice(&[0x02, 0x14]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);

    // Entry 3: Shutter Type (tag 0x1100) = Electronic (value 1)
    data.extend_from_slice(&[0x00, 0x11]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify decoded film simulation values
    assert_eq!(
        tags.get("Fujifilm:FilmMode"),
        Some(&"Classic Chrome".to_string())
    );
    assert_eq!(
        tags.get("Fujifilm:DynamicRange"),
        Some(&"Wide 2 (400%)".to_string())
    );
    assert_eq!(
        tags.get("Fujifilm:ShutterType"),
        Some(&"Electronic".to_string())
    );
}

#[test]
fn test_fujifilm_parse_focus_and_flash() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Fujifilm header
    data.extend_from_slice(b"FUJIFILM");
    data.extend_from_slice(&[0x0C, 0x00, 0x00, 0x00]);

    // IFD: 3 entries
    data.extend_from_slice(&[0x03, 0x00]);

    // Entry 1: Focus Mode (tag 0x1021) = AF-C Continuous (value 3)
    data.extend_from_slice(&[0x21, 0x10]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);

    // Entry 2: Flash Mode (tag 0x1010) = On (value 1)
    data.extend_from_slice(&[0x10, 0x10]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 3: White Balance (tag 0x1002) = Daylight (0x0100)
    data.extend_from_slice(&[0x02, 0x10]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(
        tags.get("Fujifilm:FocusMode"),
        Some(&"AF-C (Continuous)".to_string())
    );
    assert_eq!(tags.get("Fujifilm:FlashMode"), Some(&"On".to_string()));
    assert_eq!(
        tags.get("Fujifilm:WhiteBalance"),
        Some(&"Daylight".to_string())
    );
}

#[test]
fn test_fujifilm_parse_empty_data() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    let mut tags = HashMap::new();

    // Empty data should not crash
    parse_fujifilm_makernotes(&[], ByteOrder::LittleEndian, &mut tags);
    assert!(tags.is_empty());

    // Invalid header should not crash
    let invalid_data = b"Nikon\0\x00\x00";
    parse_fujifilm_makernotes(invalid_data, ByteOrder::LittleEndian, &mut tags);
    // Should have no tags extracted (error case)
}

#[test]
fn test_fujifilm_parser_big_endian() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Fujifilm header (big-endian offset)
    data.extend_from_slice(b"FUJIFILM");
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x0C]); // Offset to IFD (BE)

    // IFD: 1 entry (big-endian)
    data.extend_from_slice(&[0x00, 0x01]); // Entry count (BE)

    // Entry: Quality (tag 0x1000) = Fine+RAW (value 5)
    data.extend_from_slice(&[0x10, 0x00]); // Tag ID (BE)
    data.extend_from_slice(&[0x00, 0x03]); // Type: SHORT (BE)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // Count: 1 (BE)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x05]); // Value: 5 (BE)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD (BE)

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::BigEndian, &mut tags);

    assert_eq!(tags.get("Fujifilm:Quality"), Some(&"Fine+RAW".to_string()));
}

#[test]
fn test_fujifilm_lens_database_coverage() {
    use exiftool_rs::parsers::tiff::makernotes::fujifilm_lens_database::lookup_lens_name;

    // Count how many lenses we have in database
    let mut count = 0;
    // Scan broader range to catch all lens IDs (some like 4095 are outside normal range)
    for id in 1..=5000 {
        if lookup_lens_name(id).is_some() {
            count += 1;
        }
    }

    // Should have at least 60 lenses as specified
    assert!(
        count >= 60,
        "Expected at least 60 lenses in database, found {}",
        count
    );
}

#[test]
fn test_fujifilm_parse_advanced_settings() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Fujifilm header
    data.extend_from_slice(b"FUJIFILM");
    data.extend_from_slice(&[0x0C, 0x00, 0x00, 0x00]);

    // IFD: 4 entries for advanced features
    data.extend_from_slice(&[0x04, 0x00]);

    // Entry 1: Shadow Tone (tag 0x1040) = +16
    data.extend_from_slice(&[0x40, 0x10]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // +16

    // Entry 2: Highlight Tone (tag 0x1041) = -16
    data.extend_from_slice(&[0x41, 0x10]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    // For negative values, we need to use signed representation
    data.extend_from_slice(&[0xF0, 0xFF, 0xFF, 0xFF]); // -16 in two's complement

    // Entry 3: Faces Detected (tag 0x4100) = 3
    data.extend_from_slice(&[0x00, 0x41]);
    data.extend_from_slice(&[0x04, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);

    // Entry 4: Burst Mode (tag 0x1101) = Continuous High (value 2)
    data.extend_from_slice(&[0x01, 0x11]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify advanced settings
    assert!(tags.contains_key("Fujifilm:ShadowTone"));
    assert!(tags.contains_key("Fujifilm:HighlightTone"));
    assert_eq!(tags.get("Fujifilm:FacesDetected"), Some(&"3".to_string()));
    assert_eq!(
        tags.get("Fujifilm:BurstMode"),
        Some(&"On (High Speed)".to_string())
    );
}
