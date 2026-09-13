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
edge explicitly unwalked. The processor-oracle repair is integrated at
`62535db7`; independent whole-batch source review accepts `ff1e364c`. Exact
exported-fixture checks, the pair after retirement and full corpus are accepted
below. Final hosted acceptance remains required before merge.
No Canon reader is counted as merged retirement yet; no project-wide percentage
follows from this bounded work.

The exact seven TIFF byte vectors used by the public-reader tests are now
exported and independently replayed through pinned ExifTool, in both display
and numeric modes. All 14 native invocations exit successfully and all seven
sets of asserted values/omissions agree. These corrected successful fixtures
have no maker-note offset warning; the warning in the earlier scratch layout
is retained as historical evidence only. Full duplicate selection and native
warning output remain outside these public-reader assertions.

The final display corpus pair at runtime `3d0353f4` covers all 4,238 files and
518,919 native tags. Both builds have 468,087 correct rows, VALUE 420, MISSING
12,240, RENAME 22 and EXTRA 1,560. There are no meaningful per-file raw-output
changes, lost matched keys, input-hash changes or parse/crash failures. The
supervisor retains exit 1 for native's existing zero-byte FujiFilmISPro.jpg
diagnostic; both OxiDex processes exit 0 for that file. Elapsed time: 281.730
seconds. This demonstrates a producer migration without changing corpus output.

A separate immutable CLI triple comparison uses the seven exact fixtures and
three real files, limited to source-defined AFInfo2 fields plus RawDataOffset.
It records eight display and 25 numeric-form improvements with zero scoped
regressions. Fourteen unchanged observations are native Unknown fields
requested with `-u`, which the current OxiDex CLI does not expose; these are
not silently counted as successful extraction. Native warning text remains
outside that CLI comparison.

The first official regeneration attempt at `62535db7` completed its tier-1
generation and independent checks, but failed the write-set guard because the
coordinator edited this plan and CI configuration while it ran. No declared
artifact changed. This is an invalid complete-run attempt, not an accepted
regeneration. The failed log and snapshots are retained.

The frozen retry at `ff1e364c` passes both tiers in 91.262 seconds: all 32
declared artifacts reproduce with zero changes. The actual generated parent
mutation test also runs with fresh canonical oracle inputs: all 140 focused
Python tests pass with zero skips, including processor, validation and stale
artifact rejection checks.

The final bounded pair uses immutable `3d0353f4` after removing the manual
reader: 53 real and 14 constructed inputs, 12,758 native tags, correct rows
11,841 -> 11,843 and extras 43 -> 3. VALUE remains seven. There are no process
failures, changed inputs, lost matched keys or real-file raw-output changes.
The six constructed-file changes match the earlier checkpoint; retaining the
manual reader is therefore not what produced those accepted results.

This delivery migrates reading. Generated writing and its create/update/delete
preservation tests remain a separate workstream in the main plan.

The supported source-change proof passes. A copied pinned Canon.pm changes
only AFInfo2 entry 8's name from AFAreaWidths to UpgradeAFAreaWidths. Official
regeneration of all 32 artifacts succeeds in 247.561 seconds; four generated
files change (the serial name and associated source/dump provenance). Clippy
passes, then the isolated proof build completes in 56.001 seconds. Proof
commit `28f3f0dc` changes no handwritten Rust/Python or carrier code.

On the exact II/MM public fixtures in display and numeric modes, the regenerated
binary changes that output key, preserves its value and matches the independently
executed modified native source. All other meaningful output is unchanged.
The original unfiltered comparison is retained: two first reads changed the
filesystem FileAccessDate. The accepted comparison excludes only that established
volatile key; it does not ignore any tag-value discrepancy. This proves the
selected supported name change, not arbitrary Perl translation or a release
upgrade. Evidence is under the batch's `source-upgrade-proof/` and the preserved
native `BATCH/source-upgrade-proof/native-proof/` subdirectory.

PR #760 opened ready at `d6111d24`. Its first hosted Build & Test job failed
`test_parse_af_info2_array`: the old constructed record declared AFInfoSize=0
but expected child values. Native complete-carrier replay confirms rejection
at zero and all seven original asserted values at the correct 382-byte size.
Commit `78fb7921` retains explicit zero-size rejection assertions and repairs
the positive fixture; production code is unchanged. Formatting and Clippy
pass. The full local `cargo test --all-features --no-fail-fast` invocation
passes in 150.641 seconds: 5,993 passes across unit/integration/doc summaries,
zero failures and 124 ignored tests. The attempted local nextest invocation
could not run because that executable is not installed; its failed attempt is
retained. Hosted nextest and all required checks must pass on the final head.
