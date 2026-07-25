//! PNG-specific tag value conversion
//!
//! This module provides value conversion functions for PNG EXIF data,
//! handling the special cases where PNG outputs both enum-resolved and raw values.

use crate::core::TagValue;
use crate::io::{ByteOrder as IoByteOrder, EndianReader};
use crate::parsers::common::exif_types::ExifType;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Converts raw bytes from IFD to a TagValue WITHOUT enum interpretation.
///
/// This version is used for PNG:Exif tags where Perl ExifTool outputs raw values.
pub fn raw_bytes_to_tag_value_no_enum(
    bytes: &[u8],
    field_type: u16,
    _value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    const EXIF_VERSION: u16 = 0x9000;

    // Convert TIFF ByteOrder to IO ByteOrder for EndianReader
    let io_order = match byte_order {
        ByteOrder::LittleEndian => IoByteOrder::Little,
        ByteOrder::BigEndian => IoByteOrder::Big,
    };
    let reader = EndianReader::new(bytes, io_order);

    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // SHORT (type 3): 16-bit unsigned integer
            ExifType::Short if bytes.len() >= 2 => {
                let value = reader.u16_at(0).unwrap_or(0);
                return TagValue::new_integer(value as i64);
            }

            // LONG (type 4): 32-bit unsigned integer
            ExifType::Long if bytes.len() >= 4 => {
                let value = reader.u32_at(0).unwrap_or(0);
                return TagValue::new_integer(value as i64);
            }

            // ASCII (type 2): null-terminated string
            ExifType::Ascii => {
                let text = String::from_utf8_lossy(bytes);
                let trimmed = text.trim_end_matches('\0');
                return TagValue::new_string(trimmed);
            }

            // UNDEFINED (type 7): Return as binary or special string
            ExifType::Undefined => {
                // Special handling for ExifVersion (tag 0x9000)
                if tag_id == EXIF_VERSION && bytes.len() >= 4 {
                    // ExifVersion is stored as ASCII bytes
                    let version = String::from_utf8_lossy(&bytes[0..4]);
                    return TagValue::new_string(version.to_string());
                }
                // Perl ExifTool shows UNDEFINED bytes as "..." in PNG:Exif namespace
                return TagValue::new_string("...");
            }

            _ => {
                // Fallback
            }
        }
    }

    // Fallback: store as binary
    TagValue::new_binary(bytes.to_vec())
}

/// Converts raw bytes from IFD to a TagValue.
///
/// This is used for standard EXIF tag output in PNG files.
/// Unlike the core version, this doesn't apply enum resolution.
pub fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    // Convert TIFF ByteOrder to IO ByteOrder for EndianReader
    let io_order = match byte_order {
        ByteOrder::LittleEndian => IoByteOrder::Little,
        ByteOrder::BigEndian => IoByteOrder::Big,
    };
    let reader = EndianReader::new(bytes, io_order);

    // Try to convert field_type to ExifType
    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // RATIONAL (type 5): two 32-bit unsigned integers (numerator/denominator)
            ExifType::Rational if bytes.len() >= 8 => {
                // Check if this is an array of rationals (count > 1)
                if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
                    // Parse array of rationals and format as space-separated decimals
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 8;
                        let numerator = reader.u32_at(offset).unwrap_or(0);
                        let denominator = reader.u32_at(offset + 4).unwrap_or(0);
                        if denominator != 0 {
                            values.push(numerator as f64 / denominator as f64);
                        } else {
                            values.push(numerator as f64);
                        }
                    }
                    // Return as rational (first value) to match behavior
                    if !values.is_empty() {
                        let num = (values[0] * 1000000.0) as i32;
                        return TagValue::new_rational(num, 1000000);
                    }
                }

                // Single rational value
                let numerator = reader.u32_at(0).unwrap_or(0);
                let denominator = reader.u32_at(4).unwrap_or(0);

                // Simplify: if denominator is 1, return as integer
                if denominator == 1 {
                    return TagValue::new_integer(numerator as i64);
                }

                return TagValue::new_rational(numerator as i32, denominator as i32);
            }

            // SHORT (type 3): 16-bit unsigned integer
            ExifType::Short if bytes.len() >= 2 => {
                // Handle array of SHORT values
                if value_count > 1 && bytes.len() >= (value_count as usize * 2) {
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 2;
                        let value = reader.u16_at(offset).unwrap_or(0);
                        values.push(value as i64);
                    }
                    // Return as space-separated string for arrays
                    return TagValue::new_string(
                        values
                            .iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }

                let value = reader.u16_at(0).unwrap_or(0);

                // Preserve raw numeric value; presentation later resolves friendly name.
                return TagValue::new_integer(value as i64);
            }

            // LONG (type 4): 32-bit unsigned integer
            ExifType::Long if bytes.len() >= 4 => {
                // Handle array of LONG values
                if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 4;
                        let value = reader.u32_at(offset).unwrap_or(0);
                        values.push(value as i64);
                    }
                    // Return as space-separated string for arrays
                    return TagValue::new_string(
                        values
                            .iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }

                let value = reader.u32_at(0).unwrap_or(0);

                // Preserve raw numeric value; presentation later resolves friendly name.
                return TagValue::new_integer(value as i64);
            }

            // ASCII (type 2): null-terminated string
            ExifType::Ascii => {
                let text = String::from_utf8_lossy(bytes);
                let trimmed = text.trim_end_matches('\0');
                return TagValue::new_string(trimmed);
            }

            // UNDEFINED (type 7): typically used for ExifVersion, ComponentsConfiguration, etc.
            ExifType::Undefined => {
                // Handle ExifVersion (tag 0x9000) - 4 bytes representing version
                if tag_id == 0x9000 && bytes.len() >= 4 {
                    let version_str = format!(
                        "{}{}{}{}",
                        bytes[0] as char, bytes[1] as char, bytes[2] as char, bytes[3] as char
                    );
                    return TagValue::new_string(version_str);
                }

                // Handle ComponentsConfiguration (tag 0x9101) - 4 bytes
                if tag_id == 0x9101 && bytes.len() >= 4 {
                    let components: Vec<&str> = bytes
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
                        .collect();
                    return TagValue::new_string(components.join(", "));
                }

                // Otherwise store as binary
                return TagValue::new_binary(bytes.to_vec());
            }

            _ => {
                // Fallback for other types
            }
        }
    }

    // Fallback: try to interpret as ASCII string
    if bytes.iter().all(|&b| b.is_ascii() || b == 0) {
        let text = String::from_utf8_lossy(bytes);
        let trimmed = text.trim_end_matches('\0');
        TagValue::new_string(trimmed)
    } else {
        // Store as binary
        TagValue::new_binary(bytes.to_vec())
    }
}
