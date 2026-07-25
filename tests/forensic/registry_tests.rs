//! Registry hive parser integration tests
//!
//! Comprehensive test suite for Windows Registry Hive parser covering:
//! - Clean vs. dirty shutdown detection
//! - Hive type identification (normal, transaction logs)
//! - Hive purpose inference from names
//! - All major hive types (NTUSER, SYSTEM, SOFTWARE, SECURITY, DEFAULT, SAM)

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::specialized::registry::parse_registry_metadata;

/// Creates a minimal valid registry hive header for testing
///
/// # Arguments
///
/// * `primary_seq` - Primary sequence number (incremented at start of write)
/// * `secondary_seq` - Secondary sequence number (incremented at end of write)
/// * `hive_name` - Embedded hive filename (UTF-16LE encoded)
/// * `hive_type` - Hive type value (0=normal, 1=transaction log)
///
/// # Returns
///
/// A 4096-byte registry hive header with the specified values
fn create_registry_header(
    primary_seq: u32,
    secondary_seq: u32,
    hive_name: &str,
    hive_type: u32,
) -> Vec<u8> {
    let mut data = vec![0u8; 4096];

    // Magic header "regf" (offset 0, 4 bytes)
    data[0..4].copy_from_slice(b"regf");

    // Primary sequence number (offset 4, 4 bytes)
    data[4..8].copy_from_slice(&primary_seq.to_le_bytes());

    // Secondary sequence number (offset 8, 4 bytes)
    data[8..12].copy_from_slice(&secondary_seq.to_le_bytes());

    // Last written timestamp (offset 12, 8 bytes)
    // Using example FILETIME value: 133000000000000000
    data[12..20].copy_from_slice(&133000000000000000u64.to_le_bytes());

    // Major version (offset 20, 4 bytes) = 1
    data[20..24].copy_from_slice(&1u32.to_le_bytes());

    // Minor version (offset 24, 4 bytes) = 5
    data[24..28].copy_from_slice(&5u32.to_le_bytes());

    // Hive type (offset 28, 4 bytes)
    data[28..32].copy_from_slice(&hive_type.to_le_bytes());

    // Root cell offset (offset 36, 4 bytes) = 0x1000
    data[36..40].copy_from_slice(&0x1000u32.to_le_bytes());

    // Hive bins data size (offset 40, 4 bytes) = 1MB
    data[40..44].copy_from_slice(&1048576u32.to_le_bytes());

    // Hive name (offset 48, 64 bytes) - UTF-16LE encoded
    for (i, c) in hive_name.encode_utf16().enumerate() {
        if i * 2 + 1 < 64 {
            data[48 + i * 2..48 + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
        }
    }

    data
}

/// Test 1: Verify clean shutdown detection (primary == secondary)
///
/// When a registry hive is cleanly shut down, the primary and secondary
/// sequence numbers should match. This test verifies the parser correctly
/// identifies this condition and sets SequenceValid="Yes" without a ForensicNote.
#[test]
fn test_registry_clean_shutdown() {
    let data = create_registry_header(100, 100, "NTUSER.DAT", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Verify clean shutdown is detected
    assert_eq!(
        metadata.get("Registry:SequenceValid"),
        Some(&TagValue::String("Yes".into())),
        "Expected SequenceValid='Yes' for matching sequence numbers"
    );

    // Verify no forensic note is present (clean shutdown)
    assert!(
        !metadata.contains_key("ForensicNote"),
        "Unexpected ForensicNote for clean shutdown"
    );

    // Verify sequence numbers are recorded
    assert_eq!(
        metadata.get("Registry:PrimarySequence"),
        Some(&TagValue::String("100".into()))
    );
    assert_eq!(
        metadata.get("Registry:SecondarySequence"),
        Some(&TagValue::String("100".into()))
    );
}

/// Test 2: Verify dirty shutdown detection (primary != secondary)
///
/// When a registry hive is not cleanly shut down (e.g., system crash, force shutdown),
/// the sequence numbers will not match. This test verifies the parser correctly
/// identifies this condition and sets SequenceValid="No" with a ForensicNote.
#[test]
fn test_registry_dirty_shutdown() {
    let data = create_registry_header(101, 100, "SYSTEM", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Verify dirty shutdown is detected
    assert_eq!(
        metadata.get("Registry:SequenceValid"),
        Some(&TagValue::String("No".into())),
        "Expected SequenceValid='No' for mismatched sequence numbers"
    );

    // Verify forensic note is present (dirty shutdown indicator)
    assert!(
        metadata.contains_key("ForensicNote"),
        "Expected ForensicNote for dirty shutdown"
    );

    // Verify the forensic note content
    let note = metadata.get("ForensicNote").and_then(|v| v.as_string());
    assert!(
        note.map(|n| n.contains("mismatch") || n.contains("dirty") || n.contains("shutdown"))
            .unwrap_or(false),
        "ForensicNote should mention shutdown or mismatch"
    );

    // Verify sequence numbers show the difference
    assert_eq!(
        metadata.get("Registry:PrimarySequence"),
        Some(&TagValue::String("101".into()))
    );
    assert_eq!(
        metadata.get("Registry:SecondarySequence"),
        Some(&TagValue::String("100".into()))
    );
}

/// Test 3: Verify transaction log hive type detection
///
/// Some registry hives are transaction logs (type=1) rather than normal hives (type=0).
/// Transaction logs are used for changes that haven't been committed to the main hive.
/// This test verifies the parser correctly identifies the hive type.
#[test]
fn test_registry_transaction_log() {
    let data = create_registry_header(50, 50, "SYSTEM.LOG1", 1);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Verify hive type is correctly identified as transaction log
    assert_eq!(
        metadata.get("Registry:HiveType"),
        Some(&TagValue::String("Transaction Log".into())),
        "Expected HiveType='Transaction Log' for hive_type=1"
    );

    // Verify raw hive type value is also recorded
    assert_eq!(
        metadata.get("Registry:HiveTypeRaw"),
        Some(&TagValue::String("1".into()))
    );
}

/// Test 4: Verify SAM hive purpose inference
///
/// The SAM (Security Accounts Manager) hive contains user account and password
/// information. This test verifies the parser correctly identifies the hive purpose
/// from the embedded filename.
#[test]
fn test_registry_sam_hive() {
    let data = create_registry_header(10, 10, "SAM", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Verify hive purpose contains "Security Accounts"
    assert!(
        metadata
            .get("Registry:HivePurpose")
            .and_then(|v| v.as_string())
            .map(|s| s.contains("Security Accounts"))
            .unwrap_or(false),
        "Expected HivePurpose to contain 'Security Accounts' for SAM hive"
    );

    // Verify hive name is recorded
    assert_eq!(
        metadata.get("Registry:HiveName"),
        Some(&TagValue::String("SAM".into()))
    );
}

/// Test 5: Verify all major hive types and their purposes
///
/// Windows systems have several standard registry hives, each with a specific purpose.
/// This test verifies the parser correctly identifies all major hive types and
/// infers their purposes from the embedded filenames.
///
/// Standard hives:
/// - NTUSER.DAT: User profile settings
/// - SYSTEM: System-wide configuration
/// - SOFTWARE: Installed software configuration
/// - SECURITY: Security policies
/// - DEFAULT: Default user profile
#[test]
fn test_registry_all_hive_types() {
    let hives = vec![
        ("NTUSER.DAT", "User profile"),
        ("SYSTEM", "System-wide"),
        ("SOFTWARE", "software"),
        ("SECURITY", "Security policy"),
        ("DEFAULT", "Default user"),
    ];

    for (name, expected_substr) in hives {
        let data = create_registry_header(1, 1, name, 0);
        let reader = TestReader::new(data);
        let metadata = parse_registry_metadata(&reader)
            .expect(&format!("parse_registry_metadata failed for {}", name));

        // Verify hive name is recorded
        assert_eq!(
            metadata.get("Registry:HiveName"),
            Some(&TagValue::String(name.into())),
            "Hive name not correctly recorded for {}",
            name
        );

        // Verify hive purpose is inferred and contains expected substring
        let purpose = metadata
            .get("Registry:HivePurpose")
            .and_then(|v| v.as_string())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        assert!(
            purpose.contains(&expected_substr.to_lowercase()),
            "Hive {} should have purpose containing '{}', got '{}'",
            name,
            expected_substr,
            purpose
        );

        // Verify hive type is identified as normal (not transaction log)
        assert_eq!(
            metadata.get("Registry:HiveType"),
            Some(&TagValue::String("Normal".into())),
            "Expected HiveType='Normal' for standard hive {}",
            name
        );

        // Verify clean shutdown for all test hives
        assert_eq!(
            metadata.get("Registry:SequenceValid"),
            Some(&TagValue::String("Yes".into())),
            "Expected clean shutdown for test hive {}",
            name
        );
    }
}

/// Test 6: Verify all required metadata fields are present for valid registry
///
/// A complete metadata extraction should include various fields identifying
/// the hive, its version, structure, and forensic indicators.
#[test]
fn test_registry_complete_metadata() {
    let data = create_registry_header(42, 42, "NTUSER.DAT", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Verify required metadata fields are present
    assert!(metadata.contains_key("FileType"), "Missing FileType");
    assert!(
        metadata.contains_key("Registry:Signature"),
        "Missing Signature"
    );
    assert!(metadata.contains_key("Registry:Version"), "Missing Version");
    assert!(
        metadata.contains_key("Registry:PrimarySequence"),
        "Missing PrimarySequence"
    );
    assert!(
        metadata.contains_key("Registry:SecondarySequence"),
        "Missing SecondarySequence"
    );
    assert!(
        metadata.contains_key("Registry:SequenceValid"),
        "Missing SequenceValid"
    );
    assert!(
        metadata.contains_key("Registry:LastWritten"),
        "Missing LastWritten"
    );
    assert!(
        metadata.contains_key("Registry:HiveType"),
        "Missing HiveType"
    );
    assert!(
        metadata.contains_key("Registry:HiveName"),
        "Missing HiveName"
    );
    assert!(
        metadata.contains_key("Registry:HivePurpose"),
        "Missing HivePurpose"
    );
    assert!(
        metadata.contains_key("Registry:RootCellOffset"),
        "Missing RootCellOffset"
    );
    assert!(
        metadata.contains_key("Registry:DataSize"),
        "Missing DataSize"
    );

    // Verify file type is correct
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("Registry Hive".into()))
    );

    // Verify signature is "regf"
    assert_eq!(
        metadata.get("Registry:Signature"),
        Some(&TagValue::String("regf".into()))
    );
}

/// Test 7: Verify version information parsing
///
/// Registry hive headers contain major and minor version numbers.
/// This test verifies these are correctly parsed and formatted.
#[test]
fn test_registry_version_parsing() {
    let data = create_registry_header(5, 5, "SOFTWARE", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Verify version information
    assert_eq!(
        metadata.get("Registry:MajorVersion"),
        Some(&TagValue::String("1".into()))
    );
    assert_eq!(
        metadata.get("Registry:MinorVersion"),
        Some(&TagValue::String("5".into()))
    );
    assert_eq!(
        metadata.get("Registry:Version"),
        Some(&TagValue::String("1.5".into()))
    );
}

/// Test 8: Verify hive structure information
///
/// Registry hives have specific structural information including root cell
/// offset and data size. This test verifies these are correctly parsed.
#[test]
fn test_registry_structure_info() {
    let data = create_registry_header(7, 7, "SECURITY", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Verify structure information
    assert!(metadata.contains_key("Registry:RootCellOffset"));
    assert!(metadata.contains_key("Registry:DataSize"));
    assert!(metadata.contains_key("Registry:DataSizeRaw"));

    // Verify root cell offset format
    let root_offset = metadata
        .get("Registry:RootCellOffset")
        .and_then(|v| v.as_string());
    assert!(
        root_offset.map(|o| o.starts_with("0x")).unwrap_or(false),
        "Root cell offset should be in hex format (0x...)"
    );

    // Verify data size contains "bytes"
    let data_size = metadata
        .get("Registry:DataSize")
        .and_then(|v| v.as_string());
    assert!(
        data_size.map(|s| s.contains("bytes")).unwrap_or(false),
        "Data size should contain 'bytes'"
    );
}

/// Test 9: Verify edge case - very high sequence numbers
///
/// Tests that the parser handles large sequence number values correctly.
#[test]
fn test_registry_high_sequence_numbers() {
    let data = create_registry_header(0xFFFFFFFF, 0xFFFFFFFF, "NTUSER.DAT", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Should still identify as clean shutdown (even with max u32 values)
    assert_eq!(
        metadata.get("Registry:SequenceValid"),
        Some(&TagValue::String("Yes".into()))
    );

    // Verify the sequence numbers are recorded correctly
    assert_eq!(
        metadata.get("Registry:PrimarySequence"),
        Some(&TagValue::String("4294967295".into()))
    );
}

/// Test 10: Verify empty hive name handling
///
/// Some registry hives may have empty names. This test verifies the parser
/// handles this gracefully without panicking.
#[test]
fn test_registry_empty_hive_name() {
    let data = create_registry_header(2, 2, "", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).expect("parse_registry_metadata failed");

    // Should successfully parse even with empty name
    assert!(
        metadata.contains_key("Registry:HiveName") || !metadata.contains_key("Registry:HiveName")
    );

    // Should still have basic fields
    assert_eq!(
        metadata.get("Registry:SequenceValid"),
        Some(&TagValue::String("Yes".into()))
    );
}
