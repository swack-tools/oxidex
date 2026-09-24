//! Keeping maker-note data that lies outside the MakerNote's byte count.
//!
//! # The defect
//!
//! A MakerNote is one ExifIFD entry (0x927C) with a declared byte count, but
//! the offsets inside it routinely address bytes past that count: Casio
//! Type2's PreviewImage (0x2000, and its 0x0003/0x0004 Length/Start pair),
//! Nikon's PreviewIFD preview and the tail of `NEFBitDepth`, the Pentax,
//! Olympus, Samsung and Canon `PreviewImageStart` targets. The surgical EXIF
//! writer ([`super::exif_surgical`]) pins the MakerNote at its original
//! offset -- so the note's own offsets keep pointing where they pointed --
//! but it re-laid every other byte of the block out from scratch, so
//! whatever the note pointed at outside itself was overwritten by the new
//! layout. `-IFD0:ImageDescription=<3000 chars>` on ExifTool's
//! `t/images/Casio2.jpg` read `Casio:PreviewImage` back as `DDDD...`.
//!
//! Measured with the pinned oracle over the 4,096 JPEGs of the conformance
//! corpus (`tools/exiftool-tables/makernote_outofblob_matrix.py survey`):
//! 549 of the 3,086 with a MakerNote have a named maker-note value or
//! `IsOffset` target outside the note -- in bytes no standard structure
//! owns (292 files), in another tag's value (46), straddling the note's end
//! (57), or past the EXIF block entirely, in the JPEG trailer (231).
//!
//! # What ExifTool does
//!
//! It never pins the note. `WriteExif.pl` (13.59) rewrites a maker-note IFD
//! entry by entry (`WriteDirectory` on a `subdirInfo` whose `DataPt` is the
//! whole EXIF block, 1453-1515), reading each value from wherever it lies
//! -- inside the note or not (858-960) -- into the new note's own value
//! area, and shifts the note's absolute offsets to its new position with a
//! `Fixup` (1552-1600). `PreviewImage` data (`DataTag`) is held back in
//! `PREVIEW_INFO` (1992-2004) and appended after the EXIF data when it fits
//! in the segment, its pointers fixed up (2657-2680), or written after the
//! JPEG image with the pointers fixed up there (Writer.pl 6177-6226).
//!
//! # What this writer does instead
//!
//! Relocating a vendor's offsets needs that vendor's offset base and pointer
//! set, which this writer does not model. It keeps the data where it is
//! instead, which leaves every pointer valid without knowing any of them:
//!
//! * [`OriginalLayout`] records where every standard structure of the
//!   original block sits. When the block has a MakerNote, the serializer
//!   keeps -- at their original offsets -- every byte no standard structure
//!   owns (the "holes", where out-of-note data lives), every table whose
//!   size does not change, every value whose bytes do not change, and the
//!   thumbnail; only what the edit changes moves. It never shrinks the
//!   block, so data past its end (a JPEG-trailer preview) stays where the
//!   note says as long as the edit does not grow it.
//! * [`verify_makernote_preserved`] then proves it, before anything is
//!   written: the note is still at its offset with its bytes, the reader's
//!   own maker-note decoders read every row back unchanged from the new
//!   block ([`crate::core::tiff_helpers::makernote_readback`]), and the
//!   bytes every `IsOffset` Start/Length pair locates -- inside the block or
//!   past it, in the carrier -- are unchanged. A write that cannot keep them
//!   (a grown block with a trailer preview behind it; a note borrowing the
//!   bytes of a tag the edit deletes) is refused, the file untouched.

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::writers::exif_surgical::{IfdKind, scan_exif_entries, type_size};

const EXIF_IFD_POINTER: u16 = 0x8769;
const GPS_IFD_POINTER: u16 = 0x8825;
const INTEROP_POINTER: u16 = 0xA005;
const MAKERNOTE: u16 = 0x927C;

/// `IsOffset` Start/Offset rows a maker-note decoder reports, with the
/// Length row that sizes what they locate. The names are ExifTool 13.59's
/// `IsOffset` tags (every loaded table, walked for `IsOffset`) that come in
/// a Start/Length pair, plus Olympus `ZoomedPreviewStart`, which ExifTool
/// does not relocate (Olympus.pm:895) but which locates data all the same.
const OFFSET_PAIRS: &[(&str, &str)] = &[
    ("PreviewImageStart", "PreviewImageLength"),
    ("ThumbnailOffset", "ThumbnailLength"),
    ("OtherImageStart", "OtherImageLength"),
    ("JpgFromRawStart", "JpgFromRawLength"),
    ("HiddenDataOffset", "HiddenDataLength"),
    ("IDCPreviewStart", "IDCPreviewLength"),
    ("MPImageStart", "MPImageLength"),
    ("PreviewJXLStart", "PreviewJXLLength"),
    ("ZoomedPreviewStart", "ZoomedPreviewLength"),
];

fn u16_at(tiff: &[u8], at: usize, bo: ByteOrder) -> Option<u16> {
    let b = tiff.get(at..at.checked_add(2)?)?;
    Some(match bo {
        ByteOrder::LittleEndian => u16::from_le_bytes([b[0], b[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([b[0], b[1]]),
    })
}

fn u32_at(tiff: &[u8], at: usize, bo: ByteOrder) -> Option<u32> {
    let b = tiff.get(at..at.checked_add(4)?)?;
    Some(match bo {
        ByteOrder::LittleEndian => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        ByteOrder::BigEndian => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
    })
}

/// Where the standard structures of an original EXIF block sit: the same
/// IFD graph [`scan_exif_entries`] walks (IFD0, ExifIFD, InteropIFD, GPS,
/// IFD1 and its thumbnail), recorded by position instead of by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OriginalLayout {
    /// Length of the block.
    pub(crate) len: usize,
    /// `(ifd, table offset, rows on disk)` for every directory walked.
    pub(crate) tables: Vec<(IfdKind, usize, usize)>,
    /// `(ifd, tag, value offset, value length)` of every value stored
    /// outside its entry that lies inside the block.
    pub(crate) values: Vec<(IfdKind, u16, usize, usize)>,
    /// `(offset, length)` of IFD1's thumbnail, when it lies inside the block.
    pub(crate) thumbnail: Option<(usize, usize)>,
}

impl OriginalLayout {
    /// Walks `tiff`, or `None` when its header is not a TIFF header.
    pub(crate) fn of(tiff: &[u8]) -> Option<Self> {
        let bo = match tiff.get(0..2)? {
            b"II" => ByteOrder::LittleEndian,
            b"MM" => ByteOrder::BigEndian,
            _ => return None,
        };
        let mut layout = OriginalLayout {
            len: tiff.len(),
            tables: Vec::new(),
            values: Vec::new(),
            thumbnail: None,
        };
        let ifd0 = u32_at(tiff, 4, bo)? as usize;
        let ifd0_links = layout.walk(tiff, bo, ifd0, IfdKind::Ifd0);
        if let Some(exif) = ifd0_links.exif {
            let exif_links = layout.walk(tiff, bo, exif, IfdKind::ExifIfd);
            if let Some(interop) = exif_links.interop {
                layout.walk(tiff, bo, interop, IfdKind::Interop);
            }
        }
        if let Some(gps) = ifd0_links.gps {
            layout.walk(tiff, bo, gps, IfdKind::Gps);
        }
        if let Some(ifd1) = ifd0_links.next {
            let ifd1_links = layout.walk(tiff, bo, ifd1, IfdKind::Ifd1);
            if let (Some(at), Some(len)) = (ifd1_links.thumb_at, ifd1_links.thumb_len)
                && at.checked_add(len).is_some_and(|end| end <= tiff.len())
            {
                layout.thumbnail = Some((at, len));
            }
        }
        Some(layout)
    }

    fn walk(&mut self, tiff: &[u8], bo: ByteOrder, at: usize, ifd: IfdKind) -> Links {
        let mut links = Links::default();
        let Some(rows) = u16_at(tiff, at, bo) else {
            return links;
        };
        let rows = rows as usize;
        self.tables.push((ifd, at, rows));
        for i in 0..rows {
            let entry = at + 2 + 12 * i;
            let (Some(tag), Some(kind), Some(count), Some(field)) = (
                u16_at(tiff, entry, bo),
                u16_at(tiff, entry + 2, bo),
                u32_at(tiff, entry + 4, bo),
                u32_at(tiff, entry + 8, bo),
            ) else {
                return links;
            };
            let field = field as usize;
            match (ifd, tag) {
                (IfdKind::Ifd0, EXIF_IFD_POINTER) => links.exif = Some(field),
                (IfdKind::Ifd0, GPS_IFD_POINTER) => links.gps = Some(field),
                (IfdKind::ExifIfd, INTEROP_POINTER) => links.interop = Some(field),
                (IfdKind::Ifd1, 0x0201) => links.thumb_at = Some(field),
                (IfdKind::Ifd1, 0x0202) => links.thumb_len = Some(field),
                _ => {}
            }
            if let Some(size) = type_size(kind).checked_mul(count as usize)
                && size > 4
                && field.checked_add(size).is_some_and(|end| end <= tiff.len())
            {
                self.values.push((ifd, tag, field, size));
            }
        }
        if ifd == IfdKind::Ifd0 {
            let next = u32_at(tiff, at + 2 + 12 * rows, bo).unwrap_or(0);
            if next != 0 {
                links.next = Some(next as usize);
            }
        }
        links
    }

    /// Every `[start, end)` range a standard structure owns, the MakerNote
    /// included, clamped to the block.
    fn owned(&self) -> Vec<(usize, usize)> {
        let mut owned = vec![(0, 8.min(self.len))];
        for (_, at, rows) in &self.tables {
            owned.push((*at, (at + 2 + 12 * rows + 4).min(self.len)));
        }
        for (_, _, at, len) in &self.values {
            owned.push((*at, at + len));
        }
        if let Some((at, len)) = self.thumbnail {
            owned.push((at, at + len));
        }
        owned.retain(|(start, end)| start < end);
        owned.sort_unstable();
        owned
    }

    /// Every maximal `[start, end)` range of the block no standard structure
    /// owns: where data a MakerNote points at outside itself lives.
    pub(crate) fn holes(&self) -> Vec<(usize, usize)> {
        let mut holes = Vec::new();
        let mut cursor = 0;
        for (start, end) in self.owned() {
            if start > cursor {
                holes.push((cursor, start));
            }
            cursor = cursor.max(end);
        }
        if cursor < self.len {
            holes.push((cursor, self.len));
        }
        holes
    }

    /// The original `(offset, length)` of `ifd`'s out-of-line value for
    /// `tag` (its first occurrence).
    pub(crate) fn value(&self, ifd: IfdKind, tag: u16) -> Option<(usize, usize)> {
        self.values
            .iter()
            .find(|(i, t, _, _)| *i == ifd && *t == tag)
            .map(|(_, _, at, len)| (*at, *len))
    }

    /// The original `(offset, rows)` of `ifd`'s table.
    pub(crate) fn table(&self, ifd: IfdKind) -> Option<(usize, usize)> {
        self.tables
            .iter()
            .find(|(i, _, _)| *i == ifd)
            .map(|(_, at, rows)| (*at, *rows))
    }
}

#[derive(Default)]
struct Links {
    exif: Option<usize>,
    gps: Option<usize>,
    interop: Option<usize>,
    next: Option<usize>,
    thumb_at: Option<usize>,
    thumb_len: Option<usize>,
}

/// An EXIF block inside the file that carries it: `file[tiff_at..][..tiff_len]`
/// is the block (a JPEG APP1 after `Exif\0\0`, a PNG `eXIf` chunk, or the
/// whole of a TIFF file).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Carrier<'a> {
    pub(crate) file: &'a [u8],
    pub(crate) tiff_at: usize,
    pub(crate) tiff_len: usize,
}

impl<'a> Carrier<'a> {
    /// A block standing alone: nothing past its end is reachable.
    pub(crate) fn block(tiff: &'a [u8]) -> Self {
        Carrier {
            file: tiff,
            tiff_at: 0,
            tiff_len: tiff.len(),
        }
    }

    fn tiff(&self) -> &'a [u8] {
        self.file
            .get(self.tiff_at..self.tiff_at.saturating_add(self.tiff_len))
            .unwrap_or_default()
    }
}

fn refused(what: String) -> ExifToolError {
    ExifToolError::unsupported_format(format!(
        "EXIF write refused: {what}. This writer keeps maker-note data outside \
         the MakerNote where it lies instead of relocating it, and this edit \
         cannot keep it; nothing was written"
    ))
}

/// The ASCII value of an IFD0 entry, without its NUL padding.
fn ifd0_ascii(scan: &crate::writers::exif_surgical::ExifScan, tag: u16) -> Option<String> {
    scan.entries
        .iter()
        .find(|e| e.ifd == IfdKind::Ifd0 && e.tag_id == tag)
        .map(|e| {
            let end = e
                .value
                .iter()
                .position(|b| *b == 0)
                .unwrap_or(e.value.len());
            String::from_utf8_lossy(&e.value[..end])
                .trim_end()
                .to_string()
        })
}

fn integer(map: &MetadataMap, key: &str) -> Option<usize> {
    match map.get(key)? {
        TagValue::Integer(v) => usize::try_from(*v).ok(),
        TagValue::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Post-condition of an EXIF write on a block with a MakerNote, checked on
/// the produced bytes before anything is committed: every maker-note value
/// reads back unchanged, including the data it locates outside the note.
///
/// * the MakerNote is at its original offset with its original bytes (a
///   note the edit dropped entirely, or a block dropped entirely, is not
///   checked);
/// * [`makernote_readback`](crate::core::tiff_helpers::makernote_readback)
///   of the new block equals that of the original, row for row, decoded
///   under the original Make and Model;
/// * every `IsOffset` Start/Length pair the note decodes to locates the same
///   bytes in `output`'s carrier as in `original`'s -- inside the block or
///   past its end (a JPEG-trailer preview), when the original carrier holds
///   them.
///
/// Any mismatch refuses the write.
pub(crate) fn verify_makernote_preserved(original: Carrier<'_>, output: Carrier<'_>) -> Result<()> {
    let before_tiff = original.tiff();
    let Ok(before) = scan_exif_entries(before_tiff) else {
        return Ok(());
    };
    let Some(note_at) = before.makernote_offset else {
        return Ok(());
    };
    let Some(note) = before
        .entries
        .iter()
        .find(|e| e.ifd == IfdKind::ExifIfd && e.tag_id == MAKERNOTE)
    else {
        return Ok(());
    };
    let after_tiff = output.tiff();
    if after_tiff.is_empty() {
        return Ok(());
    }
    let after = scan_exif_entries(after_tiff)?;
    let Some(written) = after
        .entries
        .iter()
        .find(|e| e.ifd == IfdKind::ExifIfd && e.tag_id == MAKERNOTE)
    else {
        return Ok(());
    };
    if after.makernote_offset != Some(note_at) || written.value != note.value {
        return Err(refused(format!(
            "the MakerNote would move from TIFF offset {note_at} or change, which \
             invalidates the absolute offsets inside it"
        )));
    }

    let make = ifd0_ascii(&before, 0x010F).unwrap_or_default();
    let model = ifd0_ascii(&before, 0x0110);
    let read = |tiff: &[u8]| {
        crate::core::tiff_helpers::makernote_readback(
            tiff,
            note_at,
            note.value.len(),
            before.byte_order,
            &make,
            model.as_deref(),
        )
    };
    let was = read(before_tiff);
    let now = read(after_tiff);
    let mut keys: Vec<&String> = was.keys().chain(now.keys()).collect();
    keys.sort();
    keys.dedup();
    if let Some(key) = keys.into_iter().find(|key| was.get(key) != now.get(key)) {
        return Err(refused(format!(
            "maker-note tag '{key}' would read back differently: the data it \
             locates outside the MakerNote would be overwritten"
        )));
    }

    for key in was.keys() {
        let Some((group, name)) = key.rsplit_once(':') else {
            continue;
        };
        let Some((_, length_name)) = OFFSET_PAIRS.iter().find(|(start, _)| *start == name) else {
            continue;
        };
        let (Some(start), Some(len)) = (
            integer(&was, key),
            integer(&was, &format!("{group}:{length_name}")),
        ) else {
            continue;
        };
        if len == 0 {
            continue;
        }
        let from = original.tiff_at.checked_add(start);
        let Some(held) = from.and_then(|at| original.file.get(at..at.checked_add(len)?)) else {
            continue; // the original carrier does not hold it either
        };
        let to = output.tiff_at.checked_add(start);
        let kept = to.and_then(|at| output.file.get(at..at.checked_add(len)?));
        if kept != Some(held) {
            return Err(refused(format!(
                "maker-note {key} locates {len} bytes at TIFF offset {start}{}, which \
                 this edit would move or overwrite",
                if start >= original.tiff_len {
                    " (past the end of the EXIF block)"
                } else {
                    ""
                }
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le16(v: u16) -> [u8; 2] {
        v.to_le_bytes()
    }
    fn le32(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    /// header | IFD0 {Make "Acm" inline, ExifIFD pointer} @8..38 | ExifIFD
    /// {MakerNote undef[8] @56} @38..56 | note @56..64 | unowned bytes
    /// @64..70 (a hole).
    fn block() -> Vec<u8> {
        let mut t = b"II".to_vec();
        t.extend(le16(42));
        t.extend(le32(8));
        // IFD0 @8: 1 entry + pointer => 2 rows: 2 + 24 + 4 = 30 -> ends 38
        t.extend(le16(2));
        t.extend(le16(0x010F));
        t.extend(le16(2));
        t.extend(le32(4));
        t.extend(b"Acm\0");
        t.extend(le16(0x8769));
        t.extend(le16(4));
        t.extend(le32(1));
        t.extend(le32(38));
        t.extend(le32(0));
        assert_eq!(t.len(), 38);
        // ExifIFD @38: 1 row: 2 + 12 + 4 = 18 -> ends 56
        t.extend(le16(1));
        t.extend(le16(0x927C));
        t.extend(le16(7));
        t.extend(le32(8));
        t.extend(le32(56));
        t.extend(le32(0));
        assert_eq!(t.len(), 56);
        t.extend(b"NOTEDATA"); // 56..64
        t.extend(b"HOLE!!"); // 64..70
        t
    }

    #[test]
    fn layout_records_every_owned_range_and_the_holes_between() {
        let t = block();
        let layout = OriginalLayout::of(&t).unwrap();
        assert_eq!(
            layout.tables,
            vec![(IfdKind::Ifd0, 8, 2), (IfdKind::ExifIfd, 38, 1)]
        );
        assert_eq!(layout.values, vec![(IfdKind::ExifIfd, 0x927C, 56, 8)]);
        assert_eq!(layout.holes(), vec![(64, 70)]);
        assert_eq!(layout.value(IfdKind::ExifIfd, 0x927C), Some((56, 8)));
        assert_eq!(layout.table(IfdKind::ExifIfd), Some((38, 1)));
    }

    #[test]
    fn a_moved_or_changed_maker_note_is_refused() {
        let t = block();
        let mut changed = t.clone();
        changed[57] = b'X';
        let err = verify_makernote_preserved(Carrier::block(&t), Carrier::block(&changed))
            .unwrap_err()
            .to_string();
        assert!(err.contains("MakerNote would move"), "{err}");
        verify_makernote_preserved(Carrier::block(&t), Carrier::block(&t)).unwrap();
        // a dropped block is not this check's business
        verify_makernote_preserved(Carrier::block(&t), Carrier::block(&[])).unwrap();
    }
}
