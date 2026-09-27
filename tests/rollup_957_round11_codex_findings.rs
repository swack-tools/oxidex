//! Findings of three local Codex reviews (`codex review -c
//! model_reasoning_effort="xhigh" --base origin/refactor/tag-machinery`) of
//! the beta.1 roll-up (#957) at 5bdf560e. Oracle rows are pinned ExifTool
//! 13.59 (`perl5.38.2 -I<pinned>/lib <pinned>/exiftool`; probes `-ver` =
//! 13.59, `OOXML.docx` FileType = DOCX), through `exiftool_oracle::graded()`.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::modify_tag;
use oxidex::core::tag_value::TagValue;
use oxidex::error::ExifToolError;
use oxidex::exiftool_oracle::{self, Oracle};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";

fn oxidex(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .expect("run oxidex")
}

fn oracle_run(oracle: &Oracle, args: &[&str], path: &Path) -> Output {
    oracle
        .command()
        .args(["-overwrite_original"])
        .args(args)
        .arg(path)
        .output()
        .expect("run oracle")
}

/// The oracle's `-a -G1 -s` rows of `tags` in `path` (`-n` when `numeric`).
fn rows(oracle: &Oracle, path: &Path, tags: &[&str], numeric: bool) -> Vec<String> {
    let mut command = oracle.command();
    command.args(["-a", "-G1", "-s"]);
    if numeric {
        command.arg("-n");
    }
    let o = command
        .args(tags.iter().map(|tag| format!("-{tag}")))
        .arg(path)
        .output()
        .expect("run oracle read");
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

fn copy_into(dir: &TempDir, source: &Path, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(source, &path).expect("copy fixture");
    path
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A copy into a Panasonic RAW kept only `PanasonicRaw::Main` candidates, so
/// a standard ExifIFD tag found no destination at all: the copy counted it
/// and neither wrote nor named it. 13.59 writes it to the RW2's ExifIFD.
#[test]
fn a_copy_into_a_panasonic_raw_keeps_its_standard_exif_destinations() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let (Some(rw2), Some(canon)) = (
        fixtures::pinned_t_images_fixture_path("Panasonic.rw2"),
        fixtures::pinned_t_images_fixture_path("Canon.jpg"),
    ) else {
        eprintln!("skipping: pinned t/images fixtures not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    let source = canon.to_str().unwrap();
    let theirs = copy_into(&dir, &rw2, "theirs.rw2");
    let ours = copy_into(&dir, &rw2, "ours.rw2");
    let args = ["-TagsFromFile", source, "-DateTimeOriginal"];
    assert!(oracle_run(oracle, &args, &theirs).status.success());
    let before = fs::read(&ours).unwrap();
    let o = oxidex(&[&["-overwrite_original"][..], &args[..]].concat(), &ours);
    let tags = ["ExifIFD:DateTimeOriginal"];
    let expected = rows(oracle, &theirs, &tags, false);
    assert_eq!(
        expected,
        [
            "[ExifIFD] DateTimeOriginal : 2003:12:04 06:46:52",
            "[ExifIFD] DateTimeOriginal : 2003:12:04 06:46:52",
        ]
    );
    if o.status.success() {
        assert_eq!(
            rows(oracle, &ours, &tags, false),
            expected,
            "{}",
            stderr(&o)
        );
    } else {
        // Refused by name, never counted and dropped.
        assert!(stderr(&o).contains("DateTimeOriginal"), "{}", stderr(&o));
        assert_eq!(fs::read(&ours).unwrap(), before, "file untouched");
    }
}

/// `EXIF:All=` (and `IFD0:All=`) cancels an earlier set in a numbered
/// SubIFD (`SubIFD1`) as it cancels one in `SubIFD`: 13.59 deletes the EXIF
/// and never asks for the set, so the command succeeds.
#[test]
fn an_exif_group_deletion_cancels_an_earlier_numbered_subifd_set() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let Some(canon) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        eprintln!("skipping: pinned t/images/Canon.jpg not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    let args = ["-SubIFD1:Artist=x", "-EXIF:All="];
    let theirs = copy_into(&dir, &canon, "theirs.jpg");
    let ours = copy_into(&dir, &canon, "ours.jpg");
    assert!(oracle_run(oracle, &args, &theirs).status.success());
    let o = oxidex(&args, &ours);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let tags = ["EXIF:All"];
    let expected = rows(oracle, &theirs, &tags, false);
    assert!(expected.is_empty(), "13.59 removed the EXIF: {expected:?}");
    assert_eq!(rows(oracle, &ours, &tags, false), expected);
}

/// A signed rational64s is rationalized within 0x7fffffff by 13.59 too
/// (`SetRational64s`), so a value whose unsigned rationalization differs is
/// still written as 13.59 writes it: `-BrightnessValue=3.614421976e-10`
/// stores 0/1. The unsigned guard stays for rational64u tags.
#[test]
fn a_tiny_signed_rational_is_written_as_exiftool_writes_it() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = TempDir::new().unwrap();
    let arg = "-BrightnessValue=3.614421976e-10";
    let theirs = copy_into(&dir, Path::new(JPEG), "theirs.jpg");
    let ours = copy_into(&dir, Path::new(JPEG), "ours.jpg");
    assert!(oracle_run(oracle, &[arg], &theirs).status.success());
    let o = oxidex(&[arg], &ours);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let tags = ["ExifIFD:BrightnessValue"];
    let expected = rows(oracle, &theirs, &tags, true);
    assert_eq!(expected, ["[ExifIFD] BrightnessValue : 0"]);
    assert_eq!(rows(oracle, &ours, &tags, true), expected);
    // rational64u: still refused rather than stored as 0/1.
    let o = oxidex(&["-ExposureIndex=3.614421976e-10"], &ours);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
}

/// A library set of a PDF Info date bypasses the CLI's range checks; month
/// 13 was written as the invalid `D:20201302030405`. It is refused, file
/// untouched, as 13.59 refuses it.
#[test]
fn an_out_of_range_pdf_date_is_refused_through_the_library() {
    let dir = TempDir::new().unwrap();
    let ours = copy_into(&dir, Path::new(PDF), "ours.pdf");
    let before = fs::read(&ours).unwrap();
    let result = modify_tag(
        &ours,
        "PDF:CreateDate",
        TagValue::String("2020:13:02 03:04:05".into()),
    );
    assert!(
        matches!(result, Err(ExifToolError::TagsNotWritten { .. })),
        "{result:?}"
    );
    assert_eq!(fs::read(&ours).unwrap(), before, "file untouched");
    let ok = modify_tag(
        &ours,
        "PDF:CreateDate",
        TagValue::String("2020:12:02 03:04:05".into()),
    );
    assert!(ok.is_ok(), "{ok:?}");
    if let Some(oracle) = exiftool_oracle::graded() {
        let theirs = copy_into(&dir, Path::new(PDF), "theirs.pdf");
        let before = fs::read(&theirs).unwrap();
        let o = oracle_run(oracle, &["-PDF:CreateDate=2020:13:02 03:04:05"], &theirs);
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("out of range"),
            "{o:?}"
        );
        assert_eq!(fs::read(&theirs).unwrap(), before, "13.59 refuses it too");
    }
}

/// An exclusion (`--Make`) before one of oxidex's long flags ended option
/// parsing, so `--readonly` was taken as one more exclusion and the copy was
/// written. The flag is honoured wherever it stands, and the exclusion still
/// excludes (13.59 copies no Make with `--Make`).
#[test]
fn a_long_flag_after_a_copy_exclusion_is_still_a_flag() {
    let Some(canon) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        eprintln!("skipping: pinned t/images/Canon.jpg not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    let source = canon.to_str().unwrap();
    let ours = copy_into(&dir, Path::new(JPEG), "ours.jpg");
    let before = fs::read(&ours).unwrap();
    let o = oxidex(
        &["-TagsFromFile", source, "-all", "--Make", "--readonly"],
        &ours,
    );
    assert_eq!(o.status.code(), Some(1), "--readonly refuses the copy");
    assert_eq!(fs::read(&ours).unwrap(), before, "file untouched");

    let o = oxidex(
        &[
            "-overwrite_original",
            "-TagsFromFile",
            source,
            "-all",
            "--Make",
        ],
        &ours,
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    if let Some(oracle) = exiftool_oracle::graded() {
        let theirs = copy_into(&dir, Path::new(JPEG), "theirs.jpg");
        assert!(
            oracle_run(
                oracle,
                &["-TagsFromFile", source, "-all", "--Make"],
                &theirs
            )
            .status
            .success()
        );
        let tags = ["Make", "Model"];
        assert_eq!(
            rows(oracle, &ours, &tags, false),
            rows(oracle, &theirs, &tags, false)
        );
    }
}
