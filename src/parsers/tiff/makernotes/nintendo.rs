//! Nintendo 3DS Camera MakerNote parser
//!
//! Parses Nintendo-specific EXIF MakerNote tags from 3DS handheld camera.
//! The Nintendo 3DS features dual cameras for stereoscopic 3D photography.
//!
//! ## Supported Models
//! - Nintendo 3DS
//! - Nintendo 3DS XL
//! - New Nintendo 3DS
//! - New Nintendo 3DS XL
//! - Nintendo 2DS (single camera, no 3D)
//!
//! ## Key Features
//! - Stereoscopic 3D mode
//! - Parallax adjustment
//! - Camera selection (inner/outer)
//! - 3D effect depth
//! - Game integration metadata
//! - Mii face detection
//!
//! ## Architecture
//! Stores metadata specific to handheld gaming device photography,
//! including 3D stereoscopic capture settings.

#![allow(dead_code)]

use crate::const_decoder;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::nintendo::nintendo_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_string};
use super::shared::generic_decoders::ON_OFF;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// Nintendo signature
const NINTENDO_SIGNATURE: &[u8] = b"Nintendo";

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================
// These replace 7 repetitive decoder functions with compile-time const decoders,
// reducing code duplication while maintaining all functionality.

// Camera Mode decoder - 2D vs 3D photography mode
const_decoder!(pub CAMERA_MODE, i16, [(0, "2D"), (1, "3D"),]);

// Camera Selection decoder - Inner camera (facing user) vs outer stereoscopic cameras
const_decoder!(pub
    CAMERA_SELECTION,
    i16,
    [
        (0, "Inner Camera"),
        (1, "Outer Camera Left"),
        (2, "Outer Camera Right"),
    ]
);

// Photo Filter decoder - Built-in photo effects
const_decoder!(pub
    FILTER,
    i16,
    [
        (0, "None"),
        (1, "Sepia"),
        (2, "Black & White"),
        (3, "Negative"),
        (4, "Toy Camera"),
        (5, "Fisheye"),
    ]
);

// ============================================================================
// Custom Formatters
// ============================================================================
// Formatters for values that need unit conversion or special formatting logic.

// Formats parallax value (stored as hundredths of millimeters)
// # Arguments
// * `value` - Parallax value in 1/100mm units
// # Returns
// Formatted string with mm units (e.g., "3.50 mm")
fn format_parallax(value: i16) -> String {
    format!("{:.2} mm", value as f64 / 100.0)
}

// Formats 3D effect depth percentage with validation
// # Arguments
// * `value` - 3D effect depth (0-100)
// # Returns
// Formatted percentage or "Invalid" if out of range
fn format_3d_effect(value: i16) -> String {
    if !(0..=100).contains(&value) {
        return "Invalid".to_string();
    }
    format!("{}%", value)
}

// Formats boolean values as Yes/No strings
// # Arguments
// * `value` - Boolean value (0=No, non-zero=Yes)
// # Returns
// "Yes" or "No" string
fn format_yes_no(value: i16) -> String {
    if value != 0 {
        "Yes".to_string()
    } else {
        "No".to_string()
    }
}

// ============================================================================
// Tag Registry
// ============================================================================
// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(nintendo_registry);

// ============================================================================
// Parser Implementation
// ============================================================================

/// Parser for Nintendo MakerNotes
#[derive(Default)]
pub struct NintendoParser;

impl NintendoParser {
    /// Creates a new Nintendo parser instance
    pub fn new() -> Self {
        NintendoParser
    }

    /// Parses a single IFD entry and extracts the tag value
    ///
    /// # Arguments
    /// * `entry` - The IFD entry containing tag metadata
    /// * `data` - The full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - Output HashMap to store parsed tags
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_id = entry.tag_id;

        // Handle string tags (Model, SystemVersion, GameTitle)
        match tag_id {
            0x0001 | 0x0002 | 0x0107 => {
                if let Some(s) = extract_string(entry, data, byte_order)
                    && let Some(name) = TAG_REGISTRY.get_tag_name(tag_id)
                {
                    tags.insert(format!("Nintendo:{}", name), s);
                }
                return;
            }
            _ => {}
        }

        // Handle i16 array tags
        if let Some(array) = extract_i16_array(entry, data, byte_order)
            && let Some(&value) = array.first()
        {
            let tag_name = match TAG_REGISTRY.get_tag_name(tag_id) {
                Some(name) => name,
                None => return,
            };

            // Try registry decoders first for decoded tags
            let formatted_value = TAG_REGISTRY.decode_i16(tag_id, value);

            // Fallback to custom formatters for tags without registry decoders
            let formatted_value = if formatted_value == value.to_string() {
                match tag_id {
                    0x0102 => format_parallax(value),
                    0x0103 => format_3d_effect(value),
                    0x0104 => ON_OFF.decode(value),
                    0x0105 => format_yes_no(value),
                    _ => return,
                }
            } else {
                formatted_value
            };

            tags.insert(format!("Nintendo:{}", tag_name), formatted_value);
        }
    }
}

impl MakerNoteParser for NintendoParser {
    /// Returns the manufacturer name for this parser
    fn manufacturer_name(&self) -> &'static str {
        "Nintendo"
    }

    /// Returns the tag prefix used for all Nintendo tags
    fn tag_prefix(&self) -> &'static str {
        "Nintendo:"
    }

    /// Validates the MakerNote header for Nintendo format
    ///
    /// # Arguments
    /// * `data` - MakerNote data to validate
    ///
    /// # Returns
    /// true if the data appears to be a valid Nintendo MakerNote
    fn validate_header(&self, data: &[u8]) -> bool {
        data.len() >= 8 && (data.starts_with(NINTENDO_SIGNATURE) || data.len() >= 8)
    }

    /// Parses Nintendo MakerNote data and extracts all tags
    ///
    /// Uses the shared IFD parser to handle the common IFD structure,
    /// then delegates to parse_entry for tag-specific extraction.
    ///
    /// # Arguments
    /// * `data` - Full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte value parsing
    /// * `tags` - Output HashMap to populate with parsed tags
    ///
    /// # Returns
    /// * `Ok(())` - Successfully parsed MakerNote
    /// * `Err(String)` - Parse error with description
    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Configure IFD parser with Nintendo-specific settings
        let config = IfdParserConfig {
            signature: Some(NINTENDO_SIGNATURE),
            signature_offset: 8, // Skip "Nintendo" signature
            max_entries: 200,    // Reasonable upper bound for tag count
        };

        // Use shared IFD parser to iterate through entries
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })?;
        Ok(())
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nintendo_parser_creation() {
        let parser = NintendoParser::new();
        assert_eq!(parser.manufacturer_name(), "Nintendo");
        assert_eq!(parser.tag_prefix(), "Nintendo:");
    }

    #[test]
    fn test_camera_mode_decoder() {
        assert_eq!(CAMERA_MODE.decode(0), "2D");
        assert_eq!(CAMERA_MODE.decode(1), "3D");
        assert_eq!(CAMERA_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_camera_selection_decoder() {
        assert_eq!(CAMERA_SELECTION.decode(0), "Inner Camera");
        assert_eq!(CAMERA_SELECTION.decode(1), "Outer Camera Left");
        assert_eq!(CAMERA_SELECTION.decode(2), "Outer Camera Right");
    }

    #[test]
    fn test_filter_decoder() {
        assert_eq!(FILTER.decode(0), "None");
        assert_eq!(FILTER.decode(4), "Toy Camera");
        assert_eq!(FILTER.decode(5), "Fisheye");
    }

    #[test]
    fn test_format_parallax() {
        assert_eq!(format_parallax(350), "3.50 mm");
        assert_eq!(format_parallax(0), "0.00 mm");
        assert_eq!(format_parallax(-100), "-1.00 mm");
    }

    #[test]
    fn test_format_3d_effect() {
        assert_eq!(format_3d_effect(0), "0%");
        assert_eq!(format_3d_effect(50), "50%");
        assert_eq!(format_3d_effect(100), "100%");
        assert_eq!(format_3d_effect(-1), "Invalid");
        assert_eq!(format_3d_effect(101), "Invalid");
    }

    #[test]
    fn test_format_yes_no() {
        assert_eq!(format_yes_no(0), "No");
        assert_eq!(format_yes_no(1), "Yes");
        assert_eq!(format_yes_no(42), "Yes");
    }

    #[test]
    fn test_tag_registry() {
        assert_eq!(TAG_REGISTRY.get_tag_name(0x0001), Some("Model"));
        assert_eq!(TAG_REGISTRY.get_tag_name(0x0100), Some("CameraMode"));
        assert!(TAG_REGISTRY.has_tag(0x0102));
    }

    #[test]
    fn test_validate_header() {
        let parser = NintendoParser::new();

        // Valid header with signature
        let valid_data = b"NintendoXXXXXXXX";
        assert!(parser.validate_header(valid_data));

        // Valid data without signature but sufficient length
        let valid_no_sig = vec![0u8; 10];
        assert!(parser.validate_header(&valid_no_sig));

        // Invalid: too short
        let invalid_short = b"Ninten";
        assert!(!parser.validate_header(invalid_short));
    }
}
