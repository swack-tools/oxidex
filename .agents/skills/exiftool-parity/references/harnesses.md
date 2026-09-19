# Comparison Harness Internals

Full flag, env-var, and corpus detail for the parity harnesses. The SKILL.md Quick Reference covers the common invocations; read this when running or debugging a harness directly.

## Binaries

| Binary | Feature flag | What it does |
|---|---|---|
| `src/bin/tag-comparison/` | `tag-comparison-binary` (required-features — explicit `--bin` without the feature errors loudly; a bare `cargo build` silently omits the target) | Per-format tag diff + markdown reports + baseline regression detection |
| `src/bin/jpeg-tag-matrix/` | `jpeg-tag-matrix-binary` | Empirical per-tag JPEG read/write round-trip matrix; drives `oxidex` + `exiftool` CLIs |
| `src/bin/generate_baseline.rs` | none | `cargo run --bin generate_baseline -- --input tests/fixtures/ --output tests/baselines/` (also `--update`) |

## tag-comparison flags

`--samples` (default `tests/fixtures`), `--format`, `-o/--output` (default `comparison.json`), `--baseline`, `--exiftool` (default `exiftool`), `--markdown-dir` (default `docs/reference/comparison`), `--exiftool-version` (auto via `exiftool -ver`), `--oxidex-version`.

ExifTool side: `exiftool -json -G` (family-0 groups) batched via stdin. Oxidex side: the **library in-process** through `format_for_exiftool()` (`src/core/exiftool_compat.rs`) — CLI-only bugs never show here.

## jpeg-tag-matrix env

`EXIFTOOL`, `OXIDEX` (default `target/release/oxidex` — build it first), `TAGMATRIX_WORK` (default `$TMPDIR/oxidex-tagmap`; results in `$TAGMATRIX_WORK/results.json`), `TAGMATRIX_BASE` (default `tests/fixtures/jpeg/tag_matrix_base.jpg`). Ratchet baseline: `docs/reference/jpeg-tag-baseline.json`. CI (`.github/workflows/jpeg-tag-matrix.yml`) pins `EXIFTOOL_VERSION` — read the current pin from that file rather than assuming a number. Canonical CI pipeline: `manifest --flag-noops` → `run --workers 8` → `report --check-baseline`; the ratchet step is `report --update-baseline`.

## Coverage-loop snapshot command

Step 1 of the SKILL.md "Verifying a Fix" protocol — the start snapshot:

```bash
./target/release/tag-comparison \
  --exiftool /tmp/oxidex-exiftool-cache/exiftool/exiftool \
  --samples /tmp/oxidex-exiftool-cache/combined-samples \
  --format <F> \
  -o /tmp/tagcmp-<F>-start.json --markdown-dir /tmp/tagcmp-<F>-start-md
```

After the parser change, re-run with `-end` in place of `-start` in both output paths.

## Integration tests

`tests/integration/exiftool_comparison_tests.rs`: run `exiftool -json -a -G1 -struct`, self-skip if exiftool missing, assert `match_rate >= 98.0`. Expected deltas: `tests/integration/KNOWN_DISCREPANCIES.md`. `tests/tag_sync_smoke.rs` also self-skips without exiftool.

## just recipes

| Recipe | Corpus / notes |
|---|---|
| `compare-exiftool` | ExifTool's own `t/images`; ephemeral `/tmp/exiftool-test-$$`, cleaned on exit |
| `compare-exiftool-update` | Same + `--baseline/--output/--markdown-dir` into `docs/reference/comparison` (CI variant) |
| `compare-exiftool-format <F>` | Single format vs `t/images` |
| `compare-exiftool-samples` | exiftool.org manufacturer samples (GCS fallback `storage.googleapis.com/oxidex-samples/exiftool`) |
| `compare-exiftool-full` | `t/images` + 13 manufacturer tarballs; cache `${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}` deliberately NOT cleaned (reused by `find_tag_gaps.py` and the coverage loop) |
| `compare-exiftool-full-update` | Full corpus + docs update |
