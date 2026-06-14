//! Wave-3 coverage tests for the MKV, AVI, ASF, MXF, and FLV video parsers.
//!
//! Wave-1 (`cov_video_a.rs`, `cov_video_b.rs`) and wave-2 (`cov_video2.rs`)
//! already drive the happy paths and many error branches. This file targets the
//! REMAINING uncovered branches, focused most heavily on MKV and AVI (which
//! wave-2 did not touch at all):
//!
//! - **MKV:** the full codec-name table (`convert_codec_id_to_name` video + audio
//!   arms), `read_uint` 3-byte / 5-7-byte / 8-byte size branches, `read_sint`
//!   3-7-byte sign-extension, track-type mappings (Complex/Buttons/Control/
//!   Unknown), chapter TimeEnd + multiple chapters, multiple attachments,
//!   LANGUAGE_BCP47, non-unit TrackTimecodeScale, default-duration-derived FPS,
//!   FlagDefault/Enabled/Forced/CodecDecodeAll false branches, DateUTC ->
//!   `format_unix_timestamp_utc`, and the unknown-element skip paths.
//! - **AVI:** the `_PMX` XMP chunk, `IDIT` date, audio-only and video format
//!   (BITMAPINFOHEADER) chunks, the `convert_fourcc_to_codec_name` video + audio
//!   tables, `map`-style audio `format_tag` arms, the `strn` chunk skip, the
//!   second-stream `is_first == false` branches, and `MaxDataRate`.
//! - **ASF:** the File Properties object (FILETIME -> string, durations, packet
//!   sizes, bitrate), Content Description (title/author/copyright/description/
//!   rating), Stream Properties (audio + video type-specific data, error
//!   correction GUIDs), Codec List (video + two audio entries +
//!   `map_audio_format_tag`), and the FILETIME-underflow path.
//! - **MXF / FLV:** a few remaining formatting branches (FLV duration/bitrate
//!   string formatting, audio-datarate decimal stripping; MXF preface set +
//!   timeline track set tags).
//!
//! Everything is driven through the public `parse_*_metadata` / `read_metadata`
//! API using synthetic byte buffers.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::operations::read_metadata;
use oxidex::parsers::video::asf::parse_asf_metadata;
use oxidex::parsers::video::avi::parse_avi_metadata;
use oxidex::parsers::video::flv::parse_flv_metadata;
use oxidex::parsers::video::mkv::parse_mkv_metadata;
use oxidex::parsers::video::mxf::parse_mxf_metadata;
use std::io::Write;
use tempfile::NamedTempFile;

// ===========================================================================
// Shared little/big-endian helpers
// ===========================================================================

fn le16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn le32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
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

fn read_via_tempfile(data: &[u8], ext: &str) -> oxidex::error::Result<oxidex::core::MetadataMap> {
    let tmp = NamedTempFile::with_suffix(format!(".{ext}").as_str()).expect("tempfile");
    {
        let mut f = tmp.reopen().expect("reopen");
        f.write_all(data).expect("write");
        f.flush().expect("flush");
    }
    read_metadata(tmp.path())
}

// ===========================================================================
// EBML / Matroska helpers (mirroring the wave-1 encoders)
// ===========================================================================

const EBML_HEADER: u32 = 0x1A45DFA3;
const EBML_VERSION: u32 = 0x4286;
const EBML_DOC_TYPE: u32 = 0x4282;
const EBML_DOC_TYPE_VERSION: u32 = 0x4287;
const EBML_DOC_TYPE_READ_VERSION: u32 = 0x4285;

const SEGMENT: u32 = 0x18538067;
const INFO: u32 = 0x1549A966;
const TRACKS: u32 = 0x1654AE6B;
const TAGS: u32 = 0x1254C367;
const CHAPTERS: u32 = 0x1043A770;
const ATTACHMENTS: u32 = 0x1941A469;
const SEEK_HEAD: u32 = 0x114D9B74;

const TIMECODE_SCALE: u32 = 0x2AD7B1;
const DURATION: u32 = 0x4489;
const DATE_UTC: u32 = 0x4461;
const TITLE: u32 = 0x7BA9;
const MUXING_APP: u32 = 0x4D80;
const WRITING_APP: u32 = 0x5741;

const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const TRACK_UID: u32 = 0x73C5;
const TRACK_TYPE: u32 = 0x83;
const FLAG_DEFAULT: u32 = 0x88;
const FLAG_ENABLED: u32 = 0xB9;
const FLAG_FORCED: u32 = 0x55AA;
const DEFAULT_DURATION: u32 = 0x23E383;
const TRACK_TIMECODE_SCALE: u32 = 0x23314F;
const CODEC_ID: u32 = 0x86;
const CODEC_DECODE_ALL: u32 = 0xAA;
const LANGUAGE_BCP47: u32 = 0x22B59D;

const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const FRAME_RATE: u32 = 0x2383E3;
const FLAG_INTERLACED: u32 = 0x9A;

const AUDIO: u32 = 0xE1;
const SAMPLING_FREQUENCY: u32 = 0xB5;
const CHANNELS: u32 = 0x9F;
const BIT_DEPTH: u32 = 0x6264;

const TAG: u32 = 0x7373;
const SIMPLE_TAG: u32 = 0x67C8;
const TAG_NAME: u32 = 0x45A3;
const TAG_STRING: u32 = 0x4487;

const EDITION_ENTRY: u32 = 0x45B9;
const CHAPTER_ATOM: u32 = 0xB6;
const CHAPTER_TIME_START: u32 = 0x91;
const CHAPTER_TIME_END: u32 = 0x92;
const CHAPTER_DISPLAY: u32 = 0x80;
const CHAP_STRING: u32 = 0x85;

const ATTACHED_FILE: u32 = 0x61A7;
const FILE_NAME: u32 = 0x466E;
const FILE_MIME_TYPE: u32 = 0x4660;
const FILE_DESCRIPTION: u32 = 0x467E;

/// Encode an EBML size VINT (1-, 2-, or 3-byte forms).
fn ebml_size(size: u64) -> Vec<u8> {
    if size < 0x7F {
        vec![0x80 | size as u8]
    } else if size < 0x3FFF {
        vec![0x40 | ((size >> 8) as u8), (size & 0xFF) as u8]
    } else {
        vec![
            0x20 | ((size >> 16) as u8),
            ((size >> 8) & 0xFF) as u8,
            (size & 0xFF) as u8,
        ]
    }
}

fn id_bytes(id: u32) -> Vec<u8> {
    if id <= 0xFF {
        vec![id as u8]
    } else if id <= 0xFFFF {
        vec![(id >> 8) as u8, (id & 0xFF) as u8]
    } else if id <= 0xFF_FFFF {
        vec![
            (id >> 16) as u8,
            ((id >> 8) & 0xFF) as u8,
            (id & 0xFF) as u8,
        ]
    } else {
        vec![
            (id >> 24) as u8,
            ((id >> 16) & 0xFF) as u8,
            ((id >> 8) & 0xFF) as u8,
            (id & 0xFF) as u8,
        ]
    }
}

fn ebml_elem(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = id_bytes(id);
    out.extend(ebml_size(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

/// Big-endian unsigned integer payload of `len` bytes.
fn ebml_uint(id: u32, value: u64, len: usize) -> Vec<u8> {
    let mut payload = Vec::new();
    for i in (0..len).rev() {
        payload.push(((value >> (i * 8)) & 0xFF) as u8);
    }
    ebml_elem(id, &payload)
}

/// Signed integer payload of `len` bytes (two's complement big-endian).
fn ebml_sint(id: u32, value: i64, len: usize) -> Vec<u8> {
    let mut payload = Vec::new();
    let bytes = value.to_be_bytes();
    payload.extend_from_slice(&bytes[8 - len..]);
    ebml_elem(id, &payload)
}

fn ebml_str(id: u32, s: &str) -> Vec<u8> {
    ebml_elem(id, s.as_bytes())
}

fn ebml_f32(id: u32, value: f32) -> Vec<u8> {
    ebml_elem(id, &value.to_be_bytes())
}

fn ebml_f64(id: u32, value: f64) -> Vec<u8> {
    ebml_elem(id, &value.to_be_bytes())
}

fn ebml_header(doc_type: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(ebml_uint(EBML_VERSION, 1, 1));
    body.extend(ebml_str(EBML_DOC_TYPE, doc_type));
    body.extend(ebml_uint(EBML_DOC_TYPE_VERSION, 2, 1));
    body.extend(ebml_uint(EBML_DOC_TYPE_READ_VERSION, 2, 1));
    ebml_elem(EBML_HEADER, &body)
}

/// Wrap a segment body into a full MKV file (EBML header + Segment).
fn mkv_file(doc_type: &str, segment_body: &[u8]) -> Vec<u8> {
    let mut data = ebml_header(doc_type);
    data.extend(ebml_elem(SEGMENT, segment_body));
    data
}

// ===========================================================================
// MKV tests
// ===========================================================================

#[test]
fn test_mkv_read_uint_multibyte_sizes() {
    // TimecodeScale via a 3-byte uint, and a TrackUID via an 8-byte uint, plus a
    // DefaultDuration via a 4-byte uint. This drives read_uint's 3-byte and
    // 8-byte branches (the 1/2/4 branches are hit elsewhere).
    let mut info = Vec::new();
    info.extend(ebml_uint(TIMECODE_SCALE, 1_000_000, 3)); // 3-byte read_uint
    let segment = {
        let mut s = Vec::new();
        s.extend(ebml_elem(INFO, &info));
        // a video track entry with an 8-byte TrackUID and 5-byte DefaultDuration
        let mut track = Vec::new();
        track.extend(ebml_uint(TRACK_TYPE, 1, 1));
        track.extend(ebml_uint(TRACK_NUMBER, 1, 1));
        track.extend(ebml_uint(TRACK_UID, 0x0102030405060708, 8)); // 8-byte
        track.extend(ebml_uint(DEFAULT_DURATION, 33_000_000, 5)); // 5-byte branch
        track.extend(ebml_str(CODEC_ID, "V_VP9"));
        let mut video = Vec::new();
        video.extend(ebml_uint(PIXEL_WIDTH, 640, 2));
        video.extend(ebml_uint(PIXEL_HEIGHT, 480, 2));
        track.extend(ebml_elem(VIDEO, &video));
        let tracks = ebml_elem(TRACK_ENTRY, &track);
        s.extend(ebml_elem(TRACKS, &tracks));
        s
    };
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("mkv multibyte uint parse");
    assert!(md.contains_key("Matroska:TimecodeScale"));
    assert!(md.contains_key("Matroska:TrackUID"));
    assert!(md.contains_key("Matroska:DefaultDuration"));
    // fps derived from default duration (no explicit FrameRate)
    assert!(md.contains_key("Matroska:VideoFrameRate"));
}

#[test]
fn test_mkv_date_utc_drives_timestamp_format() {
    // DateUTC is signed nanoseconds since 2001-01-01; drive read_sint (2-byte)
    // and format_unix_timestamp_utc.
    let mut info = Vec::new();
    info.extend(ebml_sint(DATE_UTC, 0, 1)); // 1-byte sint -> 2001-01-01
    info.extend(ebml_str(TITLE, "My Movie"));
    info.extend(ebml_str(MUXING_APP, "libmkv"));
    info.extend(ebml_str(WRITING_APP, "oxidex"));
    let segment = ebml_elem(INFO, &info);
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("mkv dateutc parse");
    let dt = md
        .get("Matroska:DateTimeOriginal")
        .and_then(|v| v.as_string())
        .expect("datetimeoriginal present");
    assert!(dt.ends_with('Z'), "got {dt}");
    assert!(dt.starts_with("2001:01:01"), "got {dt}");
    assert_eq!(
        md.get("Matroska:Title").and_then(|v| v.as_string()),
        Some("My Movie")
    );
    assert!(md.contains_key("Matroska:MuxingApp"));
    assert!(md.contains_key("Matroska:WritingApp"));
}

#[test]
fn test_mkv_date_utc_multibyte_sint() {
    // A larger DateUTC value forces read_sint's 8-byte and the multi-byte
    // sign-extension branches when negative.
    let mut info = Vec::new();
    // +1 year worth of nanoseconds (positive, 8-byte)
    info.extend(ebml_sint(DATE_UTC, 31_536_000_000_000_000, 8));
    let segment = ebml_elem(INFO, &info);
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("mkv 8-byte dateutc parse");
    assert!(md.contains_key("Matroska:DateTimeOriginal"));

    // Negative value via a 4-byte sint to drive the sign-extension loop.
    let mut info2 = Vec::new();
    info2.extend(ebml_sint(DATE_UTC, -1_000_000_000, 4));
    let segment2 = ebml_elem(INFO, &info2);
    let data2 = mkv_file("matroska", &segment2);
    let reader2 = TestReader::new(data2);
    assert!(parse_mkv_metadata(&reader2).is_ok());
}

#[test]
fn test_mkv_video_codec_name_table() {
    // Exercise a wide spread of convert_codec_id_to_name video arms not covered
    // by wave-1's mapping test.
    for (codec_id, expect) in [
        ("V_UNCOMPRESSED", "Uncompressed"),
        ("V_MPEGH/ISO/HEVC", "H.265"),
        ("V_AV1", "AV1"),
        ("V_MPEG4/MS/V3", "MPEG4 V3"),
        ("V_MPEG1", "MPEG1"),
        ("V_MPEG2", "MPEG2"),
        ("V_REAL/RV40", "RealVideo 4.0"),
        ("V_VP8", "VP8"),
        ("V_QUICKTIME", "QuickTime"),
        ("V_DIRAC", "Dirac"),
        ("V_PRORES", "ProRes"),
        ("V_FFV1", "FFV1"),
        ("V_THEORA", "Theora"),
        ("V_JPEG2000", "JPEG 2000"),
        ("V_VC1", "VC-1"),
        ("V_UNKNOWN_XYZ", "V_UNKNOWN_XYZ"), // default arm
    ] {
        let mut track = Vec::new();
        track.extend(ebml_uint(TRACK_TYPE, 1, 1));
        track.extend(ebml_uint(TRACK_NUMBER, 1, 1));
        track.extend(ebml_str(CODEC_ID, codec_id));
        let tracks = ebml_elem(TRACK_ENTRY, &track);
        let segment = ebml_elem(TRACKS, &tracks);
        let data = mkv_file("matroska", &segment);
        let reader = TestReader::new(data);
        let md = parse_mkv_metadata(&reader).expect("video codec table parse");
        assert_eq!(
            md.get("MKV:VideoCodec").and_then(|v| v.as_string()),
            Some(expect),
            "codec {codec_id}"
        );
    }
}

#[test]
fn test_mkv_audio_codec_name_table() {
    for (codec_id, expect) in [
        ("A_MPEG/L1", "MP1"),
        ("A_MPEG/L2", "MP2"),
        ("A_PCM/INT/BIG", "PCM"),
        ("A_PCM/FLOAT/IEEE", "PCM (IEEE Float)"),
        ("A_AC3", "AC-3"),
        ("A_EAC3", "E-AC-3"),
        ("A_ALAC", "ALAC"),
        ("A_DTS/CORE", "DTS Core"),
        ("A_DTS/LOSSLESS", "DTS-HD"),
        ("A_FLAC", "FLAC"),
        ("A_TRUEHD", "TrueHD"),
        ("A_MLP", "MLP"),
        ("A_AAC/MPEG4/LC", "AAC-LC"),
        ("A_AAC/MPEG4/SBR", "AAC-HE"),
        ("A_AAC/MPEG2/MAIN", "AAC"),
        ("A_VORBIS", "Vorbis"),
        ("A_OPUS", "Opus"),
        ("A_REAL/COOK", "RealAudio Cook"),
        ("A_WAVPACK4", "WavPack"),
        ("A_TWOS", "PCM (Two's Complement)"),
        ("A_MSADPCM", "MS ADPCM"),
        ("A_IMAADPCM", "IMA ADPCM"),
        ("A_UNKNOWN_CODEC", "A_UNKNOWN_CODEC"), // default arm
    ] {
        let mut track = Vec::new();
        track.extend(ebml_uint(TRACK_TYPE, 2, 1));
        track.extend(ebml_uint(TRACK_NUMBER, 2, 1));
        track.extend(ebml_str(CODEC_ID, codec_id));
        let tracks = ebml_elem(TRACK_ENTRY, &track);
        let segment = ebml_elem(TRACKS, &tracks);
        let data = mkv_file("matroska", &segment);
        let reader = TestReader::new(data);
        let md = parse_mkv_metadata(&reader).expect("audio codec table parse");
        assert_eq!(
            md.get("MKV:AudioCodec").and_then(|v| v.as_string()),
            Some(expect),
            "codec {codec_id}"
        );
    }
}

#[test]
fn test_mkv_track_type_variants_and_flags() {
    // Complex (3), Buttons (18), Control (32) and Unknown (99) track types, plus
    // FlagDefault/Enabled/Forced/CodecDecodeAll all set to non-default values and
    // a non-unit TrackTimecodeScale, plus LANGUAGE_BCP47.
    for (ttype, expect) in [
        (3u64, "Complex"),
        (18u64, "Buttons"),
        (32u64, "Control"),
        (99u64, "Unknown"),
    ] {
        let mut track = Vec::new();
        track.extend(ebml_uint(TRACK_TYPE, ttype, 1));
        track.extend(ebml_uint(TRACK_NUMBER, 1, 1));
        track.extend(ebml_uint(FLAG_DEFAULT, 0, 1)); // -> "No"
        track.extend(ebml_uint(FLAG_ENABLED, 0, 1)); // -> "No"
        track.extend(ebml_uint(FLAG_FORCED, 1, 1)); // -> "Yes"
        track.extend(ebml_uint(CODEC_DECODE_ALL, 0, 1)); // -> "No"
        track.extend(ebml_f32(TRACK_TIMECODE_SCALE, 2.5)); // non-unit
        track.extend(ebml_str(LANGUAGE_BCP47, "en-US"));
        let tracks = ebml_elem(TRACK_ENTRY, &track);
        let segment = ebml_elem(TRACKS, &tracks);
        let data = mkv_file("matroska", &segment);
        let reader = TestReader::new(data);
        let md = parse_mkv_metadata(&reader).expect("track type parse");
        assert_eq!(
            md.get("Matroska:TrackType").and_then(|v| v.as_string()),
            Some(expect),
            "track type {ttype}"
        );
        assert_eq!(
            md.get("Matroska:TrackDefault").and_then(|v| v.as_string()),
            Some("No")
        );
        assert_eq!(
            md.get("Matroska:TrackUsed").and_then(|v| v.as_string()),
            Some("No")
        );
        assert_eq!(
            md.get("Matroska:TrackForced").and_then(|v| v.as_string()),
            Some("Yes")
        );
        assert_eq!(
            md.get("Matroska:CodecDecodeAll")
                .and_then(|v| v.as_string()),
            Some("No")
        );
        assert_eq!(
            md.get("Matroska:TrackLanguage").and_then(|v| v.as_string()),
            Some("en-US")
        );
        // non-unit timecode scale path
        assert!(md.contains_key("Matroska:TrackTimecodeScale"));
    }
}

#[test]
fn test_mkv_video_explicit_framerate_and_interlace_progressive() {
    // Explicit FrameRate (skips the default-duration derivation) and an
    // interlace flag of 2 (Progressive). Display dims explicitly absent so the
    // pixel-dim fallback runs.
    let mut video = Vec::new();
    video.extend(ebml_uint(PIXEL_WIDTH, 1920, 2));
    video.extend(ebml_uint(PIXEL_HEIGHT, 1080, 2));
    video.extend(ebml_uint(FLAG_INTERLACED, 2, 1)); // Progressive
    video.extend(ebml_f64(FRAME_RATE, 23.976));
    let mut track = Vec::new();
    track.extend(ebml_uint(TRACK_TYPE, 1, 1));
    track.extend(ebml_uint(TRACK_NUMBER, 1, 1));
    track.extend(ebml_elem(VIDEO, &video));
    let tracks = ebml_elem(TRACK_ENTRY, &track);
    let segment = ebml_elem(TRACKS, &tracks);
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("explicit framerate parse");
    assert_eq!(
        md.get("Matroska:VideoScanType").and_then(|v| v.as_string()),
        Some("Progressive")
    );
    assert!(md.contains_key("Matroska:VideoFrameRate"));
    assert!(md.contains_key("MKV:FrameRate"));
    // Display dims fell back to pixel dims.
    assert_eq!(
        md.get("Matroska:DisplayWidth").and_then(|v| v.as_integer()),
        Some(1920)
    );
}

#[test]
fn test_mkv_audio_full_with_bitdepth() {
    let mut audio = Vec::new();
    audio.extend(ebml_f64(SAMPLING_FREQUENCY, 48000.0));
    audio.extend(ebml_uint(CHANNELS, 6, 1));
    audio.extend(ebml_uint(BIT_DEPTH, 24, 1));
    let mut track = Vec::new();
    track.extend(ebml_uint(TRACK_TYPE, 2, 1));
    track.extend(ebml_uint(TRACK_NUMBER, 2, 1));
    track.extend(ebml_str(CODEC_ID, "A_OPUS"));
    track.extend(ebml_elem(AUDIO, &audio));
    let tracks = ebml_elem(TRACK_ENTRY, &track);
    let segment = ebml_elem(TRACKS, &tracks);
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("audio full parse");
    assert_eq!(
        md.get("Matroska:AudioSampleRate")
            .and_then(|v| v.as_integer()),
        Some(48000)
    );
    assert_eq!(
        md.get("Matroska:AudioChannels")
            .and_then(|v| v.as_integer()),
        Some(6)
    );
    assert_eq!(
        md.get("Matroska:AudioBitsPerSample")
            .and_then(|v| v.as_integer()),
        Some(24)
    );
}

#[test]
fn test_mkv_chapters_multiple_with_timeend() {
    // Two chapters, each with TimeStart, TimeEnd, and a display title.
    let mut chap_display1 = Vec::new();
    chap_display1.extend(ebml_str(CHAP_STRING, "Intro"));
    let mut atom1 = Vec::new();
    atom1.extend(ebml_uint(CHAPTER_TIME_START, 0, 2));
    atom1.extend(ebml_uint(CHAPTER_TIME_END, 5_000_000_000, 5)); // 5s in ns
    atom1.extend(ebml_elem(CHAPTER_DISPLAY, &chap_display1));

    let mut chap_display2 = Vec::new();
    chap_display2.extend(ebml_str(CHAP_STRING, "Main"));
    let mut atom2 = Vec::new();
    atom2.extend(ebml_uint(CHAPTER_TIME_START, 5_000_000_000, 5));
    atom2.extend(ebml_elem(CHAPTER_DISPLAY, &chap_display2));

    let mut edition = Vec::new();
    edition.extend(ebml_elem(CHAPTER_ATOM, &atom1));
    edition.extend(ebml_elem(CHAPTER_ATOM, &atom2));
    let chapters = ebml_elem(EDITION_ENTRY, &edition);
    let segment = ebml_elem(CHAPTERS, &chapters);
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("chapters parse");
    assert_eq!(
        md.get("Matroska:ChapterCount").and_then(|v| v.as_integer()),
        Some(2)
    );
    assert!(md.contains_key("Matroska:Chapter1:TimeStart"));
    assert!(md.contains_key("Matroska:Chapter1:TimeEnd"));
    assert_eq!(
        md.get("Matroska:Chapter1:Title")
            .and_then(|v| v.as_string()),
        Some("Intro")
    );
    assert_eq!(
        md.get("Matroska:Chapter2:Title")
            .and_then(|v| v.as_string()),
        Some("Main")
    );
}

#[test]
fn test_mkv_attachments_multiple() {
    let mut file1 = Vec::new();
    file1.extend(ebml_str(FILE_NAME, "cover.jpg"));
    file1.extend(ebml_str(FILE_MIME_TYPE, "image/jpeg"));
    file1.extend(ebml_str(FILE_DESCRIPTION, "Album cover"));

    let mut file2 = Vec::new();
    file2.extend(ebml_str(FILE_NAME, "subs.srt"));
    file2.extend(ebml_str(FILE_MIME_TYPE, "text/plain"));

    let mut atts = Vec::new();
    atts.extend(ebml_elem(ATTACHED_FILE, &file1));
    atts.extend(ebml_elem(ATTACHED_FILE, &file2));
    let segment = ebml_elem(ATTACHMENTS, &atts);
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("attachments parse");
    assert_eq!(
        md.get("Matroska:AttachmentCount")
            .and_then(|v| v.as_integer()),
        Some(2)
    );
    assert_eq!(
        md.get("Matroska:Attachment1:FileName")
            .and_then(|v| v.as_string()),
        Some("cover.jpg")
    );
    assert_eq!(
        md.get("Matroska:Attachment1:Description")
            .and_then(|v| v.as_string()),
        Some("Album cover")
    );
    assert_eq!(
        md.get("Matroska:Attachment2:MIMEType")
            .and_then(|v| v.as_string()),
        Some("text/plain")
    );
}

#[test]
fn test_mkv_tags_segment_simpletag() {
    let mut simple = Vec::new();
    simple.extend(ebml_str(TAG_NAME, "ARTIST"));
    simple.extend(ebml_str(TAG_STRING, "Some Artist"));
    let mut tag = Vec::new();
    tag.extend(ebml_elem(SIMPLE_TAG, &simple));
    let tags = ebml_elem(TAG, &tag);
    let segment = ebml_elem(TAGS, &tags);
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("tags parse");
    assert_eq!(
        md.get("Matroska:Tag:ARTIST").and_then(|v| v.as_string()),
        Some("Some Artist")
    );
}

#[test]
fn test_mkv_segment_with_skipped_seekhead_and_duration() {
    // A SeekHead (skipped) followed by Info with a float Duration. Drives the
    // unknown-segment-child skip and the Duration formatting branch.
    let mut info = Vec::new();
    info.extend(ebml_uint(TIMECODE_SCALE, 1_000_000, 4));
    info.extend(ebml_f64(DURATION, 125_000.0)); // 125s at 1ms scale
    let mut segment = Vec::new();
    segment.extend(ebml_elem(SEEK_HEAD, &[0u8; 4])); // skipped child
    segment.extend(ebml_elem(INFO, &info));
    let data = mkv_file("matroska", &segment);
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("seekhead+duration parse");
    assert!(md.contains_key("Matroska:Duration"));
    assert!(md.contains_key("MKV:Duration"));
}

#[test]
fn test_mkv_production_path() {
    let mut track = Vec::new();
    track.extend(ebml_uint(TRACK_TYPE, 1, 1));
    track.extend(ebml_uint(TRACK_NUMBER, 1, 1));
    track.extend(ebml_str(CODEC_ID, "V_AV1"));
    let mut video = Vec::new();
    video.extend(ebml_uint(PIXEL_WIDTH, 3840, 2));
    video.extend(ebml_uint(PIXEL_HEIGHT, 2160, 2));
    track.extend(ebml_elem(VIDEO, &video));
    let tracks = ebml_elem(TRACK_ENTRY, &track);
    let segment = ebml_elem(TRACKS, &tracks);
    let data = mkv_file("matroska", &segment);
    let md = read_via_tempfile(&data, "mkv").expect("mkv production read");
    assert_eq!(md.get("MKV:Width").and_then(|v| v.as_integer()), Some(3840));
}

// ===========================================================================
// AVI (RIFF) tests
// ===========================================================================

fn avi_container(body: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"RIFF");
    le32(&mut buf, (4 + body.len()) as u32);
    buf.extend_from_slice(b"AVI ");
    buf.extend_from_slice(body);
    buf
}

fn riff_chunk(buf: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    buf.extend_from_slice(id);
    le32(buf, payload.len() as u32);
    buf.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        buf.push(0);
    }
}

fn riff_list(buf: &mut Vec<u8>, list_type: &[u8; 4], inner: &[u8]) {
    buf.extend_from_slice(b"LIST");
    le32(buf, (4 + inner.len()) as u32);
    buf.extend_from_slice(list_type);
    buf.extend_from_slice(inner);
    if (4 + inner.len()) % 2 == 1 {
        buf.push(0);
    }
}

fn avih_payload(usec_per_frame: u32, total_frames: u32, streams: u32, w: u32, h: u32) -> Vec<u8> {
    let mut p = vec![0u8; 56];
    p[0..4].copy_from_slice(&usec_per_frame.to_le_bytes());
    p[4..8].copy_from_slice(&750_000u32.to_le_bytes()); // max bytes/sec
    p[16..20].copy_from_slice(&total_frames.to_le_bytes());
    p[24..28].copy_from_slice(&streams.to_le_bytes());
    p[32..36].copy_from_slice(&w.to_le_bytes());
    p[36..40].copy_from_slice(&h.to_le_bytes());
    p
}

fn strh_payload(
    fcc_type: &[u8; 4],
    handler: &[u8; 4],
    rate: u32,
    scale: u32,
    length: u32,
) -> Vec<u8> {
    let mut p = vec![0u8; 56];
    p[0..4].copy_from_slice(fcc_type);
    p[4..8].copy_from_slice(handler);
    p[20..24].copy_from_slice(&scale.to_le_bytes());
    p[24..28].copy_from_slice(&rate.to_le_bytes());
    p[32..36].copy_from_slice(&length.to_le_bytes());
    p[44..48].copy_from_slice(&75u32.to_le_bytes()); // quality
    p[48..52].copy_from_slice(&0u32.to_le_bytes()); // sample size = 0 => Variable
    p
}

fn bih_payload(w: u32, h: u32, bit_count: u16, compression: &[u8; 4]) -> Vec<u8> {
    let mut bih = vec![0u8; 40];
    bih[4..8].copy_from_slice(&w.to_le_bytes());
    bih[8..12].copy_from_slice(&h.to_le_bytes());
    bih[14..16].copy_from_slice(&bit_count.to_le_bytes());
    bih[16..20].copy_from_slice(compression);
    bih
}

fn wfx_payload(
    format_tag: u16,
    channels: u16,
    samples_per_sec: u32,
    avg_bps: u32,
    bits: u16,
) -> Vec<u8> {
    let mut wfx = vec![0u8; 16];
    wfx[0..2].copy_from_slice(&format_tag.to_le_bytes());
    wfx[2..4].copy_from_slice(&channels.to_le_bytes());
    wfx[4..8].copy_from_slice(&samples_per_sec.to_le_bytes());
    wfx[8..12].copy_from_slice(&avg_bps.to_le_bytes());
    wfx[14..16].copy_from_slice(&bits.to_le_bytes());
    wfx
}

#[test]
fn test_avi_audio_only_stream() {
    // An audio strl with strh (auds) + strf (WAVEFORMATEX). Drives
    // parse_audio_format with is_first == true, plus the strh audio branches
    // (sample count, sample rate from rate/scale).
    let mut astrl = Vec::new();
    riff_chunk(
        &mut astrl,
        b"strh",
        &strh_payload(b"auds", b"\0\0\0\0", 44100, 1, 5000),
    );
    riff_chunk(
        &mut astrl,
        b"strf",
        &wfx_payload(0x0001, 2, 48000, 192000, 16), // Microsoft PCM
    );

    let mut hdrl = Vec::new();
    riff_chunk(&mut hdrl, b"avih", &avih_payload(33_333, 0, 1, 0, 0));
    riff_list(&mut hdrl, b"strl", &astrl);

    let mut body = Vec::new();
    riff_list(&mut body, b"hdrl", &hdrl);
    let data = avi_container(&body);
    let reader = TestReader::new(data);
    let md = parse_avi_metadata(&reader).expect("avi audio-only parse");
    assert_eq!(
        md.get("RIFF:Encoding").and_then(|v| v.as_string()),
        Some("Microsoft PCM")
    );
    assert_eq!(
        md.get("RIFF:NumChannels").and_then(|v| v.as_integer()),
        Some(2)
    );
    assert_eq!(
        md.get("RIFF:SampleRate").and_then(|v| v.as_integer()),
        Some(48000)
    );
    assert_eq!(
        md.get("AVI:AudioBitRate").and_then(|v| v.as_integer()),
        Some(192000 * 8)
    );
    assert_eq!(
        md.get("RIFF:BitsPerSample").and_then(|v| v.as_integer()),
        Some(16)
    );
    assert!(md.contains_key("RIFF:AudioSampleCount"));
}

#[test]
fn test_avi_video_fourcc_codec_table() {
    // Each iteration builds a one-video-stream AVI to drive a distinct
    // convert_fourcc_to_codec_name video arm.
    for (fourcc, expect) in [
        (b"HEVC", "H.265"),
        (b"AV01", "AV1"),
        (b"VP80", "VP8"),
        (b"VP90", "VP9"),
        (b"DIVX", "DivX"),
        (b"XVID", "Xvid"),
        (b"MJPG", "Motion JPEG"),
        (b"CVID", "Cinepak"),
        (b"WMV1", "WMV1"),
        (b"WMV2", "WMV2"),
        (b"I263", "Intel H.263"),
        (b"FFV1", "FFV1"),
        (b"ZZZZ", "ZZZZ"), // unknown -> uppercase passthrough
    ] {
        let mut vstrl = Vec::new();
        riff_chunk(
            &mut vstrl,
            b"strh",
            &strh_payload(b"vids", fourcc, 30, 1, 100),
        );
        riff_chunk(&mut vstrl, b"strf", &bih_payload(640, 480, 24, fourcc));
        let mut hdrl = Vec::new();
        riff_chunk(&mut hdrl, b"avih", &avih_payload(33_333, 100, 1, 640, 480));
        riff_list(&mut hdrl, b"strl", &vstrl);
        let mut body = Vec::new();
        riff_list(&mut body, b"hdrl", &hdrl);
        let data = avi_container(&body);
        let reader = TestReader::new(data);
        let md = parse_avi_metadata(&reader).expect("avi video codec table parse");
        assert_eq!(
            md.get("AVI:VideoCodec").and_then(|v| v.as_string()),
            Some(expect),
            "fourcc {:?}",
            std::str::from_utf8(fourcc)
        );
        // BitDepth comes from BITMAPINFOHEADER bit_count
        assert_eq!(
            md.get("RIFF:BitDepth").and_then(|v| v.as_integer()),
            Some(24)
        );
    }
}

#[test]
fn test_avi_audio_format_tag_table() {
    // Drive several format_tag arms of parse_audio_format.
    for (tag, expect) in [
        (0x0002u16, "Microsoft ADPCM"),
        (0x0003u16, "IEEE Float"),
        (0x0006u16, "ITU G.711 a-law"),
        (0x0007u16, "ITU G.711 mu-law"),
        (0x0011u16, "Intel DVI/IMA ADPCM"),
        (0x0031u16, "GSM 6.10"),
        (0x0050u16, "MPEG"),
        (0x0161u16, "WMA v1"),
        (0x0162u16, "WMA v2"),
        (0xFFFEu16, "Extensible"),
        (0x9999u16, ""), // unknown -> empty string
    ] {
        let mut astrl = Vec::new();
        riff_chunk(
            &mut astrl,
            b"strh",
            &strh_payload(b"auds", b"\0\0\0\0", 44100, 1, 1000),
        );
        riff_chunk(&mut astrl, b"strf", &wfx_payload(tag, 1, 22050, 0, 0));
        let mut hdrl = Vec::new();
        riff_chunk(&mut hdrl, b"avih", &avih_payload(33_333, 0, 1, 0, 0));
        riff_list(&mut hdrl, b"strl", &astrl);
        let mut body = Vec::new();
        riff_list(&mut body, b"hdrl", &hdrl);
        let data = avi_container(&body);
        let reader = TestReader::new(data);
        let md = parse_avi_metadata(&reader).expect("avi audio format tag parse");
        assert_eq!(
            md.get("RIFF:Encoding").and_then(|v| v.as_string()),
            Some(expect),
            "format tag {tag:#06x}"
        );
    }
}

#[test]
fn test_avi_two_video_streams_second_not_first() {
    // Two video strl LISTs: the second one drives the is_first_video == false
    // branches in parse_stream_header and parse_stream_format.
    let make_vstrl = |fourcc: &[u8; 4]| {
        let mut v = Vec::new();
        riff_chunk(&mut v, b"strh", &strh_payload(b"vids", fourcc, 30, 1, 100));
        riff_chunk(&mut v, b"strf", &bih_payload(640, 480, 16, fourcc));
        v
    };
    let mut hdrl = Vec::new();
    riff_chunk(&mut hdrl, b"avih", &avih_payload(33_333, 100, 2, 640, 480));
    let v1 = make_vstrl(b"H264");
    let v2 = make_vstrl(b"MJPG");
    riff_list(&mut hdrl, b"strl", &v1);
    riff_list(&mut hdrl, b"strl", &v2);
    let mut body = Vec::new();
    riff_list(&mut body, b"hdrl", &hdrl);
    let data = avi_container(&body);
    let reader = TestReader::new(data);
    let md = parse_avi_metadata(&reader).expect("avi two video streams parse");
    // Only the first video codec is recorded.
    assert_eq!(
        md.get("RIFF:VideoCodec").and_then(|v| v.as_string()),
        Some("H264")
    );
    assert_eq!(
        md.get("AVI:VideoCodec").and_then(|v| v.as_string()),
        Some("H.264")
    );
}

#[test]
fn test_avi_pmx_xmp_chunk() {
    // _PMX chunk carrying XMP. We don't assert specific tags (parse_xmp shape may
    // vary) — just that the chunk path executes and parsing succeeds.
    let xmp = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:title>XMP Title</dc:title>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;
    let mut body = Vec::new();
    riff_chunk(&mut body, b"_PMX", xmp);
    let data = avi_container(&body);
    let reader = TestReader::new(data);
    assert!(parse_avi_metadata(&reader).is_ok());
}

#[test]
fn test_avi_idit_date_chunk() {
    let mut body = Vec::new();
    riff_chunk(&mut body, b"IDIT", b"2020:01:15 10:30:00\0");
    let data = avi_container(&body);
    let reader = TestReader::new(data);
    let md = parse_avi_metadata(&reader).expect("avi idit parse");
    assert_eq!(
        md.get("RIFF:DateTimeOriginal").and_then(|v| v.as_string()),
        Some("2020:01:15 10:30:00")
    );
}

#[test]
fn test_avi_strn_chunk_skipped() {
    // A strl with strh + strn (stream name, which the parser intentionally
    // skips). Confirms the strn arm and that parsing continues.
    let mut vstrl = Vec::new();
    riff_chunk(
        &mut vstrl,
        b"strh",
        &strh_payload(b"vids", b"XVID", 25, 1, 50),
    );
    riff_chunk(&mut vstrl, b"strn", b"Stream Name\0");
    let mut hdrl = Vec::new();
    riff_chunk(&mut hdrl, b"avih", &avih_payload(40_000, 50, 1, 320, 240));
    riff_list(&mut hdrl, b"strl", &vstrl);
    let mut body = Vec::new();
    riff_list(&mut body, b"hdrl", &hdrl);
    let data = avi_container(&body);
    let reader = TestReader::new(data);
    let md = parse_avi_metadata(&reader).expect("avi strn parse");
    assert_eq!(
        md.get("AVI:VideoCodec").and_then(|v| v.as_string()),
        Some("Xvid")
    );
    // MaxDataRate computed from avih max bytes/sec (750_000 / 1000 = 750 kB/s).
    assert_eq!(
        md.get("RIFF:MaxDataRate").and_then(|v| v.as_string()),
        Some("750 kB/s")
    );
}

#[test]
fn test_avi_top_level_odml_and_info() {
    // A top-level odml LIST (not nested in hdrl) plus a top-level INFO LIST.
    let mut odml = Vec::new();
    let mut dmlh = vec![0u8; 4];
    dmlh[0..4].copy_from_slice(&98765u32.to_le_bytes());
    riff_chunk(&mut odml, b"dmlh", &dmlh);

    let mut info = Vec::new();
    riff_chunk(&mut info, b"INAM", b"Top Title\0");

    let mut body = Vec::new();
    riff_list(&mut body, b"odml", &odml);
    riff_list(&mut body, b"INFO", &info);
    let data = avi_container(&body);
    let reader = TestReader::new(data);
    let md = parse_avi_metadata(&reader).expect("avi top-level odml/info parse");
    assert_eq!(
        md.get("RIFF:TotalFrameCount").and_then(|v| v.as_integer()),
        Some(98765)
    );
}

#[test]
fn test_avi_production_path() {
    let mut hdrl = Vec::new();
    riff_chunk(&mut hdrl, b"avih", &avih_payload(40_000, 100, 1, 800, 600));
    let mut body = Vec::new();
    riff_list(&mut body, b"hdrl", &hdrl);
    let data = avi_container(&body);
    let md = read_via_tempfile(&data, "avi").expect("avi production read");
    assert_eq!(
        md.get("RIFF:ImageWidth").and_then(|v| v.as_integer()),
        Some(800)
    );
}

// ===========================================================================
// ASF tests (objects wave-2 did not build)
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
const CONTENT_DESCRIPTION_GUID: [u8; 16] = [
    0x33, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const CODEC_LIST_GUID: [u8; 16] = [
    0x40, 0x52, 0xD1, 0x86, 0x1D, 0x31, 0xD0, 0x11, 0xA3, 0xA4, 0x00, 0xA0, 0xC9, 0x03, 0x48, 0xF6,
];
const AUDIO_MEDIA_GUID: [u8; 16] = [
    0x40, 0x9E, 0x69, 0xF8, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];
const VIDEO_MEDIA_GUID: [u8; 16] = [
    0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
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

#[test]
fn test_asf_file_properties_full() {
    // File Properties object body (80 bytes after the 24-byte object header).
    let mut body = Vec::new();
    body.extend_from_slice(&[0xAB; 16]); // File ID GUID
    body.extend_from_slice(&123_456u64.to_le_bytes()); // file size
    // creation time: 2010-01-01-ish in FILETIME (> the unix-diff threshold)
    let filetime: u64 = 129_000_000_000_000_000;
    body.extend_from_slice(&filetime.to_le_bytes());
    body.extend_from_slice(&500u64.to_le_bytes()); // data packets
    body.extend_from_slice(&(120u64 * 10_000_000).to_le_bytes()); // play duration 120s
    body.extend_from_slice(&(118u64 * 10_000_000).to_le_bytes()); // send duration 118s
    body.extend_from_slice(&3000u64.to_le_bytes()); // preroll
    body.extend_from_slice(&0x02u32.to_le_bytes()); // flags
    body.extend_from_slice(&3200u32.to_le_bytes()); // min packet
    body.extend_from_slice(&3200u32.to_le_bytes()); // max packet
    body.extend_from_slice(&128_000u32.to_le_bytes()); // max bitrate
    assert_eq!(body.len(), 80);

    let obj = asf_object(&FILE_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("asf file properties parse");

    assert!(md.contains_key("ASF:FileID"));
    assert_eq!(
        md.get("ASF:FileLength").and_then(|v| v.as_integer()),
        Some(123_456)
    );
    assert!(md.contains_key("ASF:CreationDate"));
    let cd = md
        .get("ASF:CreationDate")
        .and_then(|v| v.as_string())
        .unwrap();
    assert!(cd.ends_with('Z'), "got {cd}");
    assert_eq!(
        md.get("ASF:DataPackets").and_then(|v| v.as_integer()),
        Some(500)
    );
    assert!(md.contains_key("ASF:Duration"));
    assert!(md.contains_key("ASF:SendDuration"));
    assert_eq!(
        md.get("ASF:Preroll").and_then(|v| v.as_integer()),
        Some(3000)
    );
    assert!(md.contains_key("ASF:MinPacketSize"));
    assert!(md.contains_key("ASF:MaxPacketSize"));
    assert_eq!(
        md.get("ASF:MaxBitrate").and_then(|v| v.as_string()),
        Some("128.0 kbps")
    );
}

#[test]
fn test_asf_file_properties_filetime_underflow() {
    // creation_time below the FILETIME->Unix difference triggers the
    // "0000:00:00 00:00:00Z" underflow branch in filetime_to_string.
    let mut body = Vec::new();
    body.extend_from_slice(&[0u8; 16]); // File ID
    body.extend_from_slice(&1u64.to_le_bytes()); // file size
    body.extend_from_slice(&1u64.to_le_bytes()); // creation time (tiny -> underflow)
    body.extend_from_slice(&0u64.to_le_bytes()); // data packets
    body.extend_from_slice(&0u64.to_le_bytes()); // play duration (0 -> no Duration)
    body.extend_from_slice(&0u64.to_le_bytes()); // send duration
    body.extend_from_slice(&0u64.to_le_bytes()); // preroll
    body.extend_from_slice(&0u32.to_le_bytes()); // flags
    body.extend_from_slice(&0u32.to_le_bytes()); // min packet
    body.extend_from_slice(&0u32.to_le_bytes()); // max packet
    body.extend_from_slice(&0u32.to_le_bytes()); // max bitrate
    assert_eq!(body.len(), 80);

    let obj = asf_object(&FILE_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("asf filetime underflow parse");
    assert_eq!(
        md.get("ASF:CreationDate").and_then(|v| v.as_string()),
        Some("0000:00:00 00:00:00Z")
    );
    // play_duration 0 means no Duration tag.
    assert!(!md.contains_key("ASF:Duration"));
}

#[test]
fn test_asf_content_description_full() {
    // Content Description: title/author/copyright/description/rating, all set.
    let title = utf16("My Title");
    let author = utf16("The Author");
    let copyright = utf16("(c) 2020");
    let description = utf16("A description");
    let rating = utf16("PG-13");

    let mut body = Vec::new();
    le16(&mut body, title.len() as u16);
    le16(&mut body, author.len() as u16);
    le16(&mut body, copyright.len() as u16);
    le16(&mut body, description.len() as u16);
    le16(&mut body, rating.len() as u16);
    body.extend_from_slice(&title);
    body.extend_from_slice(&author);
    body.extend_from_slice(&copyright);
    body.extend_from_slice(&description);
    body.extend_from_slice(&rating);

    let obj = asf_object(&CONTENT_DESCRIPTION_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("asf content description parse");
    assert_eq!(
        md.get("ASF:Title").and_then(|v| v.as_string()),
        Some("My Title")
    );
    assert_eq!(
        md.get("ASF:Author").and_then(|v| v.as_string()),
        Some("The Author")
    );
    assert_eq!(
        md.get("ASF:Copyright").and_then(|v| v.as_string()),
        Some("(c) 2020")
    );
    assert_eq!(
        md.get("ASF:Description").and_then(|v| v.as_string()),
        Some("A description")
    );
    assert_eq!(
        md.get("ASF:Rating").and_then(|v| v.as_string()),
        Some("PG-13")
    );
}

/// Build a Stream Properties object body. `type_specific` is appended after the
/// 54-byte stream-properties header so it lands at object offset 78.
fn stream_props_body(
    stream_guid: &[u8; 16],
    error_guid: &[u8; 16],
    flags: u16,
    type_specific: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(stream_guid); // 16
    body.extend_from_slice(error_guid); // 16
    body.extend_from_slice(&0u64.to_le_bytes()); // time offset (8)
    body.extend_from_slice(&(type_specific.len() as u32).to_le_bytes()); // type data len (4)
    body.extend_from_slice(&0u32.to_le_bytes()); // error data len (4)
    body.extend_from_slice(&flags.to_le_bytes()); // flags (2)
    body.extend_from_slice(&0u32.to_le_bytes()); // reserved (4)
    // now at offset 54 -> type-specific data
    body.extend_from_slice(type_specific);
    body
}

#[test]
fn test_asf_stream_properties_audio() {
    // Audio stream with WAVEFORMATEX type-specific data + Audio Spread error
    // correction GUID.
    let mut tsd = Vec::new();
    le16(&mut tsd, 0x0055); // format tag
    le16(&mut tsd, 2); // channels
    le32(&mut tsd, 44100); // sample rate
    le32(&mut tsd, 16000); // avg bytes/sec
    le16(&mut tsd, 4); // block align
    le16(&mut tsd, 16); // bits/sample
    tsd.resize(18, 0); // ensure >= 18

    let body = stream_props_body(&AUDIO_MEDIA_GUID, &AUDIO_SPREAD_GUID, 0x0001, &tsd);
    let obj = asf_object(&STREAM_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("asf stream props audio parse");
    assert_eq!(
        md.get("ASF:StreamType").and_then(|v| v.as_string()),
        Some("Audio")
    );
    assert_eq!(
        md.get("ASF:ErrorCorrectionType")
            .and_then(|v| v.as_string()),
        Some("Audio Spread")
    );
    assert_eq!(
        md.get("ASF:AudioChannels").and_then(|v| v.as_integer()),
        Some(2)
    );
    assert_eq!(
        md.get("ASF:AudioSampleRate").and_then(|v| v.as_integer()),
        Some(44100)
    );
    assert_eq!(
        md.get("ASF:StreamNumber").and_then(|v| v.as_integer()),
        Some(1)
    );
}

#[test]
fn test_asf_stream_properties_video_unknown_error_correction() {
    // Video stream with type-specific data carrying width/height, and an
    // unrecognized error-correction GUID -> "Unknown".
    let mut tsd = Vec::new();
    le32(&mut tsd, 1280); // encoded width
    le32(&mut tsd, 720); // encoded height
    tsd.push(0); // reserved flags
    tsd.resize(11, 0); // ensure >= 11 (BITMAPINFOHEADER would follow)

    let weird_error_guid = [0x11u8; 16];
    let body = stream_props_body(&VIDEO_MEDIA_GUID, &weird_error_guid, 0x0002, &tsd);
    let obj = asf_object(&STREAM_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("asf stream props video parse");
    assert_eq!(
        md.get("ASF:StreamType").and_then(|v| v.as_string()),
        Some("Video")
    );
    assert_eq!(
        md.get("ASF:ErrorCorrectionType")
            .and_then(|v| v.as_string()),
        Some("Unknown")
    );
    assert_eq!(
        md.get("ASF:ImageWidth").and_then(|v| v.as_integer()),
        Some(1280)
    );
    assert_eq!(
        md.get("ASF:ImageHeight").and_then(|v| v.as_integer()),
        Some(720)
    );
}

/// Append one codec entry to a Codec List body.
/// type: 1=video, 2=audio. name/desc are UTF-16 strings (length in WCHARs).
/// `info` is the raw codec-information byte block.
fn codec_entry(buf: &mut Vec<u8>, codec_type: u16, name: &str, desc: &str, info: &[u8]) {
    le16(buf, codec_type);
    // name: length in WCHARs, then UTF-16LE (no null needed; parser reads len*2)
    let name_u16: Vec<u16> = name.encode_utf16().collect();
    le16(buf, name_u16.len() as u16);
    for u in &name_u16 {
        buf.extend_from_slice(&u.to_le_bytes());
    }
    let desc_u16: Vec<u16> = desc.encode_utf16().collect();
    le16(buf, desc_u16.len() as u16);
    for u in &desc_u16 {
        buf.extend_from_slice(&u.to_le_bytes());
    }
    le16(buf, info.len() as u16);
    buf.extend_from_slice(info);
}

#[test]
fn test_asf_codec_list_video_and_two_audio() {
    // Codec list with one video and two audio codecs: the second audio entry
    // exercises the `audio_idx > 0` suffix branch, and the 2-byte audio info
    // drives map_audio_format_tag.
    let mut body = Vec::new();
    body.extend_from_slice(&[0xCC; 16]); // reserved GUID
    le32(&mut body, 3); // codec count

    // Video codec: 4-byte FourCC info.
    codec_entry(&mut body, 0x0001, "WMV3", "Windows Media Video 9", b"WMV3");
    // Audio codec 1: format tag 0x0161 (WMA v2/v7/v8/v9...) as 2-byte info.
    let mut info1 = Vec::new();
    le16(&mut info1, 0x0161);
    codec_entry(&mut body, 0x0002, "WMA9", "Windows Media Audio 9", &info1);
    // Audio codec 2: format tag 0x0055 (MP3) -> "MPEG Layer 3".
    let mut info2 = Vec::new();
    le16(&mut info2, 0x0055);
    codec_entry(&mut body, 0x0002, "MP3", "MPEG Audio", &info2);

    let obj = asf_object(&CODEC_LIST_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("asf codec list parse");

    assert_eq!(
        md.get("ASF:VideoCodecName").and_then(|v| v.as_string()),
        Some("WMV3")
    );
    assert!(md.contains_key("ASF:VideoCodecDescription"));
    // first audio entry (no suffix)
    assert_eq!(
        md.get("ASF:AudioCodecName").and_then(|v| v.as_string()),
        Some("WMA9")
    );
    // first audio codec id maps from format tag 0x0161
    assert_eq!(
        md.get("ASF:AudioCodecID").and_then(|v| v.as_string()),
        Some("Windows Media Audio V2 V7 V8 V9 / DivX audio (WMA) / Alex AC3 Audio")
    );
    // second audio entry uses the "_2" suffix
    assert_eq!(
        md.get("ASF:AudioCodecName_2").and_then(|v| v.as_string()),
        Some("MP3")
    );
    assert_eq!(
        md.get("ASF:AudioCodecID_2").and_then(|v| v.as_string()),
        Some("MPEG Layer 3")
    );
}

#[test]
fn test_asf_production_path() {
    let title = utf16("Prod Title");
    let mut body = Vec::new();
    le16(&mut body, title.len() as u16);
    le16(&mut body, 0);
    le16(&mut body, 0);
    le16(&mut body, 0);
    le16(&mut body, 0);
    body.extend_from_slice(&title);
    let obj = asf_object(&CONTENT_DESCRIPTION_GUID, &body);
    let data = asf_file(&[obj]);
    let md = read_via_tempfile(&data, "wmv").expect("asf production read");
    assert_eq!(
        md.get("ASF:Title").and_then(|v| v.as_string()),
        Some("Prod Title")
    );
}

// ===========================================================================
// FLV formatting branches (duration + bitrate string forms)
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
    buf.extend_from_slice(&[0, 0, 0, 0]);
    buf.extend_from_slice(&[0, 0, 0]);
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
fn amf0_object_end(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&[0x00, 0x00, 0x09]);
}

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
fn test_flv_duration_and_bitrate_formatting() {
    // duration -> "X.XX s"; videodatarate -> integer "kbps"; audiodatarate with
    // a fractional value -> one-decimal "kbps"; audiodatarate with a whole
    // value -> stripped ".0".
    let mut body = Vec::new();
    amf0_number(&mut body, "duration", 12.345);
    amf0_number(&mut body, "videodatarate", 1500.6); // rounds to 1501 kbps
    amf0_number(&mut body, "audiodatarate", 128.4); // -> "128.4 kbps"
    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &on_metadata(&body));
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("flv formatting parse");
    assert_eq!(
        md.get("Flash:Duration").and_then(|v| v.as_string()),
        Some("12.35 s")
    );
    assert_eq!(
        md.get("Flash:VideoBitrate").and_then(|v| v.as_string()),
        Some("1501 kbps")
    );
    assert_eq!(
        md.get("Flash:AudioBitrate").and_then(|v| v.as_string()),
        Some("128.4 kbps")
    );
}

#[test]
fn test_flv_audio_bitrate_whole_strips_decimal() {
    let mut body = Vec::new();
    amf0_number(&mut body, "audiodatarate", 96.0); // -> "96 kbps" (no ".0")
    amf0_number(&mut body, "framerate", 29.97); // float arm
    let mut data = flv_header(0x05);
    flv_tag(&mut data, 18, &on_metadata(&body));
    let reader = TestReader::new(data);
    let md = parse_flv_metadata(&reader).expect("flv whole bitrate parse");
    assert_eq!(
        md.get("Flash:AudioBitrate").and_then(|v| v.as_string()),
        Some("96 kbps")
    );
    assert!(md.contains_key("Flash:FrameRate"));
}

// ===========================================================================
// MXF preface + timeline track set tags (wave-2 skipped these)
// ===========================================================================

fn partition_key(b13: u8) -> [u8; 16] {
    [
        0x06, 0x0E, 0x2B, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0D, 0x01, 0x02, 0x01, 0x01, b13, 0x01,
        0x00,
    ]
}

fn local_set_key(b13: u8) -> [u8; 16] {
    [
        0x06, 0x0E, 0x2B, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0D, 0x01, 0x01, 0x01, 0x01, b13, 0x00,
        0x00,
    ]
}

fn klv(buf: &mut Vec<u8>, key: &[u8; 16], value: &[u8]) {
    buf.extend_from_slice(key);
    assert!(value.len() < 128);
    buf.push(value.len() as u8);
    buf.extend_from_slice(value);
}

fn klv_long(buf: &mut Vec<u8>, key: &[u8; 16], value: &[u8]) {
    buf.extend_from_slice(key);
    buf.push(0x82);
    be16(buf, value.len() as u16);
    buf.extend_from_slice(value);
}

fn local_prop(buf: &mut Vec<u8>, tag: u16, value: &[u8]) {
    be16(buf, tag);
    be16(buf, value.len() as u16);
    buf.extend_from_slice(value);
}

fn header_value(major: u16, minor: u16) -> Vec<u8> {
    let mut v = Vec::new();
    be16(&mut v, major);
    be16(&mut v, minor);
    v.resize(24, 0);
    v
}

fn mxf_timestamp() -> Vec<u8> {
    vec![0x07, 0xDA, 0x0C, 0x14, 0x00, 0x0E, 0x28, 0x39]
}

#[test]
fn test_mxf_preface_set_tags() {
    // PrefaceSet UL (local set key[13] = 0x2F): last-modified date (0x3B02) and
    // version (0x3B05).
    let mut props = Vec::new();
    local_prop(&mut props, 0x3B02, &mxf_timestamp());
    let mut ver = Vec::new();
    be16(&mut ver, 0x0102); // major 1, minor 2
    local_prop(&mut props, 0x3B05, &ver);

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    klv_long(&mut data, &local_set_key(0x2F), &props);
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("mxf preface parse");
    assert!(md.contains_key("MXF:ContainerLastModifyDate"));
    assert_eq!(
        md.get("MXF:FileFormatVersion").and_then(|v| v.as_string()),
        Some("1.2")
    );
}

#[test]
fn test_mxf_timeline_track_set_tags() {
    // TimelineTrackSet UL (local set key[13] = 0x3D): edit rate (0x4B01),
    // origin (0x4B02), track id/number/name.
    let mut props = Vec::new();
    let mut edit_rate = Vec::new();
    be32(&mut edit_rate, 25); // numerator
    be32(&mut edit_rate, 1); // denominator
    local_prop(&mut props, 0x4B01, &edit_rate);
    let mut origin = Vec::new();
    origin.extend_from_slice(&100i64.to_be_bytes());
    local_prop(&mut props, 0x4B02, &origin);
    let mut tid = Vec::new();
    be32(&mut tid, 7);
    local_prop(&mut props, 0x4801, &tid);
    let mut tnum = Vec::new();
    be32(&mut tnum, 3);
    local_prop(&mut props, 0x4804, &tnum);
    // Track name as UTF-16BE
    let mut tname = Vec::new();
    for c in "Video".encode_utf16() {
        tname.extend_from_slice(&c.to_be_bytes());
    }
    local_prop(&mut props, 0x4802, &tname);

    let mut data = Vec::new();
    klv(&mut data, &partition_key(0x02), &header_value(1, 2));
    klv_long(&mut data, &local_set_key(0x3D), &props);
    data.resize(data.len() + 48, 0);

    let reader = TestReader::new(data);
    let md = parse_mxf_metadata(&reader).expect("mxf timeline track parse");
    assert_eq!(
        md.get("MXF:EditRate").and_then(|v| v.as_integer()),
        Some(25)
    );
    assert!(md.contains_key("MXF:Origin"));
    assert_eq!(md.get("MXF:TrackID").and_then(|v| v.as_integer()), Some(7));
    assert_eq!(
        md.get("MXF:TrackNumber").and_then(|v| v.as_integer()),
        Some(3)
    );
    assert_eq!(
        md.get("MXF:TrackName").and_then(|v| v.as_string()),
        Some("Video")
    );
}
