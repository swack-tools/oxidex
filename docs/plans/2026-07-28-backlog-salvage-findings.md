# Fleet patch backlog: what survived triage (2026-07-28)

The archived backlog at `~/.oxidex/patch-archive/post-drain` held 155 distinct
patches carrying 800 `Tag:` trailers. This records what was actually salvageable,
because the answer is not obvious and the archive will eventually be pruned.

Reproduce with:

```bash
python3 scripts/triage_patch_backlog.py \
    --archive ~/.oxidex/patch-archive/post-drain \
    --comparison /tmp/comparison.json
```

## Verdict: no patch was salvageable, but the gaps they found are

**Zero of 155 patches can be revived.** Two independent findings, each measured:

* **They no longer apply.** 38 of 41 CR2/DNG candidates conflict against current main, including via `git cherry-pick` from the archive bundle with full
  history. They were written against `1ee200e5`; the drain PRs (#162–#170)
  then rewrote the same parser code properly. The patches target code that no
  longer exists in that shape — superseded, not merely stale.
* **The three that do apply close nothing.** `cr2-5c60aa7bc86e`,
  `dng-78353ed24dcc` and `dng-aaee08473013` apply cleanly, build, and move
  `matched_tags` by exactly 0 — gained 0, lost 0 — in every one of the 43
  formats compared.

Only **3 patches** claim a name ExifTool defines nowhere but prints as a display
string (all `WB2`). The refusal rate was never the problem.

## Two measurement traps, both hit while producing this

Recorded because either would have produced a confident wrong answer:

* **`git apply --check --3way` exits 0 on conflict.** It validates that a patch
  parses, not that it merges. It reported all 117 candidates as applying
  cleanly; a real `git apply --3way` showed every one conflicting. Use a
  scratch worktree and a real apply.
* **A stale baseline attributes other people's work to yours.** Measuring 3
  salvaged patches against a `comparison.json` taken before #168 merged showed
  +0.94pp coverage and +59 JPEG tags. Those were #168's FLIR fields, inherited
  by the worktree. Always rebuild the baseline from the same commit the
  experiment branches from.

A third, in the triage script itself: a tag name absent from `exiftool -listx`
is **not** thereby fabricated. ExifTool names some tags at runtime — APP12's
`ucfirst $tag` fallback yields `REV`, `S0`, `STB1`, `TagQ`, `TagR`, all real and
all absent from every table. Judging on absence alone flagged 33 patches; the
correct rule (a tag nowhere **and** a display string somewhere) flags 3.

## The output worth keeping: 70 verified open gaps

Every tag below is confirmed a real ExifTool tag by `-listx`, and confirmed
still missing or still wrong on main by a freshly built `tag-comparison`.
This is a work list, not a patch queue.

### CR2 — 25 tags

```
  AFAreaHeights               AFAreaMode                  AFAreaWidths                AFAreaXPositions
  AFAreaYPositions            AFAssistBeam                AFMicroAdjMode              AFMicroAdjValue
  AmbienceSelection           AntiFlicker                 AspectRatio                 AutoLightingOptimizer
  BatteryType                 BitsPerSample               CR2CFAPattern               CreateDate
  LensInfo                    LensModel                   OwnerName                   PreviewImage
  PreviewImageStart           RawImageSegmentation        SerialNumber                ThumbnailImage
  ThumbnailLength
```

### NEF — 14 tags

```
  BitsPerSample               BlueMatrixColumn            BlueTRC                     CMMFlags
  ColorSpaceData              Compression                 ConnectionSpaceIlluminant   DeviceAttributes
  ImageHeight                 ImageWidth                  PhotometricInterpretation   RowsPerStrip
  SamplesPerPixel             StripOffsets
```

### DNG — 11 tags

```
  AnalogBalance               AsShotNeutral               BitsPerSample               CalibrationIlluminant1
  CalibrationIlluminant2      CameraCalibration1          CameraCalibration2          ColorMatrix1
  ColorMatrix2                Compression                 ThumbnailTIFF
```

### JPEG — 11 tags

```
  CameraType                  DateTimeOriginal            ExposureCompensation        ExposureTime
  FNumber                     FieldOfView                 ID                          ImageSize
  Resolution                  SerialNumber                Zoom
```

### X3F — 7 tags

```
  ColorSpace                  DateTimeOriginal            ExposureCompensation        ExposureMode
  ExposureProgram             ExposureTime                FNumber
```

### PDF — 2 tags

```
  CreateDate                  FNumber
```

## Related

* #171 — removed 16,014 PrintConv display values from the tag registry
* #172 — made `sync_tags` regeneration non-destructive; fixed the
  `tag-name-is-a-printconv-value` rule that shared the false-positive class
  described above
