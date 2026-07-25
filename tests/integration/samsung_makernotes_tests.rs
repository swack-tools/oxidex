//! Integration tests for Samsung MakerNotes parser
//!
//! Tests cover both Type1 (traditional NX cameras) and Type2/Galaxy (smartphones)
//! MakerNote formats.

use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::samsung::SamsungParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

#[test]
fn test_samsung_parser_trait() {
    let parser = SamsungParser::new();
    assert_eq!(parser.manufacturer_name(), "Samsung");
    assert_eq!(parser.tag_prefix(), "Samsung:");
}

// ============================================================================
// Type1 Tag Tests (Traditional Samsung Cameras - NX series)
// ============================================================================

#[test]
fn test_samsung_makernote_version() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x0001 (MakerNoteVersion), Type 2 (ASCII), Count 4, Value "0100"
    data.extend_from_slice(&[0x01, 0x00, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x30, 0x31, 0x30, 0x30]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:MakerNoteVersion"),
        Some(&"0100".to_string())
    );
}

#[test]
fn test_samsung_device_type() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x0002 (DeviceType), Type 4 (LONG), Count 1, Value 0x2000 (High-end NX Camera)
    data.extend_from_slice(&[0x02, 0x00, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:DeviceType"),
        Some(&"High-end NX Camera".to_string())
    );
}

#[test]
fn test_samsung_model_id_nx1() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x0003 (SamsungModelID), Type 4 (LONG), Count 1, Value 0x0100123a (NX1)
    data.extend_from_slice(&[0x03, 0x00, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x3a, 0x12, 0x00, 0x01]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:SamsungModelID"), Some(&"NX1".to_string()));
}

#[test]
fn test_samsung_color_space_srgb() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x0221 (ColorSpace), Type 4 (LONG), Count 1, Value 0 (sRGB)
    data.extend_from_slice(&[0x21, 0x02, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:ColorSpace"), Some(&"sRGB".to_string()));
}

#[test]
fn test_samsung_color_space_adobe_rgb() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x0221 (ColorSpace), Type 4 (LONG), Count 1, Value 1 (Adobe RGB)
    data.extend_from_slice(&[0x21, 0x02, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:ColorSpace"),
        Some(&"Adobe RGB".to_string())
    );
}

// ============================================================================
// Galaxy Smartphone Feature Tag Tests (0x1001-0x101E range)
// ============================================================================

#[test]
fn test_samsung_scene_optimizer_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x1001 (SceneOptimizer), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x01, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:SceneOptimizer"),
        Some(&"On".to_string())
    );
}

#[test]
fn test_samsung_scene_type_food() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x1002 (SceneType), Type 3 (SHORT), Count 1, Value 1 (Food)
    data.extend_from_slice(&[0x02, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:SceneType"), Some(&"Food".to_string()));
}

#[test]
fn test_samsung_single_take_recording() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x1005 (SingleTake), Type 3 (SHORT), Count 1, Value 1 (Recording)
    data.extend_from_slice(&[0x05, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:SingleTake"),
        Some(&"Recording".to_string())
    );
}

#[test]
fn test_samsung_expert_raw_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x1008 (ExpertRAW), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x08, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:ExpertRAW"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_night_mode_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x1012 (NightMode), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x12, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:NightMode"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_galaxy_lens_type_ultra_wide() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x101C (GalaxyLensType), Type 3 (SHORT), Count 1, Value 1 (Ultra Wide)
    data.extend_from_slice(&[0x1C, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:GalaxyLensType"),
        Some(&"Ultra Wide".to_string())
    );
}

#[test]
fn test_samsung_galaxy_lens_type_telephoto_10x() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x101C (GalaxyLensType), Type 3 (SHORT), Count 1, Value 5 (Telephoto 10x)
    data.extend_from_slice(&[0x1C, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:GalaxyLensType"),
        Some(&"Telephoto 10x".to_string())
    );
}

#[test]
fn test_samsung_zoom_level_3_5x() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x101E (ZoomLevel), Type 3 (SHORT), Count 1, Value 35 (3.5x)
    data.extend_from_slice(&[0x1E, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x23, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:ZoomLevel"), Some(&"3.5x".to_string()));
}

#[test]
fn test_samsung_portrait_effect_blur() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x101A (PortraitEffect), Type 3 (SHORT), Count 1, Value 1 (Blur)
    data.extend_from_slice(&[0x1A, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:PortraitEffect"),
        Some(&"Blur".to_string())
    );
}

#[test]
fn test_samsung_directors_view_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x100C (DirectorsView), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x0C, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:DirectorsView"),
        Some(&"On".to_string())
    );
}

#[test]
fn test_samsung_pro_mode_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x100E (ProMode), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x0E, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:ProMode"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_super_steady_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x1016 (SuperSteady), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x16, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:SuperSteady"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_food_mode_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x1018 (FoodMode), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x18, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Samsung:FoodMode"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_object_tracking_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x1010 (ObjectTracking), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x10, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:ObjectTracking"),
        Some(&"On".to_string())
    );
}

#[test]
fn test_samsung_multi_frame_nr_on() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00];
    // Tag 0x100A (MultiFrameNoiseReduction), Type 3 (SHORT), Count 1, Value 1 (On)
    data.extend_from_slice(&[0x0A, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(
        tags.get("Samsung:MultiFrameNoiseReduction"),
        Some(&"On".to_string())
    );
}

#[test]
fn test_samsung_multiple_galaxy_tags() {
    let parser = SamsungParser::new();
    let mut data = vec![0x02, 0x00]; // 2 entries

    // Scene Optimizer (0x1001)
    data.extend_from_slice(&[0x01, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    // Night Mode (0x1012)
    data.extend_from_slice(&[0x12, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.len(), 2);
    assert_eq!(
        tags.get("Samsung:SceneOptimizer"),
        Some(&"On".to_string())
    );
    assert_eq!(tags.get("Samsung:NightMode"), Some(&"On".to_string()));
}

#[test]
fn test_samsung_mixed_type1_and_galaxy_tags() {
    let parser = SamsungParser::new();
    let mut data = vec![0x03, 0x00]; // 3 entries

    // MakerNoteVersion (0x0001) - Type1 string tag
    data.extend_from_slice(&[0x01, 0x00, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x30, 0x31, 0x30, 0x30]);
    // DeviceType (0x0002) - Type1 LONG tag
    data.extend_from_slice(&[0x02, 0x00, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00]);
    // SceneOptimizer (0x1001) - Galaxy feature tag
    data.extend_from_slice(&[0x01, 0x10, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.len(), 3);
    assert_eq!(
        tags.get("Samsung:MakerNoteVersion"),
        Some(&"0100".to_string())
    );
    assert_eq!(
        tags.get("Samsung:DeviceType"),
        Some(&"High-end NX Camera".to_string())
    );
    assert_eq!(
        tags.get("Samsung:SceneOptimizer"),
        Some(&"On".to_string())
    );
}
