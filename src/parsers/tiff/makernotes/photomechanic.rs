//! Photo Mechanic MakerNote parser
//!
//! Parses Photo Mechanic workflow and IPTC metadata stored in MakerNotes.
//! Photo Mechanic is a professional photo browser and workflow tool
//! used by sports photographers, photojournalists, and agencies.
//!
//! ## Supported Versions
//! - Photo Mechanic 6 (current)
//! - Photo Mechanic Plus
//! - Photo Mechanic 5
//! - Photo Mechanic 4.6
//!
//! ## Key Features
//! - IPTC workflow status
//! - Star ratings and color classes
//! - Keywords and categories
//! - Caption and headline
//! - Copyright and credit information
//! - Location metadata (city, state, country)
//! - Person/subject identification
//! - Batch tagging metadata
//! - Code replacement variables
//! - Structured keywords
//! - Ingestion settings
//! - Contact sheet information
//! - FTP upload metadata
//!
//! ## Architecture
//! Photo Mechanic stores workflow metadata in IPTC-compatible
//! formats within MakerNotes for rapid access during culling
//! and selection workflows.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

use super::registries::photomechanic::photomechanic_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_string;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

/// Photo Mechanic MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct PhotoMechanicParser;

impl PhotoMechanicParser {
    /// Creates a new Photo Mechanic parser instance
    pub fn new() -> Self {
        PhotoMechanicParser
    }
}

impl MakerNoteParser for PhotoMechanicParser {
    fn manufacturer_name(&self) -> &'static str {
        "Photo Mechanic"
    }

    fn tag_prefix(&self) -> &'static str {
        "PhotoMechanic:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 9 {
            return false;
        }
        data.starts_with(b"PhotoMech") || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Photo Mechanic MakerNote data too short".to_string());
        }

        let registry = photomechanic_registry();

        // Photo Mechanic uses 9-byte signature, then standard IFD format
        let config = IfdParserConfig {
            signature: Some(b"PhotoMech"),
            signature_offset: 9,
            max_entries: 200,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                // String tags
                if matches!(entry.tag_id, 0x0001 | 0x0020..=0x0035 | 0x0040..=0x0044 | 0x0050..=0x0052 | 0x0060..=0x0061 | 0x0070..=0x0072 | 0x0080..=0x0083 | 0x0090..=0x0091 | 0x00C1)
                {
                    if let Some(s) = extract_string(entry, parse_data, byte_order) {
                        tags.insert(format!("PhotoMechanic:{}", tag_name), s);
                    }
                } else {
                    // Numeric tags
                    if let Some(array) = super::shared::array_extractors::extract_i16_array(
                        entry, parse_data, byte_order,
                    ) && let Some(&val) = array.first()
                    {
                        let formatted_value = registry.decode_i16(entry.tag_id, val);
                        tags.insert(format!("PhotoMechanic:{}", tag_name), formatted_value);
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
    fn test_photomechanic_parser_creation() {
        let parser = PhotoMechanicParser::new();
        assert_eq!(parser.manufacturer_name(), "Photo Mechanic");
        assert_eq!(parser.tag_prefix(), "PhotoMechanic:");
    }

    #[test]
    fn test_validate_header() {
        let parser = PhotoMechanicParser::new();
        let valid_header = b"PhotoMech\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
