//! Coverage tests for video parsers: MKV/WebM (EBML) and ASF (GUID objects).
//!
//! These tests build synthetic byte buffers that are valid enough to drive the
//! element/object walkers and tag extraction code deeply, plus malformed inputs
//! that exercise error paths. They also drive the production `read_metadata`
//! path on tempfiles for detection + dispatch coverage.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;
use oxidex::parsers::video::asf::{AsfParser, parse_asf_metadata};
use oxidex::parsers::video::mkv::{MkvParser, parse_mkv_metadata};
use oxidex::parsers::video::webm::{WebmParser, parse_webm_metadata};
use std::io::Write;
use tempfile::NamedTempFile;

// ============================================================================
// EBML encoding helpers (shared between MKV and WebM)
// ============================================================================

/// Encode an EBML size VINT. For sizes < 127 uses a 1-byte form (0x80 | size).
/// For larger sizes uses a 2-byte form. This matches the parser's read_vint.
fn ebml_size(size: u64) -> Vec<u8> {
    if size < 0x7F {
        vec![0x80 | size as u8]
    } else if size < 0x3FFF {
        vec![0x40 | ((size >> 8) as u8), (size & 0xFF) as u8]
    } else {
        // 3-byte size form
        vec![
            0x20 | ((size >> 16) as u8),
            ((size >> 8) & 0xFF) as u8,
            (size & 0xFF) as u8,
        ]
    }
}

/// Encode an element ID as its raw bytes (marker bits preserved).
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

/// Build a full EBML element: ID + size VINT + payload.
fn ebml_elem(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = id_bytes(id);
    out.extend(ebml_size(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

/// Build an element whose payload is a big-endian unsigned integer.
fn ebml_uint(id: u32, value: u64, len: usize) -> Vec<u8> {
    let mut payload = Vec::new();
    for i in (0..len).rev() {
        payload.push(((value >> (i * 8)) & 0xFF) as u8);
    }
    ebml_elem(id, &payload)
}

/// Build an element whose payload is a UTF-8 string.
fn ebml_str(id: u32, s: &str) -> Vec<u8> {
    ebml_elem(id, s.as_bytes())
}

/// Build an element whose payload is a big-endian f32.
fn ebml_f32(id: u32, value: f32) -> Vec<u8> {
    ebml_elem(id, &value.to_be_bytes())
}

/// Build an element whose payload is a big-endian f64.
fn ebml_f64(id: u32, value: f64) -> Vec<u8> {
    ebml_elem(id, &value.to_be_bytes())
}

// EBML / Matroska element IDs reused by tests.
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
const TRACK_LANGUAGE: u32 = 0x22B59C;

const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const DISPLAY_WIDTH: u32 = 0x54B0;
const DISPLAY_HEIGHT: u32 = 0x54BA;
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

// The WebM parser uses a different (smaller) set of element-ID constants than
// the MKV parser. Notably its TRACK_TYPE is 0xD7 (which is TRACK_NUMBER in the
// MKV/Matroska spec). These constants mirror src/parsers/video/webm.rs exactly.
const WEBM_TRACK_TYPE: u32 = 0xD7;
const WEBM_CODEC_ID: u32 = 0x86;
const WEBM_VIDEO: u32 = 0xE0;
const WEBM_AUDIO: u32 = 0xE1;
const WEBM_PIXEL_WIDTH: u32 = 0xB0;
const WEBM_PIXEL_HEIGHT: u32 = 0xBA;
const WEBM_FRAME_RATE: u32 = 0x2383E3;
const WEBM_SAMPLING_FREQUENCY: u32 = 0xB5;
const WEBM_CHANNELS: u32 = 0x9F;

/// Build a minimal valid EBML header element with given doctype.
fn ebml_header(doc_type: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(ebml_uint(EBML_VERSION, 1, 1));
    body.extend(ebml_str(EBML_DOC_TYPE, doc_type));
    body.extend(ebml_uint(EBML_DOC_TYPE_VERSION, 2, 1));
    body.extend(ebml_uint(EBML_DOC_TYPE_READ_VERSION, 2, 1));
    ebml_elem(EBML_HEADER, &body)
}

// ============================================================================
// MKV tests
// ============================================================================

#[test]
fn test_mkv_header_only() {
    let data = ebml_header("matroska");
    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("header-only mkv should parse");
    assert_eq!(
        md.get("Matroska:DocType"),
        Some(&TagValue::String("matroska".to_string()))
    );
    assert!(md.contains_key("Matroska:DocTypeVersion"));
    assert!(md.contains_key("Matroska:DocTypeReadVersion"));
}

#[test]
fn test_mkv_info_segment_full() {
    let mut info = Vec::new();
    info.extend(ebml_uint(TIMECODE_SCALE, 1_000_000, 4));
    // Duration as f64 in timecode units: 10000 units * 1ms = 10s
    info.extend(ebml_f64(DURATION, 10000.0));
    // DateUTC: nanoseconds since 2001-01-01; use a small positive value
    info.extend(ebml_uint(DATE_UTC, 1_000_000_000, 8));
    info.extend(ebml_str(TITLE, "My Movie"));
    info.extend(ebml_str(MUXING_APP, "libmkv"));
    info.extend(ebml_str(WRITING_APP, "OxiDex"));

    let mut segment = Vec::new();
    segment.extend(ebml_elem(INFO, &info));

    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("mkv info should parse");

    assert_eq!(
        md.get("Matroska:Title"),
        Some(&TagValue::String("My Movie".to_string()))
    );
    assert_eq!(
        md.get("Matroska:MuxingApp"),
        Some(&TagValue::String("libmkv".to_string()))
    );
    assert_eq!(
        md.get("Matroska:WritingApp"),
        Some(&TagValue::String("OxiDex".to_string()))
    );
    assert!(md.contains_key("Matroska:Duration"));
    assert!(md.contains_key("MKV:Duration"));
    assert!(md.contains_key("Matroska:TimecodeScale"));
    assert!(md.contains_key("Matroska:DateTimeOriginal"));
}

#[test]
fn test_mkv_info_duration_f32() {
    // Use a 4-byte float duration to drive read_float f32 branch.
    let mut info = Vec::new();
    info.extend(ebml_uint(TIMECODE_SCALE, 1_000_000, 8));
    info.extend(ebml_f32(DURATION, 5000.0));

    let segment = ebml_elem(INFO, &info);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("mkv f32 duration should parse");
    assert!(md.contains_key("MKV:Duration"));
}

#[test]
fn test_mkv_video_track_full() {
    // Video element with pixel + display dims, frame rate, interlace.
    let mut video = Vec::new();
    video.extend(ebml_uint(PIXEL_WIDTH, 1920, 2));
    video.extend(ebml_uint(PIXEL_HEIGHT, 1080, 2));
    video.extend(ebml_uint(DISPLAY_WIDTH, 1920, 2));
    video.extend(ebml_uint(DISPLAY_HEIGHT, 1080, 2));
    video.extend(ebml_uint(FLAG_INTERLACED, 2, 1));
    video.extend(ebml_f32(FRAME_RATE, 23.976));

    let mut track = Vec::new();
    track.extend(ebml_uint(TRACK_TYPE, 1, 1)); // video
    track.extend(ebml_uint(TRACK_NUMBER, 1, 1));
    track.extend(ebml_uint(TRACK_UID, 0xDEADBEEF, 4));
    track.extend(ebml_str(CODEC_ID, "V_MPEG4/ISO/AVC"));
    track.extend(ebml_str(TRACK_LANGUAGE, "eng"));
    track.extend(ebml_uint(FLAG_DEFAULT, 1, 1));
    track.extend(ebml_uint(FLAG_ENABLED, 1, 1));
    track.extend(ebml_uint(FLAG_FORCED, 0, 1));
    track.extend(ebml_uint(DEFAULT_DURATION, 41_708_333, 4));
    track.extend(ebml_f32(TRACK_TIMECODE_SCALE, 1.0));
    track.extend(ebml_uint(CODEC_DECODE_ALL, 1, 1));
    track.extend(ebml_elem(VIDEO, &video));

    let tracks = ebml_elem(TRACK_ENTRY, &track);
    let segment = ebml_elem(TRACKS, &tracks);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("mkv video track should parse");

    assert_eq!(
        md.get("Matroska:ImageWidth"),
        Some(&TagValue::Integer(1920))
    );
    assert_eq!(
        md.get("Matroska:ImageHeight"),
        Some(&TagValue::Integer(1080))
    );
    assert_eq!(md.get("MKV:Width"), Some(&TagValue::Integer(1920)));
    assert_eq!(md.get("MKV:Height"), Some(&TagValue::Integer(1080)));
    assert_eq!(
        md.get("Matroska:TrackType"),
        Some(&TagValue::String("Video".to_string()))
    );
    assert_eq!(
        md.get("MKV:VideoCodec"),
        Some(&TagValue::String("H.264".to_string()))
    );
    assert_eq!(
        md.get("Matroska:TrackLanguage"),
        Some(&TagValue::String("eng".to_string()))
    );
    assert!(md.contains_key("Matroska:VideoScanType"));
    assert!(md.contains_key("MKV:FrameRate"));
    assert!(md.contains_key("Matroska:TrackUID"));
}

#[test]
fn test_mkv_video_track_no_framerate_uses_default_duration() {
    // No FRAME_RATE element; default_duration drives the derived fps branch.
    let mut video = Vec::new();
    video.extend(ebml_uint(PIXEL_WIDTH, 640, 2));
    video.extend(ebml_uint(PIXEL_HEIGHT, 480, 2));

    let mut track = Vec::new();
    track.extend(ebml_uint(TRACK_TYPE, 1, 1));
    track.extend(ebml_str(CODEC_ID, "V_VP9"));
    track.extend(ebml_uint(DEFAULT_DURATION, 33_333_333, 4));
    track.extend(ebml_elem(VIDEO, &video));

    let tracks = ebml_elem(TRACK_ENTRY, &track);
    let segment = ebml_elem(TRACKS, &tracks);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("derived fps should parse");
    assert!(md.contains_key("Matroska:VideoFrameRate"));
    assert!(md.contains_key("MKV:FrameRate"));
    // display dims fall back to pixel dims
    assert_eq!(
        md.get("Matroska:DisplayWidth"),
        Some(&TagValue::Integer(640))
    );
    assert_eq!(
        md.get("Matroska:DisplayHeight"),
        Some(&TagValue::Integer(480))
    );
    assert!(md.contains_key("Matroska:DefaultDuration"));
}

#[test]
fn test_mkv_audio_track_full() {
    let mut audio = Vec::new();
    audio.extend(ebml_f32(SAMPLING_FREQUENCY, 48000.0));
    audio.extend(ebml_uint(CHANNELS, 2, 1));
    audio.extend(ebml_uint(BIT_DEPTH, 16, 1));

    let mut track = Vec::new();
    track.extend(ebml_uint(TRACK_TYPE, 2, 1)); // audio
    track.extend(ebml_str(CODEC_ID, "A_OPUS"));
    track.extend(ebml_f32(TRACK_TIMECODE_SCALE, 2.0)); // non-1.0 branch
    track.extend(ebml_elem(AUDIO, &audio));

    let tracks = ebml_elem(TRACK_ENTRY, &track);
    let segment = ebml_elem(TRACKS, &tracks);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("mkv audio track should parse");

    assert_eq!(
        md.get("Matroska:AudioSampleRate"),
        Some(&TagValue::Integer(48000))
    );
    assert_eq!(md.get("MKV:SampleRate"), Some(&TagValue::Integer(48000)));
    assert_eq!(
        md.get("Matroska:AudioChannels"),
        Some(&TagValue::Integer(2))
    );
    assert_eq!(md.get("MKV:Channels"), Some(&TagValue::Integer(2)));
    assert_eq!(
        md.get("Matroska:AudioBitsPerSample"),
        Some(&TagValue::Integer(16))
    );
    assert_eq!(
        md.get("Matroska:TrackType"),
        Some(&TagValue::String("Audio".to_string()))
    );
    assert_eq!(
        md.get("MKV:AudioCodec"),
        Some(&TagValue::String("Opus".to_string()))
    );
    // non-1.0 timecode scale branch
    assert!(md.contains_key("Matroska:TrackTimecodeScale"));
}

#[test]
fn test_mkv_subtitle_track_type_mapping() {
    // Exercise the subtitle/unknown track type string mapping + flags off.
    let mut track = Vec::new();
    track.extend(ebml_uint(TRACK_TYPE, 17, 1)); // Subtitle
    track.extend(ebml_uint(FLAG_DEFAULT, 0, 1));
    track.extend(ebml_uint(FLAG_ENABLED, 0, 1));
    track.extend(ebml_uint(FLAG_FORCED, 1, 1));
    track.extend(ebml_uint(CODEC_DECODE_ALL, 0, 1));
    track.extend(ebml_str(CODEC_ID, "S_TEXT/UTF8"));

    let tracks = ebml_elem(TRACK_ENTRY, &track);
    let segment = ebml_elem(TRACKS, &tracks);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("subtitle track should parse");
    assert_eq!(
        md.get("Matroska:TrackType"),
        Some(&TagValue::String("Subtitle".to_string()))
    );
    assert_eq!(
        md.get("Matroska:TrackDefault"),
        Some(&TagValue::String("No".to_string()))
    );
    assert_eq!(
        md.get("Matroska:TrackForced"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("Matroska:CodecDecodeAll"),
        Some(&TagValue::String("No".to_string()))
    );
}

#[test]
fn test_mkv_multiple_tracks() {
    // Two track entries in one Tracks element.
    let mut video = Vec::new();
    video.extend(ebml_uint(PIXEL_WIDTH, 1280, 2));
    video.extend(ebml_uint(PIXEL_HEIGHT, 720, 2));
    let mut vtrack = Vec::new();
    vtrack.extend(ebml_uint(TRACK_TYPE, 1, 1));
    vtrack.extend(ebml_str(CODEC_ID, "V_AV1"));
    vtrack.extend(ebml_elem(VIDEO, &video));

    let mut audio = Vec::new();
    audio.extend(ebml_f32(SAMPLING_FREQUENCY, 44100.0));
    audio.extend(ebml_uint(CHANNELS, 1, 1));
    let mut atrack = Vec::new();
    atrack.extend(ebml_uint(TRACK_TYPE, 2, 1));
    atrack.extend(ebml_str(CODEC_ID, "A_VORBIS"));
    atrack.extend(ebml_elem(AUDIO, &audio));

    let mut tracks = Vec::new();
    tracks.extend(ebml_elem(TRACK_ENTRY, &vtrack));
    tracks.extend(ebml_elem(TRACK_ENTRY, &atrack));
    let segment = ebml_elem(TRACKS, &tracks);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("multi-track should parse");
    assert!(md.contains_key("Matroska:ImageWidth"));
    assert!(md.contains_key("Matroska:AudioSampleRate"));
}

#[test]
fn test_mkv_tags_segment() {
    // Tags -> Tag -> SimpleTag(TagName, TagString)
    let mut simple1 = Vec::new();
    simple1.extend(ebml_str(TAG_NAME, "ARTIST"));
    simple1.extend(ebml_str(TAG_STRING, "Some Artist"));
    let mut simple2 = Vec::new();
    simple2.extend(ebml_str(TAG_NAME, "ALBUM"));
    simple2.extend(ebml_str(TAG_STRING, "Some Album"));

    let mut tag = Vec::new();
    tag.extend(ebml_elem(SIMPLE_TAG, &simple1));
    tag.extend(ebml_elem(SIMPLE_TAG, &simple2));

    let tags = ebml_elem(TAG, &tag);
    let segment = ebml_elem(TAGS, &tags);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("tags should parse");
    assert_eq!(
        md.get("Matroska:Tag:ARTIST"),
        Some(&TagValue::String("Some Artist".to_string()))
    );
    assert_eq!(
        md.get("Matroska:Tag:ALBUM"),
        Some(&TagValue::String("Some Album".to_string()))
    );
}

#[test]
fn test_mkv_chapters_segment() {
    // Chapters -> EditionEntry -> ChapterAtom(time start/end, display->string)
    let mut display = Vec::new();
    display.extend(ebml_str(CHAP_STRING, "Intro"));

    let mut atom = Vec::new();
    atom.extend(ebml_uint(CHAPTER_TIME_START, 0, 4));
    atom.extend(ebml_uint(CHAPTER_TIME_END, 60_000_000_000, 8));
    atom.extend(ebml_elem(CHAPTER_DISPLAY, &display));

    let edition = ebml_elem(CHAPTER_ATOM, &atom);
    let chapters = ebml_elem(EDITION_ENTRY, &edition);
    let segment = ebml_elem(CHAPTERS, &chapters);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("chapters should parse");
    assert_eq!(md.get("Matroska:ChapterCount"), Some(&TagValue::Integer(1)));
    assert_eq!(
        md.get("Matroska:Chapter1:Title"),
        Some(&TagValue::String("Intro".to_string()))
    );
    assert!(md.contains_key("Matroska:Chapter1:TimeStart"));
    assert!(md.contains_key("Matroska:Chapter1:TimeEnd"));
}

#[test]
fn test_mkv_attachments_segment() {
    let mut att = Vec::new();
    att.extend(ebml_str(FILE_NAME, "cover.jpg"));
    att.extend(ebml_str(FILE_MIME_TYPE, "image/jpeg"));
    att.extend(ebml_str(FILE_DESCRIPTION, "Cover art"));

    let attached = ebml_elem(ATTACHED_FILE, &att);
    // Segment body holds a single Attachments element containing one AttachedFile.
    let segment_body = ebml_elem(ATTACHMENTS, &attached);

    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment_body));

    let reader = TestReader::new(data);
    let md = parse_mkv_metadata(&reader).expect("attachments should parse");
    assert_eq!(
        md.get("Matroska:AttachmentCount"),
        Some(&TagValue::Integer(1))
    );
    assert_eq!(
        md.get("Matroska:Attachment1:FileName"),
        Some(&TagValue::String("cover.jpg".to_string()))
    );
    assert_eq!(
        md.get("Matroska:Attachment1:MIMEType"),
        Some(&TagValue::String("image/jpeg".to_string()))
    );
    assert_eq!(
        md.get("Matroska:Attachment1:Description"),
        Some(&TagValue::String("Cover art".to_string()))
    );
}

#[test]
fn test_mkv_codec_name_mappings() {
    // Drive a variety of codec IDs through convert_codec_id_to_name via tracks.
    for (codec, expect, ttype) in [
        ("V_MPEGH/ISO/HEVC", "H.265", 1u64),
        ("V_VP8", "VP8", 1),
        ("V_THEORA", "Theora", 1),
        ("V_UNKNOWN_CODEC", "V_UNKNOWN_CODEC", 1),
        ("A_AAC/MPEG4/LC", "AAC-LC", 2),
        ("A_FLAC", "FLAC", 2),
        ("A_AC3", "AC-3", 2),
        ("A_UNKNOWN_CODEC", "A_UNKNOWN_CODEC", 2),
    ] {
        let mut track = Vec::new();
        track.extend(ebml_uint(TRACK_TYPE, ttype, 1));
        track.extend(ebml_str(CODEC_ID, codec));
        let tracks = ebml_elem(TRACK_ENTRY, &track);
        let segment = ebml_elem(TRACKS, &tracks);
        let mut data = ebml_header("matroska");
        data.extend(ebml_elem(SEGMENT, &segment));

        let reader = TestReader::new(data);
        let md = parse_mkv_metadata(&reader).expect("codec mapping should parse");
        let key = if ttype == 1 {
            "MKV:VideoCodec"
        } else {
            "MKV:AudioCodec"
        };
        assert_eq!(
            md.get(key),
            Some(&TagValue::String(expect.to_string())),
            "codec {codec} should map to {expect}"
        );
    }
}

#[test]
fn test_mkv_invalid_signature() {
    let reader = TestReader::new(b"NOT-AN-MKV-FILE".to_vec());
    assert!(parse_mkv_metadata(&reader).is_err());
}

#[test]
fn test_mkv_too_small() {
    let reader = TestReader::new(vec![0x1A, 0x45]);
    assert!(parse_mkv_metadata(&reader).is_err());
}

#[test]
fn test_mkv_wrong_first_element() {
    // Correct EBML signature but the first element id is not EBML_HEADER.
    let mut data = Vec::new();
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]); // signature bytes match
    // But make this a SEGMENT id collision? No—signature check passes, then
    // parse_ebml_header reads id 0x1A45DFA3 which IS the header. To hit the
    // "missing EBML header" branch we corrupt after signature: keep first 4
    // bytes but then provide a tiny size and bogus inner ids.
    data.extend(ebml_size(2));
    data.extend_from_slice(&[0xFF, 0xFF]);
    let reader = TestReader::new(data);
    // Should still be Ok (header parses, inner unknown ids skipped) or error;
    // either way we exercise the code without panicking.
    let _ = parse_mkv_metadata(&reader);
}

#[test]
fn test_mkv_parser_supports_format_and_struct() {
    use oxidex::core::FileFormat;
    use oxidex::core::FormatParser;
    let parser = MkvParser;
    assert!(parser.supports_format(FileFormat::MKV));
    assert!(!parser.supports_format(FileFormat::WEBM));
    let data = ebml_header("matroska");
    let reader = TestReader::new(data);
    assert!(parser.parse(&reader).is_ok());
}

#[test]
fn test_mkv_production_path() {
    // Drive read_metadata on a real tempfile with .mkv extension.
    let mut info = Vec::new();
    info.extend(ebml_uint(TIMECODE_SCALE, 1_000_000, 4));
    info.extend(ebml_str(TITLE, "ProdTitle"));
    let segment = ebml_elem(INFO, &info);
    let mut data = ebml_header("matroska");
    data.extend(ebml_elem(SEGMENT, &segment));

    let tmp = NamedTempFile::with_suffix(".mkv").expect("tempfile");
    {
        let mut f = tmp.reopen().expect("reopen");
        f.write_all(&data).expect("write");
        f.flush().expect("flush");
    }
    let md = read_metadata(tmp.path()).expect("read_metadata mkv");
    assert_eq!(
        md.get("Matroska:Title"),
        Some(&TagValue::String("ProdTitle".to_string()))
    );
}

// ============================================================================
// WebM tests
// ============================================================================

#[test]
fn test_webm_header_only() {
    let data = ebml_header("webm");
    let reader = TestReader::new(data);
    let md = parse_webm_metadata(&reader).expect("webm header should parse");
    assert_eq!(
        md.get("WebM:DocType"),
        Some(&TagValue::String("webm".to_string()))
    );
}

#[test]
fn test_webm_wrong_doctype_errors() {
    // DocType "matroska" should be rejected by the WebM parser.
    let data = ebml_header("matroska");
    let reader = TestReader::new(data);
    assert!(parse_webm_metadata(&reader).is_err());
}

#[test]
fn test_webm_info_and_tracks() {
    // Header must be exactly 12 bytes so the WebM parser's hardcoded
    // segment-children scan starting at offset 12 lands on the first child
    // element. Unlike MKV, the WebM parser treats offset-12 content as the
    // Segment's direct children (INFO/TRACKS), not wrapped in a SEGMENT box.
    let mut header_body = Vec::new();
    header_body.extend(ebml_str(EBML_DOC_TYPE, "webm")); // 2 id +1 size +4 = 7 bytes
    let header = ebml_elem(EBML_HEADER, &header_body);
    assert_eq!(header.len(), 12, "webm header must be 12 bytes");

    // Info: timecode scale + duration
    let mut info = Vec::new();
    info.extend(ebml_uint(TIMECODE_SCALE, 1_000_000, 4));
    info.extend(ebml_f64(DURATION, 8000.0));

    // Video track
    let mut video = Vec::new();
    video.extend(ebml_uint(WEBM_PIXEL_WIDTH, 1920, 2));
    video.extend(ebml_uint(WEBM_PIXEL_HEIGHT, 1080, 2));
    video.extend(ebml_f32(WEBM_FRAME_RATE, 30.0));
    let mut vtrack = Vec::new();
    vtrack.extend(ebml_uint(WEBM_TRACK_TYPE, 1, 1));
    vtrack.extend(ebml_str(WEBM_CODEC_ID, "V_VP9"));
    vtrack.extend(ebml_elem(WEBM_VIDEO, &video));

    // Audio track
    let mut audio = Vec::new();
    audio.extend(ebml_f32(WEBM_SAMPLING_FREQUENCY, 48000.0));
    audio.extend(ebml_uint(WEBM_CHANNELS, 2, 1));
    let mut atrack = Vec::new();
    atrack.extend(ebml_uint(WEBM_TRACK_TYPE, 2, 1));
    atrack.extend(ebml_str(WEBM_CODEC_ID, "A_OPUS"));
    atrack.extend(ebml_elem(WEBM_AUDIO, &audio));

    let mut tracks_body = Vec::new();
    tracks_body.extend(ebml_elem(TRACK_ENTRY, &vtrack));
    tracks_body.extend(ebml_elem(TRACK_ENTRY, &atrack));

    // INFO and TRACKS placed directly after the 12-byte header (no SEGMENT box).
    let mut data = header;
    data.extend(ebml_elem(INFO, &info));
    data.extend(ebml_elem(TRACKS, &tracks_body));

    let reader = TestReader::new(data);
    let md = parse_webm_metadata(&reader).expect("webm info+tracks should parse");

    assert_eq!(md.get("WEBM:Width"), Some(&TagValue::Integer(1920)));
    assert_eq!(md.get("WEBM:Height"), Some(&TagValue::Integer(1080)));
    assert_eq!(
        md.get("WEBM:VideoCodec"),
        Some(&TagValue::String("VP9".to_string()))
    );
    assert_eq!(
        md.get("WEBM:AudioCodec"),
        Some(&TagValue::String("Opus".to_string()))
    );
    assert_eq!(md.get("WEBM:SampleRate"), Some(&TagValue::Integer(48000)));
    assert_eq!(md.get("WEBM:Channels"), Some(&TagValue::Integer(2)));
    assert!(md.contains_key("WEBM:FrameRate"));
    assert!(md.contains_key("WEBM:Duration"));
}

#[test]
fn test_webm_unknown_codecs() {
    // Header built to exactly 12 bytes (see test_webm_info_and_tracks).
    let mut header_body = Vec::new();
    header_body.extend(ebml_str(EBML_DOC_TYPE, "webm"));
    let header = ebml_elem(EBML_HEADER, &header_body);
    assert_eq!(header.len(), 12, "webm header must be 12 bytes");

    let mut vtrack = Vec::new();
    vtrack.extend(ebml_uint(WEBM_TRACK_TYPE, 1, 1));
    vtrack.extend(ebml_str(WEBM_CODEC_ID, "V_SOMETHING"));
    let mut atrack = Vec::new();
    atrack.extend(ebml_uint(WEBM_TRACK_TYPE, 2, 1));
    atrack.extend(ebml_str(WEBM_CODEC_ID, "A_SOMETHING"));

    let mut tracks_body = Vec::new();
    tracks_body.extend(ebml_elem(TRACK_ENTRY, &vtrack));
    tracks_body.extend(ebml_elem(TRACK_ENTRY, &atrack));

    // TRACKS placed directly after the 12-byte header (no SEGMENT box).
    let mut data = header;
    data.extend(ebml_elem(TRACKS, &tracks_body));
    let reader = TestReader::new(data);
    let md = parse_webm_metadata(&reader).expect("unknown codecs should parse");
    assert_eq!(
        md.get("WEBM:VideoCodec"),
        Some(&TagValue::String("V_SOMETHING".to_string()))
    );
    assert_eq!(
        md.get("WEBM:AudioCodec"),
        Some(&TagValue::String("A_SOMETHING".to_string()))
    );
}

#[test]
fn test_webm_invalid_signature() {
    let reader = TestReader::new(b"not webm at all".to_vec());
    assert!(parse_webm_metadata(&reader).is_err());
}

#[test]
fn test_webm_too_small() {
    let reader = TestReader::new(vec![0x1A, 0x45, 0xDF]);
    assert!(parse_webm_metadata(&reader).is_err());
}

#[test]
fn test_webm_parser_supports_format() {
    use oxidex::core::FileFormat;
    use oxidex::core::FormatParser;
    let parser = WebmParser;
    assert!(parser.supports_format(FileFormat::WEBM));
    assert!(!parser.supports_format(FileFormat::MKV));
    let data = ebml_header("webm");
    let reader = TestReader::new(data);
    assert!(parser.parse(&reader).is_ok());
}

// ============================================================================
// ASF helpers and tests
// ============================================================================

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
const EXTENDED_CONTENT_GUID: [u8; 16] = [
    0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0xF0, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x50,
];
const CODEC_LIST_GUID: [u8; 16] = [
    0x40, 0x52, 0xD1, 0x86, 0x1D, 0x31, 0xD0, 0x11, 0xA3, 0xA4, 0x00, 0xA0, 0xC9, 0x03, 0x48, 0xF6,
];
const HEADER_EXTENSION_GUID: [u8; 16] = [
    0xB5, 0x03, 0xBF, 0x5F, 0x2E, 0xA9, 0xCF, 0x11, 0x8E, 0xE3, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const METADATA_GUID: [u8; 16] = [
    0xEA, 0xCB, 0xF8, 0xC5, 0xAF, 0x5B, 0x77, 0x48, 0x84, 0x67, 0xAA, 0x8C, 0x44, 0xFA, 0x4C, 0xCA,
];
const AUDIO_MEDIA_GUID: [u8; 16] = [
    0x40, 0x9E, 0x69, 0xF8, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];
const VIDEO_MEDIA_GUID: [u8; 16] = [
    0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];

/// Build an ASF object: 16-byte GUID + 8-byte LE size + body.
/// The size field counts the entire object including the 24-byte header.
fn asf_object(guid: &[u8; 16], body: &[u8]) -> Vec<u8> {
    let total = 24 + body.len() as u64;
    let mut out = Vec::new();
    out.extend_from_slice(guid);
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// Wrap a set of header sub-objects into a full ASF header object with the
/// 30-byte header (16 GUID + 8 size + 4 object count + 2 reserved).
fn asf_file(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut inner = Vec::new();
    for o in objects {
        inner.extend_from_slice(o);
    }
    let total_size = 30 + inner.len() as u64;
    let mut out = Vec::new();
    out.extend_from_slice(&ASF_HEADER_GUID);
    out.extend_from_slice(&total_size.to_le_bytes()); // header object size
    out.extend_from_slice(&(objects.len() as u32).to_le_bytes()); // num objects
    out.push(0x01); // reserved1
    out.push(0x02); // reserved2
    out.extend_from_slice(&inner);
    out
}

/// Encode a UTF-16LE string with a trailing null terminator.
fn utf16(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]); // null terminator
    out
}

#[test]
fn test_asf_too_small() {
    let reader = TestReader::new(vec![0u8; 10]);
    assert!(parse_asf_metadata(&reader).is_err());
}

#[test]
fn test_asf_invalid_guid() {
    let mut data = vec![0u8; 40];
    // wrong leading GUID
    data[0] = 0xFF;
    let reader = TestReader::new(data);
    assert!(parse_asf_metadata(&reader).is_err());
}

#[test]
fn test_asf_empty_header() {
    let data = asf_file(&[]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("empty asf header should parse");
    assert!(md.is_empty() || md.len() == 0);
}

#[test]
fn test_asf_file_properties() {
    // File Properties Object body is 80 bytes (object total >= 104).
    let mut body = vec![0u8; 80];
    // File ID GUID is bytes 0..16 (leave as zeros -> formatted)
    // File size (u64 LE) at body offset 16
    body[16..24].copy_from_slice(&123_456_789u64.to_le_bytes());
    // Creation time (FILETIME) at offset 24 - use a value past the unix epoch
    let filetime = 116_444_736_000_000_000u64 + 10_000_000 * 1_700_000_000u64;
    body[24..32].copy_from_slice(&filetime.to_le_bytes());
    // Data packets at offset 32
    body[32..40].copy_from_slice(&42u64.to_le_bytes());
    // Play duration at offset 40 (100ns units) = 100 seconds
    body[40..48].copy_from_slice(&(100u64 * 10_000_000).to_le_bytes());
    // Send duration at offset 48 = 90 seconds
    body[48..56].copy_from_slice(&(90u64 * 10_000_000).to_le_bytes());
    // Preroll at offset 56
    body[56..64].copy_from_slice(&1000u64.to_le_bytes());
    // Flags at offset 64
    body[64..68].copy_from_slice(&2u32.to_le_bytes());
    // Min packet size at offset 68
    body[68..72].copy_from_slice(&8192u32.to_le_bytes());
    // Max packet size at offset 72
    body[72..76].copy_from_slice(&8192u32.to_le_bytes());
    // Max bitrate at offset 76
    body[76..80].copy_from_slice(&128_000u32.to_le_bytes());

    let obj = asf_object(&FILE_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("file properties should parse");

    assert_eq!(
        md.get("ASF:FileLength"),
        Some(&TagValue::Integer(123_456_789))
    );
    assert_eq!(md.get("ASF:DataPackets"), Some(&TagValue::Integer(42)));
    assert_eq!(md.get("ASF:Preroll"), Some(&TagValue::Integer(1000)));
    assert_eq!(md.get("ASF:Flags"), Some(&TagValue::Integer(2)));
    assert!(md.contains_key("ASF:FileID"));
    assert!(md.contains_key("ASF:CreationDate"));
    assert!(md.contains_key("ASF:Duration"));
    assert!(md.contains_key("ASF:SendDuration"));
    assert!(md.contains_key("ASF:MaxBitrate"));
    assert!(md.contains_key("ASF:MinPacketSize"));
    assert!(md.contains_key("ASF:MaxPacketSize"));
}

#[test]
fn test_asf_file_properties_too_small_ignored() {
    // size < 104 -> parser returns early without inserting tags.
    let body = vec![0u8; 40];
    let obj = asf_object(&FILE_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("small file props ignored");
    assert!(!md.contains_key("ASF:FileLength"));
}

#[test]
fn test_asf_stream_properties_audio() {
    // Stream Properties body: 54 bytes of header + type-specific data.
    // type-specific data (WAVEFORMATEX) is 18 bytes.
    let type_data_len = 18usize;
    let mut body = vec![0u8; 54 + type_data_len];
    // stream type GUID at 0..16 -> Audio
    body[0..16].copy_from_slice(&AUDIO_MEDIA_GUID);
    // error correction GUID at 16..32 left as zeros -> "No Error Correction"
    // time offset (u64) at 32
    body[32..40].copy_from_slice(&(10u64 * 10_000_000).to_le_bytes());
    // type-specific data length at 40
    body[40..44].copy_from_slice(&(type_data_len as u32).to_le_bytes());
    // error data length at 44
    body[44..48].copy_from_slice(&0u32.to_le_bytes());
    // flags at 48 (stream number = 1)
    body[48..50].copy_from_slice(&1u16.to_le_bytes());
    // reserved 4 bytes at 50..54
    // WAVEFORMATEX type-specific data starts at body offset 54
    let ts = &mut body[54..];
    ts[0..2].copy_from_slice(&0x0161u16.to_le_bytes()); // format tag
    ts[2..4].copy_from_slice(&2u16.to_le_bytes()); // channels
    ts[4..8].copy_from_slice(&44100u32.to_le_bytes()); // sample rate

    let obj = asf_object(&STREAM_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("audio stream props should parse");

    assert_eq!(
        md.get("ASF:StreamType"),
        Some(&TagValue::String("Audio".to_string()))
    );
    assert_eq!(md.get("ASF:StreamNumber"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("ASF:AudioChannels"), Some(&TagValue::Integer(2)));
    assert_eq!(
        md.get("ASF:AudioSampleRate"),
        Some(&TagValue::Integer(44100))
    );
    assert_eq!(
        md.get("ASF:ErrorCorrectionType"),
        Some(&TagValue::String("No Error Correction".to_string()))
    );
    assert!(md.contains_key("ASF:TimeOffset"));
}

#[test]
fn test_asf_stream_properties_video() {
    let type_data_len = 11usize;
    let mut body = vec![0u8; 54 + type_data_len];
    body[0..16].copy_from_slice(&VIDEO_MEDIA_GUID);
    body[32..40].copy_from_slice(&0u64.to_le_bytes());
    body[40..44].copy_from_slice(&(type_data_len as u32).to_le_bytes());
    body[48..50].copy_from_slice(&2u16.to_le_bytes()); // stream number 2
    // video type-specific data: width @0, height @4
    let ts = &mut body[54..];
    ts[0..4].copy_from_slice(&1280u32.to_le_bytes());
    ts[4..8].copy_from_slice(&720u32.to_le_bytes());

    let obj = asf_object(&STREAM_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("video stream props should parse");
    assert_eq!(
        md.get("ASF:StreamType"),
        Some(&TagValue::String("Video".to_string()))
    );
    assert_eq!(md.get("ASF:ImageWidth"), Some(&TagValue::Integer(1280)));
    assert_eq!(md.get("ASF:ImageHeight"), Some(&TagValue::Integer(720)));
}

#[test]
fn test_asf_stream_properties_too_small() {
    let body = vec![0u8; 40];
    let obj = asf_object(&STREAM_PROPERTIES_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("small stream props ignored");
    assert!(!md.contains_key("ASF:StreamType"));
}

#[test]
fn test_asf_content_description() {
    let title = utf16("Track Title");
    let author = utf16("The Author");
    let copyright = utf16("(c) 2024");
    let description = utf16("A description");
    let rating = utf16("5");

    let mut body = Vec::new();
    // 10-byte length header (5 x u16 LE)
    body.extend_from_slice(&(title.len() as u16).to_le_bytes());
    body.extend_from_slice(&(author.len() as u16).to_le_bytes());
    body.extend_from_slice(&(copyright.len() as u16).to_le_bytes());
    body.extend_from_slice(&(description.len() as u16).to_le_bytes());
    body.extend_from_slice(&(rating.len() as u16).to_le_bytes());
    body.extend_from_slice(&title);
    body.extend_from_slice(&author);
    body.extend_from_slice(&copyright);
    body.extend_from_slice(&description);
    body.extend_from_slice(&rating);

    let obj = asf_object(&CONTENT_DESCRIPTION_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("content description should parse");

    assert_eq!(
        md.get("ASF:Title"),
        Some(&TagValue::String("Track Title".to_string()))
    );
    assert_eq!(
        md.get("ASF:Author"),
        Some(&TagValue::String("The Author".to_string()))
    );
    assert_eq!(
        md.get("ASF:Copyright"),
        Some(&TagValue::String("(c) 2024".to_string()))
    );
    assert_eq!(
        md.get("ASF:Description"),
        Some(&TagValue::String("A description".to_string()))
    );
    assert_eq!(
        md.get("ASF:Rating"),
        Some(&TagValue::String("5".to_string()))
    );
}

#[test]
fn test_asf_extended_content_description() {
    // Two descriptors: a Unicode string and a DWORD.
    let name1 = utf16("WM/Publisher");
    let val1 = utf16("Acme Records");
    let name2 = utf16("WM/Genre");
    let val2 = utf16("Rock");
    let name3 = utf16("WM/IsVBR"); // bool
    let val3 = 1u32.to_le_bytes().to_vec();

    let mut body = Vec::new();
    body.extend_from_slice(&3u16.to_le_bytes()); // descriptor count

    // descriptor 1: type 0 (unicode string)
    body.extend_from_slice(&(name1.len() as u16).to_le_bytes());
    body.extend_from_slice(&name1);
    body.extend_from_slice(&0u16.to_le_bytes()); // value type 0
    body.extend_from_slice(&(val1.len() as u16).to_le_bytes());
    body.extend_from_slice(&val1);

    // descriptor 2: type 0 (unicode string)
    body.extend_from_slice(&(name2.len() as u16).to_le_bytes());
    body.extend_from_slice(&name2);
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&(val2.len() as u16).to_le_bytes());
    body.extend_from_slice(&val2);

    // descriptor 3: type 2 (bool)
    body.extend_from_slice(&(name3.len() as u16).to_le_bytes());
    body.extend_from_slice(&name3);
    body.extend_from_slice(&2u16.to_le_bytes());
    body.extend_from_slice(&(val3.len() as u16).to_le_bytes());
    body.extend_from_slice(&val3);

    let obj = asf_object(&EXTENDED_CONTENT_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("extended content should parse");

    assert_eq!(
        md.get("ASF:Publisher"),
        Some(&TagValue::String("Acme Records".to_string()))
    );
    assert_eq!(
        md.get("ASF:Genre"),
        Some(&TagValue::String("Rock".to_string()))
    );
    assert_eq!(
        md.get("ASF:IsVBR"),
        Some(&TagValue::String("true".to_string()))
    );
}

#[test]
fn test_asf_extended_content_numeric_types() {
    // DWORD (type 3), QWORD (type 4), WORD (type 5).
    let name_dw = utf16("WM/Track");
    let val_dw = 7u32.to_le_bytes().to_vec();
    let name_qw = utf16("WM/SomeQWord");
    let val_qw = 9_000_000_000u64.to_le_bytes().to_vec();
    let name_w = utf16("WM/SomeWord");
    let val_w = 250u16.to_le_bytes().to_vec();

    let mut body = Vec::new();
    body.extend_from_slice(&3u16.to_le_bytes());

    for (name, vtype, val) in [
        (&name_dw, 3u16, &val_dw),
        (&name_qw, 4u16, &val_qw),
        (&name_w, 5u16, &val_w),
    ] {
        body.extend_from_slice(&(name.len() as u16).to_le_bytes());
        body.extend_from_slice(name);
        body.extend_from_slice(&vtype.to_le_bytes());
        body.extend_from_slice(&(val.len() as u16).to_le_bytes());
        body.extend_from_slice(val);
    }

    let obj = asf_object(&EXTENDED_CONTENT_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("numeric ext content should parse");
    assert_eq!(md.get("ASF:Track"), Some(&TagValue::Integer(7)));
    assert_eq!(
        md.get("ASF:SomeQWord"),
        Some(&TagValue::Integer(9_000_000_000))
    );
    assert_eq!(md.get("ASF:SomeWord"), Some(&TagValue::Integer(250)));
}

#[test]
fn test_asf_extended_content_picture() {
    // WM/Picture with binary value type 1 -> parse_wm_picture.
    let name = utf16("WM/Picture");

    // Build picture payload: type(1) + size(4 LE) + mime(utf16+null) + desc(utf16+null) + data
    let mut pic = Vec::new();
    pic.push(3u8); // picture type = Front Cover
    pic.extend_from_slice(&100u32.to_le_bytes()); // picture data size
    pic.extend_from_slice(&utf16("image/jpeg")); // mime (includes null terminator)
    pic.extend_from_slice(&utf16("Cover")); // description
    pic.extend_from_slice(&[0xAB; 20]); // some binary data

    let mut body = Vec::new();
    body.extend_from_slice(&1u16.to_le_bytes()); // descriptor count
    body.extend_from_slice(&(name.len() as u16).to_le_bytes());
    body.extend_from_slice(&name);
    body.extend_from_slice(&1u16.to_le_bytes()); // value type 1 (byte array)
    body.extend_from_slice(&(pic.len() as u16).to_le_bytes());
    body.extend_from_slice(&pic);

    let obj = asf_object(&EXTENDED_CONTENT_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("picture ext content should parse");

    assert_eq!(
        md.get("ASF:PictureType"),
        Some(&TagValue::String("Front Cover".to_string()))
    );
    assert_eq!(
        md.get("ASF:PictureMIMEType"),
        Some(&TagValue::String("image/jpeg".to_string()))
    );
    assert!(md.contains_key("ASF:Picture"));
}

#[test]
fn test_asf_codec_list() {
    // Reserved GUID (16) at body 0..16, codec count u32 at body 16..20,
    // then codec entries starting at object offset 44.
    let mut body = Vec::new();
    body.extend_from_slice(&[0u8; 16]); // reserved GUID
    body.extend_from_slice(&2u32.to_le_bytes()); // codec count = 2

    // Entry 1: video codec (type 0x0001)
    body.extend_from_slice(&0x0001u16.to_le_bytes()); // codec type
    let vname: Vec<u16> = "WMV9".encode_utf16().collect();
    body.extend_from_slice(&(vname.len() as u16).to_le_bytes()); // name len (in chars)
    for c in &vname {
        body.extend_from_slice(&c.to_le_bytes());
    }
    let vdesc: Vec<u16> = "Windows Media Video 9".encode_utf16().collect();
    body.extend_from_slice(&(vdesc.len() as u16).to_le_bytes());
    for c in &vdesc {
        body.extend_from_slice(&c.to_le_bytes());
    }
    // codec info: 4-byte FourCC
    body.extend_from_slice(&4u16.to_le_bytes()); // info len in bytes
    body.extend_from_slice(b"WMV3");

    // Entry 2: audio codec (type 0x0002)
    body.extend_from_slice(&0x0002u16.to_le_bytes());
    let aname: Vec<u16> = "WMA9".encode_utf16().collect();
    body.extend_from_slice(&(aname.len() as u16).to_le_bytes());
    for c in &aname {
        body.extend_from_slice(&c.to_le_bytes());
    }
    let adesc: Vec<u16> = "Windows Media Audio 9".encode_utf16().collect();
    body.extend_from_slice(&(adesc.len() as u16).to_le_bytes());
    for c in &adesc {
        body.extend_from_slice(&c.to_le_bytes());
    }
    // codec info: 2-byte format tag (WMA = 0x0161)
    body.extend_from_slice(&2u16.to_le_bytes());
    body.extend_from_slice(&0x0161u16.to_le_bytes());

    let obj = asf_object(&CODEC_LIST_GUID, &body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("codec list should parse");

    assert_eq!(
        md.get("ASF:VideoCodecName"),
        Some(&TagValue::String("WMV9".to_string()))
    );
    assert!(md.contains_key("ASF:VideoCodecDescription"));
    assert_eq!(
        md.get("ASF:AudioCodecName"),
        Some(&TagValue::String("WMA9".to_string()))
    );
    assert!(md.contains_key("ASF:AudioCodecID"));
}

#[test]
fn test_asf_header_extension_with_metadata() {
    // Header Extension Object: 24-byte object header (added by asf_object) +
    // 16-byte reserved GUID + 2-byte reserved + 4-byte data size + nested objects.
    // Build the inner Metadata object first.
    let name = utf16("WM/Publisher");
    let val = utf16("Nested Publisher");

    let mut meta_body = Vec::new();
    meta_body.extend_from_slice(&1u16.to_le_bytes()); // record count
    // record: lang idx(2) + stream(2) + name len(2) + data type(2) + data len(4)
    meta_body.extend_from_slice(&0u16.to_le_bytes()); // lang idx
    meta_body.extend_from_slice(&0u16.to_le_bytes()); // stream
    meta_body.extend_from_slice(&(name.len() as u16).to_le_bytes()); // name len
    meta_body.extend_from_slice(&0u16.to_le_bytes()); // data type 0 (unicode)
    meta_body.extend_from_slice(&(val.len() as u32).to_le_bytes()); // data len
    meta_body.extend_from_slice(&name);
    meta_body.extend_from_slice(&val);

    let metadata_obj = asf_object(&METADATA_GUID, &meta_body);

    // Header extension body: 16 reserved GUID + 2 reserved + 4 data size + nested.
    let mut ext_body = Vec::new();
    ext_body.extend_from_slice(&[0u8; 16]); // reserved field 1
    ext_body.extend_from_slice(&0u16.to_le_bytes()); // reserved field 2
    ext_body.extend_from_slice(&(metadata_obj.len() as u32).to_le_bytes()); // data size
    ext_body.extend_from_slice(&metadata_obj);

    let obj = asf_object(&HEADER_EXTENSION_GUID, &ext_body);
    let data = asf_file(&[obj]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("header extension should parse");
    assert_eq!(
        md.get("ASF:Publisher"),
        Some(&TagValue::String("Nested Publisher".to_string()))
    );
}

#[test]
fn test_asf_multiple_objects() {
    // Combine content description + extended content in one header.
    let title = utf16("Multi");
    let mut cd_body = Vec::new();
    cd_body.extend_from_slice(&(title.len() as u16).to_le_bytes());
    cd_body.extend_from_slice(&0u16.to_le_bytes());
    cd_body.extend_from_slice(&0u16.to_le_bytes());
    cd_body.extend_from_slice(&0u16.to_le_bytes());
    cd_body.extend_from_slice(&0u16.to_le_bytes());
    cd_body.extend_from_slice(&title);
    let cd = asf_object(&CONTENT_DESCRIPTION_GUID, &cd_body);

    let name = utf16("WM/Genre");
    let val = utf16("Jazz");
    let mut ec_body = Vec::new();
    ec_body.extend_from_slice(&1u16.to_le_bytes());
    ec_body.extend_from_slice(&(name.len() as u16).to_le_bytes());
    ec_body.extend_from_slice(&name);
    ec_body.extend_from_slice(&0u16.to_le_bytes());
    ec_body.extend_from_slice(&(val.len() as u16).to_le_bytes());
    ec_body.extend_from_slice(&val);
    let ec = asf_object(&EXTENDED_CONTENT_GUID, &ec_body);

    let data = asf_file(&[cd, ec]);
    let reader = TestReader::new(data);
    let md = parse_asf_metadata(&reader).expect("multi-object should parse");
    assert_eq!(
        md.get("ASF:Title"),
        Some(&TagValue::String("Multi".to_string()))
    );
    assert_eq!(
        md.get("ASF:Genre"),
        Some(&TagValue::String("Jazz".to_string()))
    );
}

#[test]
fn test_asf_parser_supports_format_and_struct() {
    use oxidex::core::FileFormat;
    use oxidex::core::FormatParser;
    let parser = AsfParser;
    assert!(parser.supports_format(FileFormat::ASF));
    assert!(!parser.supports_format(FileFormat::MKV));
    let data = asf_file(&[]);
    let reader = TestReader::new(data);
    assert!(parser.parse(&reader).is_ok());
}

#[test]
fn test_asf_production_path() {
    let title = utf16("ProdASF");
    let mut cd_body = Vec::new();
    cd_body.extend_from_slice(&(title.len() as u16).to_le_bytes());
    cd_body.extend_from_slice(&0u16.to_le_bytes());
    cd_body.extend_from_slice(&0u16.to_le_bytes());
    cd_body.extend_from_slice(&0u16.to_le_bytes());
    cd_body.extend_from_slice(&0u16.to_le_bytes());
    cd_body.extend_from_slice(&title);
    let cd = asf_object(&CONTENT_DESCRIPTION_GUID, &cd_body);
    let data = asf_file(&[cd]);

    let tmp = NamedTempFile::with_suffix(".asf").expect("tempfile");
    {
        let mut f = tmp.reopen().expect("reopen");
        f.write_all(&data).expect("write");
        f.flush().expect("flush");
    }
    let md = read_metadata(tmp.path()).expect("read_metadata asf");
    assert_eq!(
        md.get("ASF:Title"),
        Some(&TagValue::String("ProdASF".to_string()))
    );
}
