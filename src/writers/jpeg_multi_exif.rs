//! A JPEG with more than one EXIF APP1 record: every EXIF-family write and
//! deletion is refused, by name, before anything is written.
//!
//! Pinned ExifTool 13.59 writes every EXIF APP1 of such a file: it warns
//! `Multiple APP1 EXIF records` (Writer.pl 13.59:6379) and then edits each
//! block in turn, so `-ExifIFD:WhiteBalance#=1` leaves two
//! `[ExifIFD] WhiteBalance : 1` rows and `-EXIF:All=` drops both records.
//! The oxidex EXIF writers edit exactly one block. Before this check a
//! grouped set on a file whose first block is the complete one edited that
//! block alone and reported `1 image files updated`; worse, the writer diffed
//! the whole-file read map -- where a key both blocks hold carries the later
//! block's value, as ExifTool's own duplicate winner does -- against the
//! first block, so rows only the second block holds were planted in the
//! first (`ImageWidth`, `ImageHeight`), and a first block lacking IFD0
//! `Orientation` failed on the second block's print-converted value
//! (`Invalid value for tag 'IFD0:Orientation'`).
//!
//! Until oxidex writes every block as ExifTool does, the request is refused
//! with [`ExifToolError::TagsNotWritten`], one entry per EXIF-family key it
//! sets or deletes, and the file is left byte-identical. The check runs at
//! the one entry every write goes through
//! (`core::operations::write_metadata_transaction`), so the library, the CLI
//! and the C ABI refuse alike. A whole-metadata clear (`clear_all_metadata`,
//! `-all=`) is not a request for named tags and keeps the carrier writer's
//! own refusal.

use crate::core::{FileFormat, MetadataMap};
use crate::error::{ExifToolError, Result, TagNotWritten};
use crate::io::MMapReader;
use crate::parsers::detection::detect_format;
use crate::parsers::jpeg::segment_parser::parse_segments;
use std::path::Path;

const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";
const SOS_MARKER: u16 = 0xFFDA;
const EOI_MARKER: u16 = 0xFFD9;

/// Refuse a write request that sets or deletes an EXIF-family tag of a JPEG
/// holding more than one EXIF APP1 record (see the module documentation).
///
/// `baseline` is the file's read map, `metadata` the requested map,
/// `removed` the named deletions and `assigned` the keys the caller set
/// explicitly (a same-value set is still a set). A file that is not a JPEG,
/// that holds at most one EXIF APP1, or that cannot be read here is left to
/// the writers, which report their own errors.
pub(crate) fn refuse_multi_exif_app1_writes(
    path: &Path,
    baseline: &MetadataMap,
    metadata: &MetadataMap,
    removed: &[String],
    assigned: &[String],
) -> Result<()> {
    if metadata.is_empty() && removed.is_empty() {
        return Ok(());
    }
    let Ok(reader) = MMapReader::new(path) else {
        return Ok(());
    };
    if !matches!(detect_format(&reader), Ok(FileFormat::JPEG)) {
        return Ok(());
    }
    let blocks = exif_app1_records(&reader);
    if blocks < 2 {
        return Ok(());
    }
    let keys = exif_family_request_keys(baseline, metadata, removed, assigned);
    if keys.is_empty() {
        return Ok(());
    }
    let reason = format!(
        "the file has {blocks} EXIF APP1 blocks; ExifTool writes every one, oxidex writes one"
    );
    Err(ExifToolError::TagsNotWritten {
        tags: keys
            .into_iter()
            .map(|key| TagNotWritten::new(key, reason.clone()))
            .collect(),
    })
}

/// The number of EXIF APP1 records in a JPEG's header, counted as pinned
/// ExifTool 13.59 counts them for `Multiple APP1 EXIF records`: an APP1
/// starting `Exif\0\0`, less ExtendedEXIF continuations (a record directly
/// after an EXIF record whose bytes after the identifier are no TIFF header,
/// Writer.pl 13.59:5750-5758). Only segments before SOS/EOI are looked at.
fn exif_app1_records(reader: &MMapReader) -> usize {
    let Ok(segments) = parse_segments(reader) else {
        return 0;
    };
    let mut records = 0;
    let mut previous_was_exif = false;
    for segment in segments
        .iter()
        .take_while(|segment| !matches!(segment.marker, SOS_MARKER | EOI_MARKER))
    {
        if segment.is_app1() && segment.data.starts_with(EXIF_IDENTIFIER) {
            let tiff = &segment.data[EXIF_IDENTIFIER.len()..];
            let has_header = tiff.starts_with(b"II*\0") || tiff.starts_with(b"MM\0*");
            if has_header || !previous_was_exif {
                records += 1;
            }
            previous_was_exif = true;
        } else if (0xFFE0..=0xFFEF).contains(&segment.marker) {
            previous_was_exif = false;
        }
    }
    records
}

/// Every EXIF-family key the request sets or deletes, spelled as the request
/// spelled it, each once: explicit sets, changed values and rows dropped from
/// the map, then named deletions.
fn exif_family_request_keys(
    baseline: &MetadataMap,
    metadata: &MetadataMap,
    removed: &[String],
    assigned: &[String],
) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let mut push = |key: &str| {
        if is_exif_family_key(key, baseline) && !keys.iter().any(|seen| seen == key) {
            keys.push(key.to_string());
        }
    };
    for key in assigned {
        push(key);
    }
    for (key, value) in metadata.iter() {
        if baseline.get(key) != Some(value) {
            push(key);
        }
    }
    for key in baseline.keys() {
        if !metadata.contains_key(key) {
            push(key);
        }
    }
    for key in removed {
        push(key);
    }
    keys
}

/// Whether `key` names a tag ExifTool keeps in a JPEG's EXIF APP1: a key of
/// an EXIF directory or of the `EXIF` family, a maker-note row, or an
/// ungrouped name the tag registry defines in EXIF or GPS. A trailing `#`
/// (ExifTool's raw-value marker) names the same tag.
fn is_exif_family_key(key: &str, baseline: &MetadataMap) -> bool {
    let key = key.strip_suffix('#').unwrap_or(key);
    let canonical = crate::writers::exif_surgical::canonical_write_key(key, baseline);
    match canonical.split_once(':') {
        Some((group, _)) => {
            matches!(
                group,
                "IFD0" | "IFD1" | "ExifIFD" | "GPS" | "InteropIFD" | "SubIFD" | "EXIF"
            ) || crate::writers::exif_surgical::is_makernote_row(baseline, &canonical)
        }
        None => ["EXIF", "GPS"].iter().any(|group| {
            crate::tag_db::tag_registry::canonical_tag_name_spelling(group, key).is_some_and(
                |name| {
                    crate::tag_db::tag_registry::get_tag_descriptor(&format!("{group}:{name}"))
                        .is_some()
                },
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::TagValue;

    fn map(rows: &[(&str, &str)]) -> MetadataMap {
        let mut map = MetadataMap::new();
        for (key, value) in rows {
            map.insert(*key, TagValue::new_string(*value));
        }
        map
    }

    #[test]
    fn exif_family_keys_are_the_exif_directories_maker_notes_and_exif_names() {
        let baseline = map(&[("Nikon:Quality", "Fine")]);
        for key in [
            "IFD0:Artist",
            "ifd0:artist",
            "ExifIFD:WhiteBalance#",
            "GPS:GPSAltitude",
            "IFD1:XResolution",
            "InteropIFD:InteropIndex",
            "EXIF:All",
            "MakerNotes:All",
            "Nikon:Quality",
            "Artist",
            "WhiteBalance#",
            "GPSAltitude",
        ] {
            assert!(is_exif_family_key(key, &baseline), "{key}");
        }
        for key in ["XMP:Title", "File:Comment", "IPTC:Keywords", "Title"] {
            assert!(!is_exif_family_key(key, &baseline), "{key}");
        }
    }

    #[test]
    fn request_keys_are_sets_changes_drops_and_deletions_in_that_order() {
        let baseline = map(&[
            ("IFD0:Make", "NIKON"),
            ("IFD0:Model", "E775"),
            ("XMP:Title", "t"),
        ]);
        let mut metadata = baseline.clone();
        metadata.insert("IFD0:Model", TagValue::new_string("other"));
        metadata.remove("XMP:Title");
        let keys = exif_family_request_keys(
            &baseline,
            &metadata,
            &["ExifIFD:ISO".to_string(), "XMP:Title".to_string()],
            &["IFD0:Make".to_string()],
        );
        assert_eq!(keys, ["IFD0:Make", "IFD0:Model", "ExifIFD:ISO"]);
        // A carried map with no request is no EXIF-family change.
        assert!(exif_family_request_keys(&baseline, &baseline, &[], &[]).is_empty());
    }
}
