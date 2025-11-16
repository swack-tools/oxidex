//! Motorola MakerNote parser
//!
//! Parses Motorola smartphone camera-specific EXIF MakerNote tags.
//! Motorola phones used custom MakerNote tags before adopting Android
//! standard EXIF, and some modern Moto phones still include them.
//!
//! ## Supported Devices
//! - RAZR series phones
//! - DROID series phones
//! - Moto G/X/E series (modern smartphones)
//!
//! ## Supported Features
//! - Camera mode and scene detection
//! - HDR and night mode settings
//! - Burst shot information
//! - Computational photography features
//! - Flash and focus modes
//!
//! ## Tag Structure
//! Motorola uses a simple IFD format with phone-specific tags.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;

// Motorola MakerNote Tag IDs
const MOTOROLA_CAMERA_MODE: u16 = 0x0001;
const MOTOROLA_HDR_MODE: u16 = 0x0002;
const MOTOROLA_NIGHT_MODE: u16 = 0x0003;
const MOTOROLA_BURST_MODE: u16 = 0x0004;
const MOTOROLA_SCENE_MODE: u16 = 0x0005;
const MOTOROLA_FLASH_MODE: u16 = 0x0006;
const MOTOROLA_FOCUS_MODE: u16 = 0x0007;
const MOTOROLA_PORTRAIT_MODE: u16 = 0x0008;

fn decode_camera_mode(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Photo".to_string(),
        2 => "Video".to_string(),
        3 => "Portrait".to_string(),
        4 => "Night".to_string(),
        5 => "Pro".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_scene_mode(value: u16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Portrait".to_string(),
        2 => "Landscape".to_string(),
        3 => "Food".to_string(),
        4 => "Night".to_string(),
        5 => "Document".to_string(),
        6 => "Pet".to_string(),
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

/// Parser for Motorola camera MakerNotes
pub struct MotorolaParser;

impl Default for MotorolaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MotorolaParser {
    /// Creates a new Motorola parser instance
    pub fn new() -> Self {
        MotorolaParser
    }

    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            MOTOROLA_CAMERA_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Motorola:CameraMode".to_string(), decode_camera_mode(value));
                }
            }
            MOTOROLA_HDR_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("Motorola:HDRMode".to_string(), mode.to_string());
                }
            }
            MOTOROLA_NIGHT_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("Motorola:NightMode".to_string(), mode.to_string());
                }
            }
            MOTOROLA_BURST_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("Motorola:BurstMode".to_string(), mode.to_string());
                }
            }
            MOTOROLA_SCENE_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Motorola:SceneMode".to_string(), decode_scene_mode(value));
                }
            }
            MOTOROLA_FLASH_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("Motorola:FlashMode".to_string(), mode.to_string());
                }
            }
            MOTOROLA_PORTRAIT_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("Motorola:PortraitMode".to_string(), mode.to_string());
                }
            }
            _ => {}
        }
    }
}

impl MakerNoteParser for MotorolaParser {
    fn manufacturer_name(&self) -> &'static str {
        "Motorola"
    }

    fn tag_prefix(&self) -> &'static str {
        "Motorola:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("Motorola MakerNote data too short".to_string());
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
    fn test_decode_camera_mode() {
        assert_eq!(decode_camera_mode(0), "Auto");
        assert_eq!(decode_camera_mode(3), "Portrait");
        assert_eq!(decode_camera_mode(5), "Pro");
    }

    #[test]
    fn test_decode_scene_mode() {
        assert_eq!(decode_scene_mode(0), "None");
        assert_eq!(decode_scene_mode(3), "Food");
    }

    #[test]
    fn test_motorola_parser_trait() {
        let parser = MotorolaParser::new();
        assert_eq!(parser.manufacturer_name(), "Motorola");
        assert_eq!(parser.tag_prefix(), "Motorola:");
    }

    #[test]
    fn test_parse_camera_mode() {
        let parser = MotorolaParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(
            tags.get("Motorola:CameraMode"),
            Some(&"Portrait".to_string())
        );
    }

    #[test]
    fn test_parse_hdr_mode() {
        let parser = MotorolaParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x02, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Motorola:HDRMode"), Some(&"On".to_string()));
    }
}
