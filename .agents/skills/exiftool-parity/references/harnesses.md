# Running the parity instruments

Run Bash examples in order from the clean candidate worktree. Replace all
`/absolute/...` inputs and set corpus floors from the approved corpus manifest
or prior independently authenticated scope, before running. Record commands,
exit codes and stderr even on refusal. These commands are a recipe, not
evidence that a measurement has already run.

## Setup and fail-closed probes

```bash
set -euo pipefail
tools/preflight.sh
PARITY_SHA=$(git rev-parse 'HEAD^{commit}')
PARITY_TREE=$(git rev-parse 'HEAD^{tree}')
EVIDENCE_ROOT=/absolute/durable/evidence/root
test -d "$EVIDENCE_ROOT"
PARITY_EVIDENCE=$(mktemp -d "$EVIDENCE_ROOT/parity-${PARITY_SHA}.XXXXXX")
export CARGO_TARGET_DIR=/absolute/dedicated/parity-target
PARITY_LOCK=/Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py
PARITY_PERL=/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2
PARITY_ET_TREE=/tmp/oxidex-exiftool-cache/exiftool
PARITY_PIN=$(tr -d '\r\n' < .exiftool-version)
export EXIFTOOL_PERL="$PARITY_PERL"
export EXIFTOOL_CACHE_DIR=/tmp/oxidex-exiftool-cache
unset EXIFTOOL OXIDEX_ALLOW_EXIFTOOL_SKEW OXIDEX_ALLOW_DIRTY_TREE
unset PERL5LIB PERLLIB PERL5OPT
mkdir "$PARITY_EVIDENCE/empty-oracle-home"
export EXIFTOOL_HOME="$PARITY_EVIDENCE/empty-oracle-home"
test ! -e "$PARITY_ET_TREE/.ExifTool_config"
PARITY_ORACLE=("$PARITY_PERL" "-I$PARITY_ET_TREE/lib" "$PARITY_ET_TREE/exiftool" -config '')
printf '%s\n' "$PARITY_SHA" "$PARITY_TREE" > "$PARITY_EVIDENCE/source.txt"
"$PARITY_PERL" -e 'print $^V' > "$PARITY_EVIDENCE/perl-version.txt" 2> "$PARITY_EVIDENCE/perl-version.stderr"
test "$(cat "$PARITY_EVIDENCE/perl-version.txt")" = v5.38.2
"$PARITY_PERL" -Mstrict -Mwarnings -MArchive::Zip -MCompress::Zlib -e 1 > "$PARITY_EVIDENCE/modules.txt" 2> "$PARITY_EVIDENCE/modules.stderr"
"${PARITY_ORACLE[@]}" -ver > "$PARITY_EVIDENCE/oracle-version.txt" 2> "$PARITY_EVIDENCE/oracle-version.stderr"
test "$(cat "$PARITY_EVIDENCE/oracle-version.txt")" = "$PARITY_PIN"
"${PARITY_ORACLE[@]}" -s3 -FileType "$PARITY_ET_TREE/t/images/OOXML.docx" > "$PARITY_EVIDENCE/docx.txt" 2> "$PARITY_EVIDENCE/docx.stderr"
test "$(cat "$PARITY_EVIDENCE/docx.txt")" = DOCX
```

Stop on any nonzero command, recording that exit and the failing prerequisite
in the receipt. Do not continue a partially executed shell recipe. Record
interpreter/script SHA-256 and library fingerprint from the authenticated
receipt; before that exists, use explicit file hashes and record the library
tree identity. The shared Python resolver obeys `EXIFTOOL_PERL`; its generic
fallback suggestions are not permission to change the release oracle.
Direct probes and authenticated reads use `-config ''`. The conformance and
library harnesses do not add that flag: the empty `EXIFTOOL_HOME` and refusal
of a script-directory `.ExifTool_config` exclude default configuration for
those commands. Recheck these paths after the run and record the environment.

## Build proof and single-file diagnosis

```bash
set -euo pipefail
python3 "$PARITY_LOCK" --shared "$PARITY_EVIDENCE/build.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py build --output "$PARITY_EVIDENCE/build"
PARITY_BIN=$(jq -er '.binary.path' "$PARITY_EVIDENCE/build/build-proof.json")
test -x "$PARITY_BIN"
PARITY_FILE=/absolute/path/to/sample
"${PARITY_ORACLE[@]}" -j -a -G1:4 -s "$PARITY_FILE" > "$PARITY_EVIDENCE/single-native.json"
"$PARITY_BIN" -j -a -G1 "$PARITY_FILE" > "$PARITY_EVIDENCE/single-oxidex.json"
"${PARITY_ORACLE[@]}" -G1 -s -a "$PARITY_FILE" > "$PARITY_EVIDENCE/single-native.txt"
```

Use the authenticated reader below for exact print/raw agreement. Manual
single-file output is diagnostic: family-4 copies and OxiDex duplicate suffixes
need occurrence-aware comparison. JSON lists and plain-text display are not
interchangeable. Do not infer a regression from a bare tag-name join.

## Conformance and fresh base/head comparison

Run separate evidence directories for `t/images` (pinned format breadth),
`combined-samples` (manufacturer samples), and any chosen combined run with
`tests/fixtures`. Corpora are different denominators; report each by name.
Conformance accepts multiple roots and deduplicates identical paths, not
byte-identical copies at different paths. Its built-in exclusions are in
`conformance.py`; record additional extension/name filters. Scaffolding files
in fixture roots can inflate tag totals, so choose documented exclusions
before measuring, apply them equally to base/head, and retain the manifest.

```bash
set -euo pipefail
: "${PARITY_MIN_FILES:?Set the approved file floor}"
: "${PARITY_MIN_TAGS:?Set the approved native occurrence floor}"
PARITY_CORPORA=("$PARITY_ET_TREE/t/images" /tmp/oxidex-exiftool-cache/combined-samples)
python3 "$PARITY_LOCK" "$PARITY_EVIDENCE/conformance.log" -- \
  python3 tools/exiftool-tables/conformance.py "${PARITY_CORPORA[@]}" \
  --exiftool-dir "$PARITY_ET_TREE" --oxidex "$PARITY_BIN" --recursive \
  --min-files "$PARITY_MIN_FILES" --min-tags "$PARITY_MIN_TAGS" \
  --json-out "$PARITY_EVIDENCE/conformance.json"
test "$(git rev-parse 'HEAD^{commit}')" = "$PARITY_SHA"
test -z "$(git status --porcelain)"
```

The lock writes `.status.jsonl` beside each log with actual argv/cwd/exit.
Also hash the binary before/after and keep the source, corpus and tool
manifests: the conformance JSON alone does not contain full provenance.
Its `per_format` and `per_file` data are the source of summary counts, not
grepped console output. Nonzero exits or below-floor results invalidate the
run even if partial output exists.
Reconcile the selected manifest against `per_file`: the current conformance
runner can skip a file whose native JSON is empty/unparseable and does not
preserve each subprocess exit/stderr. Floors alone do not prove every selected
file was scored. Investigate every omitted file with captured native/public
commands, and block a complete-corpus claim until omissions are accounted for
and failed executions are resolved. Use authenticated transcripts for stronger
per-file evidence. `--min-tags` counts native keys before comparison exclusions;
do not confuse that floor with the scored denominator in the release summary.

For a parser fix, repeat the entire setup/build/measurement at the exact
base SHA in a separate clean worktree and target. Preserve fresh base and
head JSON, compare per-file group/occurrence outcomes and report new losses,
VALUE changes and EXTRA changes as well as gains. Corpus/options/oracle must
match; a mismatch blocks attribution. Preserve the tool commit on both sides;
if comparator semantics changed, rerun both with one identified comparator
and explicitly document how each target binary's provenance was established.
Never claim “zero regressions” from improved aggregate score alone.

## Authenticated public reads and published-read gate

`corpus_read_receipt.py` records both print/raw modes, source capture, binary
build proof, library hashes and a recursive per-file corpus hash manifest.
It refuses dirty state and existing output directories. Run once per root;
do not pretend it supports multiple `--corpus` roots in one receipt.

```bash
set -euo pipefail
: "${PARITY_READ_MIN_FILES:?Set the approved authenticated corpus floor}"
PARITY_READ_CORPUS="$PARITY_ET_TREE/t/images"
python3 "$PARITY_LOCK" "$PARITY_EVIDENCE/observe.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py observe \
  --build-proof "$PARITY_EVIDENCE/build/build-proof.json" \
  --perl "$PARITY_PERL" --exiftool-dir "$PARITY_ET_TREE" \
  --corpus "$PARITY_READ_CORPUS" --min-files "$PARITY_READ_MIN_FILES" \
  --output "$PARITY_EVIDENCE/observe"
python3 "$PARITY_LOCK" "$PARITY_EVIDENCE/verify.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py verify \
  --receipt "$PARITY_EVIDENCE/observe/receipt.json"
python3 "$PARITY_LOCK" "$PARITY_EVIDENCE/read-gate.log" -- \
  python3 tools/ci/read_regression_gate.py --receipt "$PARITY_EVIDENCE/observe/receipt.json"
```

Set and enforce an independent native-occurrence floor on verified `metric_c`
counts; this instrument has `--min-files` but no `--min-tags` flag. Keep failed
file modes, missing/mismatched/unattributable identities and withheld source
coordinates visible. Receipt verification proves internal replay integrity;
the consumer must also compare its producer commit, binary and corpus with
the requested release scope.

The read gate compares against the published observed snapshot and catalog
under `docs/public/measurements/` (or explicit `--snapshot`/`--catalog`). Record
their paths, hashes and source commits. It is a separate regression policy,
not a replacement for a fresh base/head measurement. Do not update a published
snapshot merely to make the gate green. Record a refused measurement separately
from a measured code regression; both block the corresponding release claim.

## Catalog, library harness and JPEG matrix

| Instrument | Execution and scope |
| --- | --- |
| Catalog ratchet | `python3 tools/ci/parity_ratchet.py report` and `check`; archive logs plus referenced floors and source JSON hashes/commits. Measures published catalog/join counters; does not itself run a corpus or prove those snapshots current. Verify snapshot provenance against the candidate. Do not use `raise` to conceal losses. |
| Generated-table verification | Run `just verify-tables` under the shared lock after confirming its pinned interpreter resolution. Generated tables must be regenerated by their producer, never hand-edited. |
| `tag-comparison` | Build `--release --bin tag-comparison --features tag-comparison-binary` under shared lock; unset `EXIFTOOL`, omit `--exiftool`, and retain the explicit `EXIFTOOL_PERL`/`EXIFTOOL_CACHE_DIR` setup above after proving the pinned script exists. Require the instrument header to name the canonical interpreter/tree. An explicit script override bypasses interpreter selection and invokes its shebang. Use `--samples`, `--output`, `--markdown-dir`, and a fresh authenticated `--baseline`. This harness calls the OxiDex library in-process, so it cannot verify CLI-only behavior. Use unique outputs and caches tied to SHA/oracle/corpus. |
| Comparison tests | `cargo test --release --features exiftool-comparison -- --nocapture` under shared lock; inspect executed/ignored counts and oracle resolution. Self-skipped tests prove nothing. |
| JPEG matrix | Build `--release --features jpeg-tag-matrix-binary --bin jpeg-tag-matrix` under shared lock. Its `EXIFTOOL` accepts a single executable path, not an argv string. Use the explicit wrapper below, `OXIDEX="$PARITY_BIN"`, a unique `TAGMATRIX_WORK`, and recorded fixture/hash. Run `manifest --flag-noops`, `run --workers 2`, then `report --check-baseline`, each under exclusive lock with separate logs. Retain manifest/results JSON, baseline hash and report. |

For the matrix only, create an executable wrapper in the evidence directory
using `apply_patch`, with these exact contents (update paths only after an
explicit canonical-oracle change). Set `EXIFTOOL` to its absolute path and
re-run the version/DOCX probes through it before running the matrix:

```bash
#!/bin/bash
set -euo pipefail
exec /tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2 \
  -I/tmp/oxidex-exiftool-cache/exiftool/lib \
  /tmp/oxidex-exiftool-cache/exiftool/exiftool -config '' "$@"
```

Archive and hash the wrapper. Never point directly at the script's env-Perl
shebang or set `EXIFTOOL` to a multiword command. The matrix's own capability
header probes `combined-samples/OOXML.docx`; require that probe to pass too,
or mark matrix evidence blocked. Some matrix failures appear as warnings;
inspect probe status, manifest scope and raw failed results in addition to
process exits. Manifest-only, limited, filtered, `--skip-write` or cached
`--reread` runs cannot establish full JPEG write coverage. Keep writable,
no-op, readable, successful-write/round-trip, failed and skipped populations
separate. Do not run `report --update-baseline` during release verification.
