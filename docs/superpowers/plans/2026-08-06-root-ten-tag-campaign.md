# Root Ten-Tag Coverage Campaign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close ten unique pinned-ExifTool `MISSING` tag gaps with one fresh agent and one independently verified commit per tag.

**Architecture:** Each task is an isolated parser correction against one ExifTool 13.59 sample and one bare tag name. Tasks execute serially in the shared integration worktree, with collision checks before dispatch and task-scoped review before acceptance.

**Tech Stack:** Rust, Cargo, OxiDex metadata parsers, Python conformance harness, ExifTool 13.59.

## Global Constraints

- Exactly one fresh implementation agent is assigned to each task and exactly one unique bare tag name.
- Before every dispatch, run `ps -eo pid=,args=` and reject a tag named in another process's arguments.
- Use `/tmp/oxidex-exiftool-cache/exiftool/exiftool`; never invoke an unpinned `exiftool` from `PATH`.
- Check `src::exiftool_tables::find_table(module, table)` before deriving any binary layout.
- Never approximate a conversion; omit the value and report the blocker if exact behavior cannot be established.
- Do not add adjacent tags or unrelated refactors.
- Write a focused failing regression test before production code and commit only the assigned tag's change.

---

### Task 1: APE `Duration`

**Files:** Modify `src/parsers/audio/ape.rs`; test `tests/integration/ape_integration_tests.rs` using `/tmp/oxidex-exiftool-cache/exiftool/t/images/APE.ape`.

- [ ] Confirm pinned ExifTool emits `Duration` and baseline OxiDex omits it.
- [ ] Add a focused assertion for the exact oracle value and run it to observe failure.
- [ ] Implement only exact APE `Duration` extraction, preferring transcribed layout.
- [ ] Run the focused APE tests and pinned single-file conformance comparison.
- [ ] Commit as `fix(ape): extract Duration tag`.

### Task 2: BMP `Planes`

**Files:** Modify `src/parsers/image/bmp.rs`; add or modify the focused BMP integration test using `/tmp/oxidex-exiftool-cache/exiftool/t/images/BMP.bmp`.

- [ ] Confirm the `Planes` gap and exact pinned value.
- [ ] Add and observe a failing `Planes` regression assertion.
- [ ] Extract only `Planes` from the declared BMP header field.
- [ ] Run focused BMP tests and pinned single-file conformance.
- [ ] Commit as `fix(bmp): extract Planes tag`.

### Task 3: ICO `BitsPerPixel`

**Files:** Modify `src/parsers/image/ico.rs`; add or modify the focused ICO integration test using `/tmp/oxidex-exiftool-cache/exiftool/t/images/ICO.ico`.

- [ ] Confirm the `BitsPerPixel` gap and exact pinned value.
- [ ] Add and observe a failing `BitsPerPixel` assertion.
- [ ] Extract only `BitsPerPixel` from the correct ICO directory entry.
- [ ] Run focused ICO tests and pinned single-file conformance.
- [ ] Commit as `fix(ico): extract BitsPerPixel tag`.

### Task 4: ISO `VolumeSize`

**Files:** Modify `src/parsers/archive/iso.rs`; add or modify the focused ISO integration test using `/tmp/oxidex-exiftool-cache/exiftool/t/images/ISO.iso`.

- [ ] Confirm the `VolumeSize` gap and exact pinned value.
- [ ] Add and observe a failing `VolumeSize` assertion.
- [ ] Extract only `VolumeSize` from the ISO descriptor layout.
- [ ] Run focused ISO tests and pinned single-file conformance.
- [ ] Commit as `fix(iso): extract VolumeSize tag`.

### Task 5: M4A `AvgBitrate`

**Files:** Modify `src/parsers/quicktime/metadata_extractor.rs` or `src/parsers/video/mp4.rs` according to existing ownership; add or modify a focused M4A test using `/tmp/oxidex-exiftool-cache/exiftool/t/images/QuickTime.m4a`.

- [ ] Confirm the `AvgBitrate` gap and exact pinned value.
- [ ] Add and observe a failing `AvgBitrate` assertion.
- [ ] Implement only the exact average-bitrate derivation used by ExifTool.
- [ ] Run focused QuickTime tests and pinned single-file conformance.
- [ ] Commit as `fix(m4a): extract AvgBitrate tag`.

### Task 6: MP3 `ID3Size`

**Files:** Modify `src/parsers/audio/mp3.rs`; test `tests/integration/mp3_integration_tests.rs` using `/tmp/oxidex-exiftool-cache/exiftool/t/images/MP3.mp3`.

- [ ] Confirm the `ID3Size` gap and exact pinned value.
- [ ] Add and observe a failing `ID3Size` assertion.
- [ ] Extract only the exact ID3 block size.
- [ ] Run focused MP3 tests and pinned single-file conformance.
- [ ] Commit as `fix(mp3): extract ID3Size tag`.

### Task 7: PCAPNG `OperatingSystem`

**Files:** Modify `src/parsers/specialized/pcap.rs`; add or modify a focused PCAPNG test using `/tmp/oxidex-exiftool-cache/exiftool/t/images/PCAP.pcapng`.

- [ ] Confirm the `OperatingSystem` gap and exact pinned value.
- [ ] Add and observe a failing `OperatingSystem` assertion.
- [ ] Decode only the standard PCAPNG operating-system option.
- [ ] Run focused PCAP tests and pinned single-file conformance.
- [ ] Commit as `fix(pcapng): extract OperatingSystem tag`.

### Task 8: RAM `URL`

**Files:** Modify the existing RealMedia or text dispatch parser selected by current format ownership; add a focused RAM test using `/tmp/oxidex-exiftool-cache/exiftool/t/images/Real.ram`.

- [ ] Confirm the `URL` gap and exact pinned value.
- [ ] Add and observe a failing `URL` assertion.
- [ ] Extract only the RAM URL without broad RealMedia parsing.
- [ ] Run focused RAM tests and pinned single-file conformance.
- [ ] Commit as `fix(ram): extract URL tag`.

### Task 9: WPG `Records`

**Files:** Modify the existing WPG parser or its format-dispatch module; add a focused WPG test using `/tmp/oxidex-exiftool-cache/exiftool/t/images/WPG.wpg`.

- [ ] Confirm the `Records` gap and exact pinned value.
- [ ] Add and observe a failing `Records` assertion.
- [ ] Implement only the exact WPG record count.
- [ ] Run focused WPG tests and pinned single-file conformance.
- [ ] Commit as `fix(wpg): extract Records tag`.

### Task 10: DSS `EndTime`

**Files:** Modify the existing DSS/audio parser or its format-dispatch module; add a focused DSS test using `/tmp/oxidex-exiftool-cache/exiftool/t/images/Olympus.dss`.

- [ ] Confirm the `EndTime` gap and exact pinned value.
- [ ] Add and observe a failing `EndTime` assertion.
- [ ] Implement only exact DSS `EndTime` extraction and formatting.
- [ ] Run focused DSS tests and pinned single-file conformance.
- [ ] Commit as `fix(dss): extract EndTime tag`.

### Task 11: Combined verification

**Files:** Verify all files changed by Tasks 1-10; do not add coverage.

- [ ] Run `cargo fmt --check`.
- [ ] Run relevant Clippy checks and `cargo test --workspace` with `RUSTC_WRAPPER=sccache` when available.
- [ ] Rebuild the release binary and rerun pinned conformance on the ten fixtures.
- [ ] Review `git diff main...HEAD` for scope and confirm ten unique tag commits.
