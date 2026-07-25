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
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::ricoh::ricoh_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ============================================================================
// Ricoh MakerNote Tag IDs (for parsing reference)
// ============================================================================
// Tag definitions are centralized in the registry (registries/ricoh.rs)
// These constants are retained for parse_entry() to identify special handling

const RICOH_FOCUS_MODE: u16 = 0x001D;
const RICOH_ISO_SETTING: u16 = 0x0022;
const RICOH_SHARPNESS: u16 = 0x0035;

// Static registry instance for efficient tag lookup and decoding
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(ricoh_registry);

/// Extracts a 16-bit unsigned value from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the value
/// * `byte_order` - Byte order for interpreting multi-byte values
///
/// # Returns
/// The extracted u16 value, or None if the entry doesn't contain exactly one value
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

/// Ricoh MakerNote parser implementation
pub struct RicohParser;

impl Default for RicohParser {
    fn default() -> Self {
        Self::new()
    }
}

impl RicohParser {
    /// Creates a new RicohParser instance
    pub fn new() -> Self {
        RicohParser
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

        // Extract u16 value for all registered tags
        let value = match extract_u16_value(entry, data, byte_order) {
            Some(v) => v,
            None => return,
        };

        // Format value based on tag type and registered decoders
        let formatted_value = match entry.tag_id {
            // Tags with registry-based decoders (shooting mode, flash mode, white balance)
            0x0005 | 0x000C | 0x001E => TAG_REGISTRY.decode_u16(entry.tag_id, value),

            // Focus mode: manual binary decode
            RICOH_FOCUS_MODE => {
                if value == 0 {
                    "Auto".to_string()
                } else {
                    "Manual".to_string()
                }
            }

            // Numeric tags: ISO, Sharpness
            RICOH_ISO_SETTING | RICOH_SHARPNESS => value.to_string(),

            // Unknown tag handling (shouldn't reach here due to registry check)
            _ => return,
        };

        tags.insert(format!("Ricoh:{}", tag_name), formatted_value);
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
        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 500,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x05, 0x00]); // Tag: ShootingMode (0x0005)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Ricoh:ShootingMode"), Some(&"Program".to_string()));
    }

    #[test]
    fn test_parse_focus_mode() {
        let parser = RicohParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x1D, 0x00]); // Tag: FocusMode (0x001D)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Manual)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Ricoh:FocusMode"), Some(&"Manual".to_string()));
    }

    #[test]
    fn test_tag_registry() {
        assert_eq!(TAG_REGISTRY.get_tag_name(0x0005), Some("ShootingMode"));
        assert!(TAG_REGISTRY.has_tag(0x000C));
        assert_eq!(TAG_REGISTRY.get_tag_name(0x001E), Some("WhiteBalance"));
    }
}
