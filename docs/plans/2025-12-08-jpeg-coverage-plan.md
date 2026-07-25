# JPEG Coverage Improvement Plan

**Goal:** Increase JPEG coverage from 10.0% to 50%+ using ExifTool dataset

**Current State:**
- Files Tested: 4,085
- Coverage: 10.0%
- Matched Tags: 361
- Missing Tags: 3,050
- Extra Tags: 214
- Value Differences: 198

---

## Phase 1: Value Formatting Fixes (Quick Wins)

**Impact:** Fix 198 value differences → ~5% coverage boost
**Effort:** Low
**Files:** `src/core/value_formatter.rs`, `src/parsers/tiff/gps_parser.rs`

### 1.1 GPS Reference Values
Convert single-character refs to human-readable strings:
- `N` → `North`, `S` → `South`, `E` → `East`, `W` → `West`
- `T` → `True North`, `M` → `Magnetic North`
- `K` → `km/h`, `M` → `mph`, `N` → `knots`
- `A` → `Measurement Active`, `V` → `Measurement Void`
- `2` → `2-Dimensional Measurement`, `3` → `3-Dimensional Measurement`

### 1.2 GPS Altitude Reference
- `0x00` → `Above Sea Level`
- `0x01` → `Below Sea Level`
- Currently outputs `[Binary data]` instead

### 1.3 Numeric Precision Alignment
Align decimal precision with ExifTool:
- BrightnessValue: `3.617254` → `3.617254236` (9 decimals)
- DigitalZoomRatio: `1.001988` → `1.001988072`
- GPS coordinates: Match ExifTool's degree/minute/second formatting

### 1.4 Unit Formatting
- FocalLengthIn35mmFormat: `31` → `31 mm`
- LensInfo: `3 mm f/2.4` → `3.3mm f/2.4`
- GPSAltitude: Add "m" suffix consistently

### 1.5 Binary Data Decoding
Decode tags currently showing `[Binary data]`:
- CFAPattern → `[Red,Green][Green,Blue]`
- SceneType → `Directly photographed`
- InteropVersion → `0100`
- GPSProcessingMethod → `GPS` (strip null padding)

---

## Phase 2: EXIF Tag Mapping (Medium Impact)

**Impact:** Map unknown hex tags → ~10% coverage boost
**Effort:** Medium
**Files:** `oxidex-tags-core/src/exif.rs`, `src/parsers/tiff/ifd_parser.rs`

### 2.1 Map Hex Tags to Named Tags
Current "Extra Tags" include unmapped EXIF tags:
- `0x8822` → ExposureProgram
- `0x882A` → TimeZoneOffset
- `0x9103` → CompressedBitsPerPixel
- `0x920C`-`0x9210` → SpatialFrequencyResponse group
- `0x927C` → MakerNote (should trigger MakerNote parser)
- `0x9CA1`, `0x9CA2` → Apple HDR tags
- `0xC4A5` → PrintImageMatching

### 2.2 Add Missing EXIF Tags
Tags in "Missing Tags" that should be easy to add:
- ExposureProgram (0x8822) - enum values
- BitsPerSample (0x0102)
- SamplesPerPixel (0x0115)
- RowsPerStrip (0x0116)
- StripByteCounts (0x0117)
- StripOffsets (0x0111)

### 2.3 Thumbnail Extraction
Add ThumbnailImage, ThumbnailOffset, ThumbnailLength extraction from IFD1.

---

## Phase 3: APP Segment Parsing (High Impact)

**Impact:** Parse missing APP segments → ~15% coverage boost
**Effort:** High
**Files:** `src/parsers/jpeg/`

### 3.1 APP12 (Olympus/Agfa Picture Info)
Currently missing 70+ APP12 tags:
- CAM1-CAM9, COLOR1-COLOR4
- EXP1-EXP3, FCS1-FCS7
- WB1-WB6, STB1-STB6
- DateTimeOriginal, ExposureTime, FNumber
- CameraType, SerialNumber, Version

Create `src/parsers/jpeg/app12_parser.rs`:
```rust
pub fn parse_app12_picture_info(data: &[u8]) -> Result<MetadataMap>
```

### 3.2 APP14 Adobe Enhancement
Current parser extracts 4 tags. Add:
- APP14Flags0 interpretation (bitfield)
- APP14Flags1 interpretation
- Full ColorTransform values (Unknown, YCbCr, YCCK)

### 3.3 APP0 JFIF Extension
Add missing fields:
- InterleavedField
- OcadRevision (for OCAD files)

### 3.4 APP10/APP11 HDR Segments
Currently partially implemented. Add:
- HDRGainCurve, HDRGainCurveSize
- Alpha, Beta, CorrectionMethod
- JPEG-HDRVersion, Ln0, Ln1, S2n
- RatioImage extraction

### 3.5 APP3-APP9 Segments
Low priority but present in dataset:
- APP3: Meta/JPS stereoscopic
- APP4: Scalado SPMO
- APP5-APP9: Vendor-specific

---

## Phase 4: MakerNotes Improvements (High Impact)

**Impact:** Fix MakerNote parsing → ~15% coverage boost
**Effort:** High
**Files:** `src/parsers/tiff/makernotes/`

### 4.1 Canon MakerNotes
Coverage: 19.2% (639 missing tags)
Priority improvements:
- CameraInfo tags (CameraTemperature, BatteryType, etc.)
- ColorData tags (ColorTempAsShot, WBShift, etc.)
- AFInfo tags (AFPointsSelected, AFAreaMode, etc.)
- LensInfo tags (LensModel, LensSerialNumber)

### 4.2 Nikon MakerNotes
Coverage: 15.5% (654 missing tags)
Priority improvements:
- ShotInfo tags
- ColorBalance tags
- LensData tags
- AFInfo tags

### 4.3 Sony MakerNotes
Coverage: 16.2% (600 missing tags)
Priority improvements:
- Tag2010 (comprehensive camera settings)
- Tag9050 (additional settings)
- FocusInfo tags

### 4.4 MakerNote Value Interpretation
Many tags extracted as raw values need interpretation:
- Enum values → human-readable strings
- Bitfields → flag descriptions
- Rationals → properly formatted values

---

## Phase 5: IPTC Improvements (Medium Impact)

**Impact:** Fix IPTC parsing → ~3% coverage boost
**Effort:** Low-Medium
**Files:** `src/parsers/jpeg/iptc_parser.rs`

### 5.1 Add Missing IPTC Tags
Current parser handles 23 tags. Add:
- Record 1 tags (Envelope Record)
- Record 2 additional tags (FixtureIdentifier, ReleaseDate, etc.)
- Record 7 tags (ObjectPreviewData)

### 5.2 Fix Unknown Tags
Tags showing as `Unknown-1-90`, `Unknown-2-0`, etc. need proper mapping.

### 5.3 Multi-value Support
Keywords and other repeatable tags should accumulate values.

---

## Phase 6: ICC Profile Improvements

**Impact:** ~2% coverage boost
**Effort:** Medium
**Files:** `src/parsers/jpeg/icc_parser.rs`

### 6.1 Multi-chunk Assembly
Currently skips multi-chunk profiles. Implement:
- Chunk collection and reassembly
- Sequence number validation

### 6.2 Value Formatting
Align ICC values with ExifTool:
- ProfileCMMType: `Lino` → `Linotronic`
- DeviceManufacturer: `IEC` → `Hewlett-Packard`
- Matrix values: Reduce decimal precision

---

## Implementation Priority

| Phase | Impact | Effort | Priority |
|-------|--------|--------|----------|
| 1. Value Formatting | +5% | Low | **P0** |
| 2. EXIF Tag Mapping | +10% | Medium | **P1** |
| 3. APP Segments | +15% | High | **P1** |
| 4. MakerNotes | +15% | High | **P2** |
| 5. IPTC | +3% | Low-Med | **P2** |
| 6. ICC Profiles | +2% | Medium | **P3** |

**Estimated Total Impact:** 10% → 50%+ coverage

---

## Verification Strategy

1. Run `just compare-exiftool-full` after each phase
2. Track coverage percentage improvement
3. Focus on reducing "Value Differences" first (quick wins)
4. Then "Missing Tags" by category
5. Address "Extra Tags" by mapping hex IDs to names

---

## Files to Create/Modify

**New Files:**
- `src/parsers/jpeg/app12_parser.rs` - Olympus/Agfa picture info
- `src/parsers/jpeg/app10_parser.rs` - HDR gain curve
- `src/core/gps_formatter.rs` - GPS value formatting

**Modify:**
- `src/core/value_formatter.rs` - Precision and unit fixes
- `src/parsers/tiff/gps_parser.rs` - Reference value formatting
- `src/parsers/jpeg/iptc_parser.rs` - Additional tag support
- `src/parsers/jpeg/icc_parser.rs` - Multi-chunk support
- `oxidex-tags-core/src/exif.rs` - Tag definitions
- `src/parsers/tiff/makernotes/*.rs` - MakerNote improvements

---

## Success Metrics

- [ ] Phase 1: Coverage ≥ 15%
- [ ] Phase 2: Coverage ≥ 25%
- [ ] Phase 3: Coverage ≥ 40%
- [ ] Phase 4: Coverage ≥ 50%
- [ ] Phase 5: Coverage ≥ 53%
- [ ] Phase 6: Coverage ≥ 55%
