# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T1",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Design C-compatible FFI API in docs/api/ffi_api.md. Define: (1) Handle-based lifecycle (exiftool_create(), exiftool_destroy()), (2) Error handling (return codes, exiftool_get_last_error()), (3) Metadata reading (exiftool_read_file(), exiftool_get_tag_string(), exiftool_get_tag_count(), iterator for tags), (4) Metadata writing (exiftool_set_tag(), exiftool_write_file()), (5) Memory management (caller vs. library ownership). Document with C code examples. Ensure API is minimal, safe (no panics across FFI boundary), and idiomatic for C consumers.",
  "agent_type_hint": "DocumentationAgent",
  "inputs": "Section 2 (API Contract Style - C FFI), FFI best practices",
  "target_files": [
    "docs/api/ffi_api.md"
  ],
  "input_files": [
    "src/core/operations.rs"
  ],
  "deliverables": "Comprehensive C FFI API documentation, C code examples",
  "acceptance_criteria": "API follows C conventions (handles, return codes, null-terminated strings), error handling is explicit (no panics, all errors returned as codes), memory management is clear (who owns what), at least 5 C code examples showing usage, well-formatted Markdown",
  "dependencies": [
    "I2.T3",
    "I3.T4"
  ],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: architectural-style (from 02_Architecture_Overview.md)

```markdown
### 3.1. Architectural Style

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

### Context: security-considerations (from 05_Operational_Architecture.md)

```markdown
#### Security Considerations

**Threat Model**:

ExifTool-RS processes potentially malicious files from untrusted sources (e.g., user uploads, scraped images). Primary threats:

1. **Memory Corruption**: Buffer overflows, use-after-free in parsers
2. **Resource Exhaustion**: Zip bombs, billion laughs (XML), decompression bombs
3. **Path Traversal**: Malicious filenames in archive processing
4. **Code Injection**: Via scripting features (if added)

**Mitigations**:

| **Threat** | **Mitigation** | **Implementation** |
|------------|---------------|-------------------|
| Buffer overflows | Rust ownership system | Compile-time prevention via borrow checker |
| Integer overflows | Checked arithmetic | `#![deny(overflowing_literals)]`, `checked_add()` in parsers |
| Resource exhaustion | Size limits | Max allocation: 1GB per file, max parse depth: 64 levels (nested IFDs) |
| Zip bombs | Decompression ratio check | Reject if uncompressed > 100x compressed size |
| XXE attacks (XML) | Disable external entities | `quick-xml` configured to reject DOCTYPE, external entities |
| Path traversal | Path sanitization | `canonicalize()` + jail to working directory for batch operations |
| Dependency vulnerabilities | Automated scanning | `cargo-audit` in CI, Dependabot alerts, minimal dependency tree |
| Malicious input | Fuzzing | Continuous fuzzing with `cargo-fuzz`, OSS-Fuzz integration target |

**Input Validation**:

All parsers follow defensive pattern:
```rust
fn read_u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data.get(offset..offset+4)
        .ok_or(ParseError::UnexpectedEof)?;  // Bounds check
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}
```

**Secure Defaults**:

- No script execution (unlike Perl ExifTool's `-execute` feature)
- No network access by default (geolocation requires opt-in `--geolocation` flag)
- Read-only mode available via `--readonly` flag (prevents accidental writes)
```

### Context: directory-structure (from 01_Plan_Overview_and_Setup.md)

```markdown
├── src/ffi/                             # C FFI bindings
│       ├── mod.rs
│       └── c_api.rs                     # C-compatible function exports
│
├── docs/                                # Documentation and design artifacts
│   ├── diagrams/                        # UML diagrams (PlantUML, Mermaid)
│   │   ├── component_architecture.puml
│   │   ├── metadata_erd.mmd
│   │   ├── sequence_metadata_extraction.puml
│   │   └── sequence_metadata_write.puml
│   │
│   ├── api/                             # API specifications
│   │   ├── library_api.md               # Rust library API docs
│   │   └── ffi_api.md                   # C FFI API docs
│
├── api/                                 # API specification files
│   ├── tag_database_schema.json         # JSON Schema for tag definitions
│   └── exiftool_rs.h                    # C FFI header (cbindgen-generated)
```

### Context: library-api-reference (from docs/api/library_api.md)

```markdown
# ExifTool-RS Library API Reference

**Version:** 0.1.0
**Last Updated:** 2025-10-29

## Core Concepts

### Tag Naming Convention

All metadata tags in ExifTool-RS follow a standardized naming convention:

```
<FormatFamily>:<TagName>
```

**Examples:**

- `EXIF:Make` - Camera manufacturer (EXIF format)
- `EXIF:Model` - Camera model
- `EXIF:DateTime` - Image capture date/time
- `XMP-dc:Creator` - Document creator (XMP Dublin Core namespace)
- `GPS:Latitude` - GPS latitude coordinate
- `IPTC:Keywords` - Image keywords
- `PNG:Description` - PNG text chunk description

### Synchronous API Design

ExifTool-RS uses a **synchronous, blocking API** design:

- All operations complete before returning
- No async/await or futures
- File I/O is the bottleneck, not computation
- Parallel processing is achieved via `rayon` at the application level

### Type Safety

Metadata values are represented by the `TagValue` enum, which provides type safety at runtime:

```rust
pub enum TagValue {
    String(String),
    Integer(i64),
    Float(f64),
    Rational { numerator: i32, denominator: i32 },
    Binary(Vec<u8>),
    DateTime(chrono::DateTime<Utc>),
    Struct(Box<HashMap<String, TagValue>>),
}
```

## Error Handling

### ExifToolError

The library uses a comprehensive error type:

```rust
pub enum ExifToolError {
    IoError(io::Error),
    ParseError { message: String, offset: Option<usize> },
    TagNotFound { tag_name: String },
    InvalidTagValue { tag_name: String, reason: String },
    UnsupportedFormat { message: String },
}
```
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/operations.rs`
    *   **Summary:** This file contains the core metadata read and write operations. The `read_metadata(path: &Path) -> Result<MetadataMap>` function orchestrates file opening, format detection, and parser selection. The `write_metadata(path: &Path, metadata: &MetadataMap) -> Result<()>` function handles validation and atomic file writing.
    *   **Recommendation:** Your FFI API MUST expose these core operations. The C API should wrap `read_metadata()` for file reading and `write_metadata()` for file writing. Study the error handling patterns used here - all functions return `Result<T, ExifToolError>`.

*   **File:** `src/core/metadata_map.rs`
    *   **Summary:** Defines `MetadataMap`, the primary in-memory structure for metadata storage. It's a wrapper around `HashMap<String, TagValue>` with typed getter methods like `get_string()`, `get_integer()`, `get_float()`, and `get_datetime()`.
    *   **Recommendation:** Your FFI API MUST provide safe access to this structure. Consider a handle-based approach where `MetadataMap` is an opaque pointer. You will need to expose methods to iterate over tags, get tag count, and retrieve individual tag values by name.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** Defines the `TagValue` enum with variants: String, Integer, Float, Rational, Binary, DateTime, Struct. Each variant has typed accessor methods (`as_string()`, `as_integer()`, etc.) that return `Option<T>`.
    *   **Recommendation:** The C FFI MUST handle the polymorphic nature of TagValue safely. Consider returning an enum discriminant or type code along with the value to enable safe downcasting in C.

*   **File:** `src/error/mod.rs`
    *   **Summary:** Defines `ExifToolError` enum with variants: IoError, ParseError, TagNotFound, InvalidTagValue, UnsupportedFormat. Each has helper constructors and implements Display for error messages.
    *   **Recommendation:** Your FFI error handling MUST convert Rust `Result<T>` into C-style return codes. Define numeric error codes (e.g., `EXIFTOOL_OK = 0`, `EXIFTOOL_ERR_IO = 1`, etc.). Store the last error message in thread-local storage so C callers can retrieve it with `exiftool_get_last_error()`.

*   **File:** `src/lib.rs`
    *   **Summary:** The root library module that declares all public modules. The crate is organized into Application Layer (cli, ffi), Domain Layer (core), and Infrastructure Layer (parsers, writers, io).
    *   **Recommendation:** Your FFI module already exists at `src/ffi/`. The current `c_api.rs` file is just a placeholder with a comment. This is where you will implement the actual FFI functions in the next task (I5.T2).

*   **File:** `docs/api/library_api.md`
    *   **Summary:** Comprehensive Rust library API documentation covering tag naming conventions, synchronous design, type safety, high-level and low-level APIs, error handling, and code examples.
    *   **Recommendation:** Use this as a reference model for your C FFI documentation. The FFI API should maintain consistency with the Rust API design principles (explicit error handling, type safety where possible, clear ownership semantics).

### Implementation Tips & Notes

*   **Tip:** The FFI API MUST use **handle-based lifecycle management**. C callers will receive opaque pointers (handles) to Rust objects. Example pattern:
    ```c
    ExifToolHandle* handle = exiftool_create();
    int result = exiftool_read_file(handle, "photo.jpg");
    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
    exiftool_destroy(handle);
    ```

*   **Tip:** For error handling, follow the **return code + last error** pattern used by many C libraries:
    - Functions return `int` status codes (0 = success, non-zero = error)
    - Store error details in thread-local storage
    - Provide `const char* exiftool_get_last_error()` to retrieve human-readable error messages
    - This prevents panics from crossing FFI boundary

*   **Tip:** Memory management MUST be crystal clear. Document these rules:
    1. **Handles**: Library owns handles. Caller MUST call `exiftool_destroy()` to free.
    2. **Strings returned by library**: Library owns. Valid until next API call or handle destruction. Caller should copy if needed.
    3. **Strings passed by caller**: Caller owns. Library copies immediately if needed.
    4. **Binary data**: Use explicit length parameters, never rely on null-termination for binary data.

*   **Note:** The existing Rust API uses `std::path::Path` which accepts many types via `AsRef<Path>`. For C FFI, you MUST use `const char*` null-terminated strings for file paths. Convert using `std::ffi::CStr` and handle UTF-8 validation carefully (paths may not be valid UTF-8 on all platforms).

*   **Note:** Iterator pattern for tag enumeration is critical. C callers will need to iterate over all tags in a MetadataMap. Consider two approaches:
    1. **Callback-based**: `void exiftool_iterate_tags(handle, callback_fn, user_data)`
    2. **Index-based**: `const char* exiftool_get_tag_name_at(handle, size_t index)` with `size_t exiftool_get_tag_count(handle)`

    The index-based approach is simpler and more familiar to C developers.

*   **Warning:** The C FFI MUST catch ALL Rust panics at the boundary using `std::panic::catch_unwind()`. A Rust panic unwinding into C code is undefined behavior and will corrupt the stack. Wrap all FFI entry points with panic guards.

*   **Warning:** Thread safety: The Rust `MetadataMap` is NOT `Sync` (cannot be shared between threads). Document that handles are NOT thread-safe and callers must synchronize access themselves or use one handle per thread.

*   **Best Practice:** Provide at least 5 C code examples in the documentation:
    1. Basic usage: create handle, read file, get tag, destroy
    2. Error handling: checking return codes, retrieving error messages
    3. Iterating all tags in a file
    4. Modifying metadata: read, modify, write back
    5. Memory safety: demonstrating proper handle lifecycle

*   **Best Practice:** The API should be **minimal but complete**. Don't expose every internal detail. Focus on the most common use cases:
    - Create/destroy handle
    - Read metadata from file
    - Write metadata to file
    - Get tag value (string, integer, float variants)
    - Set tag value
    - Get tag count and iterate tags
    - Error retrieval

*   **Reference:** Look at successful C FFI examples from other Rust projects:
    - `libgit2` (C library, but good reference for API design)
    - `sqlite3` (excellent error handling pattern)
    - `ImageMagick` (opaque handle pattern)
    - Rust crates like `rust-openssl`, `nix` (Rust->C FFI examples)

### Code Structure Recommendations

Your `docs/api/ffi_api.md` document should follow this structure:

1. **Introduction**: Purpose, C FFI overview, safety guarantees
2. **Quick Start**: Minimal working example (5-10 lines)
3. **Core Concepts**: Handles, error handling, memory ownership, thread safety
4. **API Reference**: Organized by category
   - Handle lifecycle functions
   - Metadata reading functions
   - Metadata writing functions
   - Tag access functions
   - Error handling functions
5. **Type Definitions**: C structs, enums, error codes
6. **Code Examples**: At least 5 complete, runnable examples
7. **Best Practices**: Common pitfalls, safety guidelines
8. **Platform Notes**: Windows/Linux/macOS-specific considerations

Each function should document:
- Function signature
- Purpose (one-line summary)
- Parameters (type, ownership, constraints)
- Return value (success/error codes)
- Errors (what can go wrong)
- Example usage
- Safety notes (can this panic? memory ownership?)
