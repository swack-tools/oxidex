# Codacy Complexity Reduction Implementation Plan

> **Created:** November 19, 2025
> **Status:** Planning
> **Goal:** Reduce complex files from 23% (125/538) to <10% (54/538)
> **Version:** v1.2.1

## Executive Summary

This plan provides a systematic approach to reduce code complexity across the OxiDex codebase to meet Codacy's quality standards. Through proven refactoring patterns and shared utilities, we will reduce complexity by 71 files (from 125 to 54 complex files).

**Key Metrics:**
- **Current State:** 23% files complex (125/538)
- **Target State:** <10% files complex (54/538)
- **Files to Refactor:** 71 files
- **Estimated Effort:** 12-16 weeks
- **Expected Duplication Reduction:** From 100-900% to <50%

**Proven Success:**
- Samsung MakerNotes: 1294% duplication → <50%
- GoPro MakerNotes: 136% duplication → 0%
- Qualcomm MakerNotes: 153% duplication → 0%
- Format Detector: 83% duplication → ~35%

---

## Current State Analysis

### Complexity Distribution

**File Categories by Complexity:**

1. **MakerNotes Parsers (55 files)**
   - Largest complexity source
   - Size range: 7-48KB
   - Common patterns: Large match statements, repetitive decoders
   - Status: 3 refactored, 2 partial, 50 pending

2. **Core Infrastructure (3 files)**
   - operations.rs (2053 lines, 51 functions)
   - icc_parser.rs (1485 lines)
   - tiff_writer.rs (1116 lines)

3. **Format Parsers (~20 files)**
   - format_detector.rs (1053 lines) - ✅ Already refactored
   - JPEG, PNG, PDF, MP4, etc. parsers

4. **Generated/Data Files**
   - tag_registry.rs (7494 lines) - Auto-generated
   - tags_*.rs - Auto-generated tag definitions
   - Acceptable complexity (data, not logic)

### MakerNotes Parser Sizes

**Large Parsers (>30KB):**
```
canon.rs         48KB  - Camera-specific metadata
sony.rs          37KB  - Camera-specific metadata
dji.rs           34KB  - ⚠️ Partially refactored
pentax.rs        33KB
panasonic.rs     33KB
olympus.rs       32KB
fujifilm.rs      31KB
```

**Medium Parsers (20-30KB):**
```
nikon.rs         27KB
phaseone.rs      26KB
leica.rs         24KB
sigma.rs         23KB
photoshop.rs     23KB  - ⚠️ Partially refactored
flir.rs          23KB
... (10 more)
```

**Small Parsers (7-20KB):**
```
... (35+ files)
```

### Shared Framework Status

**✅ Available Utilities (src/parsers/tiff/makernotes/shared/):**
- `generic_decoders.rs` - Pre-built decoders (ON_OFF, YES_NO, etc.)
- `decoder_macros.rs` - Declarative decoder syntax (const_decoder!)
- `tag_registry.rs` - Centralized tag management
- `array_extractors.rs` - Array handling helpers
- `byte_utils.rs` - Byte manipulation utilities
- `value_decoders.rs` - Value decoding helpers
- `ifd_parser_base.rs` - Base IFD parsing logic
- `makernote_parser.rs` - Common parser patterns
- `USAGE_EXAMPLES.md` - Complete documentation

**✅ Successfully Refactored (Proof of Concept):**
1. **samsung.rs** - F(0) → B grade, 906% → 0% duplication
2. **gopro.rs** - C(68) → A grade, 136% → 0% duplication
3. **qualcomm.rs** - C(53) → A grade, 153% → 0% duplication

**⚠️ Partially Refactored:**
4. **photoshop.rs** - B(71) grade, 108% duplication → needs completion
5. **dji.rs** - C(63) grade, 113% duplication → needs completion

---

## Proven Refactoring Patterns

### Pattern 1: const_decoder! Macro

**Problem:** Repetitive decoder functions with identical structure

**Before (59 lines for 6 decoders):**
```rust
fn decode_scene_optimizer(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "On".to_string(),
        2 => "Auto".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_scene_type(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Food".to_string(),
        2 => "Sunset".to_string(),
        // ... 12 more mappings
        _ => format!("Unknown ({})", value),
    }
}
// ... 4 more similar functions
```

**After (49 lines using macro):**
```rust
const_decoder!(SCENE_OPTIMIZER, i16, [
    (0, "Off"),
    (1, "On"),
    (2, "Auto"),
]);

const_decoder!(SCENE_TYPE, i16, [
    (0, "None"),
    (1, "Food"),
    (2, "Sunset"),
    // ... mappings only
]);
```

**Results:**
- 17% reduction in lines
- 100% reduction in duplication
- Declarative data vs procedural code

### Pattern 2: Pre-built Generic Decoders

**Problem:** Many On/Off, Yes/No, Auto/Manual decoders

**Before:**
```rust
fn decode_optimizer(value: i16) -> String {
    match value { 0 => "Off", 1 => "On", _ => "Unknown" }
}
fn decode_stabilizer(value: i16) -> String {
    match value { 0 => "Off", 1 => "On", _ => "Unknown" }
}
fn decode_noise_reduction(value: i16) -> String {
    match value { 0 => "Off", 1 => "On", _ => "Unknown" }
}
```

**After:**
```rust
use super::shared::generic_decoders::ON_OFF;

// All use the same pre-built decoder
let optimizer = ON_OFF.decode(value);
let stabilizer = ON_OFF.decode(value);
let noise_reduction = ON_OFF.decode(value);
```

**Available Decoders:**
- `ON_OFF` - Binary on/off states
- `YES_NO` - Binary yes/no states
- `AUTO_MANUAL` - Auto/manual modes
- `QUALITY_LMH` - Low/Medium/High quality
- `ENABLED_DISABLED` - Enabled/disabled states

### Pattern 3: TagRegistry Pattern

**Problem:** Large parse_entry functions with 100+ line match statements

**Before (124 lines):**
```rust
fn parse_entry(tag_id: u16, value: &[u8]) -> Option<(String, String)> {
    match tag_id {
        0x0001 => Some(("Version".to_string(), decode_version(value))),
        0x0002 => Some(("SceneOptimizer".to_string(), decode_scene_optimizer(value))),
        0x0003 => Some(("SceneType".to_string(), decode_scene_type(value))),
        // ... 50+ more match arms
        _ => None,
    }
}
```

**After (11 lines):**
```rust
static SAMSUNG_TAGS: Lazy<TagRegistry<u16>> = Lazy::new(|| {
    TagRegistry::builder()
        .add(0x0001, "Version", TagType::Ascii)
        .add(0x0002, "SceneOptimizer", TagType::I16, Some(&SCENE_OPTIMIZER))
        .add(0x0003, "SceneType", TagType::I16, Some(&SCENE_TYPE))
        // ... tag definitions only
        .build()
});

fn parse_entry(tag_id: u16, value: &[u8]) -> Option<(String, String)> {
    SAMSUNG_TAGS.lookup(tag_id, value)
}
```

**Results:**
- 91% reduction in function size (124 → 11 lines)
- 100% reduction in duplication
- Table-driven, data-oriented design
- Single source of truth for tags

### Pattern 4: Table-Driven Design

**Problem:** Repetitive conditional logic for format detection

**Before (format_detector.rs):**
```rust
if bytes.starts_with(b"\xFF\xD8\xFF") { return Some(FileFormat::JPEG); }
if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { return Some(FileFormat::PNG); }
if bytes.starts_with(b"GIF87a") { return Some(FileFormat::GIF); }
// ... 50+ more if statements
```

**After (table-driven):**
```rust
const SIGNATURES: &[FormatSignature] = &[
    FormatSignature { magic: b"\xFF\xD8\xFF", format: FileFormat::JPEG },
    FormatSignature { magic: b"\x89PNG\r\n\x1a\n", format: FileFormat::PNG },
    FormatSignature { magic: b"GIF87a", format: FileFormat::GIF },
    // ... signature data
];

for sig in SIGNATURES {
    if bytes.starts_with(sig.magic) { return Some(sig.format); }
}
```

**Results:**
- 83% → 35% duplication
- C(63) → B+(75-85) grade
- Easier to maintain and extend

---

## Implementation Plan

### Phase 0: Preparation (Week 1)

**Goals:**
- Complete pending refactorings
- Validate shared framework
- Create tooling

**Tasks:**

1. **Complete Partial Refactorings**
   - ✅ Finish photoshop.rs refactoring
   - ✅ Finish dji.rs refactoring
   - ✅ Validate all tests pass
   - ✅ Document any issues encountered

2. **Framework Validation**
   - ✅ Review all shared utilities
   - ✅ Identify any missing helpers
   - ✅ Add missing decoders if needed
   - ✅ Update USAGE_EXAMPLES.md

3. **Tooling Development**
   - ✅ Create complexity measurement script
   - ✅ Create parser categorization tool
   - ✅ Create migration template generator
   - ✅ Set up CI complexity tracking

**Deliverables:**
- photoshop.rs and dji.rs completed
- Framework documentation updated
- Tooling scripts ready
- Baseline complexity metrics captured

---

### Phase 1: High-Priority MakerNotes (Weeks 2-5)

**Goals:**
- Refactor 20 largest MakerNotes parsers
- Reduce complexity by ~30 files

**Target Files (by size):**

**Batch 1 (Week 2):** Large Parsers (>30KB)
```
□ canon.rs         (48KB) - Most complex
□ sony.rs          (37KB)
□ pentax.rs        (33KB)
□ panasonic.rs     (33KB)
□ olympus.rs       (32KB)
```

**Batch 2 (Week 3):** Large Parsers continued
```
□ fujifilm.rs      (31KB)
□ nikon.rs         (27KB)
□ phaseone.rs      (26KB)
□ leica.rs         (24KB)
□ sigma.rs         (23KB)
```

**Batch 3 (Week 4):** Medium Parsers (20-23KB)
```
□ flir.rs          (23KB)
□ minolta.rs       (22KB)
□ captureone.rs    (22KB)
□ nikoncapture.rs  (21KB)
□ microsoft.rs     (20KB)
```

**Batch 4 (Week 5):** Medium Parsers continued
```
□ kodak.rs         (19KB)
□ google.rs        (19KB)
□ apple.rs         (19KB)
□ red.rs           (16KB)
□ casio.rs         (16KB)
```

**Per-File Process:**
1. Read current parser implementation
2. Identify all decoders and tag definitions
3. Create TagRegistry with const_decoder! macros
4. Replace parse_entry with registry lookup
5. Extract common patterns to shared helpers
6. Run full test suite
7. Measure complexity improvement
8. Document any patterns for future parsers

**Success Criteria per Batch:**
- All tests passing
- Duplication <50% (ideally 0%)
- Complexity grade B or better
- No performance regression

---

### Phase 2: Remaining MakerNotes (Weeks 6-9)

**Goals:**
- Refactor remaining 30 MakerNotes parsers
- Reduce complexity by ~25 files

**Batch 5-8 (Weeks 6-9):** Small to Medium Parsers
```
Week 6 (8 parsers):
□ photomechanic.rs  (15KB)
□ fotostation.rs    (15KB)
□ indesign.rs       (14KB)
□ gimp.rs           (14KB)
□ leaf.rs           (13KB)
□ infiray.rs        (13KB)
□ lytro.rs          (12KB)
□ scalado.rs        (10KB)

Week 7 (8 parsers):
□ nintendo.rs       (9.8KB)
□ parrot.rs         (9.7KB)
□ motorola.rs       (9.4KB)
□ reconyx.rs        (9.2KB)
□ sanyo.rs          (9.0KB)
□ ricoh.rs          (8.2KB)
□ ge.rs             (7.8KB)
□ jvc.rs            (7.0KB)

Week 8 (7 parsers):
□ hp.rs             (7.0KB)
□ [Any new parsers added]
□ [Remaining small parsers]

Week 9 (Review & Cleanup):
□ Final batch completion
□ Cross-parser consistency check
□ Update shared framework if needed
□ Documentation updates
```

**Automation Opportunities:**
- Script to auto-generate TagRegistry boilerplate
- Batch testing runner
- Complexity metrics dashboard

---

### Phase 3: Core Infrastructure (Weeks 10-13)

**Goals:**
- Refactor core non-MakerNotes files
- Reduce complexity by ~10-15 files

**3.1: operations.rs (Week 10)**

**Current:** 2053 lines, 51 functions

**Strategy:**
- Analyze function complexity individually
- Extract helper functions for common patterns
- Break large functions into smaller focused functions
- Use builder patterns for complex operations
- Consider strategy pattern for variant operations

**Potential Refactorings:**
```rust
// Before: Large function with many branches
fn process_metadata(input: Input) -> Result<Output> {
    // 150 lines of conditional logic
}

// After: Extracted strategies
fn process_metadata(input: Input) -> Result<Output> {
    let strategy = MetadataStrategy::for_format(input.format);
    strategy.process(input)
}
```

**3.2: icc_parser.rs (Week 11)**

**Current:** 1485 lines

**Strategy:**
- Apply TagRegistry pattern for ICC tag definitions
- Extract profile type handlers
- Table-driven tag parsing
- Reusable tag decoders

**3.3: tiff_writer.rs (Week 12)**

**Current:** 1116 lines

**Strategy:**
- Extract IFD writing strategies
- Builder pattern for TIFF structure
- Separate concerns: validation, serialization, I/O
- Reusable write helpers

**3.4: Other Large Files (Week 13)**

**Candidates:**
- Format-specific parsers that weren't already refactored
- Any files >500 lines with high complexity
- Files flagged by Codacy

---

### Phase 4: Validation & Optimization (Weeks 14-16)

**Goals:**
- Verify complexity reduction targets met
- Optimize any remaining issues
- Document improvements

**4.1: Metrics Collection (Week 14)**

**Tasks:**
- Run full Codacy analysis
- Generate complexity report
- Identify any remaining complex files
- Analyze duplication metrics
- Performance benchmarking

**Target Metrics:**
- ✅ <10% files complex (54/538)
- ✅ 0% duplication in MakerNotes
- ✅ <50% duplication in core files
- ✅ Grade B+ or better for all parsers
- ✅ No performance regression

**4.2: Cleanup & Polish (Week 15)**

**Tasks:**
- Address any files still above complexity threshold
- Refactor any newly identified patterns
- Update shared utilities based on learnings
- Code review and quality checks

**4.3: Documentation (Week 16)**

**Tasks:**
- Update all refactoring documentation
- Create migration guide for future parsers
- Document patterns and anti-patterns
- Update architecture documentation
- Create complexity reduction case study

---

## Success Metrics

### Quantitative Goals

| Metric | Current | Target | Measurement |
|--------|---------|--------|-------------|
| Complex Files % | 23% (125/538) | <10% (54/538) | Codacy grade |
| MakerNotes Duplication | 100-900% | 0% | Codacy analysis |
| Core Files Duplication | Varies | <50% | Codacy analysis |
| Average Function Length | TBD | <50 lines | Code metrics |
| Cyclomatic Complexity | TBD | <15 per function | Code metrics |

### Qualitative Goals

- ✅ All MakerNotes parsers use shared framework
- ✅ Consistent patterns across all parsers
- ✅ Improved code maintainability
- ✅ Easier to add new format support
- ✅ Better test coverage
- ✅ No performance regression
- ✅ Comprehensive documentation

### File-Specific Targets

**MakerNotes Parsers (55 files):**
- Duplication: 0%
- Grade: B+ or better
- Pattern: TagRegistry + const_decoder!

**Core Infrastructure (3 files):**
- operations.rs: Grade B+, <50% duplication
- icc_parser.rs: Grade B+, <30% duplication
- tiff_writer.rs: Grade B+, <40% duplication

**Format Parsers:**
- Maintain current quality
- Apply table-driven patterns where applicable

---

## Risk Assessment

### Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Test failures during refactoring | Medium | High | Run tests after each file, maintain test coverage |
| Performance regression | Low | High | Benchmark before/after, optimize hot paths |
| Breaking changes to API | Low | Medium | Maintain public API compatibility |
| Incomplete shared framework | Low | Medium | Validate framework early, add utilities as needed |
| Time overrun | Medium | Medium | Prioritize high-impact files first |

### Process Risks

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Inconsistent patterns | Medium | Medium | Code review, shared guidelines |
| Documentation lag | Medium | Low | Document as we go, templates |
| Loss of domain knowledge | Low | High | Comment complex business logic |
| Merge conflicts | High | Low | Small PRs, frequent integration |

---

## Resource Requirements

### Time Commitment

**Total Effort:** 12-16 weeks

**Breakdown:**
- Phase 0 (Preparation): 1 week
- Phase 1 (High-Priority MakerNotes): 4 weeks
- Phase 2 (Remaining MakerNotes): 4 weeks
- Phase 3 (Core Infrastructure): 4 weeks
- Phase 4 (Validation): 3 weeks

**Parallel Work Opportunities:**
- MakerNotes batches can be done in parallel by multiple developers
- Documentation can be done concurrently
- Tooling development can overlap with refactoring

### Expertise Needed

- **Rust Proficiency:** Intermediate to advanced
- **Pattern Recognition:** Ability to identify common patterns
- **Testing:** Unit and integration test development
- **Domain Knowledge:** Understanding of metadata formats (helpful but not required)
- **Refactoring Skills:** Experience with large-scale code refactoring

### Tools Required

- **Codacy:** Complexity and duplication analysis
- **cargo-clippy:** Rust linting
- **cargo-criterion:** Performance benchmarking
- **git:** Version control, branch management
- **Custom scripts:** Complexity measurement, migration templates

---

## Automation Strategy

### Scripts to Develop

**1. Complexity Analyzer (`scripts/analyze_complexity.sh`)**
```bash
#!/bin/bash
# Analyzes codebase complexity and generates report
# Usage: ./scripts/analyze_complexity.sh

cargo clippy -- -W clippy::cognitive_complexity
find src -name "*.rs" -exec wc -l {} + | sort -rn
# Generate JSON report for tracking
```

**2. MakerNotes Migration Template (`scripts/generate_makernote_template.sh`)**
```bash
#!/bin/bash
# Generates boilerplate for new MakerNotes parser
# Usage: ./scripts/generate_makernote_template.sh manufacturer_name

MANUFACTURER=$1
cat > src/parsers/tiff/makernotes/${MANUFACTURER}.rs << 'EOF'
use once_cell::sync::Lazy;
use super::shared::tag_registry::TagRegistry;
// ... template code
EOF
```

**3. Batch Test Runner (`scripts/test_batch.sh`)**
```bash
#!/bin/bash
# Runs tests for a batch of files
# Usage: ./scripts/test_batch.sh parser1.rs parser2.rs ...

for file in "$@"; do
    echo "Testing $file..."
    cargo test --lib --test "*${file%.rs}*"
done
```

**4. Complexity Dashboard (`scripts/complexity_dashboard.py`)**
```python
#!/usr/bin/env python3
# Generates HTML dashboard of complexity metrics
# Usage: python3 scripts/complexity_dashboard.py

import json
import matplotlib.pyplot as plt

# Parse Codacy JSON
# Generate charts
# Output HTML dashboard
```

---

## Migration Checklist

Use this checklist for each MakerNotes parser refactoring:

### Pre-Migration
- [ ] Read current implementation
- [ ] Run existing tests (capture baseline)
- [ ] Document any complex business logic
- [ ] Identify all decoder functions
- [ ] List all tag IDs and names

### Migration
- [ ] Create TagRegistry structure
- [ ] Convert decoders to const_decoder! macros
- [ ] Use pre-built generic decoders where applicable
- [ ] Replace parse_entry with registry lookup
- [ ] Extract any new patterns to shared utilities
- [ ] Add helper functions if needed

### Validation
- [ ] All tests pass
- [ ] No compiler warnings
- [ ] Run cargo clippy
- [ ] Check complexity metrics
- [ ] Verify duplication <50% (ideally 0%)
- [ ] Performance benchmark (no regression)

### Documentation
- [ ] Update file header comments
- [ ] Document any non-obvious patterns
- [ ] Update USAGE_EXAMPLES.md if new patterns added
- [ ] Add entry to refactoring log

---

## Communication Plan

### Progress Tracking

**Weekly Updates:**
- Files refactored this week
- Complexity metrics improvement
- Issues encountered
- Next week's targets

**Milestone Reports:**
- End of each phase summary
- Cumulative metrics
- Lessons learned
- Adjusted timeline if needed

### Documentation

**Living Documents:**
- This implementation plan (update as we go)
- Complexity metrics spreadsheet
- Refactoring patterns guide
- Lessons learned log

**Final Deliverables:**
- Complexity reduction case study
- Best practices guide
- Migration templates
- Updated architecture docs

---

## Contingency Plans

### If Timeline Slips

**Priority 1 (Must Have):**
- Complete Phase 0 and Phase 1 (20 largest MakerNotes)
- This alone should get us close to target

**Priority 2 (Should Have):**
- Complete Phase 2 (remaining MakerNotes)
- This ensures all parsers use consistent patterns

**Priority 3 (Nice to Have):**
- Complete Phase 3 (core infrastructure)
- Phase 4 polish

### If Shared Framework Insufficient

**Fallback:**
- Pause migrations
- Enhance shared framework
- Document new patterns
- Resume with improved utilities

### If Performance Regression

**Strategy:**
- Identify hot paths with profiling
- Optimize specific bottlenecks
- Consider caching strategies
- May need to compromise on some abstractions

---

## Appendix

### A. Shared Framework Reference

**Location:** `src/parsers/tiff/makernotes/shared/`

**Key Files:**
- `generic_decoders.rs` - Pre-built decoders
- `decoder_macros.rs` - Macro definitions
- `tag_registry.rs` - Registry pattern
- `USAGE_EXAMPLES.md` - Complete guide

**Usage Example:**
```rust
use once_cell::sync::Lazy;
use super::shared::{
    generic_decoders::{ON_OFF, QUALITY_LMH},
    tag_registry::{TagRegistry, TagType},
};

const_decoder!(SCENE_MODE, i16, [
    (0, "None"),
    (1, "Portrait"),
    (2, "Landscape"),
]);

static MYTAGS: Lazy<TagRegistry<u16>> = Lazy::new(|| {
    TagRegistry::builder()
        .add(0x0001, "Version", TagType::Ascii)
        .add(0x0002, "Quality", TagType::I16, Some(&QUALITY_LMH))
        .add(0x0003, "SceneMode", TagType::I16, Some(&SCENE_MODE))
        .build()
});
```

### B. Complexity Measurement

**Codacy Metrics:**
- **Cyclomatic Complexity:** Number of independent paths through code
- **Cognitive Complexity:** How hard code is to understand
- **Duplication:** Percentage of duplicated code blocks
- **File Grade:** Overall quality score (A-F)

**Target Thresholds:**
- Cyclomatic Complexity: <15 per function
- Cognitive Complexity: <10 per function
- Duplication: <50% (ideally 0%)
- File Grade: B+ or better

### C. Related Documentation

- `docs/development/archived-context.md` - Historical refactoring context
- `docs/refactoring/samsung-refactoring-summary.md` - Samsung case study
- `docs/refactoring/gopro-refactoring-summary.md` - GoPro case study
- `docs/refactoring/format-detector-complete.md` - Format detector refactoring
- `src/parsers/tiff/makernotes/shared/USAGE_EXAMPLES.md` - Framework guide

### D. Example Refactoring Diff

**Before (photoshop.rs partial):**
```rust
fn decode_quality(value: i16) -> String {
    match value {
        0 => "Low".to_string(),
        1 => "Medium".to_string(),
        2 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn parse_entry(tag_id: u16, data: &[u8]) -> Option<(String, String)> {
    match tag_id {
        0x0001 => Some(("Version".to_string(), decode_version(data))),
        0x0002 => Some(("Quality".to_string(), decode_quality(data))),
        // ... 50 more arms
        _ => None,
    }
}
```

**After:**
```rust
use super::shared::generic_decoders::QUALITY_LMH;

static PHOTOSHOP_TAGS: Lazy<TagRegistry<u16>> = Lazy::new(|| {
    TagRegistry::builder()
        .add(0x0001, "Version", TagType::Ascii)
        .add(0x0002, "Quality", TagType::I16, Some(&QUALITY_LMH))
        // ... tag definitions
        .build()
});

fn parse_entry(tag_id: u16, data: &[u8]) -> Option<(String, String)> {
    PHOTOSHOP_TAGS.lookup(tag_id, data)
}
```

---

## Change Log

| Date | Version | Changes |
|------|---------|---------|
| 2025-11-19 | 1.0 | Initial plan created |

---

**Document Status:** 📋 Planning
**Next Review:** After Phase 0 completion
**Owner:** Development Team
