# Running the parity instruments

Run Bash examples in order from the clean candidate worktree. Replace all
`/absolute/...` inputs and set corpus floors from the approved corpus manifest
or prior independently authenticated scope, before running. Record commands,
exit codes and stderr even on refusal. These commands are a recipe, not
evidence that a measurement has already run.

## Setup and fail-closed probes

Read the expected ExifTool release from the repository's `.exiftool-version`.
The canonical local interpreter is
`/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2`; the pinned tree is
`/Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool`; its executable is
`/Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool/exiftool` and its library
is `/Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool/lib`. Invoke that
interpreter explicitly with that library and script, with user configuration
excluded. Require
Perl `v5.38.2`, the pinned ExifTool version, working standard/decompression
modules, and `OOXML.docx` reporting **DOCX**, not ZIP. The authenticated
combined corpus is the sibling
`/Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples`, never a child
of the checkout; select it only after `bootstrap_oracle.py verify` has refreshed
its sibling manifest. A version probe alone is insufficient. Export
`EXIFTOOL_PERL` for harnesses that resolve their own oracle.

If `strict.pm` is missing, any probe fails, or the pin differs, record
`status: blocked` and the failed command/exit/stderr. Do not run a sweep or
publish a score. Never fall back to Homebrew, another Perl, a PATH-resolved
oracle, or an allow-skew switch, even if the version string matches. Recovery
requires restoring this canonical Perl 5.38.2 installation with its matching
standard library and required modules, then passing every probe again. Keep
repair work separate from the refused measurement. Do not infer current health
from a historical incident or from a version string; re-probe the selected
installation before each measurement.

```bash
set -euo pipefail
tools/preflight.sh
PARITY_SHA=$(git rev-parse 'HEAD^{commit}')
PARITY_TREE=$(git rev-parse 'HEAD^{tree}')
EVIDENCE_ROOT=/absolute/durable/evidence/root
test -d "$EVIDENCE_ROOT"
PARITY_EVIDENCE=$(mktemp -d "$EVIDENCE_ROOT/parity-${PARITY_SHA}.XXXXXX")
export CARGO_TARGET_DIR=/absolute/dedicated/parity-target
: "${PARITY_LOCK:?Set the absolute lock controller path}"
test -f "$PARITY_LOCK"
PARITY_PIN=$(tr -d '\r\n' < .exiftool-version)
: "${EXIFTOOL_PERL:=/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2}"
: "${EXIFTOOL_CACHE_DIR:=/Users/allen/oxidex-ops/cache/exiftool/$PARITY_PIN}"
PARITY_PERL="$EXIFTOOL_PERL"
PARITY_ET_TREE="$EXIFTOOL_CACHE_DIR/exiftool"
export EXIFTOOL_PERL="$PARITY_PERL"
export EXIFTOOL_CACHE_DIR
unset EXIFTOOL OXIDEX_ALLOW_EXIFTOOL_SKEW OXIDEX_ALLOW_DIRTY_TREE
unset PERL5LIB PERLLIB PERL5OPT
mkdir "$PARITY_EVIDENCE/empty-oracle-home"
export EXIFTOOL_HOME="$PARITY_EVIDENCE/empty-oracle-home"
test ! -e "$PARITY_ET_TREE/.ExifTool_config"
PARITY_ORACLE=("$PARITY_PERL" "-I$PARITY_ET_TREE/lib" "$PARITY_ET_TREE/exiftool" -config '')
printf '%s\n' "$PARITY_SHA" "$PARITY_TREE" > "$PARITY_EVIDENCE/source.txt"
python3 tools/ci/release_oracle.py --repo . --perl "$PARITY_PERL" \
  --exiftool-dir "$PARITY_ET_TREE" --output "$PARITY_EVIDENCE/oracle.json"
```

Stop on any nonzero command, recording that exit and the failing prerequisite
in the receipt. Do not continue a partially executed shell recipe. Record
interpreter/script SHA-256 and the deterministic library-path/file-count/
fingerprint fields emitted by `release_oracle.py`; map those exact fields into
the release receipt rather than reconstructing them by hand. The shared Python
resolver obeys `EXIFTOOL_PERL`; its generic
fallback suggestions are not permission to change the release oracle.
Missing standard-library modules such as `strict.pm` block the oracle; do not
treat a successful version-only probe as capability evidence.
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

Before selecting the canonical combined corpus (currently
`/Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples`), run
`bootstrap_oracle.py verify` with the root and pin below and retain its refreshed
sibling manifest. The combined corpus is a sibling of the ExifTool checkout,
never a child; an unverified or stale manifest blocks measurement.

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
python3 tools/release/bootstrap_oracle.py verify \
  --root /Users/allen/oxidex-ops --pin "$PARITY_PIN" \
  --manifest "$PARITY_EVIDENCE/bootstrap-oracle.json"
shasum -a 256 "$EXIFTOOL_CACHE_DIR/combined-samples.manifest" \
  > "$PARITY_EVIDENCE/combined-samples-manifest.sha256"
PARITY_CORPORA=("$PARITY_ET_TREE/t/images" "$EXIFTOOL_CACHE_DIR/combined-samples")
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
: "${PARITY_READ_MIN_TAGS:?Set the approved native occurrence floor}"
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
jq -e --argjson floor "$PARITY_READ_MIN_TAGS" \
  '.metric_c.native_identity_occurrences >= $floor' \
  "$PARITY_EVIDENCE/observe/receipt.json" \
  > "$PARITY_EVIDENCE/native-occurrence-floor.json"
python3 "$PARITY_LOCK" "$PARITY_EVIDENCE/read-gate.log" -- \
  python3 tools/ci/read_regression_gate.py --receipt "$PARITY_EVIDENCE/observe/receipt.json"
```

The explicit `jq -e` assertion enforces the independently chosen occurrence
floor against verified `metric_c.native_identity_occurrences`; this instrument
has `--min-files` but no `--min-tags` flag. Record the floor and
`native-occurrence-floor.json` in the release receipt. Keep failed
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
| Generated-table verification | Blocked for the current aggregate recipe under the canonical-oracle contract; see the known interpreter/tree mismatch below. Generated tables must be regenerated by their producer, never hand-edited. |
| `tag-comparison` | Build `--release --bin tag-comparison --features tag-comparison-binary` under shared lock; unset `EXIFTOOL`, omit `--exiftool`, and retain the explicit `EXIFTOOL_PERL`/`EXIFTOOL_CACHE_DIR` setup above after proving the pinned script exists. Require the instrument header to name the canonical interpreter/tree. An explicit script override bypasses interpreter selection and invokes its shebang. Use `--samples`, `--output`, `--markdown-dir`, and a fresh authenticated `--baseline`. This harness calls the OxiDex library in-process, so it cannot verify CLI-only behavior. Use unique outputs and caches tied to SHA/oracle/corpus. |
| Comparison tests | `CARGO_PROFILE_RELEASE_PANIC=unwind cargo test --release --features exiftool-comparison -- --nocapture` under shared lock; inspect executed/ignored counts and oracle resolution. Self-skipped tests prove nothing. |
| JPEG matrix | Build `--release --features jpeg-tag-matrix-binary --bin jpeg-tag-matrix` under shared lock. Its `EXIFTOOL` accepts a single executable path, not an argv string. Use the explicit wrapper below, `OXIDEX="$PARITY_BIN"`, a unique `TAGMATRIX_WORK`, and recorded fixture/hash. Run `manifest --flag-noops` and `run --workers 2` under exclusive lock with separate logs, then the isolated report recipe below. Retain manifest/results JSON, baseline hash and reports. |

The comparison-test panic override is scoped to that invocation only, as
required by `Cargo.toml` and the justfile's `unwind` prefix to avoid colliding
test/release library outputs. Pass it through `env` after the lock's `--` when
wrapping the command. Do not export it or apply it to shipped release builds.

Generated-table verification is blocked through the current `just verify-tables`
recipe: it chooses `target/exiftool-src/exiftool-<version>/lib` by default,
and calls `verify_subdirs.py` without its explicit `--perl` argument. That
helper defaults to `/usr/bin/perl` for evaluation and runs the supplied script
through its shebang for capability probes. `EXIFTOOL_PERL` does not control
those paths. Do not invoke the aggregate recipe as canonical release evidence.
Record this instrument as blocked until the recipe's tree and all helper
interpreter paths are repaired and verified against the canonical setup.
Although the helper has `--perl` and executable-wrapper seams, overriding just
one subcommand does not establish that the whole recipe obeys the contract.
The catalog ratchet remains a separate static-artifact check and cannot stand
in for generated-table verification. Repair is outside this skill revision;
do not modify generated tables or suppress the blocked status to pass a gate.

For the matrix only, create an executable wrapper in the evidence directory
using `apply_patch`, substituting the already-probed absolute values of
`PARITY_PERL` and `PARITY_ET_TREE`. Set `EXIFTOOL` to its absolute path and
re-run the version/DOCX probes through it before running the matrix:

```bash
#!/bin/bash
set -euo pipefail
exec /Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2 \
  -I/absolute/path/to/pinned-exiftool/lib \
  /absolute/path/to/pinned-exiftool/exiftool -config '' "$@"
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

### Isolate matrix report writes

`report --check-baseline` still writes both Markdown reports unconditionally.
`TAGMATRIX_WORK` controls inputs only; set `TAGMATRIX_REPO` for the report
process to a new evidence root so it cannot overwrite tracked candidate docs.
Supply the exact candidate baseline because a missing baseline otherwise makes
this instrument skip its check and exit successfully. Keep the report tree
and its baseline/hash with the receipt. Set the proven matrix executable and
the same absolute result directory used by `manifest`/`run` first:

```bash
set -euo pipefail
PARITY_MATRIX_BIN=/absolute/path/to/proven/jpeg-tag-matrix
: "${TAGMATRIX_WORK:?Set the absolute manifest/run evidence directory}"
test -x "$PARITY_MATRIX_BIN"
test -s "$TAGMATRIX_WORK/results.json"
test -s "$TAGMATRIX_WORK/instrument.txt"
test "$(git rev-parse 'HEAD^{commit}')" = "$PARITY_SHA"
test -z "$(git status --porcelain)"
PARITY_REPORT_ROOT=$(mktemp -d "$PARITY_EVIDENCE/matrix-report.XXXXXX")
mkdir -p "$PARITY_REPORT_ROOT/docs/reference"
PARITY_REPORT_BASELINE="$PARITY_REPORT_ROOT/docs/reference/jpeg-tag-baseline.json"
git show "${PARITY_SHA}:docs/reference/jpeg-tag-baseline.json" > "$PARITY_REPORT_BASELINE"
test -s "$PARITY_REPORT_BASELINE"
shasum -a 256 "$PARITY_REPORT_BASELINE" > "$PARITY_EVIDENCE/matrix-baseline.sha256"
PARITY_REPORT_EXIT=0
python3 "$PARITY_LOCK" "$PARITY_EVIDENCE/matrix-report.log" -- \
  env TAGMATRIX_REPO="$PARITY_REPORT_ROOT" TAGMATRIX_WORK="$TAGMATRIX_WORK" \
  "$PARITY_MATRIX_BIN" report --check-baseline || PARITY_REPORT_EXIT=$?
test "$(git rev-parse 'HEAD^{commit}')" = "$PARITY_SHA"
test -z "$(git status --porcelain)"
shasum -a 256 -c "$PARITY_EVIDENCE/matrix-baseline.sha256"
test "$PARITY_REPORT_EXIT" -eq 0
test -s "$PARITY_REPORT_ROOT/docs/reference/jpeg-tag-support.md"
test -s "$PARITY_REPORT_ROOT/docs/reference/jpeg-tag-matrix.md"
```

Check source cleanliness even when the report fails; the captured lock status
retains that failure. Never revert user changes to recover a clean receipt.
If source changed, stop and investigate ownership, then remeasure from a clean
identified candidate. The report directory is retained evidence, not a request
to publish its generated Markdown without the documentation factuality audit.

## Validate the release receipt

Populate the template only from the retained artifacts above, then make the
executable validator the final handoff gate:

```bash
set -euo pipefail
PARITY_RELEASE_RECEIPT=/absolute/path/to/release-parity-receipt.json
: "${VERSION:?Set the release version without a v prefix}"
python3 tools/ci/validate_release_receipt.py --kind parity \
  --receipt "$PARITY_RELEASE_RECEIPT" --version "$VERSION" \
  --candidate-sha "$PARITY_SHA"
```

A hand-edited or structurally plausible JSON file is not a verified receipt
until this command succeeds. Both identity arguments are mandatory outside
template mode. Preserve validator stdout/stderr and exit status.
