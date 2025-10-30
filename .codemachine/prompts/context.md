# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T7",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Create comprehensive user guide using mdBook in docs/book/. Structure: (1) Introduction (what is ExifTool-RS, why Rust, features), (2) Installation (binary download, cargo install, build from source), (3) Command-Line Usage (extracting metadata, writing tags, batch processing, renaming, date shifting), (4) Library API (Rust crate usage with code examples), (5) C FFI (using from C/Python/other languages), (6) Supported Formats (list of 50+ formats), (7) Tag Reference (link to generated tag database or ExifTool docs), (8) Troubleshooting (common errors, performance tips). Configure GitHub Pages to auto-publish on push to main.",
  "agent_type_hint": "DocumentationAgent",
  "inputs": "All implemented features, I2.T1 library API docs, I5.T1 FFI API docs",
  "target_files": [
    "docs/book/src/SUMMARY.md",
    "docs/book/src/intro.md",
    "docs/book/src/installation.md",
    "docs/book/src/cli_usage.md",
    "docs/book/src/library_api.md",
    "docs/book/src/ffi.md",
    "docs/book/src/formats.md",
    "docs/book/src/troubleshooting.md",
    "docs/book/book.toml",
    ".github/workflows/docs.yml"
  ],
  "input_files": [
    "docs/api/library_api.md",
    "docs/api/ffi_api.md",
    "README.md"
  ],
  "deliverables": "mdBook user guide with 8+ chapters, GitHub Pages deployment",
  "acceptance_criteria": "mdBook structure is complete (SUMMARY.md links all chapters), all chapters have substantive content (not placeholders), code examples compile and run (tested manually), mdbook build generates HTML successfully, GitHub Actions deploys to GitHub Pages on push, published site is accessible at https://<user>.github.io/exiftool-rs/",
  "dependencies": [],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: project-vision (from README.md)

ExifTool-RS aims to provide a memory-safe, zero-cost abstraction alternative to the Perl-based ExifTool while maintaining full compatibility with its extensive metadata tag support. The goal is to deliver superior performance, native cross-compilation capabilities, and seamless integration into modern software ecosystems.

**Key Features:**
- **Feature Parity**: Support for reading, writing, and editing metadata in 300+ file formats with 28,000+ recognized metadata tags
- **High Performance**: 2-5x performance improvement over Perl implementation through zero-cost abstractions and parallel processing
- **Memory Safety**: Eliminates entire classes of vulnerabilities (buffer overflows, use-after-free) through Rust's ownership system
- **Binary Distribution**: Static, self-contained binaries with no runtime dependencies
- **API-First Design**: Native Rust library with C FFI bindings for cross-language integration
- **Backward Compatibility**: CLI argument compatibility with original ExifTool for drop-in replacement scenarios
- **Cross-Platform**: Windows, Linux, macOS, and WebAssembly targets from a single codebase

### Context: architecture-overview (from README.md)

ExifTool-RS follows a **Hexagonal Architecture** (Ports and Adapters) pattern with three main layers:

- **Application Layer**: CLI interface, C FFI bindings
- **Domain Layer**: Format-agnostic metadata models and operations
- **Infrastructure Layer**: Format-specific parsers/serializers, I/O abstraction

This design ensures:
- Clean separation of concerns
- Testability of core logic independent of I/O
- Easy extensibility for new file formats
- Multiple access patterns (CLI, library API, FFI)

### Context: technology-stack (from Cargo.toml and README.md)

**Language**: Rust 1.75+ (2021 Edition)

**Key Dependencies:**
- **CLI Framework**: clap v4 (derive API for argument parsing)
- **Binary Parsing**: nom v7 (parser combinators for format parsing)
- **XML Parsing**: quick-xml (for XMP metadata)
- **JSON Output**: serde_json (serialization)
- **Date/Time**: chrono (temporal metadata)
- **String Encoding**: encoding_rs (character set conversion)
- **Memory-mapped I/O**: memmap2 (efficient large file access)
- **Concurrency**: rayon (data parallelism for batch processing)
- **Directory Traversal**: walkdir (recursive file discovery)
- **Progress Bar**: indicatif (user feedback)
- **Atomic File Operations**: tempfile (safe metadata writing)
- **CRC Calculation**: crc (PNG chunk validation)
- **CSV Output**: csv (tabular export)

**Build Dependencies:**
- **HTTP Client**: ureq (downloading ExifTool source for tag generation)
- **Regex**: regex (parsing Perl tag definitions)
- **Error Handling**: anyhow (build script error management)

**Release Optimizations:**
- opt-level = 3 (maximum optimization)
- lto = true (link-time optimization)
- codegen-units = 1 (single codegen unit for better optimization)
- strip = true (remove debug symbols)

### Context: current-status (from README.md)

**Project Status**: 🚧 Work in Progress - Pre-alpha / Active Development

**Current Version**: 0.1.0

**Completed Milestones:**
- ✅ Project structure and build system (Iteration 1)
- ✅ Core domain models (Iteration 1)
- ✅ Basic JPEG, TIFF, PNG format parsers (Iterations 1-2)
- ✅ XMP and IPTC metadata parsing (Iteration 2)
- ✅ Tag registry with 700+ tags (Iteration 2/4)
- ✅ Metadata read/write operations (Iteration 2-3)
- ✅ CLI implementation with argument parsing (Iteration 2)
- ✅ JSON, CSV, human-readable output formats (Iteration 2/4)
- ✅ JPEG, TIFF, PNG metadata writing (Iteration 3)
- ✅ Atomic file operations (Iteration 3)
- ✅ PDF and MP4/QuickTime format support (Iteration 4)
- ✅ Batch processing with recursive traversal (Iteration 4)
- ✅ Metadata copy operation (Iteration 4)
- ✅ File renaming based on metadata (Iteration 4)
- ✅ Date/time shifting (Iteration 4)
- ✅ Fuzzing infrastructure (Iteration 4)
- ✅ C FFI API (Iteration 5)
- ✅ Python bindings example (Iteration 5)
- ✅ Tag database auto-generation from ExifTool (Iteration 5)
- ✅ Cross-compilation and release builds (Iteration 5)

**In Progress:**
- 🔄 Comprehensive user guide (mdBook) - **This Task**
- 🔄 Packaging for distribution (Iteration 5)
- 🔄 Performance benchmarking vs ExifTool (Iteration 5)
- 🔄 v1.0 release preparation (Iteration 5)

### Context: deployment-targets (from architecture documentation)

**Target Platforms and Distribution:**
- **Linux** (x86_64, ARM64) - static binaries (.deb, .rpm packages)
- **macOS** (Intel, ARM/Apple Silicon) - Homebrew formula, standalone binaries
- **Windows** (x86_64) - .exe binaries, installer packages
- **WebAssembly** (WASM) - future support planned

**Distribution Methods:**
- Standalone binary downloads via GitHub Releases
- Rust crate via crates.io (`cargo install exiftool-rs`)
- Package managers (apt, dnf, brew, choco)
- C FFI library (shared/static) for language bindings

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `README.md`
    *   **Summary:** This is the project's main documentation landing page (335 lines). It provides a comprehensive overview of the project status, architecture, features, installation instructions, development workflow, tag database generation, testing, benchmarking, fuzzing, and licensing.
    *   **Recommendation:** You MUST extract and adapt content from this file for the mdBook chapters. The README already contains excellent sections on:
        - Project vision and features (lines 7-20) → intro.md
        - Architecture overview (lines 22-34) → intro.md
        - Current status (lines 36-55) → intro.md
        - Installation from source (lines 57-73) → installation.md
        - Development setup, testing, benchmarking, fuzzing (lines 110-296) → installation.md & troubleshooting.md
        - Tag database generation (lines 124-177) → installation.md
        - Technology stack (lines 316-326) → intro.md
    *   **Tip:** The README's section structure maps almost directly to your mdBook chapters. Use this as your primary content source. DO NOT rewrite from scratch.

*   **File:** `docs/api/library_api.md`
    *   **Summary:** This is a comprehensive 1401-line API reference document for the Rust library. It covers:
        - Core concepts: tag naming convention (lines 58-93), synchronous API design (lines 95-103), type safety (lines 105-127)
        - High-level API: Metadata struct, reading/writing operations (lines 129-294)
        - Low-level API: MetadataMap (lines 296-543), TagValue (lines 545-651)
        - Error handling: ExifToolError enum, Result type, error handling patterns (lines 653-915)
        - Code examples: 7 complete examples demonstrating common usage patterns (lines 917-1314)
        - Advanced topics: memory-mapped I/O, parallel processing with rayon (lines 1316-1384)
    *   **Recommendation:** You MUST reuse this content for the library_api.md chapter in mdBook. This document is production-ready and extremely thorough. Extract the most relevant sections:
        - Core Concepts section (tag naming convention, synchronous API design) → library_api.md introduction
        - Code Examples 1-7 (lines 917-1314) → library_api.md examples section
        - Error Handling Patterns section (lines 819-883) → library_api.md error handling
        - Advanced Topics (lines 1316-1384) → library_api.md advanced usage
    *   **Warning:** Many code examples are marked with `rust,ignore` because the high-level `Metadata` API is planned but not fully implemented. You MUST keep the `ignore` annotation and add a note explaining these are planned APIs. Provide working examples using the low-level `MetadataMap` and `TagValue` APIs that ARE implemented.

*   **File:** `docs/api/ffi_api.md`
    *   **Summary:** This is a comprehensive 1546-line C FFI API reference. It covers:
        - Introduction and design principles (lines 1-65)
        - Quick Start with minimal working example (lines 67-113)
        - Core Concepts: opaque handle pattern, error handling, memory ownership, thread safety (lines 115-283)
        - Complete API Reference: handle lifecycle, metadata reading/writing, tag access, error functions (lines 285-861)
        - Type Definitions: error codes, tag value types (lines 863-937)
        - Code Examples: 5 complete C examples (basic usage, error handling, iterating tags, modifying metadata, memory safety) (lines 939-1324)
        - Best Practices: 8 practical tips (lines 1326-1464)
        - Platform Notes: Linux, macOS, Windows compilation and linking (lines 1466-1530)
    *   **Recommendation:** You MUST reuse this content for the ffi.md chapter in mdBook. This document is production-ready with excellent examples. Focus on:
        - Quick Start section (lines 67-113) → ffi.md introduction
        - Core Concepts (lines 115-283) → ffi.md concepts section
        - Code Examples 1-5 (lines 939-1324) → ffi.md examples section
        - Best Practices section (lines 1326-1464) → ffi.md best practices
        - Platform Notes (lines 1466-1530) → ffi.md platform-specific instructions
    *   **Tip:** The FFI documentation is extremely thorough (1546 lines). You can extract the Quick Start and Best Practices sections directly, then link to the full API reference in `docs/api/ffi_api.md` for advanced users who need detailed function specifications.

*   **File:** `bindings/python/README.md`
    *   **Summary:** This documents the Python ctypes bindings for the C FFI (316 lines). It includes:
        - Features and prerequisites (lines 1-19)
        - Building the shared library (lines 21-34)
        - Installation and library path configuration (lines 36-66)
        - Usage examples: basic example, getting all tags, iterating tags, error handling, manual resource management (lines 68-154)
        - Running the example script (lines 156-164)
        - API reference for ExifTool class (lines 166-233)
        - Limitations of the reference implementation (lines 235-253)
        - Troubleshooting (lines 255-296)
        - Thread safety notes (lines 298-306)
    *   **Recommendation:** You SHOULD include this Python binding documentation as a subsection or example in the ffi.md chapter. It demonstrates:
        - How to use the FFI from Python (practical language binding example)
        - Context manager pattern for resource management (Pythonic API design)
        - Error handling in a high-level language
        - Library path configuration across platforms
    *   **Tip:** The Python bindings provide an excellent "proof of concept" that the FFI works. Include at least one complete Python example in the FFI chapter (from lines 72-89 of bindings/python/README.md) to show users how language bindings can be built on top of the C API.

*   **File:** `bindings/python/example.py`
    *   **Summary:** This is a working example script (158 lines) demonstrating the Python bindings with 4 complete examples:
        - Example 1: Reading EXIF metadata from a JPEG file (lines 27-74)
        - Example 2: Listing all available tags (lines 75-103)
        - Example 3: Error handling (lines 108-118)
        - Example 4: Getting all tags as a dictionary (lines 120-146)
    *   **Recommendation:** You SHOULD include a simplified version of Example 1 (lines 27-74) in the ffi.md chapter as a concrete, runnable example of using ExifTool-RS from Python. This demonstrates practical FFI usage.
    *   **Tip:** Example 1 shows the most common use case: reading camera metadata from a JPEG. Extract lines 32-69 as a standalone, documented example in your ffi.md chapter.

*   **File:** `Cargo.toml`
    *   **Summary:** This is the project's build configuration (99 lines). It defines:
        - Package metadata (lines 1-11)
        - Library and binary configuration (lines 13-24)
        - Dependencies for CLI, parsing, I/O, concurrency, etc. (lines 26-68)
        - Build dependencies for tag generation (lines 70-77)
        - Dev dependencies for testing and benchmarking (lines 79-83)
        - Profile configurations including release optimizations (lines 85-95)
        - Feature flags (lines 97-98)
    *   **Recommendation:** You MUST reference specific information from this file when documenting:
        - Installation requirements → installation.md (Rust 1.75+, specific dependencies)
        - Technology stack → intro.md (list of key dependencies with brief descriptions)
        - Building from source → installation.md (cargo build commands, profile settings)
        - Release optimizations → troubleshooting.md (performance tips section)
    *   **Tip:** The `[profile.release]` section (lines 85-89) documents the aggressive optimizations that make ExifTool-RS fast. Mention these in the Performance section of troubleshooting.md to help users understand why release builds are so much faster than debug builds.

*   **File:** `src/cli/args.rs` (not read, but exists based on directory structure)
    *   **Summary:** This file implements the CLI argument parser using clap. It defines the command-line interface.
    *   **Recommendation:** You SHOULD read this file to accurately document the CLI usage in cli_usage.md. Don't rely solely on the README's "Planned" CLI section - document what's ACTUALLY implemented.
    *   **Tip:** Use `clap`'s `--help` output as a reference for cli_usage.md. The help text generated by clap provides the canonical documentation of CLI arguments. You can capture this by running `cargo run -- --help`.

### Implementation Tips & Notes

*   **Tip - mdBook Structure**: The task requires creating a mdBook with 8+ chapters. mdBook uses a simple structure:
    - `docs/book/book.toml` - Configuration file (title, authors, language, description, GitHub link, output directory)
    - `docs/book/src/SUMMARY.md` - Table of contents (defines chapter structure and order, creates navigation)
    - `docs/book/src/*.md` - Chapter content files (individual markdown files for each chapter)

    The SUMMARY.md file MUST list all chapters in order and provide links. Example format:
    ```markdown
    # Summary

    [Introduction](intro.md)

    # User Guide

    - [Installation](installation.md)
    - [Command-Line Usage](cli_usage.md)
    - [Library API](library_api.md)

    # Advanced Topics

    - [C FFI Integration](ffi.md)
    - [Supported Formats](formats.md)
    - [Tag Reference](tags.md)
    - [Troubleshooting](troubleshooting.md)
    ```

    mdBook will automatically generate navigation based on SUMMARY.md structure. You can nest chapters with indentation.

*   **Tip - Content Reuse**: You have THREE comprehensive documentation sources to extract from:
    1. **README.md** - 335 lines with project overview, features, installation, development, current status
    2. **docs/api/library_api.md** - 1401 lines with complete Rust API reference, 7 code examples
    3. **docs/api/ffi_api.md** - 1546 lines with complete C FFI reference, 5 C examples, best practices

    **DO NOT rewrite this content from scratch.** Extract, adapt, and reorganize it for the mdBook format. You have over 3200 lines of high-quality documentation to work from.

*   **Tip - CLI Documentation**: The README has sections on CLI usage (lines 74-107), but these show "Planned" or "Coming Soon" features. You MUST:
    - Check the actual CLI implementation in `src/main.rs` and `src/cli/args.rs` to see what's REALLY implemented
    - Run `cargo run -- --help` to see the actual CLI help output from clap
    - Document the CLI features that ARE implemented based on completed tasks I2-I4 (read operations, write operations, batch processing, JSON/CSV output, file renaming, date shifting)
    - For features not yet implemented, clearly mark them as "Planned for future release"
    - Create examples that work with the current implementation, not the planned future API

*   **Tip - Supported Formats**: The README documents tag generation support for multiple format families (lines 142-154). You SHOULD:
    - Create a comprehensive list of supported formats based on the parsers in `src/parsers/` directory
    - From the directory structure, these parsers exist: JPEG, TIFF, PNG, PDF, QuickTime/MP4, XMP, IPTC
    - For each format, document:
      - File extensions (e.g., .jpg, .jpeg for JPEG)
      - Read support status (✅ Implemented, 🔄 In Progress, ⏳ Planned)
      - Write support status (✅ Implemented, ⏳ Planned)
      - Metadata types supported (EXIF, XMP, IPTC, GPS, etc.)
      - Common tags available (link to tag reference or ExifTool docs)
    - Use the tag database statistics from README lines 144-154 to show tag count per format family

*   **Note - GitHub Pages Configuration**: The task requires configuring GitHub Pages deployment. You MUST create `.github/workflows/docs.yml` with a workflow that:
    1. Triggers on push to `main` branch and manual workflow dispatch
    2. Installs mdBook (use official GitHub Actions: `peaceiris/actions-mdbook@v1`)
    3. Runs `mdbook build docs/book` to generate HTML output
    4. Deploys the `docs/book/book/` directory to GitHub Pages using `peaceiris/actions-gh-pages@v3`
    5. Handles permissions correctly (needs `contents: write` permission)
    6. Uses deployment environment for tracking (optional but recommended)

    Reference the existing `.github/workflows/ci.yml` for workflow structure patterns. The docs workflow should be simpler - just build and deploy, no testing.

*   **Warning - Code Examples**: Many code examples in `docs/api/library_api.md` are marked with ` ```rust,ignore` because they reference planned but unimplemented APIs (like the high-level `Metadata` struct). When you include these examples in mdBook:
    - Keep the `ignore` annotation so mdBook won't try to test/compile them
    - Add a prominent note at the top of library_api.md explaining: "Note: Some APIs shown in examples are planned for future implementation. Working examples using the current low-level API are provided in the Examples section."
    - Provide at least 2-3 working examples using the current low-level API (`MetadataMap`, `TagValue`, operations from `src/core/operations.rs`)
    - Test these working examples manually before including them in the documentation

*   **Note - Internal Links**: mdBook supports internal linking between chapters using relative paths. You SHOULD use this to create a well-connected user guide:
    - Link from Installation → CLI Usage → Library API (progressive learning path)
    - Link from Library API → FFI (for users building language bindings)
    - Link from CLI Usage and Library API → Tag Reference and Troubleshooting (reference material)
    - Link from Troubleshooting → specific chapters where features are explained in detail
    - Use descriptive link text: `[CLI usage guide](cli_usage.md)` not just "click here"
    - Test all internal links after building to ensure they work

*   **Tip - Project Maturity**: The project is at v0.1.0 in active pre-alpha development. Your documentation MUST:
    - Set realistic expectations by clearly stating "Work in Progress - Pre-alpha Development" prominently in intro.md
    - Distinguish between implemented features (tasks I1-I5.T6 complete) and planned features (I5.T7+ incomplete, future iterations)
    - Provide a status indicator for each major feature using emoji: ✅ Implemented, 🔄 In Progress, ⏳ Planned
    - Include a "Current Status" section in intro.md showing the roadmap from README lines 36-55
    - Direct users to GitHub Issues for reporting bugs, requesting features, and tracking development progress
    - Be honest about limitations: "This is a reimplementation of ExifTool. Not all features from the Perl version are available yet."

*   **Recommendation - Testing**: After creating the mdBook, you MUST verify:
    1. Run `mdbook build docs/book` completes without errors or warnings
    2. Open `docs/book/book/index.html` in a browser and verify the site renders correctly
    3. Check all internal links work (click through every link in the navigation)
    4. Verify code examples have proper syntax highlighting (Rust, C, Python, bash)
    5. Test on both light and dark themes (mdBook supports both)
    6. Verify SUMMARY.md table of contents matches the chapter files exactly
    7. Check that navigation (prev/next links) works between chapters

    Use `mdbook serve docs/book` to preview the site locally during development. This auto-reloads on file changes, making iteration fast.

*   **Note - mdBook Configuration (book.toml)**: The book.toml file MUST include:
    ```toml
    [book]
    title = "ExifTool-RS User Guide"
    authors = ["ExifTool-RS Contributors"]
    language = "en"
    multilingual = false
    src = "src"
    description = "Comprehensive guide to using ExifTool-RS for reading, writing, and editing metadata in 300+ file formats"

    [build]
    build-dir = "book"

    [output.html]
    default-theme = "light"
    preferred-dark-theme = "navy"
    git-repository-url = "https://github.com/exiftool-rs/exiftool-rs"
    git-repository-icon = "fa-github"
    edit-url-template = "https://github.com/exiftool-rs/exiftool-rs/edit/main/docs/book/{path}"

    [output.html.search]
    enable = true
    limit-results = 30
    use-boolean-and = true
    ```

    This configuration enables: search functionality, GitHub link in header, edit button on each page, responsive themes.

### Success Criteria Summary

To complete this task successfully, you MUST deliver:

1. **Complete mdBook structure** with all 8 required chapters:
   - intro.md (introduction, vision, features, architecture, status)
   - installation.md (binary download, cargo install, build from source)
   - cli_usage.md (CLI commands, arguments, examples)
   - library_api.md (Rust API reference, examples)
   - ffi.md (C FFI, Python bindings, examples)
   - formats.md (supported formats, extensions, capabilities)
   - tags.md (tag reference, link to ExifTool docs)
   - troubleshooting.md (common errors, performance tips)

2. **SUMMARY.md** with proper table of contents linking all chapters

3. **book.toml** configuration file with proper metadata and settings

4. **.github/workflows/docs.yml** workflow for GitHub Pages deployment

5. **Substantive content** in all chapters (minimum 100 lines per chapter, extracted from existing docs)

6. **Working code examples** tested manually (at least 2-3 examples using current low-level API)

7. **Successful mdbook build** that generates HTML without errors

8. **GitHub Pages deployment** that publishes the site and makes it accessible

Good luck! This is a documentation task focused on creating a comprehensive, user-friendly guide by extracting and reorganizing existing high-quality documentation.
