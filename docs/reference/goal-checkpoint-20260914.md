# Generated metadata parity checkpoint

Status: active and incomplete, 2026-09-14. Work is consolidated in PR #779;
#682 and #683 remain separate. This checkpoint distinguishes source declarations
from behavior measured against pinned ExifTool 13.59.

## What exists

The generated BuildTagLookup inventory contains 33,487 ordinary table entries.
Every entry joins the hydrated source inventory. The case-insensitive entry-name
count is 21,373; the native legacy total of 21,437 uses a different definition.
Neither number measures extraction or writing success.

QuickTime ItemList has 92 generated reader declarations. The integrated Keys
reader has 70 direct source-derived declarations and 11 explicit refusals. The
Keys catalog join is being completed separately; the published catalog report
must not be read as already including these new declarations. UserData remains
blocked on its shared record/language protocol. The full hydrated QuickTime
selector accounts for 399 variants across the three tables, including source
rows outside the ordinary catalog denominator.

The actual canonical writer dump replays 19 public writer declarations into the
full catalog ledger. Reader and writer implementation classifications are now
separate. The 193 helper/address rows and historical 15-row writer test cohort
are different denominators. No declaration count is an observed-write count.

## What was validated

The preceding combined run passed all-feature Clippy and 4,895 Rust library
tests, with four ignored. Its integration suite passed 602 tests, failed one,
and ignored 42; the later Python stage did not run. The failing JPEG test passed
a Make-only replacement map, implicitly requesting removal of Model. Its repair
preserves the existing metadata map and asserts exact SOS-through-EOI bytes.

That regeneration command also exposed a source-input defect: it omitted the
hydrated-layout flag, silently reducing the QuickTime ledger by three records.
The output is preserved as diagnostic evidence. The command now explicitly
captures hydrated layouts; a fresh complete regeneration and runtime gate are
required. Eleven shell control tests passed for the hydration change. Updated
controls also exercise the new Keys generator.

Fifty-four focused Python tests pass for catalog joins, QuickTime source/spec
compilation, Keys behavior selection, runtime-input identity and baseline
integrity. The Keys native fixture is recognized by pinned Perl as Keys:Artist;
its OxiDex end-to-end Rust test still needs the combined runtime gate.

## Measurements still required

All catalog observed-read and observed-write fields remain unclaimed until
fresh authenticated runtime reports are imported. The reader verifier records
native/OxiDex JSON, fixture bytes, generated source identities and compiled-input
hashes. It reports distinct Group1 names, fixture/tag occurrences, and print-mode
observations separately. Documentation edits cannot make an old parser binary
current or invalidate unchanged runtime inputs.

The original five-fixture pre-migration baseline is preserved in
`docs/reference/quicktime-reading-baseline.json`: two matched ItemList projections
and three unsigned-integer failures. It is historical evidence, not a current
runtime verdict or corpus-wide percentage. Fresh evidence must be a new report.

## Next steps and completion criteria

1. Finish corrected canonical regeneration, Clippy, Rust and affected Python
   gates; inspect generated changes and explicit refusals before accepting them.
2. Complete the Keys catalog join and regenerate JSON plus the human-readable
   report from matching catalog, hydrated source and reader/writer artifacts.
3. Run fresh native reader and real writer/readback comparisons from immutable
   source, import observed identities, and publish the family report through Pages.
4. Resolve all 14 carried review findings with their required evidence before
   squash-merging #779. Continue changes in that PR until it is ready.
5. Use the family ledger to add shared UserData and other high-leverage protocols.
   Full catalog-wide read/write parity remains the goal; this checkpoint does
   not establish it.

See `docs/reference/source-family-migration-plan.md` for the full objective and
`docs/reference/parity-rollup-review-20260914.md` for unresolved review evidence.
