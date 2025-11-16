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

/// Parses a single IPTC IIM record.
///
/// # Format
/// - Tag marker: 0x1C (1 byte)
/// - Record number: 1 byte (usually 2 for Application Record)
/// - Dataset number: 1 byte
/// - Length: 2 bytes (big-endian), or extended format for > 32767 bytes
/// - Data: variable length
fn parse_iptc_record(input: &[u8]) -> IResult<&[u8], IptcRecord> {
    // Parse tag marker (must be 0x1C)
    let (input, _) = tag(&[IPTC_TAG_MARKER])(input)?;

    // Parse record number (1 byte)
    let (input, record_number) = nom_u8(input)?;

    // Parse dataset number (1 byte)
    let (input, dataset_number) = nom_u8(input)?;

    // Parse length (2 bytes, big-endian)
    let (input, length) = be_u16(input)?;

    // Check for extended format (if length > 32767, it's actually a marker)
    // For now, we'll just support standard format (< 32768 bytes)
    let data_length = length as usize;

    // Parse data
    let (input, data_bytes) = take(data_length)(input)?;

    Ok((
        input,
        IptcRecord {
            record_number,
            dataset_number,
            data: data_bytes.to_vec(),
        },
    ))
}

/// Parses all IPTC IIM records from a data block.
///
/// Returns a vector of all successfully parsed records.
/// Stops at first parse error or end of data.
fn parse_all_iptc_records(input: &[u8]) -> Result<Vec<IptcRecord>> {
    let mut records = Vec::new();
    let mut current = input;

    while !current.is_empty() {
        // Check if next byte is tag marker
        if current[0] != IPTC_TAG_MARKER {
            break;
        }

        match parse_iptc_record(current) {
            Ok((remaining, record)) => {
                records.push(record);
                current = remaining;
            }
            Err(_) => {
                // Stop on parse error
                break;
            }
        }
    }

    Ok(records)
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

    #[test]
    fn test_parse_iptc_record() {
        // Create a minimal IPTC record
        // Record 2, Dataset 5 (ObjectName), Data: "Test"
        let data = vec![
            0x1C, // Tag marker
            0x02, // Record number (Application Record)
            0x05, // Dataset number (ObjectName)
            0x00, 0x04, // Length: 4 bytes
            b'T', b'e', b's', b't', // Data: "Test"
        ];

        let result = parse_iptc_record(&data);
        assert!(result.is_ok());

        let (remaining, record) = result.unwrap();
        assert_eq!(record.record_number, 2);
        assert_eq!(record.dataset_number, 5);
        assert_eq!(record.data, b"Test");
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_parse_multiple_iptc_records() {
        let mut data = Vec::new();

        // Record 1
        data.push(0x1C);
        data.extend_from_slice(&[0x02, 0x05]); // Record 2, Dataset 5
        data.extend_from_slice(&[0x00, 0x05]); // Length: 5
        data.extend_from_slice(b"Title");

        // Record 2
        data.push(0x1C);
        data.extend_from_slice(&[0x02, 0x50]); // Record 2, Dataset 80 (ByLine)
        data.extend_from_slice(&[0x00, 0x06]); // Length: 6
        data.extend_from_slice(b"Author");

        let result = parse_all_iptc_records(&data);
        assert!(result.is_ok());

        let records = result.unwrap();
        assert_eq!(records.len(), 2);

        assert_eq!(records[0].dataset_number, 5);
        assert_eq!(records[0].data, b"Title");

        assert_eq!(records[1].dataset_number, 80);
        assert_eq!(records[1].data, b"Author");
    }
}
