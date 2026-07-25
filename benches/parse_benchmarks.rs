//! Performance benchmarks for metadata parsing operations
//!
//! This benchmark suite measures the performance of core parsing operations
//! to establish baseline performance and detect regressions over time.
//!
//! ## Running Benchmarks
//!
//! ```bash
//! cargo bench
//! ```
//!
//! ## Viewing HTML Reports
//!
//! After running benchmarks, Criterion generates detailed HTML reports:
//!
//! ```bash
//! # macOS
//! open target/criterion/report/index.html
//!
//! # Linux
//! xdg-open target/criterion/report/index.html
//!
//! # Windows
//! start target/criterion/report/index.html
//! ```

use criterion::{Criterion, criterion_group, criterion_main};
use oxidex::core::operations::read_metadata;
use oxidex::io::MMapReader;
use oxidex::parsers::detection::detect_format;
use oxidex::parsers::jpeg::segment_parser::parse_segments;
use oxidex::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use std::hint::black_box;
use std::path::Path;

// Import IPTC parser for tag name generation benchmark
use oxidex::parsers::jpeg::iptc_parser;

/// Benchmark for format detection via magic bytes
///
/// This benchmark measures the performance of detecting file formats by
/// reading and analyzing magic bytes. Format detection is the first step
/// in the metadata extraction pipeline and should be very fast (<1ms).
fn bench_format_detection(c: &mut Criterion) {
    c.bench_function("format_detection", |b| {
        // Setup: Create a file reader for the test JPEG
        let reader = MMapReader::new(Path::new("tests/fixtures/jpeg/sample_with_exif.jpg"))
            .expect("Failed to open test fixture");

        b.iter(|| {
            // Benchmark the format detection function
            black_box(detect_format(&reader).expect("Format detection failed"))
        });
    });
}

/// Benchmark for JPEG segment parsing
///
/// This benchmark measures the performance of parsing JPEG segment structure
/// using nom combinators. JPEG parsing involves identifying markers and
/// extracting segment data, which is critical for EXIF extraction.
fn bench_jpeg_segment_parsing(c: &mut Criterion) {
    c.bench_function("jpeg_segment_parsing", |b| {
        // Setup: Create a file reader for the test JPEG
        let reader = MMapReader::new(Path::new("tests/fixtures/jpeg/sample_with_exif.jpg"))
            .expect("Failed to open test fixture");

        b.iter(|| {
            // Benchmark the JPEG segment parser
            black_box(parse_segments(&reader).expect("JPEG segment parsing failed"))
        });
    });
}

/// Benchmark for TIFF IFD parsing
///
/// This benchmark measures the performance of parsing TIFF Image File Directory
/// (IFD) structures. IFD parsing is more complex than format detection as it
/// involves reading tag entries, following offsets, and handling byte order.
fn bench_tiff_ifd_parsing(c: &mut Criterion) {
    c.bench_function("tiff_ifd_parsing", |b| {
        // Setup: Create a file reader for the test JPEG with EXIF data
        // We'll parse the TIFF IFD structure embedded in the JPEG's EXIF segment
        let reader = MMapReader::new(Path::new("tests/fixtures/jpeg/sample_with_exif.jpg"))
            .expect("Failed to open test fixture");

        // Extract TIFF data from JPEG's EXIF segment for benchmarking
        // First, parse segments to find the APP1 EXIF segment
        let segments = parse_segments(&reader).expect("Failed to parse JPEG segments");
        let app1_segment = segments
            .iter()
            .find(|s| s.is_app1() && s.data.len() >= 6 && &s.data[0..6] == b"Exif\0\0")
            .expect("No EXIF segment found in test fixture");

        // Extract TIFF data (after "Exif\0\0" header)
        let tiff_data = &app1_segment.data[6..];

        // Detect byte order
        let byte_order = if &tiff_data[0..2] == b"II" {
            ByteOrder::LittleEndian
        } else {
            ByteOrder::BigEndian
        };

        // Read IFD offset
        let ifd_offset = match byte_order {
            ByteOrder::LittleEndian => {
                u32::from_le_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]])
            }
            ByteOrder::BigEndian => {
                u32::from_be_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]])
            }
        } as u64;

        // Create a sub-reader for TIFF data (offset adjusted to TIFF data start)
        let tiff_offset = app1_segment.offset + 10; // marker(2) + length(2) + "Exif\0\0"(6)

        b.iter(|| {
            // Create TiffSubReader for each iteration (lightweight wrapper)
            let tiff_reader = TiffSubReader::new(&reader, tiff_offset);

            // Benchmark the IFD parser
            black_box(
                parse_ifd(&tiff_reader, ifd_offset, byte_order).expect("TIFF IFD parsing failed"),
            )
        });
    });
}

/// Benchmark for full metadata read operation
///
/// This benchmark measures the end-to-end performance of extracting metadata
/// from a JPEG file, including format detection, segment parsing, IFD parsing,
/// and metadata map construction. This is the most realistic benchmark for
/// measuring overall performance.
///
/// Performance target: < 5ms per file (average)
fn bench_full_read_metadata(c: &mut Criterion) {
    c.bench_function("full_read_metadata", |b| {
        // Setup: Path to test file
        let test_path = Path::new("tests/fixtures/jpeg/sample_with_exif.jpg");

        b.iter(|| {
            // Benchmark the complete metadata reading pipeline
            black_box(read_metadata(test_path).expect("read_metadata failed"))
        });
    });
}

/// Helper struct: FileReader wrapper that adjusts offsets to be relative to a base offset
///
/// This is used to create a "view" into the file where offset 0 corresponds
/// to a specific position in the original file. Needed for parsing TIFF data
/// embedded within JPEG segments.
struct TiffSubReader<'a> {
    reader: &'a MMapReader,
    base_offset: u64,
}

impl<'a> TiffSubReader<'a> {
    fn new(reader: &'a MMapReader, base_offset: u64) -> Self {
        Self {
            reader,
            base_offset,
        }
    }
}

impl<'a> oxidex::core::FileReader for TiffSubReader<'a> {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        // Adjust offset to be relative to base
        self.reader.read(self.base_offset + offset, length)
    }

    fn size(&self) -> u64 {
        // Return size relative to base (remaining size from base to end)
        let total_size = self.reader.size();
        total_size.saturating_sub(self.base_offset)
    }
}

/// Benchmark for IPTC tag name generation
///
/// This benchmark measures the performance of converting IPTC dataset numbers
/// to tag names. The current implementation uses format!() which allocates
/// on every call. This benchmark helps measure the impact of optimizing
/// to static strings for known datasets.
fn bench_iptc_tag_name_generation(c: &mut Criterion) {
    c.bench_function("iptc_tag_name_generation", |b| {
        b.iter(|| {
            // Benchmark common tag lookups
            // Using black_box to prevent compiler optimizations
            black_box(iptc_parser::dataset_to_tag_name(2, 5)); // ObjectName
            black_box(iptc_parser::dataset_to_tag_name(2, 25)); // Keywords
            black_box(iptc_parser::dataset_to_tag_name(2, 80)); // By-line
            black_box(iptc_parser::dataset_to_tag_name(2, 120)); // Caption-Abstract
        });
    });
}

// Define benchmark group with all five benchmarks
criterion_group!(
    benches,
    bench_format_detection,
    bench_jpeg_segment_parsing,
    bench_tiff_ifd_parsing,
    bench_full_read_metadata,
    bench_iptc_tag_name_generation
);

// Main entry point for Criterion
criterion_main!(benches);
