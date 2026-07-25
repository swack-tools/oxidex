# FFI API Reference

C Foreign Function Interface for cross-language integration with OxiDex.

## Overview

OxiDex provides a C-compatible Foreign Function Interface (FFI) for reading and writing metadata. This allows integration with:

- **C/C++ applications** - Direct FFI usage
- **Python** - Via ctypes bindings
- **Other languages** - Any language with C interoperability (Ruby, JavaScript/Node.js, Go, etc.)

**Key Features:**
- Safe: No Rust panics cross the FFI boundary
- Standard C idioms: Return codes, null-terminated strings, opaque handles
- Explicit error handling: Integer error codes with detailed messages
- Cross-platform: Linux, macOS, Windows
- Clear ownership: Memory management rules are explicit

## Building the Shared Library

Build OxiDex as a shared library:

```bash
# From the repository root
cargo build --lib --release
```

This generates the shared library in `target/release/`:
- **Linux**: `liboxidex.so`
- **macOS**: `liboxidex.dylib`
- **Windows**: `oxidex.dll`

The header file is available at: `include/oxidex.h`

## Quick Start (C)

Minimal working C example:

```c
#include "oxidex.h"
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
gcc -o example example.c -loxidex -L./target/release

# macOS
gcc -o example example.c -loxidex -L./target/release

# Windows (MSVC)
cl example.c oxidex.lib
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

1. **Return Codes** - Functions return integer status codes
2. **Error Messages** - Detailed error messages in thread-local storage

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

1. **Handles Must Be Destroyed** - Failing to call `exiftool_destroy()` leaks memory
2. **String Lifetimes Are Short** - Returned strings are valid until:
   - Next API call on the same handle
   - Handle destruction
   - **Copy strings immediately if needed beyond the call**
3. **Input Strings** - Must be null-terminated and UTF-8 encoded

| Resource | Owner | Lifetime | Responsibility |
|----------|-------|----------|----------------|
| `ExifToolHandle*` | Library | Until `exiftool_destroy()` | Call `exiftool_destroy()` once |
| Returned strings | Library | Until next call | Copy immediately if needed |
| Input strings | Caller | N/A | Must be null-terminated UTF-8 |

### Thread Safety

- **Handle Isolation** - Each `ExifToolHandle` is independent and thread-safe if not shared
- **No Global State** - Multiple handles can be used simultaneously from different threads
- **Error Messages** - Thread-local storage for error messages
- **Recommendation** - Use one handle per thread or add your own synchronization

## API Reference

### Handle Lifecycle

#### `exiftool_create()`

Creates a new ExifTool handle.

```c
ExifToolHandle* exiftool_create(void);
```

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

#### `exiftool_destroy()`

Destroys handle and frees all associated resources.

```c
void exiftool_destroy(ExifToolHandle* handle);
```

**Parameters:**
- `handle`: Handle to destroy (can be NULL, function is a no-op)

**Example:**

```c
exiftool_destroy(handle);
handle = NULL;  // Good practice
```

### Reading Metadata

#### `exiftool_read_file()`

Loads metadata from a file.

```c
int exiftool_read_file(ExifToolHandle* handle, const char* path);
```

**Parameters:**
- `handle`: Valid handle
- `path`: Null-terminated UTF-8 file path

**Returns:**
- `EXIFTOOL_OK` on success
- Error code on failure

**Example:**

```c
int result = exiftool_read_file(handle, "photo.jpg");
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Failed to read: %s\n", exiftool_get_last_error());
}
```

#### `exiftool_read_bytes()`

Loads metadata from memory buffer.

```c
int exiftool_read_bytes(ExifToolHandle* handle, const uint8_t* data, size_t len);
```

**Parameters:**
- `handle`: Valid handle
- `data`: Pointer to file data
- `len`: Length of data in bytes

**Returns:**
- `EXIFTOOL_OK` on success
- Error code on failure

**Example:**

```c
uint8_t* buffer = read_file_into_memory("photo.jpg", &size);
int result = exiftool_read_bytes(handle, buffer, size);
```

### Getting Tag Values

#### `exiftool_get_tag_string()`

Gets tag value as a null-terminated string.

```c
const char* exiftool_get_tag_string(ExifToolHandle* handle, const char* tag_name);
```

**Parameters:**
- `handle`: Valid handle
- `tag_name`: Tag name (e.g., "EXIF:Make")

**Returns:**
- Non-NULL: Tag value as string
- NULL: Tag not found or type mismatch

**Example:**

```c
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
if (make) {
    printf("Camera: %s\n", make);
}
```

**Important:** Copy the string immediately if you need it beyond the next call:

```c
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
if (make) {
    char* make_copy = strdup(make);  // Copy to heap
    // Use make_copy...
    free(make_copy);
}
```

#### `exiftool_get_tag_integer()`

Gets tag value as a 64-bit integer.

```c
int exiftool_get_tag_integer(ExifToolHandle* handle, const char* tag_name, int64_t* out);
```

**Parameters:**
- `handle`: Valid handle
- `tag_name`: Tag name (e.g., "EXIF:ISO")
- `out`: Pointer to store result

**Returns:**
- `EXIFTOOL_OK` on success
- Error code if tag not found or wrong type

**Example:**

```c
int64_t iso;
if (exiftool_get_tag_integer(handle, "EXIF:ISO", &iso) == EXIFTOOL_OK) {
    printf("ISO: %lld\n", iso);
}
```

#### `exiftool_get_tag_float()`

Gets tag value as a 64-bit floating-point number.

```c
int exiftool_get_tag_float(ExifToolHandle* handle, const char* tag_name, double* out);
```

**Parameters:**
- `handle`: Valid handle
- `tag_name`: Tag name (e.g., "EXIF:FNumber")
- `out`: Pointer to store result

**Returns:**
- `EXIFTOOL_OK` on success
- Error code if tag not found or wrong type

**Example:**

```c
double aperture;
if (exiftool_get_tag_float(handle, "EXIF:FNumber", &aperture) == EXIFTOOL_OK) {
    printf("f/%.1f\n", aperture);
}
```

### Writing Metadata

#### `exiftool_set_tag_string()`

Sets tag value from string.

```c
int exiftool_set_tag_string(ExifToolHandle* handle, const char* tag_name, const char* value);
```

**Parameters:**
- `handle`: Valid handle
- `tag_name`: Tag name
- `value`: Null-terminated UTF-8 string value

**Returns:**
- `EXIFTOOL_OK` on success
- Error code on failure

**Example:**

```c
int result = exiftool_set_tag_string(handle, "EXIF:Artist", "Jane Doe");
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Failed to set tag: %s\n", exiftool_get_last_error());
}
```

#### `exiftool_set_tag_integer()`

Sets tag value from 64-bit integer.

```c
int exiftool_set_tag_integer(ExifToolHandle* handle, const char* tag_name, int64_t value);
```

**Example:**

```c
exiftool_set_tag_integer(handle, "EXIF:ISO", 400);
```

#### `exiftool_set_tag_float()`

Sets tag value from 64-bit floating-point.

```c
int exiftool_set_tag_float(ExifToolHandle* handle, const char* tag_name, double value);
```

**Example:**

```c
exiftool_set_tag_float(handle, "EXIF:FNumber", 2.8);
```

#### `exiftool_remove_tag()`

Removes a tag from metadata.

```c
int exiftool_remove_tag(ExifToolHandle* handle, const char* tag_name);
```

**Example:**

```c
exiftool_remove_tag(handle, "EXIF:Thumbnail");
```

#### `exiftool_write_file()`

Writes modified metadata to file.

```c
int exiftool_write_file(ExifToolHandle* handle, const char* path);
```

**Parameters:**
- `handle`: Valid handle with modifications
- `path`: Output file path

**Returns:**
- `EXIFTOOL_OK` on success
- Error code on failure

**Example:**

```c
exiftool_set_tag_string(handle, "EXIF:Artist", "Jane Doe");
int result = exiftool_write_file(handle, "output.jpg");
```

### Utility Functions

#### `exiftool_get_last_error()`

Gets detailed error message for last failed operation.

```c
const char* exiftool_get_last_error(void);
```

**Returns:**
- Error message string (thread-local)

**Example:**

```c
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
}
```

#### `exiftool_get_tag_count()`

Gets number of tags in current metadata.

```c
size_t exiftool_get_tag_count(ExifToolHandle* handle);
```

**Returns:**
- Number of tags (0 if no file loaded or empty metadata)

**Example:**

```c
size_t count = exiftool_get_tag_count(handle);
printf("Found %zu tags\n", count);
```

#### `exiftool_get_tag_names()`

Gets all tag names.

```c
int exiftool_get_tag_names(ExifToolHandle* handle, const char** names, size_t* count);
```

**Parameters:**
- `handle`: Valid handle
- `names`: Array to store tag name pointers
- `count`: Input: array size, Output: number of tags

**Returns:**
- `EXIFTOOL_OK` on success

**Example:**

```c
size_t count = exiftool_get_tag_count(handle);
const char** names = malloc(count * sizeof(char*));
exiftool_get_tag_names(handle, names, &count);

for (size_t i = 0; i < count; i++) {
    printf("%s\n", names[i]);
}

free(names);
```

## Language Bindings

### Python Example

Using `ctypes`:

```python
import ctypes
import os

# Load shared library
lib_path = "./target/release/liboxidex.so"  # Linux
# lib_path = "./target/release/liboxidex.dylib"  # macOS
# lib_path = "./target/release/oxidex.dll"  # Windows

lib = ctypes.CDLL(lib_path)

# Define function signatures
lib.exiftool_create.restype = ctypes.c_void_p
lib.exiftool_destroy.argtypes = [ctypes.c_void_p]
lib.exiftool_read_file.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
lib.exiftool_read_file.restype = ctypes.c_int
lib.exiftool_get_tag_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
lib.exiftool_get_tag_string.restype = ctypes.c_char_p
lib.exiftool_get_last_error.restype = ctypes.c_char_p

class ExifTool:
    def __init__(self):
        self.handle = lib.exiftool_create()
        if not self.handle:
            raise MemoryError("Failed to create handle")

    def __del__(self):
        if self.handle:
            lib.exiftool_destroy(self.handle)

    def read_file(self, path):
        result = lib.exiftool_read_file(self.handle, path.encode('utf-8'))
        if result != 0:
            error = lib.exiftool_get_last_error()
            raise RuntimeError(error.decode('utf-8'))

    def get_tag(self, tag_name):
        value = lib.exiftool_get_tag_string(self.handle, tag_name.encode('utf-8'))
        if value:
            return value.decode('utf-8')
        return None

# Usage
tool = ExifTool()
tool.read_file("photo.jpg")
make = tool.get_tag("EXIF:Make")
print(f"Camera: {make}")
```

### JavaScript (Node.js) Example

Using `ffi-napi`:

```javascript
const ffi = require('ffi-napi');
const ref = require('ref-napi');

const lib = ffi.Library('./target/release/liboxidex', {
  'exiftool_create': ['pointer', []],
  'exiftool_destroy': ['void', ['pointer']],
  'exiftool_read_file': ['int', ['pointer', 'string']],
  'exiftool_get_tag_string': ['string', ['pointer', 'string']],
  'exiftool_get_last_error': ['string', []]
});

class ExifTool {
  constructor() {
    this.handle = lib.exiftool_create();
    if (this.handle.isNull()) {
      throw new Error('Failed to create handle');
    }
  }

  readFile(path) {
    const result = lib.exiftool_read_file(this.handle, path);
    if (result !== 0) {
      throw new Error(lib.exiftool_get_last_error());
    }
  }

  getTag(tagName) {
    return lib.exiftool_get_tag_string(this.handle, tagName);
  }

  close() {
    lib.exiftool_destroy(this.handle);
    this.handle = null;
  }
}

// Usage
const tool = new ExifTool();
tool.readFile('photo.jpg');
const make = tool.getTag('EXIF:Make');
console.log(`Camera: ${make}`);
tool.close();
```

## Best Practices

### Error Handling

Always check return codes:

```c
int result = exiftool_read_file(handle, path);
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Error reading %s: %s\n", path, exiftool_get_last_error());
    exiftool_destroy(handle);
    return 1;
}
```

### Resource Management

Use RAII-style patterns in C++:

```cpp
class ExifToolRAII {
    ExifToolHandle* handle;
public:
    ExifToolRAII() : handle(exiftool_create()) {
        if (!handle) throw std::bad_alloc();
    }
    ~ExifToolRAII() { exiftool_destroy(handle); }
    ExifToolHandle* get() { return handle; }
};

// Usage
{
    ExifToolRAII tool;
    exiftool_read_file(tool.get(), "photo.jpg");
    // Automatic cleanup on scope exit
}
```

### String Copying

Always copy strings immediately:

```c
// WRONG - dangling pointer after next call
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
exiftool_get_tag_string(handle, "EXIF:Model");  // make is now invalid!
printf("%s\n", make);  // UNDEFINED BEHAVIOR

// CORRECT - copy immediately
const char* make_ptr = exiftool_get_tag_string(handle, "EXIF:Make");
char make[256];
if (make_ptr) {
    strncpy(make, make_ptr, sizeof(make) - 1);
    make[sizeof(make) - 1] = '\0';
}
// make is safe to use
```

### Thread Safety

One handle per thread:

```c
// Thread function
void* worker_thread(void* arg) {
    const char* path = (const char*)arg;

    ExifToolHandle* handle = exiftool_create();
    exiftool_read_file(handle, path);
    // Process metadata...
    exiftool_destroy(handle);

    return NULL;
}
```

## Common Pitfalls

1. **Forgetting to destroy handle** - Always pair `create` with `destroy`
2. **Using returned strings after next call** - Copy immediately
3. **Ignoring return codes** - Always check for errors
4. **NULL input strings** - All strings must be null-terminated
5. **Sharing handle across threads** - Use one handle per thread

## Performance Considerations

- **Reuse handles** - Create once, use for multiple files
- **Batch processing** - Process multiple files with one handle
- **Memory mapping** - Use `exiftool_read_bytes()` with memory-mapped files for large files

## Additional Resources

- [API Reference](/reference/api-reference) - Rust library API
- [Library Guide](/guide/library-api) - Integration tutorial
- [GitHub Repository](https://github.com/swack-tools/oxidex) - Source code and examples
