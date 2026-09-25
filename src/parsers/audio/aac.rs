//! AAC (Advanced Audio Codec) format parser
//!
//! Implements metadata extraction from AAC audio files with ADTS (Audio Data
//! Transport Stream) headers and M4A (MPEG-4 Audio) files with iTunes metadata.
//!
//! # Supported Metadata
//!
//! - **ADTS Header:** Profile, sample rate, channel configuration, frame length
//! - **Frame Info:** Count of the ADTS frames actually present
//! - **Filler Payload:** Encoder name from the first frame
//! - **iTunes Atoms (M4A):** Title, Artist, Album, and 35+ other metadata tags
//!
//! Nothing here is extrapolated: no average bitrate and no duration, because
//! both would have to be projected from a single frame and `AAC.pm` says
//! outright that every frame must be scanned first.
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags (`AAC.pm`):
//! - `AAC:ProfileType` → bits 016-017 of the ADTS header
//! - `AAC:SampleRate` → bits 018-021
//! - `AAC:Channels` → bits 023-025
//! - `AAC:Encoder` → filler payload of the first frame
//! - iTunes atoms → shared QuickTime reader; source-derived ItemList groups
//!
//! # File Structure
//!
//! ```text
//! [ADTS Frame 0]
//!   ├─ Sync Word: 0xFFF (12 bits)
//!   ├─ Header: Profile, sample rate, channels
//!   └─ AAC Data (variable)
//! [ADTS Frame 1]
//! [ADTS Frame 2]
//! ...
//!
//! [M4A/MP4 Structure]
//! ftyp (File Type Box)
//! moov (Movie Box)
//!   └─ udta (User Data Atom)
//!       └─ meta (Metadata Container)
//!           └─ ilst (Item List Atom)
//!               ├─ ©nam (Title)
//!               ├─ ©ART (Artist)
//!               ├─ aART (Album Artist)
//!               ├─ ©alb (Album)
//!               ├─ ©day (Date)
//!               ├─ trkn (Track Number)
//!               ├─ disk (Disk Number)
//!               └─ ... (35+ additional tags)
//! mdat (Media Data Box)
//! ```
//!
//! # References
//!
//! - ISO 13818-7: MPEG-2 Advanced Audio Coding (AAC)
//! - ISO 14496-12: MPEG-4 File Format (MP4/M4A)
//! - ExifTool Source: `lib/Image/ExifTool/AAC.pm`, `lib/Image/ExifTool/QuickTime.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// ADTS sync word (12 bits: 0xFFF)
const ADTS_SYNC_WORD: u16 = 0xFFF;

/// AAC sample rate table (indexed by sample rate index)
const SAMPLE_RATES: [u32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0,
    0,
];

/// AAC profile names
const PROFILES: [&str; 4] = [
    "Main",
    "LC (Low Complexity)",
    "SSR (Scalable Sampling Rate)",
    "LTP (Long Term Prediction)",
];

/// AAC parser
pub struct AacParser;

/// Parses metadata from an AAC file.
///
/// This is a convenience wrapper that creates an AacParser instance and calls parse().
///
/// # Arguments
///
/// * `reader` - File reader providing access to the AAC file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_aac_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = AacParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

impl FormatParser for AacParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Every row here is read from the file (`metadata_map::file_rows`):
        // a caller's later `insert`/`get_mut` is what counts as assigned.
        crate::core::metadata_map::file_rows(|| -> Result<MetadataMap> {
            let file_size = reader.size();

            // Verify file is large enough
            if file_size < 7 {
                return Err(ExifToolError::parse_error("File too small to be AAC"));
            }

            // ISO Base Media containers share the QuickTime atom walker and its
            // generated ItemList decoder. The FourCC follows the four-byte size.
            if file_size >= 8 && reader.read(4, 4)? == b"ftyp" {
                return crate::parsers::quicktime::parse_quicktime_metadata(reader)
                    .map_err(ExifToolError::parse_error);
            }

            // Otherwise, try to parse as pure ADTS AAC. The ADTS header is 7
            // bytes: handing `parse_adts_header` the 4-byte magic made it fail
            // its own length check on every real AAC file, so the format
            // silently produced no tags at all.
            let header = reader.read(0, 7)?;
            let header_reader = EndianReader::big_endian(header);

            // Verify ADTS sync word (0xFFF in first 12 bits)
            let sync = header_reader.u16_at(0).unwrap_or(0);
            if (sync >> 4) != ADTS_SYNC_WORD {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid AAC file: not MP4 format and not valid ADTS (sync word 0x{:03X})",
                    sync >> 4
                )));
            }

            let mut metadata = MetadataMap::with_capacity(16);

            // Parse ADTS header
            let adts_info = parse_adts_header(header)?;

            // ExifTool's AAC table names these ProfileType / SampleRate /
            // Channels (AAC.pm bits 016-017, 018-021 and 023-025).
            metadata.insert(
                "AAC:ProfileType".to_string(),
                TagValue::new_string(adts_info.profile_type.to_string()),
            );
            metadata.insert(
                "AAC:AudioObjectType".to_string(),
                TagValue::new_string(adts_info.profile.to_string()),
            );
            metadata.insert(
                "AAC:SampleRate".to_string(),
                TagValue::new_integer(adts_info.sample_rate as i64),
            );
            metadata.insert(
                "AAC:ChannelConfiguration".to_string(),
                TagValue::new_integer(adts_info.channel_config as i64),
            );

            // The encoder name lives in the first frame's filler payload.
            if let Some(encoder) =
                read_encoder_from_first_frame(reader, header, adts_info.frame_length)
            {
                metadata.insert("AAC:Encoder".to_string(), TagValue::new_string(encoder));
            }
            metadata.insert(
                "AAC:FrameLength".to_string(),
                TagValue::new_integer(adts_info.frame_length as i64),
            );

            // Channels uses ExifTool's PrintConv for the channel configuration
            // code: 6 and 7 print as "5+1" and "7+1", and an unset code prints
            // "?" rather than being silently rounded to stereo.
            metadata.insert(
                "AAC:Channels".to_string(),
                TagValue::new_string(
                    match adts_info.channel_config {
                        0 => "?",
                        1 => "1",
                        2 => "2",
                        3 => "3",
                        4 => "4",
                        5 => "5",
                        6 => "5+1",
                        7 => "7+1",
                        _ => "?",
                    }
                    .to_string(),
                ),
            );

            // AAC:BitRate, AAC:ObjectType and AAC:ProfileLevel used to be
            // emitted here. ObjectType and ProfileLevel were re-spellings of
            // AudioObjectType with no ExifTool counterpart, and BitRate
            // extrapolated the whole file's rate from the first frame alone --
            // ExifTool's own AAC.pm notes that "all frames must be scanned to
            // calculate average bitrate", so that number was a guess wearing a
            // measurement's name.

            // Count the ADTS frames that are actually present (scan up to 1MB).
            let scan_size = 1_000_000u64.min(file_size);
            let mut frame_count = 0u64;
            let mut offset = 0u64;

            while offset + 7 < scan_size {
                // Verify sync word
                let sync_bytes = reader.read(offset, 2)?;
                let sync_reader = EndianReader::big_endian(sync_bytes);
                let sync = sync_reader.u16_at(0).unwrap_or(0);

                if (sync >> 4) != ADTS_SYNC_WORD {
                    break;
                }

                // Read frame length from header
                let frame_header = reader.read(offset, 7)?;
                if let Ok(frame_info) = parse_adts_header(frame_header) {
                    frame_count += 1;
                    offset += frame_info.frame_length as u64;
                } else {
                    break;
                }
            }

            if frame_count > 0 {
                metadata.insert(
                    "AAC:FrameCount".to_string(),
                    TagValue::new_integer(frame_count as i64),
                );
            }
            // AAC:Duration used to be derived here by extrapolating an average
            // frame size over the file length. That is a projection, not a
            // measurement, and ExifTool publishes no Duration for AAC at all.

            Ok(metadata)
        })
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::AAC)
    }
}

/// ADTS header information
struct AdtsInfo {
    profile: &'static str,
    /// ExifTool's `ProfileType` PrintConv for the same 2-bit code.
    profile_type: &'static str,
    sample_rate: u32,
    channel_config: u8,
    frame_length: u16,
}

/// Reads the encoder string from the first ADTS frame's filler element.
///
/// ExifTool takes `Encoder` from the filler payload (element id 6) of the
/// first frame, skipping the CRC when one is present, and accepts the value
/// only when the trimmed bytes are entirely printable ASCII.
fn read_encoder_from_first_frame(
    reader: &dyn FileReader,
    header: &[u8],
    frame_length: u16,
) -> Option<String> {
    if frame_length <= 8 {
        return None;
    }
    let payload_len = usize::from(frame_length) - 7;
    if 7 + payload_len as u64 > reader.size() {
        return None;
    }
    let payload = reader.read(7, payload_len).ok()?;

    // Bit 15 of the header ("CRC absent") is set when there is no CRC.
    let no_crc = header[1] & 0x01 != 0;
    let blocks = usize::from(header[6] & 0x03);
    let mut pos = if no_crc { 0 } else { 2 + 2 * blocks };

    if pos + 2 > payload.len() {
        return None;
    }
    let word = u16::from_be_bytes([payload[pos], payload[pos + 1]]);
    // Syntactic element id 6 is FIL (filler), which carries the encoder.
    if word >> 13 != 6 {
        return None;
    }

    let mut count = usize::from((word >> 9) & 0x0f);
    pos += 1;
    if count == 15 {
        // An escape count of 15 means the real length follows.
        count += usize::from((word >> 1) & 0xff);
        count = count.checked_sub(1)?;
        pos += 1;
    }
    let end = pos.checked_add(count).filter(|e| *e <= payload.len())?;

    let raw = &payload[pos..end];
    let trimmed = raw
        .iter()
        .position(|&b| b != 0)
        .map(|start| &raw[start..])
        .unwrap_or(&[]);
    let trimmed = match trimmed.iter().rposition(|&b| b != 0) {
        Some(last) => &trimmed[..=last],
        None => return None,
    };
    if trimmed.is_empty() || !trimmed.iter().all(|&b| (0x20..=0x7e).contains(&b)) {
        return None;
    }
    Some(String::from_utf8_lossy(trimmed).into_owned())
}

/// Parse ADTS header (7 bytes)
fn parse_adts_header(header: &[u8]) -> Result<AdtsInfo> {
    if header.len() < 7 {
        return Err(ExifToolError::parse_error("ADTS header too small"));
    }

    // Parse ADTS fields:
    // Byte 0-1: Sync word (12 bits) + MPEG version (1 bit) + Layer (2 bits) + Protection (1 bit)
    // Byte 2: Profile (2 bits) + Sample rate index (4 bits) + Private (1 bit) + Channel start (1 bit)
    // Byte 3: Channel (2 bits) + Originality (1 bit) + Home (1 bit) + Copyright (2 bits) + Frame length start (2 bits)
    // Byte 4-5: Frame length (11 bits continue) + Buffer fullness start (5 bits)
    // Byte 6: Buffer fullness (6 bits) + Frame count (2 bits)

    let profile_idx = ((header[2] >> 6) & 0x03) as usize;
    let sample_rate_idx = ((header[2] >> 2) & 0x0F) as usize;
    let channel_config = ((header[2] & 0x01) << 2) | ((header[3] >> 6) & 0x03);

    // Frame length (13 bits): bits from byte 3-5
    let frame_length =
        (((header[3] & 0x03) as u16) << 11) | ((header[4] as u16) << 3) | ((header[5] >> 5) as u16);

    // Validate frame length (must be at least 7 bytes for ADTS header)
    if frame_length < 7 {
        return Err(ExifToolError::parse_error(format!(
            "Invalid frame length: {} (must be at least 7)",
            frame_length
        )));
    }

    // Validate indices
    if sample_rate_idx >= SAMPLE_RATES.len() {
        return Err(ExifToolError::parse_error("Invalid sample rate index"));
    }

    let sample_rate = SAMPLE_RATES[sample_rate_idx];
    if sample_rate == 0 {
        return Err(ExifToolError::parse_error("Invalid sample rate"));
    }

    let profile = if profile_idx < PROFILES.len() {
        PROFILES[profile_idx]
    } else {
        "Unknown"
    };

    // ExifTool treats code 3 as reserved and rejects the file rather than
    // labelling it, so an out-of-table code must report itself.
    let profile_type = match profile_idx {
        0 => "Main",
        1 => "Low Complexity",
        2 => "Scalable Sampling Rate",
        _ => "Reserved",
    };

    Ok(AdtsInfo {
        profile,
        profile_type,
        sample_rate,
        channel_config,
        frame_length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    #[ignore] // ADTS parsing test - needs proper frame validation
    fn test_aac_adts_signature_valid() {
        // Create minimal AAC ADTS header
        // 0xFFF1 = sync word (0xFFF) + MPEG-4 (1) + Layer (00) + no CRC (1)
        let mut data = vec![0u8; 1000];
        data[0] = 0xFF; // Sync word high byte
        data[1] = 0xF1; // Sync word low nibble + flags
        data[2] = 0x50; // Profile=1 (LC), Sample rate=4 (44100), private=0, channel start=0
        data[3] = 0x80; // Channel=2 (stereo), other flags
        // Frame length = 100 bytes (0x64 = 0b0000001100100)
        // Bits 11-12 in byte 3 (lower 2 bits): 0b00
        // Bits 3-10 in byte 4: 0b00001100 = 0x0C
        // Bits 0-2 in byte 5 (upper 3 bits): 0b100 = 0x80
        data[4] = 0x0C; // Frame length middle byte
        data[5] = 0x80; // Frame length low bits + buffer fullness start
        data[6] = 0xFC; // Buffer fullness + frame count

        let reader = TestReader::new(data);
        let parser = AacParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get("AAC:SampleRate").unwrap().as_integer(),
            Some(44100)
        );
    }

    #[test]
    fn test_aac_signature_invalid() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = AacParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_aac_file_too_small() {
        let data = b"\xFF\xF1";
        let reader = TestReader::from_slice(data);
        let parser = AacParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    fn atom(key: &[u8; 4], data: &[u8]) -> Vec<u8> {
        [(data.len() as u32 + 8).to_be_bytes().as_slice(), key, data].concat()
    }

    fn m4a_item(key: &[u8; 4], payloads: &[Vec<u8>]) -> Vec<u8> {
        let children: Vec<u8> = payloads
            .iter()
            .flat_map(|data| atom(b"data", data))
            .collect();
        let ilst = atom(b"ilst", &atom(key, &children));
        let meta = atom(b"meta", &[&[0; 4][..], &ilst].concat());
        let moov = atom(b"moov", &atom(b"udta", &meta));
        [atom(b"ftyp", b"M4A \0\0\0\0M4A isom"), moov].concat()
    }

    fn payload(flags: u32, data: &[u8]) -> Vec<u8> {
        [flags.to_be_bytes().as_slice(), &[0; 4], data].concat()
    }

    #[test]
    fn m4a_uses_generated_reader_with_u64_and_retains_duplicates() {
        let data = m4a_item(
            b"plID",
            &[
                payload(0, &4294967297u64.to_be_bytes()),
                payload(0, &u64::MAX.to_be_bytes()),
            ],
        );
        let reader = TestReader::new(data);
        let metadata = AacParser.parse(&reader).unwrap();
        let rows = metadata.occurrences_for("QuickTime:AlbumID");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].raw.as_integer(), Some(4294967297));
        assert_eq!(rows[0].value_conv().as_integer(), Some(4294967297));
        assert_eq!(rows[1].raw.as_string(), Some("18446744073709551615"));
        assert_eq!(
            rows[1].value_conv().as_string(),
            Some("18446744073709551615")
        );
        assert!(rows.iter().all(|row| row.group1.as_ref() == "ItemList"));
        assert_eq!(
            metadata,
            crate::parsers::quicktime::parse_quicktime_metadata(&reader).unwrap()
        );
    }

    #[test]
    fn m4a_generated_text_and_enum_use_source_groups() {
        for (key, flags, value, name, expected) in [
            (b"\xa9nam", 2, &b"\0H\0i"[..], "QuickTime:Title", "Hi"),
            (
                b"stik",
                21,
                &b"\x02"[..],
                "QuickTime:MediaType",
                "Audiobook",
            ),
        ] {
            let reader = TestReader::new(m4a_item(key, &[payload(flags, value)]));
            let metadata = AacParser.parse(&reader).unwrap();
            assert_eq!(metadata.get_string(name), Some(expected));
            assert_eq!(
                metadata.occurrences_for(name)[0].group1.as_ref(),
                "ItemList"
            );
        }
    }

    #[test]
    fn truncated_m4a_data_cannot_escape_item_bounds() {
        let mut data = payload(0, &4294967297u64.to_be_bytes());
        data.truncate(6);
        let reader = TestReader::new(m4a_item(b"plID", &[data]));
        let metadata = AacParser.parse(&reader).unwrap();
        assert!(metadata.get("QuickTime:AlbumID").is_none());
    }
}
