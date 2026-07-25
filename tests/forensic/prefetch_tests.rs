//! Integration tests for Windows Prefetch file parser
//!
//! These tests verify comprehensive metadata extraction from Prefetch files
//! across different Windows versions (XP, Vista, Win8, Win10/11).

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::{FormatParser, TagValue};
use oxidex::parsers::specialized::prefetch::PrefetchParser;

/// Helper function to create a Prefetch file with specific parameters
///
/// This builder creates synthetic Prefetch files matching Windows format specifications.
/// The layout is:
/// - Offset 0: Version (4 bytes, little-endian)
/// - Offset 4: Signature "SCCA" (4 bytes)
/// - Offset 12: File size (4 bytes, little-endian)
/// - Offset 16: Executable name (60 bytes, UTF-16LE)
/// - Offset 76: Path hash (4 bytes, little-endian)
/// - Offset 120/128: Last run time (8 bytes FILETIME, varies by version)
/// - Offset 136: Previous run times for v26+ (7 × 8 bytes FILETIME)
/// - Offset 144: Run count (4 bytes, little-endian)
fn create_prefetch_file(version: u32, exe_name: &str, run_count: u32) -> Vec<u8> {
    let mut data = vec![0u8; 256]; // Allocate enough space for header and file info section

    // Offset 0: Version (4 bytes, little-endian)
    data[0..4].copy_from_slice(&version.to_le_bytes());

    // Offset 4: Signature "SCCA" (4 bytes)
    data[4..8].copy_from_slice(b"SCCA");

    // Offset 12: File size (4 bytes, little-endian)
    // Using a realistic file size based on version
    let file_size = match version {
        17 => 12000u32, // XP typical size
        23 => 20000u32, // Vista/7 typical size
        26 => 30000u32, // Win8 typical size
        30 => 45000u32, // Win10+ typical size
        _ => 40000u32,
    };
    data[12..16].copy_from_slice(&file_size.to_le_bytes());

    // Offset 16: Executable name (60 bytes, UTF-16LE)
    // Encode the executable name in UTF-16LE, truncate to 60 bytes if needed
    let encoded = exe_name.encode_utf16().collect::<Vec<u16>>();
    for (i, &ch) in encoded.iter().take(30).enumerate() {
        let pos = 16 + (i * 2);
        data[pos..pos + 2].copy_from_slice(&ch.to_le_bytes());
    }
    // Rest of the name area is zero-padded (already zeroed by initialization)

    // Offset 76: Path hash (4 bytes, little-endian)
    // Use a hash that corresponds to a typical Windows path
    let path_hash = 0xDEADBEEF_u32;
    data[76..80].copy_from_slice(&path_hash.to_le_bytes());

    // Offset 120/128: Last run time (version-dependent)
    // Using Windows FILETIME for a realistic execution timestamp
    // 2024-01-15 12:30:45 UTC
    let last_run_filetime: u64 = 133500420450000000;

    match version {
        17 | 23 => {
            // Versions 17 and 23: last run time at offset 120
            data[120..128].copy_from_slice(&last_run_filetime.to_le_bytes());
        }
        26 | 30 => {
            // Versions 26 and 30: last run time at offset 128
            data[128..136].copy_from_slice(&last_run_filetime.to_le_bytes());
            // Also add a previous run time at offset 136 for v26/v30
            let prev_run_filetime: u64 = 133499420450000000; // 1 day earlier
            data[136..144].copy_from_slice(&prev_run_filetime.to_le_bytes());
        }
        _ => {
            // Default to newer format
            data[128..136].copy_from_slice(&last_run_filetime.to_le_bytes());
        }
    }

    // Offset 144: Run count (4 bytes, little-endian)
    // This offset is consistent across all versions (17, 23, 26, 30)
    data[144..148].copy_from_slice(&run_count.to_le_bytes());

    data
}

/// Test 1: Windows XP Prefetch (Version 17)
///
/// Verifies that:
/// - Version 17 is correctly identified as Windows XP
/// - Basic metadata (exe name, hash) is extracted
#[test]
fn test_prefetch_windows_xp() {
    let prefetch_data = create_prefetch_file(17, "NOTEPAD.EXE", 5);
    let reader = TestReader::new(prefetch_data);
    let parser = PrefetchParser;

    let metadata = parser
        .parse(&reader)
        .expect("Should parse XP prefetch file");

    // Verify version identification
    assert_eq!(
        metadata.get("Prefetch:Version"),
        Some(&TagValue::String("17 (Windows XP/2003)".to_string())),
        "Version should be identified as 17 (Windows XP/2003)"
    );

    // Verify executable name extraction
    assert_eq!(
        metadata.get("Prefetch:ExecutableName"),
        Some(&TagValue::String("NOTEPAD.EXE".to_string())),
        "Executable name should be NOTEPAD.EXE"
    );

    // Verify path hash extraction (should be hex formatted)
    assert_eq!(
        metadata.get("Prefetch:PathHash"),
        Some(&TagValue::String("0xDEADBEEF".to_string())),
        "Path hash should be in hex format"
    );

    // Verify file type
    assert_eq!(
        metadata.get("Prefetch:FileType"),
        Some(&TagValue::String("Windows Prefetch".to_string())),
        "File type should be Windows Prefetch"
    );

    // Verify compression flag for uncompressed file
    assert_eq!(
        metadata.get("Prefetch:IsCompressed"),
        Some(&TagValue::String("false".to_string())),
        "XP file should not be compressed"
    );

    // Verify last run time is present
    assert!(
        metadata.contains_key("Prefetch:LastRunTime"),
        "Last run time should be present"
    );
}

/// Test 2: Windows 10/11 Prefetch (Version 30)
///
/// Verifies that:
/// - Version 30 is correctly identified as Windows 10/11
/// - All expected metadata fields are present
/// - Previous run times are extracted (v30+ feature)
#[test]
fn test_prefetch_windows_10() {
    let prefetch_data = create_prefetch_file(30, "CHROME.EXE", 127);
    let reader = TestReader::new(prefetch_data);
    let parser = PrefetchParser;

    let metadata = parser
        .parse(&reader)
        .expect("Should parse Win10 prefetch file");

    // Verify version identification
    assert_eq!(
        metadata.get("Prefetch:Version"),
        Some(&TagValue::String("30 (Windows 10/11)".to_string())),
        "Version should be identified as 30 (Windows 10/11)"
    );

    // Verify executable name
    assert_eq!(
        metadata.get("Prefetch:ExecutableName"),
        Some(&TagValue::String("CHROME.EXE".to_string())),
        "Executable name should be CHROME.EXE"
    );

    // Verify file size is present
    assert!(
        metadata.contains_key("Prefetch:FileSize"),
        "File size should be present"
    );

    // Verify last run time is present and formatted as ISO 8601
    assert!(
        metadata.contains_key("Prefetch:LastRunTime"),
        "Last run time should be present"
    );

    if let Some(TagValue::String(last_run)) = metadata.get("Prefetch:LastRunTime") {
        assert!(
            last_run.contains("T") && last_run.contains("Z"),
            "Last run time should be in ISO 8601 format (2024-01-15T12:30:45Z)"
        );
    }

    // Verify previous run times are present for v30
    assert!(
        metadata.contains_key("Prefetch:PreviousRunTimes"),
        "Windows 10/11 should have previous run times"
    );

    assert!(
        metadata.contains_key("Prefetch:PreviousRunTimesCount"),
        "Previous run times count should be present"
    );

    // Verify forensic note is present
    assert!(
        metadata.contains_key("Prefetch:ForensicNote"),
        "Forensic note should be present"
    );

    if let Some(TagValue::String(note)) = metadata.get("Prefetch:ForensicNote") {
        assert!(
            note.contains("127"),
            "Forensic note should mention the run count (127)"
        );
    }
}

/// Test 3: Run Count Extraction
///
/// Verifies that:
/// - Run count is correctly extracted from offset 144
/// - Run count works across different versions
/// - Run count is properly formatted in the output
#[test]
fn test_prefetch_run_count() {
    // Test with version 23 (Vista/7) and specific run count
    let run_counts = vec![1, 5, 42, 255];

    for run_count in run_counts {
        let prefetch_data = create_prefetch_file(23, "TEST.EXE", run_count);
        let reader = TestReader::new(prefetch_data);
        let parser = PrefetchParser;

        let metadata = parser.parse(&reader).expect(&format!(
            "Should parse prefetch with run count {}",
            run_count
        ));

        assert_eq!(
            metadata.get("Prefetch:RunCount"),
            Some(&TagValue::String(run_count.to_string())),
            "Run count should be {} for this file",
            run_count
        );
    }
}

/// Test 4: Hash Extraction
///
/// Verifies that:
/// - Path hash tag exists and is properly formatted
/// - Hash is extracted from offset 76
/// - Hash is displayed in hexadecimal format
#[test]
fn test_prefetch_hash_extraction() {
    let prefetch_data = create_prefetch_file(26, "EXPLORER.EXE", 15);
    let reader = TestReader::new(prefetch_data);
    let parser = PrefetchParser;

    let metadata = parser
        .parse(&reader)
        .expect("Should parse Win8 prefetch file");

    // Verify hash tag exists
    assert!(
        metadata.contains_key("Prefetch:PathHash"),
        "PathHash tag must exist"
    );

    // Verify hash format
    if let Some(TagValue::String(hash)) = metadata.get("Prefetch:PathHash") {
        // Hash should be in hexadecimal format (0xDEADBEEF)
        assert!(
            hash.starts_with("0x"),
            "Hash should be in hexadecimal format starting with 0x"
        );
        assert_eq!(
            hash.len(),
            10, // "0x" + 8 hex digits
            "Hash should be 10 characters (0x + 8 hex digits)"
        );
        // Verify it's valid hex
        assert!(
            u32::from_str_radix(&hash[2..], 16).is_ok(),
            "Hash value should be valid hexadecimal"
        );
    } else {
        panic!("PathHash should be a string value");
    }
}

/// Additional test: Multiple versions with same executable
///
/// Verifies compatibility across Windows versions for the same executable
#[test]
fn test_prefetch_version_compatibility() {
    let versions = vec![17, 23, 26, 30];
    let exe_name = "IEXPLORE.EXE";

    for version in versions {
        let prefetch_data = create_prefetch_file(version, exe_name, 10);
        let reader = TestReader::new(prefetch_data);
        let parser = PrefetchParser;

        let metadata = parser
            .parse(&reader)
            .expect(&format!("Should parse version {} prefetch", version));

        assert_eq!(
            metadata.get("Prefetch:ExecutableName"),
            Some(&TagValue::String(exe_name.to_string())),
            "Executable name should be {} for version {}",
            exe_name,
            version
        );

        assert_eq!(
            metadata.get("Prefetch:RunCount"),
            Some(&TagValue::String("10".to_string())),
            "Run count should be 10 for version {}",
            version
        );
    }
}

/// Test: Invalid signature detection
///
/// Verifies that invalid Prefetch files are properly rejected
#[test]
fn test_prefetch_invalid_signature() {
    let mut data = vec![0u8; 256];
    // Set a valid version but invalid signature
    data[0..4].copy_from_slice(&30u32.to_le_bytes());
    data[4..8].copy_from_slice(b"XXXX"); // Invalid signature, not "SCCA"

    let reader = TestReader::new(data);
    let parser = PrefetchParser;

    let result = parser.parse(&reader);
    assert!(
        result.is_err(),
        "Parser should reject files with invalid signature"
    );
}

/// Test: File too small
///
/// Verifies that files smaller than minimum size are rejected
#[test]
fn test_prefetch_too_small() {
    let data = vec![0u8; 50]; // Less than minimum 84 bytes

    let reader = TestReader::new(data);
    let parser = PrefetchParser;

    let result = parser.parse(&reader);
    assert!(
        result.is_err(),
        "Parser should reject files smaller than minimum size"
    );
}

/// Test: Compressed file detection
///
/// Verifies that MAM-compressed files are detected and handled appropriately
#[test]
fn test_prefetch_compressed_detection() {
    let mut data = vec![0u8; 256];
    // MAM compressed signature instead of SCCA
    data[0..4].copy_from_slice(b"MAM\x04");
    // Rest of data is not parsed for compressed files
    data[4..8].copy_from_slice(&30u32.to_le_bytes()); // Add some data

    let reader = TestReader::new(data);
    let parser = PrefetchParser;

    let metadata = parser
        .parse(&reader)
        .expect("Should detect and partially parse compressed file");

    // Should detect compression
    assert_eq!(
        metadata.get("Prefetch:IsCompressed"),
        Some(&TagValue::String("true (MAM LZXPRESS HUFFMAN)".to_string())),
        "Should detect MAM compression"
    );

    // Should indicate it's compressed
    assert_eq!(
        metadata.get("Prefetch:FileType"),
        Some(&TagValue::String(
            "Windows Prefetch (Compressed)".to_string()
        )),
        "File type should indicate compression"
    );

    // Should have a note about requiring decompression
    assert!(
        metadata.contains_key("Prefetch:Note"),
        "Should include note about decompression requirement"
    );
}

/// Test: UTF-16LE executable name decoding
///
/// Verifies that various executable names are correctly decoded from UTF-16LE
#[test]
fn test_prefetch_utf16_names() {
    let exe_names = vec![
        "CMD.EXE",
        "POWERSHELL.EXE",
        "A.COM",
        "VERYLONGNAMETHATSTILLFITIN.EXE",
    ];

    for exe_name in exe_names {
        let prefetch_data = create_prefetch_file(30, exe_name, 1);
        let reader = TestReader::new(prefetch_data);
        let parser = PrefetchParser;

        let metadata = parser
            .parse(&reader)
            .expect(&format!("Should parse file with name '{}'", exe_name));

        assert_eq!(
            metadata.get("Prefetch:ExecutableName"),
            Some(&TagValue::String(exe_name.to_string())),
            "Executable name '{}' should be correctly decoded",
            exe_name
        );
    }
}

/// Test: Forensic note generation
///
/// Verifies that forensic notes are properly generated with run count and timestamp
#[test]
fn test_prefetch_forensic_note() {
    let prefetch_data = create_prefetch_file(30, "SUSPICIOUS.EXE", 99);
    let reader = TestReader::new(prefetch_data);
    let parser = PrefetchParser;

    let metadata = parser.parse(&reader).expect("Should parse prefetch file");

    assert!(
        metadata.contains_key("Prefetch:ForensicNote"),
        "Forensic note should be present"
    );

    if let Some(TagValue::String(note)) = metadata.get("Prefetch:ForensicNote") {
        // Note should contain run count
        assert!(
            note.contains("99"),
            "Forensic note should mention run count (99)"
        );
        // Note should mention execution
        assert!(
            note.to_lowercase().contains("executed"),
            "Forensic note should mention execution"
        );
        // Note should mention time
        assert!(
            note.contains("Last execution"),
            "Forensic note should mention last execution time"
        );
    } else {
        panic!("ForensicNote should be a string value");
    }
}
