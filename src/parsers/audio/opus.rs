//! Opus audio format parser
//!
//! Implements metadata extraction from Opus audio files in Ogg containers,
//! parsing OpusHead and OpusTags packets.
//!
//! # Supported Metadata
//!
//! - **OpusHead:** Version, channel count, pre-skip, sample rate, output gain
//! - **OpusTags:** Vorbis comment tags (ARTIST, ALBUM, TITLE, etc.)
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `Opus.pm` module:
//! - `Opus:Version` → Version from OpusHead
//! - `Opus:Channels` → Channel count from OpusHead
//! - `Opus:SampleRate` → Input sample rate from OpusHead
//!
//! # File Structure
//!
//! ```text
//! [OggS page 0 - OpusHead]
//!   └─ "OpusHead" + version + channels + pre-skip + sample rate + gain
//! [OggS page 1 - OpusTags]
//!   └─ "OpusTags" + vendor + comments (Vorbis comment format)
//! [OggS page 2+ - Audio data]
//! ```
//!
//! # References
//!
//! - RFC 7845: Ogg Encapsulation for Opus Audio Codec
//! - ExifTool Source: `lib/Image/ExifTool/Ogg.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use encoding_rs::UTF_8;

/// OGG page signature
const OGG_SIGNATURE: &[u8] = b"OggS";

/// OpusHead packet identifier
const OPUS_HEAD: &[u8] = b"OpusHead";

/// OpusTags packet identifier
const OPUS_TAGS: &[u8] = b"OpusTags";

/// Opus parser
pub struct OpusParser;

/// Parses metadata from an Opus file.
///
/// This is a convenience wrapper that creates an OpusParser instance and calls parse().
///
/// # Arguments
///
/// * `reader` - File reader providing access to the Opus file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_opus_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = OpusParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

impl FormatParser for OpusParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify OGG signature
        if reader.size() < 4 {
            return Err(ExifToolError::parse_error("File too small to be Opus"));
        }

        let header = reader.read(0, 4)?;
        if header != OGG_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid OGG signature: expected {:?}, found {:?}",
                OGG_SIGNATURE, header
            )));
        }

        let mut metadata = MetadataMap::with_capacity(16);

        // Parse OGG pages to find OpusHead and OpusTags
        let mut offset = 0u64;
        let file_size = reader.size();
        let mut found_opus_head = false;

        while offset < file_size {
            // Read OGG page header (27 bytes minimum)
            if offset + 27 > file_size {
                break;
            }

            let page_header = reader.read(offset, 27)?;

            // Verify page signature
            if &page_header[0..4] != OGG_SIGNATURE {
                break;
            }

            // Parse page header
            let segment_count = page_header[26] as usize;

            // Read segment table
            if offset + 27 + segment_count as u64 > file_size {
                break;
            }
            let segment_table = reader.read(offset + 27, segment_count)?;

            // Calculate total page size
            let mut page_body_size = 0u64;
            for &segment_size in segment_table.iter() {
                page_body_size += segment_size as u64;
            }

            // Read page body
            let page_body_offset = offset + 27 + segment_count as u64;
            if page_body_offset + page_body_size > file_size {
                break;
            }

            // Check if this is an Opus packet
            if page_body_size >= 8 {
                let page_body = reader.read(page_body_offset, page_body_size.min(256) as usize)?;

                // Check for OpusHead
                if page_body.len() >= 19 && &page_body[0..8] == OPUS_HEAD {
                    parse_opus_head(&page_body[8..], &mut metadata)?;
                    found_opus_head = true;
                }
                // Check for OpusTags
                else if page_body.len() >= 8
                    && &page_body[0..8] == OPUS_TAGS
                    && page_body_size <= 1_000_000
                {
                    // Safety limit
                    let full_tags =
                        reader.read(page_body_offset + 8, (page_body_size - 8) as usize)?;
                    parse_opus_tags(full_tags, &mut metadata)?;
                    break; // Found both head and tags, we're done
                }
            }

            // Move to next page
            offset = page_body_offset + page_body_size;
        }

        if !found_opus_head {
            return Err(ExifToolError::parse_error(
                "Invalid Opus file: OpusHead packet not found",
            ));
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::OPUS)
    }
}

/// Parse OpusHead packet (11 bytes minimum after "OpusHead")
fn parse_opus_head(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.len() < 11 {
        return Err(ExifToolError::parse_error("OpusHead packet too small"));
    }

    let reader = EndianReader::little_endian(data);
    let version = reader.u8_at(0).unwrap_or(0);
    let channels = reader.u8_at(1).unwrap_or(0);
    let pre_skip = reader.u16_at(2).unwrap_or(0);
    let sample_rate = reader.u32_at(4).unwrap_or(0);
    let output_gain = reader.i16_at(8).unwrap_or(0);
    let channel_mapping_family = reader.u8_at(10).unwrap_or(0);

    metadata.insert(
        "Opus:Version".to_string(),
        TagValue::new_integer(version as i64),
    );
    metadata.insert(
        "Opus:Channels".to_string(),
        TagValue::new_integer(channels as i64),
    );
    metadata.insert(
        "Opus:PreSkip".to_string(),
        TagValue::new_integer(pre_skip as i64),
    );
    metadata.insert(
        "Opus:SampleRate".to_string(),
        TagValue::new_integer(sample_rate as i64),
    );
    metadata.insert(
        "Opus:OutputGain".to_string(),
        TagValue::new_integer(output_gain as i64),
    );
    metadata.insert(
        "Opus:ChannelMappingFamily".to_string(),
        TagValue::new_integer(channel_mapping_family as i64),
    );

    Ok(())
}

/// Parse OpusTags packet (Vorbis comment format)
fn parse_opus_tags(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    let mut offset = 0;
    let reader = EndianReader::little_endian(data);

    // Vendor string length (4 bytes, little-endian)
    if data.len() < 4 {
        return Err(ExifToolError::parse_error("OpusTags packet too small"));
    }

    let vendor_length = reader.u32_at(offset).unwrap_or(0) as usize;
    offset += 4;

    // Skip vendor string
    if offset + vendor_length > data.len() {
        return Err(ExifToolError::parse_error("Invalid vendor string length"));
    }
    offset += vendor_length;

    // User comment list length (4 bytes, little-endian)
    if offset + 4 > data.len() {
        return Err(ExifToolError::parse_error("Missing comment list length"));
    }

    let comment_count = reader.u32_at(offset).unwrap_or(0);
    offset += 4;

    // Safety limit
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

            // Map to Opus: prefix (following Vorbis comment convention)
            let tag_name = format!("Opus:{}", field_name.to_uppercase());
            metadata.insert(tag_name, TagValue::new_string(field_value.to_string()));
        }

        offset += comment_length;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_opus_signature_valid() {
        // Minimal Opus file with OpusHead
        let mut data = vec![0u8; 200];

        // OGG page header
        data[0..4].copy_from_slice(b"OggS");
        data[5] = 0x02; // BOS flag
        data[26] = 1; // 1 segment

        // Segment table
        data[27] = 19; // OpusHead packet size

        // OpusHead packet
        let opus_head_offset = 28;
        data[opus_head_offset..opus_head_offset + 8].copy_from_slice(b"OpusHead");
        data[opus_head_offset + 8] = 1; // version
        data[opus_head_offset + 9] = 2; // channels
        data[opus_head_offset + 10..opus_head_offset + 12].copy_from_slice(&312u16.to_le_bytes()); // pre-skip
        data[opus_head_offset + 12..opus_head_offset + 16].copy_from_slice(&48000u32.to_le_bytes()); // sample rate
        data[opus_head_offset + 16..opus_head_offset + 18].copy_from_slice(&0i16.to_le_bytes()); // output gain
        data[opus_head_offset + 18] = 0; // channel mapping family

        let reader = TestReader::from_slice(&data);
        let parser = OpusParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get("Opus:Version").unwrap().as_integer(), Some(1));
        assert_eq!(metadata.get("Opus:Channels").unwrap().as_integer(), Some(2));
    }

    #[test]
    fn test_opus_signature_invalid() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = OpusParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_opus_file_too_small() {
        let data = b"Ogg";
        let reader = TestReader::from_slice(data);
        let parser = OpusParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
