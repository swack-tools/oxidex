//! The sixth round of Codex review threads on the beta.1 roll-up (#957) at
//! 5e31207b. Oracle rows are pinned ExifTool 13.59 (`perl5.38.2
//! -I<pinned>/lib <pinned>/exiftool`; probes `-ver` = 13.59, `OOXML.docx`
//! FileType = DOCX), re-measured through `exiftool_oracle::graded()`. The
//! write-path half of the third thread is graded, case by case, by
//! `tests/request_order_matrix.rs::single_file_list_and_directory_writes_agree`.

use oxidex::core::operations::{copy_metadata_report, read_metadata, write_metadata};
use oxidex::core::{MetadataMap, TagValue, WriteOutcome};
use oxidex::exiftool_oracle;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";

fn copy_into(dir: &TempDir, fixture: &str, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).unwrap()).to_vec()
}

fn text(value: &str) -> TagValue {
    TagValue::new_string(value)
}

// --- PRRT_kwDOQNbr5M6mR8c8: every copy counts resolved destinations -------

/// A PDF read holds `CreateDate`/`CreationDate` and `ModifyDate`/`ModDate`
/// for two Info fields; a copy-all wrote two fields and reported four. The
/// PDF's writable Info fields are eight (Title, Author, Subject, Keywords,
/// Creator, Producer and the two dates); `requested` stays the raw count of
/// source rows the copy considered.
#[test]
fn a_copy_all_counts_each_written_field_once() {
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, PDF, "dst.pdf");
    let report = copy_metadata_report(Path::new(PDF), &dst, None).unwrap();
    assert_eq!(report.copied, 8, "{report:?}");
    assert!(
        report.requested >= report.copied + 2 + report.uncopied_tags.len(),
        "{report:?}"
    );

    // A JPEG has no aliased rows: each copied row is one destination.
    let dst = copy_into(&dir, JPEG, "dst.jpg");
    let report = copy_metadata_report(Path::new(JPEG), &dst, None).unwrap();
    assert_eq!(
        report.copied + report.uncopied_tags.len(),
        report.requested,
        "{report:?}"
    );
}

// --- PRRT_kwDOQNbr5M6mR8c-: names compare as request resolution does -------

/// A file-system fact or descriptive row spelled in another case is the same
/// row: never a request (the canonical spelling is ignored and the write is
/// `unchanged`), where it was a set the writer then refused.
#[test]
fn differently_cased_file_rows_are_never_requests() {
    let dir = TempDir::new().unwrap();
    for key in [
        "File:FileName",
        "file:FileName",
        "File:filename",
        "FILE:FILEMODIFYDATE",
        "system:FileAccessDate",
        "exiftool:ExifToolVersion",
    ] {
        let file = copy_into(&dir, JPEG, "a.jpg");
        let before = sha(&file);
        let mut map = MetadataMap::new();
        map.insert(key, text("x"));
        let outcome = write_metadata(&file, &map).unwrap_or_else(|e| panic!("{key}: {e}"));
        assert_eq!(outcome, WriteOutcome::Unchanged, "{key}");
        assert_eq!(sha(&file), before, "{key}");
    }
}

/// In a read map, a row removed and set again under another case is a set,
/// not a deletion that the later-wins rule then applied: the read's
/// `IFD0:Artist` replaced by `ifd0:artist` = `x` leaves Artist `x`, as
/// 13.59's `-ifd0:artist=x` does.
#[test]
fn a_row_respelled_in_another_case_is_set_not_deleted() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let mut map = read_metadata(&file).unwrap();
    map.remove("IFD0:Artist");
    map.insert("ifd0:artist", text("x"));
    write_metadata(&file, &map).unwrap();
    assert_eq!(
        read_metadata(&file).unwrap().get_string("IFD0:Artist"),
        Some("x")
    );

    // And a PDF date removed under one spelling and set under the other, in
    // another case, is set.
    let pdf = copy_into(&dir, PDF, "a.pdf");
    let mut map = read_metadata(&pdf).unwrap();
    map.remove("PDF:CreationDate");
    map.insert("pdf:createdate", text("2020:01:02 03:04:05"));
    write_metadata(&pdf, &map).unwrap();
    let date = read_metadata(&pdf)
        .unwrap()
        .get("PDF:CreateDate")
        .map(|value| format!("{value:?}"))
        .unwrap_or_default();
    assert!(date.contains("2020"), "{date}");
}

// --- PRRT_kwDOQNbr5M6mR8dA: every write path consumes the same plan --------

/// `-Artist=x -NoSuchTag=y a.jpg b.jpg`: the warning, then Artist written to
/// both, as the single-file path and pinned 13.59 do.
#[test]
fn a_file_list_write_drops_undefined_names_like_one_file() {
    let dir = TempDir::new().unwrap();
    let (a, b) = (
        copy_into(&dir, JPEG, "a.jpg"),
        copy_into(&dir, JPEG, "b.jpg"),
    );
    let o = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-Artist=x", "-NoSuchTag=y"])
        .args([&a, &b])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "Warning: Tag 'NoSuchTag' is not defined\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "    2 image files updated\n"
    );
    for file in [&a, &b] {
        assert_eq!(
            read_metadata(file).unwrap().get_string("IFD0:Artist"),
            Some("x")
        );
    }
    if let Some(oracle) = exiftool_oracle::graded() {
        let (c, d) = (
            copy_into(&dir, JPEG, "c.jpg"),
            copy_into(&dir, JPEG, "d.jpg"),
        );
        let theirs = oracle
            .command()
            .args(["-overwrite_original", "-Artist=x", "-NoSuchTag=y"])
            .args([&c, &d])
            .output()
            .unwrap();
        assert_eq!(theirs.stdout, o.stdout);
        assert_eq!(theirs.stderr, o.stderr);
    }
}
