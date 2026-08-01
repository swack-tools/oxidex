//! IPTC segment parser for JPEG
//!
//! This module handles parsing of IPTC data in JPEG APP13 segments.
//! IPTC data is stored in Adobe Photoshop Image Resource Blocks (8BIM).

use crate::core::tag_conversion::parse_string_to_tag_value;
use crate::core::value_formatter::{
    format_iptc_coded_charset, format_iptc_date, format_iptc_time, format_iptc_urgency,
};
use crate::core::{MetadataMap, TagValue};
use crate::error::Result;
use crate::parsers::jpeg::segment_parser::Segment;
use nom::{
    IResult,
    bytes::complete::{tag, take},
    number::complete::{be_u16, be_u32, u8 as nom_u8},
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
pub struct IptcRecord {
    /// Record number (usually 2 for Application Record)
    pub record_number: u8,
    /// Dataset number (identifies the specific tag)
    pub dataset_number: u8,
    /// Record data
    pub data: Vec<u8>,
}

/// Parses a single Adobe Photoshop Image Resource Block (8BIM).
///
/// # Format
/// - Signature: "8BIM" (4 bytes)
/// - ID: 2 bytes (big-endian)
/// - Name: Pascal string (1 byte length + data), padded to even length
/// - Size: 4 bytes (big-endian)
/// - Data: variable length
fn parse_image_resource_block(input: &[u8]) -> IResult<&[u8], ImageResourceBlock<'_>> {
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
    let (input, _) = tag(&[IPTC_TAG_MARKER][..])(input)?;

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
pub fn parse_all_iptc_records(input: &[u8]) -> Result<Vec<IptcRecord>> {
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
/// Maps IPTC dataset numbers to tag names.
///
/// Returns static string slices for known datasets to avoid allocations.
/// Tag name in the format "IPTC:TagName"
pub fn dataset_to_tag_name(record_number: u8, dataset_number: u8) -> String {
    // Handle Record 2 (Application Record)
    if record_number == 2 {
        let tag_name = match dataset_number {
            0 => "IPTC:ApplicationRecordVersion",
            5 => "IPTC:ObjectName",
            7 => "IPTC:EditStatus",
            10 => "IPTC:Urgency",
            15 => "IPTC:Category",
            20 => "IPTC:SupplementalCategories",
            22 => "IPTC:FixtureIdentifier",
            25 => "IPTC:Keywords",
            26 => "IPTC:ContentLocationCode",
            27 => "IPTC:ContentLocationName",
            30 => "IPTC:ReleaseDate",
            35 => "IPTC:ReleaseTime",
            37 => "IPTC:ExpirationDate",
            38 => "IPTC:ExpirationTime",
            40 => "IPTC:SpecialInstructions",
            42 => "IPTC:ActionAdvised",
            45 => "IPTC:ReferenceService",
            47 => "IPTC:ReferenceDate",
            50 => "IPTC:ReferenceNumber",
            55 => "IPTC:DateCreated",
            60 => "IPTC:TimeCreated",
            62 => "IPTC:DigitalCreationDate",
            63 => "IPTC:DigitalCreationTime",
            65 => "IPTC:OriginatingProgram",
            70 => "IPTC:ProgramVersion",
            75 => "IPTC:ObjectCycle",
            80 => "IPTC:By-line",
            85 => "IPTC:By-lineTitle",
            90 => "IPTC:City",
            92 => "IPTC:Sub-location",
            95 => "IPTC:Province-State",
            100 => "IPTC:Country-PrimaryLocationCode",
            101 => "IPTC:Country-PrimaryLocationName",
            103 => "IPTC:OriginalTransmissionReference",
            105 => "IPTC:Headline",
            110 => "IPTC:Credit",
            115 => "IPTC:Source",
            116 => "IPTC:CopyrightNotice",
            118 => "IPTC:Contact",
            120 => "IPTC:Caption-Abstract",
            121 => "IPTC:LocalCaption",
            122 => "IPTC:Writer-Editor",
            125 => "IPTC:RasterizedCaption",
            130 => "IPTC:ImageType",
            131 => "IPTC:ImageOrientation",
            135 => "IPTC:LanguageIdentifier",
            150 => "IPTC:AudioType",
            151 => "IPTC:AudioSamplingRate",
            152 => "IPTC:AudioSamplingResolution",
            153 => "IPTC:AudioDuration",
            154 => "IPTC:AudioOutcue",
            200 => "IPTC:ObjectPreviewFileFormat",
            201 => "IPTC:ObjectPreviewFileFormatVer",
            202 => "IPTC:ObjectPreviewData",
            _ => return format!("IPTC:Unknown-{}-{}", record_number, dataset_number),
        };
        return tag_name.to_string();
    }

    // Handle Record 1 (Envelope Record)
    if record_number == 1 {
        let tag_name = match dataset_number {
            0 => "IPTC:EnvelopeRecordVersion",
            5 => "IPTC:Destination",
            20 => "IPTC:FileFormat",
            22 => "IPTC:FileVersion",
            30 => "IPTC:ServiceIdentifier",
            40 => "IPTC:EnvelopeNumber",
            50 => "IPTC:ProductID",
            60 => "IPTC:EnvelopePriority",
            70 => "IPTC:DateSent",
            80 => "IPTC:TimeSent",
            90 => "IPTC:CodedCharacterSet",
            100 => "IPTC:UniqueObjectName",
            120 => "IPTC:ARMIdentifier",
            122 => "IPTC:ARMVersion",
            _ => return format!("IPTC:Unknown-{}-{}", record_number, dataset_number),
        };
        return tag_name.to_string();
    }

    format!("IPTC:Unknown-{}-{}", record_number, dataset_number)
}

/// Decodes an IPTC string from bytes.
///
/// IPTC strings are typically Latin-1 encoded, but may also be UTF-8.
/// This function attempts UTF-8 first, falls back to Latin-1, and trims whitespace.
pub fn decode_iptc_string(data: &[u8]) -> String {
    // Try UTF-8 first
    if let Ok(s) = std::str::from_utf8(data) {
        return s.trim().to_string();
    }

    // Fall back to Latin-1 (ISO-8859-1)
    // In Latin-1, each byte maps directly to a Unicode code point
    let s: String = data.iter().map(|&b| b as char).collect();
    s.trim().to_string()
}

/// The `(record, dataset)` pairs `Image::ExifTool::IPTC` flags as lists.
///
/// IIM lets these datasets repeat, and ExifTool reports every occurrence: a
/// file with three `2:25` records yields `["ExifTool","Test","IPTC"]`, not
/// the last one. The set is every tag carrying `Flags => 'List'` (plus
/// `CatalogSets`, which spells it `List => 1`) in IPTC.pm, grouped by the
/// record whose table defines it. Any dataset absent from this table keeps
/// last-wins semantics, which is what ExifTool does for non-list tags.
const LIST_DATASETS: &[(u8, u8)] = &[
    // Record 1 -- EnvelopeRecord
    (1, 5),  // Destination
    (1, 50), // ProductID
    // Record 2 -- ApplicationRecord
    (2, 4),   // ObjectAttributeReference
    (2, 12),  // SubjectReference
    (2, 20),  // SupplementalCategories
    (2, 25),  // Keywords
    (2, 26),  // ContentLocationCode
    (2, 27),  // ContentLocationName
    (2, 45),  // ReferenceService
    (2, 47),  // ReferenceDate
    (2, 50),  // ReferenceNumber
    (2, 80),  // By-line
    (2, 85),  // By-lineTitle
    (2, 118), // Contact
    (2, 122), // Writer-Editor
    (2, 255), // CatalogSets
    // Record 8 -- ObjectData
    (8, 10), // SubFile
];

/// True when ExifTool reports repeated occurrences of this dataset as a list.
pub fn is_list_dataset(record_number: u8, dataset_number: u8) -> bool {
    LIST_DATASETS.contains(&(record_number, dataset_number))
}

/// Converts one IIM record's payload to the value ExifTool prints for it.
///
/// Returns `None` when the record carries nothing worth emitting (a record
/// version with fewer than the two bytes its `int16u` format requires), so
/// callers skip the tag rather than invent an empty one.
///
/// The conversions mirror `Image::ExifTool::IPTC`:
/// - dataset 0 of any record is `Format => 'int16u'`, a number not text;
/// - `CodedCharacterSet` (1:90) is an ISO 2022 escape sequence;
/// - date datasets render `YYYYMMDD` as `YYYY:MM:DD`;
/// - time datasets render `HHMMSS±HHMM` as `HH:MM:SS±HH:MM`;
/// - `Urgency` (2:10) gains its PrintConv description.
///
/// Everything else is decoded text, then narrowed to an integer or float
/// when it parses as one, because ExifTool prints e.g. `Category` unquoted.
pub fn format_iptc_record_value(
    record_number: u8,
    dataset_number: u8,
    data: &[u8],
) -> Option<TagValue> {
    // EnvelopeRecordVersion / ApplicationRecordVersion.
    if dataset_number == 0 {
        if data.len() < 2 {
            return None;
        }
        return Some(TagValue::Integer(
            u16::from_be_bytes([data[0], data[1]]) as i64
        ));
    }

    let text = decode_iptc_string(data);
    let formatted = match (record_number, dataset_number) {
        // CodedCharacterSet is an escape sequence, so it reads the raw bytes
        // rather than the decoded text.
        (1, 90) => format_iptc_coded_charset(data),
        // DateSent, ReleaseDate, ExpirationDate, ReferenceDate, DateCreated,
        // DigitalCreationDate.
        (1, 70) | (2, 30) | (2, 37) | (2, 47) | (2, 55) | (2, 62) => format_iptc_date(&text),
        // TimeSent, ReleaseTime, ExpirationTime, TimeCreated,
        // DigitalCreationTime.
        (1, 80) | (2, 35) | (2, 38) | (2, 60) | (2, 63) => format_iptc_time(&text),
        (2, 10) => format_iptc_urgency(&text),
        _ => text,
    };

    // Narrowed after formatting, not before: only the urgencies IPTC.pm gives
    // no description keep a bare number, and a reformatted date or time no
    // longer parses as one. ExifTool prints those same values unquoted.
    Some(parse_string_to_tag_value(&formatted))
}

/// Decodes a run of IPTC IIM records into `metadata`.
///
/// This is the one place IIM payloads become tags: JPEG APP13, TIFF's
/// IPTC-NAA tag, PSD and EPS 8BIM blocks and PDF image resources all reach
/// it, so a file's IPTC reads the same whichever container carries it.
///
/// Repeated records of a [`LIST_DATASETS`] dataset accumulate into a
/// [`TagValue::Array`] instead of overwriting each other; a dataset seen
/// once stays a bare scalar, which is how ExifTool prints a one-element
/// list. Values already present under a list key are kept and extended, so
/// IPTC split across several 8BIM blocks or APP13 segments still yields one
/// complete list.
pub fn insert_iptc_records(payload: &[u8], metadata: &mut MetadataMap) {
    let Ok(records) = parse_all_iptc_records(payload) else {
        return;
    };

    let mut lists: Vec<(String, Vec<TagValue>)> = Vec::new();

    for record in records {
        let tag_name = dataset_to_tag_name(record.record_number, record.dataset_number);
        let Some(value) =
            format_iptc_record_value(record.record_number, record.dataset_number, &record.data)
        else {
            continue;
        };

        if !is_list_dataset(record.record_number, record.dataset_number) {
            metadata.insert(tag_name, value);
            continue;
        }

        match lists.iter_mut().find(|(name, _)| *name == tag_name) {
            Some((_, values)) => values.push(value),
            None => {
                // Seed from whatever this key already holds so a second
                // block extends the list rather than replacing it.
                let mut values = match metadata.get(&tag_name) {
                    Some(TagValue::Array(existing)) => existing.clone(),
                    Some(existing) => vec![existing.clone()],
                    None => Vec::new(),
                };
                values.push(value);
                lists.push((tag_name, values));
            }
        }
    }

    for (tag_name, mut values) in lists {
        // ExifTool prints a one-element list as a bare scalar.
        let value = if values.len() == 1 {
            values.remove(0)
        } else {
            TagValue::Array(values)
        };
        metadata.insert(tag_name, value);
    }
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
/// A [`MetadataMap`] of `IPTC:`-prefixed tags. Datasets IIM allows to repeat
/// come back as a [`TagValue::Array`]; a map rather than a pair list is what
/// makes that possible, since repeated keys in a pair list collapse to the
/// last one when the caller folds them into a map.
///
/// Returns an empty map if no IPTC segments are found (not an error).
///
/// # Errors
///
/// Returns `ParseError` if:
/// - APP13 segment is malformed
/// - 8BIM resource blocks are invalid
/// - IPTC records cannot be parsed
pub fn extract_iptc_from_segments(segments: &[Segment]) -> Result<MetadataMap> {
    let mut all_iptc_tags = MetadataMap::new();

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
                        insert_iptc_records(block.data, &mut all_iptc_tags);
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
        assert!(remaining.is_empty());
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
        // Application Record (Record 2)
        assert_eq!(dataset_to_tag_name(2, 0), "IPTC:ApplicationRecordVersion");
        assert_eq!(dataset_to_tag_name(2, 5), "IPTC:ObjectName");
        assert_eq!(dataset_to_tag_name(2, 25), "IPTC:Keywords");
        assert_eq!(dataset_to_tag_name(2, 80), "IPTC:By-line");
        assert_eq!(dataset_to_tag_name(2, 90), "IPTC:City");
        assert_eq!(dataset_to_tag_name(2, 120), "IPTC:Caption-Abstract");

        // Envelope Record (Record 1)
        assert_eq!(dataset_to_tag_name(1, 0), "IPTC:EnvelopeRecordVersion");
        assert_eq!(dataset_to_tag_name(1, 90), "IPTC:CodedCharacterSet");

        // Unknown dataset should return generic name
        assert_eq!(dataset_to_tag_name(2, 255), "IPTC:Unknown-2-255");
        assert_eq!(dataset_to_tag_name(3, 5), "IPTC:Unknown-3-5");
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
        assert_eq!(
            tags.get("IPTC:ObjectName"),
            Some(&TagValue::String("Test Title".to_string()))
        );
        assert_eq!(
            tags.get("IPTC:By-line"),
            Some(&TagValue::String("Test Author".to_string()))
        );
    }

    #[test]
    fn test_extract_iptc_no_app13_segments() {
        // Empty segments
        let segments = vec![];
        let result = extract_iptc_from_segments(&segments);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    /// Appends one IIM record to `out`.
    fn push_record(out: &mut Vec<u8>, record: u8, dataset: u8, data: &[u8]) {
        out.push(IPTC_TAG_MARKER);
        out.push(record);
        out.push(dataset);
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(data);
    }

    /// Wraps IIM records in the APP13 -> "Photoshop 3.0\0" -> 8BIM 0x0404
    /// envelope a real JPEG carries them in.
    fn app13_segment(iptc: &[u8]) -> Vec<u8> {
        let mut payload = PHOTOSHOP_SIGNATURE.to_vec();
        payload.extend_from_slice(EIGHTBIM_SIGNATURE);
        payload.extend_from_slice(&IPTC_RESOURCE_ID.to_be_bytes());
        payload.extend_from_slice(&[0x00, 0x00]); // empty Pascal name, padded
        payload.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
        payload.extend_from_slice(iptc);
        payload
    }

    /// ExifTool reports every dataset IPTC.pm flags `List` as an array, so
    /// repeated records must accumulate rather than overwrite each other.
    /// IPTC.jpg carries three Keywords and three SupplementalCategories;
    /// before this, only the last of each survived.
    #[test]
    fn repeated_list_datasets_accumulate_into_an_array() {
        use crate::core::{MetadataMap, TagValue};

        let mut iptc = Vec::new();
        for keyword in ["ExifTool", "Test", "IPTC"] {
            push_record(&mut iptc, 2, 25, keyword.as_bytes());
        }
        for category in ["amazing", "image", "utilities"] {
            push_record(&mut iptc, 2, 20, category.as_bytes());
        }
        // A non-list dataset keeps last-wins semantics.
        push_record(&mut iptc, 2, 5, b"Test IPTC picture");

        let data = app13_segment(&iptc);
        let segments = vec![Segment::new(APP13_MARKER, 0, &data)];

        let mut metadata = MetadataMap::new();
        crate::core::jpeg_helpers::process_iptc_segments(&segments, &mut metadata);

        assert_eq!(
            metadata.get("IPTC:Keywords"),
            Some(&TagValue::Array(vec![
                TagValue::String("ExifTool".to_string()),
                TagValue::String("Test".to_string()),
                TagValue::String("IPTC".to_string()),
            ]))
        );
        assert_eq!(
            metadata.get("IPTC:SupplementalCategories"),
            Some(&TagValue::Array(vec![
                TagValue::String("amazing".to_string()),
                TagValue::String("image".to_string()),
                TagValue::String("utilities".to_string()),
            ]))
        );
        assert_eq!(
            metadata.get("IPTC:ObjectName"),
            Some(&TagValue::String("Test IPTC picture".to_string()))
        );
    }

    /// The list set is every dataset IPTC.pm flags `List`, not just Keywords
    /// and SupplementalCategories: By-line among them, which MWG.jpg repeats.
    /// A dataset outside the set still takes the last value.
    #[test]
    fn list_datasets_beyond_keywords_accumulate() {
        use crate::core::{MetadataMap, TagValue};

        let mut iptc = Vec::new();
        push_record(&mut iptc, 2, 80, b"First Creator"); // By-line, a list
        push_record(&mut iptc, 2, 80, b"Second Creator");
        push_record(&mut iptc, 2, 105, b"First Headline"); // Headline, not a list
        push_record(&mut iptc, 2, 105, b"Second Headline");

        let data = app13_segment(&iptc);
        let segments = vec![Segment::new(APP13_MARKER, 0, &data)];

        let mut metadata = MetadataMap::new();
        crate::core::jpeg_helpers::process_iptc_segments(&segments, &mut metadata);

        assert_eq!(
            metadata.get("IPTC:By-line"),
            Some(&TagValue::Array(vec![
                TagValue::String("First Creator".to_string()),
                TagValue::String("Second Creator".to_string()),
            ]))
        );
        assert_eq!(
            metadata.get("IPTC:Headline"),
            Some(&TagValue::String("Second Headline".to_string()))
        );
    }

    /// An Urgency IPTC.pm gives no description stays a bare number, the way
    /// ExifTool prints it; only 1, 5 and 8 gain a parenthetical and so become
    /// text.
    #[test]
    fn urgency_without_a_printconv_stays_numeric() {
        use crate::core::TagValue;

        assert_eq!(
            format_iptc_record_value(2, 10, b"2"),
            Some(TagValue::Integer(2))
        );
        assert_eq!(
            format_iptc_record_value(2, 10, b"8"),
            Some(TagValue::String("8 (least urgent)".to_string()))
        );
    }

    /// ExifTool prints a one-element list as a bare scalar, not a 1-array.
    #[test]
    fn a_single_list_dataset_value_stays_scalar() {
        use crate::core::{MetadataMap, TagValue};

        let mut iptc = Vec::new();
        push_record(&mut iptc, 2, 25, b"solo");

        let data = app13_segment(&iptc);
        let segments = vec![Segment::new(APP13_MARKER, 0, &data)];

        let mut metadata = MetadataMap::new();
        crate::core::jpeg_helpers::process_iptc_segments(&segments, &mut metadata);

        assert_eq!(
            metadata.get("IPTC:Keywords"),
            Some(&TagValue::String("solo".to_string()))
        );
    }
}
