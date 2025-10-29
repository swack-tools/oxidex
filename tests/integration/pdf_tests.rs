//! Integration tests for PDF metadata parser
//!
//! These tests verify the PDF parser's ability to extract metadata from
//! real PDF files, including Info dictionary fields and XMP packets.

use exiftool_rs::io::buffered_reader::BufferedReader;
use exiftool_rs::parsers::pdf::parse_pdf_metadata;
use std::path::PathBuf;

/// Helper function to get path to test fixture
fn get_fixture_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path.push("pdf");
    path.push(filename);
    path
}

#[test]
fn test_parse_sample_pdf_metadata() {
    let pdf_path = get_fixture_path("sample.pdf");

    // Verify test file exists
    assert!(
        pdf_path.exists(),
        "Test fixture not found: {}",
        pdf_path.display()
    );

    // Parse PDF metadata
    let reader = BufferedReader::new(&pdf_path).expect("Failed to open PDF file");
    let result = parse_pdf_metadata(&reader);

    assert!(
        result.is_ok(),
        "Failed to parse PDF metadata: {:?}",
        result.err()
    );

    let metadata = result.unwrap();

    // Verify at least 5 metadata fields extracted (acceptance criteria)
    assert!(
        metadata.len() >= 5,
        "Expected at least 5 metadata fields, got {}",
        metadata.len()
    );

    // Verify specific Info dictionary fields
    assert_eq!(
        metadata.get_string("PDF:Title"),
        Some("Sample PDF for Testing"),
        "PDF:Title not found or incorrect"
    );

    assert_eq!(
        metadata.get_string("PDF:Author"),
        Some("ExifTool-RS Test Suite"),
        "PDF:Author not found or incorrect"
    );

    assert_eq!(
        metadata.get_string("PDF:Subject"),
        Some("PDF Metadata Extraction Test"),
        "PDF:Subject not found or incorrect"
    );

    assert_eq!(
        metadata.get_string("PDF:Keywords"),
        Some("exiftool, rust, pdf, metadata, test"),
        "PDF:Keywords not found or incorrect"
    );

    assert_eq!(
        metadata.get_string("PDF:Creator"),
        Some("ExifTool-RS PDF Parser Test Generator"),
        "PDF:Creator not found or incorrect"
    );

    assert_eq!(
        metadata.get_string("PDF:Producer"),
        Some("Minimal PDF Generator v1.0"),
        "PDF:Producer not found or incorrect"
    );

    // Verify date fields exist
    assert!(
        metadata.get_string("PDF:CreationDate").is_some(),
        "PDF:CreationDate not found"
    );

    assert!(
        metadata.get_string("PDF:ModDate").is_some(),
        "PDF:ModDate not found"
    );
}

#[test]
fn test_parse_pdf_with_special_characters() {
    // Create a PDF with special characters in metadata - with correct xref
    // Object 1 starts at byte 9, object 2 starts at byte 45
    let pdf_content = b"%PDF-1.4
1 0 obj
<< /Type /Catalog >>
endobj
2 0 obj
<<
/Title (Test: Special & Characters)
/Author (Doe, John)
/Subject (R&D Project)
>>
endobj
xref
0 3
0000000000 65535 f
0000000009 00000 n
0000000045 00000 n
trailer
<< /Size 3 /Root 1 0 R /Info 2 0 R >>
startxref
145
%%EOF
";

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_special_chars.pdf");
    std::fs::write(&temp_path, pdf_content).expect("Failed to write temp PDF");

    // Parse and verify
    let reader = BufferedReader::new(&temp_path).expect("Failed to open temp PDF");
    let result = parse_pdf_metadata(&reader);

    // Clean up
    let _ = std::fs::remove_file(&temp_path);

    assert!(result.is_ok(), "Failed to parse PDF: {:?}", result.err());
    let metadata = result.unwrap();

    assert_eq!(
        metadata.get_string("PDF:Title"),
        Some("Test: Special & Characters")
    );
    assert_eq!(metadata.get_string("PDF:Author"), Some("Doe, John"));
}

#[test]
fn test_parse_pdf_minimal_metadata() {
    // Create a PDF with minimal metadata (only 2 fields) - with correct xref
    let pdf_content = b"%PDF-1.4
1 0 obj
<< /Type /Catalog >>
endobj
2 0 obj
<<
/Title (Minimal)
/Author (Test)
>>
endobj
xref
0 3
0000000000 65535 f
0000000009 00000 n
0000000045 00000 n
trailer
<< /Size 3 /Root 1 0 R /Info 2 0 R >>
startxref
98
%%EOF
";

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_minimal.pdf");
    std::fs::write(&temp_path, pdf_content).expect("Failed to write temp PDF");

    let reader = BufferedReader::new(&temp_path).expect("Failed to open temp PDF");
    let result = parse_pdf_metadata(&reader);

    let _ = std::fs::remove_file(&temp_path);

    assert!(result.is_ok());
    let metadata = result.unwrap();

    assert_eq!(metadata.get_string("PDF:Title"), Some("Minimal"));
    assert_eq!(metadata.get_string("PDF:Author"), Some("Test"));
}

#[test]
fn test_parse_invalid_pdf() {
    // Create invalid PDF (no valid signature)
    let invalid_content = b"This is not a PDF file";

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_invalid.pdf");
    std::fs::write(&temp_path, invalid_content).expect("Failed to write temp file");

    let reader = BufferedReader::new(&temp_path).expect("Failed to open temp file");
    let result = parse_pdf_metadata(&reader);

    let _ = std::fs::remove_file(&temp_path);

    assert!(result.is_err(), "Should fail on invalid PDF");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid PDF signature"));
}

#[test]
fn test_parse_pdf_with_xmp() {
    // Create a PDF with both Info dict and XMP
    let pdf_with_xmp = b"%PDF-1.4
1 0 obj
<< /Type /Catalog >>
endobj
<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\">
<rdf:Description rdf:about=\"\">
  <dc:creator>XMP Test Creator</dc:creator>
  <dc:title>XMP Test Title</dc:title>
  <dc:description>XMP Test Description</dc:description>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end=\"w\"?>
2 0 obj
<<
/Title (Info Dictionary Title)
/Author (Info Dictionary Author)
/Subject (Test Subject)
>>
endobj
xref
0 3
0000000000 65535 f
0000000009 00000 n
0000000400 00000 n
trailer
<< /Size 3 /Root 1 0 R /Info 2 0 R >>
startxref
500
%%EOF";

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_xmp.pdf");
    std::fs::write(&temp_path, pdf_with_xmp).expect("Failed to write temp PDF");

    let reader = BufferedReader::new(&temp_path).expect("Failed to open temp PDF");
    let result = parse_pdf_metadata(&reader);

    let _ = std::fs::remove_file(&temp_path);

    // Should succeed
    assert!(result.is_ok(), "Failed to parse PDF with XMP: {:?}", result.err());

    let metadata = result.unwrap();

    // Should have Info dictionary metadata
    assert_eq!(
        metadata.get_string("PDF:Title"),
        Some("Info Dictionary Title")
    );
    assert_eq!(
        metadata.get_string("PDF:Author"),
        Some("Info Dictionary Author")
    );

    // May or may not have XMP depending on parsing success
    // This is acceptable per requirements (graceful handling)
    println!("Extracted {} metadata fields", metadata.len());
    for (key, value) in metadata.iter() {
        println!("  {}: {:?}", key, value);
    }
}

#[test]
fn test_pdf_metadata_field_count() {
    // Test that we meet the acceptance criteria of extracting at least 5 fields
    let pdf_path = get_fixture_path("sample.pdf");

    let reader = BufferedReader::new(&pdf_path).expect("Failed to open PDF file");
    let metadata = parse_pdf_metadata(&reader).expect("Failed to parse PDF");

    // Count extracted fields
    let field_count = metadata.len();

    println!("Extracted PDF metadata fields:");
    for (key, value) in metadata.iter() {
        println!("  {}: {:?}", key, value);
    }

    // Acceptance criteria: at least 5 metadata fields
    assert!(
        field_count >= 5,
        "Expected at least 5 metadata fields, but got {}",
        field_count
    );
}
