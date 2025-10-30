# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task ID**: I5.T9
**Description**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

**Acceptance Criteria**:
1. Test corpus contains 100+ diverse images ✅ **PASS** (102 images found)
2. Tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4) ✅ **PASS**
3. Tests cover all operations (read, write, copy, rename, date shift) 🟡 **PARTIAL** (only read implemented)
4. 98%+ tag match rate achieved for reads ❌ **FAIL** (9 out of 10 tests failing)
5. Round-trip tests pass (write → read → verify) 🟡 **PENDING** (depends on I4)
6. CI runs tests on every commit (with ExifTool installed in CI environment) ✅ **PASS**
7. README shows test results badge (pass/fail) ✅ **PASS**

---

## Issues Detected

### **Critical: 9 out of 10 Integration Tests Failing**

The test run with `cargo test --features exiftool-comparison --release` shows:
- **PASS**: 1 test (`test_comparison_jpeg_with_exif`)
- **FAIL**: 9 tests (all other comparison tests)

**Root Cause**: The ExifTool-RS implementation is returning **MISSING** for most tags that Perl ExifTool extracts. This results in match rates well below the 98% threshold.

#### Specific Test Failures:

1. **`test_comparison_jpeg_with_exif_xmp`** - Failed: Tags from XMP metadata are missing
2. **`test_comparison_jpeg_with_gps`** - Failed: GPS coordinate tags are missing
3. **`test_comparison_mp4`** - Failed: QuickTime/iTunes metadata tags are missing
4. **`test_comparison_pdf`** - Failed: PDF Info/XMP tags are missing
5. **`test_comparison_png_with_exif`** - Failed: PNG eXIf chunk tags are missing
6. **`test_comparison_png_with_text`** - Failed: PNG text chunk tags are missing
7. **`test_comparison_tiff`** - Failed: TIFF IFD tags are missing
8. **`test_comparison_tiff_big_endian`** - Failed: Big-endian TIFF tags are missing
9. **`test_comparison_tiff_multipage`** - Failed (match rate 1.85%): Multi-page TIFF tags across IFD0, IFD1, IFD2 are all missing

**Example from `test_comparison_tiff_multipage` output**:
```
Match rate: 1.85%
Matched: 1/54 tags

Mismatches (53):
  IFD0:Orientation
    Perl:  String("Horizontal (normal)")
    Rust:  MISSING
  IFD0:BitsPerSample
    Perl:  String("16 16 16 16")
    Rust:  MISSING
  IFD0:Compression
    Perl:  String("Uncompressed")
    Rust:  MISSING
  [... 50+ more tags all MISSING ...]
```

### **No Linting Errors**
- Cargo clippy completed successfully with no warnings or errors (aside from build script warnings)

---

## Best Approach to Fix

### **Phase 1: Diagnose the Metadata Reading Implementation**

The tests clearly show that ExifTool-RS is not successfully reading metadata from the test images. You need to:

1. **Check the binary execution**: Manually run the ExifTool-RS binary on one of the failing test images to see what it outputs:
   ```bash
   cargo build --release
   ./target/release/exiftool-rs --json tests/fixtures/tiff/simple/sample.tif
   ```
   Compare this output with Perl ExifTool:
   ```bash
   exiftool -json -a -G1 -struct tests/fixtures/tiff/simple/sample.tif
   ```

2. **Identify the root cause**: Determine whether:
   - The parsers are not reading the metadata correctly
   - The JSON serialization is not including the tags
   - The format parsers (JPEG, PNG, TIFF, PDF, MP4) have bugs
   - The tag extraction logic is incomplete

### **Phase 2: Fix the Core Reading Implementation**

Based on the diagnosis, fix the underlying issues in the format parsers:

1. **TIFF Parser** (`src/formats/tiff/` or similar):
   - Ensure IFD (Image File Directory) chains are fully traversed
   - Handle multi-page TIFF files (IFD0, IFD1, IFD2, etc.)
   - Support both little-endian and big-endian byte order
   - Extract all standard TIFF tags (Compression, BitsPerSample, ImageWidth, ImageHeight, etc.)

2. **PNG Parser** (`src/formats/png/` or similar):
   - Extract text chunks (tEXt, zTXt, iTXt)
   - Extract eXIf chunks (EXIF data in PNG format)
   - Ensure all chunk types are properly parsed

3. **JPEG Parser** (`src/formats/jpeg/` or similar):
   - Extract EXIF segments (APP1 EXIF)
   - Extract XMP segments (APP1 XMP)
   - Extract GPS IFD tags within EXIF

4. **PDF Parser** (`src/formats/pdf/` or similar):
   - Parse Info dictionary metadata
   - Parse XMP metadata streams

5. **MP4 Parser** (`src/formats/mp4/` or similar):
   - Parse iTunes metadata atoms (©day, ©nam, etc.)
   - Parse keys/ilst metadata structures
   - Parse QuickTime metadata atoms

### **Phase 3: Verify Tag Output Format**

Ensure that the JSON output from ExifTool-RS matches the expected structure:

1. **Group Names**: Tags should include group prefixes (e.g., `IFD0:ImageWidth`, `EXIF:Make`, `GPS:Latitude`)
2. **Tag Values**: Values should be properly formatted (strings, numbers, arrays)
3. **TagValue Enum**: The comparison test has logic to unwrap TagValue enums (e.g., `{"String": "value"}` → `"value"`). Ensure this unwrapping works correctly or that the output format is consistent.

### **Phase 4: Re-run Tests and Iterate**

After fixing the core issues:

1. Run the comparison tests again:
   ```bash
   cargo test --features exiftool-comparison --release test_comparison
   ```

2. For each test that still fails:
   - Review the mismatch report printed by the test
   - Identify which tags are still missing or incorrect
   - Fix the specific parser or tag extraction logic
   - Repeat until all tests pass with 98%+ match rate

### **Phase 5: Update Documentation**

Once all tests pass:

1. Update `tests/fixtures/COMPLETION_REPORT.md` to reflect the actual test results
2. Update the acceptance criteria status from "READY" to "PASS" for criterion #4 (98%+ tag match rate)
3. Document any known discrepancies in `tests/integration/KNOWN_DISCREPANCIES.md`

---

## Priority

**HIGH PRIORITY**: The integration tests are the acceptance criteria for task I5.T9. Without passing tests, the task cannot be marked as complete.

**Start with**: Focus on getting one format (e.g., TIFF) working completely before moving to others. The simplest test (`test_comparison_tiff` with `tests/fixtures/tiff/simple/sample.tif`) should be your first target after the one JPEG test that already passes.

**Note**: Write operations (criteria #3 and #5) are explicitly documented as I4 dependencies and should NOT block this task. Focus ONLY on read operation tests.
