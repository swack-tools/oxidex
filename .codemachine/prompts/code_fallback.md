# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task ID:** I5.T9
**Description:** Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

**Acceptance Criteria:**
- Test corpus contains 100+ diverse images ✅
- Tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4) ✅
- Tests cover all operations (read, write, copy, rename, date shift) ✅
- 98%+ tag match rate achieved for reads
- Round-trip tests pass (write → read → verify)
- CI runs tests on every commit (with ExifTool installed in CI environment) ✅
- README shows test results badge (pass/fail) ✅

---

## Issues Detected

### Critical Test Failures

The codebase has **85 failing tests** across unit tests and integration tests. The tests cannot run successfully, preventing verification of the 98%+ match rate requirement and round-trip test success.

#### 1. Unit Test Failures (7 tests)

**Parser issues causing unit test failures:**

*   **PDF Parser Issue:** `parsers::pdf::tests::test_parse_pdf_with_info_dict` is failing at src/parsers/pdf/mod.rs:274
    - Expected metadata field "Keywords" with value "test, pdf, metadata" is not being extracted (returns None instead)

*   **PNG Parser Issues (4 tests):** All PNG parser unit tests are failing due to incorrect metadata count
    - `test_parse_minimal_png`: Expected 0 tags, but parser returns 9 tags
    - `test_parse_png_with_text_chunk`: Expected 1 tag, but parser returns 10 tags
    - `test_parse_png_with_itxt_chunk`: Expected 1 tag, but parser returns 10 tags
    - `test_parse_png_with_exif_chunk`: Expected 1 tag, but parser returns 10 tags
    - **Root Cause:** The recent changes to PNG parser are adding extra metadata that tests don't expect

*   **QuickTime/MP4 Parser Issues (2 tests):**
    - `test_parse_quicktime_user_data`: Failing at src/parsers/quicktime/mod.rs:323 - "QuickTime:Title" tag not found
    - `test_parse_itunes_metadata`: Failing at src/parsers/quicktime/mod.rs:341 - "iTunes:Artist" tag not found
    - **Root Cause:** Recent changes to `src/parsers/quicktime/metadata_extractor.rs` (lines 12-519 modified, +529 additions, -33 deletions) have broken the metadata extraction logic

#### 2. Integration Test Failures (78 tests)

**Widespread integration test failures across all formats:**

*   **MP4 Tests:** 8 out of 9 MP4 tests failing (test_parse_mp4_metadata_field_count, test_parse_mp4_extracts_multiple_tags, test_parse_mp4_copyright_tag, test_parse_mp4_genre_tag, test_parse_mp4_with_quicktime_user_data, test_parse_mp4_both_itunes_and_quicktime_metadata, test_parse_sample_mp4_metadata, test_parse_mp4_atom_hierarchy)
*   **PNG Tests:** 4 PNG tests failing (test_png_empty_metadata, test_png_with_text_chunks, test_png_with_exif_chunk, test_png_with_mixed_metadata)
*   **PDF Tests:** 3 PDF tests failing (test_parse_sample_pdf_metadata, test_pdf_metadata_field_count, test_write_multiple_field_modifications)
*   **JPEG Tests:** 1 JPEG test failing (test_jpeg_xmp_extraction_end_to_end)
*   **JPEG Write Tests:** 4 tests failing (test_modify_exif_tag_in_jpeg, test_insert_exif_when_missing, test_handle_size_changes, test_write_to_real_file)
*   **Copy Metadata Tests:** All 6 copy tests failing (test_copy_all_metadata_between_jpegs, test_copy_specific_tags_only, test_copy_with_empty_source, test_copy_with_nonexistent_tag_filter, test_copy_overwrites_existing_tags, test_copy_preserves_destination_tags)
*   **Date Shift Tests:** All 8 date shift tests failing
*   **Rename Tests:** Multiple rename tests failing

**Note:** ExifTool comparison tests are being **ignored** (not failing) because they require the `exiftool-comparison` feature flag, which appears to not be active during test execution despite being specified.

#### 3. Linting Errors (3 clippy errors)

**Type complexity warnings promoted to errors:**

*   `src/parsers/png/chunk_parser.rs:529`: Complex return type `Result<Vec<(u16, u16, u32, Vec<u8>)>>`
*   `src/parsers/tiff/file_parser.rs:290`: Complex return type `Result<Vec<(u16, u16, u32, Vec<u8>)>>`
*   `src/parsers/tiff/ifd_parser.rs:140`: Complex return type `Result<Vec<(u16, u16, u32, Vec<u8>)>>`

These are blocking compilation with `-D warnings` (CI will fail).

---

## Root Cause Analysis

### Primary Issue: Breaking Changes to QuickTime Parser

The git diff shows extensive modifications to `src/parsers/quicktime/metadata_extractor.rs`:
- **778 insertions, 398 deletions** across 4 files
- The QuickTime parser was significantly refactored, adding new functions like:
  - `extract_file_level_metadata()` - extracts ftyp and mdat metadata
  - `extract_movie_header()` - extracts mvhd metadata
  - `extract_handler_metadata()` - extracts hdlr metadata
  - Additional 400+ lines of new extraction logic

**Impact:** These changes have broken the existing metadata extraction flow, causing the parser to either:
1. Not extract metadata it previously extracted (QuickTime:Title, iTunes:Artist missing)
2. Extract additional metadata that tests don't expect (PNG returning 9 extra tags)
3. Change the tag naming conventions or structure

### Secondary Issues

1. **PNG Parser Over-Extraction:** The PNG parser is now extracting file-level metadata (like PNG:IHDR data, PNG:ImageWidth, PNG:ImageHeight, etc.) that wasn't being extracted before, causing tests to fail on metadata count assertions.

2. **PDF Parser Keyword Extraction:** The PDF parser is not correctly extracting the "Keywords" field from the Info dictionary.

3. **Type Complexity:** Return types for TIFF/PNG IFD entry parsing are too complex and need to be aliased.

---

## Best Approach to Fix

### Step 1: Revert Breaking Changes to QuickTime Parser

**Action:** Revert the recent changes to `src/parsers/quicktime/metadata_extractor.rs` that were introduced in recent commits. The parser was working correctly before these modifications.

**Command:**
```bash
git checkout HEAD~1 src/parsers/quicktime/metadata_extractor.rs
```

**Rationale:** The extensive refactoring added 529 lines but broke 10+ tests. The original implementation was correct and should be restored. If enhancements are needed, they should be added incrementally with tests updated alongside.

### Step 2: Investigate and Fix PNG Parser Metadata Count Issue

**Action:** Review the PNG parser modifications that are causing it to extract 9 additional metadata fields.

**Investigation needed:**
- Check if `extract_metadata()` in `src/parsers/png/mod.rs` was modified to extract IHDR chunk data
- Determine if these extra fields are valuable (image dimensions, bit depth, color type) or should be filtered
- Update test expectations if the extra metadata is correct, OR fix the parser to not extract file-level metadata

**Files to check:**
- `src/parsers/png/mod.rs`
- `src/parsers/png/chunk_parser.rs`

### Step 3: Fix PDF Keywords Extraction

**Action:** Debug the PDF parser to ensure the Info dictionary's "Keywords" field is being extracted correctly.

**Check:**
- `src/parsers/pdf/mod.rs` around line 274
- Verify that the dictionary parsing logic correctly handles the "Keywords" key
- Ensure the value is being properly decoded (check for encoding issues, escape sequences, or whitespace trimming)

### Step 4: Fix Type Complexity Clippy Errors

**Action:** Create type aliases for the complex tuple return types.

**Implementation:**

In `src/parsers/tiff/ifd_parser.rs`:
```rust
/// Type alias for IFD entry tuples: (tag, type, count, data)
pub type IfdEntry = (u16, u16, u32, Vec<u8>);
pub type IfdEntries = Vec<IfdEntry>;
```

Then update the return types:
- `src/parsers/png/chunk_parser.rs:529`: `Result<IfdEntries>`
- `src/parsers/tiff/file_parser.rs:290`: `Result<IfdEntries>`
- `src/parsers/tiff/ifd_parser.rs:140`: `Result<IfdEntries>`

### Step 5: Rebuild and Verify Tests Pass

**Commands:**
```bash
# Build with all features
cargo build --release --all-features

# Run unit tests
cargo test --lib

# Run integration tests
cargo test --test integration

# Verify clippy passes
cargo clippy --all-features -- -D warnings
```

**Success Criteria:**
- All 387 unit tests pass
- All 122 integration tests pass
- Zero clippy errors

### Step 6: Run ExifTool Comparison Tests

Once the baseline tests pass, run the ExifTool comparison tests:

```bash
# Ensure Perl ExifTool is installed
exiftool -ver

# Run comparison tests
cargo test --release --features exiftool-comparison -- --nocapture

# Look for match rate output in test results
# Verify: 98%+ match rate for read operations
# Verify: Round-trip tests succeed
```

### Step 7: Verify CI Integration

**Check that:**
1. `.github/workflows/ci.yml` integration-tests job is properly configured (already verified ✅)
2. README.md contains test results badge (already present ✅)
3. Tests run successfully in CI environment

---

## Important Notes

### DO NOT Do These Things:

1. **DO NOT rewrite the integration test suite** - The test suite in `tests/integration/exiftool_comparison_tests.rs` is complete (14 test functions, 1275 lines, 104 test images). It is correctly implemented with proper comparison logic, tag filtering, and threshold assertions.

2. **DO NOT modify test expectations without understanding root cause** - The tests were passing before recent parser changes. Fix the parsers to match the expected behavior, not the other way around.

3. **DO NOT add more test images** - The corpus has 104 images, exceeding the 100+ requirement. The issue is not corpus size but parser correctness.

### What Success Looks Like:

After applying these fixes:
- ✅ All 387 unit tests pass
- ✅ All 122 integration tests pass
- ✅ Zero clippy errors
- ✅ ExifTool comparison tests run and show 98%+ match rates
- ✅ CI pipeline executes successfully
- ✅ Round-trip tests (write → read → verify) pass

This task (I5.T9) can then be marked as `done: true`.

---

## Additional Context

### Files Modified in Recent Commits:
- `.codemachine/prompts/code_fallback.md` (deleted, will be recreated by this document)
- `.codemachine/prompts/context.md` (documentation changes only)
- `.codemachine/template.json` (minor template changes)
- `src/parsers/quicktime/metadata_extractor.rs` (major breaking changes - **THIS IS THE PROBLEM**)

### Test Infrastructure Status:
- ✅ Test corpus: 104 images (exceeds 100+ requirement)
- ✅ CI configuration: Complete with ExifTool installation on all platforms
- ✅ Test framework: Properly implemented with comparison logic
- ✅ README badge: Present and correctly formatted
- ❌ **Tests passing: NO - 85 failures**
- ❌ **Linter passing: NO - 3 errors**

The infrastructure is complete. The issue is broken parsers, not missing test infrastructure.

---

## Priority

**CRITICAL** - The codebase is in a broken state with 85 failing tests. This must be fixed before task I5.T9 can be verified and marked complete.

**Recommended Approach:** Start with Step 1 (revert QuickTime parser changes) as this will fix ~10 tests immediately. Then proceed through steps 2-6 systematically.
