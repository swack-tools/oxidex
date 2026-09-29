//! The direct JPEG date shifter must not update only the first EXIF APP1.

use oxidex::core::date_shift::{ExifDateTag, ShiftOperation, build_shift_spec};
use oxidex::exiftool_oracle;
use oxidex::writers::exif_inplace::shift_jpeg_exif_dates;
use std::path::Path;
use std::process::Command;

#[path = "common/fixtures.rs"]
mod fixtures;

fn first_exif_segment(data: &[u8]) -> (usize, usize) {
    let mut at = 2;
    loop {
        assert_eq!(data[at], 0xff);
        let marker = data[at + 1];
        assert!(!matches!(marker, 0xda | 0xd9));
        let length = u16::from_be_bytes([data[at + 2], data[at + 3]]) as usize;
        let end = at + 2 + length;
        if marker == 0xe1 && data[at + 4..end].starts_with(b"Exif\0\0") {
            return (at, end);
        }
        at = end;
    }
}

fn two_exif_blocks(host: &[u8], donor: &[u8]) -> Vec<u8> {
    let (_, end) = first_exif_segment(host);
    let (start, stop) = first_exif_segment(donor);
    [&host[..end], &donor[start..stop], &host[end..]].concat()
}

fn fixture(name: &str) -> Option<Vec<u8>> {
    fixtures::pinned_t_images_fixture_path(name).map(|path| std::fs::read(path).unwrap())
}

#[test]
fn grouped_create_date_shift_refuses_two_present_app1_blocks_atomically() {
    let (Some(oracle), Some(canon), Some(nikon)) = (
        exiftool_oracle::graded(),
        fixture("Canon.jpg"),
        fixture("Nikon.jpg"),
    ) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dual.jpg");
    std::fs::write(&path, two_exif_blocks(&canon, &nikon)).unwrap();
    let seed = oracle
        .command()
        .args([
            "-overwrite_original",
            "-IFD0:CreateDate=2000:01:02 03:04:05",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(seed.status.success(), "{seed:?}");
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        before
            .windows(b"2000:01:02 03:04:05".len())
            .filter(|window| *window == b"2000:01:02 03:04:05")
            .count(),
        2,
        "both physical APP1 records must hold the date"
    );
    let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-IFD0:CreateDate+=1:0:0 0:0:0")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("2 EXIF APP1 blocks"),
        "{out:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn absent_date_shift_proves_both_app1_blocks_and_stays_unchanged() {
    let source = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg"),
    )
    .unwrap();
    let dual = two_exif_blocks(&source, &source);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.jpg");
    std::fs::write(&path, &dual).unwrap();
    let spec = build_shift_spec("1:00:00", ShiftOperation::Add).unwrap();
    assert_eq!(
        shift_jpeg_exif_dates(&path, &[ExifDateTag::DateTimeOriginal], &spec).unwrap(),
        0
    );
    assert_eq!(std::fs::read(&path).unwrap(), dual);
}

#[test]
fn unreadable_date_entries_do_not_prove_absence_in_two_app1_blocks() {
    let source = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg"),
    )
    .unwrap();
    let mut dual = two_exif_blocks(&source, &source);
    let marker = [0x32, 0x01, 0x02, 0x00, 0x14, 0x00, 0x00, 0x00];
    let positions: Vec<usize> = dual
        .windows(marker.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == marker).then_some(at))
        .collect();
    assert_eq!(positions.len(), 2);
    for at in positions {
        dual[at + 2] = 7; // TIFF UNDEFINED instead of ASCII.
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unreadable.jpg");
    std::fs::write(&path, &dual).unwrap();
    let spec = build_shift_spec("1:00:00", ShiftOperation::Add).unwrap();
    let error = shift_jpeg_exif_dates(&path, &[ExifDateTag::ModifyDate], &spec).unwrap_err();
    assert!(error.to_string().contains("2 EXIF APP1 blocks"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), dual);
}

#[test]
fn one_app1_date_shift_still_writes() {
    let source = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg"),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.jpg");
    std::fs::write(&path, &source).unwrap();
    let spec = build_shift_spec("1:00:00", ShiftOperation::Add).unwrap();
    assert_eq!(
        shift_jpeg_exif_dates(&path, &[ExifDateTag::ModifyDate], &spec).unwrap(),
        1
    );
    assert_ne!(std::fs::read(&path).unwrap(), source);
}
