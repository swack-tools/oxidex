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
//! An ungrouped name is followed where it is written: to its EXIF directory
//! for the dates of [`date_set_keys`] (`-ModifyDate=`, `-AllDates=`, ...,
//! which refuses the ones ExifTool also writes outside EXIF), and to the
//! directory the generated writer resolves it to otherwise (`-Artist=` is
//! IFD0's). Not covered: a date shift (ExifTool shifts the other copy too,
//! :1259; oxidex refuses a grouped shift).

use crate::core::metadata_map::MetadataMap;
use crate::error::{ExifToolError, Result};
use crate::writers::exif_surgical::{IfdKind, canonical_write_key, descriptor_tag_id};
use crate::writers::generated_mandatory_defaults::MANDATORY_DEFAULTS;
use crate::writers::generated_setnewvalue_address_rules::SET_NEW_VALUE_LOOKUP;
use crate::writers::generated_write_address::Resolution;
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
    let Some((group, name)) = key.split_once(':') else {
        return generated_directory(key).map(|directory| (directory, key));
    };
    let directory = match group {
        IFD0 => IFD0,
        EXIF_IFD => EXIF_IFD,
        "EXIF" => exif_main_write_group(name)?,
        _ => return None,
    };
    cross_directory(directory).map(|_| (directory, name))
}

/// How the generated public writer resolves `key` (`generated_public_write::
/// resolve_public`, the resolver every public write goes through).
fn generated_resolution(key: &str) -> Resolution<'static> {
    crate::writers::generated_public_write::resolve_public(
        key,
        &crate::writers::generated_write_address::generated_rules(),
        crate::writers::generated_setnewvalue_public_migration_rules::PUBLIC_SET_NEW_VALUE_MIGRATIONS,
    )
}

/// IFD0 or ExifIFD when the generated writer writes the ungrouped name
/// `name` there (ungrouped `Artist` is IFD0's), else `None`.
fn generated_directory(name: &str) -> Option<&'static str> {
    match generated_resolution(name) {
        Resolution::Resolved(row) => [IFD0, EXIF_IFD]
            .into_iter()
            .find(|directory| row.selected_group == *directory),
        _ => None,
    }
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
    // An ungrouped name in a PNG is written to PNG's own chunks first
    // (13.59: `-Artist=x` on t/images PNG.png writes `[PNG] Artist`), never
    // a move between IFD0 and ExifIFD.
    let ungrouped_is_exif = file_type != Some("PNG");
    let sets: Vec<String> = assigned
        .iter()
        .map(|key| canonical(key))
        .chain(
            desired
                .iter()
                .filter(|(key, value)| baseline.get(key) != Some(value))
                .map(|(key, _)| canonical(key)),
        )
        .filter(|key| ungrouped_is_exif || key.contains(':'))
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
///
/// # Errors
///
/// A copy to delete that no writer here can delete: one the generated
/// public writer owns but does not address in that directory (the
/// `ExifIFD:` copy of an IFD0 tag such as Artist or XResolution). The write
/// is refused, naming the copy, rather than made with the copy kept.
pub(crate) fn with_cross_deletions(
    path: &Path,
    baseline: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
    assigned: &[String],
    siblings: &[String],
) -> Result<Option<(MetadataMap, Vec<String>)>> {
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
        return Ok(None);
    }
    if let Some(key) = deletions
        .rows
        .iter()
        .chain(deletions.rowless.iter())
        .find(|key| matches!(generated_resolution(key), Resolution::Unsupported(_)))
    {
        return Err(ExifToolError::unsupported_format(format!(
            "Cannot write this tag while the file holds '{key}': pinned ExifTool 13.59 \
             deletes that copy when the tag is set in the other of IFD0/ExifIFD \
             (WriteExif.pl %crossDelete), and this writer cannot delete '{key}'; \
             nothing was written"
        )));
    }
    let mut map = desired.clone();
    for row in &deletions.rows {
        map.remove(row);
    }
    let mut removed = removed.to_vec();
    removed.extend(deletions.rowless);
    Ok(Some((map, removed)))
}

/// Groups a date row may sit in without pinned ExifTool 13.59 writing it on
/// an ungrouped set: the EXIF directories, and the groups the reader derives.
const EXIF_OR_DERIVED_GROUPS: &[&str] = &[
    "IFD0",
    "ExifIFD",
    "IFD1",
    "InteropIFD",
    "GPS",
    "EXIF",
    "File",
    "System",
    "Composite",
    "ExifTool",
];

/// The EXIF keys an absolute date set `tag` (ungrouped or `EXIF:`) writes in
/// the file whose rows are `baseline`, as pinned ExifTool 13.59 writes it:
/// `ModifyDate` to IFD0, `CreateDate` and `DateTimeOriginal` to ExifIFD, and
/// the ungrouped `AllDates` shortcut to all three -- each moving its other
/// IFD0/ExifIFD copy ([`with_cross_deletions`]). `None` when `tag` is none
/// of these, or the file is not a JPEG, TIFF or PNG (the caller keeps its
/// own route).
///
/// # Errors
///
/// An ungrouped set ExifTool also writes outside EXIF, which this route does
/// not: in a PNG, `ModifyDate`, `CreateDate` and `AllDates` go to PNG's own
/// `tIME`/text chunks (13.59 writes `[PNG] ModifyDate`); in a JPEG or TIFF, a
/// file with an MIE trailer or a non-EXIF row of the same name (XMP, CIFF,
/// ...) gets that copy updated too (13.59 on t/images ExifTool.jpg:
/// `-DateTimeOriginal=` also writes `[CIFF]` and `[MIE-Doc]`). Refused,
/// naming the copy; `-EXIF:<name>=` writes EXIF alone.
pub(crate) fn date_set_keys(
    baseline: &MetadataMap,
    tag: &str,
) -> Result<Option<Vec<&'static str>>> {
    let (grouped, name) = match tag.split_once(':') {
        None => (false, tag),
        Some((group, name)) if group.eq_ignore_ascii_case("EXIF") => (true, name),
        Some(_) => return Ok(None),
    };
    let lower = name.to_ascii_lowercase();
    let keys: &[&'static str] = match lower.as_str() {
        "alldates" if !grouped => &[
            "ExifIFD:DateTimeOriginal",
            "ExifIFD:CreateDate",
            "IFD0:ModifyDate",
        ],
        "modifydate" => &["IFD0:ModifyDate"],
        "createdate" => &["ExifIFD:CreateDate"],
        "datetimeoriginal" => &["ExifIFD:DateTimeOriginal"],
        _ => return Ok(None),
    };
    let file_type = baseline
        .get("File:FileType")
        .and_then(|value| value.as_string());
    let refuse = |copy: &str| {
        let instead = if lower == "alldates" {
            "-EXIF:DateTimeOriginal=, -EXIF:CreateDate= and -EXIF:ModifyDate=".to_string()
        } else {
            format!("-EXIF:{name}=")
        };
        Err(ExifToolError::unsupported_format(format!(
            "Cannot write '{tag}' ungrouped here: pinned ExifTool 13.59 also writes {copy}, \
             which this writer does not; name the EXIF group ({instead}) to write EXIF \
             alone. Nothing was written"
        )))
    };
    match file_type {
        Some("JPEG" | "TIFF") => {}
        // 13.59 on t/images PDF.pdf: `-AllDates=` writes PDF:CreateDate,
        // PDF:ModifyDate and their XMP copies; the date path wrote nothing
        // and reported success.
        Some("PDF") if lower == "alldates" => {
            return Err(ExifToolError::unsupported_format(format!(
                "Cannot write '{tag}' in a PDF: pinned ExifTool 13.59 writes PDF:CreateDate, \
                 PDF:ModifyDate and their XMP copies, which this route does not. Nothing was \
                 written"
            )));
        }
        Some("PNG") if grouped || lower == "datetimeoriginal" => {}
        Some("PNG") => {
            let chunk = if lower == "alldates" {
                "PNG:CreateDate and PNG:ModifyDate".to_string()
            } else {
                format!("PNG:{name}")
            };
            return refuse(&chunk);
        }
        _ => return Ok(None),
    }
    if !grouped {
        let names: Vec<&str> = keys
            .iter()
            .filter_map(|key| key.split_once(':').map(|(_, name)| name))
            .collect();
        for key in baseline.keys() {
            let Some((group, row_name)) = key.split_once(':') else {
                continue;
            };
            let same_name = names.iter().any(|n| n.eq_ignore_ascii_case(row_name));
            if group.starts_with("MIE") {
                return refuse("its MIE trailer's copy");
            }
            if same_name && !EXIF_OR_DERIVED_GROUPS.contains(&group) {
                return refuse(&format!("'{key}'"));
            }
        }
    }
    Ok(Some(keys.to_vec()))
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

    fn typed(file_type: &str, rows: &[(&str, &str)]) -> MetadataMap {
        let mut all = vec![("File:FileType", file_type)];
        all.extend_from_slice(rows);
        map(&all)
    }

    #[test]
    fn date_sets_are_written_to_their_exif_directories() {
        let jpeg = typed("JPEG", &[("IFD0:ModifyDate", "2003:12:04 06:46:52")]);
        let keys = |base: &MetadataMap, tag: &str| date_set_keys(base, tag).unwrap();
        assert_eq!(keys(&jpeg, "ModifyDate"), Some(vec!["IFD0:ModifyDate"]));
        assert_eq!(
            keys(&jpeg, "EXIF:ModifyDate"),
            Some(vec!["IFD0:ModifyDate"])
        );
        assert_eq!(keys(&jpeg, "CreateDate"), Some(vec!["ExifIFD:CreateDate"]));
        assert_eq!(
            keys(&jpeg, "AllDates"),
            Some(vec![
                "ExifIFD:DateTimeOriginal",
                "ExifIFD:CreateDate",
                "IFD0:ModifyDate"
            ])
        );
        // Not a date this route owns, a group it does not, or another type.
        assert_eq!(keys(&jpeg, "Artist"), None);
        assert_eq!(keys(&jpeg, "XMP:ModifyDate"), None);
        assert_eq!(keys(&jpeg, "EXIF:AllDates"), None);
        assert_eq!(keys(&typed("MP4", &[]), "ModifyDate"), None);
        // PNG: EXIF only when named so, or for DateTimeOriginal.
        let png = typed("PNG", &[]);
        assert_eq!(keys(&png, "EXIF:ModifyDate"), Some(vec!["IFD0:ModifyDate"]));
        assert_eq!(
            keys(&png, "DateTimeOriginal"),
            Some(vec!["ExifIFD:DateTimeOriginal"])
        );
    }

    #[test]
    fn a_date_exiftool_also_writes_outside_exif_is_refused_by_name() {
        let refused = |base: &MetadataMap, tag: &str, named: &str| {
            let err = date_set_keys(base, tag).unwrap_err().to_string();
            assert!(err.contains(named), "{tag}: {err}");
        };
        let png = typed("PNG", &[]);
        refused(&png, "ModifyDate", "PNG:ModifyDate");
        refused(&png, "AllDates", "PNG:CreateDate");
        let ciff = typed("JPEG", &[("CIFF:DateTimeOriginal", "1998:05:01 21:33:18")]);
        refused(&ciff, "DateTimeOriginal", "CIFF:DateTimeOriginal");
        refused(&ciff, "AllDates", "CIFF:DateTimeOriginal");
        assert!(date_set_keys(&ciff, "EXIF:DateTimeOriginal").is_ok());
        let mie = typed("JPEG", &[("MIE-Doc:Title", "x")]);
        refused(&mie, "ModifyDate", "MIE");
        refused(&typed("PDF", &[]), "AllDates", "PDF:CreateDate");
    }

    #[test]
    fn an_ungrouped_generated_name_writes_its_resolved_directory() {
        assert_eq!(generated_directory("Artist"), Some(IFD0));
        assert_eq!(generated_directory("NotATag"), None);
        let base = jpeg(&[("ExifIFD:Artist", "Exif Artist")]);
        let desired = with(&base, "Artist", "New");
        assert_eq!(rows(&base, &desired, &[], &[]), ["ExifIFD:Artist"]);
        // ... but not in a PNG, where ExifTool writes PNG:Artist.
        let mut png = base.clone();
        png.insert("File:FileType", TagValue::new_string("PNG"));
        let desired = with(&png, "Artist", "New");
        assert!(rows(&png, &desired, &[], &[]).is_empty());
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
