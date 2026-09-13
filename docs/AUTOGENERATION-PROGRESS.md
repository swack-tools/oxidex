# Autogeneration progress

This is the working scoreboard for [the plan](AUTOGENERATION-PLAN.md).

## Current checkpoint — September 13

Latest combined Canon definitions: **AFInfo 14/14 and AFInfo2 16/16**, both
independently verified after formatting. Across all eight serial tables,
emitted alternatives rise **106 -> 122** and omissions fall **26 -> 10**.
This checkpoint is implemented, definition-validated and replayed through the
actual generated tables: all three native/Rust tests pass, as does full
regeneration. Remaining local and hosted merge checks are pending; it is not
merged or active in a Canon production carrier.
The [Canon plan](reference/serial-afinfo-plan.md) records the failed pipeline
attempt, its formatting repair and the remaining acceptance work.

The following entries retain the completed milestones and their exact evidence.

PR #754 merged as `1138a880` at 10:11 UTC, following #753 (`1a47cfa3`) and
integration `72eae8a5`. The shared word processor now generates nine
native tables containing 132 rows, including the empty fallback table.
The independent definition verifier reports **132 native / 132 generated,
zero row discrepancies and zero source-binding discrepancies**. Scope comes
from the native processor inventory, so deleting every generated table fails.
This is definition coverage; production routing remains inactive.

Four of the parent's five unsupported child processors now have generated
descriptors. One dynamic-length processor and four omitted parent rows remain.
No Canon manual reader has been retired and no project-wide percentage has
been remeasured. The next measurable results are the remaining processor,
real-carrier verification and removal of duplicate readers.

The combined source at `f613820d` passes 25 Rust reader tests and the explicit
native/Rust comparison passes all seven cases with the real generated tables.
This includes the generated parent edge, verbose-directory reporting, rejected
headers, short reads and both byte orders. CI explicitly selects the native
test and rejects a zero-test run. Clippy and formatting pass.

Official two-tier regeneration passes at `b9c7f206` in 196 seconds and
reproduces every declared artifact with zero net changes. All 607 translated
expressions pass 16,789 native comparisons; 14 probe inputs are inapplicable.
The earlier three failed regeneration attempts remain recorded. After correcting
one stale assertion and one test setup error, the canonical full Python suite
passes **698 tests, zero failures, zero skips**, in 386.649 seconds. CI receives
the inputs needed to run native tests it previously skipped. All five hosted
jobs passed on final head `443c8458` before the squash merge; benchmarks were
intentionally skipped. The earlier Clippy process was concurrent; there is no
evidence the Python suite launched it.

The [checkpoint record](reference/word-directory-checkpoint.md) separates
these checks and publication states. Completed checkpoints are committed and
pushed without waiting for the whole merge gate.

The [serial-processor checkpoint](reference/serial-processor-checkpoint.md)
merged in PR #755 as `93d19e24` at 11:18 UTC. All five hosted checks passed
on final head `6c9e38ed`, including **706 native Python tests, zero failures
and zero skips**, in 566.450 seconds. The initial threaded-Perl failure was
reproduced locally and fixed; the eight focused probe tests pass on both
threaded and non-threaded Perl 5.38.2.

The source batch merged in PR #756 as `eb700430` at 11:46 UTC. All five
hosted checks passed at `298917dd`, including 735 canonical Python tests,
zero failures and zero skips, in 653.244 seconds. Its first published complete
inventory checkpoint was `37c32d58`. It accounts for all eight source-selected
serial tables: **130 entries, 132 alternatives, 115 clear at the source gate
and 17 with explicit refusals**. One table also retains a priority-policy
blocker. A complete processor grammar rejects changed executable behavior;
supported table-data changes still compile. These are source representability
counts, not counts of executable Rust rows.

The same batch adds actual Real AudioV3/V4 native replay and copied-source
provenance checks at `66c430c6`, plus native CIFF opaque-data probes. Their
observations cover reads, raw callbacks, image-span arguments and relevant
ordering. Final output groups/conversions and actual image digest computation
remain outside these probes' proof. Warnings are recorded separately from the
chronological callback trace. The combined checkpoint at `66c430c6` passes **37 focused tests, zero
failures and zero skips**, on both threaded and non-threaded Perl 5.38.2
against pinned ExifTool 13.59. The full hosted checks then passed for #756.

The shared serial reader and Rust emitter merged in PR #757 as `58849bc7`
at 12:47 UTC. All five hosted checks passed at `72fbebb1`, including 757
canonical Python tests with zero failures/skips in 641.913 seconds, 5,756
nextest tests passing (59 intentionally skipped), the complete Cargo test/doc
invocation, and the explicitly invoked native serial replay.
The [runtime checkpoint](reference/serial-runtime-checkpoint.md) records the
proof and its limits. All eight tables remain accounted for: **106 emitted
alternatives and 26 explicit omissions**, with zero independent verifier
mismatches. This is stricter than the source-only 115/17 split because the
emitter also refuses unproved runtime formats and missing-member conditions.
Four tables clear the definition gate; native/Rust replay currently covers
Real AudioV3 and AudioV4 only.

All 64 focused Python tests pass with zero failures and zero skips in
65.023 seconds. The full `cargo clippy --all-features -- -D warnings` check
and formatting check pass. All 13 shared-reader unit tests pass. The explicit
native/Rust test passes
its ten cases on both threaded and non-threaded Perl 5.38.2, using the actual
generated tables. Official regeneration passes across 32 declared artifacts;
all generated Rust is unchanged. The expression ledger initially records the
local absolute invocation paths. Re-running its producer with the portable
invocation reproduces the committed ledger exactly. All 607 expressions agree
on 16,789 applicable comparisons; 14 inputs are inapplicable.

That merged checkpoint enabled no production caller. **One unsupported Canon
child processor, four omitted parent rows and zero Canon manual readers
retired** remain the Canon status.

The [Real AudioV4 migration](reference/real-audio-v4-retirement.md) merged in
PR #758 as `19cb7650` at 13:15 UTC. **One manual serial reader and its 31-slot
field sequence are retired.** Runtime `4f01db97` plus regression assertions
at `d3f51325` passed all five hosted checks at final head `4bcda9d9`: 757
canonical Python tests, zero failures/skips, in 643.545 seconds; 5,761 nextest
tests pass with 59 intentional skips; full Cargo test/doc and native replay
also pass. Its generated reader replaces the manual sequence. All six carrier tests, full
Clippy and formatting pass. A fresh 19-fixture native/control/candidate comparison
has 15 scored cases matching native output and four explicitly retained scope
or diagnostic cases. Ten fixture cases correct existing legacy behavior: UTF-8 repair,
NUL truncation, or later fields invented after an incomplete string.

A supported source mutation also reaches actual output: changing AudioV4's
`Title` name to `UpgradeTitle` in a copied pinned source, regenerating and
compiling with the unchanged carrier, changes exactly that output key. No
tag-specific Python/Rust rule is edited. This proves that supported change;
it does not substitute for a real release upgrade or broader semantics.

The independently accepted full pair includes all 4,238 files and 518,919
native tags. Correct rows rise **468,086 -> 468,087**, VALUE differences fall
421 -> 420, and MISSING 12,240 / RENAME 22 / EXTRA 1,560 are unchanged. The
only raw or scored change is Real.ra Copyright matching native question-mark
repair; the other 4,237 files are unchanged. There are no parse/crash failures
or input-hash changes. Elapsed time is 273.168 seconds.

The supervisor's exit 1 is preserved: native ExifTool reports the pre-existing
zero-byte FujiFilmISPro.jpg as empty. Its parsed diagnostic remains included;
both OxiDex builds exit 0 with identical meaningful output. Independent review
accepts that known diagnostic separately from new failures.

The current occurrence API still lacks native group-0/group-2 fidelity;
native warning output and AudioV3/V5 activation remain unfinished. The retired
31-slot sequence is not 31 newly emitted tags. The real sample produces eight
correct direct Real-RA4 values through the generated reader. No project-wide
autogenerated percentage follows from that bounded count.

Next is the [Canon autofocus definition plan](reference/serial-afinfo-plan.md).
AFInfo and AFInfo2 currently account for 30 native alternatives: 15 emitted and
15 explicitly omitted, with both tables blocked. The next target is complete
source-derived representation and native execution proof for both tables.
Generic parent routing and validators follow separately; Canon's one child
processor block, four parent omissions and zero retired manual readers remain
unchanged until their respective checks pass.

### Latest recorded percentage has an unresolved validation defect

The preserved `attr-72ea.json` report at `72eae8a5` records 468,086 matched
control rows and 288,635 with the union of generated routes disabled:
179,451 lost rows, or **38.337%**. Its accounting residual is zero, but the
probe-with-silencing-unset check differs on `PentaxOptioL20.jpg`. The local
evidence lacks the raw paired outputs, binary hashes and process results needed
to bound that discrepancy. This is an arithmetically consistent recorded
aggregate, **not yet validated generated-route attribution**, and not a
fully-autogenerated percentage. Preserve and replay the differing file on
both binaries before promoting that measurement. The 97.3% conformance figure
continues to measure output agreement, not manual or generated ownership.

## Earlier milestones and evidence

The Sony focus-table pilot merged in PR #746 at `04eaf6e1`; final CI is green
at `a1626cb6`. It removed 17 duplicate declarations and brought the ten
raw-ID fixes into the shared route. Its earlier comparisons, failed gate
attempts and retirement checks remain below as history.

The shared binary string capability merged in PR #747 at `8f0fdaf4`. Its first
full candidate lost 55 previously correct rows through the legacy CameraInfo
adapter; the merged repair restores them. The repaired candidate is
artifact-complete, while both full-pair supervisor exit statuses remain
unavailable and explicitly retained as that validation limitation.

The keyed-directory schema/compiler merged in PR #748 at `ebbe1ece`, after all
final hosted checks passed at `a422e8de`. The separate reader merged in PR #752
at `634e5616` on September 13, 06:30 UTC, after all required checks passed on
`ad6d8562` against `35487962`. Focused tests cover full directory counts, deep nesting, native
group projection, legacy continuation after child failure and directory-state
restoration when a checked walk stops. Independent review found no remaining
blocking defect in those two corrections. No
production Canon route is active and neither manual Make/Model reader has
been removed.

The recorded source inventory merged in PR #749 at `18a8ef17`. Its immutable
selector replay accounts for 1,512 table identities, 34,283 alternatives,
32,043 named alternatives and 119 tables with no named alternatives. It
classifies 651 binary shapes, 496 IFD candidates, one keyed shape and 364 other
shapes. These categories describe source structure, not manual or generated
behavior. The [replay instructions](reference/source-processor-inventory-13.59.md)
record the source and selector identities. Runtime and maintenance
classification still need to be joined to this population.

Shared keyed reporting policy merged in PR #750 at `5fb979d8` after all
required hosted checks passed on `50d01be4`. The source-to-artifact join merged in PR #751 at `35487962`, after all
required checks passed on `dd8d89b3`; its 1,512-identity report preserves
the separate historical artifact snapshot `18a8ef17`. It still does not
classify runtime producers or manually maintained rules.

The work-branch compiler checkpoint captures native validation helper source
and turns seven CanonRaw call sites into shared size-comparison operands.
Independent numeric-reader source and byte-order checks now clear all seven
validation-proof blockers in local replay. **Five child edges still have
unsupported processing rules; the parent route remains inactive.** Local
source/oracle replay accounts for 61 rows (57 represented, four explicit
omissions) with zero source-fact discrepancies. It checks 262,144 native numeric
reads and ten boundary cases. Changing only native `Get16u` from a 16-bit to a
32-bit read blocks all seven newly generated checks and rejects all seven
stale artifact checks.

The complete Python tool suite passes 638 tests in 194 seconds. Focused Python
checks, 18 Rust reader tests, three Rust primitive tests and the repository's
exact CI lint command also pass. The
[checkpoint](reference/directory-validation-checkpoint.md) records the current
Python count and scope. Native probing also corrected a boundary bug: a short
read at the buffer end coerces to zero, whereas starting beyond the buffer
rejects the child. Earlier failed attempts remain recorded.

Canonical regeneration now passes locally using isolated Perl 5.38.2 and
pinned ExifTool 13.59. The official two-tier command completed in 158 seconds;
all prior table artifacts remain byte-identical. The new keyed definitions
are the 31st declared output, and the refreshed expression ledger authenticates
the new native-source dump. All 607 expressions passed, with 16,789 comparisons,
zero disagreements and 14 inapplicable probe errors. The independent keyed
inventory accounts for 61 source rows, including four explicit omissions.

The full native dump is byte-identical for absolute and relative library
locations after diagnostic paths were made portable. Source-file hashes and
error text/line numbers remain intact. The local interpreter also reproduced
the historical canonical dump exactly before the capture change.

Published source checkpoints are `51e4d864` and `46f585d1`; the reviewed next
word-directory compiler is published separately at `6a993607`. Regeneration
and routine committed-artifact verification are part of this follow-up. These
are work-branch checkpoints, not merged production activation. Five child
edges and other parent omissions still block the Canon route; no corpus gain
or manual Canon retirement is claimed. The i7 command channel remains
unverified; this regeneration did not use or claim its shared lock.

## What the pilot must prove

The shared reader must produce the focus tags using ExifTool's own table
rules. Changing a supported name, enum, offset or adding a row in the native
source must flow through regeneration without another handwritten tag rule.
The duplicated Sony handling must then be removed.

| Measure | Starting point | Current evidence | Done when |
| --- | --- | --- | --- |
| Native table entries | 17; the shared generator already knew their layout, while a custom Sony reader supplied the output | Independent native inventory finds 17 native and 17 generated entries, with zero discrepancies for this table. | All are independently accounted for and their runtime behavior is verified. |
| Stored-value and selection rules | One stored value; 16 dependent conditions; one parent selection condition | Local and Linux Rust checks pass after correcting stale test inventories. The Python tool suite passes with the recorded native dump explicitly supplied: 555 tests, zero skips. | Native comparisons, combined Rust checks and final corpus checks pass. These checks are complete. |
| Real-file coverage | 41 files; 142 native rows | Exact native focus values match. Paired full-output comparison results are identical per file. Four files with the same tag name from other tables were excluded. | Candidate and control are compared per file, with no unexplained changes. This scoped check passes. |
| Boundary coverage | 14 complete TIFF carriers | Library and CLI reproduce the native expectations for both byte orders, zero/one/fifteen points, short records and a rejected signature. | The candidate reproduces those results through both the library and CLI. This check passes. |
| Automatic upstream changes | No complete pilot proof at the start | Actual native dump, regeneration and independent artifact checking pass for rename, enum, offset and new-row changes. Each stale artifact is rejected. | All four change types pass the artifact chain. Runtime/carrier checks use the unchanged native pilot; executing each mutated artifact and broader upgrade behavior remain separate work. |
| Duplicate custom handling | One Sony root rule and 17 custom table declarations | PR #746 removes the root rule, all 17 declarations and their unused saved-value slot. A mechanical removal tool reproduces the exact artifact and records its source identity. Final local/Linux code checks and the corpus pair pass. | Complete for this pilot; wider source inventory remains open. |
| Merged pilot progress | Zero | PR #746 merged at `04eaf6e1`; final CI is green at `a1626cb6`. | Complete for this pilot. |

The source is pinned ExifTool 13.59. Native measurements use its explicit Perl
reader; the 41-file producer identification also checks the native verbose
trace. The comparison baseline is `e1191be6`. These counts describe this pilot,
not the project's overall autogenerated percentage.

The paired instrument is `conformance.py` against pinned ExifTool 13.59:
55 files, 9,629 matched rows on both sides, and identical per-file discrepancy
records. A separate exact key/value comparison checks all 228 focus rows
(142 real-file rows and 86 boundary rows). Candidate runtime source hashes
were recorded and remained unchanged during the comparison.

The complete paired `conformance.py` run now covers all 4,238 files on the same
pinned oracle and unchanged corpus. At `1a63822e` versus `e1191be6`, both sides
have 468,012 matched rows, 12,258 missing, 477 wrong-value, 1,562 extra and 22
renames. **Zero per-file discrepancy records changed.** The control took 394
seconds and the candidate took 455 seconds; these are validation timings,
not a performance benchmark.

The complete workspace test command first failed on two stale test inventories:
Fuji's `GEImageSize` and Olympus's `SensorArea`/`BlackLevel` now have generated
conditions, so they no longer belong in the list of conditions the generator
cannot represent. The correction preserves the inventory assertions and adds
positive/negative condition tests, including a saved count used by a child
table. Its combined lint/workspace rerun passes: 5,691 unit/integration tests
and 222 documentation tests, zero failures; 54 and 63 tests respectively remain
ignored. `cargo test --workspace --all-features` took 262 seconds, and the exact
CI lint command took six seconds. The recorded staged-source hash stayed
unchanged during these checks. This correction does not
claim new parser coverage: current callers still do not pass `Make` and
`TIFF_TYPE` where those three tags need them.

The follow-up retirement removes the 17 duplicate `Tag202a` entries and the
unused `Locations` slot. The other 36 custom Sony tables remain. This is not
a recovery of their missing generator. The ownership manifest contains only
source/table identities, and the removal tool mechanically adjusts table
indices after deleting the selected table. It refuses unsupported source
shapes and checks the recognized consumer forms documented in the tool README;
it is not a general Rust alias or data-flow analysis.

Independent review accepted the removal. Replaying it from the original
`1a63822e` artifact reproduces both the committed artifact and identity record
exactly. In-place replay leaves source and ledger bytes and modification times
unchanged. The retirement's own lint, complete workspace and build checks pass;
the workspace took 244 seconds. All 19 retirement checks pass. The initial
Python tool suite passed 549 tests with one skipped class. Earlier corpus
results above describe the pilot before this removal; the final pair is
recorded separately below.

The final Linux gate at `5bd15e00` passed staleness and formatting, then stopped
at the generated C-header check before tests and corpus comparison. Removing
the custom table also removes its exposed `TAG202A` macro and renumbers the
22 following Sony index macros. The header was regenerated with pinned
`cbindgen 0.29.2`; `just cbindgen-check` now passes locally. Review confirms
that the macro values match the Rust indices and no C function signature or
layout changed. External callers using these exposed indices must use the
updated definitions. The original failed gate remains recorded.

Hosted table verification then found a stale `BWMode` expected declaration:
the test literal omitted the schema's new `condition: None` field. The initial
local command skipped the six `WholeDump` checks because it had no table dump.
After correcting that expectation and explicitly supplying the recorded native
dump matching the expression ledger, all 555 Python tests pass with zero skips
in 195 seconds. This correction changes no runtime code or generated table.

The corrected Linux gate at `92dac917` passes staleness, formatting, header,
lint, release build, workspace tests, doctests, native table verification and
all JPEG matrix stages. Its private runner then stopped while misreading the
matrix's generated-document changes. The failure and diff were preserved;
only those documents were restored and the exact source was rechecked. The
unfinished paired census resumed under the shared lock, saving both JSON
reports. Its control completed in 501 seconds and candidate in 504 seconds.
The shared gate script was not changed. Hosted CI and docs also pass at
`c3d0acd7`; that follow-up changes only the Python expectation and documents.

The final `conformance.py` pair compares `92dac917` against integration
`7e928390`, pinned ExifTool 13.59 and all 4,238 files. Matches rise from
468,002 to 468,012; missing rows fall from 12,268 to 12,258. Wrong-value rows
remain 477, extra rows 1,562 and renames 22. The only per-file changes are ten
`ImageNumber` fixes from the raw-ID repair. No missing, value or extra
regressions were added. [The committed gate record](reference/sony-shared-pilot-gate.json)
contains the identities and individual changes. This is correctness evidence,
not a measurement of the overall generated share.

PR review also found that the strict native inventory was only run manually.
Both `just verify-tables` and hosted table verification now require it for
`Sony:Tag202a`, so an unaccounted native row cannot pass the normal gate merely
because the older generated rows still match. Other tables' known completeness
debts remain visible; this follow-up does not claim they pass.

## Next common capability

Shared string support is merged in PR #747 at `8f0fdaf4`. It distinguishes
a table's one-byte default from a field that reads the remaining record,
preserves raw bytes in shared-engine saved state, and repairs invalid text at
shared output projection. CameraInfo is a legacy text-domain adapter that
projects those bytes with FixUTF8 before its own later processing; that adapter
is compatible but not proof of byte-exact state semantics and remains migration
debt. Independent native cases cover CanonRaw MakeModel and EXE DebugRSDS. Native
review also found and corrected AIFF enum fallback and Kodak whitespace-trim
behavior. The local workspace run passes 5,916 tests including doctests, with
117 ignored; lint and the C-header check pass. The final formatting check and
all 563 Python tool tests pass. Independent native inventory checks for
CanonRaw MakeModel, EXE DebugRSDS and Kodak Type7 pass with zero mismatches.
The first complete pair exposed 55 lost correct rows; the `019dd530`
CameraInfo repair has a 55-file bounded proof and an artifact-complete full
candidate. Against the prior control it changes matched rows from 468,012 to
468,013, leaves 12,258 MISSING, 1,562 EXTRA and 22 RENAME rows unchanged, and
removes one Panasonic value mismatch. Both full-pair supervisor exit statuses
are unavailable, so this is explicitly artifact-only evidence.

A later boundary probe found that global inline Unicode regex flags bypassed
the compiler's refusal for scoped flags. Native Perl and the Rust byte matcher
gave different answers for the same raw bytes. The compiler now refuses those
forms, with regression checks. No condition in the recorded native dump uses
them, and regeneration leaves the binary, IFD and value artifacts unchanged.
The earlier scoped review did not cover this case; its correction and evidence
are recorded separately.

No new parser caller or table has been enabled by this capability, and neither
manual Canon Make/Model decoder has been removed. The source inventory finds
eight explicit remainder-string fields in eight declared tables. Five real
CIFF carriers include a big-endian JPEG that the current manual reader misses.
These findings guide the next migration; they are not an automation percentage.
The migration must also generate parent routing and preserve file-order state,
not stop at replacing the two child string decoders. Seven parent validation
edges and the other manual Canon child producers remain separate work.

## Keyed-directory schema validation record

This schema-only checkpoint, subsequently merged in PR #748, uses recorded
ExifTool 13.59 Perl 5.38
source-dump SHA-256 `193cf4e91326f53c7bdfd674cb10da0937c8bcdb4c0285ad4f507fd9f96a8fb8`.
The full replay at `74418ceb` produced binary source SHA-256
`a8972dde5dc012ea7db27849fc02c52f68f9820dc8191c020fa40bb5adf26d97` and keyed
source SHA-256 `830e7fcd88641ecce7e466b8c3e72b781735c46479c86bd6e8834dad2bdd05dd`;
rustfmt-normalized binary source committed in the branch has SHA-256
`076f64ff7d6318d21268d61347fd3806adc3928581877fbe3443e61395cccc82`.

The table-tool suite passes 580 tests with that ledger-matching dump. The
keyed-specific tests verify native processor selection, counts, formats,
conditions, edges, atomic variants, enums, authenticated omissions and
source-driven mutations. A keyed-only expression now appears in the shared
`ExprId` enum whether or not the optional keyed artifact is written. This is
compiler and inventory evidence only: no keyed reader or Canon route is active.

## What is still open

- Complete the wider inventory of manual, generated, unsupported and
  unclassified source rules, then refresh generated-route attribution on one
  recorded source revision.
- Integrate and validate the published keyed reader with shared reporting
  policy. Seven source validation edges, an unsupported child processor,
  undefined-format handling and three opaque-value rows still block parent
  activation. Preserve these blockers; a clean MakeModel child edge does not
  make the parent complete.
- Keep the next manual retirement unchanged: generate and execute Canon parent
  routing in both standalone CRW and embedded JPEG CIFF, then retire the two
  Make/Model child decoders only after native and runtime evidence agree.
  The separate Sony-specific recovery draft still has unresolved findings and
  is not a dependency of the merged mechanical retirement.

At the Sony-pilot checkpoint, the broader binary-artifact check accounted for
8,228 native rows: 6,993 generated and 1,235 declared omissions. The current
shared binary artifact has 7,002 fields; neither figure is a whole-project
automation denominator. Its stale records and unsupported
omission explanations were corrected. It still fails strict completeness on
2,854 enum entries across 49 maps whose native fallback behavior is unsupported.
These are source-inventory findings, not a count of missing runtime tags or an
overall automation percentage. All-refused tables and other processors remain
outside this check, so it does not yet satisfy the whole-project inventory goal.

A boundary audit of the recorded native dump identifies 29 explicitly declared
binary tables outside the emitted-table inventory. Replaying the actual
generator classifies 50 row omissions and two refused table formats containing
17 named rows; 18 of the 29 tables have no named rows. This is a discovery and
producer-refusal record, not an independent whole-source completeness verdict.
The remaining causes include native Unknown flags, unsupported string layouts
and firmware-version comparisons. These give the next shared-capability work
concrete source identities without another vendor-specific translator.

Review caught an overbroad stop rule that would lose an ID3 Genre value after
an unsupported conversion. The candidate now proves when a conversion affects
only its own value, allowing later independent fields to continue. The proof
accepts source-provided literal prefixes; it has no ID3 tag-name exception.
Focused and shared-engine tests pass after the correction, and the complete
corpus comparison confirms no changed discrepancy records.

The older Sony raw-ID correctness repair is separate. Its local full-corpus
comparison gained ten correct rows with no other changed residuals. Its first
i7 gate failed while compiling a supporting command. A fresh focused rebuild
passes. The workspace check exposed a build-setting collision already
documented in the repository; the gate omitted the prescribed test setting.
The corrected i7 workspace invocation passed: 5,564 tests, 68 skipped, 871
seconds including compilation. The corrected doctest, table verification and
JPEG matrix stages also pass. The paired Linux corpus run is complete: all
4,238 files were processed; total matches rose from 468,002 to 468,012 and
missing rows fell from 12,268 to 12,258, with other aggregate columns unchanged.
This Linux pair did not save per-file JSON, so it cannot prove that regressions
were absent. Its missing evidence is recorded explicitly. The final combined
pilot gate now saves both control and candidate JSON and compares the signed
per-file discrepancies; repeating the older raw-ID branch pair is unnecessary.
The original failed gate remains recorded, and the shared gate script has not
been changed.

Update this scoreboard after each validation or landing milestone. Do not
replace an unfinished check with a count of generated lines or active agents.
