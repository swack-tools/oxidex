---
layout: home

hero:
  name: OxiDex
  text: Modern ExifTool in Rust
  tagline: High-performance metadata management for 300+ file formats
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: ExifTool Compatibility
      link: /reference/comparison/
    - theme: alt
      text: GitHub
      link: https://github.com/swack-tools/oxidex

features:
  - icon: ⚡
    title: Up to 10x Faster
    details: 3.7-9.7x performance improvement over Perl ExifTool with zero-cost abstractions and parallel processing
  - icon: 🔒
    title: Memory Safe
    details: Rust eliminates buffer overflows, use-after-free bugs, and entire classes of vulnerabilities
  - icon: 🎯
    title: 32,677 Metadata Tags
    details: Complete parity with ExifTool across 140+ format families, automatically synchronized
  - icon: 🤖
    title: AI Integration
    details: MCP server for Claude and other AI assistants - manage metadata through natural conversation
  - icon: 🛠️
    title: Drop-in Replacement
    details: CLI compatible with original ExifTool syntax for seamless migration
  - icon: 📦
    title: Static Binaries
    details: Self-contained executables with no runtime dependencies for easy deployment
  - icon: 🌐
    title: Cross-Platform
    details: Native binaries for Windows, Linux (x86_64/ARM64), and macOS (Intel/Apple Silicon)
  - icon: 📊
    title: ExifTool Compatibility
    details: Automated tag-by-tag comparison with ExifTool - track coverage across 19 formats, updated on every parser change
    link: /reference/comparison/
    linkText: View Report
---

## Quick Example

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
```

## Performance Comparison

OxiDex delivers exceptional performance improvements over the Perl-based ExifTool:

- **3.7x faster** - Single file metadata extraction (31.8ms vs 116.5ms)
- **9.7x faster** - Batch processing 1000 files (197ms vs 1911ms)
- **8.7x faster** - Write operations (23ms vs 200ms)
- **6.5x faster** - Format detection (10ms vs 67ms)

[View detailed benchmarks →](/performance/benchmarks)

## Why OxiDex?

**For Photographers & Archivists:**
- Process large image libraries in seconds, not minutes
- Reliable metadata preservation with memory-safe operations
- Support for 40+ camera RAW formats

**For Developers:**
- Native Rust library API for integration
- C FFI bindings for cross-language support
- MCP server for AI assistant integration
- Comprehensive documentation and examples

**For AI & Automation:**
- Natural language metadata operations via MCP
- Works with Claude, Cline, and other MCP clients
- 9 specialized tools for extraction, search, and analysis

**For DevOps:**
- Static binaries with no dependencies
- Cross-compilation for all major platforms
- Continuous fuzzing for security

## Supported Formats

140+ format families including:
- **Images:** JPEG, PNG, TIFF, GIF, BMP, WebP, HEIF
- **RAW:** Canon (CR2/CR3), Nikon (NEF), Sony (ARW), and 35+ more
- **Video:** MP4, MOV, MKV, AVI, FLV
- **Audio:** MP3, FLAC, AAC, WAV, OGG
- **Documents:** PDF, Office formats
- **Metadata:** EXIF, XMP, IPTC, ICC Profiles, MakerNotes

[See complete format list →](/reference/formats/)
