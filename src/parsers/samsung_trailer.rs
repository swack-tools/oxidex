//! Samsung SEFT trailer parser.
//!
//! Samsung's `QDIOBS` trailer footer points back to a `SEFH` directory.  The
//! directory carries negatively-offset payload entries; only the SoundShot
//! entry is exposed here, matching ExifTool's two `EmbeddedAudioFile` tags.

use crate::core::{MetadataMap, TagValue};

const FOOTER: &[u8] = b"QDIOBS";
const HEADER: &[u8] = b"SEFH";
const SOUNDSHOT_TYPE: u16 = 0x0100;

pub fn parse_samsung_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(footer) = memchr::memmem::rfind(file, FOOTER) else {
        return metadata;
    };
    let Some(header) = memchr::memmem::rfind(&file[..footer], HEADER) else {
        return metadata;
    };
    let Some(count_bytes) = file.get(header + 8..header + 12) else {
        return metadata;
    };
    let count = u32::from_le_bytes(count_bytes.try_into().expect("four bytes")) as usize;
    let Some(directory_end) = header.checked_add(12 + count.saturating_mul(12)) else {
        return metadata;
    };
    if directory_end > footer {
        return metadata;
    }

    for index in 0..count {
        let at = header + 12 + index * 12;
        let Some(entry) = file.get(at..at + 12) else {
            return MetadataMap::new();
        };
        let ty = u16::from_le_bytes([entry[2], entry[3]]);
        if ty != SOUNDSHOT_TYPE {
            continue;
        }
        let offset = u32::from_le_bytes(entry[4..8].try_into().expect("four bytes")) as usize;
        let size = u32::from_le_bytes(entry[8..12].try_into().expect("four bytes")) as usize;
        let Some(block_at) = header.checked_sub(offset) else {
            continue;
        };
        let Some(block) = file.get(block_at..block_at.saturating_add(size)) else {
            continue;
        };
        let Some(name_len_bytes) = block.get(4..8) else {
            continue;
        };
        let name_len = u32::from_le_bytes(name_len_bytes.try_into().expect("four bytes")) as usize;
        let Some(name) = block.get(8..8 + name_len) else {
            continue;
        };
        let Some(value) = block.get(8 + name_len..) else {
            continue;
        };
        let Ok(name) = std::str::from_utf8(name) else {
            continue;
        };
        metadata.insert(
            "Samsung:EmbeddedAudioFileName",
            TagValue::new_string(name.trim_end_matches('\0')),
        );
        metadata.insert(
            "Samsung:EmbeddedAudioFile",
            TagValue::new_binary(value.to_vec()),
        );
        break;
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_footer_validated_soundshot_entry() {
        let mut file = Vec::new();
        let block_at = 0usize;
        file.extend_from_slice(&0u32.to_be_bytes());
        file.extend_from_slice(&(13u32).to_le_bytes());
        file.extend_from_slice(b"SoundShot_000");
        file.extend_from_slice(b"<dummy wav file>");
        let header = file.len();
        file.extend_from_slice(HEADER);
        file.extend_from_slice(&101u32.to_le_bytes());
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(&0u16.to_le_bytes());
        file.extend_from_slice(&SOUNDSHOT_TYPE.to_le_bytes());
        file.extend_from_slice(&((header - block_at) as u32).to_le_bytes());
        file.extend_from_slice(&(37u32).to_le_bytes());
        file.extend_from_slice(b"SEFT");
        file.extend_from_slice(FOOTER);

        let tags = parse_samsung_trailer(&file);
        assert_eq!(
            tags.get_string("Samsung:EmbeddedAudioFileName"),
            Some("SoundShot_000")
        );
        assert_eq!(
            tags.get("Samsung:EmbeddedAudioFile"),
            Some(&TagValue::new_binary(b"<dummy wav file>".to_vec()))
        );
    }
}
