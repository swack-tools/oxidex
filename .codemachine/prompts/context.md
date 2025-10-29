# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I3.T9",
  "iteration_id": "I3",
  "iteration_goal": "Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.",
  "description": "Add CLI flags in src/cli/args.rs for file preservation: --preserve-file-times (restore original modification time after write), --backup (create .bak backup before modifying), --readonly (prevent writes, read-only mode). Implement preservation logic: save original mtime before write, restore after. Implement backup: copy file to .bak before modification. Update main.rs to honor flags.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I2.T8 CLI args, I3.T4 write operations",
  "target_files": [
    "src/cli/args.rs",
    "src/writers/atomic_writer.rs",
    "src/main.rs"
  ],
  "input_files": [
    "src/cli/args.rs",
    "src/core/operations.rs"
  ],
  "deliverables": "File preservation flags, mtime preservation, Backup creation, Read-only mode",
  "acceptance_criteria": "--preserve-file-times restores original mtime after write, --backup creates .bak file before modification, --readonly prevents any writes (returns error), Flags work in combination, Manual test: verify mtime preserved, backup created",
  "dependencies": ["I3.T4"],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: task-i3-t9 (from 02_Iteration_I3.md)

```markdown
<!-- anchor: task-i3-t9 -->
*   **Task 3.9: Add File Preservation Options (CLI Flags)**
    *   **Task ID:** `I3.T9`
    *   **Description:** Add CLI flags in `src/cli/args.rs` for file preservation: `--preserve-file-times` (restore original modification time after write), `--backup` (create .bak backup before modifying), `--readonly` (prevent writes, read-only mode). Implement preservation logic: save original mtime before write, restore after. Implement backup: copy file to .bak before modification. Update main.rs to honor flags.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** I2.T8 CLI args, I3.T4 write operations
    *   **Input Files:** [`src/cli/args.rs`, `src/core/operations.rs`]
    *   **Target Files:**
        *   `src/cli/args.rs` (add flags)
        *   `src/writers/atomic_writer.rs` (add preserve_mtime parameter)
        *   `src/main.rs` (implement backup and readonly checks)
    *   **Deliverables:**
        *   File preservation flags
        *   mtime preservation
        *   Backup creation
        *   Read-only mode
    *   **Acceptance Criteria:**
        *   `--preserve-file-times` restores original mtime after write
        *   `--backup` creates .bak file before modification
        *   `--readonly` prevents any writes (returns error)
        *   Flags work in combination
        *   Manual test: verify mtime preserved, backup created
    *   **Dependencies:** `I3.T4`
    *   **Parallelizable:** Yes (can be developed in parallel with other I3 tasks)
```

### Context: API Design Example (from 04_Behavior_and_Communication.md)

```markdown
// Builder pattern for complex operations
let result = Metadata::from_path("input.jpg")?
    .copy_tags_to("output.jpg")?
    .with_tags(&["EXIF:DateTime", "EXIF:Make", "EXIF:Model"])
    .preserve_file_times(true)
    .execute()?;
```

This shows the architecture envisioned a `preserve_file_times()` method for the builder pattern API. For CLI implementation, this translates to a `--preserve-file-times` flag.

### Context: Security Features (from 05_Operational_Architecture.md)

```markdown
**Secure Defaults**:

- No script execution (unlike Perl ExifTool's `-execute` feature)
- No network access by default (geolocation requires opt-in `--geolocation` flag)
- Read-only mode available via `--readonly` flag (prevents accidental writes)
```

This confirms that `--readonly` is an architectural feature for preventing accidental writes, which aligns with security best practices.

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/cli/args.rs`
    *   **Summary:** Defines the CLI argument structure using `clap` with derive macros. Currently supports `-json`, `-s` (short format), `-a` (all tags), `-r` (recursive), and variable arguments for tag modifications (`-TAG=VALUE`) and file path. The struct uses `#[derive(Parser, Debug)]` and has helper methods `file()` and `tag_modifications()`.
    *   **Recommendation:** You MUST add three new boolean flags to the `CliArgs` struct: `preserve_file_times`, `backup`, and `readonly`. Use clap's `#[arg(long)]` attribute for each. The flags should be optional (default to false) and clearly documented with doc comments.
    *   **Current Implementation Pattern:**
        ```rust
        #[derive(Parser, Debug)]
        #[command(name = "exiftool-rs")]
        pub struct CliArgs {
            #[arg(short, long)]
            pub json: bool,
            // ... add new flags here following this pattern
        }
        ```

*   **File:** `src/main.rs`
    *   **Summary:** Entry point for the CLI application. Parses arguments with `CliArgs::parse()`, distinguishes between read and write operations based on presence of tag modifications, and calls either `handle_read_operation()` or `handle_write_operation()`. The write handler already verifies file exists and checks if it's writable (lines 49-66).
    *   **Recommendation:** You MUST modify `handle_write_operation()` to:
        1. Check `args.readonly` flag FIRST and return error with appropriate message if set
        2. Save original file metadata (modification time) BEFORE any operations if `args.preserve_file_times` is true
        3. Create a backup copy (`.bak` extension) BEFORE calling `modify_tag()` if `args.backup` is true
        4. Restore modification time AFTER successful write if `args.preserve_file_times` is true
    *   **Implementation Order:** The order is critical: readonly check → backup → modify → restore mtime
    *   **Current Write Flow:** Currently at line 38-43, the code checks for modifications and routes to `handle_write_operation()`. You need to pass the full `args` struct (not just modifications) to access the new flags.

*   **File:** `src/writers/atomic_writer.rs`
    *   **Summary:** Implements atomic file writing using `tempfile` crate with the temp-file-and-rename pattern. The `write_atomic()` function creates temp file in same directory, writes data, calls `fsync()`, and atomically renames. Well-tested with 11 comprehensive unit tests.
    *   **Recommendation:** You HAVE TWO OPTIONS for mtime preservation:
        - **Option A (Simpler):** Handle mtime preservation in `main.rs` by saving/restoring around the `modify_tag()` call. This keeps `write_atomic()` focused on atomicity.
        - **Option B (More Modular):** Add an optional `preserve_mtime: Option<SystemTime>` parameter to `write_atomic()`, and if provided, restore it after the rename operation using `std::fs::File::set_modified()`.
    *   **Strategic Note:** Option A is recommended for this task because it keeps the concerns separated - `atomic_writer` handles atomicity, `main.rs` handles CLI preservation logic. This follows the single responsibility principle.

*   **File:** `src/core/operations.rs`
    *   **Summary:** Defines core metadata operations. Contains `read_metadata()` (line 64), `write_metadata()` (line 443), and `modify_tag()` (line 535). The `modify_tag()` function is the convenience wrapper used by CLI - it reads existing metadata, modifies one tag, and writes all metadata back.
    *   **Recommendation:** You SHOULD NOT modify this file. The operations are already complete and the preservation logic should be handled at the CLI layer (in `main.rs`), not in the core library operations. This maintains proper architectural layering.

### Implementation Tips & Notes

*   **Tip - File Time Handling:** Use `std::fs::metadata()` to get the current file metadata, then call `.modified()` to get the `SystemTime` of the last modification. After writing, use `filetime` crate or `std::fs::File::set_modified()` (requires opening file handle) to restore it. The `filetime` crate is cleaner but requires adding a dependency. For this task, you can use the standard library approach.

*   **Tip - Backup Creation:** The backup should be a simple file copy using `std::fs::copy()`. The backup filename should be the original path with `.bak` appended (e.g., `photo.jpg` → `photo.jpg.bak`). Create the backup BEFORE calling any write operations so that if the write fails, the original is preserved.

*   **Tip - Readonly Flag:** The readonly check should be the VERY FIRST thing in `handle_write_operation()`, even before checking if the file exists. If `args.readonly` is true, immediately return an error like: `"Error: Cannot modify file in read-only mode (--readonly flag set)"`. Use `process::exit(1)` to exit with error code.

*   **Note - Flag Combinations:** The flags should work independently and in combination:
    - `--readonly` alone: prevents all writes
    - `--backup` alone: creates .bak before writing
    - `--preserve-file-times` alone: restores mtime after writing
    - `--backup --preserve-file-times`: creates backup AND preserves mtime
    - `--readonly` with any other flag: readonly takes precedence, other flags have no effect

*   **Note - Error Handling:** If backup creation fails, you MUST return an error and NOT proceed with the write operation. If mtime restoration fails, you SHOULD log a warning but NOT fail the entire operation (the write succeeded, only the mtime restoration failed).

*   **Warning - Passing Args:** Currently, `handle_write_operation()` only receives `&[(String, String)]` modifications. You will need to change its signature to accept `&CliArgs` (or the individual flags) so it can check `readonly`, `backup`, and `preserve_file_times`. Update the call site in `main()` accordingly.

*   **Testing Strategy:** Write manual tests as specified in acceptance criteria:
    1. Test `--preserve-file-times`: Modify file, check mtime before/after
    2. Test `--backup`: Verify `.bak` file is created with original content
    3. Test `--readonly`: Verify write is prevented with error message
    4. Test combinations: e.g., `--backup --preserve-file-times` together

### Dependencies and Build Notes

*   **Current Dependencies:** The project already has all necessary dependencies in `Cargo.toml`. You do NOT need to add any new dependencies for this task:
    - `clap` for CLI argument parsing (already present)
    - `std::fs` for file operations (standard library)
    - `std::time::SystemTime` for timestamps (standard library)

*   **No Changes Needed:** `Cargo.toml` does not need modification for this task.

### Code Quality Requirements

*   **Documentation:** Add doc comments (`///`) for all new CLI flags explaining their purpose and behavior
*   **Error Messages:** User-facing error messages should be clear and actionable
*   **Testing:** While full integration tests will come later, ensure the code is testable and consider adding unit tests for backup creation logic if you extract it to a helper function
*   **Formatting:** Run `cargo fmt` before committing
*   **Linting:** Run `cargo clippy` and address all warnings

---

## Summary Checklist

Before you begin coding, ensure you understand:

- [x] The three new flags to add: `--preserve-file-times`, `--backup`, `--readonly`
- [x] The execution order: readonly check → backup → modify → restore mtime
- [x] The recommended approach: handle all preservation logic in `main.rs`, not in `atomic_writer.rs` or `operations.rs`
- [x] The need to modify `handle_write_operation()` signature to accept the full args or flags
- [x] The backup naming convention: original filename + `.bak` extension
- [x] The error handling strategy: fail on backup failure, warn on mtime restoration failure
- [x] The testing approach: manual testing as specified in acceptance criteria

You are now ready to implement Task I3.T9. Good luck!

```json
{
  "task_id": "I3.T8",
  "iteration_id": "I3",
  "iteration_goal": "Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.",
  "description": "Implement PNG writer in src/writers/png_writer.rs. Write modified metadata back to PNG file: (1) Parse PNG chunks, (2) Update tEXt/iTXt chunks with modified text tags, (3) Update eXIf chunk with modified EXIF (serialize using TIFF writer I3.T2), (4) Recalculate CRC for modified chunks, (5) Write PNG structure with updated chunks. Preserve image data (IDAT chunks) unchanged. Add integration test.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I2.T7 PNG parser, I3.T2 TIFF writer (for eXIf)",
  "target_files": [
    "src/writers/png_writer.rs",
    "src/writers/mod.rs",
    "tests/integration/png_write_tests.rs"
  ],
  "input_files": [
    "src/parsers/png/chunk_parser.rs",
    "src/writers/tiff_writer.rs"
  ],
  "deliverables": "PNG metadata writer, CRC recalculation, integration test",
  "acceptance_criteria": "Writer updates tEXt/iTXt chunks correctly, updates eXIf chunk with serialized EXIF, recalculates CRC for modified chunks, preserves IDAT (image data) chunks unchanged, integration test: modify PNG text tag, verify change, cargo test png_write_tests passes",
  "dependencies": [
    "I2.T7",
    "I3.T2"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: task-i3-t8 (from 02_Iteration_I3.md)

```markdown
<!-- anchor: task-i3-t8 -->
*   **Task 3.8: Implement PNG Metadata Writer**
    *   **Task ID:** `I3.T8`
    *   **Description:** Implement PNG writer in `src/writers/png_writer.rs`. Write modified metadata back to PNG file: (1) Parse PNG chunks, (2) Update tEXt/iTXt chunks with modified text tags, (3) Update eXIf chunk with modified EXIF (serialize using TIFF writer I3.T2), (4) Recalculate CRC for modified chunks, (5) Write PNG structure with updated chunks. Preserve image data (IDAT chunks) unchanged. Add integration test.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** I2.T7 PNG parser, I3.T2 TIFF writer (for eXIf)
    *   **Input Files:** [`src/parsers/png/chunk_parser.rs`, `src/writers/tiff_writer.rs`]
    *   **Target Files:**
        *   `src/writers/png_writer.rs`
        *   `src/writers/mod.rs`
        *   `tests/integration/png_write_tests.rs`
    *   **Deliverables:**
        *   PNG metadata writer
        *   CRC recalculation
        *   Integration test
    *   **Acceptance Criteria:**
        *   Writer updates tEXt/iTXt chunks correctly
        *   Updates eXIf chunk with serialized EXIF
        *   Recalculates CRC for modified chunks
        *   Preserves IDAT (image data) chunks unchanged
        *   Integration test: modify PNG text tag, verify change
        *   `cargo test png_write_tests` passes
    *   **Dependencies:** `I2.T7`, `I3.T2`
    *   **Parallelizable:** Partially (can start after I2.T7, wait for I3.T2 for eXIf)
```

### Context: task-i2-t7 (from 02_Iteration_I2.md)

```markdown
<!-- anchor: task-i2-t7 -->
*   **Task 2.7: Implement PNG Format Parser**
    *   **Task ID:** `I2.T7`
    *   **Description:** Implement PNG parser in `src/parsers/png/` using nom or binrw. Parse PNG chunk structure: 8-byte signature, then sequence of chunks (length, type, data, CRC). Focus on metadata chunks: tEXt (uncompressed text), iTXt (international text, UTF-8), zTXt (compressed text). Extract text key-value pairs. For EXIF in PNG, check for eXIf chunk (contains raw EXIF data), parse using existing TIFF IFD parser. Return MetadataMap with PNG text tags and EXIF tags if present. Add unit tests and integration test with sample PNG.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** PNG specification, I1.T11 TIFF parser (for eXIf chunk)
    *   **Input Files:** [`src/parsers/tiff/ifd_parser.rs`]
    *   **Target Files:**
        *   `src/parsers/png/chunk_parser.rs`
        *   `src/parsers/png/mod.rs`
        *   `src/parsers/mod.rs` (export png module)
        *   `tests/integration/png_tests.rs`
        *   `tests/fixtures/png/sample_with_text.png`
        *   `tests/fixtures/png/sample_with_exif.png`
    *   **Deliverables:**
        *   PNG parser for tEXt, iTXt, zTXt, eXIf chunks
        *   Integration tests
        *   Sample PNG files
    *   **Acceptance Criteria:**
        *   Parser correctly identifies PNG signature
        *   Parses chunk structure (length, type, data, CRC validation)
        *   Extracts text from tEXt and iTXt chunks
        *   Parses EXIF from eXIf chunk using TIFF parser
        *   Integration tests verify extraction of text and EXIF tags
        *   `cargo test png_tests` passes
    *   **Dependencies:** `I1.T11` (TIFF parser for eXIf)
    *   **Parallelizable:** Yes (can be developed in parallel with I2.T2-T6)
```

### Context: task-i3-t2 (from 02_Iteration_I3.md)

```markdown
<!-- anchor: task-i3-t2 -->
*   **Task 3.2: Implement EXIF IFD Serializer (TIFF Writer)**
    *   **Task ID:** `I3.T2`
    *   **Description:** Implement TIFF IFD serializer in `src/writers/tiff_writer.rs`. Create function to serialize MetadataMap EXIF tags back to TIFF IFD structure: (1) Filter tags for EXIF family, (2) Convert TagValue to TIFF data types (Byte, ASCII, Short, Long, Rational), (3) Build IFD entries (tag ID, type, count, value/offset), (4) Handle values >4 bytes (write to separate value area), (5) Calculate offsets, (6) Write IFD header + entries + values. Support both little-endian and big-endian output. Add unit tests verifying round-trip (parse then serialize equals original).
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** TIFF specification, I1.T11 TIFF parser (for understanding structure)
    *   **Input Files:** [`src/parsers/tiff/ifd_parser.rs`, `src/core/metadata_map.rs`]
    *   **Target Files:**
        *   `src/writers/tiff_writer.rs`
        *   `src/writers/mod.rs`
    *   **Deliverables:**
        *   TIFF IFD serialization function
        *   Support for both endianness
        *   Unit and round-trip tests
    *   **Acceptance Criteria:**
        *   Serializer produces valid TIFF IFD structure
        *   Handles both little-endian and big-endian
        *   Correctly writes tag entries with type, count, value
        *   Values >4 bytes written to separate area with offset
        *   Round-trip test: parse(serialize(metadata)) == metadata for EXIF tags
        *   `cargo test tiff_writer` passes
    *   **Dependencies:** `I1.T11` (TIFF parser structure), `I2.T2` (tag registry)
    *   **Parallelizable:** Yes (can develop in parallel with I3.T1)
```

### Context: technology-stack-summary (from 02_Architecture_Overview.md)

```markdown
<!-- anchor: technology-stack-summary -->
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

**Dependency Philosophy**:
- **Minimize Count**: Target < 50 direct dependencies to reduce supply chain risk
- **Prefer `no_std` Compatible**: Where possible (e.g., `nom`, `binrw`) to enable future embedded/WASM use
- **Audit Regularly**: `cargo-audit` in CI pipeline to catch vulnerabilities in transitive dependencies
```

### Context: task-i3-t1 (from 02_Iteration_I3.md)

```markdown
<!-- anchor: task-i3-t1 -->
*   **Task 3.1: Implement Atomic File Writer**
    *   **Task ID:** `I3.T1`
    *   **Description:** Implement atomic file writing in `src/writers/atomic_writer.rs`. Create `fn write_atomic(path: &Path, data: &[u8]) -> Result<()>` that: (1) Creates temporary file in same directory with unique name (e.g., ".exiftool-rs.tmp.RANDOM"), (2) Writes data to temp file, (3) Calls `fsync()` to ensure data is on disk, (4) Atomically renames temp file to target path (overwrites original), (5) Handles errors at each step (cleanup temp file on failure). Use `tempfile` crate for temp file creation. Add unit tests verifying atomic behavior and error handling.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** Atomic file operation best practices, filesystem semantics
    *   **Input Files:** []
    *   **Target Files:**
        *   `src/writers/atomic_writer.rs`
        *   `src/writers/mod.rs`
        *   `Cargo.toml` (add `tempfile` dependency if not present)
    *   **Deliverables:**
        *   Atomic file write function
        *   Error handling and cleanup
        *   Unit tests
    *   **Acceptance Criteria:**
        *   write_atomic() creates temp file, writes data, renames atomically
        *   Temp file is in same directory as target (required for atomic rename)
        *   fsync() called before rename
        *   On error, temp file is cleaned up
        *   Unit tests verify successful write and error scenarios
        *   `cargo test atomic_writer` passes
    *   **Dependencies:** `I1` (project setup)
    *   **Parallelizable:** Yes (foundational utility, can be developed early in I3)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/png/chunk_parser.rs` (658 lines)
    *   **Summary:** This file contains complete PNG chunk parsing logic, including:
        - PNG signature verification (`PNG_SIGNATURE` constant at line 35)
        - Chunk header parsing (length + type, 8 bytes total) - function at line 107
        - Complete chunk parsing with CRC reading (but NOT validation) - function at line 135
        - tEXt chunk parsing (Latin-1 keyword\0text format) - function at line 205
        - iTXt chunk parsing (UTF-8 with compression flag, language tag, translated keyword) - function at line 256
        - eXIf chunk parsing (TIFF format EXIF data) - function at line 337
        - `PngChunk` struct with `chunk_type: [u8; 4]`, `data: Vec<u8>`, `crc: u32` at line 38
    *   **Recommendation:** You MUST reuse the `PngChunk` struct and understand the chunk format. The parser shows the exact binary layout you need to write. Note that CRC is stored but NOT validated during reading - you will need to CALCULATE CRC during writing.
    *   **Key Constants:** `PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]` at line 35
    *   **Key Methods:** `parse_chunk()` at line 135, `parse_text_chunk()` at line 205, `parse_itxt_chunk()` at line 256, `parse_exif_chunk()` at line 337
    *   **Chunk Format (as shown in parser):**
        ```
        Length: 4 bytes (big-endian u32) - length of data field only
        Type: 4 bytes (ASCII, e.g., "IHDR", "tEXt", "IEND")
        Data: N bytes (depends on chunk type)
        CRC: 4 bytes (u32 big-endian) - CRC-32 of type + data
        ```

*   **File:** `src/parsers/png/mod.rs` (387 lines)
    *   **Summary:** This file contains the high-level PNG metadata extraction logic:
        - `parse_png_metadata()` function that iterates through all chunks (line 91)
        - Shows how to iterate: start after signature (8 bytes), call `parse_chunk()`, advance to `next_offset`, stop at IEND
        - Tag naming convention: `PNG:tEXt:<keyword>` (line 124), `PNG:iTXt:<keyword>` (line 133), `EXIF:0x<hex_id>` (line 147)
        - Preserves image data by simply skipping IDAT chunks (no special handling needed)
    *   **Recommendation:** You SHOULD follow the exact same iteration pattern but in reverse for writing. Your writer needs to:
        1. Read all existing chunks using `parse_chunk()` in a loop
        2. Identify which chunks need updating (tEXt, iTXt, eXIf) by checking `chunk_type`
        3. Replace those chunks with modified versions (new data + recalculated CRC)
        4. Write all chunks (including IDAT unchanged) to output buffer
        5. Use `write_atomic()` to write final buffer to disk

*   **File:** `src/writers/tiff_writer.rs` (200+ lines)
    *   **Summary:** This file implements TIFF IFD serialization and is CRITICAL for your eXIf chunk writing:
        - `serialize_ifd()` function at line 265: converts MetadataMap EXIF tags to binary TIFF IFD structure
        - Handles both little-endian and big-endian byte orders
        - Writes TIFF header (8 bytes): byte order marker + magic 42 + IFD offset (line 174)
        - `IfdEntryData` struct for representing IFD entries (tag_id, field_type, value_count, value_bytes) at line 197
        - Inline values (≤4 bytes) vs offset values (>4 bytes) logic at line 210
        - `write_u16()`, `write_u32()` helper functions for writing multi-byte values at lines 318, 336
    *   **Recommendation:** You MUST use `serialize_ifd()` to create the EXIF data for eXIf chunks. The eXIf chunk format is: raw TIFF structure (starting with "II" or "MM" byte order marker). Call `serialize_ifd()` and wrap the result in an eXIf chunk with proper length and CRC.
    *   **Import Path:** `use crate::writers::tiff_writer::serialize_ifd;`
    *   **Function Signature:** `pub fn serialize_ifd(metadata: &MetadataMap, byte_order: ByteOrder, ifd_start_offset: u64) -> Result<Vec<u8>>`
    *   **Usage for eXIf:** Call with `ByteOrder::LittleEndian` and `ifd_start_offset: 0` to get complete TIFF structure

*   **File:** `src/writers/atomic_writer.rs` (100+ lines)
    *   **Summary:** This file provides atomic file writing using temp-file-and-rename pattern:
        - `write_atomic(path: &Path, data: &[u8]) -> Result<()>` function at line 93
        - Uses `tempfile::NamedTempFile` to create temp file in same directory at line 101
        - Calls `fsync()` before rename to ensure durability at line 111
        - Automatic cleanup on failure via `tempfile` RAII pattern
    *   **Recommendation:** You MUST use `write_atomic()` for the final file write to ensure no corruption on crash. Do NOT implement your own file writing logic. Build the complete PNG file in memory as `Vec<u8>`, then call `write_atomic()` once at the end.
    *   **Import Path:** `use crate::writers::atomic_writer::write_atomic;`

*   **File:** `src/writers/png_writer.rs` (6 lines - EMPTY)
    *   **Summary:** This is the file you need to implement. Currently only has module comments and `#![allow(dead_code)]`.
    *   **Recommendation:** This is your main implementation file. Start fresh but follow the patterns from the parser and TIFF writer.

*   **File:** `src/core/metadata_map.rs` (referenced but not read in detail)
    *   **Summary:** Provides `MetadataMap` type for storing tag-value pairs with `HashMap<String, TagValue>` internally.
    *   **Recommendation:** Use `metadata.iter()` to iterate through all tags. Filter by prefix (e.g., `tag_name.starts_with("PNG:tEXt:")`) to find text chunks. Extract keyword from tag name (e.g., "PNG:tEXt:Author" → "Author").

*   **File:** `src/error/mod.rs` (80+ lines)
    *   **Summary:** Defines `ExifToolError` enum with helper methods:
        - `parse_error()` at line 50, `parse_error_at()` at line 58, `tag_not_found()` at line 66, `invalid_tag_value()` at line 73, `unsupported_format()` at line 80
    *   **Recommendation:** Use these helper methods for error creation. For example: `ExifToolError::unsupported_format("zTXt compressed chunks not supported for writing")`

*   **File:** `src/core/file_reader_trait.rs` (referenced)
    *   **Summary:** Defines `FileReader` trait with `read()` and `size()` methods.
    *   **Recommendation:** Your `write_png_metadata()` function should accept `original_reader: &dyn FileReader` to read existing PNG structure.

### Implementation Tips & Notes

*   **Tip #1: CRC32 Calculation (CRITICAL)** - You MUST add a CRC32 calculation library to `Cargo.toml`. The `crc` crate (version 3.0) is the standard choice. PNG uses CRC-32/ISO-HDLC polynomial (also called CRC-32-IEEE or CRC-32).
    - Add to `Cargo.toml` dependencies section: `crc = "3.0"`
    - Use in your code:
      ```rust
      use crc::{Crc, CRC_32_ISO_HDLC};

      fn calculate_crc(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
          let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
          let mut digest = crc.digest();
          digest.update(chunk_type);
          digest.update(data);
          digest.finalize()
      }
      ```
    - **CRITICAL:** CRC is calculated over chunk type (4 bytes) + chunk data (N bytes), NOT including the length field.

*   **Tip #2: PNG Chunk Writing Format** - All PNG chunks use BIG-ENDIAN byte order (unlike TIFF which can be either):
    ```
    Length: 4 bytes (u32 big-endian) - length of DATA field only (not including type or CRC)
    Type: 4 bytes (ASCII, e.g., b"tEXt", b"iTXt", b"eXIf", b"IDAT")
    Data: N bytes (depends on chunk type, can be empty for IEND)
    CRC: 4 bytes (u32 big-endian) - CRC-32 of Type + Data
    ```

*   **Tip #3: tEXt Chunk Writing** - Format is `keyword\0text` (null-separated, Latin-1 encoding):
    - Extract keyword from tag name: `"PNG:tEXt:Author"` → keyword = `"Author"`
    - Extract text from TagValue (convert to string)
    - Build data: `keyword.as_bytes() + b"\0" + text.as_bytes()`
    - Calculate CRC over `b"tEXt" + data`
    - Write chunk: length (data.len() as u32 BE) + b"tEXt" + data + CRC

*   **Tip #4: iTXt Chunk Writing** - Format is `keyword\0compression_flag\0compression_method\0language\0translated_keyword\0text`:
    - For uncompressed (MVP requirement): compression_flag = 0, compression_method = 0
    - Extract keyword from tag name: `"PNG:iTXt:Title"` → keyword = `"Title"`
    - Use empty strings for language ("") and translated keyword ("") if not present
    - Text is UTF-8 encoded
    - Build data:
      ```rust
      let mut data = Vec::new();
      data.extend_from_slice(keyword.as_bytes());
      data.push(0); // null separator
      data.push(0); // compression flag = 0
      data.push(0); // compression method = 0
      data.extend_from_slice(b""); // language (empty)
      data.push(0); // null separator
      data.extend_from_slice(b""); // translated keyword (empty)
      data.push(0); // null separator
      data.extend_from_slice(text.as_bytes()); // UTF-8 text
      ```

*   **Tip #5: eXIf Chunk Writing** - This is where you use the TIFF writer:
    ```rust
    use crate::parsers::tiff::ifd_parser::ByteOrder;
    use crate::writers::tiff_writer::serialize_ifd;

    // Filter EXIF tags from metadata
    let exif_metadata = /* ... filter for EXIF: prefix ... */;

    // Serialize to TIFF format (complete with header)
    let tiff_data = serialize_ifd(&exif_metadata, ByteOrder::LittleEndian, 0)?;

    // tiff_data now contains: "II" + 0x002A + offset + IFD + values
    // This becomes the eXIf chunk data directly
    let crc = calculate_crc(b"eXIf", &tiff_data);
    // Write chunk: length + b"eXIf" + tiff_data + CRC
    ```

*   **Tip #6: Chunk Order** - PNG specification requires specific chunk ordering:
    - **IHDR**: MUST be first chunk (critical)
    - **Metadata chunks** (tEXt, iTXt, eXIf): Can appear anywhere, but SHOULD appear before IDAT for best compatibility
    - **IDAT**: Image data chunks, can be multiple sequential chunks
    - **Other chunks**: Chunks like PLTE, tRNS, etc.
    - **IEND**: MUST be last chunk (critical)
    - A safe strategy: IHDR → metadata → IDAT → other → IEND

*   **Tip #7: Writing Strategy** - Follow this high-level algorithm:
    1. Parse all existing chunks from original file into `Vec<PngChunk>`
    2. Categorize chunks: IHDR (first), metadata (tEXt/iTXt/eXIf), IDAT, other, IEND (last)
    3. Build new metadata chunks from `modified_metadata` (filter by tag name prefix)
    4. Replace old metadata chunks with new ones (or insert if new tags added)
    5. Preserve non-metadata chunks (IHDR, IDAT, other, IEND) unchanged
    6. Reassemble PNG: signature + chunks in correct order
    7. Call `write_atomic()` with complete PNG data

*   **Note #1: Handling Missing Metadata** - If a metadata tag exists in the original PNG but is NOT in `modified_metadata`, you should REMOVE that chunk (don't preserve it). This allows users to delete metadata tags. However, non-metadata chunks (IHDR, IDAT, etc.) must always be preserved.

*   **Note #2: Multiple IDAT Chunks** - PNG images often have multiple consecutive IDAT chunks. You MUST preserve ALL of them in their original order. Do NOT merge or split IDAT chunks.

*   **Note #3: Critical vs. Ancillary Chunks** - PNG chunks are categorized by the first letter of their type:
    - Uppercase first letter = critical chunk (e.g., IHDR, IDAT, IEND)
    - Lowercase first letter = ancillary chunk (e.g., tEXt, iTXt, eXIf)
    - Your implementation must preserve all critical chunks and only modify ancillary metadata chunks.

*   **Warning #1: zTXt Compression** - DO NOT implement zTXt (compressed text) writing in this iteration. The task description mentions zTXt parsing exists, but writing compressed chunks is complex and not required for MVP. If you encounter zTXt chunks in the original file, you SHOULD preserve them unchanged (copy bytes as-is) OR remove them if they appear in `modified_metadata`.

*   **Warning #2: CRC Validation** - The parser reads CRC but does NOT validate it (line 175 in chunk_parser.rs just reads the bytes). Your writer MUST calculate correct CRC values. Incorrect CRC will cause the PNG to be rejected by other tools even if the structure is correct. Use the `crc` crate as described in Tip #1.

*   **Warning #3: Big-Endian Byte Order** - Unlike TIFF (which can be either endian), PNG always uses BIG-ENDIAN byte order for multi-byte integers (length and CRC fields). Use `u32::to_be_bytes()` for conversions.

### Suggested Implementation Approach

1. **Add CRC dependency to Cargo.toml:**
   ```toml
   [dependencies]
   crc = "3.0"
   ```

2. **Create the main write function signature:**
   ```rust
   pub fn write_png_metadata(
       path: &Path,
       original_reader: &dyn FileReader,
       modified_metadata: &MetadataMap,
   ) -> Result<()>
   ```

3. **Implement helper functions:**
   ```rust
   fn calculate_crc(chunk_type: &[u8; 4], data: &[u8]) -> u32;
   fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]);
   fn serialize_text_chunk(keyword: &str, text: &str) -> Vec<u8>;
   fn serialize_itxt_chunk(keyword: &str, text: &str) -> Vec<u8>;
   fn serialize_exif_chunk(metadata: &MetadataMap) -> Result<Vec<u8>>;
   ```

4. **Main algorithm in write_png_metadata:**
   ```rust
   // 1. Parse existing PNG structure
   let mut chunks = Vec::new();
   let mut offset = 8; // After signature
   while offset < original_reader.size() {
       let (next_offset, chunk) = parse_chunk(original_reader, offset)?;
       chunks.push(chunk);
       if &chunk.chunk_type == b"IEND" { break; }
       offset = next_offset;
   }

   // 2. Build new metadata chunks from modified_metadata
   let new_metadata_chunks = /* ... build tEXt, iTXt, eXIf chunks ... */;

   // 3. Categorize and filter chunks
   let mut ihdr = /* find IHDR */;
   let mut idats = /* collect all IDATs in order */;
   let mut iend = /* find IEND */;
   let other_chunks = /* non-metadata, non-critical chunks */;

   // 4. Reassemble PNG
   let mut output = Vec::new();
   output.extend_from_slice(&PNG_SIGNATURE);
   write_chunk(&mut output, &ihdr.chunk_type, &ihdr.data);
   for chunk in new_metadata_chunks {
       write_chunk(&mut output, &chunk.chunk_type, &chunk.data);
   }
   for idat in idats {
       write_chunk(&mut output, &idat.chunk_type, &idat.data);
   }
   for chunk in other_chunks {
       write_chunk(&mut output, &chunk.chunk_type, &chunk.data);
   }
   write_chunk(&mut output, &iend.chunk_type, &iend.data);

   // 5. Write atomically
   write_atomic(path, &output)?;
   Ok(())
   ```

5. **Write integration test in tests/integration/png_write_tests.rs:**
   ```rust
   #[test]
   fn test_modify_text_chunk() {
       // Create test PNG with tEXt chunk
       let mut metadata = MetadataMap::new();
       metadata.insert("PNG:tEXt:Author", TagValue::new_string("Original Author"));

       // Write initial PNG
       let temp_path = /* create temp file */;
       write_png_metadata(&temp_path, &reader, &metadata).unwrap();

       // Modify metadata
       metadata.insert("PNG:tEXt:Author", TagValue::new_string("Modified Author"));
       write_png_metadata(&temp_path, &reader, &metadata).unwrap();

       // Re-read and verify
       let reader2 = BufferedReader::new(&temp_path).unwrap();
       let parsed = parse_png_metadata(&reader2).unwrap();
       assert_eq!(parsed.get_string("PNG:tEXt:Author"), Some("Modified Author"));
   }
   ```

### Function Signatures to Implement

Based on the analysis, here are the key functions you should implement:

```rust
// Main public API
pub fn write_png_metadata(
    path: &Path,
    original_reader: &dyn FileReader,
    modified_metadata: &MetadataMap,
) -> Result<()>;

// Helper functions
fn calculate_crc(chunk_type: &[u8; 4], data: &[u8]) -> u32;
fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]);
fn serialize_text_chunk(keyword: &str, text: &str) -> Vec<u8>;
fn serialize_itxt_chunk(keyword: &str, text: &str) -> Vec<u8>;
fn serialize_exif_chunk(metadata: &MetadataMap) -> Result<Vec<u8>>;
```

### Integration Test Requirements

Your `tests/integration/png_write_tests.rs` must include at minimum:

1. **Test: Modify tEXt chunk** - Change text metadata, verify change persists
2. **Test: Modify iTXt chunk** - Change UTF-8 text metadata, verify change
3. **Test: Modify eXIf chunk** - Change EXIF tag, verify TIFF serialization works
4. **Test: Preserve IDAT** - Verify image data unchanged (compare bytes or re-parse)
5. **Test: Round-trip** - Read → modify → write → read → verify

All tests must use `parse_png_metadata()` to verify the written file is correct.

### Summary Checklist

Before you consider the task complete, verify:

- [ ] Added `crc = "3.0"` to Cargo.toml
- [ ] Implemented `write_png_metadata()` function
- [ ] Implemented CRC-32 calculation using `crc` crate
- [ ] Implemented tEXt chunk serialization
- [ ] Implemented iTXt chunk serialization
- [ ] Implemented eXIf chunk serialization (using TIFF writer)
- [ ] Preserved IDAT chunks unchanged
- [ ] Preserved non-metadata chunks unchanged
- [ ] Maintained correct chunk order (IHDR → metadata → IDAT → other → IEND)
- [ ] Used `write_atomic()` for file writing
- [ ] Created integration tests in `tests/integration/png_write_tests.rs`
- [ ] All tests pass: `cargo test png_write_tests`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] Code formatted: `cargo fmt --check`

This comprehensive briefing should give you everything needed to implement the PNG metadata writer successfully!
