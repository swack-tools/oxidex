# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task: v1.0.0 Release Preparation - Final Verification**

Complete all 55 development tasks across 5 iterations for the ExifTool-RS project v1.0.0 stable release. The codebase should be production-ready with:
- All format parsers and writers working correctly
- Comprehensive test coverage passing
- No linting errors
- Complete documentation
- Cross-platform binaries ready for distribution

---

## Issues Detected

The verification revealed **70 failing integration tests** across multiple test modules. While all 387 library unit tests pass successfully, the integration tests are failing due to the following critical issues:

### **Critical Issue #1: TIFF Writer Tag Prefix Mismatch**

*   **Root Cause:** The TIFF IFD serializer in `src/writers/tiff_writer.rs` line 275 filters tags using `tag_name.starts_with("EXIF:")`, but the TIFF/JPEG parsers and all integration tests use the "IFD0:" prefix for main IFD tags.
*   **Impact:** ALL write operations fail silently because no tags are actually written to files. Written metadata cannot be read back.
*   **Affected Tests:** All write operation tests (70 tests) including:
    - `write_operations_tests::test_write_metadata_successful_jpeg_write`
    - `write_operations_tests::test_write_metadata_atomic_operation`
    - `write_operations_tests::test_write_metadata_with_integer_tags`
    - `write_operations_tests::test_modify_tag_single_tag_modification`
    - `jpeg_write_tests::test_modify_exif_tag_in_jpeg`
    - All TIFF, PNG, PDF write tests
    - All copy metadata tests
    - All date shift tests
    - All rename tests

### **Critical Issue #2: Missing Test Fixture Files**

*   **Root Cause:** Integration tests expect specific "sample" files (e.g., `tests/fixtures/tiff/sample.tif`, `tests/fixtures/pdf/sample.pdf`, `tests/fixtures/mp4/sample.mp4`) that don't exist in the fixtures directory.
*   **Impact:** Tests fail immediately when trying to open these non-existent files.
*   **Affected Files:**
    - `tests/fixtures/tiff/sample.tif` (expected by tiff_tests.rs)
    - `tests/fixtures/pdf/sample.pdf` (expected by pdf_tests.rs)
    - `tests/fixtures/mp4/sample.mp4` (expected by mp4_tests.rs)
    - `tests/fixtures/png/sample.png` (expected by png_tests.rs)
    - `tests/fixtures/jpeg/sample_with_exif_xmp.jpg` (expected by jpeg_tests.rs)
*   **Note:** The manifest.json references these files, but they don't exist. Only synthetic generated files are present (e.g., synthetic_001.tif, synthetic_gps_002.mp4, etc.)

### **Critical Issue #3: Validation Not Enforcing Type Constraints**

*   **Test Failure:** `write_operations_tests::test_write_metadata_validation_fails_for_invalid_type` expects validation to reject an Integer value for the "IFD0:Make" tag (which should be String type).
*   **Root Cause:** The validation in `src/core/validation.rs` is not properly enforcing type constraints. The test expects an `InvalidTagValue` error, but the write operation is not failing as expected.
*   **Expected Behavior:** Writing `TagValue::new_integer(42)` to "IFD0:Make" should return an error.
*   **Affected Tests:**
    - `test_write_metadata_validation_fails_for_invalid_type`
    - `test_write_metadata_validation_fails_for_rational_zero_denominator`

---

## Best Approach to Fix

You MUST fix these issues in the following order:

### **Step 1: Fix TIFF Writer Tag Prefix Handling**

**File:** `src/writers/tiff_writer.rs` (line 274-277)

**Current problematic code:**
```rust
for (tag_name, tag_value) in metadata.iter() {
    // Only process EXIF tags
    if !tag_name.starts_with("EXIF:") {
        continue;
    }
```

**Required fix:**
Change the tag filtering logic to accept BOTH "EXIF:" and "IFD0:" prefixes (and other IFD prefixes like "ExifIFD:", "GPS:", etc.). The code should process all tags that can be written to TIFF IFD structures, not just those starting with "EXIF:".

**Recommended approach:**
- Accept tags starting with "IFD0:", "IFD1:", "ExifIFD:", "GPS:", "EXIF:", or any other valid IFD/EXIF family prefix
- Only skip tags that are definitively NOT TIFF-writable (e.g., "QuickTime:", "PDF:", "XMP:" top-level)
- Consider checking if the tag has a numeric tag ID instead of filtering by prefix

### **Step 2: Create Missing Test Fixture Files**

**Required files to create:**

1. **`tests/fixtures/tiff/sample.tif`** - A basic single-page TIFF with standard EXIF tags (ImageWidth, ImageHeight, Make, Model, etc.)
2. **`tests/fixtures/pdf/sample.pdf`** - A simple PDF with Info dictionary metadata (Title, Author, Creator, etc.)
3. **`tests/fixtures/mp4/sample.mp4`** - A basic MP4 with QuickTime metadata (CreateDate, Duration, etc.)
4. **`tests/fixtures/png/sample.png`** - A PNG with text chunks or eXIf chunk
5. **`tests/fixtures/jpeg/sample_with_exif_xmp.jpg`** - A JPEG with both EXIF and XMP metadata

**Recommended approach:**
- Use the existing `tests/fixtures/create_synthetic_fixtures.sh` script
- OR copy and rename one of the existing synthetic files to match the expected names (e.g., `cp tests/fixtures/tiff/simple/synthetic_001.tif tests/fixtures/tiff/sample.tif`)
- Ensure the created files have appropriate metadata that matches what the tests expect

### **Step 3: Fix Tag Validation Enforcement**

**File:** `src/core/validation.rs`

**Issue:** The `validate_tag_value()` function is not properly rejecting type mismatches. When a tag descriptor specifies `value_type: ValueType::String`, but the provided TagValue is `TagValue::Integer`, it should return an `InvalidTagValue` error.

**Required fix:**
- Ensure the validation function checks the TagValue variant against the descriptor's value_type
- Return `ExifToolError::InvalidTagValue` with a clear "Type mismatch" message when types don't match
- Handle special case: Rational values with zero denominator should also return an error

**Recommended approach:**
- Review the matching logic in `validate_tag_value()`
- Add explicit checks for Integer vs String type mismatches
- Add validation for Rational denominators (must be non-zero)

### **Step 4: Re-run Tests and Verify**

After making the above fixes:

1. Run `cargo clippy --all-targets --all-features -- -D warnings` to ensure no linting errors
2. Run `cargo test --lib --all-features` to verify all 387 unit tests still pass
3. Run `cargo test --test integration --all-features` to verify all integration tests now pass
4. Fix any remaining test failures iteratively

---

## Expected Outcome

After completing all fixes:
- ✅ All 387 unit tests should pass (currently passing)
- ✅ All 70+ integration tests should pass (currently failing)
- ✅ No linting errors (currently clean)
- ✅ Write operations should successfully persist metadata that can be read back
- ✅ Validation should properly reject invalid tag values
- ✅ All test fixtures should exist and be readable

**Priority:** The tag prefix fix (Step 1) is the MOST CRITICAL as it blocks all write functionality. Focus on this first.
