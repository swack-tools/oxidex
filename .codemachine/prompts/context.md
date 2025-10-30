# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I4.T6",
  "iteration_id": "I4",
  "iteration_goal": "Add support for PDF and MP4/QuickTime formats, implement batch processing with recursive directory traversal and parallel execution, add metadata copying between files, and expand tag registry.",
  "description": "Implement file renaming feature in src/cli/rename.rs. Support CLI pattern: exiftool-rs '-FileName<DateTimeOriginal' -d %Y%m%d_%H%M%S%%-.c.%%e <files> to rename files based on metadata. Parse filename pattern with variable substitution (e.g., ${EXIF:DateTimeOriginal}, ${EXIF:Make}). Support date formatting via -d flag (use chrono format strings). Add safety checks: dry-run mode (-n), prevent overwrites without confirmation. Add integration test.",
  "agent_type_hint": "BackendAgent",
  "inputs": "ExifTool -FileName and -d flag syntax, I2.T3 read operations",
  "target_files": [
    "src/cli/rename.rs",
    "src/cli/args.rs",
    "src/cli/mod.rs",
    "src/main.rs",
    "tests/integration/rename_tests.rs"
  ],
  "input_files": [
    "src/core/operations.rs",
    "src/cli/args.rs"
  ],
  "deliverables": [
    "File renaming based on metadata",
    "Variable substitution in filename patterns",
    "Date formatting support",
    "Dry-run mode"
  ],
  "acceptance_criteria": [
    "Supports -FileName pattern with metadata variable substitution",
    "-d flag applies date format to DateTime tags",
    "-n dry-run shows proposed renames without executing",
    "Prevents accidental overwrites (checks if target exists)",
    "Integration test: rename JPEGs by DateTimeOriginal, verify new names",
    "cargo test rename_tests passes"
  ],
  "dependencies": ["I2.T3"],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: task-i4-t6 (from .codemachine/artifacts/plan/02_Iteration_I4.md)

```markdown
<!-- anchor: task-i4-t6 -->
*   **Task 4.6: Implement File Renaming Based on Metadata**
    *   **Task ID:** `I4.T6`
    *   **Description:** Implement file renaming feature in `src/cli/rename.rs`. Support CLI pattern: `exiftool-rs '-FileName<DateTimeOriginal' -d %Y%m%d_%H%M%S%%-.c.%%e <files>` to rename files based on metadata. Parse filename pattern with variable substitution (e.g., ${EXIF:DateTimeOriginal}, ${EXIF:Make}). Support date formatting via `-d` flag (use `chrono` format strings). Add safety checks: dry-run mode (`-n`), prevent overwrites without confirmation. Add integration test.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** ExifTool -FileName and -d flag syntax, I2.T3 read operations
    *   **Input Files:** [`src/core/operations.rs`, `src/cli/args.rs`]
    *   **Target Files:**
        *   `src/cli/rename.rs`
        *   `src/cli/args.rs` (add rename arguments)
        *   `src/cli/mod.rs`
        *   `src/main.rs` (integrate renaming)
        *   `tests/integration/rename_tests.rs`
    *   **Deliverables:**
        *   File renaming based on metadata
        *   Variable substitution in filename patterns
        *   Date formatting support
        *   Dry-run mode
    *   **Acceptance Criteria:**
        *   Supports -FileName pattern with metadata variable substitution
        *   `-d` flag applies date format to DateTime tags
        *   `-n` dry-run shows proposed renames without executing
        *   Prevents accidental overwrites (checks if target exists)
        *   Integration test: rename JPEGs by DateTimeOriginal, verify new names
        *   `cargo test rename_tests` passes
    *   **Dependencies:** `I2.T3`
    *   **Parallelizable:** Yes (can be developed in parallel with other I4 tasks)
```

### Context: iteration-4-plan (from .codemachine/artifacts/plan/02_Iteration_I4.md)

```markdown
<!-- anchor: iteration-4-plan -->
### Iteration 4: Extended Format Support (PDF, MP4) & Batch Processing

*   **Iteration ID:** `I4`
*   **Goal:** Add support for PDF and MP4/QuickTime formats, implement batch processing with recursive directory traversal and parallel execution, add metadata copying between files, and expand tag registry.
*   **Prerequisites:** `I3` (write operations, atomic file handling, core formats supported)
```

### Context: tag-naming-convention (from docs/api/library_api.md)

```markdown
### Tag Naming Convention

All metadata tags in ExifTool-RS follow a standardized naming convention:

```
<FormatFamily>:<TagName>
```

**Examples:**

- `EXIF:Make` - Camera manufacturer (EXIF format)
- `EXIF:Model` - Camera model
- `EXIF:DateTime` - Image capture date/time
- `XMP-dc:Creator` - Document creator (XMP Dublin Core namespace)
- `GPS:Latitude` - GPS latitude coordinate
- `IPTC:Keywords` - Image keywords
- `PNG:Description` - PNG text chunk description

**Case Sensitivity:** Tag names are **case-sensitive**.
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/operations.rs`
    *   **Summary:** This file contains the core metadata operations including `read_metadata()`, `write_metadata()`, `modify_tag()`, and `copy_metadata()`. It orchestrates format detection, parser selection, and metadata extraction following the hexagonal architecture pattern.
    *   **Recommendation:** You MUST import and use the `read_metadata()` function from this file to extract metadata tags for use in filename patterns. This is the canonical way to read metadata in the project.
    *   **Important Detail:** The file includes helper functions like `tag_id_to_name()`, `is_datetime_string()`, and `parse_exif_datetime()` which parse EXIF DateTime format ("YYYY:MM:DD HH:MM:SS"). You can reference these for understanding the DateTime format.
    *   **Key Imports:** The module uses `chrono` for DateTime handling and returns `chrono::DateTime<Utc>` for DateTime values.

*   **File:** `src/cli/args.rs`
    *   **Summary:** This file defines the CLI argument structure using `clap::Parser`. It currently supports flags like `-json`, `--preserve-file-times`, `--backup`, `--readonly`, and `--TagsFromFile` with variable tag modification arguments.
    *   **Recommendation:** You MUST extend this file to add the following arguments for the rename feature:
        - `-FileName` pattern argument (Note: ExifTool uses the syntax `'-FileName<DateTimeOriginal'` where the pattern comes after `<`)
        - `-d` date format string argument
        - `-n` dry-run/no-execute flag
    *   **Pattern to Follow:** The existing `tag_modifications()` method shows how to parse arguments with special syntax (e.g., `-TAG=VALUE`). You should implement similar parsing logic for the `-FileName<pattern>` syntax.
    *   **Key Pattern:** The `parse_modification()` method uses `splitn(2, '=')` to handle values that may contain the delimiter. Use a similar approach for parsing the `-FileName<pattern>` syntax.

*   **File:** `src/core/metadata_map.rs`
    *   **Summary:** This file defines the `MetadataMap` structure which stores key-value pairs of metadata tags. It uses a `HashMap<String, TagValue>` internally and provides typed getter methods.
    *   **Recommendation:** You SHOULD use the `MetadataMap::get()` method to retrieve tag values from the metadata when substituting variables in filename patterns.
    *   **Type System:** TagValue is an enum that can be String, Integer, Float, Rational, Binary, DateTime, or Struct. You'll need to handle type conversion when formatting values into filenames.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** Defines the `TagValue` enum with variants for different metadata value types. Provides constructor methods like `new_string()`, `new_integer()`, `new_datetime()`, and accessor methods like `as_string()`, `as_integer()`, `as_datetime()`.
    *   **Recommendation:** You MUST use the `as_string()` and `as_datetime()` methods to safely extract values from TagValue when building filenames.
    *   **DateTime Handling:** The TagValue::DateTime variant stores `chrono::DateTime<Utc>`. You'll need to format this using the chrono format string provided via the `-d` flag.

### Implementation Tips & Notes

*   **Tip:** The project already has `chrono` as a dependency (visible in `src/core/operations.rs` imports). You SHOULD use `chrono::format::strftime` or the `format()` method on DateTime to apply the user's date format string.

*   **Tip:** For variable substitution in filename patterns, you'll need to parse patterns like:
    - `${EXIF:DateTimeOriginal}` - standard variable syntax
    - `%Y%m%d_%H%M%S` - strftime-style date format placeholders
    - `%%e` - file extension (note the double %% for literal %)
    - `%%-.c` - counter for avoiding name collisions (optional enhancement)

*   **Note:** ExifTool's actual syntax for -FileName is: `'-FileName<DateTimeOriginal'` which means "set FileName from the DateTimeOriginal tag". The `<` character is a redirection operator. When combined with `-d`, the date format is applied to DateTime tags before substitution.

*   **Warning:** File renaming is a destructive operation. You MUST implement:
    1. **Dry-run mode (`-n` flag):** Print proposed renames without executing
    2. **Collision detection:** Check if target filename already exists before renaming
    3. **Error handling:** Continue processing other files if one rename fails (graceful degradation)

*   **Testing Strategy:** Create integration tests with sample JPEG files that have EXIF:DateTimeOriginal tags. Verify:
    1. Correct filename generation from metadata
    2. Dry-run mode doesn't actually rename files
    3. Collision detection prevents overwrites
    4. Date formatting works with various chrono format strings

*   **Architecture Pattern:** Following the existing CLI pattern in `src/main.rs`, you should:
    1. Parse arguments in `CliArgs`
    2. Implement the rename logic in a new `src/cli/rename.rs` module
    3. Call the rename function from `main.rs` when rename arguments are detected
    4. Return a Result type for proper error propagation

*   **Edge Cases to Handle:**
    1. Tag doesn't exist in metadata (substitute with empty string or skip file?)
    2. Tag value is not a string or DateTime (convert to string representation)
    3. Resulting filename contains invalid characters for the OS
    4. File has no parent directory (can't rename)
    5. Permission denied on rename operation

*   **Reference Implementation:** Look at how `modify_tag()` in `src/core/operations.rs` handles read-modify-write workflow. Your rename feature should follow a similar pattern: read metadata → build new filename → perform rename operation.

*   **Security Note:** Ensure filename patterns cannot contain path traversal sequences like `../` or absolute paths. Renamed files should always stay in the same directory as the original file.

### ExifTool -FileName Syntax Reference

Based on the task description and ExifTool documentation, the `-FileName` syntax works as follows:

```bash
# Basic syntax: rename from a tag value
exiftool-rs '-FileName<DateTimeOriginal' photo.jpg
# Result: photo.jpg → "2025:01:15 10:30:00.jpg" (direct tag value)

# With date formatting: -d applies format to DateTime tags
exiftool-rs '-FileName<DateTimeOriginal' -d %Y%m%d_%H%M%S photo.jpg
# Result: photo.jpg → "20250115_103000.jpg"

# With extension preservation: %%e inserts original extension
exiftool-rs '-FileName<DateTimeOriginal' -d %Y%m%d_%H%M%S%%-.%%e photo.jpg
# Result: photo.jpg → "20250115_103000.jpg"

# With Make tag: non-DateTime tags used as-is
exiftool-rs '-FileName<Make_Model' photo.jpg
# Result: photo.jpg → "Canon_EOS 5D.jpg"

# Dry-run mode: -n shows what would happen without executing
exiftool-rs -n '-FileName<DateTimeOriginal' -d %Y%m%d photo.jpg
# Output: "photo.jpg → 20250115.jpg" (no actual rename)
```

**Key Implementation Requirements:**

1. The `-FileName` argument should accept a pattern starting with `<` followed by tag name(s)
2. Tag names in the pattern should support the standard `FAMILY:TagName` format (e.g., `EXIF:DateTimeOriginal` or just `DateTimeOriginal`)
3. The `-d` flag provides a chrono format string that applies to all DateTime tags in the pattern
4. Special placeholders:
   - `%%e` = original file extension (with dot)
   - `%%-.c` = counter for collisions (optional, can start with `.c` for first collision, `.2c` for second, etc.)
5. Multiple tags can be combined with underscores or other separators
6. The `-n` flag enables dry-run mode (print only, don't execute)

### Suggested Implementation Phases

**Phase 1: Argument Parsing**
- Extend `CliArgs` in `src/cli/args.rs` with `-FileName`, `-d`, and `-n` arguments
- Implement parsing logic to extract the pattern from `-FileName<pattern>` syntax
- Add validation to ensure the pattern is valid

**Phase 2: Pattern Substitution Engine**
- Create `src/cli/rename.rs` with a function to parse and substitute variables
- Implement tag name extraction from patterns (e.g., extract "DateTimeOriginal" from `<DateTimeOriginal>`)
- Handle special placeholders like `%%e` for extension
- Support both simple tag references and complex patterns

**Phase 3: Date Formatting**
- If `-d` flag is provided and tag value is DateTime, apply chrono formatting
- Convert DateTime to string using the provided format string
- Handle format errors gracefully (invalid format strings)

**Phase 4: Rename Execution**
- Implement file rename operation using `std::fs::rename()`
- Add dry-run mode logic (print proposed renames, don't execute)
- Add collision detection (check if target exists)
- Add error handling and reporting

**Phase 5: Integration and Testing**
- Update `src/main.rs` to detect rename arguments and call rename function
- Create integration tests in `tests/integration/rename_tests.rs`
- Test with various patterns, formats, and edge cases
- Ensure all acceptance criteria are met

---

## 4. Final Checklist for the Coder Agent

Before you begin coding, ensure you understand:

- ✅ The `-FileName<pattern>` syntax and how it differs from standard CLI arguments
- ✅ How to use `read_metadata()` from `src/core/operations.rs` to extract tag values
- ✅ How to work with `TagValue` enum and handle different types (String, DateTime, Integer)
- ✅ How to use `chrono` to format DateTime values with user-provided format strings
- ✅ The importance of dry-run mode (`-n`) and collision detection for safety
- ✅ The existing CLI argument parsing patterns in `src/cli/args.rs`
- ✅ The project's error handling patterns using `Result<T, ExifToolError>`

**Critical Success Factors:**

1. **Correctness:** Filenames must be generated accurately from metadata tags
2. **Safety:** Dry-run mode and collision detection prevent data loss
3. **Compatibility:** Follow ExifTool's -FileName syntax for user familiarity
4. **Error Handling:** Gracefully handle missing tags, invalid formats, and I/O errors
5. **Testing:** Integration tests must cover all acceptance criteria

Good luck! 🚀
