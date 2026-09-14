# Source-family baseline and generic readers

Status: active under the renewed full-parity objective on 2026-09-14.
PR #775 is the first reproducible QuickTime baseline milestone; #776 repairs
the broader source/artifact inventory. The earlier checkpoint pause is superseded.

This goal starts at `304d6339`, using the repository pin, ExifTool 13.59.
The earlier writer checkpoints (#771–#774) are unfinished and remain separate.

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

Land small complete milestones as they pass. Four to six milestones per day is
a preferred working cadence, not a reason to skip gates or inflate coverage.

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

- PR #775: reproducible historical QuickTime baseline; latest generated-table
  CI check pending, review threads resolved.
- PR #776: corrected source/artifact inventory published with a conserved
  1,512-table report. Static presence and enablement remain distinct from reads.
- PR #777: hydrated catalog identity reconciliation published. The equal-sized
  dump and catalog sets differ by 67 missing catalog tables and 66 extra legacy
  identities plus one shortcut helper. Actual missing-layout capture is next.
- Local ItemList integration: 91 generated specs connected in the primary
  default-locale carrier; 22 behavior fixtures match 44/44 printed/raw
  observations. See `quicktime-generated-reader.md` for incomplete protocols,
  source-dependency guards and required pre-merge validation.
- Writing: no new observed results in these milestones. Writer checkpoints and
  the complete source-family writing denominator still require completion.
