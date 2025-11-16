//! Adobe Photoshop MakerNote parser
//!
//! Parses Photoshop-specific editing metadata stored in MakerNotes.
//! Contains layer information, adjustment details, filters applied,
//! edit history, and various processing parameters.
//!
//! ## Supported Versions
//! - Photoshop CC 2015-2024
//! - Photoshop CS6 and earlier
//! - Photoshop Elements
//! - Photoshop Lightroom (when edited with Photoshop)
//!
//! ## Key Features
//! - Layer count and structure information
//! - Adjustment layers (Curves, Levels, Hue/Saturation)
//! - Filters applied (Gaussian Blur, Sharpen, etc.)
//! - Edit history and action count
//! - Smart Object information
//! - Color mode and bit depth
//! - Document resolution settings
//! - Blending modes used
//! - Layer effects (shadows, glows, bevels)
//! - Text layer information
//! - Shape layer data
//! - Mask information
//! - Alpha channel count
//!
//! ## Architecture
//! Photoshop stores extensive editing metadata in proprietary formats.
//! This parser extracts the most commonly needed information from
//! the MakerNotes structure.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// Photoshop MakerNote Tag IDs
const PS_VERSION: u16 = 0x0001; // Photoshop version
const PS_LAYER_COUNT: u16 = 0x0010; // Number of layers
const PS_LAYER_NAMES: u16 = 0x0011; // Layer names (comma-separated)
const PS_ADJUSTMENT_COUNT: u16 = 0x0012; // Number of adjustment layers
const PS_ADJUSTMENT_TYPES: u16 = 0x0013; // Adjustment layer types
const PS_FILTER_COUNT: u16 = 0x0014; // Number of filters applied
const PS_FILTER_NAMES: u16 = 0x0015; // Filter names
const PS_EDIT_COUNT: u16 = 0x0016; // Number of edits in history
const PS_ACTION_COUNT: u16 = 0x0017; // Number of actions executed
const PS_SMART_OBJECT_COUNT: u16 = 0x0018; // Number of smart objects
const PS_COLOR_MODE: u16 = 0x0020; // Color mode (RGB, CMYK, etc.)
const PS_BIT_DEPTH: u16 = 0x0021; // Bit depth (8, 16, 32)
const PS_DPI_HORIZONTAL: u16 = 0x0022; // Horizontal DPI
const PS_DPI_VERTICAL: u16 = 0x0023; // Vertical DPI
const PS_WIDTH_PIXELS: u16 = 0x0024; // Document width in pixels
const PS_HEIGHT_PIXELS: u16 = 0x0025; // Document height in pixels
const PS_BLENDING_MODES: u16 = 0x0030; // Blending modes used (bitmask)
const PS_LAYER_EFFECTS: u16 = 0x0031; // Layer effects used (bitmask)
const PS_TEXT_LAYER_COUNT: u16 = 0x0032; // Number of text layers
const PS_SHAPE_LAYER_COUNT: u16 = 0x0033; // Number of shape layers
const PS_ADJUSTMENT_LAYER_COUNT: u16 = 0x0034; // Number of adjustment layers
const PS_FILL_LAYER_COUNT: u16 = 0x0035; // Number of fill layers
const PS_MASK_COUNT: u16 = 0x0040; // Number of layer masks
const PS_VECTOR_MASK_COUNT: u16 = 0x0041; // Number of vector masks
const PS_CLIPPING_MASK_COUNT: u16 = 0x0042; // Number of clipping masks
const PS_ALPHA_CHANNEL_COUNT: u16 = 0x0043; // Number of alpha channels
const PS_SPOT_CHANNEL_COUNT: u16 = 0x0044; // Number of spot channels
const PS_HAS_CURVES: u16 = 0x0050; // Curves adjustment present
const PS_HAS_LEVELS: u16 = 0x0051; // Levels adjustment present
const PS_HAS_HUE_SAT: u16 = 0x0052; // Hue/Saturation adjustment present
const PS_HAS_COLOR_BALANCE: u16 = 0x0053; // Color Balance adjustment present
const PS_HAS_BRIGHTNESS_CONTRAST: u16 = 0x0054; // Brightness/Contrast present
const PS_HAS_VIBRANCE: u16 = 0x0055; // Vibrance adjustment present
const PS_HAS_EXPOSURE: u16 = 0x0056; // Exposure adjustment present
const PS_HAS_SHADOWS_HIGHLIGHTS: u16 = 0x0057; // Shadows/Highlights present
const PS_GAUSSIAN_BLUR_COUNT: u16 = 0x0060; // Gaussian blur filter applied count
const PS_SHARPEN_COUNT: u16 = 0x0061; // Sharpen filter applied count
const PS_SMART_SHARPEN_COUNT: u16 = 0x0062; // Smart Sharpen applied count
const PS_UNSHARP_MASK_COUNT: u16 = 0x0063; // Unsharp Mask applied count
const PS_NOISE_REDUCTION_COUNT: u16 = 0x0064; // Noise reduction applied count
const PS_LIQUIFY_COUNT: u16 = 0x0065; // Liquify filter applied count
const PS_CAMERA_RAW_COUNT: u16 = 0x0066; // Camera Raw filter applied count
const PS_NEURAL_FILTER_COUNT: u16 = 0x0067; // Neural filters applied count
const PS_LAST_SAVE_TIME: u16 = 0x0070; // Last save timestamp
const PS_CREATION_TIME: u16 = 0x0071; // Document creation timestamp
const PS_TOTAL_EDIT_TIME: u16 = 0x0072; // Total editing time (minutes)
const PS_MODIFIED_FLAG: u16 = 0x0073; // Document modified flag
const PS_BACKUP_COUNT: u16 = 0x0074; // Number of backups created
const PS_LAYER_COMP_COUNT: u16 = 0x0080; // Number of layer comps
const PS_ACTIVE_LAYER_COMP: u16 = 0x0081; // Active layer comp name
const PS_GUIDE_COUNT: u16 = 0x0082; // Number of guides
const PS_GRID_ENABLED: u16 = 0x0083; // Grid visibility
const PS_RULER_UNITS: u16 = 0x0084; // Ruler units (pixels, inches, cm)
const PS_COLOR_PROFILE: u16 = 0x0090; // Embedded color profile name
const PS_PROOF_SETUP: u16 = 0x0091; // Proof setup name
const PS_WORKING_COLOR_SPACE: u16 = 0x0092; // Working color space

// Photoshop signature
const PHOTOSHOP_SIGNATURE: &[u8] = b"Adobe Photoshop";

/// Decodes Photoshop color mode
///
/// # Arguments
/// * `value` - Color mode code
///
/// # Returns
/// Human-readable color mode name
fn decode_color_mode(value: i16) -> String {
    match value {
        0 => "Bitmap".to_string(),
        1 => "Grayscale".to_string(),
        2 => "Indexed".to_string(),
        3 => "RGB".to_string(),
        4 => "CMYK".to_string(),
        5 => "Multichannel".to_string(),
        6 => "Duotone".to_string(),
        7 => "Lab".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes bit depth
///
/// # Arguments
/// * `value` - Bit depth value
///
/// # Returns
/// Formatted bit depth string
fn decode_bit_depth(value: i16) -> String {
    match value {
        1 => "1-bit".to_string(),
        8 => "8-bit".to_string(),
        16 => "16-bit".to_string(),
        32 => "32-bit".to_string(),
        _ => format!("{}-bit", value),
    }
}

/// Decodes ruler units
///
/// # Arguments
/// * `value` - Ruler units code
///
/// # Returns
/// Human-readable units
fn decode_ruler_units(value: i16) -> String {
    match value {
        1 => "Inches".to_string(),
        2 => "Centimeters".to_string(),
        3 => "Points".to_string(),
        4 => "Picas".to_string(),
        5 => "Pixels".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes blending modes bitmask
///
/// # Arguments
/// * `value` - Bitmask of blending modes used
///
/// # Returns
/// Comma-separated list of blending modes
fn decode_blending_modes(value: i16) -> String {
    let mut modes = Vec::new();

    if value & 0x01 != 0 {
        modes.push("Normal");
    }
    if value & 0x02 != 0 {
        modes.push("Multiply");
    }
    if value & 0x04 != 0 {
        modes.push("Screen");
    }
    if value & 0x08 != 0 {
        modes.push("Overlay");
    }
    if value & 0x10 != 0 {
        modes.push("Soft Light");
    }
    if value & 0x20 != 0 {
        modes.push("Hard Light");
    }
    if value & 0x40 != 0 {
        modes.push("Color Dodge");
    }
    if value & 0x80 != 0 {
        modes.push("Color Burn");
    }
    if value & 0x100 != 0 {
        modes.push("Darken");
    }
    if value & 0x200 != 0 {
        modes.push("Lighten");
    }

    if modes.is_empty() {
        "None".to_string()
    } else {
        modes.join(", ")
    }
}

/// Decodes layer effects bitmask
///
/// # Arguments
/// * `value` - Bitmask of layer effects used
///
/// # Returns
/// Comma-separated list of effects
fn decode_layer_effects(value: i16) -> String {
    let mut effects = Vec::new();

    if value & 0x01 != 0 {
        effects.push("Drop Shadow");
    }
    if value & 0x02 != 0 {
        effects.push("Inner Shadow");
    }
    if value & 0x04 != 0 {
        effects.push("Outer Glow");
    }
    if value & 0x08 != 0 {
        effects.push("Inner Glow");
    }
    if value & 0x10 != 0 {
        effects.push("Bevel and Emboss");
    }
    if value & 0x20 != 0 {
        effects.push("Satin");
    }
    if value & 0x40 != 0 {
        effects.push("Color Overlay");
    }
    if value & 0x80 != 0 {
        effects.push("Gradient Overlay");
    }
    if value & 0x100 != 0 {
        effects.push("Pattern Overlay");
    }
    if value & 0x200 != 0 {
        effects.push("Stroke");
    }

    if effects.is_empty() {
        "None".to_string()
    } else {
        effects.join(", ")
    }
}

/// Formats resolution in DPI
///
/// # Arguments
/// * `value` - DPI value
///
/// # Returns
/// Formatted DPI string
fn format_dpi(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} dpi", value)
}

/// Formats time duration
///
/// # Arguments
/// * `minutes` - Duration in minutes
///
/// # Returns
/// Formatted duration string
fn format_time_duration(minutes: i16) -> String {
    if minutes < 0 {
        return "Unknown".to_string();
    }
    if minutes < 60 {
        format!("{} min", minutes)
    } else {
        let hours = minutes / 60;
        let mins = minutes % 60;
        if mins == 0 {
            format!("{} hr", hours)
        } else {
            format!("{} hr {} min", hours, mins)
        }
    }
}

/// Formats timestamp
///
/// # Arguments
/// * `value` - Unix timestamp or proprietary timestamp
///
/// # Returns
/// Formatted timestamp string
fn format_timestamp(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    // For simplicity, return raw value
    // In production, would convert to human-readable format
    format!("Timestamp: {}", value)
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

/// Photoshop MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct PhotoshopParser;

impl PhotoshopParser {
    /// Creates a new Photoshop parser instance
    pub fn new() -> Self {
        PhotoshopParser
    }
}

impl MakerNoteParser for PhotoshopParser {
    fn manufacturer_name(&self) -> &'static str {
        "Adobe Photoshop"
    }

    fn tag_prefix(&self) -> &'static str {
        "Photoshop:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 15 {
            return false;
        }
        data.starts_with(PHOTOSHOP_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Photoshop MakerNote data too short".to_string());
        }

        // Skip Photoshop signature if present
        let start_offset = if data.starts_with(PHOTOSHOP_SIGNATURE) {
            15
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

        if num_entries == 0 || num_entries > 500 {
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
                PS_VERSION
                | PS_LAYER_NAMES
                | PS_ADJUSTMENT_TYPES
                | PS_FILTER_NAMES
                | PS_ACTIVE_LAYER_COMP
                | PS_COLOR_PROFILE
                | PS_PROOF_SETUP
                | PS_WORKING_COLOR_SPACE => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            PS_VERSION => "Version",
                            PS_LAYER_NAMES => "LayerNames",
                            PS_ADJUSTMENT_TYPES => "AdjustmentTypes",
                            PS_FILTER_NAMES => "FiltersApplied",
                            PS_ACTIVE_LAYER_COMP => "ActiveLayerComp",
                            PS_COLOR_PROFILE => "ColorProfile",
                            PS_PROOF_SETUP => "ProofSetup",
                            PS_WORKING_COLOR_SPACE => "WorkingColorSpace",
                            _ => continue,
                        };
                        tags.insert(format!("Photoshop:{}", tag_name), s);
                    }
                }

                _ => {
                    // Try to extract as i16 array
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                PS_LAYER_COUNT => ("LayerCount", val.to_string()),
                                PS_ADJUSTMENT_COUNT => ("AdjustmentCount", val.to_string()),
                                PS_FILTER_COUNT => ("FilterCount", val.to_string()),
                                PS_EDIT_COUNT => ("EditCount", val.to_string()),
                                PS_ACTION_COUNT => ("ActionCount", val.to_string()),
                                PS_SMART_OBJECT_COUNT => ("SmartObjectCount", val.to_string()),
                                PS_COLOR_MODE => ("ColorMode", decode_color_mode(val)),
                                PS_BIT_DEPTH => ("BitDepth", decode_bit_depth(val)),
                                PS_DPI_HORIZONTAL => ("HorizontalDPI", format_dpi(val)),
                                PS_DPI_VERTICAL => ("VerticalDPI", format_dpi(val)),
                                PS_WIDTH_PIXELS => ("WidthPixels", val.to_string()),
                                PS_HEIGHT_PIXELS => ("HeightPixels", val.to_string()),
                                PS_BLENDING_MODES => ("BlendingModes", decode_blending_modes(val)),
                                PS_LAYER_EFFECTS => ("LayerEffects", decode_layer_effects(val)),
                                PS_TEXT_LAYER_COUNT => ("TextLayerCount", val.to_string()),
                                PS_SHAPE_LAYER_COUNT => ("ShapeLayerCount", val.to_string()),
                                PS_ADJUSTMENT_LAYER_COUNT => {
                                    ("AdjustmentLayerCount", val.to_string())
                                }
                                PS_FILL_LAYER_COUNT => ("FillLayerCount", val.to_string()),
                                PS_MASK_COUNT => ("MaskCount", val.to_string()),
                                PS_VECTOR_MASK_COUNT => ("VectorMaskCount", val.to_string()),
                                PS_CLIPPING_MASK_COUNT => ("ClippingMaskCount", val.to_string()),
                                PS_ALPHA_CHANNEL_COUNT => ("AlphaChannelCount", val.to_string()),
                                PS_SPOT_CHANNEL_COUNT => ("SpotChannelCount", val.to_string()),
                                PS_HAS_CURVES => {
                                    ("HasCurves", if val != 0 { "Yes" } else { "No" }.to_string())
                                }
                                PS_HAS_LEVELS => {
                                    ("HasLevels", if val != 0 { "Yes" } else { "No" }.to_string())
                                }
                                PS_HAS_HUE_SAT => (
                                    "HasHueSaturation",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                PS_HAS_COLOR_BALANCE => (
                                    "HasColorBalance",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                PS_HAS_BRIGHTNESS_CONTRAST => (
                                    "HasBrightnessContrast",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                PS_HAS_VIBRANCE => (
                                    "HasVibrance",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                PS_HAS_EXPOSURE => (
                                    "HasExposure",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                PS_HAS_SHADOWS_HIGHLIGHTS => (
                                    "HasShadowsHighlights",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                PS_GAUSSIAN_BLUR_COUNT => ("GaussianBlurCount", val.to_string()),
                                PS_SHARPEN_COUNT => ("SharpenCount", val.to_string()),
                                PS_SMART_SHARPEN_COUNT => ("SmartSharpenCount", val.to_string()),
                                PS_UNSHARP_MASK_COUNT => ("UnsharpMaskCount", val.to_string()),
                                PS_NOISE_REDUCTION_COUNT => {
                                    ("NoiseReductionCount", val.to_string())
                                }
                                PS_LIQUIFY_COUNT => ("LiquifyCount", val.to_string()),
                                PS_CAMERA_RAW_COUNT => ("CameraRawFilterCount", val.to_string()),
                                PS_NEURAL_FILTER_COUNT => ("NeuralFilterCount", val.to_string()),
                                PS_LAST_SAVE_TIME => ("LastSaveTime", format_timestamp(val)),
                                PS_CREATION_TIME => ("CreationTime", format_timestamp(val)),
                                PS_TOTAL_EDIT_TIME => ("TotalEditTime", format_time_duration(val)),
                                PS_MODIFIED_FLAG => {
                                    ("Modified", if val != 0 { "Yes" } else { "No" }.to_string())
                                }
                                PS_BACKUP_COUNT => ("BackupCount", val.to_string()),
                                PS_LAYER_COMP_COUNT => ("LayerCompCount", val.to_string()),
                                PS_GUIDE_COUNT => ("GuideCount", val.to_string()),
                                PS_GRID_ENABLED => (
                                    "GridEnabled",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                PS_RULER_UNITS => ("RulerUnits", decode_ruler_units(val)),
                                _ => continue,
                            };
                            tags.insert(format!("Photoshop:{}", tag_name), formatted_value);
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
    fn test_photoshop_parser_creation() {
        let parser = PhotoshopParser::new();
        assert_eq!(parser.manufacturer_name(), "Adobe Photoshop");
        assert_eq!(parser.tag_prefix(), "Photoshop:");
    }

    #[test]
    fn test_decode_color_mode() {
        assert_eq!(decode_color_mode(3), "RGB");
        assert_eq!(decode_color_mode(4), "CMYK");
        assert_eq!(decode_color_mode(7), "Lab");
    }

    #[test]
    fn test_decode_bit_depth() {
        assert_eq!(decode_bit_depth(8), "8-bit");
        assert_eq!(decode_bit_depth(16), "16-bit");
        assert_eq!(decode_bit_depth(32), "32-bit");
    }

    #[test]
    fn test_decode_blending_modes() {
        assert_eq!(decode_blending_modes(0x01), "Normal");
        assert_eq!(decode_blending_modes(0x06), "Multiply, Screen");
    }

    #[test]
    fn test_decode_layer_effects() {
        assert_eq!(decode_layer_effects(0x01), "Drop Shadow");
        assert_eq!(decode_layer_effects(0x11), "Drop Shadow, Bevel and Emboss");
    }

    #[test]
    fn test_decode_ruler_units() {
        assert_eq!(decode_ruler_units(1), "Inches");
        assert_eq!(decode_ruler_units(5), "Pixels");
    }

    #[test]
    fn test_format_dpi() {
        assert_eq!(format_dpi(72), "72 dpi");
        assert_eq!(format_dpi(300), "300 dpi");
    }

    #[test]
    fn test_format_time_duration() {
        assert_eq!(format_time_duration(30), "30 min");
        assert_eq!(format_time_duration(90), "1 hr 30 min");
        assert_eq!(format_time_duration(120), "2 hr");
    }

    #[test]
    fn test_validate_header() {
        let parser = PhotoshopParser::new();
        let valid_header = b"Adobe Photoshop\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
