# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I1.T7",
  "iteration_id": "I1",
  "iteration_goal": "Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.",
  "description": "Implement port interfaces in `src/core/`: `trait FormatParser` with method `fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap, ExifToolError>` and `fn supports_format(&self, format: FileFormat) -> bool`. Implement `trait FileReader` with methods `fn read(&self, offset: u64, length: usize) -> Result<&[u8], std::io::Error>`, `fn size(&self) -> u64`. Define `enum FileFormat` with variants for JPEG, TIFF, PNG, etc. Add comprehensive documentation comments explaining trait contracts.",
  "agent_type_hint": "BackendAgent",
  "inputs": "Section 2 (Core Architecture - hexagonal architecture ports), Section 2.1 (artifact for format parser trait)",
  "target_files": [
    "src/core/format_parser_trait.rs",
    "src/core/file_reader_trait.rs",
    "src/core/file_format.rs",
    "src/core/mod.rs"
  ],
  "input_files": [
    "src/core/metadata_map.rs",
    "src/error.rs"
  ],
  "deliverables": "Rust trait definitions with documentation, FileFormat enum with initial variants (JPEG, TIFF, PNG, PDF, Unknown)",
  "acceptance_criteria": "Traits compile successfully, documentation comments explain trait purpose and method contracts, FormatParser trait has parse() and supports_format() methods, FileReader trait has read() and size() methods, FileFormat enum has at least 5 variants, code compiles with `cargo build`",
  "dependencies": [
    "I1.T6"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: Architectural Style - Layered Hexagonal Architecture (from 02_Architecture_Overview.md)

```markdown
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

### Context: Component Diagram - Ports and Adapters (from 03_System_Structure_and_Data.md)

```markdown
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

### Context: Key Entities - Data Model (from 03_System_Structure_and_Data.md)

```markdown
#### Key Entities

1. **File**: Represents a media file being processed (JPEG, PNG, etc.)
2. **MetadataMap**: Collection of all metadata tags extracted from a file
3. **TagValue**: A single metadata tag with its name, value, and type information
4. **TagDescriptor**: Definition of a tag (from tag database) including ID, name, type constraints, format family
5. **FormatFamily**: Grouping of related metadata standards (EXIF, XMP, IPTC, MakerNotes)
6. **IFD (Image File Directory)**: TIFF-specific structural element containing tags
```

### Context: Error Handling Pattern (from 04_Behavior_and_Communication.md)

```markdown
**Error Handling**: `Result<T, ExifToolError>` throughout

```rust
pub enum ExifToolError {
    IoError(std::io::Error),
    ParseError { format: String, details: String },
    TagNotFound { tag_name: String },
    InvalidTagValue { tag_name: String, expected_type: String },
    UnsupportedFormat { format: String },
}
```

Errors propagate via `?` operator, no exceptions.
```

### Context: Task I1.T7 Specification (from 02_Iteration_I1.md)

```markdown
<!-- anchor: task-i1-t7 -->
*   **Task 1.7: Define Format Parser and File Reader Traits**
    *   **Task ID:** `I1.T7`
    *   **Description:** Implement port interfaces in `src/core/`: `trait FormatParser` with method `fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap, ExifToolError>` and `fn supports_format(&self, format: FileFormat) -> bool`. Implement `trait FileReader` with methods `fn read(&self, offset: u64, length: usize) -> Result<&[u8], std::io::Error>`, `fn size(&self) -> u64`. Define `enum FileFormat` with variants for JPEG, TIFF, PNG, etc. Add comprehensive documentation comments explaining trait contracts.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** Section 2 (Core Architecture - hexagonal architecture ports), Section 2.1 (artifact for format parser trait)
    *   **Input Files:** [`src/core/metadata_map.rs`, `src/error.rs`]
    *   **Target Files:**
        *   `src/core/format_parser_trait.rs`
        *   `src/core/file_reader_trait.rs`
        *   `src/core/file_format.rs` (enum FileFormat)
        *   `src/core/mod.rs` (export traits)
    *   **Deliverables:**
        *   Rust trait definitions with documentation
        *   FileFormat enum with initial variants (JPEG, TIFF, PNG, PDF, Unknown)
    *   **Acceptance Criteria:**
        *   Traits compile successfully
        *   Documentation comments explain trait purpose and method contracts
        *   FormatParser trait has parse() and supports_format() methods
        *   FileReader trait has read() and size() methods
        *   FileFormat enum has at least 5 variants
        *   Code compiles with `cargo build`
    *   **Dependencies:** `I1.T6` (needs MetadataMap and ExifToolError)
    *   **Parallelizable:** No (depends on T6)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/metadata_map.rs`
    *   **Summary:** This file defines the core `MetadataMap` struct, which is a wrapper around `HashMap<String, TagValue>`. It provides typed getter methods (`get_string()`, `get_integer()`, `get_float()`) and implements standard collection operations. The struct derives `Debug`, `Clone`, `PartialEq`, `Serialize`, and `Deserialize` for full serde support.
    *   **Recommendation:** You MUST import and use `MetadataMap` in your `FormatParser` trait. The return type of the `parse()` method is `Result<MetadataMap, ExifToolError>`. Import it from `super::metadata_map::MetadataMap` or use `crate::core::MetadataMap`.

*   **File:** `src/error/mod.rs`
    *   **Summary:** This file defines the `ExifToolError` enum with variants: `IoError`, `ParseError`, `TagNotFound`, `InvalidTagValue`, and `UnsupportedFormat`. It implements `std::error::Error` and `std::fmt::Display`. It also provides a type alias: `pub type Result<T> = std::result::Result<T, ExifToolError>`.
    *   **Recommendation:** You MUST import `ExifToolError` and the `Result` type alias from `crate::error`. Use `Result<T>` as the return type for the `parse()` method instead of writing out the full `std::result::Result<T, ExifToolError>`. Note that `std::io::Error` should be used directly for `FileReader::read()` since it's a low-level I/O operation.

*   **File:** `src/core/tag_descriptor.rs`
    *   **Summary:** This file defines `TagDescriptor`, `TagId`, `FormatFamily`, and `ValueType` enums. The `FormatFamily` enum already includes variants for EXIF, XMP, IPTC, GPS, ICCProfile, Photoshop, MakerNotes, JFIF, PNG, PDF, and QuickTime.
    *   **Recommendation:** You SHOULD reference the `FormatFamily` enum when designing your `FileFormat` enum. They serve similar purposes (format classification) and should have similar variants to maintain consistency. However, `FileFormat` is for file-level detection, while `FormatFamily` is for metadata tag categorization.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** This file defines the `TagValue` enum with variants: String, Integer, Float, Rational, Binary, DateTime, and Struct. It includes constructors (`new_string()`, `new_integer()`, etc.) and type-checking methods (`is_string()`, `is_integer()`, etc.).
    *   **Recommendation:** While you won't directly use `TagValue` in your trait definitions, it's important to understand that `MetadataMap` stores `TagValue` instances. Your trait documentation should mention that parsers return a collection of tag name → `TagValue` mappings.

*   **File:** `src/core/mod.rs`
    *   **Summary:** This is the module root for the core domain layer. It currently exports `file_reader_trait`, `format_parser_trait`, `metadata_map`, `operations`, `tag_descriptor`, `tag_value`, and `validation` modules. It also re-exports commonly used types: `MetadataMap`, `TagDescriptor`, `FormatFamily`, `TagId`, `ValueType`, and `TagValue`.
    *   **Recommendation:** After implementing your traits, you MUST add public re-exports to this file so that consumers can easily import the traits. Add lines like: `pub use file_reader_trait::FileReader;`, `pub use format_parser_trait::FormatParser;`, and `pub use file_format::FileFormat;` (you'll need to create the `file_format` module first).

*   **File:** `src/core/file_reader_trait.rs`
    *   **Summary:** This file currently only contains a module comment and `#![allow(dead_code)]`. It is a placeholder waiting for the trait definition.
    *   **Recommendation:** You MUST implement the `FileReader` trait in this file according to the task specification.

*   **File:** `src/core/format_parser_trait.rs`
    *   **Summary:** This file currently only contains a module comment and `#![allow(dead_code)]`. It is a placeholder waiting for the trait definition.
    *   **Recommendation:** You MUST implement the `FormatParser` trait in this file according to the task specification.

*   **File:** `api/tag_database_schema.json`
    *   **Summary:** This JSON Schema defines the structure of `TagDescriptor` objects for the tag database. It shows the required fields and enums for `format_family` and `value_type`.
    *   **Recommendation:** While not directly used in your implementation, this schema confirms the design decisions around format families and value types. Your `FileFormat` enum should align with the format families mentioned here.

### Implementation Tips & Notes

*   **Tip:** I have confirmed that the project follows Rust 2021 edition idioms. All existing code uses modern patterns like `#![allow(dead_code)]` for work-in-progress modules, comprehensive doc comments with examples, and derives for common traits (Debug, Clone, PartialEq, Serialize, Deserialize).

*   **Tip:** The existing component diagram (`docs/diagrams/component_architecture.puml`) clearly shows that `FormatParser` is a "port" (interface) in the hexagonal architecture. Your trait documentation SHOULD explain this architectural role: it's the boundary between the domain layer (core library) and the infrastructure layer (format-specific parsers).

*   **Tip:** The `FileReader` trait should be designed for zero-copy access. Notice that the method signature uses `&[u8]` as the return type. This allows implementations like `MMapReader` to return direct references to memory-mapped file data without copying. Your documentation should emphasize this design goal.

*   **Note:** The task requires `FileReader::read()` to return `Result<&[u8], std::io::Error>`. This poses a lifetime challenge: the returned slice must borrow from `&self`. You'll need to declare the trait method with a lifetime parameter: `fn read(&self, offset: u64, length: usize) -> Result<&'_ [u8], std::io::Error>` or use an explicit lifetime like `'a`.

*   **Note:** The task specifies that `FormatParser::parse()` takes `reader: &dyn FileReader`. This is a trait object, which means you'll need to handle dynamic dispatch. Make sure the `FileReader` trait is object-safe (no associated types, no `Self: Sized` bounds on methods).

*   **Warning:** When creating the `FileFormat` enum, you MUST include at least 5 variants as specified in the acceptance criteria: JPEG, TIFF, PNG, PDF, and Unknown. Consider adding more variants that align with `FormatFamily` for future-proofing (e.g., GIF, BMP, MP4, QuickTime).

*   **Warning:** The project uses `#![allow(dead_code)]` to suppress warnings during development. You SHOULD include this attribute at the top of your new trait files since the traits won't be used yet (they'll be implemented in later tasks I1.T8-I1.T11).

*   **Best Practice:** Follow the existing documentation style. Every public trait, method, and enum should have:
    1. A `///` doc comment explaining its purpose
    2. `# Examples` section showing usage (if applicable)
    3. `# Errors` section explaining error conditions (for methods that return `Result`)
    4. Clear explanation of the trait contract (what implementers must guarantee)

*   **Best Practice:** The existing code uses constructor-style methods (e.g., `TagValue::new_string()`). While traits typically don't have constructors, you should document any expected patterns for creating implementations.

*   **Code Quality:** Make sure to run `cargo build`, `cargo clippy`, and `cargo fmt --check` before considering the task complete. The acceptance criteria explicitly require compilation success and adherence to the project's formatting and linting standards.

*   **Hexagonal Architecture Principle:** Your traits are the "ports" in the ports-and-adapters architecture. They should be defined in terms of domain concepts (MetadataMap, FileFormat) NOT infrastructure details (file paths, specific parser implementations). This keeps the domain layer pure and testable.

### Critical Implementation Requirements

1. **You MUST create a new file:** `src/core/file_format.rs` with the `FileFormat` enum
2. **You MUST update:** `src/core/mod.rs` to:
   - Declare the new `file_format` module with `pub mod file_format;`
   - Add re-exports: `pub use file_reader_trait::FileReader;`, `pub use format_parser_trait::FormatParser;`, `pub use file_format::FileFormat;`
3. **You MUST import:** `MetadataMap` from `super::metadata_map` in `format_parser_trait.rs`
4. **You MUST import:** `ExifToolError` and `Result` type alias from `crate::error`
5. **You MUST handle:** Lifetime parameters correctly in `FileReader::read()` to return borrowed slices
6. **You MUST ensure:** Both traits are object-safe for use with `dyn Trait`
7. **You MUST write:** Comprehensive documentation comments explaining the hexagonal architecture role
8. **You MUST include:** At least 5 `FileFormat` variants: JPEG, TIFF, PNG, PDF, Unknown
9. **You SHOULD add:** Derive macros for `FileFormat`: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`
10. **You MUST verify:** Code compiles with `cargo build` without errors or warnings
