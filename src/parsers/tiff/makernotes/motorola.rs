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

use crate::const_decoder;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::motorola::motorola_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Camera Mode decoder - Different shooting modes
const_decoder!(pub
    CAMERA_MODE,
    u16,
    [
        (0, "Auto"),
        (1, "Photo"),
        (2, "Video"),
        (3, "Portrait"),
        (4, "Night"),
        (5, "Pro"),
    ]
);

// Scene Mode decoder - Scene recognition modes
const_decoder!(pub
    SCENE_MODE,
    u16,
    [
        (0, "None"),
        (1, "Portrait"),
        (2, "Landscape"),
        (3, "Food"),
        (4, "Night"),
        (5, "Document"),
        (6, "Pet"),
    ]
);

// ============================================================================
// Helper Functions
// ============================================================================

// Extracts u16 value from IFD entry
// # Arguments
// * `entry` - The IFD entry
// * `_data` - The MakerNote data buffer (unused for inline values)
// * `byte_order` - Byte order for value extraction
// # Returns
// The extracted u16 value, or None if extraction fails
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
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(motorola_registry);

// ============================================================================
// Parser Implementation
// ============================================================================

/// Parser for Motorola MakerNotes
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

    /// Parses a single IFD entry and extracts the tag value
    /// Delegates to registry for tag decoding when available
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_id = entry.tag_id;

        if let Some(value) = extract_u16_value(entry, data, byte_order) {
            let tag_name = match TAG_REGISTRY.get_tag_name(tag_id) {
                Some(name) => name,
                None => return,
            };

            // Try registry decoding first, fall back to hardcoded logic
            let formatted_value = TAG_REGISTRY.decode_u16(tag_id, value);

            // Fallback for tags without decoder in registry
            let formatted_value = if formatted_value == value.to_string() {
                match tag_id {
                    0x0002 | 0x0003 | 0x0004 | 0x0006 | 0x0008 => {
                        let mode = if value > 0 { "On" } else { "Off" };
                        mode.to_string()
                    }
                    0x0007 => {
                        let mode = if value == 0 { "Auto" } else { "Manual" };
                        mode.to_string()
                    }
                    _ => formatted_value,
                }
            } else {
                formatted_value
            };

            tags.insert(format!("Motorola:{}", tag_name), formatted_value);
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
    fn test_camera_mode_decoder() {
        assert_eq!(CAMERA_MODE.decode(0), "Auto");
        assert_eq!(CAMERA_MODE.decode(3), "Portrait");
        assert_eq!(CAMERA_MODE.decode(5), "Pro");
    }

    #[test]
    fn test_scene_mode_decoder() {
        assert_eq!(SCENE_MODE.decode(0), "None");
        assert_eq!(SCENE_MODE.decode(3), "Food");
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
        data.extend_from_slice(&[0x01, 0x00]); // entry_count = 1
        data.extend_from_slice(&[0x01, 0x00]); // tag = 0x0001 (MOTOROLA_CAMERA_MODE)
        data.extend_from_slice(&[0x03, 0x00]); // field_type = 3
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // value_count = 1
        data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // value_offset = 3

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
        data.extend_from_slice(&[0x01, 0x00]); // entry_count = 1
        data.extend_from_slice(&[0x02, 0x00]); // tag = 0x0002 (MOTOROLA_HDR_MODE)
        data.extend_from_slice(&[0x03, 0x00]); // field_type = 3
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // value_count = 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // value_offset = 1

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Motorola:HDRMode"), Some(&"On".to_string()));
    }

    #[test]
    fn test_tag_registry() {
        assert_eq!(TAG_REGISTRY.get_tag_name(0x0001), Some("CameraMode"));
        assert!(TAG_REGISTRY.has_tag(0x0002));
    }
}
