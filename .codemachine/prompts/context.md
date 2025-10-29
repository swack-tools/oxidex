# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I1.T8",
  "iteration_id": "I1",
  "iteration_goal": "Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.",
  "description": "Implement FileReader trait in src/io/: MMapReader using memmap2 crate for memory-mapped file access (efficient for large files), and BufferedReader using std::io::BufReader for streaming access. Both should handle file opening, error propagation, and boundary checking. Add unit tests verifying read() operations at various offsets.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I1.T7 FileReader trait definition",
  "target_files": [
    "src/io/mmap_reader.rs",
    "src/io/buffered_reader.rs",
    "src/io/file_reader.rs",
    "src/io/mod.rs"
  ],
  "input_files": [
    "src/core/file_reader_trait.rs"
  ],
  "deliverables": "MMapReader and BufferedReader implementations, unit tests for both readers",
  "acceptance_criteria": "Both readers implement FileReader trait, MMapReader uses memmap2::Mmap internally, BufferedReader uses std::io::BufReader internally, read() method handles out-of-bounds requests gracefully (return error), size() method returns correct file size, unit tests verify reading at offset 0, middle, and end of file, cargo test passes for io module",
  "dependencies": [
    "I1.T7"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

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

### Context: technology-stack-summary (from 02_Architecture_Overview.md)

```markdown
### 3.2. Technology Stack Summary

| **Category** | **Technology Choice** | **Justification** |
|--------------|----------------------|-------------------|
| **Image I/O** | `memmap2` (memory-mapped files) | Efficient large file access without loading entire file into memory |

**Key Libraries Detail**:

- **`memmap2`**: Memory-mapped files via `Mmap::map(&file)`. Enables zero-copy parsing for formats with known offsets (JPEG EXIF segment, PNG chunks).

**Dependency Philosophy**:
- **Minimize Count**: Target < 50 direct dependencies to reduce supply chain risk
- **Prefer `no_std` Compatible**: Where possible (e.g., `nom`, `binrw`) to enable future embedded/WASM use
- **Audit Regularly**: `cargo-audit` in CI pipeline to catch vulnerabilities in transitive dependencies
```

### Context: component-diagram (from 03_System_Structure_and_Data.md)

```markdown
### 3.5. Component Diagram(s) (C4 Level 3)

**Description**: This diagram details the internal components of the **Core Library** container, showing the hexagonal architecture layers and their interactions.

Component_Boundary(core_lib, "Core Library") {
  ' Ports (interfaces)
  Component(io_port, "I/O Port", "Rust trait", "trait FileReader { fn read(&self, offset, len) -> Result<&[u8]> }")

  ' Infrastructure adapters (in other containers but shown for clarity)
  Component_Ext(mmap_adapter, "MMap Reader", "memmap2", "Memory-mapped file access")
}

Rel(jpeg_adapter, io_port, "Reads via")
Rel(tiff_adapter, io_port, "Reads via")
Rel(io_port, mmap_adapter, "Implemented by")
```

### Context: task-i1-t8 (from 02_Iteration_I1.md)

```markdown
<!-- anchor: task-i1-t8 -->
*   **Task 1.8: Implement File Reader Adapters (MMap and Buffered)**
    *   **Task ID:** `I1.T8`
    *   **Description:** Implement FileReader trait in `src/io/`: `MMapReader` using `memmap2` crate for memory-mapped file access (efficient for large files), and `BufferedReader` using `std::io::BufReader` for streaming access. Both should handle file opening, error propagation, and boundary checking. Add unit tests verifying read() operations at various offsets.
    *   **Agent Type Hint:** `BackendAgent`
    *   **Inputs:** I1.T7 FileReader trait definition
    *   **Input Files:** [`src/core/file_reader_trait.rs`]
    *   **Target Files:**
        *   `src/io/mmap_reader.rs`
        *   `src/io/buffered_reader.rs`
        *   `src/io/file_reader.rs` (re-exports both)
        *   `src/io/mod.rs`
    *   **Deliverables:**
        *   MMapReader and BufferedReader implementations
        *   Unit tests for both readers
    *   **Acceptance Criteria:**
        *   Both readers implement FileReader trait
        *   MMapReader uses memmap2::Mmap internally
        *   BufferedReader uses std::io::BufReader internally
        *   read() method handles out-of-bounds requests gracefully (return error)
        *   size() method returns correct file size
        *   Unit tests verify reading at offset 0, middle, and end of file
        *   `cargo test` passes for io module
    *   **Dependencies:** `I1.T7` (needs FileReader trait)
    *   **Parallelizable:** No (depends on T7)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/file_reader_trait.rs`
    *   **Summary:** This file contains the complete `FileReader` trait definition that serves as the secondary port in the hexagonal architecture. The trait defines two methods: `read(&self, offset: u64, length: usize) -> io::Result<&[u8]>` for reading file slices and `size(&self) -> u64` for getting total file size. The trait is object-safe and designed for zero-copy access patterns.
    *   **Recommendation:** You MUST import and implement this trait exactly as specified. The trait contract requires:
        - `read()` must return borrowed slices valid for the lifetime of `&self`
        - `read()` must return `Err` if `offset + length` exceeds file size
        - `size()` must return consistent values during the reader's lifetime
        - Implementations should be thread-safe if intended for concurrent access
    *   **Critical Detail:** The trait returns `io::Result<&[u8]>` (standard library result), NOT `crate::error::Result`. This is intentional to keep the infrastructure layer decoupled from domain errors.

*   **File:** `src/error/mod.rs`
    *   **Summary:** Defines the `ExifToolError` enum with variants: `IoError`, `ParseError`, `TagNotFound`, `InvalidTagValue`, and `UnsupportedFormat`. Provides conversion from `std::io::Error` to `ExifToolError` via the `From` trait.
    *   **Recommendation:** For the file readers, you should use `std::io::Error` directly since they are infrastructure adapters. The conversion to `ExifToolError` happens at the domain layer boundary, not in the I/O adapters.

*   **File:** `src/io/mod.rs`
    *   **Summary:** This is the module root that currently declares three submodules: `buffered_reader`, `file_reader`, and `mmap_reader`. The file is minimal with just module declarations.
    *   **Recommendation:** You SHOULD add public re-exports after implementing the readers to make them easily accessible. For example: `pub use mmap_reader::MMapReader;` and `pub use buffered_reader::BufferedReader;`.

*   **File:** `src/core/mod.rs`
    *   **Summary:** The domain layer module root that re-exports core types including `FileReader` from `file_reader_trait`. This confirms the trait is already part of the public API surface.
    *   **Recommendation:** You do NOT need to modify this file. The trait is already properly exported and your implementations in `src/io/` will be infrastructure adapters.

*   **File:** `Cargo.toml`
    *   **Summary:** Project configuration with all required dependencies already declared. The `memmap2 = "0.9"` dependency is present under `[dependencies]`, confirming it's available for use. Also includes `tempfile = "3.10"` under `[dev-dependencies]` which you can use for creating test files.
    *   **Recommendation:** You do NOT need to modify `Cargo.toml`. All required dependencies are already configured.

### Implementation Tips & Notes

*   **Tip: MMapReader Lifetime Management:** When using `memmap2::Mmap`, the memory-mapped region must outlive any slices returned by `read()`. Store the `Mmap` in a struct field and return slices that borrow from it. Use `unsafe { mmap.get_unchecked(start..end) }` for zero-copy access (after bounds checking).

*   **Tip: BufferedReader Caching Strategy:** `std::io::BufReader` is designed for streaming, not random access. To implement the `FileReader` trait, you'll need to store the file handle and implement seeking. Consider using `std::io::Seek` to position to the offset, then reading into a temporary buffer that you return a slice from. This means you'll need to store a buffer in the struct and manage its lifetime carefully.

*   **Note: The FileReader trait signature challenge:** The trait requires returning `&[u8]` borrowed from `&self`. For `BufferedReader`, you cannot return slices from the `BufReader` directly because `read()` returns owned data. You MUST store a buffer in the struct (e.g., `Vec<u8>`) and return slices from it. Use `RefCell<Vec<u8>>` for interior mutability if needed, or reconsider the design to use a pinned buffer.

*   **Warning: Thread Safety Considerations:** The trait documentation mentions thread-safety. For `MMapReader`, memory-mapped regions are inherently shareable across threads (read-only). For `BufferedReader`, if you use `RefCell` for interior mutability, it will NOT be thread-safe (not `Sync`). Document this limitation or use `Mutex` instead of `RefCell` if thread-safety is required.

*   **Tip: Unit Test Strategy:** The acceptance criteria require testing reads at offset 0, middle, and end of file. Use `tempfile::NamedTempFile` to create test files with known content. Test cases should verify:
    1. Successful reads returning correct data
    2. Out-of-bounds reads returning `Err`
    3. Read at exact end of file (offset = size, length = 0)
    4. `size()` returning correct value
    5. Multiple sequential reads

*   **Tip: Error Handling for Out-of-Bounds:** The trait contract requires returning `Err` if `offset + length` exceeds file size. Use `std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "read beyond end of file")` for consistency with the example in `file_reader_trait.rs`.

*   **Critical: The FileReader trait returns borrowed data:** This is a zero-copy interface. For `MMapReader`, this is natural (return slices from mmap). For `BufferedReader`, you'll need to carefully manage buffer lifetime. Consider these options:
    1. Store buffer as `Vec<u8>` in struct, return slices (requires interior mutability)
    2. Use `Rc<RefCell<Vec<u8>>>` for shared ownership
    3. Accept that `BufferedReader` may need to allocate per-read (less efficient but simpler)

*   **Recommendation: File Opening Strategy:** Both readers should accept a file path in their constructor. For `MMapReader`, use `std::fs::File::open()` then `memmap2::Mmap::map(&file)`. For `BufferedReader`, use `std::fs::File::open()` wrapped in `BufReader::new()`. Handle file opening errors by returning `io::Result` from the constructor.

*   **Note: The file_reader.rs placeholder:** The current `src/io/file_reader.rs` is just a placeholder with a comment. You should use this file to re-export both readers for convenience: `pub use crate::io::mmap_reader::MMapReader;` and `pub use crate::io::buffered_reader::BufferedReader;`.

*   **Recommendation: Struct Design Patterns:**
    - `MMapReader`: Store `file: File` and `mmap: Mmap` in struct
    - `BufferedReader`: Store `file: File`, `reader: BufReader<File>`, and `buffer: RefCell<Vec<u8>>` OR use a different approach where you return owned data wrapped in a lifetime-extending container

*   **Critical Architecture Note:** These readers are **infrastructure adapters** implementing the **secondary port** (`FileReader` trait). They should have NO dependencies on domain layer code except the trait itself. Do NOT import `ExifToolError` or any domain models in these files.

### Suggested Implementation Approach

1. **Start with MMapReader** (simpler due to natural zero-copy semantics):
   - Implement constructor: `pub fn new(path: &Path) -> io::Result<Self>`
   - Store `Mmap` in struct field
   - Implement `FileReader` trait with bounds checking
   - Write comprehensive unit tests

2. **Then implement BufferedReader** (more complex due to lifetime constraints):
   - Decide on buffer management strategy (see tips above)
   - Implement constructor similar to MMapReader
   - Implement `FileReader` trait with seeking and buffering
   - Write unit tests

3. **Update mod.rs and file_reader.rs** with proper re-exports

4. **Run `cargo test` to verify** all tests pass and meet acceptance criteria

### Key Differences from Standard Practice

*   **Zero-Copy Requirement:** Unlike typical file readers that return owned `Vec<u8>`, this trait requires returning borrowed slices. This is by design for performance (avoiding allocations in hot parsing loops).

*   **Object-Safe Trait:** The trait is designed for dynamic dispatch (`dyn FileReader`). All methods use `&self` and return non-generic types to maintain object-safety.

*   **No Error Wrapping at This Layer:** These adapters return `std::io::Error`, not `ExifToolError`. The domain layer (parsers) will handle conversion when needed.
