//! IPTC Record 1 (Envelope Record) Parser
//!
//! This module handles parsing of IPTC-IIM Record 1 (Envelope Record) tags.
//! Record 1 contains transmission envelope information as defined in the
//! IPTC-IIM (Information Interchange Model) specification.
//!
//! # Envelope Record Tags
//!
//! Record 1 contains metadata about the transmission envelope:
//! - ModelVersion (0): Version of the IIM specification
//! - Destination (5): Routing destination
//! - FileFormat (20): File format type code
//! - FileVersion (22): File format version
//! - ServiceIdentifier (30): Service provider identifier
//! - EnvelopeNumber (40): Unique envelope number
//! - ProductID (50): Product identifier within envelope
//! - EnvelopePriority (60): Priority level (1=highest, 9=lowest)
//! - DateSent (70): Date when data was sent (YYYYMMDD)
//! - TimeSent (80): Time when data was sent (HHMMSS±HHMM)
//! - CodedCharacterSet (90): Character encoding specification
//! - UniqueObjectName (100): Globally unique object identifier
//!
//! # References
//!
//! - IPTC-IIM Specification: https://iptc.org/standards/iim/
//! - ExifTool IPTC tag documentation

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;
use crate::core::value_formatter::{format_iptc_date, format_iptc_time};

// =============================================================================
// CONSTANTS
// =============================================================================

/// IPTC tag marker byte (0x1C) that precedes every IPTC dataset
const IPTC_TAG_MARKER: u8 = 0x1C;

/// Record number for Envelope Record (Record 1)
const ENVELOPE_RECORD_NUMBER: u8 = 1;

/// Minimum size for an IPTC dataset header (marker + record + dataset + length)
const MIN_DATASET_SIZE: usize = 5;

// =============================================================================
// IPTC RECORD 1 TAG DEFINITIONS
// =============================================================================

/// Dataset number for ModelVersion (IPTC Record 1, Tag 0)
///
/// A binary number identifying the version of the Information Interchange
/// Model, Part I, utilised by the provider. Version numbers are assigned
/// by IPTC. The most recent version is 4.
const DATASET_MODEL_VERSION: u8 = 0;

/// Dataset number for Destination (IPTC Record 1, Tag 5)
///
/// Optional, repeatable. Contains the characters that indicate the
/// destination of this data. The format of the address is determined
/// by the provider.
const DATASET_DESTINATION: u8 = 5;

/// Dataset number for FileFormat (IPTC Record 1, Tag 20)
///
/// A binary number representing the file format. The file format must be
/// registered with IPTC or NAA. Common values include:
/// - 0: No ObjectData
/// - 1: IPTC-NAA Digital Newsphoto Parameter Record
/// - 2: IPTC7901 Recommended Message Format
/// - 3: Tagged Image File Format (TIFF)
/// - 4: Illustrator
/// - 5: AppleSingle
/// - 6: NAA 89-3 (ANPA 1312)
/// - 7: MacBinary II
/// - 11: JPEG
const DATASET_FILE_FORMAT: u8 = 20;

/// Dataset number for FileVersion (IPTC Record 1, Tag 22)
///
/// A binary number representing the particular version of the File Format
/// specified by FileFormat.
const DATASET_FILE_VERSION: u8 = 22;

/// Dataset number for ServiceIdentifier (IPTC Record 1, Tag 30)
///
/// The characters identify the provider and product. Should be unique
/// across all providers.
const DATASET_SERVICE_IDENTIFIER: u8 = 30;

/// Dataset number for EnvelopeNumber (IPTC Record 1, Tag 40)
///
/// A number uniquely identifying the envelope within the service.
/// Consists of 8 octets (0-99999999).
const DATASET_ENVELOPE_NUMBER: u8 = 40;

/// Dataset number for ProductID (IPTC Record 1, Tag 50)
///
/// Allows a provider to identify subsets of its overall service. Used
/// to provide receiving organisation data on which services it receives.
const DATASET_PRODUCT_ID: u8 = 50;

/// Dataset number for EnvelopePriority (IPTC Record 1, Tag 60)
///
/// Specifies the envelope handling priority:
/// - 1 (most urgent) to 8 (least urgent)
/// - 0 reserved for future use
/// - 9 user-defined priority
/// - 5 is "normal" handling
const DATASET_ENVELOPE_PRIORITY: u8 = 60;

/// Dataset number for DateSent (IPTC Record 1, Tag 70)
///
/// Uses the format CCYYMMDD (century, year, month, day) to indicate
/// the year, month and day the service sent the material.
const DATASET_DATE_SENT: u8 = 70;

/// Dataset number for TimeSent (IPTC Record 1, Tag 80)
///
/// Uses the format HHMMSS±HHMM where HHMMSS refers to local hour,
/// minute and seconds and ±HHMM refers to hours and minutes ahead
/// of or behind Universal Coordinated Time (UTC).
const DATASET_TIME_SENT: u8 = 80;

/// Dataset number for CodedCharacterSet (IPTC Record 1, Tag 90)
///
/// This tag is used to indicate the code table(s) used for the remainder
/// of the data. The ISO 2022 escape sequences are used to switch between
/// character sets.
///
/// Common values:
/// - ESC % G (1B 25 47): UTF-8
/// - ESC . A (1B 2E 41): ISO-8859-1 (Latin-1)
/// - ESC . B (1B 2E 42): ISO-8859-2 (Latin-2)
const DATASET_CODED_CHARACTER_SET: u8 = 90;

/// Dataset number for UniqueObjectName (IPTC Record 1, Tag 100)
///
/// The Unique Object Name is composed of a provider-assigned service
/// identifier, date, and envelope number, combined to form a Unique
/// Object Identifier (UOI).
const DATASET_UNIQUE_OBJECT_NAME: u8 = 100;

// =============================================================================
// PUBLIC API
// =============================================================================

/// Parses IPTC Record 1 (Envelope Record) data and returns a MetadataMap.
///
/// This function extracts envelope metadata from raw IPTC IIM data. Record 1
/// contains transmission envelope information that describes how the data
/// was packaged and sent.
///
/// # Arguments
///
/// * `data` - Raw IPTC IIM data bytes. This should be the complete IPTC data
///   block, which may contain records from multiple record types.
///   Only Record 1 datasets will be extracted.
///
/// # Returns
///
/// A `MetadataMap` containing all successfully parsed Record 1 tags. Tags are
/// stored with the "IPTC:" prefix (e.g., "IPTC:ModelVersion", "IPTC:DateSent").
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::iptc_record1::parse_iptc_record1;
///
/// let iptc_data = vec![
///     0x1C, 0x01, 0x00,  // Tag marker, Record 1, Dataset 0 (ModelVersion)
///     0x00, 0x02,        // Length: 2 bytes
///     0x00, 0x04,        // Value: version 4
/// ];
///
/// let metadata = parse_iptc_record1(&iptc_data);
/// assert_eq!(metadata.get_integer("IPTC:ModelVersion"), Some(4));
/// ```
///
/// # Format Details
///
/// IPTC IIM datasets have the following structure:
/// - Byte 0: Tag marker (0x1C)
/// - Byte 1: Record number (1 for Envelope)
/// - Byte 2: Dataset number (identifies the specific tag)
/// - Bytes 3-4: Data length (big-endian, 16-bit for standard format)
/// - Bytes 5+: Data payload
///
/// Extended length format (for data > 32767 bytes) is also supported.
pub fn parse_iptc_record1(data: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let mut offset = 0;

    // Iterate through all IPTC datasets in the data block
    while offset + MIN_DATASET_SIZE <= data.len() {
        // Verify tag marker byte
        if data[offset] != IPTC_TAG_MARKER {
            // No more valid IPTC data; stop parsing
            break;
        }

        let record_number = data[offset + 1];
        let dataset_number = data[offset + 2];

        // Parse the data length (big-endian 16-bit value)
        let length_high = data[offset + 3] as usize;
        let length_low = data[offset + 4] as usize;
        let data_length = (length_high << 8) | length_low;

        // Check for extended length format (if bit 15 is set)
        // Extended format uses the length field as a count of additional
        // length bytes. We don't support this for Record 1 as these values
        // are typically small.
        if length_high & 0x80 != 0 {
            // Extended format: skip this dataset
            // The actual length is encoded in the following bytes
            offset += MIN_DATASET_SIZE;
            continue;
        }

        // Verify we have enough data for the payload
        let payload_start = offset + MIN_DATASET_SIZE;
        let payload_end = payload_start + data_length;

        if payload_end > data.len() {
            // Truncated data; stop parsing
            break;
        }

        let payload = &data[payload_start..payload_end];

        // Only process Record 1 (Envelope) datasets
        if record_number == ENVELOPE_RECORD_NUMBER {
            process_record1_dataset(dataset_number, payload, &mut metadata);
        }

        // Move to the next dataset
        offset = payload_end;
    }

    metadata
}

// =============================================================================
// INTERNAL HELPERS
// =============================================================================

/// Processes a single Record 1 dataset and adds it to the metadata map.
///
/// This function handles the type-specific parsing for each Record 1 tag,
/// converting raw bytes to appropriate string or integer values.
///
/// # Arguments
///
/// * `dataset_number` - The dataset number identifying the specific tag
/// * `payload` - The raw data bytes for this dataset
/// * `metadata` - The metadata map to populate with the parsed value
fn process_record1_dataset(dataset_number: u8, payload: &[u8], metadata: &mut MetadataMap) {
    match dataset_number {
        DATASET_MODEL_VERSION => {
            // ModelVersion is a 2-byte binary integer
            if let Some(version) = parse_binary_u16(payload) {
                metadata.insert("IPTC:ModelVersion", TagValue::new_integer(version as i64));
            }
        }

        DATASET_DESTINATION => {
            // Destination is a text string (repeatable in spec, but we take first)
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Destination", TagValue::new_string(value));
            }
        }

        DATASET_FILE_FORMAT => {
            // FileFormat is a 2-byte binary integer with known format codes
            if let Some(format_code) = parse_binary_u16(payload) {
                let formatted = format_file_format_code(format_code);
                metadata.insert("IPTC:FileFormat", TagValue::new_string(formatted));
            }
        }

        DATASET_FILE_VERSION => {
            // FileVersion is a 2-byte binary integer
            if let Some(version) = parse_binary_u16(payload) {
                metadata.insert("IPTC:FileVersion", TagValue::new_integer(version as i64));
            }
        }

        DATASET_SERVICE_IDENTIFIER => {
            // ServiceIdentifier is a text string (max 10 characters)
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ServiceIdentifier", TagValue::new_string(value));
            }
        }

        DATASET_ENVELOPE_NUMBER => {
            // EnvelopeNumber is an 8-character numeric string
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:EnvelopeNumber", TagValue::new_string(value));
            }
        }

        DATASET_PRODUCT_ID => {
            // ProductID is a text string (max 32 characters)
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ProductID", TagValue::new_string(value));
            }
        }

        DATASET_ENVELOPE_PRIORITY => {
            // EnvelopePriority is a single digit 0-9
            // Store as integer for easier comparison
            if !payload.is_empty() {
                // Handle both numeric byte and ASCII digit
                let priority = if payload[0] >= b'0' && payload[0] <= b'9' {
                    (payload[0] - b'0') as i64
                } else if payload[0] <= 9 {
                    payload[0] as i64
                } else {
                    // Invalid priority value
                    return;
                };
                metadata.insert("IPTC:EnvelopePriority", TagValue::new_integer(priority));
            }
        }

        DATASET_DATE_SENT => {
            // DateSent is YYYYMMDD format, convert to YYYY:MM:DD
            let raw_date = decode_iptc_string(payload);
            if !raw_date.is_empty() {
                let formatted = format_iptc_date(&raw_date);
                metadata.insert("IPTC:DateSent", TagValue::new_string(formatted));
            }
        }

        DATASET_TIME_SENT => {
            // TimeSent is HHMMSS±HHMM format, convert to HH:MM:SS±HH:MM
            let raw_time = decode_iptc_string(payload);
            if !raw_time.is_empty() {
                let formatted = format_iptc_time(&raw_time);
                metadata.insert("IPTC:TimeSent", TagValue::new_string(formatted));
            }
        }

        DATASET_CODED_CHARACTER_SET => {
            // CodedCharacterSet contains ISO 2022 escape sequences
            // We decode this to a human-readable description
            let description = decode_character_set(payload);
            metadata.insert("IPTC:CodedCharacterSet", TagValue::new_string(description));
        }

        DATASET_UNIQUE_OBJECT_NAME => {
            // UniqueObjectName is a structured identifier string
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:UniqueObjectName", TagValue::new_string(value));
            }
        }

        _ => {
            // Unknown Record 1 dataset; store as generic tag with raw string value
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                let tag_name = format!("IPTC:Envelope-{}", dataset_number);
                metadata.insert(tag_name, TagValue::new_string(value));
            }
        }
    }
}

/// Parses a big-endian 16-bit unsigned integer from a byte slice.
///
/// # Arguments
///
/// * `data` - Byte slice containing at least 2 bytes
///
/// # Returns
///
/// The parsed u16 value, or None if the slice is too short.
fn parse_binary_u16(data: &[u8]) -> Option<u16> {
    if data.len() < 2 {
        return None;
    }
    Some(((data[0] as u16) << 8) | (data[1] as u16))
}

/// Decodes an IPTC string from raw bytes.
///
/// IPTC strings are typically encoded as Latin-1 (ISO-8859-1), but may also
/// be UTF-8 if the CodedCharacterSet indicates so. This function attempts
/// UTF-8 first, then falls back to Latin-1.
///
/// # Arguments
///
/// * `data` - Raw byte data to decode
///
/// # Returns
///
/// The decoded string, with leading/trailing whitespace trimmed.
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

/// Formats a file format code to a human-readable description.
///
/// IPTC-IIM defines numeric codes for common file formats. This function
/// converts the code to a descriptive string matching ExifTool's output.
///
/// # Arguments
///
/// * `code` - The file format code from FileFormat dataset
///
/// # Returns
///
/// A string describing the file format, or the numeric code if unknown.
fn format_file_format_code(code: u16) -> String {
    // Format codes defined by IPTC-NAA
    match code {
        0 => "No ObjectData".to_string(),
        1 => "IPTC-NAA Digital Newsphoto Parameter Record".to_string(),
        2 => "IPTC7901 Recommended Message Format".to_string(),
        3 => "TIFF".to_string(),
        4 => "Illustrator".to_string(),
        5 => "AppleSingle".to_string(),
        6 => "NAA 89-3 (ANPA 1312)".to_string(),
        7 => "MacBinary II".to_string(),
        8 => "IPTC Unstructured Character Oriented File Format".to_string(),
        9 => "United Press International ANPA 1312".to_string(),
        10 => "United Press International Down-Load Message".to_string(),
        11 => "JPEG".to_string(),
        12 => "Photo-CD Image-Pac".to_string(),
        13 => "Microsoft Bit Mapped Graphics".to_string(),
        14 => "Digital Audio File".to_string(),
        15 => "Pixel Flow".to_string(),
        16 => "Windows BMP".to_string(),
        17 => "Audio Type A".to_string(),
        18 => "Video Type A".to_string(),
        19 => "PNG".to_string(),
        20 => "GIF".to_string(),
        21 => "PSD".to_string(),
        // Unknown code: return as numeric value
        _ => code.to_string(),
    }
}

/// Decodes a CodedCharacterSet value to a human-readable description.
///
/// The CodedCharacterSet dataset contains ISO 2022 escape sequences that
/// indicate the character encoding used in the IPTC data. This function
/// recognizes common escape sequences and returns a descriptive name.
///
/// # Arguments
///
/// * `data` - Raw bytes containing ISO 2022 escape sequence(s)
///
/// # Returns
///
/// A human-readable description of the character set.
fn decode_character_set(data: &[u8]) -> String {
    // Check for common ISO 2022 escape sequences
    // ESC sequences are 3 bytes: 0x1B (ESC) followed by two identifier bytes

    // UTF-8: ESC % G (0x1B 0x25 0x47)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x25 && data[2] == 0x47 {
        return "UTF-8".to_string();
    }

    // ISO-8859-1 (Latin-1): ESC . A (0x1B 0x2E 0x41)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x41 {
        return "ISO-8859-1".to_string();
    }

    // ISO-8859-2 (Latin-2): ESC . B (0x1B 0x2E 0x42)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x42 {
        return "ISO-8859-2".to_string();
    }

    // ISO-8859-3 (Latin-3): ESC . C (0x1B 0x2E 0x43)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x43 {
        return "ISO-8859-3".to_string();
    }

    // ISO-8859-4 (Latin-4): ESC . D (0x1B 0x2E 0x44)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x44 {
        return "ISO-8859-4".to_string();
    }

    // ISO-8859-5 (Cyrillic): ESC . L (0x1B 0x2E 0x4C)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x4C {
        return "ISO-8859-5".to_string();
    }

    // ISO-8859-6 (Arabic): ESC . G (0x1B 0x2E 0x47)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x47 {
        return "ISO-8859-6".to_string();
    }

    // ISO-8859-7 (Greek): ESC . F (0x1B 0x2E 0x46)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x46 {
        return "ISO-8859-7".to_string();
    }

    // ISO-8859-8 (Hebrew): ESC . H (0x1B 0x2E 0x48)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x48 {
        return "ISO-8859-8".to_string();
    }

    // ISO-8859-9 (Turkish): ESC . M (0x1B 0x2E 0x4D)
    if data.len() >= 3 && data[0] == 0x1B && data[1] == 0x2E && data[2] == 0x4D {
        return "ISO-8859-9".to_string();
    }

    // If we can't decode the escape sequence, show as hex
    if !data.is_empty() {
        let hex: Vec<String> = data.iter().map(|b| format!("{:02X}", b)).collect();
        return format!("ESC {}", hex.join(" "));
    }

    "(none)".to_string()
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Helper functions for test data construction
    // -------------------------------------------------------------------------

    /// Creates an IPTC dataset with the given record, dataset number, and payload.
    fn make_iptc_dataset(record: u8, dataset: u8, payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(IPTC_TAG_MARKER);
        data.push(record);
        data.push(dataset);
        data.push((payload.len() >> 8) as u8);
        data.push((payload.len() & 0xFF) as u8);
        data.extend_from_slice(payload);
        data
    }

    // -------------------------------------------------------------------------
    // ModelVersion (Dataset 0) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_model_version_parsing() {
        // Version 4 (the most common version)
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_MODEL_VERSION, &[0x00, 0x04]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:ModelVersion"), Some(4));
    }

    #[test]
    fn test_model_version_version_2() {
        // Older version 2
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_MODEL_VERSION, &[0x00, 0x02]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:ModelVersion"), Some(2));
    }

    #[test]
    fn test_model_version_empty_payload() {
        // Empty payload should not create a tag
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_MODEL_VERSION, &[]);
        let metadata = parse_iptc_record1(&data);

        assert!(metadata.get("IPTC:ModelVersion").is_none());
    }

    // -------------------------------------------------------------------------
    // Destination (Dataset 5) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_destination_parsing() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_DESTINATION, b"NEWS-WIRE");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:Destination"), Some("NEWS-WIRE"));
    }

    #[test]
    fn test_destination_with_spaces() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_DESTINATION, b"  TRIMMED  ");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:Destination"), Some("TRIMMED"));
    }

    #[test]
    fn test_destination_empty() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_DESTINATION, b"");
        let metadata = parse_iptc_record1(&data);

        // Empty destination should not be stored
        assert!(metadata.get("IPTC:Destination").is_none());
    }

    // -------------------------------------------------------------------------
    // FileFormat (Dataset 20) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_file_format_jpeg() {
        // Code 11 = JPEG
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_FILE_FORMAT, &[0x00, 0x0B]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:FileFormat"), Some("JPEG"));
    }

    #[test]
    fn test_file_format_tiff() {
        // Code 3 = TIFF
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_FILE_FORMAT, &[0x00, 0x03]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:FileFormat"), Some("TIFF"));
    }

    #[test]
    fn test_file_format_png() {
        // Code 19 = PNG
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_FILE_FORMAT, &[0x00, 0x13]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:FileFormat"), Some("PNG"));
    }

    #[test]
    fn test_file_format_unknown() {
        // Code 255 = Unknown (should show as number)
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_FILE_FORMAT, &[0x00, 0xFF]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:FileFormat"), Some("255"));
    }

    #[test]
    fn test_file_format_no_object_data() {
        // Code 0 = No ObjectData
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_FILE_FORMAT, &[0x00, 0x00]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(
            metadata.get_string("IPTC:FileFormat"),
            Some("No ObjectData")
        );
    }

    // -------------------------------------------------------------------------
    // FileVersion (Dataset 22) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_file_version_parsing() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_FILE_VERSION, &[0x00, 0x01]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:FileVersion"), Some(1));
    }

    #[test]
    fn test_file_version_high_value() {
        // Version 256 (0x0100)
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_FILE_VERSION, &[0x01, 0x00]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:FileVersion"), Some(256));
    }

    // -------------------------------------------------------------------------
    // ServiceIdentifier (Dataset 30) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_service_identifier_parsing() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_SERVICE_IDENTIFIER, b"AFP");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:ServiceIdentifier"), Some("AFP"));
    }

    #[test]
    fn test_service_identifier_max_length() {
        // Max 10 characters per spec
        let data = make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_SERVICE_IDENTIFIER,
            b"REUTERS123",
        );
        let metadata = parse_iptc_record1(&data);

        assert_eq!(
            metadata.get_string("IPTC:ServiceIdentifier"),
            Some("REUTERS123")
        );
    }

    // -------------------------------------------------------------------------
    // EnvelopeNumber (Dataset 40) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_envelope_number_parsing() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_ENVELOPE_NUMBER, b"00012345");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:EnvelopeNumber"), Some("00012345"));
    }

    #[test]
    fn test_envelope_number_numeric() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_ENVELOPE_NUMBER, b"99999999");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:EnvelopeNumber"), Some("99999999"));
    }

    // -------------------------------------------------------------------------
    // ProductID (Dataset 50) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_product_id_parsing() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_PRODUCT_ID, b"NEWS-PHOTOS");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:ProductID"), Some("NEWS-PHOTOS"));
    }

    // -------------------------------------------------------------------------
    // EnvelopePriority (Dataset 60) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_envelope_priority_ascii_digit() {
        // ASCII '5' = 0x35 = normal priority
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_ENVELOPE_PRIORITY, b"5");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:EnvelopePriority"), Some(5));
    }

    #[test]
    fn test_envelope_priority_binary() {
        // Binary 1 = most urgent
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_ENVELOPE_PRIORITY, &[0x01]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:EnvelopePriority"), Some(1));
    }

    #[test]
    fn test_envelope_priority_highest() {
        // Priority 1 = most urgent
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_ENVELOPE_PRIORITY, b"1");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:EnvelopePriority"), Some(1));
    }

    #[test]
    fn test_envelope_priority_lowest() {
        // Priority 9 = user-defined (typically lowest)
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_ENVELOPE_PRIORITY, b"9");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:EnvelopePriority"), Some(9));
    }

    // -------------------------------------------------------------------------
    // DateSent (Dataset 70) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_date_sent_parsing() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_DATE_SENT, b"20231215");
        let metadata = parse_iptc_record1(&data);

        // Should be formatted as YYYY:MM:DD
        assert_eq!(metadata.get_string("IPTC:DateSent"), Some("2023:12:15"));
    }

    #[test]
    fn test_date_sent_y2k() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_DATE_SENT, b"20000101");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:DateSent"), Some("2000:01:01"));
    }

    #[test]
    fn test_date_sent_invalid_length() {
        // Invalid date (wrong length) should pass through unchanged
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_DATE_SENT, b"2023121");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:DateSent"), Some("2023121"));
    }

    // -------------------------------------------------------------------------
    // TimeSent (Dataset 80) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_time_sent_with_timezone() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_TIME_SENT, b"143022+0100");
        let metadata = parse_iptc_record1(&data);

        // Should be formatted as HH:MM:SS±HH:MM
        assert_eq!(metadata.get_string("IPTC:TimeSent"), Some("14:30:22+01:00"));
    }

    #[test]
    fn test_time_sent_negative_timezone() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_TIME_SENT, b"093000-0500");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:TimeSent"), Some("09:30:00-05:00"));
    }

    #[test]
    fn test_time_sent_without_timezone() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_TIME_SENT, b"120000");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:TimeSent"), Some("12:00:00"));
    }

    // -------------------------------------------------------------------------
    // CodedCharacterSet (Dataset 90) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_coded_character_set_utf8() {
        // ESC % G = UTF-8
        let data = make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_CODED_CHARACTER_SET,
            &[0x1B, 0x25, 0x47],
        );
        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_string("IPTC:CodedCharacterSet"), Some("UTF-8"));
    }

    #[test]
    fn test_coded_character_set_latin1() {
        // ESC . A = ISO-8859-1
        let data = make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_CODED_CHARACTER_SET,
            &[0x1B, 0x2E, 0x41],
        );
        let metadata = parse_iptc_record1(&data);

        assert_eq!(
            metadata.get_string("IPTC:CodedCharacterSet"),
            Some("ISO-8859-1")
        );
    }

    #[test]
    fn test_coded_character_set_latin2() {
        // ESC . B = ISO-8859-2
        let data = make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_CODED_CHARACTER_SET,
            &[0x1B, 0x2E, 0x42],
        );
        let metadata = parse_iptc_record1(&data);

        assert_eq!(
            metadata.get_string("IPTC:CodedCharacterSet"),
            Some("ISO-8859-2")
        );
    }

    #[test]
    fn test_coded_character_set_unknown() {
        // Unknown escape sequence should show as hex
        let data = make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_CODED_CHARACTER_SET,
            &[0x1B, 0x99, 0x99],
        );
        let metadata = parse_iptc_record1(&data);

        let value = metadata.get_string("IPTC:CodedCharacterSet");
        assert!(value.is_some());
        assert!(value.unwrap().contains("1B"));
    }

    #[test]
    fn test_coded_character_set_empty() {
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, DATASET_CODED_CHARACTER_SET, &[]);
        let metadata = parse_iptc_record1(&data);

        assert_eq!(
            metadata.get_string("IPTC:CodedCharacterSet"),
            Some("(none)")
        );
    }

    // -------------------------------------------------------------------------
    // UniqueObjectName (Dataset 100) Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_unique_object_name_parsing() {
        let data = make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_UNIQUE_OBJECT_NAME,
            b"urn:newsml:example.com:20231215:news123",
        );
        let metadata = parse_iptc_record1(&data);

        assert_eq!(
            metadata.get_string("IPTC:UniqueObjectName"),
            Some("urn:newsml:example.com:20231215:news123")
        );
    }

    // -------------------------------------------------------------------------
    // Multiple Datasets Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_multiple_record1_datasets() {
        let mut data = Vec::new();

        // ModelVersion = 4
        data.extend(make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_MODEL_VERSION,
            &[0x00, 0x04],
        ));

        // ServiceIdentifier = "REUTERS"
        data.extend(make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_SERVICE_IDENTIFIER,
            b"REUTERS",
        ));

        // DateSent = 20231215
        data.extend(make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_DATE_SENT,
            b"20231215",
        ));

        // EnvelopePriority = 5
        data.extend(make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_ENVELOPE_PRIORITY,
            b"5",
        ));

        let metadata = parse_iptc_record1(&data);

        assert_eq!(metadata.get_integer("IPTC:ModelVersion"), Some(4));
        assert_eq!(
            metadata.get_string("IPTC:ServiceIdentifier"),
            Some("REUTERS")
        );
        assert_eq!(metadata.get_string("IPTC:DateSent"), Some("2023:12:15"));
        assert_eq!(metadata.get_integer("IPTC:EnvelopePriority"), Some(5));
    }

    // -------------------------------------------------------------------------
    // Edge Cases and Error Handling Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_empty_data() {
        let metadata = parse_iptc_record1(&[]);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_truncated_header() {
        // Only 3 bytes (incomplete header)
        let metadata = parse_iptc_record1(&[0x1C, 0x01, 0x00]);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_truncated_payload() {
        // Header indicates 10 bytes but only 5 are present
        let data = vec![
            0x1C, 0x01, 0x05, // Tag marker, Record 1, Dataset 5
            0x00, 0x0A, // Length: 10 bytes
            b'H', b'E', b'L', b'L', b'O', // Only 5 bytes of payload
        ];
        let metadata = parse_iptc_record1(&data);

        // Should not crash, and should not extract partial data
        assert!(metadata.get("IPTC:Destination").is_none());
    }

    #[test]
    fn test_wrong_tag_marker() {
        // Invalid tag marker (not 0x1C)
        let data = vec![0xFF, 0x01, 0x00, 0x00, 0x02, 0x00, 0x04];
        let metadata = parse_iptc_record1(&data);
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_record2_datasets_ignored() {
        // Record 2 (Application Record) datasets should be ignored
        let mut data = Vec::new();

        // Record 1 tag (should be parsed)
        data.extend(make_iptc_dataset(
            ENVELOPE_RECORD_NUMBER,
            DATASET_MODEL_VERSION,
            &[0x00, 0x04],
        ));

        // Record 2 tag (should be ignored)
        data.extend(make_iptc_dataset(
            2, // Record 2 (Application Record)
            5, // ObjectName dataset
            b"Ignored",
        ));

        let metadata = parse_iptc_record1(&data);

        // Should only have the Record 1 tag
        assert_eq!(metadata.get_integer("IPTC:ModelVersion"), Some(4));
        assert!(metadata.get("IPTC:ObjectName").is_none());
    }

    #[test]
    fn test_unknown_dataset_stored_as_generic() {
        // Unknown dataset number (e.g., 200) should be stored with generic name
        let data = make_iptc_dataset(ENVELOPE_RECORD_NUMBER, 200, b"UnknownData");
        let metadata = parse_iptc_record1(&data);

        assert_eq!(
            metadata.get_string("IPTC:Envelope-200"),
            Some("UnknownData")
        );
    }

    // -------------------------------------------------------------------------
    // Helper Function Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_binary_u16() {
        assert_eq!(parse_binary_u16(&[0x00, 0x01]), Some(1));
        assert_eq!(parse_binary_u16(&[0x01, 0x00]), Some(256));
        assert_eq!(parse_binary_u16(&[0xFF, 0xFF]), Some(65535));
        assert_eq!(parse_binary_u16(&[0x00]), None);
        assert_eq!(parse_binary_u16(&[]), None);
    }

    #[test]
    fn test_decode_iptc_string_utf8() {
        let utf8_bytes = "Hello World".as_bytes();
        assert_eq!(decode_iptc_string(utf8_bytes), "Hello World");
    }

    #[test]
    fn test_decode_iptc_string_trimming() {
        let with_spaces = "  trimmed  ".as_bytes();
        assert_eq!(decode_iptc_string(with_spaces), "trimmed");
    }

    #[test]
    fn test_decode_iptc_string_latin1() {
        // Latin-1 encoded "Cafe" with accented e (0xE9)
        let latin1_bytes = vec![0x43, 0x61, 0x66, 0xE9]; // "Cafe" with accented e
        let decoded = decode_iptc_string(&latin1_bytes);
        assert!(decoded.contains("Caf"));
    }

    #[test]
    fn test_format_file_format_code() {
        assert_eq!(format_file_format_code(0), "No ObjectData");
        assert_eq!(format_file_format_code(3), "TIFF");
        assert_eq!(format_file_format_code(11), "JPEG");
        assert_eq!(format_file_format_code(19), "PNG");
        assert_eq!(format_file_format_code(21), "PSD");
        assert_eq!(format_file_format_code(999), "999");
    }

    #[test]
    fn test_decode_character_set() {
        // UTF-8
        assert_eq!(decode_character_set(&[0x1B, 0x25, 0x47]), "UTF-8");

        // ISO-8859-1
        assert_eq!(decode_character_set(&[0x1B, 0x2E, 0x41]), "ISO-8859-1");

        // Empty
        assert_eq!(decode_character_set(&[]), "(none)");
    }
}
