# ExifTool upgrade execution plan

Started 2026-09-10 from `0683cb11447adef3945f19da5d6917c88aea2f7d`,
the tested artifact-manifest branch. ExifTool remains pinned to **13.59**.
This file records the current implementation sequence; the
[automation backlog](./AUTOMATION-AND-TESTER-PLAN.md) owns broader acceptance
criteria and [status page](./TAG_MACHINERY_STATUS.md) owns historical evidence.

## Current work

| Order | Work | State | Completion evidence |
| --- | --- | --- | --- |
| 0 | Review and integrate PRs [#737](https://github.com/swack-tools/oxidex/pull/737), [#738](https://github.com/swack-tools/oxidex/pull/738) and [#739](https://github.com/swack-tools/oxidex/pull/739) | Implemented, draft and unmerged | Maintainer integration in stack order; reuse the classifier, manifest and verified transaction |
| 1 | Canon CODE-ref recognition and verification | Implemented and validated; unmerged | Both native Perl oracles pass; missing verification and wrong input domain explicitly refuse |
| 2 | Complete isolated old/new upgrade transaction | Implemented and validated; unmerged | Fixture controls pass on macOS/Linux; genuine 193-file same-pin run passes with unchanged caller source/index |
| 3 | Connect three existing omitted generators | Queued | DICOM, GeoTIFF PrintConv and lens-alternative outputs regenerate from the selected source with independent checks |
| 4 | Rehearse 13.55 to 13.59 | Depends on 1–3 and sound provenance | Versioned artifacts, triage, identical-oracle corpus A/B, actual manual interventions and timings |
| 5 | Finish existing runtime migrations | Separate owners; awaiting validated integration | IFD1, RawConv and embedded-IFD routing/activation established against the pinned oracle |
| 6 | Complete producer/coverage accounting and broader walk checks | Queued | Generated, hand-maintained, withheld and unexercised behavior distinguished; deliberate bad offsets/conversions fail |

The current bounded implementation covers items 1 and 2. A passing inventory
check or same-pin generation run does not complete item 4 or certify runtime
coverage. Four Sony/Nikon outputs still lack committed producers. Catalog
synchronization has separate carry-forward semantics and is outside this table
transaction until that policy is resolved.

## 1. Repair the observed CODE-ref gaps

The 13.59 dump under Perl 5.34 contains all 29 Canon PersonalFuncs fields but
the current registry only accepts the equivalent Perl 5.38 spelling. Collection
drops the conversion before oracle jobs are created. Independently, the
recognized CODE-ref emission path accepts an empty verified-expression set.

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

## 2. Make the existing bump command a sound transaction

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
See the [command contract](https://github.com/swack-tools/oxidex/blob/codex/upgrade-next-steps/tools/exiftool-tables/README.md#isolated-upgrade-transaction).

## 3. Wire the existing omitted producers

Read-only inspection identified the smallest next order; none is wired yet:

1. `gen_geotiff_printconv.py` writes `src/parsers/tiff/geotiff_printconv.rs`.
   Preserve its loaded-version and `--check` behavior, add explicit Perl/output
   selection, then independently compare its maps and names.
2. `gen_dicom_dict.py` writes `src/parsers/specialized/dicom_dict.rs`.
   Replace the optional cache-stamp check with selected-source identity and
   compare complete dictionary/UID facts with the loaded Perl tables.
3. `dump_lens_alternatives.pl` supplies `src/composite/lens_alternatives.rs`.
   Add a deterministic complete-file contract and pin enforcement; its existing
   body-only output must not overwrite the module header. Add refusal controls
   for unmodeled values and label collisions before wiring it.

Add each output through the existing manifest and tier-2 runner. Wiring all three
would expand the manifest from 25 to 28 outputs, not establish new runtime
coverage. Preserve mixed/header contents, verify deterministic generation and
selected-source plumbing, run the relevant parser tests and independent fact
oracles, and repeat the full same-pin transaction before the release rehearsal.

## Other retained work

- Repair the opt-in MOBI `UncompressedTextLength` failure; pinned ExifTool emits
  `172 kB` for the retained sample. Keep ignored-test state explicit.
- Resolve timezone assumptions and inherited strict test-target lint separately
  from the already-passing ordinary CI gates.
- Review the remaining held worktrees after their branches are landed or their
  payloads are deliberately preserved. The 95 approved removals are complete;
  original branch refs and 12 held paths were retained.

## Handoff discipline

At each milestone record the commit, draft PR state, named validation instrument,
passed/skipped/failed counts, evidence directory, unresolved work and exact next
command in the root `HANDOFF.md`. Mark an item implemented only after its checks
finish, and integrated only after its changes actually land. Commit timestamps
are not elapsed engineering time. Do not publish a recurring upgrade-cost estimate
until the real rehearsal supplies measured evidence.
