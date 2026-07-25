//! EVTX event log parser integration tests
//!
//! Comprehensive test suite for Windows Event Log (EVTX) metadata extraction.
//! Tests cover basic parsing, flag detection, and version information extraction.
//!
//! EVTX files are critical for digital forensics investigations, containing:
//! - System events and security logs
//! - Forensic indicators (dirty/full flags)
//! - Event timeline information
//! - Corruption detection via checksums

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::specialized::evtx::parse_evtx_metadata;

/// Creates a minimal EVTX file header with configurable parameters
///
/// Constructs a 4096-byte EVTX header block following the Windows Event Log
/// specification. The header contains file metadata and forensic indicators.
///
/// # Header Structure (128 bytes of actual data within 4096-byte block)
/// - Offset 0: "ElfFile\0" signature (8 bytes)
/// - Offset 8: First chunk number (8 bytes LE)
/// - Offset 16: Last chunk number (8 bytes LE)
/// - Offset 24: Next record ID (8 bytes LE)
/// - Offset 32: Header size (4 bytes LE)
/// - Offset 36: Minor version (2 bytes LE)
/// - Offset 38: Major version (2 bytes LE)
/// - Offset 40: Header block size (2 bytes LE)
/// - Offset 42: Chunk count (2 bytes LE)
/// - Offset 76: Flags (4 bytes LE)
/// - Offset 120: Checksum (4 bytes LE)
///
/// # Parameters
/// * `chunk_count` - Number of chunks in the file
/// * `dirty` - Set FLAG_DIRTY (0x01) if true
/// * `full` - Set FLAG_FULL (0x02) if true
///
/// # Returns
/// 4096-byte vector with properly formatted EVTX header
fn create_evtx_header(chunk_count: u16, dirty: bool, full: bool) -> Vec<u8> {
    let mut data = vec![0u8; 4096];

    // Magic signature: "ElfFile\0" at offset 0
    data[0..8].copy_from_slice(b"ElfFile\0");

    // First chunk number (offset 8, 8 bytes)
    // Start with chunk 0
    data[8..16].copy_from_slice(&0u64.to_le_bytes());

    // Last chunk number (offset 16, 8 bytes)
    // If chunk_count is 5, last chunk is 4
    data[16..24].copy_from_slice(&(chunk_count.saturating_sub(1) as u64).to_le_bytes());

    // Next record ID (offset 24, 8 bytes)
    // Estimate: 100 records per chunk
    let estimated_records = (chunk_count as u64) * 100 + 1;
    data[24..32].copy_from_slice(&estimated_records.to_le_bytes());

    // Header size (offset 32, 4 bytes) = 128 bytes
    data[32..36].copy_from_slice(&128u32.to_le_bytes());

    // Minor version (offset 36, 2 bytes)
    // EVTX version 3.1: minor = 1
    data[36..38].copy_from_slice(&1u16.to_le_bytes());

    // Major version (offset 38, 2 bytes)
    // EVTX version 3.1: major = 3
    data[38..40].copy_from_slice(&3u16.to_le_bytes());

    // Header block size (offset 40, 2 bytes) = 4096 bytes
    data[40..42].copy_from_slice(&4096u16.to_le_bytes());

    // Chunk count (offset 42, 2 bytes)
    data[42..44].copy_from_slice(&chunk_count.to_le_bytes());

    // Flags (offset 76, 4 bytes)
    // FLAG_DIRTY = 0x01, FLAG_FULL = 0x02
    let mut flags = 0u32;
    if dirty {
        flags |= 0x01;
    }
    if full {
        flags |= 0x02;
    }
    data[76..80].copy_from_slice(&flags.to_le_bytes());

    // Checksum (offset 120, 4 bytes)
    // Use a dummy checksum value
    data[120..124].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

    data
}

/// Test: Basic EVTX parsing with file type and chunk count
///
/// Verifies that the parser correctly:
/// 1. Identifies EVTX file type
/// 2. Extracts chunk count from header
/// 3. Parses all required header fields
#[test]
fn test_evtx_basic_parsing() {
    let data = create_evtx_header(5, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Verify file type is identified correctly
    // Note: Parser returns "Windows Event Log" not "EVTX"
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("Windows Event Log".to_string())),
        "FileType should be Windows Event Log"
    );

    // Verify chunk count extraction
    assert_eq!(
        metadata.get("EVTX:ChunkCount"),
        Some(&TagValue::String("5".to_string())),
        "ChunkCount should be 5"
    );

    // Verify file size is calculated
    assert_eq!(
        metadata.get("FileSize"),
        Some(&TagValue::String("4096".to_string())),
        "FileSize should be 4096"
    );

    // Verify version extraction
    assert_eq!(
        metadata.get("EVTX:Version"),
        Some(&TagValue::String("3.1".to_string())),
        "Version should be 3.1"
    );

    // Verify header metadata
    assert_eq!(
        metadata.get("EVTX:HeaderSize"),
        Some(&TagValue::String("128 bytes".to_string())),
        "HeaderSize should be 128 bytes"
    );
}

/// Test: Dirty flag detection
///
/// Verifies that FLAG_DIRTY (0x01) is correctly detected and reported.
/// The dirty flag indicates the log file was not properly closed,
/// suggesting a system crash or improper shutdown - critical forensic evidence.
#[test]
fn test_evtx_dirty_flag() {
    let data = create_evtx_header(10, true, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Verify dirty flag is set (parser returns string "true")
    assert_eq!(
        metadata.get("EVTX:IsDirty"),
        Some(&TagValue::String("true".to_string())),
        "IsDirty should be true when flag 0x01 is set"
    );

    // Verify forensic note is added when dirty
    let forensic_note = metadata.get("EVTX:ForensicNote");
    assert!(
        forensic_note.is_some(),
        "ForensicNote should be present when dirty flag is set"
    );

    // Verify full flag is NOT set
    assert_eq!(
        metadata.get("EVTX:IsFull"),
        Some(&TagValue::String("false".to_string())),
        "IsFull should be false when only dirty flag is set"
    );
}

/// Test: Full flag detection
///
/// Verifies that FLAG_FULL (0x02) is correctly detected and reported.
/// The full flag indicates the log reached its size limit and stopped recording,
/// meaning events may be missing - critical for log integrity assessment.
#[test]
fn test_evtx_full_flag() {
    let data = create_evtx_header(100, false, true);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Verify full flag is set (parser returns string "true")
    assert_eq!(
        metadata.get("EVTX:IsFull"),
        Some(&TagValue::String("true".to_string())),
        "IsFull should be true when flag 0x02 is set"
    );

    // Verify forensic warning is added when full
    let forensic_warning = metadata.get("EVTX:ForensicWarning");
    assert!(
        forensic_warning.is_some(),
        "ForensicWarning should be present when full flag is set"
    );

    // Verify dirty flag is NOT set
    assert_eq!(
        metadata.get("EVTX:IsDirty"),
        Some(&TagValue::String("false".to_string())),
        "IsDirty should be false when only full flag is set"
    );
}

/// Test: Both dirty and full flags
///
/// Verifies that when both flags are set (0x03), both are correctly detected.
/// This indicates a severe condition: improper shutdown AND missing events.
#[test]
fn test_evtx_both_flags() {
    let data = create_evtx_header(50, true, true);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Verify both flags are set
    assert_eq!(
        metadata.get("EVTX:IsDirty"),
        Some(&TagValue::String("true".to_string())),
        "IsDirty should be true"
    );

    assert_eq!(
        metadata.get("EVTX:IsFull"),
        Some(&TagValue::String("true".to_string())),
        "IsFull should be true"
    );

    // Verify both forensic indicators are present
    assert!(
        metadata.contains_key("EVTX:ForensicNote"),
        "ForensicNote should be present when dirty"
    );
    assert!(
        metadata.contains_key("EVTX:ForensicWarning"),
        "ForensicWarning should be present when full"
    );
}

/// Test: Version extraction
///
/// Verifies that major and minor version numbers are correctly extracted
/// and formatted. Version info helps determine Windows version and compatibility.
#[test]
fn test_evtx_version_extraction() {
    let data = create_evtx_header(1, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Verify version string format (Major.Minor)
    assert_eq!(
        metadata.get("EVTX:Version"),
        Some(&TagValue::String("3.1".to_string())),
        "Version should be formatted as Major.Minor"
    );

    // Verify major version extraction
    assert_eq!(
        metadata.get("EVTX:MajorVersion"),
        Some(&TagValue::String("3".to_string())),
        "MajorVersion should be 3"
    );

    // Verify minor version extraction
    assert_eq!(
        metadata.get("EVTX:MinorVersion"),
        Some(&TagValue::String("1".to_string())),
        "MinorVersion should be 1"
    );
}

/// Test: Chunk count variations
///
/// Verifies chunk count extraction with various values.
/// Chunk count indicates the number of 65536-byte chunks in the file,
/// useful for calculating file size and event density.
#[test]
fn test_evtx_chunk_count_variations() {
    // Test with 1 chunk
    let data1 = create_evtx_header(1, false, false);
    let reader1 = TestReader::new(data1);
    let metadata1 = parse_evtx_metadata(&reader1).expect("Failed to parse EVTX");
    assert_eq!(
        metadata1.get("EVTX:ChunkCount"),
        Some(&TagValue::String("1".to_string()))
    );

    // Test with 255 chunks (maximum for u16)
    let data255 = create_evtx_header(255, false, false);
    let reader255 = TestReader::new(data255);
    let metadata255 = parse_evtx_metadata(&reader255).expect("Failed to parse EVTX");
    assert_eq!(
        metadata255.get("EVTX:ChunkCount"),
        Some(&TagValue::String("255".to_string()))
    );

    // Test with 65535 chunks (maximum for u16)
    let data_max = create_evtx_header(65535, false, false);
    let reader_max = TestReader::new(data_max);
    let metadata_max = parse_evtx_metadata(&reader_max).expect("Failed to parse EVTX");
    assert_eq!(
        metadata_max.get("EVTX:ChunkCount"),
        Some(&TagValue::String("65535".to_string()))
    );
}

/// Test: Chunk number extraction (first and last)
///
/// Verifies that first and last chunk numbers are correctly extracted
/// from the file header.
#[test]
fn test_evtx_chunk_numbers() {
    let data = create_evtx_header(5, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // First chunk should be 0
    assert_eq!(
        metadata.get("EVTX:FirstChunk"),
        Some(&TagValue::String("0".to_string())),
        "FirstChunk should be 0"
    );

    // Last chunk should be chunk_count - 1
    assert_eq!(
        metadata.get("EVTX:LastChunk"),
        Some(&TagValue::String("4".to_string())),
        "LastChunk should be 4 for 5-chunk file"
    );
}

/// Test: No flags set (clean file)
///
/// Verifies that when no flags are set (0x00), the file is correctly
/// identified as clean with no forensic warnings.
#[test]
fn test_evtx_no_flags() {
    let data = create_evtx_header(3, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Both flags should be false
    assert_eq!(
        metadata.get("EVTX:IsDirty"),
        Some(&TagValue::String("false".to_string())),
        "IsDirty should be false"
    );

    assert_eq!(
        metadata.get("EVTX:IsFull"),
        Some(&TagValue::String("false".to_string())),
        "IsFull should be false"
    );

    // No forensic notes should be present
    assert!(
        !metadata.contains_key("EVTX:ForensicNote"),
        "ForensicNote should not be present for clean file"
    );

    assert!(
        !metadata.contains_key("EVTX:ForensicWarning"),
        "ForensicWarning should not be present for clean file"
    );
}

/// Test: Record ID extraction
///
/// Verifies that next record ID is correctly extracted from the header.
/// Record IDs are sequential identifiers for log entries.
#[test]
fn test_evtx_record_id_extraction() {
    let data = create_evtx_header(5, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Next record ID should be (5 * 100 + 1) = 501
    assert_eq!(
        metadata.get("EVTX:NextRecordID"),
        Some(&TagValue::String("501".to_string())),
        "NextRecordID should be calculated based on chunk count"
    );
}

/// Test: Checksum extraction
///
/// Verifies that the checksum field is correctly extracted and formatted.
/// Checksums help detect file corruption.
#[test]
fn test_evtx_checksum_extraction() {
    let data = create_evtx_header(1, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Checksum should be formatted as hex
    assert_eq!(
        metadata.get("EVTX:Checksum"),
        Some(&TagValue::String("0xDEADBEEF".to_string())),
        "Checksum should be in hex format"
    );
}

/// Test: Multiple forensic indicators together
///
/// Comprehensive test verifying all forensic-related metadata is
/// extracted and formatted correctly.
#[test]
fn test_evtx_comprehensive_forensic_data() {
    let data = create_evtx_header(42, true, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Verify file identification
    assert!(metadata.contains_key("FileType"));
    assert!(metadata.contains_key("FileSize"));

    // Verify version info
    assert_eq!(
        metadata.get("EVTX:Version"),
        Some(&TagValue::String("3.1".to_string()))
    );

    // Verify structural metadata
    assert!(metadata.contains_key("EVTX:HeaderSize"));
    assert!(metadata.contains_key("EVTX:HeaderBlockSize"));
    assert!(metadata.contains_key("EVTX:FirstChunk"));
    assert!(metadata.contains_key("EVTX:LastChunk"));
    assert!(metadata.contains_key("EVTX:ChunkCount"));

    // Verify record tracking
    assert!(metadata.contains_key("EVTX:NextRecordID"));

    // Verify flags and forensics
    assert_eq!(
        metadata.get("EVTX:IsDirty"),
        Some(&TagValue::String("true".to_string()))
    );
    assert!(metadata.contains_key("EVTX:ForensicNote"));

    // Verify checksum
    assert!(metadata.contains_key("EVTX:Checksum"));
}

/// Test: Minimal valid EVTX file
///
/// Verifies parser behavior with absolute minimum chunk count.
#[test]
fn test_evtx_minimal_file() {
    let data = create_evtx_header(1, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Should parse successfully with 1 chunk
    assert_eq!(
        metadata.get("EVTX:ChunkCount"),
        Some(&TagValue::String("1".to_string()))
    );

    // Last chunk should be 0 (first chunk is also last)
    assert_eq!(
        metadata.get("EVTX:LastChunk"),
        Some(&TagValue::String("0".to_string()))
    );
}

/// Test: Large EVTX file
///
/// Verifies parser behavior with large chunk counts.
#[test]
fn test_evtx_large_file() {
    let data = create_evtx_header(10000, true, true);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).expect("Failed to parse EVTX");

    // Should handle large chunk counts
    assert_eq!(
        metadata.get("EVTX:ChunkCount"),
        Some(&TagValue::String("10000".to_string()))
    );

    // Last chunk should be 9999
    assert_eq!(
        metadata.get("EVTX:LastChunk"),
        Some(&TagValue::String("9999".to_string()))
    );

    // Record ID should be calculated: 10000 * 100 + 1
    assert_eq!(
        metadata.get("EVTX:NextRecordID"),
        Some(&TagValue::String("1000001".to_string()))
    );
}

/// Test: Invalid EVTX file (missing signature)
///
/// Verifies that parser correctly rejects files without valid EVTX signature.
#[test]
fn test_evtx_invalid_signature() {
    let mut data = vec![0u8; 4096];
    // Use invalid signature
    data[0..8].copy_from_slice(b"Invalid\0");

    let reader = TestReader::new(data);
    let result = parse_evtx_metadata(&reader);

    // Should return error for invalid signature
    assert!(result.is_err(), "Should reject file with invalid signature");
}

/// Test: File too small for EVTX
///
/// Verifies that parser correctly rejects files smaller than minimum EVTX size.
#[test]
fn test_evtx_file_too_small() {
    let data = vec![0u8; 100]; // Less than 4096 bytes
    let reader = TestReader::new(data);
    let result = parse_evtx_metadata(&reader);

    // Should return error for file too small
    assert!(
        result.is_err(),
        "Should reject file smaller than EVTX header"
    );
}
