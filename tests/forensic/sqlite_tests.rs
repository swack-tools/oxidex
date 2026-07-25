//! SQLite database parser integration tests
//!
//! Comprehensive tests for SQLite database file parsing covering header parsing,
//! metadata extraction, encoding detection, application identification, and forensic
//! indicators. Tests use synthetic SQLite headers to verify parser behavior.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::specialized::sqlite::{SQLiteParser, parse_sqlite_metadata};

/// SQLite magic signature: "SQLite format 3\0" (16 bytes)
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// SQLite header size (100 bytes)
const SQLITE_HEADER_SIZE: usize = 100;

/// Helper function to create a synthetic SQLite header with custom parameters
///
/// # Arguments
///
/// * `page_size` - Database page size in bytes (0 = 4096, 1 = 65536)
/// * `encoding` - Text encoding (1=UTF-8, 2=UTF-16LE, 3=UTF-16BE)
/// * `app_id` - Application ID (e.g., 0x42503331 for Firefox)
/// * `sqlite_version` - SQLite version number (e.g., 3040001 for 3.40.1)
fn create_sqlite_header(
    page_size: u16,
    encoding: u32,
    app_id: u32,
    sqlite_version: u32,
) -> Vec<u8> {
    let mut data = vec![0u8; SQLITE_HEADER_SIZE];

    // Magic header "SQLite format 3\0" (offset 0, 16 bytes)
    data[0..16].copy_from_slice(SQLITE_MAGIC);

    // Page size (offset 16, 2 bytes, big-endian)
    data[16..18].copy_from_slice(&page_size.to_be_bytes());

    // File format write version (offset 18, 1 byte)
    data[18] = 1;

    // File format read version (offset 19, 1 byte)
    data[19] = 1;

    // Reserved space (offset 20, 1 byte)
    data[20] = 0;

    // Maximum embedded payload fraction (offset 21, 1 byte)
    data[21] = 64;

    // Minimum embedded payload fraction (offset 22, 1 byte)
    data[22] = 32;

    // Leaf payload fraction (offset 23, 1 byte)
    data[23] = 32;

    // File change counter (offset 24, 4 bytes, big-endian)
    data[24..28].copy_from_slice(&100u32.to_be_bytes());

    // Database size in pages (offset 28, 4 bytes, big-endian)
    data[28..32].copy_from_slice(&250u32.to_be_bytes());

    // First freelist trunk page (offset 32, 4 bytes, big-endian)
    data[32..36].copy_from_slice(&0u32.to_be_bytes());

    // Total freelist pages (offset 36, 4 bytes, big-endian)
    data[36..40].copy_from_slice(&10u32.to_be_bytes());

    // Schema cookie (offset 40, 4 bytes, big-endian)
    data[40..44].copy_from_slice(&5u32.to_be_bytes());

    // Schema format number (offset 44, 4 bytes, big-endian)
    data[44..48].copy_from_slice(&4u32.to_be_bytes());

    // Default page cache size (offset 48, 4 bytes, big-endian)
    data[48..52].copy_from_slice(&0u32.to_be_bytes());

    // Largest root b-tree page (offset 52, 4 bytes, big-endian)
    data[52..56].copy_from_slice(&0u32.to_be_bytes());

    // Text encoding (offset 56, 4 bytes, big-endian)
    data[56..60].copy_from_slice(&encoding.to_be_bytes());

    // User version (offset 60, 4 bytes, big-endian)
    data[60..64].copy_from_slice(&42u32.to_be_bytes());

    // Incremental vacuum mode (offset 64, 4 bytes, big-endian)
    data[64..68].copy_from_slice(&0u32.to_be_bytes());

    // Application ID (offset 68, 4 bytes, big-endian)
    data[68..72].copy_from_slice(&app_id.to_be_bytes());

    // Reserved for expansion (offset 72, 20 bytes)
    // Left as zeros

    // Version valid for (offset 92, 4 bytes, big-endian)
    data[92..96].copy_from_slice(&100u32.to_be_bytes());

    // SQLite version number (offset 96, 4 bytes, big-endian)
    data[96..100].copy_from_slice(&sqlite_version.to_be_bytes());

    data
}

/// Test 1: SQLite header signature verification
///
/// Verifies that the parser correctly identifies valid SQLite files by
/// checking the "SQLite format 3\0" magic signature.
#[test]
fn test_sqlite_basic_header_parsing() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let result = SQLiteParser::verify_signature(&reader);
    assert!(result.is_ok(), "verify_signature should not error");
    assert!(
        result.unwrap(),
        "verify_signature should return true for valid SQLite header"
    );
}

/// Test 2: Page size extraction
///
/// Verifies correct extraction of database page size from offset 16.
/// Tests standard page size (4096 bytes).
#[test]
fn test_sqlite_page_size_extraction() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("PageSize"),
        Some(&TagValue::String("4096 bytes".to_string())),
        "Page size should be correctly extracted"
    );
}

/// Test 3: Special page size case (65536)
///
/// SQLite uses the special value 1 at offset 16 to represent a page size of 65536 bytes.
/// This tests that edge case.
#[test]
fn test_sqlite_page_size_special_case() {
    let data = create_sqlite_header(1, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("PageSize"),
        Some(&TagValue::String("65536 bytes".to_string())),
        "Page size of 1 should be interpreted as 65536 bytes"
    );
}

/// Test 4: File format version extraction
///
/// Verifies extraction of write and read format versions from the header.
#[test]
fn test_sqlite_file_format_version() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("WriteVersion"),
        Some(&TagValue::String("1".to_string())),
        "Write version should be 1"
    );
    assert_eq!(
        metadata.get("ReadVersion"),
        Some(&TagValue::String("1".to_string())),
        "Read version should be 1"
    );
}

/// Test 5: UTF-8 encoding detection
///
/// Verifies that databases with UTF-8 encoding (value 1) are correctly identified.
#[test]
fn test_sqlite_encoding_utf8() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("TextEncoding"),
        Some(&TagValue::String("UTF-8".to_string())),
        "Text encoding should be UTF-8"
    );
}

/// Test 6: UTF-16LE encoding detection
///
/// Verifies that databases with UTF-16 Little Endian encoding (value 2) are correctly identified.
#[test]
fn test_sqlite_encoding_utf16le() {
    let data = create_sqlite_header(4096, 2, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("TextEncoding"),
        Some(&TagValue::String("UTF-16le".to_string())),
        "Text encoding should be UTF-16le"
    );
}

/// Test 7: UTF-16BE encoding detection
///
/// Verifies that databases with UTF-16 Big Endian encoding (value 3) are correctly identified.
#[test]
fn test_sqlite_encoding_utf16be() {
    let data = create_sqlite_header(4096, 3, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("TextEncoding"),
        Some(&TagValue::String("UTF-16be".to_string())),
        "Text encoding should be UTF-16be"
    );
}

/// Test 8: Application ID extraction - Firefox
///
/// Verifies that the parser correctly identifies Firefox databases by their
/// application ID (0x42503331).
#[test]
fn test_sqlite_application_id_firefox() {
    let data = create_sqlite_header(4096, 1, 0x42503331, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("ApplicationID"),
        Some(&TagValue::String("0x42503331".to_string())),
        "Application ID should be correctly formatted"
    );
    assert_eq!(
        metadata.get("ApplicationName"),
        Some(&TagValue::String("Firefox".to_string())),
        "Application should be identified as Firefox"
    );
}

/// Test 9: Application ID extraction - Chrome
///
/// Verifies that the parser correctly identifies Chrome databases by their
/// application ID (0x42503332).
#[test]
fn test_sqlite_application_id_chrome() {
    let data = create_sqlite_header(4096, 1, 0x42503332, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("ApplicationID"),
        Some(&TagValue::String("0x42503332".to_string())),
        "Application ID should be correctly formatted"
    );
    assert_eq!(
        metadata.get("ApplicationName"),
        Some(&TagValue::String("Chrome".to_string())),
        "Application should be identified as Chrome"
    );
}

/// Test 10: Application ID extraction - iOS Messages
///
/// Verifies that the parser correctly identifies iOS Messages databases by their
/// application ID (0x54444233).
#[test]
fn test_sqlite_application_id_ios_messages() {
    let data = create_sqlite_header(4096, 1, 0x54444233, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("ApplicationID"),
        Some(&TagValue::String("0x54444233".to_string())),
        "Application ID should be correctly formatted"
    );
    assert_eq!(
        metadata.get("ApplicationName"),
        Some(&TagValue::String("iOS Messages".to_string())),
        "Application should be identified as iOS Messages"
    );
}

/// Test 11: Unknown application ID
///
/// Verifies that databases with unknown application IDs still have the ID extracted
/// but no application name is provided.
#[test]
fn test_sqlite_application_id_unknown() {
    let data = create_sqlite_header(4096, 1, 0x12345678, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("ApplicationID"),
        Some(&TagValue::String("0x12345678".to_string())),
        "Application ID should be correctly formatted"
    );
    assert!(
        !metadata.contains_key("ApplicationName"),
        "Unknown application ID should not have ApplicationName"
    );
}

/// Test 12: User version extraction
///
/// Verifies extraction of the user-defined version number from offset 60.
#[test]
fn test_sqlite_user_version_extraction() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("UserVersion"),
        Some(&TagValue::String("42".to_string())),
        "User version should be correctly extracted"
    );
}

/// Test 13: Freelist count extraction (forensic indicator)
///
/// Verifies extraction of free page count, which indicates potentially
/// recoverable deleted data.
#[test]
fn test_sqlite_freelist_count() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("FreePageCount"),
        Some(&TagValue::String("10".to_string())),
        "Free page count should be correctly extracted"
    );

    // Should have forensic note when free pages exist
    assert!(
        metadata.contains_key("ForensicNote"),
        "ForensicNote should be present when free pages exist"
    );
}

/// Test 14: SQLite version parsing
///
/// Verifies correct parsing and formatting of SQLite version number.
/// Version is encoded as major*1000000 + minor*1000 + patch.
#[test]
fn test_sqlite_version_parsing() {
    // Version 3.40.1 = 3*1000000 + 40*1000 + 1 = 3040001
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("SQLiteVersion"),
        Some(&TagValue::String("3.40.1".to_string())),
        "SQLite version should be formatted correctly"
    );
    assert_eq!(
        metadata.get("SQLiteVersionNumber"),
        Some(&TagValue::String("3040001".to_string())),
        "SQLite version number should be preserved"
    );
}

/// Test 15: Different SQLite versions
///
/// Verifies version parsing for different SQLite version numbers.
#[test]
fn test_sqlite_different_versions() {
    // Test version 3.35.5 = 3035005
    let data = create_sqlite_header(4096, 1, 0, 3035005);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("SQLiteVersion"),
        Some(&TagValue::String("3.35.5".to_string())),
        "SQLite version 3.35.5 should be formatted correctly"
    );
}

/// Test 16: Minimal truncated SQLite file
///
/// Verifies that files smaller than the required header size are rejected.
#[test]
fn test_sqlite_minimal_truncated_handling() {
    let data = vec![0u8; 50]; // Less than 100 bytes required
    let reader = TestReader::new(data);

    let result = SQLiteParser::verify_signature(&reader);
    assert!(result.is_ok(), "verify_signature should not error");
    assert!(
        !result.unwrap(),
        "verify_signature should return false for truncated file"
    );
}

/// Test 17: Invalid magic signature rejection
///
/// Verifies that files without the correct "SQLite format 3\0" signature
/// are rejected as invalid SQLite files.
#[test]
fn test_sqlite_invalid_magic_detection() {
    let mut data = vec![0u8; SQLITE_HEADER_SIZE];
    // Write invalid magic
    data[0..16].copy_from_slice(b"Invalid format\0\0");
    let reader = TestReader::new(data);

    let result = SQLiteParser::verify_signature(&reader);
    assert!(result.is_ok(), "verify_signature should not error");
    assert!(
        !result.unwrap(),
        "verify_signature should return false for invalid magic"
    );

    // Parsing should fail
    let parse_result = parse_sqlite_metadata(&reader);
    assert!(
        parse_result.is_err(),
        "parse_sqlite_metadata should fail for invalid signature"
    );
}

/// Test 18: Database size calculation
///
/// Verifies that the parser correctly calculates database size from
/// page count and page size.
#[test]
fn test_sqlite_database_size_calculation() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    // Page count is 250, page size is 4096
    // Expected size: 250 * 4096 = 1024000 bytes
    assert_eq!(
        metadata.get("DatabaseSize"),
        Some(&TagValue::String("1024000 bytes".to_string())),
        "Database size should be page_count * page_size"
    );
}

/// Test 19: Schema cookie extraction
///
/// Verifies extraction of schema cookie, which changes when the database schema is modified.
#[test]
fn test_sqlite_schema_cookie() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("SchemaCookie"),
        Some(&TagValue::String("5".to_string())),
        "Schema cookie should be correctly extracted"
    );
}

/// Test 20: Change counter extraction
///
/// Verifies extraction of change counter, which is incremented on each database modification.
/// Important for forensic timeline analysis.
#[test]
fn test_sqlite_change_counter() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("ChangeCounter"),
        Some(&TagValue::String("100".to_string())),
        "Change counter should be correctly extracted"
    );
}

/// Test 21: No forensic note when no free pages
///
/// Verifies that the forensic note is NOT added when there are no free pages.
#[test]
fn test_sqlite_no_free_pages() {
    let mut data = create_sqlite_header(4096, 1, 0, 3040001);
    // Set free page count to 0
    data[36..40].copy_from_slice(&0u32.to_be_bytes());
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("FreePageCount"),
        Some(&TagValue::String("0".to_string())),
        "Free page count should be 0"
    );
    assert!(
        !metadata.contains_key("ForensicNote"),
        "ForensicNote should not be present when no free pages exist"
    );
}

/// Test 22: Page count extraction
///
/// Verifies correct extraction of total database page count from offset 28.
#[test]
fn test_sqlite_page_count() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("PageCount"),
        Some(&TagValue::String("250".to_string())),
        "Page count should be correctly extracted"
    );
}

/// Test 23: Version valid for extraction
///
/// Verifies extraction of version-valid-for number from offset 92.
#[test]
fn test_sqlite_version_valid_for() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("VersionValidFor"),
        Some(&TagValue::String("100".to_string())),
        "VersionValidFor should be correctly extracted"
    );
}

/// Test 24: FileType metadata
///
/// Verifies that the parser always sets FileType to "SQLite".
#[test]
fn test_sqlite_file_type() {
    let data = create_sqlite_header(4096, 1, 0, 3040001);
    let reader = TestReader::new(data);

    let metadata = parse_sqlite_metadata(&reader).unwrap();

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("SQLite".to_string())),
        "FileType should be set to SQLite"
    );
}
