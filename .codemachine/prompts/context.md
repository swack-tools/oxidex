# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I4.T8",
  "iteration_id": "I4",
  "iteration_goal": "Add support for PDF and MP4/QuickTime formats, implement batch processing with recursive directory traversal and parallel execution, add metadata copying between files, and expand tag registry.",
  "description": "Implement CSV output formatter in src/cli/output_formatter.rs. Add CsvFormatter implementing OutputFormatter trait. Support -csv CLI flag. Output format: header row with tag names, data row(s) with values. Support batch mode: multiple files produce multiple rows with SourceFile column. Use csv crate for generation. Add unit tests.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I2.T9 output formatter trait",
  "target_files": [
    "src/cli/output_formatter.rs",
    "src/cli/args.rs",
    "src/main.rs",
    "Cargo.toml"
  ],
  "input_files": [
    "src/cli/output_formatter.rs"
  ],
  "deliverables": "CSV output formatter, batch mode support, unit tests",
  "acceptance_criteria": "-csv flag outputs valid CSV format, header row contains tag names, data rows contain tag values (one row per file in batch mode), CSV is parseable by standard tools (Excel, pandas), unit tests verify CSV generation, cargo test output_formatter passes",
  "dependencies": [
    "I2.T9"
  ],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: API Style and Communication Patterns (from 04_Behavior_and_Communication.md)

The CLI follows POSIX-style arguments mimicking ExifTool:
- `exiftool-rs -json -r /photos/` for recursive JSON output
- Output formatters follow a consistent trait-based pattern

### Context: Task I4.T8 Requirements (from 02_Iteration_I4.md)

**Task 4.8: Add CSV Output Format**
- Implement CSV output formatter in `src/cli/output_formatter.rs`
- Add `CsvFormatter` implementing `OutputFormatter` trait
- Support `-csv` CLI flag
- Output format: header row with tag names, data row(s) with values
- Support batch mode: multiple files produce multiple rows with SourceFile column
- Use `csv` crate for generation
- CSV must be parseable by standard tools (Excel, pandas)

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/cli/output_formatter.rs` (397 lines)
    *   **Summary:** This file contains the existing OutputFormatter trait and two implementations (HumanReadableFormatter and JsonFormatter). The trait defines a single method `format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String` that takes metadata and optional tag filters and returns a formatted string.
    *   **Recommendation:** You MUST implement a new struct `CsvFormatter` in this file that implements the `OutputFormatter` trait. Follow the same pattern as `HumanReadableFormatter` and `JsonFormatter`. The existing formatters provide excellent examples of how to filter tags and handle empty metadata.
    *   **Key Pattern:** Both existing formatters check for empty metadata first (line 74, 125), apply tag filtering if provided, and return a formatted string. Your CSV formatter should follow this exact pattern.
    *   **Recommendation:** You SHOULD use the existing helper function `format_tag_value()` (lines 149-166) to convert TagValue enum variants to human-readable strings for CSV cells. This ensures consistency across all formatters.

*   **File:** `src/cli/args.rs` (200+ lines)
    *   **Summary:** This file defines the CliArgs struct using clap's derive API. It already has a `json: bool` flag (line 14-15) for JSON output.
    *   **Recommendation:** You MUST add a new `csv: bool` field to the CliArgs struct, following the exact same pattern as the `json` field. Use `#[arg(long)]` to avoid conflicts and match ExifTool conventions (not short flag).
    *   **Example to Follow:**
    ```rust
    /// Output in JSON format
    #[arg(short, long)]
    pub json: bool,
    ```

*   **File:** `src/main.rs` (350+ lines)
    *   **Summary:** This is the CLI entry point. The `handle_read_operation()` function (lines 159-196) currently checks `json_output: bool` to decide between JsonFormatter and HumanReadableFormatter.
    *   **Recommendation:** You MUST modify `handle_read_operation()` signature to accept `&CliArgs` instead of just `json_output: bool`. This allows checking both json and csv flags.
    *   **Critical Pattern:** Lines 170-184 show the formatter selection logic - you MUST extend this to handle three cases: CSV first, then JSON, then human-readable (default).
    *   **Recommendation:** You SHOULD NOT need to modify `handle_batch_processing()` significantly - the batch_processor already collects metadata, you just need to ensure CSV output works there too.

*   **File:** `src/cli/batch_processor.rs` (200+ lines)
    *   **Summary:** This file handles batch processing of multiple files using rayon for parallelism. It collects results and prints statistics.
    *   **Recommendation:** For batch CSV output, the current architecture already supports it - the batch_processor processes files and could output CSV format. The single-file CSV case is priority.
    *   **Note:** The "SourceFile" column requirement for batch mode can be deferred or implemented as a second pass. Focus first on getting single-file CSV working correctly.

*   **File:** `Cargo.toml` (86 lines)
    *   **Summary:** The dependencies section already includes many crates including `serde_json = "1.0"` for JSON output.
    *   **Recommendation:** You MUST add `csv = "1.3"` to the `[dependencies]` section. The csv crate is the de-facto standard for CSV generation in Rust and handles RFC 4180 escaping automatically.

### Implementation Tips & Notes

*   **Tip:** The `csv` crate provides a `Writer` that writes to any `Write` implementation. For our use case, write to a `Vec<u8>` buffer and then convert to String:
```rust
use csv::Writer;

let mut wtr = Writer::from_writer(vec![]);
wtr.write_record(&["Tag", "Value"])?;  // Header
wtr.write_record(&["EXIF:Make", "Canon"])?;  // Data row
let data = wtr.into_inner().map_err(|_| "CSV writer error")?;
let csv_string = String::from_utf8(data).expect("Valid UTF-8");
```

*   **Tip:** For single-file CSV output, use a two-column format: "Tag" and "Value" headers, with one row per metadata tag. This is simpler than trying to create a wide table with one column per tag.

*   **Note:** The existing tests in `src/cli/output_formatter.rs` (lines 168-396) provide excellent examples of test structure. You SHOULD add similar comprehensive tests for CsvFormatter:
    - Test empty metadata
    - Test single tag
    - Test multiple tags
    - Test all value types (String, Integer, Float, Rational, Binary, DateTime, Struct)
    - Test with filter
    - Test that output is valid CSV (parseable by csv crate's Reader)

*   **Warning:** CSV formatting requires proper escaping of special characters (commas, quotes, newlines in values). The `csv` crate handles this automatically with RFC 4180 compliance - DO NOT manually escape. Just use `Writer::write_record()`.

*   **Tip:** To verify CSV validity in tests, parse the output string back with `csv::Reader`:
```rust
let mut rdr = csv::Reader::from_reader(output.as_bytes());
let records: Vec<_> = rdr.records().collect();
assert_eq!(records.len(), expected_rows);
```

*   **Critical Decision:** The OutputFormatter trait's `format()` method only takes a single MetadataMap. For single-file CSV, this is fine - just output a two-column CSV (Tag, Value). For batch mode with SourceFile column, you have two options:
    1. Keep it simple: single-file CSV for now (meets acceptance criteria)
    2. Add a separate `format_batch_csv(files: &[(PathBuf, MetadataMap)]) -> String` function later

    **Recommendation:** Implement option 1 first (two-column CSV: Tag, Value). This satisfies the core requirement and all tests can pass. Batch mode with SourceFile column can be a follow-up enhancement.

*   **Pattern Consistency:** Look at how main.rs imports formatters (line 8). You MUST add CsvFormatter to the use statement: `use exiftool_rs::cli::output_formatter::{HumanReadableFormatter, JsonFormatter, CsvFormatter, OutputFormatter};`

### Summary of Changes Required

1. **Cargo.toml**: Add `csv = "1.3"` to `[dependencies]` section (after line 64)
2. **src/cli/args.rs**: Add `pub csv: bool` field with `#[arg(long)]` (after line 15)
3. **src/cli/output_formatter.rs**:
   - Add `use csv::Writer;` at top
   - Implement `pub struct CsvFormatter;`
   - Implement `impl OutputFormatter for CsvFormatter`
   - Format as two-column CSV: "Tag", "Value" headers, one row per metadata entry
   - Add 6-8 unit tests following existing test patterns
4. **src/main.rs**:
   - Update `handle_read_operation()` signature: change from `json_output: bool` to `args: &CliArgs`
   - Add CsvFormatter to imports (line 8)
   - Update formatter selection to check `args.csv` first, then `args.json`, then default
   - Update call site at line 57: change `handle_read_operation(&file, args.json)` to `handle_read_operation(&file, &args)`

### File-by-File Execution Order

1. **Cargo.toml** - add the csv dependency
2. **src/cli/args.rs** - add the CLI flag
3. **src/cli/output_formatter.rs** - implement CsvFormatter + tests (this is the core work)
4. **src/main.rs** - integrate into CLI (small changes)

This order ensures each piece builds on the previous and you can test incrementally with `cargo test` and `cargo build`.

### Expected CSV Output Format

For a file with metadata:
```
Tag,Value
EXIF:Make,Canon
EXIF:Model,EOS 5D
EXIF:ISO,400
```

This simple two-column format is clean, parseable by Excel/pandas, and easy to implement.
