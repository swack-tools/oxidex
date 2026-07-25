# Tag Coverage Implementation Plan

**Created:** 2025-12-03
**Status:** Ready for Implementation
**Scope:** Quick wins (IPTC/XMP/ICC wiring) + Comprehensive executable analysis (Mach-O/ELF)

## Executive Summary

This plan addresses gaps identified by `just docs-coverage`:

- **Quick wins:** Wire existing IPTC, XMP, ICC parsers to extraction pipeline (~90 new tags)
- **Executable analysis:** Enhance Mach-O and ELF parsers to PE-level coverage (~107 new tags)

**Total impact:** ~197 new tags, 7-12 days effort

---

## Phase 1: Quick Wins - Wire Existing Parsers

### Background

Parsers exist but aren't connected to the main pipeline:

| Parser | File | Size | Status |
|--------|------|------|--------|
| IPTC | `src/parsers/jpeg/iptc_parser.rs` | 16KB | Exists, not wired |
| XMP | `src/parsers/xmp/*.rs` | 30KB | Exists, not wired |
| ICC | `src/parsers/icc/*.rs` | 20KB+ | Partial wiring |

### Architecture

```
JPEG file → segment_parser.rs → identifies APP1/APP13/APP2 segments
                                    ↓
                              Currently: Only EXIF extracted
                              Needed: Call iptc_parser, xmp_parser, icc_parser
```

### Implementation Tasks

#### 1.1 Wire IPTC (APP13 segments)

**File:** `src/parsers/jpeg/segment_parser.rs`

```rust
// Add to segment processing
const APP13_MARKER: u16 = 0xFFED;

if segment.marker == APP13_MARKER {
    if let Ok(iptc_tags) = parse_iptc_segment(&segment) {
        for (key, value) in iptc_tags {
            tags.insert(format!("IPTC:{}", key), value);
        }
    }
}
```

**Expected tags (~40):**
- Headline, Caption, Keywords, Category
- Copyright, Credit, Source, Writer
- City, Country, Province, Location
- DateCreated, TimeCreated, DigitalCreationDate

#### 1.2 Wire XMP (APP1 non-EXIF segments)

**File:** `src/parsers/jpeg/segment_parser.rs`

```rust
// XMP starts with "http://ns.adobe.com/xap/1.0/"
const XMP_SIGNATURE: &[u8] = b"http://ns.adobe.com/xap/1.0/";

if segment.marker == APP1_MARKER && segment.data.starts_with(XMP_SIGNATURE) {
    if let Ok(xmp_tags) = parse_xmp(&segment.data[XMP_SIGNATURE.len()..]) {
        for (key, value) in xmp_tags {
            tags.insert(format!("XMP:{}", key), value);
        }
    }
}
```

**Expected tags (~30):**
- Creator, Description, Rights, Title
- CreateDate, ModifyDate, MetadataDate
- Rating, Label, Subject
- CreatorTool, DocumentID, InstanceID

#### 1.3 Wire ICC (APP2 segments)

**File:** `src/parsers/jpeg/segment_parser.rs`

```rust
const APP2_MARKER: u16 = 0xFFE2;
const ICC_SIGNATURE: &[u8] = b"ICC_PROFILE\0";

if segment.marker == APP2_MARKER && segment.data.starts_with(ICC_SIGNATURE) {
    if let Ok(icc_tags) = parse_icc_profile(&segment.data[ICC_SIGNATURE.len()..]) {
        for (key, value) in icc_tags {
            tags.insert(format!("ICC:{}", key), value);
        }
    }
}
```

**Expected tags (~20):**
- ProfileDescription, ColorSpace, ProfileClass
- RenderingIntent, ProfileCreator, ProfileVersion
- PrimaryPlatform, DeviceManufacturer, DeviceModel

### Phase 1 Milestones

| Milestone | Success Criteria | Verification |
|-----------|------------------|--------------|
| 1.1 Wire IPTC | IPTC tags in JPEG output | `cargo test iptc` |
| 1.2 Wire XMP | XMP tags in JPEG/PNG output | `cargo test xmp` |
| 1.3 Wire ICC | ICC profile tags extracted | `cargo test icc` |
| 1.4 Update coverage | Report shows improvement | `just docs-coverage` |

---

## Phase 2: Mach-O Comprehensive Parser

### Mach-O Structure

```
┌─────────────────────────┐
│ Mach Header (32/64-bit) │ → CPU type, file type, flags
├─────────────────────────┤
│ Load Commands           │ → Segments, dylibs, UUID, version info
│  ├─ LC_SEGMENT[_64]     │ → Code/data sections
│  ├─ LC_DYLIB            │ → Linked libraries
│  ├─ LC_UUID             │ → Build UUID
│  ├─ LC_VERSION_MIN_*    │ → Min OS version
│  ├─ LC_BUILD_VERSION    │ → SDK version, platform
│  ├─ LC_CODE_SIGNATURE   │ → Code signing info
│  └─ LC_SYMTAB           │ → Symbol table
├─────────────────────────┤
│ Section Data            │ → Actual code/data
└─────────────────────────┘
```

### File Structure

```
src/parsers/macho/
├── mod.rs                    # Public API, FormatParser impl
├── structures.rs             # Mach-O structs (header, load commands)
├── header_parser.rs          # Mach header parsing (~150 lines)
├── load_command_parser.rs    # Load command dispatcher (~300 lines)
├── segment_parser.rs         # LC_SEGMENT parsing (~200 lines)
├── dylib_parser.rs           # LC_DYLIB, LC_LOAD_DYLIB (~150 lines)
├── version_parser.rs         # Version/build info (~100 lines)
├── signature_parser.rs       # Code signature (~200 lines)
├── symbol_parser.rs          # Symbol table (~200 lines)
└── metadata_extractor.rs     # Orchestrates extraction (~300 lines)
```

**Estimated: ~1,600 lines across 10 files**

### Tags to Extract (~55 tags)

#### Header Tags
- `MachO:CPUType` - ARM64, x86_64, etc.
- `MachO:CPUSubtype` - Specific CPU variant
- `MachO:FileType` - Execute, Dylib, Bundle, Object
- `MachO:Flags` - PIE, TWOLEVEL, etc.
- `MachO:ByteOrder` - Little/Big endian
- `MachO:Is64Bit` - Boolean

#### Version Tags
- `MachO:MinOSVersion` - Minimum OS version
- `MachO:SDKVersion` - SDK used to build
- `MachO:Platform` - macOS, iOS, tvOS, watchOS
- `MachO:SourceVersion` - Source version string
- `MachO:BuildVersion` - Build tool version

#### Identity Tags
- `MachO:UUID` - Unique build identifier
- `MachO:DylibCurrentVersion` - Library version
- `MachO:DylibCompatVersion` - Compatibility version

#### Segment Tags
- `MachO:SegmentCount` - Number of segments
- `MachO:SectionCount` - Total sections
- `MachO:TextSegmentSize` - __TEXT size
- `MachO:DataSegmentSize` - __DATA size

#### Dependency Tags
- `MachO:DylibCount` - Linked libraries count
- `MachO:DylibPaths` - Library paths (array)
- `MachO:RPaths` - Runtime search paths
- `MachO:WeakDylibCount` - Optional libraries

#### Symbol Tags
- `MachO:SymbolCount` - Total symbols
- `MachO:ExportedSymbolCount` - Exported symbols
- `MachO:ImportedSymbolCount` - Imported symbols
- `MachO:UndefinedSymbolCount` - Undefined symbols

#### Code Signing Tags
- `MachO:CodeSignatureSize` - Signature blob size
- `MachO:SigningAuthority` - Certificate CN
- `MachO:TeamIdentifier` - Apple Team ID
- `MachO:CDHash` - Code directory hash

#### Entry Tags
- `MachO:EntryPointOffset` - LC_MAIN entry
- `MachO:StackSize` - Stack size hint
- `MachO:MainFunction` - Entry function name

### Phase 2 Milestones

| Milestone | Success Criteria | Verification |
|-----------|------------------|--------------|
| 2.1 Header parsing | CPUType, FileType extracted | Unit tests |
| 2.2 Load commands | Segments, dylibs, UUID parsed | Real binaries |
| 2.3 Version info | MinOS, SDK, BuildVersion | Test `/bin/ls` |
| 2.4 Code signature | Signing info extracted | Signed apps |
| 2.5 Symbols | Symbol counts extracted | Compare with `nm` |
| 2.6 Integration | Full metadata map | Integration tests |

---

## Phase 3: ELF Comprehensive Parser

### ELF Structure

```
┌─────────────────────────┐
│ ELF Header              │ → Class, endianness, machine type, entry
├─────────────────────────┤
│ Program Headers         │ → Runtime segments (LOAD, DYNAMIC, INTERP)
│  ├─ PT_LOAD             │ → Loadable segments
│  ├─ PT_DYNAMIC          │ → Dynamic linking info
│  ├─ PT_INTERP           │ → Interpreter path (ld-linux.so)
│  └─ PT_NOTE             │ → Build ID, ABI info
├─────────────────────────┤
│ Section Headers         │ → Compile-time sections
│  ├─ .text               │ → Executable code
│  ├─ .data/.rodata       │ → Data sections
│  ├─ .symtab/.dynsym     │ → Symbol tables
│  ├─ .strtab/.dynstr     │ → String tables
│  ├─ .dynamic            │ → Dynamic linking table
│  └─ .gnu.hash           │ → Symbol hash table
└─────────────────────────┘
```

### File Structure

```
src/parsers/elf/
├── mod.rs                    # Public API, FormatParser impl
├── structures.rs             # ELF structs (Elf32/64_Ehdr, Phdr, Shdr)
├── header_parser.rs          # ELF header parsing (~150 lines)
├── program_header_parser.rs  # Program headers (~200 lines)
├── section_header_parser.rs  # Section headers (~250 lines)
├── dynamic_parser.rs         # .dynamic section, DT_* entries (~200 lines)
├── symbol_parser.rs          # Symbol table parsing (~200 lines)
├── note_parser.rs            # PT_NOTE (build ID, GNU ABI) (~150 lines)
├── relocation_parser.rs      # Relocation info (~150 lines)
└── metadata_extractor.rs     # Orchestrates extraction (~300 lines)
```

**Estimated: ~1,600 lines across 10 files**

### Tags to Extract (~52 tags)

#### Header Tags
- `ELF:Class` - 32-bit or 64-bit
- `ELF:Endianness` - Little/Big endian
- `ELF:OSABI` - Linux, FreeBSD, etc.
- `ELF:ABIVersion` - ABI version
- `ELF:ObjectType` - Executable, Shared, Relocatable
- `ELF:Machine` - x86_64, ARM, RISC-V, etc.
- `ELF:Version` - ELF version

#### Entry Tags
- `ELF:EntryPoint` - Entry address
- `ELF:Flags` - Processor-specific flags
- `ELF:HeaderSize` - ELF header size
- `ELF:PHOffset` - Program header offset
- `ELF:SHOffset` - Section header offset

#### Program Header Tags
- `ELF:PHCount` - Program header count
- `ELF:PHSize` - Program header entry size
- `ELF:LoadableSegmentCount` - PT_LOAD count
- `ELF:StackExecutable` - Stack execution flag

#### Section Tags
- `ELF:SHCount` - Section header count
- `ELF:SHSize` - Section header entry size
- `ELF:SectionNames` - List of section names
- `ELF:TextSectionSize` - .text size
- `ELF:DataSectionSize` - .data size

#### Dynamic Linking Tags
- `ELF:Interpreter` - Dynamic linker path
- `ELF:SharedObjectCount` - NEEDED entries count
- `ELF:SharedObjects` - Library names (array)
- `ELF:RPaths` - DT_RPATH values
- `ELF:RunPaths` - DT_RUNPATH values

#### Symbol Tags
- `ELF:SymbolCount` - .symtab symbol count
- `ELF:DynamicSymbolCount` - .dynsym count
- `ELF:ExportedSymbols` - GLOBAL symbols
- `ELF:ImportedFunctions` - UNDEFINED symbols

#### Build Info Tags
- `ELF:BuildID` - GNU build ID
- `ELF:GNURelro` - RELRO enabled
- `ELF:PIEEnabled` - Position independent
- `ELF:StackCanary` - Stack protection

#### Note Tags
- `ELF:GNUBuildID` - Build identifier
- `ELF:ABITag` - GNU ABI tag
- `ELF:GoldVersion` - Gold linker version

### Phase 3 Milestones

| Milestone | Success Criteria | Verification |
|-----------|------------------|--------------|
| 3.1 Header parsing | Class, Machine, Entry extracted | Unit tests |
| 3.2 Program headers | Segments, interpreter parsed | Real binaries |
| 3.3 Section headers | Section names, sizes extracted | Test `/bin/ls` |
| 3.4 Dynamic linking | Shared objects, rpaths extracted | Compare `ldd` |
| 3.5 Symbols | Symbol counts extracted | Compare `readelf` |
| 3.6 Build info | BuildID, security flags | Integration tests |

---

## Summary

### Total Scope

| Component | New Tags | Lines of Code | Effort |
|-----------|----------|---------------|--------|
| Quick Wins (IPTC/XMP/ICC) | ~90 | ~150 | 1-2 days |
| Mach-O Parser | ~55 | ~1,600 | 3-5 days |
| ELF Parser | ~52 | ~1,600 | 3-5 days |
| **Total** | **~197** | **~3,350** | **7-12 days** |

### Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Existing parsers return incompatible format | Check return types first, add adapter if needed |
| Mach-O universal binaries (fat files) | Handle FAT header, extract first architecture initially |
| ELF 32/64-bit divergence | Use generic structs with size parameters |
| Code signature parsing complexity | Start with basic info, enhance iteratively |
| Test file availability | Use system binaries (`/bin/ls`, `/usr/bin/file`) |

### Dependencies

- No new crate dependencies required (nom already available)
- Reference: `src/parsers/pe/` for patterns
- Test binaries: System executables on macOS/Linux

### Definition of Done

1. `just docs-coverage` shows:
   - IPTC, XMP, ICC > 50%
   - Mach-O, ELF > 80%
2. All new code has unit tests
3. Integration tests against real binaries pass
4. Documentation updated

---

## Next Steps

1. Review and approve this plan
2. Create git worktree for implementation
3. Execute Phase 1 (Quick Wins) first
4. Execute Phase 2 (Mach-O) or Phase 3 (ELF) based on priority
