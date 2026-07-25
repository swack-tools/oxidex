//! Nikon Capture NX-D/ViewNX-i MakerNote parser
//!
//! Parses Nikon Capture NX-D and ViewNX-i editing metadata.
//! Contains Picture Control settings, Active D-Lighting adjustments,
//! Vignette Control, color adjustments, and Nikon-specific processing.
//!
//! ## Supported Applications
//! - Nikon Capture NX-D (current)
//! - ViewNX-i
//! - Nikon Capture NX2 (legacy)
//! - Nikon ViewNX 2 (legacy)
//!
//! ## Key Features
//! - Picture Control settings (Standard, Neutral, Vivid, etc.)
//! - Active D-Lighting amount
//! - Vignette Control
//! - Color Booster
//! - Color Control Points
//! - Filter effects
//! - Noise Reduction
//! - Unsharp Mask settings
//! - Straighten adjustments
//! - Retouch tools used
//! - RAW processing settings
//! - White balance fine-tuning
//! - Exposure compensation
//!
//! ## Architecture
//! This parser uses the TagRegistry pattern to eliminate code duplication.
//! All tag definitions are centralized in the registry module for O(1) lookup
//! and automatic value decoding.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_string};
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

// Import the Nikon Capture tag registry
use super::registries::nikoncapture::NIKONCAPTURE_TAGS;

// ============================================================================
// Tag ID Constants
// ============================================================================

const NC_VERSION: u16 = 0x0001; // Nikon Capture version
const NC_PICTURE_CONTROL_NAME: u16 = 0x0010; // Picture Control name
const NC_PICTURE_CONTROL_BASE: u16 = 0x0011; // Picture Control base

// Nikon Capture signature
const NIKON_CAPTURE_SIGNATURE: &[u8] = b"NikonNX";

// ============================================================================
// Parser Implementation
// ============================================================================

/// Nikon Capture MakerNote parser implementing the MakerNoteParser trait
///
/// This parser extracts Nikon Capture NX-D and ViewNX-i editing metadata from MakerNotes,
/// providing information about Picture Controls, adjustments, and Nikon-specific processing.
#[derive(Default)]
pub struct NikonCaptureParser;

impl NikonCaptureParser {
    /// Creates a new Nikon Capture parser instance
    ///
    /// # Returns
    /// A new NikonCaptureParser ready to parse MakerNote data
    pub fn new() -> Self {
        NikonCaptureParser
    }
}

impl MakerNoteParser for NikonCaptureParser {
    fn manufacturer_name(&self) -> &'static str {
        "Nikon Capture"
    }

    fn tag_prefix(&self) -> &'static str {
        "NikonCapture:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Valid if starts with Nikon Capture signature or has minimum length for IFD (8 bytes)
        data.starts_with(NIKON_CAPTURE_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Configure IFD parser with Nikon Capture-specific settings
        let config = IfdParserConfig {
            signature: Some(NIKON_CAPTURE_SIGNATURE),
            signature_offset: 7,
            max_entries: 200,
        };

        // Use shared IFD parser to eliminate boilerplate
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.process_tag(entry.tag_id, entry, parse_data, byte_order, tags);
        })?;

        Ok(())
    }
}

impl NikonCaptureParser {
    /// Processes a single tag entry and adds it to the tags map
    ///
    /// This method handles both string-based and numeric tags, using the
    /// NIKONCAPTURE_TAGS registry for O(1) tag name lookups and automatic decoding.
    ///
    /// # Arguments
    /// * `tag` - Tag ID to process
    /// * `entry` - IFD entry containing tag data
    /// * `data` - Raw MakerNote data buffer
    /// * `byte_order` - Byte order for parsing
    /// * `tags` - Output map to store decoded tag values
    fn process_tag(
        &self,
        tag: u16,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        // Handle string-based tags (not in registry)
        match tag {
            NC_VERSION => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("NikonCapture:Version".to_string(), s);
                }
                return;
            }
            NC_PICTURE_CONTROL_NAME => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("NikonCapture:PictureControlName".to_string(), s);
                }
                return;
            }
            NC_PICTURE_CONTROL_BASE => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("NikonCapture:PictureControlBase".to_string(), s);
                }
                return;
            }
            _ => {}
        }

        // Handle numeric tags using registry for O(1) lookup and automatic decoding
        if let Some(tag_name) = NIKONCAPTURE_TAGS.get_tag_name(tag)
            && let Some(array) = extract_i16_array(entry, data, byte_order)
            && let Some(&val) = array.first()
        {
            let decoded = NIKONCAPTURE_TAGS.decode_i16(tag, val);
            tags.insert(format!("NikonCapture:{}", tag_name), decoded);
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::registries::nikoncapture::*;
    use super::*;

    #[test]
    fn test_nikon_capture_parser_creation() {
        let parser = NikonCaptureParser::new();
        assert_eq!(parser.manufacturer_name(), "Nikon Capture");
        assert_eq!(parser.tag_prefix(), "NikonCapture:");
    }

    #[test]
    fn test_decode_picture_control() {
        assert_eq!(PICTURE_CONTROL.decode(1), "Standard");
        assert_eq!(PICTURE_CONTROL.decode(3), "Vivid");
        assert_eq!(PICTURE_CONTROL.decode(4), "Monochrome");
        assert_eq!(PICTURE_CONTROL.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_active_d_lighting() {
        assert_eq!(ACTIVE_D_LIGHTING.decode(0), "Off");
        assert_eq!(ACTIVE_D_LIGHTING.decode(3), "High");
        assert_eq!(ACTIVE_D_LIGHTING.decode(5), "Auto");
        assert_eq!(ACTIVE_D_LIGHTING.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_vignette_control() {
        assert_eq!(VIGNETTE_CONTROL.decode(0), "Off");
        assert_eq!(VIGNETTE_CONTROL.decode(2), "Normal");
        assert_eq!(VIGNETTE_CONTROL.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_format_adjustment() {
        assert_eq!(format_adjustment(10), "+10");
        assert_eq!(format_adjustment(-5), "-5");
    }

    #[test]
    fn test_format_exposure_comp() {
        assert_eq!(format_exposure_comp(3), "+1.0 EV");
        assert_eq!(format_exposure_comp(-6), "-2.0 EV");
    }

    #[test]
    fn test_format_straighten() {
        assert_eq!(format_straighten(15), "+1.5°");
        assert_eq!(format_straighten(-25), "-2.5°");
        assert_eq!(format_straighten(0), "0°");
    }

    #[test]
    fn test_decode_filter_effect() {
        assert_eq!(FILTER_EFFECT.decode(1), "Yellow");
        assert_eq!(FILTER_EFFECT.decode(3), "Red");
        assert_eq!(FILTER_EFFECT.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_validate_header() {
        let parser = NikonCaptureParser::new();
        let valid_header = b"NikonNX\x00\x01";
        assert!(parser.validate_header(valid_header));

        // Test with minimal valid length
        let minimal_header = b"12345678"; // 8 bytes minimum
        assert!(parser.validate_header(minimal_header));

        // Test with too short data
        let short_header = b"123456";
        assert!(!parser.validate_header(short_header));
    }
}
