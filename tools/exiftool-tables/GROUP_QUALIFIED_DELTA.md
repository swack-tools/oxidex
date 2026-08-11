# Step 12 delta note: bare-name vs group-qualified matching

Dual-run of `conformance.py` before and after the group-qualified matcher
(OVERHAUL_OXIDEX_PLAN.md Step 12), same corpus, same oracle, same binary, run
back to back so nothing else could have moved the numbers.

**Instrument.** `tools/exiftool-tables/conformance.py` (old = commit `91e0ba02`,
new = this commit) against `/tmp/oxidex-exiftool-cache/exiftool/t/images`
(194 files, `--recursive` not needed — flat directory), oracle
`ExifTool 13.59 (pinned, perl /usr/bin/perl5.34, via pinned source tree)`,
`--oxidex target/fixloop/oxidex` (built from this worktree at the point of
the run), floors `--min-files 150 --min-tags 5000` (below this step's own
194-file/~13k-tag reality by a comfortable margin, still high enough that a
degraded oracle — e.g. `Archive::Zip` missing, see AGENTS.md — could not pass
silently). Both runs scored all 194 files; neither hit the floor.

## Corpus totals

| run | files | match | rename | value | missing | extra | score  | precision |
| --- | ----: | ----: | -----: | ----: | ------: | ----: | -----: | --------: |
| old (bare-name)      | 194 | 8586 | 9 | 64 | 1868 | 1632 | 81.6% | n/a (not reported) |
| new (group-qualified)| 194 | 8596 | 9 | 44 | 1878 | 1642 | 81.7% | 84.0% |

value_diff **-20** (64 → 44), missing **+10** (1868 → 1878), matched **+10**
(8586 → 8596), extra **+10** (1632 → 1642, now surfaced as its own column).
Every one of the 20 value_diffs that disappeared was a false pairing across
two genuinely different groups; none of them "went missing" in the sense of
losing a real defect — the true classification for each is now either a
correct MATCH (OxiDex read it right, the old matcher just paired the ET side
against the wrong OxiDex tag) or a real MISSING/EXTRA pair that the arbitrary
punt had been hiding inside a false VALUE diff. Net: +10 matched and +10
missing (formerly-hidden real gaps), and the false-cross-group MATCH the
plan's motivating defect flagged is gone (MPC, below).

Renames are unaffected in count (9 → 9) but PFA's rename target changed group
attribution: `PostScript:Title -> FontName` (old) became `EPS:Title ->
FontName` (new) — the same free-coverage fix, now pointing at the ET group
that's actually left over after group-qualified matching, rather than
whichever group the old fallback happened to leave behind.

New: a severity histogram over the 44 remaining (real) value_diffs —
`structural: 33, numeric: 6, display_only: 4, date_time: 1` — none of which
were reachable before this step (the old report only counted them).

## Files whose classification changed (25 of 194, verified per-file)

Per-format Counter deltas (the table above, and the standard `just`-recipe
report) only surface a *count* change. Checked per-file instead — matched/
value_diff/missing/extra as exact sets, not just their lengths, `old ==
new?` for all 194 — 25 files differ. 169 are byte-for-byte identical between
the two runs: the tiered matcher is a strict refinement of the old one, not a
behavior change for the common case (same-group exact match, unique-name
cross-group value match, unrelated tags with nothing to compare against).

Of the 25: **8 files have different match/value/missing/extra counts**
(these moved the corpus totals above, and are the real defect the plan
flagged) — `APE.mpc`, `Panasonic.rw2`, `RIFF.avi`, `Pentax.avi`, `BPG.bpg`,
`SigmaDP2.x3f`, `ExifTool.jpg`, `XMP.xml`. **17 files have identical counts
but different group attribution** on one or more entries — a secondary,
smaller-stakes fix from the same tier restructuring, detailed after the 8.

### MPC — the motivating defect (`APE.mpc`, 1 file)

| | match | rename | value | missing | extra |
|---|---:|---:|---:|---:|---:|
| old | 16 | 0 | 10 | 17 | 5 |
| new | 21 | 0 | **0** | 22 | 10 |

Old: 10 false VALUE diffs (OxiDex's real `ID3v2:*` reads paired against
unrelated `MPC:*`/`APE:*` names, or against `ID3v1:*` trailer reads, purely
because the punt grabbed whatever OxiDex tag was left in the bucket) plus one
coincidental false MATCH (`APE:Year` 2005 paired via bare value equality
against OxiDex's real `ID3:Year` 2005, before the real `ID3:Year`↔`ID3:Year`
pair got a chance to consume it — see `test_conformance.py`'s
`test_group_qualified_matching_on_the_ape_mpc_regression` for the full
walk-through). New: 11 `MPC:*` MISSING + 11 `APE:*` MISSING (OxiDex has no
MPC/APE parser wired up — Step 32 owns closing this), `ID3v1:*` EXTRA ×6
(OxiDex reads the ID3v1 trailer that ExifTool's own JSON writer drops once
ID3v2 outranks it for the same tag names — not a OxiDex defect), zero VALUE.

### RW2 — a real MATCH recovered from a false VALUE diff (`Panasonic.rw2`, 1 file)

Old: `value_diff: [('BitsPerSample', 8, 12)]` — ET's `File:BitsPerSample=8`
(a derived value) got paired against OxiDex's real `IFD0:BitsPerSample=12`
punt-style, reporting a fabricated defect, while ET's real
`EXIF:BitsPerSample=12` (which OxiDex's `IFD0:BitsPerSample=12` genuinely
matches, group-aliased) fell to MISSING with nothing to pair against. New:
`EXIF:BitsPerSample` correctly MATCHES `IFD0:BitsPerSample` on value (Tier 2,
cross-group, value-confirmed), and `File:BitsPerSample` — which OxiDex does
not compute at all — is now correctly MISSING (a real, actionable gap) instead
of hiding inside a wrong-looking VALUE diff.

### AVI — a real MISSING recovered from a false VALUE diff (both AVI files: `RIFF.avi`, `Pentax.avi`)

Old, `RIFF.avi`: `value_diff: [('Duration', '15.53 s', '15.53')]` — ET's
`Composite:Duration = "15.53 s"` (the unit-suffixed composite) got paired
against whichever of OxiDex's two duplicate `Duration` tags
(`AVI:Duration=15.53`, `RIFF:Duration=15.53`, both bare numbers, both
OxiDex-only) came first in iteration order. New: `Composite:Duration` is
correctly MISSING (OxiDex has no unit-suffixed Duration composite), and
**both** `AVI:Duration` and `RIFF:Duration` are correctly EXTRA — a genuine
duplicate-emission issue on OxiDex's side, now visible instead of one copy
being laundered into a fake value mismatch. `Pentax.avi` has the identical
`Composite:Duration` vs duplicate-`Duration` shape, resolved the same way,
plus a `RIFF:SampleRate/FrameRate/VideoCodec` → `AVI:SampleRate/FrameRate/
VideoCodec` group-label correction (the real leftover group changed, the
count of extras did not). `Pentax.avi` also carries a genuine
`DateTimeOriginal` format difference (`2009:10:27 12:14:00` vs `2009/10/27
12:14:34`, ET colons vs OxiDex slashes) as a value_diff in both runs, now
labeled `date_time`; `RIFF.avi` had no other value_diff to begin with.

### BPG — a real MISSING+EXTRA pair recovered from a false VALUE diff (`BPG.bpg`, 1 file)

Old: `value_diff: [('ComponentsConfiguration', ['Y','Cb','Cr','-'], 'Y, Cb,
Cr, -')]` — ET's `XMP:ComponentsConfiguration` (list form) got paired against
OxiDex's `XMP-exif:ComponentsConfiguration` (joined-string form), even though
ET's `EXIF:ComponentsConfiguration` and `ExifIFD:ComponentsConfiguration`
(both `"Y, Cb, Cr, -"`, string form, matching OxiDex's value exactly) were
sitting right there. New: those two match correctly, `XMP:
ComponentsConfiguration` is MISSING and `XMP-exif:ComponentsConfiguration` is
EXTRA — plausibly a rename (`XMP` → `XMP-exif`) blocked by the list-vs-string
formatting difference, which is itself worth a look, not something this step
resolves.

### X3F — two real MATCHes recovered from two false VALUE diffs (`SigmaDP2.x3f`, one of 2 X3F files)

ET has two different-groups occurrences each of `ImageWidth`/`ImageHeight`:
a placeholder `File:ImageWidth/Height = 8/8` (the file-level decode ExifTool
falls back to when it cannot determine real dimensions) and the real
`SigmaRaw:ImageWidth/Height = 2640/1760`. OxiDex emits only one occurrence of
each, `SigmaRaw:ImageHeight/Width = 1760/2640` — correct and value-identical
to ET's `SigmaRaw:*` entries. Old: `value_diff: [('ImageHeight', 8, 1760),
('ImageWidth', 8, 2640), ...]` — the punt paired ET's placeholder
`File:ImageWidth=8` against OxiDex's only (and correct) `SigmaRaw:
ImageWidth=2640` occurrence, reporting a fabricated defect, while the real
`SigmaRaw:ImageWidth` pair had nothing left to match against and fell to
MISSING (`'SigmaRaw:ImageWidth': (SigmaRaw, 2640)` — a tag OxiDex actually
gets right, reported as absent). New: Tier 1 (exact group + exact value)
pairs `SigmaRaw:ImageWidth`/`Height` correctly on the first pass, before any
punt is considered, and the leftover `File:ImageWidth`/`Height` placeholders
are reported as MISSING under their real group
(`'File:ImageWidth': (File, 8)`) — a defensible real gap (OxiDex doesn't
compute this fallback placeholder), not a fabricated one. Net for this file:
matched 142 → 144, value_diff 4 → 2. The genuine `Megapixels`/`ImageSize`
PrintConv-derived differences and `Sigma.x3f`'s `HyperfocalDistance` rounding
difference (`6.10 m` vs `6.06 m`) remain value_diffs in both runs, now
severity-labeled (`numeric`, `structural`, `structural` respectively).

### JPEG — group-attribution correction, not a count change (`ExifTool.jpg`, one of 41 JPEG files)

`value_diff` lost two entries: `('DateTimeOriginal', '1998:05:01 21:33:18',
'1998:12:31 15:17:20')` and `('ExposureCompensation', 1, '+2.0')`. Both were
false pairings against OxiDex's real `MakerNotes:DateTimeOriginal` and
`MakerNotes:ExposureCompensation` (whose actual ET counterparts are also
`MakerNotes:*`, not the `APP12:*`/`FotoStation:*` labels the old matcher's
punt happened to leave in `missing`). New: those two are correctly MISSING
under `MakerNotes:*` — `missing` stays 54 in both runs, only the group label
attached to 3 of the 54 entries changed to the one that's actually left over.
The two remaining `value_diff` entries on this file (`Comment`'s Unicode
handling, `FocalLength35efl`'s missing "35 mm equivalent" suffix) are real
PrintConv gaps in both runs, now labeled `structural` and `display_only`.

### XMP — a real MISSING+EXTRA pair recovered from two false VALUE diffs (`XMP.xml`, one of 11 XMP files)

Old: `value_diff: [('ShutterSpeed', '0.00469...', '1/213'), ('Aperture',
'9.4', 'f/9.4')]` — ET's raw `XML:ShutterSpeed`/`XML:Aperture` (unconverted
values from this sidecar-style group) got punted against OxiDex's PrintConv'd
`XMP:ShutterSpeed`/`XMP:Aperture` (`'1/213'`, `'f/9.4'`). New: zero value_diff
for this file; `XML:ShutterSpeed`/`Aperture` are correctly MISSING and
`XMP:ShutterSpeed`/`Aperture` are correctly EXTRA — the same "different
representation, not actually the same occurrence" shape as `BPG.bpg`'s
`ComponentsConfiguration` above, not a defect this step resolves.

### The other 17 — same count, corrected group attribution

Root cause, verified on three files: when OxiDex emits the *same value*
under two different groups for one tag name (a real duplicate-emission
pattern on OxiDex's side, unrelated to this step), the old single value-only
tier picked whichever candidate came first in dict-iteration order —
arbitrary, and sometimes the wrong one. Tier 1 (exact group + exact value,
now checked first) always prefers the pairing that is actually correct, so
the *other* duplicate — the genuinely-unexplained one — is what gets
reported as EXTRA or MISSING instead.

* `GIF.gif`: ET has one `GIF:BackgroundColor=0`; OxiDex has two,
  `GIF:BackgroundColor=0` and a bare `BackgroundColor=0`. Old matched the
  bare one (found first) and reported `GIF:BackgroundColor` as EXTRA — i.e.
  pointed at the correctly-grouped tag as if it were the problem. New matches
  `GIF:BackgroundColor` (Tier 1, exact group+value) and reports the bare
  duplicate as EXTRA — the tag that's actually unexplained.
* `PhotoMechanic.jpg`: ET has one `XMP:CountryCode="COD"`; OxiDex has two,
  `XMP:CountryCode="COD"` and `XMP-iptcCore:CountryCode="COD"`. Same
  pattern — old flagged the correct `XMP:CountryCode` as EXTRA, new flags
  `XMP-iptcCore:CountryCode`.
* `OlympusE1.jpg`: ET has `MakerNotes:RedBalance="1.609375"` and
  `Composite:RedBalance="1.609375"` (equal by coincidence); OxiDex computes
  only `Composite:RedBalance="1.609375"`, no raw MakerNotes read. Old matched
  the *first* ET duplicate found (`MakerNotes:RedBalance`) against OxiDex's
  one real value, then reported `Composite:RedBalance` — the tag OxiDex
  actually gets right — as MISSING. New's Tier 1 matches
  `Composite:RedBalance` to `Composite:RedBalance` directly, correctly
  leaving `MakerNotes:RedBalance`/`BlueBalance` (which OxiDex genuinely does
  not extract) as MISSING.

The remaining 14 files show the identical extra-swap shape (same value,
group label corrected, same total count): `MP3.mp3`
(`MPEG:SampleRate`→`MP3:SampleRate`), `Matroska.mkv`
(`Matroska:Duration`→`MKV:Duration`), `Photoshop.psd`/`IPTC.jpg`
(`Photoshop:X/YResolution`→`IFD1:X/YResolution` / `JPEG:X/YResolution`),
`PostScript.eps`/`Font.pfa` (`PostScript:*`→`EPS:*`, matching the PFA rename
noted above), `QuickTime.m4a`/`QuickTime.mov` (`QuickTime:*`→`ItemList:*`,
11–13 tags each — OxiDex's own MP4 atom emits both a `QuickTime` and an
`ItemList` group per tag), `VCard.ics` (`VCard:Method`→`ICS:Method`),
`Vorbis.ogg` (`Vorbis:SampleRate`→`OGG:SampleRate`), `Font.ttf`
(`Font:Copyright`→ bare `:Copyright`), `DjVu.djvu`
(`XMP:Title`/`DjVu:Author`→`DjVu-Meta:Title`/`Author`), `HTML.html`
(54 tags, `HTML:*`→`HTML-office:*`/`HTML-ncc:*`/`HTML-dc:*`/`HTML-prod:*`),
and `OOXML.docx` (21 tags, `XML:*`/`XMP:*`→`OOXML:*`/`DOCX:*`). None of these
change any format's match/value/missing/extra count — confirmed via the
per_format Counter diff, which shows these formats unchanged — only which
specific group label is attached to an already-EXTRA (or, for OlympusE1,
already-MISSING) tag.

## What this does not claim

This is a measurement fix, not a coverage improvement: nothing in OxiDex
changed. The 10 MPC/APE tags that are "newly" missing in the total were
always missing; the old instrument just misreported them as VALUE
differences on the wrong tags. The `matched` increase reflects genuine
matches (`IFD0:BitsPerSample`, `EXIF:ComponentsConfiguration`,
`ExifIFD:ComponentsConfiguration`, the real `ID3:*` pairs on `APE.mpc`, etc.)
that the old matcher had also been misreporting — as MISSING with its pairing
partner consumed elsewhere, not as VALUE diffs, but wrong either way.

## What was and was not run

Run: both matchers (`conformance.py` at commit `91e0ba02` vs this commit)
against the full `t/images` breadth corpus (194 files, `--min-files 150
--min-tags 5000`, both runs scored all 194 and neither hit the floor); a
per-file diff of `compare(et, ox)`'s exact matched/value_diff/missing/extra
output (not just the per-format aggregate) across all 194 files, which is
where the 25-vs-169 and 8-vs-17 splits above come from; the
`test_conformance.py` unit suite (11 tests, all passing) via `python3
tools/exiftool-tables/test_conformance.py`; and the single-file `APE.mpc`
corpus through the CLI end-to-end (`conformance.py
<dir-containing-only-APE.mpc> --show 1 --min-files 1 --min-tags 1`) to
confirm the console/JSON output, not just `compare()` in isolation, produces
the stated classification.

Not run: `just ci-standard`, `just verify-tables`, the jpeg-tag-matrix
ratchet, or `tests/fixtures` as a second corpus root — all deferred to the
orchestrator's centralized gate suite per this step's instructions. This
step touches no parser and no Rust code; `cargo build --profile fixloop --bin
oxidex` was run once to produce the `target/fixloop/oxidex` binary used as
the fixed point for both matcher runs.
