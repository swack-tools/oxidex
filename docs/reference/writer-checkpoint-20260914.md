> Historical writer checkpoint, preserved during consolidation into PR #779.
> The full source/read/write goal has resumed; current integration validation is pending.

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

Generated public address resolution, descriptor ownership, TIFF/JPEG carrier
edits, native mandatory defaults and complete writer source-closure artifacts.
Prior validated integration: 6c5a6aa0, nine generated final/public identities.
The full-55 gate recorded 6,077 passing tests (126 ignored), 432/432 native
public-write operations and Clippy passing. This does not validate every
subsequent branch or all writable tags.

## Outstanding

Reconcile this branch with refactor/tag-machinery and the numeric checkpoint,
regenerate every joined artifact from one canonical capture, then run the
combined gate. Numeric work is based on an earlier writer snapshot: do not
replace full source-closure ledgers with its older ledgers during integration.
The historical checkpoint carries additional version profiles and adapters.

## Checkpoint branches

- codex/generated-write-address-runtime-20260914
- codex/numeric-public-write-r3-20260914
- codex/terra-nikon-regen-preparation-20260914
- codex/terra-numeric-matrix-adapter-v4-20260914

Full local evidence is indexed by the root closeout record and the existing
untracked HANDOFF.md files. Large captures and native outputs remain archived
outside Git; this document records their limitations rather than fabricating
fresh passing results.
