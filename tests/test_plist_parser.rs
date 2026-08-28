//! Integration tests for the PLIST parser's public surface.
//!
//! The parser mirrors ExifTool's `PLIST.pm`: tags are generated from the
//! `/`-joined key path (family-1 group `XML` for the XML encoding, `PLIST`
//! for binary), and only what ExifTool reports is emitted -- the fine-grained
//! decode pins live in the module's own unit tests against the pinned 13.59
//! oracle's `t/images` outputs.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::specialized::plist::{PlistParser, parse_plist_metadata};

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
fn test_parse_xml_plist_generates_exiftool_names() {
    let reader = TestReader::new(create_test_xml_plist());
    let metadata = parse_plist_metadata(&reader).expect("XML plist parses");

    // Undeclared keys get ExifTool's generated names under the XML group.
    assert_eq!(
        metadata.get("XML:CFBundleIdentifier"),
        Some(&TagValue::String("com.example.testapp".to_string()))
    );
    assert_eq!(
        metadata.get("XML:CFBundleName"),
        Some(&TagValue::String("TestApp".to_string()))
    );
    assert_eq!(
        metadata.get("XML:CFBundleVersion"),
        Some(&TagValue::String("1.2.3".to_string()))
    );
    // The old hand-rolled parser reported Plist:* tags ExifTool never emits.
    assert_eq!(metadata.get("Plist:Format"), None);
    assert_eq!(metadata.get("Plist:KeyCount"), None);
}

#[test]
fn test_parse_binary_plist_reports_runtime_mime_type() {
    // Minimal complete binary plist: the single object is an ASCII string.
    let mut data = b"bplist00".to_vec();
    data.extend_from_slice(&[0x52, b'h', b'i']);
    let table_offset = data.len() as u64;
    data.push(8);
    let mut trailer = vec![0u8; 32];
    trailer[6] = 1;
    trailer[7] = 1;
    trailer[8..16].copy_from_slice(&1u64.to_be_bytes());
    trailer[24..32].copy_from_slice(&table_offset.to_be_bytes());
    data.extend(trailer);

    let reader = TestReader::new(data);
    let metadata = parse_plist_metadata(&reader).expect("binary plist parses");
    assert_eq!(
        metadata.get("File:MIMEType"),
        Some(&TagValue::String("application/x-plist".to_string()))
    );
}

#[test]
fn test_verify_signature_accepts_both_encodings() {
    let mut binary = b"bplist00".to_vec();
    binary.extend(vec![0u8; 40]);
    assert!(PlistParser::verify_signature(&TestReader::new(binary)).unwrap());
    assert!(PlistParser::verify_signature(&TestReader::new(create_test_xml_plist())).unwrap());
}

#[test]
fn test_verify_signature_rejects_other_content() {
    assert!(!PlistParser::verify_signature(&TestReader::new(vec![0u8; 100])).unwrap());
    assert!(!PlistParser::verify_signature(&TestReader::new(vec![0u8; 4])).unwrap());
}

#[test]
fn test_parse_invalid_signature_is_an_error() {
    assert!(parse_plist_metadata(&TestReader::new(vec![0u8; 100])).is_err());
}
