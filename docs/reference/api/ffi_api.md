# OxiDex C FFI API Reference

**Version:** 0.1.0
**Last Updated:** 2025-10-30

## Table of Contents

1. [Introduction](#introduction)
2. [Quick Start](#quick-start)
3. [Core Concepts](#core-concepts)
   - [Opaque Handle Pattern](#opaque-handle-pattern)
   - [Error Handling](#error-handling)
   - [Memory Ownership](#memory-ownership)
   - [Thread Safety](#thread-safety)
4. [API Reference](#api-reference)
   - [Handle Lifecycle](#handle-lifecycle)
   - [Metadata Reading](#metadata-reading)
   - [Metadata Writing](#metadata-writing)
   - [Tag Access](#tag-access)
   - [Error Handling Functions](#error-handling-functions)
5. [Type Definitions](#type-definitions)
   - [Error Codes](#error-codes)
   - [Tag Value Types](#tag-value-types)
6. [Code Examples](#code-examples)
   - [Example 1: Basic Usage](#example-1-basic-usage)
   - [Example 2: Error Handling](#example-2-error-handling)
   - [Example 3: Iterating All Tags](#example-3-iterating-all-tags)
   - [Example 4: Modifying Metadata](#example-4-modifying-metadata)
   - [Example 5: Memory Safety](#example-5-memory-safety)
7. [Best Practices](#best-practices)
8. [Platform Notes](#platform-notes)

---

## Introduction

OxiDex provides a C-compatible Foreign Function Interface (FFI) for reading and writing metadata in image and media files. This API allows C, C++, and other languages with C interoperability to leverage OxiDex's metadata extraction capabilities.

**Key Design Principles:**

- **Safety First**: No panics cross the FFI boundary. All Rust panics are caught and converted to error codes.
- **C Idioms**: API follows standard C conventions (return codes, null-terminated strings, opaque handles).
- **Explicit Errors**: All errors are returned as integer codes with detailed messages available via `exiftool_get_last_error()`.
- **Clear Ownership**: Memory management rules are explicit and documented.
- **Minimal Surface**: API exposes only essential operations, reducing complexity and maintenance burden.

**Compatibility:**

- **C Standard**: C99 or later
- **Platforms**: Linux, macOS, Windows
- **ABI Stability**: API uses simple C types only (no structs with padding concerns)

**What This API Is For:**

- Embedding metadata extraction in C/C++ applications
- Language bindings (Python, Ruby, etc. via C extensions)
- Legacy codebases that cannot use Rust directly
- Systems requiring dynamic library linking

**What This API Is NOT For:**

- High-performance batch processing (use native Rust API)
- Applications that can use Rust directly (prefer native API)

---

## Quick Start

Here's a minimal working example to get you started:

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

**Compile and Link:**

```bash
# Linux/macOS
gcc -o example example.c -loxidex -L/path/to/lib

# Windows (MSVC)
cl example.c oxidex.lib
```

---

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

**Example - String Lifetime Issue:**

```c
// WRONG - string pointer becomes invalid
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
exiftool_read_file(handle, "another.jpg");  // Invalidates 'make'!
printf("%s\n", make);  // UNDEFINED BEHAVIOR

// CORRECT - copy string immediately
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
char make_copy[256];
strncpy(make_copy, make, sizeof(make_copy) - 1);
make_copy[sizeof(make_copy) - 1] = '\0';
exiftool_read_file(handle, "another.jpg");  // Safe, we have a copy
printf("%s\n", make_copy);  // OK
```

### Thread Safety

**Handles Are NOT Thread-Safe**

A single `ExifToolHandle` **must not** be accessed concurrently from multiple threads. Doing so will result in:

- Data races
- Corrupted metadata
- Potential crashes

**Safe Patterns:**

1. **One Handle Per Thread**:
   ```c
   // Thread function
   void* process_file(void* arg) {
       const char* path = (const char*)arg;
       ExifToolHandle* handle = exiftool_create();  // Thread-local handle
       exiftool_read_file(handle, path);
       // ... use handle ...
       exiftool_destroy(handle);
       return NULL;
   }
   ```

2. **External Synchronization**:
   ```c
   pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
   ExifToolHandle* shared_handle = exiftool_create();

   // Thread A
   pthread_mutex_lock(&lock);
   exiftool_read_file(shared_handle, "photo1.jpg");
   pthread_mutex_unlock(&lock);

   // Thread B
   pthread_mutex_lock(&lock);
   exiftool_read_file(shared_handle, "photo2.jpg");
   pthread_mutex_unlock(&lock);
   ```

**Error Messages Are Thread-Safe**

The `exiftool_get_last_error()` function uses thread-local storage, so each thread has its own error message. This is safe:

```c
// Thread A
if (exiftool_read_file(handle_a, "photo1.jpg") != EXIFTOOL_OK) {
    // Gets thread A's error
    fprintf(stderr, "%s\n", exiftool_get_last_error());
}

// Thread B (concurrent with A)
if (exiftool_read_file(handle_b, "photo2.jpg") != EXIFTOOL_OK) {
    // Gets thread B's error (independent of A)
    fprintf(stderr, "%s\n", exiftool_get_last_error());
}
```

---

## API Reference

### Handle Lifecycle

#### `exiftool_create`

Creates a new ExifTool handle.

**Signature:**
```c
ExifToolHandle* exiftool_create(void);
```

**Returns:**
- Pointer to opaque handle on success
- `NULL` if allocation fails (out of memory)

**Description:**

Creates a new handle that encapsulates metadata state. The handle is initially empty (no metadata loaded). Use `exiftool_read_file()` to load metadata from a file.

**Errors:**

This function does not set the last error message. NULL return indicates allocation failure.

**Example:**
```c
ExifToolHandle* handle = exiftool_create();
if (!handle) {
    fprintf(stderr, "Out of memory\n");
    exit(1);
}
```

**Thread Safety:** Thread-safe. Each call returns an independent handle.

---

#### `exiftool_destroy`

Destroys a handle and frees all associated resources.

**Signature:**
```c
void exiftool_destroy(ExifToolHandle* handle);
```

**Parameters:**
- `handle`: Handle to destroy (can be `NULL`)

**Description:**

Frees all memory associated with the handle, including loaded metadata. After this call, the handle pointer is invalid and must not be used.

Passing `NULL` is safe and does nothing (similar to `free(NULL)`).

**Errors:**

This function never fails and does not set error messages.

**Example:**
```c
ExifToolHandle* handle = exiftool_create();
// ... use handle ...
exiftool_destroy(handle);
handle = NULL;  // Good practice: prevent use-after-free
```

**Thread Safety:** Not thread-safe. Do not call concurrently with other operations on the same handle.

**Critical Rule:**

> Every handle created with `exiftool_create()` must be destroyed exactly once with `exiftool_destroy()`.
> Failure to do so leaks memory.

---

### Metadata Reading

#### `exiftool_read_file`

Reads metadata from a file.

**Signature:**
```c
int exiftool_read_file(ExifToolHandle* handle, const char* filepath);
```

**Parameters:**
- `handle`: Handle to store metadata in (must not be `NULL`)
- `filepath`: Path to file (null-terminated UTF-8 string)

**Returns:**
- `EXIFTOOL_OK` (0) on success
- Error code on failure (see [Error Codes](#error-codes))

**Description:**

Opens the file at `filepath`, detects its format, and extracts all metadata tags into the handle. Any existing metadata in the handle is replaced.

The file path must be UTF-8 encoded. On Windows, non-ASCII paths should be converted using `WideCharToMultiByte`.

**Errors:**

| Error Code | Condition |
|------------|-----------|
| `EXIFTOOL_ERR_NULL_POINTER` | `handle` or `filepath` is `NULL` |
| `EXIFTOOL_ERR_IO` | File not found, permission denied, or read error |
| `EXIFTOOL_ERR_UNSUPPORTED_FORMAT` | File format not recognized or not supported |
| `EXIFTOOL_ERR_PARSE` | File is corrupted or malformed |

**Example:**
```c
int result = exiftool_read_file(handle, "photo.jpg");
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Failed to read file: %s\n", exiftool_get_last_error());
    return 1;
}
printf("Loaded %zu tags\n", exiftool_get_tag_count(handle));
```

**Thread Safety:** Not thread-safe. Do not call concurrently on the same handle.

---

#### `exiftool_get_tag_count`

Returns the number of tags in the metadata.

**Signature:**
```c
size_t exiftool_get_tag_count(const ExifToolHandle* handle);
```

**Parameters:**
- `handle`: Handle to query (can be `NULL`)

**Returns:**
- Number of tags (0 if handle is `NULL` or no metadata loaded)

**Description:**

Returns the count of metadata tags currently loaded in the handle. Useful for iteration and progress reporting.

**Errors:**

This function never fails. Returns 0 for NULL handle.

**Example:**
```c
size_t count = exiftool_get_tag_count(handle);
printf("Found %zu metadata tags\n", count);
```

**Thread Safety:** Thread-safe for read-only access (but handle must not be modified concurrently).

---

#### `exiftool_get_tag_name_at`

Retrieves tag name by index.

**Signature:**
```c
const char* exiftool_get_tag_name_at(const ExifToolHandle* handle, size_t index);
```

**Parameters:**
- `handle`: Handle to query (must not be `NULL`)
- `index`: Zero-based index (must be less than `exiftool_get_tag_count()`)

**Returns:**
- Pointer to null-terminated tag name string
- `NULL` if index is out of bounds or handle is `NULL`

**Description:**

Returns the tag name at the specified index. Tag names follow the format `<FormatFamily>:<TagName>` (e.g., `"EXIF:Make"`).

**Iteration Pattern:**
```c
size_t count = exiftool_get_tag_count(handle);
for (size_t i = 0; i < count; i++) {
    const char* tag_name = exiftool_get_tag_name_at(handle, i);
    if (tag_name) {
        printf("Tag %zu: %s\n", i, tag_name);
    }
}
```

**String Lifetime:**

Returned string is valid until:
- Next API call on same handle
- Handle destruction

**Errors:**

Returns `NULL` for out-of-bounds index. Does not set last error.

**Thread Safety:** Thread-safe for read-only access (but handle must not be modified concurrently).

---

#### `exiftool_has_tag`

Checks if a tag exists.

**Signature:**
```c
int exiftool_has_tag(const ExifToolHandle* handle, const char* tag_name);
```

**Parameters:**
- `handle`: Handle to query (can be `NULL`)
- `tag_name`: Tag name to check (e.g., `"EXIF:Make"`)

**Returns:**
- `1` if tag exists
- `0` if tag does not exist or handle is `NULL`

**Description:**

Checks whether the specified tag exists in the metadata without retrieving its value.

**Example:**
```c
if (exiftool_has_tag(handle, "EXIF:Make")) {
    printf("Make tag is present\n");
}
```

**Thread Safety:** Thread-safe for read-only access.

---

### Tag Access

#### `exiftool_get_tag_string`

Retrieves tag value as a string.

**Signature:**
```c
const char* exiftool_get_tag_string(const ExifToolHandle* handle, const char* tag_name);
```

**Parameters:**
- `handle`: Handle to query (must not be `NULL`)
- `tag_name`: Tag name (e.g., `"EXIF:Make"`)

**Returns:**
- Pointer to null-terminated UTF-8 string
- `NULL` if tag doesn't exist or is not a String type

**Description:**

Returns the string value of a tag. Only succeeds if the tag exists and is of type String.

**String Lifetime:**

Returned string is valid until:
- Next API call on same handle
- Handle destruction

Caller should copy the string if needed beyond this scope.

**Example:**
```c
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
if (make) {
    printf("Camera make: %s\n", make);
} else {
    printf("Make tag not found or wrong type\n");
}
```

**Type Checking:**

Returns `NULL` if the tag is an Integer, Float, or other non-String type.

**Thread Safety:** Thread-safe for read-only access.

---

#### `exiftool_get_tag_integer`

Retrieves tag value as a 64-bit integer.

**Signature:**
```c
int exiftool_get_tag_integer(const ExifToolHandle* handle, const char* tag_name, int64_t* out_value);
```

**Parameters:**
- `handle`: Handle to query (must not be `NULL`)
- `tag_name`: Tag name (e.g., `"EXIF:ISO"`)
- `out_value`: Pointer to output variable (must not be `NULL`)

**Returns:**
- `EXIFTOOL_OK` on success
- `EXIFTOOL_ERR_TAG_NOT_FOUND` if tag doesn't exist
- `EXIFTOOL_ERR_INVALID_TAG_VALUE` if tag is not an Integer type
- `EXIFTOOL_ERR_NULL_POINTER` if `handle` or `out_value` is `NULL`

**Description:**

Writes the integer value of a tag to `*out_value`. Only succeeds if the tag exists and is of type Integer.

**Example:**
```c
int64_t iso;
int result = exiftool_get_tag_integer(handle, "EXIF:ISO", &iso);
if (result == EXIFTOOL_OK) {
    printf("ISO: %lld\n", (long long)iso);
} else {
    fprintf(stderr, "Failed to get ISO: %s\n", exiftool_get_last_error());
}
```

**Thread Safety:** Thread-safe for read-only access.

---

#### `exiftool_get_tag_float`

Retrieves tag value as a double-precision float.

**Signature:**
```c
int exiftool_get_tag_float(const ExifToolHandle* handle, const char* tag_name, double* out_value);
```

**Parameters:**
- `handle`: Handle to query (must not be `NULL`)
- `tag_name`: Tag name (e.g., `"EXIF:FNumber"`)
- `out_value`: Pointer to output variable (must not be `NULL`)

**Returns:**
- `EXIFTOOL_OK` on success
- `EXIFTOOL_ERR_TAG_NOT_FOUND` if tag doesn't exist
- `EXIFTOOL_ERR_INVALID_TAG_VALUE` if tag is not a Float type
- `EXIFTOOL_ERR_NULL_POINTER` if `handle` or `out_value` is `NULL`

**Description:**

Writes the floating-point value of a tag to `*out_value`. Only succeeds if the tag exists and is of type Float.

**Example:**
```c
double aperture;
int result = exiftool_get_tag_float(handle, "EXIF:FNumber", &aperture);
if (result == EXIFTOOL_OK) {
    printf("Aperture: f/%.1f\n", aperture);
}
```

**Thread Safety:** Thread-safe for read-only access.

---

### Metadata Writing

#### `exiftool_set_tag_string`

Sets a tag value to a string.

**Signature:**
```c
int exiftool_set_tag_string(ExifToolHandle* handle, const char* tag_name, const char* value);
```

**Parameters:**
- `handle`: Handle to modify (must not be `NULL`)
- `tag_name`: Tag name (e.g., `"EXIF:Artist"`)
- `value`: String value to set (null-terminated UTF-8)

**Returns:**
- `EXIFTOOL_OK` on success
- `EXIFTOOL_ERR_NULL_POINTER` if any parameter is `NULL`
- `EXIFTOOL_ERR_INVALID_TAG_VALUE` if value is invalid for the tag

**Description:**

Sets the value of a tag to the specified string. Creates the tag if it doesn't exist, replaces the value if it does.

The library makes an internal copy of the string. Caller retains ownership of the input.

**Example:**
```c
int result = exiftool_set_tag_string(handle, "EXIF:Artist", "John Doe");
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Failed to set artist: %s\n", exiftool_get_last_error());
}
```

**Thread Safety:** Not thread-safe. Do not call concurrently on the same handle.

---

#### `exiftool_set_tag_integer`

Sets a tag value to an integer.

**Signature:**
```c
int exiftool_set_tag_integer(ExifToolHandle* handle, const char* tag_name, int64_t value);
```

**Parameters:**
- `handle`: Handle to modify (must not be `NULL`)
- `tag_name`: Tag name (e.g., `"EXIF:ISO"`)
- `value`: Integer value to set

**Returns:**
- `EXIFTOOL_OK` on success
- `EXIFTOOL_ERR_NULL_POINTER` if `handle` or `tag_name` is `NULL`
- `EXIFTOOL_ERR_INVALID_TAG_VALUE` if value is out of valid range for the tag

**Example:**
```c
exiftool_set_tag_integer(handle, "EXIF:ISO", 800);
```

**Thread Safety:** Not thread-safe.

---

#### `exiftool_set_tag_float`

Sets a tag value to a floating-point number.

**Signature:**
```c
int exiftool_set_tag_float(ExifToolHandle* handle, const char* tag_name, double value);
```

**Parameters:**
- `handle`: Handle to modify (must not be `NULL`)
- `tag_name`: Tag name (e.g., `"EXIF:FNumber"`)
- `value`: Float value to set

**Returns:**
- `EXIFTOOL_OK` on success
- `EXIFTOOL_ERR_NULL_POINTER` if `handle` or `tag_name` is `NULL`
- `EXIFTOOL_ERR_INVALID_TAG_VALUE` if value is invalid (NaN, infinity, out of range)

**Example:**
```c
exiftool_set_tag_float(handle, "EXIF:FNumber", 2.8);
```

**Thread Safety:** Not thread-safe.

---

#### `exiftool_remove_tag`

Removes a tag from the metadata.

**Signature:**
```c
int exiftool_remove_tag(ExifToolHandle* handle, const char* tag_name);
```

**Parameters:**
- `handle`: Handle to modify (must not be `NULL`)
- `tag_name`: Tag name to remove

**Returns:**
- `EXIFTOOL_OK` (always succeeds, even if tag didn't exist)
- `EXIFTOOL_ERR_NULL_POINTER` if `handle` or `tag_name` is `NULL`

**Description:**

Removes the specified tag from the metadata. If the tag doesn't exist, this is a no-op (still returns `EXIFTOOL_OK`).

**Example:**
```c
exiftool_remove_tag(handle, "EXIF:Thumbnail");
```

**Thread Safety:** Not thread-safe.

---

#### `exiftool_write_file`

Writes metadata to a file.

**Signature:**
```c
int exiftool_write_file(const ExifToolHandle* handle, const char* filepath);
```

**Parameters:**
- `handle`: Handle containing metadata to write (must not be `NULL`)
- `filepath`: Path to file to write (null-terminated UTF-8)

**Returns:**
- `EXIFTOOL_OK` on success
- Error code on failure

**Description:**

Writes the metadata in the handle to the specified file. The write is **atomic**: if any error occurs, the original file is left unchanged.

**Write Process:**

1. Validates all metadata values
2. Reads original file
3. Serializes metadata
4. Writes to temporary file
5. Renames temporary file to target (atomic operation)

**Errors:**

| Error Code | Condition |
|------------|-----------|
| `EXIFTOOL_ERR_NULL_POINTER` | `handle` or `filepath` is `NULL` |
| `EXIFTOOL_ERR_IO` | File not writable, disk full, permission denied |
| `EXIFTOOL_ERR_UNSUPPORTED_FORMAT` | File format doesn't support writing |
| `EXIFTOOL_ERR_INVALID_TAG_VALUE` | Metadata validation failed |

**Example:**
```c
int result = exiftool_write_file(handle, "output.jpg");
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Write failed: %s\n", exiftool_get_last_error());
}
```

**Thread Safety:** Thread-safe for read-only access to handle.

---

### Error Handling Functions

#### `exiftool_get_last_error`

Retrieves the last error message.

**Signature:**
```c
const char* exiftool_get_last_error(void);
```

**Returns:**
- Pointer to null-terminated error message string
- Never returns `NULL` (returns `"No error"` if no error occurred)

**Description:**

Returns a human-readable description of the last error that occurred on the current thread. Each thread has independent error storage (thread-local).

**String Lifetime:**

The returned string is valid until:
- Next API call that sets an error on the same thread
- Thread termination

**Example:**
```c
int result = exiftool_read_file(handle, "missing.jpg");
if (result != EXIFTOOL_OK) {
    fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
}
```

**Thread Safety:** Thread-safe. Each thread has its own error message.

**Notes:**

- Always call immediately after an error return code
- Error message may be overwritten by next failing API call
- Copy the string if you need to preserve it across multiple API calls

---

## Type Definitions

### Error Codes

All error codes are non-zero integers. Success is always `EXIFTOOL_OK` (0).

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

**Error Code Details:**

| Code | Name | Description | Common Causes |
|------|------|-------------|---------------|
| `0` | `EXIFTOOL_OK` | Success | N/A |
| `1` | `EXIFTOOL_ERR_IO` | I/O error | File not found, permission denied, read/write error, disk full |
| `2` | `EXIFTOOL_ERR_PARSE` | Parse error | Corrupted file, truncated data, invalid format structure |
| `3` | `EXIFTOOL_ERR_TAG_NOT_FOUND` | Tag not found | Requested tag doesn't exist in metadata |
| `4` | `EXIFTOOL_ERR_INVALID_TAG_VALUE` | Invalid tag value | Type mismatch, value out of range, invalid format |
| `5` | `EXIFTOOL_ERR_UNSUPPORTED_FORMAT` | Unsupported format | File format not recognized or not implemented |
| `6` | `EXIFTOOL_ERR_NULL_POINTER` | NULL pointer | Required parameter is `NULL` |
| `99` | `EXIFTOOL_ERR_INTERNAL` | Internal error | Unexpected Rust panic (should never happen) |

**Error Handling Pattern:**

```c
int result = exiftool_some_operation(handle, args);
switch (result) {
    case EXIFTOOL_OK:
        // Success
        break;
    case EXIFTOOL_ERR_IO:
        // Handle I/O error
        break;
    case EXIFTOOL_ERR_PARSE:
        // Handle parse error
        break;
    default:
        // Handle other errors
        fprintf(stderr, "Error %d: %s\n", result, exiftool_get_last_error());
        break;
}
```

### Tag Value Types

Tag values have different types. Use type-specific accessors to retrieve values:

| Type | Accessor Function | Example Tag |
|------|------------------|-------------|
| String | `exiftool_get_tag_string()` | `EXIF:Make`, `EXIF:Model` |
| Integer | `exiftool_get_tag_integer()` | `EXIF:ISO`, `EXIF:Orientation` |
| Float | `exiftool_get_tag_float()` | `EXIF:FNumber`, `GPS:Latitude` |

**Type Checking:**

If you call the wrong accessor for a tag's type, you'll get an error:

```c
// ISO is an integer
const char* iso_string = exiftool_get_tag_string(handle, "EXIF:ISO");
// Returns NULL (wrong type)

int64_t iso_int;
int result = exiftool_get_tag_integer(handle, "EXIF:ISO", &iso_int);
// Returns EXIFTOOL_OK (correct type)
```

---

## Code Examples

### Example 1: Basic Usage

Minimal example demonstrating the complete workflow.

```c
#include "oxidex.h"
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char* argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <image_file>\n", argv[0]);
        return 1;
    }

    // Create handle
    ExifToolHandle* handle = exiftool_create();
    if (!handle) {
        fprintf(stderr, "Failed to create handle\n");
        return 1;
    }

    // Read file
    int result = exiftool_read_file(handle, argv[1]);
    if (result != EXIFTOOL_OK) {
        fprintf(stderr, "Error reading file: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    // Get camera info
    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
    const char* model = exiftool_get_tag_string(handle, "EXIF:Model");

    printf("Camera: %s %s\n",
           make ? make : "Unknown",
           model ? model : "Unknown");

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

**Output:**
```
Camera: Canon Canon EOS 5D Mark IV
ISO: 400
```

---

### Example 2: Error Handling

Comprehensive error handling with specific error code checks.

```c
#include "oxidex.h"
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>

int main(int argc, char* argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <image_file>\n", argv[0]);
        return 1;
    }

    ExifToolHandle* handle = exiftool_create();
    if (!handle) {
        fprintf(stderr, "Out of memory\n");
        return 1;
    }

    int result = exiftool_read_file(handle, argv[1]);

    // Handle different error types
    switch (result) {
        case EXIFTOOL_OK:
            printf("File loaded successfully\n");
            break;

        case EXIFTOOL_ERR_IO:
            fprintf(stderr, "I/O Error: %s\n", exiftool_get_last_error());
            fprintf(stderr, "Check that the file exists and is readable\n");
            exiftool_destroy(handle);
            return 1;

        case EXIFTOOL_ERR_UNSUPPORTED_FORMAT:
            fprintf(stderr, "Unsupported Format: %s\n", exiftool_get_last_error());
            fprintf(stderr, "This file type is not supported\n");
            exiftool_destroy(handle);
            return 1;

        case EXIFTOOL_ERR_PARSE:
            fprintf(stderr, "Parse Error: %s\n", exiftool_get_last_error());
            fprintf(stderr, "File may be corrupted\n");
            exiftool_destroy(handle);
            return 1;

        default:
            fprintf(stderr, "Unknown Error (code %d): %s\n",
                    result, exiftool_get_last_error());
            exiftool_destroy(handle);
            return 1;
    }

    // Try to get a required tag
    int64_t width;
    result = exiftool_get_tag_integer(handle, "EXIF:PixelWidth", &width);
    if (result == EXIFTOOL_ERR_TAG_NOT_FOUND) {
        fprintf(stderr, "Warning: Image width not found in metadata\n");
    } else if (result == EXIFTOOL_OK) {
        printf("Image width: %lld pixels\n", (long long)width);
    } else {
        fprintf(stderr, "Error getting width: %s\n", exiftool_get_last_error());
    }

    exiftool_destroy(handle);
    return 0;
}
```

---

### Example 3: Iterating All Tags

Enumerate all metadata tags in a file.

```c
#include "oxidex.h"
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char* argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <image_file>\n", argv[0]);
        return 1;
    }

    ExifToolHandle* handle = exiftool_create();
    if (!handle) {
        fprintf(stderr, "Failed to create handle\n");
        return 1;
    }

    if (exiftool_read_file(handle, argv[1]) != EXIFTOOL_OK) {
        fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    // Get total tag count
    size_t count = exiftool_get_tag_count(handle);
    printf("Found %zu metadata tags:\n\n", count);

    // Iterate through all tags
    for (size_t i = 0; i < count; i++) {
        const char* tag_name = exiftool_get_tag_name_at(handle, i);
        if (!tag_name) {
            continue;  // Should never happen if i < count
        }

        // Try to get value as string
        const char* str_value = exiftool_get_tag_string(handle, tag_name);
        if (str_value) {
            printf("  %s: \"%s\"\n", tag_name, str_value);
            continue;
        }

        // Try to get value as integer
        int64_t int_value;
        if (exiftool_get_tag_integer(handle, tag_name, &int_value) == EXIFTOOL_OK) {
            printf("  %s: %lld\n", tag_name, (long long)int_value);
            continue;
        }

        // Try to get value as float
        double float_value;
        if (exiftool_get_tag_float(handle, tag_name, &float_value) == EXIFTOOL_OK) {
            printf("  %s: %.6f\n", tag_name, float_value);
            continue;
        }

        // Unknown type
        printf("  %s: <unsupported type>\n", tag_name);
    }

    exiftool_destroy(handle);
    return 0;
}
```

**Output:**
```
Found 32 metadata tags:

  EXIF:Make: "Canon"
  EXIF:Model: "Canon EOS 5D Mark IV"
  EXIF:Orientation: 1
  EXIF:XResolution: 72.000000
  EXIF:YResolution: 72.000000
  EXIF:Software: "Adobe Photoshop Camera Raw 15.0"
  EXIF:DateTime: "2025:10:29 14:30:00"
  EXIF:ISO: 400
  EXIF:FNumber: 2.800000
  ...
```

---

### Example 4: Modifying Metadata

Read metadata, modify tags, and write back to file.

```c
#include "oxidex.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char* argv[]) {
    if (argc != 3) {
        fprintf(stderr, "Usage: %s <input.jpg> <output.jpg>\n", argv[0]);
        return 1;
    }

    const char* input_path = argv[1];
    const char* output_path = argv[2];

    // Create handle and read metadata
    ExifToolHandle* handle = exiftool_create();
    if (!handle) {
        fprintf(stderr, "Out of memory\n");
        return 1;
    }

    if (exiftool_read_file(handle, input_path) != EXIFTOOL_OK) {
        fprintf(stderr, "Read error: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    printf("Loaded metadata from %s\n", input_path);
    printf("Original tag count: %zu\n", exiftool_get_tag_count(handle));

    // Modify tags
    exiftool_set_tag_string(handle, "EXIF:Artist", "Jane Smith");
    exiftool_set_tag_string(handle, "EXIF:Copyright", "2025 Jane Smith");
    exiftool_set_tag_integer(handle, "EXIF:Rating", 5);

    // Remove thumbnail to save space
    exiftool_remove_tag(handle, "EXIF:ThumbnailImage");

    printf("Modified tag count: %zu\n", exiftool_get_tag_count(handle));

    // Write to output file
    if (exiftool_write_file(handle, output_path) != EXIFTOOL_OK) {
        fprintf(stderr, "Write error: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    printf("Successfully wrote metadata to %s\n", output_path);

    exiftool_destroy(handle);
    return 0;
}
```

**Output:**
```
Loaded metadata from photo.jpg
Original tag count: 32
Modified tag count: 34
Successfully wrote metadata to photo_modified.jpg
```

---

### Example 5: Memory Safety

Demonstrates safe string handling and avoiding common pitfalls.

```c
#include "oxidex.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// WRONG: Stores pointer to library-owned string
void bad_example(ExifToolHandle* handle) {
    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");

    // This invalidates the 'make' pointer!
    exiftool_read_file(handle, "another_file.jpg");

    // UNDEFINED BEHAVIOR - 'make' pointer is now invalid
    printf("Make: %s\n", make);
}

// CORRECT: Copies string immediately
void good_example(ExifToolHandle* handle) {
    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
    if (!make) {
        printf("Make tag not found\n");
        return;
    }

    // Copy string immediately
    char make_copy[256];
    strncpy(make_copy, make, sizeof(make_copy) - 1);
    make_copy[sizeof(make_copy) - 1] = '\0';  // Ensure null termination

    // Now safe to do other operations
    exiftool_read_file(handle, "another_file.jpg");

    // Safe - we have our own copy
    printf("Make: %s\n", make_copy);
}

// Helper function: safe string copy
char* safe_copy_string(const char* src) {
    if (!src) {
        return NULL;
    }
    size_t len = strlen(src);
    char* copy = malloc(len + 1);
    if (copy) {
        strcpy(copy, src);
    }
    return copy;
}

int main() {
    ExifToolHandle* handle = exiftool_create();
    if (!handle) {
        return 1;
    }

    if (exiftool_read_file(handle, "photo1.jpg") != EXIFTOOL_OK) {
        fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
        exiftool_destroy(handle);
        return 1;
    }

    // Get multiple strings - need to copy each one
    const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
    char* make_saved = safe_copy_string(make);

    const char* model = exiftool_get_tag_string(handle, "EXIF:Model");
    char* model_saved = safe_copy_string(model);

    // Now we can safely read another file
    if (exiftool_read_file(handle, "photo2.jpg") != EXIFTOOL_OK) {
        fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
    } else {
        const char* make2 = exiftool_get_tag_string(handle, "EXIF:Make");
        const char* model2 = exiftool_get_tag_string(handle, "EXIF:Model");

        // Compare cameras
        printf("Photo 1: %s %s\n",
               make_saved ? make_saved : "Unknown",
               model_saved ? model_saved : "Unknown");
        printf("Photo 2: %s %s\n",
               make2 ? make2 : "Unknown",
               model2 ? model2 : "Unknown");
    }

    // Clean up
    free(make_saved);
    free(model_saved);
    exiftool_destroy(handle);

    return 0;
}
```

---

## Best Practices

### 1. Always Check Return Codes

**Never ignore return codes**. Even operations that seem infallible can fail.

```c
// WRONG
exiftool_read_file(handle, "photo.jpg");
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");

// CORRECT
if (exiftool_read_file(handle, "photo.jpg") != EXIFTOOL_OK) {
    fprintf(stderr, "Error: %s\n", exiftool_get_last_error());
    return 1;
}
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
if (make) {
    // Use make
}
```

### 2. Copy Strings Immediately

Library-owned strings are **temporary**. Copy them if you need to keep them.

```c
// Save string for later use
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
if (make) {
    char make_copy[256];
    snprintf(make_copy, sizeof(make_copy), "%s", make);
    // Now make_copy is safe to use later
}
```

### 3. Use One Handle Per Thread

Handles are **not thread-safe**. Don't share them across threads without synchronization.

```c
// WRONG - undefined behavior
ExifToolHandle* handle = exiftool_create();
#pragma omp parallel
{
    exiftool_read_file(handle, "photo.jpg");  // DATA RACE!
}

// CORRECT - one handle per thread
#pragma omp parallel
{
    ExifToolHandle* thread_handle = exiftool_create();
    exiftool_read_file(thread_handle, "photo.jpg");
    exiftool_destroy(thread_handle);
}
```

### 4. Always Destroy Handles

Every `exiftool_create()` must have a matching `exiftool_destroy()`.

```c
ExifToolHandle* handle = exiftool_create();
if (!handle) return 1;

// Do work...

exiftool_destroy(handle);  // Don't forget this!
handle = NULL;  // Good practice
```

**Use RAII in C++:**
```cpp
class ExifToolRAII {
    ExifToolHandle* handle_;
public:
    ExifToolRAII() : handle_(exiftool_create()) {
        if (!handle_) throw std::bad_alloc();
    }
    ~ExifToolRAII() { exiftool_destroy(handle_); }
    ExifToolHandle* get() const { return handle_; }
};
```

### 5. Check for NULL Returns

Functions that return pointers can return `NULL` on error.

```c
const char* make = exiftool_get_tag_string(handle, "EXIF:Make");
if (make) {
    printf("Make: %s\n", make);
} else {
    printf("Make tag not found\n");
}
```

### 6. Validate Input Paths

File paths must be **valid UTF-8**. On Windows, convert wide strings properly.

```c
// Windows: Convert wide string to UTF-8
#ifdef _WIN32
char* wide_to_utf8(const wchar_t* wstr) {
    int len = WideCharToMultiByte(CP_UTF8, 0, wstr, -1, NULL, 0, NULL, NULL);
    char* utf8 = malloc(len);
    WideCharToMultiByte(CP_UTF8, 0, wstr, -1, utf8, len, NULL, NULL);
    return utf8;
}
#endif
```

### 7. Handle Unsupported Tags Gracefully

Not all files have all tags. Code defensively.

```c
int64_t iso;
if (exiftool_get_tag_integer(handle, "EXIF:ISO", &iso) == EXIFTOOL_OK) {
    printf("ISO: %lld\n", (long long)iso);
} else {
    printf("ISO not available\n");
}
```

### 8. Use Size Types Correctly

`exiftool_get_tag_count()` returns `size_t`. Use correct format specifiers.

```c
size_t count = exiftool_get_tag_count(handle);
printf("Tag count: %zu\n", count);  // %zu for size_t

// Don't use %d or %u
```

---

## Platform Notes

### Linux

**Compilation:**
```bash
gcc -o myapp myapp.c -loxidex -L/usr/local/lib
export LD_LIBRARY_PATH=/usr/local/lib:$LD_LIBRARY_PATH
./myapp
```

**Static Linking:**
```bash
gcc -o myapp myapp.c liboxidex.a -lpthread -ldl -lm
```

### macOS

**Compilation:**
```bash
clang -o myapp myapp.c -loxidex -L/usr/local/lib
export DYLD_LIBRARY_PATH=/usr/local/lib:$DYLD_LIBRARY_PATH
./myapp
```

**Framework Embedding:**
```bash
install_name_tool -change liboxidex.dylib @executable_path/liboxidex.dylib myapp
```

### Windows

**MSVC Compilation:**
```cmd
cl /MD myapp.c oxidex.lib
myapp.exe
```

**MinGW Compilation:**
```cmd
gcc -o myapp.exe myapp.c -loxidex -L. -Wl,-Bdynamic
```

**DLL Search Path:**

Ensure `oxidex.dll` is in one of:
- Same directory as executable
- Current directory
- System PATH

**Unicode Paths:**

Windows paths may contain non-ASCII characters. Convert using:
```c
#include <windows.h>

char* wide_path_to_utf8(const wchar_t* wide_path) {
    int size = WideCharToMultiByte(CP_UTF8, 0, wide_path, -1, NULL, 0, NULL, NULL);
    char* utf8_path = malloc(size);
    WideCharToMultiByte(CP_UTF8, 0, wide_path, -1, utf8_path, size, NULL, NULL);
    return utf8_path;
}
```

---

## Additional Resources

- **Rust Library API**: [Library API](/reference/api/library_api)
- **CLI Documentation**: [CLI Usage Guide](/guide/cli-usage)
- **GitHub Repository**: [https://github.com/codemaestro64/oxidex](https://github.com/codemaestro64/oxidex)
- **Issue Tracker**: [https://github.com/codemaestro64/oxidex/issues](https://github.com/codemaestro64/oxidex/issues)

---

**Document Version:** 1.0
**Last Updated:** 2025-10-30
**Minimum C Standard:** C99
**Supported Platforms:** Linux, macOS, Windows (x86_64, ARM64)
