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
                    metadata.insert("PNG:ImageWidth".to_string(), TagValue::new_integer(width as i64));
                    metadata.insert("PNG:ImageHeight".to_string(), TagValue::new_integer(height as i64));
                    metadata.insert("PNG:BitDepth".to_string(), TagValue::new_integer(bit_depth as i64));

                    // Color type enum
                    let color_type_str = match color_type {
                        0 => "Grayscale",
                        2 => "RGB",
                        3 => "Palette",
                        4 => "Grayscale with Alpha",
                        6 => "RGB with Alpha",
                        _ => "Unknown",
                    };
                    metadata.insert("PNG:ColorType".to_string(), TagValue::new_string(color_type_str));

                    // Compression method (always 0 for PNG)
                    if compression == 0 {
                        metadata.insert("PNG:Compression".to_string(), TagValue::new_string("Deflate/Inflate"));
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
                    metadata.insert("PNG:Interlace".to_string(), TagValue::new_string(interlace_str));
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
                    metadata.insert("PNG-pHYs:PixelsPerUnitX".to_string(), TagValue::new_integer(pixels_x as i64));
                    metadata.insert("PNG-pHYs:PixelsPerUnitY".to_string(), TagValue::new_integer(pixels_y as i64));

                    let unit_str = match unit {
                        0 => "Unknown",
                        1 => "Meters",
                        _ => "Unknown",
                    };
                    metadata.insert("PNG-pHYs:PixelUnits".to_string(), TagValue::new_string(unit_str));
                }
            }

            b"bKGD" => {
                // Parse bKGD chunk (background color)
                if let Ok(bg_value) = parse_bkgd_chunk(&chunk.data) {
                    metadata.insert("PNG:BackgroundColor".to_string(), TagValue::new_integer(bg_value as i64));
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
                    TagValue::new_string(format!("(Binary data {} bytes, use -b option to extract)", chunk.data.len()))
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
                match parse_exif_chunk(&chunk.data) {
                    Ok(exif_tags) => {
                        // Convert EXIF tags to metadata entries
                        for (tag_id, _field_type, _value_count, raw_bytes) in exif_tags {
                            // Format tag name as EXIF:0x<hex_id> for now
                            // In a full implementation, we'd look up the tag name from tag registry
                            let tag_name = format!("EXIF:0x{:04X}", tag_id);

                            // Try to interpret as ASCII string (most common case)
                            // Remove null terminators if present
                            let value = if raw_bytes.iter().all(|&b| b.is_ascii() || b == 0) {
                                let text = String::from_utf8_lossy(&raw_bytes);
                                let trimmed = text.trim_end_matches('\0');
                                TagValue::new_string(trimmed)
                            } else {
                                // Store as binary if not ASCII
                                TagValue::new_binary(raw_bytes)
                            };

                            metadata.insert(tag_name, value);
                        }
                    }
                    Err(_) => {
                        // Silently skip malformed eXIf chunks
                    }
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
        // Minimal PNG has no metadata chunks
        assert_eq!(metadata.len(), 0);
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
        assert_eq!(metadata.len(), 1);
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
        assert_eq!(metadata.len(), 1);
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
        assert_eq!(metadata.len(), 1);
        // Tag 0x010F is Make
        assert_eq!(metadata.get_string("EXIF:0x010F"), Some("Tst"));
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
