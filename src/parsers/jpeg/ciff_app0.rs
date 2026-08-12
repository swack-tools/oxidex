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

use crate::core::{MetadataMap, TagValue};
use crate::parsers::jpeg::segment_parser::Segment;

/// JPEG APP0 marker (0xFFE0). CIFF containers embedded this way arrive as
/// the payload of an APP0 segment, same as JFIF/JFXX/OCAD.
const APP0_MARKER: u16 = 0xFFE0;

/// CIFF's `CanonRawMakeModel` tag (`CanonRaw.pm:74-78`).
const CANON_RAW_MAKE_MODEL: u16 = 0x080A;

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
    let value_in_dir = tag & 0x4000 != 0;
    (tag_type == 0x28 || tag_type == 0x30) && !value_in_dir
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

/// Recursively walks a CIFF directory looking only for tag `0x080a`. Mirrors
/// `raw::metadata::parse_ciff_directory`'s traversal (bounds checks, depth
/// guard, entry layout) but calls back only for the one tag this module
/// cares about, rather than every tag `parse_ciff_record` recognizes.
fn walk_for_make_model(
    data: &[u8],
    container_start: usize,
    container_end: usize,
    directory_offset: usize,
    metadata: &mut MetadataMap,
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
        let Some(value_start) = container_start.checked_add(relative) else {
            continue;
        };
        let Some(value_end) = value_start.checked_add(size) else {
            continue;
        };
        let Some(value) = data.get(value_start..value_end) else {
            continue;
        };

        if !ciff_is_subdirectory(tag) && ciff_tag_id(tag) == CANON_RAW_MAKE_MODEL {
            insert_make_model(value, metadata);
        }

        if ciff_is_subdirectory(tag) && value.len() >= 4 {
            let Some(relative_directory) =
                read_ciff_u32(value, value.len() - 4).map(|value| value as usize)
            else {
                continue;
            };
            let Some(nested_directory) = value_start.checked_add(relative_directory) else {
                continue;
            };
            walk_for_make_model(
                data,
                value_start,
                value_end,
                nested_directory,
                metadata,
                depth + 1,
            );
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
        walk_for_make_model(data, heap_start, data.len(), root_directory, metadata, 0);
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
