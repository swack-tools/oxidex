//! The maintainer's #951 API decisions (2026-09-24), each following pinned
//! ExifTool 13.59's own Perl API (ExifTool.pod):
//!
//! 1. `write_metadata` applies a row absent from the map as a deletion only
//!    when the map was read from this same file and the caller removed that
//!    row. A map built from scratch, or read from another file, only sets:
//!    ExifTool's SetNewValue model, where a tag nobody named is never
//!    deleted.
//! 2. `copy_metadata(src, dest, None)` is best-effort: every writable tag is
//!    copied, the rest skipped and reported (`copy_metadata_report`), and the
//!    call succeeds -- SetNewValuesFromFile with no tag list ("All writable
//!    tags are set if none are specified"). A tag the caller names that
//!    cannot be copied is refused by name (oxidex's fail-closed choice where
//!    ExifTool only warns).

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{
    copy_metadata, copy_metadata_report, read_metadata, write_metadata,
};
use oxidex::core::{MetadataMap, TagValue};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";
const TIFF: &str = "tests/fixtures/tiff/sample.tif";
const PNG: &str = "tests/fixtures/png/sample.png";

fn copy_into(dir: &TempDir, fixture: &Path, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).expect("read file")).to_vec()
}

/// The rows a file stores (not derived from it), for before/after checks.
fn stored_rows(path: &Path) -> Vec<(String, TagValue)> {
    read_metadata(path)
        .unwrap()
        .iter()
        .filter(|(key, _)| {
            !matches!(
                key.split_once(':').map(|(group, _)| group),
                Some("File" | "System" | "Composite" | "ExifTool")
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Decision 1: a from-scratch map naming one tag sets that tag and leaves
/// every other tag intact (ExifTool: `SetNewValue('Artist', 'x')` +
/// `WriteInfo` touches nothing else).
#[test]
fn a_from_scratch_map_with_one_set_leaves_every_other_tag_intact() {
    for (fixture, key) in [
        (JPEG, "IFD0:Artist"),
        (TIFF, "IFD0:Artist"),
        (PNG, "IFD0:Artist"),
    ] {
        let dir = TempDir::new().unwrap();
        let name = Path::new(fixture).file_name().unwrap().to_str().unwrap();
        let file = copy_into(&dir, Path::new(fixture), name);
        let before = stored_rows(&file);
        let mut map = MetadataMap::new();
        map.insert(key, TagValue::new_string("someone new"));
        write_metadata(&file, &map).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        let after = stored_rows(&file);
        for (row, value) in &before {
            if row == key {
                continue;
            }
            assert_eq!(
                after.iter().find(|(k, _)| k == row).map(|(_, v)| v),
                Some(value),
                "{fixture}: {row} was not named, yet changed or vanished"
            );
        }
        assert_eq!(
            read_metadata(&file).unwrap().get(key),
            Some(&TagValue::new_string("someone new")),
            "{fixture}: the one set"
        );
    }
}

/// Decision 1: a map read from this file with a row removed deletes that row.
#[test]
fn a_read_map_with_a_row_removed_deletes_that_tag() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, Path::new(JPEG), "a.jpg");
    let mut map = read_metadata(&file).unwrap();
    assert!(map.remove("IFD0:Artist").is_some(), "fixture has an Artist");
    write_metadata(&file, &map).unwrap();
    let after = read_metadata(&file).unwrap();
    assert!(after.get("IFD0:Artist").is_none(), "Artist not deleted");
    assert!(
        after.get("IFD0:Make").is_some(),
        "an untouched row went too"
    );
}

/// Decision 1: a map read from a *different* file carries no deletion for the
/// destination: writing it only sets (`Metadata::from_path(src).write_to(dest)`).
#[test]
fn a_map_read_from_another_file_only_sets() {
    let dir = TempDir::new().unwrap();
    let source = copy_into(&dir, Path::new(JPEG), "src.jpg");
    let dest = copy_into(&dir, Path::new(JPEG_XMP), "dest.jpg");
    let before = stored_rows(&dest);
    let mut map = read_metadata(&source).unwrap();
    // Keep the request to one writable change: the destination's Make. The
    // caller removed every other row -- from the *source's* map.
    let others: Vec<String> = map.keys().filter(|k| *k != "IFD0:Make").cloned().collect();
    for key in others {
        map.remove(&key);
    }
    write_metadata(&dest, &map).unwrap();
    let after = stored_rows(&dest);
    for (row, value) in &before {
        if row == "IFD0:Make" {
            continue;
        }
        assert_eq!(
            after.iter().find(|(k, _)| k == row).map(|(_, v)| v),
            Some(value),
            "{row} of the destination was deleted or changed"
        );
    }
    assert_eq!(
        read_metadata(&dest).unwrap().get_string("IFD0:Make"),
        read_metadata(&source).unwrap().get_string("IFD0:Make")
    );
}

/// Decision 2: copy-all from a JPEG holding JFIF, ICC_Profile and XMP into a
/// JPEG succeeds, writes what the destination's writer can hold (the EXIF),
/// and reports the groups it skipped. Pinned t/images/XMP.jpg when present;
/// the in-repo EXIF+XMP fixture always.
#[test]
fn copy_all_is_best_effort_and_reports_what_it_skipped() {
    let mut sources: Vec<(PathBuf, Vec<&str>)> = vec![(PathBuf::from(JPEG_XMP), vec!["XMP"])];
    if let Some(xmp) = fixtures::pinned_t_images_fixture_path("XMP.jpg") {
        sources.push((xmp, vec!["JFIF", "ICC_Profile", "XMP"]));
    }
    for (source, skipped) in sources {
        let label = source.display().to_string();
        let source_make = read_metadata(&source)
            .unwrap()
            .get_string("IFD0:Make")
            .map(str::to_string);
        assert!(source_make.is_some(), "{label}: source has IFD0:Make");

        let dir = TempDir::new().unwrap();
        let dest = copy_into(&dir, Path::new(JPEG), "dest.jpg");
        copy_metadata(&source, &dest, None).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(
            read_metadata(&dest)
                .unwrap()
                .get_string("IFD0:Make")
                .map(str::to_string),
            source_make,
            "{label}: the writable EXIF was copied"
        );

        let dest = copy_into(&dir, Path::new(JPEG), "dest2.jpg");
        let report = copy_metadata_report(&source, &dest, None).unwrap();
        for group in skipped {
            assert!(
                report.uncopied_groups.iter().any(|g| g == group),
                "{label}: {group} not reported skipped: {:?}",
                report.uncopied_groups
            );
        }
        assert!(report.copied > 0, "{label}: nothing copied");
    }
}

/// Decision 2: naming a tag the destination cannot hold is refused by name,
/// the destination untouched.
#[test]
fn a_named_uncopyable_tag_is_refused_by_name() {
    let dir = TempDir::new().unwrap();
    let source = copy_into(&dir, Path::new(JPEG_XMP), "src.jpg");
    let dest = copy_into(&dir, Path::new(JPEG), "dest.jpg");
    let before = sha(&dest);
    let err = copy_metadata(
        &source,
        &dest,
        Some(&["XMP:Title".to_string(), "IFD0:Make".to_string()]),
    )
    .expect_err("XMP:Title cannot be copied into a JPEG by oxidex");
    assert!(err.to_string().contains("XMP:Title"), "{err}");
    assert_eq!(sha(&dest), before, "a refused copy changed the destination");
}
