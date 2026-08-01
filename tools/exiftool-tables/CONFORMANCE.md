# conformance.py — measuring the ExifTool gap by *kind*, not just size

```sh
python3 tools/exiftool-tables/conformance.py <corpus> --exiftool-dir <exiftool-src>
```

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

## Notes on comparison fairness

* ExifTool is run **without** `-n`, so both sides apply their print conversions.
  Comparing converted output against raw values reports every correctly-read tag
  as a value mismatch.
* Tags describing the file on disk (paths, timestamps, the tool's own version)
  are ignored; they differ by construction and would swamp the signal.
* Scores against ExifTool's own `t/images` run below those in the published
  comparison report, because that corpus is deliberately exotic. It is a harsher
  yardstick. What matters is using the same corpus before and after a change.
