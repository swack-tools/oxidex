# Stopped goal checkpoint — 2026-09-14

The maintainer stopped this goal and requested preservation in a PR. This is
an unfinished checkpoint, not an implementation-complete or merge-ready milestone.
Base: `304d6339274d14f3a26cfeed85e2428855ea688a`; pinned ExifTool: `13.59`.
Do not resume automatically. The next objective belongs to the maintainer.

## Preserved work

- Prospective QuickTime source capability selector and five unit tests.
- ItemList: 105 raw rows, 91 declarative candidates, 14 refused records.
- UserData: 186 raw rows / 210 alternatives; Keys: 81 raw rows. Both protocols
  remain refused, not implemented.
- Five behavior fixtures and a pre-migration ItemList-only comparison at the
  base commit: text and enum match; unsigned 16-bit and two unsigned 64-bit
  fixtures fail. This is 2/5 fixture projections, not overall reading coverage.
- Source-family migration plan retained for a future objective.

## Validation and provenance

`python3 -m unittest discover -s tools/exiftool-tables -p 'test_quicktime_atom_tables.py'`
passes all five tests. JSON parsing and fixture hashes were checked at checkpoint.
The capability report was regenerated from the captured QuickTime subset with
this exact selector. Full hydrated dump SHA-256:
`05c13f3ee804838a6d8db5b2f8d64156883dd98afe5a5b0983b6870a8a561556`.
The report separately records the subset input hash and selector hash.
Capture used Perl 5.38.2, ExifTool 13.59, and the base commit's dump tool.

## Unfinished

No generated Rust reader, runtime connection, writing support, or coverage gain
is included. Candidate eligibility still needs validation against actual generic
runtime capabilities and the pinned processor contract. A reusable fixture
builder/native replay command, compact summary plus separate row ledger,
independent review, and implementation gates remain future work. The associated
family-consumer inventory is a separate unfinished checkpoint.

Shared-base validation: `cargo clippy --lib -- -D warnings` passed. Both
checkpoint worktrees have identical Rust/Cargo source to the base; this validates
that source only and does not resolve the Python inventory limitations.
