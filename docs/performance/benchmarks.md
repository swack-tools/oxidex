# Performance Benchmarks

OxiDex includes comprehensive benchmarking infrastructure to track performance improvements and regressions over time.

## Live Interactive Reports

Our CI/CD pipeline automatically generates detailed benchmark reports on every commit to main. These reports provide interactive visualizations, statistical analysis, and historical comparisons.

### Main Benchmark Suite

View complete benchmark results with interactive graphs:

**<a href="/benchmarks/report/index.html" target="_blank">Main Benchmark Report →</a>**

This report includes:
- Violin plots showing performance distribution
- Statistical outlier detection
- Historical trend analysis
- Regression detection

### Individual Benchmark Reports

Detailed reports for specific operations:

- **<a href="/benchmarks/single_extraction/report/index.html" target="_blank">Single File Extraction</a>** - Metadata extraction from a single JPEG file
- **<a href="/benchmarks/batch_100_jpegs/report/index.html" target="_blank">Batch Processing (100 JPEGs)</a>** - Parallel batch processing performance
- **<a href="/benchmarks/format_comparison/report/index.html" target="_blank">Format Comparison</a>** - Performance across different file formats (JPEG, PNG, TIFF, RAW)
- **<a href="/benchmarks/format_detection/report/index.html" target="_blank">Format Detection</a>** - File type identification via magic bytes
- **<a href="/benchmarks/full_read_metadata/report/index.html" target="_blank">Full Metadata Read</a>** - Complete metadata extraction with all tags

Each report provides:
- **Performance graphs:** Violin plots, line charts, and scatter plots
- **Statistical metrics:** Mean, median, standard deviation, outliers
- **Historical data:** Track performance over time across commits
- **Regression alerts:** Automatic detection of performance degradation

## Benchmark Infrastructure

### Criterion.rs

OxiDex uses [Criterion.rs](https://github.com/bheisler/criterion.rs) for robust, statistical benchmarking:

- **Statistical rigor:** Detects performance changes with confidence intervals
- **Outlier detection:** Identifies and flags anomalous measurements
- **Regression testing:** Compares against previous baseline
- **HTML reports:** Interactive visualizations for detailed analysis

### Continuous Benchmarking

Benchmarks run automatically on every commit:

1. **On push to main:** Full benchmark suite executes on GitHub Actions
2. **Results published:** HTML reports deployed to `/benchmarks/` directory
3. **Historical tracking:** Performance trends tracked across commits
4. **Alerts:** PRs blocked if significant regressions detected (future enhancement)

## Running Benchmarks Locally

### Prerequisites

Install Rust and build tools:

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install hyperfine (for comparative benchmarks)
cargo install hyperfine
# or
brew install hyperfine  # macOS
sudo apt install hyperfine  # Ubuntu
```

### Library Micro-Benchmarks

Run Criterion.rs benchmarks for internal operations:

```bash
# Clone repository
git clone https://github.com/swack-tools/oxidex.git
cd oxidex

# Run all benchmarks
cargo bench

# Run specific benchmark group
cargo bench format_detection
cargo bench jpeg_segment_parsing
cargo bench tiff_ifd_parsing
cargo bench batch_processing
```

**View results:**

```bash
# macOS
open target/criterion/report/index.html

# Linux
xdg-open target/criterion/report/index.html

# Windows
start target/criterion/report/index.html
```

### Comparative Benchmarks (vs Perl ExifTool)

Compare OxiDex against the original Perl ExifTool:

```bash
# Install ExifTool
brew install exiftool  # macOS
sudo apt install libimage-exiftool-perl  # Ubuntu

# Build OxiDex in release mode
cargo build --release

# Run comparative benchmark script
./benches/exiftool_comparison.sh

# View results
cat benches/benchmark_results.md
```

**Sample output:**

```
Benchmark 1: Single JPEG Read
  Perl ExifTool:  116.5ms ± 15.6ms
  OxiDex:         31.8ms ± 14.1ms
  Speedup:        3.7x faster

Benchmark 2: Batch Processing (1000 files)
  Perl ExifTool:  1911.4ms ± 171.9ms
  OxiDex:         197.6ms ± 3.1ms
  Speedup:        9.7x faster
```

## Benchmark Scenarios

### 1. Single File Extraction

**What it measures:** Time to extract all metadata from a single JPEG file

**Why it matters:** Represents typical single-file workflow

```bash
cargo bench single_extraction
```

### 2. Batch Processing

**What it measures:** Time to process 100 JPEG files in parallel

**Why it matters:** Tests parallel processing efficiency and scalability

```bash
cargo bench batch_100_jpegs
```

### 3. Format Detection

**What it measures:** Time to identify file format via magic byte detection

**Why it matters:** Critical path for file type identification

```bash
cargo bench format_detection
```

### 4. Format Comparison

**What it measures:** Performance across different file formats (JPEG, PNG, TIFF, RAW)

**Why it matters:** Ensures consistent performance across supported formats

```bash
cargo bench format_comparison
```

### 5. Write Operations

**What it measures:** Time to modify metadata and write file

**Why it matters:** Tests write path performance and atomic operations

```bash
cargo bench write_metadata
```

## Performance Metrics

### Key Metrics

- **Throughput:** Files processed per second
- **Latency:** Time to process single file (p50, p95, p99)
- **Memory:** Peak memory usage during processing
- **CPU:** Core utilization during batch processing

### Performance Targets

| Scenario | Target | Current | Status |
|----------|--------|---------|--------|
| Single JPEG | < 50ms | 31.8ms | ✅ Exceeds |
| Batch 1000 files | < 500ms | 197.6ms | ✅ Exceeds |
| Write operation | < 50ms | 23.0ms | ✅ Exceeds |
| Format detection | < 20ms | 10.4ms | ✅ Exceeds |

## Profiling

For detailed profiling and optimization strategies, see [Profiling Guide](/performance/profiling).

## Contributing Benchmarks

When adding new features or optimizations:

1. **Add benchmark:** Create benchmark in `benches/` directory
2. **Run locally:** Verify benchmark runs successfully
3. **Document:** Add benchmark description to this page
4. **Submit PR:** Include benchmark results in PR description

Example benchmark:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oxidex::parser::jpeg::parse_jpeg;

fn bench_jpeg_parsing(c: &mut Criterion) {
    let data = include_bytes!("../tests/fixtures/sample.jpg");

    c.bench_function("parse_jpeg", |b| {
        b.iter(|| parse_jpeg(black_box(data)))
    });
}

criterion_group!(benches, bench_jpeg_parsing);
criterion_main!(benches);
```

## Benchmark History

Track performance improvements over time:

- **v1.0.0 → v1.1.0:** 15% improvement in batch processing
- **JPEG parser optimization:** 25% faster JPEG segment parsing
- **TIFF IFD parsing:** 30% reduction in allocations

See [Changelog](/changelog) for detailed performance improvements in each release.
