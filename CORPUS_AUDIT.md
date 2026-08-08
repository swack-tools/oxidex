# Full-corpus coverage audit

Measured 2026-07-30 against `origin/main` @ `fbb38f9f`, ExifTool 13.55,
corpus `/tmp/oxidex-exiftool-cache/combined-samples` (4238 files).

Harness: `cargo build --bin tag-comparison --features tag-comparison-binary`,
then one run per format over the whole corpus directory (it recurses).

---

## 0. Read this first: three things that invalidate the obvious reading

**0.1 The corpus is a JPEG corpus.** 4085 of 4238 files are `.jpg`. All 13
vendor subdirectories (`Canon/`, `Sony/`, `Nikon/`, …) contain *only* JPEGs.
Every other format in this audit has **one or two** samples:

| ext | files | ext | files | ext | files |
|---|---|---|---|---|---|
| jpg | 4085 | tif | 2 | x3f | 2 |
| png | 1 | dng | 1 | cr2 | 1 |
| cr3 | 1 | nef | 1 | rw2 | 1 |
| raf | 1 | mrw | 1 | heic | 1 |
| psd | 1 | gif | 1 | webp | 1 |
| mov | 1 | arw | **0** | orf | **0** |
| pef | **0** | | | | |

So for 15 of the 19 requested formats, "full-corpus coverage" **is** the
single-sample number — there is nothing else to measure. Re-running them
cannot move the figure. ARW, ORF and PEF return `0/0` because the corpus
contains **no samples of those formats at all**, not because of a wrong
format name.

**0.2 `--format MOV` returns 0/0 — but so would any correct usage.**
`format_to_extensions` has no `MOV` arm; `.mov` is mapped under `MP4`
(`mp4|m4v|mov`). The one `.mov` sample is measured in the MP4 row. Verified:
`--format MOV` → `0 tags from 0 files`; `--format WebP` works (the lookup
upper-cases, so `WebP`/`WEBP` are equivalent).

**0.3 The harness's own corpus-level numbers are not per-file coverage.**
Both extractors reduce to **one `TagInfo` per `family:name` for the entire
corpus** (`all_tags: HashMap<String, TagInfo>`, first file wins —
`oxidex_extractor.rs:135`, `exiftool_extractor.rs:100`). `ComparisonEngine::compare`
then keys its lookup maps on `family:name` with no file dimension
(`engine.rs:780`). Consequences:

* **Presence is a union.** A tag oxidex emits on *one* file out of 4085 is
  "matched" for the whole corpus.
* **Values are compared across different files.** ExifTool's canonical value
  comes from the first file *it* saw the tag in; oxidex's from the first file
  *it* emitted it in. When those differ you get a phantom value difference.

Measured cost of 0.3 on the only multi-file non-JPEG format:

| CR2 run | matched |
|---|---|
| `CanonRaw.cr2` alone | 102 / 191 (53.4%) |
| `CanonRaw.cr3` alone | 51 / 300 (17.0%) |
| union of the two | **118** |
| both files in one run | **94** / 343 (27.4%) |

24 tags that match when each file is measured alone are reported as value
differences when the two are measured together — 38 of the 39 corpus-level
"value differences" carry `source_file: CanonRaw.cr3` while the single-file
cr3 run has exactly **one**. Nothing regressed; the harness compared a CR3
value against a CR2 value.

Everything below that is labelled *per-file* comes from a separate
measurement that keeps file identity (method in §6). Treat the harness's
corpus numbers as an inventory of *distinct tag names*, not as coverage.

> **Resolved.** The harness now computes the per-file measurement itself
> (`matched_instances / total_exiftool_instances`), and that is what the report
> headlines as **Overall Coverage**; the distinct-key ratio is still published
> beside it but is labelled as a name inventory. Two other inflations were
> fixed at the same time: `extension_to_format` returned `None` for any
> extension it did not name, which silently dropped 83 of the 194 files in
> `t/images` from the run — the formats with no parser first — and formats
> where ExifTool emitted only skipped pseudo-families were printed as
> "0.0% coverage" rather than as unmeasurable. On `t/images` the published
> figure moved 97.1% → 79.8% as a result. The per-file method described in §6
> is still the independent check; `tools/exiftool-tables/conformance.py` scores
> the same corpus at 75.3%.

---

## 1. Raw per-format table (corpus run vs single sample)

`coverage = matched / total_exiftool_tags`, as the harness computes it.

| Format | files | ET keys | matched | coverage | missing | value diffs | extra |
|---|---:|---:|---:|---:|---:|---:|---:|
| JPEG | 4085 | 3689 | 808 | **21.9%** | 2550 | 331 | 360 |
| TIFF | 2 | 89 | 89 | 100.0% | 0 | 0 | 9 |
| PNG | 1 | 11 | 11 | 100.0% | 0 | 0 | 3 |
| DNG | 1 | 265 | 107 | 40.4% | 141 | 17 | 22 |
| CR2 (+CR3) | 2 | 343 | 94 | 27.4% | 210 | 39 | 18 |
| NEF | 1 | 204 | 141 | 69.1% | 38 | 25 | 10 |
| RW2 | 1 | 153 | 68 | 44.4% | 85 | 0 | 7 |
| ARW | 0 | 0 | 0 | n/a | – | – | – |
| ORF | 0 | 0 | 0 | n/a | – | – | – |
| RAF | 1 | 101 | 100 | 99.0% | 1 | 0 | 8 |
| PEF | 0 | 0 | 0 | n/a | – | – | – |
| X3F | 2 | 133 | 52 | 39.1% | 80 | 1 | 4 |
| MRW | 1 | 114 | 38 | 33.3% | 72 | 4 | 6 |
| HEIC | 1 | 24 | 24 | 100.0% | 0 | 0 | 3 |
| PSD | 1 | 91 | 62 | 68.1% | 28 | 1 | 7 |
| GIF | 1 | 35 | 35 | 100.0% | 0 | 0 | 10 |
| WebP | 1 | 16 | 16 | 100.0% | 0 | 0 | 5 |
| MP4 (`.mov`) | 1 | 80 | 80 | 100.0% | 0 | 0 | 32 |
| MOV | – | – | – | – | – | – | – |

### Single-sample vs corpus: where they diverge

Only three formats have more than one sample, so this comparison is only
meaningful for them — plus JPEG, where "the single sample" is whichever
file you happened to pick:

| Format | single sample | corpus | divergence |
|---|---|---|---|
| **JPEG** | 35.9% – 89.3% depending on the file | 21.9% | **up to 67 points** |
| CR2 | 53.4% (cr2) / 17.0% (cr3) | 27.4% | 26 points; corpus is *below* the union (see §0.3) |
| X3F | 69.0% (`Sigma.x3f`) / 38.3% (`SigmaDP2.x3f`) | 39.1% | 30 points |
| TIFF | 100% / 100% | 100% | none |

The JPEG single-sample spread, measured by running the harness against one
file at a time:

| sample | coverage |
|---|---|
| `FujiFilm.jpg` | 50/56 = **89.3%** |
| `Sony.jpg` | 39/44 = 88.6% |
| `Panasonic.jpg` | 54/69 = 78.3% |
| `Apple.jpg` | 55/72 = 76.4% |
| `Olympus.jpg` | 34/49 = 69.4% |
| `Canon.jpg` | 85/133 = 63.9% |
| `Nikon.jpg` | 34/57 = 59.6% |
| `ExifTool.jpg` | 133/370 = **35.9%** |

**The biggest single-sample-vs-corpus divergence is JPEG**: the number you
get is anywhere from 35.9% to 89.3% purely as a function of which file you
open, against a full-corpus figure of 21.9% and a true per-file figure of
60.8% (§2). Every one of those numbers is "JPEG coverage".

---

## 2. What JPEG coverage actually is, per file

4084 JPEGs parsed (one file fails outright, §5.4). 359 368 ExifTool tag
instances, after excluding the `Composite`/`ExifTool`/`System`/`File`
pseudo-families the harness already skips.

| | instances | share |
|---|---:|---:|
| oxidex emits the tag **and** the value agrees | 218 627 | **60.8%** |
| oxidex emits the tag, value is wrong | 31 118 | 8.7% |
| oxidex does not emit the tag at all | 109 623 | 30.5% |

Per-file key coverage distribution (fraction of that file's ExifTool tags
oxidex emits):

| p0 | p5 | p25 | p50 | p75 | p95 | p100 |
|---|---|---|---|---|---|---|
| 0.0% | 38.5% | 70.7% | **84.3%** | 88.9% | 93.4% | 100.0% |

By vendor directory:

| dir | files | key coverage |
|---|---:|---:|
| FujiFilm | 410 | 88.3% |
| Samsung | 702 | 85.0% |
| GoPro | 23 | 84.4% |
| Panasonic | 477 | 81.7% |
| DJI | 21 | 77.6% |
| Pentax | 145 | 77.4% |
| Apple | 66 | 76.0% |
| Google | 24 | 72.8% |
| Canon | 725 | 66.7% |
| Sony | 760 | 65.4% |
| Leica | 67 | 64.9% |
| Nikon | 307 | 59.0% |
| (corpus root) | 40 | 59.4% |
| **Olympus** | 315 | **43.6%** |

**Why 21.9% and 60.8% are both true.** The 3689 distinct ExifTool keys are
a long tail: 789 of them occur on exactly one file. The harness weights each
key equally, so the tail dominates.

| ET files per key | # keys | keys oxidex emits | instance coverage |
|---|---:|---:|---:|
| 1 | 789 | 137 (17.4%) | 17.4% |
| 2–9 | 1280 | 376 (29.4%) | 22.3% |
| 10–99 | 1070 | 261 (24.4%) | 23.0% |
| 100–499 | 400 | 201 (50.2%) | 46.5% |
| 500+ | 150 | 139 (92.7%) | **85.2%** |

oxidex is good at common tags and poor at rare ones. Which is the right
number depends on the question — but "21.9%" should never be quoted as
"oxidex reads 22% of a JPEG's metadata".

Missing instances by family:

| family | missing | of total | % |
|---|---:|---:|---:|
| MakerNotes | 87 575 | 157 998 | 55.4% |
| EXIF | 16 101 | 177 164 | 9.1% |
| **PrintIM** | 1 425 | 1 425 | **100%** |
| MPF | 1 068 | 7 533 | 14.2% |
| XMP | 988 | 4 771 | 20.7% |
| **FlashPix** | 750 | 750 | **100%** |
| ICC_Profile | 452 | 4 261 | 10.6% |
| **Photoshop** | 341 | 341 | **100%** |
| APP6 | 176 | 176 | 100% |
| APP1 (FLIR) | 157 | 221 | 71.0% |
| JUMBF | 120 | 120 | 100% |
| IPTC | 107 | 755 | 14.2% |

Twenty families are emitted **zero times**: PrintIM, FlashPix, Photoshop,
JUMBF, PanasonicRaw, CanonVRD, FotoStation, Meta, PhotoMechanic, XML, MIE,
Trailer, and APP2/3/4/5/7/9/13/15. (APP6 is emitted once, against
ExifTool's 176 instances across 8 files.)

---

## 3. The priority list — top 40 JPEG gaps by files affected

`gap` = number of corpus files where ExifTool reports the tag and oxidex
does not emit it at all. `verdict` is what the corpus-level report says
about that tag today — note how little it correlates with the gap size.

| # | tag | gap (files) | ET files | oxidex emits | corpus verdict |
|---|---|---:|---:|---:|---|
| 1 | `EXIF:Compression` | 3825 | 3844 | 19 | vdiff |
| 2 | `EXIF:ThumbnailOffset` | 3792 | 3792 | 0 | missing |
| 3 | `EXIF:ThumbnailLength` | 3792 | 3792 | 0 | missing |
| 4 | `EXIF:ThumbnailImage` | 3789 | 3789 | 0 | missing |
| 5 | `PrintIM:PrintIMVersion` | 1425 | 1425 | 0 | missing |
| 6 | `MakerNotes:FacesDetected` | 727 | 1189 | 462 | vdiff |
| 7 | `MakerNotes:AFAreaMode` | 671 | 1259 | 588 | vdiff |
| 8 | `MPF:PreviewImage` | 664 | 664 | 0 | missing |
| 9 | `MakerNotes:DataDump` | 638 | 638 | 0 | missing |
| 10 | `MakerNotes:PreviewImage` | 635 | 635 | 0 | missing |
| 11 | `MakerNotes:ExposureTime` | 611 | 724 | 113 | vdiff |
| 12 | `MakerNotes:ImageStabilization` | 603 | 1207 | 604 | vdiff |
| 13 | `MakerNotes:ManualFlashOutput` | 591 | 591 | 0 | missing |
| 14 | `MakerNotes:FNumber` | 558 | 671 | 113 | vdiff |
| 15 | `MakerNotes:ThumbnailImageValidArea` | 556 | 556 | 0 | missing |
| 16 | `MakerNotes:NDFilter` | 549 | 549 | 0 | missing |
| 17 | `MakerNotes:CameraTemperature` | 544 | 584 | 40 | vdiff |
| 18 | `MakerNotes:DateStampMode` | 531 | 531 | 0 | missing |
| 19 | `MakerNotes:DigitalZoom` | 529 | 1223 | 694 | vdiff |
| 20 | `MakerNotes:PreviewImageLength` | 510 | 626 | 116 | vdiff |
| 21 | `MakerNotes:PreviewImageStart` | 510 | 625 | 115 | vdiff |
| 22 | `MakerNotes:InternalSerialNumber` | 504 | 925 | 421 | vdiff |
| 23 | `MakerNotes:ValidAFPoints` | 480 | 480 | 0 | missing |
| 24 | `MakerNotes:CanonImageWidth` | 480 | 480 | 0 | missing |
| 25 | `MakerNotes:CanonImageHeight` | 480 | 480 | 0 | missing |
| 26 | `MakerNotes:SelfTimer2` | 434 | 434 | 0 | missing |
| 27 | `MakerNotes:AspectRatio` | 425 | 428 | 3 | **MATCHED** |
| 28 | `MakerNotes:FlashOutput` | 408 | 408 | 0 | missing |
| 29 | `MakerNotes:AFPoint` | 390 | 739 | 349 | vdiff |
| 30 | `MakerNotes:OwnerName` | 389 | 551 | 162 | vdiff |
| 31 | `MakerNotes:AFPointsInFocus` | 383 | 866 | 483 | vdiff |
| 32 | `MakerNotes:PrimaryAFPoint` | 370 | 370 | 0 | missing |
| 33 | `MakerNotes:BatteryLevel` | 359 | 359 | 0 | missing |
| 34 | `MakerNotes:FocalPlaneYSize` | 358 | 358 | 0 | missing |
| 35 | `MakerNotes:ISO` | 355 | 639 | 284 | vdiff |
| 36 | `MakerNotes:FocalPlaneXSize` | 353 | 353 | 0 | missing |
| 37 | `MakerNotes:Quality` | 351 | 1743 | 1392 | vdiff |
| 38 | `MakerNotes:FlashMode` | 350 | 547 | 197 | vdiff |
| 39 | `MakerNotes:FocusMode` | 349 | 2104 | 1755 | vdiff |
| 40 | `MakerNotes:VRDOffset` | 349 | 349 | 0 | missing |

Vendor attribution for the Canon-only block (13, 15, 16, 23–26, 28, 34, 36,
40 — all ≥97% Canon files) makes it one work item, not eleven.

---

## 4. Classification of the top gaps

### (a) Wrong group family — cheap

Intersecting `missing_in_oxidex` and `extra_in_oxidex` by tag *name* is a
high-false-positive test: `MakerNotes:ColorSpace` and `EXIF:ColorSpace` are
different tags that happen to share a name and a value on 248 files. Cameras
routinely write the same value in both places, so "same name + same value +
different family" fires 3103 times on JPEG and is wrong nearly every time.

The reliable signal is a **family oxidex invents that ExifTool never uses**.
There is exactly one on JPEG:

| oxidex family | ExifTool family | instances | files |
|---|---|---:|---:|
| `AROT` | `APP10` | 28 | 14 |

`AROT:HDRGainCurve` / `AROT:HDRGainCurveSize` are byte-identical to
`APP10:HDRGainCurve*`; only the group name differs. Fix is one line — either
rename the emitted group or add `"AROT" => "APP10"` alongside the existing
`FLIR`/`HDR`/`SPIFF` arms in `normalize_family_for_comparison`
(`src/bin/tag-comparison/comparison/engine.rs:11`). **Minutes of work,
14 files.**

A second, larger naming divergence is in MPF (§4b).

### (b) Apparent value difference that is really a missing duplicate

This is the dominant class on the RAW formats, and `exiftool -G1 -a` proves
it in one command.

**NEF** — every one of the 9 `EXIF:*` "value differences" is IFD0-vs-SubIFD1:

```
[IFD0]    ImageWidth 160   BitsPerSample 8 8 8   PhotometricInterpretation RGB   SamplesPerPixel 3
[SubIFD1] ImageWidth 3040  BitsPerSample 12      PhotometricInterpretation CFA   SamplesPerPixel 1
```

oxidex's values match **IFD0 exactly**; ExifTool's `-json -G` reports the
SubIFD1 copy. oxidex is not wrong, it is missing the SubIFD chain.
DNG is identical (IFD0 8×8 thumbnail vs SubIFD 3516×2328 vs SubIFD1
1024×683) — 7 of DNG's 17 value differences, 9 of NEF's 25, and the same
pattern in CR2.

**MPF** — the same shape on JPEG, 737 files. ExifTool groups per image
(`[MPImage1]`, `[MPImage2]`, `[MPImage3]`) and with family-0 `-G` reports
the *last*. oxidex emits per-image tags under different names
(`MPF:MPImage3Offset` / `MPF:MPImage3Size` / `MPF:MPImage3Flags`) plus a
collapsed `MPF:MPImageStart`.

**A real bug hides inside that one.** oxidex's `MPImage*Offset` values are
relative to the MPF header; ExifTool's are absolute file offsets:

| file | ET `MPImageStart` | oxidex | delta |
|---|---:|---:|---:|
| `Apple/Apple_iPhone11.jpg` | 3 154 361 | 3 134 630 | 19 731 |
| `Panasonic/PanasonicDMC-LF1.jpg` | 4 301 312 | 4 266 072 | 35 240 |
| `Sony/SonyDSLR-A390.jpg` | 4 161 541 | 4 107 938 | 53 603 |

For `Apple_iPhone11.jpg` the `MPF\0` marker sits at byte 19 727, so the MP
Endian field — the origin the CIPA MPF spec defines offsets against — is at
19 731, exactly the delta. **oxidex omits the MPF-header base when
converting MP image offsets to file offsets. 689 files, one addition.**
`MPF:MPImageFlags` is a separate PrintConv defect on the same 663 files
(ExifTool `(none)` / `Dependent child image` vs oxidex `Independent` /
`Dependent parent image`).

### (c) Value formatting

8421 tag instances (3.4% of everything oxidex emits) differ only in
presentation. Two rules cover half of them:

| tag | files | ExifTool | oxidex | cause |
|---|---:|---|---|---|
| `EXIF:FocalLength` | 1010 | `4.0 mm` | `3.99 mm` | rational printed unrounded |
| `EXIF:FNumber` | 975 | `1.8` | `1.779999971` | f32 stringified at full precision |
| `EXIF:FocalPlaneXResolution` | 642 | `3443.946188` | `3443.946188341` | same |
| `EXIF:FocalPlaneYResolution` | 629 | `3442.016807` | `3442.016806723` | same |
| `EXIF:ExposureCompensation` | 209 | `-1/2` | `-0.5` | rational not preserved |
| `MakerNotes:AEBBracketValue` | 531 | `+1/3` | `+0.4` | same |
| `MakerNotes:FlashExposureComp` | 559 | `0` | `+0.0` | signed-zero padding |

A single "format a rational the way ExifTool does" change would move
~4000 file-instances. Cheap, and it is in shared value-formatting code
rather than per-format parsers.

### (d) Genuinely unparsed

The rest — and it is where the file counts are largest:

| root cause | tags | files | note |
|---|---:|---:|---|
| **IFD1 (thumbnail IFD) never walked for JPEG** | 4 | ~3800 | ranks #1–#4; `EXIF:Compression`, `ThumbnailOffset`, `ThumbnailLength`, `ThumbnailImage` all come from `[IFD1]` |
| **PrintIM segment not parsed** | 1 | 1425 | zero PrintIM tags emitted anywhere |
| Canon MakerNote table incomplete | 11+ | ~480 each | items 13/15/16/23–26/28/34/36/40, ≥97% Canon |
| Embedded-preview extraction | 3 | 664/638/635 | `MPF:PreviewImage`, `MakerNotes:PreviewImage`, `MakerNotes:DataDump` |
| FlashPix / Photoshop / JUMBF / APP2/4/5/6 segments | ~40 | 750/341/120/… | families never emitted |
| Malformed-IFD recovery | all | 7 | §5.4 |

Ranked by files, the cheap–expensive ordering is:
**IFD1 walk (≈15 200 instances, one code path) → MPF offset base (689 files,
one addition) → PrintIM (1425 files, one segment parser) → rational
formatting (~4000 instances, shared code) → Canon MakerNote block (~5000
instances) → AROT rename (14 files, one line).**

---

## 5. Silent regressions: tags that pass on one sample and fail across the corpus

Answering the question directly: **yes, and the corpus report cannot see
them**, because presence is unioned and values are compared once. These are
tags the corpus report lists in `matched_tags` while disagreeing with
ExifTool on hundreds or thousands of individual files.

| tag | ET files | oxidex emits | value agrees | corpus verdict |
|---|---:|---:|---:|---|
| `EXIF:FocalLength` | 3551 | 3539 | 1936 | MATCHED |
| `EXIF:FNumber` | 3676 | 3666 | 2629 | MATCHED |
| `MPF:MPImageFlags` | 737 | 689 | 26 | MATCHED |
| `EXIF:FocalPlaneXResolution` | 1096 | 1092 | 448 | MATCHED |
| `EXIF:FocalPlaneYResolution` | 1096 | 1092 | 461 | MATCHED |
| `MakerNotes:AutoExposureBracketing` | 572 | 548 | 0 | MATCHED |
| `MakerNotes:AEBBracketValue` | 572 | 548 | 0 | MATCHED |
| `MakerNotes:FlashBits` | 600 | 576 | 33 | MATCHED |
| `MakerNotes:CanonModelID` | 588 | 564 | 55 | MATCHED |
| `MakerNotes:MinAperture` | 522 | 493 | 10 | MATCHED |
| `MakerNotes:TargetAperture` | 505 | 482 | 0 | MATCHED |
| `MakerNotes:AspectRatio` | 428 | 3 | 3 | MATCHED |
| `MakerNotes:FocusDistanceLower` | 477 | 454 | 6 | MATCHED |
| `EXIF:ExposureCompensation` | 3436 | 3430 | 3094 | MATCHED |

`MakerNotes:AspectRatio` is the clearest: emitted on **3** of the 428 files
ExifTool reports it on, and the corpus report calls it MATCHED because those
3 agree. `MakerNotes:AutoExposureBracketing` and `AEBBracketValue` are
MATCHED with **zero** per-file agreement (548 files, ExifTool `Off`/`0`,
oxidex `+0.0` — arguably class (c), but it is being counted as a pass today).

After discounting formatting (§4c), the largest genuine per-file value
defects are:

| tag | files wrong | ExifTool | oxidex |
|---|---:|---|---|
| `MPF:MPImageStart` | 684 | `3154361` | `3134630` |
| `MPF:MPImageFlags` | 663 | `(none)` | `Dependent parent image` |
| `MakerNotes:FocusMode` | 639 | `Single` | `AI Focus AF` |
| `MakerNotes:AutoISO` | 555 | `100` | `0` |
| `MakerNotes:MeasuredEV` | 554 | `-1.25` | `2041.75` |
| `MakerNotes:ImageStabilization` | 552 | `None; Off; 0` | `Unknown (346)` |
| `MakerNotes:MakerNoteVersion` | 531 | `2.11` | `0211` |
| `MakerNotes:FileNumber` | 526 | `118-1861` | `18-2213` |
| `MakerNotes:CanonModelID` | 509 | `DC19/DC21/DC22` | `Unknown (1074255475)` |
| `MakerNotes:BaseISO` | 481 | `100` | `160` |

`MeasuredEV 2041.75`, `AutoISO 0`, `BaseISO 160`-for-`100` and
`FileNumber 18-2213`-for-`118-1861` are misread fields, not formatting —
they point at offset/width errors in the Canon MakerNote block, the same
component as the §3 Canon cluster.

### 5.4 Files oxidex extracts nothing from

7 JPEGs where oxidex emits none of ExifTool's tags (only its own `JPEG:*`
scan-header tags), plus 1 that fails to parse at all:

| file | ET tags | why |
|---|---:|---|
| `Samsung/SamsungSCH-U620.jpg` | 29 | ExifTool warns `Bad format (500) for IFD0 entry 8` and recovers; oxidex abandons the IFD |
| `Samsung/SamsungGT-I9100.jpg` | 5 | `Bad offset for IFD0 InteropVersion`, `Bad format (20) for IFD0 entry 6` |
| `Samsung/SamsungGT-S9402.jpg` | 30 | EXIF tags inlined in IFD0, no ExifIFD pointer |
| `Samsung/SamsungSGH-D980.jpg` | 30 | same |
| `Samsung/SamsungSGH-G608.jpg` | 26 | same |
| `AFCP.jpg` | 20 | IPTC in an AFCP trailer |
| `FotoStation.jpg` | 22 | IPTC + FotoStation segment |
| `FujiFilm/FujiFilmISPro.jpg` | – | `Unsupported format: Format Unknown not yet supported in this iteration` — hard parse failure |

The Samsung cluster is one behaviour: **oxidex fails closed on a malformed
IFD entry where ExifTool skips the bad entry and keeps going.**

---

## 6. Method, and what would make the next audit better

Corpus runs:

```
cargo build --bin tag-comparison --features tag-comparison-binary
./target/debug/tag-comparison \
  --samples /tmp/oxidex-exiftool-cache/combined-samples \
  --format JPEG --output /tmp/full_JPEG.json
```

Per-file measurement (§2, §3, §5) could not use that output, because the
harness discards file identity before writing the report. It was produced by
dumping both sides per file and comparing them offline:

* ExifTool side: `exiftool -json -G -@ -` in batches of 100 — byte-identical
  to `ExifToolExtractor::run_exiftool_batch`, with the same
  `Composite`/`ExifTool`/`System`/`File` families skipped.
* oxidex side: `OxiDexExtractor` with a temporary local patch writing one
  JSONL record per file before the cross-file `HashMap` collapse. The patch
  is **not** part of this change — it exists only to produce these numbers.
* Key normalisation replicated exactly from
  `normalize_key_for_comparison` (the manufacturer→MakerNotes map, the XMP
  namespace map, `FLIR`/`HDR`/`SPIFF`, and the three ICC TRC renames).
* Value agreement reported two ways: exact string equality, and a loose
  comparator approximating `normalize_value_for_comparison`. §2's 60.8% uses
  the loose one; §4c is the difference between them.

Cross-checks performed:

* The corpus JPEG run was executed twice, independently; both produced
  `808/3689 (21.9%)`.
* All group/tag claims were read out of harness JSON or `exiftool -G1 -a`,
  never out of the `oxidex` CLI, and matched on exact `Group:Tag` keys.

### Recommendations (none applied here — this change is documentation only)

1. **Do not report corpus-level coverage from this harness as coverage.**
   Give `ComparisonEngine::compare` a file dimension, or restrict runs to one
   file. As it stands, adding files to a corpus can *lower* a coverage number
   without any code changing (CR2: 118 → 94).
2. **Record how many files each gap affects.** `missing_in_oxidex` keeps only
   the first file that had the tag. The per-file count is *already computed*
   — `exiftool_extractor.rs:109` increments it per occurrence — and then
   thrown away four lines later at `exiftool_extractor.rs:135`
   (`.map(|(tag_info, _count)| tag_info)`). Keeping it would make §3 a
   harness output rather than a one-off analysis.
3. **Fix the extra-tag asymmetry.** `should_skip_family` drops `Composite`,
   `ExifTool`, `System` and `File` on the ExifTool side only. oxidex's 10 281
   `Composite:*` and 44 020 `JPEG:*` instances are therefore counted as
   "extra in oxidex" on every run. Most of the 360 JPEG extras are this.
4. **Get samples for ARW, ORF and PEF.** Three requested formats have no
   corpus coverage whatsoever, and a `0/0` result is indistinguishable from a
   typo in the format name.

Ownership: everything in §3–§5 lives under `src/parsers/`, which this audit
does not own. It is reported, not fixed. The only item in code this audit
could have touched is the `AROT`→`APP10` family alias in
`src/bin/tag-comparison/comparison/engine.rs`, and even that is left alone so
this change stays documentation-only.
