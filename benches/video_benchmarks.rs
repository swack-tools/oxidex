use criterion::{Criterion, criterion_group, criterion_main};
use oxidex::core::FormatParser;
use oxidex::io::MMapReader;
use oxidex::parsers::video::avi::AviParser;
use oxidex::parsers::video::flv::FlvParser;
use oxidex::parsers::video::mkv::MkvParser;
use oxidex::parsers::video::mts::MtsParser;
use std::hint::black_box;
use std::path::Path;

fn bench_mkv_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/video/sample.mkv");

    if !test_file.exists() {
        eprintln!("Warning: test_data/video/sample.mkv not found, skipping benchmark");
        return;
    }

    c.bench_function("mkv_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = MkvParser;
            parser.parse(&reader).expect("Failed to parse MKV");
        })
    });
}

fn bench_webm_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/video/sample.webm");

    if !test_file.exists() {
        eprintln!("Warning: test_data/video/sample.webm not found, skipping benchmark");
        return;
    }

    c.bench_function("webm_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = MkvParser; // WebM uses MKV parser
            parser.parse(&reader).expect("Failed to parse WebM");
        })
    });
}

fn bench_flv_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/video/sample.flv");

    if !test_file.exists() {
        eprintln!("Warning: test_data/video/sample.flv not found, skipping benchmark");
        return;
    }

    c.bench_function("flv_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = FlvParser;
            parser.parse(&reader).expect("Failed to parse FLV");
        })
    });
}

fn bench_avi_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/video/sample.avi");

    if !test_file.exists() {
        eprintln!("Warning: test_data/video/sample.avi not found, skipping benchmark");
        return;
    }

    c.bench_function("avi_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = AviParser;
            parser.parse(&reader).expect("Failed to parse AVI");
        })
    });
}

fn bench_mts_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/video/sample.mts");

    if !test_file.exists() {
        eprintln!("Warning: test_data/video/sample.mts not found, skipping benchmark");
        return;
    }

    c.bench_function("mts_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = MtsParser;
            parser.parse(&reader).expect("Failed to parse MTS");
        })
    });
}

criterion_group!(
    benches,
    bench_mkv_parsing,
    bench_webm_parsing,
    bench_flv_parsing,
    bench_avi_parsing,
    bench_mts_parsing
);
criterion_main!(benches);
