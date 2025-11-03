# Tag Database

## Coverage

**Total Tags:** 32,677 / 28,853 (113%)
**Unique Tag Names:** 32,677
**Modules Parsed:** 140+

## Architecture

The tag database is automatically generated during build from Perl ExifTool source:

1. **Download** - Fetches latest ExifTool master from GitHub
2. **Discover** - Finds all 140+ .pm Perl modules recursively
3. **Parse** - Extracts tag definitions using comprehensive regex patterns
4. **Resolve** - Follows subdirectory references for nested tables
5. **Generate** - Creates 124 separate module files (one per format family) + main lookup module
   - Each family module: `src/tag_db/generated/tags_<family>.rs` (100-3,500 tags each)
   - Main module: `src/tag_db/generated_tags.rs` (792 lines)
   - Total: ~35,000 lines across 125 files (vs 425,000 lines in single file)

## Performance

- **Lookup**: O(1) via HashMap (tag_name) -> TagDescriptor
- **Memory**: ~5-10MB for 32K tags (heap-allocated lazily)
- **Build Time**: ~4 minutes (cached after first build)
- **Compilation**: Uses Lazy initialization to avoid static allocation limits

## Supported Formats

All 140+ ExifTool format families including:
- **Standard**: EXIF, GPS, XMP, IPTC, JFIF, TIFF
- **Maker Notes**: Canon (930 tags), Nikon (2,398 tags), Sony (1,148 tags), Olympus, Panasonic, Pentax, FujiFilm, Samsung, Minolta, Kodak, Casio, Ricoh, etc. (30+ vendors)
- **Video**: QuickTime (1,069 tags), MP4, Matroska, Flash, ASF, MPEG, H264
- **Audio**: ID3, FLAC, Ogg, Vorbis, AAC, APE
- **Specialized**: DICOM (3,149 tags), FITS, MXF, PDF, PostScript, ISO
- **RAW**: DNG, CR2, NEF, ARW, CanonRaw, SigmaRaw, MinoltaRaw
- **Graphics**: PNG, GIF, BMP, PSD, JPEG, Jpeg2000, OpenEXR, ICO
- **Documents**: HTML, XML, SVG, VCard, LNK
- **Other**: Photoshop (136 tags), ICC_Profile (90 tags), Apple, Microsoft, Google, GoPro, DJI, FLIR, Parrot

## Notable Tag Counts by Module

- DICOM: 3,149 tags (medical imaging)
- NikonCustom: 3,512 tags (custom Nikon settings)
- Nikon: 2,398 tags (main Nikon maker notes)
- Sony: 1,148 tags
- QuickTime: 1,069 tags
- Canon: 930 tags
- Casio: 930 tags
- Pentax: 876 tags
- Exif: 718 tags (core EXIF specification)

## Tag Lookup

```rust
use exiftool_rs::tag_db::generated_tags::get_generated_tag_descriptor;

// Look up EXIF Make tag
if let Some(tag) = get_generated_tag_descriptor("EXIF:Make") {
    println!("Tag: {} (ID: {:?})", tag.tag_name, tag.tag_id);
}
```

## Rebuilding

To force regeneration:

```bash
rm src/tag_db/generated_tags.rs
cargo build
```

The build script will:
1. Download ExifTool source (~10MB)
2. Extract and discover all Perl modules
3. Parse 32,677 tag definitions
4. Generate optimized Rust code (~6MB source)
5. Compile into binary (~5MB in memory)

## Implementation Details

### Code Generation Strategy - Split-File Architecture

To handle 32,677 tags without overwhelming the Rust compiler:

**File Organization:**
- 124 format family modules in `src/tag_db/generated/tags_*.rs`
- 1 main module `src/tag_db/generated_tags.rs` with lookup logic
- Total: ~35,000 lines across 125 files (vs 425,000 in a single file)

**Each Family Module Contains:**
```rust
static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![...]);

pub fn get_tags() -> &'static HashMap<String, TagDescriptor> {
    static MAP: Lazy<HashMap<String, TagDescriptor>> = Lazy::new(|| {
        let mut map = HashMap::with_capacity(TAGS.len());
        for tag in TAGS.iter() {
            map.insert(tag.tag_name.clone(), tag.clone());
        }
        map
    });
    &MAP
}
```

**Main Module Lookup (Sequential Search):**
```rust
pub fn get_generated_tag_descriptor(name: &str) -> Option<&'static TagDescriptor> {
    // Query each family registry in sequence
    if let Some(desc) = tags_exif::get_tags().get(name) { return Some(desc); }
    if let Some(desc) = tags_canon::get_tags().get(name) { return Some(desc); }
    // ... 124 total families
    None
}
```

**Benefits:**
- Each module compiles independently, reducing peak memory usage per module
- Main file is tiny (792 lines), mostly just module declarations
- Lazy initialization happens at runtime, not compile-time
- Compiler can optimize each family module separately

### Parser Features

The Perl tag definition parser handles:
- Hash-based tag definitions: `0x0100 => { Name => 'ImageWidth', ... }`
- Simple tag definitions: `0x0100 => 'ImageWidth'`
- String-based tag IDs (hashed to numeric values)
- Nested subdirectory references
- Writable type specifications
- Value type inference
- Multi-line definitions

### Known Limitations

- **Memory usage during compilation**: The 32K tag file can cause rustc to use significant memory (20-100GB) without optimizations. The Cargo.toml includes a profile override (`[profile.dev.package.exiftool-rs]`) to compile this module with opt-level=2 even in debug builds, which reduces memory usage to ~2-5GB.
- Some ExifTool composite tags are excluded (calculated values, not stored in files)
- Shortcut tags are excluded (aliases to other tags)

### Build Memory Requirements

With the split-file architecture (124 modules instead of 1 massive file):

- **Release builds** (`cargo build --release`): ~5GB RAM, 8-10 minutes
- **Debug builds** (`cargo build`): Not recommended - will OOM (>32GB) despite file splitting
- **Testing**: Use `cargo test --release` to avoid OOM
- **Recommended**: Always use `--release` flag for builds and tests

The split-file approach reduced the main generated file from 425,000 lines to 792 lines, with the remaining code distributed across 124 family-specific modules averaging 283 lines each. However, even with splitting, debug mode compilation of the combined codebase exceeds reasonable memory limits on most development machines.
