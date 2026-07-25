# ExifTool Compatibility Integration Design

**Date:** 2025-12-08
**Status:** Approved

## Overview

Integrate the 30 new formatters and parsers into OxiDex to achieve ExifTool-compatible output. This uses a hybrid output-only formatting approach that keeps raw values internally and formats only at output time.

## Architecture

### Approach: Hybrid Output-Only Formatting

- Internal MetadataMap stores raw parsed values
- Formatting applied only when:
  1. Comparing with ExifTool (always)
  2. CLI output with `--exiftool-compat` flag

### Core Component

New module `src/core/exiftool_compat.rs` with single dispatch function:

```rust
pub fn format_for_exiftool(metadata: &MetadataMap) -> MetadataMap
```

## Tag Dispatch Logic

Priority order for formatting:

1. **GPS string references** - N→North, T→True North, K→km/h
2. **GPS binary decoders** - 0x00→Above Sea Level
3. **Binary decoders** - CFAPattern, SceneType, InteropVersion
4. **Enum tags** - ExposureProgram
5. **Unit suffixes** - mm, m, s
6. **Precision formatting** - rationals, decimals
7. **Default** - return unchanged

## Integration Points

### 1. CLI Integration

Add `--exiftool-compat` / `-e` flag:
- File: `src/main.rs` or CLI module
- Apply `format_for_exiftool()` before output when flag set

### 2. Comparison Tool

Always format for ExifTool comparison:
- File: `src/bin/tag-comparison/main.rs`
- Apply `format_for_exiftool()` to OxiDex output before comparing

### 3. APP Segment Parsers

Add processors to `src/core/jpeg_helpers.rs`:
- `process_app10_segments()` - HDR gain curve
- `process_app11_segments()` - JPEG-HDR
- `process_app12_segments()` - Olympus/Agfa Picture Info

### 4. MakerNote Sub-Parsers

Integrate with existing MakerNote parsers (incremental):
- Canon: CameraInfo, ColorData, AFInfo, LensInfo
- Nikon: ShotInfo, ColorBalance, LensData
- Sony: Tag2010, FocusInfo
- Olympus: CameraSettings
- Panasonic: Extended tags

## Files to Create

- `src/core/exiftool_compat.rs` - Main formatting dispatch

## Files to Modify

- `src/core/mod.rs` - Add exiftool_compat module
- `src/core/jpeg_helpers.rs` - Add APP10/11/12 processors
- `src/main.rs` - Add --exiftool-compat flag
- `src/bin/tag-comparison/main.rs` - Apply formatting

## Success Metrics

- JPEG coverage: 10% → 50%+
- Value differences reduced by 80%+
- All new formatters integrated and tested
