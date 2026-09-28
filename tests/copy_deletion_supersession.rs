//! Ordered single-tag deletions across TagsFromFile share assignment cancellation.
#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle;
use std::{fs, process::Command};
use tempfile::TempDir;

// As in rw2_embedded_ifd0.rs: renumber JpgFromRaw to an unknown tag,
// retaining the valid outer IFD and avoiding the separate embedded-JPEG
// update refusal. Pinned ExifTool can update this outer-only Make.
fn without_jpg_from_raw(mut bytes: Vec<u8>) -> Vec<u8> {
    assert_eq!(&bytes[..4], b"IIU\0");
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    let entry = (0..count)
        .map(|index| ifd + 2 + 12 * index)
        .find(|&at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) == 0x002e)
        .expect("fixture has JpgFromRaw");
    bytes[entry..entry + 2].copy_from_slice(&0x0040u16.to_le_bytes());
    bytes
}

fn ordered_case(deletion: &str, later: &str, succeeds: bool, refusal: &str) {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let fixture = if std::env::var("OXIDEX_REQUIRE_EXIFTOOL_ORACLE").as_deref() == Ok("1") {
        fixtures::required_t_images_fixture_path("Panasonic.rw2")
    } else {
        let Some(fixture) = fixtures::pinned_t_images_fixture_path("Panasonic.rw2") else {
            return;
        };
        fixture
    };
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.jpg");
    fs::copy("tests/fixtures/jpeg/simple/synthetic_001.jpg", &source).unwrap();
    let seed = oracle
        .command()
        .args(["-overwrite_original", "-all="])
        .arg(&source)
        .output()
        .unwrap();
    assert!(seed.status.success(), "{seed:?}");
    let dest = dir.path().join("dest.rw2");
    let bytes = fs::read(fixture).unwrap();
    // Group-wide deletes need the genuine embedded EXIF: the outer-only
    // fixture has no removable group, so its group delete is a valid no-op.
    let bytes = if deletion.ends_with(":All=") {
        bytes
    } else {
        without_jpg_from_raw(bytes)
    };
    fs::write(&dest, bytes).unwrap();
    let before = fs::read(&dest).unwrap();
    let source_before = fs::read(&source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["--backup", deletion, "-TagsFromFile"])
        .arg(&source)
        .args(["-all", later])
        .arg(&dest)
        .output()
        .unwrap();
    assert_eq!(
        out.status.success(),
        succeeds,
        "{deletion} / {later}: {out:?}"
    );
    let backup = dir.path().join("dest.rw2.bak");
    if succeeds {
        assert_ne!(fs::read(&dest).unwrap(), before);
        assert_eq!(fs::read(&backup).unwrap(), before);
        let read = oracle
            .command()
            .args(["-s3", "-IFD0:Make"])
            .arg(&dest)
            .output()
            .unwrap();
        assert!(read.status.success(), "{read:?}");
        assert_eq!(String::from_utf8_lossy(&read.stdout).trim(), "Acme");
    } else {
        assert_eq!(fs::read(&dest).unwrap(), before);
        assert!(!backup.exists());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(refusal),
            "{out:?}"
        );
    }
    assert_eq!(fs::read(source).unwrap(), source_before);
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        if succeeds { 3 } else { 2 }
    );
}

#[test]
fn later_assignment_cancels_single_tag_deletion_across_copy() {
    for (delete, set) in [
        ("-IFD0:Make=", "-IFD0:Make=Acme"),
        ("-IFD0:Make=", "-Make=Acme"),
        ("-IFD0:Make=", "-EXIF:Make=Acme"),
        ("-IFD0:Make#=", "-IFD0:Make=Acme"),
    ] {
        ordered_case(delete, set, true, "");
    }
}

#[test]
fn unrelated_or_group_deletions_across_copy_still_refuse_atomically() {
    for (delete, set) in [
        ("-IFD0:Make=", "-IFD0:Model=Acme"),
        ("-IFD0:Make=", "-ExifIFD:Make=Acme"),
        ("-IFD0:All=", "-IFD0:Make=Acme"),
        ("-EXIF:All=", "-IFD0:Make=Acme"),
        ("-IFD0:Make=", "-IFD0:Make="),
    ] {
        ordered_case(delete, set, false, "Failed to remove tag");
    }
}

#[test]
fn later_raw_assignment_cancels_single_tag_deletion_across_copy() {
    // A later raw assignment is writable and cancels the earlier deletion,
    // including when the two requests name the same tag through aliases.
    for (delete, set) in [
        ("-IFD0:Make=", "-IFD0:Make#=Acme"),
        ("-IFD0:Make#=", "-EXIF:Make#=Acme"),
    ] {
        ordered_case(delete, set, true, "");
    }
}
