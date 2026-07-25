//! MakerNote integration tests
//!
//! These tests verify that MakerNote data is correctly extracted
//! from real JPEG files with EXIF data.

use oxidex::core::operations::read_metadata;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn get_test_image_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("samples")
        .join(filename)
}

#[test]
#[ignore] // Ignore until we have real test images
fn test_canon_makernote_extraction() {
    let path = get_test_image_path("canon_sample.jpg");

    if !path.exists() {
        eprintln!("Skipping test: Canon sample image not found");
        return;
    }

    let metadata = read_metadata(&path).expect("Failed to parse JPEG");

    // Check for Canon MakerNote tags
    let canon_tags: Vec<_> = metadata
        .iter()
        .filter(|(k, _)| k.starts_with("Canon:"))
        .collect();

    assert!(
        !canon_tags.is_empty(),
        "Should extract Canon MakerNote tags"
    );
}

#[test]
#[ignore] // Ignore until we have real test images
fn test_nikon_makernote_extraction() {
    let path = get_test_image_path("nikon_sample.jpg");

    if !path.exists() {
        eprintln!("Skipping test: Nikon sample image not found");
        return;
    }

    let metadata = read_metadata(&path).expect("Failed to parse JPEG");

    // Check for Nikon MakerNote tags
    let nikon_tags: Vec<_> = metadata
        .iter()
        .filter(|(k, _)| k.starts_with("Nikon:"))
        .collect();

    assert!(
        !nikon_tags.is_empty(),
        "Should extract Nikon MakerNote tags"
    );
}

#[test]
fn test_jpeg_without_makernote() {
    // This test should pass even without test images
    // as it tests that the code doesn't crash on images without MakerNotes

    // This is a minimal valid JPEG with no EXIF/MakerNote
    let minimal_jpeg = vec![
        0xFF, 0xD8, // SOI
        0xFF, 0xE0, // APP0
        0x00, 0x10, // Length
        0x4A, 0x46, 0x49, 0x46, 0x00, // "JFIF\0"
        0x01, 0x01, // Version 1.1
        0x00, // Units: none
        0x00, 0x01, 0x00, 0x01, // X/Y density: 1x1
        0x00, 0x00, // Thumbnail: 0x0
        0xFF, 0xD9, // EOI
    ];

    // Write to temp file
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    temp_file
        .write_all(&minimal_jpeg)
        .expect("Failed to write temp file");
    let temp_path = temp_file.path();

    let _metadata = read_metadata(&temp_path).expect("Should parse JPEG without MakerNote");

    // Should succeed without crashing - if we get here, the test passed
}
