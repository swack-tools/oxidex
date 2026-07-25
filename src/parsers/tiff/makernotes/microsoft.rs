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
//!
//! ## Code Organization
//! This parser uses the TagRegistry pattern to eliminate repetitive match arms.
//! All tag definitions and decoders are centralized in the registries::microsoft module,
//! reducing code duplication and improving maintainability.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::registries::microsoft::{MICROSOFT_LIVING_IMAGE, microsoft_registry};
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_value, extract_string, extract_u32_value};
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

// Microsoft signature for validation
const MICROSOFT_SIGNATURE: &[u8] = b"Microsoft";

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

    /// Parse a single IFD entry and extract tag value using the registry
    ///
    /// This method uses the TagRegistry pattern to eliminate repetitive match arms.
    /// All tag definitions and decoders are accessed through the centralized registry,
    /// reducing code duplication and improving maintainability.
    ///
    /// # Arguments
    /// * `entry` - IFD entry to parse
    /// * `data` - Full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    ///
    /// # Implementation Strategy
    /// - Special handling for Living Image (string type)
    /// - Special handling for resolution tags (u32 type)
    /// - All other tags use i16 values with registry-based decoding
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_id = entry.tag_id;
        let registry = microsoft_registry();

        // Special case: Living Image uses string value
        if tag_id == MICROSOFT_LIVING_IMAGE {
            if let Some(id) = extract_string(entry, data, byte_order) {
                tags.insert("Microsoft:LivingImageID".to_string(), id);
                tags.insert("Microsoft:LivingImage".to_string(), "Yes".to_string());
            }
            return;
        }

        // Check if this tag is registered
        if !registry.has_tag(tag_id) {
            // Unknown tag - skip silently for forward compatibility
            return;
        }

        // Get the tag name from registry
        let tag_name = registry.get_tag_name(tag_id).unwrap();
        let full_tag_name = format!("Microsoft:{}", tag_name);

        // Try u32 extraction first (for resolution tags)
        if let Some(value) = extract_u32_value(entry, data, byte_order) {
            let decoded = registry.decode_u32(tag_id, value);
            tags.insert(full_tag_name, decoded);
            return;
        }

        // Otherwise, extract as i16 (most common type)
        if let Some(value) = extract_i16_value(entry, data, byte_order) {
            let decoded = registry.decode_i16(tag_id, value);
            tags.insert(full_tag_name, decoded);
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

        // Configure IFD parser for Microsoft format
        // Microsoft MakerNotes may start with "Microsoft" signature (9 bytes)
        // followed by 1 byte of padding before the IFD data
        let config = IfdParserConfig {
            signature: Some(MICROSOFT_SIGNATURE),
            signature_offset: 10,
            max_entries: 500,
        };

        // Use shared IFD parser to eliminate boilerplate
        // The callback receives each parsed IFD entry and the data buffer
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })?;

        Ok(())
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Accept data with or without Microsoft signature
        if data.len() >= 9 && &data[0..9] == MICROSOFT_SIGNATURE {
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
    use super::super::registries::microsoft::{
        CREATIVE_EFFECT, DYNAMIC_FLASH, LENS_TYPE, PUREVIEW_MODE, RICH_CAPTURE, RICH_CAPTURE_MODE,
    };
    use super::*;

    #[test]
    fn test_decode_rich_capture() {
        assert_eq!(RICH_CAPTURE.decode(0), "Off");
        assert_eq!(RICH_CAPTURE.decode(1), "On");
        assert_eq!(RICH_CAPTURE.decode(2), "Auto");
        assert_eq!(RICH_CAPTURE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_rich_capture_mode() {
        assert_eq!(RICH_CAPTURE_MODE.decode(0), "None");
        assert_eq!(RICH_CAPTURE_MODE.decode(1), "HDR");
        assert_eq!(RICH_CAPTURE_MODE.decode(2), "HDR + Flash");
        assert_eq!(RICH_CAPTURE_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_dynamic_flash() {
        assert_eq!(DYNAMIC_FLASH.decode(0), "Off");
        assert_eq!(DYNAMIC_FLASH.decode(1), "Flash + No Flash Blend");
        assert_eq!(DYNAMIC_FLASH.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_pureview_mode() {
        assert_eq!(PUREVIEW_MODE.decode(0), "Off");
        assert_eq!(PUREVIEW_MODE.decode(1), "5MP Oversampled");
        assert_eq!(PUREVIEW_MODE.decode(4), "Lossless Zoom");
        assert_eq!(PUREVIEW_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_creative_effect() {
        assert_eq!(CREATIVE_EFFECT.decode(0), "None");
        assert_eq!(CREATIVE_EFFECT.decode(1), "Black & White");
        assert_eq!(CREATIVE_EFFECT.decode(4), "Vivid");
        assert_eq!(CREATIVE_EFFECT.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_lens_type() {
        assert_eq!(LENS_TYPE.decode(0), "Built-in");
        assert_eq!(LENS_TYPE.decode(1), "Wide Angle Attachment");
        assert_eq!(LENS_TYPE.decode(99), "Unknown (99)");
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

    #[test]
    fn test_registry_based_parsing() {
        // Verify that the registry pattern works for all tag types
        let parser = MicrosoftParser::new();
        let mut data = Vec::new();

        // Create IFD with multiple entries
        data.extend_from_slice(&[0x03, 0x00]); // 3 entries

        // 1. Rich Capture (custom decoder)
        data.extend_from_slice(&[0x01, 0x00]); // Tag 0x0001
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Auto)

        // 2. PureView Mode (custom decoder)
        data.extend_from_slice(&[0x0B, 0x00]); // Tag 0x000B
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (5MP Oversampled)

        // 3. Video 4K (binary on/off)
        data.extend_from_slice(&[0x10, 0x00]); // Tag 0x0010
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Microsoft:RichCapture"), Some(&"Auto".to_string()));
        assert_eq!(
            tags.get("Microsoft:PureViewMode"),
            Some(&"5MP Oversampled".to_string())
        );
        assert_eq!(tags.get("Microsoft:Video4K"), Some(&"On".to_string()));
    }
}
