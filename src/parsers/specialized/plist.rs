//! macOS Property List (Plist) parser for extracting metadata
//!
//! Implements metadata extraction from macOS Property List files in both Binary and XML formats.
//! Property lists are used extensively in macOS/iOS for configuration files, application metadata,
//! and system settings (e.g., Info.plist, launchd plists, preference files).
//!
//! # Format Variants
//!
//! - **Binary Plist**: Compact binary format starting with `bplist00` or `bplist01` magic bytes
//! - **XML Plist**: XML format with `<?xml` declaration and `<plist>` root element
//!
//! # Binary Plist Structure
//!
//! ```text
//! +------------------+
//! | Header (8 bytes) |  "bplist0X" where X is format version (0 or 1)
//! +------------------+
//! | Objects          |  Variable-length encoded objects
//! +------------------+
//! | Offset Table     |  Array of offsets to objects
//! +------------------+
//! | Trailer (32 B)   |  Format metadata and pointers
//! +------------------+
//! ```
//!
//! # Trailer Structure (32 bytes)
//!
//! ```text
//! [0-5]   : Unused (padding)
//! [6]     : Offset integer size (bytes per offset in offset table)
//! [7]     : Object reference size (bytes per object ref)
//! [8-15]  : Number of objects (big-endian u64)
//! [16-23] : Top object index (big-endian u64)
//! [24-31] : Offset table offset (big-endian u64)
//! ```
//!
//! # Common Keys
//!
//! The parser extracts commonly found keys:
//! - `CFBundleIdentifier`: Application identifier (e.g., com.apple.Safari)
//! - `CFBundleName`: Human-readable application name
//! - `CFBundleVersion`: Application version
//! - `Label`: Launchd service label (for launchd plists)
//!
//! # References
//!
//! - Apple Property List Format: https://opensource.apple.com/source/CF/
//! - Binary Format: https://medium.com/@karaiskc/understanding-apples-binary-property-list-format-281e6da00dbd

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// Binary plist magic bytes (version 0)
const BPLIST_MAGIC_V0: &[u8] = b"bplist00";

/// Binary plist magic bytes (version 1)
const BPLIST_MAGIC_V1: &[u8] = b"bplist01";

/// Binary plist trailer size (last 32 bytes)
const BPLIST_TRAILER_SIZE: usize = 32;

/// Minimum file size for binary plist (header + trailer)
const BPLIST_MIN_SIZE: usize = 8 + BPLIST_TRAILER_SIZE;

/// XML plist identifiers
const XML_DECLARATION: &[u8] = b"<?xml";
const PLIST_TAG: &[u8] = b"<plist";
const DOCTYPE_PLIST: &[u8] = b"<!DOCTYPE plist";

/// macOS Property List parser for extracting metadata
pub struct PlistParser;

impl PlistParser {
    /// Verifies plist signature by checking for binary or XML format
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the plist file
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Valid plist signature detected (binary or XML)
    /// * `Ok(false)` - Invalid or missing signature
    /// * `Err` - I/O error reading the file
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        // Check file is large enough for at least XML header
        if reader.size() < 8 {
            return Ok(false);
        }

        // Check for binary plist magic
        let header = reader.read(0, 8)?;
        if header == BPLIST_MAGIC_V0 || header == BPLIST_MAGIC_V1 {
            return Ok(true);
        }

        // Check for XML plist
        // Read more bytes for XML detection (up to 512 bytes)
        let check_size = reader.size().min(512) as usize;
        let data = reader.read(0, check_size)?;

        // Check for <?xml declaration
        if data.len() >= XML_DECLARATION.len() && &data[..XML_DECLARATION.len()] == XML_DECLARATION
        {
            // Look for <plist or <!DOCTYPE plist within the checked bytes
            if Self::contains_subsequence(data, PLIST_TAG)
                || Self::contains_subsequence(data, DOCTYPE_PLIST)
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Helper function to check if a slice contains a subsequence
    fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    /// Detects whether the plist is Binary or XML format
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the plist file
    ///
    /// # Returns
    ///
    /// * Format string: "Binary" or "XML"
    fn detect_format(reader: &dyn FileReader) -> Result<&'static str> {
        if reader.size() < 8 {
            return Ok("Unknown");
        }

        let header = reader.read(0, 8)?;
        if header == BPLIST_MAGIC_V0 || header == BPLIST_MAGIC_V1 {
            Ok("Binary")
        } else {
            // Assume XML if not binary (signature verification already passed)
            Ok("XML")
        }
    }

    /// Extracts format version from binary plist header
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the plist file
    ///
    /// # Returns
    ///
    /// * Version string (e.g., "00", "01")
    fn extract_format_version(reader: &dyn FileReader) -> Result<String> {
        if reader.size() < 8 {
            return Ok("Unknown".to_string());
        }

        let header = reader.read(0, 8)?;
        if header.starts_with(b"bplist") && header.len() >= 8 {
            // Extract version bytes (positions 6-7)
            let version = String::from_utf8_lossy(&header[6..8]);
            Ok(version.to_string())
        } else {
            Ok("1.0".to_string()) // XML plist default version
        }
    }

    /// Parses binary plist format and extracts metadata
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the plist file
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted metadata from binary plist
    /// * `Err` - Parse error
    fn parse_binary_plist(reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();

        // Validate file size
        if reader.size() < BPLIST_MIN_SIZE as u64 {
            return Err(ExifToolError::parse_error(
                "File too small for binary plist format",
            ));
        }

        // Read trailer (last 32 bytes)
        let file_size = reader.size();
        let trailer_offset = file_size - BPLIST_TRAILER_SIZE as u64;
        let trailer = reader.read(trailer_offset, BPLIST_TRAILER_SIZE)?;

        // Parse trailer fields
        let offset_int_size = trailer[6];
        let object_ref_size = trailer[7];
        let num_objects = Self::read_u64_be(&trailer[8..16]);
        let top_object = Self::read_u64_be(&trailer[16..24]);
        let offset_table_offset = Self::read_u64_be(&trailer[24..32]);

        // Add binary plist specific metadata
        metadata.insert(
            "Plist:OffsetIntSize".to_string(),
            TagValue::String(offset_int_size.to_string()),
        );
        metadata.insert(
            "Plist:ObjectRefSize".to_string(),
            TagValue::String(object_ref_size.to_string()),
        );
        metadata.insert(
            "Plist:NumObjects".to_string(),
            TagValue::String(num_objects.to_string()),
        );
        metadata.insert(
            "Plist:TopObjectIndex".to_string(),
            TagValue::String(top_object.to_string()),
        );
        metadata.insert(
            "Plist:OffsetTableOffset".to_string(),
            TagValue::String(format!("0x{:X}", offset_table_offset)),
        );
        metadata.insert(
            "Plist:TrailerSize".to_string(),
            TagValue::String(BPLIST_TRAILER_SIZE.to_string()),
        );

        // Validate trailer values
        if offset_int_size == 0 || offset_int_size > 8 {
            metadata.insert(
                "Plist:Warning".to_string(),
                TagValue::String(format!("Invalid offset int size: {}", offset_int_size)),
            );
        }

        if object_ref_size == 0 || object_ref_size > 8 {
            metadata.insert(
                "Plist:Warning".to_string(),
                TagValue::String(format!("Invalid object ref size: {}", object_ref_size)),
            );
        }

        if offset_table_offset >= file_size {
            metadata.insert(
                "Plist:Warning".to_string(),
                TagValue::String(format!(
                    "Offset table offset (0x{:X}) exceeds file size",
                    offset_table_offset
                )),
            );
        }

        // Try to determine root object type (simplified - requires full parsing)
        // For now, we'll note that full object parsing would be needed
        metadata.insert(
            "Plist:Note".to_string(),
            TagValue::String(
                "Full object tree parsing requires additional implementation".to_string(),
            ),
        );

        Ok(metadata)
    }

    /// Parses XML plist format and extracts metadata
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the plist file
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted metadata from XML plist
    /// * `Err` - Parse error
    fn parse_xml_plist(reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();

        // Read file content for string searching (limit to first 64KB for performance)
        let read_size = reader.size().min(65536) as usize;
        let content = reader.read(0, read_size)?;
        let text = String::from_utf8_lossy(content);

        // Extract common plist keys using simple string matching
        // Note: This is a simplified parser. A production implementation would use
        // a proper XML parser for robustness.

        // Look for CFBundleIdentifier
        if let Some(value) = Self::extract_plist_string(&text, "CFBundleIdentifier") {
            metadata.insert(
                "Plist:CFBundleIdentifier".to_string(),
                TagValue::String(value),
            );
        }

        // Look for CFBundleName
        if let Some(value) = Self::extract_plist_string(&text, "CFBundleName") {
            metadata.insert("Plist:CFBundleName".to_string(), TagValue::String(value));
        }

        // Look for CFBundleVersion
        if let Some(value) = Self::extract_plist_string(&text, "CFBundleVersion") {
            metadata.insert("Plist:CFBundleVersion".to_string(), TagValue::String(value));
        }

        // Look for CFBundleShortVersionString
        if let Some(value) = Self::extract_plist_string(&text, "CFBundleShortVersionString") {
            metadata.insert(
                "Plist:CFBundleShortVersionString".to_string(),
                TagValue::String(value),
            );
        }

        // Look for Label (launchd plists)
        if let Some(value) = Self::extract_plist_string(&text, "Label") {
            metadata.insert("Plist:Label".to_string(), TagValue::String(value));
        }

        // Count top-level dict keys (approximate)
        let key_count = text.matches("<key>").count();
        if key_count > 0 {
            metadata.insert(
                "Plist:KeyCount".to_string(),
                TagValue::String(key_count.to_string()),
            );
        }

        // Detect root object type
        let root_type = if text.contains("<dict>") {
            "Dictionary"
        } else if text.contains("<array>") {
            "Array"
        } else if text.contains("<string>") {
            "String"
        } else if text.contains("<data>") {
            "Data"
        } else {
            "Unknown"
        };
        metadata.insert(
            "Plist:RootObjectType".to_string(),
            TagValue::String(root_type.to_string()),
        );

        Ok(metadata)
    }

    /// Extracts a string value for a given key from XML plist content
    ///
    /// # Arguments
    ///
    /// * `text` - XML plist content as string
    /// * `key` - Key to search for
    ///
    /// # Returns
    ///
    /// * `Some(String)` - Value found for the key
    /// * `None` - Key not found or value could not be extracted
    fn extract_plist_string(text: &str, key: &str) -> Option<String> {
        // Look for pattern: <key>KeyName</key><string>Value</string>
        let key_pattern = format!("<key>{}</key>", key);
        if let Some(key_pos) = text.find(&key_pattern) {
            let after_key = &text[key_pos + key_pattern.len()..];

            // Skip whitespace and find <string>
            if let Some(string_start) = after_key.find("<string>") {
                let value_start = string_start + "<string>".len();
                if let Some(string_end) = after_key[value_start..].find("</string>") {
                    let value = &after_key[value_start..value_start + string_end];
                    return Some(value.trim().to_string());
                }
            }
        }
        None
    }

    /// Reads a big-endian u64 from a byte slice
    /// Binary plist uses big-endian byte order
    fn read_u64_be(bytes: &[u8]) -> u64 {
        if bytes.len() < 8 {
            return 0;
        }
        let reader = EndianReader::big_endian(bytes);
        reader.u64_at(0).unwrap_or(0)
    }
}

impl FormatParser for PlistParser {
    /// Parses metadata from a Property List file
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the plist file
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted metadata including format info and common keys
    /// * `Err(ExifToolError)` - Invalid signature or parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify this is a valid plist file
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid plist signature"));
        }

        let mut metadata = MetadataMap::new();

        // Basic file information
        metadata.insert(
            "FileType".to_string(),
            TagValue::String("Plist".to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Detect and add format
        let format = Self::detect_format(reader)?;
        metadata.insert(
            "Plist:Format".to_string(),
            TagValue::String(format.to_string()),
        );

        // Add format version
        let version = Self::extract_format_version(reader)?;
        metadata.insert("Plist:FormatVersion".to_string(), TagValue::String(version));

        // Parse format-specific metadata
        let format_metadata = if format == "Binary" {
            Self::parse_binary_plist(reader)?
        } else {
            Self::parse_xml_plist(reader)?
        };

        // Merge format-specific metadata
        for (key, value) in format_metadata {
            metadata.insert(key, value);
        }

        Ok(metadata)
    }

    /// Checks if this parser supports the given format
    ///
    /// # Arguments
    ///
    /// * `format` - File format to check
    ///
    /// # Returns
    ///
    /// * `true` - Parser supports Plist format
    /// * `false` - Parser does not support the format
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::Plist)
    }
}

/// Parses metadata from macOS Property List files.
///
/// This is the public API function for parsing plist files.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the plist file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::specialized::plist::parse_plist_metadata;
/// use oxidex::io::MMapReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), String> {
/// let reader = MMapReader::new(Path::new("Info.plist"))
///     .map_err(|e| e.to_string())?;
/// let metadata = parse_plist_metadata(&reader)?;
/// println!("Plist metadata: {:?}", metadata);
/// # Ok(())
/// # }
/// ```
pub fn parse_plist_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = PlistParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Creates a minimal valid binary plist for testing
    fn create_test_binary_plist() -> Vec<u8> {
        let mut data = Vec::new();

        // Header: "bplist00"
        data.extend_from_slice(b"bplist00");

        // Simple object data (minimal - just padding for now)
        // In real binary plist, this would contain encoded objects
        let objects_size = 100;
        data.extend(vec![0u8; objects_size]);

        // Trailer (32 bytes)
        let mut trailer = vec![0u8; 32];
        trailer[6] = 2; // offset_int_size = 2 bytes
        trailer[7] = 1; // object_ref_size = 1 byte

        // num_objects = 5 (big-endian u64 at offset 8)
        trailer[8..16].copy_from_slice(&5u64.to_be_bytes());

        // top_object = 0 (big-endian u64 at offset 16)
        trailer[16..24].copy_from_slice(&0u64.to_be_bytes());

        // offset_table_offset = 108 (8 + 100) (big-endian u64 at offset 24)
        trailer[24..32].copy_from_slice(&108u64.to_be_bytes());

        data.extend(trailer);

        data
    }

    /// Creates a minimal XML plist for testing
    fn create_test_xml_plist() -> Vec<u8> {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.example.testapp</string>
    <key>CFBundleName</key>
    <string>TestApp</string>
    <key>CFBundleVersion</key>
    <string>1.2.3</string>
    <key>Label</key>
    <string>com.example.service</string>
</dict>
</plist>"#;
        xml.as_bytes().to_vec()
    }

    #[test]
    fn test_verify_signature_binary_v0() {
        let mut data = vec![0u8; BPLIST_MIN_SIZE];
        data[0..8].copy_from_slice(BPLIST_MAGIC_V0);
        let reader = TestReader::new(data);
        assert!(PlistParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_binary_v1() {
        let mut data = vec![0u8; BPLIST_MIN_SIZE];
        data[0..8].copy_from_slice(BPLIST_MAGIC_V1);
        let reader = TestReader::new(data);
        assert!(PlistParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_xml() {
        let data = create_test_xml_plist();
        let reader = TestReader::new(data);
        assert!(PlistParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_invalid() {
        let data = vec![0u8; 100];
        let reader = TestReader::new(data);
        assert!(!PlistParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_too_small() {
        let data = vec![0u8; 4];
        let reader = TestReader::new(data);
        assert!(!PlistParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_detect_format_binary() {
        let data = create_test_binary_plist();
        let reader = TestReader::new(data);
        assert_eq!(PlistParser::detect_format(&reader).unwrap(), "Binary");
    }

    #[test]
    fn test_detect_format_xml() {
        let data = create_test_xml_plist();
        let reader = TestReader::new(data);
        assert_eq!(PlistParser::detect_format(&reader).unwrap(), "XML");
    }

    #[test]
    fn test_extract_format_version_binary() {
        let data = create_test_binary_plist();
        let reader = TestReader::new(data);
        assert_eq!(PlistParser::extract_format_version(&reader).unwrap(), "00");
    }

    #[test]
    fn test_extract_format_version_xml() {
        let data = create_test_xml_plist();
        let reader = TestReader::new(data);
        assert_eq!(PlistParser::extract_format_version(&reader).unwrap(), "1.0");
    }

    #[test]
    fn test_parse_binary_plist_trailer() {
        let data = create_test_binary_plist();
        let reader = TestReader::new(data);
        let metadata = PlistParser::parse_binary_plist(&reader).unwrap();

        assert_eq!(
            metadata.get("Plist:OffsetIntSize"),
            Some(&TagValue::String("2".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:ObjectRefSize"),
            Some(&TagValue::String("1".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:NumObjects"),
            Some(&TagValue::String("5".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:TopObjectIndex"),
            Some(&TagValue::String("0".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:OffsetTableOffset"),
            Some(&TagValue::String("0x6C".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:TrailerSize"),
            Some(&TagValue::String("32".to_string()))
        );
    }

    #[test]
    fn test_parse_xml_plist_keys() {
        let data = create_test_xml_plist();
        let reader = TestReader::new(data);
        let metadata = PlistParser::parse_xml_plist(&reader).unwrap();

        assert_eq!(
            metadata.get("Plist:CFBundleIdentifier"),
            Some(&TagValue::String("com.example.testapp".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:CFBundleName"),
            Some(&TagValue::String("TestApp".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:CFBundleVersion"),
            Some(&TagValue::String("1.2.3".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:Label"),
            Some(&TagValue::String("com.example.service".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:RootObjectType"),
            Some(&TagValue::String("Dictionary".to_string()))
        );
    }

    #[test]
    fn test_extract_plist_string() {
        let text = "<key>TestKey</key><string>TestValue</string>";
        let value = PlistParser::extract_plist_string(text, "TestKey");
        assert_eq!(value, Some("TestValue".to_string()));
    }

    #[test]
    fn test_extract_plist_string_not_found() {
        let text = "<key>OtherKey</key><string>OtherValue</string>";
        let value = PlistParser::extract_plist_string(text, "TestKey");
        assert_eq!(value, None);
    }

    #[test]
    fn test_parse_full_binary_plist() {
        let data = create_test_binary_plist();
        let reader = TestReader::new(data);
        let parser = PlistParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("Plist".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:Format"),
            Some(&TagValue::String("Binary".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:FormatVersion"),
            Some(&TagValue::String("00".to_string()))
        );
        assert!(metadata.contains_key("Plist:NumObjects"));
    }

    #[test]
    fn test_parse_full_xml_plist() {
        let data = create_test_xml_plist();
        let reader = TestReader::new(data);
        let parser = PlistParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("Plist".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:Format"),
            Some(&TagValue::String("XML".to_string()))
        );
        assert_eq!(
            metadata.get("Plist:FormatVersion"),
            Some(&TagValue::String("1.0".to_string()))
        );
        assert!(metadata.contains_key("Plist:CFBundleIdentifier"));
    }

    #[test]
    fn test_parse_invalid_signature() {
        let data = vec![0u8; 100];
        let reader = TestReader::new(data);
        let parser = PlistParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_supports_format() {
        let parser = PlistParser;
        assert!(parser.supports_format(FileFormat::Plist));
        assert!(!parser.supports_format(FileFormat::SQLite));
        assert!(!parser.supports_format(FileFormat::Registry));
    }

    #[test]
    fn test_read_u64_be() {
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x2C];
        assert_eq!(PlistParser::read_u64_be(&bytes), 300);

        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(PlistParser::read_u64_be(&bytes), u64::MAX);
    }

    #[test]
    fn test_contains_subsequence() {
        let haystack = b"hello world test";
        assert!(PlistParser::contains_subsequence(haystack, b"world"));
        assert!(PlistParser::contains_subsequence(haystack, b"hello"));
        assert!(!PlistParser::contains_subsequence(haystack, b"goodbye"));
    }
}
