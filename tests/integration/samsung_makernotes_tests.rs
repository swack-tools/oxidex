//! Integration tests for Samsung MakerNotes parser

use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
use exiftool_rs::parsers::tiff::makernotes::samsung::SamsungParser;
use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

#[test]
fn test_samsung_parser_trait() {
    let parser = SamsungParser::new();
    assert_eq!(parser.manufacturer_name(), "Samsung");
    assert_eq!(parser.tag_prefix(), "Samsung:");
}

#[test]
fn test_samsung_scene_optimizer_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    data.extend_from_slice(&[0x01, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:SceneOptimizer"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_scene_type_food() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x02, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:SceneType"), Some(&"Food".to_string()));
}

#[test]
fn test_samsung_single_take_recording() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x05, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:SingleTake"), Some(&"Recording".to_string()));
}

#[test]
fn test_samsung_expert_raw_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x08, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:ExpertRAW"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_night_mode_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x12, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:NightMode"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_lens_type_ultra_wide() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x1C, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:LensType"), Some(&"Ultra Wide".to_string()));
}

#[test]
fn test_samsung_lens_type_telephoto_10x() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x1C, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:LensType"), Some(&"Telephoto 10x".to_string()));
}

#[test]
fn test_samsung_zoom_level_3_5x() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x1E, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x23, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:ZoomLevel"), Some(&"3.5x".to_string()));
}

#[test]
fn test_samsung_portrait_effect_blur() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x1A, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:PortraitEffect"), Some(&"Blur".to_string()));
}

#[test]
fn test_samsung_directors_view_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0C, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:DirectorsView"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_pro_mode_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0E, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:ProMode"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_super_steady_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x16, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:SuperSteady"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_food_mode_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x18, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:FoodMode"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_object_tracking_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x10, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:ObjectTracking"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_multi_frame_nr_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0A, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:MultiFrameNoiseReduction"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_multiple_tags() {
    let parser = SamsungParser::new();
    let mut data = vec![0x02, 0x00]; // 2 entries

    // Scene Optimizer
    data.extend_from_slice(&[0x01, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    // Night Mode
    data.extend_from_slice(&[0x12, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.len(), 2);
    assert_eq!(tags.get("Samsung:SceneOptimizer"), Some(&"On".to_string()));
    assert_eq!(tags.get("Samsung:NightMode"), Some(&"On".to_string()));
}
