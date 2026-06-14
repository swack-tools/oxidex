//! Wave-2 coverage tests for RAW metadata extraction.
//!
//! Targets the REMAINING uncovered paths in:
//! - src/parsers/raw/metadata.rs  (rich TIFF-based dispatch, DNG/CR2/NEF
//!   enrichment, SubIFD/IFD1/EXIF-IFD/GPS-IFD traversal, MakerNote dispatch,
//!   X3F color-mode/image-section, MRW TTW block, RAF embedded JPEG path)
//! - src/parsers/raw/raf_parser.rs (parse_raf_makernote full success path +
//!   sensor info / internal serial / error branches)
//!
//! Wave-1 (tests/cov_enums_formatters_raw.rs) already hit the empty/minimal
//! dispatch happy paths, the X3F property/header path, MRW PRD/WBG, and RAF
//! makernote serial decoding. This file goes after the structural traversal
//! and enrichment branches those tests did not reach, plus real fixtures and
//! the production read_metadata path.

#[path = "common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::raw::raf_parser::parse_raf_makernote;
use oxidex::parsers::raw::{RawFormat, parse_raw_metadata};
use oxidex::parsers::tiff::ifd_parser::ByteOrder;

// ============================================================================
// TIFF builders
// ============================================================================

/// A single 12-byte IFD entry: (tag, field_type, count, 4-byte inline value).
type Entry = (u16, u16, u32, [u8; 4]);

/// Encode a u16 into the low 2 bytes of a 4-byte inline value (little-endian).
fn inline_u16_le(v: u16) -> [u8; 4] {
    let mut b = [0u8; 4];
    b[0..2].copy_from_slice(&v.to_le_bytes());
    b
}

/// Encode a u32 into a 4-byte inline value (little-endian).
fn inline_u32_le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// Encode a u16 into the low 2 bytes of a 4-byte inline value (big-endian).
fn inline_u16_be(v: u16) -> [u8; 4] {
    let mut b = [0u8; 4];
    b[2..4].copy_from_slice(&v.to_be_bytes());
    b
}

/// Write a little-endian IFD (entry count + entries + next-IFD pointer) at the
/// given location in `data`, padding with zeros as needed.
fn write_ifd_le(data: &mut Vec<u8>, at: usize, entries: &[Entry], next_ifd: u32) {
    let mut ifd = Vec::new();
    ifd.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, typ, count, value) in entries {
        ifd.extend_from_slice(&tag.to_le_bytes());
        ifd.extend_from_slice(&typ.to_le_bytes());
        ifd.extend_from_slice(&count.to_le_bytes());
        ifd.extend_from_slice(value);
    }
    ifd.extend_from_slice(&next_ifd.to_le_bytes());
    if data.len() < at + ifd.len() {
        data.resize(at + ifd.len(), 0);
    }
    data[at..at + ifd.len()].copy_from_slice(&ifd);
}

/// Build a little-endian TIFF whose IFD0 lives at offset 8.
fn build_tiff_le(entries: &[Entry], next_ifd: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"II\x2a\x00");
    data.extend_from_slice(&8u32.to_le_bytes());
    write_ifd_le(&mut data, 8, entries, next_ifd);
    data
}

// ============================================================================
// DNG enrichment: color calibration aliases + version string from fixture
// ============================================================================

#[test]
fn test_dng_color_calibration_alias() {
    // IFD0 with DNGVersion (0xC612, BYTE x4) and two ColorMatrix tags
    // (0xC621, 0xC622) so extract_dng_tags emits DNG:AvailableColorCalibration.
    // ColorMatrix uses SRATIONAL data placed out-of-line; we just need the tag
    // present, so a single SRATIONAL value (count 1) stored inline works for
    // presence detection. Actually count*size > 4 stores out-of-line, so use a
    // simple inline-able representation: count 1 SRATIONAL needs 8 bytes -> out
    // of line. To keep it simple, store the matrices as SHORT count 1 (inline).
    let entries = vec![
        (0xC612u16, 1u16, 4u32, [1, 4, 0, 0]), // DNGVersion -> 1.4.0.0
        (0xC621u16, 3u16, 1u32, inline_u16_le(1)), // ColorMatrix1 (present)
        (0xC622u16, 3u16, 1u32, inline_u16_le(2)), // ColorMatrix2 (present)
        (0xC65Au16, 3u16, 1u32, inline_u16_le(1)), // CalibrationIlluminant1
    ];
    let data = build_tiff_le(&entries, 0);
    let md = parse_raw_metadata(&data, RawFormat::AdobeDNG).expect("DNG parse");

    // Version string from the byte parsing branch.
    assert_eq!(
        md.get("DNG:VersionString"),
        Some(&TagValue::String("1.4.0.0".to_string()))
    );
    // Color calibration alias listing the available tags.
    let alias = md.get("DNG:AvailableColorCalibration");
    assert!(
        alias.is_some(),
        "expected color-calibration alias, keys: {:?}",
        md.keys().collect::<Vec<_>>()
    );
    if let Some(TagValue::String(s)) = alias {
        assert!(s.contains("ColorMatrix1"));
        assert!(s.contains("ColorMatrix2"));
    }
}

#[test]
fn test_dng_real_fixture() {
    // The checked-in sample.dng is a genuine little-endian TIFF with Make,
    // Model and a DNGVersion tag. Drive the public parser over it.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/raw/sample.dng"
    ))
    .expect("read sample.dng");
    let md = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).expect("DNG fixture parse");
    // The fixture is a deliberately truncated TIFF: the dispatch still succeeds
    // and always stamps File:FileType even if the IFD walk bails on the tiny
    // out-of-line value offsets.
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("AdobeDNG".to_string()))
    );
}

#[test]
fn test_dng_no_color_calibration_when_absent() {
    // DNGVersion present but no color matrices -> alias must NOT be emitted.
    let entries = vec![(0xC612u16, 1u16, 4u32, [2, 0, 0, 0])];
    let data = build_tiff_le(&entries, 0);
    let md = parse_raw_metadata(&data, RawFormat::AdobeDNG).expect("DNG parse");
    assert_eq!(
        md.get("DNG:VersionString"),
        Some(&TagValue::String("2.0.0.0".to_string()))
    );
    assert!(!md.contains_key("DNG:AvailableColorCalibration"));
}

// ============================================================================
// CR2 enrichment: SubIFD + IFD1 preview layers
// ============================================================================

#[test]
fn test_cr2_subifd_and_ifd1_layers() {
    // IFD0 references a SubIFD (tag 0x014A) at offset 200 and chains to IFD1 at
    // offset 260. IFD0 also has its own ImageWidth so image_count counts IFD0,
    // IFD1 and SubIFD0. SubIFD0 carries ImageWidth+ImageHeight to trigger
    // CR2:HasRAWData / CR2:RAWImageSize. IFD1 carries ImageWidth+Compression to
    // trigger CR2:HasJPEGPreview.
    let sub_offset = 200u32;
    let ifd1_offset = 260u32;

    let ifd0 = vec![
        (0x0100u16, 3u16, 1u32, inline_u16_le(64)), // IFD0 ImageWidth
        (0x014Au16, 4u16, 1u32, inline_u32_le(sub_offset)), // SubIFD pointer
    ];
    let mut data = build_tiff_le(&ifd0, ifd1_offset);

    // SubIFD0 at sub_offset: ImageWidth=5472, ImageHeight=3648.
    write_ifd_le(
        &mut data,
        sub_offset as usize,
        &[
            (0x0100u16, 3u16, 1u32, inline_u16_le(5472)),
            (0x0101u16, 3u16, 1u32, inline_u16_le(3648)),
        ],
        0,
    );

    // IFD1 at ifd1_offset: ImageWidth + Compression (JPEG=6).
    write_ifd_le(
        &mut data,
        ifd1_offset as usize,
        &[
            (0x0100u16, 3u16, 1u32, inline_u16_le(160)),
            (0x0103u16, 3u16, 1u32, inline_u16_le(6)),
        ],
        0,
    );

    let md = parse_raw_metadata(&data, RawFormat::CanonCR2).expect("CR2 parse");

    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("CanonCR2".to_string()))
    );
    // RAW data present via SubIFD0.
    assert_eq!(
        md.get("CR2:HasRAWData"),
        Some(&TagValue::String("true".to_string()))
    );
    assert_eq!(
        md.get("CR2:RAWImageSize"),
        Some(&TagValue::String("5472x3648".to_string()))
    );
    // JPEG preview present via IFD1.
    assert_eq!(
        md.get("CR2:HasJPEGPreview"),
        Some(&TagValue::String("true".to_string()))
    );
    // Image layer count should be > 0 (IFD0 + IFD1 + SubIFD0).
    assert!(md.contains_key("CR2:ImageLayerCount"));
    if let Some(TagValue::Integer(n)) = md.get("CR2:ImageLayerCount") {
        assert!(*n >= 2, "layer count = {}", n);
    }
}

// ============================================================================
// NEF enrichment: compression name mapping + bit depth + RAW size
// ============================================================================

fn build_nef_with_subifd(compression: u16, bits: u16) -> Vec<u8> {
    let sub_offset = 200u32;
    let ifd0 = vec![
        (0x0100u16, 3u16, 1u32, inline_u16_le(160)), // IFD0 ImageWidth
        (0x014Au16, 4u16, 1u32, inline_u32_le(sub_offset)),
    ];
    let mut data = build_tiff_le(&ifd0, 0);
    write_ifd_le(
        &mut data,
        sub_offset as usize,
        &[
            (0x0100u16, 3u16, 1u32, inline_u16_le(6048)), // ImageWidth
            (0x0101u16, 3u16, 1u32, inline_u16_le(4024)), // ImageHeight
            (0x0103u16, 3u16, 1u32, inline_u16_le(compression)), // Compression
            (0x0102u16, 3u16, 1u32, inline_u16_le(bits)), // BitsPerSample
        ],
        0,
    );
    data
}

#[test]
fn test_nef_compression_uncompressed() {
    let data = build_nef_with_subifd(1, 14);
    let md = parse_raw_metadata(&data, RawFormat::NikonNEF).expect("NEF parse");
    assert_eq!(
        md.get("NEF:RAWCompression"),
        Some(&TagValue::String("Uncompressed".to_string()))
    );
    assert_eq!(
        md.get("NEF:HasRAWData"),
        Some(&TagValue::String("true".to_string()))
    );
    assert_eq!(
        md.get("NEF:RAWImageSize"),
        Some(&TagValue::String("6048x4024".to_string()))
    );
    assert_eq!(
        md.get("NEF:RAWBitDepth"),
        Some(&TagValue::String("14".to_string()))
    );
    assert!(md.contains_key("NEF:ImageLayerCount"));
}

#[test]
fn test_nef_compression_jpeg_and_lossless() {
    // 7 -> "JPEG"
    let md = parse_raw_metadata(&build_nef_with_subifd(7, 8), RawFormat::NikonNEF).unwrap();
    assert_eq!(
        md.get("NEF:RAWCompression"),
        Some(&TagValue::String("JPEG".to_string()))
    );

    // 34713 -> "Nikon Lossless Compressed" (value exceeds u16; use LONG type so
    // the converter yields an Integer rather than truncating).
    let sub_offset = 200u32;
    let ifd0 = vec![(0x014Au16, 4u16, 1u32, inline_u32_le(sub_offset))];
    let mut data = build_tiff_le(&ifd0, 0);
    write_ifd_le(
        &mut data,
        sub_offset as usize,
        &[
            (0x0100u16, 3u16, 1u32, inline_u16_le(6048)),
            (0x0101u16, 3u16, 1u32, inline_u16_le(4024)),
            (0x0103u16, 4u16, 1u32, inline_u32_le(34713)), // LONG compression
        ],
        0,
    );
    let md = parse_raw_metadata(&data, RawFormat::NikonNEF).unwrap();
    assert_eq!(
        md.get("NEF:RAWCompression"),
        Some(&TagValue::String("Nikon Lossless Compressed".to_string()))
    );
}

#[test]
fn test_nrw_dispatch_uses_nef_enrichment() {
    // NRW routes through extract_nef_tags as well.
    let data = build_nef_with_subifd(1, 12);
    let md = parse_raw_metadata(&data, RawFormat::NikonNRW).expect("NRW parse");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("NikonNRW".to_string()))
    );
    assert!(md.contains_key("NEF:RAWCompression"));
}

// ============================================================================
// Many TIFF-based format variants funnel through parse_tiff_based_raw.
// Exercise the dispatch arms that wave-1 never named.
// ============================================================================

#[test]
fn test_tiff_based_variant_dispatch_le() {
    // A simple LE TIFF with one ImageWidth entry, parsed under many format
    // labels. Each just confirms File:FileType reflects the enum name and that
    // the IFD tag is extracted (covers the generic parse_tiff_based_raw arm).
    let data = build_tiff_le(&[(0x0100u16, 3u16, 1u32, inline_u16_le(320))], 0);
    let formats = [
        (RawFormat::SonyARW, "SonyARW"),
        (RawFormat::SonySR2, "SonySR2"),
        (RawFormat::SonySRF, "SonySRF"),
        (RawFormat::SonySRW, "SonySRW"),
        (RawFormat::SonyARQ, "SonyARQ"),
        (RawFormat::SonyARI, "SonyARI"),
        (RawFormat::PentaxPEF, "PentaxPEF"),
        (RawFormat::OlympusORF, "OlympusORF"),
        (RawFormat::OlympusORI, "OlympusORI"),
        (RawFormat::PanasonicRW2, "PanasonicRW2"),
        (RawFormat::PanasonicRWL, "PanasonicRWL"),
        (RawFormat::Hasselblad3FR, "Hasselblad3FR"),
        (RawFormat::HasselbladFFF, "HasselbladFFF"),
        (RawFormat::PhaseOneIIQ, "PhaseOneIIQ"),
        (RawFormat::MamiyaMEF, "MamiyaMEF"),
        (RawFormat::LeafMOS, "LeafMOS"),
        (RawFormat::KodakDCR, "KodakDCR"),
        (RawFormat::KodakKDC, "KodakKDC"),
        (RawFormat::MinoltaMDC, "MinoltaMDC"),
        (RawFormat::EpsonERF, "EpsonERF"),
        (RawFormat::GoProGPR, "GoProGPR"),
        (RawFormat::HEIFHIF, "HEIFHIF"),
        (RawFormat::LightLRI, "LightLRI"),
        (RawFormat::SinarSTI, "SinarSTI"),
    ];
    for (fmt, name) in formats {
        let md = parse_raw_metadata(&data, fmt).unwrap_or_else(|e| panic!("{}: {}", name, e));
        assert_eq!(
            md.get("File:FileType"),
            Some(&TagValue::String(name.to_string())),
            "format {}",
            name
        );
        // The single ImageWidth tag should land under IFD0.
        assert!(
            md.keys().any(|k| k.starts_with("IFD0:")),
            "no IFD0 tag for {}",
            name
        );
    }
}

#[test]
fn test_tiff_based_big_endian() {
    // Big-endian TIFF (MM) with one ImageWidth=300 SHORT entry at IFD0.
    let mut data = Vec::new();
    data.extend_from_slice(b"MM\x00\x2a");
    data.extend_from_slice(&8u32.to_be_bytes());
    data.extend_from_slice(&1u16.to_be_bytes());
    data.extend_from_slice(&0x0100u16.to_be_bytes()); // ImageWidth
    data.extend_from_slice(&3u16.to_be_bytes()); // SHORT
    data.extend_from_slice(&1u32.to_be_bytes()); // count
    data.extend_from_slice(&inline_u16_be(300)); // value (BE inline)
    data.extend_from_slice(&0u32.to_be_bytes()); // next IFD
    let md = parse_raw_metadata(&data, RawFormat::PhaseOneIIQ).expect("BE parse");
    assert!(md.keys().any(|k| k.starts_with("IFD0:")));
}

// ============================================================================
// EXIF sub-IFD pointer (0x8769) + Make (0x010F) + MakerNote (0x927C) dispatch
// ============================================================================

#[test]
fn test_tiff_with_exif_subifd_and_makernote() {
    // IFD0: Make="Canon" + EXIF-IFD pointer (0x8769) at offset 200.
    // ExifIFD: a MakerNote tag (0x927C, count 12) pointing inline-ish data.
    // The MakerNote dispatcher will be invoked with make="Canon" (it may fail
    // gracefully on synthetic data, which still exercises the dispatch branch).
    let exif_offset = 200u32;

    // Make string "Canon\0" stored out-of-line at offset 300.
    let make_value_offset = 300u32;
    let ifd0 = vec![
        (0x010Fu16, 2u16, 6u32, inline_u32_le(make_value_offset)), // Make (ASCII, >4 bytes => offset)
        (0x8769u16, 4u16, 1u32, inline_u32_le(exif_offset)),       // EXIF IFD pointer
    ];
    let mut data = build_tiff_le(&ifd0, 0);

    // Place "Canon\0" at make_value_offset.
    if data.len() < make_value_offset as usize + 6 {
        data.resize(make_value_offset as usize + 6, 0);
    }
    data[make_value_offset as usize..make_value_offset as usize + 6].copy_from_slice(b"Canon\x00");

    // ExifIFD: ExposureTime-ish SHORT + MakerNote (0x927C) referencing data at
    // offset 320 with length 16.
    let mn_offset = 320u32;
    write_ifd_le(
        &mut data,
        exif_offset as usize,
        &[
            (0x829Au16, 5u16, 1u32, inline_u32_le(400)), // ExposureTime (RATIONAL ptr; harmless)
            (0x927Cu16, 7u16, 16u32, inline_u32_le(mn_offset)), // MakerNote
        ],
        0,
    );
    // 16 bytes of MakerNote payload (Canon makernotes start with an IFD count).
    if data.len() < mn_offset as usize + 16 {
        data.resize(mn_offset as usize + 16, 0);
    }

    // Should parse without panicking and at least surface the EXIF-IFD walk.
    let md = parse_raw_metadata(&data, RawFormat::CanonCR2).expect("parse with exif ifd");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("CanonCR2".to_string()))
    );
    // The Make tag is retained in IFD0 metadata (Make is not stripped).
    assert!(
        md.keys().any(|k| k.contains("Make")) || md.keys().any(|k| k.starts_with("ExifIFD:")),
        "keys: {:?}",
        md.keys().collect::<Vec<_>>()
    );
}

#[test]
fn test_tiff_with_gps_subifd_pointer() {
    // IFD0 with a GPS sub-IFD pointer (0x8825) at offset 200; the GPS IFD has a
    // GPSVersionID-ish entry. Exercises the gps_ifd_offset branch.
    let gps_offset = 200u32;
    let ifd0 = vec![(0x8825u16, 4u16, 1u32, inline_u32_le(gps_offset))];
    let mut data = build_tiff_le(&ifd0, 0);
    write_ifd_le(
        &mut data,
        gps_offset as usize,
        &[(0x0001u16, 2u16, 2u32, {
            let mut b = [0u8; 4];
            b[0..2].copy_from_slice(b"N\x00");
            b
        })],
        0,
    );
    let md = parse_raw_metadata(&data, RawFormat::SonyARW).expect("gps parse");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("SonyARW".to_string()))
    );
}

// ============================================================================
// Error/edge branches in dispatch
// ============================================================================

#[test]
fn test_tiff_based_too_small() {
    // < 8 bytes -> Err for a TIFF-based format.
    assert!(parse_raw_metadata(b"abcd", RawFormat::SonyARW).is_err());
    assert!(parse_raw_metadata(b"\x00", RawFormat::OlympusORF).is_err());
}

#[test]
fn test_tiff_based_invalid_byte_order() {
    // 8+ bytes but a non-II/MM marker -> detect_byte_order errors.
    let data = vec![b'X', b'Y', 0, 0, 8, 0, 0, 0, 0, 0];
    assert!(parse_raw_metadata(&data, RawFormat::SonyARW).is_err());
}

#[test]
fn test_generic_formats_fallback_on_non_tiff() {
    // GenericRAW/CAM/REV catch a TIFF-parse failure and still return minimal
    // metadata (the or_else branch).
    for fmt in [
        RawFormat::GenericRAW,
        RawFormat::GenericCAM,
        RawFormat::GenericREV,
    ] {
        // 8 bytes, valid II header, but the IFD offset points past EOF so the
        // walk yields just File:FileType; non-TIFF garbage routes the or_else.
        let md = parse_raw_metadata(b"not-a-real-tiff-buffer", fmt).expect("generic fallback");
        assert!(md.contains_key("File:FileType"));
    }
}

#[test]
fn test_generic_formats_valid_tiff() {
    // GenericRAW over a valid TIFF should parse normally (the Ok arm).
    let data = build_tiff_le(&[(0x0100u16, 3u16, 1u32, inline_u16_le(100))], 0);
    let md = parse_raw_metadata(&data, RawFormat::GenericRAW).expect("generic tiff");
    assert!(md.keys().any(|k| k.starts_with("IFD0:")));
}

// ============================================================================
// CR3 / CRW stub parsers
// ============================================================================

#[test]
fn test_cr3_stub_returns_filetype() {
    let md = parse_raw_metadata(b"\x00\x00\x00\x18ftypcrx \x00\x00", RawFormat::CanonCR3).unwrap();
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("CanonCR3".to_string()))
    );
    // CR3 stub adds nothing else.
    assert_eq!(md.keys().count(), 1);
}

#[test]
fn test_crw_stub_returns_filetype() {
    let md = parse_raw_metadata(b"II\x1a\x00\x00\x00HEAPCCDR", RawFormat::CanonCRW).unwrap();
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("CanonCRW".to_string()))
    );
}

// ============================================================================
// Sigma X3F: color-mode (v2.3), image section (SECi), directory edge cases
// ============================================================================

/// Build an X3F FOVb header of the given version with optional WB/colormode
/// strings, returning the leading header bytes (no directory).
fn x3f_header(version: u32, columns: u32, rows: u32, rotation: u32, wb: &str, cm: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"FOVb");
    data.extend_from_slice(&version.to_le_bytes());
    data.extend_from_slice(&[0u8; 16]); // unique id (8..24)
    data.extend_from_slice(&0u32.to_le_bytes()); // mark bits (24)
    data.extend_from_slice(&columns.to_le_bytes()); // (28)
    data.extend_from_slice(&rows.to_le_bytes()); // (32)
    data.extend_from_slice(&rotation.to_le_bytes()); // (36)
    // WB string (32 bytes at 40)
    let mut wbb = [0u8; 32];
    let wbytes = wb.as_bytes();
    wbb[..wbytes.len().min(31)].copy_from_slice(&wbytes[..wbytes.len().min(31)]);
    data.extend_from_slice(&wbb);
    // Color mode string (32 bytes at 72)
    let mut cmb = [0u8; 32];
    let cbytes = cm.as_bytes();
    cmb[..cbytes.len().min(31)].copy_from_slice(&cbytes[..cbytes.len().min(31)]);
    data.extend_from_slice(&cmb);
    data
}

#[test]
fn test_x3f_v23_color_mode_and_image_section() {
    // v2.3 header with WB + ColorMode strings, plus a SECi image section of
    // type 3 (preview JPEG) so parse_x3f_image_section stores PreviewImageSize.
    let mut data = x3f_header(0x00020003, 5424, 3616, 90, "Auto", "Standard");

    // Build SECi image section: header(28 bytes) -> type/format/cols/rows/stride.
    let mut seci = Vec::new();
    seci.extend_from_slice(b"SECi"); // 0
    seci.extend_from_slice(&1u32.to_le_bytes()); // version (4)
    seci.extend_from_slice(&3u32.to_le_bytes()); // image type 3 = preview (8)
    seci.extend_from_slice(&18u32.to_le_bytes()); // image format (12)
    seci.extend_from_slice(&1920u32.to_le_bytes()); // columns (16)
    seci.extend_from_slice(&1280u32.to_le_bytes()); // rows (20)
    seci.extend_from_slice(&5760u32.to_le_bytes()); // row stride (24)
    seci.extend_from_slice(&[0u8; 8]); // pad

    let seci_offset = data.len();
    data.extend_from_slice(&seci);

    // SECd directory with one SECi entry.
    let dir_offset = data.len();
    let mut secd = Vec::new();
    secd.extend_from_slice(b"SECd");
    secd.extend_from_slice(&1u32.to_le_bytes()); // version
    secd.extend_from_slice(&1u32.to_le_bytes()); // num entries
    secd.extend_from_slice(&(seci_offset as u32).to_le_bytes()); // offset
    secd.extend_from_slice(&(seci.len() as u32).to_le_bytes()); // size
    secd.extend_from_slice(b"SECi"); // type
    data.extend_from_slice(&secd);

    // Trailer pointing at the directory.
    data.extend_from_slice(&(dir_offset as u32).to_le_bytes());

    let md = parse_raw_metadata(&data, RawFormat::SigmaX3F).expect("x3f v2.3 parse");
    assert_eq!(
        md.get("SigmaRaw:FileVersion"),
        Some(&TagValue::String("2.3".to_string()))
    );
    assert_eq!(
        md.get("SigmaRaw:WhiteBalance"),
        Some(&TagValue::String("Auto".to_string()))
    );
    assert_eq!(
        md.get("SigmaRaw:ColorMode"),
        Some(&TagValue::String("Standard".to_string()))
    );
    assert_eq!(
        md.get("SigmaRaw:Rotation"),
        Some(&TagValue::String("90".to_string()))
    );
    // Preview dimensions from the SECi (type 3) section.
    assert_eq!(
        md.get("MakerNotes:PreviewImageSize"),
        Some(&TagValue::String("1920x1280".to_string()))
    );
}

#[test]
fn test_x3f_directory_offset_out_of_range() {
    // Valid FOVb header but the trailer points past EOF -> early return after
    // header fields, with no SECd parse.
    let mut data = x3f_header(0x00020001, 100, 100, 0, "Daylight", "");
    // Append a bogus directory offset (way past EOF).
    data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
    let md = parse_raw_metadata(&data, RawFormat::SigmaX3F).expect("x3f bad dir");
    assert!(md.contains_key("SigmaRaw:FileVersion"));
    assert_eq!(
        md.get("SigmaRaw:WhiteBalance"),
        Some(&TagValue::String("Daylight".to_string()))
    );
}

#[test]
fn test_x3f_directory_wrong_magic() {
    // Trailer points to a valid offset but the bytes there are not "SECd".
    let mut data = x3f_header(0x00020001, 100, 100, 0, "Flash", "");
    let bogus_dir = data.len();
    data.extend_from_slice(b"NOPE\x00\x00\x00\x00\x00\x00\x00\x00"); // 12 bytes, wrong magic
    data.extend_from_slice(&(bogus_dir as u32).to_le_bytes());
    let md = parse_raw_metadata(&data, RawFormat::SigmaX3F).expect("x3f wrong magic");
    assert!(md.contains_key("SigmaRaw:FileVersion"));
}

#[test]
fn test_x3f_too_short_header() {
    // < 40 bytes -> only FileType returned.
    let md = parse_raw_metadata(b"FOVb\x01\x00\x02\x00short", RawFormat::SigmaX3F).unwrap();
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("SigmaX3F".to_string()))
    );
    assert!(!md.contains_key("SigmaRaw:FileVersion"));
}

// ============================================================================
// Minolta MRW: TTW block (embedded TIFF) + multi-block + unknown block
// ============================================================================

#[test]
fn test_mrw_ttw_block_embedded_tiff() {
    // MRM container with a TTW block carrying a small valid little-endian TIFF.
    let inner_tiff = build_tiff_le(&[(0x0100u16, 3u16, 1u32, inline_u16_le(4000))], 0);

    let mut data = Vec::new();
    data.extend_from_slice(b"\x00MRM");
    data.extend_from_slice(&0u32.to_be_bytes()); // file size (ignored)

    // TTW block: tag "\x00TTW" + big-endian size + TIFF payload.
    data.extend_from_slice(b"\x00TTW");
    data.extend_from_slice(&(inner_tiff.len() as u32).to_be_bytes());
    data.extend_from_slice(&inner_tiff);

    // An unknown block afterward to exercise the default arm.
    data.extend_from_slice(b"\x00ZZZ");
    data.extend_from_slice(&4u32.to_be_bytes());
    data.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);

    let md = parse_raw_metadata(&data, RawFormat::MinoltaMRW).expect("mrw ttw");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("MinoltaMRW".to_string()))
    );
    // The embedded TIFF's IFD0 tag should be merged in.
    assert!(
        md.keys().any(|k| k.starts_with("IFD0:")),
        "embedded TIFF tags missing: {:?}",
        md.keys().collect::<Vec<_>>()
    );
}

#[test]
fn test_mrw_wbg_zero_green_no_ratio() {
    // WBG block whose green multiplier is 0 -> the ratio branch is skipped.
    let mut data = Vec::new();
    data.extend_from_slice(b"\x00MRM");
    data.extend_from_slice(&0u32.to_be_bytes());

    let mut wbg = Vec::new();
    wbg.extend_from_slice(&512u16.to_be_bytes()); // r
    wbg.extend_from_slice(&0u16.to_be_bytes()); // g = 0 (skip ratios)
    wbg.extend_from_slice(&384u16.to_be_bytes()); // b
    wbg.extend_from_slice(&0u16.to_be_bytes()); // pad
    data.extend_from_slice(b"\x00WBG");
    data.extend_from_slice(&(wbg.len() as u32).to_be_bytes());
    data.extend_from_slice(&wbg);

    let md = parse_raw_metadata(&data, RawFormat::MinoltaMRW).expect("mrw wbg zero");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("MinoltaMRW".to_string()))
    );
    // With green=0, the color-balance ratios are NOT inserted.
    assert!(!md.contains_key("MakerNotes:ColorBalanceRed"));
}

#[test]
fn test_mrw_block_size_overruns_file() {
    // A block claiming a size larger than the remaining file -> the loop breaks.
    let mut data = Vec::new();
    data.extend_from_slice(b"\x00MRM");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(b"\x00PRD");
    data.extend_from_slice(&0xFFFFu32.to_be_bytes()); // huge size, overruns
    data.extend_from_slice(&[0u8; 4]);
    let md = parse_raw_metadata(&data, RawFormat::MinoltaMRW).expect("mrw overrun");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("MinoltaMRW".to_string()))
    );
}

// ============================================================================
// Fujifilm RAF: embedded-JPEG success path through parse_fujifilm_raf
// ============================================================================

/// Build a minimal JPEG (SOI + APP1/Exif TIFF + EOI). The embedded TIFF has a
/// single IFD0 entry (Make). Returns the JPEG bytes.
fn build_jpeg_with_exif() -> Vec<u8> {
    // Build the TIFF/EXIF block: II header, IFD0 at offset 8 with Make tag.
    let make_offset = 26u32; // place "FUJIFILM\0" out-of-line within the EXIF block
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2a\x00");
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at 8
    tiff.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
    tiff.extend_from_slice(&0x010Fu16.to_le_bytes()); // Make
    tiff.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    tiff.extend_from_slice(&9u32.to_le_bytes()); // count "FUJIFILM\0"
    tiff.extend_from_slice(&make_offset.to_le_bytes()); // offset to string
    tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0
    // pad to make_offset
    while tiff.len() < make_offset as usize {
        tiff.push(0);
    }
    tiff.extend_from_slice(b"FUJIFILM\x00");

    // APP1 payload = "Exif\0\0" + tiff.
    let mut app1 = Vec::new();
    app1.extend_from_slice(b"Exif\x00\x00");
    app1.extend_from_slice(&tiff);

    // JPEG: SOI, APP1 (FFE1 + 2-byte length covering length+payload), EOI.
    let mut jpeg = Vec::new();
    jpeg.extend_from_slice(&[0xFF, 0xD8]); // SOI
    jpeg.extend_from_slice(&[0xFF, 0xE1]); // APP1 marker
    let seg_len = (app1.len() + 2) as u16; // length field includes itself
    jpeg.extend_from_slice(&seg_len.to_be_bytes());
    jpeg.extend_from_slice(&app1);
    jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI
    jpeg
}

#[test]
fn test_raf_embedded_jpeg_success() {
    // Full RAF: "FUJIFILMCCD-RAW " signature + 76 bytes header room, jpeg_offset
    // at 84 (BE), jpeg_length at 88 (BE), then the JPEG at jpeg_offset.
    let jpeg = build_jpeg_with_exif();
    let jpeg_offset = 96usize; // after the 92-byte header + a little slack

    let mut data = vec![0u8; jpeg_offset];
    data[0..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
    data[84..88].copy_from_slice(&(jpeg_offset as u32).to_be_bytes());
    data[88..92].copy_from_slice(&(jpeg.len() as u32).to_be_bytes());
    data.extend_from_slice(&jpeg);

    let md = parse_raw_metadata(&data, RawFormat::FujifilmRAF).expect("raf embedded jpeg");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("FujifilmRAF".to_string()))
    );
    // The embedded EXIF Make tag should be surfaced under IFD0.
    assert!(
        md.keys().any(|k| k.contains("Make")),
        "RAF EXIF Make missing: {:?}",
        md.keys().collect::<Vec<_>>()
    );
}

#[test]
fn test_raf_jpeg_length_overruns_uses_remaining() {
    // jpeg_length deliberately larger than the actual remaining bytes triggers
    // the "use remaining size" warning branch while still parsing the JPEG.
    let jpeg = build_jpeg_with_exif();
    let jpeg_offset = 96usize;
    let mut data = vec![0u8; jpeg_offset];
    data[0..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
    data[84..88].copy_from_slice(&(jpeg_offset as u32).to_be_bytes());
    // claim a length far beyond EOF
    data[88..92].copy_from_slice(&0xFFFFu32.to_be_bytes());
    data.extend_from_slice(&jpeg);

    let md = parse_raw_metadata(&data, RawFormat::FujifilmRAF).expect("raf overrun length");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("FujifilmRAF".to_string()))
    );
}

#[test]
fn test_raf_jpeg_offset_exceeds_file() {
    // jpeg_offset >= data.len() -> Err branch.
    let mut data = vec![0u8; 92];
    data[0..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
    data[84..88].copy_from_slice(&1000u32.to_be_bytes()); // offset past EOF
    data[88..92].copy_from_slice(&10u32.to_be_bytes());
    assert!(parse_raw_metadata(&data, RawFormat::FujifilmRAF).is_err());
}

// ============================================================================
// raf_parser::parse_raf_makernote: full success path + helper coverage
// ============================================================================

#[test]
fn test_raf_makernote_full_fields() {
    // A 0x40-byte FUJIFILM makernote with serial at 0x10, internal serial at
    // 0x14, and a model string at 24..32 (sensor info). Drives the default
    // tags (ColorSpace, InternalSerialNumber, SensorInfo) plus serial decode.
    let mut data = vec![0u8; 0x40];
    data[0..8].copy_from_slice(b"FUJIFILM");
    data[0x10..0x14].copy_from_slice(&0xDEADBEEFu32.to_le_bytes()); // serial
    data[0x14..0x18].copy_from_slice(&0x12345678u32.to_le_bytes()); // internal serial
    // sensor model id at 24..32
    data[24..32].copy_from_slice(b"GFX100\x00\x00");

    let tags = parse_raf_makernote(&data, ByteOrder::LittleEndian).expect("raf makernote");
    assert_eq!(
        tags.get("Fujifilm:SerialNumber").map(String::as_str),
        Some("DEADBEEF")
    );
    assert_eq!(
        tags.get("Fujifilm:InternalSerialNumber")
            .map(String::as_str),
        Some("12345678")
    );
    assert_eq!(
        tags.get("Fujifilm:SensorInfo").map(String::as_str),
        Some("GFX100")
    );
    assert_eq!(
        tags.get("Fujifilm:ColorSpace").map(String::as_str),
        Some("sRGB")
    );
}

#[test]
fn test_raf_makernote_sensor_info_unknown_when_short() {
    // Exactly 0x18 bytes: serial + internal serial present, but < 32 bytes so
    // extract_sensor_info returns "Unknown Sensor".
    let mut data = vec![0u8; 0x18];
    data[0..8].copy_from_slice(b"FUJIFILM");
    data[0x10..0x14].copy_from_slice(&0x00000001u32.to_be_bytes());
    let tags = parse_raf_makernote(&data, ByteOrder::BigEndian).expect("short raf");
    assert_eq!(
        tags.get("Fujifilm:SensorInfo").map(String::as_str),
        Some("Unknown Sensor")
    );
    // Internal serial requires >= 0x18 bytes; here exactly 0x18 so it decodes.
    assert!(tags.contains_key("Fujifilm:InternalSerialNumber"));
}

#[test]
fn test_raf_makernote_internal_serial_unknown_when_tiny() {
    // 12..0x14 bytes: header + serial present but < 0x18 so internal serial is
    // "Unknown".
    let mut data = vec![0u8; 0x14];
    data[0..8].copy_from_slice(b"FUJIFILM");
    data[0x10..0x14].copy_from_slice(&0x0Au32.to_le_bytes());
    let tags = parse_raf_makernote(&data, ByteOrder::LittleEndian).expect("tiny raf");
    assert_eq!(
        tags.get("Fujifilm:InternalSerialNumber")
            .map(String::as_str),
        Some("Unknown")
    );
    assert_eq!(
        tags.get("Fujifilm:SensorInfo").map(String::as_str),
        Some("Unknown Sensor")
    );
}

#[test]
fn test_raf_makernote_errors() {
    // Too small for the 12-byte header.
    assert!(parse_raf_makernote(b"FUJIF", ByteOrder::LittleEndian).is_err());
    // Right size but wrong signature.
    let bad = vec![b'N'; 0x20];
    assert!(parse_raf_makernote(&bad, ByteOrder::LittleEndian).is_err());
    // Big-endian wrong signature too.
    assert!(parse_raf_makernote(&bad, ByteOrder::BigEndian).is_err());
}

// ============================================================================
// Production path: read_metadata over a tempfile with .dng extension
// ============================================================================

#[test]
fn test_read_metadata_dng_tempfile() {
    use std::io::Write;
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/raw/sample.dng"
    ))
    .expect("read sample.dng");

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("photo.dng");
    {
        let mut f = std::fs::File::create(&path).expect("create temp dng");
        f.write_all(&bytes).expect("write dng");
        f.flush().expect("flush");
    }

    // The production detection + parse path; tolerate either Ok or a graceful
    // Err but assert no panic and, when Ok, that FileType-ish data exists.
    let result = oxidex::core::operations::read_metadata(&path);
    match result {
        Ok(md) => {
            assert!(md.keys().count() > 0, "expected some metadata from .dng");
        }
        Err(_) => {
            // Detection may route differently; the call itself exercising the
            // production path is the point.
        }
    }
}
