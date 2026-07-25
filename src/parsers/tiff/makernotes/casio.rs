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
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::casio::casio_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ===== Casio MakerNote Tag IDs =====
// Tag definitions are now centralized in the registry.
// See registries/casio.rs for the complete tag registry.

// Tag ID constants for special tag handling
const CASIO_RECORDING_MODE: u16 = 0x0001;
const CASIO_QUALITY: u16 = 0x0002;
const CASIO_FOCUS_MODE: u16 = 0x0003;
const CASIO_FLASH_MODE: u16 = 0x0004;
const CASIO_FLASH_INTENSITY: u16 = 0x0005;
const CASIO_WHITE_BALANCE: u16 = 0x0007;
const CASIO_DIGITAL_ZOOM: u16 = 0x000A;
const CASIO_SHARPNESS: u16 = 0x000B;
const CASIO_CONTRAST: u16 = 0x000C;
const CASIO_SATURATION: u16 = 0x000D;
const CASIO_CCD_SENSITIVITY: u16 = 0x0014;
const CASIO_COLOR_MODE: u16 = 0x0015;
const CASIO_ENHANCEMENT: u16 = 0x0016;
const CASIO_CONTINUOUS_MODE: u16 = 0x001A;
const CASIO_BEST_SHOT_MODE: u16 = 0x001B;
const CASIO_SLOW_SHUTTER: u16 = 0x0020;

// Static registry instance for efficient tag lookup and decoding
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(casio_registry);

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
    ///
    /// # Arguments
    /// * `entry` - IFD entry to parse
    /// * `data` - Full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        // Get tag name from registry
        let tag_name = match TAG_REGISTRY.get_tag_name(entry.tag_id) {
            Some(name) => name,
            None => return, // Unknown tag, skip it
        };

        // Extract and format the value based on tag type
        let formatted_value = match entry.tag_id {
            // Binary on/off tags
            CASIO_CONTINUOUS_MODE | CASIO_SLOW_SHUTTER => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    if value > 0 {
                        "On".to_string()
                    } else {
                        "Off".to_string()
                    }
                } else {
                    return;
                }
            }
            // All other tags use raw value as string
            _ => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    value.to_string()
                } else {
                    return;
                }
            }
        };

        tags.insert(format!("Casio:{}", tag_name), formatted_value);
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
        // Casio MakerNotes typically start immediately with IFD entries
        // No header is used, so signature is None
        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 500,
        };

        // Parse IFD entries using the shared parser
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x02, 0x00]); // Tag: CASIO_QUALITY (0x0002)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:Quality"), Some(&"2".to_string()));
    }

    #[test]
    fn test_parse_focus_mode_tag() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x03, 0x00]); // Tag: CASIO_FOCUS_MODE (0x0003)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:FocusMode"), Some(&"1".to_string()));
    }

    #[test]
    fn test_parse_continuous_mode_on() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x1A, 0x00]); // Tag: CASIO_CONTINUOUS_MODE (0x001A)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:ContinuousMode"), Some(&"On".to_string()));
    }

    #[test]
    fn test_parse_continuous_mode_off() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x1A, 0x00]); // Tag: CASIO_CONTINUOUS_MODE (0x001A)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Value: 0

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:ContinuousMode"), Some(&"Off".to_string()));
    }
}
