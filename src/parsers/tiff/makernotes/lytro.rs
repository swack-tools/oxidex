//! Lytro Light Field Camera MakerNote parser
//!
//! Parses Lytro-specific EXIF MakerNote tags from light field cameras.
//! Lytro pioneered consumer light field photography allowing post-capture refocusing.
//!
//! ## Supported Models
//! - Lytro (1st generation)
//! - Lytro ILLUM (professional)
//! - Lytro Cinema (VFX/cinema)
//!
//! ## Key Features
//! - Light field data version
//! - Microlens array specifications
//! - Depth map range
//! - Focus plane depth
//! - Refocus capability metadata
//! - Sensor configuration
//! - Processing algorithm version
//!
//! ## Architecture
//! Light field cameras capture multiple perspectives simultaneously,
//! enabling computational refocusing and depth mapping after capture.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

const LYTRO_MODEL: u16 = 0x0001;
const LYTRO_SERIAL: u16 = 0x0002;
const LYTRO_FIRMWARE: u16 = 0x0003;
const LYTRO_LF_VERSION: u16 = 0x0100; // Light field data version
const LYTRO_MICROLENS_PITCH: u16 = 0x0101; // Microlens pitch (micrometers)
const LYTRO_MICROLENS_ROTATION: u16 = 0x0102; // Microlens array rotation
const LYTRO_DEPTH_MIN: u16 = 0x0103; // Minimum depth (mm)
const LYTRO_DEPTH_MAX: u16 = 0x0104; // Maximum depth (mm)
const LYTRO_FOCUS_DEPTH: u16 = 0x0105; // Current focus plane depth (mm)
const LYTRO_REFOCUS_RANGE: u16 = 0x0106; // Refocusable depth range (mm)
const LYTRO_SENSOR_RESOLUTION: u16 = 0x0107; // Sensor resolution code
const LYTRO_IMAGE_ORIENTATION: u16 = 0x0108; // Image orientation
const LYTRO_EXPOSURE_DURATION: u16 = 0x0109; // Exposure duration (ms)
const LYTRO_ISO_SPEED: u16 = 0x010A; // ISO setting
const LYTRO_ZOOM_FACTOR: u16 = 0x010B; // Zoom factor (x100)
const LYTRO_ALGORITHM_VERSION: u16 = 0x010C; // Processing algorithm version
const LYTRO_DEPTH_MAP_ENABLED: u16 = 0x010D; // Depth map generation enabled
const LYTRO_PERSPECTIVE_SHIFT: u16 = 0x010E; // Perspective shift capability
const LYTRO_CALIBRATION_DATE: u16 = 0x010F; // Camera calibration date
const LYTRO_TEMPERATURE: u16 = 0x0110; // Sensor temperature (°C)
const LYTRO_RAW_DATA_SIZE: u16 = 0x0111; // Raw light field data size (MB)

const LYTRO_SIGNATURE: &[u8] = b"Lytro";

fn decode_sensor_resolution(value: i16) -> String {
    match value {
        0 => "Standard (1080x1080)".to_string(),
        1 => "High (2450x1634)".to_string(),
        2 => "Ultra (3280x3280)".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_orientation(value: i16) -> String {
    match value {
        0 => "Horizontal".to_string(),
        1 => "Rotate 90 CW".to_string(),
        2 => "Rotate 180".to_string(),
        3 => "Rotate 270 CW".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn format_depth(value: i16) -> String {
    if value < 1000 {
        format!("{} mm", value)
    } else {
        format!("{:.2} m", value as f64 / 1000.0)
    }
}

fn format_microlens_pitch(value: i16) -> String {
    format!("{} µm", value)
}

fn format_rotation(value: i16) -> String {
    format!("{:.2}°", value as f64 / 100.0)
}

fn format_exposure(value: i16) -> String {
    if value < 1000 {
        format!("{} ms", value)
    } else {
        format!("{:.2} s", value as f64 / 1000.0)
    }
}

fn format_zoom(value: i16) -> String {
    format!("{:.2}x", value as f64 / 100.0)
}

fn format_temperature(value: i16) -> String {
    format!("{}°C", value)
}

fn format_data_size(value: i16) -> String {
    if value < 1024 {
        format!("{} MB", value)
    } else {
        format!("{:.2} GB", value as f64 / 1024.0)
    }
}

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

/// Lytro Light Field Camera MakerNote parser
/// Default implementation for parser
#[derive(Default)]
pub struct LytroParser;

impl LytroParser {
    /// Creates a new Lytro parser instance
    pub fn new() -> Self {
        LytroParser
    }
}

impl MakerNoteParser for LytroParser {
    fn manufacturer_name(&self) -> &'static str {
        "Lytro"
    }

    fn tag_prefix(&self) -> &'static str {
        "Lytro:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        data.len() >= 8 && (data.starts_with(LYTRO_SIGNATURE) || data.len() >= 8)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Lytro MakerNote data too short".to_string());
        }
        let start_offset = if data.starts_with(LYTRO_SIGNATURE) {
            5
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
        for _ in 0..num_entries {
            if offset + 12 > parse_data.len() {
                break;
            }
            let entry_data = &parse_data[offset..offset + 12];

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
                LYTRO_MODEL
                | LYTRO_SERIAL
                | LYTRO_FIRMWARE
                | LYTRO_LF_VERSION
                | LYTRO_ALGORITHM_VERSION
                | LYTRO_CALIBRATION_DATE => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            LYTRO_MODEL => "Model",
                            LYTRO_SERIAL => "SerialNumber",
                            LYTRO_FIRMWARE => "FirmwareVersion",
                            LYTRO_LF_VERSION => "LightFieldVersion",
                            LYTRO_ALGORITHM_VERSION => "AlgorithmVersion",
                            LYTRO_CALIBRATION_DATE => "CalibrationDate",
                            _ => continue,
                        };
                        tags.insert(format!("Lytro:{}", tag_name), s);
                    }
                }
                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                LYTRO_MICROLENS_PITCH => {
                                    ("MicrolensPitch", format_microlens_pitch(val))
                                }
                                LYTRO_MICROLENS_ROTATION => {
                                    ("MicrolensRotation", format_rotation(val))
                                }
                                LYTRO_DEPTH_MIN => ("DepthMin", format_depth(val)),
                                LYTRO_DEPTH_MAX => ("DepthMax", format_depth(val)),
                                LYTRO_FOCUS_DEPTH => ("FocusDepth", format_depth(val)),
                                LYTRO_REFOCUS_RANGE => ("RefocusRange", format_depth(val)),
                                LYTRO_SENSOR_RESOLUTION => {
                                    ("SensorResolution", decode_sensor_resolution(val))
                                }
                                LYTRO_IMAGE_ORIENTATION => {
                                    ("ImageOrientation", decode_orientation(val))
                                }
                                LYTRO_EXPOSURE_DURATION => {
                                    ("ExposureDuration", format_exposure(val))
                                }
                                LYTRO_ISO_SPEED => ("ISO", val.to_string()),
                                LYTRO_ZOOM_FACTOR => ("ZoomFactor", format_zoom(val)),
                                LYTRO_DEPTH_MAP_ENABLED => (
                                    "DepthMapEnabled",
                                    if val != 0 {
                                        "Yes".to_string()
                                    } else {
                                        "No".to_string()
                                    },
                                ),
                                LYTRO_PERSPECTIVE_SHIFT => (
                                    "PerspectiveShiftCapable",
                                    if val != 0 {
                                        "Yes".to_string()
                                    } else {
                                        "No".to_string()
                                    },
                                ),
                                LYTRO_TEMPERATURE => ("SensorTemperature", format_temperature(val)),
                                LYTRO_RAW_DATA_SIZE => ("RawDataSize", format_data_size(val)),
                                _ => continue,
                            };
                            tags.insert(format!("Lytro:{}", tag_name), formatted_value);
                        }
                    }
                }
            }
            offset += 12;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lytro_parser_creation() {
        let parser = LytroParser::new();
        assert_eq!(parser.manufacturer_name(), "Lytro");
        assert_eq!(parser.tag_prefix(), "Lytro:");
    }

    #[test]
    fn test_decode_sensor_resolution() {
        assert_eq!(decode_sensor_resolution(1), "High (2450x1634)");
    }

    #[test]
    fn test_format_depth() {
        assert_eq!(format_depth(500), "500 mm");
        assert_eq!(format_depth(2500), "2.50 m");
    }

    #[test]
    fn test_format_zoom() {
        assert_eq!(format_zoom(100), "1.00x");
        assert_eq!(format_zoom(800), "8.00x");
    }

    #[test]
    fn test_format_microlens_pitch() {
        assert_eq!(format_microlens_pitch(14), "14 µm");
    }
}
