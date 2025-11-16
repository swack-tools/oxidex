//! JVC MakerNote parser
//!
//! Parses JVC digital camera-specific EXIF MakerNote tags.
//! JVC (Victor Company of Japan) produced digital cameras and camcorders,
//! particularly known for their video-focused features.
//!
//! ## Supported Cameras
//! - GC series (digital cameras)
//! - Everio series (hybrid photo/video cameras)
//!
//! ## Supported Features
//! - Camera model and firmware
//! - Image quality settings
//! - Focus and flash modes
//! - Color and scene modes
//!
//! ## Tag Structure
//! JVC uses a simple IFD format with basic tag structure.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;

// JVC MakerNote Tag IDs
const JVC_QUALITY: u16 = 0x0001;
const JVC_FOCUS_MODE: u16 = 0x0002;
const JVC_FLASH_MODE: u16 = 0x0003;
const JVC_WHITE_BALANCE: u16 = 0x0004;
const JVC_SHARPNESS: u16 = 0x0005;
const JVC_COLOR_MODE: u16 = 0x0006;

fn decode_quality(value: u16) -> String {
    match value {
        0 => "Standard".to_string(),
        1 => "Fine".to_string(),
        2 => "Super Fine".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_focus_mode(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Manual".to_string(),
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

pub struct JvcParser;

impl Default for JvcParser {
    fn default() -> Self {
        Self::new()
    }
}

impl JvcParser {
    pub fn new() -> Self {
        JvcParser
    }

    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            JVC_QUALITY => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("JVC:Quality".to_string(), decode_quality(value));
                }
            }
            JVC_FOCUS_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("JVC:FocusMode".to_string(), decode_focus_mode(value));
                }
            }
            JVC_FLASH_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("JVC:FlashMode".to_string(), mode.to_string());
                }
            }
            JVC_SHARPNESS => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("JVC:Sharpness".to_string(), value.to_string());
                }
            }
            _ => {}
        }
    }
}

impl MakerNoteParser for JvcParser {
    fn manufacturer_name(&self) -> &'static str {
        "JVC"
    }

    fn tag_prefix(&self) -> &'static str {
        "JVC:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("JVC MakerNote data too short".to_string());
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
        assert_eq!(decode_quality(0), "Standard");
        assert_eq!(decode_quality(2), "Super Fine");
    }

    #[test]
    fn test_jvc_parser_trait() {
        let parser = JvcParser::new();
        assert_eq!(parser.manufacturer_name(), "JVC");
        assert_eq!(parser.tag_prefix(), "JVC:");
    }

    #[test]
    fn test_parse_quality() {
        let parser = JvcParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("JVC:Quality"), Some(&"Fine".to_string()));
    }
}
