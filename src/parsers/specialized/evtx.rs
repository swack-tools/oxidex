//! Windows Event Log (EVTX) format parser for digital forensics
//!
//! Implements metadata extraction from Windows Event Log files (.evtx).
//! EVTX files are used by Windows Vista and later for system logging, making them
//! critical for security investigations, incident response, and digital forensics.
//!
//! # Format Structure
//!
//! EVTX files consist of:
//! - **File Header** (4096 bytes): Contains format identification and file-level metadata
//! - **Chunks** (65536 bytes each): Contain event records and chunk-level metadata
//! - **Event Records**: Individual log entries (structure varies by event type)
//!
//! # File Header Layout (128 bytes)
//!
//! - Signature: "ElfFile\0" (8 bytes at offset 0)
//! - First chunk number (offset 8, 8 bytes LE)
//! - Last chunk number (offset 16, 8 bytes LE)
//! - Next record identifier (offset 24, 8 bytes LE)
//! - Header size (offset 32, 4 bytes LE) - should be 128
//! - Minor version (offset 36, 2 bytes LE)
//! - Major version (offset 38, 2 bytes LE)
//! - Header block size (offset 40, 2 bytes LE) - should be 4096
//! - Chunk count (offset 42, 2 bytes LE)
//! - Flags (offset 76, 4 bytes LE)
//! - Checksum (offset 120, 4 bytes LE)
//!
//! # Chunk Header Layout (512 bytes)
//!
//! - Signature: "ElfChnk\0" (8 bytes at offset 0)
//! - First event record ID (offset 8, 8 bytes LE)
//! - Last event record ID (offset 16, 8 bytes LE)
//! - First event record timestamp (offset 24, 8 bytes FILETIME)
//! - Last event record timestamp (offset 32, 8 bytes FILETIME)
//!
//! # Forensic Value
//!
//! - **Event Timeline**: First/last event times provide temporal context
//! - **Dirty Flag**: Indicates improper shutdown or corruption, potential evidence tampering
//! - **Full Flag**: Log reached size limit and stopped recording (events may be missing)
//! - **Event Count**: Helps identify log rotation or clearing activities
//! - **Version Info**: Useful for determining Windows version and compatibility
//!
//! # References
//!
//! - EVTX Format Specification: https://github.com/libyal/libevtx/blob/main/documentation/Windows%20XML%20Event%20Log%20(EVTX).asciidoc
//! - Forensics Wiki: https://forensicswiki.xyz/wiki/index.php?title=Windows_XML_Event_Log_(EVTX)

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::io::timestamp::filetime_to_iso8601;

/// EVTX file header signature: "ElfFile\0" (8 bytes)
const EVTX_MAGIC: &[u8] = b"ElfFile\0";

/// EVTX chunk header signature: "ElfChnk\0" (8 bytes)
const EVTX_CHUNK_MAGIC: &[u8] = b"ElfChnk\0";

/// EVTX file header size (128 bytes)
const EVTX_HEADER_SIZE: usize = 128;

/// EVTX header block size (4096 bytes - first chunk)
const EVTX_HEADER_BLOCK_SIZE: usize = 4096;

/// EVTX chunk size (65536 bytes)
const EVTX_CHUNK_SIZE: usize = 65536;

/// EVTX chunk header size (512 bytes)
const EVTX_CHUNK_HEADER_SIZE: usize = 512;

/// Flag indicating log file was not properly closed (dirty)
const FLAG_DIRTY: u32 = 0x1;

/// Flag indicating log file reached size limit (full)
const FLAG_FULL: u32 = 0x2;

/// Windows Event Log (EVTX) parser for extracting forensic metadata
pub struct EVTXParser;

impl EVTXParser {
    /// Verifies EVTX signature by checking magic header
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the EVTX file
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Valid EVTX signature detected
    /// * `Ok(false)` - Invalid or missing signature
    /// * `Err` - I/O error reading the file
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        // Check file is large enough for header
        if reader.size() < EVTX_HEADER_BLOCK_SIZE as u64 {
            return Ok(false);
        }

        // Check magic header (bytes 0-7)
        let magic = reader.read(0, 8)?;
        Ok(magic == EVTX_MAGIC)
    }

    /// Reads first chunk number (offset 8, 8 bytes)
    fn read_first_chunk(reader: &dyn FileReader) -> Result<u64> {
        let data = reader.read(8, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u64_at(0).unwrap_or(0))
    }

    /// Reads last chunk number (offset 16, 8 bytes)
    fn read_last_chunk(reader: &dyn FileReader) -> Result<u64> {
        let data = reader.read(16, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u64_at(0).unwrap_or(0))
    }

    /// Reads next record identifier (offset 24, 8 bytes)
    fn read_next_record_id(reader: &dyn FileReader) -> Result<u64> {
        let data = reader.read(24, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u64_at(0).unwrap_or(0))
    }

    /// Reads header size (offset 32, 4 bytes) - should be 128
    fn read_header_size(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(32, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads minor version (offset 36, 2 bytes)
    fn read_minor_version(reader: &dyn FileReader) -> Result<u16> {
        let data = reader.read(36, 2)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u16_at(0).unwrap_or(0))
    }

    /// Reads major version (offset 38, 2 bytes)
    fn read_major_version(reader: &dyn FileReader) -> Result<u16> {
        let data = reader.read(38, 2)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u16_at(0).unwrap_or(0))
    }

    /// Reads header block size (offset 40, 2 bytes) - should be 4096
    fn read_header_block_size(reader: &dyn FileReader) -> Result<u16> {
        let data = reader.read(40, 2)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u16_at(0).unwrap_or(0))
    }

    /// Reads chunk count (offset 42, 2 bytes)
    fn read_chunk_count(reader: &dyn FileReader) -> Result<u16> {
        let data = reader.read(42, 2)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u16_at(0).unwrap_or(0))
    }

    /// Reads flags (offset 76, 4 bytes)
    fn read_flags(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(76, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads checksum (offset 120, 4 bytes)
    fn read_checksum(reader: &dyn FileReader) -> Result<u32> {
        let data = reader.read(120, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Verifies chunk signature by checking chunk magic header
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the EVTX file
    /// * `chunk_offset` - Offset to the start of the chunk
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Valid chunk signature detected
    /// * `Ok(false)` - Invalid or missing chunk signature
    /// * `Err` - I/O error reading the file
    fn verify_chunk_signature(reader: &dyn FileReader, chunk_offset: u64) -> Result<bool> {
        // Check if file is large enough for chunk
        if reader.size() < chunk_offset + EVTX_CHUNK_HEADER_SIZE as u64 {
            return Ok(false);
        }

        // Check chunk magic header
        let magic = reader.read(chunk_offset, 8)?;
        Ok(magic == EVTX_CHUNK_MAGIC)
    }

    /// Reads first event record ID from chunk (offset 8, 8 bytes)
    fn read_chunk_first_record_id(reader: &dyn FileReader, chunk_offset: u64) -> Result<u64> {
        let data = reader.read(chunk_offset + 8, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u64_at(0).unwrap_or(0))
    }

    /// Reads last event record ID from chunk (offset 16, 8 bytes)
    fn read_chunk_last_record_id(reader: &dyn FileReader, chunk_offset: u64) -> Result<u64> {
        let data = reader.read(chunk_offset + 16, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u64_at(0).unwrap_or(0))
    }

    /// Reads first event record timestamp from chunk (offset 24, 8 bytes FILETIME)
    fn read_chunk_first_timestamp(reader: &dyn FileReader, chunk_offset: u64) -> Result<u64> {
        let data = reader.read(chunk_offset + 24, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u64_at(0).unwrap_or(0))
    }

    /// Reads last event record timestamp from chunk (offset 32, 8 bytes FILETIME)
    fn read_chunk_last_timestamp(reader: &dyn FileReader, chunk_offset: u64) -> Result<u64> {
        let data = reader.read(chunk_offset + 32, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u64_at(0).unwrap_or(0))
    }

    /// Converts Windows FILETIME to ISO 8601 string using shared utility
    ///
    /// FILETIME is a 64-bit value representing 100-nanosecond intervals since
    /// January 1, 1601 UTC.
    ///
    /// # Arguments
    ///
    /// * `filetime` - Windows FILETIME value
    ///
    /// # Returns
    ///
    /// ISO 8601 formatted timestamp string (e.g., "2024-01-15T10:30:00Z")
    fn convert_filetime(filetime: u64) -> String {
        filetime_to_iso8601(filetime).unwrap_or_else(|| "Invalid".to_string())
    }

    /// Formats version as Major.Minor string
    fn format_version(major: u16, minor: u16) -> String {
        format!("{}.{}", major, minor)
    }

    /// Checks if dirty flag is set
    fn is_dirty(flags: u32) -> bool {
        (flags & FLAG_DIRTY) != 0
    }

    /// Checks if full flag is set
    fn is_full(flags: u32) -> bool {
        (flags & FLAG_FULL) != 0
    }

    /// Formats flags as hex string with descriptive text
    fn format_flags(flags: u32) -> String {
        let mut descriptions = Vec::new();

        if Self::is_dirty(flags) {
            descriptions.push("Dirty");
        }
        if Self::is_full(flags) {
            descriptions.push("Full");
        }

        if descriptions.is_empty() {
            format!("0x{:08X}", flags)
        } else {
            format!("0x{:08X} ({})", flags, descriptions.join(", "))
        }
    }
}

impl FormatParser for EVTXParser {
    /// Parses metadata from a Windows Event Log file
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the EVTX file
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted metadata including forensic indicators
    /// * `Err(ExifToolError)` - Invalid signature or parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify this is a valid EVTX file
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid EVTX signature"));
        }

        let mut metadata = MetadataMap::new();

        // Basic file information
        metadata.insert(
            "FileType".to_string(),
            TagValue::String("Windows Event Log".to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Version information
        let major_version = Self::read_major_version(reader)?;
        let minor_version = Self::read_minor_version(reader)?;
        metadata.insert(
            "EVTX:Version".to_string(),
            TagValue::String(Self::format_version(major_version, minor_version)),
        );
        metadata.insert(
            "EVTX:MajorVersion".to_string(),
            TagValue::String(major_version.to_string()),
        );
        metadata.insert(
            "EVTX:MinorVersion".to_string(),
            TagValue::String(minor_version.to_string()),
        );

        // Header information
        let header_size = Self::read_header_size(reader)?;
        metadata.insert(
            "EVTX:HeaderSize".to_string(),
            TagValue::String(format!("{} bytes", header_size)),
        );

        let header_block_size = Self::read_header_block_size(reader)?;
        metadata.insert(
            "EVTX:HeaderBlockSize".to_string(),
            TagValue::String(format!("{} bytes", header_block_size)),
        );

        // Chunk information
        let first_chunk = Self::read_first_chunk(reader)?;
        metadata.insert(
            "EVTX:FirstChunk".to_string(),
            TagValue::String(first_chunk.to_string()),
        );

        let last_chunk = Self::read_last_chunk(reader)?;
        metadata.insert(
            "EVTX:LastChunk".to_string(),
            TagValue::String(last_chunk.to_string()),
        );

        let chunk_count = Self::read_chunk_count(reader)?;
        metadata.insert(
            "EVTX:ChunkCount".to_string(),
            TagValue::String(chunk_count.to_string()),
        );

        // Record information
        let next_record_id = Self::read_next_record_id(reader)?;
        metadata.insert(
            "EVTX:NextRecordID".to_string(),
            TagValue::String(next_record_id.to_string()),
        );

        // Calculate estimated event count from first chunk
        let first_chunk_offset = EVTX_HEADER_BLOCK_SIZE as u64;
        if Self::verify_chunk_signature(reader, first_chunk_offset)? {
            let first_record_id = Self::read_chunk_first_record_id(reader, first_chunk_offset)?;
            let last_record_id = Self::read_chunk_last_record_id(reader, first_chunk_offset)?;

            // Estimate event count
            if next_record_id > 0 {
                let event_count = next_record_id - 1; // Record IDs start at 1
                metadata.insert(
                    "EVTX:EventCount".to_string(),
                    TagValue::String(format!("{} (estimated)", event_count)),
                );
            }

            // First and last event times from first chunk
            let first_timestamp = Self::read_chunk_first_timestamp(reader, first_chunk_offset)?;
            if first_timestamp > 0 {
                metadata.insert(
                    "EVTX:FirstEventTime".to_string(),
                    TagValue::String(Self::convert_filetime(first_timestamp)),
                );
            }

            let last_timestamp = Self::read_chunk_last_timestamp(reader, first_chunk_offset)?;
            if last_timestamp > 0 {
                metadata.insert(
                    "EVTX:LastEventTime".to_string(),
                    TagValue::String(Self::convert_filetime(last_timestamp)),
                );
            }

            // Record range in first chunk
            if first_record_id > 0 && last_record_id > 0 {
                metadata.insert(
                    "EVTX:FirstChunkRecordRange".to_string(),
                    TagValue::String(format!("{} - {}", first_record_id, last_record_id)),
                );
            }
        }

        // Flags and forensic indicators
        let flags = Self::read_flags(reader)?;
        metadata.insert(
            "EVTX:Flags".to_string(),
            TagValue::String(Self::format_flags(flags)),
        );

        let is_dirty = Self::is_dirty(flags);
        metadata.insert(
            "EVTX:IsDirty".to_string(),
            TagValue::String(is_dirty.to_string()),
        );

        if is_dirty {
            metadata.insert(
                "EVTX:ForensicNote".to_string(),
                TagValue::String(
                    "Log file was not properly closed - possible system crash or improper shutdown"
                        .to_string(),
                ),
            );
        }

        let is_full = Self::is_full(flags);
        metadata.insert(
            "EVTX:IsFull".to_string(),
            TagValue::String(is_full.to_string()),
        );

        if is_full {
            metadata.insert(
                "EVTX:ForensicWarning".to_string(),
                TagValue::String(
                    "Log file reached size limit and stopped recording - events may be missing"
                        .to_string(),
                ),
            );
        }

        // Checksum
        let checksum = Self::read_checksum(reader)?;
        metadata.insert(
            "EVTX:Checksum".to_string(),
            TagValue::String(format!("0x{:08X}", checksum)),
        );

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
    /// * `true` - Parser supports EVTX format
    /// * `false` - Parser does not support the format
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::EVTX)
    }
}

/// Parses metadata from Windows Event Log files.
///
/// This is the public API function for parsing EVTX files.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the EVTX file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::specialized::evtx::parse_evtx_metadata;
/// use oxidex::io::MMapReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), String> {
/// let reader = MMapReader::new(Path::new("System.evtx"))
///     .map_err(|e| e.to_string())?;
/// let metadata = parse_evtx_metadata(&reader)?;
/// println!("EVTX metadata: {:?}", metadata);
/// # Ok(())
/// # }
/// ```
pub fn parse_evtx_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = EVTXParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Creates a minimal valid EVTX header and first chunk for testing
    fn create_test_evtx() -> Vec<u8> {
        let mut data = vec![0u8; EVTX_HEADER_BLOCK_SIZE + EVTX_CHUNK_SIZE];

        // File header (128 bytes)
        // Magic header "ElfFile\0"
        data[0..8].copy_from_slice(b"ElfFile\0");

        // First chunk number (offset 8): 0
        data[8..16].copy_from_slice(&0u64.to_le_bytes());

        // Last chunk number (offset 16): 0
        data[16..24].copy_from_slice(&0u64.to_le_bytes());

        // Next record identifier (offset 24): 1001
        data[24..32].copy_from_slice(&1001u64.to_le_bytes());

        // Header size (offset 32): 128
        data[32..36].copy_from_slice(&128u32.to_le_bytes());

        // Minor version (offset 36): 1
        data[36..38].copy_from_slice(&1u16.to_le_bytes());

        // Major version (offset 38): 3
        data[38..40].copy_from_slice(&3u16.to_le_bytes());

        // Header block size (offset 40): 4096
        data[40..42].copy_from_slice(&4096u16.to_le_bytes());

        // Chunk count (offset 42): 1
        data[42..44].copy_from_slice(&1u16.to_le_bytes());

        // Flags (offset 76): 0x1 (Dirty)
        data[76..80].copy_from_slice(&1u32.to_le_bytes());

        // Checksum (offset 120): 0xDEADBEEF
        data[120..124].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

        // First chunk header (at offset 4096)
        let chunk_offset = EVTX_HEADER_BLOCK_SIZE;

        // Chunk magic "ElfChnk\0"
        data[chunk_offset..chunk_offset + 8].copy_from_slice(b"ElfChnk\0");

        // First event record ID (offset 8): 1
        data[chunk_offset + 8..chunk_offset + 16].copy_from_slice(&1u64.to_le_bytes());

        // Last event record ID (offset 16): 1000
        data[chunk_offset + 16..chunk_offset + 24].copy_from_slice(&1000u64.to_le_bytes());

        // First event timestamp (offset 24): 133000000000000000 (example FILETIME)
        data[chunk_offset + 24..chunk_offset + 32]
            .copy_from_slice(&133000000000000000u64.to_le_bytes());

        // Last event timestamp (offset 32): 133000001000000000
        data[chunk_offset + 32..chunk_offset + 40]
            .copy_from_slice(&133000001000000000u64.to_le_bytes());

        data
    }

    #[test]
    fn test_verify_signature_valid() {
        let data = create_test_evtx();
        let reader = TestReader::new(data);
        assert!(EVTXParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_invalid_magic() {
        let mut data = vec![0u8; EVTX_HEADER_BLOCK_SIZE];
        data[0..8].copy_from_slice(b"Invalid\0");
        let reader = TestReader::new(data);
        assert!(!EVTXParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_too_small() {
        let data = vec![0u8; 100]; // Less than 4096 bytes
        let reader = TestReader::new(data);
        assert!(!EVTXParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_read_header_fields() {
        let data = create_test_evtx();
        let reader = TestReader::new(data);

        assert_eq!(EVTXParser::read_first_chunk(&reader).unwrap(), 0);
        assert_eq!(EVTXParser::read_last_chunk(&reader).unwrap(), 0);
        assert_eq!(EVTXParser::read_next_record_id(&reader).unwrap(), 1001);
        assert_eq!(EVTXParser::read_header_size(&reader).unwrap(), 128);
        assert_eq!(EVTXParser::read_minor_version(&reader).unwrap(), 1);
        assert_eq!(EVTXParser::read_major_version(&reader).unwrap(), 3);
        assert_eq!(EVTXParser::read_header_block_size(&reader).unwrap(), 4096);
        assert_eq!(EVTXParser::read_chunk_count(&reader).unwrap(), 1);
        assert_eq!(EVTXParser::read_flags(&reader).unwrap(), 1);
        assert_eq!(EVTXParser::read_checksum(&reader).unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn test_verify_chunk_signature() {
        let data = create_test_evtx();
        let reader = TestReader::new(data);
        assert!(
            EVTXParser::verify_chunk_signature(&reader, EVTX_HEADER_BLOCK_SIZE as u64).unwrap()
        );
    }

    #[test]
    fn test_read_chunk_fields() {
        let data = create_test_evtx();
        let reader = TestReader::new(data);
        let chunk_offset = EVTX_HEADER_BLOCK_SIZE as u64;

        assert_eq!(
            EVTXParser::read_chunk_first_record_id(&reader, chunk_offset).unwrap(),
            1
        );
        assert_eq!(
            EVTXParser::read_chunk_last_record_id(&reader, chunk_offset).unwrap(),
            1000
        );
        assert_eq!(
            EVTXParser::read_chunk_first_timestamp(&reader, chunk_offset).unwrap(),
            133000000000000000
        );
        assert_eq!(
            EVTXParser::read_chunk_last_timestamp(&reader, chunk_offset).unwrap(),
            133000001000000000
        );
    }

    #[test]
    fn test_format_version() {
        assert_eq!(EVTXParser::format_version(3, 1), "3.1");
        assert_eq!(EVTXParser::format_version(2, 0), "2.0");
    }

    #[test]
    fn test_is_dirty() {
        assert!(EVTXParser::is_dirty(0x1));
        assert!(EVTXParser::is_dirty(0x3)); // Both flags
        assert!(!EVTXParser::is_dirty(0x2));
        assert!(!EVTXParser::is_dirty(0x0));
    }

    #[test]
    fn test_is_full() {
        assert!(EVTXParser::is_full(0x2));
        assert!(EVTXParser::is_full(0x3)); // Both flags
        assert!(!EVTXParser::is_full(0x1));
        assert!(!EVTXParser::is_full(0x0));
    }

    #[test]
    fn test_format_flags() {
        assert_eq!(EVTXParser::format_flags(0x0), "0x00000000");
        assert_eq!(EVTXParser::format_flags(0x1), "0x00000001 (Dirty)");
        assert_eq!(EVTXParser::format_flags(0x2), "0x00000002 (Full)");
        assert_eq!(EVTXParser::format_flags(0x3), "0x00000003 (Dirty, Full)");
    }

    #[test]
    fn test_convert_filetime() {
        // Test with a known timestamp
        // 133000000000000000 = approximately 2023-05-01
        let result = EVTXParser::convert_filetime(133000000000000000);
        assert!(result.contains("T"));
        assert!(result.contains("Z"));
        assert!(!result.contains("Invalid"));
    }

    #[test]
    fn test_convert_filetime_invalid() {
        // Test with zero (before Unix epoch)
        let result = EVTXParser::convert_filetime(0);
        // Should return "Invalid" for timestamps before Unix epoch
        assert_eq!(result, "Invalid");
    }

    #[test]
    fn test_parse_valid_evtx() {
        let data = create_test_evtx();
        let reader = TestReader::new(data);
        let parser = EVTXParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("Windows Event Log".to_string()))
        );
        assert_eq!(
            metadata.get("EVTX:Version"),
            Some(&TagValue::String("3.1".to_string()))
        );
        assert_eq!(
            metadata.get("EVTX:ChunkCount"),
            Some(&TagValue::String("1".to_string()))
        );
        assert_eq!(
            metadata.get("EVTX:NextRecordID"),
            Some(&TagValue::String("1001".to_string()))
        );
        assert_eq!(
            metadata.get("EVTX:IsDirty"),
            Some(&TagValue::String("true".to_string()))
        );
        assert_eq!(
            metadata.get("EVTX:IsFull"),
            Some(&TagValue::String("false".to_string()))
        );
        assert_eq!(
            metadata.get("EVTX:Checksum"),
            Some(&TagValue::String("0xDEADBEEF".to_string()))
        );

        // Should have forensic note due to dirty flag
        assert!(metadata.contains_key("EVTX:ForensicNote"));

        // Should have event count estimate
        assert!(metadata.contains_key("EVTX:EventCount"));
    }

    #[test]
    fn test_parse_invalid_signature() {
        let data = vec![0u8; EVTX_HEADER_BLOCK_SIZE];
        let reader = TestReader::new(data);
        let parser = EVTXParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_supports_format() {
        let parser = EVTXParser;
        assert!(parser.supports_format(FileFormat::EVTX));
        assert!(!parser.supports_format(FileFormat::SQLite));
        assert!(!parser.supports_format(FileFormat::PDF));
    }
}
