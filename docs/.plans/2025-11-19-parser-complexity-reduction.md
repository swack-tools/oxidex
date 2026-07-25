# Parser Complexity Reduction Plan

> **Created:** 2025-11-19
> **Status:** Planning
> **Goal:** Reduce code complexity and duplication in `src/parsers/` by consolidating patterns, expanding shared infrastructure, and migrating to declarative tag definitions.

## Current State Analysis

### Metrics
- **Total parser files:** 161 Rust files
- **Total lines of code:** 53,362 lines
- **Parser categories:** 17 domains (TIFF, Image, PDF, PE, Video, Archive, etc.)
- **MakerNotes parsers:** 55 manufacturer-specific implementations
- **Largest files:** icc_parser.rs (1,508), canon.rs (1,345), olympus.rs (1,132)
- **Macro adoption:** 240 decoder macro usages across makernotes
- **TagRegistry adoption:** Limited (0-2 parsers currently using it)

### Complexity Hotspots

#### 1. MakerNotes Parsers (27,353 lines)
- **55 manufacturer parsers** with similar structure but vendor-specific quirks
- **Pre-refactor duplication:** 500-1300% (functions implemented 5-13 times)
- **Largest parsers:** Canon (1,345), Olympus (1,132), Sony (1,113), DJI (1,060)
- **Common patterns:** IFD parsing, array extraction, CameraSettings decoding, lens lookups

#### 2. Format-Specific Complexity
- **ICC Parser** (1,508 lines) - Complex color profile tag parsing
- **Format Detector** (1,053 lines) - 200+ magic byte signatures
- **PNG Parser** (1,006 lines) - Multiple chunk types with nested structures
- **QuickTime Metadata** (927 lines) - Recursive atom navigation
- **TIFF IFD Parser** (748 lines) - Recursive IFD handling

#### 3. Current Shared Infrastructure (Good Foundation)
**Location:** `src/parsers/tiff/makernotes/shared/`

| Module | Purpose | Impact |
|--------|---------|--------|
| **generic_decoders.rs** | Pre-built decoders (ON_OFF, YES_NO, QUALITY, etc.) | Eliminates 100+ decoder functions |
| **decoder_macros.rs** | const_decoder!, simple_decoder!, bitfield_decoder! | 60-80% code reduction |
| **ifd_parser_base.rs** | Shared IFD parsing with callbacks | Removes 70-90 lines per parser |
| **array_extractors.rs** | Generic array extraction functions | 4 functions replace 200+ implementations |
| **byte_utils.rs** | Standardized byte reading | Consistent byte order handling |
| **tag_registry.rs** (744 lines) | Declarative tag system (underutilized) | Future consolidation foundation |
| **makernote_parser.rs** | MakerNoteParser trait | Interface consistency |

## Problems to Solve

### P1: TagRegistry Underutilization
The tag registry system (744 lines) exists but is barely used. It provides:
- Centralized tag definitions
- Automatic decoding
- Builder pattern for registration
- Type-safe tag handling

**Current state:** Most parsers still use scattered `match` statements and individual decoder functions instead of registry-based approach.

### P2: CameraSettings Array Duplication
40+ parsers extract the same CameraSettings array indices with near-identical code:
```rust
// Pattern repeated across parsers
if let Some(settings) = extract_i16_array(entry, data, byte_order) {
    if settings.len() > 1 { /* index 1 logic */ }
    if settings.len() > 2 { /* index 2 logic */ }
    // ... repeated for 50-200 indices
}
```

### P3: Large Monolithic Parser Files
Several parsers exceed 1,000 lines due to:
- Large `match` statements for tag IDs
- Inline decoder functions
- Repeated array index handling
- Mix of parsing logic and decoding logic

### P4: Lens Database Fragmentation
10+ parsers have similar `lookup_lens()` implementations with vendor-specific databases. No shared lens lookup infrastructure.

### P5: Limited Format Detector Maintainability
Format detector has 200+ magic byte signatures in a single 1,053-line file. Adding new formats requires manual insertion.

### P6: ICC Parser Complexity
ICC parser handles dozens of tag types with complex nested structures, all in a single 1,508-line file.

## Proposed Solutions

### Phase 1: MakerNotes TagRegistry Migration (Highest Impact)

**Goal:** Migrate 20-30 high-volume parsers to TagRegistry-based approach.

**Target parsers:** Canon, Nikon, Sony, Olympus, Panasonic, Fujifilm, Pentax, Leica, Apple, Google, Samsung, DJI, GoPro, FLIR, Adobe

**Approach:**
1. **Create registry modules** - One registry per manufacturer in `makernotes/registries/`
   ```
   src/parsers/tiff/makernotes/registries/
   ├── canon.rs       - Canon tag registry
   ├── nikon.rs       - Nikon tag registry
   ├── sony.rs        - Sony tag registry
   └── mod.rs         - Re-exports all registries
   ```

2. **Define tags declaratively**
   ```rust
   // Before: 200+ lines of match statements
   // After: 50 lines of registry setup
   pub fn canon_registry() -> TagRegistry {
       TagRegistry::new()
           .register_simple(0x0001, "MacroMode", &MACRO_MODE)
           .register_simple(0x0002, "SelfTimer", &SELF_TIMER)
           .register_simple(0x0003, "Quality", &QUALITY)
           // ... all tags
   }
   ```

3. **Simplify parser implementation**
   ```rust
   impl MakerNoteParser for CanonParser {
       fn parse(&self, data: &[u8], byte_order: ByteOrder, tags: &mut HashMap<String, String>) -> Result<(), String> {
           let registry = canon_registry();
           parse_ifd_entries(data, byte_order, &config, |entry, data| {
               registry.decode_and_insert(entry, tags);
           })
       }
   }
   ```

**Expected reduction:**
- **Per-parser savings:** 200-400 lines (30-40% reduction)
- **Total savings:** 6,000-12,000 lines across 30 parsers
- **Maintainability:** Centralized tag definitions, easier to add/update tags

### Phase 2: CameraSettings Array Consolidation

**Goal:** Create declarative array index specifications to eliminate repetitive array extraction logic.

**Approach:**
1. **Create shared array schema definitions**
   ```rust
   // src/parsers/tiff/makernotes/shared/array_schemas.rs
   pub struct ArrayIndexDef {
       pub index: usize,
       pub name: &'static str,
       pub decoder: Option<I16Decoder>,
   }

   pub struct ArraySchema {
       pub name: &'static str,
       pub indices: &'static [ArrayIndexDef],
   }
   ```

2. **Define schemas per manufacturer**
   ```rust
   pub static CANON_CAMERA_SETTINGS: ArraySchema = ArraySchema {
       name: "CameraSettings",
       indices: &[
           ArrayIndexDef { index: 1, name: "MacroMode", decoder: Some(MACRO_MODE) },
           ArrayIndexDef { index: 2, name: "SelfTimer", decoder: Some(SELF_TIMER) },
           // ... all indices
       ],
   };
   ```

3. **Generic array processor**
   ```rust
   fn process_array_with_schema(
       array: &[i16],
       schema: &ArraySchema,
       tags: &mut HashMap<String, String>
   ) {
       for def in schema.indices {
           if let Some(value) = array.get(def.index) {
               let decoded = def.decoder.map(|d| d(*value))
                   .unwrap_or_else(|| value.to_string());
               tags.insert(format!("{}:{}", schema.name, def.name), decoded);
           }
       }
   }
   ```

**Expected reduction:**
- **Per-parser savings:** 100-300 lines for parsers with large arrays
- **Total savings:** 4,000-8,000 lines across 40+ parsers
- **Consistency:** Standardized array handling across all manufacturers

### Phase 3: Lens Database Unification

**Goal:** Create shared lens lookup infrastructure.

**Approach:**
1. **Centralized lens database storage**
   ```
   src/parsers/tiff/makernotes/lens_databases/
   ├── canon.rs       - Canon lens DB
   ├── nikon.rs       - Nikon lens DB
   ├── sony.rs        - Sony lens DB
   └── registry.rs    - Unified lookup interface
   ```

2. **Shared lookup trait**
   ```rust
   pub trait LensDatabase {
       fn lookup(&self, lens_id: u16) -> Option<&'static str>;
       fn lookup_range(&self, id_min: u16, id_max: u16) -> Option<&'static str>;
   }
   ```

3. **Generic implementation**
   ```rust
   pub struct StaticLensDb {
       entries: &'static [(u16, &'static str)],
   }

   impl LensDatabase for StaticLensDb {
       fn lookup(&self, lens_id: u16) -> Option<&'static str> {
           self.entries.iter()
               .find(|(id, _)| *id == lens_id)
               .map(|(_, name)| *name)
       }
   }
   ```

**Expected reduction:**
- **Code deduplication:** 10 similar implementations → 1 shared + 10 data definitions
- **Total savings:** 500-1,000 lines
- **Extensibility:** Easy to add new lens databases

### Phase 4: Large File Decomposition

**Goal:** Break up monolithic parsers into logical modules.

**Target files:**
- icc_parser.rs (1,508 lines) → Split by tag category
- format_detector.rs (1,053 lines) → Structured format definitions
- canon.rs (1,345 lines) → Sub-modules for CameraSettings, ShotInfo, etc.
- olympus.rs (1,132 lines) → Separate equipment and settings modules

**Approach:**
1. **Module-per-concern pattern**
   ```
   src/parsers/tiff/makernotes/canon/
   ├── mod.rs              - Main parser integration
   ├── registry.rs         - Tag registry
   ├── camera_settings.rs  - CameraSettings array schema
   ├── shot_info.rs        - ShotInfo array schema
   ├── file_info.rs        - FileInfo array schema
   └── lens_database.rs    - Canon lens lookup
   ```

2. **Clear separation of concerns**
   - `mod.rs`: Parser orchestration, signature validation
   - `registry.rs`: Simple tag definitions
   - `*_settings.rs`: Array schemas and processing
   - `lens_database.rs`: Static lens data

**Expected outcome:**
- **File sizes:** 1,000+ line files → 200-300 line modules
- **Maintainability:** Easier to navigate, test, and modify
- **Reusability:** Sub-modules can be shared if patterns emerge

### Phase 5: Format Detector Restructuring

**Goal:** Make format detection declarative and maintainable.

**Approach:**
1. **Structured format definitions**
   ```rust
   pub struct FormatSignature {
       pub name: &'static str,
       pub mime_type: &'static str,
       pub magic_bytes: &'static [MagicPattern],
       pub extensions: &'static [&'static str],
   }

   pub enum MagicPattern {
       Exact { offset: usize, bytes: &'static [u8] },
       Range { offset: usize, bytes: &'static [u8], end_offset: usize },
       AnyOf { offset: usize, options: &'static [&'static [u8]] },
   }
   ```

2. **Move signatures to separate module**
   ```
   src/parsers/format_signatures/
   ├── mod.rs           - Detection engine
   ├── image.rs         - Image format signatures
   ├── video.rs         - Video format signatures
   ├── document.rs      - Document format signatures
   └── archive.rs       - Archive format signatures
   ```

3. **Generic matching engine**
   ```rust
   pub fn detect_format(data: &[u8]) -> Option<&'static FormatSignature> {
       ALL_SIGNATURES.iter()
           .find(|sig| sig.matches(data))
   }
   ```

**Expected outcome:**
- **Better organization:** Grouped by category instead of single file
- **Easier additions:** Add new format = add one struct
- **Testability:** Each signature can be unit tested independently

### Phase 6: ICC Parser Modularization

**Goal:** Break ICC parser into tag-category modules.

**Approach:**
1. **Module per tag category**
   ```
   src/parsers/icc/
   ├── mod.rs              - Main parser
   ├── header.rs           - Header parsing
   ├── profile_tags.rs     - Profile description tags
   ├── color_tags.rs       - Color transformation tags
   ├── measurement_tags.rs - Measurement tags
   └── rendering_tags.rs   - Rendering intent tags
   ```

2. **Tag category registration**
   ```rust
   pub fn parse_icc_profile(data: &[u8]) -> Result<IccProfile, String> {
       let header = parse_header(data)?;
       let mut tags = HashMap::new();

       parse_profile_tags(data, &mut tags)?;
       parse_color_tags(data, &mut tags)?;
       parse_measurement_tags(data, &mut tags)?;
       parse_rendering_tags(data, &mut tags)?;

       Ok(IccProfile { header, tags })
   }
   ```

**Expected outcome:**
- **File size reduction:** 1,508 lines → 6 modules of 200-300 lines each
- **Clarity:** Tag categories clearly separated
- **Parallel development:** Multiple contributors can work on different categories

## Implementation Roadmap

### Sprint 1: Foundation Enhancement (Week 1-2)
**Focus:** Enhance shared infrastructure to support new patterns.

**Tasks:**
- [ ] Extend TagRegistry to support array schemas
- [ ] Create ArraySchema infrastructure
- [ ] Design LensDatabase trait and implementations
- [ ] Write tests for new shared utilities
- [ ] Document new patterns with examples

**Deliverable:** Enhanced `src/parsers/tiff/makernotes/shared/` with array schema support.

### Sprint 2: Pilot Migration (Week 3-4)
**Focus:** Migrate 3-5 representative parsers to prove patterns work.

**Target parsers:** Canon, Nikon, Sony (high complexity), Apple, Google (low complexity)

**Tasks:**
- [ ] Create registry modules for pilot parsers
- [ ] Define array schemas for CameraSettings arrays
- [ ] Migrate lens databases to unified system
- [ ] Refactor pilot parsers to use new infrastructure
- [ ] Verify functionality with existing tests
- [ ] Measure code reduction and complexity metrics

**Deliverable:** 5 migrated parsers with 30-40% code reduction.

### Sprint 3: Broad MakerNotes Migration (Week 5-8)
**Focus:** Migrate remaining 25-30 makernotes parsers.

**Batches:**
- **Batch 1:** Traditional cameras (Olympus, Panasonic, Fujifilm, Pentax, Leica)
- **Batch 2:** Smartphones (Samsung, Microsoft, Qualcomm)
- **Batch 3:** Specialty devices (DJI, GoPro, FLIR, Lytro)
- **Batch 4:** Software (Adobe, Capture One, Nikon Capture)

**Tasks:**
- [ ] Create registries for each batch
- [ ] Define array schemas where applicable
- [ ] Migrate parsers batch by batch
- [ ] Run regression tests after each batch
- [ ] Update documentation

**Deliverable:** 30+ migrated parsers, 6,000-12,000 lines reduced.

### Sprint 4: Large File Decomposition (Week 9-10)
**Focus:** Break up remaining monolithic files.

**Tasks:**
- [ ] Restructure Canon parser into sub-modules
- [ ] Restructure Olympus parser into sub-modules
- [ ] Modularize ICC parser by tag category
- [ ] Restructure format detector with signature modules
- [ ] Update module imports and tests

**Deliverable:** All 1,000+ line files split into 200-300 line modules.

### Sprint 5: Documentation & Polish (Week 11-12)
**Focus:** Document new architecture and patterns.

**Tasks:**
- [ ] Write architecture documentation for shared infrastructure
- [ ] Create contribution guide for adding new parsers
- [ ] Document array schema pattern with examples
- [ ] Document lens database integration
- [ ] Run clippy and address any new warnings
- [ ] Performance benchmarking to ensure no regressions

**Deliverable:** Comprehensive documentation, clean codebase ready for future contributions.

## Success Metrics

### Quantitative Goals
- **Line reduction:** 15,000-25,000 lines removed (28-47% of current 53,362 lines)
- **File size:** No files over 800 lines (currently 10 files exceed this)
- **Module count:** 55 makernotes parsers → 55 + 30 registry modules (better organization)
- **Test coverage:** Maintain or improve current test coverage
- **Performance:** No degradation in parsing speed

### Qualitative Goals
- **Maintainability:** New tags can be added in 1-3 lines instead of 20-50
- **Consistency:** All parsers follow same patterns (TagRegistry + ArraySchema)
- **Documentation:** Clear examples for common patterns
- **Contributor experience:** Easier to add new manufacturer support
- **Code clarity:** Separation of concerns (parsing vs. decoding vs. data)

## Risk Mitigation

### Risk 1: Breaking Changes
**Mitigation:**
- Extensive test suite execution after each migration
- Gradual rollout with pilot parsers first
- Keep original parsers as reference during migration

### Risk 2: Performance Regression
**Mitigation:**
- Benchmark critical paths before/after migration
- Profile registry lookup performance
- Optimize hot paths if needed (caching, compile-time optimizations)

### Risk 3: Incomplete Migration
**Mitigation:**
- Prioritize high-impact parsers first (Canon, Nikon, Sony)
- Document both old and new patterns during transition
- Set clear milestones with deliverables

### Risk 4: TagRegistry Complexity
**Mitigation:**
- Start with simple tag-only registries
- Add array schema support incrementally
- Provide extensive documentation and examples

## Future Opportunities

### Beyond This Plan
1. **Cross-domain pattern extraction** - Apply registry patterns to PNG, PDF, Quicktime parsers
2. **Compile-time tag validation** - Macro-based tag ID collision detection
3. **Auto-generated documentation** - Extract tag docs from registries
4. **Unified metadata API** - Common interface across all parser types
5. **Performance optimization** - Tag registry could use perfect hashing for O(1) lookups

### Continuous Improvement
- Monitor new parser additions to ensure they follow established patterns
- Regular complexity audits (quarterly reviews of file sizes and duplication)
- Community contribution templates for new manufacturer support
- Automated checks in CI to prevent regression to old patterns

## Appendix: Pattern Examples

### Example 1: Before/After TagRegistry Migration

**Before (Canon parser - excerpt):**
```rust
const TAG_MACRO_MODE: u16 = 0x0001;
const TAG_SELF_TIMER: u16 = 0x0002;
const TAG_QUALITY: u16 = 0x0003;
// ... 100+ more constants

fn decode_macro_mode(value: i16) -> String {
    match value {
        1 => "Macro".to_string(),
        2 => "Normal".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_quality(value: i16) -> String {
    match value {
        1 => "Economy".to_string(),
        2 => "Normal".to_string(),
        3 => "Fine".to_string(),
        5 => "Superfine".to_string(),
        _ => format!("Unknown ({})", value),
    }
}
// ... 50+ more decoder functions

impl MakerNoteParser for CanonParser {
    fn parse(&self, data: &[u8], byte_order: ByteOrder, tags: &mut HashMap<String, String>) -> Result<(), String> {
        parse_ifd_entries(data, byte_order, &config, |entry, data| {
            match entry.tag {
                TAG_MACRO_MODE => {
                    if let Some(value) = read_i16_value(entry, data, byte_order) {
                        tags.insert("Canon:MacroMode".to_string(), decode_macro_mode(value));
                    }
                }
                TAG_QUALITY => {
                    if let Some(value) = read_i16_value(entry, data, byte_order) {
                        tags.insert("Canon:Quality".to_string(), decode_quality(value));
                    }
                }
                // ... 100+ more match arms
                _ => {}
            }
        })
    }
}
```

**After (using TagRegistry):**
```rust
// In src/parsers/tiff/makernotes/registries/canon.rs
use super::super::shared::generic_decoders::*;
use super::super::shared::tag_registry::TagRegistry;

const_decoder!(MACRO_MODE, i16, [
    (1, "Macro"),
    (2, "Normal"),
]);

const_decoder!(QUALITY, i16, [
    (1, "Economy"),
    (2, "Normal"),
    (3, "Fine"),
    (5, "Superfine"),
]);

pub fn canon_registry() -> TagRegistry {
    TagRegistry::new()
        .register_simple(0x0001, "MacroMode", &MACRO_MODE)
        .register_simple(0x0002, "SelfTimer", &SELF_TIMER)
        .register_simple(0x0003, "Quality", &QUALITY)
        // ... more tags (50 lines total vs. 400+ before)
}

// In src/parsers/tiff/makernotes/canon.rs
impl MakerNoteParser for CanonParser {
    fn parse(&self, data: &[u8], byte_order: ByteOrder, tags: &mut HashMap<String, String>) -> Result<(), String> {
        let registry = canon_registry();
        parse_ifd_entries(data, byte_order, &config, |entry, data| {
            registry.decode_and_insert(entry, data, byte_order, "Canon", tags);
        })
    }
}
```

### Example 2: Array Schema Pattern

**Before (repetitive array handling):**
```rust
if let Some(settings) = extract_i16_array(entry, data, byte_order) {
    if settings.len() > 1 {
        tags.insert("Canon:MacroMode".to_string(), decode_macro_mode(settings[1]));
    }
    if settings.len() > 2 {
        tags.insert("Canon:SelfTimer".to_string(), settings[2].to_string());
    }
    if settings.len() > 3 {
        tags.insert("Canon:Quality".to_string(), decode_quality(settings[3]));
    }
    // ... 200+ more indices
}
```

**After (declarative schema):**
```rust
static CAMERA_SETTINGS_SCHEMA: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        ArrayIndexDef { index: 1, name: "MacroMode", decoder: Some(MACRO_MODE) },
        ArrayIndexDef { index: 2, name: "SelfTimer", decoder: None },
        ArrayIndexDef { index: 3, name: "Quality", decoder: Some(QUALITY) },
        // ... all indices (50 lines vs. 200+)
    ],
};

if let Some(settings) = extract_i16_array(entry, data, byte_order) {
    process_array_with_schema(&settings, &CAMERA_SETTINGS_SCHEMA, "Canon", tags);
}
```

## References

- Current shared infrastructure: `src/parsers/tiff/makernotes/shared/`
- TagRegistry implementation: `src/parsers/tiff/makernotes/shared/tag_registry.rs`
- Decoder macros: `src/parsers/tiff/makernotes/shared/decoder_macros.rs`
- MakerNoteParser trait: `src/parsers/tiff/makernotes/shared/makernote_parser.rs`
