//! APP0 JFIF segment parser
//!
//! JPEG APP0 segments (marker 0xFFE0) contain JFIF (JPEG File Interchange Format) metadata
//! when they start with the "JFIF\0" identifier. This module provides parsing functionality
//! to extract JFIF version, resolution units, and density information.
//!
//! # APP0 JFIF Segment Format
//!
//! The APP0 JFIF segment has the following structure:
//! - Identifier: "JFIF\0" (5 bytes)
//! - JFIFVersion Major: Version major number (1 byte) - typically 1
//! - JFIFVersion Minor: Version minor number (1 byte) - typically 0, 1, or 2
//! - DensityUnits: Resolution unit code (1 byte)
//!   - 0 = No units (aspect ratio only)
//!   - 1 = Dots per inch (DPI)
//!   - 2 = Dots per centimeter (DPCM)
//! - XDensity: Horizontal resolution (2 bytes, big-endian) - typically 72 or 96 DPI
//! - YDensity: Vertical resolution (2 bytes, big-endian) - typically 72 or 96 DPI
//! - ThumbnailWidth: Thumbnail image width in pixels (1 byte)
//! - ThumbnailHeight: Thumbnail image height in pixels (1 byte)
//! - Thumbnail Data: Optional thumbnail image data (if width/height > 0)
//!
//! # Version History
//!
//! - JFIF 1.00: Initial release
//! - JFIF 1.01: Added extended APP0 (JFXX) segments for thumbnail support
//! - JFIF 1.02: Extended compatibility improvements
//!
//! # Example
//!
//! ```ignore
//! use oxidex::parsers::jpeg::app_segments::app0::parse_app0;
//!
//! let data: &[u8] = &[/* APP0 JFIF segment data */];
//! let metadata = parse_app0(data)?;
//!
//! if let Some(version) = metadata.get_string("JFIF:JFIFVersion") {
//!     println!("JFIF version: {}", version);
//! }
//! ```

use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// The "JFIF\0" identifier that marks an APP0 segment as JFIF-format.
const JFIF_IDENTIFIER: &[u8] = b"JFIF\0";

/// Minimum length for a valid APP0 JFIF segment (without thumbnail).
///
/// Structure: "JFIF\0" (5) + Version Major (1) + Version Minor (1) + DensityUnits (1)
///            + XDensity (2) + YDensity (2) + ThumbnailWidth (1) + ThumbnailHeight (1) = 14 bytes
const MIN_JFIF_SEGMENT_LENGTH: usize = 14;

/// Parses APP0 JFIF segment data and extracts version and resolution metadata.
///
/// This function validates the "JFIF\0" identifier and extracts the following tags:
/// - `JFIF:JFIFVersion` - JFIF version as "Major.Minor" (String)
/// - `JFIF:DensityUnits` - Resolution unit description (String: "None", "inches", or "cm")
/// - `JFIF:XDensity` - Horizontal resolution value (Integer)
/// - `JFIF:YDensity` - Vertical resolution value (Integer)
///
/// # Arguments
///
/// * `data` - Raw APP0 segment data (excluding the APP0 marker and length bytes)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - A metadata map containing extracted JFIF tags
/// * `Err(ExifToolError)` - If the data is not a valid JFIF segment or is malformed
///
/// # Errors
///
/// Returns an error if:
/// - The segment does not start with "JFIF\0" identifier
/// - The segment is shorter than the minimum required length (14 bytes)
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app0::parse_app0;
///
/// // Construct a valid APP0 JFIF segment
/// let mut data = Vec::new();
/// data.extend_from_slice(b"JFIF\0");        // Identifier
/// data.push(0x01);                          // Version Major = 1
/// data.push(0x01);                          // Version Minor = 1
/// data.push(0x01);                          // DensityUnits = inches (DPI)
/// data.extend_from_slice(&[0x00, 0x48]);    // XDensity = 72 DPI
/// data.extend_from_slice(&[0x00, 0x48]);    // YDensity = 72 DPI
/// data.push(0x00);                          // ThumbnailWidth = 0
/// data.push(0x00);                          // ThumbnailHeight = 0
///
/// let metadata = parse_app0(&data)?;
/// assert_eq!(metadata.get_string("JFIF:JFIFVersion"), Some("1.01"));
/// assert_eq!(metadata.get_string("JFIF:DensityUnits"), Some("inches"));
/// assert_eq!(metadata.get_integer("JFIF:XDensity"), Some(72));
/// ```
pub fn parse_app0(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Validate minimum segment length
    if data.len() < MIN_JFIF_SEGMENT_LENGTH {
        return Err(ExifToolError::parse_error(format!(
            "APP0 JFIF segment too short: expected at least {} bytes, got {}",
            MIN_JFIF_SEGMENT_LENGTH,
            data.len()
        )));
    }

    // Validate "JFIF\0" identifier at the start of the segment
    if &data[..JFIF_IDENTIFIER.len()] != JFIF_IDENTIFIER {
        return Err(ExifToolError::parse_error(
            "APP0 segment does not contain JFIF identifier",
        ));
    }

    // Extract version information (bytes 5-6)
    let version_major = data[5];
    let version_minor = data[6];
    let version_string = format!("{}.{:02}", version_major, version_minor);
    metadata.insert("JFIF:JFIFVersion", TagValue::String(version_string));

    // Extract density units (byte 7)
    let density_units = data[7];
    let units_string = match density_units {
        0 => "None",
        1 => "inches",
        2 => "cm",
        _ => "Unknown",
    };
    metadata.insert(
        "JFIF:DensityUnits",
        TagValue::String(units_string.to_string()),
    );

    // Extract X and Y density (bytes 8-11, big-endian u16)
    let reader = EndianReader::big_endian(data);
    let x_density = reader.u16_at(8).unwrap_or(0);
    let y_density = reader.u16_at(10).unwrap_or(0);

    metadata.insert("JFIF:XDensity", TagValue::Integer(i64::from(x_density)));
    metadata.insert("JFIF:YDensity", TagValue::Integer(i64::from(y_density)));

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to construct a valid APP0 JFIF segment.
    ///
    /// Creates segment data with the specified values, making test construction
    /// cleaner and more readable.
    fn make_jfif_segment(
        version_major: u8,
        version_minor: u8,
        density_units: u8,
        x_density: u16,
        y_density: u16,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(MIN_JFIF_SEGMENT_LENGTH);
        data.extend_from_slice(JFIF_IDENTIFIER);
        data.push(version_major);
        data.push(version_minor);
        data.push(density_units);
        data.extend_from_slice(&x_density.to_be_bytes());
        data.extend_from_slice(&y_density.to_be_bytes());
        data.push(0x00); // ThumbnailWidth
        data.push(0x00); // ThumbnailHeight
        data
    }

    /// Tests parsing of a valid JFIF 1.01 segment with DPI units.
    #[test]
    fn test_parse_valid_jfif_1_01_dpi() {
        let data = make_jfif_segment(1, 1, 1, 72, 72);

        let result = parse_app0(&data);
        assert!(result.is_ok(), "Parsing should succeed for valid JFIF 1.01");

        let metadata = result.unwrap();

        assert_eq!(
            metadata.get_string("JFIF:JFIFVersion"),
            Some("1.01"),
            "JFIFVersion should be 1.01"
        );
        assert_eq!(
            metadata.get_string("JFIF:DensityUnits"),
            Some("inches"),
            "DensityUnits should be 'inches'"
        );
        assert_eq!(
            metadata.get_integer("JFIF:XDensity"),
            Some(72),
            "XDensity should be 72"
        );
        assert_eq!(
            metadata.get_integer("JFIF:YDensity"),
            Some(72),
            "YDensity should be 72"
        );
    }

    /// Tests parsing with version 1.00 (initial JFIF version).
    #[test]
    fn test_parse_jfif_1_00() {
        let data = make_jfif_segment(1, 0, 1, 96, 96);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("JFIF:JFIFVersion"),
            Some("1.00"),
            "JFIFVersion should be 1.00"
        );
        assert_eq!(
            metadata.get_integer("JFIF:XDensity"),
            Some(96),
            "XDensity should be 96"
        );
    }

    /// Tests parsing with DPCM (dots per centimeter) units.
    #[test]
    fn test_parse_dpcm_units() {
        let data = make_jfif_segment(1, 1, 2, 39, 39);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("JFIF:DensityUnits"),
            Some("cm"),
            "DensityUnits should be 'cm'"
        );
        assert_eq!(
            metadata.get_integer("JFIF:XDensity"),
            Some(39),
            "XDensity should be 39 (100 DPI ~= 39 DPCM)"
        );
    }

    /// Tests parsing with no units (aspect ratio only).
    #[test]
    fn test_parse_no_units() {
        let data = make_jfif_segment(1, 0, 0, 1, 1);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("JFIF:DensityUnits"),
            Some("None"),
            "DensityUnits should be 'None'"
        );
    }

    /// Tests parsing with an unknown/invalid density units value.
    #[test]
    fn test_parse_unknown_density_units() {
        let data = make_jfif_segment(1, 1, 99, 72, 72);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("JFIF:DensityUnits"),
            Some("Unknown"),
            "Unknown density units should be reported as 'Unknown'"
        );
    }

    /// Tests parsing with version 1.02.
    #[test]
    fn test_parse_jfif_1_02() {
        let data = make_jfif_segment(1, 2, 1, 72, 72);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("JFIF:JFIFVersion"),
            Some("1.02"),
            "JFIFVersion should be 1.02"
        );
    }

    /// Tests that a segment without JFIF identifier returns an error.
    #[test]
    fn test_invalid_identifier() {
        let mut data = Vec::new();
        data.extend_from_slice(b"NOPE\0");
        data.extend_from_slice(&[0x01; 9]); // Padding to meet minimum length

        let result = parse_app0(&data);
        assert!(result.is_err(), "Should error on invalid identifier");

        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(
                message.contains("JFIF identifier"),
                "Error message should mention JFIF identifier"
            );
        } else {
            panic!("Expected ParseError variant");
        }
    }

    /// Tests that a segment shorter than minimum length returns an error.
    #[test]
    fn test_segment_too_short() {
        let data = b"JFIF\0".to_vec(); // Only identifier, missing data fields

        let result = parse_app0(&data);
        assert!(result.is_err(), "Should error when segment is too short");

        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(
                message.contains("too short"),
                "Error message should mention segment is too short"
            );
        } else {
            panic!("Expected ParseError variant");
        }
    }

    /// Tests that empty data returns an appropriate error.
    #[test]
    fn test_empty_data() {
        let data: Vec<u8> = vec![];

        let result = parse_app0(&data);
        assert!(result.is_err(), "Should error on empty data");
    }

    /// Tests parsing with maximum values for density.
    #[test]
    fn test_max_density_values() {
        let data = make_jfif_segment(1, 1, 1, 0xFFFF, 0xFFFF);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_integer("JFIF:XDensity"),
            Some(65535),
            "XDensity should handle max u16 value"
        );
        assert_eq!(
            metadata.get_integer("JFIF:YDensity"),
            Some(65535),
            "YDensity should handle max u16 value"
        );
    }

    /// Tests parsing with zero density values.
    #[test]
    fn test_zero_density_values() {
        let data = make_jfif_segment(1, 1, 1, 0, 0);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_integer("JFIF:XDensity"),
            Some(0),
            "XDensity should handle zero"
        );
        assert_eq!(
            metadata.get_integer("JFIF:YDensity"),
            Some(0),
            "YDensity should handle zero"
        );
    }

    /// Tests that segment with extra trailing data is still parsed correctly.
    #[test]
    fn test_extra_trailing_data() {
        let mut data = make_jfif_segment(1, 1, 1, 72, 72);
        // Add extra bytes after the standard structure (thumbnail data)
        data.extend_from_slice(&[0xFF, 0xAA, 0xBB, 0xCC]);

        let result = parse_app0(&data);
        assert!(
            result.is_ok(),
            "Extra trailing data (thumbnail) should be ignored"
        );

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_integer("JFIF:XDensity"),
            Some(72),
            "Should parse correctly despite extra data"
        );
    }

    /// Tests the minimum segment length constant.
    #[test]
    fn test_minimum_segment_length_constant() {
        // "JFIF\0" (5) + Major (1) + Minor (1) + Units (1) + XDensity (2) + YDensity (2) + Width (1) + Height (1) = 14
        assert_eq!(
            MIN_JFIF_SEGMENT_LENGTH, 14,
            "Minimum length should be 14 bytes"
        );
    }

    /// Tests parsing with exactly the minimum required length.
    #[test]
    fn test_exact_minimum_length() {
        let data = make_jfif_segment(1, 1, 1, 72, 72);
        assert_eq!(
            data.len(),
            MIN_JFIF_SEGMENT_LENGTH,
            "Test data should be exactly minimum length"
        );

        let result = parse_app0(&data);
        assert!(result.is_ok(), "Should succeed with exactly minimum length");
    }

    /// Tests parsing with typical real-world values (72 DPI, 1.01 version).
    #[test]
    fn test_typical_real_world_values() {
        // Most common JFIF values: version 1.01, 72 DPI, inches
        let data = make_jfif_segment(1, 1, 1, 72, 72);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("JFIF:JFIFVersion"),
            Some("1.01"),
            "Common version 1.01"
        );
        assert_eq!(
            metadata.get_integer("JFIF:XDensity"),
            Some(72),
            "Common 72 DPI"
        );
    }

    /// Tests parsing with another typical real-world value (96 DPI, common in Windows).
    #[test]
    fn test_windows_typical_96dpi() {
        let data = make_jfif_segment(1, 1, 1, 96, 96);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_integer("JFIF:XDensity"),
            Some(96),
            "Windows typical 96 DPI"
        );
    }

    /// Tests that version bytes are formatted correctly with leading zero for minor.
    #[test]
    fn test_version_formatting_leading_zero() {
        // Version 1.02 should format as "1.02", not "1.2"
        let data = make_jfif_segment(1, 2, 1, 72, 72);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("JFIF:JFIFVersion"),
            Some("1.02"),
            "Minor version should be zero-padded"
        );
    }

    /// Tests that version bytes are formatted correctly with single digit minor.
    #[test]
    fn test_version_formatting_single_digit() {
        // Version 1.01 should format as "1.01", not "1.1"
        let data = make_jfif_segment(1, 1, 1, 72, 72);

        let result = parse_app0(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("JFIF:JFIFVersion"),
            Some("1.01"),
            "Minor version should be zero-padded for single digit"
        );
    }
}
