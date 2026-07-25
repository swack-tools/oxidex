//! ASF/WMV (Advanced Systems Format) parser
//!
//! Implements metadata extraction from ASF, WMV, and WMA files.
//!
//! # Supported Metadata
//!
//! - **File Properties:** FileID, FileLength, CreationDate, DataPackets, Duration
//! - **Stream Properties:** StreamType, StreamNumber, codec info
//! - **Content Description:** Title, Author, Copyright, Description, Rating
//! - **Extended Content:** WM/* tags (ToolName, Publisher, Genre, Picture)
//!
//! # File Structure
//!
//! ASF uses GUID-based object hierarchy:
//! ```text
//! [Header Object]
//!   ├─ File Properties Object
//!   ├─ Stream Properties Object(s)
//!   ├─ Content Description Object
//!   ├─ Extended Content Description Object
//!   └─ ...
//! [Data Object]
//! [Index Object(s)]
//! ```
//!
//! # References
//!
//! - ASF Spec: Microsoft Advanced Systems Format (ASF) Specification 1.2
//! - ExifTool Source: `lib/Image/ExifTool/ASF.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

// ASF Header Object GUID: 30 26 B2 75 8E 66 CF 11 A6 D9 00 AA 00 62 CE 6C
const ASF_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];

// File Properties Object GUID
const FILE_PROPERTIES_GUID: [u8; 16] = [
    0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];

// Stream Properties Object GUID
const STREAM_PROPERTIES_GUID: [u8; 16] = [
    0x91, 0x07, 0xDC, 0xB7, 0xB7, 0xA9, 0xCF, 0x11, 0x8E, 0xE6, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];

// Content Description Object GUID
const CONTENT_DESCRIPTION_GUID: [u8; 16] = [
    0x33, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];

// Extended Content Description Object GUID
const EXTENDED_CONTENT_GUID: [u8; 16] = [
    0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0xF0, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x50,
];

// Header Extension Object GUID
const HEADER_EXTENSION_GUID: [u8; 16] = [
    0xB5, 0x03, 0xBF, 0x5F, 0x2E, 0xA9, 0xCF, 0x11, 0x8E, 0xE3, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];

// Codec List Object GUID
const CODEC_LIST_GUID: [u8; 16] = [
    0x40, 0x52, 0xD1, 0x86, 0x1D, 0x31, 0xD0, 0x11, 0xA3, 0xA4, 0x00, 0xA0, 0xC9, 0x03, 0x48, 0xF6,
];

// Metadata Library Object GUID: 44231C94-9498-49D1-A141-1D134E457054
const METADATA_LIBRARY_GUID: [u8; 16] = [
    0x94, 0x1C, 0x23, 0x44, 0x98, 0x94, 0xD1, 0x49, 0xA1, 0x41, 0x1D, 0x13, 0x4E, 0x45, 0x70, 0x54,
];

// Metadata Object GUID: C5F8CBEA-5BAF-4877-8467-AA8C44FA4CCA
const METADATA_GUID: [u8; 16] = [
    0xEA, 0xCB, 0xF8, 0xC5, 0xAF, 0x5B, 0x77, 0x48, 0x84, 0x67, 0xAA, 0x8C, 0x44, 0xFA, 0x4C, 0xCA,
];

// Stream Type GUIDs
const AUDIO_MEDIA_GUID: [u8; 16] = [
    0x40, 0x9E, 0x69, 0xF8, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];

const VIDEO_MEDIA_GUID: [u8; 16] = [
    0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];

/// ASF/WMV parser
pub struct AsfParser;

impl FormatParser for AsfParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify ASF header
        if reader.size() < 30 {
            return Err(ExifToolError::parse_error("File too small to be ASF"));
        }

        let header = reader.read(0, 30)?;

        // Check ASF header GUID
        if &header[0..16] != ASF_HEADER_GUID {
            return Err(ExifToolError::parse_error("Invalid ASF header GUID"));
        }

        let mut metadata = MetadataMap::with_capacity(32);

        // Parse header object
        let r = EndianReader::little_endian(&header);

        // Header object size (8 bytes at offset 16)
        let header_size = r.u64_at(16).unwrap_or(0);

        // Number of header objects (4 bytes at offset 24)
        let num_objects = r.u32_at(24).unwrap_or(0);

        // Parse header objects
        let mut offset = 30u64;
        let header_end = header_size.min(reader.size());

        for _ in 0..num_objects {
            if offset + 24 > header_end {
                break;
            }

            // Read object header (GUID + Size)
            let obj_header = reader.read(offset, 24)?;
            let obj_guid = &obj_header[0..16];
            let obj_r = EndianReader::little_endian(&obj_header);
            let obj_size = obj_r.u64_at(16).unwrap_or(0);

            if obj_size < 24 || offset + obj_size > header_end {
                break;
            }

            // Parse based on object type
            if obj_guid == FILE_PROPERTIES_GUID {
                parse_file_properties(reader, offset, obj_size, &mut metadata)?;
            } else if obj_guid == STREAM_PROPERTIES_GUID {
                parse_stream_properties(reader, offset, obj_size, &mut metadata)?;
            } else if obj_guid == CONTENT_DESCRIPTION_GUID {
                parse_content_description(reader, offset, obj_size, &mut metadata)?;
            } else if obj_guid == EXTENDED_CONTENT_GUID {
                parse_extended_content(reader, offset, obj_size, &mut metadata)?;
            } else if obj_guid == CODEC_LIST_GUID {
                parse_codec_list(reader, offset, obj_size, &mut metadata)?;
            } else if obj_guid == HEADER_EXTENSION_GUID {
                parse_header_extension(reader, offset, obj_size, &mut metadata)?;
            }

            offset += obj_size;
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::ASF)
    }
}

/// Convenience function to parse ASF metadata from a reader.
pub fn parse_asf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = AsfParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Format GUID as string (uppercase, with dashes)
fn format_guid(guid: &[u8]) -> String {
    if guid.len() != 16 {
        return String::from("(invalid)");
    }

    // ASF GUIDs are stored in mixed-endian format
    // First 3 components are little-endian, last 2 are big-endian
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        guid[3],
        guid[2],
        guid[1],
        guid[0],
        guid[5],
        guid[4],
        guid[7],
        guid[6],
        guid[8],
        guid[9],
        guid[10],
        guid[11],
        guid[12],
        guid[13],
        guid[14],
        guid[15]
    )
}

/// Parse File Properties Object
fn parse_file_properties(
    reader: &dyn FileReader,
    offset: u64,
    size: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    if size < 104 {
        return Ok(());
    }

    let data = reader.read(offset + 24, 80)?;
    let r = EndianReader::little_endian(&data);

    // File ID GUID (16 bytes)
    let file_id = format_guid(&data[0..16]);
    metadata.insert("ASF:FileID".to_string(), TagValue::new_string(file_id));

    // File size (8 bytes)
    let file_size = r.u64_at(16).unwrap_or(0);
    metadata.insert(
        "ASF:FileLength".to_string(),
        TagValue::new_integer(file_size as i64),
    );

    // Creation date (8 bytes - FILETIME format: 100-nanosecond intervals since 1601-01-01)
    let creation_time = r.u64_at(24).unwrap_or(0);
    if creation_time > 0 {
        let date_str = filetime_to_string(creation_time);
        metadata.insert(
            "ASF:CreationDate".to_string(),
            TagValue::new_string(date_str),
        );
    }

    // Data packets count (8 bytes)
    let data_packets = r.u64_at(32).unwrap_or(0);
    metadata.insert(
        "ASF:DataPackets".to_string(),
        TagValue::new_integer(data_packets as i64),
    );

    // Play duration (8 bytes - 100-nanosecond units) at offset 40
    let play_duration = r.u64_at(40).unwrap_or(0);
    // Send duration (8 bytes - 100-nanosecond units) at offset 48
    let send_duration = r.u64_at(48).unwrap_or(0);
    // Preroll (8 bytes - milliseconds) at offset 56
    let preroll = r.u64_at(56).unwrap_or(0);
    metadata.insert(
        "ASF:Preroll".to_string(),
        TagValue::new_integer(preroll as i64),
    );

    // Duration - ExifTool uses play_duration directly (no preroll subtraction for display)
    if play_duration > 0 {
        let duration_secs = (play_duration as f64) / 10_000_000.0;
        let duration_str = format_duration(duration_secs);
        metadata.insert(
            "ASF:Duration".to_string(),
            TagValue::new_string(duration_str),
        );
    }

    // Send duration - separate field from play_duration
    if send_duration > 0 {
        let send_duration_secs = (send_duration as f64) / 10_000_000.0;
        let send_duration_str = format_duration(send_duration_secs);
        metadata.insert(
            "ASF:SendDuration".to_string(),
            TagValue::new_string(send_duration_str),
        );
    }

    // Flags (4 bytes at offset 64)
    let flags = r.u32_at(64).unwrap_or(0);
    metadata.insert("ASF:Flags".to_string(), TagValue::new_integer(flags as i64));

    // Min packet size (4 bytes)
    let min_packet = r.u32_at(68).unwrap_or(0);
    metadata.insert(
        "ASF:MinPacketSize".to_string(),
        TagValue::new_integer(min_packet as i64),
    );

    // Max packet size (4 bytes)
    let max_packet = r.u32_at(72).unwrap_or(0);
    metadata.insert(
        "ASF:MaxPacketSize".to_string(),
        TagValue::new_integer(max_packet as i64),
    );

    // Max bitrate (4 bytes)
    let max_bitrate = r.u32_at(76).unwrap_or(0);
    let bitrate_kbps = (max_bitrate as f64) / 1000.0;
    metadata.insert(
        "ASF:MaxBitrate".to_string(),
        TagValue::new_string(format!("{:.1} kbps", bitrate_kbps)),
    );

    Ok(())
}

/// Parse Stream Properties Object
fn parse_stream_properties(
    reader: &dyn FileReader,
    offset: u64,
    size: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    if size < 78 {
        return Ok(());
    }

    let data = reader.read(offset + 24, 54)?;
    let r = EndianReader::little_endian(&data);

    // Stream type GUID (16 bytes)
    let stream_type_guid = &data[0..16];

    // Error correction type GUID (16 bytes)
    let error_correction_guid = &data[16..32];

    // Time offset (8 bytes)
    let time_offset = r.u64_at(32).unwrap_or(0);
    let time_offset_secs = (time_offset as f64) / 10_000_000.0;
    metadata.insert(
        "ASF:TimeOffset".to_string(),
        TagValue::new_string(format!("{} s", time_offset_secs as i64)),
    );

    // Type-specific data length (4 bytes)
    let type_data_len = r.u32_at(40).unwrap_or(0);

    // Error correction data length (4 bytes)
    // let error_data_len = r.u32_at(44).unwrap_or(0);

    // Flags (2 bytes) - contains stream number
    let flags = r.u16_at(48).unwrap_or(0);
    let stream_number = flags & 0x7F;
    metadata.insert(
        "ASF:StreamNumber".to_string(),
        TagValue::new_integer(stream_number as i64),
    );

    // Determine stream type
    let stream_type = if stream_type_guid == AUDIO_MEDIA_GUID {
        "Audio"
    } else if stream_type_guid == VIDEO_MEDIA_GUID {
        "Video"
    } else {
        "Unknown"
    };
    metadata.insert(
        "ASF:StreamType".to_string(),
        TagValue::new_string(stream_type),
    );

    // Error correction type - determine based on GUID
    // Audio Spread GUID: BFC3CD50-618F-11CF-8BB2-00AA00B4E220
    const AUDIO_SPREAD_GUID: [u8; 16] = [
        0x50, 0xCD, 0xC3, 0xBF, 0x8F, 0x61, 0xCF, 0x11, 0x8B, 0xB2, 0x00, 0xAA, 0x00, 0xB4, 0xE2,
        0x20,
    ];
    // No Error Correction GUID: 20FB5700-5B55-11CF-A8FD-00805F5C442B
    const NO_ERROR_CORRECTION_GUID: [u8; 16] = [
        0x00, 0x57, 0xFB, 0x20, 0x55, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44,
        0x2B,
    ];
    let error_correction = if error_correction_guid
        == [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ]
        || error_correction_guid == NO_ERROR_CORRECTION_GUID
    {
        "No Error Correction"
    } else if error_correction_guid == AUDIO_SPREAD_GUID {
        "Audio Spread"
    } else {
        // Unknown error correction type
        "Unknown"
    };
    metadata.insert(
        "ASF:ErrorCorrectionType".to_string(),
        TagValue::new_string(error_correction),
    );

    // Parse type-specific data at offset 54 from object start (after 24-byte header + 30-byte stream props header)
    // Structure: header(24) + stream_type(16) + error_correction(16) + time_offset(8) + type_len(4) + error_len(4) + flags(2) + reserved(4)
    // Type-specific data starts at offset 24 + 54 = 78
    if type_data_len > 0 && size >= 78 + type_data_len as u64 {
        let type_data = reader.read(offset + 78, type_data_len as usize)?;

        if stream_type_guid == AUDIO_MEDIA_GUID && type_data.len() >= 18 {
            // WAVEFORMATEX structure
            let audio_r = EndianReader::little_endian(&type_data);
            // let format_tag = audio_r.u16_at(0).unwrap_or(0);
            let channels = audio_r.u16_at(2).unwrap_or(0);
            let sample_rate = audio_r.u32_at(4).unwrap_or(0);
            // let avg_bytes_per_sec = audio_r.u32_at(8).unwrap_or(0);
            // let block_align = audio_r.u16_at(12).unwrap_or(0);
            // let bits_per_sample = audio_r.u16_at(14).unwrap_or(0);

            metadata.insert(
                "ASF:AudioChannels".to_string(),
                TagValue::new_integer(channels as i64),
            );
            metadata.insert(
                "ASF:AudioSampleRate".to_string(),
                TagValue::new_integer(sample_rate as i64),
            );
        } else if stream_type_guid == VIDEO_MEDIA_GUID && type_data.len() >= 11 {
            // Video type-specific data has a header before BITMAPINFOHEADER
            // According to ExifTool, ImageWidth is at offset 30 (54-24) and ImageHeight is at offset 34 (58-24)
            // In type-specific data, this means:
            // - Encoded Image Width at offset 0 (4 bytes)
            // - Encoded Image Height at offset 4 (4 bytes)
            // - Reserved Flags at offset 8 (1 byte)
            // - BITMAPINFOHEADER starts at offset 11
            let video_r = EndianReader::little_endian(&type_data);
            let width = video_r.u32_at(0).unwrap_or(0);
            let height = video_r.u32_at(4).unwrap_or(0);

            if width > 0 && width < 100000 {
                metadata.insert(
                    "ASF:ImageWidth".to_string(),
                    TagValue::new_integer(width as i64),
                );
            }
            if height > 0 && height < 100000 {
                metadata.insert(
                    "ASF:ImageHeight".to_string(),
                    TagValue::new_integer(height as i64),
                );
            }
        }
    }

    Ok(())
}

/// Parse Content Description Object
fn parse_content_description(
    reader: &dyn FileReader,
    offset: u64,
    size: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    if size < 34 {
        return Ok(());
    }

    let header = reader.read(offset + 24, 10)?;
    let r = EndianReader::little_endian(&header);

    let title_len = r.u16_at(0).unwrap_or(0) as usize;
    let author_len = r.u16_at(2).unwrap_or(0) as usize;
    let copyright_len = r.u16_at(4).unwrap_or(0) as usize;
    let description_len = r.u16_at(6).unwrap_or(0) as usize;
    let rating_len = r.u16_at(8).unwrap_or(0) as usize;

    let total_len = title_len + author_len + copyright_len + description_len + rating_len;
    if size < 34 + total_len as u64 {
        return Ok(());
    }

    let content = reader.read(offset + 34, total_len)?;
    let mut pos = 0;

    if title_len > 0 {
        let title = read_utf16_string(&content[pos..pos + title_len]);
        if !title.is_empty() {
            metadata.insert("ASF:Title".to_string(), TagValue::new_string(title));
        }
        pos += title_len;
    }

    if author_len > 0 {
        let author = read_utf16_string(&content[pos..pos + author_len]);
        if !author.is_empty() {
            metadata.insert("ASF:Author".to_string(), TagValue::new_string(author));
        }
        pos += author_len;
    }

    if copyright_len > 0 {
        let copyright = read_utf16_string(&content[pos..pos + copyright_len]);
        if !copyright.is_empty() {
            metadata.insert("ASF:Copyright".to_string(), TagValue::new_string(copyright));
        }
        pos += copyright_len;
    }

    if description_len > 0 {
        let description = read_utf16_string(&content[pos..pos + description_len]);
        if !description.is_empty() {
            metadata.insert(
                "ASF:Description".to_string(),
                TagValue::new_string(description),
            );
        }
        pos += description_len;
    }

    if rating_len > 0 {
        let rating = read_utf16_string(&content[pos..pos + rating_len]);
        if !rating.is_empty() {
            metadata.insert("ASF:Rating".to_string(), TagValue::new_string(rating));
        }
    }

    Ok(())
}

/// Parse Extended Content Description Object
fn parse_extended_content(
    reader: &dyn FileReader,
    offset: u64,
    size: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    if size < 26 {
        return Ok(());
    }

    let header = reader.read(offset + 24, 2)?;
    let r = EndianReader::little_endian(&header);
    let descriptor_count = r.u16_at(0).unwrap_or(0);

    let mut pos = offset + 26;
    let end_pos = offset + size;

    for _ in 0..descriptor_count {
        if pos + 4 > end_pos {
            break;
        }

        // Read name length
        let name_len_data = reader.read(pos, 2)?;
        let name_len = EndianReader::little_endian(&name_len_data)
            .u16_at(0)
            .unwrap_or(0) as usize;
        pos += 2;

        if pos + name_len as u64 + 4 > end_pos || name_len == 0 {
            break;
        }

        // Read name
        let name_data = reader.read(pos, name_len)?;
        let name = read_utf16_string(&name_data);
        pos += name_len as u64;

        // Read value type and length
        let value_header = reader.read(pos, 4)?;
        let value_r = EndianReader::little_endian(&value_header);
        let value_type = value_r.u16_at(0).unwrap_or(0);
        let value_len = value_r.u16_at(2).unwrap_or(0) as usize;
        pos += 4;

        if pos + value_len as u64 > end_pos {
            break;
        }

        // Read value
        let value_data = reader.read(pos, value_len)?;
        pos += value_len as u64;

        // Map WM/ names to ASF: tags
        let tag_name = map_wm_tag(&name);
        if tag_name.is_empty() {
            continue;
        }

        // Parse value based on type
        match value_type {
            0 => {
                // Unicode string
                let value = read_utf16_string(&value_data);
                if !value.is_empty() {
                    metadata.insert(tag_name, TagValue::new_string(value));
                }
            }
            1 => {
                // Byte array (binary)
                // Check for picture data
                if name == "WM/Picture" && value_data.len() > 10 {
                    parse_wm_picture(&value_data, metadata);
                } else {
                    metadata.insert(
                        tag_name,
                        TagValue::new_string(format!(
                            "(Binary data {} bytes, use -b option to extract)",
                            value_data.len()
                        )),
                    );
                }
            }
            2 => {
                // Bool
                if value_data.len() >= 4 {
                    let val = EndianReader::little_endian(&value_data)
                        .u32_at(0)
                        .unwrap_or(0);
                    let bool_str = if val != 0 { "true" } else { "false" };
                    metadata.insert(tag_name, TagValue::new_string(bool_str));
                }
            }
            3 => {
                // DWORD
                if value_data.len() >= 4 {
                    let val = EndianReader::little_endian(&value_data)
                        .u32_at(0)
                        .unwrap_or(0);
                    metadata.insert(tag_name, TagValue::new_integer(val as i64));
                }
            }
            4 => {
                // QWORD
                if value_data.len() >= 8 {
                    let val = EndianReader::little_endian(&value_data)
                        .u64_at(0)
                        .unwrap_or(0);
                    metadata.insert(tag_name, TagValue::new_integer(val as i64));
                }
            }
            5 => {
                // WORD
                if value_data.len() >= 2 {
                    let val = EndianReader::little_endian(&value_data)
                        .u16_at(0)
                        .unwrap_or(0);
                    metadata.insert(tag_name, TagValue::new_integer(val as i64));
                }
            }
            6 => {
                // GUID (16 bytes)
                if value_data.len() >= 16 {
                    let guid = format_guid(&value_data[0..16]);
                    metadata.insert(tag_name, TagValue::new_string(guid));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Parse Codec List Object
fn parse_codec_list(
    reader: &dyn FileReader,
    offset: u64,
    size: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    // Codec List Object structure:
    // 16 bytes: Object GUID
    // 8 bytes: Object Size
    // 16 bytes: Reserved GUID
    // 4 bytes: Codec Entries Count
    // Variable: Codec Entries (starting at offset 44)
    if size < 44 {
        return Ok(());
    }

    let header = reader.read(offset + 40, 4)?;
    let codec_count = EndianReader::little_endian(&header).u32_at(0).unwrap_or(0);

    let mut pos = offset + 44; // Start at codec entries (relative to object start)
    let end_pos = offset + size;
    let mut audio_idx = 0;
    let mut _video_idx = 0;

    for _ in 0..codec_count {
        if pos + 2 > end_pos {
            break;
        }

        // Codec type (2 bytes)
        let type_data = reader.read(pos, 2)?;
        let codec_type = EndianReader::little_endian(&type_data)
            .u16_at(0)
            .unwrap_or(0);
        pos += 2;

        // Codec name length (2 bytes)
        if pos + 2 > end_pos {
            break;
        }
        let name_len_data = reader.read(pos, 2)?;
        let name_len = EndianReader::little_endian(&name_len_data)
            .u16_at(0)
            .unwrap_or(0) as usize
            * 2;
        pos += 2;

        // Codec name (UTF-16)
        let codec_name = if name_len > 0 && pos + name_len as u64 <= end_pos {
            let name_data = reader.read(pos, name_len)?;
            pos += name_len as u64;
            read_utf16_string(&name_data)
        } else {
            String::new()
        };

        // Codec description length (2 bytes)
        if pos + 2 > end_pos {
            break;
        }
        let desc_len_data = reader.read(pos, 2)?;
        let desc_len = EndianReader::little_endian(&desc_len_data)
            .u16_at(0)
            .unwrap_or(0) as usize
            * 2;
        pos += 2;

        // Codec description (UTF-16)
        let codec_desc = if desc_len > 0 && pos + desc_len as u64 <= end_pos {
            let desc_data = reader.read(pos, desc_len)?;
            pos += desc_len as u64;
            read_utf16_string(&desc_data)
        } else {
            String::new()
        };

        // Codec information length (2 bytes)
        if pos + 2 > end_pos {
            break;
        }
        let info_len_data = reader.read(pos, 2)?;
        let info_len = EndianReader::little_endian(&info_len_data)
            .u16_at(0)
            .unwrap_or(0) as usize;
        pos += 2;

        // Codec information (binary - contains FourCC or format tag)
        let (codec_id, raw_format_tag) = if info_len > 0 && pos + info_len as u64 <= end_pos {
            let info_data = reader.read(pos, info_len)?;
            pos += info_len as u64;
            // For video: 4-byte FourCC
            // For audio: 2-byte format tag (little-endian)
            if info_data.len() >= 4 {
                (String::from_utf8_lossy(&info_data[0..4]).to_string(), None)
            } else if info_data.len() >= 2 {
                // Audio format tag - little-endian 16-bit
                let format_tag = u16::from_le_bytes([info_data[0], info_data[1]]);
                (map_audio_format_tag(format_tag), Some(format_tag))
            } else {
                (
                    info_data
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<String>(),
                    None,
                )
            }
        } else {
            (String::new(), None)
        };
        let _ = raw_format_tag; // May be used for future tag-specific handling

        // Store codec info based on type
        match codec_type {
            0x0001 => {
                // Video codec
                if !codec_name.is_empty() {
                    metadata.insert(
                        "ASF:VideoCodecName".to_string(),
                        TagValue::new_string(codec_name),
                    );
                    // Always output description when we have a codec (ExifTool does this)
                    metadata.insert(
                        "ASF:VideoCodecDescription".to_string(),
                        TagValue::new_string(codec_desc),
                    );
                }
                // Don't output VideoCodecID - ExifTool doesn't
                _video_idx += 1;
            }
            0x0002 => {
                // Audio codec
                if !codec_name.is_empty() {
                    let suffix = if audio_idx > 0 {
                        format!("_{}", audio_idx + 1)
                    } else {
                        String::new()
                    };
                    metadata.insert(
                        format!("ASF:AudioCodecName{}", suffix),
                        TagValue::new_string(codec_name),
                    );
                }
                if !codec_desc.is_empty() {
                    let suffix = if audio_idx > 0 {
                        format!("_{}", audio_idx + 1)
                    } else {
                        String::new()
                    };
                    metadata.insert(
                        format!("ASF:AudioCodecDescription{}", suffix),
                        TagValue::new_string(codec_desc),
                    );
                }
                if !codec_id.is_empty() {
                    let suffix = if audio_idx > 0 {
                        format!("_{}", audio_idx + 1)
                    } else {
                        String::new()
                    };
                    metadata.insert(
                        format!("ASF:AudioCodecID{}", suffix),
                        TagValue::new_string(codec_id),
                    );
                }
                audio_idx += 1;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Parse Header Extension Object
/// Contains nested objects including Metadata and Metadata Library
fn parse_header_extension(
    reader: &dyn FileReader,
    offset: u64,
    size: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    // Header Extension Object structure:
    // - 24 bytes: object header (GUID + size)
    // - 16 bytes: reserved field 1 (GUID)
    // - 2 bytes: reserved field 2
    // - 4 bytes: header extension data size
    // - N bytes: nested objects
    if size < 46 {
        return Ok(());
    }

    let header = reader.read(offset + 24, 22)?;
    let r = EndianReader::little_endian(&header);
    let data_size = r.u32_at(18).unwrap_or(0) as u64;

    let mut pos = offset + 46;
    let end_pos = (offset + 46 + data_size).min(offset + size);

    // Parse nested objects
    while pos + 24 <= end_pos {
        let obj_header = reader.read(pos, 24)?;
        let obj_guid = &obj_header[0..16];
        let obj_r = EndianReader::little_endian(&obj_header);
        let obj_size = obj_r.u64_at(16).unwrap_or(0);

        if obj_size < 24 || pos + obj_size > end_pos {
            break;
        }

        if obj_guid == METADATA_LIBRARY_GUID || obj_guid == METADATA_GUID {
            parse_metadata_object(reader, pos, obj_size, metadata)?;
        }

        pos += obj_size;
    }

    Ok(())
}

/// Parse Metadata or Metadata Library Object
/// These have a different structure than Extended Content Description:
/// - 2 bytes: description record count
/// - For each record:
///   - 2 bytes: language list index (or reserved)
///   - 2 bytes: stream number
///   - 2 bytes: name length
///   - 2 bytes: data type
///   - 4 bytes: data length
///   - N bytes: name (UTF-16)
///   - N bytes: data
fn parse_metadata_object(
    reader: &dyn FileReader,
    offset: u64,
    size: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    if size < 26 {
        return Ok(());
    }

    let header = reader.read(offset + 24, 2)?;
    let record_count = EndianReader::little_endian(&header).u16_at(0).unwrap_or(0);

    let mut pos = offset + 26;
    let end_pos = offset + size;

    // Track if we've already written IsVBR from this Metadata Object.
    // ExifTool uses the first stream's value and ignores others.
    let mut isvbr_written_from_metadata = false;

    for _ in 0..record_count {
        if pos + 12 > end_pos {
            break;
        }

        // Read record header
        let rec_header = reader.read(pos, 12)?;
        let rec_r = EndianReader::little_endian(&rec_header);
        // let language_idx = rec_r.u16_at(0).unwrap_or(0);
        // let stream_num = rec_r.u16_at(2).unwrap_or(0);
        let name_len = rec_r.u16_at(4).unwrap_or(0) as usize;
        let data_type = rec_r.u16_at(6).unwrap_or(0);
        let data_len = rec_r.u32_at(8).unwrap_or(0) as usize;
        pos += 12;

        if pos + name_len as u64 + data_len as u64 > end_pos {
            break;
        }

        // Read name
        let name_data = reader.read(pos, name_len)?;
        let name = read_utf16_string(&name_data);
        pos += name_len as u64;

        // Read value
        let value_data = reader.read(pos, data_len)?;
        pos += data_len as u64;

        // Map WM/ names to ASF: tags
        let tag_name = map_wm_tag(&name);
        if tag_name.is_empty() {
            continue;
        }

        // For IsVBR, ExifTool uses the first Metadata Object value (typically stream 1)
        // and ignores subsequent stream values. Once we've written IsVBR from this
        // Metadata Object, skip any further IsVBR entries.
        if tag_name == "ASF:IsVBR" && isvbr_written_from_metadata {
            continue;
        }

        // Parse value based on type
        let value = match data_type {
            0 => {
                // Unicode string
                TagValue::new_string(read_utf16_string(&value_data))
            }
            1 => {
                // Byte array - could be GUID or binary data
                if value_data.len() == 16 {
                    // GUID - format as string
                    TagValue::new_string(format_guid(&value_data))
                } else {
                    // Binary data
                    TagValue::new_string(format!("(Binary data {} bytes)", value_data.len()))
                }
            }
            2 => {
                // BOOL - should output "true" or "false"
                let v = if value_data.len() >= 4 {
                    u32::from_le_bytes([value_data[0], value_data[1], value_data[2], value_data[3]])
                        != 0
                } else if value_data.len() >= 2 {
                    u16::from_le_bytes([value_data[0], value_data[1]]) != 0
                } else {
                    continue;
                };
                TagValue::new_string(if v { "true" } else { "false" })
            }
            4 => {
                // QWORD (8 bytes)
                if value_data.len() >= 8 {
                    let v = u64::from_le_bytes([
                        value_data[0],
                        value_data[1],
                        value_data[2],
                        value_data[3],
                        value_data[4],
                        value_data[5],
                        value_data[6],
                        value_data[7],
                    ]);
                    TagValue::Integer(v as i64)
                } else if value_data.len() >= 4 {
                    let v = u32::from_le_bytes([
                        value_data[0],
                        value_data[1],
                        value_data[2],
                        value_data[3],
                    ]);
                    TagValue::Integer(v as i64)
                } else {
                    continue;
                }
            }
            3 => {
                // DWORD
                if value_data.len() >= 4 {
                    let v = u32::from_le_bytes([
                        value_data[0],
                        value_data[1],
                        value_data[2],
                        value_data[3],
                    ]);
                    TagValue::Integer(v as i64)
                } else {
                    continue;
                }
            }
            5 => {
                // WORD
                if value_data.len() >= 2 {
                    let v = u16::from_le_bytes([value_data[0], value_data[1]]);
                    TagValue::Integer(v as i64)
                } else {
                    continue;
                }
            }
            6 => {
                // GUID
                if value_data.len() >= 16 {
                    TagValue::new_string(format_guid(&value_data))
                } else {
                    continue;
                }
            }
            _ => continue,
        };

        // Track if we're writing IsVBR so we skip subsequent entries
        if tag_name == "ASF:IsVBR" {
            isvbr_written_from_metadata = true;
        }

        metadata.insert(tag_name, value);
    }

    Ok(())
}

/// Parse WM/Picture tag value
fn parse_wm_picture(data: &[u8], metadata: &mut MetadataMap) {
    if data.len() < 5 {
        return;
    }

    // Picture type (1 byte)
    let picture_type = data[0];
    let picture_type_str = match picture_type {
        0 => "Other",
        1 => "32x32 File Icon",
        2 => "Other File Icon",
        3 => "Front Cover",
        4 => "Back Cover",
        5 => "Leaflet Page",
        6 => "Media",
        7 => "Lead Artist",
        8 => "Artist",
        9 => "Conductor",
        10 => "Band",
        11 => "Composer",
        12 => "Lyricist",
        13 => "Recording Location",
        14 => "During Recording",
        15 => "During Performance",
        16 => "Video Capture",
        17 => "A Bright Coloured Fish",
        18 => "Illustration",
        19 => "Band Logotype",
        20 => "Publisher Logotype",
        _ => "Unknown",
    };
    metadata.insert(
        "ASF:PictureType".to_string(),
        TagValue::new_string(picture_type_str),
    );

    // Picture data size (4 bytes, little-endian)
    if data.len() < 5 {
        return;
    }
    let pic_size = EndianReader::little_endian(&data[1..5])
        .u32_at(0)
        .unwrap_or(0);

    // MIME type string (null-terminated UTF-16)
    let mut pos = 5;
    let mime_start = pos;
    while pos + 2 <= data.len() {
        if data[pos] == 0 && data[pos + 1] == 0 {
            break;
        }
        pos += 2;
    }
    let mime_type = read_utf16_string(&data[mime_start..pos]);
    if !mime_type.is_empty() {
        metadata.insert(
            "ASF:PictureMIMEType".to_string(),
            TagValue::new_string(mime_type),
        );
    }
    pos += 2; // Skip null terminator

    // Description string (null-terminated UTF-16)
    let desc_start = pos;
    while pos + 2 <= data.len() {
        if data[pos] == 0 && data[pos + 1] == 0 {
            break;
        }
        pos += 2;
    }
    let description = read_utf16_string(&data[desc_start..pos]);
    if !description.is_empty() {
        metadata.insert(
            "ASF:PictureDescription".to_string(),
            TagValue::new_string(description),
        );
    }

    // Picture data follows
    metadata.insert(
        "ASF:Picture".to_string(),
        TagValue::new_string(format!(
            "(Binary data {} bytes, use -b option to extract)",
            pic_size
        )),
    );
}

/// Map WM/* tag names to ASF: tag names
fn map_wm_tag(name: &str) -> String {
    let clean_name = name.trim_start_matches("WM/");
    match clean_name {
        "ToolName" => "ASF:ToolName".to_string(),
        "ToolVersion" => "ASF:ToolVersion".to_string(),
        "Publisher" => "ASF:Publisher".to_string(),
        "Genre" => "ASF:Genre".to_string(),
        "Picture" => "ASF:Picture".to_string(),
        "IsVBR" => "ASF:IsVBR".to_string(),
        "MediaClassPrimaryID" => "ASF:MediaClassPrimaryID".to_string(),
        "MediaClassSecondaryID" => "ASF:MediaClassSecondaryID".to_string(),
        "ASFLeakyBucketPairs" => "ASF:ASFLeakyBucketPairs".to_string(),
        "WMADRCAverageReference" => "ASF:WMADRCAverageReference".to_string(),
        "WMADRCPeakReference" => "ASF:WMADRCPeakReference".to_string(),
        _ => {
            // Unknown tag - include as ASF: tag if it has WM/ prefix
            // or if it's a known ASF-specific tag pattern
            if name.starts_with("WM/") {
                format!("ASF:{}", clean_name)
            } else if clean_name.starts_with("MediaClass")
                || clean_name.starts_with("WMADRC")
                || clean_name.starts_with("ASF")
            {
                format!("ASF:{}", clean_name)
            } else {
                String::new()
            }
        }
    }
}

/// Map audio format tag to codec name (from RIFF audioEncoding table)
fn map_audio_format_tag(tag: u16) -> String {
    match tag {
        0x0001 => "Microsoft PCM".to_string(),
        0x0002 => "Microsoft ADPCM".to_string(),
        0x0003 => "IEEE Float".to_string(),
        0x0006 => "ITU G.711 A-law".to_string(),
        0x0007 => "ITU G.711 mu-law".to_string(),
        0x0010 => "OKI ADPCM".to_string(),
        0x0011 => "Intel DVI/IMA ADPCM".to_string(),
        0x0012 => "Videologic MediaSpace ADPCM".to_string(),
        0x0020 => "Yamaha ADPCM".to_string(),
        0x0055 => "MPEG Layer 3".to_string(),
        0x0160 => "Microsoft Audio1".to_string(),
        0x0161 => "Windows Media Audio V2 V7 V8 V9 / DivX audio (WMA) / Alex AC3 Audio".to_string(),
        0x0162 => "Windows Media Audio Professional V9".to_string(),
        0x0163 => "Windows Media Audio Lossless V9".to_string(),
        0x0164 => "WMA Pro over S/PDIF".to_string(),
        0x0200 => "Creative Labs ADPCM".to_string(),
        0x2000 => "AC3".to_string(),
        0x2001 => "DTS".to_string(),
        0xFFFE => "Extensible Wave Format".to_string(),
        _ => format!("Unknown (0x{:04X})", tag),
    }
}

/// Read UTF-16LE string, stopping at null terminator
fn read_utf16_string(data: &[u8]) -> String {
    if data.len() < 2 {
        return String::new();
    }

    let mut chars: Vec<u16> = Vec::with_capacity(data.len() / 2);
    let mut i = 0;

    while i + 2 <= data.len() {
        let c = ((data[i + 1] as u16) << 8) | (data[i] as u16);
        if c == 0 {
            break;
        }
        chars.push(c);
        i += 2;
    }

    String::from_utf16_lossy(&chars)
}

/// Convert FILETIME (100-nanosecond intervals since 1601-01-01) to string
fn filetime_to_string(filetime: u64) -> String {
    // FILETIME epoch: 1601-01-01
    // Unix epoch: 1970-01-01
    // Difference: 11644473600 seconds = 116444736000000000 * 100ns
    const FILETIME_UNIX_DIFF: u64 = 116_444_736_000_000_000;

    if filetime < FILETIME_UNIX_DIFF {
        return String::from("0000:00:00 00:00:00Z");
    }

    let unix_100ns = filetime - FILETIME_UNIX_DIFF;
    let unix_secs = unix_100ns / 10_000_000;

    // Calculate date/time components
    let secs_per_day = 86400u64;
    let days = unix_secs / secs_per_day;
    let day_secs = unix_secs % secs_per_day;

    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Calculate year/month/day
    let (year, month, day) = days_to_ymd(days as i64);

    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to year/month/day
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
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
    for &mdays in &month_days {
        if remaining < mdays {
            break;
        }
        remaining -= mdays;
        month += 1;
    }

    (year, month, (remaining + 1) as u32)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Format duration as H:MM:SS
fn format_duration(secs: f64) -> String {
    let total_secs = secs as u64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    format!("{}:{:02}:{:02}", hours, minutes, seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_asf_signature_valid() {
        // Minimal ASF header
        let mut data = vec![0u8; 50];
        data[0..16].copy_from_slice(&ASF_HEADER_GUID);
        data[16..24].copy_from_slice(&30u64.to_le_bytes()); // Size
        data[24..28].copy_from_slice(&0u32.to_le_bytes()); // Object count

        let reader = TestReader::from_slice(&data);
        let parser = AsfParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());
    }

    #[test]
    fn test_asf_signature_invalid() {
        let data = b"INVALID DATA FOR ASF";
        let reader = TestReader::from_slice(data);
        let parser = AsfParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_guid_format() {
        let guid = [
            0xC4, 0xB0, 0x69, 0x5F, 0xF7, 0x04, 0x21, 0x4B, 0x98, 0x42, 0x46, 0xCC, 0xA5, 0x42,
            0xD8, 0xD3,
        ];
        let formatted = format_guid(&guid);
        assert_eq!(formatted, "5F69B0C4-04F7-4B21-9842-46CCA542D8D3");
    }
}
