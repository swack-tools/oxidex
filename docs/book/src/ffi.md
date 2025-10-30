# C FFI Integration

This chapter covers how to use ExifTool-RS from C, C++, Python, and other languages through the C Foreign Function Interface (FFI).

## Overview

ExifTool-RS provides a C-compatible Foreign Function Interface (FFI) for reading and writing metadata. This allows integration with:

- **C/C++ applications**: Direct FFI usage
- **Python**: Via ctypes bindings (example provided)
- **Other languages**: Any language with C interoperability (Ruby, JavaScript/Node.js, Go, etc.)

**Key Features:**
- ✅ Safe: No Rust panics cross the FFI boundary
- ✅ Standard C idioms: Return codes, null-terminated strings, opaque handles
- ✅ Explicit error handling: Integer error codes with detailed messages
- ✅ Cross-platform: Linux, macOS, Windows
- ✅ Clear ownership: Memory management rules are explicit and documented

## Building the Shared Library

Before using the FFI, build ExifTool-RS as a shared library:

```bash
# From the repository root
cargo build --lib --release
```

This generates the shared library in `target/release/`:
- **Linux**: `libexiftool_rs.so`
- **macOS**: `libexiftool_rs.dylib`
- **Windows**: `exiftool_rs.dll`

The header file is available at: `include/exiftool_rs.h`

## Quick Start (C)

Here's a minimal working C example:

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

**Compile and link:**

```bash
# Linux
gcc -o example example.c -lexiftool_rs -L./target/release

# macOS
gcc -o example example.c -lexiftool_rs -L./target/release

# Windows (MSVC)
cl example.c exiftool_rs.lib
```

## Core Concepts

### Opaque Handle Pattern

The C FFI uses an **opaque handle** for resource management:

```c
typedef struct ExifToolHandle ExifToolHandle;
```

- C code receives a pointer to an opaque structure
- The Rust library owns the memory
- **Every `exiftool_create()` must have a matching `exiftool_destroy()`**

**Lifecycle:**

```
exiftool_create()     → Returns handle (or NULL on failure)
    ↓
Operations            → exiftool_read_file(), exiftool_get_tag_*()
(many calls)
    ↓
exiftool_destroy()    → Frees handle and all resources
```

### Error Handling

The FFI uses a **two-part error handling system**:

1. **Return Codes**: Functions return integer status codes
2. **Error Messages**: Detailed error messages in thread-local storage

**Pattern:**

```c
int result = exiftool_read_file(handle, "photo.jpg");
if (result != EXIFTOOL_OK) {
    const char* error_msg = exiftool_get_last_error();
    fprintf(stderr, "Error: %s\n", error_msg);
}
```

**Error Codes:**

| Code | Constant | Description |
|------|----------|-------------|
| 0 | `EXIFTOOL_OK` | Success |
| -1 | `EXIFTOOL_ERR_INVALID_HANDLE` | NULL or invalid handle |
| -2 | `EXIFTOOL_ERR_IO` | File I/O error |
| -3 | `EXIFTOOL_ERR_UNSUPPORTED_FORMAT` | File format not supported |
| -4 | `EXIFTOOL_ERR_PARSE_ERROR` | File parsing error |
| -5 | `EXIFTOOL_ERR_INVALID_TAG` | Unknown tag name |
| -6 | `EXIFTOOL_ERR_TYPE_MISMATCH` | Tag type doesn't match request |
| -7 | `EXIFTOOL_ERR_INTERNAL` | Internal error (Rust panic caught) |

### Memory Ownership

**Critical Rules:**

1. **Handles Must Be Destroyed**: Failing to call `exiftool_destroy()` leaks memory
2. **String Lifetimes Are Short**: Returned strings are valid until:
   - Next API call on the same handle
   - Handle destruction
   - **Copy strings immediately if needed beyond the call**
3. **Input Strings**: Must be null-terminated and UTF-8 encoded

| Resource | Owner | Lifetime | Responsibility |
|----------|-------|----------|----------------|
| `ExifToolHandle*` | Library | Until `exiftool_destroy()` | Call `exiftool_destroy()` once |
| Returned strings | Library | Until next call | Copy immediately if needed |
| Input strings | Caller | N/A | Must be null-terminated UTF-8 |

### Thread Safety

- **Handle Isolation**: Each `ExifToolHandle` is independent and thread-safe if not shared
- **No Global State**: Multiple handles can be used simultaneously from different threads
- **Error Messages**: Thread-local storage for error messages
- **Recommendation**: Use one handle per thread or add your own synchronization

## API Reference

### Handle Lifecycle

#### exiftool_create

```c
ExifToolHandle* exiftool_create(void);
```

Creates a new ExifTool handle.

**Returns:**
- Non-NULL: Valid handle
- NULL: Allocation failed

**Example:**

```c
ExifToolHandle* handle = exiftool_create();
if (!handle) {
    fprintf(stderr, "Out of memory\n");
    exit(1);
}
```

#### exiftool_destroy

```c
void exiftool_destroy(ExifToolHandle* handle);
```

Destroys handle and frees all associated resources.

**Parameters:**
- `handle`: Handle to destroy (can be NULL, function is a no-op)

**Example:**

```c
exiftool_destroy(handle);
handle = NULL;  // Good practice
```

### Metadata Reading

#### exiftool_read_file

```c
int exiftool_read_file(ExifToolHandle* handle, const char* file_path);
```

Reads metadata from a file.

**Parameters:**
- `handle`: Valid ExifTool handle
- `file_path`: Null-terminated UTF-8 file path

**Returns:**
- `EXIFTOOL_OK`: Success
- `EXIFTOOL_ERR_*`: Error code (see error codes table)

**Example:**

```c
int result = exiftool_read_file(handle, "/path/to/photo.jpg");
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Read failed: %s\n", exiftool_get_last_error());
}
```

### Tag Access

#### exiftool_get_tag_string

```c
const char* exiftool_get_tag_string(ExifToolHandle* handle, const char* tag_name);
```

Gets a string tag value.

**Parameters:**
- `handle`: Valid handle with metadata loaded
- `tag_name`: Tag name (e.g., "EXIF:Make")

**Returns:**
- Non-NULL: Null-terminated UTF-8 string (valid until next API call)
- NULL: Tag not found or not a string

**Example:**

```c
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
if (make) {
    printf("Camera: %s\n", make);
}
```

#### exiftool_get_tag_integer

```c
int exiftool_get_tag_integer(ExifToolHandle* handle, const char* tag_name, int64_t* out_value);
```

Gets an integer tag value.

**Parameters:**
- `handle`: Valid handle
- `tag_name`: Tag name (e.g., "EXIF:ISO")
- `out_value`: Pointer to store the integer value

**Returns:**
- `EXIFTOOL_OK`: Success
- `EXIFTOOL_ERR_INVALID_TAG`: Tag not found
- `EXIFTOOL_ERR_TYPE_MISMATCH`: Tag is not an integer

**Example:**

```c
int64_t iso;
if (exiftool_get_tag_integer(handle, "EXIF:ISO", &iso) == EXIFTOOL_OK) {
    printf("ISO: %lld\n", (long long)iso);
}
```

#### exiftool_get_tag_float

```c
int exiftool_get_tag_float(ExifToolHandle* handle, const char* tag_name, double* out_value);
```

Gets a float tag value.

**Parameters:**
- `handle`: Valid handle
- `tag_name`: Tag name (e.g., "EXIF:FNumber")
- `out_value`: Pointer to store the float value

**Returns:**
- `EXIFTOOL_OK`: Success
- `EXIFTOOL_ERR_INVALID_TAG`: Tag not found
- `EXIFTOOL_ERR_TYPE_MISMATCH`: Tag is not a float

**Example:**

```c
double aperture;
if (exiftool_get_tag_float(handle, "EXIF:FNumber", &aperture) == EXIFTOOL_OK) {
    printf("Aperture: f/%.1f\n", aperture);
}
```

#### exiftool_get_tag_count

```c
size_t exiftool_get_tag_count(ExifToolHandle* handle);
```

Returns the number of tags in loaded metadata.

**Returns:**
- Number of tags (0 if no metadata loaded)

**Example:**

```c
size_t count = exiftool_get_tag_count(handle);
printf("Found %zu tags\n", count);
```

#### exiftool_get_tag_name_at

```c
const char* exiftool_get_tag_name_at(ExifToolHandle* handle, size_t index);
```

Gets the tag name at a specific index (for iteration).

**Parameters:**
- `handle`: Valid handle
- `index`: Index (0 to count-1)

**Returns:**
- Non-NULL: Tag name string (valid until next API call)
- NULL: Index out of bounds

**Example:**

```c
size_t count = exiftool_get_tag_count(handle);
for (size_t i = 0; i < count; i++) {
    const char* tag_name = exiftool_get_tag_name_at(handle, i);
    if (tag_name) {
        const char* value = exiftool_get_tag_string(handle, tag_name);
        printf("%s: %s\n", tag_name, value ? value : "(non-string)");
    }
}
```

### Error Handling Functions

#### exiftool_get_last_error

```c
const char* exiftool_get_last_error(void);
```

Gets the last error message for the current thread.

**Returns:**
- Null-terminated error message string
- Empty string if no error

**Example:**

```c
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
}
```

## Code Examples

### Example 1: Extract Camera Info

Complete example extracting camera metadata:

```c
#include "exiftool_rs.h"
#include <stdio.h>

int main(int argc, char** argv) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <image_file>\n", argv[0]);
        return 1;
    }

    ExifToolHandle* handle = exiftool_create();
    if (!handle) {
        fprintf(stderr, "Failed to create handle\n");
        return 1;
    }

    // Read file
    if (exiftool_read_file(handle, argv[1]) != EXIFTOOL_OK) {
        fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    // Extract camera info
    printf("Camera Information:\n");
    printf("------------------\n");

    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
    if (make) printf("Make:   %s\n", make);

    const char* model = exiftool_get_tag_string(handle, "EXIF:Model");
    if (model) printf("Model:  %s\n", model);

    int64_t iso;
    if (exiftool_get_tag_integer(handle, "EXIF:ISO", &iso) == EXIFTOOL_OK) {
        printf("ISO:    %lld\n", (long long)iso);
    }

    double aperture;
    if (exiftool_get_tag_float(handle, "EXIF:FNumber", &aperture) == EXIFTOOL_OK) {
        printf("F-Stop: f/%.1f\n", aperture);
    }

    // Cleanup
    exiftool_destroy(handle);
    return 0;
}
```

### Example 2: Iterate All Tags

```c
#include "exiftool_rs.h"
#include <stdio.h>

int main(int argc, char** argv) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <image_file>\n", argv[0]);
        return 1;
    }

    ExifToolHandle* handle = exiftool_create();
    if (!handle) return 1;

    if (exiftool_read_file(handle, argv[1]) != EXIFTOOL_OK) {
        fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    size_t count = exiftool_get_tag_count(handle);
    printf("Found %zu tags:\n\n", count);

    for (size_t i = 0; i < count; i++) {
        const char* tag_name = exiftool_get_tag_name_at(handle, i);
        if (!tag_name) continue;

        // Try string first
        const char* str_val = exiftool_get_tag_string(handle, tag_name);
        if (str_val) {
            printf("%-30s: %s\n", tag_name, str_val);
            continue;
        }

        // Try integer
        int64_t int_val;
        if (exiftool_get_tag_integer(handle, tag_name, &int_val) == EXIFTOOL_OK) {
            printf("%-30s: %lld\n", tag_name, (long long)int_val);
            continue;
        }

        // Try float
        double float_val;
        if (exiftool_get_tag_float(handle, tag_name, &float_val) == EXIFTOOL_OK) {
            printf("%-30s: %.4f\n", tag_name, float_val);
            continue;
        }

        printf("%-30s: (unknown type)\n", tag_name);
    }

    exiftool_destroy(handle);
    return 0;
}
```

## Python Bindings

ExifTool-RS includes a reference Python binding implementation using ctypes.

### Installation

No pip install needed. Ensure the shared library is findable:

```bash
# Option 1: Set library path
export LD_LIBRARY_PATH=/path/to/exiftool-rs/target/release:$LD_LIBRARY_PATH  # Linux
export DYLD_LIBRARY_PATH=/path/to/exiftool-rs/target/release:$DYLD_LIBRARY_PATH  # macOS

# Option 2: Copy to system location
sudo cp target/release/libexiftool_rs.so /usr/local/lib/  # Linux
sudo cp target/release/libexiftool_rs.dylib /usr/local/lib/  # macOS
```

### Python Usage Example

```python
from exiftool_rs import ExifTool, ExifToolError

try:
    # Use context manager for automatic cleanup
    with ExifTool() as et:
        # Read metadata from file
        et.read_file("photo.jpg")

        # Get specific tags
        make = et.get_tag("EXIF:Make")
        model = et.get_tag("EXIF:Model")
        iso = et.get_tag("EXIF:ISO")

        print(f"Camera: {make} {model}")
        print(f"ISO: {iso}")

        # Get all tags as dictionary
        all_tags = et.get_all_tags()
        for tag_name, tag_value in all_tags.items():
            print(f"{tag_name}: {tag_value}")

except ExifToolError as e:
    print(f"Error: {e}")
```

### Python API

#### ExifTool Class

**Constructor:**

```python
et = ExifTool(lib_path=None)
```

- `lib_path`: Optional path to shared library (auto-detected if None)

**Methods:**

- `read_file(path: str)`: Read metadata from file
- `get_tag(tag_name: str) -> Optional[str]`: Get tag value as string
- `get_tag_count() -> int`: Get number of tags
- `get_tag_name_at(index: int) -> Optional[str]`: Get tag name at index
- `get_all_tags() -> dict`: Get all tags as dictionary

**Context Manager Support:**

```python
with ExifTool() as et:
    et.read_file("photo.jpg")
    # Automatic cleanup on exit
```

## Platform-Specific Notes

### Linux

**Linking:**

```bash
gcc -o app app.c -lexiftool_rs -L./target/release
```

**Runtime Library Path:**

```bash
export LD_LIBRARY_PATH=/path/to/lib:$LD_LIBRARY_PATH
./app photo.jpg
```

### macOS

**Linking:**

```bash
gcc -o app app.c -lexiftool_rs -L./target/release
```

**Runtime Library Path:**

```bash
export DYLD_LIBRARY_PATH=/path/to/lib:$DYLD_LIBRARY_PATH
./app photo.jpg
```

**Apple Silicon Note**: Native ARM64 support. No Rosetta required.

### Windows

**MSVC Linking:**

```batch
cl app.c exiftool_rs.lib
```

**Runtime DLL:**

Ensure `exiftool_rs.dll` is in:
- Same directory as executable
- Or in system PATH

**MinGW/GNU:**

```bash
gcc -o app.exe app.c -lexiftool_rs -L./target/release
```

## Best Practices

### 1. Always Destroy Handles

```c
ExifToolHandle* handle = exiftool_create();
// ... use handle ...
exiftool_destroy(handle);  // Don't forget!
```

### 2. Copy Returned Strings Immediately

```c
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
if (make) {
    char* make_copy = strdup(make);  // Copy before next API call
    // Use make_copy safely
    free(make_copy);
}
```

### 3. Check All Return Codes

```c
if (exiftool_read_file(handle, path) != EXIFTOOL_OK) {
    fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
    // Handle error...
}
```

### 4. Use Context Managers in Python

```python
with ExifTool() as et:
    et.read_file("photo.jpg")
    # Automatic cleanup even if exception occurs
```

### 5. Handle NULL Returns

```c
const char* value = exiftool_get_tag_string(handle, tag);
if (value) {
    // Use value
} else {
    // Tag not found or not a string
}
```

### 6. Validate Tag Names

Tag names are case-sensitive:
- ✅ `EXIF:Make`
- ❌ `exif:make`
- ❌ `Exif:Make`

### 7. Thread Safety

Don't share handles between threads without synchronization:

```c
// Good: One handle per thread
void* thread_func(void* arg) {
    ExifToolHandle* handle = exiftool_create();
    // Use handle...
    exiftool_destroy(handle);
    return NULL;
}

// Bad: Sharing handle without lock
// ExifToolHandle* global_handle;  // DON'T DO THIS
```

## Additional Resources

- **[Full FFI API Reference](../api/ffi_api.md)**: Complete documentation (1500+ lines)
- **[Python Bindings README](../../bindings/python/README.md)**: Python-specific documentation
- **[Library API](library_api.md)**: Native Rust API (more features)
- **[Command-Line Usage](cli_usage.md)**: CLI interface

## Building Language Bindings

To create bindings for other languages:

1. **Study the C API**: Header file at `include/exiftool_rs.h`
2. **Use the pattern**: Most languages support C FFI (JNI for Java, cffi for Python, cgo for Go)
3. **Follow Python example**: See `bindings/python/` for a complete reference
4. **Handle errors properly**: Convert C error codes to native exceptions
5. **Wrap in high-level API**: Provide idiomatic API for your language
6. **Add context managers**: RAII, `with` blocks, `using`, etc.
7. **Document lifetimes**: Make memory ownership clear to users

Good luck building your language binding!
