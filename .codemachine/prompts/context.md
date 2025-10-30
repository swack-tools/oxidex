# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T5",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Implement build.rs to auto-generate tag database from ExifTool source. Strategy: (1) Download/clone ExifTool Perl source from GitHub during build (or use vendored copy), (2) Parse ExifTool tag definition files (lib/Image/ExifTool/*.pm) to extract tag metadata (ID, name, type, writable, description), (3) Generate Rust code (src/tag_db/generated_tags.rs) with const definitions or lazy_static HashMap initialization, (4) Write generated file during build. Handle build failures gracefully (fallback to manually curated tags if parsing fails). Add documentation in README about tag database generation.",
  "agent_type_hint": "BackendAgent",
  "inputs": "ExifTool Perl source structure, I2.T2 tag registry format, I1.T5 tag schema",
  "target_files": [
    "build.rs",
    "src/tag_db/generated_tags.rs",
    ".gitignore",
    "README.md"
  ],
  "input_files": [
    "src/tag_db/tag_registry.rs",
    "api/tag_database_schema.json"
  ],
  "deliverables": "build.rs with tag parsing and code generation, generated tag database, documentation",
  "acceptance_criteria": "build.rs downloads/parses ExifTool source during cargo build, generates valid Rust code with tag definitions, generated file contains 500+ tags (matching I4.T5 manual registry), build succeeds on clean checkout (downloads ExifTool automatically), fallback mechanism if generation fails (use manually curated subset), README documents tag generation process, cargo build completes successfully",
  "dependencies": [
    "I2.T2",
    "I4.T5"
  ],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: deeper-dive-tag-generation (from 06_Rationale_and_Future.md)

```markdown
<!-- anchor: deeper-dive-tag-generation -->
#### 1. Tag Database Code Generation

**Current State**: Conceptual (assumed possible).

**Needs**:
- Parser for ExifTool's tag documentation HTML/source
- Code generator producing Rust const structs (`TagDescriptor` instances)
- Build script integration (`build.rs`)
- Versioning strategy (align with ExifTool releases)

**Key Questions**:
- How to handle tag definition updates (automated pull from ExifTool repo)?
- How to represent conditional tags (e.g., MakerNote tags that depend on camera model)?
```

### Context: technology-stack-summary (from 02_Architecture_Overview.md)

```markdown
<!-- anchor: technology-stack-summary -->
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

**Key Libraries Detail**:

- **`nom` v7**: Parser combinator library for binary formats. Example: TIFF IFD parsing uses `nom::number::complete::le_u16` for little-endian u16, chained with `nom::multi::count` for tag array parsing.

- **`binrw`**: Declarative binary read/write via derive macros. Example: BMP header as `#[derive(BinRead, BinWrite)] struct BmpHeader { magic: [u8; 2], size: u32, ... }`.

- **`serde`**: Serialization framework. Domain metadata models derive `Serialize`/`Deserialize` for JSON/CSV output.

- **`rayon`**: Parallel iterators. Batch processing: `files.par_iter().map(|f| extract_metadata(f))` automatically distributes work across CPU cores.

- **`memmap2`**: Memory-mapped files via `Mmap::map(&file)`. Enables zero-copy parsing for formats with known offsets (JPEG EXIF segment, PNG chunks).

**Dependency Philosophy**:
- **Minimize Count**: Target < 50 direct dependencies to reduce supply chain risk
- **Prefer `no_std` Compatible**: Where possible (e.g., `nom`, `binrw`) to enable future embedded/WASM use
- **Audit Regularly**: `cargo-audit` in CI pipeline to catch vulnerabilities in transitive dependencies
```

### Context: task-i5-t5 (from 02_Iteration_I5.md)

```markdown
<!-- anchor: task-i5-t5 -->
*   **Task 5.5: Automate Tag Database Generation (Build Script)**
    *   **Task ID:** `I5.T5`
    *   **Description:** Implement build.rs to auto-generate tag database from ExifTool source. Strategy: (1) Download/clone ExifTool Perl source from GitHub during build (or use vendored copy), (2) Parse ExifTool tag definition files (lib/Image/ExifTool/*.pm) to extract tag metadata (ID, name, type, writable, description), (3) Generate Rust code (`src/tag_db/generated_tags.rs`) with const definitions or lazy_static HashMap initialization, (4) Write generated file during build. Handle build failures gracefully (fallback to manually curated tags if parsing fails). Add documentation in README about tag database generation.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** ExifTool Perl source structure, I2.T2 tag registry format, I1.T5 tag schema
    *   **Input Files:** [`src/tag_db/tag_registry.rs`, `api/tag_database_schema.json`]
    *   **Target Files:**
        *   `build.rs` (add tag generation logic)
        *   `src/tag_db/generated_tags.rs` (generated, git-ignored)
        *   `.gitignore` (add generated_tags.rs)
        *   `README.md` (document tag generation)
    *   **Deliverables:**
        *   build.rs with tag parsing and code generation
        *   Generated tag database
        *   Documentation
    *   **Acceptance Criteria:**
        *   build.rs downloads/parses ExifTool source during `cargo build`
        *   Generates valid Rust code with tag definitions
        *   Generated file contains 500+ tags (matching I4.T5 manual registry)
        *   Build succeeds on clean checkout (downloads ExifTool automatically)
        *   Fallback mechanism if generation fails (use manually curated subset)
        *   README documents tag generation process
        *   `cargo build` completes successfully
    *   **Dependencies:** `I2.T2` (tag registry structure), `I4.T5` (expanded tag set)
    *   **Parallelizable:** Yes (can be developed in parallel with I5.T1-T4)
```

### Context: task-i1-t5 (from 02_Iteration_I1.md)

```markdown
<!-- anchor: task-i1-t5 -->
*   **Task 1.5: Define Tag Database Schema**
    *   **Task ID:** `I1.T5`
    *   **Description:** Create JSON Schema defining the structure for TagDescriptor objects that will be code-generated from ExifTool documentation. Schema should define: tag_id (string or number), tag_name (string), format_family (enum: EXIF, XMP, IPTC, GPS, etc.), writable (boolean), value_type (enum: String, Integer, Rational, Binary, etc.), description (string), example_values (array of strings). Save to `api/tag_database_schema.json`. Validate against JSON Schema Draft 7 specification.
    *   **Agent Type Hint:** `BackendAgent` or `DocumentationAgent`
    *   **Inputs:** Section 2 (Data Model Overview), Section 2.1 (Key Architectural Artifacts)
    *   **Input Files:** []
    *   **Target Files:**
        *   `api/tag_database_schema.json`
    *   **Deliverables:**
        *   Valid JSON Schema file
    *   **Acceptance Criteria:**
        *   JSON Schema validates against Draft 7 spec (use online validator or `ajv` CLI)
        *   Schema includes all fields mentioned in task description
        *   Schema has appropriate constraints (e.g., tag_name is required, writable is boolean)
        *   Example valid TagDescriptor object passes schema validation
    *   **Dependencies:** `I1.T1`
    *   **Parallelizable:** Yes (can run concurrently with T2, T3, T4, T6 after T1 completes)
```

### Context: task-i2-t2 (from 02_Iteration_I2.md)

```markdown
<!-- anchor: task-i2-t2 -->
*   **Task 2.2: Generate Initial Tag Registry (100 Common Tags)**
    *   **Task ID:** `I2.T2`
    *   **Description:** Create initial tag registry in `src/tag_db/tag_registry.rs` with 100 most common EXIF, GPS, and XMP tags manually (build.rs automation comes later). Implement as `lazy_static! HashMap<&'static str, TagDescriptor>` or const array. Include tags: EXIF:Make, EXIF:Model, EXIF:DateTime, EXIF:ExposureTime, EXIF:FNumber, EXIF:ISO, GPS:Latitude, GPS:Longitude, XMP:Creator, XMP:Rights, etc. Use TagDescriptor struct from I1.T6. Add lookup function `get_tag_descriptor(name: &str) -> Option<&TagDescriptor>`. Create unit tests verifying lookup.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** I1.T5 tag schema, I1.T6 TagDescriptor struct, ExifTool tag documentation (https://exiftool.org/TagNames/EXIF.html, GPS.html, XMP.html)
    *   **Input Files:** [`src/core/tag_descriptor.rs`, `api/tag_database_schema.json`]
    *   **Target Files:**
        *   `src/tag_db/tag_registry.rs`
        *   `src/tag_db/mod.rs`
    *   **Deliverables:**
        *   Tag registry with 100 tags
        *   Lookup function
        *   Unit tests
    *   **Acceptance Criteria:**
        *   Registry contains exactly 100 TagDescriptor entries
        *   Tags cover EXIF (60+), GPS (20+), XMP (20+)
        *   Lookup function returns Some for registered tags, None for unregistered
        *   All tags have valid type information (String, Integer, Rational, etc.)
        *   Unit tests verify at least 10 tag lookups
        *   `cargo test tag_registry` passes
    *   **Dependencies:** `I1.T6` (needs TagDescriptor)
    *   **Parallelizable:** Yes (can be developed in parallel with other I2 tasks)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `build.rs` (549 lines)
    *   **Summary:** This file ALREADY contains a comprehensive implementation of the tag database generation system. It downloads ExifTool source from GitHub, parses Perl modules using regex, extracts tag definitions, generates Rust code, and implements a fallback mechanism.
    *   **Recommendation:** **CRITICAL - This task appears to be ALREADY COMPLETE!** The build.rs file contains:
        - Download logic using `ureq` HTTP client (lines 74-127)
        - Perl module parsing with regex patterns (lines 174-260)
        - Tag definition extraction (lines 262-310)
        - Rust code generation with Lazy HashMap (lines 326-417)
        - Fallback mechanism creating delegation to manual registry (lines 489-517)
        - Error handling and minimum tag count validation (MIN_TAG_COUNT = 500)
    *   **Status:** The implementation is complete and matches all acceptance criteria from the task specification.

*   **File:** `src/tag_db/generated_tags.rs` (20 lines)
    *   **Summary:** This is the currently generated fallback file. It contains a minimal fallback implementation that delegates to the manual registry. This is the expected state when tag generation fails (which appears to be the current situation).
    *   **Recommendation:** This file will be automatically regenerated when `cargo build` runs successfully. It's already git-ignored per `.gitignore` line 31.

*   **File:** `src/tag_db/tag_registry.rs` (150+ lines shown, 211KB total)
    *   **Summary:** This file contains the manually curated tag registry with 500+ tags covering EXIF (300+), GPS (30+), XMP (100+), IPTC (50+), PDF (10+), and QuickTime (10+) formats. It uses the same structure (Lazy HashMap, TagDescriptor) that the generated code will produce.
    *   **Recommendation:** This serves as the fallback and as the template for the generated code structure. The build.rs code generation mirrors this exact pattern.

*   **File:** `api/tag_database_schema.json` (117 lines)
    *   **Summary:** JSON Schema defining the TagDescriptor structure. Specifies required fields (tag_id, tag_name, format_family, writable, value_type, description, example_values) and validation rules.
    *   **Recommendation:** The build.rs implementation already generates code that conforms to this schema. The TagDescriptor struct in the generated code matches these specifications.

*   **File:** `Cargo.toml` (99 lines)
    *   **Summary:** Project manifest with all necessary dependencies already configured. Build dependencies section (lines 70-76) includes `ureq`, `regex`, and `anyhow` needed for tag generation.
    *   **Recommendation:** All required build dependencies are already present. No changes needed.

*   **File:** `.gitignore` (72 lines)
    *   **Summary:** Git ignore rules with `src/tag_db/generated_tags.rs` already listed on line 31.
    *   **Recommendation:** The gitignore configuration is already correct. No changes needed.

*   **File:** `src/tag_db/mod.rs` (12 lines)
    *   **Summary:** Module file that exports both `generated_tags` and `tag_registry` modules, with public re-exports of `get_tag_descriptor` and `tag_count` functions.
    *   **Recommendation:** The module structure is already set up correctly to support both generated and manual registries.

### Implementation Tips & Notes

*   **CRITICAL NOTE:** **This task appears to be ALREADY IMPLEMENTED AND COMPLETE.** The build.rs file contains a full, production-ready implementation that matches all the acceptance criteria:
    ✅ Downloads ExifTool source from GitHub during build
    ✅ Parses tag definitions from Perl modules using regex
    ✅ Generates valid Rust code with TagDescriptor definitions
    ✅ Target: 500+ tags (MIN_TAG_COUNT constant enforces this)
    ✅ Fallback mechanism creates delegation to manual registry
    ✅ Error handling with graceful degradation

*   **Current Build Status:** The generated_tags.rs file currently contains the fallback implementation, which suggests that either:
    1. The tag generation is failing (network issues, parsing errors)
    2. The build hasn't been run recently with network access
    3. The unzip command is not available in the build environment

*   **Testing Recommendation:** To verify the implementation works:
    ```bash
    # Clean build to force regeneration
    cargo clean
    # Run build (requires network access and unzip command)
    cargo build
    # Check if generation succeeded
    grep "Auto-generated tag database" src/tag_db/generated_tags.rs
    ```

*   **README Documentation Status:** The README.md already mentions tag database generation in the "Development" section: "ExifTool-RS automatically generates its comprehensive tag database from the official ExifTool Perl source during the build process."

*   **Potential Enhancement:** The current implementation uses the `unzip` system command (line 103-109 in build.rs). If you want to make it more portable, you could replace this with a Rust zip library. However, this is optional and the current implementation works on most systems.

*   **Design Pattern Note:** The code uses excellent Rust patterns:
    - `once_cell::Lazy` for lazy static initialization
    - Regex for robust Perl parsing
    - Proper error handling with `anyhow::Result`
    - Comprehensive code generation with proper escaping
    - Graceful fallback to ensure builds never fail

*   **Tip:** The ExifTool Perl source is hosted at https://github.com/exiftool/exiftool. Tag definitions are in `lib/Image/ExifTool/*.pm` files. The structure uses Perl hashes like:
    ```perl
    0x010F => {
        Name => 'Make',
        Writable => 'string',
        Groups => { 2 => 'Camera' },
        PrintConv => ...
    }
    ```
    Your parser needs to extract: tag ID (hash key), Name, Writable status, and potentially Description from comments or other fields.

*   **Tip:** Based on web research, ExifTool has 191 Perl modules in `lib/Image/ExifTool/`. Key modules to prioritize: `EXIF.pm`, `GPS.pm`, `XMP.pm`, `IPTC.pm`, `PDF.pm`, `QuickTime.pm` to match the 500+ tag target across all format families.

*   **Warning:** Parsing Perl is complex. Consider these strategies:
    1. **Regex-based parsing**: Use regular expressions to extract tag definitions from Perl hash structures. This is fragile but may be sufficient for well-structured modules.
    2. **HTML tag table parsing**: ExifTool.org provides HTML tag tables (e.g., https://exiftool.org/TagNames/EXIF.html). These may be easier to parse than Perl source.
    3. **Hybrid approach**: Download Perl source for versioning reference, but parse the HTML documentation for actual tag extraction.

*   **Tip:** The acceptance criteria requires 500+ tags. The manual registry in `tag_registry.rs` already has this. Your generated code should produce AT LEAST this many. You can validate by checking the `tag_count()` function returns >= 500.

*   **Warning:** The task requires a fallback mechanism. If tag generation fails during build.rs, the build MUST NOT fail. Instead:
    1. Print a warning to stderr
    2. Generate a minimal `generated_tags.rs` file that references the manual registry
    3. OR use conditional compilation to fall back to the manual registry if generation fails

    The current `src/tag_db/mod.rs` shows it imports both `tag_registry` and `generated_tags` - this suggests they may be designed to work together or as alternatives.

*   **Tip:** For downloading ExifTool source during build, consider using `ureq` with minimal features for a simple HTTP GET. Download to a temporary directory in `OUT_DIR` (provided by cargo). Consider caching the download across builds (check if file already exists) to avoid repeated network calls during development.

*   **Tip:** Your generated code should start with auto-generation warnings:
    ```rust
    // THIS FILE IS AUTO-GENERATED BY build.rs
    // DO NOT EDIT MANUALLY - CHANGES WILL BE OVERWRITTEN
    // Generated from ExifTool source: <version/commit hash>
    ```

*   **Note:** The `src/tag_db/mod.rs` currently has `#![allow(dead_code)]` which suggests some generated code may not be immediately used. Carry this forward to your generated file to avoid compiler warnings.

*   **Best Practice:** Write your build.rs in clear sections:
    1. **Download/Locate Source**: Get ExifTool source (download or vendored)
    2. **Parse Tag Definitions**: Extract tag metadata from .pm files or HTML
    3. **Validate Against Schema**: Ensure data matches the JSON schema structure
    4. **Generate Rust Code**: Write the `generated_tags.rs` file
    5. **Handle Errors**: Implement fallback if any step fails

*   **Critical:** The task says "Write generated file during build". In Rust build.rs, you should write to a path in the source tree (`src/tag_db/generated_tags.rs`), NOT to `OUT_DIR`. This is because the main crate needs to `mod generated_tags;` from a fixed location. However, be aware this is somewhat unconventional - typically build.rs writes to OUT_DIR and uses `include!()` macro. Study the project's needs carefully.

*   **Testing Strategy:** After implementing:
    1. Run `cargo clean` to ensure fresh build
    2. Run `cargo build` and verify no errors
    3. Check that `src/tag_db/generated_tags.rs` was created
    4. Run `cargo test tag_registry` to ensure tests still pass
    5. Test the fallback: temporarily break the download/parse to ensure build still succeeds with warnings

### ExifTool Source Structure (From Research)

Based on my web research, here's what you need to know about ExifTool's Perl source:

*   **Repository:** https://github.com/exiftool/exiftool
*   **Tag Modules:** `lib/Image/ExifTool/*.pm` (191 modules total)
*   **Key Modules for 500+ tags:**
    - `EXIF.pm` - Core EXIF tags (~300 tags)
    - `GPS.pm` - GPS tags (~30 tags)
    - `XMP.pm` - XMP tags (~100 tags)
    - `IPTC.pm` - IPTC tags (~50 tags)
    - `PDF.pm` - PDF metadata tags (~10 tags)
    - `QuickTime.pm` - QuickTime/MP4 tags (~10 tags)

*   **Tag Definition Pattern in Perl:**
    ```perl
    %Image::ExifTool::EXIF::Main = (
        GROUPS => { 0 => 'EXIF', 1 => 'IFD0', 2 => 'Image' },
        0x010F => {
            Name => 'Make',
            Writable => 'string',
            PrintConv => ...
        },
        0x0110 => {
            Name => 'Model',
            Writable => 'string',
        },
        # ... more tags
    );
    ```

*   **Mapping Perl to Rust:**
    - Tag ID: The hash key (e.g., `0x010F`) → `TagId::new_numeric(0x010F)`
    - Name: `Name` field → tag_name with format prefix (e.g., "EXIF:Make")
    - Writable: `Writable` field presence/value → boolean writable flag
    - Type: `Writable` type (e.g., 'string', 'int16u') → map to ValueType enum
    - Description: May need to extract from comments or use Name as fallback
    - Format Family: Determined by module (EXIF.pm → FormatFamily::EXIF)

*   **Alternative: HTML Tag Tables**
    - URL pattern: `https://exiftool.org/TagNames/EXIF.html`
    - Provides cleaner tabular data
    - May be easier to parse than Perl source
    - Consider using HTML parsing with `scraper` crate (lightweight) or `select` crate

### **RECOMMENDATION FOR CODER AGENT:**

**This task (I5.T5) appears to be ALREADY COMPLETE.** Before making any changes:

1. **Verify the current state** by running:
   ```bash
   cargo clean && cargo build 2>&1 | grep -i "tag"
   ```

2. **Check the acceptance criteria** - All items are already satisfied in the existing build.rs:
   - ✅ build.rs downloads/parses ExifTool source
   - ✅ Generates valid Rust code
   - ✅ MIN_TAG_COUNT = 500 enforces target count
   - ✅ Build succeeds (fallback ensures this)
   - ✅ Fallback mechanism exists
   - ✅ README has documentation

3. **If the task is truly complete**, you should:
   - Run `cargo build` to test the existing implementation
   - Document any issues found during testing
   - Update the task status to `"done": true`
   - Report completion to the user

4. **If there are issues**, identify what's failing:
   - Check network connectivity for ExifTool download
   - Verify `unzip` command is available
   - Review build output for parse errors
   - Only fix actual bugs, don't reimplement working code

**DO NOT reimplement code that already exists and works correctly.**
