//! Absolute file offsets inside JPEG trailers, re-based after a length change.
//!
//! The JPEG writers rewrite only header segments and copy every byte from the
//! end of the edited region onwards verbatim, so a trailer after the EOI
//! moves by exactly `output.len() - original.len()`. Trailers whose
//! structure is sized or relative to the end of the file survive that move;
//! a trailer holding *absolute* file offsets does not, and neither does an
//! EXIF offset that locates data after the image.
//!
//! Every such offset is a [`TailPointer`], re-pointed by one mechanism,
//! [`repoint_tail`]: to the same bytes, moved by the length delta and
//! checked to be there ([`TailTarget::Moved`]), or where ExifTool puts a
//! JPEG preview it cannot load ([`TailTarget::AfterEoi`]). Two producers
//! feed it: AFCP below, and a Leica IFD2 PreviewImage after the image
//! (`ifd_chain::preview_tail_pointer`, `Writer.pl` 13.59:6177-6245). A maker
//! note's trailer preview would be a third.
//!
//! # AFCP
//!
//! AFCP ("AXS File Concatenation Protocol") is laid out as ExifTool 13.59
//! reads and writes it (`Image::ExifTool::AFCP::ProcessAFCP`, AFCP.pm
//! 13.59:72-222):
//!
//! ```text
//! S+0   "AXS!" (big-endian) or "AXS*" (little-endian)       AFCP.pm:85,88
//! S+4   int16u version                                       AFCP.pm:138
//! S+6   int16u entry count n                                 AFCP.pm:143
//! S+8   int32u checksum (ExifTool writes 0)                  AFCP.pm:212
//! S+12  n x { char[4] tag, int32u size, int32u offset }      AFCP.pm:160-164
//!       -- `offset` is an absolute file position             AFCP.pm:166
//! ...   entry data
//! E+0   the same "AXS!"/"AXS*"                               AFCP.pm:85,213
//! E+4   int32u S, the absolute position of the header        AFCP.pm:89-90
//! E+8   int32u checksum (ExifTool writes 0)                  AFCP.pm:213
//! ```
//!
//! On reading, ExifTool follows `E+4`; when it does not land on the header it
//! scans for the real one and shifts every entry offset by the difference,
//! warning `[minor] Adjusted AFCP offsets by N` (AFCP.pm:90-118,154). On
//! writing it re-serializes the trailer and fixes the offsets up to the
//! trailer's final output position (AFCP.pm:205-217; `WriteTrailerBuffer`,
//! Writer.pl 13.59:5522-5543), so its output never carries that warning.
//! This module gives oxidex's verbatim copy the same property: it adds the
//! length delta to `E+4` and to every entry offset.
//!
//! An AFCP trailer that cannot be parsed exactly -- `E+4` not pointing at a
//! matching header, a directory or entry outside the trailer, a non-zero
//! checksum it cannot recompute, a stray `AXS!`/`AXS*` after the EOI that no
//! validated trailer accounts for -- refuses the write instead: shifting it
//! would write offsets nobody can vouch for, and leaving it would write
//! offsets known to be stale.
//!
//! # The other trailers ExifTool identifies (Image::ExifTool::IdentifyTrailer, ExifTool.pm 13.59:6989-7029)
//!
//! None of these is touched here. FotoStation, PhotoMechanic, CanonVRD, MIE,
//! NikonApp and Insta360 are length-framed from their footer; the Samsung
//! SEFH directory and the OnePlus JSON use offsets relative to their own
//! footer; Vivo and Google are located by scanning. They are all
//! position-independent. The exceptions ExifTool re-bases on write and this
//! module does not (yet) are the Samsung `QDIO` block's absolute audio
//! offsets (Samsung.pm 13.59:1754-1765,1862-1874), the Leica trailer
//! referenced from maker notes (Writer.pl 13.59:6147-6173), and maker-note
//! previews in the trailer.

use crate::error::{ExifToolError, Result};

/// The two AFCP signatures: `!` selects big-endian, `*` little-endian.
const AFCP_SIGNATURES: [&[u8; 4]; 2] = [b"AXS!", b"AXS*"];

/// One AFCP trailer located in the original file (absolute positions).
#[derive(Debug, Clone, PartialEq, Eq)]
struct AfcpTrailer {
    /// Header position `S`.
    start: usize,
    /// End-of-file record position `E` (the trailer ends at `E + 12`).
    eof_record: usize,
    big_endian: bool,
    /// Position of each entry's offset field, with the offset it holds.
    entry_offsets: Vec<(usize, u32)>,
}

impl AfcpTrailer {
    fn end(&self) -> usize {
        self.eof_record + 12
    }
}

fn refuse(detail: impl std::fmt::Display) -> ExifToolError {
    ExifToolError::unsupported_format(format!(
        "AFCP trailer cannot be re-based exactly after this write ({detail}); \
         nothing was written"
    ))
}

fn read_u16(bytes: &[u8], at: usize, big_endian: bool) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(if big_endian {
        u16::from_be_bytes(raw)
    } else {
        u16::from_le_bytes(raw)
    })
}

fn read_u32(bytes: &[u8], at: usize, big_endian: bool) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(if big_endian {
        u32::from_be_bytes(raw)
    } else {
        u32::from_le_bytes(raw)
    })
}

fn write_u32(bytes: &mut [u8], at: usize, value: u32, big_endian: bool) {
    let raw = if big_endian {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    bytes[at..at + 4].copy_from_slice(&raw);
}

/// Where ExifTool's `TrailerStart` falls: just after the first EOI at or
/// after `scan_from` (WriteJPEG searches the scan data for `\xff\xd9`,
/// Writer.pl 13.59:6079-6099). `None` when there is no EOI.
fn trailer_start(original: &[u8], scan_from: usize) -> Option<usize> {
    original
        .get(scan_from..)?
        .windows(2)
        .position(|pair| pair == [0xFF, 0xD9])
        .map(|at| scan_from + at + 2)
}

/// Every `AXS!`/`AXS*` in `original[from..]`, as (position, big_endian).
fn signature_hits(original: &[u8], from: usize) -> Vec<(usize, bool)> {
    let Some(region) = original.get(from..) else {
        return Vec::new();
    };
    region
        .windows(4)
        .enumerate()
        .filter_map(|(at, window)| {
            AFCP_SIGNATURES
                .iter()
                .position(|sig| window == sig.as_slice())
                .map(|which| (from + at, which == 0))
        })
        .collect()
}

/// Parse the AFCP trailer whose end-of-file record would sit at `eof_record`.
///
/// `Ok(None)`: the bytes there are not an AFCP end-of-file record (its start
/// pointer does not land on a matching header inside the trailer region).
/// `Err`: they are one, but the trailer is not exactly re-basable.
fn parse_trailer(
    original: &[u8],
    region_start: usize,
    eof_record: usize,
    big_endian: bool,
) -> Result<Option<AfcpTrailer>> {
    let Some(start) = read_u32(original, eof_record + 4, big_endian) else {
        return Ok(None);
    };
    let start = start as usize;
    if eof_record + 12 > original.len()
        || start < region_start
        || start.checked_add(12).is_none_or(|end| end > eof_record)
        || original[start..start + 4] != original[eof_record..eof_record + 4]
    {
        return Ok(None);
    }
    let count = read_u16(original, start + 6, big_endian).unwrap_or(0) as usize;
    let header_checksum = read_u32(original, start + 8, big_endian).unwrap_or(0);
    let eof_checksum = read_u32(original, eof_record + 8, big_endian).unwrap_or(0);
    if header_checksum != 0 || eof_checksum != 0 {
        return Err(refuse(format!(
            "non-zero checksum at {start}/{eof_record}; ExifTool writes 0 and oxidex \
             cannot recompute one"
        )));
    }
    let directory_end = start + 12 + 12 * count;
    if directory_end > eof_record {
        return Err(refuse(format!(
            "{count}-entry directory at {start} runs past its end-of-file record at {eof_record}"
        )));
    }
    let mut entry_offsets = Vec::with_capacity(count);
    for index in 0..count {
        let entry = start + 12 + 12 * index;
        let size = read_u32(original, entry + 4, big_endian).unwrap_or(u32::MAX) as usize;
        let offset = read_u32(original, entry + 8, big_endian).unwrap_or(u32::MAX);
        let data = offset as usize;
        if data < directory_end || data.checked_add(size).is_none_or(|end| end > eof_record) {
            return Err(refuse(format!(
                "entry {index} ({size} bytes at {offset}) lies outside the trailer data \
                 {directory_end}..{eof_record}"
            )));
        }
        entry_offsets.push((entry + 8, offset));
    }
    Ok(Some(AfcpTrailer {
        start,
        eof_record,
        big_endian,
        entry_offsets,
    }))
}

/// Locate every AFCP trailer after the EOI of `original` and prove that each
/// `AXS!`/`AXS*` there belongs to one of them.
fn locate_trailers(
    original: &[u8],
    moved_from: usize,
    scan_from: usize,
) -> Result<Vec<AfcpTrailer>> {
    let region_start = trailer_start(original, scan_from)
        .unwrap_or(scan_from)
        .max(moved_from);
    let hits = signature_hits(original, region_start);
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let mut trailers: Vec<AfcpTrailer> = Vec::new();
    for &(at, big_endian) in &hits {
        if let Some(trailer) = parse_trailer(original, region_start, at, big_endian)? {
            trailers.push(trailer);
        }
    }
    trailers.sort_by_key(|trailer| trailer.start);
    if let Some(pair) = trailers
        .windows(2)
        .find(|pair| pair[1].start < pair[0].end())
    {
        return Err(refuse(format!(
            "trailers at {} and {} overlap",
            pair[0].start, pair[1].start
        )));
    }
    for &(at, _) in &hits {
        let accounted = trailers.iter().any(|trailer| {
            // Its own header and end-of-file record, or bytes of its data.
            at == trailer.start
                || at == trailer.eof_record
                || (at >= trailer.start + 12 && at + 4 <= trailer.eof_record)
        });
        if !accounted {
            return Err(refuse(format!(
                "signature {:?} at {at} is not part of a well-formed AFCP trailer",
                String::from_utf8_lossy(&original[at..at + 4])
            )));
        }
    }
    Ok(trailers)
}

/// An offset field of the rewritten file that locates data after the edited
/// region: data the write copied verbatim from the original's tail, which
/// moved by the length change. The one mechanism for every such pointer --
/// an AFCP trailer's absolute offsets, a Leica IFD2 PreviewImage after the
/// image ([`super::ifd_chain::preview_tail_pointer`]), and whatever else
/// addresses the tail (a maker note's trailer preview) -- so each is
/// re-pointed by the same arithmetic and checked by the same post-condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TailPointer {
    /// Absolute position of the field in the output.
    pub(crate) field_at: usize,
    /// A 32-bit field; else 16-bit.
    pub(crate) wide: bool,
    pub(crate) big_endian: bool,
    /// The absolute output position the value counts from: 0 for an
    /// absolute offset, the TIFF header for an EXIF one.
    pub(crate) base: usize,
    pub(crate) target: TailTarget,
}

/// Where a [`TailPointer`] must point after the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TailTarget {
    /// At the `len` bytes that sat at absolute position `from` of the
    /// original, in its verbatim tail: now `from + delta`, and the bytes
    /// there are checked to be the same.
    Moved { from: usize, len: usize },
    /// Where pinned ExifTool 13.59 points a JPEG preview it cannot load
    /// (`LOAD_PREVIEW`): just after the image's EOI in the output, plus the
    /// junk before a JPEG header ([`exiftool_preview_position`]).
    AfterEoi,
}

/// The absolute position ExifTool gives a JPEG preview it did not load
/// (`Writer.pl` 13.59:6078-6099, 6177-6209): just after the first EOI at or
/// after `scan_from` (the start of the entropy-coded data), plus the junk
/// before the first JPEG header -- `\xff\xd8\xff.` or `.\xd8\xff\xdb`,
/// then two bytes -- in what ExifTool has buffered after that EOI: the rest
/// of the 65536-byte chunk the EOI was read in (chunks start at
/// `scan_from`), topped up to 1024 bytes. `None` when there is no EOI.
/// (Sony's 65536-byte scan and 32-byte header adjustment are not modelled;
/// callers refuse a Sony block.)
pub(crate) fn exiftool_preview_position(file: &[u8], scan_from: usize) -> Option<usize> {
    const CHUNK: usize = 65536;
    const SCAN_LEN: usize = 1024;
    let eoi = scan_from
        + file
            .get(scan_from..)?
            .windows(2)
            .position(|pair| pair == [0xFF, 0xD9])?;
    let end = eoi + 2;
    // `$buff` is the rest of the chunk the EOI's second byte was read in
    // (a chunk ending in 0xFF is joined to a 0xD9 opening the next) ...
    let chunk_end = scan_from + ((eoi + 1 - scan_from) / CHUNK + 1) * CHUNK;
    let rest_end = chunk_end.min(file.len());
    // ... topped up to `$scanLen` bytes when shorter.
    let buffered_end = if rest_end - end < SCAN_LEN {
        (end + SCAN_LEN).min(file.len())
    } else {
        rest_end
    };
    // `/(\xff\xd8\xff.|.\xd8\xff\xdb)(..)/sg`; `$junkLen = pos($buff) - 6`
    // is where the match starts.
    let junk = file[end..buffered_end]
        .windows(6)
        .position(|w| w[..3] == [0xFF, 0xD8, 0xFF] || w[1..4] == [0xD8, 0xFF, 0xDB])
        .unwrap_or(0);
    Some(end + junk)
}

/// Point every [`TailPointer`] of `output` where its target now is.
///
/// `original[moved_from..]` must be the verbatim tail of `output`; every
/// position at or after `moved_from` in `original` sits at `position +
/// delta` in `output`. A pointer whose target lies before `moved_from`, whose
/// value would not fit its field, or whose moved bytes are not found at the
/// new position refuses the write.
pub(crate) fn repoint_tail(
    original: &[u8],
    moved_from: usize,
    scan_from: usize,
    mut output: Vec<u8>,
    pointers: &[TailPointer],
) -> Result<Vec<u8>> {
    if pointers.is_empty() {
        return Ok(output);
    }
    let tail = original
        .get(moved_from..)
        .ok_or_else(|| refuse("edited region ends past the file"))?;
    if !output.ends_with(tail) {
        return Err(refuse(
            "the bytes after the edited region were not copied verbatim",
        ));
    }
    let delta = output.len() as i128 - original.len() as i128;
    let to_output = |position: usize| {
        usize::try_from(position as i128 + delta)
            .map_err(|_| refuse(format!("position {position} moved before the file's start")))
    };
    let mut patches = Vec::with_capacity(pointers.len());
    for pointer in pointers {
        let absolute = match pointer.target {
            TailTarget::Moved { from, len } => {
                if from < moved_from {
                    return Err(refuse(format!(
                        "the data at {from} lies in the part of the file this write rewrote"
                    )));
                }
                let to = to_output(from)?;
                if output.get(to..to.saturating_add(len)) != original.get(from..from + len) {
                    return Err(refuse(format!(
                        "the {len} bytes at {from} are not at {to} after the write"
                    )));
                }
                to
            }
            TailTarget::AfterEoi => {
                if scan_from < moved_from {
                    return Err(refuse(
                        "the image data lies in the part of the file this write rewrote",
                    ));
                }
                exiftool_preview_position(&output, to_output(scan_from)?)
                    .ok_or_else(|| refuse("the image has no EOI"))?
            }
        };
        let value = absolute
            .checked_sub(pointer.base)
            .filter(|v| {
                if pointer.wide {
                    u32::try_from(*v).is_ok()
                } else {
                    u16::try_from(*v).is_ok()
                }
            })
            .ok_or_else(|| {
                refuse(format!(
                    "position {absolute} does not fit a {}-bit offset from {}",
                    if pointer.wide { 32 } else { 16 },
                    pointer.base
                ))
            })?;
        patches.push((pointer, value as u32));
    }
    for (pointer, value) in patches {
        if pointer.wide {
            write_u32(&mut output, pointer.field_at, value, pointer.big_endian);
        } else {
            let raw = if pointer.big_endian {
                (value as u16).to_be_bytes()
            } else {
                (value as u16).to_le_bytes()
            };
            output[pointer.field_at..pointer.field_at + 2].copy_from_slice(&raw);
        }
    }
    Ok(output)
}

/// Re-base every offset a JPEG writer's verbatim copy of the tail moved:
/// the absolute offsets of every AFCP trailer, and a Leica IFD2
/// PreviewImage pointer ([`super::ifd_chain::preview_tail_pointer`]), all
/// through [`repoint_tail`].
///
/// `original[moved_from..]` must be the verbatim tail of `output` (the bytes
/// after the edited region); `scan_from` is where the entropy-coded data
/// starts (the end of the first SOS header, or the EOI marker itself when the
/// file has no scan), from which the EOI is found. A write that does not
/// change the file's length leaves every AFCP trailer valid; the IFD2
/// preview is re-pointed on every write, as ExifTool re-points it.
pub(crate) fn rebase_trailer_offsets(
    original: &[u8],
    moved_from: usize,
    scan_from: usize,
    output: Vec<u8>,
) -> Result<Vec<u8>> {
    let mut pointers: Vec<TailPointer> =
        crate::writers::ifd_chain::preview_tail_pointer(original, &output)?
            .into_iter()
            .collect();
    if output.len() != original.len() {
        pointers.extend(afcp_pointers(original, moved_from, scan_from, &output)?);
    }
    repoint_tail(original, moved_from, scan_from, output, &pointers)
}

/// The [`TailPointer`]s of every AFCP trailer the write moved: each
/// trailer's start pointer (absolute, at its header) and entry offsets
/// (absolute, at their data).
fn afcp_pointers(
    original: &[u8],
    moved_from: usize,
    scan_from: usize,
    output: &[u8],
) -> Result<Vec<TailPointer>> {
    let tail = original
        .get(moved_from..)
        .ok_or_else(|| refuse("edited region ends past the file"))?;
    if !output.ends_with(tail) {
        return Err(refuse(
            "the bytes after the edited region were not copied verbatim",
        ));
    }
    let trailers = locate_trailers(original, moved_from, scan_from)?;
    let delta = output.len() as i128 - original.len() as i128;
    let to_output = |position: usize| (position as i128 + delta) as usize;
    let mut pointers = Vec::new();
    for trailer in &trailers {
        let pointer = |field: usize, from: usize, len: usize| TailPointer {
            field_at: to_output(field),
            wide: true,
            big_endian: trailer.big_endian,
            base: 0,
            target: TailTarget::Moved { from, len },
        };
        pointers.push(pointer(trailer.eof_record + 4, trailer.start, 12));
        for &(field, offset) in &trailer.entry_offsets {
            let size = read_u32(original, field - 4, trailer.big_endian).unwrap_or(0) as usize;
            pointers.push(pointer(field, offset as usize, size));
        }
    }
    Ok(pointers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SOI, SOS header, two scan bytes, EOI: the smallest shape with a
    /// trailer region. Its scan data starts at [`SCAN_FROM`].
    fn body() -> Vec<u8> {
        vec![
            0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x04, 0x01, 0x00, 0x12, 0x34, 0xFF, 0xD9,
        ]
    }
    const SCAN_FROM: usize = 8;

    /// An AFCP trailer placed at `start` holding `entries` (tag, data).
    fn afcp(start: usize, big_endian: bool, entries: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let u16b = |v: u16| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let u32b = |v: u32| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let sig: &[u8] = if big_endian { b"AXS!" } else { b"AXS*" };
        let mut out = sig.to_vec();
        out.extend_from_slice(&u16b(1));
        out.extend_from_slice(&u16b(entries.len() as u16));
        out.extend_from_slice(&u32b(0));
        let mut data_at = start + 12 + 12 * entries.len();
        let mut data = Vec::new();
        for (tag, value) in entries {
            out.extend_from_slice(tag.as_slice());
            out.extend_from_slice(&u32b(value.len() as u32));
            out.extend_from_slice(&u32b(data_at as u32));
            data.extend_from_slice(value);
            data_at += value.len();
        }
        out.extend_from_slice(&data);
        out.extend_from_slice(sig);
        out.extend_from_slice(&u32b(start as u32));
        out.extend_from_slice(&u32b(0));
        out
    }

    /// `original` with `grow` bytes inserted (or `-grow` removed) at 2.
    fn edited(original: &[u8], grow: isize) -> Vec<u8> {
        let mut out = original[..2].to_vec();
        if grow >= 0 {
            out.extend(std::iter::repeat_n(0xAA, grow as usize));
            out.extend_from_slice(&original[2..]);
        } else {
            out.extend_from_slice(&original[2 + (-grow) as usize..]);
        }
        out
    }

    fn file_with(trailer_for: impl Fn(usize) -> Vec<u8>, padding: usize) -> Vec<u8> {
        let mut file = body();
        file.splice(2..2, std::iter::repeat_n(0xEE, padding));
        let start = file.len();
        file.extend(trailer_for(start));
        file
    }

    #[test]
    fn shifts_every_absolute_offset_by_the_length_delta() {
        for big_endian in [true, false] {
            let original = file_with(
                |at| {
                    afcp(
                        at,
                        big_endian,
                        &[(b"IPTC", b"iptc-data"), (b"TEXT", b"hello")],
                    )
                },
                8,
            );
            for grow in [5isize, -3] {
                let output = edited(&original, grow);
                let expected = output.clone();
                // The edit replaced bytes at 2; what follows was copied.
                let moved_from = 2 + (-grow).max(0) as usize;
                let fixed =
                    rebase_trailer_offsets(&original, moved_from, SCAN_FROM + 8, output).unwrap();
                let start = body().len() + 8;
                let moved = (start as isize + grow) as usize;
                let rebuilt = afcp(
                    moved,
                    big_endian,
                    &[(b"IPTC", b"iptc-data"), (b"TEXT", b"hello")],
                );
                assert_eq!(&fixed[moved..], rebuilt.as_slice(), "grow {grow}");
                assert_eq!(&fixed[..moved], &expected[..moved], "grow {grow}");
            }
        }
    }

    #[test]
    fn same_length_write_is_returned_untouched_even_with_stale_offsets() {
        // Stale: the end-of-file record points 7 bytes past the header.
        let original = file_with(|at| afcp(at + 7, true, &[(b"TEXT", b"x")]), 0);
        let output = original.clone();
        assert_eq!(
            rebase_trailer_offsets(&original, 2, SCAN_FROM, output.clone()).unwrap(),
            output
        );
    }

    #[test]
    fn refuses_a_trailer_whose_start_pointer_is_already_stale() {
        let original = file_with(|at| afcp(at + 7, true, &[(b"TEXT", b"x")]), 0);
        let error = rebase_trailer_offsets(&original, 2, SCAN_FROM, edited(&original, 4))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not part of a well-formed AFCP trailer"),
            "{error}"
        );
    }

    #[test]
    fn refuses_a_non_zero_checksum() {
        let mut original = file_with(|at| afcp(at, true, &[(b"TEXT", b"x")]), 0);
        let len = original.len();
        original[len - 1] = 1;
        let error = rebase_trailer_offsets(&original, 2, SCAN_FROM, edited(&original, 4))
            .unwrap_err()
            .to_string();
        assert!(error.contains("checksum"), "{error}");
    }

    #[test]
    fn refuses_an_entry_outside_the_trailer() {
        let mut original = file_with(|at| afcp(at, true, &[(b"TEXT", b"x")]), 0);
        let start = body().len();
        // Point the entry at the JPEG body.
        original[start + 12 + 8..start + 12 + 12].copy_from_slice(&3u32.to_be_bytes());
        let error = rebase_trailer_offsets(&original, 2, SCAN_FROM, edited(&original, 4))
            .unwrap_err()
            .to_string();
        assert!(error.contains("outside the trailer data"), "{error}");
    }

    #[test]
    fn refuses_a_stray_signature_after_the_eoi() {
        let mut original = body();
        original.extend_from_slice(b"junkAXS*junk");
        let error = rebase_trailer_offsets(&original, 2, SCAN_FROM, edited(&original, 4))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not part of a well-formed AFCP trailer"),
            "{error}"
        );
    }

    #[test]
    fn ignores_signatures_before_the_eoi_and_inside_trailer_data() {
        let mut original = body();
        // "AXS!" in the scan data is not a trailer.
        original.splice(8..8, b"AXS!".iter().copied());
        let start = original.len();
        original.extend(afcp(start, true, &[(b"TEXT", b"has AXS! inside")]));
        let fixed =
            rebase_trailer_offsets(&original, 2, 8, edited(&original, 6)).expect("re-based");
        assert_eq!(
            &fixed[start + 6..],
            afcp(start + 6, true, &[(b"TEXT", b"has AXS! inside")]).as_slice()
        );
    }

    #[test]
    fn a_file_without_afcp_is_returned_untouched() {
        let mut original = body();
        original.extend_from_slice(b"some other trailer\xa1\xb2\xc3\xd4");
        let output = edited(&original, 9);
        assert_eq!(
            rebase_trailer_offsets(&original, 2, SCAN_FROM, output.clone()).unwrap(),
            output
        );
    }

    #[test]
    fn refuses_when_the_tail_was_not_copied_verbatim() {
        let original = file_with(|at| afcp(at, true, &[(b"TEXT", b"x")]), 0);
        let mut output = edited(&original, 4);
        let last = output.len() - 5;
        output[last] ^= 0xFF;
        let error = rebase_trailer_offsets(&original, 2, SCAN_FROM, output)
            .unwrap_err()
            .to_string();
        assert!(error.contains("verbatim"), "{error}");
    }
}
