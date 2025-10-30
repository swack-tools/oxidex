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

*   **File:** `fuzz/fuzz_targets/fuzz_pdf.rs`
    *   **Summary:** ALREADY EXISTS! This file contains a complete fuzzing harness for the PDF parser. It implements a `FuzzReader` struct that wraps the fuzzer input and calls `parse_pdf_metadata()`.
    *   **Recommendation:** You MUST verify this file is correct and complete. The task requires creating this file, but it already exists. Review it for correctness - it uses saturating arithmetic to prevent panics and properly implements the FileReader trait.

*   **File:** `fuzz/fuzz_targets/fuzz_mp4.rs`
    *   **Summary:** ALREADY EXISTS! This file contains a complete fuzzing harness for the MP4/QuickTime parser. It implements the same pattern as the PDF fuzzer, calling `parse_quicktime_metadata()`.
    *   **Recommendation:** You MUST verify this file is correct and complete. Both fuzzers follow the same pattern with a `FuzzReader` implementation.

*   **File:** `fuzz/Cargo.toml`
    *   **Summary:** The fuzzing project manifest already includes both `fuzz_pdf` and `fuzz_mp4` as binary targets.
    *   **Recommendation:** Verify the configuration is correct. Both targets are already registered in the `[[bin]]` sections.

*   **File:** `src/parsers/pdf/mod.rs`
    *   **Summary:** This is the PDF parser entry point. It exports the public function `parse_pdf_metadata(reader: &dyn FileReader) -> Result<MetadataMap>` which is what the fuzzer calls. The parser verifies PDF signature, extracts Info dictionary metadata, and XMP metadata.
    *   **Recommendation:** The fuzzer MUST use this exact function signature. The existing fuzz_pdf.rs already does this correctly.

*   **File:** `src/parsers/quicktime/mod.rs`
    *   **Summary:** This is the QuickTime/MP4 parser entry point. It exports `parse_quicktime_metadata(reader: &dyn FileReader) -> Result<MetadataMap, String>`. Note the different error type (String vs Result).
    *   **Recommendation:** The fuzzer uses this function. Note the error type difference between PDF (returns `Result<MetadataMap>`) and QuickTime (returns `Result<MetadataMap, String>`).

*   **File:** `src/core/file_reader_trait.rs`
    *   **Summary:** Defines the `FileReader` trait with two methods: `read(&self, offset: u64, length: usize) -> io::Result<&[u8]>` and `size(&self) -> u64`. The trait is object-safe and designed for zero-copy access.
    *   **Recommendation:** Your `FuzzReader` implementation MUST implement this trait exactly. Both existing fuzzers already do this correctly with saturating arithmetic to prevent panics.

*   **File:** `fuzz/corpus/fuzz_pdf/`
    *   **Summary:** This directory ALREADY EXISTS and contains HUNDREDS of corpus files (both hash-named files from fuzzing and 3 named .pdf files: minimal.pdf, sample.pdf, special_chars.pdf).
    *   **Recommendation:** The corpus is already seeded with valid samples. You should verify there are at least 3 valid PDF samples (acceptance criteria met).

*   **File:** `fuzz/corpus/fuzz_mp4/`
    *   **Summary:** This directory ALREADY EXISTS and contains HUNDREDS of corpus files plus 3 named .mp4 files: minimal_itunes.mp4, minimal_quicktime.mp4, sample.mp4.
    *   **Recommendation:** The corpus is already seeded with valid samples. You should verify there are at least 3 valid MP4 samples (acceptance criteria met).

### Implementation Tips & Notes

*   **CRITICAL FINDING:** All fuzzing infrastructure appears to be ALREADY IMPLEMENTED! Both fuzz_pdf.rs and fuzz_mp4.rs exist, both are configured in fuzz/Cargo.toml, and both corpus directories contain valid seed files.

*   **Tip:** The main remaining work is to:
    1. Verify the fuzzing targets work correctly (`cargo fuzz run fuzz_pdf` and `cargo fuzz run fuzz_mp4`)
    2. Update the README.md with fuzzing documentation
    3. Optionally run the fuzzers for 1+ minute to verify they don't crash

*   **Note:** Both fuzzing harnesses use the same pattern:
    - Implement a `FuzzReader` struct wrapping a `Vec<u8>`
    - Implement `FileReader` trait with saturating arithmetic to prevent panics
    - Call the parser and discard errors (we only care about crashes/panics, not parse errors)
    - Use `#![no_main]` and `libfuzzer_sys::fuzz_target!` macro

*   **Warning:** The PDF parser returns `Result<MetadataMap>` (using the project's `Result` type) while the QuickTime parser returns `Result<MetadataMap, String>`. Both fuzzers handle this correctly by discarding results with `let _ = ...`.

*   **Documentation Task:** The README.md currently does not mention fuzzing. You MUST add a section explaining:
    - How to install cargo-fuzz (`cargo install cargo-fuzz`)
    - How to run the fuzzers (`cargo fuzz run fuzz_pdf`, `cargo fuzz run fuzz_mp4`)
    - Where the corpus files are located
    - How to view fuzzing coverage (`cargo fuzz coverage fuzz_pdf`)
    - That crashes are saved to `fuzz/artifacts/`

*   **Acceptance Criteria Verification:**
    - ✅ `fuzz_pdf.rs` exists
    - ✅ `fuzz_mp4.rs` exists
    - ✅ Both are configured in `fuzz/Cargo.toml`
    - ✅ Corpus contains 3+ valid PDF samples (minimal.pdf, sample.pdf, special_chars.pdf)
    - ✅ Corpus contains 3+ valid MP4 samples (minimal_itunes.mp4, minimal_quicktime.mp4, sample.mp4)
    - ⚠️ Need to verify `cargo fuzz run` executes without errors (manual test required)
    - ❌ README does not document fuzzing process (needs to be added)

### Corpus Seeding Strategy

*   **PDF Corpus:** The corpus already contains 3 named seed files plus hundreds of fuzzer-generated files. The named files represent different PDF structures:
    - `minimal.pdf` - likely a minimal valid PDF
    - `sample.pdf` - a more complete example
    - `special_chars.pdf` - tests edge cases with special characters

*   **MP4 Corpus:** The corpus contains 3 named seed files:
    - `minimal_itunes.mp4` - iTunes-style metadata format
    - `minimal_quicktime.mp4` - Classic QuickTime format
    - `sample.mp4` - Complete example file

*   **Recommendation:** The corpus seeding is already excellent. These seed files cover the major format variants mentioned in the parser documentation (iTunes metadata, QuickTime user data, classic MP4).

### GitHub Actions CI Integration

*   **Note:** The existing `.github/workflows/ci.yml` contains test, audit, and coverage jobs, but does NOT include a fuzzing job.
*   **Optional Enhancement:** If you want to add PR fuzzing (short runs), you could add a new job that runs each fuzzer for 60 seconds on pull requests. However, this is NOT required by the acceptance criteria.
