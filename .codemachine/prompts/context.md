# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I2.T9",
  "iteration_id": "I2",
  "iteration_goal": "Implement tag registry with subset of ExifTool tags, core metadata read/write operations, basic CLI with argument parsing, and extend format support to include XMP parsing and PNG format.",
  "description": "Implement output formatters in src/cli/output_formatter.rs. Create trait OutputFormatter with fn format(&self, metadata: &MetadataMap) -> String. Implement two formatters: HumanReadableFormatter (key-value pairs, one per line, e.g., EXIF:Make: Canon\\n), JsonFormatter (serialize MetadataMap using serde_json). Support filtering for specific tags. Update main.rs to select formatter based on CLI args (-json flag). Add unit tests for both formatters.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I2.T8 CLI args, I1.T6 MetadataMap with serde",
  "target_files": [
    "src/cli/output_formatter.rs",
    "src/cli/mod.rs",
    "src/main.rs"
  ],
  "input_files": [
    "src/cli/args.rs",
    "src/core/metadata_map.rs"
  ],
  "deliverables": "OutputFormatter trait, HumanReadableFormatter and JsonFormatter implementations, unit tests",
  "acceptance_criteria": "HumanReadableFormatter outputs Tag: Value\\n format, JsonFormatter produces valid JSON (parseable by jq), formatters handle empty MetadataMap gracefully, tag filtering works (only specified tags in output), unit tests verify both formatters with sample data, cargo test output_formatter passes",
  "dependencies": [
    "I2.T8"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: API Design & Communication - API Style (from 04_Behavior_and_Communication.md)

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

**Secondary APIs**:

1. **CLI Interface**: POSIX-style arguments mimicking ExifTool
   ```bash
   exiftool-rs -EXIF:DateTime photo.jpg
   exiftool-rs -json -r /photos/  # Recursive JSON output
   exiftool-rs -TagsFromFile src.jpg -all:all dest.jpg  # Copy metadata
   ```

2. **C FFI**: Minimal C-compatible surface for foreign language bindings
   ```c
   // C API example
   ExifToolHandle* handle = exiftool_create();
   ExifToolError err = exiftool_read_file(handle, "photo.jpg");
   const char* model = exiftool_get_string(handle, "EXIF:Model");
   exiftool_destroy(handle);
   ```

**Justification**:

- **Rust-First**: Leverages Rust's type system for compile-time safety (no invalid tag names at compile time via const tag identifiers)
- **No Network API**: ExifTool-RS is a library/tool, not a service. REST/GraphQL APIs would be implemented by consuming applications
- **FFI for Interop**: Enables Python (`pyo3`), Node.js (`neon`), Go (`cgo`) bindings without compromising Rust API ergonomics
```

### Context: Communication Patterns (from 04_Behavior_and_Communication.md)

```markdown
#### Communication Patterns

**Primary Pattern**: **Synchronous Request/Response**

All operations are synchronous:
1. User/application calls API function
2. Function parses file, extracts/modifies metadata
3. Function returns result or error
4. Transaction completes

**Rationale**:
- File I/O is the bottleneck, not computation. Async overhead provides no benefit.
- Synchronous code is simpler to reason about for library consumers.
- Batch parallelism is achieved via `rayon` at the application level (parallel iterator over file list), not async/await.

**Batch Processing**: Uses data parallelism (not message passing)

```rust
use rayon::prelude::*;

let results: Vec<Result<Metadata>> = file_paths
    .par_iter()  // Rayon parallel iterator
    .map(|path| Metadata::from_path(path))
    .collect();
```

Rayon's work-stealing scheduler distributes file processing across CPU cores automatically.

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

### Context: Task 2.9 Full Specification (from 02_Iteration_I2.md)

```markdown
<!-- anchor: task-i2-t9 -->
*   **Task 2.9: Implement Output Formatters (Human-Readable and JSON)**
    *   **Task ID:** `I2.T9`
    *   **Description:** Implement output formatters in `src/cli/output_formatter.rs`. Create `trait OutputFormatter` with `fn format(&self, metadata: &MetadataMap) -> String`. Implement two formatters: `HumanReadableFormatter` (key-value pairs, one per line, e.g., "EXIF:Make: Canon\n"), `JsonFormatter` (serialize MetadataMap using serde_json). Support filtering for specific tags. Update main.rs to select formatter based on CLI args (-json flag). Add unit tests for both formatters.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** I2.T8 CLI args, I1.T6 MetadataMap with serde
    *   **Input Files:** [`src/cli/args.rs`, `src/core/metadata_map.rs`]
    *   **Target Files:**
        *   `src/cli/output_formatter.rs`
        *   `src/cli/mod.rs`
        *   `src/main.rs` (update to use formatter)
    *   **Deliverables:**
        *   OutputFormatter trait
        *   HumanReadableFormatter and JsonFormatter implementations
        *   Unit tests
    *   **Acceptance Criteria:**
        *   HumanReadableFormatter outputs "Tag: Value\n" format
        *   JsonFormatter produces valid JSON (parseable by `jq`)
        *   Formatters handle empty MetadataMap gracefully
        *   Tag filtering works (only specified tags in output)
        *   Unit tests verify both formatters with sample data
        *   `cargo test output_formatter` passes
    *   **Dependencies:** `I2.T8`
    *   **Parallelizable:** No (depends on CLI args)
```

### Context: Technology Stack (from 01_Plan_Overview_and_Setup.md)

```markdown
*   **Technology Stack:**
    *   **Frontend:** None (CLI only for v1.0)
    *   **Backend Language:** Rust 1.75+ (2021 Edition)
    *   **Core Libraries:**
        *   CLI Framework: `clap` v4 (derive API)
        *   Binary Parsing: `nom` v7 (complex formats) + `binrw` (simple struct-based formats)
        *   XML Parsing: `quick-xml` (XMP metadata)
        *   JSON Output: `serde_json`
        *   Date/Time: `chrono`
        *   String Encoding: `encoding_rs`
        *   Concurrency: `rayon` (data parallelism)
        *   Memory-mapped I/O: `memmap2`
    *   **Testing:** `cargo test`, `proptest` (property-based), `cargo-fuzz` (fuzzing)
    *   **C FFI:** `cbindgen` (header generation)
    *   **Documentation:** `rustdoc`, `mdBook`
    *   **Build System:** `cargo` + `cross` (cross-compilation)
    *   **CI/CD:** GitHub Actions
    *   **Code Quality:** `clippy`, `rustfmt`, `cargo-audit`
    *   **Benchmarking:** `criterion`
    *   **Database:** None (file-based, stateless operation)
    *   **Messaging/Queues:** None (synchronous processing)
    *   **Deployment:** Static binaries, Rust crate (crates.io), optional Docker image
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/metadata_map.rs`
    *   **Summary:** This file contains the `MetadataMap` struct which is the core data structure for storing extracted metadata as a `HashMap<String, TagValue>`. It has comprehensive methods including typed getters (`get_string()`, `get_integer()`, `get_float()`, `get_datetime()`), iterators (`iter()`, `keys()`, `values()`), and is fully annotated with `#[derive(Serialize, Deserialize)]` from serde.
    *   **Recommendation:** You MUST import and use `MetadataMap` from this module. The struct is already fully set up with serde support, so JSON serialization will work out-of-the-box via `serde_json::to_string()` or `serde_json::to_string_pretty()`.
    *   **Critical Detail:** Notice that the serde annotation uses `#[serde(flatten)]` on the internal `tags` field (line 21-22), which means when serialized to JSON, tags appear at the root level (e.g., `{"EXIF:Make": {...}}`) rather than nested under a `tags` key.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** This file defines the `TagValue` enum with variants for String, Integer, Float, Rational, Binary, DateTime, and Struct. It has serde annotations: `#[serde(tag = "type", content = "value")]` (line 16), meaning JSON output will be formatted as `{"type": "String", "value": "Canon"}`.
    *   **Recommendation:** You MUST understand this enum structure when implementing the HumanReadableFormatter. For human-readable output, you'll want to display just the value, not the full JSON structure. Use the helper methods like `as_string()`, `as_integer()`, etc., or match on the enum variants to extract the display value.
    *   **Important Pattern:** The existing tests show how to format TagValue for display. For example, in test_debug_derive at line 249, they use `format!("{:?}", value)`. However, for production human-readable output, you'll want to implement custom formatting logic that extracts clean values.

*   **File:** `src/cli/args.rs`
    *   **Summary:** This file defines the `CliArgs` struct using clap's derive API. Current fields include: `file: PathBuf`, `json: bool`, `short_format: bool`, `all_tags: bool`, and `recursive: bool`.
    *   **Recommendation:** You MUST access the `json` flag from `CliArgs` to determine which formatter to use in `main.rs`. The flag is already implemented and parsed (line 18-19).
    *   **Note:** The `short_format` and `recursive` flags are marked as "not yet implemented" in comments, so ignore these for now. Focus on the `json` flag only.

*   **File:** `src/main.rs`
    *   **Summary:** The current main.rs has basic inline formatting logic. Lines 32-40 handle JSON output using `serde_json::to_string_pretty(&metadata)`. Lines 42-54 handle human-readable output with custom formatting logic (displays file name, count, and sorted tag list with `{:?}` formatting).
    *   **Recommendation:** You MUST refactor this code to use your new OutputFormatter trait. The existing logic provides a good template for what each formatter should do. Replace the inline formatting with calls to your formatter implementations.
    *   **Critical Pattern:** Notice that the current human-readable format sorts tags by name (line 48-49: `tags.sort_by_key(|(name, _)| *name)`). You SHOULD preserve this sorting behavior in your HumanReadableFormatter for consistent output.

*   **File:** `src/cli/output_formatter.rs`
    *   **Summary:** This file exists but is essentially empty—it only contains a module docstring (line 1-3) and `#![allow(dead_code)]` (line 5).
    *   **Recommendation:** You MUST implement the complete OutputFormatter trait and both formatter structs in this file from scratch.

*   **File:** `src/cli/mod.rs`
    *   **Summary:** This module file currently only declares `pub mod args;` (line 8). It has `#![allow(dead_code)]` at the top.
    *   **Recommendation:** You MUST add `pub mod output_formatter;` to this file to make your new module accessible to the rest of the crate.

### Implementation Tips & Notes

*   **Tip:** The task description specifies `fn format(&self, metadata: &MetadataMap) -> String` for the trait, but also mentions "support filtering for specific tags." You SHOULD add an optional parameter for tag filtering, such as:
    ```rust
    pub trait OutputFormatter {
        fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String;
    }
    ```
    This will allow callers to specify which tags to include in the output. However, since the current CLI doesn't have tag filtering implemented yet, you can pass `None` for now.

*   **Tip:** For the HumanReadableFormatter, the task specifies output format as "Tag: Value\n". Based on the current main.rs implementation (line 52), the format should be:
    ```
    EXIF:Make: Canon
    EXIF:Model: EOS 5D Mark IV
    ```
    Note the space after the colon. You SHOULD match this format for consistency with the existing code.

*   **Note:** For the HumanReadableFormatter value display, you need to handle the TagValue enum properly. The current main.rs uses `{:?}` (Debug trait at line 52), which outputs the full enum structure like `String("Canon")`. For a cleaner human-readable format, you SHOULD implement custom formatting that extracts just the value. For example:
    - `TagValue::String("Canon")` should display as `Canon`, not `String("Canon")`
    - `TagValue::Integer(400)` should display as `400`, not `Integer(400)`
    - `TagValue::Rational{numerator: 1, denominator: 100}` should display as `1/100`
    - Consider using a match statement to format each variant appropriately

*   **Note:** For the JsonFormatter, you already have everything you need. The MetadataMap has `#[derive(Serialize)]`, so you can use:
    ```rust
    serde_json::to_string_pretty(metadata)
    ```
    or
    ```rust
    serde_json::to_string(metadata)
    ```
    depending on whether you want pretty-printed JSON. The current main.rs uses `to_string_pretty` (line 34), so you SHOULD follow that precedent for consistency.

*   **Warning:** The acceptance criteria state "JsonFormatter produces valid JSON (parseable by jq)". You MUST handle serde_json serialization errors properly. The `to_string_pretty()` method returns a `Result<String, serde_json::Error>`. Your trait should either:
    1. Return `Result<String, Error>` instead of `String`, OR
    2. Handle errors internally and return a fallback (empty string or error message)

    I recommend option 1 for better error propagation, but since the task specifies `-> String`, you'll need to handle errors gracefully within the formatter.

*   **Tip:** For unit tests, you SHOULD test the following scenarios:
    1. Empty MetadataMap (should not panic, should return empty or minimal output)
    2. MetadataMap with multiple tags of different types (String, Integer, Float, DateTime, Rational)
    3. Tag filtering: provide `Some(&["EXIF:Make".to_string()])` and verify only that tag appears
    4. Tag filtering with non-existent tag (should not panic, should return empty output)
    5. For JsonFormatter: verify output is parseable by `serde_json::from_str()` to ensure valid JSON
    6. For HumanReadableFormatter: verify tags are sorted alphabetically
    7. For HumanReadableFormatter: verify proper formatting of each TagValue variant (String, Integer, Float, etc.)

*   **Critical:** The task says to "update main.rs to select formatter based on CLI args." You MUST modify main.rs to:
    1. Create an instance of the appropriate formatter based on `args.json`
    2. Call the formatter's `format()` method instead of inline formatting
    3. Print the result
    4. Ensure backwards compatibility with current behavior (same output format)

*   **Best Practice:** Since this is a library crate (has both lib.rs and bin target), you SHOULD make the OutputFormatter trait and implementations public so they can be used by library consumers, not just the CLI. Define them as `pub trait` and `pub struct`.

*   **Code Style:** The existing codebase follows these patterns you SHOULD maintain:
    - Comprehensive doc comments (`///`) on all public items with proper formatting
    - Example code blocks in doc comments showing usage
    - `#[cfg(test)]` module at the bottom of each file for tests
    - Import groups: std library first, then external crates, then internal modules
    - Allow dead_code is currently used liberally in stub files, but you can remove it once code is actually used

*   **Testing Strategy:** The test module structure should follow the pattern seen in metadata_map.rs (lines 168-316):
    ```rust
    #[cfg(test)]
    mod tests {
        use super::*;
        // Import any additional test dependencies like chrono for DateTime tests

        #[test]
        fn test_human_readable_formatter_empty_metadata() { ... }

        #[test]
        fn test_json_formatter_basic() { ... }

        // ... more tests (aim for 10+ tests covering all scenarios)
    }
    ```

*   **Formatter Pattern:** Based on the architecture documents mentioning builder patterns and the fact that formatters don't have state, you SHOULD implement formatters as zero-sized types (unit structs):
    ```rust
    pub struct HumanReadableFormatter;
    pub struct JsonFormatter;
    ```
    This is more idiomatic Rust than empty structs with fields, and it's zero-cost at runtime.

*   **TagValue Display Logic:** For the HumanReadableFormatter, you'll need to implement display logic for each TagValue variant. Here's a suggested approach:
    ```rust
    fn format_tag_value(value: &TagValue) -> String {
        match value {
            TagValue::String(s) => s.clone(),
            TagValue::Integer(i) => i.to_string(),
            TagValue::Float(f) => f.to_string(),
            TagValue::Rational { numerator, denominator } => format!("{}/{}", numerator, denominator),
            TagValue::Binary(bytes) => format!("(Binary, {} bytes)", bytes.len()),
            TagValue::DateTime(dt) => dt.to_rfc3339(),
            TagValue::Struct(_) => "(Structured data)".to_string(),
        }
    }
    ```
    This provides clean, human-readable output for each type.

### Summary of Required Changes

1. **src/cli/output_formatter.rs**: Implement trait + 2 structs + unit tests (main work - ~150-200 lines)
2. **src/cli/mod.rs**: Add `pub mod output_formatter;` (1 line change)
3. **src/main.rs**: Refactor formatting logic to use new trait (replace ~20 lines with formatter calls)

All dependencies are already in Cargo.toml (serde_json), and all input types (MetadataMap, CliArgs, TagValue) are already implemented and working perfectly.

### Example Refactored main.rs Logic

After implementing the formatters, your main.rs should look something like this:

```rust
// ... existing imports ...
use exiftool_rs::cli::output_formatter::{OutputFormatter, HumanReadableFormatter, JsonFormatter};

fn main() {
    let args = CliArgs::parse();

    // ... existing read_metadata call ...

    match read_metadata(&args.file) {
        Ok(metadata) => {
            if metadata.is_empty() {
                println!("No metadata found in file: {}", args.file.display());
                return;
            }

            // Select formatter based on CLI args
            let output = if args.json {
                let formatter = JsonFormatter;
                formatter.format(&metadata, None)
            } else {
                let formatter = HumanReadableFormatter;
                // Optional: add header info
                let mut output = format!("File: {}\nFound {} metadata tag(s):\n\n",
                                        args.file.display(), metadata.len());
                output.push_str(&formatter.format(&metadata, None));
                output
            };

            println!("{}", output);
        }
        Err(e) => {
            // ... existing error handling ...
        }
    }
}
```

### Key Architectural Constraints

1. **Trait-based abstraction** - Use the OutputFormatter trait to allow for future format extensions (CSV, XML, etc.)
2. **Maintain separation of concerns** - Formatters only handle presentation, no business logic
3. **Leverage existing serialization** - MetadataMap and TagValue already have Serialize derives
4. **Preserve current behavior** - Output format should match existing main.rs output for backwards compatibility
5. **Library-friendly design** - Make formatters public and reusable by library consumers

### Acceptance Criteria Checklist

After implementation, verify these criteria are met:

- [ ] `cargo test output_formatter` passes with all unit tests
- [ ] HumanReadableFormatter outputs "Tag: Value\n" format (e.g., "EXIF:Make: Canon\n")
- [ ] HumanReadableFormatter displays clean values (not enum variants like "String(...)")
- [ ] HumanReadableFormatter sorts tags alphabetically
- [ ] JsonFormatter produces valid JSON (test with `serde_json::from_str()` in tests)
- [ ] JsonFormatter output can be piped to `jq` successfully (manual test)
- [ ] Both formatters handle empty MetadataMap gracefully (no panic, return empty string or minimal output)
- [ ] Tag filtering works when Option<&[String]> is Some (only specified tags in output)
- [ ] main.rs uses formatters instead of inline formatting logic
- [ ] CLI behavior is unchanged from user perspective (same output format)
- [ ] All code has proper documentation comments
- [ ] No compiler warnings when running `cargo clippy`
