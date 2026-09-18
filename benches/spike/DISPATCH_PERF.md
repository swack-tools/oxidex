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
| Is table dispatch > 5 % of runtime? | **No. It is ~0.01 %.** `IfdTable::tag` (binary search) costs 5.9 ns per lookup; Canon.jpg walks 81 IFD entries (ExifTool `-v2`), i.e. **~0.48 µs of a 6.6 ms read**. The whole generated IFD engine, conditions and regexes included, is 0.66 % inclusive of the in-process corpus read. | criterion `benches/spike/benches/dispatch.rs`; samply `stages --reps 100 t/images/*` |
| Would `match` arms move the wall clock? | **No.** A generated `match id` is 1.5 ns/lookup vs 5.9 ns -- faster per lookup, but the saving is **~0.36 µs per file** out of 6.6 ms (0.0055 %). Even the critique's own picture (linear `iter().find`, 43.9 ns/lookup) would only be ~3.6 µs per file. | criterion, same |
| Is `ifd_engine.rs:1479` the per-tag lookup? | **No.** The per-entry lookup is `ifd_engine::resolve` -> `IfdTable::tag` (`ifd_schema.rs:89`), a `binary_search_by_key` over id-sorted `tags`. Line 1479 (`direct_serial_no_match`) is reached only from the serial-subdirectory fallback at `ifd_engine.rs:948`. The binary-data engine (`engine.rs`, ProcessBinaryData) does not look ids up at all: it iterates the table's fields in `visit_order`. | source read |
| What IS the dominant cost? | **Composite resolution's allocation storm**: `composite::apply` -> `resolve_dependency` -> `cli::tag_resolution::resolve_requested_tags` -> `MetadataMap::all_occurrences()`, which `format!`s a fresh `"{group0}:{name}"` `String` for **every occurrence, on every dependency lookup**, and the filter never reads it. 327 dependency literals across 106 composites x up to 8 fixpoint passes x 160-250 occurrences/file = **167637 heap allocations (+113484 reallocs) to read Canon.jpg's 157 tags**; 580 of them are retained by the map. This is **91.6 % of the in-process corpus read** and 39.7 % of a single CLI invocation; 54 % of all CPU time in the corpus read has its leaf inside `libsystem_malloc`, another 15 % in `memmove`. | samply + counting `#[global_allocator]` (`benches/spike/src/bin/stages.rs`) |
| Second and third | Per-process startup: `filetype::COMPILED` (a `LazyLock` that compiles every `%magicNumber` regex) is **40.5 %** of a single-file CLI run and `tag_db::TAG_ID_TO_NAME_INDEX` + the `oxidex-tags-*` `LazyLock`s another **8.7 %** -- paid on every invocation, so `oxidex` on a 112-byte JPEG stub is 9.1 ms of which `oxidex --version` is 5.0 ms. Inside the engine, `cond::regex_match_str` builds a new `regex::Regex` on **every** Condition evaluation (`cond.rs`, no cache): 3.3 % of the CLI run, i.e. most of the engine's own cost. | samply, `oxidex -j -a -G1 Canon.jpg` x40 |
| Is "50-100x faster than Perl" supported? | **No.** Measured today against the pinned ExifTool 13.59 under perl 5.38.2: **3.15x** on Canon.jpg, **3.11x** on Nikon.nef, **1.80x** on the 194-file corpus single-threaded (**6.02x** with rayon on 10 cores). Per core, oxidex is a small-integer multiple of Perl, and the reasons are allocation and startup, not dispatch. | hyperfine, exclusive lock, `--warmup 5 --runs 30`, both commands in one invocation |

Everything the critique names is already the cheap part. Compiling tables to
`match` arms would optimise ~0.48 µs of a 6.6 ms read.

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
measurement lock (`locked.py`, lock wait 2014.1 s; other agents'
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
panic=abort, strip=none on darwin), sha256 `eeca6789d7ae7ceeecc04d4a26409a0249cb0f838a02d60d0f6c5f83ebf29c37`, rebuilt at commit
`fa83d70f` (byte-identical to the build at base `6ada109b`: no commit on
this branch touches `src/`; the tree was clean apart from the untracked
`HANDOFF.md`). ExifTool:
`/tmp/oxidex-exiftool-cache/exiftool/exiftool` under
`perl5.38.2` (`-ver` -> 13.59, `-s3 -FileType OOXML.docx` -> DOCX, both
asserted by the script before any number). Machine: Apple M5, 10 cores,
32 GB, macOS 27.0. Load (1-min) before and after each run is in the table. The one large σ
(ExifTool on Canon.jpg, 26 %) is a single 100 ms outlier; its median and
minimum ratios agree to 4 %.

| Scenario | Command | Median | Min | Mean ± σ | Max | Runs | load1 before -> after | Ratio ExifTool/oxidex (median; min) |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| (a) Canon.jpg `-j -a -G1` | `oxidex -j -a -G1 Canon.jpg` | 15.5 ms | 15.0 ms | 15.6 ± 0.3 (2 %) | 16.4 ms | 30 | 6.68 -> 6.68 | **3.15x**; 3.04x |
|  | `exiftool -j -a -G1 Canon.jpg` | 49.0 ms | 45.5 ms | 53.8 ± 14.1 (26 %) | 100.1 ms | 30 | 6.68 -> 6.68 |  |
| (b) Nikon.nef `-j -a -G1` | `oxidex -j -a -G1 Nikon.nef` | 19.1 ms | 18.4 ms | 19.1 ± 0.4 (2 %) | 20.0 ms | 30 | 6.68 -> 6.55 | **3.11x**; 3.14x |
|  | `exiftool -j -a -G1 Nikon.nef` | 59.3 ms | 57.7 ms | 59.2 ± 0.7 (1 %) | 60.7 ms | 30 | 6.68 -> 6.55 |  |
| (c) 194-file t/images `-j -a -G1`, oxidex rayon 10 cores | `oxidex -j -a -G1 <194 files>` | 117.7 ms | 102.1 ms | 118.0 ± 8.7 (7 %) | 134.6 ms | 30 | 6.55 -> 8.20 | **6.02x**; 6.72x |
|  | `exiftool -j -a -G1 <194 files>` | 708.9 ms | 686.6 ms | 737.3 ± 70.2 (10 %) | 997.0 ms | 30 | 6.55 -> 8.20 |  |
| (c') same, oxidex `RAYON_NUM_THREADS=1` | `oxidex -j -a -G1 <194 files>` | 390.7 ms | 381.9 ms | 404.8 ± 47.0 (12 %) | 575.8 ms | 30 | 8.20 -> 7.41 | **1.80x**; 1.79x |
|  | `exiftool -j -a -G1 <194 files>` | 703.4 ms | 684.0 ms | 727.0 ± 59.1 (8 %) | 899.9 ms | 30 | 8.20 -> 7.41 |  |
| process floor | `oxidex --version` | 5.0 ms | 4.5 ms | 5.0 ± 0.3 (7 %) | 5.9 ms | 30 | 7.41 -> 7.41 | **6.12x**; 6.60x |
|  | `exiftool -ver` | 30.5 ms | 29.6 ms | 30.6 ± 0.4 (1 %) | 31.5 ms | 30 | 7.41 -> 7.41 |  |

Machine state around each run (load 1/5/15 and top-5 CPU consumers):

```
[canon_jpg before]  5:33  up 12 days, 22:29, 1 user, load averages: 6.68 7.15 7.12
[canon_jpg before]    %CPU ARGS
[canon_jpg before]   118.8 /usr/libexec/icloudmailagent
[canon_jpg before]   106.2 /System/Applications/Mail.app/Contents/MacOS/Mail
[canon_jpg before]    92.3 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[canon_jpg before]    40.7 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[canon_jpg before]    19.2 /Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper --type=gpu-process --
[canon_jpg after]  5:33  up 12 days, 22:29, 1 user, load averages: 6.68 7.15 7.12
[canon_jpg after]    %CPU ARGS
[canon_jpg after]   311.3 /System/Applications/Mail.app/Contents/MacOS/Mail
[canon_jpg after]   118.0 /usr/libexec/icloudmailagent
[canon_jpg after]    41.6 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[canon_jpg after]    39.5 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[canon_jpg after]     6.8 /System/Library/DriverExtensions/AppleCentauriAlpha.dext/AppleCentauriAlpha com.apple.driver.AppleCentauriAlpha 0x
[nikon_nef before]  5:33  up 12 days, 22:29, 1 user, load averages: 6.68 7.15 7.12
[nikon_nef before]    %CPU ARGS
[nikon_nef before]   311.3 /System/Applications/Mail.app/Contents/MacOS/Mail
[nikon_nef before]   118.0 /usr/libexec/icloudmailagent
[nikon_nef before]    60.9 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[nikon_nef before]    40.8 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[nikon_nef before]     5.6 /Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper --type=gpu-process --
[nikon_nef after]  5:33  up 12 days, 22:29, 1 user, load averages: 6.55 7.11 7.10
[nikon_nef after]    %CPU ARGS
[nikon_nef after]   124.1 /usr/libexec/icloudmailagent
[nikon_nef after]   100.5 /System/Applications/Mail.app/Contents/MacOS/Mail
[nikon_nef after]    72.4 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[nikon_nef after]    40.8 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[nikon_nef after]     9.8 /System/Applications/Utilities/Activity Monitor.app/Contents/MacOS/Activity Monitor
[corpus_parallel before]  5:33  up 12 days, 22:29, 1 user, load averages: 6.55 7.11 7.10
[corpus_parallel before]    %CPU ARGS
[corpus_parallel before]   124.1 /usr/libexec/icloudmailagent
[corpus_parallel before]   100.5 /System/Applications/Mail.app/Contents/MacOS/Mail
[corpus_parallel before]    72.4 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanSe
[corpus_parallel before]    40.8 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[corpus_parallel before]     9.8 /System/Applications/Utilities/Activity Monitor.app/Contents/MacOS/Activity Monitor
[corpus_parallel after]  5:34  up 12 days, 22:30, 1 user, load averages: 8.20 7.49 7.24
[corpus_parallel after]    %CPU ARGS
[corpus_parallel after]   153.7 /System/Applications/Mail.app/Contents/MacOS/Mail
[corpus_parallel after]   116.5 /usr/libexec/icloudmailagent
[corpus_parallel after]   100.7 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanSer
[corpus_parallel after]    46.7 /usr/libexec/spotlightknowledged.updater -u
[corpus_parallel after]    41.1 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[corpus_1thread before]  5:34  up 12 days, 22:30, 1 user, load averages: 8.20 7.49 7.24
[corpus_1thread before]    %CPU ARGS
[corpus_1thread before]   153.7 /System/Applications/Mail.app/Contents/MacOS/Mail
[corpus_1thread before]   116.5 /usr/libexec/icloudmailagent
[corpus_1thread before]   100.7 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanSer
[corpus_1thread before]    46.7 /usr/libexec/spotlightknowledged.updater -u
[corpus_1thread before]    41.1 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[corpus_1thread after]  5:34  up 12 days, 22:30, 1 user, load averages: 7.41 7.38 7.21
[corpus_1thread after]    %CPU ARGS
[corpus_1thread after]   194.2 /System/Applications/Mail.app/Contents/MacOS/Mail
[corpus_1thread after]   121.2 /usr/libexec/icloudmailagent
[corpus_1thread after]    47.4 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanServ
[corpus_1thread after]    41.8 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[corpus_1thread after]     6.4 /usr/libexec/spotlightknowledged.updater -u
[noop before]  5:34  up 12 days, 22:30, 1 user, load averages: 7.41 7.38 7.21
[noop before]    %CPU ARGS
[noop before]   195.8 /System/Applications/Mail.app/Contents/MacOS/Mail
[noop before]   122.6 /usr/libexec/icloudmailagent
[noop before]    40.7 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[noop before]    32.4 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[noop before]     5.6 /Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper --type=gpu-process --user-
[noop after]  5:34  up 12 days, 22:30, 1 user, load averages: 7.41 7.38 7.21
[noop after]    %CPU ARGS
[noop after]   123.2 /System/Applications/Mail.app/Contents/MacOS/Mail
[noop after]   122.0 /usr/libexec/icloudmailagent
[noop after]    83.4 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[noop after]    37.4 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[noop after]     6.0 /System/Library/ExtensionKit/Extensions/TextThumbnailExtension.appex/Contents/MacOS/TextThumbnailExtension -BSServiceDo
[stages before]  5:34  up 12 days, 22:30, 1 user, load averages: 7.41 7.38 7.21
[stages before]    %CPU ARGS
[stages before]   122.3 /usr/libexec/icloudmailagent
[stages before]   114.0 /System/Applications/Mail.app/Contents/MacOS/Mail
[stages before]    90.5 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[stages before]    39.5 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[stages before]     6.2 /System/Library/DriverExtensions/AppleCentauriAlpha.dext/AppleCentauriAlpha com.apple.driver.AppleCentauriAlpha 0x10
[stages after]  5:35  up 12 days, 22:30, 1 user, load averages: 6.86 7.26 7.17
[stages after]    %CPU ARGS
[stages after]   132.1 /System/Applications/Mail.app/Contents/MacOS/Mail
[stages after]   122.0 /usr/libexec/icloudmailagent
[stages after]    83.2 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[stages after]    38.0 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[stages after]     5.6 /System/Library/DriverExtensions/AppleCentauriAlpha.dext/AppleCentauriAlpha com.apple.driver.AppleCentauriAlpha 0x100
[criterion before]  5:35  up 12 days, 22:30, 1 user, load averages: 6.86 7.26 7.17
[criterion before]    %CPU ARGS
[criterion before]   119.3 /usr/libexec/icloudmailagent
[criterion before]   117.4 /System/Applications/Mail.app/Contents/MacOS/Mail
[criterion before]    94.0 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[criterion before]    35.8 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[criterion before]     6.8 /System/Library/DriverExtensions/AppleCentauriAlpha.dext/AppleCentauriAlpha com.apple.driver.AppleCentauriAlpha 0
[criterion after]  5:38  up 12 days, 22:34, 1 user, load averages: 5.40 6.81 7.04
[criterion after]    %CPU ARGS
[criterion after]    94.1 /usr/bin/pmset -g log
[criterion after]    92.9 /System/Applications/Mail.app/Contents/MacOS/Mail
[criterion after]    76.3 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[criterion after]    36.8 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[criterion after]    12.5 /usr/bin/grep -E (Using AC|Using Batt)
[profiles after]  5:39  up 12 days, 22:35, 1 user, load averages: 4.59 6.39 6.88
[profiles after]    %CPU ARGS
[profiles after]    93.4 /System/Applications/Mail.app/Contents/MacOS/Mail
[profiles after]    70.9 /System/Library/PrivateFrameworks/MediaAnalysis.framework/Versions/A/mediaanalysisd
[profiles after]    60.9 /Applications/CrashPlan.app/Contents/Library/LaunchServices/CrashPlanService.app/Contents/MacOS/CrashPlanService
[profiles after]    42.2 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon
[profiles after]    25.1 /Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper --type=gpu-process --us
```

Notes:
- Multi-file oxidex runs use rayon across all 10 cores; ExifTool is
  single-threaded. The `RAYON_NUM_THREADS=1` row is the like-for-like ratio.
- The `--version` / `-ver` row is the process floor. oxidex's floor is
  5.0 ms but a 112-byte JPEG costs 9.1 ms: the difference is
  the lazy `filetype::COMPILED` regex compile and `tag_db` index build
  (section 3).

## 3. Where oxidex's time goes

Instrument: `samply record --rate 4000 --unstable-presymbolicate` (function
level; fat LTO inlines aggressively, so attribution is to the outermost
surviving symbol), bucketed by
`tools/exiftool-tables/spike/samply_buckets.py`: a sample with any frame
inside a generated engine (`ifd_engine`, `engine`, `serial_engine`,
`keyed_engine`, `cond`/`exprs`) is the engine's -- the engines are always
entered through a hand parser, and the question is what they cost;
otherwise the first stage regex matched walking the stack root -> leaf owns
it (a `malloc` under Composite is Composite's). Weights are
`threadCPUDelta` (CPU time, idle excluded). Two workloads:

**(a) In-process corpus read** -- `benches/spike/target/release/stages
--reps 100 t/images/*` (same library, same profile; startup amortised away):

```
91.58%   34971041  composite
  2.98%    1138465  fs+mmap+detect
  2.66%    1015466  hand parsers
  0.89%     341353  json-output
  0.72%     274210  map/sink insert
  0.54%     207765  engine:cond/exprs
  0.43%     163150  cli/main
  0.12%      45209  engine:ifd
  0.07%      25805  value/printconv
  0.00%       1197  engine:binary
  0.00%        725  engine:serial


== allocator/memcpy leaf share (overlay, any stage) ==
 53.93%   20594720  allocator (libsystem_malloc leaf)
 15.25%    5823919  memmove/memcpy/memset leaf
```

Top inclusive functions:
```
100.00%   38184386  stages::main
 97.46%   37215611  oxidex::core::operations::read_metadata_with_detector_and_options
 91.59%   34971535  oxidex::composite::apply
 89.78%   34283759  oxidex::composite::resolve
 89.07%   34009575  oxidex::cli::tag_resolution::resolve_requested_tags
 88.70%   33869347  <core::iter::adapters::filter::Filter<core::iter::adapters::filter::Filter<core::iter::adapters::map::Map<core::iter::adapters::map::Map<core::iter::adapters::m
  1.60%     612271  oxidex::core::file_metadata::extract_file_metadata
  1.49%     569542  oxidex::parsers::xmp::rdf_parser::parse_xmp_packet
  1.45%     555023  oxidex::core::operations::parse_jpeg_metadata
  1.45%     554775  oxidex::core::operations::parse_jpeg_metadata_with_diagnostics
  1.02%     389475  oxidex::parsers::xmp::rdf_parser::parse_xmp_entries_with_rational_forms
  0.93%     354567  <oxidex::io::mmap_reader::MMapReader>::new
  0.89%     341353  <oxidex::cli::output_formatter::JsonFormatter>::format_with_status_and_mode
  0.65%     246741  oxidex::parsers::xmp::parse_xmp_file
  0.63%     240690  oxidex::core::jpeg_helpers::process_exif_segments
  0.59%     227081  oxidex::parsers::tiff::makernote_dispatcher::dispatch_makernote_with_context_and_values
  0.54%     205871  oxidex::exiftool_tables::ifd_engine::process_exif_decoded
  0.54%     205395  oxidex::exiftool_tables::ifd_engine::walk
  0.54%     204369  oxidex::core::tiff_helpers::parse_exif_directory
  0.51%     196232  <oxidex::parsers::tiff::makernotes::canon::CanonParser as oxidex::parsers::tiff::makernotes::shared::makernote_parser::MakerNoteParser>::parse_with_context_and_
  0.51%     196012  oxidex::parsers::tiff::makernotes::canon::parse_canon_makernote_directory
  0.48%     184948  oxidex::parsers::raw::metadata::parse_tiff_based_raw
```

**(b) One CLI invocation** -- `oxidex -j -a -G1 t/images/Canon.jpg`, 40
iterations (startup included, as a user pays it):

```
41.41%     269473  fs+mmap+detect
 39.69%     258322  composite
  8.80%      57304  value/printconv
  3.49%      22728  engine:cond/exprs
  3.06%      19888  cli/main
  0.96%       6225  tag_resolution
  0.92%       5997  hand parsers
  0.68%       4433  json-output
  0.51%       3341  engine:ifd
  0.29%       1857  map/sink insert
  0.15%        991  other/unattributed
  0.04%        255  engine:binary


== allocator/memcpy leaf share (overlay, any stage) ==
 42.92%     279311  allocator (libsystem_malloc leaf)
 16.07%     104576  memmove/memcpy/memset leaf
```

Top inclusive functions:
```
 99.85%     649823  oxidex::main
 99.81%     649593  oxidex::handle_read_operation
 40.71%     264951  oxidex::core::file_metadata::extract_file_metadata
 40.58%     264086  oxidex::filetype::identify
 40.45%     263261  <oxidex::filetype::COMPILED::{closure#0} as core::ops::function::FnOnce<()>>::call_once
 39.69%     258322  oxidex::composite::apply
 38.96%     253533  oxidex::composite::resolve
 38.66%     251580  oxidex::cli::tag_resolution::resolve_requested_tags
 38.51%     250646  <core::iter::adapters::filter::Filter<core::iter::adapters::filter::Filter<core::iter::adapters::map::Map<core::iter::adapters::map::Map<core::iter::adapters::m
 14.17%      92206  oxidex::core::operations::parse_jpeg_metadata_with_diagnostics
 13.62%      88655  oxidex::core::jpeg_helpers::process_exif_segments
  8.75%      56926  <std::sync::once::Once>::call_once_force::<<std::sync::lazy_lock::LazyLock<core::option::Option<std::collections::hash::map::HashMap<alloc::string::String, oxid
  8.68%      56461  oxidex::tag_db::lookup_tag_name
  8.51%      55378  <oxidex::tag_db::TAG_ID_TO_NAME_INDEX::{closure#0} as core::ops::function::FnOnce<()>>::call_once
  4.82%      31361  oxidex::core::tiff_helpers::parse_exif_directory
  4.24%      27613  oxidex::core::tiff_helpers::parse_makernote
  4.07%      26491  <oxidex_tags_camera::CAMERA_TAGS::{closure#0} as core::ops::function::FnOnce<()>>::call_once
  3.93%      25606  oxidex::parsers::tiff::makernotes::canon::parse_canon_makernote_directory
  3.93%      25606  oxidex::parsers::tiff::makernote_dispatcher::dispatch_makernote_with_context_and_values
  3.93%      25606  <oxidex::parsers::tiff::makernotes::canon::CanonParser as oxidex::parsers::tiff::makernotes::shared::makernote_parser::MakerNoteParser>::parse_with_context_and_
  3.73%      24260  oxidex::exiftool_tables::ifd_engine::process_exif_decoded
  3.73%      24260  oxidex::exiftool_tables::ifd_engine::walk
```

Reading the buckets:

| Bucket | Corpus in-process | Single CLI run | What it is |
|---|---|---|---|
| Composite (`composite::apply`) | 91.6 % | 39.7 % | `resolve_dependency` -> `resolve_requested_tags` -> `all_occurrences()` `format!` per occurrence per dependency (see verdict) |
| File I/O + detection | 3.0 % | 40.5 % | corpus: `stat`/`open`/`mmap` per file; CLI: dominated by the one-time `filetype::COMPILED` regex compile |
| Hand-written parsers (`parsers::`, `tiff_helpers`, makernotes) | 2.7 % | 0.9 % | the actual parsing |
| Generated engines (`ifd_engine`, `engine`, `cond`) | 0.66 % | 4.04 % | of which `cond::regex_match_str` (per-eval `Regex::new`) is most |
| Value/PrintConv, tag_db | 0.07 % | 8.7 % | CLI: `TAG_ID_TO_NAME_INDEX` + `*_TAGS` `LazyLock` builds |
| MetadataMap/TagSink insert + interning | 0.72 % | 0.29 % | `record`, `from_insert_shim`, `intern` (a global `Mutex<HashSet>` per occurrence x3) |
| JSON output | 0.89 % | 0.68 % | `JsonFormatter` |
| Allocator + memmove (leaf, any bucket) | 69.2 % | 59.0 % | corpus: 54 % `libsystem_malloc` + 15 % `_platform_memmove`, 67.0 points of it under Composite |

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

| table / workload | n | linear ns/lookup | bsearch ns/lookup | matcharm ns/lookup | hashmap ns/lookup |
|---|---:|---:|---:|---:|---:|
| exif_main / all | 576 | 77.2 | 6.9 | 2.2 | 3.1 |
| exif_main / canon_jpg | 44 | 63.8 | 7.1 | 1.7 | 2.9 |
| exif_main / nikon_nef | 72 | 47.7 | 9.0 | 1.7 | 2.9 |
| exif_main / miss | 64 | 138.9 | 6.5 | 2.1 | 2.6 |
| canon_main / all | 77 | 11.9 | 3.9 | 1.1 | 3.3 |
| canon_main / canon_jpg | 26 | 10.2 | 3.9 | 1.1 | 2.8 |
| canon_main / miss | 64 | 19.1 | 3.5 | 1.1 | 2.7 |

Per file: Canon.jpg walks 81 IFD entries and Nikon.nef 189 (ExifTool `-v2`,
BinaryData directories excluded because they iterate fields, they do not
look ids up). At 5.9 ns each that is 0.48 µs and
1.70 µs -- **0.007 % and 0.016 % of the
respective reads**. Replacing binary search with `match` saves
0.36 µs / 1.39 µs per file.

## 5. Allocations per file

Instrument: `benches/spike/src/bin/stages.rs` (counting `#[global_allocator]`
around each stage; `read` = `read_metadata_with_detector_and_options`,
`clone` = allocations the finished `MetadataMap` retains, `json` =
`JsonFormatter::format_with_status_and_mode`). Medians of 300 reps (30 for the corpus), exclusive lock,
load1 7.41 at start (snapshots above).

| file | tags | fs µs | mmap µs | detect µs | **read µs** | json µs | read allocs | read reallocs | read bytes | retained allocs | json allocs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Canon.jpg | 157 | 25.5 | 17.5 | 9.3 | **6574.2** | 41.4 | 167637 | 113484 | 6961106 | 580 | 1852 |
| Nikon.nef | 245 | 10.5 | 7.5 | 0.0 | **10312.6** | 66.2 | 177215 | 192913 | 4802696 | 834 | 1829 |
| corpus TOTAL (194 files, one pass) | 12409 | 2218.0 | 1542.1 | 638.9 | **367721.0** | 3108.5 | 6356079 | 6931667 | - | 41627 | 93886 |

- Read Canon.jpg (157 tags): **167637 allocations + 113484 reallocs, 7.0 MB** requested; only **580** allocations survive in the map (key `String`s, the duplicate `TagId::Named` key copy `from_insert_shim` makes, and value `String`s -- about 3.7 per tag), so **> 99.6 % are transient**, overwhelmingly `all_occurrences()`' throw-away keys under Composite. JSON output is 1852 allocations.
- Nikon.nef (245 tags): 177215 + 192913, 834 retained.
- Corpus (194 files, 12,409 tags): 6356079 allocations for one pass -- ~512 per emitted tag.
- Of the retained ~3.7 per tag: 2 are the same `"Group:Name"` text twice (the sink key and `TagId::Named(key.to_string())`), plus up to 3 `intern()` calls per occurrence each taking a global mutex.

## 6. What would move the wall clock (not done here -- spike only)

1. `MetadataMap::all_occurrences()` / `resolve_requested_tags`: match on
   `occurrence.name` / `group0` without materialising a key `String` per
   occurrence, or index occurrences by name once per `apply`. Removes ~99 %
   of allocations and, by the profile, roughly 91.6 % of the
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
