//! Reconyx Wildlife Camera MakerNote parser
//!
//! Parses Reconyx-specific EXIF MakerNote tags from trail/wildlife cameras.
//! Reconyx specializes in motion-triggered cameras for wildlife monitoring.
//!
//! ## Supported Models
//! - HyperFire Series (HF2X, HF2XC)
//! - UltraFire Series (XR6, XP9)
//! - MicroFire Series (MR5, MS8)
//! - PC900 (security)
//!
//! ## Key Features
//! - Motion trigger details
//! - Time-lapse interval
//! - Temperature (ambient)
//! - Battery voltage
//! - Moon phase
//! - Sequence number
//! - PIR (infrared) sensor data
//!
//! ## Architecture
//! Reconyx uses specialized metadata for wildlife monitoring applications.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

const RECONYX_MODEL: u16 = 0x0001;
const RECONYX_SERIAL: u16 = 0x0002;
const RECONYX_FIRMWARE: u16 = 0x0003;
const RECONYX_TRIGGER_MODE: u16 = 0x0100;
const RECONYX_SEQUENCE_NUMBER: u16 = 0x0101;
const RECONYX_EVENT_NUMBER: u16 = 0x0102;
const RECONYX_TEMPERATURE: u16 = 0x0103;
const RECONYX_BATTERY_VOLTAGE: u16 = 0x0104;
const RECONYX_MOON_PHASE: u16 = 0x0105;
const RECONYX_TIMELAPSE_INTERVAL: u16 = 0x0106;
const RECONYX_PIR_READINGS: u16 = 0x0107;
const RECONYX_FLASH_OUTPUT: u16 = 0x0108;
const RECONYX_SENSOR_SENSITIVITY: u16 = 0x0109;
const RECONYX_MOTION_DETECT_LEVEL: u16 = 0x010A;

const RECONYX_SIGNATURE: &[u8] = b"Reconyx";

fn decode_trigger_mode(value: i16) -> String {
    match value {
        0 => "Time Lapse".to_string(),
        1 => "Motion Detection".to_string(),
        2 => "Time Lapse + Motion".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_moon_phase(value: i16) -> String {
    match value {
        0 => "New Moon".to_string(),
        1 => "Waxing Crescent".to_string(),
        2 => "First Quarter".to_string(),
        3 => "Waxing Gibbous".to_string(),
        4 => "Full Moon".to_string(),
        5 => "Waning Gibbous".to_string(),
        6 => "Last Quarter".to_string(),
        7 => "Waning Crescent".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn format_temperature(value: i16) -> String {
    format!("{}°C", value)
}

fn format_voltage(value: i16) -> String {
    format!("{:.2} V", value as f64 / 1000.0)
}

fn format_interval(value: i16) -> String {
    format!("{} seconds", value)
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

/// Reconyx Wildlife Camera MakerNote parser
/// Default implementation for parser
#[derive(Default)]
pub struct ReconyxParser;

impl ReconyxParser {
    /// Creates a new Reconyx parser instance
    pub fn new() -> Self {
        ReconyxParser
    }
}

impl MakerNoteParser for ReconyxParser {
    fn manufacturer_name(&self) -> &'static str {
        "Reconyx"
    }

    fn tag_prefix(&self) -> &'static str {
        "Reconyx:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        data.len() >= 8 && (data.starts_with(RECONYX_SIGNATURE) || data.len() >= 8)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Reconyx MakerNote data too short".to_string());
        }
        let start_offset = if data.starts_with(RECONYX_SIGNATURE) {
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
                RECONYX_MODEL | RECONYX_SERIAL | RECONYX_FIRMWARE => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            RECONYX_MODEL => "Model",
                            RECONYX_SERIAL => "SerialNumber",
                            RECONYX_FIRMWARE => "FirmwareVersion",
                            _ => continue,
                        };
                        tags.insert(format!("Reconyx:{}", tag_name), s);
                    }
                }
                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                RECONYX_TRIGGER_MODE => ("TriggerMode", decode_trigger_mode(val)),
                                RECONYX_SEQUENCE_NUMBER => ("SequenceNumber", val.to_string()),
                                RECONYX_EVENT_NUMBER => ("EventNumber", val.to_string()),
                                RECONYX_TEMPERATURE => ("Temperature", format_temperature(val)),
                                RECONYX_BATTERY_VOLTAGE => ("BatteryVoltage", format_voltage(val)),
                                RECONYX_MOON_PHASE => ("MoonPhase", decode_moon_phase(val)),
                                RECONYX_TIMELAPSE_INTERVAL => {
                                    ("TimelapseInterval", format_interval(val))
                                }
                                RECONYX_PIR_READINGS => ("PIRReadings", val.to_string()),
                                RECONYX_FLASH_OUTPUT => ("FlashOutput", format!("{}%", val)),
                                RECONYX_SENSOR_SENSITIVITY => {
                                    ("SensorSensitivity", val.to_string())
                                }
                                RECONYX_MOTION_DETECT_LEVEL => {
                                    ("MotionDetectLevel", val.to_string())
                                }
                                _ => continue,
                            };
                            tags.insert(format!("Reconyx:{}", tag_name), formatted_value);
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
    fn test_reconyx_parser_creation() {
        let parser = ReconyxParser::new();
        assert_eq!(parser.manufacturer_name(), "Reconyx");
        assert_eq!(parser.tag_prefix(), "Reconyx:");
    }

    #[test]
    fn test_decode_trigger_mode() {
        assert_eq!(decode_trigger_mode(1), "Motion Detection");
    }

    #[test]
    fn test_decode_moon_phase() {
        assert_eq!(decode_moon_phase(4), "Full Moon");
    }

    #[test]
    fn test_format_voltage() {
        assert_eq!(format_voltage(6000), "6.00 V");
    }
}
