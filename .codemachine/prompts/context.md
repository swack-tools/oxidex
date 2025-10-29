# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I1.T1",
  "iteration_id": "I1",
  "iteration_goal": "Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.",
  "description": "Create Rust project with cargo, set up directory structure per Section 3 of the plan, configure Cargo.toml with initial dependencies (clap, nom, serde_json, chrono, memmap2, rayon, quick-xml, encoding_rs), add rustfmt and clippy configuration, create basic README.md and LICENSE (GPL-3.0), set up .gitignore.",
  "agent_type_hint": "SetupAgent",
  "inputs": "Section 3 (Directory Structure) from plan, Section 2 (Technology Stack) for dependency list",
  "target_files": [
    "Cargo.toml",
    "Cargo.lock",
    "src/main.rs",
    "src/lib.rs",
    "README.md",
    "LICENSE",
    ".gitignore",
    "rustfmt.toml",
    ".clippy.toml"
  ],
  "input_files": [],
  "deliverables": "Fully initialized Rust workspace with compiling (but minimal) code, all directories created per architecture plan, dependencies specified in Cargo.toml, cargo build succeeds",
  "acceptance_criteria": "cargo build completes without errors, cargo clippy runs without warnings, cargo fmt --check passes, directory structure matches Section 3 exactly, README.md contains project description and basic usage placeholder",
  "dependencies": [],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: Directory Structure (from 01_Plan_Overview_and_Setup.md)

The complete directory structure specification is in Section 3 of the plan document. Key points:

- Root directory: `exiftool-rs/`
- Source structure follows hexagonal architecture with three layers:
  - **Application Layer**: `src/cli/` and `src/ffi/`
  - **Domain Layer**: `src/core/` (format-agnostic metadata models)
  - **Infrastructure Layer**: `src/parsers/`, `src/writers/`, `src/io/`, `src/tag_db/`
- All directories should have `mod.rs` files as entry points
- Complete file tree includes: src/, tests/, fuzz/, benches/, docs/, api/, and configuration files

### Context: Technology Stack (from 01_Plan_Overview_and_Setup.md)

Required dependencies in Cargo.toml:
- **CLI Framework**: `clap` v4 (derive API)
- **Binary Parsing**: `nom` v7
- **XML Parsing**: `quick-xml` (XMP metadata)
- **JSON Output**: `serde_json`
- **Date/Time**: `chrono`
- **String Encoding**: `encoding_rs`
- **Concurrency**: `rayon` (data parallelism)
- **Memory-mapped I/O**: `memmap2`
- **Testing**: `proptest` (property-based), `criterion` (benchmarking)
- **Tooling**: `clippy`, `rustfmt`, `cargo-audit`

### Context: Architectural Style (from 02_Architecture_Overview.md)

The project follows **Layered Hexagonal Architecture** (Ports and Adapters):

**Layered Structure**:
```
┌─────────────────────────────────────────────┐
│  Application Layer (CLI, FFI, Library API) │  ← Primary Adapters
├─────────────────────────────────────────────┤
│       Domain Layer (Metadata Engine)        │  ← Core Business Logic
├─────────────────────────────────────────────┤
│  Infrastructure Layer (Format Parsers, I/O) │  ← Secondary Adapters
└─────────────────────────────────────────────┘
```

Rationale:
1. **Format Independence**: Core domain isolated from 300+ file format specifics
2. **Multiple Access Patterns**: CLI, Rust library API, C FFI bindings, format parsers, file system access
3. **Testability**: Core logic testable independently of I/O by mocking file system port
4. **Extensibility**: New formats implement adapter interface without touching core

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `Cargo.toml`
    *   **Summary:** The project manifest has been created with correct metadata (name: "exiftool-rs", version: "0.1.0", edition: "2021", license: "GPL-3.0"). All required dependencies are already specified (clap v4.5, nom v7.1, quick-xml v0.31, serde v1.0, serde_json v1.0, chrono v0.4, encoding_rs v0.8, memmap2 v0.9, rayon v1.10) with dev-dependencies (proptest v1.4, criterion v0.5, tempfile v3.10). The release profile includes optimizations (lto=true, strip=true, codegen-units=1, opt-level=3).
    *   **Recommendation:** This file is **ALREADY COMPLETE** and meets all requirements. You do NOT need to modify it.

*   **File:** `rustfmt.toml`
    *   **Summary:** The rustfmt configuration file exists with sensible defaults (edition="2021", max_width=100, reorder_imports=true, reorder_modules=true, etc.). Some unstable features are configured that require nightly Rust (wrap_comments, format_code_in_doc_comments, comment_width, normalize_comments).
    *   **Recommendation:** This file is **ALREADY COMPLETE**. Note that some features require nightly Rust and will show warnings on stable, which is acceptable.

*   **File:** `.clippy.toml`
    *   **Summary:** The clippy configuration file exists with strict linting settings (cognitive-complexity-threshold=30, missing-docs-in-crate-items=true, too-many-arguments-threshold=7, type-complexity-threshold=250, single-char-binding-names-threshold=4).
    *   **Recommendation:** This file is **ALREADY COMPLETE** and correctly configured.

*   **File:** `README.md`
    *   **Summary:** A comprehensive README exists (177 lines) with project vision, key features (planned), architecture overview, current status with checkboxes (completed/in progress/planned), installation instructions, usage examples (library API and CLI both marked as planned/coming soon), development setup, contributing guidelines, license statement, acknowledgments, technology stack list, and roadmap reference.
    *   **Recommendation:** This file is **ALREADY COMPLETE** and exceeds requirements for a basic README with project description and usage placeholder.

*   **File:** `.gitignore`
    *   **Summary:** A comprehensive .gitignore exists covering CodeMachine directories (.codemachine/memory, .codemachine/artifacts), Node modules, environment variables, OS files (.DS_Store, Thumbs.db), IDE files (.vscode, .idea, *.swp), Rust build artifacts (/target/, **/*.rs.bk, *.pdb), fuzzing corpus (fuzz/corpus/, fuzz/artifacts/), and benchmark outputs (benches/target/).
    *   **Recommendation:** This file is **ALREADY COMPLETE** and properly configured.

*   **File:** `src/main.rs`
    *   **Summary:** A minimal CLI entry point exists (12 lines) with proper documentation comment block. It imports VERSION constant from exiftool_rs library and prints version info plus work-in-progress message.
    *   **Recommendation:** This file is **ALREADY COMPLETE** for the current task. It's a minimal skeleton as required.

*   **File:** `src/lib.rs`
    *   **Summary:** The library root file exists (49 lines) with comprehensive rustdoc documentation describing the hexagonal architecture, module organization (application/domain/infrastructure layers), and a usage example. It has proper lint warnings (#![warn(missing_docs)], #![warn(clippy::all)]) and temporary #![allow(dead_code)] for initial development. Module declarations exist for all required modules: cli, ffi, core, io, parsers, writers, error, tag_db. VERSION constant is exported.
    *   **Recommendation:** This file is **ALREADY COMPLETE**. All module declarations match the required directory structure.

*   **Directory:** `src/` subdirectories
    *   **Summary:** ALL required directories and mod.rs files have been created across the entire source tree:
        - Application Layer: `cli/` (with args.rs, output_formatter.rs, batch_processor.rs), `ffi/` (with c_api.rs)
        - Domain Layer: `core/` (with metadata_map.rs, tag_value.rs, tag_descriptor.rs, operations.rs, validation.rs, format_parser_trait.rs, file_reader_trait.rs)
        - Infrastructure Layer: `parsers/` (with format_detector.rs, jpeg/, tiff/, png/, xmp/, common/), `writers/` (with jpeg_writer.rs, tiff_writer.rs, png_writer.rs, atomic_writer.rs), `io/` (with file_reader.rs, mmap_reader.rs, buffered_reader.rs), `tag_db/` (with tag_registry.rs, generated_tags.rs)
        - Supporting: `error/` (with mod.rs)
    *   Total of **48 Rust source files** exist as placeholder modules with documentation comments and #![allow(dead_code)] directives.
    *   **Recommendation:** The **COMPLETE directory structure** is ALREADY IN PLACE. All files exist as minimal module stubs.

### Critical Issues Identified

*   **File:** `LICENSE`
    *   **Status:** **MISSING** - This file does NOT exist and is **REQUIRED** by the acceptance criteria.
    *   **Recommendation:** You **MUST** create a LICENSE file containing the full text of the GNU General Public License v3.0 (GPL-3.0). The license text can be obtained from https://www.gnu.org/licenses/gpl-3.0.txt

*   **Module Ordering:** Multiple mod.rs files have submodule declarations in non-alphabetical order
    *   **Status:** `cargo fmt --check` **FAILS** with diffs in 10 files showing incorrect module ordering
    *   **Files Affected:**
        - src/cli/mod.rs (line 8-9: output_formatter before batch_processor)
        - src/core/mod.rs (lines 9-15: incorrect ordering of multiple modules)
        - src/io/mod.rs (lines 7-9: mmap_reader before buffered_reader)
        - src/parsers/common/mod.rs (lines 6-7: exif_types before encoding)
        - src/parsers/jpeg/mod.rs (lines 6-9: incorrect ordering)
        - src/parsers/mod.rs (lines 7-12: format_detector before common)
        - src/parsers/tiff/mod.rs (lines 7-9: tag_parser before makernote_parser)
        - src/parsers/xmp/mod.rs (lines 6-7: rdf_parser before namespace_resolver)
        - src/tag_db/mod.rs (lines 6-7: tag_registry before generated_tags)
        - src/writers/mod.rs (lines 6-9: incorrect ordering of multiple modules)
    *   **Recommendation:** You **MUST** run `cargo fmt` to automatically reorder the module declarations alphabetically. This is required for `cargo fmt --check` to pass.

### Implementation Tips & Notes

*   **Tip:** The GPL-3.0 license text is standardized and should be the complete license, not just a reference. You can use `curl -sS https://www.gnu.org/licenses/gpl-3.0.txt > LICENSE` to download it, or copy the full text from the GNU website.

*   **Note:** The project currently builds successfully (`cargo build` completes without errors in 0.42s) and passes clippy with no warnings (`cargo clippy` runs clean). The ONLY issues preventing acceptance are:
    1. Missing LICENSE file
    2. Module declaration ordering (fixable with a single `cargo fmt` command)

*   **Note:** The acceptance criteria state "Directory structure matches Section 3 exactly" - this is **ALREADY SATISFIED**. All directories from the complete directory tree specification exist with appropriate mod.rs files.

*   **Note:** The Cargo.toml specifies both `[[bin]]` and `[lib]` targets, making this both a library crate and a binary crate. This is intentional per the architecture (library API + CLI).

*   **Warning:** The rustfmt.toml file contains some unstable features (wrap_comments, format_code_in_doc_comments, comment_width, normalize_comments) that will generate warnings on stable Rust. These warnings do NOT prevent `cargo fmt` from working correctly. The warnings are expected and acceptable.

*   **Tip:** After running `cargo fmt`, verify the changes with `cargo fmt --check` to ensure it passes. The command should exit with no output and exit code 0 (ignoring the warnings about unstable features).

### Build and Test Commands Status

*   **`cargo build`:** ✅ **PASSES** (completes without errors in 0.42s)
*   **`cargo clippy`:** ✅ **PASSES** (no warnings, clean output)
*   **`cargo fmt --check`:** ❌ **FAILS** (module ordering issues in 10 files - see diffs above)
*   **LICENSE file:** ❌ **MISSING**

### Task Completion Percentage

The project is approximately **95% complete** for Task I1.T1. Almost all work has been done:

**Completed (95%):**
- ✅ Cargo.toml with all dependencies
- ✅ Cargo.lock (auto-generated)
- ✅ src/main.rs (minimal CLI skeleton)
- ✅ src/lib.rs (library root with module declarations)
- ✅ README.md (comprehensive project description)
- ✅ .gitignore (comprehensive)
- ✅ rustfmt.toml (formatting config)
- ✅ .clippy.toml (linting config)
- ✅ All 48 source files in complete directory structure
- ✅ cargo build succeeds
- ✅ cargo clippy runs without warnings

**Remaining (5%):**
- ❌ LICENSE file creation
- ❌ cargo fmt execution to fix module ordering

### Critical Action Items

To complete Task I1.T1, you MUST:

1. **Create LICENSE file** containing the full GPL-3.0 license text
2. **Run `cargo fmt`** to fix the module declaration ordering in 10 files
3. **Verify `cargo fmt --check` passes** (should have no output except unstable feature warnings)

That's it. Everything else is already done and correct.
