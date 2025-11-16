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
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// InfiRay MakerNote Tag IDs
const INFIRAY_MODEL: u16 = 0x0001; // Camera model
const INFIRAY_SERIAL: u16 = 0x0002; // Serial number
const INFIRAY_FIRMWARE: u16 = 0x0003; // Firmware version
const INFIRAY_TEMP_MIN: u16 = 0x0100; // Minimum temperature (°C * 10)
const INFIRAY_TEMP_MAX: u16 = 0x0101; // Maximum temperature (°C * 10)
const INFIRAY_TEMP_CENTER: u16 = 0x0102; // Center temperature (°C * 10)
const INFIRAY_EMISSIVITY: u16 = 0x0103; // Emissivity (0-100)
const INFIRAY_DISTANCE: u16 = 0x0104; // Distance to object (cm)
const INFIRAY_PALETTE: u16 = 0x0105; // Color palette
const INFIRAY_RANGE_MIN: u16 = 0x0106; // Measurement range min (°C * 10)
const INFIRAY_RANGE_MAX: u16 = 0x0107; // Measurement range max (°C * 10)
const INFIRAY_ATMOSPHERIC_TEMP: u16 = 0x0108; // Atmospheric temp (°C * 10)
const INFIRAY_HUMIDITY: u16 = 0x0109; // Relative humidity (%)
const INFIRAY_ENHANCEMENT: u16 = 0x010A; // Image enhancement mode
const INFIRAY_ZOOM: u16 = 0x010B; // Digital zoom level
const INFIRAY_CONTRAST: u16 = 0x010C; // Contrast level
const INFIRAY_BRIGHTNESS: u16 = 0x010D; // Brightness level
const INFIRAY_SHARPNESS: u16 = 0x010E; // Sharpness level
const INFIRAY_SPOT_METER: u16 = 0x010F; // Spot meter position
const INFIRAY_ISOTHERM: u16 = 0x0110; // Isotherm mode
const INFIRAY_UNIT: u16 = 0x0111; // Temperature unit

const INFIRAY_SIGNATURE: &[u8] = b"InfiRay";

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

    if s.is_empty() {
        None
    } else {
        Some(s)
    }
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
        if data.len() < 8 {
            return Err("InfiRay MakerNote data too short".to_string());
        }

        let start_offset = if data.starts_with(INFIRAY_SIGNATURE) {
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
                INFIRAY_MODEL | INFIRAY_SERIAL | INFIRAY_FIRMWARE => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            INFIRAY_MODEL => "Model",
                            INFIRAY_SERIAL => "SerialNumber",
                            INFIRAY_FIRMWARE => "FirmwareVersion",
                            _ => continue,
                        };
                        tags.insert(format!("InfiRay:{}", tag_name), s);
                    }
                }
                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                INFIRAY_TEMP_MIN => ("TemperatureMin", format_temperature(val)),
                                INFIRAY_TEMP_MAX => ("TemperatureMax", format_temperature(val)),
                                INFIRAY_TEMP_CENTER => {
                                    ("TemperatureCenter", format_temperature(val))
                                }
                                INFIRAY_EMISSIVITY => ("Emissivity", format_emissivity(val)),
                                INFIRAY_DISTANCE => ("Distance", format_distance(val)),
                                INFIRAY_PALETTE => ("Palette", decode_palette(val)),
                                INFIRAY_RANGE_MIN => ("RangeMin", format_temperature(val)),
                                INFIRAY_RANGE_MAX => ("RangeMax", format_temperature(val)),
                                INFIRAY_ATMOSPHERIC_TEMP => {
                                    ("AtmosphericTemp", format_temperature(val))
                                }
                                INFIRAY_HUMIDITY => ("Humidity", format!("{}%", val)),
                                INFIRAY_ENHANCEMENT => ("Enhancement", decode_enhancement(val)),
                                INFIRAY_ZOOM => ("DigitalZoom", format_zoom(val)),
                                INFIRAY_CONTRAST => ("Contrast", val.to_string()),
                                INFIRAY_BRIGHTNESS => ("Brightness", val.to_string()),
                                INFIRAY_SHARPNESS => ("Sharpness", val.to_string()),
                                INFIRAY_SPOT_METER => (
                                    "SpotMeter",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                INFIRAY_ISOTHERM => (
                                    "Isotherm",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                INFIRAY_UNIT => ("TemperatureUnit", decode_unit(val)),
                                _ => continue,
                            };
                            tags.insert(format!("InfiRay:{}", tag_name), formatted_value);
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
