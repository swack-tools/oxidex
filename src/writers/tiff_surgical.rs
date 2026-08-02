//! Surgical whole-file TIFF rewriting (TIFF, and the TIFF-structured RAW
//! containers: NEF, CR2, IIQ, RW2, ARW, ORF, PEF, DNG, ...)
//!
//! # Why this exists
//!
//! `tiff_writer` rebuilds a TIFF from the metadata map alone. For a JPEG's
//! EXIF block that merely loses fidelity; for a TIFF *file* it is fatal —
//! the file **is** the TIFF, so a rebuild throws away the image strips and
//! every SubIFD chain the map never surfaced. That is why
//! `write_metadata` refused TIFF outright ("the TIFF writer does not
//! preserve image data").
//!
//! # The approach
//!
//! Every value in a TIFF is reached through an absolute file offset, so the
//! safe edit is the one that **moves nothing**:
//!
//! - A changed value that still fits the entry's 4-byte inline field is
//!   written straight into the entry record.
//! - A changed value that does not fit is **appended at end-of-file** and
//!   the entry's type/count/offset triple is repointed. The old value bytes
//!   become dead space; nothing else in the file shifts.
//! - A **new** tag needs a longer entry table, so the whole IFD table is
//!   copied to end-of-file with the extra record merged in tag-id order,
//!   and the one pointer that reaches it is repointed (the TIFF header for
//!   IFD0, IFD0's 0x8769/0x8825 record for ExifIFD/GPS). Copied records keep
//!   their original value offsets, so they still resolve.
//!
//! Consequence: image strips/tiles, SubIFD chains, MakerNotes, IFD1
//! thumbnails and every unparsed byte survive bit-for-bit, because they are
//! never read, moved, or re-serialized — only appended past.
//!
//! # Validation
//!
//! Mirrors `exif_surgical`: only values the caller actually changed are
//! validated. A value equal to what the reader produced from this very file
//! is carried by *not touching its bytes at all*, so re-validating it could
//! not protect anything.

use crate::core::metadata_map::MetadataMap;
use crate::core::operations_helpers::{read_u16, read_u32};

use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::tag_db::lookup_tag_name;
use crate::tag_db::tag_registry::{declared_ieee_field_type, get_tag_descriptor};
use crate::writers::exif_surgical::{
    IfdKind, descriptor_tag_id, native_to_byte_order, tag_value_to_field, validate_changed,
};

/// IFD0 tag pointing to the ExifIFD
const EXIF_IFD_POINTER: u16 = 0x8769;
/// IFD0 tag pointing to the GPS IFD
const GPS_IFD_POINTER: u16 = 0x8825;
/// TIFF LONG field type (used for the synthesized sub-IFD pointers)
const LONG_TYPE: u16 = 4;

/// One IFD entry located in the original file by the byte offset of its
/// 12-byte record, which is what makes an in-place patch possible.
#[derive(Debug, Clone)]
struct LocatedEntry {
    ifd: IfdKind,
    tag_id: u16,
    field_type: u16,
    /// File offset of the entry's 12-byte record
    record_offset: usize,
}

/// The walked structure of a TIFF file: enough to patch it, and nothing more.
#[derive(Debug)]
struct TiffScan {
    byte_order: ByteOrder,
    /// File offset of IFD0's entry-count field
    ifd0_offset: usize,
    /// File offset of the ExifIFD's entry-count field, when one exists
    exif_ifd_offset: Option<usize>,
    /// File offset of the GPS IFD's entry-count field, when one exists
    gps_ifd_offset: Option<usize>,
    /// Record offset of IFD0's ExifIFD-pointer entry, when one exists
    exif_pointer_record: Option<usize>,
    /// Record offset of IFD0's GPS-pointer entry, when one exists
    gps_pointer_record: Option<usize>,
    entries: Vec<LocatedEntry>,
}

/// Recognizes the TIFF headers this writer can walk.
///
/// Magic 42 is standard TIFF (and every TIFF-derived RAW that kept it);
/// magic 85 is Panasonic's RW2/RWL variant, which is byte-identical in
/// layout. BigTIFF (magic 43) uses 8-byte offsets and a 20-byte entry
/// record, so it is rejected rather than mis-walked.
pub fn is_walkable_tiff(bytes: &[u8]) -> bool {
    if bytes.len() < 8 {
        return false;
    }
    let bo = match &bytes[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return false,
    };
    matches!(read_u16(&bytes[2..4], bo), 42 | 85)
}

/// Walks IFD0, the ExifIFD and the GPS IFD, recording where each entry
/// record physically sits.
///
/// Deliberately does **not** descend into SubIFDs, IFD1 or MakerNotes: this
/// writer's contract is that anything it does not locate is left untouched,
/// and an edit aimed at such a tag is refused loudly rather than mis-applied
/// to a fresh IFD0 duplicate.
fn scan_tiff(bytes: &[u8]) -> Result<TiffScan> {
    if !is_walkable_tiff(bytes) {
        return Err(ExifToolError::unsupported_format(
            "Not a TIFF structure this writer can walk (BigTIFF and non-TIFF \
             containers are not supported)",
        ));
    }
    let byte_order = if &bytes[0..2] == b"II" {
        ByteOrder::LittleEndian
    } else {
        ByteOrder::BigEndian
    };
    let ifd0_offset = read_u32(&bytes[4..8], byte_order) as usize;

    let mut scan = TiffScan {
        byte_order,
        ifd0_offset,
        exif_ifd_offset: None,
        gps_ifd_offset: None,
        exif_pointer_record: None,
        gps_pointer_record: None,
        entries: Vec::new(),
    };

    walk(bytes, ifd0_offset, IfdKind::Ifd0, &mut scan);
    if let Some(off) = scan.exif_ifd_offset {
        walk(bytes, off, IfdKind::ExifIfd, &mut scan);
    }
    if let Some(off) = scan.gps_ifd_offset {
        walk(bytes, off, IfdKind::Gps, &mut scan);
    }
    Ok(scan)
}

/// Walks one IFD table, appending its entries to `scan`.
///
/// A truncated or out-of-bounds table stops the walk instead of erroring:
/// entries that were never located are simply never edited, which is the
/// conservative outcome for this writer.
fn walk(bytes: &[u8], offset: usize, which: IfdKind, scan: &mut TiffScan) {
    let Some(entries_start) = offset.checked_add(2).filter(|e| *e <= bytes.len()) else {
        return;
    };
    let count = read_u16(&bytes[offset..entries_start], scan.byte_order) as usize;

    for i in 0..count {
        let record_offset = entries_start + i * 12;
        let Some(record_end) = record_offset.checked_add(12).filter(|e| *e <= bytes.len()) else {
            return; // truncated table: keep what we located
        };
        let record = &bytes[record_offset..record_end];
        let tag_id = read_u16(&record[0..2], scan.byte_order);
        let field_type = read_u16(&record[2..4], scan.byte_order);
        let value_or_offset = read_u32(&record[8..12], scan.byte_order) as usize;

        // Structural pointers are followed, never treated as editable entries
        if which == IfdKind::Ifd0 && tag_id == EXIF_IFD_POINTER {
            scan.exif_ifd_offset = Some(value_or_offset);
            scan.exif_pointer_record = Some(record_offset);
            continue;
        }
        if which == IfdKind::Ifd0 && tag_id == GPS_IFD_POINTER {
            scan.gps_ifd_offset = Some(value_or_offset);
            scan.gps_pointer_record = Some(record_offset);
            continue;
        }

        scan.entries.push(LocatedEntry {
            ifd: which,
            tag_id,
            field_type,
            record_offset,
        });
    }
}

fn put_u16(out: &mut [u8], v: u16, bo: ByteOrder) {
    out.copy_from_slice(&match bo {
        ByteOrder::LittleEndian => v.to_le_bytes(),
        ByteOrder::BigEndian => v.to_be_bytes(),
    });
}

fn put_u32(out: &mut [u8], v: u32, bo: ByteOrder) {
    out.copy_from_slice(&match bo {
        ByteOrder::LittleEndian => v.to_le_bytes(),
        ByteOrder::BigEndian => v.to_be_bytes(),
    });
}

/// Appends `blob` at the end of `out` on an even offset and returns where it
/// landed. TIFF values are conventionally word-aligned.
fn append_aligned(out: &mut Vec<u8>, blob: &[u8]) -> usize {
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let at = out.len();
    out.extend_from_slice(blob);
    at
}

/// A brand-new entry to merge into an IFD table.
struct NewRecord {
    tag_id: u16,
    field_type: u16,
    count: u32,
    /// Already encoded in the file's byte order, ready for the record's
    /// inline field (<= 4 bytes) or already appended and reduced to its offset
    inline_or_offset: [u8; 4],
}

/// Every metadata-map key the reader could plausibly have produced for one
/// located entry: the native per-IFD key, plus its "EXIF:" alias (which is
/// both the CLI's documented `-EXIF:Tag=value` spelling and, for some
/// readers, the group the tag is actually surfaced under).
fn entry_keys(entry: &LocatedEntry) -> Vec<String> {
    let native = lookup_tag_name(entry.tag_id, entry.ifd.prefix());
    match native.split_once(':') {
        Some((_, suffix)) if !native.starts_with("EXIF:") => {
            let alias = format!("EXIF:{}", suffix);
            vec![native, alias]
        }
        _ => vec![native],
    }
}

/// Only EXIF-family keys participate in the diff; everything else in the map
/// (File:, Composite:, XMP:, ...) is not this writer's business.
fn is_exif_family(key: &str) -> bool {
    key.starts_with("IFD0:")
        || key.starts_with("ExifIFD:")
        || key.starts_with("GPS:")
        || key.starts_with("EXIF:")
}

/// Rewrites a TIFF-structured file, applying only what the caller changed.
///
/// `original` is the metadata map the reader produced from these very bytes;
/// `desired` is the caller's map. Their difference is the edit — see the
/// module docs for why that difference is also the validation boundary.
pub fn rewrite_tiff_file(
    file_bytes: &[u8],
    original: &MetadataMap,
    desired: &MetadataMap,
) -> Result<Vec<u8>> {
    let scan = scan_tiff(file_bytes)?;
    let bo = scan.byte_order;

    if !desired.iter().any(|(k, _)| is_exif_family(k)) {
        return Err(ExifToolError::unsupported_format(
            "Clearing all metadata from a TIFF-structured file is not supported: \
             this writer only edits in place and never rebuilds the file",
        ));
    }

    let mut out = file_bytes.to_vec();
    let mut consumed: Vec<String> = Vec::new();
    // Per-IFD additions, resolved after every in-place patch has been applied
    let mut added_ifd0: Vec<NewRecord> = Vec::new();
    let mut added_exif: Vec<NewRecord> = Vec::new();
    let mut added_gps: Vec<NewRecord> = Vec::new();

    // --- Pass 1: located entries (modify in place, or refuse a removal) ---
    for entry in &scan.entries {
        let keys = entry_keys(entry);
        // The key the reader actually used for this entry. The reader's group
        // assignment is not always the physical IFD -- Panasonic RW2 surfaces
        // IFD0's XResolution as "EXIF:XResolution" -- so both spellings are
        // candidates and the one the reader emitted wins.
        let Some(base_key) = keys.iter().find(|k| original.contains_key(k)) else {
            consumed.extend(keys); // reader never surfaced it: nothing to diff
            continue;
        };
        if !desired.contains_key(base_key) {
            return Err(ExifToolError::unsupported_format(format!(
                "Removing tag '{}' from a TIFF-structured file is not yet \
                 supported: this writer edits entries in place and cannot \
                 shrink an IFD table",
                base_key
            )));
        }

        // The edit is whichever spelling the caller staged a *different* value
        // under. A value equal to its original is carried over, so an alias the
        // reader itself emitted never overwrites the entry it aliases.
        let edit = keys
            .iter()
            .find_map(|k| match (desired.get(k), original.get(k)) {
                (Some(new), orig) if Some(new) != orig => Some((k.clone(), new.clone())),
                _ => None,
            });
        consumed.extend(keys);

        let Some((key, desired_value)) = edit else {
            continue; // untouched: its bytes are never rewritten
        };

        validate_changed(&key, &desired_value)?;
        let (ft, count, native) = tag_value_to_field(&desired_value, Some(entry.field_type))?;
        let bytes = native_to_byte_order(ft, &native, bo);
        write_record_value(&mut out, entry.record_offset, ft, count, &bytes, bo);
    }

    // --- Pass 2: added keys (no located entry) ---
    for (key, value) in desired.iter() {
        if !is_exif_family(key) || consumed.iter().any(|k| k == key) {
            continue;
        }
        // A key the reader surfaced that we could not locate lives in a
        // structure this writer deliberately does not walk (SubIFD chains,
        // IFD1, MakerNotes). Adding it to IFD0 would fabricate a duplicate
        // under a name the file already uses, so refuse the edit instead.
        if let Some(original_value) = original.get(key) {
            if value == original_value {
                continue; // untouched — carried by not touching its bytes
            }
            return Err(ExifToolError::unsupported_format(format!(
                "Editing tag '{}' is not yet supported for TIFF-structured \
                 files: it lives outside IFD0/ExifIFD/GPS (SubIFD, IFD1 or \
                 MakerNote), which this writer carries untouched",
                key
            )));
        }

        let Some(descriptor) = get_tag_descriptor(key) else {
            return Err(ExifToolError::parse_error(format!(
                "Cannot add tag '{}': not a known EXIF tag",
                key
            )));
        };
        let tag_id = descriptor_tag_id(descriptor).ok_or_else(|| {
            ExifToolError::parse_error(format!("Tag '{}' has no numeric EXIF id", key))
        })?;
        validate_changed(key, value)?;
        // As in the EXIF writer: a created tag has no existing entry to take
        // an IEEE 754 width from, so the declared type has to supply it.
        let (ft, count, native) = tag_value_to_field(value, declared_ieee_field_type(key))?;
        let bytes = native_to_byte_order(ft, &native, bo);

        let bucket = if key.starts_with("ExifIFD:") {
            &mut added_exif
        } else if key.starts_with("GPS:") {
            &mut added_gps
        } else {
            &mut added_ifd0
        };
        // Aliased keys ("IFD0:Make" and "EXIF:Make") resolve to one tag id but
        // are distinct map keys, so `consumed` cannot catch the collision.
        if bucket.iter().any(|r| r.tag_id == tag_id) {
            continue;
        }
        let mut inline_or_offset = [0u8; 4];
        if bytes.len() <= 4 {
            inline_or_offset[..bytes.len()].copy_from_slice(&bytes);
        } else {
            let at = append_aligned(&mut out, &bytes);
            put_u32(
                &mut inline_or_offset,
                u32::try_from(at).map_err(too_big)?,
                bo,
            );
        }
        bucket.push(NewRecord {
            tag_id,
            field_type: ft,
            count,
            inline_or_offset,
        });
    }

    if added_ifd0.is_empty() && added_exif.is_empty() && added_gps.is_empty() {
        return Ok(out);
    }

    // --- Pass 3: grow the tables that gained entries ---
    // Sub-IFDs first: each yields an offset that IFD0 must then point at.
    // Every record is copied from `out`, which already carries pass 1's
    // in-place patches, so a tag that was both patched and copied keeps its
    // new value.
    if !added_exif.is_empty() {
        let Some(exif_at) = scan.exif_ifd_offset else {
            return Err(ExifToolError::unsupported_format(
                "Adding an ExifIFD tag to a file with no ExifIFD is not yet \
                 supported for TIFF-structured files",
            ));
        };
        let new_at = grow_ifd(&mut out, exif_at, &added_exif, bo)?;
        repoint_subifd(
            &mut out,
            scan.exif_pointer_record,
            &mut added_ifd0,
            EXIF_IFD_POINTER,
            new_at,
            bo,
        )?;
    }
    if !added_gps.is_empty() {
        let Some(gps_at) = scan.gps_ifd_offset else {
            return Err(ExifToolError::unsupported_format(
                "Adding a GPS tag to a file with no GPS IFD is not yet \
                 supported for TIFF-structured files",
            ));
        };
        let new_at = grow_ifd(&mut out, gps_at, &added_gps, bo)?;
        repoint_subifd(
            &mut out,
            scan.gps_pointer_record,
            &mut added_ifd0,
            GPS_IFD_POINTER,
            new_at,
            bo,
        )?;
    }
    if !added_ifd0.is_empty() {
        let new_at = grow_ifd(&mut out, scan.ifd0_offset, &added_ifd0, bo)?;
        let new_at = u32::try_from(new_at).map_err(too_big)?;
        put_u32(&mut out[4..8], new_at, bo);
    }

    Ok(out)
}

fn too_big(_: std::num::TryFromIntError) -> ExifToolError {
    ExifToolError::unsupported_format(
        "File exceeds 4 GB: TIFF offsets are 32-bit and cannot address the \
         appended metadata",
    )
}

/// Points the pointer entry for a sub-IFD at `new_at`, either by patching
/// IFD0's existing pointer record in place or — when the file has no such
/// record — by queueing one as an IFD0 addition.
fn repoint_subifd(
    out: &mut [u8],
    pointer_record: Option<usize>,
    added_ifd0: &mut Vec<NewRecord>,
    pointer_tag: u16,
    new_at: usize,
    bo: ByteOrder,
) -> Result<()> {
    let new_at = u32::try_from(new_at).map_err(too_big)?;
    match pointer_record {
        Some(rec) => {
            put_u32(&mut out[rec + 8..rec + 12], new_at, bo);
        }
        None => {
            let mut inline_or_offset = [0u8; 4];
            put_u32(&mut inline_or_offset, new_at, bo);
            added_ifd0.push(NewRecord {
                tag_id: pointer_tag,
                field_type: LONG_TYPE,
                count: 1,
                inline_or_offset,
            });
        }
    }
    Ok(())
}

/// Writes an entry record's type/count/value triple at `record_offset`.
///
/// Values of 4 bytes or fewer go straight into the record's inline field;
/// anything longer is appended at end-of-file and the record holds its
/// offset. Either way the record itself does not move, so every other
/// pointer in the file stays valid.
fn write_record_value(
    out: &mut Vec<u8>,
    record_offset: usize,
    field_type: u16,
    count: u32,
    bytes: &[u8],
    bo: ByteOrder,
) {
    if bytes.len() > 4 {
        let at = append_aligned(out, bytes);
        // A >4 GB file cannot be addressed by a 32-bit TIFF offset; leaving
        // the original value in place is the only non-corrupting outcome.
        let Ok(at) = u32::try_from(at) else { return };
        put_u32(&mut out[record_offset + 8..record_offset + 12], at, bo);
    } else {
        let field = &mut out[record_offset + 8..record_offset + 12];
        field.fill(0);
        field[..bytes.len()].copy_from_slice(bytes);
    }
    // The tag id at record_offset..+2 stays put; only type/count/value change
    put_u16(
        &mut out[record_offset + 2..record_offset + 4],
        field_type,
        bo,
    );
    put_u32(&mut out[record_offset + 4..record_offset + 8], count, bo);
}

/// Copies the IFD table at `table_at` to end-of-file with `additions` merged
/// in tag-id order, and returns the new table's offset.
///
/// Existing records are copied byte-for-byte, so their value offsets still
/// resolve to the original (untouched) value bytes. The next-IFD pointer is
/// carried over, keeping the IFD1/thumbnail chain intact.
fn grow_ifd(
    out: &mut Vec<u8>,
    table_at: usize,
    additions: &[NewRecord],
    bo: ByteOrder,
) -> Result<usize> {
    if table_at + 2 > out.len() {
        return Err(ExifToolError::parse_error(
            "IFD table offset lies outside the file",
        ));
    }
    let old_count = read_u16(&out[table_at..table_at + 2], bo) as usize;
    let records_end = table_at + 2 + old_count * 12;
    if records_end + 4 > out.len() {
        return Err(ExifToolError::parse_error("Truncated IFD table"));
    }
    let old_records = out[table_at + 2..records_end].to_vec();
    let next_ifd = out[records_end..records_end + 4].to_vec();

    let new_count = old_count + additions.len();
    let new_count = u16::try_from(new_count)
        .map_err(|_| ExifToolError::parse_error("IFD would exceed the 65535-entry TIFF limit"))?;

    let mut table = Vec::with_capacity(2 + new_count as usize * 12 + 4);
    table.extend_from_slice(&[0, 0]);
    put_u16(&mut table[0..2], new_count, bo);

    // Merge by ascending tag id, as the TIFF spec requires.
    let mut adds: Vec<&NewRecord> = additions.iter().collect();
    adds.sort_by_key(|r| r.tag_id);
    let mut ai = 0;
    for chunk in old_records.chunks_exact(12) {
        let tag_id = read_u16(&chunk[0..2], bo);
        while ai < adds.len() && adds[ai].tag_id < tag_id {
            push_record(&mut table, adds[ai], bo);
            ai += 1;
        }
        table.extend_from_slice(chunk);
    }
    while ai < adds.len() {
        push_record(&mut table, adds[ai], bo);
        ai += 1;
    }
    table.extend_from_slice(&next_ifd);

    Ok(append_aligned(out, &table))
}

fn push_record(table: &mut Vec<u8>, rec: &NewRecord, bo: ByteOrder) {
    let base = table.len();
    table.extend_from_slice(&[0u8; 12]);
    put_u16(&mut table[base..base + 2], rec.tag_id, bo);
    put_u16(&mut table[base + 2..base + 4], rec.field_type, bo);
    put_u32(&mut table[base + 4..base + 8], rec.count, bo);
    table[base + 8..base + 12].copy_from_slice(&rec.inline_or_offset);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;

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

    /// Record offsets in the fixture below (IFD0 records start at 10, in
    /// ascending tag-id order as the TIFF spec requires).
    const REC_MAKE: usize = 10;
    const REC_STRIP_OFFSETS: usize = 22;
    const REC_ORIENTATION: usize = 34;

    /// A minimal but realistic TIFF file:
    ///   0   header, IFD0 at 8
    ///   8   IFD0: 3 entries (Make ASCII@62, StripOffsets LONG = 80,
    ///       Orientation SHORT inline), next-IFD = 0
    ///  62   "Canon\0"
    ///  80   fake image strip, 8 bytes
    fn build_tiff(bo: ByteOrder) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(match bo {
            ByteOrder::LittleEndian => b"II",
            ByteOrder::BigEndian => b"MM",
        });
        t.extend_from_slice(&u16b(42, bo));
        t.extend_from_slice(&u32b(8, bo));
        // IFD0 at 8: count at 8..10, records at 10..46, next at 46..50
        t.extend_from_slice(&u16b(3, bo));
        // Make (0x010F) ASCII count 6 @ 62
        t.extend_from_slice(&u16b(0x010F, bo));
        t.extend_from_slice(&u16b(2, bo));
        t.extend_from_slice(&u32b(6, bo));
        t.extend_from_slice(&u32b(62, bo));
        // StripOffsets (0x0111) LONG count 1 = 80
        t.extend_from_slice(&u16b(0x0111, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(80, bo));
        // Orientation (0x0112) SHORT count 1 inline = 1
        t.extend_from_slice(&u16b(0x0112, bo));
        t.extend_from_slice(&u16b(3, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(1, bo));
        // next IFD = 0
        t.extend_from_slice(&u32b(0, bo));
        // pad to 62
        while t.len() < 62 {
            t.push(0);
        }
        t.extend_from_slice(b"Canon\0");
        while t.len() < 80 {
            t.push(0);
        }
        t.extend_from_slice(&[0xAA; 8]); // "image data"
        t
    }

    fn original_map() -> MetadataMap {
        let mut m = MetadataMap::new();
        m.insert("IFD0:Make", TagValue::new_string("Canon"));
        m.insert("IFD0:Orientation", TagValue::new_string("Horizontal"));
        m.insert("IFD0:StripOffsets", TagValue::Integer(80));
        m
    }

    #[test]
    fn rejects_bigtiff_and_non_tiff() {
        assert!(!is_walkable_tiff(b"II\x2b\x00\x08\x00\x00\x00")); // BigTIFF
        assert!(!is_walkable_tiff(b"\xff\xd8\xff\xe1\x00\x00\x00\x00")); // JPEG
        assert!(is_walkable_tiff(b"II\x2a\x00\x08\x00\x00\x00"));
        assert!(is_walkable_tiff(b"MM\x00\x2a\x00\x00\x00\x08"));
        assert!(is_walkable_tiff(b"II\x55\x00\x18\x00\x00\x00")); // RW2
    }

    /// The load-bearing property: an unchanged value's bytes are never
    /// rewritten, so image data and every untouched entry survive exactly.
    #[test]
    fn unchanged_map_leaves_the_file_byte_identical() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let file = build_tiff(bo);
            let map = original_map();
            let out = rewrite_tiff_file(&file, &map, &map).unwrap();
            assert_eq!(out, file, "unchanged write must not alter a single byte");
        }
    }

    /// A carried-over value that would fail strict validation must still be
    /// accepted — this is issue #20's actual failure mode.
    #[test]
    fn carried_over_value_of_wrong_declared_type_is_not_validated() {
        let file = build_tiff(ByteOrder::LittleEndian);
        let mut map = original_map();
        // BitsPerSample is declared Integer but the reader emits "8 8 8"
        map.insert("IFD0:BitsPerSample", TagValue::new_string("8 8 8"));
        let out = rewrite_tiff_file(&file, &map, &map).expect("carried-over value must not block");
        assert_eq!(out, file);
    }

    #[test]
    fn changed_inline_value_is_patched_without_moving_anything() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let file = build_tiff(bo);
            let original = original_map();
            let mut desired = original.clone();
            desired.insert("IFD0:Orientation", TagValue::Integer(6));
            let out = rewrite_tiff_file(&file, &original, &desired).unwrap();
            assert_eq!(out.len(), file.len(), "inline patch must not grow the file");
            assert_eq!(
                read_u16(&out[REC_ORIENTATION + 8..REC_ORIENTATION + 10], bo),
                6
            );
            // Every other record, and the image data, is untouched
            assert_eq!(&out[..REC_ORIENTATION], &file[..REC_ORIENTATION]);
            assert_eq!(&out[REC_ORIENTATION + 12..], &file[REC_ORIENTATION + 12..]);
            assert_eq!(&out[80..88], &[0xAA; 8]);
        }
    }

    #[test]
    fn changed_oversized_value_is_appended_and_repointed() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let file = build_tiff(bo);
            let original = original_map();
            let mut desired = original.clone();
            desired.insert("IFD0:Make", TagValue::new_string("A Much Longer Maker"));
            let out = rewrite_tiff_file(&file, &original, &desired).unwrap();
            assert!(out.len() > file.len(), "oversized value must be appended");
            // Only Make's own record changed; every other byte of the original
            // file — including the now-dead old "Canon\0" — is byte-identical.
            assert_eq!(&out[..REC_MAKE], &file[..REC_MAKE]);
            assert_eq!(
                &out[REC_MAKE + 12..file.len()],
                &file[REC_MAKE + 12..],
                "nothing else in the original moved"
            );
            let at = read_u32(&out[REC_MAKE + 8..REC_MAKE + 12], bo) as usize;
            let n = read_u32(&out[REC_MAKE + 4..REC_MAKE + 8], bo) as usize;
            assert!(at >= file.len(), "new value lives past the original EOF");
            assert_eq!(&out[at..at + n], b"A Much Longer Maker\0");
            assert_eq!(&out[80..88], &[0xAA; 8], "image data survives");
        }
    }

    #[test]
    fn added_tag_grows_a_copied_ifd_and_repoints_the_header() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let file = build_tiff(bo);
            let original = original_map();
            let mut desired = original.clone();
            desired.insert("EXIF:Artist", TagValue::new_string("Ansel"));
            let out = rewrite_tiff_file(&file, &original, &desired).unwrap();

            // Header now points at a new IFD0 past the original file
            let new_ifd0 = read_u32(&out[4..8], bo) as usize;
            assert!(new_ifd0 >= file.len(), "new IFD0 must be appended");
            assert_eq!(read_u16(&out[new_ifd0..new_ifd0 + 2], bo), 4);

            // Original bytes below the append point are untouched
            assert_eq!(&out[8..file.len()], &file[8..]);

            // Records are in ascending tag-id order and Artist resolves
            let mut ids = Vec::new();
            let mut artist = None;
            for i in 0..4 {
                let rec = new_ifd0 + 2 + i * 12;
                let tag = read_u16(&out[rec..rec + 2], bo);
                ids.push(tag);
                if tag == 0x013B {
                    let at = read_u32(&out[rec + 8..rec + 12], bo) as usize;
                    let n = read_u32(&out[rec + 4..rec + 8], bo) as usize;
                    artist = Some(out[at..at + n].to_vec());
                }
            }
            assert!(
                ids.windows(2).all(|w| w[0] <= w[1]),
                "ids sorted: {:?}",
                ids
            );
            assert_eq!(artist.as_deref(), Some(&b"Ansel\0"[..]));
        }
    }

    #[test]
    fn removing_a_located_tag_is_refused_not_silently_ignored() {
        let file = build_tiff(ByteOrder::LittleEndian);
        let original = original_map();
        let mut desired = original.clone();
        desired.remove("IFD0:Make");
        let err = rewrite_tiff_file(&file, &original, &desired).unwrap_err();
        assert!(
            err.to_string().contains("Removing tag 'IFD0:Make'"),
            "got: {}",
            err
        );
    }

    #[test]
    fn editing_an_unwalked_tag_is_refused_not_misapplied() {
        let file = build_tiff(ByteOrder::LittleEndian);
        let mut original = original_map();
        // A tag the reader surfaced from a SubIFD this writer never walks
        original.insert("IFD0:ImageWidth", TagValue::Integer(160));
        let mut desired = original.clone();
        desired.insert("IFD0:ImageWidth", TagValue::Integer(999));
        let err = rewrite_tiff_file(&file, &original, &desired).unwrap_err();
        assert!(
            err.to_string().contains("is not yet supported"),
            "got: {}",
            err
        );
    }

    #[test]
    fn changed_value_is_still_strictly_validated() {
        let file = build_tiff(ByteOrder::LittleEndian);
        let original = original_map();
        let mut desired = original.clone();
        desired.insert(
            "EXIF:XResolution",
            TagValue::Rational {
                numerator: 72,
                denominator: 0,
            },
        );
        let err = rewrite_tiff_file(&file, &original, &desired).unwrap_err();
        assert!(err.to_string().contains("denominator"), "got: {}", err);
    }
}
