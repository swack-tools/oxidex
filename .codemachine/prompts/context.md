# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I1.T14",
  "iteration_id": "I1",
  "iteration_goal": "Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.",
  "description": "Create integration test in tests/integration/jpeg_tests.rs that demonstrates end-to-end workflow: (1) Use MMapReader to open sample JPEG file (create sample with EXIF in tests/fixtures/jpeg/), (2) Detect format using format_detector, (3) Parse JPEG segments, (4) Parse EXIF IFD from APP1 segment, (5) Extract at least 3 tag values (Make, Model, DateTime), (6) Print extracted values. This test validates the entire parsing pipeline from I1.T8-T11. Test should pass.",
  "agent_type_hint": "BackendAgent",
  "inputs": "All code from I1.T8-T11",
  "target_files": [
    "tests/integration/jpeg_tests.rs",
    "tests/fixtures/jpeg/sample_with_exif.jpg"
  ],
  "input_files": [
    "src/parsers/format_detector.rs",
    "src/parsers/jpeg/segment_parser.rs",
    "src/parsers/tiff/ifd_parser.rs",
    "src/io/mmap_reader.rs"
  ],
  "deliverables": "Integration test demonstrating end-to-end JPEG EXIF extraction, sample JPEG file with EXIF metadata",
  "acceptance_criteria": "Test successfully opens JPEG file, format detector identifies file as JPEG, segment parser finds APP1 segment, IFD parser extracts Make, Model, DateTime tags, test assertions verify tag values are non-empty strings, cargo test jpeg_tests passes",
  "dependencies": [
    "I1.T8",
    "I1.T9",
    "I1.T10",
    "I1.T11"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: Integration Test Plan - Success Criteria

**From:** docs/testing/integration_test_plan.md (Section 1.3)

The integration test plan defines:

1. **Functional Correctness**: 99%+ tag value match rate vs. Perl ExifTool for well-formed files
2. **Graceful Degradation**: Appropriate error handling for malformed files (no crashes/hangs)
3. **Performance**: Within 2x performance of Perl ExifTool for batch operations
4. **Cross-Platform**: Pass on Linux, macOS, and Windows
5. **Regression Prevention**: No degradation in match rate or performance across commits

### Context: Integration Test Plan - Test Image Corpus

**From:** docs/testing/integration_test_plan.md (Section 2)

- Target: 100+ images across all supported formats
- JPEG category includes: Simple (basic EXIF), Complex (GPS + maker notes + thumbnails), Edge Cases, Malformed
- Test fixture location: `tests/fixtures/jpeg/`
- Sample already exists: `tests/fixtures/jpeg/sample_with_exif.jpg`

### Context: Integration Test Plan - Test Implementation Pattern

**From:** docs/testing/integration_test_plan.md (Section 6.1)

Example test pattern from the integration test plan:

```rust
#[test]
fn test_format_jpeg_simple() {
    let metadata = extract_metadata("tests/fixtures/jpeg/simple/canon_eos_5d.jpg").unwrap();
    assert_eq!(metadata.get("EXIF:Make").unwrap().as_string(), "Canon");
    assert_eq!(metadata.get("EXIF:Model").unwrap().as_string(), "Canon EOS 5D");
    assert!(metadata.contains_key("EXIF:DateTimeOriginal"));
}
```

### Context: Integration Test Plan - Tag Extraction Validation

**From:** docs/testing/integration_test_plan.md (Section 6.2)

- Tags to extract: Make (0x010F), Model (0x0110), DateTime (0x0132)
- Values should be non-empty strings
- EXIF tags are stored in APP1 segments with "Exif\0\0" header followed by TIFF IFD structure

### Context: Task I1.T14 Specification

**From:** .codemachine/artifacts/plan/02_Iteration_I1.md

**Task Requirements**:
1. Create integration test demonstrating end-to-end workflow
2. Use MMapReader to open JPEG file with EXIF
3. Detect format using format_detector
4. Parse JPEG segments to find APP1
5. Parse EXIF IFD from APP1 segment
6. Extract at least 3 tag values: Make, Model, DateTime
7. Test should PASS with cargo test

**File Structure**:
- Test file: `tests/integration/jpeg_tests.rs`
- Sample JPEG: `tests/fixtures/jpeg/sample_with_exif.jpg` (already exists!)
- Integration tests directory: `tests/integration/` (already exists!)

**Acceptance Criteria**:
- Test successfully opens JPEG file
- Format detector identifies file as JPEG
- Segment parser finds APP1 segment
- IFD parser extracts Make, Model, DateTime tags
- Test assertions verify tag values are non-empty strings
- `cargo test jpeg_tests` passes

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/format_detector.rs`
    *   **Summary:** Implements format detection by examining magic bytes. JPEG detection looks for `0xFF 0xD8 0xFF` signature.
    *   **Key Function:** `pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat>`
    *   **Returns:** `FileFormat::JPEG` for JPEG files, `FileFormat::Unknown` for unrecognized formats
    *   **Recommendation:** Import and use `detect_format()` function. It takes any `&dyn FileReader` and returns `FileFormat` enum.

*   **File:** `src/parsers/jpeg/segment_parser.rs`
    *   **Summary:** Parses JPEG segment structure using nom combinators. Identifies APP1 segments (0xFFE1) containing EXIF/XMP data.
    *   **Key Function:** `pub fn parse_segments<'a>(reader: &'a dyn FileReader) -> Result<Vec<Segment<'a>>, ExifToolError>`
    *   **Key Struct:** `Segment<'a>` with fields: `marker: u16`, `offset: u64`, `data: &'a [u8]`
    *   **Helper Methods:** `segment.is_app1()` returns true for APP1 segments (marker == 0xFFE1)
    *   **EXIF Identification:** APP1 data starts with "Exif\0\0" (6 bytes), followed by TIFF header
    *   **Recommendation:** Call `parse_segments()`, iterate through results, filter for `segment.is_app1()`, check if data starts with `b"Exif\0\0"`, then extract TIFF data starting at byte offset 6.

*   **File:** `src/parsers/tiff/ifd_parser.rs`
    *   **Summary:** Parses TIFF Image File Directory structure with support for both little-endian and big-endian byte order.
    *   **Key Function:** `pub fn parse_ifd(reader: &dyn FileReader, ifd_offset: u64, byte_order: ByteOrder) -> Result<Vec<(u16, Vec<u8>)>>`
    *   **Returns:** Vector of `(tag_id: u16, raw_value: Vec<u8>)` tuples
    *   **Byte Order Enum:** `pub enum ByteOrder { LittleEndian, BigEndian }`
    *   **Tag IDs:** Make = 0x010F, Model = 0x0110, DateTime = 0x0132
    *   **TIFF Structure:** First 2 bytes = byte order marker, bytes 2-3 = magic number 42, bytes 4-7 = IFD offset (usually 8)
    *   **Recommendation:** For EXIF in JPEG APP1 segments, TIFF data starts after "Exif\0\0" header. Read byte order from first 2 bytes, then call `parse_ifd()` with appropriate `ByteOrder` enum and IFD offset from TIFF header.

*   **File:** `src/io/mmap_reader.rs`
    *   **Summary:** Memory-mapped file reader implementing `FileReader` trait. Provides zero-copy access to file contents.
    *   **Key Function:** `pub fn new(path: &Path) -> io::Result<Self>`
    *   **Implements:** `FileReader` trait with `read(&self, offset: u64, length: usize) -> io::Result<&[u8]>` and `size(&self) -> u64`
    *   **Recommendation:** Create reader with `MMapReader::new(Path::new("tests/fixtures/jpeg/sample_with_exif.jpg"))?`

### Implementation Tips & Notes

*   **Tip:** The test fixture `tests/fixtures/jpeg/sample_with_exif.jpg` ALREADY EXISTS! I verified this with `find` command. You do NOT need to create it.

*   **Tip:** The `tests/integration/` directory already exists with a `jpeg_tests.rs` file (13KB). You should READ this file first to see if there's already partial implementation or if it needs to be completely rewritten.

*   **Tip:** EXIF data in JPEG APP1 segments has a specific structure:
    1. APP1 marker: 0xFFE1 (2 bytes)
    2. Segment length: big-endian u16 (2 bytes)
    3. EXIF identifier: "Exif\0\0" (6 bytes)
    4. TIFF header: byte order marker (2 bytes) + magic 42 (2 bytes) + IFD offset (4 bytes)
    5. IFD data starting at the offset specified in TIFF header

*   **Note:** When extracting tag values from `parse_ifd()`, the function returns `Vec<(u16, Vec<u8>)>`. The raw bytes need to be interpreted based on tag type. For ASCII strings (Make, Model, DateTime), the bytes are null-terminated strings. You should convert them with `String::from_utf8_lossy(&bytes).trim_end_matches('\0')`.

*   **Note:** The byte order detection is critical. EXIF data can be either little-endian (Intel, "II") or big-endian (Motorola, "MM"). The first 2 bytes of the TIFF header indicate which to use:
    - `[0x49, 0x49]` = little-endian (`ByteOrder::LittleEndian`)
    - `[0x4D, 0x4D]` = big-endian (`ByteOrder::BigEndian`)

*   **Warning:** The TIFF IFD offset in the header is relative to the START of the TIFF data (after "Exif\0\0"), NOT the start of the file. When creating a reader for the IFD parser, you need to create a temporary in-memory reader around the TIFF data slice (starting after "Exif\0\0").

    The cleanest approach is to create a `TestReader` struct (similar to those in existing test files) that implements `FileReader` trait and wraps the TIFF data slice.

*   **Tip:** Look at the existing test implementations in `src/parsers/jpeg/segment_parser.rs` (lines 280-566) and `src/parsers/tiff/ifd_parser.rs` (lines 303-698) for patterns on how to create test readers and structure assertions.

*   **Note:** The test should be structured as an integration test, not a unit test. This means:
    - It should test the FULL workflow from file opening to tag extraction
    - It should use the actual file reader implementations (MMapReader)
    - It should demonstrate that all components work together correctly
    - Print statements (using `println!`) are acceptable for debugging but should show actual extracted values

*   **Critical:** Make sure to handle the Result types properly. The functions return `Result<T, E>` types:
    - `MMapReader::new()` returns `io::Result<MMapReader>`
    - `detect_format()` returns `io::Result<FileFormat>`
    - `parse_segments()` returns `Result<Vec<Segment>, ExifToolError>`
    - `parse_ifd()` returns `Result<Vec<(u16, Vec<u8>)>, ExifToolError>`

    Use `.expect()` or `.unwrap()` with descriptive messages in tests, or use the `?` operator with a test function that returns `Result<(), Box<dyn std::error::Error>>`.

*   **Critical Architecture Note:** After reading the JPEG segments and finding the APP1 segment with EXIF data, you need to:
    1. Extract the TIFF data starting at offset 6 within the APP1 segment data (skipping "Exif\0\0")
    2. Read the first 2 bytes to determine byte order
    3. Read bytes 4-7 (as u32 in the detected byte order) to get the IFD offset
    4. Create a new `TestReader` wrapping the TIFF data slice
    5. Call `parse_ifd()` with this reader, the IFD offset, and the detected byte order

*   **Example TestReader Pattern:** See `src/parsers/tiff/ifd_parser.rs:309-337` for the exact `TestReader` implementation pattern you should use.
