# Ten Unique Tag Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close ten unique, unclaimed ExifTool tag gaps with exactly one fresh agent assignment per tag.

**Architecture:** Each task is an independently scoped parser-coverage investigation and fix. The agent must prove the assigned gap against pinned ExifTool 13.59, follow the existing parser/table path, add one focused red-green regression test, and commit only the assigned tag.

**Tech Stack:** Rust, Cargo, OxiDex parser and transcription tables, pinned ExifTool 13.59, Python conformance tooling.

## Global Constraints

- Work only in `/home/allen/git/oxidex/.worktrees/ten-unique-tag-fixes`.
- Exactly one bare tag name belongs to each fresh agent assignment; no neighboring tags may be added.
- Before starting, run `ps -eo pid=,args=` and stop if the assigned bare tag appears in another process's arguments.
- Reserved names are `BatteryLevel`, `ComponentsConfiguration`, `DustRemovalData`, `LensInfo`, `ImageWidth`, `PreviewImageWidth`, `SignType`, `SourceImageWidth`, `ThermalData`, `PreviewImageStart`, `ThumbnailTIFF`, `AntiFlicker`, `DarkFocusEnvironment`, `Annotation`, `AmbientTemperature`, `Azimuth`, `SensorID`, `UniformResourceName`, and `TimeZoneOffset`.
- `.exiftool-version` is the only oracle version source. Never invoke an unpinned system `exiftool`.
- Check `src::exiftool_tables::find_table(module, table)` before deriving binary layout.
- Never approximate a conversion. If exact behavior cannot be established, report `BLOCKED` without emitting the tag.
- Use TDD: add a behavior test, run it and observe the expected failure, implement the minimum fix, then run it green.
- Do not reset, discard, stage, or rewrite another agent's work.
- Write the full task report to the assigned report path and commit only the task's focused changes.

---

Each task follows the same required evidence sequence:

1. Run the live-process collision check for the exact assigned name.
2. Reproduce the gap on a pinned ExifTool test image and record fixture, family/group, oracle value, and current OxiDex result.
3. Classify it as `RENAME`, `MISSING`, `VALUE`, or `EXTRA` and inspect the transcription table plus the existing parser path.
4. Add the smallest real-behavior regression test that would fail if the extraction path were removed or decoded incorrectly.
5. Run the focused test before production changes and record the expected failure.
6. Implement only the assigned tag and run the focused test green.
7. Run the relevant parser/module tests and a pinned before/after comparison for the fixture.
8. Run `cargo fmt --check` and `git diff --check`, then commit with a `Tag: <group:name>` trailer.

### Task 1: CR2 `AFAreaHeights`

**Scope:** Canon CR2 MakerNote extraction for the bare tag `AFAreaHeights` only.

### Task 2: DNG `AnalogBalance`

**Scope:** DNG/TIFF extraction or exact conversion for the bare tag `AnalogBalance` only.

### Task 3: JPEG `CameraType`

**Scope:** JPEG segment or embedded metadata extraction for the bare tag `CameraType` only.

### Task 4: NEF `BlueMatrixColumn`

**Scope:** Nikon NEF embedded ICC extraction or exact conversion for the bare tag `BlueMatrixColumn` only.

### Task 5: PDF `FNumber`

**Scope:** PDF metadata/XMP extraction or exact conversion for the bare tag `FNumber` only.

### Task 6: X3F `ExposureMode`

**Scope:** Sigma X3F property extraction or exact conversion for the bare tag `ExposureMode` only.

### Task 7: CR2 `AFAssistBeam`

**Scope:** Canon CR2 MakerNote extraction or exact conversion for the bare tag `AFAssistBeam` only.

### Task 8: DNG `CalibrationIlluminant1`

**Scope:** DNG/TIFF extraction or exact conversion for the bare tag `CalibrationIlluminant1` only.

### Task 9: JPEG `FieldOfView`

**Scope:** JPEG composite extraction or exact conversion for the bare tag `FieldOfView` only.

### Task 10: NEF `DeviceAttributes`

**Scope:** Nikon NEF embedded ICC extraction or exact conversion for the bare tag `DeviceAttributes` only.

## Whole-branch gate

After all ten task reports and task-scoped reviews, run `cargo fmt --check`, `cargo clippy`, `cargo test --workspace`, `just verify-tables` for any transcription-sensitive change, and pinned comparisons covering every accepted tag. Confirm the commit/report ledger contains exactly the ten distinct assigned bare names and none of the reserved names.
