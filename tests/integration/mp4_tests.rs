//! Integration tests for QuickTime/MP4 metadata parser
//!
//! These tests verify the QuickTime/MP4 parser's ability to extract metadata from
//! real MP4/MOV files, including iTunes-style metadata, classic QuickTime user data,
//! and MP4 keys/ilst metadata.

use exiftool_rs::io::buffered_reader::BufferedReader;
use exiftool_rs::parsers::quicktime::parse_quicktime_metadata;
use std::path::PathBuf;

/// Helper function to get path to test fixture
fn get_fixture_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path.push("mp4");
    path.push(filename);
    path
}

#[test]
fn test_parse_sample_mp4_metadata() {
    let mp4_path = get_fixture_path("sample.mp4");

    // Verify test file exists
    assert!(
        mp4_path.exists(),
        "Test fixture not found: {}. Run 'python3 tests/fixtures/mp4/generate_sample.py' to create it.",
        mp4_path.display()
    );

    // Parse MP4 metadata
    let reader = BufferedReader::new(&mp4_path).expect("Failed to open MP4 file");
    let result = parse_quicktime_metadata(&reader);

    assert!(
        result.is_ok(),
        "Failed to parse MP4 metadata: {:?}",
        result.err()
    );

    let metadata = result.unwrap();

    println!(
        "Extracted {} metadata fields from sample.mp4:",
        metadata.len()
    );
    for (key, value) in metadata.iter() {
        println!("  {}: {:?}", key, value);
    }

    // Verify at least 5 metadata tags extracted (acceptance criteria)
    assert!(
        metadata.len() >= 5,
        "Expected at least 5 metadata tags, got {}",
        metadata.len()
    );

    // Verify specific iTunes metadata fields
    // The sample.mp4 file should contain these tags
    assert!(
        metadata.contains_key("ItemList:Title"),
        "ItemList:Title not found in metadata"
    );
    assert!(
        metadata.contains_key("ItemList:Artist"),
        "ItemList:Artist not found in metadata"
    );
    assert!(
        metadata.contains_key("ItemList:Album"),
        "ItemList:Album not found in metadata"
    );
    assert!(
        metadata.contains_key("ItemList:Year"),
        "ItemList:Year not found in metadata"
    );
    assert!(
        metadata.contains_key("ItemList:Comment"),
        "ItemList:Comment not found in metadata"
    );

    // Verify expected values
    assert_eq!(
        metadata.get_string("ItemList:Title"),
        Some("Sample Video Title"),
        "ItemList:Title value incorrect"
    );
    assert_eq!(
        metadata.get_string("ItemList:Artist"),
        Some("Sample Artist"),
        "ItemList:Artist value incorrect"
    );
    assert_eq!(
        metadata.get_string("ItemList:Album"),
        Some("Sample Album"),
        "ItemList:Album value incorrect"
    );
    assert_eq!(
        metadata.get_string("ItemList:Year"),
        Some("2024"),
        "ItemList:Year value incorrect"
    );
}

#[test]
fn test_parse_mp4_with_quicktime_user_data() {
    let mp4_path = get_fixture_path("sample.mp4");

    let reader = BufferedReader::new(&mp4_path).expect("Failed to open MP4 file");
    let metadata = parse_quicktime_metadata(&reader).expect("Failed to parse MP4");

    // The sample file also has classic QuickTime user data
    assert!(
        metadata.contains_key("QuickTime:Title"),
        "QuickTime:Title not found in metadata"
    );

    assert_eq!(
        metadata.get_string("QuickTime:Title"),
        Some("QT Title!!"),
        "QuickTime:Title value incorrect"
    );

    println!("QuickTime user data extracted successfully:");
    for (key, value) in metadata.iter() {
        if key.starts_with("QuickTime:") {
            println!("  {}: {:?}", key, value);
        }
    }
}

#[test]
fn test_parse_mp4_extracts_multiple_tags() {
    let mp4_path = get_fixture_path("sample.mp4");

    let reader = BufferedReader::new(&mp4_path).expect("Failed to open MP4 file");
    let metadata = parse_quicktime_metadata(&reader).expect("Failed to parse MP4");

    // List of tags we expect to find
    let expected_tags = [
        "ItemList:Title",
        "ItemList:Artist",
        "ItemList:Album",
        "ItemList:Year",
        "ItemList:Comment",
        "QuickTime:Title",
    ];

    let mut found_count = 0;
    for tag in &expected_tags {
        if metadata.contains_key(tag) {
            found_count += 1;
            println!("Found {}: {:?}", tag, metadata.get(tag));
        }
    }

    // Should find at least 5 of these tags
    assert!(
        found_count >= 5,
        "Expected at least 5 metadata tags, found {}",
        found_count
    );
}

#[test]
fn test_parse_mp4_copyright_tag() {
    let mp4_path = get_fixture_path("sample.mp4");

    let reader = BufferedReader::new(&mp4_path).expect("Failed to open MP4 file");
    let metadata = parse_quicktime_metadata(&reader).expect("Failed to parse MP4");

    // Verify copyright field
    assert!(
        metadata.contains_key("ItemList:Copyright"),
        "ItemList:Copyright not found"
    );

    assert_eq!(
        metadata.get_string("ItemList:Copyright"),
        Some("Copyright 2024"),
        "ItemList:Copyright value incorrect"
    );
}

#[test]
fn test_parse_mp4_genre_tag() {
    let mp4_path = get_fixture_path("sample.mp4");

    let reader = BufferedReader::new(&mp4_path).expect("Failed to open MP4 file");
    let metadata = parse_quicktime_metadata(&reader).expect("Failed to parse MP4");

    // Verify genre field
    assert!(
        metadata.contains_key("ItemList:Genre"),
        "ItemList:Genre not found"
    );

    assert_eq!(
        metadata.get_string("ItemList:Genre"),
        Some("Test Genre"),
        "ItemList:Genre value incorrect"
    );
}

#[test]
fn test_parse_invalid_mp4() {
    // Create invalid file (no valid signature)
    let invalid_content = b"This is not an MP4 file";

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_invalid.mp4");
    std::fs::write(&temp_path, invalid_content).expect("Failed to write temp file");

    let reader = BufferedReader::new(&temp_path).expect("Failed to open temp file");
    let result = parse_quicktime_metadata(&reader);

    let _ = std::fs::remove_file(&temp_path);

    assert!(result.is_err(), "Should fail on invalid MP4");
    // The error could be from validation or from reading the file
    // Either is acceptable for an invalid file
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Invalid")
            || error_msg.contains("Failed to read")
            || error_msg.contains("No moov atom"),
        "Expected error for invalid file, got: {}",
        error_msg
    );
}

#[test]
fn test_parse_mp4_atom_hierarchy() {
    // This test verifies that the parser correctly navigates the atom hierarchy
    // moov → udta → meta → ilst
    let mp4_path = get_fixture_path("sample.mp4");

    let reader = BufferedReader::new(&mp4_path).expect("Failed to open MP4 file");
    let metadata = parse_quicktime_metadata(&reader).expect("Failed to parse MP4");

    // If we successfully extracted iTunes metadata, it means we navigated:
    // moov → udta → meta → ilst → ©nam → data
    assert!(
        metadata.contains_key("ItemList:Title"),
        "Failed to navigate atom hierarchy to extract iTunes metadata"
    );

    // If we successfully extracted QuickTime user data, it means we navigated:
    // moov → udta → ©nam
    assert!(
        metadata.contains_key("QuickTime:Title"),
        "Failed to navigate atom hierarchy to extract QuickTime user data"
    );
}

#[test]
fn test_mp4_metadata_field_count() {
    // Test that we meet the acceptance criteria of extracting at least 5 fields
    let mp4_path = get_fixture_path("sample.mp4");

    let reader = BufferedReader::new(&mp4_path).expect("Failed to open MP4 file");
    let metadata = parse_quicktime_metadata(&reader).expect("Failed to parse MP4");

    // Count extracted fields
    let field_count = metadata.len();

    println!("\nExtracted MP4 metadata fields:");
    for (key, value) in metadata.iter() {
        println!("  {}: {:?}", key, value);
    }

    // Acceptance criteria: at least 5 metadata tags
    assert!(
        field_count >= 5,
        "Expected at least 5 metadata tags, but got {}",
        field_count
    );
}

#[test]
fn test_mp4_both_itunes_and_quicktime_metadata() {
    // Test that the parser extracts both iTunes-style and classic QuickTime metadata
    let mp4_path = get_fixture_path("sample.mp4");

    let reader = BufferedReader::new(&mp4_path).expect("Failed to open MP4 file");
    let metadata = parse_quicktime_metadata(&reader).expect("Failed to parse MP4");

    // Count iTunes tags
    let itunes_count = metadata
        .iter()
        .filter(|(k, _)| k.starts_with("ItemList:"))
        .count();

    // Count QuickTime tags
    let qt_count = metadata
        .iter()
        .filter(|(k, _)| k.starts_with("QuickTime:"))
        .count();

    println!(
        "iTunes tags: {}, QuickTime tags: {}",
        itunes_count, qt_count
    );

    // Should have both types
    assert!(itunes_count > 0, "No iTunes metadata extracted");
    assert!(qt_count > 0, "No QuickTime metadata extracted");
}
