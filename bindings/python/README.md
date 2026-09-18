# OxiDex Python Bindings

Python bindings for OxiDex using ctypes to interface with the C FFI.

> **Note**: This is a minimal reference implementation to demonstrate the C FFI works. It is not a production-quality binding and lacks many features like metadata writing, comprehensive error handling, and advanced functionality.

## Features

- Read EXIF metadata from image files
- Extract tags by name (string, integer, float types)
- Iterate through all available tags
- Pythonic API with context manager support
- Cross-platform (Linux, macOS, Windows)

## Prerequisites

- Python 3.7 or higher
- OxiDex library compiled as a shared library

## Building the Library

Before using the Python bindings, you must build the OxiDex shared library:

```bash
# From the repository root
cd /path/to/oxidex
cargo build --lib --release
```

This will generate the shared library in `target/release/`:
- Linux: `liboxidex.so`
- macOS: `liboxidex.dylib`
- Windows: `oxidex.dll`

## Installation

No pip installation is needed. The bindings are a single Python module that loads the shared library dynamically.

Simply ensure the shared library is findable by one of these methods:

1. **Place library in standard location** (recommended for testing):
   ```bash
   # The bindings automatically search ../target/release/ from the script location
   # So if you run examples from bindings/python/, it will find the library
   ```

2. **Set library path environment variable**:
   ```bash
   # Linux
   export LD_LIBRARY_PATH=/path/to/oxidex/target/release:$LD_LIBRARY_PATH

   # macOS
   export DYLD_LIBRARY_PATH=/path/to/oxidex/target/release:$DYLD_LIBRARY_PATH

   # Windows
   set PATH=C:\path\to\oxidex\target\release;%PATH%
   ```

3. **Copy library to system location**:
   ```bash
   # Linux
   sudo cp target/release/liboxidex.so /usr/local/lib/

   # macOS
   sudo cp target/release/liboxidex.dylib /usr/local/lib/
   ```

## Usage

### Basic Example

```python
from oxidex import Oxidex, OxidexError

try:
    # Use context manager for automatic cleanup
    with Oxidex() as ox:
        # Read metadata from file
        ox.read_file("photo.jpg")

        # Get specific tags
        make = ox.get_tag("EXIF:Make")
        model = ox.get_tag("EXIF:Model")

        print(f"Camera: {make} {model}")

except OxidexError as e:
    print(f"Error: {e}")
```

### Getting All Tags

```python
from oxidex import Oxidex

with Oxidex() as ox:
    ox.read_file("photo.jpg")

    # Get all tags as a dictionary
    all_tags = ox.get_all_tags()

    for tag_name, tag_value in all_tags.items():
        print(f"{tag_name}: {tag_value}")
```

### Iterating Through Tags

```python
from oxidex import Oxidex

with Oxidex() as ox:
    ox.read_file("photo.jpg")

    # Get tag count
    count = ox.get_tag_count()
    print(f"Found {count} tags")

    # Iterate by index
    for i in range(count):
        tag_name = ox.get_tag_name_at(i)
        if tag_name:
            tag_value = ox.get_tag(tag_name)
            print(f"{tag_name}: {tag_value}")
```

### Error Handling

```python
from oxidex import Oxidex, OxidexError

with Oxidex() as ox:
    try:
        ox.read_file("nonexistent.jpg")
    except OxidexError as e:
        print(f"Failed to read file: {e}")
```

### Manual Resource Management

```python
from oxidex import Oxidex

# Create handle
ox = Oxidex()

try:
    ox.read_file("photo.jpg")
    make = ox.get_tag("EXIF:Make")
    print(make)
finally:
    # Manually clean up (not needed with context manager)
    del ox
```

## Running the Example

The repository includes a complete example script:

```bash
cd bindings/python
python3 example.py
```

This will read metadata from the sample JPEG in `tests/fixtures/jpeg/sample_with_exif.jpg` and demonstrate various features of the bindings.

## API Reference

### Oxidex Class

#### `__init__()`
Create a new Oxidex handle.

**Raises**: `OxidexError` if handle creation fails (out of memory).

#### `read_file(filepath: str) -> None`
Read metadata from a file.

**Args**:
- `filepath`: Path to the image file

**Raises**: `OxidexError` if reading fails (file not found, parse error, unsupported format, etc.).

#### `get_tag(tag_name: str) -> Optional[str]`
Get tag value as a string.

**Args**:
- `tag_name`: Name of the tag (e.g., "EXIF:Make")

**Returns**: Tag value as string, or `None` if tag doesn't exist or is not a string type.

#### `get_tag_integer(tag_name: str) -> Optional[int]`
Get tag value as an integer.

**Returns**: Tag value as integer, or `None` if tag doesn't exist or is not an integer type.

#### `get_tag_float(tag_name: str) -> Optional[float]`
Get tag value as a float.

**Returns**: Tag value as float, or `None` if tag doesn't exist or is not a float type.

#### `has_tag(tag_name: str) -> bool`
Check if a tag exists in the metadata.

**Returns**: `True` if tag exists, `False` otherwise.

#### `get_tag_count() -> int`
Get the number of tags in loaded metadata.

**Returns**: Number of tags (0 if no metadata loaded).

#### `get_tag_name_at(index: int) -> Optional[str]`
Get tag name by index.

**Args**:
- `index`: Zero-based index (must be < tag count)

**Returns**: Tag name or `None` if index is out of bounds.

#### `get_all_tags() -> dict[str, Optional[str]]`
Get all tags as a dictionary.

**Returns**: Dictionary mapping tag names to their string values.

### Context Manager Support

The `Oxidex` class supports Python's context manager protocol:

```python
with Oxidex() as ox:
    # Use ox here
    pass
# Handle is automatically destroyed
```

## Limitations

This is a minimal reference implementation with the following limitations:

1. **Read-only**: No support for writing metadata (functions like `exiftool_set_tag_string`, `exiftool_write_file` are not wrapped)
2. **Basic error handling**: Error messages are provided, but error codes are not exposed
3. **No async support**: Synchronous API only
4. **No type hints for all cases**: Some return types use `Optional` where more specific types could be used
5. **Limited tag iteration**: No high-level API for filtering or searching tags
6. **No advanced features**: Missing features like tag removal, binary data access, etc.

For a production-quality Python binding, consider:
- Using `cffi` instead of `ctypes` for better performance
- Adding comprehensive type hints
- Implementing full API coverage (write operations, tag management)
- Adding async/await support
- Providing high-level utilities (tag filtering, format conversion)
- Including proper packaging (setup.py, wheel distribution)

## Troubleshooting

### Library Not Found Error

If you see an error like:
```
OSError: Could not find liboxidex.so
```

**Solution**: Ensure the library is built and in a searchable location:

1. Build the library:
   ```bash
   cargo build --lib --release
   ```

2. Set the library path:
   ```bash
   # Linux
   export LD_LIBRARY_PATH=$(pwd)/target/release:$LD_LIBRARY_PATH

   # macOS
   export DYLD_LIBRARY_PATH=$(pwd)/target/release:$DYLD_LIBRARY_PATH
   ```

3. Or run from the `bindings/python/` directory (it searches `../../target/release/` automatically).

### UTF-8 Encoding Errors

The bindings assume file paths and tag names are valid UTF-8. If you encounter encoding errors:

- Ensure file paths use UTF-8 encoding
- Use `errors='replace'` in your decode calls if needed

### Memory Leaks

The bindings use proper resource management:

- **With context manager**: Resources are automatically freed when exiting the `with` block
- **Without context manager**: Resources are freed when the object is garbage collected

**Best practice**: Always use the context manager (`with Oxidex() as ox:`) to ensure deterministic cleanup.

## Thread Safety

The C FFI follows these thread safety rules:

- **Handle creation** (`exiftool_create`): Thread-safe, each call returns an independent handle
- **Handle operations**: Not thread-safe - do not use the same handle from multiple threads
- **Error messages**: Thread-safe - each thread has its own error message storage

**Recommendation**: Create one `Oxidex` instance per thread.

## License

This Python binding follows the same license as the OxiDex project.

## See Also

- [OxiDex C FFI Documentation](../../docs/reference/ffi-api.md)
- [C Header File](../../include/oxidex.h)
- [Rust FFI Implementation](../../src/ffi/mod.rs)
