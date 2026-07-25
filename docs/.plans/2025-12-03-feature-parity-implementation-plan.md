# Feature Parity Implementation Plan

## Overview

This plan addresses the features documented as "planned" or "not yet implemented" in OxiDex's documentation:

- **CLI Features** (from `docs/guide/cli-usage.md`)
- **Library API** (from `docs/guide/library-api.md`)

## Feature Summary

### CLI Features (5 items)
| Feature | Syntax | Priority | Complexity |
|---------|--------|----------|------------|
| Specific Tag Extraction | `-TAG` | High | Low |
| Tag Deletion | `-TAG=` | High | Medium |
| Group Deletion | `-all=` | High | Medium |
| Short Format Output | `-s` | Low | Low |
| Conditional Edits | `-if '$Make eq "Canon"'` | Low | High |

### Library API (1 major item)
| Feature | Description | Priority | Complexity |
|---------|-------------|----------|------------|
| High-Level API | `Metadata` struct with builder pattern | Medium | Medium |

---

## Phase 1: Specific Tag Extraction (CLI)

**Goal**: Enable `oxidex -Make -Model photo.jpg` to show only specific tags

### Current State
- `args.rs:346-357` - `copy_tag_filters()` already parses `-TAG` arguments (without `=`)
- `output_formatter.rs:48` - `format()` already accepts `filter_tags: Option<&[String]>`
- `main.rs:168-210` - `handle_read_operation()` calls formatter with `None` filter

### Implementation

#### 1.1 Add `specific_tags()` method to CliArgs (`src/cli/args.rs`)

```rust
/// Extracts specific tag names to display (args starting with '-' but not containing '=')
/// Returns None if no specific tags are requested (show all tags)
/// Returns Some(Vec) with tag names if specific tags are specified
pub fn specific_tags(&self) -> Option<Vec<String>> {
    // Only apply if not in copy mode (tags_from_file)
    if self.tags_from_file.is_some() {
        return None;
    }

    // If only file argument present, show all tags
    if self.args.len() <= 1 {
        return None;
    }

    let mut tag_names = Vec::new();

    // Process all arguments except the last one (file path)
    for arg in &self.args[..self.args.len() - 1] {
        // Tag extraction: starts with '-', does NOT contain '='
        if arg.starts_with('-') && !arg.contains('=') {
            let tag_name = arg.trim_start_matches('-').to_string();
            tag_names.push(tag_name);
        }
    }

    if tag_names.is_empty() {
        None
    } else {
        Some(tag_names)
    }
}
```

#### 1.2 Update `handle_read_operation()` in `main.rs`

```rust
fn handle_read_operation(file: &std::path::Path, args: &CliArgs) {
    match read_metadata(file) {
        Ok(metadata) => {
            // Get specific tags filter if provided
            let tag_filter = args.specific_tags();
            let filter_slice = tag_filter.as_deref();

            // ... rest of function passes filter_slice to formatters
        }
    }
}
```

### Tests
- `oxidex -Make photo.jpg` → shows only EXIF:Make
- `oxidex -Make -Model -ISO photo.jpg` → shows three tags
- `oxidex -NonExistent photo.jpg` → shows nothing (or warning)
- `oxidex photo.jpg` → shows all tags (backward compatible)

---

## Phase 2: Tag Deletion (CLI)

**Goal**: Enable `oxidex -TAG= photo.jpg` to delete a specific tag

### Current State
- `parse_modification()` in `args.rs` parses `-TAG=VALUE` syntax
- Value after `=` can be empty string
- `modify_tag()` in `operations.rs` replaces tag value

### Implementation

#### 2.1 Add `remove_tag()` function to `operations.rs`

```rust
/// Removes a metadata tag from a file.
///
/// # Arguments
/// * `path` - Path to the file
/// * `tag_name` - Name of the tag to remove (e.g., "EXIF:Artist")
///
/// # Returns
/// * `Ok(())` - Tag was removed successfully
/// * `Err` - I/O error or unsupported format
pub fn remove_tag(path: &Path, tag_name: &str) -> Result<()> {
    // Step 1: Read existing metadata
    let mut metadata = read_metadata(path)?;

    // Step 2: Remove the tag
    metadata.remove(tag_name);

    // Step 3: Write metadata back
    write_metadata(path, &metadata)?;

    Ok(())
}
```

#### 2.2 Add `remove()` method to MetadataMap (`src/core/metadata_map.rs`)

```rust
/// Removes a tag from the metadata map
pub fn remove(&mut self, tag_name: &str) -> Option<TagValue> {
    self.tags.remove(tag_name)
}
```

#### 2.3 Update `handle_write_operation()` in `main.rs`

```rust
// In handle_write_operation():
for (tag_name, value) in &modifications {
    if value.is_empty() {
        // Empty value = delete tag
        if let Err(e) = remove_tag(file, tag_name) {
            eprintln!("Error: Failed to remove tag '{}': {}", tag_name, e);
            process::exit(1);
        }
    } else {
        // Non-empty value = modify tag
        let tag_value = TagValue::new_string(value.clone());
        if let Err(e) = modify_tag(file, tag_name, tag_value) {
            // ... existing error handling
        }
    }
}
```

### Tests
- `oxidex -EXIF:Artist= photo.jpg` → removes Artist tag
- `oxidex -GPS:GPSLatitude= -GPS:GPSLongitude= photo.jpg` → removes GPS
- Verify tag is actually removed from file

---

## Phase 3: Group Deletion (CLI)

**Goal**: Enable `oxidex -all= photo.jpg` to delete all metadata

### Implementation

#### 3.1 Add `clear_all_metadata()` to `operations.rs`

```rust
/// Removes all metadata from a file, preserving only essential file structure.
///
/// For JPEG: Removes all APP segments except APP0 (JFIF)
/// For TIFF: Removes all IFD entries except required structural ones
/// For PNG: Removes all metadata chunks
pub fn clear_all_metadata(path: &Path) -> Result<()> {
    // Create empty metadata map
    let metadata = MetadataMap::new();

    // Write empty metadata (format-specific writers handle cleanup)
    write_metadata(path, &metadata)?;

    Ok(())
}
```

#### 3.2 Detect `-all=` in CLI args parsing

```rust
// In CliArgs:
pub fn is_clear_all_metadata(&self) -> bool {
    self.args.iter().any(|arg| {
        arg == "-all=" || arg == "-ALL=" || arg == "--all="
    })
}
```

#### 3.3 Update `main.rs` to handle clear all

```rust
// In main(), before other write operations:
if args.is_clear_all_metadata() {
    handle_clear_all_operation(&file, &args);
} else if !modifications.is_empty() {
    handle_write_operation(&file, &args);
}
```

### Tests
- `oxidex -all= photo.jpg` → removes all metadata
- Verify file still opens correctly after metadata removal
- Verify file size decreased

---

## Phase 4: Short Format Output (CLI)

**Goal**: Enable `oxidex -s photo.jpg` for compact output

### Current State
- `args.rs:18` - `short_format: bool` already parsed
- `main.rs:28-30` - Shows "not yet implemented" warning

### Implementation

#### 4.1 Create `ShortFormatter` in `output_formatter.rs`

```rust
/// Formats metadata in compact single-line format
/// Format: "TagName: Value" (no family prefix, shorter tag names)
pub struct ShortFormatter;

impl OutputFormatter for ShortFormatter {
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String {
        let mut tags: Vec<_> = metadata.iter().collect();

        if let Some(filter) = filter_tags {
            tags.retain(|(name, _)| filter.contains(name));
        }

        tags.sort_by_key(|(name, _)| *name);

        let mut output = String::new();
        for (tag_name, tag_value) in tags {
            // Extract short name (after colon)
            let short_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
            let value = format_tag_value_short(tag_value);
            output.push_str(&format!("{}: {}\n", short_name, value));
        }
        output
    }
}

fn format_tag_value_short(value: &TagValue) -> String {
    match value {
        TagValue::String(s) => {
            // Truncate long strings
            if s.len() > 40 {
                format!("{}...", &s[..37])
            } else {
                s.clone()
            }
        }
        TagValue::Binary(bytes) => format!("({} bytes)", bytes.len()),
        // ... other variants
    }
}
```

#### 4.2 Update `handle_read_operation()` to use ShortFormatter

```rust
if args.short_format {
    let formatter = ShortFormatter;
    let output = formatter.format(&metadata, filter_slice);
    print!("{}", output);
} else if args.csv {
    // ... existing code
}
```

### Tests
- `oxidex -s photo.jpg` → shows compact output
- Verify tag names are shortened
- Verify long values are truncated

---

## Phase 5: High-Level Library API

**Goal**: Implement ergonomic `Metadata` struct with builder pattern

### Implementation

#### 5.1 Create `src/core/metadata.rs`

```rust
//! High-level metadata API with ergonomic builder pattern

use crate::core::{MetadataMap, TagValue};
use crate::core::operations::{read_metadata, write_metadata, copy_metadata};
use crate::error::Result;
use std::path::Path;

/// High-level metadata container with ergonomic API
pub struct Metadata {
    map: MetadataMap,
    source_path: Option<std::path::PathBuf>,
}

impl Metadata {
    /// Creates a new empty Metadata container
    pub fn new() -> Self {
        Self {
            map: MetadataMap::new(),
            source_path: None,
        }
    }

    /// Reads metadata from a file path
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let map = read_metadata(path)?;
        Ok(Self {
            map,
            source_path: Some(path.to_path_buf()),
        })
    }

    /// Gets a string value by tag name
    pub fn get_string(&self, tag: &str) -> Option<&str> {
        self.map.get(tag).and_then(|v| v.as_string())
    }

    /// Gets an integer value by tag name
    pub fn get_integer(&self, tag: &str) -> Option<i64> {
        self.map.get(tag).and_then(|v| v.as_integer())
    }

    /// Gets a float value by tag name
    pub fn get_float(&self, tag: &str) -> Option<f64> {
        self.map.get(tag).and_then(|v| v.as_float())
    }

    /// Sets a tag value (builder pattern)
    pub fn set_tag(mut self, tag: &str, value: impl Into<TagValue>) -> Self {
        self.map.insert(tag, value.into());
        self
    }

    /// Writes metadata to a file
    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        write_metadata(path.as_ref(), &self.map)
    }

    /// Saves metadata back to the source file
    pub fn save(&self) -> Result<()> {
        match &self.source_path {
            Some(path) => write_metadata(path, &self.map),
            None => Err(crate::error::ExifToolError::IoError(
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No source path - use write_to() instead"
                )
            )),
        }
    }

    /// Creates a copy operation builder
    pub fn copy_to<P: AsRef<Path>>(&self, dest: P) -> CopyBuilder {
        CopyBuilder {
            source: self,
            dest: dest.as_ref().to_path_buf(),
            tags: None,
        }
    }

    /// Returns number of tags
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterates over all tags
    pub fn iter(&self) -> impl Iterator<Item = (&String, &TagValue)> {
        self.map.iter()
    }
}

/// Builder for copy operations
pub struct CopyBuilder<'a> {
    source: &'a Metadata,
    dest: std::path::PathBuf,
    tags: Option<Vec<String>>,
}

impl<'a> CopyBuilder<'a> {
    /// Filter to copy only specific tags
    pub fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = Some(tags.iter().map(|s| s.to_string()).collect());
        self
    }

    /// Execute the copy operation
    pub fn execute(self) -> Result<()> {
        // Read source metadata
        let source_map = &self.source.map;

        // Read destination metadata
        let mut dest_map = read_metadata(&self.dest)?;

        // Copy tags
        for (tag_name, tag_value) in source_map.iter() {
            let should_copy = self.tags.as_ref()
                .map(|t| t.contains(tag_name))
                .unwrap_or(true);

            if should_copy {
                dest_map.insert(tag_name, tag_value.clone());
            }
        }

        // Write to destination
        write_metadata(&self.dest, &dest_map)
    }
}

// Implement From traits for easy value conversion
impl From<&str> for TagValue {
    fn from(s: &str) -> Self {
        TagValue::new_string(s.to_string())
    }
}

impl From<String> for TagValue {
    fn from(s: String) -> Self {
        TagValue::new_string(s)
    }
}

impl From<i64> for TagValue {
    fn from(i: i64) -> Self {
        TagValue::new_integer(i)
    }
}

impl From<f64> for TagValue {
    fn from(f: f64) -> Self {
        TagValue::new_float(f)
    }
}
```

#### 5.2 Export from `lib.rs`

```rust
// In src/lib.rs, add re-export:
pub use crate::core::metadata::Metadata;
```

#### 5.3 Update `src/core/mod.rs`

```rust
pub mod metadata;
pub use metadata::Metadata;
```

### Tests
```rust
#[test]
fn test_high_level_api_read() {
    let metadata = Metadata::from_path("tests/fixtures/sample.jpg").unwrap();
    assert!(metadata.get_string("EXIF:Make").is_some());
}

#[test]
fn test_high_level_api_write() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::copy("tests/fixtures/sample.jpg", temp.path()).unwrap();

    Metadata::from_path(temp.path()).unwrap()
        .set_tag("EXIF:Artist", "Test Artist")
        .save()
        .unwrap();

    let verify = Metadata::from_path(temp.path()).unwrap();
    assert_eq!(verify.get_string("EXIF:Artist"), Some("Test Artist"));
}
```

---

## Phase 6: Conditional Edits (CLI) - Future

**Goal**: Enable `oxidex -if '$Make eq "Canon"' -Artist="John" photo.jpg`

This is the most complex feature and is deferred to a later phase.

### Design Considerations
- Expression parser needed (mini DSL)
- Variable syntax: `$TagName` references tag value
- Operators: `eq`, `ne`, `=~` (regex), `<`, `>`, `<=`, `>=`
- Logical: `and`, `or`, `not`

### Suggested Approach
1. Use `pest` or `nom` for expression parsing
2. Define grammar for conditions
3. Evaluate condition against metadata before applying changes

---

## Implementation Order

| Phase | Feature | Estimated Effort | Dependencies |
|-------|---------|------------------|--------------|
| 1 | Specific Tag Extraction | 2 hours | None |
| 2 | Tag Deletion | 3 hours | None |
| 3 | Group Deletion | 2 hours | Phase 2 |
| 4 | Short Format Output | 2 hours | None |
| 5 | High-Level API | 4 hours | None |
| 6 | Conditional Edits | 8+ hours | Phases 1-4 |

**Total estimated effort**: ~21 hours (excluding Phase 6)

---

## Documentation Updates

After each phase, update:
1. `docs/guide/cli-usage.md` - Change ⏳ to ✅
2. `docs/guide/library-api.md` - Update "Planned" to "Available"
3. `CHANGELOG.md` - Add feature to Unreleased section

---

## Success Criteria

- [ ] All documented features work as described
- [ ] No regressions in existing functionality
- [ ] Test coverage > 80% for new code
- [ ] Documentation updated
- [ ] `cargo clippy` passes
- [ ] `cargo test` passes
