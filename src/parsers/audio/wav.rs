//! WAV audio format parser
//!
//! Implements metadata extraction from WAV (RIFF WAVE) audio files,
//! parsing INFO chunks and other metadata containers.
//!
//! # Supported Metadata
//!
//! - **INFO Chunk:** INAM (Name), IART (Artist), ICRD (Creation Date), IGNR (Genre)
//! - **Format Info:** Sample rate, bits per sample, channels
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `RIFF.pm` module:
//! - `RIFF:Title` → INAM from INFO chunk
//! - `RIFF:Artist` → IART from INFO chunk
//! - `RIFF:DateCreated` → ICRD from INFO chunk
//!
//! # File Structure
//!
//! ```text
//! [RIFF header - "RIFF" + size + "WAVE"]
//! [fmt  chunk - format information]
//! [data chunk - audio samples]
//! [INFO chunk - metadata (optional)]
//! [other chunks...]
//! ```
//!
//! # References
//!
//! - RIFF Spec: <https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/WAVE/WAVE.html>
//! - ExifTool Source: `lib/Image/ExifTool/RIFF.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use encoding_rs::WINDOWS_1252;

/// RIFF signature
const RIFF_SIGNATURE: &[u8] = b"RIFF";

/// WAVE format identifier
const WAVE_FORMAT: &[u8] = b"WAVE";

/// WAV parser
pub struct WavParser;

/// Parses metadata from a WAV file.
///
/// This is a convenience wrapper that creates a WavParser instance and calls parse().
///
/// # Arguments
///
/// * `reader` - File reader providing access to the WAV file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_wav_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = WavParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

impl FormatParser for WavParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify RIFF/WAVE signature
        if reader.size() < 12 {
            return Err(ExifToolError::parse_error("File too small to be WAV"));
        }

        let header = reader.read(0, 12)?;
        if &header[0..4] != RIFF_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid RIFF signature: expected {:?}, found {:?}",
                RIFF_SIGNATURE,
                &header[0..4]
            )));
        }

        if &header[8..12] != WAVE_FORMAT {
            return Err(ExifToolError::parse_error(format!(
                "Invalid WAVE format: expected {:?}, found {:?}",
                WAVE_FORMAT,
                &header[8..12]
            )));
        }

        let mut metadata = MetadataMap::with_capacity(16);
        let file_size = reader.size();

        // Parse RIFF chunks
        parse_riff_chunks(reader, 12, file_size, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::WAV)
    }
}

/// Parse RIFF chunks starting from offset
pub(crate) fn parse_riff_chunks(
    reader: &dyn FileReader,
    start_offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut offset = start_offset;

    while offset + 8 < end_offset {
        // Read chunk header (4 byte ID + 4 byte size)
        let chunk_header = reader.read(offset, 8)?;
        let header_reader = EndianReader::little_endian(chunk_header);

        let chunk_id = &chunk_header[0..4];
        let chunk_size = header_reader.u32_at(4).unwrap_or(0) as u64;

        offset += 8;

        // Ensure chunk doesn't extend beyond file
        if offset + chunk_size > end_offset {
            break;
        }

        // Process specific chunks
        match chunk_id {
            b"fmt " => {
                // Parse format chunk
                if chunk_size >= 16 {
                    parse_fmt_chunk(reader, offset, metadata)?;
                }
            }
            b"LIST" => {
                // Parse LIST chunk (may contain INFO or exif)
                if chunk_size >= 4 {
                    let list_type = reader.read(offset, 4)?;
                    if &list_type == b"INFO" {
                        parse_info_chunk(reader, offset + 4, offset + chunk_size, metadata)?;
                    } else if &list_type == b"exif" {
                        parse_exif_chunk(reader, offset + 4, offset + chunk_size, metadata)?;
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

/// Decode audio format code to human-readable encoding name
fn decode_audio_format(format: u16) -> &'static str {
    match format {
        0x0001 => "Microsoft PCM",
        0x0002 => "Microsoft ADPCM",
        0x0003 => "IEEE Float",
        0x0006 => "ITU G.711 a-law",
        0x0007 => "ITU G.711 mu-law",
        0x0011 => "Intel DVI/IMA ADPCM",
        0x0016 => "ITU G.723 ADPCM (Yamaha)",
        0x0031 => "GSM 6.10",
        0x0040 => "ITU G.721 ADPCM",
        0x0055 => "MPEG",
        0x0069 => "MPEG Layer 3",
        0xFFFE => "Extensible",
        _ => "Unknown",
    }
}

/// Parse fmt chunk (format information)
fn parse_fmt_chunk(reader: &dyn FileReader, offset: u64, metadata: &mut MetadataMap) -> Result<()> {
    let fmt_data = reader.read(offset, 16)?;
    let fmt_reader = EndianReader::little_endian(fmt_data);

    let audio_format = fmt_reader.u16_at(0).unwrap_or(0);
    let num_channels = fmt_reader.u16_at(2).unwrap_or(0);
    let sample_rate = fmt_reader.u32_at(4).unwrap_or(0);
    let avg_bytes_per_sec = fmt_reader.u32_at(8).unwrap_or(0);
    let bits_per_sample = fmt_reader.u16_at(14).unwrap_or(0);

    // Encoding - human-readable format name
    metadata.insert(
        "RIFF:Encoding".to_string(),
        TagValue::new_string(decode_audio_format(audio_format).to_string()),
    );
    metadata.insert(
        "RIFF:NumChannels".to_string(),
        TagValue::new_integer(num_channels as i64),
    );
    metadata.insert(
        "RIFF:SampleRate".to_string(),
        TagValue::new_integer(sample_rate as i64),
    );
    metadata.insert(
        "RIFF:AvgBytesPerSec".to_string(),
        TagValue::new_integer(avg_bytes_per_sec as i64),
    );
    metadata.insert(
        "RIFF:BitsPerSample".to_string(),
        TagValue::new_integer(bits_per_sample as i64),
    );

    Ok(())
}

/// Parse INFO chunk (metadata tags)
pub(crate) fn parse_info_chunk(
    reader: &dyn FileReader,
    start_offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut offset = start_offset;

    while offset + 8 < end_offset {
        // Read tag header (4 byte ID + 4 byte size)
        let tag_header = reader.read(offset, 8)?;
        let header_reader = EndianReader::little_endian(tag_header);

        let tag_id = &tag_header[0..4];
        let tag_size = header_reader.u32_at(4).unwrap_or(0) as usize;

        offset += 8;

        if offset + tag_size as u64 > end_offset {
            break;
        }

        // Read tag value
        let tag_value_bytes = reader.read(offset, tag_size)?;

        // Decode as Windows-1252 (null-terminated)
        let (tag_value, _, _) = WINDOWS_1252.decode(tag_value_bytes);
        let tag_value = tag_value.trim_end_matches('\0').trim();

        if !tag_value.is_empty() {
            // Map INFO tag IDs to readable names (comprehensive RIFF INFO tags)
            let tag_name = match tag_id {
                // Core metadata
                b"INAM" => "RIFF:Title",
                b"IART" => "RIFF:Artist",
                b"IPRD" => "RIFF:Product",
                b"ISBJ" => "RIFF:Subject",
                b"ICMT" => "RIFF:Comment",
                b"ICOP" => "RIFF:Copyright",
                b"ICRD" => "RIFF:DateCreated",
                b"IGNR" => "RIFF:Genre",
                b"IKEY" => "RIFF:Keywords",
                b"IMED" => "RIFF:Medium",

                // Software and technical
                b"ISFT" => "RIFF:Software",
                b"ISRF" => "RIFF:Source",
                b"ITCH" => "RIFF:Technician",
                b"ISRC" => "RIFF:SourceSupplier",
                b"ISMP" => "RIFF:TimeCode",

                // Album and track info
                b"IPRT" => "RIFF:TrackNumber",
                b"IFRM" => "RIFF:FrameCount",
                b"ILEN" => "RIFF:Length",

                // People
                b"IENG" => "RIFF:Engineer",
                b"IMUS" => "RIFF:Musician",
                b"IPRO" => "RIFF:Producer",
                b"ICMS" => "RIFF:Commissioned",
                b"IDIT" => "RIFF:DateTimeOriginal",

                // Additional metadata
                b"ICRP" => "RIFF:Cropped",
                b"IDIM" => "RIFF:Dimensions",
                b"IDPI" => "RIFF:DotsPerInch",
                b"IPLT" => "RIFF:Palette",
                b"ISHP" => "RIFF:Sharpness",
                b"ILGT" => "RIFF:Lightness",
                b"ICLR" => "RIFF:ColorSpace",
                b"ARCH" => "RIFF:Archival",
                b"RATE" => "RIFF:Rate",
                b"STAR" => "RIFF:Starring",
                b"CMNT" => "RIFF:Comment2",
                b"DIRC" => "RIFF:Director",
                b"PROD" => "RIFF:Producer2",
                b"STUDIO" => "RIFF:Studio",
                b"EDIT" => "RIFF:EditedBy",
                b"ALBUM" => "RIFF:Album",
                b"LABEL" => "RIFF:Label",
                b"TRCK" => "RIFF:Track",
                b"TITL" => "RIFF:Title2",

                _ => {
                    // For unknown tags, create a generic tag name
                    let id_str = String::from_utf8_lossy(tag_id);
                    let generic_name = format!("RIFF:{}", id_str.trim());

                    // Only store if it's printable ASCII-ish
                    if tag_id.iter().all(|&b| (0x20..0x7F).contains(&b)) {
                        metadata.insert(generic_name, TagValue::new_string(tag_value.to_string()));
                    }

                    offset += tag_size as u64;
                    if tag_size % 2 == 1 {
                        offset += 1;
                    }
                    continue;
                }
            };

            metadata.insert(
                tag_name.to_string(),
                TagValue::new_string(tag_value.to_string()),
            );
        }

        // Move to next tag (align to even byte boundary)
        offset += tag_size as u64;
        if tag_size % 2 == 1 {
            offset += 1;
        }
    }

    Ok(())
}

/// Parse EXIF chunk (EXIF 2.3 metadata for WAV audio files)
///
/// The LIST exif chunk contains EXIF tags with 4-byte IDs:
/// - ever: ExifVersion
/// - ecor: Make
/// - emdl: Model
/// - emnt: MakerNotes
/// - erel: RelatedImageFile
/// - etim: TimeCreated
fn parse_exif_chunk(
    reader: &dyn FileReader,
    start_offset: u64,
    end_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let mut offset = start_offset;

    while offset + 8 < end_offset {
        // Read tag header (4 byte ID + 4 byte size)
        let tag_header = reader.read(offset, 8)?;
        let header_reader = EndianReader::little_endian(tag_header);

        let tag_id = &tag_header[0..4];
        let tag_size = header_reader.u32_at(4).unwrap_or(0) as usize;

        offset += 8;

        if offset + tag_size as u64 > end_offset {
            break;
        }

        // Read tag value
        let tag_value_bytes = reader.read(offset, tag_size)?;

        // Map EXIF tag IDs to readable names (EXIF 2.3 for WAV)
        let tag_name = match tag_id {
            b"ever" => "RIFF:ExifVersion",
            b"ecor" => "RIFF:Make",
            b"emdl" => "RIFF:Model",
            b"emnt" => "RIFF:MakerNotes",
            b"erel" => "RIFF:RelatedImageFile",
            b"etim" => "RIFF:TimeCreated",
            _ => {
                // Unknown EXIF tag - skip
                offset += tag_size as u64;
                if tag_size % 2 == 1 {
                    offset += 1;
                }
                continue;
            }
        };

        // Handle different tag types
        if tag_id == b"emnt" {
            // MakerNotes is binary data - store as binary indicator (match ExifTool format)
            let binary_msg = format!("(Binary data {} bytes, use -b option to extract)", tag_size);
            metadata.insert(tag_name.to_string(), TagValue::new_string(binary_msg));
        } else {
            // Other tags are ASCII strings (null-terminated)
            let (tag_value, _, _) = WINDOWS_1252.decode(tag_value_bytes);
            let tag_value = tag_value.trim_end_matches('\0').trim();

            if !tag_value.is_empty() {
                metadata.insert(
                    tag_name.to_string(),
                    TagValue::new_string(tag_value.to_string()),
                );
            }
        }

        // Move to next tag (align to even byte boundary)
        offset += tag_size as u64;
        if tag_size % 2 == 1 {
            offset += 1;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_wav_signature_valid() {
        // Minimal WAV file structure
        let mut data = vec![0u8; 100];
        data[0..4].copy_from_slice(b"RIFF");
        data[4..8].copy_from_slice(&44u32.to_le_bytes()); // file size - 8
        data[8..12].copy_from_slice(b"WAVE");

        let reader = TestReader::from_slice(&data);
        let parser = WavParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());
    }

    #[test]
    fn test_wav_signature_invalid_riff() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = WavParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_wav_signature_invalid_wave() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(b"RIFF");
        data[4..8].copy_from_slice(&44u32.to_le_bytes());
        data[8..12].copy_from_slice(b"XXXX"); // Invalid WAVE signature

        let reader = TestReader::from_slice(&data);
        let parser = WavParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_wav_file_too_small() {
        let data = b"RIFF";
        let reader = TestReader::from_slice(data);
        let parser = WavParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
