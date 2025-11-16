//! Apple (iPhone/iPad) MakerNote parser
//!
//! Parses Apple-specific EXIF MakerNote tags containing computational photography
//! settings, multi-camera data, and iOS-specific metadata.
//!
//! ## Supported Features
//! - HDR processing mode
//! - Portrait Mode and depth data
//! - Live Photo status
//! - Scene detection
//! - Multi-camera lens identification
//! - Semantic Styles (Photographic Styles)
//! - Smart HDR version
//! - Night Mode
//!
//! ## Architecture
//! Apple's MakerNotes use a proprietary binary format with Apple-specific tags.
//! Unlike traditional camera manufacturers, Apple stores significant computational
//! photography metadata.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// Apple MakerNote Tag IDs
// Note: Apple's tag structure is proprietary and reverse-engineered
const APPLE_HDR_IMAGE_TYPE: u16 = 0x000A; // HDR processing mode (3=HDR, 4=HDR+)
const APPLE_BURST_UUID: u16 = 0x000B; // Burst mode unique ID
const APPLE_CONTENT_IDENTIFIER: u16 = 0x0011; // Media identifier
const APPLE_IMAGE_UNIQUE_ID: u16 = 0x0015; // Unique image ID
const APPLE_LIVE_PHOTO_ID: u16 = 0x0017; // Live Photo video identifier
const APPLE_RUN_TIME: u16 = 0x001A; // Runtime flags
const APPLE_ACCELERATION_VECTOR: u16 = 0x001B; // Device orientation data
const APPLE_PORTRAIT_DATA: u16 = 0x0020; // Portrait Mode information
const APPLE_FOCUS_DISTANCE_RANGE: u16 = 0x002B; // Focus distance range
const APPLE_SEMANTIC_STYLE: u16 = 0x002E; // Photographic Style setting
const APPLE_FRONT_FACING_CAMERA: u16 = 0x0032; // Front/back camera flag
const APPLE_LENS_MODEL: u16 = 0x0035; // Multi-camera lens identifier
const APPLE_SMART_HDR_VERSION: u16 = 0x0037; // Smart HDR version
const APPLE_NIGHT_MODE: u16 = 0x0039; // Night Mode status
const APPLE_SCENE_DETECTION: u16 = 0x003C; // Scene detection result

// Apple signature (not always present, but useful for validation)
const APPLE_SIGNATURE: &[u8] = b"Apple iOS";

/// Decodes Apple HDR image type to human-readable string
///
/// # Arguments
/// * `value` - HDR type value from tag 0x000A
///
/// # Returns
/// Human-readable HDR mode description
fn decode_hdr_type(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "HDR".to_string(),
        3 => "Auto HDR".to_string(),
        4 => "Smart HDR".to_string(),
        5 => "Smart HDR 2".to_string(),
        6 => "Smart HDR 3".to_string(),
        7 => "Smart HDR 4".to_string(),
        8 => "Smart HDR 5".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Portrait Mode effect type
///
/// # Arguments
/// * `value` - Portrait effect value
///
/// # Returns
/// Human-readable portrait effect description
fn decode_portrait_mode(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Natural Light".to_string(),
        2 => "Studio Light".to_string(),
        3 => "Contour Light".to_string(),
        4 => "Stage Light".to_string(),
        5 => "Stage Light Mono".to_string(),
        6 => "High-Key Light Mono".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes scene detection type
///
/// # Arguments
/// * `value` - Scene type value
///
/// # Returns
/// Human-readable scene description
fn decode_scene_type(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Sunset/Sunrise".to_string(),
        2 => "Blue Sky".to_string(),
        3 => "Snow".to_string(),
        4 => "Foliage".to_string(),
        5 => "Beach".to_string(),
        6 => "Night".to_string(),
        7 => "Fireworks".to_string(),
        8 => "Food".to_string(),
        9 => "Pet".to_string(),
        10 => "Document".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes semantic style (Photographic Style)
///
/// # Arguments
/// * `value` - Style value
///
/// # Returns
/// Human-readable style name
fn decode_semantic_style(value: i16) -> String {
    match value {
        0 => "Standard".to_string(),
        1 => "Rich Contrast".to_string(),
        2 => "Vibrant".to_string(),
        3 => "Warm".to_string(),
        4 => "Cool".to_string(),
        _ => format!("Custom ({})", value),
    }
}

/// Decodes lens model for multi-camera iPhones
///
/// # Arguments
/// * `value` - Lens identifier
///
/// # Returns
/// Human-readable lens description
fn decode_lens_model(value: i16) -> String {
    match value {
        0 => "Wide (Main Camera)".to_string(),
        1 => "Telephoto".to_string(),
        2 => "Ultra Wide".to_string(),
        3 => "Front Camera".to_string(),
        4 => "Telephoto 2x".to_string(),
        5 => "Telephoto 3x".to_string(),
        6 => "Telephoto 5x".to_string(),
        _ => format!("Unknown Lens ({})", value),
    }
}

/// Extracts a 16-bit signed value from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the value
/// * `data` - Full MakerNote data buffer
/// * `byte_order` - Byte order for parsing
///
/// # Returns
/// Extracted value or None if invalid
fn extract_i16_value(entry: &IfdEntry, _data: &[u8], byte_order: ByteOrder) -> Option<i16> {
    if entry.value_count != 1 {
        return None;
    }

    // For SHORT type (value_count=1), value is inline in value_offset field
    let value = match byte_order {
        ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as i16,
        ByteOrder::BigEndian => ((entry.value_offset >> 16) & 0xFFFF) as i16,
    };

    Some(value)
}

/// Extracts a 32-bit unsigned value from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the value
/// * `data` - Full MakerNote data buffer
/// * `byte_order` - Byte order for parsing
///
/// # Returns
/// Extracted value or None if invalid
fn extract_u32_value(entry: &IfdEntry, _data: &[u8], _byte_order: ByteOrder) -> Option<u32> {
    if entry.value_count != 1 {
        return None;
    }

    Some(entry.value_offset)
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

/// Apple MakerNote parser implementation
pub struct AppleParser;

impl Default for AppleParser {
    fn default() -> Self {
        Self::new()
    }
}

impl AppleParser {
    /// Creates a new Apple parser instance
    pub fn new() -> Self {
        AppleParser
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
        let tag_id = entry.tag_id;

        match tag_id {
            APPLE_HDR_IMAGE_TYPE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Apple:HDRImageType".to_string(), decode_hdr_type(value));
                }
            }
            APPLE_BURST_UUID => {
                if let Some(uuid) = extract_string(entry, data, byte_order) {
                    tags.insert("Apple:BurstUUID".to_string(), uuid);
                }
            }
            APPLE_CONTENT_IDENTIFIER => {
                if let Some(id) = extract_string(entry, data, byte_order) {
                    tags.insert("Apple:ContentIdentifier".to_string(), id);
                }
            }
            APPLE_IMAGE_UNIQUE_ID => {
                if let Some(id) = extract_string(entry, data, byte_order) {
                    tags.insert("Apple:ImageUniqueID".to_string(), id);
                }
            }
            APPLE_LIVE_PHOTO_ID => {
                if let Some(id) = extract_string(entry, data, byte_order) {
                    tags.insert("Apple:LivePhotoVideoID".to_string(), id);
                    tags.insert("Apple:LivePhoto".to_string(), "Yes".to_string());
                }
            }
            APPLE_RUN_TIME => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert("Apple:RunTimeFlags".to_string(), format!("0x{:08X}", value));
                }
            }
            APPLE_PORTRAIT_DATA => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Apple:PortraitMode".to_string(),
                        decode_portrait_mode(value),
                    );
                }
            }
            APPLE_FOCUS_DISTANCE_RANGE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Apple:FocusDistanceRange".to_string(), value.to_string());
                }
            }
            APPLE_SEMANTIC_STYLE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Apple:SemanticStyle".to_string(),
                        decode_semantic_style(value),
                    );
                }
            }
            APPLE_FRONT_FACING_CAMERA => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let camera = if value == 1 { "Front" } else { "Back" };
                    tags.insert("Apple:FacingCamera".to_string(), camera.to_string());
                }
            }
            APPLE_LENS_MODEL => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Apple:LensModel".to_string(), decode_lens_model(value));
                }
            }
            APPLE_SMART_HDR_VERSION => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Apple:SmartHDRVersion".to_string(), value.to_string());
                }
            }
            APPLE_NIGHT_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Apple:NightMode".to_string(), status.to_string());
                }
            }
            APPLE_SCENE_DETECTION => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Apple:SceneDetection".to_string(), decode_scene_type(value));
                }
            }
            _ => {
                // Unknown tag - skip or log for debugging
            }
        }
    }
}

impl MakerNoteParser for AppleParser {
    fn manufacturer_name(&self) -> &'static str {
        "Apple"
    }

    fn tag_prefix(&self) -> &'static str {
        "Apple:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 10 {
            return Err("Apple MakerNote data too short".to_string());
        }

        // Apple MakerNotes don't always have a consistent header
        // Some start with "Apple iOS", others are just IFD data
        let ifd_offset = if data.len() >= 9 && &data[0..9] == APPLE_SIGNATURE {
            // Skip signature and any padding
            10
        } else {
            // Assume IFD starts immediately
            0
        };

        if ifd_offset + 2 > data.len() {
            return Err("Invalid IFD offset".to_string());
        }

        // Read number of IFD entries
        let entry_count = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([data[ifd_offset], data[ifd_offset + 1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([data[ifd_offset], data[ifd_offset + 1]]),
        };

        if entry_count == 0 || entry_count > 500 {
            return Err(format!(
                "Invalid entry count: {} (expected 1-500)",
                entry_count
            ));
        }

        // Parse each IFD entry
        let entry_size = 12; // Standard IFD entry size
        let mut offset = ifd_offset + 2;

        for _ in 0..entry_count {
            if offset + entry_size > data.len() {
                break;
            }

            // Parse IFD entry manually
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

    fn validate_header(&self, data: &[u8]) -> bool {
        // Accept data with or without Apple signature
        // Many Apple MakerNotes don't have a consistent header
        if data.len() >= 9 && &data[0..9] == APPLE_SIGNATURE {
            return true;
        }

        // Also accept if it looks like valid IFD data
        if data.len() >= 2 {
            let entry_count = u16::from_le_bytes([data[0], data[1]]);
            if entry_count > 0 && entry_count < 500 {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_hdr_type() {
        assert_eq!(decode_hdr_type(0), "Off");
        assert_eq!(decode_hdr_type(1), "HDR");
        assert_eq!(decode_hdr_type(4), "Smart HDR");
        assert_eq!(decode_hdr_type(8), "Smart HDR 5");
    }

    #[test]
    fn test_decode_portrait_mode() {
        assert_eq!(decode_portrait_mode(0), "Off");
        assert_eq!(decode_portrait_mode(1), "Natural Light");
        assert_eq!(decode_portrait_mode(4), "Stage Light");
    }

    #[test]
    fn test_decode_scene_type() {
        assert_eq!(decode_scene_type(0), "None");
        assert_eq!(decode_scene_type(6), "Night");
        assert_eq!(decode_scene_type(8), "Food");
    }

    #[test]
    fn test_decode_semantic_style() {
        assert_eq!(decode_semantic_style(0), "Standard");
        assert_eq!(decode_semantic_style(2), "Vibrant");
    }

    #[test]
    fn test_decode_lens_model() {
        assert_eq!(decode_lens_model(0), "Wide (Main Camera)");
        assert_eq!(decode_lens_model(1), "Telephoto");
        assert_eq!(decode_lens_model(2), "Ultra Wide");
        assert_eq!(decode_lens_model(6), "Telephoto 5x");
    }

    #[test]
    fn test_apple_parser_trait() {
        let parser = AppleParser::new();
        assert_eq!(parser.manufacturer_name(), "Apple");
        assert_eq!(parser.tag_prefix(), "Apple:");
    }

    #[test]
    fn test_validate_header_with_signature() {
        let parser = AppleParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"Apple iOS");
        data.extend_from_slice(&[0x05, 0x00]); // 5 entries

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_validate_header_without_signature() {
        let parser = AppleParser::new();
        let data = vec![0x05, 0x00]; // Just entry count

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_parse_hdr_tag() {
        let parser = AppleParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // HDR tag entry (tag=0x000A, type=3 (SHORT), count=1, value=4 (Smart HDR))
        data.extend_from_slice(&[0x0A, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Value: 4 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(
            tags.get("Apple:HDRImageType"),
            Some(&"Smart HDR".to_string())
        );
    }
}
