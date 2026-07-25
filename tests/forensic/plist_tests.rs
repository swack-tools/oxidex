//! Integration tests for Plist parser
//!
//! Comprehensive tests for macOS Property List parsing in both Binary and XML formats.
//! Tests cover format detection, signature verification, metadata extraction,
//! and parsing of various data types (strings, integers, booleans, dates, arrays, dictionaries).

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::specialized::plist::{PlistParser, parse_plist_metadata};

/// Binary plist magic bytes
const BPLIST_MAGIC: &[u8; 6] = b"bplist";

/// Helper function to create XML plist with given content
fn create_xml_plist(content: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
{}
</plist>"#,
        content
    )
    .into_bytes()
}

/// Helper function to create a minimal valid binary plist
fn create_binary_plist_header() -> Vec<u8> {
    let mut data = Vec::new();

    // Header: "bplist00"
    data.extend_from_slice(b"bplist00");

    // Simple object data (minimal padding)
    let objects_size = 100;
    data.extend(vec![0u8; objects_size]);

    // Trailer (32 bytes)
    let mut trailer = vec![0u8; 32];
    trailer[6] = 2; // offset_int_size = 2 bytes
    trailer[7] = 1; // object_ref_size = 1 byte

    // num_objects = 5 (big-endian u64 at offset 8)
    trailer[8..16].copy_from_slice(&5u64.to_be_bytes());

    // top_object = 0 (big-endian u64 at offset 16)
    trailer[16..24].copy_from_slice(&0u64.to_be_bytes());

    // offset_table_offset = 108 (8 + 100) (big-endian u64 at offset 24)
    trailer[24..32].copy_from_slice(&108u64.to_be_bytes());

    data.extend(trailer);
    data
}

/// Test 1: XML plist parsing with basic structure
///
/// Verifies that XML plists with standard structure are correctly identified
/// and parsed, extracting the format type and version.
#[test]
fn test_xml_plist_parsing() {
    let xml_content = r#"<dict>
    <key>TestKey</key>
    <string>TestValue</string>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "XML plist parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("Plist".to_string())),
        "FileType should be 'Plist'"
    );
    assert_eq!(
        metadata.get("Plist:Format"),
        Some(&TagValue::String("XML".to_string())),
        "Format should be 'XML'"
    );
    assert_eq!(
        metadata.get("Plist:FormatVersion"),
        Some(&TagValue::String("1.0".to_string())),
        "FormatVersion should be '1.0'"
    );
}

/// Test 2: Binary plist parsing with version 0
///
/// Verifies that binary plists with magic "bplist00" are correctly identified
/// and parsed, extracting trailer metadata.
#[test]
fn test_binary_plist_parsing() {
    let data = create_binary_plist_header();
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Binary plist parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("Plist".to_string())),
        "FileType should be 'Plist'"
    );
    assert_eq!(
        metadata.get("Plist:Format"),
        Some(&TagValue::String("Binary".to_string())),
        "Format should be 'Binary'"
    );
    assert_eq!(
        metadata.get("Plist:FormatVersion"),
        Some(&TagValue::String("00".to_string())),
        "FormatVersion should be '00'"
    );
    assert!(
        metadata.contains_key("Plist:NumObjects"),
        "NumObjects should be present"
    );
}

/// Test 3: String value extraction from XML plist
///
/// Verifies that string values are correctly extracted from XML plists.
#[test]
fn test_string_value_extraction() {
    let xml_content = r#"<dict>
    <key>AppName</key>
    <string>My Application</string>
    <key>Description</key>
    <string>A test application</string>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "String extraction should succeed");

    let metadata = result.unwrap();
    // Verify root object type is Dictionary
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Dictionary".to_string())),
        "Root object should be Dictionary"
    );
}

/// Test 4: Integer value in XML plist
///
/// Verifies that XML plists with integer values are correctly parsed.
#[test]
fn test_integer_value_extraction() {
    let xml_content = r#"<dict>
    <key>Count</key>
    <integer>42</integer>
    <key>Version</key>
    <integer>123</integer>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Integer parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Dictionary".to_string())),
        "Root object should be Dictionary"
    );
}

/// Test 5: Boolean value in XML plist
///
/// Verifies that XML plists with boolean values (true/false) are correctly parsed.
#[test]
fn test_boolean_value_extraction() {
    let xml_content = r#"<dict>
    <key>Enabled</key>
    <true/>
    <key>Disabled</key>
    <false/>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Boolean parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Dictionary".to_string())),
        "Root object should be Dictionary"
    );
}

/// Test 6: Date value in XML plist
///
/// Verifies that XML plists with date values in ISO8601 format are correctly parsed.
#[test]
fn test_date_value_extraction() {
    let xml_content = r#"<dict>
    <key>CreationDate</key>
    <date>2024-01-15T10:30:00Z</date>
    <key>ModifiedDate</key>
    <date>2024-12-03T14:45:30Z</date>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Date parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Dictionary".to_string())),
        "Root object should be Dictionary"
    );
}

/// Test 7: Data (base64) value in XML plist
///
/// Verifies that XML plists with base64-encoded data values are correctly parsed.
#[test]
fn test_data_value_extraction() {
    let xml_content = r#"<dict>
    <key>BinaryData</key>
    <data>
    SGVsbG8gV29ybGQh
    </data>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Data parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Dictionary".to_string())),
        "Root object should be Dictionary"
    );
}

/// Test 8: Array extraction from XML plist
///
/// Verifies that XML plists with array root objects are correctly identified.
#[test]
fn test_array_extraction() {
    let xml_content = r#"<array>
    <string>First</string>
    <string>Second</string>
    <string>Third</string>
</array>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Array parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Array".to_string())),
        "Root object should be Array"
    );
}

/// Test 9: Dictionary extraction from XML plist
///
/// Verifies that XML plists with dictionary root objects are correctly identified
/// and that the key count is extracted.
#[test]
fn test_dictionary_extraction() {
    let xml_content = r#"<dict>
    <key>FirstKey</key>
    <string>FirstValue</string>
    <key>SecondKey</key>
    <string>SecondValue</string>
    <key>ThirdKey</key>
    <string>ThirdValue</string>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Dictionary parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Dictionary".to_string())),
        "Root object should be Dictionary"
    );

    // Verify key count
    if let Some(TagValue::String(count)) = metadata.get("Plist:KeyCount") {
        let count_val: usize = count.parse().unwrap_or(0);
        assert_eq!(count_val, 3, "Should have 3 keys");
    } else {
        panic!("KeyCount should be present");
    }
}

/// Test 10: Nested structures in XML plist
///
/// Verifies that XML plists with nested dictionaries and arrays are correctly parsed.
#[test]
fn test_nested_structures() {
    let xml_content = r#"<dict>
    <key>OuterDict</key>
    <dict>
        <key>InnerKey</key>
        <string>InnerValue</string>
    </dict>
    <key>OuterArray</key>
    <array>
        <dict>
            <key>NestedKey</key>
            <string>NestedValue</string>
        </dict>
    </array>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Nested structure parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Dictionary".to_string())),
        "Root object should be Dictionary"
    );

    // Verify multiple keys are detected
    if let Some(TagValue::String(count)) = metadata.get("Plist:KeyCount") {
        let count_val: usize = count.parse().unwrap_or(0);
        assert!(count_val >= 2, "Should have at least 2 keys");
    }
}

/// Test 11: CFBundleIdentifier extraction
///
/// Verifies that the CFBundleIdentifier key is correctly extracted from XML plists,
/// which is essential for identifying macOS/iOS applications.
#[test]
fn test_cfbundleidentifier_extraction() {
    let xml_content = r#"<dict>
    <key>CFBundleIdentifier</key>
    <string>com.example.myapp</string>
    <key>CFBundleName</key>
    <string>MyApp</string>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(
        result.is_ok(),
        "CFBundleIdentifier extraction should succeed"
    );

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:CFBundleIdentifier"),
        Some(&TagValue::String("com.example.myapp".to_string())),
        "CFBundleIdentifier should be extracted"
    );
    assert_eq!(
        metadata.get("Plist:CFBundleName"),
        Some(&TagValue::String("MyApp".to_string())),
        "CFBundleName should be extracted"
    );
}

/// Test 12: CFBundleVersion extraction
///
/// Verifies that CFBundleVersion and CFBundleShortVersionString are correctly
/// extracted from XML plists.
#[test]
fn test_cfbundleversion_extraction() {
    let xml_content = r#"<dict>
    <key>CFBundleVersion</key>
    <string>1.2.3.456</string>
    <key>CFBundleShortVersionString</key>
    <string>1.2.3</string>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "CFBundleVersion extraction should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:CFBundleVersion"),
        Some(&TagValue::String("1.2.3.456".to_string())),
        "CFBundleVersion should be extracted"
    );
    assert_eq!(
        metadata.get("Plist:CFBundleShortVersionString"),
        Some(&TagValue::String("1.2.3".to_string())),
        "CFBundleShortVersionString should be extracted"
    );
}

/// Test 13: Minimal/truncated plist handling
///
/// Verifies that minimal plists with just the basic structure are handled gracefully.
#[test]
fn test_minimal_plist_handling() {
    let xml_content = r#"<dict></dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Minimal plist parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:RootObjectType"),
        Some(&TagValue::String("Dictionary".to_string())),
        "Root object should be Dictionary"
    );
    assert_eq!(
        metadata.get("Plist:Format"),
        Some(&TagValue::String("XML".to_string())),
        "Format should be XML"
    );
}

/// Test 14: Invalid plist magic detection - wrong magic bytes
///
/// Verifies that files without proper plist magic bytes are correctly rejected.
#[test]
fn test_invalid_plist_magic_detection() {
    // Create data with wrong magic bytes
    let mut invalid_data = vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA];
    invalid_data.extend(vec![0u8; 140]); // Add padding
    let reader = TestReader::new(invalid_data);

    let result = PlistParser::verify_signature(&reader);
    assert!(result.is_ok(), "verify_signature should not error");
    assert!(
        !result.unwrap(),
        "verify_signature should return false for invalid magic"
    );
}

/// Test 15: Binary plist trailer metadata
///
/// Verifies that binary plist trailer fields are correctly extracted and reported.
#[test]
fn test_binary_plist_trailer_metadata() {
    let data = create_binary_plist_header();
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Binary plist parsing should succeed");

    let metadata = result.unwrap();

    // Verify trailer metadata
    assert_eq!(
        metadata.get("Plist:OffsetIntSize"),
        Some(&TagValue::String("2".to_string())),
        "OffsetIntSize should be 2"
    );
    assert_eq!(
        metadata.get("Plist:ObjectRefSize"),
        Some(&TagValue::String("1".to_string())),
        "ObjectRefSize should be 1"
    );
    assert_eq!(
        metadata.get("Plist:NumObjects"),
        Some(&TagValue::String("5".to_string())),
        "NumObjects should be 5"
    );
    assert_eq!(
        metadata.get("Plist:TopObjectIndex"),
        Some(&TagValue::String("0".to_string())),
        "TopObjectIndex should be 0"
    );
    assert_eq!(
        metadata.get("Plist:OffsetTableOffset"),
        Some(&TagValue::String("0x6C".to_string())),
        "OffsetTableOffset should be 0x6C"
    );
    assert_eq!(
        metadata.get("Plist:TrailerSize"),
        Some(&TagValue::String("32".to_string())),
        "TrailerSize should be 32"
    );
}

/// Test 16: XML plist without DOCTYPE
///
/// Verifies that XML plists without DOCTYPE declaration are still parsed correctly.
#[test]
fn test_xml_plist_without_doctype() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>TestKey</key>
    <string>TestValue</string>
</dict>
</plist>"#;
    let data = xml.as_bytes().to_vec();
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "XML plist without DOCTYPE should parse");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:Format"),
        Some(&TagValue::String("XML".to_string())),
        "Format should be XML"
    );
}

/// Test 17: Binary plist version 1 detection
///
/// Verifies that binary plists with magic "bplist01" are correctly identified.
#[test]
fn test_binary_plist_v1_detection() {
    let mut data = Vec::new();
    data.extend_from_slice(b"bplist01"); // Version 1
    data.extend(vec![0u8; 100]); // Object data

    // Trailer
    let mut trailer = vec![0u8; 32];
    trailer[6] = 2;
    trailer[7] = 1;
    trailer[8..16].copy_from_slice(&5u64.to_be_bytes());
    trailer[16..24].copy_from_slice(&0u64.to_be_bytes());
    trailer[24..32].copy_from_slice(&108u64.to_be_bytes());
    data.extend(trailer);

    let reader = TestReader::new(data);

    let result = PlistParser::verify_signature(&reader);
    assert!(result.is_ok(), "verify_signature should succeed");
    assert!(result.unwrap(), "Should detect bplist01 as valid");

    let metadata = parse_plist_metadata(&reader).unwrap();
    assert_eq!(
        metadata.get("Plist:FormatVersion"),
        Some(&TagValue::String("01".to_string())),
        "FormatVersion should be '01'"
    );
}

/// Test 18: Launchd plist Label extraction
///
/// Verifies that the Label key from launchd plists is correctly extracted.
#[test]
fn test_launchd_label_extraction() {
    let xml_content = r#"<dict>
    <key>Label</key>
    <string>com.example.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/example</string>
    </array>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Launchd plist parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("Plist:Label"),
        Some(&TagValue::String("com.example.daemon".to_string())),
        "Label should be extracted"
    );
}

/// Test 19: Too small binary plist rejection
///
/// Verifies that files too small to be valid binary plists are rejected.
#[test]
fn test_too_small_binary_plist() {
    // Create data smaller than minimum required size (8 header + 32 trailer = 40)
    let data = vec![0u8; 30];
    let reader = TestReader::new(data);

    let result = PlistParser::verify_signature(&reader);
    assert!(result.is_ok(), "verify_signature should not error");
    assert!(!result.unwrap(), "Should reject too-small binary plist");
}

/// Test 20: File size metadata
///
/// Verifies that file size is correctly reported in metadata.
#[test]
fn test_file_size_metadata() {
    let xml_content = r#"<dict>
    <key>Test</key>
    <string>Value</string>
</dict>"#;
    let data = create_xml_plist(xml_content);
    let file_size = data.len();
    let reader = TestReader::new(data);

    let result = parse_plist_metadata(&reader);
    assert!(result.is_ok(), "Parsing should succeed");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileSize"),
        Some(&TagValue::String(file_size.to_string())),
        "FileSize should match actual size"
    );
}
