# Replace Canon's manual AFInfo2 reader

Updated September 13, 2026. Base: `8887e5d9` (merged PR #759).

## Goal and finish conditions

Both native Canon Main parent edges, `0x0026` and `0x003c`, must execute the
source-generated AFInfo2 table through the shared IFD and serial readers.
Remove the duplicate manual field sequence and its private offsets/conversion
declarations after parity is demonstrated. A bridge with the manual producer
still present is an intermediate checkpoint, not a completed migration.

| Measure | Starting point | Required result |
| --- | --- | --- |
| Generated AFInfo2 alternatives | 16/16, independently verified | Preserve complete source accounting and fail stale artifact mutations |
| Parent edge behavior | Manual production reader; generated validation marker cannot execute | Derive effective processor, validator operands, parent condition/state, bounded child bytes and order from authenticated native facts |
| Production output | Manual field sequence | Generated child rows supply the actual caller's display and numeric values in correct parent order |
| AFInfo2 manual reader | One shared arm for two parent IDs | Remove the arm and AFInfo2-only definitions; retain helpers genuinely used by old AFInfo or CIFF |
| Parity | Exact control build at the merged base | Native/control/candidate evidence for real files and constructed boundaries; full paired corpus has no unexplained regression |
| Upgrade behavior | Table definitions can regenerate | A supported copied-source change reaches actual carrier output with no tag-specific Rust/Python edit; stale validation or processor facts are rejected |

## Implementation

Generate the effective child processor from its target table and any explicit
edge override. Reuse the existing authenticated unsigned-16 size validator.
The shared bridge passes the original bounded child span, byte order and
source-selected parent effects, then buffers child output until execution is
known valid. Native condition/validation omissions and unsupported execution
must remain distinguishable in the reader's result.

An explicit activation policy transfers ownership to the generated route.
Once transferred, an execution refusal cannot silently revive a handwritten
AFInfo2 decoder. Discard speculative child rows and state on refusal, while
preserving the native parent effects that happened before child execution.
Unmigrated families remain explicit residual work.

Native AFInfo's geometry validator is different and remains a separate
migration. The four CanonRaw parent omissions also remain counted. Do not
claim that this batch retires old AFInfo, CIFF or unrelated Canon readers.

## Acceptance and evidence

Use pinned ExifTool 13.59 and canonical Perl 5.38.2. Real parent discovery
uses native verbose directory traces or the actual parent IDs, because emitted
tag names do not reliably name their source table. An earlier 120-file search
for `CanonAFInfo2`/`AFInfo3` output keys was the wrong instrument; its absence
result is withdrawn. Native traces confirm AFInfo2 in Canon1DmkIII.jpg,
CanonEOS-1D_MarkIII.jpg and CanonPowerShotSX740HS.jpg.

Constructed TIFF/MakerNote cases cover both byte orders, nonzero child starts,
both parent IDs and their ordering, EOS conditions, signed arrays/multiword
bits, zero and short counts, rejected size, four leading NUL bytes, and a
later valid sibling after refusal. Preserve warnings and failed fixture
construction attempts separately; raw callback proof is not full carrier
output proof. Existing occurrence/group limitations must be recorded against
the control and must not be relabeled as new successful output.

Evidence is stored relative to `$OXIDEX_WORK_EVIDENCE`:

- `shared-pilot/afinfo2-production-integration-20260913/`: exact control build,
  candidate regeneration/build, comparisons, reviews and publication results.
- `shared-pilot/serial-afinfo-integration-20260913/parent-bridge-contract/`:
  independent retirement checklist, native parent contract and carrier fixtures.

Three Terra workers author the source, independently review it, and prepare
native fixtures. The coordinator alone performs combined builds and corpus
comparisons using one shared Cargo cache. Published source checkpoints are
allowed before full acceptance, with pending checks explicit; merge requires
the complete migration and its gates.

## Current state

The source checkpoint is published on
`codex/afinfo2-production-integration-20260913`. Integration removes the manual
AFInfo2/AFInfo3 arm, eight private sequence offsets, two parent-ID constants and
the private 20-value AFAreaMode enum. The shared reader is the sole replacement.
An independent source review accepts the ownership and rollback design.

At the earlier `757b861e` checkpoint, a fresh native/control/candidate pair
covered 53 real and 14 constructed files: 12,758 oracle tags, no process or
parse failures, correct rows 11,841 -> 11,843 and extras 43 -> 3. All 53 real
outputs were unchanged. Per-file matched-key sets lost no correct row; changes
were native rejection of invalid-size/zero-prefix children and two zero-count
PrimaryAFPoint corrections. This checkpoint still contained the fallback arm,
so its pair is not final retirement evidence.

With the manual arm removed, three public-reader tests pass, covering both
byte orders, signed/multiword data, EOS/AFInfo3 state, invalid size, truncation,
zero count and later siblings. The first shared-test compile exposed a missing
test-only Ctx import; after repair, all 55 IFD-reader tests pass. All 61 IFD
codegen tests and full Clippy pass. Retain that failed attempt in the evidence.

Independent artifact review also found that supported processor selection was
not yet verified and that old AFInfo's unsupported geometry validator was
incorrectly emitted as executable. The generator now leaves that distinct
edge explicitly unwalked. The processor-oracle repair, full official
regeneration, exact exported-fixture oracle checks, the pair after retirement,
upgrade proof, full corpus and final hosted acceptance remain required before
merge. No Canon reader is counted as merged retirement yet; no project-wide
percentage follows from this bounded work.
