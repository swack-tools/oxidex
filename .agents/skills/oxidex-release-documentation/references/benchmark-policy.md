# Benchmark provenance and release claims

A current speed claim requires a measurement of the **exact candidate commit**.
Classify each benchmark independently; never substitute one instrument for
another because both produce durations.

| Kind | Repository instrument | Permitted interpretation |
| --- | --- | --- |
| Shipped-profile hyperfine | `benches/exiftool_comparison.sh`, `benches/instrument_check.py`, `[profile.release]` in `Cargo.toml` | CLI end-to-end measurements on the named host/corpus; distinguish process startup, per-core and parallel work |
| Indicative CI | `.github/workflows/benchmarks.yml`, artifact `benchmark-comparison` | Shared-runner hyperfine evidence; compare like workflow/profile/hardware, label indicative; non-blocking CI is not a regression gate |
| Criterion | `ci.yml` metrics job, artifact `benchmark-results`, `target/criterion` | In-process microbenchmarks with the actual bench overrides; not shipped CLI speed or ExifTool parity |

Refresh these facts from candidate workflow/script files. Currently CI Criterion
uses `--quick`, disabled LTO and 16 codegen units; the shipped release profile
uses LTO and one codegen unit. Record actual environment overrides and binary
hashes, not merely profile names. For hyperfine against ExifTool require the
repository pin, explicit interpreter, version and DOCX capability probes,
warmups/runs, raw JSON/logs, corpus identity/file count, hardware/OS/toolchain,
load, core/thread count and exclusive heavy-job-lock evidence.

For every advertised result, add a `benchmarks` row with `kind`, `instrument`,
`measured_sha`, `candidate_sha`, `run_id`, `artifact_id`, `artifact_sha256`,
`profile`, `machine`, `corpus`, `oracle`, `measured_at`, `classification`,
`claim_indexes`, `status`, and `evidence_path`. For local measurements use null
run/artifact IDs with a hashed local artifact and explicit local provenance.
Validate downloaded files, not just an artifact lookup or successful CI job.

`deploy-docs.yml` prefers same-SHA Criterion output, then falls back to an older
successful `main` run. A download can fail while the deployment continues and
keeps committed numbers. `tools/docs-local-deploy.sh --bench auto` can select
other history. Neither proves exact-candidate results. Select an explicit run
ID and inspect it before building:

```bash
set -euo pipefail
gh run view "$BENCH_RUN_ID" -R swack-tools/oxidex \
  --json databaseId,headSha,headBranch,workflowName,status,conclusion,url \
  > "$EVIDENCE_DIR/benchmark-run.json"
gh api "repos/swack-tools/oxidex/actions/runs/$BENCH_RUN_ID/artifacts" \
  > "$EVIDENCE_DIR/benchmark-artifacts.json"
```

Resolve all variables from frozen inputs first. Require the expected workflow,
successful completed run, unexpired artifact, measured SHA and artifact hash.
If there is no candidate measurement, choose an explicit disposition:

1. Measure the candidate under the correct instrument and lock, or obtain its
   CI artifact if the workflow actually produced one.
2. Retain older results only if the rendered page says which commit produced
   them, visibly labels them **historical**, and the release summary does not
   attribute them to the candidate. Record candidate performance as unmeasured.
3. Remove the unsupported speed claim and record the absence. Missing required
   evidence remains a blocker; do not report an empty performance page as a
   passed benchmark gate.

Build with the exact disposition that will be published. If a historical run
is selected, record its explicit ID instead of pretending it is a candidate
run. Check the generated HTML and actual data: the helper's metadata stamping
uses substitution, and changing a commit/date line does not update tables or
add a historical warning. Deployment date is not measurement date. Reconcile
every number with its own source even when multiple instruments share a page.
