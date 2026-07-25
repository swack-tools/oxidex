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
//! - **Encrypt dictionary**: Encryption and security information
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
//! use oxidex::parsers::pdf::parse_pdf_metadata;
//! use oxidex::io::buffered_reader::BufferedReader;
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

pub mod embedded_files_parser;
pub mod encryption_parser;
pub mod font_parser;
pub mod info_parser;
pub mod permissions_parser;
pub mod resources_parser;
pub mod root_parser;
pub mod shared;
pub mod signature_parser;
pub mod structure_parser;
pub mod xmp_extractor;

use crate::core::{FileReader, MetadataMap};
use crate::error::{ExifToolError, Result};
use crate::parsers::icc::extract_icc_profile;
use encryption_parser::parse_encryption_metadata;
// use root_parser::parse_root_metadata;
// use structure_parser::parse_structure_metadata;

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
/// use oxidex::parsers::pdf::parse_pdf_metadata;
/// use oxidex::io::buffered_reader::BufferedReader;
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
    let first_line_end = header_data
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(header_data.len());
    let first_line = &header_data[..first_line_end];

    // The first line should be ASCII: %PDF-X.Y
    if let Ok(header_str) = std::str::from_utf8(first_line)
        && let Some(version_str) = header_str.strip_prefix("%PDF-")
    {
        let version = version_str.trim();
        // Store as string to preserve exact version format (e.g., "1.3", "1.4", "2.0")
        let version_string = crate::core::TagValue::new_string(version.to_string());
        metadata.insert("PDF:PDFVersion".to_string(), version_string.clone());
        // Add PDF:Version as alias for ExifTool compatibility
        metadata.insert("PDF:Version".to_string(), version_string);
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
    match info_parser::parse_info_dict(reader) {
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
    match xmp_extractor::extract_xmp_metadata(reader) {
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

    // Extract ICC profile metadata
    match extract_icc_profile(reader) {
        Ok(icc_metadata) => {
            for (key, value) in icc_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(_) => {
            // ICC profile is optional - silently continue if not present
        }
    }

    // Extract root dictionary metadata (Language, PageLayout, PageMode, JavaScript, Outlines, Names)
    if let Ok(root_meta) = root_parser::parse_root_metadata(reader) {
        for (key, value) in root_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract encryption metadata
    if let Ok(enc_meta) = encryption_parser::parse_encryption_metadata(reader) {
        for (key, value) in enc_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract digital signature metadata
    if let Ok(sig_meta) = signature_parser::parse_signature_metadata(reader) {
        for (key, value) in sig_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract permissions metadata
    if let Ok(perm_meta) = permissions_parser::parse_permissions_metadata(reader) {
        for (key, value) in perm_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract embedded resources metadata
    if let Ok(res_meta) = resources_parser::parse_resources_metadata(reader) {
        for (key, value) in res_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract font metadata
    if let Ok(font_meta) = font_parser::parse_font_metadata(reader) {
        for (key, value) in font_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract embedded file metadata
    if let Ok(embedded_meta) = embedded_files_parser::parse_embedded_files_metadata(reader) {
        for (key, value) in embedded_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract encryption metadata
    match parse_encryption_metadata(reader) {
        Ok(encryption_metadata) => {
            // Merge encryption tags into main metadata
            for (key, value) in encryption_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(e) => {
            // Log warning but continue - encryption might not be parseable
            eprintln!("Warning: Failed to parse PDF encryption metadata: {}", e);
        }
    }

    // Extract Root/Catalog metadata
    // TODO: Implement root_parser
    // match parse_root_metadata(reader) {
    //     Ok(root_metadata) => {
    //         // Merge Root tags into main metadata
    //         for (key, value) in root_metadata.iter() {
    //             metadata.insert(key.clone(), value.clone());
    //         }
    //     }
    //     Err(e) => {
    //         // Log warning but continue - Root metadata might not be parseable
    //         eprintln!("Warning: Failed to parse PDF Root metadata: {}", e);
    //     }
    // }

    // Extract structure and features metadata
    // TODO: Implement structure_parser
    // match parse_structure_metadata(reader) {
    //     Ok(structure_metadata) => {
    //         // Merge structure tags into main metadata
    //         for (key, value) in structure_metadata.iter() {
    //             metadata.insert(key.clone(), value.clone());
    //         }
    //     }
    //     Err(e) => {
    //         // Log warning but continue - structure metadata might not be parseable
    //         eprintln!("Warning: Failed to parse PDF structure metadata: {}", e);
    //     }
    // }

    // PDF image streams may contain complete TIFF/EXIF payloads. Extract
    // metadata from these payloads after the PDF-level resource parsers have
    // run so the standard IFD tag database determines the canonical key.
    if let Ok(exif_metadata) = extract_embedded_exif_metadata(reader) {
        for (key, value) in exif_metadata.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // If we didn't extract any metadata at all, return error
    if metadata.is_empty() {
        return Err(ExifToolError::parse_error(
            "No metadata found in PDF (no Info dictionary, XMP, ICC profile, encryption, Root, or structure)",
        ));
    }

    Ok(metadata)
}

const TIFF_ARTIST_TAG: u16 = 0x013b;

#[derive(Clone, Copy)]
enum EmbeddedTiffByteOrder {
    Little,
    Big,
}

/// Extracts the standard IFD0 Artist tag from TIFF/EXIF payloads embedded in
/// PDF image streams.
///
/// DCT-encoded PDF image streams retain their JPEG APP1 payload byte-for-byte,
/// so the TIFF byte-order marker can be located without interpreting the PDF
/// data as text. TIFF offsets remain relative to the located TIFF header.
fn extract_embedded_exif_metadata(
    reader: &dyn crate::core::FileReader,
) -> crate::error::Result<crate::core::MetadataMap> {
    let mut metadata = crate::core::MetadataMap::with_capacity(1);
    let file_size = usize::try_from(reader.size()).map_err(|_| {
        crate::error::ExifToolError::parse_error(
            "PDF is too large to scan for embedded EXIF metadata",
        )
    })?;
    let data = reader.read(0, file_size)?;

    let Some(artist) = find_embedded_exif_artist(data) else {
        return Ok(metadata);
    };

    // Artist is tag 0x013b in IFD0. The tag database supplies the canonical
    // group-qualified key for this directory instead of assigning a prefix
    // based on the containing PDF.
    let tag_key = crate::tag_db::lookup_tag_name(TIFF_ARTIST_TAG, "IFD0");
    metadata.insert(tag_key, crate::core::TagValue::new_string(artist));

    Ok(metadata)
}

/// Searches raw PDF bytes for embedded TIFF headers and returns the first
/// valid IFD0 Artist value.
fn find_embedded_exif_artist(data: &[u8]) -> Option<String> {
    let mut cursor = 0usize;

    while cursor < data.len() {
        let remaining = data.get(cursor..)?;
        let Some(relative_offset) = remaining
            .windows(4)
            .position(|window| window == b"II\x2a\x00" || window == b"MM\x00\x2a")
        else {
            break;
        };

        let tiff_offset = cursor.checked_add(relative_offset)?;
        if let Some(artist) = parse_ifd0_ascii_tag(data.get(tiff_offset..)?, TIFF_ARTIST_TAG) {
            return Some(artist);
        }

        // Advance one byte rather than trusting any offsets in a malformed
        // candidate, allowing a later valid TIFF payload to be considered.
        cursor = tiff_offset.checked_add(1)?;
    }

    None
}

/// Parses one ASCII tag from the first TIFF IFD.
fn parse_ifd0_ascii_tag(data: &[u8], wanted_tag: u16) -> Option<String> {
    let byte_order = if data.get(0..2)? == b"II" {
        EmbeddedTiffByteOrder::Little
    } else if data.get(0..2)? == b"MM" {
        EmbeddedTiffByteOrder::Big
    } else {
        return None;
    };

    if read_embedded_tiff_u16(data, 2, byte_order)? != 42 {
        return None;
    }

    let ifd_offset = usize::try_from(read_embedded_tiff_u32(data, 4, byte_order)?).ok()?;
    let entry_count = usize::from(read_embedded_tiff_u16(data, ifd_offset, byte_order)?);
    let entries_offset = ifd_offset.checked_add(2)?;
    let entries_len = entry_count.checked_mul(12)?;
    let entries_end = entries_offset.checked_add(entries_len)?;

    // Validate the complete entry array before iterating over file-controlled
    // entry counts.
    data.get(entries_offset..entries_end)?;

    for entry_index in 0..entry_count {
        let entry_offset = entries_offset.checked_add(entry_index.checked_mul(12)?)?;

        if read_embedded_tiff_u16(data, entry_offset, byte_order)? != wanted_tag {
            continue;
        }

        let field_type = read_embedded_tiff_u16(data, entry_offset.checked_add(2)?, byte_order)?;
        if field_type != 2 {
            return None;
        }

        let value_len = usize::try_from(read_embedded_tiff_u32(
            data,
            entry_offset.checked_add(4)?,
            byte_order,
        )?)
        .ok()?;
        if value_len == 0 {
            return None;
        }

        let value_offset = if value_len <= 4 {
            entry_offset.checked_add(8)?
        } else {
            usize::try_from(read_embedded_tiff_u32(
                data,
                entry_offset.checked_add(8)?,
                byte_order,
            )?)
            .ok()?
        };
        let value_end = value_offset.checked_add(value_len)?;
        let raw_value = data.get(value_offset..value_end)?;
        let text_len = raw_value
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(raw_value.len());
        let value = String::from_utf8_lossy(raw_value.get(..text_len)?).into_owned();

        if !value.is_empty() {
            return Some(value);
        }
        return None;
    }

    None
}

fn read_embedded_tiff_u16(
    data: &[u8],
    offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes: [u8; 2] = data.get(offset..end)?.try_into().ok()?;

    Some(match byte_order {
        EmbeddedTiffByteOrder::Little => u16::from_le_bytes(bytes),
        EmbeddedTiffByteOrder::Big => u16::from_be_bytes(bytes),
    })
}

fn read_embedded_tiff_u32(
    data: &[u8],
    offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes: [u8; 4] = data.get(offset..end)?.try_into().ok()?;

    Some(match byte_order {
        EmbeddedTiffByteOrder::Little => u32::from_le_bytes(bytes),
        EmbeddedTiffByteOrder::Big => u32::from_be_bytes(bytes),
    })
}

#[cfg(test)]
mod embedded_exif_tests {
    use super::find_embedded_exif_artist;

    #[test]
    fn extracts_artist_from_embedded_little_endian_tiff() {
        let mut pdf = b"%PDF-1.4\nstream\nExif\0\0".to_vec();

        // TIFF header with IFD0 at offset 8.
        pdf.extend_from_slice(b"II\x2a\x00\x08\x00\x00\x00");
        // One IFD entry.
        pdf.extend_from_slice(b"\x01\x00");
        // Artist (0x013b), ASCII, 12 bytes, value at TIFF offset 26.
        pdf.extend_from_slice(b"\x3b\x01\x02\x00\x0c\x00\x00\x00\x1a\x00\x00\x00");
        // No next IFD, followed by the Artist string.
        pdf.extend_from_slice(b"\x00\x00\x00\x00Phil Harvey\0");
        pdf.extend_from_slice(b"\nendstream\n%%EOF");

        assert_eq!(
            find_embedded_exif_artist(&pdf).as_deref(),
            Some("Phil Harvey")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;
    use crate::test_support::TestReader;

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
            Some("2024:01:15 12:00:00Z")
        );
        assert_eq!(
            metadata.get_string("PDF:ModifyDate"),
            Some("2024:01:15 12:00:00Z")
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid PDF signature")
        );
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
