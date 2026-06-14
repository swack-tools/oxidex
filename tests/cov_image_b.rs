//! Coverage tests for image parsers: BPG, EXR, FLIF, PSD, ICO, SVG, BMP.
//!
//! These tests build synthetic byte buffers valid enough to drive each parser
//! deep into its header / chunk / attribute parsing logic, and also exercise the
//! production detection + dispatch path via `read_metadata` on tempfiles.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::read_metadata;
use oxidex::parsers::image::bmp::{BMPParser, parse_bmp_metadata};
use oxidex::parsers::image::bpg::{BPGParser, parse_bpg_metadata};
use oxidex::parsers::image::exr::{EXRParser, parse_exr_metadata};
use oxidex::parsers::image::flif::{FLIFParser, parse_flif_metadata};
use oxidex::parsers::image::ico::{ICOParser, parse_ico_metadata};
use oxidex::parsers::image::psd::{PSDParser, parse_psd_metadata};
use oxidex::parsers::image::svg::{SVGParser, parse_svg_metadata};

use oxidex::core::{FileFormat, FileReader, FormatParser};
use std::io::Write;
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Build a minimal little-endian TIFF/EXIF blob.
///
/// Layout:
///   - "II" + 0x002A (magic) + IFD0 offset (8)
///   - IFD0: entry_count(2) + entries(12 each) + next-IFD offset(4, = 0)
///
/// Entries included:
///   - 0x0112 Orientation (SHORT, count 1, value 1) inline
///   - 0x010E ImageDescription (ASCII, count 4, "Hi\0") inline (<=4 bytes)
///   - 0x8769 ExifIFD pointer (LONG) -> sub IFD with one ExifVersion tag
fn build_exif_blob() -> Vec<u8> {
    // We'll construct buffers and patch offsets.
    // TIFF header is 8 bytes; IFD0 starts at offset 8.
    let mut buf: Vec<u8> = Vec::new();

    // --- TIFF header ---
    buf.extend_from_slice(b"II"); // little-endian
    buf.extend_from_slice(&0x002Au16.to_le_bytes()); // magic
    buf.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset

    // IFD0 has 3 entries.
    let entry_count: u16 = 3;
    // IFD0 size: 2 + 3*12 + 4 = 42 bytes. Starts at 8, ends at 50.
    let exif_ifd_offset: u32 = 8 + 2 + (entry_count as u32 * 12) + 4; // = 50

    buf.extend_from_slice(&entry_count.to_le_bytes());

    // Entry 1: 0x0112 Orientation, SHORT(3), count 1, value 1
    buf.extend_from_slice(&0x0112u16.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&1u32.to_le_bytes()); // value 1 inline

    // Entry 2: 0x010E ImageDescription, ASCII(2), count 3, "Hi\0"
    buf.extend_from_slice(&0x010Eu16.to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    buf.extend_from_slice(&3u32.to_le_bytes()); // count (fits inline, <=4)
    let mut desc = *b"Hi\0\0";
    desc[2] = 0;
    buf.extend_from_slice(&desc);

    // Entry 3: 0x8769 ExifIFD pointer, LONG(4), count 1, value = exif_ifd_offset
    buf.extend_from_slice(&0x8769u16.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // LONG
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&exif_ifd_offset.to_le_bytes());

    // Next-IFD offset (none)
    buf.extend_from_slice(&0u32.to_le_bytes());

    debug_assert_eq!(buf.len() as u32, exif_ifd_offset);

    // --- ExifIFD (sub IFD) with 1 entry: 0x9000 ExifVersion, UNDEFINED(7), count 4 ---
    let sub_count: u16 = 1;
    buf.extend_from_slice(&sub_count.to_le_bytes());
    buf.extend_from_slice(&0x9000u16.to_le_bytes());
    buf.extend_from_slice(&7u16.to_le_bytes()); // UNDEFINED
    buf.extend_from_slice(&4u32.to_le_bytes()); // count 4 (inline)
    buf.extend_from_slice(b"0230"); // version
    buf.extend_from_slice(&0u32.to_le_bytes()); // next-IFD = 0

    buf
}

/// ue7 encode a usize (7 bits per byte, MSB = continuation).
fn ue7(mut v: usize) -> Vec<u8> {
    // Collect 7-bit groups, most-significant first per BPG ue7 (little-endian shift in parser).
    // The parser does: value |= (byte & 0x7F) << shift; shift += 7; first byte is least significant.
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte); // MSB clear -> terminate
            break;
        } else {
            out.push(byte | 0x80);
        }
    }
    out
}

/// Write bytes to a temp file with the given extension and run read_metadata.
fn read_via_tempfile(bytes: &[u8], ext: &str) -> oxidex::error::Result<oxidex::core::MetadataMap> {
    let mut tf = NamedTempFile::with_suffix(format!(".{ext}")).expect("tempfile");
    tf.write_all(bytes).expect("write");
    tf.flush().expect("flush");
    read_metadata(tf.path())
}

// ===========================================================================
// BPG
// ===========================================================================

/// Build a BPG file with optional extension data.
fn build_bpg(header1: u8, header2: u8, ext_data: Option<&[u8]>) -> Vec<u8> {
    let mut buf = vec![0x42, 0x50, 0x47, 0xFB];
    buf.push(header1);
    buf.push(header2);
    // width, height, picture-length as ue7
    buf.extend_from_slice(&ue7(640));
    buf.extend_from_slice(&ue7(480));
    buf.extend_from_slice(&ue7(1024)); // picture data length
    if let Some(ext) = ext_data {
        buf.extend_from_slice(&ue7(ext.len()));
        buf.extend_from_slice(ext);
    }
    buf
}

#[test]
fn test_bpg_basic_grayscale() {
    // header1: pixel_format=0 (grayscale), no alpha, bit_depth_minus_8=0 => 8 bit
    // header2: color_space=0 (YCbCr BT.601), no extension
    let data = build_bpg(0x00, 0x00, None);
    let reader = TestReader::new(data);
    let md = parse_bpg_metadata(&reader).expect("bpg parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("BPG"));
    assert!(md.contains_key("ImageWidth"));
    assert!(md.contains_key("ImageHeight"));
    assert!(md.contains_key("PixelFormat"));
    assert!(md.contains_key("ColorSpace"));
}

#[test]
fn test_bpg_all_flags_and_formats() {
    // pixel_format=3 (4:4:4), alpha1 flag set (0x08), bit_depth_minus_8=2 (0x20) => depth 10
    let header1 = 0x03 | 0x08 | (2 << 4);
    // color_space=1 (RGB), extension flag (0x10), alpha2 (0x20), limited range (0x40), animation (0x80)
    let header2 = 0x01 | 0x10 | 0x20 | 0x40 | 0x80;
    // extension data: an unknown tag (0x00000000) with empty length to exercise the loop
    let mut ext = Vec::new();
    ext.extend_from_slice(&0u32.to_be_bytes()); // tag
    ext.extend_from_slice(&ue7(0)); // length 0
    let data = build_bpg(header1, header2, Some(&ext));
    let reader = TestReader::new(data);
    let md = parse_bpg_metadata(&reader).expect("bpg parse");
    assert!(md.contains_key("HasAlpha"));
    assert!(md.contains_key("AlphaPlane"));
    assert!(md.contains_key("ColorRange"));
    assert!(md.contains_key("IsAnimated"));
    assert_eq!(
        md.get("PixelFormat").and_then(|v| v.as_string()),
        Some("4:4:4")
    );
    assert_eq!(
        md.get("ColorSpace").and_then(|v| v.as_string()),
        Some("RGB")
    );
}

#[test]
fn test_bpg_extension_exif() {
    // Build extension data containing an EXIF chunk.
    let exif = build_exif_blob();
    let mut ext = Vec::new();
    ext.extend_from_slice(&0x45584946u32.to_be_bytes()); // "EXIF"
    ext.extend_from_slice(&ue7(exif.len()));
    ext.extend_from_slice(&exif);
    // Also append an ICCP chunk
    ext.extend_from_slice(&0x49434350u32.to_be_bytes()); // "ICCP"
    ext.extend_from_slice(&ue7(4));
    ext.extend_from_slice(&[1, 2, 3, 4]);
    // And an XMP chunk
    let xmp = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'></x:xmpmeta>";
    ext.extend_from_slice(&0x584D5020u32.to_be_bytes()); // "XMP "
    ext.extend_from_slice(&ue7(xmp.len()));
    ext.extend_from_slice(xmp);

    // header2 with extension flag set
    let data = build_bpg(0x00, 0x10, Some(&ext));
    let reader = TestReader::new(data);
    let md = parse_bpg_metadata(&reader).expect("bpg parse");
    // The ICCP chunk should set HasICCProfile.
    assert_eq!(
        md.get("HasICCProfile").and_then(|v| v.as_string()),
        Some("Yes")
    );
}

#[test]
fn test_bpg_pixel_format_variants() {
    for pf in 0u8..=6 {
        let data = build_bpg(pf, 0x00, None);
        let reader = TestReader::new(data);
        let md = parse_bpg_metadata(&reader).expect("bpg parse");
        assert!(md.contains_key("PixelFormat"));
    }
}

#[test]
fn test_bpg_colorspace_variants() {
    for cs in 0u8..=7 {
        let data = build_bpg(0x00, cs, None);
        let reader = TestReader::new(data);
        let md = parse_bpg_metadata(&reader).expect("bpg parse");
        assert!(md.contains_key("ColorSpace"));
    }
}

#[test]
fn test_bpg_invalid_signature() {
    let reader = TestReader::new(vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);
    assert!(parse_bpg_metadata(&reader).is_err());
}

#[test]
fn test_bpg_too_short() {
    let reader = TestReader::new(vec![0x42, 0x50, 0x47, 0xFB]);
    // verify_signature passes but parse_header returns short error (ignored), still Ok overall.
    let md = parse_bpg_metadata(&reader).expect("bpg parse short");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("BPG"));
}

#[test]
fn test_bpg_verify_signature_and_format() {
    let data = build_bpg(0x00, 0x00, None);
    let reader = TestReader::new(data);
    assert!(BPGParser::verify_signature(&reader).unwrap());
    let parser = BPGParser;
    assert!(parser.supports_format(FileFormat::BPG));
    assert!(!parser.supports_format(FileFormat::BMP));
    // Too small for signature.
    let tiny = TestReader::new(vec![0x42]);
    assert!(!BPGParser::verify_signature(&tiny).unwrap());
}

#[test]
fn test_bpg_production_path() {
    let data = build_bpg(0x00, 0x00, None);
    let md = read_via_tempfile(&data, "bpg").expect("read_metadata bpg");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("BPG"));
}

// ===========================================================================
// EXR
// ===========================================================================

/// Append a null-terminated string to a buffer.
fn push_cstr(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
}

/// Append a single EXR attribute (name, type, size-prefixed value).
fn push_attr(buf: &mut Vec<u8>, name: &str, typ: &str, value: &[u8]) {
    push_cstr(buf, name);
    push_cstr(buf, typ);
    buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
    buf.extend_from_slice(value);
}

/// Build an EXR file. `flags` is the version-field flag word.
fn build_exr(flags: u32) -> Vec<u8> {
    let mut buf = vec![0x76, 0x2F, 0x31, 0x01];
    // version field: byte 0 = version (2), then flags in remaining bits.
    let mut version_word = flags;
    version_word |= 2; // version number 2 in low byte
    buf.extend_from_slice(&version_word.to_le_bytes());

    // Attributes:
    // dataWindow box2i: x_min=0,y_min=0,x_max=639,y_max=479
    let mut dw = Vec::new();
    dw.extend_from_slice(&0i32.to_le_bytes());
    dw.extend_from_slice(&0i32.to_le_bytes());
    dw.extend_from_slice(&639i32.to_le_bytes());
    dw.extend_from_slice(&479i32.to_le_bytes());
    push_attr(&mut buf, "dataWindow", "box2i", &dw);

    // displayWindow box2i
    push_attr(&mut buf, "displayWindow", "box2i", &dw);

    // compression: ZIP (3)
    push_attr(&mut buf, "compression", "compression", &[3]);

    // lineOrder: increasing Y (0)
    push_attr(&mut buf, "lineOrder", "lineOrder", &[0]);

    // pixelAspectRatio float
    push_attr(
        &mut buf,
        "pixelAspectRatio",
        "float",
        &1.0f32.to_bits().to_le_bytes(),
    );

    // screenWindowWidth float
    push_attr(
        &mut buf,
        "screenWindowWidth",
        "float",
        &1.0f32.to_bits().to_le_bytes(),
    );

    // screenWindowCenter v2f
    let mut swc = Vec::new();
    swc.extend_from_slice(&0.0f32.to_bits().to_le_bytes());
    swc.extend_from_slice(&0.0f32.to_bits().to_le_bytes());
    push_attr(&mut buf, "screenWindowCenter", "v2f", &swc);

    // owner string
    push_attr(&mut buf, "owner", "string", b"Alice");
    push_attr(&mut buf, "comments", "string", b"A comment");
    push_attr(&mut buf, "capDate", "string", b"2024:01:01 00:00:00");
    push_attr(&mut buf, "utcOffset", "string", b"0");

    // channels chlist: one channel "R", float, sampling 1/1
    let mut chlist = Vec::new();
    push_cstr(&mut chlist, "R");
    chlist.extend_from_slice(&2u32.to_le_bytes()); // pixel_type float
    chlist.push(0); // pLinear
    chlist.extend_from_slice(&[0, 0, 0]); // reserved
    chlist.extend_from_slice(&1u32.to_le_bytes()); // xSampling
    chlist.extend_from_slice(&1u32.to_le_bytes()); // ySampling
    // second channel "G"
    push_cstr(&mut chlist, "G");
    chlist.extend_from_slice(&1u32.to_le_bytes()); // half
    chlist.push(0);
    chlist.extend_from_slice(&[0, 0, 0]);
    chlist.extend_from_slice(&2u32.to_le_bytes());
    chlist.extend_from_slice(&2u32.to_le_bytes());
    chlist.push(0); // chlist terminator
    push_attr(&mut buf, "channels", "chlist", &chlist);

    // An unknown attribute to hit the default arm.
    push_attr(&mut buf, "myCustom", "int", &42u32.to_le_bytes());

    // Header terminator (null byte).
    buf.push(0);

    buf
}

#[test]
fn test_exr_full_attributes() {
    let data = build_exr(0);
    let reader = TestReader::new(data);
    let md = parse_exr_metadata(&reader).expect("exr parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("EXR"));
    assert_eq!(md.get("ImageWidth").and_then(|v| v.as_integer()), Some(640));
    assert_eq!(
        md.get("ImageHeight").and_then(|v| v.as_integer()),
        Some(480)
    );
    assert_eq!(
        md.get("Compression").and_then(|v| v.as_string()),
        Some("ZIP")
    );
    assert_eq!(
        md.get("LineOrder").and_then(|v| v.as_string()),
        Some("Increasing Y")
    );
    assert!(md.contains_key("DataWindow"));
    assert!(md.contains_key("DisplayWindow"));
    assert!(md.contains_key("Owner"));
    assert!(md.contains_key("Comments"));
    assert!(md.contains_key("CaptureDate"));
    assert!(md.contains_key("Channels"));
    assert!(md.contains_key("PixelAspectRatio"));
    assert!(md.contains_key("ScreenWindowWidth"));
    assert!(md.contains_key("ScreenWindowCenter"));
}

#[test]
fn test_exr_flags() {
    // tiled (0x200), long names (0x400), deep data (0x800), multipart (0x1000)
    let data = build_exr(0x200 | 0x400 | 0x800 | 0x1000);
    let reader = TestReader::new(data);
    let md = parse_exr_metadata(&reader).expect("exr parse");
    assert_eq!(md.get("Tiled").and_then(|v| v.as_string()), Some("Yes"));
    assert_eq!(md.get("LongNames").and_then(|v| v.as_string()), Some("Yes"));
    assert_eq!(md.get("DeepData").and_then(|v| v.as_string()), Some("Yes"));
    assert_eq!(md.get("Multipart").and_then(|v| v.as_string()), Some("Yes"));
    let flags = md.get("Flags").and_then(|v| v.as_string()).unwrap_or("");
    assert!(flags.contains("Tiled"));
}

#[test]
fn test_exr_no_flags_none_string() {
    let data = build_exr(0);
    let reader = TestReader::new(data);
    let md = parse_exr_metadata(&reader).expect("exr parse");
    assert_eq!(md.get("Flags").and_then(|v| v.as_string()), Some("(none)"));
}

#[test]
fn test_exr_compression_variants() {
    for c in 0u8..=10 {
        let mut buf = vec![0x76, 0x2F, 0x31, 0x01];
        buf.extend_from_slice(&2u32.to_le_bytes());
        push_attr(&mut buf, "compression", "compression", &[c]);
        buf.push(0);
        let reader = TestReader::new(buf);
        let md = parse_exr_metadata(&reader).expect("exr parse");
        assert!(md.contains_key("Compression"));
    }
}

#[test]
fn test_exr_lineorder_variants() {
    for o in 0u8..=3 {
        let mut buf = vec![0x76, 0x2F, 0x31, 0x01];
        buf.extend_from_slice(&2u32.to_le_bytes());
        push_attr(&mut buf, "lineOrder", "lineOrder", &[o]);
        buf.push(0);
        let reader = TestReader::new(buf);
        let md = parse_exr_metadata(&reader).expect("exr parse");
        assert!(md.contains_key("LineOrder"));
    }
}

#[test]
fn test_exr_invalid_signature() {
    let reader = TestReader::new(vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    assert!(parse_exr_metadata(&reader).is_err());
}

#[test]
fn test_exr_too_small_for_version() {
    // Valid signature but only 5 bytes -> version read fails.
    let reader = TestReader::new(vec![0x76, 0x2F, 0x31, 0x01, 0x02]);
    assert!(parse_exr_metadata(&reader).is_err());
}

#[test]
fn test_exr_verify_and_format() {
    let data = build_exr(0);
    let reader = TestReader::new(data);
    assert!(EXRParser::verify_signature(&reader).unwrap());
    let parser = EXRParser;
    assert!(parser.supports_format(FileFormat::EXR));
    assert!(!parser.supports_format(FileFormat::BMP));
    let tiny = TestReader::new(vec![0x76]);
    assert!(!EXRParser::verify_signature(&tiny).unwrap());
}

#[test]
fn test_exr_production_path() {
    let data = build_exr(0);
    let md = read_via_tempfile(&data, "exr").expect("read_metadata exr");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("EXR"));
}

// ===========================================================================
// FLIF
// ===========================================================================

/// FLIF varint encode: single byte if value-1 < 128.
fn flif_varint(value: u32) -> Vec<u8> {
    // Inverse of parser: single byte = byte+1 where byte<128.
    if value >= 1 && value <= 128 {
        vec![(value - 1) as u8]
    } else {
        // two byte form: value = ((first-128)<<8) + second + 129
        let v = value - 129;
        let first = ((v >> 8) as u8) | 0x80;
        let second = (v & 0xFF) as u8;
        vec![first, second]
    }
}

/// Build a FLIF file.
/// channels: 0=Gray,2=RGB,3=RGBA. animated adds frame count varint.
fn build_flif(channels: u8, bytes_per_channel: u8, interlaced: u8, animated: bool) -> Vec<u8> {
    let mut buf = b"FLIF".to_vec();
    // header byte: IIIIA CC T  (interlaced<<4 | animated<<3 | channels<<1 | bpc)
    let hb = (interlaced << 4)
        | ((animated as u8) << 3)
        | ((channels & 0x03) << 1)
        | (bytes_per_channel & 0x01);
    buf.push(hb);
    // width=640, height=480
    buf.extend_from_slice(&flif_varint(640));
    buf.extend_from_slice(&flif_varint(480));
    if animated {
        buf.extend_from_slice(&flif_varint(5)); // frame count
    }
    // The parser does reader.read(4, 10), requiring at least 14 bytes total.
    // Pad with bytes that are not a recognized metadata chunk FourCC so the
    // chunk-scanning loop terminates cleanly.
    while buf.len() < 16 {
        buf.push(0);
    }
    buf
}

#[test]
fn test_flif_rgb() {
    let data = build_flif(2, 0, 0, false);
    let reader = TestReader::new(data);
    let md = parse_flif_metadata(&reader).expect("flif parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("FLIF"));
    assert_eq!(
        md.get("FLIF:ColorType").and_then(|v| v.as_string()),
        Some("RGB")
    );
    assert_eq!(
        md.get("FLIF:BitDepth").and_then(|v| v.as_integer()),
        Some(8)
    );
    assert_eq!(
        md.get("FLIF:ImageWidth").and_then(|v| v.as_integer()),
        Some(640)
    );
    assert_eq!(
        md.get("FLIF:ImageHeight").and_then(|v| v.as_integer()),
        Some(480)
    );
}

#[test]
fn test_flif_grayscale_16bit() {
    let data = build_flif(0, 1, 0, false);
    let reader = TestReader::new(data);
    let md = parse_flif_metadata(&reader).expect("flif parse");
    assert_eq!(
        md.get("FLIF:ColorType").and_then(|v| v.as_string()),
        Some("Grayscale")
    );
    assert_eq!(
        md.get("FLIF:BitDepth").and_then(|v| v.as_integer()),
        Some(16)
    );
}

#[test]
fn test_flif_rgba_interlaced_animated() {
    let data = build_flif(3, 0, 1, true);
    let reader = TestReader::new(data);
    let md = parse_flif_metadata(&reader).expect("flif parse");
    assert_eq!(
        md.get("FLIF:ColorType").and_then(|v| v.as_string()),
        Some("RGBA")
    );
    assert_eq!(
        md.get("FLIF:Interlaced").and_then(|v| v.as_string()),
        Some("Yes")
    );
    assert_eq!(
        md.get("FLIF:Animated").and_then(|v| v.as_string()),
        Some("Yes")
    );
    assert_eq!(
        md.get("FLIF:FrameCount").and_then(|v| v.as_integer()),
        Some(5)
    );
}

#[test]
fn test_flif_invalid_channels() {
    // channels=1 is invalid -> parse returns error via "?".
    let data = build_flif(1, 0, 0, false);
    let reader = TestReader::new(data);
    assert!(parse_flif_metadata(&reader).is_err());
}

#[test]
fn test_flif_with_exif_chunk() {
    // Build the FLIF base manually (no trailing padding) so the metadata
    // chunks immediately follow the width/height varints.
    let mut data = b"FLIF".to_vec();
    let hb = (2u8 & 0x03) << 1; // RGB, 8-bit, not interlaced/animated
    data.push(hb);
    data.extend_from_slice(&flif_varint(640));
    data.extend_from_slice(&flif_varint(480));
    // Append an eXif chunk: "eXif" + size(be) + exif blob
    let exif = build_exif_blob();
    data.extend_from_slice(b"eXif");
    data.extend_from_slice(&(exif.len() as u32).to_be_bytes());
    data.extend_from_slice(&exif);
    // Append an iCCP chunk
    data.extend_from_slice(b"iCCP");
    data.extend_from_slice(&4u32.to_be_bytes());
    data.extend_from_slice(&[1, 2, 3, 4]);
    // Append an eXmp chunk
    let xmp = b"<x:xmpmeta></x:xmpmeta>";
    data.extend_from_slice(b"eXmp");
    data.extend_from_slice(&(xmp.len() as u32).to_be_bytes());
    data.extend_from_slice(xmp);

    let reader = TestReader::new(data);
    let md = parse_flif_metadata(&reader).expect("flif parse");
    assert!(md.contains_key("FLIF:ICCProfileSize"));
    assert!(md.contains_key("XMP:RawXMP"));
}

#[test]
fn test_flif_invalid_signature() {
    let reader = TestReader::new(vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);
    assert!(parse_flif_metadata(&reader).is_err());
}

#[test]
fn test_flif_too_short() {
    // Valid signature but < 6 bytes -> "FLIF file too short".
    let reader = TestReader::new(b"FLIF".to_vec());
    assert!(parse_flif_metadata(&reader).is_err());
}

#[test]
fn test_flif_verify_and_format() {
    let data = build_flif(2, 0, 0, false);
    let reader = TestReader::new(data);
    assert!(FLIFParser::verify_signature(&reader).unwrap());
    let parser = FLIFParser;
    assert!(parser.supports_format(FileFormat::FLIF));
    assert!(!parser.supports_format(FileFormat::BMP));
    let tiny = TestReader::new(vec![0x46]);
    assert!(!FLIFParser::verify_signature(&tiny).unwrap());
}

#[test]
fn test_flif_production_path() {
    let data = build_flif(2, 0, 0, false);
    let md = read_via_tempfile(&data, "flif").expect("read_metadata flif");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("FLIF"));
}

// ===========================================================================
// PSD
// ===========================================================================

/// Build a PSD file.
///
/// header (26 bytes) + color_mode_data_length(4)=0 + image_resources_length(4) + resources.
fn build_psd(version: u16, channels: u16, color_mode: u16, resources: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"8BPS");
    buf.extend_from_slice(&version.to_be_bytes()); // version
    buf.extend_from_slice(&[0u8; 6]); // reserved
    buf.extend_from_slice(&channels.to_be_bytes()); // channels (offset 12)
    buf.extend_from_slice(&480u32.to_be_bytes()); // height (offset 14)
    buf.extend_from_slice(&640u32.to_be_bytes()); // width (offset 18)
    buf.extend_from_slice(&8u16.to_be_bytes()); // depth (offset 22)
    buf.extend_from_slice(&color_mode.to_be_bytes()); // color mode (offset 24)
    // Color mode data length = 0 (offset 26)
    buf.extend_from_slice(&0u32.to_be_bytes());
    // Image resources length (offset 30)
    buf.extend_from_slice(&(resources.len() as u32).to_be_bytes());
    buf.extend_from_slice(resources);
    buf
}

/// Build an "8BIM" resource block: signature + id(2) + name(pascal, even) + size(4) + data(even).
fn build_resource(resource_id: u16, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"8BIM");
    buf.extend_from_slice(&resource_id.to_be_bytes());
    // Pascal string name: empty -> length byte 0, padded to even => 2 bytes total.
    buf.push(0); // name length 0
    buf.push(0); // pad to even
    buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
    buf.extend_from_slice(data);
    // Pad data to even boundary.
    if data.len() % 2 != 0 {
        buf.push(0);
    }
    buf
}

#[test]
fn test_psd_header_rgb() {
    let data = build_psd(1, 3, 3, &[]);
    let reader = TestReader::new(data);
    let md = parse_psd_metadata(&reader).expect("psd parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("PSD"));
    assert_eq!(md.get("PSDVersion").and_then(|v| v.as_integer()), Some(1));
    assert_eq!(md.get("NumChannels").and_then(|v| v.as_integer()), Some(3));
    assert_eq!(md.get("ImageWidth").and_then(|v| v.as_integer()), Some(640));
    assert_eq!(
        md.get("ImageHeight").and_then(|v| v.as_integer()),
        Some(480)
    );
    assert_eq!(md.get("BitDepth").and_then(|v| v.as_integer()), Some(8));
    assert_eq!(md.get("ColorMode").and_then(|v| v.as_string()), Some("RGB"));
}

#[test]
fn test_psd_psb_version_and_colormodes() {
    // version 2 => PSB
    let data = build_psd(2, 4, 4, &[]);
    let reader = TestReader::new(data);
    let md = parse_psd_metadata(&reader).expect("psd parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("PSB"));
    assert_eq!(
        md.get("ColorMode").and_then(|v| v.as_string()),
        Some("CMYK")
    );

    // Exercise all documented color modes.
    for (cm, _name) in [
        (0u16, "Bitmap"),
        (1, "Grayscale"),
        (2, "Indexed"),
        (3, "RGB"),
        (4, "CMYK"),
        (7, "Multichannel"),
        (8, "Duotone"),
        (9, "Lab"),
        (99, "Unknown"),
    ] {
        let d = build_psd(1, 3, cm, &[]);
        let r = TestReader::new(d);
        let m = parse_psd_metadata(&r).expect("psd parse");
        assert!(m.contains_key("ColorMode"));
    }
}

#[test]
fn test_psd_resolution_info_resource() {
    // Resolution info resource (0x03ED): 16 bytes.
    let mut res_data = Vec::new();
    res_data.extend_from_slice(&(72u32 << 16).to_be_bytes()); // h res 72.0
    res_data.extend_from_slice(&1u16.to_be_bytes()); // unit = inch
    res_data.extend_from_slice(&0u16.to_be_bytes()); // h unit display
    res_data.extend_from_slice(&(72u32 << 16).to_be_bytes()); // v res 72.0
    res_data.extend_from_slice(&1u16.to_be_bytes());
    res_data.extend_from_slice(&0u16.to_be_bytes());
    let resources = build_resource(0x03ED, &res_data);
    let data = build_psd(1, 3, 3, &resources);
    let reader = TestReader::new(data);
    let md = parse_psd_metadata(&reader).expect("psd parse");
    assert_eq!(
        md.get("ResolutionUnit").and_then(|v| v.as_string()),
        Some("inch")
    );
    assert!(md.contains_key("XResolution"));
    assert!(md.contains_key("YResolution"));
}

#[test]
fn test_psd_exif_resource() {
    let exif = build_exif_blob();
    let resources = build_resource(0x0422, &exif); // EXIF_DATA_1
    let data = build_psd(1, 3, 3, &resources);
    let reader = TestReader::new(data);
    let md = parse_psd_metadata(&reader).expect("psd parse");
    // Just confirm header was parsed; EXIF tags merged if recognized.
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("PSD"));
}

#[test]
fn test_psd_copyright_and_xmp_resources() {
    // Copyright flag resource (0x040A): nonzero first byte => Copyrighted.
    let mut resources = build_resource(0x040A, &[1u8, 0u8]);
    // XMP resource (0x0424)
    let xmp = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'></x:xmpmeta>";
    resources.extend_from_slice(&build_resource(0x0424, xmp));
    let data = build_psd(1, 3, 3, &resources);
    let reader = TestReader::new(data);
    let md = parse_psd_metadata(&reader).expect("psd parse");
    assert_eq!(
        md.get("Copyrighted").and_then(|v| v.as_string()),
        Some("Yes")
    );
}

#[test]
fn test_psd_icc_resource() {
    // ICC profile resource (0x040F). The data need not be a real ICC profile;
    // HasICCProfile should still be set even if ICC parse fails.
    let resources = build_resource(0x040F, &[0u8; 8]);
    let data = build_psd(1, 3, 3, &resources);
    let reader = TestReader::new(data);
    let md = parse_psd_metadata(&reader).expect("psd parse");
    assert_eq!(
        md.get("HasICCProfile").and_then(|v| v.as_string()),
        Some("Yes")
    );
}

#[test]
fn test_psd_invalid_signature() {
    let reader = TestReader::new(b"NOPE0000000000000000000000".to_vec());
    assert!(parse_psd_metadata(&reader).is_err());
}

#[test]
fn test_psd_read_version_and_verify() {
    let data = build_psd(1, 3, 3, &[]);
    let reader = TestReader::new(data);
    assert!(PSDParser::verify_signature(&reader).unwrap());
    assert_eq!(PSDParser::read_version(&reader).unwrap(), 1);
    let parser = PSDParser;
    assert!(parser.supports_format(FileFormat::PSD));
    assert!(!parser.supports_format(FileFormat::BMP));
    // small reader -> read_version returns 0
    let tiny = TestReader::new(b"8B".to_vec());
    assert_eq!(PSDParser::read_version(&tiny).unwrap(), 0);
}

#[test]
fn test_psd_production_path() {
    let data = build_psd(1, 3, 3, &[]);
    let md = read_via_tempfile(&data, "psd").expect("read_metadata psd");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("PSD"));
}

// ===========================================================================
// ICO / CUR
// ===========================================================================

/// Build an ICO/CUR file.
/// `file_type`: 1 = ICO, 2 = CUR. entries: list of (w,h,colors,bpp).
fn build_ico(file_type: u16, entries: &[(u8, u8, u8, u16)]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0u16.to_le_bytes()); // reserved
    buf.extend_from_slice(&file_type.to_le_bytes()); // type
    buf.extend_from_slice(&(entries.len() as u16).to_le_bytes()); // count
    for &(w, h, colors, bpp) in entries {
        buf.push(w);
        buf.push(h);
        buf.push(colors);
        buf.push(0); // reserved
        buf.extend_from_slice(&1u16.to_le_bytes()); // planes / hotspot X
        buf.extend_from_slice(&bpp.to_le_bytes()); // bits per pixel (offset 6)
        buf.extend_from_slice(&0u32.to_le_bytes()); // data size
        buf.extend_from_slice(&0u32.to_le_bytes()); // data offset
    }
    buf
}

#[test]
fn test_ico_single_entry() {
    let data = build_ico(1, &[(32, 32, 0, 32)]);
    let reader = TestReader::new(data);
    let md = parse_ico_metadata(&reader).expect("ico parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("ICO"));
    assert_eq!(md.get("ImageCount").and_then(|v| v.as_string()), Some("1"));
    assert_eq!(md.get("ImageWidth").and_then(|v| v.as_string()), Some("32"));
    assert_eq!(
        md.get("ImageHeight").and_then(|v| v.as_string()),
        Some("32")
    );
    assert_eq!(md.get("BitDepth").and_then(|v| v.as_string()), Some("32"));
    assert!(md.contains_key("AvailableSizes"));
}

#[test]
fn test_ico_multiple_entries_and_256() {
    // width/height 0 means 256.
    let data = build_ico(1, &[(16, 16, 16, 4), (0, 0, 0, 8), (48, 48, 0, 24)]);
    let reader = TestReader::new(data);
    let md = parse_ico_metadata(&reader).expect("ico parse");
    assert_eq!(md.get("ImageCount").and_then(|v| v.as_string()), Some("3"));
    // max width should be 256 from the 0,0 entry.
    assert_eq!(
        md.get("ImageWidth").and_then(|v| v.as_string()),
        Some("256")
    );
    let sizes = md
        .get("AvailableSizes")
        .and_then(|v| v.as_string())
        .unwrap_or("");
    assert!(sizes.contains("256x256"));
    assert!(sizes.contains("16x16"));
}

#[test]
fn test_cur_type() {
    let data = build_ico(2, &[(32, 32, 0, 1)]);
    let reader = TestReader::new(data);
    let md = parse_ico_metadata(&reader).expect("cur parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("CUR"));
}

#[test]
fn test_ico_invalid_signature() {
    // Reserved nonzero / wrong type.
    let reader = TestReader::new(vec![0xFF, 0xFF, 0x09, 0x00, 0x00, 0x00]);
    assert!(parse_ico_metadata(&reader).is_err());
}

#[test]
fn test_ico_zero_count() {
    let data = build_ico(1, &[]);
    let reader = TestReader::new(data);
    let md = parse_ico_metadata(&reader).expect("ico parse");
    assert_eq!(md.get("ImageCount").and_then(|v| v.as_string()), Some("0"));
    // With zero entries, dimension keys are absent.
    assert!(!md.contains_key("ImageWidth"));
}

#[test]
fn test_ico_verify_and_format() {
    let data = build_ico(1, &[(32, 32, 0, 32)]);
    let reader = TestReader::new(data);
    assert!(ICOParser::verify_signature(&reader).unwrap());
    let parser = ICOParser;
    assert!(parser.supports_format(FileFormat::ICO));
    assert!(!parser.supports_format(FileFormat::BMP));
    let tiny = TestReader::new(vec![0x00, 0x00]);
    assert!(!ICOParser::verify_signature(&tiny).unwrap());
}

#[test]
fn test_ico_production_path() {
    let data = build_ico(1, &[(32, 32, 0, 32)]);
    let md = read_via_tempfile(&data, "ico").expect("read_metadata ico");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("ICO"));
}

// ===========================================================================
// BMP
// ===========================================================================

/// Build a BMP file with a full BITMAPINFOHEADER (40 bytes).
#[allow(clippy::too_many_arguments)]
fn build_bmp(
    width: i32,
    height: i32,
    bit_depth: u16,
    compression: u32,
    h_res: i32,
    v_res: i32,
    num_colors: u32,
    important_colors: u32,
) -> Vec<u8> {
    let mut buf = Vec::new();
    // File header (14 bytes): "BM" + size + reserved(4) + data offset
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&0u32.to_le_bytes()); // file size (offset 2)
    buf.extend_from_slice(&0u32.to_le_bytes()); // reserved
    buf.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
    // DIB header (BITMAPINFOHEADER, 40 bytes) starting at offset 14
    buf.extend_from_slice(&40u32.to_le_bytes()); // header size (offset 14)
    buf.extend_from_slice(&width.to_le_bytes()); // width (offset 18)
    buf.extend_from_slice(&height.to_le_bytes()); // height (offset 22)
    buf.extend_from_slice(&1u16.to_le_bytes()); // planes (offset 26)
    buf.extend_from_slice(&bit_depth.to_le_bytes()); // bit depth (offset 28)
    buf.extend_from_slice(&compression.to_le_bytes()); // compression (offset 30)
    buf.extend_from_slice(&0u32.to_le_bytes()); // image size (offset 34)
    buf.extend_from_slice(&h_res.to_le_bytes()); // h res (offset 38)
    buf.extend_from_slice(&v_res.to_le_bytes()); // v res (offset 42)
    buf.extend_from_slice(&num_colors.to_le_bytes()); // num colors (offset 46)
    buf.extend_from_slice(&important_colors.to_le_bytes()); // important (offset 50)
    buf
}

#[test]
fn test_bmp_full_header() {
    let data = build_bmp(640, 480, 24, 0, 2835, 2835, 0, 0);
    let reader = TestReader::new(data);
    let md = parse_bmp_metadata(&reader).expect("bmp parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("BMP"));
    assert_eq!(
        md.get("ImageWidth").and_then(|v| v.as_string()),
        Some("640")
    );
    assert_eq!(
        md.get("ImageHeight").and_then(|v| v.as_string()),
        Some("480")
    );
    assert_eq!(md.get("BMP:Width").and_then(|v| v.as_integer()), Some(640));
    assert_eq!(md.get("BMP:Height").and_then(|v| v.as_integer()), Some(480));
    assert_eq!(md.get("BitDepth").and_then(|v| v.as_string()), Some("24"));
    assert_eq!(
        md.get("Compression").and_then(|v| v.as_string()),
        Some("None")
    );
    assert!(md.contains_key("XResolution"));
    assert!(md.contains_key("YResolution"));
    assert!(md.contains_key("BMP:XResolution"));
    assert!(md.contains_key("BMP:YResolution"));
    assert!(md.contains_key("BMP:ImageSize"));
}

#[test]
fn test_bmp_negative_height_and_palette() {
    // Negative height => top-down; abs taken. num_colors and important set.
    let data = build_bmp(100, -200, 8, 1, 1000, 1000, 256, 128);
    let reader = TestReader::new(data);
    let md = parse_bmp_metadata(&reader).expect("bmp parse");
    assert_eq!(
        md.get("ImageHeight").and_then(|v| v.as_string()),
        Some("200")
    );
    assert_eq!(
        md.get("Compression").and_then(|v| v.as_string()),
        Some("RLE 8-bit")
    );
    assert_eq!(md.get("NumColors").and_then(|v| v.as_integer()), Some(256));
    assert_eq!(
        md.get("BMP:ColorCount").and_then(|v| v.as_integer()),
        Some(256)
    );
    assert_eq!(
        md.get("NumImportantColors").and_then(|v| v.as_integer()),
        Some(128)
    );
}

#[test]
fn test_bmp_compression_variants() {
    for (comp, _name) in [
        (0u32, "None"),
        (1, "RLE 8-bit"),
        (2, "RLE 4-bit"),
        (3, "Bitfields"),
        (4, "JPEG"),
        (5, "PNG"),
        (99, "Unknown"),
    ] {
        let data = build_bmp(10, 10, 24, comp, 0, 0, 0, 0);
        let reader = TestReader::new(data);
        let md = parse_bmp_metadata(&reader).expect("bmp parse");
        assert!(md.contains_key("Compression"));
    }
}

#[test]
fn test_bmp_individual_readers() {
    let data = build_bmp(320, 240, 16, 3, 500, 600, 16, 8);
    let reader = TestReader::new(data);
    assert!(BMPParser::verify_signature(&reader).unwrap());
    assert_eq!(BMPParser::read_dimensions(&reader).unwrap(), (320, 240));
    assert_eq!(BMPParser::read_bit_depth(&reader).unwrap(), 16);
    assert_eq!(BMPParser::read_compression(&reader).unwrap(), 3);
    assert_eq!(BMPParser::read_h_resolution(&reader).unwrap(), 500);
    assert_eq!(BMPParser::read_v_resolution(&reader).unwrap(), 600);
    assert_eq!(BMPParser::read_num_colors(&reader).unwrap(), 16);
    assert_eq!(BMPParser::read_num_important_colors(&reader).unwrap(), 8);
}

#[test]
fn test_bmp_truncated_readers_return_defaults() {
    // Only "BM" + a few bytes: all sized reads should return their zero defaults.
    let reader = TestReader::new(vec![0x42, 0x4D, 0x00, 0x00]);
    assert_eq!(BMPParser::read_dimensions(&reader).unwrap(), (0, 0));
    assert_eq!(BMPParser::read_bit_depth(&reader).unwrap(), 0);
    assert_eq!(BMPParser::read_compression(&reader).unwrap(), 0);
    assert_eq!(BMPParser::read_h_resolution(&reader).unwrap(), 0);
    assert_eq!(BMPParser::read_v_resolution(&reader).unwrap(), 0);
    assert_eq!(BMPParser::read_num_colors(&reader).unwrap(), 0);
    assert_eq!(BMPParser::read_num_important_colors(&reader).unwrap(), 0);
}

#[test]
fn test_bmp_invalid_signature() {
    let reader = TestReader::new(b"XX0000000000000000000000000000".to_vec());
    assert!(parse_bmp_metadata(&reader).is_err());
    let tiny = TestReader::new(vec![0x42]);
    assert!(!BMPParser::verify_signature(&tiny).unwrap());
}

#[test]
fn test_bmp_format_support() {
    let parser = BMPParser;
    assert!(parser.supports_format(FileFormat::BMP));
    assert!(!parser.supports_format(FileFormat::PSD));
}

#[test]
fn test_bmp_production_path() {
    let data = build_bmp(640, 480, 24, 0, 2835, 2835, 0, 0);
    let md = read_via_tempfile(&data, "bmp").expect("read_metadata bmp");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("BMP"));
}

// ===========================================================================
// SVG
// ===========================================================================

const SVG_FULL: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:dc="http://purl.org/dc/elements/1.1/"
     xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
     version="1.1" width="200px" height="150px" viewBox="0 0 200 150"
     preserveAspectRatio="xMidYMid meet">
  <title>My Title</title>
  <desc>A description</desc>
  <desc role="photoTitle">Role Title</desc>
  <defs></defs>
  <metadata>
    <rdf:RDF>
      <dc:title>DC Title</dc:title>
      <dc:creator>
        <rdf:Bag>
          <rdf:li>Author One</rdf:li>
          <rdf:li>Author Two</rdf:li>
        </rdf:Bag>
      </dc:creator>
      <dc:description>DC Description</dc:description>
      <dc:date>2024-01-01</dc:date>
      <dc:format>image/svg+xml</dc:format>
      <dc:language>en</dc:language>
      <dc:publisher>Acme</dc:publisher>
    </rdf:RDF>
  </metadata>
  <rect x="0" y="0" width="10" height="10"/>
  <circle cx="5" cy="5" r="3"/>
  <ellipse cx="1" cy="1" rx="2" ry="2"/>
  <line x1="0" y1="0" x2="1" y2="1"/>
  <polyline points="0,0 1,1"/>
  <polygon points="0,0 1,0 1,1"/>
  <path d="M0 0 L1 1"/>
  <text x="0" y="0">hello</text>
  <use href="#x"/>
  <g >
    <animate attributeName="x" from="0" to="10" dur="1s"/>
  </g>
</svg>"##;

#[test]
fn test_svg_full_metadata() {
    let reader = TestReader::new(SVG_FULL.as_bytes().to_vec());
    let md = parse_svg_metadata(&reader).expect("svg parse");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("SVG"));
    assert_eq!(
        md.get("ImageWidth").and_then(|v| v.as_string()),
        Some("200px")
    );
    assert_eq!(
        md.get("ImageHeight").and_then(|v| v.as_string()),
        Some("150px")
    );
    assert_eq!(
        md.get("SVG:Version").and_then(|v| v.as_string()),
        Some("1.1")
    );
    assert_eq!(
        md.get("Title").and_then(|v| v.as_string()),
        Some("My Title")
    );
    assert!(md.contains_key("SVG:ViewBox"));
    assert!(md.contains_key("SVG:Xmlns"));
    assert!(md.contains_key("SVG:PreserveAspectRatio"));
    assert_eq!(
        md.get("SVG:Animated").and_then(|v| v.as_string()),
        Some("true")
    );
    assert_eq!(
        md.get("SVG:HasDefinitions").and_then(|v| v.as_string()),
        Some("true")
    );
    assert_eq!(
        md.get("SVG:HasMetadata").and_then(|v| v.as_string()),
        Some("true")
    );
    assert!(md.contains_key("SVG:ElementCount"));
    // Dublin core merged.
    assert_eq!(
        md.get("XMP:Title").and_then(|v| v.as_string()),
        Some("DC Title")
    );
    assert_eq!(
        md.get("XMP:Creator").and_then(|v| v.as_string()),
        Some("[\"Author One\",\"Author Two\"]")
    );
    assert!(md.contains_key("XMP:Date"));
    assert!(md.contains_key("XMP:Format"));
    assert!(md.contains_key("XMP:Language"));
    assert!(md.contains_key("XMP:Publisher"));
    // Role-based desc metadata.
    assert!(md.contains_key("SVG:DescPhotoTitle"));
}

#[test]
fn test_svg_viewbox_only_dimensions() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 200"></svg>"#;
    let reader = TestReader::new(svg.as_bytes().to_vec());
    let md = parse_svg_metadata(&reader).expect("svg parse");
    assert_eq!(
        md.get("ImageWidth").and_then(|v| v.as_string()),
        Some("100")
    );
    assert_eq!(
        md.get("ImageHeight").and_then(|v| v.as_string()),
        Some("200")
    );
    // No definitions or metadata present.
    assert_eq!(
        md.get("SVG:HasDefinitions").and_then(|v| v.as_string()),
        Some("false")
    );
    assert_eq!(
        md.get("SVG:HasMetadata").and_then(|v| v.as_string()),
        Some("false")
    );
}

#[test]
fn test_svg_single_quotes_and_xmpmeta() {
    let svg = r#"<svg xmlns='http://www.w3.org/2000/svg' width='50' height='60'>
  <x:xmpmeta xmlns:x='adobe:ns:meta/'>
    <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'></rdf:RDF>
  </x:xmpmeta>
</svg>"#;
    let reader = TestReader::new(svg.as_bytes().to_vec());
    let md = parse_svg_metadata(&reader).expect("svg parse");
    assert_eq!(md.get("ImageWidth").and_then(|v| v.as_string()), Some("50"));
    assert_eq!(
        md.get("ImageHeight").and_then(|v| v.as_string()),
        Some("60")
    );
}

#[test]
fn test_svg_simple_creator() {
    let svg = r#"<svg xmlns:dc="http://purl.org/dc/elements/1.1/">
  <metadata>
    <dc:creator>Single Author</dc:creator>
  </metadata>
</svg>"#;
    let reader = TestReader::new(svg.as_bytes().to_vec());
    let md = parse_svg_metadata(&reader).expect("svg parse");
    assert_eq!(
        md.get("XMP:Creator").and_then(|v| v.as_string()),
        Some("Single Author")
    );
}

#[test]
fn test_svg_invalid() {
    let reader = TestReader::new(b"Not an SVG file at all here".to_vec());
    assert!(parse_svg_metadata(&reader).is_err());
}

#[test]
fn test_svg_verify_and_format() {
    let reader = TestReader::new(SVG_FULL.as_bytes().to_vec());
    assert!(SVGParser::verify_signature(&reader).unwrap());
    let parser = SVGParser;
    assert!(parser.supports_format(FileFormat::SVG));
    assert!(!parser.supports_format(FileFormat::BMP));
    // Too small.
    let tiny = TestReader::new(b"<s".to_vec());
    assert!(!SVGParser::verify_signature(&tiny).unwrap());
}

#[test]
fn test_svg_production_path() {
    let md = read_via_tempfile(SVG_FULL.as_bytes(), "svg").expect("read_metadata svg");
    assert_eq!(md.get("FileType").and_then(|v| v.as_string()), Some("SVG"));
}
