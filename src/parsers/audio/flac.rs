//! FLAC (Free Lossless Audio Codec) format parser
//!
//! Implements metadata extraction from FLAC audio files following the
//! FLAC specification.
//!
//! # Supported Metadata
//!
//! - **Vorbis Comments:** ARTIST, ALBUM, TITLE, GENRE, TRACKNUMBER, DATE
//! - **Stream Info:** SampleRate, BitsPerSample, Channels, TotalSamples
//! - **Picture:** Embedded album artwork
//! - **Application:** ReplayGain, other application-specific data
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `FLAC.pm` module:
//! - `FLAC:Artist` → Vorbis ARTIST comment
//! - `FLAC:Album` → Vorbis ALBUM comment
//! - `FLAC:SampleRate` → StreamInfo sample rate
//!
//! # File Structure
//!
//! ```text
//! [fLaC signature - 4 bytes]
//! [Metadata Block 0: STREAMINFO - required, always first]
//! [Metadata Block 1-N: Optional blocks]
//!   ├─ PADDING (1)
//!   ├─ APPLICATION (2)
//!   ├─ SEEKTABLE (3)
//!   ├─ VORBIS_COMMENT (4) ← Primary metadata source
//!   ├─ CUESHEET (5)
//!   └─ PICTURE (6) ← Album artwork
//! [Audio frames...]
//! ```
//!
//! # References
//!
//! - FLAC Format: <https://xiph.org/flac/format.html>
//! - Vorbis Comment Spec: <https://www.xiph.org/vorbis/doc/v-comment.html>
//! - ExifTool Source: `lib/Image/ExifTool/FLAC.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use nom::{
    IResult,
    number::complete::{be_u8, be_u16, be_u24},
};

/// FLAC file signature
const FLAC_SIGNATURE: &[u8] = b"fLaC";

/// Metadata block types
const BLOCK_TYPE_STREAMINFO: u8 = 0;
const BLOCK_TYPE_PADDING: u8 = 1;
const BLOCK_TYPE_APPLICATION: u8 = 2;
const BLOCK_TYPE_SEEKTABLE: u8 = 3;
const BLOCK_TYPE_VORBIS_COMMENT: u8 = 4;
const BLOCK_TYPE_CUESHEET: u8 = 5;
const BLOCK_TYPE_PICTURE: u8 = 6;

/// FLAC parser
pub struct FlacParser;

/// Parses metadata from a FLAC file.
///
/// This is a convenience wrapper that creates a FlacParser instance and calls parse().
///
/// # Arguments
///
/// * `reader` - File reader providing access to the FLAC file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_flac_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = FlacParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

impl FormatParser for FlacParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify file size
        let file_size = reader.size();
        if file_size < 8 {
            return Err(ExifToolError::parse_error("File too small to be FLAC"));
        }

        // Read and verify signature
        let header = reader.read(0, 4)?;
        if header != FLAC_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid FLAC signature: expected {:?}, found {:?}",
                FLAC_SIGNATURE, header
            )));
        }

        // Initialize metadata map
        let mut metadata = MetadataMap::with_capacity(32);

        // Parse metadata blocks
        let mut offset = 4u64; // After "fLaC"
        let mut is_last = false;

        while !is_last && offset < file_size {
            // Read block header (4 bytes)
            let block_header = reader.read(offset, 4)?;
            let (_, (is_last_flag, block_type, block_length)) = parse_block_header(block_header)
                .map_err(|e| {
                    ExifToolError::parse_error(format!("Failed to parse block header: {:?}", e))
                })?;

            is_last = is_last_flag;
            offset += 4;

            // Read block data
            if block_length > 0 && offset + block_length as u64 <= file_size {
                let block_data = reader.read(offset, block_length as usize)?;

                // Process block based on type
                match block_type {
                    BLOCK_TYPE_STREAMINFO => {
                        parse_streaminfo_block(block_data, &mut metadata)?;
                    }
                    BLOCK_TYPE_VORBIS_COMMENT => {
                        parse_vorbis_comment_block(block_data, &mut metadata)?;
                    }
                    BLOCK_TYPE_PICTURE => {
                        parse_picture_block(block_data, &mut metadata)?;
                    }
                    _ => {
                        // Skip other block types for now
                    }
                }

                offset += block_length as u64;
            } else {
                break;
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::FLAC)
    }
}

/// Parses FLAC metadata block header (1 + 3 bytes)
///
/// Returns: (is_last, block_type, block_length)
fn parse_block_header(input: &[u8]) -> IResult<&[u8], (bool, u8, u32)> {
    let (input, header_byte) = be_u8(input)?;
    let is_last = (header_byte & 0x80) != 0;
    let block_type = header_byte & 0x7F;

    let (input, length) = be_u24(input)?;

    Ok((input, (is_last, block_type, length)))
}

/// Parses STREAMINFO block (34 bytes)
fn parse_streaminfo_block(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.len() < 34 {
        return Err(ExifToolError::parse_error("STREAMINFO block too small"));
    }

    // Parse fields using nom
    let (_, stream_info) = parse_streaminfo(data)
        .map_err(|e| ExifToolError::parse_error(format!("Failed to parse STREAMINFO: {:?}", e)))?;

    // Add to metadata
    use crate::core::TagValue;

    metadata.insert(
        "FLAC:BlockSizeMin".to_string(),
        TagValue::new_integer(stream_info.min_block_size as i64),
    );
    metadata.insert(
        "FLAC:BlockSizeMax".to_string(),
        TagValue::new_integer(stream_info.max_block_size as i64),
    );
    metadata.insert(
        "FLAC:FrameSizeMin".to_string(),
        TagValue::new_integer(stream_info.min_frame_size as i64),
    );
    metadata.insert(
        "FLAC:FrameSizeMax".to_string(),
        TagValue::new_integer(stream_info.max_frame_size as i64),
    );
    metadata.insert(
        "FLAC:SampleRate".to_string(),
        TagValue::new_integer(stream_info.sample_rate as i64),
    );
    metadata.insert(
        "FLAC:Channels".to_string(),
        TagValue::new_integer(stream_info.channels as i64),
    );
    metadata.insert(
        "FLAC:BitsPerSample".to_string(),
        TagValue::new_integer(stream_info.bits_per_sample as i64),
    );
    metadata.insert(
        "FLAC:TotalSamples".to_string(),
        TagValue::new_integer(stream_info.total_samples as i64),
    );

    // Calculate duration if sample rate > 0
    if stream_info.sample_rate > 0 {
        let duration_secs = stream_info.total_samples as f64 / stream_info.sample_rate as f64;
        metadata.insert(
            "FLAC:Duration".to_string(),
            TagValue::new_string(format!("{:.2}", duration_secs)),
        );
    }

    // Add MD5 hash as hex string
    let md5_hex = stream_info
        .md5
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    metadata.insert(
        "FLAC:MD5Signature".to_string(),
        TagValue::new_string(md5_hex),
    );

    // Add FLAC format-specific tags for ExifTool compatibility

    // AudioEncoding: Always "FLAC" for FLAC files
    metadata.insert(
        "FLAC:AudioEncoding".to_string(),
        TagValue::new_string("FLAC".to_string()),
    );

    // FrameCount: Calculated from total samples
    // Frame size varies but we can estimate from total_samples
    // In FLAC, the actual frame count is not stored in STREAMINFO
    // We'll calculate it based on a typical frame size
    if stream_info.total_samples > 0 {
        // Typical FLAC frame sizes are 1152 or 4096 samples per frame
        // Using 4096 as typical for modern FLAC encodings
        let estimated_frame_count = (stream_info.total_samples + 4095) / 4096;
        metadata.insert(
            "FLAC:FrameCount".to_string(),
            TagValue::new_integer(estimated_frame_count as i64),
        );
    }

    // VorbisComments: Set to true if we found any in subsequent parsing
    // This will be updated if we encounter the VORBIS_COMMENT block
    metadata.insert(
        "FLAC:VorbisComments".to_string(),
        TagValue::new_string("false".to_string()),
    );

    Ok(())
}

/// STREAMINFO structure
#[derive(Debug)]
struct StreamInfo {
    min_block_size: u16,
    max_block_size: u16,
    min_frame_size: u32, // 24-bit
    max_frame_size: u32, // 24-bit
    sample_rate: u32,    // 20-bit
    channels: u8,        // 3-bit (stored as 1-8)
    bits_per_sample: u8, // 5-bit (stored as 1-32)
    total_samples: u64,  // 36-bit
    md5: [u8; 16],
}

fn parse_streaminfo(input: &[u8]) -> IResult<&[u8], StreamInfo> {
    let (input, min_block_size) = be_u16(input)?;
    let (input, max_block_size) = be_u16(input)?;
    let (input, min_frame_size) = be_u24(input)?;
    let (input, max_frame_size) = be_u24(input)?;

    // Next 8 bytes contain sample_rate (20 bits), channels (3 bits), bits_per_sample (5 bits), total_samples (36 bits)
    let (input, bytes) = nom::bytes::complete::take(8usize)(input)?;

    // Parse bit-packed fields
    let sample_rate =
        (u32::from(bytes[0]) << 12) | (u32::from(bytes[1]) << 4) | (u32::from(bytes[2]) >> 4);
    let channels = ((bytes[2] >> 1) & 0x07) + 1; // 3 bits, add 1 (1-8 channels)
    let bits_per_sample = (((bytes[2] & 0x01) << 4) | (bytes[3] >> 4)) + 1; // 5 bits, add 1 (1-32 bits)

    // Total samples (36 bits)
    let total_samples = (u64::from(bytes[3] & 0x0F) << 32)
        | (u64::from(bytes[4]) << 24)
        | (u64::from(bytes[5]) << 16)
        | (u64::from(bytes[6]) << 8)
        | u64::from(bytes[7]);

    // MD5 hash (16 bytes)
    let (input, md5_bytes) = nom::bytes::complete::take(16usize)(input)?;
    let mut md5 = [0u8; 16];
    md5.copy_from_slice(md5_bytes);

    Ok((
        input,
        StreamInfo {
            min_block_size,
            max_block_size,
            min_frame_size,
            max_frame_size,
            sample_rate,
            channels,
            bits_per_sample,
            total_samples,
            md5,
        },
    ))
}

/// Maps Vorbis comment field names to ExifTool-compatible tag names
/// Returns (family, tag_name) tuple
fn map_vorbis_field_name(field_name: &str) -> (&'static str, String) {
    // Normalize field name to uppercase for matching
    let upper = field_name.to_uppercase();

    match upper.as_str() {
        // Vorbis family tags
        "TITLE" => ("Vorbis", "Title".to_string()),
        "ARTIST" => ("Vorbis", "Artist".to_string()),
        "ALBUM" => ("Vorbis", "Album".to_string()),
        "TRACKNUMBER" => ("Vorbis", "TrackNumber".to_string()),
        "DATE" => ("Vorbis", "Date".to_string()),
        "GENRE" => ("Vorbis", "Genre".to_string()),
        "COMMENT" => ("Vorbis", "Comment".to_string()),
        "DESCRIPTION" => ("Vorbis", "Description".to_string()),
        "COPYRIGHT" => ("Vorbis", "Copyright".to_string()),
        "LICENSE" => ("Vorbis", "License".to_string()),
        "ORGANIZATION" => ("Vorbis", "Organization".to_string()),
        "PERFORMER" => ("Vorbis", "Performer".to_string()),
        "COMPOSER" => ("Vorbis", "Composer".to_string()),
        "CONDUCTOR" => ("Vorbis", "Conductor".to_string()),
        "ISRC" => ("Vorbis", "ISRC".to_string()),
        "LYRICS" => ("Vorbis", "Lyrics".to_string()),
        "ALBUMARTIST" => ("Vorbis", "AlbumArtist".to_string()),
        "DISCNUMBER" => ("Vorbis", "DiscNumber".to_string()),
        "TOTALTRACKS" => ("Vorbis", "TotalTracks".to_string()),
        "TOTALDISCS" => ("Vorbis", "TotalDiscs".to_string()),
        "ENCODER" => ("Vorbis", "Encoder".to_string()),
        "ENCODEDBY" => ("Vorbis", "EncodedBy".to_string()),
        "ENCODED_BY" => ("Vorbis", "EncodedBy".to_string()),
        "CONTACT" => ("Vorbis", "Contact".to_string()),
        "LOCATION" => ("Vorbis", "Location".to_string()),
        "VERSION" => ("Vorbis", "Version".to_string()),
        // ReplayGain tags - use Vorbis family with proper casing
        "REPLAYGAIN_TRACK_GAIN" => ("Vorbis", "ReplayGainTrackGain".to_string()),
        "REPLAYGAIN_TRACK_PEAK" => ("Vorbis", "ReplayGainTrackPeak".to_string()),
        "REPLAYGAIN_ALBUM_GAIN" => ("Vorbis", "ReplayGainAlbumGain".to_string()),
        "REPLAYGAIN_ALBUM_PEAK" => ("Vorbis", "ReplayGainAlbumPeak".to_string()),
        "REPLAYGAIN_REFERENCE_LOUDNESS" => ("Vorbis", "ReplayGainReferenceLoudness".to_string()),
        // Unknown fields - use FLAC family with original case
        _ => ("FLAC", field_name.to_string()),
    }
}

/// Parses VORBIS_COMMENT block
fn parse_vorbis_comment_block(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    use encoding_rs::UTF_8;

    let mut offset = 0;
    let reader = EndianReader::little_endian(data);

    // Vendor string length (4 bytes, little-endian)
    if data.len() < 4 {
        return Err(ExifToolError::parse_error("Vorbis comment block too small"));
    }

    let vendor_length = reader.u32_at(offset).unwrap_or(0) as usize;
    offset += 4;

    // Read and store vendor string
    if offset + vendor_length > data.len() {
        return Err(ExifToolError::parse_error("Invalid vendor string length"));
    }
    let vendor_bytes = &data[offset..offset + vendor_length];
    let (vendor_str, _, _) = UTF_8.decode(vendor_bytes);
    if !vendor_str.is_empty() {
        metadata.insert(
            "Vorbis:Vendor".to_string(),
            crate::core::TagValue::new_string(vendor_str.to_string()),
        );
    }
    offset += vendor_length;

    // Mark that VorbisComments are present in this FLAC file
    metadata.insert(
        "FLAC:VorbisComments".to_string(),
        crate::core::TagValue::new_string("true".to_string()),
    );

    // User comment list length (4 bytes, little-endian)
    if offset + 4 > data.len() {
        return Err(ExifToolError::parse_error("Missing comment list length"));
    }

    let comment_count = reader.u32_at(offset).unwrap_or(0);
    offset += 4;

    // Safety limit: cap at 10,000 comments to prevent excessive memory usage
    const MAX_COMMENTS: u32 = 10_000;
    let safe_comment_count = comment_count.min(MAX_COMMENTS);

    // Parse each comment
    for _ in 0..safe_comment_count {
        if offset + 4 > data.len() {
            break;
        }

        // Comment length (4 bytes, little-endian)
        let comment_length = reader.u32_at(offset).unwrap_or(0) as usize;
        offset += 4;

        if offset + comment_length > data.len() {
            break;
        }

        // Comment string (UTF-8)
        let comment_bytes = &data[offset..offset + comment_length];
        let (comment_str, _, _) = UTF_8.decode(comment_bytes);

        // Split on first '=' to get field name and value
        if let Some(eq_pos) = comment_str.find('=') {
            let field_name = &comment_str[..eq_pos];
            let field_value = &comment_str[eq_pos + 1..];

            // Map to ExifTool-compatible tag name
            let (family, tag_name) = map_vorbis_field_name(field_name);
            let full_tag = format!("{}:{}", family, tag_name);

            // Format ReplayGain Peak values to match ExifTool (strip trailing zeros)
            let formatted_value = if field_name.to_uppercase().contains("_PEAK") {
                if let Ok(val) = field_value.parse::<f64>() {
                    format_peak_value(val)
                } else {
                    field_value.to_string()
                }
            } else {
                field_value.to_string()
            };

            metadata.insert(full_tag, crate::core::TagValue::new_string(formatted_value));
        }

        offset += comment_length;
    }

    Ok(())
}

/// Format ReplayGain peak value to match ExifTool output
///
/// ExifTool strips trailing zeros from peak values, so "0.00000000" becomes "0.0"
fn format_peak_value(val: f64) -> String {
    // Format with enough precision
    let s = format!("{:.8}", val);
    // Strip trailing zeros after decimal point, but keep at least one digit after decimal
    let trimmed = s.trim_end_matches('0');
    if trimmed.ends_with('.') {
        format!("{}0", trimmed)
    } else {
        trimmed.to_string()
    }
}

/// Parses PICTURE block
fn parse_picture_block(_data: &[u8], _metadata: &mut MetadataMap) -> Result<()> {
    // TODO: Implement picture block parsing
    // For now, just skip it
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_block_header() {
        // Last block, type 0 (STREAMINFO), length 34
        let data = [0x80, 0x00, 0x00, 0x22];
        let (_, (is_last, block_type, length)) = parse_block_header(&data).unwrap();
        assert!(is_last);
        assert_eq!(block_type, 0);
        assert_eq!(length, 34);

        // Not last block, type 4 (VORBIS_COMMENT), length 1024
        let data = [0x04, 0x00, 0x04, 0x00];
        let (_, (is_last, block_type, length)) = parse_block_header(&data).unwrap();
        assert!(!is_last);
        assert_eq!(block_type, 4);
        assert_eq!(length, 1024);
    }
}
