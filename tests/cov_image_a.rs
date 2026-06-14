//! Coverage tests for image parsers: GIF, WebP, JXL.
//!
//! These tests build synthetic byte buffers that drive each parser deep
//! through its branches (extensions/chunks/boxes, optional sections, error
//! paths). Where useful they also route through the production
//! `read_metadata` path on a tempfile to exercise format detection.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;

use oxidex::core::operations::read_metadata;
use oxidex::parsers::image::gif::{GIFParser, parse_gif_metadata};
use oxidex::parsers::image::jxl::{JXLParser, parse_jxl_metadata};
use oxidex::parsers::image::webp::{WebPParser, parse_webp_metadata};

use oxidex::core::TagValue;
use std::io::Write;
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Render a TagValue as a comparable string for assertions.
/// (TagValue does not implement Display.)
fn sval(v: &TagValue) -> String {
    match v {
        TagValue::String(s) => s.clone(),
        TagValue::Integer(i) => i.to_string(),
        TagValue::Float(f) => f.to_string(),
        other => format!("{other:?}"),
    }
}

/// Write bytes to a tempfile with the given extension and run read_metadata.
fn read_via_tempfile(ext: &str, bytes: &[u8]) -> oxidex::core::MetadataMap {
    let mut tmp = NamedTempFile::with_suffix(&format!(".{ext}")).expect("create tempfile");
    tmp.write_all(bytes).expect("write tempfile");
    tmp.flush().expect("flush tempfile");
    read_metadata(tmp.path()).expect("read_metadata")
}

/// Build a minimal little-endian TIFF/EXIF block with a single ASCII tag
/// (ImageDescription, 0x010E) plus a pointer to an ExifIFD and a GPS IFD,
/// so the WebP/JXL EXIF parsers walk IFD0 -> ExifIFD -> GPS.
///
/// Layout (offsets relative to TIFF start):
///   0x00  "II" 0x2A00            (header)
///   0x04  IFD0 offset = 0x08
///   0x08  IFD0: 3 entries
///         0x010E ASCII (ImageDescription) -> inline "Hi\0\0" (4 bytes)
///         0x8769 LONG  (ExifIFD ptr)      -> offset of ExifIFD
///         0x8825 LONG  (GPS ptr)          -> offset of GPS IFD
///   next  IFD0 next-offset = 0
///   ExifIFD: 1 entry (0x9000 ExifVersion UNDEFINED "0230")
///   GPS IFD: 1 entry (0x0000 BYTE version, inline)
fn build_tiff_exif() -> Vec<u8> {
    let mut d: Vec<u8> = Vec::new();
    // TIFF header
    d.extend_from_slice(b"II");
    d.extend_from_slice(&0x002Au16.to_le_bytes());
    d.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at offset 8

    // IFD0 starts at 8. 3 entries -> 2 + 3*12 + 4 = 42 bytes -> ends at 50.
    // ExifIFD at 50, GPS IFD after it.
    let exif_ifd_off: u32 = 50;
    // ExifIFD: 2 + 1*12 + 4 = 18 bytes -> ends at 68
    let gps_ifd_off: u32 = 68;

    // --- IFD0 ---
    d.extend_from_slice(&3u16.to_le_bytes()); // entry count

    // Entry: ImageDescription (0x010E), ASCII (2), count 4, inline "Hi\0\0"
    d.extend_from_slice(&0x010Eu16.to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&4u32.to_le_bytes());
    d.extend_from_slice(b"Hi\0\0");

    // Entry: ExifIFD pointer (0x8769), LONG (4), count 1, value = exif_ifd_off
    d.extend_from_slice(&0x8769u16.to_le_bytes());
    d.extend_from_slice(&4u16.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&exif_ifd_off.to_le_bytes());

    // Entry: GPS pointer (0x8825), LONG (4), count 1, value = gps_ifd_off
    d.extend_from_slice(&0x8825u16.to_le_bytes());
    d.extend_from_slice(&4u16.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&gps_ifd_off.to_le_bytes());

    // IFD0 next offset = 0
    d.extend_from_slice(&0u32.to_le_bytes());

    // --- ExifIFD at offset 50 ---
    assert_eq!(d.len(), exif_ifd_off as usize);
    d.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
    // ExifVersion (0x9000), UNDEFINED (7), count 4, inline "0230"
    d.extend_from_slice(&0x9000u16.to_le_bytes());
    d.extend_from_slice(&7u16.to_le_bytes());
    d.extend_from_slice(&4u32.to_le_bytes());
    d.extend_from_slice(b"0230");
    d.extend_from_slice(&0u32.to_le_bytes()); // next offset

    // --- GPS IFD at offset 68 ---
    assert_eq!(d.len(), gps_ifd_off as usize);
    d.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
    // GPSVersionID (0x0000), BYTE (1), count 4, inline [2,3,0,0]
    d.extend_from_slice(&0x0000u16.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&4u32.to_le_bytes());
    d.extend_from_slice(&[2u8, 3, 0, 0]);
    d.extend_from_slice(&0u32.to_le_bytes()); // next offset

    d
}

// ===========================================================================
// GIF
// ===========================================================================

/// Build a GIF header + logical screen descriptor.
/// `packed` sets the LSD packed byte (global color table flag, color res, gct size).
fn gif_header(version: &[u8], width: u16, height: u16, packed: u8, bg: u8, par: u8) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(version); // "GIF89a" / "GIF87a"
    d.extend_from_slice(&width.to_le_bytes());
    d.extend_from_slice(&height.to_le_bytes());
    d.push(packed);
    d.push(bg);
    d.push(par);
    d
}

#[test]
fn gif_basic_89a_no_color_table() {
    // packed = 0x70 -> no GCT, color resolution bits 3 -> +1 = 4
    let mut data = gif_header(b"GIF89a", 16, 8, 0x70, 0, 0);
    data.push(0x3B); // trailer
    let reader = TestReader::new(data);
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert_eq!(md.get("GIFVersion").map(sval), Some("89a".to_string()));
    assert!(md.contains_key("ImageWidth"));
    assert!(md.contains_key("ImageHeight"));
    assert!(md.contains_key("HasColorMap"));
    assert!(md.contains_key("BackgroundColor"));
    assert!(md.contains_key("PixelAspectRatio"));
}

#[test]
fn gif_87a_with_global_color_table_and_aspect_ratio() {
    // packed = 0x80 | 0x01 = global color table flag set, size bits = 1 -> 2^(1+1)=4 entries
    let packed = 0b1000_0001;
    let mut data = gif_header(b"GIF87a", 4, 4, packed, 5, 49); // par != 0 path
    // Global color table: 4 entries * 3 bytes = 12 bytes
    data.extend_from_slice(&[0u8; 12]);
    data.push(0x3B); // trailer
    let reader = TestReader::new(data);
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert_eq!(md.get("GIFVersion").map(sval), Some("87a".to_string()));
    assert!(md.contains_key("GlobalColorTableSize"));
    assert!(md.contains_key("BitsPerPixel"));
    // PixelAspectRatio non-zero path
    assert!(md.contains_key("PixelAspectRatio"));
}

#[test]
fn gif_with_comment_extension() {
    let mut data = gif_header(b"GIF89a", 2, 2, 0x00, 0, 0);
    // Comment extension: 0x21 0xFE <sub-blocks> 0x00
    data.push(0x21);
    data.push(0xFE);
    let comment = b"hello world";
    data.push(comment.len() as u8);
    data.extend_from_slice(comment);
    // second comment sub-block
    let comment2 = b"more";
    data.push(comment2.len() as u8);
    data.extend_from_slice(comment2);
    data.push(0x00); // terminator
    data.push(0x3B);
    let reader = TestReader::new(data);
    let md = parse_gif_metadata(&reader).expect("parse gif");
    let c = md.get("Comment").map(sval).unwrap_or_default();
    assert!(c.contains("hello world"), "comment was {c:?}");
    assert!(c.contains("more"), "comment was {c:?}");
}

#[test]
fn gif_with_graphic_control_extension_and_image() {
    let mut data = gif_header(b"GIF89a", 2, 2, 0x00, 0, 0);
    // Graphic control extension: 0x21 0xF9 0x04 <packed delay delay tci> 0x00
    data.push(0x21);
    data.push(0xF9);
    data.push(0x04); // block size
    // packed: disposal=2 (<<2 = 0x08), transparent flag = 0x01 -> 0x09
    data.push(0b0000_1001);
    data.extend_from_slice(&10u16.to_le_bytes()); // delay time
    data.push(7); // transparent color index
    data.push(0x00); // terminator
    // Image descriptor: 0x2C + 9 bytes (no local color table)
    data.push(0x2C);
    data.extend_from_slice(&0u16.to_le_bytes()); // left
    data.extend_from_slice(&0u16.to_le_bytes()); // top
    data.extend_from_slice(&2u16.to_le_bytes()); // width
    data.extend_from_slice(&2u16.to_le_bytes()); // height
    data.push(0x00); // packed: no local color table
    data.push(0x02); // LZW min code size
    // image data sub-blocks
    data.push(0x03);
    data.extend_from_slice(&[0x01, 0x02, 0x03]);
    data.push(0x00); // sub-block terminator
    data.push(0x3B);
    let reader = TestReader::new(data);
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert!(md.contains_key("FrameCount"));
    assert!(md.contains_key("FrameDelay"));
    assert!(md.contains_key("DisposalMethod"));
    assert!(md.contains_key("HasTransparency"));
    assert!(md.contains_key("TransparentColorIndex"));
}

#[test]
fn gif_with_image_local_color_table() {
    let mut data = gif_header(b"GIF89a", 2, 2, 0x00, 0, 0);
    // Image descriptor with local color table
    data.push(0x2C);
    data.extend_from_slice(&0u16.to_le_bytes()); // left
    data.extend_from_slice(&0u16.to_le_bytes()); // top
    data.extend_from_slice(&2u16.to_le_bytes()); // width
    data.extend_from_slice(&2u16.to_le_bytes()); // height
    // packed: local color table flag (0x80), size bits 1 -> 2^(1+1)=4 entries
    data.push(0b1000_0001);
    // local color table: 4*3 bytes
    data.extend_from_slice(&[0u8; 12]);
    data.push(0x02); // LZW min code size
    data.push(0x02);
    data.extend_from_slice(&[0xAA, 0xBB]);
    data.push(0x00); // terminator
    data.push(0x3B);
    let reader = TestReader::new(data);
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert_eq!(md.get("FrameCount").map(sval), Some("1".to_string()));
}

#[test]
fn gif_netscape_animation_extension() {
    let mut data = gif_header(b"GIF89a", 2, 2, 0x00, 0, 0);
    // Application extension: 0x21 0xFF 0x0B "NETSCAPE2.0" <sub-blocks> 0x00
    data.push(0x21);
    data.push(0xFF);
    data.push(0x0B); // block size = 11
    data.extend_from_slice(b"NETSCAPE2.0");
    // sub-block: loop count
    data.push(0x03);
    data.extend_from_slice(&[0x01, 0x00, 0x00]);
    data.push(0x00); // terminator
    data.push(0x3B);
    let reader = TestReader::new(data);
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert_eq!(md.get("Animation").map(sval), Some("yes".to_string()));
}

#[test]
fn gif_unknown_extension_is_skipped() {
    let mut data = gif_header(b"GIF89a", 2, 2, 0x00, 0, 0);
    // Plain text extension (0x01) - unknown branch -> skip_sub_blocks
    data.push(0x21);
    data.push(0x01);
    data.push(0x04);
    data.extend_from_slice(&[0u8; 4]);
    data.push(0x00); // terminator
    data.push(0x3B);
    let reader = TestReader::new(data);
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert!(md.contains_key("FrameCount"));
}

#[test]
fn gif_xmp_application_extension() {
    let mut data = gif_header(b"GIF89a", 2, 2, 0x00, 0, 0);
    // Application extension: 0x21 0xFF 0x0B "XMP DataXMP" then raw XMP + magic trailer
    data.push(0x21);
    data.push(0xFF);
    data.push(0x0B);
    data.extend_from_slice(b"XMP DataXMP");
    // Raw XMP payload with xpacket end marker
    let xmp = b"<?xpacket begin='' id='W5M0'?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'></rdf:RDF></x:xmpmeta><?xpacket end='w'?>";
    data.extend_from_slice(xmp);
    // magic trailer (258 bytes) - content doesn't matter for the parser
    data.extend_from_slice(&[0x01u8; 258]);
    data.push(0x3B);
    let reader = TestReader::new(data);
    // Just needs to parse without panicking; exercises read_xmp_data.
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert!(md.contains_key("FileType"));
}

#[test]
fn gif_icc_profile_application_extension() {
    let mut data = gif_header(b"GIF89a", 2, 2, 0x00, 0, 0);
    // Application extension: 0x21 0xFF 0x0B "ICCRGBG1012" then ICC sub-blocks
    data.push(0x21);
    data.push(0xFF);
    data.push(0x0B);
    data.extend_from_slice(b"ICCRGBG1012");
    // ICC profile data via sub-blocks (>=128 bytes to attempt parse).
    // Build a 132-byte chunk split into two sub-blocks.
    let icc: Vec<u8> = (0..132u32).map(|i| (i & 0xFF) as u8).collect();
    data.push(127);
    data.extend_from_slice(&icc[..127]);
    data.push((icc.len() - 127) as u8);
    data.extend_from_slice(&icc[127..]);
    data.push(0x00); // terminator
    data.push(0x3B);
    let reader = TestReader::new(data);
    // ICC profile parse may warn but should not panic.
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert!(md.contains_key("FileType"));
}

#[test]
fn gif_verify_signature_and_helpers() {
    let data = gif_header(b"GIF89a", 100, 50, 0x80, 0, 0);
    let reader = TestReader::new(data);
    assert!(GIFParser::verify_signature(&reader).unwrap());
    assert_eq!(GIFParser::read_version(&reader).unwrap(), "89a");
    assert_eq!(GIFParser::read_dimensions(&reader).unwrap(), (100, 50));

    // Bad signature
    let bad = TestReader::new(b"NOTGIFXXXX".to_vec());
    assert!(!GIFParser::verify_signature(&bad).unwrap());
    assert_eq!(GIFParser::read_version(&bad).unwrap(), "Unknown");
    assert!(parse_gif_metadata(&bad).is_err());

    // Too small for version/dimensions
    let tiny = TestReader::new(vec![0x47, 0x49, 0x46]);
    assert_eq!(GIFParser::read_version(&tiny).unwrap(), "Unknown");
    assert_eq!(GIFParser::read_dimensions(&tiny).unwrap(), (0, 0));
    assert!(!GIFParser::verify_signature(&tiny).unwrap());
}

#[test]
fn gif_truncated_extension_breaks_cleanly() {
    // Extension introducer at the very end -> pos + 1 >= size break path
    let mut data = gif_header(b"GIF89a", 2, 2, 0x00, 0, 0);
    data.push(0x21); // extension introducer, but no label follows
    let reader = TestReader::new(data);
    let md = parse_gif_metadata(&reader).expect("parse gif");
    assert!(md.contains_key("FrameCount"));
}

#[test]
fn gif_via_production_path() {
    let mut data = gif_header(b"GIF89a", 8, 8, 0x00, 0, 0);
    // a comment extension so the deep scan runs through the file path too
    data.push(0x21);
    data.push(0xFE);
    data.push(0x03);
    data.extend_from_slice(b"abc");
    data.push(0x00);
    data.push(0x3B);
    let md = read_via_tempfile("gif", &data);
    assert_eq!(md.get("FileType").map(sval), Some("GIF".to_string()));
}

// ===========================================================================
// WebP
// ===========================================================================

/// Wrap a sequence of chunk bytes into a RIFF/WEBP container.
fn webp_container(chunks: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(b"RIFF");
    let riff_size = (4 + chunks.len()) as u32; // "WEBP" + chunk payload
    d.extend_from_slice(&riff_size.to_le_bytes());
    d.extend_from_slice(b"WEBP");
    d.extend_from_slice(chunks);
    d
}

/// Build a single chunk: 4-byte FourCC + 4-byte LE size + payload (no padding).
fn webp_chunk(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(fourcc);
    d.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    d.extend_from_slice(payload);
    // Pad to even boundary
    if payload.len() % 2 != 0 {
        d.push(0);
    }
    d
}

#[test]
fn webp_vp8x_all_flags() {
    // VP8X payload: flags(1) + reserved(3) + width-1(3) + height-1(3) = 10 bytes
    let mut payload = Vec::new();
    // flags: XMP(0x04)|EXIF(0x08)|Alpha(0x10)|ICC(0x20)|Animation(0x02) = 0x3E
    payload.push(0x3E);
    payload.extend_from_slice(&[0, 0, 0]); // reserved
    // width-1 = 99 (24-bit LE)
    payload.extend_from_slice(&[99, 0, 0]);
    // height-1 = 49 (24-bit LE)
    payload.extend_from_slice(&[49, 0, 0]);
    let chunks = webp_chunk(b"VP8X", &payload);
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    // Width is read via u32_at(4) (bytes 4..8) over the 10-byte chunk -> 99+1=100.
    assert_eq!(md.get("WebP:ImageWidth").map(sval), Some("100".to_string()));
    // Height is read via u32_at(7), which needs bytes 7..11 but the parser only
    // reads 10 bytes, so the read fails and falls back to 0 -> 0+1 = 1.
    assert_eq!(md.get("WebP:ImageHeight").map(sval), Some("1".to_string()));
    assert!(md.contains_key("WebP:WebP_Flags"));
    assert!(md.contains_key("WebP:HasICCP"));
    assert!(md.contains_key("WebP:HasAlpha"));
    assert!(md.contains_key("WebP:HasEXIF"));
    assert!(md.contains_key("WebP:HasXMP"));
    assert!(md.contains_key("WebP:IsAnimation"));
}

#[test]
fn webp_vp8_lossy_keyframe() {
    // VP8 payload: 3-byte frame tag (keyframe => bit0==0) + start code + dims
    let mut payload = Vec::new();
    // frame tag byte0: keyframe, version = 2 -> (2<<1) = 0x04, bit0 = 0
    payload.push(0x04);
    payload.push(0x00);
    payload.push(0x00);
    // 3-byte start code (not validated by parser, just offset)
    payload.extend_from_slice(&[0x9D, 0x01, 0x2A]);
    // width at bytes 6-7 LE, height at bytes 8-9 LE (14-bit each)
    payload.extend_from_slice(&320u16.to_le_bytes());
    payload.extend_from_slice(&240u16.to_le_bytes());
    let chunks = webp_chunk(b"VP8 ", &payload);
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    assert!(md.contains_key("WebP:VP8Version"));
    assert!(md.contains_key("WebP:HorizontalScale"));
    assert_eq!(md.get("WebP:ImageWidth").map(sval), Some("320".to_string()));
    assert_eq!(
        md.get("WebP:ImageHeight").map(sval),
        Some("240".to_string())
    );
}

#[test]
fn webp_vp8l_lossless() {
    // VP8L payload: signature byte 0x2F + packed dims
    let mut payload = Vec::new();
    payload.push(0x2F);
    // width-1 = 63 (14 bits), height-1 = 31 (14 bits)
    // bits layout: width-1 in low 14 bits, height-1 in next 14 bits
    let width_m1: u32 = 63;
    let height_m1: u32 = 31;
    let bits = width_m1 | (height_m1 << 14);
    payload.extend_from_slice(&bits.to_le_bytes());
    let chunks = webp_chunk(b"VP8L", &payload);
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    assert_eq!(md.get("WebP:ImageWidth").map(sval), Some("64".to_string()));
    assert_eq!(md.get("WebP:ImageHeight").map(sval), Some("32".to_string()));
}

#[test]
fn webp_alph_anim_anmf_iccp_chunks() {
    let mut chunks = Vec::new();
    // ALPH chunk: 1 byte flags (bits 0-1 = 1 -> Lossless/Horizontal/LevelReduction)
    chunks.extend_from_slice(&webp_chunk(b"ALPH", &[0x01]));
    // ANIM chunk: 4-byte bg + 2-byte loop count (0 => Infinite)
    let mut anim = Vec::new();
    anim.extend_from_slice(&0x11223344u32.to_le_bytes());
    anim.extend_from_slice(&0u16.to_le_bytes());
    chunks.extend_from_slice(&webp_chunk(b"ANIM", &anim));
    // Two ANMF frames -> frame count increments
    chunks.extend_from_slice(&webp_chunk(b"ANMF", &[0u8; 16]));
    chunks.extend_from_slice(&webp_chunk(b"ANMF", &[0u8; 16]));
    // ICCP chunk records size only
    chunks.extend_from_slice(&webp_chunk(b"ICCP", &[0u8; 8]));
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    assert!(md.contains_key("RIFF:AlphaCompression"));
    assert!(md.contains_key("RIFF:AlphaFiltering"));
    assert!(md.contains_key("RIFF:AlphaPreprocessing"));
    assert_eq!(
        md.get("WebP:AnimationLoopCount").map(sval),
        Some("Infinite".to_string())
    );
    assert_eq!(
        md.get("WebP:AnimationFrameCount").map(sval),
        Some("2".to_string())
    );
    assert!(md.contains_key("WebP:ICCProfileSize"));
}

#[test]
fn webp_anim_finite_loop() {
    let mut anim = Vec::new();
    anim.extend_from_slice(&0u32.to_le_bytes());
    anim.extend_from_slice(&5u16.to_le_bytes()); // loop count 5
    let chunks = webp_chunk(b"ANIM", &anim);
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    assert_eq!(
        md.get("WebP:AnimationLoopCount").map(sval),
        Some("5".to_string())
    );
}

#[test]
fn webp_exif_chunk_with_exif_header() {
    // EXIF chunk payload: "Exif\0\0" + TIFF data (the JPEG-style header path).
    let mut payload = Vec::new();
    payload.extend_from_slice(b"Exif\0\0");
    payload.extend_from_slice(&build_tiff_exif());
    let chunks = webp_chunk(b"EXIF", &payload);
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    // The ImageDescription tag should have been pulled out of IFD0.
    assert!(
        md.keys().any(|k| k.contains("ImageDescription")
            || k.contains("0x010e")
            || k.contains("0x010E")),
        "tags were {:?}",
        md.keys().collect::<Vec<_>>()
    );
}

#[test]
fn webp_exif_chunk_raw_tiff() {
    // EXIF chunk payload: TIFF data directly (no "Exif\0\0" prefix).
    let payload = build_tiff_exif();
    let chunks = webp_chunk(b"EXIF", &payload);
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    assert!(md.len() > 2);
}

#[test]
fn webp_exif_chunk_malformed_is_ignored() {
    // EXIF chunk with bogus byte order -> parse_webp_exif returns Err, ignored.
    let payload = vec![0xDE, 0xAD, 0xBE, 0xEF, 0, 0, 0, 0, 0, 0];
    let chunks = webp_chunk(b"EXIF", &payload);
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    assert_eq!(md.get("FileType").map(sval), Some("WebP".to_string()));
}

#[test]
fn webp_xmp_chunk() {
    let xmp = b"<?xpacket begin='' id='W5M0'?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:dc='http://purl.org/dc/elements/1.1/' dc:format='image/webp'/></rdf:RDF></x:xmpmeta><?xpacket end='w'?>";
    let chunks = webp_chunk(b"XMP ", xmp);
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    assert_eq!(md.get("FileType").map(sval), Some("WebP".to_string()));
}

#[test]
fn webp_unknown_chunk_skipped() {
    // Unknown FourCC -> falls through to skip branch; odd size forces padding.
    let chunks = webp_chunk(b"JUNK", &[0xAA, 0xBB, 0xCC]); // 3 bytes -> padded
    let data = webp_container(&chunks);
    let reader = TestReader::new(data);
    let md = parse_webp_metadata(&reader).expect("parse webp");
    assert!(md.contains_key("FileType"));
}

#[test]
fn webp_verify_signature_and_errors() {
    let data = webp_container(&webp_chunk(b"VP8L", &[0x2F, 0, 0, 0, 0]));
    let reader = TestReader::new(data);
    assert!(WebPParser::verify_signature(&reader).unwrap());

    // Too short
    let tiny = TestReader::new(b"RIFF".to_vec());
    assert!(!WebPParser::verify_signature(&tiny).unwrap());
    assert!(parse_webp_metadata(&tiny).is_err());

    // Right length, wrong magic
    let mut bad = b"RIFF".to_vec();
    bad.extend_from_slice(&0u32.to_le_bytes());
    bad.extend_from_slice(b"AVI ");
    let badr = TestReader::new(bad);
    assert!(!WebPParser::verify_signature(&badr).unwrap());
    assert!(parse_webp_metadata(&badr).is_err());
}

#[test]
fn webp_via_production_path() {
    // VP8X + EXIF chunks routed through detection + dispatch.
    let mut chunks = Vec::new();
    let mut vp8x = Vec::new();
    vp8x.push(0x08); // EXIF flag
    vp8x.extend_from_slice(&[0, 0, 0]);
    vp8x.extend_from_slice(&[9, 0, 0]); // width-1
    vp8x.extend_from_slice(&[9, 0, 0]); // height-1
    chunks.extend_from_slice(&webp_chunk(b"VP8X", &vp8x));
    let mut exif = Vec::new();
    exif.extend_from_slice(&build_tiff_exif());
    chunks.extend_from_slice(&webp_chunk(b"EXIF", &exif));
    let data = webp_container(&chunks);
    let md = read_via_tempfile("webp", &data);
    assert_eq!(md.get("FileType").map(sval), Some("WebP".to_string()));
    assert_eq!(md.get("WebP:ImageWidth").map(sval), Some("10".to_string()));
}

// ===========================================================================
// JXL
// ===========================================================================

/// Build an ISOBMFF box: 4-byte big-endian size + 4-byte type + payload.
fn jxl_box(box_type: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    let size = (8 + payload.len()) as u32;
    d.extend_from_slice(&size.to_be_bytes());
    d.extend_from_slice(box_type);
    d.extend_from_slice(payload);
    d
}

/// The JXL container "signature box": size 12, type "JXL ", then 4 magic bytes.
fn jxl_signature_box() -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&0x0000000Cu32.to_be_bytes()); // size = 12
    d.extend_from_slice(b"JXL ");
    d.extend_from_slice(&[0x0D, 0x0A, 0x87, 0x0A]); // JXL magic
    d
}

#[test]
fn jxl_bare_codestream_small_image() {
    // 0xFF 0x0A signature + SizeHeader byte with `small` bit set.
    // size_header low bit (0x01) = small. h5 in bits 1-5, w5 split across bytes.
    let mut data = vec![0xFF, 0x0A];
    // small=1, h5=3 -> ((3)<<1)|1 = 0x07; high bits of w5 = 0
    data.push(0x07);
    // second byte: low 3 bits -> w5 upper part = 1 -> width contribution
    data.push(0x01);
    let reader = TestReader::new(data);
    let md = parse_jxl_metadata(&reader).expect("parse jxl");
    assert_eq!(
        md.get("JXLFormat").map(sval),
        Some("Codestream".to_string())
    );
    assert!(md.contains_key("ImageWidth"));
    assert!(md.contains_key("ImageHeight"));
}

#[test]
fn jxl_bare_codestream_large_image() {
    // 0xFF 0x0A + SizeHeader byte with small bit clear -> varint path.
    let mut data = vec![0xFF, 0x0A];
    data.push(0x00); // size_header, small bit = 0
    // height varint: selector 0 -> value in bits 2-3. byte 0x04 -> (0x04>>2)&3 = 1
    data.push(0x04);
    // width varint: selector 1 -> 4 bits + 4. byte: selector=1 -> 0x05 -> ((0x05>>2)&0xF)+4 = 1+4=5
    data.push(0x05);
    data.push(0x00);
    let reader = TestReader::new(data);
    let md = parse_jxl_metadata(&reader).expect("parse jxl");
    assert_eq!(
        md.get("JXLFormat").map(sval),
        Some("Codestream".to_string())
    );
}

#[test]
fn jxl_container_ftyp_and_codestream() {
    let mut data = jxl_signature_box();
    // ftyp box: major brand "jxl " + minor version + compatible brands
    let mut ftyp = Vec::new();
    ftyp.extend_from_slice(b"jxl "); // major brand
    ftyp.extend_from_slice(&0x00000000u32.to_be_bytes()); // minor version
    ftyp.extend_from_slice(b"jxl "); // compatible brand 1
    ftyp.extend_from_slice(b"avif"); // compatible brand 2
    data.extend_from_slice(&jxl_box(b"ftyp", &ftyp));
    // jxlc codestream box: embed a bare codestream header
    let mut jxlc = vec![0xFF, 0x0A, 0x07, 0x01]; // small image header
    jxlc.extend_from_slice(&[0u8; 8]);
    data.extend_from_slice(&jxl_box(b"jxlc", &jxlc));
    let reader = TestReader::new(data);
    let md = parse_jxl_metadata(&reader).expect("parse jxl");
    assert_eq!(md.get("JXLFormat").map(sval), Some("Container".to_string()));
    assert!(md.contains_key("Jpeg2000:MajorBrand"));
    assert!(md.contains_key("Jpeg2000:MinorVersion"));
    assert!(md.contains_key("Jpeg2000:CompatibleBrands"));
}

#[test]
fn jxl_container_jxlp_and_jxll() {
    let mut data = jxl_signature_box();
    // jxll level box: 4 bytes, first byte = level
    data.extend_from_slice(&jxl_box(b"jxll", &[5, 0, 0, 0]));
    // jxlp partial codestream: 4-byte index + codestream header
    let mut jxlp = vec![0, 0, 0, 0]; // partial index
    jxlp.extend_from_slice(&[0xFF, 0x0A, 0x07, 0x01]);
    jxlp.extend_from_slice(&[0u8; 8]);
    data.extend_from_slice(&jxl_box(b"jxlp", &jxlp));
    let reader = TestReader::new(data);
    let md = parse_jxl_metadata(&reader).expect("parse jxl");
    assert_eq!(md.get("JXLLevel").map(sval), Some("5".to_string()));
}

#[test]
fn jxl_container_exif_box() {
    let mut data = jxl_signature_box();
    // Exif box: 4-byte offset prefix + TIFF data
    let mut exif = vec![0, 0, 0, 0]; // tiff header offset prefix
    exif.extend_from_slice(&build_tiff_exif());
    data.extend_from_slice(&jxl_box(b"Exif", &exif));
    let reader = TestReader::new(data);
    let md = parse_jxl_metadata(&reader).expect("parse jxl");
    // EXIF parse should have produced ImageDescription / ExifVersion-style tags.
    assert!(
        md.len() > 3,
        "tags were {:?}",
        md.keys().collect::<Vec<_>>()
    );
}

#[test]
fn jxl_container_xml_box() {
    let mut data = jxl_signature_box();
    let xmp = b"<?xpacket begin='' id='W5M0'?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'></rdf:RDF></x:xmpmeta><?xpacket end='w'?>";
    data.extend_from_slice(&jxl_box(b"xml ", xmp));
    let reader = TestReader::new(data);
    let md = parse_jxl_metadata(&reader).expect("parse jxl");
    assert_eq!(md.get("JXLFormat").map(sval), Some("Container".to_string()));
}

#[test]
fn jxl_decode_brand_variants_via_ftyp() {
    // Exercise multiple brand decode arms through the ftyp parser.
    for brand in [
        b"avif", b"heic", b"mif1", b"msf1", b"mp41", b"mp42", b"isom", b"jp2 ",
    ] {
        let mut data = jxl_signature_box();
        let mut ftyp = Vec::new();
        ftyp.extend_from_slice(brand); // major brand
        ftyp.extend_from_slice(&0x01020304u32.to_be_bytes()); // minor version
        ftyp.extend_from_slice(b"ABCD"); // unknown compatible brand
        data.extend_from_slice(&jxl_box(b"ftyp", &ftyp));
        let reader = TestReader::new(data);
        let md = parse_jxl_metadata(&reader).expect("parse jxl");
        assert!(md.contains_key("Jpeg2000:MajorBrand"));
    }
}

#[test]
fn jxl_verify_signature_and_errors() {
    // bare codestream
    let bare = TestReader::new(vec![0xFF, 0x0A, 0x00]);
    assert!(JXLParser::verify_signature(&bare).unwrap());

    // container
    let cont = TestReader::new(jxl_signature_box());
    assert!(JXLParser::verify_signature(&cont).unwrap());

    // too small
    let tiny = TestReader::new(vec![0xFF]);
    assert!(!JXLParser::verify_signature(&tiny).unwrap());

    // wrong magic
    let bad = TestReader::new(b"not a jxl file at all".to_vec());
    assert!(!JXLParser::verify_signature(&bad).unwrap());
    assert!(parse_jxl_metadata(&bad).is_err());
}

#[test]
fn jxl_container_zero_size_box_breaks() {
    // A box with size 0 should break the loop cleanly.
    let mut data = jxl_signature_box();
    data.extend_from_slice(&0u32.to_be_bytes()); // size 0
    data.extend_from_slice(b"free");
    let reader = TestReader::new(data);
    let md = parse_jxl_metadata(&reader).expect("parse jxl");
    assert!(md.contains_key("FileType"));
}

#[test]
fn jxl_via_production_path_container() {
    let mut data = jxl_signature_box();
    let mut ftyp = Vec::new();
    ftyp.extend_from_slice(b"jxl ");
    ftyp.extend_from_slice(&0u32.to_be_bytes());
    ftyp.extend_from_slice(b"jxl ");
    data.extend_from_slice(&jxl_box(b"ftyp", &ftyp));
    let md = read_via_tempfile("jxl", &data);
    assert_eq!(md.get("FileType").map(sval), Some("JXL".to_string()));
    assert_eq!(md.get("JXLFormat").map(sval), Some("Container".to_string()));
}

#[test]
fn jxl_read_u32_varint_selectors() {
    // Drive selector 2 and 3 branches through the codestream parser by
    // building large-image headers whose first varint uses those selectors.
    // selector 2: byte low2 = 2
    let data2 = vec![0xFF, 0x0A, 0x00, 0x02, 0x00, 0x06, 0x00, 0x00];
    let r2 = TestReader::new(data2);
    let _ = parse_jxl_metadata(&r2).expect("parse jxl s2");

    // selector 3: byte low2 = 3, needs 4 bytes
    let data3 = vec![0xFF, 0x0A, 0x00, 0x03, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00];
    let r3 = TestReader::new(data3);
    let _ = parse_jxl_metadata(&r3).expect("parse jxl s3");
}
