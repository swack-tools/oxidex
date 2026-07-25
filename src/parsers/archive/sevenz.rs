//! 7z archive format parser
//!
//! Implements comprehensive metadata extraction from 7-Zip archive files.
//! Parses the 32-byte start header to extract version, offsets, sizes, and CRCs.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// 7z signature: 0x37 0x7A 0xBC 0xAF 0x27 0x1C
const SEVENZ_SIGNATURE: &[u8] = &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
const START_HEADER_SIZE: usize = 32;

/// 7z start header structure
#[derive(Debug)]
struct StartHeader {
    major_version: u8,
    minor_version: u8,
    start_header_crc: u32,
    next_header_offset: u64,
    next_header_size: u64,
    next_header_crc: u32,
}

impl StartHeader {
    /// Parse start header from reader
    fn parse(reader: &dyn FileReader) -> Result<Self> {
        if reader.size() < START_HEADER_SIZE as u64 {
            return Err(ExifToolError::parse_error("File too small for 7z header"));
        }

        let header = reader.read(0, START_HEADER_SIZE)?;
        let r = EndianReader::little_endian(header);

        Ok(Self {
            major_version: header[6],
            minor_version: header[7],
            start_header_crc: r.u32_at(8).unwrap_or(0),
            next_header_offset: r.u64_at(12).unwrap_or(0),
            next_header_size: r.u64_at(20).unwrap_or(0),
            next_header_crc: r.u32_at(28).unwrap_or(0),
        })
    }

    /// Calculate CRC32 for header validation (bytes 12-31)
    fn calculate_header_crc(data: &[u8]) -> u32 {
        let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        crc.checksum(&data[12..32])
    }
}

/// 7z parser for extracting metadata from 7-Zip archives
pub struct SevenZParser;

impl SevenZParser {
    /// Verifies 7z signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 6 {
            return Ok(false);
        }

        let header = reader.read(0, 6)?;
        Ok(header == SEVENZ_SIGNATURE)
    }
}

impl FormatParser for SevenZParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify signature
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid 7z signature"));
        }

        let mut metadata = MetadataMap::new();
        let file_size = reader.size();

        // Parse start header
        let start_header = StartHeader::parse(reader)?;

        // Basic metadata
        metadata.insert("FileType".to_string(), TagValue::String("7z".to_string()));
        metadata.insert(
            "7zVersion".to_string(),
            TagValue::String(format!(
                "{}.{}",
                start_header.major_version, start_header.minor_version
            )),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(file_size.to_string()),
        );

        // Start header metadata
        metadata.insert(
            "StartHeaderCRC".to_string(),
            TagValue::String(format!("0x{:08X}", start_header.start_header_crc)),
        );

        // Next header (encoded header) information
        metadata.insert(
            "NextHeaderOffset".to_string(),
            TagValue::String(start_header.next_header_offset.to_string()),
        );
        metadata.insert(
            "NextHeaderSize".to_string(),
            TagValue::String(start_header.next_header_size.to_string()),
        );
        metadata.insert(
            "NextHeaderCRC".to_string(),
            TagValue::String(format!("0x{:08X}", start_header.next_header_crc)),
        );

        // Calculate derived metrics
        let data_offset = START_HEADER_SIZE as u64 + start_header.next_header_offset;
        metadata.insert(
            "DataOffset".to_string(),
            TagValue::String(data_offset.to_string()),
        );

        let header_size = START_HEADER_SIZE as u64 + start_header.next_header_size;
        metadata.insert(
            "HeaderSize".to_string(),
            TagValue::String(header_size.to_string()),
        );

        // Header overhead (total header size vs actual data)
        let header_overhead = START_HEADER_SIZE as u64 + start_header.next_header_size;
        metadata.insert(
            "HeaderOverhead".to_string(),
            TagValue::String(header_overhead.to_string()),
        );

        // Validate header CRC if possible
        if let Ok(header_data) = reader.read(0, START_HEADER_SIZE) {
            let calculated_crc = StartHeader::calculate_header_crc(header_data);
            let crc_valid = calculated_crc == start_header.start_header_crc;
            metadata.insert(
                "HeaderCRCValid".to_string(),
                TagValue::String(crc_valid.to_string()),
            );
        }

        // Detect if archive has encoded header (check if next header exists)
        let has_encoded_header = start_header.next_header_size > 0;
        metadata.insert(
            "HasEncodedHeader".to_string(),
            TagValue::String(has_encoded_header.to_string()),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::SevenZ)
    }
}

/// Standalone function for parsing 7z metadata
///
/// This function provides a convenient interface for parsing 7-Zip archive metadata
/// by instantiating the SevenZParser and calling its parse method.
///
/// # Arguments
///
/// * `reader` - A FileReader providing access to the 7z file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error description
pub fn parse_7z_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = SevenZParser;
    parser
        .parse(reader)
        .map_err(|e| format!("7z parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_7z_signature() {
        let data = vec![0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
        let reader = TestReader::new(data);
        assert!(SevenZParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_7z_start_header_parse() {
        // Create minimal valid 7z header
        let data = vec![
            0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, // Signature
            0x00, 0x04, // Version 0.4
            0x00, 0x00, 0x00, 0x00, // Start header CRC
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Next header offset
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Next header size
            0x00, 0x00, 0x00, 0x00, // Next header CRC
        ];
        let reader = TestReader::new(data);

        let header = StartHeader::parse(&reader).unwrap();
        assert_eq!(header.major_version, 0);
        assert_eq!(header.minor_version, 4);
    }

    #[test]
    fn test_7z_metadata_extraction() {
        // Create minimal valid 7z header
        let data = vec![
            0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, // Signature
            0x00, 0x04, // Version 0.4
            0x27, 0x17, 0xB5, 0xD0, // Start header CRC (example)
            0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Next header offset: 32
            0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Next header size: 64
            0x00, 0x00, 0x00, 0x00, // Next header CRC
        ];
        let reader = TestReader::new(data);

        let parser = SevenZParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType").unwrap(),
            &TagValue::String("7z".to_string())
        );
        assert_eq!(
            metadata.get("7zVersion").unwrap(),
            &TagValue::String("0.4".to_string())
        );
        assert_eq!(
            metadata.get("NextHeaderOffset").unwrap(),
            &TagValue::String("32".to_string())
        );
        assert_eq!(
            metadata.get("NextHeaderSize").unwrap(),
            &TagValue::String("64".to_string())
        );
        assert_eq!(
            metadata.get("HasEncodedHeader").unwrap(),
            &TagValue::String("true".to_string())
        );
    }
}
