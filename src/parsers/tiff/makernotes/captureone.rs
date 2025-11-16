//! Capture One Pro MakerNote parser
//!
//! Parses Capture One Pro-specific editing metadata stored in MakerNotes.
//! Contains styles, adjustments, color grading, lens corrections, and
//! professional workflow information.
//!
//! ## Supported Versions
//! - Capture One Pro 22, 23 (current)
//! - Capture One Pro 20, 21
//! - Capture One Express
//! - Capture One for Sony/Nikon/Fujifilm
//!
//! ## Key Features
//! - Styles applied (built-in and custom)
//! - Base characteristics adjustments
//! - Color grading (shadows, midtones, highlights)
//! - Lens corrections (distortion, chromatic aberration, vignetting)
//! - Local adjustments count
//! - Exposure adjustments
//! - High Dynamic Range tools
//! - Film grain settings
//! - Sharpening and noise reduction
//! - Color balance adjustments
//! - Skin tone adjustments
//! - Clarity and structure
//! - Tethered capture information
//! - Session name and metadata
//!
//! ## Architecture
//! Capture One uses a proprietary format for storing adjustment metadata.
//! This parser extracts the most commonly needed professional workflow
//! information from the MakerNotes structure.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// Capture One MakerNote Tag IDs
const C1_VERSION: u16 = 0x0001; // Capture One version
const C1_STYLE_NAME: u16 = 0x0010; // Style name
const C1_STYLE_TYPE: u16 = 0x0011; // Style type (built-in/custom)
const C1_EXPOSURE: u16 = 0x0020; // Exposure adjustment (EV)
const C1_CONTRAST: u16 = 0x0021; // Contrast adjustment
const C1_BRIGHTNESS: u16 = 0x0022; // Brightness adjustment
const C1_SATURATION: u16 = 0x0023; // Saturation adjustment
const C1_HIGH_DYNAMIC_RANGE: u16 = 0x0024; // HDR amount
const C1_CLARITY: u16 = 0x0025; // Clarity amount
const C1_STRUCTURE: u16 = 0x0026; // Structure amount
const C1_VIBRANCE: u16 = 0x0027; // Vibrance adjustment
const C1_WHITE_BALANCE_KELVIN: u16 = 0x0030; // White balance (Kelvin)
const C1_TINT: u16 = 0x0031; // Tint adjustment
const C1_COLOR_GRADING_SHADOWS: u16 = 0x0040; // Shadow color grading
const C1_COLOR_GRADING_MIDTONES: u16 = 0x0041; // Midtone color grading
const C1_COLOR_GRADING_HIGHLIGHTS: u16 = 0x0042; // Highlight color grading
const C1_SKIN_TONE_HUE: u16 = 0x0043; // Skin tone hue adjustment
const C1_SKIN_TONE_SATURATION: u16 = 0x0044; // Skin tone saturation
const C1_SKIN_TONE_LIGHTNESS: u16 = 0x0045; // Skin tone lightness
const C1_LENS_DISTORTION: u16 = 0x0050; // Lens distortion correction
const C1_CHROMATIC_ABERRATION: u16 = 0x0051; // Chromatic aberration correction
const C1_VIGNETTING: u16 = 0x0052; // Vignetting correction
const C1_PURPLE_FRINGING: u16 = 0x0053; // Purple fringing correction
const C1_LIGHT_FALLOFF: u16 = 0x0054; // Light falloff compensation
const C1_SHARPENING_AMOUNT: u16 = 0x0060; // Sharpening amount
const C1_SHARPENING_RADIUS: u16 = 0x0061; // Sharpening radius
const C1_SHARPENING_THRESHOLD: u16 = 0x0062; // Sharpening threshold
const C1_SHARPENING_HALO: u16 = 0x0063; // Sharpening halo suppression
const C1_NOISE_REDUCTION_LUMINANCE: u16 = 0x0070; // Luminance noise reduction
const C1_NOISE_REDUCTION_COLOR: u16 = 0x0071; // Color noise reduction
const C1_NOISE_REDUCTION_DETAIL: u16 = 0x0072; // Detail preservation
const C1_FILM_GRAIN_AMOUNT: u16 = 0x0080; // Film grain amount
const C1_FILM_GRAIN_SIZE: u16 = 0x0081; // Film grain size
const C1_FILM_GRAIN_ROUGHNESS: u16 = 0x0082; // Film grain roughness
const C1_LOCAL_ADJUSTMENT_COUNT: u16 = 0x0090; // Number of local adjustments
const C1_LAYER_COUNT: u16 = 0x0091; // Number of layers
const C1_MASK_COUNT: u16 = 0x0092; // Number of masks
const C1_CURVE_ADJUSTED: u16 = 0x00A0; // Curve adjustment applied
const C1_LEVELS_ADJUSTED: u16 = 0x00A1; // Levels adjustment applied
const C1_COLOR_EDITOR_ADJUSTED: u16 = 0x00A2; // Color Editor used
const C1_BASE_CHAR_FILM: u16 = 0x00B0; // Base characteristics: Film
const C1_BASE_CHAR_GENERIC: u16 = 0x00B1; // Base characteristics: Generic
const C1_BASE_CHAR_LINEAR: u16 = 0x00B2; // Base characteristics: Linear
const C1_ICC_PROFILE: u16 = 0x00C0; // ICC profile name
const C1_COLOR_SPACE: u16 = 0x00C1; // Color space
const C1_PROOF_PROFILE: u16 = 0x00C2; // Proof profile name
const C1_SESSION_NAME: u16 = 0x00D0; // Session name
const C1_OUTPUT_RECIPE_NAME: u16 = 0x00D1; // Output recipe name
const C1_TETHERED_CAPTURE: u16 = 0x00D2; // Tethered capture flag
const C1_CAPTURE_TIME: u16 = 0x00D3; // Original capture time
const C1_RATING: u16 = 0x00E0; // Rating (0-5 stars)
const C1_COLOR_TAG: u16 = 0x00E1; // Color tag
const C1_KEYWORDS: u16 = 0x00E2; // Keywords (comma-separated)
const C1_METADATA_TOOL_VERSION: u16 = 0x00F0; // Metadata tool version

// Capture One signature
const CAPTUREONE_SIGNATURE: &[u8] = b"CaptureOne";

/// Decodes style type
///
/// # Arguments
/// * `value` - Style type code
///
/// # Returns
/// Human-readable style type
fn decode_style_type(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Built-in".to_string(),
        2 => "User".to_string(),
        3 => "Custom".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes base characteristics
///
/// # Arguments
/// * `value` - Base characteristics code
///
/// # Returns
/// Human-readable base characteristics
fn decode_base_char(value: i16) -> String {
    match value {
        0 => "Film Standard".to_string(),
        1 => "Film Extra Shadow".to_string(),
        2 => "Film High Contrast".to_string(),
        3 => "Generic".to_string(),
        4 => "Linear Scientific".to_string(),
        5 => "Auto".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes color space
///
/// # Arguments
/// * `value` - Color space code
///
/// # Returns
/// Human-readable color space name
fn decode_color_space(value: i16) -> String {
    match value {
        0 => "sRGB".to_string(),
        1 => "Adobe RGB".to_string(),
        2 => "ProPhoto RGB".to_string(),
        3 => "Wide Gamut RGB".to_string(),
        4 => "Display P3".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes color tag
///
/// # Arguments
/// * `value` - Color tag code
///
/// # Returns
/// Human-readable color tag
fn decode_color_tag(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Red".to_string(),
        2 => "Orange".to_string(),
        3 => "Yellow".to_string(),
        4 => "Green".to_string(),
        5 => "Blue".to_string(),
        6 => "Purple".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Formats exposure value
///
/// # Arguments
/// * `value` - Exposure in tenths of EV
///
/// # Returns
/// Formatted exposure string
fn format_exposure(value: i16) -> String {
    let ev = value as f64 / 10.0;
    if ev >= 0.0 {
        format!("+{:.1} EV", ev)
    } else {
        format!("{:.1} EV", ev)
    }
}

/// Formats percentage adjustment
///
/// # Arguments
/// * `value` - Adjustment value (-100 to +100)
///
/// # Returns
/// Formatted percentage string
fn format_percentage(value: i16) -> String {
    if value >= 0 {
        format!("+{}", value)
    } else {
        format!("{}", value)
    }
}

/// Formats white balance Kelvin
///
/// # Arguments
/// * `value` - Temperature in Kelvin
///
/// # Returns
/// Formatted Kelvin string
fn format_kelvin(value: i16) -> String {
    if value <= 0 {
        return "Auto".to_string();
    }
    format!("{} K", value * 10)
}

/// Formats tint adjustment
///
/// # Arguments
/// * `value` - Tint value
///
/// # Returns
/// Formatted tint string
fn format_tint(value: i16) -> String {
    if value >= 0 {
        format!("+{}", value)
    } else {
        format!("{}", value)
    }
}

/// Formats sharpening radius
///
/// # Arguments
/// * `value` - Radius value
///
/// # Returns
/// Formatted radius string
fn format_radius(value: i16) -> String {
    let radius = value as f64 / 10.0;
    format!("{:.1}", radius)
}

/// Formats film grain size
///
/// # Arguments
/// * `value` - Grain size code
///
/// # Returns
/// Human-readable grain size
fn format_grain_size(value: i16) -> String {
    match value {
        0 => "Fine".to_string(),
        1 => "Medium".to_string(),
        2 => "Coarse".to_string(),
        _ => format!("{}", value),
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
    if !(0..=5).contains(&value) {
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

/// Capture One MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct CaptureOneParser;

impl CaptureOneParser {
    /// Creates a new Capture One parser instance
    pub fn new() -> Self {
        CaptureOneParser
    }
}

impl MakerNoteParser for CaptureOneParser {
    fn manufacturer_name(&self) -> &'static str {
        "Capture One"
    }

    fn tag_prefix(&self) -> &'static str {
        "CaptureOne:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 10 {
            return false;
        }
        data.starts_with(CAPTUREONE_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Capture One MakerNote data too short".to_string());
        }

        // Skip Capture One signature if present
        let start_offset = if data.starts_with(CAPTUREONE_SIGNATURE) {
            10
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

        if num_entries == 0 || num_entries > 300 {
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
                C1_VERSION
                | C1_STYLE_NAME
                | C1_ICC_PROFILE
                | C1_PROOF_PROFILE
                | C1_SESSION_NAME
                | C1_OUTPUT_RECIPE_NAME
                | C1_KEYWORDS => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            C1_VERSION => "Version",
                            C1_STYLE_NAME => "StyleName",
                            C1_ICC_PROFILE => "ICCProfile",
                            C1_PROOF_PROFILE => "ProofProfile",
                            C1_SESSION_NAME => "SessionName",
                            C1_OUTPUT_RECIPE_NAME => "OutputRecipe",
                            C1_KEYWORDS => "Keywords",
                            _ => continue,
                        };
                        tags.insert(format!("CaptureOne:{}", tag_name), s);
                    }
                }

                _ => {
                    // Try to extract as i16 array
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                C1_STYLE_TYPE => ("StyleType", decode_style_type(val)),
                                C1_EXPOSURE => ("Exposure", format_exposure(val)),
                                C1_CONTRAST => ("Contrast", format_percentage(val)),
                                C1_BRIGHTNESS => ("Brightness", format_percentage(val)),
                                C1_SATURATION => ("Saturation", format_percentage(val)),
                                C1_HIGH_DYNAMIC_RANGE => ("HDR", format_percentage(val)),
                                C1_CLARITY => ("Clarity", format_percentage(val)),
                                C1_STRUCTURE => ("Structure", format_percentage(val)),
                                C1_VIBRANCE => ("Vibrance", format_percentage(val)),
                                C1_WHITE_BALANCE_KELVIN => {
                                    ("WhiteBalanceKelvin", format_kelvin(val))
                                }
                                C1_TINT => ("Tint", format_tint(val)),
                                C1_COLOR_GRADING_SHADOWS => {
                                    ("ColorGradingShadows", format_percentage(val))
                                }
                                C1_COLOR_GRADING_MIDTONES => {
                                    ("ColorGradingMidtones", format_percentage(val))
                                }
                                C1_COLOR_GRADING_HIGHLIGHTS => {
                                    ("ColorGradingHighlights", format_percentage(val))
                                }
                                C1_SKIN_TONE_HUE => ("SkinToneHue", format_percentage(val)),
                                C1_SKIN_TONE_SATURATION => {
                                    ("SkinToneSaturation", format_percentage(val))
                                }
                                C1_SKIN_TONE_LIGHTNESS => {
                                    ("SkinToneLightness", format_percentage(val))
                                }
                                C1_LENS_DISTORTION => ("LensDistortion", format_percentage(val)),
                                C1_CHROMATIC_ABERRATION => {
                                    ("ChromaticAberration", format_percentage(val))
                                }
                                C1_VIGNETTING => ("Vignetting", format_percentage(val)),
                                C1_PURPLE_FRINGING => ("PurpleFringing", format_percentage(val)),
                                C1_LIGHT_FALLOFF => ("LightFalloff", format_percentage(val)),
                                C1_SHARPENING_AMOUNT => {
                                    ("SharpeningAmount", format_percentage(val))
                                }
                                C1_SHARPENING_RADIUS => ("SharpeningRadius", format_radius(val)),
                                C1_SHARPENING_THRESHOLD => ("SharpeningThreshold", val.to_string()),
                                C1_SHARPENING_HALO => ("SharpeningHalo", format_percentage(val)),
                                C1_NOISE_REDUCTION_LUMINANCE => {
                                    ("NoiseReductionLuminance", format_percentage(val))
                                }
                                C1_NOISE_REDUCTION_COLOR => {
                                    ("NoiseReductionColor", format_percentage(val))
                                }
                                C1_NOISE_REDUCTION_DETAIL => {
                                    ("NoiseReductionDetail", format_percentage(val))
                                }
                                C1_FILM_GRAIN_AMOUNT => ("FilmGrainAmount", format_percentage(val)),
                                C1_FILM_GRAIN_SIZE => ("FilmGrainSize", format_grain_size(val)),
                                C1_FILM_GRAIN_ROUGHNESS => {
                                    ("FilmGrainRoughness", format_percentage(val))
                                }
                                C1_LOCAL_ADJUSTMENT_COUNT => {
                                    ("LocalAdjustmentCount", val.to_string())
                                }
                                C1_LAYER_COUNT => ("LayerCount", val.to_string()),
                                C1_MASK_COUNT => ("MaskCount", val.to_string()),
                                C1_CURVE_ADJUSTED => (
                                    "CurveAdjusted",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                C1_LEVELS_ADJUSTED => (
                                    "LevelsAdjusted",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                C1_COLOR_EDITOR_ADJUSTED => (
                                    "ColorEditorAdjusted",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                C1_BASE_CHAR_FILM => {
                                    ("BaseCharacteristicsFilm", decode_base_char(val))
                                }
                                C1_BASE_CHAR_GENERIC => {
                                    ("BaseCharacteristicsGeneric", decode_base_char(val))
                                }
                                C1_BASE_CHAR_LINEAR => {
                                    ("BaseCharacteristicsLinear", decode_base_char(val))
                                }
                                C1_COLOR_SPACE => ("ColorSpace", decode_color_space(val)),
                                C1_TETHERED_CAPTURE => (
                                    "TetheredCapture",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                C1_RATING => ("Rating", format_rating(val)),
                                C1_COLOR_TAG => ("ColorTag", decode_color_tag(val)),
                                _ => continue,
                            };
                            tags.insert(format!("CaptureOne:{}", tag_name), formatted_value);
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
    fn test_captureone_parser_creation() {
        let parser = CaptureOneParser::new();
        assert_eq!(parser.manufacturer_name(), "Capture One");
        assert_eq!(parser.tag_prefix(), "CaptureOne:");
    }

    #[test]
    fn test_decode_style_type() {
        assert_eq!(decode_style_type(1), "Built-in");
        assert_eq!(decode_style_type(2), "User");
    }

    #[test]
    fn test_decode_color_space() {
        assert_eq!(decode_color_space(0), "sRGB");
        assert_eq!(decode_color_space(2), "ProPhoto RGB");
    }

    #[test]
    fn test_format_exposure() {
        assert_eq!(format_exposure(15), "+1.5 EV");
        assert_eq!(format_exposure(-10), "-1.0 EV");
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(format_percentage(25), "+25");
        assert_eq!(format_percentage(-50), "-50");
    }

    #[test]
    fn test_format_kelvin() {
        assert_eq!(format_kelvin(550), "5500 K");
        assert_eq!(format_kelvin(650), "6500 K");
    }

    #[test]
    fn test_format_rating() {
        assert_eq!(format_rating(0), "None");
        assert_eq!(format_rating(3), "3 stars");
        assert_eq!(format_rating(5), "5 stars");
    }

    #[test]
    fn test_decode_color_tag() {
        assert_eq!(decode_color_tag(1), "Red");
        assert_eq!(decode_color_tag(4), "Green");
    }
}
