# Integration Test Infrastructure Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement comprehensive integration testing infrastructure comparing ExifTool-RS against Perl ExifTool with automated baseline generation, error handling tests, performance benchmarks, and CI regression detection.

**Architecture:** Multi-phase approach covering infrastructure setup (Git LFS, CI), test corpus validation, test implementation (comparison, error handling), benchmarking (Criterion + Hyperfine), and documentation. Uses existing 102+ test fixtures with manifest tracking.

**Tech Stack:** Rust (cargo test, criterion), Git LFS, GitHub Actions, Perl ExifTool (reference implementation), hyperfine (CLI benchmarking)

---

## Task 1: Git LFS Configuration

**Files:**
- Modify: `.gitattributes`

**Step 1: Read existing .gitattributes**

Run: `cat .gitattributes`

**Step 2: Add AVIF and binary test output tracking**

```gitattributes
# Existing content...
tests/fixtures/**/*.avif filter=lfs diff=lfs merge=lfs -text

# Test output binaries
tests/fixtures/**/*.bin filter=lfs diff=lfs merge=lfs -text
```

**Step 3: Verify Git LFS is installed and tracking**

Run:
```bash
git lfs version
git lfs ls-files | head -10
```

Expected: Version output and list of tracked files

**Step 4: Commit**

```bash
git add .gitattributes
git commit -m "feat: add AVIF and .bin LFS tracking for test fixtures"
```

---

## Task 2: Create Baseline Generation Tool

**Files:**
- Create: `src/bin/generate_baseline.rs`

**Step 1: Create binary directory structure**

Run: `mkdir -p src/bin`

**Step 2: Write baseline generation tool**

```rust
//! Baseline Generation Tool for ExifTool-RS Integration Tests
//!
//! Generates baseline metadata outputs by executing both Perl ExifTool
//! and ExifTool-RS on all test fixtures, comparing outputs, and creating
//! baseline_metadata.json with match rates and discrepancies.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
struct BaselineMetadata {
    version: String,
    exiftool_version: String,
    exiftool_rs_version: String,
    generated_at: String,
    images: Vec<ImageBaseline>,
    overall_match_rate: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageBaseline {
    path: String,
    perl_tags: usize,
    rust_tags: usize,
    match_rate: f64,
    discrepancies: Vec<Discrepancy>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Discrepancy {
    tag: String,
    perl_value: String,
    rust_value: String,
    reason: Option<String>,
}

fn is_exiftool_available() -> bool {
    Command::new("exiftool")
        .arg("-ver")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn get_exiftool_version() -> Result<String, String> {
    let output = Command::new("exiftool")
        .arg("-ver")
        .output()
        .map_err(|e| format!("Failed to get ExifTool version: {}", e))?;

    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("Invalid UTF-8: {}", e))
}

fn get_perl_exiftool_output(file_path: &Path) -> Result<String, String> {
    let output = Command::new("exiftool")
        .arg("-json")
        .arg("-a")
        .arg("-G1")
        .arg("-struct")
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to execute Perl ExifTool: {}", e))?;

    if !output.status.success() {
        return Err(format!("Perl ExifTool failed on {:?}", file_path));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8: {}", e))
}

fn get_exiftool_rs_output(file_path: &Path) -> Result<String, String> {
    let cargo_target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| "target".to_string());

    let binary_path = PathBuf::from(&cargo_target_dir)
        .join("release")
        .join("exiftool-rs");

    if !binary_path.exists() {
        return Err(format!(
            "ExifTool-RS binary not found at {:?}. Run 'cargo build --release' first.",
            binary_path
        ));
    }

    let output = Command::new(&binary_path)
        .arg("--json")
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to execute ExifTool-RS: {}", e))?;

    if !output.status.success() {
        return Err(format!("ExifTool-RS failed on {:?}", file_path));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8: {}", e))
}

fn should_skip_tag(tag_name: &str) -> bool {
    tag_name.starts_with("System:")
        || tag_name.starts_with("File:")
        || tag_name.starts_with("ExifTool:")
        || tag_name.starts_with("Composite:")
        || tag_name == "SourceFile"
}

fn values_match(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(s1), Value::String(s2)) => s1 == s2,
        (Value::Number(n1), Value::Number(n2)) => {
            if let (Some(i1), Some(i2)) = (n1.as_i64(), n2.as_i64()) {
                i1 == i2
            } else if let (Some(f1), Some(f2)) = (n1.as_f64(), n2.as_f64()) {
                (f1 - f2).abs() < 0.0001
            } else {
                false
            }
        }
        (Value::Bool(b1), Value::Bool(b2)) => b1 == b2,
        (Value::Array(a1), Value::Array(a2)) => {
            a1.len() == a2.len() && a1.iter().zip(a2.iter()).all(|(v1, v2)| values_match(v1, v2))
        }
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

fn compare_outputs(perl_json: &str, rust_json: &str) -> Result<(usize, usize, f64, Vec<Discrepancy>), String> {
    let perl_data: Vec<HashMap<String, Value>> = serde_json::from_str(perl_json)
        .map_err(|e| format!("Failed to parse Perl JSON: {}", e))?;

    let rust_data: Vec<HashMap<String, Value>> = serde_json::from_str(rust_json)
        .map_err(|e| format!("Failed to parse Rust JSON: {}", e))?;

    if perl_data.is_empty() || rust_data.is_empty() {
        return Ok((0, 0, 0.0, Vec::new()));
    }

    let perl_tags = &perl_data[0];
    let rust_tags = &rust_data[0];

    let perl_filtered: HashMap<_, _> = perl_tags
        .iter()
        .filter(|(k, _)| !should_skip_tag(k))
        .collect();

    let rust_filtered: HashMap<_, _> = rust_tags
        .iter()
        .filter(|(k, _)| !should_skip_tag(k))
        .collect();

    let mut matched = 0;
    let mut discrepancies = Vec::new();

    for (key, perl_value) in &perl_filtered {
        if let Some(rust_value) = rust_filtered.get(key) {
            if values_match(perl_value, rust_value) {
                matched += 1;
            } else {
                discrepancies.push(Discrepancy {
                    tag: (*key).to_string(),
                    perl_value: format!("{:?}", perl_value),
                    rust_value: format!("{:?}", rust_value),
                    reason: None,
                });
            }
        } else {
            discrepancies.push(Discrepancy {
                tag: (*key).to_string(),
                perl_value: format!("{:?}", perl_value),
                rust_value: "MISSING".to_string(),
                reason: None,
            });
        }
    }

    let total = perl_filtered.len();
    let match_rate = if total > 0 {
        (matched as f64 / total as f64) * 100.0
    } else {
        100.0
    };

    Ok((total, matched, match_rate, discrepancies))
}

fn find_test_images(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut images = Vec::new();
    let extensions = ["jpg", "jpeg", "png", "tif", "tiff", "pdf", "mp4"];

    fn visit_dirs(dir: &Path, images: &mut Vec<PathBuf>, extensions: &[&str]) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    visit_dirs(&path, images, extensions)?;
                } else if let Some(ext) = path.extension() {
                    if extensions.contains(&ext.to_str().unwrap_or("").to_lowercase().as_str()) {
                        images.push(path);
                    }
                }
            }
        }
        Ok(())
    }

    visit_dirs(dir, &mut images, &extensions)
        .map_err(|e| format!("Failed to traverse directory: {}", e))?;

    Ok(images)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("ExifTool-RS Baseline Generation Tool");
    println!("====================================\n");

    if !is_exiftool_available() {
        eprintln!("ERROR: Perl ExifTool not found in PATH");
        std::process::exit(1);
    }

    let exiftool_version = get_exiftool_version()?;
    println!("Perl ExifTool version: {}", exiftool_version);

    let input_dir = PathBuf::from("tests/fixtures");
    let output_dir = PathBuf::from("tests/baselines");

    fs::create_dir_all(&output_dir)?;

    println!("\nScanning for test images...");
    let test_images = find_test_images(&input_dir)?;
    println!("Found {} test images", test_images.len());

    let mut image_baselines = Vec::new();
    let mut total_match_rate = 0.0;

    for (idx, image_path) in test_images.iter().enumerate() {
        let relative_path = image_path.strip_prefix(&input_dir)
            .unwrap_or(image_path)
            .to_string_lossy()
            .to_string();

        print!("[{}/{}] Processing: {} ... ", idx + 1, test_images.len(), relative_path);

        match (get_perl_exiftool_output(image_path), get_exiftool_rs_output(image_path)) {
            (Ok(perl_json), Ok(rust_json)) => {
                match compare_outputs(&perl_json, &rust_json) {
                    Ok((perl_tags, _, match_rate, discrepancies)) => {
                        println!("{:.1}%", match_rate);
                        total_match_rate += match_rate;

                        let rust_data: Vec<HashMap<String, Value>> = serde_json::from_str(&rust_json)?;
                        let rust_tags = rust_data.get(0).map(|m| m.len()).unwrap_or(0);

                        image_baselines.push(ImageBaseline {
                            path: relative_path,
                            perl_tags,
                            rust_tags,
                            match_rate,
                            discrepancies,
                        });
                    }
                    Err(e) => println!("FAILED: {}", e),
                }
            }
            (Err(e), _) | (_, Err(e)) => println!("FAILED: {}", e),
        }
    }

    let overall_match_rate = if !image_baselines.is_empty() {
        total_match_rate / image_baselines.len() as f64
    } else {
        0.0
    };

    let baseline = BaselineMetadata {
        version: "1.0.0".to_string(),
        exiftool_version,
        exiftool_rs_version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        images: image_baselines,
        overall_match_rate,
    };

    let metadata_path = output_dir.join("baseline_metadata.json");
    let metadata_file = File::create(&metadata_path)?;
    serde_json::to_writer_pretty(metadata_file, &baseline)?;

    println!("\n====================================");
    println!("Overall match rate: {:.2}%", overall_match_rate);
    println!("Baseline metadata: {}", metadata_path.display());
    println!("====================================");

    Ok(())
}
```

**Step 3: Verify the binary compiles**

Run: `cargo build --bin generate_baseline`

Expected: Successful compilation

**Step 4: Commit**

```bash
git add src/bin/generate_baseline.rs
git commit -m "feat: add baseline generation tool for integration tests"
```

---

## Task 3: Create Error Handling Tests

**Files:**
- Create: `tests/integration/error_handling_tests.rs`

**Step 1: Write error handling test module**

```rust
//! Error Handling Integration Tests
//!
//! Validates graceful degradation for invalid inputs per integration test plan.

use exiftool_rs::io::buffered_reader::BufferedReader;
use exiftool_rs::parsers::format_detector::detect_format;
use exiftool_rs::parsers::tiff::file_parser::parse_tiff_file;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

fn with_timeout<F, T>(timeout: Duration, f: F) -> Result<T, String>
where
    F: FnOnce() -> T,
{
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();

    if elapsed > timeout {
        Err(format!("Operation took {:?}, exceeding timeout of {:?}", elapsed, timeout))
    } else {
        Ok(result)
    }
}

#[test]
fn test_error_missing_file() {
    let nonexistent_path = Path::new("tests/fixtures/nonexistent.jpg");
    let result = BufferedReader::new(nonexistent_path);

    assert!(result.is_err(), "Expected error for missing file");

    if let Err(e) = result {
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }
}

#[test]
fn test_error_truncated_tiff() {
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    let truncated_tiff = vec![
        b'I', b'I',     // Little-endian
        0x2A, 0x00,     // Magic number
        0x08, 0x00, 0x00, 0x00,  // IFD offset: 8
        // Truncated: missing IFD data
    ];

    fs::write(temp_path, truncated_tiff).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");
    let result = parse_tiff_file(&reader);

    assert!(result.is_err(), "Expected error for truncated TIFF");
}

#[test]
fn test_error_circular_ifd_reference() {
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    let mut circular_tiff = vec![
        b'I', b'I',
        0x2A, 0x00,
        0x08, 0x00, 0x00, 0x00,
    ];

    circular_tiff.extend_from_slice(&[
        0x01, 0x00,  // Tag count: 1
    ]);

    circular_tiff.extend_from_slice(&[
        0x00, 0x01,  // Tag: ImageWidth
        0x03, 0x00,  // Type: SHORT
        0x01, 0x00, 0x00, 0x00,
        0x40, 0x00, 0x00, 0x00,
    ]);

    circular_tiff.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]);  // Points back to self

    fs::write(temp_path, circular_tiff).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");
    let result = with_timeout(Duration::from_secs(5), || parse_tiff_file(&reader));

    match result {
        Ok(Ok(_)) => {
            // Parser handled gracefully
        }
        Ok(Err(_)) => {
            // Detected circular reference
        }
        Err(timeout_msg) => {
            panic!("Parser should detect circular references: {}", timeout_msg);
        }
    }
}

#[test]
fn test_no_panic_on_random_data() {
    use std::fs;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path();

    let random_data: Vec<u8> = (0..1000)
        .map(|i| ((i * 37 + 91) % 256) as u8)
        .collect();

    fs::write(temp_path, random_data).expect("Failed to write temp file");

    let reader = BufferedReader::new(temp_path).expect("Failed to open temp file");

    let _ = detect_format(&reader);
    let _ = parse_tiff_file(&reader);

    println!("✓ Parser handled random data without panicking");
}
```

**Step 2: Verify tests compile**

Run: `cargo test --test integration --no-run`

Expected: Successful compilation

**Step 3: Run error handling tests**

Run: `cargo test --test integration error_handling -- --nocapture`

Expected: Tests pass with graceful error handling

**Step 4: Commit**

```bash
git add tests/integration/error_handling_tests.rs
git commit -m "test: add comprehensive error handling integration tests"
```

---

## Task 4: Create Integration Benchmarks

**Files:**
- Create: `benches/integration_benchmarks.rs`
- Modify: `Cargo.toml`

**Step 1: Write integration benchmark suite**

```rust
//! Integration Performance Benchmarks
//!
//! End-to-end performance tests per integration test plan Section 6.4

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use exiftool_rs::core::operations::read_metadata;
use std::path::Path;

fn bench_single_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_extraction");

    let test_files = [
        ("jpeg_simple", "tests/fixtures/jpeg/simple/sample_with_exif.jpg"),
        ("png_simple", "tests/fixtures/png/simple/synthetic_text_001.png"),
        ("tiff_simple", "tests/fixtures/tiff/simple/sample.tif"),
    ];

    for (name, path) in test_files.iter() {
        if Path::new(path).exists() {
            group.bench_with_input(BenchmarkId::from_parameter(name), path, |b, path| {
                b.iter(|| {
                    black_box(read_metadata(Path::new(path)).expect("Metadata extraction failed"))
                });
            });
        }
    }

    group.finish();
}

fn bench_batch_processing(c: &mut Criterion) {
    use std::fs;

    c.bench_function("batch_100_jpegs", |b| {
        let mut jpeg_files = Vec::new();

        if let Ok(entries) = fs::read_dir("tests/fixtures/jpeg/simple") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("jpg") {
                    jpeg_files.push(path);
                    if jpeg_files.len() >= 100 {
                        break;
                    }
                }
            }
        }

        b.iter(|| {
            for file_path in &jpeg_files {
                black_box(read_metadata(file_path).ok());
            }
        });
    });
}

criterion_group!(
    benches,
    bench_single_extraction,
    bench_batch_processing
);

criterion_main!(benches);
```

**Step 2: Add benchmark to Cargo.toml**

```toml
[[bench]]
name = "integration_benchmarks"
harness = false
```

**Step 3: Verify benchmarks compile**

Run: `cargo bench --bench integration_benchmarks --no-run`

Expected: Successful compilation

**Step 4: Commit**

```bash
git add benches/integration_benchmarks.rs Cargo.toml
git commit -m "bench: add integration performance benchmarks"
```

---

## Task 5: Add CI Benchmark Regression Detection

**Files:**
- Modify: `.github/workflows/ci.yml`

**Step 1: Read existing CI workflow**

Run: `cat .github/workflows/ci.yml | grep -A 20 "integration-tests:"`

**Step 2: Add benchmark job after integration-tests**

```yaml
  benchmarks:
    name: Performance Benchmarks
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - name: Checkout repository
        uses: actions/checkout@v4
        with:
          lfs: true

      - name: Setup Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Setup Rust cache
        uses: Swatinem/rust-cache@v2

      - name: Run benchmarks
        run: cargo bench --bench integration_benchmarks --bench parse_benchmarks

      - name: Store benchmark results
        uses: benchmark-action/github-action-benchmark@v1
        if: github.ref == 'refs/heads/main'
        with:
          tool: 'cargo'
          output-file-path: target/criterion/*/new/estimates.json
          github-token: ${{ secrets.GITHUB_TOKEN }}
          alert-threshold: '110%'
          comment-on-alert: true
          fail-on-alert: false

      - name: Upload benchmark results
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: target/criterion/
          retention-days: 90
```

**Step 3: Validate YAML syntax**

Run: `yamllint .github/workflows/ci.yml` (or check GitHub Actions UI)

**Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add benchmark regression detection with 10% threshold"
```

---

## Task 6: Create Test Failure Triage Documentation

**Files:**
- Create: `docs/testing/TEST_FAILURE_TRIAGE.md`

**Step 1: Write triage process document**

```markdown
# Test Failure Triage Process

Systematic process for investigating and resolving test failures.

## Quick Reference

| Failure Type | First Step |
|--------------|------------|
| Match rate < 99% | Check KNOWN_DISCREPANCIES.md |
| Benchmark regression | Check Criterion reports |
| Error handling test | Check logs for panics |

## Triage Workflow

### Step 1: Identify Failure Type

```bash
# Run integration tests
cargo test --release --features exiftool-comparison

# Run benchmarks
cargo bench
```

### Step 2: Gather Context

#### For Match Rate Failures

```bash
# Get detailed comparison
exiftool -json -a -G1 tests/fixtures/failing/image.jpg > perl.json
target/release/exiftool-rs --json tests/fixtures/failing/image.jpg > rust.json

# Visual diff
diff -u perl.json rust.json
```

#### For Benchmark Regressions

```bash
# View Criterion reports
open target/criterion/report/index.html

# Compare with baseline
cargo bench -- --baseline main
```

### Step 3: Categorize and Fix

**Acceptable Discrepancy** → Document in KNOWN_DISCREPANCIES.md
**Bug** → File issue, implement fix, update baseline
**Regression** → Profile code, optimize, verify improvement

## Common Issues

### "Match rate 97.5% below 99% threshold"

```bash
# Find missing tags
comm -23 perl_tags.txt rust_tags.txt
```

**Solutions:**
- Maker notes not implemented → Document
- Tag parsing bug → Fix
- Namespace mismatch → Update normalization

### "Benchmark 150% slower"

```bash
# Profile and identify bottleneck
cargo bench -- --profile-time=10
```

**Solutions:**
- O(n²) algorithm → Optimize to O(n log n)
- Memory allocations → Use stack or buffer pool

## References

- Integration Test Plan: `docs/testing/integration_test_plan.md`
- Known Discrepancies: `tests/integration/KNOWN_DISCREPANCIES.md`
```

**Step 2: Commit**

```bash
git add docs/testing/TEST_FAILURE_TRIAGE.md
git commit -m "docs: add test failure triage process"
```

---

## Task 7: Verify All Tests Pass

**Step 1: Run full test suite**

Run: `cargo test --all-features`

Expected: All tests pass (excluding pre-existing tag_database failures)

**Step 2: Verify integration tests specifically**

Run: `cargo test --test integration`

Expected: 108+ tests passed, 0 failed

**Step 3: Build all benchmarks**

Run: `cargo bench --no-run`

Expected: Successful compilation

**Step 4: Verify baseline tool**

Run: `cargo build --bin generate_baseline`

Expected: Successful compilation

---

## Task 8: Create Pull Request

**Step 1: Review git status**

Run: `git status`

Expected: Only integration test infrastructure files modified/added

**Step 2: Create comprehensive commit (if not done incrementally)**

```bash
git add -A
git commit -m "feat: implement comprehensive integration test infrastructure

Implements complete integration testing strategy covering:
- Baseline generation tool
- Error handling tests
- Integration benchmarks
- CI regression detection
- Triage documentation

🤖 Generated with Claude Code
Co-Authored-By: Claude <noreply@anthropic.com>"
```

**Step 3: Push branch**

Run: `git push -u origin integration-test-infrastructure`

**Step 4: Create PR**

Run:
```bash
gh pr create --title "feat: implement comprehensive integration test infrastructure" --body "
## Summary
Complete integration testing infrastructure per integration_test_plan.md

## Test Results
- 108+ integration tests passing
- Error handling tests validate graceful degradation
- Benchmarks ready for baseline establishment

## Test Plan
- [x] cargo test --all-features
- [x] cargo test --test integration
- [x] cargo bench --no-run

🤖 Generated with Claude Code"
```

---

## Verification Checklist

After completing all tasks:

- [ ] Git LFS tracking AVIF and .bin files
- [ ] Baseline generation tool compiles and runs
- [ ] Error handling tests pass without panics
- [ ] Integration benchmarks compile
- [ ] CI workflow includes benchmark job
- [ ] Triage documentation complete
- [ ] All integration tests pass
- [ ] PR created successfully

## Notes for Engineer

**Testing Strategy:**
- Follow TDD: Write test → See it fail → Implement → See it pass → Commit
- Run tests frequently during development
- Use `cargo test -- --nocapture` to see print output
- Use `cargo test test_name` to run specific tests

**Common Pitfalls:**
- Forgetting to build release binary before running baseline tool
- Not having Perl ExifTool installed for comparison tests
- Skipping timeout protection in error handling tests
- Not verifying YAML syntax in CI workflow changes

**Performance Tips:**
- Use `black_box()` in benchmarks to prevent compiler optimization
- Keep benchmark iterations consistent for fair comparison
- Profile before optimizing - don't guess where slowness is

**Skills Referenced:**
- @superpowers:verification-before-completion - Always verify tests pass before claiming done
- @superpowers:test-driven-development - Write test first, see it fail, then implement
