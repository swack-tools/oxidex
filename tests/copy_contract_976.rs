//! Copy contracts measured against pinned ExifTool 13.59 on Canon.jpg and
//! RIFF.webp. Native argument spellings include one leading dash for a copy
//! selector and two for an exclusion; the library strips one dash.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{copy_metadata, copy_metadata_report};
use oxidex::core::write_transaction::WriteOutcome;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";

fn duplicate(dir: &TempDir, source: &Path, name: &str) -> PathBuf {
    let destination = dir.path().join(name);
    fs::copy(source, &destination).unwrap();
    destination
}

fn filters(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn unchanged(path: &Path, before: &[u8], inode: u64) {
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(fs::metadata(path).unwrap().ino(), inode);
    assert!(!PathBuf::from(format!("{}_original", path.display())).exists());
}

#[test]
fn explicit_empty_filter_copies_nothing_while_none_keeps_copy_all() {
    let dir = TempDir::new().unwrap();
    let source = Path::new(JPEG);
    let destination = duplicate(&dir, source, "empty.jpg");
    let before = fs::read(&destination).unwrap();
    let inode = fs::metadata(&destination).unwrap().ino();
    let report = copy_metadata_report(source, &destination, Some(&[])).unwrap();
    assert_eq!((report.requested, report.copied), (0, 0));
    assert!(report.uncopied_tags.is_empty());
    assert_eq!(report.outcome, WriteOutcome::Unchanged);
    assert_eq!(
        copy_metadata(source, &destination, Some(&[])).unwrap(),
        WriteOutcome::Unchanged
    );
    unchanged(&destination, &before, inode);

    // An empty explicit selection has no need to open or detect a destination.
    let missing = dir.path().join("missing.webp");
    let report = copy_metadata_report(source, &missing, Some(&[])).unwrap();
    assert_eq!(report.outcome, WriteOutcome::Unchanged);
    assert_eq!(report.requested, 0);

    let all = duplicate(&dir, source, "all.jpg");
    let report = copy_metadata_report(source, &all, None).unwrap();
    assert!(report.requested > 0, "{report:?}");
    let no_match = duplicate(&dir, source, "no-match.jpg");
    for selector in ["NoSuchTag", "NoSuch*:all", "NoSuch*:Make"] {
        let report = copy_metadata_report(source, &no_match, Some(&filters(&[selector]))).unwrap();
        assert_eq!(report.requested, 0, "{selector}: {report:?}");
        assert_eq!(report.outcome, WriteOutcome::Unchanged);
    }
}

#[test]
fn physical_canon_block_follows_exif_selection_and_valid_exclusions() {
    let canon = fixtures::required_t_images_fixture_path("Canon.jpg");
    let dir = TempDir::new().unwrap();
    for (label, selector, block_selected) in [
        ("Canon", vec!["Canon:all"], false),
        ("MakerNotes", vec!["MakerNotes:all"], false),
        ("all", vec!["all"], true),
        ("all minus Canon", vec!["all", "-Canon:all"], true),
        ("all minus MakerNotes", vec!["all", "-MakerNotes:all"], true),
        ("all minus block", vec!["all", "-MakerNoteCanon"], false),
        ("EXIF", vec!["EXIF:all"], true),
        (
            "EXIF minus block",
            vec!["EXIF:all", "-MakerNoteCanon"],
            false,
        ),
        ("ExifIFD", vec!["ExifIFD:all"], true),
        ("all minus ExifIFD", vec!["all", "-ExifIFD:all"], false),
    ] {
        let destination = duplicate(&dir, Path::new(JPEG), &format!("{label}.jpg"));
        let set_make = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-overwrite_original", "-Make=Canon"])
            .arg(&destination)
            .output()
            .unwrap();
        assert!(set_make.status.success(), "{label}: {set_make:?}");
        let report = copy_metadata_report(&canon, &destination, Some(&filters(&selector))).unwrap();
        let has_block = report
            .uncopied_tags
            .iter()
            .any(|tag| tag.tag.starts_with("Canon:"));
        assert_eq!(has_block, block_selected, "{label}: {report:?}");
    }

    // This named physical source block exists in Canon.jpg; silently treating
    // it as an absent source tag would hide a native 13.59 write.
    for (label, selector) in [
        ("named", "MakerNoteCanon"),
        ("named-lowercase", "makernotecanon"),
        ("named-grouped", "ExifIFD:MakerNoteCanon"),
    ] {
        let destination = duplicate(&dir, Path::new(JPEG), &format!("{label}.jpg"));
        let before = fs::read(&destination).unwrap();
        let inode = fs::metadata(&destination).unwrap().ino();
        let error =
            copy_metadata_report(&canon, &destination, Some(&filters(&[selector]))).unwrap_err();
        assert!(error.to_string().contains(selector), "{error}");
        unchanged(&destination, &before, inode);
    }
    let no_block = duplicate(&dir, Path::new(JPEG), "no-source-block.jpg");
    let report = copy_metadata_report(
        Path::new(JPEG),
        &no_block,
        Some(&filters(&["MakerNoteCanon"])),
    )
    .unwrap();
    assert_eq!((report.requested, report.copied), (0, 0));
    assert_eq!(report.outcome, WriteOutcome::Unchanged);

    let destination = duplicate(&dir, Path::new(JPEG), "wrong-make.jpg");
    let report =
        copy_metadata_report(&canon, &destination, Some(&filters(&["all", "-Make"]))).unwrap();
    assert!(
        !report
            .uncopied_tags
            .iter()
            .any(|tag| tag.tag.starts_with("Canon:")),
        "{report:?}"
    );
}

#[test]
fn unmodelled_webp_refuses_selected_copy_but_allows_no_source_match() {
    let webp = fixtures::required_t_images_fixture_path("RIFF.webp");
    let dir = TempDir::new().unwrap();
    for (label, selector) in [
        ("all", vec!["all"]),
        ("wildcard", vec!["M*"]),
        ("named", vec!["Make"]),
    ] {
        let destination = duplicate(&dir, &webp, &format!("{label}.webp"));
        let before = fs::read(&destination).unwrap();
        let inode = fs::metadata(&destination).unwrap().ino();
        let error = copy_metadata_report(Path::new(JPEG), &destination, Some(&filters(&selector)))
            .unwrap_err();
        assert!(error.to_string().contains("WebP"), "{label}: {error}");
        unchanged(&destination, &before, inode);
    }
    let destination = duplicate(&dir, &webp, "absent.webp");
    let before = fs::read(&destination).unwrap();
    let inode = fs::metadata(&destination).unwrap().ino();
    let report = copy_metadata_report(
        Path::new(JPEG),
        &destination,
        Some(&filters(&["NoSuchTag"])),
    )
    .unwrap();
    assert_eq!((report.requested, report.copied), (0, 0));
    assert_eq!(report.outcome, WriteOutcome::Unchanged);
    unchanged(&destination, &before, inode);
}

#[test]
fn cli_bare_tags_from_file_still_means_copy_all() {
    let dir = TempDir::new().unwrap();
    let destination = duplicate(&dir, Path::new(JPEG), "cli.jpg");
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-overwrite_original", "-TagsFromFile", JPEG])
        .arg(&destination)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("updated"),
        "{output:?}"
    );
}

#[test]
fn cli_reports_only_a_maker_block_surviving_later_requests() {
    let canon = fixtures::required_t_images_fixture_path("Canon.jpg");
    let dir = TempDir::new().unwrap();
    for (label, later, block_survives) in [
        ("clear", "-all=", false),
        ("different-make", "-Make=Nikon", false),
        ("same-make", "-Make=Canon", true),
    ] {
        let destination = duplicate(&dir, Path::new(JPEG), &format!("{label}.jpg"));
        let setup = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-overwrite_original", "-Make=Canon"])
            .arg(&destination)
            .output()
            .unwrap();
        assert!(setup.status.success(), "{label}: {setup:?}");
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-overwrite_original", "-TagsFromFile"])
            .arg(&canon)
            .args(["-all", later])
            .arg(&destination)
            .output()
            .unwrap();
        assert!(output.status.success(), "{label}: {output:?}");
        let warning = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            warning.contains("cannot write the Canon"),
            block_survives,
            "{label}: {warning}"
        );
    }
}
