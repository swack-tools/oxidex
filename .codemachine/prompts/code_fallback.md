# Code Refinement Task

The previous code submission did not pass verification. The task has NOT been implemented at all. You must implement the complete TIFF file parser from scratch.

---

## Original Task Description

**Task I3.T6: Implement Full TIFF File Parser**

Extend TIFF parser from I1.T11 to handle standalone TIFF files (not just EXIF segments). Parse TIFF file structure: 8-byte header (byte order, magic number 42, first IFD offset), then IFD chain (IFD0, IFD1 for thumbnails, sub-IFDs for EXIF/GPS). Support multi-page TIFF (follow next IFD offset). Extract all tags from all IFDs. Handle both stripped and tiled image data (ignore pixel data, metadata only). Add integration test with sample TIFF file.

**Acceptance Criteria:**
- Parser reads TIFF header and identifies byte order
- Parses IFD chain (IFD0 → IFD1 → ... via next IFD offset)
- Extracts tags from all IFDs (main image + thumbnail + sub-IFDs)
- Ignores image pixel data (metadata only)
- Integration test extracts metadata from multi-page TIFF
- `cargo test tiff_tests` passes

---

## Issues Detected

**Critical: Complete Non-Implementation**

*   **Missing File:** `src/parsers/tiff/file_parser.rs` does not exist. This is the main deliverable for the task.
*   **Missing Test:** `tests/integration/tiff_tests.rs` does not exist. No integration tests have been created.
*   **Missing Fixture:** `tests/fixtures/tiff/sample.tif` does not exist. No test fixture file has been provided.
*   **Missing Module Declaration:** `src/parsers/tiff/mod.rs` does not contain `pub mod file_parser;` declaration.
*   **Wrong Work Done:** The only changes made were DateTime parsing fixes in `src/core/operations.rs`, which are related to task I3.T5, not I3.T6.

---

## Best Approach to Fix

You MUST implement the complete TIFF file parser as specified in the task. Follow these steps in order:

### Step 1: Create `src/parsers/tiff/file_parser.rs`

This file must contain the full TIFF file-level parser. The implementation should:

1. **Parse TIFF Header (8 bytes):**
   - Read bytes 0-1: Byte order marker (0x4949 = "II" little-endian, 0x4D4D = "MM" big-endian)
   - Read bytes 2-3: Magic number 42 (validate this matches expected value)
   - Read bytes 4-7: Offset to first IFD (u32, respecting byte order)
   - Return `TiffHeader` struct containing `ByteOrder` and `first_ifd_offset`

2. **Walk IFD Chain:**
   - Start at `first_ifd_offset` from header
   - For each IFD:
     - Read 2-byte entry count at IFD offset
     - Call existing `parse_ifd(reader, offset, byte_order)` from `ifd_parser.rs` to get tags
     - Calculate position of "next IFD offset": `offset + 2 + (entry_count * 12)`
     - Read 4-byte next IFD offset (u32, respecting byte order)
     - If next offset is 0, stop. Otherwise, loop to next IFD
   - Track visited offsets in a `HashSet<u64>` to prevent infinite loops

3. **Handle Sub-IFDs:**
   - After parsing each IFD, scan the extracted tags for sub-IFD pointers:
     - Tag 0x8769 (ExifIFDPointer): Contains offset to EXIF sub-IFD
     - Tag 0x8825 (GPSInfoIFDPointer): Contains offset to GPS sub-IFD
     - Tag 0x014A (SubIFDs): Contains offset(s) to thumbnail/sub-IFDs
   - For each sub-IFD pointer found, recursively call `parse_ifd()` at that offset
   - Add extracted sub-IFD tags to the main tag collection

4. **Return All Tags:**
   - Collect all tags from all IFDs (main IFD chain + all sub-IFDs) into a single `Vec<(u16, Vec<u8>)>`
   - Each tuple is (tag_id, raw_value_bytes)

**Key Functions to Implement:**

```rust
/// Parses the 8-byte TIFF file header
fn parse_tiff_header(reader: &dyn FileReader) -> Result<TiffHeader>

/// Main entry point: parses complete TIFF file and returns all tags
pub fn parse_tiff_file(reader: &dyn FileReader) -> Result<Vec<(u16, Vec<u8>)>>

/// Helper: reads the "next IFD offset" field after an IFD's entry array
fn read_next_ifd_offset(
    reader: &dyn FileReader,
    ifd_offset: u64,
    entry_count: u16,
    byte_order: ByteOrder,
) -> Result<u32>

/// Helper: reads 2-byte entry count at IFD offset
fn read_entry_count(
    reader: &dyn FileReader,
    ifd_offset: u64,
    byte_order: ByteOrder,
) -> Result<u16>

/// Helper: extracts u32 offset from tag value bytes (for sub-IFD pointers)
fn extract_u32_from_tag_value(value: &[u8], byte_order: ByteOrder) -> Option<u32>
```

**Critical Requirements:**
- MUST import and use `parse_ifd()` from `crate::parsers::tiff::ifd_parser` - DO NOT reimplement IFD parsing
- MUST accept `&dyn FileReader` as input (hexagonal architecture compliance)
- MUST return `Result<Vec<(u16, Vec<u8>)>, ExifToolError>`
- MUST handle both little-endian and big-endian byte orders
- MUST prevent infinite loops with `HashSet<u64>` to track visited offsets
- DO NOT read or parse image pixel data (only extract metadata tags)

### Step 2: Update `src/parsers/tiff/mod.rs`

Add the following line after the existing module declarations:

```rust
pub mod file_parser;
```

Optionally, also add a re-export:

```rust
pub use file_parser::parse_tiff_file;
```

### Step 3: Create Test Fixture `tests/fixtures/tiff/sample.tif`

You have two options:

**Option A (Recommended):** Use ImageMagick to generate a multi-page TIFF:
```bash
mkdir -p tests/fixtures/tiff
convert -size 100x100 xc:red xc:blue tests/fixtures/tiff/sample.tif
```

**Option B:** Write Rust code to generate a minimal valid TIFF file programmatically. This gives you full control but is more work.

The TIFF file MUST:
- Be a valid multi-page TIFF (at least 2 IFDs)
- Contain some metadata tags in each IFD
- Optionally contain EXIF and/or GPS sub-IFDs

### Step 4: Create `tests/integration/tiff_tests.rs`

This file must contain integration tests that:

1. **Test Single-Page TIFF:**
   - Create or use a simple single-page TIFF fixture
   - Call `parse_tiff_file()` via appropriate API
   - Verify expected tags are extracted (e.g., ImageWidth, ImageLength)

2. **Test Multi-Page TIFF:**
   - Use `tests/fixtures/tiff/sample.tif`
   - Call `parse_tiff_file()`
   - Verify tags from multiple IFDs are extracted
   - Verify correct number of pages detected

3. **Test Both Byte Orders:**
   - Test with little-endian TIFF file
   - Test with big-endian TIFF file

4. **Test Sub-IFD Extraction:**
   - If possible, create fixture with EXIF and/or GPS sub-IFD
   - Verify tags from sub-IFDs are extracted

5. **Test Error Cases:**
   - Test with truncated file (should return error)
   - Test with invalid magic number (should return error)

**Test Structure Example:**

```rust
use exiftool_rs::io::TestReader;
use exiftool_rs::parsers::tiff::file_parser::parse_tiff_file;

#[test]
fn test_parse_simple_tiff() {
    // Load test fixture
    let data = std::fs::read("tests/fixtures/tiff/sample.tif").unwrap();
    let reader = TestReader::new(data);

    // Parse TIFF file
    let tags = parse_tiff_file(&reader).unwrap();

    // Verify results
    assert!(!tags.is_empty(), "Should extract at least some tags");
    // Add more specific assertions based on fixture content
}

#[test]
fn test_multi_page_tiff() {
    // Similar structure, verify multiple IFDs
}

#[test]
fn test_invalid_tiff_header() {
    let data = vec![0xFF, 0xFF, 0xFF, 0xFF]; // Invalid header
    let reader = TestReader::new(data);

    let result = parse_tiff_file(&reader);
    assert!(result.is_err(), "Should fail on invalid header");
}
```

### Step 5: Register Integration Tests

If `tests/integration.rs` or a similar test registration file exists, add:

```rust
mod tiff_tests;
```

### Step 6: Add Unit Tests

Add unit tests in `src/parsers/tiff/file_parser.rs` (in `#[cfg(test)] mod tests { ... }` block) for:
- TIFF header parsing (both byte orders)
- Next IFD offset reading
- Entry count reading
- Circular reference detection

Follow the patterns in `ifd_parser.rs` unit tests.

### Step 7: Verify Implementation

Run the following commands to verify your implementation:

```bash
# Run all tests
cargo test

# Run only TIFF tests
cargo test tiff_tests

# Check for linting errors
cargo clippy

# Build project
cargo build
```

All tests must pass and there must be no clippy warnings.

---

## Key Reminders

1. **Reuse Existing Code:** You MUST call `parse_ifd()` from `ifd_parser.rs`. DO NOT reimplement IFD parsing logic.

2. **Hexagonal Architecture:** Use `&dyn FileReader` as input, NOT `&Path` or raw files.

3. **Error Handling:** Use `ExifToolError` from `crate::error` for all errors. Follow patterns in existing parser code.

4. **Byte Order:** Parse byte order from TIFF header once, then use consistently for all subsequent reads.

5. **Sub-IFDs:** Don't forget to recursively parse EXIF (0x8769) and GPS (0x8825) sub-IFDs.

6. **Circular References:** Use `HashSet<u64>` to track visited IFD offsets and prevent infinite loops.

7. **Ignore Pixel Data:** Extract tags that reference image data (StripOffsets, TileOffsets, etc.) but DO NOT read the actual pixel data.

---

## Summary

You must implement the complete TIFF file parser from scratch. This is a NEW implementation, not a fix to existing code. Focus on:

1. Creating `file_parser.rs` with header parsing, IFD chain walking, and sub-IFD recursion
2. Reusing the existing `parse_ifd()` function from `ifd_parser.rs`
3. Creating comprehensive integration tests
4. Generating or obtaining a test fixture TIFF file
5. Ensuring all tests pass and no linting errors exist

This is a foundational component that will enable TIFF file write support in subsequent tasks.
