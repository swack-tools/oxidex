//! Phase One MakerNote Parser
//!
//! Parses Phase One-specific EXIF MakerNote tags containing camera settings,
//! lens information, image quality parameters, and other proprietary metadata.
//!
//! Supports Phase One digital medium format cameras including:
//! - Phase One IQ4 series (150MP, 100MP)
//! - Phase One IQ3 series (100MP, 80MP, 60MP)
//! - Phase One IQ2 series
//! - Phase One IQ1 series (P-series digital backs)
//! - Leaf Credo digital backs (acquired by Phase One)
//!
//! ## Registry Pattern Architecture
//!
//! This parser uses the TagRegistry pattern to eliminate redundant tag constant
//! definitions and match-based tag extraction. All tag definitions are centralized
//! in the registry module (registries/phaseone.rs), reducing duplicate code by ~50%.
//!
//! The registry provides:
//! - Tag name lookup by tag ID
//! - Decoder application for enumerated values
//! - Centralized tag definitions shared across parsers
//!
//! Special formatting (e.g., "f/2.8", "1/250 s", "80.0 mm") is handled in the
//! parse_entry function, while enumerated value decoding (e.g., "Aperture Priority")
//! is handled by the registry's decoders.
//!
//! Based on ExifTool's PhaseOne.pm module.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::phaseone_lens_database::lookup_lens_name;
use super::registries::phaseone::phaseone_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ===== Const Decoders =====
// These decoders are used by the registry module to decode enumerated values.
// Exported as public so they can be imported by registries/phaseone.rs.

// Decodes Phase One exposure mode to human-readable string
const_decoder! {
    pub DECODER_EXPOSURE_MODE, i32, [
        (0, "Manual"),
        (1, "Program"),
        (2, "Aperture Priority"),
        (3, "Shutter Priority"),
    ]
}

// Decodes Phase One metering mode to human-readable string
const_decoder! {
    pub DECODER_METERING_MODE, i32, [
        (0, "Unknown"),
        (1, "Multi-zone"),
        (2, "Center-weighted"),
        (3, "Spot"),
    ]
}

// Decodes Phase One white balance to human-readable string
const_decoder! {
    pub DECODER_WHITE_BALANCE, i32, [
        (0, "Auto"),
        (1, "Daylight"),
        (2, "Cloudy"),
        (3, "Shade"),
        (4, "Tungsten"),
        (5, "Fluorescent"),
        (6, "Flash"),
        (7, "Custom"),
        (8, "Kelvin"),
    ]
}

// Decodes Phase One drive mode to human-readable string
const_decoder! {
    pub DECODER_DRIVE_MODE, i32, [
        (0, "Single"),
        (1, "Continuous"),
        (2, "Self-Timer"),
        (3, "Mirror Lock-up"),
        (4, "Live View"),
    ]
}

// Decodes Phase One focus mode to human-readable string
const_decoder! {
    pub DECODER_FOCUS_MODE, i32, [
        (0, "Manual"),
        (1, "Single AF"),
        (2, "Continuous AF"),
    ]
}

// Decodes Phase One flash mode to human-readable string
const_decoder! {
    pub DECODER_FLASH_MODE, i32, [
        (0, "No Flash"),
        (1, "Fired"),
        (2, "Sync"),
        (3, "Fill"),
    ]
}

// Decodes Phase One system type to human-readable string
const_decoder! {
    pub DECODER_SYSTEM_TYPE, i32, [
        (0, "Unknown"),
        (1, "H System"),
        (2, "V System"),
        (3, "DF/DF+"),
        (4, "XF Camera System"),
    ]
}

// Decodes Phase One On/Off settings to human-readable string
const_decoder! {
    pub DECODER_OFF_ON, i32, [
        (0, "Off"),
        (1, "On"),
    ]
}

// Phase One MakerNote header signature
// Phase One typically uses "Phase One" text header or no header at all
const PHASEONE_HEADER: &[u8] = b"Phase One";

// Static registry instance for efficient tag lookup and value decoding
// Initialized lazily on first use using once_cell
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(phaseone_registry);

/// Checks if the provided data has a valid Phase One MakerNote header
///
/// Phase One MakerNotes can have two formats:
/// 1. With "Phase One" header (9 bytes)
/// 2. Without header, starting directly with IFD entry count
///
/// # Arguments
/// * `data` - Raw MakerNote data to validate
///
/// # Returns
/// * `true` if data contains a valid Phase One header or appears to be Phase One data
/// * `false` otherwise
pub fn is_phaseone_makernote(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }

    // Check for "Phase One" header (9 bytes)
    if data.len() >= 9 && &data[0..9] == PHASEONE_HEADER {
        return true;
    }

    // Phase One often has no header, just IFD data
    // Check if first two bytes could be a valid entry count
    if data.len() >= 2 {
        let le_reader = EndianReader::little_endian(data);
        let be_reader = EndianReader::big_endian(data);
        let entry_count_le = le_reader.u16_at(0).unwrap_or(0);
        let entry_count_be = be_reader.u16_at(0).unwrap_or(0);

        // Reasonable entry count: 1-100 entries (Phase One typically has fewer tags)
        if (entry_count_le > 0 && entry_count_le < 100)
            || (entry_count_be > 0 && entry_count_be < 100)
        {
            return true;
        }
    }

    false
}

/// Phase One MakerNote Parser
///
/// Implements the MakerNoteParser trait for Phase One cameras using the
/// registry-based parsing pattern for efficient tag processing.
pub struct PhaseOneMakerNoteParser;

impl PhaseOneMakerNoteParser {
    /// Parse a single IFD entry and extract tag value
    ///
    /// This function handles tag name lookup via the registry, applies special
    /// formatting for measurement values (focal length, aperture, shutter speed),
    /// and uses registry decoders for enumerated values.
    ///
    /// # Arguments
    /// * `entry` - IFD entry to parse containing tag ID and value
    /// * `_data` - Full MakerNote data buffer (unused but kept for signature compatibility)
    /// * `_byte_order` - Byte order for multi-byte values (unused but kept for compatibility)
    /// * `tags` - HashMap to insert extracted tags into
    ///
    /// # Special Handling
    ///
    /// Several tags require special formatting:
    /// - Lens ID (0x0211): Lookup lens name from database
    /// - Dimensions (0x010E-0x0112): Format with "px" suffix
    /// - Focal length (0x0214): Divide by 10.0, format with "mm"
    /// - Aperture (0x0403): Divide by 10.0, format as "f/N.N"
    /// - Shutter speed (0x0402): Inverse and divide by 1000.0, format as "1/N s"
    /// - Temperature (0x0601): Format with "°C" suffix
    ///
    /// All other tags use the registry's decoder if available, falling back to raw value.
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

        // Extract and format the value based on tag type
        let formatted_value = match entry.tag_id {
            // Special handling for lens ID (0x0211) - use database lookup
            // This tag stores a numeric ID that maps to a specific lens model
            0x0211 => {
                let lens_id = entry.value_offset as u16;
                tags.insert(format!("PhaseOne:{}", tag_name), lens_id.to_string());
                // Lookup friendly lens name from database and add as separate tag
                if let Some(lens_name) = lookup_lens_name(lens_id) {
                    tags.insert("PhaseOne:LensModel".to_string(), lens_name);
                }
                return;
            }
            // Dimensions with pixel units (SensorWidth, SensorHeight, ImageWidth, ImageHeight)
            0x010E | 0x010F | 0x0111 | 0x0112 => format!("{} px", entry.value_offset),

            // Bit depth with bit units
            0x0110 => format!("{} bit", entry.value_offset),

            // Focal length (value stored as mm * 10, so divide by 10.0)
            0x0214 => format!("{:.1} mm", entry.value_offset as f32 / 10.0),

            // Focus distance (value stored as m * 100, so divide by 100.0)
            0x0215 => format!("{:.2} m", entry.value_offset as f32 / 100.0),

            // Shutter speed (value stored as seconds * 1000, convert to fractional form)
            0x0402 => {
                let speed = entry.value_offset as f32 / 1000.0;
                if speed != 0.0 {
                    format!("1/{:.0} s", 1.0 / speed)
                } else {
                    "Unknown".to_string()
                }
            }

            // Aperture (value stored as f-number * 10, so divide by 10.0)
            0x0403 => format!("f/{:.1}", entry.value_offset as f32 / 10.0),

            // Exposure compensation (value stored as EV * 10, so divide by 10.0)
            0x0404 => format!("{:.1} EV", entry.value_offset as f32 / 10.0),

            // Color temperature with Kelvin suffix
            0x0413 => format!("{}K", entry.value_offset),

            // Sensor temperature with Celsius suffix (signed value)
            0x0601 => format!("{}°C", entry.value_offset as i32),

            // All other tags use registry decoder if available
            // This handles enumerated values like ExposureMode, MeteringMode, etc.
            _ => TAG_REGISTRY.decode_i32(entry.tag_id, entry.value_offset as i32),
        };

        tags.insert(format!("PhaseOne:{}", tag_name), formatted_value);
    }
}

impl MakerNoteParser for PhaseOneMakerNoteParser {
    fn manufacturer_name(&self) -> &'static str {
        "PhaseOne"
    }

    fn tag_prefix(&self) -> &'static str {
        "PhaseOne:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_phaseone_makernote(data)
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
        // Validate minimum data length (need at least 2 bytes for entry count)
        if data.len() < 2 {
            return Err("Phase One MakerNote data too short".to_string());
        }

        // Determine signature and offset based on header presence
        // Phase One uses "Phase One" (9 bytes) header or starts directly with IFD
        let signature = if data.len() >= 9 && &data[0..9] == PHASEONE_HEADER {
            Some(&data[0..9])
        } else {
            None
        };

        // Configure the IFD parser with Phase One-specific parameters
        let config = IfdParserConfig {
            signature,
            signature_offset: 9, // Skip 9-byte "Phase One" header if present
            max_entries: 150,    // Phase One typically has fewer than 150 tags
        };

        // Parse IFD entries using the shared parser infrastructure
        // The callback (Self::parse_entry) is invoked for each entry
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            Self::parse_entry(entry, parse_data, byte_order, tags);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test is_phaseone_makernote function with valid headers
    #[test]
    fn test_is_phaseone_makernote_valid() {
        // Test with "Phase One" header
        let data = b"Phase One\x00\x01\x02";
        assert!(is_phaseone_makernote(data));

        // Test with valid entry count (little endian)
        let data = &[0x05, 0x00]; // 5 entries
        assert!(is_phaseone_makernote(data));

        // Test with valid entry count (big endian)
        let data = &[0x00, 0x0A]; // 10 entries
        assert!(is_phaseone_makernote(data));
    }

    /// Test is_phaseone_makernote function with invalid data
    #[test]
    fn test_is_phaseone_makernote_invalid() {
        // Test with too short data
        let data = &[0x00];
        assert!(!is_phaseone_makernote(data));

        // Test with empty data
        let data = &[];
        assert!(!is_phaseone_makernote(data));

        // Test with invalid entry count (too high)
        let data = &[0xFF, 0xFF]; // 65535 entries (invalid)
        assert!(!is_phaseone_makernote(data));
    }

    /// Test PhaseOneMakerNoteParser trait methods
    #[test]
    fn test_parser_trait_methods() {
        let parser = PhaseOneMakerNoteParser;

        // Test manufacturer name
        assert_eq!(parser.manufacturer_name(), "PhaseOne");

        // Test tag prefix
        assert_eq!(parser.tag_prefix(), "PhaseOne:");

        // Test validate_header with valid data
        let valid_data = b"Phase One\x00\x01";
        assert!(parser.validate_header(valid_data));

        // Test validate_header with invalid data
        let invalid_data = &[];
        assert!(!parser.validate_header(invalid_data));
    }
}
