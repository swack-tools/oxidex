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
- **98%+ tag match rate achieved for reads** ❌ FAILING
- Round-trip tests pass (write → read → verify) ✅
- CI runs tests on every commit (with ExifTool installed in CI environment) ✅
- README shows test results badge (pass/fail) ✅

---

## Issues Detected

### Test Failures Summary

**ExifTool Comparison Tests:** 6 out of 14 tests FAILED (57% pass rate)
- ✅ PASSING: test_comparison_jpeg_with_exif (98%+)
- ✅ PASSING: test_comparison_jpeg_with_exif_xmp (98%+)
- ❌ FAILING: test_comparison_tiff (87.50% - below 98% threshold)
- ✅ PASSING: test_comparison_pdf (100%)
- ❌ FAILING: test_comparison_mp4 (0.00% - critical failure)
- ✅ PASSING: test_comparison_png_with_text (100%)
- ❌ FAILING: test_comparison_png_with_exif (54.55% - critical failure)
- ❌ FAILING: test_comparison_tiff_multipage (25.00% - critical failure)
- ❌ FAILING: test_comparison_jpeg_with_gps (26.32% - critical failure)
- ❌ FAILING: test_comparison_tiff_big_endian (35.29% - critical failure)
- ✅ PASSING: test_write_roundtrip_jpeg_artist
- ✅ PASSING: test_copy_metadata_jpeg_to_jpeg
- ✅ PASSING: test_rename_file_pattern
- ✅ PASSING: test_date_shift_all_dates

### Root Causes

#### 1. **Rational Number Formatting Mismatch**
**Severity:** HIGH
**Impact:** TIFF tests failing at 87.50% match rate

**Issue:**
- Perl ExifTool outputs `FNumber` as a decimal number: `2.8` (JSON Number type)
- ExifTool-RS outputs it as a string ratio: `"28/10"` (JSON String type)

**Example from test output:**
```
ExifIFD:FNumber
  Perl:  Number(2.8)
  Rust:  String("28/10")
```

**File:** `src/cli/output_formatter.rs:177-191`

**Current Code:**
```rust
TagValue::Rational {
    numerator,
    denominator,
} => {
    // Normalize rational display to match Perl ExifTool
    if *denominator == 1 {
        serde_json::Value::String(format!("{}", numerator))
    } else if *numerator == 0 {
        serde_json::Value::String("0".to_string())
    } else {
        serde_json::Value::String(format!("{}/{}", numerator, denominator))
    }
}
```

**Problem:** This code only simplifies rationals with denominator=1, but doesn't convert rationals to decimal numbers when Perl ExifTool would output them as numbers.

---

#### 2. **Enum Values Displayed as Numbers Instead of Strings**
**Severity:** HIGH
**Impact:** TIFF tests showing raw enum values instead of human-readable strings

**Issue:**
- Perl ExifTool outputs enum values as human-readable strings: `"Horizontal (normal)"`, `"Uncompressed"`, `"RGB"`, `"Normal"`
- ExifTool-RS outputs raw numeric enum values: `Number(1)`, `Number(1)`, `Number(2)`, `Number(1)`

**Examples from test output:**
```
IFD0:Orientation
  Perl:  String("Horizontal (normal)")
  Rust:  Number(1)

IFD0:Compression
  Perl:  String("Uncompressed")
  Rust:  Number(1)

IFD0:PhotometricInterpretation
  Perl:  String("RGB")
  Rust:  Number(2)

IFD0:FillOrder
  Perl:  String("Normal")
  Rust:  Number(1)
```

**Root Cause:** ExifTool-RS is not translating TIFF enum tag values to their string representations. The tag database or output formatter needs to map these numeric values to their standard TIFF/EXIF enum strings.

---

#### 3. **Missing Array Tags (StripOffsets, StripByteCounts, etc.)**
**Severity:** HIGH
**Impact:** TIFF multipage and big-endian tests critically failing (25-35% match rate)

**Issue:**
- Perl ExifTool outputs array tags like `StripOffsets` and `StripByteCounts` as space-separated strings: `"334 998734 1997134"`
- ExifTool-RS shows these tags as `MISSING`

**Examples from test output:**
```
IFD0:StripOffsets
  Perl:  String("334 998734 1997134")
  Rust:  MISSING

IFD0:StripByteCounts
  Perl:  String("983040 983040 491520")
  Rust:  MISSING
```

**Root Cause:** The TIFF parser is not extracting or storing array-type tags (LONG arrays, SHORT arrays). These tags are critical for multi-page TIFF files and strip-based image storage.

**Files to check:**
- `src/parsers/tiff/` (all TIFF parsing code)
- Look for how arrays are handled in IFD entry parsing

---

#### 4. **Binary Data Not Being Parsed**
**Severity:** MEDIUM
**Impact:** Tags like WhitePoint and BitsPerSample showing as binary instead of values

**Issue:**
- Perl ExifTool parses and displays binary fields as their actual values
- ExifTool-RS shows them as `"(Binary, N bytes)"`

**Examples:**
```
IFD2:WhitePoint
  Perl:  String("0.3127000034 0.3289999962")
  Rust:  String("(Binary, 16 bytes)")

IFD2:BitsPerSample
  Perl:  String("16 16 16 16")
  Rust:  String("(Binary, 8 bytes)")
```

**Root Cause:** The TIFF parser is treating certain structured binary fields as opaque binary data instead of parsing them according to their field type definitions.

---

#### 5. **MP4 Tags Completely Missing**
**Severity:** CRITICAL
**Impact:** MP4 test at 0.00% match rate

**Issue:**
- Perl ExifTool extracts 30 QuickTime metadata tags
- ExifTool-RS extracts 0 matching tags (only shows 8 iTunes tags that Perl doesn't show)

**Missing tags include:**
- `QuickTime:TimeScale`
- `QuickTime:ModifyDate`
- `QuickTime:MinorVersion`
- `QuickTime:CompatibleBrands`
- And 26 more standard QuickTime tags

**ExifTool-RS shows these instead:**
- `iTunes:Artist`, `iTunes:Genre`, `iTunes:Year`, `iTunes:Title`, `iTunes:Copyright`, `iTunes:Comment`, `QuickTime:Title`, `iTunes:Album`

**Root Cause:** The MP4 parser is extracting iTunes metadata but not standard QuickTime metadata atoms. The parser needs to extract both:
1. **Standard QuickTime atoms**: mvhd (movie header), mdhd (media header), etc.
2. **iTunes metadata**: udta/meta/ilst tags

**Files to check:**
- `src/parsers/quicktime/` (MP4/QuickTime parser implementation)

---

#### 6. **PNG eXIf Chunk Parsing Issues**
**Severity:** HIGH
**Impact:** PNG with eXIf test at 54.55% match rate

**Issue:**
- 20 tags show as MISSING in ExifTool-RS
- Tag names have inconsistent namespacing (some show as `PNG:ExifColorSpace`, others as `ExifIFD:ExifVersion`)

**Missing tags include:**
- `PNG:ExifColorSpace`
- `ExifIFD:ExifVersion`
- `IFD0:XResolution`
- `IFD0:YCbCrPositioning`
- `IFD0:Artist`
- `ExifIFD:DateTimeOriginal`
- `ExifIFD:ColorSpace`

**Root Cause:** The PNG parser's eXIf chunk handling is incomplete. When PNG files contain EXIF data in an eXIf chunk (standard PNG EXIF extension), the parser should:
1. Extract the raw EXIF data from the eXIf chunk
2. Parse it using the TIFF/EXIF parser (since eXIf contains TIFF-formatted EXIF data)
3. Prefix tags appropriately (ExifIFD:, IFD0:, etc.)

**Files to check:**
- `src/parsers/png/` (PNG parser)
- Integration with TIFF parser for eXIf chunk data

---

### Other Test Failures (Not Related to I5.T9)

The following tests are also failing but are NOT part of the I5.T9 acceptance criteria (they test write/modify operations, not read comparisons):

- `copy_metadata_tests::*` (6 failures)
- `date_shift_tests::*` (7 failures)
- `jpeg_tests::*`, `jpeg_write_tests::*` (5 failures)
- `mp4_tests::*` (8 failures)
- `pdf_tests::*`, `pdf_write_tests::*` (4 failures)
- `png_tests::*`, `png_write_tests::*` (8 failures)
- `rename_tests::*` (8 failures)
- `tiff_tests::*`, `tiff_write_tests::*` (17 failures)
- `write_operations_tests::*` (10 failures)

**Total other failures:** 73 tests

**Note:** These failures indicate broader issues with write/modify operations and general parsing, but they are separate from the I5.T9 acceptance criteria which focuses specifically on ExifTool comparison tests achieving 98%+ match rate.

---

## Best Approach to Fix

### Priority 1: Fix Rational Number Output (addresses TIFF 87.50% → 98%+)

**File:** `src/cli/output_formatter.rs:177-191`

**Required changes:**
1. Determine if a rational should be output as a decimal number or a fraction string
2. For tags like `FNumber`, `ApertureValue`, `ShutterSpeedValue`, etc., output as JSON Number (decimal)
3. For other rationals, keep as string fractions

**Implementation approach:**
```rust
TagValue::Rational { numerator, denominator } => {
    // Tags that should be displayed as decimal numbers (like Perl ExifTool)
    const DECIMAL_TAGS: &[&str] = &[
        "FNumber", "ApertureValue", "MaxApertureValue", "FocalLength",
        "ExposureCompensation", "BrightnessValue", "SubjectDistance",
        "LensInfo", "DigitalZoomRatio"
    ];

    // Check if this tag should be a decimal (need tag name from context)
    // For now, use heuristic: if ratio reduces to a clean decimal, output as Number
    if *denominator == 1 {
        serde_json::Value::Number((*numerator).into())
    } else if *numerator == 0 {
        serde_json::Value::Number(0.into())
    } else {
        let decimal_value = *numerator as f64 / *denominator as f64;
        // If this looks like a typical F-number or aperture value, output as number
        if decimal_value < 100.0 && decimal_value.fract() != 0.0 {
            serde_json::Value::Number(
                serde_json::Number::from_f64(decimal_value).unwrap_or_else(|| {
                    // Fallback to string if f64 conversion fails
                    return serde_json::Value::String(format!("{}/{}", numerator, denominator));
                })
            )
        } else {
            serde_json::Value::String(format!("{}/{}", numerator, denominator))
        }
    }
}
```

**Alternative approach (better):** Pass tag name context to `tag_value_to_json()` function and use a lookup table to determine decimal vs. fraction output based on tag semantics.

---

### Priority 2: Add Enum String Mapping for TIFF Tags

**Files:**
- `src/parsers/tiff/` (IFD parsing)
- `src/tag_db/` or `src/parsers/tiff/tag_definitions.rs` (new file if needed)

**Required changes:**
1. Create enum mapping tables for standard TIFF tags:
   - Orientation (1=Horizontal (normal), 3=Rotate 180, 6=Rotate 90 CW, 8=Rotate 270 CW, etc.)
   - Compression (1=Uncompressed, 5=LZW, 7=JPEG, 8=Deflate, etc.)
   - PhotometricInterpretation (0=WhiteIsZero, 1=BlackIsZero, 2=RGB, 3=Palette, etc.)
   - PlanarConfiguration (1=Chunky, 2=Planar)
   - FillOrder (1=Normal, 2=Reversed)
   - ResolutionUnit (1=None, 2=inches, 3=cm)
   - SampleFormat (1=Unsigned, 2=Signed, 3=Float)

2. Apply these mappings when outputting tag values (either in parser or in `output_formatter.rs`)

**Implementation approach:**
- Create a function `tiff_enum_to_string(tag_id: u16, value: u32) -> Option<String>`
- Call this function in the TIFF parser when storing tag values
- Store as `TagValue::String(enum_string)` instead of `TagValue::Integer(raw_value)`

**Reference:** Check Perl ExifTool's TIFF.pm for complete enum definitions.

---

### Priority 3: Implement Array Tag Extraction for TIFF

**Files:**
- `src/parsers/tiff/ifd_parser.rs` (or wherever IFD entries are parsed)

**Required changes:**
1. Detect array-type IFD entries (count > 1 for LONG, SHORT, RATIONAL types)
2. Extract all array elements (currently only extracting first element?)
3. Store arrays as space-separated strings to match Perl ExifTool output format

**Tags to handle:**
- `StripOffsets` (tag 273, type LONG)
- `StripByteCounts` (tag 279, type LONG)
- `BitsPerSample` (tag 258, type SHORT)
- `SamplesPerPixel` (tag 277, type SHORT)
- `ExtraSamples` (tag 338, type SHORT)
- `TileOffsets` (tag 324, type LONG)
- `TileByteCounts` (tag 325, type LONG)
- `PageNumber` (tag 297, type SHORT, count=2)

**Example implementation:**
```rust
// When parsing IFD entry with count > 1:
if entry.count > 1 {
    match entry.field_type {
        FieldType::LONG => {
            let mut values = Vec::new();
            for i in 0..entry.count {
                values.push(read_u32_at_offset(...));
            }
            // Store as space-separated string
            TagValue::String(values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "))
        }
        FieldType::SHORT => { /* similar */ }
        // ... other types
    }
}
```

---

### Priority 4: Parse Binary Fields (WhitePoint, BitsPerSample)

**Files:**
- `src/parsers/tiff/ifd_parser.rs`

**Required changes:**
1. For `WhitePoint` (tag 318, type RATIONAL, count=2): Parse as 2 rational values and format as space-separated decimals
2. For `BitsPerSample` (tag 258, type SHORT, count=variable): Already covered by Priority 3 array handling
3. For other binary fields, check TIFF specification for proper parsing

**Example for WhitePoint:**
```rust
if tag_id == 318 { // WhitePoint
    let x = numerator[0] as f64 / denominator[0] as f64;
    let y = numerator[1] as f64 / denominator[1] as f64;
    TagValue::String(format!("{:.10} {:.10}", x, y))
}
```

---

### Priority 5: Fix MP4 QuickTime Metadata Extraction

**Files:**
- `src/parsers/quicktime/` (all MP4/QuickTime parser files)

**Required changes:**
1. Extract standard QuickTime metadata from these atoms:
   - `mvhd` (movie header): TimeScale, Duration, ModifyDate, CreateDate
   - `ftyp` (file type): MajorBrand, MinorVersion, CompatibleBrands
   - `mdhd` (media header): MediaTimeScale, MediaDuration, MediaLanguageCode
   - `hdlr` (handler): HandlerType, HandlerDescription
2. Ensure these are output with `QuickTime:` namespace prefix
3. Keep iTunes metadata extraction (already working)

**Current problem:** Parser is only extracting iTunes metadata (udta/meta/ilst), not standard QuickTime atoms.

**Implementation approach:**
- Add atom parsers for mvhd, ftyp, mdhd, hdlr
- Parse their binary structures according to QuickTime file format specification
- Store extracted values with appropriate tag names (e.g., "QuickTime:TimeScale")

**Reference:** Check Perl ExifTool's QuickTime.pm for tag definitions and atom structures.

---

### Priority 6: Fix PNG eXIf Chunk Parsing

**Files:**
- `src/parsers/png/` (PNG parser)
- Integration with `src/parsers/tiff/` (reuse TIFF parser for EXIF data)

**Required changes:**
1. Detect `eXIf` chunks in PNG files
2. Extract the raw EXIF data from the chunk (it's TIFF-formatted)
3. Parse it using the existing TIFF/EXIF parser
4. Merge the EXIF tags into the PNG metadata with proper namespace prefixes

**Implementation approach:**
```rust
// In PNG parser:
if chunk_type == "eXIf" {
    // eXIf chunk contains TIFF-formatted EXIF data
    let exif_data = chunk.data;

    // Parse using TIFF parser
    let exif_metadata = parse_tiff_metadata_from_bytes(&exif_data)?;

    // Merge into metadata map with PNG:exif: prefix or proper IFD prefixes
    for (tag_name, value) in exif_metadata {
        metadata.insert(format!("PNG:exif:{}", tag_name), value);
        // OR keep IFD prefixes: "IFD0:Make", "ExifIFD:DateTimeOriginal"
    }
}
```

**Note:** Check how Perl ExifTool prefixes PNG eXIf tags to ensure consistency.

---

## Testing Validation

After implementing the fixes, run:

```bash
cargo test --features exiftool-comparison exiftool_comparison_tests -- --nocapture
```

**Expected results:**
- `test_comparison_tiff`: Match rate ≥ 98% (currently 87.50%)
- `test_comparison_mp4`: Match rate ≥ 98% (currently 0.00%)
- `test_comparison_png_with_exif`: Match rate ≥ 98% (currently 54.55%)
- `test_comparison_tiff_multipage`: Match rate ≥ 98% (currently 25.00%)
- `test_comparison_jpeg_with_gps`: Match rate ≥ 98% (currently 26.32%)
- `test_comparison_tiff_big_endian`: Match rate ≥ 98% (currently 35.29%)

All 14 ExifTool comparison tests must pass with ≥98% match rates.

---

## Success Criteria

The task is complete when:
1. ✅ All 14 ExifTool comparison tests pass
2. ✅ Each test achieves ≥98% tag match rate
3. ✅ `cargo clippy --all-features -- -D warnings` passes (already passing)
4. ✅ CI integration tests run and pass on all platforms

**Current status:** 8/14 tests passing, 6 tests failing due to issues above.
