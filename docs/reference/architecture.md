# Architecture

OxiDex is built on a clean, modular architecture that separates concerns, enables testability, and allows multiple access patterns (CLI, library API, FFI).

## Design Philosophy

OxiDex follows **Hexagonal Architecture** (Ports and Adapters pattern) with three main layers:

1. **Application Layer** - External interfaces (CLI, FFI bindings)
2. **Domain Layer** - Core business logic and format-agnostic metadata models
3. **Infrastructure Layer** - Format-specific parsers, I/O operations, platform-specific code

This architecture ensures:
- **Clean separation of concerns** - Business logic independent of I/O
- **Testability** - Core logic testable without filesystem dependencies
- **Extensibility** - Easy to add new file formats or access patterns
- **Multiple interfaces** - Same core logic powers CLI, library API, and FFI

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    APPLICATION LAYER                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │  CLI Binary  │  │  C FFI API   │  │  Library API │     │
│  │  (oxidex)    │  │  (oxidex.h)  │  │  (crates.io) │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
│         │                  │                  │            │
└─────────┼──────────────────┼──────────────────┼────────────┘
          │                  │                  │
          ▼                  ▼                  ▼
┌─────────────────────────────────────────────────────────────┐
│                      DOMAIN LAYER                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Metadata Models (Tag, TagGroup, MetadataMap)       │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Operations (Extract, Write, Format Detection)      │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Tag Database (32,677 tags from ExifTool source)    │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
          │                  │                  │
          ▼                  ▼                  ▼
┌─────────────────────────────────────────────────────────────┐
│                   INFRASTRUCTURE LAYER                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Format Parsers (JPEG, TIFF, PNG, MP4, etc.)        │  │
│  │  - Binary parsers (nom combinators)                 │  │
│  │  - Segment extraction                               │  │
│  │  - Tag mapping to domain models                     │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  I/O Abstraction (FileReader, MemoryMap)            │  │
│  │  - Memory-mapped I/O (memmap2)                      │  │
│  │  - Buffered reading                                 │  │
│  │  - Atomic writes                                    │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Platform Layer (File system, OS attributes)        │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Core Components

### 1. Application Layer

#### CLI Binary (`src/bin/oxidex.rs`)

Command-line interface providing ExifTool-compatible syntax:

```bash
# Extract metadata
oxidex photo.jpg

# Write metadata
oxidex -Artist="Jane Doe" photo.jpg

# Batch processing
oxidex -r /path/to/photos/
```

**Features:**
- Argument parsing with clap
- Output formatting (text, JSON, CSV)
- Progress reporting
- Error handling and user-friendly messages

#### Library API (`src/lib.rs`)

Rust library for embedding metadata operations:

```rust
use oxidex::{MetadataReader, MetadataWriter};

// Read metadata
let reader = MetadataReader::from_file("photo.jpg")?;
let metadata = reader.extract_all()?;

// Write metadata
let mut writer = MetadataWriter::from_file("photo.jpg")?;
writer.set_tag("Artist", "Jane Doe")?;
writer.write()?;
```

**Features:**
- Type-safe API
- Zero-cost abstractions
- Iterator-based access
- Error handling with Result types

#### C FFI API (`src/ffi/mod.rs`)

C-compatible interface for cross-language integration:

```c
#include "oxidex.h"

// Read metadata
OxidexReader* reader = oxidex_reader_new("photo.jpg");
OxidexMetadata* metadata = oxidex_reader_extract(reader);

// Access tags
const char* artist = oxidex_metadata_get(metadata, "Artist");

// Cleanup
oxidex_metadata_free(metadata);
oxidex_reader_free(reader);
```

**Features:**
- C ABI compatibility
- Manual memory management
- Error codes and null checks
- Cross-language support (Python, Ruby, JavaScript, etc.)

### 2. Domain Layer

#### Metadata Models

**Tag** - Single metadata element:
```rust
pub struct Tag {
    pub name: String,
    pub value: TagValue,
    pub group: TagGroup,
}

pub enum TagValue {
    String(String),
    Integer(i64),
    Float(f64),
    DateTime(DateTime),
    Binary(Vec<u8>),
    Array(Vec<TagValue>),
}
```

**MetadataMap** - Collection of tags organized by group:
```rust
pub struct MetadataMap {
    tags: HashMap<String, Tag>,
    groups: HashMap<TagGroup, Vec<Tag>>,
}
```

**TagGroup** - Logical groupings (EXIF, XMP, IPTC, etc.):
```rust
pub enum TagGroup {
    EXIF,
    XMP,
    IPTC,
    GPS,
    MakerNotes,
    FileSystem,
    // ... 140+ format families
}
```

#### Operations

**Extract** - Read metadata from files:
- Format detection via magic bytes
- Parser dispatch based on format
- Tag aggregation across multiple segments/IFDs
- Value normalization and type conversion

**Write** - Modify metadata in files:
- Atomic file operations (write to temp, then rename)
- In-place updates where possible
- Preserve unmodified metadata
- Maintain file integrity

**Format Detection** - Identify file types:
- Magic byte matching (first 16 bytes)
- Extension fallback
- MIME type detection

#### Tag Database

32,677 metadata tags automatically synchronized with ExifTool source:

```rust
pub struct TagDatabase {
    tags: HashMap<u16, TagInfo>,  // Tag ID -> Info
    names: HashMap<String, u16>,  // Tag name -> ID
}

pub struct TagInfo {
    pub id: u16,
    pub name: &'static str,
    pub group: TagGroup,
    pub writable: bool,
    pub format: TagFormat,
}
```

**Generation:**
- Automated extraction from ExifTool Perl source
- Build-time code generation
- Static data for zero runtime overhead

### 3. Infrastructure Layer

#### Format Parsers

Each format has a dedicated parser module:

**JPEG Parser** (`src/parsers/jpeg/`):
- Segment extraction (APP0, APP1, APP13, etc.)
- EXIF IFD parsing (IFD0, IFD1, ExifIFD, GPS)
- XMP parsing (XML → tag map)
- IPTC parsing (Photoshop IRB → IIM records)
- JFIF metadata

**TIFF Parser** (`src/parsers/tiff/`):
- IFD (Image File Directory) traversal
- Byte order detection (little/big endian)
- Tag value extraction with type conversion
- SubIFD handling (recursive)

**PNG Parser** (`src/parsers/png/`):
- Chunk parsing (tEXt, iTXt, zTXt, etc.)
- CRC validation
- ICC profile extraction

**MP4/QuickTime Parser** (`src/parsers/mp4/`):
- Atom tree traversal
- ItemList metadata (©nam, ©ART, etc.)
- Timecode handling

**PDF Parser** (`src/parsers/pdf/`):
- Info dictionary extraction
- XMP metadata stream
- ICC profiles

**Binary Parsing:**
- Uses `nom` parser combinator library
- Type-safe parsing with compile-time guarantees
- Zero-copy where possible
- Error recovery for malformed files

#### I/O Abstraction

**FileReader** - Abstract file access:
```rust
pub trait FileReader {
    fn read(&self, offset: u64, size: usize) -> Result<Vec<u8>>;
    fn size(&self) -> u64;
}
```

**Implementations:**
- `MemoryMappedReader` - Uses `memmap2` for large files
- `BufferedReader` - Standard buffered I/O
- `InMemoryReader` - For testing with byte slices

**FileWriter** - Safe file modification:
```rust
pub struct FileWriter {
    path: PathBuf,
    temp_path: PathBuf,
}

impl FileWriter {
    pub fn write_atomic(&mut self, data: &[u8]) -> Result<()> {
        // Write to temp file
        // Sync to disk
        // Atomic rename
    }
}
```

#### Platform Layer

**File System Operations:**
- File attributes (size, permissions, timestamps)
- Directory traversal
- Recursive scanning
- Symbolic link handling

**OS-specific:**
- Unix permissions and ownership
- Windows file attributes
- macOS extended attributes

## Data Flow

### Read Operation

1. **CLI/API Entry** - User requests metadata extraction
2. **Format Detection** - Identify file type via magic bytes
3. **Parser Selection** - Dispatch to format-specific parser
4. **Binary Parsing** - Extract raw metadata from file
5. **Tag Mapping** - Convert binary data to domain Tag objects
6. **Aggregation** - Combine tags from multiple sources (EXIF + XMP + IPTC)
7. **Return** - Deliver MetadataMap to caller

### Write Operation

1. **CLI/API Entry** - User requests metadata modification
2. **Read Current Metadata** - Load existing tags
3. **Merge Changes** - Apply user modifications to metadata map
4. **Format Serialization** - Convert tags back to binary format
5. **Atomic Write** - Write to temp file, then atomic rename
6. **Verification** - Re-read to confirm changes

## Performance Optimizations

### Zero-Cost Abstractions

- **Static dispatch** - Trait objects avoided in hot paths
- **Inline functions** - Critical path functions marked `#[inline]`
- **Const generics** - Compile-time specialization where applicable

### Memory Efficiency

- **Memory-mapped I/O** - Large files accessed without full buffering
- **Zero-copy parsing** - Borrow from memory map where possible
- **String interning** - Common tag names stored as static strings

### Parallelization

- **Batch processing** - Uses Rayon for parallel file processing
- **Work stealing** - Efficient load balancing across cores
- **Lock-free** - Metadata operations are read-only or use atomic operations

### Caching

- **Tag database** - Static data embedded at compile time
- **Parser results** - Reuse parsed structures within file
- **Format detection** - Cache magic byte results

## Extensibility

### Adding a New Format

1. **Create parser module** - `src/parsers/new_format/mod.rs`
2. **Implement FileParser trait**:
   ```rust
   pub trait FileParser {
       fn detect(data: &[u8]) -> bool;
       fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap>;
   }
   ```
3. **Register in format registry** - Add to `src/formats/mod.rs`
4. **Add tests** - Unit tests + integration tests
5. **Update tag database** - If new tags are needed

### Adding New Tags

1. **Update tag database source** - `build/tag_database_generator.rs`
2. **Regenerate** - Run `cargo build` to regenerate `tags.rs`
3. **Parser support** - Add parsing logic in relevant parser

## Testing Strategy

### Unit Tests
- Individual parser components
- Tag database lookups
- Binary parsing functions

### Integration Tests
- Complete file read/write cycles
- ExifTool comparison tests
- Format validation tests

### Fuzzing
- Continuous fuzzing with cargo-fuzz
- Format-specific fuzz targets
- Crash reproduction and regression tests

## Security Considerations

### Memory Safety
- No unsafe code in critical paths
- Bounds checking on all array accesses
- UTF-8 validation for strings

### Input Validation
- Magic byte verification
- Size limits on allocations
- CRC checks where available

### Atomic Operations
- File writes are atomic (temp + rename)
- No partial updates visible to other processes
- Backup original on modification

## Future Architecture

### Planned Enhancements

1. **Async I/O** - Non-blocking file operations for GUI integration
2. **Plugin System** - Loadable parsers for proprietary formats
3. **Network Streaming** - Process files from HTTP/S3 without download
4. **GPU Acceleration** - Parallel processing of large image batches

### API Stability

- **Public API** - Semver guarantees for library users
- **Internal APIs** - May change between minor versions
- **FFI API** - C ABI stability for cross-language bindings

## References

- [Hexagonal Architecture](https://alistair.cockburn.us/hexagonal-architecture/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [ExifTool Documentation](https://exiftool.org/TagNames/)
- [EXIF Specification](http://www.cipa.jp/std/documents/e/DC-008-2012_E.pdf)
- [TIFF Specification](https://www.adobe.io/content/dam/udp/en/open/standards/tiff/TIFF6.pdf)
