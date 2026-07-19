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

use crate::core::FileReader;
use crate::core::metadata_map::MetadataMap;
use crate::core::operations_helpers::{read_u16, read_u32};
use crate::core::tag_value::TagValue;
use crate::core::validation::{validate_tag_value_intrinsics, validate_tag_value_with_name};
use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::parse_segments;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::tag_db::lookup_tag_name;
use crate::tag_db::tag_registry::{get_tag_descriptor, has_reliable_value_type};

/// EXIF identifier at the start of an EXIF APP1 segment
const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";

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

/// Reconstructs every metadata-map key the reader could plausibly have
/// produced for one raw-carried entry in an always-carried IFD class
/// (InteropIFD, IFD1, MakerNote — see the Design Rule table). These classes
/// are carried byte-for-byte in the per-entry loop without ever being
/// diffed against `desired`, but the reader still independently surfaces
/// some of them under a metadata-map key:
///   - MakerNote/IFD1 tags (and any tag with no registry name) use the
///     generic `lookup_tag_name(tag_id, ifd_prefix)` scheme — the same
///     function this writer already calls for surfaced classes, so it
///     reproduces the reader's key exactly (including its "IFD:0xNNNN"
///     hex fallback for unregistered tags, e.g. "ExifIFD:0x927C" for a
///     MakerNote blob).
///   - InteropIFD tags are additionally special-cased by
///     `parse_interop_subifd` (`src/core/tiff_helpers.rs`) under a
///     hard-coded "EXIF:" prefix with its own name table, distinct from
///     `lookup_tag_name(tag_id, "InteropIFD")`.
/// Returns both candidate keys so the Added-tag loop can recognize a
/// collision precisely instead of treating every key already present in
/// `original_map` as carried.
fn carried_class_reader_keys(entry: &RawEntry) -> Vec<String> {
    let mut keys = vec![lookup_tag_name(entry.tag_id, entry.ifd.prefix())];
    if entry.ifd == IfdKind::Interop {
        let name = crate::core::tiff_helpers::interop_tag_to_name(entry.tag_id);
        if name != "Unknown" {
            keys.push(format!("EXIF:{}", name));
        }
    }
    keys
}

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

/// One entry ready for serialization (raw carry-over or freshly typed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutEntry {
    pub tag_id: u16,
    pub field_type: u16,
    pub count: u32,
    pub value: Vec<u8>,
    /// True when `value` is a native-endian placeholder produced by
    /// `tag_value_to_field` (needs re-encoding into the plan's byte order
    /// during serialization). False for raw carry-over bytes, which are
    /// already in the original file's byte order and must not be touched.
    native_endian: bool,
}

/// A fully diffed EXIF write: per-IFD entries plus preserved blobs.
#[derive(Debug, Clone, PartialEq)]
pub struct WritePlan {
    pub byte_order: ByteOrder,
    pub ifd0: Vec<OutEntry>,
    pub exif_ifd: Vec<OutEntry>,
    pub gps: Vec<OutEntry>,
    pub interop: Vec<OutEntry>,
    pub ifd1: Vec<OutEntry>,
    pub thumbnail: Option<Vec<u8>>,
    /// Original MakerNote value offset to honor during layout
    pub makernote_pin: Option<usize>,
}

/// Serializes a caller-supplied TagValue into (field_type, count, bytes).
/// `hint` is the original entry's field type, used to keep BYTE vs UNDEFINED
/// and SHORT vs LONG stable across an edit.
fn tag_value_to_field(value: &TagValue, hint: Option<u16>) -> Result<(u16, u32, Vec<u8>)> {
    match value {
        TagValue::String(s) => {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            Ok((2, bytes.len() as u32, bytes))
        }
        TagValue::Integer(i) => {
            let i = *i;
            match hint {
                Some(3) if (0..=0xFFFF).contains(&i) => {
                    Ok((3, 1, (i as u16).to_ne_bytes().to_vec()))
                }
                Some(4) if (0..=0xFFFF_FFFF).contains(&i) => {
                    Ok((4, 1, (i as u32).to_ne_bytes().to_vec()))
                }
                Some(9) if (i32::MIN as i64..=i32::MAX as i64).contains(&i) => {
                    Ok((9, 1, (i as i32).to_ne_bytes().to_vec()))
                }
                // No hint, or value doesn't fit the hinted type: pick the
                // smallest TIFF integer type that fits
                _ if (0..=0xFFFF).contains(&i) => Ok((3, 1, (i as u16).to_ne_bytes().to_vec())),
                _ if (0..=0xFFFF_FFFF).contains(&i) => {
                    Ok((4, 1, (i as u32).to_ne_bytes().to_vec()))
                }
                _ if (i32::MIN as i64..=i32::MAX as i64).contains(&i) => {
                    Ok((9, 1, (i as i32).to_ne_bytes().to_vec()))
                }
                _ => Err(ExifToolError::parse_error(format!(
                    "Integer value {} does not fit any TIFF integer type",
                    i
                ))),
            }
        }
        TagValue::Rational {
            numerator,
            denominator,
        } => {
            if *numerator >= 0 && *denominator >= 0 {
                let mut b = (*numerator as u32).to_ne_bytes().to_vec();
                b.extend_from_slice(&(*denominator as u32).to_ne_bytes());
                Ok((5, 1, b))
            } else {
                let mut b = numerator.to_ne_bytes().to_vec();
                b.extend_from_slice(&denominator.to_ne_bytes());
                Ok((10, 1, b))
            }
        }
        TagValue::Binary(bytes) => {
            let ft = match hint {
                Some(1) => 1, // keep BYTE if it was BYTE
                _ => 7,       // UNDEFINED
            };
            Ok((ft, bytes.len() as u32, bytes.clone()))
        }
        TagValue::DateTime(dt) => {
            let mut bytes = crate::core::date_shift::format_exif_datetime(dt).into_bytes();
            bytes.push(0);
            Ok((2, bytes.len() as u32, bytes)) // always 20
        }
        TagValue::Float(f) => Ok((12, 1, f.to_ne_bytes().to_vec())),
        TagValue::Array(_) | TagValue::Struct(_) => Err(ExifToolError::parse_error(
            "Array/Struct values are not supported for EXIF write",
        )),
    }
}

/// NOTE on multi-byte native-endian buffers: tag_value_to_field intentionally
/// emits native-endian placeholder bytes for Integer/Rational/Float; the
/// serializer (Task 4) re-emits multi-byte numeric values in the plan's byte
/// order using field_type/count, so these placeholders never reach the file
/// for numeric types. ASCII/BYTE/UNDEFINED bytes are endian-neutral.
///
/// Diffs the original scan + reader-produced map against the desired map.
/// See the Design Rules table in the plan document for the exact contract.
pub fn plan_exif_write(
    scan: &ExifScan,
    original_map: &MetadataMap,
    desired: &MetadataMap,
) -> Result<WritePlan> {
    let exif_family_keys = |m: &MetadataMap| -> Vec<String> {
        m.iter()
            .map(|(k, _)| k.clone())
            .filter(|k| {
                k.starts_with("IFD0:")
                    || k.starts_with("ExifIFD:")
                    || k.starts_with("GPS:")
                    || k.starts_with("EXIF:")
            })
            .collect()
    };

    let mut plan = WritePlan {
        byte_order: scan.byte_order,
        ifd0: Vec::new(),
        exif_ifd: Vec::new(),
        gps: Vec::new(),
        interop: Vec::new(),
        ifd1: Vec::new(),
        thumbnail: None,
        makernote_pin: None,
    };

    // clear_all_metadata semantics: no EXIF-family keys desired -> drop all
    if exif_family_keys(desired).is_empty() {
        return Ok(plan);
    }

    // Normalize "EXIF:"-prefixed aliases onto their native per-entry key so
    // the per-entry loop below (which looks up `desired` by the reader's
    // literal key, e.g. "IFD0:Make") sees edits made via the alias spelling
    // (e.g. "EXIF:Make", the CLI's own documented -EXIF:Tag=value syntax)
    // instead of silently missing them. Only folds when the native key is
    // itself untouched in `desired` -- if the caller already staged an
    // explicit (different) value under the native key, that explicit value
    // wins and the alias is left for the pre-existing duplicate-tag-id guard
    // in the Added loop below to reconcile (skip if equal, once serialized).
    let mut desired = desired.clone();
    for entry in &scan.entries {
        if matches!(entry.ifd, IfdKind::Interop | IfdKind::Ifd1) || entry.tag_id == MAKERNOTE {
            continue;
        }
        let native_key = lookup_tag_name(entry.tag_id, entry.ifd.prefix());
        let Some((_, suffix)) = native_key.split_once(':') else {
            continue;
        };
        let alias_key = format!("EXIF:{}", suffix);
        if alias_key == native_key {
            continue;
        }
        if let Some(alias_value) = desired.get(&alias_key).cloned() {
            let native_untouched = desired.get(&native_key) == original_map.get(&native_key);
            if native_untouched {
                desired.insert(native_key, alias_value);
            }
        }
    }
    let desired = &desired;

    plan.thumbnail = scan.thumbnail.clone();
    plan.makernote_pin = scan.makernote_offset;

    let mut consumed_keys: Vec<String> = Vec::new();
    // Reader keys that map back to an always-carried entry (Interop/IFD1/
    // MakerNote); see `carried_class_reader_keys`.
    let mut carried_reader_keys: Vec<String> = Vec::new();

    for entry in &scan.entries {
        let bucket = |plan: &mut WritePlan, e: OutEntry| match entry.ifd {
            IfdKind::Ifd0 => plan.ifd0.push(e),
            IfdKind::ExifIfd => plan.exif_ifd.push(e),
            IfdKind::Gps => plan.gps.push(e),
            IfdKind::Interop => plan.interop.push(e),
            IfdKind::Ifd1 => plan.ifd1.push(e),
        };
        let carry = OutEntry {
            tag_id: entry.tag_id,
            field_type: entry.field_type,
            count: entry.count,
            value: entry.value.clone(),
            native_endian: false,
        };

        // Unsurfaced classes: always carry, UNLESS the caller genuinely
        // removed the tag (its reader-equivalent key was present in
        // original_map but is now absent from desired). Carrying it
        // unconditionally in that case would make remove_tag silently no-op
        // while still reporting success -- error loudly instead. Keys that
        // were never surfaced by the reader in the first place (absent from
        // original_map too) are unaffected and stay silently carried, which
        // is correct: the caller never had a chance to remove what it never
        // saw.
        if matches!(entry.ifd, IfdKind::Interop | IfdKind::Ifd1) || entry.tag_id == MAKERNOTE {
            let reader_keys = carried_class_reader_keys(entry);
            for reader_key in &reader_keys {
                if original_map.contains_key(reader_key) && !desired.contains_key(reader_key) {
                    return Err(ExifToolError::unsupported_format(format!(
                        "Removing tag '{}' is not yet supported: it belongs to an \
                         unsurfaced IFD class (InteropIFD/IFD1/MakerNote) that this \
                         writer always raw-carries",
                        reader_key
                    )));
                }
            }
            carried_reader_keys.extend(reader_keys);
            bucket(&mut plan, carry);
            continue;
        }

        let key = lookup_tag_name(entry.tag_id, entry.ifd.prefix());
        let Some(original_value) = original_map.get(&key) else {
            // Reader didn't surface this entry: never drop what it hides
            bucket(&mut plan, carry);
            continue;
        };
        let Some(desired_value) = desired.get(&key) else {
            continue; // removal by absence
        };
        consumed_keys.push(key.clone());
        if desired_value == original_value {
            bucket(&mut plan, carry);
            continue;
        }

        // Changed: strict validation, then true-typed serialization
        validate_changed(&key, desired_value)?;
        let (ft, count, bytes) = tag_value_to_field(desired_value, Some(entry.field_type))?;
        bucket(
            &mut plan,
            OutEntry {
                tag_id: entry.tag_id,
                field_type: ft,
                count,
                value: bytes,
                native_endian: true,
            },
        );
    }

    // Added: desired EXIF-family keys not matched to any original entry
    for key in exif_family_keys(desired) {
        if consumed_keys.iter().any(|k| *k == key) {
            continue;
        }
        // Keys whose physical entry lives in an always-carried IFD class
        // (InteropIFD, IFD1, MakerNote — Design Rule: "unsurfaced classes")
        // are carried byte-for-byte above without ever being diffed against
        // `desired`. The reader can still surface some of them under a
        // metadata-map key (e.g. "EXIF:InteropIndex", or a MakerNote blob's
        // "ExifIFD:0x927C" hex fallback). If the caller left such a key
        // unchanged, it's already handled by the carry-over. If the caller
        // genuinely changed it, editing that tag isn't supported by the
        // raw-preservation writer yet — error loudly rather than silently
        // discarding the edit.
        if carried_reader_keys.iter().any(|k| *k == key)
            && let Some(original_value) = original_map.get(&key)
        {
            let value = desired.get(&key).unwrap();
            if value == original_value {
                continue;
            }
            return Err(ExifToolError::unsupported_format(format!(
                "Editing tag '{}' is not yet supported: it belongs to an \
                 unsurfaced IFD class (InteropIFD/IFD1/MakerNote) that this \
                 writer always raw-carries",
                key
            )));
        }
        let value = desired.get(&key).unwrap();
        let Some(descriptor) = get_tag_descriptor(&key) else {
            return Err(ExifToolError::parse_error(format!(
                "Cannot add tag '{}': not a known EXIF tag",
                key
            )));
        };
        validate_changed(&key, value)?;
        let tag_id = descriptor_tag_id(descriptor).ok_or_else(|| {
            ExifToolError::parse_error(format!("Tag '{}' has no numeric EXIF id", key))
        })?;
        let (ft, count, bytes) = tag_value_to_field(value, None)?;
        let out = OutEntry {
            tag_id,
            field_type: ft,
            count,
            value: bytes,
            native_endian: true,
        };
        // Route by prefix; "EXIF:" keys land in IFD0 (compat with the old writer).
        // Guard against duplicate tag ids: aliased keys (e.g. "IFD0:Make" and
        // "EXIF:Make") resolve to the same numeric tag id via get_tag_descriptor's
        // prefix normalization but are distinct MetadataMap keys, so consumed_keys
        // (tracked by literal key string) cannot catch the collision.
        if key.starts_with("ExifIFD:") {
            if plan.exif_ifd.iter().any(|e| e.tag_id == tag_id) {
                continue;
            }
            plan.exif_ifd.push(out);
        } else if key.starts_with("GPS:") {
            if plan.gps.iter().any(|e| e.tag_id == tag_id) {
                continue;
            }
            plan.gps.push(out);
        } else {
            if plan.ifd0.iter().any(|e| e.tag_id == tag_id) {
                continue;
            }
            plan.ifd0.push(out);
        }
    }

    Ok(plan)
}

/// Strict validation for values the caller changed or added — identical
/// policy to write_metadata's PHASE 1 (reliable type match, else intrinsics).
fn validate_changed(key: &str, value: &TagValue) -> Result<()> {
    if let Some(descriptor) = get_tag_descriptor(key) {
        if has_reliable_value_type(key) {
            validate_tag_value_with_name(key, descriptor, value)?;
        } else {
            validate_tag_value_intrinsics(key, value)?;
        }
    }
    Ok(())
}

/// Extracts the numeric tag id from a descriptor, mirroring
/// `validate_tag_for_tiff` (`src/writers/tiff_writer/tiff/validator.rs:48-69`).
fn descriptor_tag_id(descriptor: &crate::core::TagDescriptor) -> Option<u16> {
    match &descriptor.tag_id {
        crate::core::TagId::Numeric(id) => Some(*id),
        crate::core::TagId::Named(_) => None,
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
            && t_off
                .checked_add(t_len)
                .is_some_and(|end| end <= tiff.len())
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
    if which == IfdKind::Ifd0 && next_at + 4 <= tiff.len() {
        let next = read_u32(&tiff[next_at..next_at + 4], byte_order) as usize;
        if next != 0 {
            result.next_ifd = Some(next);
        }
    }
    result
}

/// Emits v in the plan's byte order.
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

/// Re-encodes an OutEntry's value into the target byte order when the field
/// type is multi-byte numeric AND the value came from tag_value_to_field's
/// native-endian placeholder. Carried raw values are already in the file's
/// byte order (the plan preserves it), so this only converts per-element for
/// freshly serialized numeric values; it is a no-op for 1-byte element types.
fn value_in_byte_order(entry: &OutEntry, bo: ByteOrder) -> Vec<u8> {
    // Carried raw values are already encoded in the target byte order (the
    // plan never changes byte order); only freshly typed placeholders from
    // tag_value_to_field are native-endian and need re-encoding here.
    if !entry.native_endian {
        return entry.value.clone();
    }
    let elem = type_size(entry.field_type);
    if elem == 1 {
        return entry.value.clone();
    }
    // Elements inside RATIONAL/SRATIONAL are two 4-byte halves
    let unit = match entry.field_type {
        5 | 10 => 4,
        _ => elem,
    };
    let mut out = Vec::with_capacity(entry.value.len());
    for chunk in entry.value.chunks(unit) {
        let mut c = chunk.to_vec();
        let native_le = cfg!(target_endian = "little");
        let want_le = bo == ByteOrder::LittleEndian;
        if native_le != want_le {
            c.reverse();
        }
        out.extend_from_slice(&c);
    }
    out
}

/// Offset allocator that flows around one reserved window.
struct Allocator {
    cursor: usize,
    reserved: Option<(usize, usize)>, // (start, len)
}

impl Allocator {
    fn alloc(&mut self, len: usize) -> usize {
        // TIFF values should start on even offsets
        if self.cursor % 2 == 1 {
            self.cursor += 1;
        }
        if let Some((rs, rl)) = self.reserved
            && self.cursor < rs + rl
            && self.cursor + len > rs
        {
            self.cursor = rs + rl;
            if self.cursor % 2 == 1 {
                self.cursor += 1;
            }
        }
        let at = self.cursor;
        self.cursor += len;
        at
    }
}

/// Emits one IFD table: entries (sorted, with synthesized pointers merged in
/// tag-id order), then next-IFD pointer, then oversized values (which are
/// written directly at their pre-allocated offsets).
#[allow(clippy::too_many_arguments)]
fn emit_ifd(
    out: &mut [u8],
    bo: ByteOrder,
    table_at: usize,
    entries: &[OutEntry],
    offsets: &[usize],
    pointers: &[(u16, u32)],
    next_ifd: u32,
) {
    let mut rows: Vec<(u16, u16, u32, [u8; 4])> = Vec::new(); // tag, type, count, valfield
    for (e, off) in entries.iter().zip(offsets) {
        let mut val = [0u8; 4];
        if e.value.len() > 4 {
            put_u32(&mut val, *off as u32, bo);
            let bytes = value_in_byte_order(e, bo);
            out[*off..*off + bytes.len()].copy_from_slice(&bytes);
        } else {
            let bytes = value_in_byte_order(e, bo);
            val[..bytes.len()].copy_from_slice(&bytes);
        }
        rows.push((e.tag_id, e.field_type, e.count, val));
    }
    for (tag, target) in pointers {
        let mut val = [0u8; 4];
        put_u32(&mut val, *target, bo);
        rows.push((*tag, 4, 1, val)); // LONG count 1
    }
    rows.sort_by_key(|r| r.0);
    put_u16(&mut out[table_at..table_at + 2], rows.len() as u16, bo);
    for (i, (tag, ft, count, val)) in rows.iter().enumerate() {
        let at = table_at + 2 + i * 12;
        put_u16(&mut out[at..at + 2], *tag, bo);
        put_u16(&mut out[at + 2..at + 4], *ft, bo);
        put_u32(&mut out[at + 4..at + 8], *count, bo);
        out[at + 8..at + 12].copy_from_slice(val);
    }
    let next_at = table_at + 2 + rows.len() * 12;
    put_u32(&mut out[next_at..next_at + 4], next_ifd, bo);
}

/// Serializes a WritePlan into complete TIFF bytes. An empty plan yields an
/// empty Vec (the caller omits the EXIF segment entirely).
pub fn serialize_exif(plan: &WritePlan) -> Result<Vec<u8>> {
    let has_entries = !(plan.ifd0.is_empty()
        && plan.exif_ifd.is_empty()
        && plan.gps.is_empty()
        && plan.interop.is_empty()
        && plan.ifd1.is_empty());
    if !has_entries {
        return Ok(Vec::new());
    }
    let bo = plan.byte_order;

    // Sorted copies (TIFF requires ascending tag ids per IFD)
    let mut ifd0 = plan.ifd0.clone();
    let mut exif_ifd = plan.exif_ifd.clone();
    let mut gps = plan.gps.clone();
    let mut interop = plan.interop.clone();
    let mut ifd1 = plan.ifd1.clone();
    for list in [&mut ifd0, &mut exif_ifd, &mut gps, &mut interop, &mut ifd1] {
        list.sort_by_key(|e| e.tag_id);
        list.dedup_by_key(|e| e.tag_id); // defensive: one entry per tag id
    }

    // Pointer entries the tables will contain (synthesized during emit)
    let ifd0_pointers = usize::from(!exif_ifd.is_empty()) + usize::from(!gps.is_empty());
    let exif_pointers = usize::from(!interop.is_empty());
    let ifd1_pointers = if plan.thumbnail.is_some() { 2 } else { 0 };

    let table_size = |n: usize| 2 + n * 12 + 4;

    // Pass 1: allocate tables, then oversized values, honoring the pin
    let mut alloc = Allocator {
        cursor: 8,
        reserved: None,
    };
    let makernote_len = exif_ifd
        .iter()
        .find(|e| e.tag_id == MAKERNOTE)
        .map(|e| e.value.len())
        .filter(|len| *len > 4);
    let mut pinned = None;
    if let (Some(pin), Some(len)) = (plan.makernote_pin, makernote_len) {
        if pin >= 8 {
            alloc.reserved = Some((pin, len));
            pinned = Some(pin);
        } else {
            eprintln!(
                "Warning: MakerNote original offset {} cannot be honored; \
                 manufacturer-internal offsets may be invalidated",
                pin
            );
        }
    }

    let ifd0_at = alloc.alloc(table_size(ifd0.len() + ifd0_pointers));
    let exif_at = if exif_ifd.is_empty() {
        0
    } else {
        alloc.alloc(table_size(exif_ifd.len() + exif_pointers))
    };
    let interop_at = if interop.is_empty() {
        0
    } else {
        alloc.alloc(table_size(interop.len()))
    };
    let gps_at = if gps.is_empty() {
        0
    } else {
        alloc.alloc(table_size(gps.len()))
    };
    let ifd1_at = if ifd1.is_empty() && plan.thumbnail.is_none() {
        0
    } else {
        alloc.alloc(table_size(ifd1.len() + ifd1_pointers))
    };

    // Value offsets for every oversized value, deterministic order
    let mut value_offsets: Vec<Vec<usize>> = Vec::new();
    for list in [&ifd0, &exif_ifd, &interop, &gps, &ifd1] {
        let mut offsets = Vec::with_capacity(list.len());
        for e in list.iter() {
            if e.value.len() > 4 {
                if e.tag_id == MAKERNOTE && pinned.is_some() {
                    offsets.push(pinned.unwrap());
                } else {
                    offsets.push(alloc.alloc(e.value.len()));
                }
            } else {
                offsets.push(0); // inline
            }
        }
        value_offsets.push(offsets);
    }
    let thumb_at = plan.thumbnail.as_ref().map(|t| alloc.alloc(t.len()));

    let total = alloc
        .cursor
        .max(pinned.map_or(0, |p| p + makernote_len.unwrap_or(0)));
    let mut out = vec![0u8; total];

    // Header
    out[0..2].copy_from_slice(match bo {
        ByteOrder::LittleEndian => b"II",
        ByteOrder::BigEndian => b"MM",
    });
    put_u16(&mut out[2..4], 42, bo);
    put_u32(&mut out[4..8], ifd0_at as u32, bo);

    // ExifIFD (with Interop pointer), Interop, GPS, IFD1, then IFD0 last so
    // its pointer values are all known
    if exif_at != 0 {
        let mut ptrs = Vec::new();
        if interop_at != 0 {
            ptrs.push((INTEROP_POINTER, interop_at as u32));
        }
        emit_ifd(
            &mut out,
            bo,
            exif_at,
            &exif_ifd,
            &value_offsets[1],
            &ptrs,
            0,
        );
    }
    if interop_at != 0 {
        emit_ifd(
            &mut out,
            bo,
            interop_at,
            &interop,
            &value_offsets[2],
            &[],
            0,
        );
    }
    if gps_at != 0 {
        emit_ifd(&mut out, bo, gps_at, &gps, &value_offsets[3], &[], 0);
    }
    if ifd1_at != 0 {
        let mut ptrs = Vec::new();
        if let Some(t_at) = thumb_at {
            ptrs.push((THUMBNAIL_OFFSET, t_at as u32));
            ptrs.push((
                THUMBNAIL_LENGTH,
                plan.thumbnail.as_ref().unwrap().len() as u32,
            ));
        }
        emit_ifd(&mut out, bo, ifd1_at, &ifd1, &value_offsets[4], &ptrs, 0);
    }
    {
        let mut ptrs = Vec::new();
        if exif_at != 0 {
            ptrs.push((EXIF_IFD_POINTER, exif_at as u32));
        }
        if gps_at != 0 {
            ptrs.push((GPS_IFD_POINTER, gps_at as u32));
        }
        emit_ifd(
            &mut out,
            bo,
            ifd0_at,
            &ifd0,
            &value_offsets[0],
            &ptrs,
            ifd1_at as u32,
        );
    }
    if let (Some(t_at), Some(thumb)) = (thumb_at, plan.thumbnail.as_ref()) {
        out[t_at..t_at + thumb.len()].copy_from_slice(thumb);
    }

    Ok(out)
}

/// A FileReader over an in-memory byte slice (same shape as exif_inplace's).
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

/// Builds the new EXIF APP1 segment data ("Exif\0\0" + TIFF) for a JPEG,
/// preserving everything the caller did not change. Returns an empty Vec
/// when the EXIF segment should be dropped entirely.
pub fn rewrite_jpeg_exif(file_bytes: &[u8], desired: &MetadataMap) -> Result<Vec<u8>> {
    // Locate the original EXIF TIFF slice, if any
    let tiff: Option<Vec<u8>> = {
        let reader = SliceReader(file_bytes);
        let segments = parse_segments(&reader)?;
        segments
            .iter()
            .find(|s| s.is_app1() && s.data.starts_with(EXIF_IDENTIFIER))
            .map(|s| s.data[EXIF_IDENTIFIER.len()..].to_vec())
    };

    let (scan, original_map) = match &tiff {
        Some(tiff_bytes) => {
            let scan = scan_exif_entries(tiff_bytes)?;
            // The exact reader the diff must mirror: parse the whole JPEG the
            // same way read_metadata does (includes tag-name normalization)
            let reader = SliceReader(file_bytes);
            let original_map = crate::core::operations::parse_jpeg_metadata(&reader)?;
            (scan, original_map)
        }
        None => (
            ExifScan {
                byte_order: ByteOrder::LittleEndian,
                entries: Vec::new(),
                thumbnail: None,
                makernote_offset: None,
            },
            MetadataMap::new(),
        ),
    };

    let plan = plan_exif_write(&scan, &original_map, desired)?;
    let tiff_out = serialize_exif(&plan)?;
    if tiff_out.is_empty() {
        return Ok(Vec::new());
    }
    let mut segment = Vec::with_capacity(EXIF_IDENTIFIER.len() + tiff_out.len());
    segment.extend_from_slice(EXIF_IDENTIFIER);
    segment.extend_from_slice(&tiff_out);
    Ok(segment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::metadata_map::MetadataMap;
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
        assert!(
            !scan
                .entries
                .iter()
                .any(|e| { matches!(e.tag_id, 0x8769 | 0x8825 | 0x0201 | 0x0202) })
        );
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
        assert!(
            scan.entries
                .iter()
                .any(|e| e.ifd == IfdKind::Ifd0 && e.tag_id == 0x0132)
        );
        assert!(
            scan.entries
                .iter()
                .any(|e| e.ifd == IfdKind::ExifIfd && e.tag_id == MAKERNOTE)
        );
    }

    /// Runs scan + reader-symmetric conversion to build the original map the
    /// way plan_exif_write's callers do in production.
    fn scan_and_maps(tiff: &[u8]) -> (ExifScan, MetadataMap) {
        let scan = scan_exif_entries(tiff).unwrap();
        let mut map = MetadataMap::new();
        for e in &scan.entries {
            if matches!(e.ifd, IfdKind::Interop | IfdKind::Ifd1) || e.tag_id == MAKERNOTE {
                continue;
            }
            let key = crate::tag_db::lookup_tag_name(e.tag_id, e.ifd.prefix());
            let value = crate::core::tag_conversion::raw_bytes_to_tag_value(
                &e.value,
                e.field_type,
                e.count,
                e.tag_id,
                scan.byte_order,
            );
            map.insert(key, value);
        }
        (scan, map)
    }

    #[test]
    fn plan_noop_carries_everything() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let desired = original.clone();
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        // Every surfaced entry carried with identical raw bytes
        let cc = plan.exif_ifd.iter().find(|e| e.tag_id == 0x9101).unwrap();
        assert_eq!(cc.field_type, 7);
        assert_eq!(cc.value, [1, 2, 3, 0]);
        let gps = plan.gps.iter().find(|e| e.tag_id == 0x0000).unwrap();
        assert_eq!(gps.value, [2, 3, 0, 0]);
        // Unsurfaced classes carried too
        assert!(plan.exif_ifd.iter().any(|e| e.tag_id == MAKERNOTE));
        assert!(plan.ifd1.iter().any(|e| e.tag_id == 0x0103));
        assert_eq!(plan.makernote_pin, Some(116));
        assert!(plan.thumbnail.is_some());
    }

    #[test]
    fn plan_removal_by_absence() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.remove("IFD0:Orientation");
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        assert!(!plan.ifd0.iter().any(|e| e.tag_id == 0x0112));
        assert!(plan.ifd0.iter().any(|e| e.tag_id == 0x010F)); // Make survives
    }

    #[test]
    fn plan_changed_value_is_revalidated_and_retyped() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("IFD0:Make", TagValue::new_string("Nikon"));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let make = plan.ifd0.iter().find(|e| e.tag_id == 0x010F).unwrap();
        assert_eq!(make.field_type, 2);
        assert_eq!(make.value, b"Nikon\0");
        assert_eq!(make.count, 6);
    }

    #[test]
    fn plan_rejects_display_string_write_to_binary_tag() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        // User "modifies" ComponentsConfiguration with a display string:
        // strict validation must reject, exactly as before this change
        desired.insert(
            "ExifIFD:ComponentsConfiguration",
            TagValue::new_string("R, G, B, -"),
        );
        let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
        assert!(err.to_string().contains("Type mismatch"), "got: {}", err);
    }

    #[test]
    fn plan_added_tag_and_unknown_added_tag() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("IFD0:Artist", TagValue::new_string("A. Person"));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let artist = plan.ifd0.iter().find(|e| e.tag_id == 0x013B).unwrap();
        assert_eq!(artist.value, b"A. Person\0");

        let mut bad = original.clone();
        bad.insert("IFD0:NoSuchTagName", TagValue::new_string("x"));
        assert!(plan_exif_write(&scan, &original, &bad).is_err());
    }

    /// Loads the real Canon fixture, which has an actual InteropIFD whose
    /// entries the reader surfaces under "EXIF:InteropIndex" /
    /// "EXIF:InteropVersion" (see `parse_interop_subifd`). Builds
    /// `original_map` the same way `rewrite_jpeg_exif` does in production
    /// (the full JPEG reader), not the synthetic `scan_and_maps` helper,
    /// since only the real reader surfaces the "EXIF:"-prefixed Interop keys.
    fn canon_scan_and_maps() -> (ExifScan, MetadataMap, Vec<u8>) {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jpeg/makernotes/canon_sample.jpg"
        ))
        .unwrap();
        let tiff = super::super::exif_surgical_test_support::tiff_slice(&bytes).to_vec();
        let scan = scan_exif_entries(&tiff).unwrap();
        let reader = SliceReader(&bytes);
        let original = crate::core::operations::parse_jpeg_metadata(&reader).unwrap();
        assert!(
            original.contains_key("EXIF:InteropIndex"),
            "fixture must surface an InteropIFD tag for this test to be meaningful"
        );
        (scan, original, tiff)
    }

    #[test]
    fn plan_unchanged_interop_key_is_noop() {
        let (scan, original, _tiff) = canon_scan_and_maps();
        let desired = original.clone();
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        // The Interop bucket is carried unchanged (raw entries preserved
        // byte-for-byte, matching what scan_exif_entries found).
        let interop_tag_ids: Vec<u16> = scan
            .entries
            .iter()
            .filter(|e| e.ifd == IfdKind::Interop)
            .map(|e| e.tag_id)
            .collect();
        assert!(!interop_tag_ids.is_empty());
        for tag_id in interop_tag_ids {
            assert!(
                plan.interop.iter().any(|e| e.tag_id == tag_id),
                "Interop tag {:#06x} must be carried when unchanged",
                tag_id
            );
        }
    }

    #[test]
    fn plan_changed_interop_key_errors_instead_of_silently_dropping() {
        let (scan, original, _tiff) = canon_scan_and_maps();
        let mut desired = original.clone();
        let original_value = original.get("EXIF:InteropIndex").unwrap().clone();
        let new_value = TagValue::new_string("R03 - DCF option file (Adobe RGB)");
        assert_ne!(
            original_value, new_value,
            "test setup must actually change the value"
        );
        desired.insert("EXIF:InteropIndex", new_value);
        let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
        assert!(
            err.to_string().contains("InteropIFD") || err.to_string().contains("Interop"),
            "expected a clear error about unsupported Interop edits, got: {}",
            err
        );
    }

    /// The other half of the collision shape that motivated the generalized
    /// guard in `plan_exif_write` (see the comment above the
    /// `carried_reader_keys` check): MakerNote is an "unsurfaced class" that
    /// is always raw-carried, but the reader still surfaces it under a
    /// hex-fallback key ("ExifIFD:0x927C") because tag 0x927C has no name in
    /// the registry. Before the generalization, this key's presence in
    /// `original_map` made the "Added" loop treat a real edit as an
    /// already-known tag and silently `continue`, dropping it. Confirmed by
    /// reading the pre-fix code at `2e16b24` (`original_map.contains_key(&key)
    /// { continue; }` with no error path).
    #[test]
    fn plan_changed_makernote_key_errors_instead_of_silently_dropping() {
        let (scan, original, _tiff) = canon_scan_and_maps();
        assert!(
            original.get("ExifIFD:0x927C").is_some(),
            "fixture must surface the MakerNote hex-fallback key"
        );

        let mut desired = original.clone();
        let original_value = original.get("ExifIFD:0x927C").unwrap().clone();
        let new_value = TagValue::new_string("tampered");
        assert_ne!(
            original_value, new_value,
            "test setup must actually change the value"
        );
        desired.insert("ExifIFD:0x927C", new_value);

        let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
        // Must be a clear rejection, not Ok() with the edit silently dropped
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("not")
                && (msg.contains("0x927C")
                    || msg.to_lowercase().contains("makernote")
                    || msg.to_lowercase().contains("supported")),
            "expected a clear rejection error, got: {}",
            msg
        );
    }

    #[test]
    fn plan_unchanged_makernote_key_is_noop() {
        let (scan, original, _tiff) = canon_scan_and_maps();
        let desired = original.clone();

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        // The MakerNote entry must still be carried unchanged
        assert!(plan.exif_ifd.iter().any(|e| e.tag_id == MAKERNOTE));
    }

    #[test]
    fn plan_clear_semantics() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let desired = MetadataMap::new();
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        assert!(plan.ifd0.is_empty() && plan.exif_ifd.is_empty() && plan.gps.is_empty());
        assert!(plan.ifd1.is_empty() && plan.thumbnail.is_none());
    }

    #[test]
    fn plan_deduplicates_alias_keys_for_same_tag() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        // Same tag under both its native key and the EXIF: alias
        desired.insert("EXIF:Make", TagValue::new_string("Canon"));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let make_count = plan.ifd0.iter().filter(|e| e.tag_id == 0x010F).count();
        assert_eq!(
            make_count, 1,
            "must not emit duplicate entries for the same tag id"
        );
    }

    #[test]
    fn plan_edit_via_exif_alias_is_applied_not_dropped() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        // Caller edits via the "EXIF:" alias while the native key is untouched
        desired.insert("EXIF:Make", TagValue::new_string("Nikon"));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let make_entries: Vec<_> = plan.ifd0.iter().filter(|e| e.tag_id == 0x010F).collect();
        assert_eq!(make_entries.len(), 1, "must not duplicate the tag");
        assert_eq!(
            make_entries[0].value, b"Nikon\0",
            "the alias edit must actually be applied"
        );
    }

    #[test]
    fn plan_removing_interop_key_errors_instead_of_silent_noop() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jpeg/makernotes/canon_sample.jpg"
        ))
        .unwrap();
        let tiff = super::super::exif_surgical_test_support::tiff_slice(&bytes).to_vec();
        let scan = scan_exif_entries(&tiff).unwrap();
        let reader = SliceReader(&bytes);
        let original = crate::core::operations::parse_jpeg_metadata(&reader).unwrap();
        assert!(
            original.get("ExifIFD:0x927C").is_some(),
            "fixture must surface the MakerNote hex-fallback key"
        );
        let mut desired = original.clone();
        desired.remove("ExifIFD:0x927C"); // caller intends to remove the MakerNote
        let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("not"),
            "got: {}",
            err
        );
    }

    #[test]
    fn plan_preserves_long_type_hint_for_small_value() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        // Orientation is stored SHORT in the fixture; use tag_value_to_field
        // directly to test the LONG-hint path in isolation
        let (field_type, _count, _bytes) =
            tag_value_to_field(&TagValue::new_integer(5), Some(4)).unwrap();
        assert_eq!(
            field_type, 4,
            "a LONG-hinted small value must stay LONG, not downcast to SHORT"
        );
        let (field_type_short, _, _) =
            tag_value_to_field(&TagValue::new_integer(5), Some(3)).unwrap();
        assert_eq!(field_type_short, 3);
        let (field_type_none, _, _) = tag_value_to_field(&TagValue::new_integer(5), None).unwrap();
        assert_eq!(field_type_none, 3, "no hint: smallest-fit still applies");
        let _ = (scan, original); // silence unused if not otherwise referenced
    }

    /// The strongest possible property: serialize then rescan must reproduce
    /// the plan exactly (entries, blobs, byte order).
    fn assert_roundtrip(plan: &WritePlan) {
        let bytes = serialize_exif(plan).unwrap();
        let rescan = scan_exif_entries(&bytes).unwrap();
        assert_eq!(rescan.byte_order, plan.byte_order);
        let mut expected: Vec<(IfdKind, &OutEntry)> = Vec::new();
        for (ifd, list) in [
            (IfdKind::Ifd0, &plan.ifd0),
            (IfdKind::ExifIfd, &plan.exif_ifd),
            (IfdKind::Gps, &plan.gps),
            (IfdKind::Interop, &plan.interop),
            (IfdKind::Ifd1, &plan.ifd1),
        ] {
            for e in list {
                expected.push((ifd, e));
            }
        }
        assert_eq!(rescan.entries.len(), expected.len());
        for (ifd, e) in expected {
            let got = rescan
                .entries
                .iter()
                .find(|r| r.ifd == ifd && r.tag_id == e.tag_id)
                .unwrap_or_else(|| panic!("missing {:?}:{:#06x}", ifd, e.tag_id));
            assert_eq!(got.field_type, e.field_type, "type for {:#06x}", e.tag_id);
            assert_eq!(got.count, e.count, "count for {:#06x}", e.tag_id);
            assert_eq!(got.value, e.value, "value for {:#06x}", e.tag_id);
        }
        assert_eq!(rescan.thumbnail, plan.thumbnail);
    }

    #[test]
    fn serialize_roundtrips_noop_plan_le() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let plan = plan_exif_write(&scan, &original, &original.clone()).unwrap();
        assert_roundtrip(&plan);
    }

    #[test]
    fn serialize_roundtrips_noop_plan_be() {
        let tiff = build_full_tiff(ByteOrder::BigEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let plan = plan_exif_write(&scan, &original, &original.clone()).unwrap();
        assert_roundtrip(&plan);
    }

    #[test]
    fn serialize_honors_makernote_pin() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let plan = plan_exif_write(&scan, &original, &original.clone()).unwrap();
        assert_eq!(plan.makernote_pin, Some(116));
        let bytes = serialize_exif(&plan).unwrap();
        // The makernote payload must sit at its original offset
        assert_eq!(
            &bytes[116..124],
            &[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn serialize_empty_plan_is_empty() {
        let plan = WritePlan {
            byte_order: ByteOrder::LittleEndian,
            ifd0: vec![],
            exif_ifd: vec![],
            gps: vec![],
            interop: vec![],
            ifd1: vec![],
            thumbnail: None,
            makernote_pin: None,
        };
        assert!(serialize_exif(&plan).unwrap().is_empty());
    }

    #[test]
    fn serialize_real_fixture_noop_roundtrip() {
        for fixture in [
            "/tests/fixtures/jpeg/complex/synthetic_gps_001.jpg",
            "/tests/fixtures/jpeg/makernotes/canon_sample.jpg",
        ] {
            let bytes =
                std::fs::read(format!("{}{}", env!("CARGO_MANIFEST_DIR"), fixture)).unwrap();
            let tiff = crate::writers::exif_surgical_test_support::tiff_slice(&bytes);
            let (scan, original) = scan_and_maps(tiff);
            let plan = plan_exif_write(&scan, &original, &original.clone()).unwrap();
            assert_roundtrip(&plan);
        }
    }
}
