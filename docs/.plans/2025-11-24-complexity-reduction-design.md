# Complexity Reduction Plan - Duplication & File Size

> **Created:** 2025-11-24
> **Status:** Ready for Implementation
> **Goal:** Reduce code duplication from 46% to ≤10% and break up all files >1,000 lines

---

## Executive Summary

Fresh Codacy analysis reveals two major issues:
1. **46% code duplication** (target: ≤10%)
2. **20% complex files** (target: ≤10%)

This plan attacks both problems in parallel through 4 phases over ~7-9 days.

---

## Current State Analysis

### Codacy Metrics (2025-11-24)

| Metric | Current | Target | Gap |
|--------|---------|--------|-----|
| Grade | A (99) | A | ✅ Met |
| Complex Files | 20% (127) | ≤10% | ❌ 10% over |
| Duplication | 46% | ≤10% | ❌ 36% over |
| Coverage | 41.9% | ≥60% | ❌ 18% under |

### Worst Offenders - Duplication

| File | Grade | Duplication | Clones |
|------|-------|-------------|--------|
| `core/tag_conversion.rs` | **E** | 241% | 19 |
| `makernotes/pentax.rs` | D | 414% | 24 |
| `makernotes/google.rs` | D | 373% | 19 |
| `makernotes/array_extractors.rs` | D | 277% | 16 |
| `makernotes/apple.rs` | C | 263% | 10 |
| `makernotes/panasonic.rs` | C | 245% | 10 |

### Worst Offenders - File Size

| File | Lines | Complexity | Notes |
|------|-------|------------|-------|
| `tag_db/tag_registry.rs` | 7,494 | - | Central tag DB (consider generated) |
| `parsers/icc_parser.rs` | 1,508 | 98 | ICC color profile parsing |
| `makernotes/canon.rs` | 1,352 | 116 | Canon MakerNotes |
| `makernotes/shared/tag_registry.rs` | 1,060 | - | MakerNotes registry |
| `parsers/format_detector.rs` | 1,053 | - | Magic byte detection |
| `ffi/c_api.rs` | 1,012 | 109 | C FFI bindings |
| `parsers/png/mod.rs` | 1,006 | 99 | PNG parser |

---

## Solution Design

### Two Parallel Tracks

**Track A: Duplication Elimination**
- Fix Grade E/D files first (quick Codacy wins)
- Build ArraySchema infrastructure for root cause fix
- Migrate high-duplication MakerNotes parsers

**Track B: File Decomposition**
- Split all 1,000+ line files into focused modules
- Target: No file >800 lines (excluding generated)
- Maintain public API compatibility

---

## Phase 1: Quick Wins (1-2 days)

**Goal:** Eliminate worst duplication offenders, visible Codacy improvement

### 1.1 Fix `tag_conversion.rs` (Grade E → B)

**Location:** `src/core/tag_conversion.rs`
**Current:** 310 lines, 241% duplication, 19 clones, Grade E

**Problem:** Repetitive tag conversion patterns like:
```rust
// Pattern repeated 19+ times with minor variations
match tag_type {
    TagType::String => value.to_string(),
    TagType::Int => format!("{}", value.parse::<i32>().unwrap_or(0)),
    // ... similar branches
}
```

**Solution:**
```rust
// Consolidate into parameterized helper
fn convert_value<T: FromStr + Display>(value: &str, default: T) -> String {
    value.parse::<T>().unwrap_or(default).to_string()
}

// Use in match arms
match tag_type {
    TagType::String => value.to_string(),
    TagType::Int => convert_value::<i32>(value, 0),
    TagType::Float => convert_value::<f64>(value, 0.0),
    // ...
}
```

**Target:** 310 → ~180 lines, -19 clones, Grade E → B

### 1.2 Fix `array_extractors.rs` (Grade D → B)

**Location:** `src/parsers/tiff/makernotes/shared/array_extractors.rs`
**Current:** 303 lines, 277% duplication, 16 clones, Grade D

**Problem:** Near-identical functions for different types:
```rust
pub fn extract_i16_array(...) -> Option<Vec<i16>> { /* logic */ }
pub fn extract_u16_array(...) -> Option<Vec<u16>> { /* logic */ }
pub fn extract_i32_array(...) -> Option<Vec<i32>> { /* logic */ }
// ... 8+ more variants
```

**Solution:** Generic extraction function:
```rust
pub fn extract_array<T>(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<T>>
where
    T: FromBytes + Copy,
{
    // Single implementation for all numeric types
}

// Type aliases for convenience
pub fn extract_i16_array(e: &IfdEntry, d: &[u8], bo: ByteOrder) -> Option<Vec<i16>> {
    extract_array::<i16>(e, d, bo)
}
```

**Target:** 303 → ~120 lines, -13 clones, Grade D → B

### 1.3 Fix `operations_helpers.rs` (Grade D → C)

**Location:** `src/core/operations_helpers.rs`
**Current:** 227 lines, 151% duplication, 10 clones, Grade D

**Problem:** Duplicated JPEG/TIFF segment handling patterns

**Solution:** Extract shared `process_segment()` helper with format-specific callbacks

**Target:** 227 → ~150 lines, -7 clones, Grade D → C

### Phase 1 Success Criteria
- [ ] `tag_conversion.rs` Grade E → B
- [ ] `array_extractors.rs` Grade D → B
- [ ] `operations_helpers.rs` Grade D → C
- [ ] All 1,169 tests passing
- [ ] Codacy duplication: 46% → ~38%

---

## Phase 2: File Decomposition (2-3 days)

**Goal:** Break up all 1,000+ line files into focused modules

### 2.1 ICC Parser Split

**Current:** `src/parsers/icc_parser.rs` (1,508 lines, complexity 98, Grade C)

**New Structure:**
```
src/parsers/icc/
├── mod.rs              (~100 lines)
│   - Public API: parse_icc_profile()
│   - Module orchestration
│   - Re-exports
│
├── header.rs           (~150 lines)
│   - IccHeader struct
│   - parse_header()
│   - Header validation
│
├── profile_tags.rs     (~300 lines)
│   - Profile description (desc)
│   - Copyright (cprt)
│   - Device attributes
│
├── color_tags.rs       (~400 lines)
│   - Color space transforms
│   - TRC curves (rTRC, gTRC, bTRC)
│   - Matrix tags (chad, etc.)
│
└── rendering_tags.rs   (~200 lines)
    - Rendering intent
    - Gamut tags
    - Viewing conditions
```

**Migration Steps:**
1. Create `src/parsers/icc/` directory
2. Extract header parsing to `header.rs`
3. Extract tag categories to respective modules
4. Update `mod.rs` to orchestrate and re-export
5. Update imports in dependent files
6. Delete old `icc_parser.rs`

### 2.2 Format Detector Split

**Current:** `src/parsers/format_detector.rs` (1,053 lines)

**New Structure:**
```
src/parsers/detection/
├── mod.rs              (~150 lines)
│   - detect_format() main function
│   - FormatSignature struct
│   - MagicPattern enum
│
├── signatures/
│   ├── mod.rs          (~50 lines)
│   │   - ALL_SIGNATURES aggregation
│   │
│   ├── image.rs        (~250 lines)
│   │   - JPEG, PNG, TIFF, GIF, WebP, BMP
│   │   - RAW formats (CR2, NEF, ARW, etc.)
│   │
│   ├── video.rs        (~200 lines)
│   │   - MP4, MOV, MKV, AVI, MTS
│   │
│   ├── document.rs     (~150 lines)
│   │   - PDF, PE (Windows executables)
│   │
│   └── archive.rs      (~100 lines)
       - ZIP, DOCX, XLSX (ZIP-based)
```

**Key Design:**
```rust
// Declarative signature definition
pub struct FormatSignature {
    pub name: &'static str,
    pub mime_type: &'static str,
    pub extensions: &'static [&'static str],
    pub magic: &'static [MagicPattern],
}

pub enum MagicPattern {
    Exact { offset: usize, bytes: &'static [u8] },
    AnyOf { offset: usize, options: &'static [&'static [u8]] },
}

// Generic matching
impl FormatSignature {
    pub fn matches(&self, data: &[u8]) -> bool {
        self.magic.iter().all(|p| p.matches(data))
    }
}
```

### 2.3 PNG Parser Split

**Current:** `src/parsers/png/mod.rs` (1,006 lines, complexity 99, Grade C)

**New Structure:**
```
src/parsers/png/
├── mod.rs              (~200 lines)
│   - parse_png() main entry
│   - PngMetadata struct
│   - Chunk iteration
│
├── chunks/
│   ├── mod.rs          (~100 lines)
│   │   - ChunkType enum
│   │   - parse_chunk() dispatch
│   │
│   ├── critical.rs     (~200 lines)
│   │   - IHDR (image header)
│   │   - PLTE (palette)
│   │   - IDAT (image data) - skip
│   │   - IEND (end marker)
│   │
│   ├── ancillary.rs    (~300 lines)
│   │   - tEXt, iTXt, zTXt (text chunks)
│   │   - tIME (timestamp)
│   │   - pHYs (physical dimensions)
│   │   - gAMA, cHRM, sRGB (color)
│   │
│   └── exif.rs         (~150 lines)
       - eXIf chunk (EXIF in PNG)
       - Integration with TIFF parser
```

### 2.4 C API Split

**Current:** `src/ffi/c_api.rs` (1,012 lines, complexity 109, Grade C)

**New Structure:**
```
src/ffi/
├── mod.rs              (~100 lines)
│   - Re-exports all public C functions
│   - Module documentation
│
├── handle.rs           (~150 lines)
│   - ExifToolHandle struct
│   - exiftool_new(), exiftool_free()
│   - Handle validation
│
├── read_api.rs         (~250 lines)
│   - exiftool_read_file()
│   - exiftool_get_tag()
│   - exiftool_get_all_tags()
│   - Tag iteration
│
├── write_api.rs        (~250 lines)
│   - exiftool_write_file()
│   - exiftool_set_tag()
│   - exiftool_remove_tag()
│   - Batch operations
│
└── utility.rs          (~150 lines)
    - Error handling (exiftool_get_error())
    - String conversion helpers
    - Memory management helpers
```

### Phase 2 Success Criteria
- [ ] `icc_parser.rs` → 5 modules, each <400 lines
- [ ] `format_detector.rs` → 5 modules, each <300 lines
- [ ] `png/mod.rs` → 5 modules, each <350 lines
- [ ] `c_api.rs` → 5 modules, each <300 lines
- [ ] All 1,169 tests passing
- [ ] No new files >800 lines

---

## Phase 3: ArraySchema Infrastructure (2-3 days)

**Goal:** Eliminate root cause of MakerNotes duplication

### 3.1 Create ArraySchema Types

**Location:** `src/parsers/tiff/makernotes/shared/array_schemas.rs`

```rust
/// Definition for a single array index
#[derive(Debug, Clone)]
pub struct ArrayIndexDef {
    /// Index in the source array
    pub index: usize,
    /// Tag name for this value
    pub name: &'static str,
    /// Optional decoder function
    pub decoder: Option<fn(i16) -> String>,
    /// Whether this index is optional
    pub optional: bool,
}

/// Schema defining how to process an array tag
#[derive(Debug, Clone)]
pub struct ArraySchema {
    /// Schema name (e.g., "CameraSettings")
    pub name: &'static str,
    /// Array indices to extract
    pub indices: &'static [ArrayIndexDef],
}

impl ArraySchema {
    /// Process an array using this schema
    pub fn process(
        &self,
        array: &[i16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(&value) = array.get(def.index) {
                let decoded = match def.decoder {
                    Some(decoder) => decoder(value),
                    None => value.to_string(),
                };
                tags.insert(
                    format!("{}:{}:{}", prefix, self.name, def.name),
                    decoded,
                );
            } else if !def.optional {
                // Log missing required index
            }
        }
    }
}
```

### 3.2 Define Manufacturer Schemas

**Example - Pentax CameraSettings:**
```rust
// src/parsers/tiff/makernotes/schemas/pentax.rs

use super::super::shared::generic_decoders::*;
use super::super::shared::array_schemas::{ArraySchema, ArrayIndexDef};

pub static PENTAX_CAMERA_SETTINGS: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        ArrayIndexDef { index: 0, name: "CaptureMode", decoder: Some(decode_capture_mode), optional: false },
        ArrayIndexDef { index: 1, name: "Quality", decoder: Some(QUALITY), optional: false },
        ArrayIndexDef { index: 2, name: "FocusMode", decoder: Some(decode_focus_mode), optional: false },
        ArrayIndexDef { index: 3, name: "AFPointSelected", decoder: None, optional: false },
        // ... 50+ more indices defined declaratively
    ],
};

// Before: 200+ lines of repetitive if-statements
// After: 50 lines of declarative schema
```

### 3.3 Migrate High-Duplication Parsers

**Priority Order:**
1. `pentax.rs` - 414% duplication, 24 clones
2. `google.rs` - 373% duplication, 19 clones
3. `apple.rs` - 263% duplication, 10 clones
4. `panasonic.rs` - 245% duplication, 10 clones
5. `sony.rs` - 165% duplication, 13 clones
6. `fujifilm.rs` - 144% duplication, 7 clones

**Migration Pattern:**
```rust
// Before (pentax.rs - repeated 24 times)
if let Some(settings) = extract_i16_array(entry, data, byte_order) {
    if settings.len() > 0 {
        tags.insert("Pentax:CaptureMode".to_string(), decode_capture_mode(settings[0]));
    }
    if settings.len() > 1 {
        tags.insert("Pentax:Quality".to_string(), QUALITY(settings[1]));
    }
    // ... 50+ more if-statements
}

// After
if let Some(settings) = extract_i16_array(entry, data, byte_order) {
    PENTAX_CAMERA_SETTINGS.process(&settings, "Pentax", tags);
}
```

### Phase 3 Success Criteria
- [ ] ArraySchema infrastructure implemented and tested
- [ ] `pentax.rs` migrated: 991 → ~400 lines, 24 clones → ~5
- [ ] `google.rs` migrated: 566 → ~250 lines, 19 clones → ~3
- [ ] `apple.rs` migrated: 381 → ~200 lines
- [ ] `panasonic.rs` migrated: 695 → ~350 lines
- [ ] All 1,169 tests passing
- [ ] Codacy duplication: ~35% → ~20%

---

## Phase 4: Validation & Polish (1 day)

### 4.1 Verification Tasks
- [ ] Run full test suite: `cargo test`
- [ ] Run clippy: `cargo clippy -p oxidex --lib`
- [ ] Run formatter: `cargo fmt`
- [ ] Verify build: `cargo build --release`

### 4.2 Codacy Verification
- [ ] Push changes to trigger Codacy analysis
- [ ] Verify duplication ≤25%
- [ ] Verify complex files ≤15%
- [ ] Verify no D/E grade files remain

### 4.3 Documentation Updates
- [ ] Update CHANGELOG.md with refactoring summary
- [ ] Update contributor guide with new patterns
- [ ] Archive this plan to `docs/plans/archived/`

---

## Risk Mitigation

### Risk 1: Test Failures During Refactoring
**Mitigation:**
- Run tests after each file change
- Keep original code as reference until tests pass
- Use git branches for each phase

### Risk 2: Breaking Public API
**Mitigation:**
- FFI functions maintain exact same signatures
- Only internal reorganization
- Re-export everything through existing module paths

### Risk 3: Performance Regression
**Mitigation:**
- ArraySchema uses static dispatch (no runtime cost)
- Benchmark parsing speed before/after
- Profile critical paths if needed

### Risk 4: Incomplete Migration
**Mitigation:**
- Prioritize highest-impact files first
- Each phase is independently valuable
- Document any deferred work

---

## Success Metrics Summary

| Metric | Before | After Phase 1 | After Phase 4 |
|--------|--------|---------------|---------------|
| Duplication | 46% | ~38% | ≤20% |
| Complex Files | 20% | ~18% | ≤15% |
| D/E Grade Files | 5 | 2 | 0 |
| Files >1,000 lines | 7 | 7 | 0* |
| Tests Passing | 1,169 | 1,169 | 1,169 |

*Excluding generated files (`tag_db/generated/`)

---

## Appendix: File Inventory

### Files to Modify (Phase 1)
- `src/core/tag_conversion.rs`
- `src/parsers/tiff/makernotes/shared/array_extractors.rs`
- `src/core/operations_helpers.rs`

### Files to Split (Phase 2)
- `src/parsers/icc_parser.rs` → `src/parsers/icc/`
- `src/parsers/format_detector.rs` → `src/parsers/detection/`
- `src/parsers/png/mod.rs` → `src/parsers/png/chunks/`
- `src/ffi/c_api.rs` → `src/ffi/`

### Files to Migrate (Phase 3)
- `src/parsers/tiff/makernotes/pentax.rs`
- `src/parsers/tiff/makernotes/google.rs`
- `src/parsers/tiff/makernotes/apple.rs`
- `src/parsers/tiff/makernotes/panasonic.rs`
- `src/parsers/tiff/makernotes/sony.rs`
- `src/parsers/tiff/makernotes/fujifilm.rs`

---

## References

- Previous complexity plan: `docs/plans/2025-11-19-parser-complexity-reduction.md`
- Sprint 4 results: `docs/metrics/sprint4-refactoring-results.md`
- Sprint 5 results: `docs/metrics/sprint5-phase2-code-quality.md`
- Codacy dashboard: https://app.codacy.com/gh/swack-tools/oxidex/dashboard

---

**Plan Status:** Ready for Implementation
**Estimated Duration:** 7-9 days
**Generated:** 2025-11-24
