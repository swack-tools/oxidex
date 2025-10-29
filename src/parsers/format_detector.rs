//! Format detection via magic byte analysis
//!
//! This module provides format detection capabilities for determining file types
//! by examining magic bytes (file signatures) at the beginning of files.
//!
//! # Architectural Role
//!
//! The format detector is part of the **infrastructure layer** and serves as the
//! entry point for the parsing pipeline. It uses the `FileReader` port to read
//! magic bytes and returns a `FileFormat` enum variant that routes to the
//! appropriate format parser.
//!
//! # Supported Formats
//!
//! The detector currently identifies:
//! - JPEG: 0xFF 0xD8 0xFF
//! - TIFF (Little-Endian): 0x49 0x49 0x2A 0x00
//! - TIFF (Big-Endian): 0x4D 0x4D 0x00 0x2A
//! - PNG: 0x89 0x50 0x4E 0x47
//! - PDF: 0x25 0x50 0x44 0x46
//!
//! Unknown formats return `FileFormat::Unknown`.
//!
//! # Examples
//!
//! ```no_run
//! use exiftool_rs::parsers::format_detector::detect_format;
//! use exiftool_rs::io::MMapReader;
//! use std::path::Path;
//!
//! # fn example() -> std::io::Result<()> {
//! let reader = MMapReader::new(Path::new("image.jpg"))?;
//! let format = detect_format(&reader)?;
//! println!("Detected format: {}", format);
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader};
use std::io;

/// Detects the file format by examining magic bytes.
///
/// This function reads the first 16 bytes of the file (or fewer if the file is smaller)
/// and matches them against known format signatures. Format detection is performed
/// by checking byte sequences in order from most specific to least specific.
///
/// # Arguments
///
/// * `reader` - A file reader providing access to file contents via the FileReader port
///
/// # Returns
///
/// * `Ok(FileFormat)` - The detected format, or `FileFormat::Unknown` if unrecognized
/// * `Err(io::Error)` - An I/O error occurred while reading the file
///
/// # Error Handling
///
/// This function gracefully handles files smaller than 16 bytes by reading only the
/// available bytes and attempting format detection with the partial data. Empty files
/// return `Ok(FileFormat::Unknown)`.
///
/// # Magic Byte Sequences
///
/// - **JPEG**: `FF D8 FF` (3 bytes) - JPEG Start of Image marker
/// - **TIFF (LE)**: `49 49 2A 00` (4 bytes) - "II" + magic number 42 (little-endian)
/// - **TIFF (BE)**: `4D 4D 00 2A` (4 bytes) - "MM" + magic number 42 (big-endian)
/// - **PNG**: `89 50 4E 47` (4 bytes) - PNG signature (first 4 of 8 bytes)
/// - **PDF**: `25 50 44 46` (4 bytes) - "%PDF" ASCII signature
///
/// # Examples
///
/// ```no_run
/// use exiftool_rs::parsers::format_detector::detect_format;
/// use exiftool_rs::io::MMapReader;
/// use exiftool_rs::core::FileFormat;
/// use std::path::Path;
///
/// # fn example() -> std::io::Result<()> {
/// let reader = MMapReader::new(Path::new("photo.jpg"))?;
/// let format = detect_format(&reader)?;
///
/// match format {
///     FileFormat::JPEG => println!("JPEG image detected"),
///     FileFormat::PNG => println!("PNG image detected"),
///     FileFormat::TIFF => println!("TIFF image detected"),
///     FileFormat::PDF => println!("PDF document detected"),
///     FileFormat::Unknown => println!("Unknown or unsupported format"),
///     _ => println!("Other format detected"),
/// }
/// # Ok(())
/// # }
/// ```
pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat> {
    // Attempt to read 16 bytes for magic byte detection
    // If the file is smaller, read only what's available
    let magic_bytes = match reader.read(0, 16) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            // File is smaller than 16 bytes, try reading available bytes
            let size = reader.size() as usize;
            if size == 0 {
                // Empty file - cannot determine format
                return Ok(FileFormat::Unknown);
            }
            // Read whatever is available
            reader.read(0, size)?
        }
        Err(e) => return Err(e),
    };

    // Check for empty result (edge case)
    if magic_bytes.is_empty() {
        return Ok(FileFormat::Unknown);
    }

    // Check formats in order from most specific to least specific
    // Start with 4-byte signatures, then 3-byte signatures

    // TIFF Little-Endian: 0x49 0x49 0x2A 0x00 ("II" + 42 in LE)
    if magic_bytes.len() >= 4 && magic_bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00]) {
        return Ok(FileFormat::TIFF);
    }

    // TIFF Big-Endian: 0x4D 0x4D 0x00 0x2A ("MM" + 42 in BE)
    if magic_bytes.len() >= 4 && magic_bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        return Ok(FileFormat::TIFF);
    }

    // PNG: 0x89 0x50 0x4E 0x47 (first 4 bytes of 8-byte signature)
    if magic_bytes.len() >= 4 && magic_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Ok(FileFormat::PNG);
    }

    // PDF: 0x25 0x50 0x44 0x46 ("%PDF")
    if magic_bytes.len() >= 4 && magic_bytes.starts_with(&[0x25, 0x50, 0x44, 0x46]) {
        return Ok(FileFormat::PDF);
    }

    // JPEG: 0xFF 0xD8 0xFF (SOI marker + start of next marker)
    // Note: JPEG signature is 3 bytes, checked after 4-byte signatures
    if magic_bytes.len() >= 3 && magic_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(FileFormat::JPEG);
    }

    // No known format matched
    Ok(FileFormat::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// Test implementation of FileReader for unit testing
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
            let end = start.saturating_add(length).min(self.data.len());

            if start > self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "offset beyond end of data",
                ));
            }

            if end > self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "read beyond end of data",
                ));
            }

            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[test]
    fn test_detect_jpeg() {
        // Valid JPEG magic bytes: FF D8 FF
        let data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_detect_tiff_little_endian() {
        // TIFF Little-Endian: 49 49 2A 00 ("II" + 42 in LE)
        let data = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_detect_tiff_big_endian() {
        // TIFF Big-Endian: 4D 4D 00 2A ("MM" + 42 in BE)
        let data = vec![0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_detect_png() {
        // PNG signature (first 8 bytes, but we only check first 4)
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PNG);
    }

    #[test]
    fn test_detect_pdf() {
        // PDF signature: "%PDF"
        let data = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x34];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }

    #[test]
    fn test_detect_unknown() {
        // Random bytes that don't match any format
        let data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_empty_file() {
        // Empty file (0 bytes)
        let data = vec![];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_file_too_small_one_byte() {
        // File with only 1 byte (smaller than any magic byte sequence)
        let data = vec![0xFF];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_file_too_small_two_bytes() {
        // File with 2 bytes (still too small for any format)
        let data = vec![0xFF, 0xD8];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_short_file_matches_jpeg() {
        // File with exactly 3 bytes that match JPEG
        let data = vec![0xFF, 0xD8, 0xFF];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_short_file_matches_pdf() {
        // File with exactly 4 bytes that match PDF
        let data = vec![0x25, 0x50, 0x44, 0x46];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }

    #[test]
    fn test_jpeg_with_padding() {
        // JPEG with additional data after magic bytes
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE1];
        data.extend_from_slice(&[0x00; 20]); // Add padding
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_tiff_little_endian_minimal() {
        // Minimal TIFF LE header (exactly 4 bytes)
        let data = vec![0x49, 0x49, 0x2A, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_tiff_big_endian_minimal() {
        // Minimal TIFF BE header (exactly 4 bytes)
        let data = vec![0x4D, 0x4D, 0x00, 0x2A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_png_full_signature() {
        // Full 8-byte PNG signature
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PNG);
    }

    #[test]
    fn test_partial_match_not_detected() {
        // Bytes that partially match JPEG but not completely
        let data = vec![0xFF, 0xD8, 0x00, 0x00]; // Missing third 0xFF
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_pdf_with_version() {
        // PDF with version string "%PDF-1.7"
        let data = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x37, 0x0A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }
}
