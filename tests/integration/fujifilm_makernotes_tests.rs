//! Integration tests for Fujifilm MakerNotes parser
//!
//! Tests the Fujifilm MakerNotes parsing functionality including:
//! - Lens database lookups (XF, XC, GF lenses)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data
//! - Film simulation modes and dynamic range settings

#[test]
fn test_fujifilm_parser_trait() {
    use oxidex::parsers::tiff::makernotes::fujifilm::FujifilmParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = FujifilmParser;

    // Test trait methods
    assert_eq!(parser.manufacturer_name(), "FujiFilm");
    assert_eq!(parser.tag_prefix(), "FujiFilm:");

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
fn test_fujifilm_is_fujifilm_makernote() {
    use oxidex::parsers::tiff::makernotes::fujifilm::is_fujifilm_makernote;

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
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    // Create minimal Fujifilm MakerNote
    let mut data = Vec::new();

    // Fujifilm header: "FUJIFILM" + IFD offset (0x0000000C = 12)
    data.extend_from_slice(b"FUJIFILM");
    data.extend_from_slice(&[0x0C, 0x00, 0x00, 0x00]); // Offset to IFD (little-endian)

    // IFD: entry count (little-endian)
    data.extend_from_slice(&[0x02, 0x00]); // 2 entries

    // Entry 1: Quality (tag 0x1000) = "FINE" -- Quality is a raw ASCII
    // string tag (ExifTool: Writable => 'string'), not an enumerated
    // int16u, so it's stored inline as 4 bytes.
    data.extend_from_slice(&[0x00, 0x10]); // Tag ID
    data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
    data.extend_from_slice(b"FINE"); // Value: "FINE" (inline, <=4 bytes)

    // Entry 2: Sequence Number (tag 0x1101) = 42
    data.extend_from_slice(&[0x01, 0x11]); // Tag ID
    data.extend_from_slice(&[0x04, 0x00]); // Type: LONG
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x2A, 0x00, 0x00, 0x00]); // Value: 42

    // Next IFD offset
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify extracted tags
    assert!(tags.contains_key("FujiFilm:Quality"));
    assert_eq!(tags.get("FujiFilm:Quality"), Some(&"FINE".to_string()));

    assert!(tags.contains_key("FujiFilm:SequenceNumber"));
    assert_eq!(tags.get("FujiFilm:SequenceNumber"), Some(&"42".to_string()));
}

#[test]
fn test_fujifilm_parse_film_simulation() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
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

    // Entry 2: Dynamic Range (tag 0x1400) = Wide (value 3)
    data.extend_from_slice(&[0x00, 0x14]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);

    // Entry 3: AutoBracketing (tag 0x1100) = On (value 1)
    data.extend_from_slice(&[0x00, 0x11]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify decoded film simulation values
    assert_eq!(
        tags.get("FujiFilm:FilmMode"),
        Some(&"Classic Chrome".to_string())
    );
    assert_eq!(tags.get("FujiFilm:DynamicRange"), Some(&"Wide".to_string()));
    assert_eq!(tags.get("FujiFilm:AutoBracketing"), Some(&"On".to_string()));
}

#[test]
fn test_fujifilm_parse_focus_and_flash() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
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
        tags.get("FujiFilm:FocusMode"),
        Some(&"AF-C (Continuous)".to_string())
    );
    // ExifTool names this tag "FujiFlashMode", not "FlashMode".
    assert_eq!(tags.get("FujiFilm:FujiFlashMode"), Some(&"On".to_string()));
    assert_eq!(
        tags.get("FujiFilm:WhiteBalance"),
        Some(&"Daylight".to_string())
    );
}

#[test]
fn test_fujifilm_parse_empty_data() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
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
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    // IMPORTANT: Fujifilm MakerNotes ALWAYS use little-endian byte order internally,
    // regardless of the main EXIF byte order. This test verifies that even when
    // the EXIF container is big-endian, the parser correctly handles Fujifilm's
    // little-endian format.

    let mut data = Vec::new();

    // Fujifilm header with little-endian offset (Fujifilm always uses LE)
    data.extend_from_slice(b"FUJIFILM");
    data.extend_from_slice(&[0x0C, 0x00, 0x00, 0x00]); // Offset to IFD (LE) = 12

    // IFD: 1 entry (little-endian, as Fujifilm always uses)
    data.extend_from_slice(&[0x01, 0x00]); // Entry count (LE) = 1

    // Entry: Quality (tag 0x1000) = "FINE" -- a raw ASCII string tag
    // (ExifTool: Writable => 'string'), stored inline since <=4 bytes.
    data.extend_from_slice(&[0x00, 0x10]); // Tag ID (LE) = 0x1000
    data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII (LE) = 2
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4 (LE)
    data.extend_from_slice(b"FINE"); // Value: "FINE" (inline, <=4 bytes)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD (LE)

    let mut tags = HashMap::new();
    // Pass BigEndian to simulate a big-endian EXIF container,
    // but the parser should still handle Fujifilm's little-endian format correctly
    parse_fujifilm_makernotes(&data, ByteOrder::BigEndian, &mut tags);

    assert_eq!(tags.get("FujiFilm:Quality"), Some(&"FINE".to_string()));
}

#[test]
fn test_fujifilm_parse_advanced_settings() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
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

    // Entry 4: SequenceNumber (tag 0x1101) = 2
    data.extend_from_slice(&[0x01, 0x11]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // Verify advanced settings
    assert!(tags.contains_key("FujiFilm:ShadowTone"));
    assert!(tags.contains_key("FujiFilm:HighlightTone"));
    assert_eq!(tags.get("FujiFilm:FacesDetected"), Some(&"3".to_string()));
    assert_eq!(tags.get("FujiFilm:SequenceNumber"), Some(&"2".to_string()));
}

// ExifTool: FujiFilm.pm's Main table (GROUPS => { 0 => 'MakerNotes', 2 =>
// 'Camera' }, no Group1 override) defaults family-1 to the module name for
// every tag it declares -- `exiftool -G1 -s` shows `[FujiFilm]`, never
// `[MakerNotes]`. CropMode (0x104d) and ColorMode (0x1210) were two of the
// ~33 tags this parser filed under the literal group "MakerNotes" instead,
// so they scored as MISSING against every real ExifTool run even though the
// decoded values were correct. Pins the group prefix so a regression here
// (not just a value regression) fails loudly.
#[test]
fn test_fujifilm_crop_and_color_mode_use_fujifilm_group() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::fujifilm::parse_fujifilm_makernotes;
    use std::collections::HashMap;

    let mut data = Vec::new();
    data.extend_from_slice(b"FUJIFILM");
    data.extend_from_slice(&[0x0C, 0x00, 0x00, 0x00]);

    data.extend_from_slice(&[0x02, 0x00]); // 2 entries

    // CropMode (tag 0x104d) = 1 -> "Full-frame on GFX"
    data.extend_from_slice(&[0x4D, 0x10]);
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // ColorMode (tag 0x1210) = 0x10 -> "Chrome"
    data.extend_from_slice(&[0x10, 0x12]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]);

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(
        tags.get("FujiFilm:CropMode"),
        Some(&"Full-frame on GFX".to_string())
    );
    assert_eq!(tags.get("FujiFilm:ColorMode"), Some(&"Chrome".to_string()));
    assert!(!tags.contains_key("MakerNotes:CropMode"));
    assert!(!tags.contains_key("MakerNotes:ColorMode"));
}
