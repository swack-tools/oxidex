//! PNG chunk parsing (tEXt, iTXt, zTXt, eXIf)
//!
//! This module handles parsing of PNG file structure and metadata chunks.
//!
//! # PNG File Structure
//!
//! A PNG file consists of:
//! 1. **PNG Signature**: 8 bytes [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
//! 2. **Chunks**: Sequence of chunks, each containing:
//!    - Length: 4 bytes (big-endian u32) - length of data field only
//!    - Chunk Type: 4 bytes ASCII (e.g., "IHDR", "tEXt", "IEND")
//!    - Chunk Data: Length bytes
//!    - CRC: 4 bytes (CRC-32 of type + data)
//!
//! # Metadata Chunks
//!
//! - **tEXt**: Latin-1 text, format is `keyword\0text` (null-separated)
//! - **iTXt**: UTF-8 text with optional compression and language info
//! - **zTXt**: Compressed text (not implemented in MVP)
//! - **eXIf**: Raw EXIF data (TIFF format)

#![allow(dead_code)]

use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntries, parse_ifd};
use nom::{
    IResult,
    bytes::complete::{tag, take},
    number::complete::be_u32,
};
use std::io;

/// PNG file signature (8 bytes)
pub const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Type alias for cHRM chunk chromaticity values
/// (white_x, white_y, red_x, red_y, green_x, green_y, blue_x, blue_y)
pub type ChromaticityValues = (f64, f64, f64, f64, f64, f64, f64, f64);

/// Represents a parsed PNG chunk
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngChunk {
    /// 4-byte chunk type (ASCII characters)
    pub chunk_type: [u8; 4],
    /// Chunk data (variable length)
    pub data: Vec<u8>,
    /// CRC-32 checksum
    pub crc: u32,
}

impl PngChunk {
    /// Returns the chunk type as a string (e.g., "IHDR", "tEXt")
    pub fn type_str(&self) -> String {
        String::from_utf8_lossy(&self.chunk_type).into_owned()
    }

    /// Returns true if this is a text metadata chunk (tEXt, iTXt, zTXt)
    pub fn is_text_chunk(&self) -> bool {
        matches!(&self.chunk_type, b"tEXt" | b"iTXt" | b"zTXt")
    }

    /// Returns true if this is an EXIF chunk
    pub fn is_exif_chunk(&self) -> bool {
        &self.chunk_type == b"eXIf"
    }
}

/// Verifies the PNG file signature at the start of the file.
///
/// # Parameters
///
/// - `input`: Byte slice starting at the beginning of the file
///
/// # Returns
///
/// - `Ok(remaining_input)`: Signature is valid, returns remaining bytes
/// - `Err`: Signature is invalid or input too short
///
/// # Example
///
/// ```no_run
/// use oxidex::parsers::png::chunk_parser::{parse_png_signature, PNG_SIGNATURE};
///
/// let data = [PNG_SIGNATURE.as_slice(), &[0x00, 0x01, 0x02]].concat();
/// let result = parse_png_signature(&data);
/// assert!(result.is_ok());
/// ```
pub fn parse_png_signature(input: &[u8]) -> IResult<&[u8], ()> {
    let (input, _) = tag(PNG_SIGNATURE.as_slice())(input)?;
    Ok((input, ()))
}

/// Parses a chunk header (length + type).
///
/// # Parameters
///
/// - `input`: Byte slice at the start of a chunk
///
/// # Returns
///
/// - `Ok((remaining, (length, chunk_type)))`: Parsed header
/// - `Err`: Parse failure
///
/// # Format
///
/// ```text
/// Length: 4 bytes (big-endian u32)
/// Type: 4 bytes (ASCII)
/// ```
pub fn parse_chunk_header(input: &[u8]) -> IResult<&[u8], (u32, [u8; 4])> {
    let (input, length) = be_u32(input)?;
    let (input, chunk_type_bytes) = take(4usize)(input)?;

    let mut chunk_type = [0u8; 4];
    chunk_type.copy_from_slice(chunk_type_bytes);

    Ok((input, (length, chunk_type)))
}

/// Parses a complete PNG chunk at the specified offset.
///
/// # Parameters
///
/// - `reader`: FileReader for accessing file data
/// - `offset`: Byte offset to the start of the chunk
///
/// # Returns
///
/// - `Ok((next_offset, chunk))`: Parsed chunk and offset to next chunk
/// - `Err`: Parse error or I/O error
///
/// # Errors
///
/// Returns an error if:
/// - Chunk header is malformed
/// - Chunk data extends beyond file size
/// - I/O error occurs
pub fn parse_chunk(reader: &dyn FileReader, offset: u64) -> Result<(u64, PngChunk)> {
    let file_size = reader.size();

    // Read chunk header (8 bytes: length + type)
    if offset + 8 > file_size {
        return Err(ExifToolError::parse_error_at(
            "Chunk header extends beyond file size",
            offset as usize,
        ));
    }

    let header_data = reader.read(offset, 8)?;
    let (_, (length, chunk_type)) = parse_chunk_header(header_data).map_err(|e| {
        ExifToolError::parse_error_at(
            format!("Failed to parse chunk header: {}", e),
            offset as usize,
        )
    })?;

    // Calculate total chunk size: length + type (4) + data (length) + CRC (4)
    let total_size = 4 + 4 + length as u64 + 4;

    if offset + total_size > file_size {
        return Err(ExifToolError::parse_error_at(
            format!("Chunk (length={}) extends beyond file size", length),
            offset as usize,
        ));
    }

    // Read chunk data
    let data = if length > 0 {
        let data_offset = offset + 8;
        reader.read(data_offset, length as usize)?.to_vec()
    } else {
        Vec::new()
    };

    // Read CRC
    let crc_offset = offset + 8 + length as u64;
    let crc_data = reader.read(crc_offset, 4)?;
    let crc = crate::io::EndianReader::big_endian(crc_data)
        .u32_at(0)
        .ok_or_else(|| ExifToolError::parse_error_at("Failed to read CRC", crc_offset as usize))?;

    let chunk = PngChunk {
        chunk_type,
        data,
        crc,
    };

    let next_offset = offset + total_size;
    Ok((next_offset, chunk))
}

/// Parses a tEXt chunk and extracts keyword and text.
///
/// # Format
///
/// ```text
/// Keyword: 1-79 bytes (Latin-1)
/// Null separator: 1 byte (0x00)
/// Text: n bytes (Latin-1)
/// ```
///
/// # Parameters
///
/// - `data`: Chunk data bytes
///
/// # Returns
///
/// - `Ok((keyword, text))`: Extracted keyword and text as UTF-8 strings
/// - `Err`: Parse error (missing null separator, etc.)
pub fn parse_text_chunk(data: &[u8]) -> Result<(String, String)> {
    // Find null separator
    let null_pos = data
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| ExifToolError::parse_error("tEXt chunk missing null separator"))?;

    if null_pos == 0 {
        return Err(ExifToolError::parse_error("tEXt chunk has empty keyword"));
    }

    // Split at null byte
    let (keyword_bytes, rest) = data.split_at(null_pos);
    let text_bytes = &rest[1..]; // Skip the null byte

    // Convert to strings (Latin-1 is compatible with UTF-8 for ASCII range)
    let keyword = String::from_utf8_lossy(keyword_bytes).to_string();
    let text = String::from_utf8_lossy(text_bytes).to_string();

    Ok((keyword, text))
}

/// Parses an iTXt chunk and extracts keyword and text.
///
/// # Format
///
/// ```text
/// Keyword: 1-79 bytes (Latin-1)
/// Null separator: 1 byte (0x00)
/// Compression flag: 1 byte (0=uncompressed, 1=compressed)
/// Compression method: 1 byte (0=deflate)
/// Language tag: 0-n bytes (ASCII)
/// Null separator: 1 byte (0x00)
/// Translated keyword: 0-n bytes (UTF-8)
/// Null separator: 1 byte (0x00)
/// Text: n bytes (UTF-8)
/// ```
///
/// # Parameters
///
/// - `data`: Chunk data bytes
///
/// # Returns
///
/// - `Ok((keyword, text))`: Extracted keyword and text as UTF-8 strings
/// - `Err`: Parse error
///
/// # Note
///
/// For MVP, this implementation only supports uncompressed text (compression_flag=0).
/// Compressed iTXt chunks will return an error.
pub fn parse_itxt_chunk(data: &[u8]) -> Result<(String, String)> {
    // Find first null separator (after keyword)
    let keyword_end = data
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| ExifToolError::parse_error("iTXt chunk missing keyword null separator"))?;

    if keyword_end == 0 {
        return Err(ExifToolError::parse_error("iTXt chunk has empty keyword"));
    }

    let keyword = String::from_utf8_lossy(&data[..keyword_end]).to_string();

    // Check we have at least compression flag and method
    if keyword_end + 3 > data.len() {
        return Err(ExifToolError::parse_error(
            "iTXt chunk truncated after keyword",
        ));
    }

    let compression_flag = data[keyword_end + 1];
    let _compression_method = data[keyword_end + 2];

    // For MVP, only support uncompressed text
    if compression_flag != 0 {
        return Err(ExifToolError::parse_error(
            "iTXt compressed text not supported in MVP",
        ));
    }

    // Find language tag null separator
    let lang_start = keyword_end + 3;
    let lang_end = data[lang_start..]
        .iter()
        .position(|&b| b == 0)
        .map(|pos| lang_start + pos)
        .ok_or_else(|| {
            ExifToolError::parse_error("iTXt chunk missing language tag null separator")
        })?;

    // Find translated keyword null separator
    let trans_start = lang_end + 1;
    let trans_end = data[trans_start..]
        .iter()
        .position(|&b| b == 0)
        .map(|pos| trans_start + pos)
        .ok_or_else(|| {
            ExifToolError::parse_error("iTXt chunk missing translated keyword null separator")
        })?;

    // Extract text (everything after translated keyword null)
    let text_start = trans_end + 1;
    let text = if text_start < data.len() {
        String::from_utf8_lossy(&data[text_start..]).to_string()
    } else {
        String::new()
    };

    Ok((keyword, text))
}

/// Parses an IHDR chunk (Image Header).
///
/// # Format
///
/// ```text
/// Width: 4 bytes (u32, big-endian)
/// Height: 4 bytes (u32, big-endian)
/// Bit depth: 1 byte (1, 2, 4, 8, 16)
/// Color type: 1 byte (0=grayscale, 2=RGB, 3=palette, 4=grayscale+alpha, 6=RGBA)
/// Compression: 1 byte (0=deflate)
/// Filter: 1 byte (0=adaptive)
/// Interlace: 1 byte (0=none, 1=Adam7)
/// ```
///
/// # Parameters
///
/// - `data`: IHDR chunk data (13 bytes)
///
/// # Returns
///
/// - `Ok((width, height, bit_depth, color_type, compression, filter, interlace))`
/// - `Err`: Parse error
pub fn parse_ihdr_chunk(data: &[u8]) -> Result<(u32, u32, u8, u8, u8, u8, u8)> {
    if data.len() != 13 {
        return Err(ExifToolError::parse_error(format!(
            "IHDR chunk must be 13 bytes, got {}",
            data.len()
        )));
    }

    let reader = crate::io::EndianReader::big_endian(data);
    let width = reader
        .u32_at(0)
        .ok_or_else(|| ExifToolError::parse_error("IHDR chunk: failed to read width"))?;
    let height = reader
        .u32_at(4)
        .ok_or_else(|| ExifToolError::parse_error("IHDR chunk: failed to read height"))?;
    let bit_depth = data[8];
    let color_type = data[9];
    let compression = data[10];
    let filter = data[11];
    let interlace = data[12];

    Ok((
        width,
        height,
        bit_depth,
        color_type,
        compression,
        filter,
        interlace,
    ))
}

/// Parses a cHRM chunk (Primary Chromaticities).
///
/// # Format
///
/// Each value is a 4-byte unsigned integer representing the value × 100,000.
/// ```text
/// White Point X: 4 bytes (u32, big-endian)
/// White Point Y: 4 bytes (u32, big-endian)
/// Red X: 4 bytes (u32, big-endian)
/// Red Y: 4 bytes (u32, big-endian)
/// Green X: 4 bytes (u32, big-endian)
/// Green Y: 4 bytes (u32, big-endian)
/// Blue X: 4 bytes (u32, big-endian)
/// Blue Y: 4 bytes (u32, big-endian)
/// ```
///
/// # Parameters
///
/// - `data`: cHRM chunk data (32 bytes)
///
/// # Returns
///
/// - `Ok(ChromaticityValues)` - tuple of 8 f64 values (white_x, white_y, red_x, red_y, green_x, green_y, blue_x, blue_y)
///   where each value is a float between 0 and 1
/// - `Err`: Parse error
pub fn parse_chrm_chunk(data: &[u8]) -> Result<ChromaticityValues> {
    if data.len() != 32 {
        return Err(ExifToolError::parse_error(format!(
            "cHRM chunk must be 32 bytes, got {}",
            data.len()
        )));
    }

    // Each value is stored as integer × 100,000
    let reader = crate::io::EndianReader::big_endian(data);
    let white_x = reader.u32_at(0).unwrap_or(0) as f64 / 100000.0;
    let white_y = reader.u32_at(4).unwrap_or(0) as f64 / 100000.0;
    let red_x = reader.u32_at(8).unwrap_or(0) as f64 / 100000.0;
    let red_y = reader.u32_at(12).unwrap_or(0) as f64 / 100000.0;
    let green_x = reader.u32_at(16).unwrap_or(0) as f64 / 100000.0;
    let green_y = reader.u32_at(20).unwrap_or(0) as f64 / 100000.0;
    let blue_x = reader.u32_at(24).unwrap_or(0) as f64 / 100000.0;
    let blue_y = reader.u32_at(28).unwrap_or(0) as f64 / 100000.0;

    Ok((
        white_x, white_y, red_x, red_y, green_x, green_y, blue_x, blue_y,
    ))
}

/// Parses a pHYs chunk (Physical Pixel Dimensions).
///
/// # Format
///
/// ```text
/// Pixels per unit X: 4 bytes (u32, big-endian)
/// Pixels per unit Y: 4 bytes (u32, big-endian)
/// Unit: 1 byte (0=unknown, 1=meter)
/// ```
///
/// # Parameters
///
/// - `data`: pHYs chunk data (9 bytes)
///
/// # Returns
///
/// - `Ok((pixels_x, pixels_y, unit))`
/// - `Err`: Parse error
pub fn parse_phys_chunk(data: &[u8]) -> Result<(u32, u32, u8)> {
    if data.len() != 9 {
        return Err(ExifToolError::parse_error(format!(
            "pHYs chunk must be 9 bytes, got {}",
            data.len()
        )));
    }

    let reader = crate::io::EndianReader::big_endian(data);
    let pixels_x = reader.u32_at(0).unwrap_or(0);
    let pixels_y = reader.u32_at(4).unwrap_or(0);
    let unit = data[8];

    Ok((pixels_x, pixels_y, unit))
}

/// Parses a gAMA chunk (Image Gamma).
///
/// # Format
///
/// ```text
/// Gamma: 4 bytes (u32, big-endian) - gamma × 100,000
/// ```
///
/// The gamma value is stored as an unsigned integer representing gamma × 100,000.
/// For example, a gamma of 1/2.2 (≈0.45455) is stored as 45455.
///
/// # Parameters
///
/// - `data`: gAMA chunk data (4 bytes)
///
/// # Returns
///
/// - `Ok(gamma)`: Gamma value as f64
/// - `Err`: Parse error
pub fn parse_gama_chunk(data: &[u8]) -> Result<f64> {
    if data.len() != 4 {
        return Err(ExifToolError::parse_error(format!(
            "gAMA chunk must be 4 bytes, got {}",
            data.len()
        )));
    }

    let reader = crate::io::EndianReader::big_endian(data);
    let gamma_int = reader.u32_at(0).unwrap_or(0);
    let gamma = gamma_int as f64 / 100000.0;

    Ok(gamma)
}

/// Parses a bKGD chunk (Background Color).
///
/// Format depends on color type:
/// - Grayscale (0, 4): 2 bytes (gray value)
/// - RGB (2, 6): 6 bytes (red, green, blue)
/// - Palette (3): 1 byte (palette index)
///
/// # Parameters
///
/// - `data`: bKGD chunk data (1, 2, or 6 bytes)
///
/// # Returns
///
/// - `Ok(palette_index)` for palette images (1 byte)
/// - `Ok(gray_value)` for grayscale images (2 bytes)
/// - Returns first value for RGB (simplified)
/// - `Err`: Parse error
pub fn parse_bkgd_chunk(data: &[u8]) -> Result<u16> {
    match data.len() {
        1 => Ok(data[0] as u16), // Palette index
        2 | 6 => {
            // Gray value (2 bytes) or Red component (6 bytes, simplified to first value)
            let reader = crate::io::EndianReader::big_endian(data);
            Ok(reader.u16_at(0).unwrap_or(0))
        }
        _ => Err(ExifToolError::parse_error(format!(
            "bKGD chunk has invalid length: {}",
            data.len()
        ))),
    }
}

/// Parses a zTXt chunk (Compressed Text).
///
/// # Format
///
/// ```text
/// Keyword: 1-79 bytes (Latin-1)
/// Null separator: 1 byte (0x00)
/// Compression method: 1 byte (0=deflate)
/// Compressed text: n bytes (zlib compressed)
/// ```
///
/// # Parameters
///
/// - `data`: zTXt chunk data
///
/// # Returns
///
/// - `Ok((keyword, text))`: Extracted keyword and decompressed text as UTF-8 strings
/// - `Err`: Parse error
pub fn parse_ztxt_chunk(data: &[u8]) -> Result<(String, String)> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    // Find null separator
    let null_pos = data
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| ExifToolError::parse_error("zTXt chunk missing null separator"))?;

    if null_pos == 0 {
        return Err(ExifToolError::parse_error("zTXt chunk has empty keyword"));
    }

    // Extract keyword
    let keyword = String::from_utf8_lossy(&data[..null_pos]).to_string();

    // Check compression method
    if null_pos + 1 >= data.len() {
        return Err(ExifToolError::parse_error("zTXt chunk truncated"));
    }

    let compression_method = data[null_pos + 1];
    if compression_method != 0 {
        return Err(ExifToolError::parse_error(format!(
            "Unsupported zTXt compression method: {}",
            compression_method
        )));
    }

    // Decompress text
    let compressed_data = &data[null_pos + 2..];
    let mut decoder = ZlibDecoder::new(compressed_data);
    let mut decompressed = Vec::new();

    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| ExifToolError::parse_error(format!("Failed to decompress zTXt: {}", e)))?;

    let text = String::from_utf8_lossy(&decompressed).to_string();

    Ok((keyword, text))
}

/// Parses an sBIT chunk (Significant Bits).
///
/// # Format
///
/// Format depends on color type:
/// - Grayscale (0): 1 byte (gray)
/// - RGB (2): 3 bytes (red, green, blue)
/// - Palette (3): 3 bytes (red, green, blue)
/// - Grayscale+Alpha (4): 2 bytes (gray, alpha)
/// - RGBA (6): 4 bytes (red, green, blue, alpha)
///
/// # Parameters
///
/// - `data`: sBIT chunk data (1-4 bytes)
///
/// # Returns
///
/// - `Ok(bits)`: Vector of significant bit values
/// - `Err`: Parse error
pub fn parse_sbit_chunk(data: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() || data.len() > 4 {
        return Err(ExifToolError::parse_error(format!(
            "sBIT chunk has invalid length: {}",
            data.len()
        )));
    }

    Ok(data.to_vec())
}

/// Parses an hIST chunk (Image Histogram).
///
/// # Format
///
/// ```text
/// Frequencies: 2*n bytes (n = palette size)
/// Each frequency: 2 bytes (u16, big-endian)
/// ```
///
/// # Parameters
///
/// - `data`: hIST chunk data
///
/// # Returns
///
/// - `Ok(histogram)`: Vector of frequency values
/// - `Err`: Parse error
pub fn parse_hist_chunk(data: &[u8]) -> Result<Vec<u16>> {
    if !data.len().is_multiple_of(2) {
        return Err(ExifToolError::parse_error("hIST chunk length must be even"));
    }

    let reader = crate::io::EndianReader::big_endian(data);
    let mut histogram = Vec::new();
    for i in (0..data.len()).step_by(2) {
        if let Some(freq) = reader.u16_at(i) {
            histogram.push(freq);
        }
    }

    Ok(histogram)
}

/// Parses a tIME chunk (Last Modification Time).
///
/// # Format
///
/// ```text
/// Year: 2 bytes (u16, big-endian)
/// Month: 1 byte (1-12)
/// Day: 1 byte (1-31)
/// Hour: 1 byte (0-23)
/// Minute: 1 byte (0-59)
/// Second: 1 byte (0-60, allowing for leap second)
/// ```
///
/// # Parameters
///
/// - `data`: tIME chunk data (7 bytes)
///
/// # Returns
///
/// - `Ok(formatted_datetime)` as "YYYY:MM:DD HH:MM:SS" string
/// - `Err`: Parse error
pub fn parse_time_chunk(data: &[u8]) -> Result<String> {
    if data.len() != 7 {
        return Err(ExifToolError::parse_error(format!(
            "tIME chunk must be 7 bytes, got {}",
            data.len()
        )));
    }

    let reader = crate::io::EndianReader::big_endian(data);
    let year = reader.u16_at(0).unwrap_or(0);
    let month = data[2];
    let day = data[3];
    let hour = data[4];
    let minute = data[5];
    let second = data[6];

    Ok(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, minute, second
    ))
}

/// Parses an eXIf chunk and extracts EXIF tags using the TIFF parser.
///
/// # Format
///
/// The eXIf chunk contains raw TIFF/EXIF data:
/// ```text
/// Byte order: 2 bytes (0x4949 = "II" little-endian, 0x4D4D = "MM" big-endian)
/// TIFF header: 2 bytes (0x002A for TIFF)
/// IFD offset: 4 bytes (typically 8, pointing to first IFD)
/// IFD data: variable
/// ```
///
/// # Parameters
///
/// - `data`: eXIf chunk data (raw TIFF format)
///
/// # Returns
///
/// - `Ok(IfdEntries)`: Parsed EXIF tags
/// - `Err`: Parse error
pub fn parse_exif_chunk(data: &[u8]) -> Result<IfdEntries> {
    // Minimum TIFF header size: 2 (byte order) + 2 (magic) + 4 (offset) = 8 bytes
    if data.len() < 8 {
        return Err(ExifToolError::parse_error(
            "eXIf chunk too small for TIFF header",
        ));
    }

    // Detect byte order from first 2 bytes
    let byte_order = match &data[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => {
            return Err(ExifToolError::parse_error(
                "Invalid byte order marker in eXIf chunk",
            ));
        }
    };

    // Verify TIFF magic number (0x002A)
    let reader = match byte_order {
        ByteOrder::LittleEndian => crate::io::EndianReader::little_endian(data),
        ByteOrder::BigEndian => crate::io::EndianReader::big_endian(data),
    };

    let magic = reader.u16_at(2).ok_or_else(|| {
        ExifToolError::parse_error("eXIf chunk too small to read TIFF magic number")
    })?;

    if magic != 0x002A {
        return Err(ExifToolError::parse_error(format!(
            "Invalid TIFF magic number in eXIf chunk: 0x{:04X}",
            magic
        )));
    }

    // Read IFD offset
    let ifd_offset = reader
        .u32_at(4)
        .ok_or_else(|| ExifToolError::parse_error("eXIf chunk too small to read IFD offset"))?;

    // Create an in-memory reader for the EXIF data
    let exif_reader = ExifDataReader::new(data.to_vec());

    // Parse the IFD using the TIFF parser
    parse_ifd(&exif_reader, ifd_offset as u64, byte_order)
}

/// Simple in-memory FileReader implementation for EXIF data embedded in PNG.
///
/// This is used to wrap eXIf chunk data so it can be passed to the TIFF IFD parser.
pub(super) struct ExifDataReader {
    data: Vec<u8>,
}

impl ExifDataReader {
    pub(super) fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for ExifDataReader {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_parse_png_signature() {
        let data = PNG_SIGNATURE.to_vec();
        let result = parse_png_signature(&data);
        assert!(result.is_ok());
        let (remaining, _) = result.unwrap();
        assert_eq!(remaining.len(), 0);
    }

    #[test]
    fn test_parse_png_signature_invalid() {
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0xFF]; // Wrong last byte
        let result = parse_png_signature(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_chunk_header() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0D, // Length: 13
            b'I', b'H', b'D', b'R', // Type: IHDR
        ];

        let result = parse_chunk_header(&data);
        assert!(result.is_ok());

        let (_, (length, chunk_type)) = result.unwrap();
        assert_eq!(length, 13);
        assert_eq!(&chunk_type, b"IHDR");
    }

    #[test]
    fn test_parse_chunk() {
        // Create a minimal chunk: IHDR with 13 bytes of data
        let mut data = Vec::new();

        // Chunk length (13 bytes)
        data.extend_from_slice(&13u32.to_be_bytes());

        // Chunk type (IHDR)
        data.extend_from_slice(b"IHDR");

        // Chunk data (13 bytes of zeros)
        data.extend_from_slice(&[0u8; 13]);

        // CRC (dummy value)
        data.extend_from_slice(&0x12345678u32.to_be_bytes());

        let reader = TestReader::new(data);
        let result = parse_chunk(&reader, 0);
        assert!(result.is_ok());

        let (next_offset, chunk) = result.unwrap();
        assert_eq!(chunk.chunk_type, *b"IHDR");
        assert_eq!(chunk.data.len(), 13);
        assert_eq!(chunk.crc, 0x12345678);
        assert_eq!(next_offset, 4 + 4 + 13 + 4); // Total chunk size
    }

    #[test]
    fn test_parse_text_chunk() {
        let data = b"Author\0John Doe";
        let result = parse_text_chunk(data);
        assert!(result.is_ok());

        let (keyword, text) = result.unwrap();
        assert_eq!(keyword, "Author");
        assert_eq!(text, "John Doe");
    }

    #[test]
    fn test_parse_text_chunk_missing_null() {
        let data = b"AuthorJohn Doe";
        let result = parse_text_chunk(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_text_chunk_empty_keyword() {
        let data = b"\0John Doe";
        let result = parse_text_chunk(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_itxt_chunk_uncompressed() {
        // Format: keyword\0compression_flag\0compression_method\0language\0translated\0text
        let mut data = Vec::new();
        data.extend_from_slice(b"Title"); // keyword
        data.push(0); // null
        data.push(0); // compression flag = 0 (uncompressed)
        data.push(0); // compression method
        data.extend_from_slice(b"en-US"); // language tag
        data.push(0); // null
        data.extend_from_slice(b"Title"); // translated keyword
        data.push(0); // null
        data.extend_from_slice(b"My Image Title"); // text

        let result = parse_itxt_chunk(&data);
        assert!(result.is_ok());

        let (keyword, text) = result.unwrap();
        assert_eq!(keyword, "Title");
        assert_eq!(text, "My Image Title");
    }

    #[test]
    fn test_parse_itxt_chunk_compressed() {
        // Test that compressed iTXt returns an error in MVP
        let mut data = Vec::new();
        data.extend_from_slice(b"Title");
        data.push(0);
        data.push(1); // compression flag = 1 (compressed)
        data.push(0);

        let result = parse_itxt_chunk(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_exif_chunk_little_endian() {
        // Create minimal TIFF/EXIF data with little-endian byte order
        let mut data = Vec::new();

        // TIFF header
        data.extend_from_slice(b"II"); // Little-endian
        data.extend_from_slice(&0x002Au16.to_le_bytes()); // Magic
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD offset

        // IFD with 1 entry
        data.extend_from_slice(&1u16.to_le_bytes()); // Entry count

        // Tag entry: Make (0x010F) = "Test"
        data.extend_from_slice(&0x010Fu16.to_le_bytes()); // Tag ID
        data.extend_from_slice(&2u16.to_le_bytes()); // Type: ASCII
        data.extend_from_slice(&5u32.to_le_bytes()); // Count: 5 (including null)
        data.extend_from_slice(&26u32.to_le_bytes()); // Offset to value

        // Next IFD offset: 0
        data.extend_from_slice(&0u32.to_le_bytes());

        // Value data at offset 26
        data.extend_from_slice(b"Test\0");

        let result = parse_exif_chunk(&data);
        assert!(result.is_ok());

        let tags = result.unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].0, 0x010F); // Make tag
        assert_eq!(tags[0].3.as_ref(), b"Test\0");
    }

    #[test]
    fn test_parse_exif_chunk_invalid_magic() {
        let mut data = Vec::new();
        data.extend_from_slice(b"II"); // Little-endian
        data.extend_from_slice(&0x0042u16.to_le_bytes()); // Wrong magic
        data.extend_from_slice(&8u32.to_le_bytes());

        let result = parse_exif_chunk(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_png_chunk_type_str() {
        let chunk = PngChunk {
            chunk_type: *b"tEXt",
            data: Vec::new(),
            crc: 0,
        };
        assert_eq!(chunk.type_str(), "tEXt");
    }

    #[test]
    fn test_png_chunk_is_text_chunk() {
        let text_chunk = PngChunk {
            chunk_type: *b"tEXt",
            data: Vec::new(),
            crc: 0,
        };
        assert!(text_chunk.is_text_chunk());

        let ihdr_chunk = PngChunk {
            chunk_type: *b"IHDR",
            data: Vec::new(),
            crc: 0,
        };
        assert!(!ihdr_chunk.is_text_chunk());
    }

    #[test]
    fn test_png_chunk_is_exif_chunk() {
        let exif_chunk = PngChunk {
            chunk_type: *b"eXIf",
            data: Vec::new(),
            crc: 0,
        };
        assert!(exif_chunk.is_exif_chunk());

        let text_chunk = PngChunk {
            chunk_type: *b"tEXt",
            data: Vec::new(),
            crc: 0,
        };
        assert!(!text_chunk.is_exif_chunk());
    }

    #[test]
    fn test_parse_gama_chunk() {
        // Test gamma value of 2.2 (stored as 220000)
        let data = 220000u32.to_be_bytes();
        let result = parse_gama_chunk(&data);
        assert!(result.is_ok());
        let gamma = result.unwrap();
        assert!((gamma - 2.2).abs() < 0.0001);

        // Test gamma value of 1/2.2 ≈ 0.45455 (stored as 45455)
        let data = 45455u32.to_be_bytes();
        let result = parse_gama_chunk(&data);
        assert!(result.is_ok());
        let gamma = result.unwrap();
        assert!((gamma - 0.45455).abs() < 0.00001);
    }

    #[test]
    fn test_parse_gama_chunk_invalid_length() {
        let data = [0u8; 3]; // Wrong length
        let result = parse_gama_chunk(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ztxt_chunk() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        // Create compressed text data
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"This is compressed text").unwrap();
        let compressed = encoder.finish().unwrap();

        // Build zTXt chunk data: keyword\0compression_method\0compressed_data
        let mut data = Vec::new();
        data.extend_from_slice(b"Description");
        data.push(0); // null
        data.push(0); // compression method = 0 (deflate)
        data.extend_from_slice(&compressed);

        let result = parse_ztxt_chunk(&data);
        assert!(result.is_ok());

        let (keyword, text) = result.unwrap();
        assert_eq!(keyword, "Description");
        assert_eq!(text, "This is compressed text");
    }

    #[test]
    fn test_parse_sbit_chunk() {
        // Test grayscale (1 byte)
        let data = vec![8];
        let result = parse_sbit_chunk(&data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![8]);

        // Test RGB (3 bytes)
        let data = vec![8, 8, 8];
        let result = parse_sbit_chunk(&data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![8, 8, 8]);

        // Test RGBA (4 bytes)
        let data = vec![8, 8, 8, 8];
        let result = parse_sbit_chunk(&data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![8, 8, 8, 8]);

        // Test invalid (too many bytes)
        let data = vec![8, 8, 8, 8, 8];
        let result = parse_sbit_chunk(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_hist_chunk() {
        // Create histogram with 3 entries
        let mut data = Vec::new();
        data.extend_from_slice(&100u16.to_be_bytes());
        data.extend_from_slice(&200u16.to_be_bytes());
        data.extend_from_slice(&300u16.to_be_bytes());

        let result = parse_hist_chunk(&data);
        assert!(result.is_ok());

        let histogram = result.unwrap();
        assert_eq!(histogram.len(), 3);
        assert_eq!(histogram[0], 100);
        assert_eq!(histogram[1], 200);
        assert_eq!(histogram[2], 300);
    }

    #[test]
    fn test_parse_hist_chunk_invalid_length() {
        // Odd length should fail
        let data = vec![0, 1, 2];
        let result = parse_hist_chunk(&data);
        assert!(result.is_err());
    }
}
