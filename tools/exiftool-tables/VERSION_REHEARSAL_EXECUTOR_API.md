# Version rehearsal executor

`version_rehearsal_executor.py` executes an already-created plan. It does not
capture tags, select a pair, fetch archives, alter the OxiDex pin, or promote a
result. `init` copies the exact capture, catalog, plan, source-resolution and
materialization documents into a new run directory. `execute` revalidates those
documents and the materialized trees before it creates any checkout or command.

The config supplied to `init` is JSON with this shape:

```json
{
  "schema": 1,
  "host_lock": "/absolute/shared/oxidex-version-rehearsal.lock",
  "execution_source_commit": "full OxiDex commit object id",
  "perls": { "11.78": "/absolute/perl", "12.64": "/absolute/perl" },
  "native_cases": { "11.78": [{ "name": "case", "fixture": "/fixture.jpg", "read": { "query": "FileType", "expectation": "value", "value": "JPEG" }, "write": { "operation": "set", "tag": "Comment", "value": "probe", "readback": "probe" } }], "12.64": [{ "name": "case", "fixture": "/fixture.jpg", "read": { "query": "FileType", "expectation": "value", "value": "JPEG" }, "write": { "operation": "set", "tag": "Comment", "value": "probe", "readback": "probe" } }] },
  "write_fixture_manifests": { "11.78": "/absolute/write-fixtures.json", "12.64": "/absolute/write-fixtures.json" },
  "commands": {
    "generate": { "argv": ["python3", "generator.py", "{native_source}", "{report}"] },
    "build": { "argv": ["python3", "build-release.py", "--report", "{report}"] },
    "read": { "argv": ["python3", "read-compare.py", "{native_probe}", "{report}"] },
    "write": { "argv": ["python3", "write-acceptance.py", "{native_probe}", "{report}"] }
  }
}
```

`generate`, `build`, and `read` are required. `write` is optional only so an
unimplemented generated writer remains a visible `unsupported` state; its
absence cannot yield parity. Each selected release gets a private Git worktree
at `execution_source_commit` and a separate `CARGO_TARGET_DIR` under the run
directory. This source commit is immutable config evidence; it is deliberately
separate from the preserved release-selection plan commit and the executor
checks the actual checkout `HEAD`. The runner supplies `{release}`, `{checkout}`, `{target}`,
`{report}`, `{native_source}`, `{native_lib}`, `{native_program}`,
`{native_perl}`, and `{native_probe}` and equivalent `OXIDEX_REHEARSAL_*`
environment variables. `{native_probe_sha256}`, `{source_commit}` and
`OXIDEX_REHEARSAL_SOURCE_COMMIT` bind wrappers to the immutable OxiDex source.

The example commands are wrappers: a raw `cargo build` does not itself write
the required stage result. `native_cases` use the constrained case schema from
`version_rehearsal_native_oracle.py` and are bound per release.

Wrappers remain responsible for proving their own inputs: they must pin the
checkout's `.exiftool-version`, pass the selected native Perl and materialized
library to generation, and record the generated binary plus fixture-source
identity in their result. The executor supplies those bindings and checks
release-specific native readiness; it cannot infer them from a build exit code.
The concrete adapter is documented in `VERSION_REHEARSAL_STAGE_ADAPTER_API.md`.
It invokes the sanctioned `regen-all.sh`, consumes Cargo JSON, and drives the
actual selected-release `conformance.py` oracle.

Every command must write its own result JSON at `{report}`. The common fields
are `schema: 1`, `kind: "oxidex_version_rehearsal_stage_result"`, `stage`,
`release`, `state: "passed"`, and a positive integer `denominator`. Read and
write additionally require the actual selected `native_release`, the native
probe digest, and:

```json
"comparison": {
  "kind": "oxidex_vs_native",
  "native_release": "11.78",
  "matched": 123,
  "mismatched": 4
}
```

`matched + mismatched` must equal `denominator`, and `mismatched` must be zero
for a passed result. A return code of zero, a build report, or a native
readiness report is not a read/write comparison result.
For a wrapper result to be accepted, it also binds `source_commit` and the
current full source-tree digest, gives hashes for every sanctioned generated
artifact, retains a hash-verified raw command report, and (for build/read)
gives an isolated binary identity. Read additionally gives a hash-verified
immutable fixture manifest and every source fixture identity. The executor
re-hashes each proof and freezes all non-generated source entries, including
untracked files, before and after every command. Only `.exiftool-version` and
the committed `artifacts.py` manifest are allowed to differ during generation;
later stages may not change source entries.
The native stage itself calls `version_rehearsal_native_oracle` with the
release's own explicit Perl, materialized `lib`, and program before either
comparison command can run.

When `write` is configured, `write_fixture_manifests` is required for every
selected release. `init` captures each absolute manifest's content hash, byte
count, and every listed JPEG source identity into immutable config. Loading or
executing a run rechecks that binding, so changing a manifest or a source JPEG
after initialization refuses before the stage starts. The runner provides
`{write_fixture_manifest}` and `OXIDEX_REHEARSAL_WRITE_FIXTURE_MANIFEST` to
the configured write command. A write result must prove the build's
`writer_binary` and its distinct staged fixture corpus; the executor rehashes
both and rejects a substituted reader CLI, read fixture manifest, or writer
driver.

One explicit absolute host lock from immutable config covers checkout, native probe, generation, build and
both comparisons. An interruption leaves the active stage `running`; `recover`
changes it to `interrupted` and records the unknown completion state. The run is
terminal afterward, so the executor never reselects or duplicates an
interrupted action. Pair-level native-delta classification remains explicitly
`unexercised`; per-version comparison does not establish cross-version equality.
