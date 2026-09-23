//! Samsung SEFT/QDIOBS trailer reader (`Samsung.pm::ProcessSamsung`), and the
//! `ProcessTrailers` walk that reaches it and the Vivo trailer.

use crate::core::{MetadataMap, TagValue};

const QDIOBS: &[u8] = b"QDIOBS";
const DIRECT_SEFT: &[u8] = b"\0\0SEFT";
const SEFT: &[u8] = b"SEFT";
const SEFH: &[u8] = b"SEFH";
const SOUNDSHOT: u16 = 0x0100;
/// IdentifyTrailer reads at most 64 bytes before the current end.
const IDENTIFY_WINDOW: usize = 64;

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// ProcessTrailers' inward walk (ExifTool.pm:7042-7182) over the trailers this
/// crate can size, emitting the Samsung and Vivo trailer tags it reaches.
///
/// Each step classifies the bytes ending at the current end exactly as
/// IdentifyTrailer (ExifTool.pm:6989-7029) does, lets that trailer's proc size
/// it, and continues at its start. A trailer type this walk cannot size
/// (AFCP, FotoStation, CanonVRD, Insta360, NikonApp, OnePlus, Google) or any
/// proc failure ends the walk, so an inner trailer behind it is omitted rather
/// than guessed at; an interior marker is never accepted on its own.
///
/// `jpeg_trailer_start` is the JPEG `TrailerStart` (the byte after EOI).
/// ExifTool sets it only for JPEG when reading; without it ProcessVivo returns
/// 0 and the walk stops at a Vivo trailer.
pub fn parse_trailer_chain(file: &[u8], jpeg_trailer_start: Option<usize>) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let mut end = file.len();
    while end > 0 {
        let window = &file[end.saturating_sub(IDENTIFY_WINDOW)..end];
        let start = if is_unsized_trailer(window) {
            None
        } else if window.ends_with(b"cbipcbbl") {
            photo_mechanic_start(file, end)
        } else if window.starts_with(b"CANON OPTIONAL DATA\0") {
            None
        } else if let Some(start) = crate::parsers::mie::trailer_start_ending_at(file, end) {
            Some(start)
        } else if window.ends_with(b"\0\0QDIOBS") || window.ends_with(DIRECT_SEFT) {
            let samsung = parse_samsung_trailer(file, end);
            // `Samsung::Trailer` has `PRIORITY => 0`: the first one wins.
            if !metadata.contains_key("MakerNotes:EmbeddedAudioFileName") {
                metadata.merge_winners_keeping_group1(&samsung.metadata);
            }
            samsung.data_pos
        } else if crate::parsers::vivo::has_footer_at(file, end) {
            jpeg_trailer_start
                .and_then(|trailer_start| {
                    crate::parsers::vivo::process_vivo(file, end, trailer_start)
                })
                .map(|vivo| {
                    if let Some(json) = vivo.json
                        && !metadata.contains_key("Trailer:JSONInfo")
                    {
                        metadata.insert_with_group1(
                            "Trailer:JSONInfo",
                            TagValue::new_string(json),
                            "Vivo",
                        );
                    }
                    vivo.start
                })
        } else {
            None
        };
        // `last unless $result > 0 and $dirLen`, then the JPEG-only stop once
        // a trailer starts at or before TrailerStart.
        let Some(start) = start.filter(|start| *start < end) else {
            break;
        };
        if jpeg_trailer_start.is_some_and(|trailer_start| start <= trailer_start) {
            break;
        }
        end = start;
    }
    metadata
}

/// IdentifyTrailer types checked before PhotoMechanic whose length this walk
/// does not model: AFCP (`AXS[!*]` twelve bytes from the end) and FotoStation.
fn is_unsized_trailer(window: &[u8]) -> bool {
    let afcp = window.len() >= 12 && {
        let at = window.len() - 12;
        &window[at..at + 3] == b"AXS" && matches!(window[at + 3], b'!' | b'*')
    };
    afcp || window.ends_with(b"\xa1\xb2\xc3\xd4")
}

/// PhotoMechanic.pm:157-169: the fixed footer's unsigned big-endian size must
/// reach back inside the file; the trailer is `size + 12` bytes long.
fn photo_mechanic_start(file: &[u8], end: usize) -> Option<usize> {
    let footer = file.get(end.checked_sub(12)?..end)?;
    let size = u32::from_be_bytes(footer[..4].try_into().ok()?);
    if size & 0x8000_0000 != 0 {
        return None;
    }
    (end - 12).checked_sub(usize::try_from(size).ok()?)
}

/// What ProcessSamsung produced: the tags it handled, and its `DataPos` when
/// it returned success so ProcessTrailers may continue inward.
struct SamsungTrailer {
    metadata: MetadataMap,
    data_pos: Option<usize>,
}

impl SamsungTrailer {
    fn failed() -> Self {
        Self {
            metadata: MetadataMap::new(),
            data_pos: None,
        }
    }
}

/// ProcessSamsung (Samsung.pm:1721-1873) for the trailer ending at `end`:
/// reads only a fully bounded Sound & Shot SEFT directory, walking the
/// length/type-delimited blocks backward from the `QDIOBS` or `\0\0SEFT` end.
fn parse_samsung_trailer(file: &[u8], end: usize) -> SamsungTrailer {
    let Some(last6) = end.checked_sub(6).and_then(|at| file.get(at..end)) else {
        return SamsungTrailer::failed();
    };
    // For `QDIOBS` ProcessSamsung rewinds past the `BS` footer suffix, leaving
    // the block end after the QDIO type; `\0\0SEFT` ends the SEFT block itself.
    let block_end = if last6 == QDIOBS {
        end - 2
    } else if last6 == DIRECT_SEFT {
        end
    } else {
        return SamsungTrailer::failed();
    };
    parse_samsung_from_block_end(file, block_end).unwrap_or_else(SamsungTrailer::failed)
}

fn parse_samsung_from_block_end(file: &[u8], mut block_end: usize) -> Option<SamsungTrailer> {
    loop {
        let block_header = block_end.checked_sub(8)?;
        let length = read_u32(file, block_header)? as usize;
        let block_type = file.get(block_header + 4..block_end)?;
        if !block_type
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            || !(4..0x10000).contains(&length)
            || length.saturating_add(8) >= block_end
        {
            return None;
        }
        let block_at = block_header.checked_sub(length)?;
        let block = file.get(block_at..block_header)?;
        block_end = block_at;
        if block_type != SEFT {
            continue;
        }
        // (Samsung Gallery junk tolerance is not modelled: omit instead.)
        if !block.starts_with(SEFH) {
            return None;
        }
        let count = read_u32(block, 8)? as usize;
        let directory_len = count.checked_mul(12)?.checked_add(12)?;
        if directory_len > block.len() {
            return None;
        }
        let entries = &block[12..directory_len];
        let entry_u32 = |entry: &[u8], at: usize| {
            u32::from_le_bytes(entry[at..at + 4].try_into().expect("entry is 12 bytes")) as usize
        };
        // `$firstBlock` is the largest negative offset, taken before any
        // entry is validated; DataPos is that far before the directory.
        let first_block = entries
            .chunks_exact(12)
            .map(|entry| entry_u32(entry, 4))
            .max()
            .unwrap_or(0);
        let mut data_pos = block_at.checked_sub(first_block);
        let mut metadata = MetadataMap::new();
        for entry in entries.chunks_exact(12) {
            let ty = u16::from_le_bytes([entry[2], entry[3]]);
            let offset = entry_u32(entry, 4);
            let size = entry_u32(entry, 8);
            if offset > block_at || size > offset || size < 8 {
                // `last SamBlock`: tags already handled stay, but the proc
                // returns 0 and ProcessTrailers stops.
                data_pos = None;
                break;
            }
            let data = &file[block_at - offset..block_at - offset + size];
            let name_len = read_u32(data, 4)? as usize;
            let Some(name_end) = name_len.checked_add(8).filter(|end| *end <= size) else {
                // `last if $len + 8 > $size` leaves only the entry loop.
                break;
            };
            if ty != SOUNDSHOT || metadata.contains_key("MakerNotes:EmbeddedAudioFileName") {
                continue;
            }
            // HandleTag receives the raw bytes.  Pinned ExifTool's public
            // JSON rendering substitutes `?` for an invalid byte (while -b
            // retains it), so withholding both Samsung tags would be less
            // faithful than preserving its public representation here.
            let name = String::from_utf8_lossy(&data[8..name_end]).replace('\u{fffd}', "?");
            metadata.insert_with_group1(
                "MakerNotes:EmbeddedAudioFileName",
                TagValue::new_string(name.trim_end_matches('\0')),
                "Samsung",
            );
            metadata.insert_with_group1(
                "MakerNotes:EmbeddedAudioFile",
                TagValue::new_binary(data[name_end..].to_vec()),
                "Samsung",
            );
        }
        return Some(SamsungTrailer { metadata, data_pos });
    }
}
