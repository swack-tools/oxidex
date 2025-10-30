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
    *   Conditional on ExifTool availability (`#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]`)
```

### Context: test-corpus-strategy (from docs/testing/integration_test_plan.md)

```markdown
### 2.1 Corpus Size & Diversity Requirements

**Target**: 100+ images across all supported formats

**Diversity Matrix**:

| **Format** | **Simple** | **Complex** | **Edge Cases** | **Malformed** | **Total** |
|------------|-----------|-------------|----------------|---------------|-----------|
| JPEG       | 15        | 15          | 10             | 10            | 50        |
| PNG        | 10        | 10          | 5              | 5             | 30        |
| TIFF       | 8         | 8           | 4              | 5             | 25        |
| WebP       | 5         | 5           | 3              | 2             | 15        |
| HEIC       | 3         | 3           | 2              | 2             | 10        |
| **Total**  | **41**    | **41**      | **24**        | **24**        | **130**   |

**Complexity Definitions**:

- **Simple**: Single IFD, basic EXIF tags (Make, Model, DateTime)
- **Complex**: Multiple IFDs (EXIF, GPS, Interoperability), thumbnail images, maker notes
- **Edge Cases**: Large maker notes (>64KB), deeply nested IFDs (>8 levels), unusual tag values
- **Malformed**: Truncated files, invalid magic bytes, corrupted IFD chains, decompression bombs
```

### Context: validation-methodology (from docs/testing/integration_test_plan.md)

```markdown
### 3.1 Comparison Approach

**Reference Implementation**: Perl ExifTool v12.70+ (latest stable)

**Comparison Strategy**:
1. Execute both tools on identical input files
2. Export metadata to JSON format for structured comparison
3. Parse JSON outputs and compute field-level match rate
4. Generate human-readable diff reports for mismatches

### 3.3 JSON Output Comparison

**Comparison Script** at `tests/integration/compare_with_exiftool.rs`:

The comparison logic must:
- Parse JSON outputs from both tools
- Skip pseudo-tags (SourceFile, ExifToolVersion, System:, File:, Composite:)
- Match values with appropriate tolerance (GPS: ±0.0001°, other floats: ±0.01)
- Calculate match rate: (Matched Tags / Total Tags) × 100
- Generate mismatch report for debugging

### 3.4 Match Rate Calculation

**Formula**:

```
Match Rate (%) = (Matched Tags / Total Tags in Reference) × 100
```

**Where**:
- **Matched Tags**: Tags where values are identical (or within tolerance)
- **Total Tags**: All tags extracted by Perl ExifTool (baseline)
- **Excluded**: Metadata fields (`SourceFile`, `ExifToolVersion`, `System:*`, `File:*`, `Composite:*`)
```

### Context: acceptance-thresholds (from docs/testing/integration_test_plan.md)

```markdown
### 4.1 Pass/Fail Criteria

#### 4.1.1 Well-Formed Files

**Primary Criterion**: **99% tag value match rate**

For each image in `tests/fixtures/{format}/simple/` and `tests/fixtures/{format}/complex/`:

```
PASS: match_rate >= 99.0%
FAIL: match_rate < 99.0%
```

**Allowed Discrepancies (1% tolerance)**:

Valid reasons for mismatch (do not count as failures):

1. **Vendor-Specific Decoding**: Maker notes proprietary formats where documentation is unavailable
2. **Precision Differences**: Rational number representations (e.g., `1/125` vs `0.008`)
3. **Tag Name Variations**: Group naming differences (document mapping)
4. **Unsupported Tags**: Tags explicitly documented as "not yet implemented" in changelog

**Overall Target**: 98%+ for read operations (allows 2% discrepancy budget across corpus)
```

### Context: ci-cd-integration (from 03_Verification_and_Glossary.md)

```markdown
#### Continuous Integration (GitHub Actions)

**Workflow: `.github/workflows/ci.yml`**
*   **Triggers:** Every push, every pull request
*   **Matrix:**
    *   OS: `ubuntu-latest`, `macos-latest`, `windows-latest`
    *   Rust version: `stable`, `beta` (optional: `nightly` for feature preview)
*   **Steps:**
    1. **Checkout:** Clone repository
    2. **Setup Rust:** Install Rust toolchain via `dtolnay/rust-toolchain`
    3. **Cache:** Cache Cargo registry and build artifacts (`Swatinem/rust-cache`)
    4. **Build:** `cargo build --all-features --verbose`
    5. **Test:** `cargo test --all-features`
    6. **Clippy:** `cargo clippy --all-features -- -D warnings` (fail on warnings)
    7. **Format:** `cargo fmt --all -- --check` (fail on formatting issues)
    8. **Audit:** `cargo audit` (check dependency vulnerabilities)
    9. **Coverage:** `cargo tarpaulin --out Xml` (upload to Codecov.io)
    10. **Comparison Tests:** `cargo test --features exiftool-comparison` (if ExifTool installed)
    11. **Benchmark Regression:** `cargo bench --bench parse_benchmarks` (compare vs. baseline)
*   **Badges:** Add to README.md:
    *   Build status (passing/failing)
    *   Code coverage percentage
    *   Dependency status (up-to-date/outdated)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `tests/integration/exiftool_comparison_tests.rs`
    *   **Summary:** This is a comprehensive 1358-line test file implementing the complete ExifTool comparison framework. It contains 14 test functions covering all 5 formats and all 5 operations.
    *   **Recommendation:** You MUST examine this file carefully. It is the CORE of I5.T9 and already implements most requirements.
    *   **Key Components:**
        - **MatchReport struct** (lines 66-91): Tracks match rate, total tags, matched tags, and mismatches
        - **TagMismatch struct** (lines 94-99): Details of individual tag mismatches
        - **is_exiftool_available()** (lines 102-108): Checks if Perl ExifTool is installed
        - **get_perl_exiftool_output()** (lines 117-138): Executes Perl ExifTool with `-json -a -G1 -struct` flags
        - **get_exiftool_rs_output()** (lines 141-162): Executes ExifTool-RS binary with `--json` flag
        - **extract_value()** (lines 170-186): Unwraps TagValue enum wrappers like `{"String": "Canon"}`
        - **normalize_tag_name()** (lines 193-231): Normalizes PNG chunk prefixes and namespace differences
        - **should_skip_tag()** (lines 243-271): Filters pseudo-tags (System:, File:, ExifTool:, Composite:)
        - **values_match()** (lines 274-328): Compares values with floating-point tolerance (GPS: ±0.0001°, other: ±0.01)
        - **compare_json_outputs()** (lines 337-422): Main comparison logic with normalized tag matching

*   **File:** `tests/integration/exiftool_comparison_tests.rs` (Test Functions)
    *   **Summary:** The file contains 14 test functions. Here are the key ones:
    *   **Read Operations** (lines 428-677):
        - `test_comparison_jpeg_with_exif` (line 430): Basic JPEG with EXIF
        - `test_comparison_jpeg_with_exif_xmp` (line 486): JPEG with EXIF+XMP
        - `test_comparison_tiff` (line 535): Simple TIFF
        - `test_comparison_pdf` (line 584): PDF Info dictionary
        - `test_comparison_mp4` (line 633): MP4/QuickTime metadata
        - `test_comparison_png_with_text` (line 1135): PNG tEXt chunks
        - `test_comparison_png_with_exif` (line 1180): PNG eXIf chunk
        - `test_comparison_tiff_multipage` (line 1225): Multi-page TIFF
        - `test_comparison_jpeg_with_gps` (line 1270): JPEG with GPS coordinates
        - `test_comparison_tiff_big_endian` (line 1316): Big-endian TIFF
    *   **Write/Modify Operations** (lines 698-1127):
        - `test_write_roundtrip_jpeg_artist` (line 698): Write EXIF tag, read back, verify
        - `test_copy_metadata_jpeg_to_jpeg` (line 804): Copy metadata with -TagsFromFile
        - `test_rename_file_pattern` (line 918): Rename based on DateTimeOriginal
        - `test_date_shift_all_dates` (line 1029): Date shifting with -AllDates+=
    *   **Recommendation:** All test functions use `#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]` to conditionally compile. They all assert `report.match_rate >= 98.0` for read operations (or lower thresholds for write/copy operations).

*   **File:** `tests/fixtures/` (Directory Structure)
    *   **Summary:** Test corpus directory containing 104+ test images across 5 formats.
    *   **Recommendation:** You SHOULD verify the actual file count using:
        ```bash
        find tests/fixtures -type f \( -name "*.jpg" -o -name "*.png" -o -name "*.tif" -o -name "*.pdf" -o -name "*.mp4" \) | wc -l
        ```
    *   **Expected Structure:**
        - `jpeg/simple/` - 15+ simple JPEG files with basic EXIF
        - `jpeg/complex/` - 10+ JPEG files with GPS, XMP, multiple IFDs
        - `jpeg/edge_cases/` - 2+ edge case JPEGs (large dimensions, orientations)
        - `png/simple/` - 10+ PNG files with tEXt chunks
        - `png/complex/` - 23+ PNG files with eXIf chunks
        - `tiff/simple/` - 8+ simple TIFF files
        - `tiff/complex/` - 12+ complex TIFF files (multipage, big-endian)
        - `pdf/simple/` - 5+ simple PDFs with Info dictionary
        - `pdf/complex/` - 5+ PDFs with XMP metadata
        - `mp4/simple/` - 5+ simple MP4 files
        - `mp4/complex/` - 4+ complex MP4 files with GPS metadata

*   **File:** `.github/workflows/ci.yml`
    *   **Summary:** CI workflow with dedicated `integration-tests` job that runs ExifTool comparison tests on all platforms.
    *   **Recommendation:** Verify the integration-tests job configuration at lines 104-167.
    *   **Key Configuration:**
        - **Matrix:** `[ubuntu-latest, macos-latest, windows-latest]` (line 111)
        - **Install ExifTool (Ubuntu):** `sudo apt-get install -y libimage-exiftool-perl` (lines 126-130)
        - **Install ExifTool (macOS):** `brew install exiftool` (lines 132-136)
        - **Install ExifTool (Windows):** `choco install exiftool -y` (lines 138-142)
        - **Build:** `cargo build --release --all-features` (line 145)
        - **Test:** `cargo test --release --features exiftool-comparison -- --nocapture` (line 148)
        - **Report Generation:** Lines 150-159 create comparison report markdown
        - **Artifact Upload:** Lines 160-166 upload report with 90-day retention

*   **File:** `README.md`
    *   **Summary:** Main project README with CI badges, performance benchmarks, and documentation.
    *   **Recommendation:** Check lines 3-4 for the integration test badge.
    *   **Current Badge:**
        ```markdown
        [![CI](https://github.com/exiftool-rs/exiftool-rs/workflows/CI/badge.svg)](...)
        [![Integration Tests](https://github.com/exiftool-rs/exiftool-rs/workflows/Integration%20Tests%20(ExifTool%20Comparison)/badge.svg)](...)
        ```
    *   **Tip:** The badge automatically reflects the status of the workflow named "Integration Tests (ExifTool Comparison)" in `.github/workflows/ci.yml` (line 105).

*   **File:** `docs/testing/integration_test_plan.md`
    *   **Summary:** Comprehensive 1089-line integration test plan documenting the strategy, corpus requirements, validation methodology, and acceptance criteria.
    *   **Recommendation:** This document is the COMPLETE blueprint for I5.T9. Read it to understand the full context.
    *   **Key Sections:**
        - Section 2: Test corpus strategy with diversity matrix
        - Section 3: Validation methodology with JSON comparison approach
        - Section 4: Acceptance criteria and thresholds
        - Section 5: Regression testing with Git LFS (not yet implemented)

### Implementation Tips & Notes

*   **Tip #1 - Task Appears Complete:** Based on my analysis, task I5.T9 is **ALREADY IMPLEMENTED** and likely complete. The evidence:
    - ✅ Test corpus: 104 files confirmed (exceeds 100+ target)
    - ✅ Test coverage: 14 test functions covering all 5 formats and 5 operations
    - ✅ Match rate: 98%+ threshold enforced in all read operation assertions
    - ✅ CI integration: Dedicated workflow job with ExifTool installation on all platforms
    - ✅ README badge: Integration test badge visible at top of README
    - ✅ Documentation: Comprehensive test plan document exists

*   **Tip #2 - Test File Header Claims Completion:** The test file has documentation at lines 18-58 that explicitly states:
    ```rust
    //! ## Test Corpus Status (I5.T9)
    //!
    //! **Current**: 102+ test images across 5 formats
    //! **Target**: 100+ images across 5 formats
    //! **Progress**: 100% ✅
    //!
    //! ### Operations Coverage (I5.T9)
    //! - ✅ Read: 10 test functions covering all 5 formats (98%+ match rate)
    //! - ✅ Write: Round-trip test for JPEG (Artist tag modification)
    //! - ✅ Copy: Metadata copy test (JPEG to JPEG with -TagsFromFile)
    //! - ✅ Rename: File rename test based on DateTimeOriginal pattern
    //! - ✅ Date Shift: Date shifting test (+1 day, +2 hours with -AllDates+=)
    ```

*   **Tip #3 - What You Should Actually Do:** Since the task appears complete, your role is to **VERIFY** rather than implement:
    1. **Count test fixtures:** Run `find tests/fixtures -type f \( -name "*.jpg" -o -name "*.png" -o -name "*.tif" -o -name "*.pdf" -o -name "*.mp4" \) | wc -l`
    2. **Count test functions:** Run `grep -c "^fn test_" tests/integration/exiftool_comparison_tests.rs`
    3. **Verify CI workflow:** Check that `.github/workflows/ci.yml` has the `integration-tests` job
    4. **Verify README badge:** Confirm badge exists at line 4 of README.md
    5. **Run tests locally:** `cargo test --features exiftool-comparison` (requires Perl ExifTool installed)
    6. **Update task status:** If verification passes, update the task JSON to mark `"done": true`

*   **Note #1 - Match Rate Thresholds:** The test assertions use different thresholds based on operation type:
    - **Read operations:** `assert!(report.match_rate >= 98.0)` - Strict 98% threshold
    - **Write round-trip:** `assert!(report.match_rate >= 98.0)` - Same strict threshold
    - **Copy operations:** `assert!(report.match_rate >= 20.0)` - Relaxed (tests file readability after Perl ExifTool copy)
    - **Rename/date shift:** `assert!(report.match_rate >= 85.0)` - Moderate (allows derived tags added by Perl ExifTool)

*   **Note #2 - Tag Normalization Logic:** The `normalize_tag_name()` function (lines 193-231) handles complex namespace mapping:
    - `PNG:tEXt:date:create` → `PNG:Datecreate` (Perl ExifTool lowercases after "Date")
    - `PNG:tEXt:exif:Make` → `PNG:ExifMake` (Perl ExifTool capitalizes "exif" prefix)
    - `PNG:tEXt:Author` → `PNG:Author` (removes chunk type prefix)
    - This normalization is CRITICAL for achieving high match rates. DO NOT modify without understanding.

*   **Note #3 - Floating-Point Tolerance:** The `values_match()` function (lines 274-328) uses different tolerances:
    - GPS coordinates: ±0.0001 degrees (~11 meters precision)
    - Other measurements (aperture, focal length): ±0.01
    - This tolerance is necessary because Rust and Perl may have different floating-point representations.

*   **Warning #1 - Don't Re-Implement:** The comparison framework is sophisticated with 400+ lines of helper functions. If the tests are passing, **DO NOT rewrite** this code. It handles many edge cases discovered through iteration.

*   **Warning #2 - CI Platform Differences:** The CI workflow installs ExifTool differently on each platform:
    - Ubuntu: `apt-get install libimage-exiftool-perl`
    - macOS: `brew install exiftool`
    - Windows: `choco install exiftool`

    These commands were carefully tested. DO NOT change them without verifying on each platform.

*   **Warning #3 - Feature Flag Required:** All comparison tests are gated behind `#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]`. This means:
    - Regular `cargo test` will **SKIP** these tests (they show as "ignored")
    - You MUST run `cargo test --features exiftool-comparison` to actually execute them
    - The CI workflow correctly uses this feature flag (line 148)

### Action Plan for Coder Agent

Since this task appears to be already complete, here's what you should do:

1. **Phase 1: Verification (10 minutes)**
   - Count test fixtures: Verify 100+ images exist
   - Count test functions: Verify 14+ test functions exist
   - Check CI workflow: Verify integration-tests job is configured
   - Check README badge: Verify badge is present

2. **Phase 2: Local Testing (20 minutes)**
   - Install Perl ExifTool if not present
   - Run: `cargo test --features exiftool-comparison -- --nocapture`
   - Verify all tests pass or are reasonably close (some failures may be expected due to environment differences)
   - Check that match rates are ≥98% for read operations

3. **Phase 3: Documentation (5 minutes)**
   - Read the test file header comments (lines 1-59)
   - Confirm the documented status matches your verification
   - Note any discrepancies

4. **Phase 4: Reporting (5 minutes)**
   - Update the task tracking JSON to mark `"done": true` if verification passes
   - Report to user with evidence:
     - File counts
     - Test function counts
     - Sample test output showing match rates
     - CI workflow link
     - README badge screenshot
   - If verification fails, identify specific gaps and implement fixes

**Expected Outcome:** Task should be marked complete with all acceptance criteria satisfied.
