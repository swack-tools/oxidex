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
use crate::tag_db::tag_registry::{
    declared_ieee_field_type, get_tag_descriptor, has_reliable_value_type,
};

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

/// `%Image::ExifTool::Exif::Main` tags (the table of IFD0, ExifIFD,
/// InteropIFD and IFD1) whose value locates bytes outside the entry itself,
/// which [`serialize_exif`] does not model: it writes the value back while
/// re-laying out everything else, so the bytes it pointed to are not copied
/// and the pointer dangles. From the pinned Exif.pm 13.59 table:
///
/// - `SubDirectory` entries whose directory lives elsewhere (`Start =>
///   '$val'`, `Flags => 'SubIFD'`): 0x014a SubIFDs, 0x0190
///   GlobalParametersIFD, 0x8290 KodakIFD, 0x888a LeafSubIFD, 0xc634
///   DNGPrivateData (SR2Private, and the maker-note variants whose IFDs sit
///   at `$valuePtr + N` with absolute offsets), 0xc6f5 ProfileIFD, 0xfe00
///   KDC_IFD, and 0xc51b HasselbladExif (an IFD at `$valuePtr`);
/// - `IsOffset` tags: 0x0111 StripOffsets, 0x0120 FreeOffsets, 0x0144
///   TileOffsets, 0x0201 (outside IFD1, where the thumbnail pair is
///   modelled), 0x0207-0x0209 JPEG tables, 0x8781, 0xa010
///   SamsungRawPointersOffset, 0xbcc0 and 0xbcc2.
///
/// The structural pointers the writer does model (0x8769, 0x8825, 0xa005,
/// IFD1's 0x0201/0x0202) and the MakerNote, which it pins at its original
/// offset, are not listed. GPS uses `GPS::Main`, which has none.
const UNMODELLED_POINTER_TAGS: &[u16] = &[
    0x0111, 0x0120, 0x0144, 0x014a, 0x0190, 0x0201, 0x0207, 0x0208, 0x0209, 0x8290, 0x8781, 0x888a,
    0xa010, 0xbcc0, 0xbcc2, 0xc51b, 0xc634, 0xc6f5, 0xfe00,
];

/// The first entry of `scan` that [`UNMODELLED_POINTER_TAGS`] names.
fn unmodelled_pointer(scan: &ExifScan) -> Option<&RawEntry> {
    scan.entries
        .iter()
        .find(|entry| entry.ifd != IfdKind::Gps && UNMODELLED_POINTER_TAGS.contains(&entry.tag_id))
}

/// A group-wide removal `<group>:All`, as pinned ExifTool 13.59 applies it to
/// an EXIF block in a JPEG APP1 or PNG eXIf (measured on both byte orders):
/// `IFD0:All` and `EXIF:All` remove the whole block; `ExifIFD:All` removes
/// ExifIFD with the InteropIFD and MakerNote inside it; `GPS:All`, `IFD1:All`
/// (with the thumbnail) and `InteropIFD:All` remove that directory;
/// `MakerNotes:All` removes a MakerNote ExifTool files under MakerNotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupRemoval {
    Carrier,
    ExifIfd,
    Gps,
    Ifd1,
    Interop,
    MakerNotes,
}

/// The group-wide removal `key` names, if it is `<group>:All`.
pub(crate) fn group_removal(key: &str) -> Option<GroupRemoval> {
    let (group, name) = key.split_once(':')?;
    if !name.eq_ignore_ascii_case("all") {
        return None;
    }
    Some(match group.to_ascii_lowercase().as_str() {
        "ifd0" | "exif" => GroupRemoval::Carrier,
        "exififd" => GroupRemoval::ExifIfd,
        "gps" => GroupRemoval::Gps,
        "ifd1" => GroupRemoval::Ifd1,
        "interopifd" => GroupRemoval::Interop,
        "makernotes" => GroupRemoval::MakerNotes,
        _ => return None,
    })
}

/// Whether `-MakerNotes:All=` deletes the block's MakerNote: pinned
/// ExifTool 13.59 files a note no maker parser claims under EXIF, not
/// MakerNotes, when a `%MakerNotes::Main` fallback names it
/// (MakerNoteUnknownText / MakerNoteUnknownBinary / MakerNoteSamsung1a:
/// "1 image files unchanged"), and deletes any other note (a recognized
/// maker's, an unknown IFD, a preview).
fn makernote_in_makernotes_group(scan: &ExifScan, original_map: &MetadataMap) -> bool {
    scan.entries
        .iter()
        .any(|entry| entry.ifd == IfdKind::ExifIfd && entry.tag_id == MAKERNOTE)
        && ![
            "ExifIFD:MakerNoteUnknownText",
            "ExifIFD:MakerNoteUnknownBinary",
            "ExifIFD:MakerNoteSamsung1a",
        ]
        .iter()
        .any(|key| original_map.contains_key(*key))
}

/// Whether `scan` holds anything the group-wide removal `group` deletes.
pub(crate) fn group_has_content(
    group: GroupRemoval,
    scan: &ExifScan,
    original_map: &MetadataMap,
) -> bool {
    let any = |ifds: &[IfdKind]| scan.entries.iter().any(|entry| ifds.contains(&entry.ifd));
    match group {
        // The carrier itself goes, empty or not (an empty PNG eXIf chunk
        // too: pinned ExifTool 13.59 drops it on `IFD0:All` / `EXIF:All`
        // while keeping it on every other removal).
        GroupRemoval::Carrier => true,
        GroupRemoval::ExifIfd => any(&[IfdKind::ExifIfd, IfdKind::Interop]),
        GroupRemoval::Gps => any(&[IfdKind::Gps]),
        GroupRemoval::Ifd1 => any(&[IfdKind::Ifd1]) || scan.thumbnail.is_some(),
        GroupRemoval::Interop => any(&[IfdKind::Interop]),
        GroupRemoval::MakerNotes => makernote_in_makernotes_group(scan, original_map),
    }
}

/// The EXIF rows a write sets: the planned rows of `desired` whose value is
/// not `original_map`'s. After a carrier- or group-wide removal these, and
/// only these, are written back (delete first, then set).
fn requested_sets(original_map: &MetadataMap, desired: &MetadataMap) -> MetadataMap {
    let mut sets = MetadataMap::new();
    for (key, value) in desired.iter() {
        if is_planned_key(key) && original_map.get(key.as_str()) != Some(value) {
            sets.insert(key.clone(), value.clone());
        }
    }
    sets
}

/// An empty scan in `byte_order`: the starting point of a fresh block.
fn fresh_scan(byte_order: ByteOrder) -> ExifScan {
    ExifScan {
        byte_order,
        entries: Vec::new(),
        thumbnail: None,
        makernote_offset: None,
        ifd1_next: None,
    }
}

/// The payload a carrier-wide removal (`IFD0:All` / `EXIF:All`) leaves:
/// nothing when the write sets nothing, else a fresh block holding the
/// sets, in the deleted block's byte order when it had a readable one.
/// The deleted block is never parsed beyond its byte-order mark.
fn fresh_block_for_sets(
    deleted: &[u8],
    original_map: &MetadataMap,
    desired: &MetadataMap,
) -> Result<Vec<u8>> {
    let sets = requested_sets(original_map, desired);
    if sets.is_empty() {
        return Ok(Vec::new());
    }
    let byte_order = match deleted.get(..2) {
        Some(b"MM") => ByteOrder::BigEndian,
        _ => ByteOrder::LittleEndian,
    };
    let plan = plan_exif_write_inner(
        &fresh_scan(byte_order),
        &MetadataMap::new(),
        &sets,
        &[],
        false,
    )?;
    serialize_exif(&plan)
}

/// Whether the group-wide removals in `removed` delete a whole EXIF carrier
/// (`IFD0:All` / `EXIF:All` on a JPEG or PNG). Such a write never parses the
/// carrier: pinned ExifTool 13.59 drops the APP1 / eXIf wholesale, even one
/// it cannot read, and its only post-condition is that the carrier is gone.
pub(crate) fn removes_carrier(removed: &[String]) -> bool {
    removed
        .iter()
        .any(|key| group_removal(key) == Some(GroupRemoval::Carrier))
}

/// Pinned ExifTool 13.59's raw types (Writer.pl `%rawType`): the files it
/// will not delete IFD0, ExifIFD or MakerNotes from ("Can't delete ... from
/// CR2", file unchanged).
const EXIFTOOL_RAW_TYPES: &[&str] = &[
    "3FR", "CR3", "IIQ", "NEF", "RW2", "ARQ", "CRW", "K25", "NRW", "RWL", "ARW", "DCR", "KDC",
    "ORF", "SR2", "ERF", "MEF", "PEF", "SRF", "CR2", "FFF", "MOS", "RAW", "SRW",
];

/// PanasonicRaw 0x002e JpgFromRaw: the one embedded image of a
/// TIFF-structured file ExifTool writes into ("processed as an embedded
/// document because it contains full EXIF").
const PANASONIC_JPG_FROM_RAW: u16 = 0x002e;

/// IFD0 0xc634 DNGPrivateData, whose Adobe `MakN` record carries a maker
/// note ExifTool files under MakerNotes.
const DNG_PRIVATE_DATA: u16 = 0xc634;

/// Resolves the group-wide `<group>:All` removals of a write to a
/// TIFF-structured file (`file_bytes`, read by the reader into `baseline`)
/// against what pinned ExifTool 13.59 does there, and returns `removed`
/// without the ones that leave the file unchanged.
///
/// There ExifTool never deletes IFD0 ("Can't delete IFD0 from TIFF"), and
/// from a raw type ([`EXIFTOOL_RAW_TYPES`]) not ExifIFD or MakerNotes
/// either; `EXIF:All` deletes only ExifIFD (Writer.pl `InitWriteDirs`).
/// Every other group with content -- GPS, InteropIFD, IFD1 (an error there:
/// "Deleting IFD1 also deletes subsequent IFD's"), ExifIFD and MakerNotes
/// (a DNGPrivateData maker note too) of a non-raw file, and any group of a
/// Panasonic JpgFromRaw's own EXIF -- is a directory deletion this in-place
/// writer cannot make, and is refused rather than reported as done.
pub(crate) fn resolve_tiff_group_removals(
    file_bytes: &[u8],
    baseline: &MetadataMap,
    removed: &[String],
) -> Result<Vec<String>> {
    if !removed.iter().any(|key| group_removal(key).is_some()) {
        return Ok(removed.to_vec());
    }
    let file_type = baseline.get_string("File:FileType").unwrap_or("TIFF");
    let raw = EXIFTOOL_RAW_TYPES.contains(&file_type);
    let scan = scan_entries_with_magics(
        file_bytes,
        crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
    )?;
    let ifd0_value = |tag_id: u16| {
        scan.entries
            .iter()
            .find(|entry| entry.ifd == IfdKind::Ifd0 && entry.tag_id == tag_id)
            .map(|entry| entry.value.as_slice())
    };
    let dng_makernote = ifd0_value(DNG_PRIVATE_DATA)
        .is_some_and(|data| data.starts_with(b"Adobe\0") && data.windows(4).any(|w| w == b"MakN"));
    // The embedded JPEG's EXIF, if the file has one ExifTool writes into.
    let embedded = ifd0_value(PANASONIC_JPG_FROM_RAW)
        .and_then(|jpeg| jpeg_exif_payload(jpeg).ok().flatten())
        .and_then(|tiff| scan_exif_entries(&tiff).ok());

    let mut kept = Vec::with_capacity(removed.len());
    for key in removed {
        let Some(group) = group_removal(key) else {
            kept.push(key.clone());
            continue;
        };
        let is_ifd0 = key
            .split_once(':')
            .is_some_and(|(g, _)| g.eq_ignore_ascii_case("IFD0"));
        let has = |g: GroupRemoval| group_has_content(g, &scan, baseline);
        let main = match group {
            GroupRemoval::Carrier if is_ifd0 => false,
            GroupRemoval::Carrier | GroupRemoval::ExifIfd => !raw && has(GroupRemoval::ExifIfd),
            GroupRemoval::MakerNotes => !raw && (has(group) || dng_makernote),
            GroupRemoval::Gps | GroupRemoval::Ifd1 | GroupRemoval::Interop => has(group),
        };
        let in_embedded = embedded.as_ref().is_some_and(|emb| match group {
            GroupRemoval::Carrier => true,
            GroupRemoval::ExifIfd | GroupRemoval::MakerNotes => {
                !raw && group_has_content(group, emb, baseline)
            }
            _ => group_has_content(group, emb, baseline),
        });
        if main || in_embedded {
            let place = if in_embedded && !main {
                " in the embedded JpgFromRaw"
            } else {
                ""
            };
            return Err(ExifToolError::unsupported_format(format!(
                "Removing '{key}' from a {file_type} file is not supported: pinned \
                 ExifTool 13.59 deletes that directory{place}, and this writer edits \
                 entries in place and cannot delete a directory"
            )));
        }
        // Pinned ExifTool 13.59 leaves the file unchanged: a no-op here too.
    }
    Ok(kept)
}

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
///   - InteropIFD tags: `parse_interop_subifd` (`src/core/tiff_helpers.rs`)
///     keys every row `InteropIFD:<name>`, i.e. `lookup_tag_name(tag_id,
///     "InteropIFD")`. Before decision D-1 of slice E-1 it keyed the DCF
///     tags under a hard-coded "EXIF:" prefix with its own name table; that
///     second candidate stays until slice E-D deletes `interop_tag_to_name`.
/// Returns both candidate keys so the Added-tag loop can recognize a
/// collision precisely instead of treating every key already present in
/// `original_map` as carried.
fn carried_class_reader_keys(entry: &RawEntry) -> Vec<String> {
    let mut keys = vec![lookup_tag_name(entry.tag_id, entry.ifd.prefix())];
    if entry.ifd == IfdKind::ExifIfd && entry.tag_id == MAKERNOTE {
        // A MakerNote no maker parser claims is surfaced under the
        // `%MakerNotes::Main` fallback whose Condition matched (`tiff_helpers`;
        // MakerNotes.pm 13.59:951, 1102, 1110). Without these the unchanged
        // row was taken for a new tag to add, and every other edit of the
        // file failed with "not a known EXIF tag".
        for name in [
            "MakerNoteSamsung1a",
            "MakerNoteUnknownText",
            "MakerNoteUnknownBinary",
        ] {
            keys.push(format!("ExifIFD:{name}"));
        }
    }
    if entry.ifd == IfdKind::Interop {
        let name = crate::core::tiff_helpers::interop_tag_to_name(entry.tag_id);
        if name != "Unknown" {
            keys.push(format!("EXIF:{}", name));
        }
    }
    keys
}

/// The key the reader surfaced a surfaced-class entry (IFD0, ExifIFD, GPS)
/// under, and whether that key is the entry's own.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReaderKey {
    /// The entry's own key: diffed, and the entry rewritten when it changes.
    Owned(String),
    /// A name the generated `Exif::Main` gives an ExifIFD entry `tag_db` has
    /// no name for, which `tag_db` gives a DIFFERENT id (or none): the TIFF/EP
    /// FocalPlaneXResolution 0x920e of a Leica M8/M9 is surfaced as
    /// `ExifIFD:FocalPlaneXResolution`, the EXIF 2.x 0xa20e's name. The
    /// entry is not the tag that name writes -- ExifTool writes 0xa20e and
    /// leaves 0x920e alone -- so it is carried raw; the key, unchanged, is
    /// not a tag to add, and changed, it is added under its own id.
    Borrowed(String),
}

/// The key `original_map` holds for an ExifIFD entry `tag_id` that
/// `tag_db` has no name for, when the reader surfaced it under the name the
/// generated `Exif::Main` reports it by (slice E-2: 178 reported ids have no
/// `tag_db` name; `lookup_tag_name` gives `ExifIFD:0x9210`, the engine row
/// is `ExifIFD:FocalPlaneResolutionUnit`).
pub(crate) fn engine_reader_key(tag_id: u16, original_map: &MetadataMap) -> Option<String> {
    let name = crate::core::exif_dir_engine::exif_main_reported_name(tag_id)?;
    let key = format!("ExifIFD:{name}");
    original_map.contains_key(&key).then_some(key)
}

/// Whether writing `key` writes the tag `tag_id` (`tag_db`'s id for the
/// name, the one the add path would plant).
pub(crate) fn key_writes_tag_id(key: &str, tag_id: u16) -> bool {
    get_tag_descriptor(key).and_then(descriptor_tag_id) == Some(tag_id)
}

/// The metadata-map key the reader surfaces a surfaced-class entry under:
/// `lookup_tag_name`'s spelling, except for an ExifIFD entry the reader
/// surfaced by its engine name ([`engine_reader_key`]), which is the
/// entry's own only when `tag_db` writes that name to this very id
/// ([`ReaderKey`]). Without the engine name, an unchanged engine-named row
/// was taken for a new tag to add, and its printed value failed the add
/// path's validation; taken as the entry's own, an edit rewrote the
/// non-writable legacy entry instead of adding the EXIF 2.x tag.
fn surfaced_reader_key(entry: &RawEntry, original_map: &MetadataMap) -> ReaderKey {
    let key = lookup_tag_name(entry.tag_id, entry.ifd.prefix());
    if entry.ifd == IfdKind::ExifIfd
        && !original_map.contains_key(&key)
        && let Some(engine_key) = engine_reader_key(entry.tag_id, original_map)
    {
        return if key_writes_tag_id(&engine_key, entry.tag_id) {
            ReaderKey::Owned(engine_key)
        } else {
            ReaderKey::Borrowed(engine_key)
        };
    }
    ReaderKey::Owned(key)
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
    /// IFD1's next-IFD pointer when nonzero: a directory chain past IFD1
    /// (IFD2 -- a Leica JPEG's PreviewImage) that the serializer does not
    /// model and would drop, so a block carrying one is never re-laid out.
    pub ifd1_next: Option<usize>,
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

/// One entry already placed in the plan, remembered so the Added loop can
/// judge a numeric tag-id collision instead of dropping it.
///
/// Two distinct `MetadataMap` keys can resolve to the same numeric EXIF tag id
/// (`"IFD0:Make"` and `"EXIF:Make"`; `"ExifIFD:ExposureCompensation"` and
/// `"ExifIFD:ExposureBiasValue"`, both 0x9204). Only one IFD record may carry a
/// given tag id, so the second one cannot be emitted -- but whether discarding
/// it *loses* anything depends entirely on the value:
///
///   - identical to what is already planned -> nothing is lost, skip silently;
///   - different -> the caller's edit would vanish while the CLI still reports
///     success, so it must be refused loudly instead.
///
/// `value` is the effective desired value for the placed entry, or `None` when
/// the entry is raw-carried from bytes the reader never surfaced (an unsurfaced
/// IFD class, or a tag with no reader key). `None` never compares equal. A
/// collision with an unsurfaced-class entry is refused; one with a
/// surfaced-class entry the reader gave no row (`rowless`) is the caller's
/// edit of that very entry, which replaces it -- see the Added loop.
#[derive(Debug, Clone)]
struct PlacedEntry {
    ifd: IfdKind,
    tag_id: u16,
    key: Option<String>,
    value: Option<TagValue>,
    /// For an IFD0/ExifIFD/GPS entry carried because the reader surfaced no
    /// row for it: its slot in its IFD's plan bucket and its stored field
    /// type, so an explicit edit of its id can replace it in place.
    rowless: Option<(usize, u16)>,
}

/// Whether `key` names a class of entry the surgical writers carry raw and
/// can neither add nor edit: IFD1, InteropIFD, MakerNotes.
pub(crate) fn is_carried_only_key(key: &str) -> bool {
    key.starts_with("IFD1:") || key.starts_with("InteropIFD:") || key.starts_with("MakerNotes:")
}

/// Whether a named removal `key` names an entry of a raw-carried class that
/// `scan` actually holds: the reader surfaced that exact key, or the key's
/// tag id is that of an IFD1/InteropIFD entry present (IFD1's thumbnail
/// pair when a thumbnail is present), or a `MakerNotes:` key names the
/// MakerNote blob by one of the names the reader gives it. `<group>:All`
/// names every entry of its class. A name with no tag id is not a wildcard:
/// removing a tag the block does not hold is a no-op, as `remove_tag`
/// promises and as ExifTool reports ("not defined" / "unchanged").
fn removal_names_carried_entry(key: &str, scan: &ExifScan, original_map: &MetadataMap) -> bool {
    // The family-0 alias `EXIF:<name>` names an IFD1/InteropIFD entry (or
    // the MakerNote) as well -- the planner maps `-EXIF:InteropIndex=R03` to
    // InteropIFD when setting -- so its removal is judged against those
    // entries by the reader's names and by tag id.
    if let Some(name) = key.strip_prefix("EXIF:") {
        let tag_id = get_tag_descriptor(key).and_then(descriptor_tag_id);
        return scan.entries.iter().any(|entry| {
            let note = entry.ifd == IfdKind::ExifIfd && entry.tag_id == MAKERNOTE;
            (matches!(entry.ifd, IfdKind::Ifd1 | IfdKind::Interop) || note)
                && (carried_class_reader_keys(entry).iter().any(|reader_key| {
                    reader_key == key || reader_key.split_once(':').map(|(_, n)| n) == Some(name)
                }) || (!note && tag_id == Some(entry.tag_id)))
        });
    }
    if !is_carried_only_key(key) {
        return false;
    }
    if original_map.contains_key(key) {
        return true;
    }
    let Some((group, name)) = key.split_once(':') else {
        return false;
    };
    let all = name.eq_ignore_ascii_case("all");
    let ifd = match group {
        "IFD1" => IfdKind::Ifd1,
        "InteropIFD" => IfdKind::Interop,
        "MakerNotes" => {
            return scan.entries.iter().any(|entry| {
                entry.ifd == IfdKind::ExifIfd
                    && entry.tag_id == MAKERNOTE
                    && (all
                        || carried_class_reader_keys(entry).iter().any(|reader_key| {
                            reader_key.split_once(':').map(|(_, n)| n) == Some(name)
                        }))
            });
        }
        _ => return false,
    };
    if all {
        return scan.entries.iter().any(|entry| entry.ifd == ifd)
            || (ifd == IfdKind::Ifd1 && scan.thumbnail.is_some());
    }
    let Some(tag_id) = get_tag_descriptor(key).and_then(descriptor_tag_id) else {
        return false;
    };
    (ifd == IfdKind::Ifd1
        && scan.thumbnail.is_some()
        && matches!(tag_id, THUMBNAIL_OFFSET | THUMBNAIL_LENGTH))
        || scan
            .entries
            .iter()
            .any(|entry| entry.ifd == ifd && entry.tag_id == tag_id)
}

/// The refusal for an added or changed [`is_carried_only_key`] key.
pub(crate) fn carried_only_edit_refused(key: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!(
        "Editing tag '{}' is not yet supported: it belongs to an unsurfaced IFD \
         class (InteropIFD/IFD1/MakerNote) that this writer always raw-carries \
         and cannot add to or edit",
        key
    ))
}

/// The IFD a group-qualified write key names, for the three IFDs this
/// writer edits. `EXIF:` names the family, not an IFD, and has none.
fn key_ifd(key: &str) -> Option<IfdKind> {
    if key.starts_with("IFD0:") {
        Some(IfdKind::Ifd0)
    } else if key.starts_with("ExifIFD:") {
        Some(IfdKind::ExifIfd)
    } else if key.starts_with("GPS:") {
        Some(IfdKind::Gps)
    } else {
        None
    }
}

/// Whether the caller's request to delete `key` names `entry`, an entry the
/// reader surfaced no row for (so `key` is not in `original_map`, and its
/// absence from `desired` alone cannot say it was removed): `key` is
/// qualified by the entry's own IFD and writes the entry's own id.
///
/// Before slice E-2's decisions D-2 and D-3 the reader surfaced such entries
/// (DJI_XT2.jpg's ExifIFD 0x02bc ApplicationNotes, SamsungGT-B2710.jpg's
/// 0x9205 MaxApertureValue), so `-ExifIFD:ApplicationNotes=` deleted the
/// entry, as ExifTool does; without this it would be silently carried.
pub(crate) fn removal_names_rowless_entry(
    key: &str,
    ifd: IfdKind,
    tag_id: u16,
    original_map: &MetadataMap,
) -> bool {
    key_ifd(key) == Some(ifd) && !original_map.contains_key(key) && key_writes_tag_id(key, tag_id)
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
pub(crate) fn tag_value_to_field(
    value: &TagValue,
    hint: Option<u16>,
) -> Result<(u16, u32, Vec<u8>)> {
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
            if hint != Some(10) && *numerator >= 0 && *denominator >= 0 {
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
        // TIFF has two IEEE 754 widths and `TagValue::Float` is an f64, so the
        // width has to come from the hint: the entry's existing field type on
        // an edit, or the tag's declared type when one is being created.
        // Without it every float-declared tag was written as a double, which
        // `exiftool -validate` reports as
        // "Non-standard format (double) for IFD0 0xcd49 JXLDistance".
        TagValue::Float(f) => match hint {
            Some(11) => Ok((11, 1, (*f as f32).to_ne_bytes().to_vec())),
            _ => Ok((12, 1, f.to_ne_bytes().to_vec())),
        },
        TagValue::Array(_) | TagValue::Struct(_) => Err(ExifToolError::parse_error(
            "Array/Struct values are not supported for EXIF write",
        )),
    }
}

/// GPS.pm 13.59 declares GPSLongitude as `rational64u[3]` and applies
/// `ToDMS` to the decimal value before TIFF serialization. Preserve the
/// input rational exactly while splitting it into degrees, minutes, seconds.
fn gps_longitude_to_field(value: &TagValue) -> Result<(u16, u32, Vec<u8>)> {
    let TagValue::Rational {
        numerator,
        denominator,
    } = value
    else {
        return tag_value_to_field(value, Some(5));
    };
    if *denominator == 0 {
        return Err(ExifToolError::parse_error(
            "GPSLongitude must be a finite coordinate",
        ));
    }

    let numerator = i64::from(*numerator).unsigned_abs();
    let denominator = i64::from(*denominator).unsigned_abs();
    let degrees = numerator / denominator;
    let minute_numerator = (numerator % denominator) * 60;
    let minutes = minute_numerator / denominator;
    let second_numerator = (minute_numerator % denominator) * 60;
    let divisor = gcd_u64(second_numerator, denominator);
    let seconds = (second_numerator / divisor, denominator / divisor);
    let parts = [(degrees, 1), (minutes, 1), seconds];

    let mut bytes = Vec::with_capacity(24);
    for (numerator, denominator) in parts {
        let numerator = u32::try_from(numerator).map_err(|_| {
            ExifToolError::parse_error("GPSLongitude component exceeds rational64u range")
        })?;
        let denominator = u32::try_from(denominator).map_err(|_| {
            ExifToolError::parse_error("GPSLongitude component exceeds rational64u range")
        })?;
        bytes.extend_from_slice(&numerator.to_ne_bytes());
        bytes.extend_from_slice(&denominator.to_ne_bytes());
    }
    Ok((5, 3, bytes))
}

fn gcd_u64(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left.max(1)
}

/// Applies tag-specific inverse conversions before generic TIFF serialization.
/// GPS.pm 13.59 defines both coordinates as `rational64u[3]` values converted
/// by `ToDMS` into degree, minute and second rationals.
pub(crate) fn tag_value_to_field_for_key(
    key: &str,
    value: &TagValue,
    hint: Option<u16>,
) -> Result<(u16, u32, Vec<u8>)> {
    // Exif.pm 13.59 0x9c9b-0x9c9f: the map holds the decoded text, and
    // ExifTool stores `ValueConvInv` of it -- UCS-2LE plus a NUL pair, as
    // `int8u`, or as `undef` over an existing `undef` entry (`hint`).
    if crate::writers::xp_strings::is_xp_tag_key(key) {
        return crate::writers::xp_strings::xp_field(value, hint);
    }
    if key.rsplit(':').next() == Some("GPSVersionID")
        && !matches!(value, TagValue::Binary(bytes) if bytes.len() == 4)
    {
        return Err(ExifToolError::parse_error(
            "GPSVersionID must contain exactly four bytes",
        ));
    }
    if matches!(key, "ISO" | "EXIF:ISO" | "ExifIFD:ISO")
        && let TagValue::Array(values) = value
    {
        if values.is_empty() {
            return Err(ExifToolError::parse_error(
                "ISO requires at least one unsigned 16-bit value",
            ));
        }
        let mut bytes = Vec::with_capacity(values.len() * 2);
        for value in values {
            let TagValue::Integer(value) = value else {
                return Err(ExifToolError::parse_error(
                    "ISO values must be unsigned 16-bit integers",
                ));
            };
            if !(0..=u16::MAX as i64).contains(value) {
                return Err(ExifToolError::parse_error(
                    "ISO value does not fit unsigned 16-bit",
                ));
            }
            bytes.extend_from_slice(&(*value as u16).to_ne_bytes());
        }
        return Ok((3, values.len() as u32, bytes));
    }
    if key.rsplit(':').next() == Some("SubjectArea") {
        if let TagValue::Array(values) = value
            && (2..=4).contains(&values.len())
        {
            let mut bytes = Vec::with_capacity(values.len() * 2);
            for value in values {
                let TagValue::Integer(component) = value else {
                    return Err(ExifToolError::parse_error(
                        "SubjectArea requires unsigned shorts",
                    ));
                };
                let component = u16::try_from(*component).map_err(|_| {
                    ExifToolError::parse_error("SubjectArea requires unsigned shorts")
                })?;
                bytes.extend_from_slice(&component.to_ne_bytes());
            }
            return Ok((3, values.len() as u32, bytes));
        }
        return Err(ExifToolError::parse_error(
            "SubjectArea requires 2 to 4 unsigned short values",
        ));
    }
    if key.rsplit(':').next() == Some("SubjectLocation") {
        if let TagValue::Array(values) = value
            && let [TagValue::Integer(x), TagValue::Integer(y)] = values.as_slice()
            && (0..=u16::MAX as i64).contains(x)
            && (0..=u16::MAX as i64).contains(y)
        {
            let mut bytes = (u16::try_from(*x).expect("range checked"))
                .to_ne_bytes()
                .to_vec();
            bytes.extend_from_slice(&(u16::try_from(*y).expect("range checked")).to_ne_bytes());
            return Ok((3, 2, bytes));
        }
        return Err(ExifToolError::parse_error(
            "SubjectLocation requires exactly two unsigned short values",
        ));
    }
    let hint = match key.rsplit(':').next() {
        Some("ShutterSpeedValue" | "BrightnessValue") => Some(10),
        Some("GPSVersionID") => Some(1),
        _ => hint,
    };
    // Exif.pm 13.59 declares TileWidth (0x0142) and TileLength (0x0143) as
    // int32u. Do not let generic smallest-fit encoding downcast a newly
    // created small value to SHORT.
    if matches!(
        key,
        "IFD0:TileWidth"
            | "EXIF:TileWidth"
            | "IFD0:TileLength"
            | "EXIF:TileLength"
            | "EXIF:ISOSpeed"
            | "ExifIFD:ISOSpeed"
            | "EXIF:ISOSpeedLatitudeyyy"
            | "ExifIFD:ISOSpeedLatitudeyyy"
            | "EXIF:ISOSpeedLatitudezzz"
            | "ExifIFD:ISOSpeedLatitudezzz"
            | "EXIF:RecommendedExposureIndex"
            | "ExifIFD:RecommendedExposureIndex"
            | "EXIF:StandardOutputSensitivity"
            | "ExifIFD:StandardOutputSensitivity"
    ) {
        return tag_value_to_field(value, Some(4));
    }
    // Exif.pm 13.59 0x0129 declares PageNumber as `int16u[2]`. It is exposed
    // as a space-separated pair, so preserve both values instead of falling
    // through to the generic array rejection.
    if matches!(key, "IFD0:PageNumber" | "EXIF:PageNumber" | "PageNumber") {
        let TagValue::Array(values) = value else {
            return Err(ExifToolError::parse_error(
                "PageNumber requires two unsigned 16-bit values",
            ));
        };
        if values.len() != 2 {
            return Err(ExifToolError::parse_error(
                "PageNumber requires exactly two unsigned 16-bit values",
            ));
        }
        let mut bytes = Vec::with_capacity(4);
        for value in values {
            let TagValue::Integer(value) = value else {
                return Err(ExifToolError::parse_error(
                    "PageNumber components must be unsigned 16-bit integers",
                ));
            };
            if !(0..=u16::MAX as i64).contains(value) {
                return Err(ExifToolError::parse_error(
                    "PageNumber component does not fit unsigned 16-bit",
                ));
            }
            bytes.extend_from_slice(&(*value as u16).to_ne_bytes());
        }
        return Ok((3, 2, bytes));
    }
    if matches!(
        key,
        "CompositeImageCount" | "EXIF:CompositeImageCount" | "ExifIFD:CompositeImageCount"
    ) {
        let TagValue::Array(values) = value else {
            return Err(ExifToolError::parse_error(
                "CompositeImageCount requires two unsigned 16-bit values",
            ));
        };
        if values.len() != 2 {
            return Err(ExifToolError::parse_error(
                "CompositeImageCount requires exactly two unsigned 16-bit values",
            ));
        }
        let mut bytes = Vec::with_capacity(4);
        for value in values {
            let TagValue::Integer(value) = value else {
                return Err(ExifToolError::parse_error(
                    "CompositeImageCount components must be unsigned 16-bit integers",
                ));
            };
            if !(0..=u16::MAX as i64).contains(value) {
                return Err(ExifToolError::parse_error(
                    "CompositeImageCount component does not fit unsigned 16-bit",
                ));
            }
            bytes.extend_from_slice(&(*value as u16).to_ne_bytes());
        }
        return Ok((3, 2, bytes));
    }
    if matches!(
        key,
        "GPS:GPSLongitude" | "GPS:GPSDestLatitude" | "GPS:GPSDestLongitude"
    ) {
        return gps_longitude_to_field(value);
    }
    if key == "GPS:GPSLatitude"
        && let TagValue::Rational {
            numerator,
            denominator,
        } = value
    {
        if *denominator <= 0 {
            return Err(ExifToolError::parse_error(
                "GPSLatitude requires a positive rational denominator",
            ));
        }
        let denominator = i64::from(*denominator);
        let coordinate = i64::from(*numerator).abs();
        let degrees = coordinate / denominator;
        let degree_remainder = coordinate % denominator;
        let minute_numerator = degree_remainder * 60;
        let minutes = minute_numerator / denominator;
        let second_numerator = (minute_numerator % denominator) * 60;
        let divisor = gcd_i64(second_numerator, denominator);
        let seconds_num = second_numerator / divisor;
        let seconds_den = denominator / divisor;
        let components = [degrees, 1, minutes, 1, seconds_num, seconds_den];
        if components
            .iter()
            .any(|component| !(0..=u32::MAX as i64).contains(component))
        {
            return Err(ExifToolError::parse_error(
                "GPSLatitude DMS component does not fit rational64u",
            ));
        }
        let bytes = components
            .into_iter()
            .flat_map(|component| (component as u32).to_ne_bytes())
            .collect();
        return Ok((5, 3, bytes));
    }
    tag_value_to_field(value, hint)
}

fn gcd_i64(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.abs().max(1)
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
    plan_exif_write_with_removals(scan, original_map, desired, &[])
}

/// [`plan_exif_write`], plus the keys the caller asked by name to delete.
/// A removal of a key the reader surfaced is already told by its absence
/// from `desired`; `removed` matters only for an entry the reader surfaced
/// no row for ([`removal_names_rowless_entry`]), which is otherwise carried.
pub(crate) fn plan_exif_write_with_removals(
    scan: &ExifScan,
    original_map: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> Result<WritePlan> {
    plan_exif_write_inner(scan, original_map, desired, removed, true)
}

/// [`plan_exif_write_with_removals`]; `drop_all_when_no_rows` selects the
/// clear-all shortcut (a map with no IFD0/ExifIFD/GPS/EXIF key drops the
/// whole block, raw-carried IFD1 and MakerNote included). A caller for which
/// such a map is not a clear -- the legacy half of a transaction whose
/// generated half still writes -- passes false and gets the per-entry diff.
fn plan_exif_write_inner(
    scan: &ExifScan,
    original_map: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
    drop_all_when_no_rows: bool,
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

    // An added or changed key of a raw-carried class (IFD1, InteropIFD,
    // MakerNotes) is an error, never a silent drop: the Added loop below
    // places new entries in IFD0/ExifIFD/GPS only, and `exif_family_keys`
    // does not visit these prefixes. That covers an edit to a raw-carried
    // InteropIFD entry the reader surfaces under `InteropIFD:<name>` (every
    // Interop row since decision D-1 of slice E-1) and to an IFD1 entry, and
    // a tag new to either directory, all of which pinned ExifTool 13.59
    // writes (`-IFD1:PanasonicTitle=x` creates IFD1 when there is none).
    //
    // It runs before the drop-all shortcut below: a map whose only EXIF rows
    // are IFD1's took that shortcut, so `-IFD1:Compression=1` on an
    // IFD1-only block deleted the whole carrier and a new IFD1 tag on a file
    // with no EXIF created nothing, both reported as success.
    if let Some(key) = desired.iter().find_map(|(key, value)| {
        (is_carried_only_key(key) && original_map.get(key) != Some(value)).then_some(key)
    }) {
        return Err(carried_only_edit_refused(key));
    }

    // Group-wide removals (`<group>:All`) expand to that group's entries
    // (see [`GroupRemoval`]); `IFD0:All`/`EXIF:All` remove the whole block.
    // Before, none was expanded: every one was reported as success and the
    // block left as it was.
    let groups: Vec<GroupRemoval> = removed
        .iter()
        .filter_map(|key| group_removal(key))
        .collect();
    if groups.contains(&GroupRemoval::Carrier) {
        // Delete first, then set (pinned ExifTool 13.59: `-EXIF:All=
        // -Make=x` leaves a block holding Make): the rows this write sets
        // go into a fresh block, planned exactly as a set on a file with no
        // EXIF is. Before, the empty plan dropped them and the write
        // reported success. (A block created so carries no mandatory
        // entries -- YCbCrPositioning, ExifVersion... -- yet; that belongs
        // to the mandatory-entries work, staging/beta1/exififd-mandatory.)
        let sets = requested_sets(original_map, desired);
        if sets.is_empty() {
            return Ok(plan);
        }
        return plan_exif_write_inner(
            &fresh_scan(scan.byte_order),
            &MetadataMap::new(),
            &sets,
            &[],
            false,
        );
    }

    // A named removal of a raw-carried entry is refused too. The PNG reader
    // surfaces no IFD1 row, so `remove_tag("IFD1:Compression")` reaches the
    // planner only through `removed`: neither the check above nor the
    // per-entry loop (which compares `original_map` with `desired`) saw it,
    // the entry was carried and the deletion reported success. Pinned
    // ExifTool 13.59 deletes IFD1 and Interop entries (pruning directories
    // left empty or holding only mandatory entries); this writer cannot, so
    // it refuses instead of doing nothing.
    if let Some(key) = removed.iter().find(|key| {
        group_removal(key).is_none() && removal_names_carried_entry(key, scan, original_map)
    }) {
        return Err(ExifToolError::unsupported_format(format!(
            "Removing tag '{}' is not yet supported: it belongs to an \
             unsurfaced IFD class (InteropIFD/IFD1/MakerNote) that this \
             writer always raw-carries",
            key
        )));
    }

    // clear_all_metadata semantics: no EXIF-family keys desired -> drop all.
    // Only for a map without EXIF rows and without named removals: a named
    // removal of the last ordinary row (`-ExifIFD:ISO=`) is a deletion, not
    // a clear, and must carry the rest (an IFD1 thumbnail) as ExifTool does.
    if drop_all_when_no_rows && removed.is_empty() && exif_family_keys(desired).is_empty() {
        return Ok(plan);
    }

    // Everything past this point re-lays the block out. A pointer the
    // serializer does not model would be written back with its old offset
    // and the bytes it locates left behind -- a dangling SubIFDs pointer,
    // reported as success -- so such a block is refused (fail closed). The
    // in-place writers, which never move existing bytes, still edit it.
    if let Some(entry) = unmodelled_pointer(scan) {
        return Err(ExifToolError::unsupported_format(format!(
            "Cannot rewrite this EXIF block: {} tag 0x{:04X} locates data this \
             writer does not relocate (a SubIFD or offset pointer), and \
             re-laying the block out would leave it dangling",
            entry.ifd.prefix(),
            entry.tag_id
        )));
    }
    // Likewise a directory chain past IFD1 (IFD2 and on: a Leica JPEG's
    // PreviewImage IFD). The serializer writes IFD1 with a zero next-IFD
    // pointer, so the chain and the data it locates were dropped and the
    // write reported success. Pinned ExifTool 13.59 rewrites every IFD of
    // the chain and keeps it; `IFD1:All` alone deletes it (with IFD1).
    if let Some(next) = scan.ifd1_next
        && !groups.contains(&GroupRemoval::Ifd1)
    {
        return Err(ExifToolError::unsupported_format(format!(
            "Cannot rewrite this EXIF block: IFD1 links to a further directory \
             (IFD2, next-IFD offset {next}) that this writer does not relocate, and \
             re-laying the block out would drop it"
        )));
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
    // A named removal by the family-0 alias `EXIF:<name>` deletes the entry
    // of that name in whichever IFD0/ExifIFD/GPS holds it, as pinned ExifTool
    // 13.59 does (`-EXIF:Make=` deletes IFD0:Make): the per-entry loop below
    // deletes an entry whose reader key is gone from `desired`, so the native
    // key is taken out of it. (An IFD1/InteropIFD entry of that name was
    // refused above.)
    for name in removed.iter().filter_map(|key| key.strip_prefix("EXIF:")) {
        if name.eq_ignore_ascii_case("all") {
            continue;
        }
        for entry in &scan.entries {
            if !matches!(entry.ifd, IfdKind::Ifd0 | IfdKind::ExifIfd | IfdKind::Gps) {
                continue;
            }
            let native = lookup_tag_name(entry.tag_id, entry.ifd.prefix());
            if native.split_once(':').is_some_and(|(_, n)| n == name) {
                desired.remove(&native);
            }
        }
    }
    // Exif.pm 0x8298 PrintConvInv stores photographer/editor notices with an
    // internal NUL; the generic string serializer supplies the final NUL.
    for key in ["IFD0:Copyright", "EXIF:Copyright"] {
        if let Some(TagValue::String(value)) = desired.get(key).cloned()
            && let Some(separator) = value.find(['\n', '\r'])
        {
            let photographer = value[..separator].trim_end();
            let editor = value[separator..]
                .trim_start_matches(['\n', '\r'])
                .trim_start();
            let stored = if editor.is_empty() {
                photographer.to_string()
            } else {
                format!(
                    "{}\0{editor}",
                    if photographer.is_empty() {
                        " "
                    } else {
                        photographer
                    }
                )
            };
            desired.insert(key, TagValue::new_string(stored));
        }
    }
    // GPS.pm (ExifTool 13.59) maps GPSLatitudeRef's one-byte N/S codes to
    // display values. Restore the declared raw code before serialization.
    if let Some(TagValue::String(value)) = desired.get("GPS:GPSLatitudeRef") {
        let raw = match value.as_str() {
            "North" => Some("N"),
            "South" => Some("S"),
            _ => None,
        };
        if let Some(raw) = raw {
            desired.insert("GPS:GPSLatitudeRef", TagValue::new_string(raw));
        }
    }
    // GPS.pm (ExifTool 13.59) stores GPSDestLongitudeRef as E/W while
    // exposing East/West through PrintConv. Restore the raw code on writes.
    if let Some(TagValue::String(value)) = desired.get("GPS:GPSDestLongitudeRef") {
        let raw = match value.as_str() {
            "East" => Some("E"),
            "West" => Some("W"),
            _ => None,
        };
        if let Some(raw) = raw {
            desired.insert("GPS:GPSDestLongitudeRef", TagValue::new_string(raw));
        }
    }
    // GPS.pm (ExifTool 13.59) stores GPSLongitudeRef as E/W while exposing
    // East/West through PrintConv. Restore the declared raw code on writes.
    if let Some(TagValue::String(value)) = desired.get("GPS:GPSLongitudeRef") {
        let raw = match value.as_str() {
            "East" => Some("E"),
            "West" => Some("W"),
            _ => None,
        };
        if let Some(raw) = raw {
            desired.insert("GPS:GPSLongitudeRef", TagValue::new_string(raw));
        }
    }
    // GPS.pm (ExifTool 13.59) declares GPSDestBearingRef's PrintConv as
    // M => "Magnetic North", T => "True North". The CLI supplies that
    // display value, but TIFF stores the one-byte code; invert this one
    // declared conversion before the generic string serializer sees it.
    if let Some(TagValue::String(value)) = desired.get("GPS:GPSDestBearingRef") {
        let raw = match value.as_str() {
            "Magnetic North" => Some("M"),
            "True North" => Some("T"),
            _ => None,
        };
        if let Some(raw) = raw {
            desired.insert("GPS:GPSDestBearingRef", TagValue::new_string(raw));
        }
    }
    // GPS.pm uses the same direction-reference PrintConv for GPSTrackRef.
    // Restore its one-byte stored code when the CLI supplies the display label.
    if let Some(TagValue::String(value)) = desired.get("GPS:GPSTrackRef") {
        let raw = match value.as_str() {
            "Magnetic North" => Some("M"),
            "True North" => Some("T"),
            _ => None,
        };
        if let Some(raw) = raw {
            desired.insert("GPS:GPSTrackRef", TagValue::new_string(raw));
        }
    }
    if let Some(TagValue::String(value)) = desired.get("GPS:GPSImgDirectionRef") {
        let raw = match value.as_str() {
            "Magnetic North" => Some("M"),
            "True North" => Some("T"),
            _ => None,
        };
        if let Some(raw) = raw {
            desired.insert("GPS:GPSImgDirectionRef", TagValue::new_string(raw));
        }
    }
    if let Some(TagValue::String(value)) = desired.get("GPS:GPSSpeedRef") {
        let raw = match value.as_str() {
            "km/h" => Some("K"),
            "mph" => Some("M"),
            "knots" => Some("N"),
            _ => None,
        };
        if let Some(raw) = raw {
            desired.insert("GPS:GPSSpeedRef", TagValue::new_string(raw));
        }
    }
    for entry in &scan.entries {
        if matches!(entry.ifd, IfdKind::Interop | IfdKind::Ifd1) || entry.tag_id == MAKERNOTE {
            continue;
        }
        // A borrowed name folds nothing: `EXIF:<name>` is the add path's,
        // as before the engine named the entry.
        let native_key = match surfaced_reader_key(entry, original_map) {
            ReaderKey::Owned(key) => key,
            ReaderKey::Borrowed(_) => lookup_tag_name(entry.tag_id, entry.ifd.prefix()),
        };
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
    // `(EXIF:<name>, InteropIFD:<name>)` for every raw-carried Interop entry
    // with a DCF name; see the Added loop.
    let mut interop_aliases: Vec<(String, String)> = Vec::new();
    // Every entry placed into the plan, with the value it stands for, so the
    // Added loop can tell a redundant duplicate from a dropped edit.
    let mut placed: Vec<PlacedEntry> = Vec::new();
    // Engine names borrowed by a carried entry (`ReaderKey::Borrowed`).
    let mut borrowed_keys: Vec<String> = Vec::new();

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
            let group_deleted = match entry.ifd {
                IfdKind::Interop => groups
                    .iter()
                    .any(|g| matches!(g, GroupRemoval::Interop | GroupRemoval::ExifIfd)),
                IfdKind::Ifd1 => groups.contains(&GroupRemoval::Ifd1),
                _ => groups
                    .iter()
                    .any(|g| matches!(g, GroupRemoval::MakerNotes | GroupRemoval::ExifIfd)),
            };
            for reader_key in &reader_keys {
                if !group_deleted
                    && original_map.contains_key(reader_key)
                    && !desired.contains_key(reader_key)
                {
                    return Err(ExifToolError::unsupported_format(format!(
                        "Removing tag '{}' is not yet supported: it belongs to an \
                         unsurfaced IFD class (InteropIFD/IFD1/MakerNote) that this \
                         writer always raw-carries",
                        reader_key
                    )));
                }
            }
            if entry.ifd == IfdKind::Interop
                && let [native, alias] = reader_keys.as_slice()
            {
                interop_aliases.push((alias.clone(), native.clone()));
            }
            carried_reader_keys.extend(reader_keys);
            placed.push(PlacedEntry {
                ifd: entry.ifd,
                tag_id: entry.tag_id,
                key: None,
                value: None,
                rowless: None,
            });
            bucket(&mut plan, carry);
            continue;
        }

        let key = match surfaced_reader_key(entry, original_map) {
            ReaderKey::Owned(key) => key,
            ReaderKey::Borrowed(key) => {
                // Carried raw; an edit to the name goes to the add path.
                borrowed_keys.push(key);
                placed.push(PlacedEntry {
                    ifd: entry.ifd,
                    tag_id: entry.tag_id,
                    key: None,
                    value: None,
                    rowless: None,
                });
                bucket(&mut plan, carry);
                continue;
            }
        };
        let Some(original_value) = original_map.get(&key) else {
            // Reader didn't surface this entry: never drop what it hides --
            // unless the caller named it for deletion (`removed`), which is
            // what the key's absence from `desired` meant while it had a row.
            if removed
                .iter()
                .any(|k| removal_names_rowless_entry(k, entry.ifd, entry.tag_id, original_map))
            {
                continue;
            }
            let slot = match entry.ifd {
                IfdKind::ExifIfd => plan.exif_ifd.len(),
                IfdKind::Gps => plan.gps.len(),
                _ => plan.ifd0.len(),
            };
            placed.push(PlacedEntry {
                ifd: entry.ifd,
                tag_id: entry.tag_id,
                key: None,
                value: None,
                rowless: Some((slot, entry.field_type)),
            });
            bucket(&mut plan, carry);
            continue;
        };
        let Some(desired_value) = desired.get(&key) else {
            continue; // removal by absence
        };
        consumed_keys.push(key.clone());
        placed.push(PlacedEntry {
            ifd: entry.ifd,
            tag_id: entry.tag_id,
            key: Some(key.clone()),
            value: Some(desired_value.clone()),
            rowless: None,
        });
        if desired_value == original_value {
            bucket(&mut plan, carry);
            continue;
        }

        // Changed: strict validation, then true-typed serialization
        validate_changed(&key, desired_value)?;
        let (ft, count, bytes) =
            tag_value_to_field_for_key(&key, desired_value, Some(entry.field_type))?;
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
        // `EXIF:<name>` for a raw-carried Interop DCF entry is that entry
        // under its family-0 spelling (pinned ExifTool 13.59 writes
        // `-EXIF:InteropIndex=R03` to [InteropIFD]). Since decision D-1 of
        // slice E-1 the reader surfaces it only as `InteropIFD:<name>`, so
        // the carried check below no longer finds `EXIF:<name>` in
        // `original_map`, and the key would fall through to the add path and
        // plant a stray tag in IFD0 while the real entry stayed untouched.
        // Compare it with the row the reader surfaced under either spelling
        // instead: unchanged is the carry-over, anything else is refused.
        if let Some((_, native)) = interop_aliases.iter().find(|(alias, _)| *alias == key) {
            let value = desired.get(&key).unwrap();
            if original_map.get(&key).or_else(|| original_map.get(native)) == Some(value) {
                continue;
            }
            return Err(ExifToolError::unsupported_format(format!(
                "Editing tag '{}' is not yet supported: it belongs to an \
                 unsurfaced IFD class (InteropIFD/IFD1/MakerNote) that this \
                 writer always raw-carries",
                key
            )));
        }
        // Keys whose physical entry lives in an always-carried IFD class
        // (InteropIFD, IFD1, MakerNote — Design Rule: "unsurfaced classes")
        // are carried byte-for-byte above without ever being diffed against
        // `desired`. The reader can still surface some of them under a
        // metadata-map key (e.g. "InteropIFD:InteropIndex", or a MakerNote blob's
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
        // A borrowed engine name with its reader value is the carried entry
        // itself, unchanged: nothing to add.
        if borrowed_keys.iter().any(|k| *k == key) && original_map.get(&key) == Some(value) {
            continue;
        }
        if requires_subifd_write(&key) {
            return Err(ExifToolError::unsupported_format(format!(
                "Cannot write tag '{}': it requires a SubIFD, which this writer does not create or edit",
                key
            )));
        }
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
        // Route by prefix; "EXIF:" keys land in IFD0 (compat with the old writer).
        // Guard against duplicate tag ids: aliased keys (e.g. "IFD0:Make" and
        // "EXIF:Make") resolve to the same numeric tag id via get_tag_descriptor's
        // prefix normalization but are distinct MetadataMap keys, so consumed_keys
        // (tracked by literal key string) cannot catch the collision.
        //
        // A duplicate cannot be emitted -- one IFD record per tag id -- but it
        // must not be discarded in silence either, which is what this guard used
        // to do: `oxidex -ExifIFD:ExposureBiasValue=-0.5` (0x9204, which the
        // reader surfaces as ExposureCompensation) reported "1 image files
        // updated" and left the tag untouched. Skip only when the value already
        // planned for that id is the same one; otherwise the caller's edit is
        // being dropped, so refuse.
        //
        // An "EXIF:"-prefixed key names the tag *family*, not a physical IFD, so
        // its entry may already have been placed in any of the three writable
        // IFDs -- typically by the alias fold above, which routes the edit to the
        // native key. Restricting the collision search to IFD0 (the fallback this
        // key routes to) missed exactly that case and appended a second record
        // for the same tag id in a different IFD: on main,
        // `-EXIF:ExposureTime=1/250` left 0x829A in both IFD0 and ExifIFD, and
        // ExifTool then reports the file as carrying two ExposureTime tags.
        let target = if key.starts_with("ExifIFD:") {
            IfdKind::ExifIfd
        } else if key.starts_with("GPS:") {
            IfdKind::Gps
        } else {
            IfdKind::Ifd0
        };
        //
        // An entry the reader surfaced no row for (`PlacedEntry::rowless`) is
        // not a second name: the key is the caller's edit of that very entry,
        // which it replaces in its slot, serialized with the entry's own field
        // type -- the changed path's bytes while the reader surfaced it. Slice
        // E-2's D-2 and D-3 took the rows of DJI_XT2.jpg's 0x02bc
        // ApplicationNotes and SamsungGT-B2710.jpg's 0x9205 MaxApertureValue
        // away; refusing here turned `-ExifIFD:ApplicationNotes=abc`, which
        // wrote as ExifTool writes, into an error.
        let family_alias = key.starts_with("EXIF:");
        if let Some(dup) = placed.iter_mut().find(|p| {
            p.tag_id == tag_id
                && (p.ifd == target
                    || (family_alias
                        && matches!(p.ifd, IfdKind::Ifd0 | IfdKind::ExifIfd | IfdKind::Gps)))
        }) {
            if dup.value.as_ref() == Some(value) {
                continue; // same value already planned under another spelling
            }
            if let Some((slot, field_type)) = dup.rowless {
                let (ft, count, bytes) = tag_value_to_field_for_key(&key, value, Some(field_type))?;
                let replaced = OutEntry {
                    tag_id,
                    field_type: ft,
                    count,
                    value: bytes,
                    native_endian: true,
                };
                match dup.ifd {
                    IfdKind::ExifIfd => plan.exif_ifd[slot] = replaced,
                    IfdKind::Gps => plan.gps[slot] = replaced,
                    _ => plan.ifd0[slot] = replaced,
                }
                dup.key = Some(key.clone());
                dup.value = Some(value.clone());
                dup.rowless = None;
                continue;
            }
            return Err(ExifToolError::unsupported_format(match &dup.key {
                Some(placed_key) => format!(
                    "Cannot write tag '{}': it resolves to {} tag 0x{:04X}, which is \
                     already being written as '{}'. Two names for one tag id cannot \
                     both be stored; write the tag under a single name.",
                    key,
                    dup.ifd.prefix(),
                    tag_id,
                    placed_key,
                ),
                None => format!(
                    "Cannot write tag '{}': it resolves to {} tag 0x{:04X}, an entry \
                     of a class this writer always carries unedited (MakerNote, \
                     InteropIFD, IFD1)",
                    key,
                    dup.ifd.prefix(),
                    tag_id,
                ),
            }));
        }
        // A tag being created has no existing entry to take a width from, so
        // the declared type is the only thing that can tell FLOAT from DOUBLE.
        let (ft, count, bytes) =
            tag_value_to_field_for_key(&key, value, declared_ieee_field_type(&key))?;
        let out = OutEntry {
            tag_id,
            field_type: ft,
            count,
            value: bytes,
            native_endian: true,
        };
        placed.push(PlacedEntry {
            ifd: target,
            tag_id,
            key: Some(key.clone()),
            value: Some(value.clone()),
            rowless: None,
        });
        match target {
            IfdKind::ExifIfd => plan.exif_ifd.push(out),
            IfdKind::Gps => plan.gps.push(out),
            _ => plan.ifd0.push(out),
        }
    }

    // GPS.pm (ExifTool 13.59) requires GPSVersionID in every GPS IFD. A file
    // without a GPS IFD gains one when a covered tag is added, so emit
    // ExifTool's declared default version alongside it. An existing GPS IFD
    // is left as it is, version entry or not: `WriteExif` adds mandatory
    // entries only to a directory it creates (WriteExif.pl 13.59:714-719),
    // and an iPhone GPS IFD without GPSVersionID gained one on every
    // unrelated edit.
    let gps_existed = scan.entries.iter().any(|entry| entry.ifd == IfdKind::Gps);
    if !gps_existed
        && plan.gps.iter().any(|entry| {
            matches!(
                entry.tag_id,
                0x000d | 0x000f | 0x0014 | 0x0018 | 0x001d | 0x001f
            )
        })
        && !plan.gps.iter().any(|entry| entry.tag_id == 0x0000)
    {
        plan.gps.push(OutEntry {
            tag_id: 0x0000,
            field_type: 1,
            count: 4,
            value: vec![2, 3, 0, 0],
            native_endian: false,
        });
    }

    // Group-wide removals: drop the named directories wholesale (the
    // serializer omits an empty directory and its pointer) -- but for the
    // entries this same write sets there: delete first, then set, as pinned
    // ExifTool 13.59 does (`-ExifIFD:All= -ExifIFD:ISO=200` leaves ExifIFD
    // holding ISO). A directory created so carries no mandatory entries yet
    // (staging/beta1/exififd-mandatory).
    let set_addresses: Vec<(IfdKind, u16)> = requested_sets(original_map, desired)
        .keys()
        .flat_map(|key| key_addresses(key))
        .collect();
    let keep = |ifd: IfdKind| {
        let set_addresses = &set_addresses;
        move |entry: &OutEntry| set_addresses.contains(&(ifd, entry.tag_id))
    };
    for group in &groups {
        match group {
            GroupRemoval::Carrier => unreachable!("returned above"),
            GroupRemoval::ExifIfd => {
                plan.exif_ifd.retain(keep(IfdKind::ExifIfd));
                plan.interop.retain(keep(IfdKind::Interop));
                plan.makernote_pin = None;
            }
            GroupRemoval::Gps => plan.gps.retain(keep(IfdKind::Gps)),
            GroupRemoval::Ifd1 => {
                plan.ifd1.retain(keep(IfdKind::Ifd1));
                plan.thumbnail = None;
            }
            GroupRemoval::Interop => plan.interop.retain(keep(IfdKind::Interop)),
            GroupRemoval::MakerNotes => {
                if makernote_in_makernotes_group(scan, original_map) {
                    plan.exif_ifd.retain(|entry| entry.tag_id != MAKERNOTE);
                    plan.makernote_pin = None;
                }
            }
        }
    }

    Ok(plan)
}

/// Strict validation for values the caller changed or added — identical
/// policy to write_metadata's PHASE 1 (reliable type match, else intrinsics).
pub(crate) fn validate_changed(key: &str, value: &TagValue) -> Result<()> {
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
pub(crate) fn descriptor_tag_id(descriptor: &crate::core::TagDescriptor) -> Option<u16> {
    match &descriptor.tag_id {
        crate::core::TagId::Numeric(id) => Some(*id),
        crate::core::TagId::Named(_) => None,
    }
}

/// ExifTool 13.59 Exif.pm declares tag 0xC61A (BlackLevel) as a protected,
/// variable-count rational64u value in a SubIFD. The surgical writers preserve
/// SubIFDs but deliberately do not create or rewrite them, so routing a new
/// value through their IFD0 fallback would produce a semantically wrong file.
pub(crate) fn requires_subifd_write(key: &str) -> bool {
    key == "EXIF:BlackLevel"
}

/// Walks IFD0 (and ExifIFD, GPS, InteropIFD, IFD1) and returns every entry
/// with its raw value bytes. Pointer tags are consumed structurally, not
/// returned. Corrupt sub-structures degrade gracefully: an out-of-bounds
/// IFD offset or value offset skips that IFD/entry rather than erroring.
pub fn scan_exif_entries(tiff: &[u8]) -> Result<ExifScan> {
    scan_entries_with_magics(tiff, EXIF_BLOCK_MAGICS)
}

/// The TIFF magic an EXIF block (JPEG APP1, PNG eXIf) carries.
pub(crate) const EXIF_BLOCK_MAGICS: &[u16] = &[42];

/// [`scan_exif_entries`] accepting the header magics `magics` -- for a
/// TIFF-structured file, the set its writer walks (42, and 85 for RW2).
pub(crate) fn scan_entries_with_magics(tiff: &[u8], magics: &[u16]) -> Result<ExifScan> {
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
    if !magics.contains(&read_u16(&tiff[2..4], byte_order)) {
        return Err(ExifToolError::parse_error(
            "Invalid TIFF magic number in EXIF data",
        ));
    }

    let mut scan = ExifScan {
        byte_order,
        entries: Vec::new(),
        thumbnail: None,
        makernote_offset: None,
        ifd1_next: None,
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
        scan.ifd1_next = ifd1.next_ifd;
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

/// The first value an IFD record holds inline (`value` is its last 4
/// bytes), decoded by TIFF type as the reader's `read_unsigned_field` does:
/// BYTE, SHORT, LONG, SLONG or IFD. Any other type keeps the former 4-byte
/// reading. A pointer or length stored as a SHORT occupies the first two
/// bytes of the field, so reading all four as a LONG is right only in
/// little-endian order, and only by luck.
pub(crate) fn inline_unsigned(
    field_type: u16,
    count: u32,
    value: &[u8],
    order: ByteOrder,
) -> usize {
    match (field_type, count) {
        (1, 1..) => value[0] as usize,
        (3, 1..) => read_u16(&value[0..2], order) as usize,
        _ => read_u32(&value[0..4], order) as usize,
    }
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
        // The out-of-line offset of a value longer than 4 bytes is always a
        // 4-byte offset; a pointer or length held inline is decoded by its
        // TIFF type (`inline_unsigned`): a big-endian SHORT
        // JPEGInterchangeFormat `00 dc 00 00` is 0xdc, not 0x00dc0000.
        let value_or_offset = read_u32(&entry[8..12], byte_order) as usize;
        let inline = inline_unsigned(field_type, count, &entry[8..12], byte_order);

        // Structural pointers: record and continue (never stored as entries)
        match (which, tag_id) {
            (IfdKind::Ifd0, EXIF_IFD_POINTER) => {
                result.exif_pointer = Some(inline);
                continue;
            }
            (IfdKind::Ifd0, GPS_IFD_POINTER) => {
                result.gps_pointer = Some(inline);
                continue;
            }
            (IfdKind::ExifIfd, INTEROP_POINTER) => {
                result.interop_pointer = Some(inline);
                continue;
            }
            (IfdKind::Ifd1, THUMBNAIL_OFFSET) => {
                result.thumb_offset = Some(inline);
                continue;
            }
            (IfdKind::Ifd1, THUMBNAIL_LENGTH) => {
                result.thumb_length = Some(inline);
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
    if matches!(which, IfdKind::Ifd0 | IfdKind::Ifd1) && next_at + 4 <= tiff.len() {
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
    native_to_byte_order(entry.field_type, &entry.value, bo)
}

/// Re-encodes `tag_value_to_field`'s native-endian placeholder bytes into
/// `bo`, per TIFF element. A no-op for 1-byte element types (ASCII, BYTE,
/// UNDEFINED), which are endian-neutral.
pub(crate) fn native_to_byte_order(field_type: u16, value: &[u8], bo: ByteOrder) -> Vec<u8> {
    let elem = type_size(field_type);
    if elem == 1 {
        return value.to_vec();
    }
    // Elements inside RATIONAL/SRATIONAL are two 4-byte halves
    let unit = match field_type {
        5 | 10 => 4,
        _ => elem,
    };
    let mut out = Vec::with_capacity(value.len());
    for chunk in value.chunks(unit) {
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
    // A thumbnail is surviving content: an IFD1 holding only the
    // JPEGInterchangeFormat/Length pair (which the scanner moves into
    // `plan.thumbnail`) is emitted with those pointers synthesized below.
    let has_entries = !(plan.ifd0.is_empty()
        && plan.exif_ifd.is_empty()
        && plan.gps.is_empty()
        && plan.interop.is_empty()
        && plan.ifd1.is_empty())
        || plan.thumbnail.is_some();
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
pub(crate) struct SliceReader<'a>(pub(crate) &'a [u8]);

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
    rewrite_jpeg_exif_with_removals(file_bytes, desired, &[])
}

/// [`rewrite_jpeg_exif`], plus the keys the caller asked by name to delete
/// ([`plan_exif_write_with_removals`]).
pub(crate) fn rewrite_jpeg_exif_with_removals(
    file_bytes: &[u8],
    desired: &MetadataMap,
    removed: &[String],
) -> Result<Vec<u8>> {
    // Locate the original EXIF TIFF slice, if any
    let tiff: Option<Vec<u8>> = jpeg_exif_payload(file_bytes)?;

    // The exact reader the diff must mirror: parse the whole JPEG the same way
    // read_metadata does (includes tag-name normalization).
    // `ReadOptions::default_full_listing()` (non-extended, nothing
    // specifically requested) is correct here regardless: the
    // undecoded-MakerNote hex-fallback key (`ExifIFD:0x927C`) this diff
    // depends on is inserted unconditionally at the source -- Step 21 only
    // hides it at the CLI *display* boundary, never in this internal map --
    // see `core::read_options`'s "Two different gate sites" doc comment.
    let original_map = match &tiff {
        Some(_) => crate::core::operations::parse_jpeg_metadata(
            &SliceReader(file_bytes),
            &crate::core::ReadOptions::default_full_listing(),
        )?,
        None => MetadataMap::new(),
    };
    let tiff_out =
        rewrite_tiff_exif_with_removals(tiff.as_deref(), &original_map, desired, removed)?;
    if tiff_out.is_empty() {
        return Ok(Vec::new());
    }
    let mut segment = Vec::with_capacity(EXIF_IDENTIFIER.len() + tiff_out.len());
    segment.extend_from_slice(EXIF_IDENTIFIER);
    segment.extend_from_slice(&tiff_out);
    Ok(segment)
}

/// The carrier-neutral core of [`rewrite_jpeg_exif_with_removals`]: the new
/// TIFF payload (header onward, no `Exif\0\0`) for an EXIF block whose
/// original payload is `tiff`, preserving everything the caller did not
/// change. `original_map` is the map the reader produced for the file that
/// holds `tiff` (what `desired` was derived from); with no original payload
/// there is nothing to diff and it is not consulted. Returns an empty Vec
/// when the block should be dropped entirely.
///
/// The JPEG APP1 writer and the PNG `eXIf` writer share it: both carry the
/// same TIFF structure, only the framing around it differs.
pub(crate) fn rewrite_tiff_exif_with_removals(
    tiff: Option<&[u8]>,
    original_map: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> Result<Vec<u8>> {
    // `IFD0:All` / `EXIF:All` delete the carrier without reading it; what
    // the same write sets goes into a fresh block.
    if let Some(deleted) = tiff
        && removes_carrier(removed)
    {
        return fresh_block_for_sets(deleted, original_map, desired);
    }
    let empty = MetadataMap::new();
    let (scan, original_map) = match tiff {
        Some(tiff_bytes) => (scan_exif_entries(tiff_bytes)?, original_map),
        None => (
            ExifScan {
                byte_order: ByteOrder::LittleEndian,
                entries: Vec::new(),
                thumbnail: None,
                makernote_offset: None,
                ifd1_next: None,
            },
            &empty,
        ),
    };
    if let Some(tiff) = tiff
        && is_no_op(&scan, original_map, desired, removed)
    {
        return Ok(tiff.to_vec());
    }
    let plan = plan_exif_write_with_removals(&scan, original_map, desired, removed)?;
    serialize_exif(&plan)
}

/// Key prefixes the EXIF writers plan: IFD0/ExifIFD/GPS/IFD1/InteropIFD
/// rows, the family spelling `EXIF:`, and `MakerNotes:`.
fn is_planned_key(key: &str) -> bool {
    [
        "IFD0:",
        "ExifIFD:",
        "GPS:",
        "EXIF:",
        "IFD1:",
        "InteropIFD:",
        "MakerNotes:",
    ]
    .iter()
    .any(|prefix| key.starts_with(prefix))
}

/// Every planned row of `desired` is its `baseline` value and no planned
/// `baseline` row is gone.
fn rows_unchanged(baseline: &MetadataMap, desired: &MetadataMap) -> bool {
    desired
        .iter()
        .filter(|(key, _)| is_planned_key(key))
        .all(|(key, value)| baseline.get(key.as_str()) == Some(value))
        && baseline
            .iter()
            .filter(|(key, _)| is_planned_key(key))
            .all(|(key, _)| desired.contains_key(key.as_str()))
}

/// Whether none of the named `removed` keys names an entry of `scan`: not a
/// reader row, not an entry by tag id (`removal_names_rowless_entry`), not a
/// raw-carried entry (`removal_names_carried_entry`), not an entry by its
/// family-0 `EXIF:` name. A group-wide `<group>:All` names something exactly
/// when the group has content ([`group_has_content`]); with `groups` false
/// group removals are left to another block's judgement.
fn removals_name_nothing(
    scan: &ExifScan,
    original_map: &MetadataMap,
    removed: &[String],
    groups: bool,
) -> bool {
    removed.iter().filter(|key| is_planned_key(key)).all(|key| {
        if let Some(group) = group_removal(key) {
            return !groups || !group_has_content(group, scan, original_map);
        }
        !original_map.contains_key(key.as_str())
            && !removal_names_carried_entry(key, scan, original_map)
            && !scan.entries.iter().any(|entry| {
                removal_names_rowless_entry(key, entry.ifd, entry.tag_id, original_map)
                    || key.strip_prefix("EXIF:").is_some_and(|name| {
                        lookup_tag_name(entry.tag_id, entry.ifd.prefix())
                            .split_once(':')
                            .is_some_and(|(_, n)| n == name)
                    })
            })
    })
}

/// Whether a write changes nothing in the block (see
/// [`exif_request_is_no_op`]): the payload is then returned unchanged.
fn is_no_op(
    scan: &ExifScan,
    original_map: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> bool {
    rows_unchanged(original_map, desired)
        && removals_name_nothing(scan, original_map, removed, true)
        && !drops_empty_carrier(scan, removed)
}

/// Whether the write rewrites an EXIF carrier that holds no entry and no
/// thumbnail, which pinned ExifTool 13.59 then drops: any removal of an
/// EXIF-family key -- one naming nothing too -- rewrites the block, and an
/// empty block is not written back (`-IFD0:Software=` / `-GPS:All=` on a
/// JPEG whose APP1 is a bare empty IFD0: "1 image files updated", the APP1
/// gone, as tip e4edc55c also wrote it byte for byte). b83ec323's up-front
/// no-op check had kept such a block and reported success.
fn drops_empty_carrier(scan: &ExifScan, removed: &[String]) -> bool {
    scan.entries.is_empty()
        && scan.thumbnail.is_none()
        && removed.iter().any(|key| {
            // A name ExifTool does not know rewrites nothing ("not defined").
            is_planned_key(key) && (group_removal(key).is_some() || !key_addresses(key).is_empty())
        })
}

/// Whether a write request is a no-op for a carrier whose EXIF payloads are
/// `blocks` (JPEG APP1s, PNG eXIf chunks, decoded raw EXIF profiles, a
/// TIFF-structured file): every planned row of `desired` is its `baseline`
/// value, no planned row is gone, and no named removal names an entry of a
/// block -- a block no scanner reading `magics` can parse holds nothing a
/// removal could name (the reader surfaced nothing from it either).
///
/// Decided once, up front, before any refusal guard (the raw-profile and
/// multiple-eXIf guards, the unmodelled-pointer guard, the raw-carried
/// guards, the post-write check): `remove_tag` of a tag that is not there
/// succeeds with the file untouched, as it promises and as pinned ExifTool
/// 13.59 leaves it ("not defined" / "unchanged"). A whole-carrier clear is
/// not a no-op question and is decided before this.
///
/// `group_blocks` are the payloads a group-wide `<group>:All` acts on: the
/// EXIF carriers proper. A PNG's raw EXIF profile is not one -- pinned
/// ExifTool 13.59 leaves it under `-EXIF:All=` ("1 image files unchanged")
/// -- so it is in `blocks` only.
///
/// `embedded` marks a JPEG's APP1 blocks, which pinned ExifTool 13.59 drops
/// when a write rewrites one holding no entry ([`drops_empty_carrier`]); it
/// keeps an empty PNG eXIf chunk, and never deletes a TIFF file's IFD0.
pub(crate) fn exif_request_is_no_op(
    blocks: &[&[u8]],
    group_blocks: &[&[u8]],
    magics: &[u16],
    embedded: bool,
    baseline: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> bool {
    if !rows_unchanged(baseline, desired) {
        return false;
    }
    if removed
        .iter()
        .any(|key| is_planned_key(key) && baseline.contains_key(key.as_str()))
    {
        return false;
    }
    blocks
        .iter()
        .all(|block| match scan_entries_with_magics(block, magics) {
            Ok(scan) => removals_name_nothing(&scan, baseline, removed, false),
            Err(_) => true,
        })
        && group_blocks
            .iter()
            .all(|block| match scan_entries_with_magics(block, magics) {
                Ok(scan) => {
                    removed
                        .iter()
                        .filter_map(|key| group_removal(key))
                        .all(|group| !group_has_content(group, &scan, baseline))
                        && !(embedded && drops_empty_carrier(&scan, removed))
                }
                // A carrier no scanner can read is still deleted wholesale by
                // `IFD0:All` / `EXIF:All` (pinned ExifTool 13.59 drops it).
                // One whose only fault is its TIFF magic number ExifTool reads
                // anyway ("Invalid magic number in EXIF TIFF header"), and so
                // drops it too when it is empty: not a no-op, so the write
                // reaches the scanner and is refused, as at tip e4edc55c.
                Err(_) => {
                    !removes_carrier(removed)
                        && !(embedded
                            && scan_ignoring_magic(block)
                                .is_some_and(|scan| drops_empty_carrier(&scan, removed)))
                }
            })
}

/// `block` scanned whatever its TIFF magic number, as pinned ExifTool 13.59
/// reads an APP1 EXIF block (it only warns), when its byte order is sound.
fn scan_ignoring_magic(block: &[u8]) -> Option<ExifScan> {
    let order = match block.get(..2) {
        Some(b"II") => ByteOrder::LittleEndian,
        Some(b"MM") => ByteOrder::BigEndian,
        _ => return None,
    };
    let magic = read_u16(block.get(2..4)?, order);
    scan_entries_with_magics(block, &[magic]).ok()
}

/// One directory of the IFD chain past IFD1: its raw 12-byte records, the
/// out-of-line value bytes each record locates, and the data block a
/// JPEGInterchangeFormat / StripOffsets record locates with its length
/// partner (a Leica IFD2 PreviewImage), each `None` when it lies outside
/// the block.
#[derive(Debug, PartialEq)]
struct ChainIfd {
    records: Vec<[u8; 12]>,
    located: Vec<Option<Vec<u8>>>,
    data: Vec<Option<Vec<u8>>>,
}

/// The directory chain IFD1's next-IFD pointer starts (IFD2 on).
#[derive(Debug, PartialEq)]
struct IfdChain {
    /// IFD1's next-IFD pointer.
    first: u32,
    dirs: Vec<ChainIfd>,
    /// The (start, length) spans, block-relative, that a directory or a
    /// record of the chain locates past the end of the block: in a JPEG, a
    /// Leica PreviewImage stored after the image
    /// ([`verify_chain_data_after_block`]).
    outside: Vec<(usize, usize)>,
}

/// The chain past IFD1 of `tiff`, or `None` when the block has no IFD1 or
/// IFD1's next pointer is zero. Walks at most 64 directories and stops at
/// a repeated one. Compared by its directories' records and located bytes:
/// the tables may move (after a relocated IFD0), their contents may not.
fn ifd_chain_beyond_ifd1(tiff: &[u8], magics: &[u16]) -> Option<IfdChain> {
    let order = match tiff.get(..2)? {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return None,
    };
    if !magics.contains(&read_u16(tiff.get(2..4)?, order)) {
        return None;
    }
    let u16_at = |at: usize| tiff.get(at..at.checked_add(2)?).map(|b| read_u16(b, order));
    let u32_at = |at: usize| tiff.get(at..at.checked_add(4)?).map(|b| read_u32(b, order));
    let next_of = |at: usize| {
        u32_at(
            at.checked_add(2)?
                .checked_add(usize::from(u16_at(at)?) * 12)?,
        )
    };
    let ifd0 = u32_at(4)? as usize;
    let ifd1 = next_of(ifd0)? as usize;
    if ifd1 == 0 {
        return None;
    }
    let first = next_of(ifd1)?;
    if first == 0 {
        return None;
    }
    let mut chain = IfdChain {
        first,
        dirs: Vec::new(),
        outside: Vec::new(),
    };
    let mut seen = vec![ifd0, ifd1];
    let mut at = first as usize;
    while at != 0 && !seen.contains(&at) && chain.dirs.len() < 64 {
        seen.push(at);
        let Some(count) = u16_at(at) else {
            chain.outside.push((at, 2));
            break;
        };
        let mut dir = ChainIfd {
            records: Vec::new(),
            located: Vec::new(),
            data: Vec::new(),
        };
        let mut locate = |start: usize, len: usize| {
            let bytes = tiff
                .get(start..start.saturating_add(len))
                .map(<[u8]>::to_vec);
            if bytes.is_none() {
                chain.outside.push((start, len));
            }
            bytes
        };
        let mut values: Vec<(u16, usize)> = Vec::new();
        for i in 0..usize::from(count) {
            let Some(record) = tiff.get(at + 2 + i * 12..at + 14 + i * 12) else {
                locate(at + 2 + i * 12, 12);
                break;
            };
            let record: [u8; 12] = record.try_into().expect("12-byte record");
            let tag = read_u16(&record[0..2], order);
            let count = read_u32(&record[4..8], order) as usize;
            let size = type_size(read_u16(&record[2..4], order)).saturating_mul(count);
            let value = read_u32(&record[8..12], order) as usize;
            dir.located
                .push(if size > 4 { locate(value, size) } else { None });
            values.push((
                tag,
                inline_unsigned(
                    read_u16(&record[2..4], order),
                    count as u32,
                    &record[8..12],
                    order,
                ),
            ));
            dir.records.push(record);
        }
        for (offset_tag, length_tag) in [(0x0201u16, 0x0202u16), (0x0111, 0x0117)] {
            let find = |tag: u16| values.iter().find(|(t, _)| *t == tag).map(|(_, v)| *v);
            if let (Some(start), Some(len)) = (find(offset_tag), find(length_tag)) {
                dir.data.push(locate(start, len));
            }
        }
        chain.dirs.push(dir);
        at = next_of(at).unwrap_or(0) as usize;
    }
    Some(chain)
}

/// A JPEG write's check that the directory chain past IFD1 of its EXIF
/// block still locates the data it located past the block's end -- a Leica
/// PreviewImage stored after the image, which the chain addresses relative
/// to the TIFF header. `after_header` is the original JPEG from that header
/// on. When such data is really there and the rewritten block has another
/// length, everything after the block moved while the offsets did not:
/// pinned ExifTool 13.59 re-points them (Writer.pl `PREVIEW_INFO`), this
/// writer does not, so the write is refused. (Offsets past the end of the
/// file -- a truncated sample -- located nothing to begin with.)
pub(crate) fn verify_chain_data_after_block(
    original: &[u8],
    output: &[u8],
    after_header: &[u8],
) -> Result<()> {
    let Some(chain) = ifd_chain_beyond_ifd1(original, EXIF_BLOCK_MAGICS) else {
        return Ok(());
    };
    // Unchanged length moves nothing; no chain left is `IFD1:All`, which
    // deletes it (`verify_exif_write` allows that and nothing else).
    if original.len() == output.len() || ifd_chain_beyond_ifd1(output, EXIF_BLOCK_MAGICS).is_none()
    {
        return Ok(());
    }
    if let Some((start, len)) = chain.outside.iter().find(|(start, len)| {
        start
            .checked_add(*len)
            .is_some_and(|end| *start >= original.len() && end <= after_header.len())
    }) {
        return Err(ExifToolError::unsupported_format(format!(
            "EXIF write verification failed: the directory chain past IFD1 (IFD2) \
             locates {len} bytes at offset {start}, after the EXIF block (a PreviewImage \
             stored after the image), and the rewritten block has another length, \
             which would leave that offset pointing {} bytes off; nothing was written",
            output.len().abs_diff(original.len())
        )));
    }
    Ok(())
}

/// Where a JPEG's first `Exif\0\0` APP1 block's TIFF header starts.
pub(crate) fn jpeg_exif_header_offset(file_bytes: &[u8]) -> Option<usize> {
    let reader = SliceReader(file_bytes);
    parse_segments(&reader)
        .ok()?
        .iter()
        .find(|s| s.is_app1() && s.data.starts_with(EXIF_IDENTIFIER))
        .map(|s| s.offset as usize + 4 + EXIF_IDENTIFIER.len())
}

/// The TIFF payloads of every `Exif\0\0` APP1 block of a JPEG.
pub(crate) fn jpeg_exif_payloads(file_bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let reader = SliceReader(file_bytes);
    let segments = parse_segments(&reader)?;
    Ok(segments
        .iter()
        .filter(|s| s.is_app1() && s.data.starts_with(EXIF_IDENTIFIER))
        .map(|s| s.data[EXIF_IDENTIFIER.len()..].to_vec())
        .collect())
}

/// [`rewrite_tiff_exif_with_removals`] for the legacy half of a transaction
/// whose generated half still writes: a map left with no IFD0/ExifIFD/GPS/
/// EXIF row is not a clear here, so entries are diffed one by one and IFD1,
/// the thumbnail and the MakerNote are carried. Empty means no entry at all
/// is left.
pub(crate) fn rewrite_tiff_exif_keeping_carrier(
    tiff: &[u8],
    original_map: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> Result<Vec<u8>> {
    if removes_carrier(removed) {
        return fresh_block_for_sets(tiff, original_map, desired);
    }
    let scan = scan_exif_entries(tiff)?;
    if is_no_op(&scan, original_map, desired, removed) {
        return Ok(tiff.to_vec());
    }
    let plan = plan_exif_write_inner(&scan, original_map, desired, removed, false)?;
    serialize_exif(&plan)
}

/// `tiff` laid out afresh by the serializer with every entry carried
/// verbatim: IFD0 first at offset 8, then ExifIFD, InteropIFD, GPS, IFD1
/// and the thumbnail, no orphaned bytes. For a block an edit has already
/// re-laid out once (the staged half of a mixed write) and then grown in
/// place, which relocates a grown directory to the end and leaves its old
/// table behind. Carries nothing it cannot place, so it refuses what the
/// serializer refuses (an unmodelled pointer, a chain past IFD1).
pub(crate) fn relayout_exif(tiff: &[u8]) -> Result<Vec<u8>> {
    let scan = scan_exif_entries(tiff)?;
    let empty = MetadataMap::new();
    let plan = plan_exif_write_inner(&scan, &empty, &empty, &[], false)?;
    serialize_exif(&plan)
}

/// The TIFF payload of a JPEG's first `Exif\0\0` APP1 block, if any.
pub(crate) fn jpeg_exif_payload(file_bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let reader = SliceReader(file_bytes);
    let segments = parse_segments(&reader)?;
    Ok(segments
        .iter()
        .find(|s| s.is_app1() && s.data.starts_with(EXIF_IDENTIFIER))
        .map(|s| s.data[EXIF_IDENTIFIER.len()..].to_vec()))
}

/// A JPEG without its Canon CIFF APP0 segments (`(II|MM)....HEAPJPGM`), or
/// `None` when it has none. Pinned ExifTool 13.59 files every CIFF tag under
/// MakerNotes and so drops the whole segment on `MakerNotes:All`
/// (Writer.pl `%excludeGroups` CIFF => MakerNotes; WriteCRW leaves it
/// empty): t/images ExifTool.jpg loses only that segment. Everything after
/// it moves, so an AFCP trailer's absolute offsets are re-based by the
/// shared trailer writer (`jpeg_trailer::rebase_trailer_offsets`), which
/// refuses a trailer it cannot re-base exactly.
pub(crate) fn jpeg_without_ciff(file_bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let reader = SliceReader(file_bytes);
    let Ok(segments) = parse_segments(&reader) else {
        return Ok(None);
    };
    let spans: Vec<(usize, usize)> = segments
        .iter()
        .filter(|s| {
            s.marker == 0xFFE0
                && s.data.len() >= 14
                && matches!(&s.data[..2], b"II" | b"MM")
                && &s.data[6..14] == b"HEAPJPGM"
        })
        .map(|s| (s.offset as usize, s.offset as usize + 4 + s.data.len()))
        .collect();
    let Some(&(_, last_end)) = spans.last() else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(file_bytes.len());
    let mut at = 0;
    for (start, end) in spans {
        out.extend_from_slice(&file_bytes[at..start]);
        at = end;
    }
    out.extend_from_slice(&file_bytes[at..]);
    // Where the entropy-coded data starts (the end of the SOS header, or the
    // EOI marker itself), as `transform_exif` hands it to the trailer writer.
    let scan_from = segments
        .iter()
        .find_map(|s| match s.marker {
            0xFFDA => Some(s.offset as usize + 4 + s.data.len()),
            0xFFD9 => Some(s.offset as usize),
            _ => None,
        })
        .ok_or_else(|| ExifToolError::parse_error("JPEG has no SOS or EOI marker"))?;
    crate::writers::jpeg_trailer::rebase_trailer_offsets(file_bytes, last_end, scan_from, out)
        .map(Some)
}

/// The (IFD, tag id) addresses a write key can name: its group's IFD (every
/// modelled IFD for the family spelling `EXIF:`) and its descriptor's id.
fn key_addresses(key: &str) -> Vec<(IfdKind, u16)> {
    let Some(tag_id) = get_tag_descriptor(key).and_then(descriptor_tag_id) else {
        return Vec::new();
    };
    let ifds: &[IfdKind] = match key.split_once(':').map(|(group, _)| group) {
        Some("IFD0") => &[IfdKind::Ifd0],
        Some("ExifIFD") => &[IfdKind::ExifIfd],
        Some("GPS") => &[IfdKind::Gps],
        Some("IFD1") => &[IfdKind::Ifd1],
        Some("InteropIFD") => &[IfdKind::Interop],
        Some("EXIF") => &[
            IfdKind::Ifd0,
            IfdKind::ExifIfd,
            IfdKind::Gps,
            IfdKind::Interop,
            IfdKind::Ifd1,
        ],
        _ => &[],
    };
    ifds.iter().map(|ifd| (*ifd, tag_id)).collect()
}

/// The family-1 groups of pinned ExifTool 13.59's maker-note tables: every
/// table whose `GROUPS` has family 0 `MakerNotes`, with its family 1 (or
/// module name) and each tag's `Groups => { 1 => ... }` override --
/// `LoadAllTables` over `%allTables`, less `GPS` (a maker-note tag's
/// override that names the EXIF GPS group). The reader keys a decoded
/// maker-note row by these (`Canon:MacroMode`, `Pentax:AEAperture`), and
/// its occurrence's family-0 label is not reliably `MakerNotes` (`Canon`).
const MAKERNOTE_GROUPS: &[&str] = &[
    "AdobeDNG",
    "Apple",
    "CIFF",
    "Canon",
    "CanonCustom",
    "CanonRaw",
    "Casio",
    "DJI",
    "FLIR",
    "FujiFilm",
    "GE",
    "Google",
    "HP",
    "HTC",
    "JVC",
    "KDC_IFD",
    "Kodak",
    "KodakIFD",
    "KyoceraRaw",
    "LeafSubIFD",
    "Leica",
    "MakerNotes",
    "MakerUnknown",
    "Microsoft",
    "Minolta",
    "MinoltaRaw",
    "Motorola",
    "Nikon",
    "NikonCapture",
    "NikonCustom",
    "NikonScan",
    "NikonSettings",
    "Nintendo",
    "Olympus",
    "Panasonic",
    "Pentax",
    "PhaseOne",
    "PreviewIFD",
    "Qualcomm",
    "Reconyx",
    "Ricoh",
    "SR2",
    "SR2DataIFD",
    "SR2SubIFD",
    "Samsung",
    "Sanyo",
    "Sigma",
    "Sony",
    "SonyIDC",
];

/// Whether `key` of `baseline` is a row a maker-note decoder produced: its
/// family-1 group is a maker-note group ([`MAKERNOTE_GROUPS`]) or its
/// occurrence's family-0 group is `MakerNotes`.
fn is_makernote_row(baseline: &MetadataMap, key: &str) -> bool {
    key.split_once(':')
        .is_some_and(|(group, _)| MAKERNOTE_GROUPS.contains(&group))
        || baseline.group0_of(key) == Some("MakerNotes")
}

/// The maker-note rows of `baseline` a write drops from the map without
/// deleting the MakerNote itself: `read_metadata`, `map.remove(
/// "Canon:MacroMode")`, `write_metadata`. A row missing from a map read
/// from the file is a deletion; no writer here can delete one decoded
/// maker-note tag -- the MakerNote is carried byte-for-byte -- so the write
/// used to report success with the tag still in the file. Only removing
/// the whole MakerNote (`MakerNotes:All`, `ExifIFD:All`, `IFD0:All` /
/// `EXIF:All`, as `removed` names them) deletes such rows.
pub(crate) fn dropped_makernote_rows(
    baseline: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> Vec<String> {
    let deletes_makernote = removed.iter().any(|key| {
        matches!(
            group_removal(key),
            Some(GroupRemoval::MakerNotes | GroupRemoval::ExifIfd | GroupRemoval::Carrier)
        )
    });
    if deletes_makernote {
        return Vec::new();
    }
    baseline
        .keys()
        .filter(|key| !desired.contains_key(key.as_str()) && is_makernote_row(baseline, key))
        .cloned()
        .collect()
}

/// The refusal for [`dropped_makernote_rows`]: exit 1, nothing written.
pub(crate) fn refuse_dropped_makernote_rows(dropped: &[String]) -> Result<()> {
    match dropped.first() {
        None => Ok(()),
        Some(key) => Err(ExifToolError::unsupported_format(format!(
            "Deleting '{key}' is not supported: it is decoded from the MakerNote, \
             which this writer carries byte-for-byte and cannot delete one tag of \
             (remove the whole MakerNote with MakerNotes:All instead); nothing was \
             written"
        ))),
    }
}

/// Post-condition for rows a write drops from the map that this writer
/// cannot delete one by one -- a maker-note row, an IFD1 or InteropIFD row:
/// each dropped row must be gone from `output`, i.e. no entry is left at
/// its address (the MakerNote blob for a maker-note row). `verify_exif_write`
/// looks only at the entries a key addresses, and skipped maker-note rows
/// altogether, so a dropped decoded row with its blob kept passed.
pub(crate) fn verify_dropped_rows_gone(
    baseline: &MetadataMap,
    desired: &MetadataMap,
    output: &[u8],
    magics: &[u16],
) -> Result<()> {
    let dropped: Vec<&String> = baseline
        .keys()
        .filter(|key| !desired.contains_key(key.as_str()))
        .filter(|key| {
            is_makernote_row(baseline, key)
                || key.starts_with("IFD1:")
                || key.starts_with("InteropIFD:")
        })
        .collect();
    if dropped.is_empty() || output.is_empty() {
        return Ok(());
    }
    let after = scan_entries_with_magics(output, magics)?;
    let present = |ifd: IfdKind, tag_id: u16| {
        after
            .entries
            .iter()
            .any(|entry| entry.ifd == ifd && entry.tag_id == tag_id)
    };
    for key in dropped {
        let addresses = if is_makernote_row(baseline, key) {
            vec![(IfdKind::ExifIfd, MAKERNOTE)]
        } else {
            key_addresses(key)
        };
        if let Some((ifd, tag_id)) = addresses.into_iter().find(|&(ifd, id)| present(ifd, id)) {
            return Err(ExifToolError::unsupported_format(format!(
                "EXIF write verification failed: '{key}' was dropped from the map but \
                 {} tag 0x{tag_id:04X} is still present; nothing was written",
                ifd.prefix()
            )));
        }
    }
    Ok(())
}

/// Post-condition of an EXIF write, checked on the produced payload before
/// anything is committed (JPEG APP1, PNG eXIf, a TIFF-structured file):
///
/// (a) every original entry the caller removed -- a row of `baseline` gone
///     from `desired`, or named in `removed` -- is absent from `output`,
///     unless another key of `desired` still addresses it;
/// (b) every key `desired` changes against `baseline` has an entry at its
///     resolved address, and that entry is not the original one left as it
///     was (unless the requested value encodes to exactly those bytes).
///
/// Each review round of the surgical writers found another route by which
/// an edit was dropped while success was reported; this closes the class:
/// a mismatch refuses the whole write. Both payloads are read with
/// [`scan_exif_entries`], the scanner the writers plan from.
///
/// `magics` is the TIFF header set the validated writer accepts
/// ([`EXIF_BLOCK_MAGICS`] for an EXIF block, `tiff_surgical::
/// WALKABLE_TIFF_MAGICS` for a TIFF-structured file): the check must read
/// exactly what the writer reads.
pub(crate) fn verify_exif_write(
    original: Option<&[u8]>,
    output: &[u8],
    baseline: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
    magics: &[u16],
) -> Result<()> {
    let refused = |what: String| {
        ExifToolError::unsupported_format(format!(
            "EXIF write verification failed: {what}; nothing was written"
        ))
    };
    let empty = || ExifScan {
        byte_order: ByteOrder::LittleEndian,
        entries: Vec::new(),
        thumbnail: None,
        makernote_offset: None,
        ifd1_next: None,
    };
    let is_exif = |key: &str| {
        ["IFD0:", "ExifIFD:", "GPS:", "EXIF:", "IFD1:", "InteropIFD:"]
            .iter()
            .any(|prefix| key.starts_with(prefix))
    };
    // `IFD0:All` / `EXIF:All` delete the carrier, whatever it held (it is
    // never read): nothing may be left of it but a block built from the
    // tags the same write sets, checked below against an empty original.
    let carrier_deleted = removes_carrier(removed);
    let first_set = desired
        .iter()
        .find(|(key, value)| is_exif(key) && baseline.get(key.as_str()) != Some(value))
        .map(|(key, _)| key.clone());
    if carrier_deleted {
        // An empty output is the carrier gone -- right only when the write
        // sets nothing. A set is checked (b, below) against the fresh block.
        match (output.is_empty(), &first_set) {
            (true, None) => return Ok(()),
            (true, Some(key)) => {
                return Err(refused(format!(
                    "'{key}' was set but the EXIF block was deleted"
                )));
            }
            (false, None) => {
                return Err(refused(
                    "the EXIF block was to be deleted but is still present".to_string(),
                ));
            }
            (false, Some(_)) => {}
        }
    }
    let before = match original {
        Some(tiff) if !tiff.is_empty() && !carrier_deleted => {
            scan_entries_with_magics(tiff, magics)?
        }
        _ => empty(),
    };
    let after = if output.is_empty() {
        empty()
    } else {
        scan_entries_with_magics(output, magics)?
    };
    let find = |scan: &ExifScan, ifd: IfdKind, tag_id: u16| -> Option<RawEntry> {
        scan.entries
            .iter()
            .find(|entry| entry.ifd == ifd && entry.tag_id == tag_id)
            .cloned()
    };
    // Addresses a key being set names: an entry one of them addresses may
    // legitimately remain although another spelling of it went (an alias
    // replacement). An unchanged row keeps nothing alive against a removal.
    let kept: Vec<(IfdKind, u16)> = desired
        .iter()
        .filter(|(key, value)| is_exif(key) && baseline.get(key.as_str()) != Some(value))
        .flat_map(|(key, _)| key_addresses(key))
        .collect();

    // (c) the directory chain past IFD1 (IFD2 and on, with the data its
    // records locate) is still reachable from IFD1 and byte-identical --
    // or, under `IFD1:All`, gone with IFD1, as pinned ExifTool 13.59 drops
    // it. The serializer writes IFD1 with a zero next-IFD pointer, and the
    // other checks never look past IFD1, so a lost IFD2 passed. (Data the
    // chain locates past the block's end, a JPEG's Leica PreviewImage, is
    // the JPEG writer's to check: [`verify_chain_data_after_block`].)
    if !carrier_deleted
        && let Some(chain) = original
            .filter(|tiff| !tiff.is_empty())
            .and_then(|tiff| ifd_chain_beyond_ifd1(tiff, magics))
    {
        let after = ifd_chain_beyond_ifd1(output, magics);
        if removed
            .iter()
            .any(|key| group_removal(key) == Some(GroupRemoval::Ifd1))
        {
            if after.is_some() {
                return Err(refused(
                    "'IFD1:All' was to delete the directory chain past IFD1 but it is \
                     still present"
                        .to_string(),
                ));
            }
        } else if after
            .as_ref()
            .is_none_or(|after| after.dirs != chain.dirs || after.outside != chain.outside)
        {
            return Err(refused(format!(
                "the directory chain past IFD1 ({} director{} from next-IFD offset {}, \
                 IFD2 on) is no longer present byte-identical",
                chain.dirs.len(),
                if chain.dirs.len() == 1 { "y" } else { "ies" },
                chain.first
            )));
        }
    }

    // (a) removals
    for entry in &before.entries {
        let native = lookup_tag_name(entry.tag_id, entry.ifd.prefix());
        let mut keys = carried_class_reader_keys(entry);
        if let Some((_, name)) = native.split_once(':') {
            keys.push(format!("EXIF:{name}"));
        }
        if entry.ifd == IfdKind::ExifIfd
            && let Some(engine) = engine_reader_key(entry.tag_id, baseline)
        {
            keys.push(engine);
        }
        // Removed from the map: a row of this entry gone while none of its
        // other rows is still desired.
        let by_map = keys
            .iter()
            .find(|key| baseline.contains_key(key.as_str()) && !desired.contains_key(key.as_str()))
            .filter(|_| {
                !keys.iter().any(|key| {
                    baseline.contains_key(key.as_str()) && desired.contains_key(key.as_str())
                })
            });
        let by_name = removed.iter().find(|key| {
            keys.iter().any(|k| k == *key)
                || removal_names_rowless_entry(key, entry.ifd, entry.tag_id, baseline)
        });
        let Some(key) = by_map.or(by_name) else {
            continue;
        };
        if kept.contains(&(entry.ifd, entry.tag_id)) {
            continue;
        }
        if find(&after, entry.ifd, entry.tag_id).is_some() {
            return Err(refused(format!(
                "'{key}' was to be deleted but {} tag 0x{:04X} is still present",
                entry.ifd.prefix(),
                entry.tag_id
            )));
        }
    }

    // (a'') group-wide removals: after `<group>:All` nothing the group
    // holds may remain, unless a key being set addresses it (as for (a)).
    for key in removed {
        let Some(group) = group_removal(key) else {
            continue;
        };
        if !group_has_content(group, &before, baseline) {
            continue;
        }
        let in_group = |entry: &RawEntry| match group {
            GroupRemoval::Carrier => true,
            GroupRemoval::ExifIfd => matches!(entry.ifd, IfdKind::ExifIfd | IfdKind::Interop),
            GroupRemoval::Gps => entry.ifd == IfdKind::Gps,
            GroupRemoval::Ifd1 => entry.ifd == IfdKind::Ifd1,
            GroupRemoval::Interop => entry.ifd == IfdKind::Interop,
            GroupRemoval::MakerNotes => entry.ifd == IfdKind::ExifIfd && entry.tag_id == MAKERNOTE,
        };
        let still = after
            .entries
            .iter()
            .find(|entry| in_group(entry) && !kept.contains(&(entry.ifd, entry.tag_id)));
        let thumbnail_left = matches!(group, GroupRemoval::Carrier | GroupRemoval::Ifd1)
            && after.thumbnail.is_some();
        if still.is_some() || thumbnail_left {
            return Err(refused(format!(
                "'{key}' was to delete the group but {} is still present",
                still.map_or("the IFD1 thumbnail".to_string(), |entry| format!(
                    "{} tag 0x{:04X}",
                    entry.ifd.prefix(),
                    entry.tag_id
                ))
            )));
        }
    }

    // (a') the thumbnail: the IFD1 JPEGInterchangeFormat/Length pair is
    // structural (never a scanned entry), so (a) cannot see it go. Any
    // thumbnail the original carries must still be there, byte-identical and
    // reachable through the pair, unless the caller deleted it (a row naming
    // it gone from the map, or a named removal of it or of IFD1/EXIF:All).
    if let Some(thumbnail) = &before.thumbnail {
        let names_thumbnail = |key: &str| {
            let Some((group, name)) = key.split_once(':') else {
                return false;
            };
            (matches!(group, "IFD1" | "EXIF")
                || (group == "IFD0" && name.eq_ignore_ascii_case("all")))
                && (name.eq_ignore_ascii_case("all")
                    || matches!(
                        name,
                        "ThumbnailImage"
                            | "ThumbnailOffset"
                            | "ThumbnailLength"
                            | "JPEGInterchangeFormat"
                            | "JPEGInterchangeFormatLength"
                    ))
        };
        let deleted = removed.iter().any(|key| names_thumbnail(key))
            || baseline
                .iter()
                .any(|(key, _)| names_thumbnail(key) && !desired.contains_key(key.as_str()));
        if !deleted && after.thumbnail.as_ref() != Some(thumbnail) {
            return Err(refused(format!(
                "the {}-byte IFD1 thumbnail was not asked to be deleted but is {}",
                thumbnail.len(),
                if after.thumbnail.is_some() {
                    "changed"
                } else {
                    "gone"
                }
            )));
        }
    }

    // (b) sets
    for (key, value) in desired.iter() {
        if !is_exif(key) || baseline.get(key) == Some(value) {
            continue;
        }
        let addresses = key_addresses(key);
        if addresses.is_empty() {
            continue; // no numeric address to check (the planner resolves or refuses it)
        }
        let Some(written) = addresses
            .iter()
            .find_map(|(ifd, tag_id)| find(&after, *ifd, *tag_id))
        else {
            return Err(refused(format!(
                "'{key}' was set but no entry exists at its address"
            )));
        };
        if let Some(previous) = find(&before, written.ifd, written.tag_id)
            && previous == written
        {
            let expected = tag_value_to_field_for_key(key, value, Some(previous.field_type))
                .ok()
                .map(|(field_type, count, bytes)| {
                    (
                        field_type,
                        count,
                        native_to_byte_order(field_type, &bytes, after.byte_order),
                    )
                });
            if expected != Some((written.field_type, written.count, written.value.clone())) {
                return Err(refused(format!(
                    "'{key}' was set but {} tag 0x{:04X} is unchanged",
                    written.ifd.prefix(),
                    written.tag_id
                )));
            }
        }
    }
    Ok(())
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
    fn plan_encodes_copyright_newline_as_exif_nul_separator() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert(
            "IFD0:Copyright",
            TagValue::new_string("Photographer\nEditor"),
        );

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let copyright = plan
            .ifd0
            .iter()
            .find(|entry| entry.tag_id == 0x8298)
            .unwrap();

        assert_eq!(copyright.field_type, 2);
        assert_eq!(copyright.value, b"Photographer\0Editor\0");
    }

    #[test]
    fn plan_adds_required_gps_version_with_gps_h_positioning_error() {
        // ExifTool 13.59 GPS.pm requires GPSVersionID whenever a GPS IFD is
        // created. Without it, writing this otherwise-correct RATIONAL makes
        // `exiftool -validate` report "Missing required JPEG GPS tag 0x0000".
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert(
            "GPS:GPSHPositioningError",
            TagValue::Rational {
                numerator: 3,
                denominator: 2,
            },
        );

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let version = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x0000)
            .unwrap();
        assert_eq!(version.field_type, 1);
        assert_eq!(version.count, 4);
        assert_eq!(version.value, [2, 3, 0, 0]);
        let accuracy = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x001f)
            .unwrap();
        assert_eq!(accuracy.field_type, 5);
        assert_eq!(accuracy.count, 1);
    }

    #[test]
    fn plan_writes_gps_track_with_required_gps_version() {
        // ExifTool 13.59 requires GPSVersionID whenever GPSTrack creates a
        // GPS IFD; omitting it produces "Missing required JPEG GPS tag 0x0000".
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("GPS:GPSTrack", TagValue::new_rational(3, 2));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let track = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x000f)
            .unwrap();
        assert_eq!(track.field_type, 5);
        assert_eq!(track.count, 1);
        assert!(plan.gps.iter().any(|entry| entry.tag_id == 0x0000));
    }

    #[test]
    fn plan_writes_gps_speed_with_required_gps_version() {
        // ExifTool 13.59 GPS.pm declares GPSSpeed as rational64u. Creating a
        // GPS IFD for it also creates the required GPSVersionID=2.3.0.0.
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("GPS:GPSSpeed", TagValue::new_rational(91, 2));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let speed = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x000d)
            .unwrap();
        assert_eq!(speed.field_type, 5);
        assert_eq!(speed.count, 1);
        assert_eq!(
            speed.value,
            [91_u32.to_ne_bytes(), 2_u32.to_ne_bytes()].concat()
        );
        let version = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x0000)
            .unwrap();
        assert_eq!(version.field_type, 1);
        assert_eq!(version.count, 4);
        assert_eq!(version.value, [2, 3, 0, 0]);
    }

    #[test]
    fn plan_adds_required_gps_version_with_gps_dest_bearing() {
        // GPS.pm 13.59 declares GPSVersionID mandatory. Creating a GPS IFD
        // solely for GPSDestBearing must therefore add version 2.3.0.0 too.
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("GPS:GPSDestBearing", TagValue::new_rational(3, 2));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let bearing = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x0018)
            .unwrap();
        assert_eq!(bearing.field_type, 5);
        assert_eq!(bearing.count, 1);
        assert_eq!(
            bearing.value,
            [3_u32.to_ne_bytes(), 2_u32.to_ne_bytes()].concat()
        );
        let version = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x0000)
            .expect("GPSVersionID is required when GPSDestBearing creates a GPS IFD");
        assert_eq!(version.field_type, 1);
        assert_eq!(version.count, 4);
        assert_eq!(version.value, [2, 3, 0, 0]);
    }

    #[test]
    fn plan_inverts_gps_latitude_ref_display_value_before_serializing() {
        // ExifTool 13.59 GPS.pm maps the raw GPSLatitudeRef code S to the
        // display value "South". The TIFF entry must retain the raw code.
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("GPS:GPSLatitudeRef", TagValue::new_string("South"));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let latitude_ref = plan.gps.iter().find(|e| e.tag_id == 0x0001).unwrap();
        assert_eq!(latitude_ref.field_type, 2);
        assert_eq!(latitude_ref.count, 2);
        assert_eq!(latitude_ref.value, b"S\0");
    }

    #[test]
    fn plan_writes_gps_date_stamp_with_required_gps_version() {
        // ExifTool 13.59 writes GPSDateStamp as ASCII and creates the required
        // GPSVersionID=2.3.0.0 when the tag creates a new GPS IFD.
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("GPS:GPSDateStamp", TagValue::new_string("2024:01:15"));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let date_stamp = plan.gps.iter().find(|e| e.tag_id == 0x001D).unwrap();
        assert_eq!(date_stamp.field_type, 2);
        assert_eq!(date_stamp.count, 11);
        assert_eq!(date_stamp.value, b"2024:01:15\0");
        let version = plan.gps.iter().find(|e| e.tag_id == 0x0000).unwrap();
        assert_eq!(version.field_type, 1);
        assert_eq!(version.count, 4);
        assert_eq!(version.value, [2, 3, 0, 0]);
    }

    #[test]
    fn plan_serializes_gps_dest_latitude_as_three_dms_rationals() {
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert(
            "GPS:GPSDestLatitude",
            TagValue::Rational {
                numerator: 377_749,
                denominator: 10_000,
            },
        );

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let latitude = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x0014)
            .unwrap();
        assert_eq!((latitude.field_type, latitude.count), (5, 3));
        let components: Vec<(u32, u32)> = latitude
            .value
            .chunks_exact(8)
            .map(|chunk| {
                (
                    u32::from_ne_bytes(chunk[..4].try_into().unwrap()),
                    u32::from_ne_bytes(chunk[4..].try_into().unwrap()),
                )
            })
            .collect();
        assert_eq!(components, [(37, 1), (46, 1), (741, 25)]);
        assert!(plan.gps.iter().any(|entry| entry.tag_id == 0x0000));
    }

    #[test]
    fn plan_inverts_gps_longitude_ref_display_value_before_serializing() {
        // GPS.pm 13.59 maps the raw code W to "West". The writer receives
        // that display value from the CLI/read map and must restore W\0.
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("GPS:GPSLongitudeRef", TagValue::new_string("West"));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let longitude_ref = plan.gps.iter().find(|e| e.tag_id == 0x0003).unwrap();
        assert_eq!(longitude_ref.field_type, 2);
        assert_eq!(longitude_ref.count, 2);
        assert_eq!(longitude_ref.value, b"W\0");
    }

    #[test]
    fn plan_inverts_gps_dest_longitude_ref_display_value_before_serializing() {
        // GPS.pm 13.59 maps the raw code W to "West". Store W\0 when the
        // caller supplies the display value read from metadata or the CLI.
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("GPS:GPSDestLongitudeRef", TagValue::new_string("West"));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let longitude_ref = plan.gps.iter().find(|e| e.tag_id == 0x0015).unwrap();
        assert_eq!(longitude_ref.field_type, 2);
        assert_eq!(longitude_ref.count, 2);
        assert_eq!(longitude_ref.value, b"W\0");
    }

    #[test]
    fn plan_inverts_gps_dest_bearing_ref_display_value_before_serializing() {
        // ExifTool 13.59 GPS.pm maps the raw GPSDestBearingRef code M to
        // the display value "Magnetic North". A writer must store M, not the
        // display label, or ExifTool reads it back as an unknown value.
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert(
            "GPS:GPSDestBearingRef",
            TagValue::new_string("Magnetic North"),
        );

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let bearing_ref = plan.gps.iter().find(|e| e.tag_id == 0x0017).unwrap();
        assert_eq!(bearing_ref.field_type, 2);
        assert_eq!(bearing_ref.count, 2);
        assert_eq!(bearing_ref.value, b"M\0");
    }

    #[test]
    fn plan_inverts_gps_track_ref_display_value_before_serializing() {
        // ExifTool 13.59 GPS.pm maps the raw GPSTrackRef code T to the display
        // value "True North". Storing the label makes ExifTool report an
        // unknown value, so the writer must restore the declared raw code.
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("GPS:GPSTrackRef", TagValue::new_string("True North"));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let track_ref = plan.gps.iter().find(|e| e.tag_id == 0x000E).unwrap();
        assert_eq!(track_ref.field_type, 2);
        assert_eq!(track_ref.count, 2);
        assert_eq!(track_ref.value, b"T\0");
    }

    #[test]
    fn plan_inverts_gps_img_direction_ref_display_value_before_serializing() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert(
            "GPS:GPSImgDirectionRef",
            TagValue::new_string("Magnetic North"),
        );

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let image_direction_ref = plan.gps.iter().find(|e| e.tag_id == 0x0010).unwrap();
        assert_eq!(image_direction_ref.field_type, 2);
        assert_eq!(image_direction_ref.count, 2);
        assert_eq!(image_direction_ref.value, b"M\0");
    }

    #[test]
    fn plan_inverts_gps_speed_ref_display_value_before_serializing() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("GPS:GPSSpeedRef", TagValue::new_string("km/h"));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let speed_ref = plan.gps.iter().find(|e| e.tag_id == 0x000c).unwrap();
        assert_eq!(speed_ref.field_type, 2);
        assert_eq!(speed_ref.count, 2);
        assert_eq!(speed_ref.value, b"K\0");
    }

    #[test]
    fn plan_creates_gps_version_id_as_four_bytes() {
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("GPS:GPSVersionID", TagValue::Binary(vec![2, 3, 0, 0]));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let version = plan.gps.iter().find(|e| e.tag_id == 0x0000).unwrap();
        assert_eq!(version.field_type, 1);
        assert_eq!(version.count, 4);
        assert_eq!(version.value, [2, 3, 0, 0]);
    }

    #[test]
    fn plan_writes_exif_version_as_undefined_four_byte_payload() {
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("ExifIFD:ExifVersion", TagValue::Binary(b"0231".to_vec()));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let version = plan
            .exif_ifd
            .iter()
            .find(|entry| entry.tag_id == 0x9000)
            .unwrap();
        assert_eq!(version.field_type, 7);
        assert_eq!(version.count, 4);
        assert_eq!(version.value, b"0231");
    }

    #[test]
    fn plan_rejects_gps_version_id_with_wrong_byte_count() {
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("GPS:GPSVersionID", TagValue::Binary(vec![2, 3, 0]));

        assert!(plan_exif_write(&scan, &original, &desired).is_err());
    }

    #[test]
    fn plan_serializes_gps_latitude_as_three_dms_rationals() {
        // ExifTool 13.59 GPS.pm declares GPSLatitude as rational64u[3] and
        // applies ToDMS as ValueConvInv.  The matrix input 37.7749 therefore
        // becomes 37 degrees, 46 minutes and 29.64 seconds.
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("GPS:GPSLatitude", TagValue::new_rational(377_749, 10_000));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let latitude = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x0002)
            .unwrap();
        assert_eq!(latitude.field_type, 5);
        assert_eq!(latitude.count, 3);
        let expected = [37_u32, 1, 46, 1, 741, 25]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect::<Vec<_>>();
        assert_eq!(latitude.value, expected);
    }

    #[test]
    fn plan_serializes_gps_longitude_as_exif_dms_rationals() {
        // ExifTool 13.59 GPS.pm applies ToDMS before writing GPSLongitude.
        // Its exact representation for 122.4194 is 122/1, 25/1, 246/25.
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("GPS:GPSLongitude", TagValue::new_rational(612_097, 5_000));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let longitude = plan.gps.iter().find(|e| e.tag_id == 0x0004).unwrap();
        let mut expected = Vec::new();
        for (numerator, denominator) in [(122_u32, 1_u32), (25, 1), (246, 25)] {
            expected.extend_from_slice(&numerator.to_ne_bytes());
            expected.extend_from_slice(&denominator.to_ne_bytes());
        }
        assert_eq!(longitude.field_type, 5);
        assert_eq!(longitude.count, 3);
        assert_eq!(longitude.value, expected);
    }

    #[test]
    fn plan_serializes_gps_dest_longitude_as_three_dms_rationals() {
        // ExifTool 13.59 GPS.pm declares GPSDestLongitude as rational64u[3]
        // and applies ToDMS as ValueConvInv. The matrix input 122.4194
        // therefore becomes 122 degrees, 25 minutes and 9.84 seconds.
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert(
            "GPS:GPSDestLongitude",
            TagValue::new_rational(1_224_194, 10_000),
        );

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let longitude = plan
            .gps
            .iter()
            .find(|entry| entry.tag_id == 0x0016)
            .unwrap();
        assert_eq!(longitude.field_type, 5);
        assert_eq!(longitude.count, 3);
        let expected = [122_u32, 1, 25, 1, 246, 25]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect::<Vec<_>>();
        assert_eq!(longitude.value, expected);
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

    /// A JPEG whose ExifIFD holds the SHORT entries `entries`, its TIFF
    /// block, and the reader's map of it.
    fn exif_ifd_shorts_jpeg(entries: &[(u16, u16)]) -> (Vec<u8>, MetadataMap) {
        // TIFF: IFD0 at 8 (ExifOffset -> 26), ExifIFD at 26.
        let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
        tiff.extend(1u16.to_le_bytes());
        tiff.extend([0x69, 0x87, 4, 0, 1, 0, 0, 0]);
        tiff.extend(26u32.to_le_bytes());
        tiff.extend(0u32.to_le_bytes());
        tiff.extend((entries.len() as u16).to_le_bytes());
        for (tag, value) in entries {
            tiff.extend(tag.to_le_bytes());
            tiff.extend(3u16.to_le_bytes());
            tiff.extend(1u32.to_le_bytes());
            tiff.extend(u32::from(*value).to_le_bytes());
        }
        tiff.extend(0u32.to_le_bytes());
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend(((tiff.len() + 8) as u16).to_be_bytes());
        jpeg.extend(b"Exif\0\0");
        jpeg.extend(&tiff);
        jpeg.extend([0xff, 0xd9]);
        let original = crate::core::operations::parse_jpeg_metadata(
            &SliceReader(&jpeg),
            &crate::core::ReadOptions::default_full_listing(),
        )
        .unwrap();
        (tiff, original)
    }

    /// Review finding (E-2, D-2/D-3): an ExifIFD entry the reader surfaces no
    /// row for is still the entry an explicit edit of its name writes.
    /// DJI_XT2.jpg's 0x02bc ApplicationNotes (row-less after D-3) and
    /// SamsungGT-B2710.jpg's 0x9205 MaxApertureValue (withheld after D-2)
    /// were edited by `-ExifIFD:<name>=` before the slice, as ExifTool edits
    /// them; the duplicate-id guard took the carried entry for a second name
    /// and refused. ExposureProgram stands in here: the edit replaces the
    /// entry in its slot with the plan of the surfaced case, and a deletion by
    /// name drops it as the key's absence did while it had a row.
    #[test]
    fn a_rowless_entry_is_replaced_by_an_edit_and_deleted_by_name() {
        let (tiff, surfaced) = exif_ifd_shorts_jpeg(&[(0x8822, 2), (0xa001, 1)]);
        let scan = scan_exif_entries(&tiff).unwrap();
        assert!(surfaced.contains_key("ExifIFD:ExposureProgram"));
        let mut rowless = surfaced.clone();
        rowless.remove("ExifIFD:ExposureProgram");

        let carried = plan_exif_write(&scan, &rowless, &rowless).unwrap();
        assert_eq!(
            carried,
            plan_exif_write(&scan, &surfaced, &surfaced).unwrap()
        );
        let edit = |original: &MetadataMap, keys: &[(&str, i64)]| {
            let mut desired = original.clone();
            for (key, value) in keys {
                desired.insert(*key, TagValue::Integer(*value));
            }
            plan_exif_write(&scan, original, &desired)
        };
        let expected = edit(&surfaced, &[("ExifIFD:ExposureProgram", 3)]).unwrap();
        assert_eq!(
            expected
                .exif_ifd
                .iter()
                .map(|e| e.tag_id)
                .collect::<Vec<_>>(),
            [0x8822, 0xa001]
        );
        assert_eq!(expected.exif_ifd[0].value, 3u16.to_ne_bytes());
        for keys in [
            &[("ExifIFD:ExposureProgram", 3)][..],
            &[("EXIF:ExposureProgram", 3)],
            &[("ExifIFD:ExposureProgram", 3), ("EXIF:ExposureProgram", 3)],
        ] {
            assert_eq!(edit(&rowless, keys).unwrap(), expected, "{keys:?}");
        }
        let err = edit(
            &rowless,
            &[("ExifIFD:ExposureProgram", 3), ("EXIF:ExposureProgram", 4)],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("already being written as"), "{err}");

        let removed = ["ExifIFD:ExposureProgram".to_string()];
        let plan = plan_exif_write_with_removals(&scan, &rowless, &rowless, &removed).unwrap();
        let mut surfaced_removal = surfaced.clone();
        surfaced_removal.remove("ExifIFD:ExposureProgram");
        assert_eq!(
            plan,
            plan_exif_write(&scan, &surfaced, &surfaced_removal).unwrap()
        );
        assert!(plan.exif_ifd.iter().all(|e| e.tag_id != 0x8822));
        // Only a name qualified by the entry's own IFD deletes it.
        for other in ["IFD0:ExposureProgram", "EXIF:ExposureProgram"] {
            let removed = [other.to_string()];
            assert_eq!(
                plan_exif_write_with_removals(&scan, &rowless, &rowless, &removed).unwrap(),
                carried,
                "{other}"
            );
        }
    }

    /// Slice E-2: the reader names an ExifIFD entry `tag_db` has no name for
    /// by the generated `Exif::Main` (0x9210, the TIFF/EP
    /// FocalPlaneResolutionUnit of the Leica M8/M9, is
    /// `ExifIFD:FocalPlaneResolutionUnit`, no longer `ExifIFD:0x9210`). The
    /// writer carries the entry when the name is unchanged, instead of taking
    /// it for a new tag to add -- which failed every write to such a file on
    /// the printed value's type (`inches` for a SHORT).
    #[test]
    fn plan_carries_an_engine_named_exif_ifd_entry() {
        let (tiff, original) = exif_ifd_shorts_jpeg(&[(0x9210, 2), (0xa001, 1)]);
        let scan = scan_exif_entries(&tiff).unwrap();
        assert_eq!(
            original.get_string("ExifIFD:FocalPlaneResolutionUnit"),
            Some("inches")
        );
        let mut desired = original.clone();
        desired.insert("IFD0:Artist", TagValue::new_string("A. Person"));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let carried = plan.exif_ifd.iter().find(|e| e.tag_id == 0x9210).unwrap();
        assert_eq!(carried.value, [2, 0]);
        assert!(plan.ifd0.iter().any(|e| e.tag_id == 0x013B));
        assert!(
            plan.exif_ifd.iter().all(|e| e.tag_id != 0xa210),
            "no FocalPlaneResolutionUnit planted under its EXIF 2.x id"
        );
    }

    /// Review finding (E-2): the engine's name for a TIFF/EP legacy entry is
    /// the EXIF 2.x tag's (`tag_db` writes `ExifIFD:FocalPlaneResolutionUnit`
    /// to 0xa210), so the legacy 0x9210 must not own it. Pinned
    /// `exiftool-pinned.sh -ExifIFD:FocalPlaneResolutionUnit=3` (and control
    /// b4808958) keep 0x9210 = 2 and add 0xa210 = 3; a file that already has
    /// both gets only 0xa210 rewritten.
    #[test]
    fn a_legacy_entry_does_not_own_its_engine_name() {
        let (tiff, original) = exif_ifd_shorts_jpeg(&[(0x9210, 2), (0xa001, 1)]);
        let scan = scan_exif_entries(&tiff).unwrap();
        let mut desired = original.clone();
        desired.insert("ExifIFD:FocalPlaneResolutionUnit", TagValue::Integer(3));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let legacy = plan.exif_ifd.iter().find(|e| e.tag_id == 0x9210).unwrap();
        assert_eq!(legacy.value, [2, 0], "the legacy entry is carried");
        let added = plan.exif_ifd.iter().find(|e| e.tag_id == 0xa210).unwrap();
        assert_eq!(
            (added.field_type, added.value.as_slice()),
            (3, &3u16.to_ne_bytes()[..])
        );

        let (tiff, original) = exif_ifd_shorts_jpeg(&[(0x9210, 2), (0xa210, 3)]);
        let scan = scan_exif_entries(&tiff).unwrap();
        assert_eq!(
            original.get_string("ExifIFD:FocalPlaneResolutionUnit"),
            Some("cm")
        );
        let mut desired = original.clone();
        desired.insert("ExifIFD:FocalPlaneResolutionUnit", TagValue::Integer(1));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let legacy = plan.exif_ifd.iter().find(|e| e.tag_id == 0x9210).unwrap();
        assert_eq!(legacy.value, [2, 0], "the legacy entry is carried");
        let rewritten: Vec<&OutEntry> = plan
            .exif_ifd
            .iter()
            .filter(|e| e.tag_id == 0xa210)
            .collect();
        assert_eq!(rewritten.len(), 1);
        assert_eq!(rewritten[0].value, 1u16.to_ne_bytes());
    }

    /// Loads the real Canon fixture, which has an actual InteropIFD whose
    /// entries the reader surfaces under "InteropIFD:InteropIndex" /
    /// "InteropIFD:InteropVersion" (see `parse_interop_subifd`). Builds
    /// `original_map` the same way `rewrite_jpeg_exif` does in production
    /// (the full JPEG reader), not the synthetic `scan_and_maps` helper,
    /// since only the real reader surfaces the Interop keys.
    fn canon_scan_and_maps() -> (ExifScan, MetadataMap, Vec<u8>) {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jpeg/makernotes/canon_sample.jpg"
        ))
        .unwrap();
        let tiff = super::super::exif_surgical_test_support::tiff_slice(&bytes).to_vec();
        let scan = scan_exif_entries(&tiff).unwrap();
        let reader = SliceReader(&bytes);
        let original = crate::core::operations::parse_jpeg_metadata(
            &reader,
            &crate::core::ReadOptions::default_full_listing(),
        )
        .unwrap();
        assert!(
            original.contains_key("InteropIFD:InteropIndex"),
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
        let original_value = original.get("InteropIFD:InteropIndex").unwrap().clone();
        let new_value = TagValue::new_string("R03 - DCF option file (Adobe RGB)");
        assert_ne!(
            original_value, new_value,
            "test setup must actually change the value"
        );
        desired.insert("InteropIFD:InteropIndex", new_value);
        let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
        assert!(
            err.to_string().contains("InteropIFD") || err.to_string().contains("Interop"),
            "expected a clear error about unsupported Interop edits, got: {}",
            err
        );
    }

    /// Review finding (E-1 D-1): the family-0 spelling of a carried Interop
    /// DCF tag. Before D-1 the reader surfaced `EXIF:InteropIndex` and this
    /// edit was refused; after it the reader surfaces only
    /// `InteropIFD:InteropIndex`, and without the alias check
    /// `-EXIF:InteropIndex=R03` reported success while planting a stray
    /// 0x0001 in IFD0 ("Wrong IFD for 0x0001 InteropIndex" under the pinned
    /// 13.59 `-validate`) and leaving the InteropIFD entry at THM. Same for
    /// InteropVersion and RelatedImageWidth.
    #[test]
    fn plan_changed_interop_key_under_its_exif_spelling_errors_not_ifd0() {
        let (scan, original, _tiff) = canon_scan_and_maps();
        for (key, new_value) in [
            (
                "EXIF:InteropIndex",
                TagValue::new_string("R03 - DCF option file (Adobe RGB)"),
            ),
            ("EXIF:InteropVersion", TagValue::new_string("0200")),
            ("EXIF:RelatedImageWidth", TagValue::Integer(100)),
        ] {
            let native = key.replacen("EXIF:", "InteropIFD:", 1);
            assert!(
                original.get(key).is_none() && original.get(&native).is_some(),
                "{key}: the reader surfaces the entry as {native} only"
            );
            assert_ne!(original.get(&native), Some(&new_value), "{key}");
            let mut desired = original.clone();
            desired.insert(key, new_value);
            let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
            assert!(
                err.to_string().contains("not yet supported") && err.to_string().contains(key),
                "{key}: expected the carried-class refusal, got: {err}"
            );
        }
        // The same value under the EXIF: spelling is the carry-over: no IFD0
        // entry, every Interop entry carried.
        let mut desired = original.clone();
        desired.insert(
            "EXIF:InteropIndex",
            original.get("InteropIFD:InteropIndex").unwrap().clone(),
        );
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        assert!(
            !plan.ifd0.iter().any(|e| e.tag_id == 0x0001),
            "no stray InteropIndex in IFD0"
        );
        assert_eq!(
            plan.interop.len(),
            scan.entries
                .iter()
                .filter(|e| e.ifd == IfdKind::Interop)
                .count()
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
    fn plan_adding_black_level_errors_instead_of_creating_an_ifd0_tag() {
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("EXIF:BlackLevel", TagValue::new_rational(1, 1));

        let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
        assert!(
            err.to_string().contains("SubIFD"),
            "BlackLevel must be rejected rather than fabricated in IFD0: {err}"
        );
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
        let original = crate::core::operations::parse_jpeg_metadata(
            &reader,
            &crate::core::ReadOptions::default_full_listing(),
        )
        .unwrap();
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

    #[test]
    fn page_number_pair_serializes_as_two_unsigned_shorts() {
        // ExifTool 13.59 Exif.pm 0x0129: Writable => 'int16u', Count => 2.
        // A list must retain both components and the existing SHORT field.
        let value = TagValue::new_array(vec![TagValue::new_integer(3), TagValue::new_integer(17)]);
        let (field_type, count, bytes) =
            tag_value_to_field_for_key("IFD0:PageNumber", &value, Some(3)).unwrap();

        assert_eq!(field_type, 3);
        assert_eq!(count, 2);
        assert_eq!(bytes, [3_u16.to_ne_bytes(), 17_u16.to_ne_bytes()].concat());
    }

    #[test]
    fn composite_image_count_serializes_as_two_unsigned_shorts() {
        let value = TagValue::new_array(vec![TagValue::new_integer(3), TagValue::new_integer(2)]);
        let (field_type, count, bytes) =
            tag_value_to_field_for_key("ExifIFD:CompositeImageCount", &value, Some(3)).unwrap();
        assert_eq!((field_type, count), (3, 2));
        assert_eq!(bytes, [3_u16.to_ne_bytes(), 2_u16.to_ne_bytes()].concat());
    }

    #[test]
    fn subject_area_serializes_as_two_to_four_unsigned_shorts() {
        // ExifTool 13.59 Exif.pm 0x9214: Writable => 'int16u', Count => -1
        // (limited to two, three, or four components).
        let value = TagValue::new_array(vec![
            TagValue::new_integer(3),
            TagValue::new_integer(17),
            TagValue::new_integer(42),
        ]);
        let (field_type, count, bytes) =
            tag_value_to_field_for_key("EXIF:SubjectArea", &value, Some(3)).unwrap();

        assert_eq!(field_type, 3);
        assert_eq!(count, 3);
        assert_eq!(
            bytes,
            [
                3_u16.to_ne_bytes(),
                17_u16.to_ne_bytes(),
                42_u16.to_ne_bytes()
            ]
            .concat()
        );
    }

    #[test]
    fn subject_location_serializes_as_exactly_two_unsigned_shorts() {
        let value = TagValue::new_array(vec![TagValue::new_integer(3), TagValue::new_integer(4)]);
        let (field_type, count, bytes) =
            tag_value_to_field_for_key("EXIF:SubjectLocation", &value, Some(3)).unwrap();

        assert_eq!(field_type, 3);
        assert_eq!(count, 2);
        assert_eq!(bytes, [3_u16.to_ne_bytes(), 4_u16.to_ne_bytes()].concat());
    }

    #[test]
    fn signed_rational_hint_is_preserved_for_positive_apex_value() {
        let value = TagValue::Rational {
            numerator: 49_471,
            denominator: 7_102,
        };
        let (field_type, count, bytes) = tag_value_to_field(&value, Some(10)).unwrap();
        assert_eq!(field_type, 10, "ShutterSpeedValue is rational64s");
        assert_eq!(count, 1);
        assert_eq!(bytes.len(), 8);
    }

    #[test]
    fn brightness_value_uses_its_declared_signed_rational_type() {
        let value = TagValue::Rational {
            numerator: 3,
            denominator: 2,
        };
        let (field_type, count, bytes) =
            tag_value_to_field_for_key("ExifIFD:BrightnessValue", &value, None).unwrap();
        assert_eq!(field_type, 10, "BrightnessValue is rational64s");
        assert_eq!(count, 1);
        assert_eq!(bytes.len(), 8);
    }

    #[test]
    fn plan_adds_tile_width_as_declared_int32u() {
        // Exif.pm 13.59 declares IFD0 0x0142 as Writable => 'int32u'.
        // Even a small value must therefore be LONG, not the generic
        // smallest-fit SHORT selected for an untyped integer addition.
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("IFD0:TileWidth", TagValue::new_integer(3));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let tile_width = plan
            .ifd0
            .iter()
            .find(|entry| entry.tag_id == 0x0142)
            .unwrap();
        assert_eq!(tile_width.field_type, 4);
        assert_eq!(tile_width.count, 1);
    }

    #[test]
    fn plan_adds_tile_length_as_declared_int32u() {
        // Exif.pm 13.59 declares IFD0 0x0143 as Writable => 'int32u'.
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: Vec::new(),
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let original = MetadataMap::new();
        let mut desired = MetadataMap::new();
        desired.insert("IFD0:TileLength", TagValue::new_integer(3));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let tile_length = plan
            .ifd0
            .iter()
            .find(|entry| entry.tag_id == 0x0143)
            .unwrap();
        assert_eq!(tile_length.field_type, 4);
        assert_eq!(tile_length.count, 1);
    }

    /// `TagValue::Float` is an f64 and TIFF has two IEEE 754 widths, so the
    /// hint is what picks between them. Type 12 for everything made
    /// `exiftool -validate` report "Non-standard format (double) for IFD0
    /// 0xcd49 JXLDistance", whose ExifTool declaration is `Writable =>
    /// 'float'`.
    #[test]
    fn float_hint_selects_single_precision() {
        let (ft, count, bytes) = tag_value_to_field(&TagValue::Float(1.5), Some(11)).unwrap();
        assert_eq!(ft, 11, "a FLOAT-hinted value must be written FLOAT");
        assert_eq!(count, 1);
        assert_eq!(bytes.len(), 4, "FLOAT is 4 bytes");
        assert_eq!(f32::from_ne_bytes(bytes.try_into().unwrap()), 1.5);

        let (ft, count, bytes) = tag_value_to_field(&TagValue::Float(1.5), Some(12)).unwrap();
        assert_eq!(ft, 12, "a DOUBLE-hinted value stays DOUBLE");
        assert_eq!(count, 1);
        assert_eq!(bytes.len(), 8, "DOUBLE is 8 bytes");
        assert_eq!(f64::from_ne_bytes(bytes.try_into().unwrap()), 1.5);

        // No declared type to consult: the pre-existing default is kept.
        let (ft, _, bytes) = tag_value_to_field(&TagValue::Float(1.5), None).unwrap();
        assert_eq!(ft, 12);
        assert_eq!(bytes.len(), 8);
    }

    /// The hint the create path passes comes from the tag registry, so the
    /// two tags that motivated this must resolve to different widths.
    #[test]
    fn declared_ieee_field_type_separates_float_from_double() {
        // Exif.pm: 0xCD49 JXLDistance is `Writable => 'float'`.
        assert_eq!(declared_ieee_field_type("IFD0:JXLDistance"), Some(11));
        // Exif.pm: 0xC7A8 RawToPreviewGain is `Writable => 'double'`.
        assert_eq!(declared_ieee_field_type("IFD0:RawToPreviewGain"), Some(12));
        // The EXIF: spelling resolves identically to the IFD0: one.
        assert_eq!(declared_ieee_field_type("EXIF:JXLDistance"), Some(11));
        // A non-float tag declares no IEEE width and leaves the default alone.
        assert_eq!(declared_ieee_field_type("IFD0:Orientation"), None);
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

    #[test]
    fn iso_list_serializes_as_variable_count_unsigned_shorts() {
        let value =
            TagValue::new_array(vec![TagValue::new_integer(100), TagValue::new_integer(200)]);
        let (field_type, count, bytes) =
            tag_value_to_field_for_key("ExifIFD:ISO", &value, Some(3)).unwrap();
        assert_eq!((field_type, count), (3, 2));
        assert_eq!(
            bytes,
            [100_u16.to_ne_bytes(), 200_u16.to_ne_bytes()].concat()
        );
    }

    #[test]
    fn an_unchanged_fallback_maker_note_row_is_carried_not_added() {
        // The reader surfaces a SilverFast note (`LSI1\0`) as
        // `ExifIFD:MakerNoteUnknownBinary` (MakerNotes.pm 13.59:1110). Left
        // unchanged it is the carried MakerNote itself; before, it was taken
        // for a tag to add and every edit of the file failed.
        let note = b"LSI1\0\x01\x02\x03 opaque maker note".to_vec();
        let scan = ExifScan {
            byte_order: ByteOrder::BigEndian,
            entries: vec![RawEntry {
                ifd: IfdKind::ExifIfd,
                tag_id: MAKERNOTE,
                field_type: 7,
                count: note.len() as u32,
                value: note.clone(),
            }],
            thumbnail: None,
            makernote_offset: Some(38),
            ifd1_next: None,
        };
        let mut original = MetadataMap::new();
        original.insert(
            "ExifIFD:MakerNoteUnknownBinary",
            TagValue::new_binary(note.clone()),
        );
        let mut desired = original.clone();
        desired.insert("IFD0:Artist", TagValue::new_string("you"));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let carried = plan
            .exif_ifd
            .iter()
            .find(|entry| entry.tag_id == MAKERNOTE)
            .unwrap();
        assert_eq!((carried.field_type, &carried.value), (7, &note));
        assert_eq!(plan.exif_ifd.len(), 1);

        // Removing it is still refused loudly, not dropped in silence.
        let mut removed = original.clone();
        removed.remove("ExifIFD:MakerNoteUnknownBinary");
        removed.insert("IFD0:Artist", TagValue::new_string("you"));
        assert!(plan_exif_write(&scan, &original, &removed).is_err());
    }

    #[test]
    fn an_existing_gps_ifd_without_a_version_gains_none() {
        // `WriteExif` adds GPSVersionID only to a GPS IFD it creates
        // (WriteExif.pl 13.59:714-719); pinned ExifTool leaves an iPhone GPS
        // IFD that has none (t/images/Apple.jpg) as it is on an unrelated
        // edit.
        let speed = [0u32.to_be_bytes(), 1u32.to_be_bytes()].concat();
        let scan = ExifScan {
            byte_order: ByteOrder::BigEndian,
            entries: vec![RawEntry {
                ifd: IfdKind::Gps,
                tag_id: 0x000d,
                field_type: 5,
                count: 1,
                value: speed.clone(),
            }],
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let mut original = MetadataMap::new();
        original.insert(
            "GPS:GPSSpeed",
            crate::core::tag_conversion::raw_bytes_to_tag_value(
                &speed,
                5,
                1,
                0x000d,
                ByteOrder::BigEndian,
            ),
        );
        let mut desired = original.clone();
        desired.insert("IFD0:Artist", TagValue::new_string("you"));

        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        assert_eq!(plan.gps.len(), 1);
        assert_eq!(plan.gps[0].tag_id, 0x000d);
        assert_eq!(plan.gps[0].value, speed);
    }

    #[test]
    fn an_ifd1_or_interop_edit_is_refused_not_dropped() {
        // IFD1 and InteropIFD are raw-carried; the Added loop places new
        // keys in IFD0/ExifIFD/GPS only. A new `IFD1:PanasonicTitle`, a new
        // `InteropIFD:RelatedImageWidth` or a changed `IFD1:Compression`
        // (which pinned ExifTool 13.59 all write) used to leave a plan
        // without them and a write that reported success.
        let bo = ByteOrder::LittleEndian;
        let scan = ExifScan {
            byte_order: bo,
            entries: vec![
                RawEntry {
                    ifd: IfdKind::Ifd0,
                    tag_id: 0x013B,
                    field_type: 2,
                    count: 3,
                    value: b"me\0".to_vec(),
                },
                RawEntry {
                    ifd: IfdKind::Ifd1,
                    tag_id: 0x0103,
                    field_type: 3,
                    count: 1,
                    value: 6u16.to_le_bytes().to_vec(),
                },
            ],
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let mut original = MetadataMap::new();
        original.insert("IFD0:Artist", TagValue::new_string("me"));
        original.insert("IFD1:Compression", TagValue::Integer(6));
        for (key, value) in [
            ("IFD1:PanasonicTitle", TagValue::new_string("x")),
            ("InteropIFD:RelatedImageWidth", TagValue::Integer(5)),
            ("IFD1:Compression", TagValue::Integer(1)),
        ] {
            let mut desired = original.clone();
            desired.insert(key, value);
            let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
            assert!(err.to_string().contains(key), "got: {err}");
        }
        // Unchanged rows of those classes are the carry-over, not edits.
        assert!(plan_exif_write(&scan, &original, &original).is_ok());
    }

    #[test]
    fn an_ifd1_edit_is_refused_before_the_drop_all_shortcut() {
        // With no IFD0/ExifIFD/GPS/EXIF row left in the map, the planner
        // returns the empty (drop-all) plan. An IFD1-only block edited with
        // `-IFD1:Compression=1`, or a new IFD1 tag on a file with no EXIF,
        // took that shortcut and deleted the carrier (or created nothing).
        let scan = ExifScan {
            byte_order: ByteOrder::LittleEndian,
            entries: vec![RawEntry {
                ifd: IfdKind::Ifd1,
                tag_id: 0x0103,
                field_type: 3,
                count: 1,
                value: 6u16.to_le_bytes().to_vec(),
            }],
            thumbnail: None,
            makernote_offset: None,
            ifd1_next: None,
        };
        let mut original = MetadataMap::new();
        original.insert("IFD1:Compression", TagValue::Integer(6));
        let mut desired = original.clone();
        desired.insert("IFD1:Compression", TagValue::Integer(1));
        let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
        assert!(err.to_string().contains("IFD1:Compression"), "got: {err}");

        let empty = ExifScan {
            entries: Vec::new(),
            ..scan
        };
        let mut added = MetadataMap::new();
        added.insert("IFD1:PanasonicTitle", TagValue::new_string("x"));
        let err = plan_exif_write(&empty, &MetadataMap::new(), &added).unwrap_err();
        assert!(
            err.to_string().contains("IFD1:PanasonicTitle"),
            "got: {err}"
        );
    }

    #[test]
    fn short_thumbnail_pointers_are_decoded_by_type_and_their_loss_refused() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let w16 = |v: u16| match bo {
                ByteOrder::LittleEndian => v.to_le_bytes(),
                ByteOrder::BigEndian => v.to_be_bytes(),
            };
            let w32 = |v: u32| match bo {
                ByteOrder::LittleEndian => v.to_le_bytes(),
                ByteOrder::BigEndian => v.to_be_bytes(),
            };
            // A SHORT inline value: the first two bytes, the rest zero.
            let short = |v: u16| [w16(v).as_slice(), &[0, 0]].concat();
            let entry = |tag: u16, typ: u16, value: &[u8]| {
                [w16(tag).as_slice(), &w16(typ), &w32(1), value].concat()
            };
            // IFD0 {ImageWidth 7} at 8 -> IFD1 {Compression 6, thumbnail
            // offset 68 and length 4, both SHORT} at 26; thumbnail at 68.
            let mut t = match bo {
                ByteOrder::LittleEndian => b"II".to_vec(),
                ByteOrder::BigEndian => b"MM".to_vec(),
            };
            t.extend(w16(42));
            t.extend(w32(8));
            t.extend(w16(1));
            t.extend(entry(0x0100, 4, &w32(7)));
            t.extend(w32(26));
            t.extend(w16(3));
            t.extend(entry(0x0103, 3, &short(6)));
            t.extend(entry(THUMBNAIL_OFFSET, 3, &short(68)));
            t.extend(entry(THUMBNAIL_LENGTH, 3, &short(4)));
            t.extend(w32(0));
            assert_eq!(t.len(), 68);
            t.extend([0xFF, 0xD8, 0xFF, 0xD9]);
            let scan = scan_exif_entries(&t).unwrap();
            assert_eq!(
                scan.thumbnail.as_deref(),
                Some(&[0xFF, 0xD8, 0xFF, 0xD9][..]),
                "{bo:?}"
            );
            // The verifier's thumbnail-survival check reads the pair the
            // same way, so an output that lost IFD1 is refused.
            let empty = MetadataMap::new();
            let mut lost = t[..26].to_vec();
            lost[22..26].copy_from_slice(&w32(0));
            let err = verify_exif_write(Some(&t), &lost, &empty, &empty, &[], EXIF_BLOCK_MAGICS)
                .unwrap_err();
            assert!(err.to_string().contains("thumbnail"), "{bo:?}: {err}");
        }
    }

    #[test]
    fn a_chain_past_ifd1_is_refused_by_the_planner_and_the_verifier() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let w16 = |v: u16| match bo {
                ByteOrder::LittleEndian => v.to_le_bytes(),
                ByteOrder::BigEndian => v.to_be_bytes(),
            };
            let w32 = |v: u32| match bo {
                ByteOrder::LittleEndian => v.to_le_bytes(),
                ByteOrder::BigEndian => v.to_be_bytes(),
            };
            let entry = |tag: u16, typ: u16, value: u32| {
                [w16(tag).as_slice(), &w16(typ), &w32(1), &w32(value)].concat()
            };
            // IFD0 {ImageWidth 7} at 8 -> IFD1 {Compression 6} at 26 ->
            // IFD2 {ImageWidth 9} at 44.
            let mut t = match bo {
                ByteOrder::LittleEndian => b"II".to_vec(),
                ByteOrder::BigEndian => b"MM".to_vec(),
            };
            t.extend(w16(42));
            t.extend(w32(8));
            for (at_next, tag, typ, value) in [
                (26u32, 0x0100u16, 4u16, 7u32),
                (44, 0x0103, 4, 6),
                (0, 0x0100, 4, 9),
            ] {
                t.extend(w16(1));
                t.extend(entry(tag, typ, value));
                t.extend(w32(at_next));
            }
            let scan = scan_exif_entries(&t).unwrap();
            assert_eq!(scan.ifd1_next, Some(44));
            let empty = MetadataMap::new();

            // (a) the planner refuses to re-lay it out ...
            let err = plan_exif_write_with_removals(&scan, &empty, &empty, &["IFD0:Artist".into()])
                .unwrap_err();
            assert!(
                err.to_string()
                    .contains("IFD1 links to a further directory"),
                "{err}"
            );
            // ... but for IFD1:All, which deletes the chain with IFD1.
            plan_exif_write_with_removals(&scan, &empty, &empty, &["IFD1:All".into()]).unwrap();

            // (b) the verifier, alone, catches a dropped or altered chain.
            let mut lost = t.clone();
            lost[40..44].copy_from_slice(&w32(0));
            let err = verify_exif_write(Some(&t), &lost, &empty, &empty, &[], EXIF_BLOCK_MAGICS)
                .unwrap_err();
            assert!(
                err.to_string().contains("directory chain past IFD1"),
                "{err}"
            );
            let mut altered = t.clone();
            let last = altered.len() - 5;
            altered[last] ^= 1;
            assert!(
                verify_exif_write(Some(&t), &altered, &empty, &empty, &[], EXIF_BLOCK_MAGICS)
                    .is_err()
            );
            verify_exif_write(Some(&t), &t, &empty, &empty, &[], EXIF_BLOCK_MAGICS).unwrap();
            // IFD1:All: the chain must go.
            let removed = ["IFD1:All".to_string()];
            assert!(
                verify_exif_write(Some(&t), &t, &empty, &empty, &removed, EXIF_BLOCK_MAGICS)
                    .is_err()
            );
            let mut no_ifd1 = t[..26].to_vec();
            no_ifd1[22..26].copy_from_slice(&w32(0));
            verify_exif_write(
                Some(&t),
                &no_ifd1,
                &empty,
                &empty,
                &removed,
                EXIF_BLOCK_MAGICS,
            )
            .unwrap();
        }
    }

    #[test]
    fn the_write_postcondition_refuses_a_dropped_removal_or_set() {
        // Injected bad outputs: the verifier compares the produced payload
        // with the caller's request, whichever writer path produced it.
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let tiff = build_full_tiff(bo);
            let (scan, baseline) = scan_and_maps(&tiff);
            assert!(baseline.contains_key("IFD0:Make"));

            // A removal the writer "forgot": the output is the input.
            let mut removed = baseline.clone();
            removed.remove("IFD0:Make");
            let err = verify_exif_write(
                Some(&tiff),
                &tiff,
                &baseline,
                &removed,
                &[],
                EXIF_BLOCK_MAGICS,
            )
            .unwrap_err();
            assert!(err.to_string().contains("IFD0:Make"), "{err}");
            // ... and done properly, it passes.
            let good =
                serialize_exif(&plan_exif_write(&scan, &baseline, &removed).unwrap()).unwrap();
            verify_exif_write(
                Some(&tiff),
                &good,
                &baseline,
                &removed,
                &[],
                EXIF_BLOCK_MAGICS,
            )
            .unwrap();

            // A named removal of an entry the reader surfaced no row for.
            let err = verify_exif_write(
                Some(&tiff),
                &tiff,
                &removed,
                &removed,
                &["IFD0:Make".to_string()],
                EXIF_BLOCK_MAGICS,
            )
            .unwrap_err();
            assert!(err.to_string().contains("IFD0:Make"), "{err}");

            // A set the writer "forgot": entry left as it was.
            let mut set = baseline.clone();
            set.insert("IFD0:Orientation", TagValue::Integer(3));
            let err =
                verify_exif_write(Some(&tiff), &tiff, &baseline, &set, &[], EXIF_BLOCK_MAGICS)
                    .unwrap_err();
            assert!(err.to_string().contains("IFD0:Orientation"), "{err}");
            let good = serialize_exif(&plan_exif_write(&scan, &baseline, &set).unwrap()).unwrap();
            verify_exif_write(Some(&tiff), &good, &baseline, &set, &[], EXIF_BLOCK_MAGICS).unwrap();

            // A new tag with no entry at its address.
            let mut added = baseline.clone();
            added.insert("IFD0:Artist", TagValue::new_string("you"));
            let err = verify_exif_write(
                Some(&tiff),
                &tiff,
                &baseline,
                &added,
                &[],
                EXIF_BLOCK_MAGICS,
            )
            .unwrap_err();
            assert!(err.to_string().contains("IFD0:Artist"), "{err}");

            // Nothing requested, nothing to check.
            verify_exif_write(
                Some(&tiff),
                &tiff,
                &baseline,
                &baseline,
                &[],
                EXIF_BLOCK_MAGICS,
            )
            .unwrap();

            // A thumbnail the writer "forgot" (the IFD1 pointer pair is not
            // a scanned entry, so the removal check alone cannot see it).
            let mut lossy = plan_exif_write(&scan, &baseline, &added).unwrap();
            assert!(lossy.thumbnail.is_some());
            lossy.thumbnail = None;
            let lossy = serialize_exif(&lossy).unwrap();
            let err = verify_exif_write(
                Some(&tiff),
                &lossy,
                &baseline,
                &added,
                &[],
                EXIF_BLOCK_MAGICS,
            )
            .unwrap_err();
            assert!(err.to_string().contains("thumbnail"), "{err}");
            // ... unless the caller deleted it.
            verify_exif_write(
                Some(&tiff),
                &lossy,
                &baseline,
                &added,
                &["IFD1:ThumbnailImage".to_string()],
                EXIF_BLOCK_MAGICS,
            )
            .unwrap();
        }
    }
}
