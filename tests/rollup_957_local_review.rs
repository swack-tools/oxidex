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
        oracle,
        &source,
        &["-ExifIFD:CreateDate=2011:02:03 04:05:06"],
    );
    seed(
        oracle,
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
    assert_eq!(tags(oracle, &ours), tags(oracle, &native));
    assert_eq!(raw_create_dates(&ours), raw_create_dates(&native));
    assert_eq!(
        raw_create_dates(&ours).0.as_deref(),
        Some(b"2020:01:02 03:04:05\0".as_slice())
    );
    assert_eq!(
        raw_create_dates(&ours).1.as_deref(),
        Some(b"2011:02:03 04:05:06\0".as_slice())
    );
}

/// Read the physical 0x9004 ASCII entries from a JPEG's TIFF payload. The
/// metadata reader can surface only one same-ID occurrence, so a row-only
/// comparison would miss deletion of the other directory's value.
fn raw_create_dates(path: &Path) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let bytes = fs::read(path).unwrap();
    let at = bytes.windows(6).position(|w| w == b"Exif\0\0").unwrap() + 6;
    let tiff = &bytes[at..];
    let little = &tiff[..2] == b"II";
    let word = |p: usize| -> usize {
        let b: [u8; 2] = tiff[p..p + 2].try_into().unwrap();
        usize::from(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let dword = |p: usize| -> usize {
        let b: [u8; 4] = tiff[p..p + 4].try_into().unwrap();
        (if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }) as usize
    };
    let entry = |dir: usize, id: usize| -> Option<usize> {
        (0..word(dir))
            .map(|n| dir + 2 + n * 12)
            .find(|&p| word(p) == id)
    };
    let value = |dir: usize| -> Option<Vec<u8>> {
        let p = entry(dir, 0x9004)?;
        assert_eq!(word(p + 2), 2);
        let count = dword(p + 4);
        let offset = if count <= 4 { p + 8 } else { dword(p + 8) };
        Some(tiff[offset..offset + count].to_vec())
    };
    let ifd0 = dword(4);
    let exif = entry(ifd0, 0x8769).map(|p| dword(p + 8));
    (value(ifd0), exif.and_then(value))
}

#[test]
fn copied_exif_create_date_survives_later_ifd0_set_matrix() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for source_both in [false, true] {
        for target_exists in [false, true] {
            let dir = TempDir::new().unwrap();
            let source = dir.path().join("source.jpg");
            let ours = dir.path().join("ours.jpg");
            let native = dir.path().join("native.jpg");
            fs::copy(JPEG, &source).unwrap();
            fs::copy(JPEG, &ours).unwrap();
            let mut source_seed = vec!["-ExifIFD:CreateDate=2011:02:03 04:05:06"];
            if source_both {
                source_seed.push("-IFD0:CreateDate=2012:02:03 04:05:06");
            }
            seed(oracle, &source, &source_seed);
            let mut dest_seed = vec!["-ExifIFD:CreateDate=2001:02:03 04:05:06"];
            if target_exists {
                dest_seed.push("-IFD0:CreateDate=2002:02:03 04:05:06");
            }
            seed(oracle, &ours, &dest_seed);
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
            assert!(
                expected.status.success(),
                "{source_both} {target_exists}: {expected:?}"
            );
            let actual = Command::new(env!("CARGO_BIN_EXE_oxidex"))
                .args(args)
                .arg(&ours)
                .output()
                .unwrap();
            assert!(
                actual.status.success(),
                "{source_both} {target_exists}: {actual:?}"
            );
            let raw = raw_create_dates(&ours);
            assert_eq!(
                raw,
                raw_create_dates(&native),
                "{source_both} {target_exists}"
            );
            assert_eq!(raw.0.as_deref(), Some(b"2020:01:02 03:04:05\0".as_slice()));
            assert_eq!(raw.1.as_deref(), Some(b"2011:02:03 04:05:06\0".as_slice()));
        }
    }
}

#[test]
fn explicit_per_directory_sets_keep_both_while_a_single_set_moves() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for both_sets in [false, true] {
        let dir = TempDir::new().unwrap();
        let ours = dir.path().join("ours.jpg");
        let native = dir.path().join("native.jpg");
        fs::copy(JPEG, &ours).unwrap();
        seed(oracle, &ours, &["-ExifIFD:CreateDate=2001:02:03 04:05:06"]);
        fs::copy(&ours, &native).unwrap();
        let mut args = Vec::new();
        if both_sets {
            args.push("-ExifIFD:CreateDate=2011:02:03 04:05:06");
        }
        args.push("-IFD0:CreateDate=2020:01:02 03:04:05");
        let expected = oracle
            .command()
            .arg("-overwrite_original")
            .args(&args)
            .arg(&native)
            .output()
            .unwrap();
        assert!(expected.status.success(), "{both_sets}: {expected:?}");
        let actual = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(&args)
            .arg(&ours)
            .output()
            .unwrap();
        assert!(actual.status.success(), "{both_sets}: {actual:?}");
        let raw = raw_create_dates(&ours);
        assert_eq!(raw, raw_create_dates(&native), "{both_sets}");
        assert_eq!(raw.0.as_deref(), Some(b"2020:01:02 03:04:05\0".as_slice()));
        assert_eq!(
            raw.1.as_deref(),
            both_sets.then_some(b"2011:02:03 04:05:06\0".as_slice())
        );
    }
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
            oracle,
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
fn clear_all_cancels_only_copies_that_precede_it() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for (index, selectors) in [
        vec!["-TagsFromFile", "SOURCE", "-Make>XMP-dc:Title", "-all="],
        vec!["-TagsFromFile", "SOURCE", "-Make>NoSuchTag", "-all="],
        vec!["-all=", "-TagsFromFile", "SOURCE", "-IFD0:Make"],
    ]
    .into_iter()
    .enumerate()
    {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.jpg");
        let ours = dir.path().join("ours.jpg");
        let native = dir.path().join("native.jpg");
        fs::copy(JPEG, &source).unwrap();
        fs::copy(JPEG, &ours).unwrap();
        seed(oracle, &source, &["-IFD0:Make=source maker"]);
        fs::copy(&ours, &native).unwrap();
        let selectors: Vec<_> = selectors
            .into_iter()
            .map(|s| {
                if s == "SOURCE" {
                    source.to_str().unwrap()
                } else {
                    s
                }
            })
            .collect();
        let expected = oracle
            .command()
            .arg("-overwrite_original")
            .args(&selectors)
            .arg(&native)
            .output()
            .unwrap();
        assert!(expected.status.success(), "{expected:?}");
        let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(&selectors)
            .arg(&ours)
            .output()
            .unwrap();
        assert!(result.status.success(), "case {index}: {result:?}");
        assert_eq!(tags(oracle, &ours), tags(oracle, &native), "case {index}");
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
            oracle,
            &source,
            &[
                "-IFD0:Artist=source artist",
                "-IFD0:Make=source maker",
                "-IFD0:Model=source model",
            ],
        );
        seed(
            oracle,
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
        assert_eq!(tags(oracle, &ours), tags(oracle, &native), "{selectors:?}");
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
        oracle,
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
        seed(oracle, &ours, &["-all="]);
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
        assert_eq!(tags(oracle, &ours), tags(oracle, &native), "{selectors:?}");
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
    assert_eq!(tags(oracle, &ours)["PNG:GPSDateStamp"], "old");
    let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-PNG:GPSDateStamp=2024-01-02")
        .arg(&ours)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(tags(oracle, &ours)["PNG:GPSDateStamp"], "2024-01-02");
}

#[test]
fn deletion_of_an_absent_field_remains_a_final_postcondition() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for args in [
        ["-ExifIFD:ISO=200", "-ExifIFD:ColorSpace="],
        ["-ExifIFD:ColorSpace=", "-ExifIFD:ISO=200"],
    ] {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("blank.jpg");
        fs::copy(JPEG, &path).unwrap();
        seed(oracle, &path, &["-all="]);
        let before = fs::read(&path).unwrap();
        // An absent deletion by itself still needs no write or backup.
        let noop = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["--backup", "-ExifIFD:ColorSpace="])
            .arg(&path)
            .output()
            .unwrap();
        assert!(noop.status.success(), "{noop:?}");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!dir.path().join("blank.jpg.bak").exists());
        // ISO creates ExifIFD's mandatory ColorSpace. The strict transaction
        // must refuse rather than commit a field explicitly requested absent.
        let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg("--backup")
            .args(args)
            .arg(&path)
            .output()
            .unwrap();
        assert!(!result.status.success(), "{args:?}: {result:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("ColorSpace"),
            "{result:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!dir.path().join("blank.jpg.bak").exists());
        // A later explicit set replaces the deletion's postcondition.
        let replacement = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args([
                "-ExifIFD:ColorSpace=",
                "-ExifIFD:ISO=200",
                "-ExifIFD:ColorSpace#=1",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(replacement.status.success(), "{replacement:?}");
        let color_space = oracle
            .command()
            .args(["-n", "-s3", "-ExifIFD:ColorSpace"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(color_space.status.success(), "{color_space:?}");
        assert_eq!(String::from_utf8_lossy(&color_space.stdout).trim(), "1");
    }
}

#[test]
fn the_last_copy_selector_controls_destination_strictness() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.jpg");
    fs::copy(JPEG, &source).unwrap();
    seed(
        oracle,
        &source,
        &[
            "-all=",
            "-IFD0:Make=source maker",
            "-XMP-dc:Title=source title",
        ],
    );
    for (index, filters, succeeds) in [
        (0, ["-XMP-dc:Title", "-all"], true),
        (1, ["-all", "-XMP-dc:Title"], false),
    ] {
        let path = dir.path().join(format!("dest{index}.jpg"));
        fs::copy(JPEG, &path).unwrap();
        seed(oracle, &path, &["-all="]);
        let before = fs::read(&path).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-TagsFromFile", source.to_str().unwrap()])
            .args(filters)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(result.status.success(), succeeds, "{filters:?}: {result:?}");
        if succeeds {
            assert_eq!(tags(oracle, &path)["IFD0:Make"], "source maker");
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("XMP"),
                "{result:?}"
            );
        } else {
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }
}

#[test]
fn surviving_same_value_sets_count_with_noop_shifts_or_clears() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for prefix in ["-DateTimeOriginal+=1", "-all="] {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ours.jpg");
        let native = dir.path().join("native.jpg");
        fs::copy(JPEG, &path).unwrap();
        let seed = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-all=", "-IFD0:Artist=x"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(seed.status.success(), "{seed:?}");
        fs::copy(&path, &native).unwrap();
        let before = fs::read(&path).unwrap();
        let expected = oracle
            .command()
            .args(["-overwrite_original", prefix, "-IFD0:Artist=x"])
            .arg(&native)
            .output()
            .unwrap();
        assert!(expected.status.success(), "{expected:?}");
        assert!(
            String::from_utf8_lossy(&expected.stdout).contains("1 image files updated"),
            "{expected:?}"
        );
        let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["--backup", prefix, "-IFD0:Artist=x"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stdout).contains("1 image files updated"),
            "{prefix}: {result:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read(dir.path().join("ours.jpg.bak")).unwrap(), before);
    }
}
