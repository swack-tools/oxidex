//! WebM video format parser
//!
//! Implements metadata extraction from WebM container format (a subset of Matroska).
//! WebM uses the same EBML structure as MKV but is restricted to VP8/VP9/AV1 video
//! and Vorbis/Opus audio codecs.
//!
//! # Supported Metadata
//!
//! - **EBML Header:** DocType verification ("webm")
//! - **Segment Info:** Duration, muxing application, writing application
//! - **Tags:** Title, Artist, Album (from SimpleTag elements)
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `Matroska.pm` module (WebM is a Matroska profile):
//! - `Matroska:DocType` → "webm" from EBML header
//! - `Matroska:Duration` → Duration from SegmentInfo
//! - `Matroska:MuxingApp` → MuxingApp from SegmentInfo
//!
//! # File Structure
//!
//! ```text
//! [EBML Header - required]
//!   ├─ EBMLVersion
//!   ├─ DocType ("webm")
//!   └─ DocTypeVersion
//! [Segment - main container]
//!   ├─ Info (duration, muxing app)
//!   ├─ Tracks (VP8/VP9/AV1 + Vorbis/Opus)
//!   ├─ Tags (metadata)
//!   └─ Clusters (media data)
//! ```
//!
//! # References
//!
//! - WebM Spec: <https://www.webmproject.org/docs/container/>
//! - Matroska Spec: <https://www.matroska.org/technical/elements.html>
//! - ExifTool Source: `lib/Image/ExifTool/Matroska.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// EBML header signature (shared with MKV)
const EBML_SIGNATURE: &[u8] = b"\x1A\x45\xDF\xA3";

// EBML Element IDs (as variable-length integers)
const EBML_HEADER: u32 = 0x1A45DFA3;
const EBML_DOC_TYPE: u32 = 0x4282;
const EBML_DOC_TYPE_VERSION: u32 = 0x4287;

// Matroska Segment Elements
const SEGMENT: u32 = 0x18538067;
const INFO: u32 = 0x1549A966;
const TRACKS: u32 = 0x1654AE6B;

// Info Elements
const TIMECODE_SCALE: u32 = 0x2AD7B1;
const DURATION: u32 = 0x4489;

// Tracks Elements
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const VIDEO: u32 = 0xE0;
const AUDIO: u32 = 0xE1;

// Video Elements
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const FRAME_RATE: u32 = 0x2383E3;

// Audio Elements
const SAMPLING_FREQUENCY: u32 = 0xB5;
const CHANNELS: u32 = 0x9F;

/// WebM parser - uses Matroska EBML format with VP8/VP9/AV1 and Vorbis/Opus codecs
pub struct WebmParser;

impl FormatParser for WebmParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Every row here is read from the file (`metadata_map::file_rows`):
        // a caller's later `insert`/`get_mut` is what counts as assigned.
        crate::core::metadata_map::file_rows(|| -> Result<MetadataMap> {
            // Verify EBML signature
            if reader.size() < 4 {
                return Err(ExifToolError::parse_error("File too small to be WebM"));
            }

            let header = reader.read(0, 4)?;
            if header != EBML_SIGNATURE {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid WebM/EBML signature: expected {:?}, found {:?}",
                    EBML_SIGNATURE, header
                )));
            }

            let mut metadata = MetadataMap::new();

            // Parse EBML header to verify it's a WebM file
            match parse_ebml_header(reader, 0, &mut metadata) {
                Ok(segment_offset) => {
                    // Verify this is actually WebM (DocType should be "webm")
                    if let Some(TagValue::String(doc_type)) = metadata.get("Matroska:DocType") {
                        if doc_type != "webm" {
                            return Err(ExifToolError::parse_error(format!(
                                "Invalid WebM DocType: expected 'webm', found '{}'",
                                doc_type
                            )));
                        }
                    } else {
                        return Err(ExifToolError::parse_error("Missing WebM DocType"));
                    }

                    // The EBML header has variable length. Its next top-level
                    // element is the Segment wrapper, whose payload holds
                    // Info and Tracks.
                    parse_top_level_segment(reader, segment_offset, &mut metadata)?;
                }
                Err(e) => return Err(e),
            }

            Ok(metadata)
        })
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::WEBM)
    }
}

/// Find the outer Segment and bound its children to the available file bytes.
/// EBML permits an unknown-sized Segment; that form runs to end of file.
/// A truncated declared Segment keeps metadata from complete children only.
fn parse_top_level_segment(
    reader: &dyn FileReader,
    mut offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let file_end = reader.size();
    while offset < file_end {
        let Ok((element_id, id_size)) = read_vint_id(reader, offset) else {
            break;
        };
        let Some(size_offset) = offset.checked_add(id_size) else {
            break;
        };
        let Ok((element_size, size_len)) = read_vint(reader, size_offset) else {
            break;
        };
        let Some(data_offset) = size_offset
            .checked_add(size_len)
            .filter(|&start| start <= file_end)
        else {
            break;
        };
        let unknown_size = element_size == (1u64 << (7 * size_len)) - 1;
        if element_id == SEGMENT {
            let segment_end = if unknown_size {
                file_end
            } else {
                let Some(declared_end) = data_offset.checked_add(element_size) else {
                    break;
                };
                declared_end.min(file_end)
            };
            parse_segment(reader, data_offset, segment_end, metadata)?;
            break;
        }
        if unknown_size {
            break;
        }
        let Some(next_offset) = data_offset
            .checked_add(element_size)
            .filter(|&end| end <= file_end)
        else {
            break;
        };
        offset = next_offset;
    }
    Ok(())
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
                let Some(data_offset) = offset
                    .checked_add(header_size)
                    .filter(|&start| start <= end_offset)
                else {
                    break;
                };
                let Some(element_end) = data_offset
                    .checked_add(element_size)
                    .filter(|&end| end <= end_offset)
                else {
                    break;
                };

                match element_id {
                    INFO => {
                        parse_info(reader, data_offset, element_end, metadata)?;
                    }
                    TRACKS => {
                        parse_tracks(reader, data_offset, element_end, metadata)?;
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
    let mut duration = None;

    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                match elem_id {
                    TIMECODE_SCALE => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            timecode_scale = value;
                        }
                    }
                    DURATION => {
                        if let Ok(value) = read_float(reader, data_offset, elem_size as usize) {
                            duration = Some(value);
                        }
                    }
                    _ => {}
                }

                offset = data_offset + elem_size;
            }
            Err(_) => break,
        }
    }

    // Duration may precede TimecodeScale in Info. Apply the final scale only
    // after the complete directory has been inspected.
    if let Some(value) = duration {
        let duration_secs = (value * timecode_scale as f64) / 1_000_000_000.0;
        let total_secs = duration_secs.round() as u64;
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        let formatted = format!("{}:{:02}:{:02}", hours, mins, secs);
        metadata.insert("WEBM:Duration".to_string(), TagValue::new_string(formatted));
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
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;
                let element_end = data_offset + elem_size;

                if elem_id == TRACK_ENTRY {
                    parse_track_entry(reader, data_offset, element_end, metadata)?;
                }

                offset = element_end;
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Parse single track entry
fn parse_track_entry(
    reader: &dyn FileReader,
    mut offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut track_type = 0u64;
    let mut codec_id = String::new();
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
                            track_type = value;
                        }
                    }
                    CODEC_ID => {
                        if let Ok(value) = read_string(reader, data_offset, elem_size as usize) {
                            codec_id = value;
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

    // Add codec information
    if !codec_id.is_empty() {
        match track_type {
            1 => {
                // Video codec
                let codec_name = convert_codec_id_to_name(&codec_id, 1);
                metadata.insert(
                    "WEBM:VideoCodec".to_string(),
                    TagValue::new_string(codec_name),
                );
            }
            2 => {
                // Audio codec
                let codec_name = convert_codec_id_to_name(&codec_id, 2);
                metadata.insert(
                    "WEBM:AudioCodec".to_string(),
                    TagValue::new_string(codec_name),
                );
            }
            _ => {}
        }
    }

    // Parse video info if this is a video track
    if let Some(v_offset) = video_offset {
        parse_video_info(reader, v_offset, video_end, metadata)?;
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
    metadata: &mut MetadataMap,
) -> Result<()> {
    while offset < end_offset {
        match parse_element_header(reader, offset) {
            Ok((elem_id, elem_size, hdr_size)) => {
                let data_offset = offset + hdr_size;

                match elem_id {
                    PIXEL_WIDTH => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "WEBM:Width".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    PIXEL_HEIGHT => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "WEBM:Height".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    FRAME_RATE => {
                        if let Ok(value) = read_float(reader, data_offset, elem_size as usize) {
                            let frame_rate_str = format!("{:.3} fps", value);
                            metadata.insert(
                                "WEBM:FrameRate".to_string(),
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
                                "WEBM:SampleRate".to_string(),
                                TagValue::new_integer(value as i64),
                            );
                        }
                    }
                    CHANNELS => {
                        if let Ok(value) = read_uint(reader, data_offset, elem_size as usize) {
                            metadata.insert(
                                "WEBM:Channels".to_string(),
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

/// Convert WebM codec ID to human-readable codec name
fn convert_codec_id_to_name(codec_id: &str, track_type: u64) -> String {
    match track_type {
        // Video codecs - WebM restricted to VP8, VP9, AV1
        1 => match codec_id {
            "V_VP8" => "VP8".to_string(),
            "V_VP9" => "VP9".to_string(),
            "V_AV1" => "AV1".to_string(),
            _ => codec_id.to_string(),
        },
        // Audio codecs - WebM restricted to Vorbis and Opus
        2 => match codec_id {
            "A_VORBIS" => "Vorbis".to_string(),
            "A_OPUS" => "Opus".to_string(),
            _ => codec_id.to_string(),
        },
        _ => codec_id.to_string(),
    }
}

/// Parse EBML element header (ID + size)
/// Returns (element_id, element_size, header_size)
fn parse_element_header(reader: &dyn FileReader, offset: u64) -> Result<(u32, u64, u64)> {
    let (element_id, id_size) = read_vint_id(reader, offset)?;
    let (element_size, size_len) = read_vint(reader, offset + id_size)?;
    Ok((element_id, element_size, id_size + size_len))
}

/// Read EBML variable-length integer (for element IDs)
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

    let value = match size {
        1 => er.u8_at(0).map(|v| v as u64),
        2 => er.u16_at(0).map(|v| v as u64),
        3 => {
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

/// Read floating point from EBML data
fn read_float(reader: &dyn FileReader, offset: u64, size: usize) -> Result<f64> {
    match size {
        4 => {
            let bytes = reader.read(offset, 4)?;
            let er = EndianReader::big_endian(bytes);
            er.f32_at(0)
                .map(|v| v as f64)
                .ok_or_else(|| ExifToolError::parse_error("Failed to read float32"))
        }
        8 => {
            let bytes = reader.read(offset, 8)?;
            let er = EndianReader::big_endian(bytes);
            er.f64_at(0)
                .ok_or_else(|| ExifToolError::parse_error("Failed to read float64"))
        }
        _ => Err(ExifToolError::parse_error("Invalid float size")),
    }
}

/// Read string from EBML data
fn read_string(reader: &dyn FileReader, offset: u64, size: usize) -> Result<String> {
    if size == 0 {
        return Ok(String::new());
    }
    let bytes = reader.read(offset, size)?;
    String::from_utf8(bytes.to_vec())
        .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8: {}", e)))
}

/// Convenience function to parse WebM metadata from a reader.
///
/// This is a wrapper around `WebmParser::parse()` to provide a simpler API
/// for the operations module.
///
/// # Arguments
///
/// * `reader` - FileReader implementation providing access to the WebM file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_webm_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    // Every row here is read from the file (`metadata_map::file_rows`):
    // a caller's later `insert`/`get_mut` is what counts as assigned.
    crate::core::metadata_map::file_rows(|| -> std::result::Result<MetadataMap, String> {
        let parser = WebmParser;
        parser.parse(reader).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn element(id: &[u8], payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 127);
        let mut bytes = id.to_vec();
        bytes.push(0x80 | payload.len() as u8);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn header() -> Vec<u8> {
        let mut payload = element(&[0x42, 0x86], &[1]);
        payload.extend(element(&[0x42, 0x82], b"webm"));
        payload.extend(element(&[0x42, 0x87], &[4]));
        element(&[0x1A, 0x45, 0xDF, 0xA3], &payload)
    }

    fn info_and_tracks(duration_first: bool) -> (Vec<u8>, usize) {
        let duration = element(&[0x44, 0x89], &1000f32.to_be_bytes());
        let scale = element(&[0x2A, 0xD7, 0xB1], &2_000_000u32.to_be_bytes());
        let mut info = Vec::new();
        if duration_first {
            info.extend_from_slice(&duration);
            info.extend_from_slice(&scale);
        } else {
            info.extend_from_slice(&scale);
            info.extend_from_slice(&duration);
        }
        let mut payload = element(&[0x15, 0x49, 0xA9, 0x66], &info);
        let info_len = payload.len();
        let mut track = element(&[0xD7], &[2]); // TrackNumber, independent of TrackType
        track.extend(element(&[0x83], &[1])); // TrackType: video
        track.extend(element(&[0x86], b"V_VP9"));
        let track_entry = element(&[0xAE], &track);
        payload.extend(element(&[0x16, 0x54, 0xAE, 0x6B], &track_entry));
        (payload, info_len)
    }

    fn with_segment(payload: &[u8], encoded_size: u8) -> Vec<u8> {
        let mut bytes = header();
        bytes.extend_from_slice(&[0x18, 0x53, 0x80, 0x67, encoded_size]);
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn segment_children_are_read_inside_declared_or_unknown_size() {
        for duration_first in [true, false] {
            let (payload, _) = info_and_tracks(duration_first);
            for size in [0x80 | payload.len() as u8, 0xFF] {
                let data = with_segment(&payload, size);
                let metadata = WebmParser
                    .parse(&TestReader::from_slice(&data))
                    .expect("valid WebM Segment");
                assert_eq!(metadata.get_string("Matroska:DocType"), Some("webm"));
                assert_eq!(metadata.get_string("WEBM:Duration"), Some("0:00:02"));
                assert_eq!(metadata.get_string("WEBM:VideoCodec"), Some("VP9"));
                assert!(metadata.get("WEBM:AudioCodec").is_none());
            }
        }
    }

    #[test]
    fn segment_does_not_read_children_past_declared_or_truncated_end() {
        let (payload, info_len) = info_and_tracks(true);
        for data in [
            with_segment(&payload, 0x80 | info_len as u8),
            with_segment(&payload[..payload.len() - 1], 0x80 | payload.len() as u8),
        ] {
            let metadata = WebmParser
                .parse(&TestReader::from_slice(&data))
                .expect("partial WebM Segment retains complete Info");
            assert_eq!(metadata.get_string("WEBM:Duration"), Some("0:00:02"));
            assert!(metadata.get("WEBM:VideoCodec").is_none());
        }
    }

    #[test]
    fn incomplete_segment_header_does_not_emit_child_tags() {
        let mut data = header();
        data.extend_from_slice(&[0x18, 0x53, 0x80, 0x67]);
        let metadata = WebmParser
            .parse(&TestReader::from_slice(&data))
            .expect("EBML header is complete");
        assert_eq!(metadata.get_string("Matroska:DocType"), Some("webm"));
        assert!(metadata.get("WEBM:Duration").is_none());
    }

    #[test]
    fn test_webm_signature_valid() {
        // Minimal valid WebM file structure
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
        data.push(0x04); // value = 4

        let reader = TestReader::from_slice(&data);
        let parser = WebmParser;
        let result = parser.parse(&reader);
        let metadata = result.expect("valid WebM header");
        assert_eq!(metadata.get_string("Matroska:DocType"), Some("webm"));
        assert_eq!(metadata.get_integer("Matroska:DocTypeVersion"), Some(4));
    }

    #[test]
    fn test_webm_signature_invalid() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = WebmParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_webm_file_too_small() {
        let data = b"\x1A\x45";
        let reader = TestReader::from_slice(data);
        let parser = WebmParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
