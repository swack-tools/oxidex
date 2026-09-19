---
name: exiftool-parity
description: Use when measuring OxiDex output against pinned ExifTool, verifying parser regressions, diagnosing a single metadata file, or producing provenance-backed parity metrics for release documentation.
---

# ExifTool parity

Measure the public output that the selected OxiDex commit actually produces.
Every published number names its instrument, commit, binary, oracle, corpus,
options and denominator. Read [harnesses.md](references/harnesses.md) before
running comparisons; for release claims also read
[release-metrics.md](references/release-metrics.md) and fill
[release-parity-receipt.json](templates/release-parity-receipt.json).

## Oracle prerequisite

Read the expected ExifTool release from the repository's `.exiftool-version`.
The canonical local interpreter is
`/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2`; the pinned tree is
`/tmp/oxidex-exiftool-cache/exiftool`. Invoke that interpreter explicitly with
the tree's library and script, with user configuration excluded. Require
Perl `v5.38.2`, the pinned ExifTool version, working standard/decompression
modules, and `OOXML.docx` reporting **DOCX**, not ZIP. A version probe alone is
insufficient. Export `EXIFTOOL_PERL` for harnesses that resolve their own oracle.

If `strict.pm` is missing, any probe fails, or the pin differs, record
`status: blocked` and the failed command/exit/stderr. Do not run a sweep or
publish a score. Never fall back to Homebrew, another Perl, a PATH-resolved
oracle, or an allow-skew switch, even if the version string matches. Recovery
requires restoring this canonical Perl 5.38.2 installation with its matching
standard library and required modules, then passing every probe again. Keep
repair work separate from the refused measurement. The installation was known
broken during this skill's 2026-09-19 revision; re-probe rather than assuming
either continued failure or recovery.

## Measurement workflow

1. Use a dedicated `staging/<slug>` worktree and `CARGO_TARGET_DIR`. Run
   `tools/preflight.sh`; verify the exact base/head commits. Refuse dirty or
   stale source/binaries, including instrument staleness warnings. Release
   evidence may not use dirty-tree or skew overrides.
2. Create a unique, durable evidence directory outside the checkout. Record
   clean SHA/tree, tool versions, command argv, environment affecting the run,
   exit codes, raw logs and hashes. Build an explicit public CLI with
   `corpus_read_receipt.py build`; use its Cargo-selected executable and hash.
   Recheck source and binary identity after measurement.
3. Probe the oracle before expensive work. Select and hash corpus manifests;
   record roots, filters and exclusions. Use `--recursive` for nested corpora,
   and set justified `--min-files` and `--min-tags` floors before observing
   results. Missing roots and breached floors are refusals, not smaller scores.
4. Run conformance with explicit `--oxidex`, `--exiftool-dir`, and `--json-out`.
   Keep full JSON plus logs; a console TOTAL line is not a release artifact.
   Keep per-format/per-file MISSING, VALUE, RENAME and EXTRA detail, duplicate
   occurrences, group identities, score, rename ceiling and precision.
5. For change attribution, rebuild and remeasure the exact base commit in its
   own clean worktree/target, then head with the same oracle, corpus and
   options. Do not reuse yesterday's or a supplied stale baseline. Compare
   per-file identities and occurrences; a better total can hide regressions.
6. For release metrics, run the separate authenticated-read receipt/verifier
   and published-read regression gate, catalog ratchet, and JPEG write matrix
   as scoped in the references. Use shared locking for builds/tests and
   exclusive locking for corpus sweeps/read gates. Cap expensive parallel work.
7. Hand the filled receipt and hashed evidence to
   `oxidex-release-documentation`. Mark missing required evidence `unverified`
   or failed/refused evidence `blocked`; never substitute zero or a pass.

## What each claim means

Keep conformance, authenticated reads, generated catalog declarations and
write-matrix results separate. **Never report one blended overall parity
percentage.** Conformance output agreement does not establish a source row was
executed, and a JPEG round trip does not establish all-format write support.

Generated `oxidex-tags-*` declarations and the `-listx` documentation view are
tag knowledge, not observed extraction coverage. Detected-only identity tags
(`FileType`, `FileTypeExtension`, `MIMEType`) do not earn observed-read credit
for metadata payloads. Retain them in the instrument's output if it counts
them, but identify that scope explicitly. Do not alter counts to improve a
claim. Source-coordinate credit comes only from the authenticated instrument.

Before implementing a gap, inspect the pinned Perl table and
`src/exiftool_tables::find_table(module, table)`; see `docs/TRANSCRIPTION.md`.
A missing generated row can mean the generator refused an unsupported
conversion. Never approximate conversions or hand-edit generated tables.
Re-express the source behavior, test against the oracle, regenerate through
the generator, and measure again. A rename ceiling is a diagnostic estimate,
not earned parity.
