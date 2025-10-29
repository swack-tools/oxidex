# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I3.T6",
  "iteration_id": "I3",
  "iteration_goal": "Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF segments), implement metadata serialization, and add tag modification capabilities to CLI.",
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

### Context: TIFF Format in Architecture (from 03_System_Structure_and_Data.md)

The architecture describes TIFF as a key format that ExifTool-RS must support. TIFF files use a sophisticated structure based on Image File Directories (IFDs) that organize metadata tags. The system uses a hexagonal architecture where format-specific parsers (like TIFF) are adapters that implement the `FormatParser` trait.

**Key Architectural Points:**
- **TIFF Parser** is an infrastructure adapter using nom-based parsing
- Implements `FormatParser` port interface
- Must parse IFD (Image File Directory) structures
- IFD is described as: "Data structure in TIFF format organizing metadata tags. Contains array of 12-byte tag entries (tag ID, type, count, value/offset). IFDs can be nested (EXIF sub-IFD, GPS sub-IFD)."

### Context: TIFF File Structure (from TIFF Specification)

**TIFF File Format Overview:**
```
Bytes 0-1:   Byte Order Indicator
             0x4949 ("II") = Little-Endian
             0x4D4D ("MM") = Big-Endian
Bytes 2-3:   Magic Number 42 (0x002A in detected byte order)
Bytes 4-7:   Offset to first IFD (4 bytes, typically 8 for files starting with IFD)

At IFD Offset:
  Bytes 0-1:     Entry Count (number of tags)
  Bytes 2-...:   Array of 12-byte IFD entries
  Last 4 bytes:  Offset to next IFD (0 if last IFD)
```

**IFD Chain Navigation:**
- IFD0: Main image metadata
- IFD1: Thumbnail metadata (if present)
- Sub-IFDs: EXIF, GPS, Interoperability (referenced by special tags)
- Multi-page TIFF: Follow next IFD offset chain

### Context: Task I3.T6 Details (from 02_Iteration_I3.md)

**Full Task Description:**
Extend TIFF parser from I1.T11 to handle standalone TIFF files (not just EXIF segments). Parse TIFF file structure: 8-byte header (byte order, magic number 42, first IFD offset), then IFD chain (IFD0, IFD1 for thumbnails, sub-IFDs for EXIF/GPS). Support multi-page TIFF (follow next IFD offset). Extract all tags from all IFDs. Handle both stripped and tiled image data (ignore pixel data, metadata only).

**Deliverables:**
- Full TIFF file parser
- Support for multi-page TIFF
- Integration test

**Acceptance Criteria:**
- Parser reads TIFF header and identifies byte order
- Parses IFD chain (IFD0 → IFD1 → ... via next IFD offset)
- Extracts tags from all IFDs (main image + thumbnail + sub-IFDs)
- Ignores image pixel data (metadata only)
- Integration test extracts metadata from multi-page TIFF
- `cargo test tiff_tests` passes

**Dependencies:** I1.T11 (TIFF IFD parser - already complete)

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/tiff/ifd_parser.rs`
    *   **Summary:** This file contains the foundational IFD parsing logic that you MUST build upon. It includes:
        - `ByteOrder` enum (LittleEndian/BigEndian) - YOU MUST reuse this
        - `IfdEntry` struct representing a 12-byte tag entry
        - `parse_ifd()` function that parses a single IFD at a given offset
        - Helper functions for both little-endian and big-endian parsing
        - Comprehensive unit tests demonstrating how IFDs work
    *   **Recommendation:** YOU MUST import and use `parse_ifd()` from this module. It handles all the complex logic for parsing individual IFDs including inline values, offsets, type validation, and error handling. DO NOT reimplement IFD parsing - call this function for each IFD in the chain.
    *   **Critical Note:** The existing `parse_ifd()` function returns `Vec<(u16, Vec<u8>)>` which is a vector of (tag_id, raw_value_bytes) pairs. This is exactly what you need to collect from each IFD in the file.

*   **File:** `src/parsers/tiff/mod.rs`
    *   **Summary:** This is the TIFF module declaration file. Currently it only declares submodules (ifd_parser, makernote_parser, tag_parser) but does not export any public parsing functions.
    *   **Recommendation:** YOU MUST add `pub mod file_parser;` to this file to expose your new file parser module. You SHOULD also add a public convenience function like `pub use file_parser::parse_tiff_file;` for easy access.

*   **File:** `src/core/operations.rs`
    *   **Summary:** This file orchestrates the metadata reading workflow. It contains:
        - `read_metadata()` function that opens files, detects format, and routes to appropriate parsers
        - `parse_jpeg_metadata()` that shows the pattern for format-specific parsing
        - `parse_tiff_metadata()` stub that currently only handles EXIF segments from JPEG
    *   **Recommendation:** YOU MUST update the `parse_tiff_metadata()` function to call your new full TIFF file parser. The function signature is already correct - it takes a `FileReader` reference and returns `Result<MetadataMap>`.
    *   **Pattern to Follow:** Look at how `parse_jpeg_metadata()` is structured:
        1. Parses file structure (segments)
        2. Calls IFD parser for metadata sections
        3. Converts raw tag values to MetadataMap
        4. Returns MetadataMap

*   **File:** `src/parsers/format_detector.rs`
    *   **Summary:** Handles file format detection via magic bytes. Already correctly identifies TIFF files (both little-endian 0x49 0x49 0x2A 0x00 and big-endian 0x4D 0x4D 0x00 0x2A).
    *   **Recommendation:** NO changes needed here. Format detection already works for TIFF files.

*   **File:** `src/core/file_reader_trait.rs` and `src/io/mmap_reader.rs`
    *   **Summary:** These define the FileReader trait and its implementation for memory-mapped file access.
    *   **Recommendation:** YOU MUST use `reader.read(offset, length)` to read TIFF header and IFD data. The reader handles all I/O and boundary checking for you.

### Implementation Tips & Notes

*   **Tip:** The TIFF header parsing is straightforward:
    ```rust
    // Read 8-byte header
    let header = reader.read(0, 8)?;

    // Bytes 0-1: Byte order
    let byte_order = match &header[0..2] {
        [0x49, 0x49] => ByteOrder::LittleEndian,
        [0x4D, 0x4D] => ByteOrder::BigEndian,
        _ => return Err(ExifToolError::parse_error("Invalid TIFF byte order marker")),
    };

    // Bytes 2-3: Magic number (should be 42)
    let magic = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([header[2], header[3]]),
        ByteOrder::BigEndian => u16::from_be_bytes([header[2], header[3]]),
    };
    if magic != 42 {
        return Err(ExifToolError::parse_error("Invalid TIFF magic number"));
    }

    // Bytes 4-7: First IFD offset
    let first_ifd_offset = match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([header[4], header[5], header[6], header[7]]),
        ByteOrder::BigEndian => u32::from_be_bytes([header[4], header[5], header[6], header[7]]),
    };
    ```

*   **Tip:** To parse the IFD chain, you need to read the "next IFD offset" after each IFD. From the existing code in `ifd_parser.rs`, I can see the IFD structure includes this offset at the end:
    ```rust
    // After parsing all entries in an IFD, read the next IFD offset
    let next_ifd_offset_location = ifd_offset + 2 + (entry_count as u64 * 12);
    let next_offset_bytes = reader.read(next_ifd_offset_location, 4)?;
    let next_ifd_offset = match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([next_offset_bytes[0], next_offset_bytes[1], next_offset_bytes[2], next_offset_bytes[3]]),
        ByteOrder::BigEndian => u32::from_be_bytes([next_offset_bytes[0], next_offset_bytes[1], next_offset_bytes[2], next_offset_bytes[3]]),
    };
    // If next_ifd_offset == 0, we've reached the end of the chain
    ```

*   **Note:** The acceptance criteria says to "ignore image pixel data (metadata only)". This means you DON'T need to parse:
    - StripOffsets/StripByteCounts (tag 0x0111/0x0117)
    - TileOffsets/TileByteCounts (tag 0x0144/0x0145)
    - ImageData itself
    Just extract the metadata tags and skip the pixel data references.

*   **Note:** Sub-IFDs (EXIF, GPS) are referenced by special tags:
    - EXIF IFD Pointer: tag 0x8769
    - GPS IFD Pointer: tag 0x8825
    - Interoperability IFD: tag 0xA005
    These tags contain offsets to additional IFDs that you SHOULD parse recursively using the same `parse_ifd()` function.

*   **Warning:** The existing `parse_ifd()` function already includes comprehensive error handling for:
    - IFD offset beyond file size
    - Truncated IFD data
    - Invalid value offsets
    - Unknown tag types
    DO NOT duplicate this error handling - just handle errors at the file-level (invalid header, failed IFD chain navigation, etc.)

*   **Critical Pattern:** Your `parse_tiff_file()` function should follow this structure:
    1. Parse 8-byte TIFF header → get byte_order and first_ifd_offset
    2. Initialize empty `Vec<(u16, Vec<u8>)>` to collect all tags
    3. Loop through IFD chain:
        - Call `parse_ifd(reader, current_offset, byte_order)` → get tags from this IFD
        - Append tags to collection
        - Read next IFD offset (4 bytes after the IFD entries)
        - If next_offset == 0, break loop
        - Otherwise, set current_offset = next_offset and continue
    4. Convert collected tags to MetadataMap (see existing code in operations.rs for pattern)
    5. Return MetadataMap

*   **Testing Strategy:** For the integration test:
    - Create a minimal valid TIFF file with at least 2 IFDs (IFD0 and IFD1) to test the chain
    - Include common tags like Make (0x010F), Model (0x0110), DateTime (0x0132)
    - You can reference the test helpers in `ifd_parser.rs` tests to see how to construct valid TIFF data
    - Test should verify: correct number of tags extracted, specific tag values are correct, multi-page support works

*   **File Organization:**
    - Create `src/parsers/tiff/file_parser.rs` for the full TIFF file parsing logic
    - Update `src/parsers/tiff/mod.rs` to expose the new module
    - Update `src/core/operations.rs` to call your new parser in `parse_tiff_metadata()`
    - Create `tests/integration/tiff_tests.rs` with at least one test case
    - Create `tests/fixtures/tiff/sample.tif` as a test fixture

### Code Quality Reminders

*   Follow the existing code style: comprehensive documentation comments with examples
*   Add `#[allow(dead_code)]` temporarily if needed during development
*   Use `Result<T>` and proper error handling with `ExifToolError`
*   Write descriptive error messages that include offsets for debugging
*   Add unit tests in the file_parser.rs module for header parsing
*   Add integration tests in tests/integration/tiff_tests.rs for end-to-end validation
