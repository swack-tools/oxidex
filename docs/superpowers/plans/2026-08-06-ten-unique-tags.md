# Ten Unique Metadata Tags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close and verify exactly ten unique extraction gaps, with one fresh agent assigned to each tag.

**Architecture:** Each task is isolated by format or metadata container and may change only what is required for its assigned tag. Every agent must first consult the transcribed ExifTool 13.59 tables, reproduce the gap with the named fixture, add a regression test that fails for the missing tag, implement the smallest correct extraction path, and verify the focused test.

**Tech Stack:** Rust, Cargo, OxiDex comparison tooling, pinned ExifTool 13.59.

## Global Constraints

- Exactly ten agents and ten unique tag names; one tag per agent.
- Never implement or opportunistically fix a sibling tag.
- Do not use these claimed tags: `BatteryLevel`, `ComponentsConfiguration`, `DustRemovalData`, `LensInfo`, `ImageWidth`, `PreviewWidth`, `SignType`, `SourceImageWidth`, `ThermalData`.
- `.exiftool-version` is the sole ExifTool version authority; never use a bare system `exiftool`.
- Check `src::exiftool_tables::find_table(module, table)` before writing parser layouts.
- Never approximate conversions. Omit the tag if its exact conversion cannot be proved.
- Follow red-green TDD and record both failing and passing commands in the agent report.
- Work only in `/home/allen/git/oxidex-tags10` on branch `tags10-20260806`.

---

### Task 1: `JpgFromRaw` (NEF)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/Nikon.nef`

- [ ] Prove `EXIF:JpgFromRaw` is missing, locate its transcribed layout, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant NEF tests.

### Task 2: `ThumbnailTIFF` (DNG)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/DNG.dng`

- [ ] Prove `EXIF:ThumbnailTIFF` is missing, locate its transcribed layout, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant DNG tests.

### Task 3: `AFMicroAdjValue` (CR2/CR3)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/CanonRaw.cr3`

- [ ] Prove `MakerNotes:AFMicroAdjValue` is missing, locate its transcribed Canon layout, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant Canon/CR3 tests.

### Task 4: `AFPointPosition` (RW2)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/Panasonic.rw2`

- [ ] Prove `MakerNotes:AFPointPosition` is missing, locate its transcribed Panasonic layout, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant RW2 tests.

### Task 5: `Annotation` (DjVu)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/DjVu.djvu`

- [ ] Prove `DjVu:Annotation` is missing, locate the exact container semantics, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant DjVu tests.

### Task 6: `AmbientTemperature` (JPEG APP4)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_ZH20N.jpg`

- [ ] Prove `APP4:AmbientTemperature` is missing, locate its transcribed thermal layout and conversion, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant APP4 tests.

### Task 7: `Azimuth` (JPEG APP5)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/Ricoh2.jpg`

- [ ] Prove `APP5:Azimuth` is missing, locate its exact Ricoh APP5 semantics, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant APP5 tests.

### Task 8: `SensorID` (JPEG APP7)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_M30T.jpg`

- [ ] Prove `APP7:SensorID` is missing, locate its exact DJI APP7 semantics, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant APP7 tests.

### Task 9: `UniformResourceName` (JPEG core)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/Apple/Apple_iPhone16Pro.jpg`

- [ ] Prove `JPEG:UniformResourceName` is missing, locate its exact JPEG segment semantics, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant JPEG tests.

### Task 10: `TrailerSignature` (MIE in JPEG)

**Fixture:** `/tmp/oxidex-exiftool-cache/combined-samples/ExifTool.jpg`

- [ ] Prove `MIE:TrailerSignature` is missing, locate its exact MIE trailer semantics, add and run a failing focused regression test, minimally implement only this tag, then run the focused test and relevant MIE/JPEG tests.
