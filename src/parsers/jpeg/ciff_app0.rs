//! Canon CIFF (`HEAPCCDR`), embedded verbatim inside a JPEG APP0 segment.
//!
//! This is a different shape from a standalone `.CRW` file: `parse_canon_crw`
//! (`src/parsers/raw/metadata.rs`) reads a whole CIFF file from byte 0. Here
//! the same container format shows up as the payload of one APP0 marker in an
//! otherwise-ordinary JPEG. ExifTool's own test fixture,
//! `t/images/ExifTool.jpg`, carries exactly this -- a full CIFF directory in
//! its third APP0 segment -- specifically to exercise cross-table `Make`/
//! `Model` priority arbitration (`FoundTag`, `ExifTool.pm:9448`+) against the
//! file's `IFD0` `Make`/`Model`: `-Make` resolves to the CIFF `Canon`, not
//! the `IFD0` `FUJIFILM`, because both are ordinary (non-`Priority=>0`)
//! occurrences and the CIFF one is recorded later in file order (Step 18/19's
//! newest-wins tie rule, `TagSink::record`). Step 20's acceptance matrix
//! (`OVERHAUL_STEP18_DESIGN.md`) pins this exact file and tag.
//!
//! Only `CanonRaw.pm`'s tag `0x080a` (`CanonRawMakeModel`, a
//! `ProcessBinaryData` record: 6-byte NUL-padded `Make` at offset 0,
//! NUL-terminated `Model` starting at offset 6 -- `CanonRaw.pm:410-425`) is
//! decoded here. This is intentionally not a general embedded-CIFF parser:
//! `raw::metadata::parse_ciff_directory` (the standalone-`.CRW` path) itself
//! only resolves two other tags (`AEBBracketValue`, `AFInfo`) out of
//! `CanonRaw::Main`'s ~40. Extending either path to the rest of that table is
//! out of Step 20's scope (output projection, not new tag coverage); this
//! exists to make a CIFF-sourced occurrence reachable by the CLI's
//! group/priority machinery at all, using the one tag the pinned oracle's
//! matrix exercises. Recorded under the `CIFF:` prefix -- matching this
//! codebase's existing (if family-1-flavored, see AGENTS.md's tagmodel/1.6
//! finding) key convention for every other legacy-shim-inserted MakerNote
//! tag, and confirmed against the oracle's own family display
//! (`-G0:1 -Make` -> `[MakerNotes:CIFF]`): `cli::tag_resolution::resolve_family0`
//! maps this same `"CIFF"` group0 to family-0 `"MakerNotes"` on request.

use crate::core::{Instance, MetadataMap, SHIM_DEFAULT_PRIORITY, TagValue};
use crate::parsers::jpeg::segment_parser::Segment;
use crate::parsers::tiff::makernotes::canon::parse_canon_ciff_records;
use std::collections::HashMap;

/// JPEG APP0 marker (0xFFE0). CIFF containers embedded this way arrive as
/// the payload of an APP0 segment, same as JFIF/JFXX/OCAD.
const APP0_MARKER: u16 = 0xFFE0;

/// CIFF's `CanonRawMakeModel` tag (`CanonRaw.pm:74-78`).
const CANON_RAW_MAKE_MODEL: u16 = 0x080A;

/// CIFF's `CanonFocalLength` tag (`CanonRaw.pm:118-122`), a `SubDirectory`
/// onto `%Image::ExifTool::Canon::FocalLength`.
const CANON_RAW_FOCAL_LENGTH: u16 = 0x1029;

/// `ProcessCanonRaw`'s own three-way split of a raw 16-bit directory-entry
/// tag word (`CanonRaw.pm:648-655`):
///
/// ```perl
/// my $tagID = $tag & 0x3fff;          # get tag ID
/// my $tagType = ($tag >> 8) & 0x38;   # get tag type
/// my $valueInDir = ($tag & 0x4000);   # flag for value in directory
/// ```
///
/// and the subdirectory test at `:658` (`($tagType==0x28 or $tagType==0x30)
/// and not $valueInDir`): a directory entry is a nested CIFF directory --
/// its `size`/`ptr` fields point at another `entry_count` + entries block,
/// not at a value -- exactly when its type nibble is `0x28` or `0x30` *and*
/// the value isn't packed inline. `CameraObject` (tag `0x2807`, tagType
/// `0x28`) is one such subdirectory in `t/images/ExifTool.jpg`'s embedded
/// CIFF, and `CanonRawMakeModel` (`0x080a`) is one of its entries.
fn ciff_tag_id(tag: u16) -> u16 {
    tag & 0x3FFF
}

fn ciff_is_subdirectory(tag: u16) -> bool {
    let tag_type = (tag >> 8) & 0x38;
    (tag_type == 0x28 || tag_type == 0x30) && !ciff_value_in_dir(tag)
}

/// `CanonRaw.pm:655`'s `my $valueInDir = ($tag & 0x4000);` -- the entry's
/// eight bytes after the tag word are the value itself rather than a
/// size/pointer pair.
fn ciff_value_in_dir(tag: u16) -> bool {
    tag & 0x4000 != 0
}

fn read_ciff_u16(data: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes: [u8; 2] = data.get(offset..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_ciff_u32(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes: [u8; 4] = data.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Recursively walks a CIFF directory looking only for tags `0x080a`
/// (`CanonRawMakeModel`, decoded inline) and `0x1029` (`CanonFocalLength`,
/// whose bytes are handed back to the caller). Mirrors
/// `raw::metadata::parse_ciff_directory`'s traversal (bounds checks, depth
/// guard, entry layout) but calls back only for the tags this module cares
/// about, rather than every tag `parse_ciff_record` recognizes.
///
/// `focal_length_record` is an out-parameter rather than an inline decode
/// because `%Canon::FocalLength`'s own `Condition` reads the `Model`
/// DataMember, and a CIFF directory does not guarantee `0x080a` is walked
/// before `0x1029`. The caller decodes once the whole walk is done and the
/// Model is known.
fn walk_ciff_directory<'a>(
    data: &'a [u8],
    container_start: usize,
    container_end: usize,
    directory_offset: usize,
    metadata: &mut MetadataMap,
    focal_length_record: &mut Option<&'a [u8]>,
    depth: usize,
) {
    if depth > 16 {
        return;
    }
    let Some(entry_count) = read_ciff_u16(data, directory_offset).map(usize::from) else {
        return;
    };
    if entry_count > 256 {
        return;
    }
    let Some(directory_end) = entry_count
        .checked_mul(10)
        .and_then(|size| directory_offset.checked_add(2 + size + 4))
    else {
        return;
    };
    if directory_end > container_end || directory_end > data.len() {
        return;
    }

    for index in 0..entry_count {
        let Some(entry_offset) = index
            .checked_mul(10)
            .and_then(|offset| directory_offset.checked_add(2 + offset))
        else {
            continue;
        };
        let Some(tag) = read_ciff_u16(data, entry_offset) else {
            continue;
        };
        let Some(size) = read_ciff_u32(data, entry_offset + 2).map(|value| value as usize) else {
            continue;
        };
        let Some(relative) = read_ciff_u32(data, entry_offset + 6).map(|value| value as usize)
        else {
            continue;
        };
        // `CanonRaw.pm:693-695`: "this type of tag stores the value in the
        // 'size' and 'ptr' fields" -- when `valueInDir` is set the eight bytes
        // that would otherwise be a size and a pointer *are* the value, and
        // `size`/`relative` above are not offsets at all.
        //
        // Missing this silently dropped every inline-value entry in the whole
        // container, because the misread pointer almost always lands out of
        // bounds and the entry falls out through one of the `continue`s above:
        // `t/images/ExifTool.jpg`'s CIFF walked 21 entries where the pinned
        // oracle's `-v3` dump lists 26, and `CanonFocalLength` (`0x5029`, i.e.
        // `0x1029 | 0x4000`) was one of the five lost. `parse_ciff_directory`
        // in `raw::metadata` -- the standalone-`.CRW` twin of this walker --
        // has always had this branch; only the APP0 path lacked it.
        let value = if ciff_value_in_dir(tag) {
            let Some(bytes) = data.get(entry_offset + 2..entry_offset + 10) else {
                continue;
            };
            bytes
        } else {
            let Some(value_start) = container_start.checked_add(relative) else {
                continue;
            };
            let Some(value_end) = value_start.checked_add(size) else {
                continue;
            };
            let Some(bytes) = data.get(value_start..value_end) else {
                continue;
            };
            bytes
        };

        if ciff_is_subdirectory(tag) {
            if value.len() < 4 {
                continue;
            }
            // Only reachable when `valueInDir` is clear (`ciff_is_subdirectory`
            // requires it), so the value block really is at `relative`.
            let value_start = container_start + relative;
            let value_end = value_start + size;
            let Some(relative_directory) =
                read_ciff_u32(value, value.len() - 4).map(|value| value as usize)
            else {
                continue;
            };
            let Some(nested_directory) = value_start.checked_add(relative_directory) else {
                continue;
            };
            walk_ciff_directory(
                data,
                value_start,
                value_end,
                nested_directory,
                metadata,
                focal_length_record,
                depth + 1,
            );
            continue;
        }

        match ciff_tag_id(tag) {
            CANON_RAW_MAKE_MODEL => insert_make_model(value, metadata),
            CANON_RAW_FOCAL_LENGTH => *focal_length_record = Some(value),
            _ => {}
        }
    }
}

/// Decodes `%Canon::MakeModel` (`CanonRaw.pm:410-425`): `Make` is a 6-byte
/// NUL-padded string at offset 0 (`"Canon\0"`), `Model` is a NUL-terminated
/// string starting at offset 6 running to the end of the record.
fn insert_make_model(record: &[u8], metadata: &mut MetadataMap) {
    let Some(make_bytes) = record.get(0..6) else {
        return;
    };
    let make_len = make_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(make_bytes.len());
    let make = String::from_utf8_lossy(&make_bytes[..make_len]);
    if !make.is_empty() {
        metadata.insert(
            "CIFF:Make".to_string(),
            TagValue::new_string(make.into_owned()),
        );
    }

    if let Some(model_bytes) = record.get(6..) {
        let model_len = model_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(model_bytes.len());
        let model = String::from_utf8_lossy(&model_bytes[..model_len]);
        if !model.is_empty() {
            metadata.insert(
                "CIFF:Model".to_string(),
                TagValue::new_string(model.into_owned()),
            );
        }
    }
}

/// Decodes `%Canon::FocalLength`'s `FocalPlaneXSize`/`FocalPlaneYSize`
/// (`Canon.pm:2726-2770`, keys 2 and 3) out of CIFF record `0x1029`
/// (`CanonRaw.pm:118-122`).
///
/// # Why these two and not the whole record
///
/// The same record's key 1 `FocalLength` has
/// `ValueConv => '$val / ($$self{FocalUnits} || 1)'`, and `FocalUnits` is a
/// DataMember of `%Canon::CameraSettings` -- a record this APP0 path does not
/// parse. Emitting a focal length divided by an assumed unit would be a
/// plausible-but-wrong value under a real ExifTool tag name, which AGENTS.md's
/// "never approximate a conversion" rule forbids; it is omitted instead. Key 0
/// `FocalType` is omitted for scope, not correctness. The two sizes have no
/// such dependency: `$val * 25.4 / 1000` from the record alone, gated by the
/// `Model` condition and the `$val < 40 ? undef : $val` plausibility guard,
/// both of which [`parse_canon_ciff_records`] already applies.
///
/// # Why it goes through the MakerNote decoder
///
/// Same reason `raw::metadata::parse_canon_crw` does (see
/// [`parse_canon_ciff_records`]' own doc comment): there is one transcription
/// of `%Canon::FocalLength`, and a second copy written against CIFF bytes
/// would be a second chance to get the conversion -- and the unrounded
/// `ValueConv` form the Composite chain consumes -- subtly wrong. The decoder
/// keys its output `Canon:`; the embedded-CIFF-in-JPEG case is family-1
/// `CIFF` (confirmed against the pinned 13.59 oracle: `-a -G1 -s` on
/// `t/images/ExifTool.jpg` prints `[CIFF] FocalPlaneXSize : 5.05 mm`), and
/// `cli::tag_resolution::resolve_family0` maps that `CIFF` label back to
/// family-0 `MakerNotes` on request, exactly as it already does for
/// `CIFF:Make`.
///
/// # What this unblocks
///
/// `CalcScaleFactor35efl` (Exif.pm) takes the FocalPlaneX/YSize branch
/// whenever the aspect ratio looks like 4:3 or 3:2, ahead of the
/// resolution-derived fallback. Without these two, ExifTool.jpg's chain fell
/// through to that fallback, derived a 0.42 mm sensor diagonal from
/// ExifImageWidth/Height and the focal-plane resolutions, rejected it as
/// implausible and produced no `Composite:ScaleFactor35efl` at all -- so
/// `Composite:FocalLength35efl` was stuck at the un-refined `6.0 mm` where
/// the oracle says `6.0 mm (35 mm equivalent: 41.4 mm)`.
fn insert_focal_plane_sizes(record: &[u8], metadata: &mut MetadataMap) {
    let model = metadata.get_string("CIFF:Model").map(str::to_string);
    let mut value_forms: HashMap<String, String> = HashMap::new();
    let decoded = parse_canon_ciff_records(
        &[(CANON_RAW_FOCAL_LENGTH, record)],
        model.as_deref(),
        &mut value_forms,
    );

    for name in ["FocalPlaneXSize", "FocalPlaneYSize"] {
        let makernote_key = format!("Canon:{name}");
        let Some(display) = decoded.get(&makernote_key) else {
            continue;
        };
        let ciff_key = format!("CIFF:{name}");
        // The unrounded `$val * 25.4 / 1000` form rides along the same channel
        // the CRW route uses, because `CalcScaleFactor35efl` squares these
        // numbers and the "%.2f" print (5.05 for 5.0546) is a different number
        // than the one ExifTool's arithmetic sees.
        match value_forms.get(&makernote_key) {
            Some(value_form) => {
                metadata.insert_occurrence_with_raw(
                    ciff_key,
                    TagValue::new_string(display.clone()),
                    TagValue::new_string(value_form.clone()),
                    SHIM_DEFAULT_PRIORITY,
                    "",
                    Instance::default(),
                );
            }
            None => {
                metadata.insert(ciff_key, TagValue::new_string(display.clone()));
            }
        }
    }
}

/// Scans every APP0 segment for an embedded CIFF (`II`+`HEAPCCDR`)
/// container and, when found, records its `Make`/`Model`.
///
/// Must run after `process_exif_segments`: Step 18/19's tie rule gives the
/// win to the *later*-recorded occurrence on an equal-priority tie
/// (`TagSink::record`), and the oracle's cross-group arbitration for this
/// fixture depends on CIFF's `Make` being recorded after `IFD0`'s.
pub(crate) fn process_ciff_app0_segments(segments: &[Segment<'_>], metadata: &mut MetadataMap) {
    for segment in segments {
        if segment.marker != APP0_MARKER {
            continue;
        }
        let data = segment.data;
        // `ExifTool.pm:943`: `CRW => '(II|MM).{4}HEAP(CCDR|JPGM)'`. Standalone
        // `.CRW` files carry `HEAPCCDR`; a CIFF directory embedded in a
        // JPEG's APP0 segment (this module's whole reason to exist) carries
        // `HEAPJPGM` instead (`JPEG.pm:35`, `ExifTool.pm:7730`) -- confirmed
        // against `t/images/ExifTool.jpg`'s actual third APP0 segment, which
        // starts `49 49 1A 00 00 00 48 45 41 50 4A 50 47 4D` (`II`, heap
        // offset 0x1A, `HEAPJPGM`). Only little-endian (`II`) is handled:
        // every embedded-CIFF fixture in the pinned corpus is `II`, and
        // `raw::metadata::parse_canon_crw` (the standalone-file path this
        // mirrors) is little-endian-only for the same reason.
        if data.get(..2) != Some(b"II")
            || !matches!(data.get(6..14), Some(b"HEAPCCDR") | Some(b"HEAPJPGM"))
        {
            continue;
        }
        let Some(heap_start) = read_ciff_u32(data, 2).map(|value| value as usize) else {
            continue;
        };
        if heap_start >= data.len() || data.len().saturating_sub(heap_start) < 4 {
            continue;
        }
        let Some(root_relative) = read_ciff_u32(data, data.len() - 4).map(|value| value as usize)
        else {
            continue;
        };
        let Some(root_directory) = heap_start.checked_add(root_relative) else {
            continue;
        };
        if root_directory >= data.len() {
            continue;
        }
        let mut focal_length_record = None;
        walk_ciff_directory(
            data,
            heap_start,
            data.len(),
            root_directory,
            metadata,
            &mut focal_length_record,
            0,
        );
        if let Some(record) = focal_length_record {
            insert_focal_plane_sizes(record, metadata);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim payload shape (offsets are relative within the CIFF
    /// container, not this test's byte array): a root directory with one
    /// entry pointing at the `CanonRawMakeModel` record. This is a
    /// minimized version of what `t/images/ExifTool.jpg`'s third APP0
    /// segment actually carries, not a synthetic invention -- the record
    /// bytes are the real ones from that file's `-v3` dump (`Canon\0Canon
    /// PowerShot A5\0`).
    fn build_minimal_ciff_app0() -> Vec<u8> {
        let mut data = Vec::new();
        // Header: "II" + heap_start (u32 LE) + "HEAPCCDR".
        data.extend_from_slice(b"II");
        let heap_start_pos = data.len();
        data.extend_from_slice(&0u32.to_le_bytes()); // patched below
        data.extend_from_slice(b"HEAPCCDR");
        let heap_start = data.len() as u32;
        data[heap_start_pos..heap_start_pos + 4].copy_from_slice(&heap_start.to_le_bytes());

        // Heap: one record (the MakeModel binary blob), relative offset 0.
        let record: [u8; 25] = *b"Canon\0Canon PowerShot A5\0";
        let record_relative = 0u32;
        data.extend_from_slice(&record);

        // Root directory: entry_count=1, one 10-byte entry, trailing 4-byte
        // directory-offset word (unused by this walker, but part of the
        // real CIFF shape so bounds math matches production).
        let directory_offset_in_heap = data.len() as u32 - heap_start;
        data.extend_from_slice(&1u16.to_le_bytes()); // entry_count
        data.extend_from_slice(&CANON_RAW_MAKE_MODEL.to_le_bytes()); // tag
        data.extend_from_slice(&(record.len() as u32).to_le_bytes()); // size
        data.extend_from_slice(&record_relative.to_le_bytes()); // relative offset

        // Trailing 4-byte word: root directory's own offset, relative to
        // heap_start, from the end of the container (CIFF's own footer
        // convention).
        data.extend_from_slice(&directory_offset_in_heap.to_le_bytes());
        data
    }

    #[test]
    fn decodes_make_and_model_from_a_minimal_embedded_ciff_directory() {
        let payload = build_minimal_ciff_app0();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert_eq!(metadata.get_string("CIFF:Make"), Some("Canon"));
        assert_eq!(
            metadata.get_string("CIFF:Model"),
            Some("Canon PowerShot A5")
        );
    }

    /// The same container plus a second, `valueInDir` entry carrying
    /// `CanonFocalLength` (`0x1029 | 0x4000` == `0x5029`).
    ///
    /// The eight record bytes are `t/images/ExifTool.jpg`'s own, read straight
    /// out of the pinned 13.59 oracle's `-v3` dump of that file's third APP0
    /// segment (`0252: 01 00 05 00 c7 00 92 00`, decoded there as
    /// `FocalType = 1`, `FocalLength = 5`, `FocalPlaneXSize = 199`,
    /// `FocalPlaneYSize = 146`), not invented for this test.
    fn build_ciff_app0_with_focal_length() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        let heap_start_pos = data.len();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(b"HEAPCCDR");
        let heap_start = data.len() as u32;
        data[heap_start_pos..heap_start_pos + 4].copy_from_slice(&heap_start.to_le_bytes());

        let record: [u8; 25] = *b"Canon\0Canon PowerShot A5\0";
        data.extend_from_slice(&record);

        let directory_offset_in_heap = data.len() as u32 - heap_start;
        data.extend_from_slice(&2u16.to_le_bytes()); // entry_count

        // Entry 0: MakeModel, value out in the heap at relative offset 0.
        data.extend_from_slice(&CANON_RAW_MAKE_MODEL.to_le_bytes());
        data.extend_from_slice(&(record.len() as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        // Entry 1: CanonFocalLength with CanonRaw.pm's `valueInDir` bit set --
        // the eight bytes that would be `size` and `ptr` are the record.
        data.extend_from_slice(&(CANON_RAW_FOCAL_LENGTH | 0x4000).to_le_bytes());
        data.extend_from_slice(&[0x01, 0x00, 0x05, 0x00, 0xc7, 0x00, 0x92, 0x00]);

        data.extend_from_slice(&directory_offset_in_heap.to_le_bytes());
        data
    }

    /// A `valueInDir` entry is read from the directory, and
    /// `%Canon::FocalLength` keys 2/3 come out with ExifTool's own
    /// `sprintf("%.2f mm", $val * 25.4 / 1000)` print forms.
    ///
    /// Before the `valueInDir` branch existed this walker read the entry's
    /// inline value bytes as a size and a pointer, landed out of bounds and
    /// dropped the record silently -- along with every other inline-value
    /// entry in the container.
    #[test]
    fn decodes_focal_plane_sizes_from_a_value_in_dir_entry() {
        let payload = build_ciff_app0_with_focal_length();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert_eq!(metadata.get_string("CIFF:Make"), Some("Canon"));
        assert_eq!(metadata.get_string("CIFF:FocalPlaneXSize"), Some("5.05 mm"));
        assert_eq!(metadata.get_string("CIFF:FocalPlaneYSize"), Some("3.71 mm"));
    }

    /// The unrounded `ValueConv` form rides along, because
    /// `CalcScaleFactor35efl` squares these numbers and 5.05 is a different
    /// number than 5.0546.
    #[test]
    fn focal_plane_sizes_carry_the_unrounded_value_form() {
        let payload = build_ciff_app0_with_focal_length();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        let x: f64 = metadata
            .value_form("CIFF:FocalPlaneXSize")
            .expect("FocalPlaneXSize value form")
            .parse()
            .expect("numeric value form");
        assert!((x - 199.0 * 25.4 / 1000.0).abs() < 1e-12);
    }

    /// `FocalLength` (key 1) is deliberately absent: its ValueConv divides by
    /// the `FocalUnits` DataMember of `%Canon::CameraSettings`, a record this
    /// path does not parse, so emitting one would be a guess.
    #[test]
    fn focal_length_itself_is_omitted_rather_than_guessed() {
        let payload = build_ciff_app0_with_focal_length();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert!(metadata.get_string("CIFF:FocalLength").is_none());
    }

    #[test]
    fn ignores_app0_segments_without_the_ciff_signature() {
        let mut metadata = MetadataMap::new();
        let jfif = b"JFIF\0\x01\x02\x00\x00\x01\x00\x01\x00\x00";
        let segments = vec![Segment::new(APP0_MARKER, 0, jfif)];

        process_ciff_app0_segments(&segments, &mut metadata);

        assert!(metadata.get_string("CIFF:Make").is_none());
    }

    #[test]
    fn ignores_non_app0_segments() {
        let payload = build_minimal_ciff_app0();
        // Same bytes, wrong marker: must not be scanned.
        let segments = vec![Segment::new(0xFFE1, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert!(metadata.get_string("CIFF:Make").is_none());
    }
}
