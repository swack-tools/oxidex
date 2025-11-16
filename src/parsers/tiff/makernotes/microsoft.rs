//! Microsoft (Lumia) MakerNote parser
//!
//! Parses Microsoft Lumia-specific EXIF MakerNote tags containing PureView
//! imaging data, computational photography settings, and Windows Phone camera features.
//!
//! ## Supported Features
//! - Rich Capture mode (HDR + flash variants)
//! - Living Images (video + still)
//! - Dynamic Flash blending
//! - Refocus depth data
//! - PureView oversampling info
//! - Lumia Creative Studio effects
//! - 4K video recording data
//! - Audio recording settings
//!
//! ## Architecture
//! Microsoft's Lumia MakerNotes use a proprietary binary format specific to Windows
//! Phone camera pipeline. These tags capture the sophisticated computational photography
//! features introduced in the Lumia 1020, 950, and other high-end models.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// Microsoft Lumia MakerNote Tag IDs
// Note: Microsoft's tag structure is proprietary and reverse-engineered
const MICROSOFT_RICH_CAPTURE: u16 = 0x0001; // Rich Capture mode status
const MICROSOFT_RICH_CAPTURE_MODE: u16 = 0x0002; // Rich Capture variant
const MICROSOFT_LIVING_IMAGE: u16 = 0x0004; // Living Image video ID
const MICROSOFT_DYNAMIC_FLASH: u16 = 0x0006; // Dynamic Flash status
const MICROSOFT_REFOCUS: u16 = 0x0008; // Refocus depth data available
const MICROSOFT_REFOCUS_DEPTH: u16 = 0x0009; // Depth map resolution
const MICROSOFT_PUREVIEW_MODE: u16 = 0x000B; // PureView oversampling mode
const MICROSOFT_PUREVIEW_RESOLUTION: u16 = 0x000C; // Full sensor resolution
const MICROSOFT_CREATIVE_EFFECT: u16 = 0x000E; // Lumia Creative effect applied
const MICROSOFT_VIDEO_4K: u16 = 0x0010; // 4K video recording
const MICROSOFT_AUDIO_RICHRECORD: u16 = 0x0012; // Rich Recording audio
const MICROSOFT_STABILIZATION: u16 = 0x0014; // OIS stabilization status
const MICROSOFT_AUTO_HDR: u16 = 0x0016; // Auto HDR mode
const MICROSOFT_PANORAMA_MODE: u16 = 0x0018; // Panorama stitching
const MICROSOFT_LENS_TYPE: u16 = 0x001A; // Lens attachment type

// Microsoft signature for validation
const MICROSOFT_SIGNATURE: &[u8] = b"Microsoft";

/// Decodes Rich Capture mode
///
/// # Arguments
/// * `value` - Rich Capture status value
///
/// # Returns
/// Human-readable mode description
fn decode_rich_capture(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "On".to_string(),
        2 => "Auto".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Rich Capture variant type
///
/// # Arguments
/// * `value` - Rich Capture mode variant
///
/// # Returns
/// Human-readable variant description
fn decode_rich_capture_mode(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "HDR".to_string(),
        2 => "HDR + Flash".to_string(),
        3 => "Flash Variants".to_string(),
        4 => "Motion Blur Removal".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Dynamic Flash status
///
/// # Arguments
/// * `value` - Dynamic Flash mode
///
/// # Returns
/// Human-readable status
fn decode_dynamic_flash(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Flash + No Flash Blend".to_string(),
        2 => "Multi-Flash Blend".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes PureView oversampling mode
///
/// # Arguments
/// * `value` - PureView mode value
///
/// # Returns
/// Human-readable mode description
fn decode_pureview_mode(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "5MP Oversampled".to_string(),
        2 => "8MP Oversampled".to_string(),
        3 => "Full Resolution".to_string(),
        4 => "Lossless Zoom".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Creative Studio effect type
///
/// # Arguments
/// * `value` - Effect type value
///
/// # Returns
/// Human-readable effect description
fn decode_creative_effect(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Black & White".to_string(),
        2 => "Sepia".to_string(),
        3 => "Vintage".to_string(),
        4 => "Vivid".to_string(),
        5 => "Warm".to_string(),
        6 => "Cool".to_string(),
        7 => "Stamp".to_string(),
        8 => "Posterize".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes lens attachment type
///
/// # Arguments
/// * `value` - Lens type identifier
///
/// # Returns
/// Human-readable lens description
fn decode_lens_type(value: i16) -> String {
    match value {
        0 => "Built-in".to_string(),
        1 => "Wide Angle Attachment".to_string(),
        2 => "Telephoto Attachment".to_string(),
        3 => "Macro Attachment".to_string(),
        _ => format!("Unknown ({})", value),
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

/// Microsoft Lumia MakerNote parser implementation
pub struct MicrosoftParser;

impl Default for MicrosoftParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MicrosoftParser {
    /// Creates a new Microsoft parser instance
    pub fn new() -> Self {
        MicrosoftParser
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
            MICROSOFT_RICH_CAPTURE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Microsoft:RichCapture".to_string(),
                        decode_rich_capture(value),
                    );
                }
            }
            MICROSOFT_RICH_CAPTURE_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Microsoft:RichCaptureMode".to_string(),
                        decode_rich_capture_mode(value),
                    );
                }
            }
            MICROSOFT_LIVING_IMAGE => {
                if let Some(id) = extract_string(entry, data, byte_order) {
                    tags.insert("Microsoft:LivingImageID".to_string(), id);
                    tags.insert("Microsoft:LivingImage".to_string(), "Yes".to_string());
                }
            }
            MICROSOFT_DYNAMIC_FLASH => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Microsoft:DynamicFlash".to_string(),
                        decode_dynamic_flash(value),
                    );
                }
            }
            MICROSOFT_REFOCUS => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 {
                        "Available"
                    } else {
                        "Not Available"
                    };
                    tags.insert("Microsoft:Refocus".to_string(), status.to_string());
                }
            }
            MICROSOFT_REFOCUS_DEPTH => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert(
                        "Microsoft:RefocusDepthResolution".to_string(),
                        format!("{}x{}", value >> 16, value & 0xFFFF),
                    );
                }
            }
            MICROSOFT_PUREVIEW_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Microsoft:PureViewMode".to_string(),
                        decode_pureview_mode(value),
                    );
                }
            }
            MICROSOFT_PUREVIEW_RESOLUTION => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert(
                        "Microsoft:PureViewFullResolution".to_string(),
                        format!("{}x{}", value >> 16, value & 0xFFFF),
                    );
                }
            }
            MICROSOFT_CREATIVE_EFFECT => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Microsoft:CreativeEffect".to_string(),
                        decode_creative_effect(value),
                    );
                }
            }
            MICROSOFT_VIDEO_4K => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Microsoft:Video4K".to_string(), status.to_string());
                }
            }
            MICROSOFT_AUDIO_RICHRECORD => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert(
                        "Microsoft:RichRecordingAudio".to_string(),
                        status.to_string(),
                    );
                }
            }
            MICROSOFT_STABILIZATION => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On (OIS)" } else { "Off" };
                    tags.insert(
                        "Microsoft:OpticalStabilization".to_string(),
                        status.to_string(),
                    );
                }
            }
            MICROSOFT_AUTO_HDR => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Microsoft:AutoHDR".to_string(), status.to_string());
                }
            }
            MICROSOFT_PANORAMA_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let status = if value > 0 { "On" } else { "Off" };
                    tags.insert("Microsoft:PanoramaMode".to_string(), status.to_string());
                }
            }
            MICROSOFT_LENS_TYPE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert("Microsoft:LensType".to_string(), decode_lens_type(value));
                }
            }
            _ => {
                // Unknown tag - skip or log for debugging
            }
        }
    }
}

impl MakerNoteParser for MicrosoftParser {
    fn manufacturer_name(&self) -> &'static str {
        "Microsoft"
    }

    fn tag_prefix(&self) -> &'static str {
        "Microsoft:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 10 {
            return Err("Microsoft MakerNote data too short".to_string());
        }

        // Microsoft MakerNotes may start with "Microsoft" signature
        let ifd_offset = if data.len() >= 9 && &data[0..9] == MICROSOFT_SIGNATURE {
            // Skip signature and padding (usually 10 bytes total)
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
        // Accept data with or without Microsoft signature
        if data.len() >= 9 && &data[0..9] == MICROSOFT_SIGNATURE {
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
    fn test_decode_rich_capture() {
        assert_eq!(decode_rich_capture(0), "Off");
        assert_eq!(decode_rich_capture(1), "On");
        assert_eq!(decode_rich_capture(2), "Auto");
    }

    #[test]
    fn test_decode_rich_capture_mode() {
        assert_eq!(decode_rich_capture_mode(0), "None");
        assert_eq!(decode_rich_capture_mode(1), "HDR");
        assert_eq!(decode_rich_capture_mode(2), "HDR + Flash");
    }

    #[test]
    fn test_decode_dynamic_flash() {
        assert_eq!(decode_dynamic_flash(0), "Off");
        assert_eq!(decode_dynamic_flash(1), "Flash + No Flash Blend");
    }

    #[test]
    fn test_decode_pureview_mode() {
        assert_eq!(decode_pureview_mode(0), "Off");
        assert_eq!(decode_pureview_mode(1), "5MP Oversampled");
        assert_eq!(decode_pureview_mode(4), "Lossless Zoom");
    }

    #[test]
    fn test_decode_creative_effect() {
        assert_eq!(decode_creative_effect(0), "None");
        assert_eq!(decode_creative_effect(1), "Black & White");
        assert_eq!(decode_creative_effect(4), "Vivid");
    }

    #[test]
    fn test_decode_lens_type() {
        assert_eq!(decode_lens_type(0), "Built-in");
        assert_eq!(decode_lens_type(1), "Wide Angle Attachment");
    }

    #[test]
    fn test_microsoft_parser_trait() {
        let parser = MicrosoftParser::new();
        assert_eq!(parser.manufacturer_name(), "Microsoft");
        assert_eq!(parser.tag_prefix(), "Microsoft:");
    }

    #[test]
    fn test_validate_header_with_signature() {
        let parser = MicrosoftParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"Microsoft");
        data.extend_from_slice(&[0x00]); // Padding
        data.extend_from_slice(&[0x05, 0x00]); // 5 entries

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_parse_rich_capture_tag() {
        let parser = MicrosoftParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // Rich Capture tag entry (tag=0x0001, type=3 (SHORT), count=1, value=1 (On))
        data.extend_from_slice(&[0x01, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Microsoft:RichCapture"), Some(&"On".to_string()));
    }
}
