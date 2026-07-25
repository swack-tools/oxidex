# Introduction

OxiDex is a modern, high-performance Rust implementation of the industry-standard [ExifTool](https://exiftool.org/) metadata management library and command-line application.

## What is OxiDex?

OxiDex provides a memory-safe, high-performance alternative to the Perl-based ExifTool while maintaining full compatibility with its extensive metadata tag support. It delivers superior performance through zero-cost abstractions, native cross-compilation capabilities, and seamless integration into modern software ecosystems.

## Key Capabilities

### Metadata Extraction
Read metadata from 300+ file formats including images (JPEG, PNG, TIFF, RAW), videos (MP4, MKV, AVI), audio (MP3, FLAC), and documents (PDF).

### Metadata Writing
Modify EXIF, XMP, and IPTC metadata with atomic file operations ensuring data integrity.

### Batch Processing
Process thousands of files in parallel, leveraging all CPU cores for maximum performance.

### Format Detection
Automatically identify file formats using magic byte detection, even when file extensions are incorrect.

## Who Should Use OxiDex?

**Photographers:** Manage metadata in large photo libraries efficiently. Process 1000 RAW files in under 200ms.

**Archivists:** Preserve and extract metadata from diverse file formats with memory-safe operations.

**Developers:** Integrate metadata management into applications via Rust library API or C FFI bindings.

**System Administrators:** Deploy static binaries with no runtime dependencies across multiple platforms.

## Current Status

**Version:** 1.1.0 (Stable Release)

- ✅ 32,677 metadata tags (113% of ExifTool's 28,853 tags)
- ✅ 140+ format families with complete ExifTool parity
- ✅ 3.7-9.7x performance improvement
- ✅ Full CLI with backward compatibility
- ✅ Rust library API and C FFI bindings
- ✅ Cross-platform binaries (Linux, macOS, Windows)

## Next Steps

- [Installation Guide](/guide/getting-started) - Install OxiDex via cargo, homebrew, or binaries
- [CLI Usage](/guide/cli-usage) - Learn command-line interface
- [Library API](/guide/library-api) - Integrate OxiDex into Rust projects
- [Performance](/performance/) - View benchmark comparisons

## Project Goals

1. **100% ExifTool Tag Parity:** Support all 32,677+ metadata tags
2. **High Performance:** 10-100x faster than Perl implementation
3. **Memory Safety:** Eliminate vulnerabilities through Rust's ownership system
4. **Drop-in Replacement:** CLI compatibility for seamless migration
5. **Developer-Friendly:** Clean API for library and FFI integration

## License

OxiDex is released under the GNU General Public License v3.0 (GPL-3.0).

## Acknowledgments

This project is inspired by [ExifTool](https://exiftool.org/) by Phil Harvey. OxiDex is an independent reimplementation and is not affiliated with or endorsed by the original ExifTool project.
