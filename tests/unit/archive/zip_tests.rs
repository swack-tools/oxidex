//! Integration tests for ZIP archive parser

use crate::zip_writer::{Entry, build};
use oxidex::core::{FormatParser, TagValue};
use oxidex::io::BufferedReader;
use oxidex::parsers::archive::ZipParser;

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
    let report = b"Confidential report content here. ".repeat(50);
    let data = build(
        &[
            Entry::deflated("documents/report.txt", &report).at(2024, 1, 10, 9, 15, 0),
            // JPEG header, stored
            Entry::stored("images/photo.jpg", &[0xFF, 0xD8, 0xFF, 0xE0])
                .at(2024, 2, 15, 14, 30, 45),
            Entry::deflated("config.ini", b"[settings]\nkey=value\n").at(2024, 3, 1, 16, 45, 30),
        ],
        "Evidence archive created 2024-03-15",
    );
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
    let data = build(&[Entry::deflated("test.txt", b"Test content")], "");
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
    let data = build(&[Entry::deflated("file.txt", b"content")], "");
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Unix mode may or may not be present depending on how the archive was created
    // This test just verifies the parser doesn't crash when checking for it
    let _ = metadata.get("ZIP:File1:UnixMode");
}

// ---------------------------------------------------------------------------
// Fixture-based ZIP parser tests.
//
// These were unit tests in src/parsers/archive/zip.rs. They moved here when
// the `zip` crate was removed: their fixtures need a ZIP writer, the
// dependency-free replacement lives in tests/common/zip_writer.rs, and
// src/test_support is #[cfg(test)] on the library so a separate test crate
// cannot reach it. Keeping one writer beat keeping two copies in sync.
// ---------------------------------------------------------------------------
fn read_one_timestamp(
    y: u16,
    mo: u8,
    d: u8,
    h: u8,
    mi: u8,
    s: u8,
) -> oxidex::parsers::archive::zip_reader::DosDateTime {
    let data = build(&[Entry::stored("a.txt", b"x").at(y, mo, d, h, mi, s)], "");
    oxidex::parsers::archive::zip_reader::ZipReader::new(&data)
        .expect("fixture should parse")
        .by_index(0)
        .expect("one entry")
        .last_modified()
        .expect("timestamp present")
}

#[test]
fn test_zip_crc_metadata_format() {
    // Deflated, matching what SimpleFileOptions::default() produced, so
    // the asserted CRC is still the CRC of the uncompressed bytes.
    let data = build(&[Entry::deflated("test.txt", b"ExifTool test file\n")], "");
    let reader = BufferedReader::from_bytes(&data);
    let metadata = ZipParser.parse(&reader).unwrap();

    assert_eq!(
        metadata.get("ZIP:ZipCRC"),
        Some(&TagValue::new_string("0x6e17461a".to_string()))
    );
}

#[test]
fn test_compression_method_names() {
    // Names come from the reader's own table now, which covers every
    // method APPNOTE assigns rather than only the four the zip crate
    // named -- an LZMA entry used to report "Unknown".
    for (code, want) in [
        (0u16, "Stored"),
        (8, "Deflate"),
        (12, "Bzip2"),
        (93, "Zstd"),
        (14, "LZMA"),
    ] {
        let data = build(&[Entry::stored("a", b"x")], "");
        let mut data = data;
        let cd = data
            .windows(4)
            .position(|w| w == 0x0201_4b50u32.to_le_bytes())
            .expect("central header");
        data[cd + 10..cd + 12].copy_from_slice(&code.to_le_bytes());
        let r = oxidex::parsers::archive::zip_reader::ZipReader::new(&data).expect("parse");
        assert_eq!(r.by_index(0).expect("entry").compression_name(), want);
    }
}

#[test]
fn test_empty_zip_archive() {
    // Create an empty ZIP archive
    let data = build(&[], "Test archive comment");
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Verify basic metadata
    assert_eq!(
        metadata.get("ZIP:FileCount"),
        Some(&TagValue::new_integer(0))
    );

    // Verify comment
    assert_eq!(
        metadata.get("ZIP:Comment"),
        Some(&TagValue::new_string("Test archive comment".to_string()))
    );

    // Verify forensic fields
    assert_eq!(
        metadata.get("ZIP:TotalCompressedSize"),
        Some(&TagValue::new_integer(0))
    );
    assert_eq!(
        metadata.get("ZIP:TotalUncompressedSize"),
        Some(&TagValue::new_integer(0))
    );
}

#[test]
fn test_zip_with_files() {
    // Create a ZIP archive with test files
    let data = build(
        &[
            Entry::stored("test1.txt", b"Hello, World!").at(2024, 1, 15, 10, 30, 0),
            Entry::deflated(
                "test2.txt",
                b"This is a longer text that should compress well with deflate compression algorithm.",
            )
            .at(2024, 3, 20, 15, 45, 30),
        ],
        "",
    );
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Verify file count
    assert_eq!(
        metadata.get("ZIP:FileCount"),
        Some(&TagValue::new_integer(2))
    );

    // Verify file names
    assert!(metadata.contains_key("ZIP:File1:Filename"));
    assert!(metadata.contains_key("ZIP:File2:Filename"));
    assert_eq!(
        metadata.get("ZIP:File1:Filename"),
        Some(&TagValue::new_string("test1.txt".to_string()))
    );

    // Verify sizes
    assert!(metadata.contains_key("ZIP:File1:CompressedSize"));
    assert!(metadata.contains_key("ZIP:File1:UncompressedSize"));
    assert!(metadata.contains_key("ZIP:File2:CompressedSize"));
    assert!(metadata.contains_key("ZIP:File2:UncompressedSize"));

    // Verify CRC32
    assert!(metadata.contains_key("ZIP:File1:CRC32"));
    assert!(metadata.contains_key("ZIP:File2:CRC32"));

    // Verify compression methods
    assert_eq!(
        metadata.get("ZIP:File1:CompressionMethod"),
        Some(&TagValue::new_string("Stored".to_string()))
    );
    assert_eq!(
        metadata.get("ZIP:File2:CompressionMethod"),
        Some(&TagValue::new_string("Deflate".to_string()))
    );

    // Verify timestamps
    assert_eq!(
        metadata.get("ZIP:File1:LastModified"),
        Some(&TagValue::new_string("2024-01-15T10:30:00".to_string()))
    );
    assert_eq!(
        metadata.get("ZIP:File2:LastModified"),
        Some(&TagValue::new_string("2024-03-20T15:45:30".to_string()))
    );

    // Verify version made by
    assert!(metadata.contains_key("ZIP:File1:VersionMadeBy"));
    assert!(metadata.contains_key("ZIP:File2:VersionMadeBy"));

    // Verify comma-separated file list (backward compatibility)
    assert_eq!(
        metadata.get("ZIP:Files"),
        Some(&TagValue::new_string("test1.txt, test2.txt".to_string()))
    );

    // Verify forensic summary fields
    assert!(metadata.contains_key("ZIP:TotalCompressedSize"));
    assert!(metadata.contains_key("ZIP:TotalUncompressedSize"));
    assert!(metadata.contains_key("ZIP:CompressionRatio"));

    // Verify date range
    assert_eq!(
        metadata.get("ZIP:OldestFileDate"),
        Some(&TagValue::new_string("2024-01-15T10:30:00".to_string()))
    );
    assert_eq!(
        metadata.get("ZIP:NewestFileDate"),
        Some(&TagValue::new_string("2024-03-20T15:45:30".to_string()))
    );
}

#[test]
fn test_zip_with_directory() {
    // Create a ZIP archive with a directory
    // A trailing slash makes it a directory entry with no payload, which
    // is exactly how add_directory wrote it.
    let data = build(
        &[
            Entry::deflated("test_dir/", b""),
            Entry::deflated("test_dir/file.txt", b"Content"),
        ],
        "",
    );
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Verify directory is detected
    assert_eq!(
        metadata.get("ZIP:FileCount"),
        Some(&TagValue::new_integer(2))
    );
    assert_eq!(
        metadata.get("ZIP:File1:IsDirectory"),
        Some(&TagValue::new_string("true".to_string()))
    );
}

#[test]
fn test_compression_ratio_calculation() {
    // Create a ZIP with highly compressible data
    // Highly compressible, so the ratio is meaningfully small.
    let body = "A".repeat(10000);
    let data = build(&[Entry::deflated("repeated.txt", body.as_bytes())], "");
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Verify compression ratio exists and is reasonable (should be very low for repeated data)
    assert!(metadata.contains_key("ZIP:CompressionRatio"));
    if let Some(TagValue::String(ratio_str)) = metadata.get("ZIP:CompressionRatio") {
        let ratio_value: f64 = ratio_str
            .trim_end_matches('%')
            .parse()
            .expect("Failed to parse ratio");
        // Highly compressible data should have very low ratio
        assert!(
            ratio_value < 10.0,
            "Compression ratio should be < 10% for repeated data"
        );
    }
}

#[test]
fn test_zip64_detection() {
    // We can't easily create a true ZIP64 file in tests, but we can verify the logic
    // by checking that normal files don't get flagged as ZIP64
    let data = build(&[Entry::deflated("small.txt", b"Small file")], "");
    let reader = BufferedReader::from_bytes(&data);
    let parser = ZipParser;

    let metadata = parser.parse(&reader).unwrap();

    // Small archive should not be detected as ZIP64
    assert!(!metadata.contains_key("ZIP:IsZIP64"));
}

/// DOS timestamps, asserted through the published tag rather than the private
/// conversion helper. This is the stronger test: it covers the packing in the
/// writer, the unpacking in the reader, and the formatting in the parser,
/// which is what a consumer actually sees.
#[test]
fn test_last_modified_tag_formatting() {
    // DOS stores seconds in two-second units, so 44 round-trips exactly.
    let data = build(
        &[Entry::deflated("a.txt", b"x").at(2024, 3, 15, 14, 30, 44)],
        "",
    );
    let metadata = ZipParser.parse(&BufferedReader::from_bytes(&data)).unwrap();
    assert_eq!(
        metadata.get("ZIP:File1:LastModified"),
        Some(&TagValue::new_string("2024-03-15T14:30:44".to_string()))
    );

    // Earliest representable DOS date. A zero date AND time means "absent",
    // so 1980-01-01 is encoded with non-zero day and month bits.
    let data = build(
        &[Entry::deflated("a.txt", b"x").at(1980, 1, 1, 0, 0, 0)],
        "",
    );
    let metadata = ZipParser.parse(&BufferedReader::from_bytes(&data)).unwrap();
    assert_eq!(
        metadata.get("ZIP:File1:LastModified"),
        Some(&TagValue::new_string("1980-01-01T00:00:00".to_string()))
    );
}

/// Date ordering, likewise asserted through the tags rather than the private
/// comparison helper.
#[test]
fn test_oldest_and_newest_file_dates() {
    let data = build(
        &[
            Entry::deflated("middle.txt", b"m").at(2024, 6, 15, 12, 0, 0),
            Entry::deflated("oldest.txt", b"o").at(2020, 1, 2, 3, 4, 0),
            Entry::deflated("newest.txt", b"n").at(2026, 12, 31, 23, 59, 58),
        ],
        "",
    );
    let metadata = ZipParser.parse(&BufferedReader::from_bytes(&data)).unwrap();
    assert_eq!(
        metadata.get("ZIP:OldestFileDate"),
        Some(&TagValue::new_string("2020-01-02T03:04:00".to_string()))
    );
    assert_eq!(
        metadata.get("ZIP:NewestFileDate"),
        Some(&TagValue::new_string("2026-12-31T23:59:58".to_string()))
    );
}
