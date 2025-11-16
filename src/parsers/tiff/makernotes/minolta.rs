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
use std::collections::HashMap;

use super::minolta_lens_database::lookup_minolta_lens;
use super::shared::MakerNoteParser;

// Minolta MakerNote Tag IDs
const MINOLTA_CAMERA_SETTINGS_OLD: u16 = 0x0001; // Camera settings (old models)
const MINOLTA_CAMERA_SETTINGS: u16 = 0x0003; // Camera settings array
const MINOLTA_IMAGE_SIZE: u16 = 0x0040; // Image dimensions
const MINOLTA_IMAGE_QUALITY: u16 = 0x0041; // Quality setting
const MINOLTA_FLASH_MODE: u16 = 0x0042; // Flash mode
const MINOLTA_FLASH_EXPOSURE_COMP: u16 = 0x0043; // Flash compensation
const MINOLTA_TELECONVERTER: u16 = 0x0044; // Teleconverter used
const MINOLTA_WHITE_BALANCE: u16 = 0x0045; // White balance mode
const MINOLTA_BRIGHTNESS: u16 = 0x0046; // Scene brightness value
const MINOLTA_FOCUS_MODE: u16 = 0x0047; // Manual/Auto focus
const MINOLTA_FOCUS_DISTANCE: u16 = 0x0048; // Focus distance
const MINOLTA_ZOOM_POSITION: u16 = 0x004A; // Zoom position
const MINOLTA_MACRO_MODE: u16 = 0x004B; // Macro mode on/off
const MINOLTA_SHARPNESS: u16 = 0x004C; // Sharpness setting
const MINOLTA_CONTRAST: u16 = 0x004D; // Contrast setting
const MINOLTA_SATURATION: u16 = 0x004E; // Color saturation
const MINOLTA_COLOR_MODE: u16 = 0x0050; // Color mode
const MINOLTA_SCENE_MODE: u16 = 0x0052; // Scene mode selection
const MINOLTA_EXPOSURE_MODE: u16 = 0x0053; // Exposure program mode
const MINOLTA_LENS_ID: u16 = 0x0054; // Lens model ID
const MINOLTA_MIN_FOCAL_LENGTH: u16 = 0x0055; // Min focal length
const MINOLTA_MAX_FOCAL_LENGTH: u16 = 0x0056; // Max focal length
const MINOLTA_FIRMWARE_VERSION: u16 = 0x0058; // Camera firmware
const MINOLTA_AF_POINTS: u16 = 0x0059; // AF points used

/// Decodes Minolta image quality setting
///
/// # Arguments
/// * `value` - Image quality value
///
/// # Returns
/// Human-readable quality description
fn decode_image_quality(value: u16) -> String {
    match value {
        0 => "Raw".to_string(),
        1 => "Super Fine".to_string(),
        2 => "Fine".to_string(),
        3 => "Standard".to_string(),
        4 => "Economy".to_string(),
        5 => "Extra Fine".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Minolta flash mode
///
/// # Arguments
/// * `value` - Flash mode value
///
/// # Returns
/// Human-readable flash mode
fn decode_flash_mode(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "On".to_string(),
        2 => "Off".to_string(),
        3 => "Red-eye Reduction".to_string(),
        4 => "Slow Sync".to_string(),
        5 => "Rear Curtain Sync".to_string(),
        6 => "Fill Flash".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Minolta white balance mode
///
/// # Arguments
/// * `value` - White balance value
///
/// # Returns
/// Human-readable white balance mode
fn decode_white_balance(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Daylight".to_string(),
        2 => "Cloudy".to_string(),
        3 => "Tungsten".to_string(),
        4 => "Fluorescent".to_string(),
        5 => "Flash".to_string(),
        6 => "Shade".to_string(),
        7 => "Custom".to_string(),
        8 => "Kelvin".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Minolta focus mode
///
/// # Arguments
/// * `value` - Focus mode value
///
/// # Returns
/// Human-readable focus mode
fn decode_focus_mode(value: u16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Manual".to_string(),
        2 => "AF-C (Continuous)".to_string(),
        3 => "AF-S (Single)".to_string(),
        4 => "AF-A (Automatic)".to_string(),
        5 => "DMF (Direct Manual Focus)".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Minolta color mode
///
/// # Arguments
/// * `value` - Color mode value
///
/// # Returns
/// Human-readable color mode
fn decode_color_mode(value: u16) -> String {
    match value {
        0 => "Natural".to_string(),
        1 => "Vivid".to_string(),
        2 => "Portrait".to_string(),
        3 => "Landscape".to_string(),
        4 => "Black & White".to_string(),
        5 => "Adobe RGB".to_string(),
        6 => "Neutral".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Minolta exposure mode
///
/// # Arguments
/// * `value` - Exposure mode value
///
/// # Returns
/// Human-readable exposure mode
fn decode_exposure_mode(value: u16) -> String {
    match value {
        0 => "Program".to_string(),
        1 => "Aperture Priority".to_string(),
        2 => "Shutter Priority".to_string(),
        3 => "Manual".to_string(),
        4 => "Auto".to_string(),
        5 => "Portrait".to_string(),
        6 => "Landscape".to_string(),
        7 => "Sports".to_string(),
        8 => "Night Portrait".to_string(),
        9 => "Macro".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Minolta scene mode
///
/// # Arguments
/// * `value` - Scene mode value
///
/// # Returns
/// Human-readable scene mode
fn decode_scene_mode(value: u16) -> String {
    match value {
        0 => "Standard".to_string(),
        1 => "Portrait".to_string(),
        2 => "Landscape".to_string(),
        3 => "Sports".to_string(),
        4 => "Sunset".to_string(),
        5 => "Night View".to_string(),
        6 => "Night Portrait".to_string(),
        7 => "Fireworks".to_string(),
        8 => "Food".to_string(),
        9 => "Text".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

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
        let tag_id = entry.tag_id;

        match tag_id {
            MINOLTA_IMAGE_QUALITY => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Minolta:ImageQuality".to_string(),
                        decode_image_quality(value),
                    );
                }
            }
            MINOLTA_FLASH_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Minolta:FlashMode".to_string(), decode_flash_mode(value));
                }
            }
            MINOLTA_FLASH_EXPOSURE_COMP => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let ev = value as f32 / 10.0;
                    tags.insert(
                        "Minolta:FlashExposureComp".to_string(),
                        format!("{:.1} EV", ev),
                    );
                }
            }
            MINOLTA_WHITE_BALANCE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Minolta:WhiteBalance".to_string(),
                        decode_white_balance(value),
                    );
                }
            }
            MINOLTA_FOCUS_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Minolta:FocusMode".to_string(), decode_focus_mode(value));
                }
            }
            MINOLTA_MACRO_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let mode = if value > 0 { "On" } else { "Off" };
                    tags.insert("Minolta:MacroMode".to_string(), mode.to_string());
                }
            }
            MINOLTA_SHARPNESS => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Minolta:Sharpness".to_string(), value.to_string());
                }
            }
            MINOLTA_CONTRAST => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Minolta:Contrast".to_string(), value.to_string());
                }
            }
            MINOLTA_SATURATION => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Minolta:Saturation".to_string(), value.to_string());
                }
            }
            MINOLTA_COLOR_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Minolta:ColorMode".to_string(), decode_color_mode(value));
                }
            }
            MINOLTA_SCENE_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert("Minolta:SceneMode".to_string(), decode_scene_mode(value));
                }
            }
            MINOLTA_EXPOSURE_MODE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Minolta:ExposureMode".to_string(),
                        decode_exposure_mode(value),
                    );
                }
            }
            MINOLTA_LENS_ID => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    // Store the lens ID
                    tags.insert("Minolta:LensID".to_string(), format!("0x{:04X}", value));

                    // Lookup lens name from database
                    if let Some(lens_name) = lookup_minolta_lens(value) {
                        tags.insert("Minolta:LensType".to_string(), lens_name);
                    }
                }
            }
            MINOLTA_MIN_FOCAL_LENGTH => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Minolta:MinFocalLength".to_string(),
                        format!("{} mm", value),
                    );
                }
            }
            MINOLTA_MAX_FOCAL_LENGTH => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    tags.insert(
                        "Minolta:MaxFocalLength".to_string(),
                        format!("{} mm", value),
                    );
                }
            }
            MINOLTA_FIRMWARE_VERSION => {
                if let Some(version) = extract_string(entry, data, byte_order) {
                    tags.insert("Minolta:FirmwareVersion".to_string(), version);
                }
            }
            MINOLTA_IMAGE_SIZE => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let size_str = match value {
                        0 => "Full",
                        1 => "Medium",
                        2 => "Small",
                        _ => "Unknown",
                    };
                    tags.insert("Minolta:ImageSize".to_string(), size_str.to_string());
                }
            }
            MINOLTA_TELECONVERTER => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    let tc = match value {
                        0 => "None",
                        1 => "1.4x",
                        2 => "2.0x",
                        _ => "Unknown",
                    };
                    tags.insert("Minolta:Teleconverter".to_string(), tc.to_string());
                }
            }
            _ => {
                // Unknown tag - skip for now
            }
        }
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

        // Minolta MakerNotes typically start immediately with IFD
        // Some models may have a small header, but most don't
        let ifd_offset = 0;

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

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_minolta_lens(lens_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_image_quality() {
        assert_eq!(decode_image_quality(0), "Raw");
        assert_eq!(decode_image_quality(2), "Fine");
        assert_eq!(decode_image_quality(5), "Extra Fine");
    }

    #[test]
    fn test_decode_flash_mode() {
        assert_eq!(decode_flash_mode(0), "Auto");
        assert_eq!(decode_flash_mode(1), "On");
        assert_eq!(decode_flash_mode(3), "Red-eye Reduction");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0), "Auto");
        assert_eq!(decode_white_balance(3), "Tungsten");
        assert_eq!(decode_white_balance(7), "Custom");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(0), "Auto");
        assert_eq!(decode_focus_mode(2), "AF-C (Continuous)");
        assert_eq!(decode_focus_mode(5), "DMF (Direct Manual Focus)");
    }

    #[test]
    fn test_decode_color_mode() {
        assert_eq!(decode_color_mode(0), "Natural");
        assert_eq!(decode_color_mode(4), "Black & White");
    }

    #[test]
    fn test_decode_exposure_mode() {
        assert_eq!(decode_exposure_mode(0), "Program");
        assert_eq!(decode_exposure_mode(3), "Manual");
        assert_eq!(decode_exposure_mode(7), "Sports");
    }

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
