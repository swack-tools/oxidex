//! InfiRay Thermal Camera MakerNote parser
//!
//! Parses InfiRay-specific EXIF MakerNote tags from thermal imaging cameras.
//! InfiRay is a Chinese manufacturer of thermal imaging sensors and cameras
//! with growing presence in industrial and consumer markets.
//!
//! ## Supported Models
//! - InfiRay P2 Pro
//! - InfiRay T2 Pro
//! - InfiRay T3 Series
//! - InfiRay C-Series
//! - InfiRay E-Series (industrial)
//! - InfiRay Outdoor thermal scopes
//!
//! ## Key Features
//! - Temperature measurement (min/max/center)
//! - Emissivity setting
//! - Thermal palette/colormap
//! - Measurement range
//! - Distance compensation
//! - Atmospheric parameters
//! - Image enhancement mode
//!
//! ## Architecture
//! Similar to FLIR but with simplified tag structure.
//! InfiRay uses a subset of thermal imaging metadata.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::infiray::infiray_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_i16_array;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

const INFIRAY_SIGNATURE: &[u8] = b"InfiRay";

// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(infiray_registry);

/// Decodes InfiRay color palette
fn decode_palette(value: i16) -> String {
    match value {
        0 => "White Hot".to_string(),
        1 => "Black Hot".to_string(),
        2 => "Iron Red".to_string(),
        3 => "Rainbow".to_string(),
        4 => "Lava".to_string(),
        5 => "Arctic".to_string(),
        6 => "Gradient".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes image enhancement mode
fn decode_enhancement(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Low".to_string(),
        2 => "Medium".to_string(),
        3 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes temperature unit
fn decode_unit(value: i16) -> String {
    match value {
        0 => "°C".to_string(),
        1 => "°F".to_string(),
        2 => "K".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Formats temperature from scaled integer
fn format_temperature(value: i16) -> String {
    let temp = value as f64 / 10.0;
    format!("{:.1}°C", temp)
}

/// Formats emissivity
fn format_emissivity(value: i16) -> String {
    let emissivity = value as f64 / 100.0;
    format!("{:.2}", emissivity)
}

/// Formats distance
fn format_distance(value: i16) -> String {
    if value < 100 {
        format!("{} cm", value)
    } else {
        format!("{:.2} m", value as f64 / 100.0)
    }
}

/// Formats zoom level
fn format_zoom(value: i16) -> String {
    if value <= 100 {
        "1.0x".to_string()
    } else {
        format!("{:.1}x", value as f64 / 100.0)
    }
}

/// Extracts string from IFD entry
fn extract_string(entry: &IfdEntry, data: &[u8]) -> Option<String> {
    if entry.field_type != 2 {
        return None;
    }

    let offset = entry.value_offset as usize;
    let count = entry.value_count as usize;

    if count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = String::from_utf8_lossy(&bytes[..count.min(4)])
            .trim_end_matches('\0')
            .to_string();
        return if s.is_empty() { None } else { Some(s) };
    }

    if offset + count > data.len() {
        return None;
    }

    let s = String::from_utf8_lossy(&data[offset..offset + count])
        .trim_end_matches('\0')
        .to_string();

    if s.is_empty() { None } else { Some(s) }
}

/// InfiRay Thermal Camera MakerNote parser
/// Default implementation for parser
#[derive(Default)]
pub struct InfiRayParser;

impl InfiRayParser {
    /// Creates a new InfiRay parser instance
    pub fn new() -> Self {
        InfiRayParser
    }
}

impl InfiRayParser {
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_id = entry.tag_id;

        // Handle string tags (Model, Serial, Firmware)
        if let 0x0001..=0x0003 = tag_id {
            if let Some(s) = extract_string(entry, data)
                && let Some(name) = TAG_REGISTRY.get_tag_name(tag_id)
            {
                tags.insert(format!("InfiRay:{}", name), s);
            }
            return;
        }

        // Handle i16 array tags
        if let Some(array) = extract_i16_array(entry, data, byte_order)
            && let Some(&val) = array.first()
        {
            let tag_name = match TAG_REGISTRY.get_tag_name(tag_id) {
                Some(name) => name,
                None => return,
            };

            let formatted_value = match tag_id {
                0x0100..=0x0102 => format_temperature(val),
                0x0103 => format_emissivity(val),
                0x0104 => format_distance(val),
                0x0105 => decode_palette(val),
                0x0106 | 0x0107 => format_temperature(val),
                0x0108 => format_temperature(val),
                0x0109 => format!("{}%", val),
                0x010A => decode_enhancement(val),
                0x010B => format_zoom(val),
                0x010C..=0x010E => val.to_string(),
                0x010F | 0x0110 => {
                    if val != 0 {
                        "On".to_string()
                    } else {
                        "Off".to_string()
                    }
                }
                0x0111 => decode_unit(val),
                _ => return,
            };

            tags.insert(format!("InfiRay:{}", tag_name), formatted_value);
        }
    }
}

impl MakerNoteParser for InfiRayParser {
    fn manufacturer_name(&self) -> &'static str {
        "InfiRay"
    }

    fn tag_prefix(&self) -> &'static str {
        "InfiRay:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 7 {
            return false;
        }
        data.starts_with(INFIRAY_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        let config = IfdParserConfig {
            signature: Some(INFIRAY_SIGNATURE),
            signature_offset: 7,
            max_entries: 200,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infiray_parser_creation() {
        let parser = InfiRayParser::new();
        assert_eq!(parser.manufacturer_name(), "InfiRay");
        assert_eq!(parser.tag_prefix(), "InfiRay:");
    }

    #[test]
    fn test_decode_palette() {
        assert_eq!(decode_palette(0), "White Hot");
        assert_eq!(decode_palette(2), "Iron Red");
    }

    #[test]
    fn test_format_temperature() {
        assert_eq!(format_temperature(250), "25.0°C");
        assert_eq!(format_temperature(-50), "-5.0°C");
    }

    #[test]
    fn test_format_emissivity() {
        assert_eq!(format_emissivity(95), "0.95");
        assert_eq!(format_emissivity(100), "1.00");
    }

    #[test]
    fn test_format_distance() {
        assert_eq!(format_distance(50), "50 cm");
        assert_eq!(format_distance(250), "2.50 m");
    }

    #[test]
    fn test_format_zoom() {
        assert_eq!(format_zoom(100), "1.0x");
        assert_eq!(format_zoom(200), "2.0x");
    }

    #[test]
    fn test_decode_enhancement() {
        assert_eq!(decode_enhancement(0), "Off");
        assert_eq!(decode_enhancement(3), "High");
    }

    #[test]
    fn test_decode_unit() {
        assert_eq!(decode_unit(0), "°C");
        assert_eq!(decode_unit(1), "°F");
        assert_eq!(decode_unit(2), "K");
    }
}
