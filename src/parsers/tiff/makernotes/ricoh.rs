//! Ricoh MakerNote parser
//!
//! Parses Ricoh digital camera-specific EXIF MakerNote tags.
//! Ricoh (and later Pentax Ricoh) produced compact cameras and
//! specialized models like the GR series and Theta 360 cameras.
//!
//! ## Supported Cameras
//! - GR Digital series (advanced compact)
//! - Caplio series (consumer compact)
//! - CX series (high-zoom compact)
//!
//! ## Supported Features
//! - Camera model and settings
//! - Exposure and focus modes
//! - Image quality settings
//! - Flash and white balance
//! - Special shooting modes
//!
//! ## Tag Structure
//! Ricoh uses a standard IFD format similar to Pentax.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;

// Ricoh MakerNote Tag IDs
const RICOH_MODEL: u16 = 0x0001;
const RICOH_FIRMWARE: u16 = 0x0002;
const RICOH_SHOOTING_MODE: u16 = 0x0005;
const RICOH_FLASH_MODE: u16 = 0x000C;
const RICOH_FOCUS_MODE: u16 = 0x001D;
const RICOH_WHITE_BALANCE: u16 = 0x001E;
const RICOH_ISO_SETTING: u16 = 0x0022;
const RICOH_COLOR_MODE: u16 = 0x0034;
const RICOH_SHARPNESS: u16 = 0x0035;

fn decode_shooting_mode(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Program".to_string(),
        2 => "Aperture Priority".to_string(),
        3 => "Manual".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_flash_mode(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "On".to_string(),
        2 => "Off".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_white_balance(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Daylight".to_string(),
        2 => "Shade".to_string(),
        3 => "Fluorescent".to_string(),
        4 => "Tungsten".to_string(),
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

/// Parser for Ricoh camera MakerNotes
pub struct RicohParser;

impl Default for RicohParser {
    fn default() -> Self {
        Self::new()
    }
}

impl RicohParser {
    /// Creates a new Ricoh parser instance
    pub fn new() -> Self {
        RicohParser
    }

    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            RICOH_SHOOTING_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Ricoh:ShootingMode".to_string(),
                        decode_shooting_mode(value),
                    );
                }
            }
            RICOH_FLASH_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Ricoh:FlashMode".to_string(), decode_flash_mode(value));
                }
            }
            RICOH_WHITE_BALANCE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Ricoh:WhiteBalance".to_string(),
                        decode_white_balance(value),
                    );
                }
            }
            RICOH_FOCUS_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value == 0 { "Auto" } else { "Manual" };
                    tags.insert("Ricoh:FocusMode".to_string(), mode.to_string());
                }
            }
            RICOH_ISO_SETTING => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Ricoh:ISO".to_string(), value.to_string());
                }
            }
            RICOH_SHARPNESS => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Ricoh:Sharpness".to_string(), value.to_string());
                }
            }
            _ => {}
        }
    }
}

impl MakerNoteParser for RicohParser {
    fn manufacturer_name(&self) -> &'static str {
        "Ricoh"
    }

    fn tag_prefix(&self) -> &'static str {
        "Ricoh:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("Ricoh MakerNote data too short".to_string());
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
    fn test_decode_shooting_mode() {
        assert_eq!(decode_shooting_mode(0), "Auto");
        assert_eq!(decode_shooting_mode(1), "Program");
    }

    #[test]
    fn test_ricoh_parser_trait() {
        let parser = RicohParser::new();
        assert_eq!(parser.manufacturer_name(), "Ricoh");
        assert_eq!(parser.tag_prefix(), "Ricoh:");
    }

    #[test]
    fn test_parse_shooting_mode() {
        let parser = RicohParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x05, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Ricoh:ShootingMode"), Some(&"Program".to_string()));
    }
}
