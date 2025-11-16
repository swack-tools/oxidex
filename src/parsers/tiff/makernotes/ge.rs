//! GE MakerNote parser
//!
//! Parses General Electric digital camera-specific EXIF MakerNote tags.
//! GE produced consumer-oriented digital cameras under license
//! (often rebranded from other manufacturers).
//!
//! ## Supported Cameras
//! - GE Power series
//! - GE E-series (entry-level compacts)
//! - GE X-series (advanced compacts)
//!
//! ## Supported Features
//! - Camera model information
//! - Image quality settings
//! - Flash and scene modes
//! - Basic shooting parameters
//!
//! ## Tag Structure
//! GE uses a simple IFD format with basic manufacturer tags.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;

// GE MakerNote Tag IDs
const GE_QUALITY: u16 = 0x0001;
const GE_FOCUS_MODE: u16 = 0x0002;
const GE_FLASH_MODE: u16 = 0x0003;
const GE_SCENE_MODE: u16 = 0x0004;
const GE_WHITE_BALANCE: u16 = 0x0005;

fn decode_quality(value: u16) -> String {
    match value {
        1 => "Standard".to_string(),
        2 => "Fine".to_string(),
        3 => "Super Fine".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_scene_mode(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Portrait".to_string(),
        2 => "Landscape".to_string(),
        3 => "Night".to_string(),
        4 => "Sports".to_string(),
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

/// Parser for GE camera MakerNotes
pub struct GeParser;

impl Default for GeParser {
    fn default() -> Self {
        Self::new()
    }
}

impl GeParser {
    /// Creates a new GE parser instance
    pub fn new() -> Self {
        GeParser
    }

    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            GE_QUALITY => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("GE:Quality".to_string(), decode_quality(value));
                }
            }
            GE_FOCUS_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value == 0 { "Auto" } else { "Manual" };
                    tags.insert("GE:FocusMode".to_string(), mode.to_string());
                }
            }
            GE_FLASH_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("GE:FlashMode".to_string(), mode.to_string());
                }
            }
            GE_SCENE_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("GE:SceneMode".to_string(), decode_scene_mode(value));
                }
            }
            _ => {}
        }
    }
}

impl MakerNoteParser for GeParser {
    fn manufacturer_name(&self) -> &'static str {
        "GE"
    }

    fn tag_prefix(&self) -> &'static str {
        "GE:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("GE MakerNote data too short".to_string());
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
        assert_eq!(decode_quality(1), "Standard");
        assert_eq!(decode_quality(3), "Super Fine");
    }

    #[test]
    fn test_decode_scene_mode() {
        assert_eq!(decode_scene_mode(0), "Auto");
        assert_eq!(decode_scene_mode(2), "Landscape");
    }

    #[test]
    fn test_ge_parser_trait() {
        let parser = GeParser::new();
        assert_eq!(parser.manufacturer_name(), "GE");
        assert_eq!(parser.tag_prefix(), "GE:");
    }

    #[test]
    fn test_parse_quality() {
        let parser = GeParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("GE:Quality"), Some(&"Fine".to_string()));
    }

    #[test]
    fn test_parse_scene_mode() {
        let parser = GeParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x04, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("GE:SceneMode"), Some(&"Sports".to_string()));
    }
}
