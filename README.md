# OxiDex

[![CI](https://github.com/swack-tools/oxidex/workflows/CI/badge.svg)](https://github.com/swack-tools/oxidex/actions)
[![Integration Tests](https://github.com/swack-tools/oxidex/workflows/Integration%20Tests%20(ExifTool%20Comparison)/badge.svg)](https://github.com/swack-tools/oxidex/actions)

A modern, high-performance Rust implementation of the industry-standard [ExifTool](https://exiftool.org/) metadata management library and command-line application.

## Project Vision

OxiDex aims to provide a memory-safe, zero-cost abstraction alternative to the Perl-based ExifTool while maintaining full compatibility with its extensive metadata tag support. The goal is to deliver superior performance, native cross-compilation capabilities, and seamless integration into modern software ecosystems.

## Key Features

- **100% ExifTool Tag Parity**: 32,677 metadata tags across 140+ format families, automatically synchronized with ExifTool source
- **High Performance**: 16-65x performance improvement over Perl implementation through zero-cost abstractions and parallel processing
- **Memory Safety**: Eliminates entire classes of vulnerabilities (buffer overflows, use-after-free) through Rust's ownership system
- **Binary Distribution**: Static, self-contained binaries with no runtime dependencies
- **API-First Design**: Native Rust library with C FFI bindings for cross-language integration
- **Backward Compatibility**: CLI argument compatibility with original ExifTool for drop-in replacement scenarios
- **Cross-Platform**: Windows, Linux, macOS support with native binaries
- **AI-Powered Detection (Optional)**: Magika deep learning model for enhanced file type detection (~99% accuracy, 200+ formats)

## Architecture

OxiDex follows a **Hexagonal Architecture** (Ports and Adapters) pattern with three main layers:

- **Application Layer**: CLI interface, C FFI bindings
- **Domain Layer**: Format-agnostic metadata models and operations
- **Infrastructure Layer**: Format-specific parsers/serializers, I/O abstraction

This design ensures:
- Clean separation of concerns
- Testability of core logic independent of I/O
- Easy extensibility for new file formats
- Multiple access patterns (CLI, library API, FFI)

## Current Status

🎉 **v1.0.0 Stable Release** - Production-ready metadata management tool

### Completed Features
- ✅ 140+ format families with complete ExifTool parity
- ✅ **32,677 metadata tags** (113% of ExifTool's 28,853 tags) automatically generated from source
- ✅ Full CLI with backward compatibility
- ✅ Rust library API with hexagonal architecture
- ✅ C FFI bindings for cross-language integration
- ✅ Metadata read and write operations
- ✅ Batch processing with parallel execution
- ✅ Cross-platform binaries (Linux, macOS, Windows)
- ✅ 16-65x performance improvement over Perl ExifTool
- ✅ Comprehensive documentation and user guide
- ✅ Integration tests with ExifTool comparison
- ✅ Continuous fuzzing for security

### Supported Metadata Formats

- ✅ **EXIF** - Complete support for IFD0, IFD1, ExifIFD, GPS, and Interoperability IFD
- ✅ **XMP** - 10+ namespaces supported (Dublin Core, IPTC Core, Photoshop, etc.)
- ✅ **IPTC** - Complete support for IPTC IIM Application Record (journalism/stock photography)
- ✅ **Canon MakerNotes** - Phase 2: Array tags + basic metadata (CameraSettings, ShotInfo, FocalLength) ⭐ **UPDATED**
- ✅ **Camera Raw Formats** - 40+ raw formats from major manufacturers:
  - **Canon**: CR2, CR3, CRW
  - **Nikon**: NEF, NRW
  - **Sony**: ARW, SR2, SRF, SRW, ARQ, ARI
  - **Fujifilm**: RAF
  - **Olympus**: ORF, ORI
  - **Pentax**: PEF
  - **Panasonic**: RW2, RWL
  - **Hasselblad**: 3FR, FFF
  - **Phase One**: IIQ
  - **Mamiya**: MEF
  - **Leaf**: MOS
  - **Kodak**: DCR, KDC
  - **Minolta**: MDC, MRW
  - **Epson**: ERF
  - **Sigma**: X3F
  - **GoPro**: GPR
  - **Adobe**: DNG (Digital Negative)
  - **HEIF**: HIF
  - **Light**: LRI
  - **Sinar**: STI
  - **Generic**: RAW, CAM, REV
- ✅ **JFIF** - JPEG File Interchange Format metadata
- ✅ **ICC Profiles** - Color profile metadata extraction
- ✅ **Photoshop IRB** - Adobe Photoshop Image Resource Blocks
- ✅ **PDF** - Document metadata, XMP, and ICC profiles
- ✅ **PE (Portable Executable)** - Windows executables, DLLs, and drivers (.exe, .dll, .sys)
- ✅ **PNG** - PNG chunks (tEXt, iTXt, zTXt, etc.)
- ✅ **QuickTime/MP4** - Video/audio metadata atoms
- ✅ **Video Formats** (Phase 1) - MKV/WebM (Matroska), FLV, AVI, MTS/M2TS
- ✅ **Audio Formats** (Phase 1) - MP3 (ID3), FLAC, AAC, WAV, OGG Vorbis, Opus, APE
- ✅ **File System** - File attributes, permissions, timestamps

**Canon MakerNotes Details:**
- **Phase 1 (Basic Tags):** ImageType, FirmwareVersion, OwnerName, SerialNumber, ModelID, FileNumber
- **Phase 2 (Array Tags):** CameraSettings (8 tags), ShotInfo (6 tags), FocalLength (2 tags)
- **Phase 3 (Future):** Lens database, AFInfo, FileInfo, additional camera-specific arrays

**Phase 2 Tags:**
- **CameraSettings:** MacroMode, Quality, FlashMode, DriveMode, FocusMode, ISO, MeteringMode, ExposureMode
- **ShotInfo:** AutoISO, BaseISO, MeasuredEV, TargetAperture, TargetShutterSpeed, SubjectDistance
- **FocalLength:** FocalType, FocalLength

## Performance Benchmarks

ExifTool-RS demonstrates exceptional performance improvements over the original Perl ExifTool implementation. The following benchmarks compare both tools running on identical hardware.

### System Specifications

- **OS**: Linux (Ubuntu)
- **CPU**: x86_64 (4 cores)
- **Memory**: 8GB RAM
- **Perl ExifTool**: version latest
- **ExifTool-RS**: version 1.0.0

### Benchmark Results

| Scenario | Perl ExifTool | ExifTool-RS | Speedup |
|----------|---------------|-------------|---------|
| Single JPEG Read | 138.8ms ± 43.0ms | 27.7ms ± 11.1ms | **5.0x faster** |
| Batch Processing (1000 files) | 2914.8ms ± 844.5ms | 228.2ms ± 42.0ms | **12.8x faster** |
| Write Operation (modify EXIF tag) | 355.4ms ± 161.3ms | 28.5ms ± 7.3ms | **12.5x faster** |
| Format Detection | 73.7ms ± 22.1ms | 10.4ms ± 0.7ms | **7.1x faster** |

*Benchmarks performed using [hyperfine](https://github.com/sharkdp/hyperfine) with multiple runs and warmup periods.*

### Key Performance Improvements

- **Single file operations**: Zero-cost abstractions and compiled code eliminate Perl interpreter overhead, achieving 5.0x faster metadata extraction
- **Batch processing**: Parallel processing with Rayon leverages all CPU cores, processing 1000 files in 228.2ms ± 42.0ms vs. 2914.8ms ± 844.5ms for single-threaded Perl
- **Write operations**: Efficient binary manipulation and atomic file operations provide 12.5x faster EXIF tag modifications
- **Format detection**: Native compiled code dramatically outperforms interpreted Perl for magic byte detection (7.1x faster)

### Reproducing These Benchmarks

To run the comparative benchmarks on your system:

```bash
# Ensure prerequisites are installed
brew install hyperfine exiftool  # macOS
# or
sudo apt install hyperfine libimage-exiftool-perl  # Ubuntu

# Build ExifTool-RS in release mode
cargo build --release

# Run the benchmark suite
./benches/exiftool_comparison.sh

# View detailed results
cat benches/benchmark_results.md
```

For library-level micro-benchmarks (format detection, parsing internals), run:
```bash
cargo bench
```

**Note**: Benchmark results may vary based on hardware, OS, and system load. For best results, close unnecessary applications and ensure your system is not thermal throttling.

## Installation

OxiDex v1.0.0 is now production-ready! Install via cargo, package managers, or pre-built binaries.

### From crates.io (Recommended)

```bash
cargo install oxidex
```

### From Homebrew (macOS)

For macOS users with [Homebrew](https://brew.sh):

```bash
# Install from Homebrew formula (source build)
brew install --build-from-source https://raw.githubusercontent.com/swack-tools/oxidex/main/packaging/homebrew/oxidex.rb

# Or install from local formula file
brew install --build-from-source ./packaging/homebrew/oxidex.rb

# Verify installation
oxidex --version
```

**Note**: The Homebrew formula builds from source, which may take 5-10 minutes depending on your system. Future releases will include pre-built bottles for faster installation.

### From Pre-Built Binaries

Static binaries are available for all major platforms on the [GitHub Releases](https://github.com/swack-tools/oxidex/releases) page:

- **Linux** (x86_64): `oxidex-x86_64-linux-musl.tar.gz`
- **Linux** (ARM64): `oxidex-aarch64-linux-musl.tar.gz`
- **macOS** (Intel): `oxidex-x86_64-macos.tar.gz`
- **macOS** (Apple Silicon): `oxidex-aarch64-macos.tar.gz`
- **Windows** (x86_64): `oxidex-x86_64-windows.zip`

```bash
# Example: Install on Linux (x86_64)
wget https://github.com/swack-tools/oxidex/releases/download/v1.0.0/oxidex-x86_64-linux-musl.tar.gz
tar xzf oxidex-x86_64-linux-musl.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

### From Source

For development or building from source:

```bash
# Clone the repository
git clone https://github.com/swack-tools/oxidex.git
cd oxidex

# Build the project (default, signature-based detection)
cargo build --release

# Build with Magika AI detection (optional)
cargo build --release --features magika

# Run
./target/release/oxidex

# Optional: Install to system path
cargo install --path .

# Or install with Magika feature
cargo install --path . --features magika
```

#### Optional Features

**Magika AI-Powered File Detection**

OxiDex includes optional support for [Magika](https://github.com/google/magika), Google's deep learning model for file type identification, achieving ~99% accuracy across 200+ file formats.

- **Enable with**: `--features magika` flag during build
- **Use with**: `--detector=magika` CLI flag
- **Benefits**: Enhanced detection accuracy for edge cases and uncommon formats
- **Default**: Fast signature-based detection (no extra dependencies)

## Usage

### CLI

```bash
# Extract all metadata from a file
oxidex photo.jpg

# Extract specific tags
oxidex -Make -Model -DateTimeOriginal photo.jpg

# Write metadata
oxidex -Artist="Your Name" photo.jpg

# Batch processing (recursive)
oxidex -r /path/to/photos/

# JSON output
oxidex -json photo.jpg

# CSV output for batch analysis
oxidex -csv -r /path/to/photos/ > metadata.csv

# Copy metadata between files
oxidex -TagsFromFile source.jpg target.jpg

# Use AI-powered file detection (requires --features magika build)
oxidex --detector=magika photo.jpg

# Explicit signature-based detection (default)
oxidex --detector=signature photo.jpg

# Date shifting (adjust all timestamps by offset)
oxidex "-DateTimeOriginal+=1:0:0 0:0:0" photo.jpg

# Extract Canon-specific metadata (for Canon cameras)
oxidex -Canon:FirmwareVersion -Canon:SerialNumber -Canon:OwnerName canon_photo.jpg
```

### Library API

```rust
use oxidex::core::MetadataMap;

// Extract metadata from a file
let metadata = MetadataMap::from_file("photo.jpg")?;
println!("Camera: {}", metadata.get("Make")?);
println!("Date: {}", metadata.get("DateTimeOriginal")?);

// Extract Canon-specific tags (if applicable)
if let Ok(firmware) = metadata.get("Canon:FirmwareVersion") {
    println!("Canon Firmware: {}", firmware);
}
if let Ok(serial) = metadata.get("Canon:SerialNumber") {
    println!("Camera Serial: {}", serial);
}

// Edit and write metadata
metadata.set("Artist", "Your Name")?;
metadata.write_to_file("photo.jpg")?;
```

For complete documentation, see the [User Guide](https://swack-tools.github.io/oxidex/).

## Development

### Prerequisites

- Rust 1.75 or later
- Cargo

### Building

```bash
cargo build
```

### Build Performance

The tag database uses a multi-crate architecture for fast parallel compilation.
First build may take 2-3 minutes to download ExifTool source and generate YAML.
Subsequent builds are much faster (~30-60 seconds on multi-core machines).

For development, you can skip tag generation:
```bash
# Use pre-generated tags
cargo build
```

### Tag Database Generation

OxiDex automatically generates its comprehensive tag database from the official ExifTool Perl source during the build process. This ensures compatibility with ExifTool's extensive metadata tag definitions.

#### How It Works

During `cargo build`, the build script (`build.rs`) performs the following steps:

1. **Downloads ExifTool Source**: Fetches the latest ExifTool source code from the [official GitHub repository](https://github.com/exiftool/exiftool)
2. **Parses Tag Definitions**: Extracts tag metadata from Perl modules in `lib/Image/ExifTool/`:
   - Tag IDs (numeric hex codes, integers, or string identifiers)
   - Tag names and descriptions
   - Writable status and data types
   - Format family classifications (EXIF, XMP, IPTC, GPS, QuickTime, RIFF, etc.)
3. **Generates Rust Code**: Creates tag definitions with 700+ tag definitions (731 in current version)
4. **Validates Output**: Ensures the generated database meets minimum quality standards

The generated file is excluded from version control (`.gitignore`) and rebuilt automatically when you run `cargo build`.

#### Supported Format Families

The tag generator parses definitions for all major metadata formats:
- **EXIF** (244 tags): Camera settings, image parameters, manufacturer data
- **GPS** (32 tags): Geolocation and positioning data
- **IPTC** (122 tags): Press and media industry metadata
- **QuickTime** (143 tags): Video/audio metadata
- **RIFF** (46 tags): Resource Interchange File Format metadata
- **ICC_Profile** (42 tags): Color management metadata
- **Photoshop** (35 tags): Adobe Photoshop metadata
- **PNG** (30 tags): Portable Network Graphics metadata
- **JPEG** (30 tags): JPEG-specific metadata
- **XMP** (7 tags): Extensible Metadata Platform tags (base module)
- Additional formats: TIFF, JFIF, PDF, PostScript, MakerNotes

#### Fallback Mechanism

If tag generation fails (network issues, parse errors), the build automatically falls back to a manually curated tag registry, ensuring builds always succeed. You'll see a build warning if fallback is used:

```
warning: Tag generation failed: <reason>. Using fallback to manual registry.
```

#### Updating Tags

To regenerate the tag database with the latest ExifTool definitions:

```bash
# Clean build artifacts
cargo clean

# Rebuild (downloads latest ExifTool source and regenerates tags)
cargo build --release
```

The build script caches downloaded ExifTool source in the build directory for faster subsequent builds during development.

### Running Tests

```bash
cargo test
```

### Running Benchmarks

#### Latest CI Benchmark Results

📊 **[View Live Benchmark Reports](https://oxidex.net/benchmarks/report/index.html)** - Interactive Criterion.rs reports automatically updated on every commit to main

The CI pipeline runs comprehensive benchmarks on every push and publishes the results to GitHub Pages. You can view detailed performance graphs, statistical analysis, and historical trends.

Alternatively, download artifacts from the [latest workflow run](../../actions/workflows/ci.yml).

#### Running Benchmarks Locally

The project includes performance benchmarks for core parsing operations:

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench format_detection
cargo bench jpeg_segment_parsing
cargo bench tiff_ifd_parsing
cargo bench full_read_metadata
```

After running benchmarks, Criterion generates detailed HTML reports with performance graphs and statistics:

```bash
# macOS
open target/criterion/report/index.html

# Linux
xdg-open target/criterion/report/index.html

# Windows
start target/criterion/report/index.html
```

**Current Baseline Performance** (established with Iteration 2):
- Format detection: ~2.2 ns per operation
- JPEG segment parsing: ~24 ns per operation
- TIFF IFD parsing: ~94 ns per operation
- Full read_metadata: ~9.3 μs per file (well below 5ms target)

### Code Quality

```bash
# Run clippy lints
cargo clippy

# Format code
cargo fmt

# Check formatting
cargo fmt -- --check
```

### CI/CD Pipeline

OxiDex uses GitHub Actions for continuous integration and automated releases. The CI pipeline includes:

#### CI Workflow (`.github/workflows/ci.yml`)

Runs on every push and pull request:

- **Test Suite**: Full test suite with all features enabled
- **Code Quality**: Clippy linting and formatting checks
- **Security Audit**: Dependency vulnerability scanning with cargo-audit
- **Code Coverage**: Test coverage reporting via Codecov
- **Integration Tests**: Comparison testing against Perl ExifTool
- **Performance Benchmarks**: Automated benchmark runs with Criterion
- **Cross-Compilation Tests**: Builds for both ARM64 and x86_64 Linux targets using QEMU emulation

The cross-compilation job uses [cross](https://github.com/cross-rs/cross) with QEMU emulation to test builds for different architectures before release.

#### Release Workflow (`.github/workflows/release.yml`)

Triggered on git tags (`v*`):

- Builds static binaries for all supported platforms:
  - Linux x86_64 (musl)
  - Linux ARM64 (musl)
  - macOS Intel
  - macOS Apple Silicon
  - Windows x86_64
- Generates checksums (SHA256) for all binaries
- Creates GitHub Release with auto-generated release notes
- Uploads all artifacts to the release

To trigger a release:
```bash
git tag v1.0.3
git push origin v1.0.3
```

### Fuzzing

OxiDex includes continuous fuzzing targets for security-critical parsers to detect crashes, hangs, and memory safety issues.

#### Prerequisites

Install cargo-fuzz (requires nightly Rust):

```bash
cargo install cargo-fuzz
```

#### Running Fuzzing Targets

Run PDF parser fuzzer:
```bash
cargo fuzz run fuzz_pdf

# Run with time limit (e.g., 60 seconds)
cargo fuzz run fuzz_pdf -- -max_total_time=60
```

Run MP4/QuickTime parser fuzzer:
```bash
cargo fuzz run fuzz_mp4

# Run with memory limit to prevent OOM (MP4 parser reads up to 10MB)
cargo fuzz run fuzz_mp4 -- -max_len=10485760 -max_total_time=60
```

#### Corpus Management

Fuzzing seed corpus files are located in:
- `fuzz/corpus/fuzz_pdf/` - PDF test files (3+ seed files)
- `fuzz/corpus/fuzz_mp4/` - MP4/QuickTime test files (3+ seed files)

To add new seed files:
```bash
# Copy valid sample files to the corpus
cp my_test.pdf fuzz/corpus/fuzz_pdf/
cp my_test.mp4 fuzz/corpus/fuzz_mp4/
```

#### Coverage Measurement

Check fuzzing coverage:
```bash
cargo fuzz coverage fuzz_pdf
cargo fuzz coverage fuzz_mp4
```

#### CI Integration

Fuzzing targets are available for:
- **Local development**: Run fuzzers before committing parser changes
- **PR validation**: Short fuzzing runs in GitHub Actions (planned)
- **Continuous fuzzing**: OSS-Fuzz integration (planned)

#### Crash Triage

If fuzzing discovers a crash:
1. Crash inputs are saved to `fuzz/artifacts/fuzz_<target>/`
2. Reproduce with: `cargo fuzz run fuzz_<target> fuzz/artifacts/fuzz_<target>/<crash_file>`
3. Debug with: `cargo fuzz run -D fuzz_<target> fuzz/artifacts/fuzz_<target>/<crash_file>`
4. Fix the parser and verify: `cargo fuzz run fuzz_<target> fuzz/artifacts/fuzz_<target>/<crash_file>` should not crash

## Contributing

Contributions are welcome! This project is in its early stages and there are many opportunities to contribute.

Please ensure:
- All tests pass (`cargo test`)
- Code is formatted (`cargo fmt`)
- Clippy lints pass (`cargo clippy`)
- New code includes appropriate tests and documentation

## License

This project is licensed under the GNU General Public License v3.0 (GPL-3.0) - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

This project is inspired by and aims to be compatible with [ExifTool](https://exiftool.org/) by Phil Harvey. OxiDex is an independent reimplementation and is not affiliated with or endorsed by the original ExifTool project.

## Technology Stack

- **Language**: Rust 1.75+ (2021 Edition)
- **CLI Framework**: clap v4
- **Binary Parsing**: nom v7
- **XML Parsing**: quick-xml
- **JSON Output**: serde_json
- **Date/Time**: chrono
- **String Encoding**: encoding_rs
- **Concurrency**: rayon
- **Memory-mapped I/O**: memmap2

## Project Status & Roadmap

See the [project documentation](docs/) for detailed architectural decisions, implementation plans, and iteration roadmaps.

---

**Status**: Stable Release
**Current Version**: 1.0.0
**License**: GPL-3.0
**Documentation**: [User Guide](https://swack-tools.github.io/oxidex/) | [API Docs](https://docs.rs/oxidex)
**Issues**: [GitHub Issues](https://github.com/swack-tools/oxidex/issues)
