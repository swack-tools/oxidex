# Sprint 1: Foundation Enhancement - Detailed Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Parent Plan:** docs/plans/2025-11-19-parser-complexity-reduction.md
**Sprint:** 1 of 5
**Duration:** 2 weeks
**Goal:** Enhance shared infrastructure in `src/parsers/tiff/makernotes/shared/` to support array schemas and lens database unification, enabling declarative parser migrations in future sprints.

## Success Criteria
- ArraySchema infrastructure exists and is tested
- TagRegistry supports array-based tags
- LensDatabase trait and reference implementation exist
- All new code has unit tests
- Documentation with examples exists

---

## Task 1: Create ArraySchema Infrastructure

**Files:**
- Create: `src/parsers/tiff/makernotes/shared/array_schemas.rs`
- Modify: `src/parsers/tiff/makernotes/shared/mod.rs`

### Step 1: Define core array schema types

Create `src/parsers/tiff/makernotes/shared/array_schemas.rs`:

```rust
//! Array schema system for declarative CameraSettings-style array parsing
//!
//! Many MakerNote parsers extract arrays of i16/u16/i32 values with specific
//! indices mapping to camera settings. This module provides declarative schemas
//! to eliminate repetitive array extraction code.

use super::generic_decoders::SimpleValueDecoder;
use std::collections::HashMap;

/// Definition of a single array index with its name and optional decoder
#[derive(Debug, Clone)]
pub struct ArrayIndexDef {
    /// Array index (0-based)
    pub index: usize,
    /// Tag name for this index (e.g., "MacroMode", "Quality")
    pub name: &'static str,
    /// Optional decoder for i16 values at this index
    pub decoder_i16: Option<&'static SimpleValueDecoder<i16>>,
    /// Optional decoder for u16 values at this index
    pub decoder_u16: Option<&'static SimpleValueDecoder<u16>>,
    /// Optional decoder for i32 values at this index
    pub decoder_i32: Option<&'static SimpleValueDecoder<i32>>,
    /// Optional decoder for u32 values at this index
    pub decoder_u32: Option<&'static SimpleValueDecoder<u32>>,
}

impl ArrayIndexDef {
    /// Create a new index definition with i16 decoder
    pub const fn with_i16_decoder(
        index: usize,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<i16>,
    ) -> Self {
        Self {
            index,
            name,
            decoder_i16: Some(decoder),
            decoder_u16: None,
            decoder_i32: None,
            decoder_u32: None,
        }
    }

    /// Create a new index definition with u16 decoder
    pub const fn with_u16_decoder(
        index: usize,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<u16>,
    ) -> Self {
        Self {
            index,
            name,
            decoder_i16: None,
            decoder_u16: Some(decoder),
            decoder_i32: None,
            decoder_u32: None,
        }
    }

    /// Create a new index definition without decoder (raw value)
    pub const fn raw(index: usize, name: &'static str) -> Self {
        Self {
            index,
            name,
            decoder_i16: None,
            decoder_u16: None,
            decoder_i32: None,
            decoder_u32: None,
        }
    }
}

/// Schema defining how to parse an array of values
///
/// Example:
/// ```ignore
/// static CAMERA_SETTINGS: ArraySchema = ArraySchema {
///     name: "CameraSettings",
///     indices: &[
///         ArrayIndexDef::with_i16_decoder(1, "MacroMode", &MACRO_MODE),
///         ArrayIndexDef::with_i16_decoder(2, "SelfTimer", &SELF_TIMER),
///         ArrayIndexDef::raw(3, "Quality"),
///     ],
/// };
/// ```
#[derive(Debug)]
pub struct ArraySchema {
    /// Schema name (e.g., "CameraSettings", "ShotInfo")
    pub name: &'static str,
    /// Index definitions
    pub indices: &'static [ArrayIndexDef],
}

impl ArraySchema {
    /// Process an i16 array using this schema
    pub fn process_i16_array(
        &self,
        array: &[i16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(value) = array.get(def.index) {
                let decoded = if let Some(decoder) = def.decoder_i16 {
                    decoder.decode(*value)
                } else {
                    value.to_string()
                };
                tags.insert(format!("{}:{}:{}", prefix, self.name, def.name), decoded);
            }
        }
    }

    /// Process a u16 array using this schema
    pub fn process_u16_array(
        &self,
        array: &[u16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(value) = array.get(def.index) {
                let decoded = if let Some(decoder) = def.decoder_u16 {
                    decoder.decode(*value)
                } else {
                    value.to_string()
                };
                tags.insert(format!("{}:{}:{}", prefix, self.name, def.name), decoded);
            }
        }
    }

    /// Process an i32 array using this schema
    pub fn process_i32_array(
        &self,
        array: &[i32],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(value) = array.get(def.index) {
                let decoded = if let Some(decoder) = def.decoder_i32 {
                    decoder.decode(*value)
                } else {
                    value.to_string()
                };
                tags.insert(format!("{}:{}:{}", prefix, self.name, def.name), decoded);
            }
        }
    }

    /// Process a u32 array using this schema
    pub fn process_u32_array(
        &self,
        array: &[u32],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(value) = array.get(def.index) {
                let decoded = if let Some(decoder) = def.decoder_u32 {
                    decoder.decode(*value)
                } else {
                    value.to_string()
                };
                tags.insert(format!("{}:{}:{}", prefix, self.name, def.name), decoded);
            }
        }
    }
}
```

### Step 2: Export from shared module

Add to `src/parsers/tiff/makernotes/shared/mod.rs`:

```rust
pub mod array_schemas;

pub use array_schemas::{ArrayIndexDef, ArraySchema};
```

### Step 3: Write unit tests

Create `src/parsers/tiff/makernotes/shared/tests/array_schemas.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::super::super::array_schemas::*;
    use super::super::super::generic_decoders::*;
    use std::collections::HashMap;

    const_decoder!(TEST_MODE, i16, [
        (1, "Mode1"),
        (2, "Mode2"),
        (3, "Mode3"),
    ]);

    #[test]
    fn test_array_schema_with_decoder() {
        let schema = ArraySchema {
            name: "TestSettings",
            indices: &[
                ArrayIndexDef::with_i16_decoder(0, "Mode", &TEST_MODE),
                ArrayIndexDef::raw(1, "RawValue"),
            ],
        };

        let array = vec![2i16, 42];
        let mut tags = HashMap::new();

        schema.process_i16_array(&array, "Test", &mut tags);

        assert_eq!(tags.get("Test:TestSettings:Mode"), Some(&"Mode2".to_string()));
        assert_eq!(tags.get("Test:TestSettings:RawValue"), Some(&"42".to_string()));
    }

    #[test]
    fn test_array_schema_missing_indices() {
        let schema = ArraySchema {
            name: "TestSettings",
            indices: &[
                ArrayIndexDef::raw(0, "First"),
                ArrayIndexDef::raw(5, "OutOfBounds"),
            ],
        };

        let array = vec![100i16];
        let mut tags = HashMap::new();

        schema.process_i16_array(&array, "Test", &mut tags);

        assert_eq!(tags.get("Test:TestSettings:First"), Some(&"100".to_string()));
        assert_eq!(tags.get("Test:TestSettings:OutOfBounds"), None);
    }

    #[test]
    fn test_u16_array_processing() {
        let schema = ArraySchema {
            name: "U16Settings",
            indices: &[ArrayIndexDef::raw(0, "Value")],
        };

        let array = vec![65535u16];
        let mut tags = HashMap::new();

        schema.process_u16_array(&array, "Test", &mut tags);

        assert_eq!(tags.get("Test:U16Settings:Value"), Some(&"65535".to_string()));
    }
}
```

### Step 4: Add tests module to shared/mod.rs

Add to `src/parsers/tiff/makernotes/shared/mod.rs`:

```rust
#[cfg(test)]
mod tests;
```

Create `src/parsers/tiff/makernotes/shared/tests/mod.rs`:

```rust
mod array_schemas;
```

### Step 5: Run tests

```bash
cargo test -p oxidex array_schemas
```

Expect: All tests pass.

### Step 6: Commit

```bash
git add src/parsers/tiff/makernotes/shared/array_schemas.rs
git add src/parsers/tiff/makernotes/shared/mod.rs
git add src/parsers/tiff/makernotes/shared/tests/
git commit -m "feat(parsers): add array schema infrastructure for declarative array parsing"
```

---

## Task 2: Enhance TagRegistry for Array Support

**Files:**
- Modify: `src/parsers/tiff/makernotes/shared/tag_registry.rs`
- Modify: `src/parsers/tiff/makernotes/shared/tests/tag_registry.rs` (create if doesn't exist)

### Step 1: Add array schema support to TagRegistry

Add to `src/parsers/tiff/makernotes/shared/tag_registry.rs`:

```rust
use super::array_schemas::ArraySchema;

impl TagRegistry {
    // ... existing methods ...

    /// Register an array-based tag that uses an ArraySchema
    pub fn register_array_schema(
        mut self,
        tag_id: u16,
        schema: &'static ArraySchema,
    ) -> Self {
        self.tags.insert(
            tag_id,
            TagDefinition {
                id: tag_id,
                name: schema.name,
                decoder: Some(TagDecoder::ArraySchema(schema)),
            },
        );
        self
    }
}

// Add to TagDecoder enum
#[derive(Clone)]
pub enum TagDecoder {
    // ... existing variants ...
    /// Array schema for processing array-type tags
    ArraySchema(&'static ArraySchema),
}
```

### Step 2: Update decode methods to handle array schemas

Add helper method to TagRegistry:

```rust
impl TagRegistry {
    /// Decode and insert an array tag using its schema
    pub fn decode_array_i16(
        &self,
        tag_id: u16,
        array: &[i16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        if let Some(def) = self.tags.get(&tag_id) {
            if let Some(TagDecoder::ArraySchema(schema)) = def.decoder {
                schema.process_i16_array(array, prefix, tags);
            }
        }
    }

    /// Decode and insert an array tag using its schema (u16 variant)
    pub fn decode_array_u16(
        &self,
        tag_id: u16,
        array: &[u16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        if let Some(def) = self.tags.get(&tag_id) {
            if let Some(TagDecoder::ArraySchema(schema)) = def.decoder {
                schema.process_u16_array(array, prefix, tags);
            }
        }
    }
}
```

### Step 3: Write tests for array schema registration

Create or append to `src/parsers/tiff/makernotes/shared/tests/tag_registry.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::super::super::tag_registry::*;
    use super::super::super::array_schemas::*;
    use super::super::super::generic_decoders::*;
    use std::collections::HashMap;

    const_decoder!(QUALITY, i16, [
        (1, "Low"),
        (2, "Normal"),
        (3, "High"),
    ]);

    #[test]
    fn test_registry_with_array_schema() {
        static SETTINGS_SCHEMA: ArraySchema = ArraySchema {
            name: "CameraSettings",
            indices: &[
                ArrayIndexDef::with_i16_decoder(1, "Quality", &QUALITY),
                ArrayIndexDef::raw(2, "ISO"),
            ],
        };

        let registry = TagRegistry::new()
            .register_array_schema(0x0001, &SETTINGS_SCHEMA);

        let array = vec![0i16, 2, 400];
        let mut tags = HashMap::new();

        registry.decode_array_i16(0x0001, &array, "Canon", &mut tags);

        assert_eq!(tags.get("Canon:CameraSettings:Quality"), Some(&"Normal".to_string()));
        assert_eq!(tags.get("Canon:CameraSettings:ISO"), Some(&"400".to_string()));
    }
}
```

### Step 4: Run tests

```bash
cargo test -p oxidex tag_registry
```

Expect: All tests pass.

### Step 5: Commit

```bash
git add src/parsers/tiff/makernotes/shared/tag_registry.rs
git add src/parsers/tiff/makernotes/shared/tests/tag_registry.rs
git commit -m "feat(parsers): extend TagRegistry to support array schema tags"
```

---

## Task 3: Create Lens Database Infrastructure

**Files:**
- Create: `src/parsers/tiff/makernotes/shared/lens_database.rs`
- Create: `src/parsers/tiff/makernotes/shared/tests/lens_database.rs`
- Modify: `src/parsers/tiff/makernotes/shared/mod.rs`

### Step 1: Define lens database trait

Create `src/parsers/tiff/makernotes/shared/lens_database.rs`:

```rust
//! Unified lens database infrastructure
//!
//! Provides a common interface for lens lookups across different manufacturers,
//! eliminating duplicated lens database implementations.

/// Trait for lens database lookups
pub trait LensDatabase {
    /// Look up a lens name by its ID
    fn lookup(&self, lens_id: u16) -> Option<&'static str>;

    /// Look up a lens by ID range (for lenses that span multiple IDs)
    fn lookup_range(&self, id_min: u16, id_max: u16) -> Option<&'static str> {
        // Default implementation checks if any ID in range matches
        for id in id_min..=id_max {
            if let Some(name) = self.lookup(id) {
                return Some(name);
            }
        }
        None
    }
}

/// Static lens database implementation backed by a const array
///
/// Most efficient for manufacturers with < 1000 lenses.
/// Uses linear search which is fine for typical database sizes.
pub struct StaticLensDb {
    entries: &'static [(u16, &'static str)],
}

impl StaticLensDb {
    /// Create a new static lens database
    pub const fn new(entries: &'static [(u16, &'static str)]) -> Self {
        Self { entries }
    }
}

impl LensDatabase for StaticLensDb {
    fn lookup(&self, lens_id: u16) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|(id, _)| *id == lens_id)
            .map(|(_, name)| *name)
    }
}

/// Range-based lens database for lenses identified by ID ranges
///
/// Example: Lens IDs 100-105 all map to "Canon EF 50mm f/1.8"
pub struct RangeLensDb {
    entries: &'static [(u16, u16, &'static str)], // (min_id, max_id, name)
}

impl RangeLensDb {
    /// Create a new range-based lens database
    pub const fn new(entries: &'static [(u16, u16, &'static str)]) -> Self {
        Self { entries }
    }
}

impl LensDatabase for RangeLensDb {
    fn lookup(&self, lens_id: u16) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|(min, max, _)| lens_id >= *min && lens_id <= *max)
            .map(|(_, _, name)| *name)
    }
}

/// Combined lens database that checks multiple sources
///
/// Useful when a manufacturer has both exact ID matches and ranges.
pub struct CombinedLensDb {
    static_db: Option<&'static StaticLensDb>,
    range_db: Option<&'static RangeLensDb>,
}

impl CombinedLensDb {
    pub const fn new(
        static_db: Option<&'static StaticLensDb>,
        range_db: Option<&'static RangeLensDb>,
    ) -> Self {
        Self { static_db, range_db }
    }
}

impl LensDatabase for CombinedLensDb {
    fn lookup(&self, lens_id: u16) -> Option<&'static str> {
        // Try static database first
        if let Some(db) = self.static_db {
            if let Some(name) = db.lookup(lens_id) {
                return Some(name);
            }
        }

        // Fall back to range database
        if let Some(db) = self.range_db {
            return db.lookup(lens_id);
        }

        None
    }
}
```

### Step 2: Export from shared module

Add to `src/parsers/tiff/makernotes/shared/mod.rs`:

```rust
pub mod lens_database;

pub use lens_database::{LensDatabase, StaticLensDb, RangeLensDb, CombinedLensDb};
```

### Step 3: Write unit tests

Create `src/parsers/tiff/makernotes/shared/tests/lens_database.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::super::super::lens_database::*;

    const TEST_LENSES: [(u16, &str); 3] = [
        (1, "Test Lens 50mm f/1.8"),
        (2, "Test Lens 85mm f/1.4"),
        (3, "Test Lens 24-70mm f/2.8"),
    ];

    const TEST_RANGES: [(u16, u16, &str); 2] = [
        (100, 105, "Range Lens A"),
        (200, 210, "Range Lens B"),
    ];

    #[test]
    fn test_static_lens_db_lookup() {
        let db = StaticLensDb::new(&TEST_LENSES);

        assert_eq!(db.lookup(1), Some("Test Lens 50mm f/1.8"));
        assert_eq!(db.lookup(2), Some("Test Lens 85mm f/1.4"));
        assert_eq!(db.lookup(99), None);
    }

    #[test]
    fn test_range_lens_db_lookup() {
        let db = RangeLensDb::new(&TEST_RANGES);

        assert_eq!(db.lookup(100), Some("Range Lens A"));
        assert_eq!(db.lookup(103), Some("Range Lens A"));
        assert_eq!(db.lookup(105), Some("Range Lens A"));
        assert_eq!(db.lookup(106), None);
        assert_eq!(db.lookup(205), Some("Range Lens B"));
    }

    #[test]
    fn test_combined_lens_db() {
        let static_db = StaticLensDb::new(&TEST_LENSES);
        let range_db = RangeLensDb::new(&TEST_RANGES);
        let combined = CombinedLensDb::new(Some(&static_db), Some(&range_db));

        // Should find in static DB
        assert_eq!(combined.lookup(1), Some("Test Lens 50mm f/1.8"));

        // Should find in range DB
        assert_eq!(combined.lookup(102), Some("Range Lens A"));

        // Should not find
        assert_eq!(combined.lookup(999), None);
    }

    #[test]
    fn test_lens_db_trait_range_method() {
        let db = StaticLensDb::new(&TEST_LENSES);

        // Range lookup should find lens 2
        assert_eq!(db.lookup_range(1, 5), Some("Test Lens 50mm f/1.8"));
    }
}
```

### Step 4: Add to tests module

Add to `src/parsers/tiff/makernotes/shared/tests/mod.rs`:

```rust
mod lens_database;
```

### Step 5: Run tests

```bash
cargo test -p oxidex lens_database
```

Expect: All tests pass.

### Step 6: Commit

```bash
git add src/parsers/tiff/makernotes/shared/lens_database.rs
git add src/parsers/tiff/makernotes/shared/tests/lens_database.rs
git add src/parsers/tiff/makernotes/shared/mod.rs
git commit -m "feat(parsers): add unified lens database infrastructure"
```

---

## Task 4: Create Example Usage Documentation

**Files:**
- Create: `docs/architecture/parser-shared-infrastructure.md`

### Step 1: Write comprehensive documentation

Create `docs/architecture/parser-shared-infrastructure.md`:

```markdown
# Parser Shared Infrastructure Guide

This document describes the shared infrastructure available for MakerNote parsers in `src/parsers/tiff/makernotes/shared/`.

## Overview

The shared infrastructure provides reusable components that eliminate code duplication across 55+ manufacturer-specific parsers:

- **Generic Decoders**: Pre-built value decoders (ON_OFF, YES_NO, QUALITY, etc.)
- **Decoder Macros**: Declarative macros for creating custom decoders
- **IFD Parser Base**: Shared IFD entry parsing logic
- **Array Extractors**: Generic array extraction functions
- **Array Schemas**: Declarative array index specifications (NEW)
- **Tag Registry**: Centralized tag definition system (ENHANCED)
- **Lens Databases**: Unified lens lookup infrastructure (NEW)

## Array Schemas

### Problem

Many parsers extract CameraSettings arrays with repetitive code:

```rust
// Before: Repetitive and error-prone
if let Some(settings) = extract_i16_array(entry, data, byte_order) {
    if settings.len() > 1 {
        tags.insert("Canon:MacroMode".to_string(), decode_macro_mode(settings[1]));
    }
    if settings.len() > 2 {
        tags.insert("Canon:Quality".to_string(), decode_quality(settings[2]));
    }
    // ... 50-200 more indices
}
```

### Solution

Define array schemas declaratively:

```rust
use super::shared::{ArraySchema, ArrayIndexDef, generic_decoders::*};

const_decoder!(MACRO_MODE, i16, [(1, "Macro"), (2, "Normal")]);
const_decoder!(QUALITY, i16, [(1, "Economy"), (2, "Normal"), (3, "Fine")]);

static CAMERA_SETTINGS: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        ArrayIndexDef::with_i16_decoder(1, "MacroMode", &MACRO_MODE),
        ArrayIndexDef::with_i16_decoder(2, "Quality", &QUALITY),
        ArrayIndexDef::raw(3, "ISO"),
    ],
};

// Usage in parser:
if let Some(settings) = extract_i16_array(entry, data, byte_order) {
    CAMERA_SETTINGS.process_i16_array(&settings, "Canon", tags);
}
```

### Benefits

- **Declarative**: Schema definition is data, not code
- **Compact**: 3 lines replaces 50+ lines of if-statements
- **Type-safe**: Compiler ensures decoder compatibility
- **Maintainable**: Adding indices is trivial

## TagRegistry with Array Support

### Enhanced API

TagRegistry now supports array-based tags:

```rust
let registry = TagRegistry::new()
    .register_simple(0x0001, "Make", &GENERIC_STRING)
    .register_array_schema(0x0002, &CAMERA_SETTINGS);

// Decode simple tag
registry.decode_and_insert(entry, data, byte_order, "Canon", tags);

// Decode array tag
if let Some(array) = extract_i16_array(entry, data, byte_order) {
    registry.decode_array_i16(entry.tag, &array, "Canon", tags);
}
```

## Lens Databases

### Problem

10+ parsers have duplicate lens lookup implementations:

```rust
// Before: Each parser reimplements this
fn lookup_lens(&self, lens_id: u16) -> Option<String> {
    match lens_id {
        1 => Some("Canon EF 50mm f/1.8".to_string()),
        2 => Some("Canon EF 85mm f/1.4".to_string()),
        // ... 500+ more lenses
        _ => None,
    }
}
```

### Solution

Use shared LensDatabase infrastructure:

```rust
use super::shared::{LensDatabase, StaticLensDb};

static CANON_LENSES: [(u16, &str); 500] = [
    (1, "Canon EF 50mm f/1.8"),
    (2, "Canon EF 85mm f/1.4"),
    // ... all lenses
];

static CANON_LENS_DB: StaticLensDb = StaticLensDb::new(&CANON_LENSES);

impl MakerNoteParser for CanonParser {
    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        CANON_LENS_DB.lookup(lens_id).map(|s| s.to_string())
    }
}
```

### Database Types

**StaticLensDb**: For exact ID matches (most manufacturers)

```rust
static DB: StaticLensDb = StaticLensDb::new(&[
    (1, "Lens A"),
    (2, "Lens B"),
]);
```

**RangeLensDb**: For ID ranges

```rust
static DB: RangeLensDb = RangeLensDb::new(&[
    (100, 105, "Lens Family A"),  // IDs 100-105
    (200, 210, "Lens Family B"),  // IDs 200-210
]);
```

**CombinedLensDb**: For both

```rust
static COMBINED: CombinedLensDb = CombinedLensDb::new(
    Some(&STATIC_DB),
    Some(&RANGE_DB),
);
```

## Migration Checklist

When migrating a parser to use shared infrastructure:

- [ ] Replace decoder functions with `const_decoder!` macros
- [ ] Create ArraySchema for CameraSettings-style arrays
- [ ] Use TagRegistry for tag definitions
- [ ] Move lens database to LensDatabase implementation
- [ ] Remove duplicate byte-reading code (use byte_utils)
- [ ] Use IfdParserBase for IFD parsing
- [ ] Add tests verifying functionality unchanged

## Examples

See `src/parsers/tiff/makernotes/shared/tests/` for complete working examples of each pattern.

## Future Enhancements

- Auto-generated documentation from TagRegistry definitions
- Compile-time tag ID collision detection
- Perfect hashing for O(1) tag lookups
- Cross-domain pattern extraction (PNG, PDF, etc.)
```

### Step 2: Commit documentation

```bash
git add docs/architecture/parser-shared-infrastructure.md
git commit -m "docs: add comprehensive guide for parser shared infrastructure"
```

---

## Task 5: Integration Test with Sample Parser

**Files:**
- Create: `src/parsers/tiff/makernotes/shared/tests/integration.rs`

### Step 1: Create integration test demonstrating all features

Create `src/parsers/tiff/makernotes/shared/tests/integration.rs`:

```rust
//! Integration test demonstrating array schemas, TagRegistry, and lens databases working together

#[cfg(test)]
mod tests {
    use super::super::super::{
        array_schemas::*, generic_decoders::*, lens_database::*, tag_registry::*,
    };
    use std::collections::HashMap;

    // Sample decoders
    const_decoder!(MACRO_MODE, i16, [(1, "Macro"), (2, "Normal")]);
    const_decoder!(QUALITY, i16, [(1, "Economy"), (2, "Normal"), (3, "Fine")]);

    // Sample array schema
    static CAMERA_SETTINGS: ArraySchema = ArraySchema {
        name: "CameraSettings",
        indices: &[
            ArrayIndexDef::with_i16_decoder(1, "MacroMode", &MACRO_MODE),
            ArrayIndexDef::with_i16_decoder(2, "Quality", &QUALITY),
            ArrayIndexDef::raw(3, "ISO"),
        ],
    };

    // Sample lens database
    static TEST_LENSES: [(u16, &str); 2] = [
        (1, "Test 50mm f/1.8"),
        (2, "Test 85mm f/1.4"),
    ];

    static LENS_DB: StaticLensDb = StaticLensDb::new(&TEST_LENSES);

    #[test]
    fn test_complete_parser_workflow() {
        // Create registry with array schema
        let registry = TagRegistry::new().register_array_schema(0x0001, &CAMERA_SETTINGS);

        // Simulate CameraSettings array from camera
        let settings = vec![0i16, 2, 3, 400]; // index 0 unused, 1=Normal, 2=Fine, 3=ISO 400

        let mut tags = HashMap::new();

        // Process array using registry
        registry.decode_array_i16(0x0001, &settings, "Test", &mut tags);

        // Verify extracted values
        assert_eq!(
            tags.get("Test:CameraSettings:MacroMode"),
            Some(&"Normal".to_string())
        );
        assert_eq!(
            tags.get("Test:CameraSettings:Quality"),
            Some(&"Fine".to_string())
        );
        assert_eq!(
            tags.get("Test:CameraSettings:ISO"),
            Some(&"400".to_string())
        );

        // Test lens lookup
        assert_eq!(LENS_DB.lookup(1), Some("Test 50mm f/1.8"));
        assert_eq!(LENS_DB.lookup(2), Some("Test 85mm f/1.4"));
        assert_eq!(LENS_DB.lookup(99), None);
    }

    #[test]
    fn test_schema_without_registry() {
        // Array schemas can be used standalone
        let settings = vec![0i16, 1, 2, 800];
        let mut tags = HashMap::new();

        CAMERA_SETTINGS.process_i16_array(&settings, "Standalone", &mut tags);

        assert_eq!(
            tags.get("Standalone:CameraSettings:MacroMode"),
            Some(&"Macro".to_string())
        );
        assert_eq!(
            tags.get("Standalone:CameraSettings:ISO"),
            Some(&"800".to_string())
        );
    }
}
```

### Step 2: Add to tests module

Add to `src/parsers/tiff/makernotes/shared/tests/mod.rs`:

```rust
mod integration;
```

### Step 3: Run integration tests

```bash
cargo test -p oxidex integration
```

Expect: All tests pass.

### Step 4: Commit

```bash
git add src/parsers/tiff/makernotes/shared/tests/integration.rs
git commit -m "test: add integration test for shared infrastructure"
```

---

## Final Verification

Run full test suite and verification:

```bash
# Run all shared infrastructure tests
cargo test -p oxidex shared::tests

# Run clippy
cargo clippy --all-targets -- -D warnings

# Run formatter
cargo fmt --check

# Verify no compilation errors
cargo build -p oxidex
```

Expect:
- All tests pass
- No clippy warnings
- Code is formatted
- Project compiles successfully

---

## Sprint 1 Deliverables

✅ ArraySchema infrastructure with tests
✅ TagRegistry array support with tests
✅ LensDatabase trait and implementations with tests
✅ Comprehensive documentation
✅ Integration test demonstrating all features
✅ All code formatted and linted

**Next Sprint:** Use these foundations to migrate 3-5 pilot parsers (Canon, Nikon, Sony, Apple, Google).
