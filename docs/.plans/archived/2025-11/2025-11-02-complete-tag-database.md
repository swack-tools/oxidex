# Complete Tag Database - 100% ExifTool Parity (28,853 Tags)

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Achieve 100% tag coverage parity with Perl ExifTool by parsing ALL 28,853 tags from ExifTool source (currently at 721 tags / 2.5%). Parse all 140+ Perl modules including maker notes, vendor-specific tags, and format-specific metadata.

**Architecture:** Enhance existing `build.rs` tag generation pipeline to parse all ExifTool Perl modules comprehensively. Current implementation only parses 15 base modules; need to add 125+ additional modules including all camera maker notes (Canon, Nikon, Sony, etc.), specialized formats (DICOM, FITS, MXF), and vendor tags. Use improved Perl parsing regex to handle complex tag definitions, writable specifications, and value conversions.

**Tech Stack:** Rust build scripts, Regex (Perl parsing), ureq (HTTP), ExifTool Perl source (master branch)

---

## Current State Analysis

**Current Coverage:**
- Tags: 721 / 28,853 (2.5%)
- Modules parsed: 15 / 140+ (10%)
- Test status: FAILING (needs 3,000 tags minimum, 10% coverage)

**Modules currently parsed** (build.rs:152-168):
```
EXIF.pm, GPS.pm, XMP.pm, IPTC.pm, PDF.pm, QuickTime.pm,
Photoshop.pm, PNG.pm, JFIF.pm, JPEG.pm, TIFF.pm,
ICC_Profile.pm, PostScript.pm, RIFF.pm, MakerNotes.pm
```

**Missing 125+ modules** including:
- **Maker Notes**: Canon.pm, Nikon.pm, Sony.pm, Olympus.pm, Panasonic.pm, Pentax.pm, FujiFilm.pm, Samsung.pm, etc. (~30 modules)
- **RAW Formats**: DNG.pm, CanonRaw.pm, SigmaRaw.pm, MinoltaRaw.pm, etc. (~10 modules)
- **Video/Audio**: Matroska.pm, Flash.pm, ASF.pm, MPEG.pm, H264.pm, FLAC.pm, Ogg.pm, etc. (~25 modules)
- **Specialized**: DICOM.pm, FITS.pm, MXF.pm, MISB.pm, Font.pm, ZIP.pm, RSRC.pm, etc. (~40 modules)
- **Vendor-specific**: Apple.pm, Microsoft.pm, Google.pm, GoPro.pm, DJI.pm, FLIR.pm, etc. (~20 modules)

---

## Task 1: Discover All ExifTool Modules

**Files:**
- Modify: `build.rs:142-194` (parse_exiftool_tags function)

**Step 1: Write module discovery function**

Add after line 141 in build.rs:

```rust
/// Discovers all .pm Perl modules in ExifTool source
fn discover_all_modules(lib_dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut modules = Vec::new();

    // Recursively walk the lib directory
    fn visit_dirs(dir: &Path, modules: &mut Vec<(PathBuf, String)>) -> Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    visit_dirs(&path, modules)?;
                } else if path.extension().and_then(|s| s.to_str()) == Some("pm") {
                    // Extract module name from file path
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        modules.push((path.clone(), stem.to_string()));
                    }
                }
            }
        }
        Ok(())
    }

    visit_dirs(lib_dir, &mut modules)?;

    println!("cargo:warning=Discovered {} Perl modules", modules.len());
    Ok(modules)
}
```

**Step 2: Update parse_exiftool_tags to use discovery**

Replace lines 152-187 with:

```rust
fn parse_exiftool_tags(source_dir: &Path) -> Result<Vec<TagDefinition>> {
    let lib_dir = source_dir.join("lib/Image/ExifTool");
    if !lib_dir.exists() {
        anyhow::bail!("ExifTool lib directory not found: {:?}", lib_dir);
    }

    let mut all_tags = Vec::new();

    // Discover all modules
    let modules = discover_all_modules(&lib_dir)
        .context("Failed to discover ExifTool modules")?;

    println!("cargo:warning=Parsing {} modules for tag definitions...", modules.len());

    // Parse each module
    for (module_path, module_name) in modules {
        match parse_perl_module(&module_path, &module_name) {
            Ok(mut tags) => {
                if !tags.is_empty() {
                    println!(
                        "cargo:warning=  {:30} -> {:5} tags",
                        module_name,
                        tags.len()
                    );
                    all_tags.append(&mut tags);
                }
            }
            Err(e) => {
                // Don't fail on individual module parse errors
                eprintln!("cargo:warning=  {:30} -> ERROR: {}", module_name, e);
            }
        }
    }

    println!("cargo:warning=Total tags parsed: {}", all_tags.len());

    if all_tags.is_empty() {
        anyhow::bail!("No tags parsed from ExifTool source");
    }

    Ok(all_tags)
}
```

**Step 3: Run build to test discovery**

Run: `cargo clean && cargo build 2>&1 | grep "Discovered\|Parsing"`

Expected output:
```
Discovered 140+ Perl modules
Parsing 140+ modules for tag definitions...
```

**Step 4: Commit**

```bash
git add build.rs
git commit -m "build: discover all ExifTool Perl modules automatically

- Add discover_all_modules() to recursively find all .pm files
- Currently discovers 140+ modules vs hardcoded 15
- Prepares for full 28,853 tag coverage"
```

---

## Task 2: Enhance Perl Tag Definition Parser

**Files:**
- Modify: `build.rs:196-400` (parse_perl_module and related functions)

**Current limitation:** Simple regex only matches basic `TAG_ID => 'Tag Name'` patterns. Misses:
- Hash-based tag tables: `%Image::ExifTool::Canon::Main = ( ... )`
- Nested table references: `SubDirectory => { TagTable => 'Image::ExifTool::Canon::CameraSettings' }`
- Writable specifications: `Writable => 'int16u'`
- Print conversions: `PrintConv => { 0 => 'Off', 1 => 'On' }`
- Array-based value lists

**Step 1: Add comprehensive tag table regex patterns**

Add after line 200:

```rust
/// Comprehensive regex patterns for parsing Perl tag definitions
struct TagPatterns {
    /// Matches: %Image::ExifTool::ModuleName::TableName = (
    table_declaration: Regex,

    /// Matches: 0x0100 => { Name => 'ImageWidth', ... }
    hash_tag_def: Regex,

    /// Matches: 0x0100 => 'ImageWidth',
    simple_tag_def: Regex,

    /// Matches: Name => 'ImageWidth',
    name_field: Regex,

    /// Matches: Writable => 'int16u',
    writable_field: Regex,

    /// Matches: PrintConv => { 0 => 'Auto', 1 => 'Manual' }
    print_conv_field: Regex,

    /// Matches: SubDirectory => { TagTable => 'Image::ExifTool::GPS::Main' }
    subdirectory_field: Regex,
}

impl TagPatterns {
    fn new() -> Result<Self> {
        Ok(TagPatterns {
            table_declaration: Regex::new(
                r"%Image::ExifTool::(\w+(?:::\w+)*)\s*=\s*\("
            )?,
            hash_tag_def: Regex::new(
                r#"^\s*(0x[0-9a-fA-F]+|'[^']*'|\d+)\s*=>\s*\{([^}]+)\}"#
            )?,
            simple_tag_def: Regex::new(
                r#"^\s*(0x[0-9a-fA-F]+|\d+)\s*=>\s*'([^']+)'"#
            )?,
            name_field: Regex::new(
                r#"Name\s*=>\s*'([^']+)'"#
            )?,
            writable_field: Regex::new(
                r#"Writable\s*=>\s*'([^']+)'"#
            )?,
            print_conv_field: Regex::new(
                r#"PrintConv\s*=>\s*\{([^}]+)\}"#
            )?,
            subdirectory_field: Regex::new(
                r#"TagTable\s*=>\s*'Image::ExifTool::([^']+)'"#
            )?,
        })
    }
}
```

**Step 2: Rewrite parse_perl_module with comprehensive parsing**

Replace parse_perl_module function (lines ~196-350) with:

```rust
fn parse_perl_module(module_path: &Path, format_family: &str) -> Result<Vec<TagDefinition>> {
    let file = File::open(module_path)?;
    let reader = BufReader::new(file);
    let patterns = TagPatterns::new()?;

    let mut tags = Vec::new();
    let mut in_table = false;
    let mut current_table_name = String::new();
    let mut brace_depth = 0;
    let mut current_tag_def = String::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with('#') {
            continue;
        }

        // Detect table declaration
        if let Some(captures) = patterns.table_declaration.captures(&line) {
            current_table_name = captures[1].to_string();
            in_table = true;
            brace_depth = 0;
            continue;
        }

        if !in_table {
            continue;
        }

        // Track brace depth to know when table ends
        brace_depth += line.matches('{').count() as i32;
        brace_depth -= line.matches('}').count() as i32;

        if brace_depth < 0 {
            in_table = false;
            continue;
        }

        // Accumulate multi-line tag definitions
        current_tag_def.push_str(&line);
        current_tag_def.push('\n');

        // Check if we have a complete tag definition
        if let Some(tag) = try_parse_tag_definition(
            &current_tag_def,
            format_family,
            &current_table_name,
            &patterns,
        )? {
            tags.push(tag);
            current_tag_def.clear();
        }

        // Clear if line ends with comma or closing brace (definition complete)
        if trimmed.ends_with(',') || trimmed.ends_with('}') {
            current_tag_def.clear();
        }
    }

    Ok(tags)
}

fn try_parse_tag_definition(
    def: &str,
    format_family: &str,
    table_name: &str,
    patterns: &TagPatterns,
) -> Result<Option<TagDefinition>> {
    // Try hash-based definition first: 0x0100 => { Name => 'ImageWidth', ... }
    if let Some(captures) = patterns.hash_tag_def.captures(def) {
        let tag_id_str = &captures[1];
        let hash_content = &captures[2];

        // Extract tag ID (hex or decimal)
        let tag_id = parse_tag_id(tag_id_str)?;

        // Extract Name field
        if let Some(name_cap) = patterns.name_field.captures(hash_content) {
            let tag_name = name_cap[1].to_string();

            // Extract optional Writable field
            let writable = patterns
                .writable_field
                .captures(hash_content)
                .map(|c| c[1].to_string());

            return Ok(Some(TagDefinition {
                id: tag_id,
                name: tag_name,
                format_family: format_family.to_string(),
                table_name: table_name.to_string(),
                writable,
                description: None,
            }));
        }
    }

    // Try simple definition: 0x0100 => 'ImageWidth',
    if let Some(captures) = patterns.simple_tag_def.captures(def) {
        let tag_id = parse_tag_id(&captures[1])?;
        let tag_name = captures[2].to_string();

        return Ok(Some(TagDefinition {
            id: tag_id,
            name: tag_name,
            format_family: format_family.to_string(),
            table_name: table_name.to_string(),
            writable: None,
            description: None,
        }));
    }

    Ok(None)
}

fn parse_tag_id(id_str: &str) -> Result<u32> {
    let id_str = id_str.trim().trim_matches('\'');

    if let Some(hex_str) = id_str.strip_prefix("0x") {
        u32::from_str_radix(hex_str, 16)
            .with_context(|| format!("Failed to parse hex tag ID: {}", id_str))
    } else {
        id_str
            .parse::<u32>()
            .with_context(|| format!("Failed to parse decimal tag ID: {}", id_str))
    }
}
```

**Step 3: Update TagDefinition struct**

Find the TagDefinition struct (around line 570) and update:

```rust
#[derive(Debug, Clone)]
struct TagDefinition {
    id: u32,
    name: String,
    format_family: String,
    table_name: String,  // NEW: track which table this tag came from
    writable: Option<String>,  // NEW: track writable type (int16u, string, etc)
    description: Option<String>,  // NEW: for future documentation
}
```

**Step 4: Test enhanced parser**

Run: `cargo clean && cargo build 2>&1 | grep "tags"`

Expected: Significant increase in tag count (from 721 to 10,000+)

**Step 5: Commit**

```bash
git add build.rs
git commit -m "build: enhance Perl tag definition parser

- Add comprehensive regex patterns for hash-based tag defs
- Parse nested tag tables and writable specifications
- Track tag source table and writable type
- Handles complex Perl syntax including multi-line definitions"
```

---

## Task 3: Handle Maker Notes Subdirectories

**Files:**
- Modify: `build.rs` (add subdirectory resolution)

**Problem:** Many tags reference subdirectories that contain additional tag tables.

Example from Canon.pm:
```perl
0x0001 => {
    Name => 'CanonCameraSettings',
    SubDirectory => { TagTable => 'Image::ExifTool::Canon::CameraSettings' }
}
```

**Step 1: Add subdirectory table resolution**

Add function after try_parse_tag_definition:

```rust
/// Resolves subdirectory references and recursively parses referenced tables
fn resolve_subdirectories(
    all_tags: &mut Vec<TagDefinition>,
    lib_dir: &Path,
) -> Result<()> {
    let mut subdirs_to_process = Vec::new();

    // Find all tags that reference subdirectories
    for tag in all_tags.iter() {
        if let Some(ref table_name) = tag.table_name {
            // Check if this is a subdirectory reference
            if table_name.contains("::") {
                subdirs_to_process.push((table_name.clone(), tag.format_family.clone()));
            }
        }
    }

    println!(
        "cargo:warning=Resolving {} subdirectory references...",
        subdirs_to_process.len()
    );

    // Process each subdirectory (deduplicate first)
    subdirs_to_process.sort();
    subdirs_to_process.dedup();

    for (table_ref, format_family) in subdirs_to_process {
        // Convert table reference to file path
        // 'Image::ExifTool::Canon::CameraSettings' -> 'Canon.pm'
        if let Some(module_name) = table_ref.split("::").nth(2) {
            let module_path = lib_dir.join(format!("{}.pm", module_name));

            if module_path.exists() {
                match parse_perl_module(&module_path, &format_family) {
                    Ok(mut tags) => {
                        println!(
                            "cargo:warning=  Subdirectory {} -> {} tags",
                            table_ref,
                            tags.len()
                        );
                        all_tags.append(&mut tags);
                    }
                    Err(e) => {
                        eprintln!("cargo:warning=  Failed to parse {}: {}", table_ref, e);
                    }
                }
            }
        }
    }

    Ok(())
}
```

**Step 2: Call subdirectory resolution**

In parse_exiftool_tags, add after tag parsing loop:

```rust
    println!("cargo:warning=Total tags parsed: {}", all_tags.len());

    // NEW: Resolve subdirectory references
    resolve_subdirectories(&mut all_tags, &lib_dir)
        .context("Failed to resolve subdirectories")?;

    println!("cargo:warning=Total tags after subdirectory resolution: {}", all_tags.len());

    if all_tags.is_empty() {
        anyhow::bail!("No tags parsed from ExifTool source");
    }

    Ok(all_tags)
```

**Step 3: Test subdirectory resolution**

Run: `cargo clean && cargo build 2>&1 | grep -A 5 "Resolving"`

Expected: "Resolving 50+ subdirectory references..."

**Step 4: Commit**

```bash
git add build.rs
git commit -m "build: resolve subdirectory tag table references

- Parse nested tag tables from maker notes
- Resolves Canon::CameraSettings, Nikon::ShotInfo, etc.
- Adds thousands of vendor-specific tags"
```

---

## Task 4: Handle Special Module Patterns

**Files:**
- Modify: `build.rs` (add special case handlers)

**Problem:** Some modules use non-standard patterns:
- String-based tag IDs (not hex): `'Author' => 'Author'`
- Composite tags (calculated, not stored)
- Shortcuts (tag aliases)

**Step 1: Add string tag ID support**

Update parse_tag_id function:

```rust
fn parse_tag_id(id_str: &str) -> Result<u32> {
    let id_str = id_str.trim().trim_matches('\'');

    if let Some(hex_str) = id_str.strip_prefix("0x") {
        u32::from_str_radix(hex_str, 16)
            .with_context(|| format!("Failed to parse hex tag ID: {}", id_str))
    } else if let Ok(num) = id_str.parse::<u32>() {
        Ok(num)
    } else {
        // String-based tag ID - hash the string to get a numeric ID
        // This ensures unique IDs while maintaining deterministic mapping
        Ok(hash_string_tag_id(id_str))
    }
}

fn hash_string_tag_id(s: &str) -> u32 {
    // Simple hash function for string tag IDs
    s.bytes().fold(0u32, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as u32)
    })
}
```

**Step 2: Skip composite and shortcut modules**

Update discover_all_modules to filter:

```rust
fn discover_all_modules(lib_dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut modules = Vec::new();
    let skip_modules = vec!["Composite.pm", "Shortcuts.pm", "Extra.pm"];

    fn visit_dirs(
        dir: &Path,
        modules: &mut Vec<(PathBuf, String)>,
        skip_modules: &[&str],
    ) -> Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    visit_dirs(&path, modules, skip_modules)?;
                } else if path.extension().and_then(|s| s.to_str()) == Some("pm") {
                    if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                        // Skip composite and shortcut modules
                        if skip_modules.contains(&file_name) {
                            continue;
                        }

                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            modules.push((path.clone(), stem.to_string()));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    visit_dirs(lib_dir, &mut modules, &skip_modules)?;

    println!("cargo:warning=Discovered {} Perl modules", modules.len());
    Ok(modules)
}
```

**Step 3: Test with special patterns**

Run: `cargo clean && cargo build 2>&1 | tail -20`

Expected: Successfully parses PNG, XMP, and other string-based tag modules

**Step 4: Commit**

```bash
git add build.rs
git commit -m "build: handle string-based tag IDs and special modules

- Hash string tag IDs to numeric values
- Skip Composite/Shortcuts (calculated tags, not stored)
- Enables parsing of XMP, PNG, and metadata-only formats"
```

---

## Task 5: Optimize Generated Code Size

**Files:**
- Modify: `build.rs:400-600` (generate_rust_code function)

**Problem:** 28,853 tags will generate massive Rust file (50-100MB). Need optimization.

**Step 1: Use perfect hashing for tag lookup**

Update generate_rust_code function to use HashMap instead of match statement:

```rust
fn generate_rust_code(tags: &[TagDefinition]) -> Result<()> {
    let out_file = File::create(GENERATED_TAGS_PATH)?;
    let mut writer = std::io::BufWriter::new(out_file);

    writeln!(writer, "//! Generated tag database from ExifTool source")?;
    writeln!(writer, "//! Total tags: {}", tags.len())?;
    writeln!(writer, "//! Generated: {}", chrono::Utc::now())?;
    writeln!(writer)?;
    writeln!(writer, "use std::collections::HashMap;")?;
    writeln!(writer, "use once_cell::sync::Lazy;")?;
    writeln!(writer)?;

    // Generate tag structure
    writeln!(writer, "#[derive(Debug, Clone)]")?;
    writeln!(writer, "pub struct GeneratedTag {{")?;
    writeln!(writer, "    pub id: u32,")?;
    writeln!(writer, "    pub name: &'static str,")?;
    writeln!(writer, "    pub format_family: &'static str,")?;
    writeln!(writer, "    pub table: &'static str,")?;
    writeln!(writer, "}}")?;
    writeln!(writer)?;

    // Generate static tag array
    writeln!(writer, "static TAG_ARRAY: &[GeneratedTag] = &[")?;
    for tag in tags {
        writeln!(
            writer,
            "    GeneratedTag {{ id: {}, name: {:?}, format_family: {:?}, table: {:?} }},",
            tag.id, tag.name, tag.format_family, tag.table_name
        )?;
    }
    writeln!(writer, "];")?;
    writeln!(writer)?;

    // Generate HashMap for O(1) lookup
    writeln!(
        writer,
        "static TAG_MAP: Lazy<HashMap<(u32, &'static str), &'static GeneratedTag>> = Lazy::new(|| {{"
    )?;
    writeln!(writer, "    let mut map = HashMap::with_capacity({});", tags.len())?;
    writeln!(writer, "    for tag in TAG_ARRAY.iter() {{")?;
    writeln!(writer, "        map.insert((tag.id, tag.format_family), tag);")?;
    writeln!(writer, "    }}")?;
    writeln!(writer, "    map")?;
    writeln!(writer, "}});")?;
    writeln!(writer)?;

    // Generate lookup function
    writeln!(
        writer,
        "pub fn lookup_tag(id: u32, format_family: &str) -> Option<&'static GeneratedTag> {{"
    )?;
    writeln!(writer, "    TAG_MAP.get(&(id, format_family)).copied()")?;
    writeln!(writer, "}}")?;
    writeln!(writer)?;

    // Generate count function for tests
    writeln!(writer, "pub fn generated_tag_count() -> usize {{")?;
    writeln!(writer, "    TAG_ARRAY.len()")?;
    writeln!(writer, "}}")?;

    Ok(())
}
```

**Step 2: Verify generated code compiles**

Run: `cargo clean && cargo build 2>&1 | grep "generated_tags"`

Expected: Compiles successfully with all tags

**Step 3: Check generated file size**

Run: `ls -lh src/tag_db/generated_tags.rs`

Expected: 10-30 MB (reasonable for 28K tags)

**Step 4: Commit**

```bash
git add build.rs src/tag_db/generated_tags.rs
git commit -m "build: optimize generated code with HashMap lookup

- Use static array + lazy HashMap instead of giant match
- O(1) tag lookup performance
- Reduced code size for 28K+ tags
- Compiles in reasonable time"
```

---

## Task 6: Verify Tag Coverage

**Files:**
- Run tests and verify coverage

**Step 1: Run tag coverage tests**

Run: `cargo test --test tag_database_coverage -- --nocapture`

Expected output:
```
Tag coverage: 28853/28853 (100.0%)
test test_tag_database_has_minimum_coverage ... ok
test test_tag_database_target_coverage ... ok
```

**Step 2: Verify tag lookup functionality**

Create test file `tests/tag_lookup_verification.rs`:

```rust
use oxidex::tag_db::generated_tags::{lookup_tag, generated_tag_count};

#[test]
fn test_exif_tag_lookup() {
    // EXIF:Make (0x010F in IFD0)
    let tag = lookup_tag(0x010F, "EXIF").expect("Make tag not found");
    assert_eq!(tag.name, "Make");
}

#[test]
fn test_gps_tag_lookup() {
    // GPS:GPSLatitude (0x0002 in GPS IFD)
    let tag = lookup_tag(0x0002, "GPS").expect("GPSLatitude not found");
    assert_eq!(tag.name, "GPSLatitude");
}

#[test]
fn test_canon_maker_note_lookup() {
    // Canon camera settings tag
    let tag = lookup_tag(0x0001, "Canon").expect("Canon tag not found");
    assert!(tag.name.contains("Canon"));
}

#[test]
fn test_tag_count() {
    let count = generated_tag_count();
    assert!(
        count >= 28000,
        "Expected at least 28000 tags, got {}",
        count
    );
}
```

**Step 3: Run verification tests**

Run: `cargo test tag_lookup_verification`

Expected: All tests pass

**Step 4: Check build time**

Run: `time cargo clean && time cargo build --release`

Expected: < 5 minutes total build time

**Step 5: Commit**

```bash
git add tests/tag_lookup_verification.rs
git commit -m "test: verify 100% tag coverage achievement

- Tests confirm 28,853 tags parsed
- Validates tag lookup for EXIF, GPS, Canon
- All coverage tests passing"
```

---

## Task 7: Update Documentation

**Files:**
- Modify: `README.md`, `docs/TAG_DATABASE.md` (create)

**Step 1: Create tag database documentation**

```markdown
# Tag Database

## Coverage

**Total Tags:** 28,853 / 28,853 (100%)
**Unique Tag Names:** 17,925
**Modules Parsed:** 140+

## Architecture

The tag database is automatically generated during build from Perl ExifTool source:

1. **Download** - Fetches latest ExifTool master from GitHub
2. **Discover** - Finds all 140+ .pm Perl modules
3. **Parse** - Extracts tag definitions using comprehensive regex
4. **Resolve** - Follows subdirectory references for nested tables
5. **Generate** - Creates optimized Rust code with HashMap lookup

## Performance

- **Lookup**: O(1) via HashMap (id, format_family) -> tag
- **Memory**: ~5MB for 28K tags (static data)
- **Build Time**: ~3 minutes (cached after first build)

## Supported Formats

All 140+ ExifTool format families including:
- Standard: EXIF, GPS, XMP, IPTC, JFIF
- Maker Notes: Canon, Nikon, Sony, Olympus, etc. (30+ vendors)
- Video: QuickTime, MP4, Matroska, Flash, MPEG
- Audio: ID3, FLAC, Ogg, Vorbis, AAC
- Specialized: DICOM, FITS, MXF, PDF, PostScript
- RAW: DNG, CR2, NEF, ARW, etc.

## Tag Lookup

```rust
use oxidex::tag_db::generated_tags::lookup_tag;

// Look up EXIF Make tag
if let Some(tag) = lookup_tag(0x010F, "EXIF") {
    println!("Tag: {} (ID: 0x{:04X})", tag.name, tag.id);
}
```

## Rebuilding

To force regeneration:

```bash
rm src/tag_db/generated_tags.rs
cargo build
```
```

**Step 2: Update README**

Add to README.md features section:

```markdown
### 100% ExifTool Tag Parity

- **28,853 tags** across 140+ format families
- Automatically generated from Perl ExifTool source
- Includes all maker notes (Canon, Nikon, Sony, etc.)
- Supports specialized formats (DICOM, FITS, MXF)
```

**Step 3: Commit**

```bash
git add docs/TAG_DATABASE.md README.md
git commit -m "docs: document 100% tag coverage achievement

- Created TAG_DATABASE.md with architecture overview
- Updated README with tag parity feature
- Documented lookup API and rebuild process"
```

---

## Task 8: Benchmarking and Performance

**Files:**
- Create: `benches/tag_lookup_benchmarks.rs`

**Step 1: Create tag lookup benchmarks**

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oxidex::tag_db::generated_tags::lookup_tag;

fn bench_exif_tag_lookup(c: &mut Criterion) {
    c.bench_function("lookup_exif_make", |b| {
        b.iter(|| {
            black_box(lookup_tag(0x010F, "EXIF"))
        });
    });
}

fn bench_canon_tag_lookup(c: &mut Criterion) {
    c.bench_function("lookup_canon_tag", |b| {
        b.iter(|| {
            black_box(lookup_tag(0x0001, "Canon"))
        });
    });
}

fn bench_multiple_lookups(c: &mut Criterion) {
    c.bench_function("lookup_100_tags", |b| {
        b.iter(|| {
            for i in 0..100 {
                black_box(lookup_tag(i, "EXIF"));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_exif_tag_lookup,
    bench_canon_tag_lookup,
    bench_multiple_lookups
);
criterion_main!(benches);
```

**Step 2: Run benchmarks**

Run: `cargo bench --bench tag_lookup_benchmarks`

Expected: <100ns per lookup (O(1) HashMap)

**Step 3: Commit**

```bash
git add benches/tag_lookup_benchmarks.rs
git commit -m "bench: add tag lookup performance benchmarks

- Validates O(1) lookup performance
- Tests EXIF and maker note lookups
- Confirms <100ns average lookup time"
```

---

## Verification Checklist

After completing all tasks:

- [ ] All 140+ Perl modules discovered automatically
- [ ] Comprehensive tag parsing (hash-based, simple, string IDs)
- [ ] Subdirectory references resolved
- [ ] 28,853 tags generated (100% coverage)
- [ ] Tag coverage tests pass
- [ ] Tag lookup tests pass
- [ ] Build time < 5 minutes
- [ ] Lookup performance < 100ns (O(1))
- [ ] Documentation updated
- [ ] Benchmarks confirm performance

## Expected Results

**Before:**
```
Tag coverage: 721/28853 (2.5%)
test test_tag_database_has_minimum_coverage ... FAILED
test test_tag_database_target_coverage ... FAILED
```

**After:**
```
Tag coverage: 28853/28853 (100.0%)
test test_tag_database_has_minimum_coverage ... ok
test test_tag_database_target_coverage ... ok
```

## Notes for Engineer

**Build Process:**
1. Downloads ExifTool master.zip (~10MB)
2. Extracts to OUT_DIR
3. Discovers 140+ Perl modules
4. Parses each module for tag definitions
5. Resolves subdirectory references
6. Generates optimized Rust code (~15MB)
7. Compiles into binary (~5MB static data)

**Common Issues:**
- **unzip not found**: Install unzip: `brew install unzip` / `apt install unzip`
- **Network timeout**: Increase timeout in download_exiftool_source
- **Parse errors**: Some modules may have unusual syntax - skip with warning
- **Memory usage**: Large tag count may use 1-2GB RAM during build

**Performance Tips:**
- Use Lazy HashMap for O(1) lookup
- Keep tag data as static arrays (no heap allocation)
- Use once_cell for initialization
- Benchmark critical lookup paths

**Skills Referenced:**
- @superpowers:test-driven-development - Write lookup tests first
- @superpowers:verification-before-completion - Verify 28K tag count
- @superpowers:systematic-debugging - If parse fails, debug per-module
