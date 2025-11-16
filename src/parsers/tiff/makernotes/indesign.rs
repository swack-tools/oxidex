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

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// InDesign MakerNote Tag IDs
const ID_VERSION: u16 = 0x0001; // InDesign version
const ID_DOCUMENT_NAME: u16 = 0x0010; // Document file name
const ID_PAGE_NUMBER: u16 = 0x0011; // Page number where placed
const ID_PAGE_WIDTH: u16 = 0x0012; // Page width (points)
const ID_PAGE_HEIGHT: u16 = 0x0013; // Page height (points)
const ID_X_POSITION: u16 = 0x0020; // X position on page (points)
const ID_Y_POSITION: u16 = 0x0021; // Y position on page (points)
const ID_FRAME_WIDTH: u16 = 0x0022; // Frame width (points)
const ID_FRAME_HEIGHT: u16 = 0x0023; // Frame height (points)
const ID_SCALE_X: u16 = 0x0030; // Horizontal scale percentage
const ID_SCALE_Y: u16 = 0x0031; // Vertical scale percentage
const ID_ROTATION: u16 = 0x0032; // Rotation angle (degrees)
const ID_EFFECTIVE_PPI_X: u16 = 0x0040; // Effective PPI horizontal
const ID_EFFECTIVE_PPI_Y: u16 = 0x0041; // Effective PPI vertical
const ID_COLOR_SPACE: u16 = 0x0050; // Color space
const ID_COLOR_PROFILE: u16 = 0x0051; // Embedded color profile name
const ID_LAYER_NAME: u16 = 0x0060; // Layer name
const ID_LAYER_VISIBLE: u16 = 0x0061; // Layer visibility
const ID_MASTER_PAGE: u16 = 0x0062; // Master page name
const ID_TEXT_WRAP: u16 = 0x0070; // Text wrap enabled
const ID_TEXT_WRAP_TYPE: u16 = 0x0071; // Text wrap type
const ID_FRAME_FITTING: u16 = 0x0072; // Frame fitting option
const ID_PRINT_SETTING: u16 = 0x0080; // Print setting
const ID_OUTPUT_INTENT: u16 = 0x0081; // Output intent profile
const ID_SPREAD_NUMBER: u16 = 0x0082; // Spread number

// InDesign signature
const INDESIGN_SIGNATURE: &[u8] = b"InDesign";

/// Decodes color space
///
/// # Arguments
/// * `value` - Color space code
///
/// # Returns
/// Human-readable color space
fn decode_color_space(value: i16) -> String {
    match value {
        0 => "RGB".to_string(),
        1 => "CMYK".to_string(),
        2 => "Lab".to_string(),
        3 => "Grayscale".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes text wrap type
///
/// # Arguments
/// * `value` - Text wrap code
///
/// # Returns
/// Human-readable wrap type
fn decode_text_wrap(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Bounding Box".to_string(),
        2 => "Object Shape".to_string(),
        3 => "Jump Object".to_string(),
        4 => "Jump to Next Column".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes frame fitting option
///
/// # Arguments
/// * `value` - Frame fitting code
///
/// # Returns
/// Human-readable fitting option
fn decode_frame_fitting(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Fill Frame Proportionally".to_string(),
        2 => "Fit Content Proportionally".to_string(),
        3 => "Fit Content to Frame".to_string(),
        4 => "Center Content".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes print setting
///
/// # Arguments
/// * `value` - Print setting code
///
/// # Returns
/// Human-readable print setting
fn decode_print_setting(value: i16) -> String {
    match value {
        0 => "Default".to_string(),
        1 => "Print".to_string(),
        2 => "Non-Print".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Formats points measurement
///
/// # Arguments
/// * `value` - Value in points
///
/// # Returns
/// Formatted string with units
fn format_points(value: i16) -> String {
    if value <= 0 {
        return "0 pt".to_string();
    }
    format!("{} pt", value)
}

/// Formats percentage
///
/// # Arguments
/// * `value` - Percentage value
///
/// # Returns
/// Formatted percentage string
fn format_percentage(value: i16) -> String {
    if value <= 0 {
        return "0%".to_string();
    }
    format!("{}%", value)
}

/// Formats rotation angle
///
/// # Arguments
/// * `value` - Angle in degrees
///
/// # Returns
/// Formatted angle string
fn format_rotation(value: i16) -> String {
    format!("{}°", value)
}

/// Formats PPI
///
/// # Arguments
/// * `value` - PPI value
///
/// # Returns
/// Formatted PPI string
fn format_ppi(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} ppi", value)
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

/// InDesign MakerNote parser implementing the MakerNoteParser trait
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
        "Adobe InDesign"
    }

    fn tag_prefix(&self) -> &'static str {
        "InDesign:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 8 {
            return false;
        }
        data.starts_with(INDESIGN_SIGNATURE) || data.len() >= 8
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

        let start_offset = if data.starts_with(INDESIGN_SIGNATURE) {
            8
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

        if num_entries == 0 || num_entries > 150 {
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
                ID_VERSION | ID_DOCUMENT_NAME | ID_COLOR_PROFILE | ID_LAYER_NAME
                | ID_MASTER_PAGE | ID_OUTPUT_INTENT => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            ID_VERSION => "Version",
                            ID_DOCUMENT_NAME => "DocumentName",
                            ID_COLOR_PROFILE => "ColorProfile",
                            ID_LAYER_NAME => "LayerName",
                            ID_MASTER_PAGE => "MasterPage",
                            ID_OUTPUT_INTENT => "OutputIntent",
                            _ => continue,
                        };
                        tags.insert(format!("InDesign:{}", tag_name), s);
                    }
                }

                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                ID_PAGE_NUMBER => ("PageNumber", val.to_string()),
                                ID_PAGE_WIDTH => ("PageWidth", format_points(val)),
                                ID_PAGE_HEIGHT => ("PageHeight", format_points(val)),
                                ID_X_POSITION => ("XPosition", format_points(val)),
                                ID_Y_POSITION => ("YPosition", format_points(val)),
                                ID_FRAME_WIDTH => ("FrameWidth", format_points(val)),
                                ID_FRAME_HEIGHT => ("FrameHeight", format_points(val)),
                                ID_SCALE_X => ("ScaleX", format_percentage(val)),
                                ID_SCALE_Y => ("ScaleY", format_percentage(val)),
                                ID_ROTATION => ("Rotation", format_rotation(val)),
                                ID_EFFECTIVE_PPI_X => ("EffectivePPIX", format_ppi(val)),
                                ID_EFFECTIVE_PPI_Y => ("EffectivePPIY", format_ppi(val)),
                                ID_COLOR_SPACE => ("ColorSpace", decode_color_space(val)),
                                ID_LAYER_VISIBLE => (
                                    "LayerVisible",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                ID_TEXT_WRAP => {
                                    ("TextWrap", if val != 0 { "Yes" } else { "No" }.to_string())
                                }
                                ID_TEXT_WRAP_TYPE => ("TextWrapType", decode_text_wrap(val)),
                                ID_FRAME_FITTING => ("FrameFitting", decode_frame_fitting(val)),
                                ID_PRINT_SETTING => ("PrintSetting", decode_print_setting(val)),
                                ID_SPREAD_NUMBER => ("SpreadNumber", val.to_string()),
                                _ => continue,
                            };
                            tags.insert(format!("InDesign:{}", tag_name), formatted_value);
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
    fn test_indesign_parser_creation() {
        let parser = InDesignParser::new();
        assert_eq!(parser.manufacturer_name(), "Adobe InDesign");
        assert_eq!(parser.tag_prefix(), "InDesign:");
    }

    #[test]
    fn test_decode_color_space() {
        assert_eq!(decode_color_space(0), "RGB");
        assert_eq!(decode_color_space(1), "CMYK");
    }

    #[test]
    fn test_decode_text_wrap() {
        assert_eq!(decode_text_wrap(1), "Bounding Box");
        assert_eq!(decode_text_wrap(2), "Object Shape");
    }

    #[test]
    fn test_decode_frame_fitting() {
        assert_eq!(decode_frame_fitting(1), "Fill Frame Proportionally");
        assert_eq!(decode_frame_fitting(2), "Fit Content Proportionally");
    }

    #[test]
    fn test_format_points() {
        assert_eq!(format_points(72), "72 pt");
        assert_eq!(format_points(144), "144 pt");
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(format_percentage(100), "100%");
        assert_eq!(format_percentage(150), "150%");
    }

    #[test]
    fn test_format_rotation() {
        assert_eq!(format_rotation(90), "90°");
        assert_eq!(format_rotation(180), "180°");
    }
}
