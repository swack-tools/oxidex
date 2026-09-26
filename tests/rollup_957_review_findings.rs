//! The six Codex review threads on the beta.1 roll-up (#957) at f21af88a,
//! each pinned by a test that fails there. Oracle rows are pinned ExifTool
//! 13.59 (`perl5.38.2 -I<pinned>/lib <pinned>/exiftool`; probes `-ver` =
//! 13.59, `OOXML.docx` FileType = DOCX); the `oracle_*` tests re-measure
//! them through `exiftool_oracle::graded()`.

use oxidex::core::operations::{read_metadata, write_metadata};
use oxidex::core::tag_normalization::normalize_metadata_map;
use oxidex::exiftool_oracle;
use oxidex::ffi::{
    EXIFTOOL_OK, exiftool_create, exiftool_destroy, exiftool_read_file, exiftool_remove_tag,
    exiftool_set_tag_string, exiftool_write_file,
};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

/// `[IFD0]` Make/Model/Artist/..., `[ExifIFD]` ExifVersion/DateTimeOriginal.
const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
/// `[IFD0] Make: TestCamera`, `Model: TM`, plus XMP.
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";

fn copy_into(dir: &TempDir, fixture: &Path, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).expect("read file")).to_vec()
}

fn oxidex(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The groups of the EXIF directories `path` stores (oxidex's reader).
fn exif_groups(path: &Path) -> Vec<String> {
    let mut groups: Vec<String> = read_metadata(path)
        .unwrap()
        .keys()
        .filter_map(|key| key.split_once(':').map(|(group, _)| group.to_string()))
        .filter(|group| ["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD"].contains(&group.as_str()))
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

/// oxidex's `-s3 -KEY` read-back of `path` (`None` when absent).
fn get(path: &Path, key: &str) -> Option<String> {
    let o = oxidex(&["-s3", &format!("-{key}"), s(path)]);
    let value = out(&o).lines().next().unwrap_or_default().to_string();
    (!value.is_empty()).then_some(value)
}

/// The pinned oracle's `-s3` read of `tag` (empty when absent).
fn oracle_value(oracle: &exiftool_oracle::Oracle, path: &Path, tag: &str) -> String {
    let o = oracle
        .command()
        .args(["-s3", &format!("-{tag}")])
        .arg(path)
        .output()
        .expect("run oracle");
    out(&o).trim_end().to_string()
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
        err(&o)
    );
}

// --- PRRT_kwDOQNbr5M6mOo0t: -all= keeps its place in the argument order ----

/// 13.59 on synthetic_001.jpg: `-IFD0:Artist=x -all=` leaves no EXIF (the
/// later `-all=` removes the earlier value); `-all= -IFD0:Artist=x` leaves
/// Artist. oxidex applied the clear first whatever the order.
#[test]
fn a_set_before_all_is_superseded_by_it() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, Path::new(JPEG), "a.jpg");
    let o = oxidex(&["-IFD0:Artist=x", "-all=", s(&file)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "    1 image files updated\n");
    assert_eq!(exif_groups(&file), Vec::<String>::new(), "EXIF left");

    let file = copy_into(&dir, Path::new(JPEG), "b.jpg");
    let o = oxidex(&["-all=", "-IFD0:Artist=x", s(&file)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(get(&file, "IFD0:Artist").as_deref(), Some("x"));
}

/// 13.59: `-TagsFromFile SRC -all= DST` leaves DST without SRC's tags (the
/// copy is applied at its place, then cleared); `-all= -TagsFromFile SRC
/// DST` leaves SRC's Make/Model.
#[test]
fn tags_from_file_before_all_is_cleared_with_the_rest() {
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, Path::new(JPEG), "copy-then-clear.jpg");
    let o = oxidex(&["-TagsFromFile", JPEG_XMP, "-all=", s(&dst)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(exif_groups(&dst), Vec::<String>::new(), "EXIF left");

    let dst = copy_into(&dir, Path::new(JPEG), "clear-then-copy.jpg");
    let o = oxidex(&["-all=", "-TagsFromFile", JPEG_XMP, s(&dst)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(get(&dst, "IFD0:Make").as_deref(), Some("TestCamera"));
    assert_eq!(get(&dst, "IFD0:Model").as_deref(), Some("TM"));
    assert_eq!(get(&dst, "IFD0:Artist"), None, "the clear came first");
}

#[test]
fn oracle_orders_all_against_sets_and_copies() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let dir = TempDir::new().unwrap();
    for (args, key) in [
        (vec!["-IFD0:Artist=x", "-all="], "IFD0:Artist"),
        (vec!["-all=", "-IFD0:Artist=x"], "IFD0:Artist"),
        (vec!["-TagsFromFile", JPEG_XMP, "-all="], "IFD0:Make"),
        (vec!["-all=", "-TagsFromFile", JPEG_XMP], "IFD0:Make"),
    ] {
        let theirs = copy_into(&dir, Path::new(JPEG), "theirs.jpg");
        oracle_write(oracle, &args, &theirs);
        let ours = copy_into(&dir, Path::new(JPEG), "ours.jpg");
        let mut argv = args.clone();
        argv.push(s(&ours));
        let o = oxidex(&argv);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", err(&o));
        assert_eq!(
            get(&ours, key).unwrap_or_default(),
            oracle_value(oracle, &theirs, key),
            "{args:?}: {key}"
        );
        assert_eq!(
            exif_groups(&ours).is_empty(),
            oracle_value(oracle, &theirs, "EXIF:All").is_empty(),
            "{args:?}: whether any EXIF is left"
        );
    }
}

// --- PRRT_kwDOQNbr5M6mOo0u: the C ABI applies changes in call order --------

struct Handle(*mut oxidex::ffi::ExifToolHandle);

impl Handle {
    fn read(path: &Path) -> Self {
        let handle = Handle(exiftool_create());
        let c = CString::new(s(path)).unwrap();
        assert_eq!(exiftool_read_file(handle.0, c.as_ptr()), EXIFTOOL_OK);
        handle
    }
    fn remove(&self, tag: &str) {
        let tag = CString::new(tag).unwrap();
        assert_eq!(exiftool_remove_tag(self.0, tag.as_ptr()), EXIFTOOL_OK);
    }
    fn set(&self, tag: &str, value: &str) {
        let (tag, value) = (CString::new(tag).unwrap(), CString::new(value).unwrap());
        assert_eq!(
            exiftool_set_tag_string(self.0, tag.as_ptr(), value.as_ptr()),
            EXIFTOOL_OK
        );
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

/// `exiftool_remove_tag(h, "EXIF:All")` then `exiftool_set_tag_string(h,
/// "IFD0:Artist", "x")` is 13.59's `-EXIF:All= -IFD0:Artist=x`: exactly
/// `[IFD0] Artist: x` on synthetic_001.jpg. The recorded group deletion was
/// appended after the map's changes and deleted the new Artist.
#[test]
fn ffi_group_deletion_then_set_keeps_the_set() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, Path::new(JPEG), "delete-then-set.jpg");
    let handle = Handle::read(&file);
    handle.remove("EXIF:All");
    handle.set("IFD0:Artist", "x");
    assert_eq!(handle.write(&file), EXIFTOOL_OK);
    assert_eq!(get(&file, "IFD0:Artist").as_deref(), Some("x"));
    assert_eq!(get(&file, "IFD0:Make"), None, "EXIF:All deleted Make");
    assert_eq!(exif_groups(&file), ["IFD0"]);

    // The other order: the later deletion removes the earlier set.
    let file = copy_into(&dir, Path::new(JPEG), "set-then-delete.jpg");
    let handle = Handle::read(&file);
    handle.set("IFD0:Artist", "x");
    handle.remove("EXIF:All");
    assert_eq!(handle.write(&file), EXIFTOOL_OK);
    assert_eq!(exif_groups(&file), Vec::<String>::new(), "EXIF left");

    // A deletion between two sets splits them.
    let file = copy_into(&dir, Path::new(JPEG), "set-delete-set.jpg");
    let handle = Handle::read(&file);
    handle.set("IFD0:Artist", "before");
    handle.remove("EXIF:All");
    handle.set("IFD0:Model", "after");
    assert_eq!(handle.write(&file), EXIFTOOL_OK);
    assert_eq!(get(&file, "IFD0:Artist"), None);
    assert_eq!(get(&file, "IFD0:Model").as_deref(), Some("after"));
}

#[test]
fn oracle_ffi_call_order_matches_the_command_line_order() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let dir = TempDir::new().unwrap();
    for delete_first in [true, false] {
        let theirs = copy_into(&dir, Path::new(JPEG), "theirs.jpg");
        let args = if delete_first {
            ["-EXIF:All=", "-IFD0:Artist=x"]
        } else {
            ["-IFD0:Artist=x", "-EXIF:All="]
        };
        oracle_write(oracle, &args, &theirs);
        let ours = copy_into(&dir, Path::new(JPEG), "ours.jpg");
        let handle = Handle::read(&ours);
        if delete_first {
            handle.remove("EXIF:All");
            handle.set("IFD0:Artist", "x");
        } else {
            handle.set("IFD0:Artist", "x");
            handle.remove("EXIF:All");
        }
        assert_eq!(handle.write(&ours), EXIFTOOL_OK);
        for key in ["IFD0:Artist", "IFD0:Make", "ExifIFD:ExifVersion"] {
            assert_eq!(
                get(&ours, key).unwrap_or_default(),
                oracle_value(oracle, &theirs, key),
                "{args:?}: {key}"
            );
        }
    }
}

// --- PRRT_kwDOQNbr5M6mOo0v: a normalized read map keeps its source ---------

/// `normalize_metadata_map` of a read keeps the read's provenance, so a row
/// the caller then removes is deleted (13.59 `-IFD0:Artist=` removes it)
/// instead of being silently skipped as a map built from scratch.
#[test]
fn a_normalized_read_map_still_deletes_what_its_caller_removed() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, Path::new(JPEG), "a.jpg");
    let mut map = normalize_metadata_map(&read_metadata(&file).unwrap());
    assert!(map.remove("IFD0:Artist").is_some());
    write_metadata(&file, &map).expect("write the normalized map");
    assert_eq!(get(&file, "IFD0:Artist"), None, "the removal was skipped");
    assert_eq!(
        get(&file, "IFD0:Make").as_deref(),
        Some("Synthetic Camera Co")
    );

    // Unedited, the normalized read requests nothing.
    let file = copy_into(&dir, Path::new(JPEG), "b.jpg");
    let before = sha(&file);
    let map = normalize_metadata_map(&read_metadata(&file).unwrap());
    write_metadata(&file, &map).expect("write the unedited map");
    assert_eq!(sha(&file), before);
}
