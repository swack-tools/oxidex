# Upgrade rehearsal: ExifTool 11.78 and 12.64 (AUTOGENERATION-PLAN step 5)

> **2026-09-21 Task19 tooling status:** the non-promoting transition wrapper,
> checked same-pin/forward/reverse matrix, isolated-side executor seam,
> restoration/interruption controls, mandatory native write/readback contract,
> and fake-only unit tests are implemented on the transition-tooling branch.
> This is tooling readiness only. It does **not** qualify any transition or the
> beta release. Canonical verified 11.78 and 12.64 source bundles, immutable
> read/write fixture manifests, the converged post-Task18 candidate, and all
> three real matrix runs remain prerequisites.
>
> The checked matrix deliberately contains verified-input resolvers rather than
> invented historical hashes. A row refuses until the existing catalog,
> source-resolution, materialization, explicit Perl 5.38.2, version/DOCX,
> read-fixture, and mandatory write/readback verifiers all succeed. Real runs
> must use the entry point's nonblocking `transition.host.lock`; old
> fleet-controller and outer `locked.py` launch examples are not valid Task19
> commands.

Run on 2026-09-18 from `refactor/tag-machinery` at `66e48654` (#818 merged, all
15 known generation blockers closed). The persisted random pair **11.78 / 12.64**
was used as selected; it was not redrawn. The working pin (`.exiftool-version`,
13.59) is unchanged on the branch carrying this record. Every per-release pin
change and regeneration happened in detached scratch worktrees
(`claude-rehearsal-1178`, `claude-rehearsal-1264`) that are not pushed.

**Result: neither release passes end to end.** Generation passes for both
releases after one tool fix. Expression verification and the release build
pass. The unit test suite fails for both releases, and so does public writing.
Reads run, with a measured loss against a 13.59 control on the same
commit and corpus. Two code interventions were needed to get past generation
and test compilation. Both are recorded below and neither is on this branch.

Evidence root: `~/oxidex-ops/evidence/20260918-rehearsal-e2e/` (per-release
directories `11.78/`, `12.64/`, control `13.59-regen/` and `13.59/`; stage logs,
lock status JSONL, adapter stage reports under `stages*/`, intervention patches
under `interventions/`).

## Method

| Stage | Instrument | Notes |
| --- | --- | --- |
| 1 Source | `source_proof.py` (evidence dir) | Tree bytes against `materialization.json` (1,101 / 1,179 files), `-ver`, `OOXML.docx` probe → `DOCX`, all under perl 5.38.2 |
| 2 Generate | `version_rehearsal_stage_adapter.py generate` → `regen-all.sh` | Clean checkout, pin set to the release, all 66 `artifacts.py paths` outputs hashed |
| 3 Expressions | `verify_exprs.py <release dump> --et-lib <release lib>` | Standalone, after regeneration |
| 4 Build | adapter `build` (all-features CLI + lib test driver), `cargo build --release`, `cargo test --workspace` | Per-release `CARGO_TARGET_DIR` |
| 5 Read | adapter `read` → `conformance.py --json-out` against the release's own native ExifTool | Fixed corpus: the 194 files of the 13.59 `t/images` tree (hash manifest in evidence) |
| 6a Write | adapter `write` → `generated_tiff_write_matrix.py --route public-api --rehearsal-release R` | One 771-byte JPEG plus synthetic II/MM TIFF carriers |
| 6b Write | `generated_tiff_write_matrix.py --route public-api --readback-*` | Needs a clean committed tree: the regenerated tree was committed locally in the scratch worktree |

Environment: `PERL5LIB`/`PERLLIB`/`PERL5OPT` unset,
`EXIFTOOL_PERL=/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2`,
`OXIDEX_PINNED_EXIFTOOL` = the release under test. Builds, tests and
regeneration held the shared lock in `--shared` mode. Reads and write matrices
held it exclusively.

**Control.** A same-commit 13.59 control ran the same generate, build, read
and write stages in a third scratch worktree. That same-pin regeneration passed.
Only the provenance fields of two ledgers changed
(`expr_oracle_ledger.json`, `ifd_identity_ledger.json`). The 13.59 control
`cargo test --workspace` passed: 5,996 passed, 0 failed, 140 ignored.

## Per-release stage results

Times are seconds of lock-held run time for the final attempt. Lock waiting is
reported separately at the end.

### 11.78

| Stage | Result | Run (s) | First genuine failure / note |
| --- | --- | ---: | --- |
| 1 Source | PASS | 0.2 | `-ver` 11.78, DOCX, 1,101 files hash-equal to materialization |
| 2 Generate | **FAIL → PASS after I1** | 349.1 (fail), 394.4 (final) | `codegen.py`: `--fit-out requested but the dump has no Garmin module in scope` (details under F1) |
| 3 Expressions | PASS | 35.6 | 464/464 expressions, 12,740 probe comparisons, 14 inapplicable-to-probe Perl errors skipped |
| 4a Adapter build | **FAIL → PASS after I2** | 78.0 (fail), 76.9 (final) | lib test target: `E0599 no variant ExprId::ValBpm49633A` (F2) |
| 4b `cargo build --release` | PASS | 124.5 | Two `unused_imports` warnings in generated QuickTime keys/userdata specs (F5) |
| 4c `cargo test --workspace` | **FAIL** | 80.0 | lib: 4,838 passed, **113 failed**, 4 ignored. Other targets did not run (F3) |
| 5 Read | runs; not parity | 12.9 | See conformance table |
| 6a Write (rehearsal matrix) | **FAIL** | 221.5 | 2/1,530 matched. 1,528 rows refused: `ConvInv source is unsupported` (F4) |
| 6b Write (`--readback-*`) | **REFUSED** | 0.3 | `repository pin 11.78 has no reviewed native-write acceptance baseline; the 13.59 baseline is stale` (F6) |

### 12.64

| Stage | Result | Run (s) | First genuine failure / note |
| --- | --- | ---: | --- |
| 1 Source | PASS | 0.2 | `-ver` 12.64, DOCX, 1,179 files hash-equal to materialization |
| 2 Generate | **FAIL → PASS after I1** | 389.1 (fail), 428.5 (final) | Same as 11.78 (F1): 12.64 also has no `Garmin.pm` |
| 3 Expressions | PASS | 40.7 | 520/520 expressions, 14,369 probe comparisons, 14 skipped |
| 4a Adapter build | **FAIL → PASS after I2** | 79.9 (fail), 74.0 (final) | Same as 11.78 (F2) |
| 4b `cargo build --release` | PASS | 112.2 | Same two warnings (F5) |
| 4c `cargo test --workspace` | **FAIL** | 93.4 | lib: 4,872 passed, **79 failed**, 4 ignored (F3) |
| 5 Read | runs; not parity | 13.9 | See conformance table |
| 6a Write (rehearsal matrix) | **FAIL** | 233.7 | 2/1,530 matched (F4) |
| 6b Write (`--readback-*`) | **REFUSED** | 0.2 | Same as 11.78 (F6) |

13.59 control, same commit and inputs: generate PASS (574.3 s under
contention). Adapter build PASS. Read 10,149 matched. Write matrix **1,530/1,530**.
Readback matrix 1,020/1,020, with 902 successful write operations read back
across 38 distinct Group1 names.

## First genuine failures and root causes

**F1 — generation, both releases (fixed locally as I1).**
`tools/exiftool-tables/codegen.py` gates Garmin FIT generation with
`names is None or MODULE in names`. `names = args.modules or sorted(mods)` is
never `None`, and it is derived from the dump's own modules. So a release
without `Garmin.pm` can never select the module, and `fit_ledger` stays `None`.
The code then exits at the `--fit-out` check before the proven-absent path added in
#802 can run. #802's capture side works: the fact records
`garmin_fit_module_absent_v1`. Its codegen side was never exercised end to end.
The HANDOFF ledger listed the Garmin blocker as 11.78-only. 12.64 lacks
`Garmin.pm` too.
I1 changes the gate to `not args.modules or MODULE in args.modules`. With it,
both releases record `NOTE: Garmin module ABSENT ... the FIT reader extracts
nothing` and `FIT source rows: 0`.

**F2 — test compilation, both releases (historical; fixed by #846).**
The original `#[cfg(test)]` module hard-coded generated
`ExprId::ValBpm49633A` (`"$val bpm"`). The landed correction now resolves the
conversion by its Garmin source identity and treats only a generated,
source-proven `ModuleAbsent` result as inapplicable. It does not disable the
test or freeze a generated Rust identifier. The remaining Task19 matrix must
re-prove that behavior from fresh historical generation; this note is not a
substitute for that run.

**F3 — unit test suite, both releases.** The lib suite asserts 13.59 facts.
Classification of every failure, by assertion text (not by per-test native
re-verification):

| Mechanism | 11.78 | 12.64 |
| --- | ---: | ---: |
| Refusal: QuickTime ItemList/Keys/UserData protocol changed, all rows omitted | 24 | 23 |
| Refusal: ConvInv body not the 13.59 template, so public scalar writes refuse | 8 | 8 |
| Release has no `Garmin.pm`; FIT tests expect 13.59 rows | 5 | 5 |
| 13.59 census/snapshot constants (counts, allowlists, withholding snapshots) | 25 | 21 |
| 13.59 value/label fixtures the release lacks or labels differently | 51 | 22 |
| **Total** | **113** | **79** |

These counts are historical observations from the named 2026-09-18 logs, not
current qualification. Tests outside the Task19 tooling lease still need a
parent-owned correction based on generated per-release facts rather than
weakened assertions. Representative exact current locations are:

- `src/exiftool_tables/mod.rs:579`, `:789`, `:820`, and `:1059`: replace the
  fixed refusal/hook/subdirectory/offset census constants with a checked
  generator-owned fact keyed by the active generated release, while retaining
  the accounting identities.
- `src/exiftool_tables/runtime.rs:1779`: source the decoded/refused fractional
  census from that release's generated fact; retain the full per-table sum
  equality at `:1793`.
- `src/exiftool_tables/enabled.rs:285` and `:315`: bind allowlist presence and
  enabled-count expectations to an explicit per-release availability fact;
  do not silently drop absent tables.
- `src/parsers/specialized/fits.rs:1055`,
  `src/parsers/tiff/geotiff_parser.rs:543`, and
  `src/composite/lens_id.rs:1362`: replace 13.59 dictionary/map cardinalities
  with generator-produced per-release cardinalities, keeping uniqueness and
  lookup assertions generic.
- `src/parsers/jpeg/mpf_parser.rs:1162` and
  `src/parsers/tiff/makernotes/canon.rs:9411`: make label expectations follow
  the selected release's generated enum fact, including an explicit unknown
  expectation when the historical release lacks the value.

Those source files are outside this D2 edit lease and are intentionally not
changed here. Parent lease ruling is required before assigning them.

No failure is a production panic. Every panic is an `unwrap`, an index or a
map lookup inside the test itself, over a table that is correctly empty for the
older release. For example, Nikon `AF_POINTS_405` is absent in 11.78. The
12.64 failure set is a strict subset of the 11.78 set. The first genuine
defect is structural: the suite has no notion of "expectation for the pinned
release". Every upgrade needs a manual refresh of roughly 80 to 110 tests, and
nothing separates an expected upstream delta from a regression. (The
first failure listed is `composite::lens_id::tests::alternatives_keys_are_unique`,
236 vs 239 Canon EF alternatives: an upstream delta.)

**F4 — public write, both releases (correct refusal, total gap).**
`convinv_recipes.compile_convinv` requires the release's deparsed `ConvInv`
token stream to have exactly the length of `convinv_full_template.json`, the
13.59 body. Both older bodies differ, so the compiler records `emitted: false,
reason: "ConvInv body has unsupported statements"` and generates
`CONV_INV_RECIPE = None`. At runtime every generated scalar composition then
refuses with `generated TIFF scalar composition refused: ConvInv source is
unsupported`. This is fail-closed and follows the upgrade rule. It is also the
same whole-body-template pattern as blocker #1 (SetNewValue, #798), now on the
ConvInv helper. The only two matrix rows that pass are native-idempotent
`IFD1:XResolution`/`IFD1:YResolution` inserts on JPEG.

*Update (staging/rehearsal-convinv): F4 closed.* `compile_convinv` now admits
one whole-body profile per captured release (`convinv_full_template.json`,
`_11_78.json`, `_12_64.json`), selected only by exact token consumption, never
by the version label. Each profile must also contain its modeled regions exactly
once in the whitespace-stripped raw B::Deparse. Those regions are the default
type, the WriteCheck gate, both CHECK_PROC calls, the CHECK_PROC guard, the
PrintConv->ValueConv advance, conversion presence and the error result. The
bodies do differ from 13.59, and the ledger records why each difference is
exact for the scalar executor:

- 12.64 and 11.78 guard CHECK_PROC with `unless ($err2)` where 13.59 uses
  `unless (defined($err2))`. Only WriteCheck assigns `$err2` there, and every
  WriteCheck row is refused.
- 11.78 resolves the default into `$type` inside the loop. The first `$type`
  is the same falsy chain the executor computes.
- 11.78 also calls CHECK_PROC with three arguments, not four. An admitted
  CHECK_PROC binds exactly three arguments and has no statement that could
  read a fourth.

Stage 6a, `generated_tiff_write_matrix.py --route public-api` via the adapter,
each release against its own native tree, same base for before and after
(73315c00 + I1 + I2 + I3):

| | before | after |
| --- | ---: | ---: |
| 11.78 | 2/1,530 | **1,530/1,530** |
| 12.64 | 2/1,530 | **1,530/1,530** |

I3 is a third local-only intervention in the scratch worktrees, needed since
#823. `artifacts.py` now lists the per-module table files statically from
13.59. Each older release's correct regeneration drops some of those modules
(11.78: 10 binary + 12 IFD, and adds `ifd/json`, `ifd/rsrc`; 12.64: 5 + 7).
The write-set check can declare neither the dropped module nor its deletion,
so regeneration is refused before any stage runs.

**F5 — warnings (not yet a failure).** When the QuickTime generators refuse
every row, `generated_keys_specs.rs` and `generated_userdata_specs.rs` still
import `EnumOperand` and `SourceFormat`. The result is two `unused_imports`
warnings, so `cargo clippy -- -D warnings` (the CI lint) would fail on either
release.

**F6 — authenticated write readback is unavailable for any other release.**
With `--readback-*`, `generated_tiff_write_matrix.py` rejects
`--rehearsal-release` and calls `native_write_matrix.assert_contract_version`.
That function hard-requires `CONTRACT_EXIFTOOL_RELEASE = "13.59"`. The refusal is
correct and explicit, but the readback route cannot measure a rehearsal release
until there is a per-release native-write expectation.

## Read conformance

Instrument: `conformance.py` (via the stage adapter), header
`=== instrument: conformance.py ===`. The oracle is each release's own native
source under perl 5.38.2. The corpus is the same 194 files, hash-pinned.
OxiDex is the adapter-built all-features debug CLI. Release trees are the
scratch commit `9d63b624` (base `66e48654` + I1 + I2) plus that release's
regenerated artifacts: 60 files for 11.78 and 57 for 12.64, as the header
shows with `OXIDEX_ALLOW_DIRTY_TREE=1`. They were committed afterwards,
unchanged, as local `50303a05` (11.78) and `6f0dd1f5` (12.64). The control is
`66e48654` with 13.59.

| Build vs native | MATCH | VALUE | MISSING | RENAME | EXTRA | MATCH / (MATCH+VALUE+MISSING+RENAME) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 13.59 control vs 13.59 | 10,149 | 30 | 1,400 | 7 | 651 | 87.60% |
| 12.64 build vs 12.64 | 9,844 | 37 | 1,302 | 7 | 846 | 87.97% |
| 11.78 build vs 11.78 | 9,481 | 57 | 1,401 | 9 | 958 | 86.60% |

The fall in MATCH mostly reflects older native releases reporting fewer tags.
For example, FIT is −78 MATCH / −46 MISSING because neither release reads FIT.
Changes that are OxiDex's own:

- **MISSING +13 (12.64) / +16 (11.78) on M4A and +9 / +12 on MOV** come from
  the QuickTime refusal. The ItemList generator reports `eligible: false`
  because `ProcessMOV`'s contract changed (`missing_or_changed_processor_contract:PROCESS_PROC`),
  so 0 rows are generated: 352 of 352 omitted in 12.64 and 320 of 320 in
  11.78, against 92 generated in 13.59. Keys and UserData likewise generate 0
  rows, against 70 and 17 at 13.59. This is a visible, counted gap.
- **VALUE +7 (12.64) / +27 (11.78).**
- **EXTRA +195 (12.64) / +307 (11.78).** This is the one place a new-version
  build keeps 13.59-era behaviour; see the next section.

## Refusals, and the check for silently keeping 13.59 behaviour

**Generated layer: refuses as required.** Every one of the 66 manifest outputs
was rewritten after the pin change (mtimes checked). Six outputs (12.64: nine)
came out byte-identical because their upstream sources did not change, for
example the Mac CJK charsets and the Samsung lookups. After regeneration, the
only `13.59` strings left in outputs are two doc comments and the carried-forward
provenance history in the two SetNewValue ledgers. Each ledger's current
source records the release. Refusals are counted in
`<release>/refusal-diff-vs-13.59.txt`: 43 counters differ for 11.78 and 38 for
12.64. Most fall because the older releases have fewer tables. Refusals that rise
or newly appear:

| Refusal | 13.59 | 12.64 | 11.78 |
| --- | ---: | ---: | ---: |
| Nikon `PrintAFPointsLeftRight/UpDown` (sub absent) | 0 | 0 | 2 refused |
| IFD: Format outside the scalar grammar (tag not emitted) | 274 | — | 284 |
| Binary: fields refused for a per-field reason | 4 | 5 | 5 |
| Garmin FIT rows | 1,898 source / 5 refused | module absent (0) | module absent (0) |
| QuickTime ItemList / Keys / UserData generated | 92 / 70 / 17 | 0 / 0 / 0 | 0 / 0 / 0 |
| ConvInv recipe emitted | yes | **no** | **no** |

(— = unchanged from 13.59.) The unit tests confirm the generated layer follows
the release rather than 13.59. Canon RF `RFLensType 324` becomes
`Unknown (324)` on 12.64, which lacks that lens. Nikon 405/299/231-point grids
are empty on 11.78. The MPF `Gain Map Image` label is absent on 12.64.

**Handwritten layer: does not refuse.** On the same files, the release builds
emit **199 (12.64) / 315 (11.78)** tag occurrences that were not EXTRA in the
13.59 control. Native ExifTool at that release does not produce them. Some
come from whole formats the older release does not read (TNEF 37, XISF 25,
AAE 10, PCAPNG 9). Others are file-type identity for types the release does
not know (`FileType`/`MIMEType`/`FileTypeExtension`, 13 each on 11.78), and
`ExifTool.jpg` alone accounts for 61 / 67. These come from handwritten parsers and
detection with no release awareness. They are the silent 13.59 retention that
AUTOGENERATION-PLAN step 5 forbids. They are measurable here, and nothing
refuses them.

## Manual interventions and elapsed time

| # | Kind | Change | Releases | Cause |
| --- | --- | --- | --- | --- |
| I1 | code (tool) | `codegen.py` Garmin gate reads `args.modules` | both | F1 |
| I2 | code (test) | `#[cfg(any())]` on `BPM` + 2 tests in `runtime.rs` | both | F2 |
| O1 | operator | `verify_exprs.py` standalone needs `OXIDEX_ALLOW_DIRTY_TREE=1` on a regenerated, uncommitted tree | both | dirty-tree guard (by design) |
| O2 | operator | Re-ran adapter `build` before `read`: `cargo test --workspace` (default features) had replaced the adapter's all-features `debug/oxidex`, and the adapter correctly refused `built oxidex executable changed` | both | stage ordering in the rehearsal script |
| O3 | operator | Committed the regenerated tree locally and touched `src/lib.rs` to rebuild; the readback staleness guard compares binary mtime with HEAD commit time | both | readback requires a clean tree; staleness guard |

**Code interventions: 2 per release (target 0).** Both are tag-agnostic tool
or test defects, not tag rules. Operator interventions: 3. Patches are in
`interventions/` in the evidence root. Neither code intervention is on this
branch.

| | 11.78 | 12.64 |
| --- | ---: | ---: |
| Run time, final attempt of every stage | 965 s | 1,019 s |
| Run time, all attempts (failed + retried) | 1,874 s | 1,994 s |
| Shared-lock waiting | 14,723 s | 13,493 s |
| Wall clock, first queue to last stage | 286 min | 286 min |
| Build time (release / adapter all-features / test compile) | 124.5 s / 76.9 s / in 80.0 s | 112.2 s / 74.0 s / in 93.4 s |

Wall clock is dominated by the shared lock (about 7 agents on the host) and
by re-runs after I1/I2. It is not a measure of upgrade effort.

## Next

Follow-ups, each needing its own evidence:

1. **F1:** land the `codegen.py` gate fix with a test that runs codegen on a
   dump without `Garmin` plus a proven-absent fact. This is the blocker for
   every pre-13.x release.
2. **F2:** make the runtime FIT tests independent of a Garmin-only generated
   `ExprId`.
3. **F5:** the QuickTime spec generators should not emit unused imports when
   they refuse every row.
4. **F4:** done (per-release ConvInv profiles plus operand proofs, see F4).
5. **F3:** make pinned-release test expectations regenerable, or bind them to
   the pin, so an upgrade separates upstream deltas from regressions.
6. **Handwritten retention:** gate handwritten parsers and detection by
   release, or at least report them, so EXTRA growth is refused rather than
   silent.
7. **F6:** capture a per-release native-write expectation so `--readback-*`
   can measure a rehearsal release.
8. **I3 (#823):** `artifacts.py`'s per-module table outputs must follow the
   release. The fix, deriving the stems from the regenerated hubs, is on
   `staging/rehearsal-zero-intervention` (0291b28d, 447cff6b). It retires I3.

Re-run this rehearsal unchanged (same pair, same corpus) after 1–3 to reach the
read/write stages without code interventions.
