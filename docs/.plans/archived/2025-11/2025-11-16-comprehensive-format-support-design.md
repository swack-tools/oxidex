# Comprehensive Format Support - Design Document

**Date:** 2025-11-16
**Status:** Approved
**Scope:** Add support for all ExifTool file formats via phased implementation

## Executive Summary

This design outlines a comprehensive plan to add support for every file format supported by ExifTool, organized into 6 major phases over 12-18 months. The implementation will reference ExifTool Perl source code to ensure 100% compatibility while maintaining OxiDex's performance advantages (7-16x faster than Perl ExifTool).

## Goals

1. **Comprehensive Coverage:** Support all 400+ file formats from ExifTool
2. **ExifTool Parity:** Maintain 100%+ tag compatibility (currently 32,677 tags, 113% of ExifTool)
3. **Performance:** Preserve 7-16x performance advantage over Perl ExifTool
4. **Quality:** Maintain robust error handling, fuzzing, and test coverage
5. **Maintainability:** Clean architecture following hexagonal design patterns

## Phase Structure

### Phase 1: Video/Audio Formats (~3-4 months)

**Formats:**
- **Video Containers:** MKV, WEBM, MTS, FLV, AVI
- **Audio Codecs:** MP3, FLAC, AAC, WAV, OGG, OPUS, APE

**Deliverables:**
- 12 new format parsers
- ~1,450 new metadata tags
- ExifTool comparison tests for all formats
- Performance benchmarks (target: <20ms per file)

**ExifTool Reference Modules:**
- `Matroska.pm` → `src/parsers/video/mkv.rs`
- `FLAC.pm` → `src/parsers/audio/flac.rs`
- `ID3.pm` → `src/parsers/audio/mp3.rs`
- `Ogg.pm` → `src/parsers/audio/ogg.rs`
- `RIFF.pm` → `src/parsers/video/avi.rs`, `src/parsers/audio/wav.rs`

### Phase 2: Document Formats (~2-3 months)

**Formats:**
- **Office Open XML:** DOCX, XLSX, PPTX
- **iWork:** Pages, Numbers, Keynote
- **E-books:** EPUB

**Key Insight:** Office formats are ZIP archives containing XML metadata

**Deliverables:**
- 8 new format parsers (includes ZIP foundation)
- ~800 new metadata tags
- XML parsing utilities for OOXML formats

**ExifTool Reference Modules:**
- `OOXML.pm` → `src/parsers/document/ooxml.rs`
- `iWork.pm` → `src/parsers/document/iwork.rs`
- `ZIP.pm` → `src/parsers/archive/zip.rs`

### Phase 3: Archive Formats (~1-2 months)

**Formats:**
- ZIP, RAR, 7z, ISO, TAR, GZ

**Note:** ZIP parser built in Phase 2 as foundation for Office formats

**Deliverables:**
- 6 new format parsers
- ~200 new metadata tags
- Archive-specific metadata (compression ratio, file lists, checksums)

**ExifTool Reference Modules:**
- `ZIP.pm`, `RAR.pm`, `7Z.pm`, `ISO.pm`

### Phase 4: Font Files (~1 month)

**Formats:**
- TrueType (TTF), OpenType (OTF), WOFF, WOFF2

**Deliverables:**
- 4 new format parsers
- ~150 new metadata tags
- Font metrics, glyph counts, embedding permissions

**ExifTool Reference Modules:**
- `Font.pm` → `src/parsers/font/ttf.rs`, `src/parsers/font/otf.rs`

### Phase 5: Advanced Image Formats (~2-3 months)

**Formats:**
- Next-gen: AVIF, JXL (JPEG XL), BPG
- Professional: EXR (OpenEXR), FLIF
- Vector/Icon: SVG, ICO, ICNS
- Expanded PSD support

**Deliverables:**
- 8 new format parsers
- ~600 new metadata tags
- HDR/color space metadata for professional formats

**ExifTool Reference Modules:**
- `AVIF.pm`, `JXL.pm`, `OpenEXR.pm`, `SVG.pm`, `Photoshop.pm`

### Phase 6: Specialized Formats (~2-3 months)

**Formats:**
- **Executables:** ELF, Mach-O (extending PE parser patterns)
- **CAD:** DWG, DXF
- **3D:** STL, OBJ, GLTF
- **Scientific:** FITS, HDF5

**Deliverables:**
- 10 new format parsers
- ~500 new metadata tags
- Binary analysis metadata (sections, symbols, dependencies)

**ExifTool Reference Modules:**
- `EXE.pm`, `DWG.pm`, `GLTF.pm`, `FITS.pm`

## Architecture

### Directory Structure

```
src/parsers/
├── video/           # Phase 1
│   ├── mod.rs
│   ├── mkv.rs       # Matroska/WebM
│   ├── webm.rs
│   ├── flv.rs       # Flash Video
│   ├── avi.rs       # AVI/RIFF
│   └── mts.rs       # MPEG Transport Stream
├── audio/           # Phase 1
│   ├── mod.rs
│   ├── mp3.rs       # ID3v1/ID3v2
│   ├── flac.rs      # FLAC + Vorbis Comments
│   ├── aac.rs       # MPEG-4 Audio
│   ├── wav.rs       # RIFF/WAV
│   ├── ogg.rs       # Ogg Vorbis
│   ├── opus.rs      # Opus
│   └── ape.rs       # Monkey's Audio
├── document/        # Phase 2
│   ├── mod.rs
│   ├── ooxml.rs     # DOCX/XLSX/PPTX shared logic
│   ├── iwork.rs     # Pages/Numbers/Keynote
│   └── epub.rs      # EPUB e-books
├── archive/         # Phase 3
│   ├── mod.rs
│   ├── zip.rs       # ZIP (foundation for Office formats)
│   ├── rar.rs       # RAR
│   ├── sevenz.rs    # 7-Zip
│   ├── iso.rs       # ISO 9660
│   └── tar.rs       # TAR archives
├── font/            # Phase 4
│   ├── mod.rs
│   ├── ttf.rs       # TrueType
│   ├── otf.rs       # OpenType
│   └── woff.rs      # Web Open Font Format
├── image_advanced/  # Phase 5
│   ├── mod.rs
│   ├── avif.rs      # AV1 Image Format
│   ├── jxl.rs       # JPEG XL
│   ├── bpg.rs       # Better Portable Graphics
│   ├── exr.rs       # OpenEXR
│   ├── svg.rs       # Scalable Vector Graphics
│   └── psd.rs       # Photoshop (expanded)
└── specialized/     # Phase 6
    ├── mod.rs
    ├── elf.rs       # ELF executables
    ├── macho.rs     # Mach-O executables
    ├── dwg.rs       # AutoCAD
    └── gltf.rs      # 3D graphics
```

### Parser Pattern

All parsers follow the hexagonal architecture with the `FormatParser` trait:

```rust
pub struct MkvParser;

impl FormatParser for MkvParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // 1. Validate magic bytes
        verify_magic(reader, b"\x1A\x45\xDF\xA3")?;

        // 2. Parse format-specific structure
        let ebml_header = parse_ebml_header(reader)?;
        let segments = parse_segments(reader)?;

        // 3. Extract metadata
        let mut metadata = MetadataMap::new();
        extract_tags(&segments, &mut metadata)?;

        // 4. Return mapped tags
        Ok(metadata)
    }
}
```

### FileFormat Enum Expansion

```rust
pub enum FileFormat {
    // Existing
    JPEG, TIFF, PNG, PDF, GIF, BMP, QuickTime, HEIF, WebP, RAW, PE,

    // Phase 1: Video/Audio
    MKV, WEBM, FLV, AVI, MTS,
    MP3, FLAC, AAC, WAV, OGG, OPUS, APE,

    // Phase 2: Documents
    DOCX, XLSX, PPTX,
    Pages, Numbers, Keynote,
    EPUB,

    // Phase 3: Archives
    ZIP, RAR, SevenZ, ISO, TAR, GZ,

    // Phase 4: Fonts
    TTF, OTF, WOFF, WOFF2,

    // Phase 5: Advanced Images
    AVIF, JXL, BPG, EXR, FLIF, SVG, ICO, PSD,

    // Phase 6: Specialized
    ELF, MachO, DWG, DXF, STL, GLTF, FITS, HDF5,

    Unknown,
}
```

## Format Detection Strategy

### Two-Tier Detection System

1. **Magic Bytes (Primary):** Fast binary signature matching
2. **Extension Fallback (Secondary):** When magic bytes are ambiguous/absent

```rust
pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat> {
    let magic = reader.read_at(0, 64)?; // Read first 64 bytes

    // Try magic byte detection first
    if let Some(format) = detect_by_magic(&magic) {
        return Ok(format);
    }

    // Fallback to extension for formats without clear magic bytes
    if let Some(path) = reader.path() {
        if let Some(format) = detect_by_extension(path) {
            return Ok(format);
        }
    }

    Ok(FileFormat::Unknown)
}
```

### Magic Byte Examples

| Format | Magic Bytes | Offset | Notes |
|--------|-------------|--------|-------|
| MKV | `1A 45 DF A3` | 0 | EBML header |
| FLAC | `66 4C 61 43` | 0 | "fLaC" signature |
| MP3 | `FF FB` or `49 44 33` | 0 | Frame sync or ID3 |
| DOCX | `50 4B 03 04` | 0 | ZIP signature + check `[Content_Types].xml` |
| TTF | `00 01 00 00` or `74 72 75 65` | 0 | Version or "true" |
| ELF | `7F 45 4C 46` | 0 | "\x7FELF" signature |

## Metadata Extraction Strategy

### ExifTool Perl Source Mapping

For each format, reference the corresponding ExifTool Perl module:

```
ExifTool Perl (lib/Image/ExifTool/) → OxiDex Rust
├── Matroska.pm → src/parsers/video/mkv.rs
├── FLAC.pm → src/parsers/audio/flac.rs
├── ID3.pm → src/parsers/audio/mp3.rs
├── OOXML.pm → src/parsers/document/ooxml.rs
├── ZIP.pm → src/parsers/archive/zip.rs
├── Font.pm → src/parsers/font/ttf.rs
└── EXE.pm → src/parsers/specialized/elf.rs
```

### Translation Process

1. **Extract Tag Definitions:** Parse ExifTool's `%tagTableHash` structures
2. **Port Parsing Logic:** Convert Perl regex/binary parsing to Rust using `nom`
3. **Map Tag Names:** Ensure 1:1 mapping for compatibility
4. **Validate Output:** Compare against ExifTool output for same files

### Example: FLAC Metadata Extraction

**ExifTool Perl (`FLAC.pm`):**
```perl
%Image::ExifTool::FLAC::Main = (
    PROCESS_PROC => \&ProcessFLAC,
    GROUPS => { 2 => 'Audio' },
    NOTES => 'Tags extracted from FLAC audio files',
    0 => {
        Name => 'StreamInfo',
        SubDirectory => { TagTable => 'Image::ExifTool::FLAC::StreamInfo' }
    },
    4 => {
        Name => 'VorbisComment',
        SubDirectory => { TagTable => 'Image::ExifTool::Vorbis::Main' }
    },
);
```

**OxiDex Rust Translation:**
```rust
pub struct FlacParser;

impl FormatParser for FlacParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let data = reader.read_all()?;

        // Verify "fLaC" signature
        verify_magic(&data, b"fLaC")?;

        // Parse metadata blocks
        let mut metadata = MetadataMap::new();
        let blocks = parse_metadata_blocks(&data[4..])?;

        for block in blocks {
            match block.block_type {
                0 => { // StreamInfo
                    extract_stream_info(&block.data, &mut metadata)?;
                }
                4 => { // VorbisComment
                    extract_vorbis_comments(&block.data, &mut metadata)?;
                }
                6 => { // Picture
                    extract_picture_metadata(&block.data, &mut metadata)?;
                }
                _ => {} // Skip other block types
            }
        }

        Ok(metadata)
    }
}

fn parse_metadata_blocks(input: &[u8]) -> IResult<&[u8], Vec<MetadataBlock>> {
    let mut blocks = Vec::new();
    let mut remaining = input;

    loop {
        let (rest, header) = parse_block_header(remaining)?;
        let (rest, data) = take(header.length)(rest)?;

        blocks.push(MetadataBlock {
            block_type: header.block_type,
            data: data.to_vec(),
        });

        if header.is_last {
            break;
        }
        remaining = rest;
    }

    Ok((remaining, blocks))
}
```

## Error Handling & Robustness

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ParserError {
    #[error("Invalid magic bytes: expected {expected:?}, found {found:?}")]
    InvalidMagic { expected: Vec<u8>, found: Vec<u8> },

    #[error("Corrupted {format} structure at offset {offset}")]
    CorruptedStructure { format: String, offset: usize },

    #[error("Unsupported {format} version: {version}")]
    UnsupportedVersion { format: String, version: String },

    #[error("File too large: {size} bytes (max {max})")]
    FileTooLarge { size: usize, max: usize },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),
}
```

### Defensive Parsing Principles

1. **Validate Early:** Check magic bytes and file size before parsing
2. **Bounds Checking:** Never trust offsets/sizes from file data
3. **Fail Gracefully:** Return partial metadata on non-fatal errors
4. **Memory Limits:** Cap buffer allocations to prevent OOM attacks

```rust
impl FormatParser for MkvParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // 1. Validate magic bytes
        let magic = reader.read_at(0, 4)?;
        if magic != b"\x1A\x45\xDF\xA3" {
            return Err(ParserError::InvalidMagic {
                expected: b"\x1A\x45\xDF\xA3".to_vec(),
                found: magic.to_vec(),
            });
        }

        // 2. Cap file size for safety
        const MAX_MKV_SIZE: usize = 10 * 1024 * 1024 * 1024; // 10GB
        let file_size = reader.len();
        if file_size > MAX_MKV_SIZE {
            return Err(ParserError::FileTooLarge {
                size: file_size,
                max: MAX_MKV_SIZE
            });
        }

        // 3. Parse with bounds checking
        let mut metadata = MetadataMap::new();
        match parse_segments(reader) {
            Ok(segments) => extract_metadata(segments, &mut metadata)?,
            Err(e) => {
                // Log warning but return partial metadata if available
                log::warn!("MKV parsing incomplete: {}", e);
            }
        }

        Ok(metadata)
    }
}
```

### Edge Cases

- **Truncated files:** Return partial metadata when possible
- **Embedded files:** Handle formats within containers (e.g., album art in MP3)
- **Multiple metadata blocks:** Merge tags from different sections
- **Encoding issues:** Handle UTF-8, UTF-16, Latin-1 text properly using `encoding_rs`
- **Huge files:** Use streaming/chunked parsing for large videos

## Testing Strategy

### Three-Tier Testing Approach

**1. Unit Tests (Per-Parser)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flac_header_parsing() {
        let sample = include_bytes!("../../test_data/audio/sample.flac");
        let reader = BufferReader::new(sample);
        let parser = FlacParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(metadata.get("Artist").unwrap(), "Test Artist");
        assert_eq!(metadata.get("FLAC:SampleRate").unwrap(), "44100");
        assert_eq!(metadata.get("FLAC:BitsPerSample").unwrap(), "16");
    }

    #[test]
    fn test_mkv_segment_parsing() {
        let sample = include_bytes!("../../test_data/video/sample.mkv");
        let segments = parse_segments(sample).unwrap();
        assert!(segments.iter().any(|s| s.id == SEGMENT_TAGS));
    }

    #[test]
    fn test_corrupted_flac_handling() {
        let corrupted = b"fLaC\xFF\xFF\xFF\xFF"; // Invalid block header
        let reader = BufferReader::new(corrupted);
        let parser = FlacParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
```

**2. Integration Tests (ExifTool Comparison)**

```rust
// tests/integration/flac_tests.rs
use oxidex::core::MetadataMap;
use std::process::Command;

#[test]
fn test_flac_metadata_parity() {
    let file = "test_data/audio/sample.flac";

    // Run ExifTool via CLI
    let exiftool_output = Command::new("exiftool")
        .arg("-json")
        .arg(file)
        .output()
        .expect("Failed to run exiftool");

    let exiftool_json: serde_json::Value =
        serde_json::from_slice(&exiftool_output.stdout).unwrap();

    // Run OxiDex
    let oxidex_metadata = MetadataMap::from_file(file).unwrap();
    let oxidex_json = serde_json::to_value(&oxidex_metadata).unwrap();

    // Compare critical tags
    let tags_to_compare = [
        "Artist", "Album", "Title", "Genre", "TrackNumber", "Year",
        "FLAC:SampleRate", "FLAC:BitsPerSample", "FLAC:Channels",
    ];

    for tag in &tags_to_compare {
        assert_eq!(
            exiftool_json[0][tag],
            oxidex_json[tag],
            "Mismatch for tag: {}", tag
        );
    }
}

#[test]
fn test_mkv_metadata_parity() {
    compare_with_exiftool(
        "test_data/video/sample.mkv",
        &["Title", "Duration", "Matroska:MuxingApp", "Matroska:DateUTC"]
    );
}
```

**3. Format Detection Tests**

```rust
#[test]
fn test_format_detection_phase1() {
    assert_eq!(detect_format("sample.mkv"), FileFormat::MKV);
    assert_eq!(detect_format("sample.webm"), FileFormat::WEBM);
    assert_eq!(detect_format("sample.flac"), FileFormat::FLAC);
    assert_eq!(detect_format("sample.mp3"), FileFormat::MP3);
    assert_eq!(detect_format("sample.ogg"), FileFormat::OGG);

    // Test magic byte detection (not just extension)
    let mkv_data = b"\x1A\x45\xDF\xA3...";
    assert_eq!(detect_by_magic(mkv_data), Some(FileFormat::MKV));
}
```

### Test Data Corpus

Build comprehensive test corpus per phase:

- **Phase 1:** 50+ video/audio samples
  - Various codecs (H.264, VP9, HEVC for video; various bitrates for audio)
  - Different metadata standards (ID3v1, ID3v2.3, ID3v2.4 for MP3)
  - Edge cases (no metadata, corrupted headers, huge files)

- **Phase 2:** 30+ document samples
  - Different Office versions (2007, 2010, 2013, 2016, 2019, 365)
  - Various languages/encodings
  - Complex documents with embedded objects

- **Phase 3:** 20+ archive samples
- **Phase 4:** 15+ font files
- **Phase 5:** 30+ advanced image formats
- **Phase 6:** 25+ specialized formats

### Fuzzing Integration

Add fuzzing targets for each new parser:

```rust
// fuzz/fuzz_targets/fuzz_mkv.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxidex::parsers::video::MkvParser;
use oxidex::core::FormatParser;
use oxidex::io::BufferReader;

fuzz_target!(|data: &[u8]| {
    let reader = BufferReader::new(data);
    let parser = MkvParser;
    let _ = parser.parse(&reader);
    // Should never panic, even on malicious input
});
```

**Fuzzing Corpus:**
- Seed with valid test files
- Run 1000+ iterations per parser
- Report crashes/hangs to issue tracker

### CI Pipeline Requirements

Per phase, CI must include:

1. **Full test suite** (unit + integration)
2. **ExifTool comparison tests** (100% pass rate required)
3. **Benchmark suite** (performance regression detection)
4. **Fuzzing** (short runs on CI, deep fuzzing locally)
5. **Code coverage** (>90% target)
6. **Clippy lints** (no warnings)

## Performance Optimization

### Performance Goals

Maintain **7-16x performance advantage** over Perl ExifTool:

- **Single file operations:** <20ms per file (vs 100-300ms ExifTool)
- **Batch processing:** Leverage Rayon for parallel processing
- **Memory efficiency:** Minimize allocations, use zero-copy parsing

### Optimization Techniques

**1. Zero-Copy Parsing with `nom`**

```rust
use nom::{
    bytes::complete::{tag, take},
    number::complete::{be_u32, le_u16},
    IResult,
};

// Parse FLAC metadata block header without copying
fn parse_block_header(input: &[u8]) -> IResult<&[u8], BlockHeader> {
    let (input, block_type_and_last) = take(1u8)(input)?;
    let (input, length_bytes) = take(3u8)(input)?;

    let length = u32::from_be_bytes([0, length_bytes[0], length_bytes[1], length_bytes[2]]);

    Ok((input, BlockHeader {
        is_last: (block_type_and_last[0] & 0x80) != 0,
        block_type: block_type_and_last[0] & 0x7F,
        length,
    }))
}
```

**2. Memory-Mapped I/O**

Continue using `memmap2` for large files:

```rust
impl FormatParser for MkvParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // FileReader provides memory-mapped view
        // No need to load entire multi-GB file into RAM

        let header = parse_ebml_header(&reader.read_at(0, 1024)?)?;

        // Find Tags segment offset without reading entire file
        let tags_offset = find_tags_segment(reader)?;

        // Only read metadata section
        let tags_data = reader.read_at(tags_offset, 65536)?; // Cap at 64KB

        extract_tags(&tags_data)
    }
}
```

**3. Lazy Parsing (Skip Media Data)**

Only parse metadata sections:

```rust
fn parse_mkv_segments(reader: &dyn FileReader) -> Result<Vec<Segment>> {
    let mut segments = Vec::new();
    let mut offset = 0;

    while offset < reader.len() {
        let (id, size) = parse_ebml_element_header(reader, offset)?;

        match id {
            SEGMENT_INFO | SEGMENT_TAGS | SEGMENT_TRACKS => {
                // Parse these - contain metadata
                let data = reader.read_at(offset, size)?;
                segments.push(parse_segment(id, &data)?);
            }
            SEGMENT_CLUSTER => {
                // Skip video/audio data entirely
                offset += size;
                continue;
            }
            _ => {
                offset += size;
            }
        }

        offset += size;
    }

    Ok(segments)
}
```

**4. Benchmark Suite per Phase**

```rust
// benches/video_benchmarks.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oxidex::parsers::video::MkvParser;
use oxidex::core::FormatParser;

fn bench_mkv_parsing(c: &mut Criterion) {
    let data = include_bytes!("../test_data/video/sample.mkv");
    let reader = BufferReader::new(data);

    c.bench_function("mkv_parse", |b| {
        b.iter(|| {
            let parser = MkvParser;
            parser.parse(black_box(&reader)).unwrap()
        })
    });
}

fn bench_flac_parsing(c: &mut Criterion) {
    let data = include_bytes!("../test_data/audio/sample.flac");
    let reader = BufferReader::new(data);

    c.bench_function("flac_parse", |b| {
        b.iter(|| {
            let parser = FlacParser;
            parser.parse(black_box(&reader)).unwrap()
        })
    });
}

criterion_group!(benches, bench_mkv_parsing, bench_flac_parsing);
criterion_main!(benches);
```

**5. Profile-Guided Optimization**

For each phase:
1. Run benchmarks to establish baseline
2. Profile with `cargo flamegraph`
3. Optimize hot paths (typically: parsing loops, string allocations)
4. Verify improvement with benchmarks
5. Document performance in release notes

**Target: Each parser should process files in single-digit milliseconds**

## Documentation Requirements

### Code Documentation (rustdoc)

Every parser module must include:

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
//! - `Matroska:Title` → `Title`
//! - `Matroska:Duration` → `Duration`
//! - `Matroska:MuxingApp` → `MuxingApp`
//!
//! # File Structure
//!
//! ```text
//! [EBML Header]
//!   ├─ EBMLVersion
//!   ├─ DocType ("matroska" or "webm")
//!   └─ DocTypeVersion
//! [Segment]
//!   ├─ SeekHead (index)
//!   ├─ Info (duration, dates, muxing app)
//!   ├─ Tracks (video/audio codec info)
//!   ├─ Tags (metadata - THIS IS WHAT WE PARSE)
//!   └─ Clusters (actual media data - SKIP THIS)
//! ```
//!
//! # References
//!
//! - EBML RFC: <https://www.rfc-editor.org/rfc/rfc8794.html>
//! - Matroska Spec: <https://www.matroska.org/technical/elements.html>
//! - ExifTool Source: `lib/Image/ExifTool/Matroska.pm`
//!
//! # Examples
//!
//! ```no_run
//! use oxidex::parsers::video::MkvParser;
//! use oxidex::core::FormatParser;
//! use oxidex::io::MMapReader;
//!
//! let reader = MMapReader::new("video.mkv")?;
//! let parser = MkvParser;
//! let metadata = parser.parse(&reader)?;
//!
//! println!("Title: {}", metadata.get("Title")?);
//! println!("Duration: {}", metadata.get("Matroska:Duration")?);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub struct MkvParser;
```

### User Guide Updates

Add format-specific sections to user guide:

```markdown
## Supported Formats

### Video Formats

#### MKV (Matroska)

**Extensions:** `.mkv`, `.webm`, `.mka`

**Supported Metadata:**
- Title, Artist, Album (from Tags segment)
- Duration, creation date
- Codec information (video/audio)
- Muxing application version

**Example Usage:**
```bash
# Extract all metadata
oxidex video.mkv

# Extract specific Matroska tags
oxidex -Matroska:Title -Matroska:Duration video.mkv

# Batch process directory
oxidex -r -Matroska:all /path/to/videos/
```

**Tag Reference:**
| Tag Name | Description | Example Value |
|----------|-------------|---------------|
| `Matroska:Title` | Video title | "My Video" |
| `Matroska:Duration` | Duration in seconds | "125.5" |
| `Matroska:MuxingApp` | Muxing application | "libmatroska 1.6.3" |
| `Matroska:DateUTC` | Creation timestamp | "2024-01-15 14:30:00" |

### Audio Formats

#### FLAC (Free Lossless Audio Codec)

**Extensions:** `.flac`

**Supported Metadata:**
- Vorbis Comments (ARTIST, ALBUM, TITLE, etc.)
- Stream info (sample rate, bit depth, channels)
- Embedded album artwork (Picture block)
- ReplayGain values

**Example Usage:**
```bash
# Extract all metadata
oxidex audio.flac

# Extract specific FLAC/Vorbis tags
oxidex -FLAC:Artist -FLAC:Album -FLAC:SampleRate audio.flac

# Export album art
oxidex -b -Picture audio.flac > cover.jpg
```

**Tag Reference:**
| Tag Name | Description | Example Value |
|----------|-------------|---------------|
| `FLAC:Artist` | Artist name (Vorbis) | "Pink Floyd" |
| `FLAC:Album` | Album title | "Dark Side of the Moon" |
| `FLAC:SampleRate` | Sample rate (Hz) | "44100" |
| `FLAC:BitsPerSample` | Bit depth | "16" |
| `FLAC:Channels` | Channel count | "2" (stereo) |
```

### Migration Guide (ExifTool Perl → OxiDex)

```markdown
## Command Equivalents

| ExifTool Perl | OxiDex | Notes |
|---------------|--------|-------|
| `exiftool video.mkv` | `oxidex video.mkv` | Drop-in replacement |
| `exiftool -Matroska:Title video.mkv` | `oxidex -Matroska:Title video.mkv` | Tag names identical |
| `exiftool -json *.flac` | `oxidex -json *.flac` | JSON format matches |
| `exiftool -r /videos/` | `oxidex -r /videos/` | Recursive processing |
| `exiftool -csv -r /music/ > out.csv` | `oxidex -csv -r /music/ > out.csv` | CSV export |

## Tag Name Compatibility

OxiDex maintains 100% tag name compatibility with ExifTool:

- `FLAC:Artist` → `FLAC:Artist` (identical)
- `Matroska:Duration` → `Matroska:Duration` (identical)
- `ID3:Title` → `ID3:Title` (identical)

Scripts and tools using ExifTool tag names work without modification.
```

## Phase Deliverables Checklist

Each phase is complete when it includes:

- [ ] All format parsers implemented
- [ ] `FileFormat` enum variants added
- [ ] Magic byte detection for all formats
- [ ] Unit tests with >90% code coverage
- [ ] Integration tests with 100% ExifTool parity
- [ ] Benchmarks meeting performance targets (<20ms per file)
- [ ] Fuzzing targets added for all parsers
- [ ] API documentation (rustdoc) complete
- [ ] User guide updated with format examples
- [ ] CHANGELOG entry written
- [ ] Git tag created (e.g., `v1.1.0-phase1`)
- [ ] Release notes published
- [ ] Pre-built binaries for all platforms

## Release Strategy

### Versioning

- **Minor version bump** per phase: v1.1.0, v1.2.0, v1.3.0, etc.
- **Patch versions** for bug fixes within a phase
- **Major version** (v2.0.0) after Phase 6 completion

### Release Notes Template

```markdown
# OxiDex v1.1.0 - Phase 1: Video/Audio Support

**Release Date:** 2025-XX-XX

## New Format Support

Added comprehensive metadata extraction for **12 new formats**:

**Video Formats:**
- ✅ **MKV (Matroska)** - EBML-based container (.mkv, .webm, .mka)
- ✅ **FLV (Flash Video)** - Adobe Flash video format
- ✅ **AVI** - Audio Video Interleave (RIFF-based)
- ✅ **MTS** - MPEG Transport Stream (AVCHD camcorders)

**Audio Formats:**
- ✅ **MP3** - ID3v1, ID3v2.3, ID3v2.4 support
- ✅ **FLAC** - Vorbis Comments + embedded artwork
- ✅ **AAC** - MPEG-4 Audio metadata
- ✅ **WAV** - RIFF INFO chunks
- ✅ **OGG Vorbis** - Vorbis Comment metadata
- ✅ **Opus** - Opus tags
- ✅ **APE** - Monkey's Audio APEv2 tags

## Performance Benchmarks

All formats significantly faster than Perl ExifTool:

| Format | OxiDex | ExifTool | Speedup |
|--------|--------|----------|---------|
| MKV | 12ms | 145ms | **12.1x** |
| FLAC | 8ms | 95ms | **11.9x** |
| MP3 | 6ms | 88ms | **14.7x** |
| OGG | 7ms | 92ms | **13.1x** |
| AVI | 10ms | 128ms | **12.8x** |

**Batch Processing (1000 files):**
- OxiDex: 2.1s
- ExifTool: 18.4s
- **Speedup: 8.8x**

## Tag Database

- **Total tags:** 34,127 (added 1,450 video/audio tags)
- **ExifTool parity:** 118% (34,127 / 28,853)

## Breaking Changes

None - backward compatible with v1.0.x

## Installation

```bash
# From crates.io
cargo install oxidex

# From pre-built binaries (see GitHub Releases)
# Linux, macOS, Windows binaries available
```

## Documentation

- [User Guide - Video Formats](https://exiftool-rs.github.io/exiftool-rs/formats/video/)
- [User Guide - Audio Formats](https://exiftool-rs.github.io/exiftool-rs/formats/audio/)
- [API Documentation](https://docs.rs/oxidex/1.1.0)

## Contributors

Thank you to all contributors who helped with this release!

---

**Full Changelog:** https://github.com/swack-tools/oxidex/compare/v1.0.0...v1.1.0
```

## Timeline Summary

| Phase | Duration | Formats | Tags Added | Release |
|-------|----------|---------|------------|---------|
| Phase 1: Video/Audio | 3-4 months | 12 | ~1,450 | v1.1.0 |
| Phase 2: Documents | 2-3 months | 8 | ~800 | v1.2.0 |
| Phase 3: Archives | 1-2 months | 6 | ~200 | v1.3.0 |
| Phase 4: Fonts | 1 month | 4 | ~150 | v1.4.0 |
| Phase 5: Advanced Images | 2-3 months | 8 | ~600 | v1.5.0 |
| Phase 6: Specialized | 2-3 months | 10 | ~500 | v1.6.0 |
| **Total** | **12-18 months** | **48** | **~3,700** | **v2.0.0** |

**Final State:**
- **200+ total format variants** in `FileFormat` enum
- **36,000+ metadata tags** (125%+ ExifTool parity)
- **Comprehensive format coverage** matching ExifTool
- **Maintained performance advantage** (7-16x faster)

## Success Criteria

The implementation is successful when:

1. ✅ All 48 parsers implemented and tested
2. ✅ 100% ExifTool parity for all supported formats
3. ✅ Performance targets met (<20ms per file)
4. ✅ All integration tests passing (ExifTool comparison)
5. ✅ Fuzzing finds no crashes/hangs
6. ✅ Documentation complete (API + user guide)
7. ✅ Pre-built binaries for all platforms
8. ✅ Community adoption (measured by downloads, GitHub stars)

## Risks & Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| ExifTool updates during dev | Tag drift | Automated tag sync in build.rs |
| Parser complexity underestimated | Timeline slip | Start with simpler formats, buffer 20% time |
| Performance regression | User dissatisfaction | Mandatory benchmarks in CI, block on regression |
| Test data licensing | Legal issues | Use Creative Commons/public domain samples |
| Fuzzing finds security issues | Delayed releases | Fix immediately, add regression tests |

## Future Considerations

**Post-Phase 6:**
- WebAssembly builds for browser usage
- Python bindings (PyO3)
- Node.js bindings (napi-rs)
- GUI application (Tauri)
- Cloud service integration

---

**Document Status:** Approved for implementation
**Next Steps:** Create detailed implementation plan for Phase 1 using `superpowers:writing-plans`
