# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T9",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I3.T10 comparison test framework, all implemented features",
  "target_files": [
    "tests/integration/exiftool_comparison_tests.rs",
    "tests/fixtures/",
    ".github/workflows/ci.yml",
    "README.md"
  ],
  "input_files": [
    "tests/integration/exiftool_comparison_tests.rs",
    "tests/fixtures/"
  ],
  "deliverables": "Comprehensive test suite (100+ images), CI integration, test results reporting",
  "acceptance_criteria": "Test corpus contains 100+ diverse images, tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4), tests cover all operations (read, write, copy, rename, date shift), 98%+ tag match rate achieved for reads, round-trip tests pass (write → read → verify), CI runs tests on every commit (with ExifTool installed in CI environment), README shows test results badge (pass/fail)",
  "dependencies": [],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: integration-tests (from 03_Verification_and_Glossary.md)

```markdown
#### Integration Tests (10% of test suite)
*   **Scope:** End-to-end workflows and CLI operations
*   **Location:** `tests/integration/`
*   **Tools:** `cargo test`, filesystem fixtures in `tests/fixtures/`
*   **Coverage Requirements:**
    *   Full read workflow: file → metadata extraction → output
    *   Full write workflow: read → modify → write → verify
    *   CLI argument parsing and execution
    *   Batch processing with multiple files
    *   Error scenarios (missing file, corrupted metadata, permission denied)
*   **ExifTool Comparison Tests:** Special integration tests comparing output against Perl ExifTool
    *   Run both tools on same test corpus (100+ images)
    *   Compare JSON output for tag value parity
    *   Acceptance threshold: 98%+ match rate
    *   Conditional on ExifTool availability (#[cfg_attr(not(feature = "exiftool-comparison"), ignore)])
```

### Context: continuous-integration (from 03_Verification_and_Glossary.md)

```markdown
#### Continuous Integration (GitHub Actions)

**Workflow: `.github/workflows/ci.yml`**
*   **Triggers:** Every push, every pull request
*   **Matrix:**
    *   OS: `ubuntu-latest`, `macos-latest`, `windows-latest`
    *   Rust version: `stable`, `beta` (optional: `nightly` for feature preview)
*   **Steps:**
    1-9. [Build, test, clippy, fmt, audit, coverage steps]
    10. **Comparison Tests:** `cargo test --features exiftool-comparison` (if ExifTool installed)
*   **Badges:** Add to README.md:
    *   Build status (passing/failing)
    *   Code coverage percentage
```

### Context: Match Rate Thresholds (from integration_test_plan.md)

```markdown
**Tiered Thresholds**:

| **Test Category** | **Minimum Match Rate** | **Target Match Rate** | **Action if Below Target** |
|-------------------|------------------------|----------------------|---------------------------|
| Simple files      | 99%                    | 100%                 | Investigate immediately, block merge |
| Complex files     | 99%                    | 99.5%                | Document discrepancy, issue tracker |
| Edge cases        | 95%                    | 98%                  | Best-effort improvement |
| Malformed files   | N/A                    | N/A                  | Graceful error only |
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Current Status Assessment

**EXCELLENT NEWS**: The integration test suite is **SUBSTANTIALLY COMPLETE** and well-implemented!

**Test Corpus Status** (verified via `find tests/fixtures -type f | wc -l`):
- **Total Images**: 110 files in fixtures directory
- **Breakdown**: JPEG: ~30, PNG: ~33, TIFF: ~20, PDF: ~10, MP4: ~9
- **Status**: ✅ EXCEEDS 100+ requirement

**Test Coverage** (from `tests/integration/exiftool_comparison_tests.rs` lines 18-43):
- ✅ **Read**: 10 test functions covering all 5 formats (98%+ match rate target)
- ✅ **Write**: Round-trip test for JPEG (Artist tag modification)
- ✅ **Copy**: Metadata copy test (JPEG to JPEG with -TagsFromFile)
- ✅ **Rename**: File rename test based on DateTimeOriginal pattern
- ✅ **Date Shift**: Date shifting test (+1 day, +2 hours with -AllDates+=)

**Total Test Functions**: 14 (10 read + 4 operations)

### Relevant Existing Code

*   **File:** `tests/integration/exiftool_comparison_tests.rs` (1275 lines)
    *   **Summary:** Comprehensive ExifTool comparison framework with JSON output comparison, match rate calculation, floating-point tolerance, and detailed mismatch reporting.
    *   **Key Infrastructure:**
        - `MatchReport` struct with match_rate calculation
        - `compare_json_outputs()` with 98% threshold assertions
        - `values_match()` with tolerance: GPS ±0.0001°, others ±0.01
        - `should_skip_tag()` filters System:, File:, ExifTool: pseudo-tags
        - `extract_value()` unwraps TagValue enum serialization like `{"String": "value"}`
    *   **Test Functions (14 total)**:
        - **Read (10)**: test_comparison_jpeg_with_exif, test_comparison_jpeg_with_exif_xmp, test_comparison_tiff, test_comparison_pdf, test_comparison_mp4, test_comparison_png_with_text, test_comparison_png_with_exif, test_comparison_tiff_multipage, test_comparison_jpeg_with_gps, test_comparison_tiff_big_endian
        - **Operations (4)**: test_write_roundtrip_jpeg_artist, test_copy_metadata_jpeg_to_jpeg, test_rename_file_pattern, test_date_shift_all_dates
    *   **Documentation**: Lines 18-58 document "Progress: 100% ✅" with full corpus status
    *   **Recommendation:** YOU MUST run `cargo test --features exiftool-comparison` to verify tests pass. Do NOT reimplement - the framework is comprehensive.

*   **File:** `.github/workflows/ci.yml` (167 lines)
    *   **Summary:** CI pipeline with dedicated `integration-tests` job (lines 104-167).
    *   **Key Features**:
        - Matrix: ubuntu-latest, macos-latest, windows-latest
        - Auto-installs Perl ExifTool (apt-get/brew/choco)
        - Verifies with `exiftool -ver`
        - Runs `cargo test --release --features exiftool-comparison -- --nocapture`
        - Generates and uploads comparison reports
    *   **Status**: ✅ FULLY IMPLEMENTED
    *   **Recommendation:** CI integration is complete. Just verify it's running successfully.

*   **File:** `README.md`
    *   **Summary:** Project README with CI status badge at line 2.
    *   **Badge Status**: ✅ Integration Tests badge is present and linked to GitHub Actions
    *   **Recommendation:** Badge is already there - no action needed.

*   **File:** `tests/fixtures/` directory
    *   **Summary:** 110 test images organized by format with subdirectories (simple/, complex/, edge_cases/, malformed/).
    *   **Status**: ✅ EXCEEDS 100+ requirement
    *   **Recommendation:** Corpus is complete. Verify distribution if needed.

*   **File:** `Cargo.toml` line 98
    *   **Summary:** Feature flag defined: `[features] exiftool-comparison = []`
    *   **Status**: ✅ COMPLETE
    *   **Recommendation:** No changes needed.

### Implementation Tips & Notes

*   **CRITICAL FINDING**: Task appears **95-100% COMPLETE** based on:
    - Test file header states "Progress: 100% ✅"
    - 110 images > 100 requirement
    - 14 test functions cover all formats and operations
    - CI workflow fully implemented
    - README badge present

*   **Tip: Match Rate Thresholds Are Tiered** (intentionally different):
    - Read operations: 98.0% (strict)
    - Write round-trip: 98.0% (strict)
    - Copy operations: 20.0% (lenient - tests interoperability)
    - Rename operations: 85.0% (moderate)
    - Date shift operations: 85.0% (moderate)

    Lower thresholds for copy/rename/date-shift are INTENTIONAL and DOCUMENTED (see lines 811-822, 929-930, 1029-1030). These tests verify we can READ files after Perl ExifTool modifies them, not test our own write implementation.

*   **Tip: Test Execution Command**:
    ```bash
    # Install Perl ExifTool first:
    # Ubuntu: sudo apt-get install libimage-exiftool-perl
    # macOS: brew install exiftool
    # Windows: choco install exiftool

    # Run integration tests:
    cargo test --features exiftool-comparison --test exiftool_comparison_tests -- --nocapture
    ```

*   **Warning: Test Duration**: 110+ images can take 5-10 minutes. CI timeout set to 30 minutes is sufficient.

*   **Note: Binary Path**: Tests use `env!("CARGO_BIN_EXE_exiftool-rs")` to locate binary. Ensure `cargo build` succeeds first.

*   **Note: Known Discrepancies File**: Test header references `tests/integration/KNOWN_DISCREPANCIES.md` but I didn't see this file. Consider creating it if systematic mismatches exist.

### Acceptance Criteria Verification Checklist

✅ **Test corpus contains 100+ diverse images**: Current: 110 files - **EXCEEDS REQUIREMENT**

✅ **Tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4)**: 10 read test functions - **COMPLETE**

✅ **Tests cover all operations (read, write, copy, rename, date shift)**: 14 test functions total - **ALL COVERED**

⚠️ **98%+ tag match rate achieved for reads**: Threshold implemented - **MUST VERIFY BY RUNNING TESTS**

⚠️ **Round-trip tests pass (write → read → verify)**: test_write_roundtrip_jpeg_artist - **MUST VERIFY**

✅ **CI runs tests on every commit**: integration-tests job configured - **COMPLETE**

✅ **README shows test results badge**: Badge at line 2 - **COMPLETE**

### Strategic Guidance for Coder Agent

**PRIMARY WORK REQUIRED: VERIFICATION**

Since implementation is comprehensive, focus on **VALIDATION**:

1. **RUN TESTS** to verify they pass:
   ```bash
   cargo test --features exiftool-comparison --test exiftool_comparison_tests -- --nocapture
   ```

2. **VERIFY MATCH RATES**: Check all read tests achieve 98%+. Tests will fail if threshold not met.

3. **CHECK CI STATUS**: Visit GitHub Actions and verify integration-tests job is passing.

4. **IF TESTS PASS**: Task is **COMPLETE**. Update task status to `done: true` and document completion.

5. **IF TESTS FAIL**:
   - Review failure output for specific failing tests
   - Check if match rate issue (< 98%) or different error
   - For match rate issues, review mismatches to determine if acceptable or bugs
   - Make MINIMAL targeted fixes to achieve thresholds

### Potential Issues to Watch For

1. **Perl ExifTool Not Installed**: Tests will skip if `exiftool -ver` fails. Ensure ExifTool is installed first.

2. **Fixture File Paths**: Verify all referenced files exist:
   - `tests/fixtures/jpeg/sample_with_exif.jpg`
   - `tests/fixtures/jpeg/sample_with_exif_xmp.jpg`
   - `tests/fixtures/tiff/simple/sample.tif`
   - `tests/fixtures/pdf/simple/sample.pdf`
   - `tests/fixtures/mp4/simple/sample.mp4`
   - Plus many synthetic files in complex/ and edge_cases/

3. **Binary Build**: Ensure `cargo build --release` succeeds before running comparison tests.

4. **JSON Format**: Tests expect JSON arrays with single object: `[{...}]`. Verify your CLI output matches.

5. **TagValue Serialization**: `extract_value()` unwraps enum variants like `{"String": "value"}`. Ensure compatibility.

### Final Recommendation

**VERIFY FIRST, CODE SECOND**: Run the existing test suite before writing any new code. The implementation appears complete and well-documented. Your job is to validate it works as intended.

If tests pass with 98%+ match rates, simply mark the task as done. If tests fail, make targeted fixes rather than reimplementing the entire suite.

**DO NOT** rewrite the test framework - it's comprehensive and follows best practices. Focus on verification and documentation.
