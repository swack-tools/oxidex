# Tag Database Architecture

The six internal tag databases provide lookup for definitions generated from
the pinned ExifTool Perl tables. The YAML registry describes source tag
identity; read coverage is measured separately.

## Overview

| Metric | Value |
|--------|-------|
| Total Tags | 32,271 (tag definitions; see [Tag Coverage](/reference/tag-coverage-analysis) for measured extraction) |
| Modules Parsed | 140+ |
| Lookup Time | O(1) |
| Memory | ~5-10MB (lazy loaded) |

## Workspace Architecture

The tag database is implemented as a **separate workspace crate** (`oxidex-tags-*`) to solve debug build memory issues.

### Structure

```
oxidex/
├── oxidex-tags-core/     # Core types (TagDescriptor, TagId, etc.)
├── oxidex-tags-camera/   # Camera MakerNotes tags
├── oxidex-tags-media/    # Audio/video format tags
├── oxidex-tags-image/    # Image format tags (EXIF, PNG, etc.)
├── oxidex-tags-document/ # Document format tags (PDF, etc.)
├── oxidex-tags-specialty/# Specialized format tags (DICOM, etc.)
└── src/                  # Main crate (uses oxidex-tags-*)
```

### Profile Configuration

```toml
# In root Cargo.toml
[profile.dev.package.oxidex-tags-core]
opt-level = 2        # Always optimize tag crates
codegen-units = 16   # Parallel compilation

[profile.dev.package.oxidex-tags-camera]
opt-level = 2
codegen-units = 16

# ... similar for other tag crates
```

### Why Separate Crates?

- **Debug builds**: 100GB+ RAM → **11GB** (91% reduction)
- **Main crate stays in debug mode** (fast iteration)
- **Tag crates always optimized** (prevents OOM)
- **Industry-standard pattern** (used by rustc, diesel, syn)

## Tag Generation Pipeline

Tier 1 captures one fresh dump from the library selected by
`.exiftool-version`. Its `gen_tag_registry` step regenerates all six YAML
files, which the artifact manifest and CI verify:

```bash
tools/exiftool-tables/regen-all.sh
```

```
1. Capture → dump_tables.pl reads the selected Perl symbol tables and source hash
2. Check   → gen_tag_registry verifies dump version and source identity
3. Route   → source table identities map to six oxidex-tags-* domains
4. Generate → all six YAML files are declared Tier 1 artifacts
5. Verify  → -listx independently checks documented names and IDs
```

The generator resolves table-level `WRITABLE` inheritance and withholds
numeric array types that the CLI scalar parser cannot honor. Source-only
SubDirectory pointers remain in the registry even when `-listx` omits them.

### Generated Code Structure

Each domain crate pre-compiles its YAML tag definitions to binary format at build time. The `build.rs` script eliminates the cold-start YAML parsing penalty by:

1. Reading the YAML source file (e.g. `oxidex-tags-camera/src/camera_tags.yaml`)
2. Deserializing with `serde_yaml::from_str` into `TagDatabase` structures
3. Serializing to efficient binary format with `bincode::serde`
4. Writing the binary blob to `OUT_DIR` for embedding via `include_bytes!`

This converts one-time deserialization work from runtime to compile time, avoiding repeated YAML parsing on every program start while keeping the source readable and maintainable.

## What the definitions cover

The definitions are grouped into six domains, one per crate. The generated
[tag-domain pages](/tag-domains/) list every table and tag in each domain.
They come from the same YAML the crates embed, and `just docs-generate-tags`
refreshes them.

A definition records that ExifTool declares a tag. It is not evidence that
OxiDex extracts it. For what OxiDex actually reads, see
[ExifTool parity](/guide/exiftool-parity).

## Usage

### Lookup by Tag Name

```rust
use oxidex::tag_db::get_tag_descriptor;

if let Some(tag) = get_tag_descriptor("EXIF:Make") {
    println!("Tag: {} (ID: {:?})", tag.tag_name, tag.tag_id);
}
```

### Get All Tags for Format

```rust
use oxidex_tags_camera::canon::get_tags;

for (name, descriptor) in get_tags().iter() {
    println!("{}: {:?}", name, descriptor.value_type);
}
```

## Rebuilding

To regenerate from the selected pinned source:

```bash
tools/exiftool-tables/regen-all.sh
```

This updates the declared generated artifacts. It does not change
`.exiftool-version`; changing the pin is a separate source upgrade.

## Performance

- **Lookup**: O(1) via HashMap
- **Memory**: ~5-10MB (heap-allocated lazily)
- **Build Time**: ~4 minutes (cached after first build)
- **Compilation**: Uses Lazy initialization to avoid static limits

## Build Requirements

### Memory

| Build Mode | Memory |
|------------|--------|
| Release | ~5GB |
| Debug (with workspace) | ~11GB |
| Debug (without workspace) | 100GB+ (OOM) |

**Recommendation**: Always use release builds for final testing.

### Commands

```bash
# Development build
cargo build

# Release build (recommended for testing)
cargo build --release

# Run tests (release recommended)
cargo test --release --workspace
```

## XML Parser Features

The XML tag parser (`parse_listx` in `src/tag_sync/mod.rs`) handles:

- XML element parsing: `<table>` (table group) and `<tag>` (individual tag definitions)
- Tag attributes: `id`, `name`, `writable` (boolean), `type` (optional)
- Both element forms: self-closing tags (`<tag/>`) and full tags (`<tag>...</tag>`)
- Nested descriptions: `<desc lang='en'>` text extraction (English locale only)
- XML entity unescaping: `&amp;`, `&#39;`, etc. in description text
- Writable inheritance resolution: ExifTool pre-resolves table-level inheritance in `-listx` output

## Known Limitations

- Some ExifTool composite tags are excluded (calculated values)
- Shortcut tags are excluded (aliases to other tags)
- Some tags have platform-specific or format-specific variations
