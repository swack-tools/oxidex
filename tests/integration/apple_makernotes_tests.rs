//! Integration tests for Apple (iPhone/iPad) MakerNotes parser
//!
//! This file was undeclared to Cargo and had never compiled. Eleven of its
//! eighteen tests asserted tag names that do not exist in ExifTool's Apple.pm:
//! `Apple:FacingCamera`, `Apple:LensModel`, `Apple:NightMode`,
//! `Apple:PortraitMode` and `Apple:SceneDetection` (0 hits each across all of
//! ExifTool 13.59). Those are deleted rather than "fixed", because there is no
//! real tag to point them at.
//!
//! Two more asserted `Apple:SemanticStyle` as a SHORT enum at tag 0x2E. That
//! is wrong twice over: ExifTool puts SemanticStyle at 0x0040 and decodes it
//! with `ConvertPLIST` (it is a property list, not an enum), and 0x002e is
//! CameraType. Deleted for the same reason.
//!
//! What survives is checked against ExifTool 13.59 Apple.pm by tag id and by
//! PrintConv value.

use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::apple::AppleParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

/// Builds a little-endian IFD body: entry count followed by 12-byte entries
/// with the value inline.
fn ifd(entries: &[(u16, u16, u32)]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, format, value) in entries {
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&format.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes()); // Count: 1
        data.extend_from_slice(&value.to_le_bytes());
    }
    data
}

const SHORT: u16 = 3;
/// TIFF format 9, `int32s` -- the type ExifTool declares for AETarget.
const SLONG: u16 = 9;

#[test]
fn test_apple_parser_trait() {
    let parser = AppleParser::new();
    assert_eq!(parser.manufacturer_name(), "Apple");
    assert_eq!(parser.tag_prefix(), "Apple:");
}

#[test]
fn test_apple_validate_header_with_signature() {
    let parser = AppleParser::new();
    let mut data = Vec::new();
    data.extend_from_slice(b"Apple iOS");
    data.extend_from_slice(&[0x00]); // Padding
    data.extend_from_slice(&[0x05, 0x00]); // 5 entries

    assert!(parser.validate_header(&data));
}

#[test]
fn test_apple_validate_header_without_signature() {
    let parser = AppleParser::new();
    let data = vec![0x05, 0x00]; // Just entry count

    assert!(parser.validate_header(&data));
}

// ============================================================================
// HDRImageType -- Apple.pm 0x000a
//
// ExifTool 13.59 declares exactly two values:
//     3 => 'HDR Image'
//     4 => 'Original Image'
// Its `2` is commented out as unidentified (iPad mini 2).
//
// These tests previously asserted 0 => "Off" and 4 => "Smart HDR" and would
// have *passed*, because the decoder invented nine values matching them.
// ExifTool prints "Original Image" for 4. See the fix to DECODE_HDR_TYPE.
// ============================================================================

#[test]
fn test_apple_hdr_image_type_hdr_image() {
    let parser = AppleParser::new();
    let data = ifd(&[(0x000A, SHORT, 3)]);

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Apple:HDRImageType"),
        Some(&"HDR Image".to_string())
    );
}

#[test]
fn test_apple_hdr_image_type_original_image() {
    let parser = AppleParser::new();
    let data = ifd(&[(0x000A, SHORT, 4)]);

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Apple:HDRImageType"),
        Some(&"Original Image".to_string())
    );
}

#[test]
fn test_apple_hdr_image_type_unmapped_value_is_not_invented() {
    // 0 has no PrintConv entry in ExifTool, so it must surface as an unmapped
    // value rather than as a plausible label. This is the assertion that the
    // old suite got backwards: it required 0 => "Off".
    let parser = AppleParser::new();
    let data = ifd(&[(0x000A, SHORT, 0)]);

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Apple:HDRImageType"),
        Some(&"Unknown (0)".to_string()),
        "value 0 is unmapped in ExifTool 13.59 and must not be given a name"
    );
}

#[test]
fn test_apple_multiple_tags() {
    // Two real Apple.pm tags, both verified by id and shape:
    //   0x000a HDRImageType (PrintConv 4 => 'Original Image')
    //   0x0005 AETarget     (int32s, no PrintConv -- raw number)
    // The previous version of this test also asserted Apple:LensModel and
    // Apple:NightMode, neither of which exists in ExifTool.
    let parser = AppleParser::new();
    let data = ifd(&[(0x000A, SHORT, 4), (0x0005, SLONG, 100)]);

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Apple:HDRImageType"),
        Some(&"Original Image".to_string())
    );
    assert_eq!(tags.get("Apple:AETarget"), Some(&"100".to_string()));
}

#[test]
fn test_apple_invalid_data_too_short() {
    let parser = AppleParser::new();
    let data = vec![0x01]; // Too short

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_err());
}

#[test]
fn test_apple_invalid_entry_count() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x00, 0x02]); // 512 entries (invalid - too many)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_err());
}
