//! Integration tests for file renaming based on metadata
//!
//! This test validates the rename feature including:
//! - Filename pattern parsing
//! - Variable substitution with metadata tags
//! - Date formatting with -d flag
//! - Dry-run mode (-n flag)
//! - Collision detection

use exiftool_rs::cli::rename::{build_new_filename, rename_file};
use exiftool_rs::core::operations::read_metadata;
use std::fs;
use tempfile::TempDir;

/// Creates a minimal but valid JPEG file with EXIF metadata containing
/// Make, Model, and DateTimeOriginal tags for testing rename operations.
fn create_test_jpeg_with_metadata() -> Vec<u8> {
    let mut jpeg = Vec::new();

    // === JPEG SOI marker ===
    jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // === APP1 segment with EXIF ===
    let mut app1_payload = Vec::new();

    // EXIF identifier: "Exif\0\0"
    app1_payload.extend_from_slice(b"Exif\0\0");

    // === TIFF header (little-endian) ===
    // Byte order: "II" (little-endian)
    app1_payload.extend_from_slice(b"II");

    // Magic number: 0x002A (little-endian)
    app1_payload.extend_from_slice(&[0x2A, 0x00]);

    // IFD offset: 8 bytes from TIFF header start (4-byte value, little-endian)
    app1_payload.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]);

    // === IFD (Image File Directory) ===
    // Number of entries: 3 tags (Make, Model, DateTimeOriginal)
    app1_payload.extend_from_slice(&[0x03, 0x00]);

    // Prepare tag data that will be stored after the IFD
    let make_value = b"Canon\0"; // 6 bytes
    let model_value = b"EOS 5D\0"; // 7 bytes
    let datetime_value = b"2025:01:15 10:30:00\0"; // 20 bytes

    // Calculate offsets for out-of-line data
    // IFD structure: 2 (count) + 3*12 (entries) + 4 (next IFD offset) = 42 bytes
    let data_section_offset = 8 + 42; // Relative to TIFF header start

    let make_offset = data_section_offset;
    let model_offset = make_offset + make_value.len() as u32;
    let datetime_offset = model_offset + model_value.len() as u32;

    // === IFD Entry 1: Make (0x010F) ===
    app1_payload.extend_from_slice(&[0x0F, 0x01]); // Tag ID: Make
    app1_payload.extend_from_slice(&[0x02, 0x00]); // Type: ASCII (2)
    app1_payload.extend_from_slice(&(make_value.len() as u32).to_le_bytes()); // Count
    app1_payload.extend_from_slice(&make_offset.to_le_bytes()); // Offset

    // === IFD Entry 2: Model (0x0110) ===
    app1_payload.extend_from_slice(&[0x10, 0x01]); // Tag ID: Model
    app1_payload.extend_from_slice(&[0x02, 0x00]); // Type: ASCII (2)
    app1_payload.extend_from_slice(&(model_value.len() as u32).to_le_bytes()); // Count
    app1_payload.extend_from_slice(&model_offset.to_le_bytes()); // Offset

    // === IFD Entry 3: DateTimeOriginal (0x9003) ===
    app1_payload.extend_from_slice(&[0x03, 0x90]); // Tag ID: DateTimeOriginal
    app1_payload.extend_from_slice(&[0x02, 0x00]); // Type: ASCII (2)
    app1_payload.extend_from_slice(&(datetime_value.len() as u32).to_le_bytes()); // Count
    app1_payload.extend_from_slice(&datetime_offset.to_le_bytes()); // Offset

    // Next IFD offset (0 = no more IFDs)
    app1_payload.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // === Out-of-line data section ===
    app1_payload.extend_from_slice(make_value);
    app1_payload.extend_from_slice(model_value);
    app1_payload.extend_from_slice(datetime_value);

    // === Write APP1 segment ===
    jpeg.extend_from_slice(&[0xFF, 0xE1]); // APP1 marker
    let app1_length = (app1_payload.len() + 2) as u16; // +2 for length field itself
    jpeg.extend_from_slice(&app1_length.to_be_bytes());
    jpeg.extend_from_slice(&app1_payload);

    // === Minimal SOS segment (required for valid JPEG) ===
    // SOS marker
    jpeg.extend_from_slice(&[0xFF, 0xDA]);
    // Length (minimal: 2 + 6 = 8 bytes)
    jpeg.extend_from_slice(&[0x00, 0x08]);
    // Number of components (1)
    jpeg.extend_from_slice(&[0x01]);
    // Component 1: ID=1, DC/AC table=0/0
    jpeg.extend_from_slice(&[0x01, 0x00]);
    // Spectral selection: start=0, end=63
    jpeg.extend_from_slice(&[0x00, 0x3F]);
    // Successive approximation: 0
    jpeg.extend_from_slice(&[0x00]);

    // === EOI marker ===
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    jpeg
}

#[test]
fn test_build_new_filename_simple_tag() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Read metadata
    let metadata = read_metadata(&test_file).unwrap();

    // Build new filename with simple tag
    let new_name = build_new_filename("DateTimeOriginal", &metadata, &test_file, None).unwrap();

    // Should contain the datetime value (sanitized)
    assert!(new_name.contains("2025"));
    assert!(new_name.contains("01"));
    assert!(new_name.contains("15"));
}

#[test]
fn test_build_new_filename_with_date_format() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Read metadata
    let metadata = read_metadata(&test_file).unwrap();

    // Build new filename with date formatting
    let new_name = build_new_filename(
        "DateTimeOriginal",
        &metadata,
        &test_file,
        Some("%Y%m%d_%H%M%S"),
    )
    .unwrap();

    // Should be formatted as YYYYMMDD_HHMMSS
    assert_eq!(new_name, "20250115_103000");
}

#[test]
fn test_build_new_filename_with_extension() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Read metadata
    let metadata = read_metadata(&test_file).unwrap();

    // Build new filename with extension placeholder
    let new_name = build_new_filename(
        "${EXIF:DateTimeOriginal}%%e",
        &metadata,
        &test_file,
        Some("%Y%m%d_%H%M%S"),
    )
    .unwrap();

    // Should include the extension
    assert_eq!(new_name, "20250115_103000.jpg");
}

#[test]
fn test_build_new_filename_multiple_tags() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Read metadata
    let metadata = read_metadata(&test_file).unwrap();

    // Build new filename with multiple tags
    let new_name =
        build_new_filename("${EXIF:Make}_${EXIF:Model}", &metadata, &test_file, None).unwrap();

    // Should contain both Make and Model
    assert_eq!(new_name, "Canon_EOS 5D");
}

#[test]
fn test_rename_file_dry_run() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("original.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Perform dry-run rename
    let result = rename_file(
        &test_file,
        "${EXIF:DateTimeOriginal}%%e",
        Some("%Y%m%d_%H%M%S"),
        true, // dry_run = true
    );

    assert!(result.is_ok());

    // Original file should still exist
    assert!(test_file.exists());

    // New file should NOT exist (dry-run didn't execute)
    let expected_new_path = temp_dir.path().join("20250115_103000.jpg");
    assert!(!expected_new_path.exists());
}

#[test]
fn test_rename_file_actual_rename() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("original.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Perform actual rename
    let result = rename_file(
        &test_file,
        "${EXIF:DateTimeOriginal}%%e",
        Some("%Y%m%d_%H%M%S"),
        false, // dry_run = false
    );

    assert!(result.is_ok());
    let new_path = result.unwrap();

    // Original file should NOT exist
    assert!(!test_file.exists());

    // New file should exist
    assert!(new_path.exists());
    assert_eq!(new_path.file_name().unwrap(), "20250115_103000.jpg");

    // Verify content is preserved
    let new_content = fs::read(&new_path).unwrap();
    let original_content = create_test_jpeg_with_metadata();
    assert_eq!(new_content, original_content);
}

#[test]
fn test_rename_file_collision_detection() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("original.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Create a file that would collide with the rename target
    let collision_file = temp_dir.path().join("20250115_103000.jpg");
    fs::write(&collision_file, b"existing file").unwrap();

    // Attempt rename (should fail due to collision)
    let result = rename_file(
        &test_file,
        "${EXIF:DateTimeOriginal}%%e",
        Some("%Y%m%d_%H%M%S"),
        false,
    );

    // Should return an error
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("already exists"));

    // Original file should still exist
    assert!(test_file.exists());

    // Collision file should be unchanged
    let collision_content = fs::read(&collision_file).unwrap();
    assert_eq!(collision_content, b"existing file");
}

#[test]
fn test_rename_with_missing_tag() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Try to rename with a tag that doesn't exist
    let result = rename_file(
        &test_file,
        "EXIF:GPS:Latitude", // This tag doesn't exist in our test file
        None,
        false,
    );

    // Should return an error
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("not found"));
}

#[test]
fn test_rename_sanitizes_invalid_characters() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Read metadata
    let metadata = read_metadata(&test_file).unwrap();

    // DateTimeOriginal contains colons which are invalid on some filesystems
    let new_name = build_new_filename("DateTimeOriginal", &metadata, &test_file, None).unwrap();

    // Colons should be replaced with underscores
    assert!(!new_name.contains(':'));
    assert!(new_name.contains('_'));
}

#[test]
fn test_rename_preserves_file_in_same_directory() {
    // Create a test JPEG with metadata
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("original.jpg");
    fs::write(&test_file, create_test_jpeg_with_metadata()).unwrap();

    // Perform rename
    let result = rename_file(
        &test_file,
        "${EXIF:DateTimeOriginal}%%e",
        Some("%Y%m%d_%H%M%S"),
        false,
    );

    assert!(result.is_ok());
    let new_path = result.unwrap();

    // New file should be in the same directory
    assert_eq!(new_path.parent(), test_file.parent());
}
