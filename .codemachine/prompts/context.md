# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I3.T6",
  "iteration_id": "I3",
  "iteration_goal": "Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.",
  "description": "Extend TIFF parser from I1.T11 to handle standalone TIFF files (not just EXIF segments). Parse TIFF file structure: 8-byte header (byte order, magic number 42, first IFD offset), then IFD chain (IFD0, IFD1 for thumbnails, sub-IFDs for EXIF/GPS). Support multi-page TIFF (follow next IFD offset). Extract all tags from all IFDs. Handle both stripped and tiled image data (ignore pixel data, metadata only). Add integration test with sample TIFF file.",
  "agent_type_hint": "BackendAgent",
  "inputs": "TIFF specification, I1.T11 IFD parser",
  "target_files": [
    "src/parsers/tiff/mod.rs",
    "src/parsers/tiff/file_parser.rs",
    "tests/integration/tiff_tests.rs",
    "tests/fixtures/tiff/sample.tif"
  ],
  "input_files": [
    "src/parsers/tiff/ifd_parser.rs"
  ],
  "deliverables": "Full TIFF file parser, support for multi-page TIFF, integration test",
  "acceptance_criteria": "Parser reads TIFF header and identifies byte order, parses IFD chain (IFD0 → IFD1 → ... via next IFD offset), extracts tags from all IFDs (main image + thumbnail + sub-IFDs), ignores image pixel data (metadata only), integration test extracts metadata from multi-page TIFF, cargo test tiff_tests passes",
  "dependencies": ["I1.T11"],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: data-model-overview (from 03_System_Structure_and_Data.md)

```markdown
### 3.6. Data Model Overview & ERD

**Description**: ExifTool-RS operates on files without persistent database storage. The "data model" represents in-memory structures for metadata representation. The Entity-Relationship Diagram below models the logical relationships between metadata concepts.

#### Key Entities

1. **File**: Represents a media file being processed (JPEG, PNG, etc.)
2. **MetadataMap**: Collection of all metadata tags extracted from a file
3. **TagValue**: A single metadata tag with its name, value, and type information
4. **TagDescriptor**: Definition of a tag (from tag database) including ID, name, type constraints, format family
5. **FormatFamily**: Grouping of related metadata standards (EXIF, XMP, IPTC, MakerNotes)
6. **IFD (Image File Directory)**: TIFF-specific structural element containing tags

**Rationale**:

- **No Persistent Database**: The system is stateless. `MetadataMap` exists only in-memory during processing and is serialized to JSON/text output or written back to file metadata.

- **Variant Value Type**: `TagValue.value` uses a Rust `enum` to represent heterogeneous tag types:
  ```rust
  enum TagValueData {
      String(String),
      Number(f64),
      Integer(i64),
      Binary(Vec<u8>),
      Rational { numerator: i32, denominator: i32 },
      Struct(HashMap<String, TagValueData>), // For complex XMP structures
  }
  ```

- **IFD Hierarchy**: TIFF/EXIF formats use nested IFD structures. The self-referential `parent_ifd_id` models this (e.g., GPS sub-IFD under IFD0).

- **Tag Descriptor**: Compile-time generated from ExifTool tag database. In practice, this is a large static `HashMap<&'static str, TagDescriptor>` embedded in the binary, not a runtime database.
```

### Context: task-i3-t6 (from 02_Iteration_I3.md)

```markdown
*   **Task 3.6: Implement Full TIFF File Parser**
    *   **Task ID:** `I3.T6`
    *   **Description:** Extend TIFF parser from I1.T11 to handle standalone TIFF files (not just EXIF segments). Parse TIFF file structure: 8-byte header (byte order, magic number 42, first IFD offset), then IFD chain (IFD0, IFD1 for thumbnails, sub-IFDs for EXIF/GPS). Support multi-page TIFF (follow next IFD offset). Extract all tags from all IFDs. Handle both stripped and tiled image data (ignore pixel data, metadata only). Add integration test with sample TIFF file.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** TIFF specification, I1.T11 IFD parser
    *   **Input Files:** [`src/parsers/tiff/ifd_parser.rs`]
    *   **Target Files:**
        *   `src/parsers/tiff/mod.rs` (extend with full TIFF parsing)
        *   `src/parsers/tiff/file_parser.rs` (new: handle TIFF file structure)
        *   `tests/integration/tiff_tests.rs`
        *   `tests/fixtures/tiff/sample.tif`
    *   **Deliverables:**
        *   Full TIFF file parser
        *   Support for multi-page TIFF
        *   Integration test
    *   **Acceptance Criteria:**
        *   Parser reads TIFF header and identifies byte order
        *   Parses IFD chain (IFD0 → IFD1 → ... via next IFD offset)
        *   Extracts tags from all IFDs (main image + thumbnail + sub-IFDs)
        *   Ignores image pixel data (metadata only)
        *   Integration test extracts metadata from multi-page TIFF
        *   `cargo test tiff_tests` passes
    *   **Dependencies:** `I1.T11`
    *   **Parallelizable:** Yes (can be developed in parallel with I3.T1-T4)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/tiff/ifd_parser.rs`
    *   **Summary:** This is the foundation for your work. It contains a fully-functional IFD (Image File Directory) parser that handles individual IFD structures. The key function is `parse_ifd(reader, ifd_offset, byte_order)` which returns `Vec<(u16, Vec<u8>)>` - a vector of (tag_id, raw_value_bytes) pairs.
    *   **Key Types:**
        - `ByteOrder` enum: Handles both `LittleEndian` (0x4949 "II") and `BigEndian` (0x4D4D "MM")
        - `IfdEntry` struct: Represents a single 12-byte IFD entry with tag_id, field_type, value_count, value_offset
    *   **What it DOES:**
        - Reads entry count (2 bytes)
        - Parses N × 12-byte entries using nom combinators
        - Handles inline values (≤4 bytes) vs. offset values (>4 bytes)
        - Validates offsets and handles errors gracefully
        - Has comprehensive unit tests
    *   **What it DOESN'T do (your task):**
        - Parse TIFF file header (8 bytes: byte order marker + magic number 42 + first IFD offset)
        - Follow IFD chain (read "next IFD offset" at end of each IFD)
        - Handle sub-IFDs (EXIF IFD, GPS IFD linked via special tags)
        - Coordinate reading from standalone .tif/.tiff files
    *   **Recommendation:** You MUST import and heavily use the existing `parse_ifd()` function. Your `file_parser.rs` should be a higher-level orchestrator that reads the TIFF header, determines byte order, and calls `parse_ifd()` repeatedly to walk the IFD chain.

*   **File:** `src/parsers/common/exif_types.rs`
    *   **Summary:** Defines the `ExifType` enum representing all 12 TIFF/EXIF data types (Byte, ASCII, Short, Long, Rational, etc.) with methods for type size calculation and u16 conversion.
    *   **Recommendation:** You will likely NOT need to directly use this in your file_parser.rs since `parse_ifd()` already handles type interpretation. However, be aware it exists for any future type-specific value parsing.

*   **File:** `src/core/file_reader_trait.rs`
    *   **Summary:** Defines the `FileReader` trait with `read(offset, length)` and `size()` methods. This is your I/O interface.
    *   **Recommendation:** Your parser MUST accept `&dyn FileReader` as input (exactly like `parse_ifd` does). This maintains architecture compliance and enables testing with in-memory test readers.

*   **File:** `src/parsers/format_detector.rs`
    *   **Summary:** Already detects TIFF files via magic bytes (0x4949 for LE, 0x4D4D for BE). Returns `FileFormat::TIFF`.
    *   **Recommendation:** No changes needed here. Your parser will be called AFTER format detection identifies a TIFF file.

*   **File:** `src/parsers/tiff/mod.rs`
    *   **Summary:** Currently just declares submodules (ifd_parser, makernote_parser, tag_parser). Very minimal.
    *   **Recommendation:** You MUST add `pub mod file_parser;` here and expose your new parser functions via `pub use file_parser::*;` or similar.

### Implementation Tips & Notes

*   **Tip - TIFF Header Structure:** According to TIFF 6.0 spec, every TIFF file starts with an 8-byte header:
    1. Bytes 0-1: Byte order marker (0x4949 = "II" little-endian, or 0x4D4D = "MM" big-endian)
    2. Bytes 2-3: Magic number 42 (0x002A in little-endian, 0x2A00 in big-endian)
    3. Bytes 4-7: Offset to first IFD (u32, respecting byte order)

    You MUST parse this header first to determine:
    - Byte order for all subsequent reads
    - Where the first IFD is located in the file

*   **Tip - IFD Chain Following:** Each IFD ends with a 4-byte "next IFD offset" field AFTER the entry array. The `parse_ifd()` function currently reads entries but doesn't return this offset. You have two options:
    1. Read the next IFD offset separately (easier): After calling `parse_ifd(reader, offset, byte_order)`, read 4 more bytes at `offset + 2 + (entry_count * 12)` to get the next offset
    2. Modify `parse_ifd()` to also return next_ifd_offset (more invasive, avoid if possible)

    Option 1 is STRONGLY recommended to minimize changes to the existing, tested code.

*   **Tip - Sub-IFDs (EXIF, GPS):** TIFF files often have "sub-IFDs" - child IFDs linked via special tag values:
    - Tag 0x8769 (ExifIFDPointer): Points to EXIF-specific tags
    - Tag 0x8825 (GPSInfoIFDPointer): Points to GPS tags
    - Tag 0x014A (SubIFDs): Points to thumbnail or other sub-IFDs

    When you extract tags from an IFD, check for these tag IDs. If found, the tag's value is an offset to another IFD. You MUST recursively call your parser to extract those IFDs as well. This is how you "extract all tags from all IFDs" as required.

*   **Tip - Multi-Page TIFF:** Multi-page TIFFs have an IFD chain: IFD0 → IFD1 → IFD2 → ... Each IFD represents one page/image. The chain ends when next_ifd_offset = 0. Loop until you hit 0.

*   **Tip - Image Data Handling:** The acceptance criteria says "ignore image pixel data (metadata only)". TIFF stores image data via:
    - **Strips**: Tag 0x0111 (StripOffsets) + Tag 0x0117 (StripByteCounts)
    - **Tiles**: Tag 0x0144 (TileOffsets) + Tag 0x0145 (TileByteCounts)

    You do NOT need to read or process these data areas. Just extract the tag values themselves (the offsets/counts) as metadata, but don't follow the offsets to read image pixels.

*   **Warning - Circular IFD References:** Malformed TIFF files could have circular IFD chains (e.g., IFD0 → IFD1 → IFD0). You MUST track visited IFD offsets in a `HashSet<u64>` and return an error if you encounter the same offset twice.

*   **Warning - Test Fixture Generation:** The task requires `tests/fixtures/tiff/sample.tif`. Since the fixtures directory is currently empty, you have two options:
    1. Use ImageMagick to generate a multi-page TIFF: `convert -size 100x100 xc:red xc:blue multi.tif`
    2. Use Rust code to generate a minimal TIFF programmatically in your test setup

    Option 1 is simpler if ImageMagick is available. Option 2 gives you more control but is more work. Choose based on environment constraints.

*   **Note - Integration with Existing System:** Your parser will eventually be called from `src/core/operations.rs::read_metadata()` after format detection. For now, focus on the parser itself and the integration test. The operations.rs integration will come later (likely in a subsequent task).

*   **Note - Error Handling Pattern:** The existing `parse_ifd()` function uses `ExifToolError::parse_error_at(message, offset)` for errors. You SHOULD follow the same pattern for consistency. Import from `crate::error::{ExifToolError, Result}`.

*   **Note - Testing Strategy:** The existing IFD parser has excellent test coverage with synthetic test data. You SHOULD follow this pattern:
    - Unit tests with `TestReader` (in-memory data)
    - Integration tests with real TIFF files in fixtures/
    - Test both little-endian and big-endian files
    - Test multi-page TIFFs
    - Test TIFFs with sub-IFDs (EXIF, GPS)
    - Test error cases (truncated files, circular references)

### Architectural Compliance Checklist

- [ ] Use `&dyn FileReader` as input (not `&Path` or raw files)
- [ ] Return `Result<Vec<(u16, Vec<u8>)>, ExifToolError>` or similar structured result
- [ ] Call existing `parse_ifd()` function - do NOT reimplement IFD parsing
- [ ] Handle both little-endian and big-endian byte orders
- [ ] Follow IFD chain via next_ifd_offset until reaching 0
- [ ] Recursively parse sub-IFDs (EXIF, GPS) found via special tags
- [ ] Track visited offsets to prevent infinite loops
- [ ] Add comprehensive unit tests following existing patterns
- [ ] Create integration test file `tests/integration/tiff_tests.rs`
- [ ] Register new test module in `tests/integration.rs`
- [ ] Add `pub mod file_parser;` to `src/parsers/tiff/mod.rs`

### Suggested File Structure for `file_parser.rs`

```rust
//! Full TIFF file parsing
//!
//! Handles complete TIFF file structure including header, IFD chains, and sub-IFDs.

use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
use std::collections::HashSet;

/// TIFF file header structure
struct TiffHeader {
    byte_order: ByteOrder,
    first_ifd_offset: u32,
}

/// Parses TIFF file header (8 bytes)
fn parse_tiff_header(reader: &dyn FileReader) -> Result<TiffHeader> {
    // 1. Read 8-byte header
    // 2. Check byte order marker (0x4949 or 0x4D4D)
    // 3. Verify magic number 42
    // 4. Extract first IFD offset
    // 5. Return TiffHeader
    todo!()
}

/// Extracts all metadata from a TIFF file
pub fn parse_tiff_file(reader: &dyn FileReader) -> Result<Vec<(u16, Vec<u8>)>> {
    // 1. Parse header
    // 2. Initialize visited_offsets HashSet
    // 3. Walk IFD chain starting from first_ifd_offset
    // 4. For each IFD:
    //    a. Check if offset already visited (prevent loops)
    //    b. Call parse_ifd()
    //    c. Look for sub-IFD tags (0x8769, 0x8825)
    //    d. Recursively parse sub-IFDs
    //    e. Read next_ifd_offset and continue or break if 0
    // 5. Return all collected tags
    todo!()
}

/// Helper: reads next IFD offset after an IFD entry array
fn read_next_ifd_offset(
    reader: &dyn FileReader,
    ifd_offset: u64,
    entry_count: u16,
    byte_order: ByteOrder,
) -> Result<u32> {
    // offset = ifd_offset + 2 + (entry_count * 12)
    todo!()
}

#[cfg(test)]
mod tests {
    // Unit tests with TestReader
    // Integration tests are in tests/integration/tiff_tests.rs
}
```

### Summary of Your Task

You are implementing a **TIFF file-level parser** that sits one layer ABOVE the existing IFD parser. Think of it as:

- **Existing (I1.T11):** `parse_ifd(offset)` → parses ONE IFD structure at a specific offset
- **Your Task (I3.T6):** `parse_tiff_file()` → parses ENTIRE .tif file by reading header, walking IFD chains, and orchestrating multiple `parse_ifd()` calls

This is a classic facade/orchestrator pattern. Your code should be mostly glue logic that coordinates the existing, well-tested components. Focus on:
1. Header parsing (8 bytes)
2. IFD chain walking (loop until next_offset = 0)
3. Sub-IFD recursion (check for special tags)
4. Circular reference prevention (HashSet)
5. Comprehensive testing

Good luck! This is a foundational component that will enable TIFF file write support in the next task (I3.T7).
