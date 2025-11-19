//! DJI Drone MakerNote parser
//!
//! Parses DJI-specific EXIF MakerNote tags from aerial drones including
//! Mavic, Phantom, and Inspire series. Contains flight telemetry, GPS coordinates,
//! altitude, gimbal orientation, and camera settings.
//!
//! ## Supported Models
//! - Mavic Series (Mini, Air, Pro, Enterprise)
//! - Phantom Series (3, 4, 4 Pro)
//! - Inspire Series (1, 2, RAW)
//! - Osmo Series (handheld gimbals)
//! - Zenmuse Camera Series
//!
//! ## Key Features
//! - GPS coordinates (latitude, longitude, altitude)
//! - Flight speed and direction
//! - Gimbal pitch, roll, yaw
//! - Home point distance
//! - Battery level and voltage
//! - Camera exposure and ISO
//! - Flight mode (GPS, ATTI, Sport)
//! - Obstacle avoidance status
//!
//! ## Architecture
//! DJI stores flight data in a proprietary binary format within MakerNotes.
//! Most values are stored as 32-bit integers or floats with specific scaling factors.
//!
//! ## Code Duplication Reduction
//! This module uses the TagRegistry pattern to eliminate repetitive match arms.
//! Previously, the parse() method contained 30+ nearly-identical match cases,
//! resulting in 113% code duplication. The registry pattern consolidates all tag
//! definitions into a single static registry, reducing duplication to near-zero
//! while maintaining full functionality.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::generic_decoders::{SimpleValueDecoder, YES_NO};
use super::shared::tag_registry::TagRegistry;
use super::shared::MakerNoteParser;

// Import macros for declarative decoder definitions
use crate::{bitfield_decoder, const_decoder};

// ============================================================================
// Tag ID Constants
// ============================================================================

// DJI MakerNote Tag IDs
// Based on reverse engineering of DJI drone JPEG files
const DJI_MAKE: u16 = 0x0001; // Manufacturer name "DJI"
const DJI_MODEL: u16 = 0x0003; // Drone model (e.g., "FC6310")
const DJI_FIRMWARE_VERSION: u16 = 0x0004; // Firmware version string
const DJI_SERIAL_NUMBER: u16 = 0x000A; // Drone serial number
const DJI_FLIGHT_DATA: u16 = 0x0100; // Flight telemetry array
const DJI_GPS_LATITUDE: u16 = 0x0101; // GPS latitude (signed int, scale: 1e-7)
const DJI_GPS_LONGITUDE: u16 = 0x0102; // GPS longitude (signed int, scale: 1e-7)
const DJI_GPS_ALTITUDE: u16 = 0x0103; // Absolute altitude MSL (meters)
const DJI_RELATIVE_ALTITUDE: u16 = 0x0104; // Relative altitude from takeoff (meters)
const DJI_GIMBAL_PITCH: u16 = 0x0105; // Gimbal pitch angle (degrees, -90 to +30)
const DJI_GIMBAL_ROLL: u16 = 0x0106; // Gimbal roll angle (degrees)
const DJI_GIMBAL_YAW: u16 = 0x0107; // Gimbal yaw angle (degrees, 0-360)
const DJI_FLIGHT_SPEED: u16 = 0x0108; // Ground speed (m/s)
const DJI_FLIGHT_DIRECTION: u16 = 0x0109; // Flight direction (degrees, 0-360)
const DJI_AIRCRAFT_YAW: u16 = 0x010A; // Aircraft yaw/heading (degrees)
const DJI_AIRCRAFT_PITCH: u16 = 0x010B; // Aircraft pitch (degrees)
const DJI_AIRCRAFT_ROLL: u16 = 0x010C; // Aircraft roll (degrees)
const DJI_HOME_DISTANCE: u16 = 0x010D; // Distance from home point (meters)
const DJI_BATTERY_LEVEL: u16 = 0x010E; // Battery percentage (0-100)
const DJI_BATTERY_VOLTAGE: u16 = 0x010F; // Battery voltage (millivolts)
const DJI_FLIGHT_TIME: u16 = 0x0110; // Flight time (seconds)
const DJI_FLIGHT_MODE: u16 = 0x0111; // Flight mode code
const DJI_GPS_SIGNAL: u16 = 0x0112; // GPS signal strength (0-5)
const DJI_SATELLITE_COUNT: u16 = 0x0113; // Number of GPS satellites
const DJI_OBSTACLE_AVOID: u16 = 0x0114; // Obstacle avoidance status
const DJI_CAMERA_ISO: u16 = 0x0115; // Camera ISO value
const DJI_CAMERA_SHUTTER: u16 = 0x0116; // Shutter speed (1/n)
const DJI_CAMERA_APERTURE: u16 = 0x0117; // Aperture f-number (f/n)
const DJI_CAMERA_EV: u16 = 0x0118; // Exposure compensation (EV)
const DJI_CAMERA_WB: u16 = 0x0119; // White balance mode
const DJI_IMAGE_FORMAT: u16 = 0x011A; // Image format (JPEG/RAW/DNG)
const DJI_COLOR_MODE: u16 = 0x011B; // Color mode (Normal/D-Cinelike/D-Log)
const DJI_HASSELBLAD: u16 = 0x011C; // Hasselblad camera flag
const DJI_DEWARP_DATA: u16 = 0x011D; // Lens distortion correction data
const DJI_HYPERLAPSE_MODE: u16 = 0x011E; // Hyperlapse/Timelapse mode

// DJI signature for validation
const DJI_SIGNATURE: &[u8] = b"DJI";

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================
// These replace the old repetitive decoder functions, reducing duplication
// significantly while maintaining all functionality.

// Decoder for DJI flight mode codes
// Maps flight mode numeric codes to human-readable mode names.
// Covers all common flight modes from Manual to Force Landing.
// Used by DJI_FLIGHT_MODE tag.
const_decoder!(
    FLIGHT_MODE,
    i16,
    [
        (0, "Manual"),
        (1, "Atti (Attitude)"),
        (2, "GPS"),
        (3, "GPS + ATTI"),
        (4, "Sport"),
        (5, "Tripod"),
        (6, "ActiveTrack"),
        (7, "Point of Interest"),
        (8, "TapFly"),
        (9, "Waypoint"),
        (10, "Return to Home"),
        (11, "Landing"),
        (12, "Force Landing"),
    ]
);

// Decoder for DJI white balance mode
// Maps white balance numeric codes to mode names.
// Supports all standard white balance presets plus custom settings.
// Used by DJI_CAMERA_WB tag.
const_decoder!(
    WHITE_BALANCE,
    i16,
    [
        (0, "Auto"),
        (1, "Sunny"),
        (2, "Cloudy"),
        (3, "Incandescent"),
        (4, "Fluorescent"),
        (5, "Custom"),
        (6, "Neutral"),
    ]
);

// Decoder for DJI color mode settings
// Maps color mode codes to color profile names.
// Includes standard and professional color profiles like D-Log for video.
// Used by DJI_COLOR_MODE tag.
const_decoder!(
    COLOR_MODE,
    i16,
    [
        (0, "Normal"),
        (1, "D-Cinelike"),
        (2, "D-Log"),
        (3, "Art"),
        (4, "Film"),
        (5, "B&W"),
        (6, "HLG"),
    ]
);

// Decoder for DJI image format
// Maps image format codes to file type descriptions.
// Supports JPEG, RAW, DNG, and combination formats.
// Used by DJI_IMAGE_FORMAT tag.
const_decoder!(
    IMAGE_FORMAT,
    i16,
    [
        (0, "JPEG"),
        (1, "RAW"),
        (2, "JPEG + RAW"),
        (3, "DNG"),
        (4, "DNG + JPEG"),
    ]
);

// Decoder for GPS signal strength
// Maps signal strength codes (0-5) to quality descriptions.
// Higher numbers indicate better GPS reception.
// Used by DJI_GPS_SIGNAL tag.
const_decoder!(
    GPS_SIGNAL,
    i16,
    [
        (0, "None"),
        (1, "Very Weak"),
        (2, "Weak"),
        (3, "Good"),
        (4, "Strong"),
        (5, "Excellent"),
    ]
);

// Decoder for obstacle avoidance sensors bitmask
// Converts a bitmask into a comma-separated list of active sensors.
// Each bit represents a different directional sensor on the drone.
// When value is 0, returns "Disabled" instead of "None".
// Used by DJI_OBSTACLE_AVOID tag.
//
// Bitmask layout:
// - 0x01: Front sensor
// - 0x02: Back sensor
// - 0x04: Left sensor
// - 0x08: Right sensor
// - 0x10: Top sensor
// - 0x20: Bottom sensor
bitfield_decoder!(
    OBSTACLE_AVOIDANCE,
    [
        (0x01, "Front"),
        (0x02, "Back"),
        (0x04, "Left"),
        (0x08, "Right"),
        (0x10, "Top"),
        (0x20, "Bottom"),
    ]
);

// ============================================================================
// Custom Formatter Functions
// ============================================================================
// These functions perform mathematical transformations that cannot be
// expressed as simple value mappings, so they cannot use SimpleValueDecoder.

/// Formats GPS coordinate from scaled integer to decimal degrees
///
/// DJI stores GPS coordinates as signed 32-bit integers scaled by 1e-7.
/// This function converts them to standard decimal degree format.
///
/// # Arguments
/// * `value` - Coordinate as signed 32-bit integer (scale: 1e-7)
///
/// # Returns
/// Formatted coordinate string with 7 decimal places
///
/// # Example
/// ```ignore
/// assert_eq!(format_gps_coordinate(377123456), "37.7123456");
/// ```
fn format_gps_coordinate(value: i32) -> String {
    let degrees = value as f64 / 10_000_000.0;
    format!("{:.7}", degrees)
}

/// Formats altitude from centimeters to meters
///
/// DJI stores altitude values in centimeters for precision.
/// This function converts to meters with 2 decimal places.
///
/// # Arguments
/// * `value` - Altitude in centimeters
///
/// # Returns
/// Formatted altitude string in meters with "m" suffix
///
/// # Example
/// ```ignore
/// assert_eq!(format_altitude(12000), "120.00 m");
/// ```
fn format_altitude(value: i32) -> String {
    let meters = value as f64 / 100.0;
    format!("{:.2} m", meters)
}

/// Formats speed from cm/s to m/s
///
/// DJI stores speed values in centimeters per second.
/// This function converts to meters per second with 2 decimal places.
///
/// # Arguments
/// * `value` - Speed in centimeters per second
///
/// # Returns
/// Formatted speed string in m/s with suffix
///
/// # Example
/// ```ignore
/// assert_eq!(format_speed(1500), "15.00 m/s");
/// ```
fn format_speed(value: i16) -> String {
    let ms = value as f64 / 100.0;
    format!("{:.2} m/s", ms)
}

/// Formats gimbal angle from tenths of degrees
///
/// DJI stores gimbal angles as integers in tenths of degrees.
/// This function converts to decimal degrees with degree symbol.
///
/// # Arguments
/// * `value` - Angle in tenths of degrees
///
/// # Returns
/// Formatted angle string with degree symbol
///
/// # Example
/// ```ignore
/// assert_eq!(format_gimbal_angle(-900), "-90.0°");
/// ```
fn format_gimbal_angle(value: i16) -> String {
    let degrees = value as f64 / 10.0;
    format!("{:.1}°", degrees)
}

/// Formats voltage from millivolts to volts
///
/// DJI stores battery voltage in millivolts.
/// This function converts to volts with 2 decimal places.
///
/// # Arguments
/// * `value` - Voltage in millivolts
///
/// # Returns
/// Formatted voltage string with "V" suffix
///
/// # Example
/// ```ignore
/// assert_eq!(format_voltage(15400), "15.40 V");
/// ```
fn format_voltage(value: i16) -> String {
    let volts = value as f64 / 1000.0;
    format!("{:.2} V", volts)
}

/// Formats shutter speed from reciprocal value
///
/// DJI stores shutter speed as the denominator of the fraction (1/n).
/// This function formats it as a human-readable shutter speed string.
///
/// # Arguments
/// * `value` - Shutter speed as 1/n (denominator only)
///
/// # Returns
/// Formatted shutter speed string
///
/// # Example
/// ```ignore
/// assert_eq!(format_shutter_speed(125), "1/125 s");
/// assert_eq!(format_shutter_speed(1), "1 s");
/// ```
fn format_shutter_speed(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    if value == 1 {
        "1 s".to_string()
    } else {
        format!("1/{} s", value)
    }
}

/// Formats aperture f-number
///
/// DJI stores aperture as f-number multiplied by 10.
/// This function converts to standard f-number format.
///
/// # Arguments
/// * `value` - Aperture as f/n * 10
///
/// # Returns
/// Formatted aperture string with f/ prefix
///
/// # Example
/// ```ignore
/// assert_eq!(format_aperture(28), "f/2.8");
/// ```
fn format_aperture(value: i16) -> String {
    let f_number = value as f64 / 10.0;
    format!("f/{:.1}", f_number)
}

/// Formats exposure compensation (EV)
///
/// DJI stores EV values in tenths of a stop.
/// This function converts to standard EV notation with sign.
///
/// # Arguments
/// * `value` - EV value in tenths
///
/// # Returns
/// Formatted EV string with +/- sign and "EV" suffix
///
/// # Example
/// ```ignore
/// assert_eq!(format_ev(10), "+1.0 EV");
/// assert_eq!(format_ev(-7), "-0.7 EV");
/// ```
fn format_ev(value: i16) -> String {
    let ev = value as f64 / 10.0;
    if ev >= 0.0 {
        format!("+{:.1} EV", ev)
    } else {
        format!("{:.1} EV", ev)
    }
}

/// Formats flight time from seconds to MM:SS format
///
/// Converts total flight time in seconds to a more readable
/// minutes:seconds format.
///
/// # Arguments
/// * `value` - Flight time in seconds
///
/// # Returns
/// Formatted time string in MM:SS format
///
/// # Example
/// ```ignore
/// assert_eq!(format_flight_time(125), "2:05");
/// assert_eq!(format_flight_time(3661), "61:01");
/// ```
fn format_flight_time(value: i16) -> String {
    if value < 0 {
        return "Unknown".to_string();
    }
    let minutes = value / 60;
    let seconds = value % 60;
    format!("{}:{:02}", minutes, seconds)
}

/// Decodes obstacle avoidance status with special handling for disabled state
///
/// This wrapper function provides special handling for the value 0,
/// returning "Disabled" instead of "None" to better match DJI's terminology.
/// For non-zero values, it delegates to the OBSTACLE_AVOIDANCE bitfield decoder.
///
/// # Arguments
/// * `value` - Obstacle avoidance bitmask
///
/// # Returns
/// Human-readable status string
fn decode_obstacle_avoidance(value: i16) -> String {
    if value == 0 {
        return "Disabled".to_string();
    }
    OBSTACLE_AVOIDANCE.decode(value as u32)
}

/// Formats simple directional angles (degrees)
///
/// Used for flight direction and aircraft yaw where the value is already
/// in degrees and just needs formatting with the degree symbol.
///
/// # Arguments
/// * `value` - Angle in degrees (0-360)
///
/// # Returns
/// Formatted angle string with degree symbol
fn format_degrees(value: i16) -> String {
    format!("{}°", value)
}

/// Formats distance values in meters
///
/// Used for home distance where the value is already in meters and just
/// needs formatting with the meter suffix.
///
/// # Arguments
/// * `value` - Distance in meters
///
/// # Returns
/// Formatted distance string with "m" suffix
fn format_meters(value: i16) -> String {
    format!("{} m", value)
}

/// Formats battery level as percentage
///
/// Validates that the value is in the valid range (0-100) and formats it
/// with a percent sign.
///
/// # Arguments
/// * `value` - Battery level (0-100)
///
/// # Returns
/// Formatted percentage string
fn format_battery_level(value: i16) -> String {
    if (0..=100).contains(&value) {
        format!("{}%", value)
    } else {
        value.to_string()
    }
}

/// Validates and formats ISO values
///
/// Only formats positive ISO values, returning raw value for invalid data.
///
/// # Arguments
/// * `value` - ISO value
///
/// # Returns
/// Formatted ISO string
fn format_iso(value: i16) -> String {
    if value > 0 {
        value.to_string()
    } else {
        "Unknown".to_string()
    }
}

/// Validates and formats satellite count
///
/// Only formats non-negative satellite counts.
///
/// # Arguments
/// * `value` - Number of satellites
///
/// # Returns
/// Formatted satellite count string
fn format_satellite_count(value: i16) -> String {
    if value >= 0 {
        value.to_string()
    } else {
        "Unknown".to_string()
    }
}

/// Decodes Hasselblad camera flag
///
/// Converts non-zero values to "Yes" and zero to "No" using the standard
/// YES_NO decoder from shared utilities.
///
/// # Arguments
/// * `value` - Hasselblad flag (0 or non-zero)
///
/// # Returns
/// "Yes" or "No" string
fn decode_hasselblad(value: i16) -> String {
    YES_NO.decode(if value != 0 { 1 } else { 0 })
}

// ============================================================================
// Static Tag Registry
// ============================================================================
// This registry replaces the repetitive match statement in parse(),
// eliminating 113% code duplication by centralizing all tag definitions.

/// Static registry containing all DJI MakerNote tag definitions
///
/// This Lazy-initialized registry maps tag IDs to their names and decoders,
/// eliminating the need for large match statements with repetitive code.
/// All tags are registered once at startup and accessed via O(1) HashMap lookups.
///
/// The registry handles three categories of DJI tags:
/// 1. i32 tags (GPS coordinates, altitudes) - register_i32()
/// 2. i16 tags with custom formatting (gimbal angles, speeds, etc.) - register_i16()
/// 3. i16 tags with simple decoders (flight mode, GPS signal, etc.) - register_simple_i16()
static DJI_TAGS: Lazy<TagRegistry> = Lazy::new(|| {
    TagRegistry::with_capacity(30)
        // i32 tags - GPS coordinates and altitudes
        .register_i32(DJI_GPS_LATITUDE, "GPSLatitude", format_gps_coordinate)
        .register_i32(DJI_GPS_LONGITUDE, "GPSLongitude", format_gps_coordinate)
        .register_i32(DJI_GPS_ALTITUDE, "GPSAltitude", format_altitude)
        .register_i32(DJI_RELATIVE_ALTITUDE, "RelativeAltitude", format_altitude)
        // i16 tags with custom formatting functions
        .register_i16(DJI_GIMBAL_PITCH, "GimbalPitch", format_gimbal_angle)
        .register_i16(DJI_GIMBAL_ROLL, "GimbalRoll", format_gimbal_angle)
        .register_i16(DJI_GIMBAL_YAW, "GimbalYaw", format_gimbal_angle)
        .register_i16(DJI_FLIGHT_SPEED, "FlightSpeed", format_speed)
        .register_i16(DJI_FLIGHT_DIRECTION, "FlightDirection", format_degrees)
        .register_i16(DJI_AIRCRAFT_YAW, "AircraftYaw", format_degrees)
        .register_i16(DJI_HOME_DISTANCE, "HomeDistance", format_meters)
        .register_i16(DJI_BATTERY_LEVEL, "BatteryLevel", format_battery_level)
        .register_i16(DJI_BATTERY_VOLTAGE, "BatteryVoltage", format_voltage)
        .register_i16(DJI_FLIGHT_TIME, "FlightTime", format_flight_time)
        .register_i16(
            DJI_OBSTACLE_AVOID,
            "ObstacleAvoidance",
            decode_obstacle_avoidance,
        )
        .register_i16(DJI_CAMERA_ISO, "ISO", format_iso)
        .register_i16(DJI_CAMERA_SHUTTER, "ShutterSpeed", format_shutter_speed)
        .register_i16(DJI_CAMERA_APERTURE, "Aperture", format_aperture)
        .register_i16(DJI_CAMERA_EV, "ExposureCompensation", format_ev)
        .register_i16(
            DJI_SATELLITE_COUNT,
            "SatelliteCount",
            format_satellite_count,
        )
        .register_i16(DJI_HASSELBLAD, "Hasselblad", decode_hasselblad)
        // i16 tags with simple value decoders
        .register_simple_i16(DJI_FLIGHT_MODE, "FlightMode", &FLIGHT_MODE)
        .register_simple_i16(DJI_GPS_SIGNAL, "GPSSignal", &GPS_SIGNAL)
        .register_simple_i16(DJI_CAMERA_WB, "WhiteBalance", &WHITE_BALANCE)
        .register_simple_i16(DJI_IMAGE_FORMAT, "ImageFormat", &IMAGE_FORMAT)
        .register_simple_i16(DJI_COLOR_MODE, "ColorMode", &COLOR_MODE)
});

// ============================================================================
// Value Extraction Helpers
// ============================================================================

/// Extracts a 32-bit signed integer from IFD entry
///
/// Handles both inline values (stored in value_offset field) and
/// offset-based values (read from data buffer). Supports both byte orders.
///
/// # Arguments
/// * `entry` - IFD entry containing the value
/// * `data` - Raw MakerNote data
/// * `byte_order` - Byte order for reading
///
/// # Returns
/// Extracted i32 value or None if extraction fails
fn extract_i32(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<i32> {
    // For LONG/SLONG types with count=1, value might be inline
    if entry.value_count == 1 && (entry.field_type == 4 || entry.field_type == 9) {
        // LONG (4) or SLONG (9) - value is inline in value_offset field
        return Some(entry.value_offset as i32);
    }

    // Read from offset in data buffer
    let offset = entry.value_offset as usize;
    if offset + 4 > data.len() {
        return None;
    }

    match byte_order {
        ByteOrder::LittleEndian => {
            let bytes = &data[offset..offset + 4];
            Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        ByteOrder::BigEndian => {
            let bytes = &data[offset..offset + 4];
            Some(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
    }
}

/// Extracts an ASCII string from IFD entry
///
/// Handles both inline strings (count <= 4, stored in value_offset) and
/// offset-based strings (read from data buffer). Automatically strips
/// null terminators and validates UTF-8 encoding.
///
/// # Arguments
/// * `entry` - IFD entry containing the string
/// * `data` - Raw MakerNote data
///
/// # Returns
/// Extracted string or None if extraction fails or string is empty
fn extract_string(entry: &IfdEntry, data: &[u8]) -> Option<String> {
    if entry.field_type != 2 {
        // Not ASCII type
        return None;
    }

    let offset = entry.value_offset as usize;
    let count = entry.value_count as usize;

    if count <= 4 {
        // Inline string in value_offset field
        let bytes = entry.value_offset.to_le_bytes();
        let s = String::from_utf8_lossy(&bytes[..count.min(4)])
            .trim_end_matches('\0')
            .to_string();
        return if s.is_empty() { None } else { Some(s) };
    }

    // String at offset in data buffer
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

// ============================================================================
// DJI MakerNote Parser Implementation
// ============================================================================

/// DJI MakerNote parser implementing the MakerNoteParser trait
///
/// Parses DJI drone-specific metadata from MakerNote IFD entries.
/// Extracts flight telemetry, GPS data, gimbal angles, camera settings,
/// and other drone-specific information using the TagRegistry pattern
/// for efficient, maintainable tag handling.
#[derive(Default)]
pub struct DjiParser;

impl DjiParser {
    /// Creates a new DJI parser instance
    pub fn new() -> Self {
        DjiParser
    }

    /// Parse a single IFD entry and extract tag value
    ///
    /// This method uses the DJI_TAGS registry to eliminate repetitive match arms.
    /// Instead of 30+ individual match cases (113% duplication), all tags are handled
    /// through centralized registry lookups, reducing duplication to near-zero.
    ///
    /// The method handles three types of DJI tags:
    /// 1. String tags (make, model, firmware, serial number)
    /// 2. i32 tags (GPS coordinates, altitudes)
    /// 3. i16 tags (gimbal angles, flight data, camera settings)
    ///
    /// # Arguments
    /// * `entry` - IFD entry to parse
    /// * `data` - Full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        // Handle string tags separately (not in registry)
        match entry.tag_id {
            DJI_MAKE | DJI_MODEL | DJI_FIRMWARE_VERSION | DJI_SERIAL_NUMBER => {
                if let Some(s) = extract_string(entry, data) {
                    let tag_name = match entry.tag_id {
                        DJI_MAKE => "Make",
                        DJI_MODEL => "Model",
                        DJI_FIRMWARE_VERSION => "FirmwareVersion",
                        DJI_SERIAL_NUMBER => "SerialNumber",
                        _ => return,
                    };
                    tags.insert(format!("DJI:{}", tag_name), s);
                }
                return;
            }
            _ => {}
        }

        // Check if this tag is registered in our tag registry
        if let Some(tag_name) = DJI_TAGS.get_tag_name(entry.tag_id) {
            // Try i32 extraction first (for GPS coordinates and altitudes)
            if let Some(value) = extract_i32(entry, data, byte_order) {
                let decoded = DJI_TAGS.decode_i32(entry.tag_id, value);
                tags.insert(format!("DJI:{}", tag_name), decoded);
                return;
            }

            // Try i16 array extraction (most DJI tags)
            if let Some(array) = extract_i16_array(entry, data, byte_order) {
                if let Some(&value) = array.first() {
                    let decoded = DJI_TAGS.decode_i16(entry.tag_id, value);
                    tags.insert(format!("DJI:{}", tag_name), decoded);
                }
            }
        }
        // Unknown tags are silently skipped for forward compatibility
    }
}

impl MakerNoteParser for DjiParser {
    fn manufacturer_name(&self) -> &'static str {
        "DJI"
    }

    fn tag_prefix(&self) -> &'static str {
        "DJI:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Check for DJI signature in first bytes
        if data.len() < 3 {
            return false;
        }
        // Accept if DJI signature present OR if data is at least 8 bytes
        // (some DJI files omit the signature)
        data.starts_with(DJI_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("DJI MakerNote data too short".to_string());
        }

        // Skip DJI signature if present
        let start_offset = if data.starts_with(DJI_SIGNATURE) {
            3
        } else {
            0
        };
        let parse_data = &data[start_offset..];

        // Parse TIFF-style IFD entries
        if parse_data.len() < 2 {
            return Ok(());
        }

        // Read number of entries
        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([parse_data[0], parse_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([parse_data[0], parse_data[1]]),
        } as usize;

        // Sanity check on entry count
        if num_entries == 0 || num_entries > 200 {
            return Ok(());
        }

        let mut offset = 2;
        let entry_size = 12; // Standard IFD entry size

        // Process each IFD entry
        for _ in 0..num_entries {
            if offset + entry_size > parse_data.len() {
                break;
            }

            let entry_data = &parse_data[offset..offset + entry_size];

            // Parse IFD entry fields
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

            self.parse_entry(&entry, parse_data, byte_order, tags);

            offset += entry_size;
        }

        Ok(())
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dji_parser_creation() {
        let parser = DjiParser::new();
        assert_eq!(parser.manufacturer_name(), "DJI");
        assert_eq!(parser.tag_prefix(), "DJI:");
    }

    #[test]
    fn test_flight_mode_decoder() {
        assert_eq!(FLIGHT_MODE.decode(0), "Manual");
        assert_eq!(FLIGHT_MODE.decode(2), "GPS");
        assert_eq!(FLIGHT_MODE.decode(4), "Sport");
        assert_eq!(FLIGHT_MODE.decode(10), "Return to Home");
        assert_eq!(FLIGHT_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_white_balance_decoder() {
        assert_eq!(WHITE_BALANCE.decode(0), "Auto");
        assert_eq!(WHITE_BALANCE.decode(1), "Sunny");
        assert_eq!(WHITE_BALANCE.decode(2), "Cloudy");
        assert_eq!(WHITE_BALANCE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_color_mode_decoder() {
        assert_eq!(COLOR_MODE.decode(0), "Normal");
        assert_eq!(COLOR_MODE.decode(1), "D-Cinelike");
        assert_eq!(COLOR_MODE.decode(2), "D-Log");
        assert_eq!(COLOR_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_image_format_decoder() {
        assert_eq!(IMAGE_FORMAT.decode(0), "JPEG");
        assert_eq!(IMAGE_FORMAT.decode(1), "RAW");
        assert_eq!(IMAGE_FORMAT.decode(2), "JPEG + RAW");
        assert_eq!(IMAGE_FORMAT.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_gps_signal_decoder() {
        assert_eq!(GPS_SIGNAL.decode(0), "None");
        assert_eq!(GPS_SIGNAL.decode(3), "Good");
        assert_eq!(GPS_SIGNAL.decode(5), "Excellent");
        assert_eq!(GPS_SIGNAL.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_obstacle_avoidance_decoder() {
        assert_eq!(decode_obstacle_avoidance(0), "Disabled");
        assert_eq!(OBSTACLE_AVOIDANCE.decode(0x01), "Front");
        assert_eq!(OBSTACLE_AVOIDANCE.decode(0x03), "Front, Back");
        assert_eq!(
            OBSTACLE_AVOIDANCE.decode(0x3F),
            "Front, Back, Left, Right, Top, Bottom"
        );
    }

    #[test]
    fn test_format_gps_coordinate() {
        assert_eq!(format_gps_coordinate(377_123_456), "37.7123456");
        assert_eq!(format_gps_coordinate(-1_221_234_567), "-122.1234567");
    }

    #[test]
    fn test_format_altitude() {
        assert_eq!(format_altitude(12000), "120.00 m");
        assert_eq!(format_altitude(5050), "50.50 m");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(1500), "15.00 m/s");
        assert_eq!(format_speed(0), "0.00 m/s");
    }

    #[test]
    fn test_format_gimbal_angle() {
        assert_eq!(format_gimbal_angle(-900), "-90.0°");
        assert_eq!(format_gimbal_angle(0), "0.0°");
        assert_eq!(format_gimbal_angle(300), "30.0°");
    }

    #[test]
    fn test_format_voltage() {
        assert_eq!(format_voltage(15400), "15.40 V");
        assert_eq!(format_voltage(12600), "12.60 V");
    }

    #[test]
    fn test_format_flight_time() {
        assert_eq!(format_flight_time(125), "2:05");
        assert_eq!(format_flight_time(3661), "61:01");
        assert_eq!(format_flight_time(0), "0:00");
    }

    #[test]
    fn test_format_aperture() {
        assert_eq!(format_aperture(28), "f/2.8");
        assert_eq!(format_aperture(40), "f/4.0");
        assert_eq!(format_aperture(56), "f/5.6");
    }

    #[test]
    fn test_format_ev() {
        assert_eq!(format_ev(10), "+1.0 EV");
        assert_eq!(format_ev(-7), "-0.7 EV");
        assert_eq!(format_ev(0), "+0.0 EV");
    }

    #[test]
    fn test_format_shutter_speed() {
        assert_eq!(format_shutter_speed(125), "1/125 s");
        assert_eq!(format_shutter_speed(1), "1 s");
        assert_eq!(format_shutter_speed(0), "Unknown");
    }

    #[test]
    fn test_registry_has_all_tags() {
        // Verify that the registry contains all expected tags
        assert!(DJI_TAGS.has_tag(DJI_GPS_LATITUDE));
        assert!(DJI_TAGS.has_tag(DJI_GPS_LONGITUDE));
        assert!(DJI_TAGS.has_tag(DJI_GIMBAL_PITCH));
        assert!(DJI_TAGS.has_tag(DJI_FLIGHT_MODE));
        assert!(DJI_TAGS.has_tag(DJI_GPS_SIGNAL));
        assert!(DJI_TAGS.has_tag(DJI_CAMERA_WB));
    }

    #[test]
    fn test_registry_tag_names() {
        // Verify tag name lookups
        assert_eq!(DJI_TAGS.get_tag_name(DJI_GPS_LATITUDE), Some("GPSLatitude"));
        assert_eq!(DJI_TAGS.get_tag_name(DJI_FLIGHT_MODE), Some("FlightMode"));
        assert_eq!(DJI_TAGS.get_tag_name(DJI_GIMBAL_PITCH), Some("GimbalPitch"));
    }

    #[test]
    fn test_format_degrees() {
        assert_eq!(format_degrees(0), "0°");
        assert_eq!(format_degrees(180), "180°");
        assert_eq!(format_degrees(359), "359°");
    }

    #[test]
    fn test_format_meters() {
        assert_eq!(format_meters(100), "100 m");
        assert_eq!(format_meters(0), "0 m");
    }

    #[test]
    fn test_format_battery_level() {
        assert_eq!(format_battery_level(100), "100%");
        assert_eq!(format_battery_level(50), "50%");
        assert_eq!(format_battery_level(0), "0%");
    }

    #[test]
    fn test_format_iso() {
        assert_eq!(format_iso(100), "100");
        assert_eq!(format_iso(3200), "3200");
        assert_eq!(format_iso(0), "Unknown");
        assert_eq!(format_iso(-1), "Unknown");
    }

    #[test]
    fn test_format_satellite_count() {
        assert_eq!(format_satellite_count(12), "12");
        assert_eq!(format_satellite_count(0), "0");
        assert_eq!(format_satellite_count(-1), "Unknown");
    }

    #[test]
    fn test_decode_hasselblad() {
        assert_eq!(decode_hasselblad(0), "No");
        assert_eq!(decode_hasselblad(1), "Yes");
        assert_eq!(decode_hasselblad(100), "Yes");
    }
}
