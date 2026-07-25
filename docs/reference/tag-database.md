# Tag Database

OxiDex maintains a comprehensive tag database automatically synchronized with ExifTool's Perl source.

## Coverage

- **Total Tags:** 32,677 / 28,853 (113% of ExifTool)
- **Unique Tag Names:** 32,677
- **Format Families:** 140+
- **Sync:** Manual regeneration from ExifTool master (see [Synchronization](#synchronization))

## Architecture

### Workspace-Based Separation

The tag database is implemented as a **separate workspace crate** (`exiftool-tags`) to prevent memory issues during compilation:

**Crate Structure:**
- `exiftool-tags/` - Tag database crate (always optimized)
- `oxidex/` - Main crate (debug mode for fast iteration)

**Profile Configuration:**
```toml
# In root Cargo.toml
[profile.dev.package.exiftool-tags]
opt-level = 2        # Always optimize tag database
codegen-units = 16   # Parallel compilation
```

**Benefits:**
- Debug builds: 100GB+ RAM → **11GB** (91% reduction)
- Main crate stays in debug mode (fast iteration)
- Tag database always optimized (prevents OOM)
- Industry-standard pattern (used by rustc, diesel, syn)

### Tag Generation Pipeline

The tag database is automatically generated during build from Perl ExifTool source:

1. **Download** - Fetches latest ExifTool master from GitHub
2. **Discover** - Finds all 140+ .pm Perl modules recursively
3. **Parse** - Extracts tag definitions using comprehensive regex patterns
4. **Resolve** - Follows subdirectory references for nested tables
5. **Generate** - Creates 124 separate module files (one per format family) + main lookup module
   - Each family module: `exiftool-tags/src/tag_db/generated/tags_<family>.rs` (100-3,500 tags each)
   - Main module: `exiftool-tags/src/tag_db/generated_tags.rs` (792 lines)
   - Total: ~35,000 lines across 125 files (vs 425,000 lines in single file)

## Performance

- **Lookup:** O(1) via HashMap (tag_name) → TagDescriptor
- **Memory:** ~5-10MB for 32K tags (heap-allocated lazily)
- **Build Time:** ~4 minutes (cached after first build)
- **Compilation:** Uses lazy initialization to avoid static allocation limits

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

## Notable Tag Counts by Module

| Module | Tag Count | Description |
|--------|-----------|-------------|
| NikonCustom | 3,512 | Custom Nikon camera settings |
| DICOM | 3,149 | Medical imaging standard |
| Nikon | 2,398 | Main Nikon maker notes |
| Sony | 1,148 | Sony camera maker notes |
| QuickTime | 1,069 | QuickTime/MP4 video metadata |
| Casio | 930 | Casio camera metadata |
| Canon | 930 | Canon camera maker notes |
| Pentax | 876 | Pentax camera maker notes |
| EXIF | 718 | Core EXIF specification |

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

To force regeneration:

```bash
rm exiftool-tags/src/tag_db/generated_tags.rs
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

To handle 32,677 tags without overwhelming the Rust compiler:

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

The tag database stays synchronized with ExifTool through:

1. **Version tracking** - `.exiftool-version` file tracks the synced commit hash
2. **Manual sync** - Force regeneration with `rm src/tag_db/generated_tags.rs && cargo build --release`
3. **CI validation** - Automated tests verify tag count and coverage on every PR

## Additional Resources

- [API Reference](/reference/api-reference) - Using tags in code
- [Formats Overview](/reference/formats/) - Supported file formats
- [Architecture](/reference/architecture) - System design
- [ExifTool Tag Names](https://exiftool.org/TagNames/) - Original tag documentation
