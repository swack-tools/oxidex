# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I3.T2",
  "iteration_id": "I3",
  "iteration_goal": "Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.",
  "description": "Implement TIFF IFD serializer in src/writers/tiff_writer.rs. Create function to serialize MetadataMap EXIF tags back to TIFF IFD structure: (1) Filter tags for EXIF family, (2) Convert TagValue to TIFF data types (Byte, ASCII, Short, Long, Rational), (3) Build IFD entries (tag ID, type, count, value/offset), (4) Handle values >4 bytes (write to separate value area), (5) Calculate offsets, (6) Write IFD header + entries + values. Support both little-endian and big-endian output. Add unit tests verifying round-trip (parse then serialize equals original).",
  "agent_type_hint": "BackendAgent",
  "inputs": "TIFF specification, I1.T11 TIFF parser (for understanding structure)",
  "target_files": [
    "src/writers/tiff_writer.rs",
    "src/writers/mod.rs"
  ],
  "input_files": [
    "src/parsers/tiff/ifd_parser.rs",
    "src/core/metadata_map.rs"
  ],
  "deliverables": "TIFF IFD serialization function, support for both endianness, unit and round-trip tests",
  "acceptance_criteria": "Serializer produces valid TIFF IFD structure, handles both little-endian and big-endian, correctly writes tag entries with type, count, value, values >4 bytes written to separate area with offset, round-trip test: parse(serialize(metadata)) == metadata for EXIF tags, cargo test tiff_writer passes",
  "dependencies": [
    "I1.T11",
    "I2.T2"
  ],
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

**Key Libraries Detail**:

- **`nom` v7**: Parser combinator library for binary formats. Example: TIFF IFD parsing uses `nom::number::complete::le_u16` for little-endian u16, chained with `nom::multi::count` for tag array parsing.

- **`serde`**: Serialization framework. Domain metadata models derive `Serialize`/`Deserialize` for JSON/CSV output.
```

### Context: task-i3-t2 (from 02_Iteration_I3.md)

```markdown
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

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/tiff/ifd_parser.rs`
    *   **Summary:** This file contains the complete TIFF IFD **parsing** implementation. It uses nom parser combinators to parse both little-endian and big-endian IFD structures. The parser extracts tag entries from IFDs and returns `Vec<(u16, Vec<u8>)>` pairs (tag_id, raw_value).
    *   **Recommendation:** You MUST study this file carefully as it shows the **exact inverse operation** you need to implement. Key insights:
        - IFD structure: 2-byte entry count + (12-byte entries × count) + 4-byte next IFD offset
        - Each 12-byte entry: tag_id (u16) + field_type (u16) + value_count (u32) + value_offset (u32)
        - Inline value rule: if `type_size × count ≤ 4 bytes`, value stored directly in value_offset field
        - Otherwise: value_offset contains absolute file offset to value data
        - The functions `parse_ifd_entry_le()` and `parse_ifd_entry_be()` show the exact byte layout you need to write
    *   **Key Structures:**
        - `ByteOrder` enum (LittleEndian, BigEndian) - YOU MUST reuse this from the parser module
        - `IfdEntry` struct with fields: tag_id, field_type, value_count, value_offset - useful reference
        - `extract_inline_value()` function shows how inline values are packed (lines 239-253)

*   **File:** `src/parsers/common/exif_types.rs`
    *   **Summary:** This file defines the `ExifType` enum with all 12 TIFF data types (Byte=1, Ascii=2, Short=3, Long=4, Rational=5, etc.) and provides methods for type size calculations and conversions.
    *   **Recommendation:** You MUST import and use `ExifType` from this module. The `size_in_bytes()` method is critical for calculating value sizes to determine inline vs. offset storage. Use `as_u16()` when writing type codes to IFD entries.
    *   **Critical Methods:**
        - `ExifType::size_in_bytes()` - returns 1 for Byte/ASCII, 2 for Short, 4 for Long, 8 for Rational, etc.
        - `ExifType::as_u16()` - converts enum to type code for IFD entry (e.g., Ascii becomes 2)
        - `ExifType::from_u16()` - useful for validation in tests

*   **File:** `src/core/metadata_map.rs`
    *   **Summary:** This file defines the `MetadataMap` struct that stores `HashMap<String, TagValue>`. It provides typed getters like `get_string()`, `get_integer()`, `get_float()`, and an `iter()` method that returns `Iterator<Item = (&String, &TagValue)>`.
    *   **Recommendation:** You MUST iterate over the MetadataMap using `.iter()` to extract EXIF tags for serialization. Filter for tags starting with "EXIF:" prefix (e.g., "EXIF:Make", "EXIF:Model", "EXIF:DateTime"). The `.iter()` method provides access to both tag names and their TagValue enums.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** This file defines the `TagValue` enum with variants: String, Integer, Float, Rational{numerator, denominator}, Binary, DateTime, Struct. Each variant has constructors (`new_string()`, etc.) and type-checking methods (`is_string()`, `as_string()`, etc.).
    *   **Recommendation:** You MUST match on TagValue variants to convert to appropriate TIFF types:
        - `TagValue::String(s)` → `ExifType::Ascii` (null-terminated bytes)
        - `TagValue::Integer(i)` → `ExifType::Long` or `ExifType::Short` (depending on range)
        - `TagValue::Rational{numerator, denominator}` → `ExifType::Rational` (8 bytes: two u32s)
        - `TagValue::Binary(bytes)` → `ExifType::Undefined`
        - For now, SKIP `TagValue::Float`, `DateTime` and `Struct` variants in your implementation (add TODO comments for future work)

*   **File:** `src/tag_db/tag_registry.rs`
    *   **Summary:** This file contains the static tag registry with 100+ tags including EXIF tags like "EXIF:Make" (0x010F), "EXIF:Model" (0x0110), etc. Each TagDescriptor has a numeric tag ID accessible via the registry lookup. The module uses lazy_static initialization.
    *   **Recommendation:** You SHOULD attempt to look up the numeric tag ID from the tag name string. The registry has a function `get_tag_descriptor(name: &str)` that returns `Option<&TagDescriptor>`. For example, "EXIF:Make" maps to tag_id 0x010F. If a tag is not in the registry, you could either skip it or assign a placeholder tag ID (document this behavior).

*   **File:** `src/writers/tiff_writer.rs`
    *   **Summary:** This file currently exists but is nearly empty (only has a comment header and `#![allow(dead_code)]` directive at line 5).
    *   **Recommendation:** You MUST implement the complete serialization logic in this file. Start by defining helper functions for byte serialization, then build up to the main IFD serialization function. Follow the same module structure pattern as ifd_parser.rs with comprehensive documentation and tests.

### Implementation Tips & Notes

*   **Tip:** The TIFF IFD structure has a specific binary layout that MUST be followed exactly:
    1. Entry count (2 bytes) - number of tag entries
    2. All IFD entries (12 bytes each, sorted by tag ID in ascending order - THIS IS IMPORTANT)
    3. Next IFD offset (4 bytes, use 0 for single IFD / last IFD in chain)
    4. Value data area (for values >4 bytes, written sequentially)

*   **Note:** The offset calculation is CRITICAL. The value_offset field in IFD entries must contain the **absolute offset** from the start of the TIFF data (not from IFD start). Calculate offsets like this:
    - If serializing standalone IFD at offset 0: IFD starts at 0
    - Value data area starts at: `ifd_start + 2 + (entry_count × 12) + 4`
    - Each large value gets sequential offsets in this area: first at value_area_start, second at value_area_start + first_value_size, etc.

*   **Warning:** Be VERY careful with byte order! You MUST write multi-byte values (u16, u32) in the specified endianness:
    - Little-endian: use `.to_le_bytes()` on all u16 and u32 values
    - Big-endian: use `.to_be_bytes()` on all u16 and u32 values
    - The byte order applies to ALL multi-byte values: entry count, tag IDs, type codes, counts, offsets, AND the values themselves

*   **Tip:** For inline values (total size ≤4 bytes), pack them **left-justified** in the 4-byte value_offset field:
    - For BOTH endianness: bytes go in positions [0..size], remaining bytes are 0x00
    - Example: 3-byte ASCII "EOS\0" in little-endian becomes [0x45, 0x4F, 0x53, 0x00] in value_offset field
    - See `extract_inline_value()` in ifd_parser.rs (lines 239-253) for the reverse operation - your packing should be the exact inverse

*   **Note:** ASCII strings in TIFF MUST be null-terminated. When converting `TagValue::String(s)` to bytes, append a null byte: `format!("{}\0", s).into_bytes()` or `s.as_bytes()` followed by pushing 0x00. The count field should include the null terminator.

*   **Tip:** For Rational types, the value is 8 bytes: first u32 is numerator, second u32 is denominator. Both must be written in the specified byte order. For example, in little-endian: numerator.to_le_bytes() followed by denominator.to_le_bytes().

*   **Critical:** The IFD entries MUST be sorted by tag ID in ascending order. This is required by the TIFF specification. After collecting all entries, sort them by tag_id before writing.

*   **Critical:** The type and count fields MUST match the data. Examples:
    - ASCII string "Canon\0" (6 bytes) → type=Ascii(2), count=6, value/offset contains the bytes
    - Single u32 value 12345 → type=Long(4), count=1, inline in value_offset
    - Rational 1/100 → type=Rational(5), count=1, 8 bytes in value area

*   **Testing Strategy:** For round-trip tests (this is CRITICAL for acceptance criteria):
    1. Create a MetadataMap with known EXIF tags (e.g., "EXIF:Make", "EXIF:Model", "EXIF:ISO")
    2. Serialize it to `Vec<u8>` using your new writer
    3. Parse those bytes back using `parse_ifd()` from ifd_parser.rs
    4. Compare the parsed (tag_id, raw_bytes) pairs with expected values
    5. Test with BOTH little-endian and big-endian
    6. Include tests for inline values (≤4 bytes) and offset values (>4 bytes)

*   **Note:** You'll need to handle the mapping from tag names to tag IDs. You can either:
    - Use the tag registry to look up IDs (preferred)
    - Parse the tag ID from the tag name if it's in numeric format
    - Skip tags that can't be mapped to IDs (document this limitation)

*   **Project Convention:** All public functions should have comprehensive doc comments with `///` including:
    - Brief summary line
    - Detailed description of behavior
    - Parameters section
    - Returns section
    - Errors section (if returning Result)
    - Examples section with runnable code
    - See ifd_parser.rs lines 89-135 for excellent documentation examples to match

*   **Note:** Use `#[cfg(test)]` module at the bottom of the file for unit tests, similar to the extensive test suite in ifd_parser.rs (lines 303-698). Aim for similar test coverage with tests for: successful serialization, both endianness, inline vs. offset values, empty IFD, round-trip verification, etc.

*   **Implementation Hint:** Consider this function signature as a starting point:
    ```rust
    /// Serializes EXIF tags from MetadataMap to TIFF IFD bytes
    pub fn serialize_ifd(
        metadata: &MetadataMap,
        byte_order: ByteOrder,
        ifd_start_offset: u64,
    ) -> Result<Vec<u8>>
    ```

*   **Type Conversion Strategy:** For TagValue to TIFF type mapping:
    - `String` → `Ascii` (type 2)
    - `Integer` → Check value range: if fits in u16 use `Short` (type 3), else `Long` (type 4)
    - `Rational{num, denom}` → `Rational` (type 5) - 8 bytes: u32 numerator + u32 denominator
    - `Binary` → `Undefined` (type 7)
    - `Float` - skip for now (add TODO comment)
    - `DateTime` - skip for now (could be converted to ASCII datetime string in future)
    - `Struct` - skip for now (not applicable to simple EXIF tags)

*   **Error Handling:** Import and use `crate::error::{ExifToolError, Result}`. Return errors for:
    - Tag name that can't be mapped to tag ID
    - Unsupported TagValue variant
    - Value that can't be represented in TIFF format
    - Serialization failures (though most are infallible)
