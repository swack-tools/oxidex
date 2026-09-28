//! The seventh round of Codex review threads on the beta.1 roll-up (#957)
//! at 74ed8eae: date operations and `-TagsFromFile` selectors. Each is also
//! a class of `tests/request_order_matrix.rs::dates_and_copies_match_pinned_exiftool`;
//! these pin the threads' own examples. Oracle rows are pinned ExifTool
//! 13.59 (`perl5.38.2 -I<pinned>/lib <pinned>/exiftool`; probes `-ver` =
//! 13.59, `OOXML.docx` FileType = DOCX), re-measured through
//! `exiftool_oracle::graded()`.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{copy_metadata_report, read_metadata};
use oxidex::error::ExifToolError;
use oxidex::exiftool_oracle;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";

fn copy_into(dir: &TempDir, fixture: &Path, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn oxidex(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .expect("run oxidex")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn get(path: &Path, key: &str) -> Option<String> {
    let o = oxidex(&["-s3", &format!("-{key}")], path);
    let value = out(&o).lines().next().unwrap_or_default().to_string();
    (!value.is_empty()).then_some(value)
}

fn canon() -> Option<PathBuf> {
    fixtures::pinned_t_images_fixture_path("Canon.jpg")
}

// --- PRRT_kwDOQNbr5M6mSLMA: date sets keep their place among deletions -----

/// 13.59 on t/images/Canon.jpg: `-EXIF:All= -ModifyDate=<d>` keeps the new
/// ModifyDate, `-ModifyDate=<d> -EXIF:All=` does not. oxidex ran the date
/// write before every set and deletion, and deleted it in both orders.
#[test]
fn a_date_set_keeps_its_place_around_a_group_deletion() {
    let Some(canon) = canon() else {
        eprintln!("skipping: pinned t/images/Canon.jpg not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, &canon, "after.jpg");
    let o = oxidex(&["-EXIF:All=", "-ModifyDate=2025:01:02 03:04:05"], &file);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(
        get(&file, "IFD0:ModifyDate").as_deref(),
        Some("2025:01:02 03:04:05")
    );
    assert_eq!(get(&file, "IFD0:Make"), None, "EXIF:All deleted Make");

    let file = copy_into(&dir, &canon, "before.jpg");
    let o = oxidex(&["-ModifyDate=2025:01:02 03:04:05", "-EXIF:All="], &file);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(get(&file, "IFD0:ModifyDate"), None);

    // `AllDates` is ExifTool's shortcut for the three EXIF dates.
    let file = copy_into(&dir, &canon, "alldates.jpg");
    let o = oxidex(&["-EXIF:All=", "-AllDates=2025:01:02 03:04:05"], &file);
    assert_eq!(o.status.code(), Some(0));
    for key in [
        "IFD0:ModifyDate",
        "ExifIFD:DateTimeOriginal",
        "ExifIFD:CreateDate",
    ] {
        assert_eq!(
            get(&file, key).as_deref(),
            Some("2025:01:02 03:04:05"),
            "{key}"
        );
    }
}

// --- PRRT_kwDOQNbr5M6mSLMB: a same-value date set is an update -------------

/// 13.59: `-ModifyDate=<its value>` and `-AllDates=<their value>` are
/// `1 image files updated`, as every same-value set is; oxidex said
/// `unchanged`.
#[test]
fn a_same_value_date_set_is_an_update() {
    let Some(canon) = canon() else {
        eprintln!("skipping: pinned t/images/Canon.jpg not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    for arg in [
        "-ModifyDate=2003:12:04 06:46:52",
        "-AllDates=2003:12:04 06:46:52",
    ] {
        let file = copy_into(&dir, &canon, "a.jpg");
        let o = oxidex(&[arg], &file);
        assert_eq!(out(&o), "    1 image files updated\n", "{arg}");
    }
    if let Some(oracle) = exiftool_oracle::graded() {
        let file = copy_into(&dir, &canon, "theirs.jpg");
        let o = oracle
            .command()
            .args(["-overwrite_original", "-ModifyDate=2003:12:04 06:46:52"])
            .arg(&file)
            .output()
            .unwrap();
        assert_eq!(out(&o), "    1 image files updated\n");
    }
}

// --- PRRT_kwDOQNbr5M6mSLL9: `all` beside other selectors -------------------

/// 13.59: `-TagsFromFile SRC -all -Make` is a copy-all (the named Make is
/// redundant). oxidex refused the bare `all` once another selector was
/// given, through the CLI and `copy_metadata_report` alike.
#[test]
fn all_beside_another_selector_is_a_copy_all() {
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, Path::new(JPEG), "lib.jpg");
    let filters = ["all".to_string(), "Make".to_string()];
    let report = copy_metadata_report(Path::new(JPEG_XMP), &dst, Some(&filters)).unwrap();
    assert!(report.copied >= 2, "{report:?}");
    assert_eq!(get(&dst, "IFD0:Make").as_deref(), Some("TestCamera"));
    assert_eq!(get(&dst, "IFD0:Model").as_deref(), Some("TM"));

    let dst = copy_into(&dir, Path::new(JPEG), "cli.jpg");
    let o = oxidex(&["-TagsFromFile", JPEG_XMP, "-all", "-Make"], &dst);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(get(&dst, "IFD0:Model").as_deref(), Some("TM"));
}

// --- PRRT_kwDOQNbr5M6mSLL_: every selector form 13.59 accepts --------------

/// A hyphenated source group (`XMP-dc:Title`), `GROUP:all`, `--TAG`
/// exclusions, wildcards and redirection are copied as 13.59 copies them;
/// a redirected selection, which oxidex does not model, is refused by name.
#[test]
fn every_selector_form_is_copied_or_refused_by_name() {
    let dir = TempDir::new().unwrap();
    let copy = |args: &[&str], name: &str| {
        let dst = copy_into(&dir, Path::new(JPEG), name);
        let mut argv = vec!["-TagsFromFile", JPEG_XMP];
        argv.extend(args);
        let o = oxidex(&argv, &dst);
        (dst, o)
    };
    let (dst, o) = copy(&["-XMP-dc:Title>IFD0:Artist"], "hyphen.jpg");
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(get(&dst, "IFD0:Artist").as_deref(), Some("Sample Photo"));

    let (dst, o) = copy(&["-IFD0:all"], "group-all.jpg");
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(get(&dst, "IFD0:Make").as_deref(), Some("TestCamera"));

    let (dst, o) = copy(&["-all", "--Make"], "exclusion.jpg");
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        get(&dst, "IFD0:Make").as_deref(),
        Some("Synthetic Camera Co")
    );
    assert_eq!(get(&dst, "IFD0:Model").as_deref(), Some("TM"));

    let (dst, o) = copy(&["-*Model"], "wildcard.jpg");
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(get(&dst, "IFD0:Model").as_deref(), Some("TM"));
    assert_eq!(
        get(&dst, "IFD0:Make").as_deref(),
        Some("Synthetic Camera Co")
    );

    let dst = copy_into(&dir, Path::new(JPEG), "refused.jpg");
    let before = fs::read(&dst).unwrap();
    let filters = ["all>XMP:all".to_string()];
    match copy_metadata_report(Path::new(JPEG_XMP), &dst, Some(&filters)) {
        Err(ExifToolError::UnsupportedFormat { message }) => {
            assert!(message.contains("'all>XMP:all'"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fs::read(&dst).unwrap(), before);
}

/// A copy with a set or deletion before or after it applies them in
/// argument order (13.59: `-TagsFromFile SRC -Make -IFD0:Make=` deletes the
/// copied Make); oxidex refused the combination.
#[test]
fn a_copy_and_sets_apply_in_argument_order() {
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, Path::new(JPEG), "after.jpg");
    let o = oxidex(&["-TagsFromFile", JPEG_XMP, "-Make", "-IFD0:Make="], &dst);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(get(&dst, "IFD0:Make"), None);

    let dst = copy_into(&dir, Path::new(JPEG), "before.jpg");
    let o = oxidex(&["-IFD0:Make=", "-TagsFromFile", JPEG_XMP, "-Make"], &dst);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(get(&dst, "IFD0:Make").as_deref(), Some("TestCamera"));
    assert!(read_metadata(&dst).unwrap().get("IFD0:Model").is_some());
}
