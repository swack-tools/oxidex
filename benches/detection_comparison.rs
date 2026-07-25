//! Benchmark comparing signature-based vs Magika AI detection performance
//!
//! Run with: `cargo bench --features magika --bench detection_comparison`

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use oxidex::core::operations::read_metadata_with_detector;
use oxidex::parsers::DetectorMode;
use std::path::Path;

/// Benchmark file format detection on a JPEG file
fn bench_jpeg_detection(c: &mut Criterion) {
    let path = Path::new("tests/fixtures/jpeg/sample_with_exif.jpg");

    if !path.exists() {
        eprintln!("Warning: Test file not found, skipping JPEG benchmark");
        return;
    }

    let mut group = c.benchmark_group("JPEG Detection");

    // Benchmark signature-based detection
    group.bench_function("Signature", |b| {
        b.iter(|| read_metadata_with_detector(black_box(path), DetectorMode::Signature));
    });

    // Benchmark Magika detection (only if feature is enabled)
    #[cfg(feature = "magika")]
    group.bench_function("Magika", |b| {
        b.iter(|| read_metadata_with_detector(black_box(path), DetectorMode::Magika));
    });

    group.finish();
}

/// Benchmark file format detection on a PNG file
fn bench_png_detection(c: &mut Criterion) {
    let path = Path::new("tests/fixtures/png/basic.png");

    if !path.exists() {
        eprintln!("Warning: Test file not found, skipping PNG benchmark");
        return;
    }

    let mut group = c.benchmark_group("PNG Detection");

    group.bench_function("Signature", |b| {
        b.iter(|| read_metadata_with_detector(black_box(path), DetectorMode::Signature));
    });

    #[cfg(feature = "magika")]
    group.bench_function("Magika", |b| {
        b.iter(|| read_metadata_with_detector(black_box(path), DetectorMode::Magika));
    });

    group.finish();
}

/// Benchmark file format detection on a TIFF file
fn bench_tiff_detection(c: &mut Criterion) {
    let path = Path::new("tests/fixtures/tiff/basic.tiff");

    if !path.exists() {
        eprintln!("Warning: Test file not found, skipping TIFF benchmark");
        return;
    }

    let mut group = c.benchmark_group("TIFF Detection");

    group.bench_function("Signature", |b| {
        b.iter(|| read_metadata_with_detector(black_box(path), DetectorMode::Signature));
    });

    #[cfg(feature = "magika")]
    group.bench_function("Magika", |b| {
        b.iter(|| read_metadata_with_detector(black_box(path), DetectorMode::Magika));
    });

    group.finish();
}

/// Benchmark detection across multiple file formats
fn bench_multi_format_detection(c: &mut Criterion) {
    let test_files = vec![
        ("JPEG", "tests/fixtures/jpeg/sample_with_exif.jpg"),
        ("PNG", "tests/fixtures/png/basic.png"),
        ("TIFF", "tests/fixtures/tiff/basic.tiff"),
    ];

    for (format_name, file_path) in test_files {
        let path = Path::new(file_path);

        if !path.exists() {
            eprintln!("Warning: {} test file not found, skipping", format_name);
            continue;
        }

        let mut group = c.benchmark_group(format!("{} Format", format_name));

        group.bench_with_input(BenchmarkId::new("Signature", format_name), &path, |b, p| {
            b.iter(|| read_metadata_with_detector(black_box(p), DetectorMode::Signature));
        });

        #[cfg(feature = "magika")]
        group.bench_with_input(BenchmarkId::new("Magika", format_name), &path, |b, p| {
            b.iter(|| read_metadata_with_detector(black_box(p), DetectorMode::Magika));
        });

        group.finish();
    }
}

criterion_group!(
    benches,
    bench_jpeg_detection,
    bench_png_detection,
    bench_tiff_detection,
    bench_multi_format_detection
);
criterion_main!(benches);
