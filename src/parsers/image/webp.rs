//! WebP image format parser
//!
//! WebP uses RIFF container with chunks:
//! - VP8/VP8L/VP8X: Image data
//! - EXIF: TIFF/EXIF metadata
//! - ICCP: ICC color profile
//! - XMP: XMP metadata

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::{ByteOrder as EndianByteOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::parsers::xmp::rdf_parser::parse_xmp;
use crate::tag_db::lookup_tag_name;
use std::io;

/// WebP signature: "RIFF" + size + "WEBP"
const RIFF_SIGNATURE: &[u8] = b"RIFF";
const WEBP_SIGNATURE: &[u8] = b"WEBP";

/// WebP parser
pub struct WebPParser;

impl WebPParser {
    /// Verifies WebP signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        let header = reader.read(0, 12)?;
        Ok(&header[0..4] == RIFF_SIGNATURE && &header[8..12] == WEBP_SIGNATURE)
    }
}

impl FormatParser for WebPParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid WebP signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("WebP".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Parse RIFF chunks to find EXIF, XMP, and VP8X data
        parse_webp_chunks(reader, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::WebP)
    }
}

/// Parses metadata from WebP files.
///
/// This is a convenience wrapper around WebPParser that provides a functional API.
pub fn parse_webp_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = WebPParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Parse RIFF chunks in WebP file
fn parse_webp_chunks(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    let file_size = reader.size();

    // Skip RIFF header (12 bytes: "RIFF" + size + "WEBP")
    let mut offset = 12u64;

    while offset + 8 <= file_size {
        // Read chunk header: FourCC (4 bytes) + size (4 bytes, little-endian)
        let chunk_header = reader.read(offset, 8)?;
        let chunk_type = &chunk_header[0..4];
        // WebP/RIFF uses little-endian byte order
        let header_reader = EndianReader::little_endian(chunk_header);
        let chunk_size = header_reader.u32_at(4).unwrap_or(0) as u64;

        // Move past header
        let chunk_data_offset = offset + 8;

        match chunk_type {
            b"VP8X" => {
                // Extended WebP header with flags
                if chunk_size >= 10 {
                    let vp8x_data = reader.read(chunk_data_offset, 10)?;
                    let vp8x_reader = EndianReader::little_endian(vp8x_data);
                    let flags = vp8x_reader.u8_at(0).unwrap_or(0);

                    // Extract image dimensions (24-bit values, little-endian)
                    let width = (vp8x_reader.u32_at(4).unwrap_or(0) & 0x00FFFFFF) + 1;
                    let height = (vp8x_reader.u32_at(7).unwrap_or(0) & 0x00FFFFFF) + 1;

                    metadata.insert(
                        "WebP:ImageWidth".to_string(),
                        TagValue::Integer(width as i64),
                    );
                    metadata.insert(
                        "WebP:ImageHeight".to_string(),
                        TagValue::Integer(height as i64),
                    );

                    // Build WebP_Flags string like ExifTool
                    // VP8X flags (from ExifTool RIFF.pm):
                    //   Bit 1 (0x02) = Animation
                    //   Bit 2 (0x04) = XMP
                    //   Bit 3 (0x08) = EXIF
                    //   Bit 4 (0x10) = Alpha
                    //   Bit 5 (0x20) = ICC Profile
                    // ExifTool outputs in order: XMP, EXIF, Alpha, Animation
                    // (ICC is not included in WebP_Flags, it's a separate Has tag)
                    let mut flag_parts = Vec::new();
                    if flags & 0x04 != 0 {
                        flag_parts.push("XMP");
                    }
                    if flags & 0x08 != 0 {
                        flag_parts.push("EXIF");
                    }
                    if flags & 0x10 != 0 {
                        flag_parts.push("Alpha");
                    }
                    if flags & 0x02 != 0 {
                        flag_parts.push("Animation");
                    }
                    if !flag_parts.is_empty() {
                        metadata.insert(
                            "WebP:WebP_Flags".to_string(),
                            TagValue::String(flag_parts.join(", ")),
                        );
                    }

                    // Keep individual flags for compatibility
                    // Note: Bit 5 (0x20) = ICC, Bit 4 (0x10) = Alpha
                    if flags & 0x20 != 0 {
                        metadata.insert(
                            "WebP:HasICCP".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    if flags & 0x10 != 0 {
                        metadata.insert(
                            "WebP:HasAlpha".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    if flags & 0x08 != 0 {
                        metadata.insert(
                            "WebP:HasEXIF".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    if flags & 0x04 != 0 {
                        metadata.insert(
                            "WebP:HasXMP".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    if flags & 0x02 != 0 {
                        metadata.insert(
                            "WebP:IsAnimation".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                }
            }
            b"VP8 " => {
                // Lossy VP8 bitstream - extract dimensions from frame header
                if chunk_size >= 10 {
                    let vp8_data = reader.read(chunk_data_offset, 10)?;
                    let vp8_reader = EndianReader::little_endian(vp8_data);
                    // VP8 frame header starts with 3-byte frame tag
                    let frame_tag = vp8_reader.u8_at(0).unwrap_or(1);
                    // Check if this is a keyframe
                    if frame_tag & 0x01 == 0 {
                        // Extract VP8 version (bits 1-3 of frame tag)
                        let version = (frame_tag >> 1) & 0x07;
                        let version_str = match version {
                            0 => "0 (bicubic reconstruction, normal loop)",
                            1 => "1 (bilinear reconstruction, simple loop)",
                            2 => "2 (bilinear reconstruction, no loop)",
                            3 => "3 (no reconstruction, no loop)",
                            _ => "Unknown",
                        };
                        metadata.insert(
                            "WebP:VP8Version".to_string(),
                            TagValue::String(version_str.to_string()),
                        );

                        // Keyframe - dimensions at bytes 6-9
                        let width_data = vp8_reader.u16_at(6).unwrap_or(0);
                        let height_data = vp8_reader.u16_at(8).unwrap_or(0);
                        let width = width_data & 0x3FFF;
                        let height = height_data & 0x3FFF;

                        // Extract scale factors (upper 2 bits)
                        let horizontal_scale = (width_data >> 14) & 0x03;
                        let vertical_scale = (height_data >> 14) & 0x03;

                        metadata.insert(
                            "WebP:HorizontalScale".to_string(),
                            TagValue::Integer(horizontal_scale as i64),
                        );
                        metadata.insert(
                            "WebP:VerticalScale".to_string(),
                            TagValue::Integer(vertical_scale as i64),
                        );

                        if !metadata.contains_key("WebP:ImageWidth") {
                            metadata.insert(
                                "WebP:ImageWidth".to_string(),
                                TagValue::Integer(width as i64),
                            );
                            metadata.insert(
                                "WebP:ImageHeight".to_string(),
                                TagValue::Integer(height as i64),
                            );
                        }
                    }
                }
            }
            b"VP8L" => {
                // Lossless VP8L bitstream
                if chunk_size >= 5 {
                    let vp8l_data = reader.read(chunk_data_offset, 5)?;
                    let vp8l_reader = EndianReader::little_endian(vp8l_data);
                    // Check signature byte (0x2F)
                    if vp8l_reader.u8_at(0).unwrap_or(0) == 0x2F {
                        // Dimensions are packed in bytes 1-4
                        let bits = vp8l_reader.u32_at(1).unwrap_or(0);
                        let width = (bits & 0x3FFF) + 1;
                        let height = ((bits >> 14) & 0x3FFF) + 1;

                        if !metadata.contains_key("WebP:ImageWidth") {
                            metadata.insert(
                                "WebP:ImageWidth".to_string(),
                                TagValue::Integer(width as i64),
                            );
                            metadata.insert(
                                "WebP:ImageHeight".to_string(),
                                TagValue::Integer(height as i64),
                            );
                        }
                    }
                }
            }
            b"EXIF" => {
                // EXIF metadata - contains TIFF/EXIF data
                if chunk_size > 0 && chunk_data_offset + chunk_size <= file_size {
                    let exif_data = reader.read(chunk_data_offset, chunk_size as usize)?;
                    if parse_webp_exif(exif_data, metadata).is_err() {
                        // Silently ignore EXIF parsing errors
                    }
                }
            }
            b"XMP " => {
                // XMP metadata - XML format
                if chunk_size > 0 && chunk_data_offset + chunk_size <= file_size {
                    let xmp_data = reader.read(chunk_data_offset, chunk_size as usize)?;

                    // Parse the XMP and extract metadata
                    if let Ok(xmp_props) = parse_xmp(xmp_data) {
                        for (name, value) in xmp_props {
                            metadata.insert(name, TagValue::String(value));
                        }
                    }
                }
            }
            b"ICCP" => {
                // ICC color profile
                metadata.insert(
                    "WebP:ICCProfileSize".to_string(),
                    TagValue::Integer(chunk_size as i64),
                );
            }
            b"ALPH" => {
                // Alpha chunk - contains alpha compression info
                // WebP spec byte layout: |Rsv|P|F|C| where:
                //   C (Compression): bits 1-0
                //   F (Filtering): bits 3-2
                //   P (Preprocessing): bits 5-4
                //   Rsv (Reserved): bits 7-6
                //
                // Note: ExifTool's RIFF.pm has a bug - all three tags use Mask 0x03
                // without BitShift, so they all extract the same bits 0-1 from the byte.
                // We match ExifTool's buggy behavior for compatibility.
                if chunk_size >= 1 {
                    let alph_data = reader.read(chunk_data_offset, 1)?;
                    let flags = alph_data[0];

                    // All three use bits 0-1 to match ExifTool's bug
                    let value = flags & 0x03;

                    let preprocessing_str = match value {
                        0 => "none",
                        1 => "Level Reduction",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "RIFF:AlphaPreprocessing".to_string(),
                        TagValue::String(preprocessing_str.to_string()),
                    );

                    let filtering_str = match value {
                        0 => "none",
                        1 => "Horizontal",
                        2 => "Vertical",
                        3 => "Gradient",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "RIFF:AlphaFiltering".to_string(),
                        TagValue::String(filtering_str.to_string()),
                    );

                    let compression_str = match value {
                        0 => "none",
                        1 => "Lossless",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "RIFF:AlphaCompression".to_string(),
                        TagValue::String(compression_str.to_string()),
                    );
                }
            }
            b"ANIM" => {
                // Animation control chunk
                if chunk_size >= 6 {
                    let anim_data = reader.read(chunk_data_offset, 6)?;
                    let anim_reader = EndianReader::little_endian(anim_data);

                    // Background color (4 bytes ARGB)
                    let bg_color = anim_reader.u32_at(0).unwrap_or(0);

                    // Loop count (2 bytes) - 0 means infinite
                    let loop_count = anim_reader.u16_at(4).unwrap_or(0);

                    metadata.insert(
                        "WebP:AnimationBackgroundColor".to_string(),
                        TagValue::String(format!("0x{:08X}", bg_color)),
                    );

                    if loop_count == 0 {
                        metadata.insert(
                            "WebP:AnimationLoopCount".to_string(),
                            TagValue::String("Infinite".to_string()),
                        );
                    } else {
                        metadata.insert(
                            "WebP:AnimationLoopCount".to_string(),
                            TagValue::Integer(loop_count as i64),
                        );
                    }
                }
            }
            b"ANMF" => {
                // Animation frame chunk - just count them
                if !metadata.contains_key("WebP:AnimationFrameCount") {
                    metadata.insert("WebP:AnimationFrameCount".to_string(), TagValue::Integer(1));
                } else if let Some(TagValue::Integer(count)) =
                    metadata.get("WebP:AnimationFrameCount")
                {
                    metadata.insert(
                        "WebP:AnimationFrameCount".to_string(),
                        TagValue::Integer(count + 1),
                    );
                }
            }
            _ => {
                // Skip unknown chunks
            }
        }

        // Move to next chunk (chunks are padded to even byte boundary)
        offset = chunk_data_offset + chunk_size;
        if !chunk_size.is_multiple_of(2) {
            offset += 1; // Padding byte
        }
    }

    Ok(())
}

/// Parse EXIF data from WebP EXIF chunk
fn parse_webp_exif(exif_data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if exif_data.len() < 8 {
        return Err(ExifToolError::parse_error("EXIF data too short"));
    }

    // WebP EXIF chunk can start with "Exif\0\0" header (like JPEG) or directly with TIFF header
    let tiff_data = if exif_data.len() >= 6 && &exif_data[0..4] == b"Exif" {
        // Skip "Exif\0\0" header
        &exif_data[6..]
    } else {
        exif_data
    };

    if tiff_data.len() < 8 {
        return Err(ExifToolError::parse_error("TIFF data too short"));
    }

    // Detect byte order
    let byte_order = match &tiff_data[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return Err(ExifToolError::parse_error("Invalid TIFF byte order")),
    };

    // Create EndianReader for TIFF header parsing
    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let header_reader = EndianReader::new(tiff_data, endian_order);

    // Verify TIFF magic
    let magic = header_reader.u16_at(2).unwrap_or(0);
    if magic != 0x002A {
        return Err(ExifToolError::parse_error("Invalid TIFF magic number"));
    }

    // Get IFD0 offset
    let ifd_offset = header_reader.u32_at(4).unwrap_or(0);

    // Create in-memory reader
    let exif_reader = WebPExifReader::new(tiff_data.to_vec());

    // Track sub-IFD offsets
    let mut exif_ifd_offset = None;
    let mut gps_ifd_offset = None;

    // Parse IFD0
    if let Ok(ifd0_tags) = parse_ifd(&exif_reader, ifd_offset as u64, byte_order) {
        for (tag_id, field_type, value_count, raw_bytes) in &ifd0_tags {
            let tag_reader = EndianReader::new(raw_bytes, endian_order);

            // Check for ExifIFD pointer
            if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
                let offset = tag_reader.u32_at(0).unwrap_or(0);
                exif_ifd_offset = Some(offset as u64);
                continue;
            }

            // Check for GPS pointer
            if *tag_id == 0x8825 && raw_bytes.len() >= 4 {
                let offset = tag_reader.u32_at(0).unwrap_or(0);
                gps_ifd_offset = Some(offset as u64);
                continue;
            }

            let tag_name = lookup_tag_name(*tag_id, "IFD0");
            let tag_value =
                raw_bytes_to_tag_value(raw_bytes, *field_type, *value_count, byte_order);
            metadata.insert(tag_name, tag_value);
        }
    }

    // Parse ExifIFD
    if let Some(offset) = exif_ifd_offset
        && let Ok(exif_tags) = parse_ifd(&exif_reader, offset, byte_order)
    {
        for (tag_id, field_type, value_count, raw_bytes) in exif_tags {
            let tag_name = lookup_tag_name(tag_id, "ExifIFD");
            let tag_value = raw_bytes_to_tag_value(&raw_bytes, field_type, value_count, byte_order);
            metadata.insert(tag_name, tag_value);
        }
    }

    // Parse GPS IFD
    if let Some(offset) = gps_ifd_offset
        && let Ok(gps_tags) = parse_ifd(&exif_reader, offset, byte_order)
    {
        for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
            let tag_name = lookup_tag_name(tag_id, "GPS");
            let tag_value = raw_bytes_to_tag_value(&raw_bytes, field_type, value_count, byte_order);
            metadata.insert(tag_name, tag_value);
        }
    }

    Ok(())
}

/// In-memory FileReader for WebP EXIF data
struct WebPExifReader {
    data: Vec<u8>,
}

impl WebPExifReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for WebPExifReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start + length;
        if end > self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "read beyond end of EXIF data",
            ));
        }
        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

/// Convert raw EXIF bytes to TagValue
fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
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
            ExifType::Byte if !bytes.is_empty() => {
                if value_count == 1 {
                    return TagValue::Integer(reader.u8_at(0).unwrap_or(0) as i64);
                }
                return TagValue::Binary(bytes.to_vec());
            }
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
                if let Some((num, den)) = reader.rational_at(0)
                    && den != 0
                {
                    return TagValue::Float(num as f64 / den as f64);
                }
            }
            ExifType::Undefined => {
                if bytes
                    .iter()
                    .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace() || b == 0)
                {
                    let text = String::from_utf8_lossy(bytes);
                    let trimmed = text.trim_end_matches('\0');
                    if !trimmed.is_empty() {
                        return TagValue::String(trimmed.to_string());
                    }
                }
                return TagValue::Binary(bytes.to_vec());
            }
            ExifType::SRational if bytes.len() >= 8 => {
                if let Some((num, den)) = reader.srational_at(0)
                    && den != 0
                {
                    return TagValue::Float(num as f64 / den as f64);
                }
            }
            _ => {}
        }
    }
    TagValue::Binary(bytes.to_vec())
}
