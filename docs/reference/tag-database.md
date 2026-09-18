# Tag Database

OxiDex maintains a comprehensive tag database automatically synchronized with ExifTool's Perl source.

## Coverage

- **Total Tags:** 16,684 tag definitions (see [Tag Coverage](/reference/tag-coverage-analysis) — this is a definitions count, not a measurement of extraction coverage)
- **Format Families:** 140+
- **Sync:** `cargo run --release --bin sync_tags` against a locally installed ExifTool, pinned by `.exiftool-version` (see [Synchronization](#synchronization))
- **Per-domain listings:** the generated [tag domain pages](/tag-domains/) (`just docs-generate-tags`)

## Architecture

The definitions live in six workspace crates, one per domain
(`oxidex-tags-core`, `-camera`, `-image`, `-media`, `-document`,
`-specialty`), fronted by `oxidex-tags` and sharing types through
`oxidex-tags-shared`. Each crate carries its definitions as a YAML file
(`oxidex-tags-<domain>/src/<domain>_tags.yaml`) that `build.rs` pre-compiles
to a binary blob at build time, so nothing is parsed at program start. The
[Tag Database Architecture](/architecture/tag-database) page describes the
crate layout, the build profiles and the generation pipeline in detail.

## Supported Formats

All 140+ ExifTool format families including:

### Standard Formats
- **EXIF** - Exchangeable Image File Format (718 tags)
- **GPS** - GPS location data
- **XMP** - Extensible Metadata Platform
- **IPTC** - International Press Telecommunications Council
- **JFIF** - JPEG File Interchange Format
- **TIFF** - Tagged Image File Format

### Camera Maker Notes
- **Canon** - 930 tags
- **Nikon** - 2,398 tags (main) + 3,512 tags (NikonCustom)
- **Sony** - 1,148 tags
- **Olympus** - Complete maker notes
- **Panasonic** - Complete maker notes
- **Pentax** - 876 tags
- **FujiFilm** - Complete maker notes
- **Samsung, Minolta, Kodak, Casio, Ricoh** - 30+ vendors

### Video Formats
- **QuickTime** - 1,069 tags
- **MP4** - MPEG-4 video
- **Matroska** - MKV/WebM container
- **Flash** - FLV format
- **ASF** - Advanced Systems Format
- **MPEG** - MPEG video streams
- **H264** - H.264 codec metadata

### Audio Formats
- **ID3** - MP3 metadata
- **FLAC** - Free Lossless Audio Codec
- **Ogg** - Ogg container
- **Vorbis** - Vorbis audio codec
- **AAC** - Advanced Audio Coding
- **APE** - Monkey's Audio

### Specialized Formats
- **DICOM** - 3,149 tags (medical imaging)
- **FITS** - Flexible Image Transport System
- **MXF** - Material Exchange Format
- **PDF** - Portable Document Format
- **PostScript** - PostScript metadata
- **ISO** - ISO 9660 disc images

### RAW Camera Formats
- **DNG** - Digital Negative
- **CR2/CR3** - Canon RAW
- **NEF** - Nikon Electronic Format
- **ARW** - Sony Alpha RAW
- **CanonRaw** - Canon CRW
- **SigmaRaw** - Sigma X3F
- **MinoltaRaw** - Minolta MRW

### Graphics Formats
- **PNG** - Portable Network Graphics
- **GIF** - Graphics Interchange Format
- **BMP** - Bitmap
- **PSD** - Adobe Photoshop
- **JPEG** - Joint Photographic Experts Group
- **Jpeg2000** - JPEG 2000
- **OpenEXR** - High Dynamic Range
- **ICO** - Windows Icon

### Document Formats
- **HTML** - Hypertext Markup Language
- **XML** - Extensible Markup Language
- **SVG** - Scalable Vector Graphics
- **VCard** - Electronic business card
- **LNK** - Windows shortcut

### Other
- **Photoshop** - 136 tags (Adobe Photoshop metadata)
- **ICC_Profile** - 90 tags (color management)
- **Apple** - Apple-specific metadata
- **Microsoft** - Microsoft-specific metadata
- **Google** - Google-specific metadata
- **GoPro** - GoPro camera metadata
- **DJI** - DJI drone metadata
- **FLIR** - FLIR thermal camera
- **Parrot** - Parrot drone

## Tag counts by domain

Per-table and per-domain counts are rendered into the [tag domain pages](/tag-domains/)
by `just docs-generate-tags`; they are generated output and are not maintained here.

## Tag Lookup

### In Rust Code

```rust
use oxidex::tag_db::generated_tags::get_generated_tag_descriptor;

// Look up EXIF Make tag
if let Some(tag) = get_generated_tag_descriptor("EXIF:Make") {
    println!("Tag: {} (ID: {:?})", tag.tag_name, tag.tag_id);
}
```

### Tag Naming Convention

All tags follow the format: `<FormatFamily>:<TagName>`

**Examples:**
- `EXIF:Make` - Camera manufacturer
- `EXIF:Model` - Camera model
- `GPS:Latitude` - GPS latitude coordinate
- `XMP-dc:Creator` - Document creator (XMP Dublin Core)
- `IPTC:Keywords` - Image keywords
- `Canon:SerialNumber` - Canon camera serial number

**Note:** Tag names are case-sensitive.

## Rebuilding the Database

Regeneration is explicit, never a side effect of `cargo build`:

```bash
cargo run --release --bin sync_tags
```

This runs `exiftool -f -listx` against the locally installed ExifTool, routes
each tag's table to one of the six domain crates, rewrites the crates' YAML
files and updates `.exiftool-version`. Review the resulting `git diff` before
committing; the release recorded in `.exiftool-version` is the one every
comparison in this repository is graded against.

## Implementation Details

### Code Generation Strategy

To handle tens of thousands of tag definitions without overwhelming the Rust compiler:

**File Organization:**
- 124 format family modules in `src/tag_db/generated/tags_*.rs`
- 1 main module `src/tag_db/generated_tags.rs` with lookup logic
- Total: ~35,000 lines across 125 files (vs 425,000 in a single file)

**Each Family Module:**
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

**Main Module Lookup:**
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
- Each module compiles independently, reducing peak memory usage
- Main file is tiny (792 lines), mostly module declarations
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

### Build Memory Requirements

With the split-file architecture (124 modules instead of 1 massive file):

- **Release builds** (`cargo build --release`): ~5GB RAM, 8-10 minutes
- **Debug builds** (`cargo build`): Not recommended - will OOM (>32GB)
- **Testing:** Use `cargo test --release` to avoid OOM
- **Recommended:** Always use `--release` flag for builds and tests

The split-file approach reduced the main generated file from 425,000 lines to 792 lines, with the remaining code distributed across 124 family-specific modules averaging 283 lines each.

## Tag Descriptor Structure

Each tag in the database has the following information:

```rust
pub struct TagDescriptor {
    pub tag_name: String,           // e.g., "EXIF:Make"
    pub tag_id: Option<TagId>,      // Numeric or string identifier
    pub writable: bool,             // Whether tag can be written
    pub value_type: ValueType,      // Data type (string, int, rational, etc.)
    pub description: Option<String>, // Human-readable description
}
```

## Known Limitations

- **Composite tags excluded** - Calculated values, not stored in files
- **Shortcut tags excluded** - Aliases to other tags
- **Some maker notes incomplete** - Reverse engineering ongoing
- **Debug builds not supported** - Use `--release` flag always

## Synchronization

1. **Version pin** - `.exiftool-version` names the ExifTool release the definitions were synced from
2. **Explicit sync** - `cargo run --release --bin sync_tags` (above)
3. **CI validation** - `scripts/sync_tag_stats.py --check` keeps the published definitions count consistent across the documentation on every PR

A definitions count is not a coverage measurement; see
[Measuring Coverage](/contributing/measuring-coverage).

## Additional Resources

- [API Reference](/reference/api-reference) - Using tags in code
- [Formats Overview](/reference/formats/) - Supported file formats
- [Architecture](/reference/architecture) - System design
- [ExifTool Tag Names](https://exiftool.org/TagNames/) - Original tag documentation
