//! JFIF (JPEG File Interchange Format) APP0 segment parser
//!
//! This module parses JPEG APP0 (JFIF) segments to extract:
//! - JFIF version
//! - Resolution units (dots per inch, dots per cm, or no units)
//! - X and Y density values
//! - Thumbnail dimensions

use crate::core::{MetadataMap, TagValue};
use nom::{
    IResult,
    bytes::complete::tag,
    number::complete::{be_u8, be_u16},
};

/// Parse JFIF (APP0) segment data and extract metadata
///
/// # Arguments
///
/// * `data` - Raw APP0 segment data (should start with "JFIF\0")
/// * `metadata` - Metadata map to populate with extracted values
///
/// # Returns
///
/// Returns `Ok(())` if parsing succeeded, or an error message
pub fn parse_jfif_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    match parse_jfif(data) {
        Ok((_, jfif)) => {
            // Add version
            metadata.insert(
                "JFIF:JFIFVersion".to_string(),
                TagValue::String(format!("{}.{:02}", jfif.version_major, jfif.version_minor)),
            );

            // Add resolution unit
            let unit_str = match jfif.density_units {
                0 => "None",
                1 => "inches",
                2 => "cm",
                _ => "Unknown",
            };
            metadata.insert(
                "JFIF:ResolutionUnit".to_string(),
                TagValue::String(unit_str.to_string()),
            );

            // Add X and Y density
            metadata.insert(
                "JFIF:XResolution".to_string(),
                TagValue::Integer(jfif.x_density as i64),
            );
            metadata.insert(
                "JFIF:YResolution".to_string(),
                TagValue::Integer(jfif.y_density as i64),
            );

            // Add thumbnail dimensions
            metadata.insert(
                "JFIF:ThumbnailWidth".to_string(),
                TagValue::Integer(jfif.thumbnail_width as i64),
            );
            metadata.insert(
                "JFIF:ThumbnailHeight".to_string(),
                TagValue::Integer(jfif.thumbnail_height as i64),
            );

            Ok(())
        }
        Err(e) => Err(format!("Failed to parse JFIF segment: {}", e)),
    }
}

/// JFIF segment structure
#[derive(Debug, Clone, PartialEq)]
struct JfifSegment {
    version_major: u8,
    version_minor: u8,
    density_units: u8,
    x_density: u16,
    y_density: u16,
    thumbnail_width: u8,
    thumbnail_height: u8,
}

/// Parse JFIF segment using nom
fn parse_jfif(input: &[u8]) -> IResult<&[u8], JfifSegment> {
    // Parse JFIF identifier ("JFIF\0")
    let (input, _) = tag(&b"JFIF\0"[..])(input)?;

    // Parse version (2 bytes: major, minor)
    let (input, version_major) = be_u8(input)?;
    let (input, version_minor) = be_u8(input)?;

    // Parse density units (1 byte)
    let (input, density_units) = be_u8(input)?;

    // Parse X and Y density (2 bytes each)
    let (input, x_density) = be_u16(input)?;
    let (input, y_density) = be_u16(input)?;

    // Parse thumbnail dimensions (1 byte each)
    let (input, thumbnail_width) = be_u8(input)?;
    let (input, thumbnail_height) = be_u8(input)?;

    Ok((
        input,
        JfifSegment {
            version_major,
            version_minor,
            density_units,
            x_density,
            y_density,
            thumbnail_width,
            thumbnail_height,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jfif_basic() {
        // Create a basic JFIF segment
        let data = [
            b'J', b'F', b'I', b'F', 0x00, // Identifier
            0x01, 0x01, // Version 1.01
            0x01, // Units: dots per inch
            0x00, 0x48, // X density: 72 dpi
            0x00, 0x48, // Y density: 72 dpi
            0x00, // Thumbnail width: 0
            0x00, // Thumbnail height: 0
        ];

        let mut metadata = MetadataMap::new();
        let result = parse_jfif_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(metadata.get_string("JFIF:JFIFVersion"), Some("1.01"));
        assert_eq!(metadata.get_string("JFIF:ResolutionUnit"), Some("inches"));
        assert_eq!(metadata.get_integer("JFIF:XResolution"), Some(72));
        assert_eq!(metadata.get_integer("JFIF:YResolution"), Some(72));
        assert_eq!(metadata.get_integer("JFIF:ThumbnailWidth"), Some(0));
        assert_eq!(metadata.get_integer("JFIF:ThumbnailHeight"), Some(0));
    }

    #[test]
    fn test_parse_jfif_with_thumbnail() {
        // JFIF with thumbnail
        let data = [
            b'J', b'F', b'I', b'F', 0x00, // Identifier
            0x01, 0x02, // Version 1.02
            0x02, // Units: dots per cm
            0x00, 0x64, // X density: 100 dpcm
            0x00, 0x64, // Y density: 100 dpcm
            0x10, // Thumbnail width: 16
            0x10, // Thumbnail height: 16
        ];

        let mut metadata = MetadataMap::new();
        let result = parse_jfif_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(metadata.get_string("JFIF:JFIFVersion"), Some("1.02"));
        assert_eq!(metadata.get_string("JFIF:ResolutionUnit"), Some("cm"));
        assert_eq!(metadata.get_integer("JFIF:ThumbnailWidth"), Some(16));
        assert_eq!(metadata.get_integer("JFIF:ThumbnailHeight"), Some(16));
    }

    #[test]
    fn test_parse_jfif_invalid_identifier() {
        let data = [
            b'N', b'O', b'P', b'E', 0x00, // Invalid identifier
            0x01, 0x01, 0x01, 0x00, 0x48, 0x00, 0x48, 0x00, 0x00,
        ];

        let mut metadata = MetadataMap::new();
        let result = parse_jfif_segment(&data, &mut metadata);

        assert!(result.is_err());
    }
}
