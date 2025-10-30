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

### Context: integration-test-plan (from docs/testing/integration_test_plan.md)

```markdown
# Integration Test Plan

## 1. Overview

### 1.1 Purpose

This document defines the comprehensive integration testing strategy for ExifTool-RS. Integration tests validate end-to-end workflows, CLI operations, and behavioral parity with the reference Perl ExifTool implementation.

### 1.2 Scope

Integration tests complement unit tests (70% of suite) and property-based tests (20% of suite) by covering:

- **End-to-End Workflows**: Complete read → parse → extract → output pipelines
- **CLI Interface**: Command-line argument parsing and execution
- **Cross-Format Operations**: Batch processing across multiple file formats
- **Error Handling**: Real-world failure scenarios (missing files, corrupted metadata, permission errors)
- **ExifTool Parity**: Tag value comparison against Perl ExifTool (reference implementation)

### 1.3 Success Criteria

Integration tests are considered successful when:

1. **Functional Correctness**: 99%+ tag value match rate vs. Perl ExifTool for well-formed files
2. **Graceful Degradation**: Appropriate error handling for malformed files (no crashes/hangs)
3. **Performance**: Within 2x performance of Perl ExifTool for batch operations
4. **Cross-Platform**: Pass on Linux, macOS, and Windows
5. **Regression Prevention**: No degradation in match rate or performance across commits

## 3. Validation Methodology

### 3.1 Comparison Approach

**Reference Implementation**: Perl ExifTool v12.70+ (latest stable)

**Comparison Strategy**:
1. Execute both tools on identical input files
2. Export metadata to JSON format for structured comparison
3. Parse JSON outputs and compute field-level match rate
4. Generate human-readable diff reports for mismatches

### 3.4 Match Rate Calculation

**Formula**:

```
Match Rate (%) = (Matched Tags / Total Tags in Reference) × 100
```

**Where**:
- **Matched Tags**: Tags where values are identical (or within tolerance)
- **Total Tags**: All tags extracted by Perl ExifTool (baseline)
- **Excluded**: Metadata fields (`SourceFile`, `ExifToolVersion`)

## 4. Acceptance Criteria & Thresholds

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

### 4.2 Match Rate Thresholds

**Tiered Thresholds**:

| **Test Category** | **Minimum Match Rate** | **Target Match Rate** | **Action if Below Target** |
|-------------------|------------------------|----------------------|---------------------------|
| Simple files      | 99%                    | 100%                 | Investigate immediately, block merge |
| Complex files     | 99%                    | 99.5%                | Document discrepancy, issue tracker |
| Edge cases        | 95%                    | 98%                  | Best-effort improvement |
| Malformed files   | N/A                    | N/A                  | Graceful error only |
```

### Context: testing-levels (from integration test plan)

```markdown
## 6. Test Categories

### 6.1 Format Coverage Tests

**Objective**: Ensure all supported file formats can be read, parsed, and have metadata extracted.

**Test Matrix**:

| **Format** | **Test File** | **Key Tags to Verify** | **Special Handling** |
|------------|---------------|------------------------|----------------------|
| JPEG       | `jpeg/simple/canon_eos_5d.jpg` | EXIF:Make, EXIF:Model, EXIF:DateTimeOriginal | APP1 segment (EXIF), APP0 (JFIF) |
| PNG        | `png/simple/screenshot.png` | PNG:tEXt:Author, PNG:tIME | tEXt, iTXt chunks |
| TIFF       | `tiff/simple/single_page.tif` | TIFF:ImageWidth, TIFF:BitsPerSample | IFD0 parsing |
| WebP       | `webp/simple/photo.webp` | EXIF:*, XMP:* | RIFF container, VP8 bitstream |
| HEIC       | `heic/simple/iphone_photo.heic` | EXIF:*, GPS:* | ISO Base Media File Format (BMFF) |
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `tests/integration/exiftool_comparison_tests.rs`
    *   **Summary**: Comprehensive integration test framework comparing ExifTool-RS against Perl ExifTool. Contains 14 test functions (10 read tests + 4 operation tests). Includes comparison infrastructure with JSON parsing, tag normalization, and value matching with floating-point tolerance.
    *   **Current Status**: Test framework is COMPLETE and production-ready. Already handles all 5 formats (JPEG, PNG, TIFF, PDF, MP4) and all operations (read, write, copy, rename, date shift).
    *   **Match Rate Status**:
        - **Passing (7/14 tests)**: JPEG simple (100%), JPEG EXIF+XMP (100%), PNG text (100%), All 4 operation tests (write, copy, rename, date shift)
        - **Failing (7/14 tests)**: PDF (90.91%), TIFF simple (87.50%), TIFF big-endian (82.35%), TIFF multipage (76.92%), MP4 (73.33%), PNG eXIf (68.18%), JPEG GPS (42.11%)
    *   **Root Cause**: Test failures are NOT framework issues - they are caused by incomplete parser implementations (missing GPS IFD parser, incomplete TIFF baseline tags, PNG eXIf TIFF integration issue)
    *   **Recommendation**: Your PRIMARY task is to expand the test corpus from 104 → 100+ images (already met!) and achieve 98%+ match rate. This requires FIXING THE PARSERS, not modifying the test framework.

*   **File:** `tests/fixtures/`
    *   **Summary**: Test corpus containing 104 image files across 5 formats (30 JPEG, 33 PNG, 20 TIFF, 10 PDF, 9 MP4)
    *   **Status**: Already exceeds the 100+ image requirement (104 images total)
    *   **Structure**: Organized by format/category (simple, complex, edge_cases, malformed)
    *   **Recommendation**: The test corpus size requirement is ALREADY MET. Do NOT spend time adding more test images. Focus on fixing parsers to achieve 98%+ match rate.

*   **File:** `.github/workflows/ci.yml`
    *   **Summary**: CI configuration with dedicated `integration-tests` job that installs Perl ExifTool on all platforms (Ubuntu, macOS, Windows) and runs comparison tests with feature flag
    *   **Status**: CI integration is COMPLETE and working
    *   **Lines**: 104-150 define the integration-tests job with cross-platform ExifTool installation
    *   **Recommendation**: CI infrastructure is production-ready. Do NOT modify CI configuration.

*   **File:** `README.md`
    *   **Summary**: Project README with CI status badges
    *   **Status**: Already contains two badges (CI + Integration Tests) on lines 3-4
    *   **Recommendation**: README badge requirement is ALREADY MET. Do NOT modify README.

*   **File:** `tests/fixtures/I5T9_FINAL_SUMMARY.md`
    *   **Summary**: Detailed completion report showing test results and root cause analysis
    *   **Key Finding**: "The failing tests are NOT due to test framework issues. The test infrastructure is comprehensive and well-designed. The failures are due to incomplete parser implementations."
    *   **Critical Issues Identified**:
        1. GPS Tag Extraction (42.11% match) - GPS IFD parsing not implemented
        2. PNG eXIf TIFF Integration (68.18% match) - eXIf chunk outputs raw tag IDs instead of tag names
        3. TIFF Missing Tags (76-87% match) - Missing baseline TIFF tags (ResolutionUnit, Software, DateTime, Orientation)
        4. PDF Missing Field (90.91% match) - 1 tag away from passing
        5. MP4 QuickTime Atoms (73.33% match) - Missing iTunes metadata atoms
    *   **Recommendation**: Read this file completely. It provides EXACT instructions on which parsers need fixes to achieve 98%+ match rate.

*   **File:** `tests/fixtures/manifest.json`
    *   **Summary**: Test corpus metadata tracking 104 images with source attribution
    *   **Status**: Complete documentation of test corpus
    *   **Recommendation**: Use this file to understand corpus structure. Do NOT modify.

### Implementation Tips & Notes

*   **CRITICAL MISUNDERSTANDING ALERT**: The task description says "Expand integration test suite from I3.T10 to cover all supported formats and operations." This sounds like you need to write MORE TEST CODE. **THIS IS WRONG.** The test framework is ALREADY COMPLETE with 14 comprehensive tests covering all formats and operations. The corpus already has 104 images (exceeds 100+ requirement). CI integration is done. README badges are present.

*   **WHAT THIS TASK ACTUALLY REQUIRES**: The 98%+ match rate acceptance criterion is FAILING because of PARSER BUGS, not test framework gaps. To complete this task, you MUST:
    1. **Fix GPS IFD parser** (most critical - 42.11% match rate)
    2. **Fix PNG eXIf TIFF integration** (critical - 68.18% match rate)
    3. **Add missing TIFF baseline tags** (high priority - 3 tests at 76-87%)
    4. **Add 1 missing PDF tag** (low effort - 90.91% is close to 98%)
    5. **Expand MP4 QuickTime atoms** (medium priority - 73.33%)

*   **DO NOT**:
    - Add more test functions to `exiftool_comparison_tests.rs` (already has 14 tests covering everything)
    - Modify CI configuration (already working perfectly)
    - Add badges to README (already present)
    - Generate more test images (104 already exceeds 100+ requirement)
    - Modify test framework comparison logic (it's production-ready)

*   **DO**:
    - Read `tests/fixtures/I5T9_FINAL_SUMMARY.md` completely to understand exact parser fixes needed
    - Fix GPS IFD parser in `src/parsers/tiff/` to parse GPS sub-IFD (lines 200+ in summary explain this)
    - Fix PNG eXIf integration to use TIFF IFD decoder for proper tag names
    - Add 5-8 missing TIFF baseline tags to TIFF parser
    - Debug PDF parser to find 1 missing Info dictionary or XMP field
    - Expand MP4 QuickTime atom parser for iTunes metadata

*   **Parser File Locations** (from directory structure):
    - TIFF/EXIF parser: `src/parsers/tiff/` (needs GPS IFD support)
    - PNG parser: `src/parsers/png/` (needs eXIf TIFF integration)
    - PDF parser: `src/parsers/pdf/` (needs 1 missing field)
    - MP4 parser: `src/parsers/quicktime/` (needs more iTunes atoms)

*   **Success Criteria**: When you've fixed the parsers correctly, running `cargo test --release --features exiftool-comparison` will show 10/10 read tests passing (98%+ match rate) + 4/4 operation tests passing = 14/14 total tests passing.

*   **Estimated Effort** (from summary report):
    - GPS parser: 4-6 hours (CRITICAL)
    - TIFF baseline tags: 2-3 hours (HIGH)
    - PNG eXIf integration: 3-4 hours (HIGH)
    - PDF missing field: 2-3 hours (LOW effort)
    - MP4 QuickTime: 3-4 hours (MEDIUM)
    - **Total**: 14-20 hours of parser implementation work

*   **Note**: The task description is somewhat misleading. It asks you to "expand test suite" when what's actually needed is "fix parser implementations to make existing comprehensive test suite pass." The test framework itself is exemplary and needs NO changes.
