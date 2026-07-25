# API Reference

Complete reference for OxiDex's Rust library API.

## Overview

OxiDex provides both a high-level ergonomic API for common operations and a low-level API for fine-grained control over metadata manipulation.

**Key Features:**
- **Zero-cost abstractions** - Efficient parsing with minimal overhead
- **Type-safe** - Strongly-typed metadata values with compile-time checks
- **Synchronous design** - Simple, predictable execution model
- **Error handling** - Comprehensive error types with context
- **Format support** - JPEG, TIFF, XMP, PNG, MP4, and 140+ format families

## Quick Start

```rust
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    // Read metadata
    let metadata = Metadata::from_path("photo.jpg")?;

    // Access tags
    if let Some(make) = metadata.get_string("EXIF:Make") {
        println!("Camera: {}", make);
    }

    // Write metadata
    metadata
        .set_tag("EXIF:Artist", "Jane Doe")?
        .write_to("output.jpg")?;

    Ok(())
}
```

## Core Concepts

### Tag Naming Convention

All metadata tags follow the format: `<FormatFamily>:<TagName>`

**Examples:**
- `EXIF:Make` - Camera manufacturer
- `EXIF:Model` - Camera model
- `GPS:Latitude` - GPS latitude coordinate
- `XMP-dc:Creator` - Document creator
- `IPTC:Keywords` - Image keywords

**Supported Format Families:**

| Family | Description | Example Tags |
|--------|-------------|--------------|
| `EXIF` | Exchangeable Image File Format | `EXIF:Make`, `EXIF:Model`, `EXIF:ISO` |
| `XMP` | Extensible Metadata Platform | `XMP-dc:Creator`, `XMP-dc:Rights` |
| `IPTC` | Press/journalism metadata | `IPTC:Keywords`, `IPTC:Caption` |
| `GPS` | GPS location data | `GPS:Latitude`, `GPS:Longitude` |
| `MakerNotes` | Camera-specific data | `MakerNotes:SerialNumber` |
| `PNG` | PNG format metadata | `PNG:Title`, `PNG:Author` |
| `QuickTime` | Video metadata | `QuickTime:Duration` |

**Note:** Tag names are case-sensitive.

### Type Safety

Metadata values are represented by the `TagValue` enum:

```rust
pub enum TagValue {
    String(String),
    Integer(i64),
    Float(f64),
    Rational { numerator: i32, denominator: i32 },
    Binary(Vec<u8>),
    DateTime(chrono::DateTime<Utc>),
    Struct(Box<HashMap<String, TagValue>>),
}
```

Type-safe accessors prevent type confusion:

```rust
let iso = metadata.get_integer("EXIF:ISO")?;  // Option<i64>
let make = metadata.get_string("EXIF:Make")?;  // Option<&str>
```

## High-Level API

### Reading Metadata

#### `Metadata::from_path(path: impl AsRef<Path>) -> Result<Metadata>`

Opens a file and extracts all metadata tags.

```rust
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    let metadata = Metadata::from_path("photo.jpg")?;

    if let Some(make) = metadata.get_string("EXIF:Make") {
        println!("Camera: {}", make);
    }

    Ok(())
}
```

**Errors:**
- `IoError` - File not found, permission denied
- `UnsupportedFormat` - File format not recognized
- `ParseError` - File is corrupted or malformed

#### `Metadata::from_bytes(data: &[u8], format_hint: Option<FileFormat>) -> Result<Metadata>`

Parses metadata from a byte buffer.

```rust
let file_data = std::fs::read("image.jpg")?;
let metadata = Metadata::from_bytes(&file_data, Some(FileFormat::JPEG))?;
```

**Use cases:**
- Processing files from memory (HTTP uploads)
- Working with embedded resources
- Testing with synthetic data

### Writing Metadata

OxiDex uses a **builder pattern** for metadata modifications:

```rust
Metadata::from_path("input.jpg")?
    .set_tag("EXIF:Artist", "John Doe")?
    .set_tag("EXIF:Copyright", "2025 John Doe")?
    .write_to("output.jpg")?;
```

#### Key Methods

##### `set_tag(tag_name: &str, value: impl Into<TagValue>) -> Result<Self>`

Sets a single tag value.

```rust
metadata.set_tag("EXIF:Make", "Canon")?;
metadata.set_tag("EXIF:ISO", 400)?;  // Accepts integers
metadata.set_tag("EXIF:FNumber", 2.8)?;  // Accepts floats
```

##### `remove_tag(tag_name: &str) -> Result<Self>`

Removes a tag from the metadata.

```rust
metadata.remove_tag("EXIF:Thumbnail")?;
```

##### `write_to(path: impl AsRef<Path>) -> Result<()>`

Writes the modified metadata to a new file.

```rust
metadata.write_to("output.jpg")?;
```

##### `write_in_place() -> Result<()>`

Writes the modified metadata back to the original file.

```rust
Metadata::from_path("photo.jpg")?
    .set_tag("EXIF:Artist", "Jane Smith")?
    .write_in_place()?;
```

**Warning:** In-place writes modify the original file.

### Advanced Operations

#### Copy Metadata Between Files

```rust
// Copy all tags
Metadata::from_path("source.jpg")?
    .copy_tags_to("destination.jpg")?
    .execute()?;

// Copy specific tags only
Metadata::from_path("source.jpg")?
    .copy_tags_to("destination.jpg")?
    .with_tags(&["EXIF:DateTime", "EXIF:Make", "EXIF:Model"])?
    .execute()?;

// Copy all except specific tags
Metadata::from_path("source.jpg")?
    .copy_tags_to("destination.jpg")?
    .exclude_tags(&["EXIF:Thumbnail", "MakerNotes:*"])?
    .execute()?;
```

## Low-Level API

### MetadataMap

In-memory representation of file metadata. Stores key-value pairs where keys are tag names and values are `TagValue` enums.

#### Construction

```rust
use oxidex::core::metadata_map::MetadataMap;

let mut metadata = MetadataMap::new();
let metadata = MetadataMap::with_capacity(50);  // Pre-allocate
```

#### Insertion and Modification

```rust
// Insert or replace
metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

// Get mutable reference
if let Some(tag) = metadata.get_mut("EXIF:ISO") {
    *tag = TagValue::new_integer(800);
}

// Remove tag
let removed = metadata.remove("EXIF:Thumbnail");

// Clear all
metadata.clear();
```

#### Retrieval

```rust
// Generic accessor
if let Some(tag_value) = metadata.get("EXIF:Make") {
    println!("Value: {:?}", tag_value);
}

// Type-safe accessors
let make = metadata.get_string("EXIF:Make");  // Option<&str>
let iso = metadata.get_integer("EXIF:ISO");   // Option<i64>
let aperture = metadata.get_float("EXIF:FNumber");  // Option<f64>
```

#### Querying

```rust
// Check existence
if metadata.contains_key("EXIF:Make") {
    println!("Make tag is present");
}

// Count tags
println!("Found {} tags", metadata.len());

// Check if empty
if metadata.is_empty() {
    println!("No metadata found");
}
```

#### Iteration

```rust
// Iterate over name-value pairs
for (name, value) in metadata.iter() {
    println!("{}: {:?}", name, value);
}

// Iterate over tag names
for tag_name in metadata.keys() {
    println!("Tag: {}", tag_name);
}

// Iterate over values
for value in metadata.values() {
    if value.is_string() {
        println!("String value: {}", value.as_string().unwrap());
    }
}
```

#### Serialization

`MetadataMap` implements `serde::Serialize` and `serde::Deserialize`:

```rust
use serde_json;

// Serialize to JSON
let json = serde_json::to_string_pretty(&metadata)?;
std::fs::write("metadata.json", json)?;

// Deserialize from JSON
let json_data = std::fs::read_to_string("metadata.json")?;
let metadata: MetadataMap = serde_json::from_str(&json_data)?;
```

**JSON Format:**
```json
{
  "EXIF:Make": {
    "type": "String",
    "value": "Canon"
  },
  "EXIF:ISO": {
    "type": "Integer",
    "value": 400
  },
  "EXIF:FNumber": {
    "type": "Float",
    "value": 2.8
  }
}
```

### TagValue

Enum representing different metadata value types.

#### Variants

| Variant | Description | Use Cases |
|---------|-------------|-----------|
| `String(String)` | UTF-8 text | Make/model, artist, copyright |
| `Integer(i64)` | 64-bit signed integer | ISO, width/height, orientation |
| `Float(f64)` | 64-bit floating point | GPS coordinates, aperture |
| `Rational { numerator, denominator }` | Fraction | Exposure time (1/100) |
| `Binary(Vec<u8>)` | Byte data | Thumbnails, ICC profiles |
| `DateTime(DateTime<Utc>)` | UTC timestamp | Creation/modification dates |
| `Struct(Box<HashMap<...>>)` | Nested structure | Complex XMP structures |

#### Constructors

```rust
// String
let value = TagValue::new_string("Canon EOS 5D");

// Integer
let value = TagValue::new_integer(400);

// Float
let value = TagValue::new_float(2.8);

// Rational (1/100 second)
let value = TagValue::new_rational(1, 100);

// Binary
let value = TagValue::new_binary(vec![0xFF, 0xD8, 0xFF, 0xE0]);

// DateTime
use chrono::Utc;
let value = TagValue::new_datetime(Utc::now());

// Struct
let mut structure = HashMap::new();
structure.insert("author".to_string(), TagValue::new_string("John Doe"));
let value = TagValue::new_struct(structure);
```

#### Type Checking

```rust
let value = TagValue::new_string("Canon");

assert!(value.is_string());
assert!(!value.is_integer());
assert!(!value.is_float());
```

#### Type Extraction

```rust
// Safe extraction - returns Option
if let Some(s) = value.as_string() {
    println!("String value: {}", s);
}

// Pattern matching
match value {
    TagValue::String(s) => println!("String: {}", s),
    TagValue::Integer(i) => println!("Integer: {}", i),
    TagValue::Float(f) => println!("Float: {}", f),
    _ => println!("Other type"),
}
```

## Error Handling

### ExifToolError

All fallible operations return `Result<T, ExifToolError>`.

```rust
pub enum ExifToolError {
    IoError(io::Error),
    ParseError { message: String, offset: Option<usize> },
    TagNotFound { tag_name: String },
    InvalidTagValue { tag_name: String, reason: String },
    UnsupportedFormat { message: String },
}
```

#### Error Variants

**`IoError(io::Error)`** - File not found, permission denied, etc.

```rust
match Metadata::from_path("missing.jpg") {
    Err(ExifToolError::IoError(e)) => {
        if e.kind() == std::io::ErrorKind::NotFound {
            eprintln!("File does not exist");
        }
    }
    _ => {}
}
```

**`ParseError { message, offset }`** - Malformed or corrupted file.

```rust
match Metadata::from_path("corrupted.jpg") {
    Err(ExifToolError::ParseError { message, offset }) => {
        eprintln!("Parse error: {}", message);
        if let Some(off) = offset {
            eprintln!("Failed at byte offset: {}", off);
        }
    }
    _ => {}
}
```

**`TagNotFound { tag_name }`** - Requested tag doesn't exist.

```rust
match metadata.require_tag("EXIF:Artist") {
    Err(ExifToolError::TagNotFound { tag_name }) => {
        eprintln!("Required tag '{}' not found", tag_name);
    }
    _ => {}
}
```

**`InvalidTagValue { tag_name, reason }`** - Tag value type mismatch or invalid.

```rust
match metadata.set_tag("EXIF:ISO", "not_a_number") {
    Err(ExifToolError::InvalidTagValue { tag_name, reason }) => {
        eprintln!("Invalid value for {}: {}", tag_name, reason);
    }
    _ => {}
}
```

**`UnsupportedFormat { message }`** - File format not recognized.

```rust
match Metadata::from_path("document.bmp") {
    Err(ExifToolError::UnsupportedFormat { message }) => {
        eprintln!("Format not supported: {}", message);
    }
    _ => {}
}
```

### Error Handling Patterns

#### Early Return with `?`

```rust
use oxidex::{Metadata, Result};

fn process_image(path: &str) -> Result<()> {
    let metadata = Metadata::from_path(path)?;

    let artist = metadata.get_string("EXIF:Artist")
        .unwrap_or("Unknown");

    println!("Artist: {}", artist);
    Ok(())
}
```

#### Match for Detailed Handling

```rust
match Metadata::from_path(path) {
    Ok(metadata) => {
        println!("Loaded {} tags", metadata.len());
    }
    Err(ExifToolError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
        eprintln!("File not found, using defaults");
    }
    Err(ExifToolError::UnsupportedFormat { .. }) => {
        eprintln!("Format not supported, skipping");
    }
    Err(e) => {
        eprintln!("Fatal error: {}", e);
    }
}
```

#### Context with `map_err`

```rust
Metadata::from_path(path)
    .map_err(|e| {
        eprintln!("Failed to process '{}': {}", path, e);
        e
    })?;
```

## Code Examples

### Extract All Tags

```rust
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    let metadata = Metadata::from_path("photo.jpg")?;

    println!("Found {} metadata tags:", metadata.len());
    for (tag_name, tag_value) in metadata.iter_tags() {
        println!("  {}: {:?}", tag_name, tag_value);
    }

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&metadata)?;
    std::fs::write("metadata.json", json)?;

    Ok(())
}
```

### Get Specific Tags

```rust
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    let metadata = Metadata::from_path("photo.jpg")?;

    // String values
    let make = metadata.get_string("EXIF:Make").unwrap_or("Unknown");
    let model = metadata.get_string("EXIF:Model").unwrap_or("Unknown");

    // Integer values
    let iso = metadata.get_integer("EXIF:ISO").unwrap_or(0);

    // Float values
    let aperture = metadata.get_float("EXIF:FNumber").unwrap_or(0.0);

    println!("Camera: {} {}", make, model);
    println!("Settings: ISO {}, f/{:.1}", iso, aperture);

    // GPS coordinates
    if let (Some(lat), Some(lon)) = (
        metadata.get_float("GPS:Latitude"),
        metadata.get_float("GPS:Longitude")
    ) {
        println!("Location: {:.4}, {:.4}", lat, lon);
    }

    Ok(())
}
```

### Modify Metadata

```rust
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    Metadata::from_path("original.jpg")?
        .set_tag("EXIF:Artist", "Jane Smith")?
        .set_tag("EXIF:Copyright", "2025 Jane Smith")?
        .set_tag("EXIF:Rating", 5)?
        .remove_tag("EXIF:Thumbnail")?
        .write_to("modified.jpg")?;

    println!("Metadata updated successfully");
    Ok(())
}
```

### Batch Processing

```rust
use oxidex::{Metadata, Result};
use rayon::prelude::*;

fn main() -> Result<()> {
    let files = vec!["photo1.jpg", "photo2.jpg", "photo3.jpg"];

    // Process files in parallel
    let results: Vec<_> = files
        .par_iter()
        .map(|path| process_file(path))
        .collect();

    for (path, result) in files.iter().zip(results.iter()) {
        match result {
            Ok(info) => println!("✓ {}: {}", path, info),
            Err(e) => eprintln!("✗ {}: {}", path, e),
        }
    }

    Ok(())
}

fn process_file(path: &str) -> Result<String> {
    let metadata = Metadata::from_path(path)?;
    let make = metadata.get_string("EXIF:Make").unwrap_or("Unknown");
    let model = metadata.get_string("EXIF:Model").unwrap_or("Unknown");
    Ok(format!("{} {}", make, model))
}
```

## Advanced Topics

### Memory-Mapped File Access

For large files, OxiDex uses memory-mapped I/O:

```rust
use memmap2::Mmap;
use std::fs::File;

let file = File::open(path)?;
let mmap = unsafe { Mmap::map(&file)? };
let metadata = Metadata::from_bytes(&mmap, None)?;
```

**Benefits:**
- Efficient access to large files
- Only relevant portions paged into memory
- OS-level caching

### Parallel Processing

Use `rayon` for CPU-bound parallel processing:

```rust
use rayon::prelude::*;

let results: Vec<_> = paths
    .par_iter()
    .map(|path| {
        let metadata = Metadata::from_path(path)?;
        Ok(metadata.get_string("EXIF:Make").unwrap_or("Unknown").to_string())
    })
    .collect();
```

**Performance tips:**
- **SSD/NVMe:** 2-4x speedup on fast storage
- **HDD:** Minimal benefit due to I/O bottleneck
- **CPU-bound parsing:** XMP and complex formats benefit most

## Additional Resources

- [Library API Guide](/guide/library-api) - Integration tutorial
- [FFI API Reference](/reference/ffi-api) - C bindings
- [Tag Database](/reference/tag-database) - Complete tag list
- [Architecture](/reference/architecture) - System design
