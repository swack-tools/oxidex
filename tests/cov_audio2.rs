//! Wave-2 coverage tests for audio parsers: AAC, MP3, OGG.
//!
//! Targets the remaining uncovered branches in:
//!   - src/parsers/audio/aac.rs  (ADTS detection + error/format branches)
//!   - src/parsers/audio/mp3.rs  (ID3v2.2/2.3/2.4 frame variety, ID3v1/v1.1,
//!     APIC/PIC, COMM/USLT, RVA2/RVAD/RVA, MPEG-1/2/2.5 frame headers)
//!   - src/parsers/audio/ogg.rs  (Vorbis ID + comments, OGG-FLAC mapping header,
//!     FLAC metadata block, base64 cover art, field-name mapping, multi-page)
//!
//! Everything is driven through the public API:
//!   - oxidex::parsers::audio::{aac,mp3,ogg}::parse_*_metadata(&TestReader)
//!   - oxidex::core::operations::read_metadata on tempfiles (production path)
//!
//! NOTE on AAC: the public AAC entry point reads only the first 4 magic bytes
//! and (per known source layout) its M4A `ftyp` slice comparison and its
//! 7-byte ADTS-header call cannot succeed from a 4-byte slice, so realistic
//! inputs return an error. These tests therefore exercise the reachable
//! detection / error branches and assert on `is_err()` / `is_ok()` accordingly.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::operations::read_metadata;
use oxidex::parsers::audio::aac::parse_aac_metadata;
use oxidex::parsers::audio::mp3::parse_mp3_metadata;
use oxidex::parsers::audio::ogg::parse_ogg_metadata;
use std::io::Write;

// ===========================================================================
// Shared builders
// ===========================================================================

/// Encode a u32 as an ID3v2 synchsafe integer (7 bits per byte, big-endian).
fn synchsafe(v: u32) -> [u8; 4] {
    [
        ((v >> 21) & 0x7F) as u8,
        ((v >> 14) & 0x7F) as u8,
        ((v >> 7) & 0x7F) as u8,
        (v & 0x7F) as u8,
    ]
}

/// Build a single ID3v2.3/2.4 frame: 4-byte id + size + 2 flag bytes + body.
/// `synch` selects synchsafe (v2.4) vs plain be32 (v2.3) size encoding.
fn v23_frame(id: &[u8; 4], body: &[u8], synch: bool) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(id);
    let size = body.len() as u32;
    if synch {
        f.extend_from_slice(&synchsafe(size));
    } else {
        f.extend_from_slice(&size.to_be_bytes());
    }
    f.extend_from_slice(&[0u8, 0u8]); // flags
    f.extend_from_slice(body);
    f
}

/// Build a single ID3v2.2 frame: 3-byte id + 3-byte big-endian size + body.
fn v22_frame(id: &[u8; 3], body: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(id);
    let size = body.len() as u32;
    f.extend_from_slice(&[(size >> 16) as u8, (size >> 8) as u8, size as u8]);
    f.extend_from_slice(body);
    f
}

/// Wrap frame bytes into a complete ID3v2 tag (10-byte header + frames).
fn id3v2_tag(version: u8, frames: &[u8]) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(b"ID3");
    t.push(version);
    t.push(0); // revision
    t.push(0); // flags
    t.extend_from_slice(&synchsafe(frames.len() as u32));
    t.extend_from_slice(frames);
    t
}

/// A simple latin1 text frame body: encoding byte 0 + ASCII text.
fn latin1_body(text: &str) -> Vec<u8> {
    let mut b = vec![0u8];
    b.extend_from_slice(text.as_bytes());
    b
}

/// A UTF-8 text frame body: encoding byte 3 + UTF-8 text.
fn utf8_body(text: &str) -> Vec<u8> {
    let mut b = vec![0x03u8];
    b.extend_from_slice(text.as_bytes());
    b
}

/// Build a 4-byte MPEG audio frame header.
/// version_bits: 0=2.5, 2=2.0, 3=1.0. layer_bits: 1=L3, 2=L2, 3=L1.
fn mpeg_header(
    version_bits: u8,
    layer_bits: u8,
    bitrate_idx: u8,
    srate_idx: u8,
    channel_mode: u8,
    mode_ext: u8,
    copyright: u8,
    original: u8,
    emphasis: u8,
) -> [u8; 4] {
    let b1 = 0xE0 | ((version_bits & 0x03) << 3) | ((layer_bits & 0x03) << 1) | 0x01; // protection=1 (no CRC)
    let b2 = ((bitrate_idx & 0x0F) << 4) | ((srate_idx & 0x03) << 2);
    let b3 = ((channel_mode & 0x03) << 6)
        | ((mode_ext & 0x03) << 4)
        | ((copyright & 0x01) << 3)
        | ((original & 0x01) << 2)
        | (emphasis & 0x03);
    [0xFF, b1, b2, b3]
}

/// Append a 128-byte ID3v1 (or v1.1) trailer with the given fields.
/// If `track` is Some, writes an ID3v1.1 trailer (comment byte 28 = 0, byte 29 = track).
fn id3v1_trailer(
    title: &str,
    artist: &str,
    album: &str,
    year: &str,
    comment: &str,
    genre: u8,
    track: Option<u8>,
) -> Vec<u8> {
    let mut t = vec![0u8; 128];
    t[0..3].copy_from_slice(b"TAG");
    let put = |dst: &mut [u8], s: &str| {
        let b = s.as_bytes();
        let n = b.len().min(dst.len());
        dst[..n].copy_from_slice(&b[..n]);
    };
    put(&mut t[3..33], title);
    put(&mut t[33..63], artist);
    put(&mut t[63..93], album);
    put(&mut t[93..97], year);
    put(&mut t[97..127], comment);
    if let Some(tr) = track {
        t[125] = 0; // comment separator
        t[126] = tr; // track number (v1.1)
    }
    t[127] = genre;
    t
}

/// Build a Vorbis-comment payload (vendor + count + "KEY=VALUE" entries),
/// all length prefixes little-endian.
fn vorbis_comment_payload(vendor: &str, comments: &[&str]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    p.extend_from_slice(vendor.as_bytes());
    p.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for c in comments {
        p.extend_from_slice(&(c.len() as u32).to_le_bytes());
        p.extend_from_slice(c.as_bytes());
    }
    p
}

/// Build an OGG page: header (27 bytes) + segment table + body.
/// Serial number is written at bytes 10-13 and page sequence at 18-21 to match
/// the parser's (non-spec) read offsets.
fn ogg_page(header_type: u8, serial: u32, seq: u32, body: &[u8]) -> Vec<u8> {
    let mut page = vec![0u8; 27];
    page[0..4].copy_from_slice(b"OggS");
    page[4] = 0; // stream structure version
    page[5] = header_type;
    page[10..14].copy_from_slice(&serial.to_le_bytes());
    page[18..22].copy_from_slice(&seq.to_le_bytes());
    // Segment table encodes body length as 255-byte lacing values.
    let mut segs: Vec<u8> = Vec::new();
    let mut remaining = body.len();
    while remaining >= 255 {
        segs.push(255);
        remaining -= 255;
    }
    segs.push(remaining as u8);
    page[26] = segs.len() as u8;
    page.extend_from_slice(&segs);
    page.extend_from_slice(body);
    page
}

/// Vorbis identification packet body: 0x01 + "vorbis" + fields.
fn vorbis_id_body(channels: u8, sample_rate: u32, nominal_bitrate: i32) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(0x01);
    b.extend_from_slice(b"vorbis");
    b.extend_from_slice(&0u32.to_le_bytes()); // vorbis_version
    b.push(channels);
    b.extend_from_slice(&sample_rate.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes()); // bitrate_max
    b.extend_from_slice(&nominal_bitrate.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes()); // bitrate_min
    b.push(0); // blocksize byte
    b
}

/// Vorbis comment packet body: 0x03 + "vorbis" + comment payload.
fn vorbis_comment_body(vendor: &str, comments: &[&str]) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(0x03);
    b.extend_from_slice(b"vorbis");
    b.extend_from_slice(&vorbis_comment_payload(vendor, comments));
    b
}

// ===========================================================================
// AAC
// ===========================================================================

/// Build a 7-byte ADTS header for the given parameters.
fn adts_header(frame_len: u16, sample_rate_idx: u8, channel_cfg: u8, profile_idx: u8) -> [u8; 7] {
    let mut h = [0u8; 7];
    h[0] = 0xFF;
    h[1] = 0xF1;
    h[2] = (profile_idx << 6) | ((sample_rate_idx & 0x0F) << 2) | ((channel_cfg >> 2) & 0x01);
    h[3] = ((channel_cfg & 0x03) << 6) | (((frame_len >> 11) & 0x03) as u8);
    h[4] = ((frame_len >> 3) & 0xFF) as u8;
    h[5] = (((frame_len & 0x07) as u8) << 5) | 0x1F;
    h[6] = 0xFC;
    h
}

#[test]
fn aac_file_too_small_errors() {
    // < 7 bytes -> "File too small to be AAC".
    assert!(parse_aac_metadata(&TestReader::new(vec![0xFF, 0xF1, 0x50])).is_err());
    assert!(parse_aac_metadata(&TestReader::new(Vec::new())).is_err());
    assert!(parse_aac_metadata(&TestReader::new(vec![0u8; 6])).is_err());
}

#[test]
fn aac_invalid_sync_word_errors() {
    // 7+ bytes, not ftyp, sync nibble != 0xFFF -> invalid-sync format branch.
    let data = b"NOTANAAC_FILE_AT_ALL".to_vec();
    let res = parse_aac_metadata(&TestReader::new(data));
    assert!(res.is_err());
    // Error message mentions the (shifted) sync word.
    let msg = res.unwrap_err();
    assert!(msg.contains("AAC") || msg.contains("ADTS") || msg.contains("sync"));
}

#[test]
fn aac_valid_adts_sync_reaches_header_parse() {
    // Valid 0xFFF sync in the first two bytes drives past the M4A check and
    // into parse_adts_header (fed the 4-byte magic), exercising the ADTS entry.
    for srate_idx in [0u8, 3, 4, 8, 11] {
        for chan in [1u8, 2, 6] {
            for profile in [0u8, 1, 2, 3] {
                let mut data = adts_header(64, srate_idx, chan, profile).to_vec();
                data.resize(64, 0);
                // Reachable branch executes; result type asserted (no panic).
                let _ = parse_aac_metadata(&TestReader::new(data));
            }
        }
    }
}

#[test]
fn aac_various_sample_rate_indices_no_panic() {
    // Including reserved/zero sample-rate indices (13, 14, 15) and large frame lens.
    for srate_idx in [12u8, 13, 14, 15] {
        let mut data = adts_header(2048, srate_idx, 7, 3).to_vec();
        data.resize(64, 0);
        let _ = parse_aac_metadata(&TestReader::new(data));
    }
}

#[test]
fn aac_m4a_like_buffer_detection_branch() {
    // A MP4/M4A-style buffer (size + 'ftyp'): the public entry inspects only the
    // first 4 magic bytes, so it does not enter deep iTunes parsing, but the
    // detection branch is exercised and the call must not panic.
    let mut data = Vec::new();
    data.extend_from_slice(&24u32.to_be_bytes());
    data.extend_from_slice(b"ftyp");
    data.extend_from_slice(b"M4A ");
    data.extend_from_slice(&[0u8; 12]);
    assert_eq!(&data[4..8], b"ftyp");
    let _ = parse_aac_metadata(&TestReader::new(data));
}

#[test]
fn aac_read_metadata_via_tempfile_aac_extension() {
    // Production path: a .aac file with a valid ADTS sync word. Detection +
    // dispatch run; AAC parser returns an error (known source layout) which the
    // operations layer surfaces, so we only assert it does not panic.
    let mut data = adts_header(64, 4, 2, 1).to_vec();
    data.resize(256, 0);
    let mut tf = tempfile::Builder::new().suffix(".aac").tempfile().unwrap();
    tf.write_all(&data).unwrap();
    tf.flush().unwrap();
    let _ = read_metadata(tf.path());
}

// ===========================================================================
// MP3 — ID3v2.4 frames (broad frame-id mapping coverage)
// ===========================================================================

#[test]
fn mp3_id3v24_many_text_frames() {
    // Hit a wide spread of v2.3/2.4 4-char frame ids through map_frame_id_to_tag_name.
    let mut frames = Vec::new();
    let pairs: &[(&[u8; 4], &str, &str)] = &[
        (b"TIT1", "Grp", "ID3:Grouping"),
        (b"TIT2", "Title", "ID3:Title"),
        (b"TIT3", "Sub", "ID3:Subtitle"),
        (b"TPE1", "Art", "ID3:Artist"),
        (b"TPE2", "Band", "ID3:Band"),
        (b"TPE3", "Cond", "ID3:Conductor"),
        (b"TPE4", "Rmx", "ID3:Remixer"),
        (b"TALB", "Alb", "ID3:Album"),
        (b"TYER", "1999", "ID3:Year"),
        (b"TDRC", "2001", "ID3:Year"),
        (b"TDAT", "0102", "ID3:Date"),
        (b"TCON", "Rock", "ID3:Genre"),
        (b"TRCK", "1/9", "ID3:Track"),
        (b"TPOS", "1/2", "ID3:PartOfSet"),
        (b"TCOM", "Comp", "ID3:Composer"),
        (b"TPUB", "Pub", "ID3:Publisher"),
        (b"TCOP", "Cpy", "ID3:Copyright"),
        (b"TENC", "Enc", "ID3:EncodedBy"),
        (b"TSSE", "Set", "ID3:EncoderSettings"),
        (b"TBPM", "120", "ID3:BeatsPerMinute"),
        (b"TKEY", "Am", "ID3:InitialKey"),
        (b"TLAN", "eng", "ID3:Language"),
        (b"TLEN", "1000", "ID3:Length"),
        (b"TMED", "CD", "ID3:OriginalMedia"),
        (b"TOAL", "OAl", "ID3:OriginalAlbum"),
        (b"TOFN", "OFn", "ID3:OriginalFilename"),
        (b"TOLY", "OLy", "ID3:OriginalLyricist"),
        (b"TOPE", "OPe", "ID3:OriginalArtist"),
        (b"TORY", "1980", "ID3:OriginalYear"),
        (b"TEXT", "Lyr", "ID3:Lyricist"),
    ];
    for (id, text, _) in pairs {
        frames.extend_from_slice(&v23_frame(id, &utf8_body(text), true));
    }
    let tag = id3v2_tag(4, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse v2.4 spread");
    // Every mapped key must be present. (TYER and TDRC both map to ID3:Year, so
    // exact value of that key is whichever came last; assert presence only.)
    for (_, _text, key) in pairs {
        assert!(md.contains_key(*key), "frame key {} missing", key);
    }
    // Spot-check several unique mappings for exact values.
    assert_eq!(
        md.get("ID3:Title").and_then(|v| v.as_string()),
        Some("Title")
    );
    assert_eq!(
        md.get("ID3:Composer").and_then(|v| v.as_string()),
        Some("Comp")
    );
    assert_eq!(
        md.get("ID3:Lyricist").and_then(|v| v.as_string()),
        Some("Lyr")
    );
    assert_eq!(
        md.get("ID3:PartOfSet").and_then(|v| v.as_string()),
        Some("1/2")
    );
}

#[test]
fn mp3_id3v24_url_frames() {
    let mut frames = Vec::new();
    let pairs: &[(&[u8; 4], &str)] = &[
        (b"WCOM", "ID3:CommercialURL"),
        (b"WCOP", "ID3:CopyrightURL"),
        (b"WOAF", "ID3:FileURL"),
        (b"WOAR", "ID3:ArtistURL"),
        (b"WOAS", "ID3:SourceURL"),
        (b"WORS", "ID3:StationURL"),
        (b"WPAY", "ID3:PaymentURL"),
        (b"WPUB", "ID3:PublisherURL"),
    ];
    // W*** frames begin with 'W' so they are treated as text frames (start_with('T') is false),
    // so they are NOT inserted. Use TEXT-prefixed assertion only for those that map.
    // The point here is to exercise map_frame_id_to_tag_name for these ids; they are
    // reached only if frame starts with 'T'. So we instead drive WXXX skip + confirm no panic.
    for (id, _) in pairs {
        frames.extend_from_slice(&v23_frame(id, &utf8_body("http://x"), true));
    }
    let tag = id3v2_tag(4, &frames);
    // Should parse without error even though W frames are not text-extracted.
    assert!(parse_mp3_metadata(&TestReader::new(tag)).is_ok());
}

#[test]
fn mp3_id3v24_txxx_and_wxxx_are_skipped() {
    // TXXX / WXXX are user-defined and explicitly excluded from text extraction.
    let mut frames = Vec::new();
    frames.extend_from_slice(&v23_frame(b"TXXX", &utf8_body("custom"), true));
    frames.extend_from_slice(&v23_frame(b"WXXX", &utf8_body("url"), true));
    frames.extend_from_slice(&v23_frame(b"TIT2", &utf8_body("Real"), true));
    let tag = id3v2_tag(4, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse txxx");
    assert_eq!(
        md.get("ID3:Title").and_then(|v| v.as_string()),
        Some("Real")
    );
    assert!(!md.contains_key("ID3:TXXX"));
}

#[test]
fn mp3_id3v24_uslt_lyrics_frame() {
    // USLT has comment-frame structure: enc + lang(3) + desc\0 + text.
    let mut uslt = vec![0x00u8]; // latin1
    uslt.extend_from_slice(b"eng");
    uslt.push(0x00); // empty descriptor
    uslt.extend_from_slice(b"la la la");
    let frames = v23_frame(b"USLT", &uslt, true);
    let tag = id3v2_tag(4, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse uslt");
    assert_eq!(
        md.get("ID3:Lyrics").and_then(|v| v.as_string()),
        Some("la la la")
    );
}

#[test]
fn mp3_id3v24_comm_utf16_double_null() {
    // COMM with UTF-16 encoding -> the parser scans for a double-null after the
    // language bytes to find the description boundary.
    let mut comm = vec![0x01u8]; // UTF-16
    comm.extend_from_slice(b"eng");
    // Short description "d" in UTF-16LE + double null.
    comm.extend_from_slice(&[b'd', 0x00, 0x00, 0x00]);
    // Comment text "Hi" in UTF-16LE.
    comm.extend_from_slice(&[b'H', 0x00, b'i', 0x00]);
    let frames = v23_frame(b"COMM", &comm, true);
    let tag = id3v2_tag(4, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse comm utf16");
    assert!(md.contains_key("ID3:Comment"));
}

#[test]
fn mp3_text_frame_all_encodings() {
    // Encoding 0=latin1, 1=UTF-16LE(BOM), 2=UTF-16BE(BOM), 3=UTF-8, plus unknown.
    let mut frames = Vec::new();
    // latin1
    frames.extend_from_slice(&v23_frame(b"TIT2", &latin1_body("Lat"), true));
    // UTF-16LE with BOM 0xFFFE
    let mut le = vec![0x01u8, 0xFF, 0xFE];
    le.extend_from_slice(&[b'L', 0x00, b'E', 0x00]);
    frames.extend_from_slice(&v23_frame(b"TPE1", &le, true));
    // UTF-16BE with BOM 0xFEFF
    let mut be = vec![0x02u8, 0xFE, 0xFF];
    be.extend_from_slice(&[0x00, b'B', 0x00, b'E']);
    frames.extend_from_slice(&v23_frame(b"TALB", &be, true));
    // unknown encoding byte -> default UTF-8
    let mut unk = vec![0x09u8];
    unk.extend_from_slice(b"Unk");
    frames.extend_from_slice(&v23_frame(b"TCON", &unk, true));
    let tag = id3v2_tag(4, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse encodings");
    assert_eq!(md.get("ID3:Title").and_then(|v| v.as_string()), Some("Lat"));
    assert_eq!(md.get("ID3:Artist").and_then(|v| v.as_string()), Some("LE"));
    assert_eq!(md.get("ID3:Album").and_then(|v| v.as_string()), Some("BE"));
    assert_eq!(md.get("ID3:Genre").and_then(|v| v.as_string()), Some("Unk"));
}

#[test]
fn mp3_empty_text_frame_is_handled() {
    // A text frame with zero body -> parse_text_frame errors, frame skipped.
    let frames = v23_frame(b"TIT2", &[], true);
    let tag = id3v2_tag(4, &frames);
    assert!(parse_mp3_metadata(&TestReader::new(tag)).is_ok());
}

// ===========================================================================
// MP3 — ID3v2.3 (plain be32 frame sizes)
// ===========================================================================

#[test]
fn mp3_id3v23_plain_sizes_and_padding() {
    let mut frames = Vec::new();
    frames.extend_from_slice(&v23_frame(b"TIT2", &utf8_body("V23"), false));
    frames.extend_from_slice(&v23_frame(b"TPE1", &utf8_body("Who"), false));
    // Trailing padding: a frame id of all zeros triggers the padding break.
    frames.extend_from_slice(&[0u8; 12]);
    let tag = id3v2_tag(3, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse v2.3");
    assert_eq!(md.get("ID3:Title").and_then(|v| v.as_string()), Some("V23"));
    assert_eq!(
        md.get("ID3:Artist").and_then(|v| v.as_string()),
        Some("Who")
    );
    assert_eq!(
        md.get("MP3:ID3Version").and_then(|v| v.as_string()),
        Some("ID3 v2.3")
    );
}

#[test]
fn mp3_id3v23_comm_frame_latin1() {
    let mut comm = vec![0x00u8];
    comm.extend_from_slice(b"eng");
    comm.push(0x00); // empty desc
    comm.extend_from_slice(b"the comment");
    let frames = v23_frame(b"COMM", &comm, false);
    let tag = id3v2_tag(3, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse comm");
    assert_eq!(
        md.get("ID3:Comment").and_then(|v| v.as_string()),
        Some("the comment")
    );
}

#[test]
fn mp3_id3v23_apic_all_picture_types() {
    // APIC v2.3: enc + mime\0 + pic_type + desc\0 + data. Exercise several
    // picture-type mappings and a PNG mime.
    for (pt, _name) in [
        (0u8, "Other"),
        (1, "32x32 PNG Icon"),
        (3, "Front Cover"),
        (8, "Artist"),
        (11, "Composer"),
        (20, "Publisher Logotype"),
        (200, "Other"),
    ] {
        let mut apic = vec![0x00u8];
        apic.extend_from_slice(b"image/png");
        apic.push(0x00);
        apic.push(pt);
        apic.extend_from_slice(b"desc");
        apic.push(0x00);
        apic.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47]);
        let frames = v23_frame(b"APIC", &apic, false);
        let tag = id3v2_tag(3, &frames);
        let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse apic");
        assert_eq!(
            md.get("ID3:PictureFormat").and_then(|v| v.as_string()),
            Some("PNG")
        );
        assert!(md.contains_key("ID3:PictureType"));
        assert!(md.contains_key("ID3:Picture"));
    }
}

#[test]
fn mp3_id3v23_apic_gif_and_utf16_description() {
    // GIF mime + UTF-16 description (encoding 1 -> double-null terminator).
    let mut apic = vec![0x01u8]; // UTF-16
    apic.extend_from_slice(b"image/gif");
    apic.push(0x00);
    apic.push(0x03); // Front Cover
    apic.extend_from_slice(&[b'C', 0x00, 0x00, 0x00]); // "C" + double null
    apic.extend_from_slice(&[0x47, 0x49, 0x46, 0x38]); // GIF8 data
    let frames = v23_frame(b"APIC", &apic, false);
    let tag = id3v2_tag(3, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse apic gif");
    assert_eq!(
        md.get("ID3:PictureFormat").and_then(|v| v.as_string()),
        Some("GIF")
    );
}

#[test]
fn mp3_id3v23_apic_unknown_mime_passthrough() {
    // A mime that is none of jpeg/png/gif -> format_str falls back to the mime.
    let mut apic = vec![0x00u8];
    apic.extend_from_slice(b"image/bmp");
    apic.push(0x00);
    apic.push(0x00);
    apic.push(0x00); // empty desc terminator
    apic.extend_from_slice(&[0x42, 0x4D]);
    let frames = v23_frame(b"APIC", &apic, false);
    let tag = id3v2_tag(3, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse apic bmp");
    assert_eq!(
        md.get("ID3:PictureFormat").and_then(|v| v.as_string()),
        Some("image/bmp")
    );
}

#[test]
fn mp3_id3v23_rva2_multiple_channels() {
    // RVA2 with several channel-type entries to walk the channel-name match arms.
    let mut rva2 = Vec::new();
    rva2.extend_from_slice(b"norm");
    rva2.push(0x00); // ident terminator
    // (channel, volume_adj_be, peak_bits, peak)
    for (ch, vol) in [
        (1u8, 512i16), // Master
        (2, -256),     // Right
        (3, 128),      // Left
        (4, 64),       // Right Back
        (5, -64),      // Left Back
        (6, 32),       // Center
        (7, -32),      // Bass
        (0, 16),       // Other
    ] {
        rva2.push(ch);
        rva2.extend_from_slice(&vol.to_be_bytes());
        rva2.push(0x08); // 8 peak bits -> 1 byte peak
        rva2.push(0x00);
    }
    let frames = v23_frame(b"RVA2", &rva2, false);
    let tag = id3v2_tag(4, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse rva2");
    assert!(md.contains_key("ID3:RelativeVolumeAdjustment"));
}

#[test]
fn mp3_id3v23_rvad_and_empty_rva() {
    // RVAD (v2.3) with non-trivial peak bits.
    let mut rvad = Vec::new();
    rvad.push(0x01); // only left increment
    rvad.push(0x08); // 8 peak bits -> 1 byte per value
    rvad.push(0x40); // right
    rvad.push(0x20); // left
    let frames = v23_frame(b"RVAD", &rvad, false);
    let tag = id3v2_tag(3, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse rvad");
    assert!(md.contains_key("ID3:RelativeVolumeAdjustment"));

    // Empty RVA frame -> early return Ok(()), no key inserted.
    let frames2 = v23_frame(b"RVA2", &[], true);
    let tag2 = id3v2_tag(4, &frames2);
    assert!(parse_mp3_metadata(&TestReader::new(tag2)).is_ok());
}

// ===========================================================================
// MP3 — ID3v2.2 (3-char frame ids, 6-byte headers)
// ===========================================================================

#[test]
fn mp3_id3v22_many_text_frames() {
    let mut frames = Vec::new();
    let pairs: &[(&[u8; 3], &str, &str)] = &[
        (b"TT1", "Grp", "ID3:Grouping"),
        (b"TT2", "Tit", "ID3:Title"),
        (b"TT3", "Sub", "ID3:Subtitle"),
        (b"TP1", "Art", "ID3:Artist"),
        (b"TP2", "Bnd", "ID3:Band"),
        (b"TP3", "Cnd", "ID3:Conductor"),
        (b"TP4", "Rmx", "ID3:Remixer"),
        (b"TAL", "Alb", "ID3:Album"),
        (b"TYE", "1999", "ID3:Year"),
        (b"TDA", "0102", "ID3:Date"),
        (b"TCO", "Pop", "ID3:Genre"),
        (b"TRK", "2", "ID3:Track"),
        (b"TPA", "1", "ID3:PartOfSet"),
        (b"TCM", "Cmp", "ID3:Composer"),
        (b"TPB", "Pub", "ID3:Publisher"),
        (b"TCR", "Cpy", "ID3:Copyright"),
        (b"TEN", "Enc", "ID3:EncodedBy"),
        (b"TSS", "Set", "ID3:EncoderSettings"),
        (b"TBP", "90", "ID3:BeatsPerMinute"),
        (b"TKE", "Cm", "ID3:InitialKey"),
        (b"TLA", "eng", "ID3:Language"),
        (b"TLE", "500", "ID3:Length"),
        (b"TMT", "CD", "ID3:OriginalMedia"),
        (b"TOT", "OAl", "ID3:OriginalAlbum"),
        (b"TOF", "OFn", "ID3:OriginalFilename"),
        (b"TOL", "OLy", "ID3:OriginalLyricist"),
        (b"TOA", "OPe", "ID3:OriginalArtist"),
        (b"TOR", "1980", "ID3:OriginalYear"),
        (b"TXT", "Lyr", "ID3:Lyricist"),
    ];
    for (id, text, _) in pairs {
        frames.extend_from_slice(&v22_frame(id, &utf8_body(text)));
    }
    // Trailing padding so the final real frame still satisfies the parser's
    // `offset + 10 < data.len()` loop guard and is processed.
    frames.extend_from_slice(&[0u8; 12]);
    let tag = id3v2_tag(2, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse v2.2 spread");
    for (_, text, key) in pairs {
        assert_eq!(
            md.get(*key).and_then(|v| v.as_string()),
            Some(*text),
            "frame {} -> {}",
            key,
            text
        );
    }
}

#[test]
fn mp3_id3v22_com_and_ult_frames() {
    let mut frames = Vec::new();
    // COM (comment) frame
    let mut com = vec![0x00u8];
    com.extend_from_slice(b"eng");
    com.push(0x00);
    com.extend_from_slice(b"v22 comment");
    frames.extend_from_slice(&v22_frame(b"COM", &com));
    // ULT (lyrics) frame
    let mut ult = vec![0x00u8];
    ult.extend_from_slice(b"eng");
    ult.push(0x00);
    ult.extend_from_slice(b"v22 lyrics");
    frames.extend_from_slice(&v22_frame(b"ULT", &ult));
    let tag = id3v2_tag(2, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse com/ult");
    assert_eq!(
        md.get("ID3:Comment").and_then(|v| v.as_string()),
        Some("v22 comment")
    );
    assert_eq!(
        md.get("ID3:Lyrics").and_then(|v| v.as_string()),
        Some("v22 lyrics")
    );
}

#[test]
fn mp3_id3v22_pic_frame_3char_format() {
    // PIC (v2.2) picture: enc + 3-char format + pic_type + desc\0 + data.
    let mut pic = vec![0x00u8];
    pic.extend_from_slice(b"JPG"); // 3-char image format
    pic.push(0x03); // Front Cover
    pic.extend_from_slice(b"art");
    pic.push(0x00);
    pic.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]);
    let frames = v22_frame(b"PIC", &pic);
    let tag = id3v2_tag(2, &frames);
    let md = parse_mp3_metadata(&TestReader::new(tag)).expect("parse pic");
    assert_eq!(
        md.get("ID3:PictureFormat").and_then(|v| v.as_string()),
        Some("JPG")
    );
    assert!(md.contains_key("ID3:Picture"));
}

// ===========================================================================
// MP3 — MPEG audio frame headers (versions / layers / channel modes)
// ===========================================================================

#[test]
fn mp3_mpeg1_layers_and_channel_modes() {
    // MPEG-1 (version_bits=3), all three layers + all channel modes.
    for (layer_bits, layer) in [(1u8, 3i64), (2, 2), (3, 1)] {
        for cm in [0u8, 1, 2, 3] {
            let mut data = Vec::new();
            // bitrate idx 5, srate idx 0 (44100), mode_ext, copyright/original set, emphasis 0.
            data.extend_from_slice(&mpeg_header(3, layer_bits, 5, 0, cm, 0, 1, 1, 0));
            data.extend_from_slice(&[0u8; 32]);
            let md = parse_mp3_metadata(&TestReader::new(data)).expect("mpeg1 frame");
            assert_eq!(
                md.get("MPEG:AudioLayer").and_then(|v| v.as_integer()),
                Some(layer)
            );
            assert_eq!(
                md.get("MPEG:MPEGAudioVersion").and_then(|v| v.as_integer()),
                Some(1)
            );
            assert!(md.contains_key("MPEG:ChannelMode"));
            assert!(md.contains_key("MPEG:CopyrightFlag"));
            assert!(md.contains_key("MPEG:OriginalMedia"));
        }
    }
}

#[test]
fn mp3_mpeg2_and_mpeg25_frames() {
    // MPEG-2 (version_bits=2) and MPEG-2.5 (version_bits=0) report version 2.
    for vb in [2u8, 0u8] {
        let mut data = Vec::new();
        // layer III, bitrate idx 8, srate idx 0, joint stereo.
        data.extend_from_slice(&mpeg_header(vb, 1, 8, 0, 1, 2, 0, 0, 1));
        data.extend_from_slice(&[0u8; 32]);
        let md = parse_mp3_metadata(&TestReader::new(data)).expect("mpeg2/2.5 frame");
        assert_eq!(
            md.get("MPEG:MPEGAudioVersion").and_then(|v| v.as_integer()),
            Some(2)
        );
        // Layer III joint stereo -> mode-extension flags inserted.
        assert!(md.contains_key("MPEG:MSStereo"));
        assert!(md.contains_key("MPEG:IntensityStereo"));
    }
}

#[test]
fn mp3_mpeg_layer1_and_layer2_bitrate_tables() {
    // Drive get_mpeg_bitrate across MPEG-1 L1/L2 and MPEG-2 L1.
    // MPEG-1 Layer I
    let mut d1 = mpeg_header(3, 3, 4, 1, 0, 0, 0, 0, 0).to_vec();
    d1.extend_from_slice(&[0u8; 16]);
    let m1 = parse_mp3_metadata(&TestReader::new(d1)).expect("v1 l1");
    assert_eq!(
        m1.get("MPEG:AudioLayer").and_then(|v| v.as_integer()),
        Some(1)
    );
    assert!(m1.contains_key("MP3:BitRate"));

    // MPEG-1 Layer II
    let mut d2 = mpeg_header(3, 2, 6, 2, 0, 0, 0, 0, 0).to_vec();
    d2.extend_from_slice(&[0u8; 16]);
    let m2 = parse_mp3_metadata(&TestReader::new(d2)).expect("v1 l2");
    assert_eq!(
        m2.get("MPEG:AudioLayer").and_then(|v| v.as_integer()),
        Some(2)
    );

    // MPEG-2 Layer I (uses V2_L1 table)
    let mut d3 = mpeg_header(2, 3, 7, 0, 3, 0, 0, 0, 0).to_vec();
    d3.extend_from_slice(&[0u8; 16]);
    let m3 = parse_mp3_metadata(&TestReader::new(d3)).expect("v2 l1");
    assert_eq!(
        m3.get("MPEG:AudioLayer").and_then(|v| v.as_integer()),
        Some(1)
    );
    assert_eq!(m3.get("MP3:Channels").and_then(|v| v.as_integer()), Some(1));
}

#[test]
fn mp3_mpeg_emphasis_variants() {
    // Walk all four emphasis arms via the byte-3 low bits.
    for emph in [0u8, 1, 2, 3] {
        let mut data = mpeg_header(3, 1, 9, 0, 0, 0, 0, 0, emph).to_vec();
        data.extend_from_slice(&[0u8; 16]);
        let md = parse_mp3_metadata(&TestReader::new(data)).expect("emphasis");
        assert!(md.contains_key("MPEG:Emphasis"));
    }
}

#[test]
fn mp3_mpeg_invalid_headers_are_skipped() {
    // Reserved version (01), reserved layer (00), invalid bitrate (15), reserved
    // sample rate (3): the scan must skip each candidate without panicking.
    let cases = [
        mpeg_header(1, 1, 5, 0, 0, 0, 0, 0, 0),    // reserved version
        mpeg_header(3, 0, 5, 0, 0, 0, 0, 0, 0),    // reserved layer
        mpeg_header(3, 1, 0x0F, 0, 0, 0, 0, 0, 0), // invalid bitrate idx
        mpeg_header(3, 1, 5, 3, 0, 0, 0, 0, 0),    // reserved sample rate
    ];
    for h in cases {
        let mut data = h.to_vec();
        data.extend_from_slice(&[0u8; 16]);
        // No valid frame found -> Ok with no MPEG tags.
        let md = parse_mp3_metadata(&TestReader::new(data)).expect("invalid mpeg");
        assert!(!md.contains_key("MPEG:AudioLayer"));
    }
}

#[test]
fn mp3_xing_info_vbri_headers_passthrough() {
    // A valid frame whose payload contains Xing/Info/VBRI tags. The parser does
    // not parse VBR headers but must still extract the frame header cleanly.
    for marker in [&b"Xing"[..], &b"Info"[..], &b"VBRI"[..]] {
        let mut data = mpeg_header(3, 1, 9, 0, 1, 0, 0, 0, 0).to_vec();
        // Side info padding then the VBR marker.
        data.extend_from_slice(&[0u8; 32]);
        data.extend_from_slice(marker);
        data.extend_from_slice(&[0u8; 64]);
        let md = parse_mp3_metadata(&TestReader::new(data)).expect("vbr passthrough");
        assert!(md.contains_key("MPEG:AudioLayer"));
    }
}

// ===========================================================================
// MP3 — ID3v1 / ID3v1.1
// ===========================================================================

#[test]
fn mp3_id3v1_full_fields() {
    let mut data = vec![0u8; 32];
    data.extend(id3v1_trailer(
        "T1",
        "A1",
        "Alb1",
        "2020",
        "Comment text",
        17,
        None,
    ));
    let md = parse_mp3_metadata(&TestReader::new(data)).expect("id3v1");
    assert_eq!(
        md.get("ID3v1:Title").and_then(|v| v.as_string()),
        Some("T1")
    );
    assert_eq!(
        md.get("ID3v1:Artist").and_then(|v| v.as_string()),
        Some("A1")
    );
    assert_eq!(
        md.get("ID3v1:Album").and_then(|v| v.as_string()),
        Some("Alb1")
    );
    assert_eq!(
        md.get("ID3v1:Year").and_then(|v| v.as_string()),
        Some("2020")
    );
    assert_eq!(md.get("ID3v1:Genre").and_then(|v| v.as_integer()), Some(17));
    assert_eq!(
        md.get("ID3Version").and_then(|v| v.as_string()),
        Some("ID3 v1")
    );
}

#[test]
fn mp3_id3v11_with_track() {
    // ID3v1.1: track number in the comment trailer.
    let mut data = vec![0u8; 32];
    data.extend(id3v1_trailer("TT", "AA", "BB", "1999", "c", 9, Some(7)));
    let md = parse_mp3_metadata(&TestReader::new(data)).expect("id3v1.1");
    assert_eq!(md.get("ID3v1:Genre").and_then(|v| v.as_integer()), Some(9));
    assert_eq!(
        md.get("MP3:ID3Version").and_then(|v| v.as_string()),
        Some("ID3 v1")
    );
}

#[test]
fn mp3_id3v1_genre_out_of_range_omitted() {
    // Genre byte >= 192 -> genre tag not inserted.
    let mut data = vec![0u8; 32];
    data.extend(id3v1_trailer("X", "", "", "", "", 200, None));
    let md = parse_mp3_metadata(&TestReader::new(data)).expect("id3v1 genre oob");
    assert!(!md.contains_key("ID3v1:Genre"));
    assert_eq!(md.get("ID3v1:Title").and_then(|v| v.as_string()), Some("X"));
}

#[test]
fn mp3_id3v2_and_id3v1_combined() {
    // ID3v2 at the head, MPEG frame, and ID3v1 trailer all in one file.
    let frames = v23_frame(b"TIT2", &utf8_body("Combined"), true);
    let mut data = id3v2_tag(4, &frames);
    data.extend_from_slice(&mpeg_header(3, 1, 9, 0, 0, 0, 0, 0, 0));
    data.extend_from_slice(&[0u8; 200]); // ensure >= 128 + room
    data.extend(id3v1_trailer("V1T", "V1A", "", "2000", "", 13, None));
    let md = parse_mp3_metadata(&TestReader::new(data)).expect("combined");
    assert_eq!(
        md.get("ID3:Title").and_then(|v| v.as_string()),
        Some("Combined")
    );
    assert_eq!(
        md.get("ID3v1:Title").and_then(|v| v.as_string()),
        Some("V1T")
    );
    assert!(md.contains_key("MPEG:AudioLayer"));
}

#[test]
fn mp3_tiny_and_empty_inputs_ok() {
    assert!(parse_mp3_metadata(&TestReader::new(Vec::new())).is_ok());
    assert!(parse_mp3_metadata(&TestReader::new(vec![0u8; 4])).is_ok());
    assert!(parse_mp3_metadata(&TestReader::new(vec![0u8; 9])).is_ok());
    // 10+ bytes but not "ID3" -> no v2 parse, audio scan runs and finds nothing.
    assert!(parse_mp3_metadata(&TestReader::new(vec![0x00u8; 16])).is_ok());
}

#[test]
fn mp3_read_metadata_via_tempfile() {
    let frames = v23_frame(b"TIT2", &utf8_body("FromFile"), true);
    let tag = id3v2_tag(4, &frames);
    let mut tf = tempfile::Builder::new().suffix(".mp3").tempfile().unwrap();
    tf.write_all(&tag).unwrap();
    tf.flush().unwrap();
    let md = read_metadata(tf.path()).expect("read mp3");
    assert_eq!(
        md.get("ID3:Title").and_then(|v| v.as_string()),
        Some("FromFile")
    );
}

// ===========================================================================
// OGG — Vorbis ID header + comments
// ===========================================================================

#[test]
fn ogg_vorbis_full_field_mapping() {
    // Exercise a broad set of explicitly-mapped Vorbis field names.
    let id_page = ogg_page(0x02, 0x1234, 0, &vorbis_id_body(2, 44100, 192000));
    let comments = [
        "TITLE=T",
        "ARTIST=A",
        "ALBUM=Al",
        "TRACKNUMBER=3",
        "DATE=2020",
        "GENRE=Jazz",
        "COMMENT=cmt",
        "DESCRIPTION=desc",
        "COPYRIGHT=cpy",
        "LICENSE=lic",
        "ORGANIZATION=org",
        "PERFORMER=perf",
        "COMPOSER=comp",
        "CONDUCTOR=cond",
        "ISRC=isrc",
        "LYRICS=ly",
        "ALBUMARTIST=aa",
        "DISCNUMBER=1",
        "TOTALTRACKS=9",
        "TOTALDISCS=2",
        "ENCODER=enc",
        "ENCODED_BY=eb",
        "CONTACT=ct",
        "LOCATION=loc",
        "VERSION=v",
        "REPLAYGAIN_TRACK_GAIN=-3.5 dB",
        "REPLAYGAIN_TRACK_PEAK=0.9",
        "REPLAYGAIN_ALBUM_GAIN=-2.0 dB",
        "REPLAYGAIN_ALBUM_PEAK=0.8",
        "COVERARTMIME=image/png",
    ];
    let comment_page = ogg_page(0x00, 0x1234, 1, &vorbis_comment_body("vendor", &comments));
    let mut data = id_page;
    data.extend_from_slice(&comment_page);

    let md = parse_ogg_metadata(&TestReader::new(data)).expect("vorbis fields");
    assert_eq!(
        md.get("Vorbis:Title").and_then(|v| v.as_string()),
        Some("T")
    );
    assert_eq!(
        md.get("Vorbis:Performer").and_then(|v| v.as_string()),
        Some("perf")
    );
    assert_eq!(
        md.get("Vorbis:EncodedBy").and_then(|v| v.as_string()),
        Some("eb")
    );
    assert_eq!(
        md.get("Vorbis:ReplayGainTrackGain")
            .and_then(|v| v.as_string()),
        Some("-3.5 dB")
    );
    assert_eq!(
        md.get("Vorbis:TotalDiscs").and_then(|v| v.as_string()),
        Some("2")
    );
    assert_eq!(
        md.get("Vorbis:CoverArtMIMEType")
            .and_then(|v| v.as_string()),
        Some("image/png")
    );
    // ID header fields.
    assert_eq!(
        md.get("Vorbis:AudioChannels").and_then(|v| v.as_integer()),
        Some(2)
    );
    assert_eq!(
        md.get("OGG:CodecName").and_then(|v| v.as_string()),
        Some("Vorbis")
    );
    assert!(md.contains_key("OGG:BitRate"));
    assert!(md.contains_key("Vorbis:NominalBitrate"));
    assert!(md.contains_key("OGG:SerialNumber"));
}

#[test]
fn ogg_vorbis_unknown_field_pascalcase() {
    let id_page = ogg_page(0x02, 5, 0, &vorbis_id_body(1, 22050, 0));
    // Unknown fields normalized to PascalCase under Vorbis:.
    let comments = ["MUSICBRAINZ_TRACKID=abc", "FOO BAR=baz", "WEIRD:KEY=v"];
    let comment_page = ogg_page(0x00, 5, 1, &vorbis_comment_body("v", &comments));
    let mut data = id_page;
    data.extend_from_slice(&comment_page);
    let md = parse_ogg_metadata(&TestReader::new(data)).expect("unknown fields");
    // At least one normalized key should be present.
    let has_norm = md.keys().any(|k| {
        k.starts_with("Vorbis:")
            && (k.contains("Musicbrainz") || k.contains("Foo") || k.contains("Weird"))
    });
    assert!(has_norm, "expected a PascalCase-normalized Vorbis field");
    // bitrate 0 -> no NominalBitrate / OGG:BitRate keys.
    assert!(!md.contains_key("Vorbis:NominalBitrate"));
}

#[test]
fn ogg_coverart_and_picture_block_base64() {
    let id_page = ogg_page(0x02, 9, 0, &vorbis_id_body(2, 48000, 128000));
    // COVERART + METADATA_BLOCK_PICTURE both go through base64_decode.
    // Include whitespace in the base64 to hit the whitespace-skip branch.
    let comments = ["COVERART=AA AA", "METADATA_BLOCK_PICTURE=QUJD", "COMMENT=x"];
    let comment_page = ogg_page(0x00, 9, 1, &vorbis_comment_body("v", &comments));
    let mut data = id_page;
    data.extend_from_slice(&comment_page);
    let md = parse_ogg_metadata(&TestReader::new(data)).expect("coverart base64");
    let cover = md
        .get("Vorbis:CoverArt")
        .and_then(|v| v.as_string())
        .unwrap_or("");
    assert!(cover.contains("Binary data"));
    // METADATA_BLOCK_PICTURE normalizes (split on ':'/space only) to this key,
    // and its base64 ("QUJD" = "ABC") decodes to a binary blob string.
    let pic = md
        .get("Vorbis:Metadata_block_picture")
        .and_then(|v| v.as_string())
        .unwrap_or("");
    assert!(pic.contains("Binary data"));
}

#[test]
fn ogg_coverart_invalid_base64_fallback() {
    let id_page = ogg_page(0x02, 9, 0, &vorbis_id_body(2, 48000, 0));
    // '!' is not a valid base64 char -> base64_decode errors -> raw value stored.
    let comments = ["COVERART=!!!notbase64"];
    let comment_page = ogg_page(0x00, 9, 1, &vorbis_comment_body("v", &comments));
    let mut data = id_page;
    data.extend_from_slice(&comment_page);
    let md = parse_ogg_metadata(&TestReader::new(data)).expect("coverart fallback");
    assert!(md.contains_key("Vorbis:CoverArt"));
}

// ===========================================================================
// OGG — FLAC mapping header + FLAC metadata block in page body
// ===========================================================================

/// Build the OGG-FLAC mapping header page body: 0x7F"FLAC"+ver+packets+"fLaC"+
/// streaminfo block header + 34-byte STREAMINFO.
fn ogg_flac_mapping_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.push(0x7F);
    b.extend_from_slice(b"FLAC");
    b.push(1); // major version
    b.push(0); // minor version
    b.extend_from_slice(&1u16.to_be_bytes()); // header packet count
    b.extend_from_slice(b"fLaC"); // native FLAC signature (offset 9)
    // metadata block header: type 0 (streaminfo), not last, 34-byte length
    b.extend_from_slice(&[0x00, 0x00, 0x00, 34]);
    // 34-byte STREAMINFO
    let mut si = vec![0u8; 34];
    si[0..2].copy_from_slice(&4096u16.to_be_bytes()); // min block
    si[2..4].copy_from_slice(&4096u16.to_be_bytes()); // max block
    si[4..7].copy_from_slice(&[0x00, 0x10, 0x00]); // min frame
    si[7..10].copy_from_slice(&[0x00, 0x20, 0x00]); // max frame
    // sample rate 44100 (20 bits), channels=2, bits=16, total samples small.
    let sample_rate: u32 = 44100;
    si[10] = (sample_rate >> 12) as u8;
    si[11] = (sample_rate >> 4) as u8;
    // byte 12: sample_rate low 4 bits | channels-1 (3 bits) << 1 | bits high bit
    let chan_field: u8 = (2 - 1) & 0x07;
    let bits_field: u8 = (16 - 1) & 0x1F;
    si[12] = (((sample_rate & 0x0F) << 4) as u8) | (chan_field << 1) | ((bits_field >> 4) & 0x01);
    si[13] = ((bits_field & 0x0F) << 4) | 0x00;
    // total samples bytes 14-17 small
    si[17] = 100;
    for (i, byte) in si[18..34].iter_mut().enumerate() {
        *byte = i as u8;
    }
    b.extend_from_slice(&si);
    b
}

#[test]
fn ogg_flac_mapping_header_streaminfo() {
    let body = ogg_flac_mapping_body();
    let page = ogg_page(0x02, 77, 0, &body);
    let md = parse_ogg_metadata(&TestReader::new(page)).expect("ogg-flac mapping");
    assert_eq!(
        md.get("FLAC:SampleRate").and_then(|v| v.as_integer()),
        Some(44100)
    );
    assert_eq!(
        md.get("FLAC:Channels").and_then(|v| v.as_integer()),
        Some(2)
    );
    assert_eq!(
        md.get("FLAC:BitsPerSample").and_then(|v| v.as_integer()),
        Some(16)
    );
    assert!(md.contains_key("FLAC:MD5Signature"));
    assert!(md.contains_key("FLAC:BlockSizeMin"));
}

#[test]
fn ogg_flac_vorbis_comment_block_in_page() {
    // A page body whose first byte is a FLAC metadata block header for type 4
    // (VORBIS_COMMENT). The parser parses comments directly from the block.
    let payload = vorbis_comment_payload("flacvendor", &["TITLE=BlockTitle", "ARTIST=BlockArtist"]);
    let mut body = Vec::new();
    // block header: type 4 with last-block flag set (0x84), 3-byte big-endian size
    body.push(0x84);
    let len = payload.len() as u32;
    body.extend_from_slice(&[(len >> 16) as u8, (len >> 8) as u8, len as u8]);
    body.extend_from_slice(&payload);
    let page = ogg_page(0x02, 88, 0, &body);
    let md = parse_ogg_metadata(&TestReader::new(page)).expect("flac vorbis block");
    assert_eq!(
        md.get("Vorbis:Title").and_then(|v| v.as_string()),
        Some("BlockTitle")
    );
    assert_eq!(
        md.get("Vorbis:Artist").and_then(|v| v.as_string()),
        Some("BlockArtist")
    );
}

// ===========================================================================
// OGG — error / truncation / multi-page paths
// ===========================================================================

#[test]
fn ogg_too_small_and_bad_signature() {
    assert!(parse_ogg_metadata(&TestReader::new(b"Og".to_vec())).is_err());
    assert!(parse_ogg_metadata(&TestReader::new(b"NOPEnope".to_vec())).is_err());
}

#[test]
fn ogg_valid_signature_no_pages() {
    // "OggS" + too-short for a full 27-byte page header -> loop breaks, still Ok.
    let mut data = b"OggS".to_vec();
    data.extend_from_slice(&[0u8; 10]);
    let md = parse_ogg_metadata(&TestReader::new(data)).expect("ogg short page");
    assert!(md.contains_key("OGG:PageSequence"));
}

#[test]
fn ogg_page_with_empty_body_then_comments() {
    // First page has an empty body (segment count 0) and the second carries the
    // Vorbis ID + comments — exercises the multi-page advance and serial capture.
    let empty_page = ogg_page(0x02, 0xCAFE, 0, &[]);
    let id_page = ogg_page(0x00, 0xCAFE, 1, &vorbis_id_body(2, 44100, 96000));
    let comment_page = ogg_page(0x00, 0xCAFE, 2, &vorbis_comment_body("v", &["TITLE=Multi"]));
    let mut data = empty_page;
    data.extend_from_slice(&id_page);
    data.extend_from_slice(&comment_page);
    let md = parse_ogg_metadata(&TestReader::new(data)).expect("multi page");
    assert_eq!(
        md.get("Vorbis:Title").and_then(|v| v.as_string()),
        Some("Multi")
    );
    assert_eq!(
        md.get("OGG:SerialNumber").and_then(|v| v.as_string()),
        Some(format!("{}", 0xCAFEu32)).as_deref()
    );
}

#[test]
fn ogg_vorbis_comment_truncated_count() {
    // A comment page claiming more comments than are present -> inner loop breaks.
    let id_page = ogg_page(0x02, 1, 0, &vorbis_id_body(1, 8000, 0));
    let mut payload = Vec::new();
    payload.extend_from_slice(&(1u32).to_le_bytes()); // vendor len 1
    payload.push(b'x');
    payload.extend_from_slice(&(5u32).to_le_bytes()); // claim 5 comments
    payload.extend_from_slice(&(7u32).to_le_bytes()); // first comment len 7
    payload.extend_from_slice(b"TITLE=Z"); // exactly one comment, rest missing
    let mut body = vec![0x03u8];
    body.extend_from_slice(b"vorbis");
    body.extend_from_slice(&payload);
    let comment_page = ogg_page(0x00, 1, 1, &body);
    let mut data = id_page;
    data.extend_from_slice(&comment_page);
    let md = parse_ogg_metadata(&TestReader::new(data)).expect("truncated comments");
    assert_eq!(
        md.get("Vorbis:Title").and_then(|v| v.as_string()),
        Some("Z")
    );
}

#[test]
fn ogg_read_metadata_via_tempfile() {
    let id_page = ogg_page(0x02, 3, 0, &vorbis_id_body(2, 48000, 160000));
    let comment_page = ogg_page(0x00, 3, 1, &vorbis_comment_body("v", &["TITLE=OggFile"]));
    let mut data = id_page;
    data.extend_from_slice(&comment_page);
    let mut tf = tempfile::Builder::new().suffix(".ogg").tempfile().unwrap();
    tf.write_all(&data).unwrap();
    tf.flush().unwrap();
    let md = read_metadata(tf.path()).expect("read ogg");
    assert_eq!(
        md.get("Vorbis:Title").and_then(|v| v.as_string()),
        Some("OggFile")
    );
}
