# Universal MakerNotes Framework Implementation Plan

**Date**: 2025-11-16
**Status**: Planning Complete - Ready for Implementation
**Estimated Effort**: 6 phases, ~3-4 weeks per phase

## Overview

This plan outlines the implementation of a comprehensive Universal MakerNotes Framework that will parse metadata from **ALL 221 modules** including:
- **31 device manufacturers** (cameras, smartphones, drones, thermal/cinema)
- **8 software applications** (Photoshop, CaptureOne, GIMP, etc.)
- **Shared utilities** for maximum code reuse

**Current Status**: Canon MakerNotes complete ✅ (1,289 lines, ~7,379 tags)

**Goal**: Parse ~47,406+ MakerNotes tags across all manufacturers while reducing per-manufacturer code from ~1,200 lines to ~500-800 lines through shared utilities.

---

## Architecture Summary

### Core Design Principles

1. **Hybrid Architecture**: Common trait + shared utilities + manufacturer-specific parsers
2. **Maximum Code Reuse**: Extract common patterns (array extraction, value decoding, byte utilities)
3. **Market Share Priority**: Implement high-impact manufacturers first
4. **Incremental Delivery**: Each phase delivers working parsers before moving to next
5. **Quality Gates**: 3-tier testing (unit tests, integration tests, golden file regression)

### Module Structure

```
src/parsers/tiff/makernotes/
├── mod.rs                           # Registry of all parsers
├── canon.rs                         ✅ Complete (1,289 lines)
├── canon_lens_database.rs           ✅ Complete (231 lines)
│
├── shared/                          # NEW: Shared utilities module
│   ├── mod.rs
│   ├── makernote_parser.rs          # Common trait definition
│   ├── array_extractors.rs          # Array extraction utilities
│   ├── value_decoders.rs            # Common value interpretation
│   └── byte_utils.rs                # Low-level parsing helpers
│
├── nikon.rs                         # Phase 1
├── nikon_lens_database.rs           # Phase 1
├── sony.rs                          # Phase 1
├── sony_lens_database.rs            # Phase 1
├── fujifilm.rs                      # Phase 1
├── fuji_lens_database.rs            # Phase 1
├── panasonic.rs                     # Phase 1
├── panasonic_lens_database.rs       # Phase 1
│
├── olympus.rs                       # Phase 2
├── olympus_lens_database.rs         # Phase 2
├── pentax.rs                        # Phase 2
├── pentax_lens_database.rs          # Phase 2
├── leica.rs                         # Phase 2
├── leica_lens_database.rs           # Phase 2
├── sigma.rs                         # Phase 2
├── sigma_lens_database.rs           # Phase 2
├── phaseone.rs                      # Phase 2
├── phaseone_lens_database.rs        # Phase 2
│
├── apple.rs                         # Phase 3 (iPhone)
├── google.rs                        # Phase 3 (Pixel)
├── samsung.rs                       # Phase 3
├── microsoft.rs                     # Phase 3 (Lumia)
├── qualcomm.rs                      # Phase 3
│
├── minolta.rs                       # Phase 4 (Legacy)
├── kodak.rs                         # Phase 4
├── casio.rs                         # Phase 4
├── ricoh.rs                         # Phase 4
├── hp.rs                            # Phase 4
├── sanyo.rs                         # Phase 4
├── jvc.rs                           # Phase 4
├── motorola.rs                      # Phase 4
├── ge.rs                            # Phase 4
├── leaf.rs                          # Phase 4
│
├── dji.rs                           # Phase 5 (Specialty)
├── gopro.rs                         # Phase 5
├── flir.rs                          # Phase 5
├── infrared.rs                      # Phase 5
├── red.rs                           # Phase 5
├── parrot.rs                        # Phase 5
├── reconyx.rs                       # Phase 5
├── lytro.rs                         # Phase 5
├── nintendo.rs                      # Phase 5
│
├── photoshop.rs                     # Phase 6 (Software)
├── captureone.rs                    # Phase 6
├── nikoncapture.rs                  # Phase 6
├── photomechanic.rs                 # Phase 6
├── fotostation.rs                   # Phase 6
├── gimp.rs                          # Phase 6
├── scalado.rs                       # Phase 6
├── indesign.rs                      # Phase 6
│
└── shared_lens_database.rs          # Third-party lenses (Tamron, Tokina, etc.)
```

---

## Phase 0: Shared Utilities Foundation

**Goal**: Create reusable utilities that all 221 modules will leverage.

### Task 1: Create Shared Module Structure

**File**: `src/parsers/tiff/makernotes/shared/mod.rs`

```rust
//! Shared utilities for MakerNotes parsing
//!
//! This module provides common functionality used across all manufacturer
//! parsers to maximize code reuse and reduce duplication.

pub mod makernote_parser;
pub mod array_extractors;
pub mod value_decoders;
pub mod byte_utils;

pub use makernote_parser::MakerNoteParser;
```

### Task 2: Define MakerNoteParser Trait

**File**: `src/parsers/tiff/makernotes/shared/makernote_parser.rs`

```rust
use std::collections::HashMap;
use crate::parsers::tiff::ByteOrder;

/// Common trait for all MakerNotes parsers
///
/// Each manufacturer implements this trait to provide consistent
/// parsing interface across all brands.
pub trait MakerNoteParser {
    /// Returns the manufacturer identifier (e.g., "Canon", "Nikon", "Apple")
    fn manufacturer_name(&self) -> &'static str;

    /// Returns the tag namespace prefix (e.g., "Canon:", "Nikon:", "Apple:")
    fn tag_prefix(&self) -> &'static str;

    /// Parse MakerNote data and extract tags
    ///
    /// # Arguments
    /// * `data` - Raw MakerNote data bytes
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    ///
    /// # Returns
    /// Ok(()) on success, Err(message) on failure
    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>
    ) -> Result<(), String>;

    /// Optional: Validate that this data belongs to this manufacturer
    ///
    /// Some manufacturers have header signatures (e.g., "Nikon\0\0")
    /// Default implementation accepts all data.
    fn validate_header(&self, data: &[u8]) -> bool {
        let _ = data; // Suppress unused parameter warning
        true
    }

    /// Optional: Lens database lookup (if manufacturer has lens IDs)
    ///
    /// Returns lens name for given lens ID, or None if:
    /// - Manufacturer doesn't use lens IDs
    /// - Lens ID not found in database
    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        let _ = lens_id;
        None
    }
}
```

**Verification**:
```bash
cargo build
cargo test
```

### Task 3: Implement Array Extractors

**File**: `src/parsers/tiff/makernotes/shared/array_extractors.rs`

```rust
use crate::parsers::tiff::{ByteOrder, IFDEntry};

/// Extract i16 array from IFD entry
///
/// Used by: Canon CameraSettings, Nikon ShotInfo, Sony CameraSettings
pub fn extract_i16_array(
    entry: &IFDEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<i16>> {
    if entry.count == 0 {
        return None;
    }

    let offset = entry.value_or_offset as usize;
    if offset + (entry.count as usize * 2) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.count as usize);
    for i in 0..entry.count {
        let pos = offset + (i as usize * 2);
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                i16::from_le_bytes([data[pos], data[pos + 1]])
            }
            ByteOrder::BigEndian => {
                i16::from_be_bytes([data[pos], data[pos + 1]])
            }
        };
        array.push(value);
    }

    Some(array)
}

/// Extract u16 array from IFD entry
///
/// Used by: Nikon LensData, Sony AFInfo, Fuji FaceDetection
pub fn extract_u16_array(
    entry: &IFDEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<u16>> {
    if entry.count == 0 {
        return None;
    }

    let offset = entry.value_or_offset as usize;
    if offset + (entry.count as usize * 2) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.count as usize);
    for i in 0..entry.count {
        let pos = offset + (i as usize * 2);
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                u16::from_le_bytes([data[pos], data[pos + 1]])
            }
            ByteOrder::BigEndian => {
                u16::from_be_bytes([data[pos], data[pos + 1]])
            }
        };
        array.push(value);
    }

    Some(array)
}

/// Extract u32 array from IFD entry
///
/// Used by: Canon FileInfo, Nikon ShutterData, Pentax CameraInfo
pub fn extract_u32_array(
    entry: &IFDEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<u32>> {
    if entry.count == 0 {
        return None;
    }

    let offset = entry.value_or_offset as usize;
    if offset + (entry.count as usize * 4) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.count as usize);
    for i in 0..entry.count {
        let pos = offset + (i as usize * 4);
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            }
            ByteOrder::BigEndian => {
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            }
        };
        array.push(value);
    }

    Some(array)
}

/// Extract i32 array from IFD entry
///
/// Used by: Olympus CameraSettings, Panasonic WBInfo
pub fn extract_i32_array(
    entry: &IFDEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<i32>> {
    if entry.count == 0 {
        return None;
    }

    let offset = entry.value_or_offset as usize;
    if offset + (entry.count as usize * 4) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.count as usize);
    for i in 0..entry.count {
        let pos = offset + (i as usize * 4);
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                i32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            }
            ByteOrder::BigEndian => {
                i32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            }
        };
        array.push(value);
    }

    Some(array)
}

/// Extract rational (u32/u32) array from IFD entry
///
/// Used by: GPS coordinates, exposure times, focal lengths
pub fn extract_rational_array(
    entry: &IFDEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<(u32, u32)>> {
    if entry.count == 0 {
        return None;
    }

    let offset = entry.value_or_offset as usize;
    if offset + (entry.count as usize * 8) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.count as usize);
    for i in 0..entry.count {
        let pos = offset + (i as usize * 8);
        let (numerator, denominator) = match byte_order {
            ByteOrder::LittleEndian => {
                let num = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                let den = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
                (num, den)
            }
            ByteOrder::BigEndian => {
                let num = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                let den = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
                (num, den)
            }
        };
        array.push((numerator, denominator));
    }

    Some(array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_i16_array_big_endian() {
        let data = vec![0x00, 0x0A, 0x00, 0x14, 0xFF, 0xF6]; // [10, 20, -10]
        let entry = IFDEntry {
            tag: 0x0001,
            field_type: 3,
            count: 3,
            value_or_offset: 0,
        };

        let result = extract_i16_array(&entry, &data, ByteOrder::BigEndian);
        assert_eq!(result, Some(vec![10, 20, -10]));
    }

    #[test]
    fn test_extract_u32_array_little_endian() {
        let data = vec![0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00]; // [1, 2]
        let entry = IFDEntry {
            tag: 0x0001,
            field_type: 4,
            count: 2,
            value_or_offset: 0,
        };

        let result = extract_u32_array(&entry, &data, ByteOrder::LittleEndian);
        assert_eq!(result, Some(vec![1, 2]));
    }
}
```

**Verification**:
```bash
cargo test shared::array_extractors
```

### Task 4: Implement Value Decoders

**File**: `src/parsers/tiff/makernotes/shared/value_decoders.rs`

```rust
/// Decode APEX exposure time value to human-readable string
///
/// Used by: Canon, Nikon, Sony, Pentax
/// Formula: exposure_time = 1 / (2^value)
pub fn decode_exposure_time(apex_value: i32) -> String {
    if apex_value == 0 {
        return "1 s".to_string();
    }

    let divisor = 2_f64.powi(apex_value);
    format!("1/{} s", divisor.round() as i32)
}

/// Decode APEX aperture value to f-number
///
/// Used by: Canon, Nikon, Sony, Olympus, Fuji
/// Formula: f_number = sqrt(2^value)
pub fn decode_aperture(apex_value: i32) -> String {
    let f_number = 2_f64.powf(apex_value as f64 / 2.0);
    format!("f/{:.1}", f_number)
}

/// Decode ISO value from various encoding schemes
///
/// Used by: All manufacturers (different encodings)
pub fn decode_iso(value: i32) -> String {
    // Common encodings:
    // - Direct value (100, 200, 400, etc.)
    // - Log2 encoding (value = log2(ISO))
    // - Manufacturer-specific offsets

    if value < 16 {
        // Likely log2 encoding
        let iso = 2_i32.pow(value as u32);
        format!("ISO {}", iso)
    } else {
        // Direct value
        format!("ISO {}", value)
    }
}

/// Decode focal length from numerator/denominator
///
/// Used by: All manufacturers
pub fn decode_focal_length(numerator: i32, denominator: i32) -> String {
    if denominator == 0 {
        return "Unknown".to_string();
    }

    let focal_length = numerator as f64 / denominator as f64;
    format!("{:.1} mm", focal_length)
}

/// Decode color temperature in Kelvin
///
/// Used by: Canon, Nikon, Sony (white balance)
pub fn decode_temperature_kelvin(kelvin: i32) -> String {
    format!("{} K", kelvin)
}

/// Decode GPS coordinate from degrees, minutes, seconds
///
/// Used by: All manufacturers with GPS
pub fn decode_gps_coord(degrees: u32, minutes: u32, seconds: u32) -> f64 {
    degrees as f64 + (minutes as f64 / 60.0) + (seconds as f64 / 3600.0)
}

/// Decode Unix timestamp to ISO 8601 string
///
/// Used by: Apple, Google, Samsung (smartphone metadata)
pub fn decode_timestamp(unix_timestamp: u32) -> String {
    // Convert Unix timestamp to human-readable format
    // For now, return as-is; can enhance with chrono crate later
    format!("Timestamp: {}", unix_timestamp)
}

/// Decode flash mode from common values
///
/// Used by: Most manufacturers
pub fn decode_flash_mode(value: u16) -> &'static str {
    match value {
        0 => "No Flash",
        1 => "Flash Fired",
        5 => "Flash Fired, Return not detected",
        7 => "Flash Fired, Return detected",
        9 => "Flash Fired, Compulsory",
        13 => "Flash Fired, Compulsory, Return not detected",
        15 => "Flash Fired, Compulsory, Return detected",
        16 => "No Flash, Compulsory",
        24 => "No Flash, Auto",
        25 => "Flash Fired, Auto",
        29 => "Flash Fired, Auto, Return not detected",
        31 => "Flash Fired, Auto, Return detected",
        32 => "No Flash Available",
        _ => "Unknown Flash Mode",
    }
}

/// Decode white balance from common values
///
/// Used by: Most manufacturers
pub fn decode_white_balance(value: u16) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Daylight",
        2 => "Cloudy",
        3 => "Tungsten",
        4 => "Fluorescent",
        5 => "Flash",
        6 => "Custom",
        7 => "Shade",
        8 => "Kelvin",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_exposure_time() {
        assert_eq!(decode_exposure_time(0), "1 s");
        assert_eq!(decode_exposure_time(3), "1/8 s");
        assert_eq!(decode_exposure_time(10), "1/1024 s");
    }

    #[test]
    fn test_decode_aperture() {
        assert_eq!(decode_aperture(2), "f/1.4");
        assert_eq!(decode_aperture(4), "f/2.0");
        assert_eq!(decode_aperture(8), "f/4.0");
    }

    #[test]
    fn test_decode_flash_mode() {
        assert_eq!(decode_flash_mode(0), "No Flash");
        assert_eq!(decode_flash_mode(1), "Flash Fired");
        assert_eq!(decode_flash_mode(25), "Flash Fired, Auto");
    }
}
```

**Verification**:
```bash
cargo test shared::value_decoders
```

### Task 5: Implement Byte Utilities

**File**: `src/parsers/tiff/makernotes/shared/byte_utils.rs`

```rust
use crate::parsers::tiff::ByteOrder;

/// Read u16 from byte slice at offset
///
/// Returns None if offset is out of bounds
pub fn read_u16(data: &[u8], offset: usize, byte_order: ByteOrder) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }

    let value = match byte_order {
        ByteOrder::LittleEndian => {
            u16::from_le_bytes([data[offset], data[offset + 1]])
        }
        ByteOrder::BigEndian => {
            u16::from_be_bytes([data[offset], data[offset + 1]])
        }
    };

    Some(value)
}

/// Read i16 from byte slice at offset
pub fn read_i16(data: &[u8], offset: usize, byte_order: ByteOrder) -> Option<i16> {
    if offset + 2 > data.len() {
        return None;
    }

    let value = match byte_order {
        ByteOrder::LittleEndian => {
            i16::from_le_bytes([data[offset], data[offset + 1]])
        }
        ByteOrder::BigEndian => {
            i16::from_be_bytes([data[offset], data[offset + 1]])
        }
    };

    Some(value)
}

/// Read u32 from byte slice at offset
pub fn read_u32(data: &[u8], offset: usize, byte_order: ByteOrder) -> Option<u32> {
    if offset + 4 > data.len() {
        return None;
    }

    let value = match byte_order {
        ByteOrder::LittleEndian => {
            u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        }
        ByteOrder::BigEndian => {
            u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        }
    };

    Some(value)
}

/// Read ASCII string from byte slice
///
/// Reads up to `length` bytes or until null terminator
pub fn read_ascii_string(data: &[u8], offset: usize, length: usize) -> Option<String> {
    if offset + length > data.len() {
        return None;
    }

    let bytes = &data[offset..offset + length];
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(length);

    String::from_utf8(bytes[..end].to_vec()).ok()
}

/// Parse null-terminated string from beginning of slice
pub fn parse_null_terminated_string(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u16_big_endian() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        assert_eq!(read_u16(&data, 0, ByteOrder::BigEndian), Some(0x0102));
        assert_eq!(read_u16(&data, 2, ByteOrder::BigEndian), Some(0x0304));
        assert_eq!(read_u16(&data, 3, ByteOrder::BigEndian), None); // Out of bounds
    }

    #[test]
    fn test_read_ascii_string() {
        let data = b"Hello\0World";
        assert_eq!(
            read_ascii_string(data, 0, 11),
            Some("Hello".to_string())
        );
    }

    #[test]
    fn test_parse_null_terminated_string() {
        let data = b"Test\0Ignored";
        assert_eq!(parse_null_terminated_string(data), "Test");
    }
}
```

**Verification**:
```bash
cargo test shared::byte_utils
```

### Task 6: Register Shared Module

**File**: `src/parsers/tiff/makernotes/mod.rs`

Add after `pub mod canon_lens_database;`:

```rust
pub mod shared;
```

**Verification**:
```bash
cargo build
cargo clippy
cargo test
```

### Task 7: Refactor Canon to Use Shared Utilities

**Refactoring Goal**: Demonstrate shared utilities by converting Canon's `extract_i16_array` to use `shared::array_extractors::extract_i16_array`.

**File**: `src/parsers/tiff/makernotes/canon.rs`

Change:
```rust
use super::canon_lens_database::lookup_lens_name;
```

To:
```rust
use super::canon_lens_database::lookup_lens_name;
use super::shared::array_extractors::{extract_i16_array, extract_u32_array};
use super::shared::value_decoders;
```

Remove Canon's local `extract_i16_array` function and use the shared version.

**Verification**:
```bash
cargo test canon
cargo test canon_makernotes_phase3
```

All Canon tests must still pass with shared utilities.

---

## Phase 1: Top 5 Camera Manufacturers

**Goal**: Implement Nikon, Sony, Fujifilm, Panasonic (Canon already complete ✅)

### Manufacturer 1: Nikon (~6,500 tags)

#### Task 1.1: Research Nikon MakerNotes Structure

**Action**: Study Nikon tag structure from ExifTool documentation
- Tag IDs (e.g., 0x0001 = Version, 0x0002 = ISOSpeed, 0x0011 = Preview)
- Array tags (ShotInfo, ColorBalance, LensData, etc.)
- Nikon has TWO formats: Type 2 (IFD-based) and Type 3 (encrypted)
- Header signature: "Nikon\0" (6 bytes)

#### Task 1.2: Create Nikon Lens Database

**File**: `src/parsers/tiff/makernotes/nikon_lens_database.rs`

```rust
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Lookup lens name by Nikon lens ID
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    NIKON_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

static NIKON_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // F-mount lenses
    db.insert(119, "Nikon AF-S DX 18-55mm f/3.5-5.6G VR");
    db.insert(120, "Nikon AF-S DX 18-55mm f/3.5-5.6G VR II");
    db.insert(127, "Nikon AF-S DX 18-105mm f/3.5-5.6G ED VR");
    db.insert(139, "Nikon AF-S DX 18-300mm f/3.5-5.6G ED VR");
    db.insert(147, "Nikon AF-S 24-70mm f/2.8G ED");
    db.insert(148, "Nikon AF-S 24-120mm f/4G ED VR");
    db.insert(154, "Nikon AF-S 70-200mm f/2.8G ED VR II");
    db.insert(161, "Nikon AF-S 35mm f/1.8G");
    db.insert(162, "Nikon AF-S 50mm f/1.8G");
    db.insert(163, "Nikon AF-S 85mm f/1.8G");

    // Z-mount lenses
    db.insert(174, "Nikkor Z 24-70mm f/4 S");
    db.insert(175, "Nikkor Z 14-30mm f/4 S");
    db.insert(176, "Nikkor Z 35mm f/1.8 S");
    db.insert(177, "Nikkor Z 50mm f/1.8 S");
    db.insert(178, "Nikkor Z 24-70mm f/2.8 S");
    db.insert(179, "Nikkor Z 70-200mm f/2.8 VR S");

    // ... Add ~150 total Nikon lenses

    db
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nikon_lens_lookup() {
        assert_eq!(
            lookup_lens_name(147),
            Some("Nikon AF-S 24-70mm f/2.8G ED".to_string())
        );
        assert_eq!(
            lookup_lens_name(177),
            Some("Nikkor Z 50mm f/1.8 S".to_string())
        );
        assert_eq!(lookup_lens_name(9999), None);
    }
}
```

#### Task 1.3: Implement Nikon Parser

**File**: `src/parsers/tiff/makernotes/nikon.rs`

```rust
use std::collections::HashMap;
use crate::parsers::tiff::{ByteOrder, IFDEntry};
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use super::shared::value_decoders;
use super::shared::byte_utils;
use super::shared::MakerNoteParser;
use super::nikon_lens_database::lookup_lens_name;

// Nikon MakerNotes tag constants
const NIKON_VERSION: u16 = 0x0001;
const NIKON_ISO_SPEED: u16 = 0x0002;
const NIKON_COLOR_MODE: u16 = 0x0003;
const NIKON_QUALITY: u16 = 0x0004;
const NIKON_WHITE_BALANCE: u16 = 0x0005;
const NIKON_SHARPNESS: u16 = 0x0006;
const NIKON_FOCUS_MODE: u16 = 0x0007;
const NIKON_FLASH_SETTING: u16 = 0x0008;
const NIKON_SHOT_INFO: u16 = 0x0091;  // Array tag
const NIKON_COLOR_BALANCE: u16 = 0x0097;  // Array tag
const NIKON_LENS_DATA: u16 = 0x0098;  // Array tag with lens ID
const NIKON_SERIAL_NUMBER: u16 = 0x001D;
const NIKON_SHUTTER_COUNT: u16 = 0x00A7;

pub struct NikonParser;

impl MakerNoteParser for NikonParser {
    fn manufacturer_name(&self) -> &'static str {
        "Nikon"
    }

    fn tag_prefix(&self) -> &'static str {
        "Nikon:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Nikon Type 2/3 header: "Nikon\0"
        data.len() >= 6 && &data[0..6] == b"Nikon\0"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>
    ) -> Result<(), String> {
        // Validate header
        if !self.validate_header(data) {
            return Err("Invalid Nikon MakerNote header".to_string());
        }

        // Parse IFD entries (starting after "Nikon\0" header)
        // Implementation similar to Canon parser but with Nikon-specific tags

        // TODO: Implement full Nikon tag parsing
        // This is a placeholder showing the structure

        Ok(())
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

/// Public function to parse Nikon MakerNotes
pub fn parse_nikon_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = NikonParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("Nikon MakerNotes parse error: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nikon_header_validation() {
        let parser = NikonParser;

        let valid_header = b"Nikon\0\x02\x10\x00\x00";
        assert!(parser.validate_header(valid_header));

        let invalid_header = b"Canon\0\x00\x00";
        assert!(!parser.validate_header(invalid_header));
    }
}
```

**Note**: Full Nikon implementation would be ~800 lines. This shows the structure.

#### Task 1.4: Register Nikon Module

**File**: `src/parsers/tiff/makernotes/mod.rs`

Add:
```rust
pub mod nikon;
pub mod nikon_lens_database;
```

#### Task 1.5: Create Nikon Integration Tests

**File**: `tests/integration/nikon_makernotes_tests.rs`

```rust
#[test]
fn test_nikon_lens_database_integration() {
    use oxidex::parsers::tiff::makernotes::nikon_lens_database::lookup_lens_name;

    assert_eq!(
        lookup_lens_name(147),
        Some("Nikon AF-S 24-70mm f/2.8G ED".to_string())
    );
    assert_eq!(
        lookup_lens_name(177),
        Some("Nikkor Z 50mm f/1.8 S".to_string())
    );
}

#[test]
fn test_nikon_parser_trait() {
    use oxidex::parsers::tiff::makernotes::nikon::NikonParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = NikonParser;
    assert_eq!(parser.manufacturer_name(), "Nikon");
    assert_eq!(parser.tag_prefix(), "Nikon:");
}
```

**Register in** `tests/integration.rs`:
```rust
#[path = "integration/nikon_makernotes_tests.rs"]
mod nikon_makernotes_tests;
```

#### Task 1.6: Verification

```bash
cargo build
cargo test nikon
cargo clippy
cargo fmt
```

---

### Remaining Phase 1 Manufacturers

**Sony, Fujifilm, Panasonic** follow the same pattern as Nikon:

1. Research tag structure
2. Create lens database
3. Implement parser (using shared utilities)
4. Register module
5. Create integration tests
6. Verify with cargo test/clippy/fmt

**Estimated effort per manufacturer**: 1-2 days

---

## Phase 2-6: Abbreviated Structure

Due to length constraints, Phases 2-6 follow the same pattern:

### Phase 2: Mid-Tier Cameras
- Olympus, Pentax, Leica, Sigma, PhaseOne
- Same structure as Phase 1

### Phase 3: Smartphones
- Apple (iPhone HEIC/JPG metadata)
- Google (Pixel HDR+ metadata)
- Samsung, Microsoft, Qualcomm
- No lens databases needed

### Phase 4: Legacy Brands
- Minolta, Kodak, Casio, Ricoh, HP, Sanyo, JVC, Motorola, GE, Leaf
- Simpler parsers (~300-500 lines each)

### Phase 5: Specialty Devices
- DJI (drone flight data)
- GoPro (action cam settings)
- FLIR (thermal imaging)
- InfiRay, RED, Parrot, Reconyx, Lytro, Nintendo

### Phase 6: Software Metadata
- Photoshop, CaptureOne, NikonCapture, PhotoMechanic, FotoStation, GIMP, Scalado, InDesign
- Parse software-specific tags (layers, adjustments, edits)

---

## Testing Strategy

### Unit Tests
Each manufacturer module includes unit tests for:
- Header validation
- Tag extraction
- Lens database lookups
- Array parsing

### Integration Tests
One integration test file per manufacturer using real images:
```
tests/integration/
├── canon_makernotes_tests.rs ✅
├── nikon_makernotes_tests.rs
├── sony_makernotes_tests.rs
└── ... (one per manufacturer)
```

### Regression Testing
Golden file comparison:
```
tests/fixtures/
├── canon/eos_r5.jpg ✅
├── nikon/d850.nef
├── sony/a7riv.arw
└── ... (sample from each manufacturer)
```

---

## Success Criteria

### Per-Phase Criteria
- ✅ All manufacturer parsers compile
- ✅ All unit tests pass
- ✅ All integration tests pass
- ✅ No clippy warnings
- ✅ Code formatted with rustfmt
- ✅ Lens databases have >80% coverage of popular models

### Overall Criteria
- ✅ All 221 modules implemented
- ✅ ~47,406+ tags extractable
- ✅ Each manufacturer parser ≤800 lines (due to shared utilities)
- ✅ Comprehensive documentation
- ✅ Benchmark performance acceptable (<10ms per MakerNote)

---

## Risk Mitigation

### Risks
1. **Encrypted MakerNotes** (Nikon Type 3, some Sony) - May need decryption
2. **Undocumented tags** - Reverse engineering required
3. **Test fixtures** - Need real camera files for all manufacturers
4. **Performance** - 221 modules may slow down parsing

### Mitigations
1. Start with unencrypted formats, add decryption later
2. Focus on documented tags first (~80% coverage)
3. Crowdsource test fixtures from community
4. Lazy-load parsers only when manufacturer detected

---

## Estimated Timeline

| Phase | Scope | Estimated Effort |
|-------|-------|------------------|
| Phase 0 | Shared utilities | 3-4 days |
| Phase 1 | Top 5 manufacturers (Nikon, Sony, Fuji, Panasonic) | 3 weeks |
| Phase 2 | Mid-tier cameras (5 manufacturers) | 2 weeks |
| Phase 3 | Smartphones (5 manufacturers) | 1 week |
| Phase 4 | Legacy cameras (10 manufacturers) | 1.5 weeks |
| Phase 5 | Specialty devices (9 manufacturers) | 1.5 weeks |
| Phase 6 | Software metadata (8 applications) | 1 week |
| **Total** | **221 modules** | **~10-12 weeks** |

---

## Next Steps

1. **Review this plan** - Ensure alignment with project goals
2. **Start Phase 0** - Implement shared utilities foundation
3. **Execute Phase 1** - Tackle top manufacturers (Nikon, Sony, Fuji, Panasonic)
4. **Iterate** - Adjust plan based on learnings from Phase 1

---

## Appendix: Full Manufacturer List

### Devices (31)
**Cameras**: Canon✅, Nikon, Sony, Pentax, Olympus, Minolta, Panasonic, FujiFilm, Samsung, Sigma, Casio, Kodak, Ricoh, Leica, Leaf, PhaseOne, HP, JVC, Sanyo, Motorola, Reconyx, GE, Lytro, Nintendo

**Smartphones**: Apple, Microsoft, Google, Qualcomm

**Drones/Action**: DJI, GoPro, Parrot

**Thermal/Cinema**: FLIR, InfiRay, RED

### Software (8)
Photoshop, CaptureOne, NikonCapture, FotoStation, PhotoMechanic, GIMP, Scalado, InDesign

**Total**: 39 unique metadata sources → 221 parser modules (including lens DBs, sub-variants)
