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

/// Maps IPTC dataset numbers to tag names.
///
/// # Parameters
/// - `record_number`: The record number (usually 2 for Application Record)
/// - `dataset_number`: The dataset number identifying the tag
///
/// # Returns
/// Tag name in the format "IPTC:TagName"
fn dataset_to_tag_name(record_number: u8, dataset_number: u8) -> String {
    // Only handle Record 2 (Application Record) for now
    if record_number != 2 {
        return format!("IPTC:Unknown-{}-{}", record_number, dataset_number);
    }

    let tag_name = match dataset_number {
        5 => "ObjectName",
        7 => "EditStatus",
        10 => "Urgency",
        15 => "Category",
        20 => "SupplementalCategories",
        25 => "Keywords",
        40 => "SpecialInstructions",
        55 => "DateCreated",
        60 => "TimeCreated",
        80 => "By-line",
        85 => "By-lineTitle",
        90 => "City",
        92 => "Sub-location",
        95 => "Province-State",
        100 => "Country-PrimaryLocationCode",
        101 => "Country-PrimaryLocationName",
        103 => "OriginalTransmissionReference",
        105 => "Headline",
        110 => "Credit",
        115 => "Source",
        116 => "CopyrightNotice",
        118 => "Contact",
        120 => "Caption-Abstract",
        122 => "Writer-Editor",
        _ => return format!("IPTC:Unknown-{}-{}", record_number, dataset_number),
    };

    format!("IPTC:{}", tag_name)
}

/// Decodes an IPTC string from bytes.
///
/// IPTC strings are typically Latin-1 encoded, but may also be UTF-8.
/// This function attempts UTF-8 first, falls back to Latin-1, and trims whitespace.
fn decode_iptc_string(data: &[u8]) -> String {
    // Try UTF-8 first
    if let Ok(s) = std::str::from_utf8(data) {
        return s.trim().to_string();
    }

    // Fall back to Latin-1 (ISO-8859-1)
    // In Latin-1, each byte maps directly to a Unicode code point
    let s: String = data.iter().map(|&b| b as char).collect();
    s.trim().to_string()
}

/// Extracts IPTC metadata from JPEG segments.
///
/// This function scans through all segments, identifies APP13 segments with
/// the Photoshop signature, extracts IPTC data from 8BIM resource blocks,
/// and parses IPTC IIM records.
///
/// # Parameters
///
/// - `segments`: Slice of parsed JPEG segments (from `parse_segments()`)
///
/// # Returns
///
/// Vector of (tag_name, value) tuples where tag_name is in the format
/// "IPTC:PropertyName" (e.g., "IPTC:ObjectName", "IPTC:By-line").
///
/// Returns an empty vector if no IPTC segments are found (not an error).
///
/// # Errors
///
/// Returns `ParseError` if:
/// - APP13 segment is malformed
/// - 8BIM resource blocks are invalid
/// - IPTC records cannot be parsed
pub fn extract_iptc_from_segments(segments: &[Segment]) -> Result<Vec<(String, String)>> {
    let mut all_iptc_tags = Vec::new();

    // Iterate through all segments looking for APP13 segments
    for segment in segments {
        // Check if this is an APP13 segment (0xFFED)
        if segment.marker != APP13_MARKER {
            continue;
        }

        // Check if this APP13 segment contains Photoshop data
        if !segment.data.starts_with(PHOTOSHOP_SIGNATURE) {
            continue;
        }

        // Skip past the Photoshop signature
        let mut current = &segment.data[PHOTOSHOP_SIGNATURE.len()..];

        // Parse all 8BIM resource blocks
        while current.len() > 4 {
            // Check if this looks like a 8BIM block
            if !current.starts_with(EIGHTBIM_SIGNATURE) {
                break;
            }

            match parse_image_resource_block(current) {
                Ok((remaining, block)) => {
                    // Check if this is the IPTC resource block (ID 0x0404)
                    if block.id == IPTC_RESOURCE_ID {
                        // Parse IPTC records from the block data
                        match parse_all_iptc_records(block.data) {
                            Ok(records) => {
                                // Convert records to tag name/value pairs
                                for record in records {
                                    let tag_name = dataset_to_tag_name(
                                        record.record_number,
                                        record.dataset_number,
                                    );
                                    let value = decode_iptc_string(&record.data);

                                    all_iptc_tags.push((tag_name, value));
                                }
                            }
                            Err(e) => {
                                // Log error but continue processing other blocks
                                eprintln!("Warning: Failed to parse IPTC records: {}", e);
                            }
                        }
                    }

                    current = remaining;
                }
                Err(_) => {
                    // Failed to parse block, stop processing this segment
                    break;
                }
            }
        }
    }

    Ok(all_iptc_tags)
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

    #[test]
    fn test_dataset_to_tag_name() {
        assert_eq!(dataset_to_tag_name(2, 5), "IPTC:ObjectName");
        assert_eq!(dataset_to_tag_name(2, 25), "IPTC:Keywords");
        assert_eq!(dataset_to_tag_name(2, 80), "IPTC:By-line");
        assert_eq!(dataset_to_tag_name(2, 90), "IPTC:City");
        assert_eq!(dataset_to_tag_name(2, 120), "IPTC:Caption-Abstract");

        // Unknown dataset should return generic name
        assert_eq!(dataset_to_tag_name(2, 255), "IPTC:Unknown-2-255");
    }

    #[test]
    fn test_decode_iptc_string() {
        // Test ASCII string
        let ascii_data = b"Hello World";
        assert_eq!(decode_iptc_string(ascii_data), "Hello World");

        // Test string with trailing spaces (should be trimmed)
        let padded_data = b"Test    ";
        assert_eq!(decode_iptc_string(padded_data), "Test");
    }

    #[test]
    fn test_extract_iptc_from_segments() {
        // Create a complete APP13 segment with IPTC data
        let mut app13_data = Vec::new();

        // Photoshop signature
        app13_data.extend_from_slice(PHOTOSHOP_SIGNATURE);

        // 8BIM resource block
        app13_data.extend_from_slice(b"8BIM");
        app13_data.extend_from_slice(&[0x04, 0x04]); // ID: IPTC
        app13_data.push(0x00); // Empty name
        app13_data.push(0x00); // Padding

        // IPTC data
        let mut iptc_data = Vec::new();
        // Record: ObjectName (dataset 5)
        iptc_data.push(0x1C);
        iptc_data.extend_from_slice(&[0x02, 0x05]);
        iptc_data.extend_from_slice(&[0x00, 0x0A]);
        iptc_data.extend_from_slice(b"Test Title");

        // Record: By-line (dataset 80)
        iptc_data.push(0x1C);
        iptc_data.extend_from_slice(&[0x02, 0x50]);
        iptc_data.extend_from_slice(&[0x00, 0x0B]);
        iptc_data.extend_from_slice(b"Test Author");

        // Add IPTC data size and data to 8BIM block
        let iptc_size = iptc_data.len() as u32;
        app13_data.extend_from_slice(&iptc_size.to_be_bytes());
        app13_data.extend_from_slice(&iptc_data);

        // Create APP13 segment
        let segment = Segment::new(APP13_MARKER, 0, &app13_data);
        let segments = vec![segment];

        // Extract IPTC
        let result = extract_iptc_from_segments(&segments);
        assert!(result.is_ok());

        let tags = result.unwrap();
        assert_eq!(tags.len(), 2);

        // Check tags
        let title = tags.iter().find(|(k, _)| k == "IPTC:ObjectName");
        assert!(title.is_some());
        assert_eq!(title.unwrap().1, "Test Title");

        let author = tags.iter().find(|(k, _)| k == "IPTC:By-line");
        assert!(author.is_some());
        assert_eq!(author.unwrap().1, "Test Author");
    }

    #[test]
    fn test_extract_iptc_no_app13_segments() {
        // Empty segments
        let segments = vec![];
        let result = extract_iptc_from_segments(&segments);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }
}
