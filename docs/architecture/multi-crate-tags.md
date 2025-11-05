# Multi-Crate Tag Database Architecture

## Overview

The ExifTool-RS tag database is split into 6 domain-specific crates for faster compilation:

- `exiftool-tags-core` - Universal standards (EXIF, XMP, IPTC, GPS)
- `exiftool-tags-camera` - Camera manufacturers (Canon, Nikon, Sony, etc.)
- `exiftool-tags-media` - Audio/video formats (QuickTime, FLAC, MPEG)
- `exiftool-tags-image` - Image formats (PNG, GIF, JPEG2000)
- `exiftool-tags-document` - Document formats (PDF, fonts, archives)
- `exiftool-tags-specialty` - Medical/scientific (DICOM, FITS, MRC)
- `exiftool-tags` - Facade crate re-exporting everything

## Data Format

Tags are stored as YAML files embedded at compile time and parsed lazily:

```yaml
tables:
  - name: Canon
    tags:
      - id: "0x0001"
        name: CanonCameraSettings
        writable: true
        type: int16u
        description: "Camera settings"
```

## Build Process

1. `build.rs` downloads ExifTool Perl source
2. Parses tag definitions from Perl modules
3. Routes tags to appropriate domain based on table name
4. Generates compact YAML files (not Rust code)
5. Each crate embeds YAML with `include_str!()`
6. Runtime lazy parsing on first access

## Performance

- **Build time:** 3-5x faster vs monolithic crate
- **Parallelization:** 6 crates compile simultaneously
- **Incremental:** Changes to main code don't trigger tag recompilation
- **Runtime overhead:** <10ms one-time YAML parsing

## API Compatibility

The facade crate maintains full API compatibility:

```rust
use exiftool_tags::*;

// Global search across all domains
let table = get_tag_table("Canon");

// Domain-specific access
let canon = camera::get_tag_table("Canon");
```
