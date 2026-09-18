# Dispatch-perf spike: is the generated-table walk hot?

**Branch** `staging/spike-dispatch-perf` (measurement only; nothing here is
for merge as-is). **Base** `6ada109b` (`origin/refactor/tag-machinery`,
2026-09-18). **Evidence** `/Users/allen/oxidex-ops/evidence/20260918-dispatch-perf/`.

## Claim under test

> The generated tables are "an interpreter in disguise": static `&[Field]`
> arrays walked at runtime by a slow table-walker (`engine.rs`,
> `ifd_engine.rs`; e.g. `ifd_engine.rs:1479` does
> `table.tags.iter().find(|t| t.id == id)` per tag). Compiling tables to
> native `match tag_id` jump tables would make oxidex 50-100x faster than
> ExifTool.

## Verdict, with numbers

| Question | Answer | Instrument |
|---|---|---|
| Is table dispatch > 5 % of runtime? | **No. It is ~0.01 %.** `IfdTable::tag` (binary search) costs @@BSEARCH_NS@@ ns per lookup; Canon.jpg walks 81 IFD entries (ExifTool `-v2`), i.e. **~@@BSEARCH_FILE_US@@ µs of a @@READ_CANON_MS@@ ms read**. The whole generated IFD engine, conditions and regexes included, is @@ENGINE_INCL_PCT@@ % inclusive of the in-process corpus read. | criterion `benches/spike/benches/dispatch.rs`; samply `stages --reps 100 t/images/*` |
| Would `match` arms move the wall clock? | **No.** A generated `match id` is @@MATCH_NS@@ ns/lookup vs @@BSEARCH_NS@@ ns -- faster per lookup, but the saving is **~@@MATCH_SAVING_US@@ µs per file** out of @@READ_CANON_MS@@ ms (@@MATCH_SAVING_PCT@@ %). Even the critique's own picture (linear `iter().find`, @@LINEAR_NS@@ ns/lookup) would only be ~@@LINEAR_FILE_US@@ µs per file. | criterion, same |
| Is `ifd_engine.rs:1479` the per-tag lookup? | **No.** The per-entry lookup is `ifd_engine::resolve` -> `IfdTable::tag` (`ifd_schema.rs:89`), a `binary_search_by_key` over id-sorted `tags`. Line 1479 (`direct_serial_no_match`) is reached only from the serial-subdirectory fallback at `ifd_engine.rs:948`. The binary-data engine (`engine.rs`, ProcessBinaryData) does not look ids up at all: it iterates the table's fields in `visit_order`. | source read |
| What IS the dominant cost? | **Composite resolution's allocation storm**: `composite::apply` -> `resolve_dependency` -> `cli::tag_resolution::resolve_requested_tags` -> `MetadataMap::all_occurrences()`, which `format!`s a fresh `"{group0}:{name}"` `String` for **every occurrence, on every dependency lookup**, and the filter never reads it. 327 dependency literals across 106 composites x up to 8 fixpoint passes x 160-250 occurrences/file = **@@READ_ALLOCS_CANON@@ heap allocations (+@@READ_REALLOCS_CANON@@ reallocs) to read Canon.jpg's 157 tags**; 580 of them are retained by the map. This is **@@COMPOSITE_PCT@@ % of the in-process corpus read** and @@COMPOSITE_CLI_PCT@@ % of a single CLI invocation; 53 % of all CPU samples have their leaf inside `libsystem_malloc`, another 13 % in `memmove`. | samply + counting `#[global_allocator]` (`benches/spike/src/bin/stages.rs`) |
| Second and third | Per-process startup: `filetype::COMPILED` (a `LazyLock` that compiles every `%magicNumber` regex) is **@@FILETYPE_PCT@@ %** of a single-file CLI run and `tag_db::TAG_ID_TO_NAME_INDEX` + the `oxidex-tags-*` `LazyLock`s another **@@TAGDB_PCT@@ %** -- paid on every invocation, so `oxidex` on a 112-byte JPEG stub is @@STUB_MS@@ ms of which `oxidex --version` is @@NOOP_MS@@ ms. Inside the engine, `cond::regex_match_str` builds a new `regex::Regex` on **every** Condition evaluation (`cond.rs`, no cache): @@REGEX_PCT@@ % of the CLI run, i.e. most of the engine's own cost. | samply, `oxidex -j -a -G1 Canon.jpg` x40 |
| Is "50-100x faster than Perl" supported? | **No.** Measured today against the pinned ExifTool 13.59 under perl 5.38.2: **@@RATIO_CANON@@x** on Canon.jpg, **@@RATIO_NEF@@x** on Nikon.nef, **@@RATIO_CORPUS_1T@@x** on the 194-file corpus single-threaded (**@@RATIO_CORPUS_PAR@@x** with rayon on 10 cores). Per core, oxidex is a small-integer multiple of Perl, and the reasons are allocation and startup, not dispatch. | hyperfine, exclusive lock, `--warmup 5 --runs 30`, both commands in one invocation |

Everything the critique names is already the cheap part. Compiling tables to
`match` arms would optimise ~1 µs of a ~10 ms read.

## 1. What was already on file (benches/)

`benches/benchmark_results.md` (last regenerated 2025-10-30, commits
`d1c94d05`/`40fe06c8`; path-note edit 2026-09-11 `79101d7d`) reports
`exiftool-rs 0.1.0` vs an **unpinned Perl ExifTool 13.36** on an Apple M4:
16.1x single file, 64.9x batch (1000 files, rayon vs single-threaded Perl),
13.3x write, 14.2x detection. No commit, binary hash or load is recorded; the
"single file" is `tests/fixtures/jpeg/simple/sample_with_exif.jpg`, a
**112-byte stub**, so that scenario measured process startup. The binary is
now `oxidex` 1.2.1 and the pin is 13.59, so none of those numbers describe
the current code or the current oracle. (Refreshed separately in this PR by
`benches/exiftool_comparison.sh`; see `benches/benchmark_results.md`.)

`benches/*.rs` (criterion, `cargo bench`): `parse_benchmarks`
(format_detection, jpeg_segment_parsing, tiff_ifd_parsing,
full_read_metadata), `integration_benchmarks` (single extraction, batch of
100 JPEGs, large file, per-format, GPS), `raw_parsing_bench`,
`audio_benchmarks`, `video_benchmarks`, `detection_comparison`
(Signature vs Magika). All are in-process library benches of the
hand-written parsers; **none exercises the generated-table engines in
isolation and none compares against ExifTool**. CI's `metrics` job runs
`integration_benchmarks` + `parse_benchmarks` with `--quick` and
`CARGO_PROFILE_BENCH_LTO=false` -- a profile nobody ships -- and only on
`push` to `main` (`ci.yml` `if: github.event_name == 'push' && github.ref ==
'refs/heads/main'`), so it shows `skipping` on every PR and never runs for
`refactor/tag-machinery` at all. There was no measurement of whether table
dispatch is hot.

## 2. Wall clock: oxidex vs pinned ExifTool 13.59 (hyperfine)

Instrument: `tools/exiftool-tables/spike/run_timing.sh` under the exclusive
measurement lock (`locked.py`, lock wait @@LOCK_WAIT_S@@ s; other agents'
shared jobs drain and block while it runs), `hyperfine --warmup 5 --runs 30
-N`, both commands of a scenario in the same invocation so they see the same
background. **This host is never idle**: `CrashPlanService` and `Mail`/
`icloudmailagent` run permanently at 1-2 cores, so load1 sits at 4-6 with no
agent work at all. There is therefore no absolute load gate; instead load
1/5/15 and the top-5 CPU consumers are recorded before and after every timed
run (below and in `locked.log`), the run waits up to 60 s for load1 to stop
falling, and ratios are reported on medians **and on minima** (the minimum is
the statistic least sensitive to background load). Where σ/mean is large the
number is marked, not trusted. oxidex `target/release/oxidex` built with the
shipped `[profile.release]` (**fat LTO on**, codegen-units=1, opt-level=3,
panic=abort, strip=none on darwin), sha256 `@@OXIDEX_SHA@@`, from commit
`6ada109b` (tree dirty only with this spike's new files under
`benches/spike/`, `tools/exiftool-tables/spike/`, `benches/`). ExifTool:
`/tmp/oxidex-exiftool-cache/exiftool/exiftool` under
`perl5.38.2` (`-ver` -> 13.59, `-s3 -FileType OOXML.docx` -> DOCX, both
asserted by the script before any number). Machine: Apple M5, 10 cores,
32 GB, macOS 27.0. Load (1-min) at each run is in the table.

@@HF_TABLE@@

Machine state around each run (load 1/5/15 and top-5 CPU consumers):

@@SNAPSHOTS@@

Notes:
- Multi-file oxidex runs use rayon across all 10 cores; ExifTool is
  single-threaded. The `RAYON_NUM_THREADS=1` row is the like-for-like ratio.
- The `--version` / `-ver` row is the process floor. oxidex's floor is
  @@NOOP_MS@@ ms but a 112-byte JPEG costs @@STUB_MS@@ ms: the difference is
  the lazy `filetype::COMPILED` regex compile and `tag_db` index build
  (section 3).

## 3. Where oxidex's time goes

Instrument: `samply record --rate 4000 --unstable-presymbolicate` (function
level; fat LTO inlines aggressively, so attribution is to the outermost
surviving symbol), bucketed by
`tools/exiftool-tables/spike/samply_buckets.py`: a sample is owned by the
first stage regex matched walking its stack root -> leaf; weights are
`threadCPUDelta` (CPU time, idle excluded). Two workloads:

**(a) In-process corpus read** -- `benches/spike/target/release/stages
--reps 100 t/images/*` (same library, same profile; startup amortised away):

@@PROFILE_CORPUS@@

**(b) One CLI invocation** -- `oxidex -j -a -G1 t/images/Canon.jpg`, 40
iterations (startup included, as a user pays it):

@@PROFILE_CLI@@

Reading the buckets:

| Bucket | Corpus in-process | Single CLI run | What it is |
|---|---|---|---|
| Composite (`composite::apply`) | @@COMPOSITE_PCT@@ % | @@COMPOSITE_CLI_PCT@@ % | `resolve_dependency` -> `resolve_requested_tags` -> `all_occurrences()` `format!` per occurrence per dependency (see verdict) |
| File I/O + detection | @@IO_PCT@@ % | @@FILETYPE_PCT@@ % | corpus: `stat`/`open`/`mmap` per file; CLI: dominated by the one-time `filetype::COMPILED` regex compile |
| Hand-written parsers (`parsers::`, `tiff_helpers`, makernotes) | @@HAND_PCT@@ % | @@HAND_CLI_PCT@@ % | the actual parsing |
| Generated engines (`ifd_engine`, `engine`, `cond`) | @@ENGINE_INCL_PCT@@ % | @@ENGINE_CLI_PCT@@ % | of which `cond::regex_match_str` (per-eval `Regex::new`) is most |
| Value/PrintConv, tag_db | @@CONV_PCT@@ % | @@TAGDB_PCT@@ % | CLI: `TAG_ID_TO_NAME_INDEX` + `*_TAGS` `LazyLock` builds |
| MetadataMap/TagSink insert + interning | @@SINK_PCT@@ % | @@SINK_CLI_PCT@@ % | `record`, `from_insert_shim`, `intern` (a global `Mutex<HashSet>` per occurrence x3) |
| JSON output | @@JSON_PCT@@ % | @@JSON_CLI_PCT@@ % | `JsonFormatter` |
| Allocator + memmove (leaf, any bucket) | @@ALLOC_PCT@@ % | @@ALLOC_CLI_PCT@@ % | 53 % `libsystem_malloc` + 13 % `_platform_memmove`; @@ALLOC_COMPOSITE_PCT@@ points of it under Composite |

## 4. Dispatch microbench (criterion, `benches/spike/benches/dispatch.rs`)

Four ways to resolve an id against `IFD_EXIF_MAIN` (576 tags) and
`IFD_CANON_MAIN` (77): `linear` = `tags.iter().find()` (the critique's
picture), `bsearch` = `IfdTable::tag` (what `resolve` does), `matcharm` = a
generated `match id { 0x.. => Some(&T.tags[i]) }`
(`tools/exiftool-tables/spike/gen_match.py`, 653 arms -- what "compile to a
jump table" produces), `hashmap` = `HashMap<u16, &IfdTag>` for scale.
Workloads: every id in the table, the ids ExifTool 13.59 actually walks for
`Canon.jpg` (44 Exif::Main + 26 Canon::Main) and `Nikon.nef` (72), and 64
misses. All four are asserted to agree on all 65,536 ids before timing.
Same release profile as the shipped binary (fat LTO, cgu=1).

@@CRITERION@@

Per file: Canon.jpg walks 81 IFD entries and Nikon.nef 189 (ExifTool `-v2`,
BinaryData directories excluded because they iterate fields, they do not
look ids up). At @@BSEARCH_NS@@ ns each that is @@BSEARCH_FILE_US@@ µs and
@@BSEARCH_NEF_US@@ µs -- **@@BSEARCH_SHARE_PCT@@ % and @@BSEARCH_NEF_SHARE_PCT@@ % of the
respective reads**. Replacing binary search with `match` saves
@@MATCH_SAVING_US@@ µs / @@MATCH_SAVING_NEF_US@@ µs per file.

## 5. Allocations per file

Instrument: `benches/spike/src/bin/stages.rs` (counting `#[global_allocator]`
around each stage; `read` = `read_metadata_with_detector_and_options`,
`clone` = allocations the finished `MetadataMap` retains, `json` =
`JsonFormatter::format_with_status_and_mode`). Medians of 300 reps (30 for the corpus), exclusive lock,
load1 @@STAGES_LOAD@@ at start (snapshots above).

@@STAGES@@

- Read Canon.jpg (157 tags): **@@READ_ALLOCS_CANON@@ allocations + @@READ_REALLOCS_CANON@@ reallocs, @@READ_BYTES_CANON@@ MB** requested; only **580** allocations survive in the map (key `String`s, the duplicate `TagId::Named` key copy `from_insert_shim` makes, and value `String`s -- about 3.7 per tag), so **> 99.6 % are transient**, overwhelmingly `all_occurrences()`' throw-away keys under Composite. JSON output is @@JSON_ALLOCS_CANON@@ allocations.
- Nikon.nef (245 tags): @@READ_ALLOCS_NEF@@ + @@READ_REALLOCS_NEF@@, 834 retained.
- Corpus (194 files, 12,409 tags): @@READ_ALLOCS_CORPUS@@ allocations for one pass -- ~@@ALLOCS_PER_TAG@@ per emitted tag.
- Of the retained ~3.7 per tag: 2 are the same `"Group:Name"` text twice (the sink key and `TagId::Named(key.to_string())`), plus up to 3 `intern()` calls per occurrence each taking a global mutex.

## 6. What would move the wall clock (not done here -- spike only)

1. `MetadataMap::all_occurrences()` / `resolve_requested_tags`: match on
   `occurrence.name` / `group0` without materialising a key `String` per
   occurrence, or index occurrences by name once per `apply`. Removes ~99 %
   of allocations and, by the profile, roughly @@COMPOSITE_PCT@@ % of the
   in-process read.
2. `filetype::COMPILED`: compile the magic-number regexes into a single
   `RegexSet` at build time, or match the first bytes without `regex`;
   `tag_db::TAG_ID_TO_NAME_INDEX`: a `phf`/sorted static instead of a
   `HashMap<String, ..>` built per process. Together ~half of a single-file
   invocation.
3. `cond::regex_match_str`: cache compiled `Regex` per pattern (a
   `LazyLock<HashMap>` or per-Condition `OnceLock`); currently the engine's
   main cost.
4. Only after those: `from_insert_shim`'s duplicate key copy and the
   `intern()` mutex.

Compiling tables to `match` arms is not on this list.

## Reproduction

```bash
# release binary, shipped profile
cargo build --release --bin oxidex
# spike crate (detached workspace)
cd benches/spike && cp ../../Cargo.lock . && cargo build --release --bin stages && cargo bench --bench dispatch
# regenerate the match arms and replay id lists from the pinned tree
python3 tools/exiftool-tables/spike/gen_match.py
# timed set under the EXCLUSIVE lock (build first, under --shared); snapshots load + top CPU around each run
locked.py <log> -- bash tools/exiftool-tables/spike/run_all_locked.sh <evidence-dir>
# bucket a samply profile
python3 tools/exiftool-tables/spike/samply_buckets.py <profile.json.gz>
```
