//! Integration tests for the PLIST parser (ExifTool `PLIST.pm` semantics).
//!
//! Values, names and groups follow the pinned 13.59 oracle: XML plists emit
//! `XML:*` tags, binary plists emit `PLIST:*` tags, key paths join with `/`
//! and undeclared IDs get ExifTool's generated names. The byte-level pins
//! against the oracle's own `t/images` outputs (PLIST.aae, PLIST-xml.plist)
//! live in the module unit tests; these cover the parser through the public
//! entry point across each plist value type.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::specialized::plist::{PlistParser, parse_plist_metadata};

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

fn parse(content: &str) -> oxidex::core::MetadataMap {
    parse_plist_metadata(&TestReader::new(create_xml_plist(content))).expect("plist parses")
}

#[test]
fn test_string_value_extraction() {
    let metadata = parse("<dict><key>Author</key><string>Phil</string></dict>");
    assert_eq!(
        metadata.get("XML:Author"),
        Some(&TagValue::String("Phil".to_string()))
    );
}

#[test]
fn test_integer_value_extraction() {
    // ExifTool stores the XML text verbatim; the JSON writer's numeric
    // typing is a display concern, not a parsing one.
    let metadata = parse("<dict><key>Count</key><integer>256</integer></dict>");
    assert_eq!(
        metadata.get("XML:Count"),
        Some(&TagValue::String("256".to_string()))
    );
}

#[test]
fn test_boolean_value_extraction() {
    // `$val = ucfirst $prop` for the self-closing boolean elements.
    let metadata = parse("<dict><key>Yes</key><true/><key>No</key><false/></dict>");
    assert_eq!(
        metadata.get("XML:Yes"),
        Some(&TagValue::String("True".to_string()))
    );
    assert_eq!(
        metadata.get("XML:No"),
        Some(&TagValue::String("False".to_string()))
    );
}

#[test]
fn test_date_value_extraction() {
    // `ConvertXMPDate`: ISO 8601 to ExifTool's colon-separated form, the
    // zone designator preserved.
    let metadata = parse("<dict><key>When</key><date>2013-02-22T12:49:10Z</date></dict>");
    assert_eq!(
        metadata.get("XML:When"),
        Some(&TagValue::String("2013:02:22 12:49:10Z".to_string()))
    );
}

#[test]
fn test_data_value_extraction() {
    // Base64 data becomes a binary value (rendered by the writers as
    // ExifTool's "(Binary data N bytes...)" placeholder).
    let metadata = parse("<dict><key>Blob</key><data>VGhpcyBpcyBhIHRlc3Q=</data></dict>");
    assert_eq!(
        metadata.get("XML:Blob"),
        Some(&TagValue::Binary(b"This is a test".to_vec()))
    );
}

#[test]
fn test_array_extraction_joins_consecutive_values() {
    let metadata = parse(
        "<dict><key>Colors</key><array>\
         <string>red</string><string>green</string><string>blue</string>\
         </array></dict>",
    );
    assert_eq!(
        metadata.get("XML:Colors"),
        Some(&TagValue::Array(vec![
            TagValue::String("red".to_string()),
            TagValue::String("green".to_string()),
            TagValue::String("blue".to_string()),
        ]))
    );
}

#[test]
fn test_dictionary_extraction_builds_key_paths() {
    // Nested dict keys extend the tag ID path: `Outer/Inner` names the tag
    // `OuterInner`, exactly as ExifTool's generated `TestDictAuthor` etc.
    let metadata =
        parse("<dict><key>Outer</key><dict><key>Inner</key><string>x</string></dict></dict>");
    assert_eq!(
        metadata.get("XML:OuterInner"),
        Some(&TagValue::String("x".to_string()))
    );
}

#[test]
fn test_name_generation_capitalizes_and_strips() {
    // `s/([^A-Za-z])([a-z])/$1\u$2/g` + `tr/-_a-zA-Z0-9//dc` + ucfirst.
    let metadata = parse("<dict><key>lower camelCase</key><string>v</string></dict>");
    assert_eq!(
        metadata.get("XML:LowerCamelCase"),
        Some(&TagValue::String("v".to_string()))
    );
}

#[test]
fn test_entity_unescaping() {
    let metadata =
        parse("<dict><key>Escaped</key><string>a &lt;b&gt; &amp; &#x41;</string></dict>");
    assert_eq!(
        metadata.get("XML:Escaped"),
        Some(&TagValue::String("a <b> & A".to_string()))
    );
}

#[test]
fn test_binary_plist_dict_tags() {
    // bplist00 with { "Name": "Phil" }: objects are dict, key, value.
    let mut data = b"bplist00".to_vec();
    let dict_off = data.len() as u64; // 8
    data.extend_from_slice(&[0xD1, 0x01, 0x02]); // dict, 1 entry, refs 1,2
    let key_off = data.len() as u64; // 11
    data.extend_from_slice(&[0x54, b'N', b'a', b'm', b'e']); // "Name"
    let val_off = data.len() as u64; // 16
    data.extend_from_slice(&[0x54, b'P', b'h', b'i', b'l']); // "Phil"
    let table_off = data.len() as u64;
    data.extend_from_slice(&[dict_off as u8, key_off as u8, val_off as u8]);
    let mut trailer = vec![0u8; 32];
    trailer[6] = 1; // int size
    trailer[7] = 1; // ref size
    trailer[8..16].copy_from_slice(&3u64.to_be_bytes());
    trailer[16..24].copy_from_slice(&0u64.to_be_bytes());
    trailer[24..32].copy_from_slice(&table_off.to_be_bytes());
    data.extend(trailer);

    let metadata = parse_plist_metadata(&TestReader::new(data)).expect("binary plist parses");
    assert_eq!(
        metadata.get("PLIST:Name"),
        Some(&TagValue::String("Phil".to_string()))
    );
    assert_eq!(
        metadata.get("File:MIMEType"),
        Some(&TagValue::String("application/x-plist".to_string()))
    );
}

#[test]
fn test_invalid_plist_magic_detection() {
    assert!(parse_plist_metadata(&TestReader::new(b"not a plist at all".to_vec())).is_err());
}

#[test]
fn test_truncated_binary_plist_still_types_the_file() {
    // ExifTool sets the file type and MIME before reading the trailer; a
    // bad trailer is an error tag, not a failed read.
    let mut data = b"bplist00".to_vec();
    data.extend(vec![0u8; 64]); // nonsense body + trailer of zeros
    let metadata = parse_plist_metadata(&TestReader::new(data)).expect("read succeeds");
    assert_eq!(
        metadata.get("File:MIMEType"),
        Some(&TagValue::String("application/x-plist".to_string()))
    );
}

#[test]
fn test_verify_signature_requires_plist_root() {
    // XML without a plist root is not a plist.
    let xml = b"<?xml version=\"1.0\"?><notplist></notplist>".to_vec();
    assert!(!PlistParser::verify_signature(&TestReader::new(xml)).unwrap());
}
