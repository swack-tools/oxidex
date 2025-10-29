# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I1.T2",
  "iteration_id": "I1",
  "iteration_goal": "Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.",
  "description": "Create PlantUML C4 Component diagram showing hexagonal architecture: core library components (API Facade, Metadata Model, Operations, Tag Registry, Validation Engine), ports (FormatParser trait, FileReader trait), and infrastructure adapters (JPEG Parser, TIFF Parser, XMP Parser, MMap Reader). Save to docs/diagrams/component_architecture.puml. Include legend and layout directives for clarity.",
  "agent_type_hint": "DocumentationAgent",
  "inputs": "Section 2 (Core Architecture), Section 2.1 (Key Architectural Artifacts), Component Diagram specification from architecture blueprint",
  "target_files": [
    "docs/diagrams/component_architecture.puml"
  ],
  "input_files": [
    ".codemachine/artifacts/architecture/01_Context_and_Drivers.md",
    ".codemachine/artifacts/architecture/02_Architecture_Overview.md",
    ".codemachine/artifacts/architecture/03_System_Structure_and_Data.md"
  ],
  "deliverables": "PlantUML file rendering valid C4 component diagram, diagram shows all components listed in Section 2.1",
  "acceptance_criteria": "PlantUML file compiles without syntax errors (validate with plantuml -tsvg component_architecture.puml), diagram accurately reflects hexagonal architecture layers from Section 2, all components from architecture blueprint are present, clear visual separation between domain layer, ports, and adapters",
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

### Context: architectural-style (from 02_Architecture_Overview.md)

```markdown
<!-- anchor: architectural-style -->
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

### Context: component-diagram (from 03_System_Structure_and_Data.md)

```markdown
<!-- anchor: component-diagram -->
### 3.5. Component Diagram(s) (C4 Level 3)

**Description**: This diagram details the internal components of the **Core Library** container, showing the hexagonal architecture layers and their interactions.

**Diagram (PlantUML - Core Library Components)**:

```plantuml
@startuml
!include https://raw.githubusercontent.com/plantuml-stdlib/C4-PlantUML/master/C4_Component.puml

LAYOUT_WITH_LEGEND()

title Component Diagram - Core Library (exiftool-rs)

Container_Boundary(core_lib, "Core Library") {

  Component(api_facade, "Public API Facade", "Rust modules", "User-facing API: extract(), write(), copy_metadata()")

  ' Domain Layer
  Component(metadata_model, "Metadata Model", "Rust structs/enums", "TagValue, MetadataMap, TagDescriptor")
  Component(operations, "Metadata Operations", "Rust traits/impls", "Read, Write, Copy, Transform operations")
  Component(tag_registry, "Tag Registry", "Generated const maps", "28K+ tag definitions indexed by ID/name")
  Component(validation, "Validation Engine", "Rust", "Tag value type checking, range validation")

  ' Ports (interfaces)
  Component(format_port, "Format Parser Port", "Rust trait", "trait FormatParser { fn parse(&self, ...) -> Result<MetadataMap> }")
  Component(io_port, "I/O Port", "Rust trait", "trait FileReader { fn read(&self, offset, len) -> Result<&[u8]> }")

  ' Infrastructure adapters (in other containers but shown for clarity)
  Component_Ext(jpeg_adapter, "JPEG Parser", "nom-based", "EXIF/JFIF segment parser")
  Component_Ext(tiff_adapter, "TIFF Parser", "nom-based", "IFD structure parser")
  Component_Ext(xmp_adapter, "XMP Parser", "quick-xml", "RDF/XML parser for XMP")
  Component_Ext(mmap_adapter, "MMap Reader", "memmap2", "Memory-mapped file access")
}

Rel(api_facade, operations, "Orchestrates")
Rel(operations, metadata_model, "Manipulates")
Rel(operations, tag_registry, "Looks up tag definitions")
Rel(operations, format_port, "Calls")
Rel(operations, validation, "Validates values via")

Rel(format_port, jpeg_adapter, "Implemented by")
Rel(format_port, tiff_adapter, "Implemented by")
Rel(format_port, xmp_adapter, "Implemented by")

Rel(jpeg_adapter, io_port, "Reads via")
Rel(tiff_adapter, io_port, "Reads via")
Rel(io_port, mmap_adapter, "Implemented by")

@enduml
```
```

### Context: artifact-component-diagram (from 01_Plan_Overview_and_Setup.md)

```markdown
<!-- anchor: artifact-component-diagram -->
*   **Component Diagram (PlantUML)**
    *   **Purpose:** Visualize hexagonal architecture layers showing domain components, ports (interfaces), and adapters (implementations)
    *   **Format:** PlantUML (C4 Component diagram)
    *   **Location:** `docs/diagrams/component_architecture.puml`
    *   **Created In:** Iteration 1, Task 2
    *   **Content:** Core library components (API facade, metadata model, operations, tag registry), ports (FormatParser trait, I/O trait), and adapter examples (JPEG parser, TIFF parser, XMP parser)
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

**Key Libraries Detail**:

- **`nom` v7**: Parser combinator library for binary formats. Example: TIFF IFD parsing uses `nom::number::complete::le_u16` for little-endian u16, chained with `nom::multi::count` for tag array parsing.
```

### Context: directory-structure (from 01_Plan_Overview_and_Setup.md)

```markdown
<!-- anchor: directory-structure -->
## 3. Directory Structure

<!-- anchor: root-directory -->
*   **Root Directory:** `exiftool-rs/`

<!-- anchor: structure-definition -->
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
│   │   ├── xmp/                         # Shared XMP/RDF parser
│   │   │   ├── mod.rs
│   │   │   ├── rdf_parser.rs            # RDF/XML parsing (quick-xml)
│   │   │   └── namespace_resolver.rs    # XMP namespace handling
```

**Justification for Key Choices:**

*   **`src/core/`**: Isolates domain logic from infrastructure, enforcing hexagonal architecture boundaries. Contains no I/O or format-specific code.
*   **`src/parsers/` organized by format**: Each format is a separate module implementing `FormatParser` trait. Enables parallel development and incremental format addition.
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/lib.rs`
    *   **Summary:** This is the library crate root that defines the overall module structure. It documents the three-layer hexagonal architecture: Application Layer (cli, ffi), Domain Layer (core), and Infrastructure Layer (parsers, writers, io). The file has proper module exports for all major components.
    *   **Recommendation:** You SHOULD reference this file when determining the structure of the component diagram. The module organization directly maps to the hexagonal architecture layers that need to be visualized.

*   **File:** `src/core/mod.rs`
    *   **Summary:** This module declaration lists all the core domain layer components: file_reader_trait, format_parser_trait, metadata_map, operations, tag_descriptor, tag_value, and validation. These are the central components of the hexagonal architecture's domain layer.
    *   **Recommendation:** You MUST include all these components in the domain layer of your component diagram. These map directly to the components mentioned in the architecture blueprint (Metadata Model, Operations, Validation Engine, and the two ports).

*   **File:** `docs/diagrams/` (directory exists but empty)
    *   **Summary:** The directory for architectural diagrams already exists at `docs/diagrams/` as specified in the task.
    *   **Recommendation:** You SHOULD save your PlantUML file to `docs/diagrams/component_architecture.puml` as specified in the task. The directory structure is already in place.

*   **File:** `Cargo.toml`
    *   **Summary:** This file confirms all the technology dependencies mentioned in the architecture blueprint: clap (CLI), nom (binary parsing), quick-xml (XMP), serde/serde_json (serialization), chrono (dates), encoding_rs (string encoding), memmap2 (file I/O), and rayon (concurrency).
    *   **Recommendation:** When creating the component diagram, you SHOULD reference the actual technology stack in the component descriptions. For example, "JPEG Parser" should note it's "nom-based" and "XMP Parser" should note it uses "quick-xml" as shown in the architecture blueprint example.

### Implementation Tips & Notes

*   **Tip:** The architecture blueprint in `03_System_Structure_and_Data.md` includes a complete PlantUML example of the component diagram. You SHOULD use this as your template, ensuring you maintain the same C4 notation style with `!include` directives, `LAYOUT_WITH_LEGEND()`, and proper component boundaries.

*   **Note:** The task acceptance criteria requires the diagram to "accurately reflect hexagonal architecture layers from Section 2" - this means you MUST show clear visual separation between:
    1. **Domain Layer** components (API Facade, Metadata Model, Operations, Tag Registry, Validation Engine)
    2. **Ports** (interfaces/traits: FormatParser trait, FileReader trait)
    3. **Infrastructure Adapters** (JPEG Parser, TIFF Parser, XMP Parser, MMap Reader)

    Use the `Container_Boundary` and grouping to make this separation obvious.

*   **Tip:** The PlantUML example in the architecture uses `Component_Ext()` for external/infrastructure components (adapters) to visually distinguish them from internal domain components. You SHOULD follow this convention to make the hexagonal boundary clear.

*   **Note:** Based on the existing code structure in `src/`, the component diagram should reflect these actual implementation modules:
    - `core/metadata_map.rs`, `core/tag_value.rs`, `core/tag_descriptor.rs` → "Metadata Model" component
    - `core/operations.rs` → "Metadata Operations" component
    - `core/validation.rs` → "Validation Engine" component
    - `core/format_parser_trait.rs` → "Format Parser Port" component
    - `core/file_reader_trait.rs` → "I/O Port" component
    - `parsers/jpeg/` → "JPEG Parser" adapter
    - `parsers/tiff/` → "TIFF Parser" adapter
    - `parsers/xmp/` → "XMP Parser" adapter
    - `io/mmap_reader.rs` → "MMap Reader" adapter

*   **Tip:** The architecture blueprint specifies using C4 PlantUML notation. You MUST include the standard C4 include directive at the top: `!include https://raw.githubusercontent.com/plantuml-stdlib/C4-PlantUML/master/C4_Component.puml`

*   **Warning:** The task specifies validation with `plantuml -tsvg component_architecture.puml`. PlantUML syntax is strict - ensure you use proper `@startuml`/`@enduml` tags, valid relationship syntax (`Rel()`), and consistent naming (no spaces in component IDs).

*   **Tip:** The architecture emphasizes that the tag_registry will contain "28K+ tag definitions" from the generated tag database. Include this detail in the component description to show the scale and importance of this component.

*   **Note:** The relationships between components should follow the hexagonal architecture pattern:
    - API Facade orchestrates Operations
    - Operations manipulates Metadata Model
    - Operations looks up Tag Registry
    - Operations calls Format Parser Port (interface)
    - Operations validates via Validation Engine
    - Format Parser Port is implemented by concrete parsers (JPEG, TIFF, XMP)
    - Parsers read via I/O Port (interface)
    - I/O Port is implemented by MMap Reader

    This creates the classic hexagonal "dependency inversion" where the core depends on abstractions (ports) and infrastructure depends on core interfaces.

*   **Tip:** The complete PlantUML example from the architecture blueprint (Section 3.5) is nearly complete and can be used almost verbatim. The main thing you need to do is save it to the correct location (`docs/diagrams/component_architecture.puml`) and ensure the syntax is exactly correct for validation.

*   **Note:** The diagram uses PlantUML comments (lines starting with `'`) to organize sections. Keep these comments to make the source more maintainable - they explain which components belong to which layers (Domain Layer, Ports, Infrastructure adapters).

*   **Warning:** Make sure the file you create is properly formatted with:
    - The `@startuml` tag at the beginning
    - The `@enduml` tag at the end
    - The `!include` directive for C4 notation
    - The `LAYOUT_WITH_LEGEND()` macro for automatic legend generation
    - A clear `title` for the diagram
    - All component IDs must be valid identifiers (no spaces, use underscores)
