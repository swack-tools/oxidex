# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task ID**: I5.T9

**Description**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

**Acceptance Criteria**:
1. Test corpus contains 100+ diverse images
2. Tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4)
3. Tests cover all operations (read, write, copy, rename, date shift)
4. 98%+ tag match rate achieved for reads
5. Round-trip tests pass (write → read → verify)
6. CI runs tests on every commit (with ExifTool installed in CI environment)
7. README shows test results badge (pass/fail)

---

## Issues Detected

### CRITICAL ISSUE #1: Test Corpus Size Requirement Not Met
*   **Problem**: The test corpus contains only **5 images** but the acceptance criteria explicitly requires **100+ diverse images**
*   **Current State**:
    - JPEG: 2 files (requires ~50 per integration test plan)
    - TIFF: 1 file (requires ~25 per plan)
    - PDF: 1 file (requires ~15 per plan)
    - MP4: 1 file (requires ~10 per plan)
    - PNG: 0 files (requires ~30 per plan)
*   **Evidence**: `find tests/fixtures -type f` returns only 5 image files
*   **Root Cause**: Implementation created infrastructure and documentation but did not execute the image acquisition phases outlined in `tests/fixtures/ACQUISITION_GUIDE.md`

### CRITICAL ISSUE #2: Write Operations Not Tested
*   **Problem**: Acceptance criteria requires testing all operations including "write, copy, rename, date shift" but only read operations are implemented
*   **Current State**: Lines 587-616 in `tests/integration/exiftool_comparison_tests.rs` show commented-out TODO placeholders for:
    - `test_write_roundtrip_jpeg_artist` (write operation)
    - `test_copy_metadata_jpeg_to_jpeg` (copy operation)
    - `test_rename_file_pattern` (rename operation)
    - `test_date_shift_all_dates` (date shift operation)
*   **Evidence**: No uncommented test functions exist that test write/modify operations
*   **Root Cause**: Tests are marked as TODO with comments like "Implement when write functionality is complete (I4.T4)"

### CRITICAL ISSUE #3: PNG Format Not Tested
*   **Problem**: Acceptance criteria requires "all supported formats (JPEG, TIFF, PNG, PDF, MP4)" but PNG has zero test images
*   **Current State**: `tests/fixtures/png/` directories exist but contain no images
*   **Evidence**: `find tests/fixtures/png -type f` returns no results
*   **Impact**: Cannot verify 98%+ match rate for PNG format

### ISSUE #4: Round-Trip Tests Not Implemented
*   **Problem**: Acceptance criteria requires "Round-trip tests pass (write → read → verify)"
*   **Current State**: No round-trip test functions are implemented (see lines 587-616 in exiftool_comparison_tests.rs - all commented out)
*   **Impact**: Cannot verify that write operations maintain data integrity

---

## Best Approach to Fix

### Phase 1: Acquire Test Corpus (HIGHEST PRIORITY)

You MUST execute the image acquisition strategy outlined in `tests/fixtures/ACQUISITION_GUIDE.md` to reach 100+ images:

1. **Execute Acquisition Phase 1 - Public Test Suites (40-50 images)**
   - Clone the Exiv2 test suite repository: `https://github.com/Exiv2/exiv2`
   - Use sparse checkout to only get `tests/data/` directory
   - Copy 40-50 diverse images covering:
     - JPEG with EXIF only (~15 images)
     - JPEG with EXIF+XMP+IPTC (~10 images)
     - JPEG with GPS metadata (~5 images)
     - TIFF big/little-endian (~10 images)
     - PNG with text chunks (~5 images)
     - PNG with eXIf chunk (~5 images)
   - Organize into `tests/fixtures/{format}/{simple|complex|edge_cases}/` structure
   - Document each image in `tests/fixtures/manifest.json` with source attribution

2. **Execute Acquisition Phase 2 - Public Domain Images (20-30 images)**
   - Download 20-30 images from Unsplash (CC0 license) using their API or direct download
   - Prioritize images with GPS EXIF data for GPS coordinate tolerance testing
   - Use the script template in ACQUISITION_GUIDE.md section 3.2
   - Document sources in manifest.json

3. **Execute Acquisition Phase 3 - Synthetic Test Images (20-30 images)**
   - Use the `create_synthetic_fixtures.sh` script outlined in ACQUISITION_GUIDE.md section 3.3
   - Generate edge case images:
     - Large EXIF segments (>64KB)
     - Unusual character encodings (UTF-8, UTF-16)
     - Malformed but parseable metadata
     - Maximum nesting depth for XMP structures
   - Use ImageMagick + exiftool to inject known metadata values
   - Document expected values in manifest.json for verification

4. **Execute Acquisition Phase 4 - Format-Specific Tests (10-20 images)**
   - PNG: Generate PNG with various text chunk types (tEXt, zTXt, iTXt)
   - TIFF: Create multi-page TIFF files
   - PDF: Create PDF with XMP metadata (not just Info dictionary)
   - MP4: Create MP4 with GPS track data

**Expected Outcome**: 90-120 images across all formats, organized in the directory structure, with complete manifest.json documentation

### Phase 2: Implement Write Operation Tests

You MUST uncomment and implement the placeholder test functions in `tests/integration/exiftool_comparison_tests.rs`:

1. **Implement `test_write_roundtrip_jpeg_artist`** (lines 590-592)
   - Read a JPEG image
   - Modify the Artist tag to a known value
   - Write the changes using ExifTool-RS binary: `exiftool-rs -Artist="Test Artist" -o output.jpg input.jpg`
   - Read back the modified image
   - Verify the Artist tag matches the expected value
   - Compare behavior against Perl ExifTool's write operation

2. **Implement `test_copy_metadata_jpeg_to_jpeg`** (lines 595-600)
   - Use two JPEG images (source with rich metadata, destination with minimal metadata)
   - Copy metadata using ExifTool-RS: `exiftool-rs -TagsFromFile source.jpg destination.jpg`
   - Read the destination file
   - Verify copied tags match source
   - Compare against Perl ExifTool's `-TagsFromFile` operation

3. **Implement `test_rename_file_pattern`** (lines 603-608)
   - Take a JPEG with DateTimeOriginal metadata
   - Rename using pattern: `exiftool-rs '-FileName<DateTimeOriginal' -d '%Y%m%d_%H%M%S.%%e' test.jpg`
   - Verify file was renamed according to pattern
   - Compare behavior with Perl ExifTool

4. **Implement `test_date_shift_all_dates`** (lines 611-616)
   - Read a JPEG with multiple date/time tags
   - Shift all dates by a known offset: `exiftool-rs '-AllDates+=1:0:0 12:30:0' test.jpg`
   - Read back and verify all date fields shifted correctly
   - Compare against Perl ExifTool's date math

**Expected Outcome**: 4 new passing test functions that verify write operations achieve 98%+ match rate with Perl ExifTool

### Phase 3: Add PNG Test Cases

You MUST add test images and test functions for PNG format:

1. **Acquire PNG test images** (part of Phase 1 above, but specifically):
   - 5 PNG images with text chunks (tEXt, zTXt, iTXt)
   - 5 PNG images with eXIf chunk
   - Place in `tests/fixtures/png/simple/` and `tests/fixtures/png/complex/`

2. **Add test function `test_comparison_png_with_text`** (uncomment lines 625-627)
   - Follow the same pattern as `test_comparison_jpeg_with_exif`
   - Test PNG with text chunks
   - Verify 98%+ match rate

3. **Add test function `test_comparison_png_with_exif`** (uncomment lines 632-634)
   - Test PNG with eXIf chunk
   - Verify EXIF data extraction matches Perl ExifTool

**Expected Outcome**: PNG format fully tested with 10+ images and 2+ test functions

### Phase 4: Verify and Document

1. **Run full test suite**: `cargo test --features exiftool-comparison`
2. **Verify all tests pass** with 98%+ match rates
3. **Update progress tracking** in `tests/integration/exiftool_comparison_tests.rs` header (lines 20-22) to reflect actual image count
4. **Update `tests/integration/I5_T9_IMPLEMENTATION_SUMMARY.md`** to change status from "In Progress (5%)" to "Complete (100%)"
5. **Run CI locally** or push to verify GitHub Actions workflow succeeds

---

## Implementation Notes

- **Priority Order**: Phase 1 is absolutely critical - without 100+ images, the task cannot be considered complete regardless of code quality
- **Licensing**: Ensure all acquired images are properly licensed (GPL-compatible, CC0, or Public Domain). Document in manifest.json
- **Git LFS**: The `.gitattributes` file is already configured for Git LFS. After adding images, run `git lfs track` to verify tracking is active
- **Testing Time**: With 100+ images, test suite may take 5-10 minutes. The CI workflow already has 30-minute timeout which is adequate
- **Known Discrepancies**: If new format-specific discrepancies are found (e.g., PNG text encoding differences), document in `tests/integration/KNOWN_DISCREPANCIES.md` and adjust thresholds accordingly

---

## Success Criteria

The implementation will be considered complete when:

✅ `find tests/fixtures -type f \( -name "*.jpg" -o -name "*.tif" -o -name "*.png" -o -name "*.pdf" -o -name "*.mp4" \)` returns **100+** files

✅ `tests/fixtures/png/` contains at least 10 PNG images

✅ `cargo test --features exiftool-comparison` shows at least **8 passing test functions** (5 read tests + 4 write/operation tests + PNG tests)

✅ All test functions assert 98%+ match rate and pass

✅ `tests/integration/exiftool_comparison_tests.rs` has NO commented-out test functions for core operations

✅ CI workflow runs successfully on all platforms (Ubuntu, macOS, Windows)
