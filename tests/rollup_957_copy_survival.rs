//! Copy requests must remain accounted for after later ordered requests.
use oxidex::core::operations::copy_metadata_report;
use oxidex::error::ExifToolError;
use oxidex::exiftool_oracle;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/sample_with_exif.jpg";
const PNG: &str = "tests/fixtures/png/sample.png";

#[test]
fn a_named_copy_without_a_destination_is_refused_atomically() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("dest.jpg");
    fs::copy(JPEG, &dest).unwrap();
    let before = fs::read(&dest).unwrap();
    let filters = ["Make>Foo:Artist".to_string()];
    let result = copy_metadata_report(Path::new(JPEG), &dest, Some(&filters));
    assert!(
        matches!(result, Err(ExifToolError::TagsNotWritten { .. })),
        "{result:?}"
    );
    assert_eq!(fs::read(&dest).unwrap(), before);
}

#[test]
fn a_copied_set_cancelled_by_a_later_delete_does_not_trigger_a_backup() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.png");
    let dest = dir.path().join("dest.png");
    let native = dir.path().join("native.png");
    fs::copy(PNG, &source).unwrap();
    fs::copy(PNG, &dest).unwrap();
    let seed = oracle
        .command()
        .args(["-overwrite_original", "-PNG:Title=", "-PNG:Copyright="])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(seed.status.success(), "{seed:?}");
    fs::copy(&dest, &native).unwrap();
    let seed = oracle
        .command()
        .args(["-overwrite_original", "-PNG:Title=copy title"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(seed.status.success(), "{seed:?}");
    let before = fs::read(&dest).unwrap();
    let args = [
        "-TagsFromFile",
        source.to_str().unwrap(),
        "-PNG:Title",
        "-PNG:Title=",
    ];
    let theirs = oracle
        .command()
        .args(["-overwrite_original"])
        .args(args)
        .arg(&native)
        .output()
        .unwrap();
    assert!(theirs.status.success(), "{theirs:?}");
    assert!(
        String::from_utf8_lossy(&theirs.stdout).contains("1 image files unchanged"),
        "{theirs:?}"
    );
    let ours = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("--backup")
        .args(args)
        .arg(&dest)
        .output()
        .unwrap();
    assert!(ours.status.success(), "{ours:?}");
    assert!(
        String::from_utf8_lossy(&ours.stdout).contains("1 image files unchanged"),
        "{ours:?}"
    );
    assert_eq!(fs::read(&dest).unwrap(), before);
    assert!(!dir.path().join("dest.png.bak").exists());

    // An unrelated deletion must not cancel the surviving same-value copy.
    let seed = oracle
        .command()
        .args(["-overwrite_original", "-PNG:Title=copy title"])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(seed.status.success());
    let ours = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args([
            "-TagsFromFile",
            source.to_str().unwrap(),
            "-PNG:Title",
            "-PNG:Copyright=",
        ])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(ours.status.success(), "{ours:?}");
    assert!(
        String::from_utf8_lossy(&ours.stdout).contains("1 image files updated"),
        "{ours:?}"
    );
}

#[test]
fn a_later_group_deletion_cancels_unsupported_pre_copy_sets() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.jpg");
    let dest = dir.path().join("dest.jpg");
    let native = dir.path().join("native.jpg");
    fs::copy(JPEG, &source).unwrap();
    fs::copy(JPEG, &dest).unwrap();
    fs::copy(JPEG, &native).unwrap();
    let args = [
        "-XMP-dc:Title=x",
        "-TagsFromFile",
        source.to_str().unwrap(),
        "-Make",
        "-XMP:All=",
    ];
    let theirs = oracle
        .command()
        .arg("-overwrite_original")
        .args(args)
        .arg(&native)
        .output()
        .unwrap();
    assert!(theirs.status.success(), "{theirs:?}");
    let ours = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(&dest)
        .output()
        .unwrap();
    assert!(ours.status.success(), "oracle {theirs:?}; ours {ours:?}");
    assert!(
        String::from_utf8_lossy(&ours.stdout).contains("1 image files updated"),
        "{ours:?}"
    );
}
