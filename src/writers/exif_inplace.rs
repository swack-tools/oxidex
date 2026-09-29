//! In-place EXIF date/time patching
//!
//! EXIF stores date/time tags as fixed-length 20-byte ASCII values
//! ("YYYY:MM:DD HH:MM:SS\0"), so shifting a date never changes a value's
//! length. This module rewrites only those bytes, leaving every other byte
//! of the file untouched. This deliberately avoids the whole-map rewrite in
//! `write_metadata`, which reconstructs the EXIF segment from
//! display-converted values and cannot round-trip binary tags (e.g.
//! ComponentsConfiguration, GPSVersionID) losslessly.

use crate::core::FileReader;
use crate::core::date_shift::{
    ExifDateTag, ShiftSpec, apply_spec, format_exif_datetime, parse_absolute_datetime,
};
use crate::core::operations_helpers::{read_u16, read_u32};
use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::parse_segments;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::writers::atomic_writer::write_atomic;
use std::path::Path;

/// IFD0 tag pointing to the ExifIFD
const EXIF_IFD_POINTER: u16 = 0x8769;
/// TIFF ASCII type code
const ASCII_TYPE: u16 = 2;
/// Byte count of a standard EXIF date/time value (19 chars + NUL)
const DATETIME_LEN: u32 = 20;
/// EXIF identifier at the start of an EXIF APP1 segment
const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";

/// Location of a shiftable date/time value inside a TIFF structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocatedDateTag {
    /// Which date tag this is
    pub tag: ExifDateTag,
    /// Offset of the 20-byte ASCII value, relative to the TIFF header start
    pub value_offset: usize,
}

/// Which IFD is being scanned (determines which tag IDs are date/time tags).
#[derive(Clone, Copy, PartialEq)]
enum Ifd {
    Ifd0,
    ExifIfd,
}

/// Walks IFD0 and the ExifIFD of `tiff` and returns the location of every
/// standard-format date/time tag value.
///
/// Tags whose value is not type ASCII with count 20, or whose value offset
/// falls outside `tiff`, are skipped (never patched) rather than risking
/// corruption.
pub fn locate_exif_datetimes(tiff: &[u8]) -> Result<Vec<LocatedDateTag>> {
    Ok(scan_exif_datetimes(tiff)?.0)
}

/// Also retain date entries that the reader may expose but the fixed-width
/// patcher cannot edit. A missing physical location is not proof that a
/// requested date is absent.
fn scan_exif_datetimes(tiff: &[u8]) -> Result<(Vec<LocatedDateTag>, Vec<ExifDateTag>)> {
    if tiff.len() < 8 {
        return Err(ExifToolError::parse_error("EXIF TIFF structure too small"));
    }
    let byte_order = match &tiff[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => {
            return Err(ExifToolError::parse_error(
                "Invalid TIFF byte order marker in EXIF data",
            ));
        }
    };
    if read_u16(&tiff[2..4], byte_order) != 42 {
        return Err(ExifToolError::parse_error(
            "Invalid TIFF magic number in EXIF data",
        ));
    }
    let ifd0_offset = read_u32(&tiff[4..8], byte_order) as usize;

    let mut found = Vec::new();
    let mut unshiftable = Vec::new();
    let exif_ifd_offset = scan_ifd(
        tiff,
        ifd0_offset,
        byte_order,
        Ifd::Ifd0,
        &mut found,
        &mut unshiftable,
    )?;
    if let Some(offset) = exif_ifd_offset {
        scan_ifd(
            tiff,
            offset,
            byte_order,
            Ifd::ExifIfd,
            &mut found,
            &mut unshiftable,
        )?;
    }
    Ok((found, unshiftable))
}

/// The byte span (offset, length) of every other value in the reachable
/// EXIF directory graph. A date's bytes may also back a SubIFD value, even
/// when that SubIFD is not part of the ordinary date reader's walk. We must
/// prove the whole graph's storage before patching a date in place.
fn other_value_spans(tiff: &[u8], located: &[LocatedDateTag]) -> Result<Vec<(usize, usize)>> {
    const POINTERS: [u16; 3] = [EXIF_IFD_POINTER, 0x8825, 0xa005];
    const SUB_IFDS: u16 = 0x014a;
    fn incomplete() -> ExifToolError {
        ExifToolError::unsupported_format(
            "Cannot safely shift dates through an incomplete EXIF directory graph; nothing was written",
        )
    }
    fn enqueue(
        pending: &mut Vec<usize>,
        discovered: &mut std::collections::BTreeSet<usize>,
        at: usize,
    ) -> Result<()> {
        if at == 0 {
            return Err(incomplete());
        }
        if discovered.contains(&at) {
            return Ok(());
        }
        if discovered.len() >= 64 {
            return Err(ExifToolError::unsupported_format(
                "Cannot safely shift dates through more than 64 EXIF directories; nothing was written",
            ));
        }
        discovered.insert(at);
        pending.push(at);
        Ok(())
    }
    let byte_order = if tiff.starts_with(b"II") {
        ByteOrder::LittleEndian
    } else {
        ByteOrder::BigEndian
    };
    let ifd0 = read_u32(tiff.get(4..8).ok_or_else(incomplete)?, byte_order) as usize;
    let exif_ifd = scan_ifd(
        tiff,
        ifd0,
        byte_order,
        Ifd::Ifd0,
        &mut Vec::new(),
        &mut Vec::new(),
    )?;
    // Inline values, entry descriptors and pointers are storage too.
    let mut spans = vec![(0, 8)];
    let mut pending = Vec::new();
    let mut discovered = std::collections::BTreeSet::new();
    enqueue(&mut pending, &mut discovered, ifd0)?;
    while let Some(offset) = pending.pop() {
        let count_end = offset.checked_add(2).ok_or_else(incomplete)?;
        let count = read_u16(
            tiff.get(offset..count_end).ok_or_else(incomplete)?,
            byte_order,
        ) as usize;
        let table_end = count_end
            .checked_add(count.checked_mul(12).ok_or_else(incomplete)?)
            .ok_or_else(incomplete)?;
        let next_end = table_end.checked_add(4).ok_or_else(incomplete)?;
        tiff.get(offset..next_end).ok_or_else(incomplete)?;
        spans.push((offset, next_end - offset));
        let mut pair_values: Vec<(u16, Vec<usize>)> = Vec::new();
        for entry in tiff[count_end..table_end].chunks_exact(12) {
            let tag_id = read_u16(&entry[0..2], byte_order);
            let value_type = read_u16(&entry[2..4], byte_order);
            let value_count = read_u32(&entry[4..8], byte_order) as usize;
            let value = read_u32(&entry[8..12], byte_order) as usize;
            // Unknown TIFF widths cannot prove that the date's storage is
            // unaliased; the ordinary scanner's opaque-byte fallback is not
            // sufficient for an in-place safety decision.
            if !(1..=12).contains(&value_type) {
                return Err(incomplete());
            }
            let len = crate::writers::exif_surgical::type_size(value_type)
                .checked_mul(value_count)
                .ok_or_else(incomplete)?;
            let bytes = if len <= 4 {
                &entry[8..8 + len]
            } else {
                let end = value.checked_add(len).ok_or_else(incomplete)?;
                tiff.get(value..end).ok_or_else(incomplete)?
            };
            if POINTERS.contains(&tag_id) {
                // The ordinary reader follows one scalar offset. An array
                // or another type leaves reachable directories ambiguous.
                if value_count != 1 || !matches!(value_type, 1 | 3 | 4 | 9) {
                    return Err(incomplete());
                }
                enqueue(
                    &mut pending,
                    &mut discovered,
                    crate::writers::exif_surgical::inline_unsigned(
                        value_type,
                        1,
                        &entry[8..12],
                        byte_order,
                    ),
                )?;
                continue;
            }
            if crate::writers::exif_surgical::OFFSET_LENGTH_PAIRS
                .iter()
                .any(|(offset_tag, length_tag)| tag_id == *offset_tag || tag_id == *length_tag)
            {
                let width = match value_type {
                    3 => 2,
                    4 => 4,
                    _ => return Err(incomplete()),
                };
                if value_count == 0 || bytes.len() != width * value_count {
                    return Err(incomplete());
                }
                if pair_values.iter().any(|(id, _)| *id == tag_id) {
                    return Err(incomplete());
                }
                let values = bytes
                    .chunks_exact(width)
                    .map(|chunk| {
                        if width == 2 {
                            read_u16(chunk, byte_order) as usize
                        } else {
                            read_u32(chunk, byte_order) as usize
                        }
                    })
                    .collect();
                pair_values.push((tag_id, values));
            }
            if tag_id == SUB_IFDS {
                let width = match value_type {
                    3 => 2,
                    4 => 4,
                    _ => return Err(incomplete()),
                };
                if value_count == 0 || value_count > 64 || bytes.len() != width * value_count {
                    return Err(incomplete());
                }
                if len > 4 {
                    spans.push((value, len));
                }
                for child in bytes.chunks_exact(width) {
                    let at = if width == 2 {
                        read_u16(child, byte_order) as usize
                    } else {
                        read_u32(child, byte_order) as usize
                    };
                    enqueue(&mut pending, &mut discovered, at)?;
                }
                continue;
            }
            if (crate::writers::exif_surgical::UNMODELLED_POINTER_TAGS.contains(&tag_id)
                || crate::writers::exif_surgical::NAMED_POINTER_TAGS.contains(&tag_id))
                && !crate::writers::exif_surgical::OFFSET_LENGTH_PAIRS
                    .iter()
                    .any(|(offset_tag, _)| *offset_tag == tag_id)
            {
                // ExifTool's source declares more offset-bearing tags than
                // pairs whose byte extent can be proven here.
                return Err(incomplete());
            }
            let date_tag = match tag_id {
                0x0132 => Some(ExifDateTag::ModifyDate),
                0x9003 => Some(ExifDateTag::DateTimeOriginal),
                0x9004 => Some(ExifDateTag::CreateDate),
                _ => None,
            };
            let is_located = (offset == ifd0 || Some(offset) == exif_ifd)
                && value_type == ASCII_TYPE
                && value_count == DATETIME_LEN as usize
                && located
                    .iter()
                    .any(|l| Some(l.tag) == date_tag && l.value_offset == value);
            if len > 4 && !is_located {
                spans.push((value, len));
            }
        }
        for &(offset_tag, length_tag) in crate::writers::exif_surgical::OFFSET_LENGTH_PAIRS {
            let Some((_, offsets)) = pair_values.iter().find(|(id, _)| *id == offset_tag) else {
                if pair_values.iter().any(|(id, _)| *id == length_tag) {
                    return Err(incomplete());
                }
                continue;
            };
            let Some((_, lengths)) = pair_values.iter().find(|(id, _)| *id == length_tag) else {
                return Err(incomplete());
            };
            if offsets.len() != lengths.len() {
                return Err(incomplete());
            }
            for (&at, &len) in offsets.iter().zip(lengths) {
                let end = at.checked_add(len).ok_or_else(incomplete)?;
                tiff.get(at..end).ok_or_else(incomplete)?;
                spans.push((at, len));
            }
        }
        let next = read_u32(&tiff[table_end..next_end], byte_order) as usize;
        if next != 0 {
            enqueue(&mut pending, &mut discovered, next)?;
        }
    }
    Ok(spans)
}

/// Scans one IFD, appending located date/time values to `found`.
/// Returns the ExifIFD offset when this IFD contains an ExifIFD pointer.
fn scan_ifd(
    tiff: &[u8],
    offset: usize,
    byte_order: ByteOrder,
    which: Ifd,
    found: &mut Vec<LocatedDateTag>,
    unshiftable: &mut Vec<ExifDateTag>,
) -> Result<Option<usize>> {
    let entries_start = match offset.checked_add(2) {
        Some(end) if end <= tiff.len() => end,
        // A corrupt or truncated IFD offset (from untrusted file bytes) stops
        // this scan gracefully; header-level validation already ran upstream
        _ => return Ok(None),
    };
    let entry_count = read_u16(&tiff[offset..entries_start], byte_order) as usize;
    let mut exif_ifd_offset = None;

    for i in 0..entry_count {
        let entry_start = entries_start + i * 12;
        let entry_end = entry_start + 12;
        if entry_end > tiff.len() {
            // Truncated IFD: stop scanning rather than failing on real-world files
            break;
        }
        let entry = &tiff[entry_start..entry_end];
        let tag_id = read_u16(&entry[0..2], byte_order);
        let value_type = read_u16(&entry[2..4], byte_order);
        let value_count = read_u32(&entry[4..8], byte_order);
        let value_or_offset = read_u32(&entry[8..12], byte_order) as usize;

        if which == Ifd::Ifd0 && tag_id == EXIF_IFD_POINTER {
            // Decoded by TIFF type: a SHORT pointer is the field's first two
            // bytes (`exif_surgical::inline_unsigned`).
            exif_ifd_offset = Some(crate::writers::exif_surgical::inline_unsigned(
                value_type,
                value_count,
                &entry[8..12],
                byte_order,
            ));
            continue;
        }
        // A date may sit in either directory (a copy in the other one is a
        // copy pinned ExifTool 13.59 shifts too, WriteExif.pl 13.59:1259).
        let date_tag = match tag_id {
            0x0132 => ExifDateTag::ModifyDate,
            0x9003 => ExifDateTag::DateTimeOriginal,
            0x9004 => ExifDateTag::CreateDate,
            _ => continue,
        };
        // A count-20 ASCII value is larger than 4 bytes, so it is always
        // stored at an offset, never inline in the entry.
        if value_type == ASCII_TYPE
            && value_count == DATETIME_LEN
            && value_or_offset + DATETIME_LEN as usize <= tiff.len()
        {
            found.push(LocatedDateTag {
                tag: date_tag,
                value_offset: value_or_offset,
            });
        } else {
            unshiftable.push(date_tag);
        }
    }
    Ok(exif_ifd_offset)
}

/// A FileReader over an in-memory byte slice.
struct SliceReader<'a>(&'a [u8]);

impl FileReader for SliceReader<'_> {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.checked_add(length).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "read overflow")
        })?;
        if end > self.0.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read beyond end of buffer",
            ));
        }
        Ok(&self.0[start..end])
    }

    fn size(&self) -> u64 {
        self.0.len() as u64
    }
}

/// Shifts EXIF date/time tags of a JPEG in place.
///
/// Patches only the 19 ASCII characters of each target tag's value; every
/// other byte of the file is preserved verbatim. Returns the number of tags
/// modified. Targets not present in the file are skipped, not errors; the
/// file is not rewritten at all when nothing matched.
pub fn shift_jpeg_exif_dates(
    path: &Path,
    targets: &[ExifDateTag],
    spec: &ShiftSpec,
) -> Result<usize> {
    let mut file_bytes = std::fs::read(path)?;
    // Check the bytes this call will modify, not a second path lookup that
    // can observe a different inode. Prove an absent target across every
    // APP1 before allowing the unchanged case.
    crate::writers::jpeg_multi_exif::refuse_multi_exif_app1_date_shift(&file_bytes, targets)?;

    // Find the EXIF APP1 segment. The TIFF structure starts after
    // marker (2) + length field (2) + "Exif\0\0" (6).
    let (tiff_start, tiff_len) = {
        let reader = SliceReader(&file_bytes);
        let segments = parse_segments(&reader)?;
        // No EXIF, no date to shift: nothing is written (13.59: the file is
        // `unchanged`).
        let Some(exif_seg) = segments
            .iter()
            .find(|s| s.is_app1() && s.data.starts_with(EXIF_IDENTIFIER))
        else {
            return Ok(0);
        };
        (
            exif_seg.offset as usize + 4 + EXIF_IDENTIFIER.len(),
            exif_seg.data.len() - EXIF_IDENTIFIER.len(),
        )
    };

    let modified = shift_block_dates(&mut file_bytes, tiff_start, tiff_len, targets, spec)?;

    if modified > 0 {
        write_atomic(path, &file_bytes)?;
    }
    Ok(modified)
}

/// Shifts, in `file_bytes`, every IFD0/ExifIFD copy of each of `targets`
/// in the TIFF block at `tiff_start..tiff_start + tiff_len`; returns how
/// many values changed.
fn shift_block_dates(
    file_bytes: &mut [u8],
    tiff_start: usize,
    tiff_len: usize,
    targets: &[ExifDateTag],
    spec: &ShiftSpec,
) -> Result<usize> {
    let (located, unshiftable) =
        scan_exif_datetimes(&file_bytes[tiff_start..tiff_start + tiff_len])?;
    if let Some(tag) = unshiftable.into_iter().find(|tag| targets.contains(tag)) {
        return Err(ExifToolError::unsupported_format(format!(
            "Cannot shift {}: its EXIF date value is not a supported 20-byte ASCII field; nothing was written",
            tag.key()
        )));
    }
    // One value may back several entries (an IFD0 and an ExifIFD ModifyDate
    // pointing at the same 20 bytes, or ModifyDate and DateTimeOriginal).
    // Pinned ExifTool 13.59 shifts each entry once from its own old value
    // and writes it back on its own, so a shared value is patched once, and
    // only when every entry it backs is shifted: patching it for each entry
    // shifted it twice, and patching it for one entry shifted the others
    // too (review of #964). A value shared with an entry the request does
    // not shift is refused, naming both, before anything is patched.
    let other_spans = other_value_spans(&file_bytes[tiff_start..tiff_start + tiff_len], &located)?;
    let mut offsets: Vec<usize> = located.iter().map(|l| l.value_offset).collect();
    offsets.sort_unstable();
    offsets.dedup();
    let mut shared = Vec::new();
    for offset in offsets {
        let backed: Vec<ExifDateTag> = located
            .iter()
            .filter(|l| l.value_offset == offset)
            .map(|l| l.tag)
            .collect();
        let shifted: Vec<ExifDateTag> = backed
            .iter()
            .copied()
            .filter(|tag| targets.contains(tag))
            .collect();
        if shifted.is_empty() {
            continue;
        }
        if let Some(other) = located.iter().find(|other| {
            other.value_offset != offset
                && other.value_offset < offset + DATETIME_LEN as usize
                && offset < other.value_offset + DATETIME_LEN as usize
        }) {
            return Err(ExifToolError::unsupported_format(format!(
                "Cannot shift {}: its storage partially overlaps {}; nothing was written",
                shifted[0].key(),
                other.tag.key()
            )));
        }
        if let Some((at, len)) = other_spans.iter().find(|(at, len)| {
            *at < offset + DATETIME_LEN as usize && offset < at.saturating_add(*len)
        }) {
            return Err(ExifToolError::unsupported_format(format!(
                "Cannot shift {}: its value's bytes also back another EXIF value (at \
                 0x{at:x}, {len} bytes), which pinned ExifTool 13.59 writes apart from the \
                 date; nothing was written",
                shifted[0].key()
            )));
        }
        if let Some(kept) = backed.iter().find(|tag| !targets.contains(tag)) {
            return Err(ExifToolError::unsupported_format(format!(
                "Cannot shift {}: its value is stored once for {} as well, which this \
                 request leaves alone (pinned ExifTool 13.59 writes the two apart); \
                 nothing was written",
                shifted[0].key(),
                kept.key()
            )));
        }
        shared.push((offset, shifted));
    }
    let mut modified = 0;
    for (offset, shifted) in &shared {
        let target = shifted[0];
        match patch_datetime_value(file_bytes, tiff_start + offset, target, spec) {
            Ok(()) => modified += shifted.len(),
            // Multi-target shifts (AllDates) skip values that cannot be
            // shifted — matching ExifTool, which warns and continues when
            // e.g. an unset camera clock wrote "0000:00:00 00:00:00"
            Err(e)
                if targets.len() > 1 || located.iter().filter(|l| l.tag == target).count() > 1 =>
            {
                eprintln!("Warning: skipping {}: {}", target.key(), e);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(modified)
}

/// [`shift_jpeg_exif_dates`] for a walkable TIFF (its own TIFF structure)
/// or a PNG (its `eXIf` chunk, whose CRC is recomputed): every IFD0/ExifIFD
/// copy of each target shifts in place. `None` when the file is neither, or
/// holds no EXIF to shift in.
pub fn shift_tiff_png_exif_dates(
    path: &Path,
    targets: &[ExifDateTag],
    spec: &ShiftSpec,
) -> Result<Option<usize>> {
    const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let mut file_bytes = std::fs::read(path)?;
    // Classic TIFF only (magic 42, not RW2's 85 or BigTIFF).
    let classic_tiff = matches!(file_bytes.get(0..4), Some(b"II\x2a\x00" | b"MM\x00\x2a"));
    let modified = if classic_tiff {
        // This exported helper can be called without the shared transaction
        // planner. A date shift is an EXIF set to ExifTool, which also writes
        // a TIFF's MIE trailer. Reuse the planner's MIE census before any
        // in-place edit of the outer TIFF.
        let source_reader = crate::io::MMapReader::new(path)?;
        let mie = crate::core::operations::mie_census_with_reader(&source_reader)?;
        for target in targets {
            crate::writers::write_request::ensure_no_mie_copy(
                target.key(),
                target.key(),
                &crate::core::MetadataMap::new(),
                false,
                &mie,
            )?;
        }
        let len = file_bytes.len();
        shift_block_dates(&mut file_bytes, 0, len, targets, spec)?
    } else if file_bytes.starts_with(PNG_SIGNATURE) {
        // This public helper can be called without date_shift's surrounding
        // validation. Check the original container before a missing-eXIf
        // no-op or an in-place date edit, using the PNG writer's same policy.
        let reader = SliceReader(&file_bytes);
        crate::writers::png_writer::refuse_bad_chunk_crcs(&reader)?;
        // The shared PNG writer rejects a second eXIf carrier and raw EXIF
        // profile text chunks before an edit. Keep that same carrier boundary
        // on this direct date path, which otherwise finds only the first
        // eXIf chunk and could report success after changing one copy.
        match crate::writers::png_writer::png_exif_payloads(&reader)? {
            Some(payloads) if payloads.len() > 1 => {
                return Err(ExifToolError::unsupported_format(
                    "Cannot edit EXIF in a PNG with more than one eXIf chunk \
                     (ExifTool also refuses: IFD0 pointer references previous IFD0 directory)",
                ));
            }
            None => {
                return Err(ExifToolError::unsupported_format(
                    "Cannot edit EXIF stored in a PNG raw-profile text chunk; nothing was written",
                ));
            }
            Some(_) => {}
        }
        let mut at = PNG_SIGNATURE.len();
        let mut chunk = None;
        while let Some(header) = file_bytes.get(at..at + 8) {
            let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
            if at + 12 + len > file_bytes.len() {
                break;
            }
            if &header[4..8] == b"eXIf" {
                chunk = Some((at, len));
                break;
            }
            at += 12 + len;
        }
        let Some((at, len)) = chunk else {
            return Ok(None);
        };
        let data = at + 8;
        let skip = if file_bytes[data..data + len].starts_with(EXIF_IDENTIFIER) {
            EXIF_IDENTIFIER.len()
        } else {
            0
        };
        let modified = shift_block_dates(&mut file_bytes, data + skip, len - skip, targets, spec)?;
        let crc =
            crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(&file_bytes[at + 4..data + len]);
        file_bytes[data + len..data + len + 4].copy_from_slice(&crc.to_be_bytes());
        modified
    } else {
        return Ok(None);
    };
    if modified > 0 {
        write_atomic(path, &file_bytes)?;
    }
    Ok(Some(modified))
}

/// Shifts the single 20-byte ASCII datetime value at `value_start`, patching
/// the buffer in place. Fails without modifying anything when the current
/// value does not parse or the shifted value cannot be represented.
fn patch_datetime_value(
    file_bytes: &mut [u8],
    value_start: usize,
    target: ExifDateTag,
    spec: &ShiftSpec,
) -> Result<()> {
    let current =
        std::str::from_utf8(&file_bytes[value_start..value_start + 19]).map_err(|_| {
            ExifToolError::parse_error(format!("Tag '{}' has a non-ASCII date value", target.key()))
        })?;
    let dt = parse_absolute_datetime(current)?;
    let new_dt = apply_spec(dt, spec)?;
    let formatted = format_exif_datetime(&new_dt);
    if formatted.len() != 19 {
        return Err(ExifToolError::parse_error(format!(
            "Shifted date '{}' for tag '{}' is outside the representable EXIF range (year must be 4 digits)",
            formatted,
            target.key()
        )));
    }
    file_bytes[value_start..value_start + 19].copy_from_slice(formatted.as_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_bytes(v: u16, bo: ByteOrder) -> [u8; 2] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }

    fn u32_bytes(v: u32, bo: ByteOrder) -> [u8; 4] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }

    /// Builds a minimal TIFF structure:
    /// - IFD0 at offset 8 with ModifyDate (value at 38) and an ExifIFD pointer (58)
    /// - ExifIFD at 58 with DateTimeOriginal (value at 88) and CreateDate (value at 108)
    fn build_test_tiff(bo: ByteOrder) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(match bo {
            ByteOrder::LittleEndian => b"II",
            ByteOrder::BigEndian => b"MM",
        });
        t.extend_from_slice(&u16_bytes(42, bo));
        t.extend_from_slice(&u32_bytes(8, bo));
        // IFD0 at 8: 2 entries
        t.extend_from_slice(&u16_bytes(2, bo));
        t.extend_from_slice(&u16_bytes(0x0132, bo)); // ModifyDate
        t.extend_from_slice(&u16_bytes(2, bo)); // ASCII
        t.extend_from_slice(&u32_bytes(20, bo));
        t.extend_from_slice(&u32_bytes(38, bo));
        t.extend_from_slice(&u16_bytes(0x8769, bo)); // ExifIFD pointer
        t.extend_from_slice(&u16_bytes(4, bo)); // LONG
        t.extend_from_slice(&u32_bytes(1, bo));
        t.extend_from_slice(&u32_bytes(58, bo));
        t.extend_from_slice(&u32_bytes(0, bo)); // next IFD
        t.extend_from_slice(b"2025:01:15 10:30:00\0"); // 38..58
        // ExifIFD at 58: 2 entries
        t.extend_from_slice(&u16_bytes(2, bo));
        t.extend_from_slice(&u16_bytes(0x9003, bo)); // DateTimeOriginal
        t.extend_from_slice(&u16_bytes(2, bo));
        t.extend_from_slice(&u32_bytes(20, bo));
        t.extend_from_slice(&u32_bytes(88, bo));
        t.extend_from_slice(&u16_bytes(0x9004, bo)); // CreateDate
        t.extend_from_slice(&u16_bytes(2, bo));
        t.extend_from_slice(&u32_bytes(20, bo));
        t.extend_from_slice(&u32_bytes(108, bo));
        t.extend_from_slice(&u32_bytes(0, bo)); // next IFD
        t.extend_from_slice(b"2025:06:10 12:00:00\0"); // 88..108
        t.extend_from_slice(b"2025:06:10 12:00:05\0"); // 108..128
        t
    }

    #[test]
    fn a_count_twenty_non_date_value_sharing_date_storage_is_refused() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let mut tiff = build_test_tiff(bo);
            // Software has the same type, count and offset as DateTimeOriginal.
            tiff[10..12].copy_from_slice(&u16_bytes(0x0131, bo));
            tiff[18..22].copy_from_slice(&u32_bytes(88, bo));
            let before = tiff.clone();
            let len = tiff.len();
            let spec = ShiftSpec::Absolute(parse_absolute_datetime("2020:01:02 03:04:05").unwrap());
            let result =
                shift_block_dates(&mut tiff, 0, len, &[ExifDateTag::DateTimeOriginal], &spec);
            assert!(result.is_err(), "shared Software storage was patched");
            assert_eq!(tiff, before);
        }
    }

    #[test]
    fn partially_overlapping_dates_are_refused_before_patching() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let mut tiff = build_test_tiff(bo);
            tiff[18..22].copy_from_slice(&u32_bytes(89, bo));
            let before = tiff.clone();
            let len = tiff.len();
            let spec = ShiftSpec::Absolute(parse_absolute_datetime("2020:01:02 03:04:05").unwrap());
            let result = shift_block_dates(
                &mut tiff,
                0,
                len,
                &[ExifDateTag::ModifyDate, ExifDateTag::DateTimeOriginal],
                &spec,
            );
            assert!(result.is_err(), "partially overlapping dates were patched");
            assert_eq!(tiff, before);
        }
    }

    #[test]
    fn test_locate_all_three_tags_little_endian() {
        let tiff = build_test_tiff(ByteOrder::LittleEndian);
        let located = locate_exif_datetimes(&tiff).unwrap();
        assert_eq!(
            located,
            vec![
                LocatedDateTag {
                    tag: ExifDateTag::ModifyDate,
                    value_offset: 38
                },
                LocatedDateTag {
                    tag: ExifDateTag::DateTimeOriginal,
                    value_offset: 88
                },
                LocatedDateTag {
                    tag: ExifDateTag::CreateDate,
                    value_offset: 108
                },
            ]
        );
    }

    #[test]
    fn test_locate_all_three_tags_big_endian() {
        let tiff = build_test_tiff(ByteOrder::BigEndian);
        let located = locate_exif_datetimes(&tiff).unwrap();
        assert_eq!(located.len(), 3);
        assert_eq!(located[1].tag, ExifDateTag::DateTimeOriginal);
        assert_eq!(located[1].value_offset, 88);
    }

    #[test]
    fn test_nonstandard_count_is_skipped() {
        let mut tiff = build_test_tiff(ByteOrder::LittleEndian);
        // ModifyDate entry starts at 10; its count field is at 14..18
        tiff[14..18].copy_from_slice(&19u32.to_le_bytes());
        let located = locate_exif_datetimes(&tiff).unwrap();
        // ModifyDate skipped, the two ExifIFD tags still found
        assert_eq!(located.len(), 2);
        assert!(located.iter().all(|l| l.tag != ExifDateTag::ModifyDate));
    }

    #[test]
    fn test_value_offset_out_of_bounds_is_skipped() {
        let mut tiff = build_test_tiff(ByteOrder::LittleEndian);
        // ModifyDate entry value-offset field is at 18..22; point past the end
        tiff[18..22].copy_from_slice(&5000u32.to_le_bytes());
        let located = locate_exif_datetimes(&tiff).unwrap();
        assert_eq!(located.len(), 2);
        assert!(located.iter().all(|l| l.tag != ExifDateTag::ModifyDate));
    }

    #[test]
    fn test_invalid_tiff_errors() {
        assert!(locate_exif_datetimes(&[]).is_err());
        assert!(locate_exif_datetimes(b"XX\x2a\x00\x08\x00\x00\x00").is_err());
    }

    #[test]
    fn test_corrupt_ifd0_offset_returns_empty_not_err() {
        let mut tiff = build_test_tiff(ByteOrder::LittleEndian);
        // Point the header's IFD0 offset far beyond the buffer
        tiff[4..8].copy_from_slice(&50_000u32.to_le_bytes());
        let located = locate_exif_datetimes(&tiff).unwrap();
        assert!(located.is_empty());
    }

    #[test]
    fn test_corrupt_exif_ifd_pointer_keeps_ifd0_results() {
        let mut tiff = build_test_tiff(ByteOrder::LittleEndian);
        // ExifIFD pointer entry starts at 22; its value field is at 30..34
        tiff[30..34].copy_from_slice(&50_000u32.to_le_bytes());
        let located = locate_exif_datetimes(&tiff).unwrap();
        // ModifyDate from IFD0 must survive; the two ExifIFD tags are lost
        assert_eq!(located.len(), 1);
        assert_eq!(located[0].tag, ExifDateTag::ModifyDate);
    }
    #[test]
    fn subifd_queue_deduplicates_and_caps_unique_directories() {
        fn graph(children: usize, repeated: bool) -> Vec<u8> {
            let rows = if repeated { 200 } else { children };
            let child_at = 8 + 2 + rows * 12 + 4;
            let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
            tiff.extend_from_slice(&(rows as u16).to_le_bytes());
            for i in 0..rows {
                let at = child_at + if repeated { 0 } else { i * 6 };
                tiff.extend_from_slice(&0x014a_u16.to_le_bytes());
                tiff.extend_from_slice(&4_u16.to_le_bytes());
                tiff.extend_from_slice(&1_u32.to_le_bytes());
                tiff.extend_from_slice(&(at as u32).to_le_bytes());
            }
            tiff.extend_from_slice(&0_u32.to_le_bytes());
            for _ in 0..children {
                tiff.extend_from_slice(&0_u16.to_le_bytes());
                tiff.extend_from_slice(&0_u32.to_le_bytes());
            }
            tiff
        }
        // Hundreds of duplicate pointers consume one child slot.
        let duplicates = graph(1, true);
        assert!(other_value_spans(&duplicates, &[]).is_ok());
        // IFD0 plus 64 distinct children exceeds the established limit.
        let distinct = graph(64, false);
        let error = other_value_spans(&distinct, &[]).unwrap_err();
        assert!(
            error.to_string().contains("more than 64 EXIF directories"),
            "{error}"
        );
    }
}
