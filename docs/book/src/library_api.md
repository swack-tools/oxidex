# Library API

This chapter covers how to use ExifTool-RS as a Rust library in your own applications.

## Overview

ExifTool-RS provides a Rust library API for reading and writing metadata in various image and media file formats. The library offers both high-level ergonomic APIs (planned) and low-level APIs (currently implemented) for fine-grained control over metadata manipulation.

**Important Note**: This chapter documents both the **planned high-level API** (for future reference) and the **current low-level API** (available now). Many code examples use the planned API and are marked with `rust,ignore`. Working examples using the current API are provided in the [Working Examples](#working-examples-current-api) section.

## Key Features

- **Zero-Cost Abstractions**: Efficient parsing with minimal overhead
- **Type-Safe**: Strongly-typed metadata values with runtime type checks
- **Synchronous Design**: Simple, predictable execution model
- **Comprehensive Error Handling**: Detailed error types with context
- **Format Support**: JPEG, TIFF, PNG, PDF, MP4/QuickTime, XMP, IPTC

## Core Concepts

### Tag Naming Convention

All metadata tags in ExifTool-RS follow a standardized naming convention:

```
<FormatFamily>:<TagName>
```

**Examples:**

- `EXIF:Make` - Camera manufacturer
- `EXIF:Model` - Camera model
- `EXIF:DateTime` - Image capture date/time
- `XMP-dc:Creator` - Document creator (XMP Dublin Core)
- `GPS:Latitude` - GPS latitude coordinate
- `IPTC:Keywords` - Image keywords
- `PNG:Description` - PNG text description

**Supported Format Families:**

| Format Family | Description | Example Tags |
|--------------|-------------|--------------|
| `EXIF` | Exchangeable Image File Format | `EXIF:Make`, `EXIF:Model`, `EXIF:ISO` |
| `XMP` | Extensible Metadata Platform | `XMP-dc:Creator`, `XMP-dc:Rights` |
| `IPTC` | Press metadata standard | `IPTC:Keywords`, `IPTC:Caption-Abstract` |
| `GPS` | GPS location data | `GPS:GPSLatitude`, `GPS:GPSLongitude` |
| `ICC_Profile` | Color management | `ICC_Profile:ProfileDescription` |
| `Photoshop` | Adobe Photoshop metadata | `Photoshop:Credit`, `Photoshop:Source` |
| `PNG` | Portable Network Graphics | `PNG:Title`, `PNG:Author` |
| `JFIF` | JPEG File Interchange Format | `JFIF:XResolution`, `JFIF:YResolution` |
| `QuickTime` | Video metadata | `QuickTime:Duration`, `QuickTime:CreateDate` |

**Case Sensitivity**: Tag names are case-sensitive. Always use the exact capitalization.

### Synchronous API Design

ExifTool-RS uses a **synchronous, blocking API** design:

- All operations complete before returning
- No async/await or futures
- File I/O is the bottleneck, not computation
- Parallel processing is achieved via `rayon` at the application level

**Rationale**: File I/O dominates performance in metadata extraction. The overhead of async runtimes provides no benefit. For batch processing, use `rayon`'s parallel iterators (see examples below).

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

The API provides typed accessor methods that return `Option<T>`:

```rust
let iso = metadata.get("EXIF:ISO")?.as_integer()?;  // Option<i64>
let make = metadata.get("EXIF:Make")?.as_string()?;  // Option<&str>
```

## Planned High-Level API

**Status**: 🔄 In Development

The high-level API is designed to provide an ergonomic, builder-pattern interface for common operations. These examples show the planned API design (not yet fully implemented).

### Reading Metadata (Planned)

```rust,ignore
use exiftool_rs::Metadata;

fn main() -> exiftool_rs::Result<()> {
    // Open file and extract all metadata
    let metadata = Metadata::from_path("photo.jpg")?;

    // Access metadata through typed getters
    if let Some(make) = metadata.get_string("EXIF:Make") {
        println!("Camera: {}", make);
    }

    if let Some(iso) = metadata.get_integer("EXIF:ISO") {
        println!("ISO: {}", iso);
    }

    Ok(())
}
```

### Writing Metadata (Planned)

```rust,ignore
use exiftool_rs::Metadata;

fn main() -> exiftool_rs::Result<()> {
    // Load, modify, and write metadata using builder pattern
    Metadata::from_path("input.jpg")?
        .set_tag("EXIF:Artist", "John Doe")?
        .set_tag("EXIF:Copyright", "2025 John Doe")?
        .set_tag("EXIF:DateTime", "2025:01:15 14:30:00")?
        .write_to("output.jpg")?;

    println!("Metadata updated successfully");
    Ok(())
}
```

### Copying Metadata (Planned)

```rust,ignore
use exiftool_rs::Metadata;

fn main() -> exiftool_rs::Result<()> {
    // Copy all metadata from source to destination
    Metadata::from_path("source.jpg")?
        .copy_tags_to("dest.jpg")?
        .execute()?;

    // Copy only specific tags
    Metadata::from_path("source.jpg")?
        .copy_tags_to("dest.jpg")?
        .with_tags(&["EXIF:DateTime", "EXIF:Make", "EXIF:Model"])?
        .execute()?;

    Ok(())
}
```

## Current Low-Level API

**Status**: ✅ Available Now

The low-level API provides direct access to the core data structures. Use this for production code until the high-level API is fully implemented.

### Core Types

#### MetadataMap

`MetadataMap` is a wrapper around `HashMap<String, TagValue>` that stores metadata tags:

```rust
use exiftool_rs::core::metadata_map::MetadataMap;

// Create a new metadata map
let mut metadata = MetadataMap::new();

// Insert tags
metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
metadata.insert("EXIF:ISO", TagValue::new_integer(400));

// Get tags
if let Some(make) = metadata.get("EXIF:Make") {
    println!("Camera: {:?}", make);
}

// Iterate tags
for (name, value) in metadata.iter() {
    println!("{}: {:?}", name, value);
}
```

#### TagValue

`TagValue` enum represents different metadata value types:

```rust
use exiftool_rs::core::tag_value::TagValue;
use chrono::{Utc, TimeZone};

// String
let make = TagValue::new_string("Canon");

// Integer
let iso = TagValue::new_integer(400);

// Float
let aperture = TagValue::new_float(2.8);

// Rational (fraction)
let exposure = TagValue::new_rational(1, 125);  // 1/125 second

// Binary
let thumbnail = TagValue::new_binary(vec![0xFF, 0xD8, 0xFF]);

// DateTime
let dt = Utc.with_ymd_and_hms(2025, 1, 15, 14, 30, 0).unwrap();
let datetime = TagValue::new_datetime(dt);
```

### Core Operations

The `exiftool_rs::core::operations` module provides the main metadata operations:

#### read_metadata

Read all metadata from a file:

```rust
use exiftool_rs::core::operations::read_metadata;
use std::path::Path;

fn main() -> exiftool_rs::Result<()> {
    let path = Path::new("photo.jpg");
    let metadata = read_metadata(path)?;

    println!("Found {} tags", metadata.len());

    for (tag_name, tag_value) in metadata.iter() {
        println!("{}: {:?}", tag_name, tag_value);
    }

    Ok(())
}
```

#### modify_tag

Modify a single tag in a file:

```rust
use exiftool_rs::core::operations::modify_tag;
use exiftool_rs::core::tag_value::TagValue;
use std::path::Path;

fn main() -> exiftool_rs::Result<()> {
    let path = Path::new("photo.jpg");
    let tag_name = "EXIF:Artist";
    let tag_value = TagValue::new_string("John Doe".to_string());

    modify_tag(path, tag_name, tag_value)?;

    println!("Tag updated successfully");
    Ok(())
}
```

#### copy_metadata

Copy metadata from one file to another:

```rust
use exiftool_rs::core::operations::copy_metadata;
use std::path::Path;

fn main() -> exiftool_rs::Result<()> {
    let source = Path::new("source.jpg");
    let dest = Path::new("dest.jpg");

    // Copy all tags
    copy_metadata(source, dest, None)?;

    // Copy specific tags only
    let tags = vec!["EXIF:Make".to_string(), "EXIF:Model".to_string()];
    copy_metadata(source, dest, Some(&tags))?;

    Ok(())
}
```

## Working Examples (Current API)

### Example 1: Read and Display All Metadata

```rust
use exiftool_rs::core::operations::read_metadata;
use std::path::Path;

fn main() {
    let path = Path::new("photo.jpg");

    match read_metadata(path) {
        Ok(metadata) => {
            println!("Found {} metadata tags:", metadata.len());
            for (name, value) in metadata.iter() {
                // Display tag name and value
                match value {
                    exiftool_rs::core::tag_value::TagValue::String(s) => {
                        println!("  {}: {}", name, s);
                    }
                    exiftool_rs::core::tag_value::TagValue::Integer(i) => {
                        println!("  {}: {}", name, i);
                    }
                    exiftool_rs::core::tag_value::TagValue::Float(f) => {
                        println!("  {}: {}", name, f);
                    }
                    _ => {
                        println!("  {}: {:?}", name, value);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error reading metadata: {}", e);
        }
    }
}
```

### Example 2: Extract Specific Camera Settings

```rust
use exiftool_rs::core::operations::read_metadata;
use exiftool_rs::core::tag_value::TagValue;
use std::path::Path;

fn main() {
    let path = Path::new("photo.jpg");

    match read_metadata(path) {
        Ok(metadata) => {
            // Extract camera make
            if let Some(TagValue::String(make)) = metadata.get("EXIF:Make") {
                println!("Camera: {}", make);
            }

            // Extract ISO
            if let Some(TagValue::Integer(iso)) = metadata.get("EXIF:ISO") {
                println!("ISO: {}", iso);
            }

            // Extract aperture
            if let Some(TagValue::Float(aperture)) = metadata.get("EXIF:FNumber") {
                println!("Aperture: f/{:.1}", aperture);
            }

            // Extract GPS coordinates
            if let Some(TagValue::Float(lat)) = metadata.get("GPS:GPSLatitude") {
                if let Some(TagValue::Float(lon)) = metadata.get("GPS:GPSLongitude") {
                    println!("Location: {:.4}, {:.4}", lat, lon);
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }
}
```

### Example 3: Modify Metadata

```rust
use exiftool_rs::core::operations::modify_tag;
use exiftool_rs::core::tag_value::TagValue;
use std::path::Path;

fn main() {
    let path = Path::new("photo.jpg");

    // Set artist name
    if let Err(e) = modify_tag(
        path,
        "EXIF:Artist",
        TagValue::new_string("John Doe".to_string())
    ) {
        eprintln!("Error setting artist: {}", e);
        return;
    }

    // Set copyright
    if let Err(e) = modify_tag(
        path,
        "EXIF:Copyright",
        TagValue::new_string("Copyright 2025 John Doe".to_string())
    ) {
        eprintln!("Error setting copyright: {}", e);
        return;
    }

    println!("Metadata updated successfully");
}
```

### Example 4: Batch Processing with Rayon

```rust
use exiftool_rs::core::operations::read_metadata;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

fn main() {
    let files = vec![
        PathBuf::from("photo1.jpg"),
        PathBuf::from("photo2.jpg"),
        PathBuf::from("photo3.jpg"),
    ];

    // Process files in parallel
    let results: Vec<_> = files
        .par_iter()
        .map(|path| process_file(path))
        .collect();

    // Print results
    for (path, result) in files.iter().zip(results.iter()) {
        match result {
            Ok(info) => println!("✓ {}: {}", path.display(), info),
            Err(e) => eprintln!("✗ {}: {}", path.display(), e),
        }
    }
}

fn process_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let metadata = read_metadata(path)?;

    let make = metadata.get("EXIF:Make")
        .and_then(|v| v.as_string())
        .unwrap_or("Unknown");

    let model = metadata.get("EXIF:Model")
        .and_then(|v| v.as_string())
        .unwrap_or("Unknown");

    Ok(format!("{} {}", make, model))
}
```

### Example 5: Copy Metadata Between Files

```rust
use exiftool_rs::core::operations::copy_metadata;
use std::path::Path;

fn main() {
    let source = Path::new("original.jpg");
    let dest = Path::new("edited.jpg");

    // Copy all metadata
    match copy_metadata(source, dest, None) {
        Ok(_) => println!("All metadata copied successfully"),
        Err(e) => eprintln!("Error copying metadata: {}", e),
    }

    // Copy only specific tags
    let specific_tags = vec![
        "EXIF:Artist".to_string(),
        "EXIF:Copyright".to_string(),
        "EXIF:DateTime".to_string(),
    ];

    match copy_metadata(source, dest, Some(&specific_tags)) {
        Ok(_) => println!("Specific tags copied successfully"),
        Err(e) => eprintln!("Error copying tags: {}", e),
    }
}
```

## Error Handling

ExifTool-RS provides comprehensive error types through the `ExifToolError` enum:

```rust
use exiftool_rs::ExifToolError;

// Handle different error types
match read_metadata(path) {
    Ok(metadata) => { /* process metadata */ }
    Err(ExifToolError::IoError(e)) => {
        eprintln!("I/O error: {}", e);
    }
    Err(ExifToolError::UnsupportedFormat { message }) => {
        eprintln!("Unsupported format: {}", message);
    }
    Err(ExifToolError::ParseError { message, offset }) => {
        if let Some(off) = offset {
            eprintln!("Parse error at offset {}: {}", off, message);
        } else {
            eprintln!("Parse error: {}", message);
        }
    }
    Err(e) => {
        eprintln!("Error: {}", e);
    }
}
```

## Advanced Topics

### Parallel Batch Processing

Use `rayon` for efficient parallel processing of multiple files:

```rust
use exiftool_rs::core::operations::read_metadata;
use rayon::prelude::*;
use std::path::PathBuf;

fn process_directory(dir: &str) -> Vec<(PathBuf, Result<usize, String>)> {
    let files: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "jpg"))
        .map(|e| e.path().to_path_buf())
        .collect();

    files
        .par_iter()
        .map(|path| {
            let result = read_metadata(path)
                .map(|m| m.len())
                .map_err(|e| e.to_string());
            (path.clone(), result)
        })
        .collect()
}
```

### Memory-Mapped I/O

ExifTool-RS automatically uses memory-mapped I/O for efficient processing of large files. This is handled internally and requires no configuration.

### Custom Tag Definitions

For detailed information about the tag database and adding custom tags, see the [Tag Database Generation](installation.md#tag-database-generation) section in the Installation chapter.

## API Reference

For complete API documentation, run:

```bash
cargo doc --open
```

This will generate and open the full Rust API documentation in your browser.

## Additional Resources

- **[Command-Line Usage](cli_usage.md)**: CLI interface for ExifTool-RS
- **[C FFI Integration](ffi.md)**: Use ExifTool-RS from C, Python, or other languages
- **[Troubleshooting](troubleshooting.md)**: Common issues and solutions
- **[Full API Reference](../api/library_api.md)**: Comprehensive API documentation (1400+ lines)

## Migration Path

As the high-level API is implemented, we'll maintain backward compatibility with the low-level API. You can start using the low-level API now and gradually migrate to the high-level API as features become available.

**Current Status**:
- ✅ Low-level API (MetadataMap, TagValue, operations)
- 🔄 High-level API (Metadata struct with builder pattern)
- ⏳ Advanced features (conditional edits, tag deletion, group operations)
