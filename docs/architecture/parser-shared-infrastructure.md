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
