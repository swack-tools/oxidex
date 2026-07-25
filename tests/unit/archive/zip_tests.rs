//! Integration tests for ZIP archive parser

use oxidex::core::{FormatParser, TagValue};
use oxidex::io::BufferedReader;
use oxidex::parsers::archive::ZipParser;
use std::io::Write;
use zip::write::{SimpleFileOptions, ZipWriter};

#[test]
fn test_zip_invalid_signature() {
    let data = b"Not a ZIP file";
    let reader = BufferedReader::from_bytes(data);
    let parser = ZipParser;

    let result = parser.parse(&reader);
    assert!(result.is_err());
}

#[test]
fn test_zip_too_small() {
    let data = b"PK";
    let reader = BufferedReader::from_bytes(data);
    let parser = ZipParser;

    let result = parser.parse(&reader);
    assert!(result.is_err());
}

#[test]
fn test_zip_forensic_metadata_extraction() {
    // Create a realistic ZIP archive with multiple files for forensic analysis
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buffer);
        zip.set_comment("Evidence archive created 2024-03-15");

        // Document file
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::from_date_and_time(2024, 1, 10, 9, 15, 0).unwrap());
        zip.start_file("documents/report.txt", options).unwrap();
        let content = b"Confidential report content here. ".repeat(50);
        zip.write_all(&content).unwrap();

        // Image file (stored)
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(
                zip::DateTime::from_date_and_time(2024, 2, 15, 14, 30, 45).unwrap(),
            );
        zip.start_file("images/photo.jpg", options).unwrap();
        zip.write_all(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap(); // JPEG header

        // Config file
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::from_date_and_time(2024, 3, 1, 16, 45, 30).unwrap());
        zip.start_file("config.ini", options).unwrap();
        zip.write_all(b"[settings]\nkey=value\n").unwrap();

        zip.finish().unwrap();
    }

    let data = buffer.into_inner();
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Verify archive-level metadata
    assert_eq!(
        metadata.get("ZIP:FileCount"),
        Some(&TagValue::new_integer(3))
    );

    assert_eq!(
        metadata.get("ZIP:Comment"),
        Some(&TagValue::new_string(
            "Evidence archive created 2024-03-15".to_string()
        ))
    );

    // Verify per-file metadata exists
    for i in 1..=3 {
        let prefix = format!("ZIP:File{}:", i);
        assert!(
            metadata.contains_key(&format!("{}Filename", prefix)),
            "Missing filename for file {}",
            i
        );
        assert!(
            metadata.contains_key(&format!("{}CompressedSize", prefix)),
            "Missing compressed size for file {}",
            i
        );
        assert!(
            metadata.contains_key(&format!("{}UncompressedSize", prefix)),
            "Missing uncompressed size for file {}",
            i
        );
        assert!(
            metadata.contains_key(&format!("{}CRC32", prefix)),
            "Missing CRC32 for file {}",
            i
        );
        assert!(
            metadata.contains_key(&format!("{}CompressionMethod", prefix)),
            "Missing compression method for file {}",
            i
        );
        assert!(
            metadata.contains_key(&format!("{}LastModified", prefix)),
            "Missing last modified for file {}",
            i
        );
        assert!(
            metadata.contains_key(&format!("{}VersionMadeBy", prefix)),
            "Missing version made by for file {}",
            i
        );
    }

    // Verify forensic summary fields
    assert!(metadata.contains_key("ZIP:TotalCompressedSize"));
    assert!(metadata.contains_key("ZIP:TotalUncompressedSize"));
    assert!(metadata.contains_key("ZIP:CompressionRatio"));
    assert!(metadata.contains_key("ZIP:OldestFileDate"));
    assert!(metadata.contains_key("ZIP:NewestFileDate"));

    // Verify date ordering
    assert_eq!(
        metadata.get("ZIP:OldestFileDate"),
        Some(&TagValue::new_string("2024-01-10T09:15:00".to_string()))
    );
    assert_eq!(
        metadata.get("ZIP:NewestFileDate"),
        Some(&TagValue::new_string("2024-03-01T16:45:30".to_string()))
    );

    // Verify compression methods are captured correctly
    let file2_compression = metadata.get("ZIP:File2:CompressionMethod");
    assert_eq!(
        file2_compression,
        Some(&TagValue::new_string("Stored".to_string())),
        "Image file should be stored (not compressed)"
    );

    // Verify backward compatibility - comma-separated file list
    assert!(metadata.contains_key("ZIP:Files"));
    let files_list = metadata.get("ZIP:Files").unwrap();
    if let TagValue::String(files) = files_list {
        assert!(files.contains("report.txt"));
        assert!(files.contains("photo.jpg"));
        assert!(files.contains("config.ini"));
    }
}

#[test]
fn test_zip_crc32_format() {
    // Test that CRC32 is formatted as hex string with 0x prefix
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buffer);
        let options = SimpleFileOptions::default();
        zip.start_file("test.txt", options).unwrap();
        zip.write_all(b"Test content").unwrap();
        zip.finish().unwrap();
    }

    let data = buffer.into_inner();
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Verify CRC32 format
    let crc32 = metadata.get("ZIP:File1:CRC32").unwrap();
    if let TagValue::String(crc_str) = crc32 {
        assert!(crc_str.starts_with("0x"), "CRC32 should start with 0x");
        assert_eq!(crc_str.len(), 10, "CRC32 should be 0x + 8 hex digits");
    }
}

#[test]
fn test_zip_unix_mode_extraction() {
    // Test Unix file mode extraction (requires Unix-style archive)
    // This is a basic test as creating Unix-mode archives requires specific setup
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buffer);
        let options = SimpleFileOptions::default();
        zip.start_file("file.txt", options).unwrap();
        zip.write_all(b"content").unwrap();
        zip.finish().unwrap();
    }

    let data = buffer.into_inner();
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Unix mode may or may not be present depending on how the archive was created
    // This test just verifies the parser doesn't crash when checking for it
    let _ = metadata.get("ZIP:File1:UnixMode");
}
