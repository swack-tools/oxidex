//! Integration tests for Panasonic MakerNotes parser
//!
//! Tests the Panasonic MakerNotes parsing functionality including:
//! - Lens database lookups (M43 and L-mount)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_panasonic_lens_database_m43_standard_zoom() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic_lens_database::lookup_lens_name;

    // Test common M43 kit lens
    assert_eq!(
        lookup_lens_name(1),
        Some("Lumix G Vario 14-42mm f/3.5-5.6 ASPH. MEGA O.I.S.".to_string())
    );

    // Test professional M43 zoom
    assert_eq!(
        lookup_lens_name(8),
        Some("Lumix G X Vario 12-35mm f/2.8 ASPH. POWER O.I.S.".to_string())
    );

    // Test telephoto zoom
    assert_eq!(
        lookup_lens_name(13),
        Some("Lumix G Vario 100-300mm f/4.0-5.6 MEGA O.I.S.".to_string())
    );
}

#[test]
fn test_panasonic_lens_database_m43_primes() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic_lens_database::lookup_lens_name;

    // Test popular Lumix G primes
    assert_eq!(
        lookup_lens_name(20),
        Some("Lumix G 20mm f/1.7 ASPH.".to_string())
    );

    assert_eq!(
        lookup_lens_name(23),
        Some("Lumix G 25mm f/1.7 ASPH.".to_string())
    );

    assert_eq!(
        lookup_lens_name(25),
        Some("Lumix G 15mm f/1.7 ASPH.".to_string())
    );
}

#[test]
fn test_panasonic_lens_database_leica_dg() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic_lens_database::lookup_lens_name;

    // Test Leica DG Summilux lenses
    assert_eq!(
        lookup_lens_name(30),
        Some("Leica DG Summilux 15mm f/1.7 ASPH.".to_string())
    );

    assert_eq!(
        lookup_lens_name(31),
        Some("Leica DG Summilux 25mm f/1.4 ASPH.".to_string())
    );

    // Test Leica DG Nocticron
    assert_eq!(
        lookup_lens_name(32),
        Some("Leica DG Nocticron 42.5mm f/1.2 ASPH. POWER O.I.S.".to_string())
    );

    // Test Leica DG zoom
    assert_eq!(
        lookup_lens_name(37),
        Some("Leica DG Summilux 10-25mm f/1.7 ASPH.".to_string())
    );
}

#[test]
fn test_panasonic_lens_database_l_mount_pro() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic_lens_database::lookup_lens_name;

    // Test Lumix S Pro lenses
    assert_eq!(
        lookup_lens_name(103),
        Some("Lumix S Pro 24-70mm f/2.8".to_string())
    );

    assert_eq!(
        lookup_lens_name(104),
        Some("Lumix S Pro 70-200mm f/2.8 O.I.S.".to_string())
    );

    assert_eq!(
        lookup_lens_name(102),
        Some("Lumix S Pro 16-35mm f/4".to_string())
    );
}

#[test]
fn test_panasonic_lens_database_l_mount_primes() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic_lens_database::lookup_lens_name;

    // Test Lumix S prime lenses
    assert_eq!(
        lookup_lens_name(115),
        Some("Lumix S 50mm f/1.8".to_string())
    );

    assert_eq!(
        lookup_lens_name(116),
        Some("Lumix S 85mm f/1.8".to_string())
    );

    assert_eq!(
        lookup_lens_name(117),
        Some("Lumix S Pro 50mm f/1.4".to_string())
    );
}

#[test]
fn test_panasonic_lens_database_olympus_compatibility() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic_lens_database::lookup_lens_name;

    // Test Olympus M.Zuiko lenses (compatible with Panasonic M43)
    assert_eq!(
        lookup_lens_name(200),
        Some("Olympus M.Zuiko Digital ED 12-40mm f/2.8 PRO".to_string())
    );

    assert_eq!(
        lookup_lens_name(201),
        Some("Olympus M.Zuiko Digital ED 40-150mm f/2.8 PRO".to_string())
    );
}

#[test]
fn test_panasonic_lens_database_unknown() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic_lens_database::lookup_lens_name;

    // Unknown lens ID should return None
    assert_eq!(lookup_lens_name(65000), None);
    assert_eq!(lookup_lens_name(0), None);
}

#[test]
fn test_panasonic_parser_trait() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic::PanasonicParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;

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
fn test_panasonic_parser_lens_lookup() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic::PanasonicParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = PanasonicParser;

    // Test lens lookup through trait (M43)
    assert_eq!(
        parser.lookup_lens(32),
        Some("Leica DG Nocticron 42.5mm f/1.2 ASPH. POWER O.I.S.".to_string())
    );

    // Test lens lookup through trait (L-mount)
    assert_eq!(
        parser.lookup_lens(103),
        Some("Lumix S Pro 24-70mm f/2.8".to_string())
    );

    assert_eq!(parser.lookup_lens(65000), None);
}

#[test]
fn test_panasonic_is_panasonic_makernote() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic::is_panasonic_makernote;

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
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    // Create minimal Panasonic MakerNote
    let mut data = Vec::new();

    // Panasonic header (12 bytes)
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: entry count (little-endian)
    data.extend_from_slice(&[0x02, 0x00]); // 2 entries

    // Entry 1: Quality Mode (tag 0x0003) = Fine (value 3)
    data.extend_from_slice(&[0x03, 0x00]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // Value: 3 (Fine)

    // Entry 2: Macro Mode (tag 0x001C) = On (value 1)
    data.extend_from_slice(&[0x1C, 0x00]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

    // Next IFD offset
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify extracted tags
    assert!(tags.contains_key("Panasonic:QualityMode"));
    assert_eq!(tags.get("Panasonic:QualityMode"), Some(&"Fine".to_string()));

    assert!(tags.contains_key("Panasonic:MacroMode"));
    assert_eq!(tags.get("Panasonic:MacroMode"), Some(&"On".to_string()));
}

#[test]
fn test_panasonic_parse_enumerated_values() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 4 entries
    data.extend_from_slice(&[0x04, 0x00]);

    // Entry 1: White Balance (tag 0x0007) = Daylight (value 2)
    data.extend_from_slice(&[0x07, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Daylight)

    // Entry 2: Focus Mode (tag 0x000F) = AF-S (value 4)
    data.extend_from_slice(&[0x0F, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // AF-S

    // Entry 3: Shooting Mode (tag 0x001F) = Aperture Priority (value 7)
    data.extend_from_slice(&[0x1F, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]);

    // Entry 4: Film Mode (tag 0x0042) = Cinelike D (value 22)
    data.extend_from_slice(&[0x42, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x16, 0x00, 0x00, 0x00]); // 22

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
        Some(&"Cinelike D".to_string())
    );
}

#[test]
fn test_panasonic_parse_lens_type() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 1 entry
    data.extend_from_slice(&[0x01, 0x00]);

    // Entry: Lens Type (tag 0x0051) = Leica DG Nocticron 42.5mm f/1.2 (ID 32)
    data.extend_from_slice(&[0x51, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x20, 0x00, 0x00, 0x00]); // Value: 32

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify lens lookup worked
    assert!(tags.contains_key("Panasonic:LensType"));
    assert_eq!(
        tags.get("Panasonic:LensType"),
        Some(&"Leica DG Nocticron 42.5mm f/1.2 ASPH. POWER O.I.S.".to_string())
    );
}

#[test]
fn test_panasonic_parse_photo_style() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 2 entries
    data.extend_from_slice(&[0x02, 0x00]);

    // Entry 1: Photo Style (tag 0x0061) = V-Log (value 10)
    data.extend_from_slice(&[0x61, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x0A, 0x00, 0x00, 0x00]); // 10 (V-Log)

    // Entry 2: HDR (tag 0x0079) = HDR Auto (value 100)
    data.extend_from_slice(&[0x79, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x64, 0x00, 0x00, 0x00]); // 100

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(tags.get("Panasonic:PhotoStyle"), Some(&"V-Log".to_string()));
    assert_eq!(tags.get("Panasonic:HDR"), Some(&"HDR Auto".to_string()));
}

#[test]
fn test_panasonic_parse_empty_data() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
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
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 1 entry (big-endian)
    data.extend_from_slice(&[0x00, 0x01]); // Entry count (BE)

    // Entry: Quality Mode (tag 0x0003) = RAW (value 6)
    data.extend_from_slice(&[0x00, 0x03]); // Tag ID (BE)
    data.extend_from_slice(&[0x00, 0x03]); // Type: SHORT (BE)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // Count: 1 (BE)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x06]); // Value: 6 (BE)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD (BE)

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::BigEndian, &mut tags);

    assert_eq!(tags.get("Panasonic:QualityMode"), Some(&"RAW".to_string()));
}

#[test]
fn test_panasonic_lens_database_coverage() {
    use exiftool_rs::parsers::tiff::makernotes::panasonic_lens_database::lookup_lens_name;

    // Count how many lenses we have in database
    let mut count = 0;
    for id in 1..=250 {
        if lookup_lens_name(id).is_some() {
            count += 1;
        }
    }

    // Should have significant coverage (M43 + L-mount + Leica DG)
    assert!(
        count >= 50,
        "Expected at least 50 lenses in database, found {}",
        count
    );
}

#[test]
fn test_panasonic_intelligent_features() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Panasonic header
    data.extend_from_slice(b"Panasonic\0\0\0");

    // IFD: 3 entries
    data.extend_from_slice(&[0x03, 0x00]);

    // Entry 1: Intelligent Auto (tag 0x0080) = On (value 1)
    data.extend_from_slice(&[0x80, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 2: Intelligent Exposure (tag 0x0059) = Standard (value 2)
    data.extend_from_slice(&[0x59, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

    // Entry 3: Intelligent Resolution (tag 0x005D) = High (value 3)
    data.extend_from_slice(&[0x5D, 0x00]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(
        tags.get("Panasonic:IntelligentAuto"),
        Some(&"On".to_string())
    );
    assert_eq!(
        tags.get("Panasonic:IntelligentExposure"),
        Some(&"Standard".to_string())
    );
    assert_eq!(
        tags.get("Panasonic:IntelligentResolution"),
        Some(&"High".to_string())
    );
}
