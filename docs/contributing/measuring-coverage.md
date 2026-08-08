# Measuring Extraction Coverage

How OxiDex measures what it actually extracts, and what to do when a new file
type shows up.

::: warning Definitions are not coverage
Two different numbers get called "coverage", and conflating them has produced
wrong claims in this repo before.

**Tag definitions** — how many tags `oxidex-tags-*` knows exist. Counted from
YAML, exact, and *not evidence of capability*. `src/tag_sync` ingests
`exiftool -f -listx`, the documentation view: it carries `count encoding id
index lang name type version writable` and no layout at all. No
`SubDirectory`, no `FORMAT`, no `FIRST_ENTRY`, no `ValueConv`, no `Condition`.
It can say a tag exists; it can never say how to read one.

**Extraction coverage** — what OxiDex pulls out of real files, measured by
diffing against ExifTool. This is the only number that describes capability.

Never divide the first by ExifTool's published tag count and call the result
parity. That ratio moves for reasons unrelated to capability, and it read 58%
while real extraction was 48.8%.
:::

## The measurement

[`tools/exiftool-tables/conformance.py`](https://github.com/swack-tools/oxidex/blob/main/tools/exiftool-tables/conformance.py)
runs both tools over the same files and classifies every difference. The
classes matter more than the score, because they cost wildly different amounts
to fix:

| Class | Meaning | Cost to fix |
|-------|---------|-------------|
| **Match** | Same tag name, same value. | — |
| **Rename** | OxiDex read the value correctly under a name ExifTool does not use. Value-confirmed. | Rename a constant. Free coverage. |
| **Value** | Both emit the tag, values disagree. | Usually a `PrintConv` gap. |
| **Missing** | ExifTool emits a tag OxiDex does not. | Real extraction work. |
| **Extra** | OxiDex-only, no plausible counterpart. Outside the denominator. | — |

`score = Match / (Match + Rename + Value + Missing)`, and `ceiling` is the
score once every rename is corrected. **A wide score-to-ceiling spread means
naming debt, not parsing work** — check it before costing anything.

Rename detection uses the generated tables as the name universe and requires
the *values* to match. Name similarity alone would guess, and guessing is how
you get a confident wrong mapping.

## Running it

```bash
just docs-coverage
```

That builds `oxidex`, clones the pinned ExifTool if the cache is cold, scores
both corpora, and rewrites `docs/reference/tag-coverage-analysis.md`. CI runs
the identical command set on every push to `main` that touches `src/**`.

For tag definitions only — no build, no ExifTool, and it prints to stdout
rather than touching the committed report:

```bash
just docs-coverage-definitions
```

## The corpora

Three tiers, and which ones are reachable from CI is the whole design
constraint.

| Corpus | Size | Available in CI | Scored by default |
|--------|------|-----------------|-------------------|
| `tests/fixtures` | 44 media files | ✅ checked in | ✅ |
| ExifTool's `t/images` | 194 files, ~126 formats | ✅ from the pinned clone | ✅ |
| `<cache>/combined-samples` | ~4,200 files by manufacturer | ❌ local dev cache | opt-in |

`<cache>` is `$EXIFTOOL_CACHE_DIR`, default `/tmp/oxidex-exiftool-cache`.

**ExifTool's own `t/images` is the format-breadth corpus.** It ships in the
release tree already cloned for the oracle, so it costs nothing extra, and it
is pinned to the same version the transcriptions came from — it cannot drift
from the oracle grading against it.

Where the recipe finds it depends on how your cache was populated. A git clone
carries `t/images`; a tarball extract may not. So `just docs-coverage` uses
`<cache>/exiftool/t/images` when present, and otherwise clones to
`<cache>/exiftool-corpus-<version>`. It **never deletes `<cache>/exiftool`** —
that is the shared oracle tree that `just compare-exiftool-full` populates and
that the coverage loop reads afterwards, and a missing corpus is not evidence
the oracle is broken. The version assertion runs against whichever tree the
samples actually came from, because sample files change between releases and a
corpus from the wrong version is the same skew problem as an oracle from the
wrong version.

**Why `tests/fixtures` alone is not enough.** It covers 6 formats and scored
96.0%. That number was true and badly misleading: adding `t/images` dropped the
honest figure to 77.5% across 126 formats, and every format dragging it down
(CRW 1.9%, DICOM 3.0%, FIT 0%) was one the narrow corpus never touched. A
corpus that only contains what you already handle will always report that you
handle everything.

**The deep corpus is opt-in and never feeds the committed report:**

```bash
OXIDEX_DEEP_CORPUS=1 just docs-coverage
```

It is a local developer cache populated by `just compare-exiftool-full`, absent
on CI. Generating the committed doc from a corpus CI cannot see would make the
published number unreproducible, so the report is always built from the two
pinned corpora. Use the deep corpus to investigate MakerNote coverage, then
discard the regenerated file.

## Adding a new file type

This is the part designed to need no configuration.

**Drop the sample in and it is scored.** The filter is a deny-list
(`--exclude-ext sh,md,py,json`) of things that are never metadata — mock
scripts, notes, baselines. Everything else is measured, including formats added
later. This is deliberate: an allow-list silently omits each new format until
someone remembers to extend it, and that omission looks exactly like passing.

So, in order of preference:

1. **Already in ExifTool's `t/images`?** Nothing to do. Roughly 126 formats are
   there. Run `just docs-coverage` and the new format appears in the table.
2. **Not there?** Add a sample to the matching `tests/fixtures/<format>/`
   directory. Keep it small and redistributable — this repo is public, so no
   copyrighted or personal images.
3. **Adding a whole new fixture directory?** Nothing to register. Both corpus
   roots are walked recursively.

### Then raise the floors

The one thing that *does* need a human. `--min-files` / `--min-tags` are the
degraded-oracle guard:

```yaml
--min-files 200
--min-tags 10000
```

A broken oracle does not crash. It reads a fraction of the corpus and reports a
confident, precisely-formatted, completely wrong percentage — measured once at
109,261 tags over 832 files where a working oracle got 507,295 over 4,230, with
nothing about the output looking wrong. The floors sit roughly 15% under the
current run: close enough to catch a degradation, loose enough to survive a
fixture being retired.

**After growing the corpus, raise them.** A floor left at the old corpus size
still passes when half the new corpus silently fails to parse. Both
[`justfile`](https://github.com/swack-tools/oxidex/blob/main/justfile) and
[`.github/workflows/update-coverage-docs.yml`](https://github.com/swack-tools/oxidex/blob/main/.github/workflows/update-coverage-docs.yml)
carry a copy — update both.

### A matching `-ver` is not a working oracle

The pinned tree's `exiftool` starts `#!/usr/bin/env perl`, which can find a perl
with no `Archive::Zip`. ExifTool then reports `FileType: ZIP` for a `.docx` and
every container format degrades at once, *while `-ver` still prints the right
release*. Both CI and the justfile therefore probe capability, not just version,
and they do it **even on a cache hit** — a corrupted cache entry must fail the
job rather than sail through.

Never invoke a bare `exiftool`. `PATH` has resolved to 13.55 while the tables
were transcribed from 13.59, and the two disagree about which sub-table a given
byte count selects; sixteen correct Canon R6 Mark III tags were reported as
regressions. The failure is symmetric — the same skew manufactures phantom
*fixes* — and neither is distinguishable from the real thing afterwards.
[`.exiftool-version`](https://github.com/swack-tools/oxidex/blob/main/.exiftool-version)
is the only source of truth; everything resolves it at run time.

## Debugging one format

Narrow to a single format with the allow-list, and print the worst files:

```bash
uv run tools/exiftool-tables/conformance.py /tmp/oxidex-exiftool-cache/exiftool/t/images --ext crw --show 3 --oxidex ./target/debug/oxidex
```

`--only` filters by filename substring, `--json-out` writes the raw counts, and
`--exiftool-dir` points at a specific checkout. Multiple corpus roots are
accepted positionally and scored as one corpus; a root that does not exist is a
hard error rather than a silent skip, since dropping one would shrink the
denominator and report a score for a corpus nobody chose.

## Related

- [ExifTool Tag Coverage](/reference/tag-coverage-analysis) — the generated report
- [JPEG Tag Matrix](/reference/jpeg-tag-matrix) — deeper per-tag JPEG comparison, read *and* write round-trips
- [JPEG Tag Support](/reference/jpeg-tag-support)
