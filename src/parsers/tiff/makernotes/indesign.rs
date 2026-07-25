//! Adobe InDesign MakerNote parser
//!
//! Parses InDesign document metadata for placed/embedded images.
//! InDesign is a professional desktop publishing application used
//! for magazine layouts, brochures, books, and digital publications.
//!
//! ## Supported Versions
//! - InDesign CC 2024
//! - InDesign CC 2023
//! - InDesign 2022, 2021, 2020
//! - InDesign CS6 and earlier (legacy)
//!
//! ## Key Features
//! - Document page size and dimensions
//! - Image placement coordinates
//! - Effective resolution (scaled)
//! - Rotation and transformation
//! - Layer visibility
//! - Print settings
//! - Color management info
//! - Spread information
//! - Master page reference
//! - Text wrap settings
//! - Frame fitting options
//!
//! ## Architecture
//! InDesign embeds metadata about how images are used within
//! layouts, including placement, scaling, and output settings.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

use super::registries::indesign::indesign_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_string;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

/// Adobe InDesign MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct InDesignParser;

impl InDesignParser {
    /// Creates a new InDesign parser instance
    pub fn new() -> Self {
        InDesignParser
    }
}

impl MakerNoteParser for InDesignParser {
    fn manufacturer_name(&self) -> &'static str {
        "InDesign"
    }

    fn tag_prefix(&self) -> &'static str {
        "InDesign:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 8 {
            return false;
        }
        data.starts_with(b"InDesign") || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("InDesign MakerNote data too short".to_string());
        }

        let registry = indesign_registry();

        // InDesign uses 8-byte signature, then standard IFD format
        let config = IfdParserConfig {
            signature: Some(b"InDesign"),
            signature_offset: 8,
            max_entries: 150,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                // String tags
                if matches!(
                    entry.tag_id,
                    0x0001 | 0x0010 | 0x0051 | 0x0060 | 0x0062 | 0x0081
                ) {
                    if let Some(s) = extract_string(entry, parse_data, byte_order) {
                        tags.insert(format!("InDesign:{}", tag_name), s);
                    }
                } else {
                    // Numeric tags
                    if let Some(array) = super::shared::array_extractors::extract_i16_array(
                        entry, parse_data, byte_order,
                    ) && let Some(&val) = array.first()
                    {
                        let formatted_value = registry.decode_i16(entry.tag_id, val);
                        tags.insert(format!("InDesign:{}", tag_name), formatted_value);
                    }
                }
            }
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indesign_parser_creation() {
        let parser = InDesignParser::new();
        assert_eq!(parser.manufacturer_name(), "InDesign");
        assert_eq!(parser.tag_prefix(), "InDesign:");
    }

    #[test]
    fn test_validate_header() {
        let parser = InDesignParser::new();
        let valid_header = b"InDesign\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
