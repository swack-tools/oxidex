//! Scalado Mobile Photo Editor MakerNote parser
//!
//! Parses Scalado photo editing metadata from mobile applications.
//! Scalado was a mobile imaging technology company acquired by Nokia,
//! with technology integrated into many smartphone camera apps.
//!
//! ## Supported Applications
//! - Scalado Album (legacy)
//! - Scalado PhotoBeamer
//! - Various OEM camera apps (Nokia, Sony Ericsson)
//!
//! ## Key Features
//! - Photo filters applied
//! - Auto-enhance settings
//! - Red-eye reduction
//! - Crop and straighten information
//! - Brightness/contrast adjustments
//! - Effects (vintage, sepia, etc.)
//! - Face detection results
//! - Panorama stitching metadata
//! - HDR processing info
//! - Touch-up areas
//!
//! ## Architecture
//! Scalado stores lightweight editing metadata optimized
//! for mobile devices and quick sharing workflows.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

use super::registries::scalado::scalado_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_string;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

/// Parser for Scalado MakerNotes
#[derive(Default)]
pub struct ScaladoParser;

impl ScaladoParser {
    /// Creates a new Scalado parser instance
    pub fn new() -> Self {
        ScaladoParser
    }
}

impl MakerNoteParser for ScaladoParser {
    fn manufacturer_name(&self) -> &'static str {
        "Scalado"
    }

    fn tag_prefix(&self) -> &'static str {
        "Scalado:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 7 {
            return false;
        }
        data.starts_with(b"Scalado") || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Scalado MakerNote data too short".to_string());
        }

        let registry = scalado_registry();

        // Scalado uses 7-byte signature, then standard IFD format
        let config = IfdParserConfig {
            signature: Some(b"Scalado"),
            signature_offset: 7,
            max_entries: 100,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                // String tags
                if entry.tag_id == 0x0001 {
                    if let Some(s) = extract_string(entry, parse_data, byte_order) {
                        tags.insert(format!("Scalado:{}", tag_name), s);
                    }
                } else {
                    // Numeric tags
                    if let Some(array) = super::shared::array_extractors::extract_i16_array(
                        entry, parse_data, byte_order,
                    ) && let Some(&val) = array.first()
                    {
                        let formatted_value = registry.decode_i16(entry.tag_id, val);
                        tags.insert(format!("Scalado:{}", tag_name), formatted_value);
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
    fn test_scalado_parser_creation() {
        let parser = ScaladoParser::new();
        assert_eq!(parser.manufacturer_name(), "Scalado");
        assert_eq!(parser.tag_prefix(), "Scalado:");
    }

    #[test]
    fn test_validate_header() {
        let parser = ScaladoParser::new();
        let valid_header = b"Scalado\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
