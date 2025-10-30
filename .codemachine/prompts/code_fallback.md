# Code Refinement Task

The previous code submission did not pass verification. The integration tests are failing due to missing core functionality in the ExifTool-RS implementation, not due to test code issues.

---

## Original Task Description

**Task I5.T9**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

---

## Issues Detected

### Test Failures (7 out of 14 tests failing)

**Failed Tests:**
1. `test_comparison_jpeg_with_exif_xmp` - Match rate: 33.33% (expected: 98%+)
2. `test_comparison_jpeg_with_gps` - Match rate: 22.73% (expected: 98%+)
3. `test_comparison_tiff` - Match rate below 98%
4. `test_comparison_tiff_big_endian` - Match rate below 98%
5. `test_comparison_tiff_multipage` - Match rate below 98%
6. `test_comparison_png_with_exif` - Match rate below 98%
7. `test_comparison_mp4` - Match rate below 98%

**Root Causes:**

#### 1. Missing XMP Support (CRITICAL)
- ExifTool-RS is not parsing XMP metadata from JPEG files
- Missing tags: `XMP-xmp:Creator`, `XMP-xmp:Rating`, `XMP-dc:Title`, `XMP-dc:Rights`
- This is blocking 4 out of 6 tags in the EXIF+XMP test
- **Impact**: Cannot achieve 98% match rate on any file with XMP data

#### 2. Format Differences in Tag Value Display
- **Rational Values**: ExifTool-RS outputs "1/1", Perl ExifTool outputs "1" (normalized)
  - Affects: `IFD0:XResolution`, `IFD0:YResolution`, `GPS:GPSAltitude`
- **Enum Values**: ExifTool-RS outputs raw numbers, Perl ExifTool outputs descriptive strings
  - Example: `IFD0:YCbCrPositioning` shows "1" instead of "Centered"
  - Example: `JFIF:ResolutionUnit` missing (shown as "None" by Perl)
- **GPS Coordinates**: ExifTool-RS missing formatted GPS position strings
  - Perl ExifTool provides: `Composite:GPSPosition` = "37 deg 46' 33.24\", 122 deg 25' 6.24\""

#### 3. Missing JFIF Metadata Extraction
- ExifTool-RS is not extracting JFIF tags from JPEG files
- Missing tags: `JFIF:JFIFVersion`, `JFIF:XResolution`, `JFIF:YResolution`, `JFIF:ResolutionUnit`
- **Note**: These are actual embedded tags in JPEG files, not pseudo-tags

#### 4. Missing Composite/Derived Tags
- Perl ExifTool calculates derived tags: `Composite:Megapixels`, `Composite:ImageSize`, `Composite:GPSPosition`
- ExifTool-RS should skip comparison of these tags OR implement the same derived tag calculation
- Current implementation doesn't handle these properly

---

## Best Approach to Fix

You MUST address these issues in priority order:

### Priority 1: Fix XMP Metadata Extraction (BLOCKING)

**File**: `src/parsers/jpeg/segment_parser.rs` (or wherever JPEG parsing logic resides)

1. Locate the JPEG segment parsing code that handles APP1 markers
2. Check if XMP segment detection exists (XMP is in APP1 with identifier "http://ns.adobe.com/xap/1.0/\0")
3. If XMP detection is missing, add it:
   - Detect XMP namespace identifier in APP1 segments
   - Extract XMP XML data
   - Parse XMP XML to extract metadata fields
4. Map XMP fields to the tag database with proper group names (XMP-xmp, XMP-dc, XMP-exif)
5. Test with `tests/fixtures/jpeg/sample_with_exif_xmp.jpg`

**Expected Result**: `test_comparison_jpeg_with_exif_xmp` should go from 33% match to 98%+ match

### Priority 2: Normalize Rational Value Display

**File**: `src/formatters/json_formatter.rs` or `src/data_model/tag_value.rs`

1. Locate where `TagValue::Rational` is converted to string
2. Add normalization logic:
   - If denominator is 1, output only the numerator (e.g., "100/1" → "100")
   - If numerator is 0, output "0"
   - Otherwise, keep fractional form "n/d"
3. This matches Perl ExifTool's behavior

**Expected Result**: Rational value mismatches should be eliminated

### Priority 3: Add Enum Value Descriptions

**File**: `src/tag_db/tag_lookup.rs` or where tag values are formatted

1. Add a lookup table mapping (tag_id, raw_value) → descriptive_string
2. Common enums to handle:
   - `YCbCrPositioning`: 1 = "Centered", 2 = "Co-sited"
   - `ResolutionUnit`: 1 = "None", 2 = "inches", 3 = "cm"
   - `ColorSpace`: 1 = "sRGB", 65535 = "Uncalibrated"
   - `Orientation`: 1 = "Horizontal (normal)", 3 = "Rotate 180", 6 = "Rotate 90 CW", etc.
3. Use these descriptions in JSON output to match Perl ExifTool

**Expected Result**: Enum-related mismatches should be reduced significantly

### Priority 4: Add JFIF Metadata Extraction

**File**: `src/parsers/jpeg/segment_parser.rs`

1. Detect JFIF APP0 segment (marker 0xFFE0, identifier "JFIF\0")
2. Parse JFIF structure:
   - Version (2 bytes): e.g., 0x0101 → "1.01"
   - Units (1 byte): 0 = None, 1 = DPI, 2 = DPC
   - X/Y density (2 bytes each)
   - Thumbnail data (optional)
3. Add JFIF tags to output with "JFIF:" group prefix
4. Tags: `JFIF:JFIFVersion`, `JFIF:ResolutionUnit`, `JFIF:XResolution`, `JFIF:YResolution`

**Expected Result**: JFIF-related mismatches eliminated

### Priority 5: Handle Composite Tags Properly

**File**: `tests/integration/exiftool_comparison_tests.rs`

Update the `should_skip_tag()` function to skip Composite namespace tags:

```rust
fn should_skip_tag(tag_name: &str) -> bool {
    // Skip System: namespace (filesystem metadata)
    if tag_name.starts_with("System:") {
        return true;
    }

    // Skip File: namespace (format metadata added by ExifTool, not from file)
    if tag_name.starts_with("File:") {
        return true;
    }

    // Skip ExifTool: namespace (tool metadata)
    if tag_name.starts_with("ExifTool:") {
        return true;
    }

    // Skip Composite: namespace (derived tags calculated by Perl ExifTool)
    if tag_name.starts_with("Composite:") {
        return true;
    }

    // Skip specific metadata fields
    if tag_name == "SourceFile" {
        return true;
    }

    false
}
```

**Expected Result**: Composite-tag mismatches no longer counted

---

## Implementation Strategy

1. **Start with XMP** (highest impact) - This alone will fix the EXIF+XMP test
2. **Then Rational normalization** (quick win) - Affects multiple tests
3. **Then Enum descriptions** (medium effort) - Improves match rates across the board
4. **Then JFIF extraction** (JPEG-specific) - Fixes JPEG GPS test issues
5. **Finally Composite skip** (test-side fix) - Improves all test match rates

---

## Verification Steps

After implementing each fix:

1. Run specific test: `cargo test --features exiftool-comparison test_comparison_jpeg_with_exif_xmp -- --nocapture`
2. Check match rate in output
3. Review mismatch list to identify remaining issues
4. Proceed to next priority

**Target**: All 14 tests passing with 98%+ match rate (or 85%+ for write operation tests where lower threshold is acceptable)

---

## Additional Context

- **Linting**: No clippy errors detected - code quality is good
- **Test Infrastructure**: All 14 tests are properly implemented and execute correctly
- **Test Corpus**: 102 images across 5 formats - exceeds 100+ requirement
- **CI Integration**: Already configured and working
- **Write Operation Tests**: 7 of 14 tests pass (including all 4 write operation tests using 85% threshold)

The issue is NOT with the tests themselves, but with the missing parser features in the ExifTool-RS implementation. The tests are correctly identifying that the Rust implementation doesn't yet support all the metadata formats that Perl ExifTool supports.

---

## Files to Modify

1. **XMP Parser**: `src/parsers/jpeg/segment_parser.rs` or `src/parsers/xmp/` (if separate module)
2. **Value Formatter**: `src/formatters/json_formatter.rs` or `src/data_model/tag_value.rs`
3. **Enum Lookup**: `src/tag_db/tag_lookup.rs` or create new `src/tag_db/enum_descriptions.rs`
4. **JFIF Parser**: `src/parsers/jpeg/segment_parser.rs`
5. **Test Helper**: `tests/integration/exiftool_comparison_tests.rs` (add Composite: to skip list)

---

## Success Criteria

- ✅ `cargo test --features exiftool-comparison` passes with 14/14 tests passing
- ✅ All read operation tests (10 tests) achieve 98%+ match rate
- ✅ All write operation tests (4 tests) achieve 85%+ match rate
- ✅ No linting errors (`cargo clippy --all-features -- -D warnings`)
- ✅ No compilation errors
