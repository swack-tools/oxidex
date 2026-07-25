# Phase 2 Implementation Plan: 63.8% → 90%+ Coverage

**Generated:** 2025-12-07
**Current Coverage:** 63.8% (150/235 tags across 6 formats)
**Target Coverage:** 90%+

## Coverage Summary by Format

| Format | Coverage | Matched | Missing | Status |
|--------|----------|---------|---------|--------|
| MP4 | 93.1% | 27/29 | 2 | ✅ Good |
| TIFF | 96.2% | 25/26 | 1 | ✅ Good |
| PDF | 90.9% | 10/11 | 1 | ✅ Good |
| PNG | 79.2% | 38/48 | 10 | ⚠️ Needs work |
| JPEG | 41.3% | 50/121 | 69 | ❌ Primary focus |
| RAW | 0% | 0/0 | - | ⚠️ Parse issues |

## Priority 1: Canon MakerNotes Array Values (High Impact)

The JPEG test files contain Canon camera images. The main gaps are Canon MakerNotes arrays:

### Missing Canon CameraSettings Array Values
- `MacroMode` - Already have decoder, need to extract from array
- `Quality` - Already have decoder, need to extract
- `WhiteBalance` - Need decoder and extraction
- `FocusMode` - Already have decoder, need to extract
- `ContinuousDrive` - Need decoder and extraction
- `ImageSize` (CanonImageSize) - Need decoder
- `EasyMode` - Need decoder
- `DigitalZoom` - Need decoder
- `Contrast` - Need extraction
- `Saturation` - Need extraction
- `Sharpness` - Need extraction
- `MeteringMode` - Need decoder
- `FocusRange` - Need decoder
- `ExposureMode` (CanonExposureMode) - Already have decoder
- `LensType` - Need decoder

### Missing Canon ShotInfo Array Values
- `AutoISO` - Extract from array index 1
- `BaseISO` - Extract from array index 2
- `MeasuredEV` - Extract and format
- `TargetAperture` - Extract and convert to f-number
- `TargetExposureTime` - Extract and format as fraction
- `FlashExposureComp` - Extract and format as EV
- `SlowShutter` - Need decoder
- `SequenceNumber` - Direct extraction
- `AFPointsInFocus` - Need bitfield decoder

### Files to modify:
- `src/parsers/tiff/makernotes/canon.rs` - Add array field extraction

## Priority 2: EXIF Interop IFD (Medium Impact)

Missing EXIF tags from Interoperability IFD:
- `InteropIndex` (0x0001) - R98, R03, THM
- `InteropVersion` (0x0002) - Usually "0100"
- `RelatedImageWidth` (0x1001)
- `RelatedImageHeight` (0x1002)

### Files to modify:
- `src/parsers/tiff/exif_parser.rs` - Parse Interop IFD
- `src/parsers/tiff/tag_names.rs` - Add Interop tag definitions

## Priority 3: Canon Model ID Decoding

The CanonModelID value 17891328 should decode to "PowerShot S40".
Need to add Canon model ID lookup table.

### Files to modify:
- `src/parsers/tiff/makernotes/canon.rs` - Add model ID decoder

## Priority 4: PNG Additional Tags

Missing PNG tags (10):
- Various PNG-specific chunks
- Review PNG parser for completeness

## Implementation Order

1. **Canon CameraSettings extraction** - Extract all fields from array
2. **Canon ShotInfo extraction** - Extract all fields from array
3. **Canon Model ID decoder** - Add lookup table
4. **EXIF Interop IFD** - Parse interoperability IFD
5. **PNG improvements** - Add missing chunks

## Expected Impact

- Canon MakerNotes: +40-50 tags → JPEG from 41.3% to ~80%
- Interop IFD: +5 tags
- Canon Model ID: Fix 1 value difference
- Total expected: ~85-90% coverage
