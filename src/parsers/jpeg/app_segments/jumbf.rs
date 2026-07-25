//! JUMBF (JPEG Universal Metadata Box Format) parser for APP11 segments
//!
//! This module implements parsing of JUMBF metadata boxes according to ISO/IEC 19566-5.
//! JUMBF provides a universal format to embed any type of metadata in box-based JPEG files,
//! including C2PA (Content Authenticity Initiative) data for provenance tracking.
//!
//! # Format Overview
//!
//! JUMBF is used in JPEG files via APP11 segments that start with "JPXX\0" identifier
//! (where XX represents sequence numbers). The format uses a box-based structure similar
//! to ISOBMFF (ISO Base Media File Format) used in MP4/MOV files.
//!
//! # Box Structure
//!
//! Each JUMBF box follows this structure:
//! ```text
//! Offset  Size  Description
//! 0       4     Box length (big-endian u32, including header)
//! 4       4     Box type (4-character code)
//! 8       N     Box data (depends on type)
//! ```
//!
//! # Supported Content
//!
//! - **C2PA Manifest Stores**: Content authenticity and provenance data
//! - **Assertions**: Cryptographic claims about image content
//! - **Ingredients**: Information about source materials
//! - **Actions**: Edit history and transformations
//! - **Claims**: Verifiable statements about the content
//!
//! # ExifTool Compatibility
//!
//! Tags are output with appropriate family prefixes (JUMBF, C2PA, etc.) to match
//! ExifTool's output format.
//!
//! # References
//!
//! - ISO/IEC 19566-5:2023 - JPEG Universal Metadata Box Format
//! - C2PA Technical Specification v2.0
//! - ISO/IEC 18477-3 - JPEG XT Box File Format

use crate::core::{MetadataMap, TagValue};
use crate::error::Result;
use crate::io::EndianReader;

/// JUMBF identifier prefix for APP11 segments ("JPXX\0" pattern)
const JUMBF_IDENTIFIER_PREFIX: &[u8] = b"JP";

/// Minimum box size (length + type fields)
const MIN_BOX_SIZE: usize = 8;

/// Box type identifiers (4-character codes)
const BOX_TYPE_JUMB: &[u8] = b"jumb"; // JUMBF superbox
const BOX_TYPE_JUMD: &[u8] = b"jumd"; // JUMBF description box
const BOX_TYPE_JSON: &[u8] = b"json"; // JSON content
const BOX_TYPE_CBOR: &[u8] = b"cbor"; // CBOR content
const BOX_TYPE_UUID: &[u8] = b"uuid"; // UUID box
const BOX_TYPE_C2PA: &[u8] = b"c2pa"; // C2PA manifest store
const BOX_TYPE_C2MA: &[u8] = b"c2ma"; // C2PA manifest
const BOX_TYPE_C2CL: &[u8] = b"c2cl"; // C2PA claim
const BOX_TYPE_C2AS: &[u8] = b"c2as"; // C2PA assertion store
const BOX_TYPE_C2CS: &[u8] = b"c2cs"; // C2PA claim signature

/// JUMBF content type identifiers (from ISO 19566-5)
const CONTENT_TYPE_JSON: &[u8] = b"json";
const CONTENT_TYPE_CBOR: &[u8] = b"cbor";
const CONTENT_TYPE_CODESTREAM: &[u8] = b"jp2c";

/// Parsed JUMBF box structure
#[derive(Debug, Clone)]
struct JumbfBox<'a> {
    /// Box type (4-character code)
    box_type: &'a [u8],
    /// Box length including header (0 means extends to end of data)
    length: u32,
    /// Box data payload (excluding header)
    data: &'a [u8],
}

/// JUMBF description box contents
#[derive(Debug, Clone)]
struct JumbfDescription {
    /// Content type UUID or content type identifier
    content_type: Vec<u8>,
    /// Optional label (UTF-8 null-terminated string)
    label: Option<String>,
    /// Toggles and flags
    toggles: u8,
}

/// Parses JUMBF metadata from an APP11 segment.
///
/// This function detects and parses JUMBF box structures containing C2PA and other
/// metadata. It handles multiple APP11 segments that may be split across JPEG markers.
///
/// # Arguments
///
/// * `data` - Raw APP11 segment data (excluding APP11 marker and length bytes)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed JUMBF metadata tags
/// * `Err` - If the segment is not JUMBF format or parsing fails
///
/// # Supported Tags
///
/// The parser extracts these tag families:
/// - `JUMBF:*` - General JUMBF box metadata
/// - `C2PA:*` - C2PA content authenticity tags
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::jumbf::parse_jumbf;
///
/// let segment_data = &[/* APP11 segment bytes */];
/// match parse_jumbf(segment_data) {
///     Ok(metadata) => {
///         if let Some(version) = metadata.get_string("C2PA:Version") {
///             println!("C2PA Version: {}", version);
///         }
///     }
///     Err(e) => eprintln!("Not a JUMBF segment: {}", e),
/// }
/// ```
pub fn parse_jumbf(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Check for JUMBF identifier pattern "JPXX\0" (where XX is typically digits)
    if !is_jumbf_segment(data) {
        return Err(crate::error::ExifToolError::parse_error(
            "Not a JUMBF segment (expected JPXX\\0 identifier)",
        ));
    }

    // Skip past the JUMBF identifier to get to box data
    // Pattern: "JP" + 2 sequence chars + null byte = 5 bytes minimum
    let box_data = if data.len() >= 5 && data[0] == b'J' && data[1] == b'P' {
        // Find the null terminator
        let null_pos = data.iter().position(|&b| b == 0).unwrap_or(4);
        &data[null_pos + 1..]
    } else {
        return Err(crate::error::ExifToolError::parse_error(
            "Invalid JUMBF identifier format",
        ));
    };

    // Parse JUMBF boxes
    parse_jumbf_boxes(box_data, &mut metadata, 0)?;

    Ok(metadata)
}

/// Checks if segment data starts with JUMBF identifier
fn is_jumbf_segment(data: &[u8]) -> bool {
    data.len() >= 5
        && data[0..2] == *JUMBF_IDENTIFIER_PREFIX
        && data[2].is_ascii_alphanumeric()
        && data[3].is_ascii_alphanumeric()
        && data[4] == 0
}

/// Parses JUMBF box hierarchy recursively
fn parse_jumbf_boxes(data: &[u8], metadata: &mut MetadataMap, depth: usize) -> Result<()> {
    // Prevent excessive recursion
    if depth > 20 {
        return Err(crate::error::ExifToolError::parse_error(
            "JUMBF box nesting too deep",
        ));
    }

    let mut offset = 0;

    while offset + MIN_BOX_SIZE <= data.len() {
        // Parse box header
        let jumbf_box = match parse_box_header(&data[offset..]) {
            Ok(b) => b,
            Err(_) => break, // End of valid boxes
        };

        // Process box based on type
        process_jumbf_box(&jumbf_box, metadata, depth)?;

        // Move to next box
        let box_size = if jumbf_box.length == 0 {
            // Length 0 means box extends to end of data
            data.len() - offset
        } else {
            jumbf_box.length as usize
        };

        offset += box_size;

        // Safety check to prevent infinite loops
        if box_size == 0 || offset > data.len() {
            break;
        }
    }

    Ok(())
}

/// Parses a single JUMBF box header
fn parse_box_header(data: &[u8]) -> Result<JumbfBox<'_>> {
    if data.len() < MIN_BOX_SIZE {
        return Err(crate::error::ExifToolError::parse_error(
            "Insufficient data for JUMBF box header",
        ));
    }

    let reader = EndianReader::big_endian(data);

    // Read box length (4 bytes, big-endian)
    let length = reader.u32_at(0).ok_or_else(|| {
        crate::error::ExifToolError::parse_error("Failed to read JUMBF box length")
    })?;

    // Read box type (4 bytes)
    let box_type = &data[4..8];

    // Calculate data payload size
    let header_size = 8;
    let data_size = if length == 0 {
        // Length 0 means extends to end
        data.len() - header_size
    } else if length < header_size as u32 {
        return Err(crate::error::ExifToolError::parse_error(
            "Invalid JUMBF box length (too small)",
        ));
    } else {
        (length as usize) - header_size
    };

    let box_data = if header_size + data_size <= data.len() {
        &data[header_size..header_size + data_size]
    } else {
        &data[header_size..]
    };

    Ok(JumbfBox {
        box_type,
        length,
        data: box_data,
    })
}

/// Processes a JUMBF box and extracts metadata
fn process_jumbf_box(jumbf_box: &JumbfBox, metadata: &mut MetadataMap, depth: usize) -> Result<()> {
    match jumbf_box.box_type {
        BOX_TYPE_JUMB => {
            // JUMBF superbox - contains nested boxes
            parse_jumbf_superbox(jumbf_box.data, metadata, depth)?;
        }
        BOX_TYPE_JUMD => {
            // JUMBF description box
            parse_jumbf_description(jumbf_box.data, metadata)?;
        }
        BOX_TYPE_JSON => {
            // JSON content
            parse_json_content(jumbf_box.data, metadata)?;
        }
        BOX_TYPE_CBOR => {
            // CBOR content - store as binary for now
            metadata.insert(
                "JUMBF:CBORData".to_string(),
                TagValue::String(format!("(Binary data {} bytes)", jumbf_box.data.len())),
            );
        }
        BOX_TYPE_UUID => {
            // UUID box - extract UUID
            parse_uuid_box(jumbf_box.data, metadata)?;
        }
        BOX_TYPE_C2PA => {
            // C2PA manifest store
            metadata.insert(
                "C2PA:ManifestStore".to_string(),
                TagValue::String("Present".to_string()),
            );
            parse_jumbf_boxes(jumbf_box.data, metadata, depth + 1)?;
        }
        BOX_TYPE_C2MA => {
            // C2PA manifest
            metadata.insert(
                "C2PA:Manifest".to_string(),
                TagValue::String("Present".to_string()),
            );
            parse_c2pa_manifest(jumbf_box.data, metadata)?;
        }
        BOX_TYPE_C2CL => {
            // C2PA claim
            metadata.insert(
                "C2PA:Claim".to_string(),
                TagValue::String("Present".to_string()),
            );
            parse_c2pa_claim(jumbf_box.data, metadata)?;
        }
        BOX_TYPE_C2AS => {
            // C2PA assertion store
            metadata.insert(
                "C2PA:AssertionStore".to_string(),
                TagValue::String("Present".to_string()),
            );
            parse_jumbf_boxes(jumbf_box.data, metadata, depth + 1)?;
        }
        BOX_TYPE_C2CS => {
            // C2PA claim signature
            parse_c2pa_signature(jumbf_box.data, metadata)?;
        }
        _ => {
            // Unknown box type - store type for reference
            let box_type_str = String::from_utf8_lossy(jumbf_box.box_type);
            metadata.insert(
                format!("JUMBF:UnknownBox_{}", box_type_str),
                TagValue::String(format!("({} bytes)", jumbf_box.data.len())),
            );
        }
    }

    Ok(())
}

/// Parses JUMBF superbox (contains nested boxes)
fn parse_jumbf_superbox(data: &[u8], metadata: &mut MetadataMap, depth: usize) -> Result<()> {
    // A superbox contains a description box followed by content boxes
    parse_jumbf_boxes(data, metadata, depth + 1)
}

/// Parses JUMBF description box
fn parse_jumbf_description(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.len() < 17 {
        // Minimum: 16 bytes UUID + 1 byte toggles
        return Ok(());
    }

    let _reader = EndianReader::big_endian(data);

    // Read UUID (16 bytes) or content type identifier
    let uuid = &data[0..16];

    // Format UUID as hex string
    let uuid_str = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        uuid[0],
        uuid[1],
        uuid[2],
        uuid[3],
        uuid[4],
        uuid[5],
        uuid[6],
        uuid[7],
        uuid[8],
        uuid[9],
        uuid[10],
        uuid[11],
        uuid[12],
        uuid[13],
        uuid[14],
        uuid[15]
    );

    metadata.insert("JUMBF:ContentType".to_string(), TagValue::String(uuid_str));

    // Read toggles byte
    if data.len() > 16 {
        let toggles = data[16];

        // Bit 0: requestable flag
        if toggles & 0x01 != 0 {
            metadata.insert(
                "JUMBF:Requestable".to_string(),
                TagValue::String("True".to_string()),
            );
        }

        // Parse label if present (bit 1 indicates label presence in some versions)
        if data.len() > 17 {
            // Label is UTF-8 null-terminated string after toggles
            if let Some(null_pos) = data[17..].iter().position(|&b| b == 0)
                && let Ok(label) = std::str::from_utf8(&data[17..17 + null_pos])
                && !label.is_empty()
            {
                metadata.insert(
                    "JUMBF:Label".to_string(),
                    TagValue::String(label.to_string()),
                );
            }
        }
    }

    Ok(())
}

/// Parses JSON content box
fn parse_json_content(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // Try to parse as UTF-8 JSON string
    if let Ok(json_str) = std::str::from_utf8(data) {
        // For now, store raw JSON (could parse structure in future)
        metadata.insert(
            "JUMBF:JSONData".to_string(),
            TagValue::String(json_str.to_string()),
        );
    } else {
        metadata.insert(
            "JUMBF:JSONData".to_string(),
            TagValue::String(format!("(Binary data {} bytes)", data.len())),
        );
    }
    Ok(())
}

/// Parses UUID box
fn parse_uuid_box(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.len() >= 16 {
        let uuid = &data[0..16];
        let uuid_str = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            uuid[0],
            uuid[1],
            uuid[2],
            uuid[3],
            uuid[4],
            uuid[5],
            uuid[6],
            uuid[7],
            uuid[8],
            uuid[9],
            uuid[10],
            uuid[11],
            uuid[12],
            uuid[13],
            uuid[14],
            uuid[15]
        );

        metadata.insert("JUMBF:UUID".to_string(), TagValue::String(uuid_str));

        // Remaining data after UUID
        if data.len() > 16 {
            metadata.insert(
                "JUMBF:UUIDData".to_string(),
                TagValue::String(format!("({} bytes)", data.len() - 16)),
            );
        }
    }
    Ok(())
}

/// Parses C2PA manifest
fn parse_c2pa_manifest(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // C2PA manifest is typically CBOR or JSON
    // Try to detect format and extract key fields

    // Check if it starts with CBOR magic bytes
    if !data.is_empty() {
        let first_byte = data[0];

        // CBOR major types: map (0xA0-0xBF) or array (0x80-0x9F)
        if (0x80..=0xBF).contains(&first_byte) {
            metadata.insert(
                "C2PA:ManifestFormat".to_string(),
                TagValue::String("CBOR".to_string()),
            );
        }

        metadata.insert(
            "C2PA:ManifestSize".to_string(),
            TagValue::Integer(data.len() as i64),
        );
    }

    Ok(())
}

/// Parses C2PA claim
fn parse_c2pa_claim(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // C2PA claim contains assertions and other metadata
    // For now, just record presence and size
    metadata.insert(
        "C2PA:ClaimSize".to_string(),
        TagValue::Integer(data.len() as i64),
    );

    // Try to parse as JSON if it looks like JSON
    if (data.starts_with(b"{") || data.starts_with(b"["))
        && let Ok(json_str) = std::str::from_utf8(data)
    {
        // Look for common C2PA claim fields
        if json_str.contains("\"dc:title\"") {
            metadata.insert(
                "C2PA:ClaimGenerator".to_string(),
                TagValue::String("Present in claim".to_string()),
            );
        }
        if json_str.contains("\"actions\"") {
            metadata.insert(
                "C2PA:Actions".to_string(),
                TagValue::String("Present in claim".to_string()),
            );
        }
        if json_str.contains("\"assertions\"") {
            metadata.insert(
                "C2PA:Assertions".to_string(),
                TagValue::String("Present in claim".to_string()),
            );
        }
        if json_str.contains("\"ingredients\"") {
            metadata.insert(
                "C2PA:Ingredients".to_string(),
                TagValue::String("Present in claim".to_string()),
            );
        }
    }

    Ok(())
}

/// Parses C2PA claim signature
fn parse_c2pa_signature(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // Signature is typically binary cryptographic data
    metadata.insert(
        "C2PA:ClaimSignature".to_string(),
        TagValue::String(format!("(Binary signature {} bytes)", data.len())),
    );

    // Check if it looks like COSE signature (starts with CBOR map)
    if !data.is_empty() {
        let first_byte = data[0];
        if (0xA0..=0xBF).contains(&first_byte) {
            metadata.insert(
                "C2PA:SignatureFormat".to_string(),
                TagValue::String("COSE".to_string()),
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a minimal JUMBF box
    fn create_jumbf_box(box_type: &[u8], data: &[u8]) -> Vec<u8> {
        let length = (8 + data.len()) as u32;
        let mut result = Vec::new();
        result.extend_from_slice(&length.to_be_bytes());
        result.extend_from_slice(box_type);
        result.extend_from_slice(data);
        result
    }

    /// Helper to create a JUMBF segment with identifier
    fn create_jumbf_segment(boxes: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        // JUMBF identifier: "JP01\0"
        result.extend_from_slice(b"JP01\0");
        result.extend_from_slice(boxes);
        result
    }

    #[test]
    fn test_is_jumbf_segment() {
        let valid = b"JP01\0some data";
        assert!(is_jumbf_segment(valid));

        let invalid1 = b"EXIF\0";
        assert!(!is_jumbf_segment(invalid1));

        let invalid2 = b"JP"; // Too short
        assert!(!is_jumbf_segment(invalid2));

        let invalid3 = b"JP01X"; // Missing null terminator
        assert!(!is_jumbf_segment(invalid3));
    }

    #[test]
    fn test_parse_box_header() {
        // Create a simple box: length=12 (8-byte header + 4-byte data), type="jumb"
        let box_data = create_jumbf_box(b"jumb", &[0x01, 0x02, 0x03, 0x04]);

        let jumbf_box = parse_box_header(&box_data).expect("Failed to parse box");

        assert_eq!(jumbf_box.length, 12); // 8-byte header + 4-byte data
        assert_eq!(jumbf_box.box_type, b"jumb");
        assert_eq!(jumbf_box.data, &[0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_parse_jumbf_description() {
        // Create description box with UUID and label
        let mut desc_data = Vec::new();
        // UUID (16 bytes)
        desc_data.extend_from_slice(&[
            0x6A, 0x75, 0x6D, 0x62, 0x66, 0x00, 0x11, 0x00, 0x10, 0x80, 0x00, 0x00, 0xAA, 0x00,
            0x38, 0x9B,
        ]);
        // Toggles byte
        desc_data.push(0x01);
        // Label "TestLabel\0"
        desc_data.extend_from_slice(b"TestLabel\0");

        let mut metadata = MetadataMap::new();
        let result = parse_jumbf_description(&desc_data, &mut metadata);

        assert!(result.is_ok());
        assert!(metadata.get_string("JUMBF:ContentType").is_some());
        assert_eq!(
            metadata.get_string("JUMBF:Label").as_deref(),
            Some("TestLabel")
        );
    }

    #[test]
    fn test_parse_json_content() {
        let json_data = b"{\"key\": \"value\"}";
        let mut metadata = MetadataMap::new();

        let result = parse_json_content(json_data, &mut metadata);
        assert!(result.is_ok());
        assert!(metadata.get_string("JUMBF:JSONData").is_some());
    }

    #[test]
    fn test_parse_c2pa_manifest() {
        // Simulate CBOR data (map with 2 elements)
        let cbor_data = &[0xA2, 0x01, 0x02, 0x03, 0x04];
        let mut metadata = MetadataMap::new();

        let result = parse_c2pa_manifest(cbor_data, &mut metadata);
        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("C2PA:ManifestFormat").as_deref(),
            Some("CBOR")
        );
        assert_eq!(metadata.get_integer("C2PA:ManifestSize"), Some(5));
    }

    #[test]
    fn test_parse_c2pa_claim_with_json() {
        let json_claim =
            br#"{"dc:title": "Test", "actions": [], "assertions": [], "ingredients": []}"#;
        let mut metadata = MetadataMap::new();

        let result = parse_c2pa_claim(json_claim, &mut metadata);
        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("C2PA:ClaimGenerator").as_deref(),
            Some("Present in claim")
        );
        assert_eq!(
            metadata.get_string("C2PA:Actions").as_deref(),
            Some("Present in claim")
        );
    }

    #[test]
    fn test_parse_complete_jumbf_segment() {
        // Create a simple JUMBF segment with nested boxes
        let mut boxes = Vec::new();

        // Add a description box
        let mut desc_data = Vec::new();
        desc_data.extend_from_slice(&[0u8; 16]); // UUID
        desc_data.push(0x00); // Toggles
        let desc_box = create_jumbf_box(BOX_TYPE_JUMD, &desc_data);
        boxes.extend_from_slice(&desc_box);

        // Create segment
        let segment = create_jumbf_segment(&boxes);

        // Parse it
        let result = parse_jumbf(&segment);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert!(metadata.get_string("JUMBF:ContentType").is_some());
    }

    #[test]
    fn test_parse_c2pa_signature() {
        // Simulate COSE signature (CBOR map)
        let cose_data = &[0xA3, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let mut metadata = MetadataMap::new();

        let result = parse_c2pa_signature(cose_data, &mut metadata);
        assert!(result.is_ok());
        assert!(
            metadata
                .get_string("C2PA:ClaimSignature")
                .unwrap()
                .contains("Binary signature")
        );
        assert_eq!(
            metadata.get_string("C2PA:SignatureFormat").as_deref(),
            Some("COSE")
        );
    }

    #[test]
    fn test_invalid_jumbf_identifier() {
        let invalid = b"EXIF\0\x00\x00\x00\x10jumb";
        let result = parse_jumbf(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_segment() {
        let empty = b"JP01\0";
        let result = parse_jumbf(empty);
        // Should succeed but produce empty metadata
        assert!(result.is_ok());
    }
}
