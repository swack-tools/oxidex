# ExifTool parity

OxiDex aims to print what ExifTool prints: the same tag names, groups and
values, for every tag ExifTool reads. This page explains how that is
measured, what the words mean, and where the numbers stand. The current
figures, refreshed from committed measurements, are on the
[status page](/status/).

## Pinned to ExifTool 13.59

"ExifTool" always means one exact release, the one named in
`.exiftool-version` at the repository root: **13.59**. Every other tool
reads that pin:

- the tables are transcribed from that release's Perl source;
- the Rust oracle (`src/exiftool_oracle.rs`) compiles it in;
- the Python oracle (`scripts/exiftool_oracle.py`) reads it;
- CI and the `just` recipes fetch exactly that tag.

Why so strict: two ExifTool releases can disagree. Grading against 13.55
while the tables came from 13.59 once reported sixteen correct Canon R6
Mark III tags as regressions, because the two releases select a different
sub-table for the same byte count. The same skew manufactures phantom
*fixes*, and afterwards neither kind can be told apart from the real thing.

A matching version string is also not enough. The pinned tree's `exiftool`
script starts with `#!/usr/bin/env perl`. If that finds a perl without
`Archive::Zip`, ExifTool reports `FileType: ZIP` for a `.docx` and every
container format degrades, while `-ver` still prints 13.59. So the oracles
also run a **capability probe**: `OOXML.docx` must identify as `DOCX`. A
degraded run does not crash. It reports a confident, precisely formatted,
completely wrong number.

::: tip Comparing by hand
Use `just compare-file <path>`, which diffs one file against the pinned
oracle and names both tools in its output. Do not use an `exiftool` found on
your `PATH`.
:::

## What "proven read" means

The strictest measure is per **catalog entry**. A catalog entry is one
ordinary tag entry in ExifTool 13.59's own source tables: a
`(table, tag ID, variant)` coordinate. There are **33,487** of them.

An entry is a **proven read** when the authenticated corpus read receipt
(`tools/exiftool-tables/corpus_read_receipt.py`) credits it:

1. OxiDex and the pinned ExifTool both read every file of the corpus, the
   194 files of ExifTool 13.59's own `t/images`. Each tool runs in both
   print-converted mode and raw (`-n` / `--no-print-conv`) mode.
2. A `Group1:TagName` identity *matches* in a file only when both modes
   carry identical values. Values are compared as parsed JSON, with no loose
   normalisation. A match in print mode alone is never credited.
3. A capture script names the exact source-table row behind every tag
   ExifTool emitted. A coordinate is credited only if **every** file in
   which ExifTool read it matched. One failure anywhere withholds the credit.

The receipt refuses to run from a dirty checkout, and it records the build,
the transcripts and the oracle's version and capability. Its result is
published as `docs/public/measurements/catalog-corpus-observed-13.59.json`.

| Measure (instrument) | Value |
| --- | --- |
| Catalog entries proven read (`catalog-corpus-observed-13.59.json`, #822) | **2,378** of 33,487 |
| … of the entries ExifTool itself reads anywhere in the corpus | 2,378 of 4,270 (55.7%) |
| Entries ExifTool reads in the corpus that OxiDex does not yet match | 1,892 |
| Entries no corpus file exercises (a gap in the corpus, not in OxiDex) | 29,217 |
| `Group1:TagName` identities matched in both modes (same receipt) | 3,089 |

Most of the catalog is unexercised because ExifTool's test corpus is small.
An unexercised entry is not counted as a failure, and it is not counted as
a success either.

## Per-file conformance

A looser, per-file view comes from `tools/exiftool-tables/conformance.py`.
It classifies every difference between the two tools' output on a set of
files:

- **MATCH**: same tag, same value.
- **VALUE**: same tag, different value.
- **MISSING**: ExifTool emits the tag and OxiDex does not.
- **RENAME**: the value is there under a different name.
- **EXTRA**: OxiDex emits a tag that ExifTool does not.

The most recent recorded run is the 13.59 control of the upgrade rehearsal:
commit `66e48654`, the same 194 files, and the oracle under perl 5.38.2.

| MATCH | VALUE | MISSING | RENAME | EXTRA | MATCH / (MATCH+VALUE+MISSING+RENAME) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 10,149 | 30 | 1,400 | 7 | 651 | 87.60% |

This ratio counts tag occurrences per file, not catalog entries, so it is
not comparable with the proven-read count above. Read MISSING as work not
yet done. Read VALUE as defects: a wrong value under a real ExifTool name is
worse than an absent tag.

## Guarantees that stop regressions

- **Corpus read-regression gate** (CI, every PR): a change that loses any
  proven read fails, and the lost entries are named.
- **Parity ratchet** (CI, every PR): 25 committed counts may move only in
  the good direction. Examples are proven reads (at least 2,378) and known
  mismatches (at most 1,892).

## Detected is not parsed

A correct `File:FileType`, `FileTypeExtension` and `MIMEType` do not mean
the file was parsed. OxiDex identifies many more file types than it has
parsers for. A file that is identified but not parsed comes back with its
identity and filesystem tags, and nothing else. See
[Supported formats](/reference/formats/) for which state each format is in.

## Reports

- [Status](/status/): the current figures in one place
- [ExifTool comparison](/reference/comparison/): per-format tables, generated when the site is deployed
- [ExifTool coverage report](/reference/tag-coverage-analysis): the generated conformance table. The committed copy is dated; the page states when it was measured.
- [Corpus read observations](/reference/catalog-corpus-observed): the receipt behind the proven-read counts
- [Autogeneration plan](/AUTOGENERATION-PLAN): every headline number with its instrument and commit
