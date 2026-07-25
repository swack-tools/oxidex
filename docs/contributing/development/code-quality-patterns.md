# Code Quality Patterns

This document describes proven patterns for reducing code complexity and duplication in OxiDex, with concrete examples from past refactoring efforts.

## Table-Driven Design

### When to Use

Use table-driven design when you have:
- Multiple conditional checks with similar structure
- Pattern matching on magic bytes or signatures
- Mappings from values to strings or other values

### Format Detector Example

The format detector was refactored from 100+ if-else blocks to a table-driven approach:

**Before** (high duplication):
```rust
if magic_bytes.len() >= 4 && &magic_bytes[0..4] == b"fLaC" {
    return Ok(FileFormat::FLAC);
}
if magic_bytes.len() >= 3 && &magic_bytes[0..3] == b"ID3" {
    return Ok(FileFormat::MP3);
}
// ... 50+ more similar blocks
```

**After** (table-driven):
```rust
static SIMPLE_SIGNATURES: &[Signature] = &[
    signature!(b"fLaC", 0, FileFormat::FLAC),
    signature!(b"ID3", 0, FileFormat::MP3),
    // All signatures in one place
];

fn detect_format(data: &[u8]) -> Option<FileFormat> {
    for sig in SIMPLE_SIGNATURES {
        if matches_at_offset(data, sig.bytes, sig.offset) {
            return Some(sig.format);
        }
    }
    None
}
```

**Results**:
- Complexity reduced 35% (214 → 140)
- Duplication reduced 58% (83% → 35%)
- Adding new formats: 1 line instead of 5+

### Implementation Tips

1. Create a struct for the table entries:
   ```rust
   struct Signature {
       bytes: &'static [u8],
       offset: usize,
       format: FileFormat,
   }
   ```

2. Use macros for ergonomic table construction:
   ```rust
   macro_rules! signature {
       ($bytes:expr, $offset:expr, $format:expr) => {
           Signature { bytes: $bytes, offset: $offset, format: $format }
       };
   }
   ```

3. Extract complex cases to specialized functions:
   ```rust
   // Table handles simple cases
   // Functions handle complex format families
   fn detect_tiff_variants(data: &[u8]) -> Option<FileFormat> { ... }
   fn detect_bmff_variants(data: &[u8]) -> Option<FileFormat> { ... }
   ```

## Const Decoder Pattern

### When to Use

Use const decoders when you have:
- Value-to-string mappings (enums, settings)
- Repeated match statements with similar structure
- On/Off, Yes/No, or similar binary values

### const_decoder! Macro

The `const_decoder!` macro creates compile-time decoder tables:

```rust
const_decoder!(pub DECODE_FOV, i16, [
    (0, "Wide"),
    (1, "Medium"),
    (2, "Narrow"),
    (3, "Linear"),
    (4, "SuperView"),
]);
```

Usage:
```rust
let value: i16 = 2;
let decoded = DECODE_FOV.decode(value);  // Returns "Narrow"
```

### GoPro Parser Example

The GoPro parser was refactored from 31 decoder functions to 8:

**Before** (23 repetitive functions):
```rust
fn decode_fov(value: i16) -> String {
    match value {
        0 => "Wide".to_string(),
        1 => "Medium".to_string(),
        2 => "Narrow".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

fn decode_white_balance(value: i16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "3000K".to_string(),
        // ... more entries
        _ => format!("Unknown ({})", value),
    }
}
// ... 21 more similar functions
```

**After** (14 const decoders):
```rust
const_decoder!(FOV, i16, [
    (0, "Wide"),
    (1, "Medium"),
    (2, "Narrow"),
    (3, "Linear"),
    (4, "SuperView"),
]);

const_decoder!(WHITE_BALANCE, i16, [
    (0, "Auto"),
    (1, "3000K"),
    // ... compact, readable
]);
```

**Results**:
- File size reduced 14% (844 → 721 lines)
- Functions reduced 74% (31 → 8)
- Duplication reduced from 181% to <50%

### Shared Decoders

Common patterns are provided as pre-built decoders:

```rust
// In generic_decoders.rs
pub const ON_OFF: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "On"),
]);

pub const YES_NO: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "No"),
    (1, "Yes"),
]);

pub const ENABLED_DISABLED: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Disabled"),
    (1, "Enabled"),
]);
```

### Implementation Tips

1. **Use shared decoders when possible**:
   ```rust
   // Instead of creating a new decoder
   const_decoder!(MY_ON_OFF, i16, [(0, "Off"), (1, "On")]);

   // Use the shared one
   use crate::parsers::tiff::makernotes::shared::generic_decoders::ON_OFF;
   ```

2. **Keep custom formatters for calculations**:
   ```rust
   // Decoders are for simple mappings
   // Functions are for calculations
   fn format_exposure(value: i16) -> String {
       let ev = value as f32 / 10.0;
       format!("{:+.1} EV", ev)
   }
   ```

3. **Document decoder values**:
   ```rust
   // White Balance decoder - Temperature presets and modes
   const_decoder!(WHITE_BALANCE, i16, [
       (0, "Auto"),      // Automatic white balance
       (1, "3000K"),     // Tungsten
       (2, "5500K"),     // Daylight
       (3, "6500K"),     // Cloudy
   ]);
   ```

## Helper Function Extraction

### When to Use

Extract helper functions when you see:
- Repeated byte manipulation patterns
- Common bounds checking logic
- Duplicated error handling

### Example: Offset Matching

**Before** (duplicated 15+ times):
```rust
if data.len() >= offset + pattern.len() {
    if &data[offset..offset + pattern.len()] == pattern {
        // matched
    }
}
```

**After** (single helper):
```rust
#[inline]
fn matches_at_offset(data: &[u8], pattern: &[u8], offset: usize) -> bool {
    if offset + pattern.len() > data.len() {
        return false;
    }
    &data[offset..offset + pattern.len()] == pattern
}
```

### Benefits

1. **Single point of change**: Fix bugs in one place
2. **Better testing**: Test the helper in isolation
3. **Documentation**: One place to document edge cases
4. **Performance**: Easier to optimize with `#[inline]`

## Measuring Success

### Metrics to Track

1. **Cyclomatic Complexity**: Number of decision points
   - Target: < 10 per function, < 100 per module

2. **Code Duplication**: Percentage of duplicated lines
   - Target: < 40%

3. **Function Count**: Number of functions
   - Fewer functions with same coverage = less duplication

4. **Lines of Code**: Raw line count
   - Reduction indicates consolidation

### Tools

```bash
# Measure complexity with tokei
tokei src/parsers/format_detector.rs

# Check for clippy warnings
cargo clippy -p oxidex

# Run tests to ensure no regressions
cargo test --workspace
```

## Common Anti-Patterns

### 1. Decoder Function Explosion

**Problem**: One function per value mapping.

**Solution**: Use `const_decoder!` macro.

### 2. Deep Nesting

**Problem**: Multiple levels of if/else or match.

**Solution**: Extract to helper functions, use early returns.

### 3. Copy-Paste Parsing

**Problem**: Same parsing logic repeated for each tag.

**Solution**: Use `TagRegistry` with declarative definitions.

### 4. Inline Magic Numbers

**Problem**: Byte offsets and constants scattered in code.

**Solution**: Define named constants, use struct/enum types.

## References

- [TagRegistry Refactoring Project](/contributing/development/tagregistry-refactoring) - Large-scale application of these patterns
- [Parser Migration Guide](/architecture/parser-migration-guide) - How to migrate parsers to TagRegistry
