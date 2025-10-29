# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I2.T11",
  "iteration_id": "I2",
  "iteration_goal": "Implement tag registry with subset of ExifTool tags, core metadata read/write operations, basic CLI with argument parsing, and extend format support to include XMP parsing and PNG format.",
  "description": "Set up benchmarking infrastructure in benches/parse_benchmarks.rs using criterion crate. Create benchmarks for: (1) Format detection, (2) JPEG segment parsing, (3) TIFF IFD parsing, (4) Full read_metadata() on sample JPEG. Use Criterion::default() configuration. Run benchmarks to establish baseline performance. Add [[bench]] section to Cargo.toml. Document how to run benchmarks in README.",
  "agent_type_hint": "BackendAgent",
  "inputs": "Criterion crate documentation, I2.T3 read_metadata function",
  "target_files": [
    "benches/parse_benchmarks.rs",
    "Cargo.toml",
    "README.md"
  ],
  "input_files": [
    "src/core/operations.rs",
    "tests/fixtures/jpeg/sample_with_exif.jpg"
  ],
  "deliverables": "Criterion benchmarks for parsing operations, baseline performance measurements",
  "acceptance_criteria": "cargo bench runs successfully, at least 4 benchmarks defined (detection, JPEG parse, TIFF parse, full read), benchmarks use black_box() to prevent compiler optimization, Criterion generates HTML report in target/criterion/, README documents benchmark commands",
  "dependencies": [
    "I2.T3"
  ],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: technology-stack-summary (from 02_Architecture_Overview.md)

```markdown
### 3.2. Technology Stack Summary

| **Category** | **Technology Choice** | **Justification** |
|--------------|----------------------|-------------------|
| **Core Language** | Rust 1.75+ (2021 Edition) | Memory safety, zero-cost abstractions, excellent concurrency primitives, cross-platform support |
| **CLI Framework** | `clap` v4 (derive API) | Industry standard, excellent help generation, argument validation, backward compatibility via value parsers |
| **Binary Parsing** | `nom` v7 + `binrw` | `nom` for complex formats (TIFF, QuickTime), `binrw` for simple struct-based formats (BMP, WAV) |
| **XML Parsing (XMP)** | `quick-xml` | Streaming parser, low memory footprint, namespace support for XMP |
| **JSON Output** | `serde_json` | De facto standard, excellent performance, integration with domain models via derives |
| **Date/Time** | `chrono` | Comprehensive timezone support, EXIF date format parsing |
| **String Encoding** | `encoding_rs` (WHATWG standard) | Handles legacy encodings in IPTC/EXIF (Latin1, UTF-8, UTF-16) |
| **Image I/O** | `memmap2` (memory-mapped files) | Efficient large file access without loading entire file into memory |
| **Concurrency** | `rayon` (data parallelism) | Transparent batch processing parallelization, work-stealing scheduler |
| **Testing** | `cargo test` + `proptest` (property-based) | Unit tests for parsers, property-based testing for round-trip serialization |
| **Fuzzing** | `cargo-fuzz` (libFuzzer) | Continuous fuzzing of format parsers to discover crash/hang bugs |
| **C FFI** | `cbindgen` (header generation) | Automated C header generation from Rust API |
| **Documentation** | `rustdoc` + `mdBook` (user guide) | API docs from source comments, separate user guide for CLI |
| **Build System** | `cargo` + `cross` (cross-compilation) | Standard Rust tooling, `cross` for ARM/Windows builds from Linux |
| **CI/CD** | GitHub Actions | Free for open source, matrix builds across OS/architecture |
| **Code Quality** | `clippy`, `rustfmt`, `cargo-audit` | Linting, formatting, dependency vulnerability scanning |
| **Benchmarking** | `criterion` | Statistical benchmarking framework, regression detection |
| **Frontend** | None (CLI only) | Out of scope for v1.0 |
| **Database** | None (file-based operation) | Stateless tool, no persistent storage beyond processed files |
| **Messaging/Queues** | None | Synchronous processing model |
| **Cloud Platform** | None (local tooling) | Library/CLI distribution, not cloud service |
| **Containerization** | Optional Docker image | Convenience for CI/CD pipelines, not core requirement |
```

### Context: scalability-performance (from 05_Operational_Architecture.md)

```markdown
#### Scalability & Performance

**Scalability Strategy**:

1. **Vertical Scaling**: Parallel processing via `rayon`
   - Batch operations automatically distribute across CPU cores
   - Scales linearly up to core count for CPU-bound workloads (parsing)
   - I/O-bound workloads (large files on HDD) benefit less but still see 2-3x improvement

2. **Memory Efficiency**:
   - Streaming parsers for large files (process chunks, not entire file in RAM)
   - Memory-mapped I/O (`memmap2`) for random access without full load
   - Bounded buffers: Max 256MB per file in memory, larger files use mmap

3. **Horizontal Scaling** (for service deployments):
   - Stateless design enables trivial horizontal scaling
   - Process isolation: Each worker process handles subset of files
   - Example: GNU Parallel integration: `ls *.jpg | parallel -j 16 exiftool-rs`

**Performance Targets & Techniques**:

| **Metric** | **Target** | **Technique** |
|------------|-----------|---------------|
| JPEG EXIF extraction | < 5ms per file (average) | Zero-copy parsing, mmap for large files |
| Batch processing (1000 files) | < 10 seconds (excluding I/O) | Rayon parallel iterators, thread pool = CPU cores |
| Memory usage | < 512MB for 10,000 file batch | Streaming, bounded buffers, minimal cloning |
| Binary size | < 10MB statically linked | Strip symbols, LTO, codegen-units=1 |
| Startup time | < 50ms cold start | Lazy statics for tag database, no runtime initialization |

**Optimization Techniques**:

1. **Zero-Copy Parsing**: Use `&[u8]` slices instead of copying bytes
   ```rust
   // Good: Borrows slice
   fn parse_string(data: &[u8], offset: usize, len: usize) -> &str {
       std::str::from_utf8(&data[offset..offset+len])?
   }

   // Bad: Copies bytes
   fn parse_string_copy(data: &[u8], offset: usize, len: usize) -> String {
       String::from_utf8(data[offset..offset+len].to_vec())?
   }
   ```

2. **SIMD (Future)**: Use `std::simd` for bulk operations (e.g., UTF-8 validation, checksum computation)

3. **Compile-Time Tag Database**: Embed tag definitions as `const` data, avoiding runtime HashMap construction
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

### Context: task-i2-t11 (from 02_Iteration_I2.md)

```markdown
*   **Task 2.11: Create Benchmark Suite with Criterion**
    *   **Task ID:** `I2.T11`
    *   **Description:** Set up benchmarking infrastructure in `benches/parse_benchmarks.rs` using `criterion` crate. Create benchmarks for: (1) Format detection, (2) JPEG segment parsing, (3) TIFF IFD parsing, (4) Full read_metadata() on sample JPEG. Use `Criterion::default()` configuration. Run benchmarks to establish baseline performance. Add `[[bench]]` section to Cargo.toml. Document how to run benchmarks in README.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** Criterion crate documentation, I2.T3 read_metadata function
    *   **Input Files:** [`src/core/operations.rs`, `tests/fixtures/jpeg/sample_with_exif.jpg`]
    *   **Target Files:**
        *   `benches/parse_benchmarks.rs`
        *   `Cargo.toml` (add benchmark section)
        *   `README.md` (add benchmarking instructions)
    *   **Deliverables:**
        *   Criterion benchmarks for parsing operations
        *   Baseline performance measurements
    *   **Acceptance Criteria:**
        *   `cargo bench` runs successfully
        *   At least 4 benchmarks defined (detection, JPEG parse, TIFF parse, full read)
        *   Benchmarks use `black_box()` to prevent compiler optimization
        *   Criterion generates HTML report in target/criterion/
        *   README documents benchmark commands
    *   **Dependencies:** `I2.T3` (needs read_metadata)
    *   **Parallelizable:** Yes (can be set up anytime after core operations are implemented)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `Cargo.toml`
    *   **Summary:** The project's main manifest file. The `criterion` crate is already included in `[dev-dependencies]` with version `0.5`.
    *   **Recommendation:** You MUST add a `[[bench]]` section to this file to define the benchmark target. The section should specify `name = "parse_benchmarks"` and `harness = false` (required for Criterion benchmarks).
    *   **Note:** The file currently has no `[[bench]]` section, so you will be adding one for the first time.

*   **File:** `src/core/operations.rs`
    *   **Summary:** This is the core operations module containing the `read_metadata()` function and related parsing logic. It orchestrates format detection, parser selection, and metadata extraction.
    *   **Key Functions to Benchmark:**
        *   `read_metadata(path: &Path) -> Result<MetadataMap>` - This is the main end-to-end function you should benchmark (line 59)
        *   `parse_jpeg_metadata(reader: &dyn FileReader) -> Result<MetadataMap>` - Internal JPEG parsing function (line 95)
        *   `parse_tiff_metadata(reader: &dyn FileReader) -> Result<MetadataMap>` - Internal TIFF parsing function (line 180)
    *   **Recommendation:** You MUST import `read_metadata` from this module in your benchmark file. Use fully-qualified paths like `use exiftool_rs::core::operations::read_metadata;`

*   **File:** `src/parsers/format_detector.rs`
    *   **Summary:** Contains the `detect_format(reader: &dyn FileReader) -> io::Result<FileFormat>` function (line 96) that identifies file types via magic bytes.
    *   **Recommendation:** You SHOULD benchmark this function separately as it's a critical performance path. Import it as `use exiftool_rs::parsers::format_detector::detect_format;`
    *   **Implementation Detail:** The function reads the first 16 bytes and performs sequential pattern matching. This is lightweight but still worth benchmarking.

*   **File:** `src/parsers/jpeg/segment_parser.rs`
    *   **Summary:** Provides `parse_segments(reader: &dyn FileReader)` function for JPEG segment parsing using nom combinators.
    *   **Recommendation:** You SHOULD benchmark the JPEG segment parsing. Import as `use exiftool_rs::parsers::jpeg::segment_parser::parse_segments;`
    *   **Note:** This function performs zero-copy parsing with nom, which is key to performance.

*   **File:** `src/parsers/tiff/ifd_parser.rs`
    *   **Summary:** Contains `parse_ifd(reader: &dyn FileReader, ifd_offset: u64, byte_order: ByteOrder) -> Result<Vec<(u16, Vec<u8>)>>` for TIFF IFD structure parsing.
    *   **Recommendation:** You MUST benchmark the TIFF IFD parsing function. Import both the function and `ByteOrder` enum: `use exiftool_rs::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};`
    *   **Note:** IFD parsing involves reading tag entries and following offsets, which makes it more complex than format detection.

*   **File:** `tests/fixtures/jpeg/sample_with_exif.jpg`
    *   **Summary:** A small (112 bytes) test JPEG file with EXIF metadata. This file exists and is available for benchmarking.
    *   **Recommendation:** You SHOULD use this file for the full `read_metadata()` benchmark. The path is `"tests/fixtures/jpeg/sample_with_exif.jpg"`.
    *   **Note:** There's also `sample_with_exif_xmp.jpg` (624 bytes) available if you want to test with XMP data.

*   **File:** `README.md`
    *   **Summary:** The project's main README file. Currently contains sections on Development, Building, and Testing (lines 109-139).
    *   **Recommendation:** You SHOULD add a new "Benchmarking" subsection under the "Development" section (after line 125). Include commands for running benchmarks and viewing the HTML report.

### Implementation Tips & Notes

*   **Tip 1 - Criterion Usage:** The Criterion crate is already in dev-dependencies (version 0.5). You MUST use `Criterion::default()` configuration as specified in the task. Use `criterion::black_box()` to wrap inputs and prevent compiler optimizations from eliminating the benchmarked code.

*   **Tip 2 - FileReader Creation:** For benchmarking parsers directly (format detection, JPEG parsing, TIFF parsing), you'll need to create file readers. Use `MMapReader::new(Path::new("tests/fixtures/jpeg/sample_with_exif.jpg"))` for the test file. Import as `use exiftool_rs::io::MMapReader;`

*   **Tip 3 - Benchmark Structure:** Each benchmark should follow this pattern:
    ```rust
    fn benchmark_name(c: &mut Criterion) {
        c.bench_function("benchmark_display_name", |b| {
            // Setup code (outside measurement)
            let reader = MMapReader::new(Path::new("test_file")).unwrap();

            b.iter(|| {
                // Code to benchmark (wrapped in black_box)
                criterion::black_box(function_to_benchmark(&reader))
            });
        });
    }
    ```

*   **Tip 4 - Cargo.toml Benchmark Section:** Add this exact section to `Cargo.toml`:
    ```toml
    [[bench]]
    name = "parse_benchmarks"
    harness = false
    ```
    The `harness = false` is CRITICAL - Criterion provides its own benchmark harness.

*   **Tip 5 - Main Function Signature:** Your benchmark file MUST have this exact signature:
    ```rust
    criterion_group!(benches, bench_format_detection, bench_jpeg_parse, bench_tiff_parse, bench_read_metadata);
    criterion_main!(benches);
    ```
    This macro setup is required by Criterion.

*   **Tip 6 - Performance Expectations:** Based on the architecture docs, JPEG EXIF extraction should target <5ms per file. Use this as a baseline expectation. If benchmarks show significantly slower performance, something may be wrong with the implementation.

*   **Note 1 - TIFF Parsing Benchmark:** For the TIFF IFD parsing benchmark, you'll need to provide an appropriate offset and byte order. You can use the TIFF file in `tests/fixtures/tiff/` if available, or create synthetic TIFF data for testing. The most important thing is to benchmark the parsing logic itself.

*   **Note 2 - Zero-Copy Parsing:** The codebase uses zero-copy parsing patterns (borrowing `&[u8]` slices) which is key to performance. Your benchmarks should demonstrate this efficiency - avoid allocations in hot paths.

*   **Warning:** The project uses strict linting (`clippy -- -D warnings`). Ensure your benchmark code passes clippy checks. Common issues: unused imports, missing documentation for public items (though benchmarks are typically not public).

*   **Note 3 - README Documentation:** When updating the README, place the benchmarking section logically within the "Development" section. Include both how to run benchmarks (`cargo bench`) and how to view the HTML reports (`open target/criterion/report/index.html` on macOS, similar for other OSes).

*   **Note 4 - Baseline Establishment:** This is the FIRST time benchmarks are being run for this project. The baseline performance measurements you establish will be used for future regression detection (10% threshold). Document the results you observe.

*   **Tip 7 - Test Data Setup:** Since you're benchmarking against an existing test file, consider adding setup that verifies the file exists before running benchmarks. This prevents confusing errors if the fixture is missing.

*   **Tip 8 - Benchmark Naming:** Follow Criterion's naming conventions. Use descriptive names like "format_detection", "jpeg_segment_parsing", "tiff_ifd_parsing", "full_read_metadata" for the benchmark function names. These will appear in the HTML report.

*   **Note 5 - Module Structure:** Your `benches/parse_benchmarks.rs` should start with necessary imports, define 4 benchmark functions (one per requirement), then use the criterion_group! and criterion_main! macros at the end to wire everything together.
