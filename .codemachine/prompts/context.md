# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I3.T7",
  "iteration_id": "I3",
  "iteration_goal": "Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.",
  "description": "Implement full TIFF file writer in src/writers/tiff_writer.rs (extend I3.T2). Write complete TIFF file structure: header, IFD chain, tag values. Support writing modified metadata back to TIFF file. Preserve image pixel data (copy image data strips/tiles unchanged). Add integration test: read TIFF, modify tag, write, re-read, verify.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I3.T2 TIFF IFD writer, I3.T6 TIFF file parser",
  "target_files": [
    "src/writers/tiff_writer.rs",
    "tests/integration/tiff_write_tests.rs"
  ],
  "input_files": [
    "src/writers/tiff_writer.rs",
    "src/parsers/tiff/file_parser.rs"
  ],
  "deliverables": "Full TIFF file writer, integration test for TIFF modification",
  "acceptance_criteria": "Writer produces valid TIFF file (readable by other tools), preserves image pixel data unchanged, modifies metadata tags correctly, round-trip test: read → modify → write → read → verify, cargo test tiff_write_tests passes",
  "dependencies": ["I3.T2", "I3.T6"],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: iteration-3-plan (from 02_Iteration_I3.md)

```markdown
### Iteration 3: Metadata Writing, TIFF Format & Atomic File Operations

*   **Iteration ID:** `I3`
*   **Goal:** Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.
*   **Prerequisites:** `I2` (tag registry, read operations, CLI foundation, validation engine)
```

### Context: task-i3-t2 (from 02_Iteration_I3.md)

```markdown
*   **Task 3.2: Implement EXIF IFD Serializer (TIFF Writer)**
    *   **Task ID:** `I3.T2`
    *   **Description:** Implement TIFF IFD serializer in `src/writers/tiff_writer.rs`. Create function to serialize MetadataMap EXIF tags back to TIFF IFD structure: (1) Filter tags for EXIF family, (2) Convert TagValue to TIFF data types (Byte, ASCII, Short, Long, Rational), (3) Build IFD entries (tag ID, type, count, value/offset), (4) Handle values >4 bytes (write to separate value area), (5) Calculate offsets, (6) Write IFD header + entries + values. Support both little-endian and big-endian output. Add unit tests verifying round-trip (parse then serialize equals original).
    *   **Acceptance Criteria:**
        *   Serializer produces valid TIFF IFD structure
        *   Handles both little-endian and big-endian
        *   Correctly writes tag entries with type, count, value
        *   Values >4 bytes written to separate area with offset
        *   Round-trip test: parse(serialize(metadata)) == metadata for EXIF tags
        *   `cargo test tiff_writer` passes
```

### Context: task-i3-t6 (from 02_Iteration_I3.md)

```markdown
*   **Task 3.6: Implement Full TIFF File Parser**
    *   **Task ID:** `I3.T6`
    *   **Description:** Extend TIFF parser from I1.T11 to handle standalone TIFF files (not just EXIF segments). Parse TIFF file structure: 8-byte header (byte order, magic number 42, first IFD offset), then IFD chain (IFD0, IFD1 for thumbnails, sub-IFDs for EXIF/GPS). Support multi-page TIFF (follow next IFD offset). Extract all tags from all IFDs. Handle both stripped and tiled image data (ignore pixel data, metadata only). Add integration test with sample TIFF file.
    *   **Acceptance Criteria:**
        *   Parser reads TIFF header and identifies byte order
        *   Parses IFD chain (IFD0 → IFD1 → ... via next IFD offset)
        *   Extracts tags from all IFDs (main image + thumbnail + sub-IFDs)
        *   Ignores image pixel data (metadata only)
        *   Integration test extracts metadata from multi-page TIFF
        *   `cargo test tiff_tests` passes
```

### Context: task-i3-t7 (from 02_Iteration_I3.md)

```markdown
*   **Task 3.7: Implement TIFF File Writer**
    *   **Task ID:** `I3.T7`
    *   **Description:** Implement full TIFF file writer in `src/writers/tiff_writer.rs` (extend I3.T2). Write complete TIFF file structure: header, IFD chain, tag values. Support writing modified metadata back to TIFF file. Preserve image pixel data (copy image data strips/tiles unchanged). Add integration test: read TIFF, modify tag, write, re-read, verify.
    *   **Deliverables:**
        *   Full TIFF file writer
        *   Integration test for TIFF modification
    *   **Acceptance Criteria:**
        *   Writer produces valid TIFF file (readable by other tools)
        *   Preserves image pixel data unchanged
        *   Modifies metadata tags correctly
        *   Round-trip test: read → modify → write → read → verify
        *   `cargo test tiff_write_tests` passes
```

### Context: data-model-overview (from 03_System_Structure_and_Data.md)

```markdown
### 3.6. Data Model Overview & ERD

**Description**: ExifTool-RS operates on files without persistent database storage. The "data model" represents in-memory structures for metadata representation.

#### Key Entities

1. **File**: Represents a media file being processed (JPEG, PNG, etc.)
2. **MetadataMap**: Collection of all metadata tags extracted from a file
3. **TagValue**: A single metadata tag with its name, value, and type information
4. **TagDescriptor**: Definition of a tag (from tag database) including ID, name, type constraints, format family
5. **FormatFamily**: Grouping of related metadata standards (EXIF, XMP, IPTC, MakerNotes)
6. **IFD (Image File Directory)**: TIFF-specific structural element containing tags
```

### Context: alternative-flow-metadata-write (from 04_Behavior_and_Communication.md)

```markdown
#### Alternative Flow: Metadata Write Operation

**Description**: Sequence for **modifying metadata and writing back to file**.

**Key Design Decisions**:

1. **Read-Modify-Write**: Always read existing metadata first to preserve unmodified tags
2. **In-Place vs. Rewrite**: Attempt in-place modification if new metadata fits in existing segment; otherwise rewrite entire file
3. **Atomic Write**: Use temporary file + atomic rename to prevent corruption on crash
4. **Validation Before Write**: Validate tag values against type constraints before any file modification
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/writers/tiff_writer.rs`
    *   **Summary:** This file implements the TIFF IFD serialization (Task I3.T2). It contains the `serialize_ifd()` function that converts a MetadataMap to binary TIFF IFD bytes, supporting both little-endian and big-endian byte orders. The module handles inline values (≤4 bytes) and offset-based values (>4 bytes) correctly. It has comprehensive unit tests and round-trip tests that verify parse→serialize→parse cycles work correctly.
    *   **Recommendation:** You MUST extend this file by adding new functions for full TIFF file writing. The existing `serialize_ifd()` function is a building block you will use. DO NOT modify the existing function - add new functions that call it.
    *   **Key Functions to Reuse:**
        - `serialize_ifd(metadata: &MetadataMap, byte_order: ByteOrder, ifd_start_offset: u64)` - Serializes a single IFD (line 117)
        - `write_u16()`, `write_u32()` - Helper functions for writing multi-byte values in correct endianness (lines 359, 368)
    *   **Critical Detail:** The `serialize_ifd()` function takes an `ifd_start_offset` parameter that tells it where in the file the IFD will be written. This is essential for calculating correct offsets for value data that doesn't fit inline.

*   **File:** `src/parsers/tiff/file_parser.rs`
    *   **Summary:** This file implements the full TIFF file parser (Task I3.T6). It parses the 8-byte TIFF header, walks the IFD chain (IFD0 → IFD1 → ...), recursively parses sub-IFDs (EXIF, GPS), and handles multi-page TIFF files. The parser returns a flat `Vec<(u16, Vec<u8>)>` of all tags from all IFDs.
    *   **Recommendation:** You MUST study this file carefully to understand the TIFF file structure. Your writer must create files with the exact same structure that this parser expects. Key constants like `EXIF_IFD_POINTER` (0x8769), `GPS_INFO_IFD_POINTER` (0x8825) are defined here (lines 80-83).
    *   **Key Structure Details:**
        - TIFF Header: 8 bytes (byte order marker, magic number 42, first IFD offset) - see `parse_tiff_header()` at line 116
        - IFD Chain: Each IFD has an entry count, tag entries (12 bytes each), and a "next IFD offset" field - see `parse_tiff_file()` at line 290
        - Sub-IFDs: Referenced by special tags (0x8769 for EXIF, 0x8825 for GPS, 0xA005 for Interoperability) - handled in loop at line 334
        - Circular Reference Detection: The parser tracks visited offsets to prevent infinite loops (line 297)
    *   **Critical Detail:** The parser extracts ALL tags from ALL IFDs and returns them as a flat list. Your writer will need to intelligently group tags back into the appropriate IFDs.

*   **File:** `src/writers/atomic_writer.rs`
    *   **Summary:** This file implements atomic file writing using the temp-file-and-rename pattern. The `write_atomic(path: &Path, data: &[u8])` function creates a temporary file in the same directory, writes data, calls fsync(), and atomically renames to the target path (line 93).
    *   **Recommendation:** You MUST use `write_atomic()` for all file write operations. This ensures that files are never left in a corrupted state if the program crashes during writing. The atomic writer handles all error cases and cleanup automatically.
    *   **Critical Detail:** The temp file MUST be in the same directory as the target file for atomic rename to work (cannot cross filesystem boundaries) - see comment at line 95.

*   **File:** `tests/integration/tiff_tests.rs`
    *   **Summary:** This file contains comprehensive integration tests for the TIFF parser. It tests parsing of the test fixture `tests/fixtures/tiff/sample.tif`, which is a multi-page TIFF with IFD0, IFD1, and an EXIF sub-IFD. The tests verify tag extraction, byte order handling, IFD chain traversal, and sub-IFD recursion.
    *   **Recommendation:** You SHOULD create a parallel file `tests/integration/tiff_write_tests.rs` with similar test structure. Use the same test fixture for round-trip testing.
    *   **Test Pattern:** The existing tests use this pattern: open file → parse → verify tags. Your tests should follow: open file → parse → modify metadata → write → re-parse → verify modifications.

### Implementation Tips & Notes

*   **Tip #1: Image Data Preservation Strategy** - The task requires preserving image pixel data unchanged. TIFF images store pixel data in "strips" or "tiles" referenced by tags like `StripOffsets` (0x0111), `StripByteCounts` (0x0117), `TileOffsets` (0x0144), and `TileByteCounts` (0x0145). You MUST copy these data areas byte-for-byte from the original file. The strategy is:
    1. Parse the original TIFF file completely (using `parse_tiff_file()`)
    2. Read the file into memory or track the locations of image data areas
    3. When writing the new file, copy the image data blocks unchanged
    4. Update only the metadata IFDs and their offsets

*   **Tip #2: Tag Grouping Strategy** - The parser returns a flat list of all tags from all IFDs. When writing, you need to reconstruct the IFD structure. Here's a recommended strategy:
    1. Group tags by their IFD type (main image tags go in IFD0, thumbnail tags in IFD1, EXIF tags in EXIF sub-IFD)
    2. Use tag IDs to determine which IFD they belong to (e.g., 0x010F, 0x0110 are main IFD tags; 0x829A, 0x829D are EXIF tags)
    3. Create special pointer tags (0x8769 for EXIF sub-IFD) that reference the sub-IFD locations
    4. Write IFDs in order: header → IFD0 → image data → EXIF sub-IFD → IFD1 (if present)

*   **Tip #3: Offset Calculation** - TIFF files use absolute byte offsets throughout. You must carefully calculate and update all offsets:
    - Header is always 8 bytes
    - First IFD typically starts at offset 8
    - Each IFD's size = 2 (count) + 12×entries + 4 (next IFD offset) + size of value data area
    - Image data offsets must be updated if metadata size changes
    - All offsets must be written in the correct byte order (little-endian or big-endian)

*   **Tip #4: Round-Trip Testing** - The acceptance criteria require a round-trip test. Here's the exact test pattern you MUST implement:
    1. Read original TIFF file using `parse_tiff_file()`
    2. Create a MetadataMap from parsed tags (you'll need to convert raw bytes to TagValue objects)
    3. Modify one or more tags (e.g., change Make from "TestCamera" to "ModifiedCamera")
    4. Write the modified metadata using your new TIFF writer function
    5. Re-parse the written file using `parse_tiff_file()`
    6. Verify the modified tags have the new values
    7. Verify unmodified tags are unchanged
    8. CRITICALLY: Verify image data (if any) is unchanged by comparing StripOffsets/TileByteCounts

*   **Warning:** The existing `serialize_ifd()` function only handles EXIF tags (filters for "EXIF:" prefix at line 127). For a full TIFF file writer, you may need to serialize tags from multiple families (EXIF, GPS, TIFF base tags). Consider creating a variant function or adding a parameter to control tag filtering.

*   **Warning:** TIFF files can be extremely complex with multiple image pages, thumbnails, strips vs. tiles, etc. For your initial implementation (to pass acceptance criteria), focus on single-page TIFF files with simple strip-based image data. The test fixture `tests/fixtures/tiff/sample.tif` is multi-page but metadata-focused. Ensure your implementation handles at minimum: header writing, IFD chain (IFD0 → IFD1), and EXIF sub-IFD.

*   **Note:** The `ByteOrder` enum from `src/parsers/tiff/ifd_parser.rs` is used throughout the codebase. Make sure your write functions accept and honor the byte order parameter. The byte order should match the original file's byte order to maintain compatibility.

*   **Note:** Error handling is critical for file writing operations. Use the `Result<()>` pattern consistently and provide descriptive error messages. The `ExifToolError` type has helper constructors like `parse_error()`, `io_error()`, etc. Use these for consistency with the rest of the codebase.

### Function Signature Recommendations

Based on my analysis, here are the key function signatures you should implement in `src/writers/tiff_writer.rs`:

```rust
/// Writes a complete TIFF file with modified metadata
///
/// This is the main entry point for TIFF file writing.
/// It reads the original file, extracts image data, and writes
/// a new TIFF file with updated metadata while preserving pixel data.
pub fn write_tiff_file(
    path: &Path,
    original_reader: &dyn FileReader,
    modified_metadata: &MetadataMap,
) -> Result<()>

/// Reconstructs the complete TIFF file bytes from components
///
/// Helper function that assembles: header + IFDs + image data
fn reconstruct_tiff_structure(
    original_reader: &dyn FileReader,
    header: &TiffHeader,
    modified_metadata: &MetadataMap,
) -> Result<Vec<u8>>

/// Copies image data (strips/tiles) from original file unchanged
///
/// Preserves pixel data by copying the strips/tiles referenced
/// by StripOffsets/StripByteCounts or TileOffsets/TileByteCounts tags
fn copy_image_data(
    original_reader: &dyn FileReader,
    strip_offsets: &[u32],
    strip_byte_counts: &[u32],
) -> Result<Vec<u8>>
```

These are suggestions - you may design the internal functions differently, but the public API should follow this pattern.

### Integration Test Structure

Your `tests/integration/tiff_write_tests.rs` file should include at minimum:

```rust
#[test]
fn test_round_trip_tiff_modification() {
    // 1. Read original TIFF
    let path = Path::new("tests/fixtures/tiff/sample.tif");
    let reader = BufferedReader::new(path).unwrap();
    let tags = parse_tiff_file(&reader).unwrap();

    // 2. Convert to MetadataMap and modify
    let mut metadata = MetadataMap::new();
    // ... conversion and modification logic ...

    // 3. Write modified TIFF
    write_tiff_file(path_out, &reader, &metadata).unwrap();

    // 4. Re-read and verify
    let reader2 = BufferedReader::new(path_out).unwrap();
    let tags2 = parse_tiff_file(&reader2).unwrap();
    // ... verification logic ...
}
```

Make sure this test passes to meet the acceptance criteria.
