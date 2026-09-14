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

Numeric scalar compilation, explicit IFD1 operations, mandatory-directory
cleanup, expanded public operation matrices and full native lookup identity
capture. The draft declares 19 selected public identities. Six handwritten
descriptors were removed after the last full gate.

The last complete gate (r7, before the final descriptor and v3 probe edits)
FAILED: library 4,873 passed / 3 failed / 4 ignored; public matrix 1,458/1,530;
fresh batch 108/299. All original 1,242 requests passed. All 72 remaining
existing-file failures were Artist, as were the 191 fresh-batch failures.
These old results are not passing evidence for the final checkpoint.

Native v3 probe captured both Artist candidates: Exif::Main and
PanasonicRaw::Main at raw ID 315, with different Permanent controls. The
addressing/codegen focused suite reported 23 passing tests. New lookup fields
are not yet fully regenerated or connected to byte-derived carrier selection.

## Outstanding

- Complete byte-derived carrier/native-table routing; never collapse candidates
  solely because tag IDs, names or groups match.
- Regenerate addressing, descriptors, final stage and public migration artifacts
  together from one verified capture; audit the probe helper-authentication change.
- Preserve all 19 identities and original 1,242 requests; test RW2 routing,
  existing-file and fresh-file operations, payload and directory preservation.
- Reconcile this older writer base with the writer-foundation checkpoint and
  current upstream. Repeat full library, public, fresh, native and lint gates.
- Eight accepted-by-native numeric boundary inputs remain explicit omissions;
  the independent 1,240-case probe found no incorrectly accepted bytes.

The full dump launched during closeout lost its parent exit code. Its output
SHA-256 is exactly the previously validated full capture:
5f04aa37838e79a81bbbf2ec33b14f19cf01421077b9a9024dd984e4628e6147.
Do not call that orphaned process a successful gate. The later bounded v3
probe has an explicit exit 0 and preserved logs.

## Closeout checks

Final cargo fmt check and workspace all-features Clippy (-D warnings) passed.
These are formatting/compilation/lint results; the outstanding parity and
regeneration gates above are still required.
