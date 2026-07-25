# Parser Migration Guide

This guide documents how to migrate a MakerNote parser to use TagRegistry + ArraySchema infrastructure.

## Before You Start

Review the shared infrastructure guide: `docs/architecture/parser-shared-infrastructure.md`

## Migration Steps

### 1. Analyze Current Parser

Identify:
- Decoder functions (const_decoder! macros or inline functions)
- Array-based tags (CameraSettings, ShotInfo, etc.)
- Lens database implementation (if any)
- Repetitive if-statement patterns in parse() method

### 2. Create Registry Module

Create `src/parsers/tiff/makernotes/registries/<manufacturer>.rs`:

```rust
use super::super::shared::{array_schemas::*, tag_registry::TagRegistry};
use super::super::<manufacturer>::*; // Import existing decoders

static ARRAY_SCHEMA: ArraySchema = ArraySchema {
    name: "SchemaName",
    indices: &[
        ArrayIndexDef::with_i16_decoder(1, "Field1", &DECODER1),
        ArrayIndexDef::raw(2, "Field2"),
    ],
};

pub fn <manufacturer>_registry() -> TagRegistry {
    TagRegistry::new()
        .register_array_schema(0x0001, &ARRAY_SCHEMA)
}
```

### 3. Migrate Lens Database (if applicable)

Convert HashMap to StaticLensDb:

```rust
static LENS_DATA: [(u16, &str); N] = [ /* existing data */ ];
static LENS_DB: StaticLensDb = StaticLensDb::new(&LENS_DATA);

pub fn get_lens_database() -> &'static impl LensDatabase {
    &LENS_DB
}
```

### 4. Update Parser Implementation

Replace repetitive if-statements with registry calls:

```rust
let registry = <manufacturer>_registry();

match entry.tag_id {
    ARRAY_TAG => {
        if let Some(array) = extract_i16_array(entry, data, byte_order) {
            registry.decode_array_i16(ARRAY_TAG, &array, "Prefix", tags);
        }
    }
}
```

### 5. Run Tests

```bash
cargo test -p oxidex <manufacturer>
```

Verify all existing tests pass.

### 6. Measure Reduction

```bash
wc -l src/parsers/tiff/makernotes/<manufacturer>.rs  # Before
wc -l src/parsers/tiff/makernotes/<manufacturer>.rs  # After
```

### 7. Commit

```bash
git commit -m "refactor(parsers): migrate <Manufacturer> parser to TagRegistry + ArraySchema

- Reduce parser from X to Y lines (Z line reduction)
- All existing tests pass"
```

## Pilot Migration Results

| Parser | Before | After | Reduction | % |
|--------|--------|-------|-----------|---|
| Canon  | 1,345  | 1,165 | 180 lines | 13% |
| Nikon  | 792    | 642   | 150 lines | 19% |
| Sony   | 1,113  | 943   | 170 lines | 15% |
| Apple  | 558    | 458   | 100 lines | 18% |
| Google | 566    | 461   | 105 lines | 19% |

## Common Patterns

### Pattern 1: Array with Lens Lookup

```rust
pub fn process_file_info_with_lens(
    array: &[i16],
    prefix: &str,
    lens_db: &impl LensDatabase,
    tags: &mut HashMap<String, String>,
) {
    SCHEMA.process_i16_array(array, prefix, tags);

    if let Some(&lens_id) = array.get(LENS_INDEX) {
        if let Some(name) = lens_db.lookup(lens_id as u16) {
            tags.insert(format!("{}:LensID", prefix), name.to_string());
        }
    }
}
```

### Pattern 2: Multiple Array Schemas

```rust
static SCHEMA_1: ArraySchema = ArraySchema { /* ... */ };
static SCHEMA_2: ArraySchema = ArraySchema { /* ... */ };

pub fn registry() -> TagRegistry {
    TagRegistry::new()
        .register_array_schema(0x0001, &SCHEMA_1)
        .register_array_schema(0x0002, &SCHEMA_2)
}
```

### Pattern 3: Shared Decoders

Re-export decoders from original parser:

```rust
// In registries/<manufacturer>.rs
use super::super::<manufacturer>::{DECODER1, DECODER2};
```

Keep decoder definitions in original file for now; future refactoring can move them.
