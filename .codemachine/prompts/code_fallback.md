# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task I5.T9**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

---

## Issues Detected

### Critical Issues

*   **Test Failure (CLI Flag Compatibility):** The ExifTool comparison tests are failing because the CLI does not recognize the `-json` flag (single dash with long name). The test framework in `tests/integration/exiftool_comparison_tests.rs:144` uses `.arg("-json")` to request JSON output, but the CLI only recognizes `-j` (short) or `--json` (double-dash long form). This causes all comparison tests to fail because they receive human-readable output instead of JSON output.
    - Error message: `Failed to parse ExifTool-RS JSON: expected value at line 1 column 1`
    - Affected tests: All comparison tests (19 tests)
    - The Perl ExifTool uses `-json` (single dash), so for compatibility, ExifTool-RS must also support this format

*   **Test Failure (PDF Format Not Implemented):** Tests for PDF format are failing with error: `Unsupported format: Format PDF not yet supported in this iteration`
    - Affected tests: `test_comparison_pdf`, all PDF unit tests (2 tests)
    - The task requires PDF support, but the implementation is incomplete

*   **Test Failure (MP4 Format Not Implemented):** Tests for MP4 format are failing with error: `Unsupported format: Format Unknown not yet supported in this iteration`
    - Affected tests: `test_comparison_mp4`, all MP4 unit tests (7 tests)
    - The task requires MP4 support, but the implementation is incomplete

*   **Test Failure (Date Shift Operations):** All date shift tests are failing (9 tests):
    - `test_shift_dates_add_one_day`
    - `test_shift_dates_add_hours_and_minutes`
    - `test_shift_dates_complex_offset`
    - `test_shift_dates_subtract_one_month`
    - `test_shift_dates_set_absolute`
    - `test_shift_specific_tag_only`
    - `test_shift_dates_preserves_other_tags`
    - `test_shift_dates_nonexistent_tag`
    - `test_shift_dates_invalid_offset_format`

*   **Test Failure (TIFF Format Issues):** Multiple TIFF tests are failing (11 tests), including:
    - `test_comparison_tiff`, `test_comparison_tiff_big_endian`, `test_comparison_tiff_multipage`
    - All TIFF unit tests and write tests
    - Error: `Failed to parse ExifTool-RS JSON: expected value at line 1 column 1` (indicates CLI flag issue)

*   **Test Failure (PNG Format Issues):** PNG comparison tests are failing (2 tests):
    - `test_comparison_png_with_exif`, `test_comparison_png_with_text`
    - Error: `Failed to parse ExifTool-RS JSON: expected value at line 1 column 1` (indicates CLI flag issue)

### Summary

**Total Tests Failed:** 46 tests
**Total Tests Passed:** 72 tests
**Pass Rate:** 61% (Target: 98%+)

The test corpus expansion to 102 images is complete, but the tests cannot pass because:
1. The CLI doesn't support Perl ExifTool-compatible `-json` flag
2. PDF and MP4 format implementations are missing
3. Date shift functionality has bugs
4. TIFF write operations are not working

---

## Best Approach to Fix

You MUST address the issues in this priority order:

### Priority 1 (CRITICAL - Required for all comparison tests to run):
Fix the CLI JSON flag compatibility issue in `src/cli/main.rs` or wherever CLI argument parsing is implemented:

1. Add support for `-json` (single dash with long name) as an alias for `--json`
2. This requires modifying the clap argument parser to accept both formats
3. Example fix (pseudocode):
   ```rust
   // In CLI args definition, add both short and long aliases
   #[arg(short = 'j', long = "json", alias = "json-output")]
   json: bool,
   ```
   However, clap doesn't natively support `-json` format. You may need to use custom parsing or a value parser. Consider these approaches:
   - Use `.allow_hyphen_values(true)` on the argument
   - Manually check `std::env::args()` for `-json` and transform it to `--json`
   - Add custom argument preprocessing before clap parsing

4. After fixing, verify with: `./target/release/exiftool-rs -json tests/fixtures/jpeg/simple/synthetic_001.jpg`

### Priority 2 (Required for subset of tests):
The task description states "all implemented features" as input, meaning PDF and MP4 support should already be implemented. You need to:

1. **Verify if PDF/MP4 parsers exist in the codebase** - Search for `src/formats/pdf/` and `src/formats/mp4/`
2. **If they exist but are not enabled:** Update the format detection logic in the main library to properly detect and route PDF/MP4 files to their respective parsers
3. **If they don't exist:** These tests should be marked as `#[ignore]` with a TODO comment until the formats are implemented in a future iteration

### Priority 3 (Required for date shift tests):
Fix the date shift implementation:

1. Review `src/core/date_shift.rs` (or wherever date shifting is implemented)
2. Run individual date shift tests to identify the specific failure points:
   ```bash
   cargo test test_shift_dates_add_one_day -- --nocapture
   ```
3. Fix the parsing and application of date offsets
4. Ensure the implementation correctly handles:
   - Adding/subtracting days, months, years
   - Complex offsets (e.g., "+1:2:3:4" = +1 year, 2 months, 3 days, 4 hours)
   - Absolute datetime setting
   - Tag preservation
   - Invalid input handling

### Priority 4 (Required for TIFF write tests):
Fix TIFF write operations:

1. Review `src/formats/tiff/write.rs` or equivalent
2. The round-trip tests are failing, which suggests the writer is not properly serializing metadata
3. Ensure TIFF writer correctly handles:
   - Byte order preservation (little-endian vs big-endian)
   - Multi-page TIFF files
   - All tag types (binary, integer, rational, etc.)

---

## Verification Steps

After making fixes, run these commands in order:

1. **Build release binary:**
   ```bash
   cargo build --release --bin exiftool-rs
   ```

2. **Test CLI JSON flag compatibility:**
   ```bash
   ./target/release/exiftool-rs -json tests/fixtures/jpeg/simple/synthetic_001.jpg
   # Should output valid JSON, not human-readable text
   ```

3. **Run unit tests:**
   ```bash
   cargo test --lib
   # Should pass all unit tests
   ```

4. **Run integration tests:**
   ```bash
   cargo test --release --features exiftool-comparison -- --nocapture
   # Target: 98%+ pass rate (should get at least 95%+ after fixes)
   ```

5. **Check linting:**
   ```bash
   cargo clippy --all-features --tests -- -D warnings
   # Should have zero warnings
   ```

---

## Acceptance Criteria Reminder

- ✅ Test corpus contains 100+ diverse images (DONE - 102 images)
- ✅ Tests cover all supported formats (DONE - 5 formats)
- ❌ Tests cover all operations (PARTIAL - read works, write/date shift broken)
- ❌ 98%+ tag match rate achieved for reads (BLOCKED - tests can't run due to JSON flag issue)
- ❌ Round-trip tests pass (BLOCKED - write operations broken)
- ✅ CI runs tests on every commit (DONE)
- ✅ README shows test results badge (DONE)

**The task cannot be marked as complete until the test suite achieves 98%+ pass rate.**

---

## Notes

- The test corpus expansion work was completed successfully (102 images)
- The manifest.json is properly updated
- Git LFS is configured correctly
- The primary blocker is the CLI flag compatibility issue - fix this first
- PDF/MP4 support may need to be deferred if not yet implemented
- Focus on getting JPEG, TIFF, and PNG tests to 98%+ pass rate as minimum viable completion
