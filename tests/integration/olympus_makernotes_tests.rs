//! Integration tests for Olympus MakerNotes parser
//!
//! Tests the Olympus MakerNotes parsing functionality including:
//! - Lens database lookups (Four Thirds and Micro Four Thirds)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_olympus_lens_database_four_thirds() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test classic Four Thirds Zuiko Digital lenses
    assert_eq!(
        lookup_lens_name(1),
        Some("Olympus Zuiko Digital ED 50mm f/2.0 Macro".to_string())
    );

    assert_eq!(
        lookup_lens_name(4),
        Some("Olympus Zuiko Digital 14-54mm f/2.8-3.5".to_string())
    );

    assert_eq!(
        lookup_lens_name(9),
        Some("Olympus Zuiko Digital ED 12-60mm f/2.8-4.0 SWD".to_string())
    );

    // Test Four Thirds pro lens
    assert_eq!(
        lookup_lens_name(10),
        Some("Olympus Zuiko Digital ED 14-35mm f/2.0 SWD".to_string())
    );
}

#[test]
fn test_olympus_lens_database_mzuiko_standard() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test popular M.Zuiko kit lenses
    assert_eq!(
        lookup_lens_name(36),
        Some("M.Zuiko Digital ED 14-42mm f/3.5-5.6".to_string())
    );

    assert_eq!(
        lookup_lens_name(40),
        Some("M.Zuiko Digital ED 12-50mm f/3.5-6.3 EZ".to_string())
    );

    assert_eq!(
        lookup_lens_name(37),
        Some("M.Zuiko Digital ED 40-150mm f/4.0-5.6".to_string())
    );
}

#[test]
fn test_olympus_lens_database_mzuiko_primes() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test M.Zuiko prime lenses
    assert_eq!(
        lookup_lens_name(41),
        Some("M.Zuiko Digital 45mm f/1.8".to_string())
    );

    assert_eq!(
        lookup_lens_name(42),
        Some("M.Zuiko Digital ED 60mm f/2.8 Macro".to_string())
    );

    assert_eq!(
        lookup_lens_name(45),
        Some("M.Zuiko Digital ED 75mm f/1.8".to_string())
    );

    assert_eq!(
        lookup_lens_name(46),
        Some("M.Zuiko Digital 17mm f/1.8".to_string())
    );

    assert_eq!(
        lookup_lens_name(47),
        Some("M.Zuiko Digital 25mm f/1.8".to_string())
    );
}

#[test]
fn test_olympus_lens_database_pro_series() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test M.Zuiko PRO lenses
    assert_eq!(
        lookup_lens_name(48),
        Some("M.Zuiko Digital ED 12-40mm f/2.8 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(49),
        Some("M.Zuiko Digital ED 40-150mm f/2.8 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(51),
        Some("M.Zuiko Digital ED 7-14mm f/2.8 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(52),
        Some("M.Zuiko Digital ED 300mm f/4.0 IS PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(64),
        Some("M.Zuiko Digital ED 12-100mm f/4.0 IS PRO".to_string())
    );
}

#[test]
fn test_olympus_lens_database_premium_primes() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test M.Zuiko f/1.2 PRO primes
    assert_eq!(
        lookup_lens_name(65),
        Some("M.Zuiko Digital ED 25mm f/1.2 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(66),
        Some("M.Zuiko Digital ED 17mm f/1.2 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(67),
        Some("M.Zuiko Digital ED 45mm f/1.2 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(72),
        Some("M.Zuiko Digital ED 20mm f/1.4 PRO".to_string())
    );
}

#[test]
fn test_olympus_lens_database_specialty() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test specialty lenses
    assert_eq!(
        lookup_lens_name(53),
        Some("M.Zuiko Digital ED 8mm f/1.8 Fisheye PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(80),
        Some("M.Zuiko Digital ED 30mm f/3.5 Macro".to_string())
    );

    assert_eq!(
        lookup_lens_name(81),
        Some("M.Zuiko Digital 9mm f/8.0 Fisheye Body Cap Lens".to_string())
    );

    assert_eq!(
        lookup_lens_name(70),
        Some("M.Zuiko Digital ED 150-400mm f/4.5 TC1.25x IS PRO".to_string())
    );
}

#[test]
fn test_olympus_lens_database_hex_encoded() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test hex-encoded lens IDs (newer cameras)
    assert_eq!(
        lookup_lens_name(0x0206),
        Some("M.Zuiko Digital ED 12-40mm f/2.8 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(0x0209),
        Some("M.Zuiko Digital ED 7-14mm f/2.8 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(0x0210),
        Some("M.Zuiko Digital ED 12-100mm f/4.0 IS PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(0x0212),
        Some("M.Zuiko Digital ED 25mm f/1.2 PRO".to_string())
    );
}

#[test]
fn test_olympus_lens_database_unknown() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test unknown lens ID
    assert_eq!(lookup_lens_name(9999), None);
}

#[test]
fn test_olympus_lens_database_size() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test that we have a substantial database
    let mut count = 0;
    for i in 0..1000 {
        if lookup_lens_name(i).is_some() {
            count += 1;
        }
    }

    // Should have at least 70 lenses in decimal range
    assert!(
        count >= 70,
        "Expected at least 70 lens entries, found {}",
        count
    );
}

#[test]
fn test_olympus_parse_hex_lens_id() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::parse_hex_lens_id;

    // Test hex string parsing
    assert_eq!(parse_hex_lens_id("0x0210"), Some(0x0210));
    assert_eq!(parse_hex_lens_id("0X0210"), Some(0x0210));
    assert_eq!(parse_hex_lens_id("48"), Some(48));
    assert_eq!(parse_hex_lens_id("invalid"), None);
}

#[test]
fn test_olympus_parser_trait_implementation() {
    use exiftool_rs::parsers::tiff::makernotes::olympus::OlympusParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = OlympusParser;

    // Test trait methods
    assert_eq!(parser.manufacturer_name(), "Olympus");
    assert_eq!(parser.tag_prefix(), "Olympus:");

    // Test lens lookup through trait
    assert_eq!(
        parser.lookup_lens(48),
        Some("M.Zuiko Digital ED 12-40mm f/2.8 PRO".to_string())
    );

    assert_eq!(parser.lookup_lens(9999), None);
}

#[test]
fn test_olympus_header_validation() {
    use exiftool_rs::parsers::tiff::makernotes::olympus::OlympusParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;

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
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::olympus::OlympusParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
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
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::olympus::OlympusParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = OlympusParser;
    let mut tags = HashMap::new();

    // Invalid header should return error
    let data = b"NIKON\0\0\0invalid header";
    let result = parser.parse(data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}

#[test]
fn test_olympus_comprehensive_coverage() {
    use exiftool_rs::parsers::tiff::makernotes::olympus_lens_database::lookup_lens_name;

    // Test comprehensive coverage of major lens categories
    let test_categories = vec![
        (1, "Four Thirds - Macro"),
        (4, "Four Thirds - Standard Zoom"),
        (10, "Four Thirds - Pro Wide Zoom"),
        (36, "M.Zuiko - Kit Zoom"),
        (41, "M.Zuiko - Standard Prime"),
        (48, "M.Zuiko - Pro Standard Zoom"),
        (51, "M.Zuiko - Pro Wide Zoom"),
        (52, "M.Zuiko - Pro Telephoto Prime"),
        (53, "M.Zuiko - Fisheye"),
        (64, "M.Zuiko - Pro All-in-One"),
        (65, "M.Zuiko - Premium f/1.2 Prime"),
        (80, "M.Zuiko - Macro"),
        (0x0210, "M.Zuiko - Hex ID Pro Zoom"),
    ];

    for (lens_id, category) in test_categories {
        let result = lookup_lens_name(lens_id);
        assert!(
            result.is_some(),
            "Lens ID {} ({}) should be in database",
            lens_id,
            category
        );
    }
}
