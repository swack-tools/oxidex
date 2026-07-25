//! Sanyo MakerNote parser
//!
//! Parses Sanyo digital camera-specific EXIF MakerNote tags.
//! Sanyo was known for the Xacti series of dual-camera/camcorder devices
//! and waterproof/ruggedized cameras.
//!
//! ## Supported Cameras
//! - Xacti series (dual photo/video cameras)
//! - VPC series (digital cameras)
//!
//! ## Supported Features
//! - Video/photo mode settings
//! - Sequential shooting modes
//! - Scene modes
//! - Quality and color settings
//! - Flash and focus modes
//!
//! ## Tag Structure
//! Sanyo uses a standard IFD format with manufacturer-specific tags.

#![allow(dead_code)]

use crate::const_decoder;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::sanyo::sanyo_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

const_decoder!(pub
    QUALITY,
    u16,
    [(0, "Normal"), (1, "Fine"), (2, "Super Fine"),]
);

const_decoder!(pub FOCUS_MODE, u16, [(0, "Normal"), (1, "Macro"),]);

const_decoder!(pub
    SEQUENTIAL_MODE,
    u16,
    [
        (0, "None"),
        (1, "Standard"),
        (2, "Best"),
        (3, "Adjust Exposure"),
    ]
);

const_decoder!(pub
    SCENE_MODE,
    u16,
    [
        (0, "Normal"),
        (1, "Portrait"),
        (2, "Scenery"),
        (3, "Sports"),
        (4, "Night"),
        (5, "Beach"),
        (6, "Snow"),
    ]
);

const_decoder!(pub RECORD_MODE, u16, [(0, "Still Image"), (1, "Video"),]);

// ============================================================================
// Helper Functions
// ============================================================================

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

// ============================================================================
// Tag Registry
// ============================================================================

// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(sanyo_registry);

// ============================================================================
// Parser Implementation
// ============================================================================

/// Parser for Sanyo MakerNotes
pub struct SanyoParser;

impl Default for SanyoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SanyoParser {
    /// Creates a new SanyoParser instance
    pub fn new() -> Self {
        SanyoParser
    }

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
                    0x0103 => {
                        let mode = if value > 0 { "On" } else { "Off" };
                        mode.to_string()
                    }
                    0x0107 => value.to_string(),
                    _ => formatted_value,
                }
            } else {
                formatted_value
            };

            tags.insert(format!("Sanyo:{}", tag_name), formatted_value);
        }
    }
}

impl MakerNoteParser for SanyoParser {
    fn manufacturer_name(&self) -> &'static str {
        "Sanyo"
    }

    fn tag_prefix(&self) -> &'static str {
        "Sanyo:"
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

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_decoder() {
        assert_eq!(QUALITY.decode(0), "Normal");
        assert_eq!(QUALITY.decode(2), "Super Fine");
    }

    #[test]
    fn test_sequential_mode_decoder() {
        assert_eq!(SEQUENTIAL_MODE.decode(2), "Best");
    }

    #[test]
    fn test_record_mode_decoder() {
        assert_eq!(RECORD_MODE.decode(0), "Still Image");
        assert_eq!(RECORD_MODE.decode(1), "Video");
    }

    #[test]
    fn test_sanyo_parser_trait() {
        let parser = SanyoParser::new();
        assert_eq!(parser.manufacturer_name(), "Sanyo");
        assert_eq!(parser.tag_prefix(), "Sanyo:");
    }

    #[test]
    fn test_parse_quality() {
        let parser = SanyoParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]);
        data.extend_from_slice(&[0x00, 0x01]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Sanyo:Quality"), Some(&"Fine".to_string()));
    }

    #[test]
    fn test_tag_registry() {
        assert_eq!(TAG_REGISTRY.get_tag_name(0x0100), Some("Quality"));
        assert!(TAG_REGISTRY.has_tag(0x010A));
    }
}
