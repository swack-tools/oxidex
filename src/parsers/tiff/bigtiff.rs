//! BigTIFF (.btf) metadata parser.
//!
//! ExifTool 13.59 routes a TIFF header whose version word is `0x2b` away from
//! the ordinary TIFF path entirely (ExifTool.pm:8661-8665) and into
//! `BigTIFF::ProcessBTF` (BigTIFF.pm:234-264), which walks the file with
//! `ProcessBigIFD` (BigTIFF.pm:26-228) against the ordinary
//! `Exif::Main` tag table. BigTIFF differs from TIFF only in its widths: the
//! IFD entry count is a `u64`, each entry is 20 bytes rather than 12 (its
//! `count` and value/offset fields are `u64`), the inline-value threshold is
//! 8 bytes rather than 4, and the next-IFD pointer is a `u64`.
//!
//! Because the tag table is the same one, this reads the entries itself and
//! then hands them to [`process_tiff_ifd_tags`] -- the same conversion the
//! ordinary TIFF chain uses -- rather than duplicating tag naming or any
//! `PrintConv`.
//!
//! # Deliberately absent
//!
//! * **`File:ExifByteOrder`.** ExifTool sets it at ExifTool.pm:8702, *after*
//!   the `return 1` at :8667 that a successful `ProcessBTF` takes, so no
//!   BigTIFF gets one. The pinned oracle confirms it: `-ExifByteOrder` on
//!   `BigTIFF.btf` prints nothing, while `ExifTool.tif` prints
//!   `Big-endian (Motorola, MM)`.
//! * **`File:PageCount`.** For BigTIFF this comes from ExifTool.pm:8668's
//!   `$$self{MultiPage}`, which Exif.pm:455-473 sets from `SubfileType` /
//!   `OldSubfileType` *values* -- not from the number of linked IFDs, which
//!   is the rule the ordinary TIFF chain in `tiff_helpers` follows. Emitting
//!   the ordinary rule's answer here would be a wrong value, not a missing
//!   tag, so it is left out.
//! * **`SubIFD` recursion** (BigTIFF.pm:171-198), which covers the ExifIFD
//!   and GPS pointers as well as `SubIFD` proper. ExifTool descends into
//!   those as further BigTIFF directories; doing the same correctly also
//!   means carrying MakerNotes, Interop and the offset bases that go with
//!   them, and the corpus holds exactly one BigTIFF -- with no sub-IFD of
//!   any kind -- so none of it could be checked against the oracle even
//!   once. Following the MRC precedent in `AGENTS.md`, the pointers are read
//!   and not followed: the tags below them are MISSING, never wrong.
//! * **BigTIFF's own format codes 16/17/18** (`int64u`, `int64s`, `ifd64`,
//!   Exif.pm:85-91). `ExifType::from_u16` stops at 12, and widening it is a
//!   change to every TIFF-based format in the corpus for the sake of a file
//!   type none of them are. An entry declaring one is skipped, so again:
//!   missing, not wrong.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/BigTIFF.pm`

use crate::core::tiff_helpers::{get_ifd_name, process_tiff_ifd_tags};
use crate::core::{FileReader, MetadataMap};
use crate::error::{ExifToolError, Result};
use crate::parsers::common::exif_types::ExifType;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::borrow::Cow;

/// BigTIFF.pm:241, `$buff =~ /^(MM\0\x2b\0\x08\0\0|II\x2b\0\x08\0\0\0)/`.
const HEADER_BE: &[u8] = b"MM\0\x2b\0\x08\0\0";
const HEADER_LE: &[u8] = b"II\x2b\0\x08\0\0\0";
/// BigTIFF.pm:240, `$raf->Read($buff, 16) == 16`.
const HEADER_LEN: usize = 16;
/// BigTIFF.pm:82, `my $entry = 20 * $index`.
const ENTRY_LEN: usize = 20;
/// BigTIFF.pm:100, `if ($size > 8)`.
const INLINE_MAX: u64 = 8;
/// BigTIFF.pm:20, `my $maxOffset = 0x7fffffff`.
const MAX_OFFSET: u64 = 0x7fff_ffff;
/// BigTIFF.pm:213-217 chains IFD0 -> IFD1 -> ...; this is the same
/// runaway guard the ordinary TIFF chain in `tiff_helpers` uses.
const MAX_IFDS: usize = 10;

/// Whether `header` opens a BigTIFF, i.e. BigTIFF.pm:241's test.
#[must_use]
pub fn is_bigtiff(header: &[u8]) -> bool {
    header.starts_with(HEADER_BE) || header.starts_with(HEADER_LE)
}

fn read_u64(bytes: &[u8], byte_order: ByteOrder) -> Option<u64> {
    let word: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    Some(match byte_order {
        ByteOrder::LittleEndian => u64::from_le_bytes(word),
        ByteOrder::BigEndian => u64::from_be_bytes(word),
    })
}

fn read_u16(bytes: &[u8], byte_order: ByteOrder) -> Option<u16> {
    let word: [u8; 2] = bytes.get(..2)?.try_into().ok()?;
    Some(match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes(word),
        ByteOrder::BigEndian => u16::from_be_bytes(word),
    })
}

/// `ProcessBTF` (BigTIFF.pm:234-264).
pub fn parse_bigtiff_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let header = reader.read(0, HEADER_LEN)?;
    if !is_bigtiff(header) {
        return Err(ExifToolError::parse_error("Invalid BigTIFF header"));
    }
    let byte_order = if header.starts_with(HEADER_LE) {
        ByteOrder::LittleEndian
    } else {
        ByteOrder::BigEndian
    };
    // BigTIFF.pm:248, `Get64u(\$buff, 8)`.
    let mut ifd_offset = read_u64(&header[8..], byte_order)
        .ok_or_else(|| ExifToolError::parse_error("short header"))?;

    let mut metadata = MetadataMap::new();
    let mut index = 0usize;
    while ifd_offset != 0 && index < MAX_IFDS {
        // BigTIFF.pm:43-46: offsets past 2 GB need LargeFileSupport, which is
        // off by default, and ExifTool stops rather than reading them.
        if ifd_offset > MAX_OFFSET {
            break;
        }
        let Some((entries, next)) = read_big_ifd(reader, ifd_offset, byte_order) else {
            break;
        };
        // The exif/gps/makernote pointers this returns are deliberately not
        // followed -- see the module docs.
        let _ = process_tiff_ifd_tags(&entries, get_ifd_name(index), byte_order, &mut metadata);
        ifd_offset = next;
        index += 1;
    }
    Ok(metadata)
}

/// One BigTIFF directory: its entries in the shape the ordinary TIFF
/// conversion takes, plus the next-IFD pointer (BigTIFF.pm:52-217).
fn read_big_ifd(
    reader: &dyn FileReader,
    offset: u64,
    byte_order: ByteOrder,
) -> Option<(Vec<(u16, u16, u32, Cow<'static, [u8]>)>, u64)> {
    // BigTIFF.pm:52-56, an 8-byte entry count.
    let count_bytes = reader.read(offset, 8).ok()?;
    let num_entries = read_u64(count_bytes, byte_order)?;
    // BigTIFF.pm:58-62, `$bsize > $maxOffset` is refused outright.
    let byte_size = num_entries.checked_mul(ENTRY_LEN as u64)?;
    if byte_size > MAX_OFFSET {
        return None;
    }
    let dir_start = offset + 8;
    let dir = reader.read(dir_start, byte_size as usize).ok()?;

    // BigTIFF.pm:69: a missing next-IFD pointer is not fatal, it just ends
    // the chain.
    let next = reader
        .read(dir_start + byte_size, 8)
        .ok()
        .and_then(|bytes| read_u64(bytes, byte_order))
        .unwrap_or(0);

    let mut entries = Vec::with_capacity(num_entries as usize);
    for i in 0..num_entries as usize {
        let entry = &dir[i * ENTRY_LEN..(i + 1) * ENTRY_LEN];
        let tag_id = read_u16(entry, byte_order)?;
        let format = read_u16(&entry[2..], byte_order)?;
        let count = read_u64(&entry[4..], byte_order)?;

        // BigTIFF.pm:86-93: an unknown format code aborts the whole
        // directory ("assume corrupted IFD"). Format codes 16/17/18 are
        // known to ExifTool but not to `ExifType`; those are skipped rather
        // than aborting, because the directory is not corrupt -- see the
        // module docs.
        let Some(exif_type) = ExifType::from_u16(format) else {
            if format == 0 || format > 18 {
                return Some((entries, next));
            }
            continue;
        };
        let size = count.checked_mul(exif_type.size_in_bytes() as u64)?;

        // BigTIFF.pm:100-118: over 8 bytes the field is an offset, otherwise
        // the value sits in the field itself, left-justified.
        let value: Cow<'static, [u8]> = if size > INLINE_MAX {
            if size > MAX_OFFSET {
                // BigTIFF.pm:101-104, a huge entry is skipped, not fatal.
                continue;
            }
            let value_offset = read_u64(&entry[12..], byte_order)?;
            if value_offset > MAX_OFFSET {
                continue;
            }
            match reader.read(value_offset, size as usize) {
                Ok(bytes) => Cow::Owned(bytes.to_vec()),
                // BigTIFF.pm:110-113, `$et->Warn("Error reading ..."), next`.
                Err(_) => continue,
            }
        } else {
            Cow::Owned(entry[12..12 + size as usize].to_vec())
        };

        let Ok(count) = u32::try_from(count) else {
            continue;
        };
        entries.push((tag_id, format, count, value));
    }
    Some((entries, next))
}
