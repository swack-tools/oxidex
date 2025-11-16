//! HP MakerNote parser
//!
//! Parses HP PhotoSmart digital camera-specific EXIF MakerNote tags.
//! HP (Hewlett-Packard) produced the PhotoSmart series of digital cameras
//! in the early 2000s before exiting the camera market.
//!
//! ## Supported Cameras
//! - PhotoSmart series (consumer point-and-shoot)
//! - PhotoSmart Pro series (prosumer models)
//!
//! ## Supported Features
//! - Camera model and firmware
//! - Image quality and size settings
//! - Flash and exposure modes
//! - Color settings
//! - Special effects
//!
//! ## Tag Structure
//! HP uses a simple proprietary tag structure.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;

// HP MakerNote Tag IDs
const HP_MODEL: u16 = 0x0001;
const HP_QUALITY: u16 = 0x0003;
const HP_COLOR_MODE: u16 = 0x0005;
const HP_FLASH_MODE: u16 = 0x0007;
const HP_WHITE_BALANCE: u16 = 0x0009;
const HP_SHARPNESS: u16 = 0x000B;

fn decode_quality(value: u16) -> String {
    match value {
        1 => "Normal".to_string(),
        2 => "Fine".to_string(),
        3 => "Superfine".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_color_mode(value: u16) -> String {
    match value {
        0 => "Color".to_string(),
        1 => "Black & White".to_string(),
        2 => "Sepia".to_string(),
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

/// Parser for HP camera MakerNotes
pub struct HpParser;

impl Default for HpParser {
    fn default() -> Self {
        Self::new()
    }
}

impl HpParser {
    /// Creates a new HP parser instance
    pub fn new() -> Self {
        HpParser
    }

    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            HP_QUALITY => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("HP:Quality".to_string(), decode_quality(value));
                }
            }
            HP_COLOR_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("HP:ColorMode".to_string(), decode_color_mode(value));
                }
            }
            HP_FLASH_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("HP:FlashMode".to_string(), mode.to_string());
                }
            }
            HP_SHARPNESS => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("HP:Sharpness".to_string(), value.to_string());
                }
            }
            _ => {}
        }
    }
}

impl MakerNoteParser for HpParser {
    fn manufacturer_name(&self) -> &'static str {
        "HP"
    }

    fn tag_prefix(&self) -> &'static str {
        "HP:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("HP MakerNote data too short".to_string());
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
        assert_eq!(decode_quality(1), "Normal");
        assert_eq!(decode_quality(3), "Superfine");
    }

    #[test]
    fn test_hp_parser_trait() {
        let parser = HpParser::new();
        assert_eq!(parser.manufacturer_name(), "HP");
        assert_eq!(parser.tag_prefix(), "HP:");
    }

    #[test]
    fn test_parse_quality() {
        let parser = HpParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("HP:Quality"), Some(&"Fine".to_string()));
    }
}
