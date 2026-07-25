# Sprint 3: Broad MakerNotes Migration - Detailed Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Parent Plan:** docs/plans/2025-11-19-parser-complexity-reduction.md
**Sprint:** 3 of 5
**Duration:** 4 weeks
**Goal:** Migrate remaining 30+ MakerNotes parsers to TagRegistry + ArraySchema infrastructure in 4 batches, achieving 6,000-12,000 line reduction while maintaining full test coverage.

**Prerequisites:**
- Sprint 1 completed (ArraySchema, TagRegistry, LensDatabase infrastructure)
- Sprint 2 completed (5 pilot parsers validated: Canon, Nikon, Sony, Apple, Google)

## Success Criteria
- 30+ parsers migrated to new infrastructure
- 9 additional lens databases unified under LensDatabase trait
- All existing tests continue to pass
- 6,000-12,000 line reduction achieved
- Registry modules created and documented for all manufacturers

---

## Current State Analysis

### Remaining Parsers (37 total)

**Batch 1 - Traditional Cameras (5 parsers, 5,010 lines)**
- Olympus: 1,132 lines (high complexity, array tags, lens DB)
- Panasonic: 1,043 lines (high complexity, array tags, lens DB)
- Pentax: 1,020 lines (high complexity, array tags, lens DB)
- Fujifilm: 903 lines (medium complexity, array tags, lens DB)
- Leica: 912 lines (medium complexity, array tags, lens DB)

**Batch 2 - Smartphones (3 parsers, 1,532 lines)**
- Microsoft: 581 lines (low complexity)
- Samsung: 482 lines (low complexity)
- Qualcomm: 469 lines (low complexity)

**Batch 3 - Specialty Devices (4 parsers, 2,686 lines)**
- DJI: 1,060 lines (high complexity, drone telemetry)
- GoPro: 652 lines (medium complexity)
- FLIR: 627 lines (medium complexity, thermal imaging)
- Lytro: 347 lines (low complexity, light field)

**Batch 4 - Software Applications (3 parsers, 1,793 lines)**
- Photoshop: 616 lines (medium complexity)
- Capture One: 593 lines (medium complexity)
- Nikon Capture: 584 lines (medium complexity)

**Batch 5 - Legacy/Niche Manufacturers (22 parsers, ~8,500 lines)**
- Sigma, Minolta, Ricoh, Casio, Kodak, Phaseone, Leaf, Motorola, HP, JVC, GE, Sanyo, Nintendo, Parrot, Red, Reconyx, Infiray, Gimp, Fotostation, Photomechanic, Scalado, Indesign

### Lens Databases to Migrate (9 remaining)
- Olympus, Panasonic, Pentax, Fujifilm, Leica (Batch 1)
- Minolta, Sigma, Phaseone, Leaf (Batch 5)

### Expected Savings

| Batch | Parsers | Lines Before | Expected After | Reduction | % |
|-------|---------|--------------|----------------|-----------|---|
| Batch 1 | 5 | 5,010 | ~4,100 | ~910 | 18% |
| Batch 2 | 3 | 1,532 | ~1,300 | ~232 | 15% |
| Batch 3 | 4 | 2,686 | ~2,250 | ~436 | 16% |
| Batch 4 | 3 | 1,793 | ~1,500 | ~293 | 16% |
| Batch 5 | 22 | ~8,500 | ~7,200 | ~1,300 | 15% |
| **Total** | **37** | **~19,521** | **~16,350** | **~3,171** | **16%** |

**Note:** Expected reduction is conservative (16%) based on Sprint 2 learnings. Actual reduction may vary based on parser complexity and array usage.

---

## Batch 1: Traditional Camera Manufacturers

### Task 1.1: Migrate Olympus Parser

**Files:**
- Create: `src/parsers/tiff/makernotes/registries/olympus.rs`
- Modify: `src/parsers/tiff/makernotes/olympus.rs`
- Modify: `src/parsers/tiff/makernotes/olympus_lens_database.rs`

#### Step 1: Analyze Olympus parser structure

```bash
grep -n "const_decoder!\|extract_i16_array\|extract_u16_array" src/parsers/tiff/makernotes/olympus.rs | wc -l
```

Review for:
- Array-based tags (CameraSettings, Equipment, etc.)
- Decoder functions
- Lens database usage

#### Step 2: Create Olympus registry

Create `src/parsers/tiff/makernotes/registries/olympus.rs`:

```rust
//! Olympus tag registry with array schemas

use super::super::shared::{
    array_schemas::*, generic_decoders::*, tag_registry::TagRegistry,
};

// Import existing decoders from olympus.rs
use super::super::olympus::{
    // Make decoders public in olympus.rs first
    // QUALITY, FLASH_MODE, FOCUS_MODE, etc.
};

/// CameraSettings array schema
static CAMERA_SETTINGS_SCHEMA: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        // Define based on Olympus CameraSettings structure
        // Olympus typically has 200+ indices
    ],
};

/// Equipment array schema
static EQUIPMENT_SCHEMA: ArraySchema = ArraySchema {
    name: "Equipment",
    indices: &[
        // Camera body, lens, flash equipment info
    ],
};

pub fn olympus_registry() -> TagRegistry {
    TagRegistry::new()
        .register_array_schema(0x2010, &CAMERA_SETTINGS_SCHEMA)
        .register_array_schema(0x2020, &EQUIPMENT_SCHEMA)
        // ... other tags
}
```

#### Step 3: Migrate Olympus lens database

```rust
// olympus_lens_database.rs
use super::shared::{LensDatabase, StaticLensDb};

static OLYMPUS_LENSES: [(u16, &str); N] = [
    // Existing lens data
];

static OLYMPUS_LENS_DB: StaticLensDb = StaticLensDb::new(&OLYMPUS_LENSES);

pub fn get_lens_database() -> &'static impl LensDatabase {
    &OLYMPUS_LENS_DB
}

// Keep backward compatibility
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    OLYMPUS_LENS_DB.lookup(lens_id).map(|s| s.to_string())
}
```

#### Step 4: Refactor Olympus parser

Update olympus.rs to use registry for array processing.

#### Step 5: Test and measure

```bash
cargo test -p oxidex olympus
wc -l src/parsers/tiff/makernotes/olympus.rs
wc -l src/parsers/tiff/makernotes/registries/olympus.rs
```

Expected: 1,132 → ~950 lines (180 line reduction)

#### Step 6: Commit

```bash
git add src/parsers/tiff/makernotes/registries/olympus.rs
git add src/parsers/tiff/makernotes/olympus.rs
git add src/parsers/tiff/makernotes/olympus_lens_database.rs
git commit -m "refactor(parsers): migrate Olympus parser to TagRegistry + ArraySchema

- Migrate lens database to LensDatabase trait
- Reduce parser from 1,132 to ~950 lines (180 line reduction)
- All existing tests pass"
```

---

### Task 1.2: Migrate Panasonic Parser

**Similar structure to Olympus:**
- Create registry with array schemas
- Migrate lens database
- Refactor parser
- Expected: 1,043 → ~870 lines (173 line reduction)

**Commit message:** `"refactor(parsers): migrate Panasonic parser to TagRegistry + ArraySchema"`

---

### Task 1.3: Migrate Pentax Parser

**Similar structure:**
- Create registry with array schemas
- Migrate lens database
- Refactor parser
- Expected: 1,020 → ~850 lines (170 line reduction)

**Commit message:** `"refactor(parsers): migrate Pentax parser to TagRegistry + ArraySchema"`

---

### Task 1.4: Migrate Fujifilm Parser

**Similar structure:**
- Create registry with array schemas
- Migrate lens database
- Refactor parser
- Expected: 903 → ~760 lines (143 line reduction)

**Commit message:** `"refactor(parsers): migrate Fujifilm parser to TagRegistry + ArraySchema"`

---

### Task 1.5: Migrate Leica Parser

**Similar structure:**
- Create registry with array schemas
- Migrate lens database
- Refactor parser
- Expected: 912 → ~770 lines (142 line reduction)

**Commit message:** `"refactor(parsers): migrate Leica parser to TagRegistry + ArraySchema"`

---

### Batch 1 Verification

After completing all Batch 1 tasks:

```bash
# Run all Batch 1 tests
cargo test -p oxidex olympus panasonic pentax fujifilm leica

# Measure total reduction
wc -l src/parsers/tiff/makernotes/{olympus,panasonic,pentax,fujifilm,leica}.rs
wc -l src/parsers/tiff/makernotes/registries/{olympus,panasonic,pentax,fujifilm,leica}.rs

# Expected: ~910 line reduction from parsers
```

**Batch 1 Summary Commit:**

```bash
git add docs/metrics/
git commit -m "docs: update metrics after Batch 1 completion (5 traditional camera parsers)"
```

---

## Batch 2: Smartphone Manufacturers

### Task 2.1: Migrate Microsoft Parser

**Files:**
- Create: `src/parsers/tiff/makernotes/registries/microsoft.rs`
- Modify: `src/parsers/tiff/makernotes/microsoft.rs`

#### Step 1: Create Microsoft registry

```rust
//! Microsoft (Windows Phone/Lumia) tag registry

use super::super::shared::{tag_registry::TagRegistry};

pub fn microsoft_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0001, "MakerNoteVersion")
        // Microsoft has fewer tags than traditional cameras
}
```

#### Step 2: Refactor parser

Simpler than camera manufacturers (no lens DB, fewer arrays).

#### Step 3: Test and measure

```bash
cargo test -p oxidex microsoft
```

Expected: 581 → ~495 lines (86 line reduction)

#### Step 4: Commit

```bash
git commit -m "refactor(parsers): migrate Microsoft parser to TagRegistry

- Reduce parser from 581 to ~495 lines (86 line reduction)
- All existing tests pass"
```

---

### Task 2.2: Migrate Samsung Parser

**Similar low-complexity structure:**
- Create registry
- Refactor parser
- Expected: 482 → ~410 lines (72 line reduction)

**Commit message:** `"refactor(parsers): migrate Samsung parser to TagRegistry"`

---

### Task 2.3: Migrate Qualcomm Parser

**Similar low-complexity structure:**
- Create registry
- Refactor parser
- Expected: 469 → ~395 lines (74 line reduction)

**Commit message:** `"refactor(parsers): migrate Qualcomm parser to TagRegistry"`

---

### Batch 2 Verification

```bash
cargo test -p oxidex microsoft samsung qualcomm

# Expected: ~232 line reduction
```

**Batch 2 Summary Commit:**

```bash
git commit -m "docs: update metrics after Batch 2 completion (3 smartphone parsers)"
```

---

## Batch 3: Specialty Device Manufacturers

### Task 3.1: Migrate DJI Parser

**Files:**
- Create: `src/parsers/tiff/makernotes/registries/dji.rs`
- Modify: `src/parsers/tiff/makernotes/dji.rs`

**Note:** DJI is high complexity (drone telemetry, flight data arrays)

#### Step 1: Analyze DJI array structures

DJI includes:
- Flight telemetry arrays (GPS, altitude, speed)
- Gimbal position arrays
- Camera settings arrays

#### Step 2: Create DJI registry with specialized schemas

```rust
//! DJI drone telemetry tag registry

static FLIGHT_DATA_SCHEMA: ArraySchema = ArraySchema {
    name: "FlightData",
    indices: &[
        // GPS, altitude, speed, heading indices
    ],
};

static GIMBAL_DATA_SCHEMA: ArraySchema = ArraySchema {
    name: "GimbalData",
    indices: &[
        // Gimbal pitch, roll, yaw indices
    ],
};

pub fn dji_registry() -> TagRegistry {
    TagRegistry::new()
        .register_array_schema(0x0001, &FLIGHT_DATA_SCHEMA)
        .register_array_schema(0x0002, &GIMBAL_DATA_SCHEMA)
        // ... camera settings
}
```

#### Step 3: Refactor parser

Expected: 1,060 → ~880 lines (180 line reduction)

#### Step 4: Commit

```bash
git commit -m "refactor(parsers): migrate DJI parser to TagRegistry + ArraySchema

- Migrate drone telemetry arrays to declarative schemas
- Reduce parser from 1,060 to ~880 lines (180 line reduction)
- All existing tests pass"
```

---

### Task 3.2: Migrate GoPro Parser

**Medium complexity (action camera settings):**
- Create registry with video/photo mode arrays
- Expected: 652 → ~540 lines (112 line reduction)

**Commit message:** `"refactor(parsers): migrate GoPro parser to TagRegistry + ArraySchema"`

---

### Task 3.3: Migrate FLIR Parser

**Medium complexity (thermal imaging data):**
- Create registry with thermal calibration arrays
- Expected: 627 → ~520 lines (107 line reduction)

**Commit message:** `"refactor(parsers): migrate FLIR parser to TagRegistry + ArraySchema"`

---

### Task 3.4: Migrate Lytro Parser

**Low complexity (light field camera):**
- Create registry
- Expected: 347 → ~310 lines (37 line reduction)

**Commit message:** `"refactor(parsers): migrate Lytro parser to TagRegistry"`

---

### Batch 3 Verification

```bash
cargo test -p oxidex dji gopro flir lytro

# Expected: ~436 line reduction
```

**Batch 3 Summary Commit:**

```bash
git commit -m "docs: update metrics after Batch 3 completion (4 specialty device parsers)"
```

---

## Batch 4: Software Application Parsers

### Task 4.1: Migrate Photoshop Parser

**Files:**
- Create: `src/parsers/tiff/makernotes/registries/photoshop.rs`
- Modify: `src/parsers/tiff/makernotes/photoshop.rs`

**Note:** Photoshop stores editing metadata, layers, adjustments

#### Step 1: Create Photoshop registry

```rust
//! Adobe Photoshop tag registry

pub fn photoshop_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0001, "PhotoshopVersion")
        .register_raw(0x0002, "ColorMode")
        .register_raw(0x0003, "LayerCount")
        // Photoshop uses more simple tags than arrays
}
```

#### Step 2: Refactor parser

Expected: 616 → ~510 lines (106 line reduction)

#### Step 3: Commit

```bash
git commit -m "refactor(parsers): migrate Photoshop parser to TagRegistry

- Reduce parser from 616 to ~510 lines (106 line reduction)
- All existing tests pass"
```

---

### Task 4.2: Migrate Capture One Parser

**Medium complexity (RAW processing metadata):**
- Create registry
- Expected: 593 → ~495 lines (98 line reduction)

**Commit message:** `"refactor(parsers): migrate Capture One parser to TagRegistry"`

---

### Task 4.3: Migrate Nikon Capture Parser

**Medium complexity (Nikon's RAW editor metadata):**
- Create registry
- Expected: 584 → ~490 lines (94 line reduction)

**Commit message:** `"refactor(parsers): migrate Nikon Capture parser to TagRegistry"`

---

### Batch 4 Verification

```bash
cargo test -p oxidex photoshop captureone nikoncapture

# Expected: ~298 line reduction
```

**Batch 4 Summary Commit:**

```bash
git commit -m "docs: update metrics after Batch 4 completion (3 software application parsers)"
```

---

## Batch 5: Legacy and Niche Manufacturers

**22 parsers in this batch** - Process 5-6 at a time to manage scope.

### Sub-Batch 5.1: Traditional Camera Manufacturers (5 parsers)

**Parsers:** Sigma, Minolta, Ricoh, Casio, Kodak

**Approach:** Same as Batch 1
- Create registries with array schemas
- Migrate lens databases (Sigma, Minolta have lens DBs)
- Refactor parsers

**Commands:**
```bash
# For each parser:
cargo test -p oxidex <manufacturer>
git commit -m "refactor(parsers): migrate <Manufacturer> parser to TagRegistry + ArraySchema"
```

Expected combined reduction: ~300 lines

---

### Sub-Batch 5.2: Professional/Medium Format (4 parsers)

**Parsers:** Phaseone, Leaf, Red, Parrot

**Approach:**
- Phaseone and Leaf have lens databases to migrate
- Red (cinema cameras) may have specialized arrays
- Parrot (drones) similar to DJI but simpler

Expected combined reduction: ~280 lines

---

### Sub-Batch 5.3: Consumer/Legacy (7 parsers)

**Parsers:** Motorola, HP, JVC, GE, Sanyo, Nintendo, Infiray

**Approach:** Simple registries, mostly individual tags

Expected combined reduction: ~200 lines

---

### Sub-Batch 5.4: Software Tools (6 parsers)

**Parsers:** Gimp, Fotostation, Photomechanic, Scalado, Indesign, Reconyx

**Approach:** Simple registries for editing/workflow metadata

Expected combined reduction: ~180 lines

---

### Batch 5 Verification

After all sub-batches:

```bash
# Run all Batch 5 tests
cargo test -p oxidex sigma minolta ricoh casio kodak phaseone leaf red parrot motorola hp jvc ge sanyo nintendo infiray gimp fotostation photomechanic scalado indesign reconyx

# Expected: ~960 line reduction total
```

**Batch 5 Summary Commit:**

```bash
git commit -m "docs: update metrics after Batch 5 completion (22 legacy/niche parsers)"
```

---

## Sprint 3 Final Verification

After all 5 batches complete:

### Step 1: Run full test suite

```bash
cargo test -p oxidex --lib
```

Expected: All 1,081+ tests pass

### Step 2: Run clippy

```bash
cargo clippy --package oxidex -- -D warnings
```

Expected: No warnings

### Step 3: Measure total reduction

```bash
# Count all parser lines
find src/parsers/tiff/makernotes -name "*.rs" -not -path "*/shared/*" -not -path "*/registries/*" -exec wc -l {} + | tail -1

# Count all registry lines
find src/parsers/tiff/makernotes/registries -name "*.rs" -exec wc -l {} + | tail -1

# Calculate net reduction
```

Expected:
- Parsers: ~16,350 lines (down from ~19,521)
- Registries: ~3,500 lines (new)
- Net reduction: ~3,171 lines (16%)

### Step 4: Update comprehensive metrics

Create `docs/metrics/sprint3-migration-results.md`:

```markdown
# Sprint 3 Migration Results

## Summary

Successfully migrated 37 additional MakerNotes parsers to TagRegistry + ArraySchema infrastructure across 5 batches.

## Metrics by Batch

| Batch | Parsers | Before | After | Reduction | % |
|-------|---------|--------|-------|-----------|---|
| Batch 1 | 5 traditional cameras | 5,010 | 4,100 | 910 | 18% |
| Batch 2 | 3 smartphones | 1,532 | 1,300 | 232 | 15% |
| Batch 3 | 4 specialty devices | 2,686 | 2,250 | 436 | 16% |
| Batch 4 | 3 software apps | 1,793 | 1,500 | 293 | 16% |
| Batch 5 | 22 legacy/niche | 8,500 | 7,200 | 1,300 | 15% |
| **Total** | **37** | **19,521** | **16,350** | **3,171** | **16%** |

## Lens Databases Migrated

9 additional manufacturers now use unified LensDatabase trait:
- Olympus, Panasonic, Pentax, Fujifilm, Leica (Batch 1)
- Minolta, Sigma, Phaseone, Leaf (Batch 5)

**Total lens databases unified:** 12 of 12 (100%)

## Cumulative Progress (Sprints 1-3)

| Metric | Sprint 1 | Sprint 2 | Sprint 3 | Total |
|--------|----------|----------|----------|-------|
| Infrastructure | 3 modules | - | - | 3 modules |
| Parsers migrated | 0 | 5 | 37 | 42 |
| Lens DBs unified | 0 | 3 | 9 | 12 |
| Line reduction | 0 | ~300 | ~3,171 | ~3,471 |
| Tests | 1,063 | 1,081 | 1,081+ | All pass |

## Benefits Realized

1. **Code Consistency** - All 42 parsers follow identical TagRegistry pattern
2. **Reduced Duplication** - Array extraction code eliminated across all parsers
3. **Improved Maintainability** - Centralized tag definitions in registry modules
4. **Unified Lens Lookups** - All 12 lens databases use LensDatabase trait
5. **Enhanced Documentation** - Registry modules are self-documenting

## Next Steps

Sprint 4 will focus on:
- Decomposing remaining large files (ICC parser, format detector)
- Further optimization of registry modules
- Performance benchmarking
- Documentation polish
```

### Step 5: Commit metrics

```bash
git add docs/metrics/sprint3-migration-results.md
git commit -m "docs: document Sprint 3 broad migration results (37 parsers)"
```

---

## Sprint 3 Deliverables

✅ 37 parsers migrated to TagRegistry + ArraySchema
✅ 9 additional lens databases unified
✅ ~3,171 line reduction achieved
✅ All existing tests pass
✅ Registry modules created for all remaining manufacturers
✅ Comprehensive metrics documentation

**Cumulative Total:**
- **42 parsers migrated** (of 55 total)
- **12 lens databases unified** (100%)
- **~3,471 total line reduction**

**Next Sprint:** Large file decomposition and final optimizations (Sprint 4).

---

## Execution Strategy

### Recommended Approach

**Execute batches sequentially, not all in parallel:**

1. Complete Batch 1 (5 parsers) → Test → Commit → Document
2. Complete Batch 2 (3 parsers) → Test → Commit → Document
3. Complete Batch 3 (4 parsers) → Test → Commit → Document
4. Complete Batch 4 (3 parsers) → Test → Commit → Document
5. Complete Batch 5 in 4 sub-batches → Test each → Commit → Document

**Why sequential batches?**
- Easier to debug issues (smaller scope)
- Incremental progress commits
- Can pause/resume between batches
- Reduces risk of compilation errors

**Within each batch:**
- Can migrate parsers in parallel if they're independent
- Test after each parser to catch issues early
- Commit working migrations before moving to next

### Time Estimates

- Batch 1: 2-3 days (high complexity, lens DBs)
- Batch 2: 1 day (low complexity)
- Batch 3: 1-2 days (specialty devices)
- Batch 4: 1 day (software apps)
- Batch 5: 3-4 days (22 parsers in sub-batches)

**Total:** 8-11 days of focused work

---

## Risk Mitigation

### Risk 1: Test Failures

**Mitigation:**
- Test each parser individually before batch verification
- Keep original code as reference during migration
- Run tests frequently during refactoring

### Risk 2: Complex Array Structures

**Mitigation:**
- Study existing array extraction patterns carefully
- Start with simpler indices, add complex ones incrementally
- Use helper functions for special cases (lens lookups, calculations)

### Risk 3: Batch Fatigue

**Mitigation:**
- Take breaks between batches
- Celebrate milestones (each batch completion)
- Focus on quality over speed

### Risk 4: Lens Database Incompatibilities

**Mitigation:**
- Follow established pattern from Sprint 2 (Canon, Nikon, Sony)
- Verify lens ID lookups manually for a few test cases
- Maintain backward-compatible API

---

## Success Metrics

**Quantitative:**
- ✅ 37 parsers migrated
- ✅ ~3,171 line reduction (16% average)
- ✅ 9 lens databases unified
- ✅ Zero test regressions

**Qualitative:**
- ✅ All parsers follow consistent architecture
- ✅ Registry modules are well-documented
- ✅ Code is easier to maintain and extend
- ✅ Adding new tags requires minimal code changes
