//! Integration tests for Microsoft (Lumia) MakerNotes parser

use exiftool_rs::parsers::tiff::ifd_parser::ByteOrder;
use exiftool_rs::parsers::tiff::makernotes::microsoft::MicrosoftParser;
use exiftool_rs::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

#[test]
fn test_microsoft_parser_trait() {
    let parser = MicrosoftParser::new();
    assert_eq!(parser.manufacturer_name(), "Microsoft");
    assert_eq!(parser.tag_prefix(), "Microsoft:");
}

#[test]
fn test_microsoft_rich_capture_on() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x01, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:RichCapture"), Some(&"On".to_string()));
}

#[test]
fn test_microsoft_rich_capture_mode_hdr() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x02, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:RichCaptureMode"), Some(&"HDR".to_string()));
}

#[test]
fn test_microsoft_rich_capture_mode_hdr_flash() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x02, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:RichCaptureMode"), Some(&"HDR + Flash".to_string()));
}

#[test]
fn test_microsoft_dynamic_flash() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x06, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:DynamicFlash"), Some(&"Flash + No Flash Blend".to_string()));
}

#[test]
fn test_microsoft_refocus_available() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x08, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:Refocus"), Some(&"Available".to_string()));
}

#[test]
fn test_microsoft_pureview_mode_5mp() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0B, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:PureViewMode"), Some(&"5MP Oversampled".to_string()));
}

#[test]
fn test_microsoft_pureview_mode_lossless_zoom() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0B, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:PureViewMode"), Some(&"Lossless Zoom".to_string()));
}

#[test]
fn test_microsoft_creative_effect_vintage() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x0E, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:CreativeEffect"), Some(&"Vintage".to_string()));
}

#[test]
fn test_microsoft_video_4k_on() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x10, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:Video4K"), Some(&"On".to_string()));
}

#[test]
fn test_microsoft_rich_recording_on() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x12, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:RichRecordingAudio"), Some(&"On".to_string()));
}

#[test]
fn test_microsoft_ois_on() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x14, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:OpticalStabilization"), Some(&"On (OIS)".to_string()));
}

#[test]
fn test_microsoft_auto_hdr_on() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x16, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:AutoHDR"), Some(&"On".to_string()));
}

#[test]
fn test_microsoft_panorama_on() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x18, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:PanoramaMode"), Some(&"On".to_string()));
}

#[test]
fn test_microsoft_lens_type_wide_angle() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x01, 0x00];
    data.extend_from_slice(&[0x1A, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.get("Microsoft:LensType"), Some(&"Wide Angle Attachment".to_string()));
}

#[test]
fn test_microsoft_multiple_tags() {
    let parser = MicrosoftParser::new();
    let mut data = vec![0x02, 0x00]; // 2 entries

    // Rich Capture
    data.extend_from_slice(&[0x01, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    // PureView Mode
    data.extend_from_slice(&[0x0B, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);

    let mut tags = HashMap::new();
    assert!(parser.parse(&data, ByteOrder::LittleEndian, &mut tags).is_ok());
    assert_eq!(tags.len(), 2);
    assert_eq!(tags.get("Microsoft:RichCapture"), Some(&"On".to_string()));
    assert_eq!(tags.get("Microsoft:PureViewMode"), Some(&"5MP Oversampled".to_string()));
}
