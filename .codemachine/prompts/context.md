# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I4.T10",
  "iteration_id": "I4",
  "iteration_goal": "Add support for PDF and MP4/QuickTime formats, implement batch processing with recursive directory traversal and parallel execution, add metadata copying between files, and expand tag registry.",
  "description": "Create fuzzing harnesses in fuzz/fuzz_targets/ for PDF and MP4 parsers. Set up continuous fuzzing: (1) Create fuzz_pdf.rs calling PDF parser with fuzzer-generated input, (2) Create fuzz_mp4.rs calling MP4 parser, (3) Seed corpus with sample valid files, (4) Configure cargo-fuzz to run both targets, (5) Document fuzzing process in README. Optionally submit to OSS-Fuzz for continuous fuzzing infrastructure.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I4.T1 PDF parser, I4.T2 MP4 parser, cargo-fuzz documentation",
  "input_files": ["src/parsers/pdf/mod.rs", "src/parsers/quicktime/mod.rs"],
  "target_files": [
    "fuzz/fuzz_targets/fuzz_pdf.rs",
    "fuzz/fuzz_targets/fuzz_mp4.rs",
    "fuzz/corpus/pdf/",
    "fuzz/corpus/mp4/",
    "README.md"
  ],
  "deliverables": "Fuzzing targets for PDF and MP4, seed corpus, documentation",
  "acceptance_criteria": "cargo fuzz run fuzz_pdf executes without errors, cargo fuzz run fuzz_mp4 executes without errors, corpus contains at least 3 valid samples each, fuzzing runs for at least 1 minute without crashes (manual verification), README documents how to run fuzzing",
  "dependencies": ["I4.T1", "I4.T2"],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

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

### Context: reliability-availability (from 05_Operational_Architecture.md)

```markdown
#### Reliability & Availability

**Reliability Strategy**:

1. **Fault Tolerance**:
   - **Graceful Degradation**: On parser error, return partial metadata rather than failing entirely
   - **Error Recovery**: Malformed EXIF segment logs warning but continues parsing other segments (IPTC, XMP)
   - **Atomic Writes**: Temporary file + rename prevents corruption on crash mid-write

2. **Testing Pyramid**:
   ```
          /\
         /E2E\        <- Integration tests (10%): Full workflows
        /------\
       /  Unit  \      <- Unit tests (70%): Parser functions, tag validation
      /----------\
     / Property   \    <- Property-based (20%): Round-trip serialization, invariants
    /--------------\
   ```

   - **Unit Tests**: Every parser function has success/failure test cases
   - **Property-Based**: `proptest` for round-trip (write then read equals original)
     ```rust
     proptest! {
         fn roundtrip_exif_date(dt: DateTime<Utc>) {
             let serialized = serialize_exif_datetime(dt);
             let deserialized = parse_exif_datetime(serialized)?;
             assert_eq!(dt, deserialized);
         }
     }
     ```
   - **Integration Tests**: CLI invocations against reference ExifTool output
   - **Fuzzing**: Continuous fuzzing via OSS-Fuzz (targets all parsers)

3. **Crash Resistance**:
   - No `unwrap()` in production code (enforced via clippy lint)
   - All `unsafe` blocks documented with safety invariants and minimized
   - Stack overflow protection: Limit recursion depth (nested IFDs, XML depth)
```

### Context: deeper-dive-fuzzing (from 06_Rationale_and_Future.md)

```markdown
#### 6. Comprehensive Fuzzing Strategy

**Current State**: Conceptual (use `cargo-fuzz`).

**Needs**:
- Fuzzing harnesses for each format parser
- Corpus seeding (known-good and known-malicious files)
- Continuous fuzzing infrastructure (OSS-Fuzz integration)
- Crash triage and fix workflow

**Key Questions**:
- How to measure coverage (format code paths, not just line coverage)?
- How to prioritize fuzz findings (crash vs. hang vs. incorrect output)?
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

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/parsers/pdf/mod.rs`
    *   **Summary:** Main PDF parser module that provides `parse_pdf_metadata()` function. This is the entry point you should fuzz. It takes a `&dyn FileReader` and returns a `Result<MetadataMap>`. The parser validates PDF signature, extracts Info dictionary metadata, and XMP metadata. It handles errors gracefully with warnings.
    *   **Recommendation:** Your fuzzing harness MUST call `parse_pdf_metadata()` with fuzzer-generated data wrapped in a test FileReader. The parser expects PDF-formatted input starting with `%PDF-` signature.
    *   **Key Functions to Fuzz:**
        - `parse_pdf_metadata(reader: &dyn FileReader)` - Main entry point
        - Internally uses `info_parser::parse_info_dict()` and `xmp_extractor::extract_xmp_metadata()`
    *   **Error Handling:** The parser uses graceful degradation - it catches errors from sub-parsers and continues. This is GOOD for fuzzing as it won't crash on the first malformed element.

*   **File:** `src/parsers/quicktime/mod.rs`
    *   **Summary:** QuickTime/MP4 parser providing `parse_quicktime_metadata()` function. Takes a `&dyn FileReader` and returns `Result<MetadataMap, String>`. Validates file signature by checking for `ftyp`, `moov`, `mdat`, `wide`, `free`, or `skip` atoms. Reads up to 10MB of file data for parsing.
    *   **Recommendation:** Your fuzzing harness MUST call `parse_quicktime_metadata()` with fuzzer-generated data. The parser expects QuickTime/MP4 atom structure.
    *   **Key Functions to Fuzz:**
        - `parse_quicktime_metadata(reader: &dyn FileReader)` - Main entry point
        - Internally uses `atom_parser::parse_atoms()` and `metadata_extractor::extract_metadata()`
    *   **Critical Detail:** Parser reads up to 10MB (`max_read_size = 10 * 1024 * 1024`). Your fuzzer should be aware that very large inputs may consume significant memory.

*   **File:** `tests/integration/pdf_tests.rs`
    *   **Summary:** Contains helper `TestReader` struct that implements `FileReader` trait for in-memory testing. This is the EXACT pattern you should use in your fuzzing harness.
    *   **Recommendation:** You MUST copy the `TestReader` implementation pattern to your fuzzing harnesses. See lines 162-190 of `src/parsers/pdf/mod.rs` for the complete implementation.
    *   **Example Code:**
        ```rust
        struct TestReader { data: Vec<u8> }
        impl FileReader for TestReader {
            fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
                let start = offset as usize;
                let end = start + length;
                if end > self.data.len() {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "read beyond end"));
                }
                Ok(&self.data[start..end])
            }
            fn size(&self) -> u64 { self.data.len() as u64 }
        }
        ```

*   **File:** `tests/integration/mp4_tests.rs`
    *   **Summary:** Contains similar `TestReader` implementation for MP4 testing. Also shows how to create minimal valid test files.
    *   **Recommendation:** Reference this file for MP4-specific testing patterns.

*   **File:** `tests/fixtures/pdf/sample.pdf`
    *   **Summary:** Existing valid PDF file that should be used to seed the fuzzing corpus.
    *   **Recommendation:** You MUST copy this file to `fuzz/corpus/pdf/sample.pdf` as one of the 3+ seed files.

*   **File:** `tests/fixtures/mp4/sample.mp4`
    *   **Summary:** Existing valid MP4 file that should be used to seed the fuzzing corpus.
    *   **Recommendation:** You MUST copy this file to `fuzz/corpus/mp4/sample.mp4` as one of the 3+ seed files.

*   **File:** `Cargo.toml`
    *   **Summary:** Main project Cargo manifest. Currently does NOT have cargo-fuzz configuration.
    *   **Recommendation:** You will need to set up cargo-fuzz separately. cargo-fuzz typically creates its own `fuzz/Cargo.toml` file when initialized with `cargo fuzz init`.

### Implementation Tips & Notes

*   **Tip: Use cargo-fuzz init** - Run `cargo fuzz init` in the project root to automatically create the `fuzz/Cargo.toml` and basic directory structure. This sets up the fuzzing workspace correctly.

*   **Tip: Fuzzing Harness Pattern** - Each fuzzing target should follow this pattern:
    ```rust
    #![no_main]
    use libfuzzer_sys::fuzz_target;
    use exiftool_rs::parsers::pdf::parse_pdf_metadata;
    use exiftool_rs::core::FileReader;
    use std::io;

    struct FuzzReader { data: Vec<u8> }
    impl FileReader for FuzzReader {
        fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
            let start = offset as usize;
            let end = start.saturating_add(length).min(self.data.len());
            if start >= self.data.len() { return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof")); }
            Ok(&self.data[start..end])
        }
        fn size(&self) -> u64 { self.data.len() as u64 }
    }

    fuzz_target!(|data: &[u8]| {
        let reader = FuzzReader { data: data.to_vec() };
        let _ = parse_pdf_metadata(&reader);
    });
    ```

*   **Tip: Saturating Arithmetic in Fuzz Reader** - Use `saturating_add()` instead of regular addition in your fuzzing FileReader implementation to prevent integer overflow panics. Example: `start.saturating_add(length).min(self.data.len())`.

*   **Tip: Corpus Seeding Strategy** - For BEST fuzzing effectiveness, seed the corpus with:
    1. **Valid samples**: Copy from `tests/fixtures/pdf/sample.pdf` and `tests/fixtures/mp4/sample.mp4`
    2. **Minimal files**: Create the smallest possible valid file (see test helper functions in parser mod.rs files)
    3. **Edge cases**: Files with special characters, empty fields, maximum sizes

*   **Warning: Memory Limits** - The MP4 parser reads up to 10MB into memory. Set appropriate memory limits for fuzzing to prevent OOM: `cargo fuzz run fuzz_mp4 -- -max_len=10485760` (10MB).

*   **Note: Fuzzing Commands** - Document these commands in the README:
    ```bash
    # Install cargo-fuzz
    cargo install cargo-fuzz

    # Run PDF fuzzer
    cargo fuzz run fuzz_pdf

    # Run MP4 fuzzer
    cargo fuzz run fuzz_mp4 -- -max_len=10485760

    # Run with time limit (1 minute for acceptance criteria)
    cargo fuzz run fuzz_pdf -- -max_total_time=60

    # Check coverage
    cargo fuzz coverage fuzz_pdf
    ```

*   **Tip: Fuzz Target Template** - Each fuzz target file should be ~30 lines total:
    - `#![no_main]` directive
    - Import statements (libfuzzer_sys, parser module, FileReader)
    - FuzzReader struct implementation
    - `fuzz_target!` macro with parser call

*   **Critical: Error Handling in Fuzz Targets** - Your fuzz targets should DISCARD all errors with `let _ = parse_*()`. You're looking for PANICS and CRASHES, not Result errors. Errors are expected and normal for malformed input.

*   **Note: Directory Structure** - After setup, your fuzz directory should look like:
    ```
    fuzz/
    ├── Cargo.toml (auto-generated by cargo fuzz init)
    ├── fuzz_targets/
    │   ├── fuzz_pdf.rs
    │   └── fuzz_mp4.rs
    └── corpus/
        ├── pdf/
        │   ├── sample.pdf (copy from tests/fixtures)
        │   ├── minimal.pdf (create programmatically)
        │   └── special_chars.pdf (create programmatically)
        └── mp4/
            ├── sample.mp4 (copy from tests/fixtures)
            ├── minimal.mp4 (create programmatically)
            └── itunes.mp4 (create programmatically)
    ```

*   **Recommendation: Create Corpus Files** - For the "at least 3 valid samples each" requirement:
    - **PDF corpus**: Copy `sample.pdf` + create two minimal PDFs using the test helper functions from `src/parsers/pdf/mod.rs::tests::create_test_pdf_with_info()`
    - **MP4 corpus**: Copy `sample.mp4` + create two minimal MP4s using test helpers from `src/parsers/quicktime/mod.rs::tests::create_test_quicktime_file()` and `create_test_itunes_file()`

*   **Tip: README Documentation Section** - Add a new "## Fuzzing" section to README.md with:
    1. Prerequisites (cargo-fuzz installation)
    2. Running fuzz targets (commands for each target)
    3. Corpus management (where files are, how to add)
    4. Coverage measurement
    5. CI integration (future: mention OSS-Fuzz)

*   **Critical: Test Acceptance Criteria** - You MUST verify:
    1. `cargo fuzz build fuzz_pdf` compiles successfully
    2. `cargo fuzz build fuzz_mp4` compiles successfully
    3. `cargo fuzz run fuzz_pdf -- -max_total_time=60` runs for 1 minute without crashing
    4. `cargo fuzz run fuzz_mp4 -- -max_total_time=60 -max_len=10485760` runs for 1 minute without crashing
    5. Corpus directories contain 3+ files each (verify with `ls -l fuzz/corpus/pdf/ fuzz/corpus/mp4/`)

*   **Note: OSS-Fuzz Integration** - The task says "optionally submit to OSS-Fuzz". This is FUTURE WORK - document it as a TODO in the README but DO NOT implement it now. Focus on getting local fuzzing working first.

*   **Warning: Do Not Commit Fuzzer Artifacts** - Add these lines to `.gitignore`:
    ```
    fuzz/target/
    fuzz/artifacts/
    ```
    The fuzz/corpus/ files SHOULD be committed (they're seed files), but artifacts (crashes) should not be.

*   **Tip: Minimal Corpus File Creation** - Create a script or document the process for generating minimal corpus files:
    ```rust
    // In a temporary test or build script
    use std::fs;
    let minimal_pdf = create_test_pdf_with_info(); // from tests
    fs::write("fuzz/corpus/pdf/minimal.pdf", minimal_pdf)?;
    ```

*   **Recommendation: Start with fuzz_pdf First** - PDF parsing is simpler than MP4, so implement and test `fuzz_pdf.rs` completely before moving to `fuzz_mp4.rs`. This allows you to debug the fuzzing setup with a simpler target.
