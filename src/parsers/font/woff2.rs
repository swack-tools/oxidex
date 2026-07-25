//! Web Open Font Format 2 (WOFF2) parser
//!
//! Implements metadata extraction from WOFF2 font files.
//! WOFF2 uses Brotli compression and table transformations.
//!
//! WOFF2 files use big-endian byte order for all multi-byte fields.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// WOFF2 signature: "wOF2"
const WOFF2_SIGNATURE: &[u8] = b"wOF2";
/// Minimum WOFF2 header size (48 bytes)
const WOFF2_HEADER_SIZE: u64 = 48;

/// WOFF2 parser for extracting metadata from Web Open Font Format 2 files
pub struct WOFF2Parser;

impl WOFF2Parser {
    /// Verifies WOFF2 signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }

        let header = reader.read(0, 4)?;
        Ok(header == WOFF2_SIGNATURE)
    }

    /// Reads the full WOFF2 header (48 bytes)
    fn read_header(reader: &dyn FileReader) -> Result<WOFF2Header> {
        if reader.size() < WOFF2_HEADER_SIZE {
            return Err(ExifToolError::parse_error(
                "File too small for WOFF2 header",
            ));
        }

        let header_data = reader.read(0, WOFF2_HEADER_SIZE as usize)?;
        let r = EndianReader::big_endian(header_data);

        Ok(WOFF2Header {
            flavor: Self::parse_flavor(&header_data[4..8]),
            length: r.u32_at(8).unwrap_or(0),
            num_tables: r.u16_at(12).unwrap_or(0),
            total_sfnt_size: r.u32_at(16).unwrap_or(0),
            total_compressed_size: r.u32_at(20).unwrap_or(0),
            major_version: r.u16_at(24).unwrap_or(0),
            minor_version: r.u16_at(26).unwrap_or(0),
            meta_offset: r.u32_at(28).unwrap_or(0),
            meta_length: r.u32_at(32).unwrap_or(0),
            meta_orig_length: r.u32_at(36).unwrap_or(0),
            priv_offset: r.u32_at(40).unwrap_or(0),
            priv_length: r.u32_at(44).unwrap_or(0),
        })
    }

    /// Parses the flavor field to determine font type
    fn parse_flavor(flavor_bytes: &[u8]) -> String {
        if flavor_bytes == [0x00, 0x01, 0x00, 0x00] {
            "TrueType".to_string()
        } else if flavor_bytes == b"OTTO" {
            "CFF".to_string()
        } else {
            "Unknown".to_string()
        }
    }
}

/// WOFF2 header structure (48 bytes)
struct WOFF2Header {
    flavor: String,
    length: u32,
    num_tables: u16,
    total_sfnt_size: u32,
    total_compressed_size: u32,
    major_version: u16,
    minor_version: u16,
    meta_offset: u32,
    meta_length: u32,
    meta_orig_length: u32,
    priv_offset: u32,
    priv_length: u32,
}

impl FormatParser for WOFF2Parser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid WOFF2 signature"));
        }

        let header = Self::read_header(reader)?;
        let mut metadata = MetadataMap::new();

        // Basic file information
        metadata.insert(
            "FileType".to_string(),
            TagValue::String("WOFF2".to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );
        metadata.insert("FontFlavor".to_string(), TagValue::String(header.flavor));

        // WOFF2 header fields
        metadata.insert(
            "NumTables".to_string(),
            TagValue::Integer(header.num_tables as i64),
        );
        metadata.insert(
            "TotalSfntSize".to_string(),
            TagValue::Integer(header.total_sfnt_size as i64),
        );
        metadata.insert(
            "TotalCompressedSize".to_string(),
            TagValue::Integer(header.total_compressed_size as i64),
        );

        // Version information
        let version = format!("{}.{}", header.major_version, header.minor_version);
        metadata.insert("FontVersion".to_string(), TagValue::String(version));

        // Compression ratio (compressed / original * 100)
        if header.total_sfnt_size > 0 {
            let ratio =
                (header.total_compressed_size as f64 / header.total_sfnt_size as f64) * 100.0;
            metadata.insert(
                "CompressionRatio".to_string(),
                TagValue::String(format!("{:.1}%", ratio)),
            );
        }

        // Metadata block presence
        let has_metadata = header.meta_offset > 0 && header.meta_length > 0;
        metadata.insert(
            "HasMetadata".to_string(),
            TagValue::String(if has_metadata {
                "Yes".to_string()
            } else {
                "No".to_string()
            }),
        );

        if has_metadata {
            metadata.insert(
                "MetadataOffset".to_string(),
                TagValue::Integer(header.meta_offset as i64),
            );
            metadata.insert(
                "MetadataLength".to_string(),
                TagValue::Integer(header.meta_length as i64),
            );
            metadata.insert(
                "MetadataOrigLength".to_string(),
                TagValue::Integer(header.meta_orig_length as i64),
            );
        }

        // Private data block presence
        let has_private = header.priv_offset > 0 && header.priv_length > 0;
        metadata.insert(
            "HasPrivateData".to_string(),
            TagValue::String(if has_private {
                "Yes".to_string()
            } else {
                "No".to_string()
            }),
        );

        if has_private {
            metadata.insert(
                "PrivateDataOffset".to_string(),
                TagValue::Integer(header.priv_offset as i64),
            );
            metadata.insert(
                "PrivateDataLength".to_string(),
                TagValue::Integer(header.priv_length as i64),
            );
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::WOFF2)
    }
}

/// Parses metadata from WOFF2 files.
///
/// This is a convenience wrapper around WOFF2Parser that provides a functional API.
pub fn parse_woff2_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = WOFF2Parser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn create_woff2_header(
        flavor: &[u8; 4],
        num_tables: u16,
        total_sfnt_size: u32,
        total_compressed_size: u32,
        major_version: u16,
        minor_version: u16,
        has_metadata: bool,
        has_private: bool,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(48);

        // Signature
        data.extend_from_slice(b"wOF2");
        // Flavor
        data.extend_from_slice(flavor);
        // Length (48 for header only)
        data.extend_from_slice(&48u32.to_be_bytes());
        // NumTables
        data.extend_from_slice(&num_tables.to_be_bytes());
        // Reserved
        data.extend_from_slice(&[0, 0]);
        // TotalSfntSize
        data.extend_from_slice(&total_sfnt_size.to_be_bytes());
        // TotalCompressedSize
        data.extend_from_slice(&total_compressed_size.to_be_bytes());
        // MajorVersion
        data.extend_from_slice(&major_version.to_be_bytes());
        // MinorVersion
        data.extend_from_slice(&minor_version.to_be_bytes());
        // MetaOffset
        data.extend_from_slice(&(if has_metadata { 100u32 } else { 0u32 }).to_be_bytes());
        // MetaLength
        data.extend_from_slice(&(if has_metadata { 50u32 } else { 0u32 }).to_be_bytes());
        // MetaOrigLength
        data.extend_from_slice(&(if has_metadata { 100u32 } else { 0u32 }).to_be_bytes());
        // PrivOffset
        data.extend_from_slice(&(if has_private { 200u32 } else { 0u32 }).to_be_bytes());
        // PrivLength
        data.extend_from_slice(&(if has_private { 75u32 } else { 0u32 }).to_be_bytes());

        data
    }

    #[test]
    fn test_woff2_signature() {
        let data = create_woff2_header(
            &[0x00, 0x01, 0x00, 0x00],
            10,
            10000,
            5000,
            1,
            0,
            false,
            false,
        );
        let reader = TestReader::new(data);
        assert!(WOFF2Parser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_woff2_full_header() {
        let data =
            create_woff2_header(&[0x00, 0x01, 0x00, 0x00], 12, 20000, 8000, 1, 5, true, true);
        let reader = TestReader::new(data);
        let parser = WOFF2Parser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType").unwrap(),
            &TagValue::String("WOFF2".to_string())
        );
        assert_eq!(
            metadata.get("FontFlavor").unwrap(),
            &TagValue::String("TrueType".to_string())
        );
        assert_eq!(metadata.get("NumTables").unwrap(), &TagValue::Integer(12));
        assert_eq!(
            metadata.get("TotalSfntSize").unwrap(),
            &TagValue::Integer(20000)
        );
        assert_eq!(
            metadata.get("TotalCompressedSize").unwrap(),
            &TagValue::Integer(8000)
        );
        assert_eq!(
            metadata.get("FontVersion").unwrap(),
            &TagValue::String("1.5".to_string())
        );
        assert_eq!(
            metadata.get("HasMetadata").unwrap(),
            &TagValue::String("Yes".to_string())
        );
        assert_eq!(
            metadata.get("HasPrivateData").unwrap(),
            &TagValue::String("Yes".to_string())
        );
    }

    #[test]
    fn test_woff2_compression_ratio() {
        let data = create_woff2_header(&b"OTTO", 10, 10000, 4000, 2, 0, false, false);
        let reader = TestReader::new(data);
        let parser = WOFF2Parser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FontFlavor").unwrap(),
            &TagValue::String("CFF".to_string())
        );
        assert_eq!(
            metadata.get("CompressionRatio").unwrap(),
            &TagValue::String("40.0%".to_string())
        );
    }

    #[test]
    fn test_woff2_no_metadata_or_private() {
        let data = create_woff2_header(
            &[0x00, 0x01, 0x00, 0x00],
            8,
            15000,
            6000,
            1,
            0,
            false,
            false,
        );
        let reader = TestReader::new(data);
        let parser = WOFF2Parser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("HasMetadata").unwrap(),
            &TagValue::String("No".to_string())
        );
        assert_eq!(
            metadata.get("HasPrivateData").unwrap(),
            &TagValue::String("No".to_string())
        );
        assert!(metadata.get("MetadataOffset").is_none());
        assert!(metadata.get("PrivateDataOffset").is_none());
    }
}
