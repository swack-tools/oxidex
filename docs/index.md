---
layout: home

hero:
  name: OxiDex
  text: Modern ExifTool in Rust
  tagline: High-performance metadata management for 140+ format families
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
    title: Compiled Rust
    details: Native code with parallel batch processing. The published speed figures are being re-measured against the pinned ExifTool; see the performance page for their status
    link: /performance/
    linkText: Benchmark status
  - icon: 🔒
    title: Memory Safe
    details: Rust eliminates buffer overflows, use-after-free bugs, and entire classes of vulnerabilities
  - icon: 🎯
    title: 16,684 Metadata Tags
    details: 77.5% measured extraction conformance against pinned ExifTool, across 126 format families — remeasured on every push, never estimated
    link: /reference/tag-coverage-analysis
    linkText: View Coverage
  - icon: 🤖
    title: AI Integration
    details: A separate MCP server (oxidex-mcp) lets Claude and other MCP clients read, write, search, analyze and copy metadata
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
    details: Automated tag-by-tag comparison against the pinned ExifTool, regenerated when the docs deploy
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

## Performance

OxiDex is compiled, parallel Rust. The published comparison figures against
Perl ExifTool date from 2025-12 and an unpinned ExifTool, and are recorded as
stale in the [autogeneration plan](/AUTOGENERATION-PLAN); a refresh against the
pinned 13.59 is in progress.

[Benchmark status and how to reproduce →](/performance/)

## Why OxiDex?

**For Photographers & Archivists:**
- Process large image libraries in parallel
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
- Five tools: extract, write, search, analyze and copy metadata ([oxidex-mcp](https://github.com/swack-tools/oxidex-mcp))

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
