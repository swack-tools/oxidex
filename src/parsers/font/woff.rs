//! Web Open Font Format (WOFF) parser
//!
//! Implements comprehensive metadata extraction from WOFF font files.
//!
//! WOFF files use big-endian byte order for all multi-byte fields.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use flate2::read::ZlibDecoder;
use std::io::Read;

/// WOFF signature: "wOFF"
const WOFF_SIGNATURE: &[u8] = b"wOFF";

/// WOFF header structure (44 bytes)
#[derive(Debug)]
struct WOFFHeader {
    num_tables: u16,
    total_sfnt_size: u32,
    major_version: u16,
    minor_version: u16,
    meta_offset: u32,
    meta_length: u32,
    meta_orig_length: u32,
}

/// WOFF table directory entry (20 bytes)
#[derive(Debug)]
struct WOFFTableEntry {
    tag: [u8; 4],
    offset: u32,
    comp_length: u32,
    orig_length: u32,
}

/// WOFF parser for extracting metadata from Web Open Fonts
pub struct WOFFParser;

impl WOFFParser {
    /// Verifies WOFF signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }

        let header = reader.read(0, 4)?;
        Ok(header == WOFF_SIGNATURE)
    }

    /// Reads flavor (offset 4, 4 bytes) - indicates original font type
    pub fn read_flavor(reader: &dyn FileReader) -> Result<String> {
        if reader.size() < 8 {
            return Ok("Unknown".to_string());
        }

        let flavor = reader.read(4, 4)?;
        if flavor == [0x00, 0x01, 0x00, 0x00] {
            Ok("TrueType".to_string())
        } else if flavor == b"OTTO" {
            Ok("CFF".to_string())
        } else {
            Ok("Unknown".to_string())
        }
    }

    /// Parses WOFF header (44 bytes)
    fn parse_header(reader: &dyn FileReader) -> Result<(WOFFHeader, u32, u32)> {
        if reader.size() < 44 {
            return Err(ExifToolError::parse_error("WOFF header too short"));
        }

        let header_data = reader.read(0, 44)?;
        let r = EndianReader::big_endian(header_data);

        let header = WOFFHeader {
            num_tables: r.u16_at(12).unwrap_or(0),
            total_sfnt_size: r.u32_at(16).unwrap_or(0),
            major_version: r.u16_at(20).unwrap_or(0),
            minor_version: r.u16_at(22).unwrap_or(0),
            meta_offset: r.u32_at(24).unwrap_or(0),
            meta_length: r.u32_at(28).unwrap_or(0),
            meta_orig_length: r.u32_at(32).unwrap_or(0),
        };

        // Extract privOffset and privLength
        let priv_offset = r.u32_at(36).unwrap_or(0);
        let priv_length = r.u32_at(40).unwrap_or(0);

        Ok((header, priv_offset, priv_length))
    }

    /// Parses table directory entry
    fn parse_table_entry(reader: &dyn FileReader, offset: u64) -> Result<WOFFTableEntry> {
        if reader.size() < offset + 20 {
            return Err(ExifToolError::parse_error("Table entry too short"));
        }

        let entry_data = reader.read(offset, 20)?;
        let r = EndianReader::big_endian(entry_data);

        Ok(WOFFTableEntry {
            tag: [entry_data[0], entry_data[1], entry_data[2], entry_data[3]],
            offset: r.u32_at(4).unwrap_or(0),
            comp_length: r.u32_at(8).unwrap_or(0),
            orig_length: r.u32_at(12).unwrap_or(0),
        })
    }

    /// Decompresses zlib-compressed data
    fn decompress_zlib(compressed: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = ZlibDecoder::new(compressed);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| ExifToolError::parse_error(format!("Decompression failed: {}", e)))?;
        Ok(decompressed)
    }

    /// Extracts metadata from XML metadata block (simplified)
    fn parse_xml_metadata(xml: &str) -> Vec<(String, String)> {
        let tags = [
            "vendor",
            "credits",
            "description",
            "license",
            "copyright",
            "trademark",
        ];
        tags.iter()
            .filter_map(|tag| {
                let open_tag = format!("<{}", tag);
                let close_tag = format!("</{}>", tag);
                let start = xml.find(&open_tag)?;
                let content_start = start + xml[start..].find('>')? + 1;
                let end = xml[content_start..].find(&close_tag)?;
                let content = xml[content_start..content_start + end].trim();
                if content.is_empty() {
                    return None;
                }
                let mut key = String::from("WOFF");
                key.push_str(&tag[..1].to_uppercase());
                key.push_str(&tag[1..]);
                Some((key, content.to_string()))
            })
            .collect()
    }

    /// Finds table by tag
    fn find_table(
        reader: &dyn FileReader,
        num_tables: u16,
        tag: &[u8; 4],
    ) -> Result<Option<WOFFTableEntry>> {
        for i in 0..num_tables {
            let entry = Self::parse_table_entry(reader, 44 + (i as u64 * 20))?;
            if &entry.tag == tag {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }

    /// Extracts font names from name table
    fn extract_names_from_table(data: &[u8]) -> Vec<(String, String)> {
        if data.len() < 6 {
            return Vec::new();
        }
        let r = EndianReader::big_endian(data);
        let count = r.u16_at(2).unwrap_or(0);
        let str_offset = r.u16_at(4).unwrap_or(0) as usize;

        (0..count.min(20))
            .filter_map(|i| {
                let rec = 6 + (i as usize * 12);
                if rec + 12 > data.len() {
                    return None;
                }
                let id = r.u16_at(rec + 6)?;
                let len = r.u16_at(rec + 8)? as usize;
                let off = r.u16_at(rec + 10)? as usize;
                let start = str_offset + off;
                if start + len > data.len() {
                    return None;
                }
                let text = String::from_utf8_lossy(&data[start..start + len])
                    .trim()
                    .to_string();
                if text.is_empty() {
                    return None;
                }
                let tag = match id {
                    0 => "FontCopyright",
                    1 => "FontFamily",
                    2 => "FontSubfamily",
                    4 => "FontName",
                    5 => "FontVersion",
                    6 => "PostScriptName",
                    _ => return None,
                };
                Some((tag.to_string(), text))
            })
            .collect()
    }
}

impl FormatParser for WOFFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid WOFF signature"));
        }

        let mut metadata = MetadataMap::new();

        // Basic file info
        metadata.insert("FileType".to_string(), TagValue::String("WOFF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        let flavor = Self::read_flavor(reader)?;
        metadata.insert("FontFlavor".to_string(), TagValue::String(flavor));

        // Parse WOFF header
        let (header, priv_offset, _priv_length) = Self::parse_header(reader)?;
        metadata.insert(
            "NumTables".to_string(),
            TagValue::String(header.num_tables.to_string()),
        );
        metadata.insert(
            "TotalSfntSize".to_string(),
            TagValue::String(header.total_sfnt_size.to_string()),
        );
        metadata.insert(
            "FontVersion".to_string(),
            TagValue::String(format!("{}.{}", header.major_version, header.minor_version)),
        );

        // Calculate compression ratio
        let file_size = reader.size();
        if header.total_sfnt_size > 0 {
            let ratio = (file_size as f64 / header.total_sfnt_size as f64) * 100.0;
            metadata.insert(
                "CompressionRatio".to_string(),
                TagValue::String(format!("{:.1}%", ratio)),
            );
        }

        // Check for metadata and private data blocks
        metadata.insert(
            "HasMetadata".to_string(),
            TagValue::String(if header.meta_offset > 0 { "Yes" } else { "No" }.to_string()),
        );
        metadata.insert(
            "HasPrivateData".to_string(),
            TagValue::String(if priv_offset > 0 { "Yes" } else { "No" }.to_string()),
        );

        // Extract XML metadata if present
        if header.meta_offset > 0 && header.meta_length > 0 {
            let meta_offset = header.meta_offset as u64;
            let meta_length = header.meta_length as usize;

            if reader.size() >= meta_offset + meta_length as u64
                && let Ok(compressed) = reader.read(meta_offset, meta_length)
                && let Ok(decompressed) = Self::decompress_zlib(compressed)
                && let Ok(xml_str) = String::from_utf8(decompressed)
            {
                let xml_metadata = Self::parse_xml_metadata(&xml_str);
                for (key, value) in xml_metadata {
                    metadata.insert(key, TagValue::String(value));
                }
            }
        }

        // Try to extract font names from name table
        if let Ok(Some(name_table)) = Self::find_table(reader, header.num_tables, b"name") {
            let table_offset = name_table.offset as u64;
            let table_length = name_table.comp_length as usize;

            if reader.size() >= table_offset + table_length as u64
                && let Ok(compressed) = reader.read(table_offset, table_length)
            {
                // Try decompression if compressed
                let name_data = if name_table.comp_length < name_table.orig_length {
                    Self::decompress_zlib(compressed).unwrap_or_else(|_| compressed.to_vec())
                } else {
                    compressed.to_vec()
                };

                let names = Self::extract_names_from_table(&name_data);
                for (key, value) in names {
                    if !metadata.contains_key(&key) {
                        metadata.insert(key, TagValue::String(value));
                    }
                }
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::WOFF)
    }
}

/// Parses metadata from WOFF files.
///
/// This is a convenience wrapper around WOFFParser that provides a functional API.
pub fn parse_woff_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = WOFFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn create_minimal_woff_header(num_tables: u16, version: (u16, u16)) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(b"wOFF"); // signature
        header.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // flavor (TTF)
        header.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // length (placeholder)
        header.extend_from_slice(&num_tables.to_be_bytes()); // numTables
        header.extend_from_slice(&[0x00, 0x00]); // reserved
        header.extend_from_slice(&[0x00, 0x00, 0x10, 0x00]); // totalSfntSize
        header.extend_from_slice(&version.0.to_be_bytes()); // majorVersion
        header.extend_from_slice(&version.1.to_be_bytes()); // minorVersion
        header.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // metaOffset
        header.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // metaLength
        header.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // metaOrigLength
        header.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // privOffset
        header.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // privLength
        header
    }

    #[test]
    fn test_woff_signature() {
        let mut data = b"wOFF".to_vec();
        data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        let reader = TestReader::new(data);
        assert!(WOFFParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_woff_truetype_flavor() {
        let mut data = b"wOFF".to_vec();
        data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        let reader = TestReader::new(data);
        assert_eq!(WOFFParser::read_flavor(&reader).unwrap(), "TrueType");
    }

    #[test]
    fn test_woff_cff_flavor() {
        let mut data = b"wOFF".to_vec();
        data.extend_from_slice(b"OTTO");
        let reader = TestReader::new(data);
        assert_eq!(WOFFParser::read_flavor(&reader).unwrap(), "CFF");
    }

    #[test]
    fn test_woff_header_parsing() {
        let header = create_minimal_woff_header(5, (1, 2));
        let reader = TestReader::new(header);
        let (parsed, priv_offset, priv_length) = WOFFParser::parse_header(&reader).unwrap();
        assert_eq!(parsed.num_tables, 5);
        assert_eq!(parsed.major_version, 1);
        assert_eq!(parsed.minor_version, 2);
        assert_eq!(parsed.total_sfnt_size, 0x1000);
        assert_eq!(priv_offset, 0);
        assert_eq!(priv_length, 0);
    }

    #[test]
    fn test_woff_parser_integration() {
        let header = create_minimal_woff_header(3, (1, 0));
        let reader = TestReader::new(header);
        let parser = WOFFParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType").unwrap(),
            &TagValue::String("WOFF".to_string())
        );
        assert_eq!(
            metadata.get("FontFlavor").unwrap(),
            &TagValue::String("TrueType".to_string())
        );
        assert_eq!(
            metadata.get("NumTables").unwrap(),
            &TagValue::String("3".to_string())
        );
        assert_eq!(
            metadata.get("FontVersion").unwrap(),
            &TagValue::String("1.0".to_string())
        );
        assert_eq!(
            metadata.get("TotalSfntSize").unwrap(),
            &TagValue::String("4096".to_string())
        );
        assert_eq!(
            metadata.get("HasMetadata").unwrap(),
            &TagValue::String("No".to_string())
        );
        assert_eq!(
            metadata.get("HasPrivateData").unwrap(),
            &TagValue::String("No".to_string())
        );
        assert!(metadata.contains_key("CompressionRatio"));
    }

    #[test]
    fn test_xml_metadata_extraction() {
        let xml = r#"
            <metadata>
                <vendor>Test Vendor</vendor>
                <description>Test Font Description</description>
                <license>MIT License</license>
            </metadata>
        "#;
        let metadata = WOFFParser::parse_xml_metadata(xml);

        assert!(
            metadata
                .iter()
                .any(|(k, v)| k == "WOFFVendor" && v == "Test Vendor")
        );
        assert!(
            metadata
                .iter()
                .any(|(k, v)| k == "WOFFDescription" && v == "Test Font Description")
        );
        assert!(
            metadata
                .iter()
                .any(|(k, v)| k == "WOFFLicense" && v == "MIT License")
        );
    }
}
