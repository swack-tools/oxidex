//! Integration Performance Benchmarks
//!
//! This benchmark suite measures end-to-end performance as specified in the
//! integration test plan (Section 6.4).
//!
//! ## Benchmark Categories
//!
//! 1. Single file extraction (< 10ms target)
//! 2. Batch processing (< 5 seconds for 1000 JPEGs target)
//! 3. Large file handling (< 500ms for 50MB TIFF target)
//! 4. Cold start time (< 50ms for CLI launch + extraction)
//!
//! ## Running Benchmarks
//!
//! ```bash
//! cargo bench --bench integration_benchmarks
//! ```

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxidex::core::operations::read_metadata;
use std::hint::black_box;
use std::path::Path;

/// Benchmark single file extraction across different formats
///
/// Target: < 10ms per file
/// Tests: JPEG, PNG, TIFF extraction performance
fn bench_single_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_extraction");

    let test_files = [
        (
            "jpeg_simple",
            "tests/fixtures/jpeg/simple/sample_with_exif.jpg",
        ),
        (
            "jpeg_complex",
            "tests/fixtures/jpeg/complex/sample_with_exif_xmp.jpg",
        ),
        (
            "png_simple",
            "tests/fixtures/png/simple/synthetic_text_001.png",
        ),
        (
            "png_complex",
            "tests/fixtures/png/complex/synthetic_exif_001.png",
        ),
        ("tiff_simple", "tests/fixtures/tiff/simple/sample.tif"),
    ];

    for (name, path) in test_files.iter() {
        if Path::new(path).exists() {
            group.bench_with_input(BenchmarkId::from_parameter(name), path, |b, path| {
                b.iter(|| {
                    black_box(read_metadata(Path::new(path)).expect("Metadata extraction failed"))
                });
            });
        } else {
            eprintln!("Warning: Benchmark fixture not found: {}", path);
        }
    }

    group.finish();
}

/// Benchmark batch processing of multiple files
///
/// Target: < 5 seconds for 1000 files (5ms average per file)
/// Measures throughput for bulk metadata extraction
fn bench_batch_processing(c: &mut Criterion) {
    use std::fs;

    c.bench_function("batch_100_jpegs", |b| {
        // Find JPEG files in simple directory
        let mut jpeg_files = Vec::new();

        if let Ok(entries) = fs::read_dir("tests/fixtures/jpeg/simple") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("jpg") {
                    jpeg_files.push(path);
                    if jpeg_files.len() >= 100 {
                        break;
                    }
                }
            }
        }

        if jpeg_files.is_empty() {
            eprintln!("Warning: No JPEG files found for batch benchmark");
            return;
        }

        println!("Batch benchmark using {} JPEG files", jpeg_files.len());

        b.iter(|| {
            for file_path in &jpeg_files {
                black_box(read_metadata(file_path).ok());
            }
        });
    });
}

/// Benchmark large file handling
///
/// Target: < 500ms for 50MB TIFF
/// Tests performance with large files (multi-page TIFF, large dimensions)
fn bench_large_file_handling(c: &mut Criterion) {
    let large_files = [
        (
            "tiff_multipage",
            "tests/fixtures/tiff/complex/multipage.tif",
        ),
        (
            "jpeg_large_dimension",
            "tests/fixtures/jpeg/edge_cases/large_dimension.jpg",
        ),
    ];

    let mut group = c.benchmark_group("large_file_handling");

    for (name, path) in large_files.iter() {
        if Path::new(path).exists() {
            group.bench_with_input(BenchmarkId::from_parameter(name), path, |b, path| {
                b.iter(|| {
                    black_box(read_metadata(Path::new(path)).expect("Metadata extraction failed"))
                });
            });
        } else {
            eprintln!("Warning: Large file benchmark fixture not found: {}", path);
        }
    }

    group.finish();
}

/// Benchmark format-specific operations
///
/// Compares performance across different file formats to identify
/// format-specific bottlenecks
fn bench_format_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("format_comparison");

    let formats = [
        ("jpeg", "tests/fixtures/jpeg/simple/sample_with_exif.jpg"),
        ("png", "tests/fixtures/png/simple/synthetic_text_001.png"),
        ("tiff", "tests/fixtures/tiff/simple/sample.tif"),
        ("pdf", "tests/fixtures/pdf/simple/sample.pdf"),
        ("mp4", "tests/fixtures/mp4/simple/sample.mp4"),
    ];

    for (format, path) in formats.iter() {
        if Path::new(path).exists() {
            group.bench_with_input(BenchmarkId::from_parameter(format), path, |b, path| {
                b.iter(|| {
                    black_box(read_metadata(Path::new(path)).expect("Metadata extraction failed"))
                });
            });
        } else {
            eprintln!(
                "Warning: Format benchmark fixture not found for {}: {}",
                format, path
            );
        }
    }

    group.finish();
}

/// Benchmark GPS coordinate extraction
///
/// Target: Floating-point coordinate conversion should be < 1ms
/// Tests performance of GPS tag parsing and conversion
fn bench_gps_extraction(c: &mut Criterion) {
    let gps_files = [
        "tests/fixtures/jpeg/complex/synthetic_gps_001.jpg",
        "tests/fixtures/jpeg/complex/synthetic_gps_002.jpg",
        "tests/fixtures/jpeg/complex/synthetic_gps_003.jpg",
    ];

    c.bench_function("gps_coordinate_extraction", |b| {
        let valid_files: Vec<_> = gps_files.iter().filter(|p| Path::new(p).exists()).collect();

        if valid_files.is_empty() {
            eprintln!("Warning: No GPS test files found");
            return;
        }

        b.iter(|| {
            for path in &valid_files {
                black_box(read_metadata(Path::new(path)).ok());
            }
        });
    });
}

criterion_group!(
    benches,
    bench_single_extraction,
    bench_batch_processing,
    bench_large_file_handling,
    bench_format_comparison,
    bench_gps_extraction
);

criterion_main!(benches);
