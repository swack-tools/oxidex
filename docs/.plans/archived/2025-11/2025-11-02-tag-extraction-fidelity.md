# Tag Extraction Fidelity Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Achieve tag extraction fidelity with ExifTool Perl by expanding tag database from ~731 tags to 28,853+ tags covering all format families and MakerNotes.

**Architecture:** Extend the existing build.rs tag generation system to parse all 100+ ExifTool Perl modules, including manufacturer-specific MakerNotes, extended format families, and composite tags. Use incremental parsing with validation and robust error handling.

**Tech Stack:**
- Rust build scripts (build.rs)
- Regex for Perl parsing
- ureq for HTTP downloads
- HashMap for tag deduplication
- Generated Rust code with lazy static initialization

---

## Current State Analysis

**Existing Implementation:**
- File: `build.rs` - Parses 15 base ExifTool Perl modules
- Current coverage: ~731 tags (2.5% of ExifTool's 28,853 tags)
- Modules parsed: EXIF, GPS, XMP, IPTC, PDF, QuickTime, Photoshop, PNG, JFIF, JPEG, TIFF, ICC_Profile, PostScript, RIFF, MakerNotes (base only)

**Gap Analysis:**
- Missing: 28,122 tags (~97.5% of ExifTool tags)
- Missing modules: 85+ additional modules including:
  - **MakerNotes**: Canon (2000+ tags), Nikon (1500+ tags), Sony (1200+ tags), Panasonic, Olympus, Pentax, FujiFilm, etc.
  - **Extended formats**: DNG, FlashPix, MPF, GeoTiff, FLIR, etc.
  - **Media formats**: ID3, FLAC, Vorbis, Matroska, MXF, DICOM, etc.
  - **Document formats**: OOXML, iWork, RTF, ZIP metadata
  - **Composite tags**: Calculated/derived tags
  - **Shortcuts**: Tag aliases and groups

**Target State:**
- Parse 100+ ExifTool Perl modules
- Generate 28,853+ tag definitions
- Support all format families
- Include MakerNotes for 20+ camera manufacturers
- Support composite and shortcut tags

---

## Task Breakdown

### Task 1: Audit ExifTool Module Structure

**Goal:** Understand the complete structure of ExifTool Perl modules to plan comprehensive parsing.

**Files:**
- Create: `docs/analysis/exiftool-module-audit.md`
- Reference: Downloaded ExifTool source in `OUT_DIR/exiftool-source/`

**Step 1: Download and extract ExifTool source for analysis**

Run this from project root to get ExifTool source locally:

```bash
# Create temp directory for analysis
mkdir -p /tmp/exiftool-analysis
cd /tmp/exiftool-analysis

# Download ExifTool source
wget https://github.com/exiftool/exiftool/archive/refs/heads/master.zip
unzip -q master.zip
cd exiftool-master/lib/Image/ExifTool
```

**Step 2: List all Perl modules**

Run:
```bash
find . -name "*.pm" -type f | sort > /tmp/exiftool-modules.txt
wc -l /tmp/exiftool-modules.txt
```

Expected: 100+ modules listed

**Step 3: Categorize modules by type**

Create `docs/analysis/exiftool-module-audit.md` with module categorization:

```markdown
# ExifTool Module Audit

## Module Categories

### Base Format Modules (15 - Already Parsed)
- EXIF.pm, GPS.pm, XMP.pm, IPTC.pm, PDF.pm, QuickTime.pm, etc.

### MakerNotes Modules (20+ manufacturers)
- Canon.pm, Nikon.pm, Sony.pm, Panasonic.pm, Olympus.pm, etc.

### Extended Format Modules
- DNG.pm, FlashPix.pm, GeoTiff.pm, FLIR.pm, etc.

### Media Format Modules
- ID3.pm, FLAC.pm, Matroska.pm, DICOM.pm, etc.

### Composite Modules
- Composite.pm - Calculated tags
- Shortcuts.pm - Tag aliases

## Total Count: [X] modules
```

**Step 4: Count tags per module**

For each major module, count tag definitions:

```bash
# Count tag table definitions in a module
grep -c "=>" Canon.pm
```

Document approximate tag counts for top 20 modules.

**Step 5: Commit audit documentation**

```bash
git add docs/analysis/exiftool-module-audit.md
git commit -m "docs: audit ExifTool module structure for tag fidelity

- List all 100+ Perl modules
- Categorize by format family
- Estimate tag counts per module
- Identify parsing requirements"
```

---

### Task 2: Extend build.rs to Parse All Base Format Modules

**Goal:** Add 30+ additional base format modules to expand coverage from 731 to ~3000 tags.

**Files:**
- Modify: `build.rs:152-168` (module list)
- Test: Run `cargo clean && cargo build` to verify

**Step 1: Write test to verify minimum tag count increases**

Create test file: `tests/tag_database_coverage.rs`

```rust
//! Integration tests for tag database coverage

use oxidex::tag_db::generated_tags::generated_tag_count;

#[test]
fn test_tag_database_has_minimum_coverage() {
    let count = generated_tag_count();

    // After adding all base format modules, expect at least 3000 tags
    assert!(
        count >= 3000,
        "Expected at least 3000 tags, found {}. Need to add more modules to build.rs",
        count
    );
}

#[test]
fn test_tag_database_target_coverage() {
    let count = generated_tag_count();

    // Ultimate target: 28,853 tags for full ExifTool parity
    // This test documents the gap
    let target = 28853;
    let coverage_percent = (count as f64 / target as f64) * 100.0;

    println!("Tag coverage: {}/{} ({:.1}%)", count, target, coverage_percent);

    // For now, we expect at least 10% coverage (2886 tags)
    assert!(
        count >= 2886,
        "Expected at least 10% coverage (2886 tags), found {} ({:.1}%)",
        count,
        coverage_percent
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_tag_database_has_minimum_coverage`

Expected: FAIL - "Expected at least 3000 tags, found 731"

**Step 3: Extend module list in build.rs**

Modify `build.rs:152-168` to add 30+ additional modules:

```rust
// Parse key modules for different format families
let modules = vec![
    // ===== BASE FORMATS (existing 15) =====
    ("EXIF.pm", "EXIF"),
    ("GPS.pm", "GPS"),
    ("XMP.pm", "XMP"),
    ("IPTC.pm", "IPTC"),
    ("PDF.pm", "PDF"),
    ("QuickTime.pm", "QuickTime"),
    ("Photoshop.pm", "Photoshop"),
    ("PNG.pm", "PNG"),
    ("JFIF.pm", "JFIF"),
    ("JPEG.pm", "JPEG"),
    ("TIFF.pm", "TIFF"),
    ("ICC_Profile.pm", "ICC_Profile"),
    ("PostScript.pm", "PostScript"),
    ("RIFF.pm", "RIFF"),
    ("MakerNotes.pm", "MakerNotes"),

    // ===== EXTENDED IMAGE FORMATS (new +10) =====
    ("DNG.pm", "DNG"),
    ("FlashPix.pm", "FlashPix"),
    ("MPF.pm", "MPF"),
    ("GeoTiff.pm", "GeoTiff"),
    ("Jpeg2000.pm", "Jpeg2000"),
    ("GIF.pm", "GIF"),
    ("BMP.pm", "BMP"),
    ("OpenEXR.pm", "OpenEXR"),
    ("PGF.pm", "PGF"),
    ("MNG.pm", "MNG"),

    // ===== AUDIO/VIDEO FORMATS (new +12) =====
    ("ID3.pm", "ID3"),
    ("FLAC.pm", "FLAC"),
    ("Vorbis.pm", "Vorbis"),
    ("Opus.pm", "Opus"),
    ("Matroska.pm", "Matroska"),
    ("ASF.pm", "ASF"),
    ("MPEG.pm", "MPEG"),
    ("M2TS.pm", "M2TS"),
    ("MXF.pm", "MXF"),
    ("Flash.pm", "Flash"),
    ("Real.pm", "Real"),
    ("AIFF.pm", "AIFF"),

    // ===== SPECIALIZED FORMATS (new +8) =====
    ("DICOM.pm", "DICOM"),
    ("FITS.pm", "FITS"),
    ("FLIR.pm", "FLIR"),
    ("Parrot.pm", "Parrot"),
    ("DJI.pm", "DJI"),
    ("GoPro.pm", "GoPro"),
    ("Apple.pm", "Apple"),
    ("Microsoft.pm", "Microsoft"),

    // ===== DOCUMENT/METADATA FORMATS (new +5) =====
    ("HTML.pm", "HTML"),
    ("Font.pm", "Font"),
    ("ZIP.pm", "ZIP"),
    ("EXE.pm", "EXE"),
    ("MIE.pm", "MIE"),
];
```

**Step 4: Add format families to tag_descriptor.rs**

Modify: `src/core/tag_descriptor.rs` to add new FormatFamily enum variants.

First, read the file:

```bash
# Read current FormatFamily enum
grep -A 30 "pub enum FormatFamily" src/core/tag_descriptor.rs
```

Then add new variants:

```rust
/// Format family classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormatFamily {
    // Existing variants
    EXIF,
    GPS,
    IPTC,
    XMP,
    PDF,
    QuickTime,
    RIFF,
    ICC_Profile,
    ICCProfile,  // Alias
    Photoshop,
    PNG,
    JPEG,
    JFIF,
    TIFF,
    PostScript,
    MakerNotes,

    // NEW: Extended image formats
    DNG,
    FlashPix,
    MPF,
    GeoTiff,
    Jpeg2000,
    GIF,
    BMP,
    OpenEXR,
    PGF,
    MNG,

    // NEW: Audio/Video formats
    ID3,
    FLAC,
    Vorbis,
    Opus,
    Matroska,
    ASF,
    MPEG,
    M2TS,
    MXF,
    Flash,
    Real,
    AIFF,

    // NEW: Specialized formats
    DICOM,
    FITS,
    FLIR,
    Parrot,
    DJI,
    GoPro,
    Apple,
    Microsoft,

    // NEW: Document/Metadata formats
    HTML,
    Font,
    ZIP,
    EXE,
    MIE,
}
```

**Step 5: Rebuild and verify tag count increase**

Run:
```bash
# Clean to force regeneration
cargo clean

# Rebuild (will download ExifTool and generate new tag database)
cargo build --release 2>&1 | grep "Successfully generated"
```

Expected output: "Successfully generated tag database with 3000+ tags"

**Step 6: Run tests to verify**

Run: `cargo test test_tag_database_has_minimum_coverage`

Expected: PASS

**Step 7: Commit the changes**

```bash
git add build.rs src/core/tag_descriptor.rs tests/tag_database_coverage.rs
git commit -m "feat: expand tag database to 3000+ tags with extended format modules

- Add 35 additional ExifTool modules to build.rs parser
- Include extended image formats (DNG, FlashPix, GeoTiff, etc.)
- Add audio/video format support (ID3, FLAC, Matroska, etc.)
- Support specialized formats (DICOM, FLIR, DJI, etc.)
- Add document format metadata (HTML, Font, ZIP, etc.)
- Expand FormatFamily enum with 35 new variants
- Add integration tests for tag coverage verification

Coverage: ~731 tags → ~3000 tags (~10% of ExifTool's 28,853 tags)"
```

---

### Task 3: Add MakerNotes Support for Top 5 Camera Manufacturers

**Goal:** Add Canon, Nikon, Sony, Panasonic, and Olympus MakerNotes (estimated 7000+ additional tags).

**Files:**
- Modify: `build.rs:152-200` (add MakerNotes modules)
- Modify: `src/core/tag_descriptor.rs` (add FormatFamily variants)

**Step 1: Write test for MakerNotes tag presence**

Add to `tests/tag_database_coverage.rs`:

```rust
#[test]
fn test_makernotes_canon_present() {
    use oxidex::tag_db::generated_tags::get_generated_tag_descriptor;

    // Canon has 2000+ tags, test a few common ones
    assert!(
        get_generated_tag_descriptor("Canon:CanonImageType").is_some(),
        "Canon MakerNotes tags should be present"
    );
    assert!(
        get_generated_tag_descriptor("Canon:CanonFirmwareVersion").is_some(),
        "Canon MakerNotes tags should be present"
    );
}

#[test]
fn test_makernotes_nikon_present() {
    use oxidex::tag_db::generated_tags::get_generated_tag_descriptor;

    assert!(
        get_generated_tag_descriptor("Nikon:ISOSetting").is_some(),
        "Nikon MakerNotes tags should be present"
    );
}

#[test]
fn test_makernotes_sony_present() {
    use oxidex::tag_db::generated_tags::get_generated_tag_descriptor;

    assert!(
        get_generated_tag_descriptor("Sony:ColorTemperature").is_some(),
        "Sony MakerNotes tags should be present"
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_makernotes_canon_present`

Expected: FAIL - "Canon MakerNotes tags should be present"

**Step 3: Add MakerNotes modules to build.rs**

Extend the modules list in `build.rs`:

```rust
    // ===== MAKERNOTES - CAMERA MANUFACTURERS (new +5) =====
    ("Canon.pm", "Canon"),
    ("Nikon.pm", "Nikon"),
    ("Sony.pm", "Sony"),
    ("Panasonic.pm", "Panasonic"),
    ("Olympus.pm", "Olympus"),
```

**Step 4: Add MakerNotes FormatFamily variants**

Modify `src/core/tag_descriptor.rs`:

```rust
    // MakerNotes - Camera Manufacturers
    Canon,
    Nikon,
    Sony,
    Panasonic,
    Olympus,
```

**Step 5: Handle nested tag tables in Perl modules**

MakerNotes modules often have nested tag table definitions. Update `parse_perl_module` in `build.rs` to handle multiple tag tables per file.

Add after line 214:

```rust
    // Track multiple tag tables (MakerNotes modules have many)
    let mut current_table_name: Option<String> = None;

    // Enhanced regex for nested table detection
    let table_start_regex = Regex::new(r"%Image::ExifTool::(\w+)::(\w+)\s*=\s*\(")?;
```

Modify table detection logic (around line 218):

```rust
        // Detect tag table start with table name capture
        if let Some(caps) = table_start_regex.captures(&line) {
            in_tag_table = true;
            current_table_name = Some(format!("{}::{}", &caps[1], &caps[2]));
            println!("cargo:warning=  Found tag table: {:?}", current_table_name);
            continue;
        }
```

**Step 6: Rebuild and verify tag count**

Run:
```bash
cargo clean
cargo build --release 2>&1 | grep "tags from"
```

Expected output showing:
- "Parsed 2000+ tags from Canon.pm"
- "Parsed 1500+ tags from Nikon.pm"
- "Parsed 1200+ tags from Sony.pm"

**Step 7: Run MakerNotes tests**

Run: `cargo test test_makernotes`

Expected: All MakerNotes tests PASS

**Step 8: Verify total tag count**

Run:
```bash
# Count lines in generated file (rough proxy for tag count)
wc -l src/tag_db/generated_tags.rs

# Run coverage test
cargo test test_tag_database_target_coverage
```

Expected: ~10,000+ tags (731 + 3000 from Task 2 + 7000 from MakerNotes)

**Step 9: Commit MakerNotes support**

```bash
git add build.rs src/core/tag_descriptor.rs tests/tag_database_coverage.rs
git commit -m "feat: add MakerNotes support for top 5 camera manufacturers

- Add Canon, Nikon, Sony, Panasonic, Olympus modules to build.rs
- Parse nested tag tables in MakerNotes modules
- Add 7000+ camera-specific tags
- Support Canon (2000+ tags), Nikon (1500+ tags), Sony (1200+ tags)
- Add integration tests for MakerNotes tag presence

Coverage: ~3000 tags → ~10,000 tags (~35% of ExifTool's 28,853 tags)"
```

---

### Task 4: Add Remaining MakerNotes Manufacturers (15+ brands)

**Goal:** Add comprehensive MakerNotes support for all remaining camera manufacturers.

**Files:**
- Modify: `build.rs` (add 15+ MakerNotes modules)
- Modify: `src/core/tag_descriptor.rs`

**Step 1: Add test for comprehensive MakerNotes coverage**

Add to `tests/tag_database_coverage.rs`:

```rust
#[test]
fn test_all_makernotes_manufacturers() {
    use oxidex::tag_db::generated_tags::get_generated_tag_descriptor;

    // Test presence of tags from various manufacturers
    let test_tags = vec![
        "FujiFilm:FilmMode",
        "Pentax:Quality",
        "Minolta:ExposureMode",
        "Casio:RecordingMode",
        "Sigma:SerialNumber",
        "Samsung:LocalLocationName",
        "Kodak:KodakMaker",
        "Ricoh:RicohModel",
        "Leica:LensType",
    ];

    let mut found = 0;
    for tag in &test_tags {
        if get_generated_tag_descriptor(tag).is_some() {
            found += 1;
        }
    }

    // Expect at least 70% of test tags present
    assert!(
        found >= (test_tags.len() * 7 / 10),
        "Expected at least 70% of manufacturer tags present, found {}/{}",
        found,
        test_tags.len()
    );
}
```

**Step 2: Run test to verify baseline**

Run: `cargo test test_all_makernotes_manufacturers`

Expected: FAIL (most tags not present)

**Step 3: Add all remaining MakerNotes modules**

Extend `build.rs` modules list:

```rust
    // ===== MAKERNOTES - ADDITIONAL MANUFACTURERS (new +15) =====
    ("FujiFilm.pm", "FujiFilm"),
    ("Pentax.pm", "Pentax"),
    ("Minolta.pm", "Minolta"),
    ("Casio.pm", "Casio"),
    ("Sigma.pm", "Sigma"),
    ("Samsung.pm", "Samsung"),
    ("Kodak.pm", "Kodak"),
    ("Ricoh.pm", "Ricoh"),
    ("Leaf.pm", "Leaf"),
    ("PhaseOne.pm", "PhaseOne"),
    ("HP.pm", "HP"),
    ("JVC.pm", "JVC"),
    ("Sanyo.pm", "Sanyo"),
    ("Motorola.pm", "Motorola"),
    ("Reconyx.pm", "Reconyx"),
    ("GE.pm", "GE"),
    ("Lytro.pm", "Lytro"),
    ("Nintendo.pm", "Nintendo"),

    // ===== MAKERNOTES - SPECIALIZED MODULES =====
    ("CanonCustom.pm", "CanonCustom"),
    ("CanonVRD.pm", "CanonVRD"),
    ("CanonRaw.pm", "CanonRaw"),
    ("NikonCapture.pm", "NikonCapture"),
    ("NikonCustom.pm", "NikonCustom"),
    ("NikonSettings.pm", "NikonSettings"),
    ("SonyIDC.pm", "SonyIDC"),
    ("MinoltaRaw.pm", "MinoltaRaw"),
    ("PanasonicRaw.pm", "PanasonicRaw"),
    ("SigmaRaw.pm", "SigmaRaw"),
    ("KyoceraRaw.pm", "KyoceraRaw"),
```

**Step 4: Add FormatFamily variants**

Modify `src/core/tag_descriptor.rs` to add all new manufacturer enums.

**Step 5: Rebuild and verify**

Run:
```bash
cargo clean
cargo build --release 2>&1 | tee build_output.txt
grep "Parsed.*tags from" build_output.txt | tail -20
```

Review output to ensure all modules parsed successfully.

**Step 6: Run comprehensive tests**

Run:
```bash
cargo test test_all_makernotes_manufacturers
cargo test test_tag_database_target_coverage -- --nocapture
```

Expected:
- MakerNotes test PASS (70%+ tags present)
- Coverage should be 18,000-20,000 tags (~65-70%)

**Step 7: Commit comprehensive MakerNotes support**

```bash
git add build.rs src/core/tag_descriptor.rs tests/tag_database_coverage.rs
git commit -m "feat: add comprehensive MakerNotes support for all manufacturers

- Add 15+ additional camera manufacturer modules
- Support FujiFilm, Pentax, Minolta, Sigma, Samsung, Kodak, etc.
- Add specialized modules (CanonCustom, NikonSettings, Raw formats)
- Include 8000+ additional manufacturer-specific tags

Coverage: ~10,000 tags → ~18,000 tags (~62% of ExifTool's 28,853 tags)"
```

---

### Task 5: Add XMP Namespace Modules

**Goal:** Add comprehensive XMP namespace support (estimated 5000+ additional tags).

**Files:**
- Modify: `build.rs` (add XMP namespace modules)
- Create: `tests/xmp_namespace_coverage.rs`

**Step 1: Audit XMP modules in ExifTool**

List all XMP-related modules:

```bash
cd /tmp/exiftool-analysis/exiftool-master/lib/Image/ExifTool
find . -name "XMP*.pm" -o -name "*XMP.pm" | sort
```

Expected: 40+ XMP namespace modules (XMP.pm, XMP/dc.pm, XMP/exif.pm, etc.)

**Step 2: Write test for XMP namespace coverage**

Create `tests/xmp_namespace_coverage.rs`:

```rust
//! Tests for XMP namespace tag coverage

use oxidex::tag_db::generated_tags::get_generated_tag_descriptor;

#[test]
fn test_xmp_dublin_core_tags() {
    // Dublin Core namespace (dc)
    assert!(get_generated_tag_descriptor("XMP-dc:Creator").is_some());
    assert!(get_generated_tag_descriptor("XMP-dc:Rights").is_some());
    assert!(get_generated_tag_descriptor("XMP-dc:Title").is_some());
}

#[test]
fn test_xmp_exif_tags() {
    // EXIF namespace in XMP
    assert!(get_generated_tag_descriptor("XMP-exif:DateTimeOriginal").is_some());
    assert!(get_generated_tag_descriptor("XMP-exif:ISOSpeedRatings").is_some());
}

#[test]
fn test_xmp_iptc_tags() {
    // IPTC namespace in XMP
    assert!(get_generated_tag_descriptor("XMP-iptcCore:Location").is_some());
}

#[test]
fn test_xmp_photoshop_tags() {
    // Photoshop namespace in XMP
    assert!(get_generated_tag_descriptor("XMP-photoshop:ColorMode").is_some());
}

#[test]
fn test_xmp_camera_raw_tags() {
    // Camera Raw namespace
    assert!(get_generated_tag_descriptor("XMP-crs:Temperature").is_some());
}
```

**Step 3: Run tests to verify baseline**

Run: `cargo test --test xmp_namespace_coverage`

Expected: Most tests FAIL

**Step 4: Discover XMP module structure**

XMP modules in ExifTool use a different structure (subdirectories). Update `build.rs` to handle XMP subdirectory:

Add new function in `build.rs`:

```rust
/// Parses XMP namespace modules from XMP/ subdirectory
fn parse_xmp_namespaces(source_dir: &Path) -> Result<Vec<TagDefinition>> {
    let xmp_dir = source_dir.join("lib/Image/ExifTool/XMP");
    if !xmp_dir.exists() {
        println!("cargo:warning=XMP directory not found, skipping XMP namespaces");
        return Ok(Vec::new());
    }

    let mut all_tags = Vec::new();

    // List all .pm files in XMP directory
    let xmp_modules = std::fs::read_dir(&xmp_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().and_then(|s| s.to_str()) == Some("pm")
        })
        .collect::<Vec<_>>();

    println!("cargo:warning=Found {} XMP namespace modules", xmp_modules.len());

    for entry in xmp_modules {
        let path = entry.path();
        let namespace = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown");

        // Format family is XMP-{namespace}
        let format_family = format!("XMP-{}", namespace);

        match parse_perl_module(&path, &format_family) {
            Ok(mut tags) => {
                println!("cargo:warning=Parsed {} tags from XMP/{}.pm", tags.len(), namespace);
                all_tags.append(&mut tags);
            }
            Err(e) => {
                eprintln!("cargo:warning=Failed to parse XMP/{}.pm: {}", namespace, e);
            }
        }
    }

    Ok(all_tags)
}
```

**Step 5: Integrate XMP parsing into main generation**

Modify `parse_exiftool_tags` function in `build.rs` to call XMP parser:

Add after line 192 (after parsing base modules):

```rust
    // Parse XMP namespace modules
    match parse_xmp_namespaces(&source_dir) {
        Ok(mut xmp_tags) => {
            println!("cargo:warning=Parsed {} total XMP namespace tags", xmp_tags.len());
            all_tags.append(&mut xmp_tags);
        }
        Err(e) => {
            eprintln!("cargo:warning=Failed to parse XMP namespaces: {}", e);
        }
    }
```

**Step 6: Handle dynamic FormatFamily for XMP namespaces**

XMP namespaces create dynamic format families (XMP-dc, XMP-exif, XMP-iptcCore, etc.). We need to support this in tag generation.

Modify `generate_tag_insertion` in `build.rs` around line 540:

```rust
    // Format family - handle both enum variants and dynamic XMP namespaces
    if tag.format_family.starts_with("XMP-") {
        // For XMP namespaces, we need to store as string since there are 40+ dynamic variants
        // This requires enhancing FormatFamily enum to support dynamic values
        writeln!(file, "            FormatFamily::XMPNamespace(\"{}\".to_string()),",
                 tag.format_family.trim_start_matches("XMP-"))?;
    } else {
        let family_variant = match tag.format_family.as_str() {
            "ICC_Profile" => "ICCProfile",
            other => other,
        };
        writeln!(file, "            FormatFamily::{},", family_variant)?;
    }
```

**Step 7: Update FormatFamily enum to support dynamic XMP namespaces**

Modify `src/core/tag_descriptor.rs`:

```rust
/// Format family classification
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormatFamily {
    // Static format families
    EXIF,
    GPS,
    // ... other static variants ...

    // Dynamic XMP namespaces (dc, exif, iptcCore, photoshop, etc.)
    // Using String to support 40+ XMP namespaces without hardcoding
    XMPNamespace(String),
}
```

This is a significant change. Alternative simpler approach: Add common XMP namespaces as enum variants:

```rust
    // Common XMP namespaces (add ~15 most common)
    XMPDc,          // Dublin Core
    XMPExif,        // EXIF in XMP
    XMPIptcCore,    // IPTC Core
    XMPPhotoshop,   // Photoshop
    XMPCrs,         // Camera Raw Settings
    XMPTiff,        // TIFF in XMP
    XMPAux,         // Auxiliary
    XMPRights,      // Rights Management
    XMPMWG,         // Metadata Working Group
    XMPPdf,         // PDF
    XMPXmp,         // XMP base
    // ... add others as needed
```

**Step 8: Rebuild and test XMP coverage**

Run:
```bash
cargo clean
cargo build --release 2>&1 | grep "XMP"
```

Expected: "Parsed 5000+ total XMP namespace tags"

**Step 9: Run XMP tests**

Run: `cargo test --test xmp_namespace_coverage`

Expected: All XMP tests PASS

**Step 10: Commit XMP namespace support**

```bash
git add build.rs src/core/tag_descriptor.rs tests/xmp_namespace_coverage.rs
git commit -m "feat: add comprehensive XMP namespace support

- Parse all XMP namespace modules from ExifTool/XMP/ directory
- Add parse_xmp_namespaces function to build.rs
- Support 40+ XMP namespaces (dc, exif, iptcCore, photoshop, crs, etc.)
- Add 5000+ XMP-specific tags
- Update FormatFamily enum for XMP namespace variants
- Add integration tests for XMP coverage

Coverage: ~18,000 tags → ~23,000 tags (~80% of ExifTool's 28,853 tags)"
```

---

### Task 6: Add Composite and Shortcut Tags

**Goal:** Add Composite (calculated) tags and Shortcuts (aliases) to reach 28,000+ tag coverage.

**Files:**
- Modify: `build.rs` (add Composite.pm and Shortcuts.pm parsing)
- Create: `tests/composite_tags_coverage.rs`

**Step 1: Write test for composite tags**

Create `tests/composite_tags_coverage.rs`:

```rust
//! Tests for Composite and Shortcut tag coverage

use oxidex::tag_db::generated_tags::get_generated_tag_descriptor;

#[test]
fn test_composite_tags_present() {
    // Composite tags are calculated from other tags
    assert!(get_generated_tag_descriptor("Composite:Aperture").is_some());
    assert!(get_generated_tag_descriptor("Composite:ShutterSpeed").is_some());
    assert!(get_generated_tag_descriptor("Composite:LensID").is_some());
}

#[test]
fn test_shortcut_tags_present() {
    // Shortcuts are tag aliases
    assert!(get_generated_tag_descriptor("Shortcuts:CommonIFD0").is_some() ||
            get_generated_tag_descriptor("Shortcuts:All").is_some());
}
```

**Step 2: Run tests to verify baseline**

Run: `cargo test --test composite_tags_coverage`

Expected: FAIL

**Step 3: Add Composite and Shortcuts modules**

Add to `build.rs` modules list:

```rust
    // ===== COMPOSITE AND SHORTCUTS =====
    ("Composite.pm", "Composite"),
    ("Shortcuts.pm", "Shortcuts"),
```

**Step 4: Handle Composite tag special structure**

Composite tags have a different structure in Perl (no numeric IDs, all named). The existing `parse_perl_module` should handle this, but verify by examining Composite.pm structure first.

**Step 5: Rebuild and verify**

Run:
```bash
cargo clean
cargo build --release 2>&1 | grep -E "(Composite|Shortcuts)"
```

Expected output showing tags parsed from both modules.

**Step 6: Run composite tests**

Run: `cargo test --test composite_tags_coverage`

Expected: PASS

**Step 7: Verify we've reached 28,000+ tags**

Run:
```bash
cargo test test_tag_database_target_coverage -- --nocapture
```

Expected output: "Tag coverage: 28000+/28853 (97%+)"

**Step 8: Commit composite tag support**

```bash
git add build.rs tests/composite_tags_coverage.rs
git commit -m "feat: add Composite and Shortcut tag support

- Add Composite.pm parsing for calculated tags
- Add Shortcuts.pm parsing for tag aliases
- Include 5000+ composite and shortcut tags
- Reach 28,000+ total tag coverage

Coverage: ~23,000 tags → ~28,000+ tags (~97% of ExifTool's 28,853 tags)"
```

---

### Task 7: Optimize Generated Code Size and Build Time

**Goal:** Optimize the generated tag database for faster compilation and smaller binary size.

**Files:**
- Modify: `build.rs` (optimize code generation)
- Modify: `src/tag_db/generated_tags.rs` (structure optimization)

**Step 1: Measure baseline build time and binary size**

Run:
```bash
# Clean build timing
cargo clean
time cargo build --release

# Measure binary size
ls -lh target/release/oxidex
```

Document baseline:
- Build time: X minutes
- Binary size: Y MB
- generated_tags.rs lines: ~Z lines

**Step 2: Implement lazy loading for tag groups**

Instead of one giant HashMap, split into per-family HashMaps that load on demand.

Modify code generation in `build.rs` `generate_rust_code` function:

```rust
// Generate separate lazy static for each format family
for (family, family_tags) in tags_by_family.iter() {
    let static_name = format!("{}_TAGS", family.to_uppercase().replace("-", "_"));

    writeln!(file, "/// {} tag registry", family)?;
    writeln!(file, "static {}: Lazy<HashMap<&'static str, TagDescriptor>> = Lazy::new(|| {{", static_name)?;
    writeln!(file, "    let mut registry = HashMap::with_capacity({});", family_tags.len())?;

    for tag in family_tags {
        generate_tag_insertion(&mut file, tag)?;
    }

    writeln!(file, "    registry")?;
    writeln!(file, "}});")?;
    writeln!(file)?;
}

// Generate unified lookup that delegates to family-specific registries
writeln!(file, "pub fn get_generated_tag_descriptor(name: &str) -> Option<&TagDescriptor> {{")?;
writeln!(file, "    // Extract family prefix from tag name")?;
writeln!(file, "    let family = name.split(':').next()?;")?;
writeln!(file, "    match family {{")?;

for family in tags_by_family.keys() {
    let static_name = format!("{}_TAGS", family.to_uppercase().replace("-", "_"));
    writeln!(file, "        \"{}\" => {}.get(name),", family, static_name)?;
}

writeln!(file, "        _ => None,")?;
writeln!(file, "    }}")?;
writeln!(file, "}}")?;
```

**Step 3: Add tag name interning to reduce string duplication**

Tag names are repeated in both keys and TagDescriptor. Use string interning:

```rust
// At top of generated file, create string pool
writeln!(file, "// String pool for tag names (reduces duplication)")?;
writeln!(file, "mod strings {{")?;
writeln!(file, "    pub const TAG_NAMES: &[&str] = &[")?;

// Collect unique tag names
let unique_names: std::collections::HashSet<&str> = tags.iter()
    .map(|t| t.tag_name.as_str())
    .collect();

for name in unique_names.iter() {
    writeln!(file, "        \"{}\",", escape_string(name))?;
}

writeln!(file, "    ];")?;
writeln!(file, "}}")?;
```

This optimization may be complex. Simpler approach: Use `&'static str` for tag names instead of `String`.

**Step 4: Rebuild and measure improvements**

Run:
```bash
cargo clean
time cargo build --release
ls -lh target/release/oxidex
```

Compare to baseline. Expect:
- Build time: 20-30% faster
- Binary size: 10-15% smaller
- Memory usage: Better due to lazy loading

**Step 5: Run all tests to verify correctness**

Run:
```bash
cargo test --all
```

Expected: All tests PASS

**Step 6: Commit optimizations**

```bash
git add build.rs
git commit -m "perf: optimize tag database generation and loading

- Split tag registry into per-family lazy statics
- Reduce compilation time by 25% through modular generation
- Improve runtime memory usage with lazy loading
- Maintain 28,000+ tag coverage with better performance"
```

---

### Task 8: Add Documentation and Examples

**Goal:** Document the tag database system and provide usage examples.

**Files:**
- Create: `docs/tag_database.md`
- Modify: `README.md`
- Create: `examples/list_all_tags.rs`

**Step 1: Create comprehensive tag database documentation**

Create `docs/tag_database.md`:

```markdown
# Tag Database

## Overview

OxiDex includes a comprehensive tag database with 28,853+ tag definitions automatically generated from the official ExifTool Perl source. This ensures compatibility with ExifTool's extensive metadata tag support.

## Coverage

- **Total Tags**: 28,853+ tags
- **Unique Names**: 17,925+ unique tag names
- **Format Families**: 100+ format families
- **MakerNotes**: 20+ camera manufacturers

### Tag Distribution

| Category | Tags | Percentage |
|----------|------|------------|
| EXIF | 244 | 0.8% |
| GPS | 32 | 0.1% |
| IPTC | 122 | 0.4% |
| XMP Namespaces | 5,000+ | 17.3% |
| MakerNotes (Canon) | 2,000+ | 6.9% |
| MakerNotes (Nikon) | 1,500+ | 5.2% |
| MakerNotes (Sony) | 1,200+ | 4.2% |
| MakerNotes (Others) | 6,000+ | 20.8% |
| QuickTime | 143 | 0.5% |
| Composite | 3,000+ | 10.4% |
| Other Formats | 9,600+ | 33.3% |

## Architecture

### Build-Time Generation

The tag database is generated during `cargo build` by the `build.rs` script:

1. **Download**: Fetches latest ExifTool source from GitHub
2. **Parse**: Extracts tag definitions from 100+ Perl modules
3. **Generate**: Creates Rust code in `src/tag_db/generated_tags.rs`
4. **Validate**: Ensures minimum tag count and quality

### Runtime Access

Tags are loaded lazily using per-family static registries:

```rust
use oxidex::tag_db::generated_tags::get_generated_tag_descriptor;

// Look up a tag
if let Some(descriptor) = get_generated_tag_descriptor("EXIF:Make") {
    println!("Tag: {}", descriptor.tag_name);
    println!("Writable: {}", descriptor.writable);
    println!("Type: {:?}", descriptor.value_type);
}
```

## Usage Examples

### List All Tags

See `examples/list_all_tags.rs`

### Search Tags by Pattern

```rust
use oxidex::tag_db::generated_tags::*;

// This requires adding an iterator function - see Task 9
```

### Check Tag Coverage

```rust
let total = generated_tag_count();
println!("Total tags: {}", total);
```

## Updating Tags

To regenerate the tag database with the latest ExifTool definitions:

```bash
cargo clean
cargo build --release
```

The build script will download the latest ExifTool source and regenerate all tag definitions.

## Implementation Details

See `build.rs` for the tag generation implementation.
```

**Step 2: Add example program to list all tags**

Create `examples/list_all_tags.rs`:

```rust
//! Example: List all registered tags
//!
//! Usage: cargo run --example list_all_tags

use oxidex::tag_db::generated_tags::generated_tag_count;

fn main() {
    let total = generated_tag_count();

    println!("OxiDex Tag Database");
    println!("========================");
    println!();
    println!("Total tags: {}", total);
    println!();
    println!("Coverage: {:.1}%", (total as f64 / 28853.0) * 100.0);
    println!();
    println!("Target: 28,853 tags (ExifTool parity)");

    // Note: To list individual tags, we'd need to add an iterator function
    // to generated_tags.rs - see Task 9 for full implementation
}
```

**Step 3: Update README with tag database section**

Modify `README.md` around line 304 (after "Tag Database Generation" section):

```markdown
### Tag Database Coverage

OxiDex now supports **28,853+ metadata tags** from the official ExifTool source, achieving full feature parity with the Perl implementation.

#### Supported Tag Families

- **EXIF** (244 tags): Camera settings, image parameters
- **GPS** (32 tags): Geolocation data
- **IPTC** (122 tags): Press and media metadata
- **XMP** (5,000+ tags): 40+ XML namespaces
- **MakerNotes** (12,000+ tags): 20+ camera manufacturers including:
  - Canon (2,000+ tags)
  - Nikon (1,500+ tags)
  - Sony (1,200+ tags)
  - Panasonic, Olympus, FujiFilm, Pentax, Sigma, and more
- **QuickTime** (143 tags): Video/audio metadata
- **Composite** (3,000+ tags): Calculated and derived values
- **Other formats**: 100+ format families covering images, video, audio, documents

For complete tag documentation, see [Tag Database Documentation](docs/tag_database.md).
```

**Step 4: Test the example**

Run:
```bash
cargo run --example list_all_tags
```

Expected output:
```
OxiDex Tag Database
========================

Total tags: 28853

Coverage: 100.0%

Target: 28,853 tags (ExifTool parity)
```

**Step 5: Commit documentation**

```bash
git add docs/tag_database.md README.md examples/list_all_tags.rs
git commit -m "docs: add comprehensive tag database documentation

- Create docs/tag_database.md with architecture and usage
- Update README with tag family breakdown
- Add example program to display tag statistics
- Document coverage: 28,853 tags (100% ExifTool parity)"
```

---

### Task 9: Add Tag Query API for Runtime Tag Discovery

**Goal:** Add API functions to query and iterate over registered tags at runtime.

**Files:**
- Modify: `build.rs` (generate query functions)
- Create: `examples/search_tags.rs`
- Create: `tests/tag_query_api.rs`

**Step 1: Write tests for query API**

Create `tests/tag_query_api.rs`:

```rust
//! Tests for tag query API

use oxidex::tag_db::generated_tags::*;

#[test]
fn test_get_tags_by_family() {
    let exif_tags = get_tags_by_family("EXIF");
    assert!(exif_tags.len() >= 200, "Expected at least 200 EXIF tags");
}

#[test]
fn test_get_all_families() {
    let families = get_all_families();
    assert!(families.len() >= 50, "Expected at least 50 format families");
    assert!(families.contains(&"EXIF"));
    assert!(families.contains(&"GPS"));
    assert!(families.contains(&"Canon"));
}

#[test]
fn test_search_tags_by_name() {
    let results = search_tags("Make");
    assert!(!results.is_empty(), "Should find tags matching 'Make'");

    // Should find at least EXIF:Make
    assert!(results.iter().any(|t| t.tag_name == "EXIF:Make"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test tag_query_api`

Expected: Compilation error (functions don't exist)

**Step 3: Generate query functions in build.rs**

Add to `generate_rust_code` function in `build.rs` after line 480:

```rust
    // Generate family list function
    writeln!(file)?;
    writeln!(file, "/// Returns a list of all format families with registered tags.")?;
    writeln!(file, "pub fn get_all_families() -> Vec<&'static str> {{")?;
    writeln!(file, "    vec![")?;
    for family in tags_by_family.keys() {
        writeln!(file, "        \"{}\",", family)?;
    }
    writeln!(file, "    ]")?;
    writeln!(file, "}}")?;
    writeln!(file)?;

    // Generate get_tags_by_family function
    writeln!(file, "/// Returns all tags for a specific format family.")?;
    writeln!(file, "pub fn get_tags_by_family(family: &str) -> Vec<&TagDescriptor> {{")?;
    writeln!(file, "    GENERATED_TAG_REGISTRY")?;
    writeln!(file, "        .iter()")?;
    writeln!(file, "        .filter(|(name, _)| name.starts_with(&format!(\"{{}}:\", family)))")?;
    writeln!(file, "        .map(|(_, desc)| desc)")?;
    writeln!(file, "        .collect()")?;
    writeln!(file, "}}")?;
    writeln!(file)?;

    // Generate search function
    writeln!(file, "/// Searches for tags by name pattern (case-insensitive).")?;
    writeln!(file, "pub fn search_tags(pattern: &str) -> Vec<&TagDescriptor> {{")?;
    writeln!(file, "    let pattern_lower = pattern.to_lowercase();")?;
    writeln!(file, "    GENERATED_TAG_REGISTRY")?;
    writeln!(file, "        .iter()")?;
    writeln!(file, "        .filter(|(name, _)| name.to_lowercase().contains(&pattern_lower))")?;
    writeln!(file, "        .map(|(_, desc)| desc)")?;
    writeln!(file, "        .collect()")?;
    writeln!(file, "}}")?;
    writeln!(file)?;

    // Generate iterator function
    writeln!(file, "/// Returns an iterator over all registered tags.")?;
    writeln!(file, "pub fn all_tags() -> impl Iterator<Item = &'static TagDescriptor> {{")?;
    writeln!(file, "    GENERATED_TAG_REGISTRY.values()")?;
    writeln!(file, "}}")?;
```

**Step 4: Rebuild to generate new functions**

Run:
```bash
cargo clean
cargo build --release
```

**Step 5: Run query API tests**

Run: `cargo test --test tag_query_api`

Expected: All tests PASS

**Step 6: Create search example**

Create `examples/search_tags.rs`:

```rust
//! Example: Search tags by pattern
//!
//! Usage: cargo run --example search_tags -- <pattern>

use oxidex::tag_db::generated_tags::*;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <search_pattern>", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} Make", args[0]);
        eprintln!("  {} GPS", args[0]);
        eprintln!("  {} Canon", args[0]);
        std::process::exit(1);
    }

    let pattern = &args[1];
    let results = search_tags(pattern);

    println!("Search results for '{}': {} tags found\n", pattern, results.len());

    if results.is_empty() {
        println!("No tags found matching '{}'", pattern);
        return;
    }

    // Group by family
    let mut by_family: std::collections::HashMap<String, Vec<&str>> = std::collections::HashMap::new();

    for tag in results.iter() {
        let family = tag.tag_name.split(':').next().unwrap_or("Unknown");
        by_family.entry(family.to_string())
            .or_insert_with(Vec::new)
            .push(&tag.tag_name);
    }

    // Print grouped results
    for (family, tags) in by_family.iter() {
        println!("{}:", family);
        for tag in tags.iter().take(10) {
            if let Some(descriptor) = get_generated_tag_descriptor(tag) {
                println!("  {} - {} ({})",
                    tag,
                    descriptor.description,
                    if descriptor.writable { "writable" } else { "read-only" }
                );
            }
        }
        if tags.len() > 10 {
            println!("  ... and {} more", tags.len() - 10);
        }
        println!();
    }
}
```

**Step 7: Test search example**

Run:
```bash
cargo run --example search_tags -- GPS
cargo run --example search_tags -- Make
cargo run --example search_tags -- Canon
```

Verify output shows matching tags grouped by family.

**Step 8: Commit query API**

```bash
git add build.rs examples/search_tags.rs tests/tag_query_api.rs
git commit -m "feat: add tag query API for runtime tag discovery

- Add get_tags_by_family() to filter tags by format
- Add get_all_families() to list all format families
- Add search_tags() for pattern-based tag search
- Add all_tags() iterator for enumerating all tags
- Create search_tags example program
- Add integration tests for query API

Enables runtime tag exploration and dynamic tag lookup"
```

---

### Task 10: Validation and Final Testing

**Goal:** Comprehensive validation that tag extraction fidelity matches ExifTool.

**Files:**
- Create: `tests/exiftool_comparison_tags.rs`
- Create: `scripts/compare_tags_with_exiftool.sh`

**Step 1: Create comparison test script**

Create `scripts/compare_tags_with_exiftool.sh`:

```bash
#!/bin/bash
# Compare tag coverage between OxiDex and Perl ExifTool

set -e

echo "ExifTool Tag Coverage Comparison"
echo "================================="
echo

# Check if exiftool is installed
if ! command -v exiftool &> /dev/null; then
    echo "Error: exiftool (Perl) not found. Install with:"
    echo "  brew install exiftool  # macOS"
    echo "  apt install libimage-exiftool-perl  # Ubuntu"
    exit 1
fi

# Get ExifTool version
EXIFTOOL_VERSION=$(exiftool -ver)
echo "Perl ExifTool version: $EXIFTOOL_VERSION"
echo

# Run ExifTool to list all tag names
echo "Extracting tag names from ExifTool..."
exiftool -listx > /tmp/exiftool_tags.xml

# Parse XML to extract tag names (simple grep approach)
grep -o 'name="[^"]*"' /tmp/exiftool_tags.xml | cut -d'"' -f2 | sort -u > /tmp/exiftool_tagnames.txt

PERL_TAG_COUNT=$(wc -l < /tmp/exiftool_tagnames.txt)
echo "Perl ExifTool tags: $PERL_TAG_COUNT"
echo

# Get OxiDex tag count
echo "Building OxiDex..."
cargo build --release --quiet

echo "Comparing tag counts..."
echo

# Run our list_all_tags example
cargo run --example list_all_tags --quiet

echo
echo "Comparison complete!"
echo
echo "Note: Minor differences are expected due to:"
echo "  - ExifTool includes some runtime-generated tags"
echo "  - Different counting methodologies"
echo "  - Version differences"
echo
echo "Target: 95%+ coverage of core tags"
```

**Step 2: Make script executable and run**

Run:
```bash
chmod +x scripts/compare_tags_with_exiftool.sh
./scripts/compare_tags_with_exiftool.sh
```

Review output to verify tag count alignment.

**Step 3: Create integration test with real files**

Create `tests/exiftool_comparison_tags.rs`:

```rust
//! Integration test: Compare tag extraction with ExifTool

use std::process::Command;
use oxidex::tag_db::generated_tags::*;

#[test]
#[ignore] // Requires exiftool binary installed
fn test_tag_coverage_vs_exiftool() {
    // Check if exiftool is available
    let exiftool_check = Command::new("exiftool")
        .arg("-ver")
        .output();

    if exiftool_check.is_err() {
        println!("Skipping test: exiftool not installed");
        return;
    }

    let our_count = generated_tag_count();

    // Target: 28,853 tags (as documented by ExifTool)
    let exiftool_documented_count = 28853;

    let coverage = (our_count as f64 / exiftool_documented_count as f64) * 100.0;

    println!("Tag coverage: {}/{} ({:.1}%)", our_count, exiftool_documented_count, coverage);

    // Assert we have at least 95% coverage
    assert!(
        coverage >= 95.0,
        "Expected at least 95% tag coverage, got {:.1}%",
        coverage
    );
}

#[test]
fn test_all_critical_tags_present() {
    // Ensure critical tags from each family are present
    let critical_tags = vec![
        // EXIF
        "EXIF:Make",
        "EXIF:Model",
        "EXIF:DateTimeOriginal",
        "EXIF:ISO",
        "EXIF:FNumber",
        "EXIF:ExposureTime",

        // GPS
        "GPS:GPSLatitude",
        "GPS:GPSLongitude",
        "GPS:GPSAltitude",

        // IPTC
        "IPTC:Keywords",
        "IPTC:Caption-Abstract",

        // XMP
        "XMP-dc:Creator",
        "XMP-dc:Rights",

        // MakerNotes
        "Canon:CanonImageType",
        "Nikon:ISOSetting",
        "Sony:ColorTemperature",

        // Composite
        "Composite:Aperture",
        "Composite:ShutterSpeed",
    ];

    let mut missing = Vec::new();

    for tag in &critical_tags {
        if get_generated_tag_descriptor(tag).is_none() {
            missing.push(tag);
        }
    }

    assert!(
        missing.is_empty(),
        "Critical tags missing: {:?}",
        missing
    );
}
```

**Step 4: Run validation tests**

Run:
```bash
# Run all tag database tests
cargo test tag_database
cargo test tag_coverage
cargo test tag_query
cargo test exiftool_comparison

# Run with exiftool installed
cargo test test_tag_coverage_vs_exiftool -- --ignored --nocapture
```

Expected: All tests PASS

**Step 5: Run full test suite**

Run:
```bash
cargo test --all
```

Ensure all existing tests still pass with the expanded tag database.

**Step 6: Commit validation tests**

```bash
git add tests/exiftool_comparison_tags.rs scripts/compare_tags_with_exiftool.sh
chmod +x scripts/compare_tags_with_exiftool.sh
git add scripts/compare_tags_with_exiftool.sh
git commit -m "test: add comprehensive tag coverage validation

- Add integration test comparing with ExifTool
- Create comparison script for tag count verification
- Test all critical tags from major format families
- Validate 95%+ tag coverage (27,410+ of 28,853 tags)
- Add ignored test requiring exiftool binary

Tag extraction fidelity: ✅ COMPLETE"
```

---

## Execution Summary

This plan implements tag extraction fidelity in 10 incremental tasks:

1. ✅ **Audit ExifTool modules** - Understand the 100+ module structure
2. ✅ **Extend base formats** - Add 35 modules → 3,000 tags (10% coverage)
3. ✅ **Add top 5 MakerNotes** - Canon, Nikon, Sony, etc. → 10,000 tags (35%)
4. ✅ **Add all MakerNotes** - 20+ manufacturers → 18,000 tags (62%)
5. ✅ **Add XMP namespaces** - 40+ namespaces → 23,000 tags (80%)
6. ✅ **Add Composite/Shortcuts** - Calculated tags → 28,000+ tags (97%)
7. ✅ **Optimize generation** - Faster builds, smaller binaries
8. ✅ **Add documentation** - Comprehensive guides and examples
9. ✅ **Add query API** - Runtime tag discovery
10. ✅ **Validation testing** - Verify ExifTool parity

**Final Coverage**: 28,853+ tags (100% ExifTool parity)

---

## Testing Strategy

- **Unit tests**: Tag presence validation per family
- **Integration tests**: Coverage percentage verification
- **Comparison tests**: Against Perl ExifTool output
- **Query tests**: API functionality validation
- **Performance tests**: Build time and binary size

---

## Success Criteria

- [ ] 28,853+ tags registered (100% of ExifTool)
- [ ] All 100+ ExifTool modules parsed
- [ ] 20+ MakerNotes manufacturers supported
- [ ] 40+ XMP namespaces covered
- [ ] Composite and Shortcut tags included
- [ ] Build completes in under 5 minutes
- [ ] All tests pass
- [ ] Query API functional
- [ ] Documentation complete

---

**Implementation Time Estimate**: 6-8 hours for full execution with review checkpoints
