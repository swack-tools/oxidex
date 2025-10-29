//! XMP segment parser for JPEG (RDF/XML)
//!
//! This module handles extraction of XMP metadata from JPEG APP1 segments.
//! XMP in JPEG files is stored in APP1 segments (0xFFE1) with the identifier
//! "http://ns.adobe.com/xap/1.0/\0" followed by XML/RDF data.
//!
//! # XMP APP1 Segment Structure
//!
//! - Marker: 0xFFE1 (APP1 marker)
//! - Length: 2 bytes (big-endian, includes length field itself)
//! - XMP identifier: "http://ns.adobe.com/xap/1.0/\0" (29 bytes)
//! - XML payload: Rest of segment data (RDF/XML format)
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::parsers::jpeg::segment_parser::parse_segments;
//! use exiftool_rs::parsers::jpeg::xmp_parser::extract_xmp_from_segments;
//! use exiftool_rs::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("image.jpg"))?;
//! let segments = parse_segments(&reader)?;
//! let xmp_tags = extract_xmp_from_segments(&segments)?;
//!
//! for (tag_name, value) in &xmp_tags {
//!     println!("{}: {}", tag_name, value);
//! }
//! # Ok(())
//! # }
//! ```

use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::segment_parser::Segment;
use crate::parsers::xmp::parse_xmp;

/// The XMP identifier string that appears at the start of XMP APP1 segments.
/// This is a null-terminated string: "http://ns.adobe.com/xap/1.0/\0"
const XMP_IDENTIFIER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// Extracts XMP metadata from JPEG segments.
///
/// This function scans through all segments, identifies APP1 segments with
/// the XMP identifier, extracts the XML payload, and parses it using the
/// XMP/RDF parser.
///
/// # Parameters
///
/// - `segments`: Slice of parsed JPEG segments (from `parse_segments()`)
///
/// # Returns
///
/// Vector of (tag_name, value) tuples where tag_name is in the format
/// "XMP:PropertyName" (e.g., "XMP:Creator", "XMP:Rating").
///
/// Returns an empty vector if no XMP segments are found (not an error).
///
/// # Errors
///
/// Returns `ParseError` if:
/// - XMP XML payload is malformed
/// - XML parsing fails
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::jpeg::segment_parser::parse_segments;
/// use exiftool_rs::parsers::jpeg::xmp_parser::extract_xmp_from_segments;
/// use exiftool_rs::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("image.jpg"))?;
/// let segments = parse_segments(&reader)?;
/// let xmp_tags = extract_xmp_from_segments(&segments)?;
///
/// // Check for specific XMP tags
/// for (tag_name, value) in &xmp_tags {
///     if tag_name == "XMP:Creator" {
///         println!("Creator: {}", value);
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn extract_xmp_from_segments(segments: &[Segment]) -> Result<Vec<(String, String)>> {
    let mut all_xmp_tags = Vec::new();

    // Iterate through all segments looking for XMP APP1 segments
    for segment in segments {
        // Check if this is an APP1 segment
        if !segment.is_app1() {
            continue;
        }

        // Check if this APP1 segment contains XMP data
        // The XMP identifier must appear at the start of the segment data
        if !segment.data.starts_with(XMP_IDENTIFIER) {
            continue;
        }

        // Extract the XML payload (skip the 29-byte XMP identifier)
        let xml_payload = &segment.data[XMP_IDENTIFIER.len()..];

        // Parse the XMP XML data
        let xmp_tags = parse_xmp(xml_payload).map_err(|e| {
            ExifToolError::parse_error(format!("Failed to parse XMP segment: {}", e))
        })?;

        // Add all extracted tags to the result vector
        all_xmp_tags.extend(xmp_tags);
    }

    Ok(all_xmp_tags)
}

/// Checks if a segment is an XMP APP1 segment.
///
/// This is a convenience function that checks both:
/// 1. The segment is an APP1 segment (0xFFE1)
/// 2. The segment data starts with the XMP identifier
///
/// # Parameters
///
/// - `segment`: The JPEG segment to check
///
/// # Returns
///
/// `true` if this is an XMP APP1 segment, `false` otherwise.
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::jpeg::segment_parser::parse_segments;
/// use exiftool_rs::parsers::jpeg::xmp_parser::is_xmp_segment;
/// use exiftool_rs::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("image.jpg"))?;
/// let segments = parse_segments(&reader)?;
///
/// for segment in &segments {
///     if is_xmp_segment(segment) {
///         println!("Found XMP segment at offset {}", segment.offset);
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn is_xmp_segment(segment: &Segment) -> bool {
    segment.is_app1() && segment.data.starts_with(XMP_IDENTIFIER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xmp_identifier_constant() {
        assert_eq!(XMP_IDENTIFIER.len(), 29);
        assert_eq!(XMP_IDENTIFIER, b"http://ns.adobe.com/xap/1.0/\0");
    }

    #[test]
    fn test_is_xmp_segment_positive() {
        const XMP_TEST_DATA: &[u8] = b"http://ns.adobe.com/xap/1.0/\0<xml>data</xml>";
        let segment = Segment::new(0xFFE1, 0, XMP_TEST_DATA);
        assert!(is_xmp_segment(&segment));
    }

    #[test]
    fn test_is_xmp_segment_wrong_marker() {
        const XMP_TEST_DATA: &[u8] = b"http://ns.adobe.com/xap/1.0/\0<xml>data</xml>";
        // APP0 marker instead of APP1
        let segment = Segment::new(0xFFE0, 0, XMP_TEST_DATA);
        assert!(!is_xmp_segment(&segment));
    }

    #[test]
    fn test_is_xmp_segment_wrong_identifier() {
        // EXIF identifier instead of XMP
        let segment = Segment::new(0xFFE1, 0, b"Exif\0\0test data");
        assert!(!is_xmp_segment(&segment));
    }

    #[test]
    fn test_is_xmp_segment_empty() {
        let segment = Segment::new(0xFFE1, 0, b"");
        assert!(!is_xmp_segment(&segment));
    }

    #[test]
    fn test_extract_xmp_from_segments_valid() {
        // Create a minimal valid XMP segment with constant data
        const XMP_XML: &[u8] = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmp="http://ns.adobe.com/xap/1.0/">
              <rdf:Description>
                <xmp:Creator>John Doe</xmp:Creator>
                <xmp:Rating>5</xmp:Rating>
              </rdf:Description>
            </rdf:RDF>
        "#;

        const XMP_SEGMENT_DATA: &[u8] = b"http://ns.adobe.com/xap/1.0/\0\
            <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\
                     xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\
              <rdf:Description>\
                <xmp:Creator>John Doe</xmp:Creator>\
                <xmp:Rating>5</xmp:Rating>\
              </rdf:Description>\
            </rdf:RDF>\
        ";

        let segments = vec![
            Segment::new(0xFFD8, 0, b""), // SOI
            Segment::new(0xFFE1, 2, XMP_SEGMENT_DATA), // XMP APP1
            Segment::new(0xFFD9, 0, b""), // EOI
        ];

        let result = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");

        assert!(result.len() >= 2, "Expected at least 2 XMP tags, got {}", result.len());

        // Check for specific tags
        let has_creator = result.iter().any(|(name, value)| {
            name == "XMP:Creator" && value == "John Doe"
        });
        assert!(has_creator, "Missing XMP:Creator tag");

        let has_rating = result.iter().any(|(name, value)| {
            name == "XMP:Rating" && value == "5"
        });
        assert!(has_rating, "Missing XMP:Rating tag");
    }

    #[test]
    fn test_extract_xmp_from_segments_no_xmp() {
        // Create segments without XMP
        let segments = vec![
            Segment::new(0xFFD8, 0, b""), // SOI
            Segment::new(0xFFE1, 2, b"Exif\0\0test"), // EXIF APP1
            Segment::new(0xFFD9, 0, b""), // EOI
        ];

        let result = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");

        // Should return empty vector, not an error
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_extract_xmp_from_segments_empty() {
        let segments = vec![];
        let result = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_extract_xmp_from_segments_malformed_xml() {
        // Create XMP segment with malformed XML (invalid UTF-8)
        const MALFORMED_XML: &[u8] = b"http://ns.adobe.com/xap/1.0/\0<rdf:RDF><\xFF\xFE:test>value</test></rdf:RDF>";

        let segments = vec![
            Segment::new(0xFFE1, 0, MALFORMED_XML),
        ];

        let result = extract_xmp_from_segments(&segments);

        // Should return an error for malformed XML
        assert!(result.is_err());
        match result {
            Err(ExifToolError::ParseError { .. }) => {
                // Expected error type
            }
            _ => panic!("Expected ParseError for malformed XML"),
        }
    }

    #[test]
    fn test_extract_xmp_multiple_namespaces() {
        const XMP_XML: &[u8] = b"http://ns.adobe.com/xap/1.0/\0\
            <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\
                     xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\
                     xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\
                     xmlns:exif=\"http://ns.adobe.com/exif/1.0/\">\
              <rdf:Description>\
                <xmp:Creator>Jane Smith</xmp:Creator>\
                <dc:title>My Photo</dc:title>\
                <dc:rights>Copyright 2024</dc:rights>\
                <exif:Make>Canon</exif:Make>\
              </rdf:Description>\
            </rdf:RDF>\
        ";

        let segments = vec![
            Segment::new(0xFFE1, 0, XMP_XML),
        ];

        let result = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");

        assert!(result.len() >= 4, "Expected at least 4 XMP tags");

        // Check that we have properties from all namespaces
        let tag_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        assert!(tag_names.iter().any(|n| n == "XMP:Creator"), "Missing XMP:Creator");
        assert!(tag_names.iter().any(|n| n == "XMP:title"), "Missing XMP:title");
        assert!(tag_names.iter().any(|n| n == "XMP:rights"), "Missing XMP:rights");
        assert!(tag_names.iter().any(|n| n == "XMP:Make"), "Missing XMP:Make");
    }

    #[test]
    fn test_extract_xmp_multiple_segments() {
        // Test handling of multiple XMP segments in one JPEG
        const XMP_XML1: &[u8] = b"http://ns.adobe.com/xap/1.0/\0\
            <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\
                     xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\
              <rdf:Description>\
                <xmp:Creator>First Creator</xmp:Creator>\
              </rdf:Description>\
            </rdf:RDF>\
        ";

        const XMP_XML2: &[u8] = b"http://ns.adobe.com/xap/1.0/\0\
            <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\
                     xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
              <rdf:Description>\
                <dc:title>Second Title</dc:title>\
              </rdf:Description>\
            </rdf:RDF>\
        ";

        let segments = vec![
            Segment::new(0xFFD8, 0, b""), // SOI
            Segment::new(0xFFE1, 2, XMP_XML1), // First XMP
            Segment::new(0xFFE1, 100, XMP_XML2), // Second XMP
            Segment::new(0xFFD9, 0, b""), // EOI
        ];

        let result = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");

        // Should have tags from both segments
        assert!(result.len() >= 2, "Expected tags from both XMP segments");

        let has_creator = result.iter().any(|(name, _)| name == "XMP:Creator");
        let has_title = result.iter().any(|(name, _)| name == "XMP:title");

        assert!(has_creator, "Missing tag from first XMP segment");
        assert!(has_title, "Missing tag from second XMP segment");
    }
}
