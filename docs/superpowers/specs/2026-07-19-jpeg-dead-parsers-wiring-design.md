# JPEG Dead-Parser Wiring: COM, SPIFF, DQT Quality, Multi-Chunk ICC

**Date:** 2026-07-19
**Status:** Awaiting review
**Scope:** Read path only (`parse_jpeg_metadata`)

## 1. Validation Findings

All four user-flagged items are confirmed dead code, verified by call-site
analysis (zero references outside their own unit tests) and empirically: a
synthetic JPEG containing COM, SPIFF, and DQT segments produces 34 tags from
oxidex today, none of them from those three segments.

| Item | Location | Status |
|---|---|---|
| COM comment parser | `parse_comment_segment`, `src/parsers/jpeg/app_parsers.rs:219` | Dead. Never dispatched for marker 0xFFFE. |
| SPIFF parser | `parse_spiff_segment`, `src/parsers/jpeg/app_parsers.rs:490` | Dead. Never dispatched for marker 0xFFE8. Also offset-incorrect vs ExifTool (see §4.2). |
| DQT quality estimation | `estimate_quality_from_dqt`, `src/parsers/jpeg/app_parsers.rs:240` | Dead. Never dispatched for marker 0xFFDB. Heuristic diverges from ExifTool (84 vs 87 on the same table). |
| Multi-chunk ICC | `IccChunkAssembler`, `src/parsers/jpeg/icc_chunk_assembler.rs` | Complete, tested, never instantiated. Live path `process_icc_segments` (`src/core/jpeg_helpers.rs:351`) drops multi-chunk profiles with an eprintln warning. |

Additional dead code found during validation (same file, zero external refs):

| Function | Assessment |
|---|---|
| `parse_icc_profile_segment` | Redundant duplicate — live path uses `parsers::icc::parse_icc_profile_data`. Delete. |
| `parse_adobe_segment` | Redundant duplicate of live `app_segments::parse_app14_adobe`. Delete. |
| `parse_activephoto_segment` | Speculative format; ExifTool's APP10 handling is PhotoStudio Unicode comments, not "ActivePhoto". Delete. |
| `parse_jpeg_ls_segment` | Speculative; JPEG-LS is signalled via SOF55, not an APP segment. Delete. |
| `process_app6_segments` stub | Stale TODO says "re-enable when parse_app6 is implemented" but `parse_app6` exists and is exported. **Resolved separately** — wired with ExifTool-parity GoPro GPMF support in commit 55a3c5c. |

Root cause: `parse_jpeg_metadata` (`src/core/operations.rs:476`) dispatches via
an explicit list of `process_*` helper calls in `src/core/jpeg_helpers.rs`;
these parsers were written but never got a `process_*` helper or dispatch line.

## 2. Goals

- Wire COM, SPIFF, and DQT quality estimation into `parse_jpeg_metadata`,
  with output matching ExifTool 13.55 (verified locally against
  `/opt/homebrew/bin/exiftool`).
- Reassemble multi-chunk ICC profiles using the existing `IccChunkAssembler`
  and parse them through the existing ICC pipeline.
- Delete the four redundant/speculative dead functions.
- Regression coverage in the established `production_wiring_tests.rs` style
  (synthetic in-memory fixtures, no binary files).

## 3. Non-Goals

- `JPEGDigest` (MD5 of DQT tables + large known-digest lookup DB) — follow-up.
- APP6/GoPro GPMF wiring — already landed separately (commit 55a3c5c).
- Write-path support for any of these segments.
- Post-SOS trailer parsing or segment-parser hardening (pre-existing behavior:
  `parse_segments` reads garbage pseudo-segments after SOS; unchanged here).

## 4. Design

Approach chosen: **extend the existing `process_*` pattern in
`jpeg_helpers.rs`** (one helper per segment family, called from
`parse_jpeg_metadata`), fixing parser internals for ExifTool parity while
wiring. Alternatives considered:

- *Wire the dead parsers verbatim* — cheapest, rejected: bakes in wrong SPIFF
  offsets, non-ExifTool tag names, and a divergent quality formula, so the
  coverage matrix would still show mismatches.
- *Marker→handler dispatch-table refactor* — more extensible, rejected: touches
  all working segment handling for no functional gain (YAGNI).

### 4.1 COM → `File:Comment`

New `process_com_segments` in `jpeg_helpers.rs`, dispatched for marker 0xFFFE.
Per ExifTool (`ExifTool.pm` COM handler):

- Strip trailing NUL bytes (`s/\0+$//`).
- Valid UTF-8 → `TagValue::String`; otherwise `TagValue::Binary`.
- Key: `File:Comment` (ExifTool family-1 group is `File`; the current dead
  parser's `JPEG:Comment` key is wrong for parity).
- Multiple COM segments: last one wins (`MetadataMap` is last-wins; ExifTool
  shows duplicates only under `-a`). Documented in `KNOWN_DISCREPANCIES.md`.

The old `parse_comment_segment` body is replaced by this logic (fixed key,
NUL-stripping added).

### 4.2 APP8 SPIFF → `SPIFF:*`

The dead parser does not match ExifTool and is rewritten. ExifTool
(`ExifTool.pm:8221`, `JPEG.pm` `%SPIFF` table) processes APP8 as SPIFF only
when the payload starts with `SPIFF\0` **and is exactly 32 bytes** (real-world
v1.2 samples have 2 pad bytes the spec lacks; ExifTool's offsets follow the
samples, and empirically it ignores a 30-byte spec-shaped payload).

Payload layout, offsets relative to the byte after the 6-byte identifier:

| Off | Size | Tag | Conversion |
|---|---|---|---|
| 0 | 2 | `SPIFF:SPIFFVersion` | `"{major}.{minor}"` string |
| 2 | 1 | `SPIFF:ProfileID` | 0 Not Specified, 1 Continuous-tone Base, 2 Continuous-tone Progressive, 3 Bi-level Facsimile, 4 Continuous-tone Facsimile |
| 3 | 1 | `SPIFF:ColorComponents` | integer |
| 6 | 4 | `SPIFF:ImageHeight` | u32 BE |
| 10 | 4 | `SPIFF:ImageWidth` | u32 BE |
| 14 | 1 | `SPIFF:ColorSpace` | 0 Bi-level, 1 YCbCr ITU-R BT 709 video, 2 No color space specified, 3 YCbCr ITU-R BT 601-1 RGB, 4 YCbCr ITU-R BT 601-1 video, 8 Gray-scale, 9 PhotoYCC, 10 RGB, 11 CMY, 12 CMYK, 13 YCCK, 14 CIELab |
| 15 | 1 | `SPIFF:BitsPerSample` | integer |
| 16 | 1 | `SPIFF:Compression` | 0 Uncompressed interleaved 8 bits per sample, 1 Modified Huffman, 2 Modified READ, 3 Modified Modified READ, 4 JBIG, 5 JPEG |
| 17 | 1 | `SPIFF:ResolutionUnit` | 0 None, 1 inches, 2 cm |
| 18 | 4 | `SPIFF:YResolution` | u32 BE |
| 22 | 4 | `SPIFF:XResolution` | u32 BE |

Unknown enum values render as `Unknown (N)`, matching ExifTool's PrintConv-miss
format. New `process_spiff_segments` dispatches marker 0xFFE8. The old
`APP8:SPIFF*` keys and spec-offset reads are removed.

### 4.3 DQT → `File:JPEGQualityEstimate`

Replace the ad-hoc heuristic with a port of ExifTool's `EstimateQuality`
(`JPEGDigest.pm`, derived from ImageMagick `coders/jpeg.c`):

- Collect DQT segments (marker 0xFFDB) indexed by table id = first byte & 0x0F,
  keeping whole segment payloads, ids ≥ 4 ignored (`ExifTool.pm:7668`).
- Walk each kept payload in 65-byte strides (1 precision/id byte + 64 8-bit
  values), max 4 tables total; sum all values.
- `qval = t0[2] + t0[53]`, plus `t1[0] + t1[63]` when ≥ 2 tables.
- Compare against the color (≥ 2 tables) or greyscale hash/sums arrays copied
  verbatim from ExifTool; first index where thresholds pass yields quality
  1–100 (`i >= 50` fallback rule included).
- Emit `File:JPEGQualityEstimate` as `TagValue::Integer`; omit the tag when the
  algorithm returns none (e.g. no valid tables).

Divergence, documented in `KNOWN_DISCREPANCIES.md`: ExifTool computes this tag
only when explicitly requested (`-JPEGQualityEstimate` / `RequestAll > 2`);
oxidex has no tag-request infrastructure, and the computation is trivial in
Rust, so oxidex always emits it. Parity anchor: all-16s 8-bit table + 3-component
SOF → quality 87 (verified against ExifTool 13.55; the old heuristic said 84).

The old `estimate_quality_from_dqt` and its `JPEG:EstimatedQuality` key are
removed.

### 4.4 Multi-chunk ICC reassembly

Rework `process_icc_segments` (`jpeg_helpers.rs:321`) to two-phase:

1. Collect all APP2 segments whose payload starts with `ICC_PROFILE\0`.
2. Single chunk (1 of 1): parse directly, unchanged fast path (no copy).
   Multi-chunk: feed every payload into `IccChunkAssembler::add_chunk`; when
   `is_complete()`, `assemble()` and hand the result to the existing
   `parsers::icc::parse_icc_profile_data` → `ICC_Profile:*` keys, identical to
   the single-chunk path.
3. Assembler errors or an incomplete chunk set degrade to the existing
   warn-and-continue behavior (`eprintln!`), never a hard failure. The
   "not yet supported" warning is deleted.

This finally gives `IccChunkAssembler` its call site; its unit tests already
cover ordering, duplicates, and inconsistent totals.

### 4.5 Cleanup

Delete `parse_icc_profile_segment`, `parse_adobe_segment`,
`parse_activephoto_segment`, `parse_jpeg_ls_segment`, and their in-module
tests. Leave the APP6 stub and `segment_parser.rs`'s `#![allow(dead_code)]`
untouched (out of scope).

## 5. Error Handling

All new `process_*` helpers follow the established pattern: per-segment parse
errors are logged via `eprintln!` warnings and never abort
`parse_jpeg_metadata`. Malformed SPIFF (wrong length/identifier) and DQT
(short tables) segments are skipped silently, matching ExifTool, which simply
extracts nothing for them.

## 6. Testing

- **Unit tests** (in-module, existing style): COM NUL-stripping, non-UTF-8
  binary fallback; SPIFF 32-byte gate (30-byte payload → no tags), enum
  conversions; quality estimation exact values for known tables (color 87,
  greyscale variant, `i >= 50` fallback); existing assembler tests unchanged.
- **Integration tests** (`tests/integration/production_wiring_tests.rs`
  style, synthetic in-memory JPEG builders): a JPEG with COM + 32-byte SPIFF +
  DQT + SOF0 asserting `File:Comment`, all 11 `SPIFF:*` tags, and
  `File:JPEGQualityEstimate == 87`; a JPEG with a minimal ICC profile split
  across two APP2 chunks (including out-of-order delivery) asserting the same
  `ICC_Profile:*` tags as the single-chunk equivalent.
- **Parity check:** fixture bytes were validated against ExifTool 13.55 during
  design; expected values in tests are ExifTool's actual output.

## 7. Decisions Made Autonomously (flag for review)

1. **Parity-first rewrite** of SPIFF/DQT internals rather than wiring verbatim.
2. **Always emit** `File:JPEGQualityEstimate` (no request-gating infra exists).
3. **Delete** the four redundant/speculative parsers instead of wiring them.
4. **APP6/GPMF and JPEGDigest deferred** to follow-up work.
5. Multiple COM segments: **last wins** (MetadataMap constraint).
