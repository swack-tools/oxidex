//! JPEG segment marker parsing
//!
//! This module handles parsing of JPEG segment markers using nom parser combinators.
//! JPEG files consist of a sequence of segments, each with a marker, optional length,
//! and optional data payload.
//!
//! # JPEG Segment Structure
//!
//! Most segments follow this structure:
//! - **Marker**: 2 bytes (0xFFXX)
//! - **Length**: 2 bytes (big-endian), includes length field but NOT marker
//! - **Data**: Variable-length payload (length - 2 bytes)
//!
//! Special markers (SOI, EOI, RST0-RST7) have no length or data fields.
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::parsers::jpeg::segment_parser::parse_segments;
//! use exiftool_rs::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("image.jpg"))?;
//! let segments = parse_segments(&reader)?;
//!
//! // Find APP1 segments (EXIF/XMP)
//! for segment in segments.iter() {
//!     if segment.marker == 0xFFE1 {
//!         println!("Found APP1 segment at offset {}", segment.offset);
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use crate::core::FileReader;
use crate::error::ExifToolError;
use nom::{
    bytes::complete::{tag, take},
    number::complete::be_u16,
    IResult,
};

// JPEG marker constants
const MARKER_PREFIX: u8 = 0xFF;
const SOI_MARKER: u16 = 0xFFD8; // Start of Image
const EOI_MARKER: u16 = 0xFFD9; // End of Image
const APP1_MARKER: u16 = 0xFFE1; // APP1 (EXIF/XMP)

// Restart markers (RST0-RST7) have no length field
const RST0_MARKER: u16 = 0xFFD0;
const RST7_MARKER: u16 = 0xFFD7;

/// Represents a single JPEG segment with marker, offset, and data.
///
/// The `data` field is a borrowed slice from the underlying file reader,
/// enabling zero-copy parsing. The lifetime `'a` ties the segment data
/// to the reader's lifetime.
///
/// # Fields
///
/// - `marker`: The 2-byte JPEG marker (e.g., 0xFFE1 for APP1)
/// - `offset`: Byte offset of the marker in the file
/// - `data`: Borrowed slice containing the segment's payload (excludes marker and length)
#[derive(Debug, Clone, PartialEq)]
pub struct Segment<'a> {
    /// The 2-byte JPEG marker (e.g., 0xFFE1 for APP1)
    pub marker: u16,
    /// Byte offset of the marker in the file
    pub offset: u64,
    /// Segment data payload (excludes marker and length field)
    pub data: &'a [u8],
}

impl<'a> Segment<'a> {
    /// Creates a new Segment.
    ///
    /// # Parameters
    ///
    /// - `marker`: The 2-byte JPEG marker
    /// - `offset`: Byte offset in the file
    /// - `data`: Borrowed slice of segment data
    pub fn new(marker: u16, offset: u64, data: &'a [u8]) -> Self {
        Self {
            marker,
            offset,
            data,
        }
    }

    /// Returns true if this is an APP1 segment (0xFFE1).
    ///
    /// APP1 segments contain EXIF and XMP metadata.
    pub fn is_app1(&self) -> bool {
        self.marker == APP1_MARKER
    }

    /// Returns true if this is a Start of Image (SOI) marker.
    pub fn is_soi(&self) -> bool {
        self.marker == SOI_MARKER
    }

    /// Returns true if this is an End of Image (EOI) marker.
    pub fn is_eoi(&self) -> bool {
        self.marker == EOI_MARKER
    }
}

/// Parses all segments from a JPEG file.
///
/// This function reads the entire file and extracts all JPEG segments using
/// nom parser combinators. It handles both standard segments (with length fields)
/// and special markers (SOI, EOI, RST0-RST7) that have no length.
///
/// # Parameters
///
/// - `reader`: A reference to a FileReader implementation
///
/// # Returns
///
/// - `Ok(Vec<Segment>)`: Vector of all parsed segments
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Errors
///
/// Returns an error if:
/// - File is too small to be a valid JPEG (< 2 bytes)
/// - SOI marker (0xFFD8) is not found at the start
/// - Segment length exceeds remaining file size (truncated)
/// - Invalid marker or malformed segment structure
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::jpeg::segment_parser::parse_segments;
/// use exiftool_rs::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("image.jpg"))?;
/// let segments = parse_segments(&reader)?;
/// println!("Found {} segments", segments.len());
/// # Ok(())
/// # }
/// ```
pub fn parse_segments<'a>(reader: &'a dyn FileReader) -> Result<Vec<Segment<'a>>, ExifToolError> {
    let size = reader.size() as usize;

    // JPEG must be at least 2 bytes (SOI marker)
    if size < 2 {
        return Err(ExifToolError::parse_error(
            "File too small to be a valid JPEG",
        ));
    }

    // Read entire file into a slice (zero-copy with MMapReader)
    let data = reader.read(0, size)?;

    // Parse SOI marker
    let (remaining, _) = parse_soi_marker(data)
        .map_err(|e| ExifToolError::parse_error(format!("Failed to parse SOI marker: {}", e)))?;

    // Parse all segments
    let mut segments = Vec::new();

    // Add SOI marker as first segment
    segments.push(Segment::new(SOI_MARKER, 0, &[]));

    let mut current = remaining;
    let mut current_offset = 2u64; // After SOI marker

    while !current.is_empty() {
        match parse_segment(current) {
            Ok((remaining, (marker, segment_data))) => {
                // Calculate segment offset (position of marker)
                let segment_offset = current_offset;

                // Add segment to vector (all segments after SOI)
                segments.push(Segment::new(marker, segment_offset, segment_data));

                // Check for EOI marker
                if marker == EOI_MARKER {
                    break;
                }

                // Calculate bytes consumed
                let consumed = current.len() - remaining.len();
                current_offset += consumed as u64;
                current = remaining;
            }
            Err(e) => {
                // If we've only found SOI (or no real segments), this is an error
                // If we've found real content segments, it might be trailing data
                if segments.len() <= 1 {
                    return Err(ExifToolError::parse_error(format!(
                        "Failed to parse JPEG segment at offset {}: {}",
                        current_offset, e
                    )));
                }
                break;
            }
        }
    }

    Ok(segments)
}

/// Parses the JPEG Start of Image (SOI) marker.
///
/// # Parameters
///
/// - `input`: Byte slice starting at the beginning of the JPEG file
///
/// # Returns
///
/// - `Ok((remaining, ()))`: Remaining bytes after SOI marker
/// - `Err`: Parse error if SOI marker is not found
fn parse_soi_marker(input: &[u8]) -> IResult<&[u8], ()> {
    let (input, _) = tag(&[0xFF, 0xD8])(input)?;
    Ok((input, ()))
}

/// Parses a single JPEG segment (marker + optional length + optional data).
///
/// This function handles both:
/// - Standard segments with length field (APP0-APP15, DQT, DHT, etc.)
/// - Special markers without length (SOI, EOI, RST0-RST7)
///
/// # Parameters
///
/// - `input`: Byte slice positioned at the start of a segment marker
///
/// # Returns
///
/// - `Ok((remaining, (marker, data)))`: Remaining bytes, marker value, and segment data
/// - `Err`: Parse error if segment is malformed
fn parse_segment(input: &[u8]) -> IResult<&[u8], (u16, &[u8])> {
    // Parse marker (2 bytes)
    let (input, marker) = be_u16(input)?;

    // Check if this marker has no length field
    if is_standalone_marker(marker) {
        // SOI, EOI, RST0-RST7 have no length or data
        return Ok((input, (marker, &[])));
    }

    // Parse length (2 bytes, big-endian)
    let (input, length) = be_u16(input)?;

    // Length includes itself (2 bytes) but not the marker
    // So data size is (length - 2)
    if length < 2 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Verify,
        )));
    }

    let data_size = (length - 2) as usize;

    // Parse segment data
    let (input, data) = take(data_size)(input)?;

    Ok((input, (marker, data)))
}

/// Returns true if the marker is a standalone marker (no length field).
///
/// Standalone markers include:
/// - SOI (0xFFD8)
/// - EOI (0xFFD9)
/// - RST0-RST7 (0xFFD0-0xFFD7)
fn is_standalone_marker(marker: u16) -> bool {
    marker == SOI_MARKER || marker == EOI_MARKER || (RST0_MARKER..=RST7_MARKER).contains(&marker)
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

    /// Creates a minimal valid JPEG with SOI + APP1 + EOI
    fn create_valid_jpeg() -> Vec<u8> {
        let mut data = Vec::new();

        // SOI marker (0xFFD8)
        data.extend_from_slice(&[0xFF, 0xD8]);

        // APP1 marker (0xFFE1)
        data.extend_from_slice(&[0xFF, 0xE1]);
        // Length: 12 bytes (includes length field itself)
        data.extend_from_slice(&[0x00, 0x0C]);
        // 10 bytes of data (EXIF header)
        data.extend_from_slice(&[0x45, 0x78, 0x69, 0x66, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04]);

        // EOI marker (0xFFD9)
        data.extend_from_slice(&[0xFF, 0xD9]);

        data
    }

    /// Creates a JPEG with multiple segments (5 total: SOI + APP0 + APP1 + APP2 + EOI)
    fn create_jpeg_with_multiple_segments() -> Vec<u8> {
        let mut data = Vec::new();

        // SOI marker
        data.extend_from_slice(&[0xFF, 0xD8]);

        // APP0 marker (0xFFE0) - JFIF
        data.extend_from_slice(&[0xFF, 0xE0]);
        data.extend_from_slice(&[0x00, 0x06]); // Length: 6 bytes
        data.extend_from_slice(&[0x4A, 0x46, 0x49, 0x46]); // "JFIF"

        // APP1 marker (0xFFE1) - EXIF
        data.extend_from_slice(&[0xFF, 0xE1]);
        data.extend_from_slice(&[0x00, 0x08]); // Length: 8 bytes
        data.extend_from_slice(&[0x45, 0x78, 0x69, 0x66, 0x00, 0x00]); // "Exif\0\0"

        // APP2 marker (0xFFE2)
        data.extend_from_slice(&[0xFF, 0xE2]);
        data.extend_from_slice(&[0x00, 0x04]); // Length: 4 bytes
        data.extend_from_slice(&[0xAA, 0xBB]); // 2 bytes of data

        // EOI marker
        data.extend_from_slice(&[0xFF, 0xD9]);

        data
    }

    /// Creates a truncated JPEG (missing segment data)
    fn create_truncated_jpeg() -> Vec<u8> {
        let mut data = Vec::new();

        // SOI marker
        data.extend_from_slice(&[0xFF, 0xD8]);

        // APP1 marker
        data.extend_from_slice(&[0xFF, 0xE1]);
        // Length: 100 bytes (but we won't provide the data)
        data.extend_from_slice(&[0x00, 0x64]);
        // Only 5 bytes of data instead of 98
        data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05]);

        data
    }

    #[test]
    fn test_segment_creation() {
        let data = b"test data";
        let segment = Segment::new(0xFFE1, 100, data);

        assert_eq!(segment.marker, 0xFFE1);
        assert_eq!(segment.offset, 100);
        assert_eq!(segment.data, b"test data");
    }

    #[test]
    fn test_segment_is_app1() {
        let segment = Segment::new(0xFFE1, 0, &[]);
        assert!(segment.is_app1());

        let segment = Segment::new(0xFFE0, 0, &[]);
        assert!(!segment.is_app1());
    }

    #[test]
    fn test_segment_is_soi() {
        let segment = Segment::new(0xFFD8, 0, &[]);
        assert!(segment.is_soi());

        let segment = Segment::new(0xFFE1, 0, &[]);
        assert!(!segment.is_soi());
    }

    #[test]
    fn test_segment_is_eoi() {
        let segment = Segment::new(0xFFD9, 0, &[]);
        assert!(segment.is_eoi());

        let segment = Segment::new(0xFFE1, 0, &[]);
        assert!(!segment.is_eoi());
    }

    #[test]
    fn test_parse_valid_jpeg() {
        let jpeg_data = create_valid_jpeg();
        let reader = TestReader::new(jpeg_data);

        let segments = parse_segments(&reader).expect("Failed to parse valid JPEG");

        // Should have 3 segments: SOI, APP1, EOI
        assert_eq!(segments.len(), 3);

        // Check SOI
        assert_eq!(segments[0].marker, 0xFFD8);
        assert_eq!(segments[0].offset, 0);
        assert_eq!(segments[0].data.len(), 0);

        // Check APP1
        assert_eq!(segments[1].marker, 0xFFE1);
        assert_eq!(segments[1].offset, 2);
        assert_eq!(segments[1].data.len(), 10);
        assert_eq!(&segments[1].data[0..4], b"Exif");

        // Check EOI
        assert_eq!(segments[2].marker, 0xFFD9);
        assert_eq!(segments[2].data.len(), 0);
    }

    #[test]
    fn test_parse_multiple_segments() {
        let jpeg_data = create_jpeg_with_multiple_segments();
        let reader = TestReader::new(jpeg_data);

        let segments = parse_segments(&reader).expect("Failed to parse JPEG");

        // Should have 5 segments: SOI, APP0, APP1, APP2, EOI
        assert_eq!(segments.len(), 5);

        // Verify markers
        assert_eq!(segments[0].marker, 0xFFD8); // SOI
        assert_eq!(segments[1].marker, 0xFFE0); // APP0
        assert_eq!(segments[2].marker, 0xFFE1); // APP1
        assert_eq!(segments[3].marker, 0xFFE2); // APP2
        assert_eq!(segments[4].marker, 0xFFD9); // EOI

        // Verify APP1 data
        assert_eq!(&segments[2].data[0..4], b"Exif");
    }

    #[test]
    fn test_parse_truncated_jpeg() {
        let jpeg_data = create_truncated_jpeg();
        let reader = TestReader::new(jpeg_data);

        let result = parse_segments(&reader);

        // Should return an error for truncated segment
        assert!(result.is_err());
        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(message.contains("Failed to parse JPEG segment"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_parse_invalid_soi() {
        // File doesn't start with SOI marker
        let jpeg_data = vec![0x00, 0x00, 0xFF, 0xE1];
        let reader = TestReader::new(jpeg_data);

        let result = parse_segments(&reader);

        assert!(result.is_err());
        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(message.contains("SOI marker"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_parse_empty_file() {
        let jpeg_data = vec![];
        let reader = TestReader::new(jpeg_data);

        let result = parse_segments(&reader);

        assert!(result.is_err());
        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(message.contains("too small"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_parse_file_too_small() {
        let jpeg_data = vec![0xFF]; // Only 1 byte
        let reader = TestReader::new(jpeg_data);

        let result = parse_segments(&reader);

        assert!(result.is_err());
        if let Err(ExifToolError::ParseError { message, .. }) = result {
            assert!(message.contains("too small"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_parse_segment_with_zero_length() {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xD8]); // SOI
        data.extend_from_slice(&[0xFF, 0xE1]); // APP1
        data.extend_from_slice(&[0x00, 0x00]); // Invalid length: 0

        let reader = TestReader::new(data);
        let result = parse_segments(&reader);

        // Should fail due to invalid length
        assert!(result.is_err());
    }

    #[test]
    fn test_standalone_marker_detection() {
        assert!(is_standalone_marker(0xFFD8)); // SOI
        assert!(is_standalone_marker(0xFFD9)); // EOI
        assert!(is_standalone_marker(0xFFD0)); // RST0
        assert!(is_standalone_marker(0xFFD7)); // RST7

        assert!(!is_standalone_marker(0xFFE1)); // APP1
        assert!(!is_standalone_marker(0xFFE0)); // APP0
    }

    #[test]
    fn test_jpeg_with_restart_marker() {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xD8]); // SOI
        data.extend_from_slice(&[0xFF, 0xD0]); // RST0 (no length)
        data.extend_from_slice(&[0xFF, 0xD9]); // EOI

        let reader = TestReader::new(data);
        let segments = parse_segments(&reader).expect("Failed to parse");

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[1].marker, 0xFFD0); // RST0
        assert_eq!(segments[1].data.len(), 0); // No data
    }
}
