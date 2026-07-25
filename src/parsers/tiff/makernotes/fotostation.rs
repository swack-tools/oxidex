//! FotoStation/FotoWare MakerNote parser
//!
//! Parses FotoStation/FotoWare asset management metadata.
//! FotoStation is a professional digital asset management (DAM)
//! system used by agencies, corporations, and media organizations.
//!
//! ## Supported Versions
//! - FotoStation 9.x (current)
//! - FotoStation 8.x
//! - FotoWare Cloud
//! - Index Manager integration
//!
//! ## Key Features
//! - Asset workflow status
//! - Archive categories and collections
//! - Approval status and routing
//! - Publication state
//! - Archive location metadata
//! - Asset expiration dates
//! - Rights management status
//! - Taxonomies and controlled vocabularies
//! - Custom field metadata
//! - Version tracking
//! - Check-in/check-out status
//! - Batch processing metadata
//!
//! ## Architecture
//! FotoStation stores DAM workflow metadata in proprietary
//! formats for enterprise asset management and distribution.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

use super::registries::fotostation::fotostation_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_string;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

/// FotoStation MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct FotoStationParser;

impl FotoStationParser {
    /// Creates a new FotoStation parser instance
    pub fn new() -> Self {
        FotoStationParser
    }
}

impl MakerNoteParser for FotoStationParser {
    fn manufacturer_name(&self) -> &'static str {
        "FotoStation"
    }

    fn tag_prefix(&self) -> &'static str {
        "FotoStation:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 8 {
            return false;
        }
        data.starts_with(b"FotoWare") || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("FotoStation MakerNote data too short".to_string());
        }

        let registry = fotostation_registry();

        // FotoStation uses 8-byte signature, then standard IFD format
        let config = IfdParserConfig {
            signature: Some(b"FotoWare"),
            signature_offset: 8,
            max_entries: 150,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                // String tags
                if matches!(entry.tag_id, 0x0001 | 0x0020..=0x0024 | 0x0032 | 0x0040..=0x0043 | 0x0050..=0x0052 | 0x0060..=0x0063 | 0x0070..=0x0072)
                {
                    if let Some(s) = extract_string(entry, parse_data, byte_order) {
                        tags.insert(format!("FotoStation:{}", tag_name), s);
                    }
                } else {
                    // Numeric tags
                    if let Some(array) = super::shared::array_extractors::extract_i16_array(
                        entry, parse_data, byte_order,
                    ) && let Some(&val) = array.first()
                    {
                        let formatted_value = registry.decode_i16(entry.tag_id, val);
                        tags.insert(format!("FotoStation:{}", tag_name), formatted_value);
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
    fn test_fotostation_parser_creation() {
        let parser = FotoStationParser::new();
        assert_eq!(parser.manufacturer_name(), "FotoStation");
        assert_eq!(parser.tag_prefix(), "FotoStation:");
    }

    #[test]
    fn test_validate_header() {
        let parser = FotoStationParser::new();
        let valid_header = b"FotoWare\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
