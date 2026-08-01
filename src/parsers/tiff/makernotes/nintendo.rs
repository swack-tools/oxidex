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

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::nintendo::nintendo_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_string;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// Nintendo signature
const NINTENDO_SIGNATURE: &[u8] = b"Nintendo";

// ============================================================================
// Decoders
// ============================================================================
//
// ExifTool's Nintendo::Main (Nintendo.pm:19-34) names a single tag, 0x1101
// CameraInfo, which is a SubDirectory. The CAMERA_MODE / CAMERA_SELECTION /
// FILTER tables and the format_parallax / format_3d_effect / format_yes_no
// helpers that used to live here decoded tag IDs 0x0100-0x0107, none of which
// exist in Nintendo.pm - so they are gone.
//
// format_parallax additionally appended " mm", which ExifTool never does:
// Nintendo.pm:71-76 defines 0x28 Parallax as `Format => 'float'` with
// `PrintConv => 'sprintf("%.2f", $val)'`.

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
        // ExifTool's Nintendo::Main names exactly one tag, 0x1101 CameraInfo,
        // and it is a SubDirectory (Nintendo.pm:27-33) rather than a scalar.
        // Until that binary CameraInfo block is implemented the registry is
        // empty, so every entry falls through here without producing a tag.
        let Some(tag_name) = TAG_REGISTRY.get_tag_name(entry.tag_id) else {
            return;
        };
        if let Some(s) = extract_string(entry, data, byte_order) {
            tags.insert(format!("Nintendo:{}", tag_name), s);
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

    /// The removed tests asserted a fabricated table back at itself:
    /// `TAG_REGISTRY.get_tag_name(0x0001) == Some("Model")`,
    /// `CAMERA_MODE.decode(0) == "2D"`, `format_parallax(350) == "3.50 mm"`,
    /// and so on. None of those IDs, names or strings occurs in ExifTool's
    /// `Nintendo.pm`, and its Parallax PrintConv (Nintendo.pm:74) is
    /// `sprintf("%.2f", $val)` with no " mm" suffix.
    ///
    /// A real 3DS maker note carries tag 0x1101; the fabricated table never
    /// matched it, so this parser has always emitted nothing on real files.
    #[test]
    fn test_emits_no_fabricated_tags() {
        let parser = NintendoParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x02, 0x00]); // entry_count = 2
        for tag in [0x0102u16, 0x1101] {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&[0x03, 0x00]); // field_type = SHORT
            data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // value_count
            data.extend_from_slice(&[0x5E, 0x01, 0x00, 0x00]); // value = 350
        }

        let mut tags = HashMap::new();
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .expect("Nintendo maker note should parse");
        assert!(tags.is_empty(), "unexpected Nintendo tags: {tags:?}");
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
