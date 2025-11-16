//! Samsung MakerNote parser
//!
//! Parses Samsung Galaxy-specific EXIF MakerNote tags containing computational
//! photography settings, AI features, and Samsung-exclusive camera modes.
//!
//! ## Supported Features
//! - Scene Optimizer AI detection
//! - Single Take mode information
//! - Expert RAW processing data
//! - Multi-Frame Processing details
//! - Director's View settings
//! - Pro mode parameters
//! - Object tracking data
//! - Night mode settings
//!
//! ## Architecture
//! Samsung's MakerNotes use a proprietary binary format with Samsung-specific tags.
//! Many Galaxy devices include extensive AI processing metadata and multi-camera
//! coordination data.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// Samsung MakerNote Tag IDs
// Note: Samsung's tag structure is proprietary and reverse-engineered
const SAMSUNG_SCENE_OPTIMIZER: u16 = 0x0001; // Scene Optimizer AI mode
const SAMSUNG_SCENE_TYPE: u16 = 0x0002; // Detected scene type
const SAMSUNG_SINGLE_TAKE: u16 = 0x0005; // Single Take mode status
const SAMSUNG_SINGLE_TAKE_FRAME: u16 = 0x0006; // Frame number in Single Take
const SAMSUNG_EXPERT_RAW: u16 = 0x0008; // Expert RAW mode status
const SAMSUNG_MULTI_FRAME_NR: u16 = 0x000A; // Multi-frame noise reduction
const SAMSUNG_DIRECTORS_VIEW: u16 = 0x000C; // Director's View recording
const SAMSUNG_PRO_MODE: u16 = 0x000E; // Pro mode manual settings
const SAMSUNG_OBJECT_TRACKING: u16 = 0x0010; // Object tracking status
const SAMSUNG_NIGHT_MODE: u16 = 0x0012; // Night mode enhancement
const SAMSUNG_NIGHT_HYPERLAPSE: u16 = 0x0014; // Night Hyperlapse mode
const SAMSUNG_SUPER_STEADY: u16 = 0x0016; // Super Steady stabilization
const SAMSUNG_FOOD_MODE: u16 = 0x0018; // Food mode optimization
const SAMSUNG_PORTRAIT_EFFECT: u16 = 0x001A; // Portrait mode effect
const SAMSUNG_LENS_TYPE: u16 = 0x001C; // Multi-camera lens selection
const SAMSUNG_ZOOM_LEVEL: u16 = 0x001E; // Digital zoom level (10x = 100)

// Samsung signature for validation
const SAMSUNG_SIGNATURE: &[u8] = b"Samsung";

/// Decodes Samsung Scene Optimizer status
///
/// # Arguments
/// * `value` - Scene Optimizer mode value
///
/// # Returns
/// Human-readable mode description
fn decode_scene_optimizer(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "On".to_string(),
        2 => "Auto".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Samsung AI scene detection result
///
/// # Arguments
/// * `value` - Scene type value
///
/// # Returns
/// Human-readable scene description
fn decode_scene_type(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Food".to_string(),
        2 => "Sunset".to_string(),
        3 => "Blue Sky".to_string(),
        4 => "Snow".to_string(),
        5 => "Greenery".to_string(),
        6 => "Beach".to_string(),
        7 => "Night".to_string(),
        8 => "Flower".to_string(),
        9 => "Indoor".to_string(),
        10 => "Pet".to_string(),
        11 => "Text".to_string(),
        12 => "Backlit".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Single Take mode status
///
/// # Arguments
/// * `value` - Single Take status
///
/// # Returns
/// Human-readable status
fn decode_single_take(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Recording".to_string(),
        2 => "Processing".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Portrait mode effect type
///
/// # Arguments
/// * `value` - Portrait effect value
///
/// # Returns
/// Human-readable effect description
fn decode_portrait_effect(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Blur".to_string(),
        2 => "Spin".to_string(),
        3 => "Zoom".to_string(),
        4 => "Color Point".to_string(),
        5 => "Glitch".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes lens type for multi-camera Galaxy devices
///
/// # Arguments
/// * `value` - Lens identifier
///
/// # Returns
/// Human-readable lens description
fn decode_lens_type(value: i16) -> String {
    match value {
        0 => "Wide (Main)".to_string(),
        1 => "Ultra Wide".to_string(),
        2 => "Telephoto".to_string(),
        3 => "Front Camera".to_string(),
        4 => "Telephoto 3x".to_string(),
        5 => "Telephoto 10x".to_string(),
        _ => format!("Unknown Lens ({})", value),
    }
}

/// Decodes zoom level
///
/// # Arguments
/// * `value` - Zoom level (10 = 1.0x, 100 = 10.0x)
///
/// # Returns
/// Human-readable zoom level
fn decode_zoom_level(value: i16) -> String {
    if value <= 0 {
        return "1.0x".to_string();
    }
    let zoom = value as f32 / 10.0;
    format!("{:.1}x", zoom)
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

    // For SHORT type (count=1), value is inline in value_offset field
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

/// Samsung MakerNote parser implementation
pub struct SamsungParser;

impl Default for SamsungParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SamsungParser {
    /// Creates a new Samsung parser instance
    pub fn new() -> Self {
        SamsungParser
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
            SAMSUNG_SCENE_OPTIMIZER => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Samsung:SceneOptimizer".to_string(),
                        decode_scene_optimizer(value),
                    );
                }
            }
            SAMSUNG_SCENE_TYPE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Samsung:SceneType".to_string(), decode_scene_type(value));
                }
            }
            SAMSUNG_SINGLE_TAKE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Samsung:SingleTake".to_string(), decode_single_take(value));
                }
            }
            SAMSUNG_SINGLE_TAKE_FRAME => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Samsung:SingleTakeFrame".to_string(), value.to_string());
                }
            }
            SAMSUNG_EXPERT_RAW => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Samsung:ExpertRAW".to_string(), status.to_string());
                }
            }
            SAMSUNG_MULTI_FRAME_NR => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert(
                        "Samsung:MultiFrameNoiseReduction".to_string(),
                        status.to_string(),
                    );
                }
            }
            SAMSUNG_DIRECTORS_VIEW => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Samsung:DirectorsView".to_string(), status.to_string());
                }
            }
            SAMSUNG_PRO_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Samsung:ProMode".to_string(), status.to_string());
                }
            }
            SAMSUNG_OBJECT_TRACKING => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Samsung:ObjectTracking".to_string(), status.to_string());
                }
            }
            SAMSUNG_NIGHT_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Samsung:NightMode".to_string(), status.to_string());
                }
            }
            SAMSUNG_NIGHT_HYPERLAPSE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Samsung:NightHyperlapse".to_string(), status.to_string());
                }
            }
            SAMSUNG_SUPER_STEADY => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Samsung:SuperSteady".to_string(), status.to_string());
                }
            }
            SAMSUNG_FOOD_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Samsung:FoodMode".to_string(), status.to_string());
                }
            }
            SAMSUNG_PORTRAIT_EFFECT => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Samsung:PortraitEffect".to_string(),
                        decode_portrait_effect(value),
                    );
                }
            }
            SAMSUNG_LENS_TYPE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Samsung:LensType".to_string(), decode_lens_type(value));
                }
            }
            SAMSUNG_ZOOM_LEVEL => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Samsung:ZoomLevel".to_string(), decode_zoom_level(value));
                }
            }
            _ => {
                // Unknown tag - skip or log for debugging
            }
        }
    }
}

impl MakerNoteParser for SamsungParser {
    fn manufacturer_name(&self) -> &'static str {
        "Samsung"
    }

    fn tag_prefix(&self) -> &'static str {
        "Samsung:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Samsung MakerNote data too short".to_string());
        }

        // Samsung MakerNotes may start with "Samsung" signature
        let ifd_offset = if data.len() >= 7 && &data[0..7] == SAMSUNG_SIGNATURE {
            // Skip signature and padding (usually 8 bytes total)
            8
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
        // Accept data with or without Samsung signature
        if data.len() >= 7 && &data[0..7] == SAMSUNG_SIGNATURE {
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
    fn test_decode_scene_optimizer() {
        assert_eq!(decode_scene_optimizer(0), "Off");
        assert_eq!(decode_scene_optimizer(1), "On");
        assert_eq!(decode_scene_optimizer(2), "Auto");
    }

    #[test]
    fn test_decode_scene_type() {
        assert_eq!(decode_scene_type(0), "None");
        assert_eq!(decode_scene_type(1), "Food");
        assert_eq!(decode_scene_type(7), "Night");
    }

    #[test]
    fn test_decode_single_take() {
        assert_eq!(decode_single_take(0), "Off");
        assert_eq!(decode_single_take(1), "Recording");
    }

    #[test]
    fn test_decode_portrait_effect() {
        assert_eq!(decode_portrait_effect(0), "None");
        assert_eq!(decode_portrait_effect(1), "Blur");
        assert_eq!(decode_portrait_effect(4), "Color Point");
    }

    #[test]
    fn test_decode_lens_type() {
        assert_eq!(decode_lens_type(0), "Wide (Main)");
        assert_eq!(decode_lens_type(1), "Ultra Wide");
        assert_eq!(decode_lens_type(5), "Telephoto 10x");
    }

    #[test]
    fn test_decode_zoom_level() {
        assert_eq!(decode_zoom_level(10), "1.0x");
        assert_eq!(decode_zoom_level(100), "10.0x");
        assert_eq!(decode_zoom_level(35), "3.5x");
    }

    #[test]
    fn test_samsung_parser_trait() {
        let parser = SamsungParser::new();
        assert_eq!(parser.manufacturer_name(), "Samsung");
        assert_eq!(parser.tag_prefix(), "Samsung:");
    }

    #[test]
    fn test_validate_header_with_signature() {
        let parser = SamsungParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"Samsung");
        data.extend_from_slice(&[0x00]); // Padding
        data.extend_from_slice(&[0x05, 0x00]); // 5 entries

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_parse_scene_optimizer_tag() {
        let parser = SamsungParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // Scene Optimizer tag entry (tag=0x0001, type=3 (SHORT), count=1, value=1 (On))
        data.extend_from_slice(&[0x01, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Samsung:SceneOptimizer"), Some(&"On".to_string()));
    }
}
