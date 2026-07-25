# JPEG 50%+ Coverage Implementation Plan

**Generated:** 2025-12-07
**Current:** 51/1362 tags (3.7%)
**Target:** 681+ tags (50%+)
**Gap:** +630 tags needed

## Executive Summary

To reach 50% JPEG coverage, we must address THREE major areas:

| Area | Current Gap | Potential Gain | Priority |
|------|-------------|----------------|----------|
| **MakerNotes** | 708 missing | +400-500 | CRITICAL |
| **Tag Normalization** | 140 extra (wrong names) | +100-120 | HIGH |
| **Value Formatting** | 55 differences | +55 | HIGH |
| **Other Families** | ~300 missing | +150-200 | MEDIUM |

**Projected outcome:** 51 + 500 + 120 + 55 + 150 = **876 tags (64%)**

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           JPEG TAG EXTRACTION                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌─────────────┐    │
│  │   APP1      │   │   APP1      │   │   APP2      │   │  APP12-15   │    │
│  │   EXIF      │   │   XMP       │   │  ICC/MPF    │   │   Various   │    │
│  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘   └──────┬──────┘    │
│         │                 │                 │                 │            │
│         ▼                 ▼                 ▼                 ▼            │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌─────────────┐    │
│  │ IFD Parser  │   │ XMP Parser  │   │ICC Profile  │   │ APP Parser  │    │
│  │ + MakerNote │   │             │   │  Parser     │   │             │    │
│  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘   └──────┬──────┘    │
│         │                 │                 │                 │            │
│         └─────────────────┴─────────────────┴─────────────────┘            │
│                                    │                                        │
│                                    ▼                                        │
│                         ┌───────────────────┐                              │
│                         │  Tag Normalizer   │                              │
│                         │  (family prefixes │                              │
│                         │   + value format) │                              │
│                         └─────────┬─────────┘                              │
│                                   │                                        │
│                                   ▼                                        │
│                         ┌───────────────────┐                              │
│                         │   Final Output    │                              │
│                         │ MakerNotes:Tag    │                              │
│                         │ EXIF:Tag          │                              │
│                         │ ICC_Profile:Tag   │                              │
│                         └───────────────────┘                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Phase 1: Tag Normalization (8 parallel streams)

**Impact:** +175 matched tags (51 → 226, ~17%)
**Time:** Quick wins, can run in parallel

### Stream 1.1: Canon → MakerNotes Prefix
**Files:** `src/core/tag_normalization.rs`

We output 28 Canon tags with wrong prefix:
```
Canon:AFImageHeight → MakerNotes:AFImageHeight
Canon:MacroMode → MakerNotes:MacroMode
Canon:Quality → MakerNotes:Quality
... (28 total)
```

```rust
pub fn normalize_tag_family(tag_name: &str) -> String {
    if let Some((family, name)) = tag_name.split_once(':') {
        let normalized_family = match family {
            // Camera brands → MakerNotes
            "Canon" | "Nikon" | "Sony" | "Fujifilm" | "Olympus" |
            "Panasonic" | "Pentax" | "Samsung" | "Apple" => "MakerNotes",
            // Other normalizations
            "Profile" => "ICC_Profile",
            "GPS" => "EXIF",
            "IFD0" | "IFD1" | "ExifIFD" | "SubIFD" => "EXIF",
            _ => family,
        };
        format!("{}:{}", normalized_family, name)
    } else {
        tag_name.to_string()
    }
}
```

### Stream 1.2: Profile → ICC_Profile (+39 tags)
### Stream 1.3: GPS → EXIF (+27 tags)
### Stream 1.4: IFD0/ExifIFD → EXIF (+19 tags)
### Stream 1.5: XMP Namespace Alignment (+14 tags)
### Stream 1.6: Value Formatting - Rationals (+30 tags)
### Stream 1.7: Value Formatting - Enums (+25 tags)
### Stream 1.8: Value Formatting - Dates/Binary (+11 tags)

---

## Phase 2: MakerNotes Deep Parsing (10 parallel streams)

**Impact:** +400-500 matched tags
**Critical for 50%:** MakerNotes = 708/1256 missing (56%)

### The Problem

We have Canon MakerNotes parser but it only extracts ~30 tags. ExifTool extracts 700+.

Missing Canon tag categories:
- **CameraInfo** (0x000D) - Contains 200+ tags like AEAperture, AFAreaHeight
- **ProcessingInfo** (0x00A0) - Processing settings
- **ColorInfo** (various) - Color balance data
- **VignettingCorr** (0x00B0) - Lens corrections
- **LightingOpt** (0x00B1) - Lighting optimization
- **CustomFunctions** (0x000F) - User customizations
- **AFInfo3** (0x0027) - Extended AF data

### Stream 2.1: Canon CameraInfo Parser

**Files:** `src/parsers/tiff/makernotes/canon_camera_info.rs` (new)
**Impact:** +150-200 tags

CameraInfo (tag 0x000D) is a large binary structure containing:
- AE (Auto Exposure) settings: AEAperture, AEExposureTime, AEMaxAperture, etc.
- AF (Auto Focus) data: AFAreaHeight, AFAreaWidth, AFPointsInFocus, etc.
- WB (White Balance): WBShiftAB, WBShiftGM, etc.

```rust
// src/parsers/tiff/makernotes/canon_camera_info.rs

/// Parse Canon CameraInfo binary structure
/// Structure varies by camera model - need model-specific parsers
pub fn parse_camera_info(data: &[u8], model_id: u32) -> HashMap<String, String> {
    let mut tags = HashMap::new();

    // Model-specific parsing based on ExifTool's Canon.pm
    match model_id {
        // EOS 5D Mark III, 6D, 70D, etc.
        0x80000285 | 0x80000302 | 0x80000325 => {
            parse_camera_info_5d3(data, &mut tags);
        }
        // EOS R, R5, R6
        0x80000424 | 0x80000453 | 0x80000464 => {
            parse_camera_info_eos_r(data, &mut tags);
        }
        // Default parser for unknown models
        _ => {
            parse_camera_info_generic(data, &mut tags);
        }
    }

    tags
}

fn parse_camera_info_5d3(data: &[u8], tags: &mut HashMap<String, String>) {
    // Based on ExifTool Canon.pm CameraInfo5D3 structure
    if data.len() < 1000 { return; }

    // Byte offsets from ExifTool
    let ae_aperture = i16::from_le_bytes([data[0x30], data[0x31]]);
    tags.insert("MakerNotes:AEAperture".into(),
                format_aperture(ae_aperture));

    let ae_exposure_time = i16::from_le_bytes([data[0x32], data[0x34]]);
    tags.insert("MakerNotes:AEExposureTime".into(),
                format_exposure_time(ae_exposure_time));

    // Continue for all fields...
}
```

### Stream 2.2: Canon AFInfo2/AFInfo3 Parser (+50 tags)

**Files:** `src/parsers/tiff/makernotes/canon_af_info.rs` (new)

```rust
/// Parse Canon AFInfo2 (tag 0x0026) and AFInfo3 (tag 0x0027)
/// Contains detailed autofocus point information
pub fn parse_af_info2(data: &[u8]) -> HashMap<String, String> {
    let mut tags = HashMap::new();

    if data.len() < 10 { return tags; }

    let num_af_points = u16::from_le_bytes([data[0], data[1]]);
    tags.insert("MakerNotes:NumAFPoints".into(), num_af_points.to_string());

    let valid_af_points = u16::from_le_bytes([data[2], data[3]]);
    tags.insert("MakerNotes:ValidAFPoints".into(), valid_af_points.to_string());

    // AF area dimensions
    let img_width = u16::from_le_bytes([data[4], data[5]]);
    let img_height = u16::from_le_bytes([data[6], data[7]]);
    tags.insert("MakerNotes:AFImageWidth".into(), img_width.to_string());
    tags.insert("MakerNotes:AFImageHeight".into(), img_height.to_string());

    // AF area widths/heights arrays
    let offset = 8;
    let area_widths: Vec<String> = (0..num_af_points as usize)
        .map(|i| {
            let idx = offset + i * 2;
            if idx + 1 < data.len() {
                u16::from_le_bytes([data[idx], data[idx+1]]).to_string()
            } else {
                "0".to_string()
            }
        })
        .collect();
    tags.insert("MakerNotes:AFAreaWidths".into(), area_widths.join(" "));

    // Continue for AFAreaHeights, AFAreaXPositions, AFAreaYPositions...

    tags
}
```

### Stream 2.3: Nikon MakerNotes Enhancement (+80 tags)

**Files:** `src/parsers/tiff/makernotes/nikon.rs`

Similar structure - Nikon has extensive MakerNotes including:
- ShotInfo (D-Lighting, ActiveD-Lighting, HDR settings)
- LensData (detailed lens info)
- AFInfo2 (focus point data)
- ColorBalance (WB fine-tuning)

### Stream 2.4: Sony MakerNotes Enhancement (+60 tags)

**Files:** `src/parsers/tiff/makernotes/sony.rs`

### Stream 2.5: Fujifilm MakerNotes Enhancement (+40 tags)

**Files:** `src/parsers/tiff/makernotes/fujifilm.rs`

### Stream 2.6: Olympus MakerNotes Enhancement (+30 tags)

### Stream 2.7: Panasonic MakerNotes Enhancement (+25 tags)

### Stream 2.8: Pentax MakerNotes Enhancement (+20 tags)

### Stream 2.9: Generic MakerNotes Parser Improvements

Parse common tag patterns that appear across brands.

### Stream 2.10: MakerNotes Tag ID Registry

Create comprehensive tag ID → name mapping from ExifTool's .pm files.

```rust
// src/parsers/tiff/makernotes/tag_registry.rs

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Canon MakerNotes tag IDs (from ExifTool Canon.pm)
pub static CANON_TAGS: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    HashMap::from([
        (0x0001, "CameraSettings"),
        (0x0002, "FocalLength"),
        (0x0003, "FlashInfo"),
        (0x0004, "ShotInfo"),
        (0x0005, "Panorama"),
        (0x0006, "ImageType"),
        (0x0007, "FirmwareVersion"),
        (0x0008, "FileNumber"),
        (0x0009, "OwnerName"),
        (0x000a, "UnknownD30"),
        (0x000c, "SerialNumber"),
        (0x000d, "CameraInfo"),
        (0x000e, "FileLength"),
        (0x000f, "CustomFunctions"),
        (0x0010, "ModelID"),
        (0x0011, "MovieInfo"),
        (0x0012, "AFInfo"),
        // ... 100+ more tags
    ])
});
```

---

## Phase 3: Other Missing Families (6 parallel streams)

**Impact:** +150-200 tags

### Stream 3.1: ICC_Profile Complete Parsing (+39 tags)

We extract ICC profile but ExifTool shows 39 tags we're missing.

**Files:** `src/parsers/icc/profile_parser.rs`

```rust
// Tags we need to extract:
// ICC_Profile:ProfileCMMType
// ICC_Profile:ProfileVersion
// ICC_Profile:ProfileClass
// ICC_Profile:ColorSpaceData
// ICC_Profile:ProfileConnectionSpace
// ICC_Profile:ProfileDateTime
// ICC_Profile:ProfileFileSignature
// ICC_Profile:PrimaryPlatform
// ICC_Profile:CMMFlags
// ICC_Profile:DeviceManufacturer
// ICC_Profile:DeviceModel
// ICC_Profile:DeviceAttributes
// ICC_Profile:RenderingIntent
// ICC_Profile:ConnectionSpaceIlluminant
// ICC_Profile:ProfileCreator
// ICC_Profile:ProfileID
// + TRC curves, matrix columns, etc.
```

### Stream 3.2: Composite Tags (+39 tags)

Composite tags are **calculated** by ExifTool, not extracted from files:
- Aperture (from ApertureValue)
- ShutterSpeed (from ShutterSpeedValue)
- LightValue, FOV, HyperfocalDistance
- ImageSize (WxH), Megapixels
- ScaleFactor35efl

```rust
// src/core/composite_tags.rs

/// Calculate composite tags from extracted metadata
pub fn calculate_composite_tags(tags: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut composite = Vec::new();

    // Aperture: Convert APEX ApertureValue to f-number
    if let Some(av) = tags.get("EXIF:ApertureValue") {
        if let Ok(apex) = av.parse::<f64>() {
            let f_number = 2.0_f64.powf(apex / 2.0);
            composite.push(("Composite:Aperture".into(), format!("{:.1}", f_number)));
        }
    }

    // ShutterSpeed: Convert APEX to seconds
    if let Some(tv) = tags.get("EXIF:ShutterSpeedValue") {
        if let Ok(apex) = tv.parse::<f64>() {
            let seconds = 2.0_f64.powf(-apex);
            composite.push(("Composite:ShutterSpeed".into(), format_shutter(seconds)));
        }
    }

    // ImageSize
    if let (Some(w), Some(h)) = (tags.get("EXIF:ImageWidth"), tags.get("EXIF:ImageHeight")) {
        composite.push(("Composite:ImageSize".into(), format!("{}x{}", w, h)));
    }

    // Megapixels
    if let (Some(w), Some(h)) = (tags.get("EXIF:ImageWidth"), tags.get("EXIF:ImageHeight")) {
        if let (Ok(w), Ok(h)) = (w.parse::<f64>(), h.parse::<f64>()) {
            composite.push(("Composite:Megapixels".into(),
                          format!("{:.1}", w * h / 1_000_000.0)));
        }
    }

    composite
}
```

### Stream 3.3: IPTC Complete Parsing (+9 tags)

Missing IPTC tags:
- ApplicationRecordVersion
- DigitalCreationDate/Time
- DocumentNotes
- ObjectCycle
- etc.

### Stream 3.4: APP Segment Parsing (APP2, APP6, APP12) (+80 tags)

**APP2:** IJPEG data, FlashPix extensions
**APP6:** HP/Kodak proprietary
**APP12:** AGFA/Picture Info

### Stream 3.5: Photoshop IRB Parsing (+20 tags)

Parse Photoshop Image Resource Blocks in APP13.

### Stream 3.6: XMP Extended Parsing (+29 tags)

Parse additional XMP namespaces we're missing.

---

## Execution Strategy: Maximum Parallelism

### Wave 1: Tag Normalization (8 parallel agents)
```
┌────────┬────────┬────────┬────────┬────────┬────────┬────────┬────────┐
│Stream  │Stream  │Stream  │Stream  │Stream  │Stream  │Stream  │Stream  │
│1.1     │1.2     │1.3     │1.4     │1.5     │1.6     │1.7     │1.8     │
│Canon→  │Profile→│GPS→    │IFD0→   │XMP NS  │Rational│Enum    │Date/   │
│Maker   │ICC     │EXIF    │EXIF    │Align   │Format  │Lookup  │Binary  │
└────────┴────────┴────────┴────────┴────────┴────────┴────────┴────────┘
                              │
                              ▼
                    [Integration + Test]
                              │
                              ▼
                    Expected: ~17% coverage
```

### Wave 2: MakerNotes Deep Parsing (10 parallel agents)
```
┌────────┬────────┬────────┬────────┬────────┐
│Canon   │Canon   │Nikon   │Sony    │Fuji    │
│Camera  │AF      │Maker   │Maker   │Maker   │
│Info    │Info    │Notes   │Notes   │Notes   │
├────────┼────────┼────────┼────────┼────────┤
│Olympus │Pana    │Pentax  │Generic │Tag     │
│Maker   │sonic   │Maker   │Parser  │Registry│
│Notes   │Maker   │Notes   │Improve │Complete│
└────────┴────────┴────────┴────────┴────────┘
                              │
                              ▼
                    [Integration + Test]
                              │
                              ▼
                    Expected: ~45% coverage
```

### Wave 3: Other Families (6 parallel agents)
```
┌──────────┬──────────┬──────────┬──────────┬──────────┬──────────┐
│ICC       │Composite │IPTC      │APP       │Photoshop │XMP       │
│Profile   │Tags      │Complete  │Segments  │IRB       │Extended  │
└──────────┴──────────┴──────────┴──────────┴──────────┴──────────┘
                              │
                              ▼
                    [Integration + Test]
                              │
                              ▼
                    Expected: 50%+ coverage
```

---

## Detailed Task Breakdown

### Wave 1 Tasks (8 agents)

| Agent | Task | Files | Est. Tags |
|-------|------|-------|-----------|
| 1.1 | Canon/Nikon/Sony → MakerNotes prefix | tag_normalization.rs | +28 |
| 1.2 | Profile → ICC_Profile prefix | tag_normalization.rs | +39 |
| 1.3 | GPS → EXIF prefix | tag_normalization.rs | +27 |
| 1.4 | IFD0/ExifIFD → EXIF prefix | tag_normalization.rs | +19 |
| 1.5 | XMP namespace standardization | xmp/namespace_mapping.rs | +14 |
| 1.6 | Rational → decimal formatting | value_formatter.rs | +30 |
| 1.7 | Enum value lookups | exif_enums.rs (new) | +25 |
| 1.8 | Date/binary formatting | value_formatter.rs | +11 |

### Wave 2 Tasks (10 agents)

| Agent | Task | Files | Est. Tags |
|-------|------|-------|-----------|
| 2.1 | Canon CameraInfo parser | canon_camera_info.rs (new) | +150 |
| 2.2 | Canon AFInfo2/3 parser | canon_af_info.rs (new) | +50 |
| 2.3 | Nikon MakerNotes enhance | nikon.rs | +80 |
| 2.4 | Sony MakerNotes enhance | sony.rs | +60 |
| 2.5 | Fujifilm MakerNotes enhance | fujifilm.rs | +40 |
| 2.6 | Olympus MakerNotes enhance | olympus.rs | +30 |
| 2.7 | Panasonic MakerNotes enhance | panasonic.rs | +25 |
| 2.8 | Pentax MakerNotes enhance | pentax.rs | +20 |
| 2.9 | Generic MakerNotes improve | shared/makernote_parser.rs | +25 |
| 2.10 | Tag ID registry complete | tag_registry.rs | Support |

### Wave 3 Tasks (6 agents)

| Agent | Task | Files | Est. Tags |
|-------|------|-------|-----------|
| 3.1 | ICC Profile complete | icc/profile_parser.rs | +39 |
| 3.2 | Composite tag calculation | composite_tags.rs (new) | +39 |
| 3.3 | IPTC complete parsing | iptc_parser.rs | +9 |
| 3.4 | APP segment parsing | app_parsers.rs | +80 |
| 3.5 | Photoshop IRB parsing | photoshop_parser.rs (new) | +20 |
| 3.6 | XMP extended namespaces | xmp/rdf_parser.rs | +29 |

---

## Success Metrics

| Metric | Current | Wave 1 | Wave 2 | Wave 3 |
|--------|---------|--------|--------|--------|
| JPEG Coverage | 3.7% | ~17% | ~45% | 50%+ |
| Matched Tags | 51 | 230 | 610 | 680+ |
| Missing Tags | 1256 | 1077 | 697 | <630 |
| Value Diffs | 55 | <10 | <10 | <10 |

---

## Verification Commands

```bash
# After each wave
just test
just compare-exiftool-format jpeg

# Check specific improvements
cat comparison.json | jq '.by_format.JPEG | {
  coverage: .coverage_percentage,
  matched: (.matched_tags | length),
  missing: (.missing_in_oxidex | length),
  extra: (.extra_in_oxidex | length)
}'

# Check MakerNotes specifically
cat comparison.json | jq '[.by_format.JPEG.missing_in_oxidex[] |
  select(.family == "MakerNotes")] | length'
```

---

## Risk Mitigation

1. **Camera model variations:** MakerNotes structures vary by camera model
   - Mitigation: Start with most common models, add fallback parsers

2. **Binary structure complexity:** Some tags require complex decoding
   - Mitigation: Reference ExifTool's Perl source for exact byte offsets

3. **Test coverage:** Need test files from various cameras
   - Mitigation: ExifTool test suite has good variety

4. **Regression risk:** Changes could break existing extractions
   - Mitigation: Run `just compare-exiftool` before/after each wave

---

## References

- ExifTool source: https://github.com/exiftool/exiftool
- Canon MakerNotes: `lib/Image/ExifTool/Canon.pm`
- Nikon MakerNotes: `lib/Image/ExifTool/Nikon.pm`
- Sony MakerNotes: `lib/Image/ExifTool/Sony.pm`
