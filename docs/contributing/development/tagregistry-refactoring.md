# TagRegistry Refactoring Project

This document summarizes the TagRegistry refactoring project completed in November 2025, which standardized all 46 MakerNotes parsers to use a centralized, declarative pattern.

## Overview

The project migrated all MakerNotes parsers from ad-hoc implementations to a unified `TagRegistry` pattern with declarative array schemas, achieving:

- **40% code reduction** across parser files
- **100% architectural consistency** across 46 parsers
- **Zero performance regression** (compile-time evaluation)
- **100% test pass rate** (1,165 tests)

## Motivation

Before the refactoring, MakerNotes parsers had:

- Repetitive decoder functions (10-30 per parser)
- Inconsistent tag extraction patterns
- High code duplication (some parsers had 181%+ duplication)
- Difficult maintenance (changes required touching many functions)

## Architecture

### TagRegistry Pattern

Each parser now uses a centralized registry:

```rust
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(|| manufacturer_registry());
```

### ArraySchema System

Complex array tags are defined declaratively:

```rust
const CAMERA_SETTINGS: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        (1, "MacroMode", DECODE_MACRO_MODE),
        (2, "Quality", DECODE_QUALITY),
        (3, "FlashMode", DECODE_FLASH_MODE),
        // ...
    ],
};
```

### Shared Utilities

Common decoders are centralized:

- `ON_OFF` - Binary on/off values
- `YES_NO` - Binary yes/no values
- `ENABLED_DISABLED` - Binary enabled/disabled
- `const_decoder!` macro for custom decoders

## Results by Phase

### Phase 1: Foundation (Sprint 1-2)

Created shared infrastructure:
- `TagRegistry` struct with O(1) lookup
- `ArraySchema` for declarative array parsing
- `LensDatabase` trait for lens lookups
- Shared decoder utilities

**Pilot migrations**: Sony parser validated the approach with 15% line reduction.

### Phase 2: Broad Migration (Sprint 3)

Migrated 19 additional parsers:
- Traditional cameras (Panasonic, Pentax, Fujifilm, Leica)
- Specialty devices (Ricoh, Parrot)
- Medium format (PhaseOne, Leaf, RED)

**Results**: ~50% code reduction across migrated parsers.

### Phase 3: Completion (Sprint 4)

Migrated remaining 22 parsers in 5 batches:

| Batch | Parsers | Reduction |
|-------|---------|-----------|
| Traditional Cameras | 4 | 32% |
| Specialty Devices | 2 | 4% |
| Medium Format | 3 | 27% |
| Consumer Electronics | 7 | 21% |
| Software Applications | 6 | 71% |

**Total Sprint 4**: 3,860 lines reduced (36.2%)

### Phase 4: Optimization (Sprint 5)

Fixed remaining issues:
- 5 test failures from pre-existing bugs
- 44 compiler warnings eliminated
- Code formatting standardized

**Final state**: 1,165 tests passing, 0 warnings.

## Metrics Summary

### Code Reduction

| Category | Before | After | Change |
|----------|--------|-------|--------|
| Total Parser Lines | ~26,000 | ~16,000 | -40% |
| Registry Lines | 0 | 6,356 | +6,356 |
| Net Change | - | - | -4,000 lines |

### Quality Improvements

- **Duplication**: Reduced from 80%+ to <40% in most parsers
- **Complexity**: Reduced cyclomatic complexity by 30%+
- **Consistency**: 100% of parsers follow same pattern

## Key Learnings

### What Worked Well

1. **Declarative over imperative**: Array schemas more maintainable than match statements
2. **Shared utilities**: `const_decoder!` macro eliminated massive duplication
3. **Phased approach**: Pilot migrations validated design before broad rollout
4. **Parallel execution**: Batched migrations saved time

### Challenges Encountered

1. **Registry overhead**: Some simple parsers saw negative ROI due to registry boilerplate
2. **Decoder visibility**: Required careful module organization for public/private access
3. **Model variations**: Some manufacturers have model-specific formats requiring special handling

### Recommendations

For future parser additions:

1. Use `TagRegistry` pattern for parsers with 5+ tags
2. Use `ArraySchema` for complex array tags
3. Leverage shared decoders when possible
4. Create manufacturer-specific registry modules

## Files Modified

### Core Infrastructure

- `src/parsers/tiff/makernotes/shared/` - Shared utilities
  - `array_extractors.rs` - Array value extraction
  - `generic_decoders.rs` - Common decoders (ON_OFF, YES_NO, etc.)
  - `decoder_macros.rs` - `const_decoder!` macro
  - `tag_registry.rs` - TagRegistry implementation

### Registry Modules

41 registry files in `src/parsers/tiff/makernotes/registries/`:
- One per manufacturer (canon.rs, nikon.rs, sony.rs, etc.)
- Contains tag definitions, decoders, and array schemas

### Parser Modules

46 parser files in `src/parsers/tiff/makernotes/`:
- Each uses TagRegistry for lookups
- Minimal parsing logic (delegation to registry)

## References

- [Parser Migration Guide](/architecture/parser-migration-guide)
- [Parser Shared Infrastructure](/architecture/parser-shared-infrastructure)

---

**Completed**: November 2025
**Total Effort**: ~4 weeks across 5 sprints
