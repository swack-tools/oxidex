//! The directory chain past IFD1 of an EXIF block (IFD2, IFD3, ...), carried
//! through a re-laid-out block and re-pointed when it locates a JPEG preview
//! after the image.
//!
//! # What pinned ExifTool 13.59 does
//!
//! It rewrites every directory of the chain (`WriteExif.pl` follows each
//! next-IFD pointer) and keeps it; only `IFD1:All` (with IFD1) or a whole
//! EXIF deletion drops it. Data a record locates inside the block -- an
//! out-of-line value, or the strip / JPEG an offset-and-length pair locates
//! -- is copied into the new block and its offset fixed up
//! (`WriteExif.pl` 13.59:2466-2468, "take data from old dir data buffer").
//! Data located *outside* the block is an error ("Error reading StripOffsets
//! data in IFD2", 2530-2532) -- the write is refused -- with one exception:
//! in a JPEG APP1, IFD2's 0x0111/0x0117 pair is `PreviewImageStart`/
//! `PreviewImageLength` (`Exif.pm` 13.59:632-636, "APP1 IFD2 is for Leica
//! JPEG preview"), which ExifTool handles through `$$et{PREVIEW_INFO}`:
//!
//! * `WriteExif.pl` 13.59:2513-2525: the preview is read from the file at
//!   its offset; if the bytes are there and start like a JPEG
//!   (`/^.\xd8\xff[\xc4\xdb\xe0-\xef]/s`) they are held as the preview,
//!   otherwise the flag `LOAD_PREVIEW` is held instead.
//! * 2533-2547: `PREVIEW_INFO` records it (with a `Fixup` for the pointer);
//!   `WasContained` when it lay inside the EXIF data.
//! * 2657-2685: once the block is laid out, preview data that fits in the
//!   APP1 segment is appended to the block and the pointer fixed up there;
//!   otherwise the fixup is kept for the JPEG writer.
//! * `Writer.pl` 13.59:5911-5921: the JPEG writer then buffers the output
//!   from the APP1 on, and at the image's end (6177-6245) computes the new
//!   preview position -- the end of the new image's EOI, relative to the
//!   TIFF header (`length($$outfile) - 10`), plus any "junk" before the
//!   first JPEG header found in the bytes after the EOI (the rest of the
//!   64 KiB read buffer, topped up to 1024 bytes; 65536 for Sony, which
//!   also adjusts for a 32-byte header) -- and sets the pointer to it. Held
//!   preview bytes are written there (and the old trailer preview dropped);
//!   with `LOAD_PREVIEW` the trailer is copied as it was.
//!
//! So a preview after the image is re-pointed on *every* EXIF rewrite, to
//! where the image's EOI ends up. On ExifTool's own 68 Leica samples
//! (truncated: every IFD2 preview offset is past the end of the file) it
//! points the preview at the new EOI; on a whole file it points it at the
//! same preview bytes, moved.
//!
//! # What this writer does
//!
//! * [`scan_chain`] records the chain exactly, or the reason it cannot be
//!   carried (a sub-directory or unpaired offset pointer, a value or data
//!   block partly outside the block, a malformed or cyclic chain).
//! * The serializer (`exif_surgical::serialize_exif`) lays the chain's
//!   tables, values and in-block data out anew after IFD1 and links them;
//!   an out-of-block locator is written back verbatim.
//! * [`refuse_unmovable_outside`] refuses, for a JPEG or PNG, every write
//!   that keeps a chain locating data outside the block other than a JPEG's
//!   IFD2 preview -- as ExifTool refuses them.
//! * [`preview_tail_pointer`] hands a JPEG IFD2 preview's pointer to the
//!   one trailer re-pointing mechanism, `jpeg_trailer::repoint_tail` (which
//!   also re-bases AFCP): preview bytes ExifTool would hold (present,
//!   JPEG-like) and that the writer copied verbatim are pointed at where
//!   they now are; a preview ExifTool would `LOAD_PREVIEW` is pointed at the
//!   new EOI plus junk, as ExifTool does. Anything else -- the bytes not
//!   copied verbatim, a Sony file, a pointer that does not fit its field --
//!   refuses the write.
//! * [`verify_preview_repoint`] is the post-condition on the whole files.

use crate::core::operations_helpers::{read_u16, read_u32};
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Directories walked past IFD1 at most.
pub(crate) const MAX_CHAIN_DIRS: usize = 64;

/// The offset halves of the offset/length pairs a chain directory may carry:
/// `(offset tag, length tag)`. StripOffsets/StripByteCounts (IFD2's
/// PreviewImageStart/Length in a JPEG) and JPEGInterchangeFormat/Length.
pub(crate) const OFFSET_PAIRS: [(u16, u16); 2] = [(0x0111, 0x0117), (0x0201, 0x0202)];

/// Tags that locate a directory or data this module does not model when met
/// in a chain directory: the `%Exif::Main` pointers `exif_surgical` does not
/// relocate, plus the ExifIFD/GPS/Interop sub-directory pointers (which the
/// chain's directories never own here).
fn is_unmodelled_pointer(tag: u16) -> bool {
    crate::writers::exif_surgical::UNMODELLED_POINTER_TAGS.contains(&tag)
        && !OFFSET_PAIRS.iter().any(|(offset, _)| *offset == tag)
        || matches!(tag, 0x8769 | 0x8825 | 0xA005)
}

/// How one record of a chain directory is carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainValue {
    /// Stored in the record; written back verbatim.
    Inline,
    /// An out-of-line value at `at`, inside the block; relocated.
    Value { at: usize, bytes: Vec<u8> },
    /// The offset half of an offset/length pair ([`OFFSET_PAIRS`]) whose
    /// data lies inside the block; relocated.
    Data { at: usize, bytes: Vec<u8> },
    /// The offset half of a pair whose data starts at or after the end of
    /// the block (`at` block-relative, `len` bytes): written back verbatim
    /// and, for a JPEG's IFD2 preview, re-pointed by the JPEG writer.
    Outside { at: usize, len: usize },
}

/// One record of a chain directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainRecord {
    pub tag_id: u16,
    pub field_type: u16,
    pub count: u32,
    /// The 4-byte value field as stored.
    pub field: [u8; 4],
    pub value: ChainValue,
}

/// One directory of the chain, at its original table offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainDir {
    pub table_at: usize,
    pub records: Vec<ChainRecord>,
}

/// The chain IFD1's next-IFD pointer starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfdChain {
    /// IFD1's next-IFD pointer.
    pub first: usize,
    /// The directories, IFD2 first, as far as they could be walked.
    pub dirs: Vec<ChainDir>,
    /// Why the chain cannot be carried through a re-laid-out block, if it
    /// cannot: the serializer refuses such a block.
    pub refusal: Option<String>,
}

impl IfdChain {
    /// IFD2's preview pair when it locates data outside the block: the
    /// block-relative `(offset, length)`.
    pub(crate) fn outside_preview(&self) -> Option<(usize, usize)> {
        self.dirs
            .first()?
            .records
            .iter()
            .find_map(|r| match r.value {
                ChainValue::Outside { at, len } if r.tag_id == 0x0111 => Some((at, len)),
                _ => None,
            })
    }

    /// Every out-of-block locator other than IFD2's preview pair, as
    /// `(directory index, tag)`.
    pub(crate) fn other_outside(&self) -> Vec<(usize, u16)> {
        self.dirs
            .iter()
            .enumerate()
            .flat_map(|(i, dir)| {
                dir.records.iter().filter_map(move |r| match r.value {
                    ChainValue::Outside { .. } if !(i == 0 && r.tag_id == 0x0111) => {
                        Some((i, r.tag_id))
                    }
                    _ => None,
                })
            })
            .collect()
    }
}

fn u16_at(tiff: &[u8], at: usize, order: ByteOrder) -> Option<u16> {
    tiff.get(at..at.checked_add(2)?).map(|b| read_u16(b, order))
}

fn u32_at(tiff: &[u8], at: usize, order: ByteOrder) -> Option<u32> {
    tiff.get(at..at.checked_add(4)?).map(|b| read_u32(b, order))
}

/// Walk the chain starting at IFD1's nonzero next pointer `first`.
/// `seen` holds the offsets of IFD0 and IFD1, a link back to which is a
/// cycle. Never fails: what cannot be carried is recorded in
/// [`IfdChain::refusal`].
pub fn scan_chain(tiff: &[u8], order: ByteOrder, first: usize, seen: &[usize]) -> IfdChain {
    let mut chain = IfdChain {
        first,
        dirs: Vec::new(),
        refusal: None,
    };
    let mut seen = seen.to_vec();
    let mut at = first;
    let refusal = loop {
        if seen.contains(&at) {
            break Some(format!("the chain links back to the directory at {at}"));
        }
        if chain.dirs.len() >= MAX_CHAIN_DIRS {
            break Some(format!(
                "the chain is longer than {MAX_CHAIN_DIRS} directories"
            ));
        }
        seen.push(at);
        let index = chain.dirs.len() + 2;
        let Some(count) = u16_at(tiff, at, order) else {
            break Some(format!("IFD{index} at {at} lies outside the block"));
        };
        let count = usize::from(count);
        let next_at = at + 2 + 12 * count;
        let Some(next) = u32_at(tiff, next_at, order) else {
            break Some(format!("IFD{index}'s table at {at} runs past the block"));
        };
        let (dir, why) = scan_dir(tiff, order, at, count, index);
        chain.dirs.push(dir);
        if why.is_some() {
            break why;
        }
        if next == 0 {
            break None;
        }
        at = next as usize;
    };
    chain.refusal = refusal;
    chain
}

/// The records of the directory at `at` with `count` entries, and why it
/// cannot be carried, if it cannot.
fn scan_dir(
    tiff: &[u8],
    order: ByteOrder,
    at: usize,
    count: usize,
    index: usize,
) -> (ChainDir, Option<String>) {
    let mut dir = ChainDir {
        table_at: at,
        records: Vec::with_capacity(count),
    };
    let raw: Vec<(u16, u16, u32, [u8; 4])> = (0..count)
        .map(|i| {
            let r = at + 2 + 12 * i;
            let field: [u8; 4] = tiff[r + 8..r + 12].try_into().expect("4-byte field");
            (
                read_u16(&tiff[r..r + 2], order),
                read_u16(&tiff[r + 2..r + 4], order),
                read_u32(&tiff[r + 4..r + 8], order),
                field,
            )
        })
        .collect();
    let scalar = |tag: u16| {
        raw.iter()
            .find(|(t, _, _, _)| *t == tag)
            .filter(|(_, typ, count, _)| *count == 1 && matches!(typ, 3 | 4))
            .map(|(_, typ, _, field)| match typ {
                3 => usize::from(read_u16(&field[..2], order)),
                _ => read_u32(field, order) as usize,
            })
    };
    let mut why = None;
    for &(tag_id, field_type, count, field) in &raw {
        let mut refuse = |what: String| {
            why.get_or_insert(format!("IFD{index} tag 0x{tag_id:04X} {what}"));
        };
        let value = if !(1..=13).contains(&field_type) {
            refuse(format!("has unknown field type {field_type}"));
            ChainValue::Inline
        } else if is_unmodelled_pointer(tag_id) {
            refuse("locates a directory or data this writer does not relocate".to_string());
            ChainValue::Inline
        } else if let Some(&(_, length_tag)) =
            OFFSET_PAIRS.iter().find(|(offset, _)| *offset == tag_id)
        {
            match (scalar(tag_id), scalar(length_tag)) {
                (Some(start), Some(len)) if start >= tiff.len() => {
                    ChainValue::Outside { at: start, len }
                }
                (Some(start), Some(len)) => match tiff.get(start..start.saturating_add(len)) {
                    Some(bytes) => ChainValue::Data {
                        at: start,
                        bytes: bytes.to_vec(),
                    },
                    None => {
                        refuse(format!(
                            "locates {len} bytes at {start}, running past the block's end"
                        ));
                        ChainValue::Inline
                    }
                },
                _ => {
                    refuse(format!(
                        "is an offset without a single SHORT/LONG 0x{length_tag:04X} length \
                         beside it"
                    ));
                    ChainValue::Inline
                }
            }
        } else {
            let size =
                crate::writers::exif_surgical::type_size(field_type).checked_mul(count as usize);
            match size {
                Some(size) if size <= 4 => ChainValue::Inline,
                Some(size) => {
                    let start = read_u32(&field, order) as usize;
                    match tiff.get(start..start.saturating_add(size)) {
                        Some(bytes) => ChainValue::Value {
                            at: start,
                            bytes: bytes.to_vec(),
                        },
                        None => {
                            refuse(format!(
                                "has a {size}-byte value at {start}, outside the block"
                            ));
                            ChainValue::Inline
                        }
                    }
                }
                None => {
                    refuse(format!("has an impossible count {count}"));
                    ChainValue::Inline
                }
            }
        };
        dir.records.push(ChainRecord {
            tag_id,
            field_type,
            count,
            field,
            value,
        });
    }
    (dir, why)
}

/// The chain of the TIFF block `tiff` (IFD1's next pointer on), or `None`
/// when the header is not a TIFF header with one of `magics`, or IFD0 or
/// IFD1 is missing, or IFD1's next pointer is zero.
pub(crate) fn chain_of(tiff: &[u8], magics: &[u16]) -> Option<IfdChain> {
    let order = match tiff.get(..2)? {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return None,
    };
    if !magics.contains(&u16_at(tiff, 2, order)?) {
        return None;
    }
    let next_of = |at: usize| {
        u32_at(
            tiff,
            at + 2 + 12 * usize::from(u16_at(tiff, at, order)?),
            order,
        )
    };
    let ifd0 = u32_at(tiff, 4, order)? as usize;
    let ifd1 = next_of(ifd0)? as usize;
    if ifd1 == 0 {
        return None;
    }
    let first = next_of(ifd1)? as usize;
    if first == 0 {
        return None;
    }
    Some(scan_chain(tiff, order, first, &[ifd0, ifd1]))
}

/// A chain as a post-condition compares it: every record's tag, type and
/// count, its inline field, and the bytes it locates -- not where the tables
/// and data sit, which a re-laid-out block moves. An out-of-block locator
/// compares its offset and length; with `preview_wild`, IFD2's preview
/// offset is left out (the JPEG writer re-points it, and checks it,
/// [`verify_preview_repoint`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChainSignature(Vec<Vec<(u16, u16, u32, SignatureValue)>>);

#[derive(Debug, PartialEq, Eq)]
enum SignatureValue {
    Inline([u8; 4]),
    Located(Vec<u8>),
    Outside(Option<usize>, usize),
}

impl IfdChain {
    pub(crate) fn signature(&self, preview_wild: bool) -> ChainSignature {
        ChainSignature(
            self.dirs
                .iter()
                .enumerate()
                .map(|(i, dir)| {
                    dir.records
                        .iter()
                        .map(|r| {
                            let value = match &r.value {
                                ChainValue::Inline => SignatureValue::Inline(r.field),
                                ChainValue::Value { bytes, .. }
                                | ChainValue::Data { bytes, .. } => {
                                    SignatureValue::Located(bytes.clone())
                                }
                                ChainValue::Outside { at, len } => SignatureValue::Outside(
                                    (!(preview_wild && i == 0 && r.tag_id == 0x0111))
                                        .then_some(*at),
                                    *len,
                                ),
                            };
                            (r.tag_id, r.field_type, r.count, value)
                        })
                        .collect()
                })
                .collect(),
        )
    }
}

/// Refuse a write to a JPEG (`jpeg`) or PNG EXIF block whose chain, kept by
/// the write, locates data outside the block that ExifTool would not
/// re-point: it refuses such writes ("Error reading StripOffsets data in
/// IFD2", "Error reading JpgFromRaw data in IFD2"; `WriteExif.pl`
/// 13.59:2530-2532), and so does this writer. Only a JPEG's IFD2 preview
/// pair is re-pointed. `output` is the rewritten block; a write that drops
/// the chain (`IFD1:All`, a whole deletion) is not refused.
pub(crate) fn refuse_unmovable_outside(original: &[u8], output: &[u8], jpeg: bool) -> Result<()> {
    let magics = crate::writers::exif_surgical::EXIF_BLOCK_MAGICS;
    let Some(chain) = chain_of(original, magics) else {
        return Ok(());
    };
    if chain_of(output, magics).is_none() {
        return Ok(());
    }
    let mut outside = chain.other_outside();
    if !jpeg && let Some(_) = chain.outside_preview() {
        outside.insert(0, (0, 0x0111));
    }
    if let Some((dir, tag)) = outside.first() {
        return Err(ExifToolError::unsupported_format(format!(
            "Cannot write this EXIF block: IFD{} tag 0x{tag:04X} locates data outside \
             the block, which pinned ExifTool 13.59 refuses to rewrite (\"Error reading \
             ... data in IFD{}\") and this writer cannot relocate; nothing was written",
            dir + 2,
            dir + 2
        )));
    }
    Ok(())
}

/// ExifTool's test that preview bytes it read are a JPEG
/// (`WriteExif.pl` 13.59:2518: `/^.\xd8\xff[\xc4\xdb\xe0-\xef]/s`).
fn looks_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && bytes[1] == 0xD8
        && bytes[2] == 0xFF
        && matches!(bytes[3], 0xC4 | 0xDB | 0xE0..=0xEF)
}

/// A JPEG EXIF block located in a file: `(TIFF header offset, block length)`.
pub(crate) type BlockAt = (usize, usize);

/// The IFD2 PreviewImage pointer of `output`, a JPEG EXIF rewrite of
/// `original`, as a [`TailPointer`](super::jpeg_trailer::TailPointer) for
/// the shared trailer re-pointing (`jpeg_trailer::rebase_trailer_offsets`);
/// `None` when either file's first EXIF block has no IFD2 preview outside
/// itself (or the write dropped the chain).
///
/// Preview bytes ExifTool would hold -- in the original file at the pointer,
/// JPEG-like (`WriteExif.pl` 13.59:2513-2525) -- are to be located where
/// the verbatim copy moved them ([`TailTarget::Moved`]); otherwise the
/// pointer goes where ExifTool's `LOAD_PREVIEW` puts it, just after the
/// image's EOI ([`TailTarget::AfterEoi`]). A Sony block, whose preview
/// ExifTool places by rules not modelled here (`Writer.pl` 13.59:6180-6206),
/// refuses the write.
///
/// [`TailTarget::Moved`]: super::jpeg_trailer::TailTarget::Moved
/// [`TailTarget::AfterEoi`]: super::jpeg_trailer::TailTarget::AfterEoi
pub(crate) fn preview_tail_pointer(
    original: &[u8],
    output: &[u8],
) -> Result<Option<super::jpeg_trailer::TailPointer>> {
    use super::jpeg_trailer::{TailPointer, TailTarget};
    let at = crate::writers::exif_surgical::jpeg_exif_block_at;
    let (Some(orig), Some(out)) = (at(original), at(output)) else {
        return Ok(None);
    };
    let magics = crate::writers::exif_surgical::EXIF_BLOCK_MAGICS;
    let (Some(before_block), Some(after_block)) = (
        original.get(orig.0..orig.0 + orig.1),
        output.get(out.0..out.0 + out.1),
    ) else {
        return Ok(None);
    };
    let (Some(before), Some(after)) = (
        chain_of(before_block, magics),
        chain_of(after_block, magics),
    ) else {
        return Ok(None);
    };
    let (Some((start, len)), Some(_)) = (before.outside_preview(), after.outside_preview()) else {
        return Ok(None);
    };
    if is_sony(before_block) {
        return Err(ExifToolError::unsupported_format(
            "Cannot re-point the IFD2 PreviewImage after this write: a Sony block, whose \
             preview ExifTool relocates by rules this writer does not model; nothing was \
             written",
        ));
    }
    let record = after.dirs[0]
        .records
        .iter()
        .position(|r| r.tag_id == 0x0111)
        .expect("outside_preview found the 0x0111 record");
    let field_type = after.dirs[0].records[record].field_type;
    let held = orig.0.checked_add(start).and_then(|from| {
        original
            .get(from..from.checked_add(len)?)
            .filter(|bytes| looks_like_jpeg(bytes))
            .map(|_| from)
    });
    Ok(Some(TailPointer {
        field_at: out.0 + after.dirs[0].table_at + 2 + 12 * record + 8,
        wide: field_type != 3,
        big_endian: &after_block[..2] == b"MM",
        base: out.0,
        target: match held {
            Some(from) => TailTarget::Moved { from, len },
            None => TailTarget::AfterEoi,
        },
    }))
}

/// Whether IFD0's Make starts with "SONY" (any case), as ExifTool's
/// `$$self{Make} =~ /^SONY/i` tests.
fn is_sony(tiff: &[u8]) -> bool {
    crate::writers::exif_surgical::scan_exif_entries(tiff)
        .ok()
        .and_then(|scan| {
            scan.entries
                .iter()
                .find(|e| {
                    e.ifd == crate::writers::exif_surgical::IfdKind::Ifd0 && e.tag_id == 0x010F
                })
                .map(|e| {
                    e.value
                        .get(..4)
                        .is_some_and(|m| m.eq_ignore_ascii_case(b"SONY"))
                })
        })
        .unwrap_or(false)
}

/// Post-condition of a JPEG EXIF write on the whole files: the chain past
/// IFD1 kept by the write locates what it located. Every out-of-block
/// locator but IFD2's preview is unchanged and, if the original file held
/// its bytes, still locates them (a write that moves them is refused).
/// IFD2's preview, if the original file held JPEG-like bytes there, locates
/// those same bytes wherever they now are; otherwise it points where
/// ExifTool's `LOAD_PREVIEW` puts it (`jpeg_trailer::exiftool_preview_position` of the output,
/// searched from `out_scan_from`). A write that drops the chain, or a block
/// without one, is not checked here (`verify_exif_write` covers the block).
pub(crate) fn verify_preview_repoint(
    original: &[u8],
    orig: BlockAt,
    output: &[u8],
    out: BlockAt,
    out_scan_from: Option<usize>,
) -> Result<()> {
    let magics = crate::writers::exif_surgical::EXIF_BLOCK_MAGICS;
    let (Some(before_block), Some(after_block)) = (
        original.get(orig.0..orig.0 + orig.1),
        output.get(out.0..out.0 + out.1),
    ) else {
        return Ok(());
    };
    let (Some(before), Some(after)) = (
        chain_of(before_block, magics),
        chain_of(after_block, magics),
    ) else {
        return Ok(());
    };
    let failed = |what: String| {
        ExifToolError::unsupported_format(format!(
            "EXIF write verification failed: {what}; nothing was written"
        ))
    };
    fn located(file: &[u8], header: usize, at: usize, len: usize) -> Option<&[u8]> {
        header
            .checked_add(at)
            .and_then(|from| file.get(from..from.checked_add(len)?))
    }
    let outside = |chain: &IfdChain| -> Vec<(usize, u16, usize, usize)> {
        chain
            .dirs
            .iter()
            .enumerate()
            .flat_map(|(i, dir)| {
                dir.records.iter().filter_map(move |r| match r.value {
                    ChainValue::Outside { at, len } => Some((i, r.tag_id, at, len)),
                    _ => None,
                })
            })
            .collect()
    };
    let (was, now) = (outside(&before), outside(&after));
    if was.len() != now.len() {
        return Err(failed(
            "the directory chain past IFD1 no longer locates the same data outside the \
             EXIF block"
                .to_string(),
        ));
    }
    for ((dir, tag, at, len), (dir2, tag2, at2, len2)) in was.into_iter().zip(now) {
        if (dir, tag, len) != (dir2, tag2, len2) {
            return Err(failed(format!(
                "IFD{} tag 0x{tag:04X} of the chain past IFD1 changed",
                dir + 2
            )));
        }
        let held = located(original, orig.0, at, len);
        let preview = dir == 0 && tag == 0x0111;
        match held {
            Some(bytes) if !preview || looks_like_jpeg(bytes) => {
                if located(output, out.0, at2, len) != Some(bytes) {
                    return Err(failed(format!(
                        "IFD{} tag 0x{tag:04X} located {len} bytes at TIFF offset {at}, after \
                         the EXIF block, and no longer locates them (now {at2})",
                        dir + 2
                    )));
                }
            }
            _ if preview => {
                let expected = out_scan_from
                    .and_then(|scan| super::jpeg_trailer::exiftool_preview_position(output, scan))
                    .and_then(|abs| abs.checked_sub(out.0));
                if expected != Some(at2) {
                    return Err(failed(format!(
                        "IFD2's PreviewImageStart is {at2}, not the end of the image's EOI \
                         where ExifTool points a preview it cannot load ({expected:?})"
                    )));
                }
            }
            _ => {
                if at2 != at {
                    return Err(failed(format!(
                        "IFD{} tag 0x{tag:04X} of the chain past IFD1 was re-pointed from {at} \
                         to {at2}",
                        dir + 2
                    )));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_eoi_position_skips_junk_before_a_jpeg_header() {
        // SOI .. scan from 4: two bytes, EOI at 6..8, junk "ab", then a preview.
        let mut f = vec![0xFF, 0xD8, 0xFF, 0xDA, 0x12, 0x34, 0xFF, 0xD9];
        assert_eq!(
            super::super::jpeg_trailer::exiftool_preview_position(&f, 4),
            Some(8)
        );
        f.extend(b"ab");
        f.extend([0xFF, 0xD8, 0xFF, 0xE1, 0, 0]);
        assert_eq!(
            super::super::jpeg_trailer::exiftool_preview_position(&f, 4),
            Some(10)
        );
        // `.\xd8\xff\xdb`: the byte before counts as the start.
        let mut g = f[..8].to_vec();
        g.extend([b'x', 0x00, 0xD8, 0xFF, 0xDB, 0, 0]);
        assert_eq!(
            super::super::jpeg_trailer::exiftool_preview_position(&g, 4),
            Some(9)
        );
        // No EOI.
        assert_eq!(
            super::super::jpeg_trailer::exiftool_preview_position(&f[..6], 4),
            None
        );
    }

    /// A little-endian block: IFD0 {} -> IFD1 {Compression 6} -> IFD2
    /// {0x0111 = `preview_at`, 0x0117 = 6}, then `pad` bytes.
    fn block(preview_at: u32, pad: usize) -> Vec<u8> {
        let mut t = b"II\x2a\0\x08\0\0\0".to_vec();
        t.extend(0u16.to_le_bytes());
        t.extend(14u32.to_le_bytes()); // IFD0 @8: 0 rows, next -> 14
        t.extend(1u16.to_le_bytes()); // IFD1 @14
        t.extend([0x03, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        t.extend(32u32.to_le_bytes());
        t.extend(2u16.to_le_bytes()); // IFD2 @32
        t.extend([0x11, 0x01, 4, 0, 1, 0, 0, 0]);
        t.extend(preview_at.to_le_bytes());
        t.extend([0x17, 0x01, 4, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        t.extend(0u32.to_le_bytes());
        t.extend(vec![0u8; pad]);
        t
    }

    /// SOI, the APP1 holding `tiff` (TIFF header at 12), SOS, two scan
    /// bytes, EOI, then `trailer`.
    fn jpeg(tiff: &[u8], trailer: &[u8]) -> Vec<u8> {
        let mut f = vec![0xFF, 0xD8, 0xFF, 0xE1];
        f.extend(((tiff.len() + 8) as u16).to_be_bytes());
        f.extend(b"Exif\0\0");
        f.extend(tiff);
        f.extend([0xFF, 0xDA, 0x00, 0x04, 0x01, 0x00, 0x12, 0x34, 0xFF, 0xD9]);
        f.extend(trailer);
        f
    }

    /// The JPEG-level post-condition accepts a preview re-pointed onto its
    /// moved bytes, or (unloadable) onto the EOI's end, and refuses an
    /// injected bad offset in either case, a lost chain aside.
    #[test]
    fn the_jpeg_postcondition_refuses_a_mis_pointed_preview() {
        use crate::writers::exif_surgical::{jpeg_exif_block_at, jpeg_scan_start};
        const PREVIEW: &[u8] = &[0xFF, 0xD8, 0xFF, 0xDB, 0xFF, 0xD9];
        let check = |original: &[u8], output: &[u8]| {
            verify_preview_repoint(
                original,
                jpeg_exif_block_at(original).unwrap(),
                output,
                jpeg_exif_block_at(output).unwrap(),
                jpeg_scan_start(output),
            )
        };
        // Held: the preview after the EOI; the block grows by 8.
        let len = block(0, 0).len();
        let eoi_end = |pad: usize| 12 + len + pad + 10 - 12;
        let original = jpeg(&block(eoi_end(0) as u32, 0), PREVIEW);
        assert!(original.ends_with(PREVIEW));
        let good = jpeg(&block(eoi_end(8) as u32, 8), PREVIEW);
        check(&original, &good).unwrap();
        let bad = jpeg(&block(eoi_end(8) as u32 + 2, 8), PREVIEW);
        let err = check(&original, &bad).unwrap_err().to_string();
        assert!(err.contains("no longer locates them"), "{err}");
        // Unloadable (past the end of the file): ExifTool's EOI position.
        let original = jpeg(&block(7_000_000, 0), &[]);
        let good = jpeg(&block(eoi_end(8) as u32, 8), &[]);
        check(&original, &good).unwrap();
        let stale = jpeg(&block(7_000_000, 8), &[]);
        let err = check(&original, &stale).unwrap_err().to_string();
        assert!(err.contains("not the end of the image's EOI"), "{err}");
        // A repointing through the shared trailer writer lands on the same.
        let written = crate::writers::jpeg_trailer::rebase_trailer_offsets(
            &original,
            original.len() - 4,
            original.len() - 4,
            stale.clone(),
        )
        .unwrap();
        assert_eq!(written, good);
    }

    #[test]
    fn looks_like_jpeg_is_exiftools_test() {
        assert!(looks_like_jpeg(&[0xFF, 0xD8, 0xFF, 0xDB]));
        assert!(looks_like_jpeg(&[0x00, 0xD8, 0xFF, 0xE1]));
        assert!(!looks_like_jpeg(&[0xFF, 0xD8, 0xFF, 0xFE]));
        assert!(!looks_like_jpeg(&[0xFF, 0xD8, 0xFF]));
    }
}
