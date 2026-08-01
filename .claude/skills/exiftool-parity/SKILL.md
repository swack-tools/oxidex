---
name: exiftool-parity
description: Use when verifying oxidex metadata output against real ExifTool — checking tag coverage gaps, confirming a parser fix closed a gap without regressions, running or debugging the tag-comparison or jpeg-tag-matrix harnesses, comparing a single file's tags by hand, or locating ground-truth sample corpora, caches, and baselines.
user-invocable: false
---

# ExifTool Parity Verification

## Overview

Parity = same `Group:TagName` keys and values as real ExifTool. `tag-comparison` diffs oxidex **in-process via the library** (`format_for_exiftool()`) against `exiftool -json`; `jpeg-tag-matrix` drives the oxidex and exiftool CLIs.

## When to Use

- Verifying a parser change closed a tag gap; measuring coverage; before claiming parity in commits.
- NOT for wiring a new tag (see the wire-tag skill) or unit tests that never touch exiftool.

## Tag knowledge != tag coverage

Two different ExifTool views feed this repo. Confusing them explains most "we already know this tag, why is it missing?" confusion:

| Source | Gives you | Omits |
|---|---|---|
| `exiftool -f -listx` → `src/tag_sync` | `count encoding id index lang name type version writable` | **everything about layout** |
| Perl symbol table → `src/exiftool_tables` | `FORMAT`, `FIRST_ENTRY`, per-field `Format`, `Mask`, `SubDirectory` edges, `Condition`, `ValueConv` | conversions it refuses to approximate (counted, not silent) |

`-listx` is the *documentation* view: it can say a tag exists but never how to read one. Verified empirically against 13.30 — `-listx` output contains zero occurrences of `SubDirectory`, `FIRST_ENTRY`, `ValueConv`, `Condition`, or `DataMember`. So a rising `oxidex-tags-*` count is **not** evidence of rising extraction coverage; only a comparison run is.

Corollary for gap work: before writing a parser, check whether `oxidex::exiftool_tables::find_table(module, table)` already carries the layout. Re-deriving by hand a binary record ExifTool already declares is the expensive way to close a gap. See `docs/TRANSCRIPTION.md`.

## Measuring the gap by kind

`tools/exiftool-tables/conformance.py <corpus> --exiftool-dir <src> --oxidex <bin>` classifies every difference as RENAME / MISSING / VALUE / EXTRA and prints a `ceiling` column — what each format would score if every rename were fixed. A large score-to-ceiling spread means free coverage (string edits), not parsing work. ExifTool's own `t/images` (~190 files) is a ready-made corpus and `perl -Ilib ./exiftool` runs straight from an unpacked tarball.

## Quick Reference

| Task | Command |
|---|---|
| Version check | `exiftool -ver` vs `EXIFTOOL_VERSION` pin in `.github/workflows/jpeg-tag-matrix.yml` vs `/tmp/oxidex-exiftool-cache/.exiftool-version` vs `EXIFTOOL_VERSION` in `src/exiftool_tables/binary_tables.rs` |
| Verify generated tables | `just verify-tables` (reads its release from the stamp; fetches if uncached) |
| Regenerate tables | `just regen-tables [version]` (extract → codegen → independent verify) |
| Gap by kind (rename vs missing) | `python3 tools/exiftool-tables/conformance.py <corpus> --exiftool-dir <src> --oxidex <bin>` |
| Build main harness | `cargo build --release --bin tag-comparison --features tag-comparison-binary` |
| Fixloop rebuild | `--profile fixloop` instead of `--release` → `target/fixloop/tag-comparison` |
| Full-corpus comparison | `just compare-exiftool-full` (persistent cache; writes `comparison.json`) |
| One format | `just compare-exiftool-format JPEG` |
| Gap report | `uv run scripts/find_tag_gaps.py [--only-format NAME] [--cache-dir DIR]` |
| Comparison integration tests | `just test-comparison` (`cargo test --release --features exiftool-comparison -- --nocapture`) |
| Build JPEG matrix | `cargo build --release --features jpeg-tag-matrix-binary --bin jpeg-tag-matrix` |
| JPEG write matrix | `./target/release/jpeg-tag-matrix manifest --flag-noops` / `run --workers 8` / `report --check-baseline` (ratchet: `report --update-baseline`) |
| Coverage doc | `just docs-coverage` |

Full flags, env vars, per-recipe corpora: `references/harnesses.md`.

## Data Locations

| Location | Contents |
|---|---|
| `tests/fixtures/` | Committed: `jpeg/` (incl. `tag_matrix_base.jpg`, `makernotes/`, `edge_cases/`), `png/`, `tiff/`, `mp4/`, `pdf/`, `raw/`, `jpeg-tag-matrix/` stubs, `manifest.json` |
| `test_data/audio/` | `sample.flac` |
| `/tmp/oxidex-exiftool-cache/` | `exiftool/` checkout, `.exiftool-version` (at cache root), `combined-samples/`, `samples-<Mfr>.tar.gz`, `exiftool-tag-cache/`, `oxidex-tag-cache/` |
| `docs/reference/` | Committed: `jpeg-tag-baseline.json`, `jpeg-tag-matrix.md`, `jpeg-tag-support.md`, `tag-coverage-analysis.md`. Gitignored generated: `docs/reference/comparison/`, repo-root `comparison.json` |

## Single-File Manual Comparison

```bash
exiftool -G1 -s FILE                 # ground truth (quote in commits)
exiftool -json -a -G1 -struct FILE   # integration-test flags
./target/release/oxidex -j -e FILE   # JSON, exiftool-compat formatting
```

`-G` = family 0 (`EXIF:Make`); `-G1` = family 1 (`IFD0:Make`). `oxidex -j -e` emits family-1-style groups — compare against `-G1`; `tag-comparison` uses `-G` and reconciles internally.

## Verifying a Fix

1. Before: run the start-snapshot `tag-comparison` command (`references/harnesses.md`) → `/tmp/tagcmp-<F>-start.json`.
2. After the fix, re-run with `-end` names. Require: `missing_in_oxidex` + `value_differences` strictly lower AND `regressions` empty.
3. `cargo fmt --all` + `cargo test --workspace`; quote the real `exiftool -G1 -s` value in the commit.

Automated version: `.claude/workflows/exiftool-coverage-loop.js` (same cache dir and protocol).

## Common Mistakes

- Without `--features tag-comparison-binary`/`jpeg-tag-matrix-binary`: explicit `--bin` errors loudly; bare `cargo build`/`cargo test` silently omits the target.
- Comparison tests without `--features exiftool-comparison` are `ignore`d — "0 failed" proves nothing.
- `oxidex -j` without `-e` — formatting and groups differ from exiftool; always add `-e`.
- JSON display: `-json` prints `[1,2]`, plain ExifTool prints `1, 2` — verify against plain `exiftool FILE`.
- `tag-comparison` never runs the oxidex CLI — CLI-only bugs surface only via `jpeg-tag-matrix` or manual `oxidex -j -e`.
- `find_tag_gaps.py --only-format` needs a cache dir already populated by `just compare-exiftool-full`.
- Version skew: homebrew exiftool, the CI `EXIFTOOL_VERSION` pin in `.github/workflows/jpeg-tag-matrix.yml`, and the `/tmp` cache checkout (`.exiftool-version`) can all differ and explain phantom gaps — check all three before debugging.
