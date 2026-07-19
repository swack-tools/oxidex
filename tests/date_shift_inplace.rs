//! Integration tests for in-place EXIF date shifting (GitHub issue #14).
//!
//! The critical property: shifting a date must not change ANY byte of the
//! file outside the 19 ASCII characters of the target datetime value(s).
//! The GPS fixture contains ComponentsConfiguration and GPSVersionID, the
//! binary tags that the old whole-map rewrite corrupted.

use oxidex::core::date_shift::{ExifDateTag, ShiftOperation, build_shift_spec};
use oxidex::core::operations::read_metadata;
use oxidex::writers::exif_inplace::shift_jpeg_exif_dates;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg")
        .join(name)
}

fn temp_copy(src: &Path, label: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!("oxidex_shift_{}_{}", std::process::id(), label));
    std::fs::copy(src, &dst).unwrap();
    dst
}

/// Returns the byte indices at which the two files differ.
fn diff_indices(a: &Path, b: &Path) -> Vec<usize> {
    let a = std::fs::read(a).unwrap();
    let b = std::fs::read(b).unwrap();
    assert_eq!(a.len(), b.len(), "file length must not change");
    a.iter()
        .zip(b.iter())
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn inplace_shift_changes_only_datetime_bytes() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "bytediff.jpg");

    let spec = build_shift_spec("1:00:00", ShiftOperation::Subtract).unwrap();
    let modified = shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 1);

    let diffs = diff_indices(&src, &dst);
    assert!(!diffs.is_empty(), "the datetime bytes must have changed");
    assert!(
        diffs.last().unwrap() - diffs.first().unwrap() < 19,
        "all changed bytes must lie within one 19-byte datetime value, got {:?}",
        diffs
    );

    // 2024-02-01T14:30:00 minus 1 hour
    let metadata = read_metadata(&dst).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2024-02-01T13:30:00+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn inplace_shift_binary_tags_survive() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "binary_survive.jpg");

    let spec = build_shift_spec("0:0:0 1:00:00", ShiftOperation::Subtract).unwrap();
    shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap();

    // The corruption canaries must read back identically
    let before = read_metadata(&src).unwrap();
    let after = read_metadata(&dst).unwrap();
    for canary in ["ExifIFD:ComponentsConfiguration", "GPS:GPSVersionID"] {
        assert_eq!(
            before.get(canary),
            after.get(canary),
            "binary tag {} must survive a date shift unchanged",
            canary
        );
    }

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn inplace_set_absolute_value() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "set_abs.jpg");

    let spec = build_shift_spec("2030:01:02 03:04:05", ShiftOperation::Set).unwrap();
    let modified = shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 1);

    let metadata = read_metadata(&dst).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2030-01-02T03:04:05+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn inplace_shift_missing_tag_returns_zero() {
    // sample_with_exif.jpg has ModifyDate but no DateTimeOriginal
    let src = fixture("sample_with_exif.jpg");
    let dst = temp_copy(&src, "missing_tag.jpg");

    let spec = build_shift_spec("1", ShiftOperation::Subtract).unwrap();
    let modified = shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 0);
    // File must be untouched when nothing matched
    assert!(diff_indices(&src, &dst).is_empty());

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn inplace_shift_no_exif_errors() {
    // A JPEG with no EXIF APP1 segment at all
    let dst = std::env::temp_dir().join(format!("oxidex_shift_{}_noexif.jpg", std::process::id()));
    std::fs::write(&dst, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();

    let spec = build_shift_spec("1", ShiftOperation::Subtract).unwrap();
    let err = shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap_err();
    assert!(err.to_string().contains("No EXIF data"), "got: {}", err);

    std::fs::remove_file(&dst).unwrap();
}
