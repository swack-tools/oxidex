//! Public API regressions for PR976's copy selector and PNG clear findings.
use oxidex::core::operations::{clear_all_metadata, copy_metadata_report, read_metadata};
use oxidex::exiftool_oracle;
use std::{fs, path::Path};
use tempfile::TempDir;

// Isolate abort-prone or exponential matcher regressions and enforce a
// generous completion bound, so a reintroduced bug cannot hang the suite.
fn bounded_public_copy(test: &str, check: impl FnOnce()) {
    if std::env::var("OXIDEX_COPY_PNG_CHILD").as_deref() == Ok(test) {
        check();
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--nocapture"])
        .env("OXIDEX_COPY_PNG_CHILD", test)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{test}: child {status}");
            return;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{test}: public copy exceeded five seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

const JPEG: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";
const DEST: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";

#[test]
fn wildcard_only_group_retains_physical_make_group() {
    let oracle = exiftool_oracle::graded().expect("pinned oracle required");
    eprintln!("{}; command: {}", oracle.provenance(), oracle.display());
    let dir = TempDir::new().unwrap();
    for selector in ["*:Make", "IFD0:m?KE", "EXIF:M*", "*:M*ke"] {
        let ours = dir.path().join("ours.jpg");
        let native = dir.path().join("native.jpg");
        fs::copy(DEST, &ours).unwrap();
        fs::copy(DEST, &native).unwrap();
        let output = oracle
            .command()
            .args(["-overwrite_original", "-TagsFromFile", JPEG])
            .arg(format!("-{selector}"))
            .arg(&native)
            .output()
            .unwrap();
        assert!(output.status.success(), "{selector}: {output:?}");
        let report =
            copy_metadata_report(Path::new(JPEG), &ours, Some(&[selector.into()])).unwrap();
        assert!(report.copied > 0, "{selector}: {report:?}");
        let actual = read_metadata(&ours).unwrap();
        let expected = read_metadata(&native).unwrap();
        assert_eq!(
            actual.get("IFD0:Make"),
            expected.get("IFD0:Make"),
            "{selector}"
        );
        assert!(actual.get("IFD0:Make").is_some());
        assert!(
            actual.get("XMP-tiff:Make").is_none(),
            "source group retained"
        );
    }
    // A nonpreferred physical group must stay grouped too. OxiDex cannot
    // write XMP into JPEG, so it must report this field instead of silently
    // moving it into the writable preferred IFD0 group.
    let source = dir.path().join("xmp-source.jpg");
    let ours = dir.path().join("xmp-ours.jpg");
    let native = dir.path().join("xmp-native.jpg");
    for path in [&source, &ours, &native] {
        fs::copy(DEST, path).unwrap();
    }
    let seed = oracle
        .command()
        .args([
            "-overwrite_original",
            "-IFD0:Make=",
            "-XMP-tiff:Make=physical XMP make",
        ])
        .arg(&source)
        .output()
        .unwrap();
    assert!(seed.status.success(), "{seed:?}");
    let source_map = read_metadata(&source).unwrap();
    assert!(source_map.get("IFD0:Make").is_none());
    assert!(source_map.get("XMP-tiff:Make").is_some());
    let before = fs::read(&ours).unwrap();
    let original_make = read_metadata(&ours).unwrap().get("IFD0:Make").cloned();
    let output = oracle
        .command()
        .args(["-overwrite_original", "-TagsFromFile"])
        .arg(&source)
        .arg("-*:Make")
        .arg(&native)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let native_map = read_metadata(&native).unwrap();
    assert_eq!(native_map.get("IFD0:Make"), original_make.as_ref());
    assert_eq!(
        native_map.get("XMP-tiff:Make"),
        source_map.get("XMP-tiff:Make")
    );
    let report = copy_metadata_report(&source, &ours, Some(&["*:Make".into()])).unwrap();
    assert_eq!(report.copied, 0, "{report:?}");
    assert!(
        report
            .uncopied_tags
            .iter()
            .any(|tag| tag.tag == "XMP-tiff:Make"),
        "{report:?}"
    );
    assert!(
        report
            .uncopied_groups
            .iter()
            .any(|group| group == "XMP-tiff"),
        "{report:?}"
    );
    assert_eq!(fs::read(&ours).unwrap(), before);

    // Existing OxiDex wildcard-group grammar is broader than native's
    // literal source-group constraints; preserve it without a parity claim.
    let ours = dir.path().join("group-wildcard.jpg");
    fs::copy(DEST, &ours).unwrap();
    let report = copy_metadata_report(Path::new(JPEG), &ours, Some(&["IFD?:M?ke".into()])).unwrap();
    assert!(report.copied > 0, "{report:?}");
    assert_eq!(
        read_metadata(&ours).unwrap().get("IFD0:Make"),
        read_metadata(Path::new(JPEG)).unwrap().get("IFD0:Make")
    );
}

#[test]
fn long_stars_match_at_public_copy_boundary() {
    bounded_public_copy("long_stars_match_at_public_copy_boundary", || {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("ours.jpg");
        fs::copy(DEST, &dest).unwrap();
        for selector in [
            format!("{}M?ke", "*".repeat(100_000)),
            format!("{}:Make", "*".repeat(100_000)),
        ] {
            fs::copy(DEST, &dest).unwrap();
            let report = copy_metadata_report(Path::new(JPEG), &dest, Some(&[selector])).unwrap();
            assert!(report.copied > 0, "{report:?}");
            assert!(read_metadata(&dest).unwrap().get("IFD0:Make").is_some());
        }
    });
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = (data.len() as u32).to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(data);
    let mut crc = !0u32;
    for b in &bytes[4..] {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    bytes.extend((!crc).to_be_bytes());
    bytes
}

fn png(extra: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    for c in extra {
        bytes.extend(c);
    }
    // Valid zlib stream for one black RGB pixel and its filter byte.
    bytes.extend(chunk(
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x60, 0x60, 0x60, 0, 0, 0, 4, 0, 1],
    ));
    bytes.extend(chunk(b"IEND", &[]));
    bytes
}

#[test]
fn alternating_stars_nonmatch_at_public_copy_boundary() {
    bounded_public_copy("alternating_stars_nonmatch_at_public_copy_boundary", || {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.png");
        let dest = dir.path().join("dest.png");
        let data = format!("{}\0value", "A".repeat(40));
        fs::write(&source, png(&[chunk(b"tEXt", data.as_bytes())])).unwrap();
        fs::write(&dest, png(&[])).unwrap();
        let before = fs::read(&dest).unwrap();
        let report =
            copy_metadata_report(&source, &dest, Some(&[format!("{}Z", "*A".repeat(24))])).unwrap();
        assert_eq!(report.copied, 0, "{report:?}");
        assert_eq!(fs::read(&dest).unwrap(), before);
    });
}

#[test]
fn png_clear_preserves_case_permutations_bytes_crc_and_order() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("permutations.png");
    let mut extras = vec![chunk(b"vpAg", b"before")];
    let mut retained = extras.clone();
    for mask in 0..16 {
        let mut kind = *b"zxif";
        for (i, c) in kind.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                *c = c.to_ascii_uppercase();
            }
        }
        let bytes = chunk(&kind, &[mask]);
        if kind != *b"zxIf" {
            retained.push(bytes.clone());
        }
        extras.push(bytes);
    }
    extras.push(chunk(b"tEXt", b"Title\0remove"));
    extras.push(chunk(b"vpAg", b"after"));
    retained.push(chunk(b"vpAg", b"after"));
    fs::write(&file, png(&extras)).unwrap();
    clear_all_metadata(&file).unwrap();
    assert_eq!(
        fs::read(&file).unwrap(),
        png(&retained),
        "only exact listed chunks may disappear"
    );
    clear_all_metadata(&file).unwrap();
    assert_eq!(fs::read(&file).unwrap(), png(&retained));
}
