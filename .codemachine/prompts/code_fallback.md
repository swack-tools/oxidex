# Code Refinement Task

The previous code submission did not pass verification. The integration test suite is well-implemented with 104 test images and comprehensive coverage, but **5 out of 14 ExifTool comparison tests FAIL** the required 98% tag match rate threshold.

---

## Original Task Description

**Task I5.T9**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations.

**Acceptance threshold**: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

---

## Issues Detected

The integration test infrastructure is complete (104 images, 14 test functions, CI configured), but the **underlying parsers have value formatting mismatches** that prevent achieving the 98% match rate:

### Test Failures (5/14 tests below 98% threshold):

1. **`test_comparison_tiff`**: 87.50% match rate
   - **Issue**: `ExifIFD:ExposureTime` - Perl outputs `"1/100"`, Rust outputs `Number(0.01)`
   - **Root cause**: Rational number formatting difference

2. **`test_comparison_pdf`**: 90.91% match rate
   - **Issue**: `PDF:Keywords` - Perl outputs `Array [String("exiftool"), String("rust"), ...]`, Rust outputs `String("exiftool, rust, pdf, metadata, test")`
   - **Root cause**: Array vs comma-separated string representation

3. **`test_comparison_mp4`**: 73.33% match rate (22/30 tags matched)
   - **Issues**: 8 missing tags: `ItemList:Title`, `ItemList:Comment`, `ItemList:Genre`, `ItemList:ContentCreateDate`, `ItemList:Copyright`, `ItemList:Album`, `ItemList:Artist`, `UserData:Title`
   - **Root cause**: MP4 parser not extracting ItemList (ilst) metadata atoms correctly

4. **`test_comparison_png_with_exif`**: 97.73% match rate (just below 98%)
   - **Issue**: `PNG:ExifExifVersion` - Perl outputs `"0232"`, Rust outputs `"..."`
   - **Root cause**: ExifVersion tag value formatting in PNG eXIf chunk parser

5. **`test_comparison_jpeg_with_gps`**: 52.63% match rate
   - **Issues**:
     - GPS coordinates: Perl outputs `"37 deg 46' 33.24\""`, Rust outputs `"37.0000000000 46.0000000000 33.2400000000"`
     - JFIF tags missing: `JFIF:JFIFVersion`, `JFIF:ResolutionUnit`, `JFIF:XResolution`, `JFIF:YResolution`
     - GPS version: Perl outputs `"2.3.0.0"`, Rust outputs `Number(770)`
     - ComponentsConfiguration: Perl outputs `"Y, Cb, Cr, -"`, Rust outputs `Number(197121)`
   - **Root cause**: GPS coordinate formatting, JFIF segment not parsed, enum tag value decoding

### Additional Issues:

6. **Linting Error Fixed**: `src/parsers/png/mod.rs:444` - Unused variable `value_count` was prefixed with underscore

---

## Best Approach to Fix

You must **modify the format-specific parsers** to output tag values in formats that match Perl ExifTool's conventions. The comparison test framework is correct—the parsers need adjustment.

### Priority 1: Fix GPS Coordinate Formatting (JPEG GPS test: 52.63% → 98%+)

**File**: `src/parsers/tiff/ifd_parser.rs` or GPS value formatter

**Action**: When outputting GPS coordinates (GPSLatitude, GPSLongitude), format as DMS (degrees/minutes/seconds) string instead of raw rational array:
- Change: `"37.0000000000 46.0000000000 33.2400000000"`
- To: `"37 deg 46' 33.24\""`

**Also fix**:
- `GPS:GPSVersionID`: Convert byte array `[2, 3, 0, 0]` → string `"2.3.0.0"`
- `GPS:GPSAltitude`: Append unit " m" (e.g., `"110"` → `"110 m"`)

### Priority 2: Add JFIF Segment Parser (JPEG GPS test: 52.63% → 98%+)

**File**: `src/parsers/jpeg/mod.rs`

**Action**: The JPEG parser currently skips JFIF (APP0) segments. You must:
1. Detect APP0 JFIF marker (0xFFE0 with "JFIF\0" identifier)
2. Extract: `JFIFVersion` (2 bytes → format as X.YY, e.g., `[1, 1]` → `Number(1.01)`)
3. Extract: `ResolutionUnit` (1 byte → decode: 0="None", 1="inches", 2="cm")
4. Extract: `XResolution`, `YResolution` (2 bytes each, big-endian unsigned short)
5. Namespace tags as `JFIF:TagName`

### Priority 3: Fix Rational Number Display (TIFF test: 87.50% → 98%+)

**File**: `src/parsers/common/exif_types.rs` or `src/core/metadata.rs`

**Action**: When a tag is defined as RATIONAL type (e.g., ExposureTime), output as fraction string instead of decimal:
- Change: `TagValue::Number(0.01)`
- To: `TagValue::String("1/100")`

**Implementation**: Add a function to convert float back to simplified fraction (use GCD algorithm). Apply to known RATIONAL tags: ExposureTime, ShutterSpeedValue, ApertureValue, FocalLength, etc.

### Priority 4: Fix Enum Tag Decoding (JPEG GPS test)

**File**: `src/parsers/tiff/ifd_parser.rs` or `src/tag_db/mod.rs`

**Action**: Decode numeric enum values to human-readable strings for specific tags:
- `ExifIFD:ComponentsConfiguration`: Decode bytes `[1, 2, 3, 0]` → `"Y, Cb, Cr, -"` (use lookup: 0="-", 1="Y", 2="Cb", 3="Cr", 4="R", 5="G", 6="B")

### Priority 5: Fix MP4 ItemList Parsing (MP4 test: 73.33% → 98%+)

**File**: `src/parsers/mp4/mod.rs`

**Action**: The MP4 parser is not extracting `ilst` (ItemList) atoms correctly. You must:
1. Ensure `ilst` atom handler traverses all child atoms (©nam, ©cmt, ©gen, ©day, cprt, ©alb, ©ART)
2. Parse data atoms inside each tag atom (skip type/flags, extract UTF-8 string)
3. Map to standard tag names: `©nam` → `ItemList:Title`, `©ART` → `ItemList:Artist`, etc.
4. Also extract `UserData:Title` from `udta` atom if present

### Priority 6: Fix PDF Keywords Array (PDF test: 90.91% → 98%+)

**File**: `src/parsers/pdf/info_parser.rs`

**Action**: Parse `/Keywords` as array instead of string when it contains commas:
- Change: `TagValue::String("exiftool, rust, pdf, metadata, test")`
- To: `TagValue::Array(vec![TagValue::String("exiftool"), TagValue::String("rust"), ...])`

**Implementation**: Split on `, ` delimiter and create array of string values.

### Priority 7: Fix PNG ExifVersion (PNG eXIf test: 97.73% → 98%+)

**File**: `src/parsers/png/mod.rs` (eXIf chunk handler)

**Action**: The ExifVersion tag (0x9000) should output raw byte values as string, not ellipsis:
- Change: `TagValue::String("...")`
- To: `TagValue::String("0232")` (hex representation of bytes [0x30, 0x32, 0x33, 0x32])

**Implementation**: In `raw_bytes_to_tag_value_no_enum()` function, check if tag is ExifVersion (0x9000) and format bytes as ASCII string.

---

## Testing Instructions

After making each fix, run the specific failing test to verify:

```bash
# Test individual failures
cargo test --features exiftool-comparison test_comparison_jpeg_with_gps -- --nocapture
cargo test --features exiftool-comparison test_comparison_tiff -- --nocapture
cargo test --features exiftool-comparison test_comparison_mp4 -- --nocapture
cargo test --features exiftool-comparison test_comparison_pdf -- --nocapture
cargo test --features exiftool-comparison test_comparison_png_with_exif -- --nocapture

# Run all comparison tests
cargo test --features exiftool-comparison exiftool_comparison_tests -- --nocapture
```

**Success criteria**: All 14 comparison tests must show `ok` status with match rates ≥98% for read operations.

---

## Files to Modify (in priority order)

1. `src/parsers/jpeg/mod.rs` - Add JFIF segment parser
2. `src/parsers/tiff/ifd_parser.rs` - Fix GPS coordinate formatting, enum decoding
3. `src/parsers/common/exif_types.rs` - Add rational-to-fraction converter
4. `src/parsers/mp4/mod.rs` - Fix ItemList atom extraction
5. `src/parsers/pdf/info_parser.rs` - Fix Keywords array parsing
6. `src/parsers/png/mod.rs` - Fix ExifVersion formatting in eXIf chunks

---

## Important Notes

- **DO NOT modify** `tests/integration/exiftool_comparison_tests.rs` - the test framework is correct
- **DO NOT modify** `.github/workflows/ci.yml` - CI configuration is correct
- **DO NOT add** more test fixtures - 104 images is sufficient
- **Focus on parser output formatting** to match Perl ExifTool conventions
- The 98% threshold is strict by design - aim for 99%+ where possible
