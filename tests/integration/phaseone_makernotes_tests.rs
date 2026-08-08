//! Integration tests for the Phase One MakerNotes parser
//!
//! Ground truth for every tag name/value below is
//! `%Image::ExifTool::PhaseOne::Main` in
//! `/opt/homebrew/Cellar/exiftool/13.55/libexec/lib/perl5/Image/ExifTool/PhaseOne.pm`
//! and, for the fixture test, `exiftool -G1 -s` on
//! `/tmp/oxidex-exiftool-cache/combined-samples/PhaseOne.iiq`.
//!
//! This file replaced an earlier version built entirely around a fabricated
//! tag table (`SystemType`, `ExposureMode`, `DriveMode`, `MirrorLockup`,
//! `SensorBitDepth`, ...) and a plain 12-byte-per-entry TIFF-IFD wire format
//! neither of which exist in `PhaseOne::Main` -- see
//! `oxidex::parsers::tiff::makernotes::registries::phaseone`'s module docs
//! for the full defect list. Phase One's real directory format has an
//! 8-byte signature + `ifd_start` header and 16-byte entries with an
//! explicit size/offset field (`ProcessPhaseOne`, PhaseOne.pm:596); see
//! `build_dir` below.

use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::phaseone::{PhaseOneMakerNoteParser, is_phaseone_makernote};
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

/// Builds a minimal synthetic Phase One `Main` directory matching
/// `ProcessPhaseOne`'s real wire format: an 8-byte `IIIICwaR` signature, a
/// 4-byte `ifd_start` (always 12, immediately after the header), the entry
/// count, then one 16-byte entry per `(tag_id, value)` pair. Values <= 4
/// bytes are stored inline; longer values (strings, float arrays) are
/// appended after the entry table and referenced by offset.
fn build_dir(entries: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"IIIICwaR");
    out.extend_from_slice(&12u32.to_le_bytes()); // ifd_start
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]); // unused

    let entries_start = out.len();
    let table_end = entries_start + entries.len() * 16;
    let mut tail = Vec::new();

    for (tag_id, value) in entries {
        out.extend_from_slice(&tag_id.to_le_bytes());
        out.extend_from_slice(&4u32.to_le_bytes()); // format code (unused by oxidex; known tags override it)
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        if value.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..value.len()].copy_from_slice(value);
            out.extend_from_slice(&inline);
        } else {
            out.extend_from_slice(&((table_end + tail.len()) as u32).to_le_bytes());
            tail.extend_from_slice(value);
        }
    }
    out.extend_from_slice(&tail);
    out
}

#[test]
fn test_phaseone_header_validation() {
    // ExifTool's MakerNotePhaseOne Condition: /^(IIII.waR|MMMMRaw.)/ -- byte
    // 4 (II variant) or byte 7 (MM variant) is a wildcard version char.
    assert!(is_phaseone_makernote(b"IIIICwaR\0\0\0\0"));
    assert!(is_phaseone_makernote(b"MMMMRawX\0\0\0\0"));

    // Neither of the old parser's two accepted shapes is real: real files
    // carry no "Phase One" text, and a bare plausible-looking u16 entry
    // count isn't Phase One's signature either.
    assert!(!is_phaseone_makernote(b"Phase One\x00\x10\x00\x00"));
    assert!(!is_phaseone_makernote(b"\x08\x00\x00\x00\x00\x00\x00\x00"));
    assert!(!is_phaseone_makernote(b"Canon\0\0\0"));
    assert!(!is_phaseone_makernote(b"\x05"));
    assert!(!is_phaseone_makernote(b""));
}

#[test]
fn test_phaseone_makernote_parse_basic() {
    let data = build_dir(&[
        (0x0100, 0i32.to_le_bytes().to_vec()), // CameraOrientation: Horizontal (normal)
        (0x0105, 100i32.to_le_bytes().to_vec()), // ISO
    ]);
    let mut tags = HashMap::new();
    let result = PhaseOneMakerNoteParser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok(), "{result:?}");

    assert_eq!(
        tags.get("PhaseOne:CameraOrientation"),
        Some(&"Horizontal (normal)".to_string())
    );
    assert_eq!(tags.get("PhaseOne:ISO"), Some(&"100".to_string()));
}

#[test]
fn test_phaseone_makernote_parse_sensor_dimensions() {
    let data = build_dir(&[
        (0x0108, 7372i32.to_le_bytes().to_vec()), // SensorWidth
        (0x0109, 5536i32.to_le_bytes().to_vec()), // SensorHeight
        (0x010c, 7320i32.to_le_bytes().to_vec()), // ImageWidth
        (0x010d, 5484i32.to_le_bytes().to_vec()), // ImageHeight
    ]);
    let mut tags = HashMap::new();
    let result = PhaseOneMakerNoteParser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok(), "{result:?}");

    assert_eq!(tags.get("PhaseOne:SensorWidth"), Some(&"7372".to_string()));
    assert_eq!(tags.get("PhaseOne:SensorHeight"), Some(&"5536".to_string()));
    assert_eq!(tags.get("PhaseOne:ImageWidth"), Some(&"7320".to_string()));
    assert_eq!(tags.get("PhaseOne:ImageHeight"), Some(&"5484".to_string()));
}

#[test]
fn test_phaseone_makernote_parse_lens_and_focal_length() {
    // 0x0412 is the one real lens tag in PhaseOne::Main: LensModel (string).
    // 0x0403 is FocalLength (float, "%.1f mm"), not "Aperture" as the old
    // registry had it.
    let mut lens = b"Mamiya LS 80mm f/2.8 D".to_vec();
    lens.push(0);
    let data = build_dir(&[(0x0412, lens), (0x0403, 80.0f32.to_le_bytes().to_vec())]);
    let mut tags = HashMap::new();
    let result = PhaseOneMakerNoteParser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok(), "{result:?}");

    assert_eq!(
        tags.get("PhaseOne:LensModel"),
        Some(&"Mamiya LS 80mm f/2.8 D".to_string())
    );
    assert_eq!(
        tags.get("PhaseOne:FocalLength"),
        Some(&"80.0 mm".to_string())
    );
    assert!(!tags.contains_key("PhaseOne:Aperture"));
}

#[test]
fn test_phaseone_makernote_parse_error_too_short() {
    let data = b"P";
    let mut tags = HashMap::new();
    let result = PhaseOneMakerNoteParser.parse(data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}

#[test]
fn test_phaseone_makernote_parse_error_invalid_entry_count() {
    // ProcessPhaseOne requires 2..=300 entries; encode a header claiming 1.
    let mut data = Vec::new();
    data.extend_from_slice(b"IIIICwaR");
    data.extend_from_slice(&12u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes()); // 1 entry: invalid
    data.extend_from_slice(&[0u8; 4]);
    data.extend_from_slice(&[0u8; 16]); // one (unused) entry slot

    let mut tags = HashMap::new();
    let result = PhaseOneMakerNoteParser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_err());
}

#[test]
fn test_phaseone_makernote_parser_trait_implementation() {
    let parser = PhaseOneMakerNoteParser;
    assert_eq!(parser.manufacturer_name(), "PhaseOne");
    assert_eq!(parser.tag_prefix(), "PhaseOne:");
}

#[test]
fn test_phaseone_iiq_fixture_reachable_via_dispatcher() {
    if !std::path::Path::new("/tmp/oxidex-exiftool-cache/combined-samples/PhaseOne.iiq").is_file() {
        eprintln!(
            "skipping: corpus fixture not present at {}",
            "/tmp/oxidex-exiftool-cache/combined-samples/PhaseOne.iiq"
        );
        return;
    }
    // Regression: dispatch used to key strictly on Make ("phase one"/"phase
    // one a/s"), so a Leaf-branded (Make: "Leaf") .IIQ carrying this exact
    // signature matched nothing and produced zero PhaseOne: tags. Dispatch
    // must key off the signature, independent of Make.
    use oxidex::parsers::tiff::makernote_dispatcher::dispatch_makernote;

    let path = "/tmp/oxidex-exiftool-cache/combined-samples/PhaseOne.iiq";
    let Ok(file) = std::fs::read(path) else {
        eprintln!("skipping: corpus fixture not present at {path}");
        return;
    };
    let mn = &file[8..]; // MakerNote value starts right after the 8-byte TIFF header (PutFirst => 1)

    let mut tags = HashMap::new();
    let result = dispatch_makernote("Leaf", mn, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        tags.get("PhaseOne:LensModel"),
        Some(&"Mamiya LS 80mm f/2.8 D".to_string())
    );
}
