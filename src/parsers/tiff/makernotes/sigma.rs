//! Sigma MakerNote Parser
//!
//! Parses Sigma-specific EXIF MakerNote tags containing camera settings,
//! lens information, image quality parameters, and other proprietary metadata.
//!
//! Supports Sigma cameras including:
//! - Sigma SD series (SD1, SD1 Merrill, SD15, SD14, SD10, SD9)
//! - Sigma DP series compacts (DP1, DP2, DP3, Quattro series)
//! - Sigma fp/fp L mirrorless cameras
//!
//! Based on ExifTool's Sigma.pm module.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::sigma::sigma_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;
use super::sigma_lens_database::lookup_lens_name;

// ===== Sigma MakerNote Tag IDs =====
// Tag definitions are now centralized in the registry.
// See registries/sigma.rs for the complete tag registry.

// Sigma MakerNote header signatures
// Sigma typically uses "SIGMA\0\0\0" or "FOVEON" headers
const SIGMA_HEADER: &[u8] = b"SIGMA\0\0\0";
const SIGMA_HEADER_FOVEON: &[u8] = b"FOVEON\0\0";

// Static registry instance for efficient tag lookup and decoding
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(sigma_registry);

/// Checks if the provided data has a valid Sigma MakerNote header
///
/// # Arguments
/// * `data` - Raw MakerNote data to validate
///
/// # Returns
/// * `true` if data contains a valid Sigma header
/// * `false` otherwise
pub fn is_sigma_makernote(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }

    // Check for SIGMA header (8 bytes)
    if data.len() >= 8 && &data[0..8] == SIGMA_HEADER {
        return true;
    }

    // Check for FOVEON header (8 bytes)
    if data.len() >= 8 && &data[0..8] == SIGMA_HEADER_FOVEON {
        return true;
    }

    // Some Sigma cameras may have no header, check for valid IFD entry count
    if data.len() >= 2 {
        let reader = EndianReader::little_endian(data);
        let entry_count = reader.u16_at(0).unwrap_or(0);
        // Reasonable entry count: 1-150 entries
        if entry_count > 0 && entry_count < 150 {
            return true;
        }
    }

    false
}

// ============================================================================
// DECODERS - Sigma Value Decoders
// ============================================================================
// Sigma-specific value decoders for camera settings

use crate::const_decoder;

// Decoder for Sigma resolution modes
const_decoder!(pub DECODE_RESOLUTION_MODE, i32, [
    (0, "Low"),
    (1, "Medium"),
    (2, "High"),
    (3, "Ultra High"),
]);

// Decoder for Sigma autofocus modes
const_decoder!(pub DECODE_AF_MODE, i32, [
    (0, "Manual"),
    (1, "AF-S (Single)"),
    (2, "AF-C (Continuous)"),
    (3, "AF-A (Auto)"),
]);

// Decoder for Sigma white balance settings
const_decoder!(pub DECODE_WHITE_BALANCE, i32, [
    (0, "Auto"),
    (1, "Daylight"),
    (2, "Shade"),
    (3, "Cloudy"),
    (4, "Tungsten"),
    (5, "Fluorescent"),
    (6, "Flash"),
    (7, "Custom"),
    (8, "Color Temperature"),
]);

// Decoder for Sigma exposure modes
const_decoder!(pub DECODE_EXPOSURE_MODE, i32, [
    (0, "Auto"),
    (1, "Program"),
    (2, "Aperture Priority"),
    (3, "Shutter Priority"),
    (4, "Manual"),
]);

// Decoder for Sigma metering modes
const_decoder!(pub DECODE_METERING_MODE, i32, [
    (0, "Unknown"),
    (1, "Multi-segment"),
    (2, "Center-weighted Average"),
    (3, "Spot"),
    (4, "Average"),
]);

// Decoder for Sigma drive modes
const_decoder!(pub DECODE_DRIVE_MODE, i32, [
    (0, "Single"),
    (1, "Continuous"),
    (2, "Self-Timer"),
    (3, "Self-Timer (Multiple)"),
    (4, "Bracket"),
    (5, "Mirror Lock-up"),
]);

// Decoder for Sigma flash modes
const_decoder!(pub DECODE_FLASH_MODE, i32, [
    (0, "Off"),
    (1, "Auto"),
    (2, "On"),
    (3, "Red-eye Reduction"),
    (4, "Fill Flash"),
    (5, "Slow Sync"),
    (6, "Rear Curtain"),
    (7, "Wireless"),
]);

// Decoder for Sigma image quality settings
const_decoder!(pub DECODE_QUALITY, i32, [
    (0, "Low"),
    (1, "Medium"),
    (2, "High"),
    (3, "RAW"),
    (4, "RAW + JPEG"),
]);

// Decoder for Sigma color modes
const_decoder!(pub DECODE_COLOR_MODE, i32, [
    (0, "Standard"),
    (1, "Vivid"),
    (2, "Neutral"),
    (3, "Portrait"),
    (4, "Landscape"),
    (5, "Monochrome"),
    (6, "Sepia"),
    (7, "FOV Classic Blue"),
    (8, "FOV Classic Yellow"),
]);

// Decoder for Sigma color space settings
const_decoder!(pub DECODE_COLOR_SPACE, i32, [
    (0, "sRGB"),
    (1, "Adobe RGB"),
]);

// Decoder for Sigma picture styles
const_decoder!(pub DECODE_PICTURE_STYLE, i32, [
    (0, "Standard"),
    (1, "Vivid"),
    (2, "Neutral"),
    (3, "Portrait"),
    (4, "Landscape"),
    (5, "Monochrome"),
]);

/// Sigma MakerNote Parser
///
/// Implements the MakerNoteParser trait for Sigma cameras.
pub struct SigmaMakerNoteParser;

impl SigmaMakerNoteParser {
    /// Parse a single IFD entry and extract tag value
    ///
    /// # Arguments
    /// * `entry` - IFD entry to parse
    /// * `data` - Full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    fn parse_entry(
        entry: &IfdEntry,
        _data: &[u8],
        _byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        // Get tag name from registry
        let tag_name = match TAG_REGISTRY.get_tag_name(entry.tag_id) {
            Some(name) => name,
            None => return, // Unknown tag, skip it
        };

        // Extract and format the value
        let formatted_value = match entry.tag_id {
            // Special handling for lens ID (0x001B) - use database lookup
            0x001B => {
                let lens_id = entry.value_offset as u16;
                tags.insert(format!("Sigma:{}", tag_name), lens_id.to_string());
                if let Some(lens_name) = lookup_lens_name(lens_id) {
                    tags.insert("Sigma:LensModel".to_string(), lens_name);
                }
                return;
            }
            // Special handling for camera temperature (0x001D) - format with degree symbol
            0x001D => {
                let value = entry.value_offset as i32;
                format!("{}°C", value)
            }
            // Special handling for exposure compensation and flash exposure comp - format as EV
            0x000C | 0x0032 => {
                let value = entry.value_offset as i32;
                format!("{:.1} EV", value as f32 / 10.0)
            }
            // All other tags use registry decoder if available
            _ => TAG_REGISTRY.decode_i32(entry.tag_id, entry.value_offset as i32),
        };

        tags.insert(format!("Sigma:{}", tag_name), formatted_value);
    }
}

impl MakerNoteParser for SigmaMakerNoteParser {
    fn manufacturer_name(&self) -> &'static str {
        "Sigma"
    }

    fn tag_prefix(&self) -> &'static str {
        "Sigma:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_sigma_makernote(data)
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // Validate minimum data length
        if data.len() < 8 {
            return Err("Sigma MakerNote data too short".to_string());
        }

        // Determine IFD offset based on header presence
        // Sigma uses "SIGMA\0\0\0" (8 bytes) or "FOVEON\0\0" (8 bytes) headers
        let signature = if data.len() >= 8
            && (&data[0..8] == SIGMA_HEADER || &data[0..8] == SIGMA_HEADER_FOVEON)
        {
            Some(&data[0..8])
        } else {
            None
        };

        let config = IfdParserConfig {
            signature,
            signature_offset: 8, // Skip 8-byte signature (SIGMA\0\0\0 or FOVEON\0\0)
            max_entries: 200,
        };

        // Parse IFD entries using the shared parser
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            Self::parse_entry(entry, parse_data, byte_order, tags);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_sigma_makernote() {
        // Test SIGMA header
        let sigma_header = b"SIGMA\0\0\0";
        assert!(is_sigma_makernote(sigma_header));

        // Test FOVEON header
        let foveon_header = b"FOVEON\0\0";
        assert!(is_sigma_makernote(foveon_header));

        // Test invalid header
        let invalid_header = b"INVALID\0";
        assert!(!is_sigma_makernote(invalid_header));

        // Test too short data
        let short_data = b"SIG";
        assert!(!is_sigma_makernote(short_data));

        // Test reasonable entry count (no header) - need at least 8 bytes
        let entry_count_data = [10u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8]; // 10 entries in little endian
        assert!(is_sigma_makernote(&entry_count_data));

        // Test unreasonable entry count - need at least 8 bytes
        let bad_entry_count = [200u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8]; // 200 entries
        assert!(!is_sigma_makernote(&bad_entry_count));
    }

    #[test]
    fn test_sigma_parser_trait() {
        let parser = SigmaMakerNoteParser;
        assert_eq!(parser.manufacturer_name(), "Sigma");
        assert_eq!(parser.tag_prefix(), "Sigma:");
    }
}
