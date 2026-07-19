//! In-place EXIF date/time patching
//!
//! EXIF stores date/time tags as fixed-length 20-byte ASCII values
//! ("YYYY:MM:DD HH:MM:SS\0"), so shifting a date never changes a value's
//! length. This module rewrites only those bytes, leaving every other byte
//! of the file untouched. This deliberately avoids the whole-map rewrite in
//! `write_metadata`, which reconstructs the EXIF segment from
//! display-converted values and cannot round-trip binary tags (e.g.
//! ComponentsConfiguration, GPSVersionID) losslessly.

use crate::core::date_shift::ExifDateTag;
use crate::core::operations_helpers::{read_u16, read_u32};
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// IFD0 tag pointing to the ExifIFD
const EXIF_IFD_POINTER: u16 = 0x8769;
/// TIFF ASCII type code
const ASCII_TYPE: u16 = 2;
/// Byte count of a standard EXIF date/time value (19 chars + NUL)
const DATETIME_LEN: u32 = 20;

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
    let entries_start = offset
        .checked_add(2)
        .ok_or_else(|| ExifToolError::parse_error("IFD offset overflow"))?;
    if entries_start > tiff.len() {
        return Err(ExifToolError::parse_error("IFD offset beyond EXIF data"));
    }
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
}
