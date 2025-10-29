# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I1.T12",
  "iteration_id": "I1",
  "iteration_goal": "Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.",
  "description": "Write comprehensive integration test plan in `docs/testing/integration_test_plan.md`. Document: (1) Test image corpus strategy (collect 100+ diverse images across formats, including malformed samples), (2) Validation criteria (compare ExifTool-RS output against Perl ExifTool using JSON output diff), (3) Acceptance thresholds (99% tag value match for well-formed files, graceful degradation for malformed), (4) Regression testing approach (lock test corpus in git LFS, run comparison on every commit), (5) Test categories (format coverage, tag coverage, error handling, performance benchmarks).",
  "agent_type_hint": "DocumentationAgent",
  "inputs": "Section 2.1 (Key Architectural Artifacts - Test Plan), architecture blueprint testing strategy",
  "target_files": [
    "docs/testing/integration_test_plan.md"
  ],
  "input_files": [],
  "deliverables": "Markdown document with detailed test plan",
  "acceptance_criteria": "Document covers all 5 areas mentioned in task description, specifies exact comparison methodology (e.g., `exiftool -json photo.jpg` vs `exiftool-rs -json photo.jpg`), defines pass/fail criteria (e.g., \"99% of tag values must match exactly\"), includes plan for sourcing test images (public datasets, creative commons, generated), mentions git LFS for large binary test files, well-formatted Markdown with clear sections",
  "dependencies": [
    "I1.T1"
  ],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: integration-tests (from 03_Verification_and_Glossary.md)

```markdown
#### Integration Tests (10% of test suite)
*   **Scope:** End-to-end workflows and CLI operations
*   **Location:** `tests/integration/`
*   **Tools:** `cargo test`, filesystem fixtures in `tests/fixtures/`
*   **Coverage Requirements:**
    *   Full read workflow: file → metadata extraction → output
    *   Full write workflow: read → modify → write → verify
    *   CLI argument parsing and execution
    *   Batch processing with multiple files
    *   Error scenarios (missing file, corrupted metadata, permission denied)
*   **ExifTool Comparison Tests:** Special integration tests comparing output against Perl ExifTool
    *   Run both tools on same test corpus (100+ images)
    *   Compare JSON output for tag value parity
    *   Acceptance threshold: 98%+ match rate
    *   Conditional on ExifTool availability (`#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]`)
*   **Examples:**
    ```rust
    #[test]
    fn test_cli_extract_jpeg_exif() {
        let output = Command::new("./target/debug/exiftool-rs")
            .arg("tests/fixtures/jpeg/sample.jpg")
            .output()?;
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)?.contains("EXIF:Make"));
    }

    #[test]
    #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
    fn compare_against_exiftool_jpeg() {
        let exiftool_json = get_exiftool_output("sample.jpg")?;
        let our_json = get_exiftool_rs_output("sample.jpg")?;
        let match_rate = compare_json_outputs(&exiftool_json, &our_json);
        assert!(match_rate >= 0.98, "Match rate: {}", match_rate);
    }
    ```
```

### Context: fuzzing (from 03_Verification_and_Glossary.md)

```markdown
#### Fuzzing (Continuous)
*   **Scope:** Crash and hang detection in parsers
*   **Location:** `fuzz/fuzz_targets/`
*   **Tools:** `cargo-fuzz` (libFuzzer), OSS-Fuzz integration
*   **Targets:**
    *   `fuzz_jpeg` - JPEG segment parser
    *   `fuzz_tiff` - TIFF IFD parser
    *   `fuzz_png` - PNG chunk parser
    *   `fuzz_pdf` - PDF structure parser
    *   `fuzz_mp4` - QuickTime atom parser
*   **Corpus:** Seed with valid samples + malformed files
*   **Coverage:** Aim for 80%+ code coverage via fuzzing (measured with `cargo fuzz coverage`)
*   **Integration:** OSS-Fuzz for continuous fuzzing, GitHub Actions for PR fuzzing (short runs)
*   **Triage:** All crashes investigated within 48 hours, fixes prioritized by severity
```

### Context: security-considerations (from 05_Operational_Architecture.md)

```markdown
#### Security Considerations

**Threat Model**:

ExifTool-RS processes potentially malicious files from untrusted sources (e.g., user uploads, scraped images). Primary threats:

1. **Memory Corruption**: Buffer overflows, use-after-free in parsers
2. **Resource Exhaustion**: Zip bombs, billion laughs (XML), decompression bombs
3. **Path Traversal**: Malicious filenames in archive processing
4. **Code Injection**: Via scripting features (if added)

**Mitigations**:

| **Threat** | **Mitigation** | **Implementation** |
|------------|---------------|-------------------|
| Buffer overflows | Rust ownership system | Compile-time prevention via borrow checker |
| Integer overflows | Checked arithmetic | `#![deny(overflowing_literals)]`, `checked_add()` in parsers |
| Resource exhaustion | Size limits | Max allocation: 1GB per file, max parse depth: 64 levels (nested IFDs) |
| Zip bombs | Decompression ratio check | Reject if uncompressed > 100x compressed size |
| XXE attacks (XML) | Disable external entities | `quick-xml` configured to reject DOCTYPE, external entities |
| Path traversal | Path sanitization | `canonicalize()` + jail to working directory for batch operations |
| Dependency vulnerabilities | Automated scanning | `cargo-audit` in CI, Dependabot alerts, minimal dependency tree |
| Malicious input | Fuzzing | Continuous fuzzing with `cargo-fuzz`, OSS-Fuzz integration target |

**Input Validation**:

All parsers follow defensive pattern:
```rust
fn read_u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data.get(offset..offset+4)
        .ok_or(ParseError::UnexpectedEof)?;  // Bounds check
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}
```

**Secure Defaults**:

- No script execution (unlike Perl ExifTool's `-execute` feature)
- No network access by default (geolocation requires opt-in `--geolocation` flag)
- Read-only mode available via `--readonly` flag (prevents accidental writes)
```

### Context: unit-tests (from 03_Verification_and_Glossary.md)

```markdown
#### Unit Tests (70% of test suite)
*   **Scope:** Individual functions and modules
*   **Location:** Inline in source files (`#[cfg(test)] mod tests`) and `tests/` directory
*   **Tools:** `cargo test`, standard Rust test framework
*   **Coverage Requirements:**
    *   All parser functions (format detection, segment parsing, IFD parsing, tag extraction)
    *   Data model operations (metadata map accessors, tag value conversions)
    *   Validation logic (tag value type checking, constraint validation)
    *   Error handling paths (parse errors, I/O errors, validation failures)
*   **Acceptance Criteria:** 80%+ line coverage (measured with `cargo-tarpaulin` or `cargo-llvm-cov`)
*   **Examples:**
    ```rust
    #[test]
    fn test_jpeg_magic_bytes_detection() {
        let data = vec![0xFF, 0xD8, 0xFF, 0xE0];
        assert_eq!(detect_format(&data), FileFormat::JPEG);
    }

    #[test]
    fn test_tag_value_type_validation() {
        let descriptor = TagDescriptor { /* String type */ };
        let value = TagValue::Integer(42);
        assert!(validate_tag_value(&descriptor, &value).is_err());
    }
    ```
```

### Context: property-based-tests (from 03_Verification_and_Glossary.md)

```markdown
#### Property-Based Tests (20% of test suite)
*   **Scope:** Invariant verification and round-trip testing
*   **Location:** `tests/property/`
*   **Tools:** `proptest` crate
*   **Coverage Requirements:**
    *   Round-trip serialization: `parse(serialize(x)) == x`
    *   Date/time arithmetic correctness
    *   File format preservation (write doesn't corrupt image data)
    *   Tag value conversions (string ↔ integer ↔ rational)
*   **Examples:**
    ```rust
    proptest! {
        #[test]
        fn roundtrip_exif_datetime(dt: DateTime<Utc>) {
            let serialized = serialize_exif_datetime(dt);
            let deserialized = parse_exif_datetime(&serialized)?;
            assert_eq!(dt.timestamp(), deserialized.timestamp());
        }

        #[test]
        fn jpeg_write_preserves_image_data(metadata: MetadataMap) {
            let original = read_jpeg("test.jpg")?;
            write_metadata("test.jpg", &metadata)?;
            let modified = read_jpeg("test.jpg")?;
            assert_eq!(original.image_data, modified.image_data);
        }
    }
    ```
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

### Context: task-i1-t12 (from 02_Iteration_I1.md)

```markdown
*   **Task 1.12: Create Integration Test Plan Document**
    *   **Task ID:** `I1.T12`
    *   **Description:** Write comprehensive integration test plan in `docs/testing/integration_test_plan.md`. Document: (1) Test image corpus strategy (collect 100+ diverse images across formats, including malformed samples), (2) Validation criteria (compare ExifTool-RS output against Perl ExifTool using JSON output diff), (3) Acceptance thresholds (99% tag value match for well-formed files, graceful degradation for malformed), (4) Regression testing approach (lock test corpus in git LFS, run comparison on every commit), (5) Test categories (format coverage, tag coverage, error handling, performance benchmarks).
    *   **Agent Type Hint:** `DocumentationAgent`
    *   **Inputs:** Section 2.1 (Key Architectural Artifacts - Test Plan), architecture blueprint testing strategy
    *   **Input Files:** []
    *   **Target Files:**
        *   `docs/testing/integration_test_plan.md`
    *   **Deliverables:**
        *   Markdown document with detailed test plan
    *   **Acceptance Criteria:**
        *   Document covers all 5 areas mentioned in task description
        *   Specifies exact comparison methodology (e.g., `exiftool -json photo.jpg` vs `exiftool-rs -json photo.jpg`)
        *   Defines pass/fail criteria (e.g., "99% of tag values must match exactly")
        *   Includes plan for sourcing test images (public datasets, creative commons, generated)
        *   Mentions git LFS for large binary test files
        *   Well-formatted Markdown with clear sections
    *   **Dependencies:** `I1.T1`
    *   **Parallelizable:** Yes (can be written in parallel with code development)
```

### Context: ci-cd-pipeline (from 03_Verification_and_Glossary.md)

```markdown
### 5.2. CI/CD Pipeline

#### Continuous Integration (GitHub Actions)

**Workflow: `.github/workflows/ci.yml`**
*   **Triggers:** Every push, every pull request
*   **Matrix:**
    *   OS: `ubuntu-latest`, `macos-latest`, `windows-latest`
    *   Rust version: `stable`, `beta` (optional: `nightly` for feature preview)
*   **Steps:**
    1. **Checkout:** Clone repository
    2. **Setup Rust:** Install Rust toolchain via `dtolnay/rust-toolchain`
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `tests/fixtures/`
    *   **Summary:** This directory structure already exists with subdirectories for `jpeg/`, `png/`, `tiff/`, and `malformed/` test images. The subdirectories are currently empty but provide the correct organizational structure.
    *   **Recommendation:** Reference this existing directory structure in your integration test plan. The plan should document how test images will be organized within these directories.

*   **File:** `tests/integration/`
    *   **Summary:** Empty directory that will house integration test Rust code files (like `jpeg_tests.rs`, `png_tests.rs`, etc.).
    *   **Recommendation:** Your test plan should specify that integration tests will be placed here and reference the example integration test code shown in the verification strategy.

*   **File:** `tests/property/`
    *   **Summary:** Empty directory designated for property-based tests using `proptest`.
    *   **Recommendation:** Your test plan should distinguish between integration tests (end-to-end workflows) and property-based tests (invariant verification).

*   **File:** `src/parsers/format_detector.rs`
    *   **Summary:** This file demonstrates the project's current testing patterns. It contains comprehensive unit tests at the module level using `#[cfg(test)] mod tests` with a `TestReader` implementation for testing. Tests cover normal cases, edge cases, and error conditions.
    *   **Recommendation:** Your integration test plan should reference the existing unit testing patterns and note that integration tests will build upon these unit tests by testing complete workflows rather than isolated functions.

*   **File:** `src/error/mod.rs`
    *   **Summary:** Defines the `ExifToolError` enum with variants for `IoError`, `ParseError`, `TagNotFound`, `InvalidTagValue`, and `UnsupportedFormat`. Includes comprehensive error handling and test coverage.
    *   **Recommendation:** Your test plan should specify that error handling scenarios (malformed files, missing files, corrupted metadata) should trigger these specific error types and validate that appropriate errors are returned.

*   **File:** `Cargo.toml`
    *   **Summary:** The project uses `proptest`, `criterion`, and `tempfile` as dev-dependencies. The project version is `0.1.0` and targets GPL-3.0 license.
    *   **Recommendation:** Your test plan should reference these testing tools and specify which types of tests use which tools (proptest for property-based, criterion for benchmarks).

*   **File:** `.gitignore`
    *   **Summary:** Currently configured to ignore `fuzz/corpus/` and `fuzz/artifacts/` but does NOT include any Git LFS configuration. No `.gitattributes` file exists.
    *   **Recommendation:** Your test plan MUST include instructions for setting up Git LFS for test images, as this is not yet configured. Include the specific commands to set up LFS (e.g., `git lfs install`, `git lfs track "tests/fixtures/**/*.jpg"`).

### Implementation Tips & Notes

*   **Tip:** The architecture documents specify a 98%+ match rate for ExifTool comparison tests, but the task description specifies 99%. You should use **99%** as specified in the task acceptance criteria, as that's the target for this specific iteration.

*   **Note:** The existing `docs/` directory structure already includes `docs/diagrams/` (with PlantUML and Mermaid files) but `docs/testing/` does not exist yet. You MUST create this directory before writing the file.

*   **Warning:** Git LFS is mentioned in the task but is NOT currently set up in the project. Your test plan must include:
    1. Instructions to install Git LFS
    2. Configuration file `.gitattributes` to track binary test images
    3. Commands to track test fixtures: `git lfs track "tests/fixtures/**/*.jpg" "tests/fixtures/**/*.png" "tests/fixtures/**/*.tif"`
    4. Note about storage quotas and repository size management

*   **Tip:** The security considerations section emphasizes testing with malicious input. Your test plan should specifically address the `tests/fixtures/malformed/` directory and describe what types of malformed files should be included (truncated files, files with invalid magic bytes, files with malicious payloads designed to trigger parser edge cases).

*   **Note:** The project uses Rust's standard test framework (`cargo test`). The integration test plan should specify the exact command to run tests: `cargo test --test '*'` for all integration tests, or `cargo test --features exiftool-comparison` for comparison tests.

*   **Tip:** Benchmarking is mentioned as a test category but is separate from functional testing. Use `criterion` for microbenchmarks and suggest using `hyperfine` for CLI-level performance comparison against Perl ExifTool.

*   **Note:** The project architecture emphasizes cross-platform support (Linux, macOS, Windows). Your test plan should mention that the test corpus and comparison methodology must work consistently across all three platforms.

*   **Warning:** The task specifies "lock test corpus in git LFS, run comparison on every commit" but this needs clarification. Git LFS stores file pointers in commits, not the actual files. Your plan should clarify that:
    1. Test images are committed via Git LFS (pointers only)
    2. CI/CD pipeline will download actual files from LFS during test runs
    3. This requires configuring GitHub Actions to have LFS access

*   **Tip:** For sourcing test images, recommend specific public datasets:
    - EXIF Test Suite from exiv2 project (permissive license)
    - Unsplash free images (CC0 license)
    - Generated synthetic images with known EXIF using Python/ImageMagick
    - Deliberately malformed images for security testing

*   **Note:** The verification strategy shows examples using `Command::new()` to spawn CLI processes. Your test plan should specify this pattern for CLI integration tests and distinguish it from library API integration tests (which would directly call Rust functions).
