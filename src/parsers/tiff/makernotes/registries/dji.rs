//! DJI drone tag registry
//!
//! Provides TagRegistry definitions for DJI drones including Mavic, Phantom,
//! Inspire, and Osmo series. Contains decoders for flight telemetry, GPS data,
//! gimbal orientation, and camera settings.

use super::super::shared::{generic_decoders::YES_NO, tag_registry::TagRegistry};
use crate::{bitfield_decoder, const_decoder};

// Re-export tag constants from dji.rs for use in the registry
use super::super::dji::{
    DJI_AIRCRAFT_PITCH, DJI_AIRCRAFT_ROLL, DJI_AIRCRAFT_YAW, DJI_BATTERY_LEVEL,
    DJI_BATTERY_VOLTAGE, DJI_CAMERA_APERTURE, DJI_CAMERA_EV, DJI_CAMERA_ISO, DJI_CAMERA_SHUTTER,
    DJI_CAMERA_WB, DJI_COLOR_MODE, DJI_FLIGHT_DIRECTION, DJI_FLIGHT_MODE, DJI_FLIGHT_SPEED,
    DJI_FLIGHT_TIME, DJI_GIMBAL_PITCH, DJI_GIMBAL_ROLL, DJI_GIMBAL_YAW, DJI_GPS_ALTITUDE,
    DJI_GPS_LATITUDE, DJI_GPS_LONGITUDE, DJI_GPS_SIGNAL, DJI_HASSELBLAD, DJI_HOME_DISTANCE,
    DJI_IMAGE_FORMAT, DJI_OBSTACLE_AVOID, DJI_RELATIVE_ALTITUDE, DJI_SATELLITE_COUNT,
};

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Decoder for DJI flight mode codes
// Maps flight mode numeric codes to human-readable mode names.
// Covers all common flight modes from Manual to Force Landing.
const_decoder!(
    pub FLIGHT_MODE,
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
const_decoder!(
    pub WHITE_BALANCE,
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
const_decoder!(
    pub COLOR_MODE,
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
const_decoder!(
    pub IMAGE_FORMAT,
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
const_decoder!(
    pub GPS_SIGNAL,
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
//
// Bitmask layout:
// - 0x01: Front sensor
// - 0x02: Back sensor
// - 0x04: Left sensor
// - 0x08: Right sensor
// - 0x10: Top sensor
// - 0x20: Bottom sensor
bitfield_decoder!(
    pub OBSTACLE_AVOIDANCE,
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
pub fn format_gps_coordinate(value: i32) -> String {
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
pub fn format_altitude(value: i32) -> String {
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
pub fn format_speed(value: i16) -> String {
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
pub fn format_gimbal_angle(value: i16) -> String {
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
pub fn format_voltage(value: i16) -> String {
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
pub fn format_shutter_speed(value: i16) -> String {
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
pub fn format_aperture(value: i16) -> String {
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
pub fn format_ev(value: i16) -> String {
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
pub fn format_flight_time(value: i16) -> String {
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
pub fn decode_obstacle_avoidance(value: i16) -> String {
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
pub fn format_degrees(value: i16) -> String {
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
pub fn format_meters(value: i16) -> String {
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
pub fn format_battery_level(value: i16) -> String {
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
pub fn format_iso(value: i16) -> String {
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
pub fn format_satellite_count(value: i16) -> String {
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
pub fn decode_hasselblad(value: i16) -> String {
    YES_NO.decode(if value != 0 { 1 } else { 0 })
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Creates DJI tag registry with all tag definitions
///
/// This registry maps tag IDs to their names and decoders, eliminating
/// the need for large match statements with repetitive code.
/// All tags are registered and accessed via O(1) HashMap lookups.
///
/// The registry handles three categories of DJI tags:
/// 1. i32 tags (GPS coordinates, altitudes) - register_i32()
/// 2. i16 tags with custom formatting (gimbal angles, speeds, etc.) - register_i16()
/// 3. i16 tags with simple decoders (flight mode, GPS signal, etc.) - register_simple_i16()
pub fn dji_registry() -> TagRegistry {
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
        .register_i16(DJI_AIRCRAFT_PITCH, "AircraftPitch", format_gimbal_angle)
        .register_i16(DJI_AIRCRAFT_ROLL, "AircraftRoll", format_gimbal_angle)
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
}
