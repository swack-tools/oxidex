# Comprehensive Coverage Improvement Plan

**Generated:** 2025-12-07
**Current Overall:** 178/3,580 tags (5.0%)
**Current JPEG:** 51/1,362 tags (3.7%)
**Target Overall:** 25-35%
**Target JPEG:** 50%+

---

## Executive Summary

Analysis reveals we're **already extracting most of the data** but outputting it with wrong names. A massive opportunity exists in tag family normalization before any new parsing work.

### Impact Breakdown

| Phase | Work Type | Effort | Impact |
|-------|-----------|--------|--------|
| **Phase 1** | Tag Family Renaming | LOW | +400-500 tags |
| **Phase 2** | Value Formatting | LOW | +100 tags |
| **Phase 3** | MakerNotes Deep Parsing | HIGH | +400-500 tags |
| **Phase 4** | Other Families | MEDIUM | +150-200 tags |

**Total Potential:** +1,050-1,300 new matched tags

---

## Current State Analysis

### Tags We Extract But Name Wrong ("Extra" Tags)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    EXTRA TAGS BY FAMILY (608 total)                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  IFD0: 195 tags ──────────────────────────────────────► EXIF:      │
│  ExifIFD: 124 tags ───────────────────────────────────► EXIF:      │
│  Profile: 78 tags ────────────────────────────────────► ICC_Profile│
│  SubIFD0: 53 tags ────────────────────────────────────► EXIF:      │
│  Canon: 52 tags ──────────────────────────────────────► MakerNotes │
│  GPS: 27 tags ────────────────────────────────────────► EXIF:      │
│  XMP-photoshop: 28 tags ──────────────────────────────► XMP:       │
│  RIFF: 23 tags ───────────────────────────────────────► (fix names)│
│  Matroska: 13 tags ───────────────────────────────────► (fix names)│
│  ID3/ID3v1: 16 tags ──────────────────────────────────► ID3:       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Value Differences (141 total across all formats)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    VALUE FORMATTING ISSUES                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Rationals:     360/100 ──────────────────────────────► 3.6        │
│  Enums:         0 ────────────────────────────────────► Normal     │
│  Dates:         2001-05-19T18:36:41 ──────────────────► 2001:05:19 │
│  File sizes:    26.1 kB ──────────────────────────────► 26 kB      │
│  Binary:        [Binary data] ────────────────────────► 0100       │
│  Units:         75 ───────────────────────────────────► 75 mm      │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Phase 1: Tag Family Normalization

**Effort:** LOW (single file changes)
**Impact:** +400-500 matched tags
**Parallelism:** 4 agents

### Stream 1.1: TIFF/EXIF Family Normalization

**Files:** `src/core/tag_normalization.rs`
**Impact:** +300-350 tags

Map all TIFF IFD variants to ExifTool's `EXIF:` family:

```rust
// src/core/tag_normalization.rs

use std::collections::HashMap;

/// Normalize tag family prefixes to match ExifTool conventions
pub fn normalize_tag_family(tag_name: &str) -> String {
    if let Some((family, name)) = tag_name.split_once(':') {
        let normalized_family = match family {
            // TIFF IFD families → EXIF
            "IFD0" | "IFD1" | "IFD2" | "IFD3" => "EXIF",
            "ExifIFD" => "EXIF",
            "SubIFD" | "SubIFD0" | "SubIFD1" | "SubIFD2" => "EXIF",
            "InteropIFD" => "EXIF",

            // GPS → EXIF (ExifTool groups GPS under EXIF family)
            "GPS" => "EXIF",

            // ICC Profile
            "Profile" => "ICC_Profile",

            // Camera MakerNotes → MakerNotes
            "Canon" | "CanonCustom" | "CanonRaw" => "MakerNotes",
            "Nikon" | "NikonCustom" | "NikonCapture" => "MakerNotes",
            "Sony" | "SonyIDC" => "MakerNotes",
            "Fujifilm" | "FujiFilm" => "MakerNotes",
            "Olympus" => "MakerNotes",
            "Panasonic" | "PanasonicRaw" => "MakerNotes",
            "Pentax" => "MakerNotes",
            "Samsung" => "MakerNotes",
            "Apple" => "MakerNotes",
            "DJI" => "MakerNotes",
            "GoPro" => "MakerNotes",
            "Leica" => "MakerNotes",
            "Sigma" | "SigmaRaw" => "MakerNotes",
            "Minolta" | "MinoltaRaw" => "MakerNotes",
            "Casio" => "MakerNotes",
            "Kodak" => "MakerNotes",
            "Ricoh" => "MakerNotes",
            "Sanyo" => "MakerNotes",
            "HP" => "MakerNotes",

            // Keep as-is
            _ => family,
        };
        format!("{}:{}", normalized_family, name)
    } else {
        tag_name.to_string()
    }
}

/// Apply normalization to entire metadata map
pub fn normalize_metadata_map(tags: HashMap<String, String>) -> HashMap<String, String> {
    tags.into_iter()
        .map(|(k, v)| (normalize_tag_family(&k), v))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifd_normalization() {
        assert_eq!(normalize_tag_family("IFD0:Make"), "EXIF:Make");
        assert_eq!(normalize_tag_family("IFD1:Compression"), "EXIF:Compression");
        assert_eq!(normalize_tag_family("ExifIFD:ExposureTime"), "EXIF:ExposureTime");
        assert_eq!(normalize_tag_family("SubIFD0:ImageWidth"), "EXIF:ImageWidth");
    }

    #[test]
    fn test_gps_normalization() {
        assert_eq!(normalize_tag_family("GPS:GPSLatitude"), "EXIF:GPSLatitude");
        assert_eq!(normalize_tag_family("GPS:GPSAltitude"), "EXIF:GPSAltitude");
    }

    #[test]
    fn test_profile_normalization() {
        assert_eq!(normalize_tag_family("Profile:ColorSpace"), "ICC_Profile:ColorSpace");
    }

    #[test]
    fn test_makernotes_normalization() {
        assert_eq!(normalize_tag_family("Canon:MacroMode"), "MakerNotes:MacroMode");
        assert_eq!(normalize_tag_family("Nikon:ISO"), "MakerNotes:ISO");
        assert_eq!(normalize_tag_family("Sony:Quality"), "MakerNotes:Quality");
    }
}
```

### Stream 1.2: XMP Namespace Normalization

**Files:** `src/parsers/xmp/namespace_mapping.rs`
**Impact:** +30-40 tags

```rust
// src/parsers/xmp/namespace_mapping.rs

/// Map XMP namespace prefixes to ExifTool conventions
pub fn normalize_xmp_family(family: &str, tag_name: &str) -> String {
    match family {
        // Common namespaces use simple XMP: prefix
        "XMP-dc" | "XMP-xmp" | "XMP-xmpMM" | "XMP-xmpRights" => {
            format!("XMP:{}", tag_name)
        }

        // Photoshop namespace → XMP: for common tags
        "XMP-photoshop" => {
            match tag_name {
                // These map to simple XMP: prefix
                "AuthorsPosition" | "CaptionWriter" | "Category" |
                "City" | "Country" | "Credit" | "DateCreated" |
                "Headline" | "Instructions" | "Source" | "State" |
                "SupplementalCategories" | "TransmissionReference" |
                "Urgency" => format!("XMP:{}", tag_name),
                // Others keep photoshop prefix
                _ => format!("XMP-photoshop:{}", tag_name),
            }
        }

        // IPTC Core → XMP:
        "XMP-iptcCore" => format!("XMP:{}", tag_name),

        // IPTC Extension keeps prefix
        "XMP-iptcExt" => format!("XMP-iptcExt:{}", tag_name),

        // Specialized namespaces keep their prefixes
        "XMP-exif" | "XMP-tiff" | "XMP-crs" | "XMP-lr" => {
            format!("{}:{}", family, tag_name)
        }

        // Default: use as-is
        _ => format!("{}:{}", family, tag_name),
    }
}
```

### Stream 1.3: Media Container Normalization

**Files:** `src/parsers/riff/metadata.rs`, `src/parsers/matroska/metadata.rs`
**Impact:** +40-50 tags

```rust
// Normalize RIFF stream tags to ExifTool format
// We output: RIFF:Stream1:BitDepth
// ExifTool: RIFF:AudioBitDepth or RIFF:VideoBitDepth

pub fn normalize_riff_tag(tag_name: &str) -> String {
    // Remove Stream prefixes and map to ExifTool names
    if tag_name.contains(":Stream") {
        // RIFF:Stream1:BitDepth → RIFF:AudioBitDepth (if audio stream)
        // Complex mapping based on stream type
    }
    tag_name.to_string()
}

// Normalize Matroska track tags
// We output: Matroska:Track1:CodecID
// ExifTool: Matroska:VideoCodecID or Matroska:AudioCodecID

pub fn normalize_matroska_tag(tag_name: &str) -> String {
    // Similar stream-to-type mapping
    tag_name.to_string()
}
```

### Stream 1.4: ID3 Tag Normalization

**Files:** `src/parsers/id3/metadata.rs`
**Impact:** +15-20 tags

```rust
// Normalize ID3v1 → ID3 and fix tag names
// We output: ID3v1:Album, ID3:TAL
// ExifTool: ID3:Album

pub fn normalize_id3_tag(family: &str, tag_name: &str) -> String {
    // ID3v1 → ID3
    let normalized_family = match family {
        "ID3v1" => "ID3",
        _ => family,
    };

    // Map ID3v2 frame codes to readable names
    let normalized_name = match tag_name {
        "TAL" | "TALB" => "Album",
        "TCM" | "TCOM" => "Composer",
        "TCO" | "TCON" => "Genre",
        "TP1" | "TPE1" => "Artist",
        "TPA" | "TPOS" => "PartOfSet",
        "TRK" | "TRCK" => "Track",
        "TT2" | "TIT2" => "Title",
        "TYE" | "TYER" => "Year",
        _ => tag_name,
    };

    format!("{}:{}", normalized_family, normalized_name)
}
```

---

## Phase 2: Value Formatting

**Effort:** LOW-MEDIUM
**Impact:** +100 matched tags
**Parallelism:** 4 agents

### Stream 2.1: Rational to Decimal Conversion

**Files:** `src/core/value_formatter.rs`
**Impact:** +40 tags

```rust
// src/core/value_formatter.rs

/// Tags that should display as decimal instead of rational
const DECIMAL_TAGS: &[&str] = &[
    // Aperture/exposure
    "ApertureValue", "MaxApertureValue", "FNumber",
    "ShutterSpeedValue", "ExposureTime", "BrightnessValue",

    // Other rationals displayed as decimal
    "CompressedBitsPerPixel", "DigitalZoomRatio", "ExposureCompensation",
    "ExposureBiasValue", "Gamma", "FocalLength", "FocalPlaneXResolution",
    "FocalPlaneYResolution", "XResolution", "YResolution",
    "SubjectDistance", "GPSDOP", "GPSSpeed",
];

/// Convert rational to decimal string
pub fn format_rational_as_decimal(numerator: i64, denominator: i64) -> String {
    if denominator == 0 {
        return "inf".to_string();
    }

    let value = numerator as f64 / denominator as f64;

    // Clean formatting: no trailing zeros, reasonable precision
    if value.fract().abs() < 0.0001 {
        format!("{}", value as i64)
    } else {
        let s = format!("{:.6}", value);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Format value based on tag name
pub fn format_tag_value(tag_name: &str, value: &str) -> String {
    let base_name = tag_name.split(':').last().unwrap_or(tag_name);

    // Check if this is a rational that should be decimal
    if DECIMAL_TAGS.contains(&base_name) {
        if let Some((num, den)) = parse_rational(value) {
            return format_rational_as_decimal(num, den);
        }
    }

    value.to_string()
}

fn parse_rational(s: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        let num = parts[0].trim().parse().ok()?;
        let den = parts[1].trim().parse().ok()?;
        Some((num, den))
    } else {
        None
    }
}
```

### Stream 2.2: EXIF Enum Lookups

**Files:** `src/core/exif_enums.rs` (new)
**Impact:** +25 tags

```rust
// src/core/exif_enums.rs

use std::collections::HashMap;
use once_cell::sync::Lazy;

/// All EXIF enum tag decoders
pub static EXIF_ENUMS: Lazy<HashMap<&'static str, HashMap<u32, &'static str>>> = Lazy::new(|| {
    let mut enums = HashMap::new();

    // ColorSpace (0xA001)
    enums.insert("ColorSpace", HashMap::from([
        (1, "sRGB"),
        (2, "Adobe RGB"),
        (0xFFFF, "Uncalibrated"),
    ]));

    // Contrast (0xA408)
    enums.insert("Contrast", HashMap::from([
        (0, "Normal"),
        (1, "Low"),
        (2, "High"),
    ]));

    // CustomRendered (0xA401)
    enums.insert("CustomRendered", HashMap::from([
        (0, "Normal"),
        (1, "Custom"),
    ]));

    // ExposureMode (0xA402)
    enums.insert("ExposureMode", HashMap::from([
        (0, "Auto"),
        (1, "Manual"),
        (2, "Auto bracket"),
    ]));

    // ExposureProgram (0x8822)
    enums.insert("ExposureProgram", HashMap::from([
        (0, "Not Defined"),
        (1, "Manual"),
        (2, "Program AE"),
        (3, "Aperture-priority AE"),
        (4, "Shutter speed priority AE"),
        (5, "Creative (Slow speed)"),
        (6, "Action (High speed)"),
        (7, "Portrait"),
        (8, "Landscape"),
    ]));

    // Flash (0x9209) - complex bitmap
    enums.insert("Flash", HashMap::from([
        (0x00, "No Flash"),
        (0x01, "Fired"),
        (0x05, "Fired, Return not detected"),
        (0x07, "Fired, Return detected"),
        (0x08, "On, Did not fire"),
        (0x09, "On, Fired"),
        (0x0D, "On, Return not detected"),
        (0x0F, "On, Return detected"),
        (0x10, "Off, Did not fire"),
        (0x14, "Off, Did not fire, Return not detected"),
        (0x18, "Auto, Did not fire"),
        (0x19, "Auto, Fired"),
        (0x1D, "Auto, Fired, Return not detected"),
        (0x1F, "Auto, Fired, Return detected"),
        (0x20, "No flash function"),
        (0x41, "Fired, Red-eye reduction"),
        (0x45, "Fired, Red-eye reduction, Return not detected"),
        (0x47, "Fired, Red-eye reduction, Return detected"),
        (0x49, "On, Red-eye reduction"),
        (0x4D, "On, Red-eye reduction, Return not detected"),
        (0x4F, "On, Red-eye reduction, Return detected"),
        (0x59, "Auto, Fired, Red-eye reduction"),
        (0x5D, "Auto, Fired, Red-eye reduction, Return not detected"),
        (0x5F, "Auto, Fired, Red-eye reduction, Return detected"),
    ]));

    // GainControl (0xA407)
    enums.insert("GainControl", HashMap::from([
        (0, "None"),
        (1, "Low gain up"),
        (2, "High gain up"),
        (3, "Low gain down"),
        (4, "High gain down"),
    ]));

    // LightSource (0x9208)
    enums.insert("LightSource", HashMap::from([
        (0, "Unknown"),
        (1, "Daylight"),
        (2, "Fluorescent"),
        (3, "Tungsten (Incandescent)"),
        (4, "Flash"),
        (9, "Fine Weather"),
        (10, "Cloudy"),
        (11, "Shade"),
        (12, "Daylight Fluorescent"),
        (13, "Day White Fluorescent"),
        (14, "Cool White Fluorescent"),
        (15, "White Fluorescent"),
        (16, "Warm White Fluorescent"),
        (17, "Standard Light A"),
        (18, "Standard Light B"),
        (19, "Standard Light C"),
        (20, "D55"),
        (21, "D65"),
        (22, "D75"),
        (23, "D50"),
        (24, "ISO Studio Tungsten"),
        (255, "Other"),
    ]));

    // MeteringMode (0x9207)
    enums.insert("MeteringMode", HashMap::from([
        (0, "Unknown"),
        (1, "Average"),
        (2, "Center-weighted average"),
        (3, "Spot"),
        (4, "Multi-spot"),
        (5, "Multi-segment"),
        (6, "Partial"),
        (255, "Other"),
    ]));

    // Orientation (0x0112)
    enums.insert("Orientation", HashMap::from([
        (1, "Horizontal (normal)"),
        (2, "Mirror horizontal"),
        (3, "Rotate 180"),
        (4, "Mirror vertical"),
        (5, "Mirror horizontal and rotate 270 CW"),
        (6, "Rotate 90 CW"),
        (7, "Mirror horizontal and rotate 90 CW"),
        (8, "Rotate 270 CW"),
    ]));

    // ResolutionUnit (0x0128)
    enums.insert("ResolutionUnit", HashMap::from([
        (1, "None"),
        (2, "inches"),
        (3, "cm"),
    ]));

    // Saturation (0xA409)
    enums.insert("Saturation", HashMap::from([
        (0, "Normal"),
        (1, "Low"),
        (2, "High"),
    ]));

    // SceneCaptureType (0xA406)
    enums.insert("SceneCaptureType", HashMap::from([
        (0, "Standard"),
        (1, "Landscape"),
        (2, "Portrait"),
        (3, "Night"),
    ]));

    // SensingMethod (0xA217)
    enums.insert("SensingMethod", HashMap::from([
        (1, "Not defined"),
        (2, "One-chip color area"),
        (3, "Two-chip color area"),
        (4, "Three-chip color area"),
        (5, "Color sequential area"),
        (7, "Trilinear"),
        (8, "Color sequential linear"),
    ]));

    // Sharpness (0xA40A)
    enums.insert("Sharpness", HashMap::from([
        (0, "Normal"),
        (1, "Soft"),
        (2, "Hard"),
    ]));

    // SubjectDistanceRange (0xA40C)
    enums.insert("SubjectDistanceRange", HashMap::from([
        (0, "Unknown"),
        (1, "Macro"),
        (2, "Close"),
        (3, "Distant"),
    ]));

    // WhiteBalance (0xA403)
    enums.insert("WhiteBalance", HashMap::from([
        (0, "Auto"),
        (1, "Manual"),
    ]));

    // YCbCrPositioning (0x0213)
    enums.insert("YCbCrPositioning", HashMap::from([
        (1, "Centered"),
        (2, "Co-sited"),
    ]));

    // FocalPlaneResolutionUnit (0xA210)
    enums.insert("FocalPlaneResolutionUnit", HashMap::from([
        (1, "None"),
        (2, "inches"),
        (3, "cm"),
        (4, "mm"),
        (5, "um"),
    ]));

    enums
});

/// Decode an EXIF enum value to its string representation
pub fn decode_exif_enum(tag_name: &str, value: u32) -> Option<String> {
    let base_name = tag_name.split(':').last()?;
    EXIF_ENUMS.get(base_name)?.get(&value).map(|s| s.to_string())
}
```

### Stream 2.3: Date Formatting

**Files:** `src/core/value_formatter.rs`
**Impact:** +15 tags

```rust
/// Convert ISO 8601 date to EXIF date format
/// "2001-05-19T18:36:41+00:00" → "2001:05:19 18:36:41"
pub fn format_date_exif_style(iso_date: &str) -> String {
    // Handle ISO 8601 format
    if iso_date.contains('T') {
        let date_part = iso_date.split('T').next().unwrap_or(iso_date);
        let time_part = iso_date.split('T').nth(1).unwrap_or("");

        // Convert dashes to colons in date
        let formatted_date = date_part.replace('-', ":");

        // Extract time without timezone for basic EXIF dates
        let time_clean = time_part
            .split('+').next().unwrap_or(time_part)
            .split('-').next().unwrap_or(time_part)
            .split('Z').next().unwrap_or(time_part);

        if time_clean.is_empty() {
            formatted_date
        } else {
            format!("{} {}", formatted_date, time_clean)
        }
    } else {
        iso_date.to_string()
    }
}

/// Tags that use EXIF date format
const EXIF_DATE_TAGS: &[&str] = &[
    "CreateDate", "DateTimeOriginal", "ModifyDate", "DateTime",
    "DateTimeDigitized", "GPSDateStamp", "DateCreated",
];
```

### Stream 2.4: Binary Data Decoding & Unit Suffixes

**Files:** `src/core/binary_decoders.rs` (new), `src/core/value_formatter.rs`
**Impact:** +20 tags

```rust
// src/core/binary_decoders.rs

/// Decode EXIF/FlashPix version (4 ASCII bytes)
pub fn decode_version(data: &[u8]) -> Option<String> {
    if data.len() >= 4 && data.iter().all(|&b| b.is_ascii_digit()) {
        Some(String::from_utf8_lossy(&data[0..4]).to_string())
    } else {
        None
    }
}

/// Decode FileSource (tag 0xA300)
pub fn decode_file_source(value: u8) -> &'static str {
    match value {
        1 => "Film Scanner",
        2 => "Reflection Print Scanner",
        3 => "Digital Camera",
        _ => "Unknown",
    }
}

/// Decode SceneType (tag 0xA301)
pub fn decode_scene_type(value: u8) -> &'static str {
    match value {
        1 => "Directly photographed",
        _ => "Unknown",
    }
}

// src/core/value_formatter.rs

/// Tags that need unit suffixes
const MM_SUFFIX_TAGS: &[&str] = &["FocalLength", "FocalLengthIn35mmFormat"];
const METER_SUFFIX_TAGS: &[&str] = &["SubjectDistance", "GPSAltitude", "GPSHPositioningError"];

/// Add unit suffix to value
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

/// Format file size to match ExifTool
/// "26.1 kB" → "26 kB" (ExifTool rounds to nearest kB)
pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} bytes", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{} kB", bytes / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
```

---

## Phase 3: MakerNotes Deep Parsing (JPEG 50% Target)

**Effort:** HIGH
**Impact:** +400-500 tags
**Parallelism:** 10 agents

This phase is required to hit 50%+ JPEG coverage since MakerNotes = 708/1256 missing JPEG tags (56%).

### Stream 3.1: Canon CameraInfo Parser

**Files:** `src/parsers/tiff/makernotes/canon_camera_info.rs` (new)
**Impact:** +150-200 tags

Parse Canon's CameraInfo (tag 0x000D) binary structure containing:
- AE (Auto Exposure): AEAperture, AEExposureTime, AEMaxAperture, AEMeteringMode
- AF (Auto Focus): AFAreaHeight, AFAreaWidth, AFPointsInFocus
- WB (White Balance): WBShiftAB, WBShiftGM

```rust
/// Parse Canon CameraInfo based on camera model
pub fn parse_camera_info(data: &[u8], model_id: u32) -> HashMap<String, String> {
    match model_id {
        // EOS 5D3, 6D, 70D family
        0x80000285 | 0x80000302 | 0x80000325 => parse_camera_info_5d3(data),
        // EOS R series
        0x80000424 | 0x80000453 | 0x80000464 => parse_camera_info_eos_r(data),
        // Fallback for unknown models
        _ => parse_camera_info_generic(data),
    }
}
```

### Stream 3.2: Canon AFInfo2/AFInfo3 Parser (+50 tags)
### Stream 3.3: Nikon MakerNotes Enhancement (+80 tags)
### Stream 3.4: Sony MakerNotes Enhancement (+60 tags)
### Stream 3.5: Fujifilm MakerNotes Enhancement (+40 tags)
### Stream 3.6: Olympus MakerNotes Enhancement (+30 tags)
### Stream 3.7: Panasonic MakerNotes Enhancement (+25 tags)
### Stream 3.8: Pentax MakerNotes Enhancement (+20 tags)
### Stream 3.9: Generic MakerNotes Parser Improvements (+25 tags)
### Stream 3.10: Complete Tag ID Registry

---

## Phase 4: Other Missing Families

**Effort:** MEDIUM
**Impact:** +150-200 tags
**Parallelism:** 6 agents

### Stream 4.1: ICC Profile Complete (+39 tags)
### Stream 4.2: Composite Tag Calculation (+39 tags)
### Stream 4.3: IPTC Complete Parsing (+9 tags)
### Stream 4.4: APP Segment Parsing (+80 tags)
### Stream 4.5: Photoshop IRB Parsing (+20 tags)
### Stream 4.6: XMP Extended Namespaces (+29 tags)

---

## Execution Plan

### Wave 1: Quick Wins (Phases 1 + 2)

**8 parallel agents, expected completion: fast**

```
┌─────────────────────────────────────────────────────────────────────┐
│                        WAVE 1: QUICK WINS                           │
│                    8 Parallel Agents                                │
├────────┬────────┬────────┬────────┬────────┬────────┬────────┬─────┤
│1.1     │1.2     │1.3     │1.4     │2.1     │2.2     │2.3     │2.4  │
│TIFF/   │XMP     │Media   │ID3     │Rational│Enum    │Date    │Bin/ │
│EXIF    │Names   │Contain │Normal  │Format  │Lookup  │Format  │Unit │
│Normal  │        │        │        │        │        │        │     │
└────────┴────────┴────────┴────────┴────────┴────────┴────────┴─────┘
                              │
                              ▼
                    ┌───────────────────┐
                    │  Integration &    │
                    │  Test Validation  │
                    └───────────────────┘
                              │
                              ▼
                    Expected: 5% → 20%+ overall
                    JPEG: 3.7% → 15-20%
```

### Wave 2: MakerNotes (Phase 3)

**10 parallel agents**

```
┌─────────────────────────────────────────────────────────────────────┐
│                     WAVE 2: MAKERNOTES                              │
│                    10 Parallel Agents                               │
├──────┬──────┬──────┬──────┬──────┬──────┬──────┬──────┬──────┬─────┤
│Canon │Canon │Nikon │Sony  │Fuji  │Olymp │Pana  │Pent  │Gener │Reg  │
│Cam   │AF    │      │      │      │      │sonic │ax    │ic    │istry│
│Info  │Info  │      │      │      │      │      │      │      │     │
└──────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┴─────┘
                              │
                              ▼
                    Expected: 20% → 35%+ overall
                    JPEG: 20% → 45%
```

### Wave 3: Other Families (Phase 4)

**6 parallel agents**

```
┌─────────────────────────────────────────────────────────────────────┐
│                    WAVE 3: OTHER FAMILIES                           │
│                    6 Parallel Agents                                │
├──────────┬──────────┬──────────┬──────────┬──────────┬─────────────┤
│ICC       │Composite │IPTC      │APP       │Photoshop │XMP          │
│Profile   │Tags      │Complete  │Segments  │IRB       │Extended     │
└──────────┴──────────┴──────────┴──────────┴──────────┴─────────────┘
                              │
                              ▼
                    Expected: 35% → 40%+ overall
                    JPEG: 45% → 50%+
```

---

## Expected Results

### After Wave 1 (Quick Wins)

| Format | Before | After | Change |
|--------|--------|-------|--------|
| Overall | 5.0% | 20%+ | +15% |
| JPEG | 3.7% | 18% | +14% |
| TIFF | 6.7% | 25% | +18% |
| CR2 | 1.0% | 15% | +14% |
| DNG | 1.3% | 18% | +17% |
| NEF | 2.2% | 16% | +14% |
| PNG | 64% | 70% | +6% |
| MP3 | 17.5% | 35% | +17% |

### After Wave 2 (MakerNotes)

| Format | Before | After | Change |
|--------|--------|-------|--------|
| Overall | 20% | 35%+ | +15% |
| JPEG | 18% | 45% | +27% |
| CR2 | 15% | 40% | +25% |
| NEF | 16% | 35% | +19% |

### After Wave 3 (Other Families)

| Format | Before | After | Change |
|--------|--------|-------|--------|
| Overall | 35% | 40%+ | +5% |
| JPEG | 45% | 50%+ | +5% |

---

## Verification Commands

```bash
# After each wave
just test
just compare-exiftool

# Check specific format improvements
just compare-exiftool-format jpeg
just compare-exiftool-format cr2

# Detailed analysis
cat comparison.json | jq '.by_format.JPEG | {
  coverage: .coverage_percentage,
  matched: (.matched_tags | length),
  missing: (.missing_in_oxidex | length),
  extra: (.extra_in_oxidex | length),
  diff: (.value_differences | length)
}'

# Check family-specific progress
cat comparison.json | jq '[.by_format.JPEG.extra_in_oxidex[].family] |
  group_by(.) | map({family: .[0], count: length}) | sort_by(-.count)'
```

---

## Risk Mitigation

1. **Breaking existing tests**
   - Run `just test` after each stream
   - Keep normalization functions backward-compatible

2. **Over-normalizing (losing information)**
   - Only normalize when ExifTool uses different names for same data
   - Preserve original names in debug output if needed

3. **Camera model variations in MakerNotes**
   - Start with most common models
   - Implement fallback generic parsers
   - Reference ExifTool source for exact structures

4. **Performance regression**
   - Keep normalization maps as static/lazy constants
   - Profile before/after if concerned

---

## Success Criteria

| Metric | Target |
|--------|--------|
| Overall coverage | 25%+ after Wave 1, 40%+ after Wave 3 |
| JPEG coverage | 50%+ after Wave 3 |
| Zero regressions | Matched tags never decrease |
| All tests pass | `just test` succeeds |
| No new clippy warnings | `just lint` clean |

---

## Files Summary

### Phase 1 Files (Tag Normalization)
- `src/core/tag_normalization.rs` - Main normalization logic
- `src/parsers/xmp/namespace_mapping.rs` - XMP prefix mapping
- `src/parsers/riff/metadata.rs` - RIFF tag fixes
- `src/parsers/matroska/metadata.rs` - Matroska tag fixes
- `src/parsers/id3/metadata.rs` - ID3 normalization

### Phase 2 Files (Value Formatting)
- `src/core/value_formatter.rs` - Rational, date, unit formatting
- `src/core/exif_enums.rs` (NEW) - EXIF enum decoders
- `src/core/binary_decoders.rs` (NEW) - Binary field decoders

### Phase 3 Files (MakerNotes)
- `src/parsers/tiff/makernotes/canon_camera_info.rs` (NEW)
- `src/parsers/tiff/makernotes/canon_af_info.rs` (NEW)
- `src/parsers/tiff/makernotes/nikon.rs` (enhance)
- `src/parsers/tiff/makernotes/sony.rs` (enhance)
- `src/parsers/tiff/makernotes/fujifilm.rs` (enhance)
- Plus 5 more camera brand files

### Phase 4 Files (Other Families)
- `src/parsers/icc/profile_parser.rs` (enhance)
- `src/core/composite_tags.rs` (NEW)
- `src/parsers/iptc/parser.rs` (enhance)
- `src/parsers/jpeg/app_parsers.rs` (enhance)
- `src/parsers/jpeg/photoshop_parser.rs` (NEW)
- `src/parsers/xmp/rdf_parser.rs` (enhance)
