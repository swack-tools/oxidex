//! Integration tests for date/time shifting operations

use chrono::{Datelike, Timelike, Utc};
use exiftool_rs::core::date_shift::{shift_metadata_dates, ShiftOperation};
use exiftool_rs::core::operations::{read_metadata, write_metadata};
use exiftool_rs::core::tag_value::TagValue;
use std::fs;
use std::path::PathBuf;
use tempfile::NamedTempFile;

/// Helper function to create a test file by copying an existing fixture with DateTime tags
fn create_test_file_with_metadata() -> NamedTempFile {
    // Use the existing fixture file that has DateTime tags
    let fixture_path = PathBuf::from("tests/fixtures/jpeg/sample_with_exif.jpg");

    // Create a temporary file
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // Copy the fixture to the temp file
    fs::copy(&fixture_path, temp_file.path()).expect("Failed to copy fixture");

    temp_file
}

#[test]
fn test_shift_dates_add_one_day() {
    // Create test file with DateTime tags set to "2025:01:15 10:30:00"
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Add 1 day to DateTime (the fixture has IFD0:ModifyDate)
    let result = shift_metadata_dates(path, "IFD0:ModifyDate", "0:0:1 0:0:0", ShiftOperation::Add);
    assert!(result.is_ok(), "Failed to shift dates: {:?}", result.err());

    // Read metadata and verify dates were shifted
    let metadata = read_metadata(path).expect("Failed to read metadata");

    // Check DateTime
    let value = metadata
        .get("IFD0:ModifyDate")
        .expect("IFD0:ModifyDate not found");
    let dt = value
        .as_datetime()
        .expect("IFD0:ModifyDate is not a DateTime value");

    // Should be 2025-01-16 10:30:00
    assert_eq!(dt.year(), 2025);
    assert_eq!(dt.month(), 1);
    assert_eq!(dt.day(), 16);
    assert_eq!(dt.hour(), 10);
    assert_eq!(dt.minute(), 30);
}

#[test]
fn test_shift_dates_subtract_one_month() {
    // Create test file
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Subtract 1 month from DateTime
    let result = shift_metadata_dates(
        path,
        "IFD0:ModifyDate",
        "0:1:0 0:0:0",
        ShiftOperation::Subtract,
    );
    assert!(result.is_ok(), "Failed to shift dates: {:?}", result.err());

    // Read metadata and verify dates were shifted
    let metadata = read_metadata(path).expect("Failed to read metadata");

    let value = metadata
        .get("IFD0:ModifyDate")
        .expect("IFD0:ModifyDate not found");
    let dt = value
        .as_datetime()
        .expect("IFD0:ModifyDate is not a DateTime value");

    // Should be 2024-12-15 10:30:00
    assert_eq!(dt.year(), 2024);
    assert_eq!(dt.month(), 12);
    assert_eq!(dt.day(), 15);
}

#[test]
fn test_shift_specific_tag_only() {
    // This test verifies that shifting a specific tag works
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Shift DateTime by 1 day
    let result = shift_metadata_dates(path, "IFD0:ModifyDate", "0:0:1 0:0:0", ShiftOperation::Add);
    assert!(
        result.is_ok(),
        "Failed to shift DateTime: {:?}",
        result.err()
    );

    // Read metadata again
    let metadata = read_metadata(path).expect("Failed to read metadata");

    // DateTime should be shifted
    let value = metadata
        .get("IFD0:ModifyDate")
        .expect("IFD0:ModifyDate not found");
    let dt = value
        .as_datetime()
        .expect("IFD0:ModifyDate is not a DateTime value");
    assert_eq!(dt.day(), 16);
}

#[test]
fn test_shift_dates_add_hours_and_minutes() {
    // Create test file
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Add 6 hours and 30 minutes to DateTime
    let result = shift_metadata_dates(path, "IFD0:ModifyDate", "0:0:0 6:30:0", ShiftOperation::Add);
    assert!(result.is_ok(), "Failed to shift dates: {:?}", result.err());

    // Read metadata and verify dates were shifted
    let metadata = read_metadata(path).expect("Failed to read metadata");

    let value = metadata
        .get("IFD0:ModifyDate")
        .expect("IFD0:ModifyDate not found");
    let dt = value
        .as_datetime()
        .expect("IFD0:ModifyDate is not a DateTime value");

    // Original: 10:30:00, After +6:30: 17:00:00
    assert_eq!(dt.hour(), 17);
    assert_eq!(dt.minute(), 0);
}

#[test]
fn test_shift_dates_set_absolute() {
    // Create test file
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Set DateTime to absolute value
    let result = shift_metadata_dates(
        path,
        "IFD0:ModifyDate",
        "2026:06:15 14:45:30",
        ShiftOperation::Set,
    );
    assert!(result.is_ok(), "Failed to set DateTime: {:?}", result.err());

    // Read metadata and verify date was set
    let metadata = read_metadata(path).expect("Failed to read metadata");

    let value = metadata
        .get("IFD0:ModifyDate")
        .expect("IFD0:ModifyDate not found");
    let dt = value
        .as_datetime()
        .expect("IFD0:ModifyDate is not a DateTime value");

    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 6);
    assert_eq!(dt.day(), 15);
    assert_eq!(dt.hour(), 14);
    assert_eq!(dt.minute(), 45);
    assert_eq!(dt.second(), 30);
}

#[test]
fn test_shift_dates_complex_offset() {
    // Create test file
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Add 1 year, 2 months, 3 days, 4 hours, 5 minutes, 6 seconds
    let result = shift_metadata_dates(path, "IFD0:ModifyDate", "1:2:3 4:5:6", ShiftOperation::Add);
    assert!(result.is_ok(), "Failed to shift dates: {:?}", result.err());

    // Read metadata and verify dates were shifted
    let metadata = read_metadata(path).expect("Failed to read metadata");

    let value = metadata
        .get("IFD0:ModifyDate")
        .expect("IFD0:ModifyDate not found");
    let dt = value
        .as_datetime()
        .expect("IFD0:ModifyDate is not a DateTime value");

    // From 2025-01-15 10:30:00
    // Add 1 year 2 months = 2026-03-15
    // Add 3 days = 2026-03-18
    // Add 4:05:06 = 14:35:06
    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 3);
    assert_eq!(dt.day(), 18);
    assert_eq!(dt.hour(), 14);
    assert_eq!(dt.minute(), 35);
    assert_eq!(dt.second(), 6);
}

#[test]
fn test_shift_dates_invalid_offset_format() {
    // Create test file
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Try with invalid offset format
    let result = shift_metadata_dates(path, "IFD0:ModifyDate", "invalid", ShiftOperation::Add);
    assert!(result.is_err(), "Should fail with invalid offset format");
}

#[test]
fn test_shift_dates_nonexistent_tag() {
    // Create test file
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Try to shift a tag that doesn't exist
    let result = shift_metadata_dates(
        path,
        "EXIF:NonExistentDate",
        "0:0:1 0:0:0",
        ShiftOperation::Add,
    );
    assert!(result.is_err(), "Should fail when tag doesn't exist");
}

#[test]
fn test_shift_dates_preserves_other_tags() {
    // Create a test file and add some metadata
    let temp_file = create_test_file_with_metadata();
    let path = temp_file.path();

    // Add a non-DateTime tag to the metadata
    let mut metadata = read_metadata(path).expect("Failed to read metadata");
    metadata.insert(
        "IFD0:Artist".to_string(),
        TagValue::new_string("Test Artist"),
    );
    write_metadata(path, &metadata).expect("Failed to write metadata");

    // Shift dates
    let result = shift_metadata_dates(path, "IFD0:ModifyDate", "0:0:1 0:0:0", ShiftOperation::Add);
    assert!(result.is_ok());

    // Read metadata and verify non-DateTime tag is preserved
    let metadata = read_metadata(path).expect("Failed to read metadata");
    assert_eq!(
        metadata.get("IFD0:Artist").and_then(|v| v.as_string()),
        Some("Test Artist")
    );
}

#[test]
fn test_parse_offset_and_apply() {
    // Test the core date shift logic with a programmatically created DateTime
    use chrono::TimeZone;
    use exiftool_rs::core::date_shift::{apply_shift, parse_offset};

    // Create a known DateTime
    let dt = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();

    // Parse and apply offset: +1 day
    let offset = parse_offset("0:0:1 0:0:0").expect("Failed to parse offset");
    let shifted = apply_shift(dt, &offset, ShiftOperation::Add).expect("Failed to apply shift");

    assert_eq!(shifted.day(), 16);
}
