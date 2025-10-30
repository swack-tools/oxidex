# Installation

This chapter covers the different ways to install ExifTool-RS on your system. Choose the method that best fits your needs.

## Prerequisites

Before installing ExifTool-RS, ensure you have:

- **Rust 1.75 or later** (for building from source or using cargo install)
- **cargo** (Rust's package manager, comes with Rust)
- A supported operating system: Linux, macOS, or Windows

## Installation Methods

### Method 1: Binary Download (Recommended for End Users)

**Status**: 🔄 Coming Soon

Pre-built binaries for ExifTool-RS will be available on the [GitHub Releases](https://github.com/exiftool-rs/exiftool-rs/releases) page once v1.0 is released.

**Planned Platform Support:**
- Linux (x86_64, ARM64)
- macOS (Intel, Apple Silicon)
- Windows (x86_64)

**Download Instructions (Future):**

1. Visit the [Releases page](https://github.com/exiftool-rs/exiftool-rs/releases)
2. Download the binary for your platform
3. Extract the archive
4. Move the binary to a location in your `PATH`

```bash
# Linux/macOS example (future)
wget https://github.com/exiftool-rs/exiftool-rs/releases/download/v1.0.0/exiftool-rs-linux-x86_64.tar.gz
tar -xzf exiftool-rs-linux-x86_64.tar.gz
sudo mv exiftool-rs /usr/local/bin/
```

### Method 2: Cargo Install

**Status**: 🔄 In Development

Once ExifTool-RS is published to [crates.io](https://crates.io), you'll be able to install it using cargo:

```bash
# Install from crates.io (future)
cargo install exiftool-rs

# Install from git (current)
cargo install --git https://github.com/exiftool-rs/exiftool-rs
```

This will compile and install the latest version of ExifTool-RS to `~/.cargo/bin/` (or your configured cargo bin directory). Make sure this directory is in your `PATH`.

### Method 3: Build from Source (Recommended for Development)

**Status**: ✅ Available Now

Building from source gives you the most control and is required if you want to contribute to development.

#### Step 1: Install Rust

If you don't have Rust installed, use [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the on-screen instructions. After installation, restart your terminal or run:

```bash
source $HOME/.cargo/env
```

Verify the installation:

```bash
rustc --version
cargo --version
```

#### Step 2: Clone the Repository

```bash
git clone https://github.com/exiftool-rs/exiftool-rs.git
cd exiftool-rs
```

#### Step 3: Build the Project

**Development Build** (faster compilation, slower execution):

```bash
cargo build
```

The binary will be located at `target/debug/exiftool-rs`.

**Release Build** (slower compilation, optimized execution):

```bash
cargo build --release
```

The optimized binary will be located at `target/release/exiftool-rs`.

#### Step 4: Run the Binary

```bash
# Development build
./target/debug/exiftool-rs --help

# Release build
./target/release/exiftool-rs --help
```

#### Step 5: Optional - Install to System

You can install the compiled binary to your cargo bin directory:

```bash
cargo install --path .
```

Or manually copy it to a system location:

```bash
# Linux/macOS
sudo cp target/release/exiftool-rs /usr/local/bin/

# Verify installation
exiftool-rs --version
```

## Tag Database Generation

ExifTool-RS automatically generates its comprehensive tag database from the official ExifTool Perl source during the build process. This ensures compatibility with ExifTool's extensive metadata tag definitions.

### How It Works

When you run `cargo build`, the build script (`build.rs`) performs these steps:

1. **Downloads ExifTool Source**: Fetches the latest ExifTool source code from the [official GitHub repository](https://github.com/exiftool/exiftool)
2. **Parses Tag Definitions**: Extracts tag metadata from Perl modules in `lib/Image/ExifTool/`:
   - Tag IDs (numeric hex codes, integers, or string identifiers)
   - Tag names and descriptions
   - Writable status and data types
   - Format family classifications (EXIF, XMP, IPTC, GPS, QuickTime, RIFF, etc.)
3. **Generates Rust Code**: Creates `src/tag_db/generated_tags.rs` with 700+ tag definitions
4. **Validates Output**: Ensures the generated database meets minimum quality standards

The generated file is excluded from version control (`.gitignore`) and rebuilt automatically when you run `cargo build`.

### Supported Format Families

The tag generator parses definitions for all major metadata formats:

- **EXIF** (244 tags): Camera settings, image parameters, manufacturer data
- **GPS** (32 tags): Geolocation and positioning data
- **IPTC** (122 tags): Press and media industry metadata
- **QuickTime** (143 tags): Video/audio metadata for MP4, MOV files
- **RIFF** (46 tags): Resource Interchange File Format metadata (AVI, WAV)
- **ICC_Profile** (42 tags): Color management metadata
- **Photoshop** (35 tags): Adobe Photoshop-specific metadata
- **PNG** (30 tags): Portable Network Graphics metadata
- **JPEG** (30 tags): JPEG-specific metadata
- **XMP** (7 tags): Extensible Metadata Platform base tags
- Additional formats: TIFF, JFIF, PDF, PostScript, MakerNotes

### Fallback Mechanism

If tag generation fails (network issues, parse errors), the build automatically falls back to a manually curated tag registry, ensuring builds always succeed. You'll see a build warning if fallback is used:

```
warning: Tag generation failed: <reason>. Using fallback to manual registry.
```

### Updating Tags

To regenerate the tag database with the latest ExifTool definitions:

```bash
# Clean build artifacts
cargo clean

# Rebuild (downloads latest ExifTool source and regenerates tags)
cargo build --release
```

The build script caches downloaded ExifTool source in the build directory for faster subsequent builds.

## Development Setup

If you're contributing to ExifTool-RS development, follow these additional setup steps:

### Install Development Tools

```bash
# Install rustfmt (code formatter)
rustup component add rustfmt

# Install clippy (linter)
rustup component add clippy

# Install cargo-fuzz (for security fuzzing)
cargo install cargo-fuzz
```

### Run Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run a specific test
cargo test test_name
```

### Code Quality Checks

```bash
# Format code
cargo fmt

# Check formatting without modifying files
cargo fmt -- --check

# Run clippy lints
cargo clippy

# Run clippy with strict settings
cargo clippy -- -D warnings
```

### Benchmarking

ExifTool-RS includes performance benchmarks using Criterion:

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench format_detection
cargo bench jpeg_segment_parsing
cargo bench tiff_ifd_parsing
cargo bench full_read_metadata
```

After running benchmarks, view detailed reports:

```bash
# macOS
open target/criterion/report/index.html

# Linux
xdg-open target/criterion/report/index.html

# Windows
start target/criterion/report/index.html
```

**Current Baseline Performance:**
- Format detection: ~2.2 ns per operation
- JPEG segment parsing: ~24 ns per operation
- TIFF IFD parsing: ~94 ns per operation
- Full read_metadata: ~9.3 μs per file

### Fuzzing

ExifTool-RS includes continuous fuzzing targets for security-critical parsers:

**Prerequisites**: Fuzzing requires nightly Rust:

```bash
rustup install nightly
```

**Running Fuzzing Targets:**

```bash
# Run PDF parser fuzzer
cargo +nightly fuzz run fuzz_pdf

# Run with time limit (60 seconds)
cargo +nightly fuzz run fuzz_pdf -- -max_total_time=60

# Run MP4/QuickTime parser fuzzer
cargo +nightly fuzz run fuzz_mp4

# Run with memory limit
cargo +nightly fuzz run fuzz_mp4 -- -max_len=10485760 -max_total_time=60
```

**Corpus Management:**

Fuzzing seed corpus files are located in:
- `fuzz/corpus/fuzz_pdf/` - PDF test files
- `fuzz/corpus/fuzz_mp4/` - MP4/QuickTime test files

Add new seed files:

```bash
cp my_test.pdf fuzz/corpus/fuzz_pdf/
cp my_test.mp4 fuzz/corpus/fuzz_mp4/
```

**Coverage Measurement:**

```bash
cargo +nightly fuzz coverage fuzz_pdf
cargo +nightly fuzz coverage fuzz_mp4
```

**Crash Triage:**

If fuzzing discovers a crash:

1. Crash inputs are saved to `fuzz/artifacts/fuzz_<target>/`
2. Reproduce: `cargo +nightly fuzz run fuzz_<target> fuzz/artifacts/fuzz_<target>/<crash_file>`
3. Debug: `cargo +nightly fuzz run -D fuzz_<target> fuzz/artifacts/fuzz_<target>/<crash_file>`
4. Fix the parser and verify the crash is resolved

## Platform-Specific Notes

### Linux

**Dependencies**: No special dependencies required for pre-built binaries.

For building from source, ensure you have:
- build-essential (gcc, make)
- pkg-config
- libssl-dev (if building with SSL support)

```bash
# Ubuntu/Debian
sudo apt install build-essential pkg-config libssl-dev

# Fedora/RHEL
sudo dnf install gcc make pkgconfig openssl-devel
```

### macOS

**Apple Silicon (M1/M2/M3)**: Full native support. No Rosetta required.

**Intel Macs**: Fully supported.

For building from source:
- Xcode Command Line Tools are required
- Install via: `xcode-select --install`

### Windows

**MSVC Toolchain**: ExifTool-RS requires the MSVC build tools.

Install [Build Tools for Visual Studio](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022) or full Visual Studio with C++ support.

Alternatively, use the GNU toolchain:

```bash
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu
```

## Verifying Installation

After installation, verify ExifTool-RS is working correctly:

```bash
# Check version
exiftool-rs --version

# Display help
exiftool-rs --help

# Test with a sample image (if you have one)
exiftool-rs path/to/image.jpg
```

If everything is working, you should see the version number or help output.

## Updating ExifTool-RS

### Binary Installation

Download the latest binary from the [Releases page](https://github.com/exiftool-rs/exiftool-rs/releases) and replace your existing binary.

### Cargo Install

```bash
# Update from crates.io (future)
cargo install exiftool-rs

# Update from git
cargo install --git https://github.com/exiftool-rs/exiftool-rs --force
```

### Source Build

```bash
cd exiftool-rs
git pull origin main
cargo build --release
```

## Uninstalling ExifTool-RS

### Binary Installation

Simply delete the binary:

```bash
# Linux/macOS
sudo rm /usr/local/bin/exiftool-rs

# Or wherever you installed it
rm ~/.local/bin/exiftool-rs
```

### Cargo Install

```bash
cargo uninstall exiftool-rs
```

## Troubleshooting Installation

### Build Fails with "tag generation failed"

This usually indicates a network issue downloading the ExifTool source. The build will fall back to a manual registry and succeed. You can:

1. Check your internet connection
2. Try again with `cargo clean && cargo build`
3. Use a proxy if behind a firewall

### Rust Version Too Old

ExifTool-RS requires Rust 1.75 or later. Update Rust:

```bash
rustup update stable
```

### Linker Errors on Linux

Install build-essential:

```bash
sudo apt install build-essential
```

### Compilation is Very Slow

First-time builds download and compile all dependencies, which can take several minutes. Subsequent builds are much faster due to caching.

For faster incremental builds during development, use the debug profile:

```bash
cargo build  # Instead of cargo build --release
```

## Next Steps

Now that ExifTool-RS is installed, continue to:

- **[Command-Line Usage](cli_usage.md)**: Learn how to use the CLI
- **[Library API](library_api.md)**: Integrate ExifTool-RS into your Rust code
- **[Troubleshooting](troubleshooting.md)**: Common issues and solutions
