# Remaining automation work

**Reviewed 2026-09-10 at `c7f5dd81`, ExifTool 13.59.** This is the maintained
backlog accompanying [Tag machinery status](./TAG_MACHINERY_STATUS.md).
It replaces the local September 9 proposal's ordering and speculative schedule.
The status page owns completion history; this page owns remaining work and its
acceptance criteria. No parser or generator change is implemented by this plan.

## What carried forward from the September 9 proposal

| Original item | Disposition |
| --- | --- |
| 1.1 Fix the census | Landed as `380babda`; related script correction landed as `c7f5dd81`. Preserve the corrected occurrence accounting. |
| 1.2 Coverage ledger | Partial foundations already exist. Join existing artifacts and inventory hand producers; do not start a second expression ledger. |
| 1.3 Version-bump differ | A bump script and classifier already exist. Repair their current gaps, then rehearse a full transition. |
| 1.4 Sample-free walk checker | Proposed extension. Existing expression, condition, subdirectory and carrier checks remain useful; begin with the next table/engine migration. |
| 1.5 Automatic activation | Deferred until certification covers the actual walk and producer conflicts. Keep current activation gates in force meanwhile. |
| Part 2: bulk hand-code retirement | Choose bounded migrations by demonstrated benefit. Remove an overlapping hand producer only after its replacement is validated. |

The proposed ten-week schedule was not a measured estimate. Synthetic probes
test selected inputs; they do not prove arbitrary Perl/Rust equivalence or
real-world offset/firmware behavior. The declaration inventory can be exhaustive
within its extraction scope while runtime testing remains explicitly bounded.

## 1. Repair the existing upgrade workflow

**Why first:** without a trustworthy upgrade experiment, neither an AUTO
percentage nor an estimate of recurring maintenance is reliable.

Scope:

- Establish one generated-artifact manifest used by regeneration, verification,
  before/after builds and restoration. Include implicit companion outputs such
  as `src/composite/generated_compute.rs` and both conversion ledgers.
- Use complete old/new artifacts in isolated checkouts. The old comparison must
  not retain target-version tier-2 outputs.
- Make IFD classification agree with the generator. Separate declaration
  transcribability from activation and observed execution.
- Repair the dirty-tree/provenance handling for a deliberate regeneration and
  reliable cleanup on success, failure and interruption. Preserve the ordinary
  measurement guard.
- Keep standing generator-less files and pre-existing refusals separate from
  debt introduced by this release.

Acceptance: every written artifact is accounted for; a dry run restores the
starting state; a deliberate failure demonstrates cleanup; an IFD declaration
supported by codegen is classified accordingly; neither comparison build mixes
releases. Do not use a manually duplicated test manifest to prove completeness.

## 2. Measure a real upgrade

Rehearse 13.55 to 13.59 after item 1. This published interval is a test input,
not a change to the repository's supported pin. Retain the smaller 13.58 to
13.59 report as historical evidence.

The work order must identify:

- Supported declaration changes regenerated without source edits, with verifier
  scope and runtime activation shown separately.
- Unsupported expression/condition/procedure changes, with upstream source and
  the shared rule or hand implementation requiring work.
- Changes in upstream declarations with no current OxiDex producer.
- Changed upstream behavior implemented by hand that needs review; a source hash
  change is a review signal, not proof of a regression.
- Old compatibility debt, unexercised changes and unavailable evidence.

Acceptance: retain source SHAs, ExifTool versions, Perl/capability provenance,
commands, artifact diffs, classifier output, verifier results and a corpus A/B
with the same comparison script and explicit floors. Include controls that make
a known incorrect declaration and an unaccounted generated output fail. Report
manual interventions and elapsed time for this run, without extrapolating a
fixed cost for every future release.

## 3. Finish the existing runtime migrations

Coordinate with the owners listed in the status page before starting work.
IFD1 is the next high-value candidate identified by the corrected census;
RawConv and embedded-IFD work are related existing branches.

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

## 4. Complete accounting by joining existing evidence

Reuse codegen's reports, the expression/value-conversion ledgers, static
reachability and existing occurrence/read-status data. Inventory declarations
whose entire custom processing table is currently skipped, not just fields
inside emitted tables.

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

## 5. Broaden verification and retire hand code incrementally

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
