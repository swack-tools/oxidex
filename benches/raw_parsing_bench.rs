//! Performance benchmarks for camera raw format parsing
//!
//! This benchmark suite measures the performance of raw format detection
//! and metadata parsing to establish baseline performance and detect regressions.
//!
//! ## Running Benchmarks
//!
//! ```bash
//! cargo bench raw_parsing_bench
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
use oxidex::parsers::raw::{RawFormat, detect_raw_format, parse_raw_metadata};
use std::hint::black_box;

/// Benchmark for raw format detection
///
/// This benchmark measures the performance of detecting camera raw formats
/// from magic bytes and file extensions. Format detection should be very fast
/// as it's the first step in the metadata extraction pipeline.
///
/// Performance target: < 100 nanoseconds per detection
fn bench_format_detection(c: &mut Criterion) {
    c.bench_function("detect_raw_format", |b| {
        // Test with Canon CR2 magic bytes
        let cr2_data = b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00\x00\x00\x00\x00";

        b.iter(|| {
            // Benchmark the format detection function
            black_box(detect_raw_format(
                black_box(cr2_data),
                black_box("test.cr2"),
            ))
        })
    });
}

/// Benchmark for DNG metadata parsing
///
/// This benchmark measures the performance of parsing metadata from
/// Adobe DNG (Digital Negative) files. DNG is a TIFF-based format,
/// so this benchmark tests the TIFF parsing infrastructure.
///
/// Performance target: < 5ms per file (consistent with other formats)
fn bench_dng_parsing(c: &mut Criterion) {
    // Load minimal DNG file from test fixtures
    let dng_data = std::fs::read("tests/fixtures/raw/sample.dng").unwrap_or_else(|_| {
        // Fallback: create minimal TIFF header if fixture doesn't exist
        // This ensures the benchmark can run even without the fixture
        vec![
            0x49, 0x49, // Little-endian
            0x2a, 0x00, // TIFF magic number (42)
            0x08, 0x00, 0x00, 0x00, // IFD offset
        ]
    });

    c.bench_function("parse_dng_metadata", |b| {
        b.iter(|| {
            // Benchmark the DNG metadata parser
            // Using black_box to prevent compiler optimizations from skipping the actual work
            black_box(parse_raw_metadata(
                black_box(&dng_data),
                black_box(RawFormat::AdobeDNG),
            ))
        })
    });
}

// Define benchmark group with both benchmarks
criterion_group!(benches, bench_format_detection, bench_dng_parsing);

// Main entry point for Criterion
criterion_main!(benches);
