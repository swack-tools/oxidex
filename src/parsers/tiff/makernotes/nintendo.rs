//! Nintendo 3DS Camera MakerNote parser
//!
//! Parses Nintendo-specific EXIF MakerNote tags from 3DS handheld camera.
//! The Nintendo 3DS features dual cameras for stereoscopic 3D photography.
//!
//! ## Supported Models
//! - Nintendo 3DS
//! - Nintendo 3DS XL
//! - New Nintendo 3DS
//! - New Nintendo 3DS XL
//! - Nintendo 2DS (single camera, no 3D)
//!
//! ## Key Features
//! - Stereoscopic 3D mode
//! - Parallax adjustment
//! - Camera selection (inner/outer)
//! - 3D effect depth
//! - Game integration metadata
//! - Mii face detection
//!
//! ## Architecture
//! Stores metadata specific to handheld gaming device photography,
//! including 3D stereoscopic capture settings.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

const NINTENDO_MODEL: u16 = 0x0001;
const NINTENDO_SYSTEM_VERSION: u16 = 0x0002;
const NINTENDO_CAMERA_MODE: u16 = 0x0100; // 2D/3D mode
const NINTENDO_CAMERA_SELECTION: u16 = 0x0101; // Inner/Outer camera
const NINTENDO_PARALLAX: u16 = 0x0102; // Stereoscopic parallax
const NINTENDO_3D_EFFECT: u16 = 0x0103; // 3D effect depth (0-100)
const NINTENDO_FACE_DETECTION: u16 = 0x0104; // Face detection enabled
const NINTENDO_MII_DETECTED: u16 = 0x0105; // Mii character detected
const NINTENDO_FILTER_APPLIED: u16 = 0x0106; // Photo filter code
const NINTENDO_GAME_TITLE: u16 = 0x0107; // Game title (if taken in-game)

const NINTENDO_SIGNATURE: &[u8] = b"Nintendo";

fn decode_camera_mode(value: i16) -> String {
    match value {
        0 => "2D".to_string(),
        1 => "3D".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_camera_selection(value: i16) -> String {
    match value {
        0 => "Inner Camera".to_string(),
        1 => "Outer Camera Left".to_string(),
        2 => "Outer Camera Right".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_filter(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Sepia".to_string(),
        2 => "Black & White".to_string(),
        3 => "Negative".to_string(),
        4 => "Toy Camera".to_string(),
        5 => "Fisheye".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn format_parallax(value: i16) -> String {
    format!("{:.2} mm", value as f64 / 100.0)
}

fn format_3d_effect(value: i16) -> String {
    if value < 0 || value > 100 {
        return "Invalid".to_string();
    }
    format!("{}%", value)
}

fn extract_string(entry: &IfdEntry, data: &[u8]) -> Option<String> {
    if entry.field_type != 2 {
        return None;
    }
    let offset = entry.value_offset as usize;
    let count = entry.value_count as usize;
    if count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = String::from_utf8_lossy(&bytes[..count.min(4)])
            .trim_end_matches('\0')
            .to_string();
        return if s.is_empty() { None } else { Some(s) };
    }
    if offset + count > data.len() {
        return None;
    }
    let s = String::from_utf8_lossy(&data[offset..offset + count])
        .trim_end_matches('\0')
        .to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Nintendo 3DS Camera MakerNote parser
/// Default implementation for parser
#[derive(Default)]
pub struct NintendoParser;

impl NintendoParser {
    /// Creates a new Nintendo parser instance
    pub fn new() -> Self {
        NintendoParser
    }
}

impl MakerNoteParser for NintendoParser {
    fn manufacturer_name(&self) -> &'static str {
        "Nintendo"
    }

    fn tag_prefix(&self) -> &'static str {
        "Nintendo:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        data.len() >= 8 && (data.starts_with(NINTENDO_SIGNATURE) || data.len() >= 8)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Nintendo MakerNote data too short".to_string());
        }
        let start_offset = if data.starts_with(NINTENDO_SIGNATURE) {
            8
        } else {
            0
        };
        let parse_data = &data[start_offset..];
        if parse_data.len() < 2 {
            return Ok(());
        }

        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([parse_data[0], parse_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([parse_data[0], parse_data[1]]),
        } as usize;
        if num_entries == 0 || num_entries > 200 {
            return Ok(());
        }

        let mut offset = 2;
        for _ in 0..num_entries {
            if offset + 12 > parse_data.len() {
                break;
            }
            let entry_data = &parse_data[offset..offset + 12];

            let tag = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[0], entry_data[1]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[0], entry_data[1]]),
            };
            let field_type = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[2], entry_data[3]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[2], entry_data[3]]),
            };
            let count = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
            };
            let value_offset = match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
                ByteOrder::BigEndian => u32::from_be_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
            };

            let entry = IfdEntry {
                tag_id: tag,
                field_type,
                value_count: count,
                value_offset,
            };

            match tag {
                NINTENDO_MODEL | NINTENDO_SYSTEM_VERSION | NINTENDO_GAME_TITLE => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            NINTENDO_MODEL => "Model",
                            NINTENDO_SYSTEM_VERSION => "SystemVersion",
                            NINTENDO_GAME_TITLE => "GameTitle",
                            _ => continue,
                        };
                        tags.insert(format!("Nintendo:{}", tag_name), s);
                    }
                }
                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                NINTENDO_CAMERA_MODE => ("CameraMode", decode_camera_mode(val)),
                                NINTENDO_CAMERA_SELECTION => {
                                    ("CameraSelection", decode_camera_selection(val))
                                }
                                NINTENDO_PARALLAX => ("Parallax", format_parallax(val)),
                                NINTENDO_3D_EFFECT => ("3DEffect", format_3d_effect(val)),
                                NINTENDO_FACE_DETECTION => (
                                    "FaceDetection",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                NINTENDO_MII_DETECTED => (
                                    "MiiDetected",
                                    if val != 0 {
                                        "Yes".to_string()
                                    } else {
                                        "No".to_string()
                                    },
                                ),
                                NINTENDO_FILTER_APPLIED => ("Filter", decode_filter(val)),
                                _ => continue,
                            };
                            tags.insert(format!("Nintendo:{}", tag_name), formatted_value);
                        }
                    }
                }
            }
            offset += 12;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nintendo_parser_creation() {
        let parser = NintendoParser::new();
        assert_eq!(parser.manufacturer_name(), "Nintendo");
        assert_eq!(parser.tag_prefix(), "Nintendo:");
    }

    #[test]
    fn test_decode_camera_mode() {
        assert_eq!(decode_camera_mode(0), "2D");
        assert_eq!(decode_camera_mode(1), "3D");
    }

    #[test]
    fn test_decode_camera_selection() {
        assert_eq!(decode_camera_selection(0), "Inner Camera");
        assert_eq!(decode_camera_selection(1), "Outer Camera Left");
    }

    #[test]
    fn test_format_parallax() {
        assert_eq!(format_parallax(350), "3.50 mm");
    }

    #[test]
    fn test_format_3d_effect() {
        assert_eq!(format_3d_effect(50), "50%");
        assert_eq!(format_3d_effect(100), "100%");
    }

    #[test]
    fn test_decode_filter() {
        assert_eq!(decode_filter(0), "None");
        assert_eq!(decode_filter(4), "Toy Camera");
    }
}
