//! Windows Registry Hive parser for digital forensics
//!
//! Implements metadata extraction from Windows Registry Hive files (NTUSER.DAT, SYSTEM,
//! SOFTWARE, SAM, SECURITY). Registry hives are critical for Windows forensics,
//! containing system configuration, user settings, and application data.
//!
//! # Format Structure
//!
//! Registry hives begin with a 4096-byte header containing:
//! - File identification (regf signature, version information)
//! - Hive statistics (root cell offset, data size)
//! - Timestamps (last written time, sequence numbers)
//! - Hive metadata (type, embedded filename)
//!
//! # Forensic Value
//!
//! - **Last Written Time**: Tracks when the hive was last modified
//! - **Sequence Numbers**: Indicates clean vs. dirty shutdown (primary == secondary)
//! - **Hive Type**: Distinguishes normal hives from transaction logs
//! - **Hive Name**: Embedded filename helps identify the hive purpose
//! - **Data Size**: Total size of hive bins data
//!
//! # References
//!
//! - Windows Registry File Format: https://github.com/msuhanov/regf/blob/master/Windows%20registry%20file%20format%20specification.md
//! - Registry Forensics: https://forensicswiki.xyz/wiki/index.php?title=Windows_Registry

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::io::timestamp::filetime_to_iso8601;

/// Registry hive signature: "regf" (4 bytes)
const REGF_MAGIC: &[u8] = b"regf";

/// Registry hive header size (4096 bytes)
const REGF_HEADER_SIZE: usize = 4096;

/// Hive type values
const HIVE_TYPE_NORMAL: u32 = 0;
const HIVE_TYPE_TRANSACTION_LOG: u32 = 1;

/// Windows Registry Hive parser for extracting forensic metadata
pub struct RegistryParser;

impl RegistryParser {
    /// Verifies registry hive signature by checking magic header
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the registry hive file
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Valid registry hive signature detected
    /// * `Ok(false)` - Invalid or missing signature
    /// * `Err` - I/O error reading the file
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        // Check file is large enough for header
        if reader.size() < REGF_HEADER_SIZE as u64 {
            return Ok(false);
        }

        // Check magic header (bytes 0-3)
        let magic = reader.read(0, 4)?;
        Ok(magic == REGF_MAGIC)
    }

    /// Reads primary sequence number (offset 4, 4 bytes)
    ///
    /// Incremented at the beginning of a write operation.
    fn read_primary_sequence(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(4, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads secondary sequence number (offset 8, 4 bytes)
    ///
    /// Incremented at the end of a write operation. Equals primary when clean shutdown.
    fn read_secondary_sequence(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(8, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads last written timestamp (offset 12, 8 bytes)
    ///
    /// Windows FILETIME format: 100-nanosecond intervals since 1601-01-01.
    fn read_last_written(reader: &dyn FileReader) -> Result<u64> {
        let data = reader.read(12, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u64_at(0).unwrap_or(0))
    }

    /// Reads major version (offset 20, 4 bytes)
    fn read_major_version(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(20, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads minor version (offset 24, 4 bytes)
    fn read_minor_version(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(24, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads hive type (offset 28, 4 bytes)
    ///
    /// 0 = normal hive, 1 = transaction log
    fn read_hive_type(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(28, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads root cell offset (offset 36, 4 bytes)
    ///
    /// Offset to the root key cell in the hive bins data.
    fn read_root_cell_offset(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(36, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads hive bins data size (offset 40, 4 bytes)
    ///
    /// Total size of the hive bins data section.
    fn read_data_size(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(40, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads embedded hive name (offset 48, 64 bytes)
    ///
    /// UTF-16LE encoded filename, may be partially filled or empty.
    fn read_hive_name(reader: &dyn FileReader) -> Result<String> {
        let bytes = reader.read(48, 64)?;

        // Convert UTF-16LE to String using EndianReader for each u16 character
        let le_reader = EndianReader::little_endian(bytes);
        let mut utf16_chars = Vec::new();
        for i in 0..32 {
            // 64 bytes = 32 UTF-16 characters
            if let Some(char_code) = le_reader.u16_at(i * 2) {
                if char_code == 0 {
                    break; // Null terminator
                }
                utf16_chars.push(char_code);
            } else {
                break;
            }
        }

        // Decode UTF-16LE to String
        Ok(String::from_utf16(&utf16_chars)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| String::new()))
    }

    /// Converts Windows FILETIME to ISO 8601 timestamp using shared utility
    ///
    /// # Arguments
    ///
    /// * `filetime` - Windows FILETIME value (100-nanosecond intervals since 1601-01-01)
    ///
    /// # Returns
    ///
    /// ISO 8601 formatted timestamp string
    fn convert_filetime(filetime: u64) -> String {
        if filetime == 0 {
            return "1601-01-01T00:00:00Z".to_string();
        }
        filetime_to_iso8601(filetime).unwrap_or_else(|| format!("Invalid (FILETIME: {})", filetime))
    }

    /// Decodes hive type value to human-readable string
    fn decode_hive_type(hive_type: u32) -> &'static str {
        match hive_type {
            HIVE_TYPE_NORMAL => "Normal",
            HIVE_TYPE_TRANSACTION_LOG => "Transaction Log",
            _ => "Unknown",
        }
    }

    /// Checks if sequence numbers indicate clean shutdown
    ///
    /// Primary and secondary sequence numbers should match after clean shutdown.
    fn is_clean_shutdown(primary: u32, secondary: u32) -> bool {
        primary == secondary
    }

    /// Infers hive purpose from filename
    ///
    /// # Arguments
    ///
    /// * `filename` - Embedded hive filename
    ///
    /// # Returns
    ///
    /// Optional description of the hive's purpose
    fn infer_hive_purpose(filename: &str) -> Option<&'static str> {
        let filename_upper = filename.to_uppercase();
        if filename_upper.contains("NTUSER") {
            Some("User profile settings and preferences")
        } else if filename_upper.contains("SYSTEM") {
            Some("System-wide configuration and hardware settings")
        } else if filename_upper.contains("SOFTWARE") {
            Some("Installed software configuration")
        } else if filename_upper.contains("SAM") {
            Some("Security Accounts Manager - user accounts and passwords")
        } else if filename_upper.contains("SECURITY") {
            Some("Security policy and user rights")
        } else if filename_upper.contains("DEFAULT") {
            Some("Default user profile settings")
        } else if filename_upper.contains("USERDIFF") {
            Some("User profile differences")
        } else {
            None
        }
    }
}

impl FormatParser for RegistryParser {
    /// Parses metadata from a Windows Registry Hive file
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the registry hive file
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted metadata including forensic indicators
    /// * `Err(ExifToolError)` - Invalid signature or parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify this is a valid registry hive file
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error(
                "Invalid registry hive signature",
            ));
        }

        let mut metadata = MetadataMap::new();

        // Basic file information
        metadata.insert(
            "FileType".to_string(),
            TagValue::String("Registry Hive".to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );
        metadata.insert(
            "Registry:Signature".to_string(),
            TagValue::String("regf".to_string()),
        );

        // Version information
        let major_version = Self::read_major_version(reader)?;
        let minor_version = Self::read_minor_version(reader)?;
        metadata.insert(
            "Registry:Version".to_string(),
            TagValue::String(format!("{}.{}", major_version, minor_version)),
        );
        metadata.insert(
            "Registry:MajorVersion".to_string(),
            TagValue::String(major_version.to_string()),
        );
        metadata.insert(
            "Registry:MinorVersion".to_string(),
            TagValue::String(minor_version.to_string()),
        );

        // Timestamps and sequence numbers
        let last_written = Self::read_last_written(reader)?;
        let last_written_iso = Self::convert_filetime(last_written);
        metadata.insert(
            "Registry:LastWritten".to_string(),
            TagValue::String(last_written_iso),
        );
        metadata.insert(
            "Registry:LastWrittenRaw".to_string(),
            TagValue::String(format!("{}", last_written)),
        );

        let primary_seq = Self::read_primary_sequence(reader)?;
        let secondary_seq = Self::read_secondary_sequence(reader)?;
        metadata.insert(
            "Registry:PrimarySequence".to_string(),
            TagValue::String(primary_seq.to_string()),
        );
        metadata.insert(
            "Registry:SecondarySequence".to_string(),
            TagValue::String(secondary_seq.to_string()),
        );

        // Sequence validation
        let is_clean = Self::is_clean_shutdown(primary_seq, secondary_seq);
        metadata.insert(
            "Registry:SequenceValid".to_string(),
            TagValue::String(if is_clean { "Yes" } else { "No" }.to_string()),
        );

        if !is_clean {
            metadata.insert(
                "ForensicNote".to_string(),
                TagValue::String(
                    "Sequence numbers mismatch - possible dirty shutdown or corruption".to_string(),
                ),
            );
        }

        // Hive type
        let hive_type = Self::read_hive_type(reader)?;
        metadata.insert(
            "Registry:HiveType".to_string(),
            TagValue::String(Self::decode_hive_type(hive_type).to_string()),
        );
        metadata.insert(
            "Registry:HiveTypeRaw".to_string(),
            TagValue::String(hive_type.to_string()),
        );

        // Hive structure
        let root_cell_offset = Self::read_root_cell_offset(reader)?;
        metadata.insert(
            "Registry:RootCellOffset".to_string(),
            TagValue::String(format!("0x{:08X}", root_cell_offset)),
        );

        let data_size = Self::read_data_size(reader)?;
        metadata.insert(
            "Registry:DataSize".to_string(),
            TagValue::String(format!("{} bytes", data_size)),
        );
        metadata.insert(
            "Registry:DataSizeRaw".to_string(),
            TagValue::String(data_size.to_string()),
        );

        // Hive name and purpose
        let hive_name = Self::read_hive_name(reader)?;
        if !hive_name.is_empty() {
            metadata.insert(
                "Registry:HiveName".to_string(),
                TagValue::String(hive_name.clone()),
            );

            if let Some(purpose) = Self::infer_hive_purpose(&hive_name) {
                metadata.insert(
                    "Registry:HivePurpose".to_string(),
                    TagValue::String(purpose.to_string()),
                );
            }
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
    /// * `true` - Parser supports Registry format
    /// * `false` - Parser does not support the format
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::Registry)
    }
}

/// Parses metadata from Windows Registry Hive files.
///
/// This is the public API function for parsing registry hives.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the registry hive file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::specialized::registry::parse_registry_metadata;
/// use oxidex::io::MMapReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), String> {
/// let reader = MMapReader::new(Path::new("NTUSER.DAT"))
///     .map_err(|e| e.to_string())?;
/// let metadata = parse_registry_metadata(&reader)?;
/// println!("Registry metadata: {:?}", metadata);
/// # Ok(())
/// # }
/// ```
pub fn parse_registry_metadata(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = RegistryParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Creates a minimal valid registry hive header for testing
    fn create_test_header() -> Vec<u8> {
        let mut data = vec![0u8; REGF_HEADER_SIZE];

        // Magic header "regf"
        data[0..4].copy_from_slice(b"regf");

        // Primary sequence number (offset 4): 100
        data[4..8].copy_from_slice(&100u32.to_le_bytes());

        // Secondary sequence number (offset 8): 100 (clean shutdown)
        data[8..12].copy_from_slice(&100u32.to_le_bytes());

        // Last written timestamp (offset 12): 133000000000000000 (example FILETIME)
        data[12..20].copy_from_slice(&133000000000000000u64.to_le_bytes());

        // Major version (offset 20): 1
        data[20..24].copy_from_slice(&1u32.to_le_bytes());

        // Minor version (offset 24): 5
        data[24..28].copy_from_slice(&5u32.to_le_bytes());

        // Hive type (offset 28): 0 (normal)
        data[28..32].copy_from_slice(&0u32.to_le_bytes());

        // Root cell offset (offset 36): 0x1000
        data[36..40].copy_from_slice(&0x1000u32.to_le_bytes());

        // Data size (offset 40): 1048576 bytes (1MB)
        data[40..44].copy_from_slice(&1048576u32.to_le_bytes());

        // Hive name (offset 48): "NTUSER.DAT" in UTF-16LE
        let hive_name = "NTUSER.DAT";
        for (i, c) in hive_name.encode_utf16().enumerate() {
            if i * 2 + 1 < 64 {
                data[48 + i * 2..48 + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
            }
        }

        data
    }

    #[test]
    fn test_verify_signature_valid() {
        let data = create_test_header();
        let reader = TestReader::new(data);
        assert!(RegistryParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_invalid_magic() {
        let mut data = vec![0u8; REGF_HEADER_SIZE];
        data[0..4].copy_from_slice(b"invl");
        let reader = TestReader::new(data);
        assert!(!RegistryParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_too_small() {
        let data = vec![0u8; 100]; // Less than 4096 bytes
        let reader = TestReader::new(data);
        assert!(!RegistryParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_read_header_fields() {
        let data = create_test_header();
        let reader = TestReader::new(data);

        assert_eq!(RegistryParser::read_primary_sequence(&reader).unwrap(), 100);
        assert_eq!(
            RegistryParser::read_secondary_sequence(&reader).unwrap(),
            100
        );
        assert_eq!(
            RegistryParser::read_last_written(&reader).unwrap(),
            133000000000000000
        );
        assert_eq!(RegistryParser::read_major_version(&reader).unwrap(), 1);
        assert_eq!(RegistryParser::read_minor_version(&reader).unwrap(), 5);
        assert_eq!(RegistryParser::read_hive_type(&reader).unwrap(), 0);
        assert_eq!(
            RegistryParser::read_root_cell_offset(&reader).unwrap(),
            0x1000
        );
        assert_eq!(RegistryParser::read_data_size(&reader).unwrap(), 1048576);
    }

    #[test]
    fn test_read_hive_name() {
        let data = create_test_header();
        let reader = TestReader::new(data);
        let hive_name = RegistryParser::read_hive_name(&reader).unwrap();
        assert_eq!(hive_name, "NTUSER.DAT");
    }

    #[test]
    fn test_decode_hive_type() {
        assert_eq!(RegistryParser::decode_hive_type(0), "Normal");
        assert_eq!(RegistryParser::decode_hive_type(1), "Transaction Log");
        assert_eq!(RegistryParser::decode_hive_type(99), "Unknown");
    }

    #[test]
    fn test_is_clean_shutdown() {
        assert!(RegistryParser::is_clean_shutdown(100, 100));
        assert!(!RegistryParser::is_clean_shutdown(100, 99));
    }

    #[test]
    fn test_convert_filetime() {
        // Test zero FILETIME
        assert_eq!(RegistryParser::convert_filetime(0), "1601-01-01T00:00:00Z");

        // Test a known FILETIME value
        // 130000000000000000 corresponds to approximately 2013
        let timestamp = RegistryParser::convert_filetime(130000000000000000);
        assert!(timestamp.starts_with("201"));
    }

    #[test]
    fn test_infer_hive_purpose() {
        assert!(RegistryParser::infer_hive_purpose("NTUSER.DAT").is_some());
        assert!(RegistryParser::infer_hive_purpose("SYSTEM").is_some());
        assert!(RegistryParser::infer_hive_purpose("SOFTWARE").is_some());
        assert!(RegistryParser::infer_hive_purpose("SAM").is_some());
        assert!(RegistryParser::infer_hive_purpose("SECURITY").is_some());
        assert!(RegistryParser::infer_hive_purpose("DEFAULT").is_some());
        assert!(RegistryParser::infer_hive_purpose("unknown.dat").is_none());
    }

    #[test]
    fn test_parse_valid_registry() {
        let data = create_test_header();
        let reader = TestReader::new(data);
        let parser = RegistryParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("Registry Hive".to_string()))
        );
        assert_eq!(
            metadata.get("Registry:Signature"),
            Some(&TagValue::String("regf".to_string()))
        );
        assert_eq!(
            metadata.get("Registry:Version"),
            Some(&TagValue::String("1.5".to_string()))
        );
        assert_eq!(
            metadata.get("Registry:PrimarySequence"),
            Some(&TagValue::String("100".to_string()))
        );
        assert_eq!(
            metadata.get("Registry:SecondarySequence"),
            Some(&TagValue::String("100".to_string()))
        );
        assert_eq!(
            metadata.get("Registry:SequenceValid"),
            Some(&TagValue::String("Yes".to_string()))
        );
        assert_eq!(
            metadata.get("Registry:HiveType"),
            Some(&TagValue::String("Normal".to_string()))
        );
        assert_eq!(
            metadata.get("Registry:RootCellOffset"),
            Some(&TagValue::String("0x00001000".to_string()))
        );
        assert_eq!(
            metadata.get("Registry:HiveName"),
            Some(&TagValue::String("NTUSER.DAT".to_string()))
        );
        assert!(metadata.contains_key("Registry:HivePurpose"));

        // Should NOT have forensic note for clean shutdown
        assert!(!metadata.contains_key("ForensicNote"));
    }

    #[test]
    fn test_parse_dirty_shutdown() {
        let mut data = create_test_header();
        // Set different sequence numbers (dirty shutdown)
        data[8..12].copy_from_slice(&99u32.to_le_bytes()); // Secondary = 99, Primary = 100

        let reader = TestReader::new(data);
        let parser = RegistryParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("Registry:SequenceValid"),
            Some(&TagValue::String("No".to_string()))
        );
        assert!(metadata.contains_key("ForensicNote"));
    }

    #[test]
    fn test_parse_invalid_signature() {
        let data = vec![0u8; REGF_HEADER_SIZE];
        let reader = TestReader::new(data);
        let parser = RegistryParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_supports_format() {
        let parser = RegistryParser;
        assert!(parser.supports_format(FileFormat::Registry));
        assert!(!parser.supports_format(FileFormat::SQLite));
        assert!(!parser.supports_format(FileFormat::JPEG));
    }
}
