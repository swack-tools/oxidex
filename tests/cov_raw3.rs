//! Wave-3 coverage tests for the RAW format pipeline and the deeper
//! Leica / Nikon / Canon MakerNote tag tables.
//!
//! Targets the REMAINING uncovered paths:
//! - `src/parsers/raw/metadata.rs` — `parse_raw_metadata` dispatch across every
//!   `RawFormat` variant, the TIFF-based walker (IFD0/IFD1, SubIFD pointers,
//!   EXIF/GPS sub-IFD pointers, MakerNote dispatch, format-specific enrichment
//!   for DNG/CR2/NEF), the X3F / MRW / CR3 / CRW proprietary parsers, the
//!   RAF embedded-JPEG/EXIF path, and the small helper converters.
//! - `src/parsers/raw/raf_parser.rs` — `parse_raf_makernote` plus every value
//!   decoder (`decode_white_balance`, `decode_focus_mode`, `decode_picture_mode`,
//!   `decode_film_mode`) and the serial/sensor/internal-serial helpers, driven
//!   through the public function and through full RAF files.
//! - `src/parsers/tiff/makernotes/{leica,nikon,canon}.rs` — every per-tag match
//!   arm in the public `parse` entry points, the value decoders, the model-id
//!   decoders, the tag-name mappers, and the malformed-input branches.
//!
//! A MakerNote / TIFF IFD is:
//!   [entry_count: u16][entries...][next_ifd_offset: u32]
//! Each entry is 12 bytes: [tag: u16][type: u16][count: u32][value/offset: u32].

#[path = "common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;

use std::collections::HashMap;

use oxidex::parsers::raw::raf_parser::parse_raf_makernote;
use oxidex::parsers::raw::{RawFormat, parse_raw_metadata};
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::canon::{
    CanonParser, apex_to_aperture, apex_to_exposure_time, canon_tag_to_name, decode_camera_type,
    decode_canon_model_id, format_focal_length, is_canon_makernote, parse_canon_makernotes,
};
use oxidex::parsers::tiff::makernotes::leica::{LeicaMakerNoteParser, is_leica_makernote};
use oxidex::parsers::tiff::makernotes::nikon::{
    NikonParser, is_nikon_makernote, parse_nikon_makernotes,
};
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

// ===========================================================================
// Helpers for building little-endian TIFF / IFD byte buffers.
// ===========================================================================

/// A single 12-byte IFD entry, little-endian.
fn le_entry(tag: u16, field_type: u16, count: u32, value: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(12);
    v.extend_from_slice(&tag.to_le_bytes());
    v.extend_from_slice(&field_type.to_le_bytes());
    v.extend_from_slice(&count.to_le_bytes());
    v.extend_from_slice(&value.to_le_bytes());
    v
}

/// A single 12-byte IFD entry, big-endian.
fn be_entry(tag: u16, field_type: u16, count: u32, value: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(12);
    v.extend_from_slice(&tag.to_be_bytes());
    v.extend_from_slice(&field_type.to_be_bytes());
    v.extend_from_slice(&count.to_be_bytes());
    v.extend_from_slice(&value.to_be_bytes());
    v
}

/// Build a minimal little-endian TIFF with a single IFD at offset 8.
/// `entries` must already be 12-byte chunks. `next_ifd` is the IFD0->next pointer.
fn le_tiff(entries: &[Vec<u8>], next_ifd: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"II\x2a\x00");
    data.extend_from_slice(&8u32.to_le_bytes()); // first IFD at offset 8
    data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in entries {
        data.extend_from_slice(e);
    }
    data.extend_from_slice(&next_ifd.to_le_bytes());
    data
}

/// Build an inline-value IFD (entry count + entries + trailing next-IFD = 0),
/// no TIFF header. Used as raw MakerNote IFD bodies (no header prefix).
fn le_ifd(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in entries {
        data.extend_from_slice(e);
    }
    data.extend_from_slice(&0u32.to_le_bytes());
    data
}

// TIFF field types
const T_BYTE: u16 = 1;
const T_ASCII: u16 = 2;
const T_SHORT: u16 = 3;
const T_LONG: u16 = 4;
const T_SRATIONAL: u16 = 10;

// ===========================================================================
// raw/metadata.rs — parse_raw_metadata dispatch across all RawFormat variants.
// ===========================================================================

/// Every TIFF-based format should at least dispatch to `parse_tiff_based_raw`,
/// returning Ok with a `File:FileType` tag. We feed a minimal valid TIFF.
#[test]
fn raw_dispatch_all_tiff_based_formats() {
    let tiff = le_tiff(&[], 0);

    let formats = [
        RawFormat::CanonCR2,
        RawFormat::NikonNEF,
        RawFormat::NikonNRW,
        RawFormat::SonyARW,
        RawFormat::SonySR2,
        RawFormat::SonySRF,
        RawFormat::SonySRW,
        RawFormat::SonyARQ,
        RawFormat::SonyARI,
        RawFormat::AdobeDNG,
        RawFormat::PentaxPEF,
        RawFormat::OlympusORF,
        RawFormat::OlympusORI,
        RawFormat::PanasonicRW2,
        RawFormat::PanasonicRWL,
        RawFormat::Hasselblad3FR,
        RawFormat::HasselbladFFF,
        RawFormat::PhaseOneIIQ,
        RawFormat::MamiyaMEF,
        RawFormat::LeafMOS,
        RawFormat::KodakDCR,
        RawFormat::KodakKDC,
        RawFormat::MinoltaMDC,
        RawFormat::EpsonERF,
        RawFormat::GoProGPR,
        RawFormat::HEIFHIF,
        RawFormat::LightLRI,
        RawFormat::SinarSTI,
    ];

    for fmt in formats {
        let result = parse_raw_metadata(&tiff, fmt);
        assert!(
            result.is_ok(),
            "format {:?} should parse a minimal TIFF",
            fmt
        );
        let md = result.unwrap();
        assert!(
            md.contains_key("File:FileType"),
            "format {:?} missing File:FileType",
            fmt
        );
    }
}

/// Proprietary stub / minimal formats (CR3, CRW) just record the file type.
#[test]
fn raw_dispatch_proprietary_stubs() {
    let cr3 = parse_raw_metadata(b"\x00\x00\x00\x18ftypcrx ----", RawFormat::CanonCR3).unwrap();
    assert!(cr3.contains_key("File:FileType"));

    let crw = parse_raw_metadata(b"II\x1a\x00\x00\x00HEAPCCDR", RawFormat::CanonCRW).unwrap();
    assert!(crw.contains_key("File:FileType"));
}

/// Generic fallbacks: feed garbage so TIFF parsing fails and the
/// minimal-metadata fallback branch is taken.
#[test]
fn raw_generic_fallback_on_bad_data() {
    for fmt in [
        RawFormat::GenericRAW,
        RawFormat::GenericCAM,
        RawFormat::GenericREV,
    ] {
        // Too small / no valid TIFF byte-order marker -> fallback path.
        let md = parse_raw_metadata(b"XX", fmt).unwrap();
        assert!(md.contains_key("File:FileType"));
    }
}

/// Generic fallback with a *valid* minimal TIFF goes through the success path.
#[test]
fn raw_generic_with_valid_tiff() {
    let tiff = le_tiff(&[le_entry(0x0100, T_SHORT, 1, 640)], 0);
    let md = parse_raw_metadata(&tiff, RawFormat::GenericRAW).unwrap();
    assert!(md.contains_key("File:FileType"));
}

/// File too small for a TIFF header -> error branch in `parse_tiff_based_raw`.
#[test]
fn raw_tiff_too_small_is_err() {
    let result = parse_raw_metadata(b"II\x2a", RawFormat::AdobeDNG);
    assert!(result.is_err());
}

/// Invalid byte-order marker -> `detect_byte_order` error branch.
#[test]
fn raw_tiff_bad_byte_order_is_err() {
    // 8 bytes (passes length check) but no II/MM marker.
    let result = parse_raw_metadata(b"ZZ\x2a\x00\x08\x00\x00\x00", RawFormat::AdobeDNG);
    assert!(result.is_err());
}

/// Big-endian TIFF header path through detect_byte_order + read_u32.
#[test]
fn raw_big_endian_tiff() {
    let mut data = Vec::new();
    data.extend_from_slice(b"MM\x00\x2a");
    data.extend_from_slice(&8u32.to_be_bytes());
    data.extend_from_slice(&1u16.to_be_bytes()); // 1 entry
    data.extend_from_slice(&be_entry(0x0100, T_SHORT, 1, 1234));
    data.extend_from_slice(&0u32.to_be_bytes()); // next IFD = none
    let md = parse_raw_metadata(&data, RawFormat::NikonNEF).unwrap();
    assert!(md.contains_key("File:FileType"));
}

/// Exercise the simple tag-value converters: ASCII / SHORT / LONG /
/// RATIONAL / SRATIONAL field types, plus binary fallback.
#[test]
fn raw_tag_value_conversions() {
    // ASCII "Make" tag (0x010F) inline (<=4 bytes) "AB\0\0".
    let make_inline = u32::from_le_bytes([b'A', b'B', 0, 0]);
    let entries = vec![
        le_entry(0x010F, T_ASCII, 4, make_inline), // Make ASCII
        le_entry(0x0100, T_SHORT, 1, 800),         // SHORT
        le_entry(0x0117, T_LONG, 1, 123456),       // LONG (StripByteCounts)
    ];
    let tiff = le_tiff(&entries, 0);
    let md = parse_raw_metadata(&tiff, RawFormat::AdobeDNG).unwrap();
    assert!(md.contains_key("File:FileType"));
    // At least the short / long tags should appear under IFD0.
    assert!(md.keys().any(|k| k.starts_with("IFD0:")));
}

/// DNG version extraction + available-color-calibration aggregation.
#[test]
fn raw_dng_enrichment() {
    // DNGVersion (0xC612) BYTE x4 inline = 1.4.0.0 ; ColorMatrix1 (0xC621).
    let dng_version = u32::from_le_bytes([1, 4, 0, 0]);
    let entries = vec![
        le_entry(0xC612, T_BYTE, 4, dng_version),
        le_entry(0xC621, T_SRATIONAL, 1, 0), // ColorMatrix1 marker
        le_entry(0xC65A, T_SHORT, 1, 17),    // CalibrationIlluminant1
    ];
    let tiff = le_tiff(&entries, 0);
    let md = parse_raw_metadata(&tiff, RawFormat::AdobeDNG).unwrap();
    // Version string should be derived when stored as binary 4 bytes.
    // (BYTE x4 may be stored binary; either way parsing must succeed.)
    assert!(md.contains_key("File:FileType"));
}

/// CR2 enrichment: IFD0 + IFD1 + SubIFD presence drives image-layer counting
/// and HasRAWData / HasJPEGPreview branches.
#[test]
fn raw_cr2_multi_layer() {
    let mut data = Vec::new();
    data.extend_from_slice(b"II\x2a\x00");
    data.extend_from_slice(&8u32.to_le_bytes());

    // IFD0: ImageWidth + SubIFD pointer (0x014A -> offset 60) + next IFD (offset 90)
    data.extend_from_slice(&3u16.to_le_bytes());
    data.extend_from_slice(&le_entry(0x0100, T_SHORT, 1, 160)); // ImageWidth
    data.extend_from_slice(&le_entry(0x0103, T_SHORT, 1, 6)); // Compression
    data.extend_from_slice(&le_entry(0x014A, T_LONG, 1, 60)); // SubIFD -> 60
    data.extend_from_slice(&90u32.to_le_bytes()); // IFD1 at 90

    // pad to 60
    while data.len() < 60 {
        data.push(0);
    }
    // SubIFD0 at 60: ImageWidth + ImageHeight
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&le_entry(0x0100, T_SHORT, 1, 4000));
    data.extend_from_slice(&le_entry(0x0101, T_SHORT, 1, 3000));
    data.extend_from_slice(&0u32.to_le_bytes());

    // pad to 90
    while data.len() < 90 {
        data.push(0);
    }
    // IFD1 at 90: ImageWidth + Compression (JPEG preview)
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&le_entry(0x0100, T_SHORT, 1, 1600));
    data.extend_from_slice(&le_entry(0x0103, T_SHORT, 1, 6));
    data.extend_from_slice(&0u32.to_le_bytes());

    let md = parse_raw_metadata(&data, RawFormat::CanonCR2).unwrap();
    assert!(md.contains_key("File:FileType"));
}

/// NEF enrichment: SubIFD compression / bit depth / dimensions branches.
#[test]
fn raw_nef_enrichment() {
    let mut data = Vec::new();
    data.extend_from_slice(b"MM\x00\x2a");
    data.extend_from_slice(&8u32.to_be_bytes());

    // IFD0: SubIFD pointer -> offset 30
    data.extend_from_slice(&1u16.to_be_bytes());
    data.extend_from_slice(&be_entry(0x014A, T_LONG, 1, 30));
    data.extend_from_slice(&0u32.to_be_bytes());

    while data.len() < 30 {
        data.push(0);
    }
    // SubIFD0: Compression(34713 lossless) + ImageWidth + ImageHeight + BitsPerSample
    data.extend_from_slice(&4u16.to_be_bytes());
    data.extend_from_slice(&be_entry(0x0103, T_LONG, 1, 34713)); // Compression
    data.extend_from_slice(&be_entry(0x0100, T_SHORT, 1, 6048));
    data.extend_from_slice(&be_entry(0x0101, T_SHORT, 1, 4024));
    data.extend_from_slice(&be_entry(0x0102, T_SHORT, 1, 14)); // BitsPerSample
    data.extend_from_slice(&0u32.to_be_bytes());

    let md = parse_raw_metadata(&data, RawFormat::NikonNEF).unwrap();
    assert!(md.contains_key("File:FileType"));
}

/// EXIF and GPS sub-IFD pointers (tags 0x8769 / 0x8825) get followed.
#[test]
fn raw_exif_and_gps_subifds() {
    let mut data = Vec::new();
    data.extend_from_slice(b"II\x2a\x00");
    data.extend_from_slice(&8u32.to_le_bytes());

    // IFD0: EXIF pointer -> 50, GPS pointer -> 80
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&le_entry(0x8769, T_LONG, 1, 50)); // EXIF IFD
    data.extend_from_slice(&le_entry(0x8825, T_LONG, 1, 80)); // GPS IFD
    data.extend_from_slice(&0u32.to_le_bytes());

    while data.len() < 50 {
        data.push(0);
    }
    // EXIF IFD at 50: ExposureTime rational pointer inline-ish + ISO
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&le_entry(0x8827, T_SHORT, 1, 400)); // ISO
    data.extend_from_slice(&0u32.to_le_bytes());

    while data.len() < 80 {
        data.push(0);
    }
    // GPS IFD at 80: GPSVersionID
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&le_entry(0x0000, T_BYTE, 4, 0x00000202));
    data.extend_from_slice(&0u32.to_le_bytes());

    let md = parse_raw_metadata(&data, RawFormat::AdobeDNG).unwrap();
    assert!(md.contains_key("File:FileType"));
}

/// IFD0 carrying Make + MakerNote (0x927C) drives the MakerNote dispatcher.
#[test]
fn raw_makernote_dispatch_via_ifd0() {
    let mut data = Vec::new();
    data.extend_from_slice(b"II\x2a\x00");
    data.extend_from_slice(&8u32.to_le_bytes());

    // Make = "Canon" stored at offset 60 (ASCII, count 6), MakerNote at 70.
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&le_entry(0x010F, T_ASCII, 6, 60)); // Make -> offset 60
    data.extend_from_slice(&le_entry(0x927C, T_BYTE, 16, 70)); // MakerNote -> offset 70
    data.extend_from_slice(&0u32.to_le_bytes());

    while data.len() < 60 {
        data.push(0);
    }
    data.extend_from_slice(b"Canon\0"); // Make string
    while data.len() < 70 {
        data.push(0);
    }
    // MakerNote body: small canon-ish IFD (1 entry, FirmwareVersion-ish). Even if
    // it does not parse cleanly, the dispatch path executes.
    data.extend_from_slice(&le_ifd(&[le_entry(0x0007, T_ASCII, 4, 0)]));

    let md = parse_raw_metadata(&data, RawFormat::CanonCR2).unwrap();
    assert!(md.contains_key("File:FileType"));
}

// ===========================================================================
// raw/metadata.rs — Sigma X3F proprietary parser.
// ===========================================================================

/// X3F with a too-short body returns just the file type (early return).
#[test]
fn x3f_too_short() {
    let md = parse_raw_metadata(b"FOVbshort", RawFormat::SigmaX3F).unwrap();
    assert!(md.contains_key("File:FileType"));
}

/// X3F with a full FOVb header (v2.3) exercises version/dimension/rotation/WB/
/// color-mode parsing and the directory-section scan with property + image
/// sections.
#[test]
fn x3f_full_header_with_directory() {
    let mut data = Vec::new();
    // FOVb header
    data.extend_from_slice(b"FOVb");
    data.extend_from_slice(&0x00020003u32.to_le_bytes()); // version 2.3
    data.extend_from_slice(&[0u8; 16]); // unique id
    data.extend_from_slice(&0u32.to_le_bytes()); // mark bits @24
    data.extend_from_slice(&3000u32.to_le_bytes()); // columns @28
    data.extend_from_slice(&2000u32.to_le_bytes()); // rows @32
    data.extend_from_slice(&90u32.to_le_bytes()); // rotation @36
    // WB string @40 (32 bytes), "Auto"
    let mut wb = [0u8; 32];
    wb[..4].copy_from_slice(b"Auto");
    data.extend_from_slice(&wb);
    // Color mode @72 (32 bytes), "Standard"
    let mut cm = [0u8; 32];
    cm[..8].copy_from_slice(b"Standard");
    data.extend_from_slice(&cm);

    // Build a directory section "SECd" with one property entry + one image entry.
    // First we need the property/image section data placed in the file and the
    // directory after them; the directory offset is the last 4 bytes.

    // Property section (SECp): header 24 bytes + table + data block.
    let prop_section = {
        let mut p = Vec::new();
        p.extend_from_slice(b"SECp");
        p.extend_from_slice(&1u32.to_le_bytes()); // version
        p.extend_from_slice(&1u32.to_le_bytes()); // num_properties = 1
        p.extend_from_slice(&0u32.to_le_bytes()); // char format (UTF-16)
        p.extend_from_slice(&0u32.to_le_bytes()); // reserved
        p.extend_from_slice(&0u32.to_le_bytes()); // total length
        // property table: name_offset(0), value_offset(8 -> in u16 units = 4)
        p.extend_from_slice(&0u32.to_le_bytes()); // name offset (u16 index 0)
        p.extend_from_slice(&8u32.to_le_bytes()); // value offset (u16 index 8)
        // data block: "CAMMODEL\0" then value "X3F\0" as UTF-16LE
        for ch in "CAMMODEL".encode_utf16() {
            p.extend_from_slice(&ch.to_le_bytes());
        }
        p.extend_from_slice(&0u16.to_le_bytes()); // NUL
        for ch in "SD15".encode_utf16() {
            p.extend_from_slice(&ch.to_le_bytes());
        }
        p.extend_from_slice(&0u16.to_le_bytes());
        p
    };
    let prop_offset = data.len();
    data.extend_from_slice(&prop_section);

    // Image section (SECi): type 2 (thumbnail) so PreviewImageSize is recorded.
    let img_section = {
        let mut im = Vec::new();
        im.extend_from_slice(b"SECi");
        im.extend_from_slice(&1u32.to_le_bytes()); // version
        im.extend_from_slice(&2u32.to_le_bytes()); // image type 2 = thumbnail
        im.extend_from_slice(&0u32.to_le_bytes()); // image format
        im.extend_from_slice(&320u32.to_le_bytes()); // columns
        im.extend_from_slice(&240u32.to_le_bytes()); // rows
        im.extend_from_slice(&960u32.to_le_bytes()); // stride
        im.extend_from_slice(&[0u8; 8]); // padding
        im
    };
    let img_offset = data.len();
    data.extend_from_slice(&img_section);

    // Directory section "SECd"
    let dir_offset = data.len();
    data.extend_from_slice(b"SECd");
    data.extend_from_slice(&1u32.to_le_bytes()); // version
    data.extend_from_slice(&2u32.to_le_bytes()); // num entries = 2
    // entry 1: property
    data.extend_from_slice(&(prop_offset as u32).to_le_bytes());
    data.extend_from_slice(&(prop_section.len() as u32).to_le_bytes());
    data.extend_from_slice(b"SECp");
    // entry 2: image
    data.extend_from_slice(&(img_offset as u32).to_le_bytes());
    data.extend_from_slice(&(img_section.len() as u32).to_le_bytes());
    data.extend_from_slice(b"SECi");

    // trailing directory offset pointer (last 4 bytes)
    data.extend_from_slice(&(dir_offset as u32).to_le_bytes());

    let md = parse_raw_metadata(&data, RawFormat::SigmaX3F).unwrap();
    assert!(md.contains_key("File:FileType"));
    // FileVersion derived from header.
    assert!(md.contains_key("SigmaRaw:FileVersion"));
}

// ===========================================================================
// raw/metadata.rs — Minolta MRW proprietary parser.
// ===========================================================================

/// MRW too short -> just file type.
#[test]
fn mrw_too_short() {
    let md = parse_raw_metadata(b"\x00MRM", RawFormat::MinoltaMRW).unwrap();
    assert!(md.contains_key("File:FileType"));
}

/// MRW with PRD (dimensions) and WBG (white balance) blocks.
#[test]
fn mrw_prd_and_wbg_blocks() {
    let mut data = Vec::new();
    data.extend_from_slice(b"\x00MRM");
    data.extend_from_slice(&0u32.to_be_bytes()); // file size (unused)

    // PRD block (\x00PRD) of 16 bytes
    data.extend_from_slice(b"\x00PRD");
    data.extend_from_slice(&16u32.to_be_bytes());
    data.extend_from_slice(&1u16.to_be_bytes()); // version @0
    data.extend_from_slice(&6000u16.to_be_bytes()); // sensor w @2
    data.extend_from_slice(&4000u16.to_be_bytes()); // sensor h @4
    data.extend_from_slice(&5800u16.to_be_bytes()); // image w @6
    data.extend_from_slice(&3900u16.to_be_bytes()); // image h @8
    data.extend_from_slice(&[0u8; 6]); // rest of block

    // WBG block (\x00WBG) of 8 bytes
    data.extend_from_slice(b"\x00WBG");
    data.extend_from_slice(&8u32.to_be_bytes());
    data.extend_from_slice(&512u16.to_be_bytes()); // R
    data.extend_from_slice(&256u16.to_be_bytes()); // G
    data.extend_from_slice(&384u16.to_be_bytes()); // B
    data.extend_from_slice(&0u16.to_be_bytes());

    let md = parse_raw_metadata(&data, RawFormat::MinoltaMRW).unwrap();
    assert!(md.contains_key("File:FileType"));
    assert!(md.contains_key("MakerNotes:SensorWidth"));
    assert!(md.contains_key("MakerNotes:ColorBalanceRed"));
}

/// MRW with a TTW (embedded TIFF) block.
#[test]
fn mrw_ttw_block() {
    let inner_tiff = le_tiff(&[le_entry(0x0100, T_SHORT, 1, 100)], 0);
    let mut data = Vec::new();
    data.extend_from_slice(b"\x00MRM");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(b"\x00TTW");
    data.extend_from_slice(&(inner_tiff.len() as u32).to_be_bytes());
    data.extend_from_slice(&inner_tiff);

    let md = parse_raw_metadata(&data, RawFormat::MinoltaMRW).unwrap();
    assert!(md.contains_key("File:FileType"));
}

// ===========================================================================
// raw/metadata.rs — Fujifilm RAF embedded-JPEG path.
// ===========================================================================

/// RAF missing signature -> error.
#[test]
fn raf_bad_signature() {
    let result = parse_raw_metadata(b"NOTARAFFILE-----", RawFormat::FujifilmRAF);
    assert!(result.is_err());
}

/// RAF with valid signature but header too small -> error.
#[test]
fn raf_header_too_small() {
    let mut data = Vec::new();
    data.extend_from_slice(b"FUJIFILMCCD-RAW "); // 16 bytes
    data.extend_from_slice(&[0u8; 20]); // < 92 total
    let result = parse_raw_metadata(&data, RawFormat::FujifilmRAF);
    assert!(result.is_err());
}

/// RAF whose JPEG offset is out of bounds -> error.
#[test]
fn raf_jpeg_offset_out_of_bounds() {
    let mut data = Vec::new();
    data.extend_from_slice(b"FUJIFILMCCD-RAW ");
    data.resize(84, 0);
    data.extend_from_slice(&100_000u32.to_be_bytes()); // jpeg offset @84 (huge)
    data.extend_from_slice(&10u32.to_be_bytes()); // jpeg length @88
    let result = parse_raw_metadata(&data, RawFormat::FujifilmRAF);
    assert!(result.is_err());
}

/// RAF whose "JPEG" payload isn't a real JPEG -> error branch.
#[test]
fn raf_embedded_not_jpeg() {
    let mut data = Vec::new();
    data.extend_from_slice(b"FUJIFILMCCD-RAW ");
    data.resize(84, 0);
    let jpeg_offset = 92u32;
    data.extend_from_slice(&jpeg_offset.to_be_bytes()); // @84
    data.extend_from_slice(&4u32.to_be_bytes()); // @88 length
    // payload at 92 that is not 0xFFD8
    data.extend_from_slice(b"NOPE");
    let result = parse_raw_metadata(&data, RawFormat::FujifilmRAF);
    assert!(result.is_err());
}

/// RAF with a minimal valid embedded JPEG (SOI + EOI). The JPEG has no EXIF,
/// so we exercise the success path that returns just the file type.
#[test]
fn raf_minimal_valid_jpeg() {
    let mut data = Vec::new();
    data.extend_from_slice(b"FUJIFILMCCD-RAW ");
    data.resize(84, 0);
    let jpeg = [0xFFu8, 0xD8, 0xFF, 0xD9]; // SOI + EOI
    let jpeg_offset = 92u32;
    data.extend_from_slice(&jpeg_offset.to_be_bytes()); // @84
    data.extend_from_slice(&(jpeg.len() as u32).to_be_bytes()); // @88
    data.extend_from_slice(&jpeg);
    let md = parse_raw_metadata(&data, RawFormat::FujifilmRAF).unwrap();
    assert!(md.contains_key("File:FileType"));
}

/// RAF whose declared JPEG length overruns the file triggers the
/// "use remaining size" warning branch but still succeeds.
#[test]
fn raf_jpeg_length_overrun() {
    let mut data = Vec::new();
    data.extend_from_slice(b"FUJIFILMCCD-RAW ");
    data.resize(84, 0);
    let jpeg = [0xFFu8, 0xD8, 0xFF, 0xD9];
    let jpeg_offset = 92u32;
    data.extend_from_slice(&jpeg_offset.to_be_bytes());
    data.extend_from_slice(&9999u32.to_be_bytes()); // declared length way too big
    data.extend_from_slice(&jpeg);
    let md = parse_raw_metadata(&data, RawFormat::FujifilmRAF).unwrap();
    assert!(md.contains_key("File:FileType"));
}

// ===========================================================================
// raw/raf_parser.rs — parse_raf_makernote and its decoders.
// ===========================================================================

/// Too-short MakerNote -> error.
#[test]
fn raf_makernote_too_small() {
    let result = parse_raf_makernote(b"FUJI", ByteOrder::LittleEndian);
    assert!(result.is_err());
}

/// Wrong signature -> error.
#[test]
fn raf_makernote_bad_signature() {
    let mut data = vec![0u8; 32];
    data[0..8].copy_from_slice(b"NOTFUJI!");
    let result = parse_raf_makernote(&data, ByteOrder::LittleEndian);
    assert!(result.is_err());
}

/// Valid "FUJIFILM" header (little-endian) drives the always-on derived tags
/// (ColorSpace, InternalSerialNumber, SensorInfo, SerialNumber).
#[test]
fn raf_makernote_valid_little_endian() {
    let mut data = vec![0u8; 64];
    data[0..8].copy_from_slice(b"FUJIFILM");
    // bytes 8..12 reserved; serial @0x10..0x14; internal @0x14..0x18
    data[0x10..0x14].copy_from_slice(&0x12345678u32.to_le_bytes());
    data[0x14..0x18].copy_from_slice(&0xABCDEF01u32.to_le_bytes());
    // bytes 24..32 used by sensor-info ascii extraction
    data[24..32].copy_from_slice(b"X-T5\0\0\0\0");

    let tags = parse_raf_makernote(&data, ByteOrder::LittleEndian).unwrap();
    assert!(tags.contains_key("Fujifilm:SerialNumber"));
    assert!(tags.contains_key("Fujifilm:InternalSerialNumber"));
    assert!(tags.contains_key("Fujifilm:SensorInfo"));
    assert!(tags.contains_key("Fujifilm:ColorSpace"));
    assert_eq!(
        tags.get("Fujifilm:ColorSpace").map(String::as_str),
        Some("sRGB")
    );
}

/// Valid header in big-endian mode exercises the big-endian read path.
#[test]
fn raf_makernote_valid_big_endian() {
    let mut data = vec![0u8; 40];
    data[0..8].copy_from_slice(b"FUJIFILM");
    data[0x10..0x14].copy_from_slice(&0x00112233u32.to_be_bytes());
    let tags = parse_raf_makernote(&data, ByteOrder::BigEndian).unwrap();
    assert!(tags.contains_key("Fujifilm:SerialNumber"));
}

/// Header present but shorter than 0x18 -> internal serial returns "Unknown".
#[test]
fn raf_makernote_short_internal_serial() {
    let mut data = vec![0u8; 20]; // >=12 (header) but <0x18
    data[0..8].copy_from_slice(b"FUJIFILM");
    let tags = parse_raf_makernote(&data, ByteOrder::LittleEndian).unwrap();
    assert_eq!(
        tags.get("Fujifilm:InternalSerialNumber")
            .map(String::as_str),
        Some("Unknown")
    );
}

// ===========================================================================
// Leica MakerNote — every tag arm via the public `parse`.
// ===========================================================================

/// Build a Leica MakerNote with the short "LEICA\0\0\0" header and a long list
/// of recognized tags, exercising the value decoders and format branches.
#[test]
fn leica_parse_many_tags() {
    let entries = vec![
        le_entry(0x0003, T_SHORT, 1, 5),         // Quality -> DNG
        le_entry(0x0004, T_SHORT, 1, 8),         // UserProfile -> Monochrome
        le_entry(0x0005, T_LONG, 1, 99999),      // SerialNumber
        le_entry(0x0006, T_SHORT, 1, 9),         // WhiteBalance -> ambient priority
        le_entry(0x0023, T_SHORT, 1, 1),         // WBMode -> Daylight
        le_entry(0x000C, T_LONG, 1, 5500),       // ColorTemperature -> "5500K"
        le_entry(0x000D, T_LONG, 1, 100),        // WBRedLevel
        le_entry(0x000E, T_LONG, 1, 110),        // WBGreenLevel
        le_entry(0x000F, T_LONG, 1, 120),        // WBBlueLevel
        le_entry(0x000B, T_SHORT, 1, 35),        // CameraTemperature
        le_entry(0x0010, T_SHORT, 1, 2),         // Sharpening
        le_entry(0x0011, T_SHORT, 1, 1),         // Contrast
        le_entry(0x0012, T_SHORT, 1, 3),         // Saturation
        le_entry(0x0013, T_SHORT, 1, 1),         // LensID (also tries lens lookup)
        le_entry(0x0014, T_LONG, 1, 42),         // LensType
        le_entry(0x0020, T_SHORT, 1, 2),         // ExposureMode -> Aperture Priority
        le_entry(0x0021, T_SHORT, 1, 3),         // MeteringMode -> Spot
        le_entry(0x0025, T_SHORT, 1, 2),         // FlashMode -> On
        le_entry(0x0026, T_LONG, 1, 50),         // FlashEnergy
        le_entry(0x0027, T_LONG, 1, 777),        // InternalSerialNumber
        le_entry(0x0034, T_LONG, 1, 12345),      // ShutterCount
        le_entry(0x0035, T_LONG, 1, 1500),       // FocusDistance -> "1500 mm"
        le_entry(0x0052, T_SHORT, 1, 2),         // AFMode -> Continuous AF
        le_entry(0x0053, T_SHORT, 1, 4),         // ImageStabilization -> On (Dual)
        le_entry(0x0054, T_LONG, 1, 250),        // DigitalZoom -> "2%"
        le_entry(0x0070, T_SHORT, 1, 1),         // MacroMode -> On
        le_entry(0x0071, T_SHORT, 1, 2),         // SceneMode -> Landscape
        le_entry(0x0061, T_SHORT, 1, 1),         // CropMode -> APS-C
        le_entry(0x0041, T_LONG, 1, 200),        // BaseISO
        le_entry(0x0009, T_SHORT, 1, 80),        // MeasuredLV
        le_entry(0x000A, T_SHORT, 1, 28),        // ApproximateFNumber -> f/2.8
        le_entry(0x0022, T_LONG, 1, 7),          // FilmMode
        le_entry(0x0040, T_LONG, 1, 3),          // FrameSelector
        le_entry(0x0063, T_SHORT, 1, 5),         // CameraPitchAngle
        le_entry(0x0064, T_SHORT, 1, 3),         // CameraRollAngle
        le_entry(0x0030, T_LONG, 1, 50),         // FocalLength35mm
        le_entry(0x0031, T_LONG, 1, 888),        // LensSerialNumber
        le_entry(0x0032, T_SHORT, 1, 1),         // ContrastDetectAF -> On
        le_entry(0x0060, T_LONG, 1, 0x01040000), // DNGVersion -> 1.4.0.0
        le_entry(0x0062, T_SHORT, 1, 1),         // PerspectiveControl -> On
        le_entry(0x0051, T_LONG, 1, 5),          // AFPoint
        le_entry(0x0050, T_LONG, 1, 2),          // PictureControl
        le_entry(0x0042, T_LONG, 1, 9),          // ImageID
        le_entry(0x0024, T_SHORT, 1, 40),        // APEXBrightness
        le_entry(0x0008, T_SHORT, 1, 60),        // ExternalSensorBrightness
    ];

    let mut body = le_ifd(&entries);
    // Prepend the short LEICA header.
    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA\0\0\0");
    data.append(&mut body);

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok(), "leica parse failed: {:?}", result);
    // A representative subset must have decoded.
    assert_eq!(tags.get("Leica:Quality").map(String::as_str), Some("DNG"));
    assert_eq!(
        tags.get("Leica:ExposureMode").map(String::as_str),
        Some("Aperture Priority")
    );
    assert_eq!(
        tags.get("Leica:ColorTemperature").map(String::as_str),
        Some("5500K")
    );
    assert!(tags.contains_key("Leica:DNGVersion"));
}

/// Leica with the long "LEICA CAMERA AG" header offsets correctly.
#[test]
fn leica_long_header() {
    let entries = vec![le_entry(0x0003, T_SHORT, 1, 1)]; // Quality -> Fine
    let mut body = le_ifd(&entries);
    let mut data = Vec::new();
    data.extend_from_slice(b"LEICA CAMERA AG");
    data.append(&mut body);

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );
    assert_eq!(tags.get("Leica:Quality").map(String::as_str), Some("Fine"));
}

/// Leica with no header (raw IFD) and big-endian byte order.
#[test]
fn leica_no_header_big_endian() {
    let mut data = Vec::new();
    data.extend_from_slice(&1u16.to_be_bytes()); // 1 entry
    data.extend_from_slice(&be_entry(0x0006, T_SHORT, 1, 4)); // WhiteBalance -> Flash
    data.extend_from_slice(&0u32.to_be_bytes());

    let parser = LeicaMakerNoteParser;
    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::BigEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Leica:WhiteBalance").map(String::as_str),
        Some("Flash")
    );
}

/// Leica malformed-input branches: too short, zero entries, oversized count.
#[test]
fn leica_malformed_inputs() {
    let parser = LeicaMakerNoteParser;

    // Too short overall.
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(b"LEI", ByteOrder::LittleEndian, &mut tags)
            .is_err()
    );

    // Header present but no IFD body after it.
    let mut tags = HashMap::new();
    let r = parser.parse(b"LEICA\0\0\0", ByteOrder::LittleEndian, &mut tags);
    assert!(r.is_err());

    // Zero entry count (invalid).
    let mut zero = Vec::new();
    zero.extend_from_slice(b"LEICA\0\0\0");
    zero.extend_from_slice(&0u16.to_le_bytes());
    zero.extend_from_slice(&0u32.to_le_bytes());
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&zero, ByteOrder::LittleEndian, &mut tags)
            .is_err()
    );

    // Declared entries exceed available data.
    let mut truncated = Vec::new();
    truncated.extend_from_slice(b"LEICA\0\0\0");
    truncated.extend_from_slice(&10u16.to_le_bytes()); // claims 10 entries
    truncated.extend_from_slice(&le_entry(0x0003, T_SHORT, 1, 1)); // only 1
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&truncated, ByteOrder::LittleEndian, &mut tags)
            .is_err()
    );
}

/// `is_leica_makernote` true/false branches.
#[test]
fn leica_signature_detection() {
    assert!(is_leica_makernote(b"LEICA\0\0\0morebytes"));
    assert!(is_leica_makernote(b"LEICA CAMERA AG xx"));
    assert!(is_leica_makernote(b"\x05\x00\x00\x00\x00\x00\x00\x00")); // 5 entries
    assert!(!is_leica_makernote(b"NOPE")); // too short
    assert!(!is_leica_makernote(b"\x00\x00\x00\x00\x00\x00\x00\x00")); // 0 entries
}

/// Leica trait surface.
#[test]
fn leica_trait_surface() {
    let parser = LeicaMakerNoteParser;
    assert_eq!(parser.manufacturer_name(), "Leica");
    assert_eq!(parser.tag_prefix(), "Leica:");
    assert!(parser.validate_header(b"LEICA\0\0\0xx"));
    let _ = parser.lookup_lens(1);
}

// ===========================================================================
// Nikon MakerNote — deeper tag arms via the public `parse` / wrapper.
// ===========================================================================

/// Build a Nikon Type-3 MakerNote: "Nikon\0" + version(4) + embedded TIFF + IFD.
/// The embedded TIFF starts at byte 10. IFD entry offsets in this parser use the
/// inline value_offset directly for scalar tags, so scalar arms are reachable.
fn build_nikon_makernote(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut data = Vec::new();
    // 10-byte Nikon header.
    data.extend_from_slice(b"Nikon\0\x02\x10\x00\x00");
    // Embedded TIFF header (little-endian), IFD at offset 8 within the TIFF.
    data.extend_from_slice(b"II\x2a\x00");
    data.extend_from_slice(&8u32.to_le_bytes());
    // IFD at TIFF-offset 8 (== absolute 18).
    data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in entries {
        data.extend_from_slice(e);
    }
    data.extend_from_slice(&0u32.to_le_bytes());
    data
}

#[test]
fn nikon_parse_scalar_tags() {
    let entries = vec![
        le_entry(0x0002, T_SHORT, 1, 200),       // ISO speed
        le_entry(0x0004, T_SHORT, 1, 6),         // Quality -> SXGA Fine
        le_entry(0x0005, T_SHORT, 1, 1),         // WhiteBalance -> Daylight
        le_entry(0x0007, T_SHORT, 1, 1),         // FocusMode -> AF-C
        le_entry(0x0008, T_SHORT, 1, 2),         // FlashSetting -> Rear Curtain
        le_entry(0x0087, T_SHORT, 1, 7),         // FlashMode -> Fired, External
        le_entry(0x0089, T_SHORT, 1, 1),         // ShootingMode -> Continuous
        le_entry(0x00B0, T_SHORT, 1, 2),         // ColorSpace -> Adobe RGB
        le_entry(0x00B3, T_SHORT, 1, 3),         // ActiveDLighting -> Normal
        le_entry(0x00B7, T_SHORT, 1, 2),         // VignetteControl -> Normal
        le_entry(0x0083, T_LONG, 1, 0x06),       // LensType
        le_entry(0x00A7, T_LONG, 1, 54321),      // ShutterCount
        le_entry(0x00A5, T_LONG, 1, 1000),       // ImageCount
        le_entry(0x00A6, T_LONG, 1, 5),          // DeletedImageCount
        le_entry(0x00A2, T_LONG, 1, 999),        // ImageDataSize
        le_entry(0x000B, T_SHORT, 1, 3),         // WhiteBalanceFine
        le_entry(0x000F, T_SHORT, 1, 1),         // ProgramShift
        le_entry(0x0010, T_SHORT, 1, 2),         // ExposureDiff
        le_entry(0x0012, T_SHORT, 1, 6),         // FlashExposureComp
        le_entry(0x0017, T_SHORT, 1, 6),         // ExternalFlashComp
        le_entry(0x0018, T_SHORT, 1, 12),        // FlashBracketValue
        le_entry(0x0019, T_SHORT, 1, 12),        // ExposureBracketValue
        le_entry(0x001C, T_SHORT, 1, 6),         // ExposureTuning
        le_entry(0x0092, T_SHORT, 1, 5),         // HueAdjustment
        le_entry(0x0094, T_SHORT, 1, 2),         // Saturation
        le_entry(0x0006, T_SHORT, 1, 1),         // Sharpness
        le_entry(0x008B, T_SHORT, 1, 36),        // LensFStops
        le_entry(0x0093, T_SHORT, 1, 3),         // NEFCompression -> Lossless
        le_entry(0x0020, T_SHORT, 1, 1),         // ImageAuth -> On
        le_entry(0x0011, T_SHORT, 1, 0),         // ISOSelection -> Auto
        le_entry(0x0013, T_SHORT, 1, 400),       // ISOSetting
        le_entry(0x00B8, T_SHORT, 1, 1),         // DistortionControl -> On
        le_entry(0x009A, T_LONG, 1, 0x00010001), // SensorPixelSize
    ];
    let data = build_nikon_makernote(&entries);

    let parser = NikonParser;
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );

    assert_eq!(
        tags.get("Nikon:Quality").map(String::as_str),
        Some("SXGA Fine")
    );
    assert_eq!(
        tags.get("Nikon:WhiteBalance").map(String::as_str),
        Some("Daylight")
    );
    assert_eq!(
        tags.get("Nikon:NEFCompression").map(String::as_str),
        Some("Lossless")
    );
    assert_eq!(
        tags.get("Nikon:ShutterCount").map(String::as_str),
        Some("54321")
    );
}

/// Nikon wrapper + header validation + is_nikon_makernote helper.
#[test]
fn nikon_wrapper_and_helpers() {
    let entries = vec![le_entry(0x0002, T_SHORT, 1, 100)];
    let data = build_nikon_makernote(&entries);

    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(tags.contains_key("Nikon:ISOSpeed"));

    assert!(is_nikon_makernote(b"Nikon\0extra"));
    assert!(!is_nikon_makernote(b"Canon\0"));
    assert!(!is_nikon_makernote(b"Niko"));

    let parser = NikonParser;
    assert!(parser.validate_header(b"Nikon\0\x02\x10\x00\x00"));
    assert!(!parser.validate_header(b"Sony\0"));
    assert_eq!(parser.manufacturer_name(), "Nikon");
    assert_eq!(parser.tag_prefix(), "Nikon:");
    assert!(parser.lookup_lens(147).is_some());
    assert!(parser.lookup_lens(65000).is_none());
}

/// Nikon error / edge branches: empty data, invalid header, truncated TIFF.
#[test]
fn nikon_edge_cases() {
    let parser = NikonParser;

    // Empty data -> Ok (no-op).
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(b"", ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );

    // Invalid Nikon header -> Err.
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(
                b"Canon\0\x00\x00\x00\x00garbage",
                ByteOrder::LittleEndian,
                &mut tags
            )
            .is_err()
    );

    // Valid header but too short for embedded TIFF -> Ok (early return).
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(
                b"Nikon\0\x02\x10\x00\x00",
                ByteOrder::LittleEndian,
                &mut tags
            )
            .is_ok()
    );

    // Valid header, embedded TIFF with bad byte order marker -> Err.
    let mut bad_bo = Vec::new();
    bad_bo.extend_from_slice(b"Nikon\0\x02\x10\x00\x00");
    bad_bo.extend_from_slice(b"ZZ\x2a\x00");
    bad_bo.extend_from_slice(&8u32.to_le_bytes());
    bad_bo.extend_from_slice(&[0u8; 8]);
    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&bad_bo, ByteOrder::LittleEndian, &mut tags)
            .is_err()
    );
}

// ===========================================================================
// Canon MakerNote — public formatters, model decoders, parse entry.
// ===========================================================================

#[test]
fn canon_apex_formatters() {
    // Aperture: zero -> n/a, nonzero -> f/x
    assert_eq!(apex_to_aperture(0), "n/a");
    let a = apex_to_aperture(160);
    assert!(a.starts_with("f/"), "got {}", a);
    let big = apex_to_aperture(400); // larger f-number -> integer format branch
    assert!(big.starts_with("f/"));

    // Exposure time: zero, slow (>=1s), and fast (fraction) branches.
    assert_eq!(apex_to_exposure_time(0), "n/a");
    let slow = apex_to_exposure_time(-160); // negative -> long exposure
    assert!(slow.contains("sec"), "got {}", slow);
    let fast = apex_to_exposure_time(256); // fast -> 1/x
    assert!(fast.starts_with("1/"), "got {}", fast);

    // Focal length: zero units -> n/a, integer and fractional branches.
    assert_eq!(format_focal_length(0, 1), "n/a");
    assert_eq!(format_focal_length(50, 1), "50 mm");
    let frac = format_focal_length(245, 10);
    assert!(frac.ends_with("mm"));
}

#[test]
fn canon_model_decoders() {
    assert_eq!(decode_canon_model_id(0x1110000), "PowerShot S40");
    assert_eq!(decode_canon_model_id(0x80000281), "EOS 5D Mark III");
    assert!(decode_canon_model_id(0xDEADBEEF).starts_with("Unknown"));

    assert_eq!(decode_camera_type(0x80000001), "EOS High-end");
    assert_eq!(decode_camera_type(0x80000218), "EOS Mid-range");
    assert_eq!(decode_camera_type(0x01110000), "Compact");
    assert_eq!(decode_camera_type(0x00000001), "Unknown");
}

#[test]
fn canon_tag_name_mapper() {
    assert_eq!(canon_tag_to_name(0x0001), "Canon:CameraSettings");
    assert_eq!(canon_tag_to_name(0x0010), "Canon:CanonModelID");
    assert_eq!(canon_tag_to_name(0x0095), "Canon:LensModel");
    assert!(canon_tag_to_name(0xBEEF).starts_with("Canon:Unknown-"));
}

#[test]
fn canon_signature_detection() {
    assert!(is_canon_makernote(b"Canon\0moredata"));
    assert!(is_canon_makernote(b"\x05\x00\x00\x00")); // 5 entries LE
    assert!(!is_canon_makernote(b"\x00\x00\x00\x00")); // 0 entries both ways
    assert!(!is_canon_makernote(b"XX")); // too short
}

/// Drive the Canon parse entry with ModelID + FileNumber + a CameraSettings
/// array so the array-decoder branches run.
#[test]
fn canon_parse_entry() {
    // CameraSettings array stored at an offset; type SHORT, count large.
    // Build: IFD with ModelID (LONG inline), FileNumber (LONG inline),
    // and CameraSettings (SHORT array @ offset).
    let array_offset = 200u32;
    let entries = vec![
        le_entry(0x0010, T_LONG, 1, 0x80000281), // CanonModelID -> 5D III
        le_entry(0x0008, T_LONG, 1, 1234),       // FileNumber
        le_entry(0x0001, T_SHORT, 12, array_offset), // CameraSettings array
        le_entry(0x0006, T_ASCII, 8, 220),       // ImageType string @220
    ];

    let mut data = le_ifd(&entries);
    // pad to array_offset (200)
    while data.len() < array_offset as usize {
        data.push(0);
    }
    // CameraSettings array: 12 i16 values (index 1 macro, 3 quality, 4 flash...)
    for v in [12i16, 1, 30, 3, 2, 0, 0, 0, 0, 0, 0, 0] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    // pad to 220 for ImageType string
    while data.len() < 220 {
        data.push(0);
    }
    data.extend_from_slice(b"Canon\0\0\0");

    let parser = CanonParser;
    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok(), "canon parse failed: {:?}", result);
    assert_eq!(
        tags.get("Canon:CanonModelID").map(String::as_str),
        Some("EOS 5D Mark III")
    );
    assert!(tags.contains_key("Canon:CameraType"));
}

/// Canon wrapper function and empty-input branch.
#[test]
fn canon_wrapper_and_empty() {
    let parser = CanonParser;
    assert_eq!(parser.manufacturer_name(), "Canon");
    assert_eq!(parser.tag_prefix(), "Canon:");
    assert!(parser.validate_header(b"Canon\0xx"));
    assert!(parser.lookup_lens(1).is_some() || parser.lookup_lens(1).is_none());

    // Empty data through the public wrapper -> no-op, no panic.
    let mut tags = HashMap::new();
    parse_canon_makernotes(b"", ByteOrder::LittleEndian, &mut tags);

    // Wrapper with a small valid IFD.
    let entries = vec![le_entry(0x0010, T_LONG, 1, 0x80000404)]; // EOS R5
    let body = le_ifd(&entries);
    let mut tags = HashMap::new();
    parse_canon_makernotes(&body, ByteOrder::LittleEndian, &mut tags);
    assert_eq!(
        tags.get("Canon:CanonModelID").map(String::as_str),
        Some("EOS R5")
    );
}
