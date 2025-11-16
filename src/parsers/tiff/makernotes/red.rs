//! RED Cinema Camera MakerNote parser
//!
//! Parses RED-specific EXIF MakerNote tags from professional cinema cameras.
//! RED Digital Cinema manufactures high-end cameras for film and television
//! production, known for RAW workflow and modular design.
//!
//! ## Supported Models
//! - RED KOMODO (6K)
//! - RED V-RAPTOR (8K)
//! - RED MONSTRO (8K VV)
//! - RED HELIUM (8K S35)
//! - RED GEMINI (5K S35)
//! - RED DRAGON (6K)
//! - RED SCARLET-W (5K)
//!
//! ## Key Features
//! - REDCODE compression level
//! - Sensor resolution and crop mode
//! - Frame rate and shutter angle
//! - ISO and color temperature
//! - Lens metadata (focal length, T-stop, focus distance)
//! - Timecode and reel information
//! - Color science version
//! - HDRx mode
//! - Look/LUT applied
//!
//! ## Architecture
//! RED stores extensive metadata in R3D files, but still images
//! contain key camera and lens settings in MakerNotes.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// RED MakerNote Tag IDs
const RED_MODEL: u16 = 0x0001; // Camera model (KOMODO, V-RAPTOR, etc.)
const RED_SERIAL: u16 = 0x0002; // Serial number
const RED_FIRMWARE: u16 = 0x0003; // Firmware version
const RED_SENSOR: u16 = 0x0100; // Sensor type
const RED_RESOLUTION: u16 = 0x0101; // Resolution mode
const RED_REDCODE: u16 = 0x0102; // REDCODE compression ratio
const RED_FRAME_RATE: u16 = 0x0103; // Frame rate (fps)
const RED_SHUTTER_ANGLE: u16 = 0x0104; // Shutter angle (degrees)
const RED_ISO: u16 = 0x0105; // ISO setting
const RED_COLOR_TEMP: u16 = 0x0106; // Color temperature (Kelvin)
const RED_TINT: u16 = 0x0107; // Tint adjustment
const RED_EXPOSURE: u16 = 0x0108; // Exposure compensation (stops)
const RED_GAMMA_CURVE: u16 = 0x0109; // Gamma curve (Log3G10, etc.)
const RED_COLOR_SPACE: u16 = 0x010A; // Color space (REDWideGamutRGB, etc.)
const RED_LENS_TYPE: u16 = 0x010B; // Lens mount type
const RED_FOCAL_LENGTH: u16 = 0x010C; // Focal length (mm)
const RED_FOCUS_DISTANCE: u16 = 0x010D; // Focus distance (feet/meters)
const RED_APERTURE: u16 = 0x010E; // T-stop value
const RED_TIMECODE: u16 = 0x010F; // Timecode
const RED_REEL_NUMBER: u16 = 0x0110; // Reel number
const RED_CLIP_NAME: u16 = 0x0111; // Clip name
const RED_HDRX: u16 = 0x0112; // HDRx mode
const RED_LOOK: u16 = 0x0113; // Look/LUT name
const RED_COLOR_SCIENCE: u16 = 0x0114; // Color science version
const RED_CROP_MODE: u16 = 0x0115; // Sensor crop mode
const RED_PROJECT_FPS: u16 = 0x0116; // Project frame rate
const RED_KELVIN_OVERRIDE: u16 = 0x0117; // Kelvin override
const RED_SHADOW: u16 = 0x0118; // Shadow adjustment
const RED_HIGHLIGHT: u16 = 0x0119; // Highlight adjustment
const RED_SATURATION: u16 = 0x011A; // Saturation
const RED_CONTRAST: u16 = 0x011B; // Contrast
const RED_SHARPNESS: u16 = 0x011C; // Sharpness
const RED_NOISE_REDUCTION: u16 = 0x011D; // Noise reduction level

const RED_SIGNATURE: &[u8] = b"RED";

/// Decodes REDCODE compression ratio
fn decode_redcode(value: i16) -> String {
    match value {
        2 => "2:1".to_string(),
        3 => "3:1".to_string(),
        4 => "4:1".to_string(),
        5 => "5:1".to_string(),
        6 => "6:1".to_string(),
        7 => "7:1".to_string(),
        8 => "8:1".to_string(),
        9 => "9:1".to_string(),
        10 => "10:1".to_string(),
        12 => "12:1".to_string(),
        16 => "16:1".to_string(),
        22 => "22:1".to_string(),
        _ => format!("{}:1", value),
    }
}

/// Decodes sensor resolution mode
fn decode_resolution(value: i16) -> String {
    match value {
        0 => "Full".to_string(),
        1 => "6K".to_string(),
        2 => "5K".to_string(),
        3 => "4K".to_string(),
        4 => "3K".to_string(),
        5 => "2K".to_string(),
        6 => "8K".to_string(),
        7 => "8K 2.4:1".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes gamma curve
fn decode_gamma(value: i16) -> String {
    match value {
        0 => "REDLog3G10".to_string(),
        1 => "REDLogFilm".to_string(),
        2 => "Rec709".to_string(),
        3 => "REDgamma".to_string(),
        4 => "REDgamma2".to_string(),
        5 => "REDgamma3".to_string(),
        6 => "REDgamma4".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes color space
fn decode_color_space(value: i16) -> String {
    match value {
        0 => "REDWideGamutRGB".to_string(),
        1 => "Rec709".to_string(),
        2 => "DCI-P3".to_string(),
        3 => "Rec2020".to_string(),
        4 => "REDcolor".to_string(),
        5 => "REDcolor2".to_string(),
        6 => "REDcolor3".to_string(),
        7 => "REDcolor4".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes lens mount type
fn decode_lens_type(value: i16) -> String {
    match value {
        0 => "Canon EF".to_string(),
        1 => "PL Mount".to_string(),
        2 => "Nikon F".to_string(),
        3 => "Leica M".to_string(),
        4 => "RED DSMC".to_string(),
        5 => "Canon RF".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes sensor crop mode
fn decode_crop_mode(value: i16) -> String {
    match value {
        0 => "Full Frame".to_string(),
        1 => "2:1".to_string(),
        2 => "2.4:1".to_string(),
        3 => "16:9".to_string(),
        4 => "4:3".to_string(),
        5 => "6:5".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Formats frame rate
fn format_frame_rate(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} fps", value)
}

/// Formats shutter angle
fn format_shutter_angle(value: i16) -> String {
    let angle = value as f64 / 10.0;
    format!("{:.1}°", angle)
}

/// Formats color temperature
fn format_color_temp(value: i16) -> String {
    format!("{} K", value)
}

/// Formats tint
fn format_tint(value: i16) -> String {
    if value >= 0 {
        format!("+{}", value)
    } else {
        format!("{}", value)
    }
}

/// Formats exposure
fn format_exposure(value: i16) -> String {
    let stops = value as f64 / 100.0;
    if stops >= 0.0 {
        format!("+{:.2} stops", stops)
    } else {
        format!("{:.2} stops", stops)
    }
}

/// Formats focal length
fn format_focal_length(value: i16) -> String {
    format!("{} mm", value)
}

/// Formats focus distance
fn format_focus_distance(value: i16) -> String {
    if value == 0 {
        return "Infinity".to_string();
    }
    let feet = value as f64 / 10.0;
    format!("{:.1} ft", feet)
}

/// Formats T-stop
fn format_aperture(value: i16) -> String {
    let t_stop = value as f64 / 10.0;
    format!("T{:.1}", t_stop)
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

    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// RED Cinema Camera MakerNote parser
/// Default implementation for parser
#[derive(Default)]
pub struct RedParser;

impl RedParser {
    /// Creates a new RED parser instance
    pub fn new() -> Self {
        RedParser
    }
}

impl MakerNoteParser for RedParser {
    fn manufacturer_name(&self) -> &'static str {
        "RED"
    }

    fn tag_prefix(&self) -> &'static str {
        "RED:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 3 {
            return false;
        }
        data.starts_with(RED_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("RED MakerNote data too short".to_string());
        }

        let start_offset = if data.starts_with(RED_SIGNATURE) {
            3
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

            match tag {
                RED_MODEL | RED_SERIAL | RED_FIRMWARE | RED_SENSOR | RED_TIMECODE
                | RED_REEL_NUMBER | RED_CLIP_NAME | RED_LOOK | RED_COLOR_SCIENCE => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            RED_MODEL => "Model",
                            RED_SERIAL => "SerialNumber",
                            RED_FIRMWARE => "FirmwareVersion",
                            RED_SENSOR => "Sensor",
                            RED_TIMECODE => "Timecode",
                            RED_REEL_NUMBER => "ReelNumber",
                            RED_CLIP_NAME => "ClipName",
                            RED_LOOK => "Look",
                            RED_COLOR_SCIENCE => "ColorScience",
                            _ => continue,
                        };
                        tags.insert(format!("RED:{}", tag_name), s);
                    }
                }
                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                RED_RESOLUTION => ("Resolution", decode_resolution(val)),
                                RED_REDCODE => ("REDCODE", decode_redcode(val)),
                                RED_FRAME_RATE => ("FrameRate", format_frame_rate(val)),
                                RED_SHUTTER_ANGLE => ("ShutterAngle", format_shutter_angle(val)),
                                RED_ISO => ("ISO", val.to_string()),
                                RED_COLOR_TEMP => ("ColorTemperature", format_color_temp(val)),
                                RED_TINT => ("Tint", format_tint(val)),
                                RED_EXPOSURE => ("ExposureCompensation", format_exposure(val)),
                                RED_GAMMA_CURVE => ("GammaCurve", decode_gamma(val)),
                                RED_COLOR_SPACE => ("ColorSpace", decode_color_space(val)),
                                RED_LENS_TYPE => ("LensMount", decode_lens_type(val)),
                                RED_FOCAL_LENGTH => ("FocalLength", format_focal_length(val)),
                                RED_FOCUS_DISTANCE => ("FocusDistance", format_focus_distance(val)),
                                RED_APERTURE => ("Aperture", format_aperture(val)),
                                RED_HDRX => (
                                    "HDRx",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                RED_CROP_MODE => ("CropMode", decode_crop_mode(val)),
                                RED_PROJECT_FPS => ("ProjectFPS", format_frame_rate(val)),
                                RED_KELVIN_OVERRIDE => (
                                    "KelvinOverride",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                RED_SHADOW => ("Shadow", val.to_string()),
                                RED_HIGHLIGHT => ("Highlight", val.to_string()),
                                RED_SATURATION => ("Saturation", val.to_string()),
                                RED_CONTRAST => ("Contrast", val.to_string()),
                                RED_SHARPNESS => ("Sharpness", val.to_string()),
                                RED_NOISE_REDUCTION => ("NoiseReduction", val.to_string()),
                                _ => continue,
                            };
                            tags.insert(format!("RED:{}", tag_name), formatted_value);
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
    fn test_red_parser_creation() {
        let parser = RedParser::new();
        assert_eq!(parser.manufacturer_name(), "RED");
        assert_eq!(parser.tag_prefix(), "RED:");
    }

    #[test]
    fn test_decode_redcode() {
        assert_eq!(decode_redcode(5), "5:1");
        assert_eq!(decode_redcode(12), "12:1");
    }

    #[test]
    fn test_decode_resolution() {
        assert_eq!(decode_resolution(6), "8K");
        assert_eq!(decode_resolution(1), "6K");
    }

    #[test]
    fn test_decode_gamma() {
        assert_eq!(decode_gamma(0), "REDLog3G10");
        assert_eq!(decode_gamma(2), "Rec709");
    }

    #[test]
    fn test_format_shutter_angle() {
        assert_eq!(format_shutter_angle(1800), "180.0°");
        assert_eq!(format_shutter_angle(900), "90.0°");
    }

    #[test]
    fn test_format_color_temp() {
        assert_eq!(format_color_temp(5600), "5600 K");
    }

    #[test]
    fn test_format_exposure() {
        assert_eq!(format_exposure(100), "+1.00 stops");
        assert_eq!(format_exposure(-50), "-0.50 stops");
    }

    #[test]
    fn test_format_aperture() {
        assert_eq!(format_aperture(28), "T2.8");
        assert_eq!(format_aperture(56), "T5.6");
    }
}
