//! Integration tests for metadata copy operations
//!
//! These tests verify the end-to-end functionality of copying metadata
//! from one file to another using the copy_metadata operation.

use exiftool_rs::core::operations::{copy_metadata, read_metadata, write_metadata};
use exiftool_rs::core::tag_value::TagValue;
use std::io::Write;
use tempfile::NamedTempFile;

/// Helper: Creates a complete valid JPEG with EXIF metadata
#[allow(unused_assignments)]
fn create_test_jpeg_with_metadata(make: &str, model: &str, artist: Option<&str>) -> Vec<u8> {
    let mut data = Vec::new();

    // SOI marker
    data.extend_from_slice(&[0xFF, 0xD8]);

    // Build EXIF APP1 segment
    let mut exif_data = Vec::new();

    // EXIF identifier
    exif_data.extend_from_slice(b"Exif\0\0");

    // TIFF header (little-endian)
    exif_data.extend_from_slice(&[0x49, 0x49]); // Little-endian marker
    exif_data.extend_from_slice(&[0x2A, 0x00]); // TIFF magic (42)
    exif_data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // IFD offset = 8

    // Count number of entries
    let num_entries = if artist.is_some() { 3 } else { 2 };
    exif_data.extend_from_slice(&(num_entries as u16).to_le_bytes());

    // Entry 1: Make (0x010F)
    let make_bytes = format!("{}\0", make);
    exif_data.extend_from_slice(&[0x0F, 0x01]); // Tag ID: Make
    exif_data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    exif_data.extend_from_slice(&(make_bytes.len() as u32).to_le_bytes()); // Count
    let value_offset = 8 + 2 + (num_entries * 12) + 4; // After IFD
    exif_data.extend_from_slice(&(value_offset as u32).to_le_bytes());

    // Entry 2: Model (0x0110)
    let model_bytes = format!("{}\0", model);
    exif_data.extend_from_slice(&[0x10, 0x01]); // Tag ID: Model
    exif_data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    exif_data.extend_from_slice(&(model_bytes.len() as u32).to_le_bytes()); // Count
    let model_offset = value_offset + make_bytes.len();
    exif_data.extend_from_slice(&(model_offset as u32).to_le_bytes());

    // Entry 3: Artist (0x013B) - if provided
    let artist_offset;
    if let Some(artist_str) = artist {
        let artist_bytes = format!("{}\0", artist_str);
        exif_data.extend_from_slice(&[0x3B, 0x01]); // Tag ID: Artist
        exif_data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
        exif_data.extend_from_slice(&(artist_bytes.len() as u32).to_le_bytes()); // Count
        artist_offset = model_offset + model_bytes.len();
        exif_data.extend_from_slice(&(artist_offset as u32).to_le_bytes());
    } else {
        artist_offset = 0; // Not used, but avoid warning
    }

    // Next IFD offset (0 = none)
    exif_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // Value area
    exif_data.extend_from_slice(make_bytes.as_bytes());
    exif_data.extend_from_slice(model_bytes.as_bytes());
    if let Some(artist_str) = artist {
        let artist_bytes = format!("{}\0", artist_str);
        exif_data.extend_from_slice(artist_bytes.as_bytes());
    }

    // Write APP1 segment
    data.extend_from_slice(&[0xFF, 0xE1]); // APP1 marker
    let length = 2 + exif_data.len();
    data.extend_from_slice(&(length as u16).to_be_bytes());
    data.extend_from_slice(&exif_data);

    // EOI marker
    data.extend_from_slice(&[0xFF, 0xD9]);

    data
}

#[test]
fn test_copy_all_metadata_between_jpegs() -> Result<(), Box<dyn std::error::Error>> {
    // Create source JPEG with Make="Canon", Model="EOS R5", Artist="SourceArtist"
    let source_jpeg = create_test_jpeg_with_metadata("Canon", "EOS R5", Some("SourceArtist"));

    // Create destination JPEG with Make="Nikon", Model="D850"
    let dest_jpeg = create_test_jpeg_with_metadata("Nikon", "D850", None);

    // Write source to temp file
    let mut source_file = NamedTempFile::new()?;
    source_file.write_all(&source_jpeg)?;
    let source_path = source_file.path();

    // Write destination to temp file
    let mut dest_file = NamedTempFile::new()?;
    dest_file.write_all(&dest_jpeg)?;
    let dest_path = dest_file.path();

    // Copy all metadata from source to destination
    copy_metadata(source_path, dest_path, None)?;

    // Read destination metadata to verify
    let dest_metadata = read_metadata(dest_path)?;

    // Verify tags were copied from source
    assert_eq!(dest_metadata.get_string("EXIF:Make"), Some("Canon"));
    assert_eq!(dest_metadata.get_string("EXIF:Model"), Some("EOS R5"));
    assert_eq!(
        dest_metadata.get_string("EXIF:Artist"),
        Some("SourceArtist")
    );

    Ok(())
}

#[test]
fn test_copy_specific_tags_only() -> Result<(), Box<dyn std::error::Error>> {
    // Create source JPEG with Make="Canon", Model="EOS R5", Artist="SourceArtist"
    let source_jpeg = create_test_jpeg_with_metadata("Canon", "EOS R5", Some("SourceArtist"));

    // Create destination JPEG with Make="Nikon", Model="D850"
    let dest_jpeg = create_test_jpeg_with_metadata("Nikon", "D850", None);

    // Write source to temp file
    let mut source_file = NamedTempFile::new()?;
    source_file.write_all(&source_jpeg)?;
    let source_path = source_file.path();

    // Write destination to temp file
    let mut dest_file = NamedTempFile::new()?;
    dest_file.write_all(&dest_jpeg)?;
    let dest_path = dest_file.path();

    // Copy only Artist tag from source to destination
    let tags_to_copy = vec!["EXIF:Artist".to_string()];
    copy_metadata(source_path, dest_path, Some(&tags_to_copy))?;

    // Read destination metadata to verify
    let dest_metadata = read_metadata(dest_path)?;

    // Verify Artist was copied from source
    assert_eq!(
        dest_metadata.get_string("EXIF:Artist"),
        Some("SourceArtist")
    );

    // Verify Make and Model were NOT copied (should still have original dest values)
    assert_eq!(
        dest_metadata.get_string("EXIF:Make"),
        Some("Nikon"),
        "Make should remain unchanged"
    );
    assert_eq!(
        dest_metadata.get_string("EXIF:Model"),
        Some("D850"),
        "Model should remain unchanged"
    );

    Ok(())
}

#[test]
fn test_copy_preserves_destination_tags() -> Result<(), Box<dyn std::error::Error>> {
    // Create source JPEG with Make="Canon", Model="EOS R5"
    let source_jpeg = create_test_jpeg_with_metadata("Canon", "EOS R5", None);

    // Create destination JPEG with different tags
    let dest_jpeg = create_test_jpeg_with_metadata("Nikon", "D850", Some("DestArtist"));

    // Write source to temp file
    let mut source_file = NamedTempFile::new()?;
    source_file.write_all(&source_jpeg)?;
    let source_path = source_file.path();

    // Write destination to temp file
    let mut dest_file = NamedTempFile::new()?;
    dest_file.write_all(&dest_jpeg)?;
    let dest_path = dest_file.path();

    // Manually add a Software tag to destination
    let mut dest_metadata = read_metadata(dest_path)?;
    dest_metadata.insert("EXIF:Software", TagValue::new_string("TestSoftware"));
    write_metadata(dest_path, &dest_metadata)?;

    // Now copy Make and Model from source to destination
    let tags_to_copy = vec!["EXIF:Make".to_string(), "EXIF:Model".to_string()];
    copy_metadata(source_path, dest_path, Some(&tags_to_copy))?;

    // Read destination metadata to verify
    let final_metadata = read_metadata(dest_path)?;

    // Verify Make and Model were copied from source
    assert_eq!(final_metadata.get_string("EXIF:Make"), Some("Canon"));
    assert_eq!(final_metadata.get_string("EXIF:Model"), Some("EOS R5"));

    // Verify Artist and Software were preserved (not deleted)
    assert_eq!(
        final_metadata.get_string("EXIF:Artist"),
        Some("DestArtist"),
        "Artist should be preserved"
    );
    assert_eq!(
        final_metadata.get_string("EXIF:Software"),
        Some("TestSoftware"),
        "Software should be preserved"
    );

    Ok(())
}

#[test]
fn test_copy_overwrites_existing_tags() -> Result<(), Box<dyn std::error::Error>> {
    // Create source JPEG with Make="Canon"
    let source_jpeg = create_test_jpeg_with_metadata("Canon", "EOS R5", None);

    // Create destination JPEG with Make="Nikon"
    let dest_jpeg = create_test_jpeg_with_metadata("Nikon", "D850", None);

    // Write source to temp file
    let mut source_file = NamedTempFile::new()?;
    source_file.write_all(&source_jpeg)?;
    let source_path = source_file.path();

    // Write destination to temp file
    let mut dest_file = NamedTempFile::new()?;
    dest_file.write_all(&dest_jpeg)?;
    let dest_path = dest_file.path();

    // Copy all tags from source to destination
    copy_metadata(source_path, dest_path, None)?;

    // Read destination metadata to verify
    let dest_metadata = read_metadata(dest_path)?;

    // Verify Make was overwritten with source value
    assert_eq!(
        dest_metadata.get_string("EXIF:Make"),
        Some("Canon"),
        "Make should be overwritten from source"
    );

    // Verify Model was also overwritten
    assert_eq!(
        dest_metadata.get_string("EXIF:Model"),
        Some("EOS R5"),
        "Model should be overwritten from source"
    );

    Ok(())
}

#[test]
fn test_copy_with_empty_source() -> Result<(), Box<dyn std::error::Error>> {
    // Create minimal JPEG without EXIF
    let mut source_jpeg = Vec::new();
    source_jpeg.extend_from_slice(&[0xFF, 0xD8]); // SOI
    source_jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI

    // Create destination JPEG with metadata
    let dest_jpeg = create_test_jpeg_with_metadata("Nikon", "D850", Some("Artist"));

    // Write source to temp file
    let mut source_file = NamedTempFile::new()?;
    source_file.write_all(&source_jpeg)?;
    let source_path = source_file.path();

    // Write destination to temp file
    let mut dest_file = NamedTempFile::new()?;
    dest_file.write_all(&dest_jpeg)?;
    let dest_path = dest_file.path();

    // Copy from empty source (should preserve all destination tags)
    copy_metadata(source_path, dest_path, None)?;

    // Read destination metadata to verify
    let dest_metadata = read_metadata(dest_path)?;

    // Verify destination tags are still present (nothing was copied, nothing was deleted)
    assert_eq!(dest_metadata.get_string("EXIF:Make"), Some("Nikon"));
    assert_eq!(dest_metadata.get_string("EXIF:Model"), Some("D850"));
    assert_eq!(dest_metadata.get_string("EXIF:Artist"), Some("Artist"));

    Ok(())
}

#[test]
fn test_copy_with_nonexistent_tag_filter() -> Result<(), Box<dyn std::error::Error>> {
    // Create source JPEG with Make="Canon"
    let source_jpeg = create_test_jpeg_with_metadata("Canon", "EOS R5", None);

    // Create destination JPEG with Make="Nikon"
    let dest_jpeg = create_test_jpeg_with_metadata("Nikon", "D850", None);

    // Write source to temp file
    let mut source_file = NamedTempFile::new()?;
    source_file.write_all(&source_jpeg)?;
    let source_path = source_file.path();

    // Write destination to temp file
    let mut dest_file = NamedTempFile::new()?;
    dest_file.write_all(&dest_jpeg)?;
    let dest_path = dest_file.path();

    // Try to copy a tag that doesn't exist in source
    let tags_to_copy = vec!["EXIF:Copyright".to_string()];
    copy_metadata(source_path, dest_path, Some(&tags_to_copy))?;

    // Read destination metadata to verify
    let dest_metadata = read_metadata(dest_path)?;

    // Verify destination tags are unchanged (filtered tag didn't exist in source)
    assert_eq!(
        dest_metadata.get_string("EXIF:Make"),
        Some("Nikon"),
        "Make should remain unchanged"
    );
    assert_eq!(
        dest_metadata.get_string("EXIF:Model"),
        Some("D850"),
        "Model should remain unchanged"
    );

    // Verify Copyright tag was not added (it didn't exist in source)
    assert_eq!(dest_metadata.get_string("EXIF:Copyright"), None);

    Ok(())
}
