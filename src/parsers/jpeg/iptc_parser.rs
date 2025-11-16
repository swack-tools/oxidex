//! IPTC segment parser for JPEG
//!
//! This module handles parsing of IPTC data in JPEG APP13 segments.
//! IPTC data is stored in Adobe Photoshop Image Resource Blocks (8BIM).

use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::segment_parser::Segment;
use nom::{
    bytes::complete::{tag, take},
    number::complete::{be_u16, be_u32, u8 as nom_u8},
    IResult,
};

// Constants
const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";
const EIGHTBIM_SIGNATURE: &[u8] = b"8BIM";
const IPTC_RESOURCE_ID: u16 = 0x0404;
const IPTC_TAG_MARKER: u8 = 0x1C;
const APP13_MARKER: u16 = 0xFFED;

/// Represents an Adobe Photoshop Image Resource Block
#[derive(Debug, Clone, PartialEq)]
struct ImageResourceBlock<'a> {
    /// Resource ID (e.g., 0x0404 for IPTC)
    id: u16,
    /// Resource name (Pascal string)
    name: &'a [u8],
    /// Resource data payload
    data: &'a [u8],
}

/// Represents a single IPTC IIM record
#[derive(Debug, Clone, PartialEq)]
struct IptcRecord {
    /// Record number (usually 2 for Application Record)
    record_number: u8,
    /// Dataset number (identifies the specific tag)
    dataset_number: u8,
    /// Record data
    data: Vec<u8>,
}

/// Parses a single Adobe Photoshop Image Resource Block (8BIM).
///
/// # Format
/// - Signature: "8BIM" (4 bytes)
/// - ID: 2 bytes (big-endian)
/// - Name: Pascal string (1 byte length + data), padded to even length
/// - Size: 4 bytes (big-endian)
/// - Data: variable length
fn parse_image_resource_block(input: &[u8]) -> IResult<&[u8], ImageResourceBlock> {
    // Parse 8BIM signature
    let (input, _) = tag(EIGHTBIM_SIGNATURE)(input)?;

    // Parse resource ID (2 bytes, big-endian)
    let (input, id) = be_u16(input)?;

    // Parse Pascal string name (1 byte length + data)
    let (input, name_length) = nom_u8(input)?;
    let (input, name) = take(name_length as usize)(input)?;

    // Pascal string must be padded to even length (including length byte)
    // Total length so far: 1 (length byte) + name_length
    // If odd, add 1 byte padding
    let total_name_length = 1 + name_length as usize;
    let (input, _) = if total_name_length % 2 == 1 {
        take(1usize)(input)? // Take 1 byte padding
    } else {
        (input, &b""[..]) // No padding needed
    };

    // Parse data size (4 bytes, big-endian)
    let (input, data_size) = be_u32(input)?;

    // Parse data
    let (input, data) = take(data_size as usize)(input)?;

    Ok((input, ImageResourceBlock { id, name, data }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photoshop_signature() {
        assert_eq!(PHOTOSHOP_SIGNATURE, b"Photoshop 3.0\0");
        assert_eq!(PHOTOSHOP_SIGNATURE.len(), 14);
    }

    #[test]
    fn test_8bim_signature() {
        assert_eq!(EIGHTBIM_SIGNATURE, b"8BIM");
        assert_eq!(EIGHTBIM_SIGNATURE.len(), 4);
    }

    #[test]
    fn test_iptc_resource_id() {
        assert_eq!(IPTC_RESOURCE_ID, 0x0404);
    }

    #[test]
    fn test_parse_image_resource_block() {
        // Create a minimal 8BIM resource block
        let mut data = Vec::new();
        data.extend_from_slice(b"8BIM"); // Signature
        data.extend_from_slice(&[0x04, 0x04]); // ID: 0x0404 (IPTC)
        data.push(0x00); // Name: empty Pascal string (length = 0)
        data.push(0x00); // Padding to make name even length
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Size: 4 bytes
        data.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4 bytes of data

        let result = parse_image_resource_block(&data);
        assert!(result.is_ok());

        let (remaining, block) = result.unwrap();
        assert_eq!(block.id, 0x0404);
        assert_eq!(block.name, &[] as &[u8]);
        assert_eq!(block.data, &[0xAA, 0xBB, 0xCC, 0xDD]);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_parse_image_resource_block_with_name() {
        let mut data = Vec::new();
        data.extend_from_slice(b"8BIM");
        data.extend_from_slice(&[0x04, 0x04]); // ID
        data.push(0x04); // Name length: 4
        data.extend_from_slice(b"TEST"); // Name: "TEST"
        data.push(0x00); // Padding (4+1 = 5, need 1 byte padding for even)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]); // Size: 2 bytes
        data.extend_from_slice(&[0x11, 0x22]); // Data

        let result = parse_image_resource_block(&data);
        assert!(result.is_ok());

        let (remaining, block) = result.unwrap();
        assert_eq!(block.id, 0x0404);
        assert_eq!(block.name, b"TEST");
        assert_eq!(block.data, &[0x11, 0x22]);
    }
}
