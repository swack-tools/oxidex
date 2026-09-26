//! Codex thread PRRT_kwDOQNbr5M6mQbb- on the beta.1 roll-up (#957) at
//! 2738be41: a `<group>:All` deletion the file makes a no-op (it holds nothing
//! in the group) was dropped without cancelling the earlier requests in that
//! group. ExifTool's group deletion removes every value set in the group
//! before it (Writer.pl `RemoveNewValuesForGroup`), so pinned 13.59 answers
//! `-XMP:Title=x -XMP:All=` on a file without XMP with `0 image files
//! updated` / `1 image files unchanged`. oxidex planned the earlier set on
//! its own and refused it (its writers do not write XMP or IPTC), or would
//! have written it where a writer could. Oracle rows: pinned ExifTool 13.59
//! (`perl5.38.2 -I<pinned>/lib <pinned>/exiftool`; probes `-ver` = 13.59,
//! `OOXML.docx` FileType = DOCX), re-measured through
//! `exiftool_oracle::graded()`.

use oxidex::core::operations::read_metadata;
use oxidex::core::{TagChange, TagValue, WriteOutcome, apply_tag_changes};
use oxidex::exiftool_oracle;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const PNG: &str = "tests/fixtures/png/sample.png";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";
const TIFF: &str = "tests/fixtures/tiff/sample.tif";

const UNCHANGED: &str = "    0 image files updated\n    1 image files unchanged\n";

/// Each file holds nothing in the deleted group (13.59 and oxidex both answer
/// the deletion alone with `unchanged`); the earlier set is in the group.
const CANCELLED: &[(&str, &[&str])] = &[
    (JPEG, &["-XMP:Title=x", "-XMP:All="]),
    (JPEG, &["-XMP-dc:Title=x", "-XMP-dc:All="]),
    (JPEG, &["-XMP:Title=x", "-XMP-dc:All="]),
    (JPEG, &["-XMP-dc:Title=x", "-XMP:All="]),
    (JPEG, &["-IPTC:Keywords=x", "-IPTC:All="]),
    (PNG, &["-XMP:Title=x", "-XMP:All="]),
    (PDF, &["-XMP:Title=x", "-XMP:All="]),
    (TIFF, &["-XMP:Title=x", "-XMP:All="]),
    (TIFF, &["-IPTC:Keywords=x", "-IPTC:All="]),
];

fn copy_into(dir: &TempDir, fixture: &str, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).expect("read file")).to_vec()
}

fn oxidex(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .expect("run oxidex")
}

fn name_of(fixture: &str) -> String {
    let ext = Path::new(fixture).extension().unwrap().to_str().unwrap();
    format!("a.{ext}")
}

/// A group deletion cancels the earlier set in its group, even though the
/// file starts with nothing in the group: unchanged, bytes identical.
#[test]
fn a_group_deletion_cancels_earlier_sets_in_an_empty_group() {
    for (fixture, args) in CANCELLED {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture, &name_of(fixture));
        let before = sha(&file);
        let o = oxidex(args, &file);
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(0), "{fixture} {args:?}: {err}");
        assert_eq!(
            String::from_utf8_lossy(&o.stdout),
            UNCHANGED,
            "{fixture} {args:?}: {err}"
        );
        assert_eq!(sha(&file), before, "{fixture} {args:?}: file changed");
    }
}

/// The library transaction (the path `write_metadata`, the C ABI and the
/// CLI share) cancels it too.
#[test]
fn the_library_transaction_cancels_the_earlier_set() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let before = sha(&file);
    let outcome = apply_tag_changes(
        &file,
        &[
            TagChange::set("XMP:Title", TagValue::new_string("x")),
            TagChange::delete("XMP:All"),
        ],
    )
    .expect("the set is cancelled, not refused");
    assert_eq!(outcome, WriteOutcome::Unchanged);
    assert_eq!(sha(&file), before);
    assert!(read_metadata(&file).unwrap().get("XMP:Title").is_none());
}

/// What the deletion does not cover is still requested, and refused by name
/// where oxidex cannot write it (13.59 writes both): a set after the
/// deletion, and a set in another XMP namespace than the deleted one. The
/// thread's own PNG example (`-PNG:Title=x -PNG:All=`) is refused too:
/// every PNG holds PNG rows (IHDR), so oxidex cannot prove the group empty
/// and does not delete it -- an explicit refusal, never a Title left behind.
#[test]
fn what_the_deletion_does_not_cover_is_still_refused_by_name() {
    for (fixture, args) in [
        (JPEG, &["-XMP:All=", "-XMP:Title=x"][..]),
        (JPEG, &["-XMP-dc:Title=x", "-XMP-xmp:All="][..]),
        (PNG, &["-PNG:Title=x", "-PNG:All="][..]),
    ] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture, &name_of(fixture));
        let before = sha(&file);
        let o = oxidex(args, &file);
        assert_eq!(o.status.code(), Some(1), "{fixture} {args:?}");
        assert!(
            !String::from_utf8_lossy(&o.stdout).contains("image files"),
            "{fixture} {args:?}"
        );
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("Error"),
            "{fixture} {args:?}"
        );
        assert_eq!(sha(&file), before, "{fixture} {args:?}");
    }
}

#[test]
fn oracle_cancels_the_same_earlier_sets() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    for (fixture, args) in CANCELLED {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture, &name_of(fixture));
        let before = sha(&file);
        let o = oracle.command().args(*args).arg(&file).output().unwrap();
        assert!(o.status.success(), "{} {args:?}", oracle.display());
        assert_eq!(
            String::from_utf8_lossy(&o.stdout),
            UNCHANGED,
            "{fixture} {args:?}"
        );
        assert_eq!(sha(&file), before, "{fixture} {args:?}");
    }
}
