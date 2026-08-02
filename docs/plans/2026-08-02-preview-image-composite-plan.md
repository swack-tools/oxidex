# PreviewImage Tag-Gap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the `PreviewImage` tag gap (118 corpus files: Samsung 50, Sony 48, IFD2 7, Casio 3, Minolta 3, Canon 3, others) by implementing each of ExifTool's three independent extraction mechanisms for this tag, verified against real ExifTool 13.59 source and corpus files rather than approximated.

**Architecture:** `PreviewImage` is declared in `src/composite/tables.rs:369` as a `Composite` (require `PreviewImageStart`+`PreviewImageLength`), but `src/composite/compute.rs` is architecturally string-only — its own doc comment states "Composite tags are not read from the file" — so it can never implement this tag and must not try. Real ExifTool's `PreviewImage` `RawConv` (`Exif.pm:5013`, `ExtractImage`) seeks into the file and reads bytes, then reports the byte count via oxidex's existing `binary_placeholder()` format. The correct home for each mechanism is the parser layer that already has file-byte access: `src/core/tiff_helpers.rs` (mirrors the existing `parse_ifd1_thumbnail`/`ThumbnailImage` pattern), `src/core/jpeg_helpers.rs` (mirrors the existing APP2/APP3/APP4 segment dispatch), and the per-vendor MakerNotes registries (mirrors the existing `src/parsers/tiff/makernotes/sigma.rs` binary-`PreviewImage` pattern, since the generic `MakerNoteParser` trait returns `HashMap<String, String>` and cannot carry binary).

**Tech Stack:** Rust, `MetadataMap`/`TagValue` (`src/core/mod.rs`), `FileReader` trait (`src/io/`), existing tag-comparison harness (`tools/exiftool-tables/conformance.py`, `just compare-exiftool-format`).

## Global Constraints

- **Never approximate a conversion** (AGENTS.md): every task below starts with a verification step against real ExifTool 13.59 source and/or corpus files. If verification fails or is ambiguous for a sub-case, omit that sub-case rather than guess — do not extend the task to cover it speculatively.
- Oracle: `/usr/bin/perl5.34 -I/tmp/oxidex-exiftool-cache/exiftool/lib /tmp/oxidex-exiftool-cache/exiftool/exiftool` (13.59, pinned per PR #410). A bare `perl`/`exiftool` on PATH may be a different version — do not use it.
- Corpus: `/tmp/oxidex-exiftool-cache/combined-samples/` (read-only — copy any file before a write test).
- **CORRECTED default-output contract** (superseding an earlier draft of this section — see `.superpowers/sdd/2026-08-02-preview-image-composite-plan/task-1-report.md` for the full forensic trail). The original draft claimed `PreviewImage` omits entirely when unreadable, based on running `exiftool -PreviewImage <file>` (explicitly *requesting* the tag by name). That was a testing artifact: explicitly requesting a tag sets `$$self{REQ_TAG_LOOKUP}}` for it, which disables `ExtractBinary`'s pre-seek placeholder shortcut (`ExifTool.pm` ~line 9836) and forces a real seek/read that then fails and omits. **That is not how the tag-comparison harness or `oxidex -j -e` query files** — they do a full default-mode dump, not a single explicit `-PreviewImage` request. Re-verified in full-dump mode (`exiftool -G1 -s -a -u <file>`, no explicit `-PreviewImage`) on two independently-sourced out-of-bounds files: `LeicaCL.jpg` (`IFD2:PreviewImageStart`=7064224 in a 50,939-byte file) and `SamsungDV150F.jpg` (MPImage2-embedded pair) — **both show the placeholder** `(Binary data N bytes, use -b option to extract)` with the *declared* length, not an omission, despite `-b` returning 0 bytes and `-validate` warning "past end of file" / "[minor] Error reading PreviewImage from file" for both. This is the **same** mechanism already implemented for `ThumbnailImage` in `read_or_placeholder` (`src/core/tiff_helpers.rs:1048-1065`): `ExtractBinary`'s shortcut returns the placeholder *before* ever seeking, in every default-dump case, so truncation/OOB is invisible to the tag's presence — only its correctness once someone passes `-b`. **Every task in this plan must therefore reuse (or exactly mirror) `read_or_placeholder`'s placeholder-on-failure behavior for the declared-length case, not the Sigma omit-on-OOB pattern.** The one exception: if a specific mechanism's *own* extraction code doesn't go through `Exif.pm`'s shared `ExtractImage`/`ExtractBinary` at all (e.g. a hand-written per-vendor Rust port of a custom `RawConv`), that mechanism's on-failure behavior must be independently verified with a full-dump command (not a single explicit `-TagName` request) before assuming either placeholder or omission — do not generalize Sigma's existing omit behavior to a new mechanism without that check.
- `TagValue::new_binary(bytes)` stores real bytes; the CLI's existing `binary_placeholder()` formatting turns any `TagValue::Binary` into the correct default-mode string. Insert the real bytes (like `src/parsers/tiff/makernotes/sigma.rs:134-137` does), not a pre-rendered placeholder string — that keeps `-b` extraction correct too and is consistent with the rest of the codebase.
- Every insert must still be gated on the underlying data being present at all (omit when absent — e.g. Task 2's byte pattern never matches, or a MakerNote tag ID is missing entirely). What differs per mechanism is what happens when data IS present but a downstream check (bounds, pattern match) fails: **placeholder-on-failure using the raw/declared length is the norm, not the exception** — confirmed for Task 1 (offset-pair OOB) AND Task 3 (Sony 0x2001's header-strip/SOI-fixup pattern mismatch, verified on `SonyDSLR-A700.jpg`: full-dump mode shows the placeholder with the *raw* untransformed byte count even though the pattern check fails). `Exif.pm`'s shared `ExtractBinary` pre-seek shortcut is broader than originally assumed — it applies not just to the generic offset-pair `ExtractImage` path (Tasks 1, 4) but apparently to any oversized MakerNote tag value extraction too. **Treat omission as the exception that needs its own positive evidence, not the default assumption** — Sigma's existing pre-plan code (which does omit on OOB) has NOT been re-verified against this corrected understanding and may itself need re-checking outside this plan's scope. For every remaining task (5), verify the actual on-failure behavior with a full-dump (`-G1 -s -a -u`, never a single explicit `-TagName`) command against a real corpus file before writing any omission logic — do not assume placeholder-vs-omit by analogy to either pattern.
- Leave `src/composite/tables.rs:369` (the `PreviewImage` Composite table entry) untouched. It is inert by design — "a composite whose computation is not implemented simply never fires" is the documented contract in `src/composite/compute.rs:8`. No task in this plan should add a `("Exif", "PreviewImage")` arm to `compute()`; that arm can never be correct because `compute()` has no file access.
- Measure every task with the exiftool-parity workflow: `just compare-exiftool-format <FORMAT>` (or `python3 tools/exiftool-tables/conformance.py <corpus> --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool --oxidex target/release/oxidex`) before and after. Require `missing_in_oxidex` count strictly lower and `regressions` empty; quote the real `exiftool -G1 -s` value for at least one fixed file in the commit message.
- `cargo fmt --all` and `cargo clippy` before each commit. Commit with `git -c commit.gpgsign=false commit --no-gpg-sign`. Never `git add -A`.
- Mutation-test every new test: temporarily revert the implementation change and confirm the new test fails, then restore.

---

## Task 1: IFD2 PreviewImage (JPEG-embedded Leica-style preview)

**Verification status:** VERIFIED. `Exif.pm:707-768` (tag `0x117`, paired with `0x111` via `OffsetPair`) shows the default `StripOffsets`/`StripByteCounts` condition explicitly excludes `$$self{TIFF_TYPE} eq 'APP1' and $$self{DIR_NAME} eq 'IFD2'` (comment: "APP1 IFD2 is for Leica JPEG preview"). That exclusion falls through to the next condition in the array, `$$self{DIR_NAME} ne 'SubIFD2'` (true for `IFD2`), which names the pair `PreviewImageStart`/`PreviewImageLength` with `DataTag => 'PreviewImage'`. So: **in a JPEG's embedded TIFF/EXIF structure, tag `0x0111`/`0x0117` inside the `IFD2` directory is `PreviewImageStart`/`PreviewImageLength`, not `StripOffsets`/`StripByteCounts`.** Ground-truth confirmed on `LeicaCL.jpg`: `IFD2:PreviewImageStart`=7064224, `IFD2:PreviewImageLength`=895146, `IFD2:PreviewImage`=`(Binary data 895146 bytes, use -b option to extract)` — even though 7064224 is past this 50,939-byte file's end (see Global Constraints' corrected default-output contract: this is the **placeholder-on-failure** case, not omission).

**Files:**
- Modify: `src/core/tiff_helpers.rs` — `process_tiff_ifd_tags` (starts line 282) and `parse_ifd_chain` (starts line 174)
- Test: `src/core/tiff_helpers.rs` (`#[cfg(test)] mod tests`, existing convention in this file — see the `ThumbnailImage` tests near line 1385 for the pattern)

**Interfaces:**
- `process_tiff_ifd_tags` currently returns `(Option<u64> exif_ifd_offset, Option<u64> gps_ifd_offset, Option<&[u8]> makernote_data)`. Extend it to also return `Option<(u64 start, u64 length)>` for a `PreviewImageStart`/`PreviewImageLength` pair found in `IFD2`, so callers get `(Option<u64>, Option<u64>, Option<&[u8]>, Option<(u64, u64)>)`.
- `parse_ifd_chain` (which owns `reader`) uses that fourth tuple element to perform the bounds-checked read and insert `IFD2:PreviewImage`, the same way it already uses `exif_offset`/`gps_offset`/`makernote_data` from the same tuple (lines 197-219).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/core/tiff_helpers.rs` (follow the existing helper style used by the `ThumbnailImage` tests, e.g. around line 1436 which builds a synthetic TIFF/IFD1 buffer — build a synthetic buffer with IFD0 → IFD2 chain instead):

```rust
#[test]
fn ifd2_preview_image_start_length_pair_becomes_preview_image() {
    // Build a minimal TIFF: IFD0 (no entries of interest, next-IFD -> IFD1),
    // IFD1 (no entries of interest, next-IFD -> IFD2),
    // IFD2 with tag 0x0111 (PreviewImageStart) = some in-bounds offset and
    // tag 0x0117 (PreviewImageLength) = a small length, followed by that many
    // real bytes at that offset.
    let preview_bytes = b"\xff\xd8\xff\xdbFAKEPREVIEWDATA";
    let buffer = build_tiff_with_ifd2_preview(preview_bytes);
    let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&buffer);
    let mut metadata = MetadataMap::new();

    parse_ifd_chain(&reader, TIFF_HEADER_SIZE, ByteOrder::LittleEndian, &mut metadata).unwrap();

    assert_eq!(
        metadata.get("IFD2:PreviewImage"),
        Some(&TagValue::new_binary(preview_bytes.to_vec()))
    );
    assert!(metadata.get("IFD2:StripOffsets").is_none());
    assert!(metadata.get("IFD2:StripByteCounts").is_none());
}

#[test]
fn ifd2_preview_image_shows_placeholder_when_out_of_bounds() {
    // Same shape, but PreviewImageLength points past the end of the buffer.
    // Real ExifTool still reports the tag here (LeicaCL.jpg ground truth:
    // IFD2:PreviewImageStart=7064224 in a 50,939-byte file, yet
    // IFD2:PreviewImage is the placeholder, not omitted) because
    // ExtractBinary's shortcut returns the declared-length placeholder
    // before ever seeking. Mirror ThumbnailImage's `read_or_placeholder`.
    let declared_length = 895146u64;
    let buffer = build_tiff_with_ifd2_preview_oob(declared_length);
    let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&buffer);
    let mut metadata = MetadataMap::new();

    parse_ifd_chain(&reader, TIFF_HEADER_SIZE, ByteOrder::LittleEndian, &mut metadata).unwrap();

    assert_eq!(
        metadata.get("IFD2:PreviewImage"),
        Some(&TagValue::new_string(format!(
            "(Binary data {} bytes, use -b option to extract)",
            declared_length
        )))
    );
}
```

Write `build_tiff_with_ifd2_preview` and `build_tiff_with_ifd2_preview_oob` as local test helpers that assemble raw TIFF bytes by hand (little-endian `II*\0`, IFD0 with a next-IFD pointer to IFD1, IFD1 with a next-IFD pointer to IFD2, IFD2 carrying entries `0x0111`=LONG offset and `0x0117`=LONG length) — follow the byte-assembly style already used by the existing `ThumbnailImage` round-trip tests in this file (search for the test around line 1356 that builds `(TAG_THUMBNAIL_OFFSET, LONG, ...)` tuples) rather than introducing a new helper library.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ifd2_preview_image -- --nocapture`
Expected: FAIL — `IFD2:PreviewImage` is absent (currently unimplemented), and/or `IFD2:StripOffsets`/`StripByteCounts` are present instead (confirming today's mis-naming).

- [ ] **Step 3: Implement the naming + extraction**

In `process_tiff_ifd_tags`, add a case (alongside the existing `0x8769`/`0x8825`/`0xC4A5`/GeoTiff special-cases starting line 303) that, when `ifd_name == "IFD2"` and `tag_id` is `0x0111` or `0x0117`, captures the raw integer via the same `raw_bytes_to_tag_value(...).as_integer()` pattern already used for `TAG_THUMBNAIL_OFFSET`/`TAG_THUMBNAIL_LENGTH` (lines 938-955), stores it into local `preview_start`/`preview_length` variables, and `continue`s (do **not** fall through to the generic tag insertion that would otherwise name them `StripOffsets`/`StripByteCounts`). Return `(exif_ifd_offset, gps_ifd_offset, makernote_data, preview_start.zip(preview_length))` from the function.

In `parse_ifd_chain`, after the existing `makernote_data` handling (after line 219), add:

```rust
if ifd_name == "IFD2"
    && let Some((start, length)) = preview_offset_length
    && length > 0
{
    metadata.insert("IFD2:PreviewImage", read_or_placeholder(reader, start, length));
}
```

(Destructure the fourth tuple element from `process_tiff_ifd_tags`'s return at line 191-192 as `preview_offset_length`.) **Use the existing `read_or_placeholder` helper (`src/core/tiff_helpers.rs:1048-1065`) directly** — do not write a new bounds-check function and do not omit on failure. Per the corrected Global Constraints, `PreviewImage` shows the declared-length placeholder in the default dump even when the range is out of bounds, exactly like `ThumbnailImage` already does; `read_or_placeholder` already implements exactly this.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib ifd2_preview_image -- --nocapture`
Expected: PASS for both tests.

- [ ] **Step 5: Verify against the real corpus**

```bash
cargo build --release --bin oxidex
just compare-exiftool-format JPEG
```

Cross-check against `LeicaCL.jpg`, which is the confirmed ground-truth fixture for this task (out-of-bounds, so it exercises the placeholder path, not just the happy path):

```bash
/usr/bin/perl5.34 -I/tmp/oxidex-exiftool-cache/exiftool/lib /tmp/oxidex-exiftool-cache/exiftool/exiftool -G1 -s -a -u /tmp/oxidex-exiftool-cache/combined-samples/Leica/LeicaCL.jpg | grep -i preview
./target/release/oxidex -j -e /tmp/oxidex-exiftool-cache/combined-samples/Leica/LeicaCL.jpg | grep -A1 PreviewImage
```

Expect both to show `IFD2:PreviewImage` (or `EXIF:PreviewImage` in oxidex's family-0 view) as `(Binary data 895146 bytes, use -b option to extract)`. Do NOT use a bare `-PreviewImage` explicit tag request for this check — it disables ExifTool's placeholder shortcut and gives a false omission read (this is exactly the mistake the plan's original draft made; see the corrected Global Constraints). Confirm `missing_in_oxidex` for JPEG dropped by the IFD2 count and `regressions` is empty.

- [ ] **Step 6: Commit**

```bash
git add src/core/tiff_helpers.rs
git -c commit.gpgsign=false commit --no-gpg-sign -m "fix(tiff): name IFD2 0x111/0x117 PreviewImageStart/Length and extract PreviewImage

Exif.pm:707-768 excludes APP1's IFD2 (Leica JPEG preview) from the
generic StripOffsets/StripByteCounts naming; the pair is
PreviewImageStart/PreviewImageLength there instead, and DataTag
PreviewImage reads the bytes they point at. Uses read_or_placeholder
(same as ThumbnailImage), showing the declared-length placeholder even
out of bounds, per PreviewImage's ExtractImage/ExtractBinary
semantics -- verified against LeicaCL.jpg, whose IFD2:PreviewImageStart
(7064224) is past its 50,939-byte file end yet ExifTool still reports
the placeholder in default-dump mode."
```

---

## Task 2: Samsung/HP/BenQ/GoPro/Rollei direct APP2/APP3/APP4 PreviewImage dump

**Verification status:** VERIFIED at the byte level. `ExifTool.pm:7997-8127` (marker dispatch inside the main segment-scanning loop):

- **APP2** (`marker == 0xe2`): `$$segDataPt =~ /^(|QVGA\0|BGTH)\xff\xd8\xff[\xdb\xe0\xe1]/` — the segment payload, optionally prefixed by the literal bytes `QVGA\0` or `BGTH`, must continue with `\xFF\xD8\xFF` followed by one of `\xDB`/`\xE0`/`\xE1`. On match, `$preview = substr($segData, length($1))` (i.e., the matched prefix, if any, is stripped; the rest — starting at `\xFF\xD8\xFF...` — is the preview). If the *next* segment is not also APP2 (`$nextMarker ne $marker`), the tag is emitted immediately (`FoundTag('PreviewImage', $preview)`); otherwise accumulate across consecutive APP2 segments.
- **APP3** (`marker == 0xe3`): `$$segDataPt =~ /^\xff\xd8\xff\xdb/` (Samsung/HP/BenQ, no prefix) — the *whole* segment data is the preview (`$preview = $segData`), continuing into APP4 if the immediately next segment is APP4 (`$nextMarker == 0xe4`).
- **APP4** (`marker == 0xe4`, `elsif ($preview)` branch at line 8116): a continued Samsung-S1060-style preview from APP3 concatenates the APP4 payload (`$preview .= $segData`), then emits unless the next segment is APP5.
- Group: `FoundTag` with no active IFD context defaults family-1 group to `File` — verified empirically: `exiftool -G1 -s -PreviewImage SamsungDigimaxA40.jpg` → `[File]  PreviewImage : (Binary data 36864 bytes, use -b option to extract)`.

**Files:**
- Modify: `src/core/jpeg_helpers.rs` (add a new function alongside the existing APP2/APP3/APP4 dispatch — see the MPF/ICC/InfiRay handlers starting around line 488 and the `APP2_MARKER`/`APP4_MARKER` constants at lines 1057/1059 for the established pattern)
- Test: `src/core/jpeg_helpers.rs` (or a new `#[cfg(test)]` module in the same file if none exists near the segment-dispatch code — check first)

**Interfaces:**
- Consumes: `Vec<Segment>` from `src/parsers/jpeg/segment_parser.rs::parse_segments` (already used by the other APP2/APP3/APP4 handlers in this file) — each `Segment` has `.marker: u16` and `.data: &[u8]` (confirm exact field names by reading `src/parsers/jpeg/segment_parser.rs:149-240` before writing code; do not guess field names).
- Produces: inserts `File:PreviewImage` (`TagValue::new_binary`) into the `metadata: &mut MetadataMap` passed to the new function, called from wherever the other segment-dispatch functions in `jpeg_helpers.rs` are invoked from the top-level JPEG parser (grep the callers of the existing `process_mpf_segments`-style functions to find that call site).

- [ ] **Step 1: Write the failing test**

Add a test that builds a synthetic JPEG byte stream with SOI, an APP3 segment whose payload starts with `\xFF\xD8\xFF\xDB` followed by fake preview bytes, then EOI, and asserts the new function inserts `File:PreviewImage` with exactly those payload bytes:

```rust
#[test]
fn app3_preview_dump_is_extracted_as_file_preview_image() {
    let preview_payload = b"\xff\xd8\xff\xdb\x00\x43FAKEDATA";
    let jpeg = build_jpeg_with_app3_preview(preview_payload);
    let segments = crate::parsers::jpeg::segment_parser::parse_segments(
        &crate::io::buffered_reader::BufferedReader::from_bytes(&jpeg),
    )
    .unwrap();
    let mut metadata = MetadataMap::new();

    extract_direct_preview_image(&segments, &mut metadata);

    assert_eq!(
        metadata.get("File:PreviewImage"),
        Some(&TagValue::new_binary(preview_payload.to_vec()))
    );
}

#[test]
fn app2_preview_dump_strips_qvga_prefix() {
    // Segment payload: "QVGA\0" + \xFF\xD8\xFF\xE0 + fake JPEG bytes.
    // The QVGA\0 prefix must NOT be part of the stored PreviewImage.
    let inner = b"\xff\xd8\xff\xe0FAKE2";
    let mut payload = b"QVGA\0".to_vec();
    payload.extend_from_slice(inner);
    let jpeg = build_jpeg_with_app2_segment(&payload);
    let segments = crate::parsers::jpeg::segment_parser::parse_segments(
        &crate::io::buffered_reader::BufferedReader::from_bytes(&jpeg),
    )
    .unwrap();
    let mut metadata = MetadataMap::new();

    extract_direct_preview_image(&segments, &mut metadata);

    assert_eq!(
        metadata.get("File:PreviewImage"),
        Some(&TagValue::new_binary(inner.to_vec()))
    );
}
```

Write `build_jpeg_with_app3_preview` and `build_jpeg_with_app2_segment` as local helpers assembling raw marker bytes (`\xFF\xD8` SOI, `\xFF\xE3` + big-endian u16 length + payload for APP3, `\xFF\xE2` + length + payload for APP2, `\xFF\xD9` EOI) — mirror the existing segment-construction test helper in `src/parsers/jpeg/segment_parser.rs` around line 305-322 (`"Creates a JPEG with multiple segments"`) rather than inventing new byte-layout conventions.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib preview_dump -- --nocapture`
Expected: FAIL — `extract_direct_preview_image` does not exist yet (compile error), or once stubbed, `File:PreviewImage` is absent.

- [ ] **Step 3: Implement `extract_direct_preview_image`**

```rust
/// ExifTool.pm:7997-8127 — a preview JPEG embedded directly in APP2/APP3
/// (optionally continued into APP4/APP5), found by byte-pattern rather than
/// an offset/length pair. No IFD context means ExifTool's FoundTag defaults
/// the displayed group to File.
pub fn extract_direct_preview_image(segments: &[Segment], metadata: &mut MetadataMap) {
    const APP2: u16 = 0xFFE2;
    const APP3: u16 = 0xFFE3;
    const APP4: u16 = 0xFFE4;

    let mut preview: Option<Vec<u8>> = None;

    for (index, segment) in segments.iter().enumerate() {
        let next_marker = segments.get(index + 1).map(|s| s.marker);

        match segment.marker {
            APP2 => {
                for prefix in [&b""[..], b"QVGA\0", b"BGTH"] {
                    if let Some(rest) = segment.data.strip_prefix(*prefix) {
                        if rest.starts_with(b"\xff\xd8\xff")
                            && matches!(rest.get(3), Some(0xdb | 0xe0 | 0xe1))
                        {
                            preview = Some(rest.to_vec());
                            break;
                        }
                    }
                }
                if preview.is_some() && next_marker != Some(APP2) {
                    metadata.insert(
                        "File:PreviewImage",
                        TagValue::new_binary(preview.take().unwrap()),
                    );
                }
            }
            APP3 => {
                if segment.data.starts_with(b"\xff\xd8\xff\xdb") {
                    preview = Some(segment.data.to_vec());
                }
                if preview.is_some() && next_marker != Some(APP4) {
                    metadata.insert(
                        "File:PreviewImage",
                        TagValue::new_binary(preview.take().unwrap()),
                    );
                }
            }
            APP4 => {
                if let Some(existing) = preview.as_mut() {
                    existing.extend_from_slice(segment.data);
                    metadata.insert(
                        "File:PreviewImage",
                        TagValue::new_binary(preview.take().unwrap()),
                    );
                }
            }
            _ => {}
        }
    }
}
```

Before finalizing, confirm the exact `Segment` field names (`.marker`, `.data`) against `src/parsers/jpeg/segment_parser.rs` — adjust if they differ. Wire the call into the same place `jpeg_helpers.rs`'s other segment-dispatch functions (MPF/ICC) are invoked from the top-level JPEG parser.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib preview_dump -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Verify against the real corpus**

```bash
cargo build --release --bin oxidex
just compare-exiftool-format JPEG
```

Spot-check several Samsung files by hand, including the truncated/OOB ones to confirm they still omit correctly (this mechanism has no offset/length pair to go out of bounds on, since the bytes are already in the segment — but confirm no spurious tag appears where ExifTool shows none):

```bash
for f in SamsungDigimaxA40 SamsungD70 SamsungDV150F; do
  /usr/bin/perl5.34 -I/tmp/oxidex-exiftool-cache/exiftool/lib /tmp/oxidex-exiftool-cache/exiftool/exiftool -G1 -s -PreviewImage "/tmp/oxidex-exiftool-cache/combined-samples/Samsung/$f.jpg"
  ./target/release/oxidex -j -e "/tmp/oxidex-exiftool-cache/combined-samples/Samsung/$f.jpg" | grep -i preview
done
```

- [ ] **Step 6: Commit**

```bash
git add src/core/jpeg_helpers.rs
git -c commit.gpgsign=false commit --no-gpg-sign -m "feat(jpeg): extract Samsung/HP/BenQ direct APP2/APP3/APP4 PreviewImage dump

ExifTool.pm:7997-8127 finds this preview by byte pattern inside the
segment payload itself (no offset/length pair), and FoundTag with no
IFD context defaults the group to File. Verified byte-for-byte against
SamsungDigimaxA40.jpg's [File] PreviewImage output."
```

---

## Task 3: Sony MakerNotes 0x2001 PreviewImage

**Verification status:** VERIFIED. `Sony.pm:906-939`: tag `0x2001` in the Sony MakerNotes IFD, `Groups => {2 => 'Preview'}`, `DataTag => 'PreviewImage'`. Its `RawConv`:
```perl
return \$val if $val =~ /^Binary/;
$val = substr($val,0x20) if length($val) > 0x20;
return \$val if $val =~ s/^.(\xd8\xff[\xdb\xe1])/\xff$1/s;
$$self{PreviewError} = 1 unless $val eq 'none' or $val eq '';
return undef;
```
i.e.: strip the first 32 bytes (proprietary Sony header) if the value is longer than 32 bytes; if what follows byte 0 of the remainder matches `.(\xD8\xFF[\xDB\xE1])` (one arbitrary byte then `D8 FF DB` or `D8 FF E1`), replace that leading byte with `FF` (reconstructing a valid JPEG SOI `FF D8 FF ..`) and use that as the value; otherwise the tag is not emitted (`undef`).

**Files:**
- Modify: `src/parsers/tiff/makernotes/sony.rs` (find the existing MakerNotes tag dispatch table/`match` for Sony tag IDs — grep `0x2001` and `0x9402`/similar hex-ID match arms for the established pattern)
- Note: like Sigma, this needs `MetadataMap`/`TagValue::Binary` access, which the generic `MakerNoteParser` trait (`HashMap<String, String>`) cannot carry. Check first whether Sony is already special-cased outside the string-map dispatcher (grep `parse_sigma_makernote_if_sigma` in `src/core/tiff_helpers.rs:1146-1163` for the pattern, and check whether an equivalent `parse_sony_makernote_if_sony`-style hook already exists or needs adding).
- Test: `src/parsers/tiff/makernotes/sony.rs` (or wherever Sony's existing MakerNotes tests live — check first)

**CORRECTION (found during Task 3's own review, mirroring Task 1's earlier correction):** the "omits when the fixup doesn't match" claim below was wrong for the same reason Task 1's original "omits on OOB" claim was wrong. Verified with a full-dump command (`exiftool -G1 -s -a -u <file>`, NOT `-PreviewImage`) against real corpus files (`SonyDSLR-A700.jpg`, `SonyDSLR-A380.jpg`, `SonyILCE-6000.jpg`, all of which fail under an explicit `-PreviewImage`/`-b` request — "[minor] Error reading PreviewImage", 0 bytes): full-dump mode still shows `[Sony] PreviewImage : (Binary data 696508 bytes, use -b option to extract)` for `SonyDSLR-A700.jpg` — the placeholder, with the tag's *raw, untransformed* declared byte count, not an omission. This confirms the same `ExtractBinary` pre-seek shortcut from Task 1 also gates this tag: in default-dump mode, RawConv (the header-strip/SOI-fixup) never runs at all, because the shortcut returns a placeholder built from the entry's raw byte count before extraction/RawConv happens.

**Interfaces:**
- Produces `MakerNotes:PreviewImage` with a two-way result, mirroring Task 1's `read_or_placeholder` split (real bytes when verified-correct, a placeholder string otherwise — never omit when `raw` bytes are present at all):
  - When the header-strip + SOI-fixup pattern **matches**: `TagValue::new_binary(transformed_bytes)` — real, correct bytes, right for both the default view and any future `-b` support.
  - When it does **not** match (but `raw` is non-empty): `TagValue::new_string(format!("(Binary data {} bytes, use -b option to extract)", raw.len()))` — the placeholder, using the **raw, untransformed** length (matches the real corpus evidence above; this is not the transformed length, since RawConv never ran).
  - Only when `raw` itself is empty/absent does nothing get inserted.

- [ ] **Step 1: Investigate the current Sony MakerNotes dispatch**

```bash
grep -n "0x2001\|PreviewImage\|fn parse_sony" src/parsers/tiff/makernotes/sony.rs src/parsers/tiff/makernotes/registries/sony*.rs 2>/dev/null
grep -n "parse_sigma_makernote_if_sigma\|dispatch_makernote_with_context" src/core/tiff_helpers.rs
```
Confirm whether Sony already has a context-based (non-string-map) hook point like Sigma's, and whether `0x2001` is already claimed by the string-map dispatcher (in which case it must be removed from there first, the same way Sigma is fully carved out rather than partially handled by both paths).

- [ ] **Step 2: Write the failing test**

```rust
#[test]
fn sony_0x2001_strips_header_and_fixes_soi_marker() {
    // 32-byte fake header, then a single garbage byte, then D8 FF DB + fake JPEG body.
    let mut raw = vec![0u8; 32];
    raw.push(0x00); // the arbitrary byte the RawConv regex discards
    raw.extend_from_slice(b"\xd8\xff\xdbFAKEBODY");
    let expected = {
        let mut v = vec![0xff];
        v.extend_from_slice(b"\xd8\xff\xdbFAKEBODY");
        v
    };

    let mut metadata = MetadataMap::new();
    parse_sony_preview_image(&raw, &mut metadata);

    assert_eq!(
        metadata.get("MakerNotes:PreviewImage"),
        Some(&TagValue::new_binary(expected))
    );
}

#[test]
fn sony_0x2001_shows_placeholder_with_raw_length_when_no_valid_soi() {
    // Real ExifTool (verified on SonyDSLR-A700.jpg, full-dump mode) still
    // shows the placeholder here, using the RAW untransformed byte count --
    // RawConv's header-strip/SOI-fixup never runs in default-dump mode.
    let mut raw = vec![0u8; 32];
    raw.extend_from_slice(b"NOTAJPEGHEADERATALL"); // 20 bytes, total raw.len() == 52

    let mut metadata = MetadataMap::new();
    parse_sony_preview_image(&raw, &mut metadata);

    assert_eq!(
        metadata.get("MakerNotes:PreviewImage"),
        Some(&TagValue::new_string(
            "(Binary data 52 bytes, use -b option to extract)".to_string()
        ))
    );
}

#[test]
fn sony_0x2001_omits_when_raw_is_empty() {
    let mut metadata = MetadataMap::new();
    parse_sony_preview_image(&[], &mut metadata);
    assert!(metadata.get("MakerNotes:PreviewImage").is_none());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib sony_0x2001 -- --nocapture`
Expected: FAIL — `parse_sony_preview_image` does not exist yet.

- [ ] **Step 4: Implement `parse_sony_preview_image`**

```rust
/// Sony.pm:906-939 RawConv for MakerNotes 0x2001: strip a 32-byte proprietary
/// header, then require the next byte to be arbitrary and the three after it
/// to be `D8 FF DB` or `D8 FF E1` (a JPEG SOI missing its leading FF), and
/// reconstruct that FF.
///
/// In ExifTool's default (non-`-b`) dump mode, `ExtractBinary`'s pre-seek
/// placeholder shortcut means RawConv never actually runs -- verified on
/// SonyDSLR-A700.jpg: real ExifTool still shows the placeholder using the
/// RAW, untransformed byte count even when the SOI pattern wouldn't match.
/// So an unmatched pattern is NOT an omission -- it's a placeholder built
/// from `raw.len()`, matching Task 1's `read_or_placeholder` split (real
/// bytes when verified-correct, a placeholder string otherwise).
pub fn parse_sony_preview_image(raw: &[u8], metadata: &mut MetadataMap) {
    if raw.is_empty() {
        return;
    }
    let body = if raw.len() > 0x20 { &raw[0x20..] } else { raw };
    let transformed = body.get(1..).and_then(|rest| {
        (rest.len() >= 3 && rest[0] == 0xd8 && rest[1] == 0xff && matches!(rest[2], 0xdb | 0xe1))
            .then(|| {
                let mut fixed = Vec::with_capacity(rest.len() + 1);
                fixed.push(0xff);
                fixed.extend_from_slice(rest);
                fixed
            })
    });
    let value = match transformed {
        Some(bytes) => TagValue::new_binary(bytes),
        None => TagValue::new_string(format!(
            "(Binary data {} bytes, use -b option to extract)",
            raw.len()
        )),
    };
    metadata.insert("MakerNotes:PreviewImage", value);
}
```

Wire this into Sony's MakerNotes parsing at the point Step 1 identified, following whatever calling convention that hook point uses (context struct vs. raw byte slice — match Sigma's `parse_sigma_makernote(tiff, value_offset, tiff_base, metadata)` signature shape if a new dedicated hook is needed, per `src/core/tiff_helpers.rs:1146-1163`).

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib sony_0x2001 -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Verify against the real corpus**

Do NOT use an explicit `-PreviewImage`/`-b` request for this check — it disables ExifTool's placeholder shortcut and gives a false read (this is exactly the mistake this task's own verification note above corrects). Use a full-dump command instead:

```bash
/usr/bin/perl5.34 -I/tmp/oxidex-exiftool-cache/exiftool/lib /tmp/oxidex-exiftool-cache/exiftool/exiftool -G1 -s -a -u /tmp/oxidex-exiftool-cache/combined-samples/Sony/SonyDSLR-A700.jpg | grep -i preview
./target/release/oxidex -j -e /tmp/oxidex-exiftool-cache/combined-samples/Sony/SonyDSLR-A700.jpg | grep -A1 -i preview
```

Expect both to show `Sony:PreviewImage` (or however oxidex's existing Sony MakerNotes tags are grouped — match whatever convention the other Sony tags in this file already use) as `(Binary data 696508 bytes, use -b option to extract)`. If the corpus turns up a file where the header-strip/SOI-fixup pattern genuinely succeeds, verify that one shows real matching bytes too (via a hex-dump or length comparison), but the primary parity target for this task is the placeholder-with-raw-length case, confirmed above as the common one in this corpus.

- [ ] **Step 7: Commit**

```bash
git add src/parsers/tiff/makernotes/sony.rs
git -c commit.gpgsign=false commit --no-gpg-sign -m "feat(makernotes): extract Sony 0x2001 PreviewImage with header-strip/SOI-fixup

Sony.pm:906-939's RawConv strips a 32-byte proprietary header then
requires a D8 FF DB/E1 JPEG-SOI-minus-leading-FF pattern, reconstructing
the FF. In ExifTool's default dump mode this RawConv never actually
runs (ExtractBinary's pre-seek shortcut), so an unmatched pattern shows
the placeholder with the RAW untransformed byte count, not an omission
-- verified against SonyDSLR-A700.jpg's real [Sony] PreviewImage
output in full-dump mode."
```

---

## Task 4: CR2 IFD0 and DNG PreviewImageStart/Length

**Verification status:** PARTIALLY VERIFIED — the ExifTool-side condition is confirmed, oxidex's internal RAW-parser architecture is NOT yet confirmed to reuse `src/core/tiff_helpers.rs`.

`Exif.pm:644-672` gives the exact conditions (evaluated in this order against tag `0x111`/`0x117`):
1. `$$self{TIFF_TYPE} eq "CR2"` → `PreviewImageStart`/`PreviewImageLength` (this condition is checked in ANY directory of a CR2 file, but the general `StripOffsets` condition ahead of it in the array only excludes `TIFF_TYPE eq 'CR2' and DIR_NAME eq 'IFD0'`, so in practice CR2's non-IFD0 directories are already claimed by `StripOffsets` first — the practical effect is CR2 **IFD0** gets `PreviewImageStart`/`Length`, other CR2 directories keep `StripOffsets`/`StripByteCounts`).
2. `$$self{DIR_NAME} ne "SubIFD2"` (DNG, any directory except `SubIFD2`, after the CR2 and MRW-A200 and DNG-Lossy-JPEG special cases above it in the array are ruled out) → `PreviewImageStart`/`PreviewImageLength`.

**Files:** likely `src/core/tiff_helpers.rs` for the DNG case (same `parse_ifd_chain`/`process_tiff_ifd_tags` machinery as Task 1, gated on TIFF_TYPE/DNG detection instead of `ifd_name == "IFD2"`), and a RAW-specific parser for CR2 — memory note `oxidex-raw-parser-duplicate-paths` records that "RAW has simplified copies of core/tiff conversions", so **do not assume** CR2 goes through `tiff_helpers.rs` at all. Confirm before writing any code.

- [ ] **Step 1: Investigate oxidex's CR2 and DNG parsing entry points**

```bash
grep -rn "\"CR2\"\|Cr2\|\"DNG\"\|Dng" src/parsers/raw/format_detection.rs src/parsers/raw/metadata.rs | head -30
grep -rln "fn parse" src/parsers/raw/*.rs | head -20
```
Determine: (a) does CR2 parsing call into `src/core/tiff_helpers.rs::parse_ifd_chain`/`process_tiff_ifd_tags` (in which case this task extends Task 1's mechanism with a `TIFF_TYPE == CR2 && ifd_name == "IFD0"` gate), or does it use a separate RAW-specific IFD walker (in which case find that walker's file and mirror the pattern there instead)? Do the same check for DNG. Write down the actual call path before writing the implementation step below — if the two formats use different walkers, split this into Task 4a (CR2) and Task 4b (DNG) with their own file lists.

- [ ] **Step 2: Verify against real corpus files**

```bash
find /tmp/oxidex-exiftool-cache/combined-samples -iname "*.cr2" | head -5
find /tmp/oxidex-exiftool-cache/combined-samples -iname "*.dng" | head -5
```
For each format with at least one sample file, confirm the real tag name and group with:
```bash
/usr/bin/perl5.34 -I/tmp/oxidex-exiftool-cache/exiftool/lib /tmp/oxidex-exiftool-cache/exiftool/exiftool -G1 -s -PreviewImage -PreviewImageStart -PreviewImageLength <file>
```
If no CR2 or DNG sample exists in the cache, do not guess the byte layout from source reading alone — pull a real sample via the corpus tooling this repo already uses (see `exiftool-parity` skill's "Data Locations" reference) before implementing that half of this task, or split it out to run when a corpus file becomes available.

- [ ] **Step 3-7: TDD implementation, per whichever call path Step 1 identifies**

Follow the same shape as Task 1 Steps 1-6 (failing test with a synthetic file → bounds-checked extraction using `read_or_placeholder`, placeholder-on-failure like Task 1, NOT omit-on-OOB — this is the same `Exif.pm` `ExtractImage`/`ExtractBinary` mechanism as Task 1, see the corrected Global Constraints → real-corpus verification → commit), scoped separately to CR2 IFD0 and to DNG's non-`SubIFD2` directories per the two verified conditions above. Do not write this task's implementation code until Step 1's file-path investigation and Step 2's corpus check are both complete — filling in the file paths and byte layout from source reading alone would violate this plan's "never approximate" constraint, since oxidex's actual RAW-parser structure is the unverified part, not the ExifTool-side rule.

---

## Task 5: Casio, Olympus, Minolta MakerNotes-inline PreviewImage

**Verification status:** NOT YET VERIFIED. All three share ExifTool's `%Image::ExifTool::previewImageTagInfo` (`ExifTool.pm:1268-1280`) as their tag-info hash (`Casio.pm:405`, `Olympus.pm:789`, `Minolta.pm:767`), which confirms a `PreviewImage` tag exists in each MakerNotes table with `RawConv => '$self->ValidateImage(ref $val ? $val : \$val, $tag)'` — i.e., **no header-stripping** (unlike Sony) — but the specific tag ID, byte-order quirks, and whether the value is a direct offset-pointed read or an inline blob differ per vendor and have not been individually confirmed against source or corpus files.

**Files:**
- `src/parsers/tiff/makernotes/casio.rs`
- `src/parsers/tiff/makernotes/olympus.rs` (and possibly `src/parsers/tiff/makernotes/olympus/` subdirectory — check both)
- `src/parsers/tiff/makernotes/minolta.rs`

- [ ] **Step 1: Investigate each vendor's PreviewImage tag definition**

```bash
grep -n "PreviewImage\|previewImageTagInfo" /tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool/Casio.pm
grep -n "PreviewImage\|previewImageTagInfo" /tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool/Olympus.pm
grep -n "PreviewImage\|previewImageTagInfo" /tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool/Minolta.pm
```
For each hit, read the surrounding ~20 lines (tag ID, `Format`/`Writable`, whether it's a direct value or an `IsOffset`+separate-length pair like Sony, any `Condition`) the same way Task 3 verified Sony's `0x2001` — quote the exact source lines in the resulting task write-up, the same way this plan's other tasks do, before writing any Rust.

- [ ] **Step 2: Verify against real corpus files**

```bash
for f in $(find /tmp/oxidex-exiftool-cache/combined-samples/Casio /tmp/oxidex-exiftool-cache/combined-samples/Olympus /tmp/oxidex-exiftool-cache/combined-samples/Minolta -iname "*.jpg" 2>/dev/null | head -40); do
  out=$(timeout 5 /usr/bin/perl5.34 -I/tmp/oxidex-exiftool-cache/exiftool/lib /tmp/oxidex-exiftool-cache/exiftool/exiftool -G1 -s -PreviewImage "$f" 2>/dev/null)
  [ -n "$out" ] && echo "$f: $out"
done
```
Identify at least one real, non-truncated sample per vendor where ExifTool successfully emits `PreviewImage`, to use as the ground-truth fixture for the implementation's test and end-to-end check.

- [ ] **Step 3-7: TDD implementation, per vendor**

Once Steps 1-2 produce concrete tag IDs and at least one verified sample per vendor, implement each following the same shape as Task 3 (Sony): a small binary-aware extraction function taking the raw MakerNote bytes and `&mut MetadataMap`, wired in outside the string-only `MakerNoteParser` dispatcher the same way Sigma and Sony are (`src/core/tiff_helpers.rs:1146-1163` is the wiring point — extend the `if parse_sigma_makernote_if_sigma(...) { return; }` chain with one check per additional vendor that needs binary output). Write the failing test first, using the byte layout Step 1 confirmed — do not write extraction code for any vendor whose tag definition Step 1 could not pin down precisely; split it into its own follow-up task instead of guessing.

**On-failure behavior is not settled for this task and must come out of Step 1, not be assumed:** `%previewImageTagInfo` (`ExifTool.pm:1268-1280`) only supplies a generic `RawConv => ValidateImage(...)` — it does not by itself tell you whether Casio/Olympus/Minolta's `PreviewImage` is a directly-inlined MakerNote value (Sony/Sigma-shaped: no file seek, so omit-on-mismatch is right) or an `IsOffset`+separate-length pair that still routes through `Exif.pm`'s shared `ExtractImage`/`ExtractBinary` (Task 1/4-shaped: placeholder-on-failure is right). Step 1 must determine which for each vendor by reading whether their tag table entry carries `IsOffset`/`OffsetPair`, and Step 2 must confirm the actual on-failure behavior with a full-dump command on a real out-of-bounds sample if one exists in the corpus, the same way Task 1 did for `LeicaCL.jpg`.

---

## Self-Review Notes

- **Spec coverage:** Task 1 covers the IFD2 bucket (7 files). Task 2 covers the Samsung/HP/BenQ/GoPro/Rollei direct-dump bucket (50 files, the largest). Task 3 covers Sony (48 files, second largest). Task 4 covers the CR2/Canon and DNG remainder (Canon 3 + any DNG files in "others"). Task 5 covers Casio (3) + Minolta (3) + any Olympus files folded into "others". Together these account for all 118 measured files; any residual "others" not matching one of these five mechanisms should be re-measured with the tag-comparison harness after Tasks 1-5 land rather than assumed closed.
- **`compute.rs` stale entry:** addressed explicitly in Global Constraints — left alone, not touched by any task.
- **Placeholder scan:** Tasks 1-3 contain complete, real code and verified byte-level conditions with no TBDs. Tasks 4-5 deliberately stop at a verification step rather than emit unverified extraction code, per this plan's own Global Constraints — that is a scope boundary, not a placeholder, since AGENTS.md instructs omitting an unverified conversion rather than approximating it.
