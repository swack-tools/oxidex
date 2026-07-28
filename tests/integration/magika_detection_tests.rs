//! Integration tests for Magika AI-powered file detection
//!
//! These tests verify that the Magika detection feature works correctly
//! when enabled via the `--features magika` flag.
//!
//! Run with: `cargo test --features magika`

#![cfg(feature = "magika")]

use oxidex::core::operations::read_metadata_with_detector;
use oxidex::parsers::DetectorMode;
use std::path::Path;

/// Helper function to test file detection with Magika
fn test_magika_detection(file_path: &str, expected_tags: &[&str]) {
    let path = Path::new(file_path);

    // Skip test if file doesn't exist (may not be in LFS yet)
    if !path.exists() {
        eprintln!("Skipping test - file not found: {}", file_path);
        return;
    }

    // Test with Magika detection
    let result = read_metadata_with_detector(path, DetectorMode::Magika);

    match result {
        Ok(metadata) => {
            assert!(
                !metadata.is_empty(),
                "Magika detection should extract metadata from {}",
                file_path
            );

            // Verify expected tags are present
            for tag in expected_tags {
                assert!(
                    metadata.contains_key(*tag),
                    "Expected tag '{}' not found in metadata from {}",
                    tag,
                    file_path
                );
            }
        }
        Err(e) => {
            panic!("Magika detection failed for {}: {}", file_path, e);
        }
    }
}

#[test]
fn test_magika_jpeg_detection() {
    test_magika_detection(
        "tests/fixtures/jpeg/sample_with_exif.jpg",
        &["File:FileName", "File:FileSize"],
    );
}

#[test]
fn test_magika_png_detection() {
    test_magika_detection(
        "tests/fixtures/png/basic.png",
        &["File:FileName", "File:FileSize"],
    );
}

#[test]
fn test_magika_tiff_detection() {
    test_magika_detection(
        "tests/fixtures/tiff/basic.tiff",
        &["File:FileName", "File:FileSize"],
    );
}

#[test]
fn test_magika_pdf_detection() {
    test_magika_detection(
        "tests/fixtures/pdf/sample.pdf",
        &["File:FileName", "File:FileSize"],
    );
}

#[test]
fn test_magika_mp4_detection() {
    test_magika_detection(
        "tests/fixtures/video/sample.mp4",
        &["File:FileName", "File:FileSize"],
    );
}

/// Test that Magika and signature detection produce equivalent results
#[test]
fn test_magika_vs_signature_equivalence() {
    let test_files = vec![
        "tests/fixtures/jpeg/sample_with_exif.jpg",
        "tests/fixtures/png/basic.png",
        "tests/fixtures/tiff/basic.tiff",
    ];

    for file_path in test_files {
        let path = Path::new(file_path);

        // Skip if file doesn't exist
        if !path.exists() {
            eprintln!("Skipping equivalence test - file not found: {}", file_path);
            continue;
        }

        // Test with both detection modes
        let signature_result = read_metadata_with_detector(path, DetectorMode::Signature);
        let magika_result = read_metadata_with_detector(path, DetectorMode::Magika);

        match (signature_result, magika_result) {
            (Ok(sig_meta), Ok(mag_meta)) => {
                // Both should extract metadata successfully
                assert!(
                    !sig_meta.is_empty(),
                    "Signature detection failed for {}",
                    file_path
                );
                assert!(
                    !mag_meta.is_empty(),
                    "Magika detection failed for {}",
                    file_path
                );

                // File system tags should be identical
                for tag in &["File:FileName", "File:FileSize"] {
                    assert_eq!(
                        sig_meta.get(*tag),
                        mag_meta.get(*tag),
                        "Tag '{}' differs between detection modes for {}",
                        tag,
                        file_path
                    );
                }
            }
            (Err(e), _) => panic!("Signature detection failed for {}: {}", file_path, e),
            (_, Err(e)) => panic!("Magika detection failed for {}: {}", file_path, e),
        }
    }
}

/// Test error handling with corrupted/invalid files
#[test]
fn test_magika_error_handling() {
    // Test with a text file that might be misidentified
    let path = Path::new("Cargo.toml");

    if path.exists() {
        // This should either succeed or fail gracefully
        let result = read_metadata_with_detector(path, DetectorMode::Magika);

        // We don't assert success or failure here - just verify it doesn't panic
        match result {
            Ok(_) => {
                // Magika successfully detected it as some format
            }
            Err(_) => {
                // Or it failed gracefully with an error
            }
        }
    }
}

/// Test that Magika detection works with various image formats
#[test]
fn test_magika_format_coverage() {
    let test_cases = vec![
        ("tests/fixtures/jpeg/sample_with_exif.jpg", "JPEG"),
        ("tests/fixtures/png/basic.png", "PNG"),
        ("tests/fixtures/tiff/basic.tiff", "TIFF"),
        ("tests/fixtures/pdf/sample.pdf", "PDF"),
    ];

    for (file_path, format_name) in test_cases {
        let path = Path::new(file_path);

        // Skip if file doesn't exist
        if !path.exists() {
            eprintln!("Skipping format test - file not found: {}", file_path);
            continue;
        }

        let result = read_metadata_with_detector(path, DetectorMode::Magika);

        assert!(
            result.is_ok(),
            "Magika should successfully detect {} format from {}",
            format_name,
            file_path
        );
    }
}
