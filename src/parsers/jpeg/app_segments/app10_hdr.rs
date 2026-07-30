//! APP10 HDR GainCurve segment parser
//!
//! JPEG APP10 segments can contain HDR (High Dynamic Range) gain curve data.
//! This module provides parsing functionality to extract HDR gain curve
//! information from APP10 segments.
//!
//! # HDR Gain Curve Format
//!
//! The HDR gain curve data is typically stored as binary data within the APP10
//! segment. This parser extracts:
//! - `HDRGainCurve`: The raw binary gain curve data
//! - `HDRGainCurveSize`: The size of the gain curve data in bytes
//!
//! # Example
//!
//! ```ignore
//! use oxidex::parsers::jpeg::app_segments::app10_hdr::parse_app10_hdr;
//!
//! let data: &[u8] = &[/* APP10 HDR segment data */];
//! let metadata = parse_app10_hdr(data)?;
//!
//! if let Some(size) = metadata.get_integer("HDR:GainCurveSize") {
//!     println!("Gain curve size: {} bytes", size);
//! }
//! ```

use crate::core::MetadataMap;
use crate::core::TagValue;
use crate::error::Result;

/// Minimum required length for a valid APP10 HDR segment.
///
/// The segment must contain at least an identifier to be recognized
/// as HDR data.
const MIN_HDR_SEGMENT_LENGTH: usize = 4;

/// Known identifier prefix for HDR gain curve data in APP10 segments.
///
/// Some implementations use "HDR\0" as the identifier prefix.
const HDR_IDENTIFIER: &[u8] = b"HDR\0";

/// Alternative identifier for AROT (Android HDR) gain map data.
///
/// Some Android devices store HDR gain map data with this prefix.
const AROT_IDENTIFIER: &[u8] = b"AROT";

/// Offset of `HDRGainCurveSize` within an AROT segment (`int32u`, big-endian).
///
/// From ExifTool's `Image::ExifTool::JPEG::HDRGainInfo` table, whose keys are
/// offsets from the start of the segment -- the four-byte identifier and the
/// two null bytes that follow it are counted, not skipped.
const AROT_SIZE_OFFSET: usize = 6;

/// Offset of `HDRGainCurve` within an AROT segment.
///
/// The curve is `int32uRev[$val{6}]`: `HDRGainCurveSize` little-endian `u32`s,
/// byte-reversed relative to the big-endian size field that precedes them.
const AROT_CURVE_OFFSET: usize = 10;

/// Parses APP10 HDR segment data and extracts gain curve metadata.
///
/// This function attempts to parse the provided data as an APP10 HDR segment
/// containing gain curve information. It recognizes multiple HDR formats
/// including standard HDR and Android AROT gain maps.
///
/// # Arguments
///
/// * `data` - Raw APP10 segment data (excluding the APP10 marker and length bytes)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - A metadata map containing extracted HDR tags:
///   - `HDR:GainCurve` - Binary data containing the gain curve (TagValue::Binary)
///   - `HDR:GainCurveSize` - Size of the gain curve data in bytes (TagValue::Integer)
///   - `HDR:Format` - The detected HDR format identifier (TagValue::String)
///
/// * `Err(ExifToolError)` - If the data cannot be parsed as valid HDR data
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app10_hdr::parse_app10_hdr;
///
/// // Example HDR data with identifier
/// let mut data = Vec::new();
/// data.extend_from_slice(b"HDR\0");
/// data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // Curve data
///
/// let metadata = parse_app10_hdr(&data)?;
/// assert_eq!(metadata.get_integer("HDR:GainCurveSize"), Some(4));
/// ```
pub fn parse_app10_hdr(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Validate minimum segment length
    if data.len() < MIN_HDR_SEGMENT_LENGTH {
        return Err(crate::error::ExifToolError::parse_error(
            "APP10 HDR segment too short to contain valid data",
        ));
    }

    // Attempt to detect and parse known HDR formats
    if data.len() >= HDR_IDENTIFIER.len() && &data[..HDR_IDENTIFIER.len()] == HDR_IDENTIFIER {
        // Standard HDR format with "HDR\0" prefix
        parse_standard_hdr(&data[HDR_IDENTIFIER.len()..], &mut metadata)?;
        metadata.insert("HDR:Format", TagValue::String("HDR".to_string()));
    } else if data.len() >= AROT_IDENTIFIER.len()
        && &data[..AROT_IDENTIFIER.len()] == AROT_IDENTIFIER
    {
        // Android AROT HDR gain map format.
        //
        // Pass the WHOLE segment, identifier included: ExifTool's HDRGainInfo
        // table indexes from the start of the segment (offset 6 is the size,
        // offset 10 the curve), so stripping the prefix here would silently
        // shift every field by four bytes.
        parse_arot_hdr(data, &mut metadata)?;
        metadata.insert("HDR:Format", TagValue::String("AROT".to_string()));
    } else {
        // Unknown format - attempt generic parsing
        // Store the entire data as the gain curve for formats we don't recognize
        parse_generic_hdr(data, &mut metadata)?;
        metadata.insert("HDR:Format", TagValue::String("Unknown".to_string()));
    }

    Ok(metadata)
}

/// Parses standard HDR format data (after the "HDR\0" identifier).
///
/// This function extracts gain curve data from the standard HDR format.
///
/// # Arguments
///
/// * `data` - HDR payload data (after the identifier has been stripped)
/// * `metadata` - Metadata map to populate with extracted values
///
/// # Returns
///
/// * `Ok(())` if parsing succeeds
/// * `Err(ExifToolError)` if the data is malformed
fn parse_standard_hdr(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // The gain curve data follows immediately after the identifier
    let curve_size = data.len();

    // Store the gain curve size
    metadata.insert("HDR:GainCurveSize", TagValue::Integer(curve_size as i64));
    metadata.insert(
        "APP10:HDRGainCurveSize".to_string(),
        TagValue::Integer(curve_size as i64),
    );

    // Store the gain curve binary data if present
    if !data.is_empty() {
        metadata.insert("HDR:GainCurve", TagValue::Binary(data.to_vec()));
        metadata.insert(
            "APP10:HDRGainCurve".to_string(),
            TagValue::Binary(data.to_vec()),
        );
    }

    Ok(())
}

/// Parses Android AROT HDR gain map data.
///
/// AROT (possibly "Android ROTation" or HDR-related) segments contain
/// HDR gain map information used by Android devices for HDR rendering.
///
/// # Arguments
///
/// * `data` - AROT payload data (after the identifier has been stripped)
/// * `metadata` - Metadata map to populate with extracted values
///
/// # Returns
///
/// * `Ok(())` if parsing succeeds
/// * `Err(ExifToolError)` if the data is malformed
fn parse_arot_hdr(segment: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // Offsets are ExifTool's, counted from the start of the segment. Anything
    // shorter than the curve offset carries no gain curve at all, and
    // ExifTool's own condition (/^AROT\0\0.{4}/s) declines it too.
    if segment.len() < AROT_CURVE_OFFSET {
        return Ok(());
    }

    // The declared entry count, NOT a byte length. The previous code reported
    // segment.len() here, which is why an iPhone 11 Pro answered 760 (the
    // segment) where ExifTool answers 188 (the entries).
    let curve_size = u32::from_be_bytes([
        segment[AROT_SIZE_OFFSET],
        segment[AROT_SIZE_OFFSET + 1],
        segment[AROT_SIZE_OFFSET + 2],
        segment[AROT_SIZE_OFFSET + 3],
    ]);

    // `AROT` is ExifTool's family-1 group for this table (family 0 is APP10),
    // and family 1 is what the comparison harness matches on.
    metadata.insert(
        "AROT:HDRGainCurveSize",
        TagValue::Integer(curve_size.into()),
    );
    metadata.insert("HDR:GainCurveSize", TagValue::Integer(curve_size.into()));

    // Format is int32uRev[$val{6}]: `curve_size` little-endian u32s. Trust the
    // declared count only as far as the segment actually reaches, so a
    // truncated or hostile segment yields a short curve rather than a panic.
    let available = (segment.len() - AROT_CURVE_OFFSET) / 4;
    let entries = (curve_size as usize).min(available);
    if entries == 0 {
        return Ok(());
    }

    let curve: Vec<u32> = segment[AROT_CURVE_OFFSET..]
        .chunks_exact(4)
        .take(entries)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let rendered = curve
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    metadata.insert("AROT:HDRGainCurve", TagValue::String(rendered.clone()));
    metadata.insert("HDR:GainCurve", TagValue::String(rendered));

    Ok(())
}

/// Parses generic/unknown HDR format data.
///
/// When the HDR format is not recognized, this function stores the entire
/// segment data as the gain curve, allowing applications to handle the
/// raw data appropriately.
///
/// # Arguments
///
/// * `data` - Complete APP10 segment data
/// * `metadata` - Metadata map to populate with extracted values
///
/// # Returns
///
/// * `Ok(())` if parsing succeeds
/// * `Err(ExifToolError)` if the data is malformed
fn parse_generic_hdr(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    let curve_size = data.len();

    metadata.insert("HDR:GainCurveSize", TagValue::Integer(curve_size as i64));
    metadata.insert(
        "APP10:HDRGainCurveSize".to_string(),
        TagValue::Integer(curve_size as i64),
    );

    if !data.is_empty() {
        metadata.insert("HDR:GainCurve", TagValue::Binary(data.to_vec()));
        metadata.insert(
            "APP10:HDRGainCurve".to_string(),
            TagValue::Binary(data.to_vec()),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests parsing of a valid standard HDR segment with "HDR\0" prefix.
    #[test]
    fn test_parse_standard_hdr_segment() {
        // Construct a valid HDR segment: identifier + curve data
        let mut data = Vec::new();
        data.extend_from_slice(b"HDR\0");
        data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);

        let result = parse_app10_hdr(&data);
        assert!(result.is_ok(), "Parsing should succeed for valid HDR data");

        let metadata = result.unwrap();

        // Verify format is detected correctly
        assert_eq!(
            metadata.get_string("HDR:Format"),
            Some("HDR"),
            "Format should be 'HDR'"
        );

        // Verify gain curve size is correct (excluding the 4-byte identifier)
        assert_eq!(
            metadata.get_integer("HDR:GainCurveSize"),
            Some(8),
            "Gain curve size should be 8 bytes"
        );

        // Verify gain curve binary data is present
        let curve = metadata.get("HDR:GainCurve");
        assert!(curve.is_some(), "Gain curve data should be present");
        if let Some(TagValue::Binary(binary)) = curve {
            assert_eq!(binary.len(), 8, "Binary data should be 8 bytes");
            assert_eq!(binary, &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
        } else {
            panic!("Expected Binary variant for HDR:GainCurve");
        }
    }

    /// Builds an AROT segment: identifier, two nulls, big-endian entry count,
    /// then the little-endian (`int32uRev`) curve entries themselves.
    fn arot_segment(declared: u32, entries: &[u32]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"AROT");
        data.extend_from_slice(&[0x00, 0x00]);
        data.extend_from_slice(&declared.to_be_bytes());
        for e in entries {
            data.extend_from_slice(&e.to_le_bytes());
        }
        data
    }

    /// `HDRGainCurveSize` is the declared entry count read at offset 6, not the
    /// length of the segment. The two are easy to confuse and were confused:
    /// an iPhone 11 Pro reported 760 (its segment) where ExifTool reports 188.
    #[test]
    fn test_parse_arot_hdr_segment() {
        let data = arot_segment(3, &[2628, 5243, 7902]);
        let metadata = parse_app10_hdr(&data).expect("AROT segment should parse");

        assert_eq!(metadata.get_string("HDR:Format"), Some("AROT"));
        assert_eq!(
            metadata.get_integer("AROT:HDRGainCurveSize"),
            Some(3),
            "size is the declared entry count, not the {} byte segment",
            data.len()
        );
        assert_eq!(
            metadata.get_string("AROT:HDRGainCurve"),
            Some("2628 5243 7902"),
            "curve entries are int32uRev -- little-endian"
        );
    }

    /// The real bytes from the head of an iPhone 11 Pro APP10 segment, whose
    /// misreading is what this parser was rewritten to fix.
    #[test]
    fn test_parse_arot_hdr_real_device_prefix() {
        let data = [
            0x41, 0x52, 0x4f, 0x54, 0x00, 0x00, 0x00, 0x00, 0x00, 0xbc, // AROT..size=188
            0x44, 0x0a, 0x00, 0x00, // 2628
            0x7b, 0x14, 0x00, 0x00, // 5243
        ];
        let metadata = parse_app10_hdr(&data).expect("AROT segment should parse");

        assert_eq!(
            metadata.get_integer("AROT:HDRGainCurveSize"),
            Some(188),
            "ExifTool reports 188 for this file"
        );
        assert_eq!(
            metadata.get_string("AROT:HDRGainCurve"),
            Some("2628 5243"),
            "a declared count of 188 must not read past the 2 entries present"
        );
    }

    /// A segment that stops before the curve offset yields no curve tags rather
    /// than an error or a panic -- ExifTool's condition declines it as well.
    #[test]
    fn test_parse_arot_hdr_truncated_before_curve() {
        let data = b"AROT\0\0\0\0";
        let metadata = parse_app10_hdr(data).expect("short AROT segment should not error");

        assert_eq!(metadata.get_string("HDR:Format"), Some("AROT"));
        assert_eq!(metadata.get_integer("AROT:HDRGainCurveSize"), None);
        assert_eq!(metadata.get_string("AROT:HDRGainCurve"), None);
    }

    /// Tests parsing of an unknown/generic HDR format.
    #[test]
    fn test_parse_unknown_hdr_format() {
        // Data without a recognized identifier
        let data = vec![0x00, 0x11, 0x22, 0x33, 0x44, 0x55];

        let result = parse_app10_hdr(&data);
        assert!(result.is_ok(), "Parsing should succeed for unknown format");

        let metadata = result.unwrap();

        assert_eq!(
            metadata.get_string("HDR:Format"),
            Some("Unknown"),
            "Format should be 'Unknown'"
        );

        assert_eq!(
            metadata.get_integer("HDR:GainCurveSize"),
            Some(6),
            "Gain curve size should include all data"
        );
    }

    /// Tests that a segment that is too short returns an error.
    #[test]
    fn test_segment_too_short() {
        // Only 3 bytes - less than minimum required
        let data = vec![0x01, 0x02, 0x03];

        let result = parse_app10_hdr(&data);
        assert!(result.is_err(), "Should return error for segment too short");

        if let Err(crate::error::ExifToolError::ParseError { message, .. }) = result {
            assert!(
                message.contains("too short"),
                "Error message should mention segment is too short"
            );
        } else {
            panic!("Expected ParseError variant");
        }
    }

    /// Tests parsing of an HDR segment with empty curve data.
    #[test]
    fn test_hdr_with_empty_curve() {
        // Just the identifier, no curve data
        let data = b"HDR\0".to_vec();

        let result = parse_app10_hdr(&data);
        assert!(result.is_ok(), "Parsing should succeed for empty curve");

        let metadata = result.unwrap();

        assert_eq!(
            metadata.get_string("HDR:Format"),
            Some("HDR"),
            "Format should be 'HDR'"
        );

        assert_eq!(
            metadata.get_integer("HDR:GainCurveSize"),
            Some(0),
            "Gain curve size should be 0"
        );

        // Binary data should not be present for empty curve
        assert!(
            metadata.get("HDR:GainCurve").is_none(),
            "Gain curve should not be present when empty"
        );
    }

    /// Tests parsing of a large gain curve.
    #[test]
    fn test_large_gain_curve() {
        let mut data = Vec::new();
        data.extend_from_slice(b"HDR\0");

        // Create a 1KB gain curve
        let curve_data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        data.extend_from_slice(&curve_data);

        let result = parse_app10_hdr(&data);
        assert!(result.is_ok(), "Parsing should succeed for large curve");

        let metadata = result.unwrap();

        assert_eq!(
            metadata.get_integer("HDR:GainCurveSize"),
            Some(1024),
            "Gain curve size should be 1024 bytes"
        );

        if let Some(TagValue::Binary(binary)) = metadata.get("HDR:GainCurve") {
            assert_eq!(binary.len(), 1024, "Binary data should be 1024 bytes");
        } else {
            panic!("Expected Binary variant for large gain curve");
        }
    }

    /// Tests that the minimum segment length constant is reasonable.
    #[test]
    fn test_minimum_segment_length_constant() {
        assert_eq!(
            MIN_HDR_SEGMENT_LENGTH, 4,
            "Minimum length should be 4 bytes for identifier"
        );
    }

    /// Tests parsing with exactly the minimum required length.
    #[test]
    fn test_exact_minimum_length() {
        // Exactly 4 bytes - should be accepted
        let data = vec![0x00, 0x00, 0x00, 0x00];

        let result = parse_app10_hdr(&data);
        assert!(result.is_ok(), "Should succeed with exactly minimum length");
    }

    /// Tests that HDR identifier matching is case-sensitive.
    #[test]
    fn test_identifier_case_sensitivity() {
        // Lowercase "hdr\0" should not match
        let data = b"hdr\0\x01\x02\x03\x04".to_vec();

        let result = parse_app10_hdr(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();

        // Should be treated as unknown format, not HDR
        assert_eq!(
            metadata.get_string("HDR:Format"),
            Some("Unknown"),
            "Lowercase identifier should not match"
        );
    }

    /// Tests the internal parse_standard_hdr function.
    #[test]
    fn test_parse_standard_hdr_internal() {
        let mut metadata = MetadataMap::new();
        let data = vec![0x10, 0x20, 0x30, 0x40];

        let result = parse_standard_hdr(&data, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(metadata.get_integer("HDR:GainCurveSize"), Some(4));
        assert!(metadata.get("HDR:GainCurve").is_some());
    }

    /// Tests the internal parse_arot_hdr function, which takes the WHOLE
    /// segment -- identifier included -- so that ExifTool's offsets apply
    /// unshifted.
    #[test]
    fn test_parse_arot_hdr_internal() {
        let mut metadata = MetadataMap::new();
        let data = arot_segment(2, &[0xDEAD, 0xBEEF]);

        let result = parse_arot_hdr(&data, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(metadata.get_integer("AROT:HDRGainCurveSize"), Some(2));
        assert_eq!(
            metadata.get_string("AROT:HDRGainCurve"),
            Some("57005 48879")
        );
    }

    /// Tests the internal parse_generic_hdr function.
    #[test]
    fn test_parse_generic_hdr_internal() {
        let mut metadata = MetadataMap::new();
        let data = vec![0xFF, 0xEE, 0xDD];

        let result = parse_generic_hdr(&data, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(metadata.get_integer("HDR:GainCurveSize"), Some(3));
    }

    /// Tests that empty data after identifier is handled correctly.
    #[test]
    fn test_empty_data_internal_functions() {
        let mut metadata = MetadataMap::new();
        let empty_data: Vec<u8> = vec![];

        let result = parse_standard_hdr(&empty_data, &mut metadata);
        assert!(result.is_ok());
        assert_eq!(metadata.get_integer("HDR:GainCurveSize"), Some(0));
        assert!(metadata.get("HDR:GainCurve").is_none());
    }
}
