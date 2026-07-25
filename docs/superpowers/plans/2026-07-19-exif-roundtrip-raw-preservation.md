# EXIF Round-Trip Raw-Value Preservation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `modify_tag` / `remove_tag` / `copy_metadata` / `write_metadata` work on real camera JPEGs without corrupting or silently dropping any EXIF data ([issue #20](https://github.com/swack-tools/oxidex/issues/20)), and make the EXIF datetime locator tolerate corrupt IFD offsets gracefully.

**Architecture:** The current write path rebuilds the whole EXIF segment from the display-converted `MetadataMap` (`reconstruct_tiff_structure`), which (a) fails strict validation on any binary/rational display string, (b) would corrupt if validation were relaxed, and (c) silently drops MakerNotes, InteropIFD, IFD1/thumbnails, and unknown tags, while forcing little-endian. The fix is a **surgical writer with raw carry-over** in a new module `src/writers/exif_surgical.rs`: scan the original EXIF into raw entries; diff against the desired map using the *actual reader output* for symmetry; carry unchanged/unsurfaced entries byte-for-byte (validation never sees them); strictly validate and re-serialize only entries the caller changed or added; preserve the original byte order; keep the MakerNotes blob at its original offset so manufacturer-internal absolute offsets stay valid.

**Tech Stack:** Rust (existing workspace). Reuses: `parse_segments`, `read_u16`/`read_u32`, `ByteOrder`, `raw_bytes_to_tag_value`, `lookup_tag_name`, `get_tag_descriptor`/`has_reliable_value_type`, `validate_tag_value_with_name`, `write_atomic`, `parse_jpeg_metadata`, the `SliceReader` pattern from `exif_inplace.rs`.

## Global Constraints

- No new external dependencies (dev-dependency `tempfile` already exists and is used by `tests/date_shift_inplace.rs` — use its pattern for new integration tests).
- **Do NOT relax `src/core/validation.rs`.** Strict validation stays for every value the caller changed or added. Unchanged tags are carried as raw bytes and never re-validated — that is the design, not a validation bypass: raw carry-over cannot change a byte.
- Reference for "no corruption": byte-level and semantic parity assertions (the `diff_indices`-style pattern from `tests/date_shift_inplace.rs`).
- `cargo clippy` no NEW warnings; `cargo fmt --all -- --check` clean before every commit (CI enforces; `cargo fmt` on stable prints pre-existing "unstable features" warnings — ignore those, only diffs matter).
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Branch: `claude/exif-roundtrip-fix` (stacked on `claude/oxidex-issue-14-fa741f`, PR #18). Working directory: the `oxidex-issue-14-fa741f` worktree.
- TIFF file writes stay blocked (`write_metadata`'s `FileFormat::TIFF` arm unchanged). PNG/PDF paths unchanged. XMP/IPTC/ICC JPEG segments are already preserved verbatim by `write_exif_to_jpeg` — unchanged.

## File Structure

| File | Responsibility |
|---|---|
| `src/writers/exif_inplace.rs` (modify) | Task 1 only: graceful handling of corrupt IFD offsets in `scan_ifd` |
| `src/writers/exif_surgical.rs` (create) | Raw entry scanner, diff engine, layout/serializer, orchestration (`rewrite_jpeg_exif`) |
| `src/writers/mod.rs` (modify) | Register `exif_surgical` |
| `src/writers/jpeg_writer.rs` (modify) | Route EXIF segment construction through `rewrite_jpeg_exif` (real reader, no more DummyReader) |
| `src/core/operations.rs` (modify) | `write_metadata`: skip whole-map PHASE-1 validation for JPEG (the surgical path validates changed/added tags itself); non-JPEG unchanged |
| `tests/exif_roundtrip.rs` (create) | Round-trip regression suite: no-op parity, canary raw bytes, modify/remove/copy/clear, MakerNotes survival, CLI |

Verified codebase facts the implementer needs (file:line refs from investigation):

- `reconstruct_tiff_structure(_original_reader, byte_order, metadata)` ignores its reader (`src/writers/tiff_writer/mod.rs:138-147`); `build_exif_segment` passes a `DummyReader` and hardcodes little-endian (`src/writers/jpeg_writer.rs:201-218`). Only IFD0/ExifIFD/GPS are rebuilt; `IFD1:` merges into IFD0's bucket; Interop/MakerNotes/unknown/hex-named tags are silently dropped (`src/writers/tiff_writer/tiff/validator.rs:110-129`, `ifd_builder.rs:93-96`); `TagValue::String` is always serialized as ASCII regardless of true type (`ifd_entry.rs:101-113`); `Float`/`Struct`/`Array` are dropped (`ifd_entry.rs:150-152`).
- Read-side conversion: `raw_bytes_to_tag_value(bytes, field_type, count, tag_id, byte_order) -> TagValue` (`src/core/tag_conversion.rs:40-102`; exact parameter types: match the existing signature). Produces display Strings for ComponentsConfiguration/GPSVersionID/GPS coordinates/ExposureTime/LensInfo etc.; `TagValue::DateTime` for datetimes; `TagValue::Binary` for generic UNDEFINED.
- `lookup_tag_name(tag_id: u16, ifd: &str) -> String` (`crate::tag_db`): `lookup_tag_name(0x010F, "IFD0")` → `"IFD0:Make"`; unknown → `"IFD0:0xF999"`.
- `parse_jpeg_metadata(reader: &dyn FileReader) -> Result<MetadataMap>` is `pub(crate)` in `src/core/operations.rs:476` — the exact reader the diff must mirror. It ends with `normalize_metadata_map` (EXIF-family keys unchanged by it).
- `get_tag_descriptor(name)` normalizes `IFD0:`/`IFD1:`/`ExifIFD:`/`InteropIFD:` prefixes to `EXIF:` for lookup and returns the numeric tag id (`src/tag_db/tag_registry.rs:6942-6982`); the physical IFD must come from the key's string prefix. `validate_tag_value_with_name(tag_name, descriptor, value)` + `has_reliable_value_type` are used at `src/core/operations.rs:193-201`; `validate_tag_value_intrinsics` for unreliable-typed tags.
- Pointer tags: ExifIFD = IFD0 tag 0x8769; GPS = IFD0 tag 0x8825; Interop = ExifIFD tag 0xA005; thumbnail = IFD1 tags 0x0201 (JPEGInterchangeFormat, offset) / 0x0202 (length); MakerNote = ExifIFD tag 0x927C.
- `Segment { marker, offset, data }` + `parse_segments` as in `exif_inplace.rs`; EXIF TIFF starts at `segment.offset + 4 + 6`.
- Fixtures: `tests/fixtures/jpeg/complex/synthetic_gps_001.jpg` (GPS + ComponentsConfiguration + DateTimeOriginal 2024-02-01T14:30:00); `tests/fixtures/jpeg/makernotes/canon_sample.jpg` (Canon MakerNotes + all three date tags = 2003:12:14 12:01:44); `tests/fixtures/jpeg/sample_with_exif.jpg` (minimal: Make/Model/ModifyDate).
- `tests/date_shift_inplace.rs` has the `fixture`/`temp_copy`/`diff_indices` helper pattern (now tempfile-based after commit 69d06db — copy its current helper code, not the older env::temp_dir version).

## Design Rules (the diff contract)

For each raw entry found in the original EXIF, with `key = lookup_tag_name(tag_id, ifd_prefix)`:

| Condition | Disposition |
|---|---|
| entry is a pointer tag (0x8769, 0x8825, 0xA005, 0x0201, 0x0202) | **Structural** — never carried; regenerated by the serializer |
| entry is MakerNote (ExifIFD 0x927C), or lives in InteropIFD or IFD1 | **CarryRaw always** (reader doesn't surface these reliably; MakerNote additionally pinned to its original offset) |
| `key` NOT in `original_map` (reader didn't surface it) | **CarryRaw** — never drop what the reader hides |
| `key` in `original_map` but NOT in `desired` | **Remove** (removal-by-absence is `write_metadata`'s documented contract; `remove_tag` relies on it) |
| `desired[key] == original_map[key]` | **CarryRaw** — the caller didn't touch it |
| otherwise | **Changed** — strict validation + true-typed serialization |

Desired keys with prefix `IFD0:`/`ExifIFD:`/`GPS:`/`EXIF:` that matched no original entry are **Added** (descriptor required; unknown names error). `EXIF:`-prefixed keys route to IFD0 (compatibility with today's `separate_by_ifd` and the existing jpeg_writer tests). All other prefixes (`XMP:`, `File:`, `Canon:`, …) are ignored by the EXIF writer, as today.

Special case: if `desired` contains **zero** EXIF-family keys (`clear_all_metadata`), the EXIF segment is dropped entirely — including MakerNotes/IFD1 (privacy semantics; matches the intent of clearing).

Byte order: preserved from the original TIFF header; little-endian only when there was no original EXIF.

---

### Task 1: Graceful degradation in the datetime locator (fix 2 from the review)

**Files:**
- Modify: `src/writers/exif_inplace.rs` (`scan_ifd` lines ~100-105 and its callers/tests)

**Interfaces:**
- Consumes/Produces: `locate_exif_datetimes(tiff) -> Result<Vec<LocatedDateTag>>` — signature unchanged. New semantics: a corrupt (out-of-bounds) IFD0 offset or ExifIFD-pointer offset stops that scan gracefully instead of failing the whole call. Header-level errors (len < 8, bad byte-order marker, magic != 42) remain hard errors.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/writers/exif_inplace.rs`:

```rust
    #[test]
    fn test_corrupt_ifd0_offset_returns_empty_not_err() {
        let mut tiff = build_test_tiff(ByteOrder::LittleEndian);
        // Point the header's IFD0 offset far beyond the buffer
        tiff[4..8].copy_from_slice(&50_000u32.to_le_bytes());
        let located = locate_exif_datetimes(&tiff).unwrap();
        assert!(located.is_empty());
    }

    #[test]
    fn test_corrupt_exif_ifd_pointer_keeps_ifd0_results() {
        let mut tiff = build_test_tiff(ByteOrder::LittleEndian);
        // ExifIFD pointer entry starts at 22; its value field is at 30..34
        tiff[30..34].copy_from_slice(&50_000u32.to_le_bytes());
        let located = locate_exif_datetimes(&tiff).unwrap();
        // ModifyDate from IFD0 must survive; the two ExifIFD tags are lost
        assert_eq!(located.len(), 1);
        assert_eq!(located[0].tag, ExifDateTag::ModifyDate);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib exif_inplace 2>&1 | tail -5`
Expected: the two new tests FAIL (`unwrap` on `Err("IFD offset beyond EXIF data")`).

- [ ] **Step 3: Implement**

In `scan_ifd`, replace the hard error on an out-of-range IFD offset:

```rust
    let entries_start = match offset.checked_add(2) {
        Some(end) if end <= tiff.len() => end,
        // A corrupt or truncated IFD offset (from untrusted file bytes) stops
        // this scan gracefully; header-level validation already ran upstream
        _ => return Ok(None),
    };
```

(Delete the previous `checked_add(2).ok_or_else(...)` + `if entries_start > tiff.len() { return Err(...) }` block.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib exif_inplace 2>&1 | tail -5` then `cargo test --test date_shift_inplace 2>&1 | tail -3`
Expected: all PASS (the JPEG patcher's behavior on well-formed files is unchanged).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --lib --tests 2>&1 | tail -3
git add src/writers/exif_inplace.rs
git commit -m "fix: tolerate corrupt IFD offsets in the EXIF datetime locator

A garbage IFD0 offset or ExifIFD pointer in untrusted file bytes now
stops the scan gracefully (returning whatever was already located)
instead of failing the whole shift. Header validation stays strict.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Raw entry scanner (`exif_surgical::scan_exif_entries`)

**Files:**
- Create: `src/writers/exif_surgical.rs`
- Modify: `src/writers/mod.rs` (add `pub mod exif_surgical;`)

**Interfaces:**
- Consumes: `read_u16`/`read_u32` (`crate::core::operations_helpers`), `ByteOrder` (`crate::parsers::tiff::ifd_parser`).
- Produces (used by Tasks 3-5):

```rust
pub enum IfdKind { Ifd0, ExifIfd, Gps, Interop, Ifd1 }   // with pub fn prefix(self) -> &'static str
pub struct RawEntry { pub ifd: IfdKind, pub tag_id: u16, pub field_type: u16, pub count: u32, pub value: Vec<u8> }
pub struct ExifScan {
    pub byte_order: ByteOrder,
    pub entries: Vec<RawEntry>,          // pointer tags (0x8769/0x8825/0xA005/0x0201/0x0202) excluded
    pub thumbnail: Option<Vec<u8>>,      // captured via IFD1 0x0201/0x0202
    pub makernote_offset: Option<usize>, // original value offset of ExifIFD 0x927C, when offset-stored
}
pub fn scan_exif_entries(tiff: &[u8]) -> Result<ExifScan>
pub(crate) fn type_size(field_type: u16) -> usize
```

- [ ] **Step 1: Create the module with tests and implementation together** (new module; the "failing" state is its absence)

Create `src/writers/exif_surgical.rs`:

```rust
//! Surgical EXIF rewriting with raw-value carry-over
//!
//! The whole-map rebuild in `tiff_writer` re-serializes every tag from its
//! display-converted `TagValue`, which cannot round-trip binary/rational
//! tags and silently drops MakerNotes, InteropIFD, IFD1, and unknown tags
//! (issue #20). This module instead diffs the caller's desired map against
//! the original file's raw IFD entries: entries the caller did not change
//! are carried byte-for-byte (and never re-validated — raw carry-over
//! cannot alter a byte), while changed/added entries pass strict validation
//! and true-typed serialization. The original byte order is preserved, and
//! the MakerNotes blob keeps its original offset so manufacturer-internal
//! absolute offsets stay valid.

use crate::core::operations_helpers::{read_u16, read_u32};
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// IFD0 tag pointing to the ExifIFD
const EXIF_IFD_POINTER: u16 = 0x8769;
/// IFD0 tag pointing to the GPS IFD
const GPS_IFD_POINTER: u16 = 0x8825;
/// ExifIFD tag pointing to the InteropIFD
const INTEROP_POINTER: u16 = 0xA005;
/// IFD1 thumbnail offset / length
const THUMBNAIL_OFFSET: u16 = 0x0201;
const THUMBNAIL_LENGTH: u16 = 0x0202;
/// ExifIFD MakerNote blob
const MAKERNOTE: u16 = 0x927C;

/// Which physical IFD an entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfdKind {
    Ifd0,
    ExifIfd,
    Gps,
    Interop,
    Ifd1,
}

impl IfdKind {
    /// The metadata-map key prefix the reader uses for this IFD.
    pub fn prefix(self) -> &'static str {
        match self {
            IfdKind::Ifd0 => "IFD0",
            IfdKind::ExifIfd => "ExifIFD",
            IfdKind::Gps => "GPS",
            IfdKind::Interop => "InteropIFD",
            IfdKind::Ifd1 => "IFD1",
        }
    }
}

/// One IFD entry with its raw value bytes (inline or offset-stored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    pub ifd: IfdKind,
    pub tag_id: u16,
    pub field_type: u16,
    pub count: u32,
    pub value: Vec<u8>,
}

/// Everything extracted from an original EXIF TIFF structure.
#[derive(Debug, Clone, PartialEq)]
pub struct ExifScan {
    pub byte_order: ByteOrder,
    /// All entries except structural pointer tags (regenerated on write)
    pub entries: Vec<RawEntry>,
    /// Thumbnail bytes captured via IFD1's JPEGInterchangeFormat pair
    pub thumbnail: Option<Vec<u8>>,
    /// Original value offset of the MakerNote blob (for offset-stable layout)
    pub makernote_offset: Option<usize>,
}

/// Byte size of one value of the given TIFF field type.
pub(crate) fn type_size(field_type: u16) -> usize {
    match field_type {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        _ => 1,             // unknown types: treat as opaque bytes
    }
}

/// Walks IFD0 (and ExifIFD, GPS, InteropIFD, IFD1) and returns every entry
/// with its raw value bytes. Pointer tags are consumed structurally, not
/// returned. Corrupt sub-structures degrade gracefully: an out-of-bounds
/// IFD offset or value offset skips that IFD/entry rather than erroring.
pub fn scan_exif_entries(tiff: &[u8]) -> Result<ExifScan> {
    if tiff.len() < 8 {
        return Err(ExifToolError::parse_error("EXIF TIFF structure too small"));
    }
    let byte_order = match &tiff[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => {
            return Err(ExifToolError::parse_error(
                "Invalid TIFF byte order marker in EXIF data",
            ));
        }
    };
    if read_u16(&tiff[2..4], byte_order) != 42 {
        return Err(ExifToolError::parse_error(
            "Invalid TIFF magic number in EXIF data",
        ));
    }

    let mut scan = ExifScan {
        byte_order,
        entries: Vec::new(),
        thumbnail: None,
        makernote_offset: None,
    };

    let ifd0_offset = read_u32(&tiff[4..8], byte_order) as usize;
    let ifd0 = walk_ifd(tiff, ifd0_offset, byte_order, IfdKind::Ifd0, &mut scan);

    if let Some(exif_off) = ifd0.exif_pointer {
        let exif = walk_ifd(tiff, exif_off, byte_order, IfdKind::ExifIfd, &mut scan);
        if let Some(interop_off) = exif.interop_pointer {
            walk_ifd(tiff, interop_off, byte_order, IfdKind::Interop, &mut scan);
        }
    }
    if let Some(gps_off) = ifd0.gps_pointer {
        walk_ifd(tiff, gps_off, byte_order, IfdKind::Gps, &mut scan);
    }
    if let Some(ifd1_off) = ifd0.next_ifd {
        let ifd1 = walk_ifd(tiff, ifd1_off, byte_order, IfdKind::Ifd1, &mut scan);
        if let (Some(t_off), Some(t_len)) = (ifd1.thumb_offset, ifd1.thumb_length)
            && t_off.checked_add(t_len).is_some_and(|end| end <= tiff.len())
        {
            scan.thumbnail = Some(tiff[t_off..t_off + t_len].to_vec());
        }
    }

    Ok(scan)
}

/// Pointers discovered while walking one IFD.
#[derive(Default)]
struct WalkResult {
    exif_pointer: Option<usize>,
    gps_pointer: Option<usize>,
    interop_pointer: Option<usize>,
    next_ifd: Option<usize>,
    thumb_offset: Option<usize>,
    thumb_length: Option<usize>,
}

fn walk_ifd(
    tiff: &[u8],
    offset: usize,
    byte_order: ByteOrder,
    which: IfdKind,
    scan: &mut ExifScan,
) -> WalkResult {
    let mut result = WalkResult::default();
    let entries_start = match offset.checked_add(2) {
        Some(end) if end <= tiff.len() => end,
        _ => return result, // corrupt IFD offset: skip this IFD gracefully
    };
    let entry_count = read_u16(&tiff[offset..entries_start], byte_order) as usize;

    for i in 0..entry_count {
        let entry_start = entries_start + i * 12;
        let entry_end = entry_start + 12;
        if entry_end > tiff.len() {
            return result; // truncated IFD: keep what we have
        }
        let entry = &tiff[entry_start..entry_end];
        let tag_id = read_u16(&entry[0..2], byte_order);
        let field_type = read_u16(&entry[2..4], byte_order);
        let count = read_u32(&entry[4..8], byte_order);
        let value_or_offset = read_u32(&entry[8..12], byte_order) as usize;

        // Structural pointers: record and continue (never stored as entries)
        match (which, tag_id) {
            (IfdKind::Ifd0, EXIF_IFD_POINTER) => {
                result.exif_pointer = Some(value_or_offset);
                continue;
            }
            (IfdKind::Ifd0, GPS_IFD_POINTER) => {
                result.gps_pointer = Some(value_or_offset);
                continue;
            }
            (IfdKind::ExifIfd, INTEROP_POINTER) => {
                result.interop_pointer = Some(value_or_offset);
                continue;
            }
            (IfdKind::Ifd1, THUMBNAIL_OFFSET) => {
                result.thumb_offset = Some(value_or_offset);
                continue;
            }
            (IfdKind::Ifd1, THUMBNAIL_LENGTH) => {
                result.thumb_length = Some(value_or_offset);
                continue;
            }
            _ => {}
        }

        let size = match type_size(field_type).checked_mul(count as usize) {
            Some(s) => s,
            None => continue,
        };
        let value = if size <= 4 {
            entry[8..8 + size].to_vec()
        } else {
            match value_or_offset.checked_add(size) {
                Some(end) if end <= tiff.len() => tiff[value_or_offset..end].to_vec(),
                _ => continue, // out-of-bounds value: skip entry, never guess
            }
        };

        if which == IfdKind::ExifIfd && tag_id == MAKERNOTE && size > 4 {
            scan.makernote_offset = Some(value_or_offset);
        }

        scan.entries.push(RawEntry {
            ifd: which,
            tag_id,
            field_type,
            count,
            value,
        });
    }

    // Next-IFD offset follows the entry table
    let next_at = entries_start + entry_count * 12;
    if which == IfdKind::Ifd0
        && next_at + 4 <= tiff.len()
    {
        let next = read_u32(&tiff[next_at..next_at + 4], byte_order) as usize;
        if next != 0 {
            result.next_ifd = Some(next);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16b(v: u16, bo: ByteOrder) -> [u8; 2] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }
    fn u32b(v: u32, bo: ByteOrder) -> [u8; 4] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }

    /// Layout (LE and BE identical offsets):
    ///   0   header (IFD0 at 8)
    ///   8   IFD0: 4 entries (Make ASCII@74, Orientation SHORT inline,
    ///       ExifIFD ptr -> 84, GPS ptr -> 150), next-IFD -> 176
    ///  62   next-IFD field (4 bytes at 8+2+4*12=58..62 -> value 176) -- see math below
    ///  74   "Canon\0" (6 bytes)
    ///  84   ExifIFD: 2 entries (ComponentsConfiguration UNDEFINED count 4
    ///       inline, MakerNote UNDEFINED count 8 @ 116), next=0
    /// 116   makernote bytes (8)
    /// 150   GPS: 1 entry (GPSVersionID BYTE count 4 inline), next=0
    /// 176   IFD1: 3 entries (Compression SHORT inline, 0x0201 -> 220,
    ///       0x0202 = 6), next=0
    /// 220   thumbnail bytes (6)
    fn build_full_tiff(bo: ByteOrder) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(match bo {
            ByteOrder::LittleEndian => b"II",
            ByteOrder::BigEndian => b"MM",
        });
        t.extend_from_slice(&u16b(42, bo));
        t.extend_from_slice(&u32b(8, bo));
        // IFD0 at 8: count=4, entries at 10..58, next at 58..62
        t.extend_from_slice(&u16b(4, bo));
        // Make (0x010F) ASCII count 6 @ 74
        t.extend_from_slice(&u16b(0x010F, bo));
        t.extend_from_slice(&u16b(2, bo));
        t.extend_from_slice(&u32b(6, bo));
        t.extend_from_slice(&u32b(74, bo));
        // Orientation (0x0112) SHORT count 1 inline = 6
        t.extend_from_slice(&u16b(0x0112, bo));
        t.extend_from_slice(&u16b(3, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u16b(6, bo));
        t.extend_from_slice(&u16b(0, bo)); // inline padding
        // ExifIFD pointer -> 84
        t.extend_from_slice(&u16b(0x8769, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(84, bo));
        // GPS pointer -> 150
        t.extend_from_slice(&u16b(0x8825, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(150, bo));
        // next IFD -> 176 (IFD1)
        t.extend_from_slice(&u32b(176, bo));
        // pad 62..74
        t.resize(74, 0);
        t.extend_from_slice(b"Canon\0"); // 74..80
        t.resize(84, 0);
        // ExifIFD at 84: count=2, entries 86..110, next 110..114
        t.extend_from_slice(&u16b(2, bo));
        // ComponentsConfiguration (0x9101) UNDEFINED count 4 inline [1,2,3,0]
        t.extend_from_slice(&u16b(0x9101, bo));
        t.extend_from_slice(&u16b(7, bo));
        t.extend_from_slice(&u32b(4, bo));
        t.extend_from_slice(&[1, 2, 3, 0]);
        // MakerNote (0x927C) UNDEFINED count 8 @ 116
        t.extend_from_slice(&u16b(0x927C, bo));
        t.extend_from_slice(&u16b(7, bo));
        t.extend_from_slice(&u32b(8, bo));
        t.extend_from_slice(&u32b(116, bo));
        t.extend_from_slice(&u32b(0, bo)); // next
        t.resize(116, 0);
        t.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04]); // 116..124
        t.resize(150, 0);
        // GPS at 150: count=1, entry 152..164, next 164..168
        t.extend_from_slice(&u16b(1, bo));
        // GPSVersionID (0x0000) BYTE count 4 inline [2,3,0,0]
        t.extend_from_slice(&u16b(0x0000, bo));
        t.extend_from_slice(&u16b(1, bo));
        t.extend_from_slice(&u32b(4, bo));
        t.extend_from_slice(&[2, 3, 0, 0]);
        t.extend_from_slice(&u32b(0, bo)); // next
        t.resize(176, 0);
        // IFD1 at 176: count=3, entries 178..214, next 214..218
        t.extend_from_slice(&u16b(3, bo));
        // Compression (0x0103) SHORT inline = 6
        t.extend_from_slice(&u16b(0x0103, bo));
        t.extend_from_slice(&u16b(3, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u16b(6, bo));
        t.extend_from_slice(&u16b(0, bo));
        // 0x0201 thumbnail offset -> 220
        t.extend_from_slice(&u16b(0x0201, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(220, bo));
        // 0x0202 thumbnail length = 6
        t.extend_from_slice(&u16b(0x0202, bo));
        t.extend_from_slice(&u16b(4, bo));
        t.extend_from_slice(&u32b(1, bo));
        t.extend_from_slice(&u32b(6, bo));
        t.extend_from_slice(&u32b(0, bo)); // next
        t.resize(220, 0);
        t.extend_from_slice(&[0xFF, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9]); // 220..226
        t
    }

    fn find<'a>(scan: &'a ExifScan, ifd: IfdKind, tag: u16) -> &'a RawEntry {
        scan.entries
            .iter()
            .find(|e| e.ifd == ifd && e.tag_id == tag)
            .unwrap()
    }

    #[test]
    fn scan_walks_all_ifds_le() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let scan = scan_exif_entries(&tiff).unwrap();
        assert_eq!(scan.byte_order, ByteOrder::LittleEndian);
        assert_eq!(find(&scan, IfdKind::Ifd0, 0x010F).value, b"Canon\0");
        assert_eq!(find(&scan, IfdKind::Ifd0, 0x0112).value, 6u16.to_le_bytes());
        assert_eq!(find(&scan, IfdKind::ExifIfd, 0x9101).value, [1, 2, 3, 0]);
        assert_eq!(
            find(&scan, IfdKind::ExifIfd, 0x927C).value,
            [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04]
        );
        assert_eq!(find(&scan, IfdKind::Gps, 0x0000).value, [2, 3, 0, 0]);
        assert_eq!(find(&scan, IfdKind::Ifd1, 0x0103).value, 6u16.to_le_bytes());
        assert_eq!(scan.makernote_offset, Some(116));
        assert_eq!(
            scan.thumbnail.as_deref(),
            Some(&[0xFF, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9][..])
        );
        // Pointer tags are structural, not entries
        assert!(!scan.entries.iter().any(|e| {
            matches!(e.tag_id, 0x8769 | 0x8825 | 0x0201 | 0x0202)
        }));
    }

    #[test]
    fn scan_walks_all_ifds_be() {
        let tiff = build_full_tiff(ByteOrder::BigEndian);
        let scan = scan_exif_entries(&tiff).unwrap();
        assert_eq!(scan.byte_order, ByteOrder::BigEndian);
        assert_eq!(find(&scan, IfdKind::Ifd0, 0x0112).value, 6u16.to_be_bytes());
        assert_eq!(find(&scan, IfdKind::ExifIfd, 0x9101).value, [1, 2, 3, 0]);
        assert_eq!(scan.thumbnail.as_deref().map(|t| t.len()), Some(6));
    }

    #[test]
    fn scan_survives_corrupt_pointers() {
        let mut tiff = build_full_tiff(ByteOrder::LittleEndian);
        // Corrupt the ExifIFD pointer value (entry at 34, value field 42..46)
        tiff[42..46].copy_from_slice(&60_000u32.to_le_bytes());
        let scan = scan_exif_entries(&tiff).unwrap();
        // ExifIFD entries gone, everything else intact
        assert!(!scan.entries.iter().any(|e| e.ifd == IfdKind::ExifIfd));
        assert!(scan.entries.iter().any(|e| e.ifd == IfdKind::Gps));
        assert!(scan.entries.iter().any(|e| e.ifd == IfdKind::Ifd1));
    }

    #[test]
    fn scan_rejects_invalid_header() {
        assert!(scan_exif_entries(&[]).is_err());
        assert!(scan_exif_entries(b"XX\x2a\x00\x08\x00\x00\x00").is_err());
    }

    #[test]
    fn scan_real_fixture_smoke() {
        // Extract the TIFF slice of a real fixture through parse_segments
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jpeg/makernotes/canon_sample.jpg"
        ))
        .unwrap();
        let tiff = super::super::exif_surgical_test_support::tiff_slice(&bytes);
        let scan = scan_exif_entries(tiff).unwrap();
        assert!(scan.entries.iter().any(|e| e.ifd == IfdKind::Ifd0 && e.tag_id == 0x0132));
        assert!(scan.entries.iter().any(|e| e.ifd == IfdKind::ExifIfd && e.tag_id == MAKERNOTE));
    }
}
```

Also add (in `src/writers/mod.rs`): `pub mod exif_surgical;` and a tiny shared test-support helper. Put this at the bottom of `src/writers/mod.rs`:

```rust
#[cfg(test)]
pub(crate) mod exif_surgical_test_support {
    /// Returns the TIFF slice of a JPEG's EXIF APP1 segment.
    pub fn tiff_slice(jpeg: &[u8]) -> &[u8] {
        // Minimal scan: find FFE1 whose payload starts with "Exif\0\0"
        let mut i = 2; // skip SOI
        while i + 4 <= jpeg.len() {
            let marker = u16::from_be_bytes([jpeg[i], jpeg[i + 1]]);
            let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
            let data = &jpeg[i + 4..i + 2 + len];
            if marker == 0xFFE1 && data.starts_with(b"Exif\0\0") {
                return &data[6..];
            }
            i += 2 + len;
        }
        panic!("no EXIF segment in test JPEG");
    }
}
```

(In the module tests, reference it as `crate::writers::exif_surgical_test_support::tiff_slice` — adjust the `use` path accordingly if the compiler prefers that form over the `super::super::` spelling.)

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib exif_surgical 2>&1 | tail -5`
Expected: 5 tests PASS. Debug offset math against the layout comment if an assertion fails — the builder's `resize` calls make offsets self-padding, so failures mean an entry's literal offset constant is wrong.

- [ ] **Step 3: Format, lint, commit**

```bash
cargo fmt && cargo clippy --lib 2>&1 | tail -3
git add src/writers/exif_surgical.rs src/writers/mod.rs
git commit -m "feat: add raw EXIF entry scanner for surgical writes

Walks IFD0/ExifIFD/GPS/InteropIFD/IFD1 of a TIFF byte range and
returns every entry with its raw value bytes (inline or offset-stored),
capturing the thumbnail blob and the MakerNote's original offset.
Corrupt sub-structures degrade gracefully.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Diff engine (`plan_exif_write`) and true-typed serialization of changed values

**Files:**
- Modify: `src/writers/exif_surgical.rs`

**Interfaces:**
- Consumes: `ExifScan`/`RawEntry`/`IfdKind` (Task 2), `MetadataMap`, `TagValue`, `lookup_tag_name` (`crate::tag_db`), `raw_bytes_to_tag_value` (`crate::core::tag_conversion`), `get_tag_descriptor`/`has_reliable_value_type` (`crate::tag_db::tag_registry`), `validate_tag_value_with_name`/`validate_tag_value_intrinsics` (`crate::core::validation`), `format_exif_datetime` (`crate::core::date_shift`).
- Produces (used by Task 4):

```rust
pub struct OutEntry { pub tag_id: u16, pub field_type: u16, pub count: u32, pub value: Vec<u8> }
pub struct WritePlan {
    pub byte_order: ByteOrder,
    pub ifd0: Vec<OutEntry>, pub exif_ifd: Vec<OutEntry>, pub gps: Vec<OutEntry>,
    pub interop: Vec<OutEntry>, pub ifd1: Vec<OutEntry>,
    pub thumbnail: Option<Vec<u8>>, pub makernote_pin: Option<usize>,
}
pub fn plan_exif_write(scan: &ExifScan, original_map: &MetadataMap, desired: &MetadataMap) -> Result<WritePlan>
fn tag_value_to_field(value: &TagValue, hint: Option<u16>) -> Result<(u16, u32, Vec<u8>)>  // private
```

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/writers/exif_surgical.rs` (uses the Task 2 builder):

```rust
    use crate::core::metadata_map::MetadataMap;
    use crate::core::tag_value::TagValue;

    /// Runs scan + reader-symmetric conversion to build the original map the
    /// way plan_exif_write's callers do in production.
    fn scan_and_maps(tiff: &[u8]) -> (ExifScan, MetadataMap) {
        let scan = scan_exif_entries(tiff).unwrap();
        let mut map = MetadataMap::new();
        for e in &scan.entries {
            if matches!(e.ifd, IfdKind::Interop | IfdKind::Ifd1) || e.tag_id == MAKERNOTE {
                continue;
            }
            let key = crate::tag_db::lookup_tag_name(e.tag_id, e.ifd.prefix());
            let value = crate::core::tag_conversion::raw_bytes_to_tag_value(
                &e.value,
                e.field_type,
                e.count,
                e.tag_id,
                scan.byte_order,
            );
            map.insert(key, value);
        }
        (scan, map)
    }

    #[test]
    fn plan_noop_carries_everything() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let desired = original.clone();
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        // Every surfaced entry carried with identical raw bytes
        let cc = plan.exif_ifd.iter().find(|e| e.tag_id == 0x9101).unwrap();
        assert_eq!(cc.field_type, 7);
        assert_eq!(cc.value, [1, 2, 3, 0]);
        let gps = plan.gps.iter().find(|e| e.tag_id == 0x0000).unwrap();
        assert_eq!(gps.value, [2, 3, 0, 0]);
        // Unsurfaced classes carried too
        assert!(plan.exif_ifd.iter().any(|e| e.tag_id == MAKERNOTE));
        assert!(plan.ifd1.iter().any(|e| e.tag_id == 0x0103));
        assert_eq!(plan.makernote_pin, Some(116));
        assert!(plan.thumbnail.is_some());
    }

    #[test]
    fn plan_removal_by_absence() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.remove("IFD0:Orientation");
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        assert!(!plan.ifd0.iter().any(|e| e.tag_id == 0x0112));
        assert!(plan.ifd0.iter().any(|e| e.tag_id == 0x010F)); // Make survives
    }

    #[test]
    fn plan_changed_value_is_revalidated_and_retyped() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("IFD0:Make", TagValue::new_string("Nikon"));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let make = plan.ifd0.iter().find(|e| e.tag_id == 0x010F).unwrap();
        assert_eq!(make.field_type, 2);
        assert_eq!(make.value, b"Nikon\0");
        assert_eq!(make.count, 6);
    }

    #[test]
    fn plan_rejects_display_string_write_to_binary_tag() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        // User "modifies" ComponentsConfiguration with a display string:
        // strict validation must reject, exactly as before this change
        desired.insert(
            "ExifIFD:ComponentsConfiguration",
            TagValue::new_string("R, G, B, -"),
        );
        let err = plan_exif_write(&scan, &original, &desired).unwrap_err();
        assert!(err.to_string().contains("Type mismatch"), "got: {}", err);
    }

    #[test]
    fn plan_added_tag_and_unknown_added_tag() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let mut desired = original.clone();
        desired.insert("IFD0:Artist", TagValue::new_string("A. Person"));
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        let artist = plan.ifd0.iter().find(|e| e.tag_id == 0x013B).unwrap();
        assert_eq!(artist.value, b"A. Person\0");

        let mut bad = original.clone();
        bad.insert("IFD0:NoSuchTagName", TagValue::new_string("x"));
        assert!(plan_exif_write(&scan, &original, &bad).is_err());
    }

    #[test]
    fn plan_clear_semantics() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let desired = MetadataMap::new();
        let plan = plan_exif_write(&scan, &original, &desired).unwrap();
        assert!(plan.ifd0.is_empty() && plan.exif_ifd.is_empty() && plan.gps.is_empty());
        assert!(plan.ifd1.is_empty() && plan.thumbnail.is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib exif_surgical 2>&1 | tail -5`
Expected: compile error (`plan_exif_write`, `OutEntry`, `WritePlan` not found).

- [ ] **Step 3: Implement**

Add to `src/writers/exif_surgical.rs`:

```rust
use crate::core::metadata_map::MetadataMap;
use crate::core::tag_conversion::raw_bytes_to_tag_value;
use crate::core::tag_value::TagValue;
use crate::core::validation::{validate_tag_value_intrinsics, validate_tag_value_with_name};
use crate::tag_db::lookup_tag_name;
use crate::tag_db::tag_registry::{get_tag_descriptor, has_reliable_value_type};

/// One entry ready for serialization (raw carry-over or freshly typed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutEntry {
    pub tag_id: u16,
    pub field_type: u16,
    pub count: u32,
    pub value: Vec<u8>,
}

/// A fully diffed EXIF write: per-IFD entries plus preserved blobs.
#[derive(Debug, Clone, PartialEq)]
pub struct WritePlan {
    pub byte_order: ByteOrder,
    pub ifd0: Vec<OutEntry>,
    pub exif_ifd: Vec<OutEntry>,
    pub gps: Vec<OutEntry>,
    pub interop: Vec<OutEntry>,
    pub ifd1: Vec<OutEntry>,
    pub thumbnail: Option<Vec<u8>>,
    /// Original MakerNote value offset to honor during layout
    pub makernote_pin: Option<usize>,
}

/// Serializes a caller-supplied TagValue into (field_type, count, bytes).
/// `hint` is the original entry's field type, used to keep BYTE vs UNDEFINED
/// and SHORT vs LONG stable across an edit.
fn tag_value_to_field(value: &TagValue, hint: Option<u16>) -> Result<(u16, u32, Vec<u8>)> {
    match value {
        TagValue::String(s) => {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            Ok((2, bytes.len() as u32, bytes))
        }
        TagValue::Integer(i) => {
            let i = *i;
            match hint {
                Some(3) if (0..=0xFFFF).contains(&i) => Ok((3, 1, (i as u16).to_ne_bytes().to_vec())),
                _ if (0..=0xFFFF).contains(&i) => Ok((3, 1, (i as u16).to_ne_bytes().to_vec())),
                _ if (0..=0xFFFF_FFFF).contains(&i) => Ok((4, 1, (i as u32).to_ne_bytes().to_vec())),
                _ if (i32::MIN as i64..=i32::MAX as i64).contains(&i) => {
                    Ok((9, 1, (i as i32).to_ne_bytes().to_vec()))
                }
                _ => Err(ExifToolError::parse_error(format!(
                    "Integer value {} does not fit any TIFF integer type",
                    i
                ))),
            }
        }
        TagValue::Rational {
            numerator,
            denominator,
        } => {
            if *numerator >= 0 && *denominator >= 0 {
                let mut b = (*numerator as u32).to_ne_bytes().to_vec();
                b.extend_from_slice(&(*denominator as u32).to_ne_bytes());
                Ok((5, 1, b))
            } else {
                let mut b = numerator.to_ne_bytes().to_vec();
                b.extend_from_slice(&denominator.to_ne_bytes());
                Ok((10, 1, b))
            }
        }
        TagValue::Binary(bytes) => {
            let ft = match hint {
                Some(1) => 1, // keep BYTE if it was BYTE
                _ => 7,       // UNDEFINED
            };
            Ok((ft, bytes.len() as u32, bytes.clone()))
        }
        TagValue::DateTime(dt) => {
            let mut bytes = crate::core::date_shift::format_exif_datetime(dt).into_bytes();
            bytes.push(0);
            Ok((2, bytes.len() as u32, bytes)) // always 20
        }
        TagValue::Float(f) => Ok((12, 1, f.to_ne_bytes().to_vec())),
        TagValue::Array(_) | TagValue::Struct(_) => Err(ExifToolError::parse_error(
            "Array/Struct values are not supported for EXIF write",
        )),
    }
}

/// NOTE on multi-byte native-endian buffers: tag_value_to_field intentionally
/// emits native-endian placeholder bytes for Integer/Rational/Float; the
/// serializer (Task 4) re-emits multi-byte numeric values in the plan's byte
/// order using field_type/count, so these placeholders never reach the file
/// for numeric types. ASCII/BYTE/UNDEFINED bytes are endian-neutral.
///
/// Diffs the original scan + reader-produced map against the desired map.
/// See the Design Rules table in the plan document for the exact contract.
pub fn plan_exif_write(
    scan: &ExifScan,
    original_map: &MetadataMap,
    desired: &MetadataMap,
) -> Result<WritePlan> {
    let exif_family_keys = |m: &MetadataMap| -> Vec<String> {
        m.iter()
            .map(|(k, _)| k.clone())
            .filter(|k| {
                k.starts_with("IFD0:")
                    || k.starts_with("ExifIFD:")
                    || k.starts_with("GPS:")
                    || k.starts_with("EXIF:")
            })
            .collect()
    };

    let mut plan = WritePlan {
        byte_order: scan.byte_order,
        ifd0: Vec::new(),
        exif_ifd: Vec::new(),
        gps: Vec::new(),
        interop: Vec::new(),
        ifd1: Vec::new(),
        thumbnail: None,
        makernote_pin: None,
    };

    // clear_all_metadata semantics: no EXIF-family keys desired -> drop all
    if exif_family_keys(desired).is_empty() {
        return Ok(plan);
    }

    plan.thumbnail = scan.thumbnail.clone();
    plan.makernote_pin = scan.makernote_offset;

    let mut consumed_keys: Vec<String> = Vec::new();

    for entry in &scan.entries {
        let bucket = |plan: &mut WritePlan, e: OutEntry| match entry.ifd {
            IfdKind::Ifd0 => plan.ifd0.push(e),
            IfdKind::ExifIfd => plan.exif_ifd.push(e),
            IfdKind::Gps => plan.gps.push(e),
            IfdKind::Interop => plan.interop.push(e),
            IfdKind::Ifd1 => plan.ifd1.push(e),
        };
        let carry = OutEntry {
            tag_id: entry.tag_id,
            field_type: entry.field_type,
            count: entry.count,
            value: entry.value.clone(),
        };

        // Unsurfaced classes: always carry
        if matches!(entry.ifd, IfdKind::Interop | IfdKind::Ifd1) || entry.tag_id == MAKERNOTE {
            bucket(&mut plan, carry);
            continue;
        }

        let key = lookup_tag_name(entry.tag_id, entry.ifd.prefix());
        let Some(original_value) = original_map.get(&key) else {
            // Reader didn't surface this entry: never drop what it hides
            bucket(&mut plan, carry);
            continue;
        };
        let Some(desired_value) = desired.get(&key) else {
            continue; // removal by absence
        };
        consumed_keys.push(key.clone());
        if desired_value == original_value {
            bucket(&mut plan, carry);
            continue;
        }

        // Changed: strict validation, then true-typed serialization
        validate_changed(&key, desired_value)?;
        let (ft, count, bytes) = tag_value_to_field(desired_value, Some(entry.field_type))?;
        bucket(
            &mut plan,
            OutEntry {
                tag_id: entry.tag_id,
                field_type: ft,
                count,
                value: bytes,
            },
        );
    }

    // Added: desired EXIF-family keys not matched to any original entry
    for key in exif_family_keys(desired) {
        if consumed_keys.iter().any(|k| *k == key) {
            continue;
        }
        let value = desired.get(&key).unwrap();
        let Some(descriptor) = get_tag_descriptor(&key) else {
            return Err(ExifToolError::parse_error(format!(
                "Cannot add tag '{}': not a known EXIF tag",
                key
            )));
        };
        validate_changed(&key, value)?;
        let tag_id = descriptor_tag_id(&descriptor).ok_or_else(|| {
            ExifToolError::parse_error(format!("Tag '{}' has no numeric EXIF id", key))
        })?;
        let (ft, count, bytes) = tag_value_to_field(value, None)?;
        let out = OutEntry {
            tag_id,
            field_type: ft,
            count,
            value: bytes,
        };
        // Route by prefix; "EXIF:" keys land in IFD0 (compat with the old writer)
        if key.starts_with("ExifIFD:") {
            plan.exif_ifd.push(out);
        } else if key.starts_with("GPS:") {
            plan.gps.push(out);
        } else {
            plan.ifd0.push(out);
        }
    }

    Ok(plan)
}

/// Strict validation for values the caller changed or added — identical
/// policy to write_metadata's PHASE 1 (reliable type match, else intrinsics).
fn validate_changed(key: &str, value: &TagValue) -> Result<()> {
    if let Some(descriptor) = get_tag_descriptor(key) {
        if has_reliable_value_type(key) {
            validate_tag_value_with_name(key, descriptor, value)?;
        } else {
            validate_tag_value_intrinsics(key, value)?;
        }
    }
    Ok(())
}
```

`descriptor_tag_id` extracts the numeric id from the descriptor — mirror how `validate_tag_for_tiff` (`src/writers/tiff_writer/tiff/validator.rs:48-69`) matches `TagId::Numeric(n)` on the descriptor's id field, and write:

```rust
fn descriptor_tag_id(descriptor: &crate::tag_db::tag_registry::TagDescriptor) -> Option<u16> {
    // Match the pattern used by validate_tag_for_tiff (validator.rs:48-69)
    // for extracting TagId::Numeric from the descriptor.
}
```

with the body copied from that existing pattern (the exact field/type names come from `validator.rs`; keep them identical). If `get_tag_descriptor` returns an owned vs borrowed type, follow the call sites in `operations.rs:195` for the correct binding.

Compilation note: `raw_bytes_to_tag_value`'s parameter types must match `src/core/tag_conversion.rs:40` exactly — if `count` there is `usize` or `u32` differs from this plan's assumption, adapt the test helper `scan_and_maps` accordingly (cast from the `RawEntry` fields), never the other way around.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib exif_surgical 2>&1 | tail -5`
Expected: all PASS (Task 2's 5 + Task 3's 6).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --lib 2>&1 | tail -3
git add src/writers/exif_surgical.rs
git commit -m "feat: add EXIF write diff engine with raw carry-over

plan_exif_write diffs the desired metadata map against the original
file's raw entries using the reader's own conversion for symmetry:
unchanged and unsurfaced entries carry raw bytes verbatim, removals
follow the map contract, and changed/added values pass the same strict
validation write_metadata enforced — validation is never relaxed.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Layout engine and serializer (`serialize_exif`)

**Files:**
- Modify: `src/writers/exif_surgical.rs`

**Interfaces:**
- Consumes: `WritePlan`/`OutEntry` (Task 3), `type_size` (Task 2).
- Produces (used by Task 5): `pub fn serialize_exif(plan: &WritePlan) -> Result<Vec<u8>>` — complete TIFF bytes (header + IFDs + values + blobs), original byte order, MakerNote at its pinned offset when honorable (else appended with an `eprintln!` warning), thumbnail re-linked via regenerated 0x0201/0x0202, IFD0→IFD1 next-pointer chain, entries sorted ascending per IFD, pointer entries synthesized only for non-empty sub-IFDs. Returns `Ok(empty vec)` marker semantics: an entirely empty plan (clear) serializes to an empty `Vec` — the caller drops the EXIF segment.

- [ ] **Step 1: Write the failing round-trip tests**

Add to `mod tests`:

```rust
    /// The strongest possible property: serialize then rescan must reproduce
    /// the plan exactly (entries, blobs, byte order).
    fn assert_roundtrip(plan: &WritePlan) {
        let bytes = serialize_exif(plan).unwrap();
        let rescan = scan_exif_entries(&bytes).unwrap();
        assert_eq!(rescan.byte_order, plan.byte_order);
        let mut expected: Vec<(IfdKind, &OutEntry)> = Vec::new();
        for (ifd, list) in [
            (IfdKind::Ifd0, &plan.ifd0),
            (IfdKind::ExifIfd, &plan.exif_ifd),
            (IfdKind::Gps, &plan.gps),
            (IfdKind::Interop, &plan.interop),
            (IfdKind::Ifd1, &plan.ifd1),
        ] {
            for e in list {
                expected.push((ifd, e));
            }
        }
        assert_eq!(rescan.entries.len(), expected.len());
        for (ifd, e) in expected {
            let got = rescan
                .entries
                .iter()
                .find(|r| r.ifd == ifd && r.tag_id == e.tag_id)
                .unwrap_or_else(|| panic!("missing {:?}:{:#06x}", ifd, e.tag_id));
            assert_eq!(got.field_type, e.field_type, "type for {:#06x}", e.tag_id);
            assert_eq!(got.count, e.count, "count for {:#06x}", e.tag_id);
            assert_eq!(got.value, e.value, "value for {:#06x}", e.tag_id);
        }
        assert_eq!(rescan.thumbnail, plan.thumbnail);
    }

    #[test]
    fn serialize_roundtrips_noop_plan_le() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let plan = plan_exif_write(&scan, &original, &original.clone()).unwrap();
        assert_roundtrip(&plan);
    }

    #[test]
    fn serialize_roundtrips_noop_plan_be() {
        let tiff = build_full_tiff(ByteOrder::BigEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let plan = plan_exif_write(&scan, &original, &original.clone()).unwrap();
        assert_roundtrip(&plan);
    }

    #[test]
    fn serialize_honors_makernote_pin() {
        let tiff = build_full_tiff(ByteOrder::LittleEndian);
        let (scan, original) = scan_and_maps(&tiff);
        let plan = plan_exif_write(&scan, &original, &original.clone()).unwrap();
        assert_eq!(plan.makernote_pin, Some(116));
        let bytes = serialize_exif(&plan).unwrap();
        // The makernote payload must sit at its original offset
        assert_eq!(&bytes[116..124], &[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn serialize_empty_plan_is_empty() {
        let plan = WritePlan {
            byte_order: ByteOrder::LittleEndian,
            ifd0: vec![],
            exif_ifd: vec![],
            gps: vec![],
            interop: vec![],
            ifd1: vec![],
            thumbnail: None,
            makernote_pin: None,
        };
        assert!(serialize_exif(&plan).unwrap().is_empty());
    }

    #[test]
    fn serialize_real_fixture_noop_roundtrip() {
        for fixture in [
            "/tests/fixtures/jpeg/complex/synthetic_gps_001.jpg",
            "/tests/fixtures/jpeg/makernotes/canon_sample.jpg",
        ] {
            let bytes =
                std::fs::read(format!("{}{}", env!("CARGO_MANIFEST_DIR"), fixture)).unwrap();
            let tiff = crate::writers::exif_surgical_test_support::tiff_slice(&bytes);
            let (scan, original) = scan_and_maps(tiff);
            let plan = plan_exif_write(&scan, &original, &original.clone()).unwrap();
            assert_roundtrip(&plan);
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib exif_surgical 2>&1 | tail -5`
Expected: compile error (`serialize_exif` not found).

- [ ] **Step 3: Implement**

Add to `src/writers/exif_surgical.rs`:

```rust
/// Emits v in the plan's byte order.
fn put_u16(out: &mut [u8], v: u16, bo: ByteOrder) {
    out.copy_from_slice(&match bo {
        ByteOrder::LittleEndian => v.to_le_bytes(),
        ByteOrder::BigEndian => v.to_be_bytes(),
    });
}
fn put_u32(out: &mut [u8], v: u32, bo: ByteOrder) {
    out.copy_from_slice(&match bo {
        ByteOrder::LittleEndian => v.to_le_bytes(),
        ByteOrder::BigEndian => v.to_be_bytes(),
    });
}

/// Re-encodes an OutEntry's value into the target byte order when the field
/// type is multi-byte numeric AND the value came from tag_value_to_field's
/// native-endian placeholder. Carried raw values are already in the file's
/// byte order (the plan preserves it), so this only converts per-element for
/// freshly serialized numeric values; it is a no-op for 1-byte element types.
fn value_in_byte_order(entry: &OutEntry, bo: ByteOrder) -> Vec<u8> {
    let elem = type_size(entry.field_type);
    if elem == 1 {
        return entry.value.clone();
    }
    // Elements inside RATIONAL/SRATIONAL are two 4-byte halves
    let unit = match entry.field_type {
        5 | 10 => 4,
        _ => elem,
    };
    let mut out = Vec::with_capacity(entry.value.len());
    for chunk in entry.value.chunks(unit) {
        let mut c = chunk.to_vec();
        let native_le = cfg!(target_endian = "little");
        let want_le = bo == ByteOrder::LittleEndian;
        if native_le != want_le {
            c.reverse();
        }
        out.extend_from_slice(&c);
    }
    out
}

/// Offset allocator that flows around one reserved window.
struct Allocator {
    cursor: usize,
    reserved: Option<(usize, usize)>, // (start, len)
}

impl Allocator {
    fn alloc(&mut self, len: usize) -> usize {
        // TIFF values should start on even offsets
        if self.cursor % 2 == 1 {
            self.cursor += 1;
        }
        if let Some((rs, rl)) = self.reserved
            && self.cursor < rs + rl
            && self.cursor + len > rs
        {
            self.cursor = rs + rl;
            if self.cursor % 2 == 1 {
                self.cursor += 1;
            }
        }
        let at = self.cursor;
        self.cursor += len;
        at
    }
}

/// Serializes a WritePlan into complete TIFF bytes. An empty plan yields an
/// empty Vec (the caller omits the EXIF segment entirely).
pub fn serialize_exif(plan: &WritePlan) -> Result<Vec<u8>> {
    let has_entries = !(plan.ifd0.is_empty()
        && plan.exif_ifd.is_empty()
        && plan.gps.is_empty()
        && plan.interop.is_empty()
        && plan.ifd1.is_empty());
    if !has_entries {
        return Ok(Vec::new());
    }
    let bo = plan.byte_order;

    // Sorted copies (TIFF requires ascending tag ids per IFD)
    let mut ifd0 = plan.ifd0.clone();
    let mut exif_ifd = plan.exif_ifd.clone();
    let mut gps = plan.gps.clone();
    let mut interop = plan.interop.clone();
    let mut ifd1 = plan.ifd1.clone();
    for list in [&mut ifd0, &mut exif_ifd, &mut gps, &mut interop, &mut ifd1] {
        list.sort_by_key(|e| e.tag_id);
        list.dedup_by_key(|e| e.tag_id); // defensive: one entry per tag id
    }

    // Pointer entries the tables will contain (synthesized during emit)
    let ifd0_pointers = usize::from(!exif_ifd.is_empty()) + usize::from(!gps.is_empty());
    let exif_pointers = usize::from(!interop.is_empty());
    let ifd1_pointers = if plan.thumbnail.is_some() { 2 } else { 0 };

    let table_size = |n: usize| 2 + n * 12 + 4;

    // Pass 1: allocate tables, then oversized values, honoring the pin
    let mut alloc = Allocator {
        cursor: 8,
        reserved: None,
    };
    let makernote_len = exif_ifd
        .iter()
        .find(|e| e.tag_id == MAKERNOTE)
        .map(|e| e.value.len())
        .filter(|len| *len > 4);
    let mut pinned = None;
    if let (Some(pin), Some(len)) = (plan.makernote_pin, makernote_len) {
        if pin >= 8 {
            alloc.reserved = Some((pin, len));
            pinned = Some(pin);
        } else {
            eprintln!(
                "Warning: MakerNote original offset {} cannot be honored; \
                 manufacturer-internal offsets may be invalidated",
                pin
            );
        }
    }

    let ifd0_at = alloc.alloc(table_size(ifd0.len() + ifd0_pointers));
    let exif_at = if exif_ifd.is_empty() {
        0
    } else {
        alloc.alloc(table_size(exif_ifd.len() + exif_pointers))
    };
    let interop_at = if interop.is_empty() {
        0
    } else {
        alloc.alloc(table_size(interop.len()))
    };
    let gps_at = if gps.is_empty() {
        0
    } else {
        alloc.alloc(table_size(gps.len()))
    };
    let ifd1_at = if ifd1.is_empty() && plan.thumbnail.is_none() {
        0
    } else {
        alloc.alloc(table_size(ifd1.len() + ifd1_pointers))
    };

    // Value offsets for every oversized value, deterministic order
    let mut value_offsets: Vec<Vec<usize>> = Vec::new();
    for list in [&ifd0, &exif_ifd, &interop, &gps, &ifd1] {
        let mut offsets = Vec::with_capacity(list.len());
        for e in list.iter() {
            if e.value.len() > 4 {
                if e.tag_id == MAKERNOTE && pinned.is_some() {
                    offsets.push(pinned.unwrap());
                } else {
                    offsets.push(alloc.alloc(e.value.len()));
                }
            } else {
                offsets.push(0); // inline
            }
        }
        value_offsets.push(offsets);
    }
    let thumb_at = plan.thumbnail.as_ref().map(|t| alloc.alloc(t.len()));

    let total = alloc
        .cursor
        .max(pinned.map_or(0, |p| p + makernote_len.unwrap_or(0)));
    let mut out = vec![0u8; total];

    // Header
    out[0..2].copy_from_slice(match bo {
        ByteOrder::LittleEndian => b"II",
        ByteOrder::BigEndian => b"MM",
    });
    put_u16(&mut out[2..4], 42, bo);
    put_u32(&mut out[4..8], ifd0_at as u32, bo);

    // Emit one IFD table: entries (sorted, with synthesized pointers merged
    // in tag-id order), then next-IFD pointer, then oversized values.
    let mut emit_ifd = |out: &mut Vec<u8>,
                        table_at: usize,
                        entries: &[OutEntry],
                        offsets: &[usize],
                        pointers: &[(u16, u32)],
                        next_ifd: u32| {
        let mut rows: Vec<(u16, u16, u32, [u8; 4])> = Vec::new(); // tag, type, count, valfield
        for (e, off) in entries.iter().zip(offsets) {
            let mut val = [0u8; 4];
            if e.value.len() > 4 {
                put_u32(&mut val, *off as u32, bo);
                let bytes = value_in_byte_order(e, bo);
                out[*off..*off + bytes.len()].copy_from_slice(&bytes);
            } else {
                let bytes = value_in_byte_order(e, bo);
                val[..bytes.len()].copy_from_slice(&bytes);
            }
            rows.push((e.tag_id, e.field_type, e.count, val));
        }
        for (tag, target) in pointers {
            let mut val = [0u8; 4];
            put_u32(&mut val, *target, bo);
            rows.push((*tag, 4, 1, val)); // LONG count 1
        }
        rows.sort_by_key(|r| r.0);
        put_u16(&mut out[table_at..table_at + 2], rows.len() as u16, bo);
        for (i, (tag, ft, count, val)) in rows.iter().enumerate() {
            let at = table_at + 2 + i * 12;
            put_u16(&mut out[at..at + 2], *tag, bo);
            put_u16(&mut out[at + 2..at + 4], *ft, bo);
            put_u32(&mut out[at + 4..at + 8], *count, bo);
            out[at + 8..at + 12].copy_from_slice(val);
        }
        let next_at = table_at + 2 + rows.len() * 12;
        put_u32(&mut out[next_at..next_at + 4], next_ifd, bo);
    };

    // ExifIFD (with Interop pointer), Interop, GPS, IFD1, then IFD0 last so
    // its pointer values are all known
    if exif_at != 0 {
        let mut ptrs = Vec::new();
        if interop_at != 0 {
            ptrs.push((INTEROP_POINTER, interop_at as u32));
        }
        emit_ifd(&mut out, exif_at, &exif_ifd, &value_offsets[1], &ptrs, 0);
    }
    if interop_at != 0 {
        emit_ifd(&mut out, interop_at, &interop, &value_offsets[2], &[], 0);
    }
    if gps_at != 0 {
        emit_ifd(&mut out, gps_at, &gps, &value_offsets[3], &[], 0);
    }
    if ifd1_at != 0 {
        let mut ptrs = Vec::new();
        if let Some(t_at) = thumb_at {
            ptrs.push((THUMBNAIL_OFFSET, t_at as u32));
            ptrs.push((
                THUMBNAIL_LENGTH,
                plan.thumbnail.as_ref().unwrap().len() as u32,
            ));
        }
        emit_ifd(&mut out, ifd1_at, &ifd1, &value_offsets[4], &ptrs, 0);
    }
    {
        let mut ptrs = Vec::new();
        if exif_at != 0 {
            ptrs.push((EXIF_IFD_POINTER, exif_at as u32));
        }
        if gps_at != 0 {
            ptrs.push((GPS_IFD_POINTER, gps_at as u32));
        }
        emit_ifd(
            &mut out,
            ifd0_at,
            &ifd0,
            &value_offsets[0],
            &ptrs,
            ifd1_at as u32,
        );
    }
    if let (Some(t_at), Some(thumb)) = (thumb_at, plan.thumbnail.as_ref()) {
        out[t_at..t_at + thumb.len()].copy_from_slice(thumb);
    }

    Ok(out)
}
```

Compilation notes for the implementer:
- The `emit_ifd` closure mutably borrows `out` via its parameter, not capture — pass `&mut out` explicitly as written; if the borrow checker objects to the closure form, convert `emit_ifd` to a standalone `fn emit_ifd(out: &mut Vec<u8>, bo: ByteOrder, ...)`.
- `THUMBNAIL_LENGTH`'s pointer row is a plain LONG value (not an offset) — the `(tag, target)` tuple carries the length value in that case; the shared code path writes it identically.
- One subtlety the round-trip test WILL catch if wrong: `value_in_byte_order` must be applied consistently for inline (≤4 byte) numeric values too — the code above does this in both branches.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib exif_surgical 2>&1 | tail -5`
Expected: all PASS, including both real-fixture no-op round-trips (the strongest gate in the whole plan: scan→plan→serialize→rescan identity on canon_sample with live MakerNotes).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --lib 2>&1 | tail -3
git add src/writers/exif_surgical.rs
git commit -m "feat: add surgical EXIF serializer with offset-stable MakerNotes

Serializes a WritePlan into complete TIFF bytes preserving the original
byte order, carrying raw values verbatim, keeping the MakerNote blob at
its original offset (so manufacturer-internal absolute offsets stay
valid), and re-linking the IFD1 thumbnail. Verified by scan->plan->
serialize->rescan identity on real GPS and Canon MakerNote fixtures.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Orchestration (`rewrite_jpeg_exif`) and wiring into `write_metadata`

**Files:**
- Modify: `src/writers/exif_surgical.rs` (add `rewrite_jpeg_exif`)
- Modify: `src/writers/jpeg_writer.rs` (`write_exif_to_jpeg` uses the surgical path; segment omitted when serializer returns empty)
- Modify: `src/core/operations.rs` (`write_metadata`: JPEG branch skips whole-map PHASE-1 validation — the surgical path validates changed/added itself; non-JPEG branches keep PHASE 1 exactly as-is)

**Interfaces:**
- Produces: `pub fn rewrite_jpeg_exif(file_bytes: &[u8], desired: &MetadataMap) -> Result<Vec<u8>>` — returns the new EXIF **segment data** (`"Exif\0\0"` + TIFF bytes), or an empty Vec when the EXIF segment should be dropped.
- `write_exif_to_jpeg(reader, metadata)` keeps its signature (callers unchanged) but now: reads the full file bytes once, calls `rewrite_jpeg_exif`, and reconstructs the JPEG replacing/inserting/omitting the EXIF APP1 segment accordingly. The `DummyReader`/`reconstruct_tiff_structure`/`build_exif_segment` path is deleted from the JPEG writer (the tiff_writer module itself stays — it has other consumers/tests).

- [ ] **Step 1: Write the failing tests**

Create `tests/exif_roundtrip.rs`:

```rust
//! Round-trip regression suite for issue #20: read -> write must never
//! corrupt or silently drop EXIF data.

use oxidex::core::operations::{modify_tag, read_metadata, remove_tag, write_metadata};
use oxidex::core::tag_value::TagValue;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg")
        .join(name)
}

fn temp_copy(src: &Path, label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join(label);
    std::fs::copy(src, &dst).unwrap();
    (dir, dst)
}

/// Semantic parity: every key in `before` must exist in `after` with an
/// equal value (except keys in `except`). File:* pseudo-tags are ignored.
fn assert_parity(before: &oxidex::core::MetadataMap, after: &oxidex::core::MetadataMap, except: &[&str]) {
    for (key, value) in before.iter() {
        if key.starts_with("File:") || except.contains(&key.as_str()) {
            continue;
        }
        assert_eq!(
            after.get(key),
            Some(value),
            "tag {} was dropped or changed by the rewrite",
            key
        );
    }
}

#[test]
fn noop_write_preserves_everything_gps_fixture() {
    let (_d, path) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "noop_gps.jpg");
    let before = read_metadata(&path).unwrap();
    write_metadata(&path, &before).unwrap(); // was: hard validation failure
    let after = read_metadata(&path).unwrap();
    assert_parity(&before, &after, &[]);
    assert_parity(&after, &before, &[]); // and nothing appeared from nowhere
}

#[test]
fn noop_write_preserves_makernotes() {
    let (_d, path) = temp_copy(&fixture("makernotes/canon_sample.jpg"), "noop_canon.jpg");
    let before = read_metadata(&path).unwrap();
    let canon_before: Vec<(String, TagValue)> = before
        .iter()
        .filter(|(k, _)| k.starts_with("Canon:"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert!(!canon_before.is_empty(), "fixture must have Canon MakerNote tags");
    write_metadata(&path, &before).unwrap();
    let after = read_metadata(&path).unwrap();
    for (key, value) in &canon_before {
        assert_eq!(after.get(key), Some(value), "MakerNote tag {} lost", key);
    }
}

#[test]
fn modify_tag_leaves_binary_canaries_byte_identical() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let (_d, path) = temp_copy(&src, "modify.jpg");
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("Round Tripper")).unwrap();

    let after = read_metadata(&path).unwrap();
    assert_eq!(
        after.get("IFD0:Artist").and_then(|v| v.as_string()),
        Some("Round Tripper")
    );
    // The canaries that used to be corrupted/rejected
    let before = read_metadata(&src).unwrap();
    for canary in ["ExifIFD:ComponentsConfiguration", "GPS:GPSVersionID"] {
        assert_eq!(before.get(canary), after.get(canary), "{} damaged", canary);
    }
}

#[test]
fn remove_tag_removes_only_that_tag() {
    let (_d, path) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "remove.jpg");
    let before = read_metadata(&path).unwrap();
    assert!(before.get("ExifIFD:DateTimeOriginal").is_some());
    remove_tag(&path, "ExifIFD:DateTimeOriginal").unwrap();
    let after = read_metadata(&path).unwrap();
    assert!(after.get("ExifIFD:DateTimeOriginal").is_none());
    assert_parity(&before, &after, &["ExifIFD:DateTimeOriginal"]);
}

#[test]
fn changed_binary_display_string_still_rejected() {
    let (_d, path) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "reject.jpg");
    let mut map = read_metadata(&path).unwrap();
    map.insert(
        "ExifIFD:ComponentsConfiguration",
        TagValue::new_string("R, G, B, -"),
    );
    let err = write_metadata(&path, &map).unwrap_err();
    assert!(err.to_string().contains("Type mismatch"), "got: {}", err);
    // And the file was not touched
    let orig = std::fs::read(fixture("complex/synthetic_gps_001.jpg")).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), orig);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test exif_roundtrip 2>&1 | tail -8`
Expected: `noop_write_*` and `modify_tag_*` and `remove_tag_*` FAIL with the PHASE-1 validation error (`expected Binary but got String`) — the pre-fix behavior. `changed_binary_display_string_still_rejected` may pass already.

- [ ] **Step 3: Implement**

1. Add to `src/writers/exif_surgical.rs`:

```rust
use crate::core::FileReader;
use crate::parsers::jpeg::parse_segments;

/// EXIF identifier at the start of an EXIF APP1 segment
const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";

/// A FileReader over an in-memory byte slice (same shape as exif_inplace's).
struct SliceReader<'a>(&'a [u8]);

impl FileReader for SliceReader<'_> {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.checked_add(length).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "read overflow")
        })?;
        if end > self.0.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read beyond end of buffer",
            ));
        }
        Ok(&self.0[start..end])
    }
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
}

/// Builds the new EXIF APP1 segment data ("Exif\0\0" + TIFF) for a JPEG,
/// preserving everything the caller did not change. Returns an empty Vec
/// when the EXIF segment should be dropped entirely.
pub fn rewrite_jpeg_exif(file_bytes: &[u8], desired: &MetadataMap) -> Result<Vec<u8>> {
    // Locate the original EXIF TIFF slice, if any
    let tiff: Option<Vec<u8>> = {
        let reader = SliceReader(file_bytes);
        let segments = parse_segments(&reader)?;
        segments
            .iter()
            .find(|s| s.is_app1() && s.data.starts_with(EXIF_IDENTIFIER))
            .map(|s| s.data[EXIF_IDENTIFIER.len()..].to_vec())
    };

    let (scan, original_map) = match &tiff {
        Some(tiff_bytes) => {
            let scan = scan_exif_entries(tiff_bytes)?;
            // The exact reader the diff must mirror: parse the whole JPEG the
            // same way read_metadata does (includes tag-name normalization)
            let reader = SliceReader(file_bytes);
            let original_map = crate::core::operations::parse_jpeg_metadata(&reader)?;
            (scan, original_map)
        }
        None => (
            ExifScan {
                byte_order: ByteOrder::LittleEndian,
                entries: Vec::new(),
                thumbnail: None,
                makernote_offset: None,
            },
            MetadataMap::new(),
        ),
    };

    let plan = plan_exif_write(&scan, &original_map, desired)?;
    let tiff_out = serialize_exif(&plan)?;
    if tiff_out.is_empty() {
        return Ok(Vec::new());
    }
    let mut segment = Vec::with_capacity(EXIF_IDENTIFIER.len() + tiff_out.len());
    segment.extend_from_slice(EXIF_IDENTIFIER);
    segment.extend_from_slice(&tiff_out);
    Ok(segment)
}
```

2. In `src/writers/jpeg_writer.rs`, replace the body of `write_exif_to_jpeg`'s Step 2 (`build_exif_segment(metadata)?`) with:

```rust
    // Step 2: Build new EXIF APP1 segment surgically (raw carry-over,
    // original byte order, MakerNotes preserved) — issue #20
    let file_size = reader.size() as usize;
    let file_bytes = reader.read(0, file_size)?;
    let new_exif_segment = crate::writers::exif_surgical::rewrite_jpeg_exif(file_bytes, metadata)?;
```

and extend `reconstruct_jpeg` to handle segment **omission**: when `new_exif_segment.is_empty()`, the existing EXIF segment (if any) is dropped and no new one is inserted. Concretely, in `reconstruct_jpeg`, wrap both `write_segment(&mut output, APP1_MARKER, &new_exif_data)?` calls and the trailing insertion in `if !new_exif_data.is_empty() { ... }` and set `exif_written = true` unconditionally next to each (so the no-EXIF case doesn't trigger the fallback insertion). Delete `build_exif_segment` and the `DummyReader` struct (now unused); remove the now-unused `reconstruct_tiff_structure` import. Keep everything else (head/tail split, XMP/other segment passthrough) untouched.

3. In `src/core/operations.rs::write_metadata`, restructure PHASE 1 + dispatch:

```rust
    // PHASE 1: VALIDATION
    // JPEG is validated inside the surgical writer, which distinguishes
    // caller-changed values (strict validation) from unchanged originals
    // (raw carry-over that never re-enters through TagValue). Whole-map
    // validation here would wrongly reject unchanged display-form tags
    // (issue #20). Other formats keep the original whole-map validation.
    let reader = MMapReader::new(path)?;
    let format = detect_format(&reader)?;

    if format != FileFormat::JPEG {
        for (tag_name, tag_value) in metadata.iter() {
            if let Some(descriptor) = get_tag_descriptor(tag_name) {
                if has_reliable_value_type(tag_name) {
                    validate_tag_value_with_name(tag_name, descriptor, tag_value)?;
                } else {
                    validate_tag_value_intrinsics(tag_name, tag_value)?;
                }
            }
        }
    }

    match format {
        // ... existing arms unchanged (JPEG arm still calls write_exif_to_jpeg)
    }
```

(The existing `MMapReader::new` + `detect_format` further down are subsumed by this — remove the duplicates so the file is opened once.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test exif_roundtrip 2>&1 | tail -8` — all 6 PASS.
Then the full existing surface: `cargo test --test integration 2>&1 | tail -3`, `cargo test --test date_shift_inplace 2>&1 | tail -3`, and `cargo test --workspace 2>&1 | grep -cE "FAILED|failures: [1-9]"` (expect `0`).
Existing `tests/integration/jpeg_write_tests.rs` exercises `write_exif_to_jpeg` with synthetic maps on minimal JPEGs — those now route through the added-tags path; if a test asserts on `build_exif_segment` internals directly, update it to call `rewrite_jpeg_exif` semantics (assert the segment parses and contains the tag) rather than deleting coverage.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --workspace 2>&1 | tail -3
git add src/writers/exif_surgical.rs src/writers/jpeg_writer.rs src/core/operations.rs tests/exif_roundtrip.rs
git commit -m "fix: preserve raw EXIF values through JPEG metadata writes

write_metadata's JPEG path now diffs the desired map against the
original file's raw entries: unchanged tags (including binary tags,
MakerNotes, InteropIFD, IFD1/thumbnail, and unknown tags) are carried
byte-for-byte, removals follow the map contract, and only caller-
changed/added values pass strict validation — which is never relaxed.
Fixes the round-trip failure and the silent-drop classes of issue #20.

Fixes swack-tools/oxidex#20

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: End-to-end API and CLI regression coverage

**Files:**
- Modify: `tests/exif_roundtrip.rs`

**Interfaces:**
- Consumes: the `oxidex` binary via `env!("CARGO_BIN_EXE_oxidex")`; `copy_metadata`, `clear_all_metadata` from `oxidex::core::operations`.

- [ ] **Step 1: Write the tests** (expected to pass if Task 5 is correct; failures indicate CLI/API wiring gaps)

Add to `tests/exif_roundtrip.rs`:

```rust
use oxidex::core::operations::{clear_all_metadata, copy_metadata};
use std::process::Command;

#[test]
fn copy_metadata_between_real_files() {
    let (_d1, src) = temp_copy(&fixture("makernotes/canon_sample.jpg"), "copy_src.jpg");
    let (_d2, dst) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "copy_dst.jpg");
    // was: hard validation failure on the merged map
    copy_metadata(&src, &dst, Some(&["IFD0:Make".to_string()])).unwrap();
    let after = read_metadata(&dst).unwrap();
    assert_eq!(after.get("IFD0:Make").and_then(|v| v.as_string()), Some("Canon"));
    // Destination's own binary canaries untouched
    let before = read_metadata(&fixture("complex/synthetic_gps_001.jpg")).unwrap();
    assert_eq!(
        before.get("ExifIFD:ComponentsConfiguration"),
        after.get("ExifIFD:ComponentsConfiguration")
    );
}

#[test]
fn clear_all_metadata_drops_exif_entirely() {
    let (_d, path) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "clear.jpg");
    clear_all_metadata(&path).unwrap();
    let after = read_metadata(&path).unwrap();
    assert!(after.get("ExifIFD:ComponentsConfiguration").is_none());
    assert!(after.get("GPS:GPSVersionID").is_none());
    assert!(after.get("ExifIFD:DateTimeOriginal").is_none());
}

#[test]
fn cli_tag_write_on_real_gps_jpeg() {
    let (_d, path) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "cli_write.jpg");
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-IFD0:Artist=CLI Writer")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let after = read_metadata(&path).unwrap();
    assert_eq!(
        after.get("IFD0:Artist").and_then(|v| v.as_string()),
        Some("CLI Writer")
    );
    assert!(after.get("GPS:GPSVersionID").is_some());
}
```

Note: if the CLI routes `-IFD0:Artist=...` through a different code path than `modify_tag` (check `src/main.rs` / `src/cli/`), keep the test but adjust the argument spelling to whatever the CLI's tag-write syntax is (`grep -rn "modify_tag" src/main.rs src/cli/`); the assertion contract stays identical. If the CLI has no tag-write command at all, replace `cli_tag_write_on_real_gps_jpeg` with an API-level `modify_tag` test on `canon_sample.jpg` asserting Canon MakerNote parity after the edit.

- [ ] **Step 2: Run tests + full sweep**

```bash
cargo test --test exif_roundtrip 2>&1 | tail -5
cargo test --workspace 2>&1 | tail -5
cargo clippy --workspace 2>&1 | tail -3
cargo fmt --all -- --check 2>/dev/null | head -3
```
Expected: all pass, no new clippy warnings, no fmt diffs.

Then the manual cross-check against reference exiftool (record output in the report):

```bash
cargo build
cp tests/fixtures/jpeg/makernotes/canon_sample.jpg /tmp/rt_check.jpg
./target/debug/oxidex "-IFD0:Artist=Manual Check" /tmp/rt_check.jpg 2>/dev/null || true
exiftool -Artist -Make -DateTimeOriginal -ComponentsConfiguration -CanonModelID /tmp/rt_check.jpg
exiftool -validate -warning /tmp/rt_check.jpg
```
Expected: Artist set (if the CLI supports tag writes; otherwise run the equivalent through a scratch Rust test), all other values identical to the original fixture's, and `exiftool -validate` reporting no structural errors.

- [ ] **Step 3: Commit**

```bash
git add tests/exif_roundtrip.rs
git commit -m "test: cover copy/clear/CLI flows for EXIF round-trip preservation

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Out of Scope (deliberate)

- **TIFF-file writes** stay blocked (image-data preservation is a separate project; the surgical machinery here is the foundation for it).
- **XMP writes on JPEG** (`modify_tag("XMP:...")` still no-ops into the ignored-prefix bucket, as today).
- **Strip-based IFD1 thumbnails** (0x0111/0x0117): entries are carried raw; if such a fixture ever appears, offsets dangle — the JPEGInterchangeFormat form (what cameras write) is fully handled.
- **Serializing `TagValue::Array`/`Struct` for caller-supplied new values** — clean error; carried originals are unaffected (raw bytes).
- **Writing decoded MakerNote sub-tags** (`Canon:*` keys are read-only; the blob is preserved verbatim at its original offset).
