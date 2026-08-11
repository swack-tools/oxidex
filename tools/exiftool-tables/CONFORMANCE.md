# conformance.py — measuring the ExifTool gap by *kind*, not just size

```sh
python3 tools/exiftool-tables/conformance.py <corpus> --recursive --exiftool-dir <exiftool-src>
```

Use `--recursive` for a corpus root. It is harmless for a flat directory, and
without it the command scores only files directly below `<corpus>`; the shared
`combined-samples` corpus is organized into manufacturer subdirectories.

`--exiftool-dir` is an unpacked ExifTool source tree. No installation is needed:
`perl -Ilib ./exiftool` runs straight from the tarball, and ExifTool ships ~190
sample files in `t/images` covering BMP through CR2 — a full differential corpus
and a runnable oracle at no cost.

## Why

The comparison report says *which* formats score badly. It does not say why, and
the why decides what the work actually costs:

| class     | meaning                                       | cost              |
| --------- | --------------------------------------------- | ----------------- |
| `RENAME`  | value read correctly, under a different name  | a string edit     |
| `MISSING` | ExifTool emits it, OxiDex does not            | real parsing work |
| `VALUE`   | both emit it, values disagree                 | usually PrintConv |
| `EXTRA`   | OxiDex-only, no counterpart                   | investigate       |

A tag-at-a-time fix loop cannot distinguish these, so it pays full price for
renames — the cheapest class there is.

**BMP scoring 0% was entirely renames.** OxiDex parses BMP correctly and calls
the tags `Width`/`Height` where ExifTool says `ImageWidth`/`ImageHeight`. There
was no parsing work to do.

The `ceiling` column shows what each format would score if every rename were
corrected, so free coverage is visible separately from real work.

## Rename inference is deliberately conservative

A pair is reported only when the values agree, the pairing is unambiguous in
*both* directions, and either the names normalise to the same string or the
value is distinctive enough to stand alone.

The first version matched on value alone and produced crossed nonsense —
`Blue -> RedTRC` *and* `Red -> BlueTRC` in the same file, because all three ICC
curves hold identical data, and `Height -> Aperture` because both happened to be
8. Guessing a rename is worse than reporting nothing: it sends someone to "fix"
a correctly-named tag.

## Matching is group-qualified

A same-named tag from two different groups (`EXIF:Make` vs `IFD0:Make`,
`MPC:Year` vs `APE:Year` vs `ID3:Year`) is paired across groups only when the
value confirms it (a harmless group alias) or the name was unique on both
sides of the file to begin with. Anything left over is reported as MISSING +
EXTRA rather than guessed.

Before this, the matcher would grab whatever OxiDex tag was left over under a
same-sounding name regardless of how many unrelated candidates were
competing — bare-name comparison is group-blind. On a single-file `APE.mpc`
corpus this manufactured 10 false VALUE diffs and one false cross-group MATCH
out of a file that is really 11 `MPC:*` MISSING + 11 `APE:*` MISSING (OxiDex
has no MPC/APE parser wired up), `ID3v1:*` EXTRA (OxiDex reads the ID3v1
trailer ExifTool's own JSON writer drops once ID3v2 outranks it), and zero
real VALUE differences. See `test_conformance.py` for the pinned regression
and `GROUP_QUALIFIED_DELTA.md` for the corpus-wide before/after.

## EXTRA is a precision axis, not a recall penalty

`score`/`ceiling` are computed over matched + value + missing + renames only;
EXTRA never enters that denominator, so a format cannot buy a better score by
inventing tags, and correctly-scoped output is not punished on recall for
having zero of them. It is reported instead — a `precision` column plus an
"OxiDex-only tags" vote table, mirroring the missing-tags one — so later
stages (Stage 4's default-mode EXTRA-budget gate) have something to read.

## VALUE differences are severity-classed

Every real value_diff carries one of six labels: `identity` (same string
modulo case/whitespace), `date_time`, `binary`, `numeric`, `display_only` (one
side looks like a PrintConv of the other, e.g. `"5"` vs `"5 (Standard)"`), or
`structural` (the fallback — genuinely different data). This does not change
any count; it lets a reviewer tell a rounding nit from a wrong decode without
opening every file by hand.

## Seams for later steps

`parser_status()` and `family_views()` in `conformance.py` are deliberately
inert today (`None` from parser_status, `None`/`{}` from family_views and in
`--json-out`). Step 13's `ReadReport` (Parsed/Partial/IdentifiedOnly/
Unsupported) will fill the first, so a format's parser status renders per
format instead of only being inferable from "everything is MISSING". Stage
4's `TagOccurrence` store will fill the second, so this instrument can report
both a family-0 ExifTool-compatible view and a family-1 OxiDex-structural
view of a comparison without another rewrite of `compare()`.

## Notes on comparison fairness

* ExifTool is run **without** `-n`, so both sides apply their print conversions.
  Comparing converted output against raw values reports every correctly-read tag
  as a value mismatch.
* Tags describing the file on disk (paths, timestamps, the tool's own version)
  are ignored; they differ by construction and would swamp the signal.
* Scores against ExifTool's own `t/images` run below those in the published
  comparison report, because that corpus is deliberately exotic. It is a harsher
  yardstick. What matters is using the same corpus before and after a change.
