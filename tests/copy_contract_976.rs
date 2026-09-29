//! Copy contracts measured against pinned ExifTool 13.59 on Canon.jpg and
//! RIFF.webp. Native argument spellings include one leading dash for a copy
//! selector and two for an exclusion; the library strips one dash.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{copy_metadata, copy_metadata_report};
use oxidex::core::write_transaction::WriteOutcome;
use oxidex::error::ExifToolError;
use std::fs;
#[cfg(unix)]
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

fn unchanged(path: &Path, before: &[u8], metadata: &fs::Metadata) {
    assert_eq!(fs::read(path).unwrap(), before);
    #[cfg(unix)]
    assert_eq!(fs::metadata(path).unwrap().ino(), metadata.ino());
    #[cfg(not(unix))]
    let _ = metadata; // Windows still checks bytes and backup behavior below.
    assert!(!PathBuf::from(format!("{}_original", path.display())).exists());
}

fn physical_canon_refusal(error: &ExifToolError) {
    assert_eq!(
        error.tags_not_written(),
        &[oxidex::error::TagNotWritten::new(
            "ExifIFD:MakerNoteCanon",
            "the selected physical maker note block cannot be copied by oxidex",
        )],
        "{error}"
    );
}

#[test]
fn explicit_empty_filter_copies_nothing_while_none_keeps_copy_all() {
    let dir = TempDir::new().unwrap();
    let source = Path::new(JPEG);
    let destination = duplicate(&dir, source, "empty.jpg");
    let before = fs::read(&destination).unwrap();
    let metadata = fs::metadata(&destination).unwrap();
    let report = copy_metadata_report(source, &destination, Some(&[])).unwrap();
    assert_eq!((report.requested, report.copied), (0, 0));
    assert!(report.uncopied_tags.is_empty());
    assert_eq!(report.outcome, WriteOutcome::Unchanged);
    assert_eq!(
        copy_metadata(source, &destination, Some(&[])).unwrap(),
        WriteOutcome::Unchanged
    );
    unchanged(&destination, &before, &metadata);

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
fn embedded_raf_exif_does_not_count_as_proven_absent() {
    let source = fixtures::required_t_images_fixture_path("FujiFilm.raf");
    let dir = TempDir::new().unwrap();
    let destination = duplicate(&dir, Path::new(JPEG), "raf-copy.jpg");
    let before = fs::read(&destination).unwrap();
    let metadata = fs::metadata(&destination).unwrap();
    let error = copy_metadata_report(
        &source,
        &destination,
        Some(&filters(&["MakerNoteFujiFilm"])),
    )
    .unwrap_err();
    assert_eq!(error.tags_not_written()[0].tag, "MakerNoteFujiFilm");
    assert!(error.to_string().contains("physical maker note block"));
    unchanged(&destination, &before, &metadata);
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
        let before = fs::read(&destination).unwrap();
        let metadata = fs::metadata(&destination).unwrap();
        let result = copy_metadata_report(&canon, &destination, Some(&filters(&selector)));
        if block_selected {
            physical_canon_refusal(&result.unwrap_err());
            unchanged(&destination, &before, &metadata);
        } else {
            let report = result.unwrap_or_else(|error| panic!("{label}: {error}"));
            assert!(
                !report
                    .uncopied_tags
                    .iter()
                    .any(|tag| tag.tag.starts_with("Canon:")),
                "{label}: {report:?}"
            );
        }
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
        let metadata = fs::metadata(&destination).unwrap();
        let error =
            copy_metadata_report(&canon, &destination, Some(&filters(&[selector]))).unwrap_err();
        let refused = error.tags_not_written();
        assert_eq!(refused.len(), 1, "{label}: {error}");
        assert_eq!(refused[0].tag, selector, "{label}: {error}");
        assert!(
            refused[0].reason.contains(
                "ExifTool copies it to ExifIFD:MakerNoteCanon, and Cannot write tag 'ExifIFD:MakerNoteCanon': the source carries a physical maker note block, which oxidex cannot copy"
            ),
            "{label}: {error}"
        );
        unchanged(&destination, &before, &metadata);
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
        let metadata = fs::metadata(&destination).unwrap();
        let error = copy_metadata_report(Path::new(JPEG), &destination, Some(&filters(&selector)))
            .unwrap_err();
        assert!(error.to_string().contains("WebP"), "{label}: {error}");
        unchanged(&destination, &before, &metadata);
    }
    let destination = duplicate(&dir, &webp, "absent.webp");
    let before = fs::read(&destination).unwrap();
    let metadata = fs::metadata(&destination).unwrap();
    let report = copy_metadata_report(
        Path::new(JPEG),
        &destination,
        Some(&filters(&["NoSuchTag"])),
    )
    .unwrap();
    assert_eq!((report.requested, report.copied), (0, 0));
    assert_eq!(report.outcome, WriteOutcome::Unchanged);
    unchanged(&destination, &before, &metadata);
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
        let before = fs::read(&destination).unwrap();
        let metadata = fs::metadata(&destination).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-overwrite_original", "-TagsFromFile"])
            .arg(&canon)
            .args(["-all", later, "-Artist=final"])
            .arg(&destination)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if block_survives {
            assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
            assert!(
                stderr.contains("Cannot write tag 'ExifIFD:MakerNoteCanon': the selected physical maker note block cannot be copied by oxidex"),
                "{label}: {stderr}"
            );
            unchanged(&destination, &before, &metadata);
        } else {
            assert!(output.status.success(), "{label}: {output:?}");
            assert!(!stderr.contains("MakerNoteCanon"), "{label}: {stderr}");
            let readback = Command::new(env!("CARGO_BIN_EXE_oxidex"))
                .args(["-s3", "-Artist"])
                .arg(&destination)
                .output()
                .unwrap();
            assert!(readback.status.success(), "{label}: {readback:?}");
            assert_eq!(
                String::from_utf8_lossy(&readback.stdout).trim(),
                "final",
                "{label}"
            );
        }
    }
}

#[test]
fn canceled_physical_block_copy_uses_its_resolved_destination() {
    let canon = fixtures::required_t_images_fixture_path("Canon.jpg");
    let dir = TempDir::new().unwrap();
    for (label, requests, expected) in [
        (
            "qualified",
            vec!["-MakerNoteCanon>IFD0:Artist", "-IFD0:Artist=final"],
            "final",
        ),
        (
            "bare-copy",
            vec!["-MakerNoteCanon>Artist", "-IFD0:Artist=final"],
            "final",
        ),
        (
            "bare-set",
            vec!["-MakerNoteCanon>IFD0:Artist", "-Artist=final"],
            "final",
        ),
        (
            "later-copy",
            vec!["-MakerNoteCanon>IFD0:Artist", "-Make>IFD0:Artist"],
            "Canon",
        ),
    ] {
        let destination = duplicate(&dir, Path::new(JPEG), &format!("{label}.jpg"));
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-overwrite_original", "-TagsFromFile"])
            .arg(&canon)
            .args(requests)
            .arg(&destination)
            .output()
            .unwrap();
        assert!(output.status.success(), "{label}: {output:?}");
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-j", "-G1", "-Artist"])
            .arg(&destination)
            .output()
            .unwrap();
        assert!(output.status.success(), "{label}: {output:?}");
        let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(rows[0]["IFD0:Artist"], expected, "{label}: {rows}");
    }

    // A block that survives destination reduction still refuses atomically.
    let destination = duplicate(&dir, Path::new(JPEG), "surviving.jpg");
    let before = fs::read(&destination).unwrap();
    let metadata = fs::metadata(&destination).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-overwrite_original", "-TagsFromFile"])
        .arg(&canon)
        .args(["-Make>IFD0:Artist", "-MakerNoteCanon>IFD0:Artist"])
        .arg(&destination)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "Cannot write tag 'MakerNoteCanon>IFD0:Artist': ExifTool copies it to IFD0:Artist, and Cannot write tag 'IFD0:Artist': the source carries a physical maker note block, which oxidex cannot copy"
        ),
        "{output:?}"
    );
    unchanged(&destination, &before, &metadata);
}
