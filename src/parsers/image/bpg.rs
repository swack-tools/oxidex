//! BPG (Better Portable Graphics) format parser
//!
//! BPG file structure:
//! - Signature: 4 bytes (0x42 0x50 0x47 0xFB)
//! - Header byte 1: pixel_format(3 bits), alpha1_flag, bit_depth_minus_8(4 bits)
//! - Header byte 2: color_space(4 bits), extension_present_flag, alpha2_flag, limited_range_flag, animation_flag
//! - Width: ue7 variable-length encoding
//! - Height: ue7 variable-length encoding
//! - Picture data length: ue7
//! - Extension data length: ue7 (if extension_present_flag)
//! - Extension data may contain EXIF, ICC profile

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::buffered_reader::BufferedReader;
use crate::io::{ByteOrder as EndianByteOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::parsers::xmp::rdf_parser::parse_xmp;
use crate::tag_db::lookup_tag_name;

const BPG_SIGNATURE: &[u8] = &[0x42, 0x50, 0x47, 0xFB];

/// Parser for BPG (Better Portable Graphics) image files
///
/// Extracts metadata from BPG format images including dimensions, color information,
/// bit depth, pixel format, and embedded EXIF data.
pub struct BPGParser;

impl BPGParser {
    /// Verifies the BPG file signature (0x42 0x50 0x47 0xFB)
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(header == BPG_SIGNATURE)
    }

    /// Parse BPG header and extract metadata
    fn parse_header(data: &[u8], metadata: &mut MetadataMap) -> std::result::Result<usize, String> {
        if data.len() < 6 {
            return Err("BPG file too short".to_string());
        }

        // Verify signature
        if &data[0..4] != BPG_SIGNATURE {
            return Err("Invalid BPG signature".to_string());
        }

        let mut pos = 4;

        // Header byte 1: pixel_format(3 bits), alpha1_flag, bit_depth_minus_8(4 bits)
        let header1 = data[pos];
        let pixel_format = header1 & 0x07;
        let alpha1_flag = (header1 & 0x08) != 0;
        let bit_depth_minus_8 = (header1 >> 4) & 0x0F;
        let bit_depth = (bit_depth_minus_8 as u32 + 8) as i64;

        metadata.insert("BitDepth".to_string(), TagValue::Integer(bit_depth));

        let pixel_format_name = match pixel_format {
            0 => "Grayscale",
            1 => "4:2:0",
            2 => "4:2:2",
            3 => "4:4:4",
            4 => "4:2:0 video range",
            5 => "4:2:2 video range",
            _ => "Unknown",
        };
        metadata.insert(
            "PixelFormat".to_string(),
            TagValue::String(pixel_format_name.to_string()),
        );

        if alpha1_flag {
            metadata.insert("HasAlpha".to_string(), TagValue::String("Yes".to_string()));
        }

        pos += 1;

        // Header byte 2: color_space(4 bits), extension_present_flag, alpha2_flag, limited_range_flag, animation_flag
        let header2 = data[pos];
        let color_space = header2 & 0x0F;
        let extension_present_flag = (header2 & 0x10) != 0;
        let alpha2_flag = (header2 & 0x20) != 0;
        let limited_range_flag = (header2 & 0x40) != 0;
        let animation_flag = (header2 & 0x80) != 0;

        let color_space_name = match color_space {
            0 => "YCbCr BT.601",
            1 => "RGB",
            2 => "YCgCo",
            3 => "YCbCr BT.709",
            4 => "YCbCr BT.2020",
            5 => "YCbCr BT.2020 constant luminance",
            6 => "CMYK",
            _ => "Reserved",
        };
        metadata.insert(
            "ColorSpace".to_string(),
            TagValue::String(color_space_name.to_string()),
        );

        if alpha2_flag {
            metadata.insert(
                "AlphaPlane".to_string(),
                TagValue::String("Separate".to_string()),
            );
        }

        if limited_range_flag {
            metadata.insert(
                "ColorRange".to_string(),
                TagValue::String("Limited".to_string()),
            );
        }

        if animation_flag {
            metadata.insert(
                "IsAnimated".to_string(),
                TagValue::String("Yes".to_string()),
            );
        }

        pos += 1;

        // Parse ue7-encoded width
        let (width, new_pos) = Self::read_ue7(data, pos)?;
        pos = new_pos;
        metadata.insert("ImageWidth".to_string(), TagValue::Integer(width as i64));

        // Parse ue7-encoded height
        let (height, new_pos) = Self::read_ue7(data, pos)?;
        pos = new_pos;
        metadata.insert("ImageHeight".to_string(), TagValue::Integer(height as i64));

        // Parse picture data length
        let (_picture_length, new_pos) = Self::read_ue7(data, pos)?;
        pos = new_pos;

        // If extension data is present, parse it
        if extension_present_flag {
            let (extension_length, new_pos) = Self::read_ue7(data, pos)?;
            pos = new_pos;

            if extension_length > 0 && pos + extension_length <= data.len() {
                Self::parse_extension_data(&data[pos..pos + extension_length], metadata);
            }
        }

        Ok(pos)
    }

    /// Read ue7 variable-length encoded integer
    /// ue7 encoding: 7 bits per byte, MSB indicates continuation
    fn read_ue7(data: &[u8], mut pos: usize) -> std::result::Result<(usize, usize), String> {
        if pos >= data.len() {
            return Err("Unexpected end of data reading ue7".to_string());
        }

        let mut value = 0usize;
        let mut shift = 0;

        loop {
            if pos >= data.len() {
                return Err("Unexpected end of data in ue7".to_string());
            }

            let byte = data[pos] as usize;
            pos += 1;

            value |= (byte & 0x7F) << shift;
            shift += 7;

            // If MSB is 0, we're done
            if (byte & 0x80) == 0 {
                break;
            }

            // Prevent infinite loops and overflow
            if shift >= 35 {
                return Err("ue7 value too large".to_string());
            }
        }

        Ok((value, pos))
    }

    /// Parse extension data for EXIF and other metadata
    fn parse_extension_data(data: &[u8], metadata: &mut MetadataMap) {
        let mut pos = 0;

        while pos + 8 <= data.len() {
            // Extension tag: 4 bytes (big-endian)
            let tag_reader = EndianReader::big_endian(&data[pos..]);
            let tag = tag_reader.u32_at(0).unwrap_or(0);
            pos += 4;

            // Extension length: ue7
            if let Ok((length, new_pos)) = Self::read_ue7(data, pos) {
                pos = new_pos;

                if pos + length > data.len() {
                    break;
                }

                let ext_data = &data[pos..pos + length];

                match tag {
                    0x45584946 => {
                        // "EXIF"
                        Self::parse_exif_data(ext_data, metadata);
                    }
                    0x49434350 => {
                        // "ICCP"
                        metadata.insert(
                            "HasICCProfile".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    0x584D5020 => {
                        // "XMP "
                        if let Ok(xmp_str) = std::str::from_utf8(ext_data) {
                            Self::parse_xmp_data(xmp_str, metadata);
                        }
                    }
                    _ => {}
                }

                pos += length;
            } else {
                break;
            }
        }
    }

    /// Parse embedded EXIF data
    fn parse_exif_data(tiff_data: &[u8], metadata: &mut MetadataMap) {
        if tiff_data.len() < 8 {
            return;
        }

        // Detect byte order
        let byte_order = match &tiff_data[0..2] {
            b"II" => ByteOrder::LittleEndian,
            b"MM" => ByteOrder::BigEndian,
            _ => return,
        };

        // Create EndianReader with appropriate byte order
        let endian_order = match byte_order {
            ByteOrder::LittleEndian => EndianByteOrder::Little,
            ByteOrder::BigEndian => EndianByteOrder::Big,
        };
        let header_reader = EndianReader::new(tiff_data, endian_order);

        // Verify TIFF magic
        let magic = header_reader.u16_at(2).unwrap_or(0);
        if magic != 0x002A {
            return;
        }

        // Get IFD0 offset
        let ifd0_offset = header_reader.u32_at(4).unwrap_or(0);

        // Create a BufferedReader from the TIFF data
        let reader = BufferedReader::from_bytes(tiff_data);

        // Parse IFD0
        if let Ok(entries) = parse_ifd(&reader, ifd0_offset as u64, byte_order) {
            for (tag_id, field_type, value_count, raw_bytes) in &entries {
                let tag_name = lookup_tag_name(*tag_id, "IFD0");
                let value = raw_bytes_to_tag_value(
                    raw_bytes.as_ref(),
                    *field_type,
                    *value_count,
                    *tag_id,
                    byte_order,
                );
                metadata.insert(tag_name, value);

                // Check for ExifIFD pointer (tag 0x8769)
                if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
                    let tag_reader = EndianReader::new(raw_bytes, endian_order);
                    let exif_offset = tag_reader.u32_at(0).unwrap_or(0);
                    if let Ok(exif_entries) = parse_ifd(&reader, exif_offset as u64, byte_order) {
                        for (exif_tag_id, exif_field_type, exif_value_count, exif_raw_bytes) in
                            &exif_entries
                        {
                            let exif_tag_name = lookup_tag_name(*exif_tag_id, "ExifIFD");
                            let value = raw_bytes_to_tag_value(
                                exif_raw_bytes.as_ref(),
                                *exif_field_type,
                                *exif_value_count,
                                *exif_tag_id,
                                byte_order,
                            );
                            metadata.insert(exif_tag_name, value);
                        }
                    }
                }
            }
        }
    }

    /// Extract metadata from XMP using the proper RDF parser
    fn parse_xmp_data(xmp: &str, metadata: &mut MetadataMap) {
        if let Ok(xmp_tags) = parse_xmp(xmp.as_bytes()) {
            for (tag_name, value) in xmp_tags {
                metadata.insert(tag_name, TagValue::String(value));
            }
        }
    }
}

impl FormatParser for BPGParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid BPG signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("BPG".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::Integer(reader.size() as i64),
        );

        // Read header and extension data (first 1KB should be enough for most cases)
        let header_size = (reader.size() as usize).min(1024);
        let header_data = reader.read(0, header_size)?;

        let _ = Self::parse_header(header_data, &mut metadata);

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::BPG)
    }
}

/// Parses metadata from BPG files.
///
/// This is a convenience wrapper around BPGParser that provides a functional API.
pub fn parse_bpg_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = BPGParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Converts raw bytes to TagValue
fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    _value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;

    // Create EndianReader with appropriate byte order
    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let reader = EndianReader::new(bytes, endian_order);

    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            ExifType::Ascii => {
                let text = String::from_utf8_lossy(bytes);
                return TagValue::String(text.trim_end_matches('\0').to_string());
            }
            ExifType::Short if bytes.len() >= 2 => {
                let value = reader.u16_at(0).unwrap_or(0);
                return TagValue::Integer(value as i64);
            }
            ExifType::Long if bytes.len() >= 4 => {
                let value = reader.u32_at(0).unwrap_or(0);
                return TagValue::Integer(value as i64);
            }
            ExifType::Rational if bytes.len() >= 8 => {
                if let Some((num, den)) = reader.rational_at(0) {
                    if den == 1 {
                        return TagValue::Integer(num as i64);
                    }
                    return TagValue::Rational {
                        numerator: num as i32,
                        denominator: den as i32,
                    };
                }
            }
            ExifType::Undefined => {
                // Special handling for ExifVersion
                if tag_id == 0x9000 && bytes.len() >= 4 {
                    let version = String::from_utf8_lossy(&bytes[0..4]);
                    return TagValue::String(version.to_string());
                }
                return TagValue::Binary(bytes.to_vec());
            }
            _ => {}
        }
    }

    // Fallback: try ASCII
    if bytes.iter().all(|&b| b.is_ascii() || b == 0) {
        let text = String::from_utf8_lossy(bytes);
        TagValue::String(text.trim_end_matches('\0').to_string())
    } else {
        TagValue::Binary(bytes.to_vec())
    }
}
