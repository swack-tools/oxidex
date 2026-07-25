# ExifTool Coverage Improvement Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Increase OxiDex coverage from 5.1% to 50%+ across all comparison formats by fixing tag naming, adding missing parsers, and matching ExifTool's value formatting.

**Architecture:** Six parallel workstreams targeting independent parser domains. Each stream can be worked by a separate subagent. Streams 1, 5, and 6 are cross-cutting concerns affecting all formats. Streams 2, 3, and 4 focus on specific format families.

**Tech Stack:** Rust, serde, chrono, the existing parser infrastructure in `src/parsers/`

---

## Executive Summary

**Current State:**
| Format | Coverage | Missing Tags | Priority |
|--------|----------|--------------|----------|
| JPEG | 2.2% | 1325 | CRITICAL |
| MP4 | 0.0% | 99 | CRITICAL |
| CR2/NEF/DNG | 1.6-2.6% | 500+ | HIGH |
| TIFF | 7.7% | ~100 | MEDIUM |
| PNG | 68.0% | 7 | LOW |

**Root Causes:**
1. Tag family prefixes don't match ExifTool (`ExifIFD:` vs `EXIF:`)
2. JPEG APP segments not fully parsed (APP0, APP1-FLIR, APP11-HDR, APP13, APP15)
3. QuickTime atom metadata not extracted (0% coverage)
4. MakerNotes parsed but not output correctly
5. Value formatting differs (dates, sizes, rationals)

**Parallel Workstreams:**
- **Stream 1:** Tag Normalization (family prefix alignment)
- **Stream 2:** JPEG APP Segments (missing segment parsers)
- **Stream 3:** QuickTime/MP4 (atom-to-tag extraction)
- **Stream 4:** RAW Format MakerNotes (CR2, NEF, DNG, RAF, RW2)
- **Stream 5:** Value Formatting (dates, sizes, rationals)
- **Stream 6:** XMP Namespace Normalization

---

## Stream 1: Tag Normalization

**Goal:** Align tag family prefixes with ExifTool conventions.

**Problem:** OxiDex uses `ExifIFD:`, `IFD0:`, `IFD1:` but ExifTool uses `EXIF:`, `IFD0:`, `IFD1:` with different grouping.

### Task 1.1: Create Tag Normalization Module

**Files:**
- Create: `src/core/tag_normalization.rs`
- Modify: `src/core/mod.rs`

**Step 1: Write the failing test**

Create `tests/tag_normalization_tests.rs`:

```rust
use oxidex::core::tag_normalization::normalize_tag_family;

#[test]
fn test_exififd_to_exif() {
    assert_eq!(normalize_tag_family("ExifIFD:Make"), "EXIF:Make");
    assert_eq!(normalize_tag_family("ExifIFD:Model"), "EXIF:Model");
}

#[test]
fn test_ifd0_unchanged() {
    assert_eq!(normalize_tag_family("IFD0:Make"), "IFD0:Make");
}

#[test]
fn test_unknown_family_unchanged() {
    assert_eq!(normalize_tag_family("Custom:Tag"), "Custom:Tag");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test tag_normalization_tests`
Expected: FAIL with "cannot find crate"

**Step 3: Write the implementation**

Create `src/core/tag_normalization.rs`:

```rust
//! Tag family normalization to match ExifTool conventions

use std::collections::HashMap;
use std::sync::LazyLock;

/// Family prefix mappings from OxiDex to ExifTool conventions
static FAMILY_MAPPINGS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // EXIF IFD mapping
    m.insert("ExifIFD", "EXIF");
    // GPS IFD mapping
    m.insert("GPS", "GPS");
    // InteropIFD mapping
    m.insert("InteropIFD", "InteropIFD");
    // Maker note families
    m.insert("Canon", "Canon");
    m.insert("Nikon", "Nikon");
    m.insert("Sony", "Sony");
    m
});

/// Normalize a tag key to match ExifTool family conventions
///
/// # Arguments
/// * `tag_key` - Full tag key like "ExifIFD:Make"
///
/// # Returns
/// Normalized key like "EXIF:Make"
pub fn normalize_tag_family(tag_key: &str) -> String {
    if let Some((family, name)) = tag_key.split_once(':') {
        if let Some(normalized) = FAMILY_MAPPINGS.get(family) {
            return format!("{}:{}", normalized, name);
        }
    }
    tag_key.to_string()
}

/// Normalize all tags in a MetadataMap
pub fn normalize_metadata_map(
    map: &crate::core::MetadataMap,
) -> crate::core::MetadataMap {
    let mut normalized = crate::core::MetadataMap::with_capacity(map.len());
    for (key, value) in map.iter() {
        let normalized_key = normalize_tag_family(key);
        normalized.insert(normalized_key, value.clone());
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exififd_normalization() {
        assert_eq!(normalize_tag_family("ExifIFD:Make"), "EXIF:Make");
    }

    #[test]
    fn test_no_colon_unchanged() {
        assert_eq!(normalize_tag_family("NoColonHere"), "NoColonHere");
    }
}
```

**Step 4: Update mod.rs to export**

Add to `src/core/mod.rs`:

```rust
pub mod tag_normalization;
pub use tag_normalization::{normalize_tag_family, normalize_metadata_map};
```

**Step 5: Run tests**

Run: `cargo test tag_normalization`
Expected: PASS

**Step 6: Commit**

```bash
git add src/core/tag_normalization.rs src/core/mod.rs tests/tag_normalization_tests.rs
git commit -m "feat: add tag family normalization for ExifTool compatibility"
```

### Task 1.2: Apply Normalization to JPEG Parser Output

**Files:**
- Modify: `src/parsers/jpeg/exif_parser.rs`

**Step 1: Identify insertion point**

The JPEG EXIF parser returns a MetadataMap. Apply normalization before returning.

**Step 2: Modify parse function**

At the end of `parse_exif_segment()` function, before returning:

```rust
use crate::core::tag_normalization::normalize_metadata_map;

// ... existing parsing code ...

// Normalize tag families before returning
let normalized = normalize_metadata_map(&metadata);
Ok(normalized)
```

**Step 3: Run comparison tests**

Run: `cargo run --bin tag-comparison -- --format jpeg`
Expected: More tags should match

**Step 4: Commit**

```bash
git add src/parsers/jpeg/exif_parser.rs
git commit -m "fix(jpeg): apply tag family normalization to EXIF output"
```

### Task 1.3: Apply Normalization to TIFF Parser

**Files:**
- Modify: `src/parsers/tiff/mod.rs`

Apply same pattern as Task 1.2 to TIFF parser output.

### Task 1.4: Apply Normalization to RAW Parsers

**Files:**
- Modify: `src/parsers/raw/mod.rs`

Apply same pattern to all RAW format parsers (CR2, NEF, DNG, RAF, RW2).

---

## Stream 2: JPEG APP Segment Parsing

**Goal:** Add missing JPEG APP segment parsers to extract tags ExifTool finds.

**Missing Segments (from comparison):**
- APP0: JFIF extended fields, interlace info
- APP1: FLIR thermal metadata
- APP11: JPEG-HDR metadata
- APP13: IPTC-NAA (extended)
- APP15: Quality settings

### Task 2.1: Add APP0 Extended Parser

**Files:**
- Modify: `src/parsers/jpeg/app_parsers.rs`

**Step 1: Write the failing test**

Add to `src/parsers/jpeg/app_parsers.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_app0_extended() {
        // JFIF APP0 with extended fields
        let data = b"JFIF\x00\x01\x02\x00\x00\x48\x00\x48";
        let mut metadata = MetadataMap::new();
        parse_app0_extended(data, &mut metadata).unwrap();

        assert!(metadata.contains_key("JFIF:Version"));
        assert!(metadata.contains_key("JFIF:ResolutionUnit"));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test parse_app0_extended`
Expected: FAIL with "cannot find function"

**Step 3: Implement APP0 parser**

Add to `src/parsers/jpeg/app_parsers.rs`:

```rust
/// Parse JFIF APP0 segment with extended fields
pub fn parse_app0_extended(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.len() < 14 {
        return Err("APP0 segment too short".to_string());
    }

    // Check JFIF identifier
    if &data[0..5] != b"JFIF\x00" {
        // Check for JFXX extension
        if &data[0..5] == b"JFXX\x00" {
            return parse_jfxx_segment(&data[5..], metadata);
        }
        return Err("Not a JFIF segment".to_string());
    }

    // Version
    let major = data[5];
    let minor = data[6];
    metadata.insert(
        "JFIF:JFIFVersion".to_string(),
        TagValue::String(format!("{}.{:02}", major, minor)),
    );

    // Resolution unit (0=none, 1=dpi, 2=dpcm)
    let unit = data[7];
    let unit_str = match unit {
        0 => "None",
        1 => "inches",
        2 => "cm",
        _ => "Unknown",
    };
    metadata.insert(
        "JFIF:ResolutionUnit".to_string(),
        TagValue::String(unit_str.to_string()),
    );

    // X/Y resolution
    let reader = EndianReader::big_endian(data);
    if let Some(x_res) = reader.u16_at(8) {
        metadata.insert("JFIF:XResolution".to_string(), TagValue::Integer(x_res as i64));
    }
    if let Some(y_res) = reader.u16_at(10) {
        metadata.insert("JFIF:YResolution".to_string(), TagValue::Integer(y_res as i64));
    }

    Ok(())
}

/// Parse JFXX (JFIF extension) segment
fn parse_jfxx_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.is_empty() {
        return Err("JFXX segment empty".to_string());
    }

    let extension_code = data[0];
    let ext_type = match extension_code {
        0x10 => "Thumbnail JPEG",
        0x11 => "Thumbnail 1 byte/pixel",
        0x13 => "Thumbnail 3 bytes/pixel",
        _ => "Unknown",
    };

    metadata.insert(
        "JFIF:ThumbnailType".to_string(),
        TagValue::String(ext_type.to_string()),
    );

    Ok(())
}
```

**Step 4: Run tests**

Run: `cargo test parse_app0`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/jpeg/app_parsers.rs
git commit -m "feat(jpeg): add APP0/JFIF extended parsing"
```

### Task 2.2: Add FLIR APP1 Parser

**Files:**
- Create: `src/parsers/jpeg/flir_parser.rs`
- Modify: `src/parsers/jpeg/mod.rs`

**Step 1: Write the failing test**

Create `src/parsers/jpeg/flir_parser.rs`:

```rust
//! FLIR thermal imaging APP1 parser
//!
//! FLIR cameras embed thermal data in APP1 segments with "FLIR\x00" identifier.

use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;

/// Parse FLIR APP1 segment
pub fn parse_flir_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    // FLIR segments start with "FLIR\x00"
    if data.len() < 8 || &data[0..5] != b"FLIR\x00" {
        return Err("Not a FLIR segment".to_string());
    }

    let reader = EndianReader::little_endian(&data[8..]);

    // Parse FLIR FFF record structure
    // Record type at offset 0
    // Record length at offset 2

    // Camera model from segment
    if data.len() > 32 {
        if let Ok(model) = std::str::from_utf8(&data[16..32]) {
            let model = model.trim_end_matches('\x00');
            if !model.is_empty() {
                metadata.insert(
                    "APP1:CameraModel".to_string(),
                    TagValue::String(model.to_string()),
                );
            }
        }
    }

    // TODO: Parse full FLIR FFF structure for:
    // - AtmosphericTemperature
    // - Emissivity
    // - ObjectDistance
    // - RelativeHumidity
    // - RawThermalImage dimensions

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flir_identification() {
        let data = b"FLIR\x00\x01\x02\x03";
        let mut metadata = MetadataMap::new();
        // Should not error on valid FLIR prefix
        let result = parse_flir_segment(data, &mut metadata);
        assert!(result.is_ok());
    }

    #[test]
    fn test_non_flir_rejected() {
        let data = b"EXIF\x00\x00";
        let mut metadata = MetadataMap::new();
        let result = parse_flir_segment(data, &mut metadata);
        assert!(result.is_err());
    }
}
```

**Step 2: Update mod.rs**

Add to `src/parsers/jpeg/mod.rs`:

```rust
pub mod flir_parser;
```

**Step 3: Run tests**

Run: `cargo test flir`
Expected: PASS

**Step 4: Commit**

```bash
git add src/parsers/jpeg/flir_parser.rs src/parsers/jpeg/mod.rs
git commit -m "feat(jpeg): add FLIR thermal APP1 parser stub"
```

### Task 2.3: Add JPEG-HDR APP11 Parser

**Files:**
- Create: `src/parsers/jpeg/jpeg_hdr_parser.rs`

Similar pattern to Task 2.2. Parse JPEG-HDR APP11 segments with "JPEG-HDR" identifier.

### Task 2.4: Wire APP Parsers into Segment Parser

**Files:**
- Modify: `src/parsers/jpeg/segment_parser.rs`

Add dispatch logic to call new parsers based on APP segment markers:

```rust
// In parse_segments or similar function
match segment.marker {
    0xE0 => { // APP0
        let _ = app_parsers::parse_app0_extended(&segment.data, metadata);
    }
    0xE1 => { // APP1
        // Check for FLIR vs EXIF
        if segment.data.starts_with(b"FLIR\x00") {
            let _ = flir_parser::parse_flir_segment(&segment.data, metadata);
        } else if segment.data.starts_with(b"Exif\x00\x00") {
            let _ = exif_parser::parse_exif_segment(&segment.data, metadata);
        }
    }
    0xEB => { // APP11
        if segment.data.starts_with(b"JPEG-HDR") {
            let _ = jpeg_hdr_parser::parse_jpeg_hdr(&segment.data, metadata);
        }
    }
    // ... more segments
}
```

---

## Stream 3: QuickTime/MP4 Metadata Extraction

**Goal:** Achieve >50% coverage for MP4/MOV files by extracting QuickTime atom metadata.

**Current State:** 0% coverage - atoms are parsed but not converted to ExifTool-compatible tags.

### Task 3.1: Map QuickTime Atoms to ExifTool Tags

**Files:**
- Create: `src/parsers/quicktime/tag_mapping.rs`

**Step 1: Write the failing test**

```rust
//! QuickTime atom to ExifTool tag mapping

use std::collections::HashMap;
use std::sync::LazyLock;

/// Mapping from QuickTime FourCC codes to ExifTool tag names
static ATOM_TO_TAG: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // User data atoms
    m.insert("©nam", "QuickTime:Title");
    m.insert("©ART", "QuickTime:Artist");
    m.insert("©alb", "QuickTime:Album");
    m.insert("©day", "QuickTime:ContentCreateDate");
    m.insert("©cmt", "QuickTime:Comment");
    m.insert("©gen", "QuickTime:Genre");
    m.insert("©wrt", "QuickTime:Composer");
    m.insert("©too", "QuickTime:Encoder");
    m.insert("©dir", "QuickTime:Director");
    m.insert("©prd", "QuickTime:Producer");
    m.insert("©lyr", "QuickTime:Lyrics");
    m.insert("©grp", "QuickTime:Grouping");
    m.insert("aART", "QuickTime:AlbumArtist");
    m.insert("tmpo", "QuickTime:BeatsPerMinute");
    m.insert("cprt", "QuickTime:Copyright");
    m.insert("desc", "QuickTime:Description");
    m.insert("ldes", "QuickTime:LongDescription");
    m.insert("trkn", "QuickTime:TrackNumber");
    m.insert("disk", "QuickTime:DiskNumber");
    m.insert("covr", "QuickTime:CoverArt");
    // Movie header atoms
    m.insert("mvhd_timescale", "QuickTime:TimeScale");
    m.insert("mvhd_duration", "QuickTime:Duration");
    m.insert("mvhd_rate", "QuickTime:PreferredRate");
    m.insert("mvhd_volume", "QuickTime:PreferredVolume");
    m.insert("mvhd_create", "QuickTime:CreateDate");
    m.insert("mvhd_modify", "QuickTime:ModifyDate");
    // Track header atoms
    m.insert("tkhd_create", "QuickTime:TrackCreateDate");
    m.insert("tkhd_modify", "QuickTime:TrackModifyDate");
    m.insert("tkhd_duration", "QuickTime:TrackDuration");
    m.insert("tkhd_layer", "QuickTime:TrackLayer");
    m.insert("tkhd_volume", "QuickTime:TrackVolume");
    m.insert("tkhd_id", "QuickTime:TrackID");
    // Media header atoms
    m.insert("mdhd_timescale", "QuickTime:MediaTimeScale");
    m.insert("mdhd_duration", "QuickTime:MediaDuration");
    m.insert("mdhd_create", "QuickTime:MediaCreateDate");
    m.insert("mdhd_modify", "QuickTime:MediaModifyDate");
    // Handler atoms
    m.insert("hdlr_type", "QuickTime:HandlerType");
    m.insert("hdlr_name", "QuickTime:HandlerDescription");
    m.insert("hdlr_vendor", "QuickTime:HandlerVendorID");
    // Sample description atoms (video)
    m.insert("stsd_codec", "QuickTime:CompressorID");
    m.insert("stsd_name", "QuickTime:CompressorName");
    m.insert("stsd_width", "QuickTime:ImageWidth");
    m.insert("stsd_height", "QuickTime:ImageHeight");
    m.insert("stsd_depth", "QuickTime:BitDepth");
    m.insert("stsd_xres", "QuickTime:XResolution");
    m.insert("stsd_yres", "QuickTime:YResolution");
    // Sample description atoms (audio)
    m.insert("stsd_channels", "QuickTime:AudioChannels");
    m.insert("stsd_samplerate", "QuickTime:AudioSampleRate");
    m.insert("stsd_bitspersample", "QuickTime:AudioBitsPerSample");
    m.insert("stsd_audioformat", "QuickTime:AudioFormat");
    m
});

/// Get ExifTool-compatible tag name for a QuickTime atom
pub fn atom_to_exiftool_tag(atom_type: &str) -> Option<&'static str> {
    ATOM_TO_TAG.get(atom_type).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_mapping() {
        assert_eq!(atom_to_exiftool_tag("©nam"), Some("QuickTime:Title"));
    }

    #[test]
    fn test_unknown_atom() {
        assert_eq!(atom_to_exiftool_tag("xxxx"), None);
    }
}
```

**Step 2: Run tests**

Run: `cargo test atom_to_exiftool`
Expected: PASS

**Step 3: Commit**

```bash
git add src/parsers/quicktime/tag_mapping.rs
git commit -m "feat(quicktime): add atom to ExifTool tag mapping"
```

### Task 3.2: Extract Movie Header (mvhd) Metadata

**Files:**
- Modify: `src/parsers/quicktime/metadata_extractor.rs`

**Step 1: Enhance mvhd extraction**

Add to `metadata_extractor.rs`:

```rust
use super::tag_mapping::atom_to_exiftool_tag;

/// Extract movie header (mvhd) metadata
fn extract_movie_header(mvhd: &Atom, metadata: &mut MetadataMap) {
    let data = &mvhd.data;
    if data.len() < 100 {
        return;
    }

    let version = data[0];
    let reader = EndianReader::big_endian(data);

    // Offsets differ between version 0 and version 1
    let (create_offset, modify_offset, timescale_offset, duration_offset) = if version == 0 {
        (4, 8, 12, 16)
    } else {
        (4, 12, 20, 24)
    };

    // Creation time
    if let Some(create_time) = reader.u32_at(create_offset) {
        if let Some(tag) = atom_to_exiftool_tag("mvhd_create") {
            let iso = mac_time_to_iso8601(create_time as i64);
            metadata.insert(tag.to_string(), TagValue::String(iso));
        }
    }

    // Modification time
    if let Some(modify_time) = reader.u32_at(modify_offset) {
        if let Some(tag) = atom_to_exiftool_tag("mvhd_modify") {
            let iso = mac_time_to_iso8601(modify_time as i64);
            metadata.insert(tag.to_string(), TagValue::String(iso));
        }
    }

    // Time scale
    if let Some(timescale) = reader.u32_at(timescale_offset) {
        if let Some(tag) = atom_to_exiftool_tag("mvhd_timescale") {
            metadata.insert(tag.to_string(), TagValue::Integer(timescale as i64));
        }
    }

    // Duration
    if let Some(duration) = reader.u32_at(duration_offset) {
        if let Some(timescale) = reader.u32_at(timescale_offset) {
            if timescale > 0 {
                let duration_secs = duration as f64 / timescale as f64;
                if let Some(tag) = atom_to_exiftool_tag("mvhd_duration") {
                    metadata.insert(
                        tag.to_string(),
                        TagValue::String(format!("{:.2} s", duration_secs)),
                    );
                }
            }
        }
    }

    // Preferred rate (fixed point 16.16)
    if let Some(rate) = reader.u32_at(if version == 0 { 20 } else { 32 }) {
        let rate_float = (rate >> 16) as f64 + (rate & 0xFFFF) as f64 / 65536.0;
        if let Some(tag) = atom_to_exiftool_tag("mvhd_rate") {
            metadata.insert(tag.to_string(), TagValue::Float(rate_float));
        }
    }

    // Preferred volume (fixed point 8.8)
    if let Some(volume) = reader.u16_at(if version == 0 { 24 } else { 36 }) {
        let volume_pct = (volume >> 8) as f64 + (volume & 0xFF) as f64 / 256.0;
        if let Some(tag) = atom_to_exiftool_tag("mvhd_volume") {
            metadata.insert(
                tag.to_string(),
                TagValue::String(format!("{:.2}%", volume_pct * 100.0)),
            );
        }
    }
}
```

**Step 2: Run comparison**

Run: `cargo run --bin tag-comparison -- --format mp4`
Expected: Coverage should increase from 0%

**Step 3: Commit**

```bash
git add src/parsers/quicktime/metadata_extractor.rs
git commit -m "feat(quicktime): extract mvhd movie header metadata"
```

### Task 3.3: Extract User Data (udta) Atoms

**Files:**
- Modify: `src/parsers/quicktime/metadata_extractor.rs`

Extract classic QuickTime user data atoms (©nam, ©ART, etc.) using the tag mapping.

### Task 3.4: Extract Track Header (tkhd) Metadata

**Files:**
- Modify: `src/parsers/quicktime/metadata_extractor.rs`

Extract track-specific metadata using the pattern from Task 3.2.

### Task 3.5: Extract Sample Description (stsd) Metadata

**Files:**
- Modify: `src/parsers/quicktime/metadata_extractor.rs`

Extract video codec, dimensions, audio format, sample rate, etc.

---

## Stream 4: RAW Format MakerNotes

**Goal:** Ensure MakerNotes are extracted and output for CR2, NEF, DNG, RAF, RW2.

### Task 4.1: Audit MakerNote Extraction Flow

**Files:**
- Read: `src/parsers/tiff/makernote_parser.rs`
- Read: `src/parsers/raw/mod.rs`

Trace why MakerNotes are parsed but not appearing in comparison output.

### Task 4.2: Fix Canon MakerNote Output (CR2)

**Files:**
- Modify: `src/parsers/tiff/makernotes/canon.rs`

Ensure Canon MakerNote tags use ExifTool-compatible family prefix `Canon:`.

### Task 4.3: Fix Nikon MakerNote Output (NEF)

**Files:**
- Modify: `src/parsers/tiff/makernotes/nikon.rs`

Same pattern as Task 4.2 for Nikon family prefix.

### Task 4.4: Fix DNG MakerNote Passthrough

**Files:**
- Modify: `src/parsers/raw/mod.rs`

DNG files may contain embedded original MakerNotes. Ensure passthrough.

### Task 4.5: Fix Fujifilm MakerNote Output (RAF)

**Files:**
- Modify: `src/parsers/tiff/makernotes/fujifilm.rs`

### Task 4.6: Fix Panasonic MakerNote Output (RW2)

**Files:**
- Modify: `src/parsers/tiff/makernotes/panasonic.rs`

---

## Stream 5: Value Formatting

**Goal:** Match ExifTool's value formatting conventions for dates, sizes, rationals.

### Task 5.1: Create Value Formatter Module

**Files:**
- Create: `src/core/value_formatter.rs`

```rust
//! Value formatting to match ExifTool conventions

/// Format file size like ExifTool (e.g., "2.1 kB" not "2 kB")
pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1000 {
        format!("{} bytes", bytes)
    } else if bytes < 1_000_000 {
        format!("{:.1} kB", bytes as f64 / 1000.0)
    } else if bytes < 1_000_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    }
}

/// Format EXIF date string (YYYY:MM:DD HH:MM:SS)
pub fn format_exif_datetime(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y:%m:%d %H:%M:%S").to_string()
}

/// Format IPTC date (YYYYMMDD -> YYYY:MM:DD)
pub fn format_iptc_date(raw: &str) -> String {
    if raw.len() == 8 {
        format!("{}:{}:{}", &raw[0..4], &raw[4..6], &raw[6..8])
    } else {
        raw.to_string()
    }
}

/// Format IPTC time (HHMMSS±HHMM -> HH:MM:SS±HH:MM)
pub fn format_iptc_time(raw: &str) -> String {
    if raw.len() >= 6 {
        let base = format!("{}:{}:{}", &raw[0..2], &raw[2..4], &raw[4..6]);
        if raw.len() >= 11 {
            format!("{}{}:{}:{}", base, &raw[6..7], &raw[7..9], &raw[9..11])
        } else {
            base
        }
    } else {
        raw.to_string()
    }
}

/// Format rational as ExifTool does (decode common values)
pub fn format_rational(num: i32, denom: i32, tag_name: &str) -> String {
    if denom == 0 {
        return "undef".to_string();
    }

    // Some tags have special formatting
    match tag_name {
        "ExposureTime" => {
            if num == 1 {
                format!("1/{}", denom)
            } else {
                let val = num as f64 / denom as f64;
                if val >= 1.0 {
                    format!("{:.1}", val)
                } else {
                    format!("1/{:.0}", 1.0 / val)
                }
            }
        }
        "FNumber" => format!("{:.1}", num as f64 / denom as f64),
        _ => format!("{}/{}", num, denom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_size_formatting() {
        assert_eq!(format_file_size(2100), "2.1 kB");
        assert_eq!(format_file_size(500), "500 bytes");
        assert_eq!(format_file_size(1_500_000), "1.5 MB");
    }

    #[test]
    fn test_iptc_date() {
        assert_eq!(format_iptc_date("20020620"), "2002:06:20");
    }

    #[test]
    fn test_iptc_time() {
        assert_eq!(format_iptc_time("021111+0100"), "02:11:11+01:00");
    }
}
```

**Step 2: Commit**

```bash
git add src/core/value_formatter.rs
git commit -m "feat: add value formatter for ExifTool compatibility"
```

### Task 5.2: Apply Formatting to IPTC Parser

**Files:**
- Modify: `src/parsers/jpeg/iptc_parser.rs`

Use `format_iptc_date()` and `format_iptc_time()` when extracting IPTC dates.

### Task 5.3: Apply Formatting to File Metadata

**Files:**
- Modify: `src/core/file_metadata.rs` (or equivalent)

Use `format_file_size()` for FileSize tag.

---

## Stream 6: XMP Namespace Normalization

**Goal:** Align XMP tag prefixes with ExifTool conventions.

### Task 6.1: Create XMP Namespace Mapping

**Files:**
- Create: `src/parsers/xmp/namespace_mapping.rs`

```rust
//! XMP namespace to ExifTool family mapping

use std::collections::HashMap;
use std::sync::LazyLock;

/// Map XMP namespace URIs to ExifTool family prefixes
static NAMESPACE_TO_FAMILY: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("http://purl.org/dc/elements/1.1/", "XMP-dc");
    m.insert("http://ns.adobe.com/xap/1.0/", "XMP-xmp");
    m.insert("http://ns.adobe.com/xap/1.0/mm/", "XMP-xmpMM");
    m.insert("http://ns.adobe.com/xap/1.0/rights/", "XMP-xmpRights");
    m.insert("http://ns.adobe.com/exif/1.0/", "XMP-exif");
    m.insert("http://ns.adobe.com/tiff/1.0/", "XMP-tiff");
    m.insert("http://ns.adobe.com/photoshop/1.0/", "XMP-photoshop");
    m.insert("http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/", "XMP-iptcCore");
    m
});

/// Get ExifTool family prefix for XMP namespace
pub fn namespace_to_family(namespace_uri: &str) -> Option<&'static str> {
    NAMESPACE_TO_FAMILY.get(namespace_uri).copied()
}
```

### Task 6.2: Apply Namespace Mapping in XMP Parser

**Files:**
- Modify: `src/parsers/xmp/rdf_parser.rs`

When outputting XMP tags, use the namespace mapping to generate correct family prefixes.

---

## Verification

After implementing all streams, run full comparison:

```bash
cargo run --bin tag-comparison -- --all-formats
```

**Success Criteria:**
- Overall coverage: >50% (up from 5.1%)
- JPEG coverage: >30% (up from 2.2%)
- MP4 coverage: >40% (up from 0%)
- RAW formats: >20% (up from 1.6-4.5%)
- No regressions (previously matched tags still match)

---

## Parallel Execution Guide

**For maximum parallelism, dispatch subagents as follows:**

| Subagent | Stream | Dependencies |
|----------|--------|--------------|
| Agent 1 | Stream 1 (Tag Normalization) | None |
| Agent 2 | Stream 2 (JPEG APP) | None |
| Agent 3 | Stream 3 (QuickTime) | None |
| Agent 4 | Stream 4 (RAW MakerNotes) | None |
| Agent 5 | Stream 5 (Value Formatting) | None |
| Agent 6 | Stream 6 (XMP) | None |

All streams are independent and can be worked in parallel. Final integration test runs after all streams complete.
