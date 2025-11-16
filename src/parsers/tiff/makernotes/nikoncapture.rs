//! Nikon Capture NX-D/ViewNX-i MakerNote parser
//!
//! Parses Nikon Capture NX-D and ViewNX-i editing metadata.
//! Contains Picture Control settings, Active D-Lighting adjustments,
//! Vignette Control, color adjustments, and Nikon-specific processing.
//!
//! ## Supported Applications
//! - Nikon Capture NX-D (current)
//! - ViewNX-i
//! - Nikon Capture NX2 (legacy)
//! - Nikon ViewNX 2 (legacy)
//!
//! ## Key Features
//! - Picture Control settings (Standard, Neutral, Vivid, etc.)
//! - Active D-Lighting amount
//! - Vignette Control
//! - Color Booster
//! - Color Control Points
//! - Filter effects
//! - Noise Reduction
//! - Unsharp Mask settings
//! - Straighten adjustments
//! - Retouch tools used
//! - RAW processing settings
//! - White balance fine-tuning
//! - Exposure compensation
//!
//! ## Architecture
//! Nikon Capture stores editing metadata in a proprietary format
//! specific to Nikon cameras. This parser focuses on the most
//! commonly used professional features.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// Nikon Capture MakerNote Tag IDs
const NC_VERSION: u16 = 0x0001; // Nikon Capture version
const NC_PICTURE_CONTROL_NAME: u16 = 0x0010; // Picture Control name
const NC_PICTURE_CONTROL_BASE: u16 = 0x0011; // Picture Control base
const NC_PICTURE_CONTROL_ADJUST: u16 = 0x0012; // Quick adjust
const NC_SHARPENING: u16 = 0x0013; // Sharpening level
const NC_CONTRAST: u16 = 0x0014; // Contrast level
const NC_BRIGHTNESS: u16 = 0x0015; // Brightness level
const NC_SATURATION: u16 = 0x0016; // Saturation level
const NC_HUE_ADJUSTMENT: u16 = 0x0017; // Hue adjustment
const NC_FILTER_EFFECT: u16 = 0x0018; // Filter effect
const NC_TONING_EFFECT: u16 = 0x0019; // Toning effect
const NC_TONING_SATURATION: u16 = 0x001A; // Toning saturation
const NC_ACTIVE_D_LIGHTING: u16 = 0x0020; // Active D-Lighting
const NC_VIGNETTE_CONTROL: u16 = 0x0021; // Vignette Control
const NC_AUTO_DISTORTION: u16 = 0x0022; // Auto Distortion Control
const NC_LATERAL_CHROMATIC: u16 = 0x0023; // Lateral Chromatic Aberration
const NC_AXIAL_CHROMATIC: u16 = 0x0024; // Axial Chromatic Aberration
const NC_COLOR_BOOSTER: u16 = 0x0030; // Color Booster
const NC_COLOR_BOOSTER_TYPE: u16 = 0x0031; // Color Booster type
const NC_COLOR_BOOSTER_LEVEL: u16 = 0x0032; // Color Booster level
const NC_COLOR_CONTROL_POINTS: u16 = 0x0033; // Color Control Points count
const NC_NOISE_REDUCTION: u16 = 0x0040; // Noise Reduction
const NC_NOISE_REDUCTION_EDGE: u16 = 0x0041; // Edge Noise Reduction
const NC_NOISE_REDUCTION_COLOR: u16 = 0x0042; // Color Noise Reduction
const NC_UNSHARP_MASK: u16 = 0x0050; // Unsharp Mask applied
const NC_UNSHARP_AMOUNT: u16 = 0x0051; // Unsharp amount
const NC_UNSHARP_RADIUS: u16 = 0x0052; // Unsharp radius
const NC_UNSHARP_THRESHOLD: u16 = 0x0053; // Unsharp threshold
const NC_STRAIGHTEN: u16 = 0x0060; // Straighten angle
const NC_CROP_MODE: u16 = 0x0061; // Crop mode
const NC_RETOUCH_HISTORY: u16 = 0x0070; // Retouch history count
const NC_RED_EYE_CORRECTION: u16 = 0x0071; // Red-eye correction count
const NC_DUST_REMOVAL: u16 = 0x0072; // Image Dust Off
const NC_WHITE_BALANCE_MODE: u16 = 0x0080; // White balance mode
const NC_WHITE_BALANCE_FINE: u16 = 0x0081; // WB fine-tuning
const NC_EXPOSURE_COMP: u16 = 0x0082; // Exposure compensation
const NC_HIGH_ISO_NR: u16 = 0x0090; // High ISO Noise Reduction
const NC_LONG_EXPOSURE_NR: u16 = 0x0091; // Long Exposure NR
const NC_RATING: u16 = 0x00A0; // Rating
const NC_LABEL: u16 = 0x00A1; // Label color
const NC_EDIT_STATUS: u16 = 0x00B0; // Edit status

// Nikon Capture signature
const NIKON_CAPTURE_SIGNATURE: &[u8] = b"NikonNX";

/// Decodes Picture Control name
///
/// # Arguments
/// * `value` - Picture Control code
///
/// # Returns
/// Human-readable Picture Control name
fn decode_picture_control(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Standard".to_string(),
        2 => "Neutral".to_string(),
        3 => "Vivid".to_string(),
        4 => "Monochrome".to_string(),
        5 => "Portrait".to_string(),
        6 => "Landscape".to_string(),
        7 => "Flat".to_string(),
        8 => "Creative".to_string(),
        100 => "Custom".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Active D-Lighting level
///
/// # Arguments
/// * `value` - Active D-Lighting code
///
/// # Returns
/// Human-readable level
fn decode_active_d_lighting(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Low".to_string(),
        2 => "Normal".to_string(),
        3 => "High".to_string(),
        4 => "Extra High".to_string(),
        5 => "Auto".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Vignette Control level
///
/// # Arguments
/// * `value` - Vignette Control code
///
/// # Returns
/// Human-readable level
fn decode_vignette_control(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Low".to_string(),
        2 => "Normal".to_string(),
        3 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes filter effect
///
/// # Arguments
/// * `value` - Filter effect code
///
/// # Returns
/// Human-readable filter name
fn decode_filter_effect(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Yellow".to_string(),
        2 => "Orange".to_string(),
        3 => "Red".to_string(),
        4 => "Green".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes toning effect
///
/// # Arguments
/// * `value` - Toning effect code
///
/// # Returns
/// Human-readable toning name
fn decode_toning_effect(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Blue".to_string(),
        2 => "Red".to_string(),
        3 => "Yellow".to_string(),
        4 => "Green".to_string(),
        5 => "Blue-Green".to_string(),
        6 => "Blue-Purple".to_string(),
        7 => "Red-Purple".to_string(),
        8 => "Sepia".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes noise reduction level
///
/// # Arguments
/// * `value` - Noise reduction code
///
/// # Returns
/// Human-readable level
fn decode_noise_reduction(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Low".to_string(),
        2 => "Medium".to_string(),
        3 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes white balance mode
///
/// # Arguments
/// * `value` - White balance code
///
/// # Returns
/// Human-readable mode
fn decode_white_balance(value: i16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Daylight".to_string(),
        2 => "Cloudy".to_string(),
        3 => "Shade".to_string(),
        4 => "Tungsten".to_string(),
        5 => "Fluorescent".to_string(),
        6 => "Flash".to_string(),
        7 => "Custom".to_string(),
        8 => "Preset".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes label color
///
/// # Arguments
/// * `value` - Label code
///
/// # Returns
/// Human-readable label
fn decode_label(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Red".to_string(),
        2 => "Yellow".to_string(),
        3 => "Green".to_string(),
        4 => "Blue".to_string(),
        5 => "Purple".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Formats adjustment level (-20 to +20)
///
/// # Arguments
/// * `value` - Adjustment value
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

/// Formats exposure compensation
///
/// # Arguments
/// * `value` - Exposure in thirds of EV
///
/// # Returns
/// Formatted EV string
fn format_exposure_comp(value: i16) -> String {
    let ev = value as f64 / 3.0;
    if ev >= 0.0 {
        format!("+{:.1} EV", ev)
    } else {
        format!("{:.1} EV", ev)
    }
}

/// Formats straighten angle
///
/// # Arguments
/// * `value` - Angle in tenths of degree
///
/// # Returns
/// Formatted angle string
fn format_straighten(value: i16) -> String {
    let angle = value as f64 / 10.0;
    if angle.abs() < 0.1 {
        return "0°".to_string();
    }
    if angle >= 0.0 {
        format!("+{:.1}°", angle)
    } else {
        format!("{:.1}°", angle)
    }
}

/// Formats rating
///
/// # Arguments
/// * `value` - Rating (0-5)
///
/// # Returns
/// Formatted rating string
fn format_rating(value: i16) -> String {
    if value < 0 || value > 5 {
        return "None".to_string();
    }
    if value == 0 {
        "None".to_string()
    } else {
        format!("{} stars", value)
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

/// Nikon Capture MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct NikonCaptureParser;

impl NikonCaptureParser {
    /// Creates a new Nikon Capture parser instance
    pub fn new() -> Self {
        NikonCaptureParser
    }
}

impl MakerNoteParser for NikonCaptureParser {
    fn manufacturer_name(&self) -> &'static str {
        "Nikon Capture"
    }

    fn tag_prefix(&self) -> &'static str {
        "NikonCapture:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 7 {
            return false;
        }
        data.starts_with(NIKON_CAPTURE_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Nikon Capture MakerNote data too short".to_string());
        }

        // Skip Nikon Capture signature if present
        let start_offset = if data.starts_with(NIKON_CAPTURE_SIGNATURE) {
            7
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
                NC_VERSION | NC_PICTURE_CONTROL_NAME | NC_PICTURE_CONTROL_BASE => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            NC_VERSION => "Version",
                            NC_PICTURE_CONTROL_NAME => "PictureControlName",
                            NC_PICTURE_CONTROL_BASE => "PictureControlBase",
                            _ => continue,
                        };
                        tags.insert(format!("NikonCapture:{}", tag_name), s);
                    }
                }

                _ => {
                    // Try to extract as i16 array
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                NC_PICTURE_CONTROL_ADJUST => {
                                    ("PictureControlAdjust", decode_picture_control(val))
                                }
                                NC_SHARPENING => ("Sharpening", format_adjustment(val)),
                                NC_CONTRAST => ("Contrast", format_adjustment(val)),
                                NC_BRIGHTNESS => ("Brightness", format_adjustment(val)),
                                NC_SATURATION => ("Saturation", format_adjustment(val)),
                                NC_HUE_ADJUSTMENT => ("HueAdjustment", format_adjustment(val)),
                                NC_FILTER_EFFECT => ("FilterEffect", decode_filter_effect(val)),
                                NC_TONING_EFFECT => ("ToningEffect", decode_toning_effect(val)),
                                NC_TONING_SATURATION => {
                                    ("ToningSaturation", format_adjustment(val))
                                }
                                NC_ACTIVE_D_LIGHTING => {
                                    ("ActiveDLighting", decode_active_d_lighting(val))
                                }
                                NC_VIGNETTE_CONTROL => {
                                    ("VignetteControl", decode_vignette_control(val))
                                }
                                NC_AUTO_DISTORTION => (
                                    "AutoDistortion",
                                    if val != 0 { "On" } else { "Off" }.to_string(),
                                ),
                                NC_LATERAL_CHROMATIC => (
                                    "LateralChromaticAberration",
                                    if val != 0 { "On" } else { "Off" }.to_string(),
                                ),
                                NC_AXIAL_CHROMATIC => (
                                    "AxialChromaticAberration",
                                    if val != 0 { "On" } else { "Off" }.to_string(),
                                ),
                                NC_COLOR_BOOSTER => (
                                    "ColorBooster",
                                    if val != 0 { "On" } else { "Off" }.to_string(),
                                ),
                                NC_COLOR_BOOSTER_LEVEL => {
                                    ("ColorBoosterLevel", format_adjustment(val))
                                }
                                NC_COLOR_CONTROL_POINTS => ("ColorControlPoints", val.to_string()),
                                NC_NOISE_REDUCTION => {
                                    ("NoiseReduction", decode_noise_reduction(val))
                                }
                                NC_NOISE_REDUCTION_EDGE => {
                                    ("EdgeNoiseReduction", decode_noise_reduction(val))
                                }
                                NC_NOISE_REDUCTION_COLOR => {
                                    ("ColorNoiseReduction", decode_noise_reduction(val))
                                }
                                NC_UNSHARP_MASK => (
                                    "UnsharpMask",
                                    if val != 0 { "On" } else { "Off" }.to_string(),
                                ),
                                NC_UNSHARP_AMOUNT => ("UnsharpAmount", format_adjustment(val)),
                                NC_UNSHARP_RADIUS => ("UnsharpRadius", val.to_string()),
                                NC_UNSHARP_THRESHOLD => ("UnsharpThreshold", val.to_string()),
                                NC_STRAIGHTEN => ("Straighten", format_straighten(val)),
                                NC_RETOUCH_HISTORY => ("RetouchHistoryCount", val.to_string()),
                                NC_RED_EYE_CORRECTION => ("RedEyeCorrection", val.to_string()),
                                NC_DUST_REMOVAL => (
                                    "ImageDustOff",
                                    if val != 0 { "On" } else { "Off" }.to_string(),
                                ),
                                NC_WHITE_BALANCE_MODE => {
                                    ("WhiteBalanceMode", decode_white_balance(val))
                                }
                                NC_WHITE_BALANCE_FINE => {
                                    ("WhiteBalanceFine", format_adjustment(val))
                                }
                                NC_EXPOSURE_COMP => ("ExposureComp", format_exposure_comp(val)),
                                NC_HIGH_ISO_NR => ("HighISONR", decode_noise_reduction(val)),
                                NC_LONG_EXPOSURE_NR => (
                                    "LongExposureNR",
                                    if val != 0 { "On" } else { "Off" }.to_string(),
                                ),
                                NC_RATING => ("Rating", format_rating(val)),
                                NC_LABEL => ("Label", decode_label(val)),
                                NC_EDIT_STATUS => (
                                    "EditStatus",
                                    if val != 0 { "Edited" } else { "Original" }.to_string(),
                                ),
                                _ => continue,
                            };
                            tags.insert(format!("NikonCapture:{}", tag_name), formatted_value);
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
    fn test_nikon_capture_parser_creation() {
        let parser = NikonCaptureParser::new();
        assert_eq!(parser.manufacturer_name(), "Nikon Capture");
        assert_eq!(parser.tag_prefix(), "NikonCapture:");
    }

    #[test]
    fn test_decode_picture_control() {
        assert_eq!(decode_picture_control(1), "Standard");
        assert_eq!(decode_picture_control(3), "Vivid");
        assert_eq!(decode_picture_control(4), "Monochrome");
    }

    #[test]
    fn test_decode_active_d_lighting() {
        assert_eq!(decode_active_d_lighting(0), "Off");
        assert_eq!(decode_active_d_lighting(3), "High");
        assert_eq!(decode_active_d_lighting(5), "Auto");
    }

    #[test]
    fn test_decode_vignette_control() {
        assert_eq!(decode_vignette_control(0), "Off");
        assert_eq!(decode_vignette_control(2), "Normal");
    }

    #[test]
    fn test_format_adjustment() {
        assert_eq!(format_adjustment(10), "+10");
        assert_eq!(format_adjustment(-5), "-5");
    }

    #[test]
    fn test_format_exposure_comp() {
        assert_eq!(format_exposure_comp(3), "+1.0 EV");
        assert_eq!(format_exposure_comp(-6), "-2.0 EV");
    }

    #[test]
    fn test_format_straighten() {
        assert_eq!(format_straighten(15), "+1.5°");
        assert_eq!(format_straighten(-25), "-2.5°");
        assert_eq!(format_straighten(0), "0°");
    }

    #[test]
    fn test_decode_filter_effect() {
        assert_eq!(decode_filter_effect(1), "Yellow");
        assert_eq!(decode_filter_effect(3), "Red");
    }
}
