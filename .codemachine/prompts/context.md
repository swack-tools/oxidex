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

### Context: directory-structure (from 01_Plan_Overview_and_Setup.md)

```markdown
## 3. Directory Structure

*   **Root Directory:** `exiftool-rs/`

*   **Structure Definition:**

```
exiftool-rs/
├── src/
│   ├── main.rs                          # CLI entry point
│   ├── lib.rs                           # Library crate root
│   │
│   ├── cli/                             # Command-line interface layer
│   │   ├── mod.rs
│   │   ├── args.rs                      # clap argument definitions
│   │   ├── output_formatter.rs          # JSON/CSV/human-readable output
│   │   └── batch_processor.rs           # Recursive directory processing
│   │
│   ├── core/                            # Domain layer (hexagonal core)
│   │   ├── mod.rs
│   │   ├── metadata_map.rs              # MetadataMap struct
│   │   ├── tag_value.rs                 # TagValue enum (String/Number/Binary/etc.)
│   │   ├── tag_descriptor.rs            # TagDescriptor struct
│   │   ├── operations.rs                # Read/Write/Copy/Transform operations
│   │   ├── validation.rs                # Tag value validation engine
│   │   ├── format_parser_trait.rs       # Port: trait FormatParser
│   │   └── file_reader_trait.rs         # Port: trait FileReader
│   │
│   ├── parsers/                         # Infrastructure: Format adapters
│   │   ├── mod.rs
│   │   ├── format_detector.rs           # Magic byte detection
│   │   │
│   │   ├── jpeg/
│   │   │   ├── mod.rs
│   │   │   ├── segment_parser.rs        # JPEG segment marker parsing
│   │   │   ├── exif_parser.rs           # EXIF segment (TIFF IFD)
│   │   │   ├── xmp_parser.rs            # XMP segment (RDF/XML)
│   │   │   └── iptc_parser.rs           # IPTC segment
│   │   │
│   │   ├── tiff/
│   │   │   ├── mod.rs
│   │   │   ├── ifd_parser.rs            # Image File Directory parsing
│   │   │   ├── tag_parser.rs            # TIFF tag extraction
│   │   │   └── makernote_parser.rs      # Vendor-specific maker notes
│   │   │
│   │   ├── png/
│   │   │   ├── mod.rs
│   │   │   └── chunk_parser.rs          # PNG chunk parsing (tEXt, iTXt, etc.)
│   │   │
│   │   ├── xmp/                         # Shared XMP/RDF parser
│   │   │   ├── mod.rs
│   │   │   ├── rdf_parser.rs            # RDF/XML parsing (quick-xml)
│   │   │   └── namespace_resolver.rs    # XMP namespace handling
│   │   │
│   │   └── common/
│   │       ├── exif_types.rs            # EXIF data type definitions
│   │       └── encoding.rs              # String encoding (encoding_rs)
│   │
│   ├── writers/                         # Infrastructure: Metadata serializers
│   │   ├── mod.rs
│   │   ├── jpeg_writer.rs               # JPEG EXIF/XMP segment writing
│   │   ├── tiff_writer.rs               # TIFF IFD serialization
│   │   ├── png_writer.rs                # PNG chunk writing
│   │   └── atomic_writer.rs             # Atomic file write (temp + rename)
│   │
│   ├── io/                              # Infrastructure: I/O abstraction
│   │   ├── mod.rs
│   │   ├── file_reader.rs               # FileReader trait implementation
│   │   ├── mmap_reader.rs               # Memory-mapped file reader (memmap2)
│   │   └── buffered_reader.rs           # Buffered reader for streaming
│   │
│   ├── tag_db/                          # Generated tag database
│   │   ├── mod.rs
│   │   ├── tag_registry.rs              # HashMap<&'static str, TagDescriptor>
│   │   └── generated_tags.rs            # Code-generated from ExifTool specs (build.rs)
│   │
│   ├── error.rs                         # ExifToolError enum
│   └── ffi/                             # C FFI bindings
│       ├── mod.rs
│       └── c_api.rs                     # C-compatible function exports
│
├── tests/
│   ├── integration/                     # Integration tests vs. ExifTool
│   │   ├── jpeg_tests.rs
│   │   ├── tiff_tests.rs
│   │   ├── png_tests.rs
│   │   └── cli_compatibility_tests.rs
│   │
│   ├── property/                        # Property-based tests (proptest)
│   │   └── roundtrip_tests.rs           # Write then read equals original
│   │
│   └── fixtures/                        # Test images
│       ├── jpeg/
│       ├── tiff/
│       ├── png/
│       └── malformed/                   # Intentionally corrupted files
│
├── fuzz/                                # Fuzzing targets (cargo-fuzz)
│   ├── fuzz_targets/
│   │   ├── fuzz_jpeg.rs
│   │   ├── fuzz_tiff.rs
│   │   └── fuzz_png.rs
│   └── Cargo.toml
│
├── benches/                             # Benchmarks (criterion)
│   ├── parse_benchmarks.rs
│   └── batch_benchmarks.rs
│
├── docs/                                # Documentation and design artifacts
│   ├── diagrams/                        # UML diagrams (PlantUML, Mermaid)
│   │   ├── component_architecture.puml
│   │   ├── metadata_erd.mmd
│   │   ├── sequence_metadata_extraction.puml
│   │   └── sequence_metadata_write.puml
│   │
│   ├── api/                             # API specifications
│   │   ├── library_api.md               # Rust library API docs
│   │   └── ffi_api.md                   # C FFI API docs
│   │
│   ├── testing/
│   │   └── integration_test_plan.md
│   │
│   └── adr/                             # Architectural Decision Records (optional)
│       └── 001-hexagonal-architecture.md
│
├── api/                                 # API specification files
│   ├── tag_database_schema.json         # JSON Schema for tag definitions
│   └── exiftool_rs.h                    # C FFI header (cbindgen-generated)
│
├── build.rs                             # Build script (tag database code generation)
├── Cargo.toml                           # Rust project manifest
├── Cargo.lock
├── README.md                            # Project overview, installation, usage
├── LICENSE                              # GPL-3.0 (or compatible)
├── CHANGELOG.md
├── .gitignore
├── .github/
│   └── workflows/
│       ├── ci.yml                       # CI: test, lint, audit
│       ├── release.yml                  # Release: cross-compile, publish binaries
│       └── fuzz.yml                     # Continuous fuzzing
│
├── Dockerfile                           # Optional: minimal Alpine image
└── Cross.toml                           # cross-rs configuration for cross-compilation
```

**Justification for Key Choices:**

*   **`src/core/`**: Isolates domain logic from infrastructure, enforcing hexagonal architecture boundaries. Contains no I/O or format-specific code.
*   **`src/parsers/` organized by format**: Each format is a separate module implementing `FormatParser` trait. Enables parallel development and incremental format addition.
*   **`src/tag_db/generated_tags.rs`**: Code-generated at build time via `build.rs` from ExifTool tag database, ensuring parity without manual maintenance.
*   **`tests/fixtures/`**: Shared test images for integration tests and benchmarks. Organized by format for clarity.
*   **`fuzz/`**: Separate crate for fuzzing (cargo-fuzz requirement). Targets each parser independently.
*   **`docs/diagrams/`**: PlantUML (`.puml`) and Mermaid (`.mmd`) source files for version control and CI rendering.
*   **`api/`**: Machine-readable specifications (JSON Schema, C headers) separate from narrative documentation in `docs/`.
*   **`build.rs`**: Generates tag database code from ExifTool specs before compilation. Parses HTML/source to produce Rust `const` data.
```

---

### Context: technology-stack (from 01_Plan_Overview_and_Setup.md)

```markdown
*   **Technology Stack:**
    *   **Frontend:** None (CLI only for v1.0)
    *   **Backend Language:** Rust 1.75+ (2021 Edition)
    *   **Core Libraries:**
        *   CLI Framework: `clap` v4 (derive API)
        *   Binary Parsing: `nom` v7 (complex formats) + `binrw` (simple struct-based formats)
        *   XML Parsing: `quick-xml` (XMP metadata)
        *   JSON Output: `serde_json`
        *   Date/Time: `chrono`
        *   String Encoding: `encoding_rs`
        *   Concurrency: `rayon` (data parallelism)
        *   Memory-mapped I/O: `memmap2`
    *   **Testing:** `cargo test`, `proptest` (property-based), `cargo-fuzz` (fuzzing)
    *   **C FFI:** `cbindgen` (header generation)
    *   **Documentation:** `rustdoc`, `mdBook`
    *   **Build System:** `cargo` + `cross` (cross-compilation)
    *   **CI/CD:** GitHub Actions
    *   **Code Quality:** `clippy`, `rustfmt`, `cargo-audit`
    *   **Benchmarking:** `criterion`
    *   **Database:** None (file-based, stateless operation)
    *   **Messaging/Queues:** None (synchronous processing)
    *   **Deployment:** Static binaries, Rust crate (crates.io), optional Docker image
```

---

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

**Dependency Philosophy**:
- **Minimize Count**: Target < 50 direct dependencies to reduce supply chain risk
- **Prefer `no_std` Compatible**: Where possible (e.g., `nom`, `binrw`) to enable future embedded/WASM use
- **Audit Regularly**: `cargo-audit` in CI pipeline to catch vulnerabilities in transitive dependencies
```

---

### Context: project-vision (from 01_Context_and_Drivers.md)

```markdown
### 1.1. Project Vision

ExifTool-RS aims to create a modern, high-performance Rust reimplementation of the industry-standard ExifTool metadata management library and command-line application. The goal is to provide a memory-safe, zero-cost abstraction alternative that maintains full compatibility with ExifTool's extensive metadata tag support while delivering superior performance, native cross-compilation capabilities, and seamless integration into modern software ecosystems.
```

---

### Context: key-objectives (from 01_Context_and_Drivers.md)

```markdown
### 1.2. Key Objectives

- **Feature Parity**: Support reading, writing, and editing metadata for 300+ file formats with 28,000+ recognized metadata tags
- **Performance**: Achieve 2-5x performance improvement over Perl implementation through zero-cost abstractions and parallel processing
- **Memory Safety**: Eliminate entire classes of vulnerabilities (buffer overflows, use-after-free) through Rust's ownership system
- **Binary Distribution**: Provide static, self-contained binaries with no runtime dependencies (unlike Perl)
- **API-First Design**: Expose both a command-line interface and a native Rust library with C FFI bindings for cross-language integration
- **Maintainability**: Create a modular, well-documented codebase that encourages community contributions
- **Backward Compatibility**: Maintain CLI argument compatibility with original ExifTool for drop-in replacement scenarios
- **Cross-Platform**: Support Windows, Linux, macOS, and WebAssembly targets from a single codebase
```

---

### Context: architectural-style (from 02_Architecture_Overview.md)

```markdown
### 3.1. Architectural Style

**Primary Style**: **Layered Hexagonal Architecture** (Ports and Adapters)

**Rationale**:

The Hexagonal Architecture pattern is optimal for ExifTool-RS because:

1. **Format Independence**: The "core domain" (metadata extraction/manipulation logic) must remain isolated from the specifics of 300+ file formats. Hexagonal architecture enforces this separation through ports (interfaces) and adapters (format-specific implementations).

2. **Multiple Access Patterns**: The system must expose:
   - CLI interface (primary port)
   - Rust library API (primary port)
   - C FFI bindings (primary port)
   - Format parsers (secondary ports)
   - File system access (secondary port)

   This multiplicity of interfaces aligns perfectly with the ports/adapters model.

3. **Testability**: Hexagonal architecture enables testing the core metadata logic independently of file I/O by mocking the file system port. Critical for achieving 80%+ test coverage.

4. **Extensibility**: New file format support becomes a matter of implementing the format adapter interface without touching core logic. Supports phased rollout strategy (50 formats in v1.0, expanding to 300+).

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

- **Domain Layer**: Format-agnostic metadata models, tag definitions, operations (read/write/copy/transform)
- **Application Layer**: User-facing interfaces translating commands to domain operations
- **Infrastructure Layer**: Format-specific parsers/serializers, file system abstraction, configuration
```

---

### Context: iteration-1-goal (from 02_Iteration_I1.md)

```markdown
### Iteration 1: Foundation, Architecture Setup & Core Infrastructure

*   **Iteration ID:** `I1`
*   **Goal:** Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.
*   **Prerequisites:** None (initial iteration)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `.gitignore`
    *   **Summary:** This file contains basic gitignore rules for CodeMachine directories, Node modules, environment files, OS files, and IDE files.
    *   **Recommendation:** You MUST extend this .gitignore file to include Rust-specific patterns. Add entries for: `/target/` (build artifacts), `Cargo.lock` (for libraries, keep it for binaries), `**/*.rs.bk` (rustfmt backups), and any other Rust-specific patterns mentioned in the standard Rust .gitignore template.

*   **Project State:** BRAND NEW PROJECT - NO RUST CODE EXISTS YET
    *   **Summary:** The repository currently contains only the .codemachine directory with architecture/plan documents and a basic .gitignore file. This is a completely greenfield project.
    *   **Recommendation:** You are creating the ENTIRE Rust project structure from scratch. Follow the directory structure from Section 3 of the plan document EXACTLY as specified.

### Implementation Tips & Notes

*   **Critical First Step - Initialize Cargo Project:**
    *   Run `cargo init --name exiftool-rs` in the current directory to create the basic Rust project structure (Cargo.toml, src/main.rs, src/lib.rs).
    *   This will create the foundational files that you'll then customize according to the architecture.

*   **Directory Creation Strategy:**
    *   After running `cargo init`, you MUST create all subdirectories as specified in the directory structure section.
    *   Create each directory with at least a `mod.rs` file to make it a valid Rust module.
    *   Empty `mod.rs` files are acceptable for now - they'll be populated in future iterations.

*   **Cargo.toml Configuration:**
    *   The task explicitly lists these dependencies that MUST be included: `clap`, `nom`, `serde_json`, `chrono`, `memmap2`, `rayon`, `quick-xml`, `encoding_rs`
    *   Use version constraints as specified in the technology stack (e.g., clap v4, nom v7)
    *   Configure both `[dependencies]` and `[dev-dependencies]` sections
    *   Add `[[bin]]` section for the CLI and `[lib]` section for the library
    *   Set edition = "2021" as specified in the architecture
    *   Add metadata fields: name, version (start with 0.1.0), authors, description, license (GPL-3.0), repository

*   **Configuration Files:**
    *   **rustfmt.toml:** Create a configuration file for code formatting. Include settings like `max_width = 100`, `edition = "2021"`, and other standard formatting rules.
    *   **.clippy.toml:** Configure clippy linting rules. This should be strict - deny common warnings and enforce best practices.

*   **README.md Content:**
    *   MUST include the project description from the architecture vision
    *   Include a basic installation section (placeholder for now)
    *   Include basic usage examples (can be TODO/placeholder)
    *   Add badges for CI status (will be activated later)
    *   Reference the ExifTool project with proper attribution

*   **LICENSE File:**
    *   The architecture specifies GPL-3.0 licensing
    *   You can use the standard GPL-3.0 license text
    *   Add appropriate copyright header with current year

*   **Module Structure in src/lib.rs:**
    *   Declare all the major modules: `pub mod cli;`, `pub mod core;`, `pub mod parsers;`, etc.
    *   Each module declaration should correspond to a directory in src/
    *   Some modules might need to be marked as public, others private - follow Rust best practices

*   **Minimal src/main.rs:**
    *   Should have a basic `fn main()` that compiles but doesn't do much yet
    *   Can print a simple message like "ExifTool-RS v0.1.0" for now
    *   Import the library crate: `use exiftool_rs;` (using the crate name from Cargo.toml)

*   **Acceptance Criteria Validation:**
    *   After creating all files, you MUST verify:
        1. `cargo build` completes successfully
        2. `cargo clippy` runs without warnings
        3. `cargo fmt --check` passes
    *   If any of these fail, fix the issues before completing the task

*   **Best Practice - Add Initial Documentation:**
    *   Add doc comments (`///`) to main.rs and lib.rs explaining the purpose
    *   This sets a good precedent for documentation-first development

*   **gitignore Rust Patterns:**
    *   Add these essential Rust patterns to the existing .gitignore:
        ```
        # Rust build artifacts
        /target/
        **/*.rs.bk
        *.pdb

        # Cargo.lock (keep for binary crates, ignore for libraries)
        # Since this is both a binary and library, we should keep it
        # Cargo.lock
        ```

*   **Important Note on Dependencies:**
    *   Some dependencies mentioned (like `proptest`, `criterion`, `cargo-fuzz`) are for testing/benchmarking and should go in `[dev-dependencies]` or separate sections
    *   The task specifically mentions the runtime dependencies to include in `[dependencies]`

*   **Cross-Compilation Setup (Cross.toml):**
    *   Create a basic Cross.toml file with target specifications
    *   This will be used later for cross-platform builds

*   **Workflow - Recommended Order of Operations:**
    1. Run `cargo init --name exiftool-rs`
    2. Update .gitignore with Rust patterns
    3. Create all directories from the structure diagram (use mkdir -p)
    4. Create empty mod.rs files in each directory
    5. Configure Cargo.toml with all dependencies and metadata
    6. Create rustfmt.toml and .clippy.toml
    7. Update src/lib.rs with module declarations
    8. Update src/main.rs with minimal working code
    9. Create README.md with project description
    10. Create LICENSE file with GPL-3.0 text
    11. Run `cargo build` to verify everything compiles
    12. Run `cargo clippy` to check for warnings
    13. Run `cargo fmt --check` to verify formatting

*   **Expected Issues and Solutions:**
    *   **Issue:** Empty directories might cause module resolution errors
        *   **Solution:** Ensure every directory has at least an empty `mod.rs` file
    *   **Issue:** Clippy might complain about unused imports or dead code in minimal stubs
        *   **Solution:** Use `#[allow(dead_code)]` and `#[allow(unused_imports)]` attributes temporarily
    *   **Issue:** Version conflicts between dependencies
        *   **Solution:** Use `cargo update` and check compatibility; the specified versions should work together

---

## Strategic Implementation Approach

This is the FOUNDATION task for the entire project. Everything else depends on getting this right. Your implementation should:

1. **Be Meticulous About Structure:** The directory structure MUST match Section 3 exactly. Future tasks depend on files being in the right places.

2. **Use Conservative Dependency Versions:** Stick to the specified versions (clap v4, nom v7, etc.) to ensure compatibility.

3. **Create a Compiling, Linting-Clean Project:** The acceptance criteria are strict - no build errors, no clippy warnings, properly formatted code. This sets the quality bar for the entire project.

4. **Think About the Hexagonal Architecture:** Even though you're just creating stubs, organize the code to reflect the three layers: Application (cli), Domain (core), and Infrastructure (parsers, writers, io).

5. **Document as You Go:** Add doc comments to set expectations for what each module will contain.

The Coder Agent should approach this as "building the skeleton that will hold the entire body of the application." Every directory, every empty module, every dependency declaration is intentional and will be filled in during subsequent iterations.

This task creates the foundation. Do it right, and everything else will fall into place smoothly.
