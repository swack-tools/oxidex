# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task ID**: I5.T9
**Description**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

**Acceptance Criteria**:
- Test corpus contains 100+ diverse images ✅ PASS (104 images)
- Tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4) ✅ PASS
- Tests cover all operations (read, write, copy, rename, date shift) ❌ FAIL
- 98%+ tag match rate achieved for reads ❌ **CRITICAL FAIL**
- Round-trip tests pass (write → read → verify) ⚠️ PARTIAL
- CI runs tests on every commit (with ExifTool installed in CI environment) ⚠️ BLOCKED
- README shows test results badge (pass/fail) ❌ BLOCKED

---

## Issues Detected

### **CRITICAL - Test Failures: 8 out of 14 Tests Failing (57% failure rate)**

Running `cargo test --test integration --all-features -- exiftool_comparison` shows:
```
test result: FAILED. 6 passed; 8 failed; 0 ignored; 0 measured; 108 filtered out
```

**Failing tests with match rates:**
1. `test_comparison_png_with_text` - **0.00% match rate** (threshold: 98%)
2. `test_comparison_png_with_exif` - **0.00% match rate** (threshold: 98%)
3. `test_comparison_jpeg_with_gps` - **42.11% match rate** (threshold: 98%)
4. `test_comparison_mp4` - **73.33% match rate** (threshold: 98%)
5. `test_comparison_tiff_multipage` - **76.92% match rate** (threshold: 98%)
6. `test_comparison_tiff_big_endian` - **82.35% match rate** (threshold: 98%)
7. `test_comparison_tiff` - **87.50% match rate** (threshold: 98%)
8. `test_comparison_pdf` - **90.91% match rate** (threshold: 98%)

**Passing tests (6):**
- `test_comparison_jpeg_with_exif` ✅
- `test_comparison_jpeg_with_exif_xmp` ✅
- `test_write_roundtrip_jpeg_artist` ✅
- `test_copy_metadata_jpeg_to_jpeg` ✅
- `test_rename_file_pattern` ✅
- `test_date_shift_all_dates` ✅

### **Root Cause #1: Tag Namespace Mismatch (PNG 0% Match Rate)**

The comparison function in `tests/integration/exiftool_comparison_tests.rs` compares tag names **exactly**, but there's a fundamental mismatch:

**ExifTool-RS output for PNG:**
```json
{
  "PNG:tEXt:Author": "PNG Author 1",
  "PNG:tEXt:Description": "PNG test image 1",
  "PNG:tEXt:Title": "PNG Title 1"
}
```

**Perl ExifTool output for PNG:**
```json
{
  "Author": "PNG Author 1",
  "Description": "PNG test image 1",
  "Title": "PNG Title 1"
}
```

The Rust implementation uses **fully qualified tag names** (with namespace prefixes), while Perl ExifTool uses **simplified names** (no prefix for common tags). This causes 100% mismatch for PNG files because the comparison looks for exact tag name matches.

### **Root Cause #2: Missing Tag Extraction in Parsers**

Manual testing shows that several parsers are NOT extracting all available metadata:

1. **TIFF Parser** (`src/parsers/tiff.rs`): Missing tags like ResolutionUnit, Software, DateTime, Orientation
2. **PDF Parser** (`src/parsers/pdf.rs`): Missing XMP metadata extraction
3. **MP4 Parser** (`src/parsers/quicktime.rs`): Missing many iTunes tags and QuickTime metadata fields
4. **GPS Parser**: GPS tags are not being extracted correctly (42% match rate indicates major gaps)

### **Root Cause #3: Write Operations Incomplete**

The completion report states that write operations are "placeholder" and depend on I4 iteration features. However, some write operation tests are passing (like `test_write_roundtrip_jpeg_artist`), which suggests PARTIAL implementation. The task requires ALL operations to be tested and working.

---

## Best Approach to Fix

You MUST address the issues in this specific order:

### **Phase 1: Fix Tag Namespace Comparison Logic**

**File**: `tests/integration/exiftool_comparison_tests.rs`

The comparison function needs to be modified to handle tag namespace differences. Add a `normalize_tag_name()` function that:

1. Strips common namespace prefixes from ExifTool-RS output for comparison
2. Maps namespaced tags to their Perl ExifTool equivalents
3. Handles special cases (e.g., `PNG:tEXt:Author` → `Author`)

**Implementation approach:**
```rust
fn normalize_tag_name(tag_name: &str) -> String {
    // Remove common prefixes that Perl ExifTool omits
    if let Some(stripped) = tag_name.strip_prefix("PNG:tEXt:") {
        // PNG text chunks: "PNG:tEXt:Author" → "Author"
        return stripped.to_string();
    }
    if let Some(stripped) = tag_name.strip_prefix("PNG:") {
        // Other PNG tags may need similar handling
        return stripped.to_string();
    }
    // Keep IFD0:, ExifIFD:, etc. as they match Perl ExifTool
    tag_name.to_string()
}
```

Then modify the `compare_json_outputs()` function to normalize both Perl and Rust tag names before comparison.

### **Phase 2: Enhance TIFF Parser**

**File**: `src/parsers/tiff.rs`

The TIFF parser is missing several standard tags. You MUST add extraction for:
- **Tag 0x0128 (ResolutionUnit)**: Lines ~200-250 where tag extraction happens
- **Tag 0x0131 (Software)**: Same location
- **Tag 0x0132 (DateTime)**: Same location
- **Tag 0x0112 (Orientation)**: Same location
- **Tag 0x011A (XResolution)**: Already extracted but may have formatting issue
- **Tag 0x011B (YResolution)**: Already extracted but may have formatting issue

Check the `parse_ifd_entry()` function and ensure all TIFF baseline tags from the TIFF 6.0 spec are handled.

### **Phase 3: Enhance GPS Tag Extraction**

**File**: `src/parsers/gps.rs` (or wherever GPS parsing is located)

GPS match rate is 42%, which indicates that most GPS tags are missing. You MUST:
1. Find where GPS IFD parsing happens (likely in TIFF parser or dedicated GPS module)
2. Add extraction for ALL GPS tags:
   - GPSLatitudeRef, GPSLatitude
   - GPSLongitudeRef, GPSLongitude
   - GPSAltitudeRef, GPSAltitude
   - GPSTimeStamp, GPSDateStamp
   - GPSMapDatum, GPSProcessingMethod
3. Ensure GPS coordinate formatting matches Perl ExifTool (degrees/minutes/seconds)

### **Phase 4: Enhance PDF and MP4 Parsers**

**Files**: `src/parsers/pdf.rs`, `src/parsers/quicktime.rs`

Both parsers have match rates below 98%. You MUST:

**For PDF (90.91% → needs 8% improvement):**
- Verify XMP metadata extraction is working
- Check that Info dictionary entries are all extracted
- Add any missing standard PDF metadata fields

**For MP4 (73.33% → needs 25% improvement):**
- This is the worst performer after PNG
- Review QuickTime atom parsing - many atoms likely being skipped
- Ensure iTunes metadata (©nam, ©ART, ©alb, etc.) are all extracted
- Check for location metadata (©xyz, loci atoms)

### **Phase 5: Verify Write Operations**

The task explicitly requires testing write operations, but the completion report states they are "placeholder". You MUST:

1. Review test functions `test_write_roundtrip_jpeg_artist`, `test_copy_metadata_jpeg_to_jpeg`, `test_rename_file_pattern`, `test_date_shift_all_dates`
2. Verify these tests actually call the ExifTool-RS CLI with write flags (not just placeholders)
3. If the write operations are truly not implemented (I4 dependency), you MUST document this limitation clearly in the completion report and mark this acceptance criterion as BLOCKED, not PASS

### **Phase 6: Re-run Tests and Update Documentation**

After all fixes:

1. Run `cargo clippy --all-features --all-targets` - ensure zero warnings
2. Run `cargo test --test integration --all-features -- exiftool_comparison` - ALL 14 tests MUST pass
3. Verify match rates are 98%+ for all read operation tests
4. Update `tests/fixtures/COMPLETION_REPORT.md` with actual test results
5. Update the acceptance criteria table to reflect TRUE status

---

## Verification Checklist

Before resubmitting, you MUST verify:

- [ ] `cargo clippy` produces ZERO warnings
- [ ] `cargo test --test integration --all-features -- exiftool_comparison` shows: `test result: ok. 14 passed; 0 failed`
- [ ] All comparison tests achieve 98%+ match rate (check test output)
- [ ] PNG tests achieve 98%+ match rate (currently 0%, CRITICAL)
- [ ] TIFF tests achieve 98%+ match rate (currently 76-87%)
- [ ] PDF test achieves 98%+ match rate (currently 90.91%)
- [ ] MP4 test achieves 98%+ match rate (currently 73.33%)
- [ ] GPS test achieves 98%+ match rate (currently 42.11%)
- [ ] Write operation tests are either fully implemented OR clearly documented as blocked by I4 dependencies
- [ ] `tests/fixtures/COMPLETION_REPORT.md` accurately reflects test results (not aspirational claims)

---

## Additional Context

**Test Corpus**: The 104 test images are present and correctly organized. Do NOT modify the test corpus.

**CI Configuration**: The `.github/workflows/ci.yml` is already configured correctly. Do NOT modify it until tests pass locally.

**Test Framework**: The comparison test framework structure is correct. Only the tag name normalization logic and parser implementations need fixes.

**Priority**: The PNG tag namespace issue is the HIGHEST PRIORITY - it's causing 0% match rate on 2 tests. Fix this first to demonstrate quick progress.
