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
    let exif_ifd_offset = scan_ifd(tiff, ifd0_offset, byte_order, Ifd::Ifd0, &mut found)?;
    if let Some(offset) = exif_ifd_offset {
        scan_ifd(tiff, offset, byte_order, Ifd::ExifIfd, &mut found)?;
    }
    Ok(found)
}

/// Scans one IFD, appending located date/time values to `found`.
/// Returns the ExifIFD offset when this IFD contains an ExifIFD pointer.
fn scan_ifd(
    tiff: &[u8],
    offset: usize,
    byte_order: ByteOrder,
    which: Ifd,
    found: &mut Vec<LocatedDateTag>,
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
            exif_ifd_offset = Some(value_or_offset);
            continue;
        }
        let date_tag = match (which, tag_id) {
            (Ifd::Ifd0, 0x0132) => ExifDateTag::ModifyDate,
            (Ifd::ExifIfd, 0x9003) => ExifDateTag::DateTimeOriginal,
            (Ifd::ExifIfd, 0x9004) => ExifDateTag::CreateDate,
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

    // Find the EXIF APP1 segment. The TIFF structure starts after
    // marker (2) + length field (2) + "Exif\0\0" (6).
    let (tiff_start, tiff_len) = {
        let reader = SliceReader(&file_bytes);
        let segments = parse_segments(&reader)?;
        let exif_seg = segments
            .iter()
            .find(|s| s.is_app1() && s.data.starts_with(EXIF_IDENTIFIER))
            .ok_or_else(|| ExifToolError::parse_error("No EXIF data found in JPEG"))?;
        (
            exif_seg.offset as usize + 4 + EXIF_IDENTIFIER.len(),
            exif_seg.data.len() - EXIF_IDENTIFIER.len(),
        )
    };

    let located = locate_exif_datetimes(&file_bytes[tiff_start..tiff_start + tiff_len])?;

    let mut modified = 0;
    for target in targets {
        let Some(location) = located.iter().find(|l| l.tag == *target) else {
            continue;
        };
        let value_start = tiff_start + location.value_offset;
        match patch_datetime_value(&mut file_bytes, value_start, *target, spec) {
            Ok(()) => modified += 1,
            // Multi-target shifts (AllDates) skip values that cannot be
            // shifted — matching ExifTool, which warns and continues when
            // e.g. an unset camera clock wrote "0000:00:00 00:00:00"
            Err(e) if targets.len() > 1 => {
                eprintln!("Warning: skipping {}: {}", target.key(), e);
            }
            Err(e) => return Err(e),
        }
    }

    if modified > 0 {
        write_atomic(path, &file_bytes)?;
    }
    Ok(modified)
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
}
