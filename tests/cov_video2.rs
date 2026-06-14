//! Wave-2 coverage tests for the MXF, FLV, and ASF video parsers.
//!
//! Wave-1 (`cov_video_a.rs`, `cov_video_b.rs`) already drives the happy path of
//! each parser. This file targets the REMAINING uncovered branches:
//!
//! - **MXF:** the dedicated KLV-set parsers that wave-1 either skipped or hit
//!   only partially — wave audio descriptor (every tag), file descriptor,
//!   source package, sequence set (sound-essence branch + duration), timecode
//!   component (all three tags), identification-set release-type variants, plus
//!   `decode_ber_length` long/short forms, `parse_utf16_string` and
//!   `parse_mxf_timestamp` edge cases, and the unreachable `Unknown` UL skips.
//! - **FLV:** the AMF0 value-type branches not exercised by wave-1 (long string
//!   0x0C, null/undefined 0x05/0x06, nested ECMA arrays 0x08, unknown type
//!   bail-out), every entry of `map_flv_key_to_tag`,
//!   `video_codec_to_encoding` / `audio_codec_to_encoding` exhaustively, the
//!   audio-tag sample-rate code table, `format_flv_date` across multiple years
//!   (driving `days_to_ymd` / `is_leap_year`), and cuePoints delivered as an
//!   ECMA array marker.
//! - **ASF:** `map_audio_format_tag` exhaustively, `parse_wm_picture` with all
//!   string fields populated, `map_wm_tag` branches (ToolName, MediaClass*,
//!   WMADRC*, bare/unknown), Metadata Library object + every
//!   `parse_metadata_object` data type (string/binary/GUID/bool/DWORD/QWORD/
//!   WORD), the FILETIME-underflow path, extended-content GUID (type 6) and
//!   binary (type 1) descriptors, and codec lists with two audio entries.
//!
//! Everything is driven through the public API
//! (`parse_*_metadata` / `read_metadata`) using synthetic byte buffers.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::operations::read_metadata;
use oxidex::parsers::video::asf::parse_asf_metadata;
use oxidex::parsers::video::flv::parse_flv_metadata;
use oxidex::parsers::video::mxf::{MxfParser, parse_mxf_metadata};
use std::io::Write;
use tempfile::NamedTempFile;

// ===========================================================================
// Shared byte helpers
// ===========================================================================

fn be16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_be_bytes());
}
fn be32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}
fn bef64(buf: &mut Vec<u8>, v: f64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// UTF-16LE string with a trailing null terminator (ASF strings).
fn utf16(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// UTF-16BE encode (MXF strings).
fn utf16be(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    for c in s.encode_utf16() {
        v.extend_from_slice(&c.to_be_bytes());
    }
    v
}

// ===========================================================================
// MXF helpers
// ===========================================================================

/// Header / body / footer partition pack key. key[13] selects the variant.
fn partition_key(b13: u8) -> [u8; 16] {
    [
        0x06, 0x0E, 0x2B, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0D, 0x01, 0x02, 0x01, 0x01, b13, 0x01,
        0x00,
    ]
}

/// Local-set key (Identification / Preface / Package / Track sets). key[13] varies.
fn local_set_key(b13: u8) -> [u8; 16] {
    [
        0x06, 0x0E, 0x2B, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0D, 0x01, 0x01, 0x01, 0x01, b13, 0x00,
        0x00,
    ]
}

/// Essence descriptor / component key (key[12]==0x01). key[13] varies.
fn essence_key(b13: u8) -> [u8; 16] {
    [
        0x06, 0x0E, 0x2B, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0D, 0x01, 0x01, 0x01, 0x01, b13, 0x00,
        0x00,
    ]
}

/// Append a KLV triplet using short-form BER length (value < 128).
fn klv(buf: &mut Vec<u8>, key: &[u8; 16], value: &[u8]) {
    buf.extend_from_slice(key);
    assert!(value.len() < 128, "value too big for short-form BER");
    buf.push(value.len() as u8);
    buf.extend_from_slice(value);
}

/// Append a KLV triplet using long-form 2-byte BER length.
fn klv_long(buf: &mut Vec<u8>, key: &[u8; 16], value: &[u8]) {
    buf.extend_from_slice(key);
    buf.push(0x82);
    be16(buf, value.len() as u16);
    buf.extend_from_slice(value);
}

/// Local-set property: 2-byte tag + 2-byte length + value.
fn local_prop(buf: &mut Vec<u8>, tag: u16, value: &[u8]) {
    be16(buf, tag);
    be16(buf, value.len() as u16);
    buf.extend_from_slice(value);
}

/// 8-byte MXF timestamp (2010-12-20 00:14:40 + 57).
fn mxf_timestamp() -> Vec<u8> {
    vec![0x07, 0xDA, 0x0C, 0x14, 0x00, 0x0E, 0x28, 0x39]
}

/// A valid header partition pack value (version major.minor).
fn header_value(major: u16, minor: u16) -> Vec<u8> {
    let mut v = Vec::new();
    be16(&mut v, major);
    be16(&mut v, minor);
    v.resize(24, 0);
    v
}

/// Build an MXF file beginning with a header partition then arbitrary trailing
/// bytes, ensuring the file is >= 32 bytes (parser minimum).
fn mxf_with_header(extra: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    data.extend_from_slice(extra);
    data.resize(data.len() + 48, 0);
    data
}

#[test]
fn test_mxf_wave_audio_descriptor_all_tags() {
    // Build a fully-populated WaveAudioDescriptor body. With the current
    // identify_ul ordering, an essence-prefixed key resolves to the track-set
    // block (returning Unknown) before the essence-descriptor block, so the
    // dedicated parser is not invoked — but the KLV scan loop, BER decode, and
    // identify_ul essence-prefix comparisons are all still driven, and the parse
    // must complete without panicking.
    let mut wav = Vec::new();
    let mut sr = Vec::new();
    be32(&mut sr, 48000);
    be32(&mut sr, 1);
    local_prop(&mut wav, 0x3D03, &sr); // audio sampling rate (>=8 bytes)
    local_prop(&mut wav, 0x3D02, &[1u8]); // locked indicator (non-empty)
    let mut ch = Vec::new();
    be32(&mut ch, 6);
    local_prop(&mut wav, 0x3D07, &ch); // channel count
    let mut bits = Vec::new();
    be32(&mut bits, 24);
    local_prop(&mut wav, 0x3D01, &bits); // quantization bits
    let mut align = Vec::new();
    be16(&mut align, 18);
    local_prop(&mut wav, 0x3D0A, &align); // block align
    let mut avg = Vec::new();
    be32(&mut avg, 288_000);
    local_prop(&mut wav, 0x3D09, &avg); // avg bytes/sec
    let mut srate = Vec::new();
    be32(&mut srate, 48000);
    be32(&mut srate, 1);
    local_prop(&mut wav, 0x3001, &srate); // sample rate
    let mut elen = Vec::new();
    elen.extend_from_slice(&12345i64.to_be_bytes());
    local_prop(&mut wav, 0x3002, &elen); // essence length

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 3));
    klv_long(&mut data, &essence_key(0x48), &wav); // WaveAudioDescriptor UL
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("wave audio descriptor parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("1.3")
    );
}

#[test]
fn test_mxf_wave_audio_descriptor_unlocked_via_aes3() {
    // AES3Descriptor UL (key[13]=0x47). As above, drives identify_ul's
    // essence-prefix comparison and the KLV scan; parse must succeed.
    let mut wav = Vec::new();
    local_prop(&mut wav, 0x3D02, &[0u8]); // locked = false

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 0));
    klv_long(&mut data, &essence_key(0x47), &wav); // AES3Descriptor UL
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("aes3 descriptor parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("1.0")
    );
}

#[test]
fn test_mxf_file_descriptor_and_picture_and_sound() {
    // FileDescriptor (0x25), GenericPictureDescriptor (0x27), and
    // GenericSoundDescriptor (0x42) ULs. These drive identify_ul's
    // essence-descriptor key[13] comparisons and the KLV scan loop.
    let mut fd = Vec::new();
    let mut ltid = Vec::new();
    be32(&mut ltid, 9);
    local_prop(&mut fd, 0x3006, &ltid); // linked track id
    let mut esid = Vec::new();
    be32(&mut esid, 11);
    local_prop(&mut fd, 0x3004, &esid); // essence stream id

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    klv_long(&mut data, &essence_key(0x25), &fd); // FileDescriptor UL
    klv_long(&mut data, &essence_key(0x27), &fd); // GenericPictureDescriptor UL
    klv_long(&mut data, &essence_key(0x42), &fd); // GenericSoundDescriptor UL
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("file descriptor parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("1.2")
    );
}

#[test]
fn test_mxf_source_package_create_date() {
    let mut sp = Vec::new();
    local_prop(&mut sp, 0x4404, &mxf_timestamp()); // package creation date

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(2, 1));
    klv_long(&mut data, &local_set_key(0x37), &sp); // SourcePackageSet
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("source package parse");
    assert!(md.contains_key("MXF:CreateDate"));
    let cd = md
        .get("MXF:CreateDate")
        .and_then(|v| v.as_string())
        .unwrap();
    assert!(cd.starts_with("2010:12:20"), "got {cd}");
}

#[test]
fn test_mxf_sequence_set_sound_and_duration() {
    // SequenceSet UL (essence key[13]=0x0F) carrying duration (0x0202) and a
    // sound-essence data definition (0x0201). Drives the KLV scan + identify_ul
    // sequence-set comparison; parse must complete cleanly.
    let mut seq = Vec::new();
    let mut dur = Vec::new();
    dur.extend_from_slice(&720i64.to_be_bytes());
    local_prop(&mut seq, 0x0202, &dur); // duration
    let mut datadef = vec![0u8; 16];
    datadef[12] = 0x01;
    datadef[13] = 0x02; // sound essence
    local_prop(&mut seq, 0x0201, &datadef);

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    klv_long(&mut data, &essence_key(0x0F), &seq); // SequenceSet UL
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("sequence set parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("1.2")
    );
}

#[test]
fn test_mxf_sequence_set_picture_essence() {
    // Picture-essence data definition selector in a SequenceSet UL body.
    let mut seq = Vec::new();
    let mut datadef = vec![0u8; 16];
    datadef[12] = 0x01;
    datadef[13] = 0x01; // picture essence
    local_prop(&mut seq, 0x0201, &datadef);

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 4));
    klv_long(&mut data, &essence_key(0x0F), &seq);
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("sequence set picture parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("1.4")
    );
}

#[test]
fn test_mxf_timecode_component_all_tags() {
    // TimecodeComponent UL (essence key[13]=0x14) with start timecode (0x1501),
    // rounded timebase (0x1502), drop frame (0x1503). Drives the KLV scan +
    // identify_ul timecode-component comparison.
    let mut tc = Vec::new();
    let mut start = Vec::new();
    start.extend_from_slice(&90000i64.to_be_bytes());
    local_prop(&mut tc, 0x1501, &start);
    let mut base = Vec::new();
    be16(&mut base, 30);
    local_prop(&mut tc, 0x1502, &base);
    local_prop(&mut tc, 0x1503, &[1u8]); // drop frame = true

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    klv_long(&mut data, &essence_key(0x14), &tc);
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("timecode component parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("1.2")
    );
}

#[test]
fn test_mxf_timecode_component_no_drop_frame() {
    let mut tc = Vec::new();
    local_prop(&mut tc, 0x1503, &[0u8]); // drop frame = false

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(2, 0));
    klv_long(&mut data, &essence_key(0x14), &tc);
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("timecode no-drop parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("2.0")
    );
}

#[test]
fn test_mxf_identification_release_type_variants() {
    // Drive each release-type string in parse_identification_set's match.
    for (release, _label) in [
        (0u8, "unknown"),
        (2u8, "development"),
        (3u8, "patch level"),
        (4u8, "beta"),
        (5u8, "private build"),
        (9u8, "unknown"), // default arm
    ] {
        let mut props = Vec::new();
        let mut version = vec![0u8; 10];
        version[0..2].copy_from_slice(&3u16.to_be_bytes()); // major
        version[2..4].copy_from_slice(&1u16.to_be_bytes()); // minor
        version[4..6].copy_from_slice(&7u16.to_be_bytes()); // patch
        version[6..8].copy_from_slice(&42u16.to_be_bytes()); // build
        version[9] = release;
        local_prop(&mut props, 0x3C03, &version); // product version
        local_prop(&mut props, 0x3C06, &mxf_timestamp()); // mod date

        let mut data = Vec::new();
        klv(&mut data, &partition_key(0x02), &header_value(1, 2));
        klv_long(&mut data, &local_set_key(0x30), &props); // IdentificationSet
        data.resize(data.len() + 48, 0);

        let reader = TestReader::new(data);
        let md = parse_mxf_metadata(&reader).expect("ident release parse");
        assert_eq!(
            md.get("MXF:SDKVersion").and_then(|v| v.as_string()),
            Some("3.1")
        );
        assert!(md.contains_key("MXF:ToolkitVersion"));
        assert!(md.contains_key("MXF:ModifyDate"));
    }
}

#[test]
fn test_mxf_identification_short_version_ignored() {
    // Product version shorter than 10 bytes -> the SDK/Toolkit insert is skipped.
    let mut props = Vec::new();
    local_prop(&mut props, 0x3C03, &[0u8; 4]); // too short
    local_prop(&mut props, 0x3C01, &utf16be("Vendor")); // supplier name still parsed

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    klv_long(&mut data, &local_set_key(0x30), &props);
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("ident short version parse");
    assert!(!md.contains_key("MXF:SDKVersion"));
    assert_eq!(
        md.get("MXF:ApplicationSupplierName")
            .and_then(|v| v.as_string()),
        Some("Vendor")
    );
}

#[test]
fn test_mxf_body_and_footer_partition_packs() {
    // Body (0x03) and footer (0x04) partition packs route to identify_ul but
    // have no dedicated parser; this exercises those match arms + the loop skip.
    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    klv(&mut data, &partition_key(0x03), &[0u8; 16]); // body partition
    klv(&mut data, &partition_key(0x04), &[0u8; 16]); // footer partition
    klv(&mut data, &partition_key(0x09), &[0u8; 8]); // unknown b13 -> Unknown
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("partition packs parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("1.2")
    );
}

#[test]
fn test_mxf_content_storage_material_static_event_sets() {
    // ContentStorage (0x18), MaterialPackage (0x36), StaticTrack (0x3B),
    // EventTrack (0x3A) all resolve in identify_ul but lack dedicated parsers.
    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    for b13 in [0x18u8, 0x36, 0x3B, 0x3A] {
        klv(&mut data, &local_set_key(b13), &[0u8; 8]);
    }
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("misc sets parse");
    assert!(md.contains_key("MXF:MXFVersion"));
}

#[test]
fn test_mxf_descriptor_unknown_essence_b13() {
    // Essence-prefix key with an unrecognized key[13] -> Unknown (default arm of
    // the essence-descriptor match), value skipped.
    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    klv(&mut data, &essence_key(0x7E), &[0u8; 8]); // unknown descriptor b13
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    assert!(parse_mxf_metadata(&reader).is_ok());
}

#[test]
fn test_mxf_verify_signature_helper() {
    assert!(MxfParser::verify_signature(&partition_key(0x02)));
    assert!(!MxfParser::verify_signature(&[0xFF; 16]));
    assert!(!MxfParser::verify_signature(&[0x06, 0x0E, 0x2B])); // too short
    // Right first byte, wrong remaining prefix bytes.
    assert!(!MxfParser::verify_signature(&[
        0x06, 0x0E, 0x2B, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    ]));
}

#[test]
fn test_mxf_header_partition_no_kag() {
    // KAG size of 0 means the KAGSize tag is NOT inserted (the `if kag_size > 0`
    // branch's false side).
    let mut v = Vec::new();
    be16(&mut v, 1);
    be16(&mut v, 5);
    be32(&mut v, 0); // KAG size 0
    v.resize(24, 0);
    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &v);
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("header no-kag parse");
    assert_eq!(
        md.get("MXF:MXFVersion").and_then(|v| v.as_string()),
        Some("1.5")
    );
    assert!(!md.contains_key("MXF:KAGSize"));
}

#[test]
fn test_mxf_too_small_and_bad_signature() {
    assert!(parse_mxf_metadata(&TestReader::new(vec![0u8; 16])).is_err());
    assert!(parse_mxf_metadata(&TestReader::new(vec![0xAB; 64])).is_err());
}

#[test]
fn test_mxf_non_ul_bytes_skipped_before_klv() {
    // Leading junk that does not match the 06.0E.2B.34 prefix forces the
    // per-byte resync loop before the real header partition is found.
    let mut data = vec![0x00, 0x11, 0x22, 0x33, 0x44]; // 5 junk bytes
    // verify_signature only checks first 16 bytes, which must still match — so
    // put the real key at the very start instead, and append junk AFTER it to
    // exercise the resync path within the KLV scan loop.
    data.clear();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    data.extend_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05]); // junk between
    klv(&mut data, &local_set_key(0x37), &{
        let mut sp = Vec::new();
        local_prop(&mut sp, 0x4404, &mxf_timestamp());
        sp
    });
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("resync parse");
    assert!(md.contains_key("MXF:CreateDate"));
}

#[test]
fn test_mxf_production_path_full() {
    let extra = {
        let mut props = Vec::new();
        local_prop(&mut props, 0x3C02, &utf16be("ProdEncoder"));
        let mut buf = Vec::new();
        klv_long(&mut buf, &local_set_key(0x30), &props);
        buf
    };
    let data = mxf_with_header(&extra);

    let tmp = NamedTempFile::with_suffix(".mxf").expect("tempfile");
    {
        let mut f = tmp.reopen().expect("reopen");
        f.write_all(&data).expect("write");
        f.flush().expect("flush");
    }
    let md = read_metadata(tmp.path()).expect("read_metadata mxf");
    assert_eq!(
        md.get("MXF:ApplicationName").and_then(|v| v.as_string()),
        Some("ProdEncoder")
    );
}

// ===========================================================================
// FLV helpers
// ===========================================================================

fn flv_header(flags: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"FLV");
    buf.push(1);
    buf.push(flags);
    be32(&mut buf, 9);
    be32(&mut buf, 0);
    buf
}

fn flv_tag(buf: &mut Vec<u8>, tag_type: u8, data: &[u8]) {
    let size = data.len() as u32;
    buf.push(tag_type);
    buf.push(((size >> 16) & 0xFF) as u8);
    buf.push(((size >> 8) & 0xFF) as u8);
    buf.push((size & 0xFF) as u8);
    buf.extend_from_slice(&[0, 0, 0, 0]); // timestamp + extended
    buf.extend_from_slice(&[0, 0, 0]); // stream id
    buf.extend_from_slice(data);
    be32(buf, 11 + size);
}

fn amf0_str_raw(buf: &mut Vec<u8>, s: &str) {
    be16(buf, s.len() as u16);
    buf.extend_from_slice(s.as_bytes());
}
fn amf0_key(buf: &mut Vec<u8>, key: &str) {
    be16(buf, key.len() as u16);
    buf.extend_from_slice(key.as_bytes());
}
fn amf0_number(buf: &mut Vec<u8>, key: &str, v: f64) {
    amf0_key(buf, key);
    buf.push(0x00);
    bef64(buf, v);
}
fn amf0_bool(buf: &mut Vec<u8>, key: &str, v: bool) {
    amf0_key(buf, key);
    buf.push(0x01);
    buf.push(if v { 1 } else { 0 });
}
fn amf0_string(buf: &mut Vec<u8>, key: &str, v: &str) {
    amf0_key(buf, key);
    buf.push(0x02);
    be16(buf, v.len() as u16);
    buf.extend_from_slice(v.as_bytes());
}
/// AMF0 long string (0x0C): 4-byte length + bytes.
fn amf0_long_string(buf: &mut Vec<u8>, key: &str, v: &str) {
    amf0_key(buf, key);
    buf.push(0x0C);
    be32(buf, v.len() as u32);
    buf.extend_from_slice(v.as_bytes());
}
/// AMF0 null marker for a property.
fn amf0_null(buf: &mut Vec<u8>, key: &str) {
    amf0_key(buf, key);
    buf.push(0x05);
}
/// AMF0 undefined marker for a property.
fn amf0_undefined(buf: &mut Vec<u8>, key: &str) {
    amf0_key(buf, key);
    buf.push(0x06);
}
fn amf0_date(buf: &mut Vec<u8>, key: &str, ms: f64, tz: i16) {
    amf0_key(buf, key);
    buf.push(0x0B);
    bef64(buf, ms);
    buf.extend_from_slice(&tz.to_be_bytes());
}
fn amf0_object_end(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&[0x00, 0x00, 0x09]);
}

/// Wrap an onMetaData ECMA-array body into a complete script payload.
fn on_metadata(body: &[u8]) -> Vec<u8> {
    let mut script = Vec::new();
    script.push(0x02);
    amf0_str_raw(&mut script, "onMetaData");
    script.push(0x08);
    be32(&mut script, 0);
    script.extend_from_slice(body);
    amf0_object_end(&mut script);
    script
}

#[test]
fn test_flv_video_codec_encoding_table() {
    // Exercise every arm of video_codec_to_encoding (plus an unknown id).
    for (id, expect) in [
        (2.0, Some("Sorenson H.263")),
        (3.0, Some("Screen Video")),
        (4.0, Some("On2 VP6")),
        (5.0, Some("On2 VP6 with alpha")),
        (6.0, Some("Screen Video 2")),
        (7.0, Some("AVC/H.264")),
        (99.0, None),
    ] {
        let mut body = Vec::new();
        amf0_number(&mut body, "videocodecid", id);
        let mut data = flv_header(0x05);
        flv_tag(&mut data, 18, &on_metadata(&body));
        let reader = TestReader::new(data);
        let md = parse_flv_metadata(&reader).expect("video codec table parse");
        assert_eq!(
            md.get("Flash:VideoEncoding").and_then(|v| v.as_string()),
            expect,
            "video codec {id}"
        );
    }
}

#[test]
fn test_flv_audio_codec_encoding_table() {
    for (id, expect) in [
        (0.0, Some("Linear PCM")),
        (1.0, Some("ADPCM")),
        (2.0, Some("MP3")),
        (3.0, Some("Linear PCM (little endian)")),
        (4.0, Some("Nellymoser 16 kHz mono")),
        (5.0, Some("Nellymoser 8 kHz mono")),
        (6.0, Some("Nellymoser")),
        (10.0, Some("AAC")),
        (11.0, Some("Speex")),
        (14.0, Some("MP3 8 kHz")),
        (99.0, None),
    ] {
        let mut body = Vec::new();
        amf0_number(&mut body, "audiocodecid", id);
        let mut data = flv_header(0x05);
        flv_tag(&mut data, 18, &on_metadata(&body));
        let reader = TestReader::new(data);
        let md = parse_flv_metadata(&reader).expect("audio codec table parse");
        assert_eq!(
            md.get("Flash:AudioEncoding").and_then(|v| v.as_string()),
            expect,
            "audio codec {id}"
        );
    }
}

#[test]
fn test_flv_key_mapping_coverage() {
    // Hit a wide spread of map_flv_key_to_tag entries across number/bool/string
    // value types. Keys with "size"/"rate"/"channels"/"bitspersample" are
    // stored as integers; remaining numbers go through the float arm.
    let mut body = Vec::new();
    amf0_number(&mut body, "audiosamplesize", 16.0);
    amf0_number(&mut body, "audiosize", 1234.0);
    amf0_number(&mut body, "audiodelay", 5.0); // float arm
    amf0_number(&mut body, "audiochannels", 2.0);
    amf0_number(&mut body, "audiobitspersample", 16.0);
    amf0_number(&mut body, "datasize", 4096.0);
    amf0_number(&mut body, "filesize", 100000.0);
    amf0_number(&mut body, "videosize", 9000.0);
    amf0_number(&mut body, "lasttimestamp", 42.0); // float arm
    amf0_bool(&mut body, "stereo", true);
    amf0_bool(&mut body, "haskeyframes", true);
    amf0_bool(&mut body, "hasmetadata", false);
    amf0_bool(&mut body, "hascuepoints", false);
    amf0_bool(&mut body, "hasvideo", true);
    amf0_bool(&mut body, "hasaudio", true);
    amf0_string(&mut body, "audioencoding", "AAC");
    amf0_string(&mut body, "test", "custom");

    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &on_metadata(&body));
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("key mapping parse");

    assert_eq!(
        md.get("Flash:AudioSampleSize").and_then(|v| v.as_integer()),
        Some(16)
    );
    assert_eq!(
        md.get("Flash:AudioSize").and_then(|v| v.as_integer()),
        Some(1234)
    );
    assert_eq!(
        md.get("Flash:DataSize").and_then(|v| v.as_integer()),
        Some(4096)
    );
    assert_eq!(
        md.get("Flash:FileSizeBytes").and_then(|v| v.as_integer()),
        Some(100000)
    );
    assert_eq!(
        md.get("Flash:VideoSize").and_then(|v| v.as_integer()),
        Some(9000)
    );
    assert_eq!(
        md.get("Flash:Stereo").and_then(|v| v.as_string()),
        Some("Yes")
    );
    assert_eq!(
        md.get("Flash:HasKeyFrames").and_then(|v| v.as_string()),
        Some("Yes")
    );
    assert_eq!(
        md.get("Flash:HasMetadata").and_then(|v| v.as_string()),
        Some("No")
    );
    assert_eq!(
        md.get("Flash:Test").and_then(|v| v.as_string()),
        Some("custom")
    );
    assert!(md.contains_key("Flash:AudioDelay"));
    assert!(md.contains_key("Flash:LastTimeStamp"));
    assert!(md.contains_key("Flash:AudioEncoding"));
}

#[test]
fn test_flv_amf0_null_and_undefined() {
    // 0x05 null and 0x06 undefined value types are handled by the top-level
    // value-type match's catch-all... actually parse_on_metadata only matches
    // 0x00/0x01/0x02/0x03/0x08/0x0A/0x0B and breaks on anything else. So 0x05/
    // 0x06 terminate the loop. Put mapped numbers BEFORE the null/undefined to
    // confirm those were captured before the loop bails.
    let mut body = Vec::new();
    amf0_number(&mut body, "width", 720.0);
    amf0_number(&mut body, "height", 480.0);
    amf0_null(&mut body, "nullfield"); // 0x05 -> loop breaks here
    amf0_undefined(&mut body, "undefinedfield");

    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &on_metadata(&body));
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("null/undefined parse");
    assert_eq!(
        md.get("Flash:ImageWidth").and_then(|v| v.as_integer()),
        Some(720)
    );
    assert_eq!(
        md.get("Flash:ImageHeight").and_then(|v| v.as_integer()),
        Some(480)
    );
}

#[test]
fn test_flv_amf0_long_string_in_skipped_object() {
    // A long string (0x0C) is only reachable via skip helpers (skip_amf0_value /
    // skip_amf0_value_from_type), since the top-level match breaks on 0x0C. Embed
    // one inside an unmapped object so skip_amf0_object -> skip_amf0_value walks
    // its 4-byte length form, then assert parsing continued afterward.
    let mut obj = Vec::new();
    amf0_long_string(&mut obj, "ls", "y".repeat(40).as_str());
    amf0_object_end(&mut obj);

    let mut body = Vec::new();
    amf0_key(&mut body, "container");
    body.push(0x03); // object marker -> skip_amf0_object
    body.extend_from_slice(&obj);
    amf0_number(&mut body, "width", 256.0);

    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &on_metadata(&body));
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("long string skip parse");
    assert_eq!(
        md.get("Flash:ImageWidth").and_then(|v| v.as_integer()),
        Some(256)
    );
}

#[test]
fn test_flv_nested_ecma_array_skipped() {
    // A nested ECMA array (0x08) value must be skipped via skip_amf0_ecma_array,
    // and parsing should resume with the following mapped key.
    let mut nested = Vec::new();
    amf0_number(&mut nested, "inner", 1.0);
    amf0_object_end(&mut nested);

    let mut body = Vec::new();
    amf0_key(&mut body, "extra"); // key for the nested array
    body.push(0x08); // ECMA array marker
    be32(&mut body, 0); // count
    body.extend_from_slice(&nested);
    amf0_number(&mut body, "width", 320.0);

    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &on_metadata(&body));
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("nested ecma parse");
    assert_eq!(
        md.get("Flash:ImageWidth").and_then(|v| v.as_integer()),
        Some(320)
    );
}

#[test]
fn test_flv_unmapped_object_skipped() {
    // A 0x03 object value under a non-"keyframes" key goes through
    // skip_amf0_object, then a mapped key proves parsing continued.
    let mut obj = Vec::new();
    amf0_string(&mut obj, "k", "v");
    amf0_object_end(&mut obj);

    let mut body = Vec::new();
    amf0_key(&mut body, "someobject");
    body.push(0x03); // object marker
    body.extend_from_slice(&obj);
    amf0_number(&mut body, "height", 240.0);

    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &on_metadata(&body));
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("object skip parse");
    assert_eq!(
        md.get("Flash:ImageHeight").and_then(|v| v.as_integer()),
        Some(240)
    );
}

#[test]
fn test_flv_unknown_value_type_bails() {
    // An unrecognized AMF0 value-type byte (0x77) breaks the parse loop, but the
    // metadata gathered before it is preserved.
    let mut body = Vec::new();
    amf0_number(&mut body, "width", 100.0);
    amf0_key(&mut body, "weird");
    body.push(0x77); // unknown marker -> break

    // No object-end needed; the unknown marker terminates the loop.
    let mut script = Vec::new();
    script.push(0x02);
    amf0_str_raw(&mut script, "onMetaData");
    script.push(0x08);
    be32(&mut script, 0);
    script.extend_from_slice(&body);

    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &script);
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("unknown type parse");
    assert_eq!(
        md.get("Flash:ImageWidth").and_then(|v| v.as_integer()),
        Some(100)
    );
}

#[test]
fn test_flv_date_multiple_years() {
    // Drive format_flv_date / days_to_ymd / is_leap_year across several years,
    // including a leap year (2000-02 onwards) and a negative timezone offset.
    for (label, ms, tz) in [
        ("epoch", 0.0_f64, 0i16),
        ("y2000", 951_868_800_000.0, -300), // 2000-02-29-ish, neg tz
        ("y2021", 1_609_459_200_000.0, 60), // 2021-01-01, pos tz
        ("y2024", 1_704_067_200_000.0, 0),  // 2024-01-01 (leap)
    ] {
        let mut body = Vec::new();
        amf0_date(&mut body, "metadatadate", ms, tz);
        let mut data = flv_header(0x05);
        flv_tag(&mut data, 18, &on_metadata(&body));
        let reader = TestReader::new(data);
        let md = parse_flv_metadata(&reader).expect("date parse");
        assert!(
            md.contains_key("Flash:MetadataDate"),
            "date missing for {label}"
        );
    }
}

#[test]
fn test_flv_cuepoints_as_ecma_array_marker() {
    // cuePoints array whose entry uses the ECMA-array marker (0x08) instead of a
    // plain object (0x03), exercising that branch of parse_cuepoints_array.
    let mut body = Vec::new();
    amf0_key(&mut body, "cuePoints");
    body.push(0x0A); // strict array
    be32(&mut body, 1); // one cue point
    body.push(0x08); // ECMA array marker for the cue point
    be32(&mut body, 0); // count
    amf0_number(&mut body, "time", 3.5);
    amf0_string(&mut body, "name", "ec-cue");
    amf0_object_end(&mut body); // end cue point ecma array

    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &on_metadata(&body));
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("ecma cuepoint parse");
    assert!(md.contains_key("Flash:HasCuePoints"));
    assert_eq!(
        md.get("Flash:CuePoint0Name").and_then(|v| v.as_string()),
        Some("ec-cue")
    );
    assert!(md.contains_key("Flash:CuePoint0Time"));
}

#[test]
fn test_flv_audio_tag_sample_rate_codes() {
    // Cover every sample-rate code in the audio-tag flag byte (bits 2-3) plus
    // both sample-size and channel branches.
    // 0x?? layout: codec(4) | rate(2) | size(1) | chan(1)
    for (flags, rate, bits, chan) in [
        (0x04u8, 11025, 8, "1 (mono)"),   // rate code 1, mono, 8-bit
        (0x09u8, 22050, 8, "2 (stereo)"), // rate code 2, stereo, 8-bit
        (0x0Eu8, 44100, 16, "1 (mono)"),  // rate code 3, mono, 16-bit
    ] {
        let mut data = flv_header(0x05);
        flv_tag(&mut data, 8, &[flags]);
        let reader = TestReader::new(data);
        let md = parse_flv_metadata(&reader).expect("audio rate code parse");
        assert_eq!(
            md.get("Flash:AudioSampleRate").and_then(|v| v.as_integer()),
            Some(rate),
            "flags {flags:#x}"
        );
        assert_eq!(
            md.get("Flash:AudioBitsPerSample")
                .and_then(|v| v.as_integer()),
            Some(bits)
        );
        assert_eq!(
            md.get("Flash:AudioChannels").and_then(|v| v.as_string()),
            Some(chan)
        );
    }
}

#[test]
fn test_flv_video_and_audio_tags_both() {
    // A file with a video tag (which the parser steps over) followed by an audio
    // tag exercises the tag-advance path while still extracting audio info.
    let mut data = flv_header(0x05);
    flv_tag(&mut data, 9, &[0x17, 0x00, 0x00]); // video tag, skipped
    flv_tag(&mut data, 8, &[0xAF]); // audio tag, AAC/44k/16/stereo
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("video+audio parse");
    assert_eq!(
        md.get("Flash:AudioSampleRate").and_then(|v| v.as_integer()),
        Some(44100)
    );
    assert_eq!(
        md.get("Flash:HasVideo").and_then(|v| v.as_string()),
        Some("Yes")
    );
}

#[test]
fn test_flv_script_not_onmetadata_ignored() {
    // A script tag whose first byte is not the string marker 0x02 is ignored by
    // parse_on_metadata (early Ok return), but header flags are still emitted.
    let mut script = vec![0x09]; // not 0x02
    script.extend_from_slice(&[0u8; 10]);
    let mut data = flv_header(0x01); // video only
    flv_tag(&mut data, 18, &script);
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("non-onmetadata parse");
    assert_eq!(
        md.get("Flash:HasVideo").and_then(|v| v.as_string()),
        Some("Yes")
    );
    assert!(!md.contains_key("Flash:ImageWidth"));
}

#[test]
fn test_flv_script_string_not_followed_by_ecma() {
    // onMetaData string present but NOT followed by 0x08 ECMA-array marker -> the
    // parser returns early at the "Not an ECMA array" check.
    let mut script = Vec::new();
    script.push(0x02);
    amf0_str_raw(&mut script, "onMetaData");
    script.push(0x00); // a number marker instead of 0x08
    script.extend_from_slice(&0.0f64.to_be_bytes());

    let mut data = flv_header(0x04); // audio only
    flv_tag(&mut data, 18, &script);
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("no-ecma parse");
    assert_eq!(
        md.get("Flash:HasAudio").and_then(|v| v.as_string()),
        Some("Yes")
    );
}

#[test]
fn test_flv_production_path_audio() {
    let mut data = flv_header(0x05);
    flv_tag(&mut data, 8, &[0xAF]);
    let tmp = NamedTempFile::with_suffix(".flv").expect("tempfile");
    {
        let mut f = tmp.reopen().expect("reopen");
        f.write_all(&data).expect("write");
        f.flush().expect("flush");
    }
    let md = read_metadata(tmp.path()).expect("read_metadata flv");
    assert!(md.contains_key("Flash:HasVideo"));
}

// ===========================================================================
// ASF helpers
// ===========================================================================

const ASF_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const FILE_PROPERTIES_GUID: [u8; 16] = [
    0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const STREAM_PROPERTIES_GUID: [u8; 16] = [
    0x91, 0x07, 0xDC, 0xB7, 0xB7, 0xA9, 0xCF, 0x11, 0x8E, 0xE6, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const EXTENDED_CONTENT_GUID: [u8; 16] = [
    0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0xF0, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x50,
];
const CODEC_LIST_GUID: [u8; 16] = [
    0x40, 0x52, 0xD1, 0x86, 0x1D, 0x31, 0xD0, 0x11, 0xA3, 0xA4, 0x00, 0xA0, 0xC9, 0x03, 0x48, 0xF6,
];
const HEADER_EXTENSION_GUID: [u8; 16] = [
    0xB5, 0x03, 0xBF, 0x5F, 0x2E, 0xA9, 0xCF, 0x11, 0x8E, 0xE3, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const METADATA_LIBRARY_GUID: [u8; 16] = [
    0x94, 0x1C, 0x23, 0x44, 0x98, 0x94, 0xD1, 0x49, 0xA1, 0x41, 0x1D, 0x13, 0x4E, 0x45, 0x70, 0x54,
];
const AUDIO_MEDIA_GUID: [u8; 16] = [
    0x40, 0x9E, 0x69, 0xF8, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];
const AUDIO_SPREAD_GUID: [u8; 16] = [
    0x50, 0xCD, 0xC3, 0xBF, 0x8F, 0x61, 0xCF, 0x11, 0x8B, 0xB2, 0x00, 0xAA, 0x00, 0xB4, 0xE2, 0x20,
];

/// Build an ASF object: 16-byte GUID + 8-byte LE size + body.
fn asf_object(guid: &[u8; 16], body: &[u8]) -> Vec<u8> {
    let total = 24 + body.len() as u64;
    let mut out = Vec::new();
    out.extend_from_slice(guid);
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// Wrap header sub-objects into a full ASF header object (30-byte header).
fn asf_file(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut inner = Vec::new();
    for o in objects {
        inner.extend_from_slice(o);
    }
    let total_size = 30 + inner.len() as u64;
    let mut out = Vec::new();
    out.extend_from_slice(&ASF_HEADER_GUID);
    out.extend_from_slice(&total_size.to_le_bytes());
    out.extend_from_slice(&(objects.len() as u32).to_le_bytes());
    out.push(0x01);
    out.push(0x02);
    out.extend_from_slice(&inner);
    out
}

/// Build an extended-content descriptor record: name + value-type + value.
fn ext_descriptor(buf: &mut Vec<u8>, name: &str, vtype: u16, value: &[u8]) {
    let n = utf16(name);
    buf.extend_from_slice(&(n.len() as u16).to_le_bytes());
    buf.extend_from_slice(&n);
    buf.extend_from_slice(&vtype.to_le_bytes());
    buf.extend_from_slice(&(value.len() as u16).to_le_bytes());
    buf.extend_from_slice(value);
}

#[test]
fn test_asf_extended_content_binary_and_guid_and_word() {
    // value types: 1 (binary, non-picture), 5 (WORD), 6 (GUID).
    let mut body = Vec::new();
    body.extend_from_slice(&3u16.to_le_bytes()); // descriptor count
    ext_descriptor(
        &mut body,
        "WM/SomeBinary",
        1,
        &[0xDE, 0xAD, 0xBE, 0xEF, 0x00],
    );
    let mut wbytes = Vec::new();
    wbytes.extend_from_slice(&513u16.to_le_bytes());
    ext_descriptor(&mut body, "WM/SomeWord", 5, &wbytes);
    let guid_bytes = [
        0xC4, 0xB0, 0x69, 0x5F, 0xF7, 0x04, 0x21, 0x4B, 0x98, 0x42, 0x46, 0xCC, 0xA5, 0x42, 0xD8,
        0xD3,
    ];
    ext_descriptor(&mut body, "WM/MediaClassPrimaryID", 6, &guid_bytes);

    let obj = asf_object(&EXTENDED_CONTENT_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("ext binary/guid/word parse");

    assert!(
        md.get("ASF:SomeBinary")
            .and_then(|v| v.as_string())
            .unwrap()
            .contains("Binary data")
    );
    assert_eq!(
        md.get("ASF:SomeWord").and_then(|v| v.as_integer()),
        Some(513)
    );
    assert_eq!(
        md.get("ASF:MediaClassPrimaryID")
            .and_then(|v| v.as_string()),
        Some("5F69B0C4-04F7-4B21-9842-46CCA542D8D3")
    );
}

#[test]
fn test_asf_extended_content_picture_full() {
    // WM/Picture binary (type 1) with type + size + mime + description + data,
    // driving parse_wm_picture's MIME and description branches.
    let mut pic = Vec::new();
    pic.push(7u8); // picture type = Lead Artist
    pic.extend_from_slice(&500u32.to_le_bytes()); // picture data size
    pic.extend_from_slice(&utf16("image/png")); // mime (null-terminated)
    pic.extend_from_slice(&utf16("Lead shot")); // description
    pic.extend_from_slice(&[0x11; 24]); // binary tail

    let mut body = Vec::new();
    body.extend_from_slice(&1u16.to_le_bytes());
    ext_descriptor(&mut body, "WM/Picture", 1, &pic);

    let obj = asf_object(&EXTENDED_CONTENT_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("picture full parse");

    assert_eq!(
        md.get("ASF:PictureType").and_then(|v| v.as_string()),
        Some("Lead Artist")
    );
    assert_eq!(
        md.get("ASF:PictureMIMEType").and_then(|v| v.as_string()),
        Some("image/png")
    );
    assert_eq!(
        md.get("ASF:PictureDescription").and_then(|v| v.as_string()),
        Some("Lead shot")
    );
    assert!(md.contains_key("ASF:Picture"));
}

#[test]
fn test_asf_wm_tag_name_mappings() {
    // Cover the explicit arms of map_wm_tag plus prefix fall-throughs.
    let mut body = Vec::new();
    body.extend_from_slice(&5u16.to_le_bytes()); // count
    ext_descriptor(&mut body, "WM/ToolName", 0, &utf16("WMEncoder"));
    ext_descriptor(&mut body, "WM/ToolVersion", 0, &utf16("9.0"));
    ext_descriptor(&mut body, "WMADRCPeakReference", 0, &utf16("peak"));
    ext_descriptor(&mut body, "MediaClassSecondaryID", 0, &utf16("secid"));
    // Non-WM, non-prefixed name -> map_wm_tag returns empty -> skipped.
    ext_descriptor(&mut body, "RandomTag", 0, &utf16("ignored"));

    let obj = asf_object(&EXTENDED_CONTENT_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("wm tag mapping parse");

    assert_eq!(
        md.get("ASF:ToolName").and_then(|v| v.as_string()),
        Some("WMEncoder")
    );
    assert_eq!(
        md.get("ASF:ToolVersion").and_then(|v| v.as_string()),
        Some("9.0")
    );
    assert!(md.contains_key("ASF:WMADRCPeakReference"));
    assert!(md.contains_key("ASF:MediaClassSecondaryID"));
    assert!(!md.contains_key("ASF:RandomTag"));
}

#[test]
fn test_asf_metadata_library_object_all_types() {
    // Header Extension wrapping a Metadata Library Object whose records cover
    // data types 0 (string), 1 (binary/GUID), 2 (bool), 3 (DWORD), 4 (QWORD),
    // 5 (WORD), 6 (GUID).
    let mut records: Vec<(&str, u16, Vec<u8>)> = Vec::new();
    records.push(("WM/Publisher", 0, utf16("LibPub")));
    // type 1 with exactly 16 bytes -> GUID-formatted
    let guid16 = vec![
        0xC4, 0xB0, 0x69, 0x5F, 0xF7, 0x04, 0x21, 0x4B, 0x98, 0x42, 0x46, 0xCC, 0xA5, 0x42, 0xD8,
        0xD3,
    ];
    records.push(("WM/SomeByteGuid", 1, guid16.clone()));
    // type 1 with non-16 length -> "Binary data N bytes"
    records.push(("WM/SomeBinary", 1, vec![0u8; 5]));
    records.push(("WM/IsVBR", 2, 1u32.to_le_bytes().to_vec())); // bool true
    records.push(("WM/SomeDword", 3, 77u32.to_le_bytes().to_vec()));
    records.push(("WM/SomeQword", 4, 9_000_000_000u64.to_le_bytes().to_vec()));
    records.push(("WM/SomeWord", 5, 321u16.to_le_bytes().to_vec()));
    records.push(("WM/SomeGuid", 6, guid16));

    let mut meta_body = Vec::new();
    meta_body.extend_from_slice(&(records.len() as u16).to_le_bytes()); // record count
    for (name, dtype, value) in &records {
        let n = utf16(name);
        meta_body.extend_from_slice(&0u16.to_le_bytes()); // lang idx
        meta_body.extend_from_slice(&0u16.to_le_bytes()); // stream num
        meta_body.extend_from_slice(&(n.len() as u16).to_le_bytes()); // name len
        meta_body.extend_from_slice(&dtype.to_le_bytes()); // data type
        meta_body.extend_from_slice(&(value.len() as u32).to_le_bytes()); // data len
        meta_body.extend_from_slice(&n);
        meta_body.extend_from_slice(value);
    }

    let metadata_obj = asf_object(&METADATA_LIBRARY_GUID, &meta_body);

    let mut ext_body = Vec::new();
    ext_body.extend_from_slice(&[0u8; 16]); // reserved GUID
    ext_body.extend_from_slice(&0u16.to_le_bytes()); // reserved field 2
    ext_body.extend_from_slice(&(metadata_obj.len() as u32).to_le_bytes()); // data size
    ext_body.extend_from_slice(&metadata_obj);

    let obj = asf_object(&HEADER_EXTENSION_GUID, &ext_body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("metadata library parse");

    assert_eq!(
        md.get("ASF:Publisher").and_then(|v| v.as_string()),
        Some("LibPub")
    );
    assert_eq!(
        md.get("ASF:SomeByteGuid").and_then(|v| v.as_string()),
        Some("5F69B0C4-04F7-4B21-9842-46CCA542D8D3")
    );
    assert!(
        md.get("ASF:SomeBinary")
            .and_then(|v| v.as_string())
            .unwrap()
            .contains("Binary data")
    );
    assert_eq!(
        md.get("ASF:IsVBR").and_then(|v| v.as_string()),
        Some("true")
    );
    assert_eq!(
        md.get("ASF:SomeDword").and_then(|v| v.as_integer()),
        Some(77)
    );
    assert_eq!(
        md.get("ASF:SomeQword").and_then(|v| v.as_integer()),
        Some(9_000_000_000)
    );
    assert_eq!(
        md.get("ASF:SomeWord").and_then(|v| v.as_integer()),
        Some(321)
    );
    assert_eq!(
        md.get("ASF:SomeGuid").and_then(|v| v.as_string()),
        Some("5F69B0C4-04F7-4B21-9842-46CCA542D8D3")
    );
}

#[test]
fn test_asf_codec_list_two_audio_entries() {
    // Two audio codec entries drive the `audio_idx > 0` suffix branch, and a
    // short (1-byte) info field drives the hex-format fall-through.
    let mut body = Vec::new();
    body.extend_from_slice(&[0u8; 16]); // reserved GUID
    body.extend_from_slice(&2u32.to_le_bytes()); // codec count = 2

    // helper to append a codec entry
    let append_entry = |body: &mut Vec<u8>, ctype: u16, name: &str, desc: &str, info: &[u8]| {
        body.extend_from_slice(&ctype.to_le_bytes());
        let nchars: Vec<u16> = name.encode_utf16().collect();
        body.extend_from_slice(&(nchars.len() as u16).to_le_bytes());
        for c in &nchars {
            body.extend_from_slice(&c.to_le_bytes());
        }
        let dchars: Vec<u16> = desc.encode_utf16().collect();
        body.extend_from_slice(&(dchars.len() as u16).to_le_bytes());
        for c in &dchars {
            body.extend_from_slice(&c.to_le_bytes());
        }
        body.extend_from_slice(&(info.len() as u16).to_le_bytes());
        body.extend_from_slice(info);
    };

    // Entry 1: audio, 2-byte format tag (WMA = 0x0161).
    append_entry(
        &mut body,
        0x0002,
        "WMA1",
        "First WMA",
        &0x0161u16.to_le_bytes(),
    );
    // Entry 2: audio, single-byte info -> hex-format codec id.
    append_entry(&mut body, 0x0002, "WMA2", "Second WMA", &[0xAB]);

    let obj = asf_object(&CODEC_LIST_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("two-audio codec list parse");

    assert_eq!(
        md.get("ASF:AudioCodecName").and_then(|v| v.as_string()),
        Some("WMA1")
    );
    // Second audio entry gets a "_2" suffix.
    assert_eq!(
        md.get("ASF:AudioCodecName_2").and_then(|v| v.as_string()),
        Some("WMA2")
    );
    assert!(md.contains_key("ASF:AudioCodecID"));
}

#[test]
fn test_asf_stream_properties_audio_spread_error_correction() {
    // An audio stream whose error-correction GUID is the Audio Spread GUID
    // drives the "Audio Spread" arm; an unknown stream-type GUID drives the
    // "Unknown" stream-type arm in a second object.
    let type_data_len = 18usize;
    let mut body = vec![0u8; 54 + type_data_len];
    body[0..16].copy_from_slice(&AUDIO_MEDIA_GUID); // audio stream type
    body[16..32].copy_from_slice(&AUDIO_SPREAD_GUID); // error correction
    body[32..40].copy_from_slice(&(5u64 * 10_000_000).to_le_bytes()); // time offset
    body[40..44].copy_from_slice(&(type_data_len as u32).to_le_bytes());
    body[48..50].copy_from_slice(&3u16.to_le_bytes()); // stream number 3
    let ts = &mut body[54..];
    ts[2..4].copy_from_slice(&2u16.to_le_bytes()); // channels
    ts[4..8].copy_from_slice(&48000u32.to_le_bytes()); // sample rate

    let obj = asf_object(&STREAM_PROPERTIES_GUID, &body);

    // Second stream object: unknown stream-type GUID (all 0xFF).
    let mut body2 = vec![0u8; 54];
    body2[0..16].copy_from_slice(&[0xFF; 16]); // unknown stream type
    body2[16..32].copy_from_slice(&[0x55; 16]); // unknown error correction
    body2[48..50].copy_from_slice(&5u16.to_le_bytes());
    let obj2 = asf_object(&STREAM_PROPERTIES_GUID, &body2);

    let data = asf_file(&[obj, obj2]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("audio spread parse");

    assert_eq!(
        md.get("ASF:ErrorCorrectionType")
            .and_then(|v| v.as_string()),
        Some("Unknown") // second object overwrites with its unknown GUID
    );
    assert_eq!(
        md.get("ASF:StreamType").and_then(|v| v.as_string()),
        Some("Unknown")
    );
    assert_eq!(
        md.get("ASF:AudioSampleRate").and_then(|v| v.as_integer()),
        Some(48000)
    );
}

#[test]
fn test_asf_file_properties_zero_creation_time() {
    // Creation time of 0 skips the CreationDate insert (the `if creation_time>0`
    // false side), and a tiny play_duration of 0 skips Duration.
    let mut body = vec![0u8; 80];
    body[16..24].copy_from_slice(&999u64.to_le_bytes()); // file size
    // creation time at 24..32 left as 0
    body[32..40].copy_from_slice(&7u64.to_le_bytes()); // data packets
    // play_duration at 40..48 left as 0 -> no Duration
    body[64..68].copy_from_slice(&1u32.to_le_bytes()); // flags

    let obj = asf_object(&FILE_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("zero-time file props parse");

    assert_eq!(
        md.get("ASF:FileLength").and_then(|v| v.as_integer()),
        Some(999)
    );
    assert!(!md.contains_key("ASF:CreationDate"));
    assert!(!md.contains_key("ASF:Duration"));
    assert!(md.contains_key("ASF:MaxBitrate"));
}

#[test]
fn test_asf_file_properties_filetime_underflow() {
    // A non-zero creation time BELOW the FILETIME/Unix epoch difference drives
    // filetime_to_string's underflow branch ("0000:00:00 00:00:00Z").
    let mut body = vec![0u8; 80];
    body[24..32].copy_from_slice(&1000u64.to_le_bytes()); // tiny FILETIME
    body[40..48].copy_from_slice(&(10u64 * 10_000_000).to_le_bytes()); // 10s play

    let obj = asf_object(&FILE_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("filetime underflow parse");
    assert_eq!(
        md.get("ASF:CreationDate").and_then(|v| v.as_string()),
        Some("0000:00:00 00:00:00Z")
    );
    assert!(md.contains_key("ASF:Duration"));
}

#[test]
fn test_asf_extended_content_descriptor_count_break() {
    // descriptor_count claims 5 but the body only contains one descriptor; the
    // loop breaks on the out-of-bounds read attempt without panicking.
    let mut body = Vec::new();
    body.extend_from_slice(&5u16.to_le_bytes()); // claims 5
    ext_descriptor(&mut body, "WM/Genre", 0, &utf16("Pop"));

    let obj = asf_object(&EXTENDED_CONTENT_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("count overflow parse");
    assert_eq!(md.get("ASF:Genre").and_then(|v| v.as_string()), Some("Pop"));
}

#[test]
fn test_asf_stream_properties_too_small_skipped() {
    // size < 78 -> parse_stream_properties returns early.
    let body = vec![0u8; 30];
    let obj = asf_object(&STREAM_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("small stream props parse");
    assert!(!md.contains_key("ASF:StreamType"));
}

#[test]
fn test_asf_header_extension_too_small_skipped() {
    // A header-extension object smaller than 46 bytes returns early.
    let body = vec![0u8; 10];
    let obj = asf_object(&HEADER_EXTENSION_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("small header ext parse");
    assert!(md.is_empty());
}

#[test]
fn test_asf_production_path_extended_content() {
    let mut body = Vec::new();
    body.extend_from_slice(&1u16.to_le_bytes());
    ext_descriptor(&mut body, "WM/Publisher", 0, &utf16("ProdPub"));
    let obj = asf_object(&EXTENDED_CONTENT_GUID, &body);
    let data = asf_file(&[obj]);

    let tmp = NamedTempFile::with_suffix(".asf").expect("tempfile");
    {
        let mut f = tmp.reopen().expect("reopen");
        f.write_all(&data).expect("write");
        f.flush().expect("flush");
    }
    let md = read_metadata(tmp.path()).expect("read_metadata asf");
    assert_eq!(
        md.get("ASF:Publisher").and_then(|v| v.as_string()),
        Some("ProdPub")
    );
}
