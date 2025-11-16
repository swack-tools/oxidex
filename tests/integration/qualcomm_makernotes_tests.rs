//! Integration tests for Qualcomm MakerNotes parser

use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
use exiftool_rs::parsers::tiff::makernotes::qualcomm::QualcommParser;
use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

#[test]
fn test_qualcomm_parser_trait() {
    let parser = QualcommParser::new();
    assert_eq!(parser.manufacturer_name(), "Qualcomm");
    assert_eq!(parser.tag_prefix(), "Qualcomm:");
}

#[test]
fn test_qualcomm_clear_sight_on() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x01, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:ClearSight"), Some(&"On".to_string()));
}

#[test]
fn test_qualcomm_clear_sight_mode_fusion() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x02, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:ClearSightMode"), Some(&"Monochrome + RGB Fusion".to_string()));
}

#[test]
fn test_qualcomm_chroma_flash() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x04, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:ChromaFlash"), Some(&"Flash + No Flash Blend".to_string()));
}

#[test]
fn test_qualcomm_optizoom_medium() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x07, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:OptiZoom"), Some(&"Medium".to_string()));
}

#[test]
fn test_qualcomm_zoom_level_5x() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x08, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x32, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:ZoomLevel"), Some(&"5.0x".to_string()));
}

#[test]
fn test_qualcomm_hdr_mode() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0A, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:HDRMode"), Some(&"HDR".to_string()));
}

#[test]
fn test_qualcomm_hdr_mode_staggered() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0A, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:HDRMode"), Some(&"Staggered HDR".to_string()));
}

#[test]
fn test_qualcomm_scene_detection_portrait() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0E, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:SceneDetection"), Some(&"Portrait".to_string()));
}

#[test]
fn test_qualcomm_bokeh_mode_on() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x10, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:BokehMode"), Some(&"On".to_string()));
}

#[test]
fn test_qualcomm_bokeh_level() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x11, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x4B, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:BokehLevel"), Some(&"75".to_string()));
}

#[test]
fn test_qualcomm_low_light_mode_on() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x13, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:LowLightMode"), Some(&"On".to_string()));
}

#[test]
fn test_qualcomm_night_mode_on() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x15, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:NightMode"), Some(&"On".to_string()));
}

#[test]
fn test_qualcomm_phase_detect_af() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x17, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:PhaseDetectAF"), Some(&"Active".to_string()));
}

#[test]
fn test_qualcomm_frame_merge_count() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x1B, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:FrameMergeCount"), Some(&"10".to_string()));
}

#[test]
fn test_qualcomm_multi_frame_nr_on() {
    let parser = QualcommParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0C, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Qualcomm:MultiFrameNoiseReduction"), Some(&"On".to_string()));
}

#[test]
fn test_qualcomm_multiple_tags() {
    let parser = QualcommParser::new();
    let mut data = vec![0x02, 0x00]; // 2 entries

    // Clear Sight
    data.extend_from_slice(&[0x01, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    // HDR Mode
    data.extend_from_slice(&[0x0A, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.len(), 2);
    assert_eq!(tags.get("Qualcomm:ClearSight"), Some(&"On".to_string()));
    assert_eq!(tags.get("Qualcomm:HDRMode"), Some(&"HDR".to_string()));
}
