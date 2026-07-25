//! MKV (Matroska) video format parser
//!
//! Implements metadata extraction from Matroska/WebM container formats
//! following the EBML (Extensible Binary Meta Language) specification.
//!
//! # Supported Metadata
//!
//! - **Title, Artist, Album:** From Tags segment (SimpleTag elements)
//! - **Duration:** From SegmentInfo (Duration element)
//! - **Codec Information:** From Tracks segment (video/audio codec details)
//! - **Creation Date:** From DateUTC element
//! - **Muxing Application:** From MuxingApp element
//! - **Track Information:** Track names, languages, codec IDs
//! - **Video Details:** Width, height, frame rate, display dimensions
//! - **Audio Details:** Sample rate, channels, bits per sample
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `Matroska.pm` module:
//! - `Matroska:Title` → Title from Tags or Info
//! - `Matroska:Duration` → Duration from SegmentInfo
//! - `Matroska:MuxingApp` → MuxingApp from SegmentInfo
//! - `Matroska:WritingApp` → WritingApp from SegmentInfo
//! - `Matroska:DateTimeOriginal` → DateUTC from SegmentInfo
//! - `Matroska:DocType` → DocType from EBML header
//! - `Matroska:CodecID` → Codec IDs from Tracks
//! - `Matroska:ImageWidth` → Video pixel width
//! - `Matroska:ImageHeight` → Video pixel height
//! - `Matroska:FrameRate` → Video frame rate
//! - `Matroska:AudioSampleRate` → Audio sample rate
//! - `Matroska:AudioChannels` → Audio channel count
//!
//! # File Structure
//!
//! ```text
//! [EBML Header - required]
//!   ├─ EBMLVersion
//!   ├─ DocType ("matroska" or "webm")
//!   └─ DocTypeVersion
//! [Segment - main container]
//!   ├─ SeekHead (index to other segments)
//!   ├─ Info (duration, dates, muxing app)
//!   ├─ Tracks (video/audio codec info)
//!   ├─ Tags (metadata - PRIMARY METADATA SOURCE)
//!   ├─ Chapters (chapter information)
//!   ├─ Attachments (embedded files)
//!   └─ Clusters (actual media data - SKIP)
//! ```
//!
//! # EBML Variable-Length Integer Encoding
//!
//! EBML uses a variable-length integer format where the first byte indicates
//! the total length by the position of the leading 1 bit:
//! - 1xxxxxxx = 1 byte (value in lower 7 bits)
//! - 01xxxxxx xxxxxxxx = 2 bytes
//! - 001xxxxx xxxxxxxx xxxxxxxx = 3 bytes
//! - etc.
//!
//! # References
//!
//! - EBML RFC: <https://www.rfc-editor.org/rfc/rfc8794.html>
//! - Matroska Spec: <https://www.matroska.org/technical/elements.html>
//! - ExifTool Source: `lib/Image/ExifTool/Matroska.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// EBML header signature
const EBML_SIGNATURE: &[u8] = b"\x1A\x45\xDF\xA3";

// EBML Element IDs (as variable-length integers)
const EBML_HEADER: u32 = 0x1A45DFA3;
const EBML_VERSION: u32 = 0x4286;
const EBML_READ_VERSION: u32 = 0x42F7;
const EBML_MAX_ID_LENGTH: u32 = 0x42F2;
const EBML_MAX_SIZE_LENGTH: u32 = 0x42F3;
const EBML_DOC_TYPE: u32 = 0x4282;
const EBML_DOC_TYPE_VERSION: u32 = 0x4287;
const EBML_DOC_TYPE_READ_VERSION: u32 = 0x4285;

// Matroska Segment Elements
const SEGMENT: u32 = 0x18538067;
const SEEK_HEAD: u32 = 0x114D9B74;
const INFO: u32 = 0x1549A966;
const TRACKS: u32 = 0x1654AE6B;
const TAGS: u32 = 0x1254C367;
const CHAPTERS: u32 = 0x1043A770;
const ATTACHMENTS: u32 = 0x1941A469;

// Info Elements
const TIMECODE_SCALE: u32 = 0x2AD7B1;
const DURATION: u32 = 0x4489;
const DATE_UTC: u32 = 0x4461;
const TITLE: u32 = 0x7BA9;
const MUXING_APP: u32 = 0x4D80;
const WRITING_APP: u32 = 0x5741;

// Track Elements
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
const CODEC_NAME: u32 = 0x258688;
const CODEC_DECODE_ALL: u32 = 0xAA;
const TRACK_NAME: u32 = 0x536E;
const TRACK_LANGUAGE: u32 = 0x22B59C;
const LANGUAGE_BCP47: u32 = 0x22B59D;

// Video Elements
const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const DISPLAY_WIDTH: u32 = 0x54B0;
const DISPLAY_HEIGHT: u32 = 0x54BA;
const FRAME_RATE: u32 = 0x2383E3;
const FLAG_INTERLACED: u32 = 0x9A;

// Audio Elements
const AUDIO: u32 = 0xE1;
const SAMPLING_FREQUENCY: u32 = 0xB5;
const CHANNELS: u32 = 0x9F;
const BIT_DEPTH: u32 = 0x6264;

// Tag Elements
const TAG: u32 = 0x7373;
const SIMPLE_TAG: u32 = 0x67C8;
const TAG_NAME: u32 = 0x45A3;
const TAG_STRING: u32 = 0x4487;

// Chapter Elements
const EDITION_ENTRY: u32 = 0x45B9;
const CHAPTER_ATOM: u32 = 0xB6;
const CHAPTER_TIME_START: u32 = 0x91;
const CHAPTER_TIME_END: u32 = 0x92;
const CHAPTER_DISPLAY: u32 = 0x80;
const CHAP_STRING: u32 = 0x85;

// Attachment Elements
const ATTACHED_FILE: u32 = 0x61A7;
const FILE_NAME: u32 = 0x466E;
const FILE_MIME_TYPE: u32 = 0x4660;
const FILE_DESCRIPTION: u32 = 0x467E;

/// MKV parser
pub struct MkvParser;

impl FormatParser for MkvParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify EBML signature
        if reader.size() < 4 {
            return Err(ExifToolError::parse_error("File too small to be MKV"));
        }

        let header = reader.read(0, 4)?;
        if header != EBML_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid MKV signature: expected {:?}, found {:?}",
                EBML_SIGNATURE, header
            )));
        }

        let mut metadata = MetadataMap::with_capacity(32);
        let mut offset = 0u64;

        // Parse EBML header
        offset = parse_ebml_header(reader, offset, &mut metadata)?;

        // Parse Segment (main container)
        while offset < reader.size() {
            match parse_element_header(reader, offset) {
                Ok((element_id, element_size, header_size)) => {
                    if element_id == SEGMENT {
                        let segment_offset = offset + header_size;
                        let segment_end = segment_offset + element_size;
                        parse_segment(reader, segment_offset, segment_end, &mut metadata)?;
                        break;
                    }
                    offset += header_size + element_size;
                }
                Err(_) => break,
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::MKV)
    }
}

/// Parse EBML header
fn parse_ebml_header(
    reader: &dyn FileReader,
    mut offset: u64,
    metadata: &mut MetadataMap,
) -> Result<u64> {
    let (element_id, element_size, header_size) = parse_element_header(reader, offset)?;

    if element_id != EBML_HEADER {
        return Err(ExifToolError::parse_error(format!(
            "Missing EBML header: expected 0x{:08X}, found 0x{:08X}",
            EBML_HEADER, element_id
        )));
    }

    offset += header_size;
    let header_end = offset + element_size;

    while offset < header_end {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                match elem_id {
                    EBML_DOC_TYPE => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:DocType".to_string(),
                                TagValue::new_string(value),
                            );
                        }
                    }
                    EBML_DOC_TYPE_VERSION => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:DocTypeVersion".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    EBML_DOC_TYPE_READ_VERSION => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:DocTypeReadVersion".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    _ => {}
                }

                offset = data_offset + elem_size;
            }
            Err(_) => break,
        }
    }

    Ok(offset)
}

/// Parse Segment container
fn parse_segment(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((element_id, element_size, header_size)) => {
                let data_offset = offset + header_size;
                let element_end = data_offset + element_size;

                match element_id {
                    INFO => {
                        parse_info(reader, data_offset, element_end, metadata)?;
                    }
                    TRACKS => {
                        parse_tracks(reader, data_offset, element_end, metadata)?;
                    }
                    TAGS => {
                        parse_tags(reader, data_offset, element_end, metadata)?;
                    }
                    CHAPTERS => {
                        parse_chapters(reader, data_offset, element_end, metadata)?;
                    }
                    ATTACHMENTS => {
                        parse_attachments(reader, data_offset, element_end, metadata)?;
                    }
                    _ => {}
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Parse Info segment
fn parse_info(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut timecode_scale = 1_000_000u64; // Default: 1ms

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                match elem_id {
                    TIMECODE_SCALE => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            timecode_scale = value;
                            // Convert nanoseconds to milliseconds for ExifTool compatibility
                            let ms = value / 1_000_000;
                            metadata.insert(
                                "Matroska:TimecodeScale".to_string(),
                                TagValue::new_string(format!("{} ms", ms)),
                            );
                        }
                    }
                    DURATION => {
                        if let Ok(value) = read_float(reader, data_offset, elem_size as usize) {
                            // Duration is in timecode scale units
                            let duration_secs = (value * timecode_scale as f64) / 1_000_000_000.0;
                            // Format as H:MM:SS like ExifTool (round to nearest second)
                            let total_secs = duration_secs.round() as u64;
                            let hours = total_secs / 3600;
                            let mins = (total_secs % 3600) / 60;
                            let secs = total_secs % 60;
                            let formatted = format!("{}:{:02}:{:02}", hours, mins, secs);
                            metadata.insert(
                                "Matroska:Duration".to_string(),
                                TagValue::new_string(formatted.clone()),
                            );
                            // Add MKV:Duration tag for format-specific output
                            metadata.insert(
                                "MKV:Duration".to_string(),
                                TagValue::new_string(formatted),
                            );
                        }
                    }
                    DATE_UTC => {
                        if let Ok(value) = read_sint(reader, data_offset, elem_size as usize) {
                            // DateUTC is nanoseconds since 2001-01-01T00:00:00 UTC
                            let timestamp = 978307200i64 + (value / 1_000_000_000);
                            // Format as "YYYY:MM:DD HH:MM:SSZ" like ExifTool
                            let formatted = format_unix_timestamp_utc(timestamp);
                            metadata.insert(
                                "Matroska:DateTimeOriginal".to_string(),
                                TagValue::new_string(formatted),
                            );
                        }
                    }
                    TITLE => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            metadata
                                .insert("Matroska:Title".to_string(), TagValue::new_string(value));
                        }
                    }
                    MUXING_APP => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:MuxingApp".to_string(),
                                TagValue::new_string(value),
                            );
                        }
                    }
                    WRITING_APP => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:WritingApp".to_string(),
                                TagValue::new_string(value),
                            );
                        }
                    }
                    _ => {}
                }

                offset = data_offset + elem_size;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Parse Tracks segment
fn parse_tracks(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut track_count = 0;

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                if elem_id == TRACK_ENTRY {
                    track_count += 1;
                    parse_track_entry(reader, data_offset, element_end, track_count, metadata)?;
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Track info collected during parsing
struct TrackInfo {
    track_type: u64,
    track_number: u64,
    track_uid: u64,
    codec_id: String,
    language: String,
    flag_default: bool,
    flag_enabled: bool,
    flag_forced: bool,
    default_duration_ns: u64,
    track_timecode_scale: f64,
    codec_decode_all: bool,
}

impl Default for TrackInfo {
    fn default() -> Self {
        Self {
            track_type: 0,
            track_number: 0,
            track_uid: 0,
            codec_id: String::new(),
            language: "und".to_string(), // Default to "undetermined"
            flag_default: true,          // Default is true per spec
            flag_enabled: true,          // Default is true per spec
            flag_forced: false,
            default_duration_ns: 0,
            track_timecode_scale: 1.0,
            codec_decode_all: true, // Default is true per spec
        }
    }
}

/// Parse single track entry
fn parse_track_entry(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    _track_num: usize,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut track_info = TrackInfo::default();
    let mut video_offset = None;
    let mut audio_offset = None;
    let mut video_end = 0u64;
    let mut audio_end = 0u64;

    // First pass: collect track info
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                match elem_id {
                    TRACK_TYPE => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            track_info.track_type = value;
                        }
                    }
                    TRACK_NUMBER => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            track_info.track_number = value;
                        }
                    }
                    TRACK_UID => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            track_info.track_uid = value;
                        }
                    }
                    CODEC_ID => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            track_info.codec_id = value;
                        }
                    }
                    TRACK_LANGUAGE | LANGUAGE_BCP47 => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            track_info.language = value;
                        }
                    }
                    FLAG_DEFAULT => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            track_info.flag_default = value != 0;
                        }
                    }
                    FLAG_ENABLED => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            track_info.flag_enabled = value != 0;
                        }
                    }
                    FLAG_FORCED => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            track_info.flag_forced = value != 0;
                        }
                    }
                    DEFAULT_DURATION => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            track_info.default_duration_ns = value;
                        }
                    }
                    TRACK_TIMECODE_SCALE => {
                        if let Ok(value) = read_float(reader, data_offset, elem_size as usize) {
                            track_info.track_timecode_scale = value;
                        }
                    }
                    CODEC_DECODE_ALL => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            track_info.codec_decode_all = value != 0;
                        }
                    }
                    VIDEO => {
                        video_offset = Some(data_offset);
                        video_end = element_end;
                    }
                    AUDIO => {
                        audio_offset = Some(data_offset);
                        audio_end = element_end;
                    }
                    _ => {}
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    // Now output tags based on track type (ExifTool outputs per-track, not indexed)
    let track_type_str = match track_info.track_type {
        1 => "Video",
        2 => "Audio",
        3 => "Complex",
        17 => "Subtitle",
        18 => "Buttons",
        32 => "Control",
        _ => "Unknown",
    };

    // Output common track tags
    metadata.insert(
        "Matroska:TrackNumber".to_string(),
        TagValue::new_integer(track_info.track_number as i64),
    );
    metadata.insert(
        "Matroska:TrackType".to_string(),
        TagValue::new_string(track_type_str.to_string()),
    );
    if track_info.track_uid != 0 {
        metadata.insert(
            "Matroska:TrackUID".to_string(),
            TagValue::new_string(format!("{:08x}", track_info.track_uid)),
        );
    }
    metadata.insert(
        "Matroska:TrackLanguage".to_string(),
        TagValue::new_string(track_info.language.clone()),
    );
    metadata.insert(
        "Matroska:TrackDefault".to_string(),
        TagValue::new_string(if track_info.flag_default { "Yes" } else { "No" }.to_string()),
    );
    metadata.insert(
        "Matroska:TrackUsed".to_string(),
        TagValue::new_string(if track_info.flag_enabled { "Yes" } else { "No" }.to_string()),
    );
    metadata.insert(
        "Matroska:TrackForced".to_string(),
        TagValue::new_string(if track_info.flag_forced { "Yes" } else { "No" }.to_string()),
    );
    metadata.insert(
        "Matroska:CodecDecodeAll".to_string(),
        TagValue::new_string(
            if track_info.codec_decode_all {
                "Yes"
            } else {
                "No"
            }
            .to_string(),
        ),
    );

    if track_info.default_duration_ns > 0 {
        // Convert nanoseconds to milliseconds for ExifTool compatibility
        let ms = track_info.default_duration_ns / 1_000_000;
        metadata.insert(
            "Matroska:DefaultDuration".to_string(),
            TagValue::new_string(format!("{} ms", ms)),
        );
    }

    if track_info.track_timecode_scale != 1.0 {
        metadata.insert(
            "Matroska:TrackTimecodeScale".to_string(),
            TagValue::new_string(format!("{}", track_info.track_timecode_scale)),
        );
    } else {
        metadata.insert(
            "Matroska:TrackTimecodeScale".to_string(),
            TagValue::new_string("1".to_string()),
        );
    }

    // Output codec ID based on track type
    if !track_info.codec_id.is_empty() {
        match track_info.track_type {
            1 => {
                metadata.insert(
                    "Matroska:VideoCodecID".to_string(),
                    TagValue::new_string(track_info.codec_id.clone()),
                );
                // Add MKV:VideoCodec with human-readable codec name
                let codec_name = convert_codec_id_to_name(&track_info.codec_id, 1);
                metadata.insert(
                    "MKV:VideoCodec".to_string(),
                    TagValue::new_string(codec_name),
                );
            }
            2 => {
                metadata.insert(
                    "Matroska:AudioCodecID".to_string(),
                    TagValue::new_string(track_info.codec_id.clone()),
                );
                // Add MKV:AudioCodec with human-readable codec name
                let codec_name = convert_codec_id_to_name(&track_info.codec_id, 2);
                metadata.insert(
                    "MKV:AudioCodec".to_string(),
                    TagValue::new_string(codec_name),
                );
            }
            _ => {}
        }
    }

    // Parse video info if this is a video track
    if let Some(v_offset) = video_offset {
        parse_video_info(reader, v_offset, video_end, &track_info, metadata)?;
    }

    // Parse audio info if this is an audio track
    if let Some(a_offset) = audio_offset {
        parse_audio_info(reader, a_offset, audio_end, metadata)?;
    }

    Ok(())
}

/// Parse video track information
fn parse_video_info(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    track_info: &TrackInfo,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut display_width = 0u64;
    let mut display_height = 0u64;
    let mut interlace_flag = 0u64;

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                match elem_id {
                    PIXEL_WIDTH => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:ImageWidth".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                            // Add MKV:Width tag for format-specific output
                            metadata.insert(
                                "MKV:Width".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    PIXEL_HEIGHT => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:ImageHeight".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                            // Add MKV:Height tag for format-specific output
                            metadata.insert(
                                "MKV:Height".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    DISPLAY_WIDTH => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            display_width = value;
                            metadata.insert(
                                "Matroska:DisplayWidth".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    DISPLAY_HEIGHT => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            display_height = value;
                            metadata.insert(
                                "Matroska:DisplayHeight".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    FLAG_INTERLACED => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            interlace_flag = value;
                        }
                    }
                    FRAME_RATE => {
                        if let Ok(value) = read_float(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:VideoFrameRate".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                            // Add MKV:FrameRate tag with "fps" format
                            let frame_rate_str = format!("{:.3} fps", value);
                            metadata.insert(
                                "MKV:FrameRate".to_string(),
                                TagValue::new_string(frame_rate_str),
                            );
                        }
                    }
                    _ => {}
                }

                offset = data_offset + elem_size;
            }
            Err(_) => break,
        }
    }

    // Calculate frame rate from default duration if not explicitly set
    if !metadata.contains_key("Matroska:VideoFrameRate") && track_info.default_duration_ns > 0 {
        let fps = 1_000_000_000.0 / track_info.default_duration_ns as f64;
        metadata.insert(
            "Matroska:VideoFrameRate".to_string(),
            TagValue::new_integer(fps.round() as i64),
        );
        // Add MKV:FrameRate tag if not already present
        if !metadata.contains_key("MKV:FrameRate") {
            let frame_rate_str = format!("{:.3} fps", fps);
            metadata.insert(
                "MKV:FrameRate".to_string(),
                TagValue::new_string(frame_rate_str),
            );
        }
    }

    // Set scan type based on interlace flag
    let scan_type = match interlace_flag {
        0 => "Undetermined",
        1 => "Interlaced",
        2 => "Progressive",
        _ => "Undetermined",
    };
    metadata.insert(
        "Matroska:VideoScanType".to_string(),
        TagValue::new_string(scan_type.to_string()),
    );

    // Add display dimensions if not already set
    if display_width == 0 && display_height == 0 {
        // Use pixel dimensions as display dimensions if not specified
        if let Some(TagValue::Integer(w)) = metadata.get("Matroska:ImageWidth") {
            metadata.insert(
                "Matroska:DisplayWidth".to_string(),
                TagValue::new_integer(*w),
            );
        }
        if let Some(TagValue::Integer(h)) = metadata.get("Matroska:ImageHeight") {
            metadata.insert(
                "Matroska:DisplayHeight".to_string(),
                TagValue::new_integer(*h),
            );
        }
    }

    Ok(())
}

/// Parse audio track information
fn parse_audio_info(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                match elem_id {
                    SAMPLING_FREQUENCY => {
                        if let Ok(value) = read_float(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:AudioSampleRate".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                            // Add MKV:SampleRate tag for format-specific output
                            metadata.insert(
                                "MKV:SampleRate".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    CHANNELS => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:AudioChannels".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                            // Add MKV:Channels tag for format-specific output
                            metadata.insert(
                                "MKV:Channels".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    BIT_DEPTH => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "Matroska:AudioBitsPerSample".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    _ => {}
                }

                offset = data_offset + elem_size;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Parse Tags segment
fn parse_tags(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                if elem_id == TAG {
                    parse_tag(reader, data_offset, element_end, metadata)?;
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Parse single Tag element
fn parse_tag(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                if elem_id == SIMPLE_TAG {
                    parse_simple_tag(reader, data_offset, element_end, metadata)?;
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Parse SimpleTag element
fn parse_simple_tag(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut tag_name = String::new();
    let mut tag_value = String::new();

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                match elem_id {
                    TAG_NAME => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            tag_name = value;
                        }
                    }
                    TAG_STRING => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            tag_value = value;
                        }
                    }
                    _ => {}
                }

                offset = data_offset + elem_size;
            }
            Err(_) => break,
        }
    }

    if !tag_name.is_empty() && !tag_value.is_empty() {
        // Map common tag names to Matroska namespace
        let key = format!("Matroska:Tag:{}", tag_name);
        metadata.insert(key, TagValue::new_string(tag_value));
    }

    Ok(())
}

/// Parse Chapters segment
fn parse_chapters(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut chapter_count = 0;

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                if elem_id == EDITION_ENTRY {
                    chapter_count = parse_edition_entry(
                        reader,
                        data_offset,
                        element_end,
                        chapter_count,
                        metadata,
                    )?;
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    if chapter_count > 0 {
        metadata.insert(
            "Matroska:ChapterCount".to_string(),
            TagValue::new_integer(chapter_count as i64),
        );
    }

    Ok(())
}

/// Parse EditionEntry (contains chapters)
fn parse_edition_entry(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    mut chapter_count: usize,
    metadata: &mut MetadataMap,
) -> Result<usize> {
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                if elem_id == CHAPTER_ATOM {
                    chapter_count += 1;
                    parse_chapter_atom(reader, data_offset, element_end, chapter_count, metadata)?;
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    Ok(chapter_count)
}

/// Parse ChapterAtom
fn parse_chapter_atom(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    chapter_num: usize,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let chapter_prefix = format!("Matroska:Chapter{}:", chapter_num);

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                match elem_id {
                    CHAPTER_TIME_START => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            // Time is in nanoseconds
                            let time_secs = value as f64 / 1_000_000_000.0;
                            metadata.insert(
                                format!("{}TimeStart", chapter_prefix),
                                TagValue::new_string(format!("{:.3}", time_secs)),
                            );
                        }
                    }
                    CHAPTER_TIME_END => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            let time_secs = value as f64 / 1_000_000_000.0;
                            metadata.insert(
                                format!("{}TimeEnd", chapter_prefix),
                                TagValue::new_string(format!("{:.3}", time_secs)),
                            );
                        }
                    }
                    CHAPTER_DISPLAY => {
                        parse_chapter_display(
                            reader,
                            data_offset,
                            element_end,
                            &chapter_prefix,
                            metadata,
                        )?;
                    }
                    _ => {}
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Parse ChapterDisplay
fn parse_chapter_display(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    chapter_prefix: &str,
    metadata: &mut MetadataMap,
) -> Result<()> {
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                if elem_id == CHAP_STRING
                    && let Ok(value) = read_string(reader, data_offset, elem_size as usize)
                {
                    metadata.insert(
                        format!("{}Title", chapter_prefix),
                        TagValue::new_string(value),
                    );
                }

                offset = data_offset + elem_size;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Parse Attachments segment
fn parse_attachments(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut attachment_count = 0;

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                if elem_id == ATTACHED_FILE {
                    attachment_count += 1;
                    parse_attached_file(
                        reader,
                        data_offset,
                        element_end,
                        attachment_count,
                        metadata,
                    )?;
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    if attachment_count > 0 {
        metadata.insert(
            "Matroska:AttachmentCount".to_string(),
            TagValue::new_integer(attachment_count as i64),
        );
    }

    Ok(())
}

/// Parse AttachedFile
fn parse_attached_file(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    attachment_num: usize,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let attachment_prefix = format!("Matroska:Attachment{}:", attachment_num);

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                match elem_id {
                    FILE_NAME => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                format!("{}FileName", attachment_prefix),
                                TagValue::new_string(value),
                            );
                        }
                    }
                    FILE_MIME_TYPE => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                format!("{}MIMEType", attachment_prefix),
                                TagValue::new_string(value),
                            );
                        }
                    }
                    FILE_DESCRIPTION => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                format!("{}Description", attachment_prefix),
                                TagValue::new_string(value),
                            );
                        }
                    }
                    _ => {}
                }

                offset = data_offset + elem_size;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Convert Matroska codec ID to human-readable codec name
///
/// Maps standardized Matroska codec IDs (like "V_UNCOMPRESSED", "V_AV1", "A_OPUS")
/// to user-friendly codec names for the MKV: format tags.
///
/// # Arguments
///
/// * `codec_id` - The Matroska codec ID string (e.g., "V_VP9", "A_AAC")
/// * `track_type` - The track type (1 = video, 2 = audio, etc.)
///
/// # Returns
///
/// A human-readable codec name or the original codec ID if not recognized
fn convert_codec_id_to_name(codec_id: &str, track_type: u64) -> String {
    match track_type {
        // Video codecs
        1 => match codec_id {
            "V_UNCOMPRESSED" => "Uncompressed".to_string(),
            "V_MPEG4/ISO/AVC" => "H.264".to_string(),
            "V_MPEGH/ISO/HEVC" => "H.265".to_string(),
            "V_AV1" => "AV1".to_string(),
            "V_MPEG4/MS/V3" => "MPEG4 V3".to_string(),
            "V_MPEG4/ISO/SP" => "MPEG4 Part 2".to_string(),
            "V_MPEG4/ISO/ASP" => "MPEG4 Part 2".to_string(),
            "V_MPEG1" => "MPEG1".to_string(),
            "V_MPEG2" => "MPEG2".to_string(),
            "V_REAL/RV10" => "RealVideo 1.0".to_string(),
            "V_REAL/RV20" => "RealVideo 2.0".to_string(),
            "V_REAL/RV30" => "RealVideo 3.0".to_string(),
            "V_REAL/RV40" => "RealVideo 4.0".to_string(),
            "V_VP8" => "VP8".to_string(),
            "V_VP9" => "VP9".to_string(),
            "V_QUICKTIME" => "QuickTime".to_string(),
            "V_DIRAC" => "Dirac".to_string(),
            "V_PRORES" => "ProRes".to_string(),
            "V_FFV1" => "FFV1".to_string(),
            "V_THEORA" => "Theora".to_string(),
            "V_DAALA" => "Daala".to_string(),
            "V_JPEG2000" => "JPEG 2000".to_string(),
            "V_BETACODEC" => "BetaCodec".to_string(),
            "V_VC1" => "VC-1".to_string(),
            _ => codec_id.to_string(),
        },
        // Audio codecs
        2 => match codec_id {
            "A_MPEG/L1" => "MP1".to_string(),
            "A_MPEG/L2" => "MP2".to_string(),
            "A_MPEG/L3" => "MP3".to_string(),
            "A_PCM/INT/LIT" => "PCM".to_string(),
            "A_PCM/INT/BIG" => "PCM".to_string(),
            "A_PCM/FLOAT/IEEE" => "PCM (IEEE Float)".to_string(),
            "A_AC3" => "AC-3".to_string(),
            "A_EAC3" => "E-AC-3".to_string(),
            "A_ALAC" => "ALAC".to_string(),
            "A_DTS" => "DTS".to_string(),
            "A_DTS/CORE" => "DTS Core".to_string(),
            "A_DTS/EXPRESS" => "DTS Express".to_string(),
            "A_DTS/LOSSLESS" => "DTS-HD".to_string(),
            "A_FLAC" => "FLAC".to_string(),
            "A_TRUEHD" => "TrueHD".to_string(),
            "A_MLP" => "MLP".to_string(),
            "A_AAC/MPEG4/MAIN" => "AAC".to_string(),
            "A_AAC/MPEG4/LC" => "AAC-LC".to_string(),
            "A_AAC/MPEG4/SBR" => "AAC-HE".to_string(),
            "A_AAC/MPEG4/LC/SBR" => "AAC-HE".to_string(),
            "A_AAC/MPEG4/MAIN/SBR" => "AAC-HE Main".to_string(),
            "A_AAC/MPEG2/MAIN" => "AAC".to_string(),
            "A_AAC/MPEG2/LC" => "AAC-LC".to_string(),
            "A_AAC/MPEG2/SBR" => "AAC-HE".to_string(),
            "A_AAC/MPEG2/LC/SBR" => "AAC-HE".to_string(),
            "A_VORBIS" => "Vorbis".to_string(),
            "A_OPUS" => "Opus".to_string(),
            "A_REAL/14_4" => "RealAudio 1".to_string(),
            "A_REAL/28_8" => "RealAudio 2".to_string(),
            "A_REAL/COOK" => "RealAudio Cook".to_string(),
            "A_REAL/SIPR" => "RealAudio Sipr".to_string(),
            "A_REAL/RALF" => "RealAudio RALF".to_string(),
            "A_REAL/ATRC" => "RealAudio ATRC".to_string(),
            "A_WAVPACK4" => "WavPack".to_string(),
            "A_QUICKTIME" => "QuickTime Audio".to_string(),
            "A_TWOS" => "PCM (Two's Complement)".to_string(),
            "A_SOWT" => "PCM (Signed)".to_string(),
            "A_MSADPCM" => "MS ADPCM".to_string(),
            "A_IMAADPCM" => "IMA ADPCM".to_string(),
            _ => codec_id.to_string(),
        },
        _ => codec_id.to_string(),
    }
}

/// Parse EBML element header (ID + size)
/// Returns (element_id, element_size, header_size)
fn parse_element_header(reader: &dyn FileReader, offset: u64) -> Result<(u32, u64, u64)> {
    // Read ID (variable-length)
    let (element_id, id_size) = read_vint_id(reader, offset)?;

    // Read size (variable-length)
    let (element_size, size_len) = read_vint(reader, offset + id_size)?;

    let header_size = id_size + size_len;

    Ok((element_id, element_size, header_size))
}

/// Read EBML variable-length integer (for element IDs)
/// Note: Unlike size VINTs, element IDs keep the length marker bits!
fn read_vint_id(reader: &dyn FileReader, offset: u64) -> Result<(u32, u64)> {
    let first_byte = reader.read(offset, 1)?[0];

    let num_bytes = if first_byte & 0x80 != 0 {
        1
    } else if first_byte & 0x40 != 0 {
        2
    } else if first_byte & 0x20 != 0 {
        3
    } else if first_byte & 0x10 != 0 {
        4
    } else {
        return Err(ExifToolError::parse_error("Invalid VINT ID"));
    };

    let bytes = reader.read(offset, num_bytes)?;
    let mut value = bytes[0] as u32;

    for byte in bytes.iter().take(num_bytes).skip(1) {
        value = (value << 8) | *byte as u32;
    }

    Ok((value, num_bytes as u64))
}

/// Read EBML variable-length integer (for sizes)
fn read_vint(reader: &dyn FileReader, offset: u64) -> Result<(u64, u64)> {
    let first_byte = reader.read(offset, 1)?[0];

    let (num_bytes, mask) = if first_byte & 0x80 != 0 {
        (1, 0x7F)
    } else if first_byte & 0x40 != 0 {
        (2, 0x3F)
    } else if first_byte & 0x20 != 0 {
        (3, 0x1F)
    } else if first_byte & 0x10 != 0 {
        (4, 0x0F)
    } else if first_byte & 0x08 != 0 {
        (5, 0x07)
    } else if first_byte & 0x04 != 0 {
        (6, 0x03)
    } else if first_byte & 0x02 != 0 {
        (7, 0x01)
    } else if first_byte & 0x01 != 0 {
        (8, 0x00)
    } else {
        return Err(ExifToolError::parse_error("Invalid VINT size"));
    };

    let bytes = reader.read(offset, num_bytes)?;
    let mut value = (bytes[0] & mask) as u64;

    for byte in bytes.iter().take(num_bytes).skip(1) {
        value = (value << 8) | *byte as u64;
    }

    Ok((value, num_bytes as u64))
}

/// Read unsigned integer from EBML data
fn read_uint(reader: &dyn FileReader, offset: u64, size: usize) -> Result<u64> {
    if size == 0 || size > 8 {
        return Err(ExifToolError::parse_error("Invalid uint size"));
    }

    let bytes = reader.read(offset, size)?;
    let er = EndianReader::big_endian(bytes);

    // EBML uses big-endian, read based on size
    let value = match size {
        1 => er.u8_at(0).map(|v| v as u64),
        2 => er.u16_at(0).map(|v| v as u64),
        3 => {
            // 3-byte value: read as u32 with leading zero
            let b0 = er
                .u8_at(0)
                .ok_or_else(|| ExifToolError::parse_error("Failed to read byte 0"))?
                as u32;
            let b1 = er
                .u8_at(1)
                .ok_or_else(|| ExifToolError::parse_error("Failed to read byte 1"))?
                as u32;
            let b2 = er
                .u8_at(2)
                .ok_or_else(|| ExifToolError::parse_error("Failed to read byte 2"))?
                as u32;
            Some(((b0 << 16) | (b1 << 8) | b2) as u64)
        }
        4 => er.u32_at(0).map(|v| v as u64),
        5..=7 => {
            // Variable-length 5-7 bytes: manual read
            let mut value = 0u64;
            for i in 0..size {
                let byte = er
                    .u8_at(i)
                    .ok_or_else(|| ExifToolError::parse_error("Failed to read byte"))?;
                value = (value << 8) | byte as u64;
            }
            Some(value)
        }
        8 => er.u64_at(0),
        _ => None,
    }
    .ok_or_else(|| ExifToolError::parse_error("Failed to read uint"))?;

    Ok(value)
}

/// Read signed integer from EBML data
fn read_sint(reader: &dyn FileReader, offset: u64, size: usize) -> Result<i64> {
    if size == 0 || size > 8 {
        return Err(ExifToolError::parse_error("Invalid sint size"));
    }

    let bytes = reader.read(offset, size)?;
    let er = EndianReader::big_endian(bytes);

    // EBML uses big-endian, read based on size
    let value = match size {
        1 => er.i8_at(0).map(|v| v as i64),
        2 => er.i16_at(0).map(|v| v as i64),
        3..=7 => {
            // Variable-length: sign-extend manually
            let mut value = if bytes[0] & 0x80 != 0 { -1i64 } else { 0i64 };
            for &byte in bytes.iter() {
                value = (value << 8) | byte as i64;
            }
            Some(value)
        }
        8 => er.i64_at(0),
        _ => None,
    }
    .ok_or_else(|| ExifToolError::parse_error("Failed to read sint"))?;

    Ok(value)
}

/// Read floating point from EBML data
fn read_float(reader: &dyn FileReader, offset: u64, size: usize) -> Result<f64> {
    let bytes = reader.read(offset, size)?;
    let er = EndianReader::big_endian(bytes);

    match size {
        4 => er
            .f32_at(0)
            .map(|v| v as f64)
            .ok_or_else(|| ExifToolError::parse_error("Failed to read f32")),
        8 => er
            .f64_at(0)
            .ok_or_else(|| ExifToolError::parse_error("Failed to read f64")),
        _ => Err(ExifToolError::parse_error("Invalid float size")),
    }
}

/// Read UTF-8 string from EBML data
fn read_string(reader: &dyn FileReader, offset: u64, size: usize) -> Result<String> {
    if size == 0 {
        return Ok(String::new());
    }

    let bytes = reader.read(offset, size)?;
    let er = EndianReader::big_endian(bytes);

    er.str_at(0, size)
        .map(|s| s.to_string())
        .ok_or_else(|| ExifToolError::parse_error("Invalid UTF-8 string"))
}

/// Format Unix timestamp as ExifTool-compatible datetime string.
///
/// ExifTool uses format "YYYY:MM:DD HH:MM:SSZ" (colons between date parts, Z suffix)
fn format_unix_timestamp_utc(timestamp: i64) -> String {
    // Days since Unix epoch
    let days = timestamp / 86400;
    let time_of_day = (timestamp % 86400) as u32;

    // Time components
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    // Convert days to date using Howard Hinnant's algorithm
    let z = days + 719468; // Days since 0000-03-01
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32; // Day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // Year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // Day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // Month prime [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // Day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // Month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z",
        y, m, d, hour, minute, second
    )
}

/// Convenience function to parse MKV metadata from a reader.
///
/// This is a wrapper around `MkvParser::parse()` to provide a simpler API
/// for the operations module.
///
/// # Arguments
///
/// * `reader` - FileReader implementation providing access to the MKV file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_mkv_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = MkvParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_mkv_signature_valid() {
        // Minimal valid EBML file structure
        // Note: For this test, we just verify signature check passes
        // The parser will fail later when trying to parse the structure,
        // but that's OK for a basic signature test
        let mut data = vec![];

        // EBML Header element (0x1A45DFA3)
        data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]);
        // Size (using 1-byte VINT = 0x8F means size 15)
        data.push(0x8F);
        // EBML Version (0x4286)
        data.extend_from_slice(&[0x42, 0x86]);
        data.push(0x81); // size = 1
        data.push(0x01); // value = 1
        // DocType (0x4282)
        data.extend_from_slice(&[0x42, 0x82]);
        data.push(0x84); // size = 4
        data.extend_from_slice(b"webm");
        // DocTypeVersion (0x4287)
        data.extend_from_slice(&[0x42, 0x87]);
        data.push(0x81); // size = 1
        data.push(0x02); // value = 2

        let reader = TestReader::from_slice(&data);
        let parser = MkvParser;
        let result = parser.parse(&reader);
        if let Err(ref e) = result {
            eprintln!("Parse error: {:?}", e);
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_mkv_signature_invalid() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = MkvParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_mkv_file_too_small() {
        let data = b"\x1A\x45";
        let reader = TestReader::from_slice(data);
        let parser = MkvParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_vint() {
        // Test 1-byte VINT: 0x81 = 1000_0001 = value 1
        let data = vec![0x81];
        let reader = TestReader::from_slice(&data);
        let (value, size) = read_vint(&reader, 0).unwrap();
        assert_eq!(value, 1);
        assert_eq!(size, 1);

        // Test 2-byte VINT: 0x40 0x00 = value 0
        let data = vec![0x40, 0x00];
        let reader = TestReader::from_slice(&data);
        let (value, size) = read_vint(&reader, 0).unwrap();
        assert_eq!(value, 0);
        assert_eq!(size, 2);
    }

    #[test]
    fn test_read_uint() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let reader = TestReader::from_slice(&data);
        let value = read_uint(&reader, 0, 4).unwrap();
        assert_eq!(value, 0x01020304);
    }

    #[test]
    fn test_read_string() {
        let data = b"Hello, World!";
        let reader = TestReader::from_slice(data);
        let value = read_string(&reader, 0, data.len()).unwrap();
        assert_eq!(value, "Hello, World!");
    }
}
