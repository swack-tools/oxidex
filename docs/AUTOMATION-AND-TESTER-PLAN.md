# Remaining automation work

**Original audit: 2026-09-10 at `c7f5dd81`, ExifTool 13.59.** The current queue
reflects the verified PR #737–#740 landings through
[`2cea1e41`](https://github.com/swack-tools/oxidex/commit/2cea1e4194ce7fc1aeed4b110d77ad436ccd7837).
This is the maintained backlog accompanying [Tag machinery status](./TAG_MACHINERY_STATUS.md).
It replaces the local September 9 proposal's ordering and speculative schedule.
The status page owns completion history; this page owns remaining work and its
acceptance criteria. No parser or generator change is implemented by this plan.
See the [execution plan](./UPGRADE-NEXT-STEPS.md) for the current bounded work,
dependencies and milestone updates.

The classifier, shared manifest, Canon CODE-ref checks, isolated transaction and
three producer integrations are implemented, validated and landed in PRs
#737–#740. All 28 outputs are wired and independently checked. The Canon raw-ID/RF
defects and cold-build/cleanup failures are repaired. The real 193-file same-pin
transaction passed at `1428b6c7` with identical before/after totals and unchanged
caller source/index. The [execution plan](./UPGRADE-NEXT-STEPS.md) preserves the
original evidence and the [landing record](./TAG_MACHINERY_STATUS.md#landed-upgrade-tooling)
identifies the verified squash commits. The
[13.55-to-13.59 retrospective rehearsal](./reference/bump-reports/13.55-to-13.59.md)
then passed at `4fb705da` on 2026-09-11 in 611.006 seconds, with no source-edit
intervention. Current runtime migrations and the classifier/accounting follow-ups
exposed by that experiment are next.

## What carried forward from the September 9 proposal

| Original item | Disposition |
| --- | --- |
| 1.1 Fix the census | Landed as `380babda`; related script correction landed as `c7f5dd81`. Preserve the corrected occurrence accounting. |
| 1.2 Coverage ledger | Partial foundations already exist. Join existing artifacts and inventory hand producers; do not start a second expression ledger. |
| 1.3 Version-bump differ | Repairs landed in PRs #737–#740; same-pin acceptance and the 13.55-to-13.59 retrospective rehearsal passed. Classifier reasons and producer accounting still need the follow-ups below. |
| 1.4 Sample-free walk checker | Proposed extension. Existing expression, condition, subdirectory and carrier checks remain useful; begin with the next table/engine migration. |
| 1.5 Automatic activation | Deferred until certification covers the actual walk and producer conflicts. Keep current activation gates in force meanwhile. |
| Part 2: bulk hand-code retirement | Choose bounded migrations by demonstrated benefit. Remove an overlapping hand producer only after its replacement is validated. |

The proposed ten-week schedule was not a measured estimate. Synthetic probes
test selected inputs; they do not prove arbitrary Perl/Rust equivalence or
real-world offset/firmware behavior. The declaration inventory can be exhaustive
within its extraction scope while runtime testing remains explicitly bounded.

The landed classifier uses the generator's emitter/refusal rules. Its HAND
bucket includes missing verification evidence and conservatively classified
facts; it is not a count of changes that require handwritten code or an
hours-per-upgrade estimate. Generated declaration and artifact counts are not
raw extraction coverage. Keep the manifest, transaction and existing oracles
as the basis of the following work.

<a id="1-measure-a-real-upgrade"></a>

## Completed release rehearsal

The [recorded 13.55-to-13.59 run](./reference/bump-reports/13.55-to-13.59.md)
regenerated all 28 outputs in both private variants and built fresh binaries from
the current `4fb705da` runtime. It required no generator or handwritten-runtime
source edit. The same target oracle and 193-file corpus showed three improved
Garmin identity values and no other semantic change; 1,563 missing occurrences
remain. This is neither a historical-runtime replay nor a complete compatibility
result. The older 13.58-to-13.59 report remains historical evidence.

Static triage reports 5,031 rows, including four standing HAND rows; the
release-only view has 5,027 rows. Those rows are field/metadata review events,
not tags, independent implementation tasks or coding hours. The Garmin module
already has a handwritten FIT parser in both variants, so a HAND message saying
it needs a parser/dispatch is not an accurate statement of current runtime state.

Preserve the run's source/interpreter/dump/binary identities, same-oracle corpus
results, artifact diffs, verifier logs and explicit zero-intervention record.
Future release experiments should retain the same evidence and report their own
cost and limits rather than extrapolate this run's 611.006-second elapsed time.

<a id="2-finish-the-existing-runtime-migrations"></a>

## 1. Finish the existing runtime migrations

The [shared RawConv reconciliation](./reference/rawconv-reconciliation-2026-09-11.md)
completes the six-container runtime consolidation in this change, preserving
current Canon raw-ID forms and both original public EXIF helper APIs. Its
acceptance includes native scalar types, raw/printed output, occurrence
multiplicity, physical IFD1 order and container/EXIF priority. Initial aggregate
gains concealed real regressions; rejected candidates and final measured results
remain separate. PDF retains its existing winner projection until shared visited directory
state is modeled, and other reviewed output debts remain explicit.
Do not restart the old preserved RawConv port or treat its historical gates as
the final implementation's proof.

Next port IFD1 prerequisite `7a69d2fa` without weakening the named verified-key
and input-domain CODE-ref gate, and regenerate using the current pinned pipeline.
Its committed generated-file change only added `unwalked: None` fields; scratch
eligibility results must be reproduced. This remains Gate A, with no Exif::Main
enablement or extraction-gain claim. Later named-directory activation and
retained occurrences are distinct from IFD4 conditions/Olympus retirement and
the incomplete embedded-IFD0/PNG follow-ups. IFD4 can reconcile independently;
respect each existing owner and coordinate heavy gates.

Acceptance for each landing:

1. The generated table's static eligibility, runtime routing and activation are
   individually established. Eligibility alone is not a coverage result.
2. Hand residuals correspond to explicit unsupported/withheld behavior. Generated
   and hand producers have disjoint ownership or an intentionally tested priority.
3. The pinned-oracle comparison preserves occurrences, shows the claimed gain
   and introduces no unexplained value differences or extra output. Keep the
   binary/source/corpus identities with the report.
4. Retire replaced hand code only after these checks pass. Keep unexercised
   behavior explicitly unverified and preserve a bounded reversal path.

<a id="3-complete-accounting-by-joining-existing-evidence"></a>

## 2. Complete accounting by joining existing evidence

Reuse codegen's reports, the expression/value-conversion ledgers, static
reachability and existing occurrence/read-status data. Inventory declarations
whose entire custom processing table is currently skipped, not just fields
inside emitted tables.

Start with the concrete rehearsal findings: make new-module reasons aware of
existing runtime producers without declaring those producers complete; separate
the four standing generator-less outputs from release-caused changes; and group
the four EXPR rows for `Exif::Main` tag 41998 by their one unsupported UTF-8 Decode
cause. Preserve the raw row counts alongside that causal view. The Garmin dump
change does not authorize writing a second FIT parser. Review actual routing and
residual behavior first.

Keep independent axes rather than treating these states as interchangeable:

- **Declaration:** emitted, withheld with reason, unsupported processing
  procedure, or otherwise absent.
- **Producer:** generated engine, legacy generated-table adapter, registered
  hand implementation, unregistered hand implementation, or no known producer.
- **Execution evidence:** statically eligible, enabled, reached by a recorded
  runtime input, and the scope of its differential validation.

A tag can have a generated declaration and a hand producer at the same time.
The inventory must expose that overlap rather than force it into one ambiguous
status. Acceptance: complete accounting within a named dump scope, explicit
exceptions, deterministic diffs and a negative control for a silently omitted
declaration. Add conformance's status/family hooks when their data sources are
ready; avoid creating a parallel model with the same unresolved seams.

<a id="4-broaden-verification-and-retire-hand-code-incrementally"></a>

## 3. Broaden verification and retire hand code incrementally

For the next selected engine/table, synthesize inputs from its declarations and
compare the actual Rust walk with ExifTool, including conditions, nested offsets
and output priority as applicable. Start with existing expression/condition/
subdirectory oracles and carrier fixtures. Add a deliberate incorrect offset or
conversion to prove that the new check detects divergence.

Expand only after the checker exposes a real unsupported behavior or enables a
measured migration. Keep real-file comparisons for container quirks. Generated
Composite computations need checks for their distinct input domain; the scalar
expression oracle does not cover them.

Reconstruct a missing Sony/Nikon generator or extend a shared expression rule
when it removes demonstrated release work or unblocks useful runtime behavior.
Measure the resulting change before choosing the next family. Defer automatic
activation until both walk validation and producer-conflict checks cover it.

## Work deferred from this backlog

- Fleet expansion or a broad per-tag patch campaign without a demonstrated
  throughput bottleneck after these changes.
- A general Perl interpreter or whole-language transpiler as a prerequisite.
- Bulk deletion of hand parsers based on generated tag counts alone.
- Reimplementation of the existing occurrence store, read-status model,
  expression ledger or generation pipeline under a new name.
- Fixed multi-week schedules or claims of complete sample-free correctness
  before the bounded experiments above establish the actual remaining work.
