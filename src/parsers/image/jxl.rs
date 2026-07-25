//! JPEG XL (JXL) image format parser
//!
//! JPEG XL supports two formats:
//! - Bare codestream: starts with 0xFF 0x0A
//! - Container format: ISOBMFF-based boxes starting with "JXL " signature
//!
//! Container boxes include: jxlc (codestream), jxlp (partial), Exif, xml (XMP)

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::buffered_reader::BufferedReader;
use crate::io::{ByteOrder as EndianByteOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::parsers::xmp::rdf_parser::parse_xmp;
use crate::tag_db::lookup_tag_name;

/// Bare codestream signature: 0xFF 0x0A
const JXL_CODESTREAM_SIGNATURE: &[u8] = &[0xFF, 0x0A];
/// Container signature: size (4) + "JXL " (4) + ftyp header
const JXL_CONTAINER_SIGNATURE: &[u8] = b"JXL ";

/// Parser for JPEG XL (JXL) next-generation image files
///
/// Extracts metadata from JPEG XL format images including dimensions, bit depth,
/// color information, and embedded EXIF/XMP data.
pub struct JXLParser;

impl JXLParser {
    /// Verifies the JPEG XL file signature (supports both bare codestream and container formats)
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 2 {
            return Ok(false);
        }
        let header = reader.read(0, 2)?;
        if header == JXL_CODESTREAM_SIGNATURE {
            return Ok(true);
        }
        if reader.size() >= 12 {
            let header_long = reader.read(0, 12)?;
            // Container format: first 4 bytes are size, next 4 are "JXL "
            if &header_long[4..8] == JXL_CONTAINER_SIGNATURE {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Checks if file is container format (ISOBMFF-based)
    fn is_container_format(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        let header = reader.read(0, 12)?;
        Ok(&header[4..8] == JXL_CONTAINER_SIGNATURE)
    }

    /// Parse bare codestream header for dimensions
    fn parse_codestream_header(
        data: &[u8],
        metadata: &mut MetadataMap,
    ) -> std::result::Result<(), String> {
        if data.len() < 2 || data[0] != 0xFF || data[1] != 0x0A {
            return Err("Invalid codestream header".to_string());
        }

        // Skip signature (2 bytes)
        let mut pos = 2;
        if data.len() < pos + 1 {
            return Ok(());
        }

        // Parse SizeHeader using variable-length encoding
        // First byte contains flags
        let size_header = data[pos];
        let small = (size_header & 0x01) != 0;

        if small {
            // Small image: 5 bits height div 8, 5 bits width div 8
            if data.len() < pos + 2 {
                return Ok(());
            }
            let h5 = ((size_header >> 1) & 0x1F) as u32;
            let w5 = (((size_header >> 6) & 0x03) | ((data[pos + 1] & 0x07) << 2)) as u32;
            let height = (h5 + 1) * 8;
            let width = (w5 + 1) * 8;
            metadata.insert("ImageWidth".to_string(), TagValue::Integer(width as i64));
            metadata.insert("ImageHeight".to_string(), TagValue::Integer(height as i64));
        } else {
            // Large image: parse variable-length integers
            pos += 1;
            if let Some((height, new_pos)) = Self::read_u32_varint(data, pos) {
                pos = new_pos;
                if let Some((width, _)) = Self::read_u32_varint(data, pos) {
                    // Height and width are encoded as (value + 1)
                    metadata.insert(
                        "ImageHeight".to_string(),
                        TagValue::Integer((height + 1) as i64),
                    );
                    metadata.insert(
                        "ImageWidth".to_string(),
                        TagValue::Integer((width + 1) as i64),
                    );
                }
            }
        }

        Ok(())
    }

    /// Read variable-length u32 (JXL encoding)
    fn read_u32_varint(data: &[u8], pos: usize) -> Option<(u32, usize)> {
        if pos >= data.len() {
            return None;
        }

        let selector = data[pos] & 0x03;
        match selector {
            0 => {
                // 0-3: 2 bits
                Some((((data[pos] >> 2) & 0x03) as u32, pos + 1))
            }
            1 => {
                // 4-19: 4 bits + 4
                if pos + 1 >= data.len() {
                    return None;
                }
                let val = ((data[pos] >> 2) & 0x0F) as u32 + 4;
                Some((val, pos + 1))
            }
            2 => {
                // 20-275: 8 bits + 20
                if pos + 1 >= data.len() {
                    return None;
                }
                let val = ((data[pos] >> 2) as u32) | ((((data[pos + 1] & 0x3F) as u32) << 6) + 20);
                Some((val, pos + 2))
            }
            3 => {
                // 276+: 12-28 bits
                if pos + 3 >= data.len() {
                    return None;
                }
                let val = ((data[pos] >> 2) as u32)
                    | ((data[pos + 1] as u32) << 6)
                    | ((data[pos + 2] as u32) << 14)
                    | ((((data[pos + 3] & 0x0F) as u32) << 22) + 276);
                Some((val, pos + 4))
            }
            _ => None,
        }
    }

    /// Decode ISOBMFF brand code to human-readable name
    fn decode_brand(brand: &[u8]) -> String {
        match brand {
            b"jxl " => "JPEG XL Image (.JXL)".to_string(),
            b"avif" => "AV1 Image File Format".to_string(),
            b"heic" => "HEIC Image".to_string(),
            b"mif1" => "HEIF Image".to_string(),
            b"msf1" => "HEIF Image Sequence".to_string(),
            b"mp41" => "MP4 v1".to_string(),
            b"mp42" => "MP4 v2".to_string(),
            b"isom" => "ISO Base Media".to_string(),
            b"jp2 " => "JPEG 2000 Image (.JP2)".to_string(),
            _ => {
                // Return the 4-char code as string
                String::from_utf8_lossy(brand).trim().to_string()
            }
        }
    }

    /// Parse ISOBMFF container format boxes
    fn parse_container_boxes(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        let file_size = reader.size() as usize;
        let mut offset = 0usize;

        while offset + 8 <= file_size {
            let header = reader.read(offset as u64, 8)?;
            // ISOBMFF uses big-endian byte order
            let header_reader = EndianReader::big_endian(header);
            let box_size = header_reader.u32_at(0).unwrap_or(0) as usize;
            let box_type = std::str::from_utf8(&header[4..8]).unwrap_or("????");

            if box_size == 0 {
                break; // Box extends to end of file
            }
            if box_size < 8 || offset + box_size > file_size {
                break;
            }

            match box_type {
                "ftyp" => {
                    // File Type box - contains brand information
                    // Format: major_brand (4) + minor_version (4) + compatible_brands (4 each)
                    if box_size >= 16 {
                        let ftyp_data = reader.read((offset + 8) as u64, box_size - 8)?;
                        let ftyp_reader = EndianReader::big_endian(&*ftyp_data);

                        // Major brand (4 bytes)
                        if ftyp_data.len() >= 4 {
                            let major_brand = Self::decode_brand(&ftyp_data[0..4]);
                            metadata.insert(
                                "Jpeg2000:MajorBrand".to_string(),
                                TagValue::new_string(major_brand),
                            );
                        }

                        // Minor version (4 bytes as version number)
                        if ftyp_data.len() >= 8 {
                            let minor = ftyp_reader.u32_at(4).unwrap_or(0);
                            // Format as X.X.X (major.minor.patch from 32-bit value)
                            let major_ver = (minor >> 24) & 0xFF;
                            let minor_ver = (minor >> 16) & 0xFF;
                            let patch_ver = minor & 0xFFFF;
                            metadata.insert(
                                "Jpeg2000:MinorVersion".to_string(),
                                TagValue::new_string(format!(
                                    "{}.{}.{}",
                                    major_ver, minor_ver, patch_ver
                                )),
                            );
                        }

                        // Compatible brands (remaining 4-byte chunks)
                        if ftyp_data.len() > 8 {
                            let mut brands: Vec<String> = Vec::new();
                            let mut brand_offset = 8;
                            while brand_offset + 4 <= ftyp_data.len() {
                                let brand = &ftyp_data[brand_offset..brand_offset + 4];
                                let brand_str = String::from_utf8_lossy(brand).to_string();
                                brands.push(format!("\"{}\"", brand_str));
                                brand_offset += 4;
                            }
                            if !brands.is_empty() {
                                metadata.insert(
                                    "Jpeg2000:CompatibleBrands".to_string(),
                                    TagValue::new_string(format!("[{}]", brands.join(", "))),
                                );
                            }
                        }
                    }
                }
                "jxlc" | "jxlp" => {
                    // Codestream box - parse for dimensions
                    let content_offset = if box_type == "jxlp" { 12 } else { 8 };
                    if offset + content_offset < file_size {
                        let content_size = box_size.saturating_sub(content_offset);
                        let max_read = content_size.min(64); // Only need header
                        if max_read > 0 {
                            let content =
                                reader.read((offset + content_offset) as u64, max_read)?;
                            let _ = Self::parse_codestream_header(content, metadata);
                        }
                    }
                }
                "Exif" => {
                    // EXIF box: 4-byte offset + TIFF data
                    if box_size > 12 {
                        let exif_data = reader.read((offset + 8) as u64, box_size - 8)?;
                        if exif_data.len() >= 10 {
                            // Skip 4-byte offset prefix
                            let tiff_data = &exif_data[4..];
                            Self::parse_exif_data(tiff_data, metadata);
                        }
                    }
                }
                "xml " => {
                    // XMP box
                    if box_size > 8 {
                        let xmp_data = reader.read((offset + 8) as u64, box_size - 8)?;
                        if let Ok(xmp_str) = std::str::from_utf8(xmp_data) {
                            // Extract basic XMP metadata
                            Self::parse_xmp_data(xmp_str, metadata);
                        }
                    }
                }
                "jxll" => {
                    // Level box - indicates feature level
                    if box_size >= 12 {
                        let level_data = reader.read((offset + 8) as u64, 4)?;
                        let level = level_data[0];
                        metadata.insert("JXLLevel".to_string(), TagValue::Integer(level as i64));
                    }
                }
                _ => {}
            }

            offset += box_size;
        }

        Ok(())
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

                // Check for GPS IFD pointer (tag 0x8825)
                if *tag_id == 0x8825 && raw_bytes.len() >= 4 {
                    let tag_reader = EndianReader::new(raw_bytes, endian_order);
                    let gps_offset = tag_reader.u32_at(0).unwrap_or(0);
                    if let Ok(gps_entries) = parse_ifd(&reader, gps_offset as u64, byte_order) {
                        for (gps_tag_id, gps_field_type, gps_value_count, gps_raw_bytes) in
                            &gps_entries
                        {
                            let gps_tag_name = lookup_tag_name(*gps_tag_id, "GPS");
                            let value = raw_bytes_to_tag_value(
                                gps_raw_bytes.as_ref(),
                                *gps_field_type,
                                *gps_value_count,
                                *gps_tag_id,
                                byte_order,
                            );
                            metadata.insert(gps_tag_name, value);
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

impl FormatParser for JXLParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid JXL signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("JXL".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::Integer(reader.size() as i64),
        );

        if Self::is_container_format(reader)? {
            // Container format (ISOBMFF-based)
            metadata.insert(
                "JXLFormat".to_string(),
                TagValue::String("Container".to_string()),
            );
            Self::parse_container_boxes(reader, &mut metadata)?;
        } else {
            // Bare codestream
            metadata.insert(
                "JXLFormat".to_string(),
                TagValue::String("Codestream".to_string()),
            );
            // Read codestream header (first 64 bytes should be enough)
            let header_size = (reader.size() as usize).min(64);
            let header = reader.read(0, header_size)?;
            let _ = Self::parse_codestream_header(header, &mut metadata);
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::JXL)
    }
}

/// Parses metadata from JPEG XL files.
///
/// This is a convenience wrapper around JXLParser that provides a functional API.
pub fn parse_jxl_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = JXLParser;
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
                // ComponentsConfiguration
                if tag_id == 0x9101 && bytes.len() >= 4 {
                    let components: Vec<&str> = bytes
                        .iter()
                        .take(4)
                        .map(|&b| match b {
                            0 => "-",
                            1 => "Y",
                            2 => "Cb",
                            3 => "Cr",
                            4 => "R",
                            5 => "G",
                            6 => "B",
                            _ => "?",
                        })
                        .collect();
                    return TagValue::String(components.join(", "));
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
