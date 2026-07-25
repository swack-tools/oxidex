//! JPEG-HDR APP11 segment parser
//!
//! JPEG-HDR is a format for storing High Dynamic Range images using
//! standard JPEG encoding with additional metadata in APP11 segments.

use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;

/// Parse JPEG-HDR APP11 segment
///
/// JPEG-HDR segments contain HDR imaging metadata including version,
/// exposure parameters, and tone mapping information.
///
/// # Arguments
///
/// * `data` - Raw APP11 segment data (should start with "HDR_RI")
/// * `metadata` - Metadata map to populate with extracted values
///
/// # Returns
///
/// Returns `Ok(())` if parsing succeeded, or an error message
pub fn parse_jpeg_hdr_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    // JPEG-HDR segments can start with different identifiers
    // Common ones include "HDR_RI" (HDR Radiance Image)
    if data.len() < 6 {
        return Err("APP11 segment too short".to_string());
    }

    // Check for HDR_RI identifier
    if &data[0..6] == b"HDR_RI" {
        return parse_hdr_radiance_image(&data[6..], metadata);
    }

    // Some JPEG-HDR files may use other identifiers
    // Check for generic "JPEG-HDR" prefix mentioned in the plan
    if data.len() >= 8 && &data[0..8] == b"JPEG-HDR" {
        return parse_jpeg_hdr_generic(&data[8..], metadata);
    }

    Err("Not a recognized JPEG-HDR segment".to_string())
}

/// Parse HDR Radiance Image segment
fn parse_hdr_radiance_image(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.is_empty() {
        return Ok(());
    }

    metadata.insert(
        "JPEG-HDR:Format".to_string(),
        TagValue::String("Radiance Image".to_string()),
    );

    // Try to extract version if present
    if data.len() >= 4 {
        let reader = EndianReader::big_endian(data);
        if let Some(version) = reader.u16_at(0) {
            metadata.insert(
                "JPEG-HDR:Version".to_string(),
                TagValue::String(format!("{}.{}", version >> 8, version & 0xFF)),
            );
        }
    }

    Ok(())
}

/// Parse generic JPEG-HDR segment
fn parse_jpeg_hdr_generic(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.is_empty() {
        return Ok(());
    }

    // Mark as JPEG-HDR format
    metadata.insert(
        "JPEG-HDR:Format".to_string(),
        TagValue::String("HDR".to_string()),
    );

    // Try to extract version information
    if !data.is_empty() {
        let reader = EndianReader::big_endian(data);
        if let Some(version_byte) = reader.u8_at(0)
            && version_byte > 0
        {
            metadata.insert(
                "JPEG-HDR:HDRVersion".to_string(),
                TagValue::Integer(version_byte as i64),
            );
        }
    }

    // Extract parameters if available
    if data.len() >= 8 {
        let reader = EndianReader::big_endian(data);

        // Try to read exposure compensation (often at offset 2-5)
        if let Some(exposure) = reader.u16_at(2)
            && exposure > 0
        {
            metadata.insert(
                "JPEG-HDR:ExposureCompensation".to_string(),
                TagValue::Integer(exposure as i64),
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hdr_radiance_image() {
        let mut data = Vec::new();
        data.extend_from_slice(b"HDR_RI");
        data.extend_from_slice(&[0x01, 0x00]); // Version 1.0

        let mut metadata = MetadataMap::new();
        let result = parse_jpeg_hdr_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("JPEG-HDR:Format").as_deref(),
            Some("Radiance Image")
        );
    }

    #[test]
    fn test_parse_jpeg_hdr_generic() {
        let mut data = Vec::new();
        data.extend_from_slice(b"JPEG-HDR");
        data.extend_from_slice(&[0x01]); // Version 1

        let mut metadata = MetadataMap::new();
        let result = parse_jpeg_hdr_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("JPEG-HDR:Format").as_deref(),
            Some("HDR")
        );
        assert_eq!(metadata.get_integer("JPEG-HDR:HDRVersion"), Some(1));
    }

    #[test]
    fn test_non_hdr_rejected() {
        let data = b"NOTAHDR";
        let mut metadata = MetadataMap::new();
        let result = parse_jpeg_hdr_segment(data, &mut metadata);
        assert!(result.is_err());
    }

    #[test]
    fn test_too_short() {
        let data = b"HDR";
        let mut metadata = MetadataMap::new();
        let result = parse_jpeg_hdr_segment(data, &mut metadata);
        assert!(result.is_err());
    }
}
