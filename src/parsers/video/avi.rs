//! AVI (Audio Video Interleave) format parser
//!
//! Implements metadata extraction from AVI video files using the RIFF
//! container format. Shares RIFF parsing logic with WAV parser.
//!
//! # Supported Metadata
//!
//! - **INFO Chunk:** INAM (Name), IART (Artist), ICRD (Creation Date), IGNR (Genre)
//! - **Stream Headers:** Video/audio codec information
//! - **Main Header:** Frame rate, dimensions, total frames
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `RIFF.pm` module:
//! - `RIFF:Title` → INAM from INFO chunk
//! - `RIFF:Artist` → IART from INFO chunk
//! - `RIFF:FrameRate` → From main AVI header
//!
//! # File Structure
//!
//! ```text
//! [RIFF header - "RIFF" + size + "AVI "]
//! [LIST hdrl - Header list]
//!   ├─ avih (Main AVI header)
//!   └─ LIST strl (Stream headers)
//! [LIST INFO - Metadata (optional)]
//! [LIST movi - Movie data]
//! [idx1 - Index (optional)]
//! ```
//!
//! # References
//!
//! - AVI Spec: <https://msdn.microsoft.com/en-us/library/windows/desktop/dd318189>
//! - ExifTool Source: `lib/Image/ExifTool/RIFF.pm`

#![allow(dead_code)]

use crate::core::formatters::audio_encoding_name;
use crate::core::{FileFormat, FileReader, FormatParser, Instance, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::parsers::xmp::parse_xmp;

/// RIFF signature
const RIFF_SIGNATURE: &[u8] = b"RIFF";

/// AVI format identifier (note the space at the end)
const AVI_FORMAT: &[u8] = b"AVI ";

/// AVI parser
pub struct AviParser;

impl FormatParser for AviParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify RIFF/AVI signature
        if reader.size() < 12 {
            return Err(ExifToolError::parse_error("File too small to be AVI"));
        }

        let header = reader.read(0, 12)?;
        if &header[0..4] != RIFF_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid RIFF signature: expected {:?}, found {:?}",
                RIFF_SIGNATURE,
                &header[0..4]
            )));
        }

        if &header[8..12] != AVI_FORMAT {
            return Err(ExifToolError::parse_error(format!(
                "Invalid AVI format: expected {:?}, found {:?}",
                AVI_FORMAT,
                &header[8..12]
            )));
        }

        let mut metadata = MetadataMap::with_capacity(16);
        let file_size = reader.size();

        // Parse RIFF chunks (shared with WAV parser)
        parse_avi_chunks(reader, 12, file_size, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::AVI)
    }
}

/// Convenience function to parse AVI metadata from a reader.
///
/// This is a wrapper around `AviParser::parse()` to provide a simpler API
/// for the operations module.
///
/// # Arguments
///
/// * `reader` - FileReader implementation providing access to the AVI file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_avi_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = AviParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Formats a float the way Perl's default number stringification would: no
/// trailing zeros or unnecessary decimal point.
fn format_trimmed_decimal(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{:.3}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Converts a raw RIFF `IDIT` date string into ExifTool's `YYYY:MM:DD HH:MM:SS`
/// format.
///
/// Mirrors ExifTool's `Image::ExifTool::RIFF::ConvertRIFFDate()`, handling the
/// standard ctime-style AVI date format (e.g. "Mon Mar 10 15:04:43 2003") as
/// well as a couple of camera-specific variants. Unrecognized formats are
/// returned unchanged.
fn convert_riff_idit_date(raw: &str) -> String {
    let parts: Vec<&str> = raw.split_whitespace().collect();

    // Standard AVI date format: "Day Mon DD HH:MM:SS YYYY"
    if parts.len() >= 5 {
        if let (Some(month), Ok(day), Ok(year)) = (
            month_name_to_number(parts[1]),
            parts[2].parse::<u32>(),
            parts[4].parse::<u32>(),
        ) {
            return format!("{:04}:{:02}:{:02} {}", year, month, day, parts[3]);
        }
    }

    raw.to_string()
}

/// Maps a 3-letter (or full) English month name to its 1-based number.
fn month_name_to_number(name: &str) -> Option<u32> {
    let lower = name.to_lowercase();
    let short = &lower[..lower.len().min(3)];
    match short {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

/// Parse AVI RIFF chunks
fn parse_avi_chunks(
    reader: &dyn FileReader,
    start_offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut offset = start_offset;

    while offset + 8 < end_offset {
        // Read chunk header (4 byte ID + 4 byte size)
        let chunk_header = reader.read(offset, 8)?;

        let r = EndianReader::little_endian(chunk_header);
        let chunk_id = &chunk_header[0..4];
        let chunk_size = r.u32_at(4).unwrap_or(0) as u64;

        offset += 8;

        // Ensure chunk doesn't extend beyond file
        if offset + chunk_size > end_offset {
            break;
        }

        // Process specific chunks
        match chunk_id {
            b"LIST" => {
                // Parse LIST chunk
                if chunk_size >= 4 {
                    let list_type = reader.read(offset, 4)?;
                    match list_type as &[u8] {
                        b"hdrl" => {
                            // Header list - parse AVI header
                            parse_hdrl_list(reader, offset + 4, offset + chunk_size, metadata)?;
                        }
                        b"INFO" => {
                            // Metadata list - reuse WAV's INFO sub-chunk parser
                            // directly (skip the 4-byte "INFO" list-type marker;
                            // this is the tag stream itself, not another
                            // top-level RIFF chunk sequence).
                            crate::parsers::audio::wav::parse_info_chunk(
                                reader,
                                offset + 4,
                                offset + chunk_size,
                                metadata,
                            )?;
                        }
                        b"odml" => {
                            // OpenDML extended header - contains real frame count
                            parse_odml_list(reader, offset + 4, offset + chunk_size, metadata)?;
                        }
                        b"hydt" | b"pntx" => {
                            // Pentax-specific metadata (LIST_hydt/LIST_pntx in
                            // ExifTool's RIFF.pm), containing a "hymn"/"mknt"
                            // chunk with embedded Pentax MakerNotes.
                            parse_pentax_data_list(
                                reader,
                                offset + 4,
                                offset + chunk_size,
                                metadata,
                            )?;
                        }
                        _ => {
                            // Skip other LIST types (movi, etc.)
                        }
                    }
                }
            }
            b"_PMX" => {
                // XMP metadata chunk (stored as "_PMX" in RIFF)
                if chunk_size > 0 {
                    if let Ok(xmp_data) = reader.read(offset, chunk_size as usize) {
                        if let Ok(xmp_str) = std::str::from_utf8(&xmp_data) {
                            if let Ok(xmp_tuples) = parse_xmp(xmp_str.as_bytes()) {
                                for (key, value) in xmp_tuples {
                                    metadata.insert(key, TagValue::new_string(value));
                                }
                            }
                        }
                    }
                }
            }
            b"IDIT" => {
                // Date/time original chunk
                if chunk_size > 0 {
                    if let Ok(date_data) = reader.read(offset, chunk_size as usize) {
                        let date_str = String::from_utf8_lossy(&date_data).trim().replace('\0', "");
                        if !date_str.is_empty() {
                            metadata.insert(
                                "RIFF:DateTimeOriginal".to_string(),
                                TagValue::String(convert_riff_idit_date(&date_str)),
                            );
                        }
                    }
                }
            }
            _ => {
                // Skip unknown chunks
            }
        }

        // Move to next chunk (align to even byte boundary)
        offset += chunk_size;
        if chunk_size % 2 == 1 {
            offset += 1; // RIFF chunks are word-aligned
        }
    }

    Ok(())
}

/// Parse hdrl LIST (header list with avih chunk and stream headers)
fn parse_hdrl_list(
    reader: &dyn FileReader,
    start_offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut offset = start_offset;
    let mut stream_count = 0;

    while offset + 8 < end_offset {
        // Read chunk header
        let chunk_header = reader.read(offset, 8)?;

        let r = EndianReader::little_endian(chunk_header);
        let chunk_id = &chunk_header[0..4];
        let chunk_size = r.u32_at(4).unwrap_or(0) as u64;

        offset += 8;

        if offset + chunk_size > end_offset {
            break;
        }

        // Parse avih (main AVI header)
        if chunk_id == b"avih" && chunk_size >= 56 {
            parse_avih_chunk(reader, offset, metadata)?;
        }
        // IDIT (date/time original) is part of the Hdrl table in ExifTool's
        // RIFF.pm, not a top-level RIFF chunk.
        else if chunk_id == b"IDIT" && chunk_size > 0 {
            if let Ok(date_data) = reader.read(offset, chunk_size as usize) {
                let date_str = String::from_utf8_lossy(&date_data).trim().replace('\0', "");
                if !date_str.is_empty() {
                    metadata.insert(
                        "RIFF:DateTimeOriginal".to_string(),
                        TagValue::String(convert_riff_idit_date(&date_str)),
                    );
                }
            }
        }
        // Parse strl LIST (stream list) or odml LIST (OpenDML extended header)
        else if chunk_id == b"LIST" && chunk_size >= 4 {
            let list_type = reader.read(offset, 4)?;
            if list_type == b"strl" {
                stream_count += 1;
                parse_stream_list(
                    reader,
                    offset + 4,
                    offset + chunk_size,
                    stream_count,
                    metadata,
                )?;
            } else if list_type == b"odml" {
                // OpenDML extended header LIST - contains dmlh with real frame count
                parse_odml_list(reader, offset + 4, offset + chunk_size, metadata)?;
            }
        }

        // Move to next chunk
        offset += chunk_size;
        if chunk_size % 2 == 1 {
            offset += 1;
        }
    }

    Ok(())
}

/// Parse avih chunk (main AVI header)
fn parse_avih_chunk(
    reader: &dyn FileReader,
    offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let avih_data = reader.read(offset, 56)?;
    let r = EndianReader::little_endian(avih_data);

    // AVIMAINHEADER structure:
    // Offset 0:  dwMicroSecPerFrame
    // Offset 4:  dwMaxBytesPerSec
    // Offset 8:  dwPaddingGranularity
    // Offset 12: dwFlags
    // Offset 16: dwTotalFrames
    // Offset 20: dwInitialFrames
    // Offset 24: dwStreams
    // Offset 28: dwSuggestedBufferSize
    // Offset 32: dwWidth
    // Offset 36: dwHeight
    let microsec_per_frame = r.u32_at(0).unwrap_or(0);
    let max_bytes_per_sec = r.u32_at(4).unwrap_or(0);
    let total_frames = r.u32_at(16).unwrap_or(0);
    let stream_count = r.u32_at(24).unwrap_or(0);
    let width = r.u32_at(32).unwrap_or(0);
    let height = r.u32_at(36).unwrap_or(0);

    // Calculate frame rate from microseconds per frame
    if microsec_per_frame > 0 {
        let frame_rate = 1_000_000.0 / microsec_per_frame as f64;
        // ExifTool's RIFF:FrameRate tag (from avih), rounded to 3 decimal
        // places and printed without trailing zeros (matching Perl's default
        // number stringification).
        let rounded = ((frame_rate * 1000.0 + 0.5).floor()) / 1000.0;
        let frame_rate_str = format_trimmed_decimal(rounded);
        metadata.insert(
            "RIFF:FrameRate".to_string(),
            TagValue::new_string(frame_rate_str),
        );
        metadata.insert(
            "RIFF:VideoFrameRate".to_string(),
            TagValue::new_integer(frame_rate.round() as i64),
        );
    }

    // Note: ExifTool only outputs TotalFrameCount from dmlh (OpenDML extended header),
    // not from avih. We follow the same behavior for compatibility.
    // TotalFrameCount is set in parse_odml_list() if dmlh chunk is present.
    metadata.insert(
        "RIFF:ImageWidth".to_string(),
        TagValue::new_integer(width as i64),
    );

    metadata.insert(
        "RIFF:ImageHeight".to_string(),
        TagValue::new_integer(height as i64),
    );

    // StreamCount
    if stream_count > 0 {
        metadata.insert(
            "RIFF:StreamCount".to_string(),
            TagValue::new_integer(stream_count as i64),
        );
    }

    // MaxDataRate - convert to kB/s
    if max_bytes_per_sec > 0 {
        let kb_per_sec = max_bytes_per_sec / 1000;
        metadata.insert(
            "RIFF:MaxDataRate".to_string(),
            TagValue::new_string(format!("{} kB/s", kb_per_sec)),
        );
    }

    // Calculate duration if we have frame rate and total frames
    if microsec_per_frame > 0 && total_frames > 0 {
        let duration_secs = (microsec_per_frame as f64 * total_frames as f64) / 1_000_000.0;
        let duration_str = format!("{:.2}", duration_secs);
        metadata.insert(
            "RIFF:Duration".to_string(),
            TagValue::new_string(duration_str),
        );
    }

    Ok(())
}

/// Parse odml LIST (OpenDML extended header)
/// Contains dmlh chunk with real TotalFrameCount for extended AVI files
fn parse_odml_list(
    reader: &dyn FileReader,
    start_offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut offset = start_offset;

    while offset + 8 < end_offset {
        // Read chunk header
        let chunk_header = reader.read(offset, 8)?;

        let r = EndianReader::little_endian(chunk_header);
        let chunk_id = &chunk_header[0..4];
        let chunk_size = r.u32_at(4).unwrap_or(0) as u64;

        offset += 8;

        if offset + chunk_size > end_offset {
            break;
        }

        // Parse dmlh (OpenDML Extended AVI Header)
        // Structure: typedef struct { DWORD dwTotalFrames; } ODMLExtendedAVIHeader;
        if chunk_id == b"dmlh" && chunk_size >= 4 {
            let dmlh_data = reader.read(offset, 4)?;
            let dmlh_reader = EndianReader::little_endian(dmlh_data);
            let total_frames = dmlh_reader.u32_at(0).unwrap_or(0);

            // Override the TotalFrameCount from avih with the real value from dmlh
            if total_frames > 0 {
                metadata.insert(
                    "RIFF:TotalFrameCount".to_string(),
                    TagValue::new_integer(total_frames as i64),
                );
            }
        }

        // Move to next chunk
        offset += chunk_size;
        if chunk_size % 2 == 1 {
            offset += 1;
        }
    }

    Ok(())
}

/// Parse a Pentax metadata LIST (`LIST_hydt` / `LIST_pntx`), which contains a
/// `hymn` (or `mknt`) chunk holding an embedded Pentax MakerNotes IFD.
///
/// See ExifTool's `Image::ExifTool::Pentax::AVI` table: the maker note data
/// starts with a "PENTAX \0" header followed by a 2-byte byte-order marker
/// and the IFD itself (handled by the shared Pentax MakerNotes parser).
fn parse_pentax_data_list(
    reader: &dyn FileReader,
    start_offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut offset = start_offset;

    while offset + 8 < end_offset {
        let chunk_header = reader.read(offset, 8)?;
        let r = EndianReader::little_endian(chunk_header);
        let chunk_id = &chunk_header[0..4];
        let chunk_size = r.u32_at(4).unwrap_or(0) as u64;

        offset += 8;

        if offset + chunk_size > end_offset {
            break;
        }

        if (chunk_id == b"hymn" || chunk_id == b"mknt") && chunk_size > 0 {
            if let Ok(makernote_data) = reader.read(offset, chunk_size as usize) {
                let mut makernote_tags = std::collections::HashMap::new();
                // Byte order is auto-detected from the "PENTAX \0" header's
                // marker inside the Pentax parser; the default passed here is
                // only used as a fallback.
                let _ = crate::parsers::tiff::makernote_dispatcher::dispatch_makernote(
                    "Pentax",
                    &makernote_data,
                    crate::parsers::tiff::ifd_parser::ByteOrder::BigEndian,
                    &mut makernote_tags,
                );
                // Every call here is Pentax (the `make` argument above is a
                // literal `"Pentax"`), so `record_makernote_tag` -- not a
                // bare `insert` -- is required: it recognizes
                // `insert_low_priority_retained`'s synthetic `"<key> (N)"`
                // duplicate marker (LensType/LensFocalLength/PentaxModelID)
                // and records it as a real, always-losing occurrence rather
                // than a literal `"Tag (N)"` tag name.
                for (tag_name, tag_value_str) in makernote_tags {
                    crate::parsers::tiff::makernotes::shared::tag_priority::record_makernote_tag(
                        metadata,
                        tag_name,
                        TagValue::String(tag_value_str),
                    );
                }
            }
        }

        // Move to next chunk (word-aligned)
        offset += chunk_size;
        if chunk_size % 2 == 1 {
            offset += 1;
        }
    }

    Ok(())
}

/// Parse strl LIST (stream list with strh and strf chunks)
fn parse_stream_list(
    reader: &dyn FileReader,
    start_offset: u64,
    end_offset: u64,
    stream_num: usize,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut offset = start_offset;
    let mut stream_type: Option<[u8; 4]> = None;
    let is_first_video = !metadata.contains_key("RIFF:VideoCodec");
    let is_first_audio = !metadata.contains_key("RIFF:AudioCodec");

    while offset + 8 < end_offset {
        // Read chunk header
        let chunk_header = reader.read(offset, 8)?;

        let r = EndianReader::little_endian(chunk_header);
        let chunk_id = &chunk_header[0..4];
        let chunk_size = r.u32_at(4).unwrap_or(0) as u64;

        offset += 8;

        if offset + chunk_size > end_offset {
            break;
        }

        match chunk_id {
            b"strh" => {
                // Parse stream header
                if chunk_size >= 56 {
                    stream_type = parse_stream_header(
                        reader,
                        offset,
                        is_first_video,
                        is_first_audio,
                        metadata,
                    )?;
                }
            }
            b"strf" => {
                // Parse stream format (depends on stream type)
                if let Some(stype) = stream_type {
                    let is_first = match &stype {
                        b"vids" => is_first_video,
                        b"auds" => is_first_audio,
                        _ => false,
                    };
                    parse_stream_format(reader, offset, chunk_size, &stype, is_first, metadata)?;
                }
            }
            b"strn" => {
                // Parse stream name (skip for now, not commonly used)
            }
            _ => {}
        }

        // Move to next chunk
        offset += chunk_size;
        if chunk_size % 2 == 1 {
            offset += 1;
        }
    }

    // Track that we've processed this stream type
    let _ = stream_num; // Silence unused warning

    Ok(())
}

/// Parse strh chunk (stream header)
fn parse_stream_header(
    reader: &dyn FileReader,
    offset: u64,
    is_first_video: bool,
    is_first_audio: bool,
    metadata: &mut MetadataMap,
) -> Result<Option<[u8; 4]>> {
    let strh_data = reader.read(offset, 56)?;
    let r = EndianReader::little_endian(strh_data);

    let stream_type = [strh_data[0], strh_data[1], strh_data[2], strh_data[3]];
    let codec_fourcc = [strh_data[4], strh_data[5], strh_data[6], strh_data[7]];
    let scale = r.u32_at(20).unwrap_or(0);
    let rate = r.u32_at(24).unwrap_or(0);
    let length = r.u32_at(32).unwrap_or(0);
    // AVISTREAMHEADER: dwQuality is at offset 40, dwSampleSize at offset 44
    // (dwStart=28, dwLength=32, dwSuggestedBufferSize=36, dwQuality=40,
    // dwSampleSize=44).
    let quality = r.u32_at(40).unwrap_or(0);

    // RIFF.pm's StreamHeader table has `PRIORITY => 0` (RIFF.pm:1160-1165),
    // so the first stream remains the default-view winner while `-a` exposes
    // every stream occurrence. Its PrintConv maps `vids` and `auds` to these
    // display values (RIFF.pm:1166-1176).
    if stream_type == *b"vids" && is_first_video {
        metadata.insert_occurrence(
            "RIFF:StreamType",
            TagValue::new_string("Video".to_string()),
            0,
            "RIFF",
            Instance::default(),
        );
    } else if stream_type == *b"auds" && is_first_audio {
        metadata.insert_occurrence(
            "RIFF:StreamType",
            TagValue::new_string("Audio".to_string()),
            0,
            "RIFF",
            Instance::default(),
        );
    }

    // Codec FourCC
    let fourcc_str = String::from_utf8_lossy(&codec_fourcc).to_string();
    let fourcc_is_present = !fourcc_str.trim().is_empty() && fourcc_str != "\0\0\0\0";
    if fourcc_is_present && stream_type == *b"vids" && is_first_video {
        metadata.insert(
            "RIFF:VideoCodec".to_string(),
            TagValue::new_string(fourcc_str.clone()),
        );
    } else if stream_type == *b"auds" && is_first_audio {
        // Audio codec from strh is usually empty, strf has more info; ExifTool
        // still emits an (empty) RIFF:AudioCodec tag in that case.
        let value = if fourcc_is_present {
            fourcc_str.clone()
        } else {
            String::new()
        };
        metadata.insert(
            "RIFF:AudioCodec".to_string(),
            TagValue::new_string(value.clone()),
        );
    }

    // Video frame count and rate
    if stream_type == *b"vids" && is_first_video && length > 0 {
        metadata.insert(
            "RIFF:VideoFrameCount".to_string(),
            TagValue::new_integer(length as i64),
        );
        // FrameCount at stream level = same as VideoFrameCount
        metadata.insert(
            "RIFF:FrameCount".to_string(),
            TagValue::new_integer(length as i64),
        );
    }

    // Audio sample count
    if stream_type == *b"auds" && is_first_audio && length > 0 {
        metadata.insert(
            "RIFF:AudioSampleCount".to_string(),
            TagValue::new_integer(length as i64),
        );
    }

    // Sample rate for audio streams
    if stream_type == *b"auds" && is_first_audio && rate > 0 && scale > 0 {
        let sample_rate = (rate as f64 / scale as f64) as i64;
        // This is overwritten by strf parsing with more accurate value
        if !metadata.contains_key("RIFF:AudioSampleRate") {
            metadata.insert(
                "RIFF:SampleRate".to_string(),
                TagValue::new_integer(sample_rate),
            );
        }
    }

    // Quality (for video)
    if stream_type == *b"vids" && is_first_video && quality > 0 {
        metadata.insert(
            "RIFF:Quality".to_string(),
            TagValue::new_integer(quality as i64),
        );
    }

    // RIFF.pm:1243-1245 prints zero as "Variable" and a nonzero value as
    // its exact byte count (not merely "Fixed"). As above, priority 0 keeps
    // the first stream's value in the ordinary view and retains both for -a.
    let sample_size = r.u32_at(44).unwrap_or(0);
    if (stream_type == *b"vids" && is_first_video) || (stream_type == *b"auds" && is_first_audio) {
        let size_str = if sample_size == 0 {
            "Variable".to_string()
        } else if sample_size == 1 {
            "1 byte".to_string()
        } else {
            format!("{sample_size} bytes")
        };
        metadata.insert_occurrence(
            "RIFF:SampleSize",
            TagValue::new_string(size_str),
            0,
            "RIFF",
            Instance::default(),
        );
    }

    Ok(Some(stream_type))
}

/// Parse strf chunk (stream format - depends on stream type)
fn parse_stream_format(
    reader: &dyn FileReader,
    offset: u64,
    size: u64,
    stream_type: &[u8; 4],
    is_first: bool,
    metadata: &mut MetadataMap,
) -> Result<()> {
    match stream_type {
        b"vids" => {
            // Video format (BITMAPINFOHEADER)
            if size >= 40 {
                parse_video_format(reader, offset, is_first, metadata)?;
            }
        }
        b"auds" => {
            // Audio format (WAVEFORMATEX)
            if size >= 16 {
                parse_audio_format(reader, offset, is_first, metadata)?;
            }
        }
        _ => {}
    }

    Ok(())
}

/// Parse video format (BITMAPINFOHEADER)
fn parse_video_format(
    reader: &dyn FileReader,
    offset: u64,
    is_first: bool,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let bih_data = reader.read(offset, 40)?;
    let r = EndianReader::little_endian(bih_data);

    // RIFF.pm routes a video `strf` chunk to BMP::Main.  Keep these names and
    // conversions in the File group; they are not AVI aliases.
    let header_size = r.u32_at(0).unwrap_or(0);
    let width = r.u32_at(4).unwrap_or(0);
    let height = r.i32_at(8).unwrap_or(0).unsigned_abs();
    let planes = r.u16_at(12).unwrap_or(0);
    let bit_count = r.u16_at(14).unwrap_or(0);
    let compression = r.u32_at(16).unwrap_or(0);
    let image_length = r.u32_at(20).unwrap_or(0);
    let pixels_per_meter_x = r.u32_at(24).unwrap_or(0);
    let pixels_per_meter_y = r.u32_at(28).unwrap_or(0);
    let num_colors = r.u32_at(32).unwrap_or(0);
    let num_important_colors = r.u32_at(36).unwrap_or(0);

    let bmp_version = match header_size {
        40 => TagValue::new_string("Windows V3".to_string()),
        68 => TagValue::new_string("AVI BMP structure?".to_string()),
        108 => TagValue::new_string("Windows V4".to_string()),
        124 => TagValue::new_string("Windows V5".to_string()),
        value => TagValue::new_integer(value as i64),
    };
    metadata.insert("File:BMPVersion".to_string(), bmp_version);
    metadata.insert(
        "File:ImageWidth".to_string(),
        TagValue::new_integer(width as i64),
    );
    metadata.insert(
        "File:ImageHeight".to_string(),
        TagValue::new_integer(height as i64),
    );
    metadata.insert(
        "File:Planes".to_string(),
        TagValue::new_integer(planes as i64),
    );
    metadata.insert(
        "File:BitDepth".to_string(),
        TagValue::new_integer(bit_count as i64),
    );

    let compression_value = if compression <= 256 {
        Some(match compression {
            0 => TagValue::new_string("None".to_string()),
            1 => TagValue::new_string("8-Bit RLE".to_string()),
            2 => TagValue::new_string("4-Bit RLE".to_string()),
            3 => TagValue::new_string("Bitfields".to_string()),
            4 => TagValue::new_string("JPEG".to_string()),
            5 => TagValue::new_string("PNG".to_string()),
            value => TagValue::new_integer(value as i64),
        })
    } else if let Ok(fourcc) = std::str::from_utf8(&bih_data[16..20]) {
        Some(TagValue::new_string(
            fourcc.trim_end_matches([' ', '\0']).to_string(),
        ))
    } else {
        // ExifTool's `unpack("A4", ...)` is not safely representable as a Rust
        // string for non-UTF-8 bytes.  Omit rather than fabricate a conversion.
        None
    };
    if let Some(compression_value) = compression_value {
        metadata.insert("File:Compression".to_string(), compression_value);
    }
    metadata.insert(
        "File:ImageLength".to_string(),
        TagValue::new_integer(image_length as i64),
    );
    metadata.insert(
        "File:PixelsPerMeterX".to_string(),
        TagValue::new_integer(pixels_per_meter_x as i64),
    );
    metadata.insert(
        "File:PixelsPerMeterY".to_string(),
        TagValue::new_integer(pixels_per_meter_y as i64),
    );
    metadata.insert(
        "File:NumColors".to_string(),
        if num_colors == 0 {
            TagValue::new_string("Use BitDepth".to_string())
        } else {
            TagValue::new_integer(num_colors as i64)
        },
    );
    metadata.insert(
        "File:NumImportantColors".to_string(),
        if num_important_colors == 0 {
            TagValue::new_string("All".to_string())
        } else {
            TagValue::new_integer(num_important_colors as i64)
        },
    );

    // BitDepth for first video stream
    if is_first && bit_count > 0 {
        metadata.insert(
            "RIFF:BitDepth".to_string(),
            TagValue::new_integer(bit_count as i64),
        );
    }

    Ok(())
}

/// Parse audio format (WAVEFORMATEX)
fn parse_audio_format(
    reader: &dyn FileReader,
    offset: u64,
    is_first: bool,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let wfx_data = reader.read(offset, 16)?;
    let r = EndianReader::little_endian(wfx_data);

    let format_tag = r.u16_at(0).unwrap_or(0);
    let channels = r.u16_at(2).unwrap_or(0);
    let samples_per_sec = r.u32_at(4).unwrap_or(0);
    let avg_bytes_per_sec = r.u32_at(8).unwrap_or(0);
    let bits_per_sample = r.u16_at(14).unwrap_or(0);

    // Only output for first audio stream
    if !is_first {
        return Ok(());
    }

    // Encoding: ExifTool's `%RIFF::audioEncoding`, shared with wav.rs. See
    // `core::formatters::audio_encoding`. The table that used to be inline
    // here disagreed with the one in wav.rs on the same code -- both emit
    // `RIFF:Encoding`, so the same wFormatTag printed differently depending on
    // which container carried it.
    let format_name = audio_encoding_name(format_tag);
    metadata.insert(
        "RIFF:Encoding".to_string(),
        TagValue::new_string(format_name),
    );

    // NumChannels
    metadata.insert(
        "RIFF:NumChannels".to_string(),
        TagValue::new_integer(channels as i64),
    );

    // SampleRate - overwrites the value from strh
    metadata.insert(
        "RIFF:SampleRate".to_string(),
        TagValue::new_integer(samples_per_sec as i64),
    );
    // Also output as AudioSampleRate for explicit audio tag
    metadata.insert(
        "RIFF:AudioSampleRate".to_string(),
        TagValue::new_integer(samples_per_sec as i64),
    );

    // AvgBytesPerSec
    metadata.insert(
        "RIFF:AvgBytesPerSec".to_string(),
        TagValue::new_integer(avg_bytes_per_sec as i64),
    );

    // BitsPerSample
    if bits_per_sample > 0 {
        metadata.insert(
            "RIFF:BitsPerSample".to_string(),
            TagValue::new_integer(bits_per_sample as i64),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;
    use crate::test_support::assert_no_divergent_prefixed_duplicates;

    /// Builds a minimal `RIFF....AVI ` file containing a `hdrl` LIST with one
    /// `avih` chunk and one `strl` LIST per supplied stream header.
    ///
    /// `strh_streams` entries are `(stream_type, codec_fourcc, sample_size)`
    /// triples, e.g. `(b"vids", b"MJPG", 0)`.
    fn synthetic_avi(
        microsec_per_frame: u32,
        total_frames: u32,
        strh_streams: &[(&[u8; 4], &[u8; 4], u32)],
    ) -> Vec<u8> {
        fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = id.to_vec();
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(body);
            out
        }

        let mut avih = vec![0u8; 56];
        avih[0..4].copy_from_slice(&microsec_per_frame.to_le_bytes()); // dwMicroSecPerFrame
        avih[16..20].copy_from_slice(&total_frames.to_le_bytes()); // dwTotalFrames
        avih[32..36].copy_from_slice(&8u32.to_le_bytes()); // dwWidth
        avih[36..40].copy_from_slice(&8u32.to_le_bytes()); // dwHeight

        let mut hdrl = b"hdrl".to_vec();
        hdrl.extend_from_slice(&chunk(b"avih", &avih));
        for (stream_type, codec_fourcc, sample_size) in strh_streams {
            let mut strh = vec![0u8; 56];
            strh[0..4].copy_from_slice(*stream_type); // fccType
            strh[4..8].copy_from_slice(*codec_fourcc); // fccHandler
            strh[44..48].copy_from_slice(&sample_size.to_le_bytes()); // dwSampleSize
            let mut strl = b"strl".to_vec();
            strl.extend_from_slice(&chunk(b"strh", &strh));
            hdrl.extend_from_slice(&chunk(b"LIST", &strl));
        }

        let mut riff_body = b"AVI ".to_vec();
        riff_body.extend_from_slice(&chunk(b"LIST", &hdrl));
        chunk(b"RIFF", &riff_body)
    }

    /// Builds an AVI containing a first video stream with a BITMAPINFOHEADER
    /// `strf` chunk.  RIFF.pm dispatches this exact chunk to BMP::Main.
    fn synthetic_avi_with_bitmap_info_header(bitmap_info: &[u8]) -> Vec<u8> {
        fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = id.to_vec();
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(body);
            out
        }

        let mut avih = vec![0u8; 56];
        avih[0..4].copy_from_slice(&66_667u32.to_le_bytes());
        avih[16..20].copy_from_slice(&233u32.to_le_bytes());
        avih[24..28].copy_from_slice(&2u32.to_le_bytes());
        avih[32..36].copy_from_slice(&320u32.to_le_bytes());
        avih[36..40].copy_from_slice(&240u32.to_le_bytes());

        let mut strh = vec![0u8; 56];
        strh[0..4].copy_from_slice(b"vids");
        strh[4..8].copy_from_slice(b"mjpg");
        strh[20..24].copy_from_slice(&1u32.to_le_bytes());
        strh[24..28].copy_from_slice(&15u32.to_le_bytes());
        strh[32..36].copy_from_slice(&233u32.to_le_bytes());

        let mut strl = b"strl".to_vec();
        strl.extend_from_slice(&chunk(b"strh", &strh));
        strl.extend_from_slice(&chunk(b"strf", bitmap_info));

        let mut hdrl = b"hdrl".to_vec();
        hdrl.extend_from_slice(&chunk(b"avih", &avih));
        hdrl.extend_from_slice(&chunk(b"LIST", &strl));

        let mut riff_body = b"AVI ".to_vec();
        riff_body.extend_from_slice(&chunk(b"LIST", &hdrl));
        chunk(b"RIFF", &riff_body)
    }

    #[test]
    fn video_bitmap_info_header_matches_exiftool_bmp_main_fields() {
        // This is the BITMAPINFOHEADER from ExifTool 13.59's RIFF.avi fixture:
        // `strf` is routed to BMP::Main by RIFF.pm, not an AVI-specific table.
        let mut bih = vec![0u8; 40];
        bih[0..4].copy_from_slice(&40u32.to_le_bytes());
        bih[4..8].copy_from_slice(&320u32.to_le_bytes());
        bih[8..12].copy_from_slice(&240i32.to_le_bytes());
        bih[12..14].copy_from_slice(&1u16.to_le_bytes());
        bih[14..16].copy_from_slice(&24u16.to_le_bytes());
        bih[16..20].copy_from_slice(b"MJPG");
        bih[20..24].copy_from_slice(&230_400u32.to_le_bytes());

        let reader = TestReader::new(synthetic_avi_with_bitmap_info_header(&bih));
        let metadata = parse_avi_metadata(&reader).unwrap();

        for (tag, expected) in [
            (
                "File:BMPVersion",
                TagValue::String("Windows V3".to_string()),
            ),
            ("File:ImageWidth", TagValue::Integer(320)),
            ("File:ImageHeight", TagValue::Integer(240)),
            ("File:Planes", TagValue::Integer(1)),
            ("File:BitDepth", TagValue::Integer(24)),
            ("File:Compression", TagValue::String("MJPG".to_string())),
            ("File:ImageLength", TagValue::Integer(230_400)),
            ("File:PixelsPerMeterX", TagValue::Integer(0)),
            ("File:PixelsPerMeterY", TagValue::Integer(0)),
            (
                "File:NumColors",
                TagValue::String("Use BitDepth".to_string()),
            ),
            (
                "File:NumImportantColors",
                TagValue::String("All".to_string()),
            ),
        ] {
            assert_eq!(metadata.get(tag), Some(&expected), "{tag}");
        }
    }

    /// ExifTool 13.59 maps AVI through `RIFF.pm:50` and dispatches its
    /// `RIFF::Main` table at `RIFF.pm:338-339`; there is no AVI family-1
    /// override, so oxidex must emit these under `RIFF:` and nothing under
    /// `AVI:`:
    ///
    /// ```text
    /// $ exiftool -G1 -FrameRate -Duration -VideoCodec -AudioCodec \
    ///     /tmp/oxidex-exiftool-cache/combined-samples/RIFF.avi
    /// [RIFF]          Frame Rate                      : 15
    /// [Composite]     Duration                        : 15.53 s
    /// [RIFF]          Video Codec                     : mjpg
    /// [RIFF]          Audio Codec                     :
    /// ```
    ///
    /// Until this test's predecessor was retired, avih/strh/strf each inserted
    /// an `AVI:` alias immediately after the `RIFF:` value it had just written
    /// -- a same-chunk double insert, not a second provenance. The alias is
    /// gone; what survives is the pinning of the `RIFF:` side, so a future
    /// re-derivation cannot reintroduce the divergent rendering under either
    /// name.
    ///
    /// 40000 us/frame is 25 fps and 391 frames is 15.64 s, chosen so the old
    /// renderings ("25.000 fps", "0:16") differ from the correct ones ("25",
    /// "15.64") in both digits and shape.
    #[test]
    fn synthetic_avi_pins_riff_renderings_and_emits_no_avi_group() {
        let data = synthetic_avi(40_000, 391, &[(b"vids", b"MJPG", 0), (b"auds", b"VORB", 0)]);
        let reader = TestReader::new(data);
        let metadata = parse_avi_metadata(&reader).unwrap();

        assert_eq!(
            metadata.get("RIFF:FrameRate"),
            Some(&TagValue::String("25".to_string())),
        );
        assert_eq!(
            metadata.get("RIFF:Duration"),
            Some(&TagValue::String("15.64".to_string())),
        );
        assert_eq!(
            metadata.get("RIFF:VideoCodec"),
            Some(&TagValue::String("MJPG".to_string())),
            "ExifTool prints the raw FourCC, not a friendly codec name",
        );
        assert!(
            metadata.get("RIFF:AudioCodec").is_some(),
            "an auds strh must still emit RIFF:AudioCodec",
        );

        let avi_keys: Vec<_> = metadata
            .iter()
            .map(|(key, _)| key.clone())
            .filter(|key| key.starts_with("AVI:"))
            .collect();
        assert!(
            avi_keys.is_empty(),
            "ExifTool has no AVI family-1 group; fabricated aliases returned: {avi_keys:?}",
        );

        assert_no_divergent_prefixed_duplicates(&metadata);
    }

    #[test]
    fn stream_header_retains_video_and_audio_stream_type_and_sample_size() {
        // Pinned against ExifTool 13.59's RIFF.pm:1165-1176 and :1243-1245:
        // `PRIORITY => 0` keeps the video stream in the ordinary projection,
        // while -a must retain both streams. Zero prints "Variable" and a
        // nonzero audio dwSampleSize prints its exact byte count.
        let data = synthetic_avi(
            40_000,
            391,
            &[(b"vids", b"MJPG", 0), (b"auds", b"\0\0\0\0", 2)],
        );
        let metadata = parse_avi_metadata(&TestReader::new(data)).unwrap();

        assert_eq!(
            metadata.get("RIFF:StreamType"),
            Some(&TagValue::new_string("Video")),
            "priority 0 preserves the first stream in the normal view",
        );
        assert_eq!(
            metadata.get("RIFF:SampleSize"),
            Some(&TagValue::new_string("Variable")),
            "priority 0 preserves the first stream in the normal view",
        );

        let stream_types: Vec<_> = metadata
            .occurrences_for("RIFF:StreamType")
            .into_iter()
            .map(|occurrence| occurrence.raw.clone())
            .collect();
        assert_eq!(
            stream_types,
            vec![TagValue::new_string("Video"), TagValue::new_string("Audio"),],
            "-a must expose both AVISTREAMHEADER stream types in file order",
        );

        let sample_sizes: Vec<_> = metadata
            .occurrences_for("RIFF:SampleSize")
            .into_iter()
            .map(|occurrence| occurrence.raw.clone())
            .collect();
        assert_eq!(
            sample_sizes,
            vec![
                TagValue::new_string("Variable"),
                TagValue::new_string("2 bytes"),
            ],
            "-a must expose both AVISTREAMHEADER sample sizes in file order",
        );
    }

    #[test]
    fn test_avi_signature_valid() {
        // Minimal AVI file structure
        let mut data = vec![0u8; 100];
        data[0..4].copy_from_slice(b"RIFF");
        data[4..8].copy_from_slice(&100u32.to_le_bytes());
        data[8..12].copy_from_slice(b"AVI ");

        let reader = TestReader::from_slice(&data);
        let parser = AviParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());
    }

    #[test]
    fn test_avi_signature_invalid_riff() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = AviParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_avi_signature_invalid_avi() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(b"RIFF");
        data[4..8].copy_from_slice(&100u32.to_le_bytes());
        data[8..12].copy_from_slice(b"WAVE"); // Wrong format type

        let reader = TestReader::from_slice(&data);
        let parser = AviParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_avi_file_too_small() {
        let data = b"RIFF";
        let reader = TestReader::from_slice(data);
        let parser = AviParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
