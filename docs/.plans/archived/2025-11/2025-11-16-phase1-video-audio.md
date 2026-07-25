# Phase 1: Video/Audio Formats Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add comprehensive metadata extraction for 12 video/audio formats (MKV, WEBM, FLV, AVI, MTS, MP3, FLAC, AAC, WAV, OGG, OPUS, APE)

**Architecture:** Hexagonal architecture with format-specific parsers implementing `FormatParser` trait. Use `nom` for zero-copy parsing, reference ExifTool Perl source for tag mappings.

**Tech Stack:** Rust, nom (parsing), encoding_rs (text encoding), thiserror (errors), criterion (benchmarks), libfuzzer-sys (fuzzing)

**Timeline:** 3-4 months

**Reference Design:** `docs/plans/2025-11-16-comprehensive-format-support-design.md`

---

## Prerequisites

### Task 0: Setup Foundation

**Files:**
- Create: `src/parsers/video/mod.rs`
- Create: `src/parsers/audio/mod.rs`
- Modify: `src/parsers/mod.rs`
- Modify: `src/core/file_format.rs`

**Step 1: Create video parser directory**

```bash
mkdir -p src/parsers/video
```

**Step 2: Create audio parser directory**

```bash
mkdir -p src/parsers/audio
```

**Step 3: Create video mod.rs**

Create `src/parsers/video/mod.rs`:

```rust
//! Video format parsers
//!
//! This module contains parsers for various video container formats.

#![allow(dead_code)]

pub mod mkv;
pub mod flv;
pub mod avi;
pub mod mts;

pub use mkv::MkvParser;
pub use flv::FlvParser;
pub use avi::AviParser;
pub use mts::MtsParser;
```

**Step 4: Create audio mod.rs**

Create `src/parsers/audio/mod.rs`:

```rust
//! Audio format parsers
//!
//! This module contains parsers for various audio formats.

#![allow(dead_code)]

pub mod flac;
pub mod mp3;
pub mod aac;
pub mod wav;
pub mod ogg;
pub mod opus;
pub mod ape;

pub use flac::FlacParser;
pub use mp3::Mp3Parser;
pub use aac::AacParser;
pub use wav::WavParser;
pub use ogg::OggParser;
pub use opus::OpusParser;
pub use ape::ApeParser;
```

**Step 5: Update src/parsers/mod.rs**

Add to `src/parsers/mod.rs`:

```rust
pub mod video;
pub mod audio;
```

**Step 6: Add FileFormat enum variants**

In `src/core/file_format.rs`, add after the existing variants (around line 85):

```rust
    // Phase 1: Video/Audio formats
    /// MKV (Matroska) video format (.mkv)
    MKV,

    /// WebM video format (.webm)
    WEBM,

    /// FLV (Flash Video) format (.flv)
    FLV,

    /// AVI (Audio Video Interleave) format (.avi)
    AVI,

    /// MTS (MPEG Transport Stream) format (.mts, .m2ts)
    MTS,

    /// MP3 audio format (.mp3)
    MP3,

    /// FLAC audio format (.flac)
    FLAC,

    /// AAC audio format (.aac, .m4a)
    AAC,

    /// WAV audio format (.wav)
    WAV,

    /// OGG Vorbis audio format (.ogg)
    OGG,

    /// Opus audio format (.opus)
    OPUS,

    /// APE (Monkey's Audio) format (.ape)
    APE,
```

**Step 7: Update FileFormat::name() method**

In the `impl FileFormat` block, add cases for new formats:

```rust
FileFormat::MKV => "MKV",
FileFormat::WEBM => "WebM",
FileFormat::FLV => "FLV",
FileFormat::AVI => "AVI",
FileFormat::MTS => "MTS",
FileFormat::MP3 => "MP3",
FileFormat::FLAC => "FLAC",
FileFormat::AAC => "AAC",
FileFormat::WAV => "WAV",
FileFormat::OGG => "OGG",
FileFormat::OPUS => "Opus",
FileFormat::APE => "APE",
```

**Step 8: Commit foundation**

```bash
git add src/parsers/video src/parsers/audio src/parsers/mod.rs src/core/file_format.rs
git commit -m "feat: add Phase 1 video/audio format infrastructure

- Create video and audio parser modules
- Add 12 new FileFormat enum variants (MKV, WEBM, FLV, AVI, MTS, MP3, FLAC, AAC, WAV, OGG, OPUS, APE)
- Set up module structure for Phase 1 parsers"
```

---

## Parser 1: FLAC (Free Lossless Audio Codec)

**Reference:** ExifTool `lib/Image/ExifTool/FLAC.pm`
**Magic Bytes:** `66 4C 61 43` ("fLaC")
**Spec:** https://xiph.org/flac/format.html

### Task 1.1: FLAC Parser Foundation

**Files:**
- Create: `src/parsers/audio/flac.rs`
- Create: `test_data/audio/.gitkeep`
- Create: `tests/unit/audio/flac_tests.rs`

**Step 1: Write the failing test**

Create `tests/unit/audio/flac_tests.rs`:

```rust
use oxidex::parsers::audio::flac::FlacParser;
use oxidex::core::FormatParser;
use oxidex::io::BufferedReader;

#[test]
fn test_flac_magic_bytes() {
    let data = b"fLaC\x00\x00\x00\x22..."; // Mock FLAC file
    let reader = BufferedReader::from_bytes(data);
    let parser = FlacParser;
    let result = parser.parse(&reader);

    // Should succeed with valid magic bytes
    assert!(result.is_ok());
}

#[test]
fn test_flac_invalid_magic() {
    let data = b"INVALID";
    let reader = BufferedReader::from_bytes(data);
    let parser = FlacParser;
    let result = parser.parse(&reader);

    // Should fail with invalid magic bytes
    assert!(result.is_err());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test flac_tests`

Expected: FAIL - "module `flac` not found"

**Step 3: Create minimal FLAC parser**

Create `src/parsers/audio/flac.rs`:

```rust
//! FLAC (Free Lossless Audio Codec) format parser
//!
//! Implements metadata extraction from FLAC audio files following the
//! FLAC specification.
//!
//! # Supported Metadata
//!
//! - **Vorbis Comments:** ARTIST, ALBUM, TITLE, GENRE, TRACKNUMBER, DATE
//! - **Stream Info:** SampleRate, BitsPerSample, Channels, TotalSamples
//! - **Picture:** Embedded album artwork
//! - **Application:** ReplayGain, other application-specific data
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `FLAC.pm` module:
//! - `FLAC:Artist` → Vorbis ARTIST comment
//! - `FLAC:Album` → Vorbis ALBUM comment
//! - `FLAC:SampleRate` → StreamInfo sample rate
//!
//! # File Structure
//!
//! ```text
//! [fLaC signature - 4 bytes]
//! [Metadata Block 0: STREAMINFO - required, always first]
//! [Metadata Block 1-N: Optional blocks]
//!   ├─ PADDING (1)
//!   ├─ APPLICATION (2)
//!   ├─ SEEKTABLE (3)
//!   ├─ VORBIS_COMMENT (4) ← Primary metadata source
//!   ├─ CUESHEET (5)
//!   └─ PICTURE (6) ← Album artwork
//! [Audio frames...]
//! ```
//!
//! # References
//!
//! - FLAC Format: <https://xiph.org/flac/format.html>
//! - Vorbis Comment Spec: <https://www.xiph.org/vorbis/doc/v-comment.html>
//! - ExifTool Source: `lib/Image/ExifTool/FLAC.pm`

#![allow(dead_code)]

use crate::core::{FileReader, FormatParser, MetadataMap};
use crate::error::{ExifToolError, Result};
use nom::{
    bytes::complete::tag,
    number::complete::{be_u16, be_u24, be_u32, be_u8},
    IResult,
};

/// FLAC file signature
const FLAC_SIGNATURE: &[u8] = b"fLaC";

/// Metadata block types
const BLOCK_TYPE_STREAMINFO: u8 = 0;
const BLOCK_TYPE_PADDING: u8 = 1;
const BLOCK_TYPE_APPLICATION: u8 = 2;
const BLOCK_TYPE_SEEKTABLE: u8 = 3;
const BLOCK_TYPE_VORBIS_COMMENT: u8 = 4;
const BLOCK_TYPE_CUESHEET: u8 = 5;
const BLOCK_TYPE_PICTURE: u8 = 6;

/// FLAC parser
pub struct FlacParser;

impl FormatParser for FlacParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify file size
        let file_size = reader.size();
        if file_size < 8 {
            return Err(ExifToolError::parse_error("File too small to be FLAC"));
        }

        // Read and verify signature
        let header = reader.read(0, 4)?;
        if header != FLAC_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid FLAC signature: expected {:?}, found {:?}",
                FLAC_SIGNATURE, header
            )));
        }

        // Initialize metadata map
        let mut metadata = MetadataMap::with_capacity(32);

        // Parse metadata blocks
        let mut offset = 4u64; // After "fLaC"
        let mut is_last = false;

        while !is_last && offset < file_size {
            // Read block header (4 bytes)
            let block_header = reader.read(offset, 4)?;
            let (_, (is_last_flag, block_type, block_length)) =
                parse_block_header(block_header)
                    .map_err(|e| ExifToolError::parse_error(format!("Failed to parse block header: {:?}", e)))?;

            is_last = is_last_flag;
            offset += 4;

            // Read block data
            if block_length > 0 && offset + block_length as u64 <= file_size {
                let block_data = reader.read(offset, block_length as usize)?;

                // Process block based on type
                match block_type {
                    BLOCK_TYPE_STREAMINFO => {
                        parse_streaminfo_block(block_data, &mut metadata)?;
                    }
                    BLOCK_TYPE_VORBIS_COMMENT => {
                        parse_vorbis_comment_block(block_data, &mut metadata)?;
                    }
                    BLOCK_TYPE_PICTURE => {
                        parse_picture_block(block_data, &mut metadata)?;
                    }
                    _ => {
                        // Skip other block types for now
                    }
                }

                offset += block_length as u64;
            } else {
                break;
            }
        }

        Ok(metadata)
    }
}

/// Parses FLAC metadata block header (1 + 3 bytes)
///
/// Returns: (is_last, block_type, block_length)
fn parse_block_header(input: &[u8]) -> IResult<&[u8], (bool, u8, u32)> {
    let (input, header_byte) = be_u8(input)?;
    let is_last = (header_byte & 0x80) != 0;
    let block_type = header_byte & 0x7F;

    let (input, length) = be_u24(input)?;

    Ok((input, (is_last, block_type, length)))
}

/// Parses STREAMINFO block (34 bytes)
fn parse_streaminfo_block(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.len() < 34 {
        return Err(ExifToolError::parse_error("STREAMINFO block too small"));
    }

    // Parse fields using nom
    let (_, stream_info) = parse_streaminfo(data)
        .map_err(|e| ExifToolError::parse_error(format!("Failed to parse STREAMINFO: {:?}", e)))?;

    // Add to metadata
    use crate::core::TagValue;

    metadata.insert(
        "FLAC:MinBlockSize".to_string(),
        TagValue::new_integer(stream_info.min_block_size as i64),
    );
    metadata.insert(
        "FLAC:MaxBlockSize".to_string(),
        TagValue::new_integer(stream_info.max_block_size as i64),
    );
    metadata.insert(
        "FLAC:SampleRate".to_string(),
        TagValue::new_integer(stream_info.sample_rate as i64),
    );
    metadata.insert(
        "FLAC:Channels".to_string(),
        TagValue::new_integer(stream_info.channels as i64),
    );
    metadata.insert(
        "FLAC:BitsPerSample".to_string(),
        TagValue::new_integer(stream_info.bits_per_sample as i64),
    );
    metadata.insert(
        "FLAC:TotalSamples".to_string(),
        TagValue::new_integer(stream_info.total_samples as i64),
    );

    // Calculate duration if sample rate > 0
    if stream_info.sample_rate > 0 {
        let duration_secs = stream_info.total_samples as f64 / stream_info.sample_rate as f64;
        metadata.insert(
            "FLAC:Duration".to_string(),
            TagValue::new_string(format!("{:.2}", duration_secs)),
        );
    }

    Ok(())
}

/// STREAMINFO structure
#[derive(Debug)]
struct StreamInfo {
    min_block_size: u16,
    max_block_size: u16,
    min_frame_size: u32, // 24-bit
    max_frame_size: u32, // 24-bit
    sample_rate: u32,    // 20-bit
    channels: u8,        // 3-bit (stored as 1-8)
    bits_per_sample: u8, // 5-bit (stored as 1-32)
    total_samples: u64,  // 36-bit
    md5: [u8; 16],
}

fn parse_streaminfo(input: &[u8]) -> IResult<&[u8], StreamInfo> {
    let (input, min_block_size) = be_u16(input)?;
    let (input, max_block_size) = be_u16(input)?;
    let (input, min_frame_size) = be_u24(input)?;
    let (input, max_frame_size) = be_u24(input)?;

    // Next 8 bytes contain sample_rate (20 bits), channels (3 bits), bits_per_sample (5 bits), total_samples (36 bits)
    let (input, bytes) = nom::bytes::complete::take(8usize)(input)?;

    // Parse bit-packed fields
    let sample_rate = (u32::from(bytes[0]) << 12) | (u32::from(bytes[1]) << 4) | (u32::from(bytes[2]) >> 4);
    let channels = ((bytes[2] >> 1) & 0x07) + 1; // 3 bits, add 1 (1-8 channels)
    let bits_per_sample = (((bytes[2] & 0x01) << 4) | (bytes[3] >> 4)) + 1; // 5 bits, add 1 (1-32 bits)

    // Total samples (36 bits)
    let total_samples = (u64::from(bytes[3] & 0x0F) << 32)
        | (u64::from(bytes[4]) << 24)
        | (u64::from(bytes[5]) << 16)
        | (u64::from(bytes[6]) << 8)
        | u64::from(bytes[7]);

    // MD5 hash (16 bytes)
    let (input, md5_bytes) = nom::bytes::complete::take(16usize)(input)?;
    let mut md5 = [0u8; 16];
    md5.copy_from_slice(md5_bytes);

    Ok((
        input,
        StreamInfo {
            min_block_size,
            max_block_size,
            min_frame_size,
            max_frame_size,
            sample_rate,
            channels,
            bits_per_sample,
            total_samples,
            md5,
        },
    ))
}

/// Parses VORBIS_COMMENT block
fn parse_vorbis_comment_block(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    use encoding_rs::UTF_8;

    let mut offset = 0;

    // Vendor string length (4 bytes, little-endian)
    if data.len() < 4 {
        return Err(ExifToolError::parse_error("Vorbis comment block too small"));
    }

    let vendor_length = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    offset += 4;

    // Skip vendor string
    if offset + vendor_length > data.len() {
        return Err(ExifToolError::parse_error("Invalid vendor string length"));
    }
    offset += vendor_length;

    // User comment list length (4 bytes, little-endian)
    if offset + 4 > data.len() {
        return Err(ExifToolError::parse_error("Missing comment list length"));
    }

    let comment_count = u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]);
    offset += 4;

    // Parse each comment
    for _ in 0..comment_count {
        if offset + 4 > data.len() {
            break;
        }

        // Comment length (4 bytes, little-endian)
        let comment_length = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + comment_length > data.len() {
            break;
        }

        // Comment string (UTF-8)
        let comment_bytes = &data[offset..offset + comment_length];
        let (comment_str, _, _) = UTF_8.decode(comment_bytes);

        // Split on first '=' to get field name and value
        if let Some(eq_pos) = comment_str.find('=') {
            let field_name = &comment_str[..eq_pos];
            let field_value = &comment_str[eq_pos + 1..];

            // Map to FLAC: prefix
            let tag_name = format!("FLAC:{}", field_name);
            metadata.insert(tag_name, crate::core::TagValue::new_string(field_value.to_string()));
        }

        offset += comment_length;
    }

    Ok(())
}

/// Parses PICTURE block
fn parse_picture_block(_data: &[u8], _metadata: &mut MetadataMap) -> Result<()> {
    // TODO: Implement picture block parsing
    // For now, just skip it
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_block_header() {
        // Last block, type 0 (STREAMINFO), length 34
        let data = [0x80, 0x00, 0x00, 0x22];
        let (_, (is_last, block_type, length)) = parse_block_header(&data).unwrap();
        assert!(is_last);
        assert_eq!(block_type, 0);
        assert_eq!(length, 34);

        // Not last block, type 4 (VORBIS_COMMENT), length 1024
        let data = [0x04, 0x00, 0x04, 0x00];
        let (_, (is_last, block_type, length)) = parse_block_header(&data).unwrap();
        assert!(!is_last);
        assert_eq!(block_type, 4);
        assert_eq!(length, 1024);
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --test flac_tests`

Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/audio/flac.rs tests/unit/audio/flac_tests.rs
git commit -m "feat: implement basic FLAC parser

- Add FLAC file signature verification
- Parse STREAMINFO metadata block
- Parse VORBIS_COMMENT metadata
- Add unit tests for magic byte validation"
```

### Task 1.2: FLAC Format Detection

**Files:**
- Modify: `src/parsers/format_detector.rs`
- Create: `tests/unit/format_detection/phase1_tests.rs`

**Step 1: Write the failing test**

Create `tests/unit/format_detection/phase1_tests.rs`:

```rust
use oxidex::parsers::format_detector::detect_format;
use oxidex::core::FileFormat;
use oxidex::io::BufferedReader;

#[test]
fn test_detect_flac_by_magic() {
    let data = b"fLaC\x00\x00\x00\x22...";
    let reader = BufferedReader::from_bytes(data);
    let format = detect_format(&reader).unwrap();
    assert_eq!(format, FileFormat::FLAC);
}

#[test]
fn test_detect_flac_by_extension() {
    // Test extension fallback when magic bytes aren't available
    // (This will be implemented in detect_by_extension)
}
```

**Step 2: Run test**

Run: `cargo test test_detect_flac_by_magic`

Expected: FAIL - format detection doesn't recognize FLAC

**Step 3: Update format_detector.rs**

In `src/parsers/format_detector.rs`, add FLAC detection after PNG detection:

```rust
// FLAC: "fLaC" signature
if bytes.len() >= 4 && &bytes[0..4] == b"fLaC" {
    return Ok(FileFormat::FLAC);
}
```

**Step 4: Run test**

Run: `cargo test test_detect_flac_by_magic`

Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/format_detector.rs tests/unit/format_detection/phase1_tests.rs
git commit -m "feat: add FLAC format detection

- Detect FLAC by magic bytes (fLaC signature)
- Add format detection tests for Phase 1 formats"
```

### Task 1.3: FLAC Integration Test with ExifTool

**Files:**
- Create: `test_data/audio/sample.flac` (download test file)
- Create: `tests/integration/flac_integration_tests.rs`

**Step 1: Create test data directory**

```bash
mkdir -p test_data/audio
```

**Step 2: Download sample FLAC file**

Download a CC0/public domain FLAC file for testing.

**Step 3: Write integration test**

Create `tests/integration/flac_integration_tests.rs`:

```rust
use oxidex::core::MetadataMap;
use std::process::Command;
use serde_json::Value;

#[test]
#[ignore] // Requires ExifTool to be installed
fn test_flac_metadata_parity_with_exiftool() {
    let test_file = "test_data/audio/sample.flac";

    // Run ExifTool
    let exiftool_output = Command::new("exiftool")
        .arg("-json")
        .arg(test_file)
        .output()
        .expect("Failed to run exiftool - is it installed?");

    assert!(exiftool_output.status.success(), "ExifTool failed");

    let exiftool_json: Vec<Value> = serde_json::from_slice(&exiftool_output.stdout)
        .expect("Failed to parse ExifTool JSON");

    // Run OxiDex
    let oxidex_metadata = MetadataMap::from_file(test_file)
        .expect("Failed to parse FLAC file");

    // Compare key tags
    let tags_to_compare = [
        "FLAC:SampleRate",
        "FLAC:Channels",
        "FLAC:BitsPerSample",
    ];

    for tag in &tags_to_compare {
        let exiftool_value = &exiftool_json[0][tag];
        let oxidex_value = oxidex_metadata.get(tag);

        assert!(
            oxidex_value.is_some(),
            "OxiDex missing tag: {}",
            tag
        );

        // Compare values (convert to strings for comparison)
        let exiftool_str = exiftool_value.to_string().trim_matches('"').to_string();
        let oxidex_str = oxidex_value.unwrap().to_string();

        assert_eq!(
            exiftool_str, oxidex_str,
            "Mismatch for tag {}: ExifTool={}, OxiDex={}",
            tag, exiftool_str, oxidex_str
        );
    }
}
```

**Step 4: Run integration test**

Run: `cargo test test_flac_metadata_parity_with_exiftool -- --ignored`

Expected: PASS (if ExifTool is installed)

**Step 5: Commit**

```bash
git add test_data/audio/sample.flac tests/integration/flac_integration_tests.rs
git commit -m "test: add FLAC integration tests with ExifTool parity

- Add sample FLAC test file
- Verify metadata extraction matches ExifTool output
- Compare SampleRate, Channels, BitsPerSample tags"
```

### Task 1.4: FLAC Benchmarks

**Files:**
- Create: `benches/audio_benchmarks.rs`

**Step 1: Create benchmark file**

Create `benches/audio_benchmarks.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oxidex::parsers::audio::flac::FlacParser;
use oxidex::core::FormatParser;
use oxidex::io::BufferedReader;
use std::path::Path;

fn bench_flac_parsing(c: &mut Criterion) {
    let test_file = Path::new("test_data/audio/sample.flac");

    if !test_file.exists() {
        eprintln!("Warning: test_data/audio/sample.flac not found, skipping benchmark");
        return;
    }

    c.bench_function("flac_parse", |b| {
        b.iter(|| {
            let reader = BufferedReader::new(black_box(test_file))
                .expect("Failed to create reader");
            let parser = FlacParser;
            parser.parse(&reader).expect("Failed to parse FLAC");
        })
    });
}

criterion_group!(benches, bench_flac_parsing);
criterion_main!(benches);
```

**Step 2: Run benchmark**

Run: `cargo bench --bench audio_benchmarks`

**Step 3: Verify performance target**

Expected: Parsing time < 20ms (target from design doc)

**Step 4: Commit**

```bash
git add benches/audio_benchmarks.rs
git commit -m "perf: add FLAC parsing benchmarks

- Benchmark FLAC metadata extraction
- Target: <20ms per file"
```

### Task 1.5: FLAC Fuzzing

**Files:**
- Create: `fuzz/fuzz_targets/fuzz_flac.rs`
- Create: `fuzz/corpus/fuzz_flac/.gitkeep`

**Step 1: Create fuzzing target**

Create `fuzz/fuzz_targets/fuzz_flac.rs`:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxidex::parsers::audio::flac::FlacParser;
use oxidex::core::FormatParser;
use oxidex::io::BufferedReader;

fuzz_target!(|data: &[u8]| {
    let reader = BufferedReader::from_bytes(data);
    let parser = FlacParser;

    // Should never panic, even on malicious input
    let _ = parser.parse(&reader);
});
```

**Step 2: Create corpus directory**

```bash
mkdir -p fuzz/corpus/fuzz_flac
cp test_data/audio/sample.flac fuzz/corpus/fuzz_flac/
```

**Step 3: Run fuzzer (short test)**

Run: `cargo fuzz run fuzz_flac -- -max_total_time=60`

**Step 4: Verify no crashes**

Expected: No crashes or panics found

**Step 5: Commit**

```bash
git add fuzz/fuzz_targets/fuzz_flac.rs fuzz/corpus/fuzz_flac/
git commit -m "test: add FLAC fuzzing target

- Fuzz FLAC parser with libfuzzer
- Seed corpus with sample.flac
- Verify no panics on malicious input"
```

---

## Parser 2: MP3 (MPEG Audio Layer 3)

**Reference:** ExifTool `lib/Image/ExifTool/ID3.pm`
**Magic Bytes:** `FF FB` or `FF FA` (frame sync) or `49 44 33` (ID3v2 header "ID3")
**Spec:** http://id3.org/

### Task 2.1: MP3 Parser Foundation

**Files:**
- Create: `src/parsers/audio/mp3.rs`
- Create: `tests/unit/audio/mp3_tests.rs`

**Step 1: Write the failing test**

Create `tests/unit/audio/mp3_tests.rs`:

```rust
use oxidex::parsers::audio::mp3::Mp3Parser;
use oxidex::core::FormatParser;
use oxidex::io::BufferedReader;

#[test]
fn test_mp3_id3v2_magic() {
    // ID3v2 header
    let data = b"ID3\x04\x00\x00\x00\x00\x00\x00...";
    let reader = BufferedReader::from_bytes(data);
    let parser = Mp3Parser;
    let result = parser.parse(&reader);

    assert!(result.is_ok());
}

#[test]
fn test_mp3_frame_sync() {
    // MP3 frame sync
    let data = b"\xFF\xFB\x90\x00...";
    let reader = BufferedReader::from_bytes(data);
    let parser = Mp3Parser;
    let result = parser.parse(&reader);

    assert!(result.is_ok());
}
```

**Step 2: Run test**

Run: `cargo test --test mp3_tests`

Expected: FAIL - module not found

**Step 3: Create MP3 parser**

Create `src/parsers/audio/mp3.rs`:

```rust
//! MP3 (MPEG Audio Layer 3) format parser
//!
//! Implements metadata extraction from MP3 audio files, supporting ID3v1,
//! ID3v2.3, and ID3v2.4 tags.
//!
//! # Supported Metadata
//!
//! - **ID3v1:** Title, Artist, Album, Year, Comment, Genre, Track
//! - **ID3v2:** All standard frames (TIT2, TPE1, TALB, etc.)
//! - **MPEG Info:** Bitrate, sample rate, duration, channel mode
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `ID3.pm` module:
//! - `ID3:Title` → TIT2 frame
//! - `ID3:Artist` → TPE1 frame
//! - `ID3:Album` → TALB frame
//!
//! # File Structure
//!
//! ```text
//! [ID3v2 tag - optional, at start]
//!   ├─ Header (10 bytes)
//!   └─ Frames (variable)
//! [MPEG audio frames]
//! [ID3v1 tag - optional, last 128 bytes]
//! ```
//!
//! # References
//!
//! - ID3v2.4 Spec: <http://id3.org/id3v2.4.0-structure>
//! - ID3v2.3 Spec: <http://id3.org/id3v2.3.0>
//! - ID3v1 Spec: <http://id3.org/ID3v1>
//! - ExifTool Source: `lib/Image/ExifTool/ID3.pm`

#![allow(dead_code)]

use crate::core::{FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use encoding_rs::Encoding;
use nom::{
    bytes::complete::{tag, take},
    number::complete::{be_u32, be_u8},
    IResult,
};

/// ID3v2 signature
const ID3V2_SIGNATURE: &[u8] = b"ID3";

/// ID3v1 signature
const ID3V1_SIGNATURE: &[u8] = b"TAG";

/// MP3 parser
pub struct Mp3Parser;

impl FormatParser for Mp3Parser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let file_size = reader.size();
        let mut metadata = MetadataMap::with_capacity(32);

        // Try to parse ID3v2 tag (at start of file)
        if file_size >= 10 {
            let header = reader.read(0, 10)?;
            if &header[0..3] == ID3V2_SIGNATURE {
                parse_id3v2(reader, &mut metadata)?;
            }
        }

        // Try to parse ID3v1 tag (last 128 bytes)
        if file_size >= 128 {
            let id3v1_offset = file_size - 128;
            let id3v1_data = reader.read(id3v1_offset, 128)?;
            if &id3v1_data[0..3] == ID3V1_SIGNATURE {
                parse_id3v1(id3v1_data, &mut metadata)?;
            }
        }

        Ok(metadata)
    }
}

/// Parse ID3v2 tag
fn parse_id3v2(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    // Read ID3v2 header (10 bytes)
    let header = reader.read(0, 10)?;
    let (_, id3v2_header) = parse_id3v2_header(header)
        .map_err(|e| ExifToolError::parse_error(format!("Failed to parse ID3v2 header: {:?}", e)))?;

    metadata.insert(
        "ID3:Version".to_string(),
        TagValue::new_string(format!("2.{}.{}", id3v2_header.version, id3v2_header.revision)),
    );

    // Read frames
    let frames_size = id3v2_header.size as usize;
    if frames_size > 0 {
        let frames_data = reader.read(10, frames_size)?;
        parse_id3v2_frames(frames_data, id3v2_header.version, metadata)?;
    }

    Ok(())
}

#[derive(Debug)]
struct ID3v2Header {
    version: u8,
    revision: u8,
    flags: u8,
    size: u32, // Synchsafe integer
}

fn parse_id3v2_header(input: &[u8]) -> IResult<&[u8], ID3v2Header> {
    let (input, _) = tag(ID3V2_SIGNATURE)(input)?;
    let (input, version) = be_u8(input)?;
    let (input, revision) = be_u8(input)?;
    let (input, flags) = be_u8(input)?;
    let (input, size_bytes) = take(4usize)(input)?;

    // Decode synchsafe integer (7 bits per byte)
    let size = decode_synchsafe_u32(size_bytes);

    Ok((
        input,
        ID3v2Header {
            version,
            revision,
            flags,
            size,
        },
    ))
}

/// Decode synchsafe integer (ID3v2 size encoding)
fn decode_synchsafe_u32(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32 & 0x7F) << 21)
        | ((bytes[1] as u32 & 0x7F) << 14)
        | ((bytes[2] as u32 & 0x7F) << 7)
        | (bytes[3] as u32 & 0x7F)
}

/// Parse ID3v2 frames
fn parse_id3v2_frames(data: &[u8], version: u8, metadata: &mut MetadataMap) -> Result<()> {
    let mut offset = 0;

    while offset + 10 < data.len() {
        // Frame header size depends on version
        let (frame_id, frame_size, frame_flags) = if version >= 3 {
            // ID3v2.3 and v2.4: 10-byte header
            if &data[offset..offset + 4] == b"\x00\x00\x00\x00" {
                break; // Padding
            }

            let frame_id = String::from_utf8_lossy(&data[offset..offset + 4]).to_string();
            let frame_size = if version == 4 {
                // ID3v2.4 uses synchsafe integers
                decode_synchsafe_u32(&data[offset + 4..offset + 8])
            } else {
                // ID3v2.3 uses regular integers
                u32::from_be_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ])
            };
            let frame_flags = u16::from_be_bytes([data[offset + 8], data[offset + 9]]);
            offset += 10;

            (frame_id, frame_size, frame_flags)
        } else {
            // ID3v2.2: 6-byte header
            let frame_id = String::from_utf8_lossy(&data[offset..offset + 3]).to_string();
            let frame_size = u32::from_be_bytes([0, data[offset + 3], data[offset + 4], data[offset + 5]]);
            offset += 6;

            (frame_id, frame_size, 0)
        };

        // Read frame data
        if offset + frame_size as usize > data.len() {
            break;
        }

        let frame_data = &data[offset..offset + frame_size as usize];
        offset += frame_size as usize;

        // Parse text frames
        if frame_id.starts_with('T') && frame_id != "TXXX" {
            if let Ok(text) = parse_text_frame(frame_data) {
                let tag_name = format!("ID3:{}", map_frame_id_to_tag_name(&frame_id));
                metadata.insert(tag_name, TagValue::new_string(text));
            }
        }
    }

    Ok(())
}

/// Parse text frame (TXX encoding + text)
fn parse_text_frame(data: &[u8]) -> Result<String> {
    if data.is_empty() {
        return Err(ExifToolError::parse_error("Empty text frame"));
    }

    let encoding_byte = data[0];
    let text_data = &data[1..];

    let encoding = match encoding_byte {
        0 => encoding_rs::WINDOWS_1252, // ISO-8859-1
        1 => encoding_rs::UTF_16LE,
        2 => encoding_rs::UTF_16BE,
        3 => encoding_rs::UTF_8,
        _ => encoding_rs::UTF_8, // Default to UTF-8
    };

    let (decoded, _, _) = encoding.decode(text_data);
    Ok(decoded.trim_end_matches('\0').to_string())
}

/// Map ID3v2 frame ID to tag name
fn map_frame_id_to_tag_name(frame_id: &str) -> &str {
    match frame_id {
        "TIT2" => "Title",
        "TPE1" => "Artist",
        "TALB" => "Album",
        "TYER" | "TDRC" => "Year",
        "TCON" => "Genre",
        "TRCK" => "Track",
        "COMM" => "Comment",
        _ => frame_id,
    }
}

/// Parse ID3v1 tag
fn parse_id3v1(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.len() < 128 || &data[0..3] != ID3V1_SIGNATURE {
        return Err(ExifToolError::parse_error("Invalid ID3v1 tag"));
    }

    // Extract fields (all ISO-8859-1 encoded)
    let title = decode_latin1(&data[3..33]);
    let artist = decode_latin1(&data[33..63]);
    let album = decode_latin1(&data[63..93]);
    let year = decode_latin1(&data[93..97]);
    let comment = decode_latin1(&data[97..127]);
    let genre = data[127];

    if !title.is_empty() {
        metadata.insert("ID3v1:Title".to_string(), TagValue::new_string(title));
    }
    if !artist.is_empty() {
        metadata.insert("ID3v1:Artist".to_string(), TagValue::new_string(artist));
    }
    if !album.is_empty() {
        metadata.insert("ID3v1:Album".to_string(), TagValue::new_string(album));
    }
    if !year.is_empty() {
        metadata.insert("ID3v1:Year".to_string(), TagValue::new_string(year));
    }
    if !comment.is_empty() {
        metadata.insert("ID3v1:Comment".to_string(), TagValue::new_string(comment));
    }
    if genre < 192 {
        metadata.insert(
            "ID3v1:Genre".to_string(),
            TagValue::new_integer(genre as i64),
        );
    }

    Ok(())
}

/// Decode Latin-1 (ISO-8859-1) string, trimming null bytes
fn decode_latin1(bytes: &[u8]) -> String {
    let (decoded, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
    decoded.trim_end_matches('\0').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_synchsafe_u32() {
        assert_eq!(decode_synchsafe_u32(&[0x00, 0x00, 0x00, 0x00]), 0);
        assert_eq!(decode_synchsafe_u32(&[0x00, 0x00, 0x00, 0x7F]), 127);
        assert_eq!(decode_synchsafe_u32(&[0x00, 0x00, 0x01, 0x00]), 128);
        assert_eq!(decode_synchsafe_u32(&[0x7F, 0x7F, 0x7F, 0x7F]), 268435455);
    }

    #[test]
    fn test_map_frame_id_to_tag_name() {
        assert_eq!(map_frame_id_to_tag_name("TIT2"), "Title");
        assert_eq!(map_frame_id_to_tag_name("TPE1"), "Artist");
        assert_eq!(map_frame_id_to_tag_name("TALB"), "Album");
    }
}
```

**Step 4: Run tests**

Run: `cargo test --test mp3_tests`

Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/audio/mp3.rs tests/unit/audio/mp3_tests.rs
git commit -m "feat: implement MP3 parser with ID3v1/ID3v2 support

- Parse ID3v2.2, ID3v2.3, ID3v2.4 tags
- Parse ID3v1 tags
- Support multiple text encodings (ISO-8859-1, UTF-8, UTF-16)
- Map common frames (TIT2, TPE1, TALB, etc.) to tag names"
```

---

## Parser 3: MKV (Matroska)

**Reference:** ExifTool `lib/Image/ExifTool/Matroska.pm`
**Magic Bytes:** `1A 45 DF A3` (EBML header)
**Spec:** https://www.matroska.org/technical/elements.html

### Task 3.1: MKV Parser Foundation

**Files:**
- Create: `src/parsers/video/mkv.rs`
- Create: `tests/unit/video/mkv_tests.rs`

**Step 1: Write the failing test**

Create `tests/unit/video/mkv_tests.rs`:

```rust
use oxidex::parsers::video::mkv::MkvParser;
use oxidex::core::FormatParser;
use oxidex::io::BufferedReader;

#[test]
fn test_mkv_ebml_magic() {
    let data = b"\x1A\x45\xDF\xA3...";
    let reader = BufferedReader::from_bytes(data);
    let parser = MkvParser;
    let result = parser.parse(&reader);

    assert!(result.is_ok());
}
```

**Step 2: Run test**

Run: `cargo test --test mkv_tests`

Expected: FAIL

**Step 3: Create MKV parser skeleton**

Create `src/parsers/video/mkv.rs`:

```rust
//! MKV (Matroska) video format parser
//!
//! Implements metadata extraction from Matroska/WebM container formats
//! following the EBML (Extensible Binary Meta Language) specification.
//!
//! # Supported Metadata
//!
//! - **Title, Artist, Album:** From Tags segment (SimpleTag elements)
//! - **Duration:** From SegmentInfo (Duration element)
//! - **Codec Information:** From Tracks segment
//! - **Creation Date:** From DateUTC element
//! - **Muxing Application:** From MuxingApp element
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `Matroska.pm` module:
//! - `Matroska:Title` → Title from Tags
//! - `Matroska:Duration` → Duration from SegmentInfo
//! - `Matroska:MuxingApp` → MuxingApp from SegmentInfo
//!
//! # File Structure
//!
//! ```text
//! [EBML Header - required]
//!   ├─ EBMLVersion
//!   ├─ DocType ("matroska" or "webm")
//!   └─ DocTypeVersion
//! [Segment - main container]
//!   ├─ SeekHead (index to other segments)
//!   ├─ Info (duration, dates, muxing app)
//!   ├─ Tracks (video/audio codec info)
//!   ├─ Tags (metadata - PRIMARY METADATA SOURCE)
//!   └─ Clusters (actual media data - SKIP)
//! ```
//!
//! # References
//!
//! - EBML RFC: <https://www.rfc-editor.org/rfc/rfc8794.html>
//! - Matroska Spec: <https://www.matroska.org/technical/elements.html>
//! - ExifTool Source: `lib/Image/ExifTool/Matroska.pm`

#![allow(dead_code)]

use crate::core::{FileReader, FormatParser, MetadataMap};
use crate::error::{ExifToolError, Result};

/// EBML header signature
const EBML_SIGNATURE: &[u8] = b"\x1A\x45\xDF\xA3";

/// MKV parser
pub struct MkvParser;

impl FormatParser for MkvParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify EBML signature
        if reader.size() < 4 {
            return Err(ExifToolError::parse_error("File too small to be MKV"));
        }

        let header = reader.read(0, 4)?;
        if header != EBML_SIGNATURE {
            return Err(ExifToolError::parse_error("Invalid MKV signature"));
        }

        // TODO: Implement full EBML/Matroska parsing
        let metadata = MetadataMap::new();

        Ok(metadata)
    }
}
```

**Step 4: Run test**

Run: `cargo test --test mkv_tests`

Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/video/mkv.rs tests/unit/video/mkv_tests.rs
git commit -m "feat: add MKV parser skeleton

- Verify EBML signature
- Add basic structure for Matroska parsing
- Full implementation to follow"
```

---

## Remaining Parsers (Tasks 4-12)

Due to the massive scope of this plan, I'll provide a structured outline for the remaining parsers. Each follows the same TDD pattern:

### Parser 4: OGG Vorbis
- Magic bytes: `4F 67 67 53` ("OggS")
- Reference: `Ogg.pm`
- Parse Vorbis comments

### Parser 5: WAV (RIFF)
- Magic bytes: `52 49 46 46` ("RIFF") + `57 41 56 45` ("WAVE")
- Reference: `RIFF.pm`
- Parse INFO chunks

### Parser 6: AVI (RIFF)
- Magic bytes: `52 49 46 46` ("RIFF") + `41 56 49 20` ("AVI ")
- Reference: `RIFF.pm`
- Shares parsing with WAV

### Parser 7: FLV (Flash Video)
- Magic bytes: `46 4C 56` ("FLV")
- Parse onMetaData scriptDataObject

### Parser 8: MTS (MPEG Transport Stream)
- Magic bytes: `47` (sync byte, repeating every 188 bytes)
- Parse PMT/PAT tables

### Parser 9: AAC (MPEG-4 Audio)
- Magic bytes: `FF F1` or `FF F9` (ADTS sync)
- Parse ADTS headers

### Parser 10: OPUS
- Magic bytes: `4F 70 75 73` ("Opus") inside Ogg container
- Parse Opus tags

### Parser 11: APE (Monkey's Audio)
- Magic bytes: `4D 41 43 20` ("MAC ")
- Parse APEv2 tags

### Parser 12: WEBM
- Same as MKV (shares EBML parser)
- Different DocType: "webm" instead of "matroska"

---

## Integration & Testing

### Task 13: Batch Integration Tests

**Files:**
- Create: `tests/integration/phase1_batch_tests.rs`

Test all 12 formats together, verify ExifTool parity for each.

### Task 14: Performance Benchmarks

**Files:**
- Modify: `benches/audio_benchmarks.rs`
- Create: `benches/video_benchmarks.rs`

Benchmark all parsers, ensure <20ms target met.

### Task 15: Fuzzing All Formats

Add fuzzing targets for all 12 parsers, run corpus for each.

---

## Documentation

### Task 16: API Documentation

Update rustdoc for all parsers with examples and tag mappings.

### Task 17: User Guide

Create `docs/user-guide/video-formats.md` and `docs/user-guide/audio-formats.md`.

---

## Release

### Task 18: CHANGELOG

Update CHANGELOG.md with Phase 1 features.

### Task 19: Version Bump

Update Cargo.toml to v1.1.0.

### Task 20: Git Tag

```bash
git tag v1.1.0-phase1
git push origin v1.1.0-phase1
```

---

## Success Criteria

Phase 1 is complete when:

- [ ] All 12 parsers implemented and tested
- [ ] 100% ExifTool parity for all formats
- [ ] All integration tests passing
- [ ] Performance benchmarks < 20ms
- [ ] Fuzzing finds no crashes
- [ ] Documentation complete
- [ ] CI passing

**Estimated Total Tasks:** ~60-80 bite-sized tasks (2-5 minutes each)
**Total Time:** 3-4 months with continuous work

---

**Next Step:** Use `superpowers:executing-plans` or `superpowers:subagent-driven-development` to execute this plan task-by-task.
