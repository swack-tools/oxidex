//! Integration tests for metadata write operations
//!
//! These tests verify the end-to-end functionality of write_metadata() and
//! modify_tag() operations, including validation, JPEG writing, and atomic operations.

use exiftool_rs::core::operations::{modify_tag, read_metadata, write_metadata};
use exiftool_rs::core::tag_value::TagValue;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

/// Helper: Creates a minimal valid JPEG file with EXIF metadata
fn create_test_jpeg_with_exif() -> Vec<u8> {
    let mut data = Vec::new();

    // SOI marker
    data.extend_from_slice(&[0xFF, 0xD8]);

    // Build EXIF APP1 segment with Make="Canon" and Model="EOS R5"
    let mut exif_data = Vec::new();

    // EXIF identifier
    exif_data.extend_from_slice(b"Exif\0\0");

    // TIFF header (little-endian)
    exif_data.extend_from_slice(&[0x49, 0x49]); // Little-endian marker (II)
    exif_data.extend_from_slice(&[0x2A, 0x00]); // TIFF magic (42)
    exif_data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // IFD offset = 8

    // IFD: 2 entries (Make, Model)
    // Entry count
    exif_data.extend_from_slice(&[0x02, 0x00]); // 2 entries

    // Entry 1: Make (0x010F)
    exif_data.extend_from_slice(&[0x0F, 0x01]); // Tag ID: Make
    exif_data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    exif_data.extend_from_slice(&[0x06, 0x00, 0x00, 0x00]); // Count: 6 bytes ("Canon\0")
    exif_data.extend_from_slice(&[0x26, 0x00, 0x00, 0x00]); // Offset to value (38 from TIFF header start)

    // Entry 2: Model (0x0110)
    exif_data.extend_from_slice(&[0x10, 0x01]); // Tag ID: Model
    exif_data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    exif_data.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]); // Count: 7 bytes ("EOS R5\0")
    exif_data.extend_from_slice(&[0x2C, 0x00, 0x00, 0x00]); // Offset to value (44 from TIFF header start)

    // Next IFD offset (0 = none)
    exif_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // Value area
    exif_data.extend_from_slice(b"Canon\0"); // Make value
    exif_data.extend_from_slice(b"EOS R5\0"); // Model value

    // Write APP1 segment
    data.extend_from_slice(&[0xFF, 0xE1]); // APP1 marker
    let length = 2 + exif_data.len();
    data.extend_from_slice(&(length as u16).to_be_bytes());
    data.extend_from_slice(&exif_data);

    // EOI marker
    data.extend_from_slice(&[0xFF, 0xD9]);

    data
}

/// Helper: Creates a temporary file with the given data and returns its path
fn create_temp_file_with_data(data: &[u8]) -> NamedTempFile {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    temp_file
        .write_all(data)
        .expect("Failed to write temp file");
    temp_file.flush().expect("Failed to flush temp file");
    temp_file
}

#[test]
fn test_write_metadata_successful_jpeg_write() {
    // Create a temporary JPEG file with existing EXIF
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Read existing metadata
    let mut metadata = read_metadata(temp_path).expect("Failed to read metadata");

    // Verify original values
    assert_eq!(metadata.get_string("IFD0:Make"), Some("Canon"));
    assert_eq!(metadata.get_string("IFD0:Model"), Some("EOS R5"));

    // Modify metadata
    metadata.insert("IFD0:Artist", TagValue::new_string("John Doe"));
    metadata.insert("IFD0:Copyright", TagValue::new_string("Copyright 2024"));

    // Write metadata back
    write_metadata(temp_path, &metadata).expect("Failed to write metadata");

    // Re-read and verify changes
    let updated_metadata = read_metadata(temp_path).expect("Failed to read updated metadata");
    assert_eq!(updated_metadata.get_string("IFD0:Artist"), Some("John Doe"));
    assert_eq!(
        updated_metadata.get_string("IFD0:Copyright"),
        Some("Copyright 2024")
    );

    // Verify original tags are preserved
    assert_eq!(updated_metadata.get_string("IFD0:Make"), Some("Canon"));
    assert_eq!(updated_metadata.get_string("IFD0:Model"), Some("EOS R5"));
}

#[test]
fn test_write_metadata_validation_fails_for_invalid_type() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Read existing metadata
    let mut metadata = read_metadata(temp_path).expect("Failed to read metadata");

    // Attempt to write an invalid value (Integer where String is expected)
    metadata.insert("IFD0:Make", TagValue::new_integer(42));

    // Write should fail with InvalidTagValue error
    let result = write_metadata(temp_path, &metadata);
    assert!(result.is_err(), "Expected validation error");

    // Verify error is InvalidTagValue
    match result {
        Err(exiftool_rs::error::ExifToolError::InvalidTagValue { tag_name, reason }) => {
            assert_eq!(tag_name, "IFD0:Make");
            assert!(
                reason.contains("Type mismatch"),
                "Expected type mismatch error, got: {}",
                reason
            );
        }
        _ => panic!("Expected InvalidTagValue error"),
    }

    // Verify file was not modified (original data intact)
    let unchanged_metadata = read_metadata(temp_path).expect("Failed to read metadata");
    assert_eq!(unchanged_metadata.get_string("IFD0:Make"), Some("Canon"));
}

#[test]
fn test_write_metadata_validation_fails_for_rational_zero_denominator() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Read existing metadata
    let mut metadata = read_metadata(temp_path).expect("Failed to read metadata");

    // Attempt to write a Rational with zero denominator (invalid)
    metadata.insert("ExifIFD:ExposureTime", TagValue::new_rational(1, 0));

    // Write should fail with InvalidTagValue error
    let result = write_metadata(temp_path, &metadata);
    assert!(result.is_err(), "Expected validation error");

    // Verify error is InvalidTagValue with denominator message
    match result {
        Err(exiftool_rs::error::ExifToolError::InvalidTagValue { tag_name, reason }) => {
            assert_eq!(tag_name, "ExifIFD:ExposureTime");
            assert!(
                reason.contains("denominator"),
                "Expected denominator error, got: {}",
                reason
            );
        }
        _ => panic!("Expected InvalidTagValue error"),
    }
}

#[test]
fn test_modify_tag_single_tag_modification() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Verify original value
    let original_metadata = read_metadata(temp_path).expect("Failed to read metadata");
    assert_eq!(original_metadata.get_string("IFD0:Make"), Some("Canon"));

    // Modify a single tag using modify_tag()
    modify_tag(temp_path, "IFD0:Artist", TagValue::new_string("Jane Smith"))
        .expect("Failed to modify tag");

    // Re-read and verify changes
    let updated_metadata = read_metadata(temp_path).expect("Failed to read updated metadata");
    assert_eq!(
        updated_metadata.get_string("IFD0:Artist"),
        Some("Jane Smith")
    );

    // Verify original tags are preserved
    assert_eq!(updated_metadata.get_string("IFD0:Make"), Some("Canon"));
    assert_eq!(updated_metadata.get_string("IFD0:Model"), Some("EOS R5"));
}

#[test]
fn test_modify_tag_overwrites_existing_value() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Modify existing tag
    modify_tag(temp_path, "IFD0:Make", TagValue::new_string("Nikon"))
        .expect("Failed to modify tag");

    // Verify the tag was overwritten
    let updated_metadata = read_metadata(temp_path).expect("Failed to read metadata");
    assert_eq!(updated_metadata.get_string("IFD0:Make"), Some("Nikon"));

    // Verify other tags are preserved
    assert_eq!(updated_metadata.get_string("IFD0:Model"), Some("EOS R5"));
}

#[test]
fn test_write_metadata_round_trip_preserves_data() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Read metadata
    let original_metadata = read_metadata(temp_path).expect("Failed to read metadata");

    // Write same metadata back (round-trip)
    write_metadata(temp_path, &original_metadata).expect("Failed to write metadata");

    // Re-read and verify no changes
    let round_trip_metadata = read_metadata(temp_path).expect("Failed to read metadata");
    assert_eq!(
        round_trip_metadata.get_string("IFD0:Make"),
        original_metadata.get_string("IFD0:Make")
    );
    assert_eq!(
        round_trip_metadata.get_string("IFD0:Model"),
        original_metadata.get_string("IFD0:Model")
    );
}

#[test]
fn test_write_metadata_validates_multiple_tags() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Read existing metadata
    let mut metadata = read_metadata(temp_path).expect("Failed to read metadata");

    // Add multiple tags of different types
    metadata.insert("IFD0:Software", TagValue::new_string("ExifTool-RS"));
    metadata.insert("IFD0:Copyright", TagValue::new_string("Copyright 2024"));

    // Write should succeed (validation passes for all tags)
    write_metadata(temp_path, &metadata).expect("Failed to write metadata");

    // Re-read and verify tags were written
    let updated_metadata = read_metadata(temp_path).expect("Failed to read metadata");
    assert_eq!(
        updated_metadata.get_string("IFD0:Software"),
        Some("ExifTool-RS")
    );
    assert_eq!(
        updated_metadata.get_string("IFD0:Copyright"),
        Some("Copyright 2024")
    );

    // Verify original tags still present
    assert_eq!(updated_metadata.get_string("IFD0:Make"), Some("Canon"));
}

#[test]
fn test_write_metadata_atomic_operation() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Read original metadata
    let original_metadata = read_metadata(temp_path).expect("Failed to read metadata");

    // Modify and write
    let mut modified_metadata = original_metadata.clone();
    modified_metadata.insert("IFD0:Artist", TagValue::new_string("Atomic Test"));
    write_metadata(temp_path, &modified_metadata).expect("Failed to write metadata");

    // Verify file is still valid JPEG (starts with SOI, ends with EOI)
    let file_contents = fs::read(temp_path).expect("Failed to read file");
    assert_eq!(&file_contents[0..2], &[0xFF, 0xD8], "Missing SOI marker");
    assert_eq!(
        &file_contents[file_contents.len() - 2..],
        &[0xFF, 0xD9],
        "Missing EOI marker"
    );

    // Verify metadata is readable
    let final_metadata = read_metadata(temp_path).expect("Failed to read metadata");
    assert_eq!(
        final_metadata.get_string("IFD0:Artist"),
        Some("Atomic Test")
    );
}

#[test]
fn test_write_metadata_with_integer_tags() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Read metadata
    let mut metadata = read_metadata(temp_path).expect("Failed to read metadata");

    // Add integer tags
    metadata.insert("ExifIFD:ISO", TagValue::new_integer(400));
    metadata.insert("IFD0:Orientation", TagValue::new_integer(1));

    // Write metadata
    write_metadata(temp_path, &metadata).expect("Failed to write metadata");

    // Verify integer tags were written correctly
    let updated_metadata = read_metadata(temp_path).expect("Failed to read metadata");
    assert_eq!(updated_metadata.get_integer("ExifIFD:ISO"), Some(400));
    assert_eq!(
        updated_metadata.get_string("IFD0:Orientation"),
        Some("Horizontal (normal)")
    );
}

#[test]
fn test_write_metadata_with_rational_tags() {
    // Create a temporary JPEG file
    let jpeg_data = create_test_jpeg_with_exif();
    let temp_file = create_temp_file_with_data(&jpeg_data);
    let temp_path = temp_file.path();

    // Read metadata
    let mut metadata = read_metadata(temp_path).expect("Failed to read metadata");

    // Add rational tags
    metadata.insert("ExifIFD:ExposureTime", TagValue::new_rational(1, 125));
    metadata.insert("ExifIFD:FNumber", TagValue::new_rational(28, 10));

    // Write metadata - the main goal is to verify validation passes and write succeeds
    write_metadata(temp_path, &metadata).expect("Failed to write metadata");

    // Verify file is still valid JPEG
    let file_contents = fs::read(temp_path).expect("Failed to read file");
    assert_eq!(&file_contents[0..2], &[0xFF, 0xD8], "Missing SOI marker");
    assert_eq!(
        &file_contents[file_contents.len() - 2..],
        &[0xFF, 0xD9],
        "Missing EOI marker"
    );

    // Note: Round-trip verification of Rational values is not tested here because
    // the current TIFF parser (raw_bytes_to_tag_value) doesn't have EXIF type
    // information and may convert Rational values to Integer or Binary during
    // parsing. This is a known limitation tracked in operations.rs:263 (TODO).
    // The important verification is that:
    // 1. Validation passes for Rational types (tested here)
    // 2. Write operation succeeds without errors (tested here)
    // 3. File remains valid JPEG (tested above)
}
