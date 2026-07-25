//! Reconyx Wildlife Camera MakerNote parser
//!
//! Parses Reconyx-specific EXIF MakerNote tags from trail/wildlife cameras.
//! Reconyx specializes in motion-triggered cameras for wildlife monitoring.
//!
//! ## Supported Models
//! - HyperFire Series (HF2X, HF2XC)
//! - UltraFire Series (XR6, XP9)
//! - MicroFire Series (MR5, MS8)
//! - PC900 (security)
//!
//! ## Key Features
//! - Motion trigger details
//! - Time-lapse interval
//! - Temperature (ambient)
//! - Battery voltage
//! - Moon phase
//! - Sequence number
//! - PIR (infrared) sensor data
//!
//! ## Architecture
//! Reconyx uses specialized metadata for wildlife monitoring applications.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

use super::registries::reconyx::reconyx_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_string;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

/// Reconyx Wildlife Camera MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct ReconxyParser;

impl ReconxyParser {
    /// Creates a new Reconyx parser instance
    pub fn new() -> Self {
        ReconxyParser
    }
}

impl MakerNoteParser for ReconxyParser {
    fn manufacturer_name(&self) -> &'static str {
        "Reconyx"
    }

    fn tag_prefix(&self) -> &'static str {
        "Reconyx:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 7 {
            return false;
        }
        data.starts_with(b"Reconyx") || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Reconyx MakerNote data too short".to_string());
        }

        let registry = reconyx_registry();

        // Reconyx uses 7-byte signature, then standard IFD format
        let config = IfdParserConfig {
            signature: Some(b"Reconyx"),
            signature_offset: 7,
            max_entries: 100,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                // String tags
                if matches!(entry.tag_id, 0x0001..=0x0003) {
                    if let Some(s) = extract_string(entry, parse_data, byte_order) {
                        tags.insert(format!("Reconyx:{}", tag_name), s);
                    }
                } else {
                    // Numeric tags
                    if let Some(array) = super::shared::array_extractors::extract_i16_array(
                        entry, parse_data, byte_order,
                    ) && let Some(&val) = array.first()
                    {
                        let formatted_value = registry.decode_i16(entry.tag_id, val);
                        tags.insert(format!("Reconyx:{}", tag_name), formatted_value);
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
    fn test_reconyx_parser_creation() {
        let parser = ReconxyParser::new();
        assert_eq!(parser.manufacturer_name(), "Reconyx");
        assert_eq!(parser.tag_prefix(), "Reconyx:");
    }

    #[test]
    fn test_validate_header() {
        let parser = ReconxyParser::new();
        let valid_header = b"Reconyx\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
