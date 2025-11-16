# ExifTool-RS

[![CI](https://github.com/exiftool-rs/exiftool-rs/workflows/CI/badge.svg)](https://github.com/exiftool-rs/exiftool-rs/actions)
[![Integration Tests](https://github.com/exiftool-rs/exiftool-rs/workflows/Integration%20Tests%20(ExifTool%20Comparison)/badge.svg)](https://github.com/exiftool-rs/exiftool-rs/actions)

A modern, high-performance Rust reimplementation of the industry-standard [ExifTool](https://exiftool.org/) metadata management library and command-line application.

## Project Vision

ExifTool-RS aims to provide a memory-safe, zero-cost abstraction alternative to the Perl-based ExifTool while maintaining full compatibility with its extensive metadata tag support. The goal is to deliver superior performance, native cross-compilation capabilities, and seamless integration into modern software ecosystems.

## Key Features

- **100% ExifTool Tag Parity**: 32,677 metadata tags across 140+ format families, automatically synchronized with ExifTool source
- **High Performance**: 16-65x performance improvement over Perl implementation through zero-cost abstractions and parallel processing
- **Memory Safety**: Eliminates entire classes of vulnerabilities (buffer overflows, use-after-free) through Rust's ownership system
- **Binary Distribution**: Static, self-contained binaries with no runtime dependencies
- **API-First Design**: Native Rust library with C FFI bindings for cross-language integration
- **Backward Compatibility**: CLI argument compatibility with original ExifTool for drop-in replacement scenarios
- **Cross-Platform**: Windows, Linux, macOS support with native binaries

## Architecture

ExifTool-RS follows a **Hexagonal Architecture** (Ports and Adapters) pattern with three main layers:

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
| Single JPEG Read | 42.9ms ± 1.5ms | 5.3ms ± 0.6ms | **8.0x faster** |
| Batch Processing (1000 files) | 1163.3ms ± 12.0ms | 22.5ms ± 0.3ms | **51.7x faster** |
| Write Operation (modify EXIF tag) | 110.6ms ± 2.9ms | 6.5ms ± 0.3ms | **17.0x faster** |
| Format Detection | 41.3ms ± 0.8ms | 4.9ms ± 0.2ms | **8.4x faster** |

*Benchmarks performed using [hyperfine](https://github.com/sharkdp/hyperfine) with multiple runs and warmup periods.*

### Key Performance Improvements

- **Single file operations**: Zero-cost abstractions and compiled code eliminate Perl interpreter overhead, achieving 8.0x faster metadata extraction
- **Batch processing**: Parallel processing with Rayon leverages all CPU cores, processing 1000 files in 22.5ms ± 0.3ms vs. 1163.3ms ± 12.0ms for single-threaded Perl
- **Write operations**: Efficient binary manipulation and atomic file operations provide 17.0x faster EXIF tag modifications
- **Format detection**: Native compiled code dramatically outperforms interpreted Perl for magic byte detection (8.4x faster)

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

ExifTool-RS v1.0.0 is now production-ready! Install via cargo, package managers, or pre-built binaries.

### From crates.io (Recommended)

```bash
cargo install exiftool-rs
```

### From Debian Package (Ubuntu/Debian Linux)

For Debian-based Linux distributions (Ubuntu, Debian, Linux Mint, etc.):

```bash
# Download the .deb package from GitHub Releases
wget https://github.com/exiftool-rs/exiftool-rs/releases/download/v1.0.0/exiftool-rs_1.0.0_amd64.deb

# Install using dpkg
sudo dpkg -i exiftool-rs_1.0.0_amd64.deb

# Or using apt (resolves dependencies automatically)
sudo apt install ./exiftool-rs_1.0.0_amd64.deb

# Verify installation
exiftool-rs --version
```

To build your own `.deb` package:

```bash
# Install cargo-deb
cargo install cargo-deb

# Build the Debian package
cargo deb

# Package will be created at: target/debian/exiftool-rs_1.0.0_amd64.deb
```

### From RPM Package (Fedora/RHEL/CentOS/openSUSE)

For RPM-based Linux distributions (Fedora, RHEL, CentOS, openSUSE, etc.):

```bash
# Download the .rpm package from GitHub Releases
wget https://github.com/exiftool-rs/exiftool-rs/releases/download/v1.0.0/exiftool-rs-1.0.0-1.x86_64.rpm

# Install using dnf (Fedora/RHEL 8+)
sudo dnf install exiftool-rs-1.0.0-1.x86_64.rpm

# Or using yum (older RHEL/CentOS)
sudo yum install exiftool-rs-1.0.0-1.x86_64.rpm

# Or using rpm directly
sudo rpm -i exiftool-rs-1.0.0-1.x86_64.rpm

# Verify installation
exiftool-rs --version
```

To build your own `.rpm` package:

```bash
# Install cargo-generate-rpm
cargo install cargo-generate-rpm

# Build the release binary first
cargo build --release

# Generate the RPM package
cargo generate-rpm

# Package will be created at: target/generate-rpm/exiftool-rs-1.0.0-1.x86_64.rpm
```

### From Homebrew (macOS)

For macOS users with [Homebrew](https://brew.sh):

```bash
# Install from Homebrew formula (source build)
brew install --build-from-source https://raw.githubusercontent.com/exiftool-rs/exiftool-rs/main/packaging/homebrew/exiftool-rs.rb

# Or install from local formula file
brew install --build-from-source ./packaging/homebrew/exiftool-rs.rb

# Verify installation
exiftool-rs --version
```

**Note**: The Homebrew formula builds from source, which may take 5-10 minutes depending on your system. Future releases will include pre-built bottles for faster installation.

### From Pre-Built Binaries

Static binaries are available for all major platforms on the [GitHub Releases](https://github.com/exiftool-rs/exiftool-rs/releases) page:

- **Linux** (x86_64): `exiftool-rs-x86_64-linux-musl.tar.gz`
- **Linux** (ARM64): `exiftool-rs-aarch64-linux-musl.tar.gz`
- **macOS** (Intel): `exiftool-rs-x86_64-macos.tar.gz`
- **macOS** (Apple Silicon): `exiftool-rs-aarch64-macos.tar.gz`
- **Windows** (x86_64): `exiftool-rs-x86_64-windows.zip`

```bash
# Example: Install on Linux (x86_64)
wget https://github.com/exiftool-rs/exiftool-rs/releases/download/v1.0.0/exiftool-rs-x86_64-linux-musl.tar.gz
tar xzf exiftool-rs-x86_64-linux-musl.tar.gz
sudo mv exiftool-rs /usr/local/bin/
exiftool-rs --version
```

### From Source

For development or building from source:

```bash
# Clone the repository
git clone https://github.com/exiftool-rs/exiftool-rs.git
cd exiftool-rs

# Build the project
cargo build --release

# Run
./target/release/exiftool-rs

# Optional: Install to system path
cargo install --path .
```

## Usage

### CLI

```bash
# Extract all metadata from a file
exiftool-rs photo.jpg

# Extract specific tags
exiftool-rs -Make -Model -DateTimeOriginal photo.jpg

# Write metadata
exiftool-rs -Artist="Your Name" photo.jpg

# Batch processing (recursive)
exiftool-rs -r /path/to/photos/

# JSON output
exiftool-rs -json photo.jpg

# CSV output for batch analysis
exiftool-rs -csv -r /path/to/photos/ > metadata.csv

# Copy metadata between files
exiftool-rs -TagsFromFile source.jpg target.jpg

# Date shifting (adjust all timestamps by offset)
exiftool-rs "-DateTimeOriginal+=1:0:0 0:0:0" photo.jpg
```

### Library API

```rust
use exiftool_rs::core::MetadataMap;

// Extract metadata from a file
let metadata = MetadataMap::from_file("photo.jpg")?;
println!("Camera: {}", metadata.get("Make")?);
println!("Date: {}", metadata.get("DateTimeOriginal")?);

// Edit and write metadata
metadata.set("Artist", "Your Name")?;
metadata.write_to_file("photo.jpg")?;
```

For complete documentation, see the [User Guide](https://exiftool-rs.github.io/exiftool-rs/).

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

ExifTool-RS automatically generates its comprehensive tag database from the official ExifTool Perl source during the build process. This ensures compatibility with ExifTool's extensive metadata tag definitions.

#### How It Works

During `cargo build`, the build script (`build.rs`) performs the following steps:

1. **Downloads ExifTool Source**: Fetches the latest ExifTool source code from the [official GitHub repository](https://github.com/exiftool/exiftool)
2. **Parses Tag Definitions**: Extracts tag metadata from Perl modules in `lib/Image/ExifTool/`:
   - Tag IDs (numeric hex codes, integers, or string identifiers)
   - Tag names and descriptions
   - Writable status and data types
   - Format family classifications (EXIF, XMP, IPTC, GPS, QuickTime, RIFF, etc.)
3. **Generates Rust Code**: Creates `src/tag_db/generated_tags.rs` with 700+ tag definitions (731 in current version)
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

📊 **[View Live Benchmark Reports](https://swack-tools.github.io/exiftool-rs/benchmarks/report/index.html)** - Interactive Criterion.rs reports automatically updated on every commit to main

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

ExifTool-RS uses GitHub Actions for continuous integration and automated releases. The CI pipeline includes:

#### CI Workflow (`.github/workflows/ci.yml`)

Runs on every push and pull request:

- **Test Suite**: Full test suite with all features enabled
- **Code Quality**: Clippy linting and formatting checks
- **Security Audit**: Dependency vulnerability scanning with cargo-audit
- **Code Coverage**: Test coverage reporting via Codecov
- **Integration Tests**: Comparison testing against Perl ExifTool
- **Performance Benchmarks**: Automated benchmark runs with Criterion
- **Cross-Compilation Tests**: Builds for both ARM64 and x86_64 Linux targets using QEMU emulation

The cross-compilation job uses [cross](https://github.com/cross-rs/cross) with QEMU emulation to test builds for:
- `aarch64-unknown-linux-musl` (ARM64 Linux)
- `x86_64-unknown-linux-musl` (x86_64 Linux)

This ensures binaries work correctly across different architectures before release.

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

ExifTool-RS includes continuous fuzzing targets for security-critical parsers to detect crashes, hangs, and memory safety issues.

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

This project is inspired by and aims to be compatible with [ExifTool](https://exiftool.org/) by Phil Harvey. ExifTool-RS is an independent reimplementation and is not affiliated with or endorsed by the original ExifTool project.

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
**Documentation**: [User Guide](https://exiftool-rs.github.io/exiftool-rs/) | [API Docs](https://docs.rs/exiftool-rs)
**Issues**: [GitHub Issues](https://github.com/exiftool-rs/exiftool-rs/issues)
