---
name: exiftool-parity
description: Use when a release or parser-regression task requires measuring OxiDex output against pinned ExifTool, diagnosing an observed metadata mismatch, or producing provenance-backed parity metrics; not for an ordinary parser change without comparison evidence.
---

# ExifTool parity

Measure the public output produced by an exact OxiDex commit. Every number must
name its instrument, commit, binary, oracle, corpus, options, and denominator.
This skill owns comparison evidence; it does not authorize parser edits,
documentation approval, release promotion, or publication.

## Inputs and output

Require a clean candidate SHA/tree, the repository's `.exiftool-version`, an
explicit corpus and floors, a dedicated worktree/target, and a durable evidence
root outside tracked content. Copy
[`release-parity-receipt.json`](templates/release-parity-receipt.json), populate
it only from retained artifacts, and validate it before handoff.

Read [`harnesses.md`](references/harnesses.md) before running any comparison.
It defines the capability-checked Perl/ExifTool oracle, lock input, build proof,
conformance, authenticated reads, regression gate, catalog checks, and JPEG
matrix recipes. For a release claim, also read
[`release-metrics.md`](references/release-metrics.md).

## Ordered workflow

1. Run `tools/preflight.sh`, freeze the clean candidate identity, and create a
   unique evidence directory. Refuse dirty/stale source or binaries.
2. Run `python3 tools/ci/release_oracle.py` with the configured interpreter and
   pinned tree. A matching version alone is insufficient; every capability
   probe must pass before measurement.
3. Hash corpus manifests, declare filters and minimum file/native-occurrence
   floors, then run the relevant instruments exactly as documented. Preserve
   JSON, logs, argv, exits, manifests, binary hashes, and per-file detail.
4. For attribution, rebuild and remeasure the exact base and head in separate
   clean worktrees with identical oracle, corpus, options, and comparator.
   Aggregate improvement never proves zero regressions.
5. Keep conformance, authenticated reads, generated-catalog declarations, and
   write-matrix results separate. Run the receipt validator and hand the hashed
   verified receipt to `oxidex-release-documentation`.

## Claim boundaries

- Never publish one blended overall parity percentage.
- Generated declarations and ExifTool `-listx` are tag knowledge, not observed
  extraction coverage. Detected-only identity tags do not prove payload reads.
- Rename ceilings are provisional diagnostics, not earned parity.
- Missing or incomplete evidence is `unverified`; failed probes, floors, or
  regressions are `blocked`. Never substitute zero, an older baseline, or a
  different oracle.

## Gap diagnosis

For an observed gap, inspect the pinned Perl table and
`src/exiftool_tables::find_table(module, table)` before implementation; see
`docs/TRANSCRIPTION.md`. A missing generated row may mean the generator refused
an unsupported conversion. Never approximate conversions or hand-edit generated
tables. Re-express source behavior, test it against the pinned oracle,
regenerate through the producer, and measure again.
