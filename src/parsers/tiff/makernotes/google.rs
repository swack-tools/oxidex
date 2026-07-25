//! Google (Pixel) MakerNote parser
//!
//! Parses Google Pixel-specific EXIF MakerNote tags containing computational
//! photography settings, AI processing data, and Pixel-exclusive features.
//!
//! ## Supported Features
//! - HDR+ processing mode
//! - Night Sight activation and exposure time
//! - Super Res Zoom level
//! - Motion Photos status
//! - Face retouching level
//! - AI-based scene detection
//! - Computational photography settings
//! - Astrophotography mode
//!
//! ## Architecture
//! Google's MakerNotes use a custom binary format with tags specific to their
//! computational photography pipeline. These tags capture the extensive AI and
//! multi-frame processing that Pixel phones perform.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::const_decoder;
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_i16_array;

// Google Pixel MakerNote Tag IDs
// Note: Google's tag structure is proprietary and reverse-engineered
const GOOGLE_HDR_PLUS_MODE: u16 = 0x0001; // HDR+ processing mode
const GOOGLE_NIGHT_SIGHT: u16 = 0x0003; // Night Sight mode status
const GOOGLE_NIGHT_SIGHT_EXPOSURE: u16 = 0x0004; // Night Sight exposure time (ms)
const GOOGLE_SUPER_RES_ZOOM: u16 = 0x0005; // Super Res Zoom level
const GOOGLE_MOTION_PHOTO_ID: u16 = 0x0007; // Motion Photo video identifier
const GOOGLE_FACE_RETOUCHING: u16 = 0x0009; // Face retouching level (0-100)
const GOOGLE_SCENE_DETECTION: u16 = 0x000B; // AI scene detection result
const GOOGLE_PORTRAIT_BLUR: u16 = 0x000D; // Portrait mode blur amount
const GOOGLE_COLOR_POP: u16 = 0x000F; // Color Pop effect status
const GOOGLE_ASTROPHOTOGRAPHY: u16 = 0x0011; // Astrophotography mode
const GOOGLE_CINEMATIC_MODE: u16 = 0x0013; // Cinematic blur mode
const GOOGLE_MAGIC_ERASER: u16 = 0x0015; // Magic Eraser applied
const GOOGLE_UNBLUR: u16 = 0x0017; // Face Unblur applied
const GOOGLE_FRAME_COUNT: u16 = 0x0019; // Number of frames merged
const GOOGLE_EXPOSURE_STACK: u16 = 0x001B; // Multi-exposure stack info

// Google signature for validation
const GOOGLE_SIGNATURE: &[u8] = b"Google";

// Decodes Google
const_decoder! {
    DECODE_HDR_PLUS_MODE, i16, [
        (0, "Off"),
        (1, "HDR+ On"),
        (2, "HDR+ Enhanced"),
        (3, "HDR+ Auto"),
        (4, "HDR+ Bracketing"),
    ]
}

// Decodes Google
const_decoder! {
    DECODE_NIGHT_SIGHT, i16, [
        (0, "Off"),
        (1, "Auto"),
        (2, "On"),
        (3, "Astrophotography"),
    ]
}

// Decodes Google
const_decoder! {
    DECODE_SCENE_TYPE, i16, [
        (0, "None"),
        (1, "Sunset"),
        (2, "Blue Sky"),
        (3, "Snow"),
        (4, "Greenery"),
        (5, "Beach"),
        (6, "Night"),
        (7, "Food"),
        (8, "Pet"),
        (9, "Flower"),
        (10, "Landmark"),
        (11, "Document"),
        (12, "Text"),
    ]
}

/// Decodes Super Res Zoom level
///
/// # Arguments
/// * `value` - Zoom level multiplier (10 = 1.0x, 20 = 2.0x, etc.)
///
/// # Returns
/// Human-readable zoom level
fn decode_super_res_zoom(value: i16) -> String {
    if value <= 0 {
        return "Off".to_string();
    }
    let zoom_level = value as f32 / 10.0;
    format!("{:.1}x", zoom_level)
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

/// Google Pixel MakerNote parser implementation
pub struct GoogleParser;

impl Default for GoogleParser {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleParser {
    /// Creates a new Google parser instance
    pub fn new() -> Self {
        GoogleParser
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
            GOOGLE_HDR_PLUS_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Google:HDRPlusMode".to_string(),
                        DECODE_HDR_PLUS_MODE.decode(value),
                    );
                }
            }
            GOOGLE_NIGHT_SIGHT => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Google:NightSight".to_string(),
                        DECODE_NIGHT_SIGHT.decode(value),
                    );
                }
            }
            GOOGLE_NIGHT_SIGHT_EXPOSURE => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert(
                        "Google:NightSightExposureTime".to_string(),
                        format!("{} ms", value),
                    );
                }
            }
            GOOGLE_SUPER_RES_ZOOM => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Google:SuperResZoom".to_string(),
                        decode_super_res_zoom(value),
                    );
                }
            }
            GOOGLE_MOTION_PHOTO_ID => {
                if let Some(id) = extract_string(entry, data, byte_order) {
                    tags.insert("Google:MotionPhotoID".to_string(), id);
                    tags.insert("Google:MotionPhoto".to_string(), "Yes".to_string());
                }
            }
            GOOGLE_FACE_RETOUCHING => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Google:FaceRetouching".to_string(), value.to_string());
                }
            }
            GOOGLE_SCENE_DETECTION => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Google:SceneDetection".to_string(),
                        DECODE_SCENE_TYPE.decode(value),
                    );
                }
            }
            GOOGLE_PORTRAIT_BLUR => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Google:PortraitBlur".to_string(), value.to_string());
                }
            }
            GOOGLE_COLOR_POP => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Google:ColorPop".to_string(), status.to_string());
                }
            }
            GOOGLE_ASTROPHOTOGRAPHY => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Google:Astrophotography".to_string(), status.to_string());
                }
            }
            GOOGLE_CINEMATIC_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Google:CinematicMode".to_string(), status.to_string());
                }
            }
            GOOGLE_MAGIC_ERASER => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "Applied" } else { "Not Applied" };
                    tags.insert("Google:MagicEraser".to_string(), status.to_string());
                }
            }
            GOOGLE_UNBLUR => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "Applied" } else { "Not Applied" };
                    tags.insert("Google:FaceUnblur".to_string(), status.to_string());
                }
            }
            GOOGLE_FRAME_COUNT => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Google:MergedFrameCount".to_string(), value.to_string());
                }
            }
            GOOGLE_EXPOSURE_STACK => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Google:ExposureStack".to_string(), value.to_string());
                }
            }
            _ => {
                // Unknown tag - skip or log for debugging
            }
        }
    }
}

impl MakerNoteParser for GoogleParser {
    fn manufacturer_name(&self) -> &'static str {
        "Google"
    }

    fn tag_prefix(&self) -> &'static str {
        "Google:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Google MakerNote data too short".to_string());
        }

        // Google MakerNotes may start with "Google" signature
        let ifd_offset = if data.len() >= 6 && &data[0..6] == GOOGLE_SIGNATURE {
            // Skip signature and padding (usually 8 bytes total)
            8
        } else {
            // Assume IFD starts immediately
            0
        };

        if ifd_offset + 2 > data.len() {
            return Err("Invalid IFD offset".to_string());
        }

        // Read number of IFD entries using EndianReader
        let reader = EndianReader::new(data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(ifd_offset).unwrap_or(0);

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

            // Parse IFD entry using EndianReader
            let entry_data = &data[offset..offset + entry_size];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

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

            self.parse_entry(&entry, data, byte_order, tags);

            offset += entry_size;
        }

        Ok(())
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Accept data with or without Google signature
        if data.len() >= 6 && &data[0..6] == GOOGLE_SIGNATURE {
            return true;
        }

        // Also accept if it looks like valid IFD data
        if data.len() >= 2 {
            let reader = EndianReader::little_endian(data);
            let entry_count = reader.u16_at(0).unwrap_or(0);
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
    fn test_decode_hdr_plus_mode() {
        assert_eq!(DECODE_HDR_PLUS_MODE.decode(0), "Off");
        assert_eq!(DECODE_HDR_PLUS_MODE.decode(1), "HDR+ On");
        assert_eq!(DECODE_HDR_PLUS_MODE.decode(2), "HDR+ Enhanced");
    }

    #[test]
    fn test_decode_night_sight() {
        assert_eq!(DECODE_NIGHT_SIGHT.decode(0), "Off");
        assert_eq!(DECODE_NIGHT_SIGHT.decode(2), "On");
        assert_eq!(DECODE_NIGHT_SIGHT.decode(3), "Astrophotography");
    }

    #[test]
    fn test_decode_scene_type() {
        assert_eq!(DECODE_SCENE_TYPE.decode(0), "None");
        assert_eq!(DECODE_SCENE_TYPE.decode(7), "Food");
        assert_eq!(DECODE_SCENE_TYPE.decode(11), "Document");
    }

    #[test]
    fn test_decode_super_res_zoom() {
        assert_eq!(decode_super_res_zoom(0), "Off");
        assert_eq!(decode_super_res_zoom(10), "1.0x");
        assert_eq!(decode_super_res_zoom(20), "2.0x");
        assert_eq!(decode_super_res_zoom(75), "7.5x");
    }

    #[test]
    fn test_google_parser_trait() {
        let parser = GoogleParser::new();
        assert_eq!(parser.manufacturer_name(), "Google");
        assert_eq!(parser.tag_prefix(), "Google:");
    }

    #[test]
    fn test_validate_header_with_signature() {
        let parser = GoogleParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"Google");
        data.extend_from_slice(&[0x00, 0x00]); // Padding
        data.extend_from_slice(&[0x05, 0x00]); // 5 entries

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_validate_header_without_signature() {
        let parser = GoogleParser::new();
        let data = vec![0x05, 0x00]; // Just entry count

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_parse_hdr_plus_tag() {
        let parser = GoogleParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // HDR+ tag entry (tag=0x0001, type=3 (SHORT), count=1, value=2 (Enhanced))
        data.extend_from_slice(&[0x01, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(
            tags.get("Google:HDRPlusMode"),
            Some(&"HDR+ Enhanced".to_string())
        );
    }

    #[test]
    fn test_parse_night_sight_tag() {
        let parser = GoogleParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // Night Sight tag entry (tag=0x0003, type=3 (SHORT), count=1, value=2 (On))
        data.extend_from_slice(&[0x03, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Google:NightSight"), Some(&"On".to_string()));
    }
}
