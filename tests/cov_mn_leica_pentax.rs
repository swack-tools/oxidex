//! Coverage tests for TIFF MakerNote parsers: Leica and Pentax.
//!
//! Wave-1 (`cov_makernotes_b.rs`) already exercised the happy path and a small
//! subset of tags for both parsers. This wave goes after the REMAINING uncovered
//! code: every per-tag match arm in `LeicaMakerNoteParser::parse` and
//! `PentaxParser::parse`, the multiple model-variant tag tables, the format-
//! specific value branches (Kelvin / temperature / focus distance / DNG version /
//! angle / focal-length / aperture / EV formatting), the lens-name success
//! lookups, `extract_value_as_i32` field-type branches (BYTE/SHORT/SBYTE/SSHORT),
//! offset-based string extraction, big-endian parsing, and the malformed-input
//! error branches.
//!
//! A MakerNote IFD is a standard TIFF IFD:
//!   [entry_count: u16][entries...]
//! Each entry is 12 bytes: [tag: u16][type: u16][count: u32][value/offset: u32].

#[path = "common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;

use oxidex::core::FileReader;
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::leica::{LeicaMakerNoteParser, is_leica_makernote};
use oxidex::parsers::tiff::makernotes::pentax::{PentaxParser, is_pentax_makernote};
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

// ===========================================================================
// IFD construction helpers
// ===========================================================================

/// A single synthetic IFD entry: tag, type, count, inline value/offset.
#[derive(Clone, Copy)]
struct Ent {
    tag: u16,
    typ: u16,
    count: u32,
    value: u32,
}

fn ent(tag: u16, typ: u16, count: u32, value: u32) -> Ent {
    Ent {
        tag,
        typ,
        count,
        value,
    }
}

/// Build a little-endian IFD block: [entry_count:2][entries...].
fn ifd_le(entries: &[Ent]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.tag.to_le_bytes());
        out.extend_from_slice(&e.typ.to_le_bytes());
        out.extend_from_slice(&e.count.to_le_bytes());
        out.extend_from_slice(&e.value.to_le_bytes());
    }
    out
}

/// Build a big-endian IFD block.
fn ifd_be(entries: &[Ent]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for e in entries {
        out.extend_from_slice(&e.tag.to_be_bytes());
        out.extend_from_slice(&e.typ.to_be_bytes());
        out.extend_from_slice(&e.count.to_be_bytes());
        out.extend_from_slice(&e.value.to_be_bytes());
    }
    out
}

// Field type constants.
const T_BYTE: u16 = 1; // BYTE
const T_ASCII: u16 = 2; // ASCII
const T_SHORT: u16 = 3; // SHORT
const T_LONG: u16 = 4; // LONG
const T_SBYTE: u16 = 6; // SBYTE
const T_SSHORT: u16 = 8; // SSHORT

// ===========================================================================
// Template sanity: TestReader still satisfies FileReader
// ===========================================================================

#[test]
fn test_test_reader_basics() {
    let reader = TestReader::new(vec![9, 8, 7, 6]);
    assert_eq!(reader.size(), 4);
    assert_eq!(reader.read(1, 2).unwrap(), &[8, 7]);
    assert!(reader.read(0, 99).is_err());
}

// ===========================================================================
// LEICA
// ===========================================================================

mod leica {
    use super::*;

    /// Build a Leica MakerNote with the short "LEICA\0\0\0" header + IFD + tail
    /// padding so any offset-based reads stay in bounds.
    fn leica_short(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"LEICA\0\0\0");
        out.extend_from_slice(&ifd_le(entries));
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    /// Build a Leica MakerNote with no header (bare IFD) + tail padding.
    fn leica_bare(entries: &[Ent]) -> Vec<u8> {
        let mut out = ifd_le(entries);
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    // -- is_leica_makernote: every branch -----------------------------------

    #[test]
    fn test_is_leica_makernote_all_branches() {
        // Short header.
        assert!(is_leica_makernote(b"LEICA\0\0\0plus more data"));
        // Long "LEICA CAMERA AG" header.
        assert!(is_leica_makernote(b"LEICA CAMERA AG and trailing"));
        // Headerless but plausible IFD entry count (10 entries).
        assert!(is_leica_makernote(&[0x0A, 0x00, 0, 0, 0, 0, 0, 0]));
        // Entry count of exactly 1 (lower edge, still valid).
        assert!(is_leica_makernote(&[0x01, 0x00, 0, 0, 0, 0, 0, 0]));
        // Too short (< 8 bytes) -> false.
        assert!(!is_leica_makernote(b"LEICA"));
        assert!(!is_leica_makernote(&[]));
        // 8 bytes but entry count 0 and not a header -> false.
        assert!(!is_leica_makernote(&[0x00, 0x00, 0, 0, 0, 0, 0, 0]));
        // Entry count too large (>= 150) -> false.
        assert!(!is_leica_makernote(&[0x96, 0x00, 0, 0, 0, 0, 0, 0]));
    }

    #[test]
    fn test_leica_trait_basics() {
        let p = LeicaMakerNoteParser;
        assert_eq!(p.manufacturer_name(), "Leica");
        assert_eq!(p.tag_prefix(), "Leica:");
        assert!(p.validate_header(b"LEICA\0\0\0xxxx"));
        assert!(!p.validate_header(b"NOPE"));
        // lookup_lens delegates to the lens database; known id 5 resolves.
        assert_eq!(
            p.lookup_lens(5).as_deref(),
            Some("Leica Summilux-M 50mm f/1.4 ASPH")
        );
        assert!(p.lookup_lens(0xFFFF).is_none());
    }

    // -- Quality / profile / WB / color-temp / RGB levels -------------------

    #[test]
    fn test_leica_basic_info_tags() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0003, T_SHORT, 1, 5),   // Quality -> DNG
            ent(0x0004, T_SHORT, 1, 8),   // UserProfile -> Monochrome
            ent(0x0005, T_LONG, 1, 4242), // SerialNumber (<=4 count)
            ent(0x0006, T_SHORT, 1, 6),   // WhiteBalance -> Shade
            ent(0x0023, T_SHORT, 1, 9),   // WBMode -> Auto (ambient priority)
            ent(0x000C, T_LONG, 1, 5600), // ColorTemperature -> "5600K"
            ent(0x000D, T_LONG, 1, 512),  // WBRedLevel
            ent(0x000E, T_LONG, 1, 256),  // WBGreenLevel
            ent(0x000F, T_LONG, 1, 384),  // WBBlueLevel
            ent(0x000B, T_SSHORT, 1, 35), // CameraTemperature -> "35°C"
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert_eq!(tags.get("Leica:Quality").map(String::as_str), Some("DNG"));
        assert_eq!(
            tags.get("Leica:UserProfile").map(String::as_str),
            Some("Monochrome")
        );
        assert_eq!(
            tags.get("Leica:SerialNumber").map(String::as_str),
            Some("4242")
        );
        assert_eq!(
            tags.get("Leica:WhiteBalance").map(String::as_str),
            Some("Shade")
        );
        assert!(tags.contains_key("Leica:WBMode"));
        assert_eq!(
            tags.get("Leica:ColorTemperature").map(String::as_str),
            Some("5600K")
        );
        assert_eq!(
            tags.get("Leica:WBRedLevel").map(String::as_str),
            Some("512")
        );
        assert_eq!(
            tags.get("Leica:WBGreenLevel").map(String::as_str),
            Some("256")
        );
        assert_eq!(
            tags.get("Leica:WBBlueLevel").map(String::as_str),
            Some("384")
        );
        assert!(tags.contains_key("Leica:CameraTemperature"));
    }

    // SerialNumber / InternalSerialNumber / LensSerialNumber only insert when
    // value_count <= 4: drive the "count > 4 -> skipped" branch.
    #[test]
    fn test_leica_serial_count_gt_4_skipped() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0005, T_LONG, 8, 1), // SerialNumber, count 8 -> skipped
            ent(0x0027, T_LONG, 8, 1), // InternalSerialNumber, count 8 -> skipped
            ent(0x0031, T_LONG, 8, 1), // LensSerialNumber, count 8 -> skipped
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(!tags.contains_key("Leica:SerialNumber"));
        assert!(!tags.contains_key("Leica:InternalSerialNumber"));
        assert!(!tags.contains_key("Leica:LensSerialNumber"));
    }

    // -- Image processing / lens id+name / lens type ------------------------

    #[test]
    fn test_leica_processing_and_lens_tags() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0010, T_SHORT, 1, 3),   // Sharpening
            ent(0x0011, T_SHORT, 1, 2),   // Contrast
            ent(0x0012, T_SHORT, 1, 1),   // Saturation
            ent(0x0013, T_SHORT, 1, 5),   // LensID 5 -> known name lookup
            ent(0x0014, T_LONG, 1, 99),   // LensType
            ent(0x0027, T_LONG, 1, 1001), // InternalSerialNumber (<=4)
            ent(0x0031, T_LONG, 1, 2002), // LensSerialNumber (<=4)
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert_eq!(tags.get("Leica:Sharpening").map(String::as_str), Some("3"));
        assert_eq!(tags.get("Leica:Contrast").map(String::as_str), Some("2"));
        assert_eq!(tags.get("Leica:Saturation").map(String::as_str), Some("1"));
        assert_eq!(tags.get("Leica:LensID").map(String::as_str), Some("5"));
        assert_eq!(
            tags.get("Leica:LensModel").map(String::as_str),
            Some("Leica Summilux-M 50mm f/1.4 ASPH")
        );
        assert_eq!(tags.get("Leica:LensType").map(String::as_str), Some("99"));
        assert_eq!(
            tags.get("Leica:InternalSerialNumber").map(String::as_str),
            Some("1001")
        );
        assert_eq!(
            tags.get("Leica:LensSerialNumber").map(String::as_str),
            Some("2002")
        );
    }

    // Unknown lens id: LensID inserted, but LensModel NOT (lookup miss branch).
    #[test]
    fn test_leica_lens_id_unknown_no_model() {
        let p = LeicaMakerNoteParser;
        let entries = [ent(0x0013, T_SHORT, 1, 60000)];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Leica:LensID").map(String::as_str), Some("60000"));
        assert!(!tags.contains_key("Leica:LensModel"));
    }

    // -- Exposure / metering / flash decoders -------------------------------

    #[test]
    fn test_leica_exposure_metering_flash() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0020, T_SHORT, 1, 2),   // ExposureMode -> Aperture Priority
            ent(0x0021, T_SHORT, 1, 3),   // MeteringMode -> Spot
            ent(0x0025, T_SHORT, 1, 5),   // FlashMode -> Rear Curtain Sync
            ent(0x0026, T_LONG, 1, 1200), // FlashEnergy
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Leica:ExposureMode").map(String::as_str),
            Some("Aperture Priority")
        );
        assert_eq!(
            tags.get("Leica:MeteringMode").map(String::as_str),
            Some("Spot")
        );
        assert_eq!(
            tags.get("Leica:FlashMode").map(String::as_str),
            Some("Rear Curtain Sync")
        );
        assert_eq!(
            tags.get("Leica:FlashEnergy").map(String::as_str),
            Some("1200")
        );
    }

    // -- Lens / focus / AF / stabilization ----------------------------------

    #[test]
    fn test_leica_focus_af_stabilization() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0034, T_LONG, 1, 123456), // ShutterCount
            ent(0x0035, T_LONG, 1, 1500),   // FocusDistance -> "1500 mm"
            ent(0x0052, T_SHORT, 1, 3),     // AFMode -> AF-C
            ent(0x0053, T_SHORT, 1, 4),     // ImageStabilization -> On (Dual)
            ent(0x0032, T_SHORT, 1, 1),     // ContrastDetectAF -> On
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Leica:ShutterCount").map(String::as_str),
            Some("123456")
        );
        assert_eq!(
            tags.get("Leica:FocusDistance").map(String::as_str),
            Some("1500 mm")
        );
        assert_eq!(tags.get("Leica:AFMode").map(String::as_str), Some("AF-C"));
        assert_eq!(
            tags.get("Leica:ImageStabilization").map(String::as_str),
            Some("On (Dual)")
        );
        assert_eq!(
            tags.get("Leica:ContrastDetectAF").map(String::as_str),
            Some("On")
        );
    }

    // ContrastDetectAF / MacroMode / PerspectiveControl "Off"/"Unknown" arms.
    #[test]
    fn test_leica_binary_flag_off_and_unknown() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0032, T_SHORT, 1, 0),  // ContrastDetectAF -> Off
            ent(0x0070, T_SHORT, 1, 0),  // MacroMode -> Off
            ent(0x0062, T_SHORT, 1, 99), // PerspectiveControl -> Unknown
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Leica:ContrastDetectAF").map(String::as_str),
            Some("Off")
        );
        assert_eq!(tags.get("Leica:MacroMode").map(String::as_str), Some("Off"));
        assert_eq!(
            tags.get("Leica:PerspectiveControl").map(String::as_str),
            Some("Unknown")
        );
    }

    // -- Digital zoom: both formatting branches + suppressed (value 0) -------

    #[test]
    fn test_leica_digital_zoom_branches() {
        let p = LeicaMakerNoteParser;
        // value > 100 -> "{}%" branch.
        let mut tags = HashMap::new();
        let data = leica_short(&[ent(0x0054, T_LONG, 1, 250)]);
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Leica:DigitalZoom").map(String::as_str),
            Some("2%")
        );

        // 0 < value <= 100 -> "{}.{}x" branch.
        let mut tags2 = HashMap::new();
        let data2 = leica_short(&[ent(0x0054, T_LONG, 1, 35)]);
        assert!(p.parse(&data2, ByteOrder::LittleEndian, &mut tags2).is_ok());
        assert_eq!(
            tags2.get("Leica:DigitalZoom").map(String::as_str),
            Some("3.5x")
        );

        // value == 0 -> neither branch inserts a tag.
        let mut tags3 = HashMap::new();
        let data3 = leica_short(&[ent(0x0054, T_LONG, 1, 0)]);
        assert!(p.parse(&data3, ByteOrder::LittleEndian, &mut tags3).is_ok());
        assert!(!tags3.contains_key("Leica:DigitalZoom"));
    }

    // -- Q-series / scene / crop / macro-on ---------------------------------

    #[test]
    fn test_leica_q_series_and_scene() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0070, T_SHORT, 1, 1), // MacroMode -> On
            ent(0x0071, T_SHORT, 1, 3), // SceneMode -> Macro
            ent(0x0061, T_SHORT, 1, 1), // CropMode -> APS-C
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Leica:MacroMode").map(String::as_str), Some("On"));
        assert_eq!(
            tags.get("Leica:SceneMode").map(String::as_str),
            Some("Macro")
        );
        assert_eq!(
            tags.get("Leica:CropMode").map(String::as_str),
            Some("APS-C")
        );
    }

    // -- Measured LV / aperture / film / frame / angles / focal / image id --

    #[test]
    fn test_leica_misc_formatted_tags() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0041, T_LONG, 1, 200),  // BaseISO
            ent(0x0009, T_SHORT, 1, 75),  // MeasuredLV -> "7.5 EV"
            ent(0x000A, T_SHORT, 1, 28),  // ApproximateFNumber -> "f/2.8"
            ent(0x0022, T_LONG, 1, 4),    // FilmMode
            ent(0x0040, T_LONG, 1, 11),   // FrameSelector
            ent(0x0063, T_SSHORT, 1, 12), // CameraPitchAngle -> "12°"
            ent(0x0064, T_SSHORT, 1, 3),  // CameraRollAngle -> "3°"
            ent(0x0030, T_LONG, 1, 50),   // FocalLength35mm -> "50 mm"
            ent(0x0042, T_LONG, 1, 7),    // ImageID
            ent(0x0050, T_LONG, 1, 2),    // PictureControl
            ent(0x0051, T_LONG, 1, 9),    // AFPoint
            ent(0x0024, T_SHORT, 1, 80),  // APEXBrightness -> "8.0"
            ent(0x0008, T_SHORT, 1, 60),  // ExternalSensorBrightnessValue
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert_eq!(tags.get("Leica:BaseISO").map(String::as_str), Some("200"));
        assert_eq!(
            tags.get("Leica:MeasuredLV").map(String::as_str),
            Some("7.5 EV")
        );
        assert_eq!(
            tags.get("Leica:ApproximateFNumber").map(String::as_str),
            Some("f/2.8")
        );
        assert_eq!(tags.get("Leica:FilmMode").map(String::as_str), Some("4"));
        assert_eq!(
            tags.get("Leica:FrameSelector").map(String::as_str),
            Some("11")
        );
        assert_eq!(
            tags.get("Leica:CameraPitchAngle").map(String::as_str),
            Some("12°")
        );
        assert_eq!(
            tags.get("Leica:CameraRollAngle").map(String::as_str),
            Some("3°")
        );
        assert_eq!(
            tags.get("Leica:FocalLength35mm").map(String::as_str),
            Some("50 mm")
        );
        assert_eq!(tags.get("Leica:ImageID").map(String::as_str), Some("7"));
        assert_eq!(
            tags.get("Leica:PictureControl").map(String::as_str),
            Some("2")
        );
        assert_eq!(tags.get("Leica:AFPoint").map(String::as_str), Some("9"));
        assert!(tags.contains_key("Leica:APEXBrightness"));
        assert!(tags.contains_key("Leica:ExternalSensorBrightnessValue"));
    }

    // -- DNG version & perspective-control "On" -----------------------------

    #[test]
    fn test_leica_dng_version_and_perspective_on() {
        let p = LeicaMakerNoteParser;
        // 1.4.0.0 packed: (1<<24)|(4<<16)|(0<<8)|0 = 0x01040000.
        let entries = [
            ent(0x0060, T_LONG, 1, 0x0104_0000), // DNGVersion -> "1.4.0.0"
            ent(0x0062, T_SHORT, 1, 1),          // PerspectiveControl -> On
        ];
        let data = leica_short(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Leica:DNGVersion").map(String::as_str),
            Some("1.4.0.0")
        );
        assert_eq!(
            tags.get("Leica:PerspectiveControl").map(String::as_str),
            Some("On")
        );
    }

    // MacroMode "Unknown" branch (value not 0/1).
    #[test]
    fn test_leica_macro_unknown() {
        let p = LeicaMakerNoteParser;
        let data = leica_short(&[ent(0x0070, T_SHORT, 1, 9)]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Leica:MacroMode").map(String::as_str),
            Some("Unknown")
        );
    }

    // -- Header variants: long header + headerless --------------------------

    #[test]
    fn test_leica_long_header_form() {
        let p = LeicaMakerNoteParser;
        let mut data = Vec::new();
        data.extend_from_slice(b"LEICA CAMERA AG"); // 15-byte header
        data.extend_from_slice(&ifd_le(&[ent(0x0003, T_SHORT, 1, 4)])); // Quality -> Super Fine
        data.extend_from_slice(&[0u8; 64]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Leica:Quality").map(String::as_str),
            Some("Super Fine")
        );
    }

    #[test]
    fn test_leica_headerless_form() {
        let p = LeicaMakerNoteParser;
        let data = leica_bare(&[ent(0x0006, T_SHORT, 1, 1)]); // WhiteBalance -> Daylight
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Leica:WhiteBalance").map(String::as_str),
            Some("Daylight")
        );
    }

    // -- Big-endian parsing -------------------------------------------------

    #[test]
    fn test_leica_big_endian() {
        let p = LeicaMakerNoteParser;
        let mut data = Vec::new();
        data.extend_from_slice(b"LEICA\0\0\0");
        data.extend_from_slice(&ifd_be(&[
            ent(0x0003, T_SHORT, 1, 1), // Quality -> Fine
            ent(0x0020, T_SHORT, 1, 0), // ExposureMode -> Manual
        ]));
        data.extend_from_slice(&[0u8; 64]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::BigEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Leica:Quality").map(String::as_str), Some("Fine"));
        assert_eq!(
            tags.get("Leica:ExposureMode").map(String::as_str),
            Some("Manual")
        );
    }

    // -- Unknown tag id is silently ignored (default match arm) -------------

    #[test]
    fn test_leica_unknown_tag_ignored() {
        let p = LeicaMakerNoteParser;
        let data = leica_short(&[ent(0xABCD, T_SHORT, 1, 1)]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(tags.is_empty());
    }

    // -- Error / malformed branches -----------------------------------------

    #[test]
    fn test_leica_error_too_short() {
        let p = LeicaMakerNoteParser;
        let mut tags = HashMap::new();
        // < 8 bytes -> "data too short" Err.
        assert!(
            p.parse(&[0x00, 0x01, 0x02], ByteOrder::LittleEndian, &mut tags)
                .is_err()
        );
    }

    #[test]
    fn test_leica_error_no_data_after_header() {
        let p = LeicaMakerNoteParser;
        // Exactly the 8-byte short header and nothing else: offset == len.
        let mut tags = HashMap::new();
        let res = p.parse(b"LEICA\0\0\0", ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_err());
    }

    #[test]
    fn test_leica_error_invalid_entry_count() {
        let p = LeicaMakerNoteParser;
        // Header + entry count of 0 -> "Invalid Leica IFD entry count".
        let mut zero = Vec::new();
        zero.extend_from_slice(b"LEICA\0\0\0");
        zero.extend_from_slice(&[0x00, 0x00]);
        zero.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        assert!(p.parse(&zero, ByteOrder::LittleEndian, &mut tags).is_err());

        // Huge entry count (> 200) -> also invalid.
        let mut huge = Vec::new();
        huge.extend_from_slice(b"LEICA\0\0\0");
        huge.extend_from_slice(&[0xFF, 0xFF]);
        huge.extend_from_slice(&[0u8; 16]);
        let mut tags2 = HashMap::new();
        assert!(p.parse(&huge, ByteOrder::LittleEndian, &mut tags2).is_err());
    }

    #[test]
    fn test_leica_error_insufficient_entry_data() {
        let p = LeicaMakerNoteParser;
        // Header + claims 5 entries but supplies far less than 2 + 5*12 bytes.
        let mut data = Vec::new();
        data.extend_from_slice(b"LEICA\0\0\0");
        data.extend_from_slice(&[0x05, 0x00]); // 5 entries
        data.extend_from_slice(&[0u8; 6]); // not enough for 60 bytes of entries
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_err());
    }
}

// ===========================================================================
// PENTAX
// ===========================================================================

mod pentax {
    use super::*;

    /// Build a full Pentax MakerNote with AOC header + 2-byte gap + IFD + tail.
    /// (AOC parsing skips 6 bytes total: "AOC\0" + 2.)
    fn pentax_aoc(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"AOC\0");
        out.extend_from_slice(&[0x4D, 0x4D]); // 2-byte filler
        out.extend_from_slice(&ifd_le(entries));
        out.extend_from_slice(&[0u8; 128]);
        out
    }

    /// Build a big-endian AOC Pentax MakerNote.
    fn pentax_aoc_be(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"AOC\0");
        out.extend_from_slice(&[0x4D, 0x4D]);
        out.extend_from_slice(&ifd_be(entries));
        out.extend_from_slice(&[0u8; 128]);
        out
    }

    // -- is_pentax_makernote: every branch ----------------------------------

    #[test]
    fn test_is_pentax_makernote_all_branches() {
        assert!(is_pentax_makernote(b"AOC\0xx"));
        assert!(is_pentax_makernote(b"PENTAX \0rest"));
        assert!(is_pentax_makernote(&[0x05, 0x00, 0, 0])); // headerless count 5
        assert!(!is_pentax_makernote(&[0x01, 0x02, 0x03])); // < 4 bytes
        assert!(!is_pentax_makernote(&[0xFF, 0xFF, 0, 0])); // count too large
        assert!(!is_pentax_makernote(&[0x00, 0x00, 0, 0])); // count 0
    }

    #[test]
    fn test_pentax_trait_basics() {
        let p = PentaxParser;
        assert_eq!(p.manufacturer_name(), "Pentax");
        assert_eq!(p.tag_prefix(), "Pentax:");
        assert!(p.validate_header(b"AOC\0\x00\x00"));
        // Known lens id 2 resolves to a name.
        assert_eq!(p.lookup_lens(2).as_deref(), Some("SMC Pentax-K 50mm f/1.4"));
        assert!(p.lookup_lens(0xFFFF).is_none());
    }

    // -- String tags (inline + offset) --------------------------------------

    #[test]
    fn test_pentax_inline_string_tags() {
        let p = PentaxParser;
        // Version 0x0000, ASCII inline "0100"; Date/Time also inline (<=4 bytes).
        let v0100 = u32::from_le_bytes([b'0', b'1', b'0', b'0']);
        let entries = [
            ent(0x0000, T_ASCII, 4, v0100), // Version
            ent(0x0027, T_ASCII, 4, v0100), // DSPFirmwareVersion
            ent(0x0028, T_ASCII, 4, v0100), // CPUFirmwareVersion
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Pentax:Version").map(String::as_str), Some("0100"));
        assert_eq!(
            tags.get("Pentax:DSPFirmwareVersion").map(String::as_str),
            Some("0100")
        );
        assert_eq!(
            tags.get("Pentax:CPUFirmwareVersion").map(String::as_str),
            Some("0100")
        );
    }

    // Offset-based string: a >4 byte ASCII string stored in the tail padding.
    // The AOC IFD offset is 6; extract_string_value computes abs = ifd_offset +
    // value_offset into `full_data`. We place the string at an absolute position
    // and set value_offset = abs - 6 so the read lands on it.
    #[test]
    fn test_pentax_offset_string_tag() {
        let p = PentaxParser;
        let text = b"PENTAX-LENS-9000\0"; // 17 bytes (> 4)
        // Build: AOC(4) + filler(2) + IFD[count(2)+entry(12)] = 20, then string.
        // ifd_offset = 6. String placed at absolute offset 20.
        let abs = 20u32;
        let value_offset = abs - 6; // == 14, relative to ifd_offset
        let entries = [ent(0x009F, T_ASCII, text.len() as u32, value_offset)]; // LensModel
        let mut data = Vec::new();
        data.extend_from_slice(b"AOC\0");
        data.extend_from_slice(&[0x4D, 0x4D]);
        data.extend_from_slice(&ifd_le(&entries));
        // At this point data.len() == 20. Append the string at absolute 20.
        assert_eq!(data.len(), abs as usize);
        data.extend_from_slice(text);
        data.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Pentax:LensModel").map(String::as_str),
            Some("PENTAX-LENS-9000")
        );
    }

    // -- Decoded enum tags --------------------------------------------------

    #[test]
    fn test_pentax_decoded_enum_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x0008, T_SHORT, 1, 6), // Quality -> RAW + JPEG
            ent(0x000B, T_SHORT, 1, 4), // PictureMode -> Portrait
            ent(0x000C, T_SHORT, 1, 3), // FlashMode -> Red-eye Reduction
            ent(0x000D, T_SHORT, 1, 4), // FocusMode -> AF-C (Continuous)
            ent(0x0017, T_SHORT, 1, 2), // MeteringMode -> Spot
            ent(0x0019, T_SHORT, 1, 4), // WhiteBalance -> Tungsten
            ent(0x001A, T_SHORT, 1, 2), // WhiteBalanceMode -> Auto (Shade)
            ent(0x001F, T_SHORT, 1, 5), // Saturation -> Very High
            ent(0x0020, T_SHORT, 1, 6), // Contrast -> Very Low
            ent(0x0021, T_SHORT, 1, 4), // Sharpness -> Med Hard
            ent(0x0034, T_SHORT, 1, 8), // DriveMode -> Continuous (Hi)
            ent(0x0037, T_SHORT, 1, 1), // ColorSpace -> Adobe RGB
            ent(0x0009, T_SHORT, 1, 9), // ImageSize -> 3072x2304
            ent(0x0018, T_SHORT, 1, 1), // AutoBracketing -> On
            ent(0x0022, T_SHORT, 1, 1), // WorldTimeLocation -> Destination
            ent(0x0086, T_SHORT, 1, 2), // PixelShiftResolution -> On (Motion)
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert_eq!(
            tags.get("Pentax:Quality").map(String::as_str),
            Some("RAW + JPEG")
        );
        assert_eq!(
            tags.get("Pentax:PictureMode").map(String::as_str),
            Some("Portrait")
        );
        assert_eq!(
            tags.get("Pentax:FlashMode").map(String::as_str),
            Some("Red-eye Reduction")
        );
        assert_eq!(
            tags.get("Pentax:FocusMode").map(String::as_str),
            Some("AF-C (Continuous)")
        );
        assert_eq!(
            tags.get("Pentax:MeteringMode").map(String::as_str),
            Some("Spot")
        );
        assert_eq!(
            tags.get("Pentax:WhiteBalance").map(String::as_str),
            Some("Tungsten")
        );
        assert!(tags.contains_key("Pentax:WhiteBalanceMode"));
        assert_eq!(
            tags.get("Pentax:Saturation").map(String::as_str),
            Some("Very High")
        );
        assert_eq!(
            tags.get("Pentax:Contrast").map(String::as_str),
            Some("Very Low")
        );
        assert_eq!(
            tags.get("Pentax:Sharpness").map(String::as_str),
            Some("Med Hard")
        );
        assert_eq!(
            tags.get("Pentax:DriveMode").map(String::as_str),
            Some("Continuous (Hi)")
        );
        assert_eq!(
            tags.get("Pentax:ColorSpace").map(String::as_str),
            Some("Adobe RGB")
        );
        assert!(tags.contains_key("Pentax:ImageSize"));
        assert_eq!(
            tags.get("Pentax:AutoBracketing").map(String::as_str),
            Some("On")
        );
        assert_eq!(
            tags.get("Pentax:WorldTimeLocation").map(String::as_str),
            Some("Destination")
        );
        assert!(tags.contains_key("Pentax:PixelShiftResolution"));
    }

    // -- AF points / ISO / balances / focal / zoom / shutter / lens type ----

    #[test]
    fn test_pentax_numeric_and_format_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x000E, T_SHORT, 1, 5),    // AFPointSelected
            ent(0x000F, T_SHORT, 1, 3),    // AFPointInFocus
            ent(0x0014, T_LONG, 1, 800),   // ISO
            ent(0x001B, T_SHORT, 1, 100),  // BlueBalance
            ent(0x001C, T_SHORT, 1, 110),  // RedBalance
            ent(0x001D, T_LONG, 1, 13500), // FocalLength -> "135.0 mm"
            ent(0x001E, T_LONG, 1, 200),   // DigitalZoom -> "2.00x"
            ent(0x005D, T_LONG, 1, 99999), // ShutterCount
            ent(0x0001, T_LONG, 1, 7),     // ModelType
            ent(0x0005, T_LONG, 1, 12),    // ModelID
            ent(0x0002, T_LONG, 1, 4096),  // PreviewImageSize
            ent(0x0003, T_LONG, 1, 8192),  // PreviewImageLength
            ent(0x0004, T_LONG, 1, 1024),  // PreviewImageStart
            ent(0x0033, T_SHORT, 1, 3),    // PictureMode2
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert_eq!(
            tags.get("Pentax:AFPointSelected").map(String::as_str),
            Some("5")
        );
        assert_eq!(
            tags.get("Pentax:AFPointInFocus").map(String::as_str),
            Some("3")
        );
        assert_eq!(tags.get("Pentax:ISO").map(String::as_str), Some("800"));
        assert_eq!(
            tags.get("Pentax:BlueBalance").map(String::as_str),
            Some("100")
        );
        assert_eq!(
            tags.get("Pentax:RedBalance").map(String::as_str),
            Some("110")
        );
        assert_eq!(
            tags.get("Pentax:FocalLength").map(String::as_str),
            Some("135.0 mm")
        );
        assert_eq!(
            tags.get("Pentax:DigitalZoom").map(String::as_str),
            Some("2.00x")
        );
        assert_eq!(
            tags.get("Pentax:ShutterCount").map(String::as_str),
            Some("99999")
        );
        assert_eq!(tags.get("Pentax:ModelType").map(String::as_str), Some("7"));
        assert_eq!(tags.get("Pentax:ModelID").map(String::as_str), Some("12"));
        assert!(tags.contains_key("Pentax:PreviewImageSize"));
        assert!(tags.contains_key("Pentax:PreviewImageLength"));
        assert!(tags.contains_key("Pentax:PreviewImageStart"));
        assert!(tags.contains_key("Pentax:PictureMode2"));
    }

    // DigitalZoom value 0 -> suppressed branch.
    #[test]
    fn test_pentax_digital_zoom_zero_suppressed() {
        let p = PentaxParser;
        let data = pentax_aoc(&[ent(0x001E, T_LONG, 1, 0)]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(!tags.contains_key("Pentax:DigitalZoom"));
    }

    // -- Lens type known + unknown ------------------------------------------

    #[test]
    fn test_pentax_lens_type_known() {
        let p = PentaxParser;
        let data = pentax_aoc(&[ent(0x003F, T_LONG, 1, 2)]); // known id 2
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Pentax:LensType").map(String::as_str),
            Some("SMC Pentax-K 50mm f/1.4")
        );
    }

    #[test]
    fn test_pentax_lens_type_unknown() {
        let p = PentaxParser;
        let data = pentax_aoc(&[ent(0x003F, T_LONG, 1, 0xFFFF)]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(
            tags.get("Pentax:LensType")
                .map(String::as_str)
                .unwrap()
                .contains("Unknown")
        );
    }

    // -- Focus/exposure formatting tags -------------------------------------

    #[test]
    fn test_pentax_focus_exposure_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x0010, T_LONG, 1, 50),   // FocusPosition
            ent(0x0012, T_LONG, 1, 1000), // ExposureTime
            ent(0x0013, T_LONG, 1, 28),   // FNumber -> "f/2.8"
            ent(0x0014, T_LONG, 1, 1600), // ISO
            ent(0x0015, T_SHORT, 1, 12),  // LightReading
            ent(0x0016, T_SHORT, 1, 10),  // ExposureCompensation -> "+1.0 EV"
            ent(0x0047, T_SHORT, 1, 30),  // CameraTemperature -> "30 C"
            ent(0x003B, T_LONG, 1, 85),   // BatteryLevel -> "85%"
            ent(0x0023, T_LONG, 1, 42),   // HometownCity
            ent(0x0024, T_LONG, 1, 7),    // DestinationCity
            ent(0x0029, T_LONG, 1, 314),  // FrameNumber
            ent(0x002D, T_SHORT, 1, 95),  // EffectiveLV -> "9.5"
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert_eq!(
            tags.get("Pentax:FocusPosition").map(String::as_str),
            Some("50")
        );
        assert_eq!(
            tags.get("Pentax:ExposureTime").map(String::as_str),
            Some("1000")
        );
        assert_eq!(
            tags.get("Pentax:FNumber").map(String::as_str),
            Some("f/2.8")
        );
        assert_eq!(
            tags.get("Pentax:LightReading").map(String::as_str),
            Some("12")
        );
        assert_eq!(
            tags.get("Pentax:ExposureCompensation").map(String::as_str),
            Some("+1.0 EV")
        );
        assert_eq!(
            tags.get("Pentax:CameraTemperature").map(String::as_str),
            Some("30 C")
        );
        assert_eq!(
            tags.get("Pentax:BatteryLevel").map(String::as_str),
            Some("85%")
        );
        assert!(tags.contains_key("Pentax:HometownCity"));
        assert!(tags.contains_key("Pentax:DestinationCity"));
        assert!(tags.contains_key("Pentax:FrameNumber"));
        assert!(tags.contains_key("Pentax:EffectiveLV"));
    }

    // -- DST decoders -------------------------------------------------------

    #[test]
    fn test_pentax_dst_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x0025, T_SHORT, 1, 1), // HometownDST -> Yes
            ent(0x0026, T_SHORT, 1, 0), // DestinationDST -> No
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Pentax:HometownDST").map(String::as_str),
            Some("Yes")
        );
        assert_eq!(
            tags.get("Pentax:DestinationDST").map(String::as_str),
            Some("No")
        );
    }

    // -- Camera settings numeric tags (0x0032..0x004F) ----------------------

    #[test]
    fn test_pentax_camera_settings_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x0032, T_LONG, 1, 1),    // ImageProcessing
            ent(0x0035, T_LONG, 1, 23),   // SensorSize
            ent(0x0038, T_LONG, 1, 4),    // ImageAreaOffset
            ent(0x0039, T_LONG, 1, 6000), // RawImageSize
            ent(0x003C, T_LONG, 1, 11),   // AFPointsInFocus2
            ent(0x003D, T_LONG, 1, 8),    // DataScaling
            ent(0x003E, T_LONG, 1, 2),    // PreviewImageBorders
            ent(0x0040, T_SHORT, 1, 1),   // SensitivityAdjust
            ent(0x0041, T_LONG, 1, 3),    // ImageEditCount
            ent(0x0048, T_SHORT, 1, 1),   // AELock -> On
            ent(0x0049, T_SHORT, 1, 2),   // NoiseReduction -> On
            ent(0x004D, T_SSHORT, 1, 5),  // FlashExposureComp -> "+0.5 EV"
            ent(0x004F, T_SHORT, 1, 5),   // ImageTone -> Monochrome
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert!(tags.contains_key("Pentax:ImageProcessing"));
        assert!(tags.contains_key("Pentax:SensorSize"));
        assert!(tags.contains_key("Pentax:ImageAreaOffset"));
        assert!(tags.contains_key("Pentax:RawImageSize"));
        assert!(tags.contains_key("Pentax:AFPointsInFocus2"));
        assert!(tags.contains_key("Pentax:DataScaling"));
        assert!(tags.contains_key("Pentax:PreviewImageBorders"));
        assert!(tags.contains_key("Pentax:SensitivityAdjust"));
        assert!(tags.contains_key("Pentax:ImageEditCount"));
        assert_eq!(tags.get("Pentax:AELock").map(String::as_str), Some("On"));
        assert_eq!(
            tags.get("Pentax:NoiseReduction").map(String::as_str),
            Some("On")
        );
        assert!(tags.contains_key("Pentax:FlashExposureComp"));
        assert_eq!(
            tags.get("Pentax:ImageTone").map(String::as_str),
            Some("Monochrome")
        );
    }

    // -- Color and processing tags (0x0050..0x006F) -------------------------

    #[test]
    fn test_pentax_color_processing_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x0050, T_LONG, 1, 5200), // ColorTemperature -> "5200K"
            ent(0x005C, T_SHORT, 1, 6),   // ShakeReduction -> On (5-axis)
            ent(0x0060, T_LONG, 1, 1),    // FaceInfo
            ent(0x0062, T_SHORT, 1, 3),   // RawDevelopmentProcess -> Ver. 3
            ent(0x0067, T_SSHORT, 1, 2),  // Hue
            ent(0x0068, T_LONG, 1, 7),    // AWBInfo
            ent(0x0069, T_SHORT, 1, 2),   // DynamicRangeExpansion -> Auto
            ent(0x006C, T_SSHORT, 1, 1),  // HighLowKeyAdj
            ent(0x006D, T_SSHORT, 1, 2),  // ContrastHighlight
            ent(0x006E, T_SSHORT, 1, 3),  // ContrastShadow
            ent(0x006F, T_SSHORT, 1, 4),  // ContrastHighlightShadowAdj
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert_eq!(
            tags.get("Pentax:ColorTemperature").map(String::as_str),
            Some("5200K")
        );
        assert_eq!(
            tags.get("Pentax:ShakeReduction").map(String::as_str),
            Some("On (5-axis)")
        );
        assert!(tags.contains_key("Pentax:FaceInfo"));
        assert_eq!(
            tags.get("Pentax:RawDevelopmentProcess").map(String::as_str),
            Some("Ver. 3")
        );
        assert!(tags.contains_key("Pentax:Hue"));
        assert!(tags.contains_key("Pentax:AWBInfo"));
        assert_eq!(
            tags.get("Pentax:DynamicRangeExpansion").map(String::as_str),
            Some("Auto")
        );
        assert!(tags.contains_key("Pentax:HighLowKeyAdj"));
        assert!(tags.contains_key("Pentax:ContrastHighlight"));
        assert!(tags.contains_key("Pentax:ContrastShadow"));
        assert!(tags.contains_key("Pentax:ContrastHighlightShadowAdj"));
    }

    // -- Advanced features tags (0x0070..0x009F) ----------------------------

    #[test]
    fn test_pentax_advanced_feature_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x0070, T_SHORT, 1, 1),    // FineSharpness -> On
            ent(0x0071, T_SHORT, 1, 4),    // HighISONoiseReduction -> Strong
            ent(0x0072, T_SSHORT, 1, 3),   // AFAdjustment
            ent(0x0073, T_SHORT, 1, 3),    // MonochromeFilterEffect -> Red
            ent(0x0074, T_SHORT, 1, 1),    // MonochromeToning -> Sepia
            ent(0x0076, T_SHORT, 1, 1),    // FaceDetect -> On
            ent(0x0077, T_LONG, 1, 64),    // FaceDetectFrameSize
            ent(0x0079, T_SHORT, 1, 2),    // ShadowCorrection -> On
            ent(0x007A, T_LONG, 1, 5),     // ISOAutoParameters
            ent(0x007B, T_SHORT, 1, 2),    // CrossProcess -> Preset 1
            ent(0x007D, T_SHORT, 1, 7),    // LensCorr -> Distortion + CA + PI
            ent(0x007E, T_LONG, 1, 16383), // WhiteLevel
            ent(0x007F, T_LONG, 1, 1),     // LensInfo
            ent(0x0080, T_LONG, 1, 2),     // AFInfo
            ent(0x0082, T_SHORT, 1, 2),    // AspectRatio -> 16:9
            ent(0x0085, T_SHORT, 1, 1),    // HDR -> HDR Auto
            ent(0x0086, T_SHORT, 1, 1),    // PixelShiftResolution -> On
            ent(0x0087, T_SHORT, 1, 1),    // ShutterType -> Electronic
            ent(0x0088, T_SHORT, 1, 1),    // NeutralDensityFilter -> On
            ent(0x008B, T_LONG, 1, 6400),  // ISO2
            ent(0x0092, T_LONG, 1, 1),     // IntervalShooting
            ent(0x0095, T_SHORT, 1, 1),    // SkinToneCorrection -> On (Type 1)
            ent(0x0096, T_SSHORT, 1, 3),   // ClarityControl -> High 3
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        assert_eq!(
            tags.get("Pentax:FineSharpness").map(String::as_str),
            Some("On")
        );
        assert_eq!(
            tags.get("Pentax:HighISONoiseReduction").map(String::as_str),
            Some("Strong")
        );
        assert!(tags.contains_key("Pentax:AFAdjustment"));
        assert_eq!(
            tags.get("Pentax:MonochromeFilterEffect")
                .map(String::as_str),
            Some("Red")
        );
        assert_eq!(
            tags.get("Pentax:MonochromeToning").map(String::as_str),
            Some("Sepia")
        );
        assert_eq!(
            tags.get("Pentax:FaceDetect").map(String::as_str),
            Some("On")
        );
        assert!(tags.contains_key("Pentax:FaceDetectFrameSize"));
        assert_eq!(
            tags.get("Pentax:ShadowCorrection").map(String::as_str),
            Some("On")
        );
        assert!(tags.contains_key("Pentax:ISOAutoParameters"));
        assert_eq!(
            tags.get("Pentax:CrossProcess").map(String::as_str),
            Some("Preset 1")
        );
        assert_eq!(
            tags.get("Pentax:LensCorr").map(String::as_str),
            Some("Distortion + CA + PI")
        );
        assert!(tags.contains_key("Pentax:WhiteLevel"));
        assert!(tags.contains_key("Pentax:LensInfo"));
        assert!(tags.contains_key("Pentax:AFInfo"));
        assert_eq!(
            tags.get("Pentax:AspectRatio").map(String::as_str),
            Some("16:9")
        );
        assert_eq!(tags.get("Pentax:HDR").map(String::as_str), Some("HDR Auto"));
        assert!(tags.contains_key("Pentax:PixelShiftResolution"));
        assert_eq!(
            tags.get("Pentax:ShutterType").map(String::as_str),
            Some("Electronic")
        );
        assert_eq!(
            tags.get("Pentax:NeutralDensityFilter").map(String::as_str),
            Some("On")
        );
        assert!(tags.contains_key("Pentax:ISO2"));
        assert!(tags.contains_key("Pentax:IntervalShooting"));
        assert_eq!(
            tags.get("Pentax:SkinToneCorrection").map(String::as_str),
            Some("On (Type 1)")
        );
        assert_eq!(
            tags.get("Pentax:ClarityControl").map(String::as_str),
            Some("High 3")
        );
    }

    // -- TimeInfo string tag ------------------------------------------------

    #[test]
    fn test_pentax_timeinfo_string() {
        let p = PentaxParser;
        // TimeInfo 0x006B as ASCII inline (<=4 bytes).
        let inline = u32::from_le_bytes([b'A', b'B', 0, 0]);
        let data = pentax_aoc(&[ent(0x006B, T_ASCII, 2, inline)]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(tags.contains_key("Pentax:TimeInfo"));
    }

    // -- extract_value_as_i32: BYTE / SBYTE / SSHORT field-type branches ----

    #[test]
    fn test_pentax_field_type_branches() {
        let p = PentaxParser;
        // BYTE (type 1): low byte taken in LE -> value 2.
        // SBYTE (type 6): low byte as i8.
        // SSHORT (type 8): low 16 bits as i16.
        let entries = [
            ent(0x0008, T_BYTE, 1, 2),   // Quality (BYTE) -> Best
            ent(0x000B, T_SBYTE, 1, 4),  // PictureMode (SBYTE) -> Portrait
            ent(0x001F, T_SSHORT, 1, 2), // Saturation (SSHORT) -> High
        ];
        let data = pentax_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Pentax:Quality").map(String::as_str), Some("Best"));
        assert_eq!(
            tags.get("Pentax:PictureMode").map(String::as_str),
            Some("Portrait")
        );
        assert_eq!(
            tags.get("Pentax:Saturation").map(String::as_str),
            Some("High")
        );
    }

    // -- PENTAX (space) header form -----------------------------------------

    #[test]
    fn test_pentax_pentax_header_form() {
        let p = PentaxParser;
        let mut data = Vec::new();
        data.extend_from_slice(b"PENTAX \0"); // 8-byte header
        data.extend_from_slice(&ifd_le(&[ent(0x0008, T_SHORT, 1, 2)])); // Quality -> Best
        data.extend_from_slice(&[0u8; 64]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Pentax:Quality").map(String::as_str), Some("Best"));
    }

    // -- Headerless IFD form ------------------------------------------------

    #[test]
    fn test_pentax_headerless_form() {
        let p = PentaxParser;
        let mut data = ifd_le(&[ent(0x0008, T_SHORT, 1, 4)]); // Quality -> RAW
        data.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Pentax:Quality").map(String::as_str), Some("RAW"));
    }

    // -- Big-endian AOC parsing ---------------------------------------------

    #[test]
    fn test_pentax_big_endian() {
        let p = PentaxParser;
        // In big-endian TIFF, SHORT/SSHORT values are stored left-aligned in the
        // 4-byte value field, so extract_u16_value uses `(offset >> 16)`. Encode
        // the short value into the high 16 bits accordingly.
        let entries = [
            ent(0x0008, T_SHORT, 1, 2 << 16),   // Quality -> Best
            ent(0x0019, T_SHORT, 1, 1 << 16),   // WhiteBalance -> Daylight
            ent(0x0047, T_SSHORT, 1, 25 << 16), // CameraTemperature
        ];
        let data = pentax_aoc_be(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::BigEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Pentax:Quality").map(String::as_str), Some("Best"));
        assert_eq!(
            tags.get("Pentax:WhiteBalance").map(String::as_str),
            Some("Daylight")
        );
        assert!(tags.contains_key("Pentax:CameraTemperature"));
    }

    // -- Unknown tag id ignored (default arm) -------------------------------

    #[test]
    fn test_pentax_unknown_tag_ignored() {
        let p = PentaxParser;
        let data = pentax_aoc(&[ent(0x7777, T_SHORT, 1, 1)]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(tags.is_empty());
    }

    // -- Error / malformed branches -----------------------------------------

    #[test]
    fn test_pentax_error_paths() {
        let p = PentaxParser;
        // Empty -> Ok, no tags.
        let mut tags = HashMap::new();
        assert!(p.parse(&[], ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(tags.is_empty());

        // AOC header but no IFD body (len <= ifd_offset + 2) -> Ok, no tags.
        let mut tags2 = HashMap::new();
        assert!(
            p.parse(b"AOC\0\x00\x00", ByteOrder::LittleEndian, &mut tags2)
                .is_ok()
        );

        // Entry count of zero -> Ok, no tags.
        let mut tags3 = HashMap::new();
        let zero = pentax_aoc(&[]);
        assert!(p.parse(&zero, ByteOrder::LittleEndian, &mut tags3).is_ok());

        // Entry count too large (> 200) -> Ok, no tags.
        let mut huge = Vec::new();
        huge.extend_from_slice(b"AOC\0");
        huge.extend_from_slice(&[0x4D, 0x4D]);
        huge.extend_from_slice(&[0xFF, 0xFF]); // 65535 entries
        huge.extend_from_slice(&[0u8; 32]);
        let mut tags4 = HashMap::new();
        assert!(p.parse(&huge, ByteOrder::LittleEndian, &mut tags4).is_ok());
        assert!(tags4.is_empty());
    }

    // Truncated IFD: claims entries but supplies too few bytes -> parse_ifd_entries
    // fails -> Ok with no tags (graceful).
    #[test]
    fn test_pentax_truncated_ifd_entries() {
        let p = PentaxParser;
        let mut data = Vec::new();
        data.extend_from_slice(b"AOC\0");
        data.extend_from_slice(&[0x4D, 0x4D]);
        data.extend_from_slice(&[0x05, 0x00]); // claims 5 entries
        data.extend_from_slice(&[0u8; 8]); // far fewer than 60 bytes
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(tags.is_empty());
    }
}
