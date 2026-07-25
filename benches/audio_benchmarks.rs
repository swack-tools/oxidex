use criterion::{Criterion, criterion_group, criterion_main};
use oxidex::core::FormatParser;
use oxidex::io::MMapReader;
use oxidex::parsers::audio::aac::AacParser;
use oxidex::parsers::audio::ape::ApeParser;
use oxidex::parsers::audio::flac::FlacParser;
use oxidex::parsers::audio::mp3::Mp3Parser;
use oxidex::parsers::audio::ogg::OggParser;
use oxidex::parsers::audio::opus::OpusParser;
use oxidex::parsers::audio::wav::WavParser;
use std::hint::black_box;
use std::path::Path;

fn bench_flac_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/audio/sample.flac");

    if !test_file.exists() {
        eprintln!("Warning: test_data/audio/sample.flac not found, skipping benchmark");
        return;
    }

    c.bench_function("flac_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = FlacParser;
            parser.parse(&reader).expect("Failed to parse FLAC");
        })
    });
}

fn bench_mp3_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/audio/sample.mp3");

    if !test_file.exists() {
        eprintln!("Warning: test_data/audio/sample.mp3 not found, skipping benchmark");
        return;
    }

    c.bench_function("mp3_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = Mp3Parser;
            parser.parse(&reader).expect("Failed to parse MP3");
        })
    });
}

fn bench_aac_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/audio/sample.aac");

    if !test_file.exists() {
        eprintln!("Warning: test_data/audio/sample.aac not found, skipping benchmark");
        return;
    }

    c.bench_function("aac_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = AacParser;
            parser.parse(&reader).expect("Failed to parse AAC");
        })
    });
}

fn bench_wav_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/audio/sample.wav");

    if !test_file.exists() {
        eprintln!("Warning: test_data/audio/sample.wav not found, skipping benchmark");
        return;
    }

    c.bench_function("wav_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = WavParser;
            parser.parse(&reader).expect("Failed to parse WAV");
        })
    });
}

fn bench_ogg_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/audio/sample.ogg");

    if !test_file.exists() {
        eprintln!("Warning: test_data/audio/sample.ogg not found, skipping benchmark");
        return;
    }

    c.bench_function("ogg_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = OggParser;
            parser.parse(&reader).expect("Failed to parse OGG");
        })
    });
}

fn bench_opus_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/audio/sample.opus");

    if !test_file.exists() {
        eprintln!("Warning: test_data/audio/sample.opus not found, skipping benchmark");
        return;
    }

    c.bench_function("opus_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = OpusParser;
            parser.parse(&reader).expect("Failed to parse Opus");
        })
    });
}

fn bench_ape_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/audio/sample.ape");

    if !test_file.exists() {
        eprintln!("Warning: test_data/audio/sample.ape not found, skipping benchmark");
        return;
    }

    c.bench_function("ape_parse", |b| {
        b.iter(|| {
            let reader = MMapReader::new(black_box(test_file)).expect("Failed to create reader");
            let parser = ApeParser;
            parser.parse(&reader).expect("Failed to parse APE");
        })
    });
}

criterion_group!(
    benches,
    bench_flac_parsing,
    bench_mp3_parsing,
    bench_aac_parsing,
    bench_wav_parsing,
    bench_ogg_parsing,
    bench_opus_parsing,
    bench_ape_parsing
);
criterion_main!(benches);
