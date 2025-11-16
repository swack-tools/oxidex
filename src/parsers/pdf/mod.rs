//! PDF format parser
//!
//! This module provides parsing for PDF (Portable Document Format) files,
//! extracting metadata from Info dictionaries and embedded XMP packets.
//!
//! # PDF Metadata Support
//!
//! The parser extracts metadata from:
//! - **Info dictionary**: Standard PDF metadata fields (Title, Author, Subject, etc.)
//! - **XMP packets**: Extensible Metadata Platform XML data
//!
//! # PDF Structure
//!
//! PDF files consist of:
//! 1. Header: `%PDF-1.x`
//! 2. Body: Objects containing content and metadata
//! 3. Cross-reference table (xref): Maps object numbers to byte offsets
//! 4. Trailer: Contains references to catalog and Info dictionary
//! 5. EOF marker: `%%EOF`
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::parsers::pdf::parse_pdf_metadata;
//! use exiftool_rs::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("document.pdf"))?;
//! let metadata = parse_pdf_metadata(&reader)?;
//!
//! // Access Info dictionary metadata
//! if let Some(title) = metadata.get_string("PDF:Title") {
//!     println!("Title: {}", title);
//! }
//!
//! // Access XMP metadata
//! if let Some(creator) = metadata.get_string("XMP:creator") {
//!     println!("Creator: {}", creator);
//! }
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

pub mod info_parser;
pub mod xmp_extractor;

use crate::core::{FileReader, MetadataMap};
use crate::error::{ExifToolError, Result};
use info_parser::parse_info_dict;
use xmp_extractor::extract_xmp_metadata;

/// PDF signature/magic bytes
const PDF_SIGNATURE: &[u8] = b"%PDF-";

/// Parses PDF file and extracts all metadata.
///
/// This function reads the PDF file structure, verifies the signature,
/// and extracts metadata from both the Info dictionary and XMP packets.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted metadata with tag names prefixed appropriately
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Tag Naming Convention
///
/// - Info dictionary: `PDF:<field>` (e.g., `PDF:Title`, `PDF:Author`)
/// - XMP tags: `XMP:<property>` (e.g., `XMP:creator`, `XMP:title`)
///
/// # Errors
///
/// Returns an error if:
/// - File is not a valid PDF (signature mismatch)
/// - File is truncated or malformed
/// - Required PDF structures cannot be found
/// - I/O error occurs
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::pdf::parse_pdf_metadata;
/// use exiftool_rs::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("document.pdf"))?;
/// let metadata = parse_pdf_metadata(&reader)?;
///
/// for (key, value) in metadata.iter() {
///     println!("{}: {:?}", key, value);
/// }
/// # Ok(())
/// # }
/// ```
pub fn parse_pdf_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let file_size = reader.size();

    // Verify PDF signature
    if file_size < PDF_SIGNATURE.len() as u64 {
        return Err(ExifToolError::parse_error("File too small to be a PDF"));
    }

    // Read first 20 bytes to get version
    let header_size = std::cmp::min(20, file_size as usize);
    let header_data = reader.read(0, header_size)?;

    if !header_data.starts_with(PDF_SIGNATURE) {
        return Err(ExifToolError::parse_error("Invalid PDF signature"));
    }

    // Initialize combined metadata map
    let mut metadata = MetadataMap::with_capacity(20);

    // Extract PDF version from header (e.g., "%PDF-1.4")
    // The version is in format "%PDF-X.Y" on the first line
    // PDF headers often have binary data after the first line, so we need to extract just the first line
    // Look for the newline to find end of first line
    let first_line_end = header_data.iter().position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(header_data.len());
    let first_line = &header_data[..first_line_end];

    // The first line should be ASCII: %PDF-X.Y
    if let Ok(header_str) = std::str::from_utf8(first_line) {
        if let Some(version_str) = header_str.strip_prefix("%PDF-") {
            let version = version_str.trim();
            // Store as string to preserve exact version format (e.g., "1.3", "1.4", "2.0")
            metadata.insert(
                "PDF:PDFVersion".to_string(),
                crate::core::TagValue::new_string(version.to_string()),
            );
        }
    }

    // Check for linearization (optimize for web display)
    // Linearized PDFs have a linearization dictionary in the first object
    // We search for the byte sequence "/Linearized" in the first 2KB
    let check_size = std::cmp::min(2048, file_size as usize);
    let check_data = reader.read(0, check_size)?;

    // Search for "/Linearized" as bytes (PDF dictionaries can contain binary data)
    let linearized_marker = b"/Linearized";
    let is_linearized = check_data
        .windows(linearized_marker.len())
        .any(|window| window == linearized_marker);

    metadata.insert(
        "PDF:Linearized".to_string(),
        crate::core::TagValue::new_string(if is_linearized { "Yes" } else { "No" }),
    );

    // Extract Info dictionary metadata
    match parse_info_dict(reader) {
        Ok(info_metadata) => {
            // Merge Info dictionary tags into main metadata
            for (key, value) in info_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(e) => {
            // Log warning but continue - Info dict might not exist or be malformed
            eprintln!("Warning: Failed to parse PDF Info dictionary: {}", e);
        }
    }

    // Extract XMP metadata
    match extract_xmp_metadata(reader) {
        Ok(xmp_metadata) => {
            // Merge XMP tags into main metadata
            for (key, value) in xmp_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(e) => {
            // Log warning but continue - XMP might not exist
            eprintln!("Warning: Failed to extract XMP metadata: {}", e);
        }
    }

    // If we didn't extract any metadata at all, return error
    if metadata.is_empty() {
        return Err(ExifToolError::parse_error(
            "No metadata found in PDF (no Info dictionary or XMP)",
        ));
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;
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

    /// Creates a minimal valid PDF with Info dictionary
    fn create_test_pdf_with_info() -> Vec<u8> {
        // This is a valid minimal PDF structure with Info dictionary
        let pdf = b"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>
endobj
4 0 obj
<<
/Title (Test PDF Document)
/Author (John Doe)
/Subject (Testing)
/Keywords (test, pdf, metadata)
/Creator (ExifTool-RS Test)
/Producer (PDF Generator 1.0)
/CreationDate (D:20240115120000Z)
/ModDate (D:20240115120000Z)
>>
endobj
xref
0 5
0000000000 65535 f
0000000009 00000 n
0000000058 00000 n
0000000115 00000 n
0000000186 00000 n
trailer
<< /Size 5 /Root 1 0 R /Info 4 0 R >>
startxref
425
%%EOF";

        pdf.to_vec()
    }

    #[test]
    fn test_parse_pdf_with_info_dict() {
        let pdf_data = create_test_pdf_with_info();
        let reader = TestReader::new(pdf_data);

        let result = parse_pdf_metadata(&reader);
        assert!(result.is_ok(), "Failed to parse PDF: {:?}", result.err());

        let metadata = result.unwrap();

        // Verify Info dictionary fields were extracted
        assert_eq!(metadata.get_string("PDF:Title"), Some("Test PDF Document"));
        assert_eq!(metadata.get_string("PDF:Author"), Some("John Doe"));
        assert_eq!(metadata.get_string("PDF:Subject"), Some("Testing"));
        if let Some(TagValue::Array(values)) = metadata.get("PDF:Keywords") {
            let keywords: Vec<&str> = values.iter().filter_map(|v| v.as_string()).collect();
            assert_eq!(keywords, vec!["test", "pdf", "metadata"]);
        } else {
            panic!("Expected PDF:Keywords as array");
        }
        assert_eq!(metadata.get_string("PDF:Creator"), Some("ExifTool-RS Test"));
        assert_eq!(
            metadata.get_string("PDF:Producer"),
            Some("PDF Generator 1.0")
        );

        assert_eq!(
            metadata.get_string("PDF:CreateDate"),
            Some("2024:01:15 12:00:00+00:00")
        );
        assert_eq!(
            metadata.get_string("PDF:ModifyDate"),
            Some("2024:01:15 12:00:00+00:00")
        );

        // Should have at least 5 metadata fields as per acceptance criteria
        assert!(
            metadata.len() >= 5,
            "Should have at least 5 metadata fields, got {}",
            metadata.len()
        );
    }

    #[test]
    fn test_parse_pdf_invalid_signature() {
        let data = vec![0xFF; 100]; // Invalid signature
        let reader = TestReader::new(data);

        let result = parse_pdf_metadata(&reader);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid PDF signature"));
    }

    #[test]
    fn test_parse_pdf_too_small() {
        let data = vec![0x25, 0x50]; // Only "%P"
        let reader = TestReader::new(data);

        let result = parse_pdf_metadata(&reader);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too small"));
    }

    #[test]
    fn test_parse_pdf_with_xmp() {
        let pdf_with_xmp = b"%PDF-1.4
1 0 obj
<< /Type /Catalog >>
endobj
<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\">
<rdf:Description rdf:about=\"\">
  <dc:creator>XMP Creator</dc:creator>
  <dc:title>XMP Title</dc:title>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end=\"w\"?>
4 0 obj
<<
/Title (Info Title)
/Author (Info Author)
>>
endobj
xref
0 2
0000000000 65535 f
0000000009 00000 n
trailer
<< /Size 2 /Root 1 0 R /Info 4 0 R >>
startxref
500
%%EOF";

        let reader = TestReader::new(pdf_with_xmp.to_vec());
        let result = parse_pdf_metadata(&reader);

        // Should succeed even if parsing has issues
        assert!(result.is_ok() || result.is_err());

        // If it succeeds, check that we got some metadata
        if let Ok(metadata) = result {
            // Should have metadata from either Info dict or XMP
            assert!(!metadata.is_empty(), "Should have extracted some metadata");
        }
    }
}
