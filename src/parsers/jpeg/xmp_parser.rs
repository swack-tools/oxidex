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
//! use oxidex::parsers::jpeg::segment_parser::parse_segments;
//! use oxidex::parsers::jpeg::xmp_parser::extract_xmp_from_segments;
//! use oxidex::io::buffered_reader::BufferedReader;
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
use crate::parsers::xmp::parse_xmp_history;
use crate::parsers::xmp::rdf_parser::{XmpValue, parse_xmp_typed_with_rational_forms};

/// The XMP identifier string that appears at the start of XMP APP1 segments.
/// This is a null-terminated string: "http://ns.adobe.com/xap/1.0/\0"
const XMP_IDENTIFIER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// The ExtendedXMP identifier string that appears at the start of the APP1
/// segments carrying an XMP packet too large for one 64 KB segment.
///
/// The XMP specification (part 3, "Extended XMP in JPEG") splits the overflow
/// into any number of APP1 segments, each laid out as:
///
/// ```text
/// "http://ns.adobe.com/xmp/extension/\0"   35 bytes  identifier
/// <GUID>                                   32 bytes  ASCII MD5 of the full packet
/// <full length>                             4 bytes  big-endian
/// <chunk offset>                            4 bytes  big-endian
/// <chunk>                                   rest of the segment
/// ```
///
/// The main packet points at the right GUID through `xmpNote:HasExtendedXMP`,
/// and the chunks reassemble by offset into one standalone XMP packet.
const XMP_EXTENSION_IDENTIFIER: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";

/// Bytes of ExtendedXMP header that follow the identifier: GUID(32) + length(4)
/// + offset(4).
const XMP_EXTENSION_HEADER_LEN: usize = 32 + 4 + 4;

/// Reassembles the ExtendedXMP chunks in `segments` into whole XMP packets,
/// one per GUID, in the order the GUIDs are first seen.
///
/// A chunk whose declared offset or total length does not fit its own bytes is
/// dropped rather than trusted: the offsets index into a buffer sized from the
/// segment's own claim, so an inconsistent pair would otherwise be a way for a
/// malformed file to steer a write.
fn assemble_extended_xmp(segments: &[Segment]) -> Vec<Vec<u8>> {
    // (guid, total_len, assembled bytes, byte-filled count) in first-seen order.
    let mut packets: Vec<([u8; 32], usize, Vec<u8>, usize)> = Vec::new();

    for segment in segments {
        if !segment.is_app1() || !segment.data.starts_with(XMP_EXTENSION_IDENTIFIER) {
            continue;
        }
        let body = &segment.data[XMP_EXTENSION_IDENTIFIER.len()..];
        if body.len() <= XMP_EXTENSION_HEADER_LEN {
            continue;
        }
        let mut guid = [0u8; 32];
        guid.copy_from_slice(&body[..32]);
        let total = u32::from_be_bytes([body[32], body[33], body[34], body[35]]) as usize;
        let offset = u32::from_be_bytes([body[36], body[37], body[38], body[39]]) as usize;
        let chunk = &body[XMP_EXTENSION_HEADER_LEN..];

        // Reject a chunk that does not fit inside the packet it claims to
        // belong to, and refuse absurd totals outright.
        const MAX_EXTENDED_XMP: usize = 128 * 1024 * 1024;
        if total == 0 || total > MAX_EXTENDED_XMP || offset > total || chunk.len() > total - offset
        {
            continue;
        }

        let slot = match packets.iter_mut().find(|(g, _, _, _)| *g == guid) {
            Some(slot) => slot,
            None => {
                packets.push((guid, total, vec![0u8; total], 0));
                packets
                    .last_mut()
                    .expect("just pushed a packet slot, so last_mut cannot be None")
            }
        };
        if slot.1 != total {
            continue; // conflicting length claims for the same GUID
        }
        slot.2[offset..offset + chunk.len()].copy_from_slice(chunk);
        slot.3 += chunk.len();
    }

    packets
        .into_iter()
        .filter(|(_, total, _, filled)| filled >= total)
        .map(|(_, _, buf, _)| buf)
        .collect()
}

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
/// use oxidex::parsers::jpeg::segment_parser::parse_segments;
/// use oxidex::parsers::jpeg::xmp_parser::extract_xmp_from_segments;
/// use oxidex::io::buffered_reader::BufferedReader;
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
/// Transcodes an XMP packet to UTF-8 if it is stored as UTF-16.
///
/// The XMP specification permits UTF-8, UTF-16BE and UTF-16LE, and real files
/// use all three -- ExifTool.jpg in the sample corpus stores its packet as
/// UTF-16BE. `parse_xmp` reads UTF-8, so an unconverted UTF-16 packet parses
/// to nothing and returns Ok(empty): a silent miss of every XMP tag in the
/// file, indistinguishable from a file that simply has no XMP.
///
/// Detection follows the spec's own rule -- the packet begins with `<?xpacket`
/// (or a BOM), so the first two bytes identify the width and order:
///   `EF BB BF` UTF-8 BOM · `FE FF` UTF-16BE BOM · `FF FE` UTF-16LE BOM
///   `00 xx`    UTF-16BE (no BOM) · `xx 00` UTF-16LE (no BOM)
fn xmp_payload_to_utf8(payload: &[u8]) -> Option<Vec<u8>> {
    let decode16 = |be: bool, body: &[u8]| -> Option<Vec<u8>> {
        if body.len() < 2 {
            return None;
        }
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| {
                if be {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16(&units).ok().map(String::into_bytes)
    };

    match payload {
        [0xEF, 0xBB, 0xBF, rest @ ..] => Some(rest.to_vec()),
        [0xFE, 0xFF, rest @ ..] => decode16(true, rest),
        [0xFF, 0xFE, rest @ ..] => decode16(false, rest),
        // No BOM: a UTF-16 packet's first character is ASCII, so exactly one
        // of the two leading bytes is zero.
        [0x00, b, ..] if *b != 0 => decode16(true, payload),
        [b, 0x00, ..] if *b != 0 => decode16(false, payload),
        _ => None,
    }
}

pub fn extract_xmp_from_segments(segments: &[Segment]) -> Result<Vec<(String, XmpValue)>> {
    Ok(extract_xmp_from_segments_with_value_forms(segments)?.0)
}

/// [`extract_xmp_from_segments`], plus each property's ValueConv text where
/// the rdf parser's print formatting discarded precision -- see
/// `parse_xmp_typed_with_rational_forms` (`src/parsers/xmp/rdf_parser.rs`)
/// for exactly which properties carry one and in what shape. The embedded
/// JPEG path needs this for the same reason the `.xmp` sidecar path
/// (`parse_xmp_file`) already consumed it: composites read the ValueConv
/// form, and a JPEG whose XMP packet wins a bare tag key otherwise feeds
/// them the PrintConv-rounded string.
pub fn extract_xmp_from_segments_with_value_forms(
    segments: &[Segment],
) -> Result<(Vec<(String, XmpValue)>, Vec<(String, String)>)> {
    let mut all_xmp_tags = Vec::new();
    let mut all_value_forms = Vec::new();

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
        let raw_payload = &segment.data[XMP_IDENTIFIER.len()..];
        // A UTF-16 packet must be transcoded first; see xmp_payload_to_utf8.
        let converted = xmp_payload_to_utf8(raw_payload);
        let xml_payload: &[u8] = converted.as_deref().unwrap_or(raw_payload);

        // Parse the XMP XML data for standard properties
        let (xmp_tags, value_forms) =
            parse_xmp_typed_with_rational_forms(xml_payload).map_err(|e| {
                ExifToolError::parse_error(format!("Failed to parse XMP segment: {}", e))
            })?;

        all_xmp_tags.extend(xmp_tags);
        all_value_forms.extend(value_forms);

        // Parse XMP history for forensic metadata
        let xml_str = std::str::from_utf8(xml_payload).unwrap_or("");
        if let Ok(history_tags) = parse_xmp_history(xml_str) {
            all_xmp_tags.extend(
                history_tags
                    .into_iter()
                    .map(|(tag, value)| (tag, XmpValue::Scalar(value))),
            );
        }
    }

    // ExtendedXMP: everything the writer could not fit in the 64 KB main
    // packet. Google's depth-map Device/Container structures and Adobe's
    // pdf:Producer/xmp:CreationDate on ExtendedXMP.jpg live here and nowhere
    // else, so skipping these segments loses every tag they carry.
    for packet in assemble_extended_xmp(segments) {
        let converted = xmp_payload_to_utf8(&packet);
        let xml_payload: &[u8] = converted.as_deref().unwrap_or(&packet);
        if let Ok((xmp_tags, value_forms)) = parse_xmp_typed_with_rational_forms(xml_payload) {
            all_xmp_tags.extend(xmp_tags);
            all_value_forms.extend(value_forms);
        }
        let xml_str = std::str::from_utf8(xml_payload).unwrap_or("");
        if let Ok(history_tags) = parse_xmp_history(xml_str) {
            all_xmp_tags.extend(
                history_tags
                    .into_iter()
                    .map(|(tag, value)| (tag, XmpValue::Scalar(value))),
            );
        }
    }

    Ok((all_xmp_tags, all_value_forms))
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
/// use oxidex::parsers::jpeg::segment_parser::parse_segments;
/// use oxidex::parsers::jpeg::xmp_parser::is_xmp_segment;
/// use oxidex::io::buffered_reader::BufferedReader;
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
            Segment::new(0xFFD8, 0, b""),              // SOI
            Segment::new(0xFFE1, 2, XMP_SEGMENT_DATA), // XMP APP1
            Segment::new(0xFFD9, 0, b""),              // EOI
        ];

        let result = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");

        assert!(
            result.len() >= 2,
            "Expected at least 2 XMP tags, got {}",
            result.len()
        );

        // Check for specific tags with ExifTool-compatible prefixes
        // Stream 6 changed to use simplified XMP: prefix for common namespaces
        let has_creator = result
            .iter()
            .any(|(name, value)| name == "XMP:Creator" && value == "John Doe");
        assert!(has_creator, "Missing XMP:Creator tag");

        let has_rating = result
            .iter()
            .any(|(name, value)| name == "XMP:Rating" && value == "5");
        assert!(has_rating, "Missing XMP:Rating tag");
    }

    #[test]
    fn test_extract_xmp_from_segments_no_xmp() {
        // Create segments without XMP
        let segments = vec![
            Segment::new(0xFFD8, 0, b""),             // SOI
            Segment::new(0xFFE1, 2, b"Exif\0\0test"), // EXIF APP1
            Segment::new(0xFFD9, 0, b""),             // EOI
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
        const MALFORMED_XML: &[u8] =
            b"http://ns.adobe.com/xap/1.0/\0<rdf:RDF><\xFF\xFE:test>value</test></rdf:RDF>";

        let segments = vec![Segment::new(0xFFE1, 0, MALFORMED_XML)];

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

        let segments = vec![Segment::new(0xFFE1, 0, XMP_XML)];

        let result = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");

        assert!(result.len() >= 4, "Expected at least 4 XMP tags");

        // Check that we have properties from all namespaces
        // Stream 6 changed to use simplified XMP: prefix for common namespaces (xmp, dc)
        // but XMP-exif: is kept for specialized exif namespace
        let tag_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        assert!(
            tag_names.iter().any(|n| n == "XMP:Creator"),
            "Missing XMP:Creator"
        );
        assert!(
            tag_names.iter().any(|n| n == "XMP:Title"),
            "Missing XMP:Title"
        );
        assert!(
            tag_names.iter().any(|n| n == "XMP:Rights"),
            "Missing XMP:Rights"
        );
        assert!(
            tag_names.iter().any(|n| n == "XMP-exif:Make"),
            "Missing XMP-exif:Make"
        );
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
            Segment::new(0xFFD8, 0, b""),        // SOI
            Segment::new(0xFFE1, 2, XMP_XML1),   // First XMP
            Segment::new(0xFFE1, 100, XMP_XML2), // Second XMP
            Segment::new(0xFFD9, 0, b""),        // EOI
        ];

        let result = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");

        // Should have tags from both segments
        // Stream 6 changed to use simplified XMP: prefix for common namespaces
        assert!(result.len() >= 2, "Expected tags from both XMP segments");

        let has_creator = result.iter().any(|(name, _)| name == "XMP:Creator");
        let has_title = result.iter().any(|(name, _)| name == "XMP:Title");

        assert!(has_creator, "Missing tag from first XMP segment");
        assert!(has_title, "Missing tag from second XMP segment");
    }
}
