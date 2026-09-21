# Plan: make ExifTool upgrades drive the tags

Updated 2026-09-18. This is the main plan for deciding what to do next: the
goal, the progress rules, the measured state, and the ordered next steps.
The *mechanism* is specified in [`AUTOGENERATION-V2-DESIGN.md`](AUTOGENERATION-V2-DESIGN.md)
(approved 2026-09-18); this document does not repeat it. The batch-by-batch
history from PR #745 to #764 lives in [`AUTOGENERATION-PROGRESS.md`](AUTOGENERATION-PROGRESS.md)
and [`UPGRADE-NEXT-STEPS.md`](UPGRADE-NEXT-STEPS.md); those records are
accurate for their commits and are not restated here. Every number below
names its instrument and commit. A number without both is not evidence.

## The goal

A change to ExifTool's tag definitions should flow into OxiDex by regeneration,
without someone retyping the tag name, byte location, camera-selection rule or
conversion in Python or Rust. **Moving a hard-coded tag rule into a generator
or a shared helper does not count as automating it.**

What "100% generated" means, so that it is a number the ratchet can track:

- **Generated from source:** every tag definition, layout, selection
  `Condition`, `RawConv`/`ValueConv`/`PrintConv` and their inverses, evaluated
  over a `Session` that models ExifTool's `$self`.
- **Hand-ported and enumerated:** container walkers (`ProcessMOV`,
  `ProcessJPEG`, RIFF, PDF, OLE, …) are procedural Perl, not tables. Each is
  ported 1:1 from a **named** Perl sub, fenced in its own module, and counted.
- **Refused and counted, never approximated:** an expression outside the
  grammar, a helper without a port, or a walker not yet ported is an explicit,
  counted gap. A plausible wrong value under a real tag name is worse than an
  absent tag.

The working pin is 13.59; it does not limit the version scope. Every generated
build must conform to the native release it came from; the selected release
pair for upgrade rehearsal is 11.78 / 12.64 and must not be redrawn to avoid a
failure. Native read-only tags need no write path; native writable tags need a
separately verified one. A corpus is a test population, not a way to exclude
unexercised behaviour.

## Where we are now (measured)

| Axis | Measured | Instrument, commit |
| --- | --- | --- |
| Read parity, catalog entries proven | **2,378 of 33,487** (7.10%); of those ExifTool reads in the corpus, **55.69% of 4,270**; 0 lost since the previous snapshot | published `catalog-corpus-observed-13.59.json`, evidence at `6ada109b` (#822), rebound to the split layout in #823 |
| Read parity, corpus identities | **3,089** `Group1:TagName` identities matched in both print and raw modes; 2,766 credited source coordinates | `corpus_read_receipt.py`, 194 files, locked in by the ratchet in #822 |
| Write parity | **19 of 14,169** writable entries observed (0.13%); 1020/1020 public-API scalar write operations matched | `generated_tiff_write_matrix.py --route public-api`, joined at `7547ec5b` (#797) |
| Generated share of correct output | **43.43%** of 468,294 matched values, a floor: 37.52% direct, 5.65% Composite values computed from generated rows, 0.27% other cascade; engine alone 38.64%. Up 5.10 points from 38.34% at `72eae8a5` (reproduced exactly), and all of the rise comes from #838: `8cceb4a7` → `2d8ff775` adds 23,943 rows (engine +23,945) | probe census `genshare-probe/1` (`tools/exiftool-tables/genshare/`), `2d8ff775` (2026-09-18), 4,238 files against pinned 13.59; result in `docs/public/measurements/generated-share-13.59.json` |
| Generated reader declarations | 3,666 catalog entries strict (10.9%); 5,363 loose (16.0%). 80.11% of proven reads (1,905 of 2,378) sit on rows with no usable generated declaration (recomputed by `tools/docs/render_status.py` from the committed snapshot; the earlier ~82% was an estimate) | `catalog-hydrated-join-13.59.json` counts, `0f92071b` |
| Expression coverage | `exprs.py` translates **75.4%** of expression uses; a real grammar parses **99.7%**; session + top 22 helpers reaches **95%**; 157 helper subs, 10 complete + 4 partial ports today | coverage spike `run_spike.py`, `07d808a0` (#817), byte-identical under `PYTHONHASHSEED=1,2,3` |
| Generated artifacts | **255** manifest outputs after the per-module split (was 66 as two monoliths, not 44 as previously written) | `artifacts.py paths`, #823 |
| Upgrade rehearsal 11.78 → 12.64 | **First end-to-end run done (#826).** Generate, `verify_exprs` (464/464, 520/520) and release build pass, but only with 2 local code interventions per release; `cargo test` fails 113 / 79 (13.59-specific assertions); public-API writes 2/1,530 (ConvInv whole-body template); reads 86.6% / 88.0% vs each release's own ExifTool (13.59 control 87.6%) | `docs/reference/upgrade-rehearsal-11.78-12.64.md` at `66e48654` |
| CI guarantees | shard count derived from the matrix (#794); parity ratchet, 25 metrics (#795, raised in #822); corpus read-regression gate on every PR (#814); fixtures-directory guard (#811); non-blocking benchmark job on every push to the tip against pinned 13.59 (#825) | `tools/ci/`, `.github/workflows/benchmarks.yml` |
| Benchmarks (historical, non-candidate) | **Historical refresh against the pin (#821)**: oxidex vs ExifTool **13.59** (perl 5.38.2, DOCX probe asserted), measured at `8f04e288` under the exclusive lock: 3.0x on Canon.jpg, 1.83x per core on the 194-file corpus (6.15x with rayon). These values are not v2.0.0-beta.1 evidence; an exact-candidate receipt is required. The 2025 `exiftool-rs 0.1.0` vs bare 13.36 table is kept below it as historical. CI `metrics` still runs only on `main` (proposal in #821) | `benches/benchmark_results.md`, `ci.yml` `metrics` |

Two things these numbers say that shape the plan:

1. **Reads and generation are nearly independent today.** Most proven reads
   come from hand parsers; most generated declarations are unobserved. Raising
   the generated share means *retiring hand parsers onto generated code without
   losing a proven read*, which is exactly what #814 now enforces per PR.
2. **The grammar is not the work; the helper library and the session are.** A
   bare interpreter would cover *less* than today's `exprs.py` (69.6% vs
   75.4%), because `exprs.py` already inlines about a dozen helpers. Coverage
   comes from porting helpers once and giving conversions a `$self`.

## What we will do, in order

| Step and goal | How | Evidence required to call it done |
| --- | --- | --- |
| **1. Session + helper library, proven on `Exif::Main`.** | Build the `Session` (typed `model`/`make`/`byte_order`, a map for the rest, Perl truthiness as a method) and port the top 22 helpers by use, each selected by exact source match. Emit `Exif::Main` as generated `match` arms over the session, formatting at extraction time, writing (raw, ValueConv, PrintConv). | `verify_exprs.py` proves every emitted conversion against the oracle; the corpus read-regression gate (#814) reports 0 proven reads lost; each helper has a differential test. |
| **2a. Typed values beside display strings.** | #838 found generated arms produce Perl strings while library callers and output rules read typed values; a workaround keeps the typed value only where PrintConv leaves the text unchanged. Make the sink record both of ExifTool's tracks: the ValueConv value (typed) and the PrintConv string, so nothing downstream re-parses display text. Required before maker-note tables, which lean on typed values. | Every typed-value change listed; library API tests pass unchanged or the change is in the changelog; 0 proven reads lost (#814). |
| **2. Retire what `Exif::Main` replaces.** | Per-field mixed mode: the generated decoder takes every field it can prove; the hand parser keeps only the refused fields, listed. Delete the `exiftool_compat.rs` branches `Exif::Main` now formats itself. | The PR names the exact manual rules and compat branches removed; #814 green; the ratchet moves only forward. |
| **3. Re-measure the generated share.** | Run the probe census on the same corpus and control method as `72eae8a5`. | A fresh generated-share figure at a named commit; the delta attributed to step 2. If the share does not move, the architecture is not yet validated and step 4 does not start. |
| **4. Expand by the spike's curves.** | Next helpers and session keys in use-count order; next families by proven-read count. One difficult neighbouring family tests that the design generalises. | Each batch lists helpers ported, families migrated, manual rules retired, refused fields remaining, and the proven-read count before and after. No vendor-specific translator. |
| **5. Run the upgrade rehearsal end to end.** | With #818 merged, execute the persisted 11.78 / 12.64 rehearsal: pin per release, generate, build, read and write against each release's own native ExifTool. Do not redraw the pair. | Every stage passes or names its first genuine failure; a new unsupported rule refuses rather than silently keeping the old one; elapsed time, manual interventions and per-release conformance recorded. |
| **6. Close the inventory.** | Repeat 1–4 until no tag rule in scope depends on manual knowledge; port and enumerate the remaining container walkers. | Zero manual tag-specific rules, zero unclassified rules, every walker enumerated; the ratchet at 100% on the generated axis with the walker count published beside it. |

Steps 1 and 5 can proceed in parallel; step 3 gates step 4.

## Progress rules

| Measure | What we show | Target |
| --- | --- | --- |
| Manual tag knowledge | Rules before and after; exact rules removed; any new exception | Falls to zero. A new exception is visible debt. |
| Proven reads | `observed_matched_read` and corpus identities matched, both modes | Never falls (#814 fails the PR; the ratchet refuses the snapshot). |
| Generated dependency | Correct rows lost when generated routes are disabled | Rises; reported as a floor, never as automation. |
| Upgrade effort | Tag-specific edits, causes, elapsed and build time for the named pair | Zero edits for supported native changes. |
| Unfinished work | Refused fields, unported helpers, unported walkers, unexercised behaviour | Visible until resolved and verified. |

A rule is counted by its source identity, so copying it into Python and Rust
is not two units of progress. Each item reports **planned, implemented,
validated or merged** with a commit and its evidence. Generated line counts,
agent activity, table existence and green syntax checks are not outcomes.

Two measurement rules learned this month, both enforced in CI now:

- `cargo test --workspace` does not run `#[ignore]`d tests; a key rename can
  break a pinned-fixture test invisibly. Sweep `--ignored` per target and diff
  against a baseline worktree.
- Historical-release or hand-captured test data belongs in
  `tools/exiftool-tables/testdata/`, never `fixtures/`, which CI regenerates
  from the pin and diffs (#811 fails the lint job otherwise).

## How the work is run

One host (the M5). One agent, one worktree, one `staging/<slug>` branch off
`origin/refactor/tag-machinery`; nothing edits the main checkout, and nothing
lands on `main` from this tree. Heavy measurement (corpus receipts, censuses,
timing) takes the shared lock so a receipt is never starved into false
timeouts; a starved measurement reads as a regression. Gates currently take
the same lock, which is the throughput bottleneck: the next infrastructure
change is a reader-writer lock (gates shared, receipts exclusive), not to be
swapped in under live agents.

Agents queued on the lock must wait in the foreground; a stopped agent is not
woken. Never `pkill` a pattern that can match another agent's process. Every
PR names the instrument beside every number, and every merge is squash-only
after the maintainer's or coordinator's independent verification of the
central claim, not the agent's report of it.

## The next checkpoint

Done since the plan was rewritten: #818 (all 15 generation blockers), the
end-to-end rehearsal (#826), the snapshot refresh (#822), the benchmark
refresh (#821, #825), the coverage spike (#817) and step 1's first slice
(#824: `Session` + 17 helper ports, 7,380 probes, 0 mismatches).

1. **Step 1, `Exif::Main` end to end** (in progress): conversions emitted
   over `Session` in per-field mixed mode, 0 proven reads lost; `Decode`
   ported (in progress) to unblock its largest refused group.
2. **Rehearsal to zero interventions** (in progress): fix the Garmin codegen
   gate and the FIT test coupling so 11.78 / 12.64 generate and build with no
   local patches; per-release ConvInv templates so writes are not shut off.
3. **Two structural findings from #826, on the path to 100%:**
   - The test suite asserts 13.59 facts (80-110 failures per older release).
     Tests must split into release-independent behaviour and assertions
     derived from the release's own oracle, or every upgrade needs hand edits.
   - Hand parsers keep 13.59 behaviour on an older pin (199 / 315 tags the
     release's own ExifTool does not emit); only generated code follows the
     release. This is measured evidence for step 2's retirement of hand
     parsers, and a rehearsal metric to drive to zero.
4. ~~Re-measure the generated share (step 3) once `Exif::Main` lands.~~ Done
   at `2d8ff775`: 38.33% → 43.43% across #838 (+5.10 points, engine alone
   +5.10). The share moved, so under step 3's rule step 4 may start.

We do not yet have an evidence-based date for 100%. Record the effort for
step 1 and the first expansion batch before forecasting the rest; unusual
native programs (encrypted walkers, dynamic-length processors) are much harder
than a new enum entry and stay visible in the estimate.
