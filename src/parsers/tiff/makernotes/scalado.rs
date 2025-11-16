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

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// Scalado MakerNote Tag IDs
const SCALADO_VERSION: u16 = 0x0001; // Scalado version
const SCALADO_FILTER_TYPE: u16 = 0x0010; // Filter type applied
const SCALADO_AUTO_ENHANCE: u16 = 0x0011; // Auto-enhance level
const SCALADO_RED_EYE: u16 = 0x0012; // Red-eye reduction applied
const SCALADO_BRIGHTNESS: u16 = 0x0020; // Brightness adjustment
const SCALADO_CONTRAST: u16 = 0x0021; // Contrast adjustment
const SCALADO_SATURATION: u16 = 0x0022; // Saturation adjustment
const SCALADO_CROP_APPLIED: u16 = 0x0030; // Crop applied flag
const SCALADO_STRAIGHTEN_ANGLE: u16 = 0x0031; // Straighten angle
const SCALADO_FACE_COUNT: u16 = 0x0040; // Faces detected
const SCALADO_PANORAMA: u16 = 0x0041; // Panorama stitched
const SCALADO_HDR: u16 = 0x0042; // HDR processing
const SCALADO_TOUCHUP_COUNT: u16 = 0x0043; // Touch-up areas

// Scalado signature
const SCALADO_SIGNATURE: &[u8] = b"Scalado";

/// Decodes filter type
///
/// # Arguments
/// * `value` - Filter type code
///
/// # Returns
/// Human-readable filter name
fn decode_filter_type(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Vintage".to_string(),
        2 => "Sepia".to_string(),
        3 => "Black & White".to_string(),
        4 => "Cool".to_string(),
        5 => "Warm".to_string(),
        6 => "Vivid".to_string(),
        7 => "Soft".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes auto-enhance level
///
/// # Arguments
/// * `value` - Auto-enhance code
///
/// # Returns
/// Human-readable level
fn decode_auto_enhance(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Low".to_string(),
        2 => "Medium".to_string(),
        3 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Formats adjustment percentage
///
/// # Arguments
/// * `value` - Adjustment value (-100 to +100)
///
/// # Returns
/// Formatted string
fn format_adjustment(value: i16) -> String {
    if value >= 0 {
        format!("+{}", value)
    } else {
        format!("{}", value)
    }
}

/// Extracts an ASCII string from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the string
/// * `data` - Raw MakerNote data
///
/// # Returns
/// Extracted string or None if extraction fails
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

    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Scalado MakerNote parser implementing the MakerNoteParser trait
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
        data.starts_with(SCALADO_SIGNATURE) || data.len() >= 8
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

        let start_offset = if data.starts_with(SCALADO_SIGNATURE) {
            7
        } else {
            0
        };
        let parse_data = &data[start_offset..];

        if parse_data.len() < 2 {
            return Ok(());
        }

        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([parse_data[0], parse_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([parse_data[0], parse_data[1]]),
        } as usize;

        if num_entries == 0 || num_entries > 100 {
            return Ok(());
        }

        let mut offset = 2;
        let entry_size = 12;

        for _ in 0..num_entries {
            if offset + entry_size > parse_data.len() {
                break;
            }

            let entry_data = &parse_data[offset..offset + entry_size];

            let tag = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[0], entry_data[1]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[0], entry_data[1]]),
            };

            let field_type = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[2], entry_data[3]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[2], entry_data[3]]),
            };

            let count = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
            };

            let value_offset = match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
                ByteOrder::BigEndian => u32::from_be_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
            };

            let entry = IfdEntry {
                tag_id: tag,
                field_type,
                value_count: count,
                value_offset,
            };

            match tag {
                SCALADO_VERSION => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        tags.insert("Scalado:Version".to_string(), s);
                    }
                }

                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                SCALADO_FILTER_TYPE => ("FilterType", decode_filter_type(val)),
                                SCALADO_AUTO_ENHANCE => ("AutoEnhance", decode_auto_enhance(val)),
                                SCALADO_RED_EYE => (
                                    "RedEyeReduction",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                SCALADO_BRIGHTNESS => ("Brightness", format_adjustment(val)),
                                SCALADO_CONTRAST => ("Contrast", format_adjustment(val)),
                                SCALADO_SATURATION => ("Saturation", format_adjustment(val)),
                                SCALADO_CROP_APPLIED => (
                                    "CropApplied",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                SCALADO_STRAIGHTEN_ANGLE => {
                                    ("StraightenAngle", format!("{}°", val))
                                }
                                SCALADO_FACE_COUNT => ("FacesDetected", val.to_string()),
                                SCALADO_PANORAMA => {
                                    ("Panorama", if val != 0 { "Yes" } else { "No" }.to_string())
                                }
                                SCALADO_HDR => {
                                    ("HDR", if val != 0 { "Yes" } else { "No" }.to_string())
                                }
                                SCALADO_TOUCHUP_COUNT => ("TouchupCount", val.to_string()),
                                _ => continue,
                            };
                            tags.insert(format!("Scalado:{}", tag_name), formatted_value);
                        }
                    }
                }
            }

            offset += entry_size;
        }

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
    fn test_decode_filter_type() {
        assert_eq!(decode_filter_type(1), "Vintage");
        assert_eq!(decode_filter_type(2), "Sepia");
        assert_eq!(decode_filter_type(6), "Vivid");
    }

    #[test]
    fn test_decode_auto_enhance() {
        assert_eq!(decode_auto_enhance(0), "Off");
        assert_eq!(decode_auto_enhance(2), "Medium");
        assert_eq!(decode_auto_enhance(3), "High");
    }

    #[test]
    fn test_format_adjustment() {
        assert_eq!(format_adjustment(25), "+25");
        assert_eq!(format_adjustment(-15), "-15");
    }
}
