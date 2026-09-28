//! Every library write call reports what it did to the file, like ExifTool's
//! `WriteInfo` (ExifTool.pod: 1 = "file written OK", 2 = "file written but no
//! changes made"): `WriteOutcome::Updated` when the bytes changed,
//! `WriteOutcome::Unchanged` when every change was already in effect.
//! Maintainer decision on #951 (2026-09-24).

use oxidex::core::operations::{
    clear_all_metadata, copy_metadata, modify_tag, read_metadata, remove_tag, write_metadata,
};
use oxidex::core::{Metadata, TagValue, WriteOutcome};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const PNG: &str = "tests/fixtures/png/sample.png";

fn copy_into(dir: &TempDir, fixture: &str) -> PathBuf {
    let path = dir.path().join(Path::new(fixture).file_name().unwrap());
    fs::copy(fixture, &path).unwrap();
    path
}

/// A same-value set is `Unchanged` with the bytes identical, on a PNG (whose
/// IFD0:Artist is `PNG Artist 1`) and a JPEG (`Synthetic Artist 1`); a real
/// change is `Updated` -- through `write_metadata`, `modify_tag` and the
/// `Metadata` builder alike.
#[test]
fn same_value_no_op_is_unchanged_and_a_real_change_is_updated() {
    for fixture in [PNG, JPEG] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture);
        let original = fs::read(&file).unwrap();
        let artist = read_metadata(&file)
            .unwrap()
            .get("IFD0:Artist")
            .cloned()
            .expect("fixture has IFD0:Artist");

        let mut map = read_metadata(&file).unwrap();
        map.insert("IFD0:Artist", artist.clone());
        assert_eq!(
            write_metadata(&file, &map).unwrap(),
            WriteOutcome::Unchanged,
            "{fixture}"
        );
        assert_eq!(
            modify_tag(&file, "IFD0:Artist", artist.clone()).unwrap(),
            WriteOutcome::Unchanged,
            "{fixture}"
        );
        assert_eq!(
            Metadata::from_path(&file).unwrap().save().unwrap(),
            WriteOutcome::Unchanged,
            "{fixture}"
        );
        assert_eq!(
            fs::read(&file).unwrap(),
            original,
            "{fixture}: bytes changed"
        );

        assert_eq!(
            modify_tag(&file, "IFD0:Artist", TagValue::new_string("someone else")).unwrap(),
            WriteOutcome::Updated,
            "{fixture}"
        );
        assert_ne!(fs::read(&file).unwrap(), original, "{fixture}");

        let mut map = read_metadata(&file).unwrap();
        map.insert("IFD0:Artist", artist.clone());
        assert_eq!(
            write_metadata(&file, &map).unwrap(),
            WriteOutcome::Updated,
            "{fixture}"
        );
    }
}

/// Deleting what is not there, clearing what is already clear, and copying
/// what the destination already holds are `Unchanged`.
#[test]
fn no_op_deletions_clears_and_copies_are_unchanged() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG);
    assert_eq!(
        remove_tag(&file, "IFD0:XPTitle").unwrap(),
        WriteOutcome::Unchanged
    );
    assert_eq!(
        remove_tag(&file, "IFD0:Artist").unwrap(),
        WriteOutcome::Updated
    );
    let twin = dir.path().join("twin.jpg");
    fs::copy(&file, &twin).unwrap();
    assert_eq!(
        copy_metadata(&twin, &file, Some(&["IFD0:Make".to_string()])).unwrap(),
        WriteOutcome::Unchanged
    );
    assert_eq!(clear_all_metadata(&file).unwrap(), WriteOutcome::Updated);
    assert_eq!(clear_all_metadata(&file).unwrap(), WriteOutcome::Unchanged);
}
