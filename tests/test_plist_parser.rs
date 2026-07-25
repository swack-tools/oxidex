#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::specialized::plist::{PlistParser, parse_plist_metadata};

/// Creates a minimal valid binary plist for testing
fn create_test_binary_plist() -> Vec<u8> {
    let mut data = Vec::new();

    // Header: "bplist00"
    data.extend_from_slice(b"bplist00");

    // Simple object data (minimal - just padding for now)
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

/// Creates a minimal XML plist for testing
fn create_test_xml_plist() -> Vec<u8> {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.example.testapp</string>
    <key>CFBundleName</key>
    <string>TestApp</string>
    <key>CFBundleVersion</key>
    <string>1.2.3</string>
</dict>
</plist>"#;
    xml.as_bytes().to_vec()
}

#[test]
fn test_parse_binary_plist() {
    let data = create_test_binary_plist();
    let reader = TestReader::new(data);
    let result = parse_plist_metadata(&reader);

    assert!(result.is_ok(), "Failed to parse binary plist: {:?}", result);
    let metadata = result.unwrap();

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("Plist".to_string()))
    );
    assert_eq!(
        metadata.get("Plist:Format"),
        Some(&TagValue::String("Binary".to_string()))
    );
    assert_eq!(
        metadata.get("Plist:FormatVersion"),
        Some(&TagValue::String("00".to_string()))
    );
    assert!(metadata.contains_key("Plist:NumObjects"));
}

#[test]
fn test_parse_xml_plist() {
    let data = create_test_xml_plist();
    let reader = TestReader::new(data);
    let result = parse_plist_metadata(&reader);

    assert!(result.is_ok(), "Failed to parse XML plist: {:?}", result);
    let metadata = result.unwrap();

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("Plist".to_string()))
    );
    assert_eq!(
        metadata.get("Plist:Format"),
        Some(&TagValue::String("XML".to_string()))
    );
    assert_eq!(
        metadata.get("Plist:CFBundleIdentifier"),
        Some(&TagValue::String("com.example.testapp".to_string()))
    );
    assert_eq!(
        metadata.get("Plist:CFBundleName"),
        Some(&TagValue::String("TestApp".to_string()))
    );
}

#[test]
fn test_verify_binary_signature() {
    let data = create_test_binary_plist();
    let reader = TestReader::new(data);
    assert!(PlistParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_verify_xml_signature() {
    let data = create_test_xml_plist();
    let reader = TestReader::new(data);
    assert!(PlistParser::verify_signature(&reader).unwrap());
}
