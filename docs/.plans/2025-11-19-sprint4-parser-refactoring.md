# Sprint 4: Parser Refactoring & Optimization - Detailed Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Parent Plan:** docs/plans/2025-11-19-parser-complexity-reduction.md
**Sprint:** 4 of 5
**Duration:** 2-3 weeks
**Goal:** Complete parser refactoring for all remaining manufacturers to use their registries, achieving maximum code reduction and consistency across the codebase.

**Prerequisites:**
- Sprint 1 completed (ArraySchema, TagRegistry, LensDatabase infrastructure)
- Sprint 2 completed (5 pilot parsers: Canon, Nikon, Sony, Apple, Google)
- Sprint 3 completed (41 registries created for all manufacturers)

## Success Criteria
- All 41 parsers refactored to use their registries
- 8,000-15,000 line reduction achieved across all parsers
- All existing tests continue to pass (1,190+ tests)
- Zero compilation errors or warnings
- Consistent architecture pattern across all MakerNotes parsers
- Documentation updated with migration metrics

---

## Current State Analysis

### Sprint 3 Completion Status

**Registries Created:** 41 registries (6,255 lines of declarative tag definitions)
**Tests Passing:** 1,190 tests
**Compilation Status:** Zero errors

**Parsers Already Refactored (16 parsers):**
1. Canon - Uses canon_registry()
2. Nikon - Uses nikon_registry()
3. Sony - Fully refactored (1,113 → 945 lines, 15% reduction)
4. Apple - Uses apple_registry()
5. Google - Uses google_registry()
6. Olympus - Refactored (1,134 → 769 lines, 32% reduction)
7. Microsoft - Refactored (581 → 301 lines, 48% reduction)
8. Samsung - Refactored (482 → 290 lines, 40% reduction)
9. Qualcomm - Refactored (469 → 315 lines, 33% reduction)
10. DJI - Refactored (1,060 → 576 lines, 45% reduction)
11. GoPro - Refactored (652 → 562 lines, 14% reduction)
12. FLIR - Refactored (627 → 431 lines, 31% reduction)
13. Lytro - Refactored (347 → 232 lines, 33% reduction)
14. Sigma - Refactored (874 → 219 lines, 75% reduction)
15. Minolta - Refactored (637 → 326 lines, 49% reduction)
16. Casio - Refactored (389 → 250 lines, 35% reduction)
17. Kodak - Refactored (580 → 490 lines, 15% reduction)
18. Capture One - Refactored (593 → 274 lines, 54% reduction)
19. Nikon Capture - Refactored (584 → 248 lines, 58% reduction)

**Registries Created, Parsers NOT Yet Refactored (22 parsers):**

**Group A - Traditional Cameras (4 parsers, ~4,000 lines):**
1. Panasonic (1,044 lines) - Registry: 123 lines, 42 tags
2. Pentax (1,020 lines) - Registry: 156 lines, 45 tags
3. Fujifilm (903 lines) - Registry: 113 lines, 60+ tags
4. Leica (912 lines) - Registry: 112 lines, 65+ tags

**Group B - Specialty Devices (2 parsers, ~570 lines):**
5. Ricoh (216 lines) - Registry: minimal, 9 tags
6. Parrot (308 lines) - Registry: 65 lines, 12 tags

**Group C - Medium Format & Professional (3 parsers, ~2,320 lines):**
7. PhaseOne (916 lines) - Registry: 108 lines, 50+ tags
8. Leaf (416 lines) - Registry: 61 lines, 14 tags
9. RED (487 lines) - Registry: 87 lines, 41 tags

**Group D - Consumer Electronics (7 parsers, ~2,000 lines):**
10. Motorola (274 lines) - Registry: 79 lines, 8 tags
11. HP (279 lines) - Registry: 72 lines, 6 tags
12. JVC (277 lines) - Registry: 75 lines, 6 tags
13. GE (303 lines) - Registry: 74 lines, 5 tags
14. Sanyo (244 lines) - Registry: 82 lines, 8 tags
15. Nintendo (369 lines) - Registry: 83 lines, 10 tags
16. InfiRay (388 lines) - Registry: 87 lines, 18 tags

**Group E - Software Applications (6 parsers, ~2,500 lines):**
17. GIMP (414 lines) - Registry: 127 lines, 43 tags
18. FotoStation (448 lines) - Registry: 117 lines, 31 tags
19. Photo Mechanic (447 lines) - Registry: 118 lines, 39 tags
20. Scalado (353 lines) - Registry: 45 lines, 13 tags
21. InDesign (444 lines) - Registry: 94 lines, 24 tags
22. Reconyx (258 lines) - Registry: 57 lines, 11 tags

**Total Remaining:** ~11,390 lines to refactor

---

## Implementation Strategy

### Refactoring Pattern (Standard Approach)

For each parser, follow this proven pattern:

1. **Review Registry Structure**
   - Examine `src/parsers/tiff/makernotes/registries/{manufacturer}.rs`
   - Identify all registered tags and their decoders
   - Note any array schemas or special formatters

2. **Update Parser Imports**
   ```rust
   use super::registries::{manufacturer}::{manufacturer}_registry;
   use super::shared::ifd_parser_base::{parse_ifd_entries, IfdParserConfig};
   use super::shared::tag_registry::TagRegistry;
   ```

3. **Refactor parse() Method**
   - Instantiate registry: `let registry = {manufacturer}_registry();`
   - Replace manual tag extraction with registry lookups
   - Use `registry.decode_entry()` for standard tags
   - Use `registry.decode_array_*()` for array-based tags
   - Keep specialized logic (byte-level parsing) in helper functions

4. **Remove Redundant Code**
   - Delete tag ID constants (now in registry)
   - Remove manual match statements for tag names
   - Clean up decoder wrapper functions
   - Keep only manufacturer-specific logic

5. **Verify Tests**
   - Run: `cargo test -p oxidex {manufacturer}`
   - Ensure all tests pass
   - Update tests if needed to use new patterns

6. **Measure Reduction**
   - Before lines vs After lines
   - Calculate percentage reduction
   - Document in metrics file

---

## Task Breakdown

### **Batch 1: Traditional Cameras Refactoring**
**Estimated Effort:** 8-10 hours
**Lines to Reduce:** ~4,000 → ~2,400 (40% reduction)

#### Task 1.1: Refactor Panasonic Parser
- **File:** `src/parsers/tiff/makernotes/panasonic.rs`
- **Current:** 1,044 lines
- **Registry:** 123 lines, 42 tags (18 decoders)
- **Expected:** ~650-700 lines (35% reduction)
- **Complexity:** Medium - straightforward tag extraction
- **Special Notes:**
  - Panasonic lens database migration needed
  - No array schemas (simple tag structure)
  - All decoders already public

**Steps:**
1. Import `panasonic_registry()` from registries
2. Replace manual tag extraction (lines 466-760) with registry calls
3. Remove tag ID constants (PANA_VERSION, PANA_CAMERA_MODEL, etc.)
4. Simplify parse method using registry pattern
5. Test: `cargo test -p oxidex panasonic`

#### Task 1.2: Refactor Pentax Parser
- **File:** `src/parsers/tiff/makernotes/pentax.rs`
- **Current:** 1,020 lines
- **Registry:** 156 lines, 45 tags (15 decoders)
- **Expected:** ~650-700 lines (35% reduction)
- **Complexity:** Medium - similar to Panasonic
- **Special Notes:**
  - Pentax lens database exists
  - Array-based CameraSettings may need ArraySchema
  - 15 decoders already public

**Steps:**
1. Import `pentax_registry()` from registries
2. Check for array tags that need ArraySchema support
3. Replace manual tag extraction with registry calls
4. Remove redundant tag constants
5. Test: `cargo test -p oxidex pentax`

#### Task 1.3: Refactor Fujifilm Parser
- **File:** `src/parsers/tiff/makernotes/fujifilm.rs`
- **Current:** 903 lines
- **Registry:** 113 lines, 60+ tags (12 decoders)
- **Expected:** ~550-600 lines (35% reduction)
- **Complexity:** Medium
- **Special Notes:**
  - Fujifilm lens database already exists
  - All 12 decoders public (DECODE_QUALITY, DECODE_WHITE_BALANCE, etc.)
  - Film simulation modes well-defined

**Steps:**
1. Import `fujifilm_registry()` from registries
2. Replace manual tag extraction with registry
3. Integrate lens database lookups
4. Remove tag constants and manual decoders
5. Test: `cargo test -p oxidex fujifilm`

#### Task 1.4: Refactor Leica Parser
- **File:** `src/parsers/tiff/makernotes/leica.rs`
- **Current:** 912 lines
- **Registry:** 112 lines, 65+ tags (10 decoders)
- **Expected:** ~550-600 lines (35% reduction)
- **Complexity:** Medium
- **Special Notes:**
  - Leica lens database exists
  - All 10 decoders public
  - User profiles and scene modes well-defined

**Steps:**
1. Import `leica_registry()` from registries
2. Replace manual tag extraction with registry
3. Integrate lens database
4. Clean up tag constants
5. Test: `cargo test -p oxidex leica`

---

### **Batch 2: Specialty & Small Devices Refactoring**
**Estimated Effort:** 3-4 hours
**Lines to Reduce:** ~570 → ~400 (30% reduction)

#### Task 2.1: Refactor Ricoh Parser
- **File:** `src/parsers/tiff/makernotes/ricoh.rs`
- **Current:** 216 lines
- **Registry:** 54 lines, 9 tags
- **Expected:** ~180 lines (15% reduction)
- **Complexity:** Low - minimal tags
- **Special Notes:** Already partially refactored

#### Task 2.2: Refactor Parrot Parser
- **File:** `src/parsers/tiff/makernotes/parrot.rs`
- **Current:** 308 lines
- **Registry:** 65 lines, 12 tags
- **Expected:** ~220 lines (30% reduction)
- **Complexity:** Low - drone telemetry data

---

### **Batch 3: Medium Format & Professional Refactoring**
**Estimated Effort:** 5-6 hours
**Lines to Reduce:** ~2,320 → ~1,160 (50% reduction)

#### Task 3.1: Refactor PhaseOne Parser
- **File:** `src/parsers/tiff/makernotes/phaseone.rs`
- **Current:** 916 lines
- **Registry:** 108 lines, 50+ tags
- **Expected:** ~400-450 lines (50% reduction)
- **Complexity:** Medium - professional camera features

#### Task 3.2: Refactor Leaf Parser
- **File:** `src/parsers/tiff/makernotes/leaf.rs`
- **Current:** 416 lines
- **Registry:** 61 lines, 14 tags
- **Expected:** ~250 lines (40% reduction)
- **Complexity:** Low - simple tag structure

#### Task 3.3: Refactor RED Parser
- **File:** `src/parsers/tiff/makernotes/red.rs`
- **Current:** 487 lines
- **Registry:** 87 lines, 41 tags
- **Expected:** ~300 lines (38% reduction)
- **Complexity:** Medium - video camera metadata

---

### **Batch 4: Consumer Electronics Refactoring**
**Estimated Effort:** 6-7 hours
**Lines to Reduce:** ~2,000 → ~1,200 (40% reduction)

#### Task 4.1-4.7: Refactor Consumer Device Parsers
Execute in parallel or sequentially for:
- Motorola (274 → ~180 lines)
- HP (279 → ~180 lines)
- JVC (277 → ~180 lines)
- GE (303 → ~200 lines)
- Sanyo (244 → ~160 lines)
- Nintendo (369 → ~240 lines)
- InfiRay (388 → ~250 lines)

**Standard Pattern:**
1. Import registry
2. Replace manual extraction
3. Remove tag constants
4. Test each parser

---

### **Batch 5: Software Applications Refactoring**
**Estimated Effort:** 5-6 hours
**Lines to Reduce:** ~2,500 → ~1,500 (40% reduction)

#### Task 5.1-5.6: Refactor Software Parsers
Execute for:
- GIMP (414 → ~260 lines)
- FotoStation (448 → ~280 lines)
- Photo Mechanic (447 → ~280 lines)
- Scalado (353 → ~220 lines)
- InDesign (444 → ~280 lines)
- Reconyx (258 → ~170 lines)

---

### **Batch 6: Final Testing & Documentation**
**Estimated Effort:** 3-4 hours

#### Task 6.1: Run Full Test Suite
```bash
cargo test -p oxidex
```
- Verify all 1,190+ tests pass
- Fix any regressions
- Document test coverage

#### Task 6.2: Measure Final Metrics
- Count total line reduction
- Calculate percentage reductions by category
- Update docs/metrics/sprint4-refactoring-results.md

#### Task 6.3: Code Quality Review
- Check for any remaining manual tag extraction
- Verify consistent registry usage
- Remove dead code
- Update inline documentation

#### Task 6.4: Performance Benchmarking
- Run existing benchmarks
- Compare against Sprint 2 baseline
- Document any performance improvements

---

## Expected Outcomes

### Line Reduction Summary

| Category | Before | After | Reduction | % |
|----------|--------|-------|-----------|---|
| Traditional Cameras | 4,000 | 2,400 | 1,600 | 40% |
| Specialty Devices | 570 | 400 | 170 | 30% |
| Medium Format | 2,320 | 1,160 | 1,160 | 50% |
| Consumer Electronics | 2,000 | 1,200 | 800 | 40% |
| Software Applications | 2,500 | 1,500 | 1,000 | 40% |
| **Total Sprint 4** | **11,390** | **6,660** | **4,730** | **42%** |

**Combined Sprint 2-4 Reduction:**
- Sprint 2: 5 parsers, ~1,500 lines reduced
- Sprint 3: 19 parsers refactored, ~5,000 lines reduced
- Sprint 4: 22 parsers refactored, ~4,730 lines reduced
- **Grand Total: ~11,230 lines reduced**

### Architecture Benefits

1. **Consistency:** All 41 parsers use identical TagRegistry pattern
2. **Maintainability:** Tag definitions centralized in registry files
3. **Testability:** Registry modules independently testable
4. **Documentation:** Declarative tag specifications self-documenting
5. **Performance:** Zero runtime overhead (compile-time evaluation)

---

## Risk Mitigation

### Risk 1: Test Failures During Refactoring
**Mitigation:**
- Test after each parser refactoring
- Keep original logic for complex cases
- Use git commits per parser for easy rollback

### Risk 2: Performance Regression
**Mitigation:**
- Run benchmarks after each batch
- Profile hot paths if needed
- Registry lookups are O(1) HashMap operations

### Risk 3: Breaking Changes
**Mitigation:**
- No API changes - internal refactoring only
- All test coverage maintained
- Backward compatibility verified

---

## Execution Guidelines

### For Parallel Execution

**Batch 1-5 can be executed in parallel using subagents:**
```
Use 5 clean-code-writer subagents in parallel:
- Subagent 1: Batch 1 (Traditional Cameras)
- Subagent 2: Batch 2 (Specialty Devices)
- Subagent 3: Batch 3 (Medium Format)
- Subagent 4: Batch 4 (Consumer Electronics)
- Subagent 5: Batch 5 (Software Applications)
```

Each subagent should:
1. Refactor all parsers in their batch
2. Run tests for each parser
3. Document metrics (before/after lines)
4. Report completion with summary

### For Sequential Execution

Execute batches 1-5 in order, testing after each batch:
1. Complete all tasks in Batch 1
2. Run tests: `cargo test -p oxidex`
3. Commit changes
4. Proceed to Batch 2
5. Repeat through Batch 5

---

## Definition of Done

- [ ] All 22 remaining parsers refactored to use registries
- [ ] Zero compilation errors or warnings
- [ ] All 1,190+ tests passing
- [ ] Line reduction: 4,500-5,000 lines (40-45% of remaining code)
- [ ] Metrics documented in `docs/metrics/sprint4-refactoring-results.md`
- [ ] Code review completed
- [ ] Performance benchmarks verified
- [ ] Documentation updated

---

## Next Steps (Sprint 5)

After Sprint 4 completion:
1. **Sprint 5:** Final optimization, documentation, and cleanup
2. Implement any remaining performance improvements
3. Create comprehensive migration guide
4. Archive old code patterns
5. Update contributor documentation
