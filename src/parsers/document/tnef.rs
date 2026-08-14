//! Transport Neutral Encapsulation Format (TNEF / winmail.dat) reader.
//!
//! This currently walks the message-property attribute and preserves its
//! `PR_CORRELATION_KEY` occurrences.  The byte walk is a direct port of
//! `Image::ExifTool::TNEF::ProcessProps` (TNEF.pm:277-395), rather than a
//! search for the sample's strings: property records carry type, id, optional
//! multivalue count, length-prefixed data and 4-byte padding.

use crate::core::{FileReader, MetadataMap, TagValue};

const TNEF_KEY: &[u8; 4] = b"\x78\x9f\x3e\x22";
const MESSAGE_PROPS: u32 = 0x069003;
const PR_CORRELATION_KEY: u16 = 0x007f;

fn le_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn le_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn padded(size: usize) -> Option<usize> {
    size.checked_add(3).map(|v| v & !3)
}

/// `TNEF.pm`'s `%propType` / `%fmtSize` fixed-width branch (lines 27-52,
/// 328-331).  `None` means a variable-sized property below, not an unknown
/// width to guess at.
fn fixed_width(kind: u16) -> Option<usize> {
    match kind {
        0x0001 => Some(0), // PT_NULL
        0x0002 | 0x000b => Some(2),
        0x0003 | 0x0004 | 0x000a => Some(4),
        0x0005 | 0x0006 | 0x0007 | 0x0014 | 0x0040 => Some(8),
        0x0048 => Some(16),
        _ => None,
    }
}

/// Decode the text forms that ExifTool returns unchanged for CorrelationKey.
/// Binary values are accepted only when they are valid UTF-8 after their
/// optional C-string terminator; otherwise we omit rather than invent a
/// rendering for an arbitrary MAPI binary property.
fn correlation_text(kind: u16, bytes: &[u8]) -> Option<String> {
    match kind {
        // PT_UNICODE, excluding the terminating UTF-16 NUL exactly as
        // TNEF.pm:370 does through Decode(..., 'UTF16') then s/\0+$//.
        0x001f => {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                .take_while(|unit| *unit != 0)
                .collect();
            String::from_utf16(&units).ok()
        }
        // PT_STRING8 and PT_BINARY. `RawConv => '$$val'` on CorrelationKey
        // deliberately dereferences the latter (TNEF.pm:145), so printable
        // binary keys are ordinary scalar output, not a binary placeholder.
        0x001e | 0x0102 => std::str::from_utf8(bytes)
            .ok()
            .map(|text| text.trim_end_matches('\0').to_owned()),
        _ => None,
    }
}

fn parse_message_props(data: &[u8], metadata: &mut MetadataMap) {
    let Some(entries) = le_u32(data, 0) else {
        return;
    };
    let mut pos = 4usize;

    for _ in 0..entries {
        let (Some(raw_type), Some(mut tag)) = (le_u16(data, pos), le_u16(data, pos + 2)) else {
            return;
        };
        pos += 4;
        // Named properties carry a GUID, an id kind and either a numeric id
        // or a padded UTF-16 name before the actual value.  We don't invent
        // names for unrecognised properties, but must walk this real framing
        // to reach later ordinary CorrelationKey records (TNEF.pm:294-322).
        if tag & 0x8000 != 0 {
            let (Some(id_kind), Some(name_len)) = (le_u32(data, pos + 16), le_u32(data, pos + 20))
            else {
                return;
            };
            pos += 24;
            if id_kind == 1 {
                let Some(skip) = padded(name_len as usize) else {
                    return;
                };
                let Some(next) = pos.checked_add(skip) else {
                    return;
                };
                if next > data.len() {
                    return;
                }
                pos = next;
            } else if id_kind != 0 {
                return;
            }
            // A named property cannot be PR_CORRELATION_KEY, whose exact
            // numeric id is 0x007f, so prevent accidental equality below.
            tag = 0;
        }
        let multi = raw_type & 0x1000 != 0;
        let kind = raw_type & 0x0fff;
        let count = if multi {
            let Some(count) = le_u32(data, pos) else {
                return;
            };
            pos += 4;
            count
        } else {
            1
        };

        if let Some(width) = fixed_width(kind) {
            let Some(total) = width.checked_mul(count as usize).and_then(padded) else {
                return;
            };
            let Some(next) = pos.checked_add(total) else {
                return;
            };
            if next > data.len() {
                return;
            }
            pos = next;
            continue;
        }

        if !matches!(kind, 0x001e | 0x001f | 0x0102 | 0x000d) {
            // The pinned table does not declare any other variable MAPI
            // format.  Stop rather than desynchronising the following tags.
            return;
        }
        for _ in 0..count {
            // Variable-sized MAPI types have the one-count quirk at
            // TNEF.pm:331-337 before their length.
            if !multi {
                pos = match pos.checked_add(4) {
                    Some(value) => value,
                    None => return,
                };
            }
            let Some(size) = le_u32(data, pos).map(|v| v as usize) else {
                return;
            };
            pos += 4;
            let Some(end) = pos.checked_add(size) else {
                return;
            };
            let Some(value) = data.get(pos..end) else {
                return;
            };

            if tag == PR_CORRELATION_KEY
                && let Some(value) = correlation_text(kind, value)
            {
                metadata.insert("File:CorrelationKey", TagValue::new_string(value));
            }
            let Some(next) = padded(size).and_then(|padding| pos.checked_add(padding)) else {
                return;
            };
            pos = next;
        }
    }
}

pub fn parse_tnef_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let data = reader
        .read(0, reader.size() as usize)
        .map_err(|err| err.to_string())?;
    if data.len() < 15 || !data.starts_with(TNEF_KEY) {
        return Err("invalid TNEF header".to_owned());
    }

    let mut metadata = MetadataMap::new();
    // ProcessTNEF starts after the 4-byte key and 2-byte legacy key. Every
    // attribute has level(1), tag(4), length(4), payload, checksum(2).
    let mut pos = 6usize;
    while let (Some(tag), Some(size)) = (le_u32(&data, pos + 1), le_u32(&data, pos + 5)) {
        let payload = pos + 9;
        let Some(end) = payload.checked_add(size as usize) else {
            break;
        };
        let Some(value) = data.get(payload..end) else {
            break;
        };
        if tag == MESSAGE_PROPS {
            parse_message_props(value, &mut metadata);
        }
        let Some(next) = end.checked_add(2) else {
            break;
        };
        pos = next;
    }
    Ok(metadata)
}
