//! A `-TAG=VALUE` write either changes the file or says it did not.
//!
//! The defect: `oxidex -XPTitle=v photo.jpg` (no group) printed
//! `1 image files updated`, exited 0, and wrote nothing, while
//! `-IFD0:XPTitle=v` and `-EXIF:XPTitle=v` worked. The ungrouped name reached
//! the EXIF writers unresolved; they only visit `IFD0:`/`ExifIFD:`/`GPS:`/
//! `EXIF:` keys, skipped it, and `main` printed success unconditionally. The
//! same silence covered undefined names (`-NoSuchTag=v`), grouped keys the
//! format's writer does not address (`-XMP:Title=v` in a JPEG), and
//! no-op deletions (`-XPTitle=` where there is none), which rewrote the file
//! and claimed an update.
//!
//! Every expectation below was taken from the pinned oracle (ExifTool 13.59,
//! `perl5.38.2 -I<pinned>/lib <pinned>/exiftool -config ""`, probes `-ver` =
//! 13.59 and `OOXML.docx` FileType = DOCX) on the same fixtures:
//!
//! | command (fixture)                          | ExifTool 13.59                                      |
//! |--------------------------------------------|-----------------------------------------------------|
//! | `-XPTitle=v` (synthetic_001.jpg)           | `[IFD0] XPTitle: v`; `1 image files updated`; rc 0  |
//! | `-Make=v` (synthetic_001.jpg)              | `[IFD0] Make: v`; updated; rc 0                     |
//! | `-XPTitle=` (synthetic_001.jpg, none)      | bytes same; `0 image files updated` / `1 image files unchanged`; rc 0 |
//! | `-NoSuchTag=v`                             | `Warning: Tag 'NoSuchTag' is not defined` / `Nothing to do.`; rc 1 |
//! | `-NoSuchTag=v -XPTitle=v`                  | the warning, then `1 image files updated`; rc 0     |
//! | `-XPTitle=v` (sample.tif)                  | `[IFD0] XPTitle: v`; updated; rc 0                  |
//! | `-Title=v` (sample_with_exif_xmp.jpg)      | writes `[XMP-dc] Title` (oxidex cannot: refused)    |
//! | `-XResolution=300` (tag_matrix_base.jpg)   | writes `[IFD0]` *and* `[JFIF] XResolution` (refused)|
//!
//! Where ExifTool writes a group oxidex cannot, oxidex must refuse with an
//! error and leave the file byte-identical -- never report an update.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";
const JPEG_JFIF: &str = "tests/fixtures/jpeg/tag_matrix_base.jpg";
const TIFF: &str = "tests/fixtures/tiff/sample.tif";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";
const PNG: &str = "tests/fixtures/png/sample.png";

fn oxidex(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

fn copy_into(dir: &TempDir, fixture: &str, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).expect("read file")).to_vec()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The value oxidex reads back for `key` (`-s3` prints the bare value).
fn read_back(path: &Path, key: &str) -> String {
    let out = oxidex(&["-s3", &format!("-{key}"), path.to_str().unwrap()]);
    stdout(&out).lines().next().unwrap_or_default().to_string()
}

fn write(path: &Path, args: &[&str]) -> Output {
    let mut all: Vec<&str> = args.to_vec();
    all.push(path.to_str().unwrap());
    oxidex(&all)
}

#[test]
fn ungrouped_xptitle_writes_ifd0_like_exiftool() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let out = write(&file, &["-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "v");
}

#[test]
fn ungrouped_existing_ifd0_tag_is_rewritten() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let out = write(&file, &["-Make=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:Make"), "v");
}

#[test]
fn ungrouped_xptitle_writes_ifd0_in_a_tiff() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, TIFF, "a.tif");
    let out = write(&file, &["-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "v");
}

#[test]
fn undefined_tag_is_exiftools_warning_and_nothing_to_do() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let before = sha(&file);
    let out = write(&file, &["-NoSuchTag=v"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "Warning: Tag 'NoSuchTag' is not defined\nNothing to do.\n"
    );
    assert_eq!(sha(&file), before, "file must be untouched");
}

#[test]
fn undefined_tag_beside_a_real_one_warns_and_writes_the_rest() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let out = write(&file, &["-NoSuchTag=v", "-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stderr(&out), "Warning: Tag 'NoSuchTag' is not defined\n");
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "v");
}

#[test]
fn deleting_an_absent_tag_reports_unchanged_and_keeps_bytes() {
    for tag in ["-XPTitle=", "-IFD0:XPTitle=", "-EXIF:XPTitle="] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, JPEG, "a.jpg");
        let before = sha(&file);
        let out = write(&file, &[tag]);
        assert_eq!(out.status.code(), Some(0), "{tag}: {}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "    0 image files updated\n    1 image files unchanged\n",
            "{tag}"
        );
        assert_eq!(sha(&file), before, "{tag}: bytes must be untouched");
    }
}

/// ExifTool writes these where oxidex cannot; refusing is the only outcome
/// that is not a lie. The file must stay byte-identical.
#[test]
fn writes_oxidex_cannot_perform_are_refused_not_reported() {
    let cases: &[(&str, &str, &[&str])] = &[
        (JPEG_XMP, "a.jpg", &["-Title=v"]),
        (JPEG_XMP, "a.jpg", &["-XMP:Title=v"]),
        (JPEG, "a.jpg", &["-IPTC:Keywords=k"]),
        (JPEG, "a.jpg", &["-File:Comment=c"]),
        (JPEG_JFIF, "a.jpg", &["-XResolution=300"]),
        (PDF, "a.pdf", &["-Title=v"]),
        (PDF, "a.pdf", &["-XMP:Title=v"]),
        (PNG, "a.png", &["-XMP:Title=v"]),
        // A refused request aborts the whole file: the XPTitle beside it is
        // not half-applied (ExifTool writes all of a file's tags or none).
        (JPEG, "a.jpg", &["-XPTitle=v", "-XMP:Title=v"]),
    ];
    for (fixture, name, args) in cases {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture, name);
        let before = sha(&file);
        let out = write(&file, args);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{fixture} {args:?}: {}",
            stdout(&out)
        );
        assert!(
            !stdout(&out).contains("image files updated")
                || stdout(&out).contains("0 image files updated"),
            "{fixture} {args:?} claimed an update: {}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("Error"),
            "{fixture} {args:?}: {}",
            stderr(&out)
        );
        assert_eq!(sha(&file), before, "{fixture} {args:?}: file changed");
    }
}

/// The grouped spellings that already worked keep working.
#[test]
fn grouped_spellings_still_write() {
    for tag in ["-IFD0:XPTitle=v", "-EXIF:XPTitle=v"] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, JPEG, "a.jpg");
        let out = write(&file, &[tag]);
        assert_eq!(out.status.code(), Some(0), "{tag}: {}", stderr(&out));
        assert_eq!(stdout(&out), "    1 image files updated\n", "{tag}");
        assert_eq!(read_back(&file, "IFD0:XPTitle"), "v", "{tag}");
    }
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, PDF, "a.pdf");
    let out = write(&file, &["-PDF:Title=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(read_back(&file, "PDF:Title"), "v");
}

/// Multi-file writes summarize like ExifTool 13.59 (`exiftool -XPTitle= a b`):
/// `    0 image files updated` / `    2 image files unchanged`, no read count.
#[test]
fn multi_file_no_op_deletion_reports_unchanged() {
    let dir = TempDir::new().unwrap();
    let a = copy_into(&dir, JPEG, "a.jpg");
    let b = copy_into(&dir, JPEG, "b.jpg");
    let (ha, hb) = (sha(&a), sha(&b));
    let out = oxidex(&["-XPTitle=", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "    0 image files updated\n    2 image files unchanged\n"
    );
    assert_eq!((sha(&a), sha(&b)), (ha, hb));

    let out = oxidex(&["-XPTitle=v", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    2 image files updated\n");
    assert_eq!(read_back(&a, "IFD0:XPTitle"), "v");
    assert_eq!(read_back(&b, "IFD0:XPTitle"), "v");

    let out = oxidex(&["-NoSuchTag=v", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "Warning: Tag 'NoSuchTag' is not defined\nNothing to do.\n"
    );
}

/// The generic guard: across every spelling and fixture here, a run that
/// prints `1 image files updated` must have changed the file's bytes, and a
/// run whose bytes did not change must not claim an update.
#[test]
fn updated_count_never_increments_without_a_byte_change() {
    let fixtures = [
        (JPEG, "a.jpg"),
        (JPEG_XMP, "a.jpg"),
        (JPEG_JFIF, "a.jpg"),
        (TIFF, "a.tif"),
        (PDF, "a.pdf"),
        (PNG, "a.png"),
    ];
    let args = [
        "-XPTitle=v",
        "-XPTitle=",
        "-Make=v",
        "-Make=",
        "-Title=v",
        "-Artist=v",
        "-Software=v",
        "-NoSuchTag=v",
        "-IFD0:XPTitle=v",
        "-EXIF:XPTitle=",
        "-XMP:Title=v",
        "-XMP:Title=",
        "-IPTC:Keywords=k",
        "-IFD1:XResolution=300",
        "-InteropIFD:InteropIndex=R03",
        "-JFIF:XResolution=300",
        "-File:Comment=c",
        "-PDF:Title=v",
        "-PDF:Trapped=True",
        "-PNG:Title=v",
    ];
    for (fixture, name) in fixtures {
        for arg in args {
            let dir = TempDir::new().unwrap();
            let file = copy_into(&dir, fixture, name);
            let before = sha(&file);
            let out = write(&file, &[arg]);
            let changed = sha(&file) != before;
            let text = stdout(&out);
            if text.contains("    1 image files updated") {
                assert!(
                    changed,
                    "{fixture} {arg}: claimed an update, bytes identical"
                );
                assert_eq!(out.status.code(), Some(0), "{fixture} {arg}");
            }
            if !changed {
                assert!(
                    !text.contains("    1 image files updated"),
                    "{fixture} {arg}: {text}"
                );
            }
            if out.status.code() != Some(0) {
                assert!(!changed, "{fixture} {arg}: failed but modified the file");
            }
        }
    }
}

/// The same guard covers the other CLI writers. Pinned ExifTool 13.59:
/// `-all=` on a file with nothing left to strip, and `-TagsFromFile SRC
/// -XPTitle` from a source without one, both print `0 image files updated` /
/// `1 image files unchanged` and leave the bytes alone. oxidex rewrote the
/// file and printed `1 image files updated` (`(1 tags copied)`) for both.
#[test]
fn clear_all_and_tags_from_file_report_unchanged_when_nothing_changes() {
    let dir = TempDir::new().unwrap();
    let stripped = copy_into(&dir, JPEG, "stripped.jpg");
    let out = write(&stripped, &["-all="]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");

    let before = sha(&stripped);
    let out = write(&stripped, &["-all="]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "    0 image files updated\n    1 image files unchanged\n"
    );
    assert_eq!(sha(&stripped), before);

    let dest = copy_into(&dir, JPEG, "dest.jpg");
    let before = sha(&dest);
    let out = write(
        &dest,
        &["-TagsFromFile", stripped.to_str().unwrap(), "-XPTitle"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "    0 image files updated\n    1 image files unchanged\n"
    );
    assert_eq!(sha(&dest), before);
}

/// `--backup` copies the original only when the write changes it (ExifTool
/// makes no `_original` for an unchanged file).
#[test]
fn backup_is_made_only_for_an_update() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let backup = dir.path().join("a.jpg.bak");
    let out = write(&file, &["--backup", "-XPTitle="]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!backup.exists(), "no backup for an unchanged file");
    let original = sha(&file);
    let out = write(&file, &["--backup", "-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(sha(&backup), original, "backup holds the pre-write bytes");
}
