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

### Context: key-interaction-flow (from 04_Behavior_and_Communication.md)

```markdown
#### Key Interaction Flow (Sequence Diagram)

**Description**: This diagram illustrates the core workflow for **extracting metadata from a JPEG file**. It shows how the CLI delegates to the core library, which orchestrates format detection, parser selection, and metadata extraction through the hexagonal architecture layers.

**Workflow Breakdown**:

1. **Format Detection**: Read file magic bytes (first 16 bytes) to identify format (JPEG: `0xFF 0xD8`)
2. **Parser Selection**: Based on format, select appropriate parser implementation (JPEG parser in this case)
3. **Segment Parsing**: JPEG parser reads segment markers (0xFFE0-0xFFEF) to locate metadata containers
4. **Metadata Extraction**:
   - EXIF segment contains TIFF-encoded metadata, parsed via EXIF/TIFF parser
   - XMP segment contains RDF/XML, parsed via XMP parser
5. **Tag Resolution**: Each raw tag ID (e.g., TIFF tag 0x010F) is looked up in Tag Registry to get semantic name ("EXIF:Make")
6. **Validation**: Tag values validated against expected types (e.g., "EXIF:Make" must be string, "EXIF:ISOSpeedRatings" must be integer)
7. **Output**: Metadata returned to CLI, formatted per user request (human-readable, JSON, CSV, etc.)
```

### Context: task-i1-t14 (from 02_Iteration_I1.md)

```markdown
*   **Task 1.14: Implement End-to-End Test (JPEG EXIF Extraction)**
    *   **Task ID:** `I1.T14`
    *   **Description:** Create integration test in `tests/integration/jpeg_tests.rs` that demonstrates end-to-end workflow: (1) Use MMapReader to open sample JPEG file (create sample with EXIF in tests/fixtures/jpeg/), (2) Detect format using format_detector, (3) Parse JPEG segments, (4) Parse EXIF IFD from APP1 segment, (5) Extract at least 3 tag values (Make, Model, DateTime), (6) Print extracted values. This test validates the entire parsing pipeline from I1.T8-T11. Test should pass.
    *   **Acceptance Criteria:**
        *   Test successfully opens JPEG file
        *   Format detector identifies file as JPEG
        *   Segment parser finds APP1 segment
        *   IFD parser extracts Make, Model, DateTime tags
        *   Test assertions verify tag values are non-empty strings
        *   `cargo test jpeg_tests` passes
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

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/format_detector.rs`
    *   **Summary:** This file contains the `detect_format()` function that reads the first 16 bytes of a file and identifies the format by magic bytes. It returns a `FileFormat` enum.
    *   **Recommendation:** You MUST use this function in your integration test to detect that the JPEG file is indeed a JPEG format. Import it: `use exiftool_rs::parsers::format_detector::detect_format;`
    *   **Key Function Signature:** `pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat>`
    *   **Expected Output:** Returns `Ok(FileFormat::JPEG)` for JPEG files that start with `0xFF 0xD8 0xFF`

*   **File:** `src/parsers/jpeg/segment_parser.rs`
    *   **Summary:** This file contains the JPEG segment parser using nom combinators. The key function is `parse_segments()` which returns a `Vec<Segment>`.
    *   **Recommendation:** You MUST use this function to parse the JPEG file and find APP1 segments containing EXIF data. The function is: `pub fn parse_segments<'a>(reader: &'a dyn FileReader) -> Result<Vec<Segment<'a>>, ExifToolError>`
    *   **Key Struct:** `Segment` has fields: `marker` (u16), `offset` (u64), `data` (&[u8])
    *   **Key Constants:** `APP1_MARKER = 0xFFE1` - This is what you need to filter for EXIF segments
    *   **Helper Method:** `Segment::is_app1()` returns true if the segment is an APP1 segment (0xFFE1)

*   **File:** `src/parsers/tiff/ifd_parser.rs`
    *   **Summary:** This file contains the TIFF IFD parser that extracts tag values from EXIF data. The key function is `parse_ifd()`.
    *   **Recommendation:** You MUST use this function to parse the EXIF IFD structure from the APP1 segment data. The function signature is: `pub fn parse_ifd(reader: &dyn FileReader, ifd_offset: u64, byte_order: ByteOrder) -> Result<Vec<(u16, Vec<u8>)>>`
    *   **Important:** EXIF data in JPEG APP1 segments has a 6-byte header "Exif\0\0" followed by the TIFF header. You need to skip this 6-byte prefix before passing to the IFD parser.
    *   **Key Constants for Tags:**
        - Make tag: `0x010F`
        - Model tag: `0x0110`
        - DateTime tag: `0x0132`
    *   **Byte Order Detection:** The TIFF header starts with either "II" (0x4949, little-endian) or "MM" (0x4D4D, big-endian). You must detect this and pass the correct `ByteOrder` enum value.

*   **File:** `src/io/mmap_reader.rs`
    *   **Summary:** This file contains the `MMapReader` struct that implements the `FileReader` trait using memory-mapped I/O.
    *   **Recommendation:** You MUST use `MMapReader::new(path)` to open the test JPEG file. This is the most efficient way to read files.
    *   **Constructor Signature:** `pub fn new(path: &Path) -> io::Result<Self>`
    *   **Note:** The MMapReader implements the FileReader trait, which provides `read(offset, length)` and `size()` methods.

*   **File:** `src/core/file_format.rs`
    *   **Summary:** Defines the `FileFormat` enum with variants like JPEG, TIFF, PNG, PDF, Unknown.
    *   **Usage:** You will compare the detected format against `FileFormat::JPEG` in your test assertions.

*   **File:** `src/error.rs`
    *   **Summary:** Defines the `ExifToolError` enum for error handling.
    *   **Recommendation:** The parsing functions return `Result<T, ExifToolError>`. Use `.expect()` or `.unwrap()` in tests for simplicity, or proper error handling if needed.

### Implementation Tips & Notes

*   **Tip: EXIF Data Structure in JPEG APP1 Segments**
    - APP1 segments with EXIF have this structure:
        1. APP1 marker: 2 bytes (0xFFE1)
        2. Segment length: 2 bytes (big-endian)
        3. EXIF identifier: 6 bytes ("Exif\0\0" = `[0x45, 0x78, 0x69, 0x66, 0x00, 0x00]`)
        4. TIFF header: starts at offset 6 in the segment data
            - Byte order: 2 bytes ("II" or "MM")
            - Magic number: 2 bytes (0x002A for LE or 0x2A00 for BE)
            - IFD offset: 4 bytes (offset to first IFD, usually 0x00000008 which means 8 bytes from TIFF header start)
        5. IFD data: follows the TIFF header

*   **Tip: Creating Test JPEG with EXIF**
    - You SHOULD create a valid JPEG file with embedded EXIF data. The simplest approach is to create a minimal JPEG programmatically in your test with synthetic EXIF data.
    - Structure: SOI (0xFFD8) + APP1 (with EXIF) + minimal image data + EOI (0xFFD9)
    - You can use the test helper patterns from `segment_parser.rs` tests as inspiration.

*   **Tip: Parsing Workflow**
    1. Open file with `MMapReader::new()`
    2. Detect format with `detect_format(reader)`
    3. Parse segments with `parse_segments(reader)`
    4. Filter for APP1 segments: `segments.iter().filter(|s| s.is_app1())`
    5. Check EXIF identifier: ensure segment.data starts with "Exif\0\0"
    6. Create a sub-reader or slice from offset 6 onwards (after "Exif\0\0")
    7. Detect byte order from TIFF header (first 2 bytes after EXIF identifier)
    8. Parse IFD with `parse_ifd(sub_reader, ifd_offset, byte_order)`
    9. Extract tags 0x010F (Make), 0x0110 (Model), 0x0132 (DateTime)
    10. Convert raw bytes to strings (these tags are ASCII type, so UTF-8 decode)

*   **Note: Test Infrastructure**
    - The directory `tests/integration/` already exists but is empty.
    - The directory `tests/fixtures/jpeg/` already exists but is empty.
    - You MUST create both `tests/integration/jpeg_tests.rs` and a sample JPEG file in `tests/fixtures/jpeg/sample_with_exif.jpg`.

*   **Note: Tag Value Decoding**
    - The IFD parser returns `Vec<(u16, Vec<u8>)>` - tag ID and raw bytes.
    - For ASCII tags (Make, Model, DateTime), you can decode with `String::from_utf8_lossy(&bytes)` or `std::str::from_utf8(&bytes)`.
    - Remember that ASCII strings in EXIF are null-terminated, so you may want to trim the trailing null byte.

*   **Warning: TIFF Header Offset**
    - When calling `parse_ifd()`, the `ifd_offset` parameter is relative to the START of the TIFF header, NOT the start of the segment data.
    - You need to create a FileReader that treats the TIFF header start as offset 0. One approach is to wrap the segment data in a temporary reader struct.

*   **Recommendation: Use Existing Test Patterns**
    - Both `segment_parser.rs` and `ifd_parser.rs` have comprehensive unit tests with `TestReader` implementations.
    - You can reference these patterns for creating test data and assertions.
    - The tests show how to create synthetic binary data for JPEG segments and TIFF IFDs.

*   **Important: Acceptance Criteria**
    - The test MUST successfully extract Make, Model, and DateTime tags.
    - The test MUST verify that these tag values are non-empty strings.
    - The test MUST pass when running `cargo test jpeg_tests`.
    - You should add `println!()` statements to print the extracted values as requested in the task description.
