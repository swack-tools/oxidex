# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I1.T9",
  "iteration_id": "I1",
  "iteration_goal": "Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.",
  "description": "Implement format detection logic in src/parsers/format_detector.rs. Read first 16 bytes (magic bytes) from file using FileReader trait. Implement detection for JPEG (0xFF 0xD8 0xFF), TIFF (little-endian: 0x49 0x49 0x2A 0x00, big-endian: 0x4D 0x4D 0x00 0x2A), PNG (0x89 0x50 0x4E 0x47), PDF (0x25 0x50 0x44 0x46). Return FileFormat enum. Add unit tests with sample magic bytes.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I1.T7 FileFormat enum, I1.T8 FileReader implementations",
  "target_files": [
    "src/parsers/format_detector.rs",
    "src/parsers/mod.rs"
  ],
  "input_files": [
    "src/core/file_format.rs",
    "src/io/file_reader.rs"
  ],
  "deliverables": "Format detection function: fn detect_format(reader: &dyn FileReader) -> Result<FileFormat>, unit tests for each supported format",
  "acceptance_criteria": "Function correctly identifies JPEG, TIFF (both endianness), PNG, PDF from magic bytes, unknown formats return FileFormat::Unknown, error handling for files smaller than 16 bytes, unit tests cover all supported formats plus Unknown case, cargo test passes",
  "dependencies": [
    "I1.T7",
    "I1.T8"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: Hexagonal Architecture - Ports and Adapters (from 02_Architecture_Overview.md)

```markdown
**Primary Style**: **Layered Hexagonal Architecture** (Ports and Adapters)

**Rationale**:

The Hexagonal Architecture pattern is optimal for ExifTool-RS because:

1. **Format Independence**: The "core domain" (metadata extraction/manipulation logic) must remain isolated from the specifics of 300+ file formats. Hexagonal architecture enforces this separation through ports (interfaces) and adapters (format-specific implementations).

2. **Multiple Access Patterns**: The system must expose:
   - CLI interface (primary port)
   - Rust library API (primary port)
   - C FFI bindings (primary port)
   - Format parsers (secondary ports)
   - File system access (secondary port)

   This multiplicity of interfaces aligns perfectly with the ports/adapters model.

3. **Testability**: Hexagonal architecture enables testing the core metadata logic independently of file I/O by mocking the file system port. Critical for achieving 80%+ test coverage.

4. **Extensibility**: New file format support becomes a matter of implementing the format adapter interface without touching core logic. Supports phased rollout strategy (50 formats in v1.0, expanding to 300+).

**Layered Structure**:

```
┌─────────────────────────────────────────────┐
│  Application Layer (CLI, FFI, Library API) │  ← Primary Adapters
├─────────────────────────────────────────────┤
│       Domain Layer (Metadata Engine)        │  ← Core Business Logic
├─────────────────────────────────────────────┤
│  Infrastructure Layer (Format Parsers, I/O) │  ← Secondary Adapters
└─────────────────────────────────────────────┘
```

- **Domain Layer**: Format-agnostic metadata models, tag definitions, operations (read/write/copy/transform)
- **Application Layer**: User-facing interfaces translating commands to domain operations
- **Infrastructure Layer**: Format-specific parsers/serializers, file system abstraction, configuration
```

### Context: Technology Stack - Binary Parsing (from 02_Architecture_Overview.md)

```markdown
| **Category** | **Technology Choice** | **Justification** |
|--------------|----------------------|-------------------|
| **Binary Parsing** | `nom` v7 + `binrw` | `nom` for complex formats (TIFF, QuickTime), `binrw` for simple struct-based formats (BMP, WAV) |
| **Image I/O** | `memmap2` (memory-mapped files) | Efficient large file access without loading entire file into memory |

**Key Libraries Detail**:

- **`nom` v7**: Parser combinator library for binary formats. Example: TIFF IFD parsing uses `nom::number::complete::le_u16` for little-endian u16, chained with `nom::multi::count` for tag array parsing.

- **`memmap2`**: Memory-mapped files via `Mmap::map(&file)`. Enables zero-copy parsing for formats with known offsets (JPEG EXIF segment, PNG chunks).

**Dependency Philosophy**:
- **Minimize Count**: Target < 50 direct dependencies to reduce supply chain risk
- **Prefer `no_std` Compatible**: Where possible (e.g., `nom`, `binrw`) to enable future embedded/WASM use
- **Audit Regularly**: `cargo-audit` in CI pipeline to catch vulnerabilities in transitive dependencies
```

### Context: Component Diagram - Infrastructure Layer (from 03_System_Structure_and_Data.md)

```markdown
Container_Boundary(core_lib, "Core Library") {

  Component(api_facade, "Public API Facade", "Rust modules", "User-facing API: extract(), write(), copy_metadata()")

  ' Domain Layer
  Component(metadata_model, "Metadata Model", "Rust structs/enums", "TagValue, MetadataMap, TagDescriptor")
  Component(operations, "Metadata Operations", "Rust traits/impls", "Read, Write, Copy, Transform operations")
  Component(tag_registry, "Tag Registry", "Generated const maps", "28K+ tag definitions indexed by ID/name")
  Component(validation, "Validation Engine", "Rust", "Tag value type checking, range validation")

  ' Ports (interfaces)
  Component(format_port, "Format Parser Port", "Rust trait", "trait FormatParser { fn parse(&self, ...) -> Result<MetadataMap> }")
  Component(io_port, "I/O Port", "Rust trait", "trait FileReader { fn read(&self, offset, len) -> Result<&[u8]> }")

  ' Infrastructure adapters (in other containers but shown for clarity)
  Component_Ext(jpeg_adapter, "JPEG Parser", "nom-based", "EXIF/JFIF segment parser")
  Component_Ext(tiff_adapter, "TIFF Parser", "nom-based", "IFD structure parser")
  Component_Ext(xmp_adapter, "XMP Parser", "quick-xml", "RDF/XML parser for XMP")
  Component_Ext(mmap_adapter, "MMap Reader", "memmap2", "Memory-mapped file access")
}

Rel(operations, format_port, "Calls")
Rel(format_port, jpeg_adapter, "Implemented by")
Rel(format_port, tiff_adapter, "Implemented by")
Rel(jpeg_adapter, io_port, "Reads via")
Rel(tiff_adapter, io_port, "Reads via")
Rel(io_port, mmap_adapter, "Implemented by")
```

**Note**: Format detection is the first step in the parsing pipeline. It reads magic bytes using the FileReader port and returns a FileFormat enum, which is then used to route to the appropriate format parser adapter.

### Context: Task I1.T9 Specification (from 02_Iteration_I1.md)

```markdown
*   **Task 1.9: Implement Format Detector**
    *   **Task ID:** `I1.T9`
    *   **Description:** Implement format detection logic in `src/parsers/format_detector.rs`. Read first 16 bytes (magic bytes) from file using FileReader trait. Implement detection for JPEG (0xFF 0xD8 0xFF), TIFF (little-endian: 0x49 0x49 0x2A 0x00, big-endian: 0x4D 0x4D 0x00 0x2A), PNG (0x89 0x50 0x4E 0x47), PDF (0x25 0x50 0x44 0x46). Return FileFormat enum. Add unit tests with sample magic bytes.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** I1.T7 FileFormat enum, I1.T8 FileReader implementations
    *   **Input Files:** [`src/core/file_format.rs`, `src/io/file_reader.rs`]
    *   **Target Files:**
        *   `src/parsers/format_detector.rs`
        *   `src/parsers/mod.rs`
    *   **Deliverables:**
        *   Format detection function: `fn detect_format(reader: &dyn FileReader) -> Result<FileFormat>`
        *   Unit tests for each supported format
    *   **Acceptance Criteria:**
        *   Function correctly identifies JPEG, TIFF (both endianness), PNG, PDF from magic bytes
        *   Unknown formats return FileFormat::Unknown
        *   Error handling for files smaller than 16 bytes
        *   Unit tests cover all supported formats plus Unknown case
        *   `cargo test` passes
    *   **Dependencies:** `I1.T7`, `I1.T8`
    *   **Parallelizable:** No (depends on T7, T8)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/file_format.rs`
    *   **Summary:** This file defines the `FileFormat` enum with 11 variants (JPEG, TIFF, PNG, PDF, GIF, BMP, QuickTime, HEIF, WebP, RAW, Unknown). The enum is `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]` and includes helper methods `name()` and `extensions()` that return static string slices.
    *   **Recommendation:** You MUST use this exact `FileFormat` enum as the return type for your `detect_format()` function. Import it with `use crate::core::FileFormat;` at the top of `format_detector.rs`.
    *   **Key Implementation Detail:** The enum already has all necessary format variants defined, including `FileFormat::Unknown` for unrecognized formats.

*   **File:** `src/core/file_reader_trait.rs`
    *   **Summary:** This file defines the `FileReader` trait with two methods: `fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]>` and `fn size(&self) -> u64`. The trait is object-safe and designed for use with `&dyn FileReader`. Comprehensive documentation explains that implementations must return borrowed slices, handle out-of-bounds gracefully, and maintain thread safety.
    *   **Recommendation:** Your `detect_format()` function MUST accept `&dyn FileReader` as a parameter. Call `reader.read(0, 16)?` to read the first 16 bytes for magic byte detection. You MUST handle the case where the file is smaller than 16 bytes - don't panic if `read()` returns an error.
    *   **Error Handling Pattern:** The trait returns `io::Result`, so you can use the `?` operator to propagate errors. However, if a file is smaller than 16 bytes, you may want to try reading fewer bytes or return `FileFormat::Unknown` rather than propagating the error.

*   **File:** `src/io/mmap_reader.rs`
    *   **Summary:** This file implements `MMapReader`, a concrete adapter for the `FileReader` trait using memory-mapped I/O via `memmap2::Mmap`. The implementation includes extensive unit tests (16 test functions) covering edge cases like empty files, out-of-bounds reads, overflow handling, and zero-byte reads.
    *   **Recommendation:** Study the test patterns in this file as a model for your own unit tests. Note how tests use `NamedTempFile` from the `tempfile` crate to create test files with controlled content. You SHOULD follow the same testing pattern for format detection, though for format detection you can use simpler in-memory test readers rather than actual files.
    *   **Test Coverage Insight:** The MMapReader tests demonstrate the project's high standards for error handling and edge case coverage. Your format detector tests should match this thoroughness.

*   **File:** `src/io/buffered_reader.rs`
    *   **Summary:** Another `FileReader` implementation for buffered I/O. Your format detector will work with both implementations transparently via the `FileReader` trait.
    *   **Recommendation:** No direct interaction needed. The trait abstraction ensures your code works with any `FileReader` implementation.

*   **File:** `src/parsers/format_detector.rs`
    *   **Summary:** Currently contains only a module-level doc comment and `#![allow(dead_code)]`. This is your primary target file - it's essentially a blank canvas.
    *   **Recommendation:** You MUST implement the complete format detection logic here, including the public function `pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat>` and comprehensive unit tests in a `#[cfg(test)] mod tests` block.

*   **File:** `src/parsers/mod.rs`
    *   **Summary:** Module file that exports submodules: `pub mod common;`, `pub mod format_detector;`, `pub mod jpeg;`, `pub mod png;`, `pub mod tiff;`, `pub mod xmp;`.
    *   **Recommendation:** The `format_detector` module is already exported. You MAY want to add a re-export of the `detect_format` function to make it accessible as `parsers::detect_format` (e.g., `pub use format_detector::detect_format;`), but this is optional. The acceptance criteria don't explicitly require it.

### Implementation Tips & Notes

*   **Tip:** **Magic Byte Detection Strategy**
    *   JPEG: Check bytes [0, 1, 2] == [0xFF, 0xD8, 0xFF] - Note that all three bytes are required, not just 0xFFD8
    *   TIFF Little-Endian: bytes [0, 1, 2, 3] == [0x49, 0x49, 0x2A, 0x00] (ASCII "II" + magic number 42)
    *   TIFF Big-Endian: bytes [0, 1, 2, 3] == [0x4D, 0x4D, 0x00, 0x2A] (ASCII "MM" + magic number 42)
    *   PNG: bytes [0, 1, 2, 3] == [0x89, 0x50, 0x4E, 0x47] (first 4 bytes of PNG signature)
    *   PDF: bytes [0, 1, 2, 3] == [0x25, 0x50, 0x44, 0x46] (ASCII "%PDF")

    Create helper functions or use pattern matching to check these byte sequences clearly. The full PNG signature is 8 bytes but checking the first 4 is sufficient for this task.

*   **Tip:** **Error Handling for Small Files**
    *   The task requires handling files smaller than 16 bytes gracefully. I recommend this approach:
    ```rust
    let magic_bytes = match reader.read(0, 16) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            // File is smaller than 16 bytes, try reading what's available
            let size = reader.size() as usize;
            if size == 0 {
                return Ok(FileFormat::Unknown);
            }
            reader.read(0, size)?
        }
        Err(e) => return Err(e),
    };
    ```
    Then check `magic_bytes.len()` and only test formats that fit within the available bytes.

*   **Tip:** **Testing with In-Memory Data**
    *   You don't need to create actual files for unit tests. Create a simple test implementation of `FileReader`:
    ```rust
    #[cfg(test)]
    struct TestReader {
        data: Vec<u8>,
    }

    #[cfg(test)]
    impl FileReader for TestReader {
        fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
            let start = offset as usize;
            let end = start.saturating_add(length).min(self.data.len());
            if start > self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "offset beyond end of data"
                ));
            }
            if end > self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "read beyond end of data"
                ));
            }
            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }
    ```

*   **Note:** **Module Organization**
    *   The `src/parsers/` directory follows a clear pattern: each format has its own submodule (jpeg, tiff, png, xmp), plus a `common` module for shared utilities and a `format_detector` module for format detection.
    *   Format detection is the **first step** in the parsing pipeline. Later tasks (I1.T10, I1.T11) will implement actual parsers that are selected based on the format detected by this module.

*   **Note:** **Return Type Signature**
    *   The task specifies `fn detect_format(reader: &dyn FileReader) -> Result<FileFormat>`, but "Result" without a specific error type is ambiguous. Based on the FileReader trait returning `io::Result`, I recommend:
    ```rust
    pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat>
    ```
    This allows you to use `?` to propagate I/O errors from `reader.read()` calls. The function should return `Ok(FileFormat::Unknown)` for unrecognized formats, NOT an error.

*   **Warning:** **Magic Byte Array Indexing**
    *   When checking magic bytes, ensure you don't panic on short reads. Always check `magic_bytes.len()` before indexing or use slice comparison:
    ```rust
    if magic_bytes.len() >= 4 && &magic_bytes[0..4] == b"\x89PNG" {
        return Ok(FileFormat::PNG);
    }
    ```
    Or even better, use pattern matching with slice patterns:
    ```rust
    if magic_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Ok(FileFormat::PNG);
    }
    ```

*   **Tip:** **Test Coverage Requirements**
    *   Based on the acceptance criteria and the thoroughness of existing tests (see `mmap_reader.rs`), you SHOULD write at least these test cases:
        1. `test_detect_jpeg` - Valid JPEG magic bytes
        2. `test_detect_tiff_little_endian` - TIFF LE magic bytes
        3. `test_detect_tiff_big_endian` - TIFF BE magic bytes
        4. `test_detect_png` - PNG magic bytes
        5. `test_detect_pdf` - PDF magic bytes
        6. `test_detect_unknown` - Random bytes that don't match any format
        7. `test_empty_file` - Empty file (size 0)
        8. `test_file_too_small` - File with 1-2 bytes (smaller than smallest magic)
        9. `test_short_file_matches_format` - File with exactly 4 bytes that match a format

    This gives comprehensive coverage of success paths, unknown formats, and edge cases.

*   **Tip:** **Order of Format Checks**
    *   Check formats in order from most specific to least specific. For example:
        1. Check 4-byte signatures (TIFF, PNG, PDF) first
        2. Check JPEG (3 bytes) afterward
        3. Return `FileFormat::Unknown` as the fallback

    This ensures more specific formats aren't masked by shorter, less specific checks. Consider using a match statement or if-else chain that checks longer signatures first.

*   **Critical Architecture Note:** The format detector is part of the **infrastructure layer** and uses the **FileReader secondary port**. It should have NO dependencies on domain layer code except `FileFormat` (which is technically a shared kernel type). Do NOT import `ExifToolError` or metadata models - use `std::io::Error` directly.

*   **Recommended Function Signature:**
    ```rust
    /// Detects the file format by examining magic bytes.
    ///
    /// This function reads the first 16 bytes of the file (or fewer if the file is smaller)
    /// and matches them against known format signatures.
    ///
    /// # Arguments
    ///
    /// * `reader` - A file reader providing access to file contents
    ///
    /// # Returns
    ///
    /// * `Ok(FileFormat)` - The detected format, or `FileFormat::Unknown` if unrecognized
    /// * `Err(io::Error)` - An I/O error occurred while reading the file
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use exiftool_rs::parsers::format_detector::detect_format;
    /// use exiftool_rs::io::MMapReader;
    /// use std::path::Path;
    ///
    /// # fn example() -> std::io::Result<()> {
    /// let reader = MMapReader::new(Path::new("image.jpg"))?;
    /// let format = detect_format(&reader)?;
    /// println!("Detected format: {}", format);
    /// # Ok(())
    /// # }
    /// ```
    pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat>
    ```

*   **Recommended Implementation Structure:**
    1. Try to read 16 bytes, fall back to reading available bytes if file is smaller
    2. Check formats using `starts_with()` or slice comparison
    3. Return first match, or `FileFormat::Unknown` if no match
    4. Never panic - all error paths should return `Err` or `Ok(Unknown)`
