//! FLIF (Free Lossless Image Format) parser
//!
//! FLIF format structure:
//! - Magic: 4 bytes "FLIF"
//! - Header byte: interlaced(4 bits) + animated(1 bit) + channels(2 bits) + bytes_per_channel(1 bit)
//! - Width: varint encoding
//! - Height: varint encoding
//! - Frame count: varint (if animated)
//! - Metadata chunks: iCCP, eXif, eXmp (optional)

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::{ByteOrder as EndianByteOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use std::io;

const FLIF_SIGNATURE: &[u8] = b"FLIF";

/// Parser for FLIF (Free Lossless Image Format) files
///
/// Extracts metadata from FLIF format images including dimensions, color type, bit depth,
/// interlacing, animation, and embedded EXIF data.
pub struct FLIFParser;

impl FLIFParser {
    /// Verifies the FLIF file signature ("FLIF")
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(header == FLIF_SIGNATURE)
    }
}

impl FormatParser for FLIFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid FLIF signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("FLIF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Parse FLIF header and metadata chunks
        parse_flif_header(reader, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::FLIF)
    }
}

/// Parse FLIF header and extract metadata
fn parse_flif_header(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    if reader.size() < 6 {
        return Err(ExifToolError::parse_error("FLIF file too short"));
    }

    // Read header byte at offset 4
    let header_data = reader.read(4, 10)?; // Read enough for header + varints
    let header_byte = header_data[0];

    // Parse header byte: IIIIIACT
    // I = interlacing (4 bits), A = animated (1 bit), C = channels (2 bits), T = bytes_per_channel (1 bit)
    let interlaced = (header_byte >> 4) & 0x0F;
    let animated = (header_byte >> 3) & 0x01;
    let channels = (header_byte >> 1) & 0x03;
    let bytes_per_channel = header_byte & 0x01;

    // Decode color type from channels
    let color_type = match channels {
        0 => "Grayscale",
        2 => "RGB",
        3 => "RGBA",
        _ => return Err(ExifToolError::parse_error("Invalid FLIF channels value")),
    };
    metadata.insert(
        "FLIF:ColorType".to_string(),
        TagValue::String(color_type.to_string()),
    );

    // Decode bit depth
    let bit_depth = if bytes_per_channel == 0 { 8 } else { 16 };
    metadata.insert("FLIF:BitDepth".to_string(), TagValue::Integer(bit_depth));

    // Decode interlacing
    if interlaced > 0 {
        metadata.insert(
            "FLIF:Interlaced".to_string(),
            TagValue::String("Yes".to_string()),
        );
    }

    // Decode animation flag
    let is_animated = animated == 1;
    if is_animated {
        metadata.insert(
            "FLIF:Animated".to_string(),
            TagValue::String("Yes".to_string()),
        );
    }

    // Parse varint-encoded width and height
    let mut offset = 5u64; // Start after magic + header byte
    let (width, width_bytes) = read_varint(reader, offset)?;
    offset += width_bytes;
    let (height, height_bytes) = read_varint(reader, offset)?;
    offset += height_bytes;

    metadata.insert(
        "FLIF:ImageWidth".to_string(),
        TagValue::Integer(width as i64),
    );
    metadata.insert(
        "FLIF:ImageHeight".to_string(),
        TagValue::Integer(height as i64),
    );

    // Parse frame count if animated
    if is_animated {
        let (frame_count, frame_bytes) = read_varint(reader, offset)?;
        offset += frame_bytes;
        metadata.insert(
            "FLIF:FrameCount".to_string(),
            TagValue::Integer(frame_count as i64),
        );
    }

    // Look for metadata chunks
    parse_flif_metadata_chunks(reader, offset, metadata)?;

    Ok(())
}

/// Parse FLIF metadata chunks (iCCP, eXif, eXmp)
fn parse_flif_metadata_chunks(
    reader: &dyn FileReader,
    mut offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let file_size = reader.size();

    // Metadata chunks appear before image data
    // They are identified by 4-byte FourCC codes
    while offset + 8 <= file_size {
        // Try to read chunk header
        let chunk_header = match reader.read(offset, 8) {
            Ok(data) => data,
            Err(_) => break, // End of metadata section
        };

        // Check for known chunk types
        let chunk_type = &chunk_header[0..4];

        // If not a valid chunk type, we've reached image data
        if !matches!(chunk_type, b"iCCP" | b"eXif" | b"eXmp") {
            break;
        }

        // Read chunk size (4 bytes, big-endian after chunk type)
        let size_reader = EndianReader::big_endian(&chunk_header[4..8]);
        let chunk_size = size_reader.u32_at(0).unwrap_or(0) as u64;

        offset += 8;

        // Ensure chunk doesn't exceed file size
        if offset + chunk_size > file_size {
            break;
        }

        match chunk_type {
            b"eXif" => {
                // Parse EXIF metadata
                if let Ok(exif_data) = reader.read(offset, chunk_size as usize) {
                    let _ = parse_flif_exif(exif_data, metadata);
                }
            }
            b"iCCP" => {
                metadata.insert(
                    "FLIF:ICCProfileSize".to_string(),
                    TagValue::Integer(chunk_size as i64),
                );
            }
            b"eXmp" => {
                if let Ok(xmp_data) = reader.read(offset, chunk_size as usize)
                    && let Ok(xmp_str) = std::str::from_utf8(xmp_data)
                {
                    metadata.insert(
                        "XMP:RawXMP".to_string(),
                        TagValue::String(xmp_str.to_string()),
                    );
                }
            }
            _ => {}
        }

        offset += chunk_size;
    }

    Ok(())
}

/// Parse EXIF data from FLIF eXif chunk
fn parse_flif_exif(exif_data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if exif_data.len() < 8 {
        return Err(ExifToolError::parse_error("EXIF data too short"));
    }

    // Detect byte order
    let byte_order = match &exif_data[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return Err(ExifToolError::parse_error("Invalid TIFF byte order")),
    };

    // Create EndianReader with appropriate byte order
    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let tiff_reader = EndianReader::new(exif_data, endian_order);

    // Verify TIFF magic (0x002A)
    let magic = tiff_reader.u16_at(2).unwrap_or(0);
    if magic != 0x002A {
        return Err(ExifToolError::parse_error("Invalid TIFF magic number"));
    }

    // Get IFD0 offset
    let ifd_offset = tiff_reader.u32_at(4).unwrap_or(0);

    // Create in-memory reader for EXIF data
    let exif_reader = FLIFExifReader::new(exif_data.to_vec());

    // Parse IFD0
    if let Ok(ifd0_tags) = parse_ifd(&exif_reader, ifd_offset as u64, byte_order) {
        for (tag_id, field_type, value_count, raw_bytes) in &ifd0_tags {
            let tag_name = crate::tag_db::lookup_tag_name(*tag_id, "EXIF");
            let tag_value =
                raw_bytes_to_tag_value(raw_bytes, *field_type, *value_count, byte_order);
            metadata.insert(format!("EXIF:{}", tag_name), tag_value);
        }
    }

    Ok(())
}

/// Read FLIF varint-encoded integer
/// Returns (value, bytes_read)
fn read_varint(reader: &dyn FileReader, offset: u64) -> Result<(u32, u64)> {
    let first_byte = reader.read(offset, 1)?[0];

    if first_byte < 128 {
        // Single byte: value = byte + 1
        Ok((first_byte as u32 + 1, 1))
    } else {
        // Two bytes: value = ((byte - 128) << 8) + next_byte + 129
        if offset + 2 > reader.size() {
            return Err(ExifToolError::parse_error("Incomplete varint"));
        }
        let second_byte = reader.read(offset + 1, 1)?[0];
        let value = ((first_byte as u32 - 128) << 8) + second_byte as u32 + 129;
        Ok((value, 2))
    }
}

/// In-memory reader for FLIF EXIF data
struct FLIFExifReader {
    data: Vec<u8>,
}

impl FLIFExifReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for FLIFExifReader {
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
                if value_count == 1 {
                    let val = reader.u16_at(0).unwrap_or(0);
                    return TagValue::Integer(val as i64);
                }
            }
            ExifType::Long if bytes.len() >= 4 => {
                if value_count == 1 {
                    let val = reader.u32_at(0).unwrap_or(0);
                    return TagValue::Integer(val as i64);
                }
            }
            _ => {}
        }
    }

    TagValue::Binary(bytes.to_vec())
}

/// Parses metadata from FLIF files.
///
/// This is a convenience wrapper around FLIFParser that provides a functional API.
pub fn parse_flif_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = FLIFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
