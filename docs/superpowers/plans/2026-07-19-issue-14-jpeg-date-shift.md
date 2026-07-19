# JPEG EXIF Date Shifting (Issue #14) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `oxidex "-DateTimeOriginal-=1:00:00" test.jpg` work exactly like ExifTool, without corrupting any other metadata byte in the file.

**Architecture:** Three stacked bugs block the feature: (1) the shift-string parser rejects ExifTool's flexible grammar, (2) tag lookup only matches internal group-prefixed map keys, (3) the write path rebuilds the whole EXIF segment from display-converted values, which either fails validation (`ComponentsConfiguration`/`GPSVersionID`: "expected Binary but got String") or — if validation is relaxed — silently writes display-string ASCII into binary tags. The fix for (3) is to **never rewrite the EXIF segment for a date shift**: EXIF date/time values are fixed-length 20-byte ASCII (`"YYYY:MM:DD HH:MM:SS\0"`), so a shift can be patched in place at the tag's value offset. Only those 19 characters change; every other byte of the file is preserved verbatim. Non-JPEG formats (PNG, PDF) keep the existing map-based path with improved tag matching.

**Tech Stack:** Rust (existing workspace), `chrono` (already a dependency), existing helpers: `parse_segments` (JPEG), `read_u16`/`read_u32` (`ByteOrder`-aware), `write_atomic`.

## Global Constraints

- No new external dependencies.
- Reference behavior is ExifTool 13.55, pinned empirically (see grammar table in Task 1).
- `cargo clippy` must stay clean and `cargo fmt` applied before every commit (project CLAUDE.md).
- Do NOT touch `src/core/validation.rs` — the strict validator is what prevents silent corruption on other write paths. (A prior attempt, commit `2433c79` on `fix/wiring`, relaxed it and caused byte-level corruption of `ComponentsConfiguration`; that approach is rejected.)
- Every commit message ends with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Working directory: the `oxidex-issue-14-fa741f` worktree (branch `claude/oxidex-issue-14-fa741f`).

## File Structure

| File | Responsibility |
|---|---|
| `src/core/date_shift.rs` (modify) | Shift grammar (`parse_offset`), `ShiftSpec`, `ExifDateTag` + pattern resolution, orchestration (`shift_metadata_dates`) |
| `src/writers/exif_inplace.rs` (create) | Locate EXIF datetime values in a TIFF byte range; patch a JPEG's EXIF dates in place |
| `src/writers/mod.rs` (modify) | Register the new module |
| `tests/date_shift_inplace.rs` (create) | Integration tests: byte-preservation, issue-#14 CLI repro |

Existing key facts the implementer needs (verified in this codebase):

- Metadata map keys are group-prefixed: `IFD0:ModifyDate` (0x0132), `ExifIFD:DateTimeOriginal` (0x9003), `ExifIFD:DateTimeDigitized` (0x9004; ExifTool calls it `CreateDate`).
- `Segment` (`src/parsers/jpeg/segment_parser.rs`): `pub marker: u16`, `pub offset: u64` (file offset of the marker), `pub data: &[u8]` (payload after the 2-byte length field; for EXIF APP1 it starts with `"Exif\0\0"`). So the TIFF structure starts at file offset `segment.offset + 4 + 6`.
- `read_u16(bytes, byte_order) -> u16` and `read_u32(bytes, byte_order) -> u32` live in `src/core/operations_helpers.rs` (pub). `ByteOrder` is `crate::parsers::tiff::ifd_parser::ByteOrder`.
- `write_atomic(path: &Path, data: &[u8]) -> Result<()>` in `src/writers/atomic_writer.rs`.
- `ExifToolError` implements `From<std::io::Error>`, so `?` works on `std::fs::read`.
- Fixtures: `tests/fixtures/jpeg/complex/synthetic_gps_001.jpg` has `ExifIFD:DateTimeOriginal = 2024-02-01T14:30:00`, plus `ExifIFD:ComponentsConfiguration` and `GPS:GPSVersionID` (the corruption canaries). `tests/fixtures/jpeg/sample_with_exif.jpg` has `IFD0:ModifyDate = 2025-01-15T10:30:00` and no DateTimeOriginal.
- The CLI (`src/cli/args.rs::parse_date_shift`) already passes any `-Tag+=…`/`-Tag-=…` through as a shift op with the tag pattern verbatim; no CLI changes are needed.
- `src/main.rs::handle_date_shift_operation` already prints `    1 image files updated` on success and `Error: Failed to shift dates for '<tag>': <err>` on failure; no changes needed there.

---

### Task 1: ExifTool shift grammar in `parse_offset`

**Files:**
- Modify: `src/core/date_shift.rs` (replace `parse_offset` at lines 68–144, update its callers and unit tests)

**Interfaces:**
- Consumes: existing `DateOffset` struct (fields `years: u32, months: u32, days: i64, hours: i64, minutes: i64, seconds: i64`), `ExifToolError::parse_error`.
- Produces: `pub fn parse_offset(s: &str) -> Result<(DateOffset, bool)>` — the `bool` is `true` when the string had a leading `-` (the caller flips Add↔Subtract). This changed signature is what Tasks 2 and 5 build on.

Grammar, pinned against ExifTool 13.55 (base value `2025:06:10 12:00:00`, operator `-=`):

| Shift string | Meaning | Result |
|---|---|---|
| `1` | 1 hour | `11:00:00` |
| `1:30` | 1 h 30 m (time is **left**-justified) | `10:30:00` |
| `0:0:30` | 30 s | `11:59:30` |
| `1:0:0 0:0:0` | 1 year (two args: `DATE TIME`) | `2024:06:10 12:00:00` |
| `1:2 3` | 1 mo 2 d (date is **right**-justified) + 3 h | `2025:05:08 09:00:00` |
| `1:2:3:4` | invalid (max 3 numbers per part) | error |

- [ ] **Step 1: Write the failing tests**

In `src/core/date_shift.rs`, replace the six existing `test_parse_offset_*` tests inside `mod tests` with:

```rust
    #[test]
    fn test_parse_offset_full_form() {
        let (offset, neg) = parse_offset("1:2:3 4:5:6").unwrap();
        assert!(!neg);
        assert_eq!(offset, DateOffset::new(1, 2, 3, 4, 5, 6));
    }

    #[test]
    fn test_parse_offset_single_number_is_hours() {
        let (offset, neg) = parse_offset("1").unwrap();
        assert!(!neg);
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 0, 0));
    }

    #[test]
    fn test_parse_offset_time_is_left_justified() {
        // ExifTool: '1:30' means 1 hour 30 minutes, NOT 1 minute 30 seconds
        let (offset, _) = parse_offset("1:30").unwrap();
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 30, 0));
    }

    #[test]
    fn test_parse_offset_three_part_time() {
        let (offset, _) = parse_offset("0:0:30").unwrap();
        assert_eq!(offset, DateOffset::new(0, 0, 0, 0, 0, 30));
    }

    #[test]
    fn test_parse_offset_issue_14_form() {
        // The exact string from GitHub issue #14
        let (offset, neg) = parse_offset("1:00:00").unwrap();
        assert!(!neg);
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 0, 0));
    }

    #[test]
    fn test_parse_offset_date_is_right_justified() {
        // ExifTool: date part '1:2' means 1 month 2 days, NOT 1 year 2 months
        let (offset, _) = parse_offset("1:2 3").unwrap();
        assert_eq!(offset, DateOffset::new(0, 1, 2, 3, 0, 0));
    }

    #[test]
    fn test_parse_offset_two_arg_full_date() {
        let (offset, _) = parse_offset("1:0:0 0:0:0").unwrap();
        assert_eq!(offset, DateOffset::new(1, 0, 0, 0, 0, 0));
    }

    #[test]
    fn test_parse_offset_leading_minus_sets_negated() {
        let (offset, neg) = parse_offset("-1").unwrap();
        assert!(neg);
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 0, 0));
    }

    #[test]
    fn test_parse_offset_leading_plus_ignored() {
        let (offset, neg) = parse_offset("+1:30").unwrap();
        assert!(!neg);
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 30, 0));
    }

    #[test]
    fn test_parse_offset_invalid() {
        assert!(parse_offset("").is_err());
        assert!(parse_offset("1:2:3:4").is_err()); // too many numbers in one part
        assert!(parse_offset("1:2:3:4:5:6").is_err());
        assert!(parse_offset("1:2:3 4:5:6 7").is_err()); // three parts
        assert!(parse_offset("abc").is_err());
        assert!(parse_offset("1:2:3 4:x").is_err());
        assert!(parse_offset("1:").is_err()); // empty component
    }
```

Also update `test_parse_offset_zero` and `test_parse_offset_one_day` if present: delete them (covered above). Keep all `test_apply_shift_*` and datetime tests unchanged.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib date_shift 2>&1 | tail -20`
Expected: compile error (`parse_offset` returns `Result<DateOffset>`, tests destructure a tuple). A compile failure is the failing state here.

- [ ] **Step 3: Implement the new parser**

Replace the whole `parse_offset` function (including its doc comment, currently lines 68–144) with:

```rust
/// Parses an ExifTool-style shift string.
///
/// Grammar (matches `Image::ExifTool::Shift.pl`, verified against ExifTool 13.55):
/// - Optional leading `+` or `-`; `-` negates the whole shift (the returned
///   bool is `true`), which callers apply by flipping Add and Subtract.
/// - One or two space-separated parts, each 1-3 colon-separated non-negative
///   integers.
/// - Two parts are `DATE TIME`. DATE is right-justified: `D`, `M:D`, or
///   `Y:M:D`. TIME is left-justified: `H`, `H:M`, or `H:M:S`.
/// - A single part is a TIME shift (`H`, `H:M`, or `H:M:S`) because every tag
///   this module shifts is a full date-time value.
///
/// # Examples
///
/// - `"1:00:00"` -> 1 hour
/// - `"1:30"` -> 1 hour 30 minutes
/// - `"0:0:1 0:0:0"` -> 1 day
/// - `"1:2 3"` -> 1 month, 2 days, 3 hours
/// - `"-1"` -> 1 hour, negated
pub fn parse_offset(s: &str) -> Result<(DateOffset, bool)> {
    let invalid = || {
        ExifToolError::parse_error(format!(
            "Invalid shift string '{}': expected 'TIME' or 'DATE TIME' with 1-3 \
             numbers per part (e.g., '1:30' for 1.5 hours, '0:0:1 12' for 1 day 12 hours)",
            s
        ))
    };

    let trimmed = s.trim();
    let (negated, rest) = match trimmed.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };

    let parts: Vec<&str> = rest.split_whitespace().collect();
    let (date_part, time_part) = match parts.as_slice() {
        [time] => (None, *time),
        [date, time] => (Some(*date), *time),
        _ => return Err(invalid()),
    };

    let parse_components = |part: &str| -> Result<Vec<u32>> {
        let fields: Vec<&str> = part.split(':').collect();
        if fields.is_empty() || fields.len() > 3 {
            return Err(invalid());
        }
        fields
            .iter()
            .map(|f| f.parse::<u32>().map_err(|_| invalid()))
            .collect()
    };

    let mut offset = DateOffset::zero();
    if let Some(date) = date_part {
        // Right-justified: the last number is always days
        let mut values = parse_components(date)?.into_iter().rev();
        offset.days = values.next().unwrap_or(0) as i64;
        offset.months = values.next().unwrap_or(0);
        offset.years = values.next().unwrap_or(0);
    }
    // Left-justified: the first number is always hours
    let mut values = parse_components(time_part)?.into_iter();
    offset.hours = values.next().unwrap_or(0) as i64;
    offset.minutes = values.next().unwrap_or(0) as i64;
    offset.seconds = values.next().unwrap_or(0) as i64;

    Ok((offset, negated))
}
```

Then fix the one caller inside `shift_metadata_dates` (line ~309). Change:

```rust
    let offset = if op == ShiftOperation::Set {
        None
    } else {
        Some(parse_offset(offset_or_value)?)
    };
```

to:

```rust
    let (offset, op) = if op == ShiftOperation::Set {
        (None, op)
    } else {
        let (parsed, negated) = parse_offset(offset_or_value)?;
        let effective = match (op, negated) {
            (ShiftOperation::Add, true) => ShiftOperation::Subtract,
            (ShiftOperation::Subtract, true) => ShiftOperation::Add,
            (other, _) => other,
        };
        (Some(parsed), effective)
    };
```

(This whole function body is replaced again in Task 5; this minimal edit just keeps the crate compiling and the behavior correct in the interim.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib date_shift 2>&1 | tail -5`
Expected: all `date_shift` tests PASS (`test result: ok`).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --lib 2>&1 | tail -3
git add src/core/date_shift.rs
git commit -m "feat: accept ExifTool-style shift strings in date shifting

parse_offset now implements the Shift.pl grammar (verified against
ExifTool 13.55): one or two space-separated parts, 1-3 numbers each,
date right-justified, time left-justified, optional leading sign.
Fixes the first failure in issue #14 ('1:00:00' was rejected).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `ExifDateTag`, pattern resolution, and `ShiftSpec`

**Files:**
- Modify: `src/core/date_shift.rs` (add new items after the `DateOffset` impl block; add unit tests)

**Interfaces:**
- Consumes: `parse_offset(s) -> Result<(DateOffset, bool)>` (Task 1), existing `parse_absolute_datetime`, `apply_shift`, `ShiftOperation`.
- Produces (used by Tasks 3–5):
  - `pub enum ExifDateTag { ModifyDate, DateTimeOriginal, CreateDate }` with `pub fn tag_id(self) -> u16` and `pub fn key(self) -> &'static str`
  - `pub fn resolve_exif_targets(pattern: &str) -> Option<Vec<ExifDateTag>>`
  - `pub enum ShiftSpec { Relative { offset: DateOffset, op: ShiftOperation }, Absolute(DateTime<Utc>) }`
  - `pub fn build_shift_spec(offset_or_value: &str, op: ShiftOperation) -> Result<ShiftSpec>`
  - `pub fn apply_spec(dt: DateTime<Utc>, spec: &ShiftSpec) -> Result<DateTime<Utc>>`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/core/date_shift.rs`:

```rust
    #[test]
    fn test_resolve_bare_name() {
        assert_eq!(
            resolve_exif_targets("DateTimeOriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
        assert_eq!(
            resolve_exif_targets("datetimeoriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
    }

    #[test]
    fn test_resolve_aliases() {
        // ExifTool's names and the EXIF spec's names both resolve
        assert_eq!(resolve_exif_targets("ModifyDate"), Some(vec![ExifDateTag::ModifyDate]));
        assert_eq!(resolve_exif_targets("DateTime"), Some(vec![ExifDateTag::ModifyDate]));
        assert_eq!(resolve_exif_targets("CreateDate"), Some(vec![ExifDateTag::CreateDate]));
        assert_eq!(
            resolve_exif_targets("DateTimeDigitized"),
            Some(vec![ExifDateTag::CreateDate])
        );
    }

    #[test]
    fn test_resolve_group_prefixes() {
        assert_eq!(
            resolve_exif_targets("EXIF:DateTimeOriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
        assert_eq!(
            resolve_exif_targets("ExifIFD:DateTimeOriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
        assert_eq!(
            resolve_exif_targets("IFD0:ModifyDate"),
            Some(vec![ExifDateTag::ModifyDate])
        );
        // Wrong group for the tag: DateTimeOriginal lives in ExifIFD, not IFD0
        assert_eq!(resolve_exif_targets("IFD0:DateTimeOriginal"), None);
        // Unknown group
        assert_eq!(resolve_exif_targets("XMP:CreateDate"), None);
    }

    #[test]
    fn test_resolve_alldates() {
        assert_eq!(
            resolve_exif_targets("AllDates"),
            Some(vec![
                ExifDateTag::ModifyDate,
                ExifDateTag::DateTimeOriginal,
                ExifDateTag::CreateDate,
            ])
        );
        assert_eq!(resolve_exif_targets("alldates"), resolve_exif_targets("AllDates"));
    }

    #[test]
    fn test_resolve_unknown_returns_none() {
        assert_eq!(resolve_exif_targets("Artist"), None);
        assert_eq!(resolve_exif_targets("GPSDateStamp"), None);
    }

    #[test]
    fn test_build_shift_spec_relative_negated() {
        let spec = build_shift_spec("-1", ShiftOperation::Subtract).unwrap();
        // Subtracting a negative shift adds
        match spec {
            ShiftSpec::Relative { offset, op } => {
                assert_eq!(op, ShiftOperation::Add);
                assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 0, 0));
            }
            other => panic!("expected Relative, got {:?}", other),
        }
    }

    #[test]
    fn test_build_shift_spec_absolute() {
        let spec = build_shift_spec("2030:01:02 03:04:05", ShiftOperation::Set).unwrap();
        match spec {
            ShiftSpec::Absolute(dt) => {
                assert_eq!(format_exif_datetime(&dt), "2030:01:02 03:04:05");
            }
            other => panic!("expected Absolute, got {:?}", other),
        }
    }

    #[test]
    fn test_apply_spec_relative_and_absolute() {
        let dt = Utc.with_ymd_and_hms(2025, 6, 10, 12, 0, 0).unwrap();
        let relative = ShiftSpec::Relative {
            offset: DateOffset::new(0, 0, 0, 1, 0, 0),
            op: ShiftOperation::Subtract,
        };
        assert_eq!(
            format_exif_datetime(&apply_spec(dt, &relative).unwrap()),
            "2025:06:10 11:00:00"
        );
        let absolute = ShiftSpec::Absolute(Utc.with_ymd_and_hms(2030, 1, 2, 3, 4, 5).unwrap());
        assert_eq!(
            format_exif_datetime(&apply_spec(dt, &absolute).unwrap()),
            "2030:01:02 03:04:05"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib date_shift 2>&1 | tail -5`
Expected: compile error — `ExifDateTag`, `resolve_exif_targets`, `ShiftSpec`, `build_shift_spec`, `apply_spec` not found.

- [ ] **Step 3: Implement**

Add after the `DateOffset` impl block in `src/core/date_shift.rs`:

```rust
/// EXIF date/time tags that oxidex can shift in place.
///
/// These are exactly the three tags ExifTool's "AllDates" shortcut covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifDateTag {
    /// IFD0 tag 0x0132 — ExifTool name "ModifyDate" (EXIF spec: "DateTime")
    ModifyDate,
    /// ExifIFD tag 0x9003 — "DateTimeOriginal"
    DateTimeOriginal,
    /// ExifIFD tag 0x9004 — ExifTool name "CreateDate" (EXIF spec: "DateTimeDigitized")
    CreateDate,
}

impl ExifDateTag {
    /// The EXIF/TIFF tag ID.
    pub fn tag_id(self) -> u16 {
        match self {
            ExifDateTag::ModifyDate => 0x0132,
            ExifDateTag::DateTimeOriginal => 0x9003,
            ExifDateTag::CreateDate => 0x9004,
        }
    }

    /// The group-prefixed key oxidex uses for this tag in a MetadataMap.
    pub fn key(self) -> &'static str {
        match self {
            ExifDateTag::ModifyDate => "IFD0:ModifyDate",
            ExifDateTag::DateTimeOriginal => "ExifIFD:DateTimeOriginal",
            ExifDateTag::CreateDate => "ExifIFD:DateTimeDigitized",
        }
    }
}

/// Resolves a user-supplied tag pattern to the EXIF date tags it names.
///
/// Accepts ExifTool conventions: bare names ("DateTimeOriginal"), name
/// aliases ("DateTime" for ModifyDate, "DateTimeDigitized" for CreateDate),
/// the "EXIF:" family, oxidex's internal groups ("IFD0:", "ExifIFD:"), and
/// the "AllDates" shortcut. Matching is ASCII case-insensitive.
///
/// Returns `None` when the pattern does not name a known EXIF date/time tag.
pub fn resolve_exif_targets(pattern: &str) -> Option<Vec<ExifDateTag>> {
    let lowered = pattern.to_ascii_lowercase();
    if lowered == "alldates" {
        return Some(vec![
            ExifDateTag::ModifyDate,
            ExifDateTag::DateTimeOriginal,
            ExifDateTag::CreateDate,
        ]);
    }

    let (family, name) = match lowered.split_once(':') {
        Some((f, n)) => (Some(f), n),
        None => (None, lowered.as_str()),
    };

    let tag = match name {
        "modifydate" | "datetime" => ExifDateTag::ModifyDate,
        "datetimeoriginal" => ExifDateTag::DateTimeOriginal,
        "createdate" | "datetimedigitized" => ExifDateTag::CreateDate,
        _ => return None,
    };

    let family_ok = match family {
        None => true,
        Some("exif") => true,
        Some("ifd0") => tag == ExifDateTag::ModifyDate,
        Some("exififd") => tag != ExifDateTag::ModifyDate,
        Some(_) => false,
    };
    if family_ok { Some(vec![tag]) } else { None }
}

/// A fully parsed shift request: a relative offset or an absolute value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShiftSpec {
    /// Add or subtract a relative offset. `op` is only ever Add or Subtract.
    Relative {
        /// The parsed offset amount
        offset: DateOffset,
        /// The effective direction after folding in any leading sign
        op: ShiftOperation,
    },
    /// Set to an absolute date/time (the `=` operation).
    Absolute(DateTime<Utc>),
}

/// Builds a ShiftSpec from an operation and its argument string, folding a
/// leading `-` on the shift string into the operation direction.
pub fn build_shift_spec(offset_or_value: &str, op: ShiftOperation) -> Result<ShiftSpec> {
    if op == ShiftOperation::Set {
        return Ok(ShiftSpec::Absolute(parse_absolute_datetime(offset_or_value)?));
    }
    let (offset, negated) = parse_offset(offset_or_value)?;
    let effective = match (op, negated) {
        (ShiftOperation::Add, true) => ShiftOperation::Subtract,
        (ShiftOperation::Subtract, true) => ShiftOperation::Add,
        (other, _) => other,
    };
    Ok(ShiftSpec::Relative { offset, op: effective })
}

/// Applies a ShiftSpec to a date/time value.
pub fn apply_spec(dt: DateTime<Utc>, spec: &ShiftSpec) -> Result<DateTime<Utc>> {
    match spec {
        ShiftSpec::Absolute(value) => Ok(*value),
        ShiftSpec::Relative { offset, op } => apply_shift(dt, offset, *op),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib date_shift 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --lib 2>&1 | tail -3
git add src/core/date_shift.rs
git commit -m "feat: add EXIF date tag resolution and ShiftSpec for date shifting

resolve_exif_targets maps user patterns (bare names, EXIF:/IFD0:/ExifIFD:
groups, ExifTool aliases, AllDates) to the three canonical EXIF date tags.
ShiftSpec unifies relative and absolute shifts. Fixes the second failure
in issue #14 (bare 'DateTimeOriginal' was 'not found in metadata').

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: In-place EXIF datetime locator

**Files:**
- Create: `src/writers/exif_inplace.rs`
- Modify: `src/writers/mod.rs` (add `pub mod exif_inplace;` alongside the existing module declarations)

**Interfaces:**
- Consumes: `ExifDateTag` (Task 2), `read_u16`/`read_u32` from `crate::core::operations_helpers`, `ByteOrder` from `crate::parsers::tiff::ifd_parser`.
- Produces (used by Task 4):
  - `pub struct LocatedDateTag { pub tag: ExifDateTag, pub value_offset: usize }` (`value_offset` relative to TIFF header start)
  - `pub fn locate_exif_datetimes(tiff: &[u8]) -> Result<Vec<LocatedDateTag>>`

- [ ] **Step 1: Create the module with failing tests**

Create `src/writers/exif_inplace.rs`:

```rust
//! In-place EXIF date/time patching
//!
//! EXIF stores date/time tags as fixed-length 20-byte ASCII values
//! ("YYYY:MM:DD HH:MM:SS\0"), so shifting a date never changes a value's
//! length. This module rewrites only those bytes, leaving every other byte
//! of the file untouched. This deliberately avoids the whole-map rewrite in
//! `write_metadata`, which reconstructs the EXIF segment from
//! display-converted values and cannot round-trip binary tags (e.g.
//! ComponentsConfiguration, GPSVersionID) losslessly.

use crate::core::date_shift::ExifDateTag;
use crate::core::operations_helpers::{read_u16, read_u32};
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// IFD0 tag pointing to the ExifIFD
const EXIF_IFD_POINTER: u16 = 0x8769;
/// TIFF ASCII type code
const ASCII_TYPE: u16 = 2;
/// Byte count of a standard EXIF date/time value (19 chars + NUL)
const DATETIME_LEN: u32 = 20;

/// Location of a shiftable date/time value inside a TIFF structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocatedDateTag {
    /// Which date tag this is
    pub tag: ExifDateTag,
    /// Offset of the 20-byte ASCII value, relative to the TIFF header start
    pub value_offset: usize,
}

/// Which IFD is being scanned (determines which tag IDs are date/time tags).
#[derive(Clone, Copy, PartialEq)]
enum Ifd {
    Ifd0,
    ExifIfd,
}

/// Walks IFD0 and the ExifIFD of `tiff` and returns the location of every
/// standard-format date/time tag value.
///
/// Tags whose value is not type ASCII with count 20, or whose value offset
/// falls outside `tiff`, are skipped (never patched) rather than risking
/// corruption.
pub fn locate_exif_datetimes(tiff: &[u8]) -> Result<Vec<LocatedDateTag>> {
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
    let ifd0_offset = read_u32(&tiff[4..8], byte_order) as usize;

    let mut found = Vec::new();
    let exif_ifd_offset = scan_ifd(tiff, ifd0_offset, byte_order, Ifd::Ifd0, &mut found)?;
    if let Some(offset) = exif_ifd_offset {
        scan_ifd(tiff, offset, byte_order, Ifd::ExifIfd, &mut found)?;
    }
    Ok(found)
}

/// Scans one IFD, appending located date/time values to `found`.
/// Returns the ExifIFD offset when this IFD contains an ExifIFD pointer.
fn scan_ifd(
    tiff: &[u8],
    offset: usize,
    byte_order: ByteOrder,
    which: Ifd,
    found: &mut Vec<LocatedDateTag>,
) -> Result<Option<usize>> {
    let entries_start = offset
        .checked_add(2)
        .ok_or_else(|| ExifToolError::parse_error("IFD offset overflow"))?;
    if entries_start > tiff.len() {
        return Err(ExifToolError::parse_error("IFD offset beyond EXIF data"));
    }
    let entry_count = read_u16(&tiff[offset..entries_start], byte_order) as usize;
    let mut exif_ifd_offset = None;

    for i in 0..entry_count {
        let entry_start = entries_start + i * 12;
        let entry_end = entry_start + 12;
        if entry_end > tiff.len() {
            // Truncated IFD: stop scanning rather than failing on real-world files
            break;
        }
        let entry = &tiff[entry_start..entry_end];
        let tag_id = read_u16(&entry[0..2], byte_order);
        let value_type = read_u16(&entry[2..4], byte_order);
        let value_count = read_u32(&entry[4..8], byte_order);
        let value_or_offset = read_u32(&entry[8..12], byte_order) as usize;

        if which == Ifd::Ifd0 && tag_id == EXIF_IFD_POINTER {
            exif_ifd_offset = Some(value_or_offset);
            continue;
        }
        let date_tag = match (which, tag_id) {
            (Ifd::Ifd0, 0x0132) => ExifDateTag::ModifyDate,
            (Ifd::ExifIfd, 0x9003) => ExifDateTag::DateTimeOriginal,
            (Ifd::ExifIfd, 0x9004) => ExifDateTag::CreateDate,
            _ => continue,
        };
        // A count-20 ASCII value is larger than 4 bytes, so it is always
        // stored at an offset, never inline in the entry.
        if value_type == ASCII_TYPE
            && value_count == DATETIME_LEN
            && value_or_offset + DATETIME_LEN as usize <= tiff.len()
        {
            found.push(LocatedDateTag {
                tag: date_tag,
                value_offset: value_or_offset,
            });
        }
    }
    Ok(exif_ifd_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_bytes(v: u16, bo: ByteOrder) -> [u8; 2] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }

    fn u32_bytes(v: u32, bo: ByteOrder) -> [u8; 4] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }

    /// Builds a minimal TIFF structure:
    /// - IFD0 at offset 8 with ModifyDate (value at 38) and an ExifIFD pointer (58)
    /// - ExifIFD at 58 with DateTimeOriginal (value at 88) and CreateDate (value at 108)
    fn build_test_tiff(bo: ByteOrder) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(match bo {
            ByteOrder::LittleEndian => b"II",
            ByteOrder::BigEndian => b"MM",
        });
        t.extend_from_slice(&u16_bytes(42, bo));
        t.extend_from_slice(&u32_bytes(8, bo));
        // IFD0 at 8: 2 entries
        t.extend_from_slice(&u16_bytes(2, bo));
        t.extend_from_slice(&u16_bytes(0x0132, bo)); // ModifyDate
        t.extend_from_slice(&u16_bytes(2, bo)); // ASCII
        t.extend_from_slice(&u32_bytes(20, bo));
        t.extend_from_slice(&u32_bytes(38, bo));
        t.extend_from_slice(&u16_bytes(0x8769, bo)); // ExifIFD pointer
        t.extend_from_slice(&u16_bytes(4, bo)); // LONG
        t.extend_from_slice(&u32_bytes(1, bo));
        t.extend_from_slice(&u32_bytes(58, bo));
        t.extend_from_slice(&u32_bytes(0, bo)); // next IFD
        t.extend_from_slice(b"2025:01:15 10:30:00\0"); // 38..58
        // ExifIFD at 58: 2 entries
        t.extend_from_slice(&u16_bytes(2, bo));
        t.extend_from_slice(&u16_bytes(0x9003, bo)); // DateTimeOriginal
        t.extend_from_slice(&u16_bytes(2, bo));
        t.extend_from_slice(&u32_bytes(20, bo));
        t.extend_from_slice(&u32_bytes(88, bo));
        t.extend_from_slice(&u16_bytes(0x9004, bo)); // CreateDate
        t.extend_from_slice(&u16_bytes(2, bo));
        t.extend_from_slice(&u32_bytes(20, bo));
        t.extend_from_slice(&u32_bytes(108, bo));
        t.extend_from_slice(&u32_bytes(0, bo)); // next IFD
        t.extend_from_slice(b"2025:06:10 12:00:00\0"); // 88..108
        t.extend_from_slice(b"2025:06:10 12:00:05\0"); // 108..128
        t
    }

    #[test]
    fn test_locate_all_three_tags_little_endian() {
        let tiff = build_test_tiff(ByteOrder::LittleEndian);
        let located = locate_exif_datetimes(&tiff).unwrap();
        assert_eq!(
            located,
            vec![
                LocatedDateTag { tag: ExifDateTag::ModifyDate, value_offset: 38 },
                LocatedDateTag { tag: ExifDateTag::DateTimeOriginal, value_offset: 88 },
                LocatedDateTag { tag: ExifDateTag::CreateDate, value_offset: 108 },
            ]
        );
    }

    #[test]
    fn test_locate_all_three_tags_big_endian() {
        let tiff = build_test_tiff(ByteOrder::BigEndian);
        let located = locate_exif_datetimes(&tiff).unwrap();
        assert_eq!(located.len(), 3);
        assert_eq!(located[1].tag, ExifDateTag::DateTimeOriginal);
        assert_eq!(located[1].value_offset, 88);
    }

    #[test]
    fn test_nonstandard_count_is_skipped() {
        let mut tiff = build_test_tiff(ByteOrder::LittleEndian);
        // ModifyDate entry starts at 10; its count field is at 14..18
        tiff[14..18].copy_from_slice(&19u32.to_le_bytes());
        let located = locate_exif_datetimes(&tiff).unwrap();
        // ModifyDate skipped, the two ExifIFD tags still found
        assert_eq!(located.len(), 2);
        assert!(located.iter().all(|l| l.tag != ExifDateTag::ModifyDate));
    }

    #[test]
    fn test_value_offset_out_of_bounds_is_skipped() {
        let mut tiff = build_test_tiff(ByteOrder::LittleEndian);
        // ModifyDate entry value-offset field is at 18..22; point past the end
        tiff[18..22].copy_from_slice(&5000u32.to_le_bytes());
        let located = locate_exif_datetimes(&tiff).unwrap();
        assert_eq!(located.len(), 2);
        assert!(located.iter().all(|l| l.tag != ExifDateTag::ModifyDate));
    }

    #[test]
    fn test_invalid_tiff_errors() {
        assert!(locate_exif_datetimes(&[]).is_err());
        assert!(locate_exif_datetimes(b"XX\x2a\x00\x08\x00\x00\x00").is_err());
    }
}
```

And add to `src/writers/mod.rs`:

```rust
pub mod exif_inplace;
```

- [ ] **Step 2: Run tests to verify they pass** (implementation and tests land together here since the module is new; the "failing" state was the missing module)

Run: `cargo test --lib exif_inplace 2>&1 | tail -5`
Expected: 5 tests PASS.

- [ ] **Step 3: Format, lint, commit**

```bash
cargo fmt && cargo clippy --lib 2>&1 | tail -3
git add src/writers/exif_inplace.rs src/writers/mod.rs
git commit -m "feat: add in-place EXIF datetime locator

Walks IFD0 and ExifIFD of a TIFF byte range and returns the value
offsets of the three canonical date/time tags, verifying each is a
standard 20-byte ASCII value before it may be patched.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: In-place JPEG date patcher

**Files:**
- Modify: `src/writers/exif_inplace.rs` (add `shift_jpeg_exif_dates` and `SliceReader`)
- Create: `tests/date_shift_inplace.rs`

**Interfaces:**
- Consumes: `locate_exif_datetimes` (Task 3); `ShiftSpec`, `apply_spec`, `parse_absolute_datetime`, `format_exif_datetime`, `ExifDateTag` from `crate::core::date_shift` (Task 2); `parse_segments` from `crate::parsers::jpeg`; `FileReader` from `crate::core`; `write_atomic` from `crate::writers::atomic_writer`.
- Produces (used by Task 5): `pub fn shift_jpeg_exif_dates(path: &Path, targets: &[ExifDateTag], spec: &ShiftSpec) -> Result<usize>` — returns the number of tags modified; targets absent from the file are skipped, not errors.

- [ ] **Step 1: Write the failing integration test**

Create `tests/date_shift_inplace.rs`:

```rust
//! Integration tests for in-place EXIF date shifting (GitHub issue #14).
//!
//! The critical property: shifting a date must not change ANY byte of the
//! file outside the 19 ASCII characters of the target datetime value(s).
//! The GPS fixture contains ComponentsConfiguration and GPSVersionID, the
//! binary tags that the old whole-map rewrite corrupted.

use oxidex::core::date_shift::{
    ExifDateTag, ShiftOperation, build_shift_spec,
};
use oxidex::core::operations::read_metadata;
use oxidex::writers::exif_inplace::shift_jpeg_exif_dates;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg")
        .join(name)
}

fn temp_copy(src: &Path, label: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!(
        "oxidex_shift_{}_{}",
        std::process::id(),
        label
    ));
    std::fs::copy(src, &dst).unwrap();
    dst
}

/// Returns the byte indices at which the two files differ.
fn diff_indices(a: &Path, b: &Path) -> Vec<usize> {
    let a = std::fs::read(a).unwrap();
    let b = std::fs::read(b).unwrap();
    assert_eq!(a.len(), b.len(), "file length must not change");
    a.iter()
        .zip(b.iter())
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn inplace_shift_changes_only_datetime_bytes() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "bytediff.jpg");

    let spec = build_shift_spec("1:00:00", ShiftOperation::Subtract).unwrap();
    let modified =
        shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 1);

    let diffs = diff_indices(&src, &dst);
    assert!(!diffs.is_empty(), "the datetime bytes must have changed");
    assert!(
        diffs.last().unwrap() - diffs.first().unwrap() < 19,
        "all changed bytes must lie within one 19-byte datetime value, got {:?}",
        diffs
    );

    // 2024-02-01T14:30:00 minus 1 hour
    let metadata = read_metadata(&dst).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2024-02-01T13:30:00+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn inplace_shift_binary_tags_survive() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "binary_survive.jpg");

    let spec = build_shift_spec("0:0:0 1:00:00", ShiftOperation::Subtract).unwrap();
    shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap();

    // The corruption canaries must read back identically
    let before = read_metadata(&src).unwrap();
    let after = read_metadata(&dst).unwrap();
    for canary in ["ExifIFD:ComponentsConfiguration", "GPS:GPSVersionID"] {
        assert_eq!(
            before.get(canary),
            after.get(canary),
            "binary tag {} must survive a date shift unchanged",
            canary
        );
    }

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn inplace_set_absolute_value() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "set_abs.jpg");

    let spec = build_shift_spec("2030:01:02 03:04:05", ShiftOperation::Set).unwrap();
    let modified =
        shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 1);

    let metadata = read_metadata(&dst).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2030-01-02T03:04:05+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn inplace_shift_missing_tag_returns_zero() {
    // sample_with_exif.jpg has ModifyDate but no DateTimeOriginal
    let src = fixture("sample_with_exif.jpg");
    let dst = temp_copy(&src, "missing_tag.jpg");

    let spec = build_shift_spec("1", ShiftOperation::Subtract).unwrap();
    let modified =
        shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec).unwrap();
    assert_eq!(modified, 0);
    // File must be untouched when nothing matched
    assert!(diff_indices(&src, &dst).is_empty());

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn inplace_shift_no_exif_errors() {
    // A JPEG with no EXIF APP1 segment at all
    let dst = std::env::temp_dir().join(format!(
        "oxidex_shift_{}_noexif.jpg",
        std::process::id()
    ));
    std::fs::write(&dst, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();

    let spec = build_shift_spec("1", ShiftOperation::Subtract).unwrap();
    let err = shift_jpeg_exif_dates(&dst, &[ExifDateTag::DateTimeOriginal], &spec)
        .unwrap_err();
    assert!(err.to_string().contains("No EXIF data"), "got: {}", err);

    std::fs::remove_file(&dst).unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test date_shift_inplace 2>&1 | tail -5`
Expected: compile error — `shift_jpeg_exif_dates` not found.

- [ ] **Step 3: Implement**

Add to `src/writers/exif_inplace.rs` (after the existing `use` lines, extend imports):

```rust
use crate::core::FileReader;
use crate::core::date_shift::{ShiftSpec, apply_spec, format_exif_datetime, parse_absolute_datetime};
use crate::parsers::jpeg::parse_segments;
use crate::writers::atomic_writer::write_atomic;
use std::path::Path;

/// EXIF identifier at the start of an EXIF APP1 segment
const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";
```

Then add after `scan_ifd`:

```rust
/// A FileReader over an in-memory byte slice.
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

/// Shifts EXIF date/time tags of a JPEG in place.
///
/// Patches only the 19 ASCII characters of each target tag's value; every
/// other byte of the file is preserved verbatim. Returns the number of tags
/// modified. Targets not present in the file are skipped, not errors; the
/// file is not rewritten at all when nothing matched.
pub fn shift_jpeg_exif_dates(
    path: &Path,
    targets: &[ExifDateTag],
    spec: &ShiftSpec,
) -> Result<usize> {
    let mut file_bytes = std::fs::read(path)?;

    // Find the EXIF APP1 segment. The TIFF structure starts after
    // marker (2) + length field (2) + "Exif\0\0" (6).
    let (tiff_start, tiff_len) = {
        let reader = SliceReader(&file_bytes);
        let segments = parse_segments(&reader)?;
        let exif_seg = segments
            .iter()
            .find(|s| s.is_app1() && s.data.starts_with(EXIF_IDENTIFIER))
            .ok_or_else(|| ExifToolError::parse_error("No EXIF data found in JPEG"))?;
        (
            exif_seg.offset as usize + 4 + EXIF_IDENTIFIER.len(),
            exif_seg.data.len() - EXIF_IDENTIFIER.len(),
        )
    };

    let located = locate_exif_datetimes(&file_bytes[tiff_start..tiff_start + tiff_len])?;

    let mut modified = 0;
    for target in targets {
        let Some(location) = located.iter().find(|l| l.tag == *target) else {
            continue;
        };
        let value_start = tiff_start + location.value_offset;
        let current = std::str::from_utf8(&file_bytes[value_start..value_start + 19])
            .map_err(|_| {
                ExifToolError::parse_error(format!(
                    "Tag '{}' has a non-ASCII date value",
                    target.key()
                ))
            })?;
        let dt = parse_absolute_datetime(current)?;
        let new_dt = apply_spec(dt, spec)?;
        let formatted = format_exif_datetime(&new_dt);
        file_bytes[value_start..value_start + 19].copy_from_slice(formatted.as_bytes());
        modified += 1;
    }

    if modified > 0 {
        write_atomic(path, &file_bytes)?;
    }
    Ok(modified)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test date_shift_inplace 2>&1 | tail -5` and `cargo test --lib exif_inplace 2>&1 | tail -3`
Expected: all PASS. If `parse_absolute_datetime` fails on a fixture value, inspect the actual 19 bytes — they must be `YYYY:MM:DD HH:MM:SS`.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --lib --tests 2>&1 | tail -3
git add src/writers/exif_inplace.rs tests/date_shift_inplace.rs
git commit -m "feat: shift JPEG EXIF dates by in-place byte patching

EXIF datetimes are fixed-length 20-byte ASCII, so a shift can overwrite
the 19 value characters at their original offset without rewriting the
EXIF segment. This fixes the third failure in issue #14: the whole-map
rewrite either failed validation on binary tags (ComponentsConfiguration,
GPSVersionID) or corrupted them. Byte-level regression tests pin the
only-19-bytes-change property.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Rewire `shift_metadata_dates`

**Files:**
- Modify: `src/core/date_shift.rs` (replace `shift_metadata_dates` and the `ALL_DATES_TAGS` const, lines ~240–403; update imports and doc comment)

**Interfaces:**
- Consumes: `build_shift_spec`, `resolve_exif_targets`, `apply_spec` (Task 2), `shift_jpeg_exif_dates` (Task 4), `read_metadata`/`write_metadata` (existing), `MMapReader` (`crate::io`), `detect_format` (`crate::parsers::detection`), `FileFormat` (`crate::core`).
- Produces: `pub fn shift_metadata_dates(path: &Path, tag_pattern: &str, offset_or_value: &str, op: ShiftOperation) -> Result<()>` — same signature as today (called from `src/main.rs:492`), new routing: JPEG → in-place patch; other formats → map path with case-insensitive pattern matching.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/core/date_shift.rs`:

```rust
    #[test]
    fn test_key_matches_pattern() {
        // Bare pattern matches any family, case-insensitively
        assert!(key_matches_pattern("XMP:CreateDate", "createdate"));
        assert!(key_matches_pattern("PDF:CreateDate", "CreateDate"));
        // Prefixed pattern must match the whole key
        assert!(key_matches_pattern("XMP:CreateDate", "xmp:createdate"));
        assert!(!key_matches_pattern("PDF:CreateDate", "XMP:CreateDate"));
        // Name-only mismatch
        assert!(!key_matches_pattern("XMP:ModifyDate", "CreateDate"));
    }
```

And add to `tests/date_shift_inplace.rs`:

```rust
#[test]
fn shift_metadata_dates_bare_name_on_jpeg() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "public_api.jpg");

    // The exact tag pattern and offset string from issue #14
    oxidex::core::date_shift::shift_metadata_dates(
        &dst,
        "DateTimeOriginal",
        "1:00:00",
        ShiftOperation::Subtract,
    )
    .unwrap();

    let metadata = read_metadata(&dst).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2024-02-01T13:30:00+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn shift_metadata_dates_alldates_on_jpeg() {
    // The fixture's only canonical date tag is ExifIFD:DateTimeOriginal,
    // so AllDates shifts exactly that one
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "alldates.jpg");

    oxidex::core::date_shift::shift_metadata_dates(
        &dst,
        "AllDates",
        "0:0:0 1:00:00",
        ShiftOperation::Subtract,
    )
    .unwrap();

    let metadata = read_metadata(&dst).unwrap();
    let dt = metadata
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2024-02-01T13:30:00+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn shift_metadata_dates_unsupported_jpeg_tag_errors_cleanly() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "unsupported.jpg");

    let err = oxidex::core::date_shift::shift_metadata_dates(
        &dst,
        "XMP:CreateDate",
        "1",
        ShiftOperation::Subtract,
    )
    .unwrap_err();
    assert!(err.to_string().contains("not supported"), "got: {}", err);
    // Must not have touched the file
    assert!(diff_indices(&src, &dst).is_empty());

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn shift_metadata_dates_missing_tag_errors() {
    // sample_with_exif.jpg has no DateTimeOriginal
    let src = fixture("sample_with_exif.jpg");
    let dst = temp_copy(&src, "missing_err.jpg");

    let err = oxidex::core::date_shift::shift_metadata_dates(
        &dst,
        "DateTimeOriginal",
        "1",
        ShiftOperation::Subtract,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("No date/time tags matching"),
        "got: {}",
        err
    );

    std::fs::remove_file(&dst).unwrap();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib date_shift 2>&1 | tail -5` (compile error: `key_matches_pattern` missing) and `cargo test --test date_shift_inplace 2>&1 | tail -8` (the new tests fail: bare `DateTimeOriginal` → "not found in metadata"; the unsupported-tag test fails because there is no clean error yet).

- [ ] **Step 3: Implement**

In `src/core/date_shift.rs`:

1. Extend the imports at the top of the file:

```rust
use super::operations::{read_metadata, write_metadata};
use super::tag_value::TagValue;
use crate::core::FileFormat;
use crate::error::{ExifToolError, Result};
use crate::io::MMapReader;
use crate::parsers::detection::detect_format;
use chrono::{DateTime, Duration, Months, NaiveDateTime, Utc};
use std::path::Path;
```

2. Delete the `ALL_DATES_TAGS` const (lines ~240–252) and replace the entire `shift_metadata_dates` function (doc comment included) with:

```rust
/// Canonical date/time tag names shifted by the "AllDates" pattern on
/// formats that use the metadata-map write path (PNG, PDF). Lowercase.
const ALL_DATES_NAMES: &[&str] = &[
    "modifydate",
    "datetime",
    "datetimeoriginal",
    "createdate",
    "datetimedigitized",
    "creationtime",
    "contentcreatedate",
];

/// Returns true when a metadata key matches a user-supplied tag pattern.
///
/// A bare pattern ("DateTimeOriginal") matches the name part of any
/// group-prefixed key; a prefixed pattern ("XMP:CreateDate") must match the
/// full key. Comparison is ASCII case-insensitive.
fn key_matches_pattern(key: &str, pattern: &str) -> bool {
    if key.eq_ignore_ascii_case(pattern) {
        return true;
    }
    if !pattern.contains(':')
        && let Some((_, name)) = key.split_once(':')
    {
        return name.eq_ignore_ascii_case(pattern);
    }
    false
}

/// Shifts date/time tags in a file's metadata.
///
/// # Arguments
///
/// * `path` - Path to the file to modify
/// * `tag_pattern` - "AllDates", a bare tag name ("DateTimeOriginal"), or a
///   group-prefixed name ("EXIF:DateTimeOriginal", "IFD0:ModifyDate")
/// * `offset_or_value` - ExifTool-style shift string (see [`parse_offset`]),
///   or an absolute "YYYY:MM:DD HH:MM:SS" for the Set operation
/// * `op` - Operation type (Add, Subtract, or Set)
///
/// # Behavior by format
///
/// * **JPEG**: the EXIF date values are patched in place — only the 19 ASCII
///   characters of each target value change, every other byte of the file is
///   preserved. Supported tags: AllDates, ModifyDate (DateTime),
///   DateTimeOriginal, CreateDate (DateTimeDigitized).
/// * **Other formats** (PNG, PDF): tags are shifted through the metadata map
///   and rewritten with [`write_metadata`].
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::date_shift::{shift_metadata_dates, ShiftOperation};
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Subtract 1 hour from DateTimeOriginal (ExifTool: -DateTimeOriginal-=1:00:00)
/// shift_metadata_dates(
///     Path::new("photo.jpg"),
///     "DateTimeOriginal",
///     "1:00:00",
///     ShiftOperation::Subtract
/// )?;
///
/// // Add 1 day to all date tags
/// shift_metadata_dates(Path::new("photo.jpg"), "AllDates", "0:0:1 0", ShiftOperation::Add)?;
/// # Ok(())
/// # }
/// ```
pub fn shift_metadata_dates(
    path: &Path,
    tag_pattern: &str,
    offset_or_value: &str,
    op: ShiftOperation,
) -> Result<()> {
    let spec = build_shift_spec(offset_or_value, op)?;

    let format = {
        let reader = MMapReader::new(path)?;
        detect_format(&reader)?
    };

    if format == FileFormat::JPEG {
        return shift_jpeg_dates(path, tag_pattern, &spec);
    }
    shift_map_dates(path, tag_pattern, &spec)
}

/// JPEG path: patch EXIF date/time values in place. Never rewrites the EXIF
/// segment, so binary tags are preserved byte-for-byte.
fn shift_jpeg_dates(path: &Path, tag_pattern: &str, spec: &ShiftSpec) -> Result<()> {
    let Some(targets) = resolve_exif_targets(tag_pattern) else {
        return Err(ExifToolError::parse_error(format!(
            "Shifting tag '{}' is not supported for JPEG. Supported: AllDates, \
             ModifyDate (DateTime), DateTimeOriginal, CreateDate (DateTimeDigitized)",
            tag_pattern
        )));
    };
    let modified = crate::writers::exif_inplace::shift_jpeg_exif_dates(path, &targets, spec)?;
    if modified == 0 {
        return Err(ExifToolError::parse_error(format!(
            "No date/time tags matching '{}' found in EXIF data",
            tag_pattern
        )));
    }
    Ok(())
}

/// Non-JPEG path: shift date/time tags through the metadata map (PNG, PDF).
fn shift_map_dates(path: &Path, tag_pattern: &str, spec: &ShiftSpec) -> Result<()> {
    let mut metadata = read_metadata(path)?;
    let all_dates = tag_pattern.eq_ignore_ascii_case("AllDates");

    let keys: Vec<String> = metadata.iter().map(|(k, _)| k.clone()).collect();
    let mut modified = 0;
    for key in keys {
        let matches = if all_dates {
            // Filesystem dates are never shifted by AllDates
            !key.starts_with("File:")
                && key.split_once(':').map_or_else(
                    || ALL_DATES_NAMES.contains(&key.to_ascii_lowercase().as_str()),
                    |(_, name)| ALL_DATES_NAMES.contains(&name.to_ascii_lowercase().as_str()),
                )
        } else {
            key_matches_pattern(&key, tag_pattern)
        };
        if !matches {
            continue;
        }
        let Some(dt) = metadata.get(&key).and_then(|v| v.as_datetime()).copied() else {
            if !all_dates {
                return Err(ExifToolError::parse_error(format!(
                    "Tag '{}' is not a DateTime tag",
                    key
                )));
            }
            continue;
        };
        let new_dt = apply_spec(dt, spec)?;
        metadata.insert(key, TagValue::new_datetime(new_dt));
        modified += 1;
    }

    if modified == 0 {
        return Err(ExifToolError::parse_error(format!(
            "Tag '{}' not found in metadata",
            tag_pattern
        )));
    }
    write_metadata(path, &metadata)?;
    Ok(())
}
```

Note: the Task 1 interim edit inside the old `shift_metadata_dates` body is deleted along with that body — `build_shift_spec` now owns the sign-folding.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib date_shift 2>&1 | tail -5` and `cargo test --test date_shift_inplace 2>&1 | tail -5`
Expected: all PASS.

- [ ] **Step 5: Run the full workspace suite to catch regressions**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: no failures. If a pre-existing test asserted the old strict-format error messages or `ALL_DATES_TAGS` behavior, update it to the new semantics (grep for `Invalid offset format` and `ALL_DATES_TAGS` across `tests/` — as of planning, no test outside `date_shift.rs` references them).

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt && cargo clippy --workspace 2>&1 | tail -3
git add src/core/date_shift.rs tests/date_shift_inplace.rs
git commit -m "fix: route JPEG date shifts through the in-place EXIF patcher

shift_metadata_dates now resolves ExifTool-style tag patterns and, for
JPEG, patches datetime values in place instead of rewriting the EXIF
segment from display-converted values. Non-JPEG formats keep the map
path with case-insensitive pattern matching; AllDates no longer shifts
arbitrary datetime-valued tags or filesystem dates.

Fixes swack-tools/oxidex#14

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: End-to-end CLI verification of the issue's exact commands

**Files:**
- Modify: `tests/date_shift_inplace.rs` (add CLI-level tests)

**Interfaces:**
- Consumes: the `oxidex` binary via `env!("CARGO_BIN_EXE_oxidex")` (provided by Cargo for integration tests), `read_metadata`, fixtures.
- Produces: regression coverage for the exact commands from the issue report.

- [ ] **Step 1: Write the failing tests** (they should pass immediately if Tasks 1–5 are correct; a failure here means a wiring bug in `main.rs`/`args.rs`)

Add to `tests/date_shift_inplace.rs`:

```rust
use std::process::Command;

/// Runs the oxidex binary with one shift argument against a file.
fn run_oxidex(shift_arg: &str, file: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg(shift_arg)
        .arg(file)
        .output()
        .unwrap()
}

fn read_dto(path: &Path) -> String {
    read_metadata(path)
        .unwrap()
        .get("ExifIFD:DateTimeOriginal")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap()
        .to_rfc3339()
}

#[test]
fn cli_issue_14_short_form() {
    // The first command from the issue report, verbatim
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "cli_short.jpg");

    let output = run_oxidex("-DateTimeOriginal-=1:00:00", &dst);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read_dto(&dst), "2024-02-01T13:30:00+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn cli_issue_14_long_form() {
    // The second command from the issue report, verbatim
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "cli_long.jpg");

    let output = run_oxidex("-DateTimeOriginal-=0:0:0 1:00:00", &dst);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read_dto(&dst), "2024-02-01T13:30:00+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn cli_exif_prefixed_add() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let dst = temp_copy(&src, "cli_prefixed.jpg");

    let output = run_oxidex("-EXIF:DateTimeOriginal+=1:30", &dst);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read_dto(&dst), "2024-02-01T16:00:00+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn cli_modify_date_on_sample_fixture() {
    // sample_with_exif.jpg: IFD0:ModifyDate = 2025-01-15T10:30:00
    let src = fixture("sample_with_exif.jpg");
    let dst = temp_copy(&src, "cli_modifydate.jpg");

    let output = run_oxidex("-ModifyDate-=1", &dst);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dt = read_metadata(&dst)
        .unwrap()
        .get("IFD0:ModifyDate")
        .and_then(|v| v.as_datetime())
        .copied()
        .unwrap();
    assert_eq!(dt.to_rfc3339(), "2025-01-15T09:30:00+00:00");

    std::fs::remove_file(&dst).unwrap();
}

#[test]
fn cli_failure_exits_nonzero_with_clear_message() {
    let src = fixture("sample_with_exif.jpg"); // no DateTimeOriginal
    let dst = temp_copy(&src, "cli_fail.jpg");

    let output = run_oxidex("-DateTimeOriginal-=1:00:00", &dst);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No date/time tags matching"),
        "stderr: {}",
        stderr
    );

    std::fs::remove_file(&dst).unwrap();
}
```

- [ ] **Step 2: Run the CLI tests**

Run: `cargo test --test date_shift_inplace 2>&1 | tail -8`
Expected: all PASS (Cargo builds the binary automatically for `CARGO_BIN_EXE_`). If `cli_exif_prefixed_add` fails at argument parsing (lexopt may eat `-EXIF:...`), check how `src/cli/args.rs` recovers raw args via `extract_arg_from_error` — the same path already handles `-EXIF:Artist=...`; adjust only if the test proves otherwise.

- [ ] **Step 3: Full verification sweep**

```bash
cargo test --workspace 2>&1 | tail -5
cargo clippy --workspace 2>&1 | tail -3
cargo fmt --check
```
Expected: tests ok, clippy clean, fmt clean.

Then verify the fix end-to-end against real ExifTool (manual, exiftool 13.55 is installed):

```bash
cargo build
cp tests/fixtures/jpeg/complex/synthetic_gps_001.jpg /tmp/issue14_final.jpg
./target/debug/oxidex "-DateTimeOriginal-=1:00:00" /tmp/issue14_final.jpg
exiftool -s3 -DateTimeOriginal -ComponentsConfiguration -GPSVersionID /tmp/issue14_final.jpg
```
Expected output: `2024:02:01 13:30:00`, `Y, Cb, Cr, -`, `2.3.0.0` — date shifted, binary tags intact, readable by reference ExifTool.

- [ ] **Step 4: Commit**

```bash
git add tests/date_shift_inplace.rs
git commit -m "test: cover issue #14 CLI commands end-to-end

Runs the oxidex binary with the exact arguments from the issue report
and verifies results against ExifTool-pinned expectations.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Out of Scope (deliberate)

- **TIFF / camera-raw date shifting**: the in-place patcher would work there too (TIFF origin = file offset 0), but the issue is JPEG-specific. Follow-up candidate.
- **XMP date shifting on JPEG**: requires safe XMP rewriting; currently errors clearly instead of corrupting.
- **Fractional-seconds and timezone shift strings** (`OffsetTime*`, `SubSecTime*` tags): ExifTool supports them; rejected here with a clear parse error.
- **The general read→write round-trip corruption** (writer serializes display strings for binary tags via `reconstruct_tiff_structure`): pre-existing, affects `modify_tag`/`copy_metadata`, needs its own issue and plan. Do not "fix" it by relaxing validation (see Global Constraints).
