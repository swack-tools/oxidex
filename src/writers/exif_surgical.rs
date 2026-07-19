//! Surgical EXIF rewriting with raw-value carry-over
//!
//! The whole-map rebuild in `tiff_writer` re-serializes every tag from its
//! display-converted `TagValue`, which cannot round-trip binary/rational
//! tags and silently drops MakerNotes, InteropIFD, IFD1, and unknown tags
//! (issue #20). This module instead diffs the caller's desired map against
//! the original file's raw IFD entries: entries the caller did not change
//! are carried byte-for-byte (and never re-validated — raw carry-over
//! cannot alter a byte), while changed/added entries pass strict validation
//! and true-typed serialization. The original byte order is preserved, and
//! the MakerNotes blob keeps its original offset so manufacturer-internal
//! absolute offsets stay valid.

use crate::core::operations_helpers::{read_u16, read_u32};
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// IFD0 tag pointing to the ExifIFD
const EXIF_IFD_POINTER: u16 = 0x8769;
/// IFD0 tag pointing to the GPS IFD
const GPS_IFD_POINTER: u16 = 0x8825;
/// ExifIFD tag pointing to the InteropIFD
const INTEROP_POINTER: u16 = 0xA005;
/// IFD1 thumbnail offset / length
const THUMBNAIL_OFFSET: u16 = 0x0201;
const THUMBNAIL_LENGTH: u16 = 0x0202;
/// ExifIFD MakerNote blob
const MAKERNOTE: u16 = 0x927C;

/// Which physical IFD an entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfdKind {
    Ifd0,
    ExifIfd,
    Gps,
    Interop,
    Ifd1,
}

impl IfdKind {
    /// The metadata-map key prefix the reader uses for this IFD.
    pub fn prefix(self) -> &'static str {
        match self {
            IfdKind::Ifd0 => "IFD0",
            IfdKind::ExifIfd => "ExifIFD",
            IfdKind::Gps => "GPS",
            IfdKind::Interop => "InteropIFD",
            IfdKind::Ifd1 => "IFD1",
        }
    }
}

/// One IFD entry with its raw value bytes (inline or offset-stored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    pub ifd: IfdKind,
    pub tag_id: u16,
    pub field_type: u16,
    pub count: u32,
    pub value: Vec<u8>,
}

/// Everything extracted from an original EXIF TIFF structure.
#[derive(Debug, Clone, PartialEq)]
pub struct ExifScan {
    pub byte_order: ByteOrder,
    /// All entries except structural pointer tags (regenerated on write)
    pub entries: Vec<RawEntry>,
    /// Thumbnail bytes captured via IFD1's JPEGInterchangeFormat pair
    pub thumbnail: Option<Vec<u8>>,
    /// Original value offset of the MakerNote blob (for offset-stable layout)
    pub makernote_offset: Option<usize>,
}

/// Byte size of one value of the given TIFF field type.
pub(crate) fn type_size(field_type: u16) -> usize {
    match field_type {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        _ => 1,             // unknown types: treat as opaque bytes
    }
}

/// Walks IFD0 (and ExifIFD, GPS, InteropIFD, IFD1) and returns every entry
/// with its raw value bytes. Pointer tags are consumed structurally, not
/// returned. Corrupt sub-structures degrade gracefully: an out-of-bounds
/// IFD offset or value offset skips that IFD/entry rather than erroring.
pub fn scan_exif_entries(tiff: &[u8]) -> Result<ExifScan> {
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

    let mut scan = ExifScan {
        byte_order,
        entries: Vec::new(),
        thumbnail: None,
        makernote_offset: None,
    };

    let ifd0_offset = read_u32(&tiff[4..8], byte_order) as usize;
    let ifd0 = walk_ifd(tiff, ifd0_offset, byte_order, IfdKind::Ifd0, &mut scan);

    if let Some(exif_off) = ifd0.exif_pointer {
        let exif = walk_ifd(tiff, exif_off, byte_order, IfdKind::ExifIfd, &mut scan);
        if let Some(interop_off) = exif.interop_pointer {
            walk_ifd(tiff, interop_off, byte_order, IfdKind::Interop, &mut scan);
        }
    }
    if let Some(gps_off) = ifd0.gps_pointer {
        walk_ifd(tiff, gps_off, byte_order, IfdKind::Gps, &mut scan);
    }
    if let Some(ifd1_off) = ifd0.next_ifd {
        let ifd1 = walk_ifd(tiff, ifd1_off, byte_order, IfdKind::Ifd1, &mut scan);
        if let (Some(t_off), Some(t_len)) = (ifd1.thumb_offset, ifd1.thumb_length)
            && t_off.checked_add(t_len).is_some_and(|end| end <= tiff.len())
        {
            scan.thumbnail = Some(tiff[t_off..t_off + t_len].to_vec());
        }
    }

    Ok(scan)
}

/// Pointers discovered while walking one IFD.
#[derive(Default)]
struct WalkResult {
    exif_pointer: Option<usize>,
    gps_pointer: Option<usize>,
    interop_pointer: Option<usize>,
    next_ifd: Option<usize>,
    thumb_offset: Option<usize>,
    thumb_length: Option<usize>,
}

fn walk_ifd(
    tiff: &[u8],
    offset: usize,
    byte_order: ByteOrder,
    which: IfdKind,
    scan: &mut ExifScan,
) -> WalkResult {
    let mut result = WalkResult::default();
    let entries_start = match offset.checked_add(2) {
        Some(end) if end <= tiff.len() => end,
        _ => return result, // corrupt IFD offset: skip this IFD gracefully
    };
    let entry_count = read_u16(&tiff[offset..entries_start], byte_order) as usize;

    for i in 0..entry_count {
        let entry_start = entries_start + i * 12;
        let entry_end = entry_start + 12;
        if entry_end > tiff.len() {
            return result; // truncated IFD: keep what we have
        }
        let entry = &tiff[entry_start..entry_end];
        let tag_id = read_u16(&entry[0..2], byte_order);
        let field_type = read_u16(&entry[2..4], byte_order);
        let count = read_u32(&entry[4..8], byte_order);
        let value_or_offset = read_u32(&entry[8..12], byte_order) as usize;

        // Structural pointers: record and continue (never stored as entries)
        match (which, tag_id) {
            (IfdKind::Ifd0, EXIF_IFD_POINTER) => {
                result.exif_pointer = Some(value_or_offset);
                continue;
            }
            (IfdKind::Ifd0, GPS_IFD_POINTER) => {
                result.gps_pointer = Some(value_or_offset);
                continue;
            }
            (IfdKind::ExifIfd, INTEROP_POINTER) => {
                result.interop_pointer = Some(value_or_offset);
                continue;
            }
            (IfdKind::Ifd1, THUMBNAIL_OFFSET) => {
                result.thumb_offset = Some(value_or_offset);
                continue;
            }
            (IfdKind::Ifd1, THUMBNAIL_LENGTH) => {
                result.thumb_length = Some(value_or_offset);
                continue;
            }
            _ => {}
        }

        let size = match type_size(field_type).checked_mul(count as usize) {
            Some(s) => s,
            None => continue,
        };
        let value = if size <= 4 {
            entry[8..8 + size].to_vec()
        } else {
            match value_or_offset.checked_add(size) {
                Some(end) if end <= tiff.len() => tiff[value_or_offset..end].to_vec(),
                _ => continue, // out-of-bounds value: skip entry, never guess
            }
        };

        if which == IfdKind::ExifIfd && tag_id == MAKERNOTE && size > 4 {
            scan.makernote_offset = Some(value_or_offset);
        }

        scan.entries.push(RawEntry {
            ifd: which,
            tag_id,
            field_type,
            count,
            value,
        });
    }

    // Next-IFD offset follows the entry table
    let next_at = entries_start + entry_count * 12;
    if which == IfdKind::Ifd0
        && next_at + 4 <= tiff.len()
    {
        let next = read_u32(&tiff[next_at..next_at + 4], byte_order) as usize;
        if next != 0 {
            result.next_ifd = Some(next);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16b(v: u16, bo: ByteOrder) -> [u8; 2] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }
    fn u32b(v: u32, bo: ByteOrder) -> [u8; 4] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }

    /// Layout (LE and BE identical offsets):
    ///   0   header (IFD0 at 8)
    ///   8   IFD0: 4 entries (Make ASCII@74, Orientation SHORT inline,
    ///       ExifIFD ptr -> 84, GPS ptr -> 150), next-IFD -> 176
    ///  62   next-IFD field (4 bytes at 8+2+4*12=58..62 -> value 176) -- see math below
    ///  74   "Canon\0" (6 bytes)
    ///  84   ExifIFD: 2 entries (ComponentsConfiguration UNDEFINED count 4
    ///       inline, MakerNote UNDEFINED count 8 @ 116), next=0
    /// 116   makernote bytes (8)
    /// 150   GPS: 1 entry (GPSVersionID BYTE count 4 inline), next=0
    /// 176   IFD1: 3 entries (Compression SHORT inline, 0x0201 -> 220,
    ///       0x0202 = 6), next=0
    /// 220   thumbnail bytes (6)
    fn build_full_tiff(bo: ByteOrder) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(match bo {
            ByteOrder::LittleEndian => b"II",
            ByteOrder::BigEndian => b"MM",
        });
        t.extend_from_slice(&u16b(42, bo));
        t.extend_from_slice(&u32b(8, bo));
        // IFD0 at 8: count=4, entries at 10..58, next at 58..62
        t.extend_from_slice(&u16b(4, bo));
        // Make (0x010F) ASCII count 6 @ 74
        t.extend_from_slice(&u16b(0x010F, bo));
        t.extend_from_slice(&u16b(2, bo));
        t.extend_from_slice(&u32b(6, bo));
        t.extend_from_slice(&u32b(74, bo));
        // Orientation (0x0112) SHORT count 1 inline = 6
        t.extend_from_slice(&u16b(0x0112, bo));
        t.extend_from_slice(&u16b(3, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u16b(6, bo));
        t.extend_from_slice(&u16b(0, bo)); // inline padding
        // ExifIFD pointer -> 84
        t.extend_from_slice(&u16b(0x8769, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(84, bo));
        // GPS pointer -> 150
        t.extend_from_slice(&u16b(0x8825, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(150, bo));
        // next IFD -> 176 (IFD1)
        t.extend_from_slice(&u32b(176, bo));
        // pad 62..74
        t.resize(74, 0);
        t.extend_from_slice(b"Canon\0"); // 74..80
        t.resize(84, 0);
        // ExifIFD at 84: count=2, entries 86..110, next 110..114
        t.extend_from_slice(&u16b(2, bo));
        // ComponentsConfiguration (0x9101) UNDEFINED count 4 inline [1,2,3,0]
        t.extend_from_slice(&u16b(0x9101, bo));
        t.extend_from_slice(&u16b(7, bo));
        t.extend_from_slice(&u32b(4, bo));
        t.extend_from_slice(&[1, 2, 3, 0]);
        // MakerNote (0x927C) UNDEFINED count 8 @ 116
        t.extend_from_slice(&u16b(0x927C, bo));
        t.extend_from_slice(&u16b(7, bo));
        t.extend_from_slice(&u32b(8, bo));
        t.extend_from_slice(&u32b(116, bo));
        t.extend_from_slice(&u32b(0, bo)); // next
        t.resize(116, 0);
        t.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04]); // 116..124
        t.resize(150, 0);
        // GPS at 150: count=1, entry 152..164, next 164..168
        t.extend_from_slice(&u16b(1, bo));
        // GPSVersionID (0x0000) BYTE count 4 inline [2,3,0,0]
        t.extend_from_slice(&u16b(0x0000, bo));
        t.extend_from_slice(&u16b(1, bo));
        t.extend_from_slice(&u32b(4, bo));
        t.extend_from_slice(&[2, 3, 0, 0]);
        t.extend_from_slice(&u32b(0, bo)); // next
        t.resize(176, 0);
        // IFD1 at 176: count=3, entries 178..214, next 214..218
        t.extend_from_slice(&u16b(3, bo));
        // Compression (0x0103) SHORT inline = 6
        t.extend_from_slice(&u16b(0x0103, bo));
        t.extend_from_slice(&u16b(3, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u16b(6, bo));
        t.extend_from_slice(&u16b(0, bo));
        // 0x0201 thumbnail offset -> 220
        t.extend_from_slice(&u16b(0x0201, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(220, bo));
        // 0x0202 thumbnail length = 6
        t.extend_from_slice(&u16b(0x0202, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(6, bo));
        t.extend_from_slice(&u32b(0, bo)); // next
        t.resize(220, 0);
        t.extend_from_slice(&[0xFF, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9]); // 220..226
        t
    }

    fn find<'a>(scan: &'a ExifScan, ifd: IfdKind, tag: u16) -> &'a RawEntry {
        scan.entries
            .iter()
            .find(|e| e.ifd == ifd && e.tag_id == tag)
            .unwrap()
    }

    #[test]
    fn scan_walks_all_ifds_le() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let scan = scan_exif_entries(&tiff).unwrap();
        assert_eq!(scan.byte_order, ByteOrder::LittleEndian);
        assert_eq!(find(&scan, IfdKind::Ifd0, 0x010F).value, b"Canon\0");
        assert_eq!(find(&scan, IfdKind::Ifd0, 0x0112).value, 6u16.to_le_bytes());
        assert_eq!(find(&scan, IfdKind::ExifIfd, 0x9101).value, [1, 2, 3, 0]);
        assert_eq!(
            find(&scan, IfdKind::ExifIfd, 0x927C).value,
            [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04]
        );
        assert_eq!(find(&scan, IfdKind::Gps, 0x0000).value, [2, 3, 0, 0]);
        assert_eq!(find(&scan, IfdKind::Ifd1, 0x0103).value, 6u16.to_le_bytes());
        assert_eq!(scan.makernote_offset, Some(116));
        assert_eq!(
            scan.thumbnail.as_deref(),
            Some(&[0xFF, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9][..])
        );
        // Pointer tags are structural, not entries
        assert!(!scan.entries.iter().any(|e| {
            matches!(e.tag_id, 0x8769 | 0x8825 | 0x0201 | 0x0202)
        }));
    }

    #[test]
    fn scan_walks_all_ifds_be() {
        let tiff = build_full_tiff(ByteOrder::BigEndian);
        let scan = scan_exif_entries(&tiff).unwrap();
        assert_eq!(scan.byte_order, ByteOrder::BigEndian);
        assert_eq!(find(&scan, IfdKind::Ifd0, 0x0112).value, 6u16.to_be_bytes());
        assert_eq!(find(&scan, IfdKind::ExifIfd, 0x9101).value, [1, 2, 3, 0]);
        assert_eq!(scan.thumbnail.as_deref().map(|t| t.len()), Some(6));
    }

    #[test]
    fn scan_survives_corrupt_pointers() {
        let mut tiff = build_full_tiff(ByteOrder::LittleEndian);
        // Corrupt the ExifIFD pointer value (entry at 34, value field 42..46)
        tiff[42..46].copy_from_slice(&60_000u32.to_le_bytes());
        let scan = scan_exif_entries(&tiff).unwrap();
        // ExifIFD entries gone, everything else intact
        assert!(!scan.entries.iter().any(|e| e.ifd == IfdKind::ExifIfd));
        assert!(scan.entries.iter().any(|e| e.ifd == IfdKind::Gps));
        assert!(scan.entries.iter().any(|e| e.ifd == IfdKind::Ifd1));
    }

    #[test]
    fn scan_rejects_invalid_header() {
        assert!(scan_exif_entries(&[]).is_err());
        assert!(scan_exif_entries(b"XX\x2a\x00\x08\x00\x00\x00").is_err());
    }

    #[test]
    fn scan_real_fixture_smoke() {
        // Extract the TIFF slice of a real fixture through parse_segments
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jpeg/makernotes/canon_sample.jpg"
        ))
        .unwrap();
        let tiff = super::super::exif_surgical_test_support::tiff_slice(&bytes);
        let scan = scan_exif_entries(tiff).unwrap();
        assert!(scan.entries.iter().any(|e| e.ifd == IfdKind::Ifd0 && e.tag_id == 0x0132));
        assert!(scan.entries.iter().any(|e| e.ifd == IfdKind::ExifIfd && e.tag_id == MAKERNOTE));
    }
}
