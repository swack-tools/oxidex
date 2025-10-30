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
*   **Examples:**
    ```rust
    #[test]
    fn test_cli_extract_jpeg_exif() {
        let output = Command::new("./target/debug/exiftool-rs")
            .arg("tests/fixtures/jpeg/sample.jpg")
            .output()?;
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)?.contains("EXIF:Make"));
    }

    #[test]
    #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
    fn compare_against_exiftool_jpeg() {
        let exiftool_json = get_exiftool_output("sample.jpg")?;
        let our_json = get_exiftool_rs_output("sample.jpg")?;
        let match_rate = compare_json_outputs(&exiftool_json, &our_json);
        assert!(match_rate >= 0.98, "Match rate: {}", match_rate);
    }
    ```
```

### Context: testing-levels (from 03_Verification_and_Glossary.md)

```markdown
### 5.1. Testing Levels

The project employs a comprehensive testing pyramid to ensure correctness and reliability:

#### Unit Tests (70% of test suite)
*   **Scope:** Individual functions and modules
*   **Location:** Inline in source files (`#[cfg(test)] mod tests`) and `tests/` directory
*   **Tools:** `cargo test`, standard Rust test framework
*   **Coverage Requirements:**
    *   All parser functions (format detection, segment parsing, IFD parsing, tag extraction)
    *   Data model operations (metadata map accessors, tag value conversions)
    *   Validation logic (tag value type checking, constraint validation)
    *   Error handling paths (parse errors, I/O errors, validation failures)
*   **Acceptance Criteria:** 80%+ line coverage (measured with `cargo-tarpaulin` or `cargo-llvm-cov`)
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
```

### Context: security-considerations (from 05_Operational_Architecture.md)

```markdown
#### Security Considerations

**Threat Model**:

ExifTool-RS processes potentially malicious files from untrusted sources (e.g., user uploads, scraped images). Primary threats:

1. **Memory Corruption**: Buffer overflows, use-after-free in parsers
2. **Resource Exhaustion**: Zip bombs, billion laughs (XML), decompression bombs
3. **Path Traversal**: Malicious filenames in archive processing
4. **Code Injection**: Via scripting features (if added)

**Mitigations**:

| **Threat** | **Mitigation** | **Implementation** |
|------------|---------------|-------------------|
| Buffer overflows | Rust ownership system | Compile-time prevention via borrow checker |
| Integer overflows | Checked arithmetic | `#![deny(overflowing_literals)]`, `checked_add()` in parsers |
| Resource exhaustion | Size limits | Max allocation: 1GB per file, max parse depth: 64 levels (nested IFDs) |
| Zip bombs | Decompression ratio check | Reject if uncompressed > 100x compressed size |
| XXE attacks (XML) | Disable external entities | `quick-xml` configured to reject DOCTYPE, external entities |
| Path traversal | Path sanitization | `canonicalize()` + jail to working directory for batch operations |
| Dependency vulnerabilities | Automated scanning | `cargo-audit` in CI, Dependabot alerts, minimal dependency tree |
| Malicious input | Fuzzing | Continuous fuzzing with `cargo-fuzz`, OSS-Fuzz integration target |
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `tests/integration/exiftool_comparison_tests.rs`
    *   **Summary:** This file contains the core comparison test framework with 3 existing tests (JPEG with EXIF, JPEG with EXIF+XMP, TIFF). It includes sophisticated comparison logic with tolerance for floating-point values, TagValue enum unwrapping, and detailed mismatch reporting.
    *   **Recommendation:** You MUST extend this file by adding additional test functions for the missing formats and operations. The existing infrastructure (MatchReport, compare_json_outputs, values_match) is well-designed and SHOULD be reused. Pay special attention to the TODO comments at the end of the file (lines 461-485) which list exactly what needs to be implemented.
    *   **Key Functions to Understand:**
        - `get_perl_exiftool_output()` - Executes Perl ExifTool with flags `-json -a -G1 -struct`
        - `get_exiftool_rs_output()` - Executes the compiled ExifTool-RS binary
        - `compare_json_outputs()` - Core comparison logic with 95% threshold
        - `values_match()` - Handles type mismatches, floating-point tolerance, nested structures
    *   **Important Details:** Tests are conditionally compiled with `#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]` and check for ExifTool availability at runtime.

*   **File:** `docs/testing/integration_test_plan.md`
    *   **Summary:** This is a comprehensive 1089-line integration testing plan that defines the exact test corpus requirements, validation methodology, acceptance criteria, and directory structure for test fixtures.
    *   **Recommendation:** You MUST follow this plan as the authoritative specification. It defines:
        - Test corpus: 130+ images across 5 formats (JPEG: 50, PNG: 30, TIFF: 25, WebP: 15, HEIC: 10)
        - Directory structure: `tests/fixtures/{format}/{simple|complex|edge_cases|malformed}/`
        - Match rate thresholds: 99% for simple/complex, 95% for edge cases
        - Comparison methodology: JSON output comparison with tolerance for GPS coordinates (±0.0001°) and other floats (±0.01)
    *   **Critical Sections:**
        - Section 2.2: Image Sourcing Strategy (use Exiv2 test suite, Unsplash, synthetic images)
        - Section 3.3: JSON Output Comparison (exact comparison logic already implemented in exiftool_comparison_tests.rs)
        - Section 5.1: Git LFS Setup (test images should be tracked with Git LFS to avoid bloating repository)

*   **File:** `.github/workflows/ci.yml`
    *   **Summary:** Current CI workflow tests on Ubuntu, macOS, Windows with build, test, clippy, and format checks. Also includes security audit and code coverage jobs.
    *   **Recommendation:** You MUST extend this workflow to add ExifTool installation and run comparison tests. The plan (Section 5.2 in integration_test_plan.md) provides the exact workflow steps needed:
        - Install Perl ExifTool via package manager (apt-get, brew, choco)
        - Run `cargo test --features exiftool-comparison`
        - Generate comparison report and upload artifacts
        - Check match rate threshold (fail if < 99%)
    *   **Important Note:** The existing workflow has no ExifTool installation step. This is a critical missing piece.

*   **File:** `tests/fixtures/`
    *   **Summary:** Currently contains only 5 test files (2 JPEG, 1 TIFF, 1 PDF, 1 MP4). The task requires expanding this to 100+ images.
    *   **Recommendation:** You MUST create the directory structure defined in the integration test plan (Section 2.3):
        ```
        tests/fixtures/
        ├── jpeg/simple/      (15 images)
        ├── jpeg/complex/     (15 images)
        ├── jpeg/edge_cases/  (10 images)
        ├── jpeg/malformed/   (10 images)
        ├── png/simple/       (10 images)
        ├── png/complex/      (10 images)
        ... and so on
        ```
    *   **Git LFS Requirement:** The plan specifies that test images MUST be tracked with Git LFS. You should create `.gitattributes` with patterns like `tests/fixtures/**/*.jpg filter=lfs diff=lfs merge=lfs -text`.

*   **File:** `Cargo.toml`
    *   **Summary:** Project configuration with the `exiftool-comparison` feature flag already defined (line 98).
    *   **Recommendation:** No changes needed to Cargo.toml - the feature flag infrastructure is already in place. Tests should use `#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]` as done in existing tests.

### Implementation Tips & Notes

*   **Tip 1 - Test Fixture Acquisition:** The integration test plan (Section 2.2) recommends three sources for test images:
    1. Exiv2 test suite (30-40 images) - GPL-compatible, diverse EXIF/IPTC/XMP coverage
    2. Unsplash (20-30 images) - CC0 public domain, real-world photos with GPS
    3. Synthetic generated images (20-30 images) - Created with ImageMagick + exiftool for known metadata

    You SHOULD prioritize downloadable public datasets first (Exiv2, sample repos) before generating synthetic images, as this provides real-world diversity.

*   **Tip 2 - Test Coverage Strategy:** The task acceptance criteria requires testing all operations: read, write, copy, rename, date shift. Currently only read operations are tested. You MUST add test functions for:
    - Write round-trip: modify tag → write → read → verify change
    - Copy metadata: `-TagsFromFile` operation
    - Rename: `-FileName` pattern substitution
    - Date shift: `-AllDates+=` operation

    These operations are all implemented in previous iterations (I3.T4, I4.T4, I4.T6, I4.T7), so you can test them.

*   **Tip 3 - CI Badge and Reporting:** The acceptance criteria requires "README shows test results badge (pass/fail)". You SHOULD:
    1. Add a workflow status badge to README.md: `[![Integration Tests](https://github.com/org/repo/workflows/Integration%20Tests/badge.svg)](https://github.com/org/repo/actions)`
    2. Generate a comparison report that gets uploaded as a GitHub Actions artifact
    3. Optionally use `$GITHUB_STEP_SUMMARY` to show match rates in the Actions UI

*   **Tip 4 - Match Rate Threshold:** The existing tests use 95% threshold, but the task specifies 98%+ for reads. You SHOULD update the assertion threshold to align with the requirement: `assert!(report.match_rate >= 98.0, ...)`. The integration test plan suggests 99% for well-formed files (Section 4.1.1).

*   **Warning:** The current test fixture directory is very small (5 files). Acquiring and organizing 100+ test images is a significant undertaking. You SHOULD start by creating the directory structure and adding a smaller representative corpus (e.g., 10-20 images) to validate the test infrastructure works, then expand to the full 100+ corpus iteratively.

*   **Note:** The integration test plan mentions running comparison tests "on every commit" in CI. However, the tests may be slow with 100+ images. You SHOULD consider adding a timeout to the CI job (e.g., `timeout-minutes: 30`) and potentially using test sharding or only running on certain branches (main, release/*) to avoid CI bottlenecks.

*   **Performance Consideration:** The comparison tests shell out to both Perl ExifTool and ExifTool-RS for every test file. With 100+ images, this could take several minutes. The test plan (Section 6.4) suggests using `hyperfine` for CLI benchmarking, which handles warmup runs and statistical analysis. You SHOULD consider whether to run full comparison on all files or sample a subset for PR CI runs.

*   **ExifTool Version Dependency:** The integration test plan (Appendix A) specifies Perl ExifTool 12.70 as the reference version. You SHOULD add a check in the test suite to verify the ExifTool version and warn if it's significantly different, as tag extraction behavior can vary between versions.

---

## 4. Additional Strategic Guidance

### Test Development Workflow

1. **Phase 1 - Infrastructure:** Extend CI to install ExifTool and run comparison tests (low-hanging fruit, immediate value)
2. **Phase 2 - Corpus Expansion:** Create directory structure and acquire initial test corpus (20-30 images) covering all formats
3. **Phase 3 - Test Coverage:** Add tests for write, copy, rename, date shift operations (reuse existing comparison framework)
4. **Phase 4 - Scale to 100+:** Expand corpus to meet 100+ image requirement, ensure Git LFS is configured
5. **Phase 5 - CI Polish:** Add badges, reporting, and documentation

### Potential Challenges

1. **Git LFS Setup:** If you haven't used Git LFS before, pay special attention to the integration test plan Section 5.1 which provides detailed setup instructions. The `.gitattributes` file is critical.

2. **Cross-Platform CI:** ExifTool installation differs across platforms (apt-get, brew, choco). The CI workflow should handle this gracefully (see plan Section 5.2 for example workflow).

3. **Match Rate Variations:** Real-world images may have tags that ExifTool-RS doesn't yet support (especially maker notes, GPS). The plan allows for documented discrepancies in `tests/integration/KNOWN_DISCREPANCIES.md`. You SHOULD create this file to track acceptable mismatches.

4. **Test Fixture Licensing:** Be careful about image licensing. Unsplash is CC0 (safe), Exiv2 is GPL-compatible (safe), but random internet images may have copyright issues. Always document image sources in `tests/fixtures/manifest.json` (see plan Section 2.4).

### Success Metrics

According to the task acceptance criteria, you've succeeded when:
- ✅ Test corpus contains 100+ diverse images across all formats
- ✅ Tests cover all 5 formats (JPEG, TIFF, PNG, PDF, MP4)
- ✅ Tests cover all 5 operations (read, write, copy, rename, date shift)
- ✅ Match rate ≥ 98% for read operations
- ✅ Round-trip tests pass (write → read → verify)
- ✅ CI runs tests on every commit with ExifTool installed
- ✅ README shows test results badge

The integration test plan adds additional success criteria:
- ✅ Git LFS configured for test fixtures
- ✅ Test corpus documented in manifest.json
- ✅ CI completes in <30 minutes (per platform)
- ✅ Known discrepancies documented

Good luck with the implementation! The existing comparison test framework is well-designed, so you're building on a solid foundation.
