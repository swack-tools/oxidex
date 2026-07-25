# Performance Profiling Guide

This guide explains how to profile OxiDex to identify performance bottlenecks and validate optimizations.

## Quick Start: Text-Based Profiling

For accessible, cross-platform performance analysis without visual tools:

```bash
just profile-simple
```

This runs all benchmarks and displays timing results in plain text:

```
Benchmark: full_read_metadata
time:   [3.4 ms 3.5 ms 3.6 ms]

Benchmark: tiff_simple
time:   [5.3 ms 5.5 ms 5.7 ms]  ← Slowest, optimization target
```

**Benefits:**
- No sudo required
- Works on all platforms
- Screen-reader friendly
- Easy to compare before/after

## Overview

OxiDex supports multiple profiling approaches:

1. **Text-based benchmarking** (recommended) - Simple timing via Criterion benchmarks
2. **samply profiling** - Interactive visual profiling with Firefox Profiler UI
3. **cargo-flamegraph** - SVG flame graph generation (Linux preferred)

### samply

samply provides interactive flame graphs, call trees, and timelines through the Firefox Profiler UI. This allows you to:

- Identify CPU hotspots (functions consuming the most time)
- Visualize call stacks and execution flow
- Measure impact of optimizations
- Find allocation bottlenecks

## Prerequisites

### Installing samply

samply is a modern profiling tool for macOS and Linux that requires no sudo access.

**macOS:**
```bash
cargo install samply
```

**Linux:**
```bash
cargo install samply
```

**Verify installation:**
```bash
samply --version
```

## Quick Start

### Profile a Specific Benchmark

```bash
# Profile the full metadata read benchmark
just profile full_read_metadata

# Profile format detection
just profile format_detection

# Profile JPEG segment parsing
just profile jpeg_segment_parsing
```

This will:
1. Run the benchmark with profiling enabled
2. Capture performance data
3. Automatically open the Firefox Profiler with results

### Profile Integration Benchmarks

```bash
# Profile single file extraction
just profile-integration single_extraction

# Profile batch processing
just profile-integration batch_processing
```

### Profile the CLI Binary

```bash
# Profile parsing a specific file
just profile-bin tests/fixtures/jpeg/sample_with_exif.jpg

# Profile with multiple files
just profile-bin -r tests/fixtures/jpeg/
```

### Profile All Benchmarks

```bash
# Warning: This takes several minutes
just profile-all
```

## Interpreting Results

### Firefox Profiler UI

When samply opens the Firefox Profiler, you'll see several views:

#### 1. Flame Graph (Default)
- **Width** = time spent in function
- **Height** = call stack depth
- **Hover** = see function name, time, percentage
- **Click** = zoom into that function

**What to look for:**
- Wide bars = hot functions (>5% of total time)
- Tall stacks = deep call chains (potential for inlining)
- Repeated patterns = opportunities for caching

#### 2. Call Tree
- Hierarchical view of function calls
- Shows self time vs. total time
- Sort by "self time" to find leaf hotspots

**What to look for:**
- High self time = actual work being done
- High total time, low self time = orchestration overhead

#### 3. Stack Chart
- Timeline view showing execution over time
- Shows what's running at each moment

**What to look for:**
- Repeated patterns = potential for batching
- Long-running functions = optimization targets

#### 4. Marker Chart
- Shows discrete events and markers
- Useful for understanding execution flow

## Common Optimization Patterns

### 1. Allocation Hotspots

**Symptoms in profiler:**
- Time spent in `alloc::alloc`, `String::from`, `format!`, `to_string`
- High percentage in allocation functions

**Example:**
```rust
// BEFORE (allocates on every call)
fn tag_name(id: u16) -> String {
    format!("Tag_{}", id)
}

// AFTER (uses static strings for known values)
fn tag_name(id: u16) -> &'static str {
    match id {
        0x0001 => "ImageWidth",
        0x0002 => "ImageHeight",
        _ => "Unknown",
    }
}
```

**How to verify:**
```bash
just profile full_read_metadata
# Look for reduced time in alloc::* functions
```

### 2. HashMap Lookup Overhead

**Symptoms in profiler:**
- Time in `HashMap::get`, hash calculations
- Repeated lookups in tight loops

**Example:**
```rust
// BEFORE (HashMap lookup on every tag)
let tag_info = tag_map.get(&tag_id)?;

// AFTER (perfect hash or array lookup)
const TAG_NAMES: &[&str; 256] = &[
    "ImageWidth", "ImageHeight", /* ... */
];
let tag_name = TAG_NAMES[tag_id as usize];
```

### 3. Redundant Parsing

**Symptoms in profiler:**
- Same parsing function called multiple times
- Time spent re-reading same data

**Example:**
```rust
// BEFORE (parses byte order repeatedly)
fn parse_entry(data: &[u8]) -> Entry {
    let byte_order = detect_byte_order(data); // Called N times
    // ...
}

// AFTER (parse once, pass down)
fn parse_ifd(data: &[u8]) -> IFD {
    let byte_order = detect_byte_order(data); // Called once
    parse_entries(data, byte_order)
}
```

### 4. Small Repeated I/O

**Symptoms in profiler:**
- Many small calls to `read()`, `read_at()`
- Time spent in I/O syscalls

**Example:**
```rust
// BEFORE (many small reads)
for i in 0..count {
    let value = reader.read(offset + i * 4, 4)?;
    process(value);
}

// AFTER (bulk read)
let buffer = reader.read(offset, count * 4)?;
for chunk in buffer.chunks(4) {
    process(chunk);
}
```

### 5. nom Parser Overhead

**Symptoms in profiler:**
- Time in nom combinator functions
- Many allocations in parser code

**Example:**
```rust
// BEFORE (combinator overhead)
fn parse_tag(input: &[u8]) -> IResult<&[u8], Tag> {
    let (input, id) = be_u16(input)?;
    let (input, type_) = be_u16(input)?;
    let (input, count) = be_u32(input)?;
    // ...
}

// AFTER (hand-written for hot path)
fn parse_tag(input: &[u8]) -> Result<Tag> {
    if input.len() < 12 { return Err(Error::TooShort); }
    let id = u16::from_be_bytes([input[0], input[1]]);
    let type_ = u16::from_be_bytes([input[2], input[3]]);
    let count = u32::from_be_bytes([input[4], input[5], input[6], input[7]]);
    // ...
}
```

## Optimization Workflow

### 1. Establish Baseline

```bash
# Run benchmarks to record current performance
just bench

# Save results for comparison
cp -r target/criterion target/criterion-baseline
```

### 2. Profile Hotspots

```bash
# Profile the target benchmark
just profile full_read_metadata

# Analyze in Firefox Profiler
# Identify top 3-5 functions by time (>5% of total)
```

### 3. Prioritize Targets

Focus on functions that are:
- **Hot** (>5% of total time)
- **Fixable** (in your code, not external libraries)
- **High leverage** (called frequently or in critical path)

### 4. Optimize & Validate

```bash
# Make changes to hot path
# ...

# Re-run benchmark
cargo bench --bench parse_benchmarks full_read_metadata

# Compare against baseline
# Should see measurable improvement

# Re-profile to verify
just profile full_read_metadata
# Hotspot should be reduced or eliminated
```

### 5. Iterate

```bash
# Profile again to find next bottleneck
just profile full_read_metadata

# Repeat process until hitting diminishing returns
```

## Tips & Best Practices

### Profiling Tips

1. **Profile release builds** - Debug builds have overhead that masks real bottlenecks
2. **Profile real workloads** - Use actual test files, not synthetic data
3. **Look for patterns** - Single outliers may not matter; repeated patterns do
4. **Measure, don't guess** - Profile before and after every optimization

### Benchmark Tips

1. **Use Criterion's compare** - `cargo bench` automatically compares to baseline
2. **Run multiple times** - Performance can vary; look at median and variance
3. **Minimize system load** - Close other apps, don't browse while benchmarking
4. **Check for regressions** - Ensure optimizations don't break other paths

### Code Review Tips

1. **Document optimizations** - Explain why non-obvious code is faster
2. **Keep tests passing** - Performance means nothing if correctness breaks
3. **Avoid premature optimization** - Profile first, optimize second
4. **Measure impact** - Document speedup in commit messages

## Example Session

Here's a complete optimization session:

```bash
# 1. Establish baseline
just bench
cp -r target/criterion target/criterion-baseline

# 2. Profile to find hotspots
just profile full_read_metadata
# Firefox Profiler shows: 15% time in format!() for tag names

# 3. Make optimization
# Edit src/parsers/jpeg/iptc_parser.rs
# Replace format!() with static string table

# 4. Validate improvement
cargo bench --bench parse_benchmarks full_read_metadata
# Output: "time: [-12.5% -10.2% -8.1%]" = 10% faster

# 5. Re-profile to verify
just profile full_read_metadata
# Firefox Profiler shows: format!() now <1% of time

# 6. Run tests to ensure correctness
just test
# All tests pass

# 7. Commit changes
git add src/parsers/jpeg/iptc_parser.rs
git commit -m "perf: optimize IPTC tag name generation

Replace format!() allocations with static string lookups
for known dataset numbers, falling back to format!() only
for unknown values.

Benchmark results: 10.2% improvement in full_read_metadata
Profiling: Reduced format!() from 15% to <1% of runtime"
```

## Troubleshooting

### samply command not found

```bash
# Install samply
cargo install samply

# Verify installation
which samply
samply --version
```

### Profile data too large

```bash
# Profile a shorter benchmark
cargo bench --bench parse_benchmarks format_detection -- --sample-size 10

# Or profile a single file instead of batch
just profile-bin tests/fixtures/jpeg/sample_with_exif.jpg
```

### Firefox Profiler won't open

```bash
# samply saves to /tmp by default
# Find the profile file
ls -lt /tmp/*.json | head -1

# Open manually at profiler.firefox.com
open https://profiler.firefox.com
# Upload the JSON file
```

### Results show mostly syscalls

This is expected for I/O-heavy workloads. Focus on:
- What's calling those syscalls
- Can you batch the operations?
- Are you memory-mapping (already using memmap2)?

## macOS Limitations

**Symbol Resolution:** samply on macOS may not resolve function symbols properly, showing hex addresses (like `0x44f8`) instead of function names. This is a known limitation of macOS profiling tools.

**Workarounds:**
- Use `just profile-simple` for text-based timing analysis
- Use Instruments.app (requires Xcode)
- Add manual instrumentation for specific functions
- Focus on benchmark timing comparisons rather than deep profiling

**Why this happens:** macOS DTrace and samply have difficulty resolving symbols from Rust binaries even with debug info enabled. Linux's `perf` tool works better for symbol resolution.

## Alternative: cargo-flamegraph

cargo-flamegraph generates visual flame graphs as SVG files. It uses:
- **Linux**: `perf` (no sudo required)
- **macOS**: `dtrace` (requires sudo)

### Installation

```bash
cargo install flamegraph
```

### Usage (Linux - recommended)

```bash
# Profile a benchmark
cargo flamegraph --bench parse_benchmarks -o flamegraph.svg -- --bench full_read_metadata

# Profile the CLI
cargo flamegraph --bin oxidex -o flamegraph.svg -- tests/fixtures/jpeg/sample_with_exif.jpg

# View in browser
firefox flamegraph.svg
```

### Comparison: samply vs flamegraph vs text-based

| Feature | Text-Based | samply | flamegraph |
|---------|------------|--------|------------|
| **Accessibility** | Screen-reader friendly | Visual | Visual (SVG) |
| **macOS sudo** | Not required | Not required | Required |
| **Symbol resolution** | N/A | May fail on macOS | May fail on macOS |
| **Detail level** | Benchmark-level | Function-level | Function-level |
| **Best for** | Quick comparison | Interactive analysis | Static reports |

**Recommendation:** Start with `just profile-simple` to identify slow benchmarks, then use samply (Linux) or Instruments (macOS) for function-level detail.

## Additional Resources

- [samply GitHub](https://github.com/mstange/samply) - Official documentation
- [Firefox Profiler Guide](https://profiler.firefox.com/docs/) - UI documentation
- [Rust Performance Book](https://nnethercote.github.io/perf-book/) - Optimization patterns
- OxiDex benchmarks: `benches/parse_benchmarks.rs`, `benches/integration_benchmarks.rs`

## Summary

**Quick commands:**
```bash
just profile full_read_metadata    # Profile core benchmark
just profile-bin <file>            # Profile CLI with specific file
just bench                         # Run all benchmarks
```

**Workflow:**
1. Baseline → 2. Profile → 3. Prioritize → 4. Optimize → 5. Validate → 6. Iterate

**Look for:**
- Allocations (format!, String::from)
- Lookups (HashMap::get)
- Redundant work (parsing same data twice)
- Small I/O (many small reads)
- Parser overhead (nom combinators)

**Success:** 2-3x improvement in targeted hot paths, maintaining correctness.
