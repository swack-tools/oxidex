# JPEG Dead-Parser Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the dead JPEG COM, SPIFF, and DQT-quality parsers plus multi-chunk ICC reassembly into `parse_jpeg_metadata` with ExifTool-13.55 parity, and delete four orphaned parser functions.

**Architecture:** Each JPEG segment family gets a `process_*` helper in `src/core/jpeg_helpers.rs` dispatched from `parse_jpeg_metadata` in `src/core/operations.rs` (existing pattern). Parser internals live in `src/parsers/jpeg/`. The DQT quality algorithm is a verbatim port of ExifTool's `EstimateQuality` (JPEGDigest.pm, derived from ImageMagick).

**Tech Stack:** Rust, no new dependencies. Tests: in-module unit tests + `tests/integration/production_wiring_tests.rs` synthetic-fixture style.

**Spec:** `docs/superpowers/specs/2026-07-19-jpeg-dead-parsers-wiring-design.md`

## Global Constraints

- Tag keys are exact: `File:Comment`, `SPIFF:SPIFFVersion`, `SPIFF:ProfileID`, `SPIFF:ColorComponents`, `SPIFF:ImageHeight`, `SPIFF:ImageWidth`, `SPIFF:ColorSpace`, `SPIFF:BitsPerSample`, `SPIFF:Compression`, `SPIFF:ResolutionUnit`, `SPIFF:YResolution`, `SPIFF:XResolution`, `File:JPEGQualityEstimate`, `ICC_Profile:*`.
- Parity anchor values were captured from ExifTool 13.55 (`/opt/homebrew/bin/exiftool`); do not "fix" them to look nicer.
- Segment parse errors never abort `parse_jpeg_metadata`: warn via `eprintln!` and continue (existing pattern).
- SPIFF is processed only when the APP8 payload is exactly 32 bytes and starts with `SPIFF\0` (ExifTool gate).
- Unknown enum values render as `Unknown (N)`.
- Every task ends with `cargo test` green and a commit. Run `cargo fmt` and `cargo clippy` before each commit.

---

### Task 1: COM comment → `File:Comment`

**Files:**
- Modify: `src/parsers/jpeg/app_parsers.rs` (rewrite `parse_comment_segment` ~line 219, and its test `test_parse_comment_segment` ~line 755)
- Modify: `src/core/jpeg_helpers.rs` (add `process_com_segments`)
- Modify: `src/core/operations.rs` (import + dispatch, ~lines 9–13 and ~489)
- Test: `tests/integration/production_wiring_tests.rs`

**Interfaces:**
- Consumes: `Segment { marker: u16, data: &[u8] }` from `crate::parsers::jpeg::segment_parser`; `MetadataMap::insert`, `TagValue::{String, Binary}`.
- Produces: `pub fn parse_comment_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String>` (key changes to `File:Comment`); `pub fn process_com_segments(segments: &[Segment], metadata: &mut MetadataMap)` in `jpeg_helpers`.

- [ ] **Step 1: Replace the unit test for the new behavior**

In `src/parsers/jpeg/app_parsers.rs`, replace the existing `test_parse_comment_segment` with:

```rust
    #[test]
    fn test_parse_comment_segment() {
        let mut metadata = MetadataMap::new();
        // Trailing NULs are stripped, matching ExifTool's COM handler
        let result = parse_comment_segment(b"Hello JPEG\0\0", &mut metadata);
        assert!(result.is_ok());
        assert_eq!(metadata.get_string("File:Comment"), Some("Hello JPEG"));
    }

    #[test]
    fn test_parse_comment_segment_binary_fallback() {
        let mut metadata = MetadataMap::new();
        let result = parse_comment_segment(&[0xFF, 0xFE, 0x00, 0x41], &mut metadata);
        assert!(result.is_ok());
        assert_eq!(
            metadata.get("File:Comment"),
            Some(&TagValue::Binary(vec![0xFF, 0xFE, 0x00, 0x41]))
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oxidex --lib parse_comment -- --nocapture`
Expected: FAIL — old implementation inserts `JPEG:Comment` and does not strip NULs.

- [ ] **Step 3: Rewrite `parse_comment_segment`**

Replace the function body at `src/parsers/jpeg/app_parsers.rs:219`:

```rust
/// Parse JPEG Comment segment (COM, marker 0xFFFE)
///
/// ExifTool exposes COM data as the File:Comment tag and strips trailing NUL
/// bytes ("some dumb softwares add null terminators" — ExifTool.pm COM handler).
pub fn parse_comment_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
    let trimmed = &data[..end];
    match std::str::from_utf8(trimmed) {
        Ok(comment) => {
            metadata.insert(
                "File:Comment".to_string(),
                TagValue::String(comment.to_string()),
            );
        }
        Err(_) => {
            metadata.insert(
                "File:Comment".to_string(),
                TagValue::Binary(trimmed.to_vec()),
            );
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p oxidex --lib parse_comment`
Expected: PASS (2 tests).

- [ ] **Step 5: Add the failing integration test**

`tests/integration/production_wiring_tests.rs` already contains the JPEG
fixture helpers `jpeg_segment`, `jpeg_with_segments`, `sof0_payload`, and
`gpmf_record` (added with the APP6 wiring, commit 55a3c5c) — do NOT re-add
them. Add only the test:

```rust
#[test]
fn jpeg_com_segment_yields_file_comment() {
    let jpeg = jpeg_with_segments(&[
        jpeg_segment(0xFE, b"Test comment\0\0"),
        jpeg_segment(0xC0, &sof0_payload()),
    ]);
    let metadata = read_temp_file(&jpeg, ".jpg");
    assert_eq!(metadata.get_string("File:Comment"), Some("Test comment"));
}
```

- [ ] **Step 6: Run integration test to verify it fails**

Run: `cargo test --test integration jpeg_com_segment_yields_file_comment`
Expected: FAIL — COM is never dispatched, tag absent.
(If the integration harness entry differs, use `cargo test jpeg_com_segment_yields_file_comment`.)

- [ ] **Step 7: Add `process_com_segments` and dispatch it**

Append to `src/core/jpeg_helpers.rs`:

```rust
/// Processes JPEG COM (comment) segments.
///
/// COM segments (marker 0xFFFE) carry free-form comment text. ExifTool exposes
/// them as File:Comment with trailing NULs stripped; when several COM segments
/// are present the last one wins (MetadataMap holds one value per key).
pub fn process_com_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const COM_MARKER: u16 = 0xFFFE;
    for segment in segments.iter().filter(|s| s.marker == COM_MARKER) {
        let _ =
            crate::parsers::jpeg::app_parsers::parse_comment_segment(segment.data, metadata);
    }
}
```

In `src/core/operations.rs` extend the import at line 9:

```rust
use crate::core::jpeg_helpers::{
    process_app10_segments, process_app11_segments, process_app12_segments, process_app14_segments,
    process_com_segments, process_exif_segments, process_icc_segments, process_iptc_segments,
    process_jfif_segments, process_mpf_segments, process_sof_segments, process_xmp_segments,
};
```

and in `parse_jpeg_metadata` add after `process_sof_segments(&segments, &mut metadata);`:

```rust
    process_com_segments(&segments, &mut metadata);
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test jpeg_com_segment_yields_file_comment` and `cargo test -p oxidex --lib parse_comment`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
cargo fmt && cargo clippy --quiet 2>/dev/null; git add -A src tests
git commit -m "feat(jpeg): wire COM segment parsing to File:Comment"
```

---

### Task 2: DQT → `File:JPEGQualityEstimate`

**Files:**
- Create: `src/parsers/jpeg/quality_estimate.rs`
- Modify: `src/parsers/jpeg/mod.rs` (add `pub mod quality_estimate;`)
- Modify: `src/parsers/jpeg/app_parsers.rs` (delete `estimate_quality_from_dqt` ~line 240 and test `test_estimate_quality_high` ~line 769; drop `DQT` from the module doc comment)
- Modify: `src/core/jpeg_helpers.rs` (add `process_dqt_segments`)
- Modify: `src/core/operations.rs` (import + dispatch)
- Test: `tests/integration/production_wiring_tests.rs`

**Interfaces:**
- Consumes: `Segment`, `MetadataMap::insert`, `TagValue::Integer`.
- Produces: `pub fn estimate_quality_from_dqt_tables(dqt_list: &[Option<&[u8]>]) -> Option<i64>` in `quality_estimate.rs`; `pub fn process_dqt_segments(segments: &[Segment], metadata: &mut MetadataMap)` in `jpeg_helpers`.

- [ ] **Step 1: Create the module with failing unit tests**

Create `src/parsers/jpeg/quality_estimate.rs`:

```rust
//! JPEG quality estimation from DQT quantization tables.
//!
//! Verbatim port of ExifTool's EstimateQuality (JPEGDigest.pm v1.06), itself
//! derived from ImageMagick coders/jpeg.c. Inputs are raw DQT segment payloads
//! (marker 0xFFDB) indexed by table id (first byte & 0x0F, ids 0-3), matching
//! how ExifTool's ProcessJPEG collects them. Each payload is walked in 65-byte
//! strides (1 precision/id byte + 64 8-bit values), up to 4 tables total.

/// Threshold tables for color images (>= 2 quantization tables), from
/// ExifTool JPEGDigest.pm / ImageMagick. Index i corresponds to quality i+1.
const COLOR_HASH: [u32; 100] = [
    1020, 1015, 932, 848, 780, 735, 702, 679, 660, 645, //
    632, 623, 613, 607, 600, 594, 589, 585, 581, 571, //
    555, 542, 529, 514, 494, 474, 457, 439, 424, 410, //
    397, 386, 373, 364, 351, 341, 334, 324, 317, 309, //
    299, 294, 287, 279, 274, 267, 262, 257, 251, 247, //
    243, 237, 232, 227, 222, 217, 213, 207, 202, 198, //
    192, 188, 183, 177, 173, 168, 163, 157, 153, 148, //
    143, 139, 132, 128, 125, 119, 115, 108, 104, 99, //
    94, 90, 84, 79, 74, 70, 64, 59, 55, 49, //
    45, 40, 34, 30, 25, 20, 15, 11, 6, 4,
];

const COLOR_SUMS: [u32; 100] = [
    32640, 32635, 32266, 31495, 30665, 29804, 29146, 28599, 28104, 27670, //
    27225, 26725, 26210, 25716, 25240, 24789, 24373, 23946, 23572, 22846, //
    21801, 20842, 19949, 19121, 18386, 17651, 16998, 16349, 15800, 15247, //
    14783, 14321, 13859, 13535, 13081, 12702, 12423, 12056, 11779, 11513, //
    11135, 10955, 10676, 10392, 10208, 9928, 9747, 9564, 9369, 9193, //
    9017, 8822, 8639, 8458, 8270, 8084, 7896, 7710, 7527, 7347, //
    7156, 6977, 6788, 6607, 6422, 6236, 6054, 5867, 5684, 5495, //
    5305, 5128, 4945, 4751, 4638, 4442, 4248, 4065, 3888, 3698, //
    3509, 3326, 3139, 2957, 2775, 2586, 2405, 2216, 2037, 1846, //
    1666, 1483, 1297, 1109, 927, 735, 554, 375, 201, 128,
];

/// Threshold tables for greyscale images (single quantization table).
const GRAY_HASH: [u32; 100] = [
    510, 505, 422, 380, 355, 338, 326, 318, 311, 305, //
    300, 297, 293, 291, 288, 286, 284, 283, 281, 280, //
    279, 278, 277, 273, 262, 251, 243, 233, 225, 218, //
    211, 205, 198, 193, 186, 181, 177, 172, 168, 164, //
    158, 156, 152, 148, 145, 142, 139, 136, 133, 131, //
    129, 126, 123, 120, 118, 115, 113, 110, 107, 105, //
    102, 100, 97, 94, 92, 89, 87, 83, 81, 79, //
    76, 74, 70, 68, 66, 63, 61, 57, 55, 52, //
    50, 48, 44, 42, 39, 37, 34, 31, 29, 26, //
    24, 21, 18, 16, 13, 11, 8, 6, 3, 2,
];

const GRAY_SUMS: [u32; 100] = [
    16320, 16315, 15946, 15277, 14655, 14073, 13623, 13230, 12859, 12560, //
    12240, 11861, 11456, 11081, 10714, 10360, 10027, 9679, 9368, 9056, //
    8680, 8331, 7995, 7668, 7376, 7084, 6823, 6562, 6345, 6125, //
    5939, 5756, 5571, 5421, 5240, 5086, 4976, 4829, 4719, 4616, //
    4463, 4393, 4280, 4166, 4092, 3980, 3909, 3835, 3755, 3688, //
    3621, 3541, 3467, 3396, 3323, 3247, 3170, 3096, 3021, 2952, //
    2874, 2804, 2727, 2657, 2583, 2509, 2437, 2362, 2290, 2211, //
    2136, 2068, 1996, 1915, 1858, 1773, 1692, 1620, 1552, 1477, //
    1398, 1326, 1251, 1179, 1109, 1031, 961, 884, 814, 736, //
    667, 592, 518, 441, 369, 292, 221, 151, 86, 64,
];

/// Estimates JPEG quality (1-100) from DQT segment payloads.
///
/// `dqt_list` holds raw DQT payloads indexed by table id; `None` entries are
/// skipped. Returns `None` when no table is present or the thresholds reject
/// the values (mirroring ExifTool returning undef).
pub fn estimate_quality_from_dqt_tables(dqt_list: &[Option<&[u8]>]) -> Option<i64> {
    let mut qtbl: Vec<&[u8]> = Vec::new();
    let mut sum: u32 = 0;

    'dqt: for dqt in dqt_list.iter().flatten() {
        let mut i = 1;
        while i + 64 <= dqt.len() {
            let qt = &dqt[i..i + 64];
            sum += qt.iter().map(|&v| v as u32).sum::<u32>();
            qtbl.push(qt);
            if qtbl.len() >= 4 {
                break 'dqt;
            }
            i += 65;
        }
    }

    if qtbl.is_empty() {
        return None;
    }

    let mut qval = qtbl[0][2] as u32 + qtbl[0][53] as u32;
    let (hash, sums) = if qtbl.len() > 1 {
        // color JPEG
        qval += qtbl[1][0] as u32 + qtbl[1][63] as u32;
        (&COLOR_HASH, &COLOR_SUMS)
    } else {
        // greyscale JPEG
        (&GRAY_HASH, &GRAY_SUMS)
    };

    for i in 0..100 {
        if qval < hash[i] && sum < sums[i] {
            continue;
        }
        if (qval <= hash[i] && sum <= sums[i]) || i >= 50 {
            return Some((i + 1) as i64);
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dqt(table_id: u8, value: u8) -> Vec<u8> {
        let mut d = vec![table_id];
        d.extend_from_slice(&[value; 64]);
        d
    }

    #[test]
    fn test_greyscale_all_16_matches_exiftool() {
        // ExifTool 13.55: -JPEGQualityEstimate => 87 for a single all-16 table
        let t0 = dqt(0, 16);
        let list = [Some(t0.as_slice()), None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), Some(87));
    }

    #[test]
    fn test_color_two_tables() {
        // qval = 16+16+17+17 = 66, sum = 64*16 + 64*17 = 2112 -> quality 87
        let t0 = dqt(0, 16);
        let t1 = dqt(1, 17);
        let list = [Some(t0.as_slice()), Some(t1.as_slice()), None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), Some(87));
    }

    #[test]
    fn test_highest_quality_table() {
        // All-1s table: qval = 2, sum = 64 -> exact match at index 99 -> 100
        let t0 = dqt(0, 1);
        let list = [Some(t0.as_slice()), None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), Some(100));
    }

    #[test]
    fn test_no_tables_returns_none() {
        let list: [Option<&[u8]>; 4] = [None, None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), None);
    }

    #[test]
    fn test_short_segment_ignored() {
        let short = [0u8; 10];
        let list = [Some(&short[..]), None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), None);
    }

    #[test]
    fn test_one_segment_two_tables_is_color() {
        // A single DQT segment may carry several 65-byte tables back to back.
        let mut seg = dqt(0, 16);
        seg.extend_from_slice(&dqt(1, 17));
        let list = [Some(seg.as_slice()), None, None, None];
        assert_eq!(estimate_quality_from_dqt_tables(&list), Some(87));
    }
}
```

Register the module in `src/parsers/jpeg/mod.rs` next to the other `pub mod` lines:

```rust
pub mod quality_estimate;
```

- [ ] **Step 2: Run unit tests, verify they pass (pure function, no wiring yet)**

Run: `cargo test -p oxidex --lib quality_estimate`
Expected: PASS (6 tests). These lock in the ExifTool anchors before wiring.

- [ ] **Step 3: Add the failing integration test**

Add to `tests/integration/production_wiring_tests.rs`:

```rust
fn dqt_payload() -> Vec<u8> {
    // 8-bit precision, table id 0, all values 16
    let mut p = vec![0x00];
    p.extend_from_slice(&[16u8; 64]);
    p
}

#[test]
fn jpeg_dqt_yields_exiftool_quality_estimate() {
    let jpeg = jpeg_with_segments(&[
        jpeg_segment(0xDB, &dqt_payload()),
        jpeg_segment(0xC0, &sof0_payload()),
    ]);
    let metadata = read_temp_file(&jpeg, ".jpg");
    // ExifTool 13.55 reports 87 for this quantization table
    assert_eq!(metadata.get_integer("File:JPEGQualityEstimate"), Some(87));
}
```

Run: `cargo test jpeg_dqt_yields_exiftool_quality_estimate`
Expected: FAIL — DQT is never dispatched.

- [ ] **Step 4: Wire `process_dqt_segments`**

Append to `src/core/jpeg_helpers.rs`:

```rust
/// Processes DQT (Define Quantization Table) segments into a quality estimate.
///
/// Collects DQT payloads indexed by table id (first byte & 0x0F, ids 0-3,
/// later segments overwrite earlier ones — ExifTool.pm DQT handler) and emits
/// File:JPEGQualityEstimate. ExifTool computes this tag only when explicitly
/// requested; oxidex has no tag-request mechanism and always emits it (see
/// tests/integration/KNOWN_DISCREPANCIES.md).
pub fn process_dqt_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const DQT_MARKER: u16 = 0xFFDB;
    let mut dqt_list: [Option<&[u8]>; 4] = [None, None, None, None];
    for segment in segments.iter().filter(|s| s.marker == DQT_MARKER) {
        if segment.data.is_empty() {
            continue;
        }
        let table_id = (segment.data[0] & 0x0F) as usize;
        if table_id < 4 {
            dqt_list[table_id] = Some(segment.data);
        }
    }
    if let Some(quality) = estimate_quality_from_dqt_tables(&dqt_list) {
        metadata.insert(
            "File:JPEGQualityEstimate".to_string(),
            TagValue::Integer(quality),
        );
    }
}
```

Add the import at the top of `jpeg_helpers.rs`:

```rust
use crate::parsers::jpeg::quality_estimate::estimate_quality_from_dqt_tables;
```

In `src/core/operations.rs`: add `process_dqt_segments` to the `jpeg_helpers` import list, and in `parse_jpeg_metadata` add after the `process_com_segments` line:

```rust
    process_dqt_segments(&segments, &mut metadata);
```

- [ ] **Step 5: Delete the superseded heuristic**

In `src/parsers/jpeg/app_parsers.rs`: delete `estimate_quality_from_dqt` (~lines 237–278) and its test `test_estimate_quality_high` (~line 769). Remove `- DQT: Quantization tables (for quality estimation)` from the module doc comment.

- [ ] **Step 6: Run tests**

Run: `cargo test jpeg_dqt_yields_exiftool_quality_estimate && cargo test -p oxidex --lib quality_estimate && cargo build`
Expected: PASS; build confirms no dangling references to the deleted function.

- [ ] **Step 7: Commit**

```bash
cargo fmt && git add -A src tests
git commit -m "feat(jpeg): port ExifTool quality estimation and wire DQT segments"
```

---

### Task 3: APP8 SPIFF → `SPIFF:*`

**Files:**
- Modify: `src/parsers/jpeg/app_parsers.rs` (rewrite `parse_spiff_segment` ~line 490; replace tests `test_parse_spiff_segment` ~line 817 and `test_parse_spiff_with_dimensions` ~line 847)
- Modify: `src/core/jpeg_helpers.rs` (add `process_spiff_segments`)
- Modify: `src/core/operations.rs` (import + dispatch)
- Test: `tests/integration/production_wiring_tests.rs`

**Interfaces:**
- Consumes: `Segment`, `EndianReader::big_endian`, `MetadataMap`, `TagValue::{String, Integer}`.
- Produces: `pub fn parse_spiff_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String>` (32-byte gate, `SPIFF:*` keys); `pub fn process_spiff_segments(segments: &[Segment], metadata: &mut MetadataMap)`.

- [ ] **Step 1: Replace the SPIFF unit tests**

In `src/parsers/jpeg/app_parsers.rs`, replace `test_parse_spiff_segment` and `test_parse_spiff_with_dimensions` with:

```rust
    /// Builds the 32-byte APP8 SPIFF payload ExifTool recognizes
    /// (identifier + version + profile + components + 2 pad bytes +
    /// dimensions + colorspace/bps/compression/unit + resolutions).
    fn spiff_payload_32() -> Vec<u8> {
        let mut p = b"SPIFF\0".to_vec();
        p.extend_from_slice(&[1, 0]); // version 1.0
        p.push(1); // ProfileID: Continuous-tone Base
        p.push(3); // 3 color components
        p.extend_from_slice(&[0, 0]); // pad bytes seen in real v1.2 samples
        p.extend_from_slice(&480u32.to_be_bytes()); // height
        p.extend_from_slice(&640u32.to_be_bytes()); // width
        p.extend_from_slice(&[3, 8, 5, 1]); // BT601 RGB, 8 bits, JPEG, inches
        p.extend_from_slice(&72u32.to_be_bytes()); // Y resolution
        p.extend_from_slice(&72u32.to_be_bytes()); // X resolution
        assert_eq!(p.len(), 32);
        p
    }

    #[test]
    fn test_parse_spiff_segment_full() {
        let mut metadata = MetadataMap::new();
        let result = parse_spiff_segment(&spiff_payload_32(), &mut metadata);
        assert!(result.is_ok());
        assert_eq!(metadata.get_string("SPIFF:SPIFFVersion"), Some("1.0"));
        assert_eq!(
            metadata.get_string("SPIFF:ProfileID"),
            Some("Continuous-tone Base")
        );
        assert_eq!(metadata.get_integer("SPIFF:ColorComponents"), Some(3));
        assert_eq!(metadata.get_integer("SPIFF:ImageHeight"), Some(480));
        assert_eq!(metadata.get_integer("SPIFF:ImageWidth"), Some(640));
        assert_eq!(
            metadata.get_string("SPIFF:ColorSpace"),
            Some("YCbCr, ITU-R BT 601-1, RGB")
        );
        assert_eq!(metadata.get_integer("SPIFF:BitsPerSample"), Some(8));
        assert_eq!(metadata.get_string("SPIFF:Compression"), Some("JPEG"));
        assert_eq!(metadata.get_string("SPIFF:ResolutionUnit"), Some("inches"));
        assert_eq!(metadata.get_integer("SPIFF:YResolution"), Some(72));
        assert_eq!(metadata.get_integer("SPIFF:XResolution"), Some(72));
    }

    #[test]
    fn test_parse_spiff_segment_rejects_non_32_byte_payload() {
        // ExifTool only recognizes 32-byte SPIFF payloads; a 30-byte
        // spec-shaped payload must extract nothing.
        let mut payload = spiff_payload_32();
        payload.truncate(30);
        let mut metadata = MetadataMap::new();
        assert!(parse_spiff_segment(&payload, &mut metadata).is_err());
        assert!(metadata.get("SPIFF:SPIFFVersion").is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oxidex --lib parse_spiff`
Expected: FAIL — old parser uses `APP8:` keys, spec offsets, no 32-byte gate.

- [ ] **Step 3: Rewrite `parse_spiff_segment`**

Replace the function at `src/parsers/jpeg/app_parsers.rs:490`:

```rust
/// Parse APP8 (SPIFF) segment
///
/// SPIFF (Still Picture Interchange File Format, ISO/IEC 10918-3) stores basic
/// image parameters in the first APP8 segment. ExifTool processes APP8 as
/// SPIFF only when the payload starts with "SPIFF\0" AND is exactly 32 bytes;
/// real-world v1.2 samples carry 2 pad bytes after ColorComponents that the
/// spec does not mention, and the offsets below follow those samples
/// (ExifTool JPEG.pm %SPIFF table).
pub fn parse_spiff_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.len() != 32 {
        return Err(format!(
            "APP8 SPIFF payload must be 32 bytes, got {}",
            data.len()
        ));
    }
    if &data[0..6] != b"SPIFF\0" {
        return Err("Invalid SPIFF identifier".to_string());
    }

    // Offsets are relative to the byte after the 6-byte identifier.
    let body = &data[6..];
    let reader = EndianReader::big_endian(body);

    metadata.insert(
        "SPIFF:SPIFFVersion".to_string(),
        TagValue::String(format!("{}.{}", body[0], body[1])),
    );

    let profile_id = match body[2] {
        0 => "Not Specified".to_string(),
        1 => "Continuous-tone Base".to_string(),
        2 => "Continuous-tone Progressive".to_string(),
        3 => "Bi-level Facsimile".to_string(),
        4 => "Continuous-tone Facsimile".to_string(),
        other => format!("Unknown ({})", other),
    };
    metadata.insert("SPIFF:ProfileID".to_string(), TagValue::String(profile_id));

    metadata.insert(
        "SPIFF:ColorComponents".to_string(),
        TagValue::Integer(body[3] as i64),
    );

    metadata.insert(
        "SPIFF:ImageHeight".to_string(),
        TagValue::Integer(reader.u32_at(6).unwrap_or(0) as i64),
    );
    metadata.insert(
        "SPIFF:ImageWidth".to_string(),
        TagValue::Integer(reader.u32_at(10).unwrap_or(0) as i64),
    );

    let color_space = match body[14] {
        0 => "Bi-level".to_string(),
        1 => "YCbCr, ITU-R BT 709, video".to_string(),
        2 => "No color space specified".to_string(),
        3 => "YCbCr, ITU-R BT 601-1, RGB".to_string(),
        4 => "YCbCr, ITU-R BT 601-1, video".to_string(),
        8 => "Gray-scale".to_string(),
        9 => "PhotoYCC".to_string(),
        10 => "RGB".to_string(),
        11 => "CMY".to_string(),
        12 => "CMYK".to_string(),
        13 => "YCCK".to_string(),
        14 => "CIELab".to_string(),
        other => format!("Unknown ({})", other),
    };
    metadata.insert("SPIFF:ColorSpace".to_string(), TagValue::String(color_space));

    metadata.insert(
        "SPIFF:BitsPerSample".to_string(),
        TagValue::Integer(body[15] as i64),
    );

    let compression = match body[16] {
        0 => "Uncompressed, interleaved, 8 bits per sample".to_string(),
        1 => "Modified Huffman".to_string(),
        2 => "Modified READ".to_string(),
        3 => "Modified Modified READ".to_string(),
        4 => "JBIG".to_string(),
        5 => "JPEG".to_string(),
        other => format!("Unknown ({})", other),
    };
    metadata.insert(
        "SPIFF:Compression".to_string(),
        TagValue::String(compression),
    );

    let resolution_unit = match body[17] {
        0 => "None".to_string(),
        1 => "inches".to_string(),
        2 => "cm".to_string(),
        other => format!("Unknown ({})", other),
    };
    metadata.insert(
        "SPIFF:ResolutionUnit".to_string(),
        TagValue::String(resolution_unit),
    );

    metadata.insert(
        "SPIFF:YResolution".to_string(),
        TagValue::Integer(reader.u32_at(18).unwrap_or(0) as i64),
    );
    metadata.insert(
        "SPIFF:XResolution".to_string(),
        TagValue::Integer(reader.u32_at(22).unwrap_or(0) as i64),
    );

    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p oxidex --lib parse_spiff`
Expected: PASS (2 tests).

- [ ] **Step 5: Add the failing integration test**

Add to `tests/integration/production_wiring_tests.rs`:

```rust
fn spiff_payload() -> Vec<u8> {
    let mut p = b"SPIFF\0".to_vec();
    p.extend_from_slice(&[1, 0]);
    p.push(1);
    p.push(3);
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(&480u32.to_be_bytes());
    p.extend_from_slice(&640u32.to_be_bytes());
    p.extend_from_slice(&[3, 8, 5, 1]);
    p.extend_from_slice(&72u32.to_be_bytes());
    p.extend_from_slice(&72u32.to_be_bytes());
    assert_eq!(p.len(), 32);
    p
}

#[test]
fn jpeg_spiff_segment_yields_spiff_tags() {
    let jpeg = jpeg_with_segments(&[
        jpeg_segment(0xE8, &spiff_payload()),
        jpeg_segment(0xC0, &sof0_payload()),
    ]);
    let metadata = read_temp_file(&jpeg, ".jpg");
    assert_eq!(metadata.get_string("SPIFF:SPIFFVersion"), Some("1.0"));
    assert_eq!(metadata.get_integer("SPIFF:ImageWidth"), Some(640));
    assert_eq!(metadata.get_string("SPIFF:Compression"), Some("JPEG"));
}

#[test]
fn jpeg_spiff_segment_wrong_length_is_ignored() {
    let mut short = spiff_payload();
    short.truncate(30);
    let jpeg = jpeg_with_segments(&[
        jpeg_segment(0xE8, &short),
        jpeg_segment(0xC0, &sof0_payload()),
    ]);
    let metadata = read_temp_file(&jpeg, ".jpg");
    assert!(metadata.get("SPIFF:SPIFFVersion").is_none());
}
```

Run: `cargo test jpeg_spiff`
Expected: `jpeg_spiff_segment_yields_spiff_tags` FAILS (not dispatched); the wrong-length test passes vacuously.

- [ ] **Step 6: Wire `process_spiff_segments`**

Append to `src/core/jpeg_helpers.rs`:

```rust
/// Processes APP8 SPIFF segments.
///
/// Matching ExifTool, only 32-byte payloads starting with "SPIFF\0" are
/// treated as SPIFF headers; other APP8 payloads (InfiRay, SEAL, ...) are
/// left alone.
pub fn process_spiff_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const APP8_MARKER: u16 = 0xFFE8;
    for segment in segments.iter().filter(|s| s.marker == APP8_MARKER) {
        if segment.data.len() == 32 && segment.data.starts_with(b"SPIFF\0") {
            let _ =
                crate::parsers::jpeg::app_parsers::parse_spiff_segment(segment.data, metadata);
        }
    }
}
```

In `src/core/operations.rs`: add `process_spiff_segments` to the `jpeg_helpers` import list, and in `parse_jpeg_metadata` add after the `process_dqt_segments` line:

```rust
    process_spiff_segments(&segments, &mut metadata);
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test jpeg_spiff`
Expected: PASS (2 tests).

- [ ] **Step 8: Commit**

```bash
cargo fmt && git add -A src tests
git commit -m "feat(jpeg): wire APP8 SPIFF parsing with ExifTool-parity offsets and tags"
```

---

### Task 4: Multi-chunk ICC profile reassembly

**Files:**
- Modify: `src/core/jpeg_helpers.rs` (rework `process_icc_segments` ~line 321, add `insert_icc_tags` helper)
- Test: `tests/integration/production_wiring_tests.rs`

**Interfaces:**
- Consumes: `IccChunkAssembler::{new, add_chunk, is_complete, assemble, chunk_count, expected_total}` from `crate::parsers::jpeg::icc_chunk_assembler`; `crate::parsers::icc::parse_icc_profile_data(&[u8]) -> Result<HashMap<String, TagValue>>`.
- Produces: same public signature `pub fn process_icc_segments(segments: &[Segment], metadata: &mut MetadataMap)`; new private `fn insert_icc_tags(icc_data: &[u8], metadata: &mut MetadataMap)`.

- [ ] **Step 1: Add the failing integration test**

Add to `tests/integration/production_wiring_tests.rs`:

```rust
fn icc_header_128() -> Vec<u8> {
    let mut h = vec![0u8; 128];
    h[0..4].copy_from_slice(&128u32.to_be_bytes()); // profile size
    h[4..8].copy_from_slice(b"ADBE"); // CMM type
    h[8] = 4; // version 4.0
    h[12..16].copy_from_slice(b"mntr"); // display device profile
    h[16..20].copy_from_slice(b"RGB "); // color space
    h[20..24].copy_from_slice(b"XYZ "); // PCS
    h[36..40].copy_from_slice(b"acsp"); // profile file signature
    h
}

fn icc_chunk(chunk_num: u8, total: u8, data: &[u8]) -> Vec<u8> {
    let mut p = b"ICC_PROFILE\0".to_vec();
    p.push(chunk_num);
    p.push(total);
    p.extend_from_slice(data);
    p
}

#[test]
fn jpeg_multichunk_icc_profile_reassembles() {
    let profile = icc_header_128();
    let (part1, part2) = profile.split_at(64);
    // Chunks arrive out of order to exercise reassembly rather than luck.
    let jpeg_multi = jpeg_with_segments(&[
        jpeg_segment(0xE2, &icc_chunk(2, 2, part2)),
        jpeg_segment(0xE2, &icc_chunk(1, 2, part1)),
        jpeg_segment(0xC0, &sof0_payload()),
    ]);
    let jpeg_single = jpeg_with_segments(&[
        jpeg_segment(0xE2, &icc_chunk(1, 1, &profile)),
        jpeg_segment(0xC0, &sof0_payload()),
    ]);
    let multi = read_temp_file(&jpeg_multi, ".jpg");
    let single = read_temp_file(&jpeg_single, ".jpg");
    let key = "ICC_Profile:ColorSpaceData";
    assert!(
        multi.get(key).is_some(),
        "multi-chunk ICC profile produced no {key}"
    );
    assert_eq!(multi.get(key), single.get(key));
}

#[test]
fn jpeg_incomplete_multichunk_icc_profile_degrades_gracefully() {
    let profile = icc_header_128();
    let (part1, _part2) = profile.split_at(64);
    let jpeg = jpeg_with_segments(&[
        jpeg_segment(0xE2, &icc_chunk(1, 2, part1)), // chunk 2 of 2 missing
        jpeg_segment(0xC0, &sof0_payload()),
    ]);
    let metadata = read_temp_file(&jpeg, ".jpg");
    assert!(metadata.get("ICC_Profile:ColorSpaceData").is_none());
    // File-level tags still parse; the read never hard-fails.
    assert_eq!(metadata.get_integer("File:ImageWidth"), Some(640));
}
```

Run: `cargo test jpeg_multichunk_icc jpeg_incomplete_multichunk` (or run each test name)
Expected: `jpeg_multichunk_icc_profile_reassembles` FAILS (multi-chunk dropped today); the graceful-degradation test passes already.

- [ ] **Step 2: Rework `process_icc_segments`**

Replace the function at `src/core/jpeg_helpers.rs:321` with:

```rust
/// Processes ICC profile APP2 segments and extracts color profile metadata.
///
/// ICC (International Color Consortium) profiles describe the color
/// characteristics of an image. Profiles larger than one APP2 segment
/// (~64KB) are split into chunks carrying a 1-based sequence number and a
/// total count; chunks are reassembled with IccChunkAssembler before parsing.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with ICC profile tags
pub fn process_icc_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    let icc_segments: Vec<&Segment> = segments
        .iter()
        .filter(|s| {
            s.marker == 0xFFE2 && s.data.len() >= 14 && &s.data[0..12] == b"ICC_PROFILE\0"
        })
        .collect();
    if icc_segments.is_empty() {
        return;
    }

    // Fast path: single-chunk profile parses in place, no reassembly copy.
    if icc_segments.len() == 1 && icc_segments[0].data[12] == 1 && icc_segments[0].data[13] == 1 {
        insert_icc_tags(&icc_segments[0].data[14..], metadata);
        return;
    }

    let mut assembler = IccChunkAssembler::new();
    for segment in &icc_segments {
        if let Err(e) = assembler.add_chunk(segment.data) {
            eprintln!("Warning: Invalid ICC profile chunk: {}", e);
            return;
        }
    }
    if !assembler.is_complete() {
        eprintln!(
            "Warning: Incomplete multi-chunk ICC profile ({} of {:?} chunks), skipping",
            assembler.chunk_count(),
            assembler.expected_total()
        );
        return;
    }
    match assembler.assemble() {
        Ok(profile) => insert_icc_tags(&profile, metadata),
        Err(e) => eprintln!("Warning: Failed to assemble ICC profile: {}", e),
    }
}

/// Parses raw ICC profile bytes and inserts ICC_Profile-prefixed tags.
fn insert_icc_tags(icc_data: &[u8], metadata: &mut MetadataMap) {
    match crate::parsers::icc::parse_icc_profile_data(icc_data) {
        Ok(icc_tags) => {
            for (tag_name, value) in icc_tags {
                metadata.insert(format!("ICC_Profile:{}", tag_name), value);
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to parse ICC profile: {}", e);
        }
    }
}
```

Add the import at the top of `jpeg_helpers.rs`:

```rust
use crate::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test jpeg_multichunk_icc && cargo test icc`
Expected: PASS, including all pre-existing ICC and assembler unit tests.

- [ ] **Step 4: Commit**

```bash
cargo fmt && git add -A src tests
git commit -m "feat(jpeg): reassemble multi-chunk ICC profiles via IccChunkAssembler"
```

---

### Task 5: Delete redundant dead parsers

**Files:**
- Modify: `src/parsers/jpeg/app_parsers.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: removal only — `parse_icc_profile_segment`, `parse_adobe_segment`, `parse_activephoto_segment`, `parse_jpeg_ls_segment` cease to exist. `parse_ducky_segment`, `parse_comment_segment`, `parse_spiff_segment`, `parse_app0_extended`, `parse_sof_segment`, `parse_jpeg_hdr_segment` remain.

- [ ] **Step 1: Delete the four functions and their tests**

In `src/parsers/jpeg/app_parsers.rs` delete:

- `parse_icc_profile_segment` (~lines 14–93) — superseded by `crate::parsers::icc::parse_icc_profile_data` used in the live path.
- `parse_adobe_segment` — superseded by `app_segments::parse_app14_adobe` used in the live path.
- `parse_activephoto_segment` — speculative format; ExifTool has no such APP10 handling.
- `parse_jpeg_ls_segment` — speculative; JPEG-LS is signalled by SOF55, not an APP segment.
- Tests: `test_parse_icc_profile`, `test_parse_adobe_segment`, `test_parse_activephoto_segment`, `test_parse_activephoto_empty`, `test_parse_jpeg_ls_segment`, `test_parse_jpeg_ls_with_marker`.
- Update the module doc comment to list only what remains (ICC helper reference removed, COM stays, SOF stays).

- [ ] **Step 2: Verify nothing references the deleted functions**

Run: `grep -rn "parse_icc_profile_segment\|parse_adobe_segment\|parse_activephoto_segment\|parse_jpeg_ls_segment" src/ tests/ bindings/ 2>/dev/null`
Expected: no output.

Run: `cargo build && cargo test -p oxidex --lib app_parsers`
Expected: build succeeds; remaining app_parsers tests pass.

- [ ] **Step 3: Commit**

```bash
cargo fmt && git add -A src
git commit -m "refactor(jpeg): delete superseded dead APP segment parsers"
```

---

### Task 6: Document discrepancies and full verification

**Files:**
- Modify: `tests/integration/KNOWN_DISCREPANCIES.md`

**Interfaces:** none — documentation and verification sweep.

- [ ] **Step 1: Document the two deliberate divergences**

Append to `tests/integration/KNOWN_DISCREPANCIES.md` (follow the file's existing entry format):

```markdown
## JPEG COM / DQT wiring (2026-07-19)

- **File:JPEGQualityEstimate is always emitted.** ExifTool computes this tag
  only when explicitly requested (`-JPEGQualityEstimate` or `RequestAll > 2`)
  because of Perl-side overhead; oxidex has no tag-request mechanism and the
  computation is trivial, so it is always present. Values match ExifTool's
  algorithm exactly (JPEGDigest.pm EstimateQuality).
- **Multiple COM segments collapse to one File:Comment (last wins).** ExifTool
  reports each COM segment as a duplicate Comment tag under `-a`; MetadataMap
  stores one value per key.
```

- [ ] **Step 2: Full workspace verification**

Run, in order, and require all to pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all green. If clippy flags pre-existing issues outside the touched files, do not fix them here; only new warnings block.

- [ ] **Step 3: Manual parity spot-check (evidence for the PR)**

```bash
python3 - <<'EOF'
import struct
out = bytearray(b'\xff\xd8')
def seg(m, p):
    return struct.pack('>BBH', 0xFF, m, len(p)+2) + p
spiff = b'SPIFF\x00' + bytes([1,0,1,3,0,0]) + struct.pack('>II',480,640) + bytes([3,8,5,1]) + struct.pack('>II',72,72)
out += seg(0xE8, spiff)
out += seg(0xFE, b'Parity check comment')
out += seg(0xDB, bytes([0]) + bytes([16]*64))
out += seg(0xC0, bytes([8]) + struct.pack('>HH',480,640) + bytes([3,1,0x22,0,2,0x11,1,3,0x11,1]))
out += b'\xff\xd9'
open('/tmp/parity_check.jpg','wb').write(bytes(out))
EOF
cargo run --quiet -- /tmp/parity_check.jpg
exiftool -G1 -a -JPEGQualityEstimate -Comment -SPIFF:all /tmp/parity_check.jpg
```

Expected: oxidex shows `File:Comment: Parity check comment`,
`File:JPEGQualityEstimate: 87`, and the 11 `SPIFF:*` tags; exiftool's values
match one for one.

- [ ] **Step 4: Commit**

```bash
git add tests/integration/KNOWN_DISCREPANCIES.md
git commit -m "docs(tests): record COM/DQT wiring divergences from ExifTool"
```
