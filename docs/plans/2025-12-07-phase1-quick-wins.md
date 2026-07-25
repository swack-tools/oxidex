# Phase 1: Quick Wins Coverage Improvement Plan

**Generated:** 2025-12-07
**Target:** Increase overall coverage from 5.0% to ~15-20%
**Approach:** Maximum parallel subagents (8 streams)

## Executive Summary

Analysis of comparison data reveals three categories of quick wins:

| Category | Impact | Effort | Tags Affected |
|----------|--------|--------|---------------|
| Value Formatting | HIGH | LOW | ~100+ tags become matches |
| Tag Family Normalization | HIGH | LOW | ~150+ tags become matches |
| Missing Standard EXIF Output | MEDIUM | LOW | ~50 tags |
| QuickTime Tag Mapping | HIGH | MEDIUM | ~70 tags (MP4 0%→50%+) |

### Current Pain Points

1. **Value Differences (55+ JPEG tags):** We extract the data but format it differently
   - Rationals: `360/100` vs ExifTool's `3.5`
   - Enums: `0` vs ExifTool's `Normal`
   - Dates: `2001-05-19T18:36:41+00:00` vs ExifTool's `2001:05:19 18:36:41`

2. **Tag Family Mismatches (150+ tags):** Same data, different prefixes
   - `Profile:` vs ExifTool's `ICC_Profile:`
   - `Canon:` vs ExifTool's `MakerNotes:`
   - `IFD0:` vs ExifTool's `EXIF:`
   - `GPS:` standalone vs ExifTool's `EXIF:GPS*`

3. **Suppressed Output:** Tags we parse but don't output
   - Standard EXIF tags in IFD0/IFD1
   - GPS tags (27 extra tags we have that ExifTool shows differently)

---

## Stream 1: Rational-to-Decimal Conversion

**Agent:** clean-code-writer
**Estimated Impact:** +30 matched tags
**Files:** `src/core/value_formatter.rs`

### Problem
We output raw rational values like `360/100` but ExifTool shows `3.5`.

### Tags Affected
```
EXIF:ApertureValue       360/100 → 3.5
EXIF:BrightnessValue     200/100 → 2
EXIF:CompressedBitsPerPixel  16/10 → 1.6
EXIF:DigitalZoomRatio    1/1 → 1
EXIF:ExposureCompensation    0/100 → 0
EXIF:FNumber             350/100 → 3.5
EXIF:FocalLength         600/100 → 6.0 mm
EXIF:FocalPlaneXResolution   3053/1 → 3053
EXIF:Gamma               22/10 → 2.2
EXIF:MaxApertureValue    360/100 → 3.5
EXIF:SubjectDistance     0/1 → 0 m
```

### Implementation

```rust
// src/core/value_formatter.rs

/// Format rational value to decimal string matching ExifTool output
pub fn format_rational_as_decimal(numerator: i64, denominator: i64) -> String {
    if denominator == 0 {
        return "inf".to_string();
    }
    let value = numerator as f64 / denominator as f64;

    // ExifTool typically shows clean integers without decimal
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        // Limit to reasonable precision, trim trailing zeros
        let formatted = format!("{:.6}", value);
        formatted.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Tags that should be formatted as decimal
const DECIMAL_RATIONAL_TAGS: &[&str] = &[
    "ApertureValue", "BrightnessValue", "CompressedBitsPerPixel",
    "DigitalZoomRatio", "ExposureCompensation", "ExposureBiasValue",
    "FNumber", "FocalLength", "FocalPlaneXResolution", "FocalPlaneYResolution",
    "Gamma", "MaxApertureValue", "SubjectDistance", "XResolution", "YResolution",
];
```

### Tests
```rust
#[test]
fn test_rational_formatting() {
    assert_eq!(format_rational_as_decimal(360, 100), "3.6");
    assert_eq!(format_rational_as_decimal(350, 100), "3.5");
    assert_eq!(format_rational_as_decimal(1, 1), "1");
    assert_eq!(format_rational_as_decimal(3053, 1), "3053");
    assert_eq!(format_rational_as_decimal(22, 10), "2.2");
}
```

---

## Stream 2: EXIF Enum Value Lookup

**Agent:** clean-code-writer
**Estimated Impact:** +25 matched tags
**Files:** `src/core/exif_enums.rs` (new), `src/parsers/tiff/tag_decoder.rs`

### Problem
We output raw integer values but ExifTool decodes them to human-readable strings.

### Tags Affected
```
EXIF:ColorSpace          1 → sRGB
EXIF:Contrast            0 → Normal
EXIF:CustomRendered      0 → Normal
EXIF:ExposureMode        0 → Auto
EXIF:Flash               1 → Fired
EXIF:GainControl         1 → Low gain up
EXIF:LightSource         0 → Unknown
EXIF:MeteringMode        5 → Multi-segment
EXIF:Saturation          0 → Normal
EXIF:SceneCaptureType    0 → Standard
EXIF:SensingMethod       2 → One-chip color area
EXIF:Sharpness           2 → Hard
EXIF:SubjectDistanceRange    0 → Unknown
EXIF:WhiteBalance        0 → Auto
EXIF:Orientation         1 → Horizontal (normal)
```

### Implementation

```rust
// src/core/exif_enums.rs

use std::collections::HashMap;
use once_cell::sync::Lazy;

/// EXIF ColorSpace values (tag 0xA001)
pub static COLOR_SPACE: Lazy<HashMap<u32, &'static str>> = Lazy::new(|| {
    HashMap::from([
        (1, "sRGB"),
        (2, "Adobe RGB"),
        (0xFFFF, "Uncalibrated"),
    ])
});

/// EXIF Contrast values (tag 0xA408)
pub static CONTRAST: Lazy<HashMap<u32, &'static str>> = Lazy::new(|| {
    HashMap::from([
        (0, "Normal"),
        (1, "Low"),
        (2, "High"),
    ])
});

/// EXIF ExposureMode values (tag 0xA402)
pub static EXPOSURE_MODE: Lazy<HashMap<u32, &'static str>> = Lazy::new(|| {
    HashMap::from([
        (0, "Auto"),
        (1, "Manual"),
        (2, "Auto bracket"),
    ])
});

/// EXIF Flash values (tag 0x9209) - bitmap decoding
pub fn decode_flash(value: u32) -> String {
    let fired = (value & 0x01) != 0;
    let return_detected = (value >> 1) & 0x03;
    let mode = (value >> 3) & 0x03;
    let function = (value >> 5) & 0x01;
    let red_eye = (value >> 6) & 0x01;

    let mut parts = Vec::new();

    if fired {
        parts.push("Fired");
    } else {
        parts.push("No Flash");
    }

    // Simplified - full implementation would handle all combinations
    parts.join(", ")
}

/// EXIF MeteringMode values (tag 0x9207)
pub static METERING_MODE: Lazy<HashMap<u32, &'static str>> = Lazy::new(|| {
    HashMap::from([
        (0, "Unknown"),
        (1, "Average"),
        (2, "Center-weighted average"),
        (3, "Spot"),
        (4, "Multi-spot"),
        (5, "Multi-segment"),
        (6, "Partial"),
        (255, "Other"),
    ])
});

/// EXIF Orientation values (tag 0x0112)
pub static ORIENTATION: Lazy<HashMap<u32, &'static str>> = Lazy::new(|| {
    HashMap::from([
        (1, "Horizontal (normal)"),
        (2, "Mirror horizontal"),
        (3, "Rotate 180"),
        (4, "Mirror vertical"),
        (5, "Mirror horizontal and rotate 270 CW"),
        (6, "Rotate 90 CW"),
        (7, "Mirror horizontal and rotate 90 CW"),
        (8, "Rotate 270 CW"),
    ])
});

/// EXIF WhiteBalance values (tag 0xA403)
pub static WHITE_BALANCE: Lazy<HashMap<u32, &'static str>> = Lazy::new(|| {
    HashMap::from([
        (0, "Auto"),
        (1, "Manual"),
    ])
});

// Map tag IDs to their enum lookup tables
pub fn decode_exif_enum(tag_id: u16, value: u32) -> Option<String> {
    match tag_id {
        0xA001 => COLOR_SPACE.get(&value).map(|s| s.to_string()),
        0xA408 => CONTRAST.get(&value).map(|s| s.to_string()),
        0xA402 => EXPOSURE_MODE.get(&value).map(|s| s.to_string()),
        0x9209 => Some(decode_flash(value)),
        0x9207 => METERING_MODE.get(&value).map(|s| s.to_string()),
        0x0112 => ORIENTATION.get(&value).map(|s| s.to_string()),
        0xA403 => WHITE_BALANCE.get(&value).map(|s| s.to_string()),
        // Add more as needed
        _ => None,
    }
}
```

---

## Stream 3: Tag Family Normalization (Profile → ICC_Profile)

**Agent:** clean-code-writer
**Estimated Impact:** +39 matched tags
**Files:** `src/core/tag_normalization.rs`

### Problem
We output ICC profile tags as `Profile:*` but ExifTool uses `ICC_Profile:*`.

### Current OxiDex Output (Extra Tags)
```
Profile:BlueMatrixColumn
Profile:BlueToneReproductionCurve
Profile:CMMFlags
Profile:ColorSpaceData
Profile:ConnectionSpaceIlluminant
... (39 total)
```

### ExifTool Expected
```
ICC_Profile:BlueMatrixColumn
ICC_Profile:BlueTRC
ICC_Profile:CMMFlags
ICC_Profile:ColorSpaceData
ICC_Profile:ConnectionSpaceIlluminant
```

### Implementation

```rust
// Add to src/core/tag_normalization.rs

/// Normalize tag family prefixes to match ExifTool conventions
pub fn normalize_tag_family(tag_name: &str) -> String {
    // Split into family:name
    if let Some((family, name)) = tag_name.split_once(':') {
        let normalized_family = match family {
            "Profile" => "ICC_Profile",
            "ExifIFD" => "EXIF",
            "IFD0" => "EXIF",  // Main image tags
            "IFD1" => "EXIF",  // Thumbnail tags
            "SubIFD" => "EXIF",
            _ => family,
        };
        format!("{}:{}", normalized_family, name)
    } else {
        tag_name.to_string()
    }
}

/// Additional name normalization for specific tags
pub fn normalize_tag_name(family: &str, name: &str) -> String {
    match (family, name) {
        // ICC Profile naming differences
        ("ICC_Profile", "BlueToneReproductionCurve") => "BlueTRC".to_string(),
        ("ICC_Profile", "GreenToneReproductionCurve") => "GreenTRC".to_string(),
        ("ICC_Profile", "RedToneReproductionCurve") => "RedTRC".to_string(),
        _ => name.to_string(),
    }
}
```

---

## Stream 4: Date Format Alignment

**Agent:** clean-code-writer
**Estimated Impact:** +10 matched tags
**Files:** `src/core/value_formatter.rs`

### Problem
We use ISO 8601 format, ExifTool uses EXIF-style colons.

### Tags Affected
```
EXIF:CreateDate         2001-05-19T18:36:41+00:00 → 2001:05:19 18:36:41
EXIF:DateTimeOriginal   2001-05-19T18:36:41+00:00 → 2001:05:19 18:36:41
EXIF:ModifyDate         same pattern
XMP:ModifyDate          2003-03-03T03:33:33.333+03:00 → 2003:03:03 03:33:33.333+03:00
```

### Implementation

```rust
// src/core/value_formatter.rs

/// Convert ISO 8601 date to EXIF-style date format
/// Input: "2001-05-19T18:36:41+00:00"
/// Output: "2001:05:19 18:36:41"
pub fn format_date_exif_style(iso_date: &str) -> String {
    // Parse ISO 8601 and reformat
    // Remove timezone for basic EXIF dates, keep for XMP

    // Simple regex-based conversion for common formats
    let re = regex::Regex::new(r"(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})").unwrap();

    if let Some(caps) = re.captures(iso_date) {
        format!("{}:{}:{} {}:{}:{}",
            &caps[1], &caps[2], &caps[3],
            &caps[4], &caps[5], &caps[6])
    } else {
        iso_date.to_string()
    }
}

/// Tags that use EXIF-style date format (no T separator, colons in date)
const EXIF_DATE_TAGS: &[&str] = &[
    "CreateDate", "DateTimeOriginal", "ModifyDate", "DateTimeDigitized",
    "DateTime", "DateTimeCreated",
];
```

---

## Stream 5: GPS Tag Family Alignment

**Agent:** clean-code-writer
**Estimated Impact:** +27 matched tags (currently showing as "extra")
**Files:** `src/core/tag_normalization.rs`, `src/parsers/tiff/gps_parser.rs`

### Problem
We output GPS tags with `GPS:` prefix, but ExifTool includes them in `EXIF:` family.

### Current OxiDex Output (Extra Tags)
```
GPS:GPSAltitude
GPS:GPSAltitudeRef
GPS:GPSDOP
GPS:GPSDateStamp
GPS:GPSLatitude
GPS:GPSLongitude
... (27 total)
```

### ExifTool Expected
```
EXIF:GPSAltitude
EXIF:GPSAltitudeRef
EXIF:GPSDOP
EXIF:GPSDateStamp
EXIF:GPSLatitude
EXIF:GPSLongitude
```

### Implementation

```rust
// Add to src/core/tag_normalization.rs

pub fn normalize_tag_family(tag_name: &str) -> String {
    if let Some((family, name)) = tag_name.split_once(':') {
        let normalized_family = match family {
            "GPS" => "EXIF",  // GPS tags go under EXIF family
            "Profile" => "ICC_Profile",
            "ExifIFD" => "EXIF",
            "IFD0" => "EXIF",
            _ => family,
        };
        format!("{}:{}", normalized_family, name)
    } else {
        tag_name.to_string()
    }
}
```

---

## Stream 6: QuickTime/MP4 Tag Output

**Agent:** clean-code-writer
**Estimated Impact:** +50 matched tags (MP4 from 0% to ~50%)
**Files:** `src/parsers/quicktime/metadata.rs`, `src/parsers/quicktime/tag_mapping.rs`

### Problem
MP4 is at 0% coverage. We parse QuickTime atoms but don't output tags.

### Missing Tags (68 total)
```
QuickTime:Album
QuickTime:Artist
QuickTime:AudioBitsPerSample
QuickTime:AudioChannels
QuickTime:AudioFormat
QuickTime:AudioSampleRate
QuickTime:CompatibleBrands
QuickTime:CreateDate
QuickTime:Duration
QuickTime:ImageHeight
QuickTime:ImageWidth
QuickTime:MajorBrand
QuickTime:MediaTimeScale
QuickTime:ModifyDate
QuickTime:Title
QuickTime:VideoFrameRate
```

### Implementation

Verify tag_mapping.rs is being used in the output path:

```rust
// src/parsers/quicktime/metadata.rs

use crate::parsers::quicktime::tag_mapping::map_atom_to_tag;

impl QuickTimeParser {
    pub fn extract_metadata(&self) -> Result<Vec<(String, String)>> {
        let mut tags = Vec::new();

        // Movie header atoms
        if let Some(mvhd) = self.find_atom(b"mvhd") {
            tags.push(("QuickTime:Duration".to_string(),
                       format_duration(mvhd.duration, mvhd.time_scale)));
            tags.push(("QuickTime:CreateDate".to_string(),
                       format_quicktime_date(mvhd.creation_time)));
            tags.push(("QuickTime:ModifyDate".to_string(),
                       format_quicktime_date(mvhd.modification_time)));
        }

        // Track dimensions
        if let Some(tkhd) = self.find_atom(b"tkhd") {
            tags.push(("QuickTime:ImageWidth".to_string(),
                       tkhd.width.to_string()));
            tags.push(("QuickTime:ImageHeight".to_string(),
                       tkhd.height.to_string()));
        }

        // iTunes metadata atoms
        for (atom_type, value) in self.ilst_atoms() {
            if let Some(tag_name) = map_atom_to_tag(atom_type) {
                tags.push((format!("QuickTime:{}", tag_name), value));
            }
        }

        Ok(tags)
    }
}
```

---

## Stream 7: Binary Data Decoding

**Agent:** clean-code-writer
**Estimated Impact:** +15 matched tags
**Files:** `src/core/binary_decoders.rs` (new)

### Problem
We output `[Binary data]` but ExifTool decodes specific binary fields.

### Tags Affected
```
EXIF:CFAPattern          [Binary data] → [Green,Blue][Red,Green]
EXIF:FileSource          [Binary data] → Digital Camera
EXIF:FlashpixVersion     [Binary data] → 0100
EXIF:SceneType           [Binary data] → Directly photographed
EXIF:UserComment         [Binary data] → GCM_TAG
EXIF:ExifVersion         [Binary data] → 0232
```

### Implementation

```rust
// src/core/binary_decoders.rs

/// Decode EXIF version bytes to string (e.g., [0x30,0x32,0x33,0x32] → "0232")
pub fn decode_exif_version(data: &[u8]) -> Option<String> {
    if data.len() >= 4 {
        Some(String::from_utf8_lossy(&data[0..4]).to_string())
    } else {
        None
    }
}

/// Decode FlashPix version (same format as EXIF version)
pub fn decode_flashpix_version(data: &[u8]) -> Option<String> {
    decode_exif_version(data)
}

/// Decode FileSource (tag 0xA300)
pub fn decode_file_source(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        1 => Some("Film Scanner".to_string()),
        2 => Some("Reflection Print Scanner".to_string()),
        3 => Some("Digital Camera".to_string()),
        _ => Some(format!("Unknown ({})", data[0])),
    }
}

/// Decode SceneType (tag 0xA301)
pub fn decode_scene_type(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        1 => Some("Directly photographed".to_string()),
        _ => Some(format!("Unknown ({})", data[0])),
    }
}

/// Decode CFA Pattern (tag 0xA302)
pub fn decode_cfa_pattern(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }

    let h_repeat = u16::from_be_bytes([data[0], data[1]]) as usize;
    let v_repeat = u16::from_be_bytes([data[2], data[3]]) as usize;

    if data.len() < 4 + h_repeat * v_repeat {
        return None;
    }

    let colors = ["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"];
    let mut result = String::new();

    for row in 0..v_repeat {
        result.push('[');
        for col in 0..h_repeat {
            let idx = data[4 + row * h_repeat + col] as usize;
            if col > 0 { result.push(','); }
            result.push_str(colors.get(idx).unwrap_or(&"Unknown"));
        }
        result.push(']');
    }

    Some(result)
}

/// Decode UserComment - handle encoding prefix
pub fn decode_user_comment(data: &[u8]) -> Option<String> {
    if data.len() < 8 {
        return None;
    }

    let encoding = &data[0..8];
    let text_data = &data[8..];

    match encoding {
        b"ASCII\0\0\0" => Some(String::from_utf8_lossy(text_data).trim_end_matches('\0').to_string()),
        b"UNICODE\0" => {
            // UTF-16 decode
            let u16_data: Vec<u16> = text_data.chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
                .collect();
            Some(String::from_utf16_lossy(&u16_data).trim_end_matches('\0').to_string())
        },
        _ => Some(String::from_utf8_lossy(text_data).trim_end_matches('\0').to_string()),
    }
}
```

---

## Stream 8: Unit Suffix Formatting

**Agent:** clean-code-writer
**Estimated Impact:** +10 matched tags
**Files:** `src/core/value_formatter.rs`

### Problem
ExifTool adds unit suffixes to certain values.

### Tags Affected
```
EXIF:FocalLength             600/100 → 6.0 mm
EXIF:FocalLengthIn35mmFormat 75 → 75 mm
EXIF:SubjectDistance         0/1 → 0 m
EXIF:ExposureTime           1/125 → 1/125 s  (or formatted as fraction)
GPS:GPSAltitude             117 → 117 m
```

### Implementation

```rust
// src/core/value_formatter.rs

/// Tags that need "mm" suffix (focal length related)
const MM_SUFFIX_TAGS: &[&str] = &[
    "FocalLength", "FocalLengthIn35mmFormat", "FocalLength35efl",
];

/// Tags that need "m" suffix (distance/altitude)
const METER_SUFFIX_TAGS: &[&str] = &[
    "SubjectDistance", "GPSAltitude", "HyperfocalDistance",
];

pub fn format_with_unit(tag_name: &str, value: &str) -> String {
    let base_name = tag_name.split(':').last().unwrap_or(tag_name);

    if MM_SUFFIX_TAGS.contains(&base_name) {
        format!("{} mm", value)
    } else if METER_SUFFIX_TAGS.contains(&base_name) {
        format!("{} m", value)
    } else {
        value.to_string()
    }
}
```

---

## Execution Plan

### Parallel Dispatch (All 8 streams simultaneously)

```
┌─────────────────────────────────────────────────────────────────────┐
│                     PARALLEL EXECUTION                               │
├─────────────────────────────────────────────────────────────────────┤
│ Stream 1: Rational→Decimal    │ Stream 5: GPS Family Align          │
│ Stream 2: Enum Value Lookup   │ Stream 6: QuickTime Output          │
│ Stream 3: Profile→ICC_Profile │ Stream 7: Binary Decoding           │
│ Stream 4: Date Formatting     │ Stream 8: Unit Suffixes             │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
                    ┌───────────────────────┐
                    │   Integration Pass    │
                    │  (Sequential merge)   │
                    └───────────────────────┘
                                │
                                ▼
                    ┌───────────────────────┐
                    │  Verification Tests   │
                    │  just compare-exiftool│
                    └───────────────────────┘
```

### Expected Outcomes

| Stream | New Matches | Coverage Impact |
|--------|-------------|-----------------|
| 1. Rationals | +30 | +0.8% |
| 2. Enums | +25 | +0.7% |
| 3. ICC_Profile | +39 | +1.1% |
| 4. Dates | +10 | +0.3% |
| 5. GPS | +27 | +0.8% |
| 6. QuickTime | +50 | +1.4% |
| 7. Binary | +15 | +0.4% |
| 8. Units | +10 | +0.3% |
| **TOTAL** | **+206** | **+5.8%** |

**Projected Coverage:** 5.0% → ~10-12% (conservative estimate accounting for overlap)

---

## Verification Commands

```bash
# Build and test after all streams complete
just test

# Run full ExifTool comparison
just compare-exiftool

# Check specific format improvements
just compare-exiftool-format jpeg
just compare-exiftool-format mp4

# Verify no regressions
cat comparison.json | jq '.total_regressions'
```

---

## Success Criteria

1. Overall coverage increases from 5.0% to 10%+
2. JPEG coverage increases from 3.7% to 8%+
3. MP4 coverage increases from 0% to 40%+
4. Zero regressions (matched tags don't decrease)
5. All tests pass (`just test`)

---

## Files Modified Summary

| File | Changes |
|------|---------|
| `src/core/value_formatter.rs` | Rational formatting, dates, units |
| `src/core/tag_normalization.rs` | Family prefix mapping |
| `src/core/exif_enums.rs` (NEW) | Enum value lookups |
| `src/core/binary_decoders.rs` (NEW) | Binary field decoding |
| `src/parsers/quicktime/metadata.rs` | Enable tag output |
| `src/parsers/tiff/tag_decoder.rs` | Use enum/binary decoders |
