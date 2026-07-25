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

use crate::const_decoder;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::jvc::jvc_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// Decodes JVC image quality
const_decoder!(pub DECODE_QUALITY, u16, [
    (0, "Standard"),
    (1, "Fine"),
    (2, "Super Fine"),
]);

// Decodes JVC focus mode
const_decoder!(pub DECODE_FOCUS_MODE, u16, [
    (0, "Auto"),
    (1, "Manual"),
]);

// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(jvc_registry);

// Extracts a u16 value from an IFD entry's value_offset field
// This handles the case where the value is stored inline in the offset field
// rather than as a pointer to external data
fn extract_u16_value(entry: &IfdEntry, _data: &[u8], byte_order: ByteOrder) -> Option<u16> {
    if entry.value_count != 1 {
        return None;
    }
    // Extract the u16 value from the appropriate bytes of the u32 value_offset
    // based on byte order. Little endian uses lower 16 bits, big endian uses upper 16 bits
    let value = match byte_order {
        ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as u16,
        ByteOrder::BigEndian => ((entry.value_offset >> 16) & 0xFFFF) as u16,
    };
    Some(value)
}

/// Parser for JVC MakerNotes
pub struct JvcParser;

impl Default for JvcParser {
    fn default() -> Self {
        Self::new()
    }
}

impl JvcParser {
    /// Creates a new JVC parser instance
    pub fn new() -> Self {
        JvcParser
    }

    /// Parses a single JVC MakerNote IFD entry and extracts its tag value
    /// Uses centralized registry for tag metadata and decoding
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        if let Some(value) = extract_u16_value(entry, data, byte_order) {
            let tag_name = match TAG_REGISTRY.get_tag_name(entry.tag_id) {
                Some(name) => name,
                None => return,
            };

            // Try registry decoding first
            let formatted_value = TAG_REGISTRY.decode_u16(entry.tag_id, value);

            // Fallback for tags without decoder in registry
            let formatted_value = if formatted_value == value.to_string() {
                match entry.tag_id {
                    0x0003 => {
                        let mode = if value > 0 { "On" } else { "Off" };
                        mode.to_string()
                    }
                    0x0005 => value.to_string(),
                    _ => formatted_value,
                }
            } else {
                formatted_value
            };

            tags.insert(format!("JVC:{}", tag_name), formatted_value);
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
        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 500,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_quality() {
        assert_eq!(DECODE_QUALITY.decode(0), "Standard");
        assert_eq!(DECODE_QUALITY.decode(2), "Super Fine");
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
