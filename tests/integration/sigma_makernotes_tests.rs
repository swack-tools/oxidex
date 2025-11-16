//! Integration tests for Sigma MakerNotes parser
//!
//! Tests the Sigma MakerNotes parsing functionality including:
//! - Lens database lookups (Art, Contemporary, Sports series)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_sigma_lens_database_art_primes() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series prime lenses
    assert_eq!(
        lookup_lens_name(1),
        Some("Sigma 14mm f/1.8 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(3),
        Some("Sigma 24mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(6),
        Some("Sigma 35mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(10),
        Some("Sigma 50mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(13),
        Some("Sigma 85mm f/1.4 DG HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_telephoto() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series telephoto primes
    assert_eq!(
        lookup_lens_name(15),
        Some("Sigma 105mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(16),
        Some("Sigma 135mm f/1.8 DG HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_macro() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series macro lenses
    assert_eq!(
        lookup_lens_name(20),
        Some("Sigma 70mm f/2.8 DG Macro Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(21),
        Some("Sigma 105mm f/2.8 DG DN Macro Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_zooms() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series zoom lenses
    assert_eq!(
        lookup_lens_name(30),
        Some("Sigma 14-24mm f/2.8 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(31),
        Some("Sigma 18-35mm f/1.8 DC HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(33),
        Some("Sigma 24-70mm f/2.8 DG OS HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(35),
        Some("Sigma 50-100mm f/1.8 DC HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_contemporary_primes() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Contemporary series primes
    assert_eq!(
        lookup_lens_name(50),
        Some("Sigma 16mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(51),
        Some("Sigma 23mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(52),
        Some("Sigma 30mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(53),
        Some("Sigma 56mm f/1.4 DC DN Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_contemporary_zooms() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Contemporary series zoom lenses
    assert_eq!(
        lookup_lens_name(54),
        Some("Sigma 17-70mm f/2.8-4.0 DC Macro OS HSM Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(57),
        Some("Sigma 100-400mm f/5.0-6.3 DG OS HSM Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(58),
        Some("Sigma 150-600mm f/5.0-6.3 DG OS HSM Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_sports_series() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Sports series lenses
    assert_eq!(
        lookup_lens_name(70),
        Some("Sigma 120-300mm f/2.8 DG OS HSM Sports".to_string())
    );

    assert_eq!(
        lookup_lens_name(71),
        Some("Sigma 150-600mm f/5.0-6.3 DG OS HSM Sports".to_string())
    );

    assert_eq!(
        lookup_lens_name(72),
        Some("Sigma 500mm f/4.0 DG OS HSM Sports".to_string())
    );
}

#[test]
fn test_sigma_lens_database_legacy_sa_mount_zooms() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test legacy SA-mount zoom lenses
    assert_eq!(
        lookup_lens_name(100),
        Some("Sigma 8-16mm f/4.5-5.6 DC HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(102),
        Some("Sigma 17-50mm f/2.8 EX DC OS HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(107),
        Some("Sigma 50-500mm f/4.5-6.3 APO DG OS HSM".to_string())
    );
}

#[test]
fn test_sigma_lens_database_legacy_sa_mount_primes() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test legacy SA-mount prime lenses
    assert_eq!(
        lookup_lens_name(120),
        Some("Sigma 8mm f/3.5 EX DG Circular Fisheye".to_string())
    );

    assert_eq!(
        lookup_lens_name(123),
        Some("Sigma 30mm f/1.4 EX DC HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(125),
        Some("Sigma 180mm f/2.8 EX DG OS HSM APO Macro".to_string())
    );
}

#[test]
fn test_sigma_lens_database_dg_dn_mirrorless() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma DG DN mirrorless lenses
    assert_eq!(
        lookup_lens_name(150),
        Some("Sigma 14-24mm f/2.8 DG DN Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(151),
        Some("Sigma 20mm f/2.0 DG DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(154),
        Some("Sigma 35mm f/2.0 DG DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(157),
        Some("Sigma 90mm f/2.8 DG DN Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_not_found() {
    use exiftool_rs::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test that unknown lens IDs return None
    assert_eq!(lookup_lens_name(9999), None);
    assert_eq!(lookup_lens_name(0), None);
    assert_eq!(lookup_lens_name(500), None);
}

#[test]
fn test_sigma_makernote_parser_trait_implementation() {
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::sigma::SigmaMakerNoteParser;

    let parser = SigmaMakerNoteParser;

    // Test manufacturer name
    assert_eq!(parser.manufacturer_name(), "Sigma");

    // Test tag prefix
    assert_eq!(parser.tag_prefix(), "Sigma:");

    // Test lens lookup via trait method
    assert_eq!(
        parser.lookup_lens(10),
        Some("Sigma 50mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(parser.lookup_lens(9999), None);
}

#[test]
fn test_sigma_header_validation_sigma() {
    use exiftool_rs::parsers::tiff::makernotes::sigma::is_sigma_makernote;

    // Test valid SIGMA header
    let valid_header = b"SIGMA\0\0\0\x00\x10\x00\x00";
    assert!(is_sigma_makernote(valid_header));

    // Test invalid header
    let invalid_header = b"CANON\0\0\0";
    assert!(!is_sigma_makernote(invalid_header));

    // Test too short data
    let too_short = b"SIG";
    assert!(!is_sigma_makernote(too_short));
}

#[test]
fn test_sigma_header_validation_foveon() {
    use exiftool_rs::parsers::tiff::makernotes::sigma::is_sigma_makernote;

    // Test valid FOVEON header (for Foveon X3 sensor cameras)
    let valid_header = b"FOVEON\0\0\x00\x10\x00\x00";
    assert!(is_sigma_makernote(valid_header));
}

#[test]
fn test_sigma_header_validation_no_header() {
    use exiftool_rs::parsers::tiff::makernotes::sigma::is_sigma_makernote;

    // Test data with no header but valid IFD entry count (10 entries)
    let no_header = b"\x0A\x00\x00\x00\x00\x00\x00\x00";
    assert!(is_sigma_makernote(no_header));

    // Test data with unreasonable entry count (should fail)
    let bad_count = b"\xFF\xFF\x00\x00\x00\x00\x00\x00";
    assert!(!is_sigma_makernote(bad_count));
}

#[test]
fn test_sigma_makernote_parse_basic() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::sigma::SigmaMakerNoteParser;
    use std::collections::HashMap;

    let parser = SigmaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic Sigma MakerNote data with header and 2 IFD entries
    // Header: "SIGMA\0\0\0" (8 bytes)
    // Entry count: 2 (little-endian u16)
    // Entry 1: Resolution Mode tag (0x0004) = 2 (High)
    // Entry 2: Quality tag (0x0016) = 3 (RAW)
    let mut data = Vec::new();
    data.extend_from_slice(b"SIGMA\0\0\0"); // Header
    data.extend_from_slice(&[0x02, 0x00]); // 2 entries (little-endian)

    // Entry 1: Resolution Mode (0x0004), type SHORT (3), count 1, value 2
    data.extend_from_slice(&[0x04, 0x00]); // tag: 0x0004
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // value: 2

    // Entry 2: Quality (0x0016), type SHORT (3), count 1, value 3
    data.extend_from_slice(&[0x16, 0x00]); // tag: 0x0016
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // value: 3

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(tags.get("Sigma:ResolutionMode"), Some(&"High".to_string()));
    assert_eq!(tags.get("Sigma:Quality"), Some(&"RAW".to_string()));
}

#[test]
fn test_sigma_makernote_parse_lens_id() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::sigma::SigmaMakerNoteParser;
    use std::collections::HashMap;

    let parser = SigmaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic data with Lens ID tag (0x001B) = 13 (85mm f/1.4 Art)
    let mut data = Vec::new();
    data.extend_from_slice(b"SIGMA\0\0\0"); // Header
    data.extend_from_slice(&[0x01, 0x00]); // 1 entry

    // Entry: Lens ID (0x001B), type SHORT (3), count 1, value 13
    data.extend_from_slice(&[0x1B, 0x00]); // tag: 0x001B
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x0D, 0x00, 0x00, 0x00]); // value: 13

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify lens ID and lens model are extracted
    assert_eq!(tags.get("Sigma:LensID"), Some(&"13".to_string()));
    assert_eq!(
        tags.get("Sigma:LensModel"),
        Some(&"Sigma 85mm f/1.4 DG HSM Art".to_string())
    );
}

#[test]
fn test_sigma_makernote_parse_camera_settings() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::sigma::SigmaMakerNoteParser;
    use std::collections::HashMap;

    let parser = SigmaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic data with multiple camera settings
    let mut data = Vec::new();
    data.extend_from_slice(b"SIGMA\0\0\0"); // Header
    data.extend_from_slice(&[0x04, 0x00]); // 4 entries

    // Entry 1: Exposure Mode (0x0008) = 2 (Aperture Priority)
    data.extend_from_slice(&[0x08, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

    // Entry 2: Metering Mode (0x0009) = 1 (Multi-segment)
    data.extend_from_slice(&[0x09, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 3: AF Mode (0x0005) = 1 (AF-S Single)
    data.extend_from_slice(&[0x05, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 4: White Balance (0x0007) = 1 (Daylight)
    data.extend_from_slice(&[0x07, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(
        tags.get("Sigma:ExposureMode"),
        Some(&"Aperture Priority".to_string())
    );
    assert_eq!(
        tags.get("Sigma:MeteringMode"),
        Some(&"Multi-segment".to_string())
    );
    assert_eq!(tags.get("Sigma:AFMode"), Some(&"AF-S (Single)".to_string()));
    assert_eq!(
        tags.get("Sigma:WhiteBalance"),
        Some(&"Daylight".to_string())
    );
}

#[test]
fn test_sigma_makernote_parse_image_processing() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::sigma::SigmaMakerNoteParser;
    use std::collections::HashMap;

    let parser = SigmaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create synthetic data with image processing parameters
    let mut data = Vec::new();
    data.extend_from_slice(b"SIGMA\0\0\0"); // Header
    data.extend_from_slice(&[0x05, 0x00]); // 5 entries

    // Entry 1: Contrast (0x000D) = 5
    data.extend_from_slice(&[0x0D, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x05, 0x00, 0x00, 0x00]);

    // Entry 2: Saturation (0x0010) = 3
    data.extend_from_slice(&[0x10, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);

    // Entry 3: Sharpness (0x0011) = 7
    data.extend_from_slice(&[0x11, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]);

    // Entry 4: Color Mode (0x001E) = 1 (Vivid)
    data.extend_from_slice(&[0x1E, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Entry 5: Color Space (0x000B) = 0 (sRGB)
    data.extend_from_slice(&[0x0B, 0x00, 0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());

    // Verify extracted tags
    assert_eq!(tags.get("Sigma:Contrast"), Some(&"5".to_string()));
    assert_eq!(tags.get("Sigma:Saturation"), Some(&"3".to_string()));
    assert_eq!(tags.get("Sigma:Sharpness"), Some(&"7".to_string()));
    assert_eq!(tags.get("Sigma:ColorMode"), Some(&"Vivid".to_string()));
    assert_eq!(tags.get("Sigma:ColorSpace"), Some(&"sRGB".to_string()));
}

#[test]
fn test_sigma_makernote_parse_error_too_short() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::sigma::SigmaMakerNoteParser;
    use std::collections::HashMap;

    let parser = SigmaMakerNoteParser;
    let mut tags = HashMap::new();

    // Test with data that's too short (less than 8 bytes)
    let data = b"SIGMA";
    let result = parser.parse(data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}

#[test]
fn test_sigma_makernote_parse_error_invalid_entry_count() {
    use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
    use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
    use exiftool_rs::parsers::tiff::makernotes::sigma::SigmaMakerNoteParser;
    use std::collections::HashMap;

    let parser = SigmaMakerNoteParser;
    let mut tags = HashMap::new();

    // Create data with invalid entry count (250, exceeding limit of 200)
    let mut data = Vec::new();
    data.extend_from_slice(b"SIGMA\0\0\0"); // Header
    data.extend_from_slice(&[0xFA, 0x00]); // 250 entries (little-endian) - invalid

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}
