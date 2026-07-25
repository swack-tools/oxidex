//! ICO (Windows Icon) format parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

const ICO_SIGNATURE: &[u8] = &[0x00, 0x00];
const ICO_TYPE_ICON: u16 = 1;
const ICO_TYPE_CURSOR: u16 = 2;

/// Parser for Windows ICO (Icon) and CUR (Cursor) image files
///
/// Extracts metadata from ICO/CUR files including image count, dimensions, and bit depth.
pub struct ICOParser;

#[derive(Debug)]
struct IcoDirectoryEntry {
    width: u16,
    height: u16,
    color_count: u8,
    bits_per_pixel: u16,
}

impl ICOParser {
    /// Verifies the ICO/CUR file signature (0x00 0x00 followed by type 0x01 or 0x02)
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        if &header[0..2] != ICO_SIGNATURE {
            return Ok(false);
        }
        // ICO uses little-endian byte order
        let header_reader = EndianReader::little_endian(header);
        let file_type = header_reader.u16_at(2).unwrap_or(0);
        Ok(file_type == ICO_TYPE_ICON || file_type == ICO_TYPE_CURSOR)
    }

    /// Reads the file type (1=ICO, 2=CUR)
    fn read_file_type(reader: &dyn FileReader) -> Result<u16> {
        let header = reader.read(2, 2)?;
        // ICO uses little-endian byte order
        let header_reader = EndianReader::little_endian(header);
        Ok(header_reader.u16_at(0).unwrap_or(0))
    }

    /// Reads the image count from header
    fn read_image_count(reader: &dyn FileReader) -> Result<u16> {
        let count_bytes = reader.read(4, 2)?;
        // ICO uses little-endian byte order
        let count_reader = EndianReader::little_endian(count_bytes);
        Ok(count_reader.u16_at(0).unwrap_or(0))
    }

    /// Parses a directory entry at the given offset
    fn read_directory_entry(reader: &dyn FileReader, offset: u64) -> Result<IcoDirectoryEntry> {
        let entry = reader.read(offset, 16)?;
        // ICO uses little-endian byte order
        let entry_reader = EndianReader::little_endian(entry);

        // Width and height: 0 means 256
        let width = if entry[0] == 0 { 256 } else { entry[0] as u16 };
        let height = if entry[1] == 0 { 256 } else { entry[1] as u16 };
        let color_count = entry[2];

        // Bits per pixel at offset 6-7 (for ICO) or hotspot Y (for CUR)
        let bits_per_pixel = entry_reader.u16_at(6).unwrap_or(0);

        Ok(IcoDirectoryEntry {
            width,
            height,
            color_count,
            bits_per_pixel,
        })
    }

    /// Reads all directory entries and extracts metadata
    fn analyze_entries(reader: &dyn FileReader, count: u16) -> Result<(u16, u16, u16, String)> {
        let mut max_width = 0u16;
        let mut max_height = 0u16;
        let mut max_bits = 0u16;
        let mut sizes = Vec::new();

        for i in 0..count {
            let offset = 6 + (i as u64 * 16);
            if offset + 16 > reader.size() {
                break;
            }

            let entry = Self::read_directory_entry(reader, offset)?;

            if entry.width > max_width {
                max_width = entry.width;
            }
            if entry.height > max_height {
                max_height = entry.height;
            }
            if entry.bits_per_pixel > max_bits {
                max_bits = entry.bits_per_pixel;
            }

            sizes.push(format!("{}x{}", entry.width, entry.height));
        }

        let available_sizes = sizes.join(", ");
        Ok((max_width, max_height, max_bits, available_sizes))
    }
}

impl FormatParser for ICOParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid ICO/CUR signature"));
        }

        let mut metadata = MetadataMap::new();

        let file_type = Self::read_file_type(reader)?;
        let type_name = if file_type == ICO_TYPE_CURSOR {
            "CUR"
        } else {
            "ICO"
        };

        metadata.insert(
            "FileType".to_string(),
            TagValue::String(type_name.to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        let image_count = Self::read_image_count(reader)?;
        metadata.insert(
            "ImageCount".to_string(),
            TagValue::String(image_count.to_string()),
        );

        if image_count > 0 && reader.size() >= 6 + (image_count as u64 * 16) {
            let (max_width, max_height, max_bits, available_sizes) =
                Self::analyze_entries(reader, image_count)?;

            metadata.insert(
                "ImageWidth".to_string(),
                TagValue::String(max_width.to_string()),
            );
            metadata.insert(
                "ImageHeight".to_string(),
                TagValue::String(max_height.to_string()),
            );
            metadata.insert(
                "BitDepth".to_string(),
                TagValue::String(max_bits.to_string()),
            );
            metadata.insert(
                "AvailableSizes".to_string(),
                TagValue::String(available_sizes),
            );
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::ICO)
    }
}

/// Parses metadata from ICO files.
///
/// This is a convenience wrapper around ICOParser that provides a functional API.
pub fn parse_ico_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = ICOParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
