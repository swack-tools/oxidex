//! Leaf MakerNote parser
//!
//! Parses Leaf digital back-specific EXIF MakerNote tags.
//! Leaf (later acquired by Phase One) produced high-end digital backs
//! for medium format cameras, primarily used in commercial photography.
//!
//! ## Supported Systems
//! - Leaf Aptus series (digital backs for Mamiya, Contax, Hasselblad)
//! - Leaf Valeo series (earlier digital backs)
//! - Leaf Cantare series (large format backs)
//!
//! ## Supported Features
//! - Sensor and back information
//! - Image quality and bit depth
//! - Color calibration data
//! - Exposure and ISO settings
//! - Lens information with database
//! - Medium format specific metadata
//!
//! ## Tag Structure
//! Leaf uses a standard IFD format with professional imaging tags.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::leaf_lens_database::lookup_leaf_lens;
use super::shared::MakerNoteParser;

// Leaf MakerNote Tag IDs
const LEAF_BACK_MODEL: u16 = 0x0001; // Digital back model
const LEAF_BACK_SERIAL: u16 = 0x0002; // Serial number
const LEAF_IMAGE_WIDTH: u16 = 0x0003; // Image width
const LEAF_IMAGE_HEIGHT: u16 = 0x0004; // Image height
const LEAF_BIT_DEPTH: u16 = 0x0005; // Bit depth per channel
const LEAF_ISO_SPEED: u16 = 0x0006; // ISO sensitivity
const LEAF_SHUTTER_SPEED: u16 = 0x0007; // Shutter speed
const LEAF_APERTURE: u16 = 0x0008; // Aperture value
const LEAF_FOCAL_LENGTH: u16 = 0x0009; // Focal length
const LEAF_LENS_ID: u16 = 0x000A; // Lens model ID
const LEAF_WHITE_BALANCE: u16 = 0x000B; // White balance mode
const LEAF_COLOR_SPACE: u16 = 0x000C; // Color space
const LEAF_CALIBRATION: u16 = 0x000D; // Calibration profile
const LEAF_FIRMWARE: u16 = 0x000E; // Firmware version

/// Decodes Leaf white balance mode
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
        2 => "Tungsten".to_string(),
        3 => "Fluorescent".to_string(),
        4 => "Flash".to_string(),
        5 => "Cloudy".to_string(),
        6 => "Custom".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Leaf color space
///
/// # Arguments
/// * `value` - Color space value
///
/// # Returns
/// Human-readable color space
fn decode_color_space(value: u16) -> String {
    match value {
        0 => "sRGB".to_string(),
        1 => "Adobe RGB".to_string(),
        2 => "ProPhoto RGB".to_string(),
        3 => "ECI RGB".to_string(),
        _ => format!("Unknown ({})", value),
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

/// Extracts a 32-bit unsigned value from IFD entry
fn extract_u32_value(entry: &IfdEntry, _data: &[u8], _byte_order: ByteOrder) -> Option<u32> {
    if entry.value_count != 1 {
        return None;
    }
    Some(entry.value_offset)
}

/// Extracts an ASCII string from IFD entry
fn extract_string(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<String> {
    if entry.value_count == 0 {
        return None;
    }

    let value_bytes = if entry.value_count <= 4 {
        let mut bytes = Vec::new();
        for i in 0..entry.value_count as usize {
            let byte = match byte_order {
                ByteOrder::LittleEndian => ((entry.value_offset >> (i * 8)) & 0xFF) as u8,
                ByteOrder::BigEndian => ((entry.value_offset >> (24 - i * 8)) & 0xFF) as u8,
            };
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }
        bytes
    } else {
        let offset = entry.value_offset as usize;
        if offset >= data.len() {
            return None;
        }
        let end = std::cmp::min(offset + entry.value_count as usize, data.len());
        data[offset..end].to_vec()
    };

    if value_bytes.is_empty() {
        return None;
    }

    let string = String::from_utf8_lossy(&value_bytes)
        .trim_end_matches('\0')
        .to_string();

    if string.is_empty() {
        None
    } else {
        Some(string)
    }
}

/// Leaf MakerNote parser implementation
pub struct LeafParser;

impl Default for LeafParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LeafParser {
    /// Creates a new Leaf parser instance
    pub fn new() -> Self {
        LeafParser
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
            LEAF_BACK_MODEL => {
                if let Some(model) = extract_string(entry, data, byte_order) {
                    tags.insert("Leaf:BackModel".to_string(), model);
                }
            }
            LEAF_BACK_SERIAL => {
                if let Some(serial) = extract_string(entry, data, byte_order) {
                    tags.insert("Leaf:SerialNumber".to_string(), serial);
                }
            }
            LEAF_IMAGE_WIDTH => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert("Leaf:ImageWidth".to_string(), value.to_string());
                }
            }
            LEAF_IMAGE_HEIGHT => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert("Leaf:ImageHeight".to_string(), value.to_string());
                }
            }
            LEAF_BIT_DEPTH => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Leaf:BitDepth".to_string(), format!("{} bits", value));
                }
            }
            LEAF_ISO_SPEED => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Leaf:ISOSpeed".to_string(), value.to_string());
                }
            }
            LEAF_FOCAL_LENGTH => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Leaf:FocalLength".to_string(), format!("{} mm", value));
                }
            }
            LEAF_LENS_ID => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    // Store the lens ID
                    tags.insert("Leaf:LensID".to_string(), format!("0x{:04X}", value));

                    // Lookup lens name from database
                    if let Some(lens_name) = lookup_leaf_lens(value) {
                        tags.insert("Leaf:LensType".to_string(), lens_name);
                    }
                }
            }
            LEAF_WHITE_BALANCE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Leaf:WhiteBalance".to_string(), decode_white_balance(value));
                }
            }
            LEAF_COLOR_SPACE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Leaf:ColorSpace".to_string(), decode_color_space(value));
                }
            }
            LEAF_CALIBRATION => {
                if let Some(profile) = extract_string(entry, data, byte_order) {
                    tags.insert("Leaf:CalibrationProfile".to_string(), profile);
                }
            }
            LEAF_FIRMWARE => {
                if let Some(firmware) = extract_string(entry, data, byte_order) {
                    tags.insert("Leaf:FirmwareVersion".to_string(), firmware);
                }
            }
            _ => {}
        }
    }
}

impl MakerNoteParser for LeafParser {
    fn manufacturer_name(&self) -> &'static str {
        "Leaf"
    }

    fn tag_prefix(&self) -> &'static str {
        "Leaf:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("Leaf MakerNote data too short".to_string());
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

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_leaf_lens(lens_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0), "Auto");
        assert_eq!(decode_white_balance(1), "Daylight");
        assert_eq!(decode_white_balance(6), "Custom");
    }

    #[test]
    fn test_decode_color_space() {
        assert_eq!(decode_color_space(0), "sRGB");
        assert_eq!(decode_color_space(2), "ProPhoto RGB");
    }

    #[test]
    fn test_leaf_parser_trait() {
        let parser = LeafParser::new();
        assert_eq!(parser.manufacturer_name(), "Leaf");
        assert_eq!(parser.tag_prefix(), "Leaf:");
    }

    #[test]
    fn test_parse_bit_depth() {
        let parser = LeafParser::new();
        let mut data = Vec::new();

        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x05, 0x00]); // Tag: LEAF_BIT_DEPTH
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // Value: 16

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Leaf:BitDepth"), Some(&"16 bits".to_string()));
    }

    #[test]
    fn test_lens_lookup() {
        let parser = LeafParser::new();
        assert_eq!(
            parser.lookup_lens(0x0103),
            Some("Mamiya AF 80mm f/2.8".to_string())
        );
        assert_eq!(
            parser.lookup_lens(0x0302),
            Some("Contax 645 80mm f/2.0".to_string())
        );
        assert_eq!(parser.lookup_lens(0xFFFF), None);
    }

    #[test]
    fn test_parse_white_balance() {
        let parser = LeafParser::new();
        let mut data = Vec::new();

        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x0B, 0x00]); // Tag: LEAF_WHITE_BALANCE
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Daylight)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Leaf:WhiteBalance"), Some(&"Daylight".to_string()));
    }
}
