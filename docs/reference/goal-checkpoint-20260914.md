# Migration goal checkpoint — 2026-09-14

The maintainer stopped this goal and requested PRs preserving the current work.
Do not resume autonomous migration or merge unfinished checkpoints until a new
request establishes the next goal. These branches overlap; they are not four
independent changes that can be merged in any order.

## Measured state

- Reading: conformance.py with pinned ExifTool 13.59, 4,238 files,
  468,087 / 480,769 expected occurrences = 97.3621427338%. This measurement
  belongs to source b0cc3c62, merged as 3cf7c522; it is not a fresh measurement
  of this checkpoint and includes generated and handwritten readers.
- Writing: overall coverage is unknown. The full archived source inventory
  contains 1,512 tables, 33,073 rows and 34,260 variants; only 2,699 explicitly
  declare Writable true. Missing effective values do not mean unsupported.
  No inventory count is an overall public-write coverage denominator.
- Inventory tool PR #770 merged as 304d6339 with all five enabled CI checks
  passed, no unresolved review threads; Benchmarks was skipped.

## Preserved work

Version rehearsal executor/stage adapter, historical native source profiles,
fresh-JPEG byte-order and raw-JFIF profiles, and v4 numeric matrix contract.
Adapter source b8256314 had 21 passing offline tests. Historical profile source
checks reported 14 native passes; neither count proves a complete cross-version
public read/write rehearsal.

## Outstanding

The persisted randomly selected pair is 11.78 / 12.64. Do not redraw it to
avoid a failure. Run the complete real read/write upgrade rehearsal after
writer/numeric integration is validated. The v4 adapter describes 15 targets
and 1,242 requests; reconcile it with the 19-target draft without dropping
old request identities or substituting a smaller green subset. Bring forward
writer-foundation fixes when reconciling the overlapping branches.
