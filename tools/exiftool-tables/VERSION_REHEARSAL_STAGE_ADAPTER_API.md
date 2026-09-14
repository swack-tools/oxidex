# Version rehearsal stage adapter

`version_rehearsal_stage_adapter.py` is the concrete non-promoting command
adapter for `version_rehearsal_executor.py`.  The executor still selects and
materializes releases; this adapter never selects a pair, checks out a branch,
or promotes a result.

Each command receives the executor's owned checkout, an isolated target and
the selected release's materialized native source.  It requires an immutable
`--source-commit`, explicitly sets `EXIFTOOL_PERL`, `OXIDEX_EXIFTOOL_LIB`,
`OXIDEX_ET_CACHE` and `CARGO_TARGET_DIR`, and refuses a native library whose
`Image/ExifTool.pm` version differs from the selected release.

The generate command requires a clean checkout, changes only that checkout's
`.exiftool-version`, then invokes exactly:

```text
bash <checkout>/tools/exiftool-tables/regen-all.sh
```

It records every path in the committed `artifacts.py` manifest with its
post-generation hash. Build uses `cargo build --message-format=json --bin
oxidex` and accepts only the executable announced by Cargo JSON under the
isolated target. Read copies an immutable, hash-verified fixture manifest into
the isolated target and invokes the checkout's `conformance.py` against that
release's selected native source and the Cargo-announced binary. A nonzero
VALUE, MISSING, RENAME or EXTRA count produces a `failed` report; it never
becomes a parity pass.

A fixture manifest is an immutable JSON object of this form:

```json
{
  "schema": 1,
  "kind": "oxidex_version_rehearsal_fixture_manifest",
  "fixtures": [{"path": "/absolute/sample.jpg", "sha256": "...", "bytes": 123}]
}
```

The adapter writes full stdout/stderr command records beside the stage report.
Its result binds the execution source commit/tree, native identity, generated
artifact hashes, Cargo binary identity, fixture identities and raw reports.
The executor re-hashes these files before it accepts a stage.

Use command entries equivalent to the following, with a separately prepared,
immutable fixture-manifest path substituted by the rehearsal owner:

```text
python3 tools/exiftool-tables/version_rehearsal_stage_adapter.py generate
  --checkout {checkout} --target {target} --report {report} --release {release}
  --source-commit {source_commit} --native-source {native_source}
  --native-lib {native_lib} --native-perl {native_perl}
```

`build` takes the same arguments. `read` additionally takes
`--fixture-manifest /absolute/manifest.json --native-probe-sha256
{native_probe_sha256}`.
The executor exposes `{source_commit}` and the equivalent
`OXIDEX_REHEARSAL_SOURCE_COMMIT` environment value for this purpose.

`write` always writes an `unsupported` result and exits nonzero. It does not
run a legacy 13.59 writer, infer a writer result from a read result, or create
a public writer route. A generated writer acceptance contract is still needed
before write can be configured in a rehearsal.
