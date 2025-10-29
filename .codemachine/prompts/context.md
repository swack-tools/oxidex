# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I2.T3",
  "iteration_id": "I2",
  "iteration_goal": "Implement tag registry with subset of ExifTool tags, core metadata read/write operations, basic CLI with argument parsing, and extend format support to include XMP parsing and PNG format.",
  "description": "Implement read operations in src/core/operations.rs: read_metadata(path: &Path) -> Result<MetadataMap> that orchestrates: (1) open file with MMapReader, (2) detect format, (3) select appropriate parser, (4) parse to MetadataMap, (5) lookup tag descriptors and enrich metadata. Integrate format_detector, JPEG parser, TIFF/EXIF parser from I1. Add convenience methods to MetadataMap: get_string(tag_name), get_i64(tag_name), get_f64(tag_name), get_datetime(tag_name) with type coercion. Add unit and integration tests.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I1 parsers (format_detector, JPEG, TIFF), I2.T2 tag registry",
  "target_files": [
    "src/core/operations.rs",
    "src/core/metadata_map.rs",
    "src/core/mod.rs"
  ],
  "input_files": [
    "src/parsers/format_detector.rs",
    "src/parsers/jpeg/segment_parser.rs",
    "src/parsers/tiff/ifd_parser.rs",
    "src/tag_db/tag_registry.rs",
    "src/core/metadata_map.rs"
  ],
  "deliverables": "read_metadata() orchestration function, Typed getter methods on MetadataMap, Integration tests using test JPEG files",
  "acceptance_criteria": "read_metadata() successfully reads JPEG with EXIF, Getter methods return correct types (String, i64, f64, DateTime), Type coercion works (e.g., integer tag accessible via get_i64()), Returns Err for nonexistent tags, Integration test extracts at least 5 tags from sample image, cargo test operations passes",
  "dependencies": [
    "I1.T8",
    "I1.T9",
    "I1.T10",
    "I1.T11",
    "I2.T2"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: Functional Requirements - Metadata Reading (from 01_Context_and_Drivers.md)

```markdown
### 2.1. Functional Requirements Summary

The system must replicate ExifTool's core functionality:

1. **Metadata Reading**
   - Extract metadata from 300+ file formats
   - Support 28,000+ unique metadata tags across all families (EXIF, XMP, IPTC, etc.)
   - Handle maker-specific notes from camera manufacturers (Canon, Nikon, Sony, etc.)
   - Parse complex nested structures (XMP XML, QuickTime atoms, TIFF IFDs)
```

### Context: Hexagonal Architecture - Layered Structure (from 02_Architecture_Overview.md)

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

### Context: Core Library Components - Hexagonal Architecture (from 03_System_Structure_and_Data.md)

```markdown
### 3.5. Component Diagram(s) (C4 Level 3)

**Description**: This diagram details the internal components of the **Core Library** container, showing the hexagonal architecture layers and their interactions.

**Core Components**:

- **API Facade**: User-facing API: extract(), write(), copy_metadata()
- **Metadata Model**: TagValue, MetadataMap, TagDescriptor
- **Metadata Operations**: Read, Write, Copy, Transform operations
- **Tag Registry**: 28K+ tag definitions indexed by ID/name
- **Validation Engine**: Tag value type checking, range validation

**Ports (interfaces)**:
- **Format Parser Port**: `trait FormatParser { fn parse(&self, ...) -> Result<MetadataMap> }`
- **I/O Port**: `trait FileReader { fn read(&self, offset, len) -> Result<&[u8]> }`

**Infrastructure adapters**:
- JPEG Parser (nom-based)
- TIFF Parser (nom-based)
- XMP Parser (quick-xml)
- MMap Reader (memmap2)

**Interaction Flow**:
API Facade → Operations → Metadata Model
Operations → Tag Registry (lookup tag definitions)
Operations → Format Port → Parsers
Parsers → I/O Port → Readers
```

### Context: Data Model - Key Entities (from 03_System_Structure_and_Data.md)

```markdown
#### Key Entities

1. **File**: Represents a media file being processed (JPEG, PNG, etc.)
2. **MetadataMap**: Collection of all metadata tags extracted from a file
3. **TagValue**: A single metadata tag with its name, value, and type information
4. **TagDescriptor**: Definition of a tag (from tag database) including ID, name, type constraints, format family
5. **FormatFamily**: Grouping of related metadata standards (EXIF, XMP, IPTC, MakerNotes)
6. **IFD (Image File Directory)**: TIFF-specific structural element containing tags
```

### Context: API Design - Rust Library API Style (from 04_Behavior_and_Communication.md)

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

### Context: Metadata Extraction Workflow (from 04_Behavior_and_Communication.md)

```markdown
#### Key Interaction Flow (Sequence Diagram)

**Description**: This diagram illustrates the core workflow for **extracting metadata from a JPEG file**. It shows how the CLI delegates to the core library, which orchestrates format detection, parser selection, and metadata extraction through the hexagonal architecture layers.

**Workflow**:
1. User → CLI → Core Library: Metadata::from_path("photo.jpg")
2. Core → Format Detector: detect_format()
3. Detector → I/O Layer: read_magic_bytes()
4. Detector → Core: FileFormat::JPEG
5. Core → JPEG Parser: parse(io_handle)
6. JPEG → I/O: read_segment_markers()
7. JPEG → EXIF Parser: parse_exif_segment() [if APP1 EXIF found]
8. EXIF → EXIF: parse TIFF IFD structure
9. EXIF → Tag Registry: lookup_tag(0x010F) // Manufacturer tag
10. Tag Registry → EXIF: TagDescriptor { name: "EXIF:Make", type: String, ... }
11. EXIF → JPEG: Vec<TagValue> (EXIF tags)
12. JPEG → Core: MetadataMap with all tags
13. Core → CLI: Result<MetadataMap>
```

### Context: Task I2.T3 Specification (from 02_Iteration_I2.md)

```markdown
<!-- anchor: task-i2-t3 -->
*   **Task 2.3: Implement Metadata Read Operations**
    *   **Task ID:** `I2.T3`
    *   **Description:** Implement read operations in `src/core/operations.rs`: `read_metadata(path: &Path) -> Result<MetadataMap>` that orchestrates: (1) open file with MMapReader, (2) detect format, (3) select appropriate parser, (4) parse to MetadataMap, (5) lookup tag descriptors and enrich metadata. Integrate format_detector, JPEG parser, TIFF/EXIF parser from I1. Add convenience methods to MetadataMap: `get_string(tag_name)`, `get_i64(tag_name)`, `get_f64(tag_name)`, `get_datetime(tag_name)` with type coercion. Add unit and integration tests.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** I1 parsers (format_detector, JPEG, TIFF), I2.T2 tag registry
    *   **Input Files:** [`src/parsers/format_detector.rs`, `src/parsers/jpeg/segment_parser.rs`, `src/parsers/tiff/ifd_parser.rs`, `src/tag_db/tag_registry.rs`, `src/core/metadata_map.rs`]
    *   **Target Files:**
        *   `src/core/operations.rs`
        *   `src/core/metadata_map.rs` (add getter methods)
        *   `src/core/mod.rs` (export operations)
    *   **Deliverables:**
        *   read_metadata() orchestration function
        *   Typed getter methods on MetadataMap
        *   Integration tests using test JPEG files
    *   **Acceptance Criteria:**
        *   read_metadata() successfully reads JPEG with EXIF
        *   Getter methods return correct types (String, i64, f64, DateTime)
        *   Type coercion works (e.g., integer tag accessible via get_i64())
        *   Returns Err for nonexistent tags
        *   Integration test extracts at least 5 tags from sample image
        *   `cargo test operations` passes
    *   **Dependencies:** `I1.T8-T11`, `I2.T2`
    *   **Parallelizable:** No (depends on parsers and tag registry)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/metadata_map.rs` (310 lines, fully implemented)
    *   **Summary:** This file contains the complete MetadataMap implementation with basic CRUD operations (insert, get, remove, contains_key, iter). It already has typed getter methods: `get_string()`, `get_integer()`, and `get_float()` that delegate to TagValue's conversion methods. The struct is fully serializable with serde and has comprehensive unit tests (18 test functions).
    *   **Recommendation:** The MetadataMap is already well-implemented with the required getter methods. You DO NOT need to add `get_i64()` or `get_f64()` methods - they already exist as `get_integer()` and `get_float()`. You MAY need to add a `get_datetime()` method if the task requires it, but check the TagValue enum first to see if DateTime support exists. Simply USE the existing MetadataMap API as-is.

*   **File:** `src/parsers/format_detector.rs` (349 lines, fully implemented)
    *   **Summary:** This file provides a complete format detection implementation using magic bytes. The function `detect_format(reader: &dyn FileReader) -> io::Result<FileFormat>` identifies JPEG, TIFF (both endianness), PNG, and PDF formats. It includes extensive unit tests (18 test functions) with TestReader implementation for validation.
    *   **Recommendation:** You MUST import and use the `detect_format()` function from this module. It is production-ready and well-tested. DO NOT reimplement format detection. Import it with: `use crate::parsers::format_detector::detect_format;`

*   **File:** `src/parsers/jpeg/segment_parser.rs` (567 lines, fully implemented)
    *   **Summary:** This file contains a complete JPEG segment parser using nom combinators. The `parse_segments()` function reads JPEG structure and returns a `Vec<Segment<'a>>`. Each Segment has a marker (u16), offset (u64), and borrowed data slice (&'a [u8]). The implementation handles SOI (0xFFD8), APP1 (0xFFE1), EOI (0xFFD9) markers and all standard segments with extensive error handling. Includes 16 unit tests.
    *   **Recommendation:** You MUST import and use `parse_segments()` to parse JPEG files. Look for APP1 segments (marker 0xFFE1) which contain EXIF data. The segment data will need to be passed to the TIFF IFD parser to extract EXIF tags. Import with: `use crate::parsers::jpeg::segment_parser::{parse_segments, Segment};`

*   **File:** `src/parsers/tiff/ifd_parser.rs` (200+ lines viewed, fully functional)
    *   **Summary:** This file implements TIFF IFD (Image File Directory) parsing with `parse_ifd(reader: &dyn FileReader, ifd_offset: u64, byte_order: ByteOrder) -> Result<Vec<(u16, Vec<u8>)>>`. It returns raw tag ID (u16) and value byte pairs (Vec<u8>). The parser handles both little-endian and big-endian formats with proper validation. Exports ByteOrder enum with LittleEndian and BigEndian variants.
    *   **Recommendation:** You MUST use `parse_ifd()` to parse EXIF data from JPEG APP1 segments. EXIF in JPEG has format: "Exif\0\0" (6 bytes) + TIFF header + IFD data. You need to: (1) skip the 6-byte EXIF header, (2) detect byte order from TIFF header bytes 6-7 (0x4949="II" or 0x4D4D="MM"), (3) read IFD offset from bytes 10-13, (4) call parse_ifd() with offset relative to byte 6 (TIFF data start).

*   **File:** `src/tag_db/tag_registry.rs` (150 lines viewed, functional with 100 tags)
    *   **Summary:** This file provides a lazy-initialized static registry with 100 common tags using the `once_cell` crate. The registry is a `HashMap<&'static str, TagDescriptor>` with tags like "EXIF:Make" (0x010F), "EXIF:Model" (0x0110), etc. It includes public function `get_tag_descriptor(name: &str) -> Option<&TagDescriptor>` for lookups.
    *   **Recommendation:** You SHOULD import and use the tag registry to enrich parsed metadata. After extracting raw tag IDs and values, lookup the TagDescriptor to get the human-readable tag name (e.g., tag 0x010F → "EXIF:Make"). However, note that the current registry is minimal (100 tags) - you MUST handle unknown tags gracefully by creating a fallback tag name like "EXIF:0x010F" format.

*   **File:** `src/io/mmap_reader.rs` (100 lines viewed, fully functional)
    *   **Summary:** This file implements MMapReader using the memmap2 crate for zero-copy file access. It provides memory-mapped file reading implementing the FileReader trait with read() and size() methods. Includes safety documentation about mapping lifetime.
    *   **Recommendation:** You MUST use MMapReader to open files in read_metadata(). Create it with `MMapReader::new(path)?` and it will implement the FileReader trait needed by all parsers. Import with: `use crate::io::MMapReader;`

*   **File:** `src/core/operations.rs` (6 lines, EMPTY)
    *   **Summary:** This file is EMPTY except for module-level comments and #[allow(dead_code)]. This is your primary implementation target.
    *   **Recommendation:** You will implement the complete read_metadata() orchestration function here from scratch. Remove the #[allow(dead_code)] directive once you add functions.

*   **File:** `src/error/mod.rs` (264 lines, fully implemented)
    *   **Summary:** Complete ExifToolError enum with variants: IoError, ParseError (with optional offset), TagNotFound, InvalidTagValue, UnsupportedFormat. Includes helper constructors (parse_error, tag_not_found, unsupported_format, etc.), Display implementation, Error trait implementation, and conversion from io::Error. Also defines Result<T> type alias for std::result::Result<T, ExifToolError>.
    *   **Recommendation:** You MUST use ExifToolError and the Result type alias throughout operations.rs. Use ExifToolError::unsupported_format() for unknown formats, and the From<io::Error> implementation to convert io errors from format_detector. Import with: `use crate::error::{ExifToolError, Result};`

*   **File:** `src/core/tag_value.rs` (not viewed but referenced)
    *   **Summary:** Likely contains TagValue enum with variants for different value types (String, Integer, Float, etc.) and conversion methods (as_string(), as_integer(), as_float()).
    *   **Recommendation:** You will need to construct TagValue instances from raw bytes parsed from TIFF IFD. Check this file to understand how to create TagValue::new_string(), TagValue::new_integer(), etc.

### Implementation Tips & Notes

*   **Tip 1 - EXIF Structure in JPEG:** The EXIF data in JPEG APP1 segments has this exact structure:
    ```
    Bytes 0-1:   0xFF 0xE1         (APP1 marker)
    Bytes 2-3:   Length            (big-endian u16, includes length field but not marker)
    Bytes 4-9:   "Exif\0\0"        (6-byte EXIF header: 0x45 0x78 0x69 0x66 0x00 0x00)
    Bytes 10+:   TIFF data starts here
      Bytes 10-11: Byte order (0x4949 "II" = little-endian, 0x4D4D "MM" = big-endian)
      Bytes 12-13: Magic number 42 (0x002A in detected byte order)
      Bytes 14-17: IFD offset (4 bytes in detected byte order, relative to byte 10)
      At IFD offset: IFD structure begins
    ```
    You need to: (1) Check segment data starts with "Exif\0\0", (2) Skip to byte 6 (TIFF start), (3) Read bytes 6-7 for byte order, (4) Read bytes 10-13 for IFD offset, (5) Call parse_ifd() with offset+6 (absolute) or offset (relative to TIFF start depending on parser implementation).

*   **Tip 2 - Orchestration Flow:** For the read_metadata() implementation, follow this exact sequence to match the architecture:
    ```rust
    pub fn read_metadata(path: &Path) -> Result<MetadataMap> {
        // 1. Open file with MMapReader
        let reader = MMapReader::new(path)?;

        // 2. Detect format
        let format = detect_format(&reader)
            .map_err(|e| ExifToolError::from(e))?;

        // 3. Route to appropriate parser based on format
        match format {
            FileFormat::JPEG => parse_jpeg_metadata(&reader),
            FileFormat::TIFF => parse_tiff_metadata(&reader),
            _ => Err(ExifToolError::unsupported_format(
                format!("Format {:?} not yet supported", format)
            )),
        }
    }
    ```

*   **Tip 3 - Tag ID to Name Mapping:** You need to create a helper function to convert numeric tag IDs to tag names. The tag_registry only allows lookup by name, so you need a reverse mapping:
    ```rust
    fn tag_id_to_name(tag_id: u16, family: &str) -> String {
        // Try to find TagDescriptor with matching numeric ID
        // For now, you might need to iterate the registry or create a reverse map
        // Fallback: format!("{}:0x{:04X}", family, tag_id)
        format!("EXIF:0x{:04X}", tag_id)  // Simple fallback
    }
    ```
    Better approach: Create a static reverse mapping HashMap<u16, &'static str> in tag_registry.rs for EXIF tags only.

*   **Tip 4 - Value Conversion:** Raw IFD values are Vec<u8>. You need to convert based on EXIF type:
    - Type 2 (ASCII): String::from_utf8_lossy(&bytes).trim_end_matches('\0')
    - Type 3 (SHORT): u16::from_le_bytes() or from_be_bytes() depending on byte_order
    - Type 4 (LONG): u32::from_le_bytes() or from_be_bytes()
    - Type 5 (RATIONAL): Two u32 values (numerator/denominator)
    Wrap in appropriate TagValue variant: TagValue::new_string(), TagValue::new_integer(), etc.

*   **Note 1 - MetadataMap Methods:** The task description says to add get_i64(), get_f64(), get_datetime() methods to MetadataMap. However:
    - get_integer() already exists (returns Option<i64>)
    - get_float() already exists (returns Option<f64>)
    - You may only need to add get_i64 and get_f64 as ALIASES pointing to existing methods, OR just document that they exist with different names. Check the task's strict interpretation.
    - get_datetime() likely doesn't exist yet - you'll need to add it if TagValue has a DateTime variant.

*   **Note 2 - Integration Test Location:** Based on standard Rust conventions and the task description mentioning I1.T14 test location, your integration test should go in:
    - `tests/integration/operations_tests.rs` OR
    - Extend existing `tests/integration/jpeg_tests.rs`
    The test should use the fixture at `tests/fixtures/jpeg/sample_with_exif.jpg`.

*   **Note 3 - Test Requirements:** The acceptance criteria requires extracting "at least 5 tags from sample image". Common EXIF tags to verify:
    - EXIF:Make (0x010F)
    - EXIF:Model (0x0110)
    - EXIF:DateTime (0x0132)
    - EXIF:Software (0x0131)
    - EXIF:Orientation (0x0112)

*   **Warning 1 - Lifetime Management:** The Segment struct from JPEG parser has lifetime 'a tied to the reader. When building MetadataMap, you need to COPY data into owned Strings/Vec<u8>, not borrow. Example:
    ```rust
    let segments = parse_segments(&reader)?;
    for segment in segments.iter() {
        if segment.is_app1() {
            // segment.data is &[u8] borrowed from reader
            // Must copy: let owned_data = segment.data.to_vec();
        }
    }
    ```

*   **Warning 2 - Error Conversion:** format_detector returns io::Result, but operations.rs uses ExifToolError Result. Convert with:
    ```rust
    let format = detect_format(&reader)
        .map_err(|e| ExifToolError::from(e))?;
    // OR use ? and rely on From<io::Error> implementation
    let format = detect_format(&reader)?;  // Works because of From impl
    ```

*   **Important - Test Fixture:** A test fixture already exists at `tests/fixtures/jpeg/sample_with_exif.jpg`. Use this in your integration test with:
    ```rust
    #[test]
    fn test_read_jpeg_with_exif() {
        let path = Path::new("tests/fixtures/jpeg/sample_with_exif.jpg");
        let metadata = read_metadata(path).expect("Failed to read metadata");
        assert!(metadata.len() >= 5);
        // Verify specific tags exist
        assert!(metadata.get_string("EXIF:Make").is_some());
    }
    ```

*   **Best Practice - Module Organization:** Keep operations.rs clean with clear separation:
    1. Public API function: `pub fn read_metadata(path: &Path) -> Result<MetadataMap>`
    2. Format-specific helpers: `fn parse_jpeg_metadata(reader: &dyn FileReader) -> Result<MetadataMap>`
    3. Utility functions: `fn bytes_to_tag_value(bytes: &[u8], exif_type: u16, byte_order: ByteOrder) -> TagValue`
    4. Make helpers private (no pub) - only export read_metadata from mod.rs

*   **Best Practice - Hexagonal Architecture:** Follow the architectural pattern strictly. Keep operations.rs in the domain layer - it should only orchestrate and delegate to parsers. Do NOT put parsing logic directly in operations.rs. All parsing happens via the existing parser functions (parse_segments, parse_ifd, etc.). Your code is the "orchestrator" that connects infrastructure adapters to domain models.
