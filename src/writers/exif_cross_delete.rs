//! ExifTool's cross-directory delete: writing an EXIF tag into IFD0 removes
//! the copy of it in ExifIFD, and writing it into ExifIFD removes the one in
//! IFD0.
//!
//! Pinned ExifTool 13.59 keeps one copy of a tag that may live in either of
//! the two directories (WriteExif.pl 13.59:17-23):
//!
//! ```perl
//! my %crossDelete = (
//!     ExifIFD => 'IFD0',
//!     IFD0    => 'ExifIFD',
//! );
//! ```
//!
//! While `WriteExif` rewrites one of those directories (`$dirName`), a tag
//! with no new value for `$dirName` but one for the other directory
//! (`$wrongDir = $crossDelete{$dirName}`) is deleted from `$dirName`
//! (WriteExif.pl 13.59:1156-1171 for a tag known by ID, :990-996 for one
//! whose `Condition` needs its value; the deletion itself at :1259). So
//! `-IFD0:CreateDate=...` on a JPEG whose ExifIFD holds CreateDate leaves
//! only `[IFD0] CreateDate`; `-ExifIFD:XResolution=300` drops
//! `[IFD0] XResolution`. The same code runs for every file type.
//!
//! The rule does not apply when (each measured with the pinned oracle; see
//! `tests/exif_cross_delete.rs`):
//!
//! - the same request also sets the tag in the other directory (it then has
//!   its own new value there): `-IFD0:CreateDate=a -ExifIFD:CreateDate=b`
//!   keeps both;
//! - the request deletes rather than sets (`-IFD0:CreateDate=`): "don't cross
//!   delete if specifically deleting from the other directory", :1165-1169;
//! - the tag is one of the directory's mandatory entries
//!   (`%mandatory`, WriteExif.pl 13.59:26-54, checked at :1156): IFD0's
//!   YCbCrPositioning, ExifIFD's ExifVersion, ComponentsConfiguration and
//!   ColorSpace;
//! - the directories are any other pair: `%crossDelete` names only IFD0 and
//!   ExifIFD, so IFD1, GPS and InteropIFD copies are never touched.
//!
//! A set whose value the file already holds still deletes the other copy
//! (`IsOverwriting` is true for a plain assignment, Writer.pl 13.59:3676-3681).
//!
//! This module turns the rule into deletions every EXIF writer already
//! makes: a row taken out of the caller's map, or, for a copy the reader
//! reports no row for (it shows one of the two), a named removal. The
//! transaction's post-write checks then prove the copy is gone, and a writer
//! that cannot delete it refuses the whole write rather than keep it (the
//! generated IFD0 writer does not admit the `ExifIFD:` qualifier of, say,
//! Artist, so `-IFD0:Artist=x` on a file whose ExifIFD holds Artist is
//! refused).
//!
//! Not covered: a date shift (ExifTool shifts the other copy too, :1259;
//! oxidex refuses a grouped shift), an absolute `-ModifyDate=`/
//! `-EXIF:ModifyDate=`, which the CLI still sends down its date-shift path,
//! and an ungrouped name the write path has not resolved to IFD0/ExifIFD.

use crate::core::metadata_map::MetadataMap;
use crate::writers::exif_surgical::{IfdKind, canonical_write_key, descriptor_tag_id};
use crate::writers::generated_mandatory_defaults::MANDATORY_DEFAULTS;
use crate::writers::generated_setnewvalue_address_rules::SET_NEW_VALUE_LOOKUP;
use std::path::Path;

const IFD0: &str = "IFD0";
const EXIF_IFD: &str = "ExifIFD";

/// `%crossDelete` (WriteExif.pl 13.59:20-23).
fn cross_directory(directory: &str) -> Option<&'static str> {
    match directory {
        IFD0 => Some(EXIF_IFD),
        EXIF_IFD => Some(IFD0),
        _ => None,
    }
}

/// The file types whose EXIF writers delete an IFD0/ExifIFD row the map
/// drops: JPEG (APP1), PNG (`eXIf`) and TIFF. Elsewhere the rule is left
/// alone rather than turned into a deletion some writer cannot make.
const FILE_TYPES: &[&str] = &["JPEG", "PNG", "TIFF"];

/// `Exif::Main`'s write group for `name` when the pinned `FindTagInfo`
/// capture has writable `Exif::Main` candidates for it that all agree: the
/// candidate's `WriteGroup`, else the table's `WRITE_GROUP => 'ExifIFD'`
/// (Exif.pm 13.59:415). An `EXIF:<name>` set is written there.
fn exif_main_write_group(name: &str) -> Option<&'static str> {
    let mut groups = SET_NEW_VALUE_LOOKUP
        .iter()
        .filter(|candidate| {
            candidate.module == Some("Exif")
                && candidate.table == Some("Main")
                && candidate.writable.is_some()
                && candidate.name.eq_ignore_ascii_case(name)
        })
        .map(|candidate| candidate.write_group.unwrap_or(EXIF_IFD));
    let first = groups.next()?;
    groups.all(|group| group == first).then_some(first)
}

/// The directory a set of `key` (canonical) writes to, and the tag's name:
/// its group for `IFD0:`/`ExifIFD:`, `Exif::Main`'s write group for the
/// family-0 alias `EXIF:`.
fn written_directory(key: &str) -> Option<(&'static str, &str)> {
    let (group, name) = key.split_once(':')?;
    let directory = match group {
        IFD0 => IFD0,
        EXIF_IFD => EXIF_IFD,
        "EXIF" => exif_main_write_group(name)?,
        _ => return None,
    };
    cross_directory(directory).map(|_| (directory, name))
}

/// Whether `tag_id` is one of `directory`'s mandatory entries, which
/// `WriteExif` never cross-deletes (WriteExif.pl 13.59:1156).
fn is_mandatory(directory: &str, tag_id: u16) -> bool {
    MANDATORY_DEFAULTS
        .directories
        .iter()
        .filter(|entry| entry.directory == directory)
        .flat_map(|entry| entry.defaults.iter())
        .any(|default| default.tag_id == tag_id)
}

/// The copies pinned ExifTool 13.59 deletes when it applies the sets of
/// this request: every IFD0/ExifIFD tag the request sets in the other of the
/// two directories and not in its own.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CrossDeletions {
    /// Rows of the baseline map: deleted by leaving them out of the map.
    pub rows: Vec<String>,
    /// Entries the file holds but the reader surfaced no row for (when both
    /// directories hold a tag the reader reports only one copy): deleted by
    /// naming them (`exif_surgical::removal_names_rowless_entry`).
    pub rowless: Vec<String>,
}

/// [`CrossDeletions`] of a request, given `held`, whether the file's EXIF
/// holds an entry (directory, tag ID) -- asked only of a copy with no row.
///
/// A set is a key of `assigned` (set whatever its value) or of `desired`
/// whose value differs from `baseline`'s. `siblings` are further keys set by
/// the same command in other transactions (the CLI applies `-TAG=VALUE`
/// requests one at a time); they count as sets for the "also set there"
/// test only.
pub(crate) fn cross_deletions(
    baseline: &MetadataMap,
    desired: &MetadataMap,
    assigned: &[String],
    siblings: &[String],
    held: &mut dyn FnMut(IfdKind, u16) -> bool,
) -> CrossDeletions {
    let mut deletions = CrossDeletions::default();
    let file_type = baseline
        .get("File:FileType")
        .and_then(|value| value.as_string());
    if !file_type.is_some_and(|file_type| FILE_TYPES.contains(&file_type)) {
        return deletions;
    }
    let canonical = |key: &str| canonical_write_key(key, baseline);
    let sets: Vec<String> = assigned
        .iter()
        .map(|key| canonical(key))
        .chain(
            desired
                .iter()
                .filter(|(key, value)| baseline.get(key) != Some(value))
                .map(|(key, _)| canonical(key)),
        )
        .collect();
    let siblings: Vec<String> = siblings.iter().map(|key| canonical(key)).collect();
    let written: Vec<(&str, String)> = sets
        .iter()
        .chain(siblings.iter())
        .filter_map(|key| written_directory(key))
        .map(|(directory, name)| (directory, name.to_ascii_lowercase()))
        .collect();
    for (directory, name) in sets.iter().filter_map(|key| written_directory(key)) {
        let Some(other) = cross_directory(directory) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if written
            .iter()
            .any(|(written_in, written_name)| *written_in == other && *written_name == lower)
        {
            continue;
        }
        let wanted = format!("{other}:{name}");
        let Some(tag_id) =
            crate::tag_db::tag_registry::get_tag_descriptor(&wanted).and_then(descriptor_tag_id)
        else {
            continue;
        };
        if is_mandatory(other, tag_id) {
            continue;
        }
        if let Some(row) = baseline
            .keys()
            .find(|key| key.eq_ignore_ascii_case(&wanted))
        {
            if !deletions.rows.contains(row) {
                deletions.rows.push(row.clone());
            }
            continue;
        }
        let kind = if other == IFD0 {
            IfdKind::Ifd0
        } else {
            IfdKind::ExifIfd
        };
        if held(kind, tag_id) && !deletions.rowless.contains(&wanted) {
            deletions.rowless.push(wanted);
        }
    }
    deletions
}

/// The (directory, tag ID) of every entry of the EXIF block of the file at
/// `path`, a `file_type` of [`FILE_TYPES`]: the JPEG's first EXIF APP1, the
/// PNG's first `eXIf` chunk, or the TIFF itself. Empty when there is none or
/// it cannot be read (the writers then refuse or carry it on their own).
fn held_entries(path: &Path, file_type: &str) -> Vec<(IfdKind, u16)> {
    use crate::core::FileReader;
    use crate::writers::exif_surgical::{
        EXIF_BLOCK_MAGICS, jpeg_exif_payload, scan_entries_with_magics,
    };
    let Ok(reader) = crate::io::MMapReader::new(path) else {
        return Vec::new();
    };
    let Ok(bytes) = reader.read(0, reader.size() as usize) else {
        return Vec::new();
    };
    let scan = match file_type {
        "JPEG" => jpeg_exif_payload(bytes)
            .ok()
            .flatten()
            .and_then(|tiff| scan_entries_with_magics(&tiff, EXIF_BLOCK_MAGICS).ok()),
        "PNG" => png_exif_payload(bytes)
            .and_then(|tiff| scan_entries_with_magics(tiff, EXIF_BLOCK_MAGICS).ok()),
        "TIFF" => {
            scan_entries_with_magics(bytes, crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS)
                .ok()
        }
        _ => None,
    };
    scan.map(|scan| {
        scan.entries
            .iter()
            .map(|entry| (entry.ifd, entry.tag_id))
            .collect()
    })
    .unwrap_or_default()
}

/// The TIFF payload of a PNG's first `eXIf` chunk, less any improper
/// `Exif\0\0` header (PNG.pm 13.59:1367-1372).
fn png_exif_payload(bytes: &[u8]) -> Option<&[u8]> {
    let mut at = bytes
        .strip_prefix(b"\x89PNG\r\n\x1a\n".as_slice())
        .map(|_| 8)?;
    while let Some(header) = bytes.get(at..at + 8) {
        let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let data = bytes.get(at + 8..(at + 8).checked_add(len)?)?;
        if &header[4..8] == b"eXIf" {
            return Some(data.strip_prefix(b"Exif\0\0".as_slice()).unwrap_or(data));
        }
        at = at + 12 + len;
    }
    None
}

/// The request with [`CrossDeletions`] applied: `desired` without the rows,
/// `removed` naming the rowless copies. `None` when there is nothing to
/// delete.
pub(crate) fn with_cross_deletions(
    path: &Path,
    baseline: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
    assigned: &[String],
    siblings: &[String],
) -> Option<(MetadataMap, Vec<String>)> {
    let mut entries: Option<Vec<(IfdKind, u16)>> = None;
    let mut held = |ifd: IfdKind, tag_id: u16| {
        entries
            .get_or_insert_with(|| {
                let file_type = baseline
                    .get("File:FileType")
                    .and_then(|value| value.as_string())
                    .unwrap_or_default();
                held_entries(path, file_type)
            })
            .contains(&(ifd, tag_id))
    };
    let deletions = cross_deletions(baseline, desired, assigned, siblings, &mut held);
    if deletions == CrossDeletions::default() {
        return None;
    }
    let mut map = desired.clone();
    for row in &deletions.rows {
        map.remove(row);
    }
    let mut removed = removed.to_vec();
    removed.extend(deletions.rowless);
    Some((map, removed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;

    fn map(rows: &[(&str, &str)]) -> MetadataMap {
        let mut map = MetadataMap::new();
        for (key, value) in rows {
            map.insert(*key, TagValue::new_string(*value));
        }
        map
    }

    fn jpeg(rows: &[(&str, &str)]) -> MetadataMap {
        let mut all = vec![("File:FileType", "JPEG")];
        all.extend_from_slice(rows);
        map(&all)
    }

    fn with(base: &MetadataMap, key: &str, value: &str) -> MetadataMap {
        let mut map = base.clone();
        map.insert(key, TagValue::new_string(value));
        map
    }

    /// The rows deleted, with nothing held beyond them.
    fn rows(
        base: &MetadataMap,
        desired: &MetadataMap,
        assigned: &[&str],
        siblings: &[&str],
    ) -> Vec<String> {
        let assigned: Vec<String> = assigned.iter().map(|key| key.to_string()).collect();
        let siblings: Vec<String> = siblings.iter().map(|key| key.to_string()).collect();
        let deletions = cross_deletions(base, desired, &assigned, &siblings, &mut |_, _| false);
        assert!(deletions.rowless.is_empty());
        deletions.rows
    }

    #[test]
    fn a_set_in_one_directory_deletes_the_other_copy() {
        let base = jpeg(&[
            ("IFD0:ModifyDate", "2003:12:04 06:46:52"),
            ("ExifIFD:CreateDate", "2003:12:04 06:46:52"),
        ]);
        let desired = with(&base, "IFD0:CreateDate", "2020:01:02 03:04:05");
        assert_eq!(rows(&base, &desired, &[], &[]), ["ExifIFD:CreateDate"]);
        let desired = with(&base, "ExifIFD:ModifyDate", "2020:01:02 03:04:05");
        assert_eq!(rows(&base, &desired, &[], &[]), ["IFD0:ModifyDate"]);
    }

    #[test]
    fn a_copy_the_reader_reports_no_row_for_is_named() {
        // Both directories hold CreateDate; the reader reports IFD0's only.
        let base = jpeg(&[("IFD0:CreateDate", "2001:01:01 01:01:01")]);
        let mut asked = Vec::new();
        let deletions = cross_deletions(
            &base,
            &base,
            &["IFD0:CreateDate".into()],
            &[],
            &mut |ifd, id| {
                asked.push((ifd, id));
                true
            },
        );
        assert_eq!(asked, [(IfdKind::ExifIfd, 0x9004)]);
        assert_eq!(
            deletions,
            CrossDeletions {
                rows: Vec::new(),
                rowless: vec!["ExifIFD:CreateDate".into()],
            }
        );
        // ... and nothing when the file does not hold it either.
        let deletions = cross_deletions(
            &base,
            &base,
            &["IFD0:CreateDate".into()],
            &[],
            &mut |_, _| false,
        );
        assert_eq!(deletions, CrossDeletions::default());
    }

    #[test]
    fn a_same_value_set_counts_when_assigned() {
        let base = jpeg(&[
            ("IFD0:CreateDate", "2001:01:01 01:01:01"),
            ("ExifIFD:CreateDate", "2002:02:02 02:02:02"),
        ]);
        assert!(rows(&base, &base, &[], &[]).is_empty());
        assert_eq!(
            rows(&base, &base, &["IFD0:CreateDate"], &[]),
            ["ExifIFD:CreateDate"]
        );
    }

    #[test]
    fn a_set_of_both_copies_keeps_both() {
        let base = jpeg(&[("ExifIFD:CreateDate", "2003:12:04 06:46:52")]);
        let desired = with(
            &with(&base, "IFD0:CreateDate", "2020:01:02 03:04:05"),
            "ExifIFD:CreateDate",
            "2021:01:01 00:00:00",
        );
        assert!(rows(&base, &desired, &[], &[]).is_empty());
        // ... also when the other set is the same command's next request.
        let desired = with(&base, "IFD0:CreateDate", "2020:01:02 03:04:05");
        assert!(rows(&base, &desired, &[], &["ExifIFD:CreateDate"]).is_empty());
    }

    #[test]
    fn other_directories_and_mandatory_entries_are_left_alone() {
        let base = jpeg(&[
            ("IFD0:XResolution", "180"),
            ("IFD1:XResolution", "72"),
            ("IFD0:YCbCrPositioning", "Centered"),
            ("InteropIFD:InteropIndex", "R98"),
            ("ExifIFD:ExifVersion", "0221"),
        ]);
        for (key, value) in [
            ("IFD1:XResolution", "300"),
            ("GPS:GPSAltitude", "100"),
            ("IFD0:InteropIndex", "R03"),
            ("ExifIFD:YCbCrPositioning", "Co-sited"),
            ("IFD0:ExifVersion", "0230"),
        ] {
            // Only the rows above are held: no ExifIFD InteropIndex, say.
            let desired = with(&base, key, value);
            let deletions = cross_deletions(&base, &desired, &[], &[], &mut |_, _| false);
            assert_eq!(deletions, CrossDeletions::default(), "{key}");
        }
    }

    #[test]
    fn the_family_0_alias_writes_the_tags_write_group() {
        let base = jpeg(&[
            ("IFD0:CreateDate", "2001:01:01 01:01:01"),
            ("ExifIFD:CreateDate", "2002:02:02 02:02:02"),
        ]);
        let desired = with(&base, "EXIF:CreateDate", "2020:01:02 03:04:05");
        assert_eq!(rows(&base, &desired, &[], &[]), ["IFD0:CreateDate"]);
        assert_eq!(exif_main_write_group("ModifyDate"), Some(IFD0));
        assert_eq!(exif_main_write_group("CreateDate"), Some(EXIF_IFD));
    }

    #[test]
    fn other_file_types_are_left_alone() {
        let base = map(&[
            ("File:FileType", "DNG"),
            ("ExifIFD:CreateDate", "2003:12:04 06:46:52"),
        ]);
        let desired = with(&base, "IFD0:CreateDate", "2020:01:02 03:04:05");
        let deletions = cross_deletions(&base, &desired, &[], &[], &mut |_, _| true);
        assert_eq!(deletions, CrossDeletions::default());
    }

    #[test]
    fn the_png_exif_payload_is_found_past_other_chunks() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, data) in [
            (&b"IHDR"[..], &b"0123456789abc"[..]),
            (b"eXIf", b"Exif\0\0MM\0*"),
        ] {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            png.extend_from_slice(&[0; 4]);
        }
        assert_eq!(png_exif_payload(&png), Some(&b"MM\0*"[..]));
        assert_eq!(png_exif_payload(b"not a png"), None);
    }
}
