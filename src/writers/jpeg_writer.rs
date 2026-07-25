//! JPEG EXIF/XMP segment writing
//!
//! This module handles writing metadata to JPEG files, specifically replacing or
//! inserting EXIF APP1 segments with modified metadata.
//!
//! # JPEG Structure with EXIF
//!
//! JPEG files consist of a sequence of segments:
//! - **SOI marker**: 0xFFD8 (Start of Image) - 2 bytes, no length field
//! - **Segments**: Each segment has:
//!   - **Marker**: 2 bytes (0xFFXX)
//!   - **Length**: 2 bytes (big-endian), includes length field but NOT marker
//!   - **Data**: Variable-length payload (length - 2 bytes)
//! - **EOI marker**: 0xFFD9 (End of Image) - 2 bytes, no length field
//!
//! # EXIF APP1 Segment Structure
//!
//! EXIF metadata is stored in an APP1 segment (marker 0xFFE1):
//! 1. Marker: 0xFFE1 (2 bytes)
//! 2. Length: 2 bytes (big-endian, includes itself + header + TIFF data, but NOT marker)
//! 3. EXIF identifier: "Exif\0\0" (6 bytes)
//! 4. TIFF IFD data: Complete TIFF structure with header and IFD
//!
//! # Example
//!
//! ```no_run
//! use oxidex::writers::jpeg_writer::write_exif_to_jpeg;
//! use oxidex::core::metadata_map::MetadataMap;
//! use oxidex::core::tag_value::TagValue;
//! use oxidex::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("image.jpg"))?;
//! let mut metadata = MetadataMap::new();
//! metadata.insert("EXIF:Artist", TagValue::new_string("John Doe"));
//!
//! let modified_jpeg = write_exif_to_jpeg(&reader, &metadata)?;
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use crate::core::FileReader;
use crate::core::metadata_map::MetadataMap;
use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::{Segment, parse_segments};

/// EXIF identifier that appears at the start of EXIF APP1 segment data
const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";

/// APP1 marker (0xFFE1) - used for EXIF and XMP
const APP1_MARKER: u16 = 0xFFE1;

/// Start of Image marker (0xFFD8)
const SOI_MARKER: u16 = 0xFFD8;

/// End of Image marker (0xFFD9)
const EOI_MARKER: u16 = 0xFFD9;

/// Start of Scan marker (0xFFDA) - entropy-coded image data follows its header
const SOS_MARKER: u16 = 0xFFDA;

/// Restart markers (RST0-RST7) have no length field
const RST0_MARKER: u16 = 0xFFD0;
const RST7_MARKER: u16 = 0xFFD7;

/// Writes modified EXIF metadata to a JPEG file structure.
///
/// This function:
/// 1. Parses the original JPEG using segment_parser
/// 2. Serializes modified EXIF tags using tiff_writer
/// 3. Replaces the EXIF APP1 segment (or inserts if not present)
/// 4. Returns the complete modified JPEG as Vec<u8>
///
/// # EXIF Segment Construction
///
/// The new EXIF APP1 segment is constructed as follows:
/// - **TIFF Header**: 8 bytes (byte order marker + magic + IFD offset)
/// - **TIFF IFD**: Serialized using `serialize_ifd()`
/// - **EXIF Identifier**: "Exif\0\0" prefix (6 bytes)
/// - **Segment Length**: Calculated as 2 + 6 + 8 + IFD size
///
/// # Segment Preservation
///
/// - All non-EXIF segments are preserved in their original positions
/// - If EXIF segment exists, it's replaced with the new one
/// - If EXIF segment doesn't exist, new one is inserted after APP0 (or after SOI)
/// - Multiple APP1 segments (EXIF + XMP) are handled correctly
///
/// # Parameters
///
/// - `reader`: FileReader for reading the original JPEG file
/// - `metadata`: MetadataMap containing EXIF tags to write (only "EXIF:" tags are processed)
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: Complete modified JPEG file as bytes
/// - `Err(ExifToolError)`: If parsing fails or JPEG structure is invalid
///
/// # Errors
///
/// Returns an error if:
/// - The file is not a valid JPEG (missing SOI marker)
/// - Segment parsing fails (truncated or malformed segments)
/// - TIFF IFD serialization fails (invalid tag values)
/// - Resulting JPEG would be invalid (e.g., segments too large)
///
/// # Example
///
/// ```no_run
/// use oxidex::writers::jpeg_writer::write_exif_to_jpeg;
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("image.jpg"))?;
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Artist", TagValue::new_string("John Doe"));
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
///
/// let modified_jpeg = write_exif_to_jpeg(&reader, &metadata)?;
/// // Write modified_jpeg to file...
/// # Ok(())
/// # }
/// ```
pub fn write_exif_to_jpeg(reader: &dyn FileReader, metadata: &MetadataMap) -> Result<Vec<u8>> {
    // Step 1: Parse original JPEG segments
    let segments = parse_segments(reader)?;

    // Step 2: Build new EXIF APP1 segment surgically (raw carry-over,
    // original byte order, MakerNotes preserved) — issue #20
    let file_size = reader.size() as usize;
    let file_bytes = reader.read(0, file_size)?;
    let new_exif_segment = crate::writers::exif_surgical::rewrite_jpeg_exif(file_bytes, metadata)?;

    // Step 3: Entropy-coded scan data follows the SOS header and is not
    // segment-structured; the parser cannot represent it (it either stops or
    // misreads scan bytes as segments). Reconstruct only the segments up to
    // and including the SOS header, and copy everything after it verbatim.
    let sos_index = segments.iter().position(|seg| seg.marker == SOS_MARKER);
    let (head_segments, raw_tail) = match sos_index {
        Some(index) => {
            let sos = &segments[index];
            // marker (2) + length field (2) + scan header payload
            let tail_start = sos.offset as usize + 4 + sos.data.len();
            let file_size = reader.size() as usize;
            let tail = if tail_start < file_size {
                &reader.read(0, file_size)?[tail_start..]
            } else {
                &[][..]
            };
            (&segments[..=index], tail)
        }
        None => (&segments[..], &[][..]),
    };

    // Step 4: Find existing EXIF segment position (metadata segments always
    // precede the scan, so search only the head)
    let exif_position = head_segments.iter().position(|seg| is_exif_segment(seg));

    // Step 5: Reconstruct JPEG with modified EXIF
    reconstruct_jpeg(head_segments, new_exif_segment, exif_position, raw_tail)
}

/// Checks if a segment is an EXIF APP1 segment.
///
/// An EXIF APP1 segment is identified by:
/// - Marker 0xFFE1 (APP1)
/// - Data starts with "Exif\0\0" identifier
///
/// This distinguishes EXIF from XMP (which also uses APP1 but has different identifier).
fn is_exif_segment(segment: &Segment) -> bool {
    segment.is_app1() && segment.data.starts_with(EXIF_IDENTIFIER)
}

/// Reconstructs a complete JPEG file with modified EXIF segment.
///
/// This function iterates through all original segments and:
/// - Copies SOI marker
/// - For each segment:
///   - If it's the EXIF segment, writes new version
///   - Otherwise, copies original
/// - If no EXIF segment existed, inserts new one after APP0 or SOI
/// - Copies EOI marker
///
/// # Parameters
///
/// - `segments`: Original JPEG segments from parser
/// - `new_exif_data`: New EXIF segment data (excluding marker and length)
/// - `exif_position`: Position of existing EXIF segment, or None if not present
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: Complete modified JPEG file
/// - `Err(ExifToolError)`: If reconstruction fails or segment is too large
fn reconstruct_jpeg(
    segments: &[Segment],
    new_exif_data: Vec<u8>,
    exif_position: Option<usize>,
    raw_tail: &[u8],
) -> Result<Vec<u8>> {
    // Pre-allocate buffer (rough estimate)
    let mut output = Vec::with_capacity(
        segments.iter().map(|s| s.data.len() + 4).sum::<usize>()
            + new_exif_data.len()
            + raw_tail.len(),
    );

    // Determine insertion position if EXIF doesn't exist
    let insert_position = if exif_position.is_none() {
        // Find APP0 position (insert after it)
        // If no APP0, insert after SOI (position 1)
        segments
            .iter()
            .position(|seg| seg.marker == 0xFFE0)
            .map(|pos| pos + 1)
            .unwrap_or(1) // After SOI
    } else {
        0 // Not used if exif_position is Some
    };

    let mut exif_written = false;

    for (i, segment) in segments.iter().enumerate() {
        // Check if we need to insert new EXIF before this segment
        if exif_position.is_none() && i == insert_position && !exif_written {
            if !new_exif_data.is_empty() {
                write_segment(&mut output, APP1_MARKER, &new_exif_data)?;
            }
            exif_written = true;
        }

        // Write segment
        if Some(i) == exif_position {
            // Replace EXIF segment (or drop it entirely when new_exif_data is
            // empty — e.g. clear_all_metadata)
            if !new_exif_data.is_empty() {
                write_segment(&mut output, APP1_MARKER, &new_exif_data)?;
            }
            exif_written = true;
        } else {
            // Copy original segment
            write_segment(&mut output, segment.marker, segment.data)?;
        }
    }

    // If we still haven't written EXIF (shouldn't happen), add at end before EOI
    if !exif_written && !new_exif_data.is_empty() {
        // Remove EOI if present
        if output.len() >= 2 && output[output.len() - 2..] == [0xFF, 0xD9] {
            output.truncate(output.len() - 2);
        }
        write_segment(&mut output, APP1_MARKER, &new_exif_data)?;
        // Re-add EOI
        output.extend_from_slice(&EOI_MARKER.to_be_bytes());
    }

    // Entropy-coded scan data, EOI, and any trailer copied verbatim
    output.extend_from_slice(raw_tail);

    Ok(output)
}

/// Writes a single JPEG segment to output buffer.
///
/// For segments with data, writes:
/// - Marker (2 bytes, big-endian)
/// - Length (2 bytes, big-endian, includes length field but NOT marker)
/// - Data (variable length)
///
/// For standalone markers (SOI, EOI, RST0-RST7), writes only the marker.
///
/// # Parameters
///
/// - `output`: Output buffer to write to
/// - `marker`: 2-byte JPEG marker
/// - `data`: Segment data (empty for standalone markers)
///
/// # Returns
///
/// - `Ok(())`: Segment written successfully
/// - `Err(ExifToolError)`: If segment is too large (>65533 bytes)
fn write_segment(output: &mut Vec<u8>, marker: u16, data: &[u8]) -> Result<()> {
    // Write marker (2 bytes, big-endian)
    output.extend_from_slice(&marker.to_be_bytes());

    // Check if this is a standalone marker (no length or data)
    if is_standalone_marker(marker) {
        return Ok(());
    }

    // Calculate length: 2 (length field) + data.len()
    let length = 2 + data.len();

    // Validate length fits in u16
    if length > 0xFFFF {
        return Err(ExifToolError::invalid_tag_value(
            "segment_length",
            format!("Segment data too large: {} bytes (max 65533)", data.len()),
        ));
    }

    // Write length (2 bytes, big-endian)
    output.extend_from_slice(&(length as u16).to_be_bytes());

    // Write data
    output.extend_from_slice(data);

    Ok(())
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
    use crate::core::tag_value::TagValue;
    use crate::io::EndianReader;
    use crate::test_support::TestReader;

    /// Creates a minimal valid JPEG with EXIF
    fn create_jpeg_with_exif() -> Vec<u8> {
        let mut data = Vec::new();

        // SOI marker
        data.extend_from_slice(&[0xFF, 0xD8]);

        // APP1 marker (EXIF)
        data.extend_from_slice(&[0xFF, 0xE1]);
        // Length: 2 + 6 (Exif\0\0) + 8 (TIFF header) = 16
        data.extend_from_slice(&[0x00, 0x10]);
        // EXIF identifier
        data.extend_from_slice(b"Exif\0\0");
        // TIFF header (little-endian, IFD at offset 8)
        data.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);

        // EOI marker
        data.extend_from_slice(&[0xFF, 0xD9]);

        data
    }

    /// Creates a JPEG without EXIF (only SOI + EOI)
    fn create_jpeg_without_exif() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xD8]); // SOI
        data.extend_from_slice(&[0xFF, 0xD9]); // EOI
        data
    }

    /// Creates a JPEG with APP0 but no EXIF
    fn create_jpeg_with_app0() -> Vec<u8> {
        let mut data = Vec::new();

        // SOI
        data.extend_from_slice(&[0xFF, 0xD8]);

        // APP0 (JFIF)
        data.extend_from_slice(&[0xFF, 0xE0]);
        data.extend_from_slice(&[0x00, 0x06]); // Length: 6
        data.extend_from_slice(&[0x4A, 0x46, 0x49, 0x46]); // "JFIF"

        // EOI
        data.extend_from_slice(&[0xFF, 0xD9]);

        data
    }

    #[test]
    fn test_is_exif_segment() {
        // Create EXIF segment
        let exif_data = b"Exif\0\0test";
        let exif_seg = Segment::new(0xFFE1, 0, exif_data);
        assert!(is_exif_segment(&exif_seg));

        // Create XMP segment (also APP1, but different identifier)
        let xmp_data = b"http://ns.adobe.com/xap/1.0/\0test";
        let xmp_seg = Segment::new(0xFFE1, 0, xmp_data);
        assert!(!is_exif_segment(&xmp_seg));

        // Create non-APP1 segment
        let app0_seg = Segment::new(0xFFE0, 0, b"JFIF");
        assert!(!is_exif_segment(&app0_seg));
    }

    #[test]
    fn test_rewrite_jpeg_exif_builds_new_segment() {
        // No original EXIF segment: rewrite_jpeg_exif must still build a
        // fresh one from the desired map (the "insert new" path).
        let file_bytes = create_jpeg_without_exif();
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let result = crate::writers::exif_surgical::rewrite_jpeg_exif(&file_bytes, &metadata);
        assert!(result.is_ok());

        let segment_data = result.unwrap();

        // Should start with EXIF identifier
        assert_eq!(&segment_data[0..6], EXIF_IDENTIFIER);

        // Should have TIFF header
        assert_eq!(&segment_data[6..8], &[0x49, 0x49]); // Little-endian
        assert_eq!(&segment_data[8..10], &[0x2A, 0x00]); // Magic
        assert_eq!(&segment_data[10..14], &[0x08, 0x00, 0x00, 0x00]); // IFD offset

        // The segment must actually parse and contain the tag we asked for
        let tiff = &segment_data[EXIF_IDENTIFIER.len()..];
        let scan = crate::writers::exif_surgical::scan_exif_entries(tiff).unwrap();
        let make = scan
            .entries
            .iter()
            .find(|e| e.tag_id == 0x010F)
            .expect("Make tag must be present");
        assert_eq!(make.value, b"Canon\0");
    }

    #[test]
    fn test_write_segment() {
        let mut output = Vec::new();

        // Write APP1 segment
        let data = b"test data";
        write_segment(&mut output, 0xFFE1, data).unwrap();

        // Check marker
        assert_eq!(&output[0..2], &[0xFF, 0xE1]);

        // Check length (2 + 9 = 11)
        let reader = EndianReader::big_endian(&output);
        assert_eq!(reader.u16_at(2).unwrap_or(0), 11);

        // Check data
        assert_eq!(&output[4..13], data);
    }

    #[test]
    fn test_write_standalone_marker() {
        let mut output = Vec::new();

        // Write SOI (standalone, no length or data)
        write_segment(&mut output, SOI_MARKER, &[]).unwrap();

        // Should only have marker (2 bytes)
        assert_eq!(output.len(), 2);
        assert_eq!(&output, &[0xFF, 0xD8]);
    }

    #[test]
    fn test_write_exif_to_jpeg_replace_existing() {
        let jpeg = create_jpeg_with_exif();
        let reader = TestReader::new(jpeg);

        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Artist", TagValue::new_string("TestArtist"));

        let result = write_exif_to_jpeg(&reader, &metadata);
        assert!(result.is_ok());

        let modified_jpeg = result.unwrap();

        // Should still be valid JPEG
        assert_eq!(&modified_jpeg[0..2], &[0xFF, 0xD8]); // SOI
        assert_eq!(&modified_jpeg[modified_jpeg.len() - 2..], &[0xFF, 0xD9]); // EOI

        // Should have APP1 segment
        assert!(modified_jpeg.windows(2).any(|w| w == [0xFF, 0xE1]));

        // Parse and verify EXIF identifier is present
        let modified_reader = TestReader::new(modified_jpeg);
        let segments = parse_segments(&modified_reader).unwrap();
        let exif_seg = segments.iter().find(|s| is_exif_segment(s));
        assert!(exif_seg.is_some());
    }

    #[test]
    fn test_write_exif_to_jpeg_insert_new() {
        let jpeg = create_jpeg_without_exif();
        let reader = TestReader::new(jpeg);

        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let result = write_exif_to_jpeg(&reader, &metadata);
        assert!(result.is_ok());

        let modified_jpeg = result.unwrap();

        // Should have SOI and EOI
        assert_eq!(&modified_jpeg[0..2], &[0xFF, 0xD8]);
        assert_eq!(&modified_jpeg[modified_jpeg.len() - 2..], &[0xFF, 0xD9]);

        // Should have new APP1 segment
        let modified_reader = TestReader::new(modified_jpeg);
        let segments = parse_segments(&modified_reader).unwrap();
        let exif_seg = segments.iter().find(|s| is_exif_segment(s));
        assert!(exif_seg.is_some());
    }

    #[test]
    fn test_write_exif_to_jpeg_insert_after_app0() {
        let jpeg = create_jpeg_with_app0();
        let reader = TestReader::new(jpeg);

        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Model", TagValue::new_string("EOS"));

        let result = write_exif_to_jpeg(&reader, &metadata);
        assert!(result.is_ok());

        let modified_jpeg = result.unwrap();

        // Parse segments
        let modified_reader = TestReader::new(modified_jpeg);
        let segments = parse_segments(&modified_reader).unwrap();

        // Should have: SOI, APP0, APP1 (EXIF), EOI
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].marker, 0xFFD8); // SOI
        assert_eq!(segments[1].marker, 0xFFE0); // APP0
        assert_eq!(segments[2].marker, 0xFFE1); // APP1 (EXIF)
        assert!(is_exif_segment(&segments[2]));
        assert_eq!(segments[3].marker, 0xFFD9); // EOI
    }

    #[test]
    fn test_is_standalone_marker() {
        assert!(is_standalone_marker(SOI_MARKER));
        assert!(is_standalone_marker(EOI_MARKER));
        assert!(is_standalone_marker(0xFFD0)); // RST0
        assert!(is_standalone_marker(0xFFD7)); // RST7

        assert!(!is_standalone_marker(0xFFE1)); // APP1
        assert!(!is_standalone_marker(0xFFE0)); // APP0
    }
}
