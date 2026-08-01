# Design: Nikon `AFPointsUsed` / `PrimaryAFPoint` grid transcription

## Problem

Nikon `AFPointsUsed` and `PrimaryAFPoint` (`Nikon::Main` 0x00b7, the `AFInfo2`
block) are deliberately not decoded in
[af_info2.rs](../../../src/parsers/tiff/makernotes/nikon/af_info2.rs) — see
the module header. Each is a bitmap or index whose meaning depends on a
per-body point-name grid; picking the wrong grid would silently produce a
confident, wrong list of focus points. 54 files under
`/tmp/oxidex-exiftool-cache/combined-samples/Nikon` carry one or both tags,
and oxidex currently emits neither.

The guard is correct and stays. The fix is to supply the grids ExifTool
already has, not to remove the guard.

## Scope

Exactly two tags: `AFPointsUsed` and `PrimaryAFPoint`, across every
`AFInfo2` version ExifTool dispatches them for: `0100`, `0101`, `0200`,
`0201`, `0300`, `0301`, `0400`, `0401`, `0402`.

Explicitly **out of scope**: `AFPointsInFocus`, `AFPointsSelected`,
`FocusPositionHorizontal`/`FocusPositionVertical` — real tags in the same
binary tables, but not named in the task. Pulling them in blurs the
verification bar.

## Source of truth

`/tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool/Nikon.pm`:

- Dispatch tables: `%Image::ExifTool::Nikon::AFInfo2V0100` (~line 4130)
  through `AFInfo2V0400` (~line 4922). `AFPointsUsed`/`PrimaryAFPoint`
  `Condition` arms are at (first occurrences) lines 4181, 4194, 4213, 4224
  (`PrimaryAFPoint`, V0100/V0101) and 4231, 4244, 4271, 4284 (`AFPointsUsed`,
  same versions), then repeated per-version through 4396–4989.
- Point-name tables (lines 1441–~1720): `%afPoints11` (BITMASK-style, named
  directions not grid coordinates), `%afPoints51`, `%afPoints39`,
  `%afPoints105`, `%afPoints135`, `%afPoints153`, `%afPoints81` (hashes,
  `bit-number => "RowCol"`), and `@afPoints231`, `@afPoints299`,
  `@afPoints405` (plain arrays, ExifTool authored these as literal `qw()`
  lists rather than hashes — same underlying meaning, kept as arrays in Rust
  too, to stay faithful to the source rather than force a single
  representation).
- Print conversion: `sub PrintAFPoints` (line 13307) — bitmap walk with a
  table/array lookup per set bit. `sub PrintAFPointsGrid` +
  `sub GetAFPointGrid` (lines 13361, 13378) — bitmap walk with the point name
  *computed* (`row = bit/ncols`, `col = bit%ncols+1`,
  `name = chr(65+row) + col`) instead of looked up. Three of the ten tables
  (Nikon 1's newer 135-point mode, the 171-point mode, and all of
  V0400's 231/299/405-point grids) go through `PrintAFPointsGrid`; the rest
  go through `PrintAFPoints`.

Precedent for this shape of Condition chain plus cross-tag data-member
dependency: PR #319 (Sony `AFAreaModeSetting`/`AFPointSelected`).

## Design

### 1. Extraction: a Perl slice-and-eval script

`%afPoints51` and friends are Perl `my` lexicals — invisible to the
symbol-table walk `dump_tables.pl` already uses to extract ordinary tables.
This is the exact situation `docs/TRANSCRIPTION.md` documents for
`%fileTypeExt`: rather than retype the literal by hand, slice it out of the
`.pm` source and `eval` it with real Perl, with a hard error if the shape
changes.

New `tools/exiftool-tables/dump_af_points.pl`:
- Locates each `my %afPointsNNN = ( ... );` / `my @afPointsNNN = ( ... );`
  block in `Nikon.pm` by name (all ten), slices the literal text, and
  `eval`s it.
- Emits a single JSON file with all ten tables (hash tables as
  `{bit_number: name}`, array tables as an ordered list of names).
- Fails loudly (non-zero exit, no partial output) if any of the ten names
  isn't found — the same shape-changed guard `fileTypeExt`'s script uses.

A small Python codegen step (extending `codegen.py` or a new
`codegen_af_points.py`) turns that JSON into a committed Rust file:

`src/parsers/tiff/makernotes/nikon/af_points.rs`
- The seven hash-shaped tables as `const AF_POINTS_NNN: &[(u16, &str)]`,
  matching the `(u8, &str)` const style already used in `af_info2.rs`.
- The three array-shaped tables as `const AF_POINTS_NNN: &[&str]`.
- `just regen-tables`-style regeneration: re-running the two scripts against
  a fresh ExifTool checkout must reproduce this file byte-for-byte
  (`cargo fmt`-ed as part of generation, per the determinism rule in
  `TRANSCRIPTION.md`).

### 2. Print-conversion logic

Two pure functions in `af_points.rs` (or a `print_af_points` submodule),
direct ports of the two ExifTool subs — no inverse/write-side, since oxidex
doesn't write MakerNotes:

```rust
/// Port of Nikon.pm's PrintAFPoints (~line 13307): walk the bitmap, look
/// up bit-number+1 in `table`, comma-join, "(none)" if nothing is set.
fn print_af_points_lookup(bits: &[u8], table: &[(u16, &str)]) -> String

/// Same bitmap walk, but the point name is looked up in a positional array
/// (bit index, not bit-number+1) rather than a (number, name) table --
/// used for the three array-shaped tables above.
fn print_af_points_array(bits: &[u8], table: &[&str]) -> String

/// Port of PrintAFPointsGrid + GetAFPointGrid (~13361, 13378): walk the
/// bitmap, but *compute* the name from (row, col) instead of a lookup.
fn print_af_points_grid(bits: &[u8], ncols: u16) -> String
```

`%afPoints11` is a special case (`BITMASK` with a `0x7ff => 'All 11 Points'`
literal override) — it is not walked through `PrintAFPoints` in ExifTool at
all; it's a direct `PrintConv` hash on the raw `int16u` value with a
`BITMASK` sub-key. This one is transcribed as its own small dispatch in
`af_info2.rs` directly (it's only 11 named bits plus one literal), not
through `af_points.rs`.

### 3. Dispatch: extend `af_info2.rs`'s existing per-version `match`

Each version arm already exists in `parse_af_info2` and already tracks the
data members it needs. This adds the `AFPointsUsed`/`PrimaryAFPoint` reads
at their documented byte offsets, gated on the same `Condition` values
ExifTool uses:

- **V0100/V0101** (offset 8 for `AFPointsUsed`, elsewhere for
  `PrimaryAFPoint`): `FocusPointSchema==1` → 51-point
  (`print_af_points_lookup` + `AF_POINTS_51`), `==2` → `%afPoints11`
  BITMASK inline, `==3` → 39-point. `==0`/unclaimed → `"(none)"` literal
  (ExifTool still emits the tag with this fixed value; oxidex will match).
- **V0200/V0201**: needs `PhaseDetectAF` added as a tracked data member
  (currently read but not stored). `==4` → 135-point lookup
  (`AF_POINTS_135`), `==5` → grid-computed (ncols=15, older/"B-J" 135-point
  layout), `==6` → grid-computed (ncols=21, 171-point layout).
- **V0300/V0301**: gated on `FocusPointSchema` (`1`→51-point, `8`→81-point,
  `9`→105-point) **and** `AFCoordinatesAvailable==0` (when the block instead
  reports X/Y coordinates directly, `AFPointsUsed`/`PrimaryAFPoint` are not
  populated at all, matching ExifTool's Condition).
- **V0400/V0401/V0402**: gated on `AFAreaModeUsed` (`==197` Auto or `==207`
  3D-tracking) **and** the camera model string (Z8/Z9 → 405-point array,
  Z6III/Zf/Z5II → 299-point, Z50II → 231-point). This is the only place a
  `Model` string enters this file; I'll follow the Sony PR #319 precedent
  and thread it in via a small `AfInfo2Ctx { model: &str }` parameter to
  `parse_af_info2` rather than smuggling it through the `tags` map. Callers
  already have `Model` available (same pattern as PR #319's `MainCtx`).

### 4. Guard behavior preserved

All four documented `AFInfo2Version` families are now covered end to end;
the `_ => {}` fallback for a genuinely unclaimed version string is
unchanged. No body/version this design doesn't have a real ExifTool table
for gets a guessed grid.

### 5. Testing

- Unit tests per version arm (mirroring the existing `#[cfg(test)]` module):
  synthetic byte buffers exercising each `Condition` branch, asserting the
  exact ExifTool-format string (including the `"(none)"` and
  `"C6 (Center)"`-style central-point suffix cases).
- Corpus verification: `exiftool -G1 -s` vs `oxidex -e -s`, byte-for-byte,
  over the 54 files that carry either tag, classified per-file
  (matched/wrong/oxidex-only) as in PR #319's table, plus a full sweep of
  the ~307-file Nikon corpus for regressions (no previously-matched tag may
  regress).
- Gates: `cargo fmt --all`, `cargo clippy --workspace`,
  `cargo test --workspace`.

## Open risk

The 405/299/231-point grids (V0400 family) are not exercised by any sample
in the current local corpus — those sample files have `AFAreaModeUsed`
values that don't hit the `197`/`207` condition, so ExifTool itself reports
`(none)` for them today. The transcription is still mechanical and
verifiable (both ExifTool and oxidex will agree on `(none)` for these
particular files), but the interesting non-`(none)` branch for that family
is untested against a real byte-for-byte oracle file. This is called out
explicitly rather than silently claimed as verified.
