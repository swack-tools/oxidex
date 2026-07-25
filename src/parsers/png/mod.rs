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
//! use oxidex::parsers::png::parse_png_metadata;
//! use oxidex::io::buffered_reader::BufferedReader;
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
mod exif;
mod value_conversion;

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use chunk_parser::{
    PNG_SIGNATURE, parse_bkgd_chunk, parse_chrm_chunk, parse_chunk, parse_gama_chunk,
    parse_hist_chunk, parse_ihdr_chunk, parse_itxt_chunk, parse_phys_chunk, parse_png_signature,
    parse_sbit_chunk, parse_text_chunk, parse_time_chunk, parse_ztxt_chunk,
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
/// use oxidex::parsers::png::parse_png_metadata;
/// use oxidex::io::buffered_reader::BufferedReader;
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
                    // Width tag - support both naming conventions
                    metadata.insert("PNG:Width".to_string(), TagValue::new_integer(width as i64));
                    metadata.insert(
                        "PNG:ImageWidth".to_string(),
                        TagValue::new_integer(width as i64),
                    );

                    // Height tag - support both naming conventions
                    metadata.insert(
                        "PNG:Height".to_string(),
                        TagValue::new_integer(height as i64),
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

                    // HasTransparency tag - true for color types with alpha channel (4, 6)
                    // Color type 3 (Palette) may have transparency via tRNS chunk,
                    // but IHDR alone doesn't tell us, so only mark 4 and 6 as definitely transparent
                    let has_transparency = matches!(color_type, 4 | 6);
                    metadata.insert(
                        "PNG:HasTransparency".to_string(),
                        TagValue::new_string(if has_transparency { "Yes" } else { "No" }),
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

            b"gAMA" => {
                // Parse gAMA chunk (gamma)
                if let Ok(gamma) = parse_gama_chunk(&chunk.data) {
                    metadata.insert("PNG:Gamma".to_string(), TagValue::new_float(gamma));
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
                    // Check if this iTXt chunk contains XMP metadata
                    // XMP is stored with keyword "XML:com.adobe.xmp"
                    if keyword == "XML:com.adobe.xmp" {
                        // Parse XMP content and insert tags
                        // Note: XMP parser already returns tags with "XMP-" prefix (e.g., "XMP-xmp:Creator")
                        match crate::parsers::xmp::parse_xmp(text.as_bytes()) {
                            Ok(xmp_tags) => {
                                for (tag_name, value) in xmp_tags {
                                    metadata.insert(tag_name, TagValue::new_string(value));
                                }
                            }
                            Err(e) => {
                                eprintln!("Warning: Failed to parse XMP in iTXt chunk: {}", e);
                                // Fall back to storing as regular iTXt
                                let tag_name = format!("PNG:iTXt:{}", keyword);
                                metadata.insert(tag_name, TagValue::new_string(text));
                            }
                        }
                    } else {
                        // Regular iTXt metadata - use PNG:iTXt: prefix
                        let tag_name = format!("PNG:iTXt:{}", keyword);
                        metadata.insert(tag_name, TagValue::new_string(text));
                    }
                }
                // Silently skip malformed or compressed iTXt chunks
            }

            b"zTXt" => {
                // Parse zTXt chunk (compressed text)
                if let Ok((keyword, text)) = parse_ztxt_chunk(&chunk.data) {
                    let tag_name = format!("PNG:zTXt:{}", keyword);
                    metadata.insert(tag_name, TagValue::new_string(text));
                }
                // Silently skip malformed zTXt chunks
            }

            b"sBIT" => {
                // Parse sBIT chunk (significant bits)
                if let Ok(bits) = parse_sbit_chunk(&chunk.data) {
                    let bits_str = bits
                        .iter()
                        .map(|b| b.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    metadata.insert(
                        "PNG:SignificantBits".to_string(),
                        TagValue::new_string(bits_str),
                    );
                }
            }

            b"hIST" => {
                // Parse hIST chunk (histogram)
                if let Ok(histogram) = parse_hist_chunk(&chunk.data) {
                    metadata.insert(
                        "PNG:Histogram".to_string(),
                        TagValue::new_string(format!(
                            "(Binary data {} entries, use -b option to extract)",
                            histogram.len()
                        )),
                    );
                }
            }

            b"eXIf" => {
                // Parse eXIf chunk and extract EXIF tags
                // The eXIf chunk contains raw TIFF/EXIF data which needs to be parsed
                // to extract tags from IFD0, ExifIFD, and GPS IFD
                if let Err(e) = exif::parse_and_insert_exif_tags(&chunk.data, &mut metadata) {
                    eprintln!("Warning: Failed to parse eXIf chunk: {}", e);
                    // Silently skip malformed eXIf chunks
                }
            }

            b"iCCP" => {
                // Parse iCCP chunk (ICC profile)
                // Structure: profile name (null-terminated) + compression method (1 byte) + compressed profile data
                if let Some(null_pos) = chunk.data.iter().position(|&b| b == 0)
                    && null_pos + 2 <= chunk.data.len()
                {
                    let _profile_name = String::from_utf8_lossy(&chunk.data[..null_pos]);
                    let compression_method = chunk.data[null_pos + 1];

                    if compression_method == 0 {
                        // Deflate/Inflate compression
                        let compressed_data = &chunk.data[null_pos + 2..];

                        // Decompress ICC profile data
                        use flate2::read::ZlibDecoder;
                        use std::io::Read;

                        let mut decoder = ZlibDecoder::new(compressed_data);
                        let mut icc_data = Vec::new();

                        if decoder.read_to_end(&mut icc_data).is_ok() {
                            // Parse ICC profile
                            match crate::parsers::icc::parse_icc_profile_data(&icc_data) {
                                Ok(icc_tags) => {
                                    // Add all ICC tags to metadata with "Profile:" prefix
                                    for (tag_name, value) in icc_tags {
                                        metadata.insert(format!("Profile:{}", tag_name), value);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Warning: Failed to parse ICC profile in PNG: {}", e);
                                }
                            }
                        } else {
                            eprintln!("Warning: Failed to decompress iCCP chunk data");
                        }
                    } else {
                        eprintln!(
                            "Warning: Unknown iCCP compression method: {}",
                            compression_method
                        );
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
    use crate::test_support::TestReader;

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
        // Minimal PNG now extracts IHDR chunk metadata (9 tags - Width, Height, BitDepth, ColorType,
        // Compression, Filter, Interlace, and their duplicates ImageWidth, ImageHeight, plus HasTransparency)
        assert!(
            metadata.len() >= 9,
            "Expected at least 9 IHDR tags, got {}",
            metadata.len()
        );
        // Verify IHDR tags are present (both new and old naming conventions)
        assert!(metadata.contains_key("PNG:Width"));
        assert!(metadata.contains_key("PNG:ImageWidth"));
        assert!(metadata.contains_key("PNG:Height"));
        assert!(metadata.contains_key("PNG:ImageHeight"));
        assert!(metadata.contains_key("PNG:BitDepth"));
        assert!(metadata.contains_key("PNG:ColorType"));
        assert!(metadata.contains_key("PNG:Compression"));
        assert!(metadata.contains_key("PNG:Filter"));
        assert!(metadata.contains_key("PNG:Interlace"));
        assert!(metadata.contains_key("PNG:HasTransparency"));
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
        // 10 IHDR tags (Width, ImageWidth, Height, ImageHeight, BitDepth, ColorType,
        // Compression, Filter, Interlace, HasTransparency) + 1 tEXt tag = 11 total
        assert!(
            metadata.len() >= 10,
            "Expected at least 10 IHDR tags + tEXt, got {}",
            metadata.len()
        );
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
        // 10 IHDR tags + 1 iTXt tag = 11 total
        assert!(
            metadata.len() >= 10,
            "Expected at least 10 IHDR tags + iTXt, got {}",
            metadata.len()
        );
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
        // 10 IHDR tags (Width, ImageWidth, Height, ImageHeight, BitDepth, ColorType,
        // Compression, Filter, Interlace, HasTransparency) + EXIF tags (at least 1) = 11+ total
        assert!(
            metadata.len() >= 11,
            "Expected at least 11 tags (10 IHDR + 1+ EXIF), got {}",
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

    #[test]
    fn test_parse_png_with_itxt_xmp_chunk() {
        let mut data = create_minimal_png();

        // Create iTXt chunk data with XMP content
        let mut itxt_data = Vec::new();
        itxt_data.extend_from_slice(b"XML:com.adobe.xmp"); // keyword for XMP
        itxt_data.push(0); // null
        itxt_data.push(0); // compression flag = 0
        itxt_data.push(0); // compression method
        itxt_data.extend_from_slice(b""); // language (empty)
        itxt_data.push(0); // null
        itxt_data.extend_from_slice(b""); // translated keyword (empty)
        itxt_data.push(0); // null

        // XMP content
        let xmp_content = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmp="http://ns.adobe.com/xap/1.0/">
              <rdf:Description>
                <xmp:Creator>Test Creator</xmp:Creator>
              </rdf:Description>
            </rdf:RDF>"#;
        itxt_data.extend_from_slice(xmp_content);

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
        // Should have parsed XMP and extracted Creator tag
        // Check for XMP-xmp:Creator (the format from parse_xmp)
        let has_creator = metadata.contains_key("XMP-xmp:Creator")
            || metadata.keys().any(|k| k.contains("Creator"));
        assert!(
            has_creator,
            "Expected Creator tag, got keys: {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
        // Get the creator value from whichever key format is present
        let creator_value = metadata.get_string("XMP-xmp:Creator").or_else(|| {
            metadata
                .iter()
                .find(|(k, _)| k.contains("Creator"))
                .and_then(|(_, v)| v.as_string())
        });
        assert_eq!(creator_value, Some("Test Creator"));
    }

    #[test]
    fn test_parse_png_with_gama_chunk() {
        let mut data = create_minimal_png();

        // Create gAMA chunk data (gamma = 2.2, stored as 220000)
        let gama_data = 220000u32.to_be_bytes();

        // Insert gAMA chunk before IEND
        let iend_pos = data.len() - 12;
        let mut gama_chunk = Vec::new();
        gama_chunk.extend_from_slice(&(gama_data.len() as u32).to_be_bytes());
        gama_chunk.extend_from_slice(b"gAMA");
        gama_chunk.extend_from_slice(&gama_data);
        gama_chunk.extend_from_slice(&0u32.to_be_bytes());

        data.splice(iend_pos..iend_pos, gama_chunk);

        let reader = TestReader::new(data);
        let result = parse_png_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        // 10 IHDR tags + 1 gAMA tag = 11 total
        assert!(
            metadata.len() >= 10,
            "Expected at least 10 IHDR tags + gAMA, got {}",
            metadata.len()
        );

        // Check gamma value
        let gamma = metadata.get_float("PNG:Gamma");
        assert!(gamma.is_some());
        let gamma_val = gamma.unwrap();
        assert!((gamma_val - 2.2).abs() < 0.0001);
    }
}
