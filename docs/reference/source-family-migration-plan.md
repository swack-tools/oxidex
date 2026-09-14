# Source-family baseline and generic readers

Status: active and incomplete, 2026-09-14. PR #779 is the single consolidated
integration PR. Earlier writer PRs #771–#774 and follow-ups #780/#781 are
preserved in its history. Only #779, #682 and #683 remain open; #682/#683 are
outside this rollup. Further fixes and evidence belong in #779.

The integration base is `b027ce0b`, using the repository pin, ExifTool 13.59.
The full goal includes every catalog family and both reading and writing.

## What we are measuring

Each source table needs four independent records:

1. **Declared:** module, table, raw keys, conditional alternatives, processor
   and source identity from the hydrated Perl dump.
2. **Generated:** emitted definitions, accepted rules and explicit omissions.
   Being listed in a generated registry does not mean the table is enabled.
3. **Connected:** generic executor, enablement policy and verified caller.
   A generic engine existing somewhere does not establish a file-format route.
4. **Observed:** exact source/binary, file corpus, native version, operation
   and result. Reading and writing have separate observations and denominators.

Unknowns stay unknown. Neither a documented name nor a generated definition
counts as an observed read or successful write. A native writable declaration
also does not establish support for creating, replacing or deleting that tag.

## Milestones

### 1. Produce the baseline

Capture the current pinned modules with the current `dump_tables.pl`, retain
its hash and module-selection scope, and reuse `inventory_source_processors.py`
and `join_source_artifacts.py`. Add family summaries and immutable evidence for
consumer/enablement claims. Report missing generated definitions separately
from unavailable protocols and unresolved dynamic dispatch.

Deliverables: machine-readable table/family counts, acceptance/refusal reasons,
runtime evidence, limitations and a reproducible command. No runtime coverage
increase is claimed for this milestone.

### 2. Add a generic QuickTime ItemList path

Inspect ItemList, UserData and Keys from the same capture. Generate raw keys,
names, groups, reading formats and safe conversions. Keep every rejected row
with its exact reason. ItemList data atoms are the first protocol candidate;
UserData language records and Keys indexed names require distinct protocol
handling and must not be treated as equivalent merely because names overlap.

Replace the handwritten ItemList name matches with the generated lookup and
one protocol executor. `plID`/AlbumID is a canary for unsigned 64-bit reading,
not a tag-specific implementation. Reading `Format` and native `Writable`
format can differ, so the writer must not inherit reader assumptions.

Deliverables: generator, generated Rust, omission ledger, generic consumer,
per-behavior fixtures, pinned native comparisons and regeneration checks.

### 3. Extend shared protocol capabilities

Use refusal counts to choose the next useful shared behavior: language handling,
indexed Keys, UserData framing or a conversion family. A change must update the
family ledger and prove that ordinary supported source-row additions regenerate
without adding Rust names or tag lists. Preserve unknown atoms as unknown.

## Evidence required before each implementation PR

- Source tables/rows covered; generated acceptance and refusal counts by reason.
- Real files and synthetic behaviors exercised, with group-qualified results.
- Before/after conformance on an explicitly identified corpus and pinned oracle.
- Clear classification: catalog inventory, generated support, observed reading,
  or observed writing. No global write percentage without a defensible denominator.
- Formatting, lint and relevant native/parser tests; all actionable review
  comments addressed before merge.

Keep implementation milestones separately reviewable as commits in #779. The
maintainer requested one consolidated PR to avoid a growing queue. Merge only
after the combined checks and all actionable review comments are resolved.

## Full-goal completion criteria

QuickTime is the starting protocol, not the final scope. Account for every table
and tag family in the pinned release's BuildTagLookup catalog, including any
catalog families missing from the default hydrated dump. Catalog membership
establishes scope only; hydrated source facts establish decoding and writing rules.

The final ledger must conserve source identities across generated and blocked
rows. It must name the generated artifact, connected executor and caller evidence,
exact omission reason, and separate read/write observations. Unknown coverage
cannot be relabeled supported. A five-file or full-corpus pass cannot prove
untested source-row parity. Completion requires the full declared scope to be
implemented and validated, including unsupported callbacks/protocols discovered
along the way, with all relevant PRs reviewed and merged.

An upgrade comparison must account for added, removed and changed source rows and
processor/conversion contracts, regenerate supported changes, and expose new
unsupported behavior. It must test retained behavior as well as newly added tags.

Progress measures, recorded at each milestone: source identities inventoried;
accepted and refused rows with reasons; verified runtime connections; observed
read matches/missing/value errors; observed write create/replace/delete results;
and committed/merged state. Keep every denominator and fixture/corpus scope visible.


## Current measured checkpoint

- PR #775 merged as `996665ef`: reproducible historical QuickTime baseline.
- PR #776 merged as `4a460cac`: corrected source/artifact inventory, conserving
  1,512 tables. Static definitions and enablement are separate from observed reads.
- PR #777 merged as `f6101205`: hydrated catalog identity reconciliation. The
  equal-sized old dump and catalog differ by 67 missing catalog tables and 66
  extra legacy identities plus one shortcut helper.
- PR #778 merged as `b027ce0b`: complete catalog-entry snapshot, denominator definitions,
  downloadable JSON, Pages report and native regeneration check. It preserves
  33,487 ordinary entries, 21,373 actual case-insensitive entry names, and the
  distinct native legacy counter of 21,437. Container rows remain separate.
- Full hydrated source capture now succeeds with canonical Perl 5.38.2 and
  pinned ExifTool 13.59: 1,512 tables, 34,897 raw keys, 35,886 variants,
  41,818 interned objects and zero unresolved references. The checked-in audit
  and source join conserve all 33,487 ordinary catalog entries. This proves
  source accounting, not runtime support.
- ItemList's generated reader is in #779: 92 accepted declarations and 304
  refusals across the 396 selected ItemList/UserData/Keys source records.
  The recorded behavior suite has 25 fixtures and 50/50 native comparisons;
  this is bounded evidence, not full-catalog reading parity. The 11-file paired
  corpus comparison changed matched occurrences from 622 to 620, VALUE from
  2 to 1, MISSING from 234 to 240, and EXTRA from 114 to 86. Those results are
  not an aggregate conformance pass.
- The permanent source join exists, but its generated-artifact and observed
  read/write joins remain incomplete. A draft QuickTime implementation join
  still needs complete input validation and reconciliation of hydrated source
  wrappers with selector inputs before publication. Other protocol families
  must also be joined; unobserved entries must remain visibly unobserved.
- Writing fixes, numeric directory selection, Nikon generator recovery and
  upgrade-rehearsal fixes are consolidated in #779. Full regeneration currently
  fails at the Nikon encrypted-callback contract. It also exposed a separate
  native address-probe load-context mismatch; the repaired probe emits 191
  address rows and 1,261 candidates against the fresh dump, with six native
  probe tests passing. The focused integrated checks now pass: 219 writer tests
  (two ignored), the exact registry distribution test, 18 native mandatory-default
  tests and library lint. Broader workspace/native matrix checks remain pending.

## Next steps and measurable exit checks

1. **Make regeneration reliable.** Finish the Nikon callback diagnosis, then run
   the sanctioned full regeneration using the recorded Perl and pinned library.
   Require both generation tiers, their independent native verifiers, and the
   declared-write-set check to pass. Inspect lost/added rows and refusals before
   committing generated artifacts; a successful command that emits an empty
   writer registry is not success.
2. **Validate the consolidated runtime.** Run the writer, registry and native
   mandatory-default checks, then the combined workspace and relevant native
   read/write gates. Record skipped tests separately. Reconcile all 14 carried
   review threads with their actual fixes and evidence; keep #779 unmerged
   until the required checks pass.
3. **Finish the useful baseline.** Join catalog table/key/variant identities to
   authenticated generated reader/writer artifacts and exact refusal reasons.
   Publish family counts and remaining shared capability, with explicit input
   hashes. Require all 33,487 current catalog entries to be classified without
   treating declarations as observations. Regenerate this denominator on upgrades.
4. **Connect observations and Pages.** Attach pinned, group-qualified fixture
   evidence separately for reading and real write/read-back operations. Publish
   JSON and the human-readable family report, and make CI reject stale artifacts,
   missing identities or unclassified rows.
5. **Convert the next whole protocol.** Use the family report's refusal counts to
   choose shared UserData, Keys or conversion support. Prove that ordinary source
   row additions enter the generic path after regeneration. Repeat across the
   full catalog; no per-tag handwritten mappings and no claim of full parity
   while required protocols or behaviors remain unimplemented.

## Permanent catalog accounting requirement

The expanded goal requires a per-entry join, not just table totals. Preserve the
full BuildTagLookup table/key/variant identity and case-insensitive name. Join it
with hydrated runtime source, generated reader and writer artifacts, exact
omission reasons, and observed read/write evidence. Every entry must have an
explicit classification, including unresolved joins and unobserved behavior.

The first complete baseline must publish JSON and a human-readable Pages report,
state the denominators, and fail CI if regeneration leaves entries unclassified.
PR #778 establishes the source snapshot and checks its conservation; it does
not yet satisfy the joined implementation/observation ledger requirement.

Measure these axes independently:

1. Catalog entries and distinct names accounted for, preserving table context.
2. Source rows accepted by generated readers and writers, with exact refusals.
3. Observed Group1:TagName identities and occurrences read correctly; identities
   and operations actually written and verified by pinned read-back.

A fixture is required per distinct on-disk format/conversion behavior, not per
catalog name. Generated verification identifies rows sharing that behavior;
only tags actually exercised receive an observed-read or observed-write claim.
