//! EXIF data type definitions
//!
//! This module defines EXIF data types used across formats.
//! Based on TIFF 6.0 and EXIF 2.3 specifications.

#![allow(dead_code)]

/// EXIF field data types as defined in TIFF/EXIF specification.
///
/// Each type has a corresponding numeric code that appears in TIFF IFD entries.
/// The type determines how to interpret the raw bytes in the value field.
///
/// # TIFF Type Codes
///
/// | Code | Type Name | Size per Value | Description |
/// |------|-----------|----------------|-------------|
/// | 1 | BYTE | 1 byte | Unsigned 8-bit integer |
/// | 2 | ASCII | 1 byte | ASCII string (null-terminated) |
/// | 3 | SHORT | 2 bytes | Unsigned 16-bit integer |
/// | 4 | LONG | 4 bytes | Unsigned 32-bit integer |
/// | 5 | RATIONAL | 8 bytes | Two LONGs: numerator/denominator |
/// | 6 | SBYTE | 1 byte | Signed 8-bit integer |
/// | 7 | UNDEFINED | 1 byte | Byte array (uninterpreted) |
/// | 8 | SSHORT | 2 bytes | Signed 16-bit integer |
/// | 9 | SLONG | 4 bytes | Signed 32-bit integer |
/// | 10 | SRATIONAL | 8 bytes | Two SLONGs: numerator/denominator |
/// | 11 | FLOAT | 4 bytes | IEEE 754 single precision |
/// | 12 | DOUBLE | 8 bytes | IEEE 754 double precision |
///
/// # Reference
///
/// - TIFF 6.0 Specification, Section 2: Image File Directory
/// - EXIF 2.3 Specification, Section 4.6: TIFF Tags
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifType {
    /// Unsigned 8-bit integer (1 byte)
    Byte = 1,
    /// ASCII string, null-terminated (1 byte per character)
    Ascii = 2,
    /// Unsigned 16-bit integer (2 bytes)
    Short = 3,
    /// Unsigned 32-bit integer (4 bytes)
    Long = 4,
    /// Two LONGs: numerator and denominator (8 bytes)
    Rational = 5,
    /// Signed 8-bit integer (1 byte)
    SByte = 6,
    /// Byte array with no specific type (1 byte per value)
    Undefined = 7,
    /// Signed 16-bit integer (2 bytes)
    SShort = 8,
    /// Signed 32-bit integer (4 bytes)
    SLong = 9,
    /// Two SLONGs: numerator and denominator (8 bytes)
    SRational = 10,
    /// IEEE 754 single precision float (4 bytes)
    Float = 11,
    /// IEEE 754 double precision float (8 bytes)
    Double = 12,
}

impl ExifType {
    /// Returns the size in bytes of a single value of this type.
    ///
    /// # Examples
    ///
    /// ```
    /// use exiftool_rs::parsers::common::exif_types::ExifType;
    ///
    /// assert_eq!(ExifType::Byte.size_in_bytes(), 1);
    /// assert_eq!(ExifType::Short.size_in_bytes(), 2);
    /// assert_eq!(ExifType::Long.size_in_bytes(), 4);
    /// assert_eq!(ExifType::Rational.size_in_bytes(), 8);
    /// ```
    pub const fn size_in_bytes(&self) -> usize {
        match self {
            ExifType::Byte => 1,
            ExifType::Ascii => 1,
            ExifType::Short => 2,
            ExifType::Long => 4,
            ExifType::Rational => 8,
            ExifType::SByte => 1,
            ExifType::Undefined => 1,
            ExifType::SShort => 2,
            ExifType::SLong => 4,
            ExifType::SRational => 8,
            ExifType::Float => 4,
            ExifType::Double => 8,
        }
    }

    /// Attempts to create an ExifType from a u16 type code.
    ///
    /// Returns `None` if the type code is not recognized.
    ///
    /// # Examples
    ///
    /// ```
    /// use exiftool_rs::parsers::common::exif_types::ExifType;
    ///
    /// assert_eq!(ExifType::from_u16(1), Some(ExifType::Byte));
    /// assert_eq!(ExifType::from_u16(2), Some(ExifType::Ascii));
    /// assert_eq!(ExifType::from_u16(99), None); // Invalid type code
    /// ```
    pub const fn from_u16(code: u16) -> Option<Self> {
        match code {
            1 => Some(ExifType::Byte),
            2 => Some(ExifType::Ascii),
            3 => Some(ExifType::Short),
            4 => Some(ExifType::Long),
            5 => Some(ExifType::Rational),
            6 => Some(ExifType::SByte),
            7 => Some(ExifType::Undefined),
            8 => Some(ExifType::SShort),
            9 => Some(ExifType::SLong),
            10 => Some(ExifType::SRational),
            11 => Some(ExifType::Float),
            12 => Some(ExifType::Double),
            _ => None,
        }
    }

    /// Converts the ExifType to its u16 type code.
    ///
    /// # Examples
    ///
    /// ```
    /// use exiftool_rs::parsers::common::exif_types::ExifType;
    ///
    /// assert_eq!(ExifType::Byte.as_u16(), 1);
    /// assert_eq!(ExifType::Ascii.as_u16(), 2);
    /// ```
    pub const fn as_u16(&self) -> u16 {
        *self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_sizes() {
        assert_eq!(ExifType::Byte.size_in_bytes(), 1);
        assert_eq!(ExifType::Ascii.size_in_bytes(), 1);
        assert_eq!(ExifType::Short.size_in_bytes(), 2);
        assert_eq!(ExifType::Long.size_in_bytes(), 4);
        assert_eq!(ExifType::Rational.size_in_bytes(), 8);
        assert_eq!(ExifType::SByte.size_in_bytes(), 1);
        assert_eq!(ExifType::Undefined.size_in_bytes(), 1);
        assert_eq!(ExifType::SShort.size_in_bytes(), 2);
        assert_eq!(ExifType::SLong.size_in_bytes(), 4);
        assert_eq!(ExifType::SRational.size_in_bytes(), 8);
        assert_eq!(ExifType::Float.size_in_bytes(), 4);
        assert_eq!(ExifType::Double.size_in_bytes(), 8);
    }

    #[test]
    fn test_from_u16_valid() {
        assert_eq!(ExifType::from_u16(1), Some(ExifType::Byte));
        assert_eq!(ExifType::from_u16(2), Some(ExifType::Ascii));
        assert_eq!(ExifType::from_u16(3), Some(ExifType::Short));
        assert_eq!(ExifType::from_u16(4), Some(ExifType::Long));
        assert_eq!(ExifType::from_u16(5), Some(ExifType::Rational));
        assert_eq!(ExifType::from_u16(7), Some(ExifType::Undefined));
        assert_eq!(ExifType::from_u16(9), Some(ExifType::SLong));
        assert_eq!(ExifType::from_u16(10), Some(ExifType::SRational));
    }

    #[test]
    fn test_from_u16_invalid() {
        assert_eq!(ExifType::from_u16(0), None);
        assert_eq!(ExifType::from_u16(13), None);
        assert_eq!(ExifType::from_u16(99), None);
        assert_eq!(ExifType::from_u16(255), None);
    }

    #[test]
    fn test_as_u16() {
        assert_eq!(ExifType::Byte.as_u16(), 1);
        assert_eq!(ExifType::Ascii.as_u16(), 2);
        assert_eq!(ExifType::Short.as_u16(), 3);
        assert_eq!(ExifType::Long.as_u16(), 4);
        assert_eq!(ExifType::Rational.as_u16(), 5);
    }

    #[test]
    fn test_round_trip_conversion() {
        for code in 1..=12u16 {
            if let Some(exif_type) = ExifType::from_u16(code) {
                assert_eq!(exif_type.as_u16(), code);
            }
        }
    }
}
