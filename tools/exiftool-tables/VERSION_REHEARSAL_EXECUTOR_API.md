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
  "read_fixture_manifests": { "11.78": "/absolute/read-fixtures.json", "12.64": "/absolute/read-fixtures.json" },
  "write_fixture_manifests": { "11.78": "/absolute/write-fixtures.json", "12.64": "/absolute/write-fixtures.json" },
  "commands": {
    "generate": { "argv": ["python3", "generator.py", "{native_source}", "{report}"] },
    "build": { "argv": ["python3", "build-release.py", "--report", "{report}"] },
    "read": { "argv": ["python3", "read-compare.py", "{native_probe}", "{report}", "--fixture-manifest", "{read_fixture_manifest}"] },
    "write": { "argv": ["python3", "write-acceptance.py", "{native_probe}", "{report}"] }
  }
}
```

`generate`, `build`, and `read` are required. `write` is optional only so an
unimplemented generated writer remains a visible `unsupported` state; its
absence cannot yield parity. `test` (run after `build`, before `read`) runs
the regenerated checkout's own test suite; when it is absent the stage is
`unsupported` and `scope.release_tests` says so. A test result passes only
with a `test_suite` whose totals count at least one passed test, zero
failures and every command exiting 0. Each selected release gets a private Git worktree
at `execution_source_commit` and a separate `CARGO_TARGET_DIR` under the run
directory. This source commit is immutable config evidence; it is deliberately
separate from the preserved release-selection plan commit and the executor
checks the actual checkout `HEAD`. The runner supplies `{release}`, `{checkout}`, `{target}`,
`{report}`, `{native_source}`, `{native_lib}`, `{native_program}`,
`{native_perl}`, `{native_probe}`, and `{read_fixture_manifest}` and equivalent
`OXIDEX_REHEARSAL_*` environment variables, including
`OXIDEX_REHEARSAL_READ_FIXTURE_MANIFEST`. `{native_probe_sha256}`, `{source_commit}` and
`OXIDEX_REHEARSAL_SOURCE_COMMIT` bind wrappers to the immutable OxiDex source.

The stage adapter's `test` subcommand runs, in the owned checkout with its
own `CARGO_TARGET_DIR` (`<target>/test-suite`, so the build's proven CLI and
writer driver are never replaced), exactly one invocation:

```bash
cargo test --workspace --all-features --no-fail-fast
```

It is one invocation on purpose, matching CI's required test step: a
separate `--doc` run self-heals a mid-run lib rebuild that only the combined
command exposes (see the doc-test step in `.github/workflows/ci.yml`).

The suite runs from an allowlisted environment, not the caller's: only
`PATH`, `HOME`, `USER`, `LOGNAME`, `TMPDIR`, locale, `CARGO_HOME`,
`RUSTUP_HOME`, `SDKROOT` and `DEVELOPER_DIR` pass through, so an ambient
`EXIFTOOL`, `EXIFTOOL_CACHE_DIR`, `OXIDEX_ALLOW_EXIFTOOL_SKEW`, `RUSTFLAGS`,
`CARGO_TERM_QUIET`, `CARGO_TARGET_*_RUNNER` or compiler wrapper cannot reach
it. The adapter then sets `EXIFTOOL_CACHE_DIR` to
`<target>/test-suite/exiftool-oracle`, whose `exiftool` entry is the side's
selected native source, `EXIFTOOL_PERL` to the selected Perl, and
`OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES=1`, and prepends a `bin/exiftool`
shim that runs the same tree under the same Perl. Before any test runs it
probes that oracle in that environment (`-ver`, the tree's `OOXML.docx`
FileType, and `strict`/`warnings`/`Archive::Zip`/`Compress::Zlib`) and
refuses unless it reports the selected release, `DOCX` and every module, from
the selected tree, library and Perl. The probe result and the exact
environment are recorded.

The merged stdout/stderr stream is parsed strictly, per target announced by
cargo. A test binary is read at its boundaries (its first `running N tests`
line and its last summary), because tests that re-execute their own binary
interleave nested, filtered runs; a doc-test target may hold several blocks
(edition 2024 prints a merged and a standalone block), and each is parsed
and summed. Every counted summary must be unfiltered, account for its N and
agree with its status, and the exit status must agree with the failure
count. Anything else refuses. The result records the command, exit,
duration, passed/failed/ignored/measured/filtered-out/target counts, the ExifTool
oracle, the environment, and the raw output log path and hash; failures
produce a `failed` result, never a pass.

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
both comparisons. Standalone `execute` and `recover` acquire it themselves;
the in-process transition wrapper borrows only a live capability issued by
its owning lease. An arbitrary matching file descriptor is refused because
probing it with `flock` could acquire a previously unlocked descriptor.
Stage children inherit the lock's open file description (`close_fds=False`).
Because `flock` is per description, an owner's `LOCK_UN` would release the lock
for a still-live child as well, so the owner unlocks only after every child it
spawned has been reaped and its process group is empty. Otherwise the lock
fails closed: no unlock, the descriptor is retained, and the owner exits
non-zero naming each surviving PID (`LockRetained`; the transition wrapper
reports it as `LeaseRetained`, exit 5). The lock then frees only when the last
holder of the description exits. Descendants that leave their process group
are not tracked.
An interruption leaves the active stage `running`; `recover`
changes it to `interrupted` and records the unknown completion state. The run is
terminal afterward, so the executor never reselects or duplicates an
interrupted action. Pair-level native-delta classification remains explicitly
`unexercised`; per-version comparison does not establish cross-version equality.
