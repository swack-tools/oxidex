//! Helper utilities for operations.rs
//!
//! This module contains extracted helper functions to reduce complexity in operations.rs.
//! These helpers are organized by functionality:
//! - Byte order reading utilities
//! - DateTime parsing
//! - String parsing
//! - Mathematical utilities
//! - Tag value type conversion

use crate::core::TagValue;
use crate::error::{ExifToolError, Result};
use crate::io::{ByteOrder as IoByteOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use chrono;

// ============================================================================
// BYTE ORDER UTILITIES
// ============================================================================

/// Converts from TIFF ByteOrder to IO ByteOrder.
///
/// This helper bridges the two ByteOrder enums used in the codebase:
/// - `crate::parsers::tiff::ifd_parser::ByteOrder` (LittleEndian/BigEndian)
/// - `crate::io::ByteOrder` (Little/Big)
#[inline]
fn to_io_byte_order(byte_order: ByteOrder) -> IoByteOrder {
    match byte_order {
        ByteOrder::LittleEndian => IoByteOrder::Little,
        ByteOrder::BigEndian => IoByteOrder::Big,
    }
}

/// Reads an unsigned 16-bit integer from bytes with the specified byte order.
///
/// Uses `EndianReader` for consistent byte order handling across the codebase.
///
/// # Arguments
///
/// * `bytes` - Byte slice (must be at least 2 bytes)
/// * `byte_order` - Byte order for interpretation
///
/// # Returns
///
/// The u16 value (returns 0 if bytes are too short)
#[inline]
pub fn read_u16(bytes: &[u8], byte_order: ByteOrder) -> u16 {
    let reader = EndianReader::new(bytes, to_io_byte_order(byte_order));
    reader.u16_at(0).unwrap_or(0)
}

/// Reads an unsigned 32-bit integer from bytes with the specified byte order.
///
/// Uses `EndianReader` for consistent byte order handling across the codebase.
///
/// # Arguments
///
/// * `bytes` - Byte slice (must be at least 4 bytes)
/// * `byte_order` - Byte order for interpretation
///
/// # Returns
///
/// The u32 value (returns 0 if bytes are too short)
#[inline]
pub fn read_u32(bytes: &[u8], byte_order: ByteOrder) -> u32 {
    let reader = EndianReader::new(bytes, to_io_byte_order(byte_order));
    reader.u32_at(0).unwrap_or(0)
}

/// Reads a signed 32-bit integer from bytes with the specified byte order.
///
/// Uses `EndianReader` for consistent byte order handling across the codebase.
///
/// # Arguments
///
/// * `bytes` - Byte slice (must be at least 4 bytes)
/// * `byte_order` - Byte order for interpretation
///
/// # Returns
///
/// The i32 value (returns 0 if bytes are too short)
#[inline]
pub fn read_i32(bytes: &[u8], byte_order: ByteOrder) -> i32 {
    let reader = EndianReader::new(bytes, to_io_byte_order(byte_order));
    reader.i32_at(0).unwrap_or(0)
}

// ============================================================================
// DATETIME UTILITIES
// ============================================================================

/// Checks if a string matches the EXIF DateTime format (YYYY:MM:DD HH:MM:SS).
///
/// EXIF DateTime format: "2025:01:15 10:30:00" (19 characters)
/// - 4 digits for year
/// - 2 colons separating year:month:day
/// - 1 space between date and time
/// - 2 colons separating hour:minute:second
///
/// # Arguments
///
/// * `s` - String to check
///
/// # Returns
///
/// true if the string matches EXIF DateTime format, false otherwise
pub fn is_datetime_string(s: &str) -> bool {
    s.len() == 19
        && s.chars().filter(|&c| c == ':').count() == 4
        && s.chars().filter(|&c| c == ' ').count() == 1
        && s.chars().nth(4) == Some(':')
        && s.chars().nth(7) == Some(':')
        && s.chars().nth(10) == Some(' ')
        && s.chars().nth(13) == Some(':')
        && s.chars().nth(16) == Some(':')
}

/// Parses an EXIF DateTime string into a chrono::DateTime<Utc>.
///
/// EXIF format: "2025:01:15 10:30:00" (YYYY:MM:DD HH:MM:SS)
///
/// # Arguments
///
/// * `s` - EXIF DateTime string
///
/// # Returns
///
/// * `Ok(DateTime<Utc>)` - Successfully parsed datetime
/// * `Err(ExifToolError)` - Invalid datetime format
pub fn parse_exif_datetime(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    use chrono::NaiveDateTime;

    let naive = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .map_err(|e| ExifToolError::parse_error(format!("Invalid DateTime: {}", e)))?;

    Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        naive,
        chrono::Utc,
    ))
}

// ============================================================================
// STRING PARSING UTILITIES
// ============================================================================

/// Parses a string value to an appropriate TagValue.
///
/// Attempts to parse as integer first, then float, otherwise returns as string.
/// Used for XMP and IPTC metadata parsing.
///
/// # Arguments
///
/// * `value` - String value to parse
///
/// # Returns
///
/// A TagValue with the appropriate type
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
// MATHEMATICAL UTILITIES
// ============================================================================

/// Computes the Greatest Common Divisor (GCD) of two unsigned integers using Euclid's algorithm.
///
/// Used for simplifying fractions when displaying RATIONAL values.
///
/// # Arguments
///
/// * `a` - First number
/// * `b` - Second number
///
/// # Returns
///
/// The GCD of a and b
pub fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

// ============================================================================
// RATIONAL VALUE FORMATTING
// ============================================================================

/// Formats a GPS coordinate from 3 rational values (degrees, minutes, seconds).
///
/// GPS coordinates are stored as 3 rationals representing degrees, minutes, and seconds.
/// This function converts them to a human-readable DMS (Degrees, Minutes, Seconds) format.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing 3 rationals (24 bytes total)
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// Formatted GPS coordinate string (e.g., "37 deg 46' 33.24\"")
pub fn format_gps_coordinate(bytes: &[u8], byte_order: ByteOrder) -> String {
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
    format!("{} deg {}' {}\"", dms[0] as i32, dms[1] as i32, sec_trimmed)
}

/// Parses an array of rational values into a space-separated string.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing multiple rationals
/// * `value_count` - Number of rational values
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// Space-separated decimal values string
pub fn parse_rational_array(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> String {
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
    values
        .iter()
        .map(|v| format!("{:.10}", v))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses an array of signed rational values into a space-separated string.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing multiple signed rationals
/// * `value_count` - Number of signed rational values
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// Space-separated decimal values string
pub fn parse_srational_array(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> String {
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
    values
        .iter()
        .map(|v| format!("{:.10}", v))
        .collect::<Vec<_>>()
        .join(" ")
}

// ============================================================================
// SPECIAL TAG HANDLERS
// ============================================================================

/// Handles special byte-encoded tags that need custom formatting.
///
/// This includes GPS_VERSION_ID, EXIF_VERSION, and COMPONENTS_CONFIGURATION
/// which have specific byte-level encoding requirements.
///
/// # Arguments
///
/// * `tag_id` - The tag ID to check
/// * `bytes` - The raw bytes
///
/// # Returns
///
/// Some(TagValue) if this is a special tag, None otherwise
pub fn handle_special_byte_tags(tag_id: u16, bytes: &[u8]) -> Option<TagValue> {
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
// TYPE DETECTION HELPERS
// ============================================================================

/// Checks if bytes are likely a printable ASCII string.
///
/// # Arguments
///
/// * `bytes` - Bytes to check
///
/// # Returns
///
/// true if bytes appear to be printable ASCII
pub fn is_printable_ascii(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|&b| (32..=126).contains(&b) || b == 0 || b == b'\n' || b == b'\r' || b == b'\t')
}

/// Checks if 4-byte sequence is likely a string rather than an integer.
///
/// # Arguments
///
/// * `bytes` - 4-byte sequence to check
///
/// # Returns
///
/// true if likely a string
pub fn is_likely_short_string(bytes: &[u8]) -> bool {
    if bytes.len() != 4 {
        return false;
    }

    let null_count = bytes.iter().filter(|&&b| b == 0).count();
    let has_printable = bytes.iter().any(|&b| (32..=126).contains(&b));

    // If multiple nulls or no printable chars, not a string
    if null_count > 1 || !has_printable {
        return false;
    }

    // If all bytes are printable ASCII (and maybe one trailing null), likely a string
    bytes.iter().all(|&b| (32..=126).contains(&b) || b == 0)
        && bytes.iter().filter(|&&b| (32..=126).contains(&b)).count() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u16() {
        let bytes = [0x12, 0x34];
        assert_eq!(read_u16(&bytes, ByteOrder::LittleEndian), 0x3412);
        assert_eq!(read_u16(&bytes, ByteOrder::BigEndian), 0x1234);
    }

    #[test]
    fn test_read_u32() {
        let bytes = [0x12, 0x34, 0x56, 0x78];
        assert_eq!(read_u32(&bytes, ByteOrder::LittleEndian), 0x78563412);
        assert_eq!(read_u32(&bytes, ByteOrder::BigEndian), 0x12345678);
    }

    #[test]
    fn test_read_i32() {
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read_i32(&bytes, ByteOrder::LittleEndian), -1);
        assert_eq!(read_i32(&bytes, ByteOrder::BigEndian), -1);
    }

    #[test]
    fn test_is_datetime_string() {
        assert!(is_datetime_string("2025:01:15 10:30:00"));
        assert!(!is_datetime_string("2025-01-15 10:30:00"));
        assert!(!is_datetime_string("not a date"));
        assert!(!is_datetime_string("2025:01:15"));
    }

    #[test]
    fn test_parse_exif_datetime() {
        let result = parse_exif_datetime("2025:01:15 10:30:00");
        assert!(result.is_ok());

        let result = parse_exif_datetime("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_string_to_tag_value() {
        let value = parse_string_to_tag_value("123");
        assert_eq!(value.as_integer(), Some(123));

        let value = parse_string_to_tag_value("123.45");
        assert_eq!(value.as_float(), Some(123.45));

        let value = parse_string_to_tag_value("hello");
        assert_eq!(value.as_string(), Some("hello"));
    }

    #[test]
    fn test_gcd() {
        assert_eq!(gcd(48, 18), 6);
        assert_eq!(gcd(100, 10), 10);
        assert_eq!(gcd(17, 19), 1);
        assert_eq!(gcd(10, 0), 10);
    }

    #[test]
    fn test_is_printable_ascii() {
        assert!(is_printable_ascii(b"Hello World"));
        assert!(is_printable_ascii(b"Test\0"));
        assert!(is_printable_ascii(b"Line1\nLine2"));
        assert!(!is_printable_ascii(&[0xFF, 0xFE, 0xFD]));
    }

    #[test]
    fn test_is_likely_short_string() {
        assert!(is_likely_short_string(b"EOS\0"));
        assert!(is_likely_short_string(b"JPEG"));
        assert!(!is_likely_short_string(b"\0\0\0\0"));
        assert!(!is_likely_short_string(b"AB\0\0"));
        assert!(!is_likely_short_string(&[0xFF, 0xD8, 0xFF, 0xE0]));
    }
}
