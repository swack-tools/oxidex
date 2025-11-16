//! Integration tests for Leica MakerNotes parser
//!
//! Tests the Leica MakerNotes parsing functionality including:
//! - Lens database lookups (M-mount, SL-mount, L-mount lenses)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_leica_lens_database_m_mount_summilux() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test Leica M-mount Summilux lenses (f/1.4 premium series)
    assert_eq!(
        lookup_lens_name(1),
        Some("Leica Summilux-M 21mm f/1.4 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(3),
        Some("Leica Summilux-M 28mm f/1.4 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(4),
        Some("Leica Summilux-M 35mm f/1.4 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(5),
        Some("Leica Summilux-M 50mm f/1.4 ASPH".to_string())
    );
}

#[test]
fn test_leica_lens_database_m_mount_noctilux() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test legendary Noctilux ultra-fast lenses
    assert_eq!(
        lookup_lens_name(10),
        Some("Leica Noctilux-M 50mm f/0.95 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(11),
        Some("Leica Noctilux-M 50mm f/1.2 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(12),
        Some("Leica Noctilux-M 75mm f/1.25 ASPH".to_string())
    );
}

#[test]
fn test_leica_lens_database_apo_summicron_m() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test APO-Summicron-M lenses (apochromatic f/2.0)
    assert_eq!(
        lookup_lens_name(20),
        Some("Leica APO-Summicron-M 35mm f/2.0 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(21),
        Some("Leica APO-Summicron-M 50mm f/2.0 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(22),
        Some("Leica APO-Summicron-M 75mm f/2.0 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(23),
        Some("Leica APO-Summicron-M 90mm f/2.0 ASPH".to_string())
    );
}

#[test]
fn test_leica_lens_database_summicron_m() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test standard Summicron-M lenses (f/2.0)
    assert_eq!(
        lookup_lens_name(30),
        Some("Leica Summicron-M 21mm f/2.0 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(32),
        Some("Leica Summicron-M 35mm f/2.0 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(33),
        Some("Leica Summicron-M 50mm f/2.0".to_string())
    );
}

#[test]
fn test_leica_lens_database_elmarit_m() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test Elmarit-M compact lenses (f/2.8)
    assert_eq!(
        lookup_lens_name(40),
        Some("Leica Elmarit-M 21mm f/2.8 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(41),
        Some("Leica Elmarit-M 24mm f/2.8 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(43),
        Some("Leica Elmarit-M 90mm f/2.8".to_string())
    );
}

#[test]
fn test_leica_lens_database_sl_mount_apo() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test SL-mount APO-Summicron lenses (autofocus)
    assert_eq!(
        lookup_lens_name(100),
        Some("Leica APO-Summicron-SL 35mm f/2.0 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(101),
        Some("Leica APO-Summicron-SL 50mm f/2.0 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(102),
        Some("Leica APO-Summicron-SL 75mm f/2.0 ASPH".to_string())
    );
}

#[test]
fn test_leica_lens_database_sl_zoom_lenses() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test SL-mount zoom lenses
    assert_eq!(
        lookup_lens_name(120),
        Some("Leica Vario-Elmarit-SL 24-70mm f/2.8 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(130),
        Some("Leica APO-Vario-Elmarit-SL 90-280mm f/2.8-4.0".to_string())
    );
}

#[test]
fn test_leica_lens_database_tl_mount() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test TL/CL mount lenses (APS-C)
    assert_eq!(
        lookup_lens_name(200),
        Some("Leica APO-Summicron-TL 23mm f/2.0 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(220),
        Some("Leica Elmarit-TL 18mm f/2.8 ASPH".to_string())
    );

    assert_eq!(
        lookup_lens_name(230),
        Some("Leica Vario-Elmar-TL 18-56mm f/3.5-5.6 ASPH".to_string())
    );
}

#[test]
fn test_leica_lens_database_r_mount_legacy() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test legacy R-mount SLR lenses
    assert_eq!(
        lookup_lens_name(300),
        Some("Leica Summilux-R 50mm f/1.4".to_string())
    );

    assert_eq!(
        lookup_lens_name(302),
        Some("Leica Elmarit-R 28mm f/2.8".to_string())
    );

    assert_eq!(
        lookup_lens_name(304),
        Some("Leica APO-Telyt-R 180mm f/3.4".to_string())
    );
}

#[test]
fn test_leica_lens_database_l_mount_alliance() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test L-mount alliance lenses (Sigma, Panasonic compatible)
    assert_eq!(
        lookup_lens_name(400),
        Some("Sigma 35mm f/1.2 DG DN Art (L-mount)".to_string())
    );

    assert_eq!(
        lookup_lens_name(401),
        Some("Sigma 50mm f/1.4 DG DN Art (L-mount)".to_string())
    );

    assert_eq!(
        lookup_lens_name(404),
        Some("Panasonic Lumix S Pro 50mm f/1.4 (L-mount)".to_string())
    );
}

#[test]
fn test_leica_lens_database_not_found() {
    use exiftool_rs::parsers::tiff::makernotes::leica_lens_database::lookup_lens_name;

    // Test that unknown lens IDs return None
    assert_eq!(lookup_lens_name(9999), None);
    assert_eq!(lookup_lens_name(0), None);
    assert_eq!(lookup_lens_name(500), None);
}

#[test]
fn test_leica_makernote_parser_trait_implementation() {
    use exiftool_rs::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = LeicaMakerNoteParser;

    // Test manufacturer name
    assert_eq!(parser.manufacturer_name(), "Leica");

    // Test tag prefix
    assert_eq!(parser.tag_prefix(), "Leica:");

    // Test lens lookup via trait method
    assert_eq!(
        parser.lookup_lens(5),
        Some("Leica Summilux-M 50mm f/1.4 ASPH".to_string())
    );

    assert_eq!(parser.lookup_lens(9999), None);
}

#[test]
fn test_leica_header_validation_short() {
    use exiftool_rs::parsers::tiff::makernotes::leica::is_leica_makernote;

    // Test valid short LEICA header
    let valid_header = b"LEICA\0\0\0\x00\x10\x00\x00";
    assert!(is_leica_makernote(valid_header));

    // Test invalid header
    let invalid_header = b"NIKON\0\0\0";
    assert!(!is_leica_makernote(invalid_header));

    // Test too short data
    let too_short = b"LEI";
    assert!(!is_leica_makernote(too_short));
}

#[test]
fn test_leica_header_validation_long() {
    use exiftool_rs::parsers::tiff::makernotes::leica::is_leica_makernote;

    // Test valid long "LEICA CAMERA AG" header
    let valid_header = b"LEICA CAMERA AG\x00\x00\x10";
    assert!(is_leica_makernote(valid_header));
}

#[test]
fn test_leica_header_validation_no_header() {
    use exiftool_rs::parsers::tiff::makernotes::leica::is_leica_makernote;

    // Test data with no header but valid IFD entry count (15 entries)
    let no_header = b"\x0F\x00\x00\x00\x00\x00\x00\x00";
    assert!(is_leica_makernote(no_header));

    // Test data with unreasonable entry count (should fail)
    let bad_count = b"\xFF\xFF\x00\x00\x00\x00\x00\x00";
    assert!(!is_leica_makernote(bad_count));
}

#[test]
fn test_leica_makernote_parse_basic() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic Leica MakerNote data with header and 2 IFD entries
    // Header: "LEICA\0\0\0" (8 bytes)
    // Entry count: 2 (little-endian u16)
    // Entry 1: Quality tag (0x0003) = 1 (Fine)
    // Entry 2: User Profile tag (0x0004) = 5 (Standard)
    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA\0\0\0"); // Header
    data.extend_from_slice(&[0x02, 0x00]); // 2 entries (little-endian)

    // Entry 1: Quality (0x0003), type SHORT (3), count 1, value 1
    data.extend_from_slice(&[0x03, 0x00]); // tag: 0x0003
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // value: 1

    // Entry 2: User Profile (0x0004), type SHORT (3), count 1, value 5
    data.extend_from_slice(&[0x04, 0x00]); // tag: 0x0004
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x05, 0x00, 0x00, 0x00]); // value: 5

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(tags.get("Leica:Quality"), Some(&"Fine".to_string()));
    assert_eq!(tags.get("Leica:UserProfile"), Some(&"Standard".to_string()));
}

#[test]
fn test_leica_makernote_parse_lens_id() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic data with Lens ID tag (0x0013) = 10 (Noctilux 50mm f/0.95)
    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA\0\0\0"); // Header
    data.extend_from_slice(&[0x01, 0x00]); // 1 entry

    // Entry: Lens ID (0x0013), type SHORT (3), count 1, value 10
    data.extend_from_slice(&[0x13, 0x00]); // tag: 0x0013
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x0A, 0x00, 0x00, 0x00]); // value: 10

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify lens ID and lens model are extracted
    assert_eq!(tags.get("Leica:LensID"), Some(&"10".to_string()));
    assert_eq!(
        tags.get("Leica:LensModel"),
        Some(&"Leica Noctilux-M 50mm f/0.95 ASPH".to_string())
    );
}

#[test]
fn test_leica_makernote_parse_camera_settings() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic data with multiple camera settings
    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA\0\0\0"); // Header
    data.extend_from_slice(&[0x04, 0x00]); // 4 entries

    // Entry 1: Exposure Mode (0x0020) = 2 (Aperture Priority)
    data.extend_from_slice(&[0x20, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

    // Entry 2: Metering Mode (0x0021) = 1 (Multi-segment)
    data.extend_from_slice(&[0x21, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 3: AF Mode (0x0052) = 1 (Single AF)
    data.extend_from_slice(&[0x52, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 4: Image Stabilization (0x0053) = 2 (On - Body)
    data.extend_from_slice(&[0x53, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(
        tags.get("Leica:ExposureMode"),
        Some(&"Aperture Priority".to_string())
    );
    assert_eq!(
        tags.get("Leica:MeteringMode"),
        Some(&"Multi-segment".to_string())
    );
    assert_eq!(tags.get("Leica:AFMode"), Some(&"Single AF".to_string()));
    assert_eq!(
        tags.get("Leica:ImageStabilization"),
        Some(&"On (Body)".to_string())
    );
}

#[test]
fn test_leica_makernote_parse_error_too_short() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    // Test with data that's too short (less than 8 bytes)
    let data = b"LEICA";
    let result = parser.parse(data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}

#[test]
fn test_leica_makernote_parse_error_invalid_entry_count() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create data with invalid entry count (300, exceeding limit of 200)
    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA\0\0\0"); // Header
    data.extend_from_slice(&[0x2C, 0x01]); // 300 entries (little-endian) - invalid

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}
