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

### Context: architectural-style (from 02_Architecture_Overview.md)

The project follows **Layered Hexagonal Architecture** (Ports and Adapters):

**Rationale**:
1. **Format Independence**: The core domain (metadata extraction/manipulation logic) must remain isolated from the specifics of 300+ file formats through ports (interfaces) and adapters (format-specific implementations).
2. **Testability**: Hexagonal architecture enables testing the core metadata logic independently of file I/O by mocking the file system port. Critical for achieving 80%+ test coverage.
3. **Extensibility**: New file format support becomes a matter of implementing the format adapter interface without touching core logic.

**Layered Structure**:
- **Domain Layer**: Format-agnostic metadata models, tag definitions, operations
- **Application Layer**: User-facing interfaces translating commands to domain operations
- **Infrastructure Layer**: Format-specific parsers/serializers, file system abstraction

### Context: technology-stack (from 02_Architecture_Overview.md)

**Binary Parsing**: `nom` v7 for complex formats (TIFF, QuickTime)
- Parser combinator library for binary formats
- Example: TIFF IFD parsing uses `nom::number::complete::le_u16` for little-endian u16, chained with `nom::multi::count` for tag array parsing

**Image I/O**: `memmap2` for memory-mapped files
- Efficient large file access without loading entire file into memory
- Enables zero-copy parsing for formats with known offsets

### Context: data-model (from 03_System_Structure_and_Data.md)

**Key Entities**:
1. **File**: Represents a media file being processed
2. **MetadataMap**: Collection of all metadata tags extracted from a file
3. **TagValue**: A single metadata tag with its name, value, and type information
4. **IFD (Image File Directory)**: TIFF-specific structural element containing tags

### Context: iteration-3-goal (from 02_Iteration_I3.md)

**Iteration 3 Goal**: Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.

**Task I3.T6 Specification**:
- Extend TIFF parser from I1.T11 to handle standalone TIFF files
- Parse 8-byte header (byte order, magic number 42, first IFD offset)
- Parse IFD chain (IFD0, IFD1 for thumbnails, sub-IFDs for EXIF/GPS)
- Support multi-page TIFF (follow next IFD offset)
- Extract all tags from all IFDs
- Ignore image pixel data (metadata only)
- Add integration test with sample TIFF file

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/tiff/ifd_parser.rs`
    *   **Summary:** This file contains a complete and well-tested implementation for parsing individual TIFF Image File Directories (IFDs). It includes:
        - `ByteOrder` enum (LittleEndian/BigEndian)
        - `IfdEntry` struct representing a single tag entry
        - `parse_ifd()` function that reads IFD at a given offset and returns `Vec<(u16, Vec<u8>)>` tag data
        - Support for both byte orders
        - Inline value extraction (≤4 bytes) vs offset-based values (>4 bytes)
        - Comprehensive unit tests with synthetic IFD data (699 lines total, ~200 lines of tests)
    *   **Recommendation:** You MUST import and reuse the `parse_ifd()` function from this file. DO NOT rewrite IFD parsing logic. Your task is to add the TIFF file-level parser that orchestrates calling `parse_ifd()` for each IFD in the chain.
    *   **Key Functions to Reuse:**
        ```rust
        pub fn parse_ifd(reader: &dyn FileReader, ifd_offset: u64, byte_order: ByteOrder) -> Result<Vec<(u16, Vec<u8>)>>
        pub enum ByteOrder { LittleEndian, BigEndian }
        ```

*   **File:** `src/parsers/tiff/mod.rs`
    *   **Summary:** TIFF parser module file. Currently very minimal with just module declarations for `ifd_parser`, `makernote_parser`, and `tag_parser`.
    *   **Recommendation:** Add `pub mod file_parser;` to this file.

*   **File:** `src/parsers/format_detector.rs`
    *   **Summary:** Contains `detect_format()` function that identifies TIFF files by magic bytes:
        - Little-Endian: `0x49 0x49 0x2A 0x00` ("II" + 42)
        - Big-Endian: `0x4D 0x4D 0x00 0x2A` ("MM" + 42)
    *   **Recommendation:** You DO NOT need to modify this file. Format detection for TIFF is already complete.

*   **File:** `src/core/operations.rs`
    *   **Summary:** Orchestrates metadata extraction workflow. Contains `read_metadata()` which routes to format-specific parsers. Has `parse_tiff_metadata()` function that needs your full TIFF file parser.
    *   **Recommendation:** Once you implement the TIFF file parser, update `parse_tiff_metadata()` to call your new parser. This function exists at line ~100.

*   **File:** `src/io/mmap_reader.rs` and `src/io/buffered_reader.rs`
    *   **Summary:** Implementations of the `FileReader` trait providing `read(offset, length)` and `size()` methods.
    *   **Recommendation:** Your TIFF file parser MUST accept a `&dyn FileReader` parameter (following hexagonal architecture). Use the reader's `read()` method to access file data.

### Implementation Tips & Notes

*   **Tip:** The TIFF file structure you need to parse is:
    ```
    Bytes 0-1:   Byte order marker (0x4949="II" or 0x4D4D="MM")
    Bytes 2-3:   Magic number 42 (0x002A in the detected byte order)
    Bytes 4-7:   Offset to first IFD (4-byte offset from start of file)
    At IFD offset: IFD0 data (parsed by existing parse_ifd())
    After IFD entries: 4-byte "next IFD offset" (0 = no more IFDs)
    ```

*   **Tip:** For multi-page TIFF support, you MUST follow the IFD chain:
    1. Parse the 8-byte TIFF header to get first IFD offset and byte order
    2. Call `parse_ifd()` at that offset
    3. After the IFD entries, at position `ifd_offset + 2 + (entry_count * 12)`, read the 4-byte "next IFD offset"
    4. If next offset is non-zero, repeat from step 2
    5. Collect all tags from all IFDs into a single `Vec<(u16, Vec<u8>)>`

*   **Tip:** To read the "next IFD offset" after parsing an IFD:
    - The existing `parse_ifd()` function returns tag data but doesn't return the entry count
    - You'll need to read the entry count yourself from `ifd_offset` (2 bytes)
    - Then calculate: `next_offset_position = ifd_offset + 2 + (entry_count * 12)`
    - Read 4 bytes at this position using the detected byte order

*   **Note:** According to acceptance criteria, you should IGNORE image pixel data:
    - DO NOT parse or extract actual image bytes (stored in strips or tiles)
    - Only extract metadata tags from IFDs
    - Tags pointing to image data (StripOffsets, TileOffsets) can be extracted as metadata but don't follow those offsets

*   **Note:** For sub-IFDs (EXIF IFD, GPS IFD), these are referenced by specific tags in IFD0:
    - Tag 0x8769 (ExifIFDPointer) contains offset to EXIF sub-IFD
    - Tag 0x8825 (GPSInfoIFDPointer) contains offset to GPS sub-IFD
    - You SHOULD parse these sub-IFDs by checking for these tags and recursively calling `parse_ifd()` at those offsets

*   **Warning:** Be careful with byte order consistency. The byte order marker is read once at file start, and ALL subsequent multi-byte values use that same byte order. Pass `ByteOrder` through all parsing functions.

*   **Tip:** Suggested function structure for `file_parser.rs`:
    ```rust
    pub fn parse_tiff_file(reader: &dyn FileReader) -> Result<Vec<(u16, Vec<u8>)>>
    fn parse_tiff_header(reader: &dyn FileReader) -> Result<(ByteOrder, u32)>
    fn read_entry_count(reader: &dyn FileReader, ifd_offset: u64, byte_order: ByteOrder) -> Result<u16>
    fn read_next_ifd_offset(reader: &dyn FileReader, offset: u64, byte_order: ByteOrder) -> Result<u32>
    fn parse_ifd_chain(reader: &dyn FileReader, first_ifd_offset: u64, byte_order: ByteOrder) -> Result<Vec<(u16, Vec<u8>)>>
    ```

*   **Tip:** For integration tests, create a sample TIFF file:
    1. Use an existing TIFF from a public test corpus
    2. Generate one using ImageMagick: `convert -size 100x100 xc:white sample.tif`
    3. Create a minimal synthetic TIFF using the same approach as unit tests in `ifd_parser.rs`

*   **Note:** Return type should be `Vec<(u16, Vec<u8>)>` where u16 is tag ID and Vec<u8> is raw tag value. The orchestration in `operations.rs` will convert these to TagValues and populate MetadataMap.

### Testing Strategy

*   **Unit Tests:** Add unit tests in `file_parser.rs` for:
    - TIFF header parsing (both byte orders)
    - Next IFD offset reading
    - Single-IFD file parsing
    - Multi-page TIFF parsing (2-3 IFDs)

*   **Integration Test:** Create `tests/integration/tiff_tests.rs` that:
    - Reads a real TIFF file from `tests/fixtures/tiff/sample.tif`
    - Calls your parser via the public API
    - Verifies expected tags are extracted
    - Tests both single-page and multi-page TIFF files if possible
    - Follow the pattern from existing integration tests (see `tests/integration/` directory)

### File Structure Guidance

**Create these new files:**
1. `src/parsers/tiff/file_parser.rs` - Your main implementation
2. `tests/integration/tiff_tests.rs` - Integration tests
3. `tests/fixtures/tiff/sample.tif` - Sample TIFF file for testing

**Update these existing files:**
1. `src/parsers/tiff/mod.rs` - Add `pub mod file_parser;`
2. `src/core/operations.rs` - Update `parse_tiff_metadata()` to use your new parser

### Critical Integration Points

*   **Hexagonal Architecture Compliance**: Your parser MUST use the `FileReader` trait, NOT direct file I/O. This allows the parser to work with both memory-mapped files and buffered readers.

*   **Error Handling**: Use the existing `ExifToolError` type from `src/error/mod.rs`. The `parse_ifd()` function already returns `Result<Vec<(u16, Vec<u8>)>>`, so follow this pattern.

*   **Nom Usage**: You may use `nom` parser combinators for byte order and header parsing, following the patterns in `ifd_parser.rs`. Alternatively, manual parsing with byte array slicing is acceptable for the simple TIFF header.

*   **Integration with Operations**: The `parse_tiff_metadata()` function in `operations.rs` currently has a stub implementation. Update it to:
    ```rust
    fn parse_tiff_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
        use crate::parsers::tiff::file_parser::parse_tiff_file;
        let tags = parse_tiff_file(reader)?;
        // Convert tags to MetadataMap (may need additional implementation)
        // ...
    }
    ```

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I3.T5",
  "iteration_id": "I3",
  "iteration_goal": "Implement metadata write operations with atomic file handling, extend TIFF parser for standalone TIFF files (not just EXIF in JPEG), implement metadata serialization, and add tag modification capabilities to CLI.",
  "description": "Extend CLI args in src/cli/args.rs to support tag modification: -TAG_NAME=VALUE syntax (e.g., -EXIF:Artist=John Doe). Parse modification arguments, call modify_tag() from I3.T4. Support multiple modifications in one command (e.g., exiftool-rs -EXIF:Artist=John -EXIF:Copyright=2025 photo.jpg). Update main.rs to handle write operations. Add validation that file is writable. Print success/failure message.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I2.T8 CLI args, I3.T4 write operations",
  "target_files": [
    "src/cli/args.rs",
    "src/main.rs"
  ],
  "input_files": [
    "src/cli/args.rs",
    "src/core/operations.rs"
  ],
  "deliverables": "CLI support for -TAG=VALUE syntax, multiple modifications in one command",
  "acceptance_criteria": "exiftool-rs -EXIF:Artist=John Doe photo.jpg modifies tag, multiple modifications work: -Tag1=Val1 -Tag2=Val2, prints success message: 1 image file updated, prints error on validation failure: Invalid value for TAG, verifies file exists and is writable before modification, manual test: modify tag, re-run with read, verify change",
  "dependencies": [
    "I3.T4"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: CLI Interface Specification (from 04_Behavior_and_Communication.md)

```markdown
**Secondary APIs**:

1. **CLI Interface**: POSIX-style arguments mimicking ExifTool
   ```bash
   exiftool-rs -EXIF:DateTime photo.jpg
   exiftool-rs -json -r /photos/  # Recursive JSON output
   exiftool-rs -TagsFromFile src.jpg -all:all dest.jpg  # Copy metadata
   ```

**Justification**:

- **Rust-First**: Leverages Rust's type system for compile-time safety (no invalid tag names at compile time via const tag identifiers)
- **No Network API**: ExifTool-RS is a library/tool, not a service. REST/GraphQL APIs would be implemented by consuming applications
- **FFI for Interop**: Enables Python (`pyo3`), Node.js (`neon`), Go (`cgo`) bindings without compromising Rust API ergonomics
```

### Context: Metadata Write Operation Flow (from 04_Behavior_and_Communication.md)

```markdown
#### Alternative Flow: Metadata Write Operation

**Description**: Sequence for **modifying metadata and writing back to file**.

**Key Design Decisions**:

1. **Read-Modify-Write**: Always read existing metadata first to preserve unmodified tags
2. **In-Place vs. Rewrite**: Attempt in-place modification if new metadata fits in existing segment; otherwise rewrite entire file
3. **Atomic Write**: Use temporary file + atomic rename to prevent corruption on crash
4. **Validation Before Write**: Validate tag values against type constraints before any file modification
```

### Context: Task I3.T5 Specification (from 02_Iteration_I3.md)

```markdown
*   **Task 3.5: Extend CLI to Support Tag Modification**
    *   **Task ID:** `I3.T5`
    *   **Description:** Extend CLI args in `src/cli/args.rs` to support tag modification: `-TAG_NAME=VALUE` syntax (e.g., `-EXIF:Artist="John Doe"`). Parse modification arguments, call modify_tag() from I3.T4. Support multiple modifications in one command (e.g., `exiftool-rs -EXIF:Artist="John" -EXIF:Copyright="2025" photo.jpg`). Update main.rs to handle write operations. Add validation that file is writable. Print success/failure message.
    *   **Acceptance Criteria:**
        *   `exiftool-rs -EXIF:Artist="John Doe" photo.jpg` modifies tag
        *   Multiple modifications work: `-Tag1=Val1 -Tag2=Val2`
        *   Prints success message: "1 image file updated"
        *   Prints error on validation failure: "Invalid value for TAG"
        *   Verifies file exists and is writable before modification
        *   Manual test: modify tag, re-run with read, verify change
```

### Context: CLI Framework and Technology Stack (from 01_Plan_Overview_and_Setup.md)

```markdown
*   **Core Libraries:**
    *   CLI Framework: `clap` v4 (derive API)
    *   Binary Parsing: `nom` v7 (complex formats) + `binrw` (simple struct-based formats)
    *   JSON Output: `serde_json`
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/cli/args.rs`
    *   **Summary:** This file defines the current CLI argument structure using `clap` v4 derive API. It currently supports: positional `FILE` argument, `-json` flag, `-s` (short format, not implemented), `-a` (all tags, no effect), and `-r` (recursive, not implemented).
    *   **Current Structure:** Uses `#[derive(Parser, Debug)]` with simple flags. All arguments are boolean flags or a single PathBuf.
    *   **Recommendation:** You MUST extend this struct to capture tag modification arguments. Clap's derive API does NOT natively support the `-TAG=VALUE` syntax directly. You have two implementation options:
        1. **Option A (Recommended):** Use `#[arg(allow_hyphen_values = true, number_of_values = 0..)]` with `Vec<String>` to capture all remaining arguments, then parse `-TAG=VALUE` manually in main.rs
        2. **Option B:** Use clap's `value_parser` with a custom parser function, but this is more complex
    *   **Important Note:** The existing code shows warnings for unimplemented features (recursive, short_format). You should follow this pattern for your success/error messages.

*   **File:** `src/main.rs`
    *   **Summary:** This is the current CLI entry point. It handles: parsing args with `clap::Parser`, reading metadata via `read_metadata()`, formatting output with `HumanReadableFormatter` or `JsonFormatter`, and error handling with `process::exit(1)`.
    *   **Current Workflow:** Parse args → Read metadata → Format output → Print. It only supports READ operations currently.
    *   **Recommendation:** You MUST extend the workflow to:
        1. Detect if tag modifications are requested (check if any `-TAG=VALUE` args present)
        2. If modifications present: Parse tag assignments, call `modify_tag()` for each, handle errors, print success message
        3. If no modifications: Use existing read-only workflow
    *   **File Writability Check:** Use `std::fs::metadata(path)?.permissions().readonly()` to check if file is writable before attempting modification.
    *   **Error Handling Pattern:** The existing code uses `match` with `Err(e)` and `eprintln!()` for errors. You SHOULD follow this pattern for consistency.

*   **File:** `src/core/operations.rs`
    *   **Summary:** This file contains the core metadata operations including `read_metadata()`, `write_metadata()`, and `modify_tag()`. The `modify_tag()` function (lines 491-502) is your primary integration point.
    *   **Function Signature:** `pub fn modify_tag(path: &Path, tag_name: &str, new_value: TagValue) -> Result<()>`
    *   **Recommendation:** You MUST import and call this function from main.rs. The function already handles: reading existing metadata, modifying the single tag, and writing back atomically.
    *   **Key Design:** `modify_tag()` is a convenience wrapper that preserves all other tags unchanged. It's the perfect API for CLI tag modification.
    *   **Error Types:** Returns `Result<(), ExifToolError>` which includes: `IoError`, `ParseError`, `InvalidTagValue`, `UnsupportedFormat`. You need to handle these in main.rs.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** Defines the `TagValue` enum with variants: String, Integer, Float, Rational, Binary, DateTime, Struct. Includes constructor methods like `TagValue::new_string()`, `TagValue::new_integer()`.
    *   **Recommendation:** You MUST parse the VALUE part of `-TAG=VALUE` and convert it to the appropriate `TagValue` variant. For this iteration, **only String values are required** (per acceptance criteria example `-EXIF:Artist="John Doe"`). Future iterations can add type detection.
    *   **String Parsing:** Use `TagValue::new_string()` constructor. Handle quoted strings correctly (strip surrounding quotes if present).

*   **File:** `src/core/validation.rs`
    *   **Summary:** Contains `validate_tag_value()` function that checks TagValue against TagDescriptor type constraints. This is called automatically by `write_metadata()` (see operations.rs:402-409).
    *   **Recommendation:** You do NOT need to call this directly. The validation happens inside `write_metadata()` which is called by `modify_tag()`. Your CLI code will receive validation errors via the `Result` type.

### Implementation Tips & Notes

*   **Tip:** The Perl ExifTool uses `-TAG=VALUE` syntax (with single hyphen). For Rust clap, you need to carefully handle this because clap normally expects `--tag=value` (double hyphen) or `-t value` (short flag + value). The `allow_hyphen_values = true` option is critical.

*   **Tip:** To parse `-TAG=VALUE` from command line args:
    1. Capture remaining args as `Vec<String>`
    2. Iterate through each arg
    3. Check if it starts with `-` and contains `=`
    4. Split on first `=` to get tag_name and value
    5. Strip leading `-` from tag_name
    6. Strip surrounding quotes from value if present

*   **Note:** Multiple modifications example: `exiftool-rs -EXIF:Artist=John -EXIF:Copyright=2025 photo.jpg`. The FILE argument must come LAST. This is standard Unix convention. Make sure your parsing preserves this.

*   **Note:** For the success message, Perl ExifTool outputs "1 image files updated" (note: plural "files" even for 1 file). You should match this for backward compatibility.

*   **Warning:** The task requires checking if file is writable BEFORE attempting modification. Use `std::fs::metadata()` to check permissions. If file is read-only, print a clear error message and exit before calling `modify_tag()`. This prevents cryptic errors from atomic writer.

*   **Warning:** When printing error messages, the task specifies format "Invalid value for TAG". Make sure your error handling extracts the tag name from `ExifToolError::InvalidTagValue` and formats it correctly.

*   **Tip:** For manual testing (acceptance criteria), you can use the existing test fixtures in `tests/fixtures/jpeg/` directory. The `sample_with_exif.jpg` file is a good candidate for testing tag modification.

### Suggested Implementation Steps

1. **Extend src/cli/args.rs:**
   - Add a new field to `CliArgs` struct: `pub tag_modifications: Vec<String>` with appropriate clap attributes
   - Use `#[arg(allow_hyphen_values = true)]` to capture `-TAG=VALUE` args
   - OR use clap's `trailing_var_arg` if needed

2. **Update src/main.rs:**
   - After parsing args, check if `args.tag_modifications` is non-empty
   - If empty: Use existing read-only workflow
   - If non-empty:
     a. Check file exists and is writable
     b. Parse each modification string into (tag_name, value) pairs
     c. For each pair, call `modify_tag(path, tag_name, TagValue::new_string(value))`
     d. Collect any errors
     e. If all succeed: print "1 image files updated"
     f. If any fail: print error with tag name and exit with code 1

3. **Test your implementation:**
   - Compile and run basic modification: `./target/debug/exiftool-rs -EXIF:Artist="Test Artist" tests/fixtures/jpeg/sample_with_exif.jpg`
   - Verify with read: `./target/debug/exiftool-rs tests/fixtures/jpeg/sample_with_exif.jpg | grep Artist`
   - Test multiple modifications: `-EXIF:Artist="John" -EXIF:Copyright="2025"`
   - Test error cases: invalid file, read-only file, invalid tag value type

### Code Quality Reminders

*   Ensure `cargo fmt` compliance (project uses rustfmt.toml)
*   Run `cargo clippy` and fix all warnings
*   Add appropriate `// TODO:` comments for features not yet implemented (e.g., type detection for non-string values)
*   Follow existing error message formatting style (see main.rs:51-55 for reference)
*   Use descriptive variable names following Rust conventions (snake_case)
