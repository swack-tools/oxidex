# OxiDex Library API Reference

**Version:** 0.1.0
**Last Updated:** 2025-10-29

## Table of Contents

1. [Introduction](#introduction)
2. [Core Concepts](#core-concepts)
   - [Tag Naming Convention](#tag-naming-convention)
   - [Synchronous API Design](#synchronous-api-design)
   - [Type Safety](#type-safety)
3. [High-Level API](#high-level-api)
   - [Metadata Struct](#metadata-struct)
   - [Reading Metadata](#reading-metadata)
   - [Writing Metadata](#writing-metadata)
   - [Builder Pattern Operations](#builder-pattern-operations)
4. [Low-Level API](#low-level-api)
   - [MetadataMap](#metadatamap)
   - [TagValue](#tagvalue)
5. [Error Handling](#error-handling)
   - [ExifToolError](#exiftoolerror)
   - [Result Type](#result-type)
   - [Error Handling Patterns](#error-handling-patterns)
6. [Code Examples](#code-examples)
   - [Example 1: Extract All Tags](#example-1-extract-all-tags)
   - [Example 2: Get Specific Tag Values](#example-2-get-specific-tag-values)
   - [Example 3: Modify Tag Values](#example-3-modify-tag-values)
   - [Example 4: Copy Metadata Between Files](#example-4-copy-metadata-between-files)
   - [Example 5: Batch Processing with Error Handling](#example-5-batch-processing-with-error-handling)
   - [Example 6: Working with Different Value Types](#example-6-working-with-different-value-types)
   - [Example 7: JSON Serialization](#example-7-json-serialization)
7. [Advanced Topics](#advanced-topics)
   - [Memory-Mapped File Access](#memory-mapped-file-access)
   - [Parallel Processing](#parallel-processing)

---

## Introduction

OxiDex is a Rust library for reading and writing metadata in various image and media file formats. The library provides both a high-level ergonomic API for common operations and a low-level API for fine-grained control over metadata manipulation.

This document covers the **Rust library API** only. For CLI usage, see the [CLI Documentation](/guide/cli-usage). For C FFI bindings, see the [FFI Documentation](/reference/ffi-api).

**Key Features:**

- **Zero-cost abstractions**: Efficient parsing with minimal overhead
- **Type-safe**: Strongly-typed metadata values with compile-time checks
- **Synchronous design**: Simple, predictable execution model
- **Error handling**: Comprehensive error types with context
- **Format support**: JPEG, TIFF, XMP, PNG, and more

---

## Core Concepts

### Tag Naming Convention

All metadata tags in OxiDex follow a standardized naming convention:

```
<FormatFamily>:<TagName>
```

**Examples:**

- `EXIF:Make` - Camera manufacturer (EXIF format)
- `EXIF:Model` - Camera model
- `EXIF:DateTime` - Image capture date/time
- `XMP-dc:Creator` - Document creator (XMP Dublin Core namespace)
- `XMP-dc:Title` - Document title
- `GPS:Latitude` - GPS latitude coordinate
- `GPS:Longitude` - GPS longitude coordinate
- `IPTC:Keywords` - Image keywords
- `PNG:Description` - PNG text chunk description

**Supported Format Families:**

| Format Family | Description | Example Tags |
|--------------|-------------|--------------|
| `EXIF` | Exchangeable Image File Format | `EXIF:Make`, `EXIF:Model`, `EXIF:ISO` |
| `XMP` | Extensible Metadata Platform | `XMP-dc:Creator`, `XMP-dc:Rights` |
| `IPTC` | International Press Telecommunications Council | `IPTC:Keywords`, `IPTC:Caption` |
| `GPS` | GPS location data | `GPS:Latitude`, `GPS:Longitude` |
| `ICC_Profile` | Color management profiles | `ICC_Profile:ProfileDescription` |
| `Photoshop` | Adobe Photoshop metadata | `Photoshop:Credit`, `Photoshop:Source` |
| `MakerNotes` | Camera-specific maker notes | `MakerNotes:SerialNumber` |
| `PNG` | Portable Network Graphics | `PNG:Title`, `PNG:Author` |
| `JFIF` | JPEG File Interchange Format | `JFIF:XResolution`, `JFIF:YResolution` |
| `QuickTime` | QuickTime/MOV video metadata | `QuickTime:Duration`, `QuickTime:CreateDate` |

**Case Sensitivity:** Tag names are **case-sensitive**. Always use the exact capitalization shown in the tag database.

### Synchronous API Design

OxiDex uses a **synchronous, blocking API** design:

- All operations complete before returning
- No async/await or futures
- File I/O is the bottleneck, not computation
- Parallel processing is achieved via `rayon` at the application level

**Rationale:** File I/O dominates performance in metadata extraction. The overhead of async runtimes provides no benefit and adds complexity. For batch processing, use `rayon`'s parallel iterators (see [Example 5](#example-5-batch-processing-with-error-handling)).

### Type Safety

Metadata values are represented by the `TagValue` enum, which provides type safety at runtime:

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

The API provides typed accessor methods that return `Option<T>`, preventing type confusion errors:

```rust
let iso = metadata.get_integer("EXIF:ISO")?;  // Option<i64>
let make = metadata.get_string("EXIF:Make")?;  // Option<&str>
```

---

## High-Level API

### Metadata Struct

**Note:** The `Metadata` struct is the primary entry point for the high-level API. This API is **planned for implementation** and represents the future public interface.

```rust,ignore
use oxidex::Metadata;

pub struct Metadata {
    // Internal fields (not public)
}
```

The `Metadata` struct wraps the lower-level `MetadataMap` and provides convenient methods for reading and writing metadata.

### Reading Metadata

#### `Metadata::from_path(path: impl AsRef<Path>) -> Result<Metadata>`

Opens a file and extracts all metadata tags.

```rust,ignore
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    let metadata = Metadata::from_path("photo.jpg")?;

    // Access metadata through typed getters
    if let Some(make) = metadata.get_string("EXIF:Make") {
        println!("Camera: {}", make);
    }

    Ok(())
}
```

**Parameters:**
- `path`: File path (accepts anything implementing `AsRef<Path>`)

**Returns:**
- `Result<Metadata>`: Metadata object on success, or `ExifToolError` on failure

**Errors:**
- `IoError`: File not found, permission denied, etc.
- `UnsupportedFormat`: File format not recognized
- `ParseError`: File is corrupted or malformed

#### `Metadata::from_bytes(data: &[u8], format_hint: Option<FileFormat>) -> Result<Metadata>`

Parses metadata from a byte buffer.

```rust,ignore
use oxidex::{Metadata, FileFormat};

let file_data = std::fs::read("image.jpg")?;
let metadata = Metadata::from_bytes(&file_data, Some(FileFormat::JPEG))?;
```

**Parameters:**
- `data`: Raw file bytes
- `format_hint`: Optional format hint to skip format detection

**Use Cases:**
- Processing files from memory (e.g., HTTP uploads)
- Working with embedded resources
- Testing with synthetic data

### Writing Metadata

#### Builder Pattern for Modifications

OxiDex uses a **builder pattern** for metadata write operations, enabling fluent, chainable API calls:

```rust,ignore
use oxidex::Metadata;

Metadata::from_path("input.jpg")?
    .set_tag("EXIF:Artist", "John Doe")?
    .set_tag("EXIF:Copyright", "2025 John Doe")?
    .set_tag("EXIF:DateTime", "2025:10:29 14:30:00")?
    .write_to("output.jpg")?;
```

**Key Methods:**

##### `set_tag(tag_name: &str, value: impl Into<TagValue>) -> Result<Self>`

Sets a single tag value.

```rust,ignore
metadata.set_tag("EXIF:Make", "Canon")?;
metadata.set_tag("EXIF:ISO", 400)?;  // Accepts integers
metadata.set_tag("EXIF:FNumber", 2.8)?;  // Accepts floats
```

##### `remove_tag(tag_name: &str) -> Result<Self>`

Removes a tag from the metadata.

```rust,ignore
metadata.remove_tag("EXIF:Thumbnail")?;
```

##### `write_to(path: impl AsRef<Path>) -> Result<()>`

Writes the modified metadata to a new file.

```rust,ignore
metadata.write_to("output.jpg")?;
```

##### `write_in_place() -> Result<()>`

Writes the modified metadata back to the original file.

```rust,ignore
Metadata::from_path("photo.jpg")?
    .set_tag("EXIF:Artist", "Jane Smith")?
    .write_in_place()?;
```

**Warning:** In-place writes modify the original file. Always work on copies for critical files.

### Builder Pattern Operations

#### Copy Metadata Between Files

```rust,ignore
use oxidex::Metadata;

// Copy all tags from source to destination
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

#### Advanced Options

```rust,ignore
// Preserve file modification times
Metadata::from_path("input.jpg")?
    .set_tag("EXIF:Artist", "Photographer")?
    .preserve_file_times(true)?
    .write_to("output.jpg")?;

// Overwrite existing file
Metadata::from_path("source.jpg")?
    .copy_tags_to("existing.jpg")?
    .overwrite(true)?
    .execute()?;
```

---

## Low-Level API

The low-level API provides direct access to the core data structures. Use this when you need fine-grained control or when building higher-level abstractions.

### MetadataMap

`MetadataMap` is the in-memory representation of file metadata. It stores key-value pairs where keys are tag names and values are `TagValue` enums.

**Location:** `src/core/metadata_map.rs:19`

```rust
use oxidex::core::metadata_map::MetadataMap;
use oxidex::core::tag_value::TagValue;

let mut metadata = MetadataMap::new();
metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
```

#### Construction

##### `new() -> Self`

Creates an empty `MetadataMap`.

```rust
let metadata = MetadataMap::new();
assert_eq!(metadata.len(), 0);
```

##### `with_capacity(capacity: usize) -> Self`

Creates a `MetadataMap` pre-allocated for the specified number of tags.

```rust
let metadata = MetadataMap::with_capacity(50);
```

**Use Case:** Improves performance when you know the approximate tag count in advance.

#### Insertion and Modification

##### `insert<K: Into<String>>(&mut self, key: K, value: TagValue) -> Option<TagValue>`

Inserts or replaces a tag. Returns the previous value if the tag existed.

```rust
let mut metadata = MetadataMap::new();
metadata.insert("EXIF:Make", TagValue::new_string("Nikon"));

// Replace existing value
let old_value = metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
assert_eq!(old_value.unwrap().as_string(), Some("Nikon"));
```

##### `get_mut(&mut self, key: &str) -> Option<&mut TagValue>`

Returns a mutable reference to a tag value.

```rust
if let Some(tag) = metadata.get_mut("EXIF:ISO") {
    *tag = TagValue::new_integer(800);
}
```

##### `remove(&mut self, key: &str) -> Option<TagValue>`

Removes a tag and returns its value.

```rust
let removed = metadata.remove("EXIF:Thumbnail");
```

##### `clear(&mut self)`

Removes all tags.

```rust
metadata.clear();
assert!(metadata.is_empty());
```

#### Retrieval

##### `get(&self, key: &str) -> Option<&TagValue>`

Returns a reference to a tag value.

```rust
if let Some(tag_value) = metadata.get("EXIF:Make") {
    println!("Value: {:?}", tag_value);
}
```

##### `get_string(&self, key: &str) -> Option<&str>`

Typed accessor for string values. Returns `None` if the tag doesn't exist or isn't a `String` variant.

```rust
match metadata.get_string("EXIF:Make") {
    Some(make) => println!("Camera make: {}", make),
    None => println!("Make tag not found or wrong type"),
}
```

**See also:** `src/core/metadata_map.rs:128`

##### `get_integer(&self, key: &str) -> Option<i64>`

Typed accessor for integer values.

```rust
if let Some(iso) = metadata.get_integer("EXIF:ISO") {
    println!("ISO: {}", iso);
}
```

**Alias:** This method is also available as `get_i64()` (planned).

**See also:** `src/core/metadata_map.rs:135`

##### `get_float(&self, key: &str) -> Option<f64>`

Typed accessor for floating-point values.

```rust
if let Some(aperture) = metadata.get_float("EXIF:FNumber") {
    println!("f/{:.1}", aperture);
}
```

**Alias:** This method is also available as `get_f64()` (planned).

**See also:** `src/core/metadata_map.rs:142`

##### `get_datetime(&self, key: &str) -> Option<DateTime<Utc>>` (Planned)

Typed accessor for datetime values. This method will extract `DateTime` variants from tags.

```rust,ignore
use chrono::{DateTime, Utc};

if let Some(dt) = metadata.get_datetime("EXIF:DateTime") {
    println!("Photo taken: {}", dt.format("%Y-%m-%d %H:%M:%S"));
}
```

**Status:** Planned for implementation

#### Querying

##### `contains_key(&self, key: &str) -> bool`

Checks if a tag exists.

```rust
if metadata.contains_key("EXIF:Make") {
    println!("Make tag is present");
}
```

##### `len(&self) -> usize`

Returns the number of tags.

```rust
println!("Found {} tags", metadata.len());
```

##### `is_empty(&self) -> bool`

Returns `true` if no tags are present.

```rust
if metadata.is_empty() {
    println!("No metadata found");
}
```

#### Iteration

##### `iter(&self) -> impl Iterator<Item = (&String, &TagValue)>`

Returns an iterator over all tag name-value pairs.

```rust
for (name, value) in metadata.iter() {
    println!("{}: {:?}", name, value);
}
```

**Alias:** This method is also available as `iter_tags()` (planned).

##### `keys(&self) -> impl Iterator<Item = &String>`

Returns an iterator over tag names.

```rust
for tag_name in metadata.keys() {
    println!("Tag: {}", tag_name);
}
```

##### `values(&self) -> impl Iterator<Item = &TagValue>`

Returns an iterator over tag values.

```rust
for value in metadata.values() {
    if value.is_string() {
        println!("String value: {}", value.as_string().unwrap());
    }
}
```

#### Serialization

`MetadataMap` implements `serde::Serialize` and `serde::Deserialize`, enabling JSON serialization:

```rust
use serde_json;

let json = serde_json::to_string_pretty(&metadata)?;
println!("{}", json);

// Deserialize from JSON
let metadata: MetadataMap = serde_json::from_str(&json)?;
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

`TagValue` is an enum representing different metadata value types.

**Location:** `src/core/tag_value.rs:17`

```rust
use oxidex::core::tag_value::TagValue;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub enum TagValue {
    String(String),
    Integer(i64),
    Float(f64),
    Rational { numerator: i32, denominator: i32 },
    Binary(Vec<u8>),
    DateTime(DateTime<Utc>),
    Struct(Box<HashMap<String, TagValue>>),
}
```

#### Variants

| Variant | Description | Common Use Cases |
|---------|-------------|------------------|
| `String(String)` | UTF-8 text | Camera make/model, artist name, copyright |
| `Integer(i64)` | 64-bit signed integer | ISO speed, image width/height, orientation |
| `Float(f64)` | 64-bit floating point | GPS coordinates, aperture, shutter speed |
| `Rational { numerator, denominator }` | Fraction (n/d) | EXIF rational values, exposure time (e.g., 1/100) |
| `Binary(Vec<u8>)` | Arbitrary byte data | Thumbnails, ICC profiles, maker notes |
| `DateTime(DateTime<Utc>)` | UTC timestamp | Creation date, modification date |
| `Struct(Box<HashMap<String, TagValue>>)` | Nested structure | Complex XMP structures |

#### Constructors

```rust
// String
let value = TagValue::new_string("Canon EOS 5D");
let value = TagValue::new_string(String::from("Nikon"));

// Integer
let value = TagValue::new_integer(400);

// Float
let value = TagValue::new_float(2.8);

// Rational
let value = TagValue::new_rational(1, 100);  // 1/100 second

// Binary
let value = TagValue::new_binary(vec![0xFF, 0xD8, 0xFF, 0xE0]);

// DateTime
use chrono::Utc;
let value = TagValue::new_datetime(Utc::now());

// Struct
let mut structure = HashMap::new();
structure.insert("author".to_string(), TagValue::new_string("John Doe"));
structure.insert("version".to_string(), TagValue::new_integer(1));
let value = TagValue::new_struct(structure);
```

**See also:** `src/core/tag_value.rs:47-83`

#### Type Checking

```rust
let value = TagValue::new_string("Canon");

assert!(value.is_string());
assert!(!value.is_integer());
assert!(!value.is_float());
assert!(!value.is_rational());
assert!(!value.is_binary());
assert!(!value.is_datetime());
assert!(!value.is_struct());
```

**See also:** `src/core/tag_value.rs:85-118`

#### Type Extraction

```rust
let value = TagValue::new_string("Canon");

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

**Available Extractors:**
- `as_string(&self) -> Option<&str>`
- `as_integer(&self) -> Option<i64>`
- `as_float(&self) -> Option<f64>`

**See also:** `src/core/tag_value.rs:120-143`

**Note:** Extractors for `Rational`, `Binary`, `DateTime`, and `Struct` variants are planned for future implementation.

---

## Error Handling

### ExifToolError

All fallible operations return `Result<T, ExifToolError>`. The `ExifToolError` enum provides detailed error information with context.

**Location:** `src/error/mod.rs:14`

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

##### `IoError(io::Error)`

Wraps standard I/O errors (file not found, permission denied, etc.).

```rust
use oxidex::{Metadata, ExifToolError};

match Metadata::from_path("missing.jpg") {
    Err(ExifToolError::IoError(e)) => {
        eprintln!("File error: {}", e);
        // Check specific I/O error kind
        if e.kind() == std::io::ErrorKind::NotFound {
            eprintln!("File does not exist");
        }
    }
    Ok(metadata) => { /* ... */ }
    _ => {}
}
```

##### `ParseError { message, offset }`

Indicates a malformed or corrupted file. Optionally includes the byte offset where parsing failed.

```rust,ignore
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

**Common Causes:**
- Truncated files
- Invalid JPEG/TIFF markers
- Malformed XMP XML
- Corrupt IFD structures

##### `TagNotFound { tag_name }`

The requested tag does not exist in the metadata.

```rust,ignore
match metadata.get_string("EXIF:Artist") {
    Some(artist) => println!("Artist: {}", artist),
    None => {
        // Tag doesn't exist or wrong type
        // This returns None, not an error
    }
}

// If using a method that returns Result:
match metadata.require_tag("EXIF:Artist") {
    Err(ExifToolError::TagNotFound { tag_name }) => {
        eprintln!("Required tag '{}' not found", tag_name);
    }
    Ok(value) => { /* ... */ }
    _ => {}
}
```

**Note:** Most `get_*` methods return `Option<T>` rather than `Result<T>`, so missing tags don't produce errors. Only operations that explicitly require a tag will return `TagNotFound`.

##### `InvalidTagValue { tag_name, reason }`

Tag value doesn't match the expected type or is invalid.

```rust,ignore
match metadata.set_tag("EXIF:ISO", "not_a_number") {
    Err(ExifToolError::InvalidTagValue { tag_name, reason }) => {
        eprintln!("Invalid value for {}: {}", tag_name, reason);
    }
    _ => {}
}
```

**Common Causes:**
- Type mismatch (e.g., string provided for integer tag)
- Value out of valid range
- Invalid date/time format

##### `UnsupportedFormat { message }`

File format is not recognized or not yet supported.

```rust,ignore
match Metadata::from_path("document.bmp") {
    Err(ExifToolError::UnsupportedFormat { message }) => {
        eprintln!("Format not supported: {}", message);
    }
    _ => {}
}
```

#### Error Constructors

The `ExifToolError` enum provides convenient constructor methods:

```rust
use oxidex::error::ExifToolError;

// Create errors
let err1 = ExifToolError::parse_error("Invalid marker");
let err2 = ExifToolError::parse_error_at("Unexpected byte", 1024);
let err3 = ExifToolError::tag_not_found("EXIF:Make");
let err4 = ExifToolError::invalid_tag_value("EXIF:ISO", "must be positive");
let err5 = ExifToolError::unsupported_format("BMP not supported");
```

**See also:** `src/error/mod.rs:49-86`

### Result Type

OxiDex defines a type alias for convenience:

```rust
pub type Result<T> = std::result::Result<T, ExifToolError>;
```

**Usage:**

```rust
use oxidex::error::Result;

fn extract_camera_info(path: &str) -> Result<String> {
    let metadata = Metadata::from_path(path)?;

    let make = metadata.get_string("EXIF:Make")
        .ok_or_else(|| ExifToolError::tag_not_found("EXIF:Make"))?;

    let model = metadata.get_string("EXIF:Model")
        .ok_or_else(|| ExifToolError::tag_not_found("EXIF:Model"))?;

    Ok(format!("{} {}", make, model))
}
```

**See also:** `src/error/mod.rs:129`

### Error Handling Patterns

#### Pattern 1: Early Return with `?`

The most idiomatic approach for functions returning `Result`:

```rust,ignore
use oxidex::{Metadata, Result};

fn process_image(path: &str) -> Result<()> {
    let metadata = Metadata::from_path(path)?;

    let artist = metadata.get_string("EXIF:Artist")
        .unwrap_or("Unknown");

    println!("Artist: {}", artist);

    Ok(())
}
```

#### Pattern 2: Match for Detailed Handling

When you need to handle different error types differently:

```rust,ignore
use oxidex::{Metadata, ExifToolError};

fn process_with_fallback(path: &str) {
    match Metadata::from_path(path) {
        Ok(metadata) => {
            println!("Loaded {} tags", metadata.len());
        }
        Err(ExifToolError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("File not found, using defaults");
            // Use default metadata
        }
        Err(ExifToolError::UnsupportedFormat { .. }) => {
            eprintln!("Format not supported, skipping");
        }
        Err(e) => {
            eprintln!("Fatal error: {}", e);
            std::process::exit(1);
        }
    }
}
```

#### Pattern 3: Context with `map_err`

Add context to errors as they propagate:

```rust,ignore
use oxidex::{Metadata, ExifToolError, Result};

fn batch_process(paths: &[&str]) -> Result<()> {
    for path in paths {
        Metadata::from_path(path)
            .map_err(|e| {
                eprintln!("Failed to process '{}': {}", path, e);
                e
            })?;
    }
    Ok(())
}
```

#### Pattern 4: Option Conversion

Convert `Option` to `Result` when needed:

```rust,ignore
use oxidex::{Metadata, ExifToolError, Result};

fn get_required_tag(metadata: &Metadata, tag: &str) -> Result<String> {
    metadata.get_string(tag)
        .map(String::from)
        .ok_or_else(|| ExifToolError::tag_not_found(tag))
}
```

#### Pattern 5: Validation

Use `InvalidTagValue` for custom validation:

```rust,ignore
fn set_iso(metadata: &mut Metadata, iso: i64) -> Result<()> {
    if iso < 0 || iso > 409600 {
        return Err(ExifToolError::invalid_tag_value(
            "EXIF:ISO",
            format!("ISO value {} is out of valid range (0-409600)", iso)
        ));
    }
    metadata.set_tag("EXIF:ISO", iso)
}
```

---

## Code Examples

### Example 1: Extract All Tags

Extract and display all metadata tags from an image file.

```rust,ignore
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    // Open file and extract metadata
    let metadata = Metadata::from_path("photo.jpg")?;

    // Iterate through all tags
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

**Output:**

```
Found 47 metadata tags:
  EXIF:Make: String("Canon")
  EXIF:Model: String("Canon EOS 5D Mark IV")
  EXIF:ISO: Integer(400)
  EXIF:FNumber: Float(2.8)
  EXIF:DateTime: DateTime(2025-10-29T14:30:00Z)
  GPS:Latitude: Float(37.7749)
  GPS:Longitude: Float(-122.4194)
  ...
```

### Example 2: Get Specific Tag Values

Extract specific metadata fields with type safety.

```rust,ignore
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    let metadata = Metadata::from_path("photo.jpg")?;

    // String values
    let camera_make = metadata.get_string("EXIF:Make")
        .unwrap_or("Unknown");
    let camera_model = metadata.get_string("EXIF:Model")
        .unwrap_or("Unknown");

    // Integer values
    let iso = metadata.get_integer("EXIF:ISO")
        .unwrap_or(0);

    // Float values
    let aperture = metadata.get_float("EXIF:FNumber")
        .unwrap_or(0.0);
    let shutter_speed = metadata.get_float("EXIF:ExposureTime")
        .unwrap_or(0.0);

    // Datetime values
    let date_taken = metadata.get_datetime("EXIF:DateTime");

    // Print camera settings
    println!("Camera: {} {}", camera_make, camera_model);
    println!("Settings: ISO {}, f/{:.1}, {:.4}s", iso, aperture, shutter_speed);

    if let Some(dt) = date_taken {
        println!("Taken: {}", dt.format("%Y-%m-%d %H:%M:%S"));
    }

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

**Output:**

```
Camera: Canon Canon EOS 5D Mark IV
Settings: ISO 400, f/2.8, 0.0125s
Taken: 2025-10-29 14:30:00
Location: 37.7749, -122.4194
```

### Example 3: Modify Tag Values

Modify existing metadata and write to a new file.

```rust,ignore
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    // Load metadata from source file
    let mut metadata = Metadata::from_path("original.jpg")?;

    // Modify multiple tags using builder pattern
    metadata
        .set_tag("EXIF:Artist", "Jane Smith")?
        .set_tag("EXIF:Copyright", "2025 Jane Smith. All rights reserved.")?
        .set_tag("EXIF:Rating", 5)?
        .set_tag("EXIF:DateTime", "2025:10:29 14:30:00")?
        .set_tag("IPTC:Keywords", "landscape, nature, mountains")?
        .remove_tag("EXIF:Thumbnail")?  // Remove thumbnail
        .write_to("modified.jpg")?;

    println!("Metadata updated successfully");

    Ok(())
}
```

**In-Place Modification:**

```rust,ignore
use oxidex::{Metadata, Result};

fn add_copyright(path: &str, owner: &str) -> Result<()> {
    Metadata::from_path(path)?
        .set_tag("EXIF:Copyright", format!("© 2025 {}", owner))?
        .set_tag("EXIF:Artist", owner)?
        .write_in_place()?;

    Ok(())
}
```

### Example 4: Copy Metadata Between Files

Copy metadata from one file to another.

```rust,ignore
use oxidex::{Metadata, Result};

fn main() -> Result<()> {
    // Copy all metadata from source to destination
    Metadata::from_path("original.jpg")?
        .copy_tags_to("edited.jpg")?
        .execute()?;

    println!("All metadata copied");

    // Copy only specific tags
    Metadata::from_path("original.jpg")?
        .copy_tags_to("edited.jpg")?
        .with_tags(&[
            "EXIF:DateTime",
            "EXIF:Make",
            "EXIF:Model",
            "EXIF:ISO",
            "GPS:*",  // All GPS tags
        ])?
        .preserve_file_times(true)?
        .execute()?;

    println!("Selected metadata copied");

    // Copy all except thumbnails and maker notes
    Metadata::from_path("original.jpg")?
        .copy_tags_to("edited.jpg")?
        .exclude_tags(&[
            "EXIF:Thumbnail*",
            "MakerNotes:*",
        ])?
        .execute()?;

    Ok(())
}
```

### Example 5: Batch Processing with Error Handling

Process multiple files in parallel with comprehensive error handling.

```rust,ignore
use oxidex::{Metadata, ExifToolError, Result};
use rayon::prelude::*;
use std::path::PathBuf;

fn main() -> Result<()> {
    let image_files = vec![
        "photo1.jpg",
        "photo2.jpg",
        "photo3.jpg",
        "photo4.jpg",
    ];

    // Process files in parallel using rayon
    let results: Vec<_> = image_files
        .par_iter()
        .map(|path| process_single_file(path))
        .collect();

    // Analyze results
    let mut success_count = 0;
    let mut error_count = 0;

    for (path, result) in image_files.iter().zip(results.iter()) {
        match result {
            Ok(info) => {
                println!("✓ {}: {}", path, info);
                success_count += 1;
            }
            Err(e) => {
                eprintln!("✗ {}: {}", path, e);
                error_count += 1;
            }
        }
    }

    println!("\nProcessed {} files: {} succeeded, {} failed",
             image_files.len(), success_count, error_count);

    Ok(())
}

fn process_single_file(path: &str) -> Result<String> {
    let metadata = Metadata::from_path(path)?;

    // Extract camera info
    let make = metadata.get_string("EXIF:Make")
        .unwrap_or("Unknown");
    let model = metadata.get_string("EXIF:Model")
        .unwrap_or("Unknown");

    Ok(format!("{} {}", make, model))
}
```

**With Detailed Error Handling:**

```rust,ignore
fn process_single_file_robust(path: &str) -> Result<String> {
    let metadata = match Metadata::from_path(path) {
        Ok(m) => m,
        Err(ExifToolError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ExifToolError::parse_error(format!("File not found: {}", path)));
        }
        Err(ExifToolError::UnsupportedFormat { message }) => {
            return Err(ExifToolError::unsupported_format(
                format!("{}: {}", path, message)
            ));
        }
        Err(ExifToolError::ParseError { message, offset }) => {
            let detail = if let Some(off) = offset {
                format!("at offset {}: {}", off, message)
            } else {
                message
            };
            return Err(ExifToolError::parse_error(format!("{}: {}", path, detail)));
        }
        Err(e) => return Err(e),
    };

    // Extract info...
    Ok(format!("Processed {}", path))
}
```

### Example 6: Working with Different Value Types

Demonstrate handling all TagValue variants.

```rust,ignore
use oxidex::core::metadata_map::MetadataMap;
use oxidex::core::tag_value::TagValue;
use chrono::{Utc, TimeZone};
use std::collections::HashMap;

fn main() {
    let mut metadata = MetadataMap::new();

    // String
    metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

    // Integer
    metadata.insert("EXIF:ISO", TagValue::new_integer(400));
    metadata.insert("EXIF:PixelWidth", TagValue::new_integer(6000));

    // Float
    metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));
    metadata.insert("GPS:Latitude", TagValue::new_float(37.7749));

    // Rational (exposure time: 1/125 second)
    metadata.insert("EXIF:ExposureTime", TagValue::new_rational(1, 125));

    // Binary (thumbnail data)
    let thumbnail = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    metadata.insert("EXIF:ThumbnailImage", TagValue::new_binary(thumbnail));

    // DateTime
    let dt = Utc.with_ymd_and_hms(2025, 10, 29, 14, 30, 0).unwrap();
    metadata.insert("EXIF:DateTime", TagValue::new_datetime(dt));

    // Struct (XMP structure)
    let mut author_struct = HashMap::new();
    author_struct.insert("name".to_string(), TagValue::new_string("John Doe"));
    author_struct.insert("email".to_string(), TagValue::new_string("john@example.com"));
    metadata.insert("XMP-dc:Creator", TagValue::new_struct(author_struct));

    // Iterate and print types
    for (name, value) in metadata.iter() {
        let type_name = match value {
            TagValue::String(_) => "String",
            TagValue::Integer(_) => "Integer",
            TagValue::Float(_) => "Float",
            TagValue::Rational { .. } => "Rational",
            TagValue::Binary(_) => "Binary",
            TagValue::DateTime(_) => "DateTime",
            TagValue::Struct(_) => "Struct",
        };
        println!("{}: {} = {:?}", name, type_name, value);
    }
}
```

### Example 7: JSON Serialization

Serialize and deserialize metadata to/from JSON.

```rust,ignore
use oxidex::{Metadata, Result};
use serde_json;

fn main() -> Result<()> {
    // Load metadata from image
    let metadata = Metadata::from_path("photo.jpg")?;

    // Serialize to JSON (pretty-printed)
    let json = serde_json::to_string_pretty(&metadata)?;

    // Save to file
    std::fs::write("metadata.json", &json)?;
    println!("Metadata exported to metadata.json");

    // Load from JSON
    let json_data = std::fs::read_to_string("metadata.json")?;
    let loaded_metadata: MetadataMap = serde_json::from_str(&json_data)?;

    println!("Loaded {} tags from JSON", loaded_metadata.len());

    // Compact JSON (one line)
    let compact = serde_json::to_string(&metadata)?;
    println!("Compact JSON: {} bytes", compact.len());

    Ok(())
}
```

**JSON Output Format:**

```json
{
  "EXIF:Make": {
    "type": "String",
    "value": "Canon"
  },
  "EXIF:Model": {
    "type": "String",
    "value": "Canon EOS 5D Mark IV"
  },
  "EXIF:ISO": {
    "type": "Integer",
    "value": 400
  },
  "EXIF:FNumber": {
    "type": "Float",
    "value": 2.8
  },
  "EXIF:ExposureTime": {
    "type": "Rational",
    "value": {
      "numerator": 1,
      "denominator": 125
    }
  },
  "EXIF:DateTime": {
    "type": "DateTime",
    "value": "2025-10-29T14:30:00Z"
  }
}
```

---

## Advanced Topics

### Memory-Mapped File Access

For large files, OxiDex uses memory-mapped I/O to avoid loading entire files into RAM:

```rust,ignore
use memmap2::Mmap;
use std::fs::File;

fn process_large_file(path: &str) -> Result<()> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    // Parse directly from memory-mapped region
    let metadata = Metadata::from_bytes(&mmap, None)?;

    Ok(())
}
```

**Benefits:**
- Efficient access to large files (multi-GB video files)
- Only relevant portions are paged into memory
- OS-level caching automatically applied

### Parallel Processing

Use `rayon` for CPU-bound parallel processing:

```rust,ignore
use rayon::prelude::*;
use oxidex::{Metadata, Result};

fn batch_extract(paths: &[String]) -> Vec<Result<String>> {
    paths.par_iter()
        .map(|path| {
            let metadata = Metadata::from_path(path)?;
            let make = metadata.get_string("EXIF:Make")
                .unwrap_or("Unknown");
            Ok(make.to_string())
        })
        .collect()
}
```

**Performance Considerations:**

- **I/O Bound:** Parallel processing provides minimal benefit for HDD-backed storage
- **SSD/NVMe:** Can see 2-4x speedup on fast storage
- **Network Storage:** May saturate network bandwidth with parallelism
- **CPU Bound:** Parsing XMP and complex formats benefit from parallelism

**Thread Pool Configuration:**

```rust,ignore
use rayon::ThreadPoolBuilder;

fn main() {
    // Configure rayon thread pool
    ThreadPoolBuilder::new()
        .num_threads(4)
        .build_global()
        .unwrap();

    // Now parallel iterators will use 4 threads
}
```

---

## Additional Resources

- **Source Code:** [https://github.com/codemaestro64/oxidex](https://github.com/codemaestro64/oxidex)
- **CLI Documentation:** [CLI Usage Guide](/guide/cli-usage)
- **FFI Documentation:** [C API Reference](/reference/ffi-api)
- **Tag Database:** [Supported Tags](/reference/tag-database)
- **Format Support:** [Supported File Formats](/reference/formats/)

---

**Document Version:** 1.0
**Last Updated:** 2025-10-29
**Minimum Rust Version:** 1.75+
