//! FLV (Flash Video) format parser
//!
//! Implements metadata extraction from FLV video files, parsing the FLV header
//! and onMetaData script data objects.
//!
//! # Supported Metadata
//!
//! - **FLV Header:** Version, flags (video/audio presence)
//! - **onMetaData:** Duration, width, height, framerate, videodatarate, audiodatarate
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `Flash.pm` module:
//! - `Flash:Duration` → duration from onMetaData
//! - `Flash:ImageWidth` → width from onMetaData
//! - `Flash:ImageHeight` → height from onMetaData
//!
//! # File Structure
//!
//! ```text
//! [FLV Header - 9 bytes]
//!   ├─ Signature: "FLV" (3 bytes)
//!   ├─ Version: 1 byte
//!   ├─ Flags: 1 byte (audio/video)
//!   └─ DataOffset: 4 bytes (always 9)
//! [Previous Tag Size 0: 4 bytes]
//! [FLV Tags...]
//!   ├─ Tag Type (1 byte): Audio/Video/Script
//!   ├─ Data Size (3 bytes)
//!   ├─ Timestamp (3 bytes + 1 byte extended)
//!   ├─ Stream ID (3 bytes)
//!   └─ Data (variable)
//! ```
//!
//! # References
//!
//! - FLV Spec: Adobe FLV File Format Specification v10.1
//! - ExifTool Source: `lib/Image/ExifTool/Flash.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// FLV signature
const FLV_SIGNATURE: &[u8] = b"FLV";

/// FLV tag types
const TAG_TYPE_AUDIO: u8 = 8;
const TAG_TYPE_VIDEO: u8 = 9;
const TAG_TYPE_SCRIPT: u8 = 18;

/// FLV parser
pub struct FlvParser;

impl FormatParser for FlvParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify FLV signature
        if reader.size() < 9 {
            return Err(ExifToolError::parse_error("File too small to be FLV"));
        }

        let header = reader.read(0, 9)?;
        if &header[0..3] != FLV_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid FLV signature: expected {:?}, found {:?}",
                FLV_SIGNATURE,
                &header[0..3]
            )));
        }

        let mut metadata = MetadataMap::with_capacity(16);

        // Parse FLV header
        let version = header[3];
        let flags = header[4];
        let has_video = (flags & 0x01) != 0;
        let has_audio = (flags & 0x04) != 0;

        // Note: FLVVersion is available but ExifTool doesn't output it, so we skip it
        let _version = version;

        metadata.insert(
            "Flash:HasVideo".to_string(),
            TagValue::new_string(if has_video { "Yes" } else { "No" }),
        );
        metadata.insert(
            "Flash:HasAudio".to_string(),
            TagValue::new_string(if has_audio { "Yes" } else { "No" }),
        );

        // Look for onMetaData script tag and first audio tag
        // Skip: Previous Tag Size 0 (4 bytes after header)
        let mut offset = 13u64;
        let file_size = reader.size();

        // Search for script and audio tags (limited search to avoid scanning entire file)
        let max_search_offset = (offset + 10_000).min(file_size);
        let mut found_script = false;
        let mut found_audio = false;

        while offset + 11 < max_search_offset && (!found_script || !found_audio) {
            // Read tag header (11 bytes)
            let tag_header = reader.read(offset, 11)?;

            let r = EndianReader::big_endian(tag_header);
            let tag_type = tag_header[0];
            // Read 24-bit big-endian value (3 bytes) for data size
            let data_size = ((r.u8_at(1).unwrap_or(0) as u32) << 16)
                | ((r.u8_at(2).unwrap_or(0) as u32) << 8)
                | (r.u8_at(3).unwrap_or(0) as u32);

            // Check if this is a script data tag
            if tag_type == TAG_TYPE_SCRIPT && data_size > 0 && data_size < 100_000 && !found_script
            {
                // Read script data
                let script_data = reader.read(offset + 11, data_size as usize)?;

                // Parse onMetaData (simplified parsing)
                parse_on_metadata(script_data, &mut metadata)?;
                found_script = true;
            }

            // Check if this is an audio tag - extract codec info from first byte
            if tag_type == TAG_TYPE_AUDIO && data_size > 0 && !found_audio {
                let audio_data = reader.read(offset + 11, 1)?;
                let audio_flags = audio_data[0];

                // Parse audio flags:
                // Bits 4-7: Sound format (codec)
                // Bits 2-3: Sample rate (0=5.5kHz, 1=11kHz, 2=22kHz, 3=44kHz)
                // Bit 1: Sample size (0=8-bit, 1=16-bit)
                // Bit 0: Channel type (0=mono, 1=stereo)
                let sample_size = if (audio_flags & 0x02) != 0 { 16 } else { 8 };
                let channels = if (audio_flags & 0x01) != 0 { 2 } else { 1 };
                let sample_rate_code = (audio_flags >> 2) & 0x03;
                let sample_rate = match sample_rate_code {
                    0 => 5512,
                    1 => 11025,
                    2 => 22050,
                    3 => 44100,
                    _ => 44100,
                };

                metadata.insert(
                    "Flash:AudioBitsPerSample".to_string(),
                    TagValue::new_integer(sample_size),
                );

                let channels_str = if channels == 1 {
                    "1 (mono)".to_string()
                } else {
                    format!("{} (stereo)", channels)
                };
                metadata.insert(
                    "Flash:AudioChannels".to_string(),
                    TagValue::new_string(channels_str),
                );

                // Update sample rate from audio tag (more accurate than metadata)
                metadata.insert(
                    "Flash:AudioSampleRate".to_string(),
                    TagValue::new_integer(sample_rate),
                );

                found_audio = true;
            }

            // Move to next tag (tag header + data size + previous tag size)
            offset += 11 + data_size as u64 + 4;
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::FLV)
    }
}

/// Convenience function to parse FLV metadata from a reader.
///
/// This is a wrapper around `FlvParser::parse()` to provide a simpler API
/// for the operations module.
///
/// # Arguments
///
/// * `reader` - FileReader implementation providing access to the FLV file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_flv_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = FlvParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Map FLV metadata key to ExifTool tag name
fn map_flv_key_to_tag(key: &str) -> Option<&'static str> {
    match key {
        // Core video metadata
        "duration" => Some("Flash:Duration"),
        "width" => Some("Flash:ImageWidth"),
        "height" => Some("Flash:ImageHeight"),
        "framerate" => Some("Flash:FrameRate"),
        "videodatarate" => Some("Flash:VideoBitrate"),
        "audiodatarate" => Some("Flash:AudioBitrate"),
        "videocodecid" => Some("Flash:VideoCodecID"),
        "audiocodecid" => Some("Flash:AudioCodecID"),
        // Audio details
        "audiosamplerate" => Some("Flash:AudioSampleRate"),
        "audiosamplesize" => Some("Flash:AudioSampleSize"),
        "stereo" => Some("Flash:Stereo"),
        "audiosize" => Some("Flash:AudioSize"),
        "audiodelay" => Some("Flash:AudioDelay"),
        "audioencoding" | "audioformat" => Some("Flash:AudioEncoding"),
        "audiochannels" => Some("Flash:AudioChannels"),
        "audiobitspersample" => Some("Flash:AudioBitsPerSample"),
        // File/data info
        "datasize" => Some("Flash:DataSize"),
        "filesize" | "filesizebytes" => Some("Flash:FileSizeBytes"),
        "videosize" => Some("Flash:VideoSize"),
        // Metadata creator info
        "metadatacreator" => Some("Flash:MetadataCreator"),
        "metadatadate" | "creationdate" => Some("Flash:MetadataDate"),
        // Boolean flags
        "canseektoend" => Some("Flash:CanSeekToEnd"),
        "hasaudio" => Some("Flash:HasAudio"),
        "hasvideo" => Some("Flash:HasVideo"),
        "haskeyframes" => Some("Flash:HasKeyFrames"),
        "hasmetadata" => Some("Flash:HasMetadata"),
        "hascuepoints" => Some("Flash:HasCuePoints"),
        // Keyframe data
        "lasttimestamp" => Some("Flash:LastTimeStamp"),
        "lastkeyframetime" | "lastkeyframetimestamp" => Some("Flash:LastKeyFrameTime"),
        // Custom user field
        "test" => Some("Flash:Test"),
        _ => None,
    }
}

/// Map video codec ID to encoding name
fn video_codec_to_encoding(codec_id: i64) -> Option<&'static str> {
    match codec_id {
        2 => Some("Sorenson H.263"),
        3 => Some("Screen Video"),
        4 => Some("On2 VP6"),
        5 => Some("On2 VP6 with alpha"),
        6 => Some("Screen Video 2"),
        7 => Some("AVC/H.264"),
        _ => None,
    }
}

/// Map audio codec ID to encoding name
fn audio_codec_to_encoding(codec_id: i64) -> Option<&'static str> {
    match codec_id {
        0 => Some("Linear PCM"),
        1 => Some("ADPCM"),
        2 => Some("MP3"),
        3 => Some("Linear PCM (little endian)"),
        4 => Some("Nellymoser 16 kHz mono"),
        5 => Some("Nellymoser 8 kHz mono"),
        6 => Some("Nellymoser"),
        10 => Some("AAC"),
        11 => Some("Speex"),
        14 => Some("MP3 8 kHz"),
        _ => None,
    }
}

/// Parse onMetaData script data object (AMF0 parsing)
fn parse_on_metadata(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // AMF0 format:
    // - 0x02 (String marker) + length (2 bytes) + "onMetaData"
    // - 0x08 (ECMA array marker) + count (4 bytes) + key-value pairs

    if data.is_empty() || data[0] != 0x02 {
        return Ok(()); // Not a string, skip
    }

    let r = EndianReader::big_endian(data);
    let mut offset = 1;

    // Skip first string (should be "onMetaData")
    if offset + 2 > data.len() {
        return Ok(());
    }
    let str_len = r.u16_at(offset).unwrap_or(0) as usize;
    offset += 2 + str_len;

    // Check for ECMA array marker
    if offset >= data.len() || data[offset] != 0x08 {
        return Ok(()); // Not an ECMA array
    }
    offset += 1;

    // Skip array count (4 bytes)
    if offset + 4 > data.len() {
        return Ok(());
    }
    offset += 4;

    // Track video/audio codec IDs to add encoding names later
    let mut video_codec_id: Option<i64> = None;
    let mut audio_codec_id: Option<i64> = None;

    // Parse key-value pairs
    while offset + 3 < data.len() {
        // Read key length
        let key_len = r.u16_at(offset).unwrap_or(0) as usize;
        offset += 2;

        if key_len == 0 {
            break; // End of object marker
        }

        if offset + key_len > data.len() {
            break;
        }

        let key = String::from_utf8_lossy(&data[offset..offset + key_len]).to_string();
        offset += key_len;

        if offset >= data.len() {
            break;
        }

        // Read value type
        let value_type = data[offset];
        offset += 1;

        let key_lower = key.to_lowercase();

        // Parse value based on type
        match value_type {
            0x00 => {
                // Number (8 bytes double)
                if offset + 8 > data.len() {
                    break;
                }
                let value = r.f64_at(offset).unwrap_or(0.0);
                offset += 8;

                // Map to ExifTool tag name
                if let Some(tag_name) = map_flv_key_to_tag(&key_lower) {
                    // Format duration with "s" suffix
                    if key_lower == "duration" {
                        metadata.insert(
                            tag_name.to_string(),
                            TagValue::new_string(format!("{:.2} s", value)),
                        );
                    }
                    // Format bitrates with "kbps" suffix
                    // ExifTool formatting: video rounds to integer, audio shows one decimal
                    else if key_lower.contains("datarate") {
                        let formatted = if key_lower.starts_with("video") {
                            // Video bitrate rounds to integer
                            format!("{} kbps", value.round() as i64)
                        } else {
                            // Audio bitrate uses one decimal place, strips ".0"
                            let rounded = (value * 10.0).round() / 10.0;
                            if (rounded - rounded.floor()).abs() < 0.001 {
                                format!("{} kbps", rounded as i64)
                            } else {
                                format!("{:.1} kbps", rounded)
                            }
                        };
                        metadata.insert(tag_name.to_string(), TagValue::new_string(formatted));
                    }
                    // Track codec IDs
                    else if key_lower == "videocodecid" {
                        video_codec_id = Some(value as i64);
                        metadata.insert(tag_name.to_string(), TagValue::new_integer(value as i64));
                    } else if key_lower == "audiocodecid" {
                        audio_codec_id = Some(value as i64);
                        metadata.insert(tag_name.to_string(), TagValue::new_integer(value as i64));
                    }
                    // Integer values
                    else if key_lower == "width"
                        || key_lower == "height"
                        || key_lower.contains("size")
                        || key_lower.contains("rate")
                        || key_lower.contains("channels")
                        || key_lower.contains("bitspersample")
                    {
                        metadata.insert(tag_name.to_string(), TagValue::new_integer(value as i64));
                    }
                    // Float values (framerate, timestamps)
                    else {
                        metadata.insert(tag_name.to_string(), TagValue::new_float(value));
                    }
                }
            }
            0x01 => {
                // Boolean (1 byte)
                if offset + 1 > data.len() {
                    break;
                }
                let bool_val = data[offset] != 0;
                offset += 1;

                if let Some(tag_name) = map_flv_key_to_tag(&key_lower) {
                    // ExifTool uses "Yes"/"No" for booleans
                    let value = if bool_val { "Yes" } else { "No" };
                    metadata.insert(tag_name.to_string(), TagValue::new_string(value));
                }
            }
            0x02 => {
                // String (2 bytes length + data)
                if offset + 2 > data.len() {
                    break;
                }
                let str_len = r.u16_at(offset).unwrap_or(0) as usize;
                offset += 2;
                if offset + str_len > data.len() {
                    break;
                }
                let str_val = String::from_utf8_lossy(&data[offset..offset + str_len]).to_string();
                offset += str_len;

                if let Some(tag_name) = map_flv_key_to_tag(&key_lower) {
                    metadata.insert(tag_name.to_string(), TagValue::new_string(str_val));
                }
            }
            0x03 => {
                // Object - parse keyframes, cuePoints, etc.
                if key_lower == "keyframes" {
                    offset = parse_keyframes_object(data, offset, metadata);
                } else {
                    // Skip other objects
                    offset = skip_amf0_object(data, offset);
                }
            }
            0x08 => {
                // ECMA Array - parse recursively for nested data
                offset = skip_amf0_ecma_array(data, offset);
            }
            0x0A => {
                // Strict array - parse cuePoints array
                if key_lower == "cuepoints" {
                    offset = parse_cuepoints_array(data, offset, metadata);
                } else {
                    // Skip other strict arrays
                    if offset + 4 > data.len() {
                        break;
                    }
                    let arr_len = r.u32_at(offset).unwrap_or(0);
                    offset += 4;
                    for _ in 0..arr_len {
                        if offset >= data.len() {
                            break;
                        }
                        offset = skip_amf0_value(data, offset);
                    }
                }
            }
            0x0B => {
                // Date (8 bytes double + 2 bytes timezone offset)
                if offset + 10 > data.len() {
                    break;
                }
                let timestamp_ms = r.f64_at(offset).unwrap_or(0.0);
                let tz_offset = r.i16_at(offset + 8).unwrap_or(0);
                offset += 10;

                if let Some(tag_name) = map_flv_key_to_tag(&key_lower) {
                    // Format as ExifTool date string
                    let date_str = format_flv_date(timestamp_ms, tz_offset);
                    metadata.insert(tag_name.to_string(), TagValue::new_string(date_str));
                }
            }
            _ => {
                // Unknown type, skip
                break;
            }
        }
    }

    // Add video/audio encoding names based on codec IDs
    if let Some(codec_id) = video_codec_id {
        if let Some(encoding) = video_codec_to_encoding(codec_id) {
            metadata.insert(
                "Flash:VideoEncoding".to_string(),
                TagValue::new_string(encoding),
            );
        }
    }
    if let Some(codec_id) = audio_codec_id {
        if let Some(encoding) = audio_codec_to_encoding(codec_id) {
            metadata.insert(
                "Flash:AudioEncoding".to_string(),
                TagValue::new_string(encoding),
            );
        }
    }

    Ok(())
}

/// Format FLV date (milliseconds since epoch) to ExifTool format
fn format_flv_date(timestamp_ms: f64, tz_offset_minutes: i16) -> String {
    // Convert ms to seconds
    let secs = (timestamp_ms / 1000.0) as i64;
    // Use round() for proper microsecond precision
    let subsec_micros = ((timestamp_ms % 1000.0) * 1000.0).round() as u32;

    // Calculate date/time components (simplified - assumes Unix epoch)
    // For a proper implementation, we'd use chrono, but we'll do basic math
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since 1970-01-01
    let (year, month, day) = days_to_ymd(days_since_epoch);

    // Format timezone
    let tz_hours = tz_offset_minutes.abs() / 60;
    let tz_mins = tz_offset_minutes.abs() % 60;
    let tz_sign = if tz_offset_minutes >= 0 { '+' } else { '-' };

    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:06}{}{:02}:{:02}",
        year, month, day, hours, minutes, seconds, subsec_micros, tz_sign, tz_hours, tz_mins
    )
}

/// Convert days since Unix epoch to year/month/day
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Simplified calculation - accurate for dates 1970-2099
    let mut remaining = days;
    let mut year = 1970i32;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let leap = is_leap_year(year);
    let month_days: [i64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u32;
    for &days in &month_days {
        if remaining < days {
            break;
        }
        remaining -= days;
        month += 1;
    }

    (year, month, (remaining + 1) as u32)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Parse keyframes object to extract times and positions arrays
fn parse_keyframes_object(data: &[u8], mut offset: usize, metadata: &mut MetadataMap) -> usize {
    let r = EndianReader::big_endian(data);

    // keyframes is an object with "times" and "filepositions" arrays
    while offset + 3 <= data.len() {
        // Check for end marker
        if data[offset] == 0x00 && data[offset + 1] == 0x00 && data[offset + 2] == 0x09 {
            return offset + 3;
        }

        // Read key length
        if offset + 2 > data.len() {
            return data.len();
        }
        let key_len = r.u16_at(offset).unwrap_or(0) as usize;
        offset += 2;

        if key_len == 0 {
            if offset < data.len() && data[offset] == 0x09 {
                return offset + 1;
            }
            break;
        }

        if offset + key_len > data.len() {
            break;
        }

        let key = String::from_utf8_lossy(&data[offset..offset + key_len]).to_string();
        offset += key_len;

        if offset >= data.len() {
            break;
        }

        let value_type = data[offset];
        offset += 1;

        let key_lower = key.to_lowercase();

        if value_type == 0x0A {
            // Strict array of numbers
            if offset + 4 > data.len() {
                break;
            }
            let arr_len = r.u32_at(offset).unwrap_or(0) as usize;
            offset += 4;

            let mut values: Vec<f64> = Vec::with_capacity(arr_len.min(100));
            for _ in 0..arr_len {
                if offset >= data.len() {
                    break;
                }
                if data[offset] == 0x00 && offset + 9 <= data.len() {
                    // Number
                    offset += 1;
                    let val = r.f64_at(offset).unwrap_or(0.0);
                    offset += 8;
                    values.push(val);
                } else {
                    offset = skip_amf0_value(data, offset);
                }
            }

            if key_lower == "times" {
                let times_str = format!(
                    "[{}]",
                    values
                        .iter()
                        .map(|v| format!("{:.3}", v))
                        .collect::<Vec<_>>()
                        .join(",")
                );
                metadata.insert(
                    "Flash:KeyFramesTimes".to_string(),
                    TagValue::new_string(times_str),
                );
                // Also get last keyframe time
                if let Some(&last) = values.last() {
                    metadata.insert(
                        "Flash:LastKeyFrameTime".to_string(),
                        TagValue::new_float(last),
                    );
                }
            } else if key_lower == "filepositions" {
                let positions_str = format!(
                    "[{}]",
                    values
                        .iter()
                        .map(|v| format!("{}", *v as i64))
                        .collect::<Vec<_>>()
                        .join(",")
                );
                metadata.insert(
                    "Flash:KeyFramePositions".to_string(),
                    TagValue::new_string(positions_str),
                );
            }
        } else {
            offset = skip_amf0_value_from_type(data, offset, value_type);
        }
    }
    offset
}

/// Parse cuePoints strict array
fn parse_cuepoints_array(data: &[u8], mut offset: usize, metadata: &mut MetadataMap) -> usize {
    let r = EndianReader::big_endian(data);

    if offset + 4 > data.len() {
        return data.len();
    }
    let arr_len = r.u32_at(offset).unwrap_or(0) as usize;
    offset += 4;

    // Add HasCuePoints flag
    if arr_len > 0 {
        metadata.insert(
            "Flash:HasCuePoints".to_string(),
            TagValue::new_string("Yes"),
        );
    }

    // Parse each cue point object
    for idx in 0..arr_len.min(10) {
        // Limit to 10 cue points
        if offset >= data.len() {
            break;
        }

        let type_marker = data[offset];
        offset += 1;

        if type_marker == 0x03 {
            // Object
            offset = parse_cuepoint_object(data, offset, idx, metadata);
        } else if type_marker == 0x08 {
            // ECMA Array (also used for cue points)
            if offset + 4 > data.len() {
                break;
            }
            offset += 4; // Skip count
            offset = parse_cuepoint_object(data, offset, idx, metadata);
        } else {
            offset = skip_amf0_value_from_type(data, offset, type_marker);
        }
    }

    // Skip remaining cue points if more than 10
    for _ in arr_len.min(10)..arr_len {
        if offset >= data.len() {
            break;
        }
        offset = skip_amf0_value(data, offset);
    }

    offset
}

/// Parse a single cuepoint object
fn parse_cuepoint_object(
    data: &[u8],
    mut offset: usize,
    idx: usize,
    metadata: &mut MetadataMap,
) -> usize {
    let r = EndianReader::big_endian(data);

    while offset + 3 <= data.len() {
        // Check for end marker
        if data[offset] == 0x00 && data[offset + 1] == 0x00 && data[offset + 2] == 0x09 {
            return offset + 3;
        }

        // Read key length
        if offset + 2 > data.len() {
            return data.len();
        }
        let key_len = r.u16_at(offset).unwrap_or(0) as usize;
        offset += 2;

        if key_len == 0 {
            if offset < data.len() && data[offset] == 0x09 {
                return offset + 1;
            }
            break;
        }

        if offset + key_len > data.len() {
            break;
        }

        let key = String::from_utf8_lossy(&data[offset..offset + key_len]).to_string();
        offset += key_len;

        if offset >= data.len() {
            break;
        }

        let value_type = data[offset];
        offset += 1;

        let key_lower = key.to_lowercase();

        match value_type {
            0x00 => {
                // Number
                if offset + 8 > data.len() {
                    break;
                }
                let value = r.f64_at(offset).unwrap_or(0.0);
                offset += 8;

                if key_lower == "time" {
                    metadata.insert(
                        format!("Flash:CuePoint{}Time", idx),
                        TagValue::new_float(value),
                    );
                }
            }
            0x02 => {
                // String
                if offset + 2 > data.len() {
                    break;
                }
                let str_len = r.u16_at(offset).unwrap_or(0) as usize;
                offset += 2;
                if offset + str_len > data.len() {
                    break;
                }
                let str_val = String::from_utf8_lossy(&data[offset..offset + str_len]).to_string();
                offset += str_len;

                if key_lower == "name" {
                    metadata.insert(
                        format!("Flash:CuePoint{}Name", idx),
                        TagValue::new_string(str_val),
                    );
                } else if key_lower == "type" {
                    metadata.insert(
                        format!("Flash:CuePoint{}Type", idx),
                        TagValue::new_string(str_val),
                    );
                }
            }
            0x03 => {
                // Nested object (parameters)
                if key_lower == "parameters" {
                    offset = parse_cuepoint_parameters(data, offset, idx, metadata);
                } else {
                    offset = skip_amf0_object(data, offset);
                }
            }
            0x08 => {
                // Nested ECMA array (parameters can be ECMA array too)
                if offset + 4 > data.len() {
                    break;
                }
                offset += 4; // Skip count
                if key_lower == "parameters" {
                    offset = parse_cuepoint_parameters(data, offset, idx, metadata);
                } else {
                    offset = skip_amf0_object(data, offset);
                }
            }
            _ => {
                offset = skip_amf0_value_from_type(data, offset, value_type);
            }
        }
    }
    offset
}

/// Parse cuepoint parameters object
fn parse_cuepoint_parameters(
    data: &[u8],
    mut offset: usize,
    cue_idx: usize,
    metadata: &mut MetadataMap,
) -> usize {
    let r = EndianReader::big_endian(data);

    while offset + 3 <= data.len() {
        if data[offset] == 0x00 && data[offset + 1] == 0x00 && data[offset + 2] == 0x09 {
            return offset + 3;
        }

        if offset + 2 > data.len() {
            return data.len();
        }
        let key_len = r.u16_at(offset).unwrap_or(0) as usize;
        offset += 2;

        if key_len == 0 {
            if offset < data.len() && data[offset] == 0x09 {
                return offset + 1;
            }
            break;
        }

        if offset + key_len > data.len() {
            break;
        }

        let key = String::from_utf8_lossy(&data[offset..offset + key_len]).to_string();
        offset += key_len;

        if offset >= data.len() {
            break;
        }

        let value_type = data[offset];
        offset += 1;

        if value_type == 0x02 {
            // String parameter
            if offset + 2 > data.len() {
                break;
            }
            let str_len = r.u16_at(offset).unwrap_or(0) as usize;
            offset += 2;
            if offset + str_len > data.len() {
                break;
            }
            let str_val = String::from_utf8_lossy(&data[offset..offset + str_len]).to_string();
            offset += str_len;

            // Capitalize first letter of key for tag name
            let param_key = capitalize_first(&key);
            metadata.insert(
                format!("Flash:CuePoint{}Parameter{}", cue_idx, param_key),
                TagValue::new_string(str_val),
            );
        } else {
            offset = skip_amf0_value_from_type(data, offset, value_type);
        }
    }
    offset
}

/// Capitalize first letter of string
fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Skip an AMF0 object (key-value pairs until end marker)
fn skip_amf0_object(data: &[u8], mut offset: usize) -> usize {
    let r = EndianReader::big_endian(data);

    while offset + 3 <= data.len() {
        if data[offset] == 0x00 && data[offset + 1] == 0x00 && data[offset + 2] == 0x09 {
            return offset + 3;
        }

        if offset + 2 > data.len() {
            return data.len();
        }
        let key_len = r.u16_at(offset).unwrap_or(0) as usize;
        offset += 2;

        if key_len == 0 {
            if offset < data.len() && data[offset] == 0x09 {
                return offset + 1;
            }
            break;
        }

        offset += key_len;
        if offset >= data.len() {
            break;
        }

        offset = skip_amf0_value(data, offset);
    }
    offset
}

/// Skip an AMF0 ECMA array
fn skip_amf0_ecma_array(data: &[u8], mut offset: usize) -> usize {
    if offset + 4 > data.len() {
        return data.len();
    }
    offset += 4; // Skip count

    skip_amf0_object(data, offset)
}

/// Skip AMF0 value given type byte already read
fn skip_amf0_value_from_type(data: &[u8], offset: usize, value_type: u8) -> usize {
    match value_type {
        0x00 => offset + 8, // Number
        0x01 => offset + 1, // Boolean
        0x02 => {
            // String
            if offset + 2 > data.len() {
                return data.len();
            }
            let len = ((data[offset] as usize) << 8) | (data[offset + 1] as usize);
            offset + 2 + len
        }
        0x03 => skip_amf0_object(data, offset),     // Object
        0x05 | 0x06 => offset,                      // Null/Undefined
        0x08 => skip_amf0_ecma_array(data, offset), // ECMA Array
        0x0A => {
            // Strict array
            if offset + 4 > data.len() {
                return data.len();
            }
            let r = EndianReader::big_endian(data);
            let count = r.u32_at(offset).unwrap_or(0);
            let mut off = offset + 4;
            for _ in 0..count {
                if off >= data.len() {
                    break;
                }
                off = skip_amf0_value(data, off);
            }
            off
        }
        0x0B => offset + 10, // Date
        0x0C => {
            // Long string
            if offset + 4 > data.len() {
                return data.len();
            }
            let len = ((data[offset] as usize) << 24)
                | ((data[offset + 1] as usize) << 16)
                | ((data[offset + 2] as usize) << 8)
                | (data[offset + 3] as usize);
            offset + 4 + len
        }
        _ => data.len(),
    }
}

/// Skip an AMF0 value and return new offset
fn skip_amf0_value(data: &[u8], mut offset: usize) -> usize {
    if offset >= data.len() {
        return offset;
    }
    let value_type = data[offset];
    offset += 1;
    match value_type {
        0x00 => offset + 8, // Number (8 bytes)
        0x01 => offset + 1, // Boolean (1 byte)
        0x02 => {
            // String (2 byte length + data)
            if offset + 2 > data.len() {
                return data.len();
            }
            let len = ((data[offset] as usize) << 8) | (data[offset + 1] as usize);
            offset + 2 + len
        }
        0x03 | 0x08 => {
            // Object (0x03) or ECMA Array (0x08)
            // ECMA array has 4-byte count prefix
            if value_type == 0x08 {
                if offset + 4 > data.len() {
                    return data.len();
                }
                offset += 4;
            }
            // Skip key-value pairs until end marker (00 00 09)
            while offset + 3 <= data.len() {
                // Check for end marker
                if data[offset] == 0x00 && data[offset + 1] == 0x00 && data[offset + 2] == 0x09 {
                    return offset + 3;
                }
                // Read key length
                if offset + 2 > data.len() {
                    return data.len();
                }
                let key_len = ((data[offset] as usize) << 8) | (data[offset + 1] as usize);
                offset += 2;
                if key_len == 0 {
                    // End of object (just need to skip 0x09)
                    if offset < data.len() && data[offset] == 0x09 {
                        return offset + 1;
                    }
                    break;
                }
                // Skip key
                offset += key_len;
                if offset >= data.len() {
                    return data.len();
                }
                // Recursively skip value
                offset = skip_amf0_value(data, offset);
            }
            offset
        }
        0x05 | 0x06 => offset, // Null/Undefined
        0x0A => {
            // Strict array (4 byte count + elements)
            if offset + 4 > data.len() {
                return data.len();
            }
            let count = ((data[offset] as u32) << 24)
                | ((data[offset + 1] as u32) << 16)
                | ((data[offset + 2] as u32) << 8)
                | (data[offset + 3] as u32);
            offset += 4;
            for _ in 0..count {
                if offset >= data.len() {
                    break;
                }
                offset = skip_amf0_value(data, offset);
            }
            offset
        }
        0x0B => offset + 10, // Date (8 bytes + 2 byte timezone)
        0x0C => {
            // Long string (4 byte length + data)
            if offset + 4 > data.len() {
                return data.len();
            }
            let len = ((data[offset] as usize) << 24)
                | ((data[offset + 1] as usize) << 16)
                | ((data[offset + 2] as usize) << 8)
                | (data[offset + 3] as usize);
            offset + 4 + len
        }
        _ => data.len(), // Unknown - bail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_flv_signature_valid() {
        // Minimal FLV header
        let mut data = vec![0u8; 100];
        data[0..3].copy_from_slice(b"FLV");
        data[3] = 1; // version
        data[4] = 0x05; // flags (has audio + video)
        data[5..9].copy_from_slice(&9u32.to_be_bytes()); // data offset

        let reader = TestReader::from_slice(&data);
        let parser = FlvParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        // FLVVersion is not output to match ExifTool behavior
        assert!(metadata.get("Flash:FLVVersion").is_none());
        // But HasVideo and HasAudio should be present
        assert!(metadata.get("Flash:HasVideo").is_some());
        assert!(metadata.get("Flash:HasAudio").is_some());
    }

    #[test]
    fn test_flv_signature_invalid() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = FlvParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_flv_file_too_small() {
        let data = b"FLV";
        let reader = TestReader::from_slice(data);
        let parser = FlvParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
