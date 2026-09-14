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
environment variables.

The example commands are wrappers: a raw `cargo build` does not itself write
the required stage result. `native_cases` use the constrained case schema from
`version_rehearsal_native_oracle.py` and are bound per release.

Wrappers remain responsible for proving their own inputs: they must pin the
checkout's `.exiftool-version`, pass the selected native Perl and materialized
library to generation, and record the generated binary plus fixture-source
identity in their result. The executor supplies those bindings and checks
release-specific native readiness; it cannot infer them from a build exit code.

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
The native stage itself calls `version_rehearsal_native_oracle` with the
release's own explicit Perl, materialized `lib`, and program before either
comparison command can run.

One explicit absolute host lock from immutable config covers checkout, native probe, generation, build and
both comparisons. An interruption leaves the active stage `running`; `recover`
changes it to `interrupted` and records the unknown completion state. The run is
terminal afterward, so the executor never reselects or duplicates an
interrupted action. Pair-level native-delta classification remains explicitly
`unexercised`; per-version comparison does not establish cross-version equality.
