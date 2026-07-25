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
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::parrot::parrot_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_i32_value, extract_string};
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ============================================================================
// Parrot MakerNote Tag IDs (for parsing reference)
// ============================================================================
// Tag definitions are centralized in the registry (registries/parrot.rs)
// These constants are retained for parse_entry() to identify special handling

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

// Parrot signature
const PARROT_SIGNATURE: &[u8] = b"Parrot";

// Static registry instance for efficient tag lookup and decoding
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(parrot_registry);

// ============================================================================
// Custom Value Formatters
// ============================================================================

/// Formats GPS coordinates (stored as 1/10,000,000 degrees)
///
/// # Arguments
/// * `value` - GPS coordinate in 1/10,000,000 degree units
///
/// # Returns
/// Formatted coordinate with 7 decimal places
fn format_gps_coord(value: i32) -> String {
    format!("{:.7}", value as f64 / 10_000_000.0)
}

/// Formats altitude (stored as centimeters)
///
/// # Arguments
/// * `value` - Altitude in centimeters
///
/// # Returns
/// Formatted altitude in meters with 2 decimal places
fn format_altitude(value: i16) -> String {
    format!("{:.2} m", value as f64 / 100.0)
}

/// Formats speed (stored as tenths of m/s)
///
/// # Arguments
/// * `value` - Speed in 1/10 m/s units
///
/// # Returns
/// Formatted speed in m/s with 1 decimal place
fn format_speed(value: i16) -> String {
    format!("{:.1} m/s", value as f64 / 10.0)
}

/// Formats gimbal angle (stored as decidegrees)
///
/// # Arguments
/// * `value` - Angle in 1/10 degree units
///
/// # Returns
/// Formatted angle in degrees with 1 decimal place
fn format_gimbal_angle(value: i16) -> String {
    format!("{:.1}°", value as f64 / 10.0)
}

// ============================================================================
// Parser Implementation
// ============================================================================

/// Parrot Drone MakerNote parser
#[derive(Default)]
pub struct ParrotParser;

impl ParrotParser {
    /// Creates a new Parrot parser instance
    pub fn new() -> Self {
        ParrotParser
    }

    /// Parses a single IFD entry and extracts the tag value
    ///
    /// # Arguments
    /// * `entry` - IFD entry to parse
    /// * `data` - Full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    ///
    /// Handles three types of Parrot tags:
    /// - String tags (model, serial, version)
    /// - GPS coordinates (i32 values with unit conversion)
    /// - i16 array tags (flight metrics with custom formatting)
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_id = entry.tag_id;

        // Get tag name from registry first - skip unknown tags
        let tag_name = match TAG_REGISTRY.get_tag_name(tag_id) {
            Some(name) => name,
            None => return,
        };

        // Handle string tags (Model, SerialNumber, Version)
        if matches!(tag_id, PARROT_MODEL | PARROT_SERIAL | PARROT_VERSION) {
            if let Some(s) = extract_string(entry, data, byte_order) {
                tags.insert(format!("Parrot:{}", tag_name), s);
            }
            return;
        }

        // Handle GPS coordinates (i32 type with decimal formatting)
        if matches!(tag_id, PARROT_GPS_LAT | PARROT_GPS_LON) {
            if let Some(val) = extract_i32_value(entry, data, byte_order) {
                tags.insert(format!("Parrot:{}", tag_name), format_gps_coord(val));
            }
            return;
        }

        // Handle i16 array tags (flight metrics, gimbal angles, battery, etc.)
        if let Some(array) = extract_i16_array(entry, data, byte_order)
            && let Some(&value) = array.first()
        {
            // Apply tag-specific formatting
            let formatted_value = match tag_id {
                // Flight mode has a registry decoder
                PARROT_FLIGHT_MODE => TAG_REGISTRY.decode_i16(tag_id, value),

                // Altitude: cm to meters
                PARROT_ALTITUDE => format_altitude(value),

                // Speed: 0.1 m/s to m/s
                PARROT_SPEED => format_speed(value),

                // Direction: degrees
                PARROT_DIRECTION => format!("{}°", value),

                // Gimbal angles: 0.1 degrees to degrees
                PARROT_GIMBAL_PITCH | PARROT_GIMBAL_ROLL => format_gimbal_angle(value),

                // Gimbal yaw: degrees
                PARROT_GIMBAL_YAW => format!("{}°", value),

                // Battery: percentage
                PARROT_BATTERY => format!("{}%", value),

                // WiFi: dBm signal strength
                PARROT_WIFI_SIGNAL => format!("{} dBm", value),

                // Home distance: meters
                PARROT_DISTANCE => format!("{} m", value),

                // Fallback for unhandled i16 tags
                _ => return,
            };

            tags.insert(format!("Parrot:{}", tag_name), formatted_value);
        }
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
        let config = IfdParserConfig {
            signature: Some(PARROT_SIGNATURE),
            signature_offset: 6, // Skip "Parrot" signature
            max_entries: 200,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

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
    fn test_format_gps_coord() {
        assert_eq!(format_gps_coord(123456789), "12.3456789");
        assert_eq!(format_gps_coord(-87654321), "-8.7654321");
    }

    #[test]
    fn test_format_altitude() {
        assert_eq!(format_altitude(1250), "12.50 m");
        assert_eq!(format_altitude(0), "0.00 m");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(55), "5.5 m/s");
        assert_eq!(format_speed(100), "10.0 m/s");
    }

    #[test]
    fn test_format_gimbal_angle() {
        assert_eq!(format_gimbal_angle(450), "45.0°");
        assert_eq!(format_gimbal_angle(-300), "-30.0°");
    }

    #[test]
    fn test_tag_registry() {
        assert_eq!(TAG_REGISTRY.get_tag_name(PARROT_MODEL), Some("Model"));
        assert_eq!(
            TAG_REGISTRY.get_tag_name(PARROT_FLIGHT_MODE),
            Some("FlightMode")
        );
        assert!(TAG_REGISTRY.has_tag(PARROT_ALTITUDE));
    }
}
