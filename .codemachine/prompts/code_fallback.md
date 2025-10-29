# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

Task I3.T5: Extend CLI args in src/cli/args.rs to support tag modification: -TAG_NAME=VALUE syntax (e.g., -EXIF:Artist=John Doe). Parse modification arguments, call modify_tag() from I3.T4. Support multiple modifications in one command (e.g., exiftool-rs -EXIF:Artist=John -EXIF:Copyright=2025 photo.jpg). Update main.rs to handle write operations. Add validation that file is writable. Print success/failure message.

**Acceptance Criteria:**
- exiftool-rs -EXIF:Artist=John Doe photo.jpg modifies tag
- Multiple modifications work: -Tag1=Val1 -Tag2=Val2
- Prints success message: 1 image file updated
- Prints error on validation failure: Invalid value for TAG
- Verifies file exists and is writable before modification
- Manual test: modify tag, re-run with read, verify change

---

## Issues Detected

**Critical Bug in Core Operations Module:** The write operations are failing due to a type mismatch in the validation layer. Specifically:

*   **Root Cause:** When `read_metadata()` parses EXIF DateTime tags from files, it stores them as `TagValue::String` (e.g., "2025:01:15 10:30:00"). However, when `write_metadata()` validates ALL tags in the metadata map before writing, the validation function expects DateTime tags to be `TagValue::DateTime` type, not `TagValue::String`.

*   **Error Message:** "Invalid value for EXIF:Artist: Invalid value for tag 'EXIF:DateTime': Type mismatch: expected DateTime but got String"

*   **Location:** The issue occurs in `src/core/operations.rs` at the validation phase (lines 402-410) when `write_metadata()` iterates through all tags and calls `validate_tag_value()`.

*   **Impact:** This bug blocks ALL write operations, including the CLI tag modification feature implemented in I3.T5. The CLI implementation itself is correct, but it cannot work until this core bug is fixed.

*   **Test Case That Fails:**
    ```bash
    ./target/release/exiftool-rs -EXIF:Artist="John Doe" /tmp/test_photo.jpg
    # Expected: "1 image files updated"
    # Actual: "Error: Invalid value for EXIF:Artist: Invalid value for tag 'EXIF:DateTime': Type mismatch: expected DateTime but got String"
    ```

---

## Best Approach to Fix

You MUST fix the type conversion issue in the metadata read/write pipeline. There are two possible approaches:

### Approach 1: Fix the Reader (Recommended)

Modify the `raw_bytes_to_tag_value()` function in `src/core/operations.rs` (starting at line 262) to detect DateTime strings and convert them to `TagValue::DateTime` type during reading. This requires:

1. Add logic to detect DateTime format strings (YYYY:MM:DD HH:MM:SS)
2. Parse them using `chrono::DateTime::parse_from_str()`
3. Return `TagValue::DateTime` instead of `TagValue::String` for these cases

### Approach 2: Relax the Validator

Modify the `validate_tag_value()` function in `src/core/validation.rs` to accept `TagValue::String` when the expected type is `ValueType::DateTime`, and automatically convert string representations of DateTime to proper DateTime objects during validation. This approach is more lenient but may hide issues.

**I recommend Approach 1** because it ensures type correctness throughout the pipeline and prevents similar issues in the future.

### Implementation Details for Approach 1:

In `src/core/operations.rs`, modify the `raw_bytes_to_tag_value()` function around line 294-305:

```rust
// Try to interpret as ASCII string (null-terminated)
if bytes
    .iter()
    .all(|&b| (32..=126).contains(&b) || b == 0 || b == b'\n' || b == b'\r' || b == b'\t')
{
    // Convert to string, removing null terminator
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim_end_matches('\0');
    if !s.is_empty() {
        // Check if this is a DateTime string (YYYY:MM:DD HH:MM:SS format)
        if is_datetime_string(&s) {
            // Parse and return as DateTime type
            if let Ok(dt) = parse_exif_datetime(&s) {
                return TagValue::DateTime(dt);
            }
        }
        return TagValue::new_string(s.to_string());
    }
}
```

You will also need to add helper functions:

```rust
fn is_datetime_string(s: &str) -> bool {
    // EXIF DateTime format: YYYY:MM:DD HH:MM:SS (19 characters)
    s.len() == 19 && s.chars().filter(|&c| c == ':').count() == 4 && s.chars().filter(|&c| c == ' ').count() == 1
}

fn parse_exif_datetime(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    use chrono::NaiveDateTime;
    // EXIF format: "2025:01:15 10:30:00"
    let naive = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .map_err(|e| ExifToolError::parse_error(format!("Invalid DateTime: {}", e), 0))?;
    Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc))
}
```

Make sure to add the necessary imports at the top of the file:
```rust
use chrono::{DateTime, NaiveDateTime, Utc};
```

### Testing After Fix:

After implementing the fix, verify that:
1. `cargo test` passes all tests
2. `cargo clippy` has no warnings
3. Manual test succeeds:
   ```bash
   cp tests/fixtures/jpeg/sample_with_exif.jpg /tmp/test.jpg
   ./target/release/exiftool-rs -EXIF:Artist="John Doe" /tmp/test.jpg
   # Should output: "1 image files updated"
   ./target/release/exiftool-rs /tmp/test.jpg | grep Artist
   # Should show: "EXIF:Artist: John Doe"
   ```
