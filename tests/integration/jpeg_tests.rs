//! Integration tests for end-to-end JPEG EXIF extraction
//!
//! This test validates the entire parsing pipeline from file reading through
//! format detection, segment parsing, and EXIF tag extraction.

use exiftool_rs::core::{FileFormat, FileReader};
use exiftool_rs::io::MMapReader;
use exiftool_rs::parsers::format_detector::detect_format;
use exiftool_rs::parsers::jpeg::segment_parser::parse_segments;
use exiftool_rs::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
use std::io::Write;
use std::path::Path;

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
    let mut file = std::fs::File::create(&exif_fixture_path)?;
    file.write_all(&jpeg_data)?;

    // Create EXIF+XMP fixture
    let exif_xmp_fixture_path = fixture_dir.join("sample_with_exif_xmp.jpg");
    let jpeg_data_xmp = create_jpeg_with_exif_and_xmp();
    let mut file = std::fs::File::create(&exif_xmp_fixture_path)?;
    file.write_all(&jpeg_data_xmp)?;

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
    use exiftool_rs::parsers::jpeg::xmp_parser::extract_xmp_from_segments;

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
        .filter(|(name, _)| name == "XMP:title")
        .collect();
    assert_eq!(title_tags.len(), 1, "Should have exactly one XMP:title tag");
    assert_eq!(
        title_tags[0].1, "Sample Photo",
        "XMP:title should be 'Sample Photo'"
    );
    println!("  ✓ XMP:title: {}", title_tags[0].1);

    // Check for rights (dc:rights)
    let rights_tags: Vec<_> = xmp_tags
        .iter()
        .filter(|(name, _)| name == "XMP:rights")
        .collect();
    assert_eq!(
        rights_tags.len(),
        1,
        "Should have exactly one XMP:rights tag"
    );
    assert_eq!(
        rights_tags[0].1, "Copyright 2024",
        "XMP:rights should be 'Copyright 2024'"
    );
    println!("  ✓ XMP:rights: {}", rights_tags[0].1);

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
