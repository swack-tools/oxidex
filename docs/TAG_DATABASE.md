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
5. **Generate** - Creates optimized Rust code with Lazy Vec + HashMap lookup

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

### Code Generation Strategy

Due to the large number of tags (32,677), the code uses:
- `Lazy<Vec<TagDescriptor>>` for the tag array (allows heap allocation)
- `Lazy<HashMap<String, TagDescriptor>>` for O(1) lookup
- Compact single-line tag entries to minimize generated code size
- Lazy initialization to avoid static allocation constraints

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

- **With optimization (default)**: 2-5GB RAM
- **Without optimization**: 20-100GB RAM (will OOM on most systems)
- **Recommended**: Use the provided Cargo.toml configuration which automatically optimizes the generated module
