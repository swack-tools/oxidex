//! HDF5 (Hierarchical Data Format) parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

const HDF5_SIGNATURE: &[u8] = &[0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A];

/// Parser for HDF5 (Hierarchical Data Format version 5) files
///
/// Extracts metadata from HDF5 scientific data container files.
pub struct HDF5Parser;

impl HDF5Parser {
    /// Verifies the HDF5 file signature (PNG-like header with "HDF")
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 8 {
            return Ok(false);
        }
        Ok(reader.read(0, 8)? == HDF5_SIGNATURE)
    }

    /// Parses HDF5 superblock metadata
    fn parse_superblock(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        if reader.size() < 9 {
            return Err(ExifToolError::parse_error("File too small for superblock"));
        }

        let version = reader.read(8, 1)?[0];
        Self::insert_int(metadata, "SuperblockVersion", version as i64);

        match version {
            0 | 1 => Self::parse_superblock_v0_v1(reader, metadata)?,
            2 | 3 => Self::parse_superblock_v2_v3(reader, metadata)?,
            _ => {
                return Err(ExifToolError::parse_error(format!(
                    "Unsupported superblock version: {}",
                    version
                )));
            }
        }
        Ok(())
    }

    /// Parses superblock version 0 or 1
    fn parse_superblock_v0_v1(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        if reader.size() < 32 {
            return Err(ExifToolError::parse_error(
                "File too small for v0/v1 superblock",
            ));
        }

        // HDF5 uses little-endian byte order
        let sb_data = reader.read(8, 24)?;
        let sb = EndianReader::little_endian(sb_data);

        Self::insert_int(
            metadata,
            "FreeSpaceVersion",
            sb.u8_at(1).unwrap_or(0) as i64,
        );
        Self::insert_int(
            metadata,
            "RootGroupVersion",
            sb.u8_at(2).unwrap_or(0) as i64,
        );
        Self::insert_int(
            metadata,
            "SharedHeaderVersion",
            sb.u8_at(4).unwrap_or(0) as i64,
        );

        let offset_size = sb.u8_at(5).unwrap_or(0);
        Self::insert_int(metadata, "OffsetSize", offset_size as i64);
        Self::insert_addressing_mode(metadata, offset_size);
        Self::insert_int(metadata, "LengthSize", sb.u8_at(6).unwrap_or(0) as i64);

        let group_leaf_k = sb.u16_at(8).unwrap_or(0);
        Self::insert_int(metadata, "GroupLeafNodeK", group_leaf_k as i64);

        let group_internal_k = sb.u16_at(10).unwrap_or(0);
        Self::insert_int(metadata, "GroupInternalNodeK", group_internal_k as i64);

        let flags = sb.u32_at(12).unwrap_or(0);
        Self::insert_int(metadata, "FileConsistencyFlags", flags as i64);
        Self::insert_closed_status(metadata, flags == 0);

        // Base address at offset 16 (after flags)
        if reader.size() >= (32 + offset_size as u64) {
            let addr_bytes = reader.read(24, offset_size as usize)?;
            let base_addr = Self::read_offset(addr_bytes, offset_size);
            Self::insert_int(metadata, "BaseAddress", base_addr as i64);
        }

        Ok(())
    }

    /// Parses superblock version 2 or 3
    fn parse_superblock_v2_v3(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        if reader.size() < 24 {
            return Err(ExifToolError::parse_error(
                "File too small for v2/v3 superblock",
            ));
        }

        let sb = reader.read(8, 16)?;

        let offset_size = sb[1];
        Self::insert_int(metadata, "OffsetSize", offset_size as i64);
        Self::insert_addressing_mode(metadata, offset_size);
        Self::insert_int(metadata, "LengthSize", sb[2] as i64);

        let flags = sb[3];
        Self::insert_int(metadata, "FileConsistencyFlags", flags as i64);
        Self::insert_closed_status(metadata, flags == 0);

        // Base address and EOF address at offset 12+
        if reader.size() >= (20 + 2 * offset_size as u64) {
            let addr_bytes = reader.read(20, offset_size as usize)?;
            let base_addr = Self::read_offset(addr_bytes, offset_size);
            Self::insert_int(metadata, "BaseAddress", base_addr as i64);

            let eof_bytes = reader.read(20 + offset_size as u64, offset_size as usize)?;
            let eof_addr = Self::read_offset(eof_bytes, offset_size);
            Self::insert_int(metadata, "EndOfFileAddress", eof_addr as i64);
        }

        Ok(())
    }

    /// Reads an offset value (4 or 8 bytes) as little-endian
    fn read_offset(bytes: &[u8], size: u8) -> u64 {
        let reader = EndianReader::little_endian(bytes);
        match size {
            4 => reader.u32_at(0).unwrap_or(0) as u64,
            8 => reader.u64_at(0).unwrap_or(0),
            _ => 0,
        }
    }

    // Helper functions to reduce code duplication
    fn insert_int(metadata: &mut MetadataMap, key: &str, value: i64) {
        metadata.insert(key.to_string(), TagValue::Integer(value));
    }

    fn insert_addressing_mode(metadata: &mut MetadataMap, offset_size: u8) {
        let mode = if offset_size == 8 { "64-bit" } else { "32-bit" };
        metadata.insert(
            "AddressingMode".to_string(),
            TagValue::String(mode.to_string()),
        );
    }

    fn insert_closed_status(metadata: &mut MetadataMap, properly_closed: bool) {
        let status = if properly_closed { "Yes" } else { "No" };
        metadata.insert(
            "FileProperlyClosed".to_string(),
            TagValue::String(status.to_string()),
        );
    }
}

impl FormatParser for HDF5Parser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid HDF5 signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("HDF5".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        Self::parse_superblock(reader, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::HDF5)
    }
}

/// Parses metadata from HDF5 files.
///
/// This is a convenience wrapper around HDF5Parser that provides a functional API.
pub fn parse_hdf5_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = HDF5Parser;
    parser.parse(reader).map_err(|e| e.to_string())
}
