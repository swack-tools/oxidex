# Tag machinery: completed work and useful next steps

**Status reviewed:** 2026-09-10 against `refactor/tag-machinery` at
`c7f5dd81eac5` (ExifTool pin **13.59**). This is a dated integration-branch
snapshot, not a claim about `main` or a released binary. The documentation
change itself does not implement or enable any parser behavior.

An implementation completed after this audit is recorded under
[pending upgrade-classifier repair](#pending-upgrade-classifier-repair).
Its branch has not landed in the integration snapshot above.

Start here to decide what to work on. Use the
[implementation backlog](./AUTOMATION-AND-TESTER-PLAN.md) for acceptance criteria,
[Transcription](./TRANSCRIPTION.md) for the method, and
`tools/exiftool-tables/README.md` for commands. Older plans are design records;
their unchecked steps and old measurements are not a current task queue.

## What the evidence establishes

The audit inspected committed source, first-parent landing commits, local branch
ancestry and the existing reports. It ran `reachability.py` on a clean checkout.
It did **not** rerun the full Rust acceptance gates, the expression oracle, a
corpus comparison or a version bump. A gate result quoted below is a **recorded
result** in the named commit/report, not a new test result from this
documentation audit.

Status terms:

- **Landed:** implementation is in the reviewed integration commit. This does
  not certify all behavior or the original stage's entire acceptance checklist.
- **Partial:** useful implementation exists, with the specific missing part named.
- **Unlanded:** a separate local branch contains work; its tests, merge and
  production effect must be established before marking it complete.
- **Proposed:** a useful next step with no completed implementation established.
- **Historical/deferred:** retained for context; do not execute it as the next task.

## What is already implemented

| Area | Status at the reviewed commit | Evidence and limit |
| --- | --- | --- |
| Tag-name catalog | Landed | `src/tag_sync/` and the tag crates consume the documentation view. Knowing a name does not establish a parser, layout or output. |
| Binary table transcription | Landed | `dump_tables.pl`, `codegen.py`, `src/exiftool_tables/binary_tables.rs`, independent `oracle.pl`/`verify.py`. Unsupported behavior remains. |
| IFD table transcription and engine | Landed, I-1 through I-3 | `ifd_schema.rs`, `ifd_tables.rs`, `ifd_engine.rs`; landings `83bff055`, `d4d6528b`, `25a2109e`. IFD generation is no longer future work. |
| Expression and condition translation | Landed, partial language coverage | `exprs.py`, `conds.py`, shared Rust helpers and differential verifiers. New language forms and stateful behavior can still require implementation. |
| Expression/value-conversion ledgers | Landed | `expr_oracle_ledger.json`, `value_conv_ledger.json`; codegen checks the dump digest, version and verdict. These are not a complete declaration-to-runtime ledger. |
| Shared decoding and table activation | Landed, partial migration | `engine.rs`, `ifd_engine.rs`, `enabled.rs`, `enabled_ifd.rs`. Many parsers still use legacy generated-table adapters or handwritten code. |
| Olympus IFD migration | Landed for Main and seven subtables | I-2/I-3; `src/parsers/tiff/makernotes/olympus.rs` and its residual tables. Withheld conversions still have hand implementations. |
| ICC header migration | Landed | `01fc51b7`; the generated `ICC_Profile::Header` replaced the old handwritten header transcription. |
| Occurrence store and output projection | Landed foundation; broader migration partial | `src/core/tag_occurrence.rs`, `tag_sink.rs`, `metadata_map.rs`, `src/cli/tag_resolution.rs` and `tests/step20_output_projection_matrix.rs`. Presence of the store does not establish complete raw/value/print forms or source provenance for every producer. |
| Three August occurrence defects | Fixes landed | `9f087f55` repairs removal/tombstones, ungrouped `-a` duplicates and track timestamps. Do not schedule the old D1/D2/D3 findings as unfixed merely because an August plan or test comment calls them red. |
| Structured read status | Landed foundation; comparison integration partial | `src/core/read_report.rs` exists. `conformance.py`'s `parser_status()` and `family_views()` hooks remain placeholders. |
| Lossless comparison accounting | Landed | `380babda`: family `0:1:4` oracle keys and leftover occurrence counting, plus JFIF/PanasonicTitle corrections. `c7f5dd81` extends oracle-key corrections to other scripts. |
| Release-bump and staleness tooling | Partial | `bump-exiftool.sh`, `triage_bump.py`, hand-enum and Perl-anchor checks exist. The bump workflow has the gaps below; it is not a verified unattended release path. |

## Generation is not runtime coverage

Instrument: `python3 tools/exiftool-tables/reachability.py --json-out <report>`
on the clean `c7f5dd81` checkout, 2026-09-10. It reads the generated Rust and
allowlists; it runs neither ExifTool nor OxiDex and uses no corpus.

| Table kind | Emitted | Enabled through generic engine gates | Eligible but not enabled | Refused by static gate |
| --- | ---: | ---: | ---: | ---: |
| Binary | 622 | 9 | 476 | 137 |
| IFD | 496 | 8 | 253 | 235 |
| Total | 1,118 | 17 | 729 | 372 |

The 17 enabled tables are not a percentage of extraction coverage. The legacy
path in `runtime.rs::decode_field` still consumes generated tables when the new
shared reader is not enabled; vendor-specific generated tables have additional
interpreters. Static lookup scans also include test references and dynamic
lookups: they are not observed execution counts.

Likewise, generated line counts include schemas, lookup data, comments and
formatting. They do not measure developer effort, supported declarations or
release-maintenance cost. The historical 83.5% declaration classification in
Transcription and 36.5% AUTO in the old bump report answer different questions.

## What is incomplete and worth doing

The priority below is the recommended order for **new work**. Existing owners
may finish their bounded branches in parallel; this list does not restart them.

| Priority | Work | Why it is useful | Completion evidence |
| --- | --- | --- | --- |
| 1 | Repair the existing bump transaction and classifier | Makes upgrade maintenance measurable and prevents mixed-version artifacts | Complete generated-output manifest, complete old/new builds, IFD-aware triage, reliable failure/cleanup behavior; see backlog item 1 |
| 2 | Rehearse 13.55 to 13.59 with the repaired workflow | Separates actual new-release work from accumulated compatibility debt | Reproducible work order and artifact/probe/corpus results for both releases, with explicit unsupported and unexercised changes |
| 3 | Finish the IFD1/shared EXIF work already started | Reuses the Exif table for a large missing-output cluster instead of adding individual thumbnail tags | Landed routing plus measured occurrence-preserving comparison and residual ownership; predicted gains are not yet results |
| 4 | Connect existing accounting into a declaration/producer inventory | Exposes generated-but-off tables, handwritten ownership and completely unemitted tables | Every upstream declaration gets an explicit disposition; preserve separate static, enabled and actually observed execution states |
| 5 | Extend shared semantics and synthetic walk checks for the next migration | Removes a demonstrated blocker across multiple tables | Independent differential checks, negative controls and a measured runtime migration |
| 6 | Retire duplicated hand data and reconstruct needed missing generators | Reduces future upgrade review without losing behavior | Replacement regenerates reproducibly; overlapping producers removed only after parity; remaining hand residuals name their reason |

### Concrete upgrade gaps at this snapshot

These are source-review findings; the documentation audit did not execute the
bump command. They remain open implementation work.

1. `bump-exiftool.sh`'s `TIER1_FILES`/`TIER2_FILES` lists lag the generators.
   Outputs absent from its backup/restore accounting include IFD tables,
   expression/value-conversion ledgers, the generated Composite computation
   companion, recovered Sony/Minolta tables and Macintosh charset tables.
   Derive the list once for generation, backup, restoration and verification.
2. The temporary BEFORE build regenerates only tier 1. Tier 2 remains at the
   target release, so the comparison is not between complete old/new artifacts.
3. A real pin change dirties the checkout before `verify_exprs.py` runs;
   that verifier rejects a dirty tree unless the intended generation operation
   provides the explicit, recorded override. The wrapper also calls `verify.py`
   without that override. Fix the transaction without weakening normal audit
   provenance checks.
4. `triage_bump.py::diff_table` and layout classification only recognize the
   binary pathway and tier-2 manifest; they do not account for supported IFD
   generation. A fresh AUTO/HAND percentage from that classifier is misleading.
   An unlanded repair now exists; see the follow-up below before starting
   another implementation of this item.
5. Four generated-origin files have no committed generator:
   `sony/plain_tables.rs`, `sony/enciphered_tables.rs`,
   `nikon/settings_tables.rs`, `nikon/encrypted_tables.rs` under
   `src/parsers/tiff/makernotes/`. `regen-all.sh` explicitly records the limit.
   Count these as standing debt, not new work introduced by every release.

The [August 11 rehearsal](./reference/bump-reports/13.58-to-13.59.md) remains
valuable evidence: 42 AUTO, 10 COND and 63 HAND classifications, including six
standing HAND items at that time. Its corpus did not exercise the changed
declarations directly and its BEFORE rebuild was tier-1-only. It does not
establish today's automation share or an hours-per-upgrade estimate.

### Other useful residual work

- Complete per-producer raw/value/print forms and source provenance where they
  affect output; do not confuse `TagOccurrence`'s existence with universal use.
- Connect the existing `ReadReport`/occurrence data to conformance's placeholder
  hooks when it helps explain missing or mis-grouped output.
- Preserve a separate check for generated Composite computations: the scalar
  expression oracle excludes that input domain. Composite dependency generation,
  hand computations and generated computations already exist.
- The original step 30 catalog consolidation is unfinished: `src/tag_sync/` and
  `src/bin/sync_tags.rs` still use the separate `-listx` path. Retire that path
  only after dump-based generation preserves the existing catalog/writer contract;
  assess its upgrade benefit after the release rehearsal.
- Audit unsupported custom processing procedures and handwritten MakerNote
  dispatch. Inventory omissions as omissions; a missing emitted table is not
  evidence that ExifTool has no declaration.
- Evaluate remaining old-plan features such as Composite `Override` against a
  concrete upstream use and an observable gap before implementing them.

### Pending upgrade-classifier repair

Commit `b36983c2` on `codex/ifd-upgrade-triage` repairs IFD declaration
classification using the actual generator's tag/variant emitter and refusal
counters. Supported fields can receive AUTO; ignored facts, unsupported shapes
and withheld conversions remain explicit review work. Full-dump context is used
for subdirectory targets. The existing binary classification is unchanged.

Validation on 2026-09-10: 36 focused triage tests passed; the broader Python
tool suite ran 269 tests with one dump-dependent skip; formatting and ordinary
Clippy passed. Independent review found no blocker. An additional strict
all-targets/all-features Clippy run failed on 11 inherited `print_literal`
findings in unchanged `tests/raw_metadata_parsing.rs`.

This is pending branch work, not an integration landing or upgrade rehearsal.
Triage has no oracle-ledger input, so HAND may mean missing verification evidence
rather than a need for new source code. Equivalent-to-default facts may also
remain conservatively classified for review. Artifact accounting, complete
old/new builds, transaction cleanup and a release rehearsal are still open.

## Work on separate branches

Local refs inspected on 2026-09-10. None of these tips was an ancestor of the
reviewed integration commit. Branch names and comments are not live process
status or proof that the work is ready to land.

| Branch | Observed tip | Scope and remaining distinction |
| --- | --- | --- |
| `staging/unify-rawconv` | `0bf02cd9` | Shared embedded-EXIF conversion; local handoff reports implementation/reviews complete, integration gate/landing still outstanding |
| `staging/ifd1-gate-a` | `7a69d2fa` | Initial IFD1 generator-policy work; table eligibility is not activation, and this is not the IFD1 runtime landing |
| `staging/ifd-4` | `be0df53e` | Further conditions and Olympus retirement; new manufacturer Main routing must not be inferred from the branch name |
| `staging/png-text-names` | `0bf02cd9` | Same committed base as RawConv; no separate implementation commit at the inspected ref |
| `staging/embedded-ifd0` | `0bf02cd9` | Same committed base as RawConv; no separate implementation commit at the inspected ref |

The local handoff forecasts roughly 15–18k missing occurrences recoverable by
IFD1 routing. Treat that as a prioritization estimate, not a measured gain or a
promise. Check fresh refs and the owning task before starting duplicate work.

## What happened when

Dates below use America/Chicago. Landing timestamps come from first-parent Git
history; author dates, commit dates and working-file modification times are not
interchangeable, and none measures time spent working.

| Date | Recorded milestone |
| --- | --- |
| Aug 10 | Original six-stage plan, `9c2f79d1`; integration/gating rules updated in `91e0ba02` |
| Aug 11 | Expression compiler and historical bump rehearsal, `3740b669` / `1f1f2cb2` |
| Aug 12 | Occurrence foundation, exemplar migration and output projections, `fcad6033`, `13628191`, `914d1d6c` |
| Aug 13–14 | Schema/shared BinaryData work and activation gates; two missing generators reconstructed, `e263aa40` |
| Aug 21–27 | The 31 non-merge commits in this interval reachable from the audited tip concern fleet/Keel infrastructure; this is commit purpose, not an effort estimate |
| Aug 28–30 | Format-specific coverage work, reconciliation repairs and occurrence fixes; wave 5 landed `6441e824` on Aug 30 at 02:28 |
| Sep 6 | Shared expression forms, formatter consolidation and ICC migration; IFD foundation landed `83bff055` at 22:46 |
| Sep 7, 16:12 | Olympus Main via IFD engine, `d4d6528b` |
| Sep 8, 16:45 | Seven Olympus subtables and engine exactness work, `25a2109e` |
| Sep 9 → Sep 10, 04:48 | Lossless census and JFIF/PanasonicTitle fixes authored, then landed as `380babda` |
| Sep 10, 11:44 | Oracle-key corrections in other scripts landed as `c7f5dd81`; Rust unchanged from `380babda` |

The September 9 census repair changed the instrument's denominator. The local
`cfix` record reports 4,238 files, 450,021 matches, 30,196 MISSING, 524 VALUE and
1,565 EXTRA under the corrected `conformance.py`. These are **recorded local
results**, not regenerated evidence shipped with this page. Older aggregate
scores that dropped duplicate oracle occurrences are not comparable. Use the
current comparison script on both binaries in a new A/B.

## Original plans: keep, finish or defer

| Original area | Current disposition |
| --- | --- |
| Stage 1: specific wrong-value fixes | Historical landed fixes and tests exist; the original blanket exit checklist was not re-executed in this audit. Do not restart from step 1 or claim all present formats are correct. |
| Stage 2: accounting and refusal APIs | Useful foundations landed. Complete inventory and comparison-hook integration remain partial. |
| Stage 3: regeneration, staleness and bump | Implemented tools with known integration gaps. Repair and rehearse them before replacing them. |
| Stage 4: occurrence/output contract | Store and major follow-up fixes landed. Full producer/value-form/provenance migration remains incomplete. |
| Stage 5: schema and engine | Binary and IFD engines exist; semantics and routing remain partial. Extend the existing engine for a demonstrated blocker. |
| Stage 6: routing, retirement and coverage | Ongoing. Olympus/ICC are concrete landed migrations; IFD1 and other branches above are unfinished at this snapshot. |
| Sep 9 tester proposal | Retain inventory, release work orders and incremental differential walk checks. The ten-week schedule and broad proof claims are not accepted completion evidence. See the revised backlog. |
| Fleet expansion / mass per-tag patching | Defer as a metadata-coverage strategy. Reconsider only for a measured operational bottleneck; the current priority is generation, runtime adoption and release maintenance. |

Do not turn a passing finite probe suite into a proof of equivalence for all
possible inputs. The expression/condition/subdirectory verifiers and existing
Rust/carrier tests are useful today. A general differential full-walk checker
and complete declaration-to-output ledger have not been established by this
audit; build the missing integration incrementally, with negative controls.

## Documentation map and authority

| Document | How to use it |
| --- | --- |
| This page | Dated integration status, evidence limits and dispositions; update after a relevant landing |
| [Automation backlog](./AUTOMATION-AND-TESTER-PLAN.md) | Remaining useful work and acceptance criteria; proposals do not authorize changes to running branches |
| [Transcription](./TRANSCRIPTION.md) | Method and historical lessons; old census figures are labeled historical |
| `tools/exiftool-tables/README.md` / `CONFORMANCE.md` | Tool scope and current comparison contract |
| [IFD design](./superpowers/specs/2026-09-06-ifd-tables-design.md) | Design and original I-1–I-4 sequence; consult this page for landed state |
| [Bump report](./reference/bump-reports/13.58-to-13.59.md) | Historical experiment with explicit limits; do not reuse its percentage as current |
| [Corpus synthesis](./reference/corpus-synthesis.md) | Historical sample-generation feasibility study, not a general walk checker |
| `OVERHAUL_OXIDEX_PLAN.md` and `OVERHAUL_STEP*_*.md` | Original roadmap and design decisions; no stage is complete merely because its design was approved |
| Local `HANDOFF.md` | Short-lived owner/queue/next-command context; verify branch/gate facts and copy durable results into this page |
| Local `OVERHAUL_PROGRESS.md` | August execution history; not today's checklist |
| Local `docs/TAG_MACHINERY_RECONCILIATION.md` | August 28 audit against `63d13641`; useful historical findings, including defects subsequently fixed |
| Local `docs/TAG_MACHINERY_LEDGER_PLAN.md` / `EVIDENCE_LEDGER_DESIGN.md` | August proposals; useful remaining integration carried into the backlog, not a requirement to build another parallel ledger |
| Local `docs/FLEET-COMPLETION-PLAN.md` | Mixed fleet/coverage progress log; its title and early roadmap no longer describe the whole current effort |

“Local” files were present in the maintainer checkout during this audit but are
untracked/ignored and may be absent in a clone. The useful dispositions are
recorded here so a new checkout can resume without those files. External August
architecture reviews also describe their named historical commits, not this tip.

## Keeping this page current

1. Record the integration SHA and pin before refreshing a claim. A branch's
   implementation commit and its landing commit are separate facts.
2. For each completed item, give the source/landing and instrument, inputs,
   result and evidence location. Separate freshly run checks from recorded ones.
3. Move landed branches out of the unlanded table; retire their old blockers and
   update the corresponding backlog item in the same documentation change.
4. Re-run `reachability.py` on the intended clean checkout when tables or
   activation change. Keep its static counts separate from corpus execution.
5. When the comparison instrument changes, invalidate incompatible historical
   baselines and run both sides with the same instrument. Never silently replace
   an old report's numbers with a new measurement.
6. Keep unresolved work specific: missing behavior, why it matters, next bounded
   action and acceptance evidence. Avoid completion percentages for entire stages.
