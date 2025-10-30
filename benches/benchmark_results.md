# ExifTool-RS Performance Benchmarks

Comparative benchmarks between ExifTool-RS (Rust) and Perl ExifTool.

## System Specifications

- **OS**: Darwin 25.0.0
- **Architecture**: arm64
- **CPU**: Apple M4
- **Cores**: 10
- **Memory**: 32GB
- **Perl ExifTool**: 13.36
- **ExifTool-RS**: 0.1.0

## Benchmark Results

### 1. Single File Extraction (JPEG with EXIF)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `exiftool '/Users/allen/Documents/git/exiftools/tests/fixtures/jpeg/simple/sample_with_exif.jpg' > /dev/null` | 38.7 ± 0.5 | 37.8 | 40.2 | 14.32 ± 2.38 |
| `'/Users/allen/Documents/git/exiftools/target/release/exiftool-rs' '/Users/allen/Documents/git/exiftools/tests/fixtures/jpeg/simple/sample_with_exif.jpg' > /dev/null` | 2.7 ± 0.4 | 2.2 | 4.2 | 1.00 |

**Speedup**: 14.32x faster

### 2. Batch Processing (1000+ JPEG Files)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `exiftool -r '/var/folders/t6/nf3m4kn14ks5kxcqqd6f4h5w0000gp/T/tmp.VgPR0RBPEU/batch_test' > /dev/null 2>&1` | 929.5 ± 6.4 | 922.9 | 938.6 | 79.18 ± 1.23 |
| `'/Users/allen/Documents/git/exiftools/target/release/exiftool-rs' -r '/var/folders/t6/nf3m4kn14ks5kxcqqd6f4h5w0000gp/T/tmp.VgPR0RBPEU/batch_test' > /dev/null 2>&1` | 11.7 ± 0.2 | 11.6 | 12.0 | 1.00 |

**Speedup**: 79.17x faster

### 3. Write Operation (Modify EXIF Tag)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `exiftool -Artist='BenchmarkTest' -overwrite_original '/var/folders/t6/nf3m4kn14ks5kxcqqd6f4h5w0000gp/T/tmp.VgPR0RBPEU/write_test/test_perl.jpg' > /dev/null 2>&1` | 98.2 ± 1.0 | 96.4 | 101.5 | 12.68 ± 0.64 |
| `'/Users/allen/Documents/git/exiftools/target/release/exiftool-rs' -EXIF:Artist=BenchmarkTest '/var/folders/t6/nf3m4kn14ks5kxcqqd6f4h5w0000gp/T/tmp.VgPR0RBPEU/write_test/test_rust.jpg' > /dev/null 2>&1` | 7.7 ± 0.4 | 6.8 | 8.6 | 1.00 |

**Speedup**: 12.68x faster

### 4. Format Detection

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `exiftool '/var/folders/t6/nf3m4kn14ks5kxcqqd6f4h5w0000gp/T/tmp.VgPR0RBPEU/detection_test/test.jpg' > /dev/null` | 39.7 ± 0.4 | 38.8 | 41.0 | 15.05 ± 0.67 |
| `'/Users/allen/Documents/git/exiftools/target/release/exiftool-rs' '/var/folders/t6/nf3m4kn14ks5kxcqqd6f4h5w0000gp/T/tmp.VgPR0RBPEU/detection_test/test.jpg' > /dev/null` | 2.6 ± 0.1 | 2.4 | 3.0 | 1.00 |

**Speedup**: 15.05x faster

## Interpretation

ExifTool-RS demonstrates significant performance improvements over Perl ExifTool across all tested scenarios:

1. **Single File Extraction**: Rust's zero-cost abstractions and efficient memory management eliminate interpreter overhead.
2. **Batch Processing**: Parallel processing with Rayon provides substantial speedup when processing multiple files.
3. **Write Operations**: Efficient binary manipulation and atomic file operations improve write performance.
4. **Format Detection**: Simple magic byte detection showcases the performance benefits of compiled code vs. interpreted Perl.

## Reproducing These Benchmarks

To reproduce these benchmarks on your system:

```bash
# 1. Ensure prerequisites are installed
brew install hyperfine exiftool  # macOS
# or
sudo apt install hyperfine libimage-exiftool-perl  # Ubuntu

# 2. Build ExifTool-RS in release mode
cargo build --release

# 3. Run the benchmark suite
./benches/exiftool_comparison.sh

# 4. View results
cat benches/benchmark_results.md
```

**Note**: Results will vary based on your hardware, OS, and system load. For consistent results, close unnecessary applications and ensure the system is not thermal throttling.

## Additional Benchmarks

For library-level micro-benchmarks (format detection, JPEG parsing, TIFF parsing, etc.), run:

```bash
cargo bench
```

Results will be generated in `target/criterion/` as HTML reports.
