//! MTS (MPEG Transport Stream) format parser
//!
//! Implements metadata extraction from MPEG Transport Stream files (.mts, .m2ts),
//! commonly used for HD video recording on camcorders and Blu-ray discs.
//!
//! # Supported Metadata
//!
//! - **Transport Stream:** Packet count, sync byte verification
//! - **PAT/PMT:** Program information (basic detection)
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `M2TS.pm` module:
//! - `M2TS:PacketSize` → Transport stream packet size (188 or 192 bytes)
//! - `M2TS:PacketCount` → Number of packets detected
//!
//! # File Structure
//!
//! ```text
//! [TS Packet 0 - 188 bytes]
//!   ├─ Sync Byte: 0x47 (1 byte)
//!   ├─ Header: 3 bytes (PID, flags)
//!   └─ Payload: 184 bytes
//! [TS Packet 1 - 188 bytes]
//! [TS Packet 2 - 188 bytes]
//! ...
//! ```
//!
//! M2TS variant has 4-byte timestamp prefix before sync byte (192 bytes total).
//!
//! # References
//!
//! - ISO 13818-1: MPEG-2 Transport Stream Specification
//! - ExifTool Source: `lib/Image/ExifTool/M2TS.pm`

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// MPEG-TS sync byte (appears every 188 or 192 bytes)
const TS_SYNC_BYTE: u8 = 0x47;

/// Standard TS packet size (188 bytes)
const TS_PACKET_SIZE: usize = 188;

/// M2TS packet size with timestamp (192 bytes)
const M2TS_PACKET_SIZE: usize = 192;

/// MTS parser
pub struct MtsParser;

impl FormatParser for MtsParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let file_size = reader.size();

        // Verify file is large enough for at least 3 packets
        if file_size < (TS_PACKET_SIZE * 3) as u64 {
            return Err(ExifToolError::parse_error("File too small to be MTS"));
        }

        // Read first 1024 bytes to detect packet size
        let header_size = (TS_PACKET_SIZE * 5).min(file_size as usize);
        let header = reader.read(0, header_size)?;

        // Detect packet size by checking sync byte pattern
        let packet_size = detect_packet_size(header)?;

        let mut metadata = MetadataMap::with_capacity(8);

        metadata.insert(
            "M2TS:PacketSize".to_string(),
            TagValue::new_integer(packet_size as i64),
        );

        // Verify sync bytes at regular intervals
        let mut sync_count = 0;
        let max_packets_to_check = 100.min((file_size / packet_size as u64) as usize);

        for i in 0..max_packets_to_check {
            let offset = (i * packet_size) as u64;

            // For M2TS, sync byte is at offset 4 (after timestamp)
            let sync_offset = if packet_size == M2TS_PACKET_SIZE {
                offset + 4
            } else {
                offset
            };

            if sync_offset >= file_size {
                break;
            }

            let sync_byte = reader.read(sync_offset, 1)?;
            if sync_byte[0] == TS_SYNC_BYTE {
                sync_count += 1;
            } else {
                // Sync byte mismatch - might not be a valid TS file
                break;
            }
        }

        if sync_count < 3 {
            return Err(ExifToolError::parse_error(
                "Failed to verify MPEG-TS sync bytes",
            ));
        }

        // Calculate total packet count
        let total_packets = file_size / packet_size as u64;
        metadata.insert(
            "M2TS:PacketCount".to_string(),
            TagValue::new_integer(total_packets as i64),
        );

        // Add format type
        let format_type = if packet_size == M2TS_PACKET_SIZE {
            "M2TS (with timestamp)"
        } else {
            "MTS (standard)"
        };
        metadata.insert(
            "M2TS:FormatType".to_string(),
            TagValue::new_string(format_type.to_string()),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::MTS)
    }
}

/// Convenience function to parse MTS metadata from a reader.
///
/// This is a wrapper around `MtsParser::parse()` to provide a simpler API
/// for the operations module.
///
/// # Arguments
///
/// * `reader` - FileReader implementation providing access to the MTS file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_mts_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = MtsParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Detect packet size by checking sync byte patterns
fn detect_packet_size(data: &[u8]) -> Result<usize> {
    // Check for standard TS (188 bytes)
    if data.len() >= TS_PACKET_SIZE * 3
        && data[0] == TS_SYNC_BYTE
        && data[TS_PACKET_SIZE] == TS_SYNC_BYTE
        && data[TS_PACKET_SIZE * 2] == TS_SYNC_BYTE
    {
        return Ok(TS_PACKET_SIZE);
    }

    // Check for M2TS (192 bytes with 4-byte timestamp prefix)
    if data.len() >= M2TS_PACKET_SIZE * 3
        && data[4] == TS_SYNC_BYTE
        && data[M2TS_PACKET_SIZE + 4] == TS_SYNC_BYTE
        && data[M2TS_PACKET_SIZE * 2 + 4] == TS_SYNC_BYTE
    {
        return Ok(M2TS_PACKET_SIZE);
    }

    Err(ExifToolError::parse_error(
        "Invalid MPEG-TS signature: sync byte pattern not found",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_mts_signature_valid() {
        // Create minimal MTS file with sync bytes at correct intervals
        let mut data = vec![0u8; TS_PACKET_SIZE * 5];

        // Place sync bytes at correct offsets
        for i in 0..5 {
            data[i * TS_PACKET_SIZE] = TS_SYNC_BYTE;
        }

        let reader = TestReader::from_slice(&data);
        let parser = MtsParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get("M2TS:PacketSize").unwrap().as_integer(),
            Some(188)
        );
    }

    #[test]
    fn test_m2ts_signature_valid() {
        // Create minimal M2TS file (192 bytes with timestamp)
        let mut data = vec![0u8; M2TS_PACKET_SIZE * 5];

        // Place sync bytes at offset 4 within each packet
        for i in 0..5 {
            data[i * M2TS_PACKET_SIZE + 4] = TS_SYNC_BYTE;
        }

        let reader = TestReader::from_slice(&data);
        let parser = MtsParser;
        let result = parser.parse(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get("M2TS:PacketSize").unwrap().as_integer(),
            Some(192)
        );
    }

    #[test]
    fn test_mts_signature_invalid() {
        let data = b"INVALID DATA";
        let reader = TestReader::from_slice(data);
        let parser = MtsParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_mts_file_too_small() {
        let data = vec![TS_SYNC_BYTE; 100];
        let reader = TestReader::from_slice(&data);
        let parser = MtsParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
