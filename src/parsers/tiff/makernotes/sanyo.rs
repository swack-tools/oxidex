//! Sanyo MakerNote parser
//!
//! Parses Sanyo digital camera-specific EXIF MakerNote tags.
//! Sanyo was known for the Xacti series of dual-camera/camcorder devices
//! and waterproof/ruggedized cameras.
//!
//! ## Supported Cameras
//! - Xacti series (dual photo/video cameras)
//! - VPC series (digital cameras)
//!
//! ## Supported Features
//! - Video/photo mode settings
//! - Sequential shooting modes
//! - Scene modes
//! - Quality and color settings
//! - Flash and focus modes
//!
//! ## Tag Structure
//! Sanyo uses a standard IFD format with manufacturer-specific tags.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;

// Sanyo MakerNote Tag IDs
const SANYO_QUALITY: u16 = 0x0100;
const SANYO_FOCUS_MODE: u16 = 0x0102;
const SANYO_FLASH_MODE: u16 = 0x0103;
const SANYO_SEQUENTIAL_MODE: u16 = 0x0104;
const SANYO_WHITE_BALANCE: u16 = 0x0105;
const SANYO_SHARPNESS: u16 = 0x0107;
const SANYO_COLOR_MODE: u16 = 0x0108;
const SANYO_SCENE_MODE: u16 = 0x010A;
const SANYO_RECORD_MODE: u16 = 0x010B;

fn decode_quality(value: u16) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Fine".to_string(),
        2 => "Super Fine".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_focus_mode(value: u16) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Macro".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_sequential_mode(value: u16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Standard".to_string(),
        2 => "Best".to_string(),
        3 => "Adjust Exposure".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_scene_mode(value: u16) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Portrait".to_string(),
        2 => "Scenery".to_string(),
        3 => "Sports".to_string(),
        4 => "Night".to_string(),
        5 => "Beach".to_string(),
        6 => "Snow".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_record_mode(value: u16) -> String {
    match value {
        0 => "Still Image".to_string(),
        1 => "Video".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn extract_u16_value(entry: &IfdEntry, _data: &[u8], byte_order: ByteOrder) -> Option<u16> {
    if entry.value_count != 1 {
        return None;
    }
    let value = match byte_order {
        ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as u16,
        ByteOrder::BigEndian => ((entry.value_offset >> 16) & 0xFFFF) as u16,
    };
    Some(value)
}

pub struct SanyoParser;

impl Default for SanyoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SanyoParser {
    pub fn new() -> Self {
        SanyoParser
    }

    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            SANYO_QUALITY => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Sanyo:Quality".to_string(), decode_quality(value));
                }
            }
            SANYO_FOCUS_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Sanyo:FocusMode".to_string(), decode_focus_mode(value));
                }
            }
            SANYO_FLASH_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("Sanyo:FlashMode".to_string(), mode.to_string());
                }
            }
            SANYO_SEQUENTIAL_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Sanyo:SequentialMode".to_string(),
                        decode_sequential_mode(value),
                    );
                }
            }
            SANYO_SHARPNESS => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Sanyo:Sharpness".to_string(), value.to_string());
                }
            }
            SANYO_SCENE_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Sanyo:SceneMode".to_string(), decode_scene_mode(value));
                }
            }
            SANYO_RECORD_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Sanyo:RecordMode".to_string(), decode_record_mode(value));
                }
            }
            _ => {}
        }
    }
}

impl MakerNoteParser for SanyoParser {
    fn manufacturer_name(&self) -> &'static str {
        "Sanyo"
    }

    fn tag_prefix(&self) -> &'static str {
        "Sanyo:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("Sanyo MakerNote data too short".to_string());
        }

        let ifd_offset = 0;
        let entry_count = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([data[ifd_offset], data[ifd_offset + 1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([data[ifd_offset], data[ifd_offset + 1]]),
        };

        if entry_count == 0 || entry_count > 500 {
            return Err(format!("Invalid entry count: {}", entry_count));
        }

        let entry_size = 12;
        let mut offset = ifd_offset + 2;

        for _ in 0..entry_count {
            if offset + entry_size > data.len() {
                break;
            }

            let tag = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([data[offset], data[offset + 1]]),
                ByteOrder::BigEndian => u16::from_be_bytes([data[offset], data[offset + 1]]),
            };

            let field_type = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([data[offset + 2], data[offset + 3]]),
                ByteOrder::BigEndian => u16::from_be_bytes([data[offset + 2], data[offset + 3]]),
            };

            let count = match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]),
                ByteOrder::BigEndian => u32::from_be_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]),
            };

            let value_offset = match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    data[offset + 8],
                    data[offset + 9],
                    data[offset + 10],
                    data[offset + 11],
                ]),
                ByteOrder::BigEndian => u32::from_be_bytes([
                    data[offset + 8],
                    data[offset + 9],
                    data[offset + 10],
                    data[offset + 11],
                ]),
            };

            let entry = IfdEntry {
                tag_id: tag,
                field_type,
                value_count: count,
                value_offset,
            };

            self.parse_entry(&entry, data, byte_order, tags);
            offset += entry_size;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_quality() {
        assert_eq!(decode_quality(0), "Normal");
        assert_eq!(decode_quality(2), "Super Fine");
    }

    #[test]
    fn test_decode_sequential_mode() {
        assert_eq!(decode_sequential_mode(2), "Best");
    }

    #[test]
    fn test_decode_record_mode() {
        assert_eq!(decode_record_mode(0), "Still Image");
        assert_eq!(decode_record_mode(1), "Video");
    }

    #[test]
    fn test_sanyo_parser_trait() {
        let parser = SanyoParser::new();
        assert_eq!(parser.manufacturer_name(), "Sanyo");
        assert_eq!(parser.tag_prefix(), "Sanyo:");
    }

    #[test]
    fn test_parse_quality() {
        let parser = SanyoParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x00, 0x01]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Sanyo:Quality"), Some(&"Fine".to_string()));
    }
}
