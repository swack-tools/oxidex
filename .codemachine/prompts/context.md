# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T2",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Implement C FFI in src/ffi/c_api.rs based on I5.T1 design. Use #[no_mangle] and extern C for exported functions. Implement handle management (opaque pointer to Rust struct), error handling (convert Rust Result to error codes), string conversion (CStr/CString for C strings), memory safety (catch panics with catch_unwind, return error codes). Export functions: exiftool_create(), exiftool_destroy(), exiftool_read_file(), exiftool_get_tag_string(), exiftool_set_tag(), exiftool_write_file(), exiftool_get_last_error(). Add C integration test (optional: use cc crate to compile C test file linking to Rust library).",
  "agent_type_hint": "BackendAgent",
  "inputs": "I5.T1 FFI API design, I2.T3 read operations, I3.T4 write operations",
  "target_files": [
    "src/ffi/c_api.rs",
    "src/ffi/mod.rs",
    "src/lib.rs",
    "Cargo.toml",
    "tests/ffi/c_integration_test.c",
    "tests/ffi/build.rs"
  ],
  "input_files": [
    "docs/api/ffi_api.md",
    "src/core/operations.rs"
  ],
  "deliverables": "C FFI implementation, handle-based API, error handling across FFI boundary, optional C integration test",
  "acceptance_criteria": "FFI functions are extern C with #[no_mangle], handles are opaque pointers (Box<T> leaked and reclaimed), panics caught with catch_unwind, converted to error codes, strings converted correctly (Rust String <-> C char*), memory leaks prevented (handles destroyed properly), C integration test (if present) compiles and runs successfully, cargo build --lib produces shared library (.so/.dll/.dylib)",
  "dependencies": [
    "I5.T1",
    "I2.T3",
    "I3.T4"
  ],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: API Design Style (from 04_Behavior_and_Communication.md)

```markdown
<!-- anchor: api-style -->
#### API Style

**Primary API**: **Rust Library API** (procedural + builder pattern)

The core API is designed for Rust consumers and follows idiomatic patterns:

```rust
use exiftool_rs::{Metadata, FileFormat};

// Simple extraction
let metadata = Metadata::from_path("photo.jpg")?;
let camera_model = metadata.get_string("EXIF:Model")?;

// Builder pattern for complex operations
let result = Metadata::from_path("input.jpg")?
    .copy_tags_to("output.jpg")?
    .with_tags(&["EXIF:DateTime", "EXIF:Make", "EXIF:Model"])
    .preserve_file_times(true)
    .execute()?;
```

**Secondary APIs**:

1. **CLI Interface**: POSIX-style arguments mimicking ExifTool
   ```bash
   exiftool-rs -EXIF:DateTime photo.jpg
   exiftool-rs -json -r /photos/  # Recursive JSON output
   exiftool-rs -TagsFromFile src.jpg -all:all dest.jpg  # Copy metadata
   ```

2. **C FFI**: Minimal C-compatible surface for foreign language bindings
   ```c
   // C API example
   ExifToolHandle* handle = exiftool_create();
   ExifToolError err = exiftool_read_file(handle, "photo.jpg");
   const char* model = exiftool_get_string(handle, "EXIF:Model");
   exiftool_destroy(handle);
   ```

**Justification**:

- **Rust-First**: Leverages Rust's type system for compile-time safety (no invalid tag names at compile time via const tag identifiers)
- **No Network API**: ExifTool-RS is a library/tool, not a service. REST/GraphQL APIs would be implemented by consuming applications
- **FFI for Interop**: Enables Python (`pyo3`), Node.js (`neon`), Go (`cgo`) bindings without compromising Rust API ergonomics
```

### Context: C FFI API Requirements (from docs/api/ffi_api.md)

The FFI API specification document provides comprehensive details on:

**Key Design Principles:**
- **Safety First**: No panics cross the FFI boundary. All Rust panics are caught and converted to error codes.
- **C Idioms**: API follows standard C conventions (return codes, null-terminated strings, opaque handles).
- **Explicit Errors**: All errors are returned as integer codes with detailed messages available via `exiftool_get_last_error()`.
- **Clear Ownership**: Memory management rules are explicit and documented.
- **Minimal Surface**: API exposes only essential operations.

**Required Functions:**

1. **Handle Lifecycle:**
   - `ExifToolHandle* exiftool_create(void)` - Creates new handle, returns NULL on allocation failure
   - `void exiftool_destroy(ExifToolHandle* handle)` - Destroys handle and frees resources

2. **Metadata Reading:**
   - `int exiftool_read_file(ExifToolHandle* handle, const char* filepath)` - Reads metadata from file
   - `size_t exiftool_get_tag_count(const ExifToolHandle* handle)` - Returns number of tags
   - `const char* exiftool_get_tag_name_at(const ExifToolHandle* handle, size_t index)` - Get tag name by index
   - `int exiftool_has_tag(const ExifToolHandle* handle, const char* tag_name)` - Check if tag exists

3. **Tag Access:**
   - `const char* exiftool_get_tag_string(const ExifToolHandle* handle, const char* tag_name)` - Get string tag value
   - `int exiftool_get_tag_integer(const ExifToolHandle* handle, const char* tag_name, int64_t* out_value)` - Get integer tag value
   - `int exiftool_get_tag_float(const ExifToolHandle* handle, const char* tag_name, double* out_value)` - Get float tag value

4. **Metadata Writing:**
   - `int exiftool_set_tag_string(ExifToolHandle* handle, const char* tag_name, const char* value)` - Set string tag
   - `int exiftool_set_tag_integer(ExifToolHandle* handle, const char* tag_name, int64_t value)` - Set integer tag
   - `int exiftool_set_tag_float(ExifToolHandle* handle, const char* tag_name, double value)` - Set float tag
   - `int exiftool_remove_tag(ExifToolHandle* handle, const char* tag_name)` - Remove tag
   - `int exiftool_write_file(const ExifToolHandle* handle, const char* filepath)` - Write metadata to file

5. **Error Handling:**
   - `const char* exiftool_get_last_error(void)` - Returns last error message (thread-local storage)

**Error Codes:**
```c
#define EXIFTOOL_OK                      0
#define EXIFTOOL_ERR_IO                  1
#define EXIFTOOL_ERR_PARSE               2
#define EXIFTOOL_ERR_TAG_NOT_FOUND       3
#define EXIFTOOL_ERR_INVALID_TAG_VALUE   4
#define EXIFTOOL_ERR_UNSUPPORTED_FORMAT  5
#define EXIFTOOL_ERR_NULL_POINTER        6
#define EXIFTOOL_ERR_INTERNAL            99
```

**Memory Management Rules:**
- Handles must be destroyed exactly once
- Returned strings are valid until next API call or handle destruction
- Input strings must be null-terminated UTF-8
- Library makes internal copies of input strings
- Handles are NOT thread-safe (one handle per thread or external synchronization required)
- Error messages ARE thread-safe (thread-local storage)

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/operations.rs`
    *   **Summary:** This file contains the core `read_metadata()` and `write_metadata()` functions that orchestrate metadata extraction and modification. It uses the MMapReader for file access, format detection, and parser selection. The `read_metadata()` function returns a `Result<MetadataMap>`.
    *   **Recommendation:** You MUST import and use `read_metadata()` and `write_metadata()` from this file in your FFI implementation. These are the primary entry points for all metadata operations.
    *   **Key Functions:**
        - `pub fn read_metadata(path: &Path) -> Result<MetadataMap>` - Main entry point for reading
        - `pub fn write_metadata(path: &Path, metadata: &MetadataMap) -> Result<()>` - Main entry point for writing (expected)

*   **File:** `src/core/metadata_map.rs`
    *   **Summary:** This file defines the `MetadataMap` struct which is a wrapper around `HashMap<String, TagValue>`. It provides typed getter methods for accessing metadata values.
    *   **Recommendation:** The MetadataMap is your bridge between Rust operations and FFI. You'll need to store this in your opaque handle struct and provide methods to iterate and access its contents.
    *   **Key Methods:**
        - `pub fn new() -> Self`
        - `pub fn insert<K: Into<String>>(&mut self, key: K, value: TagValue) -> Option<TagValue>`
        - `pub fn get(&self, key: &str) -> Option<&TagValue>`
        - `pub fn len(&self) -> usize`
        - `pub fn get_string(&self, key: &str) -> Option<String>` (if implemented)
        - `pub fn get_integer(&self, key: &str) -> Option<i64>` (if implemented)
        - `pub fn get_float(&self, key: &str) -> Option<f64>` (if implemented)

*   **File:** `src/error/mod.rs`
    *   **Summary:** This file defines the `ExifToolError` enum with variants: `IoError`, `ParseError`, `TagNotFound`, `InvalidTagValue`, and `UnsupportedFormat`. It implements `std::error::Error` and provides a `Result<T>` type alias.
    *   **Recommendation:** You MUST map these Rust error variants to the C error codes defined in the FFI API spec. Create a helper function to convert `ExifToolError` to integer error codes.
    *   **Error Mapping:**
        - `ExifToolError::IoError` → `EXIFTOOL_ERR_IO` (1)
        - `ExifToolError::ParseError` → `EXIFTOOL_ERR_PARSE` (2)
        - `ExifToolError::TagNotFound` → `EXIFTOOL_ERR_TAG_NOT_FOUND` (3)
        - `ExifToolError::InvalidTagValue` → `EXIFTOOL_ERR_INVALID_TAG_VALUE` (4)
        - `ExifToolError::UnsupportedFormat` → `EXIFTOOL_ERR_UNSUPPORTED_FORMAT` (5)

*   **File:** `src/ffi/c_api.rs`
    *   **Summary:** Currently empty with just module documentation and `#![allow(dead_code)]`.
    *   **Recommendation:** This is your primary implementation file. You will write all FFI functions here.

*   **File:** `src/ffi/mod.rs`
    *   **Summary:** Currently only declares `pub mod c_api;`.
    *   **Recommendation:** This file is already correctly set up. No changes needed.

*   **File:** `src/lib.rs`
    *   **Summary:** The main library root that declares all modules. It already includes `pub mod ffi;`.
    *   **Recommendation:** Ensure the library is configured to build as both a static and dynamic library. Check Cargo.toml `[lib]` section.

### Implementation Tips & Notes

*   **Tip #1: Opaque Handle Design**
    - Define an internal Rust struct (e.g., `ExifToolContext`) that holds the `MetadataMap` and the current file path
    - Use `Box::into_raw()` to convert `Box<ExifToolContext>` to `*mut ExifToolContext` for exporting to C
    - Use `Box::from_raw()` to reclaim ownership when destroying the handle
    - NEVER allow the C code to dereference the pointer - it must remain opaque

*   **Tip #2: Thread-Local Error Storage**
    - Use `thread_local!` macro to create a `RefCell<String>` for storing error messages
    - Each thread maintains its own error message, making `exiftool_get_last_error()` thread-safe
    - Update this storage before returning error codes from FFI functions

*   **Tip #3: Panic Catching**
    - Wrap EVERY FFI function body in `std::panic::catch_unwind(AssertUnwindSafe(|| { ... }))`
    - If a panic is caught, set last error to "Internal error: unexpected panic" and return `EXIFTOOL_ERR_INTERNAL` (99)
    - This prevents Rust panics from unwinding through C code, which is undefined behavior

*   **Tip #4: String Conversion**
    - For input strings (C → Rust): Use `std::ffi::CStr::from_ptr(c_str).to_str()` to convert `*const c_char` to `&str`
    - For output strings (Rust → C): Use `std::ffi::CString::new(rust_string)` to create null-terminated C strings
    - Store CString instances in the handle context to maintain ownership (strings must outlive C calls)
    - Return raw pointers via `CString::as_ptr()` for C consumption

*   **Tip #5: TagValue Type Checking**
    - The `TagValue` enum has variants for different types (String, Integer, Float, etc.)
    - Use pattern matching to check the variant before returning values to C
    - Return appropriate error codes if the requested type doesn't match the actual type

*   **Note #1: Cargo.toml Configuration**
    - You MUST configure `[lib]` section to produce both static and dynamic libraries:
      ```toml
      [lib]
      name = "exiftool_rs"
      crate-type = ["lib", "staticlib", "cdylib"]
      ```
    - This enables: `lib` (for Rust crates), `staticlib` (for static linking), `cdylib` (for dynamic linking)

*   **Note #2: Testing Strategy**
    - The C integration test is optional but highly recommended
    - If you implement it, create a `tests/ffi/` directory with `c_integration_test.c` and `build.rs`
    - The `build.rs` should use the `cc` crate to compile the C test file
    - Link against the Rust library built by cargo

*   **Warning #1: String Lifetime Management**
    - The FFI spec states that returned strings are valid "until next API call or handle destruction"
    - This means you must store CString instances in your handle context struct
    - When returning a string, clear any previous cached strings to avoid accumulating memory
    - Consider using a small cache (e.g., last 5 strings) to handle multiple concurrent string getters

*   **Warning #2: Path Handling**
    - Rust's `std::path::Path` expects valid UTF-8 on most platforms
    - The FFI spec requires UTF-8 input, which aligns with Rust expectations
    - However, on Windows, native paths are UTF-16. Document this clearly in comments.
    - Test with non-ASCII paths to ensure proper encoding handling

### Implementation Structure Recommendation

1. **Define Constants:** Start by defining all error code constants at the top of `c_api.rs`
2. **Thread-Local Storage:** Define the thread-local error storage using `thread_local!` macro
3. **Internal Context Struct:** Define `ExifToolContext` struct to hold `MetadataMap` and cached CStrings
4. **Helper Functions:** Implement private helper functions:
   - `fn set_last_error(msg: String)` - Updates thread-local error
   - `fn error_to_code(err: &ExifToolError) -> c_int` - Maps errors to codes
   - `fn catch_panic<F, T>(f: F) -> Result<T, c_int>` - Wraps functions in panic handler
5. **Public FFI Functions:** Implement each FFI function with:
   - `#[no_mangle]` and `pub extern "C"` attributes
   - Null pointer checks at the start
   - Panic catching wrapper
   - Proper error handling and error message setting
6. **Testing:** If implementing C tests, keep them simple - just verify basic create/read/destroy cycle

### Code Quality Notes

- Add comprehensive doc comments to all public FFI functions explaining safety requirements
- Use `unsafe` blocks judiciously - only where absolutely necessary for raw pointer operations
- Follow the project's existing code style (rustfmt is configured)
- Ensure all error paths set the last error message before returning error codes
- Validate that cargo build --lib succeeds and produces .so/.dll/.dylib artifacts

---

**Good luck implementing the C FFI layer! This is a critical component for cross-language integration.**
