//! Canon MakerNote parser
//!
//! Parses Canon-specific EXIF MakerNote tags containing camera settings,
//! lens information, focus data, and other proprietary metadata.

#![allow(dead_code)]
#![allow(unused_imports)]

// Submodules for extended tag parsing
pub mod binary_tables;
pub mod camera_info;
pub mod camera_info_tables;
pub mod color_data;
mod custom_functions2;
mod custom_functions2_tables;
pub mod filter_info;

use crate::core::formatters::perl_number as format_perl_number;
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::shared::ifd_parser_base::{
    IfdParserConfig, parse_ifd_entries, resolve_makernote_byte_order,
};
use nom::{
    IResult,
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
};
use std::collections::HashMap;

use super::canon_lens_database::lookup_lens_name;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_i16_array;
use super::shared::value_extractors::{extract_inline_value, extract_integer_value};
use crate::bitfield_decoder;
use crate::const_decoder;
pub(super) use crate::core::formatters::exif_print_conv::print_exposure_time;

/// Canon-specific i16 array extractor that handles UNDEFINED (7) field type.
/// Canon MakerNotes often store i16 arrays with field_type 7 (UNDEFINED) instead of 3 (SHORT).
/// This function accepts both types while the standard extract_i16_array only accepts SHORT.
///
/// The `base_offset` parameter is the TIFF offset where the MakerNote data starts.
/// Canon MakerNote value_offsets are TIFF-relative, so we need to subtract the base
/// to get the position within the data slice.
fn extract_canon_i16_array_with_base(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
    base_offset: u32,
) -> Option<Vec<i16>> {
    // Accept both SHORT (3) and UNDEFINED (7) field types
    // Canon stores CameraSettings, ShotInfo, etc. as UNDEFINED but they contain i16 arrays
    if entry.field_type != 3 && entry.field_type != 7 {
        return None;
    }

    if entry.value_count == 0 {
        return None;
    }

    // For UNDEFINED type, value_count is byte count, not element count
    // For SHORT type, value_count is element count
    let (count, bytes_needed) = if entry.field_type == 7 {
        // UNDEFINED: value_count is bytes, so elements = bytes / 2
        let byte_count = entry.value_count as usize;
        (byte_count / 2, byte_count)
    } else {
        // SHORT: value_count is elements
        let element_count = entry.value_count as usize;
        (element_count, element_count * 2)
    };

    if count == 0 {
        return None;
    }

    // Inline: ≤2 shorts fit in 4-byte value_offset field
    if bytes_needed <= 4 {
        let mut result = Vec::with_capacity(count);
        let bytes = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };

        let reader = EndianReader::new(&bytes, byte_order.to_io_byte_order());
        for i in 0..count {
            if let Some(value) = reader.i16_at(i * 2) {
                result.push(value);
            }
        }
        return Some(result);
    }

    // Offset-based: Canon MakerNote offsets are TIFF-relative
    // Adjust by subtracting the MakerNote base offset to get position in data slice
    let tiff_offset = entry.value_offset;
    if tiff_offset < base_offset {
        return None; // Offset is before MakerNote start, invalid
    }
    let relative_offset = (tiff_offset - base_offset) as usize;

    if relative_offset + bytes_needed > data.len() {
        return None;
    }

    let array_data = &data[relative_offset..relative_offset + bytes_needed];
    let reader = EndianReader::new(array_data, byte_order.to_io_byte_order());
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        if let Some(value) = reader.i16_at(i * 2) {
            result.push(value);
        }
    }
    Some(result)
}

/// The base recorded in a Canon MakerNote's own TIFF footer, if it has one.
///
/// # What the footer is
///
/// Canon closes most of its MakerNotes with an 8-byte TIFF footer: a TIFF magic
/// (`II\x2a\0` or `MM\0\x2a`) followed by the 32-bit offset the directory was
/// written at. When an editor moves the EXIF block without rewriting the
/// MakerNote's internal offsets -- which is what every one of the 38 corpus
/// JPEGs ExifTool re-bases has had done to it -- that recorded offset is still
/// the origin those offsets are measured from, and the footer hands it over
/// exactly. ExifTool reads it first thing in `FixBase` (MakerNotes.pm:1306-1338):
///
/// ```text
///     if ($$et{Make} =~ /^Canon/ and $$dirInfo{DirLen} > 8) {
///         my $footerPos = $dirStart + $$dirInfo{DirLen} - 8;
///         my $footer = substr($$dataPt, $footerPos, 8);
///         if ($footer =~ /^(II\x2a\0|MM\0\x2a)/ and  # check for TIFF footer
///             substr($footer,0,2) eq GetByteOrder()) # validate byte ordering
///         {
///             my $oldOffset = Get32u(\$footer, 4);
///             my $newOffset = $dirStart + $dataPos;
///             ...
///             $fix = $newOffset - $oldOffset;
///             ...
///             $$dirInfo{Base} += $fix;
///             $$dirInfo{DataPos} -= $fix;
/// ```
///
/// `ProcessExif` resolves a value at `$valuePtr - $dataPos` (Exif.pm:6447), so
/// shifting `DataPos` down by `$fix = dirStart - oldOffset` is exactly the same
/// as subtracting `oldOffset` instead of `dirStart`. The footer's number *is*
/// the base.
///
/// # Measured
///
/// 547 of the 670 Canon MakerNotes in the ExifTool sample corpus carry a valid
/// footer, and the value it records reproduces every one of ExifTool's 38
/// `Adjusted MakerNotes base by N` warnings to the byte -- `-294` on
/// `CanonDC210.jpg`, `+2` on `CanonPowerShotSX130IS.jpg`, `+4214` on
/// `CanonPowerShotSX600HS.jpg`.
///
/// The byte-order check is not decoration: the footer is read in the
/// *MakerNote's* order, which on 24 of those 38 files is the opposite of the
/// enclosing TIFF's.
fn canon_footer_base(payload: &[u8], byte_order: ByteOrder) -> Option<u32> {
    // ExifTool: `$$dirInfo{DirLen} > 8`
    if payload.len() <= 8 {
        return None;
    }
    let footer = &payload[payload.len() - 8..];
    // ExifTool: `$footer =~ /^(II\x2a\0|MM\0\x2a)/ and substr($footer,0,2) eq GetByteOrder()`
    // -- one test, since the magic and the order have to agree with each other
    // and with the order the directory is being read in.
    let expected: &[u8] = match byte_order {
        ByteOrder::LittleEndian => b"II\x2a\x00",
        ByteOrder::BigEndian => b"MM\x00\x2a",
    };
    if &footer[..4] != expected {
        return None;
    }
    EndianReader::new(footer, byte_order.to_io_byte_order()).u32_at(4)
}

/// ExifTool's test for a Canon TIFF footer that lies.
///
/// Picasa and ACDSee update a MakerNote's value offsets without updating the
/// footer, so ExifTool validates the footer against where the last value
/// actually ends before trusting it (MakerNotes.pm:1319-1330):
///
/// ```text
///     # Picasa and ACDSee have a bug where they update other offsets without
///     # updating the TIFF footer (PH - 2009/02/25), so test for this case:
///     # validate Canon maker note footer fix by checking offset of last value
///     my $maxPt = $valPtrs[-1] + $$valBlock{$valPtrs[-1]};
///     # compare to end of maker notes, taking 8-byte footer into account
///     my $endDiff = $dirStart + $$dirInfo{DirLen} - ($maxPt - $dataPos) - 8;
///     # ignore footer offset only if end difference is exactly correct
///     # (allow for possible padding byte, although I have never seen this)
///     if (not $endDiff or $endDiff == 1) {
///         $et->Warn('Canon maker note footer may be invalid (ignored)',1);
///         return 0;
///     }
/// ```
///
/// `$maxPt` is the end of the last value in the coordinates the entries were
/// *written* in, and `$dirStart + $$dirInfo{DirLen}` the end of the directory
/// where it sits *now*; the two agreeing means the offsets were rewritten for
/// the new position and the footer was left behind. `$dataPos` is 0 for a
/// MakerNote inside a loaded EXIF block, so this is
/// `dir_tiff_offset + payload.len() - max_value_end - 8`.
///
/// `$valPtrs[-1]` is the *highest* value offset and `$$valBlock{...}` the longest
/// block recorded at it, so this takes the size of the entry at the largest
/// offset rather than the largest `offset + size`; the two differ when a long
/// value starts below a short one.
///
/// Returns `true` when the footer must be ignored. Needs the directory's real
/// position, so a caller holding a detached payload cannot run it -- see
/// [`canon_makernote_base_located`].
fn canon_footer_is_stale(
    offset_entries: &[(u32, usize)],
    dir_tiff_offset: u32,
    dir_len: usize,
) -> bool {
    let Some(&last_offset) = offset_entries.iter().map(|(offset, _)| offset).max() else {
        return false;
    };
    // ExifTool's %valBlock keeps the longest block at each offset:
    // `unless (defined $valBlock{$valPtr} and $valBlock{$valPtr} > $size)`.
    let last_size = offset_entries
        .iter()
        .filter(|&&(offset, _)| offset == last_offset)
        .map(|&(_, size)| size)
        .max()
        .unwrap_or(0);
    let max_pt = last_offset as i64 + last_size as i64;
    let end_diff = dir_tiff_offset as i64 + dir_len as i64 - max_pt - 8;
    end_diff == 0 || end_diff == 1
}

/// Widest gap, in bytes, between the end of the MakerNote IFD header and the
/// first byte of value data that [`calculate_makernote_base`] will consider.
///
/// Measured over the 644 Canon JPEGs in the ExifTool sample corpus that carry a
/// Canon MakerNote, the real gap is 0 on 544 files, 2 on 6, 12 on 64, 24 on 18,
/// and never exceeds 350. 1024 covers every observed layout with room to spare
/// while keeping the search to at most 1024 candidates evaluated once per file.
const MAKERNOTE_BASE_SEARCH_SPAN: u32 = 1024;

/// How many records must independently confirm a candidate base before it is
/// preferred over the packed-layout assumption.
///
/// One agreement can happen by chance -- any 16-bit slot in the MakerNote could
/// hold a value that happens to equal some record's byte count. Two independent
/// records agreeing on the *same* base is already vanishingly unlikely, and the
/// corpus check below shows the true base typically collects 10-30 votes.
const MAKERNOTE_BASE_MIN_VOTES: u32 = 2;

/// Every IFD entry whose value lives outside the 4-byte inline slot, as
/// `(value_offset, declared byte size)`, paired with the IFD header's size.
///
/// This is ExifTool's `GetValueBlocks` (MakerNotes.pm:1241-1275): the same
/// `$size <= 4 -> next` skip and nothing else, over the same standard TIFF entry
/// layout `[tag_id:2][field_type:2][value_count:4][value_offset:4]`. Callers
/// that want only the entries which can anchor a base apply their own filter --
/// [`calculate_makernote_base`] does, [`canon_footer_is_stale`] must not, since
/// ExifTool's own staleness test runs over the unfiltered blocks.
fn canon_offset_entries(data: &[u8], byte_order: ByteOrder) -> Option<(Vec<(u32, usize)>, usize)> {
    if data.len() < 2 {
        return None;
    }

    let reader = EndianReader::new(data, byte_order.to_io_byte_order());
    let entry_count = reader.u16_at(0)? as usize;

    if entry_count == 0 || entry_count > 100 {
        return None;
    }

    // Calculate IFD header size: 2 bytes (entry count) + 12 bytes per entry + 4 bytes (next IFD pointer)
    let header_size = 2 + entry_count * 12 + 4;

    if header_size > data.len() {
        return None;
    }

    let mut offset_entries: Vec<(u32, usize)> = Vec::new();
    for i in 0..entry_count {
        let entry_offset = 2 + i * 12;
        if entry_offset + 12 > data.len() {
            break;
        }

        let field_type = reader.u16_at(entry_offset + 2)?;
        let value_count = reader.u32_at(entry_offset + 4)?;
        let value_offset = reader.u32_at(entry_offset + 8)?;

        // Reference: TIFF specification field types
        let type_size = match field_type {
            1 => 1,        // BYTE
            2 => 1,        // ASCII
            3 => 2,        // SHORT
            4 => 4,        // LONG
            5 => 8,        // RATIONAL
            6 => 1,        // SBYTE
            7 => 1,        // UNDEFINED
            8 => 2,        // SSHORT
            9 => 4,        // SLONG
            10 => 8,       // SRATIONAL
            11 => 4,       // FLOAT
            12 => 8,       // DOUBLE
            _ => continue, // Unknown type, skip
        };

        let total_size = type_size * value_count as usize;

        // ExifTool: `next if $size <= 4;`
        if total_size > 4 {
            offset_entries.push((value_offset, total_size));
        }
    }

    Some((offset_entries, header_size))
}

/// Calculates the MakerNote base offset by examining the IFD structure.
/// The base offset is needed to convert TIFF-relative value_offsets to positions
/// within the MakerNote data slice.
///
/// Canon MakerNotes use TIFF-relative offsets, meaning the value_offset field
/// in each IFD entry contains an offset from the start of the entire TIFF file,
/// not from the start of the MakerNote. To correctly extract values, we need
/// to calculate: `position_in_slice = value_offset - base_offset`.
///
/// This is the fallback for a caller that hands the parser a **detached**
/// MakerNote slice with no record of where it sat in the file, so the base has
/// to be recovered from the MakerNote's own bytes. A caller that knows the
/// position goes through [`canon_makernote_base_located`] instead, which reads
/// the base out of the file rather than inferring it and is right on the 31
/// corpus files where this vote is not.
///
/// # Why the packed-layout guess is not enough
///
/// The obvious derivation -- assume the first value follows the IFD header with
/// no gap, so `base = min(value_offset) - (2 + 12 * entries + 4)` -- is what this
/// function used to do, and it is wrong whenever Canon leaves padding between the
/// header and the value block. Measured across the 644 Canon JPEGs in the
/// ExifTool sample corpus that carry a Canon MakerNote, it is wrong on 100 of
/// them: 64 files off by 12 bytes, 18 by 24, 6 by 2, and 12 by larger amounts.
///
/// An off-by-N base shifts every offset-based record by N bytes, which is far
/// worse than a missing tag because the tags still come out -- just wrong. On
/// `CanonDIGITAL_IXUS100IS.jpg` (12 bytes low) the old code reported
/// `OwnerName "sion 1.00"` for `""`, `FocalUnits "16390/mm"` for `1000/mm`,
/// `AESetting "Unknown (256)"` for `Normal AE`, and a 100-digit `MinAperture`.
///
/// # How the base is recovered instead
///
/// Canon binary records are self-describing: every `%Canon` table with
/// `FIRST_ENTRY => 1` opens with its own size in bytes, which ExifTool spells out
/// at Canon.pm:7444 -- `# 0x00: size of record in bytes`. So for the correct base
/// `B`, a record whose IFD entry declares `size` bytes at `value_offset` satisfies
///
/// ```text
///     u16(data[value_offset - B]) == size
/// ```
///
/// Each offset-based entry is therefore a vote for one candidate base, and the
/// base the most records agree on is the base. Against the corpus this is not a
/// marginal signal: it holds for 98-100% of the entries of tags 0x0001, 0x0004,
/// 0x0026, 0x001D, 0x001F, 0x0022, 0x0023, 0x0035, 0x0093, 0x00A0, 0x00AA,
/// 0x00E0, 0x0099 and the whole 0x40xx block. No table of "which tags are
/// self-describing" is needed -- entries that are not simply never vote.
///
/// Candidates are bounded below by the requirement that the value block fit
/// inside the slice, and above by the requirement that it not overlap the IFD
/// header (`min(value_offset) - header_size`, i.e. the old guess). Ties keep the
/// largest base, so a correctly packed MakerNote still resolves to the old answer,
/// and a MakerNote with too little self-describing evidence falls back to it
/// outright. Corpus result: 544 -> 626 files given the right base, with no file
/// moved off a base that was already right.
fn calculate_makernote_base(data: &[u8], byte_order: ByteOrder) -> Option<u32> {
    let reader = EndianReader::new(data, byte_order.to_io_byte_order());
    let (all_entries, header_size) = canon_offset_entries(data, byte_order)?;
    // Only an entry whose value sits past the IFD header can anchor a base; a
    // zero or header-overlapping offset is a garbage pointer, not evidence.
    let offset_entries: Vec<(u32, usize)> = all_entries
        .into_iter()
        .filter(|&(offset, _)| offset > 0 && offset as usize >= header_size)
        .collect();

    // No offset-based entries: nothing to anchor a base on (callers fall back to 0).
    let lowest_value_offset = offset_entries.iter().map(|&(offset, _)| offset).min()?;

    // Upper bound: any larger base would place the first value inside the IFD
    // header. This is exactly the old packed-layout guess, and stays the answer
    // whenever the records offer no better evidence.
    let packed_base = lowest_value_offset - header_size as u32;

    // Lower bound: any smaller base would run the last value past the end of the
    // slice. Clamped to the search span so the scan is bounded regardless of what
    // a corrupt entry claims.
    let furthest_end = offset_entries
        .iter()
        .map(|&(offset, size)| offset as u64 + size as u64)
        .max()
        .unwrap_or(0);
    let fits_in_slice = furthest_end.saturating_sub(data.len() as u64);
    let lowest_base = u32::try_from(fits_in_slice)
        .unwrap_or(u32::MAX)
        .max(packed_base.saturating_sub(MAKERNOTE_BASE_SEARCH_SPAN))
        .min(packed_base);

    // Score each candidate by how many records validate their own declared size
    // at it, walking down from the packed base so that ties keep the largest.
    let mut best_votes = 0u32;
    let mut best_base = packed_base;
    let mut candidate = packed_base;
    loop {
        let mut votes = 0u32;
        for &(value_offset, total_size) in &offset_entries {
            // A record longer than u16::MAX cannot state its own size in the
            // 16-bit leading slot, so it never votes.
            let Ok(declared) = u16::try_from(total_size) else {
                continue;
            };
            let Some(position) = value_offset.checked_sub(candidate) else {
                continue;
            };
            let position = position as usize;
            if position < header_size || position + 2 > data.len() {
                continue;
            }
            if reader.u16_at(position) == Some(declared) {
                votes += 1;
            }
        }
        if votes > best_votes {
            best_votes = votes;
            best_base = candidate;
        }
        if candidate == lowest_base {
            break;
        }
        candidate -= 1;
    }

    if best_votes >= MAKERNOTE_BASE_MIN_VOTES {
        Some(best_base)
    } else {
        Some(packed_base)
    }
}

/// The base for a MakerNote whose position inside its TIFF block is known.
///
/// This is what ExifTool does, and it needs no guessing at all. A Canon
/// MakerNote entry's value offset is measured from the TIFF header, and
/// `ProcessExif` turns it into an index with `$valuePtr -= $dataPos`
/// (Exif.pm:6447), so the number subtracted is the directory's own offset --
/// `dir_tiff_offset` -- unless `FixBase` moved it. For Canon the only thing that
/// moves it is the TIFF footer ([`canon_footer_base`]), which records the offset
/// the directory's own offsets were written against.
///
/// So there are exactly two answers, and both are read out of the file rather
/// than inferred:
///
/// * a valid, non-stale footer -> the offset it records;
/// * otherwise -> where the directory actually sits.
///
/// # Why the vote is not used here
///
/// [`calculate_makernote_base`] recovers a base by majority vote among records
/// that declare their own size, because a decoder handed a detached payload has
/// nothing else to go on. Measured over the 670 Canon MakerNotes in the ExifTool
/// sample corpus, the vote reaches the same answer as this function on 639 and
/// differs on 31 -- and on all 31 it is the vote that is wrong:
///
/// * 25 camcorder samples (`CanonMVX*`, `CanonOPTURA*`, `CanonZR65MC`,
///   `CanonIXY_DV*`) have no record that declares its own size, so the vote
///   produces nothing and the caller falls back to a base of 0;
/// * 5 (`CanonFV_M30`, `CanonMVX40`, `CanonMVX45i`, `CanonOPTURA50`,
///   `CanonOPTURA60`) land 24 bytes high -- these are the bodies ExifTool
///   singles out at MakerNotes.pm:1162-1164, "some Canon models (FV-M30,
///   Optura50, Optura60) leave 24 unused bytes at the end of the IFD";
/// * `CanonHG20.jpg` votes 605 where its own footer records 856.
///
/// The vote therefore stays as the fallback for a detached payload, and nothing
/// more.
fn canon_makernote_base_located(
    data: &[u8],
    declared: &[u8],
    byte_order: ByteOrder,
    dir_tiff_offset: u32,
) -> u32 {
    if let Some(footer_base) = canon_footer_base(declared, byte_order) {
        let stale = canon_offset_entries(data, byte_order)
            .map(|(entries, _)| canon_footer_is_stale(&entries, dir_tiff_offset, declared.len()))
            .unwrap_or(false);
        if !stale {
            return footer_base;
        }
    }
    dir_tiff_offset
}

/// Returns an IFD entry's value as raw bytes, resolved against the MakerNote base.
///
/// [`extract_canon_i16_array_with_base`] reinterprets everything as `int16`, which is
/// wrong for the records ExifTool declares as `int32` (`CustomFunctions2` is stored as
/// TIFF LONG). This hands back the untyped bytes so the caller can decode them with the
/// width its own table specifies.
fn extract_canon_bytes_with_base<'a>(
    entry: &IfdEntry,
    data: &'a [u8],
    base_offset: u32,
) -> Option<&'a [u8]> {
    let type_size: usize = match entry.field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    };
    let byte_count = type_size.checked_mul(entry.value_count as usize)?;
    if byte_count == 0 || byte_count <= 4 {
        // Values of 4 bytes or fewer live inline in the offset slot, not in the data area.
        return None;
    }
    let relative_offset = entry.value_offset.checked_sub(base_offset)? as usize;
    data.get(relative_offset..relative_offset.checked_add(byte_count)?)
}

/// Returns a Canon BinaryData record as the 16-bit-word view used by
/// [`binary_tables`].
///
/// The older array extractor deliberately accepts only TIFF SHORT and UNDEFINED.
/// Canon's tables whose ExifTool `FORMAT` is `int32*` are stored as TIFF LONG/SLONG,
/// though, and were therefore skipped before their format-aware decoder could run.
/// Keep that broader interpretation local to the generic BinaryData path instead of
/// changing every legacy i16-array caller's accepted TIFF types.
fn extract_canon_binary_words_with_base(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
    base_offset: u32,
) -> Option<Vec<i16>> {
    if matches!(entry.field_type, 3 | 7) {
        return extract_canon_i16_array_with_base(entry, data, byte_order, base_offset);
    }
    if !matches!(entry.field_type, 4 | 9) {
        return None;
    }

    let bytes = extract_canon_bytes_with_base(entry, data, base_offset)?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let reader = EndianReader::new(bytes, byte_order.to_io_byte_order());
    (0..bytes.len() / 2)
        .map(|index| reader.i16_at(index * 2))
        .collect()
}

/// Resolves the MakerNote base once, on the same post-signature slice that
/// [`parse_ifd_entries`] hands to its entry callbacks.
///
/// `dir_tiff_offset` is where that slice sits inside the enclosing TIFF block,
/// when the caller knows -- then the base is read out of the file by
/// [`canon_makernote_base_located`] rather than inferred. A caller holding a
/// detached payload passes `None` and gets [`calculate_makernote_base`]'s vote,
/// which scans up to [`MAKERNOTE_BASE_SEARCH_SPAN`] candidates and is therefore
/// resolved once per MakerNote and threaded through the extractors rather than
/// recomputed per entry. A MakerNote with no offset-based entry to anchor on
/// keeps the historical fallback of treating value offsets as slice-relative.
fn canon_makernote_base(
    data: &[u8],
    declared: &[u8],
    byte_order: ByteOrder,
    config: &IfdParserConfig,
    dir_tiff_offset: Option<u32>,
) -> u32 {
    let start_offset = match config.signature {
        Some(sig) if data.len() >= sig.len() && &data[..sig.len()] == sig => {
            config.signature_offset
        }
        _ => 0,
    };
    if start_offset >= data.len() {
        return 0;
    }
    match dir_tiff_offset {
        // The IFD begins `start_offset` bytes into the payload, so that is where
        // it sits in the block -- and what a TIFF-relative value offset has to
        // be measured against. The footer is looked for in the *declared* block,
        // since that is where its last eight bytes are.
        Some(offset) => canon_makernote_base_located(
            &data[start_offset..],
            declared.get(start_offset..).unwrap_or(declared),
            byte_order,
            offset.saturating_add(start_offset as u32),
        ),
        None => calculate_makernote_base(&data[start_offset..], byte_order).unwrap_or(0),
    }
}

/// Legacy wrapper for extract_canon_i16_array without base offset (for test compatibility)
#[allow(dead_code)]
fn extract_canon_i16_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<i16>> {
    // For legacy calls, try to calculate base offset
    if let Some(base) = calculate_makernote_base(data, byte_order) {
        extract_canon_i16_array_with_base(entry, data, byte_order, base)
    } else {
        // Fallback: assume offsets are relative to data slice (original behavior)
        extract_canon_i16_array_with_base(entry, data, byte_order, 0)
    }
}

/// Extracts a string value from a Canon MakerNote IFD entry.
///
/// Canon MakerNotes use TIFF-relative offsets, so we need to calculate the
/// base offset and subtract it from the value_offset to get the position
/// within the MakerNote data slice.
///
/// # Parameters
/// - `entry`: The IFD entry containing the tag metadata
/// - `data`: The MakerNote data slice (after any signature)
/// - `byte_order`: Byte order for parsing
/// - `base_offset`: The calculated base offset to subtract from value_offset
///
/// # Returns
/// The extracted string value, or None if extraction fails
fn extract_canon_string_with_base(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
    base_offset: u32,
) -> Option<String> {
    // Only handle ASCII type (2)
    if entry.field_type != 2 {
        return None;
    }

    let byte_count = entry.value_count as usize;
    if byte_count == 0 {
        return None;
    }

    // For inline strings (<=4 bytes), value is stored in value_offset field
    if byte_count <= 4 {
        let bytes = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };
        let inline = &bytes[..byte_count];
        let terminated = match inline.iter().position(|&b| b == 0) {
            Some(nul) => &inline[..nul],
            None => inline,
        };
        return Some(String::from_utf8_lossy(terminated).to_string());
    }

    // For offset-based strings, adjust the offset using base_offset
    let tiff_offset = entry.value_offset;
    if tiff_offset < base_offset {
        // Offset is before MakerNote start - might be inline or invalid
        return None;
    }

    let relative_offset = (tiff_offset - base_offset) as usize;
    if relative_offset + byte_count > data.len() {
        return None;
    }

    let bytes = &data[relative_offset..relative_offset + byte_count];
    // A TIFF ASCII value ends at its first NUL; Canon pads the remainder of the fixed
    // 32-byte OwnerName/ImageType slots with unrelated bytes, so trimming only trailing
    // NULs leaks that padding into the value (ExifTool prints "unknown", not
    // "unknown\0\x01\0\0\0<...").
    let terminated = match bytes.iter().position(|&b| b == 0) {
        Some(nul) => &bytes[..nul],
        None => bytes,
    };

    // No trimming, and an empty result is still a result. ExifTool prints the ASCII
    // value exactly as stored once the NUL terminator is cut: `CanonImageType` keeps the
    // space padding Canon writes ("IMG:ELURA60 JPEG" + 48 spaces), and an all-NUL
    // `OwnerName` prints as the empty string rather than vanishing -- 545 of the 668
    // Canon samples in the corpus carry an OwnerName, and most of them are empty.
    Some(String::from_utf8_lossy(terminated).to_string())
}

/// Extracts a string value from a Canon MakerNote IFD entry.
///
/// This is a convenience wrapper that calculates the base offset automatically.
fn extract_canon_string(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<String> {
    if let Some(base) = calculate_makernote_base(data, byte_order) {
        extract_canon_string_with_base(entry, data, byte_order, base)
    } else {
        // Fallback: try with base_offset = 0 (original behavior)
        extract_canon_string_with_base(entry, data, byte_order, 0)
    }
}

/// Renders a bitmask the way ExifTool's `DecodeBits($val, undef, 16)` does.
///
/// Reference: `Image::ExifTool::DecodeBits` (ExifTool.pm:6362). With no lookup table the
/// set bit numbers are emitted in ascending order joined by `,`, words are 16 bits wide
/// and word `w` contributes bits `w*16 .. w*16+15`, and an empty result prints `(none)`:
///
/// ```text
///     return '(none)' unless @bitList;
///     return join($lookup ? ', ' : ',', @bitList);
/// ```
fn decode_bits_16(words: &[i16]) -> String {
    let mut bits: Vec<String> = Vec::new();
    for (word_index, &word) in words.iter().enumerate() {
        for bit in 0..16usize {
            if (word as u16) & (1u16 << bit) != 0 {
                bits.push((word_index * 16 + bit).to_string());
            }
        }
    }
    if bits.is_empty() {
        "(none)".to_string()
    } else {
        bits.join(",")
    }
}

/// Joins a slice of AF coordinates the way ExifTool prints an `int16s[n]` value.
fn join_i16_slice(values: &[i16]) -> String {
    values
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drops the stray leading element from a Canon binary record that was read one slot early.
///
/// Every `%Canon` record with `FIRST_ENTRY => 1` (CameraSettings, ShotInfo, Processing,
/// SensorInfo, MeasuredColor, FileInfo) and every `ColorData*` table opens with its own
/// size in bytes; ExifTool spells that out at Canon.pm:7444 — `# 0x00: size of record in
/// bytes`. A correctly located record therefore always satisfies
/// `record[0] == record.len() * 2`.
///
/// On a large share of Canon JPEGs the MakerNote base this parser computes lands two bytes
/// low, so the size word turns up at index 1 and every documented index is shifted by one.
/// That single fault is why a 20D reported `SensorWidth 34` — the SensorInfo record's own
/// byte count — instead of 3596, and why `ControlMode` read the slot before the real one.
/// When the size word is unambiguously at index 1, dropping the leading element restores
/// ExifTool's numbering for every index the callers below use.
///
/// The test is deliberately one-sided: when index 0 already holds the size the record is
/// returned untouched, so a correctly read record can never be shifted by this function.
/// Realignment costs the record's final element, which no index used here reaches.
fn realign_length_prefixed_record(array: Vec<i16>) -> Vec<i16> {
    let declares_size =
        |slot: usize| array.get(slot).map(|&v| v as u16 as usize) == Some(array.len() * 2);
    if !declares_size(0) && declares_size(1) {
        return array[1..].to_vec();
    }
    array
}

// Canon MakerNote Tag IDs
const CANON_CAMERA_SETTINGS: u16 = 0x0001;
const CANON_FOCAL_LENGTH: u16 = 0x0002;
const CANON_SHOT_INFO: u16 = 0x0004;
const CANON_PANORAMA: u16 = 0x0005;
const CANON_IMAGE_TYPE: u16 = 0x0006;
const CANON_FIRMWARE_VERSION: u16 = 0x0007;
const CANON_FILE_NUMBER: u16 = 0x0008;
const CANON_OWNER_NAME: u16 = 0x0009;
const CANON_SERIAL_NUMBER: u16 = 0x000C;
const CANON_CAMERA_INFO: u16 = 0x000D;
const CANON_CUSTOM_FUNCTIONS: u16 = 0x000F;
const CANON_MODEL_ID: u16 = 0x0010;
const CANON_FLASH_INFO: u16 = 0x0003;
const CANON_AF_INFO: u16 = 0x0012;
const CANON_SERIAL_NUMBER_FORMAT: u16 = 0x0015;
const CANON_AF_INFO2: u16 = 0x0026;
/// ExifTool Canon.pm:1764 -- `0x3c => { Name => 'AFInfo3', ... TagTable =>
/// 'Image::ExifTool::Canon::AFInfo2' }`. A second MakerNote tag carrying the very same
/// `%Canon::AFInfo2` record, used by the G1XmkII and the EOS M bodies after it.
const CANON_AF_INFO3: u16 = 0x003C;
const CANON_FILE_INFO: u16 = 0x0093;
const CANON_LENS_MODEL: u16 = 0x0095;
const CANON_INTERNAL_SERIAL_NUMBER: u16 = 0x0096;
const CANON_PROCESSING_INFO: u16 = 0x00A0;
const CANON_MEASURED_COLOR: u16 = 0x00AA;
const CANON_COLOR_SPACE: u16 = 0x00B4;
const CANON_VRD_OFFSET: u16 = 0x00D0;
/// ExifTool Canon.pm:1965 — `0xe0 => { Name => 'SensorInfo', ... }`
const CANON_SENSOR_INFO: u16 = 0x00E0;
/// ExifTool Canon.pm:1607 — `0x13 => { Name => 'ThumbnailImageValidArea', ... }`
const CANON_THUMBNAIL_IMAGE_VALID_AREA: u16 = 0x0013;
/// ExifTool Canon.pm:1785 — `0x83 => { Name => 'OriginalDecisionDataOffset', ... }`
const CANON_ORIGINAL_DECISION_DATA_OFFSET: u16 = 0x0083;
/// ExifTool Canon.pm:1972 — `0x4001 => [ ... ColorData1..ColorData12 ... ]`
const CANON_COLOR_DATA: u16 = 0x4001;

/// FilterInfo (MakerNote tag 0x4024). Not a plain BinaryData table -- it
/// declares its own `PROCESS_PROC`; see [`filter_info`].
const CANON_FILTER_INFO: u16 = 0x4024;
/// `%Canon::LensInfo` (MakerNote tag 0x4019, Canon.pm:9130). One field,
/// `LensSerialNumber` (key 0, `undef[5]`), `Priority => 0`.
const CANON_LENS_INFO: u16 = 0x4019;
/// `%Canon::LevelInfo` (MakerNote tag 0x4059, Canon.pm:9583). `FORMAT =>
/// 'int32s'`, `FIRST_ENTRY => 1`, ordinary (default) priority.
const CANON_LEVEL_INFO: u16 = 0x4059;
/// ExifTool Canon.pm:1802 — `0x91 => { Name => 'PersonalFunctions', ... }`
const CANON_PERSONAL_FUNCTIONS: u16 = 0x0091;
/// ExifTool Canon.pm:1809 — `0x92 => { Name => 'PersonalFunctionValues', ... }`
const CANON_PERSONAL_FUNCTION_VALUES: u16 = 0x0092;
/// ExifTool Canon.pm:1883 — `0x99 => { Name => 'CustomFunctions2', ... }`
const CANON_CUSTOM_FUNCTIONS2: u16 = 0x0099;

/// `%CanonCustom::PersonalFuncs` (CanonCustom.pm:1091) — EOS-1D personal function
/// switches, keyed by their `int16u` index in the record.
///
/// The table is `FIRST_ENTRY => 1` (index 0 is the record's own byte count) and
/// indices 12, 13 and 23 are commented out in ExifTool as unused, so they are absent
/// here too rather than emitted under a guessed name.
const PERSONAL_FUNCS: &[(usize, &str)] = &[
    (1, "PF0CustomFuncRegistration"),
    (2, "PF1DisableShootingModes"),
    (3, "PF2DisableMeteringModes"),
    (4, "PF3ManualExposureMetering"),
    (5, "PF4ExposureTimeLimits"),
    (6, "PF5ApertureLimits"),
    (7, "PF6PresetShootingModes"),
    (8, "PF7BracketContinuousShoot"),
    (9, "PF8SetBracketShots"),
    (10, "PF9ChangeBracketSequence"),
    (11, "PF10RetainProgramShift"),
    (14, "PF13DrivePriority"),
    (15, "PF14DisableFocusSearch"),
    (16, "PF15DisableAFAssistBeam"),
    (17, "PF16AutoFocusPointShoot"),
    (18, "PF17DisableAFPointSel"),
    (19, "PF18EnableAutoAFPointSel"),
    (20, "PF19ContinuousShootSpeed"),
    (21, "PF20LimitContinousShots"),
    (22, "PF21EnableQuietOperation"),
    (24, "PF23SetTimerLengths"),
    (25, "PF24LightLCDDuringBulb"),
    (26, "PF25DefaultClearSettings"),
    (27, "PF26ShortenReleaseLag"),
    (28, "PF27ReverseDialRotation"),
    (29, "PF28NoQuickDialExpComp"),
    (30, "PF29QuickDialSwitchOff"),
    (31, "PF30EnlargementMode"),
    (32, "PF31OriginalDecisionData"),
];

/// `%CanonCustom::PersonalFuncValues` (CanonCustom.pm:1135) entries that ExifTool
/// reports verbatim, keyed by their `int16u` index in the record.
///
/// Indices 4-7 are excluded: they carry exposure-time and aperture encodings and are
/// converted separately below.
const PERSONAL_FUNC_VALUES: &[(usize, &str)] = &[
    (1, "PF1Value"),
    (2, "PF2Value"),
    (3, "PF3Value"),
    (8, "PF8BracketShots"),
    (9, "PF19ShootingSpeedLow"),
    (10, "PF19ShootingSpeedHigh"),
    (11, "PF20MaxContinousShots"),
    (12, "PF23ShutterButtonTime"),
    (13, "PF23FELockTime"),
    (14, "PF23PostReleaseTime"),
    (15, "PF25AEMode"),
    (16, "PF25MeteringMode"),
    (17, "PF25DriveMode"),
    (18, "PF25AFMode"),
    (19, "PF25AFPointSel"),
    (20, "PF25ImageSize"),
    (21, "PF25WBMode"),
    (22, "PF25Parameters"),
    (23, "PF25ColorMatrix"),
    (24, "PF27Value"),
];

/// Renders a personal-function switch the way `CanonCustom::ConvertPfn` does
/// (CanonCustom.pm:2624):
///
/// ```text
///     return $val ? ($val==1 ? 'On' : "On ($val)") : "Off";
/// ```
fn convert_personal_function(value: u16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "On".to_string(),
        other => format!("On ({})", other),
    }
}

// Canon signature (not always present)
const CANON_SIGNATURE: &[u8] = b"Canon";

// CameraSettings array (tag 0x0001) indices
// Array contains ~50 values with camera settings
// Reference: ExifTool Canon.pm CameraSettings table
const CAMERA_SETTINGS_MACRO_MODE: usize = 1;
const CAMERA_SETTINGS_SELF_TIMER: usize = 2;
const CAMERA_SETTINGS_QUALITY: usize = 3;
const CAMERA_SETTINGS_FLASH_MODE: usize = 4;
const CAMERA_SETTINGS_DRIVE_MODE: usize = 5;
const CAMERA_SETTINGS_FOCUS_MODE: usize = 7;
const CAMERA_SETTINGS_RECORD_MODE: usize = 9;
const CAMERA_SETTINGS_IMAGE_SIZE: usize = 10;
const CAMERA_SETTINGS_EASY_MODE: usize = 11;
const CAMERA_SETTINGS_DIGITAL_ZOOM: usize = 12;
const CAMERA_SETTINGS_CONTRAST: usize = 13;
const CAMERA_SETTINGS_SATURATION: usize = 14;
const CAMERA_SETTINGS_SHARPNESS: usize = 15;
const CAMERA_SETTINGS_ISO: usize = 16;
const CAMERA_SETTINGS_METERING_MODE: usize = 17;
const CAMERA_SETTINGS_FOCUS_RANGE: usize = 18;
// Alias for backward compatibility with tests
const CAMERA_SETTINGS_FOCUS_TYPE: usize = 18;
const CAMERA_SETTINGS_AF_POINT: usize = 19;
const CAMERA_SETTINGS_EXPOSURE_MODE: usize = 20;
const CAMERA_SETTINGS_LENS_TYPE: usize = 22;
const CAMERA_SETTINGS_MAX_FOCAL_LENGTH: usize = 23;
const CAMERA_SETTINGS_MIN_FOCAL_LENGTH: usize = 24;
const CAMERA_SETTINGS_FOCAL_UNITS: usize = 25;
const CAMERA_SETTINGS_MAX_APERTURE: usize = 26;
const CAMERA_SETTINGS_MIN_APERTURE: usize = 27;
/// ExifTool `%Canon::CameraSettings` key 28 (Canon.pm:2553) — `Name => 'FlashModel'`,
/// `Mask => 0x7f`, `RawConv => '$val == 127 ? undef : $val'`. There is no `FlashActivity`
/// key in this table.
const CAMERA_SETTINGS_FLASH_MODEL: usize = 28;
const CAMERA_SETTINGS_FLASH_BITS: usize = 29;
const CAMERA_SETTINGS_FOCUS_CONTINUOUS: usize = 32;
const CAMERA_SETTINGS_AE_SETTING: usize = 33;
/// ExifTool `%Canon::CameraSettings` key 35 (Canon.pm:2645) — `DisplayAperture`, not 40.
const CAMERA_SETTINGS_DISPLAY_APERTURE: usize = 35;
const CAMERA_SETTINGS_ZOOM_SOURCE_WIDTH: usize = 36;
const CAMERA_SETTINGS_ZOOM_TARGET_WIDTH: usize = 37;
const CAMERA_SETTINGS_SPOT_METERING_MODE: usize = 39;
/// ExifTool `%Canon::CameraSettings` key 40 (Canon.pm:2651) — `PhotoEffect`.
const CAMERA_SETTINGS_PHOTO_EFFECT: usize = 40;
/// ExifTool `%Canon::CameraSettings` key 41 (Canon.pm:2668) — `ManualFlashOutput`.
const CAMERA_SETTINGS_MANUAL_FLASH_OUTPUT: usize = 41;
/// ExifTool `%Canon::CameraSettings` key 42 (Canon.pm:2681) — `ColorTone`.
const CAMERA_SETTINGS_COLOR_TONE: usize = 42;

// ShotInfo array (tag 0x0004) indices
// Reference: ExifTool Canon.pm ShotInfo table
const SHOT_INFO_AUTO_ISO: usize = 1;
const SHOT_INFO_BASE_ISO: usize = 2;
const SHOT_INFO_MEASURED_EV: usize = 3;
const SHOT_INFO_TARGET_APERTURE: usize = 4;
const SHOT_INFO_TARGET_EXPOSURE_TIME: usize = 5;
// Alias for backward compatibility with tests
const SHOT_INFO_TARGET_SHUTTER_SPEED: usize = 5;
const SHOT_INFO_EXPOSURE_COMPENSATION: usize = 6;
const SHOT_INFO_WHITE_BALANCE: usize = 7;
const SHOT_INFO_SLOW_SHUTTER: usize = 8;
const SHOT_INFO_SEQUENCE_NUMBER: usize = 9;
const SHOT_INFO_OPTICAL_ZOOM_CODE: usize = 10;
const SHOT_INFO_FLASH_GUIDE_NUMBER: usize = 13;
const SHOT_INFO_AF_POINTS_IN_FOCUS: usize = 14;
// Alias for backward compatibility with tests
const SHOT_INFO_AF_POINTS_USED: usize = 14;
const SHOT_INFO_FLASH_EXPOSURE_COMP: usize = 15;
const SHOT_INFO_AUTO_EXPOSURE_BRACKETING: usize = 16;
const SHOT_INFO_AEB_BRACKET_VALUE: usize = 17;
const SHOT_INFO_CONTROL_MODE: usize = 18;
const SHOT_INFO_FOCUS_DISTANCE_UPPER: usize = 19;
// Alias for backward compatibility with tests
const SHOT_INFO_SUBJECT_DISTANCE: usize = 19;
const SHOT_INFO_FOCUS_DISTANCE_LOWER: usize = 20;
/// ExifTool `%Canon::ShotInfo` key 21 (Canon.pm:2956) — `FNumber`.
const SHOT_INFO_FNUMBER: usize = 21;
/// ExifTool `%Canon::ShotInfo` key 22 (Canon.pm:2965) — `ExposureTime`.
const SHOT_INFO_EXPOSURE_TIME: usize = 22;
/// ExifTool `%Canon::ShotInfo` key 23 (Canon.pm:3001) — `MeasuredEV2`.
const SHOT_INFO_MEASURED_EV2: usize = 23;
const SHOT_INFO_BULB_DURATION: usize = 24;
/// ExifTool `%Canon::ShotInfo` key 26 — `Name => 'CameraType'` (Canon.pm:3011).
const SHOT_INFO_CAMERA_TYPE: usize = 26;
/// ExifTool `%Canon::ShotInfo` key 27 — `Name => 'AutoRotate'` (Canon.pm:3022).
const SHOT_INFO_AUTO_ROTATE: usize = 27;
/// ExifTool `%Canon::ShotInfo` key 28 (Canon.pm:3033) — `NDFilter`.
const SHOT_INFO_ND_FILTER: usize = 28;
/// ExifTool `%Canon::ShotInfo` key 29 (Canon.pm:3037) — `SelfTimer2`.
const SHOT_INFO_SELF_TIMER2: usize = 29;

// FileInfo array indices (tag 0x0093)
//
// ExifTool `%Image::ExifTool::Canon::FileInfo` (Canon.pm:6842), `FORMAT => 'int16s'`:
//
// ```text
//     1 => [ { Name => 'FileNumber', ... Format => 'int32u', ... } ... ],
//     3 => { Name => 'BracketMode', PrintConv => { 0 => 'Off', 1 => 'AEB', ... } },
//     4 => 'BracketValue', #PH
//     5 => 'BracketShotNumber', #PH
// ```
const FILE_INFO_FILE_NUMBER: usize = 1;
// NOTE: the two indices below are a legacy heuristic that has no counterpart in
// `%Canon::FileInfo` (key 1 is a 4-byte int32u spanning int16 slots 1-2, and slot 3 is
// BracketMode). Kept for the models where oxidex has no better source, but suppressed
// on the bodies where key 1 is known to be FileNumber.
const FILE_INFO_SHUTTER_COUNT_LOW: usize = 2;
const FILE_INFO_SHUTTER_COUNT_HIGH: usize = 3;
const FILE_INFO_BRACKET_MODE: usize = 3;
const FILE_INFO_BRACKET_VALUE: usize = 4;
const FILE_INFO_BRACKET_SHOT_NUMBER: usize = 5;
/// ExifTool `%Canon::FileInfo` key 8 (Canon.pm:6948) — `LongExposureNoiseReduction2`.
const FILE_INFO_LONG_EXPOSURE_NR2: usize = 8;
/// ExifTool `%Canon::FileInfo` key 9 (Canon.pm:6963) — `WBBracketMode`.
const FILE_INFO_WB_BRACKET_MODE: usize = 9;
/// ExifTool `%Canon::FileInfo` key 12 (Canon.pm:6971) — `WBBracketValueAB`.
const FILE_INFO_WB_BRACKET_VALUE_AB: usize = 12;
/// ExifTool `%Canon::FileInfo` key 13 (Canon.pm:6972) — `WBBracketValueGM`.
const FILE_INFO_WB_BRACKET_VALUE_GM: usize = 13;
/// ExifTool `%Canon::FileInfo` key 14 (Canon.pm:6973) — `FilterEffect`.
const FILE_INFO_FILTER_EFFECT: usize = 14;
/// ExifTool `%Canon::FileInfo` key 15 (Canon.pm:6984) — `ToningEffect`.
const FILE_INFO_TONING_EFFECT: usize = 15;

// SensorInfo array indices (tag 0x00E0)
//
// ExifTool `%Image::ExifTool::Canon::SensorInfo` (Canon.pm:7409), `FORMAT => 'int16s'`,
// `FIRST_ENTRY => 1` — entry N lives at byte offset 2*N, so the raw int16 index equals
// the Perl key:
//
// ```text
//     9 => { Name => 'BlackMaskLeftBorder', ... },
//     10 => 'BlackMaskTopBorder', #22
//     11 => 'BlackMaskRightBorder', #22
//     12 => 'BlackMaskBottomBorder', #22
// ```
const SENSOR_INFO_SENSOR_WIDTH: usize = 1;
const SENSOR_INFO_SENSOR_HEIGHT: usize = 2;
const SENSOR_INFO_SENSOR_LEFT_BORDER: usize = 5;
const SENSOR_INFO_SENSOR_TOP_BORDER: usize = 6;
const SENSOR_INFO_SENSOR_RIGHT_BORDER: usize = 7;
const SENSOR_INFO_SENSOR_BOTTOM_BORDER: usize = 8;
const SENSOR_INFO_BLACK_MASK_LEFT_BORDER: usize = 9;
const SENSOR_INFO_BLACK_MASK_TOP_BORDER: usize = 10;
const SENSOR_INFO_BLACK_MASK_RIGHT_BORDER: usize = 11;
const SENSOR_INFO_BLACK_MASK_BOTTOM_BORDER: usize = 12;

// AFInfo sequence indices (tag 0x0012)
//
// ExifTool `%Image::ExifTool::Canon::AFInfo` (Canon.pm:6432) is a *serial* record
// (`PROCESS_PROC => \&ProcessSerialData`, `FORMAT => 'int16u'`) with no leading length
// word (Canon.pm:1602 "this record does not begin with a length word"). Keys 0..7 are
// scalars, so the raw int16 index equals the Perl key up to and including key 7:
//
// ```text
//     0 => { Name => 'NumAFPoints', },
//     1 => { Name => 'ValidAFPoints', ... },
//     2 => { Name => 'CanonImageWidth', ... },
//     3 => { Name => 'CanonImageHeight', ... },
//     4 => { Name => 'AFImageWidth', ... },
//     5 => 'AFImageHeight',
//     6 => 'AFAreaWidth',
//     7 => 'AFAreaHeight',
//     8 => { Name => 'AFAreaXPositions', Format => 'int16s[$val{0}]', },
//     9 => { Name => 'AFAreaYPositions', Format => 'int16s[$val{0}]', },
//     10 => { Name => 'AFPointsInFocus', Format => 'int16s[int(($val{0}+15)/16)]', ... },
// ```
const AF_INFO_NUM_AF_POINTS: usize = 0;
const AF_INFO_VALID_AF_POINTS: usize = 1;
const AF_INFO_CANON_IMAGE_WIDTH: usize = 2;
const AF_INFO_CANON_IMAGE_HEIGHT: usize = 3;
const AF_INFO_AF_IMAGE_WIDTH: usize = 4;
const AF_INFO_AF_IMAGE_HEIGHT: usize = 5;
const AF_INFO_AF_AREA_WIDTH: usize = 6;
const AF_INFO_AF_AREA_HEIGHT: usize = 7;
/// First variable-length slot of `%Canon::AFInfo` (Perl key 8, `AFAreaXPositions`).
const AF_INFO_VARIABLE_START: usize = 8;

// AFInfo2 sequence indices (tag 0x0026)
//
// ExifTool `%Image::ExifTool::Canon::AFInfo2` (Canon.pm:6503), also serial
// (`PROCESS_PROC => \&ProcessSerialData`, `FORMAT => 'int16u'`). Keys 0..7 are scalars:
//
// ```text
//     0 => { Name => 'AFInfoSize', Unknown => 1, ... },
//     1 => { Name => 'AFAreaMode', PrintConv => { ... } },
//     2 => { Name => 'NumAFPoints', RawConv => '$$self{NumAFPoints} = $val', },
//     3 => { Name => 'ValidAFPoints', ... },
//     4 => { Name => 'CanonImageWidth', ... },
//     5 => { Name => 'CanonImageHeight', ... },
//     6 => { Name => 'AFImageWidth', ... },
//     7 => 'AFImageHeight',
//     8 => { Name => 'AFAreaWidths', Format => 'int16s[$val{2}]', },
//     9 => { Name => 'AFAreaHeights', Format => 'int16s[$val{2}]', },
//     10 => { Name => 'AFAreaXPositions', Format => 'int16s[$val{2}]', },
//     11 => { Name => 'AFAreaYPositions', Format => 'int16s[$val{2}]', },
//     12 => { Name => 'AFPointsInFocus', Format => 'int16s[int(($val{2}+15)/16)]', ... },
// ```
const AF_INFO2_AF_AREA_MODE: usize = 1;
const AF_INFO2_NUM_AF_POINTS: usize = 2;
const AF_INFO2_VALID_AF_POINTS: usize = 3;
const AF_INFO2_CANON_IMAGE_WIDTH: usize = 4;
const AF_INFO2_CANON_IMAGE_HEIGHT: usize = 5;
const AF_INFO2_AF_IMAGE_WIDTH: usize = 6;
const AF_INFO2_AF_IMAGE_HEIGHT: usize = 7;
/// First variable-length slot of `%Canon::AFInfo2` (Perl key 8, `AFAreaWidths`).
const AF_INFO2_VARIABLE_START: usize = 8;

// FlashInfo array indices (tag 0x0003)
const FLASH_INFO_FLASH_GUIDE_NUMBER: usize = 0;
const FLASH_INFO_FLASH_THRESHOLD: usize = 1;

// ProcessingInfo array indices (tag 0x00A0)
const PROCESSING_INFO_TONE_CURVE: usize = 1;
const PROCESSING_INFO_SHARPNESS: usize = 2;
const PROCESSING_INFO_SHARPNESS_FREQ: usize = 3;
const PROCESSING_INFO_SENSOR_RED_LEVEL: usize = 4;
const PROCESSING_INFO_SENSOR_BLUE_LEVEL: usize = 5;
const PROCESSING_INFO_WHITE_BALANCE_RED: usize = 6;
const PROCESSING_INFO_WHITE_BALANCE_BLUE: usize = 7;
const PROCESSING_INFO_WHITE_BALANCE: usize = 8;
const PROCESSING_INFO_COLOR_TEMPERATURE: usize = 9;
const PROCESSING_INFO_PICTURE_STYLE: usize = 10;
const PROCESSING_INFO_DIGITAL_GAIN: usize = 11;
const PROCESSING_INFO_WB_SHIFT_AB: usize = 12;
const PROCESSING_INFO_WB_SHIFT_GM: usize = 13;

// MeasuredColor array indices (tag 0x00AA)
//
// ExifTool `%Image::ExifTool::Canon::MeasuredColor` (Canon.pm:7294), `FORMAT => 'int16u'`,
// `FIRST_ENTRY => 1` — the only named key is a 4-element array:
//
// ```text
//     1 => { Name => 'MeasuredRGGB', Format => 'int16u[4]' },
// ```
const MEASURED_COLOR_RGGB: usize = 1;

// ============================================================================
// DECODERS - Canon Value Decoders
// ============================================================================
// Using const_decoder! macro for declarative, zero-overhead value decoding

// Canon macro mode decoder
// Used for MacroMode in CameraSettings (index 1)
// Reference: ExifTool Canon.pm MacroMode table
// Value 0 = "Off" (no macro), 1 = "Macro" (macro mode active), 2 = "Normal"
// Public to allow re-use in registry module
const_decoder!(pub MACRO_MODE, i16, [
    (1, "Macro"),
    (2, "Normal"),
]);

// Canon quality setting decoder
// Public to allow re-use in registry module
const_decoder!(
    pub QUALITY,
    i16,
    [
        (-1, "n/a"),
        (1, "Economy"),
        (2, "Normal"),
        (3, "Fine"),
        (4, "RAW"),
        (5, "Superfine"),
        (7, "CRAW"),
        (130, "Normal Movie"),
        (131, "Movie (2)"),
        (132, "Movie (3)"),
        (133, "Movie (4)"),
    ]
);

// Canon flash mode decoder
// Public to allow re-use in registry module
const_decoder!(
    pub FLASH_MODE,
    i16,
    [
        (-1, "n/a"),
        (0, "Off"),
        (1, "Auto"),
        (2, "On"),
        (3, "Red-eye reduction"),
        (4, "Slow-sync"),
        (5, "Red-eye reduction (Auto)"),
        (6, "Red-eye reduction (On)"),
        (16, "External flash"),
    ]
);

// Canon drive mode decoder
// Public to allow re-use in registry module
const_decoder!(
    pub DRIVE_MODE,
    i16,
    [
        (0, "Single"),
        (1, "Continuous"),
        (2, "Movie"),
        (4, "Continuous, Speed Priority"),
        (5, "Continuous, Low"),
        (6, "Continuous, High"),
    ]
);

// Canon focus mode decoder
// Public to allow re-use in registry module
const_decoder!(
    pub FOCUS_MODE,
    i16,
    [
        (0, "One-shot AF"),
        (1, "AI Servo AF"),
        (2, "AI Focus AF"),
        (3, "Manual Focus (3)"),
        (4, "Single"),
        (5, "Continuous"),
        (6, "Manual Focus (6)"),
        (16, "Pan Focus"),
        // Live View and movie focus modes, Canon.pm:2288-2292. A CR3 from an
        // EOS M50 reports 256 here and was printing "Unknown (256)".
        (256, "One-shot AF (Live View)"),
        (257, "AI Servo AF (Live View)"),
        (258, "AI Focus AF (Live View)"),
        (512, "Movie Snap Focus"),
        (519, "Movie Servo AF"),
    ]
);

// Canon metering mode decoder
// Public to allow re-use in registry module
const_decoder!(
    pub METERING_MODE,
    i16,
    [
        (0, "Default"),
        (1, "Spot"),
        (2, "Average"),
        (3, "Evaluative"),
        (4, "Partial"),
        (5, "Center-weighted average"),
    ]
);

// Canon exposure mode decoder
//
// ExifTool `%Image::ExifTool::Canon::CameraSettings` key 20 (Canon.pm:2485). The labels
// are ExifTool's verbatim - "Shutter speed priority AE" and "Aperture-priority AE", not
// the shortened forms other manufacturers use.
const_decoder!(
    pub EXPOSURE_MODE,
    i16,
    [
        (0, "Easy"),
        (1, "Program AE"),
        (2, "Shutter speed priority AE"),
        (3, "Aperture-priority AE"),
        (4, "Manual"),
        (5, "Depth-of-field AE"),
        (6, "M-Dep"),
        (7, "Bulb"),
        (8, "Flexible-priority AE"),
    ]
);

// Canon color space decoder
// Used for ColorSpace tag (0x00B4)
const_decoder!(
    pub COLOR_SPACE,
    i32,
    [
        (1, "sRGB"),
        (2, "Adobe RGB"),
        (65535, "Uncalibrated"),
    ]
);

// Canon picture style decoder
// Used for PictureStyle in ProcessingInfo
const_decoder!(
    pub PICTURE_STYLE,
    i32,
    [
        // ExifTool `%pictureStyles` (Canon.pm:1118) starts at 0x00 - the "ColorMatrix"
        // codes below 0x21 are part of the same table.
        (0x0000, "None"),
        (0x0001, "Standard"),
        (0x0002, "Portrait"),
        (0x0003, "High Saturation"),
        (0x0004, "Adobe RGB"),
        (0x0005, "Low Saturation"),
        (0x0006, "CM Set 1"),
        (0x0007, "CM Set 2"),
        (0x0021, "User Def. 1"),
        (0x0022, "User Def. 2"),
        (0x0023, "User Def. 3"),
        (0x0041, "PC 1"),
        (0x0042, "PC 2"),
        (0x0043, "PC 3"),
        (0x0081, "Standard"),
        (0x0082, "Portrait"),
        (0x0083, "Landscape"),
        (0x0084, "Neutral"),
        (0x0085, "Faithful"),
        (0x0086, "Monochrome"),
        (0x0087, "Auto"),
        (0x0088, "Fine Detail"),
        (0x00ff, "n/a"),
        (0xffff, "n/a"),
    ]
);

// Canon tone curve decoder
// Used for ToneCurve in ProcessingInfo
const_decoder!(
    pub TONE_CURVE,
    i32,
    [
        (0, "Standard"),
        (1, "Manual"),
        (2, "Custom"),
    ]
);

// Canon record mode decoder
// Used for RecordMode in CameraSettings (index 9)
const_decoder!(
    pub RECORD_MODE,
    i16,
    [
        (0, "n/a"),
        (1, "JPEG"),
        (2, "CRW+THM"),
        (3, "AVI+THM"),
        (4, "TIF"),
        (5, "TIF+JPEG"),
        (6, "CR2"),
        (7, "CR2+JPEG"),
        (9, "MOV"),
        (10, "MP4"),
        (11, "CRM"),
        (12, "CR3"),
        (13, "CR3+JPEG"),
        (14, "HIF"),
        (15, "CR3+HIF"),
    ]
);

// Canon image size decoder
// Used for CanonImageSize in CameraSettings (index 10)
const_decoder!(
    pub CANON_IMAGE_SIZE,
    i16,
    [
        (-1, "n/a"),
        (0, "Large"),
        (1, "Medium"),
        (2, "Small"),
        (5, "Medium 1"),
        (6, "Medium 2"),
        (7, "Medium 3"),
        (8, "Postcard"),
        (9, "Widescreen"),
        (10, "Medium Widescreen"),
        (14, "Small 1"),
        (15, "Small 2"),
        (16, "Small 3"),
        (128, "640x480 Movie"),
        (129, "Medium Movie"),
        (130, "Small Movie"),
        (137, "1280x720 Movie"),
        (142, "1920x1080 Movie"),
        (143, "4096x2160 Movie"),
    ]
);

// Canon easy mode decoder (scene modes)
// Used for EasyMode in CameraSettings (index 11)
const_decoder!(
    pub EASY_MODE,
    i16,
    [
        (0, "Full auto"),
        (1, "Manual"),
        (2, "Landscape"),
        (3, "Fast shutter"),
        (4, "Slow shutter"),
        (5, "Night"),
        (6, "Gray Scale"),
        (7, "Sepia"),
        (8, "Portrait"),
        (9, "Sports"),
        (10, "Macro"),
        (11, "Black & White"),
        (12, "Pan focus"),
        (13, "Vivid"),
        (14, "Neutral"),
        (15, "Flash Off"),
        (16, "Long Shutter"),
        (17, "Super Macro"),
        (18, "Foliage"),
        (19, "Indoor"),
        (20, "Fireworks"),
        (21, "Beach"),
        (22, "Underwater"),
        (23, "Snow"),
        (24, "Kids & Pets"),
        (25, "Night Snapshot"),
        (26, "Digital Macro"),
        (27, "My Colors"),
        (28, "Movie Snap"),
        (29, "Super Macro 2"),
        (30, "Color Accent"),
        (31, "Color Swap"),
        (32, "Aquarium"),
        (33, "ISO 3200"),
        (34, "ISO 6400"),
        (35, "Creative Light Effect"),
        (36, "Easy"),
        (37, "Quick Shot"),
        (38, "Creative Auto"),
        (39, "Zoom Blur"),
        (40, "Low Light"),
        (41, "Nostalgic"),
        (42, "Super Vivid"),
        (43, "Poster Effect"),
        (44, "Face Self-timer"),
        (45, "Smile"),
        (46, "Wink Self-timer"),
        (47, "Fisheye Effect"),
        (48, "Miniature Effect"),
        (49, "High-speed Burst"),
        (50, "Best Image Selection"),
        (51, "High Dynamic Range"),
        (52, "Handheld Night Scene"),
        (53, "Movie Digest"),
        (54, "Live View Control"),
        (55, "Discreet"),
        (56, "Blur Reduction"),
        (57, "Monochrome"),
        (58, "Toy Camera Effect"),
        (59, "Scene Intelligent Auto"),
        (60, "High-speed Burst HQ"),
        (61, "Smooth Skin"),
        (62, "Soft Focus"),
        (68, "Food"),
        (84, "HDR Art Standard"),
        (85, "HDR Art Vivid"),
        (93, "HDR Art Bold"),
        (257, "Spotlight"),
        (258, "Night 2"),
        (259, "Night+"),
        (260, "Super Night"),
        (261, "Sunset"),
        (263, "Night Scene"),
        (264, "Surface"),
        (265, "Low Light 2"),
    ]
);

// Canon digital zoom decoder
// Used for DigitalZoom in CameraSettings (index 12)
// Reference: ExifTool Canon.pm DigitalZoom table
// Note: -1 indicates "Off" (not available), 0 indicates "None" (not used)
const_decoder!(
    pub DIGITAL_ZOOM,
    i16,
    [
        (-1, "Off"),
        (0, "None"),
        (1, "2x"),
        (2, "4x"),
        (3, "Other"),
    ]
);

// Canon focus range decoder
// Used for FocusRange in CameraSettings (index 18)
const_decoder!(
    pub FOCUS_RANGE,
    i16,
    [
        (0, "Manual"),
        (1, "Auto"),
        (2, "Not Known"),
        (3, "Macro"),
        (4, "Very Close"),
        (5, "Close"),
        (6, "Middle Range"),
        (7, "Far Range"),
        (8, "Pan Focus"),
        (9, "Super Macro"),
        (10, "Infinity"),
    ]
);

// Canon AF point selected decoder
// Used for AFPoint in CameraSettings (index 19)
const_decoder!(
    pub AF_POINT,
    i16,
    [
        (0x2005, "Manual AF point selection"),
        (0x3000, "None (MF)"),
        (0x3001, "Auto AF point selection"),
        (0x3002, "Right"),
        (0x3003, "Center"),
        (0x3004, "Left"),
        (0x4001, "Auto AF point selection"),
        (0x4006, "Face Detect"),
    ]
);

// Canon AE setting decoder
// Used for AESetting in CameraSettings (index 33)
const_decoder!(
    pub AE_SETTING,
    i16,
    [
        (0, "Normal AE"),
        (1, "Exposure Compensation"),
        (2, "AE Lock"),
        (3, "AE Lock + Exposure Compensation"),
        (4, "No AE"),
    ]
);

// Canon spot metering mode decoder
// Used for SpotMeteringMode in CameraSettings (index 39)
const_decoder!(
    pub SPOT_METERING_MODE,
    i16,
    [
        (0, "Center"),
        (1, "AF Point"),
    ]
);

// Canon focus continuous decoder
// Used for FocusContinuous in CameraSettings (index 32)
const_decoder!(
    pub FOCUS_CONTINUOUS,
    i16,
    [
        (0, "Single"),
        (1, "Continuous"),
        (8, "Manual"),
    ]
);

/// `%Canon::CameraSettings` key 29 `FlashBits` (Canon.pm:2686).
///
/// ExifTool renders it as `PrintConv => { 0 => '(none)', BITMASK => { ... } }`, i.e. the
/// set bits' names joined with ", ", and the literal "(none)" when no bit is set. The
/// bit NUMBERS below are ExifTool's own -- note the gaps at 5, 6, 8-10 and 12, which an
/// evenly-spaced mask table silently mislabels.
const FLASH_BITS: &[(u32, &str)] = &[
    (0, "Manual"),
    (1, "TTL"),
    (2, "A-TTL"),
    (3, "E-TTL"),
    (4, "FP sync enabled"),
    (7, "2nd-curtain sync used"),
    (11, "FP sync used"),
    (13, "Built-in"),
    (14, "External"),
];

/// Renders `FlashBits` the way ExifTool's `BITMASK` PrintConv does.
fn decode_flash_bits(value: u16) -> String {
    let names: Vec<&str> = FLASH_BITS
        .iter()
        .filter(|(bit, _)| value & (1u16 << *bit) != 0)
        .map(|(_, name)| *name)
        .collect();
    if names.is_empty() {
        "(none)".to_string()
    } else {
        names.join(", ")
    }
}

// Canon slow shutter decoder
// Used for SlowShutter in ShotInfo (index 8)
const_decoder!(
    pub SLOW_SHUTTER,
    i16,
    [
        (-1, "n/a"),
        (0, "Off"),
        (1, "Night Scene"),
        (2, "On"),
        (3, "None"),
    ]
);

// Canon control mode decoder
//
// ExifTool `%Image::ExifTool::Canon::ShotInfo` key 18 (Canon.pm:2925):
//
// ```text
//     18 => { #22
//         Name => 'ControlMode',
//         PrintConv => {
//             0 => 'n/a',
//             1 => 'Camera Local Control',
//             # 2 - have seen this for EOS M studio picture
//             3 => 'Computer Remote Control',
//         },
//     },
// ```
const_decoder!(
    pub CONTROL_MODE,
    i16,
    [
        (0, "n/a"),
        (1, "Camera Local Control"),
        (3, "Computer Remote Control"),
    ]
);

// Canon external flash model decoder
//
// ExifTool `%flashModel` (Canon.pm:1028), used by `%Canon::CameraSettings` key 28 after
// masking with 0x7f. Code 127 is discarded by that key's `RawConv`, and code 1 is
// deliberately absent from the table upstream.
const_decoder!(
    pub FLASH_MODEL,
    i16,
    [
        (0, "n/a"),
        (4, "Speedlite 540EZ"),
        (5, "Speedlite 380EX"),
        (6, "Speedlite 550EX"),
        (8, "Speedlite ST-E2"),
        (9, "Speedlite MR-14EX"),
        (12, "Speedlite 580EX"),
        (13, "Speedlite 430EX"),
        (17, "Speedlite 580EX II"),
        (18, "Speedlite 430EX II"),
        (22, "Speedlite 600EX-RT"),
        (23, "Speedlite 600EX II-RT"),
        (24, "Speedlite 90EX"),
        (25, "Speedlite 430EX III-RT"),
        (31, "Speedlite EL-1 ver2"),
        (33, "Speedlite EL-5"),
        (34, "Speedlite EL-10"),
    ]
);

// Canon photo effect decoder
// ExifTool `%Canon::CameraSettings` key 40 (Canon.pm:2651)
const_decoder!(
    pub PHOTO_EFFECT,
    i16,
    [
        (0, "Off"),
        (1, "Vivid"),
        (2, "Neutral"),
        (3, "Smooth"),
        (4, "Sepia"),
        (5, "B&W"),
        (6, "Custom"),
        (100, "My Color Data"),
    ]
);

// Canon manual flash output decoder
// ExifTool `%Canon::CameraSettings` key 41 (Canon.pm:2668), `PrintHex => 1`
const_decoder!(
    pub MANUAL_FLASH_OUTPUT,
    i32,
    [
        (0, "n/a"),
        (0x500, "Full"),
        (0x502, "Medium"),
        (0x504, "Low"),
        (0x7fff, "n/a"),
    ]
);

// Canon sharpness frequency decoder
// ExifTool `%Canon::Processing` key 3 (Canon.pm:7220)
const_decoder!(
    pub SHARPNESS_FREQUENCY,
    i16,
    [
        (0, "n/a"),
        (1, "Lowest"),
        (2, "Low"),
        (3, "Standard"),
        (4, "High"),
        (5, "Highest"),
    ]
);

// Canon long-exposure noise reduction (second flavour) decoder
// ExifTool `%Canon::FileInfo` key 8 (Canon.pm:6948)
const_decoder!(
    pub LONG_EXPOSURE_NOISE_REDUCTION2,
    i16,
    [(0, "Off"), (1, "On (1D)"), (3, "On"), (4, "Auto"),]
);

// Canon white-balance bracket mode decoder
// ExifTool `%Canon::FileInfo` key 9 (Canon.pm:6963)
const_decoder!(
    pub WB_BRACKET_MODE,
    i16,
    [(0, "Off"), (1, "On (shift AB)"), (2, "On (shift GM)"),]
);

// Canon monochrome filter effect decoder
// ExifTool `%Canon::FileInfo` key 14 (Canon.pm:6973)
const_decoder!(
    pub FILTER_EFFECT,
    i16,
    [
        (0, "None"),
        (1, "Yellow"),
        (2, "Orange"),
        (3, "Red"),
        (4, "Green"),
    ]
);

// Canon monochrome toning effect decoder
// ExifTool `%Canon::FileInfo` key 15 (Canon.pm:6984)
const_decoder!(
    pub TONING_EFFECT,
    i16,
    [
        (0, "None"),
        (1, "Sepia"),
        (2, "Blue"),
        (3, "Purple"),
        (4, "Green"),
    ]
);

// Canon D30/D60/PowerShot AF-points-in-focus code decoder
//
// ExifTool `%Canon::ShotInfo` key 14 (Canon.pm:2884) is a `PrintHex` lookup, not a
// bitmask; its `RawConv` drops 0 so the caller must skip that value.
const_decoder!(
    pub SHOT_INFO_AF_POINTS_IN_FOCUS_CODES,
    i32,
    [
        (0x3000, "None (MF)"),
        (0x3001, "Right"),
        (0x3002, "Center"),
        (0x3003, "Center+Right"),
        (0x3004, "Left"),
        (0x3005, "Left+Right"),
        (0x3006, "Left+Center"),
        (0x3007, "All"),
    ]
);

// Canon ND filter decoder
// ExifTool `%Canon::ShotInfo` key 28 (Canon.pm:3033)
const_decoder!(pub ND_FILTER, i16, [(-1, "n/a"), (0, "Off"), (1, "On"),]);

// Canon auto exposure bracketing decoder
// ExifTool `%Canon::ShotInfo` key 16 (Canon.pm:2907)
const_decoder!(
    pub AUTO_EXPOSURE_BRACKETING,
    i16,
    [
        (-1, "On"),
        (0, "Off"),
        (1, "On (shot 1)"),
        (2, "On (shot 2)"),
        (3, "On (shot 3)"),
    ]
);

// Canon serial number display format decoder
// ExifTool Canon.pm:1615 — `0x15 => { Name => 'SerialNumberFormat', PrintHex => 1, ... }`
const_decoder!(
    pub SERIAL_NUMBER_FORMAT,
    i64,
    [(0x9000_0000, "Format 1"), (0xa000_0000, "Format 2"),]
);

// ----------------------------------------------------------------------------
// CanonCustom::Functions350D (CanonCustom.pm:809)
// ----------------------------------------------------------------------------
// Selected by Canon.pm:1542 when `$$self{Model} =~ /\b(350D|REBEL XT|Kiss Digital N)\b/`.
// Every key is an `int8u` produced by `ProcessCanonCustom` (CanonCustom.pm:2772), which
// reads one int16 per entry and splits it into `tag = $val >> 8` / `value = $val & 0xff`.

const_decoder!(
    pub CC350D_SET_BUTTON_CROSS_KEYS_FUNC,
    i16,
    [
        (0, "Normal"),
        (1, "Set: Quality"),
        (2, "Set: Parameter"),
        (3, "Set: Playback"),
        (4, "Cross keys: AF point select"),
    ]
);

const_decoder!(
    pub CC350D_LONG_EXPOSURE_NOISE_REDUCTION,
    i16,
    [(0, "Off"), (1, "On"),]
);

const_decoder!(
    pub CC350D_FLASH_SYNC_SPEED_AV,
    i16,
    [(0, "Auto"), (1, "1/200 Fixed"),]
);

const_decoder!(
    pub CC350D_SHUTTER_AE_LOCK,
    i16,
    [
        (0, "AF/AE lock"),
        (1, "AE lock/AF"),
        (2, "AF/AF lock, No AE lock"),
        (3, "AE/AF, No AE lock"),
    ]
);

const_decoder!(
    pub CC350D_AF_ASSIST_BEAM,
    i16,
    [
        (0, "Emits"),
        (1, "Does not emit"),
        (2, "Only ext. flash emits"),
    ]
);

const_decoder!(
    pub CC350D_EXPOSURE_LEVEL_INCREMENTS,
    i16,
    [(0, "1/3 Stop"), (1, "1/2 Stop"),]
);

const_decoder!(pub CC350D_MIRROR_LOCKUP, i16, [(0, "Disable"), (1, "Enable"),]);

const_decoder!(pub CC350D_ETTL_II, i16, [(0, "Evaluative"), (1, "Average"),]);

const_decoder!(
    pub CC350D_SHUTTER_CURTAIN_SYNC,
    i16,
    [(0, "1st-curtain sync"), (1, "2nd-curtain sync"),]
);

// Canon AutoRotate decoder
//
// ExifTool `%Image::ExifTool::Canon::ShotInfo` key 27 (Canon.pm:3022):
//
// ```text
//     27 => {
//         Name => 'AutoRotate',
//         RawConv => '$val >= 0 ? $val : undef',
//         PrintConv => {
//            -1 => 'n/a', # (set to -1 when rotated by Canon software)
//             0 => 'None',
//             1 => 'Rotate 90 CW',
//             2 => 'Rotate 180',
//             3 => 'Rotate 270 CW',
//         },
//     },
// ```
//
// The `-1 => 'n/a'` entry is unreachable: `RawConv` discards every negative value before
// `PrintConv` runs, so the caller must skip negatives rather than map them.
const_decoder!(
    pub AUTO_ROTATE,
    i16,
    [
        (0, "None"),
        (1, "Rotate 90 CW"),
        (2, "Rotate 180"),
        (3, "Rotate 270 CW"),
    ]
);

// Canon BracketMode decoder
//
// ExifTool `%Image::ExifTool::Canon::FileInfo` key 3 (Canon.pm:6929):
//
// ```text
//     3 => { #PH
//         Name => 'BracketMode',
//         PrintConv => {
//             0 => 'Off',
//             1 => 'AEB',
//             2 => 'FEB',
//             3 => 'ISO',
//             4 => 'WB',
//         },
//     },
// ```
const_decoder!(
    pub BRACKET_MODE,
    i16,
    [(0, "Off"), (1, "AEB"), (2, "FEB"), (3, "ISO"), (4, "WB"),]
);

// Canon AFAreaMode decoder (AFInfo2 key 1)
//
// ExifTool `%Image::ExifTool::Canon::AFInfo2` key 1 (Canon.pm:6517):
//
// ```text
//     1 => {
//         Name => 'AFAreaMode',
//         PrintConv => {
//             0 => 'Off (Manual Focus)',
//             1 => 'AF Point Expansion (surround)', #PH
//             2 => 'Single-point AF',
//             # 3 - n/a
//             4 => 'Auto', #forum6237 (AiAF on A570IS)
//             5 => 'Face Detect AF',
//             6 => 'Face + Tracking', #PH (NC, EOS M, live view)
//             7 => 'Zone AF', #46
//             8 => 'AF Point Expansion (4 point)', #46/PH/forum6237
//             9 => 'Spot AF', #46
//             10 => 'AF Point Expansion (8 point)', #forum6237
//             11 => 'Flexizone Multi (49 point)', #PH (NC, EOS M, live view; 750D 49 points)
//             12 => 'Flexizone Multi (9 point)', #PH (750D, 9 points)
//             13 => 'Flexizone Single', #PH (EOS M default, live view) ...
//             14 => 'Large Zone AF', #PH/forum6237 (7DmkII)
//             16 => 'Large Zone AF (vertical)', #forum16223
//             17 => 'Large Zone AF (horizontal)', #forum16223
//             19 => 'Flexible Zone AF 1', #github268 (R7)
//             20 => 'Flexible Zone AF 2', #github268 (R7)
//             21 => 'Flexible Zone AF 3', #github268 (R7)
//             22 => 'Whole Area AF', #github268 (R7)
//         },
//     },
// ```
const_decoder!(
    pub AF_AREA_MODE,
    i16,
    [
        (0, "Off (Manual Focus)"),
        (1, "AF Point Expansion (surround)"),
        (2, "Single-point AF"),
        (4, "Auto"),
        (5, "Face Detect AF"),
        (6, "Face + Tracking"),
        (7, "Zone AF"),
        (8, "AF Point Expansion (4 point)"),
        (9, "Spot AF"),
        (10, "AF Point Expansion (8 point)"),
        (11, "Flexizone Multi (49 point)"),
        (12, "Flexizone Multi (9 point)"),
        (13, "Flexizone Single"),
        (14, "Large Zone AF"),
        (16, "Large Zone AF (vertical)"),
        (17, "Large Zone AF (horizontal)"),
        (19, "Flexible Zone AF 1"),
        (20, "Flexible Zone AF 2"),
        (21, "Flexible Zone AF 3"),
        (22, "Whole Area AF"),
    ]
);

// Canon white balance decoder for ShotInfo
// More detailed than standard EXIF white balance
const_decoder!(
    pub WHITE_BALANCE,
    i16,
    [
        (0, "Auto"),
        (1, "Daylight"),
        (2, "Cloudy"),
        (3, "Tungsten"),
        (4, "Fluorescent"),
        (5, "Flash"),
        (6, "Custom"),
        (7, "Black & White"),
        (8, "Shade"),
        (9, "Manual Temperature (Kelvin)"),
        (10, "PC Set 1"),
        (11, "PC Set 2"),
        (12, "PC Set 3"),
        (14, "Daylight Fluorescent"),
        (15, "Custom 1"),
        (16, "Custom 2"),
        (17, "Underwater"),
        (18, "Custom 3"),
        (19, "Custom 4"),
        (20, "PC Set 4"),
        (21, "PC Set 5"),
        (23, "Auto (Ambience Priority)"),
    ]
);

// Canon Contrast decoder
// Used for Contrast in CameraSettings (index 13)
// Reference: ExifTool Canon.pm Contrast table
// Canon uses signed values: 0=Normal, negative=Low, positive=High
const_decoder!(
    pub CONTRAST,
    i16,
    [
        (-2, "Very Low"),
        (-1, "Low"),
        (0, "Normal"),
        (1, "High"),
        (2, "Very High"),
    ]
);

// Canon Saturation decoder
// Used for Saturation in CameraSettings (index 14)
// Reference: ExifTool Canon.pm Saturation table
// Canon uses signed values: 0=Normal, negative=Low, positive=High
const_decoder!(
    pub SATURATION,
    i16,
    [
        (-2, "Very Low"),
        (-1, "Low"),
        (0, "Normal"),
        (1, "High"),
        (2, "Very High"),
    ]
);

// Canon Sharpness decoder
// Used for Sharpness in CameraSettings (index 15)
// Reference: ExifTool Canon.pm Sharpness table
// Canon uses signed values: 0=Normal, negative=Soft, positive=Sharp
const_decoder!(
    pub SHARPNESS,
    i16,
    [
        (-2, "Very Soft"),
        (-1, "Soft"),
        (0, "Normal"),
        (1, "Sharp"),
        (2, "Very Sharp"),
    ]
);

// Canon FocalType decoder
// Used for FocalType in FocalLength array (index 0)
// Reference: ExifTool Canon.pm FocalType table
// Describes whether lens is fixed focal length or zoom
const_decoder!(
    pub FOCAL_TYPE,
    i16,
    [
        (0, "Unknown"),
        (1, "Fixed"),
        (2, "Zoom"),
        (3, "Fixed"),  // Alternative encoding for fixed lens
    ]
);

// ============================================================================
// APEX CONVERSION HELPERS
// ============================================================================
// Canon stores aperture and shutter speed values in APEX format.
// APEX (Additive System of Photographic Exposure) uses logarithmic scales.

/// Converts a Canon APEX aperture value the way every `%Canon` aperture key does.
///
/// Shared verbatim by `MaxAperture` and `MinAperture` (`%Canon::CameraSettings` keys
/// 26/27, Canon.pm:2617) and `TargetAperture` (`%Canon::ShotInfo` key 4, Canon.pm:2803):
///
/// ```text
///     RawConv   => '$val > 0 ? $val : undef',
///     ValueConv => 'exp(Image::ExifTool::Canon::CanonEv($val)*log(2)/2)',
///     PrintConv => 'sprintf("%.2g",$val)',
/// ```
///
/// Two things this is easy to get wrong, and which the corpus punishes hard: the
/// printed value carries **no** `f/` prefix (ExifTool prints `5.8`, not `f/5.8`), and
/// the exponent must go through [`canon_ev`] rather than a plain `raw / 64` -- the two
/// agree only when the value lands on a whole stop, and diverge on Canon's 1/3-stop
/// codes 0x0c and 0x14.
///
/// # Returns
/// `None` when the raw value is not positive, matching ExifTool's `RawConv` dropping
/// the tag entirely rather than reporting a placeholder.
pub fn canon_aperture(raw_value: i16) -> Option<String> {
    // `RawConv => '$val > 0 ? $val : undef'`
    if raw_value <= 0 {
        return None;
    }
    // `ValueConv => 'exp(CanonEv($val)*log(2)/2)'` == 2 ** (CanonEv($val) / 2)
    Some(format_g2(
        2.0_f64.powf(canon_ev(i32::from(raw_value)) / 2.0),
    ))
}

/// Converts a Canon APEX shutter speed value to an exposure time string.
///
/// Canon stores shutter speed as raw value that needs conversion using the formula:
/// exposure_time = 2 ^ (-apex_value / 32)
///
/// # Parameters
/// - `apex_value`: The raw APEX shutter speed value from Canon MakerNote
///
/// # Returns
/// A formatted exposure time string (e.g., "1/250", "1/60", "2 sec")
///
/// # Example
/// ```ignore
/// let shutter = apex_to_exposure_time(256); // Returns "1/256" approximately
/// ```
pub fn apex_to_exposure_time(apex_value: i16) -> String {
    if apex_value == 0 {
        return "n/a".to_string();
    }

    // Canon formula: exposure = 2^(-apex/32)
    let exposure_time = 2.0_f64.powf(-(apex_value as f64) / 32.0);

    // Format based on the exposure time value
    if exposure_time >= 1.0 {
        // 1 second or longer
        if exposure_time == exposure_time.round() {
            format!("{} sec", exposure_time as i32)
        } else {
            format!("{:.1} sec", exposure_time)
        }
    } else if exposure_time >= 0.5 {
        // Between 0.5 and 1 second - show as fraction
        let denominator = (1.0 / exposure_time).round() as i32;
        format!("1/{}", denominator)
    } else {
        // Faster than 0.5 second - calculate as 1/x
        let denominator = (1.0 / exposure_time).round() as i32;
        format!("1/{}", denominator)
    }
}

/// Formats a focal length value with units.
///
/// Takes a raw focal length value and the focal units per mm,
/// and returns a formatted string like "50 mm" or "24.0 mm".
///
/// # Parameters
/// - `raw_value`: The raw focal length value from Canon MakerNote
/// - `focal_units`: The units per mm (typically 1, but can be other values)
///
/// # Returns
/// A formatted focal length string with "mm" suffix
pub fn format_focal_length(raw_value: u16, focal_units: i16) -> String {
    if focal_units == 0 {
        return "n/a".to_string();
    }

    // ExifTool `ValueConv => '$val / ($$self{FocalUnits} || 1)'` followed by
    // `PrintConv => '"$val mm"'` interpolates whatever Perl's own stringification
    // produces -- the full quotient, not a rounded one. A 1/16 mm unit body prints
    // `16.1875 mm`; rounding that to one decimal (`16.2 mm`) is a value difference on
    // every such sample.
    format!(
        "{} mm",
        format_perl_number(f64::from(raw_value) / f64::from(focal_units))
    )
}

/// Converts a Canon APEX-style value to an EV (exposure value) string.
///
/// Canon stores many exposure-related values in a scaled format where the
/// raw value needs to be divided by 32 to get the actual EV value.
///
/// # Parameters
/// - `value`: The raw APEX-encoded value from the ShotInfo array
///
/// # Returns
/// A formatted string with the EV value to 1 decimal place, with sign prefix
/// (e.g., "+1.5", "-0.7", "0.0")
fn apex_to_ev(value: i16) -> String {
    // Canon APEX values are scaled by 32 (5 bits of fraction)
    let ev = value as f64 / 32.0;
    if ev >= 0.0 {
        format!("+{:.1}", ev)
    } else {
        format!("{:.1}", ev)
    }
}

/// Converts a Canon hex-based EV code to a number.
///
/// ExifTool `Image::ExifTool::Canon::CanonEv` (Canon.pm:10648):
///
/// ```text
///     my $frac = $val & 0x1f;
///     $val -= $frac;                       # remove fraction
///     if ($frac == 0x0c) { $frac = 0x20 / 3; }        # 1/3 stop
///     elsif ($frac == 0x14) { $frac = 0x40 / 3; }     # 2/3 stop
///     return $sign * ($val + $frac) / 0x20;
/// ```
pub fn canon_ev(value: i32) -> f64 {
    let sign = if value < 0 { -1.0 } else { 1.0 };
    let magnitude = value.unsigned_abs();
    let raw_frac = magnitude & 0x1f;
    let whole = (magnitude - raw_frac) as f64;
    let frac = match raw_frac {
        0x0c => 0x20 as f64 / 3.0,
        0x14 => 0x40 as f64 / 3.0,
        other => other as f64,
    };
    sign * (whole + frac) / 32.0
}

/// Renders a number the way Perl's `sprintf("%.2g", $val)` does.
///
/// Used by every `%Canon` aperture key (`MaxAperture`, `TargetAperture`, `FNumber`, ...).
/// Values large enough for `%g` to switch to exponential notation are printed in full
/// instead, because an aperture is never displayed as `1.3e+02`.
fn format_g2(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return "0".to_string();
    }
    let exponent = value.abs().log10().floor() as i32;
    let decimals = (1 - exponent).max(0) as usize;
    let rendered = format!("{:.*}", decimals, value);
    if rendered.contains('.') {
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rendered
    }
}

/// Renders an EV offset the way ExifTool does.
///
/// ExifTool `Image::ExifTool::Exif::PrintFraction` (Exif.pm): exact integers print as
/// `%+d`, halves as `%+d/2`, thirds as `%+d/3`, anything else as `%+.3g`, and zero as a
/// bare `0`.
fn print_fraction(value: f64) -> String {
    crate::core::formatters::exif_print_conv::print_fraction(value)
}

/// Renders a Canon "parameter" value (Contrast/Saturation/ColorTone style).
///
/// ExifTool `%Image::ExifTool::Exif::printParameter` (Exif.pm:317) maps 0 to `Normal` and
/// defers everything else to `PrintParameter` (Exif.pm:5533), which prefixes positive
/// values with `+` and re-signs anything above 0xfff0.
fn print_parameter(value: i16) -> String {
    let raw = value as u16 as i32;
    if raw == 0 {
        return "Normal".to_string();
    }
    if raw > 0xfff0 {
        return (raw - 0x10000).to_string();
    }
    format!("+{}", raw)
}

/// Renders `%Canon::FileInfo` key 1 / Canon tag 0x0008 as `directory-file`.
///
/// ExifTool Canon.pm:1264 — `PrintConv => '$_=$val,s/(\d+)(\d{4})/$1-$2/,$_'`: the last
/// four digits are the file number, everything before them the directory number.
fn format_canon_file_number(value: u32) -> String {
    let digits = value.to_string();
    if digits.len() > 4 {
        let split = digits.len() - 4;
        format!("{}-{}", &digits[..split], &digits[split..])
    } else {
        digits
    }
}

/// Decodes `%Canon::CameraSettings` key 16, `CameraISO`.
///
/// ExifTool `Image::ExifTool::Canon::CameraISO` (Canon.pm:10464): 0x7fff means "not
/// present" (the key's `RawConv` drops it), bit 0x4000 marks a literal ISO in the low 14
/// bits, and everything else is a small lookup code.
fn camera_iso(value: i16) -> Option<String> {
    let raw = value as u16;
    if raw == 0x7fff {
        return None;
    }
    if raw & 0x4000 != 0 {
        return Some((raw & 0x3fff).to_string());
    }
    Some(match raw {
        0 => "n/a".to_string(),
        14 => "Auto High".to_string(),
        15 => "Auto".to_string(),
        16 => "50".to_string(),
        17 => "100".to_string(),
        18 => "200".to_string(),
        19 => "400".to_string(),
        20 => "800".to_string(),
        other => format!("Unknown ({})", other),
    })
}

/// True for the bodies whose `%Canon::FileInfo` key 1 is a 20D/350D-style `FileNumber`.
///
/// ExifTool Canon.pm:6850 — `Condition => '$$self{Model} =~ /\b(20D|350D|REBEL XT|Kiss
/// Digital N)\b/'`. The same set selects `%Canon::ShotInfo` key 22's first `ExposureTime`
/// variant (Canon.pm:2968) and excludes `%Canon::Processing` key 2's `Sharpness`
/// (Canon.pm:7217).
fn is_20d_or_350d(model: &str) -> bool {
    ["20D", "350D", "REBEL XT", "Kiss Digital N"]
        .iter()
        .any(|needle| has_word(model, needle))
}

/// True for the bodies that select `%CanonCustom::Functions350D`.
///
/// ExifTool Canon.pm:1542 — `Condition => '$$self{Model} =~ /\b(350D|REBEL XT|Kiss
/// Digital N)\b/'`. The 400D shares most of the layout but redefines keys 0 and 1, so it
/// must not fall through to this table.
fn is_350d_custom_functions(model: &str) -> bool {
    ["350D", "REBEL XT", "Kiss Digital N"]
        .iter()
        .any(|needle| has_word(model, needle))
}

/// True for the bodies whose `%Canon::FocalLength` keys 2 and 3 hold real focal plane
/// sizes.
///
/// ExifTool Canon.pm:2735:
///
/// ```text
///     $$self{Model} !~ /EOS/ or
///     $$self{Model} =~ /\b(1DS?|5D|D30|D60|10D|20D|30D|K236)$/ or
///     $$self{Model} =~ /\b((300D|350D|400D) DIGITAL|REBEL( XTi?)?|Kiss Digital( [NX])?)$/
/// ```
fn focal_plane_size_supported(model: &str) -> bool {
    if !model.contains("EOS") {
        return true;
    }
    const TRAILING: &[&str] = &["1D", "1DS", "5D", "D30", "D60", "10D", "20D", "30D", "K236"];
    if TRAILING
        .iter()
        .any(|suffix| model.ends_with(suffix) && has_word(model, suffix))
    {
        return true;
    }
    // Every alternative in this branch is anchored to the end of the model name by
    // ExifTool's trailing `$`, so a body such as "Canon EOS REBEL T3i" must NOT match
    // "REBEL" — its focal plane slots hold something else entirely.
    const REBEL_FAMILY: &[&str] = &[
        "300D DIGITAL",
        "350D DIGITAL",
        "400D DIGITAL",
        "REBEL",
        "REBEL XT",
        "REBEL XTi",
        "Kiss Digital",
        "Kiss Digital N",
        "Kiss Digital X",
    ];
    REBEL_FAMILY
        .iter()
        .any(|needle| model.ends_with(needle) && has_word(model, needle))
}

/// Perl `\b...\b` word-boundary containment for the model-name conditions above.
fn has_word(haystack: &str, needle: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut start = 0;
    while let Some(found) = haystack[start..].find(needle) {
        let begin = start + found;
        let end = begin + needle.len();
        let before_ok = begin == 0 || !is_word(haystack[..begin].chars().next_back().unwrap());
        let after_ok = end == haystack.len() || !is_word(haystack[end..].chars().next().unwrap());
        if before_ok && after_ok {
            return true;
        }
        start = begin + 1;
        if start >= haystack.len() {
            break;
        }
    }
    false
}

/// Formats a Canon focus distance value to a human-readable string.
///
/// Canon stores focus distance in centimeters. A value of 0xFFFF (65535 or
/// -1 as signed) indicates infinity focus. A value of 0 also indicates infinity.
///
/// # Parameters
/// - `value`: The raw focus distance value from the ShotInfo array (in centimeters)
///
/// # Returns
/// A formatted string with the distance in meters (e.g., "1.50 m", "7.82 m")
/// or "inf" for infinity focus
fn format_focus_distance(value: i16) -> String {
    // ExifTool `%Canon::ShotInfo` keys 19/20 declare `Format => 'int16u'`, so 0xFFFF is
    // 65535 cm, not -1 cm:
    //
    // ```text
    //     ValueConv => '$val / 100',
    //     PrintConv => '$val > 655.345 ? "inf" : "$val m"',
    // ```
    let distance_m = (value as u16) as f64 / 100.0;
    if distance_m > 655.345 {
        return "inf".to_string();
    }
    format!("{} m", format_perl_number(distance_m))
}

/// Decodes the AF points in focus bitfield to a human-readable string.
///
/// Canon stores which AF points were used for focus as a bitmask, where
/// each bit represents a specific AF point. This function converts that
/// bitmask to a comma-separated list of point numbers.
///
/// # Parameters
/// - `value`: The raw bitfield value from the ShotInfo array
///
/// # Returns
/// A comma-separated string of AF point numbers that were in focus
/// (e.g., "Center", "1,2,5", "Center, 1"), or "None" if no points selected
fn decode_af_points_in_focus(value: i16) -> String {
    if value == 0 {
        return "None".to_string();
    }

    let mut points = Vec::new();
    for bit in 0..16 {
        if (value & (1 << bit)) != 0 {
            // AF point numbering typically starts at 1 for display
            // Bit 0 is often the center point
            if bit == 0 {
                points.push("Center".to_string());
            } else {
                points.push(format!("{}", bit));
            }
        }
    }

    if points.is_empty() {
        "None".to_string()
    } else {
        points.join(", ")
    }
}

// ============================================================================
// CANON MODEL ID DECODER
// ============================================================================
// Maps Canon Model ID (tag 0x0010) to human-readable camera model names.
// The model ID is a 32-bit unsigned integer that uniquely identifies each
// Canon camera model. Values typically follow patterns:
// - 0x01XXXXXX: PowerShot series and early cameras
// - 0x80XXXXXX: EOS series digital SLRs and mirrorless cameras
//
// Reference: ExifTool Canon.pm CanonModelID table

/// `%canonModelID` (Canon.pm:656), transcribed from ExifTool -- the PrintConv for
/// MakerNote tag 0x0010 `CanonModelID`.
///
/// All 357 entries; an id absent from the table prints as ExifTool's
/// `Unknown (n)`, which is exactly what ExifTool does for an unlisted key.
const CANON_MODEL_IDS: &[(u32, &str)] = &[
    (0x01010000, "PowerShot A30"),
    (
        0x01040000,
        "PowerShot S300 / Digital IXUS 300 / IXY Digital 300",
    ),
    (0x01060000, "PowerShot A20"),
    (0x01080000, "PowerShot A10"),
    (
        0x01090000,
        "PowerShot S110 / Digital IXUS v / IXY Digital 200",
    ),
    (0x01100000, "PowerShot G2"),
    (0x01110000, "PowerShot S40"),
    (0x01120000, "PowerShot S30"),
    (0x01130000, "PowerShot A40"),
    (0x01140000, "EOS D30"),
    (0x01150000, "PowerShot A100"),
    (
        0x01160000,
        "PowerShot S200 / Digital IXUS v2 / IXY Digital 200a",
    ),
    (0x01170000, "PowerShot A200"),
    (
        0x01180000,
        "PowerShot S330 / Digital IXUS 330 / IXY Digital 300a",
    ),
    (0x01190000, "PowerShot G3"),
    (0x01210000, "PowerShot S45"),
    (
        0x01230000,
        "PowerShot SD100 / Digital IXUS II / IXY Digital 30",
    ),
    (
        0x01240000,
        "PowerShot S230 / Digital IXUS v3 / IXY Digital 320",
    ),
    (0x01250000, "PowerShot A70"),
    (0x01260000, "PowerShot A60"),
    (
        0x01270000,
        "PowerShot S400 / Digital IXUS 400 / IXY Digital 400",
    ),
    (0x01290000, "PowerShot G5"),
    (0x01300000, "PowerShot A300"),
    (0x01310000, "PowerShot S50"),
    (0x01340000, "PowerShot A80"),
    (
        0x01350000,
        "PowerShot SD10 / Digital IXUS i / IXY Digital L",
    ),
    (0x01360000, "PowerShot S1 IS"),
    (0x01370000, "PowerShot Pro1"),
    (0x01380000, "PowerShot S70"),
    (0x01390000, "PowerShot S60"),
    (0x01400000, "PowerShot G6"),
    (
        0x01410000,
        "PowerShot S500 / Digital IXUS 500 / IXY Digital 500",
    ),
    (0x01420000, "PowerShot A75"),
    (
        0x01440000,
        "PowerShot SD110 / Digital IXUS IIs / IXY Digital 30a",
    ),
    (0x01450000, "PowerShot A400"),
    (0x01470000, "PowerShot A310"),
    (0x01490000, "PowerShot A85"),
    (
        0x01520000,
        "PowerShot S410 / Digital IXUS 430 / IXY Digital 450",
    ),
    (0x01530000, "PowerShot A95"),
    (
        0x01540000,
        "PowerShot SD300 / Digital IXUS 40 / IXY Digital 50",
    ),
    (
        0x01550000,
        "PowerShot SD200 / Digital IXUS 30 / IXY Digital 40",
    ),
    (0x01560000, "PowerShot A520"),
    (0x01570000, "PowerShot A510"),
    (
        0x01590000,
        "PowerShot SD20 / Digital IXUS i5 / IXY Digital L2",
    ),
    (0x01640000, "PowerShot S2 IS"),
    (
        0x01650000,
        "PowerShot SD430 / Digital IXUS Wireless / IXY Digital Wireless",
    ),
    (
        0x01660000,
        "PowerShot SD500 / Digital IXUS 700 / IXY Digital 600",
    ),
    (0x01668000, "EOS D60"),
    (
        0x01700000,
        "PowerShot SD30 / Digital IXUS i Zoom / IXY Digital L3",
    ),
    (0x01740000, "PowerShot A430"),
    (0x01750000, "PowerShot A410"),
    (0x01760000, "PowerShot S80"),
    (0x01780000, "PowerShot A620"),
    (0x01790000, "PowerShot A610"),
    (
        0x01800000,
        "PowerShot SD630 / Digital IXUS 65 / IXY Digital 80",
    ),
    (
        0x01810000,
        "PowerShot SD450 / Digital IXUS 55 / IXY Digital 60",
    ),
    (0x01820000, "PowerShot TX1"),
    (
        0x01870000,
        "PowerShot SD400 / Digital IXUS 50 / IXY Digital 55",
    ),
    (0x01880000, "PowerShot A420"),
    (
        0x01890000,
        "PowerShot SD900 / Digital IXUS 900 Ti / IXY Digital 1000",
    ),
    (
        0x01900000,
        "PowerShot SD550 / Digital IXUS 750 / IXY Digital 700",
    ),
    (0x01920000, "PowerShot A700"),
    (
        0x01940000,
        "PowerShot SD700 IS / Digital IXUS 800 IS / IXY Digital 800 IS",
    ),
    (0x01950000, "PowerShot S3 IS"),
    (0x01960000, "PowerShot A540"),
    (
        0x01970000,
        "PowerShot SD600 / Digital IXUS 60 / IXY Digital 70",
    ),
    (0x01980000, "PowerShot G7"),
    (0x01990000, "PowerShot A530"),
    (
        0x02000000,
        "PowerShot SD800 IS / Digital IXUS 850 IS / IXY Digital 900 IS",
    ),
    (
        0x02010000,
        "PowerShot SD40 / Digital IXUS i7 / IXY Digital L4",
    ),
    (0x02020000, "PowerShot A710 IS"),
    (0x02030000, "PowerShot A640"),
    (0x02040000, "PowerShot A630"),
    (0x02090000, "PowerShot S5 IS"),
    (0x02100000, "PowerShot A460"),
    (
        0x02120000,
        "PowerShot SD850 IS / Digital IXUS 950 IS / IXY Digital 810 IS",
    ),
    (0x02130000, "PowerShot A570 IS"),
    (0x02140000, "PowerShot A560"),
    (
        0x02150000,
        "PowerShot SD750 / Digital IXUS 75 / IXY Digital 90",
    ),
    (
        0x02160000,
        "PowerShot SD1000 / Digital IXUS 70 / IXY Digital 10",
    ),
    (0x02180000, "PowerShot A550"),
    (0x02190000, "PowerShot A450"),
    (0x02230000, "PowerShot G9"),
    (0x02240000, "PowerShot A650 IS"),
    (0x02260000, "PowerShot A720 IS"),
    (0x02290000, "PowerShot SX100 IS"),
    (
        0x02300000,
        "PowerShot SD950 IS / Digital IXUS 960 IS / IXY Digital 2000 IS",
    ),
    (
        0x02310000,
        "PowerShot SD870 IS / Digital IXUS 860 IS / IXY Digital 910 IS",
    ),
    (
        0x02320000,
        "PowerShot SD890 IS / Digital IXUS 970 IS / IXY Digital 820 IS",
    ),
    (
        0x02360000,
        "PowerShot SD790 IS / Digital IXUS 90 IS / IXY Digital 95 IS",
    ),
    (
        0x02370000,
        "PowerShot SD770 IS / Digital IXUS 85 IS / IXY Digital 25 IS",
    ),
    (0x02380000, "PowerShot A590 IS"),
    (0x02390000, "PowerShot A580"),
    (0x02420000, "PowerShot A470"),
    (
        0x02430000,
        "PowerShot SD1100 IS / Digital IXUS 80 IS / IXY Digital 20 IS",
    ),
    (0x02460000, "PowerShot SX1 IS"),
    (0x02470000, "PowerShot SX10 IS"),
    (0x02480000, "PowerShot A1000 IS"),
    (0x02490000, "PowerShot G10"),
    (0x02510000, "PowerShot A2000 IS"),
    (0x02520000, "PowerShot SX110 IS"),
    (
        0x02530000,
        "PowerShot SD990 IS / Digital IXUS 980 IS / IXY Digital 3000 IS",
    ),
    (
        0x02540000,
        "PowerShot SD880 IS / Digital IXUS 870 IS / IXY Digital 920 IS",
    ),
    (0x02550000, "PowerShot E1"),
    (0x02560000, "PowerShot D10"),
    (
        0x02570000,
        "PowerShot SD960 IS / Digital IXUS 110 IS / IXY Digital 510 IS",
    ),
    (0x02580000, "PowerShot A2100 IS"),
    (0x02590000, "PowerShot A480"),
    (0x02600000, "PowerShot SX200 IS"),
    (
        0x02610000,
        "PowerShot SD970 IS / Digital IXUS 990 IS / IXY Digital 830 IS",
    ),
    (
        0x02620000,
        "PowerShot SD780 IS / Digital IXUS 100 IS / IXY Digital 210 IS",
    ),
    (0x02630000, "PowerShot A1100 IS"),
    (
        0x02640000,
        "PowerShot SD1200 IS / Digital IXUS 95 IS / IXY Digital 110 IS",
    ),
    (0x02700000, "PowerShot G11"),
    (0x02710000, "PowerShot SX120 IS"),
    (0x02720000, "PowerShot S90"),
    (0x02750000, "PowerShot SX20 IS"),
    (
        0x02760000,
        "PowerShot SD980 IS / Digital IXUS 200 IS / IXY Digital 930 IS",
    ),
    (
        0x02770000,
        "PowerShot SD940 IS / Digital IXUS 120 IS / IXY Digital 220 IS",
    ),
    (0x02800000, "PowerShot A495"),
    (0x02810000, "PowerShot A490"),
    (0x02820000, "PowerShot A3100/A3150 IS"),
    (0x02830000, "PowerShot A3000 IS"),
    (0x02840000, "PowerShot SD1400 IS / IXUS 130 / IXY 400F"),
    (0x02850000, "PowerShot SD1300 IS / IXUS 105 / IXY 200F"),
    (0x02860000, "PowerShot SD3500 IS / IXUS 210 / IXY 10S"),
    (0x02870000, "PowerShot SX210 IS"),
    (0x02880000, "PowerShot SD4000 IS / IXUS 300 HS / IXY 30S"),
    (0x02890000, "PowerShot SD4500 IS / IXUS 1000 HS / IXY 50S"),
    (0x02920000, "PowerShot G12"),
    (0x02930000, "PowerShot SX30 IS"),
    (0x02940000, "PowerShot SX130 IS"),
    (0x02950000, "PowerShot S95"),
    (0x02980000, "PowerShot A3300 IS"),
    (0x02990000, "PowerShot A3200 IS"),
    (0x03000000, "PowerShot ELPH 500 HS / IXUS 310 HS / IXY 31S"),
    (0x03010000, "PowerShot Pro90 IS"),
    (0x03010001, "PowerShot A800"),
    (0x03020000, "PowerShot ELPH 100 HS / IXUS 115 HS / IXY 210F"),
    (0x03030000, "PowerShot SX230 HS"),
    (0x03040000, "PowerShot ELPH 300 HS / IXUS 220 HS / IXY 410F"),
    (0x03050000, "PowerShot A2200"),
    (0x03060000, "PowerShot A1200"),
    (0x03070000, "PowerShot SX220 HS"),
    (0x03080000, "PowerShot G1 X"),
    (0x03090000, "PowerShot SX150 IS"),
    (0x03100000, "PowerShot ELPH 510 HS / IXUS 1100 HS / IXY 51S"),
    (0x03110000, "PowerShot S100 (new)"),
    (0x03130000, "PowerShot SX40 HS"),
    (0x03120000, "PowerShot ELPH 310 HS / IXUS 230 HS / IXY 600F"),
    (0x03140000, "IXY 32S"),
    (0x03160000, "PowerShot A1300"),
    (0x03170000, "PowerShot A810"),
    (0x03180000, "PowerShot ELPH 320 HS / IXUS 240 HS / IXY 420F"),
    (0x03190000, "PowerShot ELPH 110 HS / IXUS 125 HS / IXY 220F"),
    (0x03200000, "PowerShot D20"),
    (0x03210000, "PowerShot A4000 IS"),
    (0x03220000, "PowerShot SX260 HS"),
    (0x03230000, "PowerShot SX240 HS"),
    (0x03240000, "PowerShot ELPH 530 HS / IXUS 510 HS / IXY 1"),
    (0x03250000, "PowerShot ELPH 520 HS / IXUS 500 HS / IXY 3"),
    (0x03260000, "PowerShot A3400 IS"),
    (0x03270000, "PowerShot A2400 IS"),
    (0x03280000, "PowerShot A2300"),
    (0x03320000, "PowerShot S100V"),
    (0x03330000, "PowerShot G15"),
    (0x03340000, "PowerShot SX50 HS"),
    (0x03350000, "PowerShot SX160 IS"),
    (0x03360000, "PowerShot S110 (new)"),
    (0x03370000, "PowerShot SX500 IS"),
    (0x03380000, "PowerShot N"),
    (0x03390000, "IXUS 245 HS / IXY 430F"),
    (0x03400000, "PowerShot SX280 HS"),
    (0x03410000, "PowerShot SX270 HS"),
    (0x03420000, "PowerShot A3500 IS"),
    (0x03430000, "PowerShot A2600"),
    (0x03440000, "PowerShot SX275 HS"),
    (0x03450000, "PowerShot A1400"),
    (0x03460000, "PowerShot ELPH 130 IS / IXUS 140 / IXY 110F"),
    (
        0x03470000,
        "PowerShot ELPH 115/120 IS / IXUS 132/135 / IXY 90F/100F",
    ),
    (0x03490000, "PowerShot ELPH 330 HS / IXUS 255 HS / IXY 610F"),
    (0x03510000, "PowerShot A2500"),
    (0x03540000, "PowerShot G16"),
    (0x03550000, "PowerShot S120"),
    (0x03560000, "PowerShot SX170 IS"),
    (0x03580000, "PowerShot SX510 HS"),
    (0x03590000, "PowerShot S200 (new)"),
    (0x03600000, "IXY 620F"),
    (0x03610000, "PowerShot N100"),
    (0x03640000, "PowerShot G1 X Mark II"),
    (0x03650000, "PowerShot D30"),
    (0x03660000, "PowerShot SX700 HS"),
    (0x03670000, "PowerShot SX600 HS"),
    (0x03680000, "PowerShot ELPH 140 IS / IXUS 150 / IXY 130"),
    (0x03690000, "PowerShot ELPH 135 / IXUS 145 / IXY 120"),
    (0x03700000, "PowerShot ELPH 340 HS / IXUS 265 HS / IXY 630"),
    (0x03710000, "PowerShot ELPH 150 IS / IXUS 155 / IXY 140"),
    (0x03740000, "EOS M3"),
    (0x03750000, "PowerShot SX60 HS"),
    (0x03760000, "PowerShot SX520 HS"),
    (0x03770000, "PowerShot SX400 IS"),
    (0x03780000, "PowerShot G7 X"),
    (0x03790000, "PowerShot N2"),
    (0x03800000, "PowerShot SX530 HS"),
    (0x03820000, "PowerShot SX710 HS"),
    (0x03830000, "PowerShot SX610 HS"),
    (0x03840000, "EOS M10"),
    (0x03850000, "PowerShot G3 X"),
    (0x03860000, "PowerShot ELPH 165 HS / IXUS 165 / IXY 160"),
    (0x03870000, "PowerShot ELPH 160 / IXUS 160"),
    (0x03880000, "PowerShot ELPH 350 HS / IXUS 275 HS / IXY 640"),
    (0x03890000, "PowerShot ELPH 170 IS / IXUS 170"),
    (0x03910000, "PowerShot SX410 IS"),
    (0x03930000, "PowerShot G9 X"),
    (0x03940000, "EOS M5"),
    (0x03950000, "PowerShot G5 X"),
    (0x03970000, "PowerShot G7 X Mark II"),
    (0x03980000, "EOS M100"),
    (0x03990000, "PowerShot ELPH 360 HS / IXUS 285 HS / IXY 650"),
    (0x04010000, "PowerShot SX540 HS"),
    (0x04020000, "PowerShot SX420 IS"),
    (0x04030000, "PowerShot ELPH 190 IS / IXUS 180 / IXY 190"),
    (0x04040000, "PowerShot G1"),
    (0x04040001, "PowerShot ELPH 180 IS / IXUS 175 / IXY 180"),
    (0x04050000, "PowerShot SX720 HS"),
    (0x04060000, "PowerShot SX620 HS"),
    (0x04070000, "EOS M6"),
    (0x04100000, "PowerShot G9 X Mark II"),
    (0x00000412, "EOS M50 / Kiss M"),
    (0x04150000, "PowerShot ELPH 185 / IXUS 185 / IXY 200"),
    (0x04160000, "PowerShot SX430 IS"),
    (0x04170000, "PowerShot SX730 HS"),
    (0x04180000, "PowerShot G1 X Mark III"),
    (0x06040000, "PowerShot S100 / Digital IXUS / IXY Digital"),
    (0x00000801, "PowerShot SX740 HS"),
    (0x00000804, "PowerShot G5 X Mark II"),
    (0x00000805, "PowerShot SX70 HS"),
    (0x00000808, "PowerShot G7 X Mark III"),
    (0x00000811, "EOS M6 Mark II"),
    (0x00000812, "EOS M200"),
    (0x40000227, "EOS C50"),
    (0x4007d673, "DC19/DC21/DC22"),
    (0x4007d674, "XH A1"),
    (0x4007d675, "HV10"),
    (0x4007d676, "MD130/MD140/MD150/MD160/ZR850"),
    (0x4007d777, "DC50"),
    (0x4007d778, "HV20"),
    (0x4007d779, "DC211"),
    (0x4007d77a, "HG10"),
    (0x4007d77b, "HR10"),
    (0x4007d77d, "MD255/ZR950"),
    (0x4007d81c, "HF11"),
    (0x4007d878, "HV30"),
    (0x4007d87c, "XH A1S"),
    (0x4007d87e, "DC301/DC310/DC311/DC320/DC330"),
    (0x4007d87f, "FS100"),
    (0x4007d880, "HF10"),
    (0x4007d882, "HG20/HG21"),
    (0x4007d925, "HF21"),
    (0x4007d926, "HF S11"),
    (0x4007d978, "HV40"),
    (0x4007d987, "DC410/DC411/DC420"),
    (0x4007d988, "FS19/FS20/FS21/FS22/FS200"),
    (0x4007d989, "HF20/HF200"),
    (0x4007d98a, "HF S10/S100"),
    (0x4007da8e, "HF R10/R16/R17/R18/R100/R106"),
    (0x4007da8f, "HF M30/M31/M36/M300/M306"),
    (0x4007da90, "HF S20/S21/S200"),
    (0x4007da92, "FS31/FS36/FS37/FS300/FS305/FS306/FS307"),
    (0x4007dca0, "EOS C300"),
    (0x4007dda9, "HF G25"),
    (0x4007dfb4, "XC10"),
    (0x4007e1c3, "EOS C200"),
    (0x80000001, "EOS-1D"),
    (0x80000167, "EOS-1DS"),
    (0x80000168, "EOS 10D"),
    (0x80000169, "EOS-1D Mark III"),
    (0x80000170, "EOS Digital Rebel / 300D / Kiss Digital"),
    (0x80000174, "EOS-1D Mark II"),
    (0x80000175, "EOS 20D"),
    (0x80000176, "EOS Digital Rebel XSi / 450D / Kiss X2"),
    (0x80000188, "EOS-1Ds Mark II"),
    (0x80000189, "EOS Digital Rebel XT / 350D / Kiss Digital N"),
    (0x80000190, "EOS 40D"),
    (0x80000213, "EOS 5D"),
    (0x80000215, "EOS-1Ds Mark III"),
    (0x80000218, "EOS 5D Mark II"),
    (0x80000219, "WFT-E1"),
    (0x80000232, "EOS-1D Mark II N"),
    (0x80000234, "EOS 30D"),
    (0x80000236, "EOS Digital Rebel XTi / 400D / Kiss Digital X"),
    (0x80000241, "WFT-E2"),
    (0x80000246, "WFT-E3"),
    (0x80000250, "EOS 7D"),
    (0x80000252, "EOS Rebel T1i / 500D / Kiss X3"),
    (0x80000254, "EOS Rebel XS / 1000D / Kiss F"),
    (0x80000261, "EOS 50D"),
    (0x80000269, "EOS-1D X"),
    (0x80000270, "EOS Rebel T2i / 550D / Kiss X4"),
    (0x80000271, "WFT-E4"),
    (0x80000273, "WFT-E5"),
    (0x80000281, "EOS-1D Mark IV"),
    (0x80000285, "EOS 5D Mark III"),
    (0x80000286, "EOS Rebel T3i / 600D / Kiss X5"),
    (0x80000287, "EOS 60D"),
    (0x80000288, "EOS Rebel T3 / 1100D / Kiss X50"),
    (0x80000289, "EOS 7D Mark II"),
    (0x80000297, "WFT-E2 II"),
    (0x80000298, "WFT-E4 II"),
    (0x80000301, "EOS Rebel T4i / 650D / Kiss X6i"),
    (0x80000302, "EOS 6D"),
    (0x80000324, "EOS-1D C"),
    (0x80000325, "EOS 70D"),
    (0x80000326, "EOS Rebel T5i / 700D / Kiss X7i"),
    (0x80000327, "EOS Rebel T5 / 1200D / Kiss X70 / Hi"),
    (0x80000328, "EOS-1D X Mark II"),
    (0x80000331, "EOS M"),
    (0x80000350, "EOS 80D"),
    (0x80000355, "EOS M2"),
    (0x80000346, "EOS Rebel SL1 / 100D / Kiss X7"),
    (0x80000347, "EOS Rebel T6s / 760D / 8000D"),
    (0x80000349, "EOS 5D Mark IV"),
    (0x80000382, "EOS 5DS"),
    (0x80000393, "EOS Rebel T6i / 750D / Kiss X8i"),
    (0x80000401, "EOS 5DS R"),
    (0x80000404, "EOS Rebel T6 / 1300D / Kiss X80"),
    (0x80000405, "EOS Rebel T7i / 800D / Kiss X9i"),
    (0x80000406, "EOS 6D Mark II"),
    (0x80000408, "EOS 77D / 9000D"),
    (0x80000417, "EOS Rebel SL2 / 200D / Kiss X9"),
    (0x80000421, "EOS R5"),
    (0x80000422, "EOS Rebel T100 / 4000D / 3000D"),
    (0x80000424, "EOS R"),
    (0x80000428, "EOS-1D X Mark III"),
    (0x80000432, "EOS Rebel T7 / 2000D / 1500D / Kiss X90"),
    (0x80000433, "EOS RP"),
    (0x80000435, "EOS Rebel T8i / 850D / X10i"),
    (0x80000436, "EOS SL3 / 250D / Kiss X10"),
    (0x80000437, "EOS 90D"),
    (0x80000450, "EOS R3"),
    (0x80000453, "EOS R6"),
    (0x80000464, "EOS R7"),
    (0x80000465, "EOS R10"),
    (0x80000467, "PowerShot ZOOM"),
    (0x80000468, "EOS M50 Mark II / Kiss M2"),
    (0x80000480, "EOS R50"),
    (0x80000481, "EOS R6 Mark II"),
    (0x80000487, "EOS R8"),
    (0x80000491, "PowerShot V10"),
    (0x80000495, "EOS R1"),
    (0x80000496, "EOS R5 Mark II"),
    (0x80000497, "PowerShot V1"),
    (0x80000498, "EOS R100"),
    (0x80000516, "EOS R50 V"),
    (0x80000518, "EOS R6 Mark III"),
    (0x80000520, "EOS D2000C"),
    (0x80000560, "EOS D6000C"),
];

/// Decodes a Canon Model ID to the corresponding camera model name.
///
/// Canon cameras store a numeric model identifier in the MakerNotes which
/// uniquely identifies the camera model. This function translates that
/// numeric ID into a human-readable camera name, matching ExifTool's output.
///
/// # Parameters
/// - `model_id`: The raw 32-bit Canon Model ID value
///
/// # Returns
/// A string containing the camera model name. For unknown IDs, returns
/// "Unknown ({id})" where {id} is the decimal value.
///
/// # Examples
/// ```
/// use oxidex::parsers::tiff::makernotes::canon::decode_canon_model_id;
///
/// // PowerShot S40 has model ID 0x1110000 (17891328 decimal)
/// assert_eq!(decode_canon_model_id(0x1110000), "PowerShot S40");
/// assert_eq!(decode_canon_model_id(17891328), "PowerShot S40");
///
/// // 0x80000281 is the EOS-1D Mark IV -- the 5D Mark III is 0x80000285. The
/// // hand-maintained table this replaced had these two swapped.
/// assert_eq!(decode_canon_model_id(0x80000281), "EOS-1D Mark IV");
/// assert_eq!(decode_canon_model_id(0x80000285), "EOS 5D Mark III");
/// ```
pub fn decode_canon_model_id(model_id: u32) -> String {
    CANON_MODEL_IDS
        .iter()
        .find(|(id, _)| *id == model_id)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| format!("Unknown ({})", model_id))
}

/// Decodes `%Canon::ShotInfo` key 26 `CameraType` (Canon.pm:3011).
///
/// ```text
///     PrintConv => {
///         0 => 'n/a',       248 => 'EOS High-end',   250 => 'Compact',
///         252 => 'EOS Mid-range',                    255 => 'DV Camera',
///     },
/// ```
///
/// This tag is a ShotInfo slot, **not** a function of `CanonModelID`. Deriving it from
/// the model id instead -- as this parser used to -- reported "Unknown" for all 406
/// camcorder samples in the corpus, where the real value is the literal 255 written in
/// the record.
pub fn decode_camera_type(raw_value: i16) -> String {
    match raw_value {
        0 => "n/a".to_string(),
        248 => "EOS High-end".to_string(),
        250 => "Compact".to_string(),
        252 => "EOS Mid-range".to_string(),
        255 => "DV Camera".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// Represents a Canon MakerNote tag value
#[derive(Debug, Clone, PartialEq)]
pub enum CanonTagValue {
    /// Single integer value
    Integer(i32),
    /// String value (model name, firmware, etc.)
    String(String),
    /// Array of integers (camera settings, shot info)
    IntArray(Vec<i16>),
}

/// Maps Canon MakerNote tag IDs to human-readable tag names.
///
/// # Parameters
/// - `tag_id`: The Canon-specific tag ID
///
/// # Returns
/// Tag name in the format "Canon:TagName"
///
/// # Example
/// ```
/// use oxidex::parsers::tiff::makernotes::canon::canon_tag_to_name;
/// assert_eq!(canon_tag_to_name(0x0001), "Canon:CameraSettings");
/// ```
pub fn canon_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        CANON_CAMERA_SETTINGS => "CameraSettings",
        CANON_FOCAL_LENGTH => "FocalLength",
        CANON_FLASH_INFO => "FlashInfo",
        CANON_SHOT_INFO => "ShotInfo",
        CANON_PANORAMA => "Panorama",
        CANON_IMAGE_TYPE => "ImageType",
        CANON_FIRMWARE_VERSION => "FirmwareVersion",
        CANON_FILE_NUMBER => "FileNumber",
        CANON_OWNER_NAME => "OwnerName",
        CANON_SERIAL_NUMBER => "SerialNumber",
        CANON_CAMERA_INFO => "CameraInfo",
        CANON_CUSTOM_FUNCTIONS => "CustomFunctions",
        CANON_MODEL_ID => "CanonModelID",
        CANON_AF_INFO => "AFInfo",
        CANON_SERIAL_NUMBER_FORMAT => "SerialNumberFormat",
        CANON_AF_INFO2 => "AFInfo2",
        CANON_AF_INFO3 => "AFInfo3",
        CANON_FILE_INFO => "FileInfo",
        CANON_LENS_MODEL => "LensModel",
        CANON_INTERNAL_SERIAL_NUMBER => "InternalSerialNumber",
        CANON_PROCESSING_INFO => "ProcessingInfo",
        CANON_MEASURED_COLOR => "MeasuredColor",
        CANON_COLOR_SPACE => "ColorSpace",
        CANON_VRD_OFFSET => "VRDOffset",
        _ => return format!("Canon:Unknown-{:#06X}", tag_id),
    };

    format!("Canon:{}", tag_name)
}

/// Represents a Canon MakerNote parser
pub struct CanonParser;

impl MakerNoteParser for CanonParser {
    fn manufacturer_name(&self) -> &'static str {
        "Canon"
    }

    fn tag_prefix(&self) -> &'static str {
        "Canon:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_canon_makernote(data)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // Call the existing parse_canon_makernote function and handle Result conversion
        match parse_canon_makernote_impl(data, byte_order) {
            Ok(parsed_tags) => {
                tags.extend(parsed_tags);
                Ok(())
            }
            Err(e) => Err(format!("Canon MakerNote parse error: {}", e)),
        }
    }

    /// Canon's model-conditional tables are selected from the EXIF `Model`, so
    /// Canon takes the model when the dispatcher has it.
    fn parse_with_model(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        match parse_canon_makernote_impl_with_model(data, byte_order, model) {
            Ok(parsed_tags) => {
                tags.extend(parsed_tags);
                Ok(())
            }
            Err(e) => Err(format!("Canon MakerNote parse error: {}", e)),
        }
    }

    /// Takes the MakerNote's position in its enclosing TIFF block, because that
    /// position *is* the base its value offsets are measured from.
    ///
    /// `ProcessExif` resolves a value at `$valuePtr - $dataPos` (Exif.pm:6447),
    /// and for a MakerNote inside a loaded EXIF block `$dataPos` is the
    /// directory's own offset. Only Canon's TIFF footer moves it, which
    /// [`canon_makernote_base_located`] reads. Without the position a decoder
    /// has to recover the base from the record bytes, which is right on 639 of
    /// the corpus's 670 Canon MakerNotes and wrong on 31.
    ///
    /// Both slices are handed over: values resolve against the window, because
    /// `ProcessExif` resolves them against the whole loaded block rather than
    /// the entry's declared extent, but the footer is looked for in the declared
    /// block, whose last eight bytes it is.
    fn parse_with_context(
        &self,
        ctx: &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        match parse_canon_makernote_impl_located(
            ctx.window(),
            ctx.payload(),
            byte_order,
            model,
            ctx.payload_tiff_offset(),
        ) {
            Ok(parsed_tags) => {
                tags.extend(parsed_tags);
                Ok(())
            }
            Err(e) => Err(format!("Canon MakerNote parse error: {}", e)),
        }
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

/// Checks if data appears to be a Canon MakerNote.
///
/// Canon MakerNotes may optionally start with "Canon" signature,
/// but always contain a valid IFD structure.
///
/// # Parameters
/// - `data`: Raw byte data to check
///
/// # Returns
/// `true` if the data appears to be a Canon MakerNote, `false` otherwise
pub fn is_canon_makernote(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // Check for optional Canon signature
    if data.starts_with(CANON_SIGNATURE) {
        return true;
    }

    // Check if it looks like an IFD (starts with entry count)
    // Valid IFD has at least 2 bytes for entry count
    // Try both little-endian and big-endian interpretations
    if data.len() >= 2 {
        let le_reader = EndianReader::little_endian(data);
        let be_reader = EndianReader::big_endian(data);
        let entry_count_le = le_reader.u16_at(0).unwrap_or(0);
        let entry_count_be = be_reader.u16_at(0).unwrap_or(0);

        // Reasonable entry count (Canon typically has 1-100 entries)
        // Accept if either byte order yields a reasonable count
        let is_reasonable = |count: u16| count > 0 && count < 200;

        return is_reasonable(entry_count_le) || is_reasonable(entry_count_be);
    }

    false
}

/// Internal implementation of Canon MakerNote parsing.
///
/// This parser extracts tags from Canon MakerNotes including simple tags
/// (strings and integers) and complex array tags (CameraSettings, ShotInfo, etc.).
///
/// # Parameters
/// - `data`: Raw MakerNote data (may include Canon signature)
/// - `byte_order`: Byte order for parsing (usually matches TIFF header)
///
/// # Returns
/// HashMap of tag names to string values
///
/// # Errors
/// Returns error if IFD parsing fails or data is invalid
fn parse_canon_makernote_impl(
    data: &[u8],
    byte_order: ByteOrder,
) -> Result<HashMap<String, String>> {
    parse_canon_makernote_impl_with_model(data, byte_order, None)
}

/// [`parse_canon_makernote_impl_with_model`] for a caller that does not know
/// where the MakerNote sits.
fn parse_canon_makernote_impl_with_model(
    data: &[u8],
    byte_order: ByteOrder,
    exif_model: Option<&str>,
) -> Result<HashMap<String, String>> {
    parse_canon_makernote_impl_located(data, data, byte_order, exif_model, None)
}

/// Same as [`parse_canon_makernote_impl`], plus the EXIF `Model` the dispatcher
/// resolved from IFD0.
///
/// ExifTool selects model-conditional tables from `$$self{Model}`, and
/// nothing inside the MakerNote is a reliable stand-in: `CanonImageType` agrees
/// with `Model` on 492 of the 493 ExifTool sample files that carry a CameraInfo
/// record, but the Kiss X70 writes "Canon EOS X70" there and would dispatch
/// nowhere. `Model` is used when it is available and `CanonImageType` is the
/// fallback for callers that do not have it.
///
/// `dir_tiff_offset` is the MakerNote's offset inside its enclosing TIFF block
/// when the caller knows it, which is what makes the base a fact rather than an
/// inference -- see [`canon_makernote_base_located`].
///
/// `data` is what entries and values are read from and `declared` the
/// MakerNote's declared block. They differ when the caller can offer the wider
/// window: `CanonMVX100i.jpg` declares 42 MakerNote bytes for an IFD whose five
/// entries alone need 66, so `CanonImageType` and `OwnerName` sit past the
/// declared end and only the window reaches them. The footer is still looked for
/// in `declared`, because that is the block whose last eight bytes it is.
fn parse_canon_makernote_impl_located(
    data: &[u8],
    declared: &[u8],
    byte_order: ByteOrder,
    exif_model: Option<&str>,
    dir_tiff_offset: Option<u32>,
) -> Result<HashMap<String, String>> {
    if data.is_empty() {
        return Ok(HashMap::new());
    }

    let mut tags = HashMap::new();

    let config = IfdParserConfig {
        signature: Some(CANON_SIGNATURE),
        signature_offset: CANON_SIGNATURE.len(),
        max_entries: 200,
    };

    // `MakerNoteCanon` declares `ByteOrder => 'Unknown'` (MakerNotes.pm:67), so
    // the directory's endianness is its own, not the enclosing TIFF's, and has
    // to be resolved from its entry count before anything else is read.
    let byte_order = resolve_makernote_byte_order(data, &config, byte_order);

    // Canon value offsets are TIFF-relative but the dispatcher hands this parser a
    // detached slice, so the base has to be recovered from the MakerNote's own
    // bytes. Resolved once here and threaded through every extractor below --
    // see `calculate_makernote_base` for why the packed-layout guess is not enough
    // and how the records vote for the real base.
    let base = canon_makernote_base(data, declared, byte_order, &config, dir_tiff_offset);

    // Several `%Canon` keys are model-conditional (`%Canon::FileInfo` key 1,
    // `%Canon::ShotInfo` key 22, `%Canon::Processing` key 2, `%Canon::FocalLength` keys
    // 2-3, and the `CustomFunctions*` table selection). ExifTool reads `$$self{Model}`
    // from IFD0, which the MakerNote dispatcher does not pass down; CanonImageType
    // (MakerNote tag 0x0006) carries the same body name, so resolve it up front rather
    // than relying on the order entries happen to arrive in.
    let mut model = String::new();
    // `%Canon::CameraSettings` key 22 is ExifTool's `LensType` DATAMEMBER, which
    // `%Canon::CameraInfo*` MacroMagnification conditions on. It is read in the
    // same pre-pass so the CameraInfo record does not depend on IFD entry order.
    let mut camera_settings_lens_type: Option<i64> = None;
    let _ = parse_ifd_entries(data, byte_order, &config, |entry, ifd_data| {
        match entry.tag_id {
            CANON_IMAGE_TYPE => {
                if let Some(value) =
                    extract_canon_string_with_base(entry, ifd_data, byte_order, base)
                {
                    model = value;
                }
            }
            CANON_CAMERA_SETTINGS => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                    && let Some(&lens) = array.get(CAMERA_SETTINGS_LENS_TYPE)
                {
                    camera_settings_lens_type = Some(lens as i64);
                }
            }
            _ => {}
        }
    });
    let model = model;
    let camera_settings_lens_type = camera_settings_lens_type;
    let camera_info_model = exif_model.filter(|m| !m.is_empty()).unwrap_or(&model);

    // `%Canon::CameraInfo*` is PRIORITY => 0, so its values are collected apart
    // and only fill names no other Canon table produced. See `merge_priority0`.
    let mut camera_info_tags: HashMap<String, String> = HashMap::new();

    // `%Canon::LensInfo` key 0 `LensSerialNumber` is `Priority => 0` too: some
    // `%Canon::CameraInfo*` tables (CameraInfo1DX, CameraInfo5DmkIII) carry their
    // own `LensSerialNumber` field, and ExifTool reads tag 0x000D before 0x4019,
    // so a CameraInfo value must win that collision. Deferred and merged after
    // `camera_info_tags` below for the same reason.
    let mut lens_info_serial: Option<String> = None;

    // FocalUnits (`%Canon::CameraSettings` key 25) divides `%Canon::FocalLength` key 1.
    let mut focal_units: i16 = 1;

    // Use shared IFD parser
    // Note: we don't propagate errors here to maintain existing behavior of
    // returning whatever tags we found even if parsing isn't perfect
    let _ = parse_ifd_entries(data, byte_order, &config, |entry, ifd_data| {
        match entry.tag_id {
            // Simple string tags (Phase 1)
            // Canon MakerNotes use TIFF-relative offsets, so we use extract_canon_string
            // which properly calculates and applies the base offset
            //
            // The emitted names are ExifTool's own: tag 0x0006 is `CanonImageType` and
            // 0x0007 is `CanonFirmwareVersion` (Canon.pm:1252/1256). The bare
            // `FirmwareVersion` and `ImageType` aliases this used to add alongside them
            // are not ExifTool names for these tags -- `FirmwareVersion` is a CameraInfo
            // tag holding just "1.1.0", so aliasing 0x0007's "Firmware Version 1.1.0"
            // onto it reported a wrong value on every body that has both.
            CANON_IMAGE_TYPE | CANON_FIRMWARE_VERSION | CANON_OWNER_NAME => {
                if let Some(value) =
                    extract_canon_string_with_base(entry, ifd_data, byte_order, base)
                {
                    let tag_name = match entry.tag_id {
                        CANON_IMAGE_TYPE => "Canon:CanonImageType",
                        CANON_FIRMWARE_VERSION => "Canon:CanonFirmwareVersion",
                        _ => "Canon:OwnerName",
                    };
                    tags.insert(tag_name.to_string(), value);
                }
            }

            // SerialNumber (tag 0x000C) is an int32u, not a string.
            //
            // ExifTool Canon.pm:1299 (the fall-through variant used by every body except
            // the D30 and the EOS-1D family):
            //
            // ```text
            //     Name => 'SerialNumber',
            //     Writable => 'int32u',
            //     PrintConv => 'sprintf("%.10u",$val)',
            // ```
            CANON_SERIAL_NUMBER => {
                let serial = entry.value_offset;
                let rendered = if has_word(&model, "EOS D30") {
                    format!("{:04x}{:05}", serial >> 16, serial & 0xffff)
                } else if model.contains("EOS-1D") {
                    format!("{:06}", serial)
                } else {
                    format!("{:010}", serial)
                };
                tags.insert("Canon:SerialNumber".to_string(), rendered);
            }

            // SerialNumberFormat (tag 0x0015) - int32u display-format selector
            CANON_SERIAL_NUMBER_FORMAT => {
                tags.insert(
                    "Canon:SerialNumberFormat".to_string(),
                    SERIAL_NUMBER_FORMAT.decode(entry.value_offset as i64),
                );
            }

            // ThumbnailImageValidArea (tag 0x0013) - int16u[4] crop box
            CANON_THUMBNAIL_IMAGE_VALID_AREA => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                    && array.len() >= 4
                {
                    tags.insert(
                        "Canon:ThumbnailImageValidArea".to_string(),
                        join_i16_slice(&array[..4]),
                    );
                }
            }

            // OriginalDecisionDataOffset (tag 0x0083) and VRDOffset (tag 0x00D0) are
            // plain int32u offsets that ExifTool reports verbatim.
            CANON_ORIGINAL_DECISION_DATA_OFFSET => {
                tags.insert(
                    "Canon:OriginalDecisionDataOffset".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            CANON_VRD_OFFSET => {
                tags.insert(
                    "Canon:VRDOffset".to_string(),
                    entry.value_offset.to_string(),
                );
            }

            // Canon Model ID - decode to camera model name
            // The model ID is stored as a 32-bit integer that maps to specific camera models
            CANON_MODEL_ID => {
                // The value_offset contains the model ID directly for LONG type (4 bytes)
                let model_id = entry.value_offset;
                let model_name = decode_canon_model_id(model_id);
                tags.insert("Canon:CanonModelID".to_string(), model_name);
            }

            // FileNumber (tag 0x0008) - int32u, ExifTool Canon.pm:1260 renders it as
            // `directory-file` via `s/(\d+)(\d{4})/$1-$2/`.
            CANON_FILE_NUMBER => {
                tags.insert(
                    "Canon:FileNumber".to_string(),
                    format_canon_file_number(entry.value_offset),
                );
            }

            // CameraSettings array (Phase 2)
            // Reference: ExifTool Canon.pm CameraSettings table
            CANON_CAMERA_SETTINGS => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                {
                    // Extract specific settings from array using const decoders
                    // Note: All tag names use "Canon:" prefix for consistency

                    // MacroMode (index 1) - Macro shooting mode
                    if array.len() > CAMERA_SETTINGS_MACRO_MODE {
                        tags.insert(
                            "Canon:MacroMode".to_string(),
                            MACRO_MODE.decode(array[CAMERA_SETTINGS_MACRO_MODE]),
                        );
                    }

                    // SelfTimer (index 2). ExifTool `%Canon::CameraSettings` key 2
                    // (Canon.pm:2229):
                    //
                    // ```text
                    //     return 'Off' unless $val;
                    //     return (($val&0xfff) / 10) . ' s' . ($val & 0x4000 ? ', Custom' : '');
                    // ```
                    //
                    // The delay is the low 12 bits only, the unit is " s" (not " sec"),
                    // and Perl's numeric stringification drops a trailing ".0", so 20
                    // prints as "2 s" rather than "2.0 sec".
                    if let Some(&raw_timer) = array.get(CAMERA_SETTINGS_SELF_TIMER) {
                        let raw_timer = raw_timer as u16;
                        let rendered = if raw_timer == 0 {
                            "Off".to_string()
                        } else {
                            let custom = if raw_timer & 0x4000 != 0 {
                                ", Custom"
                            } else {
                                ""
                            };
                            format!(
                                "{} s{}",
                                format_perl_number(f64::from(raw_timer & 0xfff) / 10.0),
                                custom
                            )
                        };
                        tags.insert("Canon:SelfTimer".to_string(), rendered);
                    }

                    // Quality (index 3) - Image quality setting
                    if array.len() > CAMERA_SETTINGS_QUALITY {
                        tags.insert(
                            "Canon:Quality".to_string(),
                            QUALITY.decode(array[CAMERA_SETTINGS_QUALITY]),
                        );
                    }

                    // CanonFlashMode (index 4) - Flash mode setting
                    // Also output as Canon:FlashMode for backward compatibility
                    if array.len() > CAMERA_SETTINGS_FLASH_MODE {
                        let flash_mode = FLASH_MODE.decode(array[CAMERA_SETTINGS_FLASH_MODE]);
                        tags.insert("Canon:CanonFlashMode".to_string(), flash_mode.clone());
                        tags.insert("Canon:FlashMode".to_string(), flash_mode);
                    }

                    // ContinuousDrive (index 5) - Drive mode setting
                    // Also output as Canon:DriveMode for backward compatibility
                    if array.len() > CAMERA_SETTINGS_DRIVE_MODE {
                        let drive_mode = DRIVE_MODE.decode(array[CAMERA_SETTINGS_DRIVE_MODE]);
                        tags.insert("Canon:ContinuousDrive".to_string(), drive_mode.clone());
                        tags.insert("Canon:DriveMode".to_string(), drive_mode);
                    }

                    // FocusMode (index 7) - Focus mode setting
                    if array.len() > CAMERA_SETTINGS_FOCUS_MODE {
                        tags.insert(
                            "Canon:FocusMode".to_string(),
                            FOCUS_MODE.decode(array[CAMERA_SETTINGS_FOCUS_MODE]),
                        );
                    }

                    // RecordMode (index 9). `RawConv => '$val==-1 ? undef : $val'`
                    // (Canon.pm:2297).
                    if let Some(&record_mode) = array.get(CAMERA_SETTINGS_RECORD_MODE)
                        && record_mode != -1
                    {
                        tags.insert(
                            "Canon:RecordMode".to_string(),
                            RECORD_MODE.decode(record_mode),
                        );
                    }

                    // CanonImageSize (index 10) - Image size setting
                    if array.len() > CAMERA_SETTINGS_IMAGE_SIZE {
                        tags.insert(
                            "Canon:CanonImageSize".to_string(),
                            CANON_IMAGE_SIZE.decode(array[CAMERA_SETTINGS_IMAGE_SIZE]),
                        );
                    }

                    // EasyMode (index 11) - Scene mode / Easy mode setting
                    if array.len() > CAMERA_SETTINGS_EASY_MODE {
                        tags.insert(
                            "Canon:EasyMode".to_string(),
                            EASY_MODE.decode(array[CAMERA_SETTINGS_EASY_MODE]),
                        );
                    }

                    // DigitalZoom (index 12) - Digital zoom setting
                    if array.len() > CAMERA_SETTINGS_DIGITAL_ZOOM {
                        tags.insert(
                            "Canon:DigitalZoom".to_string(),
                            DIGITAL_ZOOM.decode(array[CAMERA_SETTINGS_DIGITAL_ZOOM]),
                        );
                    }

                    // Contrast (index 13) and Saturation (index 14). ExifTool
                    // (Canon.pm:2384/2392) gives both `RawConv => '$val == 0x7fff ? undef
                    // : $val'` and the shared `%printParameter`, so each is a signed
                    // adjustment printed with its sign -- not a Low/Normal/High band.
                    if let Some(&contrast) = array.get(CAMERA_SETTINGS_CONTRAST)
                        && contrast != 0x7fff
                    {
                        tags.insert("Canon:Contrast".to_string(), print_parameter(contrast));
                    }

                    if let Some(&saturation) = array.get(CAMERA_SETTINGS_SATURATION)
                        && saturation != 0x7fff
                    {
                        tags.insert("Canon:Saturation".to_string(), print_parameter(saturation));
                    }

                    // Sharpness (index 15). ExifTool `%Canon::CameraSettings` key 15
                    // (Canon.pm:2404): `RawConv => '$val == 0x7fff ? undef : $val'`,
                    // `PrintConv => '$val > 0 ? "+$val" : $val'` -- a positive value
                    // carries an explicit plus sign, and the 0x7fff sentinel is dropped.
                    if let Some(&sharpness) = array.get(CAMERA_SETTINGS_SHARPNESS)
                        && sharpness != 0x7fff
                    {
                        let rendered = if sharpness > 0 {
                            format!("+{}", sharpness)
                        } else {
                            sharpness.to_string()
                        };
                        tags.insert("Canon:Sharpness".to_string(), rendered);
                    }

                    // CameraISO (index 16). ExifTool's RawConv drops the 0x7fff
                    // "not present" sentinel instead of reporting it as an ISO.
                    if let Some(&raw_iso) = array.get(CAMERA_SETTINGS_ISO)
                        && let Some(iso) = camera_iso(raw_iso)
                    {
                        tags.insert("Canon:CameraISO".to_string(), iso);
                    }

                    // MeteringMode (index 17) - Metering mode setting
                    if array.len() > CAMERA_SETTINGS_METERING_MODE {
                        tags.insert(
                            "Canon:MeteringMode".to_string(),
                            METERING_MODE.decode(array[CAMERA_SETTINGS_METERING_MODE]),
                        );
                    }

                    // FocusRange (index 18) - Focus range/type setting
                    if array.len() > CAMERA_SETTINGS_FOCUS_RANGE {
                        tags.insert(
                            "Canon:FocusRange".to_string(),
                            FOCUS_RANGE.decode(array[CAMERA_SETTINGS_FOCUS_RANGE]),
                        );
                    }

                    // AFPoint (index 19). ExifTool's `RawConv => '$val==0 ? undef : $val'`
                    // suppresses the tag entirely on bodies that leave the slot at zero.
                    if let Some(&af_point) = array.get(CAMERA_SETTINGS_AF_POINT)
                        && af_point != 0
                    {
                        tags.insert("Canon:AFPoint".to_string(), AF_POINT.decode(af_point));
                    }

                    // CanonExposureMode (index 20) - Exposure mode setting
                    // Also output as Canon:ExposureMode for backward compatibility
                    if array.len() > CAMERA_SETTINGS_EXPOSURE_MODE {
                        let exposure_mode =
                            EXPOSURE_MODE.decode(array[CAMERA_SETTINGS_EXPOSURE_MODE]);
                        tags.insert("Canon:CanonExposureMode".to_string(), exposure_mode.clone());
                        tags.insert("Canon:ExposureMode".to_string(), exposure_mode);
                    }

                    // LensType (`%Canon::CameraSettings` key 22, Canon.pm:2499):
                    //
                    // ```text
                    //     Format => 'int16u',
                    //     RawConv => '$val ? $$self{LensType}=$val : undef',
                    //     PrintConv => \%canonLensTypes,
                    // ```
                    //
                    // The slot is int16u, so 0xFFFF is 65535 - the key ExifTool
                    // files "n/a" under - and not -1. Zero is vetoed by the
                    // RawConv and suppresses the tag entirely rather than
                    // printing "n/a"; a body with no lens reports 0xFFFF, not 0.
                    // An id the table does not carry gets ExifTool's ordinary
                    // unmatched-PrintConv rendering.
                    if let Some(&raw) = array.get(CAMERA_SETTINGS_LENS_TYPE) {
                        let lens_id = raw as u16;
                        if lens_id != 0 {
                            tags.insert(
                                "Canon:LensType".to_string(),
                                lookup_lens_name(lens_id)
                                    .unwrap_or_else(|| format!("Unknown ({})", lens_id)),
                            );
                        }
                    }

                    // Get focal units for focal length calculations (index 25)
                    focal_units = if array.len() > CAMERA_SETTINGS_FOCAL_UNITS {
                        let units = array[CAMERA_SETTINGS_FOCAL_UNITS];
                        if units > 0 { units } else { 1 }
                    } else {
                        1
                    };

                    // FocalUnits (index 25) - Units per mm for focal length
                    if array.len() > CAMERA_SETTINGS_FOCAL_UNITS {
                        tags.insert(
                            "Canon:FocalUnits".to_string(),
                            format!("{}/mm", focal_units),
                        );
                    }

                    // MaxFocalLength (index 23) - Maximum focal length
                    if array.len() > CAMERA_SETTINGS_MAX_FOCAL_LENGTH {
                        tags.insert(
                            "Canon:MaxFocalLength".to_string(),
                            format_focal_length(
                                array[CAMERA_SETTINGS_MAX_FOCAL_LENGTH] as u16,
                                focal_units,
                            ),
                        );
                    }

                    // MinFocalLength (index 24) - Minimum focal length
                    if array.len() > CAMERA_SETTINGS_MIN_FOCAL_LENGTH {
                        tags.insert(
                            "Canon:MinFocalLength".to_string(),
                            format_focal_length(
                                array[CAMERA_SETTINGS_MIN_FOCAL_LENGTH] as u16,
                                focal_units,
                            ),
                        );
                    }

                    // MaxAperture (index 26) - Maximum aperture (APEX value)
                    if let Some(&raw_aperture) = array.get(CAMERA_SETTINGS_MAX_APERTURE)
                        && let Some(rendered) = canon_aperture(raw_aperture)
                    {
                        tags.insert("Canon:MaxAperture".to_string(), rendered);
                    }

                    // MinAperture (index 27) - Minimum aperture (APEX value)
                    if let Some(&raw_aperture) = array.get(CAMERA_SETTINGS_MIN_APERTURE)
                        && let Some(rendered) = canon_aperture(raw_aperture)
                    {
                        tags.insert("Canon:MinAperture".to_string(), rendered);
                    }

                    // FlashModel (index 28). ExifTool masks with 0x7f and discards the
                    // "no information" code 127; there is no FlashActivity key here.
                    if let Some(&raw_flash_model) = array.get(CAMERA_SETTINGS_FLASH_MODEL) {
                        let masked = raw_flash_model & 0x7f;
                        if masked != 127 {
                            tags.insert("Canon:FlashModel".to_string(), FLASH_MODEL.decode(masked));
                        }
                    }

                    // FlashBits (index 29) - Flash features bitfield
                    if array.len() > CAMERA_SETTINGS_FLASH_BITS {
                        let flash_bits = array[CAMERA_SETTINGS_FLASH_BITS] as u16;
                        tags.insert("Canon:FlashBits".to_string(), decode_flash_bits(flash_bits));
                    }

                    // FocusContinuous (index 32). `RawConv => '$val==-1 ? undef : $val'`
                    // (Canon.pm:2580), the same guard AESetting and SpotMeteringMode
                    // below already carry -- an absent setting must not surface as
                    // "Unknown (-1)". The EOS M50's CR3 stores -1 here and ExifTool
                    // reports no FocusContinuous at all for it.
                    if let Some(&focus_continuous) = array.get(CAMERA_SETTINGS_FOCUS_CONTINUOUS)
                        && focus_continuous != -1
                    {
                        tags.insert(
                            "Canon:FocusContinuous".to_string(),
                            FOCUS_CONTINUOUS.decode(focus_continuous),
                        );
                    }

                    // AESetting (index 33). `RawConv => '$val==-1 ? undef : $val'` — an
                    // absent setting must not surface as "Unknown (-1)".
                    if let Some(&ae_setting) = array.get(CAMERA_SETTINGS_AE_SETTING)
                        && ae_setting != -1
                    {
                        tags.insert("Canon:AESetting".to_string(), AE_SETTING.decode(ae_setting));
                    }

                    // DisplayAperture (index 35). ExifTool `%Canon::CameraSettings` key 35
                    // (Canon.pm:2645) is `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => '$val / 10'` and *no* PrintConv, so the value prints as a
                    // bare number ("3.9"), never with an "f/" prefix.
                    if let Some(&display_aperture) = array.get(CAMERA_SETTINGS_DISPLAY_APERTURE)
                        && display_aperture != 0
                    {
                        tags.insert(
                            "Canon:DisplayAperture".to_string(),
                            format_perl_number(display_aperture as f64 / 10.0),
                        );
                    }

                    // ZoomSourceWidth (index 36) / ZoomTargetWidth (index 37). ExifTool
                    // has no RawConv on these keys, so a zero width is still reported.
                    if let Some(&width) = array.get(CAMERA_SETTINGS_ZOOM_SOURCE_WIDTH) {
                        tags.insert("Canon:ZoomSourceWidth".to_string(), width.to_string());
                    }
                    if let Some(&width) = array.get(CAMERA_SETTINGS_ZOOM_TARGET_WIDTH) {
                        tags.insert("Canon:ZoomTargetWidth".to_string(), width.to_string());
                    }

                    // SpotMeteringMode (index 39). `RawConv => '$val==-1 ? undef : $val'`.
                    if let Some(&spot) = array.get(CAMERA_SETTINGS_SPOT_METERING_MODE)
                        && spot != -1
                    {
                        tags.insert(
                            "Canon:SpotMeteringMode".to_string(),
                            SPOT_METERING_MODE.decode(spot),
                        );
                    }

                    // PhotoEffect (index 40). `RawConv => '$val==-1 ? undef : $val'`.
                    if let Some(&photo_effect) = array.get(CAMERA_SETTINGS_PHOTO_EFFECT)
                        && photo_effect != -1
                    {
                        tags.insert(
                            "Canon:PhotoEffect".to_string(),
                            PHOTO_EFFECT.decode(photo_effect),
                        );
                    }

                    // ManualFlashOutput (index 41) - PrintHex lookup, 0x7fff means n/a
                    if let Some(&manual_flash) = array.get(CAMERA_SETTINGS_MANUAL_FLASH_OUTPUT) {
                        tags.insert(
                            "Canon:ManualFlashOutput".to_string(),
                            MANUAL_FLASH_OUTPUT.decode(manual_flash as u16 as i32),
                        );
                    }

                    // ColorTone (index 42). `RawConv => '$val == 0x7fff ? undef : $val'`,
                    // then `%Image::ExifTool::Exif::printParameter`.
                    if let Some(&color_tone) = array.get(CAMERA_SETTINGS_COLOR_TONE)
                        && color_tone as u16 != 0x7fff
                    {
                        tags.insert("Canon:ColorTone".to_string(), print_parameter(color_tone));
                    }
                }
            }

            // ShotInfo array (Phase 2) - Extended extraction
            // Extracts all available fields from the Canon ShotInfo array
            CANON_SHOT_INFO => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                {
                    // AutoISO (index 1). ExifTool `%Canon::ShotInfo` key 1 (Canon.pm:2778):
                    // `ValueConv => 'exp($val/32*log(2))*100'`, `PrintConv => '%.0f'`.
                    // The slot is a log-scale code, never a literal ISO speed.
                    if let Some(&auto_iso) = array.get(SHOT_INFO_AUTO_ISO) {
                        let value = (auto_iso as f64 / 32.0 * std::f64::consts::LN_2).exp() * 100.0;
                        tags.insert("Canon:AutoISO".to_string(), format!("{:.0}", value));
                    }

                    // BaseISO (index 2). `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => 'exp($val/32*log(2))*100/32'`, `PrintConv => '%.0f'`.
                    if let Some(&base_iso) = array.get(SHOT_INFO_BASE_ISO)
                        && base_iso != 0
                    {
                        let value =
                            (base_iso as f64 / 32.0 * std::f64::consts::LN_2).exp() * 100.0 / 32.0;
                        tags.insert("Canon:BaseISO".to_string(), format!("{:.0}", value));
                    }

                    // MeasuredEV (index 3). ExifTool `%Canon::ShotInfo` key 3
                    // (Canon.pm:2794): `ValueConv => '$val / 32 + 5'`, `PrintConv =>
                    // '%.2f'`. The +5 offset is not optional — without it every EOS body
                    // reports a light value 5 stops too dark.
                    if let Some(&measured_ev) = array.get(SHOT_INFO_MEASURED_EV) {
                        let ev = measured_ev as f64 / 32.0 + 5.0;
                        tags.insert("Canon:MeasuredEV".to_string(), format!("{:.2}", ev));
                    }

                    // TargetAperture (index 4) - convert APEX to f-number
                    if let Some(&raw_aperture) = array.get(SHOT_INFO_TARGET_APERTURE)
                        && let Some(rendered) = canon_aperture(raw_aperture)
                    {
                        tags.insert("Canon:TargetAperture".to_string(), rendered);
                    }

                    // TargetExposureTime (index 5) - convert APEX to fractional time
                    if array.len() > SHOT_INFO_TARGET_EXPOSURE_TIME {
                        tags.insert(
                            "Canon:TargetExposureTime".to_string(),
                            apex_to_exposure_time(array[SHOT_INFO_TARGET_EXPOSURE_TIME]),
                        );
                    }

                    // ExposureCompensation (index 6) - CanonEv + PrintFraction
                    if let Some(&comp) = array.get(SHOT_INFO_EXPOSURE_COMPENSATION) {
                        tags.insert(
                            "Canon:ExposureCompensation".to_string(),
                            print_fraction(canon_ev(comp as i32)),
                        );
                    }

                    // WhiteBalance (index 7) - use decoder
                    if array.len() > SHOT_INFO_WHITE_BALANCE {
                        tags.insert(
                            "Canon:WhiteBalance".to_string(),
                            WHITE_BALANCE.decode(array[SHOT_INFO_WHITE_BALANCE]),
                        );
                    }

                    // SlowShutter (index 8) - use decoder
                    if array.len() > SHOT_INFO_SLOW_SHUTTER {
                        tags.insert(
                            "Canon:SlowShutter".to_string(),
                            SLOW_SHUTTER.decode(array[SHOT_INFO_SLOW_SHUTTER]),
                        );
                    }

                    // SequenceNumber (index 9) - direct value
                    if array.len() > SHOT_INFO_SEQUENCE_NUMBER {
                        tags.insert(
                            "Canon:SequenceNumber".to_string(),
                            array[SHOT_INFO_SEQUENCE_NUMBER].to_string(),
                        );
                    }

                    // OpticalZoomCode (index 10). ExifTool `PrintConv => '$val == 8 ?
                    // "n/a" : $val'` — every EOS body writes 8 here, which is a sentinel
                    // rather than a zoom step.
                    if let Some(&zoom_code) = array.get(SHOT_INFO_OPTICAL_ZOOM_CODE) {
                        tags.insert(
                            "Canon:OpticalZoomCode".to_string(),
                            if zoom_code == 8 {
                                "n/a".to_string()
                            } else {
                                zoom_code.to_string()
                            },
                        );
                    }

                    // FlashGuideNumber (index 13). `RawConv => '$val==-1 ? undef : $val'`,
                    // `ValueConv => '$val / 32'`.
                    if let Some(&guide_number) = array.get(SHOT_INFO_FLASH_GUIDE_NUMBER)
                        && guide_number != -1
                    {
                        tags.insert(
                            "Canon:FlashGuideNumber".to_string(),
                            format_perl_number(guide_number as f64 / 32.0),
                        );
                    }

                    // AFPointsInFocus (index 14). `RawConv => '$val==0 ? undef : $val'`
                    // plus a PrintHex lookup — this slot is a code, not a bitmask, and is
                    // only meaningful on the D30/D60 and some PowerShot bodies.
                    if let Some(&af_points) = array.get(SHOT_INFO_AF_POINTS_IN_FOCUS)
                        && af_points != 0
                    {
                        tags.insert(
                            "Canon:AFPointsInFocus".to_string(),
                            SHOT_INFO_AF_POINTS_IN_FOCUS_CODES.decode(af_points as u16 as i32),
                        );
                    }

                    // FlashExposureComp (index 15) - CanonEv + PrintFraction
                    if let Some(&flash_comp) = array.get(SHOT_INFO_FLASH_EXPOSURE_COMP) {
                        tags.insert(
                            "Canon:FlashExposureComp".to_string(),
                            print_fraction(canon_ev(flash_comp as i32)),
                        );
                    }

                    // AutoExposureBracketing (index 16) - enumeration, not an EV offset
                    if let Some(&aeb) = array.get(SHOT_INFO_AUTO_EXPOSURE_BRACKETING) {
                        tags.insert(
                            "Canon:AutoExposureBracketing".to_string(),
                            AUTO_EXPOSURE_BRACKETING.decode(aeb),
                        );
                    }

                    // AEBBracketValue (index 17) - CanonEv + PrintFraction
                    if let Some(&aeb_value) = array.get(SHOT_INFO_AEB_BRACKET_VALUE) {
                        tags.insert(
                            "Canon:AEBBracketValue".to_string(),
                            print_fraction(canon_ev(aeb_value as i32)),
                        );
                    }

                    // ControlMode (index 18) - use decoder
                    if array.len() > SHOT_INFO_CONTROL_MODE {
                        tags.insert(
                            "Canon:ControlMode".to_string(),
                            CONTROL_MODE.decode(array[SHOT_INFO_CONTROL_MODE]),
                        );
                    }

                    // FocusDistanceUpper (index 19) / FocusDistanceLower (index 20).
                    // ExifTool: "FocusDistance tags are only extracted if
                    // FocusDistanceUpper is non-zero" — key 19's RawConv returns undef on
                    // zero and key 20 is conditional on it.
                    let focus_distance_upper = array
                        .get(SHOT_INFO_FOCUS_DISTANCE_UPPER)
                        .copied()
                        .unwrap_or(0);
                    if focus_distance_upper != 0 {
                        tags.insert(
                            "Canon:FocusDistanceUpper".to_string(),
                            format_focus_distance(focus_distance_upper),
                        );
                        if let Some(&lower) = array.get(SHOT_INFO_FOCUS_DISTANCE_LOWER) {
                            tags.insert(
                                "Canon:FocusDistanceLower".to_string(),
                                format_focus_distance(lower),
                            );
                        }
                    }

                    // FNumber (index 21). `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => 'exp(CanonEv($val)*log(2)/2)'`, `PrintConv => '%.2g'`.
                    if let Some(&f_number) = array.get(SHOT_INFO_FNUMBER)
                        && f_number != 0
                    {
                        let value =
                            (canon_ev(f_number as i32) * std::f64::consts::LN_2 / 2.0).exp();
                        tags.insert("Canon:FNumber".to_string(), format_g2(value));
                    }

                    // ExposureTime (index 22). ExifTool has two variants of this key: the
                    // 20D/350D encoding carries an extra *1000/32 factor (Canon.pm:2965).
                    if let Some(&exposure_time) = array.get(SHOT_INFO_EXPOSURE_TIME)
                        && exposure_time != 0
                    {
                        let base = (-canon_ev(exposure_time as i32) * std::f64::consts::LN_2).exp();
                        let seconds = if is_20d_or_350d(&model) {
                            base * 1000.0 / 32.0
                        } else {
                            base
                        };
                        tags.insert(
                            "Canon:ExposureTime".to_string(),
                            print_exposure_time(seconds),
                        );
                    }

                    // MeasuredEV2 (index 23). `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => '$val / 8 - 6'` (no PrintConv).
                    if let Some(&measured_ev2) = array.get(SHOT_INFO_MEASURED_EV2)
                        && measured_ev2 != 0
                    {
                        tags.insert(
                            "Canon:MeasuredEV2".to_string(),
                            format_perl_number(measured_ev2 as f64 / 8.0 - 6.0),
                        );
                    }

                    // BulbDuration (index 24). `ValueConv => '$val / 10'`.
                    if let Some(&duration) = array.get(SHOT_INFO_BULB_DURATION) {
                        tags.insert(
                            "Canon:BulbDuration".to_string(),
                            format_perl_number(duration as f64 / 10.0),
                        );
                    }

                    // CameraType (index 26) - ExifTool `%Canon::ShotInfo` key 26.
                    if let Some(&camera_type) = array.get(SHOT_INFO_CAMERA_TYPE) {
                        tags.insert(
                            "Canon:CameraType".to_string(),
                            decode_camera_type(camera_type),
                        );
                    }

                    // AutoRotate (index 27). ExifTool's RawConv drops negative values.
                    if let Some(&auto_rotate) = array.get(SHOT_INFO_AUTO_ROTATE)
                        && auto_rotate >= 0
                    {
                        tags.insert(
                            "Canon:AutoRotate".to_string(),
                            AUTO_ROTATE.decode(auto_rotate),
                        );
                    }

                    // NDFilter (index 28)
                    if let Some(&nd_filter) = array.get(SHOT_INFO_ND_FILTER) {
                        tags.insert("Canon:NDFilter".to_string(), ND_FILTER.decode(nd_filter));
                    }

                    // SelfTimer2 (index 29). `RawConv => '$val >= 0 ? $val : undef'`,
                    // `ValueConv => '$val / 10'`.
                    if let Some(&self_timer2) = array.get(SHOT_INFO_SELF_TIMER2)
                        && self_timer2 >= 0
                    {
                        tags.insert(
                            "Canon:SelfTimer2".to_string(),
                            format_perl_number(self_timer2 as f64 / 10.0),
                        );
                    }
                }
            }

            // FocalLength array (Phase 2)
            // Contains focal type, focal length and (on supported bodies) focal plane size
            CANON_FOCAL_LENGTH => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                {
                    // FocalType (key 0). `RawConv => '$val ? $val : undef'`.
                    if let Some(&focal_type) = array.first()
                        && focal_type != 0
                    {
                        tags.insert("Canon:FocalType".to_string(), FOCAL_TYPE.decode(focal_type));
                    }
                    // FocalLength (key 1). `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => '$val / $$self{FocalUnits}'`, `PrintConv => '"$val mm"'`.
                    if let Some(&focal_length) = array.get(1)
                        && focal_length != 0
                    {
                        tags.insert(
                            "Canon:FocalLength".to_string(),
                            format_focal_length(focal_length as u16, focal_units),
                        );
                    }
                    // FocalPlaneXSize / FocalPlaneYSize (keys 2 and 3), in 1/1000 inch.
                    // ExifTool only trusts these on the bodies listed in its Condition,
                    // and drops implausibly small values via `$val < 40 ? undef : $val`.
                    if focal_plane_size_supported(&model) {
                        for (index, name) in [
                            (2usize, "Canon:FocalPlaneXSize"),
                            (3usize, "Canon:FocalPlaneYSize"),
                        ] {
                            if let Some(&raw) = array.get(index) {
                                let thousandths = raw as u16 as f64;
                                if thousandths >= 40.0 {
                                    tags.insert(
                                        name.to_string(),
                                        format!("{:.2} mm", thousandths * 25.4 / 1000.0),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // LensModel tag (Phase 3) - ASCII string containing lens name
            //
            // This is the same TIFF-relative offset every other Canon string
            // entry uses, so it goes through `extract_canon_string_with_base`
            // like CanonImageType and OwnerName do. It used to index `data` with
            // the raw `value_offset`, which is only correct when the base
            // happens to be 0; on a CR3, whose MakerNote sits 8 bytes into the
            // CMT3 TIFF, that read the string 8 bytes late and ran `value_count`
            // bytes past its end -- "EF-M15-45mm f/3.5-6.3 IS STM" came out as
            // "5mm f/3.5-6.3 IS STM" plus 46 NULs and the first 8 characters of
            // the InternalSerialNumber that follows it.
            CANON_LENS_MODEL => {
                if let Some(lens_model) =
                    extract_canon_string_with_base(entry, ifd_data, byte_order, base)
                    && !lens_model.is_empty()
                {
                    tags.insert("Canon:LensModel".to_string(), lens_model);
                }
            }

            // FileInfo array (Phase 3)
            CANON_FILE_INFO => {
                // FileInfo is a SHORT array
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                {
                    // FileNumber (Perl key 1) is an int32u spanning int16 slots 1-2 on the
                    // 20D/350D family, with the bit layout documented at Canon.pm:6862:
                    //
                    // ```text
                    //   31....24 23....16 15.....8 7......0
                    //   00000000 ffffffff DDDDDDDD ddFFFFFF
                    // ```
                    let file_number_is_known = is_20d_or_350d(&model);
                    if file_number_is_known
                        && let (Some(&low), Some(&high)) = (array.get(1), array.get(2))
                    {
                        let raw = ((high as u16 as u32) << 16) | (low as u16 as u32);
                        let value = ((raw & 0xffc0) >> 6) * 10000
                            + ((raw >> 16) & 0xff)
                            + ((raw & 0x3f) << 8);
                        tags.insert(
                            "Canon:FileNumber".to_string(),
                            format_canon_file_number(value),
                        );
                    }

                    // BracketMode (Perl key 3)
                    if let Some(&bracket_mode) = array.get(FILE_INFO_BRACKET_MODE) {
                        tags.insert(
                            "Canon:BracketMode".to_string(),
                            BRACKET_MODE.decode(bracket_mode),
                        );
                    }

                    // BracketValue (Perl key 4)
                    if let Some(&bracket_value) = array.get(FILE_INFO_BRACKET_VALUE) {
                        tags.insert("Canon:BracketValue".to_string(), bracket_value.to_string());
                    }

                    // BracketShotNumber (Perl key 5)
                    if let Some(&bracket_shot) = array.get(FILE_INFO_BRACKET_SHOT_NUMBER) {
                        tags.insert(
                            "Canon:BracketShotNumber".to_string(),
                            bracket_shot.to_string(),
                        );
                    }

                    // LongExposureNoiseReduction2 (Perl key 8). `RawConv => '$val<0 ? undef'`.
                    if let Some(&long_exposure_nr) = array.get(FILE_INFO_LONG_EXPOSURE_NR2)
                        && long_exposure_nr >= 0
                    {
                        tags.insert(
                            "Canon:LongExposureNoiseReduction2".to_string(),
                            LONG_EXPOSURE_NOISE_REDUCTION2.decode(long_exposure_nr),
                        );
                    }

                    // WBBracketMode (Perl key 9)
                    if let Some(&wb_bracket_mode) = array.get(FILE_INFO_WB_BRACKET_MODE) {
                        tags.insert(
                            "Canon:WBBracketMode".to_string(),
                            WB_BRACKET_MODE.decode(wb_bracket_mode),
                        );
                    }

                    // WBBracketValueAB / WBBracketValueGM (Perl keys 12 and 13)
                    if let Some(&value_ab) = array.get(FILE_INFO_WB_BRACKET_VALUE_AB) {
                        tags.insert("Canon:WBBracketValueAB".to_string(), value_ab.to_string());
                    }
                    if let Some(&value_gm) = array.get(FILE_INFO_WB_BRACKET_VALUE_GM) {
                        tags.insert("Canon:WBBracketValueGM".to_string(), value_gm.to_string());
                    }

                    // FilterEffect / ToningEffect (Perl keys 14 and 15).
                    // Both have `RawConv => '$val==-1 ? undef : $val'`.
                    if let Some(&filter_effect) = array.get(FILE_INFO_FILTER_EFFECT)
                        && filter_effect != -1
                    {
                        tags.insert(
                            "Canon:FilterEffect".to_string(),
                            FILTER_EFFECT.decode(filter_effect),
                        );
                    }
                    if let Some(&toning_effect) = array.get(FILE_INFO_TONING_EFFECT)
                        && toning_effect != -1
                    {
                        tags.insert(
                            "Canon:ToningEffect".to_string(),
                            TONING_EFFECT.decode(toning_effect),
                        );
                    }

                    // Legacy ShutterCount heuristic: slots 2-3 have no counterpart in
                    // %Canon::FileInfo, so only keep it where key 1 is not a FileNumber.
                    if !file_number_is_known
                        && let (Some(&low), Some(&high)) = (
                            array.get(FILE_INFO_SHUTTER_COUNT_LOW),
                            array.get(FILE_INFO_SHUTTER_COUNT_HIGH),
                        )
                    {
                        let shutter_count = ((high as u32) << 16) | (low as u32 & 0xFFFF);
                        if shutter_count > 0 {
                            tags.insert(
                                "Canon:ShutterCount".to_string(),
                                shutter_count.to_string(),
                            );
                        }
                    }
                }
            }

            // ProcessingInfo (tag 0x00A0) - ExifTool `%Image::ExifTool::Canon::Processing`
            // (Canon.pm:7201), `FORMAT => 'int16s'`, `FIRST_ENTRY => 1`.
            CANON_PROCESSING_INFO => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                {
                    if let Some(&tone_curve) = array.get(PROCESSING_INFO_TONE_CURVE) {
                        tags.insert(
                            "Canon:ToneCurve".to_string(),
                            TONE_CURVE.decode(tone_curve as i32),
                        );
                    }

                    // Key 2 (Sharpness) is deliberately left alone: it is excluded on the
                    // 20D/350D by ExifTool's Condition and carries `Priority => 0`
                    // elsewhere, so CameraSettings key 15 stays authoritative.

                    if let Some(&frequency) = array.get(PROCESSING_INFO_SHARPNESS_FREQ) {
                        tags.insert(
                            "Canon:SharpnessFrequency".to_string(),
                            SHARPNESS_FREQUENCY.decode(frequency),
                        );
                    }

                    for (index, name) in [
                        (PROCESSING_INFO_SENSOR_RED_LEVEL, "Canon:SensorRedLevel"),
                        (PROCESSING_INFO_SENSOR_BLUE_LEVEL, "Canon:SensorBlueLevel"),
                        (PROCESSING_INFO_WHITE_BALANCE_RED, "Canon:WhiteBalanceRed"),
                        (PROCESSING_INFO_WHITE_BALANCE_BLUE, "Canon:WhiteBalanceBlue"),
                        (PROCESSING_INFO_COLOR_TEMPERATURE, "Canon:ColorTemperature"),
                        (PROCESSING_INFO_WB_SHIFT_AB, "Canon:WBShiftAB"),
                        (PROCESSING_INFO_WB_SHIFT_GM, "Canon:WBShiftGM"),
                    ] {
                        if let Some(&value) = array.get(index) {
                            tags.insert(name.to_string(), value.to_string());
                        }
                    }

                    // WhiteBalance (key 8). `RawConv => '$val < 0 ? undef : $val'` — the
                    // -32768 sentinel means "not recorded here".
                    if let Some(&white_balance) = array.get(PROCESSING_INFO_WHITE_BALANCE)
                        && white_balance >= 0
                    {
                        tags.insert(
                            "Canon:WhiteBalance".to_string(),
                            WHITE_BALANCE.decode(white_balance),
                        );
                    }

                    if let Some(&picture_style) = array.get(PROCESSING_INFO_PICTURE_STYLE) {
                        tags.insert(
                            "Canon:PictureStyle".to_string(),
                            PICTURE_STYLE.decode(picture_style as u16 as i32),
                        );
                    }

                    // DigitalGain (key 11). `ValueConv => '$val / 10'`.
                    if let Some(&digital_gain) = array.get(PROCESSING_INFO_DIGITAL_GAIN) {
                        tags.insert(
                            "Canon:DigitalGain".to_string(),
                            format_perl_number(digital_gain as f64 / 10.0),
                        );
                    }
                }
            }

            // MeasuredColor (tag 0x00AA) - ExifTool `%Canon::MeasuredColor` key 1 is a
            // single `int16u[4]` value, not four scalars.
            CANON_MEASURED_COLOR => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                    && array.len() >= MEASURED_COLOR_RGGB + 4
                {
                    tags.insert(
                        "Canon:MeasuredRGGB".to_string(),
                        array[MEASURED_COLOR_RGGB..MEASURED_COLOR_RGGB + 4]
                            .iter()
                            .map(|v| (*v as u16).to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }
            }

            // The plain %Canon binary sub-tables -- CropInfo, AspectInfo, ModifiedInfo,
            // AFConfig, VignettingCorr, ContrastInfo, FaceDetect1/3, Ambience and the
            // rest. See `binary_tables` for the transcription, the ExifTool `Condition`
            // each tag carries, and what the transcription deliberately omits.
            tag if binary_tables::handles_tag(tag) => {
                if let Some(raw_record) =
                    extract_canon_binary_words_with_base(entry, ifd_data, byte_order, base)
                {
                    // Tables that open with their own byte count go through the
                    // realignment guard; the others are indexed from 0 as stored.
                    let record = if binary_tables::table_is_length_prefixed(tag) {
                        realign_length_prefixed_record(raw_record)
                    } else {
                        raw_record
                    };
                    // ExifTool's Condition reads `$$valPt`, the untyped bytes, so the
                    // predicate has to see them rather than the byte-order-decoded words.
                    let raw_bytes =
                        extract_canon_bytes_with_base(entry, ifd_data, base).unwrap_or_default();
                    binary_tables::parse_binary_table(
                        tag, raw_bytes, &record, byte_order, &mut tags,
                    );
                }
            }

            // CameraInfo (tag 0x000D). ExifTool models this tag as a list of 33
            // alternatives (Canon.pm:1307), each naming a different
            // `%Canon::CameraInfo*` table: the first 31 are selected by the camera
            // model, the last four by the record's declared format and element count.
            // The record carries no version word and no length prefix, so the body is
            // the only thing that says what the bytes mean.
            CANON_CAMERA_INFO => {
                if let Some(record) = extract_canon_bytes_with_base(entry, ifd_data, base) {
                    camera_info_tags = camera_info::parse_camera_info(
                        record,
                        entry.field_type,
                        entry.value_count,
                        byte_order,
                        camera_info_model,
                        camera_settings_lens_type,
                    );
                }
            }

            // FilterInfo (tag 0x4024). `%Canon::FilterInfo` declares
            // `PROCESS_PROC => \&ProcessFilters` and its keys are parameter ids inside
            // a self-describing record, so it is walked rather than indexed.
            CANON_FILTER_INFO => {
                if let Some(record) = extract_canon_bytes_with_base(entry, ifd_data, base) {
                    filter_info::parse_filter_info(record, byte_order, &mut tags);
                }
            }

            // LensInfo (tag 0x4019). `%Canon::LensInfo` key 0 `LensSerialNumber`:
            // `RawConv => '$val=~/^\0\0\0\0/ ? undef : $val'` drops it when the first
            // four of its five raw bytes are all zero, `ValueConv =>
            // 'unpack("H*", $val)'` hexes what remains. Verified against
            // CanonEOS-1D_MarkIV.jpg: raw bytes `00 00 40 0e b1` -> "0000400eb1",
            // matching ExifTool exactly (first four bytes aren't all zero, so kept).
            CANON_LENS_INFO => {
                if let Some(record) = extract_canon_bytes_with_base(entry, ifd_data, base)
                    && record.len() >= 5
                    && record[..4] != [0, 0, 0, 0]
                {
                    lens_info_serial =
                        Some(record[..5].iter().map(|b| format!("{:02x}", b)).collect());
                }
            }

            // LevelInfo (tag 0x4059). `%Canon::LevelInfo`: `FORMAT => 'int32s'`,
            // `FIRST_ENTRY => 1`, so ExifTool's ProcessBinaryData puts key K at byte
            // offset K*4 (`$entry = int($index) * $increment`, increment ==
            // formatSize('int32s') == 4). Confirmed against CanonEOS_R5m2.jpg: key 4
            // (RollAngle) sits at byte 16, key 7 (FocalLength) at byte 28. Ordinary
            // (default) priority, and 0x4059 is read after CameraSettings' own
            // FocalLength (tag 0x0001), so this unconditionally overwrites it --
            // matching ExifTool's same-priority tie, where the later tag wins.
            CANON_LEVEL_INFO => {
                if let Some(record) = extract_canon_bytes_with_base(entry, ifd_data, base) {
                    let read_i32 = |key: usize| -> Option<i32> {
                        let at = key * 4;
                        let bytes = record.get(at..at + 4)?;
                        Some(match byte_order {
                            ByteOrder::LittleEndian => {
                                i32::from_le_bytes(bytes.try_into().unwrap())
                            }
                            ByteOrder::BigEndian => i32::from_be_bytes(bytes.try_into().unwrap()),
                        })
                    };
                    // `$val > 1800 and $val -= 3600; -$val / 10` / `... ; $val / 10`.
                    if let Some(mut roll) = read_i32(4) {
                        if roll > 1800 {
                            roll -= 3600;
                        }
                        tags.insert(
                            "Canon:RollAngle".to_string(),
                            format_perl_number(-(roll as f64) / 10.0),
                        );
                    }
                    if let Some(mut pitch) = read_i32(5) {
                        if pitch > 1800 {
                            pitch -= 3600;
                        }
                        tags.insert(
                            "Canon:PitchAngle".to_string(),
                            format_perl_number(pitch as f64 / 10.0),
                        );
                    }
                    for (key, name) in [
                        (7, "Canon:FocalLength"),
                        (8, "Canon:MinFocalLength2"),
                        (9, "Canon:MaxFocalLength2"),
                    ] {
                        if let Some(val) = read_i32(key) {
                            tags.insert(
                                name.to_string(),
                                format!("{} mm", format_perl_number(val as f64 / 10.0)),
                            );
                        }
                    }
                }
            }

            // ColorData (tag 0x4001). ExifTool picks one of twelve %Canon::ColorData*
            // tables by the record's element count (Canon.pm:1972), then gates individual
            // fields on the ColorDataVersion at index 0. All twelve are in `color_data`.
            //
            // The record is NOT length-prefixed -- index 0 is the version, not a size --
            // so it is passed on raw rather than through
            // `realign_length_prefixed_record`.
            CANON_COLOR_DATA => {
                if let Some(record) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                {
                    color_data::parse_color_data(&record, byte_order, &mut tags);
                }
            }

            // AFInfo (tag 0x0012) - autofocus information used by older Canon models
            CANON_AF_INFO => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                {
                    // `ProcessSerialData` (Canon.pm:10518) walks the record slot by slot
                    // and calls `FoundTag` for every slot that is present, gated only on
                    // `last if $pos + $len > $size` -- it never inspects the value. A slot
                    // that exists is reported whatever it holds, zero included, so the
                    // only condition here is "is the slot inside the record".
                    let num_points = array.get(AF_INFO_NUM_AF_POINTS).copied().unwrap_or(0);
                    if let Some(&points) = array.get(AF_INFO_NUM_AF_POINTS) {
                        tags.insert("Canon:NumAFPoints".to_string(), points.to_string());
                    }

                    // ValidAFPoints (key 1), CanonImageWidth (key 2), CanonImageHeight
                    // (key 3) - scalars ExifTool reports alongside the AF geometry.
                    if let Some(&valid_points) = array.get(AF_INFO_VALID_AF_POINTS) {
                        tags.insert("Canon:ValidAFPoints".to_string(), valid_points.to_string());
                    }
                    if let Some(&width) = array.get(AF_INFO_CANON_IMAGE_WIDTH) {
                        tags.insert("Canon:CanonImageWidth".to_string(), width.to_string());
                    }
                    if let Some(&height) = array.get(AF_INFO_CANON_IMAGE_HEIGHT) {
                        tags.insert("Canon:CanonImageHeight".to_string(), height.to_string());
                    }

                    if let Some(&width) = array.get(AF_INFO_AF_IMAGE_WIDTH) {
                        tags.insert("Canon:AFImageWidth".to_string(), width.to_string());
                    }
                    if let Some(&height) = array.get(AF_INFO_AF_IMAGE_HEIGHT) {
                        tags.insert("Canon:AFImageHeight".to_string(), height.to_string());
                    }
                    if let Some(&area_width) = array.get(AF_INFO_AF_AREA_WIDTH) {
                        tags.insert("Canon:AFAreaWidth".to_string(), area_width.to_string());
                    }
                    if let Some(&area_height) = array.get(AF_INFO_AF_AREA_HEIGHT) {
                        tags.insert("Canon:AFAreaHeight".to_string(), area_height.to_string());
                    }

                    // Keys 8+ are variable-length: AFAreaXPositions[n], AFAreaYPositions[n]
                    // then AFPointsInFocus as ceil(n/16) 16-bit words.
                    if num_points > 0 {
                        let n = num_points as usize;
                        let x_start = AF_INFO_VARIABLE_START;
                        let y_start = x_start + n;
                        let focus_start = y_start + n;
                        let focus_words = n.div_ceil(16);

                        if array.len() >= y_start {
                            tags.insert(
                                "Canon:AFAreaXPositions".to_string(),
                                join_i16_slice(&array[x_start..y_start]),
                            );
                        }
                        if array.len() >= focus_start {
                            tags.insert(
                                "Canon:AFAreaYPositions".to_string(),
                                join_i16_slice(&array[y_start..focus_start]),
                            );
                        }
                        if array.len() >= focus_start + focus_words {
                            tags.insert(
                                "Canon:AFPointsInFocus".to_string(),
                                decode_bits_16(&array[focus_start..focus_start + focus_words]),
                            );
                        }
                    }
                }
            }

            // AFInfo2 (tag 0x0026) - autofocus information used by newer Canon models.
            // AFInfo3 (tag 0x003c) is the same `%Canon::AFInfo2` record under a second
            // tag id (Canon.pm:1764). The two are alternatives, not siblings: all 42
            // sample-corpus files that carry 0x003c carry no 0x0026 at all, so reading
            // only 0x0026 left them with no AF tags whatsoever.
            CANON_AF_INFO2 | CANON_AF_INFO3 => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                {
                    if let Some(&mode) = array.get(AF_INFO2_AF_AREA_MODE) {
                        tags.insert("Canon:AFAreaMode".to_string(), AF_AREA_MODE.decode(mode));
                    }

                    // Same `ProcessSerialData` rule as AFInfo above: present slots are
                    // reported unconditionally. Keys 3/4/5 were transcribed into the
                    // comment on the index constants but never read, so ValidAFPoints,
                    // CanonImageWidth and CanonImageHeight went missing on every body
                    // that writes AFInfo2 (e.g. the 1D Mk III, where ExifTool prints
                    // `ValidAFPoints: 45`, `CanonImageWidth: 3888`,
                    // `CanonImageHeight: 2592`).
                    let num_points = array.get(AF_INFO2_NUM_AF_POINTS).copied().unwrap_or(0);
                    if let Some(&points) = array.get(AF_INFO2_NUM_AF_POINTS) {
                        tags.insert("Canon:NumAFPoints".to_string(), points.to_string());
                    }
                    if let Some(&valid_points) = array.get(AF_INFO2_VALID_AF_POINTS) {
                        tags.insert("Canon:ValidAFPoints".to_string(), valid_points.to_string());
                    }
                    if let Some(&width) = array.get(AF_INFO2_CANON_IMAGE_WIDTH) {
                        tags.insert("Canon:CanonImageWidth".to_string(), width.to_string());
                    }
                    if let Some(&height) = array.get(AF_INFO2_CANON_IMAGE_HEIGHT) {
                        tags.insert("Canon:CanonImageHeight".to_string(), height.to_string());
                    }

                    if let Some(&width) = array.get(AF_INFO2_AF_IMAGE_WIDTH) {
                        tags.insert("Canon:AFImageWidth".to_string(), width.to_string());
                    }
                    if let Some(&height) = array.get(AF_INFO2_AF_IMAGE_HEIGHT) {
                        tags.insert("Canon:AFImageHeight".to_string(), height.to_string());
                    }

                    // Keys 8+ are variable-length: AFAreaWidths[n], AFAreaHeights[n],
                    // AFAreaXPositions[n], AFAreaYPositions[n], then AFPointsInFocus as
                    // ceil(n/16) 16-bit words.
                    if num_points > 0 {
                        let n = num_points as usize;
                        let widths_start = AF_INFO2_VARIABLE_START;
                        let heights_start = widths_start + n;
                        let x_start = heights_start + n;
                        let y_start = x_start + n;
                        let focus_start = y_start + n;
                        let focus_words = n.div_ceil(16);

                        if array.len() >= heights_start {
                            tags.insert(
                                "Canon:AFAreaWidths".to_string(),
                                join_i16_slice(&array[widths_start..heights_start]),
                            );
                        }
                        if array.len() >= x_start {
                            tags.insert(
                                "Canon:AFAreaHeights".to_string(),
                                join_i16_slice(&array[heights_start..x_start]),
                            );
                        }
                        if array.len() >= y_start {
                            tags.insert(
                                "Canon:AFAreaXPositions".to_string(),
                                join_i16_slice(&array[x_start..y_start]),
                            );
                        }
                        if array.len() >= focus_start {
                            tags.insert(
                                "Canon:AFAreaYPositions".to_string(),
                                join_i16_slice(&array[y_start..focus_start]),
                            );
                        }
                        if array.len() >= focus_start + focus_words {
                            tags.insert(
                                "Canon:AFPointsInFocus".to_string(),
                                decode_bits_16(&array[focus_start..focus_start + focus_words]),
                            );
                        }
                    }
                }
            }

            // CustomFunctions (tag 0x000F).
            //
            // ExifTool Canon.pm:1500 picks a per-body table; only
            // `%CanonCustom::Functions350D` (CanonCustom.pm:809) is implemented here, so
            // the record is skipped on every other body rather than decoded with the
            // wrong labels. `ProcessCanonCustom` (CanonCustom.pm:2772) reads one int16
            // per entry after a leading byte-length word and splits it into
            // `tag = $val >> 8` / `value = $val & 0xff`.
            CANON_CUSTOM_FUNCTIONS => {
                if is_350d_custom_functions(camera_info_model)
                    && let Some(array) =
                        extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                    && array.first().map(|&len| len as u16 as usize) == Some(array.len() * 2)
                {
                    for &word in &array[1..] {
                        let raw = word as u16;
                        let function = raw >> 8;
                        let value = (raw & 0xff) as i16;
                        // `%CanonCustom::Functions350D` (CanonCustom.pm:809) has no
                        // group-1 override, so its default family-1 group is the module
                        // name, "CanonCustom" -- not "Canon".
                        let (name, rendered) = match function {
                            0 => (
                                "CanonCustom:SetButtonCrossKeysFunc",
                                CC350D_SET_BUTTON_CROSS_KEYS_FUNC.decode(value),
                            ),
                            1 => (
                                "CanonCustom:LongExposureNoiseReduction",
                                CC350D_LONG_EXPOSURE_NOISE_REDUCTION.decode(value),
                            ),
                            2 => (
                                "CanonCustom:FlashSyncSpeedAv",
                                CC350D_FLASH_SYNC_SPEED_AV.decode(value),
                            ),
                            3 => (
                                "CanonCustom:Shutter-AELock",
                                CC350D_SHUTTER_AE_LOCK.decode(value),
                            ),
                            4 => (
                                "CanonCustom:AFAssistBeam",
                                CC350D_AF_ASSIST_BEAM.decode(value),
                            ),
                            5 => (
                                "CanonCustom:ExposureLevelIncrements",
                                CC350D_EXPOSURE_LEVEL_INCREMENTS.decode(value),
                            ),
                            6 => (
                                "CanonCustom:MirrorLockup",
                                CC350D_MIRROR_LOCKUP.decode(value),
                            ),
                            7 => ("CanonCustom:ETTLII", CC350D_ETTL_II.decode(value)),
                            8 => (
                                "CanonCustom:ShutterCurtainSync",
                                CC350D_SHUTTER_CURTAIN_SYNC.decode(value),
                            ),
                            _ => continue,
                        };
                        tags.insert(name.to_string(), rendered);
                    }
                }
            }

            // CustomFunctions2 (tag 0x0099) - the custom-function block written by the
            // EOS-1D Mark III and every later body (ExifTool Canon.pm:1883).
            CANON_CUSTOM_FUNCTIONS2 => {
                if let Some(bytes) = extract_canon_bytes_with_base(entry, ifd_data, base) {
                    custom_functions2::parse_custom_functions2(
                        bytes,
                        byte_order,
                        camera_info_model,
                        &mut tags,
                    );
                }
            }

            // PersonalFunctions (tag 0x0091) - EOS-1D personal function switches.
            //
            // `%CanonCustom::PersonalFuncs` (CanonCustom.pm:1091) is an `int16u`
            // BinaryData table with `FIRST_ENTRY => 1`, so index 0 is the record's own
            // byte count and every switch runs through `ConvertPfn`. The table has no
            // group-1 override, so its default family-1 group is the module name,
            // "CanonCustom" -- not "Canon".
            CANON_PERSONAL_FUNCTIONS => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                {
                    for &(index, name) in PERSONAL_FUNCS {
                        if let Some(&raw) = array.get(index) {
                            tags.insert(
                                format!("CanonCustom:{}", name),
                                convert_personal_function(raw as u16),
                            );
                        }
                    }
                }
            }

            // PersonalFunctionValues (tag 0x0092) - the values those switches carry.
            //
            // `%CanonCustom::PersonalFuncValues` (CanonCustom.pm:1135), also `int16u`
            // with `FIRST_ENTRY => 1`. Most keys are reported verbatim; keys 4-7 carry
            // Canon's EV encoding. Same as above: default family-1 group is
            // "CanonCustom".
            CANON_PERSONAL_FUNCTION_VALUES => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                {
                    for &(index, name) in PERSONAL_FUNC_VALUES {
                        if let Some(&raw) = array.get(index) {
                            tags.insert(format!("CanonCustom:{}", name), (raw as u16).to_string());
                        }
                    }

                    // Keys 4/5 - PF4ExposureTimeMin/Max. CanonCustom.pm:1139:
                    //     ValueConv => 'exp(-CanonEv($val*4)*log(2))*1000/8'
                    //     PrintConv => 'Image::ExifTool::Exif::PrintExposureTime($val)'
                    for &(index, name) in &[(4, "PF4ExposureTimeMin"), (5, "PF4ExposureTimeMax")] {
                        if let Some(&raw) = array.get(index) {
                            let seconds =
                                2.0_f64.powf(-canon_ev(raw as u16 as i32 * 4)) * 1000.0 / 8.0;
                            tags.insert(
                                format!("CanonCustom:{}", name),
                                print_exposure_time(seconds),
                            );
                        }
                    }

                    // Keys 6/7 - PF5ApertureMin/Max. CanonCustom.pm:1163:
                    //     ValueConv => 'exp(CanonEv($val*4-32)*log(2)/2)'
                    //     PrintConv => 'sprintf("%.2g",$val)'
                    for &(index, name) in &[(6, "PF5ApertureMin"), (7, "PF5ApertureMax")] {
                        if let Some(&raw) = array.get(index) {
                            let f_number = 2.0_f64.powf(canon_ev(raw as u16 as i32 * 4 - 32) / 2.0);
                            tags.insert(format!("CanonCustom:{}", name), format_g2(f_number));
                        }
                    }
                }
            }

            // SensorInfo (tag 0x00E0) - sensor dimensions, image borders, black mask
            CANON_SENSOR_INFO => {
                if let Some(array) =
                    extract_canon_i16_array_with_base(entry, ifd_data, byte_order, base)
                        .map(realign_length_prefixed_record)
                {
                    for (index, name) in [
                        (SENSOR_INFO_SENSOR_WIDTH, "Canon:SensorWidth"),
                        (SENSOR_INFO_SENSOR_HEIGHT, "Canon:SensorHeight"),
                        (SENSOR_INFO_SENSOR_LEFT_BORDER, "Canon:SensorLeftBorder"),
                        (SENSOR_INFO_SENSOR_TOP_BORDER, "Canon:SensorTopBorder"),
                        (SENSOR_INFO_SENSOR_RIGHT_BORDER, "Canon:SensorRightBorder"),
                        (SENSOR_INFO_SENSOR_BOTTOM_BORDER, "Canon:SensorBottomBorder"),
                        (
                            SENSOR_INFO_BLACK_MASK_LEFT_BORDER,
                            "Canon:BlackMaskLeftBorder",
                        ),
                        (
                            SENSOR_INFO_BLACK_MASK_TOP_BORDER,
                            "Canon:BlackMaskTopBorder",
                        ),
                        (
                            SENSOR_INFO_BLACK_MASK_RIGHT_BORDER,
                            "Canon:BlackMaskRightBorder",
                        ),
                        (
                            SENSOR_INFO_BLACK_MASK_BOTTOM_BORDER,
                            "Canon:BlackMaskBottomBorder",
                        ),
                    ] {
                        if let Some(&value) = array.get(index) {
                            tags.insert(name.to_string(), value.to_string());
                        }
                    }
                }
            }

            // Other array tags - skip for now (will add in future phases)
            _ => {}
        }
    });

    camera_info::merge_priority0(&mut tags, camera_info_tags);

    // See the comment on `lens_info_serial`'s declaration: this only fills a gap
    // left by `camera_info_tags`, which is why it's merged after that call.
    if let Some(serial) = lens_info_serial {
        tags.entry("Canon:LensSerialNumber".to_string())
            .or_insert(serial);
    }

    Ok(tags)
}

/// Parses Canon MakerNote data into a map of tag names to values.
///
/// This is the public API that delegates to the CanonParser trait implementation.
///
/// # Parameters
/// - `data`: Raw MakerNote data (may include Canon signature)
/// - `byte_order`: Byte order for parsing (usually matches TIFF header)
/// - `tags`: Mutable reference to HashMap to populate with extracted tags
///
/// # Example
/// ```ignore
/// use std::collections::HashMap;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
///
/// let mut tags = HashMap::new();
/// parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
/// ```
pub fn parse_canon_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = CanonParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("Canon MakerNotes parse error: {}", e);
    }
}

/// Extracts inline value bytes from the value_offset field.
///
/// For values that fit in 4 bytes or less, they are stored directly
/// in the value_offset field rather than at an external offset.
#[cfg(test)]
mod tests {
    /// `format_focal_length` divides by `FocalUnits`, and ExifTool lets the
    /// quotient reach the output as a bare Perl scalar -- so it is printed
    /// with Perl's own 15-significant-digit stringification.
    ///
    /// Every expected string here is what the installed Perl 5.42 prints for
    /// the same division (`perl -e 'print 100/3'` => `33.3333333333333`).
    /// The local `format!("{:.6}", ..)` copy this file used to carry stops at
    /// six decimals and answers `33.333333`, so this test fails against it.
    #[test]
    fn test_focal_length_quotient_keeps_perl_significant_digits() {
        use super::format_focal_length;

        assert_eq!(format_focal_length(100, 3), "33.3333333333333 mm");
        assert_eq!(format_focal_length(5, 3), "1.66666666666667 mm");
        assert_eq!(format_focal_length(2, 3), "0.666666666666667 mm");
        // Exact quotients still lose the decimal point entirely.
        assert_eq!(format_focal_length(50, 1), "50 mm");
        assert_eq!(format_focal_length(259, 16), "16.1875 mm");
    }

    use super::*;

    #[test]
    fn test_canon_tag_ids() {
        assert_eq!(CANON_CAMERA_SETTINGS, 0x0001);
        assert_eq!(CANON_FOCAL_LENGTH, 0x0002);
        assert_eq!(CANON_SHOT_INFO, 0x0004);
        assert_eq!(CANON_MODEL_ID, 0x0010);
    }

    #[test]
    fn test_canon_signature() {
        assert_eq!(CANON_SIGNATURE, b"Canon");
    }

    #[test]
    fn test_canon_tag_to_name() {
        assert_eq!(canon_tag_to_name(0x0001), "Canon:CameraSettings");
        assert_eq!(canon_tag_to_name(0x0002), "Canon:FocalLength");
        assert_eq!(canon_tag_to_name(0x0004), "Canon:ShotInfo");
        assert_eq!(canon_tag_to_name(0x0006), "Canon:ImageType");
        assert_eq!(canon_tag_to_name(0x0007), "Canon:FirmwareVersion");
        assert_eq!(canon_tag_to_name(0x0010), "Canon:CanonModelID");

        // Unknown tag
        assert_eq!(canon_tag_to_name(0xFFFF), "Canon:Unknown-0xFFFF");
    }

    #[test]
    fn test_is_canon_makernote() {
        // With Canon signature
        let data_with_sig = b"Canon\x00\x01\x00\x02\x00";
        assert!(is_canon_makernote(data_with_sig));

        // Without signature (starts with IFD)
        let data_without_sig = b"\x00\x01\x00\x02\x00";
        assert!(is_canon_makernote(data_without_sig));

        // Invalid data
        let invalid_data = b"Nikon";
        assert!(!is_canon_makernote(invalid_data));
    }

    #[test]
    fn test_parse_canon_makernote_basic() {
        // Create minimal Canon MakerNote with signature
        let mut data = Vec::new();

        // Canon signature (optional)
        data.extend_from_slice(b"Canon");

        // Simple IFD with one entry (little-endian format)
        data.extend_from_slice(&[
            0x01, 0x00, // Number of entries: 1 (little-endian)
            // Entry 1: ImageType (0x0006)
            0x06, 0x00, // Tag ID: 0x0006 (little-endian)
            0x02, 0x00, // Type: 2 = ASCII string (little-endian)
            0x0B, 0x00, 0x00, 0x00, // Count: 11 bytes (little-endian)
            0x12, 0x00, 0x00, 0x00, // Offset to data: 0x12 (18 bytes from IFD start)
            // Next IFD offset
            0x00, 0x00, 0x00, 0x00,
            // String data at offset 0x12 from IFD start (= byte 23 from data start)
            b'I', b'M', b'G', b':', b'E', b'O', b'S', b' ', b'R', b'5', 0x00,
        ]);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian);
        assert!(result.is_ok());

        let tags = result.unwrap();
        assert!(!tags.is_empty());
        // ExifTool's name for MakerNote tag 0x0006 is `CanonImageType` (Canon.pm:1252);
        // the bare `ImageType` alias this used to also emit is not an ExifTool tag.
        assert_eq!(
            tags.get("Canon:CanonImageType"),
            Some(&"IMG:EOS R5".to_string())
        );
        assert_eq!(tags.get("Canon:ImageType"), None);
    }

    #[test]
    fn test_extract_i16_array_inline() {
        // Test inline array (count * 2 <= 4 bytes)
        let entry = IfdEntry {
            tag_id: CANON_FOCAL_LENGTH,
            field_type: 3, // SHORT
            value_count: 2,
            value_offset: 0x0064_0032, // Two shorts: 50, 100 (little-endian)
        };

        let result = extract_i16_array(&entry, &[], ByteOrder::LittleEndian);
        assert_eq!(result, Some(vec![50, 100]));
    }

    #[test]
    fn test_extract_i16_array_offset() {
        // Test offset-based array (count * 2 > 4 bytes)
        let entry = IfdEntry {
            tag_id: CANON_CAMERA_SETTINGS,
            field_type: 3, // SHORT
            value_count: 4,
            value_offset: 0, // Offset to data
        };

        // Data at offset 0: [1, 2, 3, 4] as little-endian shorts
        let data = vec![
            0x01, 0x00, // 1
            0x02, 0x00, // 2
            0x03, 0x00, // 3
            0x04, 0x00, // 4
        ];

        let result = extract_i16_array(&entry, &data, ByteOrder::LittleEndian);
        assert_eq!(result, Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn test_extract_i16_array_big_endian() {
        let entry = IfdEntry {
            tag_id: CANON_CAMERA_SETTINGS,
            field_type: 3,
            value_count: 3, // Use 3 values to force offset-based reading (>4 bytes)
            value_offset: 0,
        };

        // Big-endian data: [256, 512, 768]
        let data = vec![
            0x01, 0x00, // 256 (big-endian)
            0x02, 0x00, // 512 (big-endian)
            0x03, 0x00, // 768 (big-endian)
        ];

        let result = extract_i16_array(&entry, &data, ByteOrder::BigEndian);
        assert_eq!(result, Some(vec![256, 512, 768]));
    }

    #[test]
    fn test_camera_settings_indices() {
        // Verify key CameraSettings array indices are defined correctly
        assert_eq!(CAMERA_SETTINGS_MACRO_MODE, 1);
        assert_eq!(CAMERA_SETTINGS_SELF_TIMER, 2);
        assert_eq!(CAMERA_SETTINGS_QUALITY, 3);
        assert_eq!(CAMERA_SETTINGS_FLASH_MODE, 4);
        assert_eq!(CAMERA_SETTINGS_DRIVE_MODE, 5);
        assert_eq!(CAMERA_SETTINGS_FOCUS_MODE, 7);
        assert_eq!(CAMERA_SETTINGS_IMAGE_SIZE, 10);
        assert_eq!(CAMERA_SETTINGS_EASY_MODE, 11);
        assert_eq!(CAMERA_SETTINGS_CONTRAST, 13);
        assert_eq!(CAMERA_SETTINGS_SATURATION, 14);
        assert_eq!(CAMERA_SETTINGS_SHARPNESS, 15);
        assert_eq!(CAMERA_SETTINGS_ISO, 16);
        assert_eq!(CAMERA_SETTINGS_METERING_MODE, 17);
        assert_eq!(CAMERA_SETTINGS_FOCUS_TYPE, 18);
        assert_eq!(CAMERA_SETTINGS_AF_POINT, 19);
        assert_eq!(CAMERA_SETTINGS_EXPOSURE_MODE, 20);
        assert_eq!(CAMERA_SETTINGS_FLASH_MODEL, 28);
        assert_eq!(CAMERA_SETTINGS_FOCUS_CONTINUOUS, 32);
    }

    #[test]
    fn test_decode_macro_mode() {
        assert_eq!(MACRO_MODE.decode(1), "Macro");
        assert_eq!(MACRO_MODE.decode(2), "Normal");
        assert_eq!(MACRO_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(QUALITY.decode(2), "Normal");
        assert_eq!(QUALITY.decode(3), "Fine");
        assert_eq!(QUALITY.decode(5), "Superfine");
        assert_eq!(QUALITY.decode(130), "Normal Movie");
        assert_eq!(QUALITY.decode(131), "Movie (2)");
        assert_eq!(QUALITY.decode(99), "Unknown (99)");
    }

    /// Labels are ExifTool's verbatim (`%Canon::CameraSettings` key 4, Canon.pm:2243) --
    /// lower-case "reduction", "Slow-sync" rather than "Slow Sync", and the parenthesised
    /// "(Auto)"/"(On)" forms. Key -1 is `n/a`, which older Canon camcorders write.
    #[test]
    fn test_decode_flash_mode() {
        assert_eq!(FLASH_MODE.decode(-1), "n/a");
        assert_eq!(FLASH_MODE.decode(0), "Off");
        assert_eq!(FLASH_MODE.decode(1), "Auto");
        assert_eq!(FLASH_MODE.decode(2), "On");
        assert_eq!(FLASH_MODE.decode(3), "Red-eye reduction");
        assert_eq!(FLASH_MODE.decode(4), "Slow-sync");
        assert_eq!(FLASH_MODE.decode(5), "Red-eye reduction (Auto)");
        assert_eq!(FLASH_MODE.decode(6), "Red-eye reduction (On)");
        assert_eq!(FLASH_MODE.decode(16), "External flash");
        assert_eq!(FLASH_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_drive_mode() {
        assert_eq!(DRIVE_MODE.decode(0), "Single");
        assert_eq!(DRIVE_MODE.decode(1), "Continuous");
        assert_eq!(DRIVE_MODE.decode(2), "Movie");
        assert_eq!(DRIVE_MODE.decode(4), "Continuous, Speed Priority");
        assert_eq!(DRIVE_MODE.decode(5), "Continuous, Low");
        assert_eq!(DRIVE_MODE.decode(6), "Continuous, High");
        assert_eq!(DRIVE_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE.decode(0), "One-shot AF");
        assert_eq!(FOCUS_MODE.decode(1), "AI Servo AF");
        assert_eq!(FOCUS_MODE.decode(2), "AI Focus AF");
        assert_eq!(FOCUS_MODE.decode(3), "Manual Focus (3)");
        assert_eq!(FOCUS_MODE.decode(4), "Single");
        assert_eq!(FOCUS_MODE.decode(5), "Continuous");
        assert_eq!(FOCUS_MODE.decode(6), "Manual Focus (6)");
        assert_eq!(FOCUS_MODE.decode(16), "Pan Focus");
        assert_eq!(FOCUS_MODE.decode(99), "Unknown (99)");
    }

    /// `%Canon::CameraSettings` key 17 (Canon.pm:2434). Keys 0-2 exist and were missing
    /// here, so every older IXUS reported `Unknown (0)` for `Default`.
    #[test]
    fn test_decode_metering_mode() {
        assert_eq!(METERING_MODE.decode(0), "Default");
        assert_eq!(METERING_MODE.decode(1), "Spot");
        assert_eq!(METERING_MODE.decode(2), "Average");
        assert_eq!(METERING_MODE.decode(3), "Evaluative");
        assert_eq!(METERING_MODE.decode(4), "Partial");
        assert_eq!(METERING_MODE.decode(5), "Center-weighted average");
        assert_eq!(METERING_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_exposure_mode() {
        assert_eq!(EXPOSURE_MODE.decode(0), "Easy");
        assert_eq!(EXPOSURE_MODE.decode(1), "Program AE");
        assert_eq!(EXPOSURE_MODE.decode(2), "Shutter speed priority AE");
        assert_eq!(EXPOSURE_MODE.decode(3), "Aperture-priority AE");
        assert_eq!(EXPOSURE_MODE.decode(4), "Manual");
        assert_eq!(EXPOSURE_MODE.decode(5), "Depth-of-field AE");
        assert_eq!(EXPOSURE_MODE.decode(6), "M-Dep");
        assert_eq!(EXPOSURE_MODE.decode(7), "Bulb");
        assert_eq!(EXPOSURE_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_parse_camera_settings_array() {
        // Create Canon MakerNote with CameraSettings array
        let mut data = Vec::new();

        // Canon signature
        data.extend_from_slice(b"Canon");

        // IFD: 1 entry (CameraSettings)
        data.extend_from_slice(&[0x01, 0x00]); // Entry count (LE)

        // IFD Entry for CameraSettings (tag 0x0001)
        data.extend_from_slice(&[0x01, 0x00]); // Tag: CameraSettings
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x15, 0x00, 0x00, 0x00]); // Count: 21 values
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23 (5 sig + 2 count + 12 entry + 4 next = 23)

        // Next IFD offset
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        // CameraSettings array data at offset 20 (21 i16 values)
        let settings: Vec<i16> = vec![
            21, // [0] Array length
            2,  // [1] Macro mode: Normal
            0,  // [2] Self-timer: Off
            3,  // [3] Quality: Fine
            2,  // [4] Flash mode: On
            0,  // [5] Drive mode: Single
            0,  // [6] (unused)
            0,  // [7] Focus mode: One-shot AF
            0,  // [8] (unused)
            0,  // [9] (unused)
            1,  // [10] Image size: Large
            0,  // [11] Easy mode: Off
            0,  // [12] (unused)
            0,  // [13] Contrast: Normal
            0,  // [14] Saturation: Normal
            0,  // [15] Sharpness: Normal
            19, // [16] CameraISO code 19 -> ISO 400 (ExifTool Canon.pm:10475)
            3,  // [17] Metering mode: Evaluative
            0,  // [18] Focus type
            0,  // [19] AF point
            1,  // [20] Exposure mode: Program AE
        ];

        for value in settings {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // Verify extracted values
        assert_eq!(result.get("Canon:MacroMode"), Some(&"Normal".to_string()));
        assert_eq!(result.get("Canon:Quality"), Some(&"Fine".to_string()));
        assert_eq!(result.get("Canon:FlashMode"), Some(&"On".to_string()));
        assert_eq!(result.get("Canon:DriveMode"), Some(&"Single".to_string()));
        assert_eq!(
            result.get("Canon:FocusMode"),
            Some(&"One-shot AF".to_string())
        );
        assert_eq!(
            result.get("Canon:MeteringMode"),
            Some(&"Evaluative".to_string())
        );
        assert_eq!(
            result.get("Canon:ExposureMode"),
            Some(&"Program AE".to_string())
        );
        // `%Canon::CameraSettings` key 16 is CameraISO, whose ValueConv is a lookup
        // (Canon.pm:10464) - the slot is a code, not a literal speed, and there is no
        // `ISO` key in this table.
        assert_eq!(result.get("Canon:CameraISO"), Some(&"400".to_string()));
        assert_eq!(result.get("Canon:ISO"), None);
    }

    #[test]
    fn test_shot_info_indices() {
        assert_eq!(SHOT_INFO_AUTO_ISO, 1);
        assert_eq!(SHOT_INFO_BASE_ISO, 2);
        assert_eq!(SHOT_INFO_MEASURED_EV, 3);
        assert_eq!(SHOT_INFO_TARGET_APERTURE, 4);
        assert_eq!(SHOT_INFO_TARGET_SHUTTER_SPEED, 5);
        assert_eq!(SHOT_INFO_WHITE_BALANCE, 7);
        assert_eq!(SHOT_INFO_SLOW_SHUTTER, 8);
        assert_eq!(SHOT_INFO_SEQUENCE_NUMBER, 9);
        assert_eq!(SHOT_INFO_FLASH_GUIDE_NUMBER, 13);
        assert_eq!(SHOT_INFO_AF_POINTS_USED, 14);
        assert_eq!(SHOT_INFO_FLASH_EXPOSURE_COMP, 15);
        assert_eq!(SHOT_INFO_AUTO_EXPOSURE_BRACKETING, 16);
        assert_eq!(SHOT_INFO_SUBJECT_DISTANCE, 19);
    }

    #[test]
    fn test_parse_shot_info_array() {
        // Build test data without Canon signature for simpler offset calculation
        // IFD structure: entry_count(2) + entry(12) + next_ifd(4) = 18 bytes header
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // ShotInfo tag (0x0004)
        data.extend_from_slice(&[0x04, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Count: 20
        data.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]); // Offset: 18 (right after IFD header)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // ShotInfo array (20 values) starts at offset 18
        let shot_info: Vec<i16> = vec![
            20,  // [0] Array length
            100, // [1] Auto ISO
            100, // [2] Base ISO
            128, // [3] Measured EV
            160, // [4] Target aperture (f/5.6)
            96,  // [5] Target shutter speed (1/60)
            0,   // [6] (unused)
            0,   // [7] White balance: Auto
            0,   // [8] Slow shutter: Off
            0,   // [9] Sequence number
            0, 0, 0, 0, // [10-13]
            0, // [14] AF points used
            0, // [15] Flash exposure comp
            0, // [16] Auto exposure bracketing
            0, 0,    // [17-18]
            1000, // [19] Focus distance upper (cm) = 10.00 m
        ];

        for value in shot_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // AutoISO/BaseISO are log-scale codes: ExifTool applies
        // `exp($val/32*log(2))*100` and `.../32` respectively (Canon.pm:2778, 2789),
        // so raw 100 is ISO 872 / ISO 27, not ISO 100.
        assert_eq!(result.get("Canon:AutoISO"), Some(&"872".to_string()));
        assert_eq!(result.get("Canon:BaseISO"), Some(&"27".to_string()));
        // MeasuredEV carries ExifTool's empirical +5 offset (`$val / 32 + 5`).
        assert_eq!(result.get("Canon:MeasuredEV"), Some(&"9.00".to_string()));
        // `PrintConv => 'sprintf("%.2g",$val)'` (Canon.pm:2803) -- a bare number, with
        // no "f/" prefix.
        assert_eq!(result.get("Canon:TargetAperture"), Some(&"5.7".to_string()));
        assert_eq!(
            result.get("Canon:TargetExposureTime"),
            Some(&"1/8".to_string())
        );
        // `PrintConv => '$val > 655.345 ? "inf" : "$val m"'` interpolates the number
        // without padding, so 1000 cm prints as "10 m".
        assert_eq!(
            result.get("Canon:FocusDistanceUpper"),
            Some(&"10 m".to_string())
        );
    }

    /// `%Canon::ShotInfo` key 27 is AutoRotate; keys 26 and 28 are CameraType and
    /// NDFilter, so an off-by-one lands on a neighbour with a different meaning.
    #[test]
    fn test_parse_shot_info_auto_rotate() {
        let mut shot_info = vec![0i16; 30];
        shot_info[0] = 30;
        shot_info[26] = 252; // CameraType
        shot_info[27] = 2; // AutoRotate -> 'Rotate 180'
        shot_info[28] = -1; // NDFilter
        let data = canon_makernote_with_short_array(0x0004, &shot_info);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(
            result.get("Canon:AutoRotate"),
            Some(&"Rotate 180".to_string())
        );

        // ExifTool's RawConv discards negatives before PrintConv, so -1 emits nothing.
        shot_info[27] = -1;
        let data = canon_makernote_with_short_array(0x0004, &shot_info);
        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(result.get("Canon:AutoRotate"), None);
    }

    #[test]
    fn test_parse_focal_length_array() {
        // Build test data without Canon signature for simpler offset calculation
        // IFD structure: entry_count(2) + entry(12) + next_ifd(4) = 18 bytes header
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // FocalLength tag (0x0002)
        data.extend_from_slice(&[0x02, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
        data.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]); // Offset: 18 (right after IFD header)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // FocalLength array: [focal_type, focal_length, focal_plane_x_size, focal_plane_y_size]
        // focal_type: 2 (35mm equivalent available)
        // focal_length: 50mm (stored as 50)
        // focal_units: typically stored separately
        let focal_data: Vec<i16> = vec![2, 50, 0, 0];

        for value in focal_data {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // FocalType value 2 is decoded to "Zoom" using FOCAL_TYPE decoder
        assert_eq!(result.get("Canon:FocalType"), Some(&"Zoom".to_string()));
        assert_eq!(result.get("Canon:FocalLength"), Some(&"50 mm".to_string()));
    }

    #[test]
    fn test_parse_lens_model_tag() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // LensModel tag (0x0095)
        data.extend_from_slice(&[0x95, 0x00]); // Tag
        data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
        data.extend_from_slice(&[0x1E, 0x00, 0x00, 0x00]); // Count: 30 chars
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // Lens model string: "Canon EF 24-70mm f/2.8L II USM\0"
        let lens_name = b"Canon EF 24-70mm f/2.8L II USM\0";
        data.extend_from_slice(lens_name);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:LensModel"),
            Some(&"Canon EF 24-70mm f/2.8L II USM".to_string())
        );
    }

    /// Canon's int32 BinaryData sub-tables arrive as TIFF LONG records. This is the
    /// 16-byte layout of `%Canon::TimeInfo`: size, time-zone minutes, city, DST.
    /// The old generic path accepted only SHORT/UNDEFINED and silently skipped it.
    #[test]
    fn test_parse_long_binary_table_record() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        data.extend_from_slice(&0x0035u16.to_le_bytes()); // TimeInfo
        data.extend_from_slice(&[0x04, 0x00]); // Type: LONG
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4 int32 values
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        for value in [16i32, -420, 30, 60] {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(
            result.get("Canon:TimeZoneCity"),
            Some(&"Los Angeles".to_string())
        );
        assert_eq!(result.get("Canon:DaylightSavings"), Some(&"On".to_string()));
    }

    // ========================================================================
    // LensInfo (MakerNote tag 0x4019)
    // ========================================================================

    /// Byte-for-byte the LensInfo record of
    /// `/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS-1D_MarkIV.jpg`, as
    /// dumped by `exiftool -v3`: raw bytes `00 00 40 0e b1`, and ExifTool prints
    /// `[Canon] LensSerialNumber = 0000400eb1`. The first four bytes aren't all
    /// zero, so `RawConv` keeps it and `ValueConv => 'unpack("H*", $val)'` hexes
    /// all five.
    #[test]
    fn test_lens_info_serial_number() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        data.extend_from_slice(&CANON_LENS_INFO.to_le_bytes()); // Tag 0x4019
        data.extend_from_slice(&[0x07, 0x00]); // Type: UNDEFINED
        data.extend_from_slice(&[0x05, 0x00, 0x00, 0x00]); // Count: 5 bytes
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        data.extend_from_slice(&[0x00, 0x00, 0x40, 0x0e, 0xb1]);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(
            result.get("Canon:LensSerialNumber"),
            Some(&"0000400eb1".to_string())
        );
    }

    /// `RawConv => '$val=~/^\0\0\0\0/ ? undef : $val'`: when the first four bytes
    /// are all zero the tag is dropped entirely rather than hexed to all zeros.
    #[test]
    fn test_lens_info_serial_number_dropped_when_leading_bytes_are_zero() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]);

        data.extend_from_slice(&CANON_LENS_INFO.to_le_bytes());
        data.extend_from_slice(&[0x07, 0x00]);
        data.extend_from_slice(&[0x05, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x99]);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert!(!result.contains_key("Canon:LensSerialNumber"));
    }

    // ========================================================================
    // LevelInfo (MakerNote tag 0x4059)
    // ========================================================================

    /// Byte-for-byte the LevelInfo record of
    /// `/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS_R5m2.jpg`, as
    /// dumped by `exiftool -v3`. ExifTool prints RollAngle=0.3, PitchAngle=-11.1,
    /// FocalLength="70 mm", MinFocalLength2="24 mm", MaxFocalLength2="70 mm" --
    /// this pins the `key * 4` byte-offset math (see the comment on the
    /// `CANON_LEVEL_INFO` match arm) against real camera data.
    #[test]
    fn test_level_info_matches_exiftool() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        data.extend_from_slice(&CANON_LEVEL_INFO.to_le_bytes()); // Tag 0x4059
        data.extend_from_slice(&[0x07, 0x00]); // Type: UNDEFINED (declared int32u[10])
        data.extend_from_slice(&[0x28, 0x00, 0x00, 0x00]); // Count: 40 bytes
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        #[rustfmt::skip]
        let record: [u8; 40] = [
            0x28, 0x00, 0x00, 0x00, 0x80, 0xff, 0xff, 0xff, 0x00, 0x00, 0x26, 0x02,
            0x80, 0xd8, 0x6e, 0x01, 0x0d, 0x0e, 0x00, 0x00, 0xa1, 0x0d, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0xbc, 0x02, 0x00, 0x00, 0xf0, 0x00, 0x00, 0x00,
            0xbc, 0x02, 0x00, 0x00,
        ];
        data.extend_from_slice(&record);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(result.get("Canon:RollAngle"), Some(&"0.3".to_string()));
        assert_eq!(result.get("Canon:PitchAngle"), Some(&"-11.1".to_string()));
        assert_eq!(result.get("Canon:FocalLength"), Some(&"70 mm".to_string()));
        assert_eq!(
            result.get("Canon:MinFocalLength2"),
            Some(&"24 mm".to_string())
        );
        assert_eq!(
            result.get("Canon:MaxFocalLength2"),
            Some(&"70 mm".to_string())
        );
    }

    /// LevelInfo (tag 0x4059) is read after the standalone FocalLength tag
    /// (0x0002) in file order, and both are ExifTool's default priority 1, so
    /// LevelInfo's FocalLength must overwrite -- not lose to -- the earlier
    /// value, matching ExifTool's same-priority tie (last value wins).
    #[test]
    fn test_level_info_focal_length_overwrites_earlier_tag() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x02, 0x00]); // 2 entries

        // FocalLength (tag 0x0002): FocalType=1, FocalLength=24, packed inline.
        data.extend_from_slice(&CANON_FOCAL_LENGTH.to_le_bytes());
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Count: 2
        data.extend_from_slice(&[0x01, 0x00, 0x18, 0x00]); // Inline: [1, 24]

        // LevelInfo (tag 0x4059).
        data.extend_from_slice(&CANON_LEVEL_INFO.to_le_bytes());
        data.extend_from_slice(&[0x07, 0x00]); // Type: UNDEFINED
        data.extend_from_slice(&[0x28, 0x00, 0x00, 0x00]); // Count: 40 bytes
        data.extend_from_slice(&[0x23, 0x00, 0x00, 0x00]); // Offset: 35 (5 sig + 2 count + 2*12 entries + 4 next-IFD)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        #[rustfmt::skip]
        let record: [u8; 40] = [
            0x28, 0x00, 0x00, 0x00, 0x80, 0xff, 0xff, 0xff, 0x00, 0x00, 0x26, 0x02,
            0x80, 0xd8, 0x6e, 0x01, 0x0d, 0x0e, 0x00, 0x00, 0xa1, 0x0d, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0xbc, 0x02, 0x00, 0x00, 0xf0, 0x00, 0x00, 0x00,
            0xbc, 0x02, 0x00, 0x00,
        ];
        data.extend_from_slice(&record);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(result.get("Canon:FocalLength"), Some(&"70 mm".to_string()));
    }

    /// Byte-for-byte the CanonFileInfo record of
    /// `/tmp/oxidex-exiftool-cache/combined-samples/CanonRaw.cr2` (Canon EOS 350D), as
    /// dumped by `exiftool -v3`:
    ///
    /// ```text
    ///   | | |     - Tag 0x0093 (32 bytes, int16u[16] read as undef[32]):
    ///   | | |         0558: 20 00 00 19 18 00 00 00 00 00 00 00 ff ff ff ff
    ///   | | |         0568: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    /// ```
    ///
    /// `%Canon::FileInfo` has no LensID key - key 6 is RawJpgQuality - so a lens name
    /// must never be sourced from this record.
    #[test]
    fn test_parse_file_info_350d() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x02, 0x00]); // 2 entries

        // CanonImageType (0x0006) - selects the 20D/350D FileNumber variant
        data.extend_from_slice(&[0x06, 0x00]); // Tag
        data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
        data.extend_from_slice(&[0x18, 0x00, 0x00, 0x00]); // Count: 24
        data.extend_from_slice(&[0x23, 0x00, 0x00, 0x00]); // Offset: 35 (5 sig + 30 IFD)
        // FileInfo tag (0x0093)
        data.extend_from_slice(&[0x93, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // Count: 16
        data.extend_from_slice(&[0x3B, 0x00, 0x00, 0x00]); // Offset: 59 (35 + 24)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        let mut model = b"Canon EOS 350D DIGITAL".to_vec();
        model.resize(24, 0);
        data.extend_from_slice(&model);

        let file_info: Vec<i16> = vec![
            0x0020, // [0] record length in bytes
            0x1900, // [1] FileNumber low half (int32u spans slots 1-2)
            0x0018, // [2] FileNumber high half
            0,      // [3] BracketMode
            0,      // [4] BracketValue
            0,      // [5] BracketShotNumber
            -1,     // [6] RawJpgQuality (dropped: <= 0)
            -1,     // [7] RawJpgSize (dropped: < 0)
            0,      // [8] LongExposureNoiseReduction2
            0,      // [9] WBBracketMode
            0, 0, // [10-11]
            0, // [12] WBBracketValueAB
            0, // [13] WBBracketValueGM
            0, // [14] FilterEffect
            0, // [15] ToningEffect
        ];

        for value in file_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // Literal strings below are exactly what `exiftool -s` prints for this record.
        assert_eq!(
            result.get("Canon:FileNumber"),
            Some(&"100-0024".to_string())
        );
        assert_eq!(result.get("Canon:BracketMode"), Some(&"Off".to_string()));
        assert_eq!(
            result.get("Canon:LongExposureNoiseReduction2"),
            Some(&"Off".to_string())
        );
        assert_eq!(result.get("Canon:WBBracketMode"), Some(&"Off".to_string()));
        assert_eq!(result.get("Canon:WBBracketValueAB"), Some(&"0".to_string()));
        assert_eq!(result.get("Canon:WBBracketValueGM"), Some(&"0".to_string()));
        assert_eq!(result.get("Canon:FilterEffect"), Some(&"None".to_string()));
        assert_eq!(result.get("Canon:ToningEffect"), Some(&"None".to_string()));
        // %Canon::FileInfo defines neither of these - they must not be invented.
        assert_eq!(result.get("Canon:LensType"), None);
        assert_eq!(result.get("Canon:ShutterCount"), None);
    }

    /// Wraps a single Canon MakerNote IFD entry holding a SHORT array.
    fn canon_makernote_with_short_array(tag: u16, values: &[i16]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&(values.len() as u32).to_le_bytes());
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD
        for value in values {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data
    }

    /// Byte-for-byte the CanonAFInfo record of
    /// `/tmp/oxidex-exiftool-cache/combined-samples/CanonRaw.cr2` (Canon EOS 350D), as
    /// dumped by `exiftool -v3`:
    ///
    /// ```text
    ///   | | |     - Tag 0x0012 (48 bytes, int16u[24] read as undef[48]):
    ///   | | |         0520: 07 00 07 00 80 0d 00 09 80 0d 00 09 bd 00 bc 00
    ///   | | |         0530: 00 00 2b fb 1a fd 00 00 e6 02 d5 04 00 00 97 fd
    ///   | | |         0540: 00 00 00 00 00 00 00 00 00 00 69 02 08 00 ff ff
    /// ```
    #[test]
    fn test_parse_af_info_array() {
        let af_info: Vec<i16> = vec![
            7, 7, 3456, 2304, 3456, 2304, 189, 188, // keys 0-7
            0, -1237, -742, 0, 742, 1237, 0, // key 8: AFAreaXPositions[7]
            -617, 0, 0, 0, 0, 0, 617, // key 9: AFAreaYPositions[7]
            8,   // key 10: AFPointsInFocus (bit 3)
            -1,  // key 11
        ];
        let data = canon_makernote_with_short_array(0x0012, &af_info);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // Literal strings below are exactly what
        // `exiftool -s -G1 CanonRaw.cr2` prints for this record.
        assert_eq!(result.get("Canon:NumAFPoints"), Some(&"7".to_string()));
        assert_eq!(result.get("Canon:AFImageWidth"), Some(&"3456".to_string()));
        assert_eq!(result.get("Canon:AFImageHeight"), Some(&"2304".to_string()));
        assert_eq!(result.get("Canon:AFAreaWidth"), Some(&"189".to_string()));
        assert_eq!(result.get("Canon:AFAreaHeight"), Some(&"188".to_string()));
        assert_eq!(
            result.get("Canon:AFAreaXPositions"),
            Some(&"0 -1237 -742 0 742 1237 0".to_string())
        );
        assert_eq!(
            result.get("Canon:AFAreaYPositions"),
            Some(&"-617 0 0 0 0 0 617".to_string())
        );
        assert_eq!(result.get("Canon:AFPointsInFocus"), Some(&"3".to_string()));
        // %Canon::AFInfo has no AFPointsSelected key - it must not be invented.
        assert_eq!(result.get("Canon:AFPointsSelected"), None);
    }

    /// Mirrors the AFInfo2 record of
    /// `/tmp/oxidex-exiftool-cache/combined-samples/Canon1DmkIII.jpg`, whose
    /// `exiftool -s` output is `AFAreaMode: Single-point AF`, `NumAFPoints: 45`,
    /// `AFImageWidth: 3888`, `AFImageHeight: 2592`, `AFPointsInFocus: 13`.
    #[test]
    fn test_parse_af_info2_array() {
        let n = 45usize;
        let mut af_info2: Vec<i16> = vec![
            0,    // key 0: AFInfoSize
            2,    // key 1: AFAreaMode -> 'Single-point AF'
            45,   // key 2: NumAFPoints
            45,   // key 3: ValidAFPoints
            3888, // key 4: CanonImageWidth
            2592, // key 5: CanonImageHeight
            3888, // key 6: AFImageWidth
            2592, // key 7: AFImageHeight
        ];
        af_info2.extend(std::iter::repeat_n(112i16, n)); // key 8: AFAreaWidths
        af_info2.extend(std::iter::repeat_n(168i16, n)); // key 9: AFAreaHeights
        af_info2.extend(std::iter::repeat_n(-625i16, n)); // key 10: AFAreaXPositions
        af_info2.extend(std::iter::repeat_n(-554i16, n)); // key 11: AFAreaYPositions
        af_info2.extend_from_slice(&[0x2000, 0x0000, 0x0000]); // key 12: bit 13 set

        let data = canon_makernote_with_short_array(0x0026, &af_info2);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:AFAreaMode"),
            Some(&"Single-point AF".to_string())
        );
        assert_eq!(result.get("Canon:NumAFPoints"), Some(&"45".to_string()));
        assert_eq!(result.get("Canon:AFImageWidth"), Some(&"3888".to_string()));
        assert_eq!(result.get("Canon:AFImageHeight"), Some(&"2592".to_string()));
        assert_eq!(
            result.get("Canon:AFAreaWidths"),
            Some(&vec!["112"; n].join(" "))
        );
        assert_eq!(
            result.get("Canon:AFAreaHeights"),
            Some(&vec!["168"; n].join(" "))
        );
        assert_eq!(result.get("Canon:AFPointsInFocus"), Some(&"13".to_string()));
    }

    /// Byte-for-byte the SensorInfo record of `CanonRaw.cr2`, as dumped by
    /// `exiftool -v3`:
    ///
    /// ```text
    ///   | | |     - Tag 0x00e0 (34 bytes, int16u[17] read as undef[34]):
    ///   | | |         059e: 22 00 bc 0d 18 09 01 00 01 00 34 00 13 00 b3 0d
    ///   | | |         05ae: 12 09 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    ///   | | |         05be: 00 00
    /// ```
    #[test]
    fn test_parse_sensor_info_black_mask_borders() {
        let sensor_info: Vec<i16> = vec![
            34, 3516, 2328, 1, 1, 52, 19, 3507, 2322, // keys 0-8
            11, 22, 33, 44, // keys 9-12: BlackMask left/top/right/bottom
            0, 0, 0, 0,
        ];
        let data = canon_makernote_with_short_array(0x00E0, &sensor_info);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:BlackMaskLeftBorder"),
            Some(&"11".to_string())
        );
        assert_eq!(
            result.get("Canon:BlackMaskTopBorder"),
            Some(&"22".to_string())
        );
        assert_eq!(
            result.get("Canon:BlackMaskRightBorder"),
            Some(&"33".to_string())
        );
        assert_eq!(
            result.get("Canon:BlackMaskBottomBorder"),
            Some(&"44".to_string())
        );
    }

    /// Byte-for-byte the CanonFileInfo record of `CanonRaw.cr2`, as dumped by
    /// `exiftool -v3`:
    ///
    /// ```text
    ///   | | |     - Tag 0x0093 (32 bytes, int16u[16] read as undef[32]):
    ///   | | |         0558: 20 00 00 19 18 00 00 00 00 00 00 00 ff ff ff ff
    ///   | | |         0568: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    /// ```
    ///
    /// with the three bracket slots given distinct values so an off-by-one cannot pass.
    #[test]
    fn test_parse_file_info_bracket_slots() {
        let file_info: Vec<i16> = vec![
            32, 0x1900, 0x0018, // keys 0-2 (key 1 is the int32u FileNumber)
            1,      // key 3: BracketMode -> 'AEB'
            7,      // key 4: BracketValue
            9,      // key 5: BracketShotNumber
            -1, -1, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let data = canon_makernote_with_short_array(0x0093, &file_info);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(result.get("Canon:BracketMode"), Some(&"AEB".to_string()));
        assert_eq!(result.get("Canon:BracketValue"), Some(&"7".to_string()));
        assert_eq!(
            result.get("Canon:BracketShotNumber"),
            Some(&"9".to_string())
        );
    }

    #[test]
    fn test_decode_bits_16_matches_exiftool() {
        // ExifTool.pm DecodeBits: no lookup -> bit numbers joined by ',', '(none)' if empty.
        assert_eq!(decode_bits_16(&[8]), "3");
        assert_eq!(decode_bits_16(&[0]), "(none)");
        assert_eq!(decode_bits_16(&[0x0000, 0x0020]), "21");
        assert_eq!(decode_bits_16(&[0x0003]), "0,1");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = CanonParser;
        assert_eq!(parser.manufacturer_name(), "Canon");
        assert_eq!(parser.tag_prefix(), "Canon:");
    }

    #[test]
    fn test_validate_header() {
        let parser = CanonParser;

        // Test with Canon signature
        let with_signature = b"Canon\x00\x01\x00extra";
        assert!(parser.validate_header(with_signature));

        // Test without signature but valid IFD (reasonable entry count)
        let without_signature = b"\x05\x00extra_data_here_to_make_it_longer";
        assert!(parser.validate_header(without_signature));

        // Test invalid data (unreasonable entry count)
        let invalid = b"\xFF\xFF";
        assert!(!parser.validate_header(invalid));

        // Test too short data
        let too_short = b"\x01";
        assert!(!parser.validate_header(too_short));
    }

    /// Spot-checks against `%canonLensTypes`. Every expected string below is
    /// the literal right-hand side of a Canon.pm line, quoted in the comment.
    #[test]
    fn test_lens_lookup() {
        let parser = CanonParser;

        // Canon.pm:99  `1 => 'Canon EF 50mm f/1.8',`
        assert_eq!(
            parser.lookup_lens(1),
            Some("Canon EF 50mm f/1.8".to_string())
        );

        // An id shared by several lenses prints ExifTool's own combined string;
        // only Composite:LensID narrows it, and that is not this tag.
        // Canon.pm:480  `368 => 'Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens',`
        assert_eq!(
            parser.lookup_lens(368),
            Some("Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string())
        );

        // Canon.pm:583  `61182 => 'Canon RF 50mm F1.2L USM or other Canon RF Lens',`
        // The 68 RF lenses ExifTool files under 61182.1-61182.68 all report this
        // id; none of them has an id of its own.
        assert_eq!(
            parser.lookup_lens(61182),
            Some("Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string())
        );

        // Canon.pm:652  `65535 => 'n/a',` - what a body with no lens reports.
        assert_eq!(parser.lookup_lens(65535), Some("n/a".to_string()));

        // Absent from %canonLensTypes.
        assert_eq!(parser.lookup_lens(65000), None);
        assert_eq!(parser.lookup_lens(61183), None);
    }

    // ========================================================================
    // Tests for newly added tags (Phase 4 - Extended Canon MakerNotes)
    // ========================================================================

    #[test]
    fn test_decode_color_space() {
        assert_eq!(COLOR_SPACE.decode(1), "sRGB");
        assert_eq!(COLOR_SPACE.decode(2), "Adobe RGB");
        assert_eq!(COLOR_SPACE.decode(65535), "Uncalibrated");
        assert_eq!(COLOR_SPACE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_picture_style() {
        assert_eq!(PICTURE_STYLE.decode(0x0081), "Standard");
        assert_eq!(PICTURE_STYLE.decode(0x0082), "Portrait");
        assert_eq!(PICTURE_STYLE.decode(0x0083), "Landscape");
        assert_eq!(PICTURE_STYLE.decode(0x0084), "Neutral");
        assert_eq!(PICTURE_STYLE.decode(0x0085), "Faithful");
        assert_eq!(PICTURE_STYLE.decode(0x0086), "Monochrome");
        assert_eq!(PICTURE_STYLE.decode(0x0087), "Auto");
        assert_eq!(PICTURE_STYLE.decode(0x0088), "Fine Detail");
        assert_eq!(PICTURE_STYLE.decode(0x0021), "User Def. 1");
    }

    #[test]
    fn test_decode_tone_curve() {
        assert_eq!(TONE_CURVE.decode(0), "Standard");
        assert_eq!(TONE_CURVE.decode(1), "Manual");
        assert_eq!(TONE_CURVE.decode(2), "Custom");
        assert_eq!(TONE_CURVE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_canon_tag_to_name_extended() {
        // Test new tags added in Phase 4
        assert_eq!(canon_tag_to_name(0x0003), "Canon:FlashInfo");
        assert_eq!(canon_tag_to_name(0x0012), "Canon:AFInfo");
        assert_eq!(canon_tag_to_name(0x0015), "Canon:SerialNumberFormat");
        assert_eq!(canon_tag_to_name(0x0026), "Canon:AFInfo2");
        assert_eq!(canon_tag_to_name(0x0093), "Canon:FileInfo");
        assert_eq!(canon_tag_to_name(0x0095), "Canon:LensModel");
        assert_eq!(canon_tag_to_name(0x0096), "Canon:InternalSerialNumber");
        assert_eq!(canon_tag_to_name(0x00A0), "Canon:ProcessingInfo");
        assert_eq!(canon_tag_to_name(0x00AA), "Canon:MeasuredColor");
        assert_eq!(canon_tag_to_name(0x00B4), "Canon:ColorSpace");
        assert_eq!(canon_tag_to_name(0x00D0), "Canon:VRDOffset");
    }

    #[test]
    fn test_flash_info_indices() {
        // Verify FlashInfo array indices
        assert_eq!(FLASH_INFO_FLASH_GUIDE_NUMBER, 0);
        assert_eq!(FLASH_INFO_FLASH_THRESHOLD, 1);
    }

    #[test]
    fn test_processing_info_indices() {
        // Verify ProcessingInfo array indices
        assert_eq!(PROCESSING_INFO_TONE_CURVE, 1);
        assert_eq!(PROCESSING_INFO_SHARPNESS, 2);
        assert_eq!(PROCESSING_INFO_SHARPNESS_FREQ, 3);
        assert_eq!(PROCESSING_INFO_SENSOR_RED_LEVEL, 4);
        assert_eq!(PROCESSING_INFO_SENSOR_BLUE_LEVEL, 5);
        assert_eq!(PROCESSING_INFO_WHITE_BALANCE_RED, 6);
        assert_eq!(PROCESSING_INFO_WHITE_BALANCE_BLUE, 7);
        assert_eq!(PROCESSING_INFO_WHITE_BALANCE, 8);
        assert_eq!(PROCESSING_INFO_COLOR_TEMPERATURE, 9);
        assert_eq!(PROCESSING_INFO_PICTURE_STYLE, 10);
        assert_eq!(PROCESSING_INFO_DIGITAL_GAIN, 11);
        assert_eq!(PROCESSING_INFO_WB_SHIFT_AB, 12);
        assert_eq!(PROCESSING_INFO_WB_SHIFT_GM, 13);
    }

    /// `%Canon::MeasuredColor` has a single named key: `1 => MeasuredRGGB`, an
    /// `int16u[4]`. There are no separate red/green/blue/temperature keys, and
    /// `FIRST_ENTRY => 1` means the array does not start at index 0.
    #[test]
    fn test_measured_color_indices() {
        assert_eq!(MEASURED_COLOR_RGGB, 1);
    }

    /// ExifTool Canon.pm:2735 anchors every alternative of the FocalPlane*Size
    /// `Condition` to the end of the model name:
    ///
    /// ```text
    ///     $$self{Model} !~ /EOS/ or
    ///     $$self{Model} =~ /\b(1DS?|5D|D30|D60|10D|20D|30D|K236)$/ or
    ///     $$self{Model} =~ /\b((300D|350D|400D) DIGITAL|REBEL( XTi?)?|Kiss Digital( [NX])?)$/
    /// ```
    ///
    /// Dropping the anchor would hand every later Rebel a focal plane size read out of
    /// slots that hold something else on those bodies.
    #[test]
    fn test_focal_plane_size_supported_is_end_anchored() {
        // Non-EOS bodies always qualify.
        assert!(focal_plane_size_supported("Canon PowerShot S30"));
        // Listed bodies, at the end of the name.
        assert!(focal_plane_size_supported("Canon EOS 350D DIGITAL"));
        assert!(focal_plane_size_supported("Canon EOS DIGITAL REBEL XT"));
        assert!(focal_plane_size_supported("Canon EOS Kiss Digital N"));
        assert!(focal_plane_size_supported("Canon EOS 5D"));
        assert!(focal_plane_size_supported("Canon EOS 20D"));
        // Same tokens, but not at the end: ExifTool's `$` rejects these.
        assert!(!focal_plane_size_supported("Canon EOS REBEL T3i"));
        assert!(!focal_plane_size_supported("Canon EOS Kiss Digital X4"));
        assert!(!focal_plane_size_supported("Canon EOS 5D Mark III"));
        assert!(!focal_plane_size_supported("Canon EOS 350D DIGITAL X"));
        // Unlisted EOS bodies never qualify.
        assert!(!focal_plane_size_supported("Canon EOS R5"));
        assert!(!focal_plane_size_supported("Canon EOS 40D"));
    }

    /// The 20D/350D family selects `%Canon::FileInfo` key 1's `FileNumber` variant and
    /// `%Canon::ShotInfo` key 22's first `ExposureTime` variant (Canon.pm:6850, 2968);
    /// `%CanonCustom::Functions350D` (Canon.pm:1542) excludes the 20D from that set.
    #[test]
    fn test_model_conditions() {
        for model in [
            "Canon EOS 20D",
            "Canon EOS 350D DIGITAL",
            "Canon EOS DIGITAL REBEL XT",
            "Canon EOS Kiss Digital N",
        ] {
            assert!(is_20d_or_350d(model), "{model}");
        }
        assert!(!is_20d_or_350d("Canon EOS 30D"));
        assert!(!is_20d_or_350d("Canon EOS 400D DIGITAL"));

        assert!(is_350d_custom_functions("Canon EOS 350D DIGITAL"));
        assert!(is_350d_custom_functions("Canon EOS DIGITAL REBEL XT"));
        // The 400D redefines keys 0 and 1, and the 20D uses a different table entirely.
        assert!(!is_350d_custom_functions("Canon EOS 20D"));
        assert!(!is_350d_custom_functions("Canon EOS 400D DIGITAL"));
    }

    /// DNG files may carry the camera model only in IFD0, not in Canon's
    /// `CanonImageType` MakerNote tag. ExifTool selects `Functions350D` from
    /// the IFD0 Model, so the dispatcher-provided model must control this table.
    #[test]
    fn test_custom_functions_uses_external_model_when_image_type_is_absent() {
        let record = [
            0x14, 0x00, // record byte length
            0x00, 0x00, // SetButtonCrossKeysFunc: Normal
            0x00, 0x01, // LongExposureNoiseReduction: Off
            0x00, 0x02, // FlashSyncSpeedAv: Auto
            0x00, 0x03, // Shutter-AELock: AF/AE lock
            0x00, 0x04, // AFAssistBeam: Emits
            0x00, 0x05, // ExposureLevelIncrements: 1/3 Stop
            0x00, 0x06, // MirrorLockup: Disable
            0x00, 0x07, // ETTLII: Evaluative
            0x00, 0x08, // ShutterCurtainSync: 1st-curtain sync
        ];

        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&CANON_CUSTOM_FUNCTIONS.to_le_bytes());
        data.extend_from_slice(&7u16.to_le_bytes()); // UNDEFINED
        data.extend_from_slice(&(record.len() as u32).to_le_bytes());
        data.extend_from_slice(&18u32.to_le_bytes()); // packed value offset
        data.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        data.extend_from_slice(&record);

        let tags = parse_canon_makernote_impl_with_model(
            &data,
            ByteOrder::LittleEndian,
            Some("Canon EOS 350D DIGITAL"),
        )
        .unwrap();

        for (name, expected) in [
            ("AFAssistBeam", "Emits"),
            ("ETTLII", "Evaluative"),
            ("ExposureLevelIncrements", "1/3 Stop"),
            ("FlashSyncSpeedAv", "Auto"),
            ("LongExposureNoiseReduction", "Off"),
            ("MirrorLockup", "Disable"),
        ] {
            assert_eq!(
                tags.get(&format!("CanonCustom:{name}")).map(String::as_str),
                Some(expected),
                "{name}"
            );
        }
    }

    /// `has_word` stands in for Perl's `\b`, so a model token must not match inside a
    /// longer alphanumeric run.
    #[test]
    fn test_has_word_respects_boundaries() {
        assert!(has_word("Canon EOS 5D", "5D"));
        assert!(has_word("Canon EOS 5D Mark III", "5D"));
        assert!(!has_word("Canon EOS 15D", "5D"));
        assert!(!has_word("Canon EOS 5DS", "5D"));
    }

    // TODO: Enable these tests once ProcessingInfo array parsing is implemented
    // These tests verify correct parsing of Canon ProcessingInfo, MeasuredColor,
    // and FlashInfo arrays. Currently disabled as the parser doesn't extract
    // individual fields from these arrays.
    /*
    #[test]
    fn test_parse_processing_info_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // ProcessingInfo tag (0x00A0)
        data.extend_from_slice(&[0xA0, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // Count: 16
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // ProcessingInfo array (16 values)
        let processing_info: Vec<i16> = vec![
            16,     // [0] Array length
            0,      // [1] Tone curve: Standard
            3,      // [2] Sharpness: 3
            1,      // [3] Sharpness frequency: 1
            0,      // [4] Sensor red level
            0,      // [5] Sensor blue level
            0,      // [6] WB red
            0,      // [7] WB blue
            0,      // [8] White balance
            5500,   // [9] Color temperature: 5500K
            0x0081, // [10] Picture style: Standard
            0,      // [11] Digital gain
            0,      // [12] WB shift A-B
            0,      // [13] WB shift G-M
            0, 0, // [14-15] padding
        ];

        for value in processing_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(result.get("Canon:ToneCurve"), Some(&"Standard".to_string()));
        assert_eq!(result.get("Canon:Sharpness"), Some(&"3".to_string()));
        assert_eq!(
            result.get("Canon:SharpnessFrequency"),
            Some(&"1".to_string())
        );
        assert_eq!(
            result.get("Canon:ColorTemperature"),
            Some(&"5500 K".to_string())
        );
        assert_eq!(
            result.get("Canon:PictureStyle"),
            Some(&"Standard".to_string())
        );
    }

    #[test]
    fn test_parse_measured_color_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // MeasuredColor tag (0x00AA)
        data.extend_from_slice(&[0xAA, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // MeasuredColor array: [red, green, blue, temperature]
        let measured_color: Vec<i16> = vec![
            1024, // Red
            1000, // Green
            980,  // Blue
            5200, // Color temperature in K
        ];

        for value in measured_color {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:MeasuredRGGB_R"),
            Some(&"1024".to_string())
        );
        assert_eq!(
            result.get("Canon:MeasuredRGGB_G"),
            Some(&"1000".to_string())
        );
        assert_eq!(result.get("Canon:MeasuredRGGB_B"), Some(&"980".to_string()));
        assert_eq!(
            result.get("Canon:MeasuredColorTemperature"),
            Some(&"5200 K".to_string())
        );
    }

    #[test]
    fn test_parse_flash_info_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // FlashInfo tag (0x0003)
        data.extend_from_slice(&[0x03, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // FlashInfo array: [guide_number, threshold, ...]
        let flash_info: Vec<i16> = vec![
            14,  // Guide number
            256, // Threshold
            0,   // unused
            0,   // unused
        ];

        for value in flash_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:FlashGuideNumber"),
            Some(&"14".to_string())
        );
        assert_eq!(result.get("Canon:FlashThreshold"), Some(&"256".to_string()));
    }
    */

    // ========================================================================
    // MakerNote base recovery (see `calculate_makernote_base`)
    // ========================================================================

    /// Builds a little-endian Canon MakerNote whose value block starts `gap` bytes after
    /// the IFD header, and whose value offsets are stated relative to `base`.
    ///
    /// The single offset-based entry is a `CameraSettings`-shaped record: an int16 array
    /// whose first word is its own size in bytes, exactly as every `FIRST_ENTRY => 1`
    /// `%Canon` table is laid out.
    fn build_makernote_with_gap(base: u32, gap: usize, elements: usize) -> Vec<u8> {
        // Two records, so a candidate base can collect the corroborating votes
        // MAKERNOTE_BASE_MIN_VOTES requires -- a single agreement is treated as chance.
        let entry_count = 2usize;
        let header_size = 2 + entry_count * 12 + 4;
        let first_start = header_size + gap;
        let second_start = first_start + elements * 2;
        let mut data = Vec::new();
        data.extend_from_slice(&(entry_count as u16).to_le_bytes());
        for (tag, start) in [(0x0001u16, first_start), (0x0004u16, second_start)] {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&3u16.to_le_bytes()); // SHORT
            data.extend_from_slice(&(elements as u32).to_le_bytes());
            data.extend_from_slice(&(base + start as u32).to_le_bytes());
        }
        data.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        data.resize(first_start, 0);
        for _ in 0..2 {
            // record[0] is the record's own size in bytes
            data.extend_from_slice(&((elements * 2) as u16).to_le_bytes());
            for index in 1..elements {
                data.extend_from_slice(&(index as i16).to_le_bytes());
            }
        }
        data
    }

    #[test]
    fn test_makernote_base_packed_layout_is_unchanged() {
        let data = build_makernote_with_gap(1082, 0, 32);
        assert_eq!(
            calculate_makernote_base(&data, ByteOrder::LittleEndian),
            Some(1082)
        );
    }

    /// The regression this whole search exists for: Canon leaves a gap between the IFD
    /// header and the value block on a large share of bodies. Assuming no gap puts the
    /// base `gap` bytes too high, which shifts every record by `gap / 2` slots.
    #[test]
    fn test_makernote_base_recovers_padded_layout() {
        for gap in [2usize, 12, 24] {
            let data = build_makernote_with_gap(734, gap, 48);
            assert_eq!(
                calculate_makernote_base(&data, ByteOrder::LittleEndian),
                Some(734),
                "gap of {gap} bytes"
            );
        }
    }

    #[test]
    fn test_makernote_base_falls_back_without_evidence() {
        // One offset-based entry that does NOT declare its own size: no record can vote,
        // so the packed-layout guess stands.
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0x0095u16.to_le_bytes()); // LensModel (a string, not a record)
        data.extend_from_slice(&2u16.to_le_bytes()); // ASCII
        data.extend_from_slice(&64u32.to_le_bytes());
        data.extend_from_slice(&(500u32 + 18).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.resize(18 + 64, b'x');
        assert_eq!(
            calculate_makernote_base(&data, ByteOrder::LittleEndian),
            Some(500)
        );
    }

    // ========================================================================
    // Base recovery from the TIFF footer (ExifTool FixBase, MakerNotes.pm:1306)
    // ========================================================================

    /// [`build_makernote_with_gap`] plus Canon's 8-byte TIFF footer recording
    /// `recorded_base` as the offset the directory was written at.
    fn build_makernote_with_footer(
        written_base: u32,
        gap: usize,
        elements: usize,
        recorded_base: u32,
        magic: &[u8; 4],
    ) -> Vec<u8> {
        let mut data = build_makernote_with_gap(written_base, gap, elements);
        data.extend_from_slice(magic);
        data.extend_from_slice(&recorded_base.to_le_bytes());
        data
    }

    /// The whole point: a MakerNote that was relocated without its offsets being
    /// rewritten. The records still address the old position, and the footer is
    /// the only thing that still records it.
    #[test]
    fn footer_supplies_the_base_the_offsets_were_written_against() {
        let data = build_makernote_with_footer(764, 0, 32, 764, b"II\x2a\x00");
        // The directory now sits at 4978 -- ExifTool's "Adjusted MakerNotes base
        // by 4214" on CanonPowerShotSX600HS.jpg.
        assert_eq!(
            canon_makernote_base_located(&data, &data, ByteOrder::LittleEndian, 4978),
            764
        );
    }

    /// Without a footer there is nothing to adjust, so the base is simply where
    /// the directory sits -- which is what `$valuePtr -= $dataPos` amounts to.
    #[test]
    fn no_footer_means_the_base_is_where_the_directory_sits() {
        let data = build_makernote_with_gap(852, 24, 32);
        assert_eq!(
            canon_makernote_base_located(&data, &data, ByteOrder::LittleEndian, 852),
            852
        );
    }

    /// ExifTool validates the footer's byte order against the directory's
    /// (`substr($footer,0,2) eq GetByteOrder()`): an "MM" footer read
    /// little-endian is not a footer.
    #[test]
    fn a_footer_in_the_other_byte_order_is_not_a_footer() {
        let data = build_makernote_with_footer(764, 0, 32, 764, b"MM\x00\x2a");
        assert_eq!(canon_footer_base(&data, ByteOrder::LittleEndian), None);
        assert_eq!(
            canon_makernote_base_located(&data, &data, ByteOrder::LittleEndian, 4978),
            4978
        );
    }

    /// A block too short to hold a footer must not have its last 8 bytes read as
    /// one (`$$dirInfo{DirLen} > 8`).
    #[test]
    fn a_block_of_eight_bytes_or_fewer_has_no_footer() {
        assert_eq!(
            canon_footer_base(b"II\x2a\x00\x00\x03\x00\x00", ByteOrder::LittleEndian),
            None
        );
    }

    /// Picasa and ACDSee rewrite the offsets and leave the footer stale. ExifTool
    /// detects that by the last value ending exactly where the directory does,
    /// and ignores the footer.
    #[test]
    fn a_footer_left_behind_by_an_offset_rewrite_is_ignored() {
        // Offsets rewritten for the directory's *current* position, so the last
        // value ends exactly 8 bytes (the footer) before the block's end.
        let gap = 0usize;
        let elements = 32usize;
        let dir_offset = 4978u32;
        let data = build_makernote_with_footer(dir_offset, gap, elements, 764, b"II\x2a\x00");
        let (entries, _) = canon_offset_entries(&data, ByteOrder::LittleEndian).unwrap();
        assert!(canon_footer_is_stale(&entries, dir_offset, data.len()));
        assert_eq!(
            canon_makernote_base_located(&data, &data, ByteOrder::LittleEndian, dir_offset),
            dir_offset,
            "a stale footer must not move the base"
        );
    }

    /// `GetValueBlocks` skips only values of four bytes or fewer -- an inline
    /// value has no offset to record.
    #[test]
    fn inline_values_are_not_offset_entries() {
        let mut data = Vec::new();
        data.extend_from_slice(&2u16.to_le_bytes());
        // 0x0001: SHORT[1] = 2 bytes, inline
        data.extend_from_slice(&0x0001u16.to_le_bytes());
        data.extend_from_slice(&3u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&7u32.to_le_bytes());
        // 0x0002: SHORT[8] = 16 bytes, offset-based
        data.extend_from_slice(&0x0002u16.to_le_bytes());
        data.extend_from_slice(&3u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.resize(64, 0);
        let (entries, _) = canon_offset_entries(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(entries, vec![(100u32, 16usize)]);
    }

    // ========================================================================
    // CustomFunctions2 (MakerNote tag 0x0099)
    // ========================================================================

    /// Assembles one `CustomFunctions2` record holding a single group of entries.
    fn build_custom_functions2(entries: &[(u32, &[i32])]) -> Vec<u8> {
        let mut group = Vec::new();
        for (tag, values) in entries {
            group.extend_from_slice(&tag.to_le_bytes());
            group.extend_from_slice(&(values.len() as u32).to_le_bytes());
            for value in *values {
                group.extend_from_slice(&value.to_le_bytes());
            }
        }
        let mut data = Vec::new();
        let total = 8 + 12 + group.len();
        data.extend_from_slice(&(total as u16).to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes()); // one group
        data.extend_from_slice(&1u32.to_le_bytes()); // group number
        data.extend_from_slice(&((group.len() + 8) as u32).to_le_bytes());
        data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        data.extend_from_slice(&group);
        assert_eq!(data.len(), total);
        data
    }

    #[test]
    fn test_custom_functions2_decodes_lookup_and_bitmask() {
        let data = build_custom_functions2(&[
            (0x0102, &[1]),           // ISOSpeedIncrements
            (0x040a, &[0b0000_0111]), // ViewfinderWarnings (BITMASK)
            (0x0518, &[0]),           // AccelerationTracking (no PrintConv)
        ]);
        let mut tags = HashMap::new();
        custom_functions2::parse_custom_functions2(
            &data,
            ByteOrder::LittleEndian,
            "Canon EOS 7D",
            &mut tags,
        );

        assert_eq!(
            tags.get("CanonCustom:ISOSpeedIncrements"),
            Some(&"1 Stop".to_string())
        );
        assert_eq!(
            tags.get("CanonCustom:ViewfinderWarnings"),
            Some(&"Monochrome, WB corrected, One-touch image quality".to_string())
        );
        assert_eq!(
            tags.get("CanonCustom:AccelerationTracking"),
            Some(&"0".to_string())
        );
    }

    #[test]
    fn test_custom_functions2_bitmask_with_no_bits_set() {
        let data = build_custom_functions2(&[(0x040a, &[0])]);
        let mut tags = HashMap::new();
        custom_functions2::parse_custom_functions2(
            &data,
            ByteOrder::LittleEndian,
            "Canon EOS 7D",
            &mut tags,
        );
        assert_eq!(
            tags.get("CanonCustom:ViewfinderWarnings"),
            Some(&"(none)".to_string())
        );
    }

    /// A record whose leading length word disagrees with its actual size is rejected
    /// outright, the same way ExifTool warns "Invalid CanonCustom2 data" and bails.
    #[test]
    fn test_custom_functions2_rejects_bad_length() {
        let mut data = build_custom_functions2(&[(0x0102, &[1])]);
        data[0] = data[0].wrapping_add(4);
        let mut tags = HashMap::new();
        custom_functions2::parse_custom_functions2(
            &data,
            ByteOrder::LittleEndian,
            "Canon EOS 7D",
            &mut tags,
        );
        assert!(tags.is_empty());
    }

    /// A two-converter entry that arrives with a single value runs only the first
    /// converter: ExifTool's loop ends when it runs out of *values*, not converters
    /// (ExifTool.pm:3673). `MultiFunctionLock` on the EOS-1DXmkIII stores 66, and
    /// `exiftool -a -G1 -s Canon1DXmkIII.jpg` prints exactly this.
    #[test]
    fn test_custom_functions2_single_value_uses_the_first_converter() {
        let data = build_custom_functions2(&[(0x070f, &[66])]); // MultiFunctionLock
        let mut tags = HashMap::new();
        custom_functions2::parse_custom_functions2(
            &data,
            ByteOrder::LittleEndian,
            "Canon EOS-1D X Mark III",
            &mut tags,
        );
        assert_eq!(
            tags.get("CanonCustom:MultiFunctionLock"),
            Some(&"Unknown (66)".to_string())
        );
    }

    // ========================================================================
    // PrintConv fidelity
    // ========================================================================

    #[test]
    fn test_convert_personal_function() {
        assert_eq!(convert_personal_function(0), "Off");
        assert_eq!(convert_personal_function(1), "On");
        assert_eq!(convert_personal_function(7), "On (7)");
    }

    /// `sprintf("%.2g",$val)` over `2 ** (CanonEv($val) / 2)` -- and no "f/" prefix.
    #[test]
    fn test_canon_aperture_matches_exiftool() {
        assert_eq!(canon_aperture(0), None);
        assert_eq!(canon_aperture(-32), None);
        assert_eq!(canon_aperture(96).as_deref(), Some("2.8"));
        assert_eq!(canon_aperture(160).as_deref(), Some("5.7"));
        assert_eq!(canon_aperture(256).as_deref(), Some("16"));
    }

    /// ExifTool's bit NUMBERS, which are not evenly spaced: 4, then 7, 11, 13, 14.
    #[test]
    fn test_decode_flash_bits() {
        assert_eq!(decode_flash_bits(0), "(none)");
        assert_eq!(decode_flash_bits(0x0008), "E-TTL");
        assert_eq!(decode_flash_bits(0x2000), "Built-in");
        assert_eq!(decode_flash_bits(0x4000), "External");
        assert_eq!(decode_flash_bits(0x0080), "2nd-curtain sync used");
        assert_eq!(decode_flash_bits(0x2008), "E-TTL, Built-in");
    }

    /// `"$val mm"` interpolates Perl's own stringification of the quotient, so a body
    /// with 1/16 mm focal units prints every sixteenth, not a rounded tenth.
    #[test]
    fn test_format_focal_length_keeps_full_precision() {
        assert_eq!(format_focal_length(259, 16), "16.1875 mm");
        assert_eq!(format_focal_length(50, 1), "50 mm");
        // int16u (Canon.pm:2578/2586): reading it signed wraps every focal length past
        // 32767 raw units negative -- an IXUS on 1000/mm units reported "-32.536 mm" for
        // "33 mm", on 84 corpus samples.
        assert_eq!(format_focal_length(33000, 1000), "33 mm");
        // MinFocalLength has no RawConv dropping zero, so ExifTool prints "0 mm".
        assert_eq!(format_focal_length(0, 1), "0 mm");
        assert_eq!(format_focal_length(10, 0), "n/a");
    }

    /// CameraType is a ShotInfo slot, not a function of the model id.
    #[test]
    fn test_decode_camera_type() {
        assert_eq!(decode_camera_type(0), "n/a");
        assert_eq!(decode_camera_type(248), "EOS High-end");
        assert_eq!(decode_camera_type(250), "Compact");
        assert_eq!(decode_camera_type(252), "EOS Mid-range");
        assert_eq!(decode_camera_type(255), "DV Camera");
        assert_eq!(decode_camera_type(7), "Unknown (7)");
    }

    #[test]
    fn test_decode_canon_model_id_covers_camcorders() {
        // 0x4007d673 -- the id the DC19/DC21/DC22 camcorders write, previously unlisted
        // by this parser's hand-maintained match arm and reported as "Unknown".
        assert_eq!(decode_canon_model_id(0x4007d673), "DC19/DC21/DC22");
        assert_eq!(decode_canon_model_id(0x80000285), "EOS 5D Mark III");
        assert_eq!(decode_canon_model_id(0xdeadbeef), "Unknown (3735928559)");
    }
}
