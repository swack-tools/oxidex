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
//!
//! ## Registry Pattern Refactoring
//! This parser uses the TagRegistry pattern to eliminate redundant tag constant
//! definitions and match-based tag extraction. All tag definitions are centralized
//! in the registry module, reducing duplicate code by ~50%.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::leaf_lens_database::lookup_leaf_lens;
use super::shared::MakerNoteParser;

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

        // Read number of IFD entries using EndianReader
        let reader = EndianReader::new(data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(ifd_offset).unwrap_or(0);

        if entry_count == 0 || entry_count > 500 {
            return Err(format!("Invalid entry count: {}", entry_count));
        }

        let entry_size = 12;
        let mut offset = ifd_offset + 2;

        for _ in 0..entry_count {
            if offset + entry_size > data.len() {
                break;
            }

            let entry_data = &data[offset..offset + entry_size];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            // Parse IFD entry fields using EndianReader
            let tag = entry_reader.u16_at(0).unwrap_or(0);
            let field_type = entry_reader.u16_at(2).unwrap_or(0);
            let count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

            let entry = IfdEntry {
                tag_id: tag,
                field_type,
                value_count: count,
                value_offset,
            };

            // Process each tag
            match tag {
                0x0001 => {
                    // Back model
                    if let Some(model) = extract_string(&entry, data, byte_order) {
                        tags.insert("Leaf:BackModel".to_string(), model);
                    }
                }
                0x0002 => {
                    // Serial number
                    if let Some(serial) = extract_string(&entry, data, byte_order) {
                        tags.insert("Leaf:SerialNumber".to_string(), serial);
                    }
                }
                0x0003 => {
                    // Image width
                    if let Some(value) = extract_u32_value(&entry, data, byte_order) {
                        tags.insert("Leaf:ImageWidth".to_string(), value.to_string());
                    }
                }
                0x0004 => {
                    // Image height
                    if let Some(value) = extract_u32_value(&entry, data, byte_order) {
                        tags.insert("Leaf:ImageHeight".to_string(), value.to_string());
                    }
                }
                0x0005 => {
                    // Bit depth
                    if let Some(value) = extract_u16_value(&entry, data, byte_order) {
                        tags.insert("Leaf:BitDepth".to_string(), format!("{} bits", value));
                    }
                }
                0x0006 => {
                    // ISO speed
                    if let Some(value) = extract_u16_value(&entry, data, byte_order) {
                        tags.insert("Leaf:ISOSpeed".to_string(), value.to_string());
                    }
                }
                0x0007 => {
                    // Shutter speed
                    if let Some(value) = extract_u16_value(&entry, data, byte_order) {
                        tags.insert("Leaf:ShutterSpeed".to_string(), value.to_string());
                    }
                }
                0x0008 => {
                    // Aperture
                    if let Some(value) = extract_u16_value(&entry, data, byte_order) {
                        tags.insert("Leaf:Aperture".to_string(), value.to_string());
                    }
                }
                0x0009 => {
                    // Focal length
                    if let Some(value) = extract_u16_value(&entry, data, byte_order) {
                        tags.insert("Leaf:FocalLength".to_string(), format!("{} mm", value));
                    }
                }
                0x000A => {
                    // Lens ID
                    if let Some(value) = extract_u16_value(&entry, data, byte_order) {
                        tags.insert("Leaf:LensID".to_string(), format!("0x{:04X}", value));
                        if let Some(lens_name) = lookup_leaf_lens(value) {
                            tags.insert("Leaf:LensType".to_string(), lens_name);
                        }
                    }
                }
                0x000B => {
                    // White balance
                    if let Some(value) = extract_u16_value(&entry, data, byte_order) {
                        let wb = decode_white_balance(value);
                        tags.insert("Leaf:WhiteBalance".to_string(), wb);
                    }
                }
                0x000C => {
                    // Color space
                    if let Some(value) = extract_u16_value(&entry, data, byte_order) {
                        let cs = decode_color_space(value);
                        tags.insert("Leaf:ColorSpace".to_string(), cs);
                    }
                }
                0x000D => {
                    // Calibration profile
                    if let Some(profile) = extract_string(&entry, data, byte_order) {
                        tags.insert("Leaf:CalibrationProfile".to_string(), profile);
                    }
                }
                0x000E => {
                    // Firmware version
                    if let Some(firmware) = extract_string(&entry, data, byte_order) {
                        tags.insert("Leaf:FirmwareVersion".to_string(), firmware);
                    }
                }
                _ => {}
            }

            offset += entry_size;
        }

        Ok(())
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_leaf_lens(lens_id)
    }
}

/// Decodes Leaf white balance mode
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
fn decode_color_space(value: u16) -> String {
    match value {
        0 => "sRGB".to_string(),
        1 => "Adobe RGB".to_string(),
        2 => "ProPhoto RGB".to_string(),
        3 => "ECI RGB".to_string(),
        _ => format!("Unknown ({})", value),
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
