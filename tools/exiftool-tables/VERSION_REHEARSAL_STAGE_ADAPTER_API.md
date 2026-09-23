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
post-generation hash. Build produces both the reader CLI and the public-writer
test driver with explicit all-feature Cargo commands:

```text
cargo build --all-features --message-format=json --bin oxidex
cargo test --lib --all-features --no-run --message-format=json
```

It accepts exactly one executable per intended target kind and test profile,
from this checkout's Cargo manifest, inside the isolated target directory.
The two identities are recorded as `binary` and `writer_binary`; the executor
rechecks their hashes. Neither a CLI executable substituted for the test driver
nor a failed second compilation can produce a passing build report. The build
denominator of two counts executables, not tests or tag coverage; the
`--no-run` compile proves only that the driver builds. `test` then actually
runs the checkout's suite, `cargo test --workspace --all-features
--no-fail-fast --tests` followed by `--doc`, in `<target>/test-suite`, parses
every target's libtest summary strictly, and reports `passed` only with zero
failures (see `VERSION_REHEARSAL_EXECUTOR_API.md`). Read copies an immutable, hash-verified fixture manifest into
the isolated target and invokes the checkout's `conformance.py` against that
release's selected native source and the Cargo-announced binary. A nonzero
VALUE, MISSING, RENAME or EXTRA count produces a `failed` report; it never
becomes a parity pass.

A fixture manifest is an immutable JSON object of this form:

```json
{
  "schema": 1,
  "kind": "oxidex_version_rehearsal_fixture_manifest",
  "fixtures": [{"path": "fixtures/sample.jpg", "sha256": "...", "bytes": 123}]
}
```

The adapter writes full stdout/stderr command records beside the stage report.
Its result binds the execution source commit/tree, native identity, generated
artifact hashes, Cargo binary identity, fixture identities and raw reports.
The executor re-hashes these files before it accepts a stage.

Adapter subprocesses remain in the executor-owned process group and inherit its
host-lock descriptor. An executor timeout therefore terminates the adapter and
its regeneration/build/comparison child together; the child cannot outlive the
supervisor and retain the shared host lock. The generated corpus is copied into
the isolated target, re-hashed before and after `conformance.py`, and the
report must contain exactly one `per_file` entry for every staged fixture.

Use command entries equivalent to the following, with a separately prepared,
immutable fixture-manifest path substituted by the rehearsal owner:

```text
python3 tools/exiftool-tables/version_rehearsal_stage_adapter.py generate
  --checkout {checkout} --target {target} --report {report} --release {release}
  --source-commit {source_commit} --native-source {native_source}
  --native-lib {native_lib} --native-perl {native_perl}
```

`build` and `test` take the same arguments. `read` additionally takes
`--fixture-manifest fixtures/manifest.json --native-probe-sha256
{native_probe_sha256}`.
The executor exposes `{source_commit}` and the equivalent
`OXIDEX_REHEARSAL_SOURCE_COMMIT` environment value for this purpose.

`write` is an actual, bounded selected-release acceptance stage. It requires
the build report's separately proven `writer_binary`, rechecks every generated
artifact and source identity, stages a dedicated immutable JPEG fixture
manifest under `rehearsal-write-fixtures/`, and invokes
`generated_tiff_write_matrix.py --route public-api` once per staged JPEG. The
matrix also exercises its existing synthetic little- and big-endian TIFF
carriers. It does not reuse or overwrite the read corpus.

Historical write mode is opt-in: it binds the selected release to the owned
checkout pin, the selected native identity, and the regenerated
`tiff_scalar_final_ledger.json` version. The normal matrix still defaults to
its independent reviewed 13.59 contract. A write report passes only with a
positive real matrix denominator and zero mismatches. The declared cohort must
exactly match both generated ledger and Rust operands, and every carrier,
qualified name and operation must appear exactly once. A nonzero matrix
process cannot pass by leaving behind an all-pass JSON report. Source files,
generated operands, native writer sources, fixture bytes and the writer
executable are rechecked after execution. Its scope is limited to
the emitted TIFF/JPEG scalar cohort; fresh/empty EXIF, other writer grammars,
and non-JPEG formats remain explicitly unexercised.

Write fixtures use a separate manifest kind:

```json
{
  "schema": 1,
  "kind": "oxidex_version_rehearsal_write_fixture_manifest",
  "fixtures": [{"path": "fixtures/input.jpg", "sha256": "...", "bytes": 123}]
}
```

Every requested fixture must be a nonempty JPEG and match its hash before and
after the matrix. A stale, changed, missing, or non-JPEG write fixture refuses
the stage; it is never silently skipped.
