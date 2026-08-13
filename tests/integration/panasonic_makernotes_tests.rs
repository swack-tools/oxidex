//! Integration tests for Panasonic MakerNotes parser
//!
//! Tests the Panasonic MakerNotes parsing functionality including:
//! - Lens database lookups (M43 and L-mount)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

const DC_S1M2ES: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Panasonic/PanasonicDC-S1M2ES.jpg";
const LEICA_D_LUX8: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Leica/LeicaD-Lux8.jpg";

/// Leica's D-Lux 8 uses ExifTool's `MakerNoteLeica10`, which routes its
/// "LEICA CAMERA AG" MakerNote to Panasonic::Main.  The expected values below
/// are from the pinned ExifTool 13.59 oracle (`-G1 -s -n`).
#[test]
fn leica_d_lux8_reports_panasonic_main_late_tags() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(LEICA_D_LUX8).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(LEICA_D_LUX8)).expect("Leica D-Lux 8 parses");
    assert_eq!(
        metadata.get_string("Panasonic:WBShiftIntelligentAuto"),
        Some("0")
    );
    assert_eq!(
        metadata.get_string("Panasonic:WBShiftCreativeControl"),
        Some("0")
    );
    assert_eq!(
        metadata.get_string("Panasonic:HighlightShadow"),
        Some("0 0")
    );
    assert_eq!(
        metadata.get_string("Panasonic:VideoBurstResolution"),
        Some("Off or 4K")
    );
    assert_eq!(metadata.get_string("Panasonic:RedEyeRemoval"), Some("Off"));
    assert_eq!(metadata.get_string("Panasonic:VideoBurstMode"), Some("Off"));
    assert_eq!(metadata.get_string("Panasonic:FocusBracket"), Some("0"));
    assert_eq!(
        metadata.get_string("Panasonic:LongExposureNRUsed"),
        Some("No")
    );
    assert_eq!(
        metadata.get_string("Panasonic:PostFocusMerging"),
        Some("Post Focus Auto Merging or None")
    );
    assert_eq!(metadata.get_string("Panasonic:VideoPreburst"), Some("No"));
    assert_eq!(
        metadata.get_string("Panasonic:SensorType"),
        Some("Multi-aspect")
    );
    assert_eq!(
        metadata.get_string("Panasonic:MonochromeGrainEffect"),
        Some("Off")
    );
}

#[test]
fn panasonic_dc_s1m2es_reports_late_makernote_tags() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(DC_S1M2ES).is_file() {
        return;
    }

    let fixture = DC_S1M2ES;
    if !Path::new(fixture).is_file() {
        eprintln!("skipping: corpus fixture not present at {fixture}");
        return;
    }

    let metadata = read_metadata(Path::new(fixture)).expect("Panasonic DC-S1M2ES parses");
    assert_eq!(metadata.get_string("Panasonic:HybridLogGamma"), Some("Off"));
    assert_eq!(
        metadata.get_string("Panasonic:LensTypeModel"),
        Some("07 40")
    );
    assert_eq!(metadata.get_string("Panasonic:MinimumISO"), Some("100"));
    assert_eq!(
        metadata.get_string("Panasonic:AFSubjectDetection"),
        Some("Human Eye/Face/Body")
    );
    assert_eq!(
        metadata.get_string("Panasonic:DynamicRangeBoost"),
        Some("Off")
    );
    assert_eq!(metadata.get_string("Panasonic:LUT1Name"), Some(""));
    assert_eq!(metadata.get_string("Panasonic:LUT1Opacity"), Some("0"));
    assert_eq!(metadata.get_string("Panasonic:LUT2Name"), Some(""));
    assert_eq!(metadata.get_string("Panasonic:LUT2Opacity"), Some("0"));
    assert_eq!(
        metadata.get_string("Panasonic:AFAreaSize"),
        Some("0.0205078125 0.03125")
    );
    assert_eq!(
        metadata.get_string("Panasonic:NoiseReductionStrength"),
        Some("0")
    );
}

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

    // Entry 2: FocusMode (tag 0x0007) = 4.
    // Panasonic.pm:329 declares `4 => 'Auto, Focus button'`. The label this
    // once asserted, "AF-S (Single)", appears in no ExifTool source file --
    // and ExifTool's plain `AF-S` is id 6, not 4, so it was wrong twice over.
    data.extend_from_slice(&[0x07, 0x00]); // Tag ID = 0x0007
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // 4 = Auto, Focus button

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
        Some(&"Auto, Focus button".to_string())
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

    // Entry 1: PhotoStyle (tag 0x0089) = 17.
    // Panasonic.pm:1152 declares `17 => 'V-Log'`. This once fed 10 and
    // expected V-Log, which encoded the pre-transcription table that was
    // shifted at every shared id; ExifTool declares nothing at 10.
    data.extend_from_slice(&[0x89, 0x00]); // Tag ID = 0x0089
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x11, 0x00, 0x00, 0x00]); // 17 = V-Log

    // Entry 2: HDR (tag 0x009E) = 100.
    // Panasonic.pm:1251 declares `100 => '1 EV'`. The label this once
    // asserted, "HDR Auto", is Sony's (Sony.pm:6310, value 1) -- it appears
    // nowhere in Panasonic.pm.
    data.extend_from_slice(&[0x9E, 0x00]); // Tag ID = 0x009E
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x64, 0x00, 0x00, 0x00]); // 100 = 1 EV

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(tags.get("Panasonic:PhotoStyle"), Some(&"V-Log".to_string()));
    assert_eq!(tags.get("Panasonic:HDR"), Some(&"1 EV".to_string()));
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
    // TIFF6.0: a value that fits within the 4-byte field is left-justified
    // there, regardless of byte order -- so a count-1 SHORT's 2 bytes occupy
    // the field's FIRST two bytes, not its last two. This fixture used to
    // encode the value right-justified (`00 00 00 02`), which is not how any
    // real TIFF/EXIF writer lays inline values out; it happened to match the
    // pre-fix code's incorrect low-half read. `inline_u16_value` now reads
    // the high half for BigEndian, matching the spec and every real
    // big-endian Panasonic MakerNote directory verified against pinned
    // ExifTool 13.59 (see PanasonicDMC-LC5/20/40.jpg, LeicaDigilux1.jpg).
    data.extend_from_slice(&[0x00, 0x02, 0x00, 0x00]); // Value: 2 (BE) = Daylight

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

/// `MakerNoteLeica10` (MakerNotes.pm:724-731) is dispatched on its signature
/// alone -- `Condition => '$$valPt =~ /^LEICA CAMERA AG\0/'` -- and routes to
/// `Image::ExifTool::Panasonic::Main` with `Start => '$valuePtr + 18'`, not to
/// any `Leica2`..`Leica9` table. It is what Leica's Panasonic-built compacts
/// write: the D-Lux 7, D-Lux 8 and V-Lux 5.
///
/// The signature is 16 bytes and the IFD begins at 18, so two pad bytes sit
/// between them. `LeicaD-Lux7.jpg`'s MakerNote opens
/// `4c 45 49 43 41 20 43 41 4d 45 52 41 20 41 47 00  00 00  9d 00  01 00 03 00`
/// -- "LEICA CAMERA AG\0", two NULs, a 157-entry count, then tag 0x0001 as a
/// SHORT. Before this was recognised, the Leica parser claimed the header,
/// skipped 15 bytes of it and decoded nothing, so all three bodies reported
/// zero MakerNote tags where ExifTool reports ~100.
#[test]
fn test_leica10_header_routes_to_panasonic_main() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::panasonic::{
        PanasonicParser, is_leica10_makernote, parse_panasonic_makernotes,
    };
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA CAMERA AG\0"); // 16-byte signature
    data.extend_from_slice(&[0x00, 0x00]); // two pad bytes -> IFD at +18
    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x01, 0x00]); // tag 0x0001 ImageQuality
    data.extend_from_slice(&[0x03, 0x00]); // type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // count 1
    data.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]); // inline value 7 = RAW
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // next IFD

    assert!(is_leica10_makernote(&data));
    assert!(PanasonicParser.validate_header(&data));

    let mut tags = HashMap::new();
    parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // ExifTool prints `[Panasonic] ImageQuality : RAW` for LeicaD-Lux7.jpg.
    assert_eq!(
        tags.get("Panasonic:ImageQuality"),
        Some(&"RAW".to_string()),
        "Leica10 payload must decode against Panasonic::Main"
    );
}

/// The Leica10 signature requires its terminating NUL. ExifTool's Condition is
/// `/^LEICA CAMERA AG\0/`, so a payload that merely begins with the words is
/// not a Leica10 MakerNote and must not be given the +18 IFD offset.
#[test]
fn test_leica10_requires_terminating_nul() {
    use oxidex::parsers::tiff::makernotes::panasonic::is_leica10_makernote;

    assert!(!is_leica10_makernote(b"LEICA CAMERA AG extra data"));
    // ...and the bare signature with no room for an IFD is not one either.
    assert!(!is_leica10_makernote(b"LEICA CAMERA AG\0"));
}
