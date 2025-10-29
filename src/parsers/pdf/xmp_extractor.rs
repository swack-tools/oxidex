//! PDF XMP metadata packet extractor
//!
//! This module handles extraction of XMP (Extensible Metadata Platform)
//! metadata from PDF files. XMP is embedded as XML packets that can appear
//! in metadata streams or as raw byte sequences.
//!
//! # XMP Packet Format
//!
//! XMP packets in PDFs are marked with special processing instructions:
//! ```text
//! <?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
//! <x:xmpmeta xmlns:x="adobe:ns:meta/">
//!   <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
//!     <!-- XMP properties -->
//!   </rdf:RDF>
//! </x:xmpmeta>
//! <?xpacket end="w"?>
//! ```
//!
//! # Location in PDF
//!
//! XMP can be found in:
//! - Metadata streams referenced by the Catalog dictionary
//! - Embedded within object streams
//! - As standalone objects

use crate::core::{FileReader, MetadataMap};
use crate::error::{ExifToolError, Result};
use crate::parsers::xmp::parse_xmp;

/// Searches for and extracts XMP metadata packets from a PDF file.
///
/// This function scans the PDF file for XMP packet markers and extracts
/// the XML content, then parses it using the XMP parser.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted XMP metadata with "XMP:" prefix
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # XMP Packet Detection
///
/// The function searches for the XMP packet begin marker `<?xpacket` and
/// extracts content until the end marker `<?xpacket end=`. This approach
/// works for both:
/// - Uncompressed XMP streams
/// - XMP embedded directly in the PDF structure
///
/// Note: This implementation does not handle compressed streams. PDFs with
/// compressed metadata streams will not have their XMP extracted.
pub fn extract_xmp_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let file_size = reader.size();

    // XMP packets are typically in the first part of the file (after header)
    // or in metadata streams. Search the first 1MB or entire file if smaller.
    let search_size = std::cmp::min(1024 * 1024, file_size) as usize;
    let search_data = reader.read(0, search_size)?;

    // Search for XMP packet markers
    match find_xmp_packet(search_data) {
        Some(xmp_xml) => {
            // Parse XMP using existing XMP parser
            let xmp_tags = parse_xmp(xmp_xml)
                .map_err(|e| ExifToolError::parse_error(format!("XMP parsing failed: {}", e)))?;

            // Convert to MetadataMap
            let mut metadata = MetadataMap::with_capacity(xmp_tags.len());
            for (key, value) in xmp_tags {
                metadata.insert(key, crate::core::TagValue::new_string(value));
            }

            Ok(metadata)
        }
        None => {
            // No XMP packet found - return empty metadata
            Ok(MetadataMap::new())
        }
    }
}

/// Finds and extracts XMP packet content from PDF data
fn find_xmp_packet(data: &[u8]) -> Option<&[u8]> {
    // XMP packet markers
    const XMP_BEGIN: &[u8] = b"<?xpacket begin=";
    const XMP_END: &[u8] = b"<?xpacket end=";

    // Search for XMP begin marker
    let begin_pos = find_subsequence(data, XMP_BEGIN)?;

    // Find the actual start of XML content (after the xpacket processing instruction)
    // The xpacket begin looks like: <?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
    // We need to find the closing ?> and start from there
    let after_begin = &data[begin_pos..];
    let xml_start_offset = find_subsequence(after_begin, b"?>")? + 2; // +2 to skip the ?>
    let xml_start_pos = begin_pos + xml_start_offset;

    // Search for XMP end marker
    let end_pos = find_subsequence(&data[xml_start_pos..], XMP_END)?;
    let xml_end_pos = xml_start_pos + end_pos;

    // Extract the XMP packet content (including the RDF wrapper)
    Some(&data[xml_start_pos..xml_end_pos])
}

/// Finds a subsequence in a byte slice
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_subsequence() {
        let data = b"Hello World";
        assert_eq!(find_subsequence(data, b"World"), Some(6));
        assert_eq!(find_subsequence(data, b"Foo"), None);
    }

    #[test]
    fn test_find_xmp_packet() {
        let pdf_data = b"\
%PDF-1.4
1 0 obj
<< /Type /Catalog >>
endobj
<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\">
<rdf:Description rdf:about=\"\">
  <dc:creator>Test Creator</dc:creator>
  <dc:title>Test Title</dc:title>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end=\"w\"?>
%%EOF";

        let xmp_content = find_xmp_packet(pdf_data);
        assert!(xmp_content.is_some());

        let xmp = xmp_content.unwrap();
        let xmp_str = std::str::from_utf8(xmp).unwrap();

        // Should contain the XMP/RDF content but not the xpacket processing instructions
        assert!(xmp_str.contains("<x:xmpmeta"));
        assert!(xmp_str.contains("dc:creator"));
        assert!(!xmp_str.contains("<?xpacket begin"));
    }

    #[test]
    fn test_find_xmp_packet_not_found() {
        let pdf_data = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF";
        let xmp_content = find_xmp_packet(pdf_data);
        assert!(xmp_content.is_none());
    }
}
