<!-- historical -->
## Historical results (ExifTool 13.36, `exiftool-rs` 0.1.0, measured 2025-10-30)

> **Historical record.** These timings compare `exiftool-rs` 0.1.0 with a bare
> `exiftool` 13.36 found on `PATH`, not the repository pin (`.exiftool-version`,
> 13.59), with `hyperfine --warmup 3` and no instrument header (no commit, no
> binary hash, no load). The "single file" is a 112-byte stub, so that row
> measured process startup, and the batch row is rayon against single-threaded
> Perl. Kept verbatim so the change is visible; not comparable to the current
> table above.

> **Path notation:** Recorded commands show checkout-relative files and executables. `benchmark-work/` is a relative presentation alias for the original temporary benchmark directory; timings, inputs and tool versions are unchanged, and no files were moved.

#### System Specifications

- **OS**: Darwin 25.0.0
- **Architecture**: arm64
- **CPU**: Apple M4
- **Cores**: 10
- **Memory**: 32GB
- **Perl ExifTool**: 13.36
- **ExifTool-RS**: 0.1.0

### Benchmark Results

#### 1. Single File Extraction (JPEG with EXIF)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `exiftool 'tests/fixtures/jpeg/simple/sample_with_exif.jpg' > /dev/null` | 37.5 ± 0.5 | 36.6 | 39.1 | 16.07 ± 0.73 |
| `'target/release/exiftool-rs' 'tests/fixtures/jpeg/simple/sample_with_exif.jpg' > /dev/null` | 2.3 ± 0.1 | 2.1 | 2.6 | 1.00 |

**Speedup**: 16.06x faster

#### 2. Batch Processing (1000+ JPEG Files)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `exiftool -r 'benchmark-work/batch_test' > /dev/null 2>&1` | 916.4 ± 8.0 | 907.4 | 925.8 | 64.94 ± 1.56 |
| `'target/release/exiftool-rs' -r 'benchmark-work/batch_test' > /dev/null 2>&1` | 14.1 ± 0.3 | 13.7 | 14.5 | 1.00 |

**Speedup**: 64.94x faster

#### 3. Write Operation (Modify EXIF Tag)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `exiftool -Artist='BenchmarkTest' -overwrite_original 'benchmark-work/write_test/test_perl.jpg' > /dev/null 2>&1` | 96.8 ± 1.3 | 95.0 | 101.3 | 13.32 ± 1.11 |
| `'target/release/exiftool-rs' -EXIF:Artist=BenchmarkTest 'benchmark-work/write_test/test_rust.jpg' > /dev/null 2>&1` | 7.3 ± 0.6 | 6.3 | 8.0 | 1.00 |

**Speedup**: 13.32x faster

#### 4. Format Detection

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `exiftool 'benchmark-work/detection_test/test.jpg' > /dev/null` | 39.3 ± 0.4 | 38.6 | 40.7 | 14.21 ± 0.62 |
| `'target/release/exiftool-rs' 'benchmark-work/detection_test/test.jpg' > /dev/null` | 2.8 ± 0.1 | 2.3 | 3.1 | 1.00 |

**Speedup**: 14.20x faster
