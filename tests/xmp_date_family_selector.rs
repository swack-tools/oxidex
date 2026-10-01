//! A family-0 XMP shift must not silently miss a selected family-1 property.

use oxidex::core::date_shift::{ShiftOperation, shift_metadata_dates};
use oxidex::core::operations::read_metadata;

const PNG: &[u8] = include_bytes!("fixtures/png/xmp_date_family_selector.png");

#[test]
fn qualified_family_zero_xmp_date_is_refused_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dates.png");
    std::fs::write(&file, PNG).unwrap();
    let map = read_metadata(&file).unwrap();
    assert_eq!(
        map.get_string("XMP-exif:DateTimeOriginal"),
        Some("2024:01:02 03:04:05")
    );
    assert_eq!(
        map.get_string("XMP:CreateDate"),
        Some("2024:01:02 03:04:05"),
        "ordinary xmp:CreateDate uses the legacy map key, not its public -G1 label"
    );

    let error = shift_metadata_dates(
        &file,
        "XMP:DateTimeOriginal",
        "0:0:1 0:0:0",
        ShiftOperation::Add,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("XMP-exif:DateTimeOriginal"), "{message}");
    assert!(message.contains("cannot write"), "{message}");
    assert_eq!(std::fs::read(&file).unwrap(), PNG);
}

#[test]
fn family_one_and_common_xmp_dates_keep_their_existing_refusals() {
    for (pattern, expected_key) in [
        ("XMP-exif:DateTimeOriginal", "XMP-exif:DateTimeOriginal"),
        ("XMP:CreateDate", "XMP:CreateDate"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dates.png");
        std::fs::write(&file, PNG).unwrap();
        let error =
            shift_metadata_dates(&file, pattern, "0:0:1 0:0:0", ShiftOperation::Add).unwrap_err();
        assert!(
            error.to_string().contains(expected_key),
            "{pattern}: {error}"
        );
        assert_eq!(std::fs::read(&file).unwrap(), PNG, "{pattern}");
    }
}
