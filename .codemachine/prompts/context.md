# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

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
