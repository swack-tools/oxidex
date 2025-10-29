//! PNG chunk writing
//!
//! This module handles writing PNG chunks with metadata modifications.
//!
//! The writer preserves image data (IDAT chunks) unchanged while updating
//! metadata chunks (tEXt, iTXt, eXIf) based on the modified MetadataMap.

use crate::core::metadata_map::MetadataMap;
use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use crate::parsers::png::chunk_parser::{parse_chunk, PngChunk, PNG_SIGNATURE};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::writers::atomic_writer::write_atomic;
use crate::writers::tiff_writer::serialize_ifd;
use crc::{Crc, CRC_32_ISO_HDLC};
use std::path::Path;

/// CRC-32 instance for PNG chunk validation
const PNG_CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

/// Calculates CRC-32 checksum for a PNG chunk.
///
/// The CRC is calculated over the chunk type (4 bytes) and chunk data,
/// but NOT the length field.
///
/// # Parameters
///
/// - `chunk_type`: 4-byte chunk type (e.g., b"tEXt", b"IDAT")
/// - `data`: Chunk data bytes
///
/// # Returns
///
/// CRC-32 checksum as u32
fn calculate_crc(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
    let mut digest = PNG_CRC.digest();
    digest.update(chunk_type);
    digest.update(data);
    digest.finalize()
}

/// Writes a PNG chunk to the output buffer.
///
/// PNG chunk format:
/// - Length: 4 bytes (big-endian u32) - length of data field only
/// - Type: 4 bytes (ASCII)
/// - Data: N bytes
/// - CRC: 4 bytes (big-endian u32) - CRC-32 of type + data
///
/// # Parameters
///
/// - `output`: Output buffer to write to
/// - `chunk_type`: 4-byte chunk type
/// - `data`: Chunk data
fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    // Write length (big-endian)
    let length = data.len() as u32;
    output.extend_from_slice(&length.to_be_bytes());

    // Write type
    output.extend_from_slice(chunk_type);

    // Write data
    output.extend_from_slice(data);

    // Calculate and write CRC
    let crc = calculate_crc(chunk_type, data);
    output.extend_from_slice(&crc.to_be_bytes());
}

/// Serializes a tEXt chunk from keyword and text.
///
/// tEXt chunk format: `keyword\0text`
/// - Keyword: Latin-1 string (1-79 bytes)
/// - Null separator: 1 byte
/// - Text: Latin-1 string
///
/// # Parameters
///
/// - `keyword`: Text tag keyword (e.g., "Author", "Title")
/// - `text`: Text value
///
/// # Returns
///
/// Serialized chunk data (without length, type, or CRC)
fn serialize_text_chunk(keyword: &str, text: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(keyword.as_bytes());
    data.push(0); // Null separator
    data.extend_from_slice(text.as_bytes());
    data
}

/// Serializes an iTXt chunk from keyword and text.
///
/// iTXt chunk format: `keyword\0compression_flag\0compression_method\0language\0translated_keyword\0text`
/// - Keyword: Latin-1 string (1-79 bytes)
/// - Compression flag: 1 byte (0 = uncompressed, 1 = compressed)
/// - Compression method: 1 byte (0 = zlib, only if compressed)
/// - Language tag: UTF-8 string (can be empty)
/// - Translated keyword: UTF-8 string (can be empty)
/// - Text: UTF-8 string
///
/// This implementation creates uncompressed iTXt chunks only.
///
/// # Parameters
///
/// - `keyword`: Text tag keyword (e.g., "Title", "Description")
/// - `text`: UTF-8 text value
///
/// # Returns
///
/// Serialized chunk data (without length, type, or CRC)
fn serialize_itxt_chunk(keyword: &str, text: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(keyword.as_bytes());
    data.push(0); // Null separator
    data.push(0); // Compression flag = 0 (uncompressed)
    data.push(0); // Compression method = 0
    data.extend_from_slice(b""); // Language tag (empty)
    data.push(0); // Null separator
    data.extend_from_slice(b""); // Translated keyword (empty)
    data.push(0); // Null separator
    data.extend_from_slice(text.as_bytes()); // UTF-8 text
    data
}

/// Serializes EXIF metadata to eXIf chunk data.
///
/// The eXIf chunk contains raw TIFF-formatted EXIF data, starting with
/// the byte order marker ("II" for little-endian or "MM" for big-endian).
///
/// # Parameters
///
/// - `metadata`: MetadataMap containing EXIF tags
///
/// # Returns
///
/// Serialized eXIf chunk data (TIFF format), or error if serialization fails
fn serialize_exif_chunk(metadata: &MetadataMap) -> Result<Vec<u8>> {
    // Filter only EXIF tags
    let mut exif_metadata = MetadataMap::new();
    for (tag_name, tag_value) in metadata.iter() {
        if tag_name.starts_with("EXIF:") {
            exif_metadata.insert(tag_name, tag_value.clone());
        }
    }

    // If no EXIF tags, return empty (no eXIf chunk needed)
    if exif_metadata.is_empty() {
        return Ok(Vec::new());
    }

    // Build complete TIFF structure with header
    let mut result = Vec::new();
    let byte_order = ByteOrder::LittleEndian;

    // Write TIFF header (8 bytes)
    // "II" - Intel byte order (little-endian)
    result.extend_from_slice(&[0x49, 0x49]);
    // Magic number 42 (little-endian)
    result.extend_from_slice(&[0x2A, 0x00]);
    // First IFD offset: 8 (little-endian) - starts right after header
    result.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]);

    // Serialize IFD starting at offset 8
    let ifd_bytes = serialize_ifd(&exif_metadata, byte_order, 8)?;
    result.extend_from_slice(&ifd_bytes);

    Ok(result)
}

/// Writes modified metadata to a PNG file.
///
/// This function:
/// 1. Parses existing PNG chunk structure from the original file
/// 2. Builds new metadata chunks (tEXt, iTXt, eXIf) from modified_metadata
/// 3. Preserves non-metadata chunks (IHDR, IDAT, etc.) unchanged
/// 4. Reassembles PNG with updated metadata
/// 5. Writes atomically to prevent corruption
///
/// # Parameters
///
/// - `path`: Output file path
/// - `original_reader`: File reader for the original PNG file
/// - `modified_metadata`: Modified metadata to write
///
/// # Returns
///
/// - `Ok(())` on success
/// - `Err` if file is not valid PNG, parsing fails, or write fails
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::core::metadata_map::MetadataMap;
/// use exiftool_rs::core::tag_value::TagValue;
/// use exiftool_rs::io::buffered_reader::BufferedReader;
/// use exiftool_rs::writers::png_writer::write_png_metadata;
/// use std::path::Path;
///
/// let path = Path::new("image.png");
/// let reader = BufferedReader::new(path)?;
/// let mut metadata = MetadataMap::new();
/// metadata.insert("PNG:tEXt:Author", TagValue::new_string("John Doe"));
/// write_png_metadata(path, &reader, &metadata)?;
/// # Ok::<(), exiftool_rs::error::ExifToolError>(())
/// ```
pub fn write_png_metadata(
    path: &Path,
    original_reader: &dyn FileReader,
    modified_metadata: &MetadataMap,
) -> Result<()> {
    // Verify PNG signature
    if original_reader.size() < 8 {
        return Err(ExifToolError::parse_error("File too small to be valid PNG"));
    }

    let signature_bytes = original_reader.read(0, 8)?;
    if signature_bytes != PNG_SIGNATURE {
        return Err(ExifToolError::parse_error("Invalid PNG signature"));
    }

    // Parse all existing chunks
    let mut chunks = Vec::new();
    let mut offset = 8; // Start after signature

    while offset < original_reader.size() {
        let (next_offset, chunk) = parse_chunk(original_reader, offset)?;
        let is_iend = chunk.chunk_type == *b"IEND";
        chunks.push(chunk);
        if is_iend {
            break;
        }
        offset = next_offset;
    }

    if chunks.is_empty() {
        return Err(ExifToolError::parse_error("No PNG chunks found"));
    }

    // Categorize chunks
    let mut ihdr_chunk: Option<&PngChunk> = None;
    let mut idat_chunks = Vec::new();
    let mut iend_chunk: Option<&PngChunk> = None;
    let mut other_chunks = Vec::new();

    for chunk in &chunks {
        match &chunk.chunk_type {
            b"IHDR" => ihdr_chunk = Some(chunk),
            b"IDAT" => idat_chunks.push(chunk),
            b"IEND" => iend_chunk = Some(chunk),
            b"tEXt" | b"iTXt" | b"zTXt" | b"eXIf" => {
                // Skip old metadata chunks - they'll be replaced
            }
            _ => {
                // Preserve other chunks (PLTE, tRNS, etc.)
                other_chunks.push(chunk);
            }
        }
    }

    // Verify critical chunks exist
    let ihdr = ihdr_chunk.ok_or_else(|| ExifToolError::parse_error("Missing IHDR chunk"))?;
    let iend = iend_chunk.ok_or_else(|| ExifToolError::parse_error("Missing IEND chunk"))?;

    if idat_chunks.is_empty() {
        return Err(ExifToolError::parse_error("Missing IDAT chunks"));
    }

    // Build new metadata chunks from modified_metadata
    let mut metadata_chunks = Vec::new();

    // Process tEXt chunks
    for (tag_name, tag_value) in modified_metadata.iter() {
        if let Some(keyword) = tag_name.strip_prefix("PNG:tEXt:") {
            if let Some(text) = tag_value.as_string() {
                let data = serialize_text_chunk(keyword, text);
                metadata_chunks.push((b"tEXt", data));
            }
        }
    }

    // Process iTXt chunks
    for (tag_name, tag_value) in modified_metadata.iter() {
        if let Some(keyword) = tag_name.strip_prefix("PNG:iTXt:") {
            if let Some(text) = tag_value.as_string() {
                let data = serialize_itxt_chunk(keyword, text);
                metadata_chunks.push((b"iTXt", data));
            }
        }
    }

    // Process eXIf chunk
    let exif_data = serialize_exif_chunk(modified_metadata)?;
    if !exif_data.is_empty() {
        metadata_chunks.push((b"eXIf", exif_data));
    }

    // Reassemble PNG file
    let mut output = Vec::new();

    // Write PNG signature
    output.extend_from_slice(&PNG_SIGNATURE);

    // Write IHDR (must be first)
    write_chunk(&mut output, &ihdr.chunk_type, &ihdr.data);

    // Write metadata chunks (before IDAT for better compatibility)
    for (chunk_type, data) in metadata_chunks {
        write_chunk(&mut output, chunk_type, &data);
    }

    // Write other chunks (PLTE, tRNS, etc.)
    for chunk in other_chunks {
        write_chunk(&mut output, &chunk.chunk_type, &chunk.data);
    }

    // Write IDAT chunks (preserve image data unchanged)
    for chunk in idat_chunks {
        write_chunk(&mut output, &chunk.chunk_type, &chunk.data);
    }

    // Write IEND (must be last)
    write_chunk(&mut output, &iend.chunk_type, &iend.data);

    // Write atomically to prevent corruption
    write_atomic(path, &output)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_crc() {
        // Test CRC calculation for a simple chunk
        let chunk_type = b"tEXt";
        let data = b"Test\0Data";

        let crc1 = calculate_crc(chunk_type, data);
        let crc2 = calculate_crc(chunk_type, data);

        // CRC should be deterministic
        assert_eq!(crc1, crc2);

        // Different data should produce different CRC
        let data2 = b"Test\0Different";
        let crc3 = calculate_crc(chunk_type, data2);
        assert_ne!(crc1, crc3);
    }

    #[test]
    fn test_serialize_text_chunk() {
        let data = serialize_text_chunk("Author", "John Doe");
        assert_eq!(data, b"Author\0John Doe");
    }

    #[test]
    fn test_serialize_itxt_chunk() {
        let data = serialize_itxt_chunk("Title", "Test Image");
        // keyword\0 compression_flag compression_method language\0 translated\0 text
        assert_eq!(data, b"Title\0\0\0\0\0Test Image");
    }

    #[test]
    fn test_write_chunk() {
        let mut output = Vec::new();
        let chunk_type = b"tEXt";
        let data = b"Author\0Test";

        write_chunk(&mut output, chunk_type, data);

        // Verify structure: length (4) + type (4) + data (11) + crc (4) = 23 bytes
        assert_eq!(output.len(), 23);

        // Verify length field (big-endian)
        assert_eq!(&output[0..4], &11u32.to_be_bytes());

        // Verify type field
        assert_eq!(&output[4..8], b"tEXt");

        // Verify data field
        assert_eq!(&output[8..19], b"Author\0Test");

        // CRC is in last 4 bytes (we don't verify the value here)
    }
}
