# ExifTool upgrade execution plan

Started 2026-09-10 from `0683cb11447adef3945f19da5d6917c88aea2f7d`,
the tested artifact-manifest branch. ExifTool remains pinned to **13.59**.
This file records the current implementation sequence; the
[automation backlog](./AUTOMATION-AND-TESTER-PLAN.md) owns broader acceptance
criteria and [status page](./TAG_MACHINERY_STATUS.md) owns historical evidence.

## Current work

The upgrade tooling is implemented, validated and landed through PRs [#737](https://github.com/swack-tools/oxidex/pull/737), [#738](https://github.com/swack-tools/oxidex/pull/738), [#739](https://github.com/swack-tools/oxidex/pull/739), [#740](https://github.com/swack-tools/oxidex/pull/740)
on `refactor/tag-machinery` at
[`2cea1e41`](https://github.com/swack-tools/oxidex/commit/2cea1e4194ce7fc1aeed4b110d77ad436ccd7837).
The completed milestones below retain their original validation commits and
populations. They are implementation history, not work to restart.

| Priority | Remaining work | State | Completion evidence |
| --- | --- | --- | --- |
| 1 | Reconcile RawConv, then IFD1 prerequisite/regeneration | Separate owners; awaiting fresh validation | Preserve current raw-ID attachment and verified CODE-ref gates; IFD1 eligibility remains separate from subsequent activation; IFD4/embedded follow independently |
| 2 | Correct classifier/producer accounting exposed by the release rehearsal | Queued | Existing Garmin runtime is recognized, standing debt is separated, repeated field changes are joined by cause, and emitted/activated/observed states remain distinct |
| 3 | Broaden walk checks and reconstruct useful missing producers | Queued | Demonstrated migration blockers addressed; deliberate bad offsets/conversions fail; residual and unexercised behavior stays explicit |

The [13.55-to-13.59 retrospective rehearsal](./reference/bump-reports/13.55-to-13.59.md)
passed on 2026-09-11 at `4fb705da` in **611.006 seconds**, with zero source-edit
interventions and unchanged caller source/index/pin. Both sides regenerated all
28 outputs using the same current handwritten runtime. Against the same 13.59
oracle and 193 files, MATCH rose 9,963 to 9,966 and VALUE fell 34 to 31; only
three Garmin file-identity values improved. MISSING remained 1,563. This measures
one controlled refresh, not historical upgrade effort or complete coverage.
The earlier 28-output same-pin acceptance at `1428b6c7` remains recorded below. Four Sony/Nikon outputs still lack
committed producers. Catalog synchronization has separate carry-forward
semantics and is outside this table transaction until that policy is resolved.
See the [landing record](./TAG_MACHINERY_STATUS.md#landed-upgrade-tooling) for the
four verified squash commits.

The runtime work order is RawConv (`ad9fdc32`, then `0bf02cd9`) on a fresh
integration base, preserving current Canon label/raw-ID attachment, followed by
IFD1 prerequisite `7a69d2fa` with the current verified-key/input-domain CODE-ref
gate and fresh regeneration. Its committed table changes were `unwalked: None`
fields; improved eligibility was a scratch preview. Gate A is not Exif::Main
activation or an extraction result. Separate later named-directory activation
from IFD4 conditions/Olympus retirement and the incomplete embedded-IFD0/PNG
follow-ups. IFD4 can reconcile independently without delaying RawConv. Existing
old-head gates support review; fresh baseline/candidate and occurrence-aware
acceptance are required before any landing.

### Operational preservation on 2026-09-11

A verified recovery archive preserved **1,396 Git refs and 28 working-file
payloads**. After restore/dependency checks, an atomic retirement removed five
redundant local branch names: **212 to 207** local heads, with **13 registered
worktrees retained**. Other refs, caller/protected source and indexes were
unchanged. Remote/fleet branches remain held because their ownership was not
established by these local checks. This is repository housekeeping, not parser
retirement or a coverage improvement.

## Completed implementation and validation

### 1. Canon CODE-ref recognition and verification

Before the completed repair, the 13.59 dump under Perl 5.34 contained all 29
Canon PersonalFuncs fields but the registry only accepted the equivalent Perl
5.38 spelling. Collection dropped the conversion before oracle jobs were
created. Independently, recognized CODE references accepted an empty
verified-expression set. The repair and its validation are recorded below.

- Register both complete audited bodies. Do not match by function name or
  normalize arbitrary syntax into equivalence.
- Preserve quoted literal contents while tolerating formatting outside them.
  Changed labels, operators, constants or statements must refuse.
- Require the recognized expression's actual key in the verified ledger and
  preserve input-domain checks and explicit refusal behavior.
- Test collection itself, including all 29 uses, so disappearance before
  verification is detected. Test missing, unrelated and valid ledger entries.
- Run fresh pinned expression verification and regenerate using that dump's
  own hash-matching ledger. Separate semantic equivalence from provenance
  differences between interpreters; do not reuse a stale ledger.

Validation completed on 2026-09-10 at `43764828`: 10 focused controls passed;
the full Python tooling suite reported 312 tests run and `OK (skipped=1)`.
The class-level skip leaves five full-dump methods unrun; it must not be
subtracted from the executed-test count to infer a pass count.
Independent review passed. Fresh native Perl 5.34.1 and 5.38.2 runs each verified
607 expressions with 16,789 successful probe comparisons and 14 inapplicable
probes skipped. Both dumps retain all 29 Canon uses; removing only that named
ledger key explicitly withholds all 29 fields. Generated binary and IFD Rust
match across both interpreters and the existing committed files. Independent
table verification, formatting and strict package Clippy passed. No generated
Rust, parser implementation or version pin changed.

### 2. Isolated old/new upgrade transaction

Keep the public `bump-exiftool.sh` / `just bump-exiftool` entry points and the
existing manifest, triage, conformance checker and report formats. Use private
temporary clones for variants, without accumulating registered worktrees.

- Capture one source commit and caller entry state. Reject conflicting source
  changes without discarding them. Record a unique external run directory.
- Resolve exact old/new ExifTool trees and Perl interpreter, capability-probe
  them, refresh dumps, and give both generator tiers the same selected source.
- Ordinary BEFORE builds the actual committed starting source. A retrospective
  dry-run BEFORE regenerates **all selected tiers**. AFTER regenerates all
  selected tiers at the requested version. Never mix releases.
- Build with explicit separate target directories and resolve the actual fresh
  Cargo executable. Record source/artifact/binary identities. Ambient cache,
  interpreter and target settings must not redirect a measured build silently.
- Grade both binaries using the same target-version oracle and corpus selection.
  Keep non-vacuity floors, group-qualified comparison and existing regression
  gates. Skipped conformance means incomplete evidence and cannot promote.
- Dry runs and pre-promotion failures leave caller files and index untouched.
  Before live promotion, recheck entry identity and apply only the selected
  manifest outputs plus pin. Keep a durable recovery snapshot and stage record;
  refuse to overwrite unrelated concurrent changes during recovery.

Controls must exercise the real orchestration with small fixture repositories:
old/new tier sentinels, failure after producer/build/gate stages, hostile target
and source settings, undeclared writes, dirty caller/concurrent edits, interrupted
promotion and missing gate output. After those pass, run an actual same-pin
exercise. Record any genuine generator/oracle refusal as an unresolved limitation.

This work replaces the old temporary tier-1-only rebuild and index-based
checkout restoration after equivalent failure controls pass. It does not remove
handwritten parser implementations or change the supported version pin.

The implementation uses `upgrade_transaction.py` behind the existing shell
entry point. Independent review found and verified repairs for corpus alias
identity, excluded-file population floors, malformed comparison reports,
descendant-process cleanup, terminal journal-write failure and Git environment
redirection of the caller's index. Separate controls execute the real generation
shell scripts with leaf stubs and verify explicit source/Perl selection, fresh
tier-2 dumps and missing-source refusal. These controls establish orchestration
behavior. Combined local Python tooling passed 337 executed tests; one skipped
class left five full-dump methods unrun because no default dump was configured.
Hosted CI at `d5c47033` supplied that dump and passed all 342 tooling tests with
no skips. It also passed 5,532 nextest tests (50 skipped) and 221 doctests
(63 ignored); all active CI jobs and the clean Docs Build passed.
All 25 transaction controls also passed on Linux Python 3.12; strict package
Clippy, formatting and shell syntax passed.

The genuine same-pin run at `9100020d` completed on 2026-09-10 in **383.731
seconds** using `bump-exiftool.sh` / `upgrade_transaction.py`, native Perl 5.34.1,
two private source clones and fresh binaries. It regenerated all 25 selected
outputs; only the expression ledger's interpreter/source/dump provenance changed.
Every generated Rust file matched its committed counterpart. BEFORE and AFTER
each scored 193 real files with `conformance.py`: 9,966 MATCH, 1,563 MISSING,
31 VALUE, 13 RENAME and 680 EXTRA. The existing gates passed with zero new VALUE
regressions and zero MISSING growth. Existing differences remain compatibility
debt. Caller files, modes, branch, HEAD and raw/logical index were unchanged.
This is workflow validation, not a release-delta or live-promotion rehearsal;
promotion/recovery evidence comes from the focused controls.

Recovery restores only known original/prepared contents. Unknown concurrent
edits and partial or changed staging files stop recovery and retain evidence for
manual preservation/review. Skipped conformance is incomplete and cannot promote.
See the [command contract](https://github.com/swack-tools/oxidex/blob/2cea1e4194ce7fc1aeed4b110d77ad436ccd7837/tools/exiftool-tables/README.md#isolated-upgrade-transaction).

### 3. GeoTIFF, DICOM and lens producer integration

Implementation started on 2026-09-10 from `785ffcfd` on
`codex/wire-remaining-producers`. DICOM was integrated as `b9934841` (author
`ea6ea1a4`), GeoTIFF and the first two runner additions as `10d20b78`, the
real-shell controls as `f384b68f`, and the lens repair as `5e40396b` (author
`78daf9d6`). All three outputs now participate in the shared manifest, tier-2
runner, formatting, independent verification and CI drift checks: **28 outputs,
8 in tier 1 and 20 in tier 2**. Final integrated checks and the complete same-pin
transaction passed before landing in PR #740. The later
[release-delta rehearsal](./reference/bump-reports/13.55-to-13.59.md) passed at
`4fb705da`; the original validation populations below remain historical.

| Producer | Output | Independent facts at 13.59 |
| --- | --- | --- |
| `gen_geotiff_printconv.py` | `src/parsers/tiff/geotiff_printconv.rs` | Compiled lookups match all 64 names, 19 map associations and 2,089 conversion facts |
| `gen_dicom_dict.py` | `src/parsers/specialized/dicom_dict.rs` | Loaded Perl agrees with all 5,669 dictionary entries and 1,978 UIDs |
| `dump_lens_alternatives.pl` | `src/composite/lens_alternatives.rs` | Canon EF 239 bases/296 alternatives; RF 76 bases/no alternatives; Pentax 14 ambiguous rows/45 alternatives; Olympus has no fractional alternatives |

Each producer binds the selected source and Perl, refuses unsupported facts,
and has explicit complete-file/check behavior. GeoTIFF and DICOM output remains
byte-identical. The lens producer now emits the complete module and preserves
raw IDs in its generated Canon tables. Eleven GeoTIFF, ten DICOM and twenty-three
lens CLI controls pass. Nineteen manifest controls and four real-shell controls
pass; the latter deliberately fail each new producer/verifier and reject
undeclared writes using the production manifest and scripts with synthetic
leaf tools. They establish orchestration, separately from the native fact checks.

**Runtime defect found and repaired:** Canon IDs 129 and 136 share the display
name `Canon EF 300mm f/2.8L USM`, but only 136 has the Tamron 15–30mm alternative.
The old label-keyed table conflated them. The repair preserves display/raw forms
atomically on the exact occurrence, selects its identity with that occurrence,
and retains it through TIFF, DNG and CR3 bridges. RF uses its own PrintConv table
as well as its own label/ID; RF-zero falls back according to the pinned oracle.
A losing duplicate cannot overwrite the winning occurrence's raw ID.

The first RF regression failed because its fixture used EF IDs as RF IDs, which
also exposed a real draft defect selecting EF alternatives for RF. That evidence
is retained and explicitly superseded by native RF Composite checks. Final
independent core comparisons pass **239/239 Canon EF** and **154/154 RF** cases
(all 76 RF rows plus unknown 136, each paired with EF129/136). Compound Pentax
and Olympus values retain their behavior. The public legacy `IfdEntries` helper
has no raw-value channel; no production caller beyond its export was found.
With missing identity, the core preserves unambiguous labels and omits only
EF129/136 in the measured no-optional-input domain (237/239 still match).

Author validation: 91 lens-focused Rust tests pass; the full library passes
4,600 tests with one ignored. Strict package Clippy and formatting pass. Eight
real TIFF test carriers match pinned ExifTool's LensType, RFLensType and LensID;
LensID correctness improves from **5/8 to 8/8**, fixing three EF129 cases.
Direct `conformance.py` A/B selected **194 files** with no extension exclusions:
both binaries report 9,969 MATCH, 1,573 MISSING, 31 VALUE, 13 RENAME and 684 EXTRA.
No per-file semantic result changed after normalizing only list ordering. The
historical baseline binary's archive, executable hash and 720 Rust/Cargo/config
inputs were verified against the starting base; the candidate was freshly built.
This A/B includes `JSON.json`, which the transaction's explicit 193-file selection
excludes. Keep those two populations and instruments distinct.

The earlier clean `10d20b78` tier-2 run reproduced all 27 then-wired outputs with
zero net source changes. The final 28-output same-pin transaction passed at `1428b6c7`; its evidence
is recorded below. A file inventory or table/core oracle does not establish
complete extraction coverage. No parser retirement or version-pin change is part
of this continuation. Implementation commits and source-equivalence evidence are
retained separately from the squash landing.

### Cold-build rehearsal failure and repair

The first complete 28-output same-pin attempt at `65d6a791` failed after
**429.229 seconds**. `verify_exprs.py` gave compilation plus probe execution a
single 180-second budget; a fresh Cargo build exceeded it. A subsequent macOS
process-group cleanup error (`EPERM` while descendants were exiting) obscured
the original timeout. The caller's files and index remained unchanged. The
failed journal and logs are retained separately from the next attempt.

The repair separates a **1,800-second Cargo build budget** from each oracle's
**180-second execution budget**, executes the unique harness artifact reported
by Cargo, and requires exactly one result for every probe. Existing temporary
harnesses refuse without being overwritten. Shared process-group cleanup waits
for descendants, preserves the original failure in the transaction journal,
and refuses persistent cleanup errors. Full-script controls exercise successful
slow compilation, build/execution failures, bad Cargo output, incomplete probe
output and ownership. The repaired real rehearsal subsequently passed as recorded below. Neither
the failed nor successful same-pin run is an upgrade-cost estimate.

Repair validation: the integrated tooling suite passes **415 executed methods**
in 203.048 seconds, with five WholeDump methods unrun through one missing-fixture
class skip. An additional native 5.34 dump run passes four of those methods and
skips ledger validation because the committed ledger records native 5.38
provenance. The completed transaction generated a matching dump/ledger pair; all five
WholeDump methods then passed in the regenerated AFTER variant with no skips.
The suite includes 16 full-script execution controls, 13 owned-process controls
and command-journal failure preservation. Strict package Clippy, formatting and
independent source review pass.


### Final 28-output acceptance

The real `bump-exiftool.sh 13.59 --dry-run` / `upgrade_transaction.py` run at
`1428b6c7935afeed52b5ebd008bb7befc4b84902` passed in **668.522 seconds** using
selected ExifTool 13.59 and native `/usr/bin/perl` 5.34.1. Both variants were
freshly built. The command selected 193 eligible files from the pinned source's
`t/images`, with explicit floors of 193 files and 1,000 oracle tags. Both sides
reported **9,966 MATCH, 1,563 MISSING, 31 VALUE, 13 RENAME and 680 EXTRA**;
comparison gates passed and the caller's source and index were unchanged.

All 28 outputs were accounted for. Only `expr_oracle_ledger.json` changed,
recording the fresh interpreter/source/build provenance; all generated Rust
was unchanged. The expression oracle passed 607/607 expressions and 16,789
probe comparisons, with 14 inapplicable Perl-error probes separately skipped.
Complete GeoTIFF, DICOM and lens fact checks passed inside the real runner.
The five WholeDump checks passed against that regenerated matching ledger.

Evidence is retained in the cleanup audit's
`handoff-continuation/producer-wiring-xmgiqiui/rehearsal-retry/`: `SUMMARY.json`,
`driver-result.json`, `whole-dump.log`, and the `bump-sum5o8h1` transaction
journal, source/artifact identities, reports and preserved measured binaries.
The earlier failed attempt remains separately under `rehearsal/`. The subsequent
[13.55-to-13.59 experiment](./reference/bump-reports/13.55-to-13.59.md) passed at
`4fb705da`, with its own release, artifact and corpus evidence. PRs #737–#740
have landed; the separate retained runtime work below remains open.

The implementation inputs from the acceptance commit `1428b6c7` are unchanged:
only these four status/command documents differ at reviewed head `028f2bb5`.
The final squash tree was verified equal to that reviewed head. This preserves
the original acceptance evidence; it does not claim a new corpus run at the
squash commit.

## Other retained work

- Repair the opt-in MOBI `UncompressedTextLength` failure; pinned ExifTool emits
  `172 kB` for the retained sample. Keep ignored-test state explicit.
- Resolve timezone assumptions and inherited strict test-target lint separately
  from the already-passing ordinary CI gates.
- Review the remaining held worktrees after their branches are landed or their
  payloads are deliberately preserved. The 95 approved removals are complete;
  original branch refs and 12 held paths were retained.

## Handoff discipline

At each milestone record the commit, PR and landing state, named validation instrument,
passed/skipped/failed counts, evidence directory, unresolved work and exact next
command in the root `HANDOFF.md`. Mark an item implemented only after its checks
finish, and integrated only after its changes actually land. Commit timestamps
are not elapsed engineering time. The completed rehearsal records one automated
run on current runtime source; it does not establish a recurring upgrade-cost
estimate or the historical work needed to reach this implementation.
