# Parser Complexity Reduction Design

## Overview

Refactor all parsers in `src/parsers/` to use a shared IO module and split large files into focused submodules. This eliminates ~2,500 lines of duplicated binary reading code and improves maintainability.

## Goals

1. Create shared `src/io/` module for binary reading utilities
2. Split large parser files (>1,000 lines) into focused submodules
3. Consolidate timestamp conversion utilities
4. Reduce total codebase by ~1,700 lines

## Shared IO Module

### Structure

```
src/io/
├── mod.rs                 # Public exports
├── endian_reader.rs       # Generic binary reading with byte order
├── cursor.rs              # Stateful cursor for sequential reads
└── timestamp.rs           # Cross-format timestamp conversions
```

### endian_reader.rs

Core abstraction used by 15+ parsers:

```rust
pub enum ByteOrder {
    Big,
    Little,
}

pub struct EndianReader<'a> {
    data: &'a [u8],
    order: ByteOrder,
}

impl<'a> EndianReader<'a> {
    pub fn new(data: &'a [u8], order: ByteOrder) -> Self;
    pub fn big_endian(data: &'a [u8]) -> Self;
    pub fn little_endian(data: &'a [u8]) -> Self;

    // Integer reads
    pub fn u8_at(&self, offset: usize) -> Option<u8>;
    pub fn u16_at(&self, offset: usize) -> Option<u16>;
    pub fn u32_at(&self, offset: usize) -> Option<u32>;
    pub fn u64_at(&self, offset: usize) -> Option<u64>;
    pub fn i16_at(&self, offset: usize) -> Option<i16>;
    pub fn i32_at(&self, offset: usize) -> Option<i32>;
    pub fn i64_at(&self, offset: usize) -> Option<i64>;

    // Float reads
    pub fn f32_at(&self, offset: usize) -> Option<f32>;
    pub fn f64_at(&self, offset: usize) -> Option<f64>;

    // Rational (for EXIF)
    pub fn rational_at(&self, offset: usize) -> Option<(u32, u32)>;
    pub fn srational_at(&self, offset: usize) -> Option<(i32, i32)>;

    // String reads
    pub fn str_at(&self, offset: usize, len: usize) -> Option<&'a str>;
    pub fn cstr_at(&self, offset: usize, max_len: usize) -> Option<&'a str>;

    pub fn len(&self) -> usize;
    pub fn slice(&self, start: usize, end: usize) -> Option<&'a [u8]>;
}
```

### cursor.rs

Stateful sequential reader for variable-length formats:

```rust
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    order: ByteOrder,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8], order: ByteOrder) -> Self;

    // Read and advance position
    pub fn read_u8(&mut self) -> Option<u8>;
    pub fn read_u16(&mut self) -> Option<u16>;
    pub fn read_u32(&mut self) -> Option<u32>;
    pub fn read_u64(&mut self) -> Option<u64>;
    pub fn read_bytes(&mut self, len: usize) -> Option<&'a [u8]>;
    pub fn read_cstring(&mut self, max_len: usize) -> Option<&'a str>;

    // Position management
    pub fn position(&self) -> usize;
    pub fn seek(&mut self, pos: usize);
    pub fn skip(&mut self, count: usize);
    pub fn remaining(&self) -> usize;
}
```

### timestamp.rs

Unified timestamp conversions:

```rust
/// Windows FILETIME (100-nanosecond intervals since 1601-01-01)
pub fn filetime_to_iso8601(filetime: u64) -> Option<String>;
pub fn filetime_to_unix(filetime: u64) -> Option<i64>;

/// Mac/QuickTime time (seconds since 1904-01-01)
pub fn mac_time_to_iso8601(mac_time: u64) -> Option<String>;
pub fn mac_time_to_unix(mac_time: u64) -> Option<i64>;

/// Unix timestamp to ISO 8601
pub fn unix_to_iso8601(unix_time: i64) -> String;
```

## Parser Restructuring

### QuickTime Parser

```
src/parsers/quicktime/
├── mod.rs                    # Public API, FormatParser impl
├── atom_parser.rs            # (existing) Atom structure parsing
├── metadata_extractor.rs     # Slim coordinator (~200 lines)
├── extractors/
│   ├── mod.rs
│   ├── movie_header.rs       # extract_movie_header, track headers
│   ├── user_data.rs          # extract_user_data_atoms, iTunes metadata
│   ├── mp4_metadata.rs       # extract_mp4_metadata (keys/ilst)
│   └── heif_exif.rs          # HEIF/HEIC EXIF extraction
└── constants.rs              # Atom type codes, handler types
```

### MKV Parser

```
src/parsers/video/mkv/
├── mod.rs                    # Public API, FormatParser impl
├── ebml.rs                   # EBML-specific: read_vint, read_vint_id
├── elements.rs               # Element ID constants, parse_element_header
├── extractors/
│   ├── mod.rs
│   ├── info.rs               # parse_info (duration, dates, apps)
│   ├── tracks.rs             # parse_tracks, video_info, audio_info
│   ├── tags.rs               # parse_tags, simple_tag
│   └── chapters.rs           # parse_chapters, chapter_atom
└── constants.rs              # EBML element IDs
```

### PCAP Parser

```
src/parsers/specialized/pcap/
├── mod.rs                    # Public API, FormatParser impl
├── pcap_classic.rs           # Classic PCAP format parsing
├── pcap_ng.rs                # PCAP-NG block parsing (SHB, IDB, EPB)
├── options.rs                # PCAP-NG option parsing
└── constants.rs              # Link layer types, block types, magic numbers
```

### LNK Parser

```
src/parsers/specialized/lnk/
├── mod.rs                    # Public API, FormatParser impl
├── header.rs                 # Shell link header parsing (76 bytes)
├── link_info.rs              # LinkInfo structure (volume, paths)
├── string_data.rs            # String data blocks (name, args, etc.)
├── extra_data.rs             # Extra data blocks (tracker, property store)
└── constants.rs              # Flags, GUIDs, block signatures
```

## Parsers Affected

### High Impact (duplicate readers)

| Parser | Current Duplication | Will Use |
|--------|---------------------|----------|
| `tiff/ifd_parser.rs` | `ByteOrder` enum, manual reads | `io::EndianReader` |
| `jpeg/segment_parser.rs` | Big-endian reads inline | `io::EndianReader::big_endian()` |
| `png/chunk_parser.rs` | Big-endian u32 reads | `io::EndianReader::big_endian()` |
| `pe/metadata_extractor.rs` | Little-endian reads | `io::EndianReader::little_endian()` |
| `elf/structures.rs` | Configurable endianness | `io::EndianReader` |
| `macho/structures.rs` | Little-endian reads | `io::EndianReader::little_endian()` |
| `archive/rar.rs` | Little-endian reads | `io::EndianReader::little_endian()` |
| `raw/metadata.rs` | `read_u32` function | `io::EndianReader` |

### Medium Impact (timestamp conversions)

| Parser | Current | Will Use |
|--------|---------|----------|
| `specialized/evtx.rs` | FILETIME inline | `io::timestamp::filetime_to_iso8601()` |
| `specialized/prefetch.rs` | FILETIME inline | `io::timestamp::filetime_to_iso8601()` |
| `specialized/registry.rs` | FILETIME inline | `io::timestamp::filetime_to_iso8601()` |

## Implementation Phases

### Phase 1: Foundation (src/io/) - Sequential

```
1.1 Create src/io/mod.rs, endian_reader.rs
1.2 Create src/io/cursor.rs
1.3 Create src/io/timestamp.rs
1.4 Add comprehensive unit tests
1.5 Verify: cargo test --lib
```

### Phase 2: P3 Parsers - 4 Parallel Agents

```
Agent A: quicktime/ restructure
Agent B: video/mkv/ restructure
Agent C: specialized/pcap/ restructure
Agent D: specialized/lnk/ restructure
```

### Phase 3: Windows Forensic Parsers - 4 Parallel Agents

```
Agent A: specialized/evtx.rs
Agent B: specialized/prefetch.rs
Agent C: specialized/registry.rs
Agent D: pe/ restructure
```

### Phase 4: Binary Format Parsers - 4 Parallel Agents

```
Agent A: elf/ restructure
Agent B: macho/ restructure
Agent C: archive/rar.rs
Agent D: archive/sevenz.rs + other archives
```

### Phase 5: Image/Media Parsers - 4 Parallel Agents

```
Agent A: tiff/ (complex - ifd_parser, makernotes)
Agent B: jpeg/segment_parser.rs + png/chunk_parser.rs
Agent C: raw/metadata.rs + image formats (bmp, ico, psd)
Agent D: remaining media parsers (gif, webp, etc.)
```

### Phase 6: Cleanup - 2 Parallel Agents

```
Agent A: Dead code removal, clippy fixes
Agent B: Documentation updates
```

## Parallelism Strategy

### Dependency Graph

```
Phase 1 ──┬──> Phase 2 (4 agents) ──┬──> Phase 6
          │                         │
          ├──> Phase 3 (4 agents) ──┤
          │                         │
          ├──> Phase 4 (4 agents) ──┤
          │                         │
          └──> Phase 5 (4 agents) ──┘
```

Phases 2-5 can run in parallel after Phase 1 completes.

## Error Handling

The `io::EndianReader` returns `Option<T>` for safety. Parsers convert to errors at their boundary:

```rust
let width = reader.u32_at(offset)
    .ok_or_else(|| ExifToolError::parse_error("Failed to read width"))?;
```

## Testing Strategy

1. Each new module gets dedicated unit tests
2. All existing parser tests must pass unchanged
3. Run `just ci` after each phase
4. Integration tests validate end-to-end behavior

## Expected Impact

- ~45 files modified
- ~2,500 lines removed (duplicates)
- ~800 lines added (shared io/ module + new submodules)
- Net reduction: ~1,700 lines
