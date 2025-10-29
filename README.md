# ExifTool-RS

[![CI](https://github.com/exiftool-rs/exiftool-rs/workflows/CI/badge.svg)](https://github.com/exiftool-rs/exiftool-rs/actions)

A modern, high-performance Rust reimplementation of the industry-standard [ExifTool](https://exiftool.org/) metadata management library and command-line application.

## Project Vision

ExifTool-RS aims to provide a memory-safe, zero-cost abstraction alternative to the Perl-based ExifTool while maintaining full compatibility with its extensive metadata tag support. The goal is to deliver superior performance, native cross-compilation capabilities, and seamless integration into modern software ecosystems.

## Key Features (Planned)

- **Feature Parity**: Support for reading, writing, and editing metadata in 300+ file formats with 28,000+ recognized metadata tags
- **High Performance**: 2-5x performance improvement over Perl implementation through zero-cost abstractions and parallel processing
- **Memory Safety**: Eliminates entire classes of vulnerabilities (buffer overflows, use-after-free) through Rust's ownership system
- **Binary Distribution**: Static, self-contained binaries with no runtime dependencies
- **API-First Design**: Native Rust library with C FFI bindings for cross-language integration
- **Backward Compatibility**: CLI argument compatibility with original ExifTool for drop-in replacement scenarios
- **Cross-Platform**: Windows, Linux, macOS, and WebAssembly targets from a single codebase

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

🚧 **Work in Progress** - This project is in early development (Iteration 1: Foundation Setup)

### Completed
- ✅ Project structure and build system
- ✅ Directory layout following hexagonal architecture
- ✅ Core dependencies configuration
- ✅ Development tooling (rustfmt, clippy)

### In Progress
- 🔄 Core domain models
- 🔄 Basic JPEG EXIF parsing

### Planned
- ⏳ Support for JPEG, TIFF, PNG formats
- ⏳ XMP and IPTC metadata parsing
- ⏳ Full CLI implementation
- ⏳ Metadata writing capabilities
- ⏳ Additional format support

## Installation

**Note**: ExifTool-RS is not yet ready for production use.

### From Source

```bash
# Clone the repository
git clone https://github.com/exiftool-rs/exiftool-rs.git
cd exiftool-rs

# Build the project
cargo build --release

# Run
./target/release/exiftool-rs
```

## Usage

**Coming Soon** - Full CLI functionality is under development.

### Library API (Planned)

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

### CLI (Planned)

```bash
# Extract all metadata
exiftool-rs photo.jpg

# Extract specific tags
exiftool-rs -Make -Model photo.jpg

# Write metadata
exiftool-rs -Artist="Your Name" photo.jpg

# Batch processing
exiftool-rs -r /path/to/photos/
```

## Development

### Prerequisites

- Rust 1.75 or later
- Cargo

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running Benchmarks

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

**Status**: Pre-alpha / Active Development
**Current Version**: 0.1.0
