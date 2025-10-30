# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T4",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Create example Python bindings in bindings/python/ using ctypes to demonstrate C FFI usage. Implement thin wrapper: load shared library (ctypes.CDLL), declare function signatures, provide Pythonic interface (class ExifTool wrapping handle). Include example script extracting and printing metadata. Add README with installation and usage instructions. This is a minimal reference implementation to prove FFI works, not a production-quality binding.",
  "agent_type_hint": "DocumentationAgent",
  "inputs": "I5.T2 FFI implementation, I5.T3 C header",
  "target_files": [
    "bindings/python/exiftool_rs.py",
    "bindings/python/example.py",
    "bindings/python/README.md"
  ],
  "input_files": [
    "api/exiftool_rs.h",
    "src/ffi/c_api.rs"
  ],
  "deliverables": "Python ctypes wrapper, example usage script, documentation",
  "acceptance_criteria": "Python script loads shared library successfully, wrapper creates handle, reads file, extracts tags, destroys handle, example script prints metadata from sample JPEG, README documents installation (build Rust library, install Python, run example), no memory leaks (verified with handle lifecycle)",
  "dependencies": [
    "I5.T2"
  ],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: api-design-communication (from 04_Behavior_and_Communication.md)

```markdown
### 3.7. API Design & Communication

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
```

### Context: task-i5-t4 (from 02_Iteration_I5.md)

```markdown
*   **Task 5.4: Create Python Bindings Example with ctypes**
    *   **Task ID:** `I5.T4`
    *   **Description:** Create example Python bindings in `bindings/python/` using `ctypes` to demonstrate C FFI usage. Implement thin wrapper: load shared library (`ctypes.CDLL`), declare function signatures, provide Pythonic interface (`class ExifTool` wrapping handle). Include example script extracting and printing metadata. Add README with installation and usage instructions. This is a minimal reference implementation to prove FFI works, not a production-quality binding.
    *   **Agent Type Hint:** `DocumentationAgent` or `BackendAgent`
    *   **Inputs:** I5.T2 FFI implementation, I5.T3 C header
    *   **Input Files:** [`api/exiftool_rs.h`, `src/ffi/c_api.rs`]
    *   **Target Files:**
        *   `bindings/python/exiftool_rs.py`
        *   `bindings/python/example.py`
        *   `bindings/python/README.md`
    *   **Deliverables:**
        *   Python ctypes wrapper
        *   Example usage script
        *   Documentation
    *   **Acceptance Criteria:**
        *   Python script loads shared library successfully
        *   Wrapper creates handle, reads file, extracts tags, destroys handle
        *   Example script prints metadata from sample JPEG
        *   README documents installation (build Rust library, install Python, run example)
        *   No memory leaks (verified with handle lifecycle)
    *   **Dependencies:** `I5.T2` (needs compiled shared library)
    *   **Parallelizable:** Yes (can be developed after FFI is implemented)
```

### Context: FFI API Documentation - Core Concepts (from ffi_api.md)

```markdown
## Core Concepts

### Opaque Handle Pattern

The C FFI uses an **opaque handle** pattern for resource management. C code receives a pointer to an opaque structure (`ExifToolHandle*`) that encapsulates Rust objects:

```c
typedef struct ExifToolHandle ExifToolHandle;
```

**Key Properties:**

- **Opaque**: C code cannot access the internal structure
- **Owned by Library**: The Rust library owns the memory
- **Must Be Destroyed**: Every `exiftool_create()` must have a matching `exiftool_destroy()`

**Lifecycle:**

```
┌─────────────────┐
│ exiftool_create │  Returns handle (or NULL on failure)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Operations      │  exiftool_read_file(), exiftool_get_tag_*(), etc.
│ (many calls)    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│exiftool_destroy │  Frees handle and all associated resources
└─────────────────┘
```

**Why Opaque Handles?**

1. **ABI Stability**: Internal representation can change without breaking C code
2. **Safety**: Prevents C code from corrupting Rust memory
3. **Resource Management**: Clear ownership boundaries

### Error Handling

The FFI uses a **two-part error handling system**:

1. **Return Codes**: Functions return integer status codes
2. **Error Messages**: Detailed error messages stored in thread-local storage

**Pattern:**

```c
int result = exiftool_some_operation(handle, args);
if (result != EXIFTOOL_OK) {
    const char* error_msg = exiftool_get_last_error();
    fprintf(stderr, "Error: %s\n", error_msg);
}
```

**Why This Design?**

- **Standard C Practice**: Matches conventions from `errno`, SQLite, OpenSSL
- **Error Context**: Return code for quick checks, message for detailed diagnostics
- **Thread-Safe**: Each thread has its own error message storage

**Critical Safety Rule:**

> **No Rust panics will ever cross the FFI boundary.**
> All potential panics are caught and converted to `EXIFTOOL_ERR_INTERNAL` error codes.

### Memory Ownership

Memory ownership rules are **explicit and strict**:

| **Resource** | **Owner** | **Lifetime** | **Caller Responsibility** |
|--------------|-----------|--------------|---------------------------|
| `ExifToolHandle*` | Library | Until `exiftool_destroy()` | Must call `exiftool_destroy()` exactly once |
| Returned strings (`const char*`) | Library | Until next API call or handle destruction | Copy immediately if needed beyond call |
| Input strings (`const char*`) | Caller | N/A | Must be null-terminated, UTF-8 encoded |
| Output pointers (`int64_t*`, `double*`) | Caller | N/A | Must provide valid, non-NULL pointer |

**Critical Rules:**

1. **Handles Must Be Destroyed**: Failing to call `exiftool_destroy()` leaks memory
2. **String Lifetimes Are Short**: Returned strings are invalidated by:
   - Next API call on same handle
   - Handle destruction
   - Thread termination (for error messages)
3. **Input Strings Are Copied**: Library makes internal copies, caller retains ownership
4. **Binary Data Uses Explicit Length**: Never rely on null-termination for binary data
```

### Context: FFI Quick Start Example (from ffi_api.md)

```markdown
## Quick Start

Here's a minimal working example to get you started:

```c
#include "exiftool_rs.h"
#include <stdio.h>
#include <stdlib.h>

int main() {
    // Create handle
    ExifToolHandle* handle = exiftool_create();
    if (!handle) {
        fprintf(stderr, "Failed to create handle\n");
        return 1;
    }

    // Read metadata from file
    int result = exiftool_read_file(handle, "photo.jpg");
    if (result != EXIFTOOL_OK) {
        fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    // Get camera make
    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
    if (make) {
        printf("Camera: %s\n", make);
    }

    // Clean up
    exiftool_destroy(handle);
    return 0;
}
```
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `api/exiftool_rs.h`
    *   **Summary:** This is the auto-generated C header file (via cbindgen) that defines the complete C FFI API. It contains all function signatures, error codes, and the opaque handle type definition.
    *   **Recommendation:** You MUST use this header as your reference for all FFI function signatures when creating Python ctypes declarations. The header defines:
        - Error codes: `EXIFTOOL_OK (0)`, `EXIFTOOL_ERR_IO (1)`, `EXIFTOOL_ERR_PARSE (2)`, `EXIFTOOL_ERR_TAG_NOT_FOUND (3)`, `EXIFTOOL_ERR_INVALID_TAG_VALUE (4)`, `EXIFTOOL_ERR_UNSUPPORTED_FORMAT (5)`, `EXIFTOOL_ERR_NULL_POINTER (6)`, `EXIFTOOL_ERR_INTERNAL (99)`
        - Key functions: `exiftool_create()`, `exiftool_destroy()`, `exiftool_read_file()`, `exiftool_get_tag_string()`, `exiftool_get_tag_count()`, `exiftool_get_tag_name()`, `exiftool_get_last_error()`
        - Opaque handle type: `struct ExifToolHandle`

*   **File:** `src/ffi/c_api.rs`
    *   **Summary:** This is the complete Rust implementation of the C FFI layer. It demonstrates the exact behavior of each FFI function including error handling, panic catching, and memory management.
    *   **Recommendation:** You SHOULD review this file to understand:
        - How handles are created (Box::into_raw) and destroyed (Box::from_raw)
        - Error handling pattern: catch_unwind wraps all operations, errors are converted to codes and stored in thread-local storage
        - String lifetime management: strings are cached in the context and invalidated on next call
        - Key implementation details:
          - `exiftool_create()` returns NULL on allocation failure
          - `exiftool_destroy()` safely handles NULL pointers
          - `exiftool_read_file()` clears string cache and rebuilds tag cache on success
          - `exiftool_get_tag_string()` returns NULL for missing tags or non-string types
          - Returned strings are valid until next API call or handle destruction

*   **File:** `Cargo.toml`
    *   **Summary:** Project configuration defining library build targets. Line 16 specifies: `crate-type = ["lib", "staticlib", "cdylib"]`
    *   **Recommendation:** You MUST build the project with `cargo build --lib --release` to generate the shared library. The library will be located at:
        - Linux: `target/release/libexiftool_rs.so`
        - macOS: `target/release/libexiftool_rs.dylib`
        - Windows: `target/release/exiftool_rs.dll`
    *   **Note:** The Python wrapper needs to know which library name to load based on the platform.

*   **File:** `tests/fixtures/jpeg/sample_with_exif.jpg`
    *   **Summary:** Sample JPEG file with EXIF metadata for testing.
    *   **Recommendation:** You SHOULD use this file (or `sample_with_exif_xmp.jpg`) in your example.py script to demonstrate the Python bindings. These files are known-good test fixtures that contain readable metadata.

*   **File:** `docs/api/ffi_api.md`
    *   **Summary:** Comprehensive documentation of the C FFI API including design principles, usage patterns, memory management rules, and thread safety considerations.
    *   **Recommendation:** You SHOULD extract key sections from this documentation and adapt them for Python users in your README. Focus on:
        - The handle lifecycle pattern (create → operations → destroy)
        - Error handling approach (check return codes, get error messages)
        - Memory/string lifetime rules (copy strings immediately if needed beyond the call)
        - Thread safety (one handle per thread)

### Implementation Tips & Notes

*   **Tip - Library Loading:** Use `ctypes.util.find_library()` to locate the shared library in a cross-platform way. However, this often fails to find custom libraries, so provide a fallback that searches common locations (`./target/release/`, `../target/release/`, `../../target/release/`, system paths). Document in README that users may need to set `LD_LIBRARY_PATH` (Linux), `DYLD_LIBRARY_PATH` (macOS), or `PATH` (Windows).

*   **Tip - Function Signatures:** When declaring ctypes function signatures, follow this pattern:
    ```python
    # Define return type and argument types
    lib.exiftool_create.restype = ctypes.c_void_p
    lib.exiftool_create.argtypes = []

    lib.exiftool_destroy.restype = None
    lib.exiftool_destroy.argtypes = [ctypes.c_void_p]

    lib.exiftool_read_file.restype = ctypes.c_int
    lib.exiftool_read_file.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    lib.exiftool_get_tag_string.restype = ctypes.c_char_p
    lib.exiftool_get_tag_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    lib.exiftool_get_last_error.restype = ctypes.c_char_p
    lib.exiftool_get_last_error.argtypes = []
    ```

*   **Tip - Python Class Wrapper:** Create a Pythonic wrapper class that:
    - Uses `__init__()` to call `exiftool_create()` and store the handle
    - Uses `__del__()` to call `exiftool_destroy()` for automatic cleanup
    - Provides context manager support (`__enter__` and `__exit__`) for explicit resource management
    - Converts error codes to Python exceptions for better error handling
    - Encodes strings to UTF-8 bytes when passing to C functions
    - Decodes returned C strings from bytes to Python strings

*   **Tip - Error Handling Pattern:** Implement a helper method that checks return codes and raises exceptions:
    ```python
    def _check_error(self, result):
        if result != 0:  # EXIFTOOL_OK = 0
            error_msg = self._lib.exiftool_get_last_error()
            if error_msg:
                raise ExifToolError(error_msg.decode('utf-8'))
            else:
                raise ExifToolError(f"Unknown error (code {result})")
    ```

*   **Warning - String Lifetime:** Returned strings from `exiftool_get_tag_string()` are only valid until the next API call. Your Python wrapper MUST copy these strings immediately:
    ```python
    # CORRECT - copy immediately
    c_str = lib.exiftool_get_tag_string(handle, b"EXIF:Make")
    if c_str:
        value = c_str.decode('utf-8')  # This creates a Python copy

    # WRONG - storing pointer
    c_str = lib.exiftool_get_tag_string(handle, b"EXIF:Make")
    # ... later use - may be invalid
    ```

*   **Note - Platform Differences:** The shared library has different extensions on different platforms:
    - Linux: `.so`
    - macOS: `.dylib`
    - Windows: `.dll`

    Your Python wrapper should detect the platform and try the appropriate extension. Use `sys.platform` to determine the OS.

*   **Note - Minimal Implementation:** The task description specifies this is "a minimal reference implementation to prove FFI works, not a production-quality binding." Therefore:
    - Focus on demonstrating core functionality: create handle, read file, get tags, destroy handle
    - You DO NOT need to wrap every FFI function (e.g., you can skip write functions, iteration, tag count, etc.)
    - Keep error handling simple but correct
    - Prioritize clarity and documentation over feature completeness

*   **Tip - README Structure:** Your README should include:
    1. **Introduction:** Brief explanation of what this is (Python bindings for ExifTool-RS)
    2. **Prerequisites:** Python 3.7+, built Rust library
    3. **Building the Library:** Instructions to run `cargo build --lib --release`
    4. **Installation:** No pip install needed, just ensure library is findable
    5. **Usage Example:** Copy from example.py
    6. **Limitations:** Clearly state this is a minimal demo, not production-ready
    7. **Troubleshooting:** Common issues like library not found, how to set LD_LIBRARY_PATH

*   **Tip - Example Script:** Your example.py should:
    - Use the context manager pattern (`with ExifTool() as et:`) to demonstrate proper cleanup
    - Read the test fixture: `../../tests/fixtures/jpeg/sample_with_exif.jpg` (relative path from bindings/python/)
    - Print several tags: EXIF:Make, EXIF:Model, EXIF:DateTime
    - Show error handling (e.g., try reading a non-existent file)
    - Be runnable from the bindings/python/ directory

### Critical Architecture Constraints

*   **Constraint - Crate Type Configuration:** The Cargo.toml already specifies `crate-type = ["lib", "staticlib", "cdylib"]` on line 16, which means the project is configured to generate both static and dynamic libraries. No changes needed to Cargo.toml.

*   **Constraint - FFI Safety:** All FFI functions catch panics at the boundary via `catch_unwind()`. Your Python bindings can safely assume that no Rust panic will crash the Python interpreter - panics will be converted to error codes (`EXIFTOOL_ERR_INTERNAL`).

*   **Constraint - No Modifications to Rust Code:** This task is purely about creating Python bindings. You MUST NOT modify any Rust source files in `src/`. The FFI API is already complete and tested (tasks I5.T2 and I5.T3 are done).

*   **Constraint - Directory Structure:** Create the new `bindings/python/` directory. This follows the standard convention for language bindings in multi-language projects.

### Suggested Implementation Order

1. **Create Directory:** `mkdir -p bindings/python`
2. **Create exiftool_rs.py:**
   - Load library with platform detection
   - Declare FFI function signatures
   - Create ExifToolError exception class
   - Create ExifTool wrapper class with handle lifecycle management
   - Implement read_file() and get_tag() methods
3. **Create example.py:**
   - Import the wrapper
   - Demonstrate usage with context manager
   - Read test fixture and print metadata
   - Show error handling
4. **Create README.md:**
   - Document prerequisites and build instructions
   - Show usage example (copy from example.py)
   - Add troubleshooting section
5. **Test Manually:**
   - Build library: `cargo build --lib --release`
   - Run example: `cd bindings/python && python3 example.py`
   - Verify no errors, metadata prints correctly

This order ensures you build incrementally with each piece testable.
