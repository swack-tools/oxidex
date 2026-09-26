//! The second round of Codex review threads on the beta.1 roll-up (#957) at
//! 0d9af422, each pinned by a test that fails there. Oracle rows are pinned
//! ExifTool 13.59 (`perl5.38.2 -I<pinned>/lib <pinned>/exiftool`; probes
//! `-ver` = 13.59, `OOXML.docx` FileType = DOCX), re-measured by the
//! `oracle_*` tests through `exiftool_oracle::graded()`.

use oxidex::Metadata;
use oxidex::core::operations::{read_metadata, write_metadata};
use oxidex::core::{TagValue, WriteOutcome};
use oxidex::exiftool_oracle;
use oxidex::ffi::{
    EXIFTOOL_OK, exiftool_create, exiftool_destroy, exiftool_read_file, exiftool_remove_tag,
    exiftool_set_tag_string, exiftool_write_file,
};
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// `[IFD0] Make: TestCamera`, `Model: TM` and no ExifIFD, plus XMP.
const JPEG_NO_EXIF_IFD: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";

/// The rows pinned 13.59 seeds when a write creates an ExifIFD.
const SEEDED: &[&str] = &[
    "ExifIFD:ExifVersion",
    "ExifIFD:ComponentsConfiguration",
    "ExifIFD:ColorSpace",
];

fn copy_into(dir: &TempDir, fixture: &str, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// oxidex's `-s3 -KEY` read-back of `path` (`None` when absent).
fn get(path: &Path, key: &str) -> Option<String> {
    let o = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-s3", &format!("-{key}"), s(path)])
        .output()
        .expect("run oxidex");
    let value = String::from_utf8_lossy(&o.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn text(value: &str) -> TagValue {
    TagValue::new_string(value)
}

/// The pinned oracle's `-s3` read of `tag` (empty when absent).
fn oracle_value(oracle: &exiftool_oracle::Oracle, path: &Path, tag: &str) -> String {
    let o = oracle
        .command()
        .args(["-s3", &format!("-{tag}")])
        .arg(path)
        .output()
        .expect("run oracle");
    String::from_utf8_lossy(&o.stdout).trim_end().to_string()
}

fn oracle_write(oracle: &exiftool_oracle::Oracle, args: &[&str], path: &Path) {
    let o = oracle
        .command()
        .args(args)
        .arg(path)
        .output()
        .expect("run oracle");
    assert!(
        o.status.success(),
        "{} {args:?}: {}",
        oracle.display(),
        String::from_utf8_lossy(&o.stderr)
    );
}

// --- PRRT_kwDOQNbr5M6mO8E4: a map deletes only rows its read saw ----------

/// The rows the first write seeds (and the edits of both writes) are all in
/// the file after the second write from the same map.
fn assert_both_writes_kept(file: &Path, label: &str) {
    for key in SEEDED {
        assert!(
            get(file, key).is_some(),
            "{label}: second write removed {key}"
        );
    }
    assert_eq!(
        get(file, "ExifIFD:LensModel").as_deref(),
        Some("L1"),
        "{label}"
    );
    assert_eq!(get(file, "IFD0:Artist").as_deref(), Some("x"), "{label}");
    assert_eq!(
        get(file, "IFD0:Make").as_deref(),
        Some("TestCamera"),
        "{label}"
    );
}

/// A read map written twice: the first write creates the ExifIFD and pinned
/// 13.59 seeds ExifVersion, ComponentsConfiguration and ColorSpace with it;
/// the second compared the fresh file against the stale map and deleted
/// every seeded row as a caller "removal". The second write must keep them.
#[test]
fn a_second_write_from_one_map_keeps_what_the_first_seeded() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG_NO_EXIF_IFD, "map.jpg");
    let mut map = read_metadata(&file).unwrap();
    map.insert("ExifIFD:LensModel", text("L1"));
    assert_eq!(write_metadata(&file, &map).unwrap(), WriteOutcome::Updated);
    for key in SEEDED {
        assert!(get(&file, key).is_some(), "the first write seeds {key}");
    }
    map.insert("IFD0:Artist", text("x"));
    write_metadata(&file, &map).expect("second write");
    assert_both_writes_kept(&file, "write_metadata");

    // A removal after the first write is still a deletion: the map saw it.
    map.remove("IFD0:Model");
    write_metadata(&file, &map).expect("third write");
    assert_eq!(get(&file, "IFD0:Model"), None, "the removal was skipped");
    assert_both_writes_kept(&file, "write_metadata, third");
}

#[test]
fn a_second_save_keeps_what_the_first_seeded() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG_NO_EXIF_IFD, "save.jpg");
    let mut meta = Metadata::from_path(&file).unwrap();
    meta.insert("ExifIFD:LensModel", "L1");
    meta.save().expect("first save");
    meta.insert("IFD0:Artist", "x");
    meta.save().expect("second save");
    assert_both_writes_kept(&file, "Metadata::save");
}

struct Handle(*mut oxidex::ffi::ExifToolHandle);

impl Handle {
    fn read(path: &Path) -> Self {
        let handle = Handle(exiftool_create());
        let c = CString::new(s(path)).unwrap();
        assert_eq!(exiftool_read_file(handle.0, c.as_ptr()), EXIFTOOL_OK);
        handle
    }
    fn set(&self, tag: &str, value: &str) {
        let (tag, value) = (CString::new(tag).unwrap(), CString::new(value).unwrap());
        assert_eq!(
            exiftool_set_tag_string(self.0, tag.as_ptr(), value.as_ptr()),
            EXIFTOOL_OK
        );
    }
    fn remove(&self, tag: &str) {
        let tag = CString::new(tag).unwrap();
        assert_eq!(exiftool_remove_tag(self.0, tag.as_ptr()), EXIFTOOL_OK);
    }
    fn write(&self, path: &Path) -> i32 {
        let c = CString::new(s(path)).unwrap();
        exiftool_write_file(self.0, c.as_ptr())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        exiftool_destroy(self.0);
    }
}

#[test]
fn a_second_ffi_write_keeps_what_the_first_seeded() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG_NO_EXIF_IFD, "ffi.jpg");
    let handle = Handle::read(&file);
    handle.set("ExifIFD:LensModel", "L1");
    assert_eq!(handle.write(&file), EXIFTOOL_OK);
    handle.set("IFD0:Artist", "x");
    assert_eq!(handle.write(&file), EXIFTOOL_OK);
    assert_both_writes_kept(&file, "exiftool_write_file");
    handle.remove("IFD0:Model");
    assert_eq!(handle.write(&file), EXIFTOOL_OK);
    assert_eq!(get(&file, "IFD0:Model"), None, "the removal was skipped");
    assert_both_writes_kept(&file, "exiftool_write_file, third");
}

/// The same two edits made by two ExifTool commands leave the same EXIF.
#[test]
fn oracle_two_writes_from_one_map_match_two_commands() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let dir = TempDir::new().unwrap();
    let theirs = copy_into(&dir, JPEG_NO_EXIF_IFD, "theirs.jpg");
    oracle_write(oracle, &["-ExifIFD:LensModel=L1"], &theirs);
    oracle_write(oracle, &["-IFD0:Artist=x"], &theirs);
    let ours = copy_into(&dir, JPEG_NO_EXIF_IFD, "ours.jpg");
    let mut map = read_metadata(&ours).unwrap();
    map.insert("ExifIFD:LensModel", text("L1"));
    write_metadata(&ours, &map).unwrap();
    map.insert("IFD0:Artist", text("x"));
    write_metadata(&ours, &map).unwrap();
    for key in SEEDED.iter().chain(&[
        "ExifIFD:LensModel",
        "IFD0:Artist",
        "IFD0:Make",
        "IFD0:Model",
    ]) {
        assert_eq!(
            oracle_value(oracle, &ours, key),
            oracle_value(oracle, &theirs, key),
            "{key}"
        );
    }
}
