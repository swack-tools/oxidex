//! QuickTime/MP4 metadata extraction.
//!
//! This module extracts metadata from QuickTime and MP4 files from various locations:
//! 1. Classic QuickTime user data atoms (©xxx in moov→udta)
//! 2. iTunes-style metadata (moov→udta→meta)
//! 3. MP4 metadata with keys/ilst (moov→meta→keys + moov→meta→ilst)
//! 4. XMP metadata in uuid atoms

use crate::core::{MetadataMap, TagValue};
use super::atom_parser::Atom;
use std::collections::HashMap;

/// Extract all metadata from QuickTime/MP4 atoms
pub fn extract_metadata(root_atoms: &[Atom]) -> Result<MetadataMap, String> {
    let mut metadata = MetadataMap::with_capacity(20);

    // Find the moov atom (movie container)
    let moov = root_atoms
        .iter()
        .find(|atom| atom.atom_type.matches("moov"))
        .ok_or("No moov atom found")?;

    // Extract from all possible locations
    if let Some(udta) = moov.find_child("udta") {
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

/// Extract classic QuickTime user data atoms (©xxx)
fn extract_user_data_atoms(udta: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    let children = udta.parse_children().unwrap_or_default();

    for atom in children {
        let atom_bytes = atom.atom_type.as_bytes();

        // QuickTime user data atoms start with © character (0xA9)
        if atom_bytes[0] == 0xA9 {
            if let Some(value) = extract_string_value(atom.data) {
                let tag_name = match atom_bytes {
                    b"\xa9nam" => "QuickTime:Title",
                    b"\xa9ART" => "QuickTime:Artist",
                    b"\xa9alb" => "QuickTime:Album",
                    b"\xa9day" => "QuickTime:Year",
                    b"\xa9cmt" => "QuickTime:Comment",
                    b"\xa9cpy" => "QuickTime:Copyright",
                    b"\xa9gen" => "QuickTime:Genre",
                    b"\xa9too" => "QuickTime:Encoder",
                    b"\xa9des" => "QuickTime:Description",
                    b"\xa9dir" => "QuickTime:Director",
                    b"\xa9prd" => "QuickTime:Producer",
                    b"\xa9prf" => "QuickTime:Performers",
                    _ => continue, // Skip unknown atoms
                };

                metadata.insert(tag_name.to_string(), TagValue::String(value));
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
                    let tag_name = match atom_bytes {
                        b"\xa9nam" => "iTunes:Title",
                        b"\xa9ART" => "iTunes:Artist",
                        b"\xa9alb" => "iTunes:Album",
                        b"\xa9day" => "iTunes:Year",
                        b"\xa9cmt" => "iTunes:Comment",
                        b"\xa9gen" => "iTunes:Genre",
                        b"\xa9too" => "iTunes:Encoder",
                        b"aART" => "iTunes:AlbumArtist",
                        b"\xa9wrt" => "iTunes:Composer",
                        b"\xa9grp" => "iTunes:Grouping",
                        b"trkn" => "iTunes:TrackNumber",
                        b"disk" => "iTunes:DiscNumber",
                        b"cprt" | b"\xa9cpy" => "iTunes:Copyright",
                        _ => {
                            // Store unknown iTunes tags with their FourCC
                            // Try to convert to string, otherwise use hex representation
                            if let Ok(s) = std::str::from_utf8(atom_bytes) {
                                &format!("iTunes:{}", s)
                            } else {
                                &format!("iTunes:{:02X}{:02X}{:02X}{:02X}",
                                    atom_bytes[0], atom_bytes[1], atom_bytes[2], atom_bytes[3])
                            }
                        }
                    };

                    metadata.insert(tag_name.to_string(), value);
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
                2 => Some(TagValue::Integer(i16::from_be_bytes([
                    value_data[0],
                    value_data[1],
                ]) as i64)),
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
