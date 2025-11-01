//! PNG format parser
//!
//! This module provides parsing for PNG (Portable Network Graphics) files,
//! extracting metadata from text chunks (tEXt, iTXt) and embedded EXIF data (eXIf).
//!
//! # PNG Metadata Support
//!
//! The parser extracts metadata from:
//! - **tEXt chunks**: Latin-1 text metadata with keyword-value pairs
//! - **iTXt chunks**: UTF-8 internationalized text metadata
//! - **eXIf chunks**: Raw EXIF data in TIFF format
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::parsers::png::parse_png_metadata;
//! use exiftool_rs::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("image.png"))?;
//! let metadata = parse_png_metadata(&reader)?;
//!
//! // Access text metadata
//! if let Some(author) = metadata.get_string("PNG:tEXt:Author") {
//!     println!("Author: {}", author);
//! }
//!
//! // Access EXIF metadata
//! if let Some(make) = metadata.get_string("EXIF:Make") {
//!     println!("Camera Make: {}", make);
//! }
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

pub mod chunk_parser;

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::tag_db::lookup_tag_name;
use chunk_parser::{
    parse_bkgd_chunk, parse_chrm_chunk, parse_chunk, parse_exif_chunk, parse_ihdr_chunk,
    parse_itxt_chunk, parse_phys_chunk, parse_png_signature, parse_text_chunk, parse_time_chunk,
    PNG_SIGNATURE,
};

/// Parses PNG file and extracts all metadata.
///
/// This function reads the PNG file structure, verifies the signature,
/// and extracts metadata from tEXt, iTXt, and eXIf chunks.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PNG file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted metadata with tag names prefixed by chunk type
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Tag Naming Convention
///
/// - Text chunks: `PNG:tEXt:<keyword>` or `PNG:iTXt:<keyword>`
/// - EXIF tags: `EXIF:<tag_name>` (using standard EXIF tag IDs)
///
/// # Errors
///
/// Returns an error if:
/// - File is not a valid PNG (signature mismatch)
/// - File is truncated or malformed
/// - I/O error occurs
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::png::parse_png_metadata;
/// use exiftool_rs::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("photo.png"))?;
/// let metadata = parse_png_metadata(&reader)?;
///
/// for (key, value) in metadata.iter() {
///     println!("{}: {:?}", key, value);
/// }
/// # Ok(())
/// # }
/// ```
pub fn parse_png_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let file_size = reader.size();

    // Verify PNG signature
    if file_size < PNG_SIGNATURE.len() as u64 {
        return Err(ExifToolError::parse_error("File too small to be a PNG"));
    }

    let signature_data = reader.read(0, PNG_SIGNATURE.len())?;
    parse_png_signature(signature_data)
        .map_err(|_| ExifToolError::parse_error("Invalid PNG signature"))?;

    // Initialize metadata map with estimated capacity
    let mut metadata = MetadataMap::with_capacity(32);

    // Start parsing chunks after signature
    let mut offset = PNG_SIGNATURE.len() as u64;

    // Parse chunks until we reach the end or find IEND
    while offset < file_size {
        // Parse chunk at current offset
        let (next_offset, chunk) = parse_chunk(reader, offset)?;

        // Check for IEND chunk (marks end of PNG)
        if &chunk.chunk_type == b"IEND" {
            break;
        }

        // Process metadata chunks
        match &chunk.chunk_type {
            b"IHDR" => {
                // Parse IHDR chunk (image header)
                if let Ok((width, height, bit_depth, color_type, compression, filter, interlace)) =
                    parse_ihdr_chunk(&chunk.data)
                {
                    metadata.insert(
                        "PNG:ImageWidth".to_string(),
                        TagValue::new_integer(width as i64),
                    );
                    metadata.insert(
                        "PNG:ImageHeight".to_string(),
                        TagValue::new_integer(height as i64),
                    );
                    metadata.insert(
                        "PNG:BitDepth".to_string(),
                        TagValue::new_integer(bit_depth as i64),
                    );

                    // Color type enum
                    let color_type_str = match color_type {
                        0 => "Grayscale",
                        2 => "RGB",
                        3 => "Palette",
                        4 => "Grayscale with Alpha",
                        6 => "RGB with Alpha",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "PNG:ColorType".to_string(),
                        TagValue::new_string(color_type_str),
                    );

                    // Compression method (always 0 for PNG)
                    if compression == 0 {
                        metadata.insert(
                            "PNG:Compression".to_string(),
                            TagValue::new_string("Deflate/Inflate"),
                        );
                    }

                    // Filter method (always 0 for PNG)
                    if filter == 0 {
                        metadata.insert("PNG:Filter".to_string(), TagValue::new_string("Adaptive"));
                    }

                    // Interlace method
                    let interlace_str = match interlace {
                        0 => "Noninterlaced",
                        1 => "Adam7 Interlace",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "PNG:Interlace".to_string(),
                        TagValue::new_string(interlace_str),
                    );
                }
            }

            b"cHRM" => {
                // Parse cHRM chunk (chromaticity)
                if let Ok((white_x, white_y, red_x, red_y, green_x, green_y, blue_x, blue_y)) =
                    parse_chrm_chunk(&chunk.data)
                {
                    metadata.insert("PNG:WhitePointX".to_string(), TagValue::new_float(white_x));
                    metadata.insert("PNG:WhitePointY".to_string(), TagValue::new_float(white_y));
                    metadata.insert("PNG:RedX".to_string(), TagValue::new_float(red_x));
                    metadata.insert("PNG:RedY".to_string(), TagValue::new_float(red_y));
                    metadata.insert("PNG:GreenX".to_string(), TagValue::new_float(green_x));
                    metadata.insert("PNG:GreenY".to_string(), TagValue::new_float(green_y));
                    metadata.insert("PNG:BlueX".to_string(), TagValue::new_float(blue_x));
                    metadata.insert("PNG:BlueY".to_string(), TagValue::new_float(blue_y));
                }
            }

            b"pHYs" => {
                // Parse pHYs chunk (physical pixel dimensions)
                if let Ok((pixels_x, pixels_y, unit)) = parse_phys_chunk(&chunk.data) {
                    metadata.insert(
                        "PNG-pHYs:PixelsPerUnitX".to_string(),
                        TagValue::new_integer(pixels_x as i64),
                    );
                    metadata.insert(
                        "PNG-pHYs:PixelsPerUnitY".to_string(),
                        TagValue::new_integer(pixels_y as i64),
                    );

                    let unit_str = match unit {
                        0 => "Unknown",
                        1 => "Meters",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "PNG-pHYs:PixelUnits".to_string(),
                        TagValue::new_string(unit_str),
                    );
                }
            }

            b"bKGD" => {
                // Parse bKGD chunk (background color)
                if let Ok(bg_value) = parse_bkgd_chunk(&chunk.data) {
                    metadata.insert(
                        "PNG:BackgroundColor".to_string(),
                        TagValue::new_integer(bg_value as i64),
                    );
                }
            }

            b"tIME" => {
                // Parse tIME chunk (modification time)
                if let Ok(datetime) = parse_time_chunk(&chunk.data) {
                    metadata.insert("PNG:ModifyDate".to_string(), TagValue::new_string(datetime));
                }
            }

            b"PLTE" => {
                // Parse PLTE chunk (palette)
                // Perl ExifTool shows "(Binary data N bytes, use -b option to extract)"
                metadata.insert(
                    "PNG:Palette".to_string(),
                    TagValue::new_string(format!(
                        "(Binary data {} bytes, use -b option to extract)",
                        chunk.data.len()
                    )),
                );
            }

            b"tEXt" => {
                // Parse tEXt chunk
                if let Ok((keyword, text)) = parse_text_chunk(&chunk.data) {
                    let tag_name = format!("PNG:tEXt:{}", keyword);
                    metadata.insert(tag_name, TagValue::new_string(text));
                }
                // Silently skip malformed tEXt chunks to continue parsing
            }

            b"iTXt" => {
                // Parse iTXt chunk
                if let Ok((keyword, text)) = parse_itxt_chunk(&chunk.data) {
                    // Use PNG:iTXt: prefix for iTXt chunks
                    let tag_name = format!("PNG:iTXt:{}", keyword);
                    metadata.insert(tag_name, TagValue::new_string(text));
                }
                // Silently skip malformed or compressed iTXt chunks
            }

            b"eXIf" => {
                // Parse eXIf chunk and extract EXIF tags
                // The eXIf chunk contains raw TIFF/EXIF data which needs to be parsed
                // to extract tags from IFD0, ExifIFD, and GPS IFD
                if let Err(e) = parse_and_insert_exif_tags(&chunk.data, &mut metadata) {
                    eprintln!("Warning: Failed to parse eXIf chunk: {}", e);
                    // Silently skip malformed eXIf chunks
                }
            }

            _ => {
                // Skip other chunk types (IDAT, etc.)
            }
        }

        // Safety check to prevent infinite loops
        if next_offset <= offset {
            return Err(ExifToolError::parse_error(
                "Invalid chunk offset (no progress)",
            ));
        }

        // Move to next chunk
        offset = next_offset;
    }

    Ok(metadata)
}

/// Parses EXIF data from PNG eXIf chunk and inserts tags into metadata map.
///
/// This function handles the complete EXIF parsing including:
/// - IFD0 (main image tags)
/// - ExifIFD sub-IFD (extended EXIF tags)
/// - GPS sub-IFD (GPS tags)
///
/// The implementation follows the same logic as JPEG EXIF parsing to ensure
/// consistent tag naming and value conversion.
///
/// # Arguments
///
/// * `exif_data` - Raw TIFF-format EXIF data from the eXIf chunk
/// * `metadata` - Metadata map to insert parsed tags into
///
/// # Returns
///
/// - `Ok(())` if parsing succeeded
/// - `Err(ExifToolError)` if parsing failed
fn parse_and_insert_exif_tags(exif_data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    use crate::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};

    // Parse the eXIf chunk to get IFD0 tags
    let tags = parse_exif_chunk(exif_data)?;

    // Detect byte order from TIFF header (first 2 bytes)
    let byte_order = match &exif_data[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => {
            return Err(ExifToolError::parse_error(
                "Invalid byte order marker in eXIf chunk",
            ));
        }
    };

    // Create a reader for the EXIF data to parse sub-IFDs
    let exif_reader = chunk_parser::ExifDataReader::new(exif_data.to_vec());

    // Track sub-IFD offsets
    let mut exif_ifd_offset = None;
    let mut gps_ifd_offset = None;

    // Convert raw tag data to MetadataMap entries
    for (tag_id, field_type, value_count, raw_bytes) in &tags {
        // Check for EXIF Sub-IFD pointer (tag 0x8769)
        if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
            let offset = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                }
            };
            exif_ifd_offset = Some(offset as u64);

            // Perl ExifTool outputs ExifOffset in PNG:Exif namespace
            metadata.insert(
                "PNG:ExifExifOffset".to_string(),
                TagValue::new_integer(offset as i64),
            );
            continue; // Don't add to IFD0: namespace
        }

        // Check for GPS Sub-IFD pointer (tag 0x8825)
        if *tag_id == 0x8825 && raw_bytes.len() >= 4 {
            let offset = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                }
            };
            gps_ifd_offset = Some(offset as u64);
            continue; // Don't add the pointer tag to metadata
        }

        // Convert tag ID to tag name
        let base_tag_name = lookup_tag_name(*tag_id, "IFD0");

        // Convert raw bytes to TagValue using the same logic as JPEG
        let tag_value =
            raw_bytes_to_tag_value(raw_bytes, *field_type, *value_count, *tag_id, byte_order);

        // Perl ExifTool outputs PNG eXIf tags in BOTH "IFD0:" AND "PNG:Exif" namespaces
        // Add the IFD0: version (with enum interpretation)
        metadata.insert(base_tag_name.clone(), tag_value);

        // Also add the PNG:Exif version (WITHOUT enum interpretation, raw values only)
        if let Some(stripped) = base_tag_name.strip_prefix("IFD0:") {
            let raw_value = raw_bytes_to_tag_value_no_enum(
                raw_bytes,
                *field_type,
                *value_count,
                *tag_id,
                byte_order,
            );
            metadata.insert(format!("PNG:Exif{}", stripped), raw_value);
        }
    }

    // Parse EXIF Sub-IFD if present
    if let Some(offset) = exif_ifd_offset {
        if let Ok(exif_tags) = parse_ifd(&exif_reader, offset, byte_order) {
            for (tag_id, field_type, value_count, raw_bytes) in exif_tags {
                let base_tag_name = lookup_tag_name(tag_id, "ExifIFD");
                let tag_value =
                    raw_bytes_to_tag_value(&raw_bytes, field_type, value_count, tag_id, byte_order);

                // Perl ExifTool outputs PNG eXIf tags in BOTH "ExifIFD:" AND "PNG:Exif" namespaces
                // Add the ExifIFD: version (with enum interpretation)
                metadata.insert(base_tag_name.clone(), tag_value);

                // Also add the PNG:Exif version (WITHOUT enum interpretation, raw values only)
                if let Some(stripped) = base_tag_name.strip_prefix("ExifIFD:") {
                    let raw_value = raw_bytes_to_tag_value_no_enum(
                        &raw_bytes,
                        field_type,
                        value_count,
                        tag_id,
                        byte_order,
                    );
                    metadata.insert(format!("PNG:Exif{}", stripped), raw_value);
                }
            }
        }
    }

    // Parse GPS Sub-IFD if present
    if let Some(offset) = gps_ifd_offset {
        if let Ok(gps_tags) = parse_ifd(&exif_reader, offset, byte_order) {
            for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
                // GPS tags keep their "GPS:" prefix even in PNG eXIf chunks
                let tag_name = lookup_tag_name(tag_id, "GPS");
                let tag_value =
                    raw_bytes_to_tag_value(&raw_bytes, field_type, value_count, tag_id, byte_order);
                metadata.insert(tag_name, tag_value);
            }
        }
    }

    Ok(())
}

/// Converts raw bytes from IFD to a TagValue WITHOUT enum interpretation.
///
/// This version is used for PNG:Exif tags where Perl ExifTool outputs raw values.
fn raw_bytes_to_tag_value_no_enum(
    bytes: &[u8],
    field_type: u16,
    _value_count: u32,
    tag_id: u16,
    byte_order: crate::parsers::tiff::ifd_parser::ByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;
    use crate::parsers::tiff::ifd_parser::ByteOrder;

    const EXIF_VERSION: u16 = 0x9000;

    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // SHORT (type 3): 16-bit unsigned integer
            ExifType::Short if bytes.len() >= 2 => {
                let value = match byte_order {
                    ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
                    ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
                };
                return TagValue::new_integer(value as i64);
            }

            // LONG (type 4): 32-bit unsigned integer
            ExifType::Long if bytes.len() >= 4 => {
                let value = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                };
                return TagValue::new_integer(value as i64);
            }

            // ASCII (type 2): null-terminated string
            ExifType::Ascii => {
                let text = String::from_utf8_lossy(bytes);
                let trimmed = text.trim_end_matches('\0');
                return TagValue::new_string(trimmed);
            }

            // UNDEFINED (type 7): Return as binary or special string
            ExifType::Undefined => {
                // Special handling for ExifVersion (tag 0x9000)
                if tag_id == EXIF_VERSION && bytes.len() >= 4 {
                    // ExifVersion is stored as ASCII bytes
                    let version = String::from_utf8_lossy(&bytes[0..4]);
                    return TagValue::new_string(version.to_string());
                }
                // Perl ExifTool shows UNDEFINED bytes as "..." in PNG:Exif namespace
                return TagValue::new_string("...");
            }

            _ => {
                // Fallback
            }
        }
    }

    // Fallback: store as binary
    TagValue::new_binary(bytes.to_vec())
}

/// Converts raw bytes from IFD to a TagValue.
///
/// This is a wrapper around the function in operations.rs to make it available
/// in the PNG parser. See operations.rs for the full implementation.
fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    tag_id: u16,
    byte_order: crate::parsers::tiff::ifd_parser::ByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;
    use crate::parsers::tiff::ifd_parser::ByteOrder;
    use crate::parsers::tiff::tiff_enums::tiff_enum_to_string;

    // Try to convert field_type to ExifType
    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // RATIONAL (type 5): two 32-bit unsigned integers (numerator/denominator)
            ExifType::Rational if bytes.len() >= 8 => {
                // Check if this is an array of rationals (count > 1)
                if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
                    // Parse array of rationals and format as space-separated decimals
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 8;
                        let numerator = match byte_order {
                            ByteOrder::LittleEndian => u32::from_le_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                            ByteOrder::BigEndian => u32::from_be_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                        };
                        let denominator = match byte_order {
                            ByteOrder::LittleEndian => u32::from_le_bytes([
                                bytes[offset + 4],
                                bytes[offset + 5],
                                bytes[offset + 6],
                                bytes[offset + 7],
                            ]),
                            ByteOrder::BigEndian => u32::from_be_bytes([
                                bytes[offset + 4],
                                bytes[offset + 5],
                                bytes[offset + 6],
                                bytes[offset + 7],
                            ]),
                        };
                        if denominator != 0 {
                            values.push(numerator as f64 / denominator as f64);
                        } else {
                            values.push(numerator as f64);
                        }
                    }
                    // Return as rational (first value) to match behavior
                    if !values.is_empty() {
                        let num = (values[0] * 1000000.0) as i32;
                        return TagValue::new_rational(num, 1000000);
                    }
                }

                // Single rational value
                let numerator = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                };
                let denominator = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
                    }
                };

                // Simplify: if denominator is 1, return as integer
                if denominator == 1 {
                    return TagValue::new_integer(numerator as i64);
                }

                return TagValue::new_rational(numerator as i32, denominator as i32);
            }

            // SHORT (type 3): 16-bit unsigned integer
            ExifType::Short if bytes.len() >= 2 => {
                // Handle array of SHORT values
                if value_count > 1 && bytes.len() >= (value_count as usize * 2) {
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 2;
                        let value = match byte_order {
                            ByteOrder::LittleEndian => {
                                u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
                            }
                            ByteOrder::BigEndian => {
                                u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
                            }
                        };
                        values.push(value as i64);
                    }
                    // Return as space-separated string for arrays
                    return TagValue::new_string(
                        values
                            .iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }

                let value = match byte_order {
                    ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
                    ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
                };

                // Try to convert to enum string if applicable
                if let Some(enum_str) = tiff_enum_to_string(tag_id, value as i64) {
                    return TagValue::new_string(enum_str);
                }

                return TagValue::new_integer(value as i64);
            }

            // LONG (type 4): 32-bit unsigned integer
            ExifType::Long if bytes.len() >= 4 => {
                // Handle array of LONG values
                if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 4;
                        let value = match byte_order {
                            ByteOrder::LittleEndian => u32::from_le_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                            ByteOrder::BigEndian => u32::from_be_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                        };
                        values.push(value as i64);
                    }
                    // Return as space-separated string for arrays
                    return TagValue::new_string(
                        values
                            .iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }

                let value = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                };

                // Try to convert to enum string if applicable
                if let Some(enum_str) = tiff_enum_to_string(tag_id, value as i64) {
                    return TagValue::new_string(enum_str);
                }

                return TagValue::new_integer(value as i64);
            }

            // ASCII (type 2): null-terminated string
            ExifType::Ascii => {
                let text = String::from_utf8_lossy(bytes);
                let trimmed = text.trim_end_matches('\0');
                return TagValue::new_string(trimmed);
            }

            // UNDEFINED (type 7): typically used for ExifVersion, ComponentsConfiguration, etc.
            ExifType::Undefined => {
                // Handle ExifVersion (tag 0x9000) - 4 bytes representing version
                if tag_id == 0x9000 && bytes.len() >= 4 {
                    let version_str = format!(
                        "{}{}{}{}",
                        bytes[0] as char, bytes[1] as char, bytes[2] as char, bytes[3] as char
                    );
                    return TagValue::new_string(version_str);
                }

                // Handle ComponentsConfiguration (tag 0x9101) - 4 bytes
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
                    return TagValue::new_string(components.join(", "));
                }

                // Otherwise store as binary
                return TagValue::new_binary(bytes.to_vec());
            }

            _ => {
                // Fallback for other types
            }
        }
    }

    // Fallback: try to interpret as ASCII string
    if bytes.iter().all(|&b| b.is_ascii() || b == 0) {
        let text = String::from_utf8_lossy(bytes);
        let trimmed = text.trim_end_matches('\0');
        TagValue::new_string(trimmed)
    } else {
        // Store as binary
        TagValue::new_binary(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// Simple in-memory FileReader for testing
    struct TestReader {
        data: Vec<u8>,
    }

    impl TestReader {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    impl FileReader for TestReader {
        fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
            let start = offset as usize;
            let end = start + length;

            if end > self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "read beyond end of file",
                ));
            }

            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    /// Creates a minimal valid PNG with IHDR and IEND chunks
    fn create_minimal_png() -> Vec<u8> {
        let mut data = Vec::new();

        // PNG signature
        data.extend_from_slice(&PNG_SIGNATURE);

        // IHDR chunk (13 bytes data)
        data.extend_from_slice(&13u32.to_be_bytes()); // Length
        data.extend_from_slice(b"IHDR"); // Type
        data.extend_from_slice(&[
            0, 0, 0, 1, // Width: 1
            0, 0, 0, 1, // Height: 1
            8, // Bit depth
            2, // Color type: RGB
            0, // Compression
            0, // Filter
            0, // Interlace
        ]);
        data.extend_from_slice(&0u32.to_be_bytes()); // CRC (dummy)

        // IEND chunk (0 bytes data)
        data.extend_from_slice(&0u32.to_be_bytes()); // Length
        data.extend_from_slice(b"IEND"); // Type
        data.extend_from_slice(&0u32.to_be_bytes()); // CRC (dummy)

        data
    }

    #[test]
    fn test_parse_minimal_png() {
        let png_data = create_minimal_png();
        let reader = TestReader::new(png_data);

        let result = parse_png_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        // Minimal PNG now extracts IHDR chunk metadata (7 tags)
        assert_eq!(metadata.len(), 7);
        // Verify IHDR tags are present
        assert!(metadata.contains_key("PNG:ImageWidth"));
        assert!(metadata.contains_key("PNG:ImageHeight"));
        assert!(metadata.contains_key("PNG:BitDepth"));
        assert!(metadata.contains_key("PNG:ColorType"));
        assert!(metadata.contains_key("PNG:Compression"));
        assert!(metadata.contains_key("PNG:Filter"));
        assert!(metadata.contains_key("PNG:Interlace"));
    }

    #[test]
    fn test_parse_png_with_text_chunk() {
        let mut data = create_minimal_png();

        // Insert tEXt chunk before IEND
        let iend_pos = data.len() - 12; // IEND chunk is 12 bytes
        let mut text_chunk = Vec::new();

        let text_data = b"Author\0John Doe";
        text_chunk.extend_from_slice(&(text_data.len() as u32).to_be_bytes()); // Length
        text_chunk.extend_from_slice(b"tEXt"); // Type
        text_chunk.extend_from_slice(text_data); // Data
        text_chunk.extend_from_slice(&0u32.to_be_bytes()); // CRC (dummy)

        // Insert text chunk before IEND
        data.splice(iend_pos..iend_pos, text_chunk);

        let reader = TestReader::new(data);
        let result = parse_png_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        // 7 IHDR tags + 1 tEXt tag = 8 total
        assert_eq!(metadata.len(), 8);
        assert_eq!(metadata.get_string("PNG:tEXt:Author"), Some("John Doe"));
    }

    #[test]
    fn test_parse_png_with_itxt_chunk() {
        let mut data = create_minimal_png();

        // Create iTXt chunk data
        let mut itxt_data = Vec::new();
        itxt_data.extend_from_slice(b"Title"); // keyword
        itxt_data.push(0); // null
        itxt_data.push(0); // compression flag = 0
        itxt_data.push(0); // compression method
        itxt_data.extend_from_slice(b"en"); // language
        itxt_data.push(0); // null
        itxt_data.extend_from_slice(b"Title"); // translated keyword
        itxt_data.push(0); // null
        itxt_data.extend_from_slice(b"My PNG Image"); // text

        // Insert iTXt chunk before IEND
        let iend_pos = data.len() - 12;
        let mut itxt_chunk = Vec::new();
        itxt_chunk.extend_from_slice(&(itxt_data.len() as u32).to_be_bytes());
        itxt_chunk.extend_from_slice(b"iTXt");
        itxt_chunk.extend_from_slice(&itxt_data);
        itxt_chunk.extend_from_slice(&0u32.to_be_bytes());

        data.splice(iend_pos..iend_pos, itxt_chunk);

        let reader = TestReader::new(data);
        let result = parse_png_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        // 7 IHDR tags + 1 iTXt tag = 8 total
        assert_eq!(metadata.len(), 8);
        assert_eq!(metadata.get_string("PNG:iTXt:Title"), Some("My PNG Image"));
    }

    #[test]
    fn test_parse_png_with_exif_chunk() {
        let mut data = create_minimal_png();

        // Create minimal EXIF data (little-endian)
        let mut exif_data = Vec::new();
        exif_data.extend_from_slice(b"II"); // Little-endian
        exif_data.extend_from_slice(&0x002Au16.to_le_bytes()); // Magic
        exif_data.extend_from_slice(&8u32.to_le_bytes()); // IFD offset

        // IFD with 1 entry
        exif_data.extend_from_slice(&1u16.to_le_bytes()); // Entry count

        // Tag entry: Make (0x010F) = "Test" (inline)
        exif_data.extend_from_slice(&0x010Fu16.to_le_bytes()); // Tag ID
        exif_data.extend_from_slice(&2u16.to_le_bytes()); // Type: ASCII
        exif_data.extend_from_slice(&4u32.to_le_bytes()); // Count: 4
        exif_data.extend_from_slice(b"Tst\0"); // Inline value (4 bytes max)

        // Next IFD offset: 0
        exif_data.extend_from_slice(&0u32.to_le_bytes());

        // Insert eXIf chunk before IEND
        let iend_pos = data.len() - 12;
        let mut exif_chunk = Vec::new();
        exif_chunk.extend_from_slice(&(exif_data.len() as u32).to_be_bytes());
        exif_chunk.extend_from_slice(b"eXIf");
        exif_chunk.extend_from_slice(&exif_data);
        exif_chunk.extend_from_slice(&0u32.to_be_bytes());

        data.splice(iend_pos..iend_pos, exif_chunk);

        let reader = TestReader::new(data);
        let result = parse_png_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        // 7 IHDR tags + EXIF tags (at least 1) = 8+ total
        assert!(
            metadata.len() >= 8,
            "Expected at least 8 tags (7 IHDR + 1+ EXIF), got {}",
            metadata.len()
        );
        // Tag 0x010F is Make - parser now uses "IFD0:Make" instead of "EXIF:0x010F"
        assert_eq!(metadata.get_string("IFD0:Make"), Some("Tst"));
    }

    #[test]
    fn test_parse_png_invalid_signature() {
        let data = vec![0xFF; 100]; // Invalid signature
        let reader = TestReader::new(data);

        let result = parse_png_metadata(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_png_too_small() {
        let data = vec![0x89, 0x50]; // Only 2 bytes
        let reader = TestReader::new(data);

        let result = parse_png_metadata(&reader);
        assert!(result.is_err());
    }
}
