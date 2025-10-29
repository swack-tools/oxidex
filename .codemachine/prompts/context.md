# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I2.T1",
  "iteration_id": "I2",
  "iteration_goal": "Implement tag registry with subset of ExifTool tags, core metadata read/write operations, basic CLI with argument parsing, and extend format support to include XMP parsing and PNG format.",
  "description": "Create comprehensive Markdown documentation for the Rust library API in docs/api/library_api.md. Document public API surface: Metadata::from_path(), Metadata::from_bytes(), MetadataMap accessors (get_string(), get_i64(), get_f64(), get_datetime(), iter_tags()), builder pattern for write operations, error types and handling. Include Rust code examples for common use cases: extract all tags, get specific tag, modify tag value, copy metadata between files. Reference tag naming convention (e.g., EXIF:Make, XMP:Creator).",
  "agent_type_hint": "DocumentationAgent",
  "inputs": "Section 2 (API Contract Style), Section 2.1 (Key Architectural Artifacts), I1.T6 core models",
  "target_files": ["docs/api/library_api.md"],
  "input_files": ["src/core/metadata_map.rs", "src/core/tag_value.rs", "src/error.rs"],
  "deliverables": "Comprehensive API documentation in Markdown, at least 5 code examples",
  "acceptance_criteria": "Document covers all major API functions, code examples compile (can be tested with cargo test --doc later), tag naming convention clearly explained, error handling patterns documented, well-formatted Markdown with table of contents",
  "dependencies": ["I1.T6"],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: api-style (from 04_Behavior_and_Communication.md)

```markdown
#### API Style

**Primary API**: **Rust Library API** (procedural + builder pattern)

The core API is designed for Rust consumers and follows idiomatic patterns:

```rust
use exiftool_rs::{Metadata, FileFormat};

// Simple extraction
let metadata = Metadata::from_path("photo.jpg")?;
let camera_model = metadata.get_string("EXIF:Model")?;

// Builder pattern for complex operations
let result = Metadata::from_path("input.jpg")?
    .copy_tags_to("output.jpg")?
    .with_tags(&["EXIF:DateTime", "EXIF:Make", "EXIF:Model"])
    .preserve_file_times(true)
    .execute()?;
```

**Secondary APIs**:

1. **CLI Interface**: POSIX-style arguments mimicking ExifTool
   ```bash
   exiftool-rs -EXIF:DateTime photo.jpg
   exiftool-rs -json -r /photos/  # Recursive JSON output
   exiftool-rs -TagsFromFile src.jpg -all:all dest.jpg  # Copy metadata
   ```

2. **C FFI**: Minimal C-compatible surface for foreign language bindings
   ```c
   // C API example
   ExifToolHandle* handle = exiftool_create();
   ExifToolError err = exiftool_read_file(handle, "photo.jpg");
   const char* model = exiftool_get_string(handle, "EXIF:Model");
   exiftool_destroy(handle);
   ```

**Justification**:

- **Rust-First**: Leverages Rust's type system for compile-time safety (no invalid tag names at compile time via const tag identifiers)
- **No Network API**: ExifTool-RS is a library/tool, not a service. REST/GraphQL APIs would be implemented by consuming applications
- **FFI for Interop**: Enables Python (`pyo3`), Node.js (`neon`), Go (`cgo`) bindings without compromising Rust API ergonomics
```

### Context: communication-patterns (from 04_Behavior_and_Communication.md)

```markdown
#### Communication Patterns

**Primary Pattern**: **Synchronous Request/Response**

All operations are synchronous:
1. User/application calls API function
2. Function parses file, extracts/modifies metadata
3. Function returns result or error
4. Transaction completes

**Rationale**:
- File I/O is the bottleneck, not computation. Async overhead provides no benefit.
- Synchronous code is simpler to reason about for library consumers.
- Batch parallelism is achieved via `rayon` at the application level (parallel iterator over file list), not async/await.

**Batch Processing**: Uses data parallelism (not message passing)

```rust
use rayon::prelude::*;

let results: Vec<Result<Metadata>> = file_paths
    .par_iter()  // Rayon parallel iterator
    .map(|path| Metadata::from_path(path))
    .collect();
```

Rayon's work-stealing scheduler distributes file processing across CPU cores automatically.

**Error Handling**: `Result<T, ExifToolError>` throughout

```rust
pub enum ExifToolError {
    IoError(std::io::Error),
    ParseError { format: String, details: String },
    TagNotFound { tag_name: String },
    InvalidTagValue { tag_name: String, expected_type: String },
    UnsupportedFormat { format: String },
}
```

Errors propagate via `?` operator, no exceptions.
```

### Context: api-contract-style (from 01_Plan_Overview_and_Setup.md)

```markdown
*   **API Contract Style:**
    *   **Primary:** Rust Library API (procedural + builder pattern)
        ```rust
        let metadata = Metadata::from_path("photo.jpg")?;
        let camera = metadata.get_string("EXIF:Model")?;
        ```
    *   **Secondary:** CLI (POSIX-style arguments, ExifTool-compatible)
        ```bash
        exiftool-rs -EXIF:DateTime photo.jpg
        exiftool-rs -json -r /photos/
        ```
    *   **Tertiary:** C FFI (minimal C-compatible surface)
        ```c
        ExifToolHandle* h = exiftool_create();
        exiftool_read_file(h, "photo.jpg");
        ```

    *See API Specification (Section 2.1, Iteration 2, Task 1)*
```

### Context: technology-stack-summary (from 02_Architecture_Overview.md)

```markdown
### 3.2. Technology Stack Summary

| **Category** | **Technology Choice** | **Justification** |
|--------------|----------------------|-------------------|
| **Core Language** | Rust 1.75+ (2021 Edition) | Memory safety, zero-cost abstractions, excellent concurrency primitives, cross-platform support |
| **CLI Framework** | `clap` v4 (derive API) | Industry standard, excellent help generation, argument validation, backward compatibility via value parsers |
| **Binary Parsing** | `nom` v7 + `binrw` | `nom` for complex formats (TIFF, QuickTime), `binrw` for simple struct-based formats (BMP, WAV) |
| **XML Parsing (XMP)** | `quick-xml` | Streaming parser, low memory footprint, namespace support for XMP |
| **JSON Output** | `serde_json` | De facto standard, excellent performance, integration with domain models via derives |
| **Date/Time** | `chrono` | Comprehensive timezone support, EXIF date format parsing |
| **String Encoding** | `encoding_rs` (WHATWG standard) | Handles legacy encodings in IPTC/EXIF (Latin1, UTF-8, UTF-16) |
| **Image I/O** | `memmap2` (memory-mapped files) | Efficient large file access without loading entire file into memory |
| **Concurrency** | `rayon` (data parallelism) | Transparent batch processing parallelization, work-stealing scheduler |
| **Testing** | `cargo test` + `proptest` (property-based) | Unit tests for parsers, property-based testing for round-trip serialization |
| **Fuzzing** | `cargo-fuzz` (libFuzzer) | Continuous fuzzing of format parsers to discover crash/hang bugs |
| **C FFI** | `cbindgen` (header generation) | Automated C header generation from Rust API |
| **Documentation** | `rustdoc` + `mdBook` (user guide) | API docs from source comments, separate user guide for CLI |
| **Build System** | `cargo` + `cross` (cross-compilation) | Standard Rust tooling, `cross` for ARM/Windows builds from Linux |
| **CI/CD** | GitHub Actions | Free for open source, matrix builds across OS/architecture |
| **Code Quality** | `clippy`, `rustfmt`, `cargo-audit` | Linting, formatting, dependency vulnerability scanning |
| **Benchmarking** | `criterion` | Statistical benchmarking framework, regression detection |
```

### Context: data-model-overview (from 01_Plan_Overview_and_Setup.md)

```markdown
*   **Data Model Overview:**
    *   **File:** Represents media file being processed (path, format, size)
    *   **MetadataMap:** Collection of all tags extracted from a file
    *   **TagValue:** Single metadata tag with name, value, type information, and optional byte offset
    *   **TagDescriptor:** Tag definition from database (ID, name, type constraints, format family)
    *   **FormatFamily:** Grouping of metadata standards (EXIF, XMP, IPTC, MakerNotes)
    *   **IFD (Image File Directory):** TIFF-specific structural element for tag organization

    *See ERD (Section 2.1, Iteration 1, Task 3)*

    **Note:** No persistent database storage. All data structures are in-memory during processing, serialized to JSON/text output or written back to file metadata.
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/metadata_map.rs`
    *   **Summary:** This file implements the `MetadataMap` struct, which is the core data structure for storing metadata. It wraps a `HashMap<String, TagValue>` and provides typed accessor methods like `get_string()`, `get_integer()`, `get_float()`. It has full serde support for JSON serialization. The file has comprehensive unit tests covering all functionality.
    *   **Recommendation:** Your API documentation MUST reference the existing methods on `MetadataMap`: `new()`, `insert()`, `get()`, `get_string()`, `get_integer()`, `get_float()`, `iter()`, `keys()`, `values()`, `len()`, `is_empty()`. These are already implemented and tested. Note that the task description mentions methods like `get_i64()`, `get_f64()`, and `get_datetime()` - these DO NOT yet exist. The current methods are `get_integer()` (returns `Option<i64>`), `get_float()` (returns `Option<f64>`), but there is no `get_datetime()` method yet.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** This file defines the `TagValue` enum with variants: `String`, `Integer`, `Float`, `Rational`, `Binary`, `DateTime`, and `Struct`. Each variant has constructors (e.g., `new_string()`, `new_integer()`) and type checkers (e.g., `is_string()`, `is_integer()`) plus type accessors (e.g., `as_string()`, `as_integer()`). It uses serde with `#[serde(tag = "type", content = "value")]` for JSON serialization.
    *   **Recommendation:** Your API documentation MUST explain how `TagValue` works and how users can work with the different value types. Document the enum variants and the accessor methods. Note that `DateTime` uses `chrono::DateTime<Utc>` internally.

*   **File:** `src/error/mod.rs`
    *   **Summary:** This file implements the `ExifToolError` enum with variants: `IoError`, `ParseError`, `TagNotFound`, `InvalidTagValue`, and `UnsupportedFormat`. It implements `std::error::Error` and `Display` traits. There are helper constructors like `parse_error()`, `tag_not_found()`, etc. There's also a type alias `Result<T> = std::result::Result<T, ExifToolError>`.
    *   **Recommendation:** Your API documentation MUST include a section on error handling. Explain all the error variants and when they occur. Document the `Result<T>` type alias pattern used throughout the library. Show examples of error handling with the `?` operator.

*   **File:** `src/core/tag_descriptor.rs`
    *   **Summary:** This file defines `TagDescriptor` (containing tag metadata like ID, name, format family, type, description), `TagId` (enum for numeric or named IDs), `FormatFamily` (enum for EXIF, XMP, IPTC, GPS, etc.), and `ValueType` (enum for String, Integer, Float, etc.). These are used to describe tags in the tag registry.
    *   **Recommendation:** While users won't typically create `TagDescriptor` objects directly, your API documentation should mention the tag naming convention that uses the format family prefix (e.g., "EXIF:Make", "XMP:Creator", "GPS:Latitude"). This is central to how tags are identified in the API.

*   **File:** `src/core/operations.rs`
    *   **Summary:** This file currently has only a module comment and an `#![allow(dead_code)]` directive. It's essentially empty - the actual read/write operations have not been implemented yet.
    *   **Recommendation:** Your API documentation will describe functions that DON'T YET EXIST, such as `Metadata::from_path()`, `Metadata::from_bytes()`, and builder patterns for write operations. This is intentional - you are documenting the PLANNED API before implementation. Make sure your code examples follow Rust idioms and are consistent with the architecture's design (synchronous, Result-based error handling, builder pattern for complex operations).

*   **File:** `src/lib.rs`
    *   **Summary:** The library root file that defines the module structure and exports the public API. It currently exports `cli`, `ffi`, `core`, `io`, `parsers`, `writers`, `error`, and `tag_db` modules. The core types are re-exported from `src/core/mod.rs`.
    *   **Recommendation:** When you document the API, consider that users will typically import types from the root crate (`use exiftool_rs::core::MetadataMap;`) or they might use the `Metadata` struct (not yet implemented) directly from the root.

### Implementation Tips & Notes

*   **Tip:** The architecture blueprint shows example code with a `Metadata` struct that provides methods like `from_path()` and a builder pattern. However, I found NO such struct in the current codebase. You are documenting a FUTURE API. The current code only has `MetadataMap`, `TagValue`, and related types. Your documentation should describe the planned `Metadata` API that will wrap these lower-level types.

*   **Note:** The tag naming convention follows the pattern `<FormatFamily>:<TagName>`, e.g., "EXIF:Make", "XMP-dc:Creator", "GPS:Latitude". This convention MUST be clearly explained in your documentation, as it's how users will identify tags.

*   **Note:** The architecture emphasizes a **builder pattern** for write operations. Your documentation should show examples of this pattern, even though the implementation doesn't exist yet. For example:
    ```rust
    Metadata::from_path("input.jpg")?
        .set_tag("EXIF:Artist", "John Doe")?
        .set_tag("EXIF:Copyright", "2025")?
        .write_to("output.jpg")?;
    ```

*   **Note:** The current `MetadataMap` methods are:
    - `get_string()` → returns `Option<&str>`
    - `get_integer()` → returns `Option<i64>`
    - `get_float()` → returns `Option<f64>`

    But the task description asks you to document:
    - `get_i64()` (same as `get_integer()`?)
    - `get_f64()` (same as `get_float()`?)
    - `get_datetime()` (NOT implemented)
    - `iter_tags()` (currently just `iter()`)

    You should either (a) document the existing method names, or (b) document the planned method names that will be added. I recommend documenting both: show the existing methods AND note that convenience aliases or additional methods are planned.

*   **Tip:** The acceptance criteria says "Code examples compile (can be tested with cargo test --doc later)". This means you should write your examples in a way that COULD be tested with rustdoc tests, even if they won't compile yet because the `Metadata` API isn't implemented. Use the `rust,ignore` tag in your code blocks for examples that reference unimplemented APIs, or use `rust,no_run` for examples that compile but shouldn't be executed.

*   **Warning:** The task says to include "at least 5 code examples". Make sure you cover diverse use cases:
    1. Simple extraction (read metadata from file)
    2. Get specific tag value
    3. Modify tag value (write operation)
    4. Copy metadata between files
    5. Batch processing with error handling

    Show both the high-level `Metadata` API and the lower-level `MetadataMap` API where appropriate.

*   **Tip:** The acceptance criteria requires "Well-formatted Markdown with table of contents". Make sure you structure your documentation with clear sections, headers, and a TOC at the top. Consider sections like:
    - Introduction
    - Core Concepts
    - API Reference
    - Code Examples
    - Error Handling
    - Tag Naming Convention
