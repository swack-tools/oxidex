# ExifTool-RS User Guide (mdBook)

This directory contains the comprehensive user guide for ExifTool-RS, built with [mdBook](https://rust-lang.github.io/mdBook/).

## Building the Book

### Prerequisites

Install mdBook:

```bash
cargo install mdbook
```

### Build

```bash
# From this directory (docs/book/)
mdbook build

# Output is generated in: book/
```

### Preview Locally

```bash
mdbook serve

# Open browser to: http://localhost:3000
```

The `serve` command watches for file changes and auto-reloads the browser.

## Structure

```
docs/book/
├── book.toml           # Configuration file
├── src/                # Source markdown files
│   ├── SUMMARY.md      # Table of contents
│   ├── intro.md        # Introduction chapter
│   ├── installation.md # Installation guide
│   ├── cli_usage.md    # Command-line usage
│   ├── library_api.md  # Rust library API
│   ├── ffi.md          # C FFI integration
│   ├── formats.md      # Supported formats
│   └── troubleshooting.md # Troubleshooting guide
└── book/               # Generated HTML (gitignored)
```

## Chapters

1. **Introduction** (245 lines)
   - Project vision, features, architecture
   - Current status and roadmap
   - Technology stack

2. **Installation** (473 lines)
   - Binary download, cargo install, build from source
   - Tag database generation
   - Development setup (testing, benchmarking, fuzzing)
   - Platform-specific notes

3. **Command-Line Usage** (645 lines)
   - Reading and writing metadata
   - Batch processing
   - File renaming, date shifting
   - Output formats (JSON, CSV)
   - Common options and examples

4. **Library API** (567 lines)
   - Core concepts (tag naming, type safety)
   - Planned high-level API (builder pattern)
   - Current low-level API (MetadataMap, TagValue)
   - Working code examples
   - Error handling

5. **C FFI Integration** (718 lines)
   - Quick start guide
   - Core concepts (opaque handles, error handling)
   - Complete API reference
   - C and Python examples
   - Platform-specific compilation notes

6. **Supported Formats** (407 lines)
   - Comprehensive format list (JPEG, TIFF, PNG, PDF, MP4)
   - Metadata standards (EXIF, XMP, IPTC, GPS)
   - Tag database statistics
   - Format detection
   - Planned format support

7. **Troubleshooting** (700 lines)
   - Common errors and solutions
   - Performance tips
   - Debugging strategies
   - Known limitations
   - Getting help

**Total**: 3,755 lines of comprehensive documentation

## Deployment

The documentation is automatically deployed to GitHub Pages via GitHub Actions when changes are pushed to the `main` branch.

**Workflow**: `.github/workflows/docs.yml`

**Published URL**: https://exiftool-rs.github.io/exiftool-rs/ (once repository is public)

## Contributing

To contribute to the documentation:

1. Edit markdown files in `src/`
2. Test locally with `mdbook serve`
3. Submit a pull request

Follow these guidelines:
- Use clear, concise language
- Include code examples with comments
- Test all code examples
- Maintain consistent formatting
- Link between chapters for navigation

## License

This documentation is licensed under the same license as ExifTool-RS (GPL-3.0).
