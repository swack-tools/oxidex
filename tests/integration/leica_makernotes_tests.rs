//! Integration tests for Leica MakerNotes parser
//!
//! Tests the Leica MakerNotes parsing functionality including:
//! - Lens database lookups (M-mount, SL-mount, L-mount lenses)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

#[test]
fn test_leica_header_validation_short() {
    use oxidex::parsers::tiff::makernotes::leica::is_leica_makernote;

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
    use oxidex::parsers::tiff::makernotes::leica::is_leica_makernote;

    // Test valid long "LEICA CAMERA AG" header
    let valid_header = b"LEICA CAMERA AG\x00\x00\x10";
    assert!(is_leica_makernote(valid_header));
}

#[test]
fn test_leica_header_validation_no_header() {
    use oxidex::parsers::tiff::makernotes::leica::is_leica_makernote;

    // Test data with no header but valid IFD entry count (15 entries)
    let no_header = b"\x0F\x00\x00\x00\x00\x00\x00\x00";
    assert!(is_leica_makernote(no_header));

    // Test data with unreasonable entry count (should fail)
    let bad_count = b"\xFF\xFF\x00\x00\x00\x00\x00\x00";
    assert!(!is_leica_makernote(bad_count));
}

#[test]
fn test_leica_makernote_parse_basic() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
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
fn test_leica_makernote_parse_camera_settings() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
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
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
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
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
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

/// `MakerNoteLeica2` (M8), values immediately after the entry table.
///
/// Ground truth: `exiftool -G1 -s LeicaM8.jpg` reports
/// `ExternalSensorBrightnessValue = 9.14` and `MeasuredLV = 7.54`
/// (ExifTool Panasonic.pm:1656/1664, `Base => '$start'`).
#[test]
fn test_leica2_measured_lv_and_external_sensor_brightness() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA\0\0\0"); // 8-byte header; IFD starts here + 8
    data.extend_from_slice(&2u16.to_le_bytes()); // 2 entries

    // 0x0311 ExternalSensorBrightnessValue: rational64s at IFD-relative 26
    data.extend_from_slice(&0x0311u16.to_le_bytes());
    data.extend_from_slice(&5u16.to_le_bytes()); // type 5: RATIONAL
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&26u32.to_le_bytes());

    // 0x0312 MeasuredLV: rational64s at IFD-relative 34
    data.extend_from_slice(&0x0312u16.to_le_bytes());
    data.extend_from_slice(&5u16.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&34u32.to_le_bytes());

    // Value block starts right after the 2-entry table (IFD-relative 26), so
    // `leica2_base_shift` sees a gap of -2 and keeps the unshifted `$start` base.
    data.extend_from_slice(&914i32.to_le_bytes());
    data.extend_from_slice(&100i32.to_le_bytes());
    data.extend_from_slice(&754i32.to_le_bytes());
    data.extend_from_slice(&100i32.to_le_bytes());

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());
    assert_eq!(
        tags.get("Leica:ExternalSensorBrightnessValue"),
        Some(&"9.14".to_string())
    );
    assert_eq!(tags.get("Leica:MeasuredLV"), Some(&"7.54".to_string()));
}

/// `MakerNoteLeica2` (M8), `FixLeicaBase` shift applied.
///
/// Some M8 files (`LeicaM8.2.jpg` in the ExifTool test corpus) write their
/// value offsets relative to the payload's own start rather than the IFD --
/// ExifTool detects this by the gap between the lowest value offset and the
/// end of the entry list (`MakerNotes.pm:1669-1691`, `FixLeicaBase`). Ground
/// truth: `ExternalSensorBrightnessValue = 0.90`, `MeasuredLV = -0.95`.
#[test]
fn test_leica2_fix_leica_base_shift() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    let mut data = vec![0u8; 56];
    data[0..8].copy_from_slice(b"LEICA\0\0\0");
    data[8..10].copy_from_slice(&2u16.to_le_bytes());

    // Value offsets of 40 and 48 put the gap past the entry table
    // (2 * 12 + 4 = 28) at more than 8 bytes, so FixLeicaBase shifts the base
    // 8 bytes earlier -- to the payload's own start -- and these offsets
    // resolve against `data[40..]` / `data[48..]` directly.
    data[10..12].copy_from_slice(&0x0311u16.to_le_bytes());
    data[12..14].copy_from_slice(&5u16.to_le_bytes());
    data[14..18].copy_from_slice(&1u32.to_le_bytes());
    data[18..22].copy_from_slice(&40u32.to_le_bytes());

    data[22..24].copy_from_slice(&0x0312u16.to_le_bytes());
    data[24..26].copy_from_slice(&5u16.to_le_bytes());
    data[26..30].copy_from_slice(&1u32.to_le_bytes());
    data[30..34].copy_from_slice(&48u32.to_le_bytes());

    data[40..44].copy_from_slice(&90i32.to_le_bytes());
    data[44..48].copy_from_slice(&100i32.to_le_bytes());
    data[48..52].copy_from_slice(&(-95i32).to_le_bytes());
    data[52..56].copy_from_slice(&100i32.to_le_bytes());

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());
    assert_eq!(
        tags.get("Leica:ExternalSensorBrightnessValue"),
        Some(&"0.90".to_string())
    );
    assert_eq!(tags.get("Leica:MeasuredLV"), Some(&"-0.95".to_string()));
}

/// `MakerNoteLeica9` (M10/M11/S): `Base` is unset, so value offsets count
/// from the enclosing TIFF header, not the payload. Only reachable through
/// [`MakerNoteParser::parse_with_context`] with a located context.
///
/// Ground truth: `exiftool -G1 -s LeicaM10-R.jpg` reports
/// `ExternalSensorBrightnessValue = 0.56`, `MeasuredLV = -19.93`.
#[test]
fn test_leica9_measured_lv_needs_tiff_relative_base() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    let mut tiff = vec![0u8; 80];
    let payload_offset = 20usize;

    tiff[payload_offset..payload_offset + 8].copy_from_slice(b"LEICA\0\x02\0");
    tiff[payload_offset + 8..payload_offset + 10].copy_from_slice(&2u16.to_le_bytes());

    // Value offsets are absolute TIFF offsets (60, 70) -- unreachable from the
    // 34-byte payload alone, which is why this needs `parse_with_context`.
    let e0 = payload_offset + 10;
    tiff[e0..e0 + 2].copy_from_slice(&0x0311u16.to_le_bytes());
    tiff[e0 + 2..e0 + 4].copy_from_slice(&10u16.to_le_bytes()); // type 10: SRATIONAL
    tiff[e0 + 4..e0 + 8].copy_from_slice(&1u32.to_le_bytes());
    tiff[e0 + 8..e0 + 12].copy_from_slice(&60u32.to_le_bytes());

    let e1 = e0 + 12;
    tiff[e1..e1 + 2].copy_from_slice(&0x0312u16.to_le_bytes());
    tiff[e1 + 2..e1 + 4].copy_from_slice(&10u16.to_le_bytes());
    tiff[e1 + 4..e1 + 8].copy_from_slice(&1u32.to_le_bytes());
    tiff[e1 + 8..e1 + 12].copy_from_slice(&70u32.to_le_bytes());

    tiff[60..64].copy_from_slice(&56i32.to_le_bytes());
    tiff[64..68].copy_from_slice(&100i32.to_le_bytes());
    tiff[70..74].copy_from_slice(&(-1993i32).to_le_bytes());
    tiff[74..78].copy_from_slice(&100i32.to_le_bytes());

    let payload_len = 34; // header(8) + count(2) + 2 entries * 12
    let ctx = MakerNoteContext::in_tiff(&tiff, payload_offset, payload_len, 0);

    let result = parser.parse_with_context(&ctx, ByteOrder::LittleEndian, None, &mut tags);
    assert!(result.is_ok());
    assert_eq!(
        tags.get("Leica:ExternalSensorBrightnessValue"),
        Some(&"0.56".to_string())
    );
    assert_eq!(tags.get("Leica:MeasuredLV"), Some(&"-19.93".to_string()));
}

/// `MakerNoteLeica9`, but parsed detached (no enclosing TIFF known): the two
/// tags are skipped rather than resolved against a guessed base.
#[test]
fn test_leica9_measured_lv_absent_without_tiff_context() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    let mut data = vec![0u8; 34];
    data[0..8].copy_from_slice(b"LEICA\0\x02\0");
    data[8..10].copy_from_slice(&2u16.to_le_bytes());
    data[10..12].copy_from_slice(&0x0311u16.to_le_bytes());
    data[12..14].copy_from_slice(&10u16.to_le_bytes());
    data[14..18].copy_from_slice(&1u32.to_le_bytes());
    data[18..22].copy_from_slice(&60u32.to_le_bytes());
    data[22..24].copy_from_slice(&0x0312u16.to_le_bytes());
    data[24..26].copy_from_slice(&10u16.to_le_bytes());
    data[26..30].copy_from_slice(&1u32.to_le_bytes());
    data[30..34].copy_from_slice(&70u32.to_le_bytes());

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());
    assert_eq!(tags.get("Leica:ExternalSensorBrightnessValue"), None);
    assert_eq!(tags.get("Leica:MeasuredLV"), None);
}

/// `MakerNoteLeica4` (M9/M Monochrom): `MeasuredLV`/`ExternalSensorBrightnessValue`
/// live in the `Subdir3400` subdirectory as `int32s` with `ValueConv $val/1e5`
/// (ExifTool Panasonic.pm:1908/1917).
///
/// Ground truth: `exiftool -G1 -s LeicaM9.jpg` reports `MeasuredLV = 6.92`,
/// `ExternalSensorBrightnessValue = 8.89`.
#[test]
fn test_leica4_subdir3400_measured_lv() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    let mut data = vec![0u8; 48];
    data[0..8].copy_from_slice(b"LEICA0\x03\0");
    data[8..10].copy_from_slice(&1u16.to_le_bytes()); // 1 top-level entry: Subdir3400

    // Subdir3400 pointer: payload-relative offset 22, 26 bytes long.
    data[10..12].copy_from_slice(&0x3400u16.to_le_bytes());
    data[12..14].copy_from_slice(&7u16.to_le_bytes()); // type 7: UNDEFINED
    data[14..18].copy_from_slice(&26u32.to_le_bytes());
    data[18..22].copy_from_slice(&22u32.to_le_bytes());

    // Subdirectory: 2 entries, values inline (int32s fits in 4 bytes).
    data[22..24].copy_from_slice(&2u16.to_le_bytes());
    data[24..26].copy_from_slice(&0x3407u16.to_le_bytes());
    data[26..28].copy_from_slice(&9u16.to_le_bytes()); // type 9: SLONG
    data[28..32].copy_from_slice(&1u32.to_le_bytes());
    data[32..36].copy_from_slice(&691_968i32.to_le_bytes());

    data[36..38].copy_from_slice(&0x3408u16.to_le_bytes());
    data[38..40].copy_from_slice(&9u16.to_le_bytes());
    data[40..44].copy_from_slice(&1u32.to_le_bytes());
    data[44..48].copy_from_slice(&888_832i32.to_le_bytes());

    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());
    assert_eq!(tags.get("Leica:MeasuredLV"), Some(&"6.92".to_string()));
    assert_eq!(
        tags.get("Leica:ExternalSensorBrightnessValue"),
        Some(&"8.89".to_string())
    );
}

#[test]
fn test_leica_makernote_parser_trait_implementation() {
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = LeicaMakerNoteParser;
    assert_eq!(parser.manufacturer_name(), "Leica");
    assert_eq!(parser.tag_prefix(), "Leica:");
}

/// `%Image::ExifTool::Panasonic::leicaLensTypes`, with the Panasonic.pm line
/// quoted beside each expectation.  ExifTool splits the stored int32u with
/// `ValueConv => '($val >> 2) . " " . ($val & 0x3)'` and looks the pair up,
/// falling back through its `OTHER` handler to the first number alone.
#[test]
fn test_leica_lens_types_match_exiftool() {
    use oxidex::parsers::tiff::makernotes::lens_data::leica;

    // LeicaM8.jpg: Leica2 0x0310 holds 135 -> "33 3" -> falls back to 33.
    // `33 => 'Summicron-M 50mm f/2 (IV, V)'` (Panasonic.pm:103)
    assert_eq!(leica::lookup(135), Some("Summicron-M 50mm f/2 (IV, V)"));

    // LeicaM9.jpg: Subdir 0x3405 holds 122 -> "30 2" -> falls back to 30.
    // `30 => 'Summicron-M 35mm f/2 ASPH.'` (Panasonic.pm:99)
    assert_eq!(leica::lookup(122), Some("Summicron-M 35mm f/2 ASPH."));

    // The two-number keys win over the bare id when they exist.
    // `'6 0' => 'Summilux-M 35mm f/1.4'` (Panasonic.pm:81) at raw 24 = "6 0",
    // versus `6 => 'Summicron-M 35mm f/2 (IV)'` (Panasonic.pm:80) at raw 25.
    assert_eq!(leica::lookup(24), Some("Summilux-M 35mm f/1.4"));
    assert_eq!(leica::lookup(25), Some("Summicron-M 35mm f/2 (IV)"));

    // `'0 0' => 'Uncoded lens'` (Panasonic.pm:71)
    assert_eq!(leica::lookup(0), Some("Uncoded lens"));

    // The invented ids the old fabricated table carried are absent: ExifTool
    // files nothing under 405, and 100 is not a %leicaLensTypes key.
    assert_eq!(leica::lookup(405 << 2), None);
    assert_eq!(leica::lookup(100 << 2), None);

    // ExifTool prints the ValueConv string inside "Unknown (...)".
    assert_eq!(leica::value_conv(135), "33 3");
    assert_eq!(leica::value_conv(122), "30 2");
}

/// `%Panasonic::Leica2` 0x0310 is the `LensType` tag (Panasonic.pm:1648); the
/// 0x0013/0x0014/0x0015 ids the old reader watched are not Leica tags at all.
#[test]
fn test_leica_makernote_parse_lens_type() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();

    // LeicaM8.jpg stores 135 here; ExifTool prints
    // "Summicron-M 50mm f/2 (IV, V)" (135 >> 2 == 33, Panasonic.pm:103).
    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA\0\0\0"); // header
    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x10, 0x03]); // tag: 0x0310
    data.extend_from_slice(&[0x04, 0x00]); // type: LONG
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count: 1
    data.extend_from_slice(&[0x87, 0x00, 0x00, 0x00]); // value: 135

    assert!(
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );
    assert_eq!(
        tags.get("Leica:LensType"),
        Some(&"Summicron-M 50mm f/2 (IV, V)".to_string())
    );
}
