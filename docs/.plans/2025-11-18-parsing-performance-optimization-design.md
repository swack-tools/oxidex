# Parsing Performance Optimization Design

**Date:** 2025-11-18
**Status:** Approved
**Goal:** Optimize core parsing infrastructure to push single-file parsing from ~30ms to sub-10ms

## Overview

This design establishes a data-driven profiling infrastructure and systematic optimization workflow for OxiDex. Rather than optimizing specific formats, we focus on foundational improvements that benefit all 140+ format families.

## Current State

**Performance Baseline:**
- Format detection: ~2.2 ns
- JPEG segment parsing: ~24 ns
- TIFF IFD parsing: ~94 ns
- Full read_metadata: ~9.3 μs (well below 5ms target)
- Single JPEG read: 31.8ms ± 14.1ms

**Infrastructure:**
- ✅ Comprehensive Criterion benchmarks (parse_benchmarks.rs, integration_benchmarks.rs)
- ✅ CI benchmark publishing to GitHub Pages
- ✅ Just recipes for running benchmarks
- ❌ No profiling infrastructure (flamegraph/samply)
- ❌ No systematic optimization workflow

**Known Opportunities:**
- IPTC tag name generation uses `format!()` with allocations on every call (identified in parse_benchmarks.rs:186-199)

## Goals

**Primary Goal:** Reduce single-file parsing time from ~30ms to sub-10ms through systematic infrastructure optimization.

**Success Metrics:**
- 2-3x improvement in single-file operations
- All existing tests continue to pass
- No regression in batch processing performance
- Measurable reduction in allocations and memory usage

## Design

### 1. Profiling Infrastructure

**Tool Choice:** samply
- No sudo required on macOS
- Interactive Firefox Profiler UI with flame graphs, call trees, timelines
- Easy integration with Cargo benchmarks

**Justfile Additions:**

```just
# Profile a specific benchmark with samply
profile benchmark:
    @echo "Profiling {{benchmark}} benchmark..."
    samply record cargo bench --bench parse_benchmarks {{benchmark}}

# Profile integration benchmarks
profile-integration benchmark:
    @echo "Profiling integration benchmark: {{benchmark}}..."
    samply record cargo bench --bench integration_benchmarks {{benchmark}}

# Profile the CLI binary with arguments
profile-bin *args:
    @echo "Profiling binary with args: {{args}}..."
    cargo build --release
    samply record ./target/release/oxidex {{args}}

# Profile all parse benchmarks (warning: takes a while)
profile-all:
    @echo "Profiling all parse benchmarks..."
    samply record cargo bench --bench parse_benchmarks
```

**Usage Examples:**
```bash
# Profile the full_read_metadata benchmark
just profile full_read_metadata

# Profile parsing a specific file
just profile-bin tests/fixtures/jpeg/sample_with_exif.jpg

# Profile all benchmarks
just profile-all
```

### 2. Optimization Workflow

**Data-Driven 5-Step Process:**

**Step 1: Establish Baseline**
- Run `just bench` to capture current performance metrics
- Criterion saves baseline results for comparison

**Step 2: Profile Hotspots**
- Run `just profile full_read_metadata` to profile end-to-end
- Identify top 3-5 functions consuming most CPU time
- Look for: allocations, string operations, repeated lookups, unnecessary copies

**Step 3: Prioritize by Impact**
Focus on functions that are:
- **Hot** (>5% of total time)
- **Fixable** (not in external libraries)
- **High leverage** (called frequently or in critical path)

**Step 4: Optimize & Validate**
- Make targeted changes to hot paths
- Run `cargo bench` to measure improvement
- Use `samply` to verify hotspot reduction
- Ensure tests pass (`just test`)

**Step 5: Iterate**
- Profile again to find next bottleneck
- Repeat until hitting diminishing returns

### 3. Common Optimization Targets

When analyzing samply profiles, look for:

**1. Allocation Hotspots**
- `String::from()`, `format!()`, `to_string()` in hot paths
- **Fix:** Use `&'static str` for known values, string interning, stack buffers
- **Example:** IPTC tag name generation (already identified)
- **Expected win:** 2-5x in allocation-heavy code

**2. Tag Lookup Performance**
- HashMap lookups in tight loops
- Linear searches through tag definitions
- **Fix:** Perfect hashing, compile-time lookup tables, cached results
- **Expected win:** 1.5-2x in lookup-heavy code

**3. Redundant Parsing**
- Re-parsing same data multiple times
- Unnecessary validation in inner loops
- **Fix:** Parse once, cache results, skip redundant checks
- **Expected win:** 1.5-3x by eliminating duplicate work

**4. I/O Patterns**
- Small, repeated reads instead of bulk reads
- Unnecessary memory copies
- **Fix:** Batch reads, leverage memmap2, zero-copy parsing
- **Expected win:** 1.2-2x with better I/O

**5. nom Parser Overhead**
- Parser combinator allocations
- Backtracking in parsers
- **Fix:** Hand-written parsers for hot paths, optimize combinator chains
- **Expected win:** 1.5-2x in parser-heavy code

### 4. Documentation

Create `docs/profiling.md` with:
- How to install samply (if needed)
- How to run profiling with Just recipes
- How to interpret Firefox Profiler results
- Workflow for identifying and fixing bottlenecks
- Examples of common optimization patterns

## Implementation Plan

1. **Add Just recipes** for profiling workflows
2. **Create docs/profiling.md** documentation
3. **Run initial profiling** to establish hotspot baseline
4. **Iterate on optimizations** using the 5-step workflow
5. **Document wins** and update benchmarks

## Success Criteria

- ✅ Profiling infrastructure integrated (samply + Just recipes)
- ✅ Documentation complete and tested
- ✅ Initial profiling run identifies top 5 hotspots
- ✅ At least one optimization implemented and validated
- ✅ Performance improvement measured and documented
- ✅ All tests continue to pass

## Future Considerations

- Consider adding cargo-flamegraph for quick static SVG generation
- Explore memory profiling (heaptrack, valgrind) for memory optimization
- Set up automated performance regression detection in CI
- Create performance dashboard tracking improvements over time

## References

- [samply GitHub](https://github.com/mstange/samply)
- [Firefox Profiler](https://profiler.firefox.com/)
- Current benchmarks: `benches/parse_benchmarks.rs`, `benches/integration_benchmarks.rs`
- Existing performance metrics: README.md benchmark section
