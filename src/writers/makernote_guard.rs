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
//! owns (292 files), running on past the note's end (92), in another tag's
//! value or table (21), or past the EXIF block, in the JPEG trailer (231;
//! 3 more straddle its end). A file can have several.
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
use crate::writers::exif_surgical::{IfdKind, scan_entries_with_magics, type_size};

const EXIF_IFD_POINTER: u16 = 0x8769;
const GPS_IFD_POINTER: u16 = 0x8825;
const INTEROP_POINTER: u16 = 0xA005;
const MAKERNOTE: u16 = 0x927C;

// Which decoded rows are offset/length pairs: generated from the pinned
// ExifTool's `IsOffset`/`OffsetPair` inventory
// (`tools/exiftool-tables/makernote_offset_pairs.py`), never a hand list.
use super::makernote_offset_pairs::{OFFSET_PAIRS, UNPAIRED_OFFSETS};

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
    /// `(offset, length)` of every table, out-of-line value and located data
    /// of the directory chain past IFD1 ([`OriginalLayout::walk_chain`]).
    pub(crate) chain: Vec<(usize, usize)>,
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
            chain: Vec::new(),
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
            if let Some(first) = ifd1_links.next {
                layout.walk_chain(tiff, bo, first, &[ifd0, ifd1]);
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
            // A pointer or length held inline is decoded by its TIFF type,
            // as the scanner decodes it (`exif_surgical::inline_unsigned`):
            // a big-endian SHORT `00 dc 00 00` is 0xdc.
            let inline = crate::writers::exif_surgical::inline_unsigned(
                kind,
                count,
                &tiff[entry + 8..entry + 12],
                bo,
            );
            match (ifd, tag) {
                (IfdKind::Ifd0, EXIF_IFD_POINTER) => links.exif = Some(inline),
                (IfdKind::Ifd0, GPS_IFD_POINTER) => links.gps = Some(inline),
                (IfdKind::ExifIfd, INTEROP_POINTER) => links.interop = Some(inline),
                (IfdKind::Ifd1, 0x0201) => links.thumb_at = Some(inline),
                (IfdKind::Ifd1, 0x0202) => links.thumb_len = Some(inline),
                _ => {}
            }
            if let Some(size) = type_size(kind).checked_mul(count as usize)
                && size > 4
                && field.checked_add(size).is_some_and(|end| end <= tiff.len())
            {
                self.values.push((ifd, tag, field, size));
            }
        }
        if matches!(ifd, IfdKind::Ifd0 | IfdKind::Ifd1) {
            let next = u32_at(tiff, at + 2 + 12 * rows, bo).unwrap_or(0);
            if next != 0 {
                links.next = Some(next as usize);
            }
        }
        links
    }

    /// Records, as owned, the directory chain IFD1's next pointer starts
    /// (IFD2 on: a Leica JPEG's PreviewImage IFD): every table, every
    /// out-of-line value and the data each JPEGInterchangeFormat/Length or
    /// StripOffsets/StripByteCounts pair locates, inside the block. Owned,
    /// they are no hole: a write that keeps the chain carries or refuses it
    /// (`exif_surgical`, IFD1's next pointer), and `IFD1:All`, which deletes
    /// it with IFD1 as pinned ExifTool 13.59 does, does not keep its bytes.
    /// Walks at most 64 directories and stops at one already seen.
    fn walk_chain(&mut self, tiff: &[u8], bo: ByteOrder, first: usize, seen: &[usize]) {
        let mut seen = seen.to_vec();
        let mut at = first;
        while at != 0 && !seen.contains(&at) && seen.len() < 66 {
            seen.push(at);
            let Some(rows) = u16_at(tiff, at, bo) else {
                return;
            };
            let rows = rows as usize;
            self.chain.push((at, 2 + 12 * rows + 4));
            let mut inline: Vec<(u16, usize)> = Vec::new();
            for i in 0..rows {
                let entry = at + 2 + 12 * i;
                let (Some(tag), Some(kind), Some(count), Some(field)) = (
                    u16_at(tiff, entry, bo),
                    u16_at(tiff, entry + 2, bo),
                    u32_at(tiff, entry + 4, bo),
                    u32_at(tiff, entry + 8, bo),
                ) else {
                    return;
                };
                if let Some(size) = type_size(kind).checked_mul(count as usize)
                    && size > 4
                {
                    self.chain.push((field as usize, size));
                } else {
                    inline.push((
                        tag,
                        crate::writers::exif_surgical::inline_unsigned(
                            kind,
                            count,
                            &tiff[entry + 8..entry + 12],
                            bo,
                        ),
                    ));
                }
            }
            for (offset_tag, length_tag) in [(0x0201u16, 0x0202u16), (0x0111, 0x0117)] {
                let find = |tag: u16| inline.iter().find(|(t, _)| *t == tag).map(|(_, v)| *v);
                if let (Some(start), Some(len)) = (find(offset_tag), find(length_tag)) {
                    self.chain.push((start, len));
                }
            }
            at = u32_at(tiff, at + 2 + 12 * rows, bo).unwrap_or(0) as usize;
        }
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
        for (at, len) in &self.chain {
            owned.push((*at, at.saturating_add(*len).min(self.len)));
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

    /// The original `(offset, length)` of the `n`-th (from 0) out-of-line
    /// value for `tag` in `ifd`: a directory can hold a tag id more than
    /// once (duplicate MakerNotes).
    pub(crate) fn nth_value(&self, ifd: IfdKind, tag: u16, n: usize) -> Option<(usize, usize)> {
        self.values
            .iter()
            .filter(|(i, t, _, _)| *i == ifd && *t == tag)
            .nth(n)
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

/// The byte ranges of the block `tiff` that the MakerNote at
/// `tiff[note_at..note_at + note_len]` addresses outside itself through its
/// own IFD: every value longer than four bytes, at the offset its entry
/// holds, clipped to the block, less the part inside the note.
///
/// A vendor-neutral reading of an IFD-style note, after ExifTool's
/// `MakerNotes.pm` (13.59) `LocateIFD` (1494) and `GetMakerNoteOffset` /
/// `FixBase` (1149, 1282), without their per-model tables:
///
/// * the IFD is looked for at the header lengths the IFD-style notes use
///   (0 Canon/Minolta, 6 `QVC\0`/`AOC\0`, 8 `OLYMP\0`/`LEICA\0`/`SANYO`,
///   10, 12 `Panasonic`/`SONY`/`OLYMPUS\0II`, 14, 16, 18) in the block's
///   byte order and then the other one, and, for a note
///   carrying its own TIFF header (`Nikon\0\x02`), at that header's first
///   IFD, in that header's byte order and with its base;
/// * a candidate is a plausible directory: 1..=512 entries, the table inside
///   the note, every entry of a TIFF type 1..=13;
/// * its offsets are read from the TIFF header, from the note's start, or
///   from the note's own TIFF header; the reading kept is the one whose
///   values all land inside the block and most of them inside the note,
///   where a note keeps its value data (ExifTool's expectation too), then
///   the one with the most entries. None landing in the block: no reading;
/// * a zero offset is "no data", and the TIFF header is never a reference.
///
/// A note that is no such directory yields nothing. The ranges tell the
/// serializer which bytes outside the note to keep where they are when the
/// structure owning them moves or goes (a Panasonic note whose values run on
/// into IFD1's table, a Sony `SONY PI` CameraParameters running 42 bytes past
/// the note), and let [`verify_makernote_preserved`] check them byte for byte
/// where no decoder reads them.
pub(crate) fn note_references(
    tiff: &[u8],
    note_at: usize,
    note_len: usize,
    bo: ByteOrder,
) -> Vec<(usize, usize)> {
    let note_end = note_at.saturating_add(note_len).min(tiff.len());
    if note_at >= note_end {
        return Vec::new();
    }
    // (IFD offset in the block, byte order, the note's own base if any)
    let mut candidates: Vec<(usize, ByteOrder, Option<usize>)> = Vec::new();
    for k in 0..=12usize {
        let at = note_at + k;
        let inner = match tiff.get(at..at + 4) {
            Some(b"II*\0") => ByteOrder::LittleEndian,
            Some(b"MM\0*") => ByteOrder::BigEndian,
            _ => continue,
        };
        if let Some(first) = u32_at(tiff, at + 4, inner) {
            candidates.push((at + first as usize, inner, Some(at)));
        }
        break;
    }
    // The block's byte order first; then the other one, for the notes that
    // keep their own regardless of the block's (a Panasonic or `SONY PI` note
    // is little-endian inside a big-endian block).
    let other = match bo {
        ByteOrder::LittleEndian => ByteOrder::BigEndian,
        ByteOrder::BigEndian => ByteOrder::LittleEndian,
    };
    for order in [bo, other] {
        for start in [0usize, 6, 8, 10, 12, 14, 16, 18] {
            candidates.push((note_at + start, order, None));
        }
    }
    let valid = |ifd: usize, order: ByteOrder| -> Option<Vec<(u16, usize, usize)>> {
        let rows = u16_at(tiff, ifd, order)? as usize;
        if rows == 0 || rows > 512 || ifd + 2 + 12 * rows > note_end {
            return None;
        }
        let mut values = Vec::new();
        for i in 0..rows {
            let entry = ifd + 2 + 12 * i;
            let kind = u16_at(tiff, entry + 2, order)?;
            let count = u32_at(tiff, entry + 4, order)?;
            if !(1..=13).contains(&kind) {
                return None;
            }
            let size = type_size(kind).checked_mul(count as usize)?;
            if size > 4 {
                values.push((kind, u32_at(tiff, entry + 8, order)? as usize, size));
            }
        }
        Some(values)
    };
    // How many values land inside the note, and whether every value lands
    // inside the block at all, reading offsets from `base`.
    let fit = |values: &[(u16, usize, usize)], base: usize| {
        let lands = |off: usize, size: usize, lo: usize, hi: usize| {
            base.checked_add(off)
                .is_some_and(|s| s >= lo && s.saturating_add(size) <= hi)
        };
        let inside = values
            .iter()
            .filter(|(_, off, size)| lands(*off, *size, note_at, note_end))
            .count();
        let all_in_block = values
            .iter()
            .all(|(_, off, size)| *off == 0 || lands(*off, *size, 0, tiff.len()));
        (all_in_block, inside)
    };
    // The best-fitting reading: a directory whose values all land inside the
    // block, then the most values inside the note, then the most entries --
    // not merely the first plausible table (Apple's `Apple iOS\0\0\x01MM`
    // header reads as a one-entry directory at +10).
    let mut best: Option<((bool, usize, usize), Vec<(u16, usize, usize)>, usize)> = None;
    // A note carrying its own TIFF header says where its IFD is and what its
    // offsets count from: that reading is taken as it stands.
    if let Some((ifd, order, Some(own))) = candidates.first()
        && let Some(values) = valid(*ifd, *order)
    {
        best = Some(((true, usize::MAX, 0), values, *own));
    }
    for (ifd, order, own) in &candidates {
        let Some(values) = valid(*ifd, *order) else {
            continue;
        };
        let rows = u16_at(tiff, *ifd, *order).unwrap_or(0) as usize;
        let bases: Vec<usize> = match own {
            Some(own) => vec![*own],
            None => vec![0, note_at],
        };
        for base in bases {
            let (all_in_block, inside) = fit(&values, base);
            let score = (all_in_block, inside, rows);
            if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
                best = Some((score, values.clone(), base));
            }
        }
    }
    let Some(((true, _, _), values, base)) = best else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (_, off, size) in values {
        // A zero offset is TIFF's "no data" (Canon writes one for an empty
        // tag 0x0000), not a reference to the header.
        if off == 0 {
            continue;
        }
        let Some(at) = base.checked_add(off) else {
            continue;
        };
        let end = at.saturating_add(size).min(tiff.len());
        // The TIFF header is always rewritten.
        let start = at.max(8);
        for (s, e) in [(start, end.min(note_at)), (start.max(note_end), end)] {
            if s < e {
                out.push((s, e));
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// An EXIF block inside the file that carries it: `file[tiff_at..][..tiff_len]`
/// is the block (a JPEG APP1 after `Exif\0\0`, a PNG `eXIf` chunk, or the
/// whole of a TIFF file).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Carrier<'a> {
    pub(crate) file: &'a [u8],
    pub(crate) tiff_at: usize,
    pub(crate) tiff_len: usize,
    /// Where a Canon `OriginalDecisionDataOffset` counts from, as an index
    /// into `file`: the file start in a JPEG (the offset is a FILE offset
    /// there, Canon.pm 13.59:1786-1795), the TIFF header elsewhere; `None`
    /// for a JPEG's block seen without its file, where the block does not
    /// hold what the offset locates.
    pub(crate) odd_base: Option<usize>,
}

impl<'a> Carrier<'a> {
    /// A block standing alone -- a PNG `eXIf` chunk, a TIFF file: nothing
    /// past its end is reachable, and its offsets count from its header.
    pub(crate) fn block(tiff: &'a [u8]) -> Self {
        Carrier {
            file: tiff,
            tiff_at: 0,
            tiff_len: tiff.len(),
            odd_base: Some(0),
        }
    }

    /// A block taken out of a carrier this check cannot see (the shared
    /// surgical rewrite, which serves JPEG and PNG alike): a FILE offset
    /// cannot be followed in it.
    pub(crate) fn detached(tiff: &'a [u8]) -> Self {
        Carrier {
            odd_base: None,
            ..Carrier::block(tiff)
        }
    }

    /// The `index`-th block of a JPEG file, at `tiff_at` for `tiff_len`.
    pub(crate) fn jpeg(file: &'a [u8], tiff_at: usize, tiff_len: usize) -> Self {
        Carrier {
            file,
            tiff_at,
            tiff_len,
            odd_base: Some(0),
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

fn integer(value: &TagValue) -> Option<usize> {
    match value {
        TagValue::Integer(v) => usize::try_from(*v).ok(),
        TagValue::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Every maker-note row a readback produced, in file order, duplicates
/// included: `(lookup key, family-1 group, every value form)`. Comparing the
/// map's winning projection would let a changed duplicate occurrence pass.
fn occurrence_rows(map: &MetadataMap) -> Vec<(String, String, String)> {
    map.all_occurrences()
        .map(|(key, occurrence)| {
            (
                key,
                occurrence.group1.to_string(),
                format!(
                    "{:?}|{:?}|{:?}",
                    occurrence.raw, occurrence.value, occurrence.print
                ),
            )
        })
        .collect()
}

/// Every MakerNote entry (0x927C, value past its entry) of `scan`, in any
/// directory the reader accepts one in -- ExifIFD, and IFD0 or another
/// top-level directory, which the reader also decodes -- with its offset.
fn makernotes(
    scan: &crate::writers::exif_surgical::ExifScan,
    tiff: &[u8],
) -> Vec<(IfdKind, usize, Vec<u8>)> {
    let layout = OriginalLayout::of(tiff);
    let mut notes = Vec::new();
    for entry in &scan.entries {
        if entry.tag_id != MAKERNOTE || entry.value.len() <= 4 {
            continue;
        }
        if notes.iter().any(|(ifd, _, _)| *ifd == entry.ifd) {
            continue; // the first of duplicates is the one kept and decoded
        }
        let at = if entry.ifd == IfdKind::ExifIfd {
            scan.makernote_offset
        } else {
            layout
                .as_ref()
                .and_then(|l| l.value(entry.ifd, MAKERNOTE))
                .map(|(at, _)| at)
        };
        if let Some(at) = at {
            notes.push((entry.ifd, at, entry.value.clone()));
        }
    }
    notes
}

/// Every physical MakerNote entry (0x927C, value past its entry) of `tiff`,
/// duplicates included, in table order: `(directory, value offset, bytes)`.
/// [`makernotes`] keeps only the first per directory (the one decoded);
/// this is every entry the block carries.
fn physical_makernotes(tiff: &[u8]) -> Vec<(IfdKind, usize, &[u8])> {
    let Some(layout) = OriginalLayout::of(tiff) else {
        return Vec::new();
    };
    layout
        .values
        .iter()
        .filter(|(_, tag, _, _)| *tag == MAKERNOTE)
        .filter_map(|(ifd, _, at, len)| Some((*ifd, *at, tiff.get(*at..at.checked_add(*len)?)?)))
        .collect()
}

/// A directory that keeps any MakerNote after the write keeps every one the
/// original held there, in order, each at its original offset with its
/// original bytes. An IFD may carry several physical 0x927C entries (Apple
/// iPhone JPEGs add an editing app's note after the camera's); pinned
/// ExifTool 13.59 keeps all of them on an edit. The serializer keeps each at
/// its original offset when it can (`serialize_exif_keeping`); one it could
/// not keep in place (an odd offset) would otherwise move or be lost. A
/// directory the write leaves with no MakerNote deleted them.
fn verify_every_physical_note(before_tiff: &[u8], after_tiff: &[u8]) -> Result<()> {
    let before = physical_makernotes(before_tiff);
    let after = physical_makernotes(after_tiff);
    let mut ifds: Vec<IfdKind> = Vec::new();
    for (ifd, _, _) in &before {
        if !ifds.contains(ifd) {
            ifds.push(*ifd);
        }
    }
    for ifd in ifds {
        let of = |notes: &[(IfdKind, usize, &[u8])]| {
            notes
                .iter()
                .filter(|(i, _, _)| *i == ifd)
                .map(|(_, at, bytes)| (*at, bytes.to_vec()))
                .collect::<Vec<_>>()
        };
        let (was, kept) = (of(&before), of(&after));
        // a lone note is checked (with its readback) by the caller's loop
        if was.len() < 2 || kept.is_empty() || kept == was {
            continue;
        }
        return Err(refused(format!(
            "the {} holds {} MakerNote entries and the write would keep only {} of \
             them in place and unchanged",
            ifd.prefix(),
            was.len(),
            kept.iter().filter(|note| was.contains(note)).count()
        )));
    }
    Ok(())
}

/// Post-condition of an EXIF write on a block with a MakerNote, checked on
/// the produced bytes before anything is committed: every maker-note value
/// reads back unchanged, including the data it locates outside the note.
///
/// `magics` are the TIFF magics the carrier accepts (42 for an EXIF block;
/// 42 and 85 for a TIFF-structured file, RW2/RWL included). An original
/// block that cannot be scanned is not assumed safe: unless the write left
/// it byte-identical where it was, the write is refused.
///
/// For every MakerNote the original holds (in ExifIFD or a top-level
/// directory) that the output still holds:
///
/// * it is at its original offset with its original bytes;
/// * the bytes its own IFD addresses outside it are unchanged
///   ([`note_references`]);
/// * [`makernote_readback`](crate::core::tiff_helpers::makernote_readback)
///   of the new block equals that of the original, every occurrence in
///   order, decoded under the original Make/Model AND under the output's
///   (an edit of Make or Model selects the decoder the file is read with
///   next);
/// * every offset/length pair the note decodes to (the pinned `IsOffset` /
///   `OffsetPair` inventory, every occurrence) locates the same bytes in the
///   output's carrier as in the original's -- inside the block or past its
///   end (a JPEG-trailer preview), when the original carrier holds them;
/// * a Canon `OriginalDecisionDataOffset` (no length tag) locates the same
///   OriginalDecisionData block, read with the ODD reader.
///
/// A directory that keeps any MakerNote keeps every physical 0x927C entry
/// the original held there, duplicates included, each in place and
/// unchanged ([`verify_every_physical_note`]).
///
/// Any mismatch refuses the write. A note the write deletes is not checked.
pub(crate) fn verify_makernote_preserved(
    original: Carrier<'_>,
    output: Carrier<'_>,
    magics: &[u16],
) -> Result<()> {
    // A block dropped entirely (a clear) has nothing to check, and the
    // discarded original is never parsed.
    let after_tiff = output.tiff();
    let before_tiff = original.tiff();
    if after_tiff.is_empty() || before_tiff.is_empty() {
        return Ok(());
    }
    let before = match scan_entries_with_magics(before_tiff, magics) {
        Ok(scan) => scan,
        // Nothing can be verified about a block that does not scan; only one
        // the write left untouched, in place, is safe.
        Err(_) if before_tiff == after_tiff && original.tiff_at == output.tiff_at => {
            return Ok(());
        }
        Err(e) => {
            return Err(refused(format!(
                "the original EXIF block cannot be scanned to verify its maker note ({e})"
            )));
        }
    };
    let notes = makernotes(&before, before_tiff);
    if notes.is_empty() {
        return Ok(());
    }
    let after = scan_entries_with_magics(after_tiff, magics)?;
    let after_notes = makernotes(&after, after_tiff);
    verify_every_physical_note(before_tiff, after_tiff)?;

    let identity = |scan: &crate::writers::exif_surgical::ExifScan| {
        (
            ifd0_ascii(scan, 0x010F).unwrap_or_default(),
            ifd0_ascii(scan, 0x0110),
        )
    };
    let mut identities = vec![identity(&before)];
    if identity(&after) != identities[0] {
        identities.push(identity(&after));
    }

    for (ifd, note_at, note) in &notes {
        let Some((_, written_at, written)) = after_notes.iter().find(|(i, _, _)| i == ifd) else {
            continue; // deleted by the write
        };
        if written_at != note_at || written != note {
            return Err(refused(format!(
                "the {} MakerNote would move from TIFF offset {note_at} or change, which \
                 invalidates the absolute offsets inside it",
                ifd.prefix()
            )));
        }

        // The bytes the note's own IFD addresses outside it, where no decoder
        // may read them (`note_references`): unchanged, at the same offsets.
        for (start, end) in note_references(before_tiff, *note_at, note.len(), before.byte_order) {
            if after_tiff.get(start..end) != before_tiff.get(start..end) {
                return Err(refused(format!(
                    "the MakerNote addresses bytes {start}..{end} outside itself, which \
                     this edit would move or overwrite"
                )));
            }
        }

        for (make, model) in &identities {
            let read = |tiff: &[u8]| {
                crate::core::tiff_helpers::makernote_readback(
                    tiff,
                    *note_at,
                    note.len(),
                    before.byte_order,
                    make,
                    model.as_deref(),
                )
            };
            let was_map = read(before_tiff);
            let now_map = read(after_tiff);
            let was = occurrence_rows(&was_map);
            let now = occurrence_rows(&now_map);
            if was != now {
                let key = was
                    .iter()
                    .zip(&now)
                    .find(|(a, b)| a != b)
                    .map(|(a, _)| a.0.clone())
                    .or_else(|| {
                        was.get(now.len())
                            .or(now.get(was.len()))
                            .map(|r| r.0.clone())
                    })
                    .unwrap_or_default();
                return Err(refused(format!(
                    "maker-note tag '{key}' would read back differently (decoded as {make}): \
                     the data it locates outside the MakerNote would be overwritten"
                )));
            }
            verify_located_bytes(&was_map, &original, &output)?;
            verify_original_decision_data(&was_map, &original, &output, before.byte_order)?;
        }
    }
    Ok(())
}

/// [`verify_makernote_preserved`] for a whole JPEG: every `Exif\0\0` APP1
/// block of `original`, not only the first, against the block in the same
/// position of `output`. A later block moves when an earlier segment
/// changes length, and its maker note's file-level targets (a trailer
/// preview, an ODD block) with it. Where the two files hold a different
/// number of EXIF blocks the blocks cannot be paired: refused unless no
/// original block holds a MakerNote (as the generated adapter refuses an
/// ambiguous multi-EXIF JPEG outright).
pub(crate) fn verify_jpeg_makernotes(original: &[u8], output: &[u8]) -> Result<()> {
    use crate::writers::exif_surgical::{EXIF_BLOCK_MAGICS, jpeg_exif_blocks};
    let was = jpeg_exif_blocks(original)?;
    let now = jpeg_exif_blocks(output)?;
    if was.len() != now.len() {
        let holds_note = was.iter().any(|(at, len)| {
            let tiff = &original[*at..at + len];
            match scan_entries_with_magics(tiff, EXIF_BLOCK_MAGICS) {
                Ok(scan) => !makernotes(&scan, tiff).is_empty(),
                Err(_) => false, // no TIFF structure: no maker note to lose
            }
        });
        if holds_note {
            return Err(refused(format!(
                "the JPEG holds {} EXIF blocks and the output {}, so the blocks holding a \
                 MakerNote cannot be paired to verify it",
                was.len(),
                now.len()
            )));
        }
        return Ok(());
    }
    for ((was_at, was_len), (now_at, now_len)) in was.into_iter().zip(now) {
        verify_makernote_preserved(
            Carrier::jpeg(original, was_at, was_len),
            Carrier::jpeg(output, now_at, now_len),
            EXIF_BLOCK_MAGICS,
        )?;
    }
    Ok(())
}

/// Every offset/length pair of `rows` (every occurrence, paired within its
/// family-1 group by rank) locates the same bytes in `output` as in
/// `original`, when the original carrier holds them.
fn verify_located_bytes(
    rows: &MetadataMap,
    original: &Carrier<'_>,
    output: &Carrier<'_>,
) -> Result<()> {
    let occurrences: Vec<(String, String, usize)> = rows
        .all_occurrences()
        .filter_map(|(key, o)| {
            let value = integer(o.value.as_ref().unwrap_or(&o.raw))?;
            let name = key
                .rsplit_once(':')
                .map_or(key.as_str(), |(_, n)| n)
                .to_string();
            Some((name, format!("{}|{}", o.group0, o.group1), value))
        })
        .collect();
    for (offset_name, length_name) in OFFSET_PAIRS {
        let mut groups: Vec<&String> = occurrences
            .iter()
            .filter(|(n, _, _)| n == offset_name)
            .map(|(_, g, _)| g)
            .collect();
        groups.dedup();
        for group in groups {
            let starts = occurrences
                .iter()
                .filter(|(n, g, _)| n == offset_name && g == group);
            let lengths: Vec<usize> = occurrences
                .iter()
                .filter(|(n, g, _)| n == length_name && g == group)
                .map(|(_, _, v)| *v)
                .collect();
            for ((_, _, start), len) in starts.zip(lengths) {
                let (start, len) = (*start, len);
                if len == 0 || start == 0 {
                    continue;
                }
                let from = original.tiff_at.checked_add(start);
                let Some(held) = from.and_then(|at| original.file.get(at..at.checked_add(len)?))
                else {
                    continue; // the original carrier does not hold it either
                };
                let to = output.tiff_at.checked_add(start);
                let kept = to.and_then(|at| output.file.get(at..at.checked_add(len)?));
                if kept != Some(held) {
                    return Err(refused(format!(
                        "maker-note {offset_name} locates {len} bytes at TIFF offset {start}{}, \
                         which this edit would move or overwrite",
                        if start >= original.tiff_len {
                            " (past the end of the EXIF block)"
                        } else {
                            ""
                        }
                    )));
                }
            }
        }
    }
    Ok(())
}

/// A Canon `OriginalDecisionDataOffset` -- an `IsOffset` tag with no length
/// tag (`UNPAIRED_OFFSETS`) -- still locates the same OriginalDecisionData
/// block: read with the ODD reader (Canon.pm 13.59 `ReadODD`, the
/// `Composite:OriginalDecisionData` source) from the original and the
/// output carrier. A FILE offset in a JPEG, TIFF-relative elsewhere.
fn verify_original_decision_data(
    rows: &MetadataMap,
    original: &Carrier<'_>,
    output: &Carrier<'_>,
    order: ByteOrder,
) -> Result<()> {
    debug_assert!(UNPAIRED_OFFSETS.contains(&"OriginalDecisionDataOffset"));
    let little_endian = order == ByteOrder::LittleEndian;
    for (key, o) in rows.all_occurrences() {
        if !key.ends_with(":OriginalDecisionDataOffset") {
            continue;
        }
        let Some(value) = integer(o.value.as_ref().unwrap_or(&o.raw)) else {
            continue;
        };
        if value == 0 {
            continue;
        }
        let (Some(was_base), Some(now_base)) = (original.odd_base, output.odd_base) else {
            continue;
        };
        let locate = |carrier: &Carrier<'_>, base: usize| {
            crate::parsers::tiff::makernotes::canon::original_decision_data::odd_block(
                carrier.file,
                base.saturating_add(value) as u64,
                little_endian,
            )
        };
        let was = locate(original, was_base);
        if was.is_some() && locate(output, now_base) != was {
            return Err(refused(format!(
                "maker-note OriginalDecisionDataOffset locates the OriginalDecisionData \
                 block at {value}, which this edit would move or overwrite"
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

    /// A little-endian block, magic `magic`: IFD0 {Make, ExifIFD} @8,
    /// "Make\0", ExifIFD {MakerNote} , the note, then `tail` (bytes after the
    /// note, where out-of-note data lives). `note(note_at)` builds the note
    /// once its offset is known. Returns the block and the note's offset.
    fn note_block(
        magic: u16,
        make: &str,
        note: &dyn Fn(usize) -> Vec<u8>,
        tail: &[u8],
    ) -> (Vec<u8>, usize) {
        let mut make_bytes = make.as_bytes().to_vec();
        make_bytes.push(0);
        let make_at = 8 + 2 + 12 * 2 + 4;
        let exif_at = make_at + make_bytes.len() + make_bytes.len() % 2;
        let note_at = exif_at + 2 + 12 + 4;
        let len = note(note_at).len();
        let mut t = b"II".to_vec();
        t.extend(le16(magic));
        t.extend(le32(8));
        t.extend(le16(2));
        t.extend(le16(0x010F));
        t.extend(le16(2));
        t.extend(le32(make_bytes.len() as u32));
        t.extend(le32(make_at as u32));
        t.extend(le16(0x8769));
        t.extend(le16(4));
        t.extend(le32(1));
        t.extend(le32(exif_at as u32));
        t.extend(le32(0));
        t.extend(&make_bytes);
        t.resize(exif_at, 0);
        t.extend(le16(1));
        t.extend(le16(0x927C));
        t.extend(le16(7));
        t.extend(le32(len as u32));
        t.extend(le32(note_at as u32));
        t.extend(le32(0));
        assert_eq!(t.len(), note_at);
        t.extend(note(note_at));
        t.extend(tail);
        (t, note_at)
    }

    /// A Casio Type2 note (`QVC\0\0\0`, TIFF-relative offsets) whose
    /// PreviewImage entries (0x2000, `copies` of them) all locate
    /// `preview_len` bytes at `preview_at`, just past the note.
    fn casio_note(
        copies: usize,
        preview_at: impl Fn(usize) -> usize,
        preview_len: u32,
    ) -> impl Fn(usize) -> Vec<u8> {
        move |note_at| {
            let rows = 1 + copies;
            let at = preview_at(note_at + 6 + 2 + 12 * rows + 4);
            let mut n = b"QVC\0\0\0".to_vec();
            n.extend(le16(rows as u16));
            n.extend(le16(0x0002));
            n.extend(le16(3));
            n.extend(le32(2));
            n.extend(le16(320));
            n.extend(le16(240));
            for i in 0..copies {
                n.extend(le16(0x2000));
                n.extend(le16(7));
                n.extend(le32(preview_len));
                n.extend(le32((at + i * preview_len as usize) as u32));
            }
            n.extend(le32(0));
            n
        }
    }

    /// P1 "validate maker notes in RW2 files": a block of magic 85 (RW2/RWL)
    /// is scanned with the carrier's magics, and an original that does not
    /// scan is refused unless the write left it untouched -- never "Ok".
    /// Red at 6e14d505: `scan_exif_entries` accepts magic 42 only, and its
    /// `Err` was returned as `Ok(())`.
    #[test]
    fn a_magic_85_block_is_verified_and_an_unscannable_one_refused() {
        let (t, note_at) = note_block(85, "CASIO", &casio_note(1, |end| end, 8), b"PREVIEW!");
        let mut overwritten = t.clone();
        let n = overwritten.len();
        overwritten[n - 8..].copy_from_slice(b"DDDDDDDD");
        let err = verify_makernote_preserved(
            Carrier::block(&t),
            Carrier::block(&overwritten),
            crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("maker-note"), "{err}");
        verify_makernote_preserved(
            Carrier::block(&t),
            Carrier::block(&t),
            crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
        )
        .unwrap();
        let _ = note_at;
        // With the EXIF magics only, the same original does not scan: that
        // is a refusal, not a pass, unless the block is untouched.
        let err = verify_makernote_preserved(
            Carrier::block(&t),
            Carrier::block(&overwritten),
            crate::writers::exif_surgical::EXIF_BLOCK_MAGICS,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("cannot be scanned"), "{err}");
        verify_makernote_preserved(
            Carrier::block(&t),
            Carrier::block(&t),
            crate::writers::exif_surgical::EXIF_BLOCK_MAGICS,
        )
        .unwrap();
    }

    /// P1 "compare every maker-note occurrence": two PreviewImage entries;
    /// overwriting what the SECOND locates must refuse although the map's
    /// winning projection may show only one of them. Red at 6e14d505.
    #[test]
    fn a_changed_duplicate_occurrence_is_refused() {
        let (t, _) = note_block(
            42,
            "CASIO",
            &casio_note(2, |end| end, 8),
            b"FIRST!!!SECOND!!",
        );
        for which in [0usize, 1] {
            let mut changed = t.clone();
            let n = changed.len();
            let at = n - 16 + 8 * which;
            changed[at..at + 8].copy_from_slice(b"XXXXXXXX");
            let err =
                verify_makernote_preserved(Carrier::block(&t), Carrier::block(&changed), &[42])
                    .unwrap_err()
                    .to_string();
            assert!(err.contains("maker-note"), "occurrence {which}: {err}");
        }
    }

    /// The comparison is over every recorded occurrence, in order: a
    /// duplicate that does not win the map's lookup still counts.
    #[test]
    fn occurrence_rows_keep_every_duplicate() {
        use crate::core::tag_occurrence::Instance;
        let map = |second: &str| {
            let mut m = MetadataMap::new();
            m.insert_occurrence(
                "Casio:Quality",
                TagValue::new_string("Fine"),
                5,
                "Casio",
                Instance(0),
            );
            m.insert_occurrence(
                "Casio:Quality",
                TagValue::new_string(second),
                5,
                "Casio",
                Instance(1),
            );
            m
        };
        let rows = occurrence_rows(&map("Normal"));
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_ne!(rows, occurrence_rows(&map("Economy")));
    }

    /// P1 "verify readback under the output camera identity": a Canon note
    /// (IFD at 0, TIFF-relative offsets) whose PreviewImageInfo locates a
    /// preview past the note. Under the original Make ("Acme") no decoder
    /// reads it; the edit makes the file a Canon, whose decoder follows the
    /// pair -- so overwriting the preview while changing Make must refuse.
    /// Red at 6e14d505, which decoded under the original Make only.
    #[test]
    fn readback_is_checked_under_the_output_identity_too() {
        let canon = |preview_at: usize| {
            move |note_at: usize| {
                let info_at = note_at + 2 + 12 + 4;
                let mut n = le16(1).to_vec();
                n.extend(le16(0x00B6));
                n.extend(le16(4));
                n.extend(le32(12));
                n.extend(le32(info_at as u32));
                n.extend(le32(0));
                for v in [48u32, 2, 8, 160, 120, preview_at as u32, 0, 0, 0, 0, 0, 0] {
                    n.extend(le32(v));
                }
                n
            }
        };
        // two passes: the preview sits right after the note
        let (probe, _) = note_block(42, "Acme", &canon(0), b"PREVIEW!");
        let preview_at = probe.len() - 8;
        let (t, _) = note_block(42, "Acme", &canon(preview_at), b"PREVIEW!");
        let (as_canon, _) = note_block(42, "Canon", &canon(preview_at), b"PREVIEW!");
        assert_eq!(
            as_canon.len(),
            t.len(),
            "Make strings of one length keep offsets"
        );
        // same Make: nothing changed
        verify_makernote_preserved(Carrier::block(&t), Carrier::block(&t), &[42]).unwrap();
        // Make edited to Canon, preview overwritten
        let mut edited = as_canon.clone();
        let n = edited.len();
        edited[n - 8..].copy_from_slice(b"DDDDDDDD");
        let err = verify_makernote_preserved(Carrier::block(&t), Carrier::block(&edited), &[42])
            .unwrap_err()
            .to_string();
        assert!(err.contains("decoded as Canon"), "{err}");
        // Make edited to Canon, preview kept: fine
        verify_makernote_preserved(Carrier::block(&t), Carrier::block(&as_canon), &[42]).unwrap();
    }

    /// `note_references` finds the note's IFD in either byte order and
    /// after a vendor header, and returns only what lies outside the note.
    #[test]
    fn note_references_reads_the_notes_own_ifd() {
        // MM block, LE note "SONY PI\0" + 4 bytes, IFD at +12: one undef
        // value at TIFF 40 of 30 bytes, the note spanning 20..60 -- so the
        // value (40..70) runs 10 bytes past the note's end.
        let mut t = vec![0u8; 80];
        t[..2].copy_from_slice(b"MM");
        let note_at = 20;
        t[note_at..note_at + 8].copy_from_slice(b"SONY PI\0");
        let ifd = note_at + 12;
        t[ifd..ifd + 2].copy_from_slice(&1u16.to_le_bytes());
        t[ifd + 2..ifd + 4].copy_from_slice(&0x2050u16.to_le_bytes());
        t[ifd + 4..ifd + 6].copy_from_slice(&7u16.to_le_bytes());
        t[ifd + 6..ifd + 10].copy_from_slice(&30u32.to_le_bytes());
        t[ifd + 10..ifd + 14].copy_from_slice(&40u32.to_le_bytes());
        assert_eq!(
            note_references(&t, note_at, 40, ByteOrder::BigEndian),
            vec![(60, 70)]
        );
        // not a directory at all: nothing
        let junk = vec![0xEEu8; 80];
        assert!(note_references(&junk, 20, 40, ByteOrder::BigEndian).is_empty());
    }

    #[test]
    fn a_moved_or_changed_maker_note_is_refused() {
        let t = block();
        let mut changed = t.clone();
        changed[57] = b'X';
        let err = verify_makernote_preserved(Carrier::block(&t), Carrier::block(&changed), &[42])
            .unwrap_err()
            .to_string();
        assert!(err.contains("MakerNote would move"), "{err}");
        verify_makernote_preserved(Carrier::block(&t), Carrier::block(&t), &[42]).unwrap();
        // a dropped block is not this check's business
        verify_makernote_preserved(Carrier::block(&t), Carrier::block(&[]), &[42]).unwrap();
    }
}
