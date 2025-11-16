//! Integration tests for Apple (iPhone/iPad) MakerNotes parser
//!
//! Tests the Apple MakerNotes parsing functionality including:
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data
//! - HDR mode detection
//! - Portrait Mode effects
//! - Live Photo status
//! - Multi-camera lens identification
//! - Semantic Styles

use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
use exiftool_rs::parsers::tiff::makernotes::apple::AppleParser;
use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

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

#[test]
fn test_apple_hdr_image_type_off() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    // Create minimal IFD with one entry
    data.extend_from_slice(&[0x01, 0x00]); // 1 entry

    // HDR tag entry (tag=0x000A, type=3 (SHORT), count=1, value=0 (Off))
    data.extend_from_slice(&[0x0A, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Value: 0 (inline)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Apple:HDRImageType"), Some(&"Off".to_string()));
}

#[test]
fn test_apple_hdr_image_type_smart_hdr() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x0A, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Value: 4 (Smart HDR)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Apple:HDRImageType"), Some(&"Smart HDR".to_string()));
}

#[test]
fn test_apple_portrait_mode_natural_light() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x20, 0x00]); // Tag: Portrait Data
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Natural Light)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Apple:PortraitMode"),
        Some(&"Natural Light".to_string())
    );
}

#[test]
fn test_apple_portrait_mode_stage_light() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x20, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Value: 4 (Stage Light)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Apple:PortraitMode"),
        Some(&"Stage Light".to_string())
    );
}

#[test]
fn test_apple_lens_model_wide() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x35, 0x00]); // Tag: Lens Model
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Value: 0 (Wide)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Apple:LensModel"),
        Some(&"Wide (Main Camera)".to_string())
    );
}

#[test]
fn test_apple_lens_model_telephoto() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x35, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Telephoto)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Apple:LensModel"), Some(&"Telephoto".to_string()));
}

#[test]
fn test_apple_lens_model_ultra_wide() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x35, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Ultra Wide)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Apple:LensModel"), Some(&"Ultra Wide".to_string()));
}

#[test]
fn test_apple_semantic_style_standard() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x2E, 0x00]); // Tag: Semantic Style
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Value: 0 (Standard)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Apple:SemanticStyle"),
        Some(&"Standard".to_string())
    );
}

#[test]
fn test_apple_semantic_style_vibrant() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x2E, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Vibrant)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Apple:SemanticStyle"), Some(&"Vibrant".to_string()));
}

#[test]
fn test_apple_night_mode_on() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x39, 0x00]); // Tag: Night Mode
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Apple:NightMode"), Some(&"On".to_string()));
}

#[test]
fn test_apple_scene_detection_food() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x3C, 0x00]); // Tag: Scene Detection
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // Value: 8 (Food)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Apple:SceneDetection"), Some(&"Food".to_string()));
}

#[test]
fn test_apple_front_facing_camera() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x32, 0x00]); // Tag: Front Facing Camera
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Front)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Apple:FacingCamera"), Some(&"Front".to_string()));
}

#[test]
fn test_apple_multiple_tags() {
    let parser = AppleParser::new();
    let mut data = Vec::new();

    // Create IFD with multiple entries
    data.extend_from_slice(&[0x03, 0x00]); // 3 entries

    // HDR tag
    data.extend_from_slice(&[0x0A, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Value: 4 (Smart HDR)

    // Lens Model tag
    data.extend_from_slice(&[0x35, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Telephoto)

    // Night Mode tag
    data.extend_from_slice(&[0x39, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.len(), 3);
    assert_eq!(tags.get("Apple:HDRImageType"), Some(&"Smart HDR".to_string()));
    assert_eq!(tags.get("Apple:LensModel"), Some(&"Telephoto".to_string()));
    assert_eq!(tags.get("Apple:NightMode"), Some(&"On".to_string()));
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
