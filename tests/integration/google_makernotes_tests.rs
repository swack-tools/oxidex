//! Integration tests for Google (Pixel) MakerNotes parser
//!
//! Tests the Google Pixel MakerNotes parsing functionality including:
//! - MakerNoteParser trait implementation
//! - Header validation
//! - HDR+ mode detection
//! - Night Sight status
//! - Super Res Zoom
//! - Motion Photos
//! - Computational photography settings

use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
use exiftool_rs::parsers::tiff::makernotes::google::GoogleParser;
use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

#[test]
fn test_google_parser_trait() {
    let parser = GoogleParser::new();
    assert_eq!(parser.manufacturer_name(), "Google");
    assert_eq!(parser.tag_prefix(), "Google:");
}

#[test]
fn test_google_validate_header_with_signature() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();
    data.extend_from_slice(b"Google");
    data.extend_from_slice(&[0x00, 0x00]); // Padding
    data.extend_from_slice(&[0x05, 0x00]); // 5 entries

    assert!(parser.validate_header(&data));
}

#[test]
fn test_google_hdr_plus_off() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x01, 0x00]); // Tag: HDR+ Mode
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Value: 0 (Off)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:HDRPlusMode"), Some(&"Off".to_string()));
}

#[test]
fn test_google_hdr_plus_enhanced() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x01, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Enhanced)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Google:HDRPlusMode"),
        Some(&"HDR+ Enhanced".to_string())
    );
}

#[test]
fn test_google_night_sight_off() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x03, 0x00]); // Tag: Night Sight
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Value: 0 (Off)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:NightSight"), Some(&"Off".to_string()));
}

#[test]
fn test_google_night_sight_on() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x03, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (On)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:NightSight"), Some(&"On".to_string()));
}

#[test]
fn test_google_night_sight_astrophotography() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x03, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // Value: 3 (Astrophotography)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Google:NightSight"),
        Some(&"Astrophotography".to_string())
    );
}

#[test]
fn test_google_super_res_zoom_off() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x05, 0x00]); // Tag: Super Res Zoom
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Value: 0 (Off)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:SuperResZoom"), Some(&"Off".to_string()));
}

#[test]
fn test_google_super_res_zoom_2x() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x05, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Value: 20 (2.0x)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:SuperResZoom"), Some(&"2.0x".to_string()));
}

#[test]
fn test_google_super_res_zoom_7_5x() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x05, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x4B, 0x00, 0x00, 0x00]); // Value: 75 (7.5x)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:SuperResZoom"), Some(&"7.5x".to_string()));
}

#[test]
fn test_google_scene_detection_food() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x0B, 0x00]); // Tag: Scene Detection
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]); // Value: 7 (Food)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:SceneDetection"), Some(&"Food".to_string()));
}

#[test]
fn test_google_face_retouching() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x09, 0x00]); // Tag: Face Retouching
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x32, 0x00, 0x00, 0x00]); // Value: 50

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:FaceRetouching"), Some(&"50".to_string()));
}

#[test]
fn test_google_color_pop_on() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x0F, 0x00]); // Tag: Color Pop
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.get("Google:ColorPop"), Some(&"On".to_string()));
}

#[test]
fn test_google_astrophotography_on() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x11, 0x00]); // Tag: Astrophotography
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Google:Astrophotography"),
        Some(&"On".to_string())
    );
}

#[test]
fn test_google_frame_merge_count() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    data.extend_from_slice(&[0x01, 0x00]); // 1 entry
    data.extend_from_slice(&[0x19, 0x00]); // Tag: Frame Count
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x0F, 0x00, 0x00, 0x00]); // Value: 15 frames

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(
        tags.get("Google:MergedFrameCount"),
        Some(&"15".to_string())
    );
}

#[test]
fn test_google_multiple_tags() {
    let parser = GoogleParser::new();
    let mut data = Vec::new();

    // Create IFD with multiple entries
    data.extend_from_slice(&[0x03, 0x00]); // 3 entries

    // HDR+ Mode
    data.extend_from_slice(&[0x01, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Enhanced)

    // Night Sight
    data.extend_from_slice(&[0x03, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (On)

    // Super Res Zoom
    data.extend_from_slice(&[0x05, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x20, 0x00, 0x00, 0x00]); // Value: 32 (3.2x)

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_ok());
    assert_eq!(tags.len(), 3);
    assert_eq!(
        tags.get("Google:HDRPlusMode"),
        Some(&"HDR+ Enhanced".to_string())
    );
    assert_eq!(tags.get("Google:NightSight"), Some(&"On".to_string()));
    assert_eq!(tags.get("Google:SuperResZoom"), Some(&"3.2x".to_string()));
}

#[test]
fn test_google_invalid_data() {
    let parser = GoogleParser::new();
    let data = vec![0x01]; // Too short

    let mut tags = HashMap::new();
    let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

    assert!(result.is_err());
}
