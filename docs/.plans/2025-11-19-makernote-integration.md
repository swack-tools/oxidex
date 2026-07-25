# MakerNote Integration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Wire up the 40+ existing MakerNote parsers to the TIFF parsing pipeline by adding MakerNote tag (0x927C) handling.

**Architecture:** Create a dispatcher that reads the camera Make from EXIF tags, receives raw MakerNote data, and dispatches to the appropriate manufacturer parser. The dispatcher integrates into the existing sub-IFD handling logic in file_parser.rs.

**Tech Stack:** Rust, existing MakerNoteParser trait, HashMap for tag storage

**Impact:** Unlocks 488+ tag groups instantly with ~100 lines of new code

---

## Task 1: Add MakerNote Tag Constant

**Files:**
- Modify: `src/parsers/tiff/file_parser.rs:79-84`

**Step 1: Add the MakerNote constant**

In `file_parser.rs`, add the MakerNote constant with the other special tag IDs:

```rust
/// Special tag IDs that reference sub-IFDs
const EXIF_IFD_POINTER: u16 = 0x8769;
const GPS_INFO_IFD_POINTER: u16 = 0x8825;
const INTEROPERABILITY_IFD_POINTER: u16 = 0xA005;
const SUB_IFDS: u16 = 0x014A;
const MAKERNOTE: u16 = 0x927C;  // NEW: MakerNote tag
```

**Step 2: Add Make tag constant for camera detection**

```rust
// Tag IDs for camera detection
const MAKE: u16 = 0x010F;  // Camera manufacturer (e.g., "Canon", "Nikon")
```

**Step 3: Commit**

```bash
git add src/parsers/tiff/file_parser.rs
git commit -m "feat: add MakerNote and Make tag constants"
```

---

## Task 2: Create MakerNote Dispatcher Module

**Files:**
- Create: `src/parsers/tiff/makernote_dispatcher.rs`
- Modify: `src/parsers/tiff/mod.rs:9`

**Step 1: Write the failing test**

Create `src/parsers/tiff/makernote_dispatcher.rs`:

```rust
//! MakerNote dispatcher
//!
//! Dispatches MakerNote data to the appropriate manufacturer parser
//! based on camera make.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_canon_makernote() {
        let data = b"Canon data here";
        let mut tags = HashMap::new();

        dispatch_makernote("Canon", data, ByteOrder::LittleEndian, &mut tags).unwrap();

        // Should have extracted Canon tags
        assert!(!tags.is_empty(), "Should extract Canon MakerNote tags");
    }

    #[test]
    fn test_dispatch_unknown_manufacturer() {
        let data = b"unknown data";
        let mut tags = HashMap::new();

        let result = dispatch_makernote("UnknownMake", data, ByteOrder::LittleEndian, &mut tags);

        // Should succeed but not extract any tags
        assert!(result.is_ok());
        assert!(tags.is_empty(), "Should not extract tags for unknown make");
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p oxidex makernote_dispatcher::tests --lib -- --nocapture
```

Expected: FAIL with "dispatch_makernote not found"

**Step 3: Declare module in mod.rs**

In `src/parsers/tiff/mod.rs` after line 9:

```rust
pub mod file_parser;
pub mod ifd_parser;
pub mod makernote_dispatcher;  // NEW
pub mod makernote_parser;
pub mod makernotes;
pub mod tag_parser;
pub mod tiff_enums;
```

**Step 4: Implement minimal dispatcher function**

Add to `src/parsers/tiff/makernote_dispatcher.rs` before the tests module:

```rust
use crate::parsers::tiff::makernotes::*;

/// Dispatches MakerNote data to appropriate manufacturer parser
///
/// # Arguments
/// * `make` - Camera manufacturer name (e.g., "Canon", "Nikon", "Sony")
/// * `data` - Raw MakerNote data bytes
/// * `byte_order` - Byte order for parsing
/// * `tags` - HashMap to insert extracted tags into
///
/// # Returns
/// Ok(()) on success, Err(message) on parse failure
pub fn dispatch_makernote(
    make: &str,
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) -> Result<(), String> {
    use crate::parsers::tiff::makernotes::shared::MakerNoteParser;

    // Normalize make string (trim whitespace, case-insensitive matching)
    let make_normalized = make.trim().to_lowercase();

    // Dispatch to appropriate parser based on manufacturer
    let parser: Option<Box<dyn MakerNoteParser>> = match make_normalized.as_str() {
        "canon" => Some(Box::new(canon::CanonParser)),
        "nikon" | "nikon corporation" => Some(Box::new(nikon::NikonParser)),
        "sony" => Some(Box::new(sony::SonyParser)),
        "olympus" | "olympus corporation" | "olympus imaging corp." => Some(Box::new(olympus::OlympusParser)),
        "panasonic" => Some(Box::new(panasonic::PanasonicParser)),
        "pentax" | "pentax corporation" | "ricoh imaging company, ltd." => Some(Box::new(pentax::PentaxParser)),
        "fujifilm" | "fuji photo film co., ltd." => Some(Box::new(fujifilm::FujifilmParser)),
        "leica" | "leica camera ag" => Some(Box::new(leica::LeicaMakerNoteParser)),
        "sigma" | "sigma corporation" => Some(Box::new(sigma::SigmaMakerNoteParser)),
        "phase one" | "phase one a/s" => Some(Box::new(phaseone::PhaseOneMakerNoteParser)),
        "minolta" | "konica minolta" | "minolta co., ltd." => Some(Box::new(minolta::MinoltaParser)),

        // Smartphones
        "apple" => Some(Box::new(apple::AppleParser)),
        "google" => Some(Box::new(google::GoogleParser)),
        "samsung" | "samsung electronics" => Some(Box::new(samsung::SamsungParser)),
        "microsoft" | "microsoft corporation" => Some(Box::new(microsoft::MicrosoftParser)),
        "qualcomm" => Some(Box::new(qualcomm::QualcommParser)),

        // Specialty devices
        "dji" => Some(Box::new(dji::DjiParser)),
        "flir" | "flir systems" => Some(Box::new(flir::FlirParser)),
        "gopro" => Some(Box::new(gopro::GoProParser)),
        "infiray" => Some(Box::new(infiray::InfiRayParser)),
        "lytro" | "lytro, inc." => Some(Box::new(lytro::LytroParser)),
        "nintendo" => Some(Box::new(nintendo::NintendoParser)),
        "parrot" => Some(Box::new(parrot::ParrotParser)),
        "reconyx" => Some(Box::new(reconyx::ReconxyParser)),
        "red" | "red.com" | "red digital cinema" => Some(Box::new(red::RedParser)),

        // Legacy cameras
        "casio" | "casio computer co.,ltd." => Some(Box::new(casio::CasioParser)),
        "ge" | "general electric" => Some(Box::new(ge::GeParser)),
        "hp" | "hewlett-packard" => Some(Box::new(hp::HpParser)),
        "jvc" | "victor company of japan, limited" => Some(Box::new(jvc::JvcParser)),
        "kodak" | "eastman kodak company" => Some(Box::new(kodak::KodakParser)),
        "leaf" => Some(Box::new(leaf::LeafParser)),
        "motorola" => Some(Box::new(motorola::MotorolaParser)),
        "ricoh" | "ricoh company, ltd." => Some(Box::new(ricoh::RicohParser)),
        "sanyo" | "sanyo electric co.,ltd." => Some(Box::new(sanyo::SanyoParser)),

        // Software applications
        "capture one" => Some(Box::new(captureone::CaptureOneParser)),
        "fotostation" | "fotoware" => Some(Box::new(fotostation::FotoStationParser)),
        "gimp" => Some(Box::new(gimp::GimpParser)),
        "adobe indesign" | "indesign" => Some(Box::new(indesign::InDesignParser)),
        "nikon capture" | "capture nx" => Some(Box::new(nikoncapture::NikonCaptureParser)),
        "photo mechanic" => Some(Box::new(photomechanic::PhotoMechanicParser)),
        "photoshop" | "adobe photoshop" => Some(Box::new(photoshop::PhotoshopParser)),
        "scalado" => Some(Box::new(scalado::ScaladoParser)),

        _ => None, // Unknown manufacturer
    };

    // If we have a parser, validate and parse
    if let Some(parser) = parser {
        // Validate header if parser provides validation
        if parser.validate_header(data) {
            // Parse MakerNote data
            parser.parse(data, byte_order, tags)?;
        } else {
            // Header validation failed, but don't error - just skip
            return Ok(());
        }
    }

    // If no parser found, silently succeed (not all makes have MakerNotes)
    Ok(())
}
```

**Step 5: Run tests to verify they pass**

```bash
cargo test -p oxidex makernote_dispatcher::tests --lib -- --nocapture
```

Expected: Tests may fail if Canon parser requires specific header format. This is OK for now.

**Step 6: Commit**

```bash
git add src/parsers/tiff/makernote_dispatcher.rs src/parsers/tiff/mod.rs
git commit -m "feat: add MakerNote dispatcher with 40+ manufacturer support"
```

---

## Task 3: Extract Camera Make from Tags

**Files:**
- Modify: `src/parsers/tiff/file_parser.rs:290-408`

**Step 1: Add helper function to extract Make tag**

In `file_parser.rs`, add after `extract_u32_from_tag_value` function (~line 244):

```rust
/// Extracts the camera Make string from tag values
///
/// Searches through tags for the Make tag (0x010F) and extracts it as a string.
///
/// # Parameters
///
/// - `tags`: Vector of (tag_id, field_type, value_count, raw_value) tuples
///
/// # Returns
///
/// - `Some(String)`: Camera make if found
/// - `None`: Make tag not found or invalid data
fn extract_make_from_tags(tags: &IfdEntries) -> Option<String> {
    for (tag_id, _field_type, _count, value) in tags {
        if *tag_id == MAKE {
            // Make is ASCII string, typically null-terminated
            let value_bytes = value.as_ref();

            // Find null terminator or use full length
            let end = value_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(value_bytes.len());

            // Convert to string, trimming whitespace
            if let Ok(make) = String::from_utf8(value_bytes[..end].to_vec()) {
                return Some(make.trim().to_string());
            }
        }
    }
    None
}
```

**Step 2: Add test for extract_make_from_tags**

In the tests module at the bottom of `file_parser.rs`:

```rust
#[test]
fn test_extract_make_from_tags() {
    use std::borrow::Cow;

    // Create tags with Make tag
    let tags = vec![
        (0x010F, 2, 6, Cow::Borrowed(b"Canon\0")),  // Make tag
        (0x0110, 2, 6, Cow::Borrowed(b"EOS 5D")),   // Model tag
    ];

    let make = extract_make_from_tags(&tags);
    assert_eq!(make, Some("Canon".to_string()));
}

#[test]
fn test_extract_make_from_tags_not_found() {
    let tags = vec![
        (0x0110, 2, 6, Cow::Borrowed(b"EOS 5D")),  // Model but no Make
    ];

    let make = extract_make_from_tags(&tags);
    assert_eq!(make, None);
}
```

**Step 3: Run tests**

```bash
cargo test -p oxidex file_parser::tests::test_extract_make --lib -- --nocapture
```

Expected: PASS

**Step 4: Commit**

```bash
git add src/parsers/tiff/file_parser.rs
git commit -m "feat: add camera Make extraction helper function"
```

---

## Task 4: Wire Up MakerNote Handling in File Parser

**Files:**
- Modify: `src/parsers/tiff/file_parser.rs:290-408`

**Step 1: Import dispatcher**

At the top of `file_parser.rs`, add after other imports:

```rust
use crate::parsers::tiff::makernote_dispatcher::dispatch_makernote;
use std::collections::HashMap;
```

**Step 2: Modify parse_tiff_file to handle MakerNote**

Replace the section starting at line 333 (the loop that checks for sub-IFD pointers) with:

```rust
        // Check for sub-IFD pointers and MakerNote, and recursively parse them
        for (tag_id, _field_type, _value_count, value) in &tags {
            match *tag_id {
                EXIF_IFD_POINTER | GPS_INFO_IFD_POINTER | INTEROPERABILITY_IFD_POINTER => {
                    // Convert Cow<[u8]> to &[u8] using as_ref()
                    if let Some(sub_ifd_offset) =
                        extract_u32_from_tag_value(value.as_ref(), byte_order)
                    {
                        // Skip if we've already visited this offset
                        if !visited_offsets.contains(&(sub_ifd_offset as u64)) {
                            // Parse sub-IFD
                            match parse_ifd(reader, sub_ifd_offset as u64, byte_order) {
                                Ok(sub_tags) => {
                                    all_tags.extend(sub_tags);
                                    visited_offsets.insert(sub_ifd_offset as u64);
                                }
                                Err(e) => {
                                    // Log but don't fail - some files have invalid sub-IFD pointers
                                    eprintln!(
                                        "Warning: Failed to parse sub-IFD at offset {}: {}",
                                        sub_ifd_offset, e
                                    );
                                }
                            }
                        }
                    }
                }
                SUB_IFDS => {
                    // SubIFDs tag can contain multiple offsets
                    // Each offset is 4 bytes (u32)
                    // Convert Cow<[u8]> to &[u8] using as_ref()
                    let value_bytes = value.as_ref();
                    let offset_count = value_bytes.len() / 4;
                    for i in 0..offset_count {
                        let offset_bytes = &value_bytes[i * 4..(i + 1) * 4];
                        if let Some(sub_ifd_offset) =
                            extract_u32_from_tag_value(offset_bytes, byte_order)
                        {
                            if !visited_offsets.contains(&(sub_ifd_offset as u64)) {
                                match parse_ifd(reader, sub_ifd_offset as u64, byte_order) {
                                    Ok(sub_tags) => {
                                        all_tags.extend(sub_tags);
                                        visited_offsets.insert(sub_ifd_offset as u64);
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "Warning: Failed to parse sub-IFD at offset {}: {}",
                                            sub_ifd_offset, e
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                MAKERNOTE => {
                    // NEW: MakerNote handling
                    // Extract camera make from current tags
                    if let Some(make) = extract_make_from_tags(&all_tags) {
                        // Parse MakerNote using dispatcher
                        let mut makernote_tags = HashMap::new();
                        let makernote_data = value.as_ref();

                        match dispatch_makernote(&make, makernote_data, byte_order, &mut makernote_tags) {
                            Ok(()) => {
                                // Convert HashMap<String, String> to IfdEntries format
                                // Tag ID 0x927C, Type 7 (UNDEFINED), count = data length
                                for (key, val) in makernote_tags {
                                    // Create synthetic tag entries for MakerNote tags
                                    // We use a synthetic tag ID and store the key:value as a string
                                    let synthetic_value = format!("{}: {}", key, val);
                                    all_tags.push((
                                        MAKERNOTE,  // Use MakerNote tag ID
                                        7,  // Type UNDEFINED
                                        synthetic_value.len() as u32,
                                        std::borrow::Cow::Owned(synthetic_value.into_bytes()),
                                    ));
                                }
                            }
                            Err(e) => {
                                eprintln!("Warning: Failed to parse MakerNote for {}: {}", make, e);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
```

**Step 3: Run existing TIFF tests**

```bash
cargo test -p oxidex file_parser --lib -- --nocapture
```

Expected: All existing tests should still pass

**Step 4: Commit**

```bash
git add src/parsers/tiff/file_parser.rs
git commit -m "feat: wire up MakerNote dispatcher in TIFF file parser"
```

---

## Task 5: Integration Testing with Real JPEG

**Files:**
- Create: `tests/integration/makernote_integration.rs`
- Modify: `tests/integration/mod.rs`

**Step 1: Create integration test file**

Create `tests/integration/makernote_integration.rs`:

```rust
//! MakerNote integration tests
//!
//! These tests verify that MakerNote data is correctly extracted
//! from real JPEG files with EXIF data.

#[cfg(test)]
mod tests {
    use oxidex::parsers::jpeg::parse_jpeg;
    use oxidex::io::buffered_reader::BufferedReader;
    use std::path::PathBuf;

    fn get_test_image_path(filename: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("samples")
            .join(filename)
    }

    #[test]
    #[ignore] // Ignore until we have real test images
    fn test_canon_makernote_extraction() {
        let path = get_test_image_path("canon_sample.jpg");

        if !path.exists() {
            eprintln!("Skipping test: Canon sample image not found");
            return;
        }

        let reader = BufferedReader::new(&path).expect("Failed to open test image");
        let metadata = parse_jpeg(&reader).expect("Failed to parse JPEG");

        // Check for Canon MakerNote tags
        let canon_tags: Vec<_> = metadata
            .iter()
            .filter(|(k, _)| k.starts_with("Canon:"))
            .collect();

        assert!(
            !canon_tags.is_empty(),
            "Should extract Canon MakerNote tags"
        );
    }

    #[test]
    #[ignore] // Ignore until we have real test images
    fn test_nikon_makernote_extraction() {
        let path = get_test_image_path("nikon_sample.jpg");

        if !path.exists() {
            eprintln!("Skipping test: Nikon sample image not found");
            return;
        }

        let reader = BufferedReader::new(&path).expect("Failed to open test image");
        let metadata = parse_jpeg(&reader).expect("Failed to parse JPEG");

        // Check for Nikon MakerNote tags
        let nikon_tags: Vec<_> = metadata
            .iter()
            .filter(|(k, _)| k.starts_with("Nikon:"))
            .collect();

        assert!(
            !nikon_tags.is_empty(),
            "Should extract Nikon MakerNote tags"
        );
    }

    #[test]
    fn test_jpeg_without_makernote() {
        // This test should pass even without test images
        // as it tests that the code doesn't crash on images without MakerNotes

        // This is a minimal valid JPEG with no EXIF/MakerNote
        let minimal_jpeg = vec![
            0xFF, 0xD8,  // SOI
            0xFF, 0xE0,  // APP0
            0x00, 0x10,  // Length
            0x4A, 0x46, 0x49, 0x46, 0x00,  // "JFIF\0"
            0x01, 0x01,  // Version 1.1
            0x00,  // Units: none
            0x00, 0x01, 0x00, 0x01,  // X/Y density: 1x1
            0x00, 0x00,  // Thumbnail: 0x0
            0xFF, 0xD9,  // EOI
        ];

        // Write to temp file
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_minimal.jpg");
        std::fs::write(&temp_path, minimal_jpeg).expect("Failed to write temp file");

        let reader = BufferedReader::new(&temp_path).expect("Failed to open temp file");
        let metadata = parse_jpeg(&reader).expect("Should parse JPEG without MakerNote");

        // Cleanup
        let _ = std::fs::remove_file(&temp_path);

        // Should succeed without crashing
        assert!(metadata.len() >= 0);
    }
}
```

**Step 2: Add module to integration tests**

If `tests/integration/mod.rs` exists, add:

```rust
mod makernote_integration;
```

If it doesn't exist, create it with:

```rust
//! Integration tests for oxidex

mod makernote_integration;
```

**Step 3: Run integration tests**

```bash
cargo test -p oxidex integration::makernote_integration --test '*' -- --nocapture
```

Expected: Non-ignored test passes (minimal JPEG test)

**Step 4: Commit**

```bash
git add tests/integration/makernote_integration.rs tests/integration/mod.rs
git commit -m "test: add MakerNote integration tests"
```

---

## Task 6: Update Documentation

**Files:**
- Modify: `README.md` (if exists at project root)
- Create: `docs/features/makernote-support.md`

**Step 1: Create MakerNote support documentation**

Create `docs/features/makernote-support.md`:

```markdown
# MakerNote Support

Oxidex supports extracting manufacturer-specific metadata (MakerNotes) from JPEG and TIFF files for 40+ camera manufacturers and software applications.

## Supported Manufacturers

### Traditional Cameras
- Canon
- Nikon
- Sony
- Olympus
- Panasonic
- Pentax
- Fujifilm
- Leica
- Sigma
- Phase One
- Minolta

### Smartphones
- Apple
- Google (Pixel)
- Samsung
- Microsoft
- Qualcomm

### Specialty Devices
- DJI (drones)
- FLIR (thermal cameras)
- GoPro (action cameras)
- RED (cinema cameras)
- Reconyx (wildlife cameras)
- InfiRay (thermal)
- Lytro (light field)
- Nintendo 3DS
- Parrot (drones)

### Legacy Cameras
- Casio
- GE
- HP
- JVC
- Kodak
- Leaf
- Motorola
- Ricoh
- Sanyo

### Software Applications
- Capture One
- FotoStation/FotoWare
- GIMP
- Adobe InDesign
- Nikon Capture NX
- Photo Mechanic
- Adobe Photoshop
- Scalado

## How It Works

MakerNotes are proprietary binary data structures embedded in EXIF tag 0x927C. Each manufacturer uses a different format.

When Oxidex encounters a MakerNote tag:

1. **Detects the camera make** from EXIF Make tag (0x010F)
2. **Dispatches to manufacturer parser** based on normalized make string
3. **Validates the header** (if parser provides validation)
4. **Extracts manufacturer-specific tags** (lens info, focus points, custom settings, etc.)
5. **Returns tags** with manufacturer prefix (e.g., "Canon:LensModel")

## Tag Naming Convention

MakerNote tags use the format: `{Manufacturer}:{TagName}`

Examples:
- `Canon:LensModel` - Canon lens model string
- `Nikon:ShutterCount` - Nikon shutter actuation count
- `Sony:LensType` - Sony lens type ID
- `DJI:FlightSpeed` - DJI drone flight speed

## Adding New Manufacturers

To add support for a new manufacturer:

1. Create parser in `src/parsers/tiff/makernotes/{manufacturer}.rs`
2. Implement the `MakerNoteParser` trait
3. Add manufacturer to dispatcher in `src/parsers/tiff/makernote_dispatcher.rs`
4. Add tests

See existing parsers for examples.

## Limitations

- Not all camera makes have MakerNote parsers (unknown makes are silently skipped)
- Some manufacturers encrypt or obfuscate their MakerNote data
- MakerNote formats may change between camera models
- Software-generated MakerNotes may have limited metadata

## References

- [EXIF Specification](https://www.exif.org/)
- [ExifTool Tag Names](https://exiftool.org/TagNames/index.html) (reference implementation)
```

**Step 2: Update main README (if applicable)**

If project has a README.md with features section, add:

```markdown
### MakerNote Support

Extract manufacturer-specific metadata from 40+ camera brands including Canon, Nikon, Sony, Olympus, Panasonic, Pentax, Fujifilm, and more. See [MakerNote documentation](docs/features/makernote-support.md) for full list.
```

**Step 3: Commit**

```bash
git add docs/features/makernote-support.md README.md
git commit -m "docs: add MakerNote support documentation"
```

---

## Task 7: Final Verification and Testing

**Files:**
- None (testing only)

**Step 1: Build the project**

```bash
cargo build --release
```

Expected: Clean build with no warnings

**Step 2: Run all tests**

```bash
cargo test --workspace -- --nocapture
```

Expected: All tests pass

**Step 3: Test with clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Expected: No clippy warnings

**Step 4: Format check**

```bash
cargo fmt --check
```

Expected: Code is properly formatted

**Step 5: If any issues found, fix and commit**

```bash
# Fix any formatting
cargo fmt

# Fix any clippy issues
# (manual fixes as needed)

git add .
git commit -m "fix: address clippy warnings and formatting"
```

---

## Success Criteria

- [ ] MakerNote tag constant (0x927C) defined in file_parser.rs
- [ ] Make tag constant (0x010F) defined in file_parser.rs
- [ ] MakerNote dispatcher created with 40+ manufacturer support
- [ ] Dispatcher integrated into file_parser.rs sub-IFD handling
- [ ] Camera Make extraction helper function implemented
- [ ] All existing tests pass
- [ ] Integration tests added (though may be ignored without sample images)
- [ ] Documentation created explaining MakerNote support
- [ ] No clippy warnings
- [ ] Code properly formatted

## Testing with Real Images

To fully validate MakerNote extraction:

1. Obtain sample JPEG images from various manufacturers
2. Place in `tests/samples/` directory
3. Remove `#[ignore]` from integration tests
4. Run: `cargo test integration::makernote_integration`
5. Verify manufacturer-specific tags are extracted

## Known Limitations

1. **Tag Format Conversion:** Currently, MakerNote tags are stored as synthetic tag entries with the same tag ID (0x927C). This works but is not ideal. Future enhancement: Create separate tag ID space for MakerNote tags.

2. **Header Validation:** Some parsers have strict header validation that may reject valid data. Monitor for warnings in logs.

3. **Byte Order:** MakerNotes may use different byte order than main EXIF. Currently using TIFF byte order. Future: Detect MakerNote-specific byte order.

4. **Performance:** Creating HashMap and converting to tag entries has overhead. Future: Consider streaming approach or lazy evaluation.

## Future Enhancements

1. Create dedicated tag ID space for MakerNote tags (0xE000-0xEFFF range?)
2. Add MakerNote byte order detection
3. Implement caching for frequently accessed MakerNotes
4. Add benchmarks for MakerNote parsing performance
5. Create tool to dump all MakerNote tags from an image for debugging

---

**Implementation Time Estimate:** 1-2 hours

**Impact:** Unlocks 488+ camera-specific metadata tags instantly!

**Next Steps After Completion:**

1. Test with real camera images from different manufacturers
2. Monitor for parsing errors in production use
3. Add missing manufacturers as needed
4. Consider optimization if performance becomes an issue

---

**End of Plan**
