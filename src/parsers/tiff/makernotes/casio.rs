//! Casio MakerNote parser
//!
//! Parses Casio digital camera-specific EXIF MakerNote tags.
//! Casio was known for the Exilim series of ultra-compact digital cameras
//! with high-speed capture and unique features.
//!
//! ## Supported Cameras
//! - Exilim series (EX-Z, EX-S, EX-F)
//! - QV series (early digital cameras)
//! - GV series (with LCD viewfinder)
//!
//! ## Supported Features
//! - High-speed burst mode settings
//! - Best Shot scene selection
//! - Continuous shooting modes
//! - Image quality and sharpness
//! - Flash and focus settings
//! - Color mode and effects
//! - Digital zoom information
//!
//! ## Tag Structure
//! Casio uses a standard IFD format with manufacturer-specific tag IDs.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;

// Casio MakerNote Tag IDs
const CASIO_RECORDING_MODE: u16 = 0x0001; // Recording mode
const CASIO_QUALITY: u16 = 0x0002; // Image quality
const CASIO_FOCUS_MODE: u16 = 0x0003; // Focus mode
const CASIO_FLASH_MODE: u16 = 0x0004; // Flash mode
const CASIO_FLASH_INTENSITY: u16 = 0x0005; // Flash intensity
const CASIO_WHITE_BALANCE: u16 = 0x0007; // White balance
const CASIO_DIGITAL_ZOOM: u16 = 0x000A; // Digital zoom
const CASIO_SHARPNESS: u16 = 0x000B; // Sharpness
const CASIO_CONTRAST: u16 = 0x000C; // Contrast
const CASIO_SATURATION: u16 = 0x000D; // Saturation
const CASIO_CCD_SENSITIVITY: u16 = 0x0014; // CCD ISO sensitivity
const CASIO_COLOR_MODE: u16 = 0x0015; // Color mode
const CASIO_ENHANCEMENT: u16 = 0x0016; // Image enhancement
const CASIO_COLOR_FILTER: u16 = 0x0017; // Color filter effect
const CASIO_CONTINUOUS_MODE: u16 = 0x001A; // Continuous shooting mode
const CASIO_BEST_SHOT_MODE: u16 = 0x001B; // Best Shot scene mode
const CASIO_SLOW_SHUTTER: u16 = 0x0020; // Slow shutter setting

/// Decodes Casio recording mode
///
/// # Arguments
/// * `value` - Recording mode value
///
/// # Returns
/// Human-readable recording mode
fn decode_recording_mode(value: u16) -> String {
    match value {
        1 => "Single".to_string(),
        2 => "Panorama".to_string(),
        3 => "Night Scene".to_string(),
        4 => "Portrait".to_string(),
        5 => "Landscape".to_string(),
        6 => "Sports".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Casio image quality
///
/// # Arguments
/// * `value` - Quality value
///
/// # Returns
/// Human-readable quality setting
fn decode_quality(value: u16) -> String {
    match value {
        1 => "Economy".to_string(),
        2 => "Normal".to_string(),
        3 => "Fine".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Casio focus mode
///
/// # Arguments
/// * `value` - Focus mode value
///
/// # Returns
/// Human-readable focus mode
fn decode_focus_mode(value: u16) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Macro".to_string(),
        2 => "Super Macro".to_string(),
        3 => "Infinity".to_string(),
        4 => "Manual".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Casio flash mode
///
/// # Arguments
/// * `value` - Flash mode value
///
/// # Returns
/// Human-readable flash mode
fn decode_flash_mode(value: u16) -> String {
    match value {
        1 => "Auto".to_string(),
        2 => "On".to_string(),
        3 => "Off".to_string(),
        4 => "Red-eye Reduction".to_string(),
        5 => "Slow Sync".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Casio white balance
///
/// # Arguments
/// * `value` - White balance value
///
/// # Returns
/// Human-readable white balance mode
fn decode_white_balance(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Daylight".to_string(),
        2 => "Shade".to_string(),
        3 => "Tungsten".to_string(),
        4 => "Fluorescent".to_string(),
        5 => "Manual".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Casio color mode
///
/// # Arguments
/// * `value` - Color mode value
///
/// # Returns
/// Human-readable color mode
fn decode_color_mode(value: u16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "On".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Casio enhancement mode
///
/// # Arguments
/// * `value` - Enhancement value
///
/// # Returns
/// Human-readable enhancement mode
fn decode_enhancement(value: u16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Red".to_string(),
        2 => "Green".to_string(),
        3 => "Blue".to_string(),
        4 => "Flesh Tones".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Casio Best Shot mode
///
/// # Arguments
/// * `value` - Best Shot mode value
///
/// # Returns
/// Human-readable Best Shot mode
fn decode_best_shot_mode(value: u16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Portrait".to_string(),
        2 => "Scenery".to_string(),
        3 => "Night Scene".to_string(),
        4 => "Night Scene Portrait".to_string(),
        5 => "Sunset".to_string(),
        6 => "High Sensitivity".to_string(),
        7 => "Children".to_string(),
        8 => "Sports".to_string(),
        9 => "Candlelight".to_string(),
        10 => "Fireworks".to_string(),
        11 => "Food".to_string(),
        12 => "Text".to_string(),
        _ => format!("Mode {}", value),
    }
}

/// Extracts a 16-bit unsigned value from IFD entry
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

/// Casio MakerNote parser implementation
pub struct CasioParser;

impl Default for CasioParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CasioParser {
    /// Creates a new Casio parser instance
    pub fn new() -> Self {
        CasioParser
    }

    /// Parse a single IFD entry and extract tag value
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            CASIO_RECORDING_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Casio:RecordingMode".to_string(),
                        decode_recording_mode(value),
                    );
                }
            }
            CASIO_QUALITY => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:Quality".to_string(), decode_quality(value));
                }
            }
            CASIO_FOCUS_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:FocusMode".to_string(), decode_focus_mode(value));
                }
            }
            CASIO_FLASH_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:FlashMode".to_string(), decode_flash_mode(value));
                }
            }
            CASIO_FLASH_INTENSITY => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:FlashIntensity".to_string(), value.to_string());
                }
            }
            CASIO_WHITE_BALANCE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Casio:WhiteBalance".to_string(),
                        decode_white_balance(value),
                    );
                }
            }
            CASIO_DIGITAL_ZOOM => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:DigitalZoom".to_string(), value.to_string());
                }
            }
            CASIO_SHARPNESS => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:Sharpness".to_string(), value.to_string());
                }
            }
            CASIO_CONTRAST => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:Contrast".to_string(), value.to_string());
                }
            }
            CASIO_SATURATION => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:Saturation".to_string(), value.to_string());
                }
            }
            CASIO_CCD_SENSITIVITY => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:CCDSensitivity".to_string(), value.to_string());
                }
            }
            CASIO_COLOR_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:ColorMode".to_string(), decode_color_mode(value));
                }
            }
            CASIO_ENHANCEMENT => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Casio:Enhancement".to_string(), decode_enhancement(value));
                }
            }
            CASIO_BEST_SHOT_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Casio:BestShotMode".to_string(),
                        decode_best_shot_mode(value),
                    );
                }
            }
            CASIO_CONTINUOUS_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("Casio:ContinuousMode".to_string(), mode.to_string());
                }
            }
            CASIO_SLOW_SHUTTER => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Casio:SlowShutter".to_string(), status.to_string());
                }
            }
            _ => {}
        }
    }
}

impl MakerNoteParser for CasioParser {
    fn manufacturer_name(&self) -> &'static str {
        "Casio"
    }

    fn tag_prefix(&self) -> &'static str {
        "Casio:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("Casio MakerNote data too short".to_string());
        }

        let ifd_offset = 0;

        // Read number of IFD entries
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
    fn test_decode_recording_mode() {
        assert_eq!(decode_recording_mode(1), "Single");
        assert_eq!(decode_recording_mode(4), "Portrait");
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(decode_quality(1), "Economy");
        assert_eq!(decode_quality(3), "Fine");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(1), "Macro");
        assert_eq!(decode_focus_mode(2), "Super Macro");
    }

    #[test]
    fn test_decode_best_shot_mode() {
        assert_eq!(decode_best_shot_mode(0), "Off");
        assert_eq!(decode_best_shot_mode(10), "Fireworks");
    }

    #[test]
    fn test_casio_parser_trait() {
        let parser = CasioParser::new();
        assert_eq!(parser.manufacturer_name(), "Casio");
        assert_eq!(parser.tag_prefix(), "Casio:");
    }

    #[test]
    fn test_parse_quality_tag() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x02, 0x00]); // Tag: CASIO_QUALITY
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Normal)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:Quality"), Some(&"Normal".to_string()));
    }

    #[test]
    fn test_parse_focus_mode_tag() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x03, 0x00]); // Tag: CASIO_FOCUS_MODE
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Macro)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:FocusMode"), Some(&"Macro".to_string()));
    }

    #[test]
    fn test_parse_best_shot_tag() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x1B, 0x00]); // Tag: CASIO_BEST_SHOT_MODE
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // Value: 8 (Sports)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:BestShotMode"), Some(&"Sports".to_string()));
    }
}
