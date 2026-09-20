# Authenticated generated-route census

This directory implements `genshare-receipt/v3`. Historical `genshare-probe/1`
patches and `genshare-receipt/v2` summaries are discovery evidence only; they
are not accepted by the validator.

The census measures one maintained OxiDex binary with six silence tokens in
this exact order:

```text
engine,legacy-l1,legacy-l2,producers,serial,keyed
```

It executes maintained-binary controls with the environment absent and
explicitly empty, plus a true pre-seam ordinary-binary control built from the
parent of the commit that introduced `src/exiftool_tables/attribution.rs`.
Each token and the six-token union run separately. The union is never computed
by summing individual losses. A zero-loss token is reported as
`unexercised`, not authenticated coverage. In particular, `keyed` remains
unexercised until a production caller reaches the keyed engine.

## Bounded first probe

The committed manifest contains exactly:

```text
ICC_Profile.icc
AAC.aac
OOXML.docx
```

`testdata/bounded-corpus-expectations.json` binds their reviewed content hashes
and roles. It deliberately contains no exact loss arrays: the first valid probe
is `observed_unreviewed` until the controller and independent reviewer inspect
the raw deltas and source routes. It cannot satisfy `--require-success`.

Run only through the exclusive measurement lock and use a new durable output
directory every time:

```bash
tools/exiftool-tables/genshare/census.sh \
  --repository /absolute/clean/oxidex-worktree \
  --target-dir /absolute/durable/cargo-target \
  --output /absolute/durable/evidence/run-id \
  --corpus /Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples \
  --manifest tools/exiftool-tables/genshare/testdata/bounded-corpus.txt \
  --min-files 3 \
  --min-tags 30 \
  --perl /absolute/perl-with-Archive-Zip \
  --exiftool-dir /Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool \
  --tokens engine,legacy-l1,legacy-l2,producers,serial,keyed
```

The output path must not exist, even if empty. The tool never reuses or deletes
a run and never stages evidence in a system temporary directory. It validates an absolute corpus
root, copies the exact ordered selection into `RUN/selection`, byte-compares and
hashes each copy, and passes only that directory's files to its raw child
driver. Individual manifest paths are never misrepresented as conformance
corpus roots.

## Evidence retained

For every file and every mode, the run retains oracle and candidate argv, cwd,
allow-listed environment state, scrubbed variables, timestamps, timeout/signal/
return code, raw stdout, raw stderr, parsed JSON, and hashes. Duplicate JSON
keys, invalid UTF-8, malformed shape, timeout, signal, or nonzero return fail the
entire run. Selected, oracle-success, candidate-success, and scored path sets
must be identical.

Each corpus child is launched with `Popen` and bound, while unreaped, to its PID
and kernel start identity. Linux records boot ID plus `/proc` start ticks;
macOS records the libproc start timeval. An unsupported query, failed query, or
identity change before output collection fails closed.

The projection imports only the pure comparison rules from the pinned
`conformance.py`; it does not use that wrapper's child runners. Counts are
occurrences: one `missing` `[group, value]` pair is one missing occurrence.
Every file and aggregate must satisfy both Task 6 equations, and each probe must
satisfy both control/probe delta equations. Gains and the independently run
union remain explicit.

Unset/empty inertness compares distinct child runs from the same binary. It
preserves order, typed values, duplicate identity, raw keys, and stderr. The
only normalization replaces the value of the exact key
`System:FileAccessDate`; no other key or value is dropped or rewritten.
The pre-seam control uses the identical staged selection with the environment
absent and must match the maintained unset control in normalized ordered output
and raw stderr. Its introducing commit, one-parent relationship, parent tree,
run-owned detached checkout, isolated target, build logs, and binary are
retained as distinct proof; it is not a maintained-binary self-comparison.

Source commit/tree/clean state, binary content/size/mode/mtime, comparator,
Perl, complete ExifTool `exiftool`+`lib/` manifest, corpus originals, staged
copies, route ledger, contracts, and all raw artifacts are content-bound. A
post-run recheck detects drift.

## Validation and failures

Replay a completed observation:

```bash
python3 tools/exiftool-tables/genshare/attribute.py validate \
  --receipt /absolute/run/receipt.json \
  --recheck-live-inputs
```

The validator recomputes the exact artifact set, hashes, parsed outputs,
projections, path sets, token/mode set, and reconciliation counters. It also
authenticates every PID/start binding and replays the pre-seam build proof and
ordinary-binary equality claim. Use
`--require-success` only for a later controller-reviewed receipt; it correctly
rejects `observed_unreviewed`.

After a run directory is created, every refusal produces
`receipt.failed.json`. It records the terminal stage, error, child-start count,
and identities of artifacts actually retained; success-only fields are null.
Validate its integrity with:

```bash
python3 tools/exiftool-tables/genshare/attribute.py validate-failure \
  --receipt /absolute/run/receipt.failed.json
```

An already-existing output directory is refused without mutation and therefore
cannot receive a failure receipt. Retry with a new run ID.

`probe.patch` is a non-applicable retirement notice. Do not apply it or create a
second attribution implementation.
