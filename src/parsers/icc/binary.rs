//! Binary data readers for ICC profile parsing
//!
//! This module provides low-level functions for reading binary data
//! from ICC profile byte arrays with proper endianness handling.
//!
//! ICC profiles use big-endian byte order throughout.

use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// Reads a 4-byte big-endian unsigned integer
pub fn read_u32_be(data: &[u8], offset: usize) -> Result<u32> {
    let reader = EndianReader::big_endian(data);
    reader
        .u32_at(offset)
        .ok_or_else(|| ExifToolError::parse_error("Offset out of bounds"))
}

/// Reads a 2-byte big-endian unsigned integer
pub fn read_u16_be(data: &[u8], offset: usize) -> Result<u16> {
    let reader = EndianReader::big_endian(data);
    reader
        .u16_at(offset)
        .ok_or_else(|| ExifToolError::parse_error("Offset out of bounds"))
}

/// Reads an 8-byte big-endian unsigned integer
pub fn read_u64_be(data: &[u8], offset: usize) -> Result<u64> {
    let reader = EndianReader::big_endian(data);
    reader
        .u64_at(offset)
        .ok_or_else(|| ExifToolError::parse_error("Offset out of bounds"))
}

/// Reads a 4-byte signature as a trimmed ASCII string
/// Null bytes are stripped to handle null-padded signatures.
pub fn read_signature(data: &[u8], offset: usize) -> Result<String> {
    let reader = EndianReader::big_endian(data);
    let bytes = reader
        .bytes_at(offset, 4)
        .ok_or_else(|| ExifToolError::parse_error("Offset out of bounds"))?;
    // Convert to string and strip null bytes and whitespace
    let s = String::from_utf8_lossy(bytes);
    Ok(s.trim_matches('\0').trim().to_string())
}

/// Reads a signed 15.16 fixed-point number and converts to f64
///
/// The format stores values as a 32-bit signed integer where:
/// - Upper 16 bits: signed integer part
/// - Lower 16 bits: fractional part (0-65535 maps to 0.0-0.99998)
pub fn read_s15fixed16(data: &[u8], offset: usize) -> Result<f64> {
    let reader = EndianReader::big_endian(data);
    let value = reader
        .i32_at(offset)
        .ok_or_else(|| ExifToolError::parse_error("Offset out of bounds"))?;
    let integer_part = (value >> 16) as f64;
    let fractional_part = (value & 0xFFFF) as f64 / 65536.0;
    Ok(integer_part + fractional_part)
}

/// Reads an unsigned 16.16 fixed-point number and converts to f64
///
/// The format stores values as a 32-bit unsigned integer where:
/// - Upper 16 bits: unsigned integer part
/// - Lower 16 bits: fractional part (0-65535 maps to 0.0-0.99998)
pub fn read_u16fixed16(data: &[u8], offset: usize) -> Result<f64> {
    let reader = EndianReader::big_endian(data);
    let value = reader
        .u32_at(offset)
        .ok_or_else(|| ExifToolError::parse_error("Offset out of bounds"))?;
    let integer_part = (value >> 16) as f64;
    let fractional_part = (value & 0xFFFF) as f64 / 65536.0;
    Ok(integer_part + fractional_part)
}

/// Finds a byte sequence in a larger byte slice
pub fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
