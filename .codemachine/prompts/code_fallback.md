# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task I5.T9**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

**Acceptance Criteria**:
- Test corpus contains 100+ diverse images
- Tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4)
- Tests cover all operations (read, write, copy, rename, date shift)
- 98%+ tag match rate achieved for reads
- Round-trip tests pass (write → read → verify)
- CI runs tests on every commit (with ExifTool installed in CI environment)
- README shows test results badge (pass/fail)

---

## Issues Detected

The integration test suite is well-implemented but has **6 critical test failures** that prevent meeting the 98%+ match rate requirement:

### Test Failures

1. **test_comparison_mp4** - 0.00% match rate
   - Root cause: MP4 parser not returning any metadata tags
   - All 30 QuickTime and ItemList tags are MISSING in Rust output
   - Expected tags: QuickTime:TimeScale, ItemList:Artist, ItemList:Title, etc.
   - Error: "Match rate 0.00% below 98% threshold. 30 mismatches out of 30 tags."

2. **test_comparison_tiff** - 76-88% match rate
   - Root cause: Missing array handling for multi-value tags
   - Missing tags: IFD0:StripOffsets, IFD0:StripByteCounts (array values)
   - Format differences: WhitePoint, PrimaryChromaticities (floating-point precision)
   - Error: "Match rate below 98% threshold"

3. **test_comparison_tiff_big_endian** - 82.35% match rate
   - Root cause: Same as test_comparison_tiff plus byte order issues
   - Missing tags: IFD0:StripOffsets, IFD0:StripByteCounts
   - Format differences: SamplesPerPixel ("3 3 3" vs Number(3))
   - Error: "Match rate 82.35% below 98% threshold. 3 mismatches out of 17 tags."

4. **test_comparison_tiff_multipage** - 76.92% match rate
   - Root cause: Multi-page TIFF not parsing all IFDs correctly
   - Missing tags: IFD0/IFD1/IFD2 StripOffsets and StripByteCounts arrays
   - Format differences: SubfileType (Number(2) vs String("Single page of multi-page image"))
   - Error: "Match rate 76.92% below 98% threshold"

5. **test_comparison_jpeg_with_gps** - Match rate below threshold
   - Root cause: GPS coordinate formatting and JFIF tag handling
   - Format differences:
     - GPS:GPSLatitude: "37 deg 46' 33.24\"" (Perl) vs "37.0000000000 46.0000000000 33.2400000000" (Rust)
     - GPS:GPSLongitude: "122 deg 25' 6.24\"" (Perl) vs "122.0000000000 25.0000000000 6.2400000000" (Rust)
   - Missing tags: JFIF:JFIFVersion, JFIF:XResolution, JFIF:YResolution, JFIF:ResolutionUnit

6. **test_comparison_png_with_exif** - Match rate below threshold
   - Root cause: PNG eXIf chunk parsing incomplete
   - Missing or incorrectly formatted EXIF tags embedded in PNG
   - Warning: "ExifTool-RS has additional tag not in Perl ExifTool: PNG:exif:Model"

### Compilation Fixes Applied

- ✅ Fixed tuple destructuring errors in `tests/integration/tiff_tests.rs` (7 locations)
- ✅ Fixed tuple destructuring errors in `src/writers/tiff_writer.rs` (6 locations)
- All patterns changed from `(id, _, _)` and `(_, _, value)` to `(id, _, _, _)` and `(_, _, _, value)` to match 4-element tuple type

### Successful Tests (8/14 passed)

- ✅ test_comparison_jpeg_with_exif - PASSED
- ✅ test_comparison_jpeg_with_exif_xmp - PASSED
- ✅ test_comparison_pdf - 100% match rate
- ✅ test_comparison_png_with_text - PASSED
- ✅ test_write_roundtrip_jpeg_artist - 100% match rate
- ✅ test_copy_metadata_jpeg_to_jpeg - 42.11% match rate (intentionally lenient threshold)
- ✅ test_rename_file_pattern - 100% match rate
- ✅ test_date_shift_all_dates - 100% match rate

---

## Best Approach to Fix

### Priority 1: Fix MP4 Parser (Critical - 0% match rate)

**File**: `src/parsers/mp4/mod.rs` or equivalent MP4 parser module

**Problem**: MP4 parser is not extracting any metadata tags from MP4 files.

**Required Fix**:
1. Verify MP4 parser is being invoked in the main metadata extraction pipeline
2. Ensure QuickTime atoms are being read correctly (moov, udta, meta)
3. Implement ItemList (ilst) tag extraction for iTunes metadata:
   - ItemList:Artist, ItemList:Title, ItemList:Album
   - ItemList:Genre, ItemList:Comment, ItemList:Copyright
   - ItemList:ContentCreateDate
4. Implement QuickTime metadata extraction:
   - QuickTime:Duration, QuickTime:TimeScale
   - QuickTime:CreateDate, QuickTime:ModifyDate
   - QuickTime:MajorBrand, QuickTime:MinorVersion
   - QuickTime:HandlerType, QuickTime:PreferredRate
5. Ensure UserData tags are extracted: UserData:Title

**Test Command**: `cargo test --features exiftool-comparison test_comparison_mp4 -- --nocapture`

### Priority 2: Fix TIFF Array Tag Handling

**File**: `src/parsers/tiff/ifd_parser.rs` and `src/cli/output_formatter.rs`

**Problem**: TIFF tags with multiple values (StripOffsets, StripByteCounts) are not being serialized to JSON output.

**Required Fix**:
1. In `ifd_parser.rs`: Ensure tags with count > 1 store all array values
2. In `output_formatter.rs`: Format multi-value tags as space-separated strings to match Perl ExifTool format:
   - Example: `"StripOffsets": "954 983994 1967034"` (Perl format)
   - Instead of: `"StripOffsets": [954, 983994, 1967034]` (JSON array)
3. Handle special cases:
   - RATIONAL arrays: e.g., PrimaryChromaticities (6 values)
   - LONG arrays: e.g., StripOffsets (variable count)

**Test Command**: `cargo test --features exiftool-comparison test_comparison_tiff -- --nocapture`

### Priority 3: Fix GPS Coordinate Formatting

**File**: `src/cli/output_formatter.rs`

**Problem**: GPS coordinates are output as decimal values instead of DMS (degrees, minutes, seconds) format.

**Required Fix**:
1. Detect GPS latitude/longitude tags (GPS:GPSLatitude, GPS:GPSLongitude)
2. Format as: `"37 deg 46' 33.24\""` instead of `"37.0000000000 46.0000000000 33.2400000000"`
3. GPS altitude should be formatted as: `"110 m"` instead of `"110"`
4. GPS version should be: `"2.3.0.0"` instead of `Number(770)`

### Priority 4: Add JFIF Tag Support

**File**: `src/parsers/jpeg/mod.rs`

**Problem**: JFIF metadata segment is not being parsed.

**Required Fix**:
1. Detect and parse APP0 JFIF segment
2. Extract JFIF metadata:
   - JFIF:JFIFVersion (e.g., 1.01)
   - JFIF:XResolution, JFIF:YResolution
   - JFIF:ResolutionUnit
3. Include in JSON output with "JFIF:" prefix

### Priority 5: Fix SubfileType Display Format

**File**: `src/cli/output_formatter.rs`

**Problem**: SubfileType is output as Number(2) instead of descriptive string.

**Required Fix**:
1. Add lookup table for SubfileType values:
   - 0 → "Full-resolution image"
   - 1 → "Reduced-resolution image"
   - 2 → "Single page of multi-page image"
2. Format as string in JSON output to match Perl ExifTool

### Priority 6: Fix Multi-Page TIFF IFD Traversal

**File**: `src/parsers/tiff/file_parser.rs`

**Problem**: Multi-page TIFF files only parse first IFD, missing IFD1 and IFD2.

**Required Fix**:
1. In `parse_tiff_file()`, ensure the IFD chain is fully traversed:
   - Read IFD0, check next_ifd_offset
   - If next_ifd_offset != 0, read IFD1
   - Continue until next_ifd_offset == 0
2. Prefix tags with IFD number: "IFD0:Make", "IFD1:ImageWidth", "IFD2:SubfileType"

---

## Verification Steps

After implementing fixes, run the following commands to verify:

1. **Build**: `cargo build --release`
2. **Run all comparison tests**: `cargo test --release --features exiftool-comparison exiftool_comparison -- --nocapture`
3. **Verify match rates**:
   - MP4: Should achieve 98%+ match rate
   - TIFF: Should achieve 98%+ match rate
   - TIFF big-endian: Should achieve 98%+ match rate
   - TIFF multi-page: Should achieve 98%+ match rate
   - JPEG with GPS: Should achieve 98%+ match rate
   - PNG with eXIf: Should achieve 98%+ match rate
4. **Check clippy**: `cargo clippy --all-targets --features exiftool-comparison`

---

## Success Criteria

All 14 exiftool_comparison tests must pass with:
- Read operations: 98%+ match rate
- Write operations: 98%+ match rate (already passing)
- Copy operations: 20%+ match rate (already passing - intentionally lenient)
- Rename operations: 85%+ match rate (already passing)
- Date shift operations: 85%+ match rate (already passing)

**Current Status**: 8/14 tests passing (57%)
**Target Status**: 14/14 tests passing (100%)

---

## Additional Notes

- The test infrastructure is excellent and does not need changes
- Focus on parser implementations and output formatting only
- Maintain existing test thresholds (do NOT lower them to make tests pass)
- The 104-image test corpus is complete and sufficient
- CI integration is properly configured
- README badge is present

Priority order: MP4 (0% → 98%) > TIFF arrays > GPS formatting > JFIF > SubfileType > Multi-page TIFF
