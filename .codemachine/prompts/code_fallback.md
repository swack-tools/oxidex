# Code Refinement Task

The previous code submission did not pass verification. The integration test suite is well-implemented with 104 test images and comprehensive coverage, and **13 out of 14 tests pass with 100% match rate**. However, **1 test (MP4) fails** with only 73.33% match rate, below the required 98% threshold.

---

## Original Task Description

**Task I5.T9**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations.

**Acceptance threshold**: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

---

## Issues Detected

### Test Results Summary:
- ✅ **13/14 tests passing** with 100% match rate:
  - test_comparison_jpeg_with_exif: 100%
  - test_comparison_jpeg_with_exif_xmp: 100%
  - test_comparison_jpeg_with_gps: 100%
  - test_comparison_tiff: 100%
  - test_comparison_tiff_big_endian: 100%
  - test_comparison_tiff_multipage: 100%
  - test_comparison_png_with_text: 100%
  - test_comparison_png_with_exif: 100%
  - test_comparison_pdf: 100%
  - test_write_roundtrip_jpeg_artist: 100%
  - test_rename_file_pattern: 100%
  - test_copy_metadata_jpeg_to_jpeg: 100%
  - test_date_shift_all_dates: 100%

- ❌ **1/14 tests failing** below 98% threshold:
  - test_comparison_mp4: **73.33%** (22/30 tags matched, 8 mismatches)

### MP4 Test Failure Details:

The MP4 parser is missing 8 ItemList metadata tags that Perl ExifTool extracts correctly:

1. **ItemList:Album** - Perl: `"Sample Album"`, Rust: MISSING
2. **ItemList:Artist** - Perl: `"Sample Artist"`, Rust: MISSING
3. **ItemList:Title** - Perl: `"Sample Video Title"`, Rust: MISSING
4. **ItemList:Comment** - Perl: `"Test MP4 file for ExifTool-RS"`, Rust: MISSING
5. **ItemList:ContentCreateDate** - Perl: `Number(2024)`, Rust: MISSING
6. **ItemList:Genre** - Perl: `"Test Genre"`, Rust: MISSING
7. **ItemList:Copyright** - Perl: `"Copyright 2024"`, Rust: MISSING
8. **UserData:Title** - Perl: `"QT Title!!"`, Rust: MISSING

**Additional Context:**
- ExifTool-RS extracts tags with `iTunes:` prefix instead of `ItemList:` prefix
- The test shows warnings: "ExifTool-RS has additional tag not in Perl ExifTool: iTunes:Artist", etc.
- This indicates the parser IS extracting the data, but using incorrect namespace prefix

### Linting Issues:
- ✅ **FIXED**: Collapsible if statements in `src/core/operations.rs:743` and `src/core/operations.rs:756` have been corrected

---

## Best Approach to Fix

You must **modify the MP4 parser** to use the correct tag namespace prefix that matches Perl ExifTool's conventions.

### Root Cause Analysis:

The MP4 parser is extracting ItemList (`ilst`) metadata correctly, but outputting tags with the `iTunes:` prefix instead of the `ItemList:` prefix that Perl ExifTool uses.

**Example:**
- Current (incorrect): `iTunes:Artist`
- Expected (correct): `ItemList:Artist`

Additionally, the parser is not extracting the `UserData:Title` tag from the `udta` atom.

### Fix Instructions:

**File**: `src/parsers/mp4/mod.rs`

**Action 1: Fix ItemList Tag Namespace**

Locate the code that adds ItemList tags to the metadata map. It's likely using a prefix like `"iTunes:"` or similar. Change it to use `"ItemList:"` instead.

Look for code patterns like:
```rust
metadata.insert("iTunes:Artist", ...);
// or
format!("iTunes:{}", tag_name)
```

Change to:
```rust
metadata.insert("ItemList:Artist", ...);
// or
format!("ItemList:{}", tag_name)
```

**Action 2: Extract UserData:Title**

The parser needs to also extract the `Title` tag from the `udta` (UserData) atom, not just from the `ilst` (ItemList) atom.

1. Ensure the `udta` atom handler is traversing its child atoms
2. Look for a `title` atom (or similar) inside `udta`
3. Extract the string value and add it as `UserData:Title`

**Atom Structure Reference:**
```
moov
  ├─ udta (UserData)
  │   └─ title → extract as "UserData:Title"
  └─ meta
      └─ ilst (ItemList)
          ├─ ©nam → "ItemList:Title"
          ├─ ©ART → "ItemList:Artist"
          ├─ ©alb → "ItemList:Album"
          ├─ ©cmt → "ItemList:Comment"
          ├─ ©day → "ItemList:ContentCreateDate"
          ├─ ©gen → "ItemList:Genre"
          └─ cprt → "ItemList:Copyright"
```

**Mapping Reference:**
- `©nam` (0xA96E616D) → `ItemList:Title`
- `©ART` (0xA9415254) → `ItemList:Artist`
- `©alb` (0xA9616C62) → `ItemList:Album`
- `©cmt` (0xA9636D74) → `ItemList:Comment`
- `©day` (0xA9646179) → `ItemList:ContentCreateDate`
- `©gen` (0xA967656E) → `ItemList:Genre`
- `cprt` (0x63707274) → `ItemList:Copyright`

---

## Testing Instructions

After making the fix, run the MP4 test to verify:

```bash
# Test the specific failing test
cargo test --features exiftool-comparison test_comparison_mp4 -- --nocapture

# Run all comparison tests to ensure no regressions
cargo test --features exiftool-comparison exiftool_comparison_tests -- --nocapture
```

**Success criteria**:
- `test_comparison_mp4` must show match rate ≥98% (ideally 100%)
- All 14 comparison tests must pass with `ok` status
- No linting errors from `cargo clippy --all-features -- -D warnings`

---

## Files to Modify

1. `src/parsers/mp4/mod.rs` - Change ItemList tag prefix from `iTunes:` to `ItemList:`, add UserData:Title extraction

---

## Important Notes

- **DO NOT modify** `tests/integration/exiftool_comparison_tests.rs` - the test framework is correct
- **DO NOT modify** `.github/workflows/ci.yml` - CI configuration is correct
- **DO NOT add** more test fixtures - 104 images is sufficient
- **Focus on namespace prefix correction** - this is a simple string replacement issue
- The test currently shows warnings about "ExifTool-RS has additional tag not in Perl ExifTool: iTunes:Artist" - after the fix, these warnings should disappear and the tags should match

---

## Expected Outcome

After this fix:
- **14/14 tests passing** with ≥98% match rate (target: 100%)
- MP4 test match rate: 73.33% → 100%
- All acceptance criteria met:
  - ✅ Test corpus: 104 images
  - ✅ Format coverage: JPEG, TIFF, PNG, PDF, MP4
  - ✅ Operation coverage: read, write, copy, rename, date shift
  - ✅ Match rate: 98%+ for all read tests
  - ✅ Round-trip tests: passing
  - ✅ CI integration: configured
  - ✅ README badges: present
  - ✅ No linting errors
