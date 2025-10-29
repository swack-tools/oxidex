# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I3.T3",
  "iteration_id": "I3",
  "iteration_goal": "Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.",
  "description": "Implement JPEG EXIF writer in src/writers/jpeg_writer.rs. Create function to write modified EXIF back to JPEG: (1) Read original JPEG using segment parser, (2) Serialize modified EXIF tags using TIFF writer (I3.T2) with EXIF header (Exif\\0\\0 + TIFF IFD), (3) Create new APP1 segment with EXIF marker (0xFFE1), length, and data, (4) Replace old EXIF APP1 segment with new one in JPEG structure, (5) Write modified JPEG to buffer. Handle segment size changes (if new EXIF larger/smaller than original). Add integration test modifying EXIF tag in JPEG and verifying change.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I1.T10 JPEG parser, I3.T2 TIFF writer",
  "target_files": [
    "src/writers/jpeg_writer.rs",
    "src/writers/mod.rs",
    "tests/integration/jpeg_write_tests.rs"
  ],
  "input_files": [
    "src/parsers/jpeg/segment_parser.rs",
    "src/writers/tiff_writer.rs"
  ],
  "deliverables": "JPEG EXIF segment writer, integration test for EXIF modification",
  "acceptance_criteria": "Writer replaces EXIF APP1 segment with modified data, handles EXIF header (Exif\\0\\0) correctly, handles segment size changes (larger/smaller new EXIF), preserves other JPEG segments (XMP, IPTC, image data), integration test: modify EXIF:Artist, re-read, verify new value, cargo test jpeg_write_tests passes",
  "dependencies": [
    "I1.T10",
    "I3.T2"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: JPEG EXIF Writing Workflow

**JPEG File Structure Requirements:**

JPEG files consist of a sequence of segments with the following structure:
- **SOI marker**: 0xFFD8 (Start of Image) - 2 bytes, no length field
- **Segment sequence**: Each segment has:
  - **Marker**: 2 bytes (0xFFXX)
  - **Length**: 2 bytes (big-endian), includes length field but NOT marker
  - **Data**: Variable-length payload (length - 2 bytes)
- **EOI marker**: 0xFFD9 (End of Image) - 2 bytes, no length field

**EXIF in JPEG:**
- EXIF metadata is stored in an APP1 segment (marker 0xFFE1)
- EXIF APP1 segment structure:
  1. Marker: 0xFFE1 (2 bytes)
  2. Length: 2 bytes (big-endian, includes itself + header + TIFF data, but NOT marker)
  3. EXIF identifier: "Exif\0\0" (6 bytes)
  4. TIFF IFD data: Complete TIFF structure with header and IFD

**Critical Implementation Requirements:**

1. **EXIF Header**: The EXIF APP1 segment MUST begin with the 6-byte identifier "Exif\0\0" (0x45 0x78 0x69 0x66 0x00 0x00)

2. **Segment Length Calculation**:
   - Length field = 2 (length bytes) + 6 (EXIF identifier) + TIFF data size
   - Total segment size = 2 (marker) + 2 (length) + 6 (EXIF identifier) + TIFF data size

3. **Segment Preservation**: When replacing EXIF:
   - MUST preserve all non-EXIF segments (XMP, IPTC, other APPx, image data)
   - MUST preserve segment order (typically: SOI → APP0/JFIF → APP1/EXIF → APP1/XMP → SOS → image data → EOI)
   - Handle cases where EXIF segment doesn't exist (create new APP1 segment)
   - Handle cases where multiple APP1 segments exist (only replace EXIF, preserve XMP)

4. **Size Changes**: The new EXIF segment may be larger or smaller than the original:
   - Larger: Insert expanded segment, shift remaining segments
   - Smaller: Insert smaller segment, shift remaining segments
   - The JPEG structure must remain valid after modification

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/jpeg/segment_parser.rs`
    *   **Summary:** Provides comprehensive JPEG segment parsing using nom combinators. The `parse_segments()` function reads an entire JPEG file and returns a `Vec<Segment>` where each segment contains marker, offset, and data as a borrowed slice. The module handles SOI marker validation, segment iteration, and EOI detection.
    *   **Recommendation:** You MUST use the `parse_segments()` function to read the original JPEG structure. The `Segment` struct provides `is_app1()`, `is_soi()`, and `is_eoi()` helper methods. Each segment's `data` field contains the payload (excludes marker and length).
    *   **Critical Detail:** For APP1 segments, the `data` field includes the identifier (e.g., "Exif\0\0" or "http://ns.adobe.com/xap/1.0/\0"). You MUST check for the "Exif\0\0" identifier to distinguish EXIF APP1 segments from XMP APP1 segments.

*   **File:** `src/writers/tiff_writer.rs`
    *   **Summary:** Implements TIFF IFD serialization via the `serialize_ifd()` function. Takes a `MetadataMap`, `ByteOrder`, and `ifd_start_offset` and returns a complete IFD structure as `Vec<u8>`. Handles both little-endian and big-endian output, inline values vs. offset values, and tag sorting by ID.
    *   **Recommendation:** You MUST use `serialize_ifd()` to convert EXIF tags to binary TIFF IFD format. The function filters for "EXIF:" prefixed tags automatically. Note that it returns ONLY the IFD structure (entry count + entries + next IFD offset + value area), NOT the TIFF header.
    *   **Critical Detail:** For JPEG EXIF, you need to prepend a TIFF header BEFORE the IFD data. The TIFF header is 8 bytes:
        - Little-endian: [0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]
        - Big-endian: [0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08]
        - The last 4 bytes are the IFD offset (always 8, pointing right after the header)

*   **File:** `src/writers/atomic_writer.rs`
    *   **Summary:** Provides the `write_atomic()` function for safe file writes using temp-file-and-rename pattern with fsync. Creates temp file in same directory, writes data, syncs to disk, and atomically renames.
    *   **Recommendation:** You SHOULD use `write_atomic()` when writing the final modified JPEG to disk in I3.T4 (write operations), but for THIS task (I3.T3), you're only implementing the segment replacement logic. The integration test will likely use `write_atomic()` indirectly through the write operations API.

*   **File:** `src/parsers/jpeg/mod.rs`
    *   **Summary:** Module entry point that re-exports `parse_segments` and `Segment` from `segment_parser`, plus other parsers (exif_parser, xmp_parser, iptc_parser).
    *   **Recommendation:** You can import directly from `crate::parsers::jpeg::{parse_segments, Segment}` for convenience.

### Implementation Tips & Notes

*   **Tip #1 - EXIF Identifier Detection:** When parsing segments to find the EXIF APP1 segment, you MUST check that `segment.data` starts with `b"Exif\0\0"` (6 bytes). Multiple APP1 segments can exist (EXIF, XMP), so don't assume the first APP1 is EXIF.
    ```rust
    const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";

    fn is_exif_segment(segment: &Segment) -> bool {
        segment.is_app1() && segment.data.starts_with(EXIF_IDENTIFIER)
    }
    ```

*   **Tip #2 - Segment Reconstruction:** To rebuild the JPEG, you need to write segments in order. For each segment:
    ```rust
    // Write marker (2 bytes, big-endian)
    output.extend_from_slice(&segment.marker.to_be_bytes());

    // For standalone markers (SOI, EOI, RST0-RST7), no length or data
    if is_standalone_marker(segment.marker) {
        continue;
    }

    // Calculate length: 2 (length field itself) + data.len()
    let length = 2 + segment.data.len();
    output.extend_from_slice(&(length as u16).to_be_bytes());

    // Write data
    output.extend_from_slice(segment.data);
    ```

*   **Tip #3 - EXIF APP1 Segment Construction:** When creating a new EXIF APP1 segment:
    ```rust
    // 1. Serialize EXIF tags to TIFF IFD
    let ifd_bytes = serialize_ifd(&metadata, ByteOrder::LittleEndian, 8)?;

    // 2. Build TIFF header (little-endian example)
    let tiff_header = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];

    // 3. Combine: EXIF identifier + TIFF header + IFD
    let mut segment_data = Vec::new();
    segment_data.extend_from_slice(b"Exif\0\0");
    segment_data.extend_from_slice(&tiff_header);
    segment_data.extend_from_slice(&ifd_bytes);

    // 4. Calculate segment length
    let segment_length = 2 + segment_data.len(); // 2 for length field itself

    // 5. Write APP1 marker, length, and data
    output.extend_from_slice(&0xFFE1u16.to_be_bytes()); // APP1 marker
    output.extend_from_slice(&(segment_length as u16).to_be_bytes());
    output.extend_from_slice(&segment_data);
    ```

*   **Tip #4 - Byte Order Consistency:** The TIFF writer supports both byte orders. For maximum compatibility, use **little-endian** (Intel byte order) as it's more common in modern systems. This matches the example in the TIFF parser tests.

*   **Tip #5 - Segment Order:** When reconstructing the JPEG:
    - Always keep SOI (0xFFD8) first
    - Preserve the order of all segments
    - If replacing EXIF, substitute the new APP1 segment in place of the old one
    - Keep EOI (0xFFD9) last
    - If no EXIF segment exists, insert the new APP1 segment early (typically after APP0/JFIF if present, or immediately after SOI)

*   **Warning:** The `segment_parser.rs` uses lifetime-based borrows (`Segment<'a>` with `data: &'a [u8]`). When building a new segment for writing, you'll need to create owned `Vec<u8>` data. Don't try to reuse the borrowed slices directly.

*   **Note:** The integration test should:
    1. Create a test JPEG with EXIF metadata
    2. Parse it using `parse_segments()`
    3. Modify a tag value (e.g., change "EXIF:Artist" from "Original" to "Modified")
    4. Use your writer function to create modified JPEG bytes
    5. Parse the modified JPEG again
    6. Verify the tag value changed and other segments are preserved

### Function Signature Recommendation

Based on the existing codebase patterns, I recommend this public API for your writer:

```rust
/// Writes modified EXIF metadata to a JPEG file structure.
///
/// This function:
/// 1. Parses the original JPEG using segment_parser
/// 2. Serializes modified EXIF tags using tiff_writer
/// 3. Replaces the EXIF APP1 segment (or inserts if not present)
/// 4. Returns the complete modified JPEG as Vec<u8>
///
/// # Parameters
/// - `reader`: FileReader for reading the original JPEG file
/// - `metadata`: MetadataMap containing EXIF tags to write (only "EXIF:" tags are processed)
///
/// # Returns
/// - `Ok(Vec<u8>)`: Complete modified JPEG file as bytes
/// - `Err(ExifToolError)`: If parsing fails or JPEG structure is invalid
pub fn write_exif_to_jpeg(
    reader: &dyn FileReader,
    metadata: &MetadataMap,
) -> Result<Vec<u8>>
```

### Testing Strategy

1. **Unit Tests** (in `jpeg_writer.rs`):
   - Test EXIF segment creation from metadata
   - Test segment replacement logic
   - Test handling of missing EXIF segment (insertion)
   - Test preservation of non-EXIF segments

2. **Integration Tests** (in `tests/integration/jpeg_write_tests.rs`):
   - Create test JPEG with known EXIF (e.g., Make="Canon", Model="EOS")
   - Modify one tag (e.g., Artist="TestArtist")
   - Write and re-parse
   - Verify: modified tag changed, other tags preserved, non-EXIF segments preserved
   - Use existing test fixtures or create minimal valid JPEGs programmatically

### Key Imports You'll Need

```rust
use crate::core::metadata_map::MetadataMap;
use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::{parse_segments, Segment};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::writers::tiff_writer::serialize_ifd;
```
