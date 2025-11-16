# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Canon MakerNotes Phase 1**: Basic Canon-specific metadata extraction from EXIF MakerNote tags
  - **Supported Tags:**
    - `Canon:ImageType` - Image type identifier (e.g., "IMG:EOS R5")
    - `Canon:FirmwareVersion` - Camera firmware version
    - `Canon:OwnerName` - Camera owner name
    - `Canon:SerialNumber` - Camera serial number
    - `Canon:CanonModelID` - Canon-specific model identifier
    - `Canon:FileNumber` - Internal file number assigned by camera
  - **Implementation Details:**
    - Parses Canon MakerNote IFD structure (TIFF-based format)
    - Supports both little-endian and big-endian byte orders
    - Handles both inline values (≤4 bytes) and offset-based values (>4 bytes)
    - Gracefully handles missing or invalid MakerNote data
  - **Phase 2 Preview:** Complex array tags (CameraSettings, ShotInfo, AFInfo, LensInfo) planned for next phase
  - **Files Added:**
    - `src/parsers/tiff/makernotes/mod.rs` - MakerNotes module structure
    - `src/parsers/tiff/makernotes/canon.rs` - Canon MakerNote parser with comprehensive documentation
  - **Integration:** Canon MakerNotes are automatically extracted from JPEG EXIF data when present

### Fixed
- **CI/CD**: Fixed ARM64 cross-compilation in GitHub Actions by implementing QEMU emulation. Previously, attempting to run ARM64 Docker images (`ghcr.io/cross-rs/aarch64-unknown-linux-musl`) on x86_64 runners resulted in "exec format error". The fix adds `docker/setup-qemu-action` and `docker/setup-buildx-action` to enable multi-platform builds on x86_64 runners.

### Added
- **CI/CD**: New `cross-compile` job in CI workflow that tests both ARM64 (`aarch64-unknown-linux-musl`) and x86_64 (`x86_64-unknown-linux-musl`) Linux builds using the `cross` tool with QEMU emulation support.

## [1.0.0] - 2025-10-30

### Added

#### Core Features
- **Multi-format metadata extraction** supporting 50+ file formats:
  - JPEG (EXIF, JFIF, XMP, IPTC, Photoshop)
  - PNG (eXIf, tEXt, zTXt, iTXt chunks)
  - TIFF (multi-page support, sub-IFDs)
  - PDF (Info dictionary, XMP metadata)
  - MP4/QuickTime (atoms and metadata tracks)
  - RAW formats and camera maker notes
- **Comprehensive tag database** with 700+ metadata tags automatically generated from ExifTool source
- **Format families support**: EXIF (244 tags), GPS (32 tags), IPTC (122 tags), QuickTime (143 tags), RIFF (46 tags), ICC_Profile (42 tags), Photoshop (35 tags), PNG (30 tags), JPEG (30 tags), XMP, and more

#### CLI Application
- **Full CLI implementation** with backward compatibility for 90% of common ExifTool usage patterns
- **Batch processing** with parallel execution using Rayon for multi-core performance
- **Recursive directory traversal** with `-r` flag for processing entire folder structures
- **Flexible output formats**: human-readable, JSON, CSV
- **File preservation options**: backup originals, preserve timestamps
- **Advanced metadata operations**:
  - Read and display metadata (`exiftool-rs file.jpg`)
  - Write and modify metadata (`exiftool-rs -Artist="Name" file.jpg`)
  - Copy metadata between files (`-TagsFromFile` support)
  - Date/time shifting for batch timestamp corrections
  - File renaming based on metadata patterns

#### Library API
- **Clean Rust library API** with hexagonal architecture design
- **Zero-copy parsing** for efficient memory usage
- **Type-safe metadata operations** with comprehensive tag validation
- **Extensible parser system** for adding new format support
- **Memory-mapped I/O** for handling large files efficiently

#### Cross-Language Integration
- **C FFI bindings** (`staticlib` and `cdylib` crate types) for integration with C/C++, Python, Node.js, and other languages
- **Auto-generated C headers** using cbindgen with CI verification
- **Python ctypes bindings** with comprehensive examples and documentation
- **FFI safety guarantees** with proper error handling across language boundaries

#### Build & Distribution
- **Automated tag database generation** from ExifTool Perl source during build
- **Cross-compilation support** for Linux (x86_64, ARM64), macOS (Intel, Apple Silicon), Windows (x86_64)
- **Automated release pipeline** with GitHub Actions building binaries on git tag push
- **Package formats**:
  - Debian packages (.deb) for Ubuntu/Debian
  - RPM packages for Fedora/RHEL/CentOS
  - Homebrew formula for macOS
  - Static binaries (musl libc) with no runtime dependencies
- **Release profile optimizations**: LTO, codegen-units=1, strip=true for maximum performance

#### Testing & Quality
- **Comprehensive test suite** with 100+ integration tests
- **Automated ExifTool comparison tests** validating parity with Perl ExifTool
- **102-image test corpus** covering diverse real-world scenarios
- **Continuous fuzzing** infrastructure for PDF and MP4 parsers with libFuzzer
- **Performance benchmarks** using Criterion and hyperfine
- **CI/CD pipeline** with automated testing, linting, and formatting checks

#### Documentation
- **Comprehensive user guide** (mdBook) with GitHub Pages deployment at `docs/book/`:
  - Installation instructions for all platforms
  - CLI usage guide with examples
  - Library API documentation with code samples
  - FFI integration guide for C and Python
  - Supported formats and tag reference
  - Architecture overview and design rationale
- **API documentation**: Rust library API reference and C FFI API specification
- **Migration guide** from Perl ExifTool with compatibility notes

### Performance

ExifTool-RS delivers exceptional performance improvements over Perl ExifTool (version 13.36) on Apple M4 hardware:

| Operation | Perl ExifTool | ExifTool-RS | Speedup |
|-----------|---------------|-------------|---------|
| Single JPEG read | 37.5ms ± 0.5ms | 2.3ms ± 0.1ms | **16.1x faster** |
| Batch processing (1000 files) | 916.4ms ± 8.0ms | 14.1ms ± 0.3ms | **64.9x faster** |
| Write operation (modify EXIF) | 96.8ms ± 1.3ms | 7.3ms ± 0.6ms | **13.3x faster** |
| Format detection | 39.3ms ± 0.4ms | 2.8ms ± 0.1ms | **14.2x faster** |

**Key optimizations:**
- Zero-cost abstractions and compiled code eliminate interpreter overhead
- Parallel processing with Rayon leverages all CPU cores for batch operations
- Memory-mapped I/O and efficient binary manipulation
- SIMD-friendly data layouts for future vectorization

### Security

- **Memory safety** guaranteed by Rust's ownership system (zero buffer overflows, use-after-free, or data races)
- **Continuous fuzzing** with libFuzzer detecting crashes, hangs, and undefined behavior
- **Safe parser design** with explicit bounds checking and error propagation
- **No unsafe code** in critical parsing paths (FFI layer uses minimal `unsafe` with safety invariants)

### Known Limitations

These features are planned for future releases (v1.1+):

- **Array type validation**: ValueType::Array not yet supported in validation system (src/core/validation.rs:167)
  - Current impact: Array-valued metadata can bypass type checking
  - Workaround: Manual validation for array-valued tags
- **TIFF writer limitations**: Float, Struct, and Array tag values not yet implemented in TIFF writer (src/writers/tiff_writer.rs:474-477)
  - Current impact: These value types are silently skipped during TIFF write operations
  - Workaround: Use supported types (Integer, Text, Binary, Rational)
- **Rational round-trip parsing**: TIFF parser may convert Rational values to Integer/Binary when type information is unavailable (tests/integration/write_operations_tests.rs:362)
  - Current impact: Rational value precision may be lost in some round-trip scenarios
  - Workaround: Ensure EXIF type information is present

## [Unreleased]

### Future Enhancements (Roadmap)

**Phase 2 (v1.1 - v2.0): Expansion & Performance**
- Expand to 150+ formats (obscure camera formats, extended maker notes)
- SIMD optimizations for bulk operations (UTF-8 validation, checksums)
- WebAssembly build for browser-based metadata extraction
- Profile-guided optimization (PGO) builds
- Incremental metadata updates (avoid full file rewrites)

**Phase 3 (v2.1+): Ecosystem & Intelligence**
- Machine learning integration for tag suggestion and auto-correction
- Cloud storage integration (S3, Azure Blob with async I/O)
- Metadata analytics and query DSL
- Optional GUI (egui framework)
- Streaming API for real-time video metadata processing
- Distributed processing for million-file archives

---

## Release Notes

### v1.0.0: Initial Stable Release

ExifTool-RS achieves its foundational milestone with a fully-featured, production-ready metadata management tool. This release completes **Phase 1: Foundation & Adoption** with:

- **50+ format support** covering 90% of common use cases (JPEG, PNG, TIFF, PDF, MP4, RAW formats)
- **16-65x performance improvement** over Perl ExifTool through compiled code and parallel processing
- **Memory safety guarantees** eliminating entire classes of security vulnerabilities
- **Cross-platform binaries** for Linux, macOS, Windows with zero runtime dependencies
- **Comprehensive documentation** and migration guide for Perl ExifTool users

This release is suitable for production use in:
- Automated media processing pipelines
- Digital asset management systems
- Photography workflow automation
- Embedded systems requiring native binaries
- Cross-language integration via C FFI (Python, Node.js, Go, etc.)

**Migration from Perl ExifTool**: ExifTool-RS is designed as a drop-in replacement for 90% of common workflows. See the migration guide in `docs/book/src/migration.md` for compatibility notes and feature differences.

**Installation**: `cargo install exiftool-rs` or download pre-built binaries from [GitHub Releases](https://github.com/exiftool-rs/exiftool-rs/releases/tag/v1.0.0).

---

[1.0.0]: https://github.com/exiftool-rs/exiftool-rs/releases/tag/v1.0.0
