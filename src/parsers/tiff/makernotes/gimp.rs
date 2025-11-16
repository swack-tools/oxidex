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

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// GIMP MakerNote Tag IDs
const GIMP_VERSION: u16 = 0x0001; // GIMP version
const GIMP_LAYER_COUNT: u16 = 0x0010; // Number of layers
const GIMP_LAYER_MODES: u16 = 0x0011; // Layer modes used (bitmask)
const GIMP_FILTER_COUNT: u16 = 0x0012; // Filters applied count
const GIMP_FILTER_NAMES: u16 = 0x0013; // Filter names
const GIMP_TOOL_HISTORY: u16 = 0x0014; // Tools used
const GIMP_COLOR_CURVE: u16 = 0x0020; // Curves adjustment applied
const GIMP_COLOR_LEVELS: u16 = 0x0021; // Levels adjustment applied
const GIMP_HUE_SATURATION: u16 = 0x0022; // Hue/Saturation adjusted
const GIMP_BRIGHTNESS_CONTRAST: u16 = 0x0023; // Brightness/Contrast
const GIMP_COLOR_BALANCE: u16 = 0x0024; // Color balance adjusted
const GIMP_THRESHOLD: u16 = 0x0025; // Threshold applied
const GIMP_POSTERIZE: u16 = 0x0026; // Posterize applied
const GIMP_DESATURATE: u16 = 0x0027; // Desaturate applied
const GIMP_SELECTION_ACTIVE: u16 = 0x0030; // Selection present
const GIMP_SELECTION_TYPE: u16 = 0x0031; // Selection type
const GIMP_PATH_COUNT: u16 = 0x0032; // Number of paths
const GIMP_CHANNEL_COUNT: u16 = 0x0033; // Number of channels
const GIMP_ALPHA_CHANNEL: u16 = 0x0034; // Alpha channel present
const GIMP_UNDO_LEVELS: u16 = 0x0040; // Undo history depth
const GIMP_PLUGIN_COUNT: u16 = 0x0041; // Plug-ins used count
const GIMP_SCRIPT_FU_COUNT: u16 = 0x0042; // Script-Fu operations
const GIMP_BLUR_COUNT: u16 = 0x0050; // Blur filter count
const GIMP_SHARPEN_COUNT: u16 = 0x0051; // Sharpen filter count
const GIMP_NOISE_COUNT: u16 = 0x0052; // Noise filter count
const GIMP_DISTORT_COUNT: u16 = 0x0053; // Distort filter count
const GIMP_EDGE_DETECT_COUNT: u16 = 0x0054; // Edge detect count
const GIMP_ENHANCE_COUNT: u16 = 0x0055; // Enhance filter count
const GIMP_RENDER_COUNT: u16 = 0x0056; // Render filter count
const GIMP_PARASITES: u16 = 0x0060; // Parasites count

// GIMP signature
const GIMP_SIGNATURE: &[u8] = b"GIMP";

/// Decodes layer modes bitmask
///
/// # Arguments
/// * `value` - Bitmask of layer modes used
///
/// # Returns
/// Comma-separated list of layer modes
fn decode_layer_modes(value: i16) -> String {
    let mut modes = Vec::new();

    if value & 0x01 != 0 {
        modes.push("Normal");
    }
    if value & 0x02 != 0 {
        modes.push("Dissolve");
    }
    if value & 0x04 != 0 {
        modes.push("Multiply");
    }
    if value & 0x08 != 0 {
        modes.push("Screen");
    }
    if value & 0x10 != 0 {
        modes.push("Overlay");
    }
    if value & 0x20 != 0 {
        modes.push("Difference");
    }
    if value & 0x40 != 0 {
        modes.push("Addition");
    }
    if value & 0x80 != 0 {
        modes.push("Subtract");
    }
    if value & 0x100 != 0 {
        modes.push("Darken Only");
    }
    if value & 0x200 != 0 {
        modes.push("Lighten Only");
    }

    if modes.is_empty() {
        "None".to_string()
    } else {
        modes.join(", ")
    }
}

/// Decodes selection type
///
/// # Arguments
/// * `value` - Selection type code
///
/// # Returns
/// Human-readable selection type
fn decode_selection_type(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Rectangle".to_string(),
        2 => "Ellipse".to_string(),
        3 => "Free".to_string(),
        4 => "Fuzzy".to_string(),
        5 => "By Color".to_string(),
        6 => "Path".to_string(),
        _ => format!("Unknown ({})", value),
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
        data.starts_with(GIMP_SIGNATURE) || data.len() >= 8
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

        // Skip GIMP signature if present
        let start_offset = if data.starts_with(GIMP_SIGNATURE) {
            4
        } else {
            0
        };
        let parse_data = &data[start_offset..];

        if parse_data.len() < 2 {
            return Ok(());
        }

        // Read number of entries
        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([parse_data[0], parse_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([parse_data[0], parse_data[1]]),
        } as usize;

        if num_entries == 0 || num_entries > 200 {
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

            // Extract value based on tag type
            match tag {
                GIMP_VERSION | GIMP_FILTER_NAMES | GIMP_TOOL_HISTORY => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            GIMP_VERSION => "Version",
                            GIMP_FILTER_NAMES => "FiltersApplied",
                            GIMP_TOOL_HISTORY => "ToolsUsed",
                            _ => continue,
                        };
                        tags.insert(format!("GIMP:{}", tag_name), s);
                    }
                }

                _ => {
                    // Try to extract as i16 array
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                GIMP_LAYER_COUNT => ("LayerCount", val.to_string()),
                                GIMP_LAYER_MODES => ("LayerModes", decode_layer_modes(val)),
                                GIMP_FILTER_COUNT => ("FilterCount", val.to_string()),
                                GIMP_COLOR_CURVE => (
                                    "CurvesAdjusted",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_COLOR_LEVELS => (
                                    "LevelsAdjusted",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_HUE_SATURATION => (
                                    "HueSaturationAdjusted",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_BRIGHTNESS_CONTRAST => (
                                    "BrightnessContrastAdjusted",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_COLOR_BALANCE => (
                                    "ColorBalanceAdjusted",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_THRESHOLD => (
                                    "ThresholdApplied",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_POSTERIZE => (
                                    "PosterizeApplied",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_DESATURATE => (
                                    "DesaturateApplied",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_SELECTION_ACTIVE => (
                                    "SelectionActive",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                GIMP_SELECTION_TYPE => {
                                    ("SelectionType", decode_selection_type(val))
                                }
                                GIMP_PATH_COUNT => ("PathCount", val.to_string()),
                                GIMP_CHANNEL_COUNT => ("ChannelCount", val.to_string()),
                                GIMP_ALPHA_CHANNEL => (
                                    "AlphaChannel",
                                    if val != 0 { "Present" } else { "None" }.to_string(),
                                ),
                                GIMP_UNDO_LEVELS => ("UndoLevels", val.to_string()),
                                GIMP_PLUGIN_COUNT => ("PluginCount", val.to_string()),
                                GIMP_SCRIPT_FU_COUNT => ("ScriptFuCount", val.to_string()),
                                GIMP_BLUR_COUNT => ("BlurFilterCount", val.to_string()),
                                GIMP_SHARPEN_COUNT => ("SharpenFilterCount", val.to_string()),
                                GIMP_NOISE_COUNT => ("NoiseFilterCount", val.to_string()),
                                GIMP_DISTORT_COUNT => ("DistortFilterCount", val.to_string()),
                                GIMP_EDGE_DETECT_COUNT => ("EdgeDetectCount", val.to_string()),
                                GIMP_ENHANCE_COUNT => ("EnhanceFilterCount", val.to_string()),
                                GIMP_RENDER_COUNT => ("RenderFilterCount", val.to_string()),
                                GIMP_PARASITES => ("ParasitesCount", val.to_string()),
                                _ => continue,
                            };
                            tags.insert(format!("GIMP:{}", tag_name), formatted_value);
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
    fn test_gimp_parser_creation() {
        let parser = GimpParser::new();
        assert_eq!(parser.manufacturer_name(), "GIMP");
        assert_eq!(parser.tag_prefix(), "GIMP:");
    }

    #[test]
    fn test_decode_layer_modes() {
        assert_eq!(decode_layer_modes(0x01), "Normal");
        assert_eq!(decode_layer_modes(0x05), "Normal, Multiply");
    }

    #[test]
    fn test_decode_selection_type() {
        assert_eq!(decode_selection_type(1), "Rectangle");
        assert_eq!(decode_selection_type(2), "Ellipse");
        assert_eq!(decode_selection_type(4), "Fuzzy");
    }

    #[test]
    fn test_validate_header() {
        let parser = GimpParser::new();
        let valid_header = b"GIMP\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
