# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T10",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Expand benchmark suite from I2.T11 to compare performance against Perl ExifTool. Benchmark scenarios: (1) Single file extraction (JPEG with EXIF), (2) Batch processing (1000 JPEGs), (3) Write operation (modify EXIF tag), (4) Format detection overhead. Run both ExifTool and ExifTool-RS, measure wall-clock time and memory usage. Use hyperfine for CLI benchmarking, criterion for library benchmarking. Document results in README with comparison table. Target: demonstrate 2-5x speedup for typical operations.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I2.T11 benchmark suite, all implemented features",
  "target_files": [
    "benches/parse_benchmarks.rs",
    "benches/exiftool_comparison.sh",
    "README.md"
  ],
  "input_files": [
    "benches/parse_benchmarks.rs"
  ],
  "deliverables": "Comparative benchmarks, performance documentation",
  "acceptance_criteria": "Benchmarks compare ExifTool-RS vs. Perl ExifTool on same machine, at least 4 benchmark scenarios (single read, batch read, write, detection), results show wall-clock time and memory usage, ExifTool-RS achieves 2x+ speedup for at least 2 scenarios, results documented in README with table, benchmarks reproducible (documented setup, test corpus)",
  "dependencies": [],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: nfr-performance (from 01_Context_and_Drivers.md)

```markdown
#### Performance
- **Target**: 2-5x faster than Perl ExifTool for typical operations
- **Justification**: Rust's zero-cost abstractions, elimination of interpreter overhead, and potential for SIMD optimization
- **Measurement**: Benchmark suite comparing against ExifTool on 1000-file corpus
- **Design Impact**: Zero-copy parsing strategies, memory-mapped I/O for large files, parallel batch processing
```

### Context: benchmarking (from 03_Verification_and_Glossary.md)

```markdown
#### Benchmarking (Regression Detection)
*   **Scope:** Performance validation and regression detection
*   **Location:** `benches/`
*   **Tools:** `criterion` (statistical benchmarking), `hyperfine` (CLI benchmarking)
*   **Benchmarks:**
    *   Format detection (1000 iterations)
    *   JPEG EXIF extraction (single file, 1000x)
    *   Batch processing (1000 files)
    *   Write operation (modify + rewrite)
    *   Comparison vs. Perl ExifTool (wall-clock time, memory usage)
*   **Regression Detection:** CI fails if performance degrades >10% vs. baseline
*   **Reporting:** `criterion` generates HTML reports in `target/criterion/`
```

### Context: release-criteria (from 03_Verification_and_Glossary.md)

```markdown
### 5.5. Release Criteria (v1.0)

The v1.0 release is approved when all of the following are met:

1. **Feature Completeness:**
   *   ✅ Core read/write operations for JPEG, TIFF, PNG, PDF, MP4
   *   ✅ CLI with ExifTool-compatible arguments for common use cases
   *   ✅ Rust library API with comprehensive documentation
   *   ✅ C FFI bindings with auto-generated header
   *   ✅ Batch processing with parallel execution
   *   ✅ 500+ tag support across EXIF, XMP, IPTC, GPS, PDF, QuickTime
   *   ✅ Metadata operations: read, write, copy, rename, date shift

2. **Quality Metrics:**
   *   ✅ 80%+ code coverage
   *   ✅ 98%+ tag parity with ExifTool (comparison tests)
   *   ✅ 2x+ performance vs. ExifTool (benchmark validation) ← THIS TASK
   *   ✅ Zero crashes in 24-hour fuzz testing
   *   ✅ Zero clippy warnings
   *   ✅ Zero critical/high severity vulnerabilities (cargo audit)
```

### Context: glossary-performance-tools (from 03_Verification_and_Glossary.md)

```markdown
| **Corpus** | Collection of test inputs for fuzzing or regression testing. Seed corpus: known-good inputs. Crash corpus: inputs that triggered bugs. |
| **Coverage-Guided Fuzzing** | Fuzzing technique that uses code coverage feedback to guide input mutation. Prioritizes inputs that explore new code paths. libFuzzer and AFL use this approach. |
```

**Note on hyperfine**: The planning documents reference hyperfine as the CLI benchmarking tool. Hyperfine is a command-line benchmarking tool that:
- Measures wall-clock time with statistical significance
- Performs warmup runs
- Supports multiple runs for accurate results
- Outputs in various formats (markdown, JSON, etc.)
- Can compare multiple commands side-by-side

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `benches/parse_benchmarks.rs`
    *   **Summary:** This file contains the existing Criterion-based benchmark suite created in I2.T11. It includes 4 benchmark functions:
        1. `bench_format_detection` - Benchmarks magic byte detection using JPEG fixture
        2. `bench_jpeg_segment_parsing` - Benchmarks JPEG segment structure parsing
        3. `bench_tiff_ifd_parsing` - Benchmarks TIFF IFD parsing (extracts IFD from JPEG EXIF)
        4. `bench_full_read_metadata` - End-to-end metadata extraction benchmark
    *   **Recommendation:** You MUST extend this file to add comparative benchmarks. Consider adding a new function `bench_exiftool_comparison()` that runs both tools and collects timing data. However, note that Criterion is designed for micro-benchmarks, not CLI tool comparison. You SHOULD create a separate shell script for CLI comparisons using hyperfine.
    *   **Key Details:**
        - Uses `criterion::black_box()` to prevent compiler optimizations
        - Uses test fixture at `tests/fixtures/jpeg/sample_with_exif.jpg`
        - Includes helper `TiffSubReader` struct for offset-adjusted reading
        - Configured in Cargo.toml with `harness = false`

*   **File:** `tests/fixtures/`
    *   **Summary:** Test corpus directory containing 102+ test files across 5 formats (JPEG: 30, PNG: 33, TIFF: 20, PDF: 10, MP4: 9). This is the result of I5.T9 implementation.
    *   **Recommendation:** You MUST use this test corpus for batch benchmarking. The fixtures are organized by format and complexity (simple/complex/edge_cases). For the "1000 JPEGs" batch benchmark scenario, you may need to either:
        1. Create a script that replicates the JPEG fixtures to reach 1000 files, OR
        2. Adjust the benchmark description to use the actual corpus size (30 JPEGs) and extrapolate
    *   **Key Details:**
        - Fixtures tracked by Git LFS (see `.gitattributes`)
        - Manifest at `tests/fixtures/manifest.json` documents metadata
        - Script `tests/fixtures/create_synthetic_fixtures.sh` shows how fixtures were generated
        - Most fixtures are synthetic (95%) with known metadata

*   **File:** `Cargo.toml`
    *   **Summary:** Project configuration file. Already includes `criterion = "0.5"` in `[dev-dependencies]` and has `[[bench]]` configuration for `parse_benchmarks`.
    *   **Recommendation:** You DO NOT need to add Criterion as it's already configured. However, you SHOULD document that hyperfine needs to be installed separately: `cargo install hyperfine` or via system package manager.
    *   **Key Details:**
        - Release profile has aggressive optimizations: `opt-level = 3`, `lto = true`, `codegen-units = 1`
        - Binary name is `exiftool-rs` (from `[[bin]]` section)
        - Feature flag `exiftool-comparison` exists for integration tests

*   **File:** `src/core/operations.rs`
    *   **Summary:** Core operations module providing `read_metadata()`, `write_metadata()`, and related functions. This is what the benchmarks will be measuring.
    *   **Recommendation:** You SHOULD benchmark both `read_metadata()` (via Criterion for library-level benchmarking) and the CLI tool `exiftool-rs` (via hyperfine for CLI benchmarking). The CLI overhead should be measured separately.
    *   **Key Functions:**
        - `read_metadata(path: &Path) -> Result<MetadataMap>` - Main read function
        - `write_metadata(path: &Path, metadata: &MetadataMap) -> Result<()>` - Main write function
        - Format-specific internal functions: `parse_jpeg_metadata()`, `parse_tiff_metadata()`, etc.

*   **File:** `README.md`
    *   **Summary:** Project README with installation, usage, and status information. Currently has placeholders for features and no performance documentation.
    *   **Recommendation:** You MUST add a new section (e.g., "## Performance Benchmarks") documenting the comparative results. Include:
        1. A comparison table showing ExifTool-RS vs Perl ExifTool timings
        2. System specs where benchmarks were run
        3. Instructions for reproducing the benchmarks
        4. Interpretation of results (speedup factors)
    *   **Current Sections:** Project Vision, Key Features, Architecture, Current Status, Installation (multiple methods), Usage, Contributing

*   **File:** `.github/workflows/ci.yml`
    *   **Summary:** GitHub Actions CI workflow. Already has test, audit, coverage, and integration-tests jobs. Does NOT have benchmark regression detection yet.
    *   **Recommendation:** Based on the planning doc requirement "CI fails if performance degrades >10% vs. baseline", you SHOULD consider adding a benchmark job to the CI workflow. However, this may be out of scope for I5.T10 if the task focuses only on establishing baseline benchmarks. You can document this as a future enhancement.
    *   **Note:** The planning doc mentions benchmark regression detection in CI, but implementing this requires:
        1. Storing baseline results (e.g., in git or GitHub Pages)
        2. Running benchmarks on every PR
        3. Comparing against baseline with tolerance threshold
        4. This is complex and may warrant a separate task post-v1.0

### Implementation Tips & Notes

*   **Tip: hyperfine Installation**
    - hyperfine is NOT currently installed on the development system (confirmed via `which hyperfine`)
    - You SHOULD document installation instructions in the benchmark script:
        - macOS: `brew install hyperfine`
        - Ubuntu: `cargo install hyperfine` or `sudo apt install hyperfine` (if available)
        - Windows: `cargo install hyperfine` or `choco install hyperfine`
    - The benchmark script SHOULD check if hyperfine is installed and provide helpful error message if not

*   **Tip: Perl ExifTool Availability**
    - Perl ExifTool IS installed on the current system (`/opt/homebrew/bin/exiftool`, version 13.36)
    - Your benchmark script SHOULD check for ExifTool availability: `which exiftool`
    - You SHOULD capture the ExifTool version in the benchmark report for reproducibility

*   **Tip: Benchmark Scenarios**
    The task description specifies 4 scenarios. Here's how to implement each:
    1. **Single file extraction (JPEG with EXIF)**
        - Use hyperfine to compare: `exiftool sample.jpg` vs `exiftool-rs sample.jpg`
        - Use a representative JPEG from `tests/fixtures/jpeg/simple/`
    2. **Batch processing (1000 JPEGs)**
        - Challenge: We only have 30 JPEG fixtures
        - Solution: Create a temporary directory with replicated fixtures (copy fixtures 34 times to get 1020 files)
        - Compare: `exiftool -r temp_dir/` vs `exiftool-rs -r temp_dir/`
    3. **Write operation (modify EXIF tag)**
        - Compare: `exiftool -Artist="Test" copy.jpg` vs `exiftool-rs -EXIF:Artist=Test copy.jpg`
        - Use a temporary copy of a fixture to avoid modifying originals
    4. **Format detection overhead**
        - This is library-level, use Criterion (already exists in parse_benchmarks.rs as `bench_format_detection`)
        - For CLI comparison, you could measure time to just detect format without extracting all tags (if such a mode exists)

*   **Tip: Memory Usage Measurement**
    - hyperfine doesn't measure memory usage by default
    - On Unix systems, you can use `/usr/bin/time -v` for memory stats
    - On macOS, use `/usr/bin/time -l` (different format than GNU time)
    - Consider creating a wrapper script that captures both time and memory
    - Alternatively, document memory measurement as a manual step using `htop` or Activity Monitor

*   **Note: Build Requirements for Fair Comparison**
    - You MUST build ExifTool-RS in release mode before benchmarking: `cargo build --release`
    - The binary will be at `target/release/exiftool-rs`
    - Ensure the build uses the optimized profile defined in Cargo.toml (LTO enabled)

*   **Note: Benchmark Result Variability**
    - hyperfine performs multiple runs and warmup automatically
    - Criterion uses statistical methods to detect outliers
    - Results can vary based on: CPU frequency scaling, background processes, disk cache state
    - You SHOULD document system state recommendations (close other apps, ensure CPU is not thermal throttling, etc.)

*   **Note: README Documentation Format**
    - The README.md currently uses markdown tables and has a clear section structure
    - You SHOULD add the benchmark results as a new section after "## Current Status"
    - Use a markdown table format like this:
      ```markdown
      ## Performance Benchmarks

      | Scenario | ExifTool (Perl) | ExifTool-RS (Rust) | Speedup |
      |----------|-----------------|--------------------|---------|
      | Single JPEG read | 45ms | 18ms | 2.5x faster |
      | ... | ... | ... | ... |
      ```
    - Include a footnote with system specs: CPU, RAM, OS, ExifTool version, ExifTool-RS version

*   **Warning: Batch Processing Implementation Status**
    - The task depends on "all implemented features", including batch processing (I4.T3)
    - Confirmed: Batch processor exists at `src/cli/batch_processor.rs`
    - Confirmed: `-r` flag for recursive processing is implemented (checked integration tests)
    - You CAN proceed with batch benchmarking

*   **Warning: Write Operation Implementation Status**
    - Write operations were implemented in I3 (I3.T4: write_metadata)
    - CLI write support added in I3.T5 (`-TAG=VALUE` syntax)
    - You CAN proceed with write operation benchmarking
    - Sample command: `exiftool-rs -EXIF:Artist=TestArtist photo.jpg`

### Project Conventions & Standards

*   **Shell Script Location:** Based on project structure, shell scripts typically go in a `scripts/` or `benches/` directory. The task specifies `benches/exiftool_comparison.sh`, which is appropriate.

*   **Documentation Style:** The project uses comprehensive inline documentation (see existing code). Your benchmark script SHOULD include:
    - Header comment explaining purpose
    - Usage instructions
    - Prerequisites (hyperfine, ExifTool installed)
    - Example output

*   **Error Handling:** Shell scripts in the project (e.g., `tests/fixtures/create_synthetic_fixtures.sh`) use `set -euo pipefail` for strict error handling. You SHOULD follow this convention.

*   **Output Formats:** hyperfine supports multiple output formats. You SHOULD:
    - Use markdown output for including in documentation: `--export-markdown results.md`
    - Consider JSON output for programmatic processing: `--export-json results.json`

### Performance Target Interpretation

The task states "Target: demonstrate 2-5x speedup for typical operations." Based on the architecture requirements:
- **Minimum acceptable:** 2x speedup for at least 2 scenarios (per acceptance criteria)
- **Target range:** 2-5x speedup for typical operations (from NFR)
- **Realistic expectations:**
    - Single file reads: Likely 2-3x speedup (Rust's zero-cost abstractions vs Perl interpreter overhead)
    - Batch processing: Likely 3-5x speedup (parallel processing with rayon)
    - Write operations: Likely 2-3x speedup (similar parsing overhead, atomic file operations)
    - Format detection: Likely 5-10x speedup (very simple operation, minimal interpreter overhead)

If results don't meet the 2x target, you SHOULD:
1. Document actual results honestly
2. Investigate profiling results to identify bottlenecks
3. Add notes about potential optimizations
4. Do NOT artificially inflate results or cherry-pick favorable scenarios

---

## 4. Recommended Implementation Approach

Based on my analysis, here is the recommended step-by-step approach:

### Step 1: Create the Shell Script (benches/exiftool_comparison.sh)
1. Add prerequisite checks (hyperfine, exiftool, exiftool-rs binary)
2. Create a temporary directory with replicated JPEG fixtures for batch testing
3. Define 4 hyperfine benchmark commands matching the scenarios
4. Export results to markdown format
5. Clean up temporary files
6. Make script executable: `chmod +x benches/exiftool_comparison.sh`

### Step 2: Extend Criterion Benchmarks (benches/parse_benchmarks.rs)
1. Add additional Criterion benchmarks for write operations if not already present
2. Consider adding batch processing benchmarks (measuring library-level performance)
3. Ensure all benchmarks use realistic fixtures from tests/fixtures/
4. Update documentation comments

### Step 3: Run Benchmarks and Capture Results
1. Build release binary: `cargo build --release`
2. Run Criterion benchmarks: `cargo bench`
3. Run shell script: `./benches/exiftool_comparison.sh`
4. Review results, ensure 2x+ speedup achieved in at least 2 scenarios
5. If results are below target, profile and document findings

### Step 4: Document in README.md
1. Add "## Performance Benchmarks" section after "## Current Status"
2. Create comparison table with 4 scenarios
3. Add system specifications footnote
4. Add instructions for reproducing benchmarks
5. Add interpretation/commentary on results

### Step 5: Verify Acceptance Criteria
- [x] Benchmarks compare ExifTool-RS vs. Perl ExifTool on same machine
- [x] At least 4 benchmark scenarios (single read, batch read, write, detection)
- [x] Results show wall-clock time and memory usage
- [ ] ExifTool-RS achieves 2x+ speedup for at least 2 scenarios (verify after running)
- [x] Results documented in README with table
- [x] Benchmarks reproducible (documented setup, test corpus)

---

## 5. Key Files Summary

**Files you MUST create:**
- `benches/exiftool_comparison.sh` - Shell script for CLI benchmarking with hyperfine

**Files you MUST modify:**
- `README.md` - Add performance benchmarks section with comparison table
- `benches/parse_benchmarks.rs` - (Optional) Extend with additional benchmarks

**Files you SHOULD reference:**
- `tests/fixtures/jpeg/simple/` - Source of test images for benchmarking
- `tests/fixtures/manifest.json` - Metadata about test fixtures
- `Cargo.toml` - Verify benchmark configuration
- `target/release/exiftool-rs` - Binary to benchmark (ensure built first)

**Files you can IGNORE for this task:**
- `.github/workflows/ci.yml` - CI benchmark integration is out of scope (document as future work)
- Test files in `tests/integration/` - Not needed for performance benchmarking

---

**End of Task Briefing Package**
