//! Samsung SEFT/QDIOBS trailer reader (`Samsung.pm::ProcessSamsung`).

use crate::core::{MetadataMap, TagValue};

const QDIOBS: &[u8] = b"QDIOBS";
const SEFT: &[u8] = b"SEFT";
const SEFH: &[u8] = b"SEFH";
const SOUNDSHOT: u16 = 0x0100;

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Return the Samsung trailer boundary recognized by ProcessTrailers.  Samsung
/// may be immediately followed by a Vivo trailer: after Vivo consumes its
/// fixed EOF footer, ExifTool retries trailer identification at that positive
/// offset from EOF.  Do not accept arbitrary internal QDIO/SEFT signatures.
fn samsung_end_before_bounded_vivo(file: &[u8]) -> Option<usize> {
    let vivo_start = crate::parsers::vivo::validated_vivo_suffix_start(file)?;
    let qdio_start = vivo_start.checked_sub(QDIOBS.len())?;
    if file.get(qdio_start..vivo_start) == Some(QDIOBS)
        || file.get(vivo_start.checked_sub(b"\0\0SEFT".len())?..vivo_start) == Some(b"\0\0SEFT")
    {
        Some(vivo_start)
    } else {
        None
    }
}

/// Reads only a fully bounded Sound & Shot SEFT directory. The QDIOBS footer
/// identifies the outer block; its preceding length/type record locates the
/// SEFH directory without searching arbitrary JPEG payload for a header.
pub fn parse_samsung_trailer(file: &[u8]) -> MetadataMap {
    // ProcessSamsung seeks exactly six bytes back from EOF.  For `QDIOBS` it
    // rewinds two bytes, leaving `block_end` immediately after the QDIO type;
    // `BS` is a footer suffix, not a SEFT block type.
    let mut block_end = if file.ends_with(QDIOBS) {
        // Rewind from EOF past the `BS` footer suffix to the end of `QDIO`.
        file.len().saturating_sub(2)
    } else if file.ends_with(b"\0\0SEFT") {
        // The alternative form leaves SEFT itself as the final block type.
        file.len()
    } else if let Some(samsung_end) = samsung_end_before_bounded_vivo(file) {
        let Some(qdio_start) = samsung_end.checked_sub(QDIOBS.len()) else {
            return MetadataMap::new();
        };
        if file.get(qdio_start..samsung_end) == Some(QDIOBS) {
            let Some(block_end) = samsung_end.checked_sub(2) else {
                return MetadataMap::new();
            };
            block_end
        } else {
            samsung_end
        }
    } else {
        return MetadataMap::new();
    };
    loop {
        let Some(block_header) = block_end.checked_sub(8) else {
            return MetadataMap::new();
        };
        let Some(length) = read_u32(file, block_header).map(|value| value as usize) else {
            return MetadataMap::new();
        };
        let Some(block_type) = file.get(block_header + 4..block_end) else {
            return MetadataMap::new();
        };
        if !block_type
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            || !(4..0x10000).contains(&length)
            || length.saturating_add(8) >= block_end
        {
            return MetadataMap::new();
        }
        let Some(block_at) = block_header.checked_sub(length) else {
            return MetadataMap::new();
        };
        let Some(block) = file.get(block_at..block_header) else {
            return MetadataMap::new();
        };
        block_end = block_at;
        if block_type != SEFT {
            continue;
        }
        if !block.starts_with(SEFH) {
            return MetadataMap::new();
        }
        let Some(count) = read_u32(block, 8).map(|value| value as usize) else {
            continue;
        };
        let Some(directory_len) = 12usize.checked_add(count.checked_mul(12).unwrap_or(usize::MAX))
        else {
            continue;
        };
        if directory_len > block.len() {
            continue;
        }
        let mut metadata = MetadataMap::new();
        for index in 0..count {
            let Some(entry_at) = 12usize.checked_add(index.checked_mul(12).unwrap_or(usize::MAX))
            else {
                return MetadataMap::new();
            };
            let Some(entry) = block.get(entry_at..entry_at + 12) else {
                return MetadataMap::new();
            };
            let ty = u16::from_le_bytes([entry[2], entry[3]]);
            let offset =
                u32::from_le_bytes(entry[4..8].try_into().expect("entry has four bytes")) as usize;
            let size =
                u32::from_le_bytes(entry[8..12].try_into().expect("entry has four bytes")) as usize;
            if offset > block_at || size > offset || size < 8 {
                return MetadataMap::new();
            }
            if ty != SOUNDSHOT {
                continue;
            }
            let Some(data_at) = block_at.checked_sub(offset) else {
                return MetadataMap::new();
            };
            let Some(data_end) = data_at.checked_add(size) else {
                return MetadataMap::new();
            };
            let Some(data) = file.get(data_at..data_end) else {
                return MetadataMap::new();
            };
            let Some(name_len) = read_u32(data, 4).map(|value| value as usize) else {
                return MetadataMap::new();
            };
            let Some(name_end) = 8usize.checked_add(name_len) else {
                return MetadataMap::new();
            };
            let Some(name_bytes) = data.get(8..name_end) else {
                return MetadataMap::new();
            };
            // HandleTag receives the raw bytes.  Pinned ExifTool's public
            // JSON rendering substitutes `?` for an invalid byte (while -b
            // retains it), so withholding both Samsung tags would be less
            // faithful than preserving its public representation here.
            let name = String::from_utf8_lossy(name_bytes).replace('\u{fffd}', "?");
            metadata.insert_with_group1(
                "Samsung:EmbeddedAudioFileName",
                TagValue::new_string(name.trim_end_matches('\0')),
                "Samsung",
            );
            metadata.insert_with_group1(
                "Samsung:EmbeddedAudioFile",
                TagValue::new_binary(data[name_end..].to_vec()),
                "Samsung",
            );
            return metadata;
        }
    }
}
