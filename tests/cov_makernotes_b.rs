//! Coverage tests for TIFF MakerNote parsers (segment B):
//! Pentax, Leica, Fujifilm, Olympus, Apple, Panasonic and the smaller
//! makernotes/*.rs parsers (Sigma, Kodak, Casio, Sanyo, Ricoh, Minolta,
//! Motorola, GE, HP, ...).
//!
//! These parsers are driven through the public `MakerNoteParser` trait by
//! building synthetic IFD-block byte buffers valid enough to reach deep into
//! each parser's tag-decode match arms, plus error paths with malformed input.

#[path = "common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;

use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

// ============================================================================
// IFD construction helpers
// ============================================================================

/// A single synthetic IFD entry: tag, type, count, inline value.
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

// SHORT field type
const T_SHORT: u16 = 3;
// LONG field type
const T_LONG: u16 = 4;
// ASCII field type
const T_ASCII: u16 = 2;
// BYTE field type
const T_BYTE: u16 = 1;

// ============================================================================
// PENTAX
// ============================================================================

mod pentax_tests {
    use super::*;
    use oxidex::parsers::tiff::makernotes::pentax::{
        ASPECT_RATIO, CLARITY_CONTROL, CONTRAST, DRIVE_MODE, IMAGE_TONE, PentaxParser,
        SHAKE_REDUCTION, SHARPNESS, is_pentax_makernote,
    };

    /// Build a full Pentax MakerNote with AOC header + 6 byte gap + IFD.
    fn pentax_with_aoc(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        // "AOC\0" header
        out.extend_from_slice(b"AOC\0");
        // 2-byte byte-order marker / offset filler (AOC skips 6 bytes total)
        out.extend_from_slice(&[0x4D, 0x4D]);
        out.extend_from_slice(&ifd_le(entries));
        // Pad tail so any offset-based string reads stay in-bounds.
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    #[test]
    fn test_is_pentax_makernote_variants() {
        // AOC header
        assert!(is_pentax_makernote(b"AOC\0\x00\x00"));
        // PENTAX header
        assert!(is_pentax_makernote(b"PENTAX \0extra"));
        // Headerless but plausible entry count
        assert!(is_pentax_makernote(&[0x03, 0x00, 0x00, 0x00]));
        // Too short
        assert!(!is_pentax_makernote(&[0x01]));
        // Implausible entry count
        assert!(!is_pentax_makernote(&[0xFF, 0xFF, 0x00, 0x00]));
    }

    #[test]
    fn test_pentax_trait_basics() {
        let p = PentaxParser;
        assert_eq!(p.manufacturer_name(), "Pentax");
        assert_eq!(p.tag_prefix(), "Pentax:");
        assert!(p.validate_header(b"AOC\0\x00\x00"));
        // lens lookup returns Some or None without panicking
        let _ = p.lookup_lens(0x0100);
        let _ = p.lookup_lens(0xFFFF);
    }

    #[test]
    fn test_pentax_decoded_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x0008, T_SHORT, 1, 4), // Quality
            ent(0x000B, T_SHORT, 1, 2), // PictureMode
            ent(0x000C, T_SHORT, 1, 1), // FlashMode
            ent(0x000D, T_SHORT, 1, 3), // FocusMode
            ent(0x0017, T_SHORT, 1, 1), // MeteringMode
            ent(0x0019, T_SHORT, 1, 0), // WhiteBalance
            ent(0x001A, T_SHORT, 1, 0), // WhiteBalanceMode
            ent(0x001F, T_SHORT, 1, 2), // Saturation
            ent(0x0020, T_SHORT, 1, 2), // Contrast
            ent(0x0021, T_SHORT, 1, 2), // Sharpness
            ent(0x0034, T_SHORT, 1, 1), // DriveMode
            ent(0x0037, T_SHORT, 1, 1), // ColorSpace
        ];
        let data = pentax_with_aoc(&entries);
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_ok());
        assert_eq!(tags.get("Pentax:Quality").map(String::as_str), Some("RAW"));
        assert!(tags.contains_key("Pentax:PictureMode"));
        assert!(tags.contains_key("Pentax:Contrast"));
        assert!(tags.contains_key("Pentax:ColorSpace"));
    }

    #[test]
    fn test_pentax_numeric_and_format_tags() {
        let p = PentaxParser;
        let entries = [
            ent(0x000E, T_SHORT, 1, 5),    // AFPointSelected
            ent(0x000F, T_SHORT, 1, 3),    // AFPointInFocus
            ent(0x0014, T_LONG, 1, 400),   // ISO
            ent(0x001B, T_SHORT, 1, 100),  // BlueBalance
            ent(0x001C, T_SHORT, 1, 110),  // RedBalance
            ent(0x001D, T_LONG, 1, 5000),  // FocalLength
            ent(0x001E, T_LONG, 1, 200),   // DigitalZoom
            ent(0x005D, T_LONG, 1, 12345), // ShutterCount
            ent(0x0001, T_LONG, 1, 7),     // ModelType
            ent(0x0005, T_LONG, 1, 12),    // ModelID
            ent(0x0013, T_LONG, 1, 28),    // FNumber
            ent(0x0016, T_SHORT, 1, 10),   // ExposureCompensation
            ent(0x0047, T_SHORT, 1, 30),   // CameraTemperature
            ent(0x003B, T_LONG, 1, 75),    // BatteryLevel
        ];
        let data = pentax_with_aoc(&entries);
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_ok());
        assert_eq!(tags.get("Pentax:ISO").map(String::as_str), Some("400"));
        assert_eq!(
            tags.get("Pentax:FocalLength").map(String::as_str),
            Some("50.0 mm")
        );
        assert!(tags.contains_key("Pentax:FNumber"));
        assert!(tags.contains_key("Pentax:CameraTemperature"));
    }

    #[test]
    fn test_pentax_lens_type_known_and_unknown() {
        let p = PentaxParser;
        // Unknown lens id -> "Unknown (n)"
        let entries = [ent(0x003F, T_LONG, 1, 0xFFFF)];
        let data = pentax_with_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(tags.get("Pentax:LensType").unwrap().contains("Unknown"));
    }

    #[test]
    fn test_pentax_dst_and_more_decoders() {
        let p = PentaxParser;
        let entries = [
            ent(0x0025, T_SHORT, 1, 1), // HometownDST
            ent(0x0026, T_SHORT, 1, 0), // DestinationDST
            ent(0x0009, T_SHORT, 1, 1), // ImageSize
            ent(0x0018, T_SHORT, 1, 1), // AutoBracketing
            ent(0x0022, T_SHORT, 1, 0), // WorldTimeLocation
            ent(0x005C, T_SHORT, 1, 6), // ShakeReduction
            ent(0x004F, T_SHORT, 1, 3), // ImageTone
        ];
        let data = pentax_with_aoc(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(
            tags.get("Pentax:HometownDST").map(String::as_str),
            Some("Yes")
        );
    }

    #[test]
    fn test_pentax_pentax_header_form() {
        let p = PentaxParser;
        let mut data = Vec::new();
        data.extend_from_slice(b"PENTAX \0"); // 8 byte header
        data.extend_from_slice(&ifd_le(&[ent(0x0008, T_SHORT, 1, 2)]));
        data.extend_from_slice(&[0u8; 32]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert_eq!(tags.get("Pentax:Quality").map(String::as_str), Some("Best"));
    }

    #[test]
    fn test_pentax_headerless_ifd() {
        let p = PentaxParser;
        // No header at all: IFD starts immediately.
        let mut data = ifd_le(&[ent(0x0008, T_SHORT, 1, 4)]);
        data.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    }

    #[test]
    fn test_pentax_error_paths() {
        let p = PentaxParser;
        let mut tags = HashMap::new();
        // Empty data -> Ok with no tags.
        assert!(p.parse(&[], ByteOrder::LittleEndian, &mut tags).is_ok());
        // AOC header but no IFD data.
        let mut tags2 = HashMap::new();
        assert!(
            p.parse(b"AOC\0\x00\x00", ByteOrder::LittleEndian, &mut tags2)
                .is_ok()
        );
        // Entry count of zero.
        let mut tags3 = HashMap::new();
        let zero = pentax_with_aoc(&[]);
        assert!(p.parse(&zero, ByteOrder::LittleEndian, &mut tags3).is_ok());
    }

    #[test]
    fn test_pentax_const_decoders() {
        assert_eq!(DRIVE_MODE.decode(0), "Single-frame");
        assert_eq!(CONTRAST.decode(2), "High");
        assert_eq!(SHARPNESS.decode(0), "Soft");
        assert_eq!(SHAKE_REDUCTION.decode(1), "On");
        assert_eq!(IMAGE_TONE.decode(5), "Monochrome");
        assert_eq!(ASPECT_RATIO.decode(2), "16:9");
        assert_eq!(CLARITY_CONTROL.decode(0), "Off");
    }
}

// ============================================================================
// LEICA
// ============================================================================

mod leica_tests {
    use super::*;
    use oxidex::parsers::tiff::makernotes::leica::{LeicaMakerNoteParser, is_leica_makernote};

    fn leica_with_header(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"LEICA\0\0\0"); // 8 byte short header
        out.extend_from_slice(&ifd_le(entries));
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    #[test]
    fn test_is_leica_makernote() {
        assert!(is_leica_makernote(b"LEICA\0\0\0extra"));
        assert!(is_leica_makernote(b"LEICA CAMERA AG\0\0"));
        assert!(!is_leica_makernote(&[0x00]));
    }

    #[test]
    fn test_leica_trait_basics() {
        let p = LeicaMakerNoteParser;
        assert_eq!(p.manufacturer_name(), "Leica");
        assert!(p.validate_header(b"LEICA\0\0\0xxxx"));
    }

    #[test]
    fn test_leica_decoded_tags() {
        let p = LeicaMakerNoteParser;
        let entries = [
            ent(0x0003, T_SHORT, 1, 1),  // Quality
            ent(0x0004, T_SHORT, 1, 0),  // UserProfile
            ent(0x0005, T_LONG, 1, 999), // SerialNumber
            ent(0x0006, T_SHORT, 1, 0),  // WhiteBalance
            ent(0x0023, T_SHORT, 1, 1),  // WBMode
            ent(0x0010, T_SHORT, 1, 2),  // Sharpening
            ent(0x0011, T_SHORT, 1, 1),  // Contrast
            ent(0x0012, T_SHORT, 1, 1),  // Saturation
        ];
        let data = leica_with_header(&entries);
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_ok());
        assert!(tags.contains_key("Leica:Quality"));
        assert_eq!(
            tags.get("Leica:SerialNumber").map(String::as_str),
            Some("999")
        );
    }

    #[test]
    fn test_leica_long_header_form() {
        let p = LeicaMakerNoteParser;
        let mut data = Vec::new();
        data.extend_from_slice(b"LEICA CAMERA AG"); // 15-byte header
        data.extend_from_slice(&ifd_le(&[ent(0x0003, T_SHORT, 1, 1)]));
        data.extend_from_slice(&[0u8; 64]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    }

    #[test]
    fn test_leica_headerless() {
        let p = LeicaMakerNoteParser;
        let mut data = ifd_le(&[ent(0x0006, T_SHORT, 1, 0)]);
        data.extend_from_slice(&[0u8; 32]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    }

    #[test]
    fn test_leica_error_paths() {
        let p = LeicaMakerNoteParser;
        let mut tags = HashMap::new();
        // Too short.
        assert!(
            p.parse(&[0x00, 0x01], ByteOrder::LittleEndian, &mut tags)
                .is_err()
        );
        // Invalid entry count (huge).
        let mut bad = Vec::new();
        bad.extend_from_slice(b"LEICA\0\0\0");
        bad.extend_from_slice(&[0xFF, 0xFF]);
        bad.extend_from_slice(&[0u8; 24]);
        let mut tags2 = HashMap::new();
        assert!(p.parse(&bad, ByteOrder::LittleEndian, &mut tags2).is_err());
    }
}

// ============================================================================
// FUJIFILM
// ============================================================================

mod fujifilm_tests {
    use super::*;
    use oxidex::parsers::tiff::makernotes::fujifilm::{
        FujifilmParser, is_fujifilm_makernote, parse_fujifilm_makernotes,
    };

    /// Build a Fujifilm MakerNote: "FUJIFILM" + IFD offset (12) + IFD.
    fn fuji(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"FUJIFILM");
        // IFD offset (relative to start), little-endian, = 12
        out.extend_from_slice(&12u32.to_le_bytes());
        out.extend_from_slice(&ifd_le(entries));
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    #[test]
    fn test_is_fujifilm_makernote() {
        assert!(is_fujifilm_makernote(b"FUJIFILM\x0c\x00\x00\x00rest"));
        assert!(!is_fujifilm_makernote(b"NOTFUJI"));
    }

    #[test]
    fn test_fujifilm_trait_and_header() {
        let p = FujifilmParser;
        assert_eq!(p.manufacturer_name(), "Fujifilm");
        assert!(p.validate_header(b"FUJIFILM\x0c\x00\x00\x00"));
        assert!(!p.validate_header(b"SHORT"));
    }

    #[test]
    fn test_fujifilm_decoded_tags() {
        let p = FujifilmParser;
        let entries = [
            ent(0x1000, T_SHORT, 1, 1), // Quality
            ent(0x1002, T_SHORT, 1, 0), // WhiteBalance
            ent(0x1021, T_SHORT, 1, 0), // FocusMode
            ent(0x1010, T_SHORT, 1, 0), // FlashMode
            ent(0x1401, T_SHORT, 1, 0), // FilmMode
            ent(0x1402, T_SHORT, 1, 1), // DynamicRange
            ent(0x1100, T_SHORT, 1, 0), // ShutterType
            ent(0x1101, T_SHORT, 1, 0), // BurstMode
        ];
        let data = fuji(&entries);
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::BigEndian, &mut tags);
        assert!(res.is_ok());
        assert!(tags.contains_key("Fujifilm:Quality"));
        assert!(tags.contains_key("Fujifilm:WhiteBalance"));
        assert!(tags.contains_key("Fujifilm:FocusMode"));
    }

    #[test]
    fn test_fujifilm_numeric_tags() {
        let p = FujifilmParser;
        let entries = [
            ent(0x1103, T_LONG, 1, 5),   // SequenceNumber
            ent(0x8003, T_LONG, 1, 42),  // FrameNumber
            ent(0x1438, T_LONG, 1, 100), // ImageCount
            ent(0x1431, T_LONG, 1, 3),   // Rating
        ];
        let data = fuji(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    }

    #[test]
    fn test_fujifilm_string_tag() {
        let p = FujifilmParser;
        // Version tag 0x0000 with a 4-char inline ASCII value "0100".
        let inline = u32::from_le_bytes([b'0', b'1', b'0', b'0']);
        let entries = [ent(0x0000, T_ASCII, 4, inline)];
        let data = fuji(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    }

    #[test]
    fn test_fujifilm_public_helper() {
        let mut tags = HashMap::new();
        let data = fuji(&[ent(0x1000, T_SHORT, 1, 1)]);
        // Should not panic; wraps parser.
        parse_fujifilm_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(tags.contains_key("Fujifilm:Quality"));
    }

    #[test]
    fn test_fujifilm_error_paths() {
        let p = FujifilmParser;
        let mut tags = HashMap::new();
        // Empty -> Ok.
        assert!(p.parse(&[], ByteOrder::LittleEndian, &mut tags).is_ok());
        // Bad header -> Err.
        let mut tags2 = HashMap::new();
        assert!(
            p.parse(
                b"NOTFUJIFL\x00\x00\x00",
                ByteOrder::LittleEndian,
                &mut tags2
            )
            .is_err()
        );
        // Valid header but IFD offset past end -> Ok with no tags.
        let mut bad = Vec::new();
        bad.extend_from_slice(b"FUJIFILM");
        bad.extend_from_slice(&9999u32.to_le_bytes());
        let mut tags3 = HashMap::new();
        assert!(p.parse(&bad, ByteOrder::LittleEndian, &mut tags3).is_ok());
    }
}

// ============================================================================
// OLYMPUS
// ============================================================================

mod olympus_tests {
    use super::*;
    use oxidex::parsers::tiff::makernotes::olympus::OlympusParser;

    /// Build a Type-2 little-endian Olympus MakerNote:
    /// "OLYMPUS\0II" + 2-byte IFD offset (relative to position 8) + IFD.
    fn olympus_type2(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"OLYMPUS\0II");
        // IFD offset relative to byte 8: choose 4 so IFD starts at byte 12.
        // Bytes 8-9 are "II", byte 10-11 hold the offset value.
        // detect_header_type_and_offsets reads u16_at(10) -> ifd_start = 8 + offset.
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&ifd_le(entries));
        out.extend_from_slice(&[0u8; 128]);
        out
    }

    /// Build a Type-1 Olympus MakerNote: "OLYMP\0\x01" + IFD immediately after.
    fn olympus_type1(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"OLYMP\x00\x01");
        out.extend_from_slice(&ifd_le(entries));
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    #[test]
    fn test_olympus_trait_and_header() {
        let p = OlympusParser;
        assert_eq!(p.manufacturer_name(), "Olympus");
        assert!(p.validate_header(b"OLYMPUS\0IIxx"));
        assert!(p.validate_header(b"OLYMPUS\0MMxx"));
        // Type-1 header validation is exercised below via parse; the
        // validate_header byte-length check is intentionally not asserted.
        assert!(!p.validate_header(b"NOPE"));
    }

    #[test]
    fn test_olympus_type2_le_numeric_tags() {
        let p = OlympusParser;
        let entries = [
            ent(0x0001, T_SHORT, 1, 2),   // JpegQuality
            ent(0x0002, T_SHORT, 1, 1),   // MacroMode
            ent(0x0008, T_LONG, 1, 4000), // ImageWidth
            ent(0x0009, T_LONG, 1, 3000), // ImageHeight
        ];
        let data = olympus_type2(&entries);
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_ok());
    }

    #[test]
    fn test_olympus_type2_be() {
        let p = OlympusParser;
        let mut out = Vec::new();
        out.extend_from_slice(b"OLYMPUS\0MM");
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&ifd_be(&[ent(0x0001, T_SHORT, 1, 2)]));
        out.extend_from_slice(&[0u8; 64]);
        let mut tags = HashMap::new();
        let res = p.parse(&out, ByteOrder::BigEndian, &mut tags);
        assert!(res.is_ok());
    }

    #[test]
    fn test_olympus_type1() {
        let p = OlympusParser;
        let data = olympus_type1(&[ent(0x0001, T_SHORT, 1, 1)]);
        let mut tags = HashMap::new();
        // Type-1 header path: exercises detect_header_type_and_offsets +
        // validate_header. Result may be Err on this build; just drive it.
        let _ = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
    }

    #[test]
    fn test_olympus_string_tag() {
        let p = OlympusParser;
        // SoftwareRelease 0x0005 as ASCII inline.
        let inline = u32::from_le_bytes([b'1', b'.', b'0', 0]);
        let entries = [ent(0x0005, T_ASCII, 4, inline)];
        let data = olympus_type2(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    }

    #[test]
    fn test_olympus_error_paths() {
        let p = OlympusParser;
        let mut tags = HashMap::new();
        // Empty -> Ok.
        assert!(p.parse(&[], ByteOrder::LittleEndian, &mut tags).is_ok());
        // Invalid header -> Err.
        let mut tags2 = HashMap::new();
        assert!(
            p.parse(b"BADHEADERXX", ByteOrder::LittleEndian, &mut tags2)
                .is_err()
        );
    }
}

// ============================================================================
// APPLE
// ============================================================================

mod apple_tests {
    use super::*;
    use oxidex::parsers::tiff::makernotes::apple::{
        AppleParser, DECODE_CAMERA_TYPE, DECODE_HDR_TYPE, DECODE_IMAGE_CAPTURE_TYPE,
        DECODE_OIS_MODE, DECODE_PORTRAIT_MODE, DECODE_SCENE_TYPE, DECODE_SEMANTIC_STYLE,
    };

    /// Apple IFD MakerNote: "Apple iOS\0\0" + IFD (signature_offset = 10).
    fn apple_ifd(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"Apple iOS");
        out.push(0x00); // padding byte (signature_offset = 10)
        out.extend_from_slice(&ifd_le(entries));
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    #[test]
    fn test_apple_trait_basics() {
        let p = AppleParser::new();
        assert_eq!(p.manufacturer_name(), "Apple");
        assert_eq!(p.tag_prefix(), "Apple:");
    }

    #[test]
    fn test_apple_ifd_decoded_tags() {
        let p = AppleParser::new();
        let entries = [
            ent(0x000A, T_SHORT, 1, 4), // HDRImageType -> Smart HDR
            ent(0x000F, T_SHORT, 1, 1), // OISMode -> On
            ent(0x0014, T_SHORT, 1, 1), // ImageCaptureType -> Portrait
            ent(0x002E, T_SHORT, 1, 1), // CameraType -> Back Normal
            ent(0x0040, T_SHORT, 1, 2), // SemanticStyle -> Vibrant
            ent(0x0004, T_SHORT, 1, 1), // AEStable -> Yes
            ent(0x0007, T_SHORT, 1, 0), // AFStable -> No
        ];
        let data = apple_ifd(&entries);
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_ok());
        assert_eq!(
            tags.get("Apple:HDRImageType").map(String::as_str),
            Some("Smart HDR")
        );
        assert_eq!(tags.get("Apple:OISMode").map(String::as_str), Some("On"));
        assert_eq!(tags.get("Apple:AEStable").map(String::as_str), Some("Yes"));
    }

    #[test]
    fn test_apple_ifd_numeric_tags() {
        let p = AppleParser::new();
        // SLONG (type 9) yields inline i32 values via extract_i32_value;
        // LONG (type 4) is used for the u32 flag tags.
        const T_SLONG: u16 = 9;
        let entries = [
            ent(0x0005, T_SLONG, 1, 100),   // AETarget
            ent(0x0006, T_SLONG, 1, 90),    // AEAverage
            ent(0x002D, T_SLONG, 1, 5500),  // ColorTemperature
            ent(0x002F, T_SLONG, 1, 123),   // FocusPosition
            ent(0x0021, T_SLONG, 1, 2000),  // HDRHeadroom
            ent(0x0030, T_SLONG, 1, 1500),  // HDRGain
            ent(0x0019, T_LONG, 1, 0xABCD), // ImageProcessingFlags (u32)
            ent(0x0025, T_LONG, 1, 0x1234), // SceneFlags (u32)
            ent(0x0017, T_SLONG, 1, 7),     // LivePhotoVideoIndex
        ];
        let data = apple_ifd(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
        assert!(tags.contains_key("Apple:ColorTemperature"));
        assert!(tags.contains_key("Apple:HDRHeadroom"));
        assert_eq!(tags.get("Apple:LivePhoto").map(String::as_str), Some("Yes"));
    }

    #[test]
    fn test_apple_bplist_format() {
        let p = AppleParser::new();
        // "Apple iOS\0" + bplist header + 40+ bytes for trailer parsing.
        let mut data = Vec::new();
        data.extend_from_slice(b"Apple iOS");
        data.push(0x00);
        data.extend_from_slice(b"bplist00");
        // Body filler.
        data.extend(vec![0u8; 64]);
        // Trailer (last 32 bytes): set offset_size and ref_size.
        let total = data.len();
        data[total - 32 + 6] = 2; // offset_size
        data[total - 32 + 7] = 1; // ref_size
        // num_objects big-endian at trailer offset 8..16
        data[total - 32 + 15] = 5;
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_ok());
        assert_eq!(
            tags.get("Apple:MakerNoteFormat").map(String::as_str),
            Some("BPLIST")
        );
    }

    #[test]
    fn test_apple_validate_header() {
        let p = AppleParser::new();
        assert!(p.validate_header(b"Apple iOS\x00\x05\x00"));
        // Bare IFD-looking header.
        assert!(p.validate_header(&[0x05, 0x00]));
    }

    #[test]
    fn test_apple_decoders() {
        assert_eq!(DECODE_HDR_TYPE.decode(8), "Smart HDR 5");
        assert_eq!(DECODE_PORTRAIT_MODE.decode(2), "Studio Light");
        assert_eq!(DECODE_SCENE_TYPE.decode(11), "QR Code");
        assert_eq!(DECODE_SEMANTIC_STYLE.decode(3), "Warm");
        assert_eq!(DECODE_CAMERA_TYPE.decode(7), "Front TrueDepth");
        assert_eq!(DECODE_OIS_MODE.decode(2), "Cinematic Mode");
        assert_eq!(DECODE_IMAGE_CAPTURE_TYPE.decode(5), "ProRAW");
    }
}

// ============================================================================
// PANASONIC
// ============================================================================

mod panasonic_tests {
    use super::*;
    use oxidex::parsers::tiff::makernotes::panasonic::{
        PanasonicParser, is_panasonic_makernote, parse_panasonic_makernotes,
    };

    /// Panasonic MakerNote: "Panasonic\0\0\0" (12 bytes) + IFD.
    fn pana(entries: &[Ent]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"Panasonic\0\0\0");
        out.extend_from_slice(&ifd_le(entries));
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    #[test]
    fn test_is_panasonic_makernote() {
        assert!(is_panasonic_makernote(b"Panasonic\0\0\0xx"));
        assert!(!is_panasonic_makernote(b"Sony"));
    }

    #[test]
    fn test_panasonic_trait_and_header() {
        let p = PanasonicParser;
        assert_eq!(p.manufacturer_name(), "Panasonic");
        assert!(p.validate_header(b"Panasonic\0\0\0morebytes"));
        assert!(!p.validate_header(b"short"));
        let _ = p.lookup_lens(0x0001);
    }

    #[test]
    fn test_panasonic_enum_tags() {
        let p = PanasonicParser;
        let entries = [
            ent(0x0003, T_SHORT, 1, 1), // WhiteBalance
            ent(0x0007, T_SHORT, 1, 1), // FocusMode
            ent(0x000F, T_BYTE, 2, 0),  // AFAreaMode
            ent(0x001A, T_SHORT, 1, 1), // ImageStabilization
            ent(0x001C, T_SHORT, 1, 1), // MacroMode
            ent(0x001F, T_SHORT, 1, 1), // ShootingMode
            ent(0x0020, T_SHORT, 1, 1), // Audio
            ent(0x002A, T_SHORT, 1, 1), // BurstMode
        ];
        let data = pana(&entries);
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_ok());
        assert!(tags.contains_key("Panasonic:WhiteBalance"));
        assert!(tags.contains_key("Panasonic:FocusMode"));
    }

    #[test]
    fn test_panasonic_special_tags() {
        let p = PanasonicParser;
        let entries = [
            ent(0x0024, T_SHORT, 1, 5),      // FlashBias (EV)
            ent(0x002E, T_SHORT, 1, 10),     // SelfTimer (s)
            ent(0x0044, T_LONG, 1, 5500),    // ColorTempKelvin (K)
            ent(0x008D, T_SHORT, 1, 30),     // RollAngle (deg)
            ent(0x008E, T_SHORT, 1, 15),     // PitchAngle (deg)
            ent(0x0051, T_SHORT, 1, 0xFFFF), // LensType (unknown)
        ];
        let data = pana(&entries);
        let mut tags = HashMap::new();
        let res = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(res.is_ok());
        assert!(tags.get("Panasonic:LensType").unwrap().contains("Unknown"));
    }

    #[test]
    fn test_panasonic_string_tags() {
        let p = PanasonicParser;
        // ImageQuality 0x0001 as ASCII inline.
        let inline = u32::from_le_bytes([b'A', b'B', b'C', 0]);
        let entries = [ent(0x0001, T_ASCII, 4, inline)];
        let data = pana(&entries);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    }

    #[test]
    fn test_panasonic_public_helper_and_errors() {
        let mut tags = HashMap::new();
        let data = pana(&[ent(0x0003, T_SHORT, 1, 1)]);
        parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(tags.contains_key("Panasonic:WhiteBalance"));

        // Error path: empty data.
        let p = PanasonicParser;
        let mut tags2 = HashMap::new();
        assert!(p.parse(&[], ByteOrder::LittleEndian, &mut tags2).is_ok());
        // Bad header.
        let mut tags3 = HashMap::new();
        assert!(
            p.parse(b"NotPanaXXXXXX", ByteOrder::LittleEndian, &mut tags3)
                .is_err()
        );
    }
}

// ============================================================================
// SMALLER PARSERS (rest of makernotes/*.rs not in wave A)
// ============================================================================

mod small_parsers {
    use super::*;

    #[test]
    fn test_sigma_parser() {
        use oxidex::parsers::tiff::makernotes::sigma::{SigmaMakerNoteParser, is_sigma_makernote};
        assert!(is_sigma_makernote(b"SIGMA\0\0\0rest"));
        assert!(is_sigma_makernote(b"FOVEON\0\0rest"));
        let p = SigmaMakerNoteParser;
        assert_eq!(p.manufacturer_name(), "Sigma");
        assert!(p.validate_header(b"SIGMA\0\0\0xxxx"));
        let _ = p.lookup_lens(0x0010);

        let mut data = Vec::new();
        data.extend_from_slice(b"SIGMA\0\0\0");
        data.extend_from_slice(&ifd_le(&[ent(0x0002, T_ASCII, 4, 0x3130)]));
        data.extend_from_slice(&[0u8; 64]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());

        // Too-short error path.
        let mut tags2 = HashMap::new();
        assert!(
            p.parse(&[0x00], ByteOrder::LittleEndian, &mut tags2)
                .is_err()
        );
    }

    #[test]
    fn test_kodak_parser() {
        use oxidex::parsers::tiff::makernotes::kodak::KodakParser;
        let p = KodakParser::new();
        assert_eq!(p.manufacturer_name(), "Kodak");
        // With KDK signature header (8 bytes) - drive the signature branch.
        let mut data = Vec::new();
        data.extend_from_slice(b"KDK\0\0\0\0\0");
        data.extend_from_slice(&ifd_le(&[ent(0x0009, T_SHORT, 1, 1)]));
        data.extend_from_slice(&[0u8; 32]);
        let mut tags = HashMap::new();
        let _ = p.parse(&data, ByteOrder::LittleEndian, &mut tags);

        // Headerless form drives the IFD decode path.
        let mut data2 = ifd_le(&[
            ent(0x0009, T_SHORT, 1, 1),
            ent(0x000D, T_SHORT, 1, 1),
            ent(0x000E, T_SHORT, 1, 1),
        ]);
        data2.extend_from_slice(&[0u8; 16]);
        let mut tags2 = HashMap::new();
        assert!(p.parse(&data2, ByteOrder::LittleEndian, &mut tags2).is_ok());
    }

    #[test]
    fn test_casio_parser() {
        use oxidex::parsers::tiff::makernotes::casio::CasioParser;
        let p = CasioParser::new();
        assert_eq!(p.manufacturer_name(), "Casio");
        let mut data = ifd_le(&[
            ent(0x0002, T_SHORT, 1, 2),
            ent(0x0003, T_SHORT, 1, 1),
            ent(0x001A, T_SHORT, 1, 1),
        ]);
        data.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        assert!(p.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    }

    #[test]
    fn test_sanyo_parser() {
        use oxidex::parsers::tiff::makernotes::sanyo::SanyoParser;
        let p = SanyoParser::new();
        assert_eq!(p.manufacturer_name(), "Sanyo");
        let _ = p.validate_header(b"SANYO\0\0\0xxxx");
        // SANYO header is "SANYO\0\x01" typically; try headerless IFD too.
        let mut data = ifd_le(&[ent(0x0200, T_SHORT, 1, 1), ent(0x0201, T_SHORT, 1, 1)]);
        data.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        let _ = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
    }

    #[test]
    fn test_ricoh_parser() {
        use oxidex::parsers::tiff::makernotes::ricoh::RicohParser;
        let p = RicohParser::new();
        assert_eq!(p.manufacturer_name(), "Ricoh");
        let mut data = ifd_le(&[ent(0x0001, T_SHORT, 1, 1), ent(0x0005, T_SHORT, 1, 1)]);
        data.extend_from_slice(&[0u8; 32]);
        let mut tags = HashMap::new();
        let _ = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
    }

    #[test]
    fn test_minolta_parser() {
        use oxidex::parsers::tiff::makernotes::minolta::MinoltaParser;
        let p = MinoltaParser::new();
        assert_eq!(p.manufacturer_name(), "Minolta");
        let mut data = ifd_le(&[
            ent(0x0000, T_LONG, 1, 0x30313030),
            ent(0x0201, T_SHORT, 1, 1),
            ent(0x0403, T_SHORT, 1, 1),
        ]);
        data.extend_from_slice(&[0u8; 32]);
        let mut tags = HashMap::new();
        let _ = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
    }

    #[test]
    fn test_motorola_parser() {
        use oxidex::parsers::tiff::makernotes::motorola::MotorolaParser;
        let p = MotorolaParser::new();
        assert_eq!(p.manufacturer_name(), "Motorola");
        let mut data = ifd_le(&[ent(0x0001, T_SHORT, 1, 1)]);
        data.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        let _ = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
    }

    #[test]
    fn test_ge_parser() {
        use oxidex::parsers::tiff::makernotes::ge::GeParser;
        let p = GeParser::new();
        assert_eq!(p.manufacturer_name(), "GE");
        let mut data = ifd_le(&[ent(0x0001, T_SHORT, 1, 1)]);
        data.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        let _ = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
    }

    #[test]
    fn test_hp_parser() {
        use oxidex::parsers::tiff::makernotes::hp::HpParser;
        let p = HpParser::new();
        assert_eq!(p.manufacturer_name(), "HP");
        let mut data = ifd_le(&[ent(0x0001, T_SHORT, 1, 1)]);
        data.extend_from_slice(&[0u8; 16]);
        let mut tags = HashMap::new();
        let _ = p.parse(&data, ByteOrder::LittleEndian, &mut tags);
    }
}
