# Code Refinement Task

The previous code submission did not pass verification. The integration test suite is failing with match rates well below the 98% threshold for most formats. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task I5.T9: Comprehensive Integration Testing Against ExifTool**

Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

**Acceptance Criteria:**
- Test corpus contains 100+ diverse images ✅ (104 images present)
- Tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4) ✅ (Tests exist)
- Tests cover all operations (read, write, copy, rename, date shift) ⚠️ (Write operations have TODO placeholders)
- **98%+ tag match rate achieved for reads ❌ FAILING**
- Round-trip tests pass (write → read → verify) ⚠️ (Not implemented - I4 dependencies)
- CI runs tests on every commit ✅ (CI configured)
- README shows test results badge ✅ (Badge present)

---

## Issues Detected

### Critical Test Failures (9 out of 10 tests failing)

**Test Results Summary:**
- ✅ **PASS**: `test_comparison_jpeg_with_exif` - 100.00% match rate (3/3 tags)
- ❌ **FAIL**: `test_comparison_jpeg_with_exif_xmp` - 33.33% match rate (2/6 tags) - **THRESHOLD: 98%**
- ❌ **FAIL**: `test_comparison_jpeg_with_gps` - 22.73% match rate (5/22 tags) - **THRESHOLD: 98%**
- ❌ **FAIL**: `test_comparison_tiff` - 58.33% match rate (7/12 tags) - **THRESHOLD: 98%**
- ❌ **FAIL**: `test_comparison_tiff_multipage` - 24.07% match rate (13/54 tags) - **THRESHOLD: 98%**
- ❌ **FAIL**: `test_comparison_tiff_big_endian` - 31.58% match rate (6/19 tags) - **THRESHOLD: 98%**
- ❌ **FAIL**: `test_comparison_png_with_text` - 0.00% match rate (0/26 tags) - **THRESHOLD: 98%**
- ❌ **FAIL**: `test_comparison_png_with_exif` - 0.00% match rate (0/46 tags) - **THRESHOLD: 98%**
- ❌ **FAIL**: `test_comparison_pdf` - 45.45% match rate (5/11 tags) - **THRESHOLD: 98%**
- ❌ **FAIL**: `test_comparison_mp4` - 0.00% match rate (0/31 tags) - **THRESHOLD: 98%**

### Detailed Issues by Format

#### 1. **PNG Parser Issues (0% match rate for both tests)**

**Problem**: ExifTool-RS is not extracting PNG metadata at all. All tags show as "MISSING".

**Missing Capabilities:**
- PNG chunk parsing (IHDR, tEXt, zTXt, iTXt, eXIf, pHYs, gAMA, cHRM, bKGD)
- Text chunk extraction (Title, Author, Description, timestamps)
- eXIf chunk parsing (embedded EXIF data)
- Image properties (width, height, bit depth, color type, compression, interlace)
- Physical dimensions (pHYs: PixelsPerUnitX/Y, PixelUnits)
- Color space data (gAMA: Gamma, cHRM: Chromaticity)

**Example Missing Tags:**
- `PNG:ImageWidth`, `PNG:ImageHeight` - Basic image dimensions from IHDR
- `PNG:BitDepth`, `PNG:ColorType`, `PNG:Compression` - Format properties
- `PNG:Title`, `PNG:Author`, `PNG:Description` - tEXt chunks
- `PNG:Datetimestamp`, `PNG:Datecreate`, `PNG:Datemodify` - tEXt timestamp chunks
- `PNG:WhitePointX`, `PNG:WhitePointY`, `PNG:RedX`, etc. - cHRM chromaticity data
- All eXIf chunk EXIF tags (IFD0:Make, IFD0:Model, ExifIFD:*, etc.)

#### 2. **PDF Parser Issues (45.45% match rate)**

**Problem**: ExifTool-RS extracts some tags but misses critical PDF-specific metadata.

**Missing Capabilities:**
- PDF Info dictionary parsing (PDFVersion, PageCount, Linearized)
- Date formatting (`CreateDate`, `ModifyDate` should be formatted as "YYYY:MM:DD HH:MM:SS+TZ")
- Keyword array handling (Keywords shows as comma-separated string instead of array)

**Example Missing Tags:**
- `PDF:PDFVersion` - MISSING (Perl shows "Number(1.4)")
- `PDF:PageCount` - MISSING (Perl shows "Number(1)")
- `PDF:CreateDate` - MISSING (Perl shows "2024:01:15 14:30:00+00:00")
- `PDF:ModifyDate` - MISSING (Perl shows "2024:01:15 15:00:00+00:00")
- `PDF:Linearized` - MISSING (Perl shows "No")

**Wrong Format:**
- `PDF:Keywords` - Rust shows `String("exiftool, rust, pdf, metadata, test")` but Perl shows `Array [String("exiftool"), String("rust"), ...]`

#### 3. **MP4 Parser Issues (0% match rate)**

**Problem**: ExifTool-RS is not extracting QuickTime/MP4 metadata at all.

**Missing Capabilities:**
- QuickTime atom parsing (ftyp, moov, mvhd, udta, meta, ilst)
- iTunes metadata (©nam/Title, ©ART/Artist, ©alb/Album, ©day/Year, ©gen/Genre, ©cmt/Comment)
- QuickTime headers (CreateDate, ModifyDate, Duration, TimeScale, etc.)
- File structure metadata (MajorBrand, CompatibleBrands, HandlerType, etc.)

**Example Missing Tags:**
- `QuickTime:MajorBrand`, `QuickTime:CompatibleBrands` - ftyp atom
- `QuickTime:CreateDate`, `QuickTime:ModifyDate`, `QuickTime:Duration` - mvhd atom
- `ItemList:Title`, `ItemList:Artist`, `ItemList:Album` - iTunes ilst metadata
- All other QuickTime metadata (TimeScale, PreferredRate, etc.)

#### 4. **TIFF Parser Issues (24-58% match rate)**

**Problem**: ExifTool-RS extracts raw tag values but doesn't format them properly or compute derived tags.

**Value Formatting Issues:**
- **Rational values**: Showing as string "28/10" instead of decimal "2.8" for FNumber
- **GPS coordinates**: Showing as binary "(Binary, 24 bytes)" instead of formatted "37 deg 46' 33.24\""
- **Enumerated values**: Showing as numbers (e.g., `ResolutionUnit: 1`) instead of strings (e.g., "None")
- **Component configuration**: Showing as number `197121` instead of string "Y, Cb, Cr, -"
- **Array values**: Showing as binary for multi-value tags (BitsPerSample, StripOffsets, etc.)
- **GPS version**: Showing as number `770` instead of string "2.3.0.0"

**Missing Composite Tags:**
- `Composite:ImageSize` - Should be "WIDTHxHEIGHT" string (e.g., "640x480")
- `Composite:Megapixels` - Should be calculated from width × height ÷ 1,000,000
- `Composite:Aperture` - Should match FNumber in decimal format
- `Composite:ShutterSpeed` - Should be formatted fraction (e.g., "1/100")
- `Composite:GPSPosition` - Should be formatted lat/lon string

**Missing JFIF Tags (for JPEG files):**
- `JFIF:JFIFVersion`, `JFIF:ResolutionUnit`, `JFIF:XResolution`, `JFIF:YResolution`

#### 5. **XMP Parser Issues (33% match rate for JPEG with XMP)**

**Problem**: ExifTool-RS is not extracting XMP metadata embedded in JPEG files.

**Missing XMP Tags:**
- `XMP-dc:Title`, `XMP-dc:Rights` - Dublin Core metadata
- `XMP-xmp:Creator`, `XMP-xmp:Rating` - XMP basic metadata

---

## Best Approach to Fix

### Phase 1: PNG Parser Implementation (CRITICAL - 0% match rate)

**File**: `src/parsers/png/chunk_parser.rs`

You MUST implement a comprehensive PNG chunk parser that extracts all standard PNG metadata:

1. **Basic PNG Structure Parsing**
   - Parse PNG signature (8-byte header: `89 50 4E 47 0D 0A 1A 0A`)
   - Implement chunk reading loop (Length + Type + Data + CRC)
   - Handle chunk types: IHDR, tEXt, zTXt, iTXt, eXIf, pHYs, gAMA, cHRM, bKGD, PLTE

2. **IHDR Chunk** (Image Header)
   - Extract: Width, Height, BitDepth, ColorType, CompressionMethod, FilterMethod, InterlaceMethod
   - Return as tags: `PNG:ImageWidth`, `PNG:ImageHeight`, `PNG:BitDepth`, `PNG:ColorType`, `PNG:Compression`, `PNG:Filter`, `PNG:Interlace`
   - Format ColorType as string enum: "Grayscale", "RGB", "Palette", "GrayscaleAlpha", "RGBAlpha"
   - Format Compression as "Deflate/Inflate"
   - Format Interlace as "Noninterlaced" or "Adam7 Interlace"

3. **tEXt/zTXt/iTXt Chunks** (Text Metadata)
   - Parse keyword-value pairs (null-terminated keyword, then text)
   - Handle standard keywords: Title, Author, Description, Copyright, Creation Time, Software, Disclaimer, Warning, Source, Comment
   - Handle custom keywords with namespace prefixes (e.g., "date:create", "date:modify", "date:timestamp")
   - Return as tags: `PNG:Title`, `PNG:Author`, `PNG:Description`, `PNG:Datecreate`, `PNG:Datemodify`, `PNG:Datetimestamp`
   - For zTXt: decompress data using zlib/flate2
   - For iTXt: handle UTF-8 encoding and optional compression

4. **eXIf Chunk** (Embedded EXIF)
   - Extract embedded TIFF/EXIF data (starts after 4-byte "Exif\0\0" header)
   - Parse using existing TIFF parser (`src/parsers/tiff/ifd_parser.rs`)
   - Return tags with proper namespace (e.g., `IFD0:Make`, `ExifIFD:DateTimeOriginal`)
   - Also create PNG-prefixed versions: `PNG:ExifMake`, `PNG:ExifModel`, etc.

5. **pHYs Chunk** (Physical Pixel Dimensions)
   - Extract: PixelsPerUnitX (4 bytes), PixelsPerUnitY (4 bytes), Unit (1 byte)
   - Return as tags: `PNG-pHYs:PixelsPerUnitX`, `PNG-pHYs:PixelsPerUnitY`, `PNG-pHYs:PixelUnits`
   - Format Unit as: 0="Unknown", 1="Meters"

6. **cHRM Chunk** (Primary Chromaticities and White Point)
   - Extract 8 × 4-byte unsigned integers (each divided by 100,000 to get float)
   - Return as tags: `PNG:WhitePointX`, `PNG:WhitePointY`, `PNG:RedX`, `PNG:RedY`, `PNG:GreenX`, `PNG:GreenY`, `PNG:BlueX`, `PNG:BlueY`
   - Format as decimal with 4 decimal places

7. **bKGD Chunk** (Background Color)
   - Extract background color index/value (size depends on ColorType)
   - Return as tag: `PNG:BackgroundColor`

8. **PLTE Chunk** (Palette)
   - Extract palette data (3 bytes per entry: R, G, B)
   - Return as binary tag: `PNG:Palette` with value "(Binary data N bytes, use -b option to extract)"

9. **ModifyDate Computation**
   - If "date:modify" or "tIME" exists, parse and format as `PNG:ModifyDate` in format "YYYY:MM:DD HH:MM:SS"

**Reference**: Study Perl ExifTool's `lib/Image/ExifTool/PNG.pm` for exact tag name mapping and value formatting.

### Phase 2: PDF Parser Implementation (45% → 98%+)

**File**: `src/parsers/pdf/` (create new module)

You MUST implement a PDF metadata parser:

1. **PDF Info Dictionary**
   - Parse PDF trailer to find Info dictionary object reference
   - Extract Info dictionary keys: Title, Author, Subject, Keywords, Creator, Producer, CreationDate, ModDate
   - Parse PDF version from header (`%PDF-1.4` → version "1.4")
   - Count pages by parsing Pages object tree

2. **Date Formatting**
   - Parse PDF date format: `D:YYYYMMDDHHmmSSOHH'mm` (e.g., `D:20240115143000+00'00`)
   - Convert to ExifTool format: `YYYY:MM:DD HH:MM:SS+HH:mm` (e.g., "2024:01:15 14:30:00+00:00")

3. **Keywords Handling**
   - If Keywords contains comma-separated values, split into array
   - Return as `PDF:Keywords` with array value

4. **Additional Metadata**
   - `PDF:PDFVersion` - from file header
   - `PDF:PageCount` - from Pages tree
   - `PDF:Linearized` - check for linearization dictionary ("Yes" or "No")

5. **XMP Metadata** (if embedded)
   - Parse XML metadata stream in Metadata object
   - Extract XMP properties (dc:title, dc:creator, etc.)
   - Return with XMP namespace prefixes

**Reference**: Study Perl ExifTool's `lib/Image/ExifTool/PDF.pm` and PDF specification.

### Phase 3: MP4 Parser Implementation (0% → 98%+)

**File**: `src/parsers/mp4/` (create new module)

You MUST implement a QuickTime/MP4 atom parser:

1. **Atom Structure**
   - Read atom header: 4-byte size + 4-byte type (e.g., "ftyp", "moov", "udta")
   - Handle 64-bit extended size for large atoms
   - Recursively parse container atoms (moov, trak, mdia, udta, meta, ilst)

2. **ftyp Atom** (File Type)
   - Extract major brand (4 bytes): "isom", "mp41", "mp42", "M4V ", etc.
   - Extract minor version (4 bytes)
   - Extract compatible brands (remaining 4-byte chunks)
   - Return as: `QuickTime:MajorBrand`, `QuickTime:MinorVersion`, `QuickTime:CompatibleBrands` (array)

3. **mvhd Atom** (Movie Header)
   - Extract: version, creation time, modification time, time scale, duration
   - Convert Mac epoch times (seconds since Jan 1, 1904) to "0000:00:00 00:00:00" format
   - Calculate duration in seconds: duration ÷ time scale
   - Return as: `QuickTime:CreateDate`, `QuickTime:ModifyDate`, `QuickTime:TimeScale`, `QuickTime:Duration`

4. **udta Atom** (User Data)
   - Look for "©nam" (title), "©cmt" (comment), "©des" (description)
   - Return as: `UserData:Title`, `UserData:Comment`, etc.

5. **meta → ilst Atom** (iTunes Metadata)
   - Parse ilst items: "©nam" (title), "©ART" (artist), "©alb" (album), "©day" (year), "©gen" (genre), "©cmt" (comment), "cprt" (copyright)
   - Each item contains "data" atom with type indicator and UTF-8 text
   - Return as: `ItemList:Title`, `ItemList:Artist`, `ItemList:Album`, `ItemList:ContentCreateDate`, `ItemList:Genre`, `ItemList:Comment`, `ItemList:Copyright`

6. **Additional QuickTime Tags**
   - Matrix structure, preferred rate/volume, preview time/duration, poster time, etc.
   - Return as: `QuickTime:MatrixStructure`, `QuickTime:PreferredRate`, `QuickTime:PreferredVolume`, etc.

**Reference**: Study Perl ExifTool's `lib/Image/ExifTool/QuickTime.pm` and ISO 14496-12 specification.

### Phase 4: TIFF Value Formatting (24-58% → 98%+)

**Files**: `src/parsers/tiff/ifd_parser.rs`, `src/core/operations.rs`

You MUST fix TIFF value formatting to match ExifTool output:

1. **Rational Value Formatting**
   - Convert rational fractions to decimals when appropriate
   - Example: FNumber "28/10" → "2.8" (format with 1-2 decimal places)
   - Keep as fraction for ExposureTime: "1/100" (formatted string, not "0.01")

2. **Enumerated Value Lookup**
   - Map numeric values to string descriptions using tag definitions
   - Examples:
     - `ResolutionUnit: 1` → "None", `2` → "inches", `3` → "cm"
     - `PhotometricInterpretation: 2` → "RGB", `1` → "BlackIsZero", etc.
     - `Compression: 1` → "Uncompressed", `6` → "JPEG", etc.
     - `ColorSpace: 65535` → "Uncalibrated", `1` → "sRGB"
     - `YCbCrPositioning: 1` → "Centered", `2` → "Co-sited"
     - `Orientation: 1` → "Horizontal (normal)", etc.
     - `FillOrder: 1` → "Normal", `2` → "Reversed"
     - `PlanarConfiguration: 1` → "Chunky", `2` → "Planar"
     - `SubfileType: 2` → "Single page of multi-page image"
     - `ExtraSamples: 2` → "Unassociated Alpha"

3. **GPS Coordinate Formatting**
   - Parse GPS rational arrays (3 rationals: degrees, minutes, seconds)
   - Format as: `DD deg MM' SS.SS"` (e.g., "37 deg 46' 33.24\"")
   - Example: `[37/1, 46/1, 3324/100]` → "37 deg 46' 33.24\""

4. **GPS Version Formatting**
   - Convert 4-byte array to dotted version string
   - Example: `[2, 3, 0, 0]` → "2.3.0.0"

5. **Component Configuration**
   - Parse 4-byte value as component identifiers
   - Map: 1=Y, 2=Cb, 3=Cr, 0=-
   - Example: `[1, 2, 3, 0]` → "Y, Cb, Cr, -"

6. **Multi-Value Array Formatting**
   - Format arrays as space-separated strings for display
   - Examples:
     - `BitsPerSample: [16, 16, 16, 16]` → "16 16 16 16"
     - `StripOffsets: [334, 998734, 1997134]` → "334 998734 1997134"
     - `WhitePoint: [0.3127, 0.3290]` → "0.3127000034 0.3289999962" (formatted to 10 decimal places)
     - `PrimaryChromaticities: [0.64, 0.33, 0.30, 0.60, 0.15, 0.06]` → "0.6399999857 0.3300000131 ..." (formatted)

7. **Page Number Formatting**
   - Parse 2-value SHORT array (page index, total pages)
   - Format as: "INDEX TOTAL" (e.g., "0 3" for first page of 3)

8. **GPS Altitude Formatting**
   - Parse rational value and format with unit
   - Example: "110/1" → "110 m" (append " m" for meters)

9. **Composite Tags** (new module: `src/core/composite.rs`)
   - Implement derived tag calculations:
     - `Composite:ImageSize` = `"{ImageWidth}x{ImageHeight}"` (e.g., "640x480")
     - `Composite:Megapixels` = `ImageWidth × ImageHeight ÷ 1,000,000` (formatted to 3 decimals)
     - `Composite:Aperture` = same as FNumber (formatted decimal)
     - `Composite:ShutterSpeed` = format ExposureTime as fraction (e.g., "1/100")
     - `Composite:GPSPosition` = `"{GPSLatitude}, {GPSLongitude}"` (formatted coordinates)

10. **JFIF Tags** (for JPEG files)
    - Parse JFIF APP0 marker segment (starts with "JFIF\0")
    - Extract version (2 bytes), density units, X/Y density
    - Return as: `JFIF:JFIFVersion`, `JFIF:ResolutionUnit`, `JFIF:XResolution`, `JFIF:YResolution`

**Reference**: Use `src/tag_db/generated_tags.rs` for enum lookup tables (check `get_tag_name()` function).

### Phase 5: XMP Parser Implementation (33% → 98%+)

**File**: `src/parsers/xmp/` (create new module or extend JPEG parser)

You MUST implement XMP metadata extraction:

1. **XMP Packet Detection**
   - For JPEG: scan for "http://ns.adobe.com/xap/1.0/" in APP1 marker
   - For PDF: look for Metadata stream object
   - For TIFF: check for XMP tag (0x02BC)

2. **XML Parsing**
   - Parse RDF/XML structure
   - Extract properties from namespaces: dc (Dublin Core), xmp, xmpRights, etc.

3. **Tag Mapping**
   - `dc:title` → `XMP-dc:Title`
   - `dc:creator` → `XMP-dc:Creator`
   - `dc:rights` → `XMP-dc:Rights`
   - `xmp:Rating` → `XMP-xmp:Rating`
   - And other XMP properties

4. **Integration**
   - Call XMP parser from JPEG/PDF/TIFF parsers when XMP data is detected
   - Merge XMP tags into main tag list with proper namespace

**Reference**: Study Perl ExifTool's `lib/Image/ExifTool/XMP.pm`.

---

## Implementation Order

Execute in this exact order to maximize test pass rate quickly:

1. **CRITICAL**: PNG parser (fixes 2 tests: png_with_text, png_with_exif) - 0% → 98%+
2. **HIGH**: TIFF value formatting (fixes 3 tests: tiff, tiff_multipage, tiff_big_endian) - 24-58% → 98%+
3. **HIGH**: MP4 parser (fixes 1 test: mp4) - 0% → 98%+
4. **MEDIUM**: PDF parser (fixes 1 test: pdf) - 45% → 98%+
5. **MEDIUM**: XMP parser (fixes 1 test: jpeg_with_exif_xmp) - 33% → 98%+
6. **MEDIUM**: GPS/Composite tags (fixes 1 test: jpeg_with_gps) - 22% → 98%+

After each phase, run tests to verify progress:
```bash
cargo test --release --features exiftool-comparison --test integration exiftool_comparison -- --nocapture
```

---

## Testing Strategy

After implementing fixes, verify each format:

```bash
# Test individual formats
cargo test --release --features exiftool-comparison test_comparison_png_with_text -- --nocapture
cargo test --release --features exiftool-comparison test_comparison_tiff -- --nocapture
cargo test --release --features exiftool-comparison test_comparison_mp4 -- --nocapture
cargo test --release --features exiftool-comparison test_comparison_pdf -- --nocapture

# Full test suite
cargo test --release --features exiftool-comparison --test integration exiftool_comparison
```

Success criteria: **All 10 tests must pass with 98%+ match rate**.

---

## Additional Notes

- Do NOT modify the test framework (`exiftool_comparison_tests.rs`) - it is working correctly
- Do NOT modify the test fixtures - they are valid and comprehensive (104 images)
- Do NOT modify the CI configuration - it is already properly set up
- FOCUS on implementing the missing parsers and fixing value formatting
- Use Perl ExifTool as the reference: `exiftool -json -a -G1 -struct tests/fixtures/png/simple/synthetic_001.png`
- Study the ExifTool source code (downloaded during build) for exact tag name mapping and formatting logic
- The tag database (`src/tag_db/generated_tags.rs`) has enum lookup functions you should use for value formatting

The primary issue is that ExifTool-RS has incomplete format parsers. The JPEG EXIF parser works (100% match), but PNG, PDF, MP4, and TIFF value formatting are severely lacking. You must implement these parsers following the ExifTool specification to achieve the 98%+ match rate threshold.
