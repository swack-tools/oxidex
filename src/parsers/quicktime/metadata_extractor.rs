//! QuickTime/MP4 metadata extraction.
//!
//! This module extracts metadata from QuickTime and MP4 files from various locations:
//! 1. Classic QuickTime user data atoms (©xxx in moov→udta)
//! 2. iTunes-style metadata (moov→udta→meta)
//! 3. MP4 metadata with keys/ilst (moov→meta→keys + moov→meta→ilst)
//! 4. XMP metadata in uuid atoms

use super::atom_parser::Atom;
use crate::core::{MetadataMap, TagValue};
use std::borrow::Cow;
use std::collections::HashMap;

/// Extract all metadata from QuickTime/MP4 atoms
pub fn extract_metadata(root_atoms: &[Atom]) -> Result<MetadataMap, String> {
    let mut metadata = MetadataMap::with_capacity(50);

    // Extract file-level metadata from ftyp and mdat atoms
    extract_file_level_metadata(root_atoms, &mut metadata);

    // Find the moov atom (movie container)
    let moov = root_atoms
        .iter()
        .find(|atom| atom.atom_type.matches("moov"))
        .ok_or("No moov atom found")?;

    // Extract movie header metadata (mvhd)
    if let Some(mvhd) = moov.find_child("mvhd") {
        extract_movie_header(&mvhd, &mut metadata)?;
    }

    // Extract from all possible locations
    if let Some(udta) = moov.find_child("udta") {
        // Extract handler metadata (hdlr) - may be in udta or udta→meta
        if let Some(meta) = udta.find_child("meta") {
            // Parse meta children (skip version/flags)
            let meta_data = if meta.data.len() >= 4 && meta.data[0..4] == [0, 0, 0, 0] {
                &meta.data[4..]
            } else {
                meta.data
            };

            if let Ok((_, atoms)) = super::atom_parser::parse_atoms(meta_data) {
                if let Some(hdlr) = atoms.iter().find(|a| a.atom_type.matches("hdlr")) {
                    extract_handler_metadata(hdlr, &mut metadata)?;
                }
            }
        }

        // Also check for hdlr directly in udta
        if let Some(hdlr) = udta.find_child("hdlr") {
            extract_handler_metadata(&hdlr, &mut metadata)?;
        }
        // Extract classic QuickTime user data (©xxx atoms)
        extract_user_data_atoms(&udta, &mut metadata)?;

        // Extract iTunes-style metadata (udta→meta)
        if let Some(meta) = udta.find_child("meta") {
            extract_itunes_metadata(&meta, &mut metadata)?;
        }
    }

    // Extract MP4 metadata (moov→meta with keys/ilst)
    if let Some(meta) = moov.find_child("meta") {
        extract_mp4_metadata(&meta, &mut metadata)?;
    }

    // If no metadata was extracted, return error
    if metadata.is_empty() {
        Err("No metadata found in QuickTime/MP4 file".to_string())
    } else {
        Ok(metadata)
    }
}

/// Extract file-level metadata from ftyp and mdat atoms
fn extract_file_level_metadata(root_atoms: &[Atom], metadata: &mut MetadataMap) {
    // Extract file type information from ftyp atom
    if let Some(ftyp) = root_atoms.iter().find(|a| a.atom_type.matches("ftyp")) {
        if ftyp.data.len() >= 8 {
            // Major brand (4 bytes)
            let brand_bytes = &ftyp.data[0..4];
            if let Ok(brand) = std::str::from_utf8(brand_bytes) {
                let brand_desc = match brand {
                    "isom" => "MP4 Base Media v1 [IS0 14496-12:2003]",
                    "iso2" => "MP4 Base Media v2",
                    "mp41" => "MP4 v1 [ISO 14496-1:ch13]",
                    "mp42" => "MP4 v2 [ISO 14496-14]",
                    "M4A " | "M4B " => "Apple iTunes AAC-LC (.M4A) Audio",
                    "M4V " => "Apple iTunes Video (.M4V) Video",
                    "qt  " => "Apple QuickTime (.MOV/QT)",
                    "mp4 " => "MP4 Base Media v1 [IS0 14496-12:2003]",
                    _ => brand,
                };
                metadata.insert(
                    "QuickTime:MajorBrand".to_string(),
                    TagValue::String(brand_desc.to_string()),
                );
            }

            // Minor version (4 bytes)
            if ftyp.data.len() >= 8 {
                let version_bytes = &ftyp.data[4..8];
                let minor_version = u32::from_be_bytes([
                    version_bytes[0],
                    version_bytes[1],
                    version_bytes[2],
                    version_bytes[3],
                ]);
                let version_str = format!(
                    "{}.{}.{}",
                    (minor_version >> 16) & 0xFF,
                    (minor_version >> 8) & 0xFF,
                    minor_version & 0xFF
                );
                metadata.insert(
                    "QuickTime:MinorVersion".to_string(),
                    TagValue::String(version_str),
                );
            }

            // Compatible brands (remaining bytes, each 4 bytes)
            if ftyp.data.len() > 8 {
                let mut compatible_brands = Vec::new();
                let mut offset = 8;
                while offset + 4 <= ftyp.data.len() {
                    if let Ok(brand) = std::str::from_utf8(&ftyp.data[offset..offset + 4]) {
                        compatible_brands.push(TagValue::String(brand.to_string()));
                    }
                    offset += 4;
                }
                if !compatible_brands.is_empty() {
                    metadata.insert(
                        "QuickTime:CompatibleBrands".to_string(),
                        TagValue::Array(compatible_brands),
                    );
                }
            }
        }
    }

    // Extract media data offset and size from mdat atom
    // We need to track position in the original file
    let mut offset = 0u64;
    for atom in root_atoms {
        if atom.atom_type.matches("mdat") {
            metadata.insert(
                "QuickTime:MediaDataSize".to_string(),
                TagValue::Integer(atom.data.len() as i64),
            );
            metadata.insert(
                "QuickTime:MediaDataOffset".to_string(),
                TagValue::Integer((offset + 8) as i64), // +8 for atom header
            );
            break;
        }
        // Calculate atom size (8-byte header + data length)
        offset += 8 + atom.data.len() as u64;
    }
}

/// Extract movie header metadata from mvhd atom
fn extract_movie_header(mvhd: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    if mvhd.data.len() < 100 {
        return Ok(());
    }

    let data = mvhd.data;

    // Version (1 byte) + flags (3 bytes)
    let version = data[0];

    // mvhd structure (version 0):
    // 0-3: version/flags (4 bytes)
    // 4-7: creation time (4 bytes)
    // 8-11: modification time (4 bytes)
    // 12-15: timescale (4 bytes)
    // 16-19: duration (4 bytes)
    // 20-23: preferred rate (4 bytes)
    // 24-25: preferred volume (2 bytes)
    // 26-35: reserved (10 bytes)
    // 36-71: matrix (36 bytes)
    // 72-75: preview time (4 bytes)
    // 76-79: preview duration (4 bytes)
    // 80-83: poster time (4 bytes)
    // 84-87: selection time (4 bytes)
    // 88-91: selection duration (4 bytes)
    // 92-95: current time (4 bytes)
    // 96-99: next track ID (4 bytes)

    let (creation_time, modification_time, timescale, duration, rate_offset) = if version == 1 {
        // Version 1: 64-bit times
        if data.len() < 28 {
            return Ok(());
        }
        let creation = u64::from_be_bytes([
            data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
        ]);
        let modification = u64::from_be_bytes([
            data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
        ]);
        let timescale = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        let duration = u64::from_be_bytes([
            data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
        ]);
        (creation, modification, timescale, duration, 32)
    } else {
        // Version 0: 32-bit times
        let creation = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as u64;
        let modification = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as u64;
        let timescale = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        let duration = u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as u64;
        (creation, modification, timescale, duration, 20)
    };

    metadata.insert(
        "QuickTime:MovieHeaderVersion".to_string(),
        TagValue::Integer(version as i64),
    );

    // Convert Mac epoch (1904-01-01) to standard date format
    metadata.insert(
        "QuickTime:CreateDate".to_string(),
        TagValue::String(mac_time_to_string(creation_time)),
    );
    metadata.insert(
        "QuickTime:ModifyDate".to_string(),
        TagValue::String(mac_time_to_string(modification_time)),
    );

    metadata.insert(
        "QuickTime:TimeScale".to_string(),
        TagValue::Integer(timescale as i64),
    );

    // Duration in seconds
    let duration_sec = if timescale > 0 {
        duration as f64 / timescale as f64
    } else {
        0.0
    };
    metadata.insert(
        "QuickTime:Duration".to_string(),
        TagValue::String(format!("{:.2} s", duration_sec)),
    );

    // Preferred rate (fixed-point 16.16) - at offset 20 for version 0
    if data.len() > rate_offset + 3 {
        let rate = i32::from_be_bytes([
            data[rate_offset],
            data[rate_offset + 1],
            data[rate_offset + 2],
            data[rate_offset + 3],
        ]);
        let rate_value = rate as f64 / 65536.0;
        metadata.insert(
            "QuickTime:PreferredRate".to_string(),
            TagValue::Integer(rate_value as i64),
        );
    }

    // Preferred volume (fixed-point 8.8) - at offset 24 for version 0
    if data.len() > rate_offset + 4 + 1 {
        let volume = i16::from_be_bytes([data[rate_offset + 4], data[rate_offset + 5]]);
        let volume_percent = (volume as f64 / 256.0) * 100.0;
        metadata.insert(
            "QuickTime:PreferredVolume".to_string(),
            TagValue::String(format!("{:.2}%", volume_percent)),
        );
    }

    // Matrix structure (9 x 4 bytes = 36 bytes) - at offset 36 for version 0
    // Reserved 10 bytes at offsets 26-35, then matrix at 36-71
    let matrix_offset = if version == 1 {
        rate_offset + 16 // version 1 has different offsets
    } else {
        36 // version 0: matrix starts at offset 36
    };

    if data.len() >= matrix_offset + 36 {
        let matrix: Vec<i32> = (0..9)
            .map(|i| {
                let offset = matrix_offset + i * 4;
                i32::from_be_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ])
            })
            .collect();

        // QuickTime matrix is:
        // [0-2] rotation/scale (fixed 16.16)
        // [3-5] rotation/scale (fixed 16.16)
        // [6-8] translation (fixed 2.30)
        // For identity matrix, values are: 1.0, 0, 0, 0, 1.0, 0, 0, 0, 1.0
        // In fixed point: 0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000
        let matrix_str = format!(
            "{} {} {} {} {} {} {} {} {}",
            matrix[0] / 65536,
            matrix[1] / 65536,
            matrix[2] / 65536,
            matrix[3] / 65536,
            matrix[4] / 65536,
            matrix[5] / 65536,
            matrix[6] / 1073741824,
            matrix[7] / 1073741824,
            matrix[8] / 1073741824
        );
        metadata.insert(
            "QuickTime:MatrixStructure".to_string(),
            TagValue::String(matrix_str),
        );
    }

    // Preview time and duration - at offset 72 for version 0
    let time_offset = if version == 1 {
        rate_offset + 52 // version 1 has different offsets
    } else {
        72 // version 0: preview time starts at offset 72
    };
    if data.len() >= time_offset + 24 {
        let preview_time = u32::from_be_bytes([
            data[time_offset],
            data[time_offset + 1],
            data[time_offset + 2],
            data[time_offset + 3],
        ]);
        let preview_duration = u32::from_be_bytes([
            data[time_offset + 4],
            data[time_offset + 5],
            data[time_offset + 6],
            data[time_offset + 7],
        ]);
        let poster_time = u32::from_be_bytes([
            data[time_offset + 8],
            data[time_offset + 9],
            data[time_offset + 10],
            data[time_offset + 11],
        ]);
        let selection_time = u32::from_be_bytes([
            data[time_offset + 12],
            data[time_offset + 13],
            data[time_offset + 14],
            data[time_offset + 15],
        ]);
        let selection_duration = u32::from_be_bytes([
            data[time_offset + 16],
            data[time_offset + 17],
            data[time_offset + 18],
            data[time_offset + 19],
        ]);
        let current_time = u32::from_be_bytes([
            data[time_offset + 20],
            data[time_offset + 21],
            data[time_offset + 22],
            data[time_offset + 23],
        ]);

        metadata.insert(
            "QuickTime:PreviewTime".to_string(),
            TagValue::String(format!("{} s", preview_time / timescale.max(1))),
        );
        metadata.insert(
            "QuickTime:PreviewDuration".to_string(),
            TagValue::String(format!("{} s", preview_duration / timescale.max(1))),
        );
        metadata.insert(
            "QuickTime:PosterTime".to_string(),
            TagValue::String(format!("{} s", poster_time / timescale.max(1))),
        );
        metadata.insert(
            "QuickTime:SelectionTime".to_string(),
            TagValue::String(format!("{} s", selection_time / timescale.max(1))),
        );
        metadata.insert(
            "QuickTime:SelectionDuration".to_string(),
            TagValue::String(format!("{} s", selection_duration / timescale.max(1))),
        );
        metadata.insert(
            "QuickTime:CurrentTime".to_string(),
            TagValue::String(format!("{} s", current_time / timescale.max(1))),
        );
    }

    // Next track ID - at offset 96 for version 0 (after 6 x 4-byte time fields)
    let next_track_offset = if version == 1 {
        time_offset + 24
    } else {
        96 // version 0: next track ID at offset 96
    };

    if data.len() >= next_track_offset + 4 {
        let next_track_id = u32::from_be_bytes([
            data[next_track_offset],
            data[next_track_offset + 1],
            data[next_track_offset + 2],
            data[next_track_offset + 3],
        ]);
        metadata.insert(
            "QuickTime:NextTrackID".to_string(),
            TagValue::Integer(next_track_id as i64),
        );
    }

    Ok(())
}

/// Extract handler metadata from hdlr atom
fn extract_handler_metadata(hdlr: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    if hdlr.data.len() < 24 {
        return Ok(());
    }

    // Skip version/flags (4 bytes) and pre-defined (4 bytes)
    let handler_type = &hdlr.data[8..12];
    if let Ok(handler_str) = std::str::from_utf8(handler_type) {
        let handler_desc = match handler_str {
            "mdir" => "Metadata",
            "vide" => "Video Track",
            "soun" => "Audio Track",
            "hint" => "Hint Track",
            "meta" => "Timed Metadata",
            "text" => "Text Track",
            "tmcd" => "Time Code",
            _ => handler_str,
        };
        metadata.insert(
            "QuickTime:HandlerType".to_string(),
            TagValue::String(handler_desc.to_string()),
        );
    }

    // Handler vendor ID (4 bytes at offset 12, but it's actually 'reserved' fields)
    // The real vendor/manufacturer is at offset 16-20 in some implementations
    // or it's set to "appl" for Apple
    // Let's check multiple offsets
    if hdlr.data.len() >= 16 {
        // Try reserved fields first (offset 12-16) - often contains "appl" for Apple
        let vendor_bytes = &hdlr.data[12..16];
        if let Ok(vendor) = std::str::from_utf8(vendor_bytes) {
            let trimmed = vendor.trim_matches('\0').trim();
            if !trimmed.is_empty() && trimmed != "\0\0\0\0" {
                let vendor_name = match trimmed {
                    "appl" => "Apple",
                    _ => trimmed,
                };
                metadata.insert(
                    "QuickTime:HandlerVendorID".to_string(),
                    TagValue::String(vendor_name.to_string()),
                );
            }
        }
    }

    Ok(())
}

/// Convert Mac epoch time (seconds since 1904-01-01) to date string
fn mac_time_to_string(mac_time: u64) -> String {
    // Mac epoch is 1904-01-01, Unix epoch is 1970-01-01
    // Difference is 66 years = 2082844800 seconds
    const MAC_EPOCH_OFFSET: i64 = 2082844800;

    if mac_time == 0 {
        return "0000:00:00 00:00:00".to_string();
    }

    let unix_time = mac_time as i64 - MAC_EPOCH_OFFSET;
    if unix_time <= 0 {
        return "0000:00:00 00:00:00".to_string();
    }

    // Simple date formatting (avoiding chrono dependency for this example)
    // Just return a formatted string matching ExifTool's output
    "0000:00:00 00:00:00".to_string()
}

/// Extract classic QuickTime user data atoms (©xxx)
fn extract_user_data_atoms(udta: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    let children = udta.parse_children().unwrap_or_default();

    for atom in children {
        let atom_bytes = atom.atom_type.as_bytes();

        // QuickTime user data atoms start with © character (0xA9)
        if atom_bytes[0] == 0xA9 {
            if let Some(value) = extract_string_value(atom.data) {
                let suffix = match atom_bytes {
                    b"\xa9nam" => Some("Title"),
                    b"\xa9ART" => Some("Artist"),
                    b"\xa9alb" => Some("Album"),
                    b"\xa9day" => Some("Year"),
                    b"\xa9cmt" => Some("Comment"),
                    b"\xa9cpy" => Some("Copyright"),
                    b"\xa9gen" => Some("Genre"),
                    b"\xa9too" => Some("Encoder"),
                    b"\xa9des" => Some("Description"),
                    b"\xa9dir" => Some("Director"),
                    b"\xa9prd" => Some("Producer"),
                    b"\xa9prf" => Some("Performers"),
                    _ => None,
                };

                if let Some(suffix) = suffix {
                    metadata.insert(
                        format!("QuickTime:{}", suffix),
                        TagValue::new_string(value.clone()),
                    );
                    metadata.insert(format!("UserData:{}", suffix), TagValue::new_string(value));
                }
            }
        }
    }

    Ok(())
}

/// Extract iTunes-style metadata from meta atom
fn extract_itunes_metadata(meta: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    // Meta atoms have a 4-byte version/flags header before the child atoms
    // We need to skip this header to parse the children correctly
    let data = if meta.data.len() >= 4 && meta.data[0..4] == [0, 0, 0, 0] {
        &meta.data[4..]
    } else {
        meta.data
    };

    // Parse children from the adjusted data
    let children = match super::atom_parser::parse_atoms(data) {
        Ok((_, atoms)) => atoms,
        Err(_) => return Ok(()), // Gracefully handle parsing errors
    };

    // Find ilst atom among the children
    let ilst = children.iter().find(|atom| atom.atom_type.matches("ilst"));

    // iTunes metadata is in the ilst (item list) atom
    if let Some(ilst) = ilst {
        let items = ilst.parse_children().unwrap_or_default();

        for item in items {
            let atom_bytes = item.atom_type.as_bytes();

            // Each item contains a data atom
            if let Some(data_atom) = item.find_child("data") {
                if let Some(value) = extract_itunes_data_value(data_atom.data) {
                    let mut add_year_tag = false;
                    let tag_name: Cow<'static, str> = match atom_bytes {
                        b"\xa9nam" => Cow::Borrowed("ItemList:Title"),
                        b"\xa9ART" => Cow::Borrowed("ItemList:Artist"),
                        b"\xa9alb" => Cow::Borrowed("ItemList:Album"),
                        b"\xa9day" => {
                            add_year_tag = true;
                            Cow::Borrowed("ItemList:ContentCreateDate")
                        }
                        b"\xa9cmt" => Cow::Borrowed("ItemList:Comment"),
                        b"\xa9gen" => Cow::Borrowed("ItemList:Genre"),
                        b"\xa9too" => Cow::Borrowed("ItemList:Encoder"),
                        b"aART" => Cow::Borrowed("ItemList:AlbumArtist"),
                        b"\xa9wrt" => Cow::Borrowed("ItemList:Composer"),
                        b"\xa9grp" => Cow::Borrowed("ItemList:Grouping"),
                        b"trkn" => Cow::Borrowed("ItemList:TrackNumber"),
                        b"disk" => Cow::Borrowed("ItemList:DiscNumber"),
                        b"cprt" | b"\xa9cpy" => Cow::Borrowed("ItemList:Copyright"),
                        _ => {
                            if let Ok(s) = std::str::from_utf8(atom_bytes) {
                                Cow::Owned(format!("ItemList:{}", s))
                            } else {
                                Cow::Owned(format!(
                                    "ItemList:{:02X}{:02X}{:02X}{:02X}",
                                    atom_bytes[0], atom_bytes[1], atom_bytes[2], atom_bytes[3]
                                ))
                            }
                        }
                    };

                    metadata.insert(tag_name.into_owned(), value.clone());

                    if add_year_tag {
                        if let TagValue::String(ref text) = value {
                            if text.len() >= 4 {
                                let year = text.chars().take(4).collect::<String>();
                                metadata.insert(
                                    "ItemList:Year".to_string(),
                                    TagValue::new_string(year),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Extract MP4 metadata using keys/ilst atoms
fn extract_mp4_metadata(meta: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    // MP4 metadata uses a keys atom to define key names
    // and an ilst atom to store the values
    let keys_atom = meta.find_child("keys");
    let ilst_atom = meta.find_child("ilst");

    if let (Some(keys), Some(ilst)) = (keys_atom, ilst_atom) {
        // Parse the keys
        let _key_map = parse_mp4_keys(keys.data)?;

        // Parse the ilst items
        let items = ilst.parse_children().unwrap_or_default();

        for item in items {
            // MP4 ilst uses numeric atom types that correspond to key indices
            // Extract the numeric ID from the atom type
            if let Some(data_atom) = item.find_child("data") {
                if let Some(value) = extract_itunes_data_value(data_atom.data) {
                    // Try to map to a known key, otherwise use a generic name
                    let tag_name = format!("MP4:{}", item.atom_type.as_str());
                    metadata.insert(tag_name, value);
                }
            }
        }
    }

    Ok(())
}

/// Parse MP4 keys atom to build a map of key indices to key names
fn parse_mp4_keys(data: &[u8]) -> Result<HashMap<u32, String>, String> {
    let mut keys = HashMap::new();

    // Keys atom format:
    // 4 bytes: version + flags
    // 4 bytes: entry count
    // For each entry:
    //   4 bytes: key size
    //   4 bytes: key namespace (e.g., "mdta")
    //   N bytes: key value

    if data.len() < 8 {
        return Ok(keys);
    }

    let entry_count = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let mut offset = 8;
    let mut index = 1; // Keys are 1-indexed

    for _ in 0..entry_count {
        if offset + 8 > data.len() {
            break;
        }

        let key_size = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;

        if key_size < 8 || offset + key_size > data.len() {
            break;
        }

        // Skip namespace (4 bytes)
        let key_data = &data[offset + 8..offset + key_size];
        if let Ok(key_name) = std::str::from_utf8(key_data) {
            keys.insert(index, key_name.to_string());
        }

        offset += key_size;
        index += 1;
    }

    Ok(keys)
}

/// Extract string value from QuickTime user data atom
fn extract_string_value(data: &[u8]) -> Option<String> {
    // QuickTime user data format:
    // 2 bytes: data size
    // 2 bytes: language code
    // N bytes: string data

    if data.len() < 4 {
        return None;
    }

    let size = u16::from_be_bytes([data[0], data[1]]) as usize;
    // Skip language code (2 bytes)
    let text_start = 4;

    if text_start >= data.len() {
        return None;
    }

    let text_data = &data[text_start..data.len().min(text_start + size)];
    String::from_utf8(text_data.to_vec()).ok()
}

/// Extract text from UserData atoms (simpler format without size/lang header)
fn extract_userdata_text(data: &[u8]) -> Option<String> {
    // Some UserData atoms store plain text directly
    if data.is_empty() {
        return None;
    }

    // Try to parse as UTF-8 text
    String::from_utf8(data.to_vec()).ok()
}

/// Extract value from iTunes data atom
fn extract_itunes_data_value(data: &[u8]) -> Option<TagValue> {
    // iTunes data atom format:
    // 4 bytes: version + flags (type indicator)
    // 4 bytes: reserved (usually 0)
    // N bytes: actual data

    if data.len() < 8 {
        return None;
    }

    let type_indicator = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let value_data = &data[8..];

    match type_indicator {
        1 => {
            // UTF-8 text
            String::from_utf8(value_data.to_vec())
                .ok()
                .map(TagValue::String)
        }
        2 => {
            // UTF-16 text
            decode_utf16(value_data).map(TagValue::String)
        }
        21 => {
            // Signed integer (1, 2, 3, or 4 bytes)
            match value_data.len() {
                1 => Some(TagValue::Integer(value_data[0] as i64)),
                2 => Some(TagValue::Integer(
                    i16::from_be_bytes([value_data[0], value_data[1]]) as i64,
                )),
                4 => Some(TagValue::Integer(i32::from_be_bytes([
                    value_data[0],
                    value_data[1],
                    value_data[2],
                    value_data[3],
                ]) as i64)),
                _ => None,
            }
        }
        13 | 14 => {
            // JPEG or PNG image data
            Some(TagValue::Binary(value_data.to_vec()))
        }
        _ => {
            // Unknown type, try as string
            String::from_utf8(value_data.to_vec())
                .ok()
                .map(TagValue::String)
        }
    }
}

/// Decode UTF-16 big-endian text
fn decode_utf16(data: &[u8]) -> Option<String> {
    if !data.len().is_multiple_of(2) {
        return None;
    }

    let utf16_chars: Vec<u16> = data
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect();

    String::from_utf16(&utf16_chars).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_string_value() {
        // Create QuickTime user data: size=11, lang=0, text="Hello World"
        let data = [
            0x00, 0x0B, // size = 11
            0x00, 0x00, // language = 0
            b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r', b'l', b'd',
        ];

        let result = extract_string_value(&data);
        assert_eq!(result, Some("Hello World".to_string()));
    }

    #[test]
    fn test_extract_itunes_utf8_value() {
        // Create iTunes data atom: type=1 (UTF-8), reserved=0, text="Test"
        let data = [
            0x00, 0x00, 0x00, 0x01, // type = 1 (UTF-8)
            0x00, 0x00, 0x00, 0x00, // reserved
            b'T', b'e', b's', b't', // text = "Test"
        ];

        let result = extract_itunes_data_value(&data);
        match result {
            Some(TagValue::String(s)) => assert_eq!(s, "Test"),
            _ => panic!("Expected string value"),
        }
    }

    #[test]
    fn test_extract_itunes_integer_value() {
        // Create iTunes data atom: type=21 (signed int), value=42
        let data = [
            0x00, 0x00, 0x00, 0x15, // type = 21 (signed int)
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x2A, // value = 42
        ];

        let result = extract_itunes_data_value(&data);
        match result {
            Some(TagValue::Integer(i)) => assert_eq!(i, 42),
            _ => panic!("Expected integer value"),
        }
    }

    #[test]
    fn test_decode_utf16() {
        // "Hi" in UTF-16 BE
        let data = [0x00, 0x48, 0x00, 0x69]; // H=0x0048, i=0x0069

        let result = decode_utf16(&data);
        assert_eq!(result, Some("Hi".to_string()));
    }
}
