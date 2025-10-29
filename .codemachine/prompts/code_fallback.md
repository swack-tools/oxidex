# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

Create integration test in tests/integration/jpeg_tests.rs that demonstrates end-to-end workflow: (1) Use MMapReader to open sample JPEG file (create sample with EXIF in tests/fixtures/jpeg/), (2) Detect format using format_detector, (3) Parse JPEG segments, (4) Parse EXIF IFD from APP1 segment, (5) Extract at least 3 tag values (Make, Model, DateTime), (6) Print extracted values. This test validates the entire parsing pipeline from I1.T8-T11. Test should pass.

---

## Issues Detected

*   **File Location Error:** The integration test file was created at `tests/jpeg_tests.rs` but the task description explicitly requires it to be at `tests/integration/jpeg_tests.rs`. The test must be in the `integration` subdirectory.

---

## Best Approach to Fix

You MUST move the test file from `tests/jpeg_tests.rs` to `tests/integration/jpeg_tests.rs`. Use the following steps:

1. Create the `tests/integration/` directory if it doesn't exist (use `mkdir -p tests/integration`)
2. Move the file: `mv tests/jpeg_tests.rs tests/integration/jpeg_tests.rs`
3. Verify the test still runs with: `cargo test --test jpeg_tests` or `cargo test jpeg_tests`

The test code itself is correct and passes all acceptance criteria - it just needs to be in the correct location as specified in the task description.
