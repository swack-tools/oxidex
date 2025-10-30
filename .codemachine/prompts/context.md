# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I4.T7",
  "iteration_id": "I4",
  "iteration_goal": "Add support for PDF and MP4/QuickTime formats, implement batch processing with recursive directory traversal and parallel execution, add metadata copying between files, and expand tag registry.",
  "description": "Implement date shifting feature in src/core/date_shift.rs. Support CLI syntax: exiftool-rs \"-AllDates+=1:30:0 0:0:0\" <files> to shift all date/time tags by specified offset (years:months:days hours:minutes:seconds). Parse offset string, apply to DateTime tags (EXIF:DateTime, EXIF:DateTimeOriginal, EXIF:CreateDate, XMP:CreateDate, etc.). Use chrono for date arithmetic. Support += (add), -= (subtract), = (set). Handle timezone if present. Add integration test.",
  "agent_type_hint": "BackendAgent",
  "inputs": "ExifTool date shifting syntax, I2.T3 read operations, I3.T4 write operations",
  "target_files": [
    "src/core/date_shift.rs",
    "src/cli/args.rs",
    "src/main.rs",
    "tests/integration/date_shift_tests.rs"
  ],
  "input_files": [
    "src/core/operations.rs"
  ],
  "deliverables": "Date/time shifting function, CLI support for -AllDates and specific date tags, integration test",
  "acceptance_criteria": "Parses offset format: years:months:days hours:minutes:seconds, applies offset to all DateTime tags with += or -=, sets absolute date/time with = operator, uses chrono for date arithmetic (handles month/year overflow), integration test: shift dates by +1 day, verify all date tags updated, cargo test date_shift passes",
  "dependencies": [
    "I2.T3",
    "I3.T4"
  ],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: task-i4-t7 (from 02_Iteration_I4.md)

```markdown
*   **Task 4.7: Implement Date/Time Shifting**
    *   **Task ID:** `I4.T7`
    *   **Description:** Implement date shifting feature in `src/core/date_shift.rs`. Support CLI syntax: `exiftool-rs "-AllDates+=1:30:0 0:0:0" <files>` to shift all date/time tags by specified offset (years:months:days hours:minutes:seconds). Parse offset string, apply to DateTime tags (EXIF:DateTime, EXIF:DateTimeOriginal, EXIF:CreateDate, XMP:CreateDate, etc.). Use `chrono` for date arithmetic. Support `+=` (add), `-=` (subtract), `=` (set). Handle timezone if present. Add integration test.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** ExifTool date shifting syntax, I2.T3 read operations, I3.T4 write operations
    *   **Input Files:** [`src/core/operations.rs`]
    *   **Target Files:**
        *   `src/core/date_shift.rs`
        *   `src/cli/args.rs` (add date shift arguments)
        *   `src/main.rs` (integrate date shifting)
        *   `tests/integration/date_shift_tests.rs`
    *   **Deliverables:**
        *   Date/time shifting function
        *   CLI support for -AllDates and specific date tags
        *   Integration test
    *   **Acceptance Criteria:**
        *   Parses offset format: "years:months:days hours:minutes:seconds"
        *   Applies offset to all DateTime tags with += or -=
        *   Sets absolute date/time with = operator
        *   Uses chrono for date arithmetic (handles month/year overflow)
        *   Integration test: shift dates by +1 day, verify all date tags updated
        *   `cargo test date_shift` passes
    *   **Dependencies:** `I2.T3`, `I3.T4`
    *   **Parallelizable:** Yes (can be developed in parallel with other I4 tasks)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/operations.rs`
    *   **Summary:** This file contains the core metadata operations including `read_metadata()`, `write_metadata()`, `modify_tag()`, and `copy_metadata()`. It also contains helper functions for parsing EXIF DateTime strings and converting tag values.
    *   **Recommendation:** You MUST import and use the `read_metadata()` and `write_metadata()` functions from this file. You SHOULD also use the existing `parse_exif_datetime()` function (lines 279-290) to parse EXIF DateTime strings into `chrono::DateTime<Utc>`.
    *   **Critical Detail:** You will need to create a complementary function to serialize DateTime values back to EXIF format. The EXIF DateTime format is "YYYY:MM:DD HH:MM:SS" (19 characters). You can use `chrono::format()` with the pattern `"%Y:%m:%d %H:%M:%S"`.
    *   **Key Functions to Reuse:**
        *   `read_metadata(path: &Path) -> Result<MetadataMap>` - reads metadata from a file (lines 64-80)
        *   `write_metadata(path: &Path, metadata: &MetadataMap) -> Result<()>` - writes metadata to a file (lines 443-487)
        *   `parse_exif_datetime(s: &str) -> Result<chrono::DateTime<Utc>>` - parses "YYYY:MM:DD HH:MM:SS" format (lines 279-290)
        *   `is_datetime_string(s: &str) -> bool` - validates EXIF DateTime format (lines 264-274)

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** Defines the `TagValue` enum with variants for String, Integer, Float, Rational, Binary, DateTime, and Struct. The DateTime variant uses `chrono::DateTime<Utc>`.
    *   **Recommendation:** You MUST use `TagValue::DateTime(chrono::DateTime<Utc>)` for all DateTime values. You can check if a value is DateTime using `value.is_datetime()` (line 111) and extract it using `value.as_datetime()` (lines 145-150) which returns `Option<&DateTime<Utc>>`.
    *   **Note:** The TagValue enum already has all necessary methods for working with DateTime values. You should use `TagValue::new_datetime(dt)` (lines 76-78) to create new DateTime values after shifting.

*   **File:** `src/cli/args.rs`
    *   **Summary:** Defines the CLI argument structure using `clap::Parser`. Contains methods for parsing tag modifications in the format `-TAG=VALUE`.
    *   **Recommendation:** You MUST extend this file to add support for date shifting arguments. The existing `parse_modification()` method (lines 96-116) handles `-TAG=VALUE` syntax. You will need to add similar parsing logic for date shift syntax like `-AllDates+=1:0:0 0:0:0` or `-EXIF:DateTime-=0:1:0 0:0:0`.
    *   **Pattern to Follow:** Look at how `tag_modifications()` (lines 81-94) and `parse_modification()` work. You should create analogous methods like `date_shift_modifications()` and `parse_date_shift()`.
    *   **Important:** The `args: Vec<String>` field (line 70) uses `trailing_var_arg = true`, which means all remaining arguments are captured. You'll need to parse date shift operations from this list.

*   **File:** `src/tag_db/tag_registry.rs`
    *   **Summary:** Contains the tag registry with 500+ tags. DateTime tags are identified with `ValueType::DateTime`.
    *   **Recommendation:** You SHOULD iterate through the metadata map and identify DateTime tags by checking if the tag value `is_datetime()`. For the `-AllDates` special tag, you need to identify all common DateTime tags.
    *   **Common DateTime Tags (from registry analysis):**
        *   `EXIF:DateTime` (tag 0x0132)
        *   `EXIF:DateTimeOriginal` (tag 0x9003)
        *   `EXIF:DateTimeDigitized` (tag 0x9004)
        *   `XMP:CreateDate`
        *   `XMP:ModifyDate`
        *   `PDF:CreateDate`
        *   `PDF:ModifyDate`
        *   `QuickTime:ContentCreateDate`
    *   **Tip:** The registry can be queried using `get_tag_descriptor(tag_name)` which returns `Option<&TagDescriptor>`. You can then check `descriptor.value_type == ValueType::DateTime`.

### Implementation Tips & Notes

*   **Tip:** The EXIF DateTime format is "YYYY:MM:DD HH:MM:SS" (19 characters). The existing `parse_exif_datetime()` function in `operations.rs` already handles this parsing. You will need to create a reverse function to format DateTime back to this string format when writing.

*   **Critical Implementation Detail:** The chrono crate is already imported and available. You should use:
    *   `chrono::Duration` for representing hours, minutes, seconds offsets
    *   `chrono::Months` for month offsets (handles month/year overflow correctly)
    *   DateTime arithmetic: `dt + duration`, `dt - duration`, `dt.checked_add_months()`, `dt.checked_sub_months()`
    *   For year offsets, multiply months by 12: `Months::new(years * 12)`

*   **Offset Format Specification:** The date offset format is "years:months:days hours:minutes:seconds". Example: "1:2:3 4:5:6" represents:
    *   1 year (12 months)
    *   2 months
    *   3 days
    *   4 hours
    *   5 minutes
    *   6 seconds
    *   **Total offset:** Add 14 months (1 year + 2 months), 3 days, 4 hours, 5 minutes, 6 seconds

*   **Operation Types:** The task requires three operation types:
    1. `+=` (add offset): Add the parsed offset to the DateTime
    2. `-=` (subtract offset): Subtract the parsed offset from the DateTime
    3. `=` (set absolute): Parse the value as an absolute date/time and replace the existing value
    *   **Parser Pattern:** Check if argument contains `+=`, `-=`, or `=` (in that order) to determine operation type

*   **Warning:** When implementing the CLI argument parsing, be aware that the existing `args` field uses `trailing_var_arg = true` which means all remaining arguments are captured. You'll need to carefully parse date shift arguments from this list before the file path.

*   **Tip:** For the integration test, you should:
    1. Create a test file with known DateTime tags (or use an existing fixture)
    2. Apply a date shift operation (e.g., +1 day with offset "0:0:1 0:0:0")
    3. Re-read the file
    4. Verify all DateTime tags have been shifted by exactly 1 day
    5. Use a simple offset to avoid month/year boundary issues in the basic test

*   **Note:** The task mentions handling timezone if present, but EXIF DateTime format doesn't include timezone information by default. The existing code treats all EXIF DateTimes as UTC (see `parse_exif_datetime` line 286-289). You should maintain this behavior. For XMP dates which may include timezone, you can use chrono's timezone-aware parsing.

*   **Recommended Module Structure for `src/core/date_shift.rs`:**
    1. Define an enum `ShiftOperation { Add, Subtract, Set }` for operation type
    2. Define a struct `DateOffset { years, months, days, hours, minutes, seconds }` for the offset
    3. Implement `fn parse_offset(s: &str) -> Result<DateOffset>` to parse "Y:M:D H:M:S" format
    4. Implement `fn apply_shift(dt: DateTime<Utc>, offset: &DateOffset, op: ShiftOperation) -> Result<DateTime<Utc>>`
    5. Implement `fn shift_metadata_dates(path: &Path, tag_pattern: &str, offset: &DateOffset, op: ShiftOperation) -> Result<()>`
    6. Add comprehensive unit tests for parsing and shifting logic

*   **Tip:** Look at the `copy_metadata()` function in `operations.rs` (lines 606-628) as a reference for how to:
    1. Read metadata from a file
    2. Iterate through the MetadataMap
    3. Modify specific tags (or filter based on criteria)
    4. Write the modified metadata back to the file

*   **AllDates Handling:** When the tag pattern is "AllDates", you should shift ALL DateTime tags in the file. You can identify these by:
    1. Iterating through all tags in the MetadataMap
    2. Checking if `tag_value.is_datetime()` returns true
    3. Applying the shift to those tags

*   **Error Handling:** Follow the existing pattern of returning `Result<T, ExifToolError>`. Common errors to handle:
    1. Invalid offset format (parse error)
    2. DateTime overflow (chrono checked operations return Option)
    3. File I/O errors (from read/write operations)
    4. Tag not found (if shifting a specific tag that doesn't exist)

### Testing Pattern

*   **Integration Test Structure:** Follow the pattern used in other integration tests (e.g., `write_operations_tests.rs`):
    1. Use `tempfile::NamedTempFile` to create a temporary test file
    2. Copy a fixture file or create test data
    3. Perform the date shift operation
    4. Re-read the file and verify the dates
    5. Cleanup happens automatically when NamedTempFile is dropped

*   **Test Cases to Implement:**
    1. **Basic shift (+1 day):** Verify dates increase by exactly 24 hours
    2. **Negative shift (-1 month):** Verify dates decrease by 1 month with correct overflow handling
    3. **AllDates shift:** Verify all DateTime tags in a file are shifted
    4. **Specific tag shift:** Verify only the specified tag is shifted
    5. **Set operation:** Verify absolute date setting works
    6. **Offset parsing:** Unit tests for valid and invalid offset formats

*   **Fixture Files:** You can use existing test fixtures from `tests/fixtures/jpeg/` that already have EXIF DateTime tags, or create new ones specifically for date shifting tests.

### ExifTool Date Shifting Syntax Reference

Based on the task description and ExifTool documentation:

```bash
# Add 1 year, 2 months, 3 days to all date tags
exiftool-rs "-AllDates+=1:2:3 0:0:0" photo.jpg

# Subtract 5 days from DateTimeOriginal only
exiftool-rs "-EXIF:DateTimeOriginal-=0:0:5 0:0:0" photo.jpg

# Set DateTime to a specific value (absolute)
exiftool-rs "-EXIF:DateTime=2025:01:15 10:30:00" photo.jpg

# Add 6 hours and 30 minutes to all dates
exiftool-rs "-AllDates+=0:0:0 6:30:0" photo.jpg
```

**Key Implementation Requirements:**

1. Parse the tag name (e.g., "AllDates", "EXIF:DateTime")
2. Parse the operation (+=, -=, =)
3. Parse the offset format "Y:M:D H:M:S"
4. Apply the operation to matching tags in the metadata
5. Write the modified metadata back to the file

### Suggested Implementation Phases

**Phase 1: Offset Parsing**
- Implement `DateOffset` struct to hold parsed values
- Implement `parse_offset()` to parse "Y:M:D H:M:S" format
- Add unit tests for valid and invalid formats

**Phase 2: Date Arithmetic**
- Implement `apply_shift()` to add/subtract offset from DateTime
- Use chrono's checked operations to handle overflow
- Add unit tests for date arithmetic edge cases

**Phase 3: CLI Argument Parsing**
- Extend `CliArgs` to recognize date shift arguments
- Parse tag pattern, operation, and offset from arguments
- Validate the parsed components

**Phase 4: Metadata Operations**
- Implement `shift_metadata_dates()` to:
  1. Read metadata from file
  2. Identify matching DateTime tags
  3. Apply shift to each matching tag
  4. Write modified metadata back
- Handle "AllDates" special case

**Phase 5: Integration**
- Update `src/main.rs` to detect date shift arguments
- Call date shift function when detected
- Add comprehensive integration tests

**Phase 6: Testing**
- Create test fixtures with known DateTime tags
- Verify shift accuracy for various offsets
- Test edge cases (month boundaries, leap years, etc.)

---

## 4. Final Checklist for the Coder Agent

Before you begin coding, ensure you understand:

- ✅ The offset format "years:months:days hours:minutes:seconds" with space separator
- ✅ The three operation types: `+=` (add), `-=` (subtract), `=` (set)
- ✅ How to use chrono for date arithmetic with `Duration` and `Months`
- ✅ The EXIF DateTime format "YYYY:MM:DD HH:MM:SS"
- ✅ How to identify DateTime tags using `TagValue::is_datetime()`
- ✅ The "AllDates" special pattern that matches all DateTime tags
- ✅ The existing `read_metadata()` and `write_metadata()` operations
- ✅ The project's error handling patterns using `Result<T, ExifToolError>`

**Critical Success Factors:**

1. **Correctness:** Date arithmetic must be precise, handling month/year overflow correctly
2. **Parsing:** Offset string parsing must be robust and validate input format
3. **Operations:** All three operation types (+=, -=, =) must work correctly
4. **AllDates:** The special "AllDates" pattern must shift all DateTime tags in the file
5. **Testing:** Integration tests must verify date shifts are applied accurately

**Common Pitfalls to Avoid:**

1. **Don't use simple Duration for months/years** - Use `chrono::Months` for month arithmetic
2. **Don't forget to format DateTime back to EXIF format** - Create a helper function for this
3. **Don't modify non-DateTime tags** - Only shift tags where `is_datetime()` returns true
4. **Don't ignore overflow** - Use checked arithmetic and return errors on overflow
5. **Don't forget the space separator** - Format is "Y:M:D H:M:S" (space between date and time)

Good luck! 🚀
