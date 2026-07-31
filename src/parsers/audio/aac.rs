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
//! - iTunes atoms → `ItemList:*` and `QuickTime:*` tags
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

/// Media type values for iTunes stik atom
const MEDIA_TYPES: &[(u8, &str)] = &[
    (0, "Music"),
    (1, "Video"),
    (2, "Audiobook"),
    (6, "Music Video"),
    (9, "Short Film"),
    (10, "TV Show"),
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
        let file_size = reader.size();

        // Verify file is large enough
        if file_size < 7 {
            return Err(ExifToolError::parse_error("File too small to be AAC"));
        }

        // Read first 4 bytes to detect file type
        let magic = reader.read(0, 4)?;

        // Check if this is an M4A file (MP4 container) by looking for 'ftyp' box
        // M4A files start with a size (4 bytes) followed by 'ftyp' (4 bytes)
        if magic.len() >= 4 && &magic[1..4] == b"ftyp" {
            // This is an M4A/MP4 file - extract iTunes metadata
            let mut metadata = MetadataMap::with_capacity(32);
            if let Err(e) = extract_itunes_metadata(reader, &mut metadata) {
                // If iTunes extraction fails, log error but don't fail completely
                // (there may be other metadata to extract)
                eprintln!("Warning: Failed to extract iTunes metadata: {}", e);
            }

            // No synthetic AAC:AudioEncoding / AAC:iTunesVersion here: both
            // were fixed strings that restated the container rather than
            // anything read out of it, and ExifTool reports neither tag.
            return Ok(metadata);
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
        if let Some(encoder) = read_encoder_from_first_frame(reader, header, adts_info.frame_length)
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

/// Extract iTunes metadata from M4A (MP4 container with AAC audio)
///
/// M4A files store iTunes metadata in the ilst (item list) atom within the moov atom.
/// This function locates the ilst atom and extracts all iTunes tags.
fn extract_itunes_metadata(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    let file_size = reader.size();

    // Search for 'moov' atom (typically within first 2MB of file)
    let search_limit = (2_000_000u64).min(file_size);
    let moov_offset = find_atom_offset(reader, "moov", search_limit)?;
    let moov_offset = moov_offset
        .ok_or_else(|| ExifToolError::parse_error("M4A file missing required moov atom"))?;

    // Read moov atom header to get size
    let moov_header = reader.read(moov_offset, 8)?;
    let moov_size = u32::from_be_bytes([
        moov_header[0],
        moov_header[1],
        moov_header[2],
        moov_header[3],
    ]) as u64;

    if moov_size < 8 {
        return Err(ExifToolError::parse_error("Invalid moov atom size"));
    }

    // Search for 'udta' atom within moov
    let udta_offset =
        find_atom_offset_in_range(reader, "udta", moov_offset + 8, moov_offset + moov_size)?;

    if let Some(udta_offset) = udta_offset {
        // Read udta atom header
        let udta_header = reader.read(udta_offset, 8)?;
        let udta_size = u32::from_be_bytes([
            udta_header[0],
            udta_header[1],
            udta_header[2],
            udta_header[3],
        ]) as u64;

        if udta_size >= 8 {
            // Search for 'meta' atom within udta
            let meta_offset = find_atom_offset_in_range(
                reader,
                "meta",
                udta_offset + 8,
                udta_offset + udta_size,
            )?;

            if let Some(meta_offset) = meta_offset {
                // Read meta atom header
                let meta_header = reader.read(meta_offset, 8)?;
                let meta_size = u32::from_be_bytes([
                    meta_header[0],
                    meta_header[1],
                    meta_header[2],
                    meta_header[3],
                ]) as u64;

                if meta_size >= 8 {
                    // Skip version/flags (4 bytes) to get to children
                    let ilst_offset = find_atom_offset_in_range(
                        reader,
                        "ilst",
                        meta_offset + 12, // Skip size (4) + type (4) + version/flags (4)
                        meta_offset + meta_size,
                    )?;

                    if let Some(ilst_offset) = ilst_offset {
                        extract_ilst_items(reader, ilst_offset, metadata)?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Find an atom by type within a specified range in the file
fn find_atom_offset_in_range(
    reader: &dyn FileReader,
    atom_type: &str,
    start: u64,
    end: u64,
) -> Result<Option<u64>> {
    let atom_bytes = atom_type.as_bytes();
    if atom_bytes.len() != 4 {
        return Ok(None);
    }

    let mut offset = start;
    while offset + 8 <= end {
        let header = reader.read(offset, 8)?;
        let atom_size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;

        if atom_size < 8 {
            break;
        }

        if &header[4..8] == atom_bytes {
            return Ok(Some(offset));
        }

        offset += atom_size;

        // Ensure we don't exceed file bounds or search limit
        if offset > end {
            break;
        }
    }

    Ok(None)
}

/// Find an atom by type in the file (searches up to specified limit)
fn find_atom_offset(reader: &dyn FileReader, atom_type: &str, limit: u64) -> Result<Option<u64>> {
    find_atom_offset_in_range(reader, atom_type, 0, limit)
}

/// Extract iTunes metadata items from the ilst atom
///
/// Each child of ilst is an iTunes metadata item with the following structure:
/// - 4 bytes: size
/// - 4 bytes: atom type (e.g., ©nam, aART, trkn)
/// - Child atom 'data' containing the actual metadata value
fn extract_ilst_items(
    reader: &dyn FileReader,
    ilst_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    // Read ilst atom header to get size
    let ilst_header = reader.read(ilst_offset, 8)?;
    let ilst_size = u32::from_be_bytes([
        ilst_header[0],
        ilst_header[1],
        ilst_header[2],
        ilst_header[3],
    ]) as u64;

    if ilst_size < 8 {
        return Ok(());
    }

    // Iterate through child atoms in ilst
    let mut offset = ilst_offset + 8;
    let end = ilst_offset + ilst_size;

    while offset + 8 <= end {
        let item_header = reader.read(offset, 8)?;
        let item_size = u32::from_be_bytes([
            item_header[0],
            item_header[1],
            item_header[2],
            item_header[3],
        ]) as u64;

        if item_size < 8 {
            break;
        }

        let atom_type = &item_header[4..8];

        // Look for 'data' atom within this item
        let data_offset =
            find_atom_offset_in_range(reader, "data", offset + 8, offset + item_size)?;

        if let Some(data_offset) = data_offset {
            // Read data atom header
            let data_header = reader.read(data_offset, 8)?;
            let data_size = u32::from_be_bytes([
                data_header[0],
                data_header[1],
                data_header[2],
                data_header[3],
            ]) as u64;

            if data_size >= 16 {
                // data atom structure: size (4) + type (4) + flags+type (4) + reserved (4) + value
                let value_offset = data_offset + 16;
                let value_size = data_size - 16;

                let value_data = reader.read(value_offset, value_size as usize)?;
                let data_type = u32::from_be_bytes([
                    data_header[8],
                    data_header[9],
                    data_header[10],
                    data_header[11],
                ]);

                // Extract and map the iTunes tag
                extract_itunes_item(atom_type, data_type, &value_data, metadata);
            }
        }

        offset += item_size;
    }

    Ok(())
}

/// Extract a single iTunes metadata item and add it to the metadata map
///
/// Maps iTunes atom types (4-byte codes) to ExifTool tag names and formats the values
/// appropriately based on the data type indicator.
fn extract_itunes_item(
    atom_type: &[u8],
    data_type: u32,
    value_data: &[u8],
    metadata: &mut MetadataMap,
) {
    // Determine tag name and format based on atom type
    let (tag_name, format_func): (&str, fn(&[u8], u32) -> Option<TagValue>) = match atom_type {
        // Text tags (UTF-8)
        b"\xa9nam" => ("ItemList:Title", format_text_tag),
        b"\xa9ART" => ("ItemList:Artist", format_text_tag),
        b"\xa9alb" => ("ItemList:Album", format_text_tag),
        b"\xa9day" => ("ItemList:ContentCreateDate", format_text_tag),
        b"\xa9cmt" => ("ItemList:Comment", format_text_tag),
        b"\xa9gen" => ("ItemList:Genre", format_text_tag),
        b"\xa9grp" => ("ItemList:Grouping", format_text_tag),
        b"\xa9lyr" => ("ItemList:Lyrics", format_text_tag),
        b"\xa9too" => ("ItemList:Encoder", format_text_tag),
        b"\xa9cpy" => ("ItemList:Copyright", format_text_tag),
        b"\xa9prd" => ("ItemList:Producer", format_text_tag),
        b"\xa9prf" => ("ItemList:Performer", format_text_tag),
        b"\xa9aut" => ("ItemList:Composer", format_text_tag),
        b"\xa9dir" => ("ItemList:Director", format_text_tag),
        b"\xa9inf" => ("ItemList:Information", format_text_tag),
        b"\xa9des" => ("ItemList:Description", format_text_tag),
        b"\xa9st3" => ("ItemList:Subtitle", format_text_tag),
        b"\xa9wrk" => ("ItemList:Work", format_text_tag),
        b"\xa9mvn" => ("ItemList:Movement", format_text_tag),
        b"\xa9mvm" => ("ItemList:MovementNumber", format_text_tag),
        b"\xa9sne" => ("ItemList:ShowName", format_text_tag),
        b"\xa9snm" => ("ItemList:ShowNameSort", format_text_tag),
        b"\xa9tvsh" => ("ItemList:TVShow", format_text_tag),
        b"\xa9tven" => ("ItemList:TVEpisodeID", format_text_tag),
        b"\xa9tvsn" => ("ItemList:TVSeason", format_text_tag),
        b"\xa9tvnn" => ("ItemList:TVNetworkName", format_text_tag),
        b"\xa9url" => ("ItemList:URL", format_text_tag),

        // Album artist tag
        b"aART" => ("ItemList:AlbumArtist", format_text_tag),

        // Copyright (alternate form)
        b"cprt" => ("ItemList:Copyright", format_text_tag),

        // Numeric/binary tags
        b"disk" => ("ItemList:DiskNumber", format_track_disk_tag),
        b"gnre" => ("ItemList:Genre", format_genre_tag),
        b"tmpo" => ("ItemList:BeatsPerMinute", format_integer_tag),
        b"trkn" => ("ItemList:TrackNumber", format_track_disk_tag),
        b"stik" => ("ItemList:MediaType", format_media_type_tag),
        b"pegn" => ("ItemList:ParentalAdvisory", format_integer_tag),
        b"tves" => ("ItemList:TVEpisodeNumber", format_integer_tag),
        b"tvsa" => ("ItemList:TVSeason", format_integer_tag),

        // Freeform atoms
        b"----" => ("ItemList:CustomTag", format_freeform_tag),

        // Fallback: try to interpret atom type as ASCII
        _ => {
            if std::str::from_utf8(atom_type).is_ok() {
                // Create a generic tag name from the atom type
                // This is stored in an owned string, so we can't return a borrowed reference
                // Instead, we'll handle it specially below
                return extract_unknown_itunes_item(atom_type, value_data, metadata);
            }
            return;
        }
    };

    // Format and add the value
    if let Some(value) = format_func(value_data, data_type) {
        metadata.insert(tag_name.to_string(), value.clone());

        // Also add QuickTime namespace versions for certain tags (matching ExifTool behavior)
        if let Some(qt_tag) = get_quicktime_tag_name(atom_type) {
            metadata.insert(qt_tag.to_string(), value);
        }
    }
}

/// Handle unknown iTunes atom types
fn extract_unknown_itunes_item(atom_type: &[u8], value_data: &[u8], metadata: &mut MetadataMap) {
    if let Ok(s) = std::str::from_utf8(atom_type) {
        let tag_name = format!("ItemList:{}", s);
        if let Some(value) = format_text_tag(value_data, 1) {
            metadata.insert(tag_name, value);
        }
    }
}

/// Get the QuickTime namespace tag name for a given iTunes atom type
/// ExifTool uses QuickTime: prefix for iTunes metadata
fn get_quicktime_tag_name(atom_type: &[u8]) -> Option<&'static str> {
    match atom_type {
        b"\xa9nam" => Some("QuickTime:Title"),
        b"\xa9ART" => Some("QuickTime:Artist"),
        b"\xa9alb" => Some("QuickTime:Album"),
        b"\xa9day" => Some("QuickTime:ContentCreateDate"),
        b"\xa9cmt" => Some("QuickTime:Comment"),
        b"\xa9gen" => Some("QuickTime:Genre"),
        b"\xa9grp" => Some("QuickTime:Grouping"),
        b"\xa9lyr" => Some("QuickTime:Lyrics"),
        b"\xa9too" => Some("QuickTime:Encoder"),
        b"\xa9cpy" => Some("QuickTime:Copyright"),
        b"\xa9prd" => Some("QuickTime:Producer"),
        b"\xa9prf" => Some("QuickTime:Performer"),
        b"\xa9aut" => Some("QuickTime:Composer"),
        b"\xa9dir" => Some("QuickTime:Director"),
        b"\xa9inf" => Some("QuickTime:Information"),
        b"\xa9des" => Some("QuickTime:Description"),
        b"\xa9st3" => Some("QuickTime:Subtitle"),
        b"\xa9wrk" => Some("QuickTime:Work"),
        b"\xa9mvn" => Some("QuickTime:Movement"),
        b"\xa9mvm" => Some("QuickTime:MovementNumber"),
        b"\xa9sne" => Some("QuickTime:ShowName"),
        b"\xa9snm" => Some("QuickTime:ShowNameSort"),
        b"\xa9tvsh" => Some("QuickTime:TVShow"),
        b"\xa9tven" => Some("QuickTime:TVEpisodeID"),
        b"\xa9tvsn" => Some("QuickTime:TVSeason"),
        b"\xa9tvnn" => Some("QuickTime:TVNetworkName"),
        b"\xa9url" => Some("QuickTime:URL"),
        b"aART" => Some("QuickTime:AlbumArtist"),
        b"cprt" => Some("QuickTime:Copyright"),
        b"disk" => Some("QuickTime:DiskNumber"),
        b"gnre" => Some("QuickTime:Genre"),
        b"tmpo" => Some("QuickTime:BeatsPerMinute"),
        b"trkn" => Some("QuickTime:TrackNumber"),
        b"stik" => Some("QuickTime:MediaType"),
        _ => None,
    }
}

/// Format a text value (UTF-8 or UTF-16)
fn format_text_tag(value_data: &[u8], data_type: u32) -> Option<TagValue> {
    match data_type {
        1 => {
            // UTF-8 text
            String::from_utf8(value_data.to_vec())
                .ok()
                .map(TagValue::String)
        }
        2 => {
            // UTF-16 big-endian text
            if value_data.len() % 2 != 0 {
                return None;
            }
            let utf16_chars: Vec<u16> = (0..value_data.len() / 2)
                .map(|i| u16::from_be_bytes([value_data[i * 2], value_data[i * 2 + 1]]))
                .collect();
            String::from_utf16(&utf16_chars).ok().map(TagValue::String)
        }
        _ => {
            // Try as UTF-8 fallback
            String::from_utf8(value_data.to_vec())
                .ok()
                .map(TagValue::String)
        }
    }
}

/// Format a track or disk number (binary format: reserved + current + total)
fn format_track_disk_tag(value_data: &[u8], _data_type: u32) -> Option<TagValue> {
    // Track/Disk format: 2 bytes reserved + 2 bytes current + 2 bytes total
    if value_data.len() < 6 {
        return None;
    }

    let current = u16::from_be_bytes([value_data[2], value_data[3]]) as u32;
    let total = u16::from_be_bytes([value_data[4], value_data[5]]) as u32;

    let formatted = if total > 0 {
        format!("{} of {}", current, total)
    } else {
        format!("{}", current)
    };

    Some(TagValue::String(formatted))
}

/// Format a genre tag (may be integer ID or string)
fn format_genre_tag(value_data: &[u8], data_type: u32) -> Option<TagValue> {
    match data_type {
        21 => {
            // Integer genre ID (ID3v1 style)
            if value_data.len() >= 2 {
                let genre_id = u16::from_be_bytes([value_data[0], value_data[1]]);
                // Map ID3v1 genre IDs to names (subset of common genres)
                let genre_name = match genre_id {
                    0 => "Blues",
                    1 => "Classic Rock",
                    2 => "Country",
                    3 => "Dance",
                    4 => "Disco",
                    5 => "Funk",
                    6 => "Grunge",
                    7 => "Hip-Hop",
                    8 => "Jazz",
                    9 => "Metal",
                    10 => "New Age",
                    11 => "Oldies",
                    12 => "Other",
                    13 => "Pop",
                    14 => "R&B",
                    15 => "Rap",
                    16 => "Reggae",
                    17 => "Rock",
                    18 => "Techno",
                    19 => "Industrial",
                    20 => "Alternative",
                    _ => return Some(TagValue::Integer(genre_id as i64)),
                };
                return Some(TagValue::String(genre_name.to_string()));
            }
            None
        }
        _ => {
            // String genre
            format_text_tag(value_data, data_type)
        }
    }
}

/// Format a signed integer value
fn format_integer_tag(value_data: &[u8], _data_type: u32) -> Option<TagValue> {
    match value_data.len() {
        1 => Some(TagValue::Integer(value_data[0] as i64)),
        2 => {
            let val = i16::from_be_bytes([value_data[0], value_data[1]]);
            Some(TagValue::Integer(val as i64))
        }
        4 => {
            let val =
                i32::from_be_bytes([value_data[0], value_data[1], value_data[2], value_data[3]]);
            Some(TagValue::Integer(val as i64))
        }
        _ => None,
    }
}

/// Format media type tag (stik atom)
/// Returns a human-readable media type description
fn format_media_type_tag(value_data: &[u8], _data_type: u32) -> Option<TagValue> {
    if value_data.is_empty() {
        return None;
    }

    let media_type_id = value_data[0];

    // Look up media type name from constant table
    for (id, name) in MEDIA_TYPES {
        if *id == media_type_id {
            return Some(TagValue::String(name.to_string()));
        }
    }

    // Unknown media type - return integer ID
    Some(TagValue::Integer(media_type_id as i64))
}

/// Format freeform atom (mean + name + value)
/// Freeform atoms have a special structure with namespace and name
fn format_freeform_tag(value_data: &[u8], _data_type: u32) -> Option<TagValue> {
    // For now, store as binary - freeform atoms are complex and rarely used
    Some(TagValue::Binary(value_data.to_vec()))
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

    // iTunes metadata tests
    #[test]
    fn test_itunes_text_tag_utf8() {
        // Test UTF-8 text tag formatting
        let value = b"Test Artist";
        let result = format_text_tag(value, 1);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("Test Artist"));
    }

    #[test]
    fn test_itunes_text_tag_utf16() {
        // Test UTF-16 big-endian text tag formatting
        // "Hi" in UTF-16 BE: 0x00, 0x48, 0x00, 0x69
        let value: &[u8] = &[0x00, 0x48, 0x00, 0x69];
        let result = format_text_tag(value, 2);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("Hi"));
    }

    #[test]
    fn test_itunes_track_number() {
        // Track number format: 2 bytes reserved + 2 bytes current + 2 bytes total
        // Example: track 5 of 12
        let value = vec![0x00, 0x00, 0x00, 0x05, 0x00, 0x0C];
        let result = format_track_disk_tag(&value, 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("5 of 12"));
    }

    #[test]
    fn test_itunes_track_number_no_total() {
        // Track number with no total specified
        let value = vec![0x00, 0x00, 0x00, 0x07, 0x00, 0x00];
        let result = format_track_disk_tag(&value, 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("7"));
    }

    #[test]
    fn test_itunes_disk_number() {
        // Disk number format: 2 bytes reserved + 2 bytes current + 2 bytes total
        // Example: disk 2 of 3
        let value = vec![0x00, 0x00, 0x00, 0x02, 0x00, 0x03];
        let result = format_track_disk_tag(&value, 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("2 of 3"));
    }

    #[test]
    fn test_itunes_genre_string() {
        // Genre as string
        let value = b"Rock";
        let result = format_genre_tag(value, 1);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("Rock"));
    }

    #[test]
    fn test_itunes_genre_id() {
        // Genre as ID3v1 integer (13 = Pop)
        let value: &[u8] = &[0x00, 0x0D];
        let result = format_genre_tag(value, 21);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("Pop"));
    }

    #[test]
    fn test_itunes_integer_1byte() {
        // 1-byte integer (e.g., parental advisory)
        let value: &[u8] = &[0x01];
        let result = format_integer_tag(value, 21);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_integer(), Some(1));
    }

    #[test]
    fn test_itunes_integer_2byte() {
        // 2-byte integer (BPM = 120)
        let value: &[u8] = &[0x00, 0x78];
        let result = format_integer_tag(value, 21);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_integer(), Some(120));
    }

    #[test]
    fn test_itunes_integer_4byte() {
        // 4-byte integer
        let value: &[u8] = &[0x00, 0x00, 0x04, 0xD2];
        let result = format_integer_tag(value, 21);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_integer(), Some(1234));
    }

    #[test]
    fn test_itunes_media_type_music() {
        // Media type: 0 = Music
        let value: &[u8] = &[0x00];
        let result = format_media_type_tag(value, 21);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("Music"));
    }

    #[test]
    fn test_itunes_media_type_audiobook() {
        // Media type: 2 = Audiobook
        let value: &[u8] = &[0x02];
        let result = format_media_type_tag(value, 21);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("Audiobook"));
    }

    #[test]
    fn test_itunes_media_type_tv_show() {
        // Media type: 10 = TV Show
        let value: &[u8] = &[0x0A];
        let result = format_media_type_tag(value, 21);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("TV Show"));
    }

    #[test]
    fn test_itunes_media_type_unknown() {
        // Unknown media type ID
        let value: &[u8] = &[0xFF];
        let result = format_media_type_tag(value, 21);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_integer(), Some(255));
    }

    #[test]
    fn test_itunes_item_extraction_title() {
        // Test extraction of a title tag
        let mut metadata = MetadataMap::with_capacity(4);
        let atom_type = b"\xa9nam";
        let value_data = b"My Song";

        extract_itunes_item(atom_type, 1, value_data, &mut metadata);

        // Should have both ItemList and QuickTime versions
        assert_eq!(
            metadata.get("ItemList:Title").unwrap().as_string(),
            Some("My Song")
        );
        assert_eq!(
            metadata.get("QuickTime:Title").unwrap().as_string(),
            Some("My Song")
        );
    }

    #[test]
    fn test_itunes_item_extraction_artist() {
        // Test extraction of an artist tag
        let mut metadata = MetadataMap::with_capacity(4);
        let atom_type = b"\xa9ART";
        let value_data = b"Artist Name";

        extract_itunes_item(atom_type, 1, value_data, &mut metadata);

        assert_eq!(
            metadata.get("ItemList:Artist").unwrap().as_string(),
            Some("Artist Name")
        );
        assert_eq!(
            metadata.get("QuickTime:Artist").unwrap().as_string(),
            Some("Artist Name")
        );
    }

    #[test]
    fn test_itunes_item_extraction_album_artist() {
        // Test extraction of album artist tag
        let mut metadata = MetadataMap::with_capacity(4);
        let atom_type = b"aART";
        let value_data = b"Album Artist";

        extract_itunes_item(atom_type, 1, value_data, &mut metadata);

        assert_eq!(
            metadata.get("ItemList:AlbumArtist").unwrap().as_string(),
            Some("Album Artist")
        );
        assert_eq!(
            metadata.get("QuickTime:AlbumArtist").unwrap().as_string(),
            Some("Album Artist")
        );
    }

    #[test]
    fn test_itunes_item_extraction_track_number() {
        // Test extraction of track number
        let mut metadata = MetadataMap::with_capacity(4);
        let atom_type = b"trkn";
        let value_data: &[u8] = &[0x00, 0x00, 0x00, 0x03, 0x00, 0x0C];

        extract_itunes_item(atom_type, 0, value_data, &mut metadata);

        assert_eq!(
            metadata.get("ItemList:TrackNumber").unwrap().as_string(),
            Some("3 of 12")
        );
    }

    #[test]
    fn test_quicktime_tag_name_mapping() {
        // Test that QuickTime tag names are correctly mapped
        assert_eq!(get_quicktime_tag_name(b"\xa9nam"), Some("QuickTime:Title"));
        assert_eq!(get_quicktime_tag_name(b"\xa9ART"), Some("QuickTime:Artist"));
        assert_eq!(
            get_quicktime_tag_name(b"aART"),
            Some("QuickTime:AlbumArtist")
        );
        assert_eq!(
            get_quicktime_tag_name(b"trkn"),
            Some("QuickTime:TrackNumber")
        );
        assert_eq!(get_quicktime_tag_name(b"xxxx"), None);
    }

    #[test]
    fn test_itunes_multiple_tags() {
        // Test extraction of multiple tags to metadata map
        let mut metadata = MetadataMap::with_capacity(16);

        // Add title
        extract_itunes_item(b"\xa9nam", 1, b"Test Song", &mut metadata);

        // Add artist
        extract_itunes_item(b"\xa9ART", 1, b"Test Artist", &mut metadata);

        // Add album
        extract_itunes_item(b"\xa9alb", 1, b"Test Album", &mut metadata);

        // Add track number
        let track_data: &[u8] = &[0x00, 0x00, 0x00, 0x05, 0x00, 0x0A];
        extract_itunes_item(b"trkn", 0, track_data, &mut metadata);

        // Verify all tags are present
        assert_eq!(
            metadata.get("ItemList:Title").unwrap().as_string(),
            Some("Test Song")
        );
        assert_eq!(
            metadata.get("ItemList:Artist").unwrap().as_string(),
            Some("Test Artist")
        );
        assert_eq!(
            metadata.get("ItemList:Album").unwrap().as_string(),
            Some("Test Album")
        );
        assert_eq!(
            metadata.get("ItemList:TrackNumber").unwrap().as_string(),
            Some("5 of 10")
        );
    }

    #[test]
    fn test_itunes_all_text_tags() {
        // Verify all text-based iTunes tags are properly mapped
        let text_tags: &[(&[u8], &str)] = &[
            (b"\xa9nam", "ItemList:Title"),
            (b"\xa9ART", "ItemList:Artist"),
            (b"\xa9alb", "ItemList:Album"),
            (b"\xa9day", "ItemList:ContentCreateDate"),
            (b"\xa9cmt", "ItemList:Comment"),
            (b"\xa9gen", "ItemList:Genre"),
            (b"\xa9grp", "ItemList:Grouping"),
            (b"\xa9lyr", "ItemList:Lyrics"),
            (b"\xa9too", "ItemList:Encoder"),
            (b"\xa9cpy", "ItemList:Copyright"),
            (b"\xa9prd", "ItemList:Producer"),
            (b"\xa9prf", "ItemList:Performer"),
            (b"\xa9aut", "ItemList:Composer"),
            (b"\xa9dir", "ItemList:Director"),
            (b"\xa9inf", "ItemList:Information"),
            (b"\xa9des", "ItemList:Description"),
            (b"\xa9st3", "ItemList:Subtitle"),
            (b"\xa9wrk", "ItemList:Work"),
            (b"\xa9mvn", "ItemList:Movement"),
            (b"\xa9mvm", "ItemList:MovementNumber"),
            (b"\xa9sne", "ItemList:ShowName"),
            (b"\xa9snm", "ItemList:ShowNameSort"),
            (b"\xa9tvsh", "ItemList:TVShow"),
            (b"\xa9tven", "ItemList:TVEpisodeID"),
            (b"\xa9tvsn", "ItemList:TVSeason"),
            (b"\xa9tvnn", "ItemList:TVNetworkName"),
            (b"\xa9url", "ItemList:URL"),
            (b"aART", "ItemList:AlbumArtist"),
            (b"cprt", "ItemList:Copyright"),
        ];

        for (atom_type, expected_tag) in text_tags {
            let mut metadata = MetadataMap::with_capacity(1);
            extract_itunes_item(atom_type, 1, b"Test", &mut metadata);
            assert!(
                metadata.contains_key(*expected_tag),
                "Tag {} not found for atom {:?}",
                expected_tag,
                atom_type
            );
        }
    }
}
