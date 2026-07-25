# Optimization Strategy

OxiDex uses a data-driven profiling infrastructure and systematic optimization workflow to continuously improve parsing performance. Our goal is to push single-file parsing from ~30ms to sub-10ms through foundational improvements that benefit all 140+ format families.

## Current Performance

View the latest benchmark results on the [Performance Overview](/performance/) page, which is automatically updated by CI/CD on every commit to main.

**Micro-Benchmark Baseline** *(from Criterion reports)*:
- Format detection: ~2.2 ns
- JPEG segment parsing: ~24 ns
- TIFF IFD parsing: ~94 ns
- Full read_metadata: ~9.3 μs

For detailed interactive reports, see:
- [Full Metadata Read](/benchmarks/full_read_metadata/report/index.html)
- [Format Detection](/benchmarks/format_detection/report/index.html)

**Infrastructure:**
- ✅ Comprehensive Criterion benchmarks
- ✅ CI benchmark publishing to GitHub Pages
- ✅ Just recipes for running benchmarks
- ✅ Profiling infrastructure (samply/flamegraph)

## Optimization Workflow

We follow a **data-driven 5-step process** for systematic performance improvements:

### Step 1: Establish Baseline

Run benchmarks to capture current performance metrics:

```bash
just bench
```

Criterion saves baseline results for comparison against future optimizations.

### Step 2: Profile Hotspots

Profile end-to-end execution to identify bottlenecks:

```bash
# Profile a specific benchmark
just profile full_read_metadata

# Profile parsing a real file
just profile-bin tests/fixtures/jpeg/sample_with_exif.jpg

# Profile all benchmarks
just profile-all
```

Look for:
- Functions consuming >5% of total CPU time
- Allocation hotspots
- String operations in tight loops
- Repeated lookups or unnecessary copies

### Step 3: Prioritize by Impact

Focus on functions that are:
- **Hot** (>5% of total time)
- **Fixable** (not in external libraries)
- **High leverage** (called frequently or in critical path)

### Step 4: Optimize & Validate

Make targeted changes:

```bash
# Measure improvement
cargo bench

# Verify hotspot reduction
just profile full_read_metadata

# Ensure no regressions
just test
```

### Step 5: Iterate

Profile again to find the next bottleneck and repeat until hitting diminishing returns.

## Profiling Tools

### samply (Primary Tool)

Interactive profiling with Firefox Profiler UI:

```bash
# Install samply
cargo install samply

# Profile a benchmark
samply record cargo bench --bench parse_benchmarks full_read_metadata
```

**Advantages:**
- No sudo required on macOS
- Interactive flame graphs, call trees, timelines
- Easy integration with Cargo benchmarks

### cargo-flamegraph (Static SVGs)

Quick static flame graph generation:

```bash
# Install cargo-flamegraph
cargo install flamegraph

# Generate flame graph
cargo flamegraph --bench parse_benchmarks -- --bench full_read_metadata
```

## Common Optimization Targets

When analyzing profiles, look for these patterns:

### 1. Allocation Hotspots

**Symptoms:** `String::from()`, `format!()`, `to_string()` in hot paths

**Fixes:**
- Use `&'static str` for known values
- String interning for repeated strings
- Stack buffers instead of heap allocations

**Expected win:** 2-5x in allocation-heavy code

**Example:**
```rust
// Before: allocates on every call
fn tag_name(id: u16) -> String {
    format!("IPTC:{}", id)
}

// After: zero allocations
const TAG_NAMES: &[&str] = &["IPTC:0", "IPTC:1", ...];
fn tag_name(id: u16) -> &'static str {
    TAG_NAMES[id as usize]
}
```

### 2. Tag Lookup Performance

**Symptoms:** HashMap lookups in tight loops, linear searches

**Fixes:**
- Perfect hashing for compile-time known keys
- Cached lookup results
- Compile-time lookup tables

**Expected win:** 1.5-2x in lookup-heavy code

### 3. Redundant Parsing

**Symptoms:** Re-parsing same data, unnecessary validation in loops

**Fixes:**
- Parse once, cache results
- Skip redundant checks in inner loops
- Lazy parsing where possible

**Expected win:** 1.5-3x by eliminating duplicate work

### 4. I/O Patterns

**Symptoms:** Small repeated reads, unnecessary memory copies

**Fixes:**
- Batch reads instead of many small ones
- Leverage memmap2 for large files
- Zero-copy parsing where safe

**Expected win:** 1.2-2x with better I/O

### 5. nom Parser Overhead

**Symptoms:** Parser combinator allocations, excessive backtracking

**Fixes:**
- Hand-written parsers for critical hot paths
- Optimize combinator chains
- Reduce backtracking with better parser design

**Expected win:** 1.5-2x in parser-heavy code

## Success Metrics

Our optimization efforts target:

- **2-3x improvement** in single-file operations
- **All tests passing** (no functionality regressions)
- **No batch performance regression** (parallel processing maintained)
- **Measurable allocation reduction** (lower memory pressure)

## Next Steps

See our [profiling guide](/performance/profiling) for detailed instructions on running profilers and interpreting results.

For current focus areas, check the [optimization design document](https://github.com/swack-tools/oxidex/blob/main/docs/plans/2025-11-18-parsing-performance-optimization-design.md) in the repository.
