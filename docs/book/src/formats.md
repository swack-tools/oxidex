# Supported Formats

This chapter lists the file formats and metadata standards currently supported by OxiDex.

## Implementation Status

OxiDex is actively being developed. The current implementation (v0.1.0) focuses on core formats with comprehensive metadata support. Additional formats will be added in future releases.

**Legend:**
- ✅ **Fully Implemented**: Read and write support with comprehensive tag coverage
- 🔄 **Partial Support**: Read support implemented, write support in progress or limited
- ⏳ **Planned**: On the roadmap for future implementation
- ❌ **Not Supported**: Not currently planned

## Image Formats

### JPEG (Joint Photographic Experts Group)

**Status**: ✅ Fully Implemented

**File Extensions**: `.jpg`, `.jpeg`, `.jpe`

**Metadata Types Supported:**
- ✅ EXIF (Exchangeable Image File Format)
- ✅ XMP (Extensible Metadata Platform)
- ✅ IPTC (International Press Telecommunications Council)
- ✅ JFIF (JPEG File Interchange Format)
- ✅ GPS (Geolocation data)
- ✅ ICC Profile (Color management)
- ✅ Photoshop metadata
- ✅ Thumbnail extraction

**Read Operations**: ✅ Full support
**Write Operations**: ✅ Full support

**Available Tags**: 244 EXIF tags + 122 IPTC tags + XMP support

**Common Use Cases:**
- Digital camera photos
- Web images
- Social media uploads
- Scanned documents

### TIFF (Tagged Image File Format)

**Status**: ✅ Fully Implemented

**File Extensions**: `.tif`, `.tiff`

**Metadata Types Supported:**
- ✅ EXIF
- ✅ XMP
- ✅ IPTC
- ✅ GPS
- ✅ ICC Profile
- ✅ Photoshop
- ✅ Multi-page/multi-image support

**Read Operations**: ✅ Full support
**Write Operations**: ✅ Full support

**Available Tags**: 244 EXIF tags + additional TIFF-specific tags

**Common Use Cases:**
- Professional photography
- Archival images
- Scientific imaging
- Medical imaging (DICOM-TIFF)

### PNG (Portable Network Graphics)

**Status**: ✅ Fully Implemented

**File Extensions**: `.png`

**Metadata Types Supported:**
- ✅ PNG text chunks (tEXt, zTXt, iTXt)
- ✅ XMP
- ✅ EXIF (embedded via PNG chunks)
- ✅ ICC Profile
- ✅ Creation time (tIME chunk)
- ✅ Physical dimensions (pHYs chunk)

**Read Operations**: ✅ Full support
**Write Operations**: ✅ Full support

**Available Tags**: 30 PNG-specific tags + EXIF/XMP support

**Common Use Cases:**
- Web graphics
- Screenshots
- Lossless image archiving
- Images with transparency

## Document Formats

### PDF (Portable Document Format)

**Status**: 🔄 Partial Support

**File Extensions**: `.pdf`

**Metadata Types Supported:**
- ✅ PDF Info Dictionary (Title, Author, Subject, Keywords, Creator, Producer)
- ✅ Creation/Modification dates
- ✅ XMP metadata packets
- 🔄 Embedded image metadata (read-only)

**Read Operations**: ✅ Supported
**Write Operations**: ⏳ Planned for future release

**Available Tags**: PDF Info keys + XMP support

**Common Use Cases:**
- Document metadata extraction
- PDF library management
- Compliance and archiving

## Video Formats

### MP4/QuickTime

**Status**: ✅ Fully Implemented

**File Extensions**: `.mp4`, `.m4v`, `.mov`, `.3gp`, `.3g2`

**Metadata Types Supported:**
- ✅ QuickTime atoms (moov, udta, meta)
- ✅ Creation/modification times
- ✅ Duration, dimensions
- ✅ GPS coordinates (from video metadata)
- ✅ Camera make/model
- ✅ XMP packets

**Read Operations**: ✅ Full support
**Write Operations**: ⏳ Planned for future release

**Available Tags**: 143 QuickTime-specific tags

**Common Use Cases:**
- Video library management
- Smartphone video metadata
- Media asset databases
- GPS-tagged videos

## Metadata Standards

### EXIF (Exchangeable Image File Format)

**Status**: ✅ Comprehensive Support

**Supported in Formats**: JPEG, TIFF, PNG (embedded), RAW formats

**Tag Categories:**
- Image structure (width, height, color space)
- Camera settings (ISO, aperture, shutter speed, focal length)
- Camera identification (make, model, serial number)
- Date/time stamps (original, digitized, modified)
- Image processing (white balance, exposure compensation, flash)
- Thumbnail images
- Copyright and author information

**Available Tags**: 244 tags from ExifTool spec

**Standards Compliance**: EXIF 2.3 specification

### XMP (Extensible Metadata Platform)

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF, PNG, PDF, MP4

**XMP Namespaces Supported:**
- `dc` (Dublin Core): Title, Creator, Rights, Description
- `xmp`: Base XMP properties
- `xmpRights`: Copyright management
- `photoshop`: Adobe Photoshop metadata
- `exif`: EXIF properties in XMP format
- `tiff`: TIFF properties in XMP format
- `aux`: Additional camera metadata

**Read Operations**: ✅ Full XML parsing
**Write Operations**: ✅ Full XML serialization

**Available Tags**: 7 base XMP tags + namespace-specific tags

### IPTC (International Press Telecommunications Council)

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF

**IPTC Categories:**
- Descriptive metadata (Caption, Keywords, Headline)
- Administrative metadata (Credit, Source, Copyright Notice)
- People and locations (City, Province, Country, Creator)
- Rights information (Usage Terms, Copyright Notice)
- Technical metadata (Date Created, Digital Creation Date)

**Read Operations**: ✅ Full support
**Write Operations**: ✅ Full support

**Available Tags**: 122 IPTC tags

**Standards Compliance**: IPTC Core 1.3, IPTC Extension

### GPS Metadata

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF, MP4/MOV

**GPS Tags Supported:**
- Latitude, Longitude (decimal degrees)
- Altitude (meters above sea level)
- Timestamp (UTC)
- Speed, Track (direction of movement)
- Satellites used, DOP (dilution of precision)
- Map Datum (coordinate system)
- Differential correction

**Available Tags**: 32 GPS-specific tags

**Coordinate Formats**: Decimal degrees, degrees/minutes/seconds (DMS)

## Additional Metadata Families

### ICC Profile (Color Management)

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF, PNG

**Profile Information:**
- Profile description
- Color space (RGB, CMYK, Lab)
- Rendering intent
- White point, primaries
- Gamma/transfer curve

**Available Tags**: 42 ICC Profile tags

### Photoshop Metadata

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF, PNG

**Photoshop Resources:**
- Image resources (layers, paths)
- Copyright flag
- URL
- Credit, Source
- Caption Writer

**Available Tags**: 35 Photoshop-specific tags

### RIFF (Resource Interchange File Format)

**Status**: 🔄 Parser Implemented

**Supported in Formats**: AVI, WAV (planned)

**Available Tags**: 46 RIFF tags

**Status**: Tag database generated, parser infrastructure ready

## Tag Database Statistics

OxiDex automatically generates its tag database from the official ExifTool source during build:

**Current Tag Count**: 731 tags (v0.1.0)

**Tags by Format Family:**
- EXIF: 244 tags
- QuickTime: 143 tags
- IPTC: 122 tags
- RIFF: 46 tags
- ICC_Profile: 42 tags
- Photoshop: 35 tags
- GPS: 32 tags
- PNG: 30 tags
- JPEG: 30 tags
- XMP: 7 tags (base module)

**Total Coverage**: ~700+ unique metadata tags across 10+ format families

## Planned Format Support

The following formats are on the roadmap for future releases:

### High Priority

- **RAW Formats** (⏳ Planned):
  - CR2, CR3 (Canon)
  - NEF (Nikon)
  - ARW (Sony)
  - DNG (Adobe Digital Negative)
  - ORF (Olympus)
  - RAF (Fujifilm)

- **Additional Video Formats** (⏳ Planned):
  - AVI (Audio Video Interleave)
  - MKV (Matroska)
  - WebM

### Medium Priority

- **Document Formats** (⏳ Planned):
  - DOCX (Microsoft Word)
  - XLSX (Microsoft Excel)
  - PPTX (Microsoft PowerPoint)
  - ODT, ODS, ODP (OpenDocument)

- **Audio Formats** (⏳ Planned):
  - MP3 (ID3 tags)
  - FLAC
  - M4A/AAC
  - WAV (RIFF metadata)
  - OGG Vorbis

### Lower Priority

- **Archive Formats** (⏳ Planned):
  - ZIP (embedded metadata)
  - 7z

- **Vector Formats** (⏳ Planned):
  - SVG (XML metadata)
  - AI (Adobe Illustrator)
  - EPS (Encapsulated PostScript)

## Checking Format Support

To check if a file format is supported, use the CLI:

```bash
oxidex photo.unknown
```

If the format is not supported, you'll see:

```
Error: Unsupported file format: unknown
```

Supported formats will display metadata or indicate no metadata was found.

## Format Detection

OxiDex provides two file format detection methods: a fast **signature-based detector** (default) and an optional **AI-powered detector** using Google's Magika. Choose the detection method that best fits your use case.

### Detection Methods Comparison

| Feature | Signature-Based (Default) | Magika AI (Optional) |
|---------|---------------------------|----------------------|
| **Speed** | <1μs per file | ~5ms per file |
| **Accuracy** | 95-98% for common formats | ~99% across all formats |
| **Formats Supported** | 50+ common formats | 200+ formats |
| **Binary Size** | No overhead | +5MB (model file) |
| **Build Flag** | Default (always enabled) | `--features magika` |
| **Use Case** | Fast, common formats | Maximum accuracy, rare formats |

### Signature-Based Detection (Default)

**How It Works:**

OxiDex's default detection engine uses **magic byte signatures** - unique byte patterns at specific file offsets that identify file formats. This traditional approach is:

- **Lightning fast**: <1 microsecond per file (simple byte comparison)
- **Zero overhead**: No model loading or dependencies
- **Reliable**: Handles 50+ common formats with high accuracy
- **Tested**: Industry-standard approach used for decades

**Detection Process:**

1. **Read header bytes**: Extracts first 600 bytes of the file
2. **Match signatures**: Compares against signature table (see below)
3. **Specialized detection**: For complex formats, uses advanced heuristics:
   - **TIFF variants**: Identifies Canon CR2/CRW, Panasonic RW2, Olympus ORF
   - **ISO Base Media**: Detects Canon CR3, AVIF, HEIF via `ftyp` brand codes
   - **RIFF containers**: Distinguishes WAV, AVI, WebP by checking container headers
   - **ZIP-based documents**: Opens ZIP and checks for marker files (EPUB, DOCX, XLSX, etc.)
   - **Sync patterns**: Detects MP3, AAC, MTS via MPEG sync frames
4. **Extension fallback**: For TIFF-based raw formats (NEF, ARW, DNG), checks file extension

**Supported Signatures:**

| Format | Magic Bytes | Offset | Notes |
|--------|-------------|--------|-------|
| JPEG | `FF D8 FF` | 0 | JPEG SOI marker |
| PNG | `89 50 4E 47 0D 0A 1A 0A` | 0 | PNG signature |
| TIFF (LE) | `49 49 2A 00` | 0 | "II" + little-endian marker |
| TIFF (BE) | `4D 4D 00 2A` | 0 | "MM" + big-endian marker |
| PDF | `25 50 44 46` (`%PDF`) | 0 | PDF version header |
| MP4/MOV | `66 74 79 70` (`ftyp`) | 4 | ISO BMFF signature |
| GIF | `47 49 46 38 37 61` / `47 49 46 38 39 61` | 0 | GIF87a / GIF89a |
| WebP | `52 49 46 46 xx xx xx xx 57 45 42 50` | 0 | RIFF + WEBP |
| FLAC | `66 4C 61 43` (`fLaC`) | 0 | FLAC stream marker |
| ZIP | `50 4B 03 04` / `50 4B 05 06` | 0 | PK ZIP signature |
| RAR | `52 61 72 21 1A 07` | 0 | RAR archive marker |
| 7z | `37 7A BC AF 27 1C` | 0 | 7-Zip signature |
| GZIP | `1F 8B` | 0 | GZIP compressed data |
| BMP | `42 4D` (`BM`) | 0 | Bitmap image file |
| ICO | `00 00 01 00` | 0 | Icon file |
| EXE/DLL | `4D 5A` (`MZ`) | 0 | DOS/Windows executable |
| ELF | `7F 45 4C 46` | 0 | Unix/Linux executable |
| Mach-O | `CE FA ED FE` / `CF FA ED FE` | 0 | macOS executable |

*Plus 30+ additional signatures for video, audio, document, and camera raw formats.*

**Location in Source Code:**

- **Core detector**: `src/parsers/format_detector.rs:680` - `detect_format()` function
- **Signature table**: `src/parsers/format_detector.rs` - `SIMPLE_SIGNATURES` static array
- **Specialized detectors**: Individual functions for complex formats (TIFF, BMFF, RIFF, ZIP)
- **Camera raw detection**: `src/parsers/raw/format_detection.rs:157` - `detect_raw_format()`

**Usage (Default):**

```bash
# Signature-based detection is used automatically
oxidex photo.jpg

# Or explicitly specify signature mode
oxidex --detector=signature photo.jpg
```

### Magika AI-Powered Detection (Optional)

**What is Magika?**

[Magika](https://github.com/google/magika) is Google's deep learning system for file type identification, used in production at Gmail, Google Drive, and Safe Browsing to process "hundreds of billions of samples weekly." OxiDex integrates Magika as an optional enhancement for maximum detection accuracy.

**Key Benefits:**

- **Superior accuracy**: ~99% precision/recall across 200+ file formats
- **Broader coverage**: 5× more formats than signature-based (200+ vs 50+)
- **Text format excellence**: Accurately identifies source code, scripts, configs, data files
- **Context-aware**: Uses deep learning to understand file semantics, not just byte patterns
- **Battle-tested**: Proven at Google scale with billions of files

**How It Works:**

1. **Sampling**: Extracts 3×512 bytes from start, middle, and end of file
2. **Neural network inference**: Runs samples through optimized ONNX model (~5MB)
3. **Classification**: Outputs content type label with confidence score
4. **Format mapping**: Maps Magika label to OxiDex's `FileFormat` enum

**Performance Characteristics:**

- **Cold start**: ~100ms (one-time model loading)
- **Warm inference**: ~5ms per file
- **Throughput**: 1000 files/sec on modern hardware
- **Memory**: ~50MB (model + runtime)
- **Threading**: Thread-safe, reusable session

**Enabling Magika:**

**1. Build with Magika support:**

```bash
# Build with Magika feature
cargo build --release --features magika

# Or install with Magika
cargo install oxidex --features magika
```

**2. Use Magika detection:**

```bash
# AI-powered detection
oxidex --detector=magika photo.jpg

# For a directory of files
oxidex --detector=magika -r /path/to/photos/
```

**When to Use Magika:**

✅ **Use Magika when:**
- Processing files with unknown or untrusted extensions
- Working with uncommon or proprietary formats
- Need to identify source code, scripts, or text files accurately
- Maximum detection accuracy is critical
- Processing batches where 5ms/file overhead is acceptable

❌ **Use signature-based when:**
- Processing common formats (JPEG, PNG, PDF, MP4)
- Speed is critical (<1μs vs 5ms matters)
- Working in memory-constrained environments
- Building minimal/lightweight binaries

**Error Handling:**

If you request Magika without building with `--features magika`:

```bash
$ oxidex --detector=magika photo.jpg
Error: Magika AI detection not available (build with --features magika)
```

**Location in Source Code:**

- **Magika integration**: `src/parsers/magika_detector.rs` (feature-gated)
- **CLI integration**: `src/main.rs` - detector mode selection
- **Detector enum**: `src/parsers/mod.rs` - `DetectorMode::Signature` vs `DetectorMode::Magika`

### Technical Implementation

**Detection Architecture:**

```
┌─────────────┐
│  User File  │
└──────┬──────┘
       │
       ├─────────────────────────────────────┐
       │                                     │
       ▼                                     ▼
┌──────────────────┐              ┌──────────────────┐
│ Signature-Based  │              │  Magika AI       │
│ (Default)        │              │  (Optional)      │
├──────────────────┤              ├──────────────────┤
│ • Read 600 bytes │              │ • Extract samples│
│ • Match table    │              │ • Load model     │
│ • Return format  │              │ • Run inference  │
└────────┬─────────┘              └────────┬─────────┘
         │                                 │
         └──────────────┬──────────────────┘
                        ▼
                 ┌──────────────┐
                 │  FileFormat  │
                 │  Enum        │
                 └──────────────┘
```

**Format Enum:**

Both detectors return the same `FileFormat` enum, ensuring consistent downstream processing regardless of detection method:

```rust
pub enum FileFormat {
    JPEG,
    TIFF,
    PNG,
    PDF,
    MP4,
    // ... 50+ formats
    CameraRaw(RawFormat), // 40+ camera raw formats
    Unknown,
}
```

### Performance Comparison

**Benchmark Results** (MacBook M4, 2025):

| Operation | Signature-Based | Magika AI | Speedup |
|-----------|----------------|-----------|---------|
| Single file (cold) | 0.5μs | 105ms | 210,000× slower |
| Single file (warm) | 0.5μs | 5ms | 10,000× slower |
| Batch 100 files | 50μs | 500ms | 10,000× slower |
| Batch 1000 files | 500μs | 5s | 10,000× slower |

**Trade-off Analysis:**

- **Signature**: Optimized for speed, handles common formats excellently
- **Magika**: Optimized for accuracy, handles rare/complex formats better
- **Hybrid future**: Could use signature for fast-path, Magika for unknowns

**When Speed Matters:**

For large-scale batch processing where every millisecond counts, signature-based detection is recommended. Processing 10,000 files:
- Signature: ~5 milliseconds total
- Magika: ~50 seconds total

### Format Support Matrix

**Both Methods Support:**

Core image formats (JPEG, PNG, TIFF, GIF, BMP, WebP, HEIF, AVIF), video formats (MP4, MOV, MKV, AVI, WebM), audio formats (MP3, FLAC, AAC, WAV, OGG), documents (PDF), and camera raw formats (Canon CR2/CR3, Nikon NEF, Sony ARW, etc.)

**Magika Exclusive:**

200+ additional formats including:
- **Source code**: Python, JavaScript, TypeScript, Rust, Go, Java, C/C++, Ruby, PHP, etc.
- **Config files**: YAML, TOML, JSON, XML, INI, .env files
- **Scripts**: Bash, PowerShell, Batch, Lua, Perl
- **Data formats**: CSV, TSV, Parquet, Protobuf
- **Specialized**: CAD files, scientific data formats, proprietary formats

### Troubleshooting

**Q: Which detector should I use?**

A: Start with signature-based (default). Only enable Magika if you need broader format coverage or higher accuracy for uncommon formats.

**Q: Can I use both detectors?**

A: Not simultaneously, but you can build with `--features magika` and choose at runtime with `--detector=signature` or `--detector=magika`.

**Q: Magika detection is slow. How can I speed it up?**

A: Magika's first detection (~100ms) loads the model. Subsequent detections are much faster (~5ms). For batch processing, the model loading overhead is amortized across all files.

**Q: Does Magika work offline?**

A: Yes! The Magika model is bundled in the binary when you build with `--features magika`. No network connection required.

**Q: What happens if Magika can't identify a file?**

A: Magika returns a content type label with a confidence score. Low-confidence results are mapped to `FileFormat::Unknown`, and OxiDex will report the file as unsupported.

### References

- **Magika Project**: [github.com/google/magika](https://github.com/google/magika)
- **Magika Announcement**: [Google Open Source Blog](https://opensource.googleblog.com/2025/11/announcing-magika-10-now-faster-smarter.html)
- **Integration Plan**: `docs/plans/archive/2025-11-19-magika-integration-plan-COMPLETED.md`
- **Source Code**: `src/parsers/format_detector.rs` (signature), `src/parsers/magika_detector.rs` (Magika)

## Performance by Format

**Relative Performance** (compared to baseline JPEG parsing):

| Format | Read Speed | Write Speed | Notes |
|--------|-----------|-------------|-------|
| JPEG | 1.0x (baseline) | 1.0x | Optimized segment parsing |
| TIFF | 0.9x | 0.9x | IFD chain traversal |
| PNG | 1.1x | 1.1x | Simple chunk-based format |
| PDF | 0.5x | N/A | Complex object parsing |
| MP4/MOV | 0.7x | N/A | Atom tree traversal |

All formats process typical files in < 10ms on modern hardware.

## Contributing New Format Support

Interested in adding support for a new format? See the [Contributing Guide](https://github.com/oxidex/oxidex/blob/main/CONTRIBUTING.md) for:

- Parser implementation guidelines
- Format-specific testing requirements
- Tag database integration
- Documentation standards

## Additional Resources

- **[ExifTool Format Support](https://exiftool.org/#supported)**: Official ExifTool format list (300+ formats)
- **[Command-Line Usage](cli_usage.md)**: How to use the CLI with different formats
- **[Library API](library_api.md)**: Programmatic format detection and parsing
- **[Installation](installation.md)**: Tag database generation from ExifTool source

## Future Roadmap

**v1.0 Goal**: 10+ core formats with full read/write support (DONE)
**v2.0 Goal**: 50+ formats including RAW, video, audio
**v3.0 Goal**: 200+ formats approaching ExifTool feature parity
**Long-term**: 300+ formats with 28,000+ tags (full ExifTool compatibility)

We welcome contributions to accelerate format support development!
