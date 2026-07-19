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
use std::process::Command;
use tempfile::NamedTempFile;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg")
        .join(name)
}

fn temp_copy(src: &Path, _label: &str) -> NamedTempFile {
    let dst = NamedTempFile::new().unwrap();
    std::fs::copy(src, dst.path()).unwrap();
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
    let modified =
        shift_jpeg_exif_dates(dst.path(), &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 1);

    let diffs = diff_indices(&src, dst.path());
    assert!(!diffs.is_empty(), "the datetime bytes must have changed");
    assert!(
        diffs.last().unwrap() - diffs.first().unwrap() < 19,
        "all changed bytes must lie within one 19-byte datetime value, got {:?}",
        diffs
    );

    // 2024-02-01T14:30:00 minus 1 hour
    let metadata = read_metadata(dst.path()).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2024-02-01T13:30:00+00:00");
}

#[test]
fn inplace_shift_binary_tags_survive() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "binary_survive.jpg");

    let spec = build_shift_spec("0:0:0 1:00:00", ShiftOperation::Subtract).unwrap();
    shift_jpeg_exif_dates(dst.path(), &[ExifDateTag::DateTimeOriginal], &spec).unwrap();

    // The corruption canaries must read back identically
    let before = read_metadata(&src).unwrap();
    let after = read_metadata(dst.path()).unwrap();
    for canary in ["ExifIFD:ComponentsConfiguration", "GPS:GPSVersionID"] {
        assert_eq!(
            before.get(canary),
            after.get(canary),
            "binary tag {} must survive a date shift unchanged",
            canary
        );
    }
}

#[test]
fn inplace_set_absolute_value() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "set_abs.jpg");

    let spec = build_shift_spec("2030:01:02 03:04:05", ShiftOperation::Set).unwrap();
    let modified =
        shift_jpeg_exif_dates(dst.path(), &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 1);

    let metadata = read_metadata(dst.path()).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2030-01-02T03:04:05+00:00");
}

#[test]
fn inplace_shift_missing_tag_returns_zero() {
    // sample_with_exif.jpg has ModifyDate but no DateTimeOriginal
    let src = fixture("sample_with_exif.jpg");
    let dst = temp_copy(&src, "missing_tag.jpg");

    let spec = build_shift_spec("1", ShiftOperation::Subtract).unwrap();
    let modified =
        shift_jpeg_exif_dates(dst.path(), &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 0);
    // File must be untouched when nothing matched
    assert!(diff_indices(&src, dst.path()).is_empty());
}

#[test]
fn inplace_shift_no_exif_errors() {
    // A JPEG with no EXIF APP1 segment at all
    let dst = NamedTempFile::new().unwrap();
    std::fs::write(dst.path(), [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();

    let spec = build_shift_spec("1", ShiftOperation::Subtract).unwrap();
    let err =
        shift_jpeg_exif_dates(dst.path(), &[ExifDateTag::DateTimeOriginal], &spec).unwrap_err();
    assert!(err.to_string().contains("No EXIF data"), "got: {}", err);
}

#[test]
fn inplace_shift_beyond_year_range_errors_cleanly() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "year_overflow.jpg");

    // 8000 years past 2024 formats as a 5-digit year, which cannot fit the
    // fixed 19-byte EXIF value; must be a clean error, not a panic
    let spec = build_shift_spec("8000:0:0 0:0:0", ShiftOperation::Add).unwrap();
    let err =
        shift_jpeg_exif_dates(dst.path(), &[ExifDateTag::DateTimeOriginal], &spec).unwrap_err();
    assert!(err.to_string().contains("representable"), "got: {}", err);
    // File untouched
    assert!(diff_indices(&src, dst.path()).is_empty());
}

#[test]
fn shift_metadata_dates_bare_name_on_jpeg() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "public_api.jpg");

    // The exact tag pattern and offset string from issue #14
    oxidex::core::date_shift::shift_metadata_dates(
        dst.path(),
        "DateTimeOriginal",
        "1:00:00",
        ShiftOperation::Subtract,
    )
    .unwrap();

    let metadata = read_metadata(dst.path()).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2024-02-01T13:30:00+00:00");
}

#[test]
fn alldates_skips_unparseable_sibling_value() {
    // canon_sample.jpg has ModifyDate, DateTimeOriginal, and CreateDate all
    // equal to "2003:12:14 12:01:44" (three occurrences of that ASCII string)
    let src = fixture("makernotes/canon_sample.jpg");
    let dst = temp_copy(&src, "alldates_skip.jpg");

    // Corrupt the first occurrence in place to an unset-camera-clock value,
    // as real cameras write when the clock was never set
    let mut bytes = std::fs::read(dst.path()).unwrap();
    let needle = b"2003:12:14 12:01:44";
    let pos = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap();
    bytes[pos..pos + needle.len()].copy_from_slice(b"0000:00:00 00:00:00");
    std::fs::write(dst.path(), &bytes).unwrap();

    // AllDates must shift the two parseable tags and skip the corrupt one
    oxidex::core::date_shift::shift_metadata_dates(
        dst.path(),
        "AllDates",
        "1:00:00",
        ShiftOperation::Subtract,
    )
    .unwrap();

    let after = std::fs::read(dst.path()).unwrap();
    let count = |needle: &[u8]| after.windows(needle.len()).filter(|w| *w == needle).count();
    assert_eq!(
        count(b"2003:12:14 11:01:44"),
        2,
        "both valid tags must be shifted"
    );
    assert_eq!(
        count(b"0000:00:00 00:00:00"),
        1,
        "the corrupt tag must be left untouched"
    );
    assert_eq!(
        count(b"2003:12:14 12:01:44"),
        0,
        "no unshifted valid value may remain"
    );
}

#[test]
fn shift_metadata_dates_alldates_on_jpeg() {
    // The fixture's only canonical date tag is ExifIFD:DateTimeOriginal,
    // so AllDates shifts exactly that one
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "alldates.jpg");

    oxidex::core::date_shift::shift_metadata_dates(
        dst.path(),
        "AllDates",
        "0:0:0 1:00:00",
        ShiftOperation::Subtract,
    )
    .unwrap();

    let metadata = read_metadata(dst.path()).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2024-02-01T13:30:00+00:00");
}

#[test]
fn shift_metadata_dates_unsupported_jpeg_tag_errors_cleanly() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "unsupported.jpg");

    let err = oxidex::core::date_shift::shift_metadata_dates(
        dst.path(),
        "XMP:CreateDate",
        "1",
        ShiftOperation::Subtract,
    )
    .unwrap_err();
    assert!(err.to_string().contains("not supported"), "got: {}", err);
    // Must not have touched the file
    assert!(diff_indices(&src, dst.path()).is_empty());
}

#[test]
fn shift_metadata_dates_missing_tag_errors() {
    // sample_with_exif.jpg has no DateTimeOriginal
    let src = fixture("sample_with_exif.jpg");
    let dst = temp_copy(&src, "missing_err.jpg");

    let err = oxidex::core::date_shift::shift_metadata_dates(
        dst.path(),
        "DateTimeOriginal",
        "1",
        ShiftOperation::Subtract,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("No date/time tags matching"),
        "got: {}",
        err
    );
}

// ============================================================================
// CLI-level tests: run the oxidex binary with the exact commands from issue #14
// ============================================================================

/// Runs the oxidex binary with one shift argument against a file.
fn run_oxidex(shift_arg: &str, file: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg(shift_arg)
        .arg(file)
        .output()
        .unwrap()
}

fn read_dto(path: &Path) -> String {
    read_metadata(path)
        .unwrap()
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap()
        .to_rfc3339()
}

#[test]
fn cli_issue_14_short_form() {
    // The first command from the issue report, verbatim
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "cli_short.jpg");

    let output = run_oxidex("-DateTimeOriginal-=1:00:00", dst.path());
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read_dto(dst.path()), "2024-02-01T13:30:00+00:00");
}

#[test]
fn cli_issue_14_long_form() {
    // The second command from the issue report, verbatim
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "cli_long.jpg");

    let output = run_oxidex("-DateTimeOriginal-=0:0:0 1:00:00", dst.path());
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read_dto(dst.path()), "2024-02-01T13:30:00+00:00");
}

#[test]
fn cli_exif_prefixed_add() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "cli_prefixed.jpg");

    let output = run_oxidex("-EXIF:DateTimeOriginal+=1:30", dst.path());
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read_dto(dst.path()), "2024-02-01T16:00:00+00:00");
}

#[test]
fn cli_modify_date_on_sample_fixture() {
    // sample_with_exif.jpg: IFD0:ModifyDate = 2025-01-15T10:30:00
    let src = fixture("sample_with_exif.jpg");
    let dst = temp_copy(&src, "cli_modifydate.jpg");

    let output = run_oxidex("-ModifyDate-=1", dst.path());
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dt = read_metadata(dst.path())
        .unwrap()
        .get("IFD0:ModifyDate")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2025-01-15T09:30:00+00:00");
}

#[test]
fn cli_failure_exits_nonzero_with_clear_message() {
    let src = fixture("sample_with_exif.jpg"); // no DateTimeOriginal
    let dst = temp_copy(&src, "cli_fail.jpg");

    let output = run_oxidex("-DateTimeOriginal-=1:00:00", dst.path());
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No date/time tags matching"),
        "stderr: {}",
        stderr
    );
}
