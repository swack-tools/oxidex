//! GIMP (GNU Image Manipulation Program) MakerNote parser
//!
//! Parses GIMP editing metadata stored in MakerNotes.
//! GIMP is a free and open-source raster graphics editor used for
//! photo retouching, image composition, and image authoring.
//!
//! ## Supported Versions
//! - GIMP 2.10.x (current stable)
//! - GIMP 2.99.x (development)
//! - GIMP 2.8.x (legacy)
//!
//! ## Key Features
//! - Layer count and structure
//! - Layer modes (multiply, overlay, etc.)
//! - Filters applied
//! - Tool history
//! - Color adjustments
//! - Selection information
//! - Path count
//! - Channel information
//! - Undo history depth
//! - Plug-in usage
//! - Script-Fu operations
//! - Parasites (metadata attachments)
//!
//! ## Architecture
//! GIMP stores editing metadata in XCF format internally,
//! but exports simplified metadata to JPEG/PNG MakerNotes.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

use super::registries::gimp::gimp_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_string;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

/// GIMP MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct GimpParser;

impl GimpParser {
    /// Creates a new GIMP parser instance
    pub fn new() -> Self {
        GimpParser
    }
}

impl MakerNoteParser for GimpParser {
    fn manufacturer_name(&self) -> &'static str {
        "GIMP"
    }

    fn tag_prefix(&self) -> &'static str {
        "GIMP:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 4 {
            return false;
        }
        data.starts_with(b"GIMP") || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("GIMP MakerNote data too short".to_string());
        }

        let registry = gimp_registry();

        // GIMP uses 4-byte signature, then standard IFD format
        let config = IfdParserConfig {
            signature: Some(b"GIMP"),
            signature_offset: 4,
            max_entries: 200,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            // Extract value based on tag type and use registry for decoding
            if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                // String tags
                if matches!(entry.tag_id, 0x0001 | 0x0013 | 0x0014) {
                    if let Some(s) = extract_string(entry, parse_data, byte_order) {
                        tags.insert(format!("GIMP:{}", tag_name), s);
                    }
                } else {
                    // Numeric tags - try as i16 array
                    if let Some(array) = super::shared::array_extractors::extract_i16_array(
                        entry, parse_data, byte_order,
                    ) && let Some(&val) = array.first()
                    {
                        let formatted_value = registry.decode_i16(entry.tag_id, val);
                        tags.insert(format!("GIMP:{}", tag_name), formatted_value);
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
    fn test_gimp_parser_creation() {
        let parser = GimpParser::new();
        assert_eq!(parser.manufacturer_name(), "GIMP");
        assert_eq!(parser.tag_prefix(), "GIMP:");
    }

    #[test]
    fn test_validate_header() {
        let parser = GimpParser::new();
        let valid_header = b"GIMP\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
