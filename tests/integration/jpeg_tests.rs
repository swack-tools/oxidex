//! Integration tests for end-to-end JPEG EXIF extraction
//!
//! This test validates the entire parsing pipeline from file reading through
//! format detection, segment parsing, and EXIF tag extraction.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::{FileFormat, FileReader};
use oxidex::io::MMapReader;
use oxidex::parsers::detection::detect_format;
use oxidex::parsers::jpeg::segment_parser::parse_segments;
use oxidex::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

/// Helper struct to wrap a slice of data and provide FileReader interface
/// starting at a specific offset within the original data.
struct SliceReader<'a> {
    data: &'a [u8],
}

impl<'a> SliceReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

impl<'a> FileReader for SliceReader<'a> {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        let start = offset as usize;
        let end = start + length;

        if end > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read beyond end of data",
            ));
        }

        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

/// Creates a minimal but valid JPEG file with EXIF metadata containing
/// Make, Model, and DateTime tags.
///
/// Structure:
/// - SOI marker (0xFFD8)
/// - APP1 segment with EXIF data:
///   - Marker: 0xFFE1
///   - Length field
///   - EXIF identifier: "Exif\0\0"
///   - TIFF header (little-endian)
///   - IFD with 3 tags (Make, Model, DateTime)
/// - Minimal SOS segment (Start of Scan - required for valid JPEG)
/// - EOI marker (0xFFD9)
fn create_jpeg_with_exif() -> Vec<u8> {
    let mut jpeg = Vec::new();

    // === JPEG SOI marker ===
    jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // === APP1 segment with EXIF ===
    // We'll build the APP1 payload first, then calculate its length

    let mut app1_payload = Vec::new();

    // EXIF identifier: "Exif\0\0"
    app1_payload.extend_from_slice(b"Exif\0\0");

    // === TIFF header (little-endian) ===
    let _tiff_header_start = app1_payload.len();

    // Byte order: "II" (little-endian)
    app1_payload.extend_from_slice(b"II");

    // Magic number: 0x002A (little-endian)
    app1_payload.extend_from_slice(&[0x2A, 0x00]);

    // IFD offset: 8 bytes from TIFF header start (4-byte value, little-endian)
    app1_payload.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]);

    // === IFD (Image File Directory) ===
    // The IFD offset points here (8 bytes from TIFF header start)

    // Number of entries: 3 tags (Make, Model, DateTime)
    app1_payload.extend_from_slice(&[0x03, 0x00]);

    // Prepare tag data that will be stored after the IFD
    let make_value = b"TestCamera\0"; // 11 bytes
    let datetime_value = b"2025:01:15 10:30:00\0"; // 20 bytes

    // Calculate offsets for out-of-line data
    // IFD structure: 2 (count) + 3*12 (entries) + 4 (next IFD offset) = 42 bytes
    // Out-of-line data starts at: tiff_header_start + 8 (tiff header) + 42 (IFD) = tiff_header_start + 50
    let data_section_offset = 8 + 42; // Relative to TIFF header start

    let make_offset = data_section_offset;
    let datetime_offset = make_offset + make_value.len();

    // === Tag Entry 1: Make (0x010F) ===
    // Tag ID: 0x010F (little-endian)
    app1_payload.extend_from_slice(&[0x0F, 0x01]);
    // Type: 2 (ASCII)
    app1_payload.extend_from_slice(&[0x02, 0x00]);
    // Count: 11 (length of "TestCamera\0")
    app1_payload.extend_from_slice(&[0x0B, 0x00, 0x00, 0x00]);
    // Offset to value (relative to TIFF header start)
    let make_offset_bytes = (make_offset as u32).to_le_bytes();
    app1_payload.extend_from_slice(&make_offset_bytes);

    // === Tag Entry 2: Model (0x0110) - inline value ===
    // Tag ID: 0x0110 (little-endian)
    app1_payload.extend_from_slice(&[0x10, 0x01]);
    // Type: 2 (ASCII, little-endian)
    app1_payload.extend_from_slice(&[0x02, 0x00]);
    // Count: 3 (length of "TM\0", little-endian)
    app1_payload.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]);
    // Inline value: "TM\0" padded with 0x00 (left-justified)
    app1_payload.extend_from_slice(b"TM\0\0");

    // === Tag Entry 3: DateTime (0x0132) ===
    // Tag ID: 0x0132
    app1_payload.extend_from_slice(&[0x32, 0x01]);
    // Type: 2 (ASCII)
    app1_payload.extend_from_slice(&[0x02, 0x00]);
    // Count: 20 (length of "2025:01:15 10:30:00\0")
    app1_payload.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]);
    // Offset to value
    let datetime_offset_bytes = (datetime_offset as u32).to_le_bytes();
    app1_payload.extend_from_slice(&datetime_offset_bytes);

    // === Next IFD offset (0 = no next IFD) ===
    app1_payload.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // === Out-of-line tag value data ===
    // Make value at offset 50 from TIFF header
    app1_payload.extend_from_slice(make_value);
    // DateTime value immediately after Make
    app1_payload.extend_from_slice(datetime_value);

    // Now build the complete APP1 segment
    jpeg.push(0xFF);
    jpeg.push(0xE1); // APP1 marker

    // Length: 2 bytes for length field itself + payload size
    let app1_length = 2 + app1_payload.len();
    jpeg.extend_from_slice(&(app1_length as u16).to_be_bytes());

    // APP1 payload
    jpeg.extend_from_slice(&app1_payload);

    // === Minimal Start of Scan segment (required for valid JPEG) ===
    // SOS marker
    jpeg.extend_from_slice(&[0xFF, 0xDA]);
    // Length: minimal (12 bytes is typical for baseline JPEG)
    jpeg.extend_from_slice(&[0x00, 0x0C]);
    // Number of components: 3 (Y, Cb, Cr)
    jpeg.push(0x03);
    // Component 1: Y
    jpeg.extend_from_slice(&[0x01, 0x00]);
    // Component 2: Cb
    jpeg.extend_from_slice(&[0x02, 0x11]);
    // Component 3: Cr
    jpeg.extend_from_slice(&[0x03, 0x11]);
    // Spectral selection: 0, 63
    jpeg.extend_from_slice(&[0x00, 0x3F]);
    // Successive approximation: 0
    jpeg.push(0x00);

    // Minimal compressed image data (just a few bytes)
    jpeg.extend_from_slice(&[0xFF, 0x00, 0xD9]); // Stuffed 0xFF and some data

    // === JPEG EOI marker ===
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    jpeg
}

/// Creates a JPEG file with both EXIF and XMP metadata.
///
/// Structure:
/// - SOI marker (0xFFD8)
/// - APP1 segment with EXIF data
/// - APP1 segment with XMP data
/// - Minimal SOS segment
/// - EOI marker (0xFFD9)
fn create_jpeg_with_exif_and_xmp() -> Vec<u8> {
    let mut jpeg = Vec::new();

    // === JPEG SOI marker ===
    jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // === APP1 segment with EXIF ===
    let mut exif_payload = Vec::new();

    // EXIF identifier: "Exif\0\0"
    exif_payload.extend_from_slice(b"Exif\0\0");

    // === TIFF header (little-endian) ===
    // Byte order: "II" (little-endian)
    exif_payload.extend_from_slice(b"II");
    // Magic number: 0x002A
    exif_payload.extend_from_slice(&[0x2A, 0x00]);
    // IFD offset: 8 bytes from TIFF header start
    exif_payload.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]);

    // === IFD with 2 tags (Make, Model) ===
    // Number of entries: 2
    exif_payload.extend_from_slice(&[0x02, 0x00]);

    let make_value = b"TestCamera\0"; // 11 bytes
    let data_section_offset = 8 + 2 + (2 * 12) + 4; // tiff header + count + entries + next IFD
    let make_offset = data_section_offset;

    // Tag Entry 1: Make (0x010F)
    exif_payload.extend_from_slice(&[0x0F, 0x01]); // Tag ID
    exif_payload.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    exif_payload.extend_from_slice(&[0x0B, 0x00, 0x00, 0x00]); // Count: 11
    let make_offset_bytes = (make_offset as u32).to_le_bytes();
    exif_payload.extend_from_slice(&make_offset_bytes); // Offset

    // Tag Entry 2: Model (0x0110) - inline value
    exif_payload.extend_from_slice(&[0x10, 0x01]); // Tag ID
    exif_payload.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    exif_payload.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // Count: 3
    exif_payload.extend_from_slice(b"TM\0\0"); // Inline value

    // Next IFD offset (0 = no next IFD)
    exif_payload.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // Out-of-line tag value data
    exif_payload.extend_from_slice(make_value);

    // Build complete EXIF APP1 segment
    jpeg.push(0xFF);
    jpeg.push(0xE1); // APP1 marker
    let exif_length = 2 + exif_payload.len();
    jpeg.extend_from_slice(&(exif_length as u16).to_be_bytes());
    jpeg.extend_from_slice(&exif_payload);

    // === APP1 segment with XMP ===
    let mut xmp_payload = Vec::new();

    // XMP identifier: "http://ns.adobe.com/xap/1.0/\0"
    xmp_payload.extend_from_slice(b"http://ns.adobe.com/xap/1.0/\0");

    // XMP XML data
    let xmp_xml = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/"
                     xmlns:dc="http://purl.org/dc/elements/1.1/">
      <xmp:Creator>John Doe</xmp:Creator>
      <xmp:Rating>5</xmp:Rating>
      <dc:title>Sample Photo</dc:title>
      <dc:rights>Copyright 2024</dc:rights>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

    xmp_payload.extend_from_slice(xmp_xml);

    // Build complete XMP APP1 segment
    jpeg.push(0xFF);
    jpeg.push(0xE1); // APP1 marker
    let xmp_length = 2 + xmp_payload.len();
    jpeg.extend_from_slice(&(xmp_length as u16).to_be_bytes());
    jpeg.extend_from_slice(&xmp_payload);

    // === Minimal Start of Scan segment ===
    jpeg.extend_from_slice(&[0xFF, 0xDA]); // SOS marker
    jpeg.extend_from_slice(&[0x00, 0x0C]); // Length: 12 bytes
    jpeg.push(0x03); // Number of components: 3
    jpeg.extend_from_slice(&[0x01, 0x00]); // Component 1: Y
    jpeg.extend_from_slice(&[0x02, 0x11]); // Component 2: Cb
    jpeg.extend_from_slice(&[0x03, 0x11]); // Component 3: Cr
    jpeg.extend_from_slice(&[0x00, 0x3F]); // Spectral selection
    jpeg.push(0x00); // Successive approximation

    // Minimal compressed image data
    jpeg.extend_from_slice(&[0xFF, 0x00, 0xD9]);

    // === JPEG EOI marker ===
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    jpeg
}

/// Creates the test fixture files if they don't exist or are outdated.
fn ensure_test_fixtures() -> std::io::Result<()> {
    let fixture_dir = Path::new("tests/fixtures/jpeg");
    std::fs::create_dir_all(fixture_dir)?;

    // Create EXIF-only fixture
    let exif_fixture_path = fixture_dir.join("sample_with_exif.jpg");
    let jpeg_data = create_jpeg_with_exif();
    write_fixture_atomically(&exif_fixture_path, &jpeg_data)?;

    // Create EXIF+XMP fixture
    let exif_xmp_fixture_path = fixture_dir.join("sample_with_exif_xmp.jpg");
    let jpeg_data_xmp = create_jpeg_with_exif_and_xmp();
    write_fixture_atomically(&exif_xmp_fixture_path, &jpeg_data_xmp)?;

    Ok(())
}

fn write_fixture_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(data)?;
    temp.flush()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[test]
fn test_jpeg_exif_extraction_end_to_end() {
    // === Step 0: Setup test fixtures ===
    ensure_test_fixtures().expect("Failed to create test fixtures");

    let fixture_path = Path::new("tests/fixtures/jpeg/sample_with_exif.jpg");

    println!("\n=== JPEG EXIF Extraction Integration Test ===\n");

    // === Step 1: Open file with MMapReader ===
    println!("Step 1: Opening JPEG file with MMapReader...");
    let reader = MMapReader::new(fixture_path).expect("Failed to open JPEG file");
    println!("  ✓ File opened successfully ({} bytes)", reader.size());

    // === Step 2: Detect format ===
    println!("\nStep 2: Detecting file format...");
    let format = detect_format(&reader).expect("Failed to detect format");
    println!("  ✓ Detected format: {:?}", format);
    assert_eq!(
        format,
        FileFormat::JPEG,
        "Format should be detected as JPEG"
    );

    // === Step 3: Parse JPEG segments ===
    println!("\nStep 3: Parsing JPEG segments...");
    let segments = parse_segments(&reader).expect("Failed to parse JPEG segments");
    println!("  ✓ Found {} segments", segments.len());

    // Print segment information
    for segment in &segments {
        println!(
            "    - Marker: 0x{:04X}, Offset: {}, Data size: {} bytes",
            segment.marker,
            segment.offset,
            segment.data.len()
        );
    }

    // === Step 4: Find APP1 segment with EXIF ===
    println!("\nStep 4: Locating APP1 segment with EXIF data...");
    let app1_segment = segments
        .iter()
        .find(|s| s.is_app1() && s.data.starts_with(b"Exif\0\0"))
        .expect("No APP1 segment with EXIF found");

    println!("  ✓ Found APP1 segment at offset {}", app1_segment.offset);
    println!(
        "    EXIF identifier: {:?}",
        &app1_segment.data[0..6]
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ")
    );

    // === Step 5: Extract TIFF data (skip "Exif\0\0" header) ===
    println!("\nStep 5: Extracting TIFF data from APP1 segment...");
    let tiff_data = &app1_segment.data[6..]; // Skip "Exif\0\0"
    println!("  ✓ TIFF data starts at byte 6 of APP1 payload");
    println!("    TIFF data size: {} bytes", tiff_data.len());

    // === Step 6: Detect byte order ===
    println!("\nStep 6: Detecting TIFF byte order...");
    assert!(
        tiff_data.len() >= 8,
        "TIFF data too small for header (need at least 8 bytes)"
    );

    let byte_order = if tiff_data.starts_with(b"II") {
        ByteOrder::LittleEndian
    } else if tiff_data.starts_with(b"MM") {
        ByteOrder::BigEndian
    } else {
        panic!("Invalid TIFF byte order marker");
    };
    println!("  ✓ Byte order: {:?}", byte_order);

    // Verify TIFF magic number (0x002A for LE, 0x2A00 for BE)
    let magic = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([tiff_data[2], tiff_data[3]]),
        ByteOrder::BigEndian => u16::from_be_bytes([tiff_data[2], tiff_data[3]]),
    };
    assert_eq!(magic, 0x002A, "Invalid TIFF magic number");
    println!("    Magic number: 0x{:04X} ✓", magic);

    // Read IFD offset
    let ifd_offset = match byte_order {
        ByteOrder::LittleEndian => {
            u32::from_le_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]])
        }
        ByteOrder::BigEndian => {
            u32::from_be_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]])
        }
    };
    println!("    IFD offset: {} bytes from TIFF header", ifd_offset);

    // === Step 7: Parse IFD ===
    println!("\nStep 7: Parsing TIFF IFD...");

    // Create a SliceReader for the TIFF data
    let tiff_reader = SliceReader::new(tiff_data);

    let tags = parse_ifd(&tiff_reader, ifd_offset as u64, byte_order).expect("Failed to parse IFD");

    println!("  ✓ Parsed {} tags from IFD", tags.len());

    // === Step 8: Extract and decode specific tags ===
    println!("\nStep 8: Extracting Make, Model, and DateTime tags...");

    // Tag IDs
    const TAG_MAKE: u16 = 0x010F;
    const TAG_MODEL: u16 = 0x0110;
    const TAG_DATETIME: u16 = 0x0132;

    let mut make_value = String::new();
    let mut model_value = String::new();
    let mut datetime_value = String::new();

    for (tag_id, _field_type, _value_count, value_bytes) in &tags {
        match *tag_id {
            TAG_MAKE => {
                make_value = String::from_utf8_lossy(value_bytes)
                    .trim_end_matches('\0')
                    .to_string();
                println!("  ✓ Make (0x{:04X}): '{}'", tag_id, make_value);
            }
            TAG_MODEL => {
                model_value = String::from_utf8_lossy(value_bytes)
                    .trim_end_matches('\0')
                    .to_string();
                println!("  ✓ Model (0x{:04X}): '{}'", tag_id, model_value);
            }
            TAG_DATETIME => {
                datetime_value = String::from_utf8_lossy(value_bytes)
                    .trim_end_matches('\0')
                    .to_string();
                println!("  ✓ DateTime (0x{:04X}): '{}'", tag_id, datetime_value);
            }
            _ => {
                println!("    Tag 0x{:04X}: {} bytes", tag_id, value_bytes.len());
            }
        }
    }

    // === Step 9: Verify extracted values ===
    println!("\nStep 9: Verifying extracted tag values...");

    assert!(!make_value.is_empty(), "Make tag value should be non-empty");
    assert!(
        !model_value.is_empty(),
        "Model tag value should be non-empty"
    );
    assert!(
        !datetime_value.is_empty(),
        "DateTime tag value should be non-empty"
    );

    println!("  ✓ All tag values are non-empty strings");

    // Verify expected values match what we created
    assert_eq!(make_value, "TestCamera");
    assert_eq!(model_value, "TM");
    assert_eq!(datetime_value, "2025:01:15 10:30:00");

    println!("\n=== Test Summary ===");
    println!("Make:     {}", make_value);
    println!("Model:    {}", model_value);
    println!("DateTime: {}", datetime_value);
    println!("\n✓ All integration test assertions passed!\n");
}

#[test]
fn test_jpeg_xmp_extraction_end_to_end() {
    use oxidex::parsers::jpeg::xmp_parser::extract_xmp_from_segments;

    // === Step 0: Setup test fixtures ===
    ensure_test_fixtures().expect("Failed to create test fixtures");

    let fixture_path = Path::new("tests/fixtures/jpeg/sample_with_exif_xmp.jpg");

    println!("\n=== JPEG XMP Extraction Integration Test ===\n");

    // === Step 1: Open file with MMapReader ===
    println!("Step 1: Opening JPEG file with MMapReader...");
    let reader = MMapReader::new(fixture_path).expect("Failed to open JPEG file");
    println!("  ✓ File opened successfully ({} bytes)", reader.size());

    // === Step 2: Detect format ===
    println!("\nStep 2: Detecting file format...");
    let format = detect_format(&reader).expect("Failed to detect format");
    println!("  ✓ Detected format: {:?}", format);
    assert_eq!(
        format,
        FileFormat::JPEG,
        "Format should be detected as JPEG"
    );

    // === Step 3: Parse JPEG segments ===
    println!("\nStep 3: Parsing JPEG segments...");
    let segments = parse_segments(&reader).expect("Failed to parse JPEG segments");
    println!("  ✓ Found {} segments", segments.len());

    // Print segment information
    for segment in &segments {
        println!(
            "    - Marker: 0x{:04X}, Offset: {}, Data size: {} bytes",
            segment.marker,
            segment.offset,
            segment.data.len()
        );
    }

    // === Step 4: Find APP1 segments (both EXIF and XMP) ===
    println!("\nStep 4: Locating APP1 segments...");
    let app1_segments: Vec<_> = segments.iter().filter(|s| s.is_app1()).collect();
    println!("  ✓ Found {} APP1 segments", app1_segments.len());

    // Verify we have both EXIF and XMP
    let has_exif = app1_segments
        .iter()
        .any(|s| s.data.starts_with(b"Exif\0\0"));
    let has_xmp = app1_segments
        .iter()
        .any(|s| s.data.starts_with(b"http://ns.adobe.com/xap/1.0/\0"));

    assert!(has_exif, "Should have EXIF APP1 segment");
    assert!(has_xmp, "Should have XMP APP1 segment");
    println!("    - Found EXIF APP1 segment");
    println!("    - Found XMP APP1 segment");

    // === Step 5: Extract XMP metadata ===
    println!("\nStep 5: Extracting XMP metadata...");
    let xmp_tags = extract_xmp_from_segments(&segments).expect("Failed to extract XMP");
    println!("  ✓ Extracted {} XMP tags", xmp_tags.len());

    // Print all XMP tags
    for (tag_name, value) in &xmp_tags {
        println!("    - {}: {}", tag_name, value);
    }

    // === Step 6: Verify at least 3 XMP tags extracted ===
    println!("\nStep 6: Verifying XMP tag extraction...");
    assert!(
        xmp_tags.len() >= 3,
        "Should extract at least 3 XMP tags, got {}",
        xmp_tags.len()
    );
    println!("  ✓ Extracted {} XMP tags (>= 3 required)", xmp_tags.len());

    // === Step 7: Verify specific XMP tags ===
    println!("\nStep 7: Verifying specific XMP tag values...");

    // Check for Creator
    let creator_tags: Vec<_> = xmp_tags
        .iter()
        .filter(|(name, _)| name == "XMP:Creator")
        .collect();
    assert_eq!(
        creator_tags.len(),
        1,
        "Should have exactly one XMP:Creator tag"
    );
    assert_eq!(
        creator_tags[0].1, "John Doe",
        "XMP:Creator should be 'John Doe'"
    );
    println!("  ✓ XMP:Creator: {}", creator_tags[0].1);

    // Check for Rating
    let rating_tags: Vec<_> = xmp_tags
        .iter()
        .filter(|(name, _)| name == "XMP:Rating")
        .collect();
    assert_eq!(
        rating_tags.len(),
        1,
        "Should have exactly one XMP:Rating tag"
    );
    assert_eq!(rating_tags[0].1, "5", "XMP:Rating should be '5'");
    println!("  ✓ XMP:Rating: {}", rating_tags[0].1);

    // Check for title (dc:title)
    let title_tags: Vec<_> = xmp_tags
        .iter()
        .filter(|(name, _)| name == "XMP:Title")
        .collect();
    assert_eq!(title_tags.len(), 1, "Should have exactly one XMP:Title tag");
    assert_eq!(
        title_tags[0].1, "Sample Photo",
        "XMP:Title should be 'Sample Photo'"
    );
    println!("  ✓ XMP:Title: {}", title_tags[0].1);

    // Check for rights (dc:rights)
    let rights_tags: Vec<_> = xmp_tags
        .iter()
        .filter(|(name, _)| name == "XMP:Rights")
        .collect();
    assert_eq!(
        rights_tags.len(),
        1,
        "Should have exactly one XMP:Rights tag"
    );
    assert_eq!(
        rights_tags[0].1, "Copyright 2024",
        "XMP:Rights should be 'Copyright 2024'"
    );
    println!("  ✓ XMP:Rights: {}", rights_tags[0].1);

    // === Step 8: Verify both EXIF and XMP can coexist ===
    println!("\nStep 8: Verifying EXIF and XMP coexistence...");

    // Extract EXIF data as well
    let exif_segment = segments
        .iter()
        .find(|s| s.is_app1() && s.data.starts_with(b"Exif\0\0"))
        .expect("No APP1 segment with EXIF found");

    let tiff_data = &exif_segment.data[6..]; // Skip "Exif\0\0"

    // Detect byte order
    let byte_order = if tiff_data.starts_with(b"II") {
        ByteOrder::LittleEndian
    } else if tiff_data.starts_with(b"MM") {
        ByteOrder::BigEndian
    } else {
        panic!("Invalid TIFF byte order marker");
    };

    // Read IFD offset
    let ifd_offset = match byte_order {
        ByteOrder::LittleEndian => {
            u32::from_le_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]])
        }
        ByteOrder::BigEndian => {
            u32::from_be_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]])
        }
    };

    // Create a SliceReader for TIFF data
    let tiff_reader = SliceReader::new(tiff_data);

    // Parse EXIF IFD
    let exif_tags =
        parse_ifd(&tiff_reader, ifd_offset as u64, byte_order).expect("Failed to parse EXIF IFD");

    println!("  ✓ Successfully extracted {} EXIF tags", exif_tags.len());
    println!("  ✓ Successfully extracted {} XMP tags", xmp_tags.len());
    println!("  ✓ Both EXIF and XMP metadata coexist in the same JPEG");

    // === Test Summary ===
    println!("\n=== Test Summary ===");
    println!("EXIF tags extracted: {}", exif_tags.len());
    println!("XMP tags extracted:  {}", xmp_tags.len());
    println!("\nXMP Tag Values:");
    println!("  Creator: {}", creator_tags[0].1);
    println!("  Rating:  {}", rating_tags[0].1);
    println!("  Title:   {}", title_tags[0].1);
    println!("  Rights:  {}", rights_tags[0].1);
    println!("\n✓ All integration test assertions passed!\n");
}

#[test]
fn test_jpeg_with_iptc_metadata() {
    use oxidex::parsers::jpeg::iptc_parser::extract_iptc_from_segments;
    use oxidex::parsers::jpeg::segment_parser::parse_segments;

    // Create minimal JPEG with APP13 (IPTC) segment
    let mut jpeg_data = Vec::new();

    // SOI marker
    jpeg_data.extend_from_slice(&[0xFF, 0xD8]);

    // APP13 marker
    jpeg_data.extend_from_slice(&[0xFF, 0xED]);

    // Create IPTC data
    let mut iptc_payload = Vec::new();
    iptc_payload.extend_from_slice(b"Photoshop 3.0\0"); // Signature
    iptc_payload.extend_from_slice(b"8BIM"); // 8BIM signature
    iptc_payload.extend_from_slice(&[0x04, 0x04]); // IPTC resource ID
    iptc_payload.push(0x00); // Empty name
    iptc_payload.push(0x00); // Padding

    // IPTC IIM records
    let mut iptc_records = Vec::new();
    iptc_records.push(0x1C); // Tag marker
    iptc_records.extend_from_slice(&[0x02, 0x05]); // Record 2, Dataset 5 (ObjectName)
    iptc_records.extend_from_slice(&[0x00, 0x0A]); // Length: 10
    iptc_records.extend_from_slice(b"IPTC Title");

    let iptc_size = iptc_records.len() as u32;
    iptc_payload.extend_from_slice(&iptc_size.to_be_bytes());
    iptc_payload.extend_from_slice(&iptc_records);

    // APP13 length
    let app13_length = (iptc_payload.len() + 2) as u16;
    jpeg_data.extend_from_slice(&app13_length.to_be_bytes());
    jpeg_data.extend_from_slice(&iptc_payload);

    // EOI marker
    jpeg_data.extend_from_slice(&[0xFF, 0xD9]);

    // Parse segments
    let reader = TestReader::new(jpeg_data);
    let segments = parse_segments(&reader).expect("Failed to parse segments");

    // Extract IPTC
    let iptc_tags = extract_iptc_from_segments(&segments).expect("Failed to extract IPTC");

    assert_eq!(iptc_tags.len(), 1);
    assert_eq!(iptc_tags[0].0, "IPTC:ObjectName");
    assert_eq!(iptc_tags[0].1, "IPTC Title");
}

/// Creates a minimal valid 128-byte ICC profile header for testing.
///
/// This creates a minimal sRGB-like display profile with just the header.
/// The header contains:
/// - Profile size (128 bytes - header only)
/// - CMM type: "none"
/// - Version: 2.1.0
/// - Profile class: Display Device Profile ("mntr")
/// - Color space: RGB
/// - PCS: XYZ
/// - Date/time: 2024-01-01 00:00:00
/// - Signature: "acsp"
/// - Platform: Apple ("APPL")
/// - Rendering intent: Perceptual
fn create_minimal_icc_profile() -> Vec<u8> {
    let mut profile = vec![0u8; 128];

    // Profile size (128 bytes) at offset 0 - big-endian u32
    profile[0..4].copy_from_slice(&128u32.to_be_bytes());

    // CMM Type at offset 4: "none" (4 bytes)
    profile[4..8].copy_from_slice(b"none");

    // Profile version at offset 8: 2.1.0
    // Byte 8: major version (2)
    // Byte 9: minor.bugfix (0x10 = 1.0)
    profile[8] = 2;
    profile[9] = 0x10;
    profile[10] = 0;
    profile[11] = 0;

    // Profile class at offset 12: Display Device Profile ("mntr")
    profile[12..16].copy_from_slice(b"mntr");

    // Color space at offset 16: RGB ("RGB ")
    profile[16..20].copy_from_slice(b"RGB ");

    // Profile Connection Space at offset 20: XYZ ("XYZ ")
    profile[20..24].copy_from_slice(b"XYZ ");

    // Date/time at offset 24 (12 bytes):
    // Year (2024), Month (1), Day (1), Hour (0), Minute (0), Second (0)
    profile[24..26].copy_from_slice(&2024u16.to_be_bytes()); // Year
    profile[26..28].copy_from_slice(&1u16.to_be_bytes()); // Month
    profile[28..30].copy_from_slice(&1u16.to_be_bytes()); // Day
    profile[30..32].copy_from_slice(&0u16.to_be_bytes()); // Hour
    profile[32..34].copy_from_slice(&0u16.to_be_bytes()); // Minute
    profile[34..36].copy_from_slice(&0u16.to_be_bytes()); // Second

    // Profile file signature at offset 36: "acsp" (required)
    profile[36..40].copy_from_slice(b"acsp");

    // Primary platform at offset 40: Apple ("APPL")
    profile[40..44].copy_from_slice(b"APPL");

    // CMM flags at offset 44: 0 (not embedded, independent)
    profile[44..48].copy_from_slice(&0u32.to_be_bytes());

    // Device manufacturer at offset 48: "TEST"
    profile[48..52].copy_from_slice(b"TEST");

    // Device model at offset 52: "MOD1"
    profile[52..56].copy_from_slice(b"MOD1");

    // Device attributes at offset 56 (8 bytes): 0 (reflective, glossy, positive, color)
    profile[56..64].copy_from_slice(&0u64.to_be_bytes());

    // Rendering intent at offset 64: 0 (Perceptual)
    profile[64..68].copy_from_slice(&0u32.to_be_bytes());

    // Connection space illuminant at offset 68 (12 bytes - XYZ s15.16 fixed-point)
    // D50 illuminant: X=0.9642, Y=1.0, Z=0.8249
    // s15.16 format: integer part in high 16 bits, fraction in low 16 bits
    // 0.9642 * 65536 = 63189.7 -> 0x0000F6D5
    // 1.0 * 65536 = 65536 -> 0x00010000
    // 0.8249 * 65536 = 54061.7 -> 0x0000D32D
    profile[68..72].copy_from_slice(&0x0000F6D5u32.to_be_bytes()); // X
    profile[72..76].copy_from_slice(&0x00010000u32.to_be_bytes()); // Y
    profile[76..80].copy_from_slice(&0x0000D32Du32.to_be_bytes()); // Z

    // Profile creator at offset 80: "TEST"
    profile[80..84].copy_from_slice(b"TEST");

    // Profile ID at offset 84 (16 bytes): zeros (not computed)
    // Already zeros from initialization

    // Tag count at offset 128 would normally be here, but for minimal profile
    // we just have the header (0 tags)

    profile
}

#[test]
fn test_jpeg_with_icc_profile() {
    use oxidex::core::MetadataMap;
    use oxidex::core::jpeg_helpers::process_icc_segments;
    use oxidex::parsers::jpeg::segment_parser::parse_segments;

    // Create minimal JPEG with APP2 (ICC) segment
    let mut jpeg_data = Vec::new();

    // SOI marker
    jpeg_data.extend_from_slice(&[0xFF, 0xD8]);

    // APP2 marker (ICC Profile)
    jpeg_data.extend_from_slice(&[0xFF, 0xE2]);

    // Create ICC profile payload
    let mut icc_payload = Vec::new();
    icc_payload.extend_from_slice(b"ICC_PROFILE\0"); // 12 bytes identifier
    icc_payload.push(1); // Chunk number (1)
    icc_payload.push(1); // Total chunks (1)

    // Add minimal ICC profile data
    let icc_profile = create_minimal_icc_profile();
    icc_payload.extend_from_slice(&icc_profile);

    // APP2 length (includes length field itself)
    let app2_length = (icc_payload.len() + 2) as u16;
    jpeg_data.extend_from_slice(&app2_length.to_be_bytes());
    jpeg_data.extend_from_slice(&icc_payload);

    // EOI marker
    jpeg_data.extend_from_slice(&[0xFF, 0xD9]);

    // Parse segments
    let reader = TestReader::new(jpeg_data);
    let segments = parse_segments(&reader).expect("Failed to parse segments");

    // Verify we found the APP2 segment
    let app2_segments: Vec<_> = segments.iter().filter(|s| s.marker == 0xFFE2).collect();
    assert_eq!(
        app2_segments.len(),
        1,
        "Should have exactly one APP2 segment"
    );

    // Verify segment has ICC_PROFILE identifier
    assert!(
        app2_segments[0].data.starts_with(b"ICC_PROFILE\0"),
        "APP2 segment should start with ICC_PROFILE identifier"
    );

    // Extract ICC metadata using the process_icc_segments function
    let mut metadata = MetadataMap::new();
    process_icc_segments(&segments, &mut metadata);

    // Verify ICC tags were extracted
    println!("Extracted ICC tags:");
    for (key, value) in metadata.iter() {
        println!("  {}: {:?}", key, value);
    }

    // Check for expected ICC profile header fields
    assert!(
        metadata.contains_key("ICC_Profile:ProfileVersion"),
        "Should have ICC_Profile:ProfileVersion tag"
    );
    assert!(
        metadata.contains_key("ICC_Profile:ProfileClass"),
        "Should have ICC_Profile:ProfileClass tag"
    );
    assert!(
        metadata.contains_key("ICC_Profile:ColorSpaceData"),
        "Should have ICC_Profile:ColorSpaceData tag"
    );
    assert!(
        metadata.contains_key("ICC_Profile:RenderingIntent"),
        "Should have ICC_Profile:RenderingIntent tag"
    );

    // Verify specific values
    let version = metadata.get("ICC_Profile:ProfileVersion").unwrap();
    assert!(
        format!("{:?}", version).contains("2.1"),
        "Profile version should be 2.1.0"
    );

    let profile_class = metadata.get("ICC_Profile:ProfileClass").unwrap();
    assert!(
        format!("{:?}", profile_class).contains("Display Device"),
        "Profile class should be Display Device"
    );

    let color_space = metadata.get("ICC_Profile:ColorSpaceData").unwrap();
    assert!(
        format!("{:?}", color_space).contains("RGB"),
        "Color space should be RGB"
    );

    println!("\nICC profile extraction test passed!");
}

#[test]
fn test_xmp_flows_to_metadata_map_via_read_metadata() {
    use oxidex::core::read_metadata;

    // === Setup ===
    ensure_test_fixtures().expect("Failed to create test fixtures");
    let path = Path::new("tests/fixtures/jpeg/sample_with_exif_xmp.jpg");

    println!("\n=== XMP Flow to MetadataMap Test ===\n");
    println!("Testing that XMP tags flow through read_metadata() API to final MetadataMap");

    // === Execute ===
    println!("\nStep 1: Calling read_metadata()...");
    let metadata = read_metadata(path).expect("Failed to read metadata");
    println!("  ✓ Successfully read metadata");
    println!("  Total tags: {}", metadata.len());

    // === Analyze ===
    println!("\nStep 2: Analyzing MetadataMap contents...");

    let mut xmp_tags = Vec::new();
    let mut exif_tags = Vec::new();
    let mut file_tags = Vec::new();
    let mut other_tags = Vec::new();

    for (key, value) in metadata.iter() {
        // XMP tags can be XMP: (simplified) or XMP-namespace: (specific)
        if key.starts_with("XMP-") || key.starts_with("XMP:") {
            xmp_tags.push((key, value));
        } else if key.starts_with("IFD0:") || key.starts_with("EXIF:") {
            exif_tags.push((key, value));
        } else if key.starts_with("File:") {
            file_tags.push((key, value));
        } else {
            other_tags.push((key, value));
        }
    }

    println!("  - File tags: {}", file_tags.len());
    println!("  - EXIF tags: {}", exif_tags.len());
    println!("  - XMP tags:  {}", xmp_tags.len());
    println!("  - Other tags: {}", other_tags.len());

    // === Verify XMP tags present ===
    println!("\nStep 3: Verifying XMP tags are present...");

    assert!(
        !xmp_tags.is_empty(),
        "❌ CRITICAL FAILURE: No XMP tags found in MetadataMap!\n\
         This indicates XMP data is extracted but NOT flowing to final output.\n\
         Check process_xmp_segments() integration in parse_jpeg_metadata()."
    );

    println!("  ✓ Found {} XMP tags", xmp_tags.len());

    // === Display XMP tags ===
    println!("\nStep 4: XMP tags found in MetadataMap:");
    for (key, value) in &xmp_tags {
        println!("  {}: {:?}", key, value);
    }

    // === Verify specific expected XMP tags ===
    println!("\nStep 5: Verifying specific XMP tag values...");

    // Stream 6 changed to use simplified XMP: prefix for common namespaces
    assert!(
        metadata.contains_key("XMP:Creator"),
        "Missing XMP:Creator tag"
    );
    let creator = metadata.get("XMP:Creator").unwrap();
    assert!(
        format!("{:?}", creator).contains("John Doe"),
        "XMP:Creator should be 'John Doe', got {:?}",
        creator
    );
    println!("  ✓ XMP:Creator: {:?}", creator);

    assert!(
        metadata.contains_key("XMP:Rating"),
        "Missing XMP:Rating tag"
    );
    let rating = metadata.get("XMP:Rating").unwrap();
    println!("  ✓ XMP:Rating: {:?}", rating);

    assert!(metadata.contains_key("XMP:Title"), "Missing XMP:Title tag");
    let title = metadata.get("XMP:Title").unwrap();
    assert!(
        format!("{:?}", title).contains("Sample Photo"),
        "XMP:Title should be 'Sample Photo', got {:?}",
        title
    );
    println!("  ✓ XMP:Title: {:?}", title);

    assert!(
        metadata.contains_key("XMP:Rights"),
        "Missing XMP:Rights tag"
    );
    let rights = metadata.get("XMP:Rights").unwrap();
    println!("  ✓ XMP:Rights: {:?}", rights);

    // === Final verification ===
    println!("\n=== Test Summary ===");
    println!("✅ SUCCESS: XMP tags are flowing correctly through read_metadata() API!");
    println!("   XMP tags found: {}", xmp_tags.len());
    println!("   EXIF tags found: {}", exif_tags.len());
    println!("   Total tags: {}", metadata.len());
    println!("\n✅ Data flow verified: JPEG → Segments → XMP Parser → MetadataMap");
}
