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

use crate::core::{FileFormat, FileReader, MetadataMap};
use crate::error::{ExifToolError, Result, TagNotWritten};
use crate::io::MMapReader;
use crate::parsers::detection::detect_format;
use std::path::Path;

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
    if metadata.is_empty() && removed.is_empty() && assigned.is_empty() {
        return Ok(());
    }
    let Ok(reader) = MMapReader::new(path) else {
        return Ok(());
    };
    refuse_multi_exif_app1_writes_from_reader(&reader, baseline, metadata, removed, assigned)
}

/// Plan-time guard using the transaction's authoritative opened reader.
pub(crate) fn refuse_multi_exif_app1_writes_from_reader(
    reader: &MMapReader,
    baseline: &MetadataMap,
    metadata: &MetadataMap,
    removed: &[String],
    assigned: &[String],
) -> Result<()> {
    if metadata.is_empty() && removed.is_empty() && assigned.is_empty() {
        return Ok(());
    }
    let Some(blocks) = multiple_exif_app1_records_from_reader(reader) else {
        return Ok(());
    };
    // The planner normally proves an absent deletion before writer guards.
    // This guard runs earlier to avoid a partial write, so use that same
    // physical check here: a bare name may be absent from every APP1 while
    // a rowless entry in one block must still be refused.
    let active_removed: Vec<String> = removed
        .iter()
        .filter(|key| {
            if key.contains(':') {
                // The absence helper scans canonical EXIF directory names.
                // Keep the caller's spelling in `active_removed` so a later
                // refusal still names the original request.
                let canonical = crate::writers::exif_surgical::canonical_write_key(key, baseline);
                !crate::core::operations::removal_is_no_op_with_reader(&canonical, baseline, reader)
                    .unwrap_or(false)
            } else {
                !crate::core::operations::bare_removal_is_no_op_with_reader(key, baseline, reader)
            }
        })
        .cloned()
        .collect();
    let keys = exif_family_request_keys(baseline, metadata, &active_removed, assigned);
    refuse_keys(blocks, keys)
}

/// The public byte-level JPEG writer can be called without the file-level
/// transaction. It must also refuse a rewrite that would edit just the first
/// EXIF record. An empty desired map is a named EXIF deletion at this level:
/// only the carrier-level whole clear removes every record.
pub(crate) fn refuse_multi_exif_app1_rewrite(
    file_bytes: &[u8],
    baseline: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> Result<()> {
    let blocks = exif_app1_records(file_bytes);
    if blocks <= 1 {
        return Ok(());
    }
    let assigned = desired.assigned_keys();
    let keys = exif_family_request_keys(baseline, desired, removed, &assigned);
    refuse_keys(blocks, keys)
}

fn refuse_keys(blocks: usize, keys: Vec<String>) -> Result<()> {
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

/// Count only JPEGs with several EXIF records; other files need no guard.
pub(crate) fn multiple_exif_app1_records_from_reader(reader: &MMapReader) -> Option<usize> {
    if !matches!(detect_format(reader), Ok(FileFormat::JPEG)) {
        return None;
    }
    let data = reader.read(0, reader.size() as usize).ok()?;
    let records = exif_app1_records(data);
    (records > 1).then_some(records)
}

/// The number of EXIF APP1 records in a JPEG's header. Marker scanning and
/// continuation classification follow pinned ExifTool 13.59's JPEG pre-scan
/// (Writer.pl 13.59:5709-5781); identifier-only records also count because
/// its rewrite loop recognizes and populates them (Writer.pl 13.59:6352),
/// each of which its writer edits and warns `Multiple APP1 EXIF records`
/// about. The scan is ExifTool's, not the reader's segment parser: it skips
/// to the next 0xFF after each segment and over any number of 0xFF fill
/// bytes before a marker, so a padded header is counted whole (the parser
/// stopped at the fill byte, counted one record, and let the one-block
/// writer truncate the file); it stops at SOS or EOI. An APP1 is an EXIF
/// record when it matches `/^(.{0,4})Exif\0./is` (so `Exif\0\x01`,
/// `exif\0\0` and up to four bytes of leading junk count), less an
/// ExtendedEXIF continuation: one directly after an EXIF record or
/// continuation, with no junk, whose bytes after the identifier are no TIFF
/// header.
fn exif_app1_records(data: &[u8]) -> usize {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return 0;
    }
    let mut pos = 2;
    let mut records = 0;
    // Whether the previous segment was an EXIF record or continuation.
    let mut after_exif = false;
    loop {
        // `ReadLine` with `$/ = "\xff"`: up to and past the next 0xFF.
        let Some(skip) = data
            .get(pos..)
            .and_then(|rest| rest.iter().position(|&b| b == 0xFF))
        else {
            break;
        };
        pos += skip + 1;
        // Any number of 0xFF fill bytes, then the marker.
        let marker = loop {
            let Some(&byte) = data.get(pos) else {
                return records;
            };
            pos += 1;
            if byte != 0xFF {
                break byte;
            }
        };
        if matches!(marker, 0xDA | 0xD9) {
            break;
        }
        let mut exif = false;
        if marker & 0xF0 == 0xC0 && (marker == 0xC0 || marker & 0x03 != 0) {
            // SOF0-SOF15 but DHT, JPGA and DAC: ExifTool skips 7 bytes.
            pos += 7;
        } else if !matches!(marker, 0x00 | 0x01 | 0xD0..=0xD7) {
            let Some(&[high, low]) = data.get(pos..pos + 2) else {
                break;
            };
            pos += 2;
            let Some(length) = (usize::from(u16::from_be_bytes([high, low]))).checked_sub(2) else {
                break;
            };
            if marker & 0xF0 == 0xE0 {
                let Some(head) = data.get(pos..pos + length.min(64)) else {
                    break;
                };
                if marker == 0xE1
                    && let Some((junk, bytes)) = exif_identifier(head)
                {
                    let tiff = bytes.starts_with(b"MM\0\x2a") || bytes.starts_with(b"II\x2a\0");
                    // The rewrite loop also accepts an identifier-only record,
                    // which the stricter pre-scan does not call ExtendedEXIF.
                    if bytes.is_empty() || !(after_exif && junk == 0 && !tiff) {
                        records += 1;
                    }
                    exif = !bytes.is_empty();
                }
            }
            pos += length;
        }
        after_exif = exif;
    }
    records
}

/// The rewrite loop's EXIF APP1 identifier, `/^(.{0,4})Exif\0./is`:
/// leading junk length and up to four following bytes. An empty tail still
/// names a record the writer populates, although the pre-scan skips it.
fn exif_identifier(head: &[u8]) -> Option<(usize, &[u8])> {
    (0..=4).find_map(|junk| {
        let id = head.get(junk..junk + 5)?;
        if !id.eq_ignore_ascii_case(b"Exif\0") {
            return None;
        }
        let bytes = head.get(junk + 6..)?;
        Some((junk, &bytes[..bytes.len().min(4)]))
    })
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
/// an EXIF directory (IFD0 and every IFD chained after it, ExifIFD, GPS,
/// InteropIFD, a SubIFD) or of the `EXIF` family, any row the reader filed
/// under family-0 `EXIF`, or a maker-note row. An ungrouped name is one when
/// the tag registry defines it in EXIF or GPS, or when the file holds a row
/// of that name that is one: pinned ExifTool 13.59 writes a bare
/// `-Quality=Normal` into every maker note holding a `Quality` (both blocks
/// of `multi-app1-canon-nikon.jpg`). A trailing `#` (ExifTool's raw-value
/// marker) names the same tag.
fn is_exif_family_key(key: &str, baseline: &MetadataMap) -> bool {
    let key = key.strip_suffix('#').unwrap_or(key);
    let canonical = crate::writers::exif_surgical::canonical_write_key(key, baseline);
    if canonical.contains(':') {
        return is_exif_family_row(&canonical, baseline);
    }
    let registered = ["EXIF", "GPS"].iter().any(|group| {
        crate::tag_db::tag_registry::canonical_tag_name_spelling(group, key).is_some_and(|name| {
            crate::tag_db::tag_registry::get_tag_descriptor(&format!("{group}:{name}")).is_some()
        })
    });
    registered
        || baseline.keys().any(|row| {
            row.split_once(':')
                .is_some_and(|(_, name)| name.eq_ignore_ascii_case(key))
                && is_exif_family_row(row, baseline)
        })
}

/// Whether the grouped key `key` is EXIF-family (see [`is_exif_family_key`]).
fn is_exif_family_row(key: &str, baseline: &MetadataMap) -> bool {
    let Some((group, _)) = key.split_once(':') else {
        return false;
    };
    is_exif_directory_group(group)
        || baseline.group0_of(key) == Some("EXIF")
        || crate::writers::exif_surgical::is_makernote_row(baseline, key)
}

/// An EXIF directory's family-1 group, or the `EXIF` family itself, in any
/// case. The chain past IFD1 counts: the reader files a Leica JPEG's preview
/// rows under `IFD2`, and a SubIFD may be numbered (`SubIFD1`).
fn is_exif_directory_group(group: &str) -> bool {
    let numbered = |prefix: &str| {
        group
            .get(..prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(prefix))
            .map(|_| &group[prefix.len()..])
            .is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
    };
    ["ExifIFD", "GPS", "InteropIFD", "EXIF"]
        .iter()
        .any(|name| group.eq_ignore_ascii_case(name))
        || (numbered("IFD") && group.len() > "IFD".len())
        || numbered("SubIFD")
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

    /// One JPEG segment: marker, length, payload.
    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, marker];
        out.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// A JPEG of `segments`, then SOS and a scan.
    fn jpeg(segments: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for segment in segments {
            out.extend_from_slice(segment);
        }
        out.extend_from_slice(&segment(0xDA, &[1, 1, 0, 0, 0x3F, 0]));
        out.extend_from_slice(&[0x12, 0x34, 0xFF, 0xD9]);
        out
    }

    fn exif(identifier: &[u8]) -> Vec<u8> {
        let mut payload = identifier.to_vec();
        payload.extend_from_slice(b"MM\0\x2a\0\0\0\x08\0\0\0\0\0\0");
        segment(0xE1, &payload)
    }

    /// The count pinned ExifTool 13.59's pre-scan makes: fill bytes and
    /// garbage between segments are skipped, `/^(.{0,4})Exif\0./is`
    /// identifies a record, an ExtendedEXIF continuation is part of the
    /// record before it, and nothing after SOS counts.
    #[test]
    fn exif_records_are_counted_as_exiftool_prescans_them() {
        let canonical = exif(b"Exif\0\0");
        let dqt = segment(0xDB, &[0; 5]);
        assert_eq!(exif_app1_records(&jpeg(&[canonical.clone()])), 1);
        assert_eq!(
            exif_app1_records(&jpeg(&[canonical.clone(), canonical.clone()])),
            2
        );
        // Fill bytes before the second record, garbage after the first.
        let mut padded = canonical.clone();
        padded.extend_from_slice(&[0xFF, 0xFF]);
        assert_eq!(
            exif_app1_records(&jpeg(&[padded.clone(), canonical.clone()])),
            2
        );
        let mut garbage = canonical.clone();
        garbage.extend_from_slice(&[0x00, 0x11]);
        assert_eq!(exif_app1_records(&jpeg(&[garbage, canonical.clone()])), 2);
        // Identifiers ExifTool accepts.
        for identifier in [
            &b"Exif\0\x01"[..],
            b"exif\0\0",
            b"EXIF\0\0",
            b"junkExif\0\0",
        ] {
            assert_eq!(
                exif_app1_records(&jpeg(&[canonical.clone(), exif(identifier)])),
                2,
                "{identifier:?}"
            );
        }
        // ... and one it does not: five bytes of junk.
        assert_eq!(
            exif_app1_records(&jpeg(&[canonical.clone(), exif(b"junk!Exif\0\0")])),
            1
        );
        // An ExtendedEXIF continuation directly after the record is part of
        // it; after another segment it is a record of its own.
        let continuation = segment(0xE1, b"Exif\0\0\x01\x02\x03\x04");
        assert_eq!(
            exif_app1_records(&jpeg(&[canonical.clone(), continuation.clone()])),
            1
        );
        assert_eq!(
            exif_app1_records(&jpeg(&[canonical.clone(), dqt, continuation])),
            2
        );
        // A record after SOS is image data.
        let mut after_sos = jpeg(&[canonical.clone()]);
        after_sos.truncate(after_sos.len() - 2);
        after_sos.extend_from_slice(&canonical);
        assert_eq!(exif_app1_records(&after_sos), 1);
        assert_eq!(exif_app1_records(b"not a jpeg"), 0);
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

    /// The reader files rows of the IFD chain past IFD1 under `IFD2`,
    /// `IFD3`, ... (a Leica JPEG's preview rows): writes to them, or rows
    /// dropped from them, are EXIF-family whatever the case of the group
    /// and whether or not the file holds that row.
    #[test]
    fn chained_ifd_groups_are_exif_directories() {
        let baseline = map(&[("IFD2:PreviewImageStart", "7063524")]);
        for key in [
            "IFD2:PreviewImageStart",
            "IFD2:XResolution",
            "ifd2:xresolution",
            "IFD3:ImageWidth",
            "IFD12:ImageWidth",
            "SubIFD:ImageWidth",
            "SubIFD1:ImageWidth",
            "IFD2:All",
        ] {
            assert!(is_exif_family_key(key, &baseline), "{key}");
        }
        for key in ["IFD:ImageWidth", "IFDX:ImageWidth", "SubIFDs:ImageWidth"] {
            assert!(!is_exif_family_key(key, &baseline), "{key}");
        }
        let mut metadata = baseline.clone();
        metadata.remove("IFD2:PreviewImageStart");
        assert_eq!(
            exif_family_request_keys(&baseline, &metadata, &[], &[]),
            ["IFD2:PreviewImageStart"]
        );
    }

    /// A bare name the EXIF and GPS registries do not define is still an
    /// EXIF-family key when the file holds a maker-note row of that name:
    /// pinned ExifTool 13.59 writes `-Quality=Normal` into every maker note
    /// holding a `Quality`. Without such a row the name is not one.
    #[test]
    fn a_bare_name_of_a_maker_note_row_is_exif_family() {
        let baseline = map(&[("Canon:Quality", "RAW"), ("XMP:Label", "x")]);
        for key in ["Quality", "quality", "Quality#"] {
            assert!(is_exif_family_key(key, &baseline), "{key}");
        }
        assert!(!is_exif_family_key("Label", &baseline));
        assert!(!is_exif_family_key("Quality", &MetadataMap::new()));
        let mut metadata = baseline.clone();
        metadata.insert("Quality", TagValue::new_string("Normal"));
        assert_eq!(
            exif_family_request_keys(&baseline, &metadata, &["quality".to_string()], &[]),
            ["Quality", "quality"]
        );
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
