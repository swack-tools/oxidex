# C FFI Usage Guide

This guide provides quick examples for using the ExifTool-RS C FFI bindings.

## Building the Library

Build the library with FFI support:

```bash
# Debug build
cargo build --lib

# Release build
cargo build --lib --release
```

This generates:
- **Static library**: `target/[debug|release]/libexiftool_rs.a`
- **Dynamic library**: `target/[debug|release]/libexiftool_rs.dylib` (macOS), `.so` (Linux), `.dll` (Windows)

## Files

- **Header file**: `include/exiftool_rs.h` - C function declarations
- **Test file**: `tests/ffi/c_integration_test.c` - Example usage
- **API documentation**: `docs/api/ffi_api.md` - Complete API reference

## Quick Example

```c
#include "exiftool_rs.h"
#include <stdio.h>

int main() {
    // Create handle
    ExifToolHandle* handle = exiftool_create();
    if (!handle) {
        fprintf(stderr, "Out of memory\n");
        return 1;
    }

    // Read metadata
    if (exiftool_read_file(handle, "photo.jpg") != EXIFTOOL_OK) {
        fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    // Get camera make
    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
    if (make) {
        printf("Camera: %s\n", make);
    }

    // Get ISO
    int64_t iso;
    if (exiftool_get_tag_integer(handle, "EXIF:ISO", &iso) == EXIFTOOL_OK) {
        printf("ISO: %lld\n", (long long)iso);
    }

    // Clean up
    exiftool_destroy(handle);
    return 0;
}
```

## Compilation

### macOS

```bash
# Using dynamic library
clang -o myapp myapp.c -lexiftool_rs -L./target/release -I./include
export DYLD_LIBRARY_PATH=./target/release:$DYLD_LIBRARY_PATH
./myapp

# Using static library
clang -o myapp myapp.c ./target/release/libexiftool_rs.a -I./include
./myapp
```

### Linux

```bash
# Using dynamic library
gcc -o myapp myapp.c -lexiftool_rs -L./target/release -I./include
export LD_LIBRARY_PATH=./target/release:$LD_LIBRARY_PATH
./myapp

# Using static library
gcc -o myapp myapp.c ./target/release/libexiftool_rs.a -lpthread -ldl -lm -I./include
./myapp
```

### Windows (MSVC)

```cmd
REM Using dynamic library
cl /MD myapp.c /I.\include exiftool_rs.lib /link /LIBPATH:.\target\release
myapp.exe

REM Using static library
cl /MT myapp.c .\target\release\libexiftool_rs.a /I.\include
myapp.exe
```

## Running Tests

The C integration test can be compiled and run manually:

```bash
# Compile test
gcc -o test_ffi tests/ffi/c_integration_test.c \
    -I./include \
    -L./target/debug \
    -lexiftool_rs

# Run test
./test_ffi
```

Expected output:
```
========================================
ExifTool-RS C FFI Integration Tests
========================================

Test 1: Handle Lifecycle
  [PASS] Handle creation succeeds
  [PASS] Handle destruction succeeds
  [PASS] Destroying NULL handle is safe

...

========================================
Test Summary
========================================
Passed: 35
Failed: 0
Total:  35

All tests PASSED! ✓
```

## Key Features

### Memory Safety
- All panics are caught at the FFI boundary
- NULL pointers are checked and return safe error codes
- Handles are opaque - C code cannot corrupt internal state

### Error Handling
- **Return codes**: All functions return integer status codes
- **Error messages**: Detailed messages available via `exiftool_get_last_error()`
- **Thread-local storage**: Each thread has independent error state

### String Lifetime
- Returned strings are valid until:
  - Next API call on the same handle
  - Handle destruction
  - Thread termination (for error messages)
- **Always copy strings if you need to keep them**

### Thread Safety
- **Handles are NOT thread-safe** - use one handle per thread or external synchronization
- **Error messages ARE thread-safe** - thread-local storage

## API Functions

### Handle Lifecycle
- `exiftool_create()` - Create new handle
- `exiftool_destroy()` - Destroy handle

### Metadata Reading
- `exiftool_read_file()` - Read file
- `exiftool_get_tag_count()` - Get tag count
- `exiftool_get_tag_name_at()` - Get tag name by index
- `exiftool_has_tag()` - Check if tag exists

### Tag Access
- `exiftool_get_tag_string()` - Get string value
- `exiftool_get_tag_integer()` - Get integer value
- `exiftool_get_tag_float()` - Get float value

### Metadata Writing
- `exiftool_set_tag_string()` - Set string value
- `exiftool_set_tag_integer()` - Set integer value
- `exiftool_set_tag_float()` - Set float value
- `exiftool_remove_tag()` - Remove tag
- `exiftool_write_file()` - Write to file

### Error Handling
- `exiftool_get_last_error()` - Get last error message

## Error Codes

```c
#define EXIFTOOL_OK                      0   // Success
#define EXIFTOOL_ERR_IO                  1   // I/O error
#define EXIFTOOL_ERR_PARSE               2   // Parse error
#define EXIFTOOL_ERR_TAG_NOT_FOUND       3   // Tag not found
#define EXIFTOOL_ERR_INVALID_TAG_VALUE   4   // Invalid tag value
#define EXIFTOOL_ERR_UNSUPPORTED_FORMAT  5   // Unsupported format
#define EXIFTOOL_ERR_NULL_POINTER        6   // NULL pointer
#define EXIFTOOL_ERR_INTERNAL            99  // Internal error
```

## Best Practices

1. **Always check return codes** - Don't ignore errors
2. **Copy strings immediately** - They become invalid after next API call
3. **One handle per thread** - Don't share handles across threads
4. **Always destroy handles** - Prevents memory leaks
5. **Check for NULL** - Functions can return NULL on error

## Language Bindings

The C FFI can be used to create bindings for other languages:

- **Python**: Use `ctypes` or `cffi`
- **Ruby**: Use `fiddle` or `ffi`
- **Node.js**: Use `node-ffi-napi` or `neon`
- **Go**: Use `cgo`
- **Java**: Use JNI
- **C#**: Use P/Invoke

## Complete Documentation

For complete API documentation, see:
- **API Reference**: `docs/api/ffi_api.md`
- **Header File**: `include/exiftool_rs.h`
