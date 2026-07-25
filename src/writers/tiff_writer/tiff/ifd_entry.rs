//! IFD entry data structures and conversion logic
//!
//! This module defines the `IfdEntryData` structure that represents a single
//! TIFF IFD entry, along with conversion functions for transforming TagValue
//! instances into TIFF-compatible entry data.

use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use crate::parsers::common::exif_types::ExifType;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Represents a single TIFF IFD entry to be serialized.
///
/// Each entry contains all the information needed to write a TIFF tag:
/// - Tag identifier (e.g., 0x010F for Make)
/// - EXIF data type code
/// - Number of values (not bytes)
/// - Raw value bytes (either stored inline or in the value area)
#[derive(Debug, Clone)]
pub struct IfdEntryData {
    /// Tag identifier (e.g., 0x010F for Make)
    pub tag_id: u16,
    /// EXIF data type code
    pub field_type: ExifType,
    /// Number of values (not bytes)
    pub value_count: u32,
    /// Raw value bytes (will be inline or in value area)
    pub value_bytes: Vec<u8>,
}

impl IfdEntryData {
    /// Creates a new IFD entry with the specified parameters.
    ///
    /// # Parameters
    ///
    /// - `tag_id`: Numeric tag identifier
    /// - `field_type`: EXIF data type for the value
    /// - `value_count`: Number of values (not bytes)
    /// - `value_bytes`: Raw byte representation of the value
    pub fn new(tag_id: u16, field_type: ExifType, value_count: u32, value_bytes: Vec<u8>) -> Self {
        Self {
            tag_id,
            field_type,
            value_count,
            value_bytes,
        }
    }

    /// Returns true if this entry's value should be stored inline.
    ///
    /// In TIFF format, values of 4 bytes or less are stored directly in the
    /// entry's value field. Larger values are stored in a separate value area
    /// and referenced by an offset.
    pub fn is_inline(&self) -> bool {
        self.value_bytes.len() <= 4
    }

    /// Returns the size of the value data in bytes.
    pub fn value_size(&self) -> usize {
        self.value_bytes.len()
    }
}

/// Converts a TagValue to an IfdEntryData structure.
///
/// This function handles the conversion from our high-level TagValue representation
/// to the low-level TIFF IFD entry format. It automatically selects the appropriate
/// EXIF data type and serializes the value in the correct byte order.
///
/// # Parameters
///
/// - `tag_id`: Numeric tag identifier for this entry
/// - `tag_value`: The TagValue to convert
/// - `byte_order`: Endianness for multi-byte values
///
/// # Returns
///
/// - `Ok(Some(entry))`: Conversion succeeded
/// - `Ok(None)`: Tag type is not yet supported (will be skipped)
/// - `Err`: Actual error occurred during conversion
///
/// # Supported Types
///
/// - `String`: Converted to ASCII type with null terminator
/// - `Integer`: Converted to Short, Long, or SLong based on value range
/// - `Rational`: Converted to Rational type (two u32 values)
/// - `Binary`: Converted to Undefined type (raw bytes)
/// - `DateTime`: Formatted as EXIF datetime string (ASCII)
///
/// # Unsupported Types
///
/// - `Float`: Not yet implemented
/// - `Struct`: Not yet implemented
/// - `Array`: Not yet implemented
pub fn convert_tag_value_to_entry(
    tag_id: u16,
    tag_value: &TagValue,
    byte_order: ByteOrder,
) -> Result<Option<IfdEntryData>> {
    match tag_value {
        TagValue::String(s) => {
            // ASCII type - null-terminated string
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0); // Add null terminator
            let count = bytes.len() as u32;

            Ok(Some(IfdEntryData::new(
                tag_id,
                ExifType::Ascii,
                count,
                bytes,
            )))
        }

        TagValue::Integer(i) => convert_integer_to_entry(tag_id, *i, byte_order),

        TagValue::Rational {
            numerator,
            denominator,
        } => convert_rational_to_entry(tag_id, *numerator as i64, *denominator as i64, byte_order),

        TagValue::Binary(data) => {
            // Undefined type - raw bytes
            Ok(Some(IfdEntryData::new(
                tag_id,
                ExifType::Undefined,
                data.len() as u32,
                data.clone(),
            )))
        }

        TagValue::DateTime(dt) => {
            // Format DateTime to EXIF format string: "YYYY:MM:DD HH:MM:SS"
            use crate::core::date_shift::format_exif_datetime;
            let datetime_str = format_exif_datetime(dt);

            // Add null terminator
            let mut bytes = datetime_str.into_bytes();
            bytes.push(0);

            Ok(Some(IfdEntryData::new(
                tag_id,
                ExifType::Ascii,
                bytes.len() as u32,
                bytes,
            )))
        }

        // Unsupported types - skip for now (will add TODO in tests)
        TagValue::Float(_) => Ok(None),
        TagValue::Struct(_) => Ok(None),
        TagValue::Array(_) => Ok(None), // Arrays not yet supported in TIFF writer
    }
}

/// Converts an integer value to the appropriate TIFF entry type.
///
/// Selects the most compact representation:
/// - Short (u16) for values 0-65535
/// - Long (u32) for values 0-4294967295
/// - SLong (i32) for negative values or values requiring signed representation
fn convert_integer_to_entry(
    tag_id: u16,
    value: i64,
    byte_order: ByteOrder,
) -> Result<Option<IfdEntryData>> {
    if value >= 0 && value <= u16::MAX as i64 {
        // Fits in u16 - use Short
        let value_u16 = value as u16;
        let bytes = match byte_order {
            ByteOrder::LittleEndian => value_u16.to_le_bytes().to_vec(),
            ByteOrder::BigEndian => value_u16.to_be_bytes().to_vec(),
        };

        Ok(Some(IfdEntryData::new(tag_id, ExifType::Short, 1, bytes)))
    } else if value >= 0 && value <= u32::MAX as i64 {
        // Fits in u32 - use Long
        let value_u32 = value as u32;
        let bytes = match byte_order {
            ByteOrder::LittleEndian => value_u32.to_le_bytes().to_vec(),
            ByteOrder::BigEndian => value_u32.to_be_bytes().to_vec(),
        };

        Ok(Some(IfdEntryData::new(tag_id, ExifType::Long, 1, bytes)))
    } else if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
        // Needs signed long - use SLong
        let value_i32 = value as i32;
        let bytes = match byte_order {
            ByteOrder::LittleEndian => value_i32.to_le_bytes().to_vec(),
            ByteOrder::BigEndian => value_i32.to_be_bytes().to_vec(),
        };

        Ok(Some(IfdEntryData::new(tag_id, ExifType::SLong, 1, bytes)))
    } else {
        Err(ExifToolError::invalid_tag_value(
            "integer_value",
            format!("Integer value {} out of range for TIFF types", value),
        ))
    }
}

/// Converts a rational value (numerator/denominator) to a TIFF entry.
///
/// Rational values are stored as two consecutive u32 values in TIFF format.
fn convert_rational_to_entry(
    tag_id: u16,
    numerator: i64,
    denominator: i64,
    byte_order: ByteOrder,
) -> Result<Option<IfdEntryData>> {
    let mut bytes = Vec::with_capacity(8);

    match byte_order {
        ByteOrder::LittleEndian => {
            bytes.extend_from_slice(&(numerator as u32).to_le_bytes());
            bytes.extend_from_slice(&(denominator as u32).to_le_bytes());
        }
        ByteOrder::BigEndian => {
            bytes.extend_from_slice(&(numerator as u32).to_be_bytes());
            bytes.extend_from_slice(&(denominator as u32).to_be_bytes());
        }
    }

    Ok(Some(IfdEntryData::new(
        tag_id,
        ExifType::Rational,
        1,
        bytes,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_is_inline() {
        let entry_inline =
            IfdEntryData::new(0x0110, ExifType::Ascii, 4, vec![0x41, 0x42, 0x43, 0x00]);
        assert!(entry_inline.is_inline());

        let entry_offset = IfdEntryData::new(
            0x010F,
            ExifType::Ascii,
            6,
            vec![0x41, 0x42, 0x43, 0x44, 0x45, 0x00],
        );
        assert!(!entry_offset.is_inline());
    }

    #[test]
    fn test_convert_string_to_entry() {
        let result = convert_tag_value_to_entry(
            0x010F,
            &TagValue::new_string("Canon"),
            ByteOrder::LittleEndian,
        );

        assert!(result.is_ok());
        let entry = result.unwrap().unwrap();
        assert_eq!(entry.tag_id, 0x010F);
        assert_eq!(entry.field_type, ExifType::Ascii);
        assert_eq!(entry.value_count, 6); // "Canon" + null
        assert_eq!(entry.value_bytes, b"Canon\0");
    }

    #[test]
    fn test_convert_integer_to_short() {
        let result = convert_tag_value_to_entry(
            0x8827,
            &TagValue::new_integer(400),
            ByteOrder::LittleEndian,
        );

        assert!(result.is_ok());
        let entry = result.unwrap().unwrap();
        assert_eq!(entry.field_type, ExifType::Short);
        assert_eq!(entry.value_count, 1);
        assert_eq!(entry.value_bytes, vec![0x90, 0x01]); // 400 in little-endian
    }

    #[test]
    fn test_convert_integer_to_long() {
        let result = convert_tag_value_to_entry(
            0x0100,
            &TagValue::new_integer(100000),
            ByteOrder::LittleEndian,
        );

        assert!(result.is_ok());
        let entry = result.unwrap().unwrap();
        assert_eq!(entry.field_type, ExifType::Long);
        assert_eq!(entry.value_count, 1);
    }

    #[test]
    fn test_convert_rational() {
        let result = convert_tag_value_to_entry(
            0x829D,
            &TagValue::new_rational(28, 10),
            ByteOrder::LittleEndian,
        );

        assert!(result.is_ok());
        let entry = result.unwrap().unwrap();
        assert_eq!(entry.field_type, ExifType::Rational);
        assert_eq!(entry.value_count, 1);
        assert_eq!(entry.value_bytes.len(), 8); // Two u32 values
    }

    #[test]
    fn test_convert_binary() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let result = convert_tag_value_to_entry(
            0x9286,
            &TagValue::new_binary(data.clone()),
            ByteOrder::LittleEndian,
        );

        assert!(result.is_ok());
        let entry = result.unwrap().unwrap();
        assert_eq!(entry.field_type, ExifType::Undefined);
        assert_eq!(entry.value_count, 4);
        assert_eq!(entry.value_bytes, data);
    }
}
