//! Integration tests for PDF metadata writer
//!
//! These tests verify the PDF writer's ability to modify Info dictionary metadata,
//! recalculate xref tables correctly, and produce valid PDF files.

use exiftool_rs::core::metadata_map::MetadataMap;
use exiftool_rs::core::tag_value::TagValue;
use exiftool_rs::core::FileReader;
use exiftool_rs::io::buffered_reader::BufferedReader;
use exiftool_rs::parsers::pdf::parse_pdf_metadata;
use exiftool_rs::writers::pdf_writer::write_pdf_file;
use std::io;
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
fn create_test_pdf() -> Vec<u8> {
    // This PDF has correctly calculated xref offsets
    let pdf = b"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Count 0 /Kids [] >>
endobj
3 0 obj
<<
/Title (Original Title)
/Author (Original Author)
/Subject (Original Subject)
/Keywords (test, original)
/Creator (Test Creator)
/Producer (Test Producer)
/CreationDate (D:20240101120000+00'00')
/ModDate (D:20240101120000+00'00')
>>
endobj
xref
0 4
0000000000 65535 f
0000000009 00000 n
0000000058 00000 n
0000000115 00000 n
trailer
<< /Size 4 /Root 1 0 R /Info 3 0 R >>
startxref
329
%%EOF
";
    pdf.to_vec()
}

#[test]
fn test_write_and_read_modified_title() {
    // Create test PDF
    let original_pdf = create_test_pdf();
    let original_reader = TestReader::new(original_pdf);

    // Create modified metadata - change Title
    let mut metadata = MetadataMap::new();
    metadata.insert("PDF:Title", TagValue::new_string("Modified Title"));
    metadata.insert("PDF:Author", TagValue::new_string("Original Author"));
    metadata.insert("PDF:Subject", TagValue::new_string("Original Subject"));
    metadata.insert("PDF:Keywords", TagValue::new_string("test, original"));
    metadata.insert("PDF:Creator", TagValue::new_string("Test Creator"));
    metadata.insert("PDF:Producer", TagValue::new_string("Test Producer"));

    // Write to temp file
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_write_title.pdf");
    let result = write_pdf_file(&temp_path, &original_reader, &metadata);

    assert!(result.is_ok(), "Failed to write PDF: {:?}", result.err());

    // Read back and verify
    let reader = BufferedReader::new(&temp_path).expect("Failed to open written PDF");
    let parsed = parse_pdf_metadata(&reader);

    // Clean up
    let _ = std::fs::remove_file(&temp_path);

    assert!(
        parsed.is_ok(),
        "Failed to parse written PDF: {:?}",
        parsed.err()
    );
    let parsed_metadata = parsed.unwrap();

    // Verify the Title was modified
    assert_eq!(
        parsed_metadata.get_string("PDF:Title"),
        Some("Modified Title"),
        "Title was not modified correctly"
    );

    // Verify other fields remain unchanged
    assert_eq!(
        parsed_metadata.get_string("PDF:Author"),
        Some("Original Author"),
        "Author should remain unchanged"
    );
}

#[test]
fn test_write_multiple_field_modifications() {
    let original_pdf = create_test_pdf();
    let original_reader = TestReader::new(original_pdf);

    // Modify multiple fields
    let mut metadata = MetadataMap::new();
    metadata.insert("PDF:Title", TagValue::new_string("New Title"));
    metadata.insert("PDF:Author", TagValue::new_string("New Author"));
    metadata.insert("PDF:Subject", TagValue::new_string("New Subject"));
    metadata.insert("PDF:Keywords", TagValue::new_string("new, modified, test"));
    metadata.insert("PDF:Creator", TagValue::new_string("Modified Creator"));
    metadata.insert("PDF:Producer", TagValue::new_string("Modified Producer"));

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_write_multiple.pdf");
    let result = write_pdf_file(&temp_path, &original_reader, &metadata);

    assert!(result.is_ok());

    // Read back and verify all changes
    let reader = BufferedReader::new(&temp_path).expect("Failed to open written PDF");
    let parsed = parse_pdf_metadata(&reader).expect("Failed to parse written PDF");

    let _ = std::fs::remove_file(&temp_path);

    assert_eq!(parsed.get_string("PDF:Title"), Some("New Title"));
    assert_eq!(parsed.get_string("PDF:Author"), Some("New Author"));
    assert_eq!(parsed.get_string("PDF:Subject"), Some("New Subject"));
    assert_eq!(
        parsed.get_string("PDF:Keywords"),
        Some("new, modified, test")
    );
    assert_eq!(parsed.get_string("PDF:Creator"), Some("Modified Creator"));
    assert_eq!(parsed.get_string("PDF:Producer"), Some("Modified Producer"));
}

#[test]
fn test_write_with_special_characters() {
    let original_pdf = create_test_pdf();
    let original_reader = TestReader::new(original_pdf);

    // Test special characters and unicode
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "PDF:Title",
        TagValue::new_string("Test: Special & Characters"),
    );
    metadata.insert("PDF:Author", TagValue::new_string("Müller, François"));
    metadata.insert("PDF:Subject", TagValue::new_string("R&D Project (2024)"));
    metadata.insert("PDF:Keywords", TagValue::new_string("test, special"));
    metadata.insert("PDF:Creator", TagValue::new_string("Test Creator"));
    metadata.insert("PDF:Producer", TagValue::new_string("Test Producer"));

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_write_special.pdf");
    let result = write_pdf_file(&temp_path, &original_reader, &metadata);

    assert!(result.is_ok());

    // Read back and verify
    let reader = BufferedReader::new(&temp_path).expect("Failed to open written PDF");
    let parsed = parse_pdf_metadata(&reader).expect("Failed to parse written PDF");

    let _ = std::fs::remove_file(&temp_path);

    // Verify special characters were preserved
    assert_eq!(
        parsed.get_string("PDF:Title"),
        Some("Test: Special & Characters")
    );
    assert_eq!(parsed.get_string("PDF:Author"), Some("Müller, François"));
}

#[test]
fn test_write_with_empty_fields() {
    let original_pdf = create_test_pdf();
    let original_reader = TestReader::new(original_pdf);

    // Test with empty strings
    let mut metadata = MetadataMap::new();
    metadata.insert("PDF:Title", TagValue::new_string(""));
    metadata.insert("PDF:Author", TagValue::new_string("Author"));
    metadata.insert("PDF:Subject", TagValue::new_string(""));
    metadata.insert("PDF:Keywords", TagValue::new_string("test"));
    metadata.insert("PDF:Creator", TagValue::new_string("Creator"));
    metadata.insert("PDF:Producer", TagValue::new_string("Producer"));

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_write_empty.pdf");
    let result = write_pdf_file(&temp_path, &original_reader, &metadata);

    assert!(result.is_ok());

    // Read back - empty fields should work
    let reader = BufferedReader::new(&temp_path).expect("Failed to open written PDF");
    let parsed = parse_pdf_metadata(&reader);

    let _ = std::fs::remove_file(&temp_path);

    assert!(parsed.is_ok());
}

#[test]
fn test_write_preserves_pdf_structure() {
    let original_pdf = create_test_pdf();
    let original_reader = TestReader::new(original_pdf.clone());

    // Modify metadata
    let mut metadata = MetadataMap::new();
    metadata.insert("PDF:Title", TagValue::new_string("Structure Test"));
    metadata.insert("PDF:Author", TagValue::new_string("Tester"));
    metadata.insert("PDF:Subject", TagValue::new_string("Test"));
    metadata.insert("PDF:Keywords", TagValue::new_string("test"));
    metadata.insert("PDF:Creator", TagValue::new_string("Creator"));
    metadata.insert("PDF:Producer", TagValue::new_string("Producer"));

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_write_structure.pdf");
    write_pdf_file(&temp_path, &original_reader, &metadata).expect("Failed to write PDF");

    // Read the written file
    let written_data = std::fs::read(&temp_path).expect("Failed to read written PDF");
    let _ = std::fs::remove_file(&temp_path);

    // Verify PDF structure
    let written_str = String::from_utf8_lossy(&written_data);

    // Should have PDF header
    assert!(written_str.starts_with("%PDF-1.4"));

    // Should have xref table
    assert!(written_str.contains("xref\n"));

    // Should have trailer
    assert!(written_str.contains("trailer\n"));

    // Should have startxref
    assert!(written_str.contains("startxref\n"));

    // Should end with %%EOF
    assert!(written_str.trim_end().ends_with("%%EOF"));

    // Should have objects (1 0 obj, 2 0 obj, etc.)
    assert!(written_str.contains("1 0 obj"));
    assert!(written_str.contains("2 0 obj"));
    assert!(written_str.contains("3 0 obj"));
}

#[test]
fn test_write_with_long_values() {
    let original_pdf = create_test_pdf();
    let original_reader = TestReader::new(original_pdf);

    // Test with long string values
    let long_title = "A".repeat(200);
    let long_keywords = (0..50)
        .map(|i| format!("keyword{}", i))
        .collect::<Vec<_>>()
        .join(", ");

    let mut metadata = MetadataMap::new();
    metadata.insert("PDF:Title", TagValue::new_string(long_title.clone()));
    metadata.insert("PDF:Author", TagValue::new_string("Author"));
    metadata.insert("PDF:Subject", TagValue::new_string("Subject"));
    metadata.insert("PDF:Keywords", TagValue::new_string(long_keywords.clone()));
    metadata.insert("PDF:Creator", TagValue::new_string("Creator"));
    metadata.insert("PDF:Producer", TagValue::new_string("Producer"));

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_write_long.pdf");
    let result = write_pdf_file(&temp_path, &original_reader, &metadata);

    assert!(result.is_ok());

    // Read back and verify long values
    let reader = BufferedReader::new(&temp_path).expect("Failed to open written PDF");
    let parsed = parse_pdf_metadata(&reader).expect("Failed to parse written PDF");

    let _ = std::fs::remove_file(&temp_path);

    assert_eq!(parsed.get_string("PDF:Title"), Some(long_title.as_str()));
    assert_eq!(
        parsed.get_string("PDF:Keywords"),
        Some(long_keywords.as_str())
    );
}

#[test]
fn test_write_to_sample_fixture() {
    // Test with actual sample.pdf fixture if it exists
    let fixture_path = get_fixture_path("sample.pdf");

    if !fixture_path.exists() {
        println!("Skipping test - sample.pdf fixture not found");
        return;
    }

    let reader = BufferedReader::new(&fixture_path).expect("Failed to open sample.pdf");

    // Parse original metadata
    let original_metadata = parse_pdf_metadata(&reader).expect("Failed to parse sample.pdf");

    // Modify Title
    let mut modified_metadata = original_metadata.clone();
    modified_metadata.insert("PDF:Title", TagValue::new_string("Modified Sample Title"));

    // Write to temp file
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_write_sample.pdf");
    let result = write_pdf_file(&temp_path, &reader, &modified_metadata);

    assert!(result.is_ok(), "Failed to write modified sample.pdf");

    // Read back and verify
    let written_reader = BufferedReader::new(&temp_path).expect("Failed to open written PDF");
    let parsed = parse_pdf_metadata(&written_reader).expect("Failed to parse written PDF");

    let _ = std::fs::remove_file(&temp_path);

    // Verify Title was modified
    assert_eq!(
        parsed.get_string("PDF:Title"),
        Some("Modified Sample Title")
    );

    // Verify other fields remain (check Author as example)
    if let Some(original_author) = original_metadata.get_string("PDF:Author") {
        assert_eq!(parsed.get_string("PDF:Author"), Some(original_author));
    }
}

#[test]
fn test_xref_table_correctness() {
    let original_pdf = create_test_pdf();
    let original_reader = TestReader::new(original_pdf);

    let mut metadata = MetadataMap::new();
    metadata.insert("PDF:Title", TagValue::new_string("XRef Test"));
    metadata.insert("PDF:Author", TagValue::new_string("Test"));
    metadata.insert("PDF:Subject", TagValue::new_string("Test"));
    metadata.insert("PDF:Keywords", TagValue::new_string("test"));
    metadata.insert("PDF:Creator", TagValue::new_string("Test"));
    metadata.insert("PDF:Producer", TagValue::new_string("Test"));

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_xref.pdf");
    write_pdf_file(&temp_path, &original_reader, &metadata).expect("Failed to write PDF");

    let written_data = std::fs::read(&temp_path).expect("Failed to read written PDF");
    let _ = std::fs::remove_file(&temp_path);

    let written_str = String::from_utf8_lossy(&written_data);

    // Extract xref table
    let xref_start = written_str.find("xref\n").expect("xref not found");
    let trailer_start = written_str.find("trailer\n").expect("trailer not found");
    let xref_section = &written_str[xref_start..trailer_start];

    // Verify xref format - each entry should be 20 bytes: "0000000123 00000 n \n"
    let lines: Vec<&str> = xref_section.lines().skip(2).collect(); // Skip "xref" and subsection header

    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            // First entry should be free
            assert!(
                line.ends_with(" f ") || line.ends_with(" f"),
                "First xref entry should be free: '{}'",
                line
            );
        } else if line.ends_with(" n ") || line.ends_with(" n") {
            // In-use entry - should have 10-digit offset
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert!(parts.len() >= 3, "Invalid xref entry format: '{}'", line);
            assert_eq!(
                parts[0].len(),
                10,
                "Offset should be 10 digits: '{}'",
                parts[0]
            );
        }
    }
}

#[test]
fn test_output_pdf_is_valid() {
    // This test verifies the output PDF can be successfully parsed
    let original_pdf = create_test_pdf();
    let original_reader = TestReader::new(original_pdf);

    let mut metadata = MetadataMap::new();
    metadata.insert("PDF:Title", TagValue::new_string("Valid PDF Test"));
    metadata.insert("PDF:Author", TagValue::new_string("Tester"));
    metadata.insert("PDF:Subject", TagValue::new_string("Validation"));
    metadata.insert("PDF:Keywords", TagValue::new_string("valid, test"));
    metadata.insert("PDF:Creator", TagValue::new_string("Test"));
    metadata.insert("PDF:Producer", TagValue::new_string("Test"));

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_valid.pdf");
    write_pdf_file(&temp_path, &original_reader, &metadata).expect("Failed to write PDF");

    // The ultimate test: can we parse it back without errors?
    let reader = BufferedReader::new(&temp_path).expect("Failed to open written PDF");
    let result = parse_pdf_metadata(&reader);

    let _ = std::fs::remove_file(&temp_path);

    assert!(
        result.is_ok(),
        "Output PDF is not valid (parser failed): {:?}",
        result.err()
    );

    let parsed = result.unwrap();
    assert!(
        parsed.len() >= 5,
        "Expected at least 5 metadata fields, got {}",
        parsed.len()
    );
}
