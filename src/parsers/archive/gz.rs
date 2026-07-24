//! GZIP compressed file format parser
//!
//! Implements comprehensive metadata extraction from GZIP files including
//! header fields, optional fields, and trailer information.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// GZIP signature: 0x1F 0x8B
const GZ_SIGNATURE: &[u8] = &[0x1F, 0x8B];

/// GZIP flag bits
const FTEXT: u8 = 0x01;
const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;

/// GZIP parser for extracting metadata from compressed files
pub struct GZParser;

impl GZParser {
    /// Verifies GZIP signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 2 {
            return Ok(false);
        }

        let header = reader.read(0, 2)?;
        Ok(header == GZ_SIGNATURE)
    }

    /// Parses the GZIP header and returns the offset after all header fields
    pub fn parse_header(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<u64> {
        if reader.size() < 10 {
            return Err(ExifToolError::parse_error("GZIP header too short"));
        }

        let header = reader.read(0, 10)?;
        let r = EndianReader::little_endian(header);

        let method = header[2];
        let compression_name = match method {
            8 => "DEFLATE",
            _ => "Unknown",
        };
        metadata.insert(
            "CompressionMethod".to_string(),
            TagValue::String(compression_name.to_string()),
        );

        let flags = header[3];

        // MTIME: Unix timestamp (4 bytes, little-endian)
        let mtime = r.u32_at(4).unwrap_or(0);
        if mtime != 0 {
            use chrono::{TimeZone, Utc};
            if let Some(dt) = Utc.timestamp_opt(mtime as i64, 0).single() {
                metadata.insert(
                    "ModificationTime".to_string(),
                    TagValue::String(dt.format("%Y:%m:%d %H:%M:%S").to_string()),
                );
            }
        }

        // XFL: Extra flags
        let xfl = header[8];
        let compression_level = match xfl {
            2 => "Maximum compression",
            4 => "Fastest compression",
            _ => "Normal",
        };
        metadata.insert(
            "CompressionLevel".to_string(),
            TagValue::String(compression_level.to_string()),
        );

        // OS: Operating system
        let os_name = match header[9] {
            0 => "FAT",
            1 => "Amiga",
            2 => "VMS",
            3 => "Unix",
            4 => "VM/CMS",
            5 => "Atari TOS",
            6 => "HPFS",
            7 => "Macintosh",
            8 => "Z-System",
            9 => "CP/M",
            10 => "TOPS-20",
            11 => "NTFS",
            12 => "QDOS",
            13 => "Acorn RISCOS",
            255 => "Unknown",
            _ => "Unknown",
        };
        metadata.insert(
            "OperatingSystem".to_string(),
            TagValue::String(os_name.to_string()),
        );

        let mut offset = 10u64;

        // FEXTRA: Extra field
        if flags & FEXTRA != 0 {
            if reader.size() < offset + 2 {
                return Ok(offset);
            }
            let xlen_bytes = reader.read(offset, 2)?;
            let r = EndianReader::little_endian(xlen_bytes);
            let xlen = r.u16_at(0).unwrap_or(0) as u64;
            offset += 2 + xlen;
        }

        // FNAME: Original filename
        if flags & FNAME != 0
            && let Some(filename) = Self::read_null_terminated_string(reader, offset)?
        {
            metadata.insert(
                "ZIP:ArchivedFileName".to_string(),
                TagValue::String(filename.0),
            );
            offset = filename.1;
        }

        // FCOMMENT: Comment
        if flags & FCOMMENT != 0
            && let Some(comment) = Self::read_null_terminated_string(reader, offset)?
        {
            metadata.insert("ZIP:Comment".to_string(), TagValue::String(comment.0));
            offset = comment.1;
        }

        // FHCRC: Header CRC
        if flags & FHCRC != 0 {
            offset += 2;
        }

        Ok(offset)
    }

    /// Reads null-terminated string from offset, returns (string, next_offset) or None
    fn read_null_terminated_string(
        reader: &dyn FileReader,
        offset: u64,
    ) -> Result<Option<(String, u64)>> {
        let available = (reader.size().saturating_sub(offset)).min(256);
        if available == 0 {
            return Ok(None);
        }

        let data = reader.read(offset, available as usize)?;
        data.iter()
            .position(|&b| b == 0)
            .map(|pos| {
                (
                    String::from_utf8_lossy(&data[..pos]).to_string(),
                    offset + pos as u64 + 1,
                )
            })
            .map_or(Ok(None), |v| Ok(Some(v)))
    }

    /// Parses the GZIP trailer (last 8 bytes: CRC32 and original size)
    pub fn parse_trailer(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        let size = reader.size();
        if size < 8 {
            return Ok(());
        }

        let trailer = reader.read(size - 8, 8)?;
        let r = EndianReader::little_endian(trailer);
        let crc32 = r.u32_at(0).unwrap_or(0);
        let isize = r.u32_at(4).unwrap_or(0);

        metadata.insert(
            "CRC32".to_string(),
            TagValue::String(format!("0x{:08X}", crc32)),
        );
        metadata.insert(
            "OriginalSize".to_string(),
            TagValue::String(isize.to_string()),
        );
        Ok(())
    }
}

impl FormatParser for GZParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify signature
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid GZIP signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("GZIP".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Parse header fields including optional fields
        Self::parse_header(reader, &mut metadata)?;

        // Parse trailer (CRC32 and original size)
        Self::parse_trailer(reader, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::GZ)
    }
}

/// Standalone function for parsing GZIP metadata
pub fn parse_gz_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    GZParser
        .parse(reader)
        .map_err(|e| format!("GZIP parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_gz_signature() {
        let data = vec![0x1F, 0x8B, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03];
        let reader = TestReader::new(data);
        assert!(GZParser::verify_signature(&reader).unwrap());
    }
}
