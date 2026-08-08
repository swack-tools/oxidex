//! Error Handling Integration Tests
//!
//! These tests validate graceful degradation for invalid inputs as specified
//! in the integration test plan (Section 6.3).
//!
//! ## Test Coverage
//!
//! - Missing files (IoError::NotFound)
//! - Unsupported formats (UnsupportedFormat)
//! - Truncated files (ParseError::UnexpectedEof)
//! - Corrupted IFD structures (ParseError::InvalidTagCount)
//! - Integer overflow protection (ParseError::IntegerOverflow)
//! - No panics (all errors return Result)
//! - No infinite loops (timeout protection)
//!
//! ## Success Criteria
//!
//! Per integration test plan section 4.1.3:
//! - Returns `Err(ExifToolError::ParseError(..))` for corrupted structure
//! - Returns `Err(ExifToolError::UnsupportedFormat)` for invalid magic bytes
//! - Completes within 5 seconds (no infinite loops)
//! - No memory leaks (Rust ownership system)
//! - No panics (all errors are `Result<T, E>`)

use oxidex::core::FileFormat;
use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::detection::detect_format;
use oxidex::parsers::jpeg::segment_parser::parse_segments;
use oxidex::parsers::tiff::file_parser::parse_tiff_file;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// Helper to ensure operation completes within timeout
fn with_timeout<F, T>(timeout: Duration, f: F) -> Result<T, String>
where
    F: FnOnce() -> T,
{
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();

    if elapsed > timeout {
        Err(format!(
            "Operation took {:?}, exceeding timeout of {:?}",
            elapsed, timeout
        ))
    } else {
        Ok(result)
    }
}

// ============================================================================
// Missing File Tests
// ============================================================================

#[test]
fn test_error_missing_file() {
    let nonexistent_path = Path::new("tests/fixtures/nonexistent.jpg");

    let result = BufferedReader::new(nonexistent_path);

    assert!(result.is_err(), "Expected error for missing file");

    if let Err(e) = result {
        assert_eq!(
            e.kind(),
            io::ErrorKind::NotFound,
            "Expected NotFound error, got: {:?}",
            e.kind()
        );
    }
}

// ============================================================================
// Unsupported Format Tests
// ============================================================================

#[test]
fn test_error_unsupported_format() {
    // Create a temporary file with invalid magic bytes
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    // Binary bytes matching no known signature. ExifTool 13.59 reports
    // "Error: Unknown file type" for exactly these bytes, so Unknown is the
    // agreed-upon answer rather than an oxidex-only convention.
    let unrecognized: &[u8] = &[
        0x00, 0xde, 0xad, 0xbe, 0xef, 0x13, 0x37, 0x00, 0x01, 0x02, 0xfe, 0xdc, 0xba, 0x98, 0x76,
        0x54, 0x00, 0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33,
        0x22, 0x11,
    ];
    fs::write(temp_path, unrecognized).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");

    // `detect_format` reports an unrecognized signature as `Ok(FileFormat::Unknown)`
    // rather than an error, so assert on the variant: treating a plain `Ok` as
    // success here would pass for every format.
    let format = detect_format(&reader).expect("detection must not error on readable data");

    assert_eq!(
        format,
        FileFormat::Unknown,
        "Expected FileFormat::Unknown for unrecognized binary signature, got: {:?}",
        format
    );
}

#[test]
fn test_printable_ascii_is_txt_not_unknown() {
    // This guards the fixture used by `test_error_unsupported_format`. That test
    // originally fed in the printable string "INVALID_FORMAT_MAGIC_BYTES_HERE"
    // and asserted "not a known format" -- but printable ASCII *is* a known
    // format. ExifTool 13.59 reports `FileType: TXT, MIMEType: text/plain` for
    // that exact byte string, so a future change that makes plain text detect as
    // Unknown would be a regression against ExifTool, not a fix.
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    fs::write(temp_path, b"INVALID_FORMAT_MAGIC_BYTES_HERE").expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");
    let format = detect_format(&reader).expect("detection must not error on readable data");

    assert_eq!(
        format,
        FileFormat::TXT,
        "Printable ASCII must detect as TXT (ExifTool 13.59 agrees), got: {:?}",
        format
    );
}

// ============================================================================
// Truncated File Tests
// ============================================================================

#[test]
fn test_error_truncated_jpeg() {
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    // Write JPEG SOI marker and start of APP1 segment, then truncate
    let truncated_jpeg = vec![
        0xFF, 0xD8, // SOI marker
        0xFF, 0xE1, // APP1 marker
        0x00, 0x10, // Length: 16 bytes (but we won't provide all of them)
        b'E', b'x', b'i', b'f', 0x00,
        0x00, // EXIF identifier
              // Truncated: missing TIFF header and IFD data
    ];

    fs::write(temp_path, truncated_jpeg).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");

    // Try to parse JPEG segments - should handle truncation gracefully
    let result = parse_segments(&reader);

    // The parser should either:
    // 1. Return an error (preferred)
    // 2. Return partial results without panicking (acceptable)
    // We're testing that it doesn't panic
    match result {
        Ok(segments) => {
            println!(
                "Parser handled truncated file gracefully, extracted {} segments",
                segments.len()
            );
        }
        Err(e) => {
            println!("Parser correctly returned error for truncated file: {}", e);
        }
    }
}

#[test]
fn test_error_truncated_tiff() {
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    // Write TIFF header, then truncate before IFD
    let truncated_tiff = vec![
        b'I', b'I', // Little-endian byte order
        0x2A, 0x00, // Magic number
        0x08, 0x00, 0x00, 0x00, // IFD offset: 8
              // Truncated: missing IFD data
    ];

    fs::write(temp_path, truncated_tiff).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");

    // Try to parse TIFF file - should handle truncation gracefully
    let result = parse_tiff_file(&reader);

    // Should return an error, not panic
    assert!(
        result.is_err(),
        "Expected error for truncated TIFF file, got: {:?}",
        result
    );

    if let Err(e) = result {
        println!("Parser correctly returned error for truncated TIFF: {}", e);
        // Error should name the truncation, not some unrelated failure.
        // "IFD offset N exceeds file size N" is the parser's current phrasing
        // and is a precise truncation diagnosis, so it counts.
        let error_msg = e.to_string().to_lowercase();
        assert!(
            error_msg.contains("eof")
                || error_msg.contains("truncate")
                || error_msg.contains("unexpected")
                || error_msg.contains("invalid")
                || error_msg.contains("exceeds file size"),
            "Error message should indicate truncation/EOF, got: {}",
            e
        );
    }
}

// ============================================================================
// Corrupted IFD Tests
// ============================================================================

#[test]
fn test_error_corrupted_ifd_invalid_tag_count() {
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    // Create TIFF with invalid tag count (0xFFFF = 65535 tags)
    let mut corrupted_tiff = vec![
        b'I', b'I', // Little-endian
        0x2A, 0x00, // Magic number
        0x08, 0x00, 0x00, 0x00, // IFD offset: 8
    ];

    // IFD with invalid tag count
    corrupted_tiff.extend_from_slice(&[
        0xFF, 0xFF, // Tag count: 65535 (invalid - would require huge file)
    ]);

    // Add some dummy data to avoid immediate EOF
    corrupted_tiff.extend_from_slice(&[0; 1000]);

    fs::write(temp_path, corrupted_tiff).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");

    // Try to parse - should reject invalid tag count
    let result = with_timeout(Duration::from_secs(5), || parse_tiff_file(&reader));

    match result {
        Ok(Ok(_)) => {
            panic!("Parser should reject IFD with 65535 tags");
        }
        Ok(Err(e)) => {
            println!("Parser correctly rejected corrupted IFD: {}", e);
        }
        Err(timeout_msg) => {
            panic!("Parser timed out on corrupted IFD: {}", timeout_msg);
        }
    }
}

#[test]
fn test_error_corrupted_ifd_circular_reference() {
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    // Create TIFF with circular IFD chain: IFD0 points back to itself
    let mut circular_tiff = vec![
        b'I', b'I', // Little-endian
        0x2A, 0x00, // Magic number
        0x08, 0x00, 0x00, 0x00, // IFD offset: 8 (points to IFD0)
    ];

    // IFD0 at offset 8
    circular_tiff.extend_from_slice(&[
        0x01, 0x00, // Tag count: 1
    ]);

    // Single tag (ImageWidth)
    circular_tiff.extend_from_slice(&[
        0x00, 0x01, // Tag: ImageWidth (0x0100)
        0x03, 0x00, // Type: SHORT (3)
        0x01, 0x00, 0x00, 0x00, // Count: 1
        0x40, 0x00, 0x00, 0x00, // Value: 64
    ]);

    // Next IFD offset: points back to offset 8 (circular reference)
    circular_tiff.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]);

    fs::write(temp_path, circular_tiff).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");

    // Try to parse with timeout (should not hang)
    let result = with_timeout(Duration::from_secs(5), || parse_tiff_file(&reader));

    match result {
        Ok(Ok(tags)) => {
            println!(
                "Parser handled circular reference gracefully, extracted {} tags",
                tags.len()
            );
            // Should have extracted at least the first IFD's tags
            assert!(
                tags.len() >= 1,
                "Should extract at least one tag before detecting cycle"
            );
        }
        Ok(Err(e)) => {
            println!("Parser correctly detected circular reference: {}", e);
        }
        Err(timeout_msg) => {
            panic!(
                "Parser should detect circular references and not hang: {}",
                timeout_msg
            );
        }
    }
}

// ============================================================================
// Integer Overflow Tests
// ============================================================================

#[test]
fn test_error_integer_overflow_protection() {
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    // Create TIFF with tag value count that could cause overflow
    let mut overflow_tiff = vec![
        b'I', b'I', // Little-endian
        0x2A, 0x00, // Magic number
        0x08, 0x00, 0x00, 0x00, // IFD offset: 8
    ];

    // IFD with tag that has extreme value count
    overflow_tiff.extend_from_slice(&[
        0x01, 0x00, // Tag count: 1
    ]);

    // Tag with extreme count (0xFFFFFFFF = 4294967295)
    overflow_tiff.extend_from_slice(&[
        0x00, 0x01, // Tag: ImageWidth
        0x04, 0x00, // Type: LONG (4 bytes per value)
        0xFF, 0xFF, 0xFF, 0xFF, // Count: 0xFFFFFFFF (would require ~16GB)
        0x00, 0x00, 0x00, 0x00, // Value offset
    ]);

    // Next IFD offset: 0 (end of chain)
    overflow_tiff.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    fs::write(temp_path, overflow_tiff).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");

    // Parse with a timeout: the point of this test is that the ~16GB the entry
    // asks for is never allocated and the parse returns promptly.
    let result = with_timeout(Duration::from_secs(5), || parse_tiff_file(&reader));

    match result {
        // ExifTool does not reject the *file* over one oversized entry. On this
        // exact 26-byte input, exiftool 13.59 reports FileType/ExifByteOrder and
        // emits only `Warning: Invalid size (17179869180) for IFD0 tag 0x0100
        // ImageWidth` -- Exif.pm:6505 warns and does `next`, skipping the entry
        // and continuing the directory. So a successful parse is correct; what
        // must never happen is the bogus tag showing up with a made-up value.
        Ok(Ok(entries)) => {
            assert!(
                !entries.iter().any(|(tag_id, ..)| *tag_id == 0x0100),
                "the oversized ImageWidth entry must be skipped, not given a value"
            );
        }
        Ok(Err(e)) => {
            println!("Parser rejected extreme value count: {}", e);
        }
        Err(timeout_msg) => {
            panic!(
                "Parser should quickly skip extreme counts, not timeout: {}",
                timeout_msg
            );
        }
    }
}

// ============================================================================
// No Panic Tests
// ============================================================================

#[test]
fn test_no_panic_on_random_data() {
    use std::fs;
    use tempfile::NamedTempFile;

    // Test that parser doesn't panic on random data
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    // Generate random-looking data (deterministic for reproducibility)
    let random_data: Vec<u8> = (0..1000).map(|i| ((i * 37 + 91) % 256) as u8).collect();

    fs::write(temp_path, random_data).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");

    // Try various parsers - none should panic
    let _ = detect_format(&reader);

    // If detected as a real format, try parsing (should not panic).
    if detect_format(&reader).is_ok_and(|f| f != FileFormat::Unknown) {
        let _ = parse_tiff_file(&reader);
        let _ = parse_segments(&reader);
    }

    println!("✓ Parser handled random data without panicking");
}

// ============================================================================
// Malformed Test Fixtures
// ============================================================================

#[test]
fn test_malformed_fixtures_directory() {
    use std::fs;

    // Test that we can handle all files in malformed directory without panicking
    let malformed_dirs = [
        "tests/fixtures/jpeg/malformed",
        "tests/fixtures/png/malformed",
        "tests/fixtures/tiff/malformed",
    ];

    for malformed_dir in &malformed_dirs {
        let path = Path::new(malformed_dir);
        if !path.exists() {
            println!("Skipping non-existent directory: {}", malformed_dir);
            continue;
        }

        let entries = fs::read_dir(path);
        if entries.is_err() {
            println!("Cannot read directory: {}", malformed_dir);
            continue;
        }

        for entry in entries.unwrap() {
            if let Ok(entry) = entry {
                let file_path = entry.path();
                if file_path.is_file() {
                    println!("Testing malformed file: {:?}", file_path);

                    // Try to parse with timeout - should not panic or hang
                    if let Ok(reader) = BufferedReader::new(&file_path) {
                        let result = with_timeout(Duration::from_secs(5), || {
                            let _ = detect_format(&reader);
                            let _ = parse_tiff_file(&reader);
                            let _ = parse_segments(&reader);
                        });

                        match result {
                            Ok(_) => println!("  ✓ Handled gracefully"),
                            Err(e) => panic!("  ✗ Timed out: {}", e),
                        }
                    }
                }
            }
        }
    }
}
