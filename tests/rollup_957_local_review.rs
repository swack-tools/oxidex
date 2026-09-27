//! Regressions from the local review of the reconstructed #957 candidate.
use oxidex::exiftool_oracle;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";

fn seed(oracle: &exiftool_oracle::Oracle, path: &Path, assignments: &[&str]) {
    let output = oracle
        .command()
        .arg("-overwrite_original")
        .args(assignments)
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success(), "seed {assignments:?}: {output:?}");
}

fn tags(oracle: &exiftool_oracle::Oracle, path: &Path) -> serde_json::Value {
    let output = oracle
        .command()
        .args([
            "-G1",
            "-s",
            "-j",
            "-CreateDate",
            "-Artist",
            "-Make",
            "-GPSDateStamp",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let mut rows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    rows[0].as_object_mut().unwrap().remove("SourceFile");
    rows.remove(0)
}

#[test]
fn individual_copy_and_set_keep_distinct_exif_directories() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.jpg");
    let ours = dir.path().join("ours.jpg");
    let native = dir.path().join("native.jpg");
    fs::copy(JPEG, &source).unwrap();
    fs::copy(JPEG, &ours).unwrap();
    seed(
        &oracle,
        &source,
        &["-ExifIFD:CreateDate=2011:02:03 04:05:06"],
    );
    seed(
        &oracle,
        &ours,
        &[
            "-ExifIFD:CreateDate=2001:02:03 04:05:06",
            "-IFD0:CreateDate=2002:02:03 04:05:06",
        ],
    );
    fs::copy(&ours, &native).unwrap();
    let args = [
        "-TagsFromFile",
        source.to_str().unwrap(),
        "-ExifIFD:CreateDate",
        "-IFD0:CreateDate=2020:01:02 03:04:05",
    ];
    let expected = oracle
        .command()
        .arg("-overwrite_original")
        .args(args)
        .arg(&native)
        .output()
        .unwrap();
    assert!(expected.status.success(), "{expected:?}");
    let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(&ours)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(tags(&oracle, &ours), tags(&oracle, &native));
}

#[test]
fn later_group_deletion_cancels_unsupported_named_copies_before_validation() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for selector in ["-XMP-dc:Title", "-IFD0:Artist>XMP-dc:Title"] {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.jpg");
        let ours = dir.path().join("ours.jpg");
        let native = dir.path().join("native.jpg");
        fs::copy(JPEG, &source).unwrap();
        fs::copy(JPEG, &ours).unwrap();
        seed(
            &oracle,
            &source,
            &["-IFD0:Artist=source", "-XMP-dc:Title=source"],
        );
        fs::copy(&ours, &native).unwrap();
        let before = fs::read(&ours).unwrap();
        let args = [
            "-TagsFromFile",
            source.to_str().unwrap(),
            selector,
            "-XMP:All=",
        ];
        let expected = oracle
            .command()
            .arg("-overwrite_original")
            .args(args)
            .arg(&native)
            .output()
            .unwrap();
        assert!(expected.status.success(), "{expected:?}");
        assert_eq!(fs::read(&native).unwrap(), before);
        let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg("--backup")
            .args(args)
            .arg(&ours)
            .output()
            .unwrap();
        assert!(result.status.success(), "{selector}: {result:?}");
        assert_eq!(fs::read(&ours).unwrap(), before);
        assert!(!dir.path().join("ours.jpg.bak").exists());
    }
}

#[test]
fn copy_selector_order_and_exclusions_match_the_pinned_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let cases: &[&[&str]] = &[
        &["-IFD0:Make>IFD0:Artist", "-IFD0:all"],
        &["-IFD0:all", "-IFD0:Make>IFD0:Artist"],
        &["-IFD0:Model>IFD0:Artist", "-IFD0:Art*"],
        &["-IFD0:Art*", "-IFD0:Model>IFD0:Artist"],
        &["-IFD0:Artist", "--IFD0:Artist"],
        &["-IFD0:Artist", "--IFD0:Artist", "-IFD0:Artist"],
        &["-IFD0:all", "--IFD0:Artist"],
        &["--IFD0:Artist", "-IFD0:all"],
        &["-IFD0:Artist>XMP-dc:Title", "--IFD0:Artist"],
    ];
    for selectors in cases {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.jpg");
        let ours = dir.path().join("ours.jpg");
        let native = dir.path().join("native.jpg");
        fs::copy(JPEG, &source).unwrap();
        fs::copy(JPEG, &ours).unwrap();
        seed(
            &oracle,
            &source,
            &[
                "-IFD0:Artist=source artist",
                "-IFD0:Make=source maker",
                "-IFD0:Model=source model",
            ],
        );
        seed(
            &oracle,
            &ours,
            &[
                "-IFD0:Artist=destination artist",
                "-IFD0:Make=destination maker",
            ],
        );
        fs::copy(&ours, &native).unwrap();
        let expected = oracle
            .command()
            .arg("-overwrite_original")
            .args(["-TagsFromFile", source.to_str().unwrap()])
            .args(*selectors)
            .arg(&native)
            .output()
            .unwrap();
        assert!(expected.status.success(), "{selectors:?}: {expected:?}");
        let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-TagsFromFile", source.to_str().unwrap()])
            .args(*selectors)
            .arg(&ours)
            .output()
            .unwrap();
        assert!(result.status.success(), "{selectors:?}: {result:?}");
        assert_eq!(
            tags(&oracle, &ours),
            tags(&oracle, &native),
            "{selectors:?}"
        );
    }
}

#[test]
fn cancelled_copies_do_not_create_directories_for_surviving_copies() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.jpg");
    fs::copy(JPEG, &source).unwrap();
    seed(
        &oracle,
        &source,
        &[
            "-IFD0:Make=source maker",
            "-ExifIFD:DateTimeOriginal=2020:01:02 03:04:05",
        ],
    );
    for (fixture, extension, selectors) in [
        (
            "tests/fixtures/png/sample.png",
            "png",
            vec![
                "-Make",
                "-ExifIFD:DateTimeOriginal",
                "-ExifIFD:DateTimeOriginal=",
            ],
        ),
        (JPEG, "jpg", vec!["-Make", "-IFD0:Make="]),
    ] {
        let ours = dir.path().join(format!("ours.{extension}"));
        let native = dir.path().join(format!("native.{extension}"));
        fs::copy(fixture, &ours).unwrap();
        seed(&oracle, &ours, &["-all="]);
        fs::copy(&ours, &native).unwrap();
        let expected = oracle
            .command()
            .arg("-overwrite_original")
            .args(["-TagsFromFile", source.to_str().unwrap()])
            .args(&selectors)
            .arg(&native)
            .output()
            .unwrap();
        assert!(expected.status.success(), "{expected:?}");
        let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-TagsFromFile", source.to_str().unwrap()])
            .args(&selectors)
            .arg(&ours)
            .output()
            .unwrap();
        assert!(result.status.success(), "{selectors:?}: {result:?}");
        assert_eq!(
            tags(&oracle, &ours),
            tags(&oracle, &native),
            "{selectors:?}"
        );
    }
}

#[test]
fn png_gps_date_stamp_keyword_remains_opaque_text() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let ours = dir.path().join("ours.png");
    fs::copy("tests/fixtures/png/sample.png", &ours).unwrap();
    // Literal PNG text fixture with an independently computed CRC. The oracle
    // reads this keyword but refuses to write it; this tests OxiDex's existing
    // opaque-text contract, without claiming native write parity for the key.
    let mut bytes = fs::read(&ours).unwrap();
    let iend = bytes.len() - 12;
    assert_eq!(&bytes[iend + 4..iend + 8], b"IEND");
    bytes.splice(iend..iend, b"\x00\x00\x00\x10\x74\x45\x58\x74\x47\x50\x53\x44\x61\x74\x65\x53\x74\x61\x6d\x70\x00\x6f\x6c\x64\x1b\x99\x44\x66".iter().copied());
    fs::write(&ours, bytes).unwrap();
    assert_eq!(tags(&oracle, &ours)["PNG:GPSDateStamp"], "old");
    let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-PNG:GPSDateStamp=2024-01-02")
        .arg(&ours)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(tags(&oracle, &ours)["PNG:GPSDateStamp"], "2024-01-02");
}
