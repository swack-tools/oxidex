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

### Context: ci-cd-pipeline (from 03_Verification_and_Glossary.md)

```markdown
### 5.2. CI/CD Pipeline

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

### Context: release-criteria (from 03_Verification_and_Glossary.md)

```markdown
### 5.5. Release Criteria (v1.0)

The v1.0 release is approved when all of the following are met:

2. **Quality Metrics:**
   *   ✅ 80%+ code coverage
   *   ✅ 98%+ tag parity with ExifTool (comparison tests)
   *   ✅ 2x+ performance vs. ExifTool (benchmark validation)
   *   ✅ Zero crashes in 24-hour fuzz testing
   *   ✅ Zero clippy warnings
   *   ✅ Zero critical/high severity vulnerabilities (cargo audit)

5. **Testing:**
   *   ✅ All unit tests passing
   *   ✅ All integration tests passing
   *   ✅ Property-based tests passing (10,000+ cases)
   *   ✅ Comparison tests passing (100+ images)
   *   ✅ Manual testing on all target platforms
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `tests/integration/exiftool_comparison_tests.rs`
    *   **Summary:** This is a comprehensive, well-designed integration test framework that compares ExifTool-RS output against Perl ExifTool. It includes helper functions for running both tools, comparing JSON outputs, normalizing tag names, handling floating-point tolerances, and filtering pseudo-tags. The file contains 14 test functions covering read operations (10 tests) and write/copy/rename/date-shift operations (4 tests).
    *   **Current Status:** The test framework is **71.4% complete** (5/7 acceptance criteria met). The code quality is excellent, but **50% of tests are failing** (7/14). **CRITICAL**: The failures are NOT due to test framework issues - they are due to **incomplete parser implementations** in the production code.
    *   **Test Corpus:** Already has **104 images** (✅ exceeds 100+ requirement) across all 5 formats.
    *   **CI Integration:** Already fully configured in `.github/workflows/ci.yml` with a separate `integration-tests` job that installs Perl ExifTool on all platforms (Ubuntu, macOS, Windows).
    *   **Badges:** Already present in README (line 3-4).
    *   **Recommendation:** You MUST carefully read the detailed completion report at `tests/fixtures/I5T9_FINAL_SUMMARY.md` (already in the project) before taking any action. This 376-line document contains the complete analysis of what's working and what needs to be fixed.

*   **File:** `tests/fixtures/I5T9_FINAL_SUMMARY.md`
    *   **Summary:** Comprehensive completion report that documents the current state of task I5.T9. This file contains detailed test results, root cause analysis for all 7 failing tests, and specific implementation guides for fixing each issue.
    *   **Key Findings:**
        *   ✅ JPEG simple EXIF: 100% match rate (PASSING)
        *   ✅ JPEG with EXIF+XMP: 100% match rate (PASSING)
        *   ✅ PNG text chunks: 100% match rate (PASSING)
        *   ❌ JPEG with GPS: 42.11% match rate (CRITICAL - GPS parser missing)
        *   ❌ PNG with eXIf: 68.18% match rate (CRITICAL - eXIf/TIFF integration missing)
        *   ❌ TIFF files: 76-87% match rates (HIGH - missing baseline tags)
        *   ❌ PDF: 90.91% match rate (HIGH - 1 tag away from passing)
        *   ❌ MP4: 73.33% match rate (MEDIUM - QuickTime atoms incomplete)
    *   **Recommendation:** This file is your primary guide. Read it thoroughly before making any code changes.

*   **File:** `.github/workflows/ci.yml`
    *   **Summary:** GitHub Actions CI configuration with separate jobs for main tests, security audit, code coverage, and integration tests. The `integration-tests` job (lines 104-167) is already fully configured to install Perl ExifTool on all three platforms and run comparison tests.
    *   **Current Status:** Fully functional and running on every commit.
    *   **Recommendation:** No changes needed to CI configuration. The infrastructure is complete.

*   **File:** `README.md`
    *   **Summary:** Project README with badges, project description, architecture overview, and current status.
    *   **Current Status:** Already has two CI badges (lines 3-4): one for main CI and one for integration tests.
    *   **Recommendation:** No changes needed to README. The badges are already present and working.

### Implementation Tips & Notes

*   **CRITICAL FINDING:** Task I5.T9 is already **substantially complete**. The test framework, test corpus, CI integration, and badges are all in place and working correctly. The task acceptance criteria show **5/7 criteria met (71.4%)**:
    *   ✅ Test corpus: 104 images (exceeds 100+ requirement)
    *   ✅ Format coverage: All 5 formats tested
    *   ✅ Operation coverage: All 5 operations tested
    *   ✅ Round-trip tests: Passing
    *   ✅ CI integration: Fully configured and running
    *   ✅ README badge: Present and working
    *   ❌ 98%+ match rate: Only 3/10 read tests passing (30%)

*   **Root Cause Analysis:** The failing tests are NOT due to test infrastructure issues. The comprehensive report at `tests/fixtures/I5T9_FINAL_SUMMARY.md` identifies the exact production code issues:
    1. **GPS parser not implemented** (causes 42.11% match rate for GPS test)
    2. **PNG eXIf/TIFF integration missing** (causes 68.18% match rate for PNG+EXIF test)
    3. **TIFF baseline tags incomplete** (causes 76-87% match rates for TIFF tests)
    4. **PDF missing one field** (causes 90.91% match rate, very close to passing)
    5. **MP4 QuickTime atoms incomplete** (causes 73.33% match rate)

*   **What You Should Report to the User:** Inform the user that:
    1. Task I5.T9 is **substantially complete** (5/7 criteria met)
    2. The test framework, corpus, CI integration, and badges are all in place and working correctly
    3. The remaining issue (7/14 tests failing) is due to **incomplete parser implementations**, not test infrastructure
    4. A comprehensive completion report exists at `tests/fixtures/I5T9_FINAL_SUMMARY.md` with detailed analysis and fix instructions
    5. The user should review the report and decide on next steps for v1.0 release (3 options presented in report)

*   **Test Results Summary (from completion report):**
    ```
    Passing: 7/14 (50%)
    - 3 read tests passing at 100% match rate (JPEG simple/XMP, PNG text)
    - 4 operation tests passing at 98%+ match rate (write, copy, rename, date shift)

    Failing: 7/14 (50%)
    - GPS: 42.11% (CRITICAL)
    - PNG eXIf: 68.18% (CRITICAL)
    - TIFF: 76-87% (HIGH)
    - PDF: 90.91% (HIGH)
    - MP4: 73.33% (MEDIUM)
    ```

---

**End of Task Briefing Package**
