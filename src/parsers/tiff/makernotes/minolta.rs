//! Minolta MakerNote parser
//!
//! Parses Minolta (and early Konica Minolta) camera-specific EXIF MakerNote tags.
//! Minolta was a major camera manufacturer from 1985-2006, later merged with Konica
//! to form Konica Minolta before Sony acquired the camera division in 2006.
//!
//! ## Supported Cameras
//! - Minolta Maxxum/Dynax series (film and early digital)
//! - DiMAGE digital camera series
//! - Early Konica Minolta models (7D, 5D)
//!
//! ## Supported Features
//! - Camera model and firmware
//! - Exposure settings and modes
//! - Focus mode and AF points
//! - Image quality and color settings
//! - Flash settings
//! - Lens information with database lookup
//! - White balance and metering
//!
//! ## Tag Structure
//! Minolta uses a standard IFD format with manufacturer-specific tag IDs.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::minolta_lens_database::lookup_minolta_lens;
use super::registries::minolta::minolta_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ===== Minolta MakerNote Tag IDs =====
// Tag definitions are now centralized in the registry.
// See registries/minolta.rs for the complete tag registry.

// Static registry instance for efficient tag lookup and decoding
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(minolta_registry);

/// Extracts a 16-bit unsigned value from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the value
/// * `_data` - Full MakerNote data buffer (unused for inline values)
/// * `byte_order` - Byte order for parsing
///
/// # Returns
/// Extracted value or None if invalid
fn extract_u16_value(entry: &IfdEntry, _data: &[u8], byte_order: ByteOrder) -> Option<u16> {
    if entry.value_count != 1 {
        return None;
    }

    // For SHORT type (count=1), value is inline in value_offset field
    let value = match byte_order {
        ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as u16,
        ByteOrder::BigEndian => ((entry.value_offset >> 16) & 0xFFFF) as u16,
    };

    Some(value)
}

/// Extracts a signed 16-bit value from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the value
/// * `_data` - Full MakerNote data buffer
/// * `byte_order` - Byte order for parsing
///
/// # Returns
/// Extracted value or None if invalid
fn extract_i16_value(entry: &IfdEntry, _data: &[u8], byte_order: ByteOrder) -> Option<i16> {
    if entry.value_count != 1 {
        return None;
    }

    let value = match byte_order {
        ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as i16,
        ByteOrder::BigEndian => ((entry.value_offset >> 16) & 0xFFFF) as i16,
    };

    Some(value)
}

/// Extracts an ASCII string from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the string
/// * `data` - Full MakerNote data buffer
/// * `byte_order` - Byte order for parsing
///
/// # Returns
/// Extracted string or None if invalid
fn extract_string(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<String> {
    if entry.value_count == 0 {
        return None;
    }

    let value_bytes = if entry.value_count <= 4 {
        // Inline string (stored in value_offset field)
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
        // External string (offset points to data)
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

// ============================================================================
// DECODERS - Minolta Value Decoders
// ============================================================================
// Minolta-specific value decoders for camera settings

use crate::const_decoder;

// Decoder for Minolta image quality settings
// Maps image quality codes to quality level names:
// - 0 = Standard quality (baseline compression)
// - 1 = Super Fine quality (highest setting, minimal compression)
// - 2 = Fine quality (medium-high setting, moderate compression)
const_decoder!(pub DECODE_IMAGE_QUALITY, u16, [
    (0, "Standard"),
    (1, "Super Fine"),
    (2, "Fine"),
]);

// Decoder for Minolta flash modes
const_decoder!(pub DECODE_FLASH_MODE, u16, [
    (0, "Off"),
    (1, "Auto"),
    (2, "On"),
    (3, "Red-eye Reduction"),
    (4, "Fill Flash"),
]);

// Decoder for Minolta white balance settings
const_decoder!(pub DECODE_WHITE_BALANCE, u16, [
    (0, "Auto"),
    (1, "Daylight"),
    (2, "Cloudy"),
    (3, "Tungsten"),
    (4, "Fluorescent"),
    (5, "Flash"),
    (6, "Custom"),
]);

// Decoder for Minolta focus modes
const_decoder!(pub DECODE_FOCUS_MODE, u16, [
    (0, "Single Shot"),
    (1, "Continuous"),
    (2, "Manual"),
    (3, "AF-S"),
    (4, "AF-C"),
]);

// Decoder for Minolta color modes
const_decoder!(pub DECODE_COLOR_MODE, u16, [
    (0, "Standard"),
    (1, "Vivid"),
    (2, "Neutral"),
    (3, "B&W"),
    (4, "Sepia"),
]);

// Decoder for Minolta exposure modes
const_decoder!(pub DECODE_EXPOSURE_MODE, u16, [
    (0, "Auto"),
    (1, "Program"),
    (2, "Aperture Priority"),
    (3, "Shutter Priority"),
    (4, "Manual"),
]);

// Decoder for Minolta scene modes
const_decoder!(pub DECODE_SCENE_MODE, u16, [
    (0, "Standard"),
    (1, "Portrait"),
    (2, "Landscape"),
    (3, "Macro"),
    (4, "Sports"),
    (5, "Sunset"),
    (6, "Night"),
]);

/// Minolta MakerNote parser implementation
pub struct MinoltaParser;

impl Default for MinoltaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MinoltaParser {
    /// Creates a new Minolta parser instance
    pub fn new() -> Self {
        MinoltaParser
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

        // Extract value using helper functions and format based on tag type
        let formatted_value = match entry.tag_id {
            // Lens ID (0x0054) - use database lookup for lens name
            0x0054 => {
                let lens_id = extract_u16_value(entry, data, byte_order).unwrap_or(0);
                tags.insert(
                    format!("Minolta:{}", tag_name),
                    format!("0x{:04X}", lens_id),
                );
                if let Some(lens_name) = lookup_minolta_lens(lens_id) {
                    tags.insert("Minolta:LensType".to_string(), lens_name);
                }
                return;
            }
            // Flash Exposure Compensation (0x0043) - format as EV
            0x0043 => {
                let value = extract_i16_value(entry, data, byte_order).unwrap_or(0);
                let ev = value as f32 / 10.0;
                format!("{:.1} EV", ev)
            }
            // Min/Max focal length (0x0055, 0x0056) - format with "mm"
            0x0055 | 0x0056 => {
                let value = extract_u16_value(entry, data, byte_order).unwrap_or(0);
                format!("{} mm", value)
            }
            // Image size (0x0040) - convert to readable format
            0x0040 => {
                let value = extract_u16_value(entry, data, byte_order).unwrap_or(0);
                match value {
                    0 => "Full".to_string(),
                    1 => "Medium".to_string(),
                    2 => "Small".to_string(),
                    _ => "Unknown".to_string(),
                }
            }
            // Teleconverter (0x0044) - convert to readable format
            0x0044 => {
                let value = extract_u16_value(entry, data, byte_order).unwrap_or(0);
                match value {
                    0 => "None".to_string(),
                    1 => "1.4x".to_string(),
                    2 => "2.0x".to_string(),
                    _ => "Unknown".to_string(),
                }
            }
            // Macro mode (0x004B) - binary on/off
            0x004B => {
                let value = extract_u16_value(entry, data, byte_order).unwrap_or(0);
                if value > 0 {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
            // Firmware version (0x0058) - extract as string
            0x0058 => extract_string(entry, data, byte_order).unwrap_or_default(),
            // All other tags use registry decoder if available
            _ => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    TAG_REGISTRY.decode_u16(entry.tag_id, value)
                } else {
                    return;
                }
            }
        };

        tags.insert(format!("Minolta:{}", tag_name), formatted_value);
    }
}

impl MakerNoteParser for MinoltaParser {
    fn manufacturer_name(&self) -> &'static str {
        "Minolta"
    }

    fn tag_prefix(&self) -> &'static str {
        "Minolta:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 2 {
            return Err("Minolta MakerNote data too short".to_string());
        }

        // Minolta MakerNotes typically start immediately with IFD entries
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

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_minolta_lens(lens_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minolta_parser_trait() {
        let parser = MinoltaParser::new();
        assert_eq!(parser.manufacturer_name(), "Minolta");
        assert_eq!(parser.tag_prefix(), "Minolta:");
    }

    #[test]
    fn test_parse_image_quality_tag() {
        let parser = MinoltaParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // Image quality tag entry (tag=0x0041, type=3 (SHORT), count=1, value=2 (Fine))
        data.extend_from_slice(&[0x41, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Minolta:ImageQuality"), Some(&"Fine".to_string()));
    }

    #[test]
    fn test_lens_lookup() {
        let parser = MinoltaParser::new();
        assert_eq!(
            parser.lookup_lens(0x0100),
            Some("AF 50mm f/1.4".to_string())
        );
        assert_eq!(
            parser.lookup_lens(0x0200),
            Some("AF 28-70mm f/2.8 G".to_string())
        );
        assert_eq!(parser.lookup_lens(0xFFFF), None);
    }
}
