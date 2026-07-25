//! Capture One Pro MakerNote parser
//!
//! Parses Capture One Pro-specific editing metadata stored in MakerNotes.
//! Contains styles, adjustments, color grading, lens corrections, and
//! professional workflow information.
//!
//! ## Supported Versions
//! - Capture One Pro 22, 23 (current)
//! - Capture One Pro 20, 21
//! - Capture One Express
//! - Capture One for Sony/Nikon/Fujifilm
//!
//! ## Key Features
//! - Styles applied (built-in and custom)
//! - Base characteristics adjustments
//! - Color grading (shadows, midtones, highlights)
//! - Lens corrections (distortion, chromatic aberration, vignetting)
//! - Local adjustments count
//! - Exposure adjustments
//! - High Dynamic Range tools
//! - Film grain settings
//! - Sharpening and noise reduction
//! - Color balance adjustments
//! - Skin tone adjustments
//! - Clarity and structure
//! - Tethered capture information
//! - Session name and metadata
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

// Import the Capture One tag registry
use super::registries::captureone::CAPTUREONE_TAGS;

// ============================================================================
// Tag ID Constants
// ============================================================================

const C1_VERSION: u16 = 0x0001; // Capture One version
const C1_STYLE_NAME: u16 = 0x0010; // Style name
const C1_ICC_PROFILE: u16 = 0x00C0; // ICC profile name
const C1_PROOF_PROFILE: u16 = 0x00C2; // Proof profile name
const C1_SESSION_NAME: u16 = 0x00D0; // Session name
const C1_OUTPUT_RECIPE_NAME: u16 = 0x00D1; // Output recipe name
const C1_KEYWORDS: u16 = 0x00E2; // Keywords (comma-separated)

// Capture One signature
const CAPTUREONE_SIGNATURE: &[u8] = b"CaptureOne";

// ============================================================================
// Parser Implementation
// ============================================================================

/// Capture One MakerNote parser implementing the MakerNoteParser trait
///
/// This parser extracts Capture One Pro editing metadata from MakerNotes,
/// providing information about styles, adjustments, and professional workflow.
#[derive(Default)]
pub struct CaptureOneParser;

impl CaptureOneParser {
    /// Creates a new Capture One parser instance
    ///
    /// # Returns
    /// A new CaptureOneParser ready to parse MakerNote data
    pub fn new() -> Self {
        CaptureOneParser
    }
}

impl MakerNoteParser for CaptureOneParser {
    fn manufacturer_name(&self) -> &'static str {
        "Capture One"
    }

    fn tag_prefix(&self) -> &'static str {
        "CaptureOne:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Valid if starts with Capture One signature or has minimum length for IFD (8 bytes)
        data.starts_with(CAPTUREONE_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Configure IFD parser with Capture One-specific settings
        let config = IfdParserConfig {
            signature: Some(CAPTUREONE_SIGNATURE),
            signature_offset: 10,
            max_entries: 300,
        };

        // Use shared IFD parser to eliminate boilerplate
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.process_tag(entry.tag_id, entry, parse_data, byte_order, tags);
        })?;

        Ok(())
    }
}

impl CaptureOneParser {
    /// Processes a single tag entry and adds it to the tags map
    ///
    /// This method handles both string-based and numeric tags, using the
    /// CAPTUREONE_TAGS registry for O(1) tag name lookups and automatic decoding.
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
            C1_VERSION => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("CaptureOne:Version".to_string(), s);
                }
                return;
            }
            C1_STYLE_NAME => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("CaptureOne:StyleName".to_string(), s);
                }
                return;
            }
            C1_ICC_PROFILE => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("CaptureOne:ICCProfile".to_string(), s);
                }
                return;
            }
            C1_PROOF_PROFILE => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("CaptureOne:ProofProfile".to_string(), s);
                }
                return;
            }
            C1_SESSION_NAME => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("CaptureOne:SessionName".to_string(), s);
                }
                return;
            }
            C1_OUTPUT_RECIPE_NAME => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("CaptureOne:OutputRecipe".to_string(), s);
                }
                return;
            }
            C1_KEYWORDS => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("CaptureOne:Keywords".to_string(), s);
                }
                return;
            }
            _ => {}
        }

        // Handle numeric tags using registry for O(1) lookup and automatic decoding
        if let Some(tag_name) = CAPTUREONE_TAGS.get_tag_name(tag)
            && let Some(array) = extract_i16_array(entry, data, byte_order)
            && let Some(&val) = array.first()
        {
            let decoded = CAPTUREONE_TAGS.decode_i16(tag, val);
            tags.insert(format!("CaptureOne:{}", tag_name), decoded);
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::registries::captureone::*;
    use super::*;

    #[test]
    fn test_captureone_parser_creation() {
        let parser = CaptureOneParser::new();
        assert_eq!(parser.manufacturer_name(), "Capture One");
        assert_eq!(parser.tag_prefix(), "CaptureOne:");
    }

    #[test]
    fn test_decode_style_type() {
        assert_eq!(STYLE_TYPE.decode(1), "Built-in");
        assert_eq!(STYLE_TYPE.decode(2), "User");
        assert_eq!(STYLE_TYPE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_color_space() {
        assert_eq!(COLOR_SPACE.decode(0), "sRGB");
        assert_eq!(COLOR_SPACE.decode(2), "ProPhoto RGB");
        assert_eq!(COLOR_SPACE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_format_exposure() {
        assert_eq!(format_exposure(15), "+1.5 EV");
        assert_eq!(format_exposure(-10), "-1.0 EV");
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(format_percentage(25), "+25");
        assert_eq!(format_percentage(-50), "-50");
    }

    #[test]
    fn test_format_kelvin() {
        assert_eq!(format_kelvin(550), "5500 K");
        assert_eq!(format_kelvin(650), "6500 K");
    }

    #[test]
    fn test_format_rating() {
        assert_eq!(format_rating(0), "None");
        assert_eq!(format_rating(3), "3 stars");
        assert_eq!(format_rating(5), "5 stars");
    }

    #[test]
    fn test_decode_color_tag() {
        assert_eq!(COLOR_TAG.decode(1), "Red");
        assert_eq!(COLOR_TAG.decode(4), "Green");
        assert_eq!(COLOR_TAG.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_validate_header() {
        let parser = CaptureOneParser::new();
        let valid_header = b"CaptureOne\x00\x01";
        assert!(parser.validate_header(valid_header));

        // Test with minimal valid length
        let minimal_header = b"12345678"; // 8 bytes minimum
        assert!(parser.validate_header(minimal_header));

        // Test with too short data
        let short_header = b"123456";
        assert!(!parser.validate_header(short_header));
    }
}
