//! Parrot Drone MakerNote parser
//!
//! Parses Parrot-specific EXIF MakerNote tags from consumer and professional drones.
//! Parrot manufactures the Anafi, Bebop, and other drone series.
//!
//! ## Supported Models
//! - Anafi (4K, USA, Thermal)
//! - Anafi AI
//! - Bebop 2
//! - Bebop Drone
//! - Disco FPV
//! - Bluegrass (agriculture)
//!
//! ## Key Features
//! - GPS coordinates and altitude
//! - Flight speed and direction
//! - Gimbal angles
//! - Battery level
//! - WiFi signal strength
//! - Flight mode
//! - Camera settings
//!
//! ## Architecture
//! Similar to DJI but with simplified metadata structure.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

const PARROT_MODEL: u16 = 0x0001;
const PARROT_SERIAL: u16 = 0x0002;
const PARROT_VERSION: u16 = 0x0003;
const PARROT_GPS_LAT: u16 = 0x0100;
const PARROT_GPS_LON: u16 = 0x0101;
const PARROT_ALTITUDE: u16 = 0x0102;
const PARROT_SPEED: u16 = 0x0103;
const PARROT_DIRECTION: u16 = 0x0104;
const PARROT_GIMBAL_PITCH: u16 = 0x0105;
const PARROT_GIMBAL_ROLL: u16 = 0x0106;
const PARROT_GIMBAL_YAW: u16 = 0x0107;
const PARROT_BATTERY: u16 = 0x0108;
const PARROT_WIFI_SIGNAL: u16 = 0x0109;
const PARROT_FLIGHT_MODE: u16 = 0x010A;
const PARROT_DISTANCE: u16 = 0x010B;

const PARROT_SIGNATURE: &[u8] = b"Parrot";

fn decode_flight_mode(value: i16) -> String {
    match value {
        0 => "Manual".to_string(),
        1 => "GPS".to_string(),
        2 => "Follow Me".to_string(),
        3 => "Return Home".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn format_gps_coord(value: i32) -> String {
    format!("{:.7}", value as f64 / 10_000_000.0)
}

fn format_altitude(value: i16) -> String {
    format!("{:.2} m", value as f64 / 100.0)
}

fn format_speed(value: i16) -> String {
    format!("{:.1} m/s", value as f64 / 10.0)
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

fn extract_i32(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<i32> {
    if entry.value_count == 1 && (entry.field_type == 4 || entry.field_type == 9) {
        return Some(entry.value_offset as i32);
    }
    let offset = entry.value_offset as usize;
    if offset + 4 > data.len() {
        return None;
    }
    match byte_order {
        ByteOrder::LittleEndian => Some(i32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])),
        ByteOrder::BigEndian => Some(i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])),
    }
}

/// Parrot Drone MakerNote parser
/// Default implementation for parser
#[derive(Default)]
pub struct ParrotParser;

impl ParrotParser {
    /// Creates a new Parrot parser instance
    pub fn new() -> Self {
        ParrotParser
    }
}

impl MakerNoteParser for ParrotParser {
    fn manufacturer_name(&self) -> &'static str {
        "Parrot"
    }

    fn tag_prefix(&self) -> &'static str {
        "Parrot:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        data.len() >= 8 && (data.starts_with(PARROT_SIGNATURE) || data.len() >= 8)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Parrot MakerNote data too short".to_string());
        }
        let start_offset = if data.starts_with(PARROT_SIGNATURE) {
            6
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
                PARROT_MODEL | PARROT_SERIAL | PARROT_VERSION => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            PARROT_MODEL => "Model",
                            PARROT_SERIAL => "SerialNumber",
                            PARROT_VERSION => "Version",
                            _ => continue,
                        };
                        tags.insert(format!("Parrot:{}", tag_name), s);
                    }
                }
                PARROT_GPS_LAT | PARROT_GPS_LON => {
                    if let Some(val) = extract_i32(&entry, parse_data, byte_order) {
                        let tag_name = if tag == PARROT_GPS_LAT {
                            "GPSLatitude"
                        } else {
                            "GPSLongitude"
                        };
                        tags.insert(format!("Parrot:{}", tag_name), format_gps_coord(val));
                    }
                }
                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                PARROT_ALTITUDE => ("Altitude", format_altitude(val)),
                                PARROT_SPEED => ("Speed", format_speed(val)),
                                PARROT_DIRECTION => ("Direction", format!("{}°", val)),
                                PARROT_GIMBAL_PITCH => {
                                    ("GimbalPitch", format!("{:.1}°", val as f64 / 10.0))
                                }
                                PARROT_GIMBAL_ROLL => {
                                    ("GimbalRoll", format!("{:.1}°", val as f64 / 10.0))
                                }
                                PARROT_GIMBAL_YAW => ("GimbalYaw", format!("{}°", val)),
                                PARROT_BATTERY => ("BatteryLevel", format!("{}%", val)),
                                PARROT_WIFI_SIGNAL => ("WiFiSignal", format!("{} dBm", val)),
                                PARROT_FLIGHT_MODE => ("FlightMode", decode_flight_mode(val)),
                                PARROT_DISTANCE => ("HomeDistance", format!("{} m", val)),
                                _ => continue,
                            };
                            tags.insert(format!("Parrot:{}", tag_name), formatted_value);
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
    fn test_parrot_parser_creation() {
        let parser = ParrotParser::new();
        assert_eq!(parser.manufacturer_name(), "Parrot");
        assert_eq!(parser.tag_prefix(), "Parrot:");
    }

    #[test]
    fn test_decode_flight_mode() {
        assert_eq!(decode_flight_mode(1), "GPS");
        assert_eq!(decode_flight_mode(3), "Return Home");
    }

    #[test]
    fn test_format_altitude() {
        assert_eq!(format_altitude(1250), "12.50 m");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(55), "5.5 m/s");
    }
}
