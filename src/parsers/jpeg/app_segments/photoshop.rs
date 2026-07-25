//! APP13 Photoshop Image Resource Block (IRB) parser
//!
//! JPEG APP13 segments (marker 0xFFED) can contain Adobe Photoshop metadata
//! stored in Image Resource Blocks (8BIM format). This module extends the
//! existing IPTC parser to extract additional Photoshop-specific tags.
//!
//! # Photoshop IRB Format
//!
//! The APP13 Photoshop segment has the following structure:
//! - Signature: "Photoshop 3.0\0" (14 bytes)
//! - Image Resource Blocks: Sequence of 8BIM blocks
//!
//! Each 8BIM block contains:
//! - Signature: "8BIM" (4 bytes)
//! - Resource ID: 2 bytes (big-endian)
//! - Resource Name: Pascal string (1 byte length + data), padded to even length
//! - Data Size: 4 bytes (big-endian)
//! - Data: variable length, padded to even length

use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use nom::{
    IResult,
    bytes::complete::{tag, take},
    number::complete::{be_u16, be_u32, u8 as nom_u8},
};

// Constants
const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";
const EIGHTBIM_SIGNATURE: &[u8] = b"8BIM";

// Resource IDs for Photoshop tags
const RES_RESOLUTION_INFO: u16 = 0x03ED;
const RES_ALPHA_CHANNELS: u16 = 0x03EE;
const RES_CAPTION: u16 = 0x03F0;
const RES_BORDER_INFO: u16 = 0x03F1;
const RES_BACKGROUND_COLOR: u16 = 0x03F2;
const RES_COPYRIGHT_FLAG: u16 = 0x040A;
const RES_URL: u16 = 0x040B;
const RES_THUMBNAIL: u16 = 0x040C;
const RES_GLOBAL_ANGLE: u16 = 0x040D;
const RES_GLOBAL_ALTITUDE: u16 = 0x0419;
const RES_PRINT_SCALE: u16 = 0x0426;
const RES_PRINT_INFO: u16 = 0x042F;
const RES_PRINT_STYLE: u16 = 0x043B;
const RES_PRINT_FLAGS_INFO: u16 = 0x2710;

/// Represents an Adobe Photoshop Image Resource Block
#[derive(Debug, Clone, PartialEq)]
struct ImageResourceBlock<'a> {
    /// Resource ID (e.g., 0x03ED for Resolution Info)
    id: u16,
    /// Resource name (Pascal string)
    name: &'a [u8],
    /// Resource data payload
    data: &'a [u8],
}

/// Parses a single Adobe Photoshop Image Resource Block (8BIM).
///
/// # Format
/// - Signature: "8BIM" (4 bytes)
/// - ID: 2 bytes (big-endian)
/// - Name: Pascal string (1 byte length + data), padded to even length
/// - Size: 4 bytes (big-endian)
/// - Data: variable length, padded to even length
fn parse_image_resource_block(input: &[u8]) -> IResult<&[u8], ImageResourceBlock<'_>> {
    // Parse 8BIM signature
    let (input, _) = tag(EIGHTBIM_SIGNATURE)(input)?;

    // Parse resource ID (2 bytes, big-endian)
    let (input, id) = be_u16(input)?;

    // Parse Pascal string name (1 byte length + data)
    let (input, name_length) = nom_u8(input)?;
    let (input, name) = take(name_length as usize)(input)?;

    // Pascal string must be padded to even length (including length byte)
    let total_name_length = 1 + name_length as usize;
    let (input, _) = if total_name_length % 2 == 1 {
        take(1usize)(input)?
    } else {
        (input, &b""[..])
    };

    // Parse data size (4 bytes, big-endian)
    let (input, data_size) = be_u32(input)?;

    // Parse data
    let (input, data) = take(data_size as usize)(input)?;

    // Data must be padded to even length
    let (input, _) = if data_size % 2 == 1 {
        take(1usize)(input)?
    } else {
        (input, &b""[..])
    };

    Ok((input, ImageResourceBlock { id, name, data }))
}

/// Parse ResolutionInfo structure (resource 0x03ED)
fn parse_resolution_info(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    if data.len() < 16 {
        return Ok(metadata);
    }

    // Resolution is stored as fixed-point 16.16 (4 bytes each)
    let h_res_raw = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let _h_res = h_res_raw as f64 / 65536.0;

    let h_res_unit = u16::from_be_bytes([data[4], data[5]]);
    let width_unit = u16::from_be_bytes([data[6], data[7]]);

    let v_res_raw = i32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let _v_res = v_res_raw as f64 / 65536.0;

    let _v_res_unit = u16::from_be_bytes([data[12], data[13]]);
    let height_unit = u16::from_be_bytes([data[14], data[15]]);

    metadata.insert(
        "Photoshop:XResolution",
        TagValue::Rational {
            numerator: h_res_raw,
            denominator: 65536,
        },
    );
    metadata.insert(
        "Photoshop:YResolution",
        TagValue::Rational {
            numerator: v_res_raw,
            denominator: 65536,
        },
    );

    // Resolution unit: 1=pixels/inch, 2=pixels/cm
    let res_unit_str = match h_res_unit {
        1 => "inches",
        2 => "cm",
        _ => "Unknown",
    };
    metadata.insert(
        "Photoshop:ResolutionUnit",
        TagValue::String(res_unit_str.to_string()),
    );

    // Width/Height unit: 1=inches, 2=cm, 3=points, 4=picas, 5=columns
    let width_unit_str = match width_unit {
        1 => "inches",
        2 => "cm",
        3 => "points",
        4 => "picas",
        5 => "columns",
        _ => "Unknown",
    };
    metadata.insert(
        "Photoshop:WidthUnit",
        TagValue::String(width_unit_str.to_string()),
    );

    let height_unit_str = match height_unit {
        1 => "inches",
        2 => "cm",
        3 => "points",
        4 => "picas",
        5 => "columns",
        _ => "Unknown",
    };
    metadata.insert(
        "Photoshop:HeightUnit",
        TagValue::String(height_unit_str.to_string()),
    );

    Ok(metadata)
}

/// Parse GlobalAngle (resource 0x040D)
fn parse_global_angle(data: &[u8]) -> Result<i32> {
    if data.len() < 4 {
        return Err(ExifToolError::parse_error("GlobalAngle data too short"));
    }
    Ok(i32::from_be_bytes([data[0], data[1], data[2], data[3]]))
}

/// Parse GlobalAltitude (resource 0x0419)
fn parse_global_altitude(data: &[u8]) -> Result<i32> {
    if data.len() < 4 {
        return Err(ExifToolError::parse_error("GlobalAltitude data too short"));
    }
    Ok(i32::from_be_bytes([data[0], data[1], data[2], data[3]]))
}

/// Parse CopyrightFlag (resource 0x040A)
fn parse_copyright_flag(data: &[u8]) -> Result<bool> {
    if data.is_empty() {
        return Err(ExifToolError::parse_error("CopyrightFlag data too short"));
    }
    Ok(data[0] != 0)
}

/// Parse URL string (resource 0x040B)
fn parse_url(data: &[u8]) -> Result<String> {
    std::str::from_utf8(data)
        .map(|s| s.trim_end_matches('\0').to_string())
        .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in URL"))
}

/// Parse caption/description string (resource 0x03F0)
fn parse_caption(data: &[u8]) -> Result<String> {
    // Caption is stored as a Pascal string
    if data.is_empty() {
        return Ok(String::new());
    }

    let length = data[0] as usize;
    if length + 1 > data.len() {
        return Ok(String::new());
    }

    std::str::from_utf8(&data[1..=length])
        .map(|s| s.to_string())
        .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in caption"))
}

/// Parse PrintStyle descriptor (resource 0x043B)
fn parse_print_style(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // PrintStyle is a descriptor structure - simplified parsing
    // For now, just note its presence
    if !data.is_empty() {
        metadata.insert(
            "Photoshop:PrintStylePresent",
            TagValue::String("Yes".to_string()),
        );
    }

    Ok(metadata)
}

/// Parse PrintFlags (resource 0x2710)
fn parse_print_flags(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    if data.len() >= 2 {
        let flags = u16::from_be_bytes([data[0], data[1]]);
        metadata.insert("Photoshop:PrintFlags", TagValue::Integer(flags as i64));

        // Parse individual flag bits
        if flags & 0x0001 != 0 {
            metadata.insert(
                "Photoshop:PrintLabels",
                TagValue::String("True".to_string()),
            );
        }
        if flags & 0x0002 != 0 {
            metadata.insert(
                "Photoshop:PrintCropMarks",
                TagValue::String("True".to_string()),
            );
        }
        if flags & 0x0004 != 0 {
            metadata.insert(
                "Photoshop:PrintColorBars",
                TagValue::String("True".to_string()),
            );
        }
        if flags & 0x0008 != 0 {
            metadata.insert(
                "Photoshop:PrintRegistrationMarks",
                TagValue::String("True".to_string()),
            );
        }
        if flags & 0x0010 != 0 {
            metadata.insert(
                "Photoshop:PrintNegative",
                TagValue::String("True".to_string()),
            );
        }
    }

    Ok(metadata)
}

/// Extracts Photoshop metadata from APP13 segment data.
///
/// This function parses Photoshop Image Resource Blocks (8BIM) from APP13
/// segments and extracts various Photoshop-specific metadata tags.
///
/// # Parameters
///
/// - `data`: Raw APP13 segment data (must start with "Photoshop 3.0\0")
///
/// # Returns
///
/// `MetadataMap` containing extracted Photoshop tags, or error if parsing fails.
///
/// # Errors
///
/// Returns `ParseError` if:
/// - Data doesn't start with Photoshop signature
/// - 8BIM blocks are malformed
pub fn parse_photoshop_irb(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Check for Photoshop signature
    if !data.starts_with(PHOTOSHOP_SIGNATURE) {
        return Err(ExifToolError::parse_error("Not a Photoshop IRB segment"));
    }

    // Skip past the Photoshop signature
    let mut current = &data[PHOTOSHOP_SIGNATURE.len()..];

    // Parse all 8BIM resource blocks
    while current.len() > 4 {
        // Check if this looks like a 8BIM block
        if !current.starts_with(EIGHTBIM_SIGNATURE) {
            break;
        }

        match parse_image_resource_block(current) {
            Ok((remaining, block)) => {
                // Parse specific resource types
                match block.id {
                    RES_RESOLUTION_INFO => {
                        if let Ok(res_metadata) = parse_resolution_info(block.data) {
                            // TODO: extend not available, iterate instead
                            for (k, v) in res_metadata.iter() {
                                metadata.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    RES_COPYRIGHT_FLAG => {
                        if let Ok(flag) = parse_copyright_flag(block.data) {
                            metadata.insert(
                                "Photoshop:CopyrightFlag",
                                TagValue::String(if flag { "True" } else { "False" }.to_string()),
                            );
                        }
                    }
                    RES_URL => {
                        if let Ok(url) = parse_url(block.data) {
                            metadata.insert("Photoshop:URL", TagValue::String(url));
                        }
                    }
                    RES_CAPTION => {
                        if let Ok(caption) = parse_caption(block.data)
                            && !caption.is_empty()
                        {
                            metadata.insert("Photoshop:Caption", TagValue::String(caption));
                        }
                    }
                    RES_GLOBAL_ANGLE => {
                        if let Ok(angle) = parse_global_angle(block.data) {
                            metadata
                                .insert("Photoshop:GlobalAngle", TagValue::Integer(angle as i64));
                        }
                    }
                    RES_GLOBAL_ALTITUDE => {
                        if let Ok(altitude) = parse_global_altitude(block.data) {
                            metadata.insert(
                                "Photoshop:GlobalAltitude",
                                TagValue::Integer(altitude as i64),
                            );
                        }
                    }
                    RES_PRINT_STYLE => {
                        if let Ok(style_metadata) = parse_print_style(block.data) {
                            // TODO: extend not available, iterate instead
                            for (k, v) in style_metadata.iter() {
                                metadata.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    RES_PRINT_FLAGS_INFO => {
                        if let Ok(flags_metadata) = parse_print_flags(block.data) {
                            // TODO: extend not available, iterate instead
                            for (k, v) in flags_metadata.iter() {
                                metadata.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    RES_THUMBNAIL => {
                        // Just note presence, don't extract full thumbnail
                        metadata.insert(
                            "Photoshop:ThumbnailPresent",
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    RES_ALPHA_CHANNELS => {
                        if !block.data.is_empty() {
                            metadata.insert(
                                "Photoshop:AlphaChannelsPresent",
                                TagValue::String("Yes".to_string()),
                            );
                        }
                    }
                    RES_PRINT_INFO => {
                        if !block.data.is_empty() {
                            metadata.insert(
                                "Photoshop:PrintInfoPresent",
                                TagValue::String("Yes".to_string()),
                            );
                        }
                    }
                    _ => {
                        // For other resource types, just note their presence
                        // This helps with debugging and completeness
                    }
                }

                current = remaining;
            }
            Err(_) => {
                // Failed to parse block, stop processing
                break;
            }
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_image_resource_block_minimal() {
        let mut data = Vec::new();
        data.extend_from_slice(b"8BIM");
        data.extend_from_slice(&[0x04, 0x0D]); // GlobalAngle
        data.push(0x00); // Empty name
        data.push(0x00); // Padding
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Size: 4
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x1E]); // Angle: 30

        let result = parse_image_resource_block(&data);
        assert!(result.is_ok());

        let (remaining, block) = result.unwrap();
        assert_eq!(block.id, 0x040D);
        assert_eq!(block.data, &[0x00, 0x00, 0x00, 0x1E]);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_parse_resolution_info() {
        let mut data = Vec::new();
        // H resolution: 72.0 (72 * 65536 = 4718592 = 0x00480000)
        data.extend_from_slice(&[0x00, 0x48, 0x00, 0x00]);
        data.extend_from_slice(&[0x00, 0x01]); // H unit: inches
        data.extend_from_slice(&[0x00, 0x01]); // Width unit: inches
        // V resolution: 72.0
        data.extend_from_slice(&[0x00, 0x48, 0x00, 0x00]);
        data.extend_from_slice(&[0x00, 0x01]); // V unit: inches
        data.extend_from_slice(&[0x00, 0x01]); // Height unit: inches

        let result = parse_resolution_info(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_string("Photoshop:ResolutionUnit").as_deref(),
            Some("inches")
        );
    }

    #[test]
    fn test_parse_global_angle() {
        let data = [0x00, 0x00, 0x00, 0x5A]; // 90 degrees
        let result = parse_global_angle(&data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 90);
    }

    #[test]
    fn test_parse_copyright_flag() {
        let data_true = [0x01];
        let data_false = [0x00];

        assert_eq!(parse_copyright_flag(&data_true).unwrap(), true);
        assert_eq!(parse_copyright_flag(&data_false).unwrap(), false);
    }

    #[test]
    fn test_parse_url() {
        let data = b"https://example.com\0";
        let result = parse_url(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com");
    }

    #[test]
    fn test_parse_photoshop_irb_complete() {
        let mut data = Vec::new();

        // Photoshop signature
        data.extend_from_slice(PHOTOSHOP_SIGNATURE);

        // GlobalAngle block
        data.extend_from_slice(b"8BIM");
        data.extend_from_slice(&[0x04, 0x0D]); // ID: GlobalAngle
        data.push(0x00); // Empty name
        data.push(0x00); // Padding
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Size: 4
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x78]); // Angle: 120

        // CopyrightFlag block
        data.extend_from_slice(b"8BIM");
        data.extend_from_slice(&[0x04, 0x0A]); // ID: CopyrightFlag
        data.push(0x00); // Empty name
        data.push(0x00); // Padding
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // Size: 1
        data.push(0x01); // True
        data.push(0x00); // Padding

        let result = parse_photoshop_irb(&data);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_integer("Photoshop:GlobalAngle"), Some(120));
        assert_eq!(
            metadata.get_string("Photoshop:CopyrightFlag").as_deref(),
            Some("True")
        );
    }

    #[test]
    fn test_parse_photoshop_irb_invalid_signature() {
        let data = b"NotPhotoshop";
        let result = parse_photoshop_irb(data);
        assert!(result.is_err());
    }
}
