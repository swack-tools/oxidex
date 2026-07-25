//! Tag Value Conversion Utilities
//!
//! This module provides utilities for converting raw bytes from TIFF IFD entries
//! into strongly-typed TagValue instances. It handles all EXIF field types including
//! RATIONAL, SRATIONAL, SHORT, LONG, ASCII, and UNDEFINED.
//!
//! The conversion process includes:
//! - Type-specific handlers for each EXIF field type
//! - Special formatting for GPS coordinates, DateTime, and exposure settings
//! - Heuristic conversion for unknown or ambiguous types
//! - Utility functions for reading multi-byte values in different byte orders

use crate::core::TagValue;
use crate::core::operations_helpers::{
    gcd, is_datetime_string, is_printable_ascii, parse_exif_datetime, read_i32, read_u16, read_u32,
};
use crate::parsers::common::exif_types::ExifType;
use crate::parsers::tiff::ifd_parser::ByteOrder;

// ============================================================================
// PUBLIC API
// ============================================================================

/// Converts raw bytes from IFD to a TagValue.
///
/// This function interprets raw bytes according to the EXIF field type,
/// converting them to the appropriate TagValue variant.
///
/// # Arguments
///
/// * `bytes` - The raw bytes to convert
/// * `field_type` - The EXIF field type (from IFD entry)
/// * `value_count` - The number of values (from IFD entry)
/// * `tag_id` - The tag ID (for enum mapping and special handling)
/// * `byte_order` - The byte order for interpreting multi-byte values
///
/// # Returns
///
/// A TagValue representing the data
pub fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    // Try special tag handlers first (GPS_VERSION_ID, EXIF_VERSION, etc.)
    if let Some(value) = handle_special_byte_tags(tag_id, bytes) {
        return value;
    }

    // Try to convert field_type to ExifType
    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // RATIONAL (type 5): two 32-bit unsigned integers (numerator/denominator)
            ExifType::Rational if bytes.len() >= 8 => {
                return handle_rational_type(bytes, value_count, tag_id, byte_order);
            }

            // SRATIONAL (type 10): two 32-bit signed integers (numerator/denominator)
            ExifType::SRational if bytes.len() >= 8 => {
                return handle_srational_type(bytes, value_count, byte_order);
            }

            // SHORT (type 3): unsigned 16-bit integers
            ExifType::Short if bytes.len() >= 2 => {
                return handle_short_type(bytes, value_count, byte_order);
            }

            // LONG (type 4): unsigned 32-bit integers
            ExifType::Long if bytes.len() >= 4 => {
                return handle_long_type(bytes, value_count, byte_order);
            }

            // SLONG (type 9): signed 32-bit integers
            ExifType::SLong if bytes.len() >= 4 => {
                return handle_slong_type(bytes, value_count, byte_order);
            }

            // ASCII (type 2): null-terminated string
            ExifType::Ascii => {
                return handle_ascii_type(bytes);
            }

            // BYTE (type 1) and UNDEFINED (type 7): binary or heuristic conversion
            ExifType::Byte | ExifType::Undefined => {
                // For UNDEFINED type, if no specific handler matched, return binary
                if field_type == 7 {
                    return TagValue::new_binary(bytes.to_vec());
                }
                // Fall through to heuristic conversion for BYTE type
            }

            _ => {
                // Fall through to heuristic conversion below
            }
        }
    }

    // Fallback heuristic conversion for unknown types or when type-specific logic doesn't apply
    heuristic_bytes_to_tag_value(bytes, byte_order)
}

/// Parses a string value to an appropriate TagValue.
///
/// Attempts to parse as integer first, then float, otherwise returns as string.
/// Used for XMP and IPTC metadata parsing.
pub fn parse_string_to_tag_value(value: &str) -> TagValue {
    if let Ok(int_val) = value.parse::<i64>() {
        TagValue::Integer(int_val)
    } else if let Ok(float_val) = value.parse::<f64>() {
        TagValue::Float(float_val)
    } else {
        TagValue::String(value.to_string())
    }
}

// ============================================================================
// SPECIAL TAG HANDLERS
// ============================================================================

/// Handles special byte-encoded tags that need custom formatting.
fn handle_special_byte_tags(tag_id: u16, bytes: &[u8]) -> Option<TagValue> {
    // Tag ID constants
    const GPS_VERSION_ID: u16 = 0x0000;
    const EXIF_VERSION: u16 = 0x9000;
    const COMPONENTS_CONFIGURATION: u16 = 0x9101;

    match tag_id {
        // GPS Version ID (4 bytes: major.minor.rev.0)
        GPS_VERSION_ID if bytes.len() >= 4 => Some(TagValue::new_string(format!(
            "{}.{}.{}.{}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        ))),

        // Exif Version (4 bytes: ASCII "0232")
        EXIF_VERSION if bytes.len() >= 4 => {
            let version = String::from_utf8_lossy(&bytes[0..4]);
            Some(TagValue::new_string(version.to_string()))
        }

        // ComponentsConfiguration (4 bytes with component IDs)
        COMPONENTS_CONFIGURATION if bytes.len() >= 4 => {
            let component_names = bytes
                .iter()
                .take(4)
                .map(|&b| match b {
                    0 => "-",
                    1 => "Y",
                    2 => "Cb",
                    3 => "Cr",
                    4 => "R",
                    5 => "G",
                    6 => "B",
                    _ => "?",
                })
                .collect::<Vec<_>>();
            Some(TagValue::new_string(component_names.join(", ")))
        }

        _ => None,
    }
}

// ============================================================================
// TYPE-SPECIFIC HANDLERS
// ============================================================================

/// Handles RATIONAL type fields (type 5).
fn handle_rational_type(
    bytes: &[u8],
    value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    // GPS coordinate tags (3 rationals: degrees, minutes, seconds)
    const GPS_LATITUDE: u16 = 0x0002;
    const GPS_LONGITUDE: u16 = 0x0004;
    const GPS_DEST_LATITUDE: u16 = 0x0014;
    const GPS_DEST_LONGITUDE: u16 = 0x0016;
    const GPS_ALTITUDE: u16 = 0x0006;

    // GPS timestamp (3 rationals: hours, minutes, seconds)
    const GPS_TIMESTAMP: u16 = 0x0007;

    // GPS movement and tracking tags (single rational)
    const GPS_SPEED: u16 = 0x000D;
    const GPS_TRACK: u16 = 0x000F;
    const GPS_IMG_DIRECTION: u16 = 0x0011;
    const GPS_DEST_BEARING: u16 = 0x0018;
    const GPS_DEST_DISTANCE: u16 = 0x001A;
    const GPS_H_POSITIONING_ERROR: u16 = 0x001F;

    const EXPOSURE_TIME: u16 = 0x829A;
    const LENS_INFO: u16 = 0xA432; // LensInfo tag (4 rationals)

    // Check if this is an array of rationals (count > 1)
    if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
        // Special handling for GPS coordinates (3 rationals: degrees, minutes, seconds)
        if matches!(
            tag_id,
            GPS_LATITUDE | GPS_LONGITUDE | GPS_DEST_LATITUDE | GPS_DEST_LONGITUDE
        ) && value_count == 3
        {
            return format_gps_coordinate(bytes, byte_order);
        }

        // Special handling for LensInfo (4 rationals: min_focal, max_focal, min_aperture_min, min_aperture_max)
        if tag_id == LENS_INFO && value_count == 4 {
            return format_lens_info(bytes, byte_order);
        }

        // Special handling for GPSTimeStamp (3 rationals: hours, minutes, seconds)
        // ExifTool formats this as "HH:MM:SS" (e.g., "15:38:33")
        if tag_id == GPS_TIMESTAMP && value_count == 3 {
            return format_gps_timestamp(bytes, byte_order);
        }

        // Parse array of rationals and format as space-separated decimals
        return parse_rational_array(bytes, value_count, byte_order);
    }

    // Single rational value - parse numerator and denominator
    let numerator = read_u32(&bytes[0..4], byte_order);
    let denominator = read_u32(&bytes[4..8], byte_order);

    // Special handling for GPS Altitude
    if tag_id == GPS_ALTITUDE && denominator != 0 {
        let value = numerator as f64 / denominator as f64;
        if value.fract() == 0.0 {
            return TagValue::new_string(format!("{} m", value as i32));
        } else {
            return TagValue::new_string(format!("{:.1} m", value));
        }
    }

    // Special handling for GPS movement tags
    // ExifTool displays whole numbers without decimals ("20" not "20.00")
    if denominator != 0 {
        let value = numerator as f64 / denominator as f64;

        match tag_id {
            // GPSSpeed - format with precision, no unit (unit is in GPSSpeedRef)
            GPS_SPEED => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSTrack - direction in degrees (0-359.99)
            GPS_TRACK => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSImgDirection - camera pointing direction in degrees (0-359.99)
            GPS_IMG_DIRECTION => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSDestBearing - bearing to destination in degrees (0-359.99)
            GPS_DEST_BEARING => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSDestDistance - distance to destination (unit in GPSDestDistanceRef)
            GPS_DEST_DISTANCE => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSHPositioningError - horizontal positioning error in meters
            GPS_H_POSITIONING_ERROR => {
                return TagValue::new_string(format!("{} m", format_gps_numeric_value(value)));
            }
            _ => {}
        }
    }

    // Special handling for ExposureTime - format as fraction string
    // Match ExifTool's behavior: 1/N format for exposures < 1 second, decimal for >= 1 second
    if tag_id == EXPOSURE_TIME && denominator != 0 {
        let val = numerator as f64 / denominator as f64;
        if val >= 1.0 {
            // Show as decimal for exposure >= 1 second (e.g., "2.0")
            return TagValue::new_string(format!("{:.1}", val));
        } else {
            // For exposures < 1 second, try to simplify first
            let gcd_value = gcd(numerator, denominator);
            let simplified_num = numerator / gcd_value;
            let simplified_den = denominator / gcd_value;
            if simplified_num == 1 {
                // Already in 1/N form after simplification
                return TagValue::new_string(format!("1/{}", simplified_den));
            } else {
                // Approximate to 1/N form like ExifTool does
                let approx_denom = (1.0 / val).round() as u64;
                return TagValue::new_string(format!("1/{}", approx_denom));
            }
        }
    }

    TagValue::new_rational(numerator as i32, denominator as i32)
}

/// Formats a GPS coordinate from 3 rational values.
fn format_gps_coordinate(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    let mut dms = Vec::new();
    for i in 0..3 {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            dms.push(numerator as f64 / denominator as f64);
        } else {
            dms.push(numerator as f64);
        }
    }
    // Format seconds with up to 9 decimal places, trim trailing zeros for ExifTool compat
    let sec_str = format!("{:.9}", dms[2]);
    let sec_trimmed = sec_str.trim_end_matches('0').trim_end_matches('.');
    let formatted = format!("{} deg {}' {}\"", dms[0] as i32, dms[1] as i32, sec_trimmed);
    TagValue::new_string(formatted)
}

/// Formats GPSTimeStamp from 3 rational values (hours, minutes, seconds).
///
/// ExifTool formats GPSTimeStamp as "HH:MM:SS" (e.g., "15:38:33").
/// The GPS timestamp represents UTC time.
///
/// # Format Rules
///
/// - Hours and minutes are zero-padded to 2 digits (e.g., "08:05:03")
/// - Seconds are displayed without decimal places if they are whole numbers
/// - Fractional seconds are displayed with appropriate precision (e.g., "15:38:33.5")
fn format_gps_timestamp(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    let mut hms = Vec::new();
    for i in 0..3 {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            hms.push(numerator as f64 / denominator as f64);
        } else {
            hms.push(numerator as f64);
        }
    }

    let hours = hms[0] as u32;
    let minutes = hms[1] as u32;
    let seconds = hms[2];

    // Format seconds: no decimal places if whole number, otherwise show fractional part
    let formatted = if seconds.fract() == 0.0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds as u32)
    } else {
        // Trim trailing zeros from fractional seconds - use 9 decimals for ExifTool compat
        let sec_str = format!("{:.9}", seconds);
        let sec_str = sec_str.trim_end_matches('0').trim_end_matches('.');
        format!("{:02}:{:02}:{}", hours, minutes, sec_str)
    };

    TagValue::new_string(formatted)
}

/// Formats LensInfo from 4 rational values (min_focal, max_focal, min_f_at_min, min_f_at_max).
///
/// LensInfo contains:
/// - [0] = Minimum focal length (mm)
/// - [1] = Maximum focal length (mm)
/// - [2] = Minimum F-number at minimum focal length
/// - [3] = Minimum F-number at maximum focal length
///
/// Formatted as: "focal_min-focal_maxmm f/aperture_min-aperture_max"
/// Example: "24-70mm f/2.8-2.8" or "50mm f/1.8" or "3.99mm f/1.8"
///
/// # Formatting Rules (ExifTool compatibility)
///
/// - Focal lengths preserve decimal precision when present (e.g., "3.99mm")
/// - Whole numbers display without decimals (e.g., "24mm" not "24.0mm")
/// - No space between number and "mm" (e.g., "3.99mm" not "3.99 mm")
fn format_lens_info(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    let mut values = Vec::new();
    for i in 0..4 {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            values.push(numerator as f64 / denominator as f64);
        } else {
            values.push(0.0);
        }
    }

    let min_focal = values[0];
    let max_focal = values[1];
    let min_f_at_min = values[2];
    let min_f_at_max = values[3];

    /// Helper to format focal length with appropriate precision.
    /// Whole numbers display without decimals, fractional values preserve precision.
    /// Uses up to 2 decimal places, trimming trailing zeros.
    fn format_focal(f: f64) -> String {
        // Format with 2 decimal places then trim trailing zeros
        let formatted = format!("{:.2}", f);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    }

    // Format focal length range (no space before "mm")
    let focal_str = if (min_focal - max_focal).abs() < 0.01 {
        // Prime lens (single focal length)
        format!("{}mm", format_focal(min_focal))
    } else {
        // Zoom lens (focal range)
        format!("{}-{}mm", format_focal(min_focal), format_focal(max_focal))
    };

    // Format aperture range - keep one decimal for f-numbers, trim if whole
    let format_aperture = |f: f64| -> String {
        if f.fract().abs() < 0.001 {
            format!("{:.0}", f)
        } else {
            // Format with 1 decimal, trim trailing zeros
            format!("{:.1}", f)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
    };

    let aperture_str = if (min_f_at_min - min_f_at_max).abs() < 0.01 {
        // Constant aperture (e.g., f/2.8 or f/4)
        format!("f/{}", format_aperture(min_f_at_min))
    } else {
        // Variable aperture (e.g., f/3.5-5.6)
        format!(
            "f/{}-{}",
            format_aperture(min_f_at_min),
            format_aperture(min_f_at_max)
        )
    };

    let formatted = format!("{} {}", focal_str, aperture_str);
    TagValue::new_string(formatted)
}

/// Parses an array of rational values into a space-separated string.
fn parse_rational_array(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    let mut values = Vec::new();
    for i in 0..value_count as usize {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            values.push(numerator as f64 / denominator as f64);
        } else {
            values.push(numerator as f64);
        }
    }
    let formatted = values
        .iter()
        .map(|v| format!("{:.10}", v))
        .collect::<Vec<_>>()
        .join(" ");
    TagValue::new_string(formatted)
}

/// Handles SRATIONAL type fields (type 10).
fn handle_srational_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
        return parse_srational_array(bytes, value_count, byte_order);
    }

    let numerator = read_i32(&bytes[0..4], byte_order);
    let denominator = read_i32(&bytes[4..8], byte_order);
    TagValue::new_rational(numerator, denominator)
}

/// Parses an array of signed rational values.
fn parse_srational_array(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    let mut values = Vec::new();
    for i in 0..value_count as usize {
        let offset = i * 8;
        let numerator = read_i32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_i32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            values.push(numerator as f64 / denominator as f64);
        } else {
            values.push(numerator as f64);
        }
    }
    let formatted = values
        .iter()
        .map(|v| format!("{:.10}", v))
        .collect::<Vec<_>>()
        .join(" ");
    TagValue::new_string(formatted)
}

/// Handles SHORT type fields (type 3).
fn handle_short_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 2) {
        let mut values = Vec::new();
        for i in 0..value_count as usize {
            let offset = i * 2;
            let value = read_u16(&bytes[offset..offset + 2], byte_order);
            values.push(value.to_string());
        }
        return TagValue::new_string(values.join(" "));
    }

    let value = read_u16(&bytes[0..2], byte_order) as i64;
    TagValue::new_integer(value)
}

/// Handles LONG type fields (type 4).
fn handle_long_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
        let mut values = Vec::new();
        for i in 0..value_count as usize {
            let offset = i * 4;
            let value = read_u32(&bytes[offset..offset + 4], byte_order);
            values.push(value.to_string());
        }
        return TagValue::new_string(values.join(" "));
    }

    let value = read_u32(&bytes[0..4], byte_order) as i64;
    TagValue::new_integer(value)
}

/// Handles SLONG type fields (type 9) - signed 32-bit integers.
fn handle_slong_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
        let mut values = Vec::new();
        for i in 0..value_count as usize {
            let offset = i * 4;
            let value = read_i32(&bytes[offset..offset + 4], byte_order);
            values.push(value.to_string());
        }
        return TagValue::new_string(values.join(" "));
    }

    let value = read_i32(&bytes[0..4], byte_order) as i64;
    TagValue::new_integer(value)
}

/// Handles ASCII type fields (type 2).
fn handle_ascii_type(bytes: &[u8]) -> TagValue {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim_end_matches('\0');
    if !s.is_empty() {
        if is_datetime_string(s)
            && let Ok(dt) = parse_exif_datetime(s)
        {
            return TagValue::DateTime(dt);
        }
        return TagValue::new_string(s.to_string());
    }
    TagValue::new_string(String::new())
}

/// Applies heuristic conversion for unknown or ambiguous byte sequences.
fn heuristic_bytes_to_tag_value(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    if bytes.len() == 2 {
        let value = read_u16(bytes, byte_order) as i64;
        return TagValue::new_integer(value);
    } else if bytes.len() == 4 {
        let null_count = bytes.iter().filter(|&&b| b == 0).count();
        let has_printable = bytes.iter().any(|&b| (32..=126).contains(&b));

        // If multiple nulls or no printable chars, treat as integer using the
        // actual byte order from the TIFF file (not hardcoded little-endian)
        if null_count > 1 || !has_printable {
            let value = read_u32(bytes, byte_order) as i64;
            return TagValue::new_integer(value);
        }

        if bytes.iter().all(|&b| (32..=126).contains(&b) || b == 0) {
            let s = String::from_utf8_lossy(bytes);
            let s = s.trim_end_matches('\0');
            if !s.is_empty() && s.len() >= 3 {
                return TagValue::new_string(s.to_string());
            }
        }

        let value = read_u32(bytes, byte_order) as i64;
        return TagValue::new_integer(value);
    }

    if is_printable_ascii(bytes) {
        let s = String::from_utf8_lossy(bytes);
        let s = s.trim_end_matches('\0');
        if !s.is_empty() {
            if is_datetime_string(s)
                && let Ok(dt) = parse_exif_datetime(s)
            {
                return TagValue::DateTime(dt);
            }
            return TagValue::new_string(s.to_string());
        }
    }

    TagValue::new_binary(bytes.to_vec())
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================
// Note: Utility functions (read_u16, read_u32, read_i32, is_datetime_string,
// parse_exif_datetime, gcd) are imported from operations_helpers module
// to avoid duplication.

/// Formats a GPS numeric value for ExifTool compatibility.
///
/// GPS values like GPSImgDirection, GPSSpeed, GPSTrack, GPSDestBearing, and
/// GPSDestDistance are formatted to match ExifTool's output:
/// - Whole numbers display without decimals: "20" not "20.00"
/// - Fractional values display with minimal precision (trailing zeros trimmed)
///
/// # Arguments
///
/// * `value` - The floating-point GPS value to format
///
/// # Returns
///
/// A string formatted to match ExifTool's GPS numeric output.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(format_gps_numeric_value(20.0), "20");
/// assert_eq!(format_gps_numeric_value(45.5), "45.5");
/// assert_eq!(format_gps_numeric_value(123.456), "123.456");
/// ```
fn format_gps_numeric_value(value: f64) -> String {
    // Use a small epsilon to detect near-integer values
    // This handles floating-point representation issues
    const EPSILON: f64 = 1e-9;

    if (value.fract().abs()) < EPSILON {
        // Whole number - format without decimals
        format!("{:.0}", value)
    } else {
        // Fractional value - format with up to 9 decimal places and trim trailing zeros
        // ExifTool uses 9+ decimal precision for GPS values
        let formatted = format!("{:.9}", value);
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::tiff::ifd_parser::ByteOrder;

    /// Helper function to create RATIONAL bytes (numerator/denominator)
    fn make_rational_bytes(numerator: u32, denominator: u32, byte_order: ByteOrder) -> Vec<u8> {
        let mut bytes = Vec::new();
        match byte_order {
            ByteOrder::LittleEndian => {
                bytes.extend_from_slice(&numerator.to_le_bytes());
                bytes.extend_from_slice(&denominator.to_le_bytes());
            }
            ByteOrder::BigEndian => {
                bytes.extend_from_slice(&numerator.to_be_bytes());
                bytes.extend_from_slice(&denominator.to_be_bytes());
            }
        }
        bytes
    }

    #[test]
    fn test_gps_speed_formatting() {
        // Test GPSSpeed (tag 0x000D) - whole numbers without decimals (ExifTool compatible)
        let bytes = make_rational_bytes(25, 1, ByteOrder::BigEndian); // 25
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000D, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "25"); // Not "25.00"
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test with fractional value
        let bytes = make_rational_bytes(1234, 100, ByteOrder::BigEndian); // 12.34
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000D, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "12.34");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_speed_ref_formatting() {
        // Test GPSSpeedRef (tag 0x000C) - should be ASCII string (K, M, or N)
        let bytes = b"K\0"; // km/h
        let result = raw_bytes_to_tag_value(bytes, 2, 2, 0x000C, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "K");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_track_formatting() {
        // Test GPSTrack (tag 0x000F) - direction in degrees (0-359.99)
        let bytes = make_rational_bytes(27512, 100, ByteOrder::BigEndian); // 275.12 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000F, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "275.12");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test with integer degrees - whole numbers without decimals
        let bytes = make_rational_bytes(90, 1, ByteOrder::BigEndian); // 90 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000F, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "90"); // Not "90.00"
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_track_ref_formatting() {
        // Test GPSTrackRef (tag 0x000E) - should be ASCII string (T or M)
        let bytes = b"T\0"; // True north
        let result = raw_bytes_to_tag_value(bytes, 2, 2, 0x000E, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "T");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_img_direction_formatting() {
        // Test GPSImgDirection (tag 0x0011) - camera pointing direction
        let bytes = make_rational_bytes(18050, 100, ByteOrder::LittleEndian); // 180.50 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x0011, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "180.5"); // Trailing zero trimmed
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test whole number - no decimals
        let bytes = make_rational_bytes(20, 1, ByteOrder::LittleEndian); // 20 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x0011, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "20"); // Not "20.00"
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_img_direction_ref_formatting() {
        // Test GPSImgDirectionRef (tag 0x0010) - should be ASCII string (T or M)
        let bytes = b"M\0"; // Magnetic north
        let result = raw_bytes_to_tag_value(bytes, 2, 2, 0x0010, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "M");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_dest_bearing_formatting() {
        // Test GPSDestBearing (tag 0x0018) - bearing to destination
        let bytes = make_rational_bytes(4525, 100, ByteOrder::BigEndian); // 45.25 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x0018, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "45.25");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_dest_distance_formatting() {
        // Test GPSDestDistance (tag 0x001A) - distance to destination
        let bytes = make_rational_bytes(12345, 1000, ByteOrder::LittleEndian); // 12.345
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x001A, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "12.345");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_h_positioning_error_formatting() {
        // Test GPSHPositioningError (tag 0x001F) - horizontal positioning error in meters
        let bytes = make_rational_bytes(525, 100, ByteOrder::BigEndian); // 5.25 m
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x001F, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "5.25 m");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test with integer value - whole numbers without decimals
        let bytes = make_rational_bytes(10, 1, ByteOrder::BigEndian); // 10 m
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x001F, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "10 m"); // Not "10.00 m"
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_movement_tags_with_zero_denominator() {
        // Test that zero denominators don't cause division by zero
        let bytes = make_rational_bytes(100, 0, ByteOrder::BigEndian);

        // Should fall through to rational representation
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000D, ByteOrder::BigEndian);

        if let TagValue::Rational {
            numerator,
            denominator,
        } = result
        {
            assert_eq!(numerator, 100);
            assert_eq!(denominator, 0);
        } else {
            panic!(
                "Expected Rational variant for zero denominator, got {:?}",
                result
            );
        }
    }

    #[test]
    fn test_gps_movement_tags_forensic_scenario() {
        // Forensic scenario: Vehicle moving at 55.5 km/h, heading 275.5 degrees

        // GPSSpeed: 55.5
        let speed_bytes = make_rational_bytes(555, 10, ByteOrder::BigEndian);
        let speed = raw_bytes_to_tag_value(&speed_bytes, 5, 1, 0x000D, ByteOrder::BigEndian);
        if let TagValue::String(s) = speed {
            assert_eq!(s, "55.5"); // Trailing zero trimmed
        } else {
            panic!("Expected String for GPSSpeed");
        }

        // GPSSpeedRef: K (km/h)
        let speed_ref = raw_bytes_to_tag_value(b"K\0", 2, 2, 0x000C, ByteOrder::BigEndian);
        if let TagValue::String(s) = speed_ref {
            assert_eq!(s, "K");
        } else {
            panic!("Expected String for GPSSpeedRef");
        }

        // GPSTrack: 275.5 degrees
        let track_bytes = make_rational_bytes(2755, 10, ByteOrder::BigEndian);
        let track = raw_bytes_to_tag_value(&track_bytes, 5, 1, 0x000F, ByteOrder::BigEndian);
        if let TagValue::String(s) = track {
            assert_eq!(s, "275.5"); // Trailing zero trimmed
        } else {
            panic!("Expected String for GPSTrack");
        }

        // GPSTrackRef: T (true north)
        let track_ref = raw_bytes_to_tag_value(b"T\0", 2, 2, 0x000E, ByteOrder::BigEndian);
        if let TagValue::String(s) = track_ref {
            assert_eq!(s, "T");
        } else {
            panic!("Expected String for GPSTrackRef");
        }

        // GPSImgDirection: 90.25 degrees (camera pointing east)
        let img_dir_bytes = make_rational_bytes(9025, 100, ByteOrder::BigEndian);
        let img_dir = raw_bytes_to_tag_value(&img_dir_bytes, 5, 1, 0x0011, ByteOrder::BigEndian);
        if let TagValue::String(s) = img_dir {
            assert_eq!(s, "90.25");
        } else {
            panic!("Expected String for GPSImgDirection");
        }

        // GPSHPositioningError: 8.5 m
        let error_bytes = make_rational_bytes(85, 10, ByteOrder::BigEndian);
        let error = raw_bytes_to_tag_value(&error_bytes, 5, 1, 0x001F, ByteOrder::BigEndian);
        if let TagValue::String(s) = error {
            assert_eq!(s, "8.5 m"); // Trailing zero trimmed
        } else {
            panic!("Expected String for GPSHPositioningError");
        }
    }

    // ============================================================================
    // GPS TIMESTAMP TESTS
    // ============================================================================

    /// Helper function to create rational bytes array (for LensInfo, GPSTimeStamp, etc.)
    fn make_rational_array_bytes(values: &[(u32, u32)], byte_order: ByteOrder) -> Vec<u8> {
        let mut bytes = Vec::new();
        for &(num, den) in values {
            match byte_order {
                ByteOrder::LittleEndian => {
                    bytes.extend_from_slice(&num.to_le_bytes());
                    bytes.extend_from_slice(&den.to_le_bytes());
                }
                ByteOrder::BigEndian => {
                    bytes.extend_from_slice(&num.to_be_bytes());
                    bytes.extend_from_slice(&den.to_be_bytes());
                }
            }
        }
        bytes
    }

    #[test]
    fn test_gps_timestamp_basic() {
        // Test GPSTimeStamp (tag 0x0007) - should format as "HH:MM:SS"
        // Input: 15 hours, 38 minutes, 33 seconds
        let bytes = make_rational_array_bytes(&[(15, 1), (38, 1), (33, 1)], ByteOrder::BigEndian);

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "15:38:33");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_timestamp_zero_padded() {
        // Test zero-padding for hours/minutes/seconds
        // Input: 8 hours, 5 minutes, 3 seconds -> "08:05:03"
        let bytes = make_rational_array_bytes(&[(8, 1), (5, 1), (3, 1)], ByteOrder::LittleEndian);

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "08:05:03");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_timestamp_fractional_seconds() {
        // Test fractional seconds (e.g., 33.5 seconds)
        // Input: 15 hours, 38 minutes, 33.5 seconds
        let bytes = make_rational_array_bytes(
            &[(15, 1), (38, 1), (67, 2)], // 67/2 = 33.5
            ByteOrder::BigEndian,
        );

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "15:38:33.5");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_timestamp_midnight() {
        // Test midnight (00:00:00)
        let bytes = make_rational_array_bytes(&[(0, 1), (0, 1), (0, 1)], ByteOrder::LittleEndian);

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "00:00:00");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_timestamp_end_of_day() {
        // Test 23:59:59
        let bytes = make_rational_array_bytes(&[(23, 1), (59, 1), (59, 1)], ByteOrder::BigEndian);

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "23:59:59");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    // ============================================================================
    // DEVICE IDENTIFICATION TESTS - For Forensic Device Attribution
    // ============================================================================

    #[test]
    fn test_lens_info_prime_lens() {
        // 50mm f/1.8 prime lens - common forensic scenario
        let bytes = make_rational_array_bytes(
            &[(50, 1), (50, 1), (18, 10), (18, 10)],
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm
            assert_eq!(s, "50mm f/1.8");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_zoom_constant_aperture() {
        // 24-70mm f/2.8 zoom lens with constant aperture - professional camera
        let bytes = make_rational_array_bytes(
            &[(24, 1), (70, 1), (28, 10), (28, 10)],
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm, no trailing .0
            assert_eq!(s, "24-70mm f/2.8");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_zoom_variable_aperture() {
        // 18-55mm f/3.5-5.6 zoom lens - common kit lens
        let bytes = make_rational_array_bytes(
            &[(18, 1), (55, 1), (35, 10), (56, 10)],
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm
            assert_eq!(s, "18-55mm f/3.5-5.6");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_big_endian() {
        // 85mm f/1.4 prime lens with big-endian byte order
        let bytes = make_rational_array_bytes(
            &[(85, 1), (85, 1), (14, 10), (14, 10)],
            ByteOrder::BigEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::BigEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm
            assert_eq!(s, "85mm f/1.4");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_telephoto() {
        // 70-200mm f/4 telephoto zoom lens
        let bytes = make_rational_array_bytes(
            &[(70, 1), (200, 1), (40, 10), (40, 10)],
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm, no trailing .0
            assert_eq!(s, "70-200mm f/4");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_fractional_focal_rounding() {
        // Test focal length rounding: 3.99mm should round to 4mm
        // This matches ExifTool's behavior for smartphone lenses
        let bytes = make_rational_array_bytes(
            &[(399, 100), (399, 100), (18, 10), (18, 10)], // 3.99mm f/1.8
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool keeps exact value (3.99), no rounding. Format: no space before mm
            assert_eq!(s, "3.99mm f/1.8");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test a truly fractional value that shouldn't round
        let bytes2 = make_rational_array_bytes(
            &[(45, 10), (45, 10), (28, 10), (28, 10)], // 4.5mm f/2.8
            ByteOrder::LittleEndian,
        );

        let result2 = raw_bytes_to_tag_value(
            &bytes2,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result2 {
            // 4.5 stays as 4.5 (not rounded), ExifTool format: no space before mm
            assert_eq!(s, "4.5mm f/2.8");
        } else {
            panic!("Expected String variant, got {:?}", result2);
        }
    }

    #[test]
    fn test_image_unique_id() {
        // ImageUniqueID is a 32-character hex string for unique image identification
        let unique_id = b"0123456789ABCDEF0123456789ABCDEF\0";

        let result = raw_bytes_to_tag_value(
            unique_id,
            2, // ExifType::Ascii
            33,
            0xA420, // ImageUniqueID
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "0123456789ABCDEF0123456789ABCDEF");
            assert_eq!(s.len(), 32, "ImageUniqueID should be 32 characters");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_owner_name() {
        // OwnerName - camera owner for attribution
        let owner_name = b"John Doe\0";

        let result = raw_bytes_to_tag_value(
            owner_name,
            2, // ExifType::Ascii
            9,
            0xA430, // OwnerName
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "John Doe");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_body_serial_number() {
        // BodySerialNumber (tag 0xA431) - camera body serial for forensic attribution
        let serial = b"1234567890\0";

        let result = raw_bytes_to_tag_value(
            serial,
            2, // ExifType::Ascii
            11,
            0xA431, // SerialNumber (BodySerialNumber)
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "1234567890");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_make() {
        // LensMake - lens manufacturer
        let lens_make = b"Canon\0";

        let result = raw_bytes_to_tag_value(
            lens_make,
            2, // ExifType::Ascii
            6,
            0xA433, // LensMake
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "Canon");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_model() {
        // LensModel - specific lens model name
        let lens_model = b"EF 50mm f/1.8 STM\0";

        let result = raw_bytes_to_tag_value(
            lens_model,
            2, // ExifType::Ascii
            18,
            0xA434, // LensModel
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "EF 50mm f/1.8 STM");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_serial_number() {
        // LensSerialNumber - lens serial for unique identification
        let lens_serial = b"ABC123456\0";

        let result = raw_bytes_to_tag_value(
            lens_serial,
            2, // ExifType::Ascii
            10,
            0xA435, // LensSerialNumber
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "ABC123456");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_camera_serial_number_dng() {
        // CameraSerialNumber (DNG/Adobe tag 0xC62F)
        let camera_serial = b"DNG9876543210\0";

        let result = raw_bytes_to_tag_value(
            camera_serial,
            2, // ExifType::Ascii
            14,
            0xC62F, // CameraSerialNumber (DNG)
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "DNG9876543210");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_forensic_device_attribution_scenario() {
        // Complete forensic scenario: Camera + Lens identification

        // Camera body serial number
        let body_serial = b"CN123456789\0";
        let body_result =
            raw_bytes_to_tag_value(body_serial, 2, 12, 0xA431, ByteOrder::LittleEndian);
        assert!(matches!(body_result, TagValue::String(ref s) if s == "CN123456789"));

        // Lens serial number
        let lens_serial = b"LS987654321\0";
        let lens_result =
            raw_bytes_to_tag_value(lens_serial, 2, 12, 0xA435, ByteOrder::LittleEndian);
        assert!(matches!(lens_result, TagValue::String(ref s) if s == "LS987654321"));

        // Lens info: 24-70mm f/2.8 (ExifTool format: no space before mm)
        let lens_info_bytes = make_rational_array_bytes(
            &[(24, 1), (70, 1), (28, 10), (28, 10)],
            ByteOrder::LittleEndian,
        );
        let lens_info_result =
            raw_bytes_to_tag_value(&lens_info_bytes, 5, 4, 0xA432, ByteOrder::LittleEndian);
        assert!(matches!(lens_info_result, TagValue::String(ref s) if s == "24-70mm f/2.8"));

        // Owner name
        let owner = b"Evidence Photographer\0";
        let owner_result = raw_bytes_to_tag_value(owner, 2, 22, 0xA430, ByteOrder::LittleEndian);
        assert!(matches!(owner_result, TagValue::String(ref s) if s == "Evidence Photographer"));

        // Image unique ID
        let unique_id = b"ABCDEF0123456789FEDCBA9876543210\0";
        let id_result = raw_bytes_to_tag_value(unique_id, 2, 33, 0xA420, ByteOrder::LittleEndian);
        assert!(
            matches!(id_result, TagValue::String(ref s) if s == "ABCDEF0123456789FEDCBA9876543210")
        );
    }
}
