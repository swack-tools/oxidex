# Performance

OxiDex is a compiled Rust binary. Measured against the pinned Perl ExifTool
13.59 on the same machine, it is about **3x faster on a single file** and
**1.8x faster per core** on a real 194-file corpus. The parallel reader
(rayon) adds more on multi-core machines. Those are the only speed claims
this site makes. Every number on this page names the instrument, commit,
ExifTool build and machine it came from.

::: tip Two kinds of numbers, never mixed
- **Committed numbers** come from a local run on the maintainer's
  workstation, under the repository's exclusive heavy-job lock.
  They are in `benches/benchmark_results.md` and in the first table below.
- **CI numbers** come from `.github/workflows/benchmarks.yml` on a shared
  GitHub-hosted runner. They are *indicative* and are never committed.

Different hardware, core count and background load separate the two kinds.
Compare a CI number only with other runs of the same workflow.
:::

## Committed measurement (#821)

| | |
| --- | --- |
| Instrument | `benches/exiftool_comparison.sh` with `benches/instrument_check.py`; hyperfine 1.20.0, `--warmup 5 --runs 30 -N`, both commands in one invocation |
| OxiDex | commit `8f04e288`, oxidex 1.2.1 built with the shipped `[profile.release]` (fat LTO, `codegen-units=1`) |
| ExifTool | 13.59 from the pinned source tree under perl 5.38.2. The `OOXML.docx` capability probe was asserted before timing started. |
| Machine | Apple M5 laptop (10 cores, macOS 27.0), not idle: load1 6.7 to 8.4, recorded per scenario in `benches/benchmark_results.log` |
| Date | 2026-09-18 |

| Scenario | ExifTool (median) | OxiDex (median) | ExifTool / OxiDex |
|----------|------------------:|----------------:|------------------:|
| 112-byte JPEG stub (measures process startup) | 30.8 ms | 8.4 ms | 3.66x |
| `t/images/Canon.jpg -j -a -G1` | 45.5 ms | 15.0 ms | 3.03x |
| Write one tag | 80.8 ms | 14.6 ms | 5.53x |
| Format detection, one JPEG | 30.9 ms | 8.3 ms | 3.74x |
| 1000-file batch `-r` (OxiDex parallel, ExifTool single-threaded) | 1169.6 ms | 273.3 ms | 4.28x |
| 194-file `t/images -j -a -G1`, OxiDex parallel | 704 ms | 114.6 ms | 6.15x |
| same corpus, `RAYON_NUM_THREADS=1` (**like-for-like, per core**) | 707 ms | 386 ms | **1.83x** |

The full tables in `benches/benchmark_results.md` add minima, mean ± σ and
maxima, and give each ratio on both medians and minima. They also record
these caveats:

- On `Canon.jpg`, ExifTool's σ/mean is 28%, because of one 96 ms outlier.
- The rayon rows measure OxiDex using every core against single-threaded
  Perl. The per-core figure is the `RAYON_NUM_THREADS=1` row.
- #830 (lazy magic-regex compilation, a static tag-ID index) and #832
  (Composite dependency resolution) landed after this measurement. Both
  removed costs that the #821 profile identified, and each PR records its
  own before/after measurements. The committed table has not been re-run since.

The profile behind this table is in `benches/spike/DISPATCH_PERF.md`. It
measured the whole generated table engine at 0.66% of a corpus read, and
found the dominant costs were per-process initialisation and Composite
allocation, not tag dispatch.

## Indicative CI measurement (#825)

`.github/workflows/benchmarks.yml` runs the same script on every push to
`refactor/tag-machinery`. It uses the shipped release profile, the pinned
ExifTool and the same capability probe, with hyperfine `--warmup 3 --runs 20`.
The run publishes a step summary and a 90-day `benchmark-comparison`
artifact. The workflow is **non-blocking**: it has no regression threshold,
and it fails only when the measurement itself is refused (a missing or stale
binary, a wrong or degraded oracle, or a dirty tree).

The run that proved the workflow in #825 (run `35355315041`, commit
`24184580`, `ubuntu-24.04`, 4 vCPU AMD EPYC 7763) gave these ratios:
5.59x startup, 5.12x `Canon.jpg`, 11.95x write, 5.39x detection, 5.50x
batch, 7.50x corpus (parallel) and **3.63x corpus single-threaded**. They are
higher than the committed numbers because ExifTool runs about 2.7 to 3 times
slower on that runner, while OxiDex runs only about 1.5 to 1.8 times slower.
That gap is the reason CI numbers are labelled indicative and kept out of
the committed table.

## Criterion micro-benchmarks

The in-process Criterion suites (`parse_benchmarks`, `integration_benchmarks`)
run in `ci.yml`'s `metrics` job **only on pushes to `main`**. That job uses
a CI-only profile (`lto=false`, 16 codegen units) that no release ships, so
its absolute times are not comparable with the tables above.
`deploy-docs.yml` publishes the latest reports with the site:

<div class="benchmark-links">

- <a href="/benchmarks/report/index.html" target="_blank">All Criterion reports</a>
- <a href="/benchmarks/single_extraction/report/index.html" target="_blank">Single file extraction</a>
- <a href="/benchmarks/batch_100_jpegs/report/index.html" target="_blank">Batch of 100 JPEGs</a>
- <a href="/benchmarks/format_comparison/report/index.html" target="_blank">Format comparison</a>
- <a href="/benchmarks/format_detection/report/index.html" target="_blank">Format detection</a>
- <a href="/benchmarks/full_read_metadata/report/index.html" target="_blank">Full metadata read</a>

</div>

The deploy stamps the following lines with the date of that deploy and the
commit whose `ci.yml` run produced the published reports. A local build
without reports shows the placeholders.

- **Date:** not stamped (local build)
- **Commit:** not stamped (local build)

## Claims this site does not make

Release notes for 1.x published much larger multipliers. Those figures came
from a 112-byte stub file (so they measured process startup), from an
unpinned ExifTool 13.36 found on `PATH`, and from parallel OxiDex timed
against single-threaded Perl. They survive only in a labelled historical
section of `benches/benchmark_results.md` and in the 1.x changelog entries.
They are not comparable with the tables above, and the #821 profile does not
support them.

## Reproduce it

See [Benchmarks](/performance/benchmarks) for the exact commands, and
[Profiling](/performance/profiling) for finding where time goes.
