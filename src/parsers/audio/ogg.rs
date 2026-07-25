//! OGG Vorbis audio format parser
//!
//! Implements metadata extraction from OGG Vorbis audio files,
//! parsing Vorbis comments embedded in the container.
//!
//! # Supported Metadata
//!
//! - **Vorbis Comments:** ARTIST, ALBUM, TITLE, GENRE, TRACKNUMBER, DATE
//! - **Audio Info:** Channels, sample rate, bitrate
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `Ogg.pm` module:
//! - `Vorbis:Artist` → ARTIST comment
//! - `Vorbis:Album` → ALBUM comment
//! - `Vorbis:Title` → TITLE comment
//!
//! # File Structure
//!
//! ```text
//! [OggS page 0 - Identification header]
//! [OggS page 1 - Comment header with Vorbis comments]
//! [OggS page 2+ - Setup header and audio data]
//! ```
//!
//! # References
//!
//! - Ogg Spec: <https://xiph.org/ogg/doc/framing.html>
//! - Vorbis Comment Spec: <https://www.xiph.org/vorbis/doc/v-comment.html>
//! - ExifTool Source: `lib/Image/ExifTool/Ogg.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use encoding_rs::UTF_8;

/// OGG page signature
const OGG_SIGNATURE: &[u8] = b"OggS";

/// Vorbis identification header packet type
const VORBIS_ID_HEADER: u8 = 0x01;

/// Vorbis comment header packet type
const VORBIS_COMMENT_HEADER: u8 = 0x03;

/// OGG FLAC mapping header marker
const OGG_FLAC_MARKER: u8 = 0x7F;

/// FLAC metadata block type for Vorbis Comment
const FLAC_METADATA_VORBIS_COMMENT: u8 = 4;

/// OGG parser
pub struct OggParser;

/// Parses metadata from an Ogg Vorbis file.
///
/// This is a convenience wrapper that creates an OggParser instance and calls parse().
///
/// # Arguments
///
/// * `reader` - File reader providing access to the Ogg file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_ogg_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = OggParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

impl FormatParser for OggParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify OGG signature
        if reader.size() < 4 {
            return Err(ExifToolError::parse_error("File too small to be OGG"));
        }

        let header = reader.read(0, 4)?;
        if header != OGG_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid OGG signature: expected {:?}, found {:?}",
                OGG_SIGNATURE, header
            )));
        }

        let mut metadata = MetadataMap::with_capacity(16);

        // Parse OGG pages to find Vorbis comment header
        let mut offset = 0u64;
        let file_size = reader.size();
        let mut page_sequence = 0u32;
        let mut serial_number: Option<u32> = None;

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
            let _header_type = page_header[5];
            let segment_count = page_header[26] as usize;

            // Extract serial number from page header (bytes 10-13, little-endian)
            let page_serial = u32::from_le_bytes([
                page_header[10],
                page_header[11],
                page_header[12],
                page_header[13],
            ]);

            // Extract page sequence number (bytes 18-21, little-endian)
            let seq_number = u32::from_le_bytes([
                page_header[18],
                page_header[19],
                page_header[20],
                page_header[21],
            ]);

            // Store the first serial number we encounter
            if serial_number.is_none() {
                serial_number = Some(page_serial);
            }

            page_sequence = seq_number;

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

            // Check if this is a Vorbis comment header or FLAC header
            if page_body_size > 0 {
                let page_body = reader.read(page_body_offset, page_body_size as usize)?;

                // Vorbis packets start with packet type (1 byte) + "vorbis" (6 bytes)
                if page_body.len() >= 7 && &page_body[1..7] == b"vorbis" {
                    match page_body[0] {
                        VORBIS_ID_HEADER => {
                            // Parse Vorbis identification header
                            parse_vorbis_id_header(&page_body[7..], &mut metadata)?;
                        }
                        VORBIS_COMMENT_HEADER => {
                            // Parse Vorbis comments
                            parse_vorbis_comments(&page_body[7..], &mut metadata)?;
                            break; // Found comments, we're done
                        }
                        _ => {}
                    }
                }
                // OGG FLAC header: 0x7F "FLAC" version info + STREAMINFO
                else if page_body.len() >= 13
                    && page_body[0] == OGG_FLAC_MARKER
                    && &page_body[1..5] == b"FLAC"
                {
                    parse_ogg_flac_header(&page_body, &mut metadata)?;
                }
                // FLAC metadata block: first byte contains type (bits 0-6) and last-block flag (bit 7)
                // Type 4 = VORBIS_COMMENT, contains vendor string and user comments
                else if page_body.len() >= 4 {
                    let block_type = page_body[0] & 0x7F; // Mask off last-block flag
                    if block_type == FLAC_METADATA_VORBIS_COMMENT {
                        // Block header: 1 byte type + 3 bytes big-endian size
                        let block_size = ((page_body[1] as u32) << 16)
                            | ((page_body[2] as u32) << 8)
                            | (page_body[3] as u32);
                        if page_body.len() >= 4 + block_size as usize {
                            // Parse Vorbis comments from the block data (after 4-byte header)
                            parse_vorbis_comments(
                                &page_body[4..4 + block_size as usize],
                                &mut metadata,
                            )?;
                            break; // Found comments, we're done
                        }
                    }
                }
            }

            // Move to next page
            offset = page_body_offset + page_body_size;
        }

        // Add OGG format-specific tags
        if let Some(sn) = serial_number {
            metadata.insert(
                "OGG:SerialNumber".to_string(),
                TagValue::new_string(format!("{}", sn)),
            );
        }

        metadata.insert(
            "OGG:PageSequence".to_string(),
            TagValue::new_integer(page_sequence as i64),
        );

        // Duration: Would require scanning entire file for proper calculation
        // For now, add a placeholder that can be improved in future versions
        // This would ideally be calculated from granule positions in OGG pages
        // metadata.insert("OGG:Duration".to_string(), TagValue::new_string("".to_string()));

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::OGG)
    }
}

/// Maps Vorbis comment field names to ExifTool-compatible tag names
fn map_vorbis_field_name(field_name: &str) -> String {
    // Normalize field name to uppercase for matching
    let upper = field_name.to_uppercase();

    match upper.as_str() {
        // Core Vorbis tags
        "TITLE" => "Vorbis:Title".to_string(),
        "ARTIST" => "Vorbis:Artist".to_string(),
        "ALBUM" => "Vorbis:Album".to_string(),
        "TRACKNUMBER" => "Vorbis:TrackNumber".to_string(),
        "DATE" => "Vorbis:Date".to_string(),
        "GENRE" => "Vorbis:Genre".to_string(),
        "COMMENT" => "Vorbis:Comment".to_string(),
        "DESCRIPTION" => "Vorbis:Description".to_string(),
        "COPYRIGHT" => "Vorbis:Copyright".to_string(),
        "LICENSE" => "Vorbis:License".to_string(),
        "ORGANIZATION" => "Vorbis:Organization".to_string(),
        "PERFORMER" => "Vorbis:Performer".to_string(),
        "COMPOSER" => "Vorbis:Composer".to_string(),
        "CONDUCTOR" => "Vorbis:Conductor".to_string(),
        "ISRC" => "Vorbis:ISRC".to_string(),
        "LYRICS" => "Vorbis:Lyrics".to_string(),
        "ALBUMARTIST" => "Vorbis:AlbumArtist".to_string(),
        "DISCNUMBER" => "Vorbis:DiscNumber".to_string(),
        "TOTALTRACKS" => "Vorbis:TotalTracks".to_string(),
        "TOTALDISCS" => "Vorbis:TotalDiscs".to_string(),
        "ENCODER" => "Vorbis:Encoder".to_string(),
        "ENCODEDBY" | "ENCODED_BY" => "Vorbis:EncodedBy".to_string(),
        "CONTACT" => "Vorbis:Contact".to_string(),
        "LOCATION" => "Vorbis:Location".to_string(),
        "VERSION" => "Vorbis:Version".to_string(),
        // ReplayGain tags
        "REPLAYGAIN_TRACK_GAIN" => "Vorbis:ReplayGainTrackGain".to_string(),
        "REPLAYGAIN_TRACK_PEAK" => "Vorbis:ReplayGainTrackPeak".to_string(),
        "REPLAYGAIN_ALBUM_GAIN" => "Vorbis:ReplayGainAlbumGain".to_string(),
        "REPLAYGAIN_ALBUM_PEAK" => "Vorbis:ReplayGainAlbumPeak".to_string(),
        // Cover art
        "COVERART" => "Vorbis:CoverArt".to_string(),
        "COVERARTMIME" => "Vorbis:CoverArtMIMEType".to_string(),
        // Unknown - normalize tag name to PascalCase
        // e.g., "MEDIAJUKEBOX:DATE" -> "MediajukeboxDate"
        // e.g., "MEDIAJUKEBOX:TOOL NAME" -> "MediajukeboxToolName"
        _ => {
            // Normalize to PascalCase: split on : and space, capitalize first letter, lowercase rest
            let normalized = field_name
                .split(|c| c == ':' || c == ' ')
                .map(|part| {
                    let mut chars: Vec<char> = part.chars().collect();
                    if !chars.is_empty() {
                        chars[0] = chars[0].to_ascii_uppercase();
                        for c in chars.iter_mut().skip(1) {
                            *c = c.to_ascii_lowercase();
                        }
                    }
                    chars.into_iter().collect::<String>()
                })
                .collect::<String>();
            format!("Vorbis:{}", normalized)
        }
    }
}

/// Parse OGG FLAC header (mapping header + STREAMINFO)
fn parse_ogg_flac_header(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // OGG FLAC mapping header structure:
    // 1 byte: 0x7F marker
    // 4 bytes: "FLAC"
    // 1 byte: major version
    // 1 byte: minor version
    // 2 bytes: number of header packets (big-endian)
    // 4 bytes: "fLaC" native FLAC signature
    // 4 bytes: metadata block header
    // 34 bytes: STREAMINFO data

    if data.len() < 51 {
        return Ok(());
    }

    // Verify "fLaC" signature at offset 9
    if &data[9..13] != b"fLaC" {
        return Ok(());
    }

    // STREAMINFO is at offset 13 (after "fLaC") + 4 (block header) = 17
    // Block header: 1 byte type + 3 bytes size
    let streaminfo_offset = 17;

    if data.len() < streaminfo_offset + 34 {
        return Ok(());
    }

    let streaminfo = &data[streaminfo_offset..];

    // STREAMINFO format (34 bytes):
    // 2 bytes: min block size
    // 2 bytes: max block size
    // 3 bytes: min frame size
    // 3 bytes: max frame size
    // 8 bytes: sample rate (20 bits), channels (3 bits), bits/sample (5 bits), total samples (36 bits)
    // 16 bytes: MD5 signature

    let block_size_min = u16::from_be_bytes([streaminfo[0], streaminfo[1]]);
    let block_size_max = u16::from_be_bytes([streaminfo[2], streaminfo[3]]);
    let frame_size_min =
        ((streaminfo[4] as u32) << 16) | ((streaminfo[5] as u32) << 8) | (streaminfo[6] as u32);
    let frame_size_max =
        ((streaminfo[7] as u32) << 16) | ((streaminfo[8] as u32) << 8) | (streaminfo[9] as u32);

    // Sample rate, channels, bits per sample, total samples packed into bytes 10-17
    let sample_rate = ((streaminfo[10] as u32) << 12)
        | ((streaminfo[11] as u32) << 4)
        | ((streaminfo[12] as u32) >> 4);
    let channels = ((streaminfo[12] >> 1) & 0x07) + 1;
    let bits_per_sample = (((streaminfo[12] & 0x01) << 4) | (streaminfo[13] >> 4)) + 1;
    let total_samples = (((streaminfo[13] as u64) & 0x0F) << 32)
        | ((streaminfo[14] as u64) << 24)
        | ((streaminfo[15] as u64) << 16)
        | ((streaminfo[16] as u64) << 8)
        | (streaminfo[17] as u64);

    // MD5 signature (bytes 18-33)
    let md5_sig = &streaminfo[18..34];
    let md5_str: String = md5_sig.iter().map(|b| format!("{:02x}", b)).collect();

    metadata.insert(
        "FLAC:BlockSizeMin".to_string(),
        TagValue::new_integer(block_size_min as i64),
    );
    metadata.insert(
        "FLAC:BlockSizeMax".to_string(),
        TagValue::new_integer(block_size_max as i64),
    );
    metadata.insert(
        "FLAC:FrameSizeMin".to_string(),
        TagValue::new_integer(frame_size_min as i64),
    );
    metadata.insert(
        "FLAC:FrameSizeMax".to_string(),
        TagValue::new_integer(frame_size_max as i64),
    );
    metadata.insert(
        "FLAC:SampleRate".to_string(),
        TagValue::new_integer(sample_rate as i64),
    );
    metadata.insert(
        "FLAC:Channels".to_string(),
        TagValue::new_integer(channels as i64),
    );
    metadata.insert(
        "FLAC:BitsPerSample".to_string(),
        TagValue::new_integer(bits_per_sample as i64),
    );
    metadata.insert(
        "FLAC:TotalSamples".to_string(),
        TagValue::new_integer(total_samples as i64),
    );
    metadata.insert(
        "FLAC:MD5Signature".to_string(),
        TagValue::new_string(md5_str),
    );

    Ok(())
}

/// Simple base64 decoder for embedded binary data
fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, &'static str> {
    const DECODE_TABLE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i as i8;
            table[(b'a' + i) as usize] = (i + 26) as i8;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            table[(b'0' + i) as usize] = (i + 52) as i8;
            i += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table
    };

    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;

    for &byte in bytes {
        if byte == b'=' {
            break;
        }
        let val = DECODE_TABLE[byte as usize];
        if val < 0 {
            if byte.is_ascii_whitespace() {
                continue;
            }
            return Err("Invalid base64 character");
        }
        buffer = (buffer << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }

    Ok(output)
}

/// Parse Vorbis identification header
fn parse_vorbis_id_header(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // Vorbis identification header (after "vorbis" signature):
    // 4 bytes: vorbis_version (u32 LE, always 0)
    // 1 byte:  audio_channels
    // 4 bytes: audio_sample_rate (u32 LE)
    // 4 bytes: bitrate_maximum (i32 LE)
    // 4 bytes: bitrate_nominal (i32 LE)
    // 4 bytes: bitrate_minimum (i32 LE)
    // Total: 21 bytes minimum

    if data.len() < 21 {
        return Ok(());
    }

    let reader = EndianReader::little_endian(data);

    let vorbis_version = reader.u32_at(0).unwrap_or(0);
    let audio_channels = data[4];
    let sample_rate = reader.u32_at(5).unwrap_or(0);
    let _bitrate_max = reader.i32_at(9).unwrap_or(0);
    let bitrate_nominal = reader.i32_at(13).unwrap_or(0);
    let _bitrate_min = reader.i32_at(17).unwrap_or(0);

    metadata.insert(
        "Vorbis:VorbisVersion".to_string(),
        TagValue::new_integer(vorbis_version as i64),
    );
    metadata.insert(
        "Vorbis:AudioChannels".to_string(),
        TagValue::new_integer(audio_channels as i64),
    );
    metadata.insert(
        "Vorbis:SampleRate".to_string(),
        TagValue::new_integer(sample_rate as i64),
    );
    if bitrate_nominal > 0 {
        // ExifTool formats bitrate as "N kbps"
        let kbps = bitrate_nominal / 1000;
        metadata.insert(
            "Vorbis:NominalBitrate".to_string(),
            TagValue::new_string(format!("{} kbps", kbps)),
        );
    }

    // Add OGG format-specific tags for ExifTool compatibility

    // AudioEncoding: Always "Vorbis" for OGG Vorbis files
    metadata.insert(
        "OGG:AudioEncoding".to_string(),
        TagValue::new_string("Vorbis".to_string()),
    );

    // Channels
    metadata.insert(
        "OGG:Channels".to_string(),
        TagValue::new_integer(audio_channels as i64),
    );

    // SampleRate
    metadata.insert(
        "OGG:SampleRate".to_string(),
        TagValue::new_integer(sample_rate as i64),
    );

    // BitRate (from nominal bitrate)
    if bitrate_nominal > 0 {
        metadata.insert(
            "OGG:BitRate".to_string(),
            TagValue::new_integer((bitrate_nominal / 1000) as i64),
        );
    }

    // CodecName: Always "Vorbis" for OGG Vorbis
    metadata.insert(
        "OGG:CodecName".to_string(),
        TagValue::new_string("Vorbis".to_string()),
    );

    Ok(())
}

/// Parse Vorbis comment data
fn parse_vorbis_comments(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
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
            TagValue::new_string(vendor_str.to_string()),
        );
    }
    offset += vendor_length;

    // User comment list length (4 bytes, little-endian)
    if offset + 4 > data.len() {
        return Err(ExifToolError::parse_error("Missing comment list length"));
    }

    let comment_count = reader.u32_at(offset).unwrap_or(0);
    offset += 4;

    // Parse each comment
    for _ in 0..comment_count {
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
            let tag_name = map_vorbis_field_name(field_name);

            // Handle binary data (like CoverArt which is base64-encoded)
            let upper_field = field_name.to_uppercase();
            if upper_field == "COVERART" || upper_field == "METADATA_BLOCK_PICTURE" {
                // Decode base64 to get actual size
                if let Ok(decoded) = base64_decode(field_value) {
                    let size = decoded.len();
                    metadata.insert(
                        tag_name,
                        TagValue::new_string(format!(
                            "(Binary data {} bytes, use -b option to extract)",
                            size
                        )),
                    );
                } else {
                    // If decoding fails, just report the base64 size
                    metadata.insert(tag_name, TagValue::new_string(field_value.to_string()));
                }
            } else {
                metadata.insert(tag_name, TagValue::new_string(field_value.to_string()));
            }
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
    fn test_ogg_signature_valid() {
        // Minimal OGG page header
        let mut data = vec![0u8; 100];
        data[0..4].copy_from_slice(b"OggS");
        data[5] = 0x00; // header_type (unused but present)
        data[26] = 0; // segment count

        let reader = TestReader::from_slice(&data);
        let parser = OggParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ogg_signature_invalid() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = OggParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_ogg_file_too_small() {
        let data = b"Ogg";
        let reader = TestReader::from_slice(data);
        let parser = OggParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
