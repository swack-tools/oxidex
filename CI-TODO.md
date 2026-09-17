# CI TODO

Status of `.github/workflows/ci.yml` as of 2026-09-17 (after #786), and options
for the open problems. Working rules for the current system live in
`AGENTS.md` → "CI".

## How CI works today

- **Runners.** All workflows use free standard GitHub-hosted runners. They
  moved off WarpBuild, which billed about $175 for August 2026. A standard Linux
  runner has 4 vCPU and 16 GB.
- **Checks.** `ci.yml` comments call `Lint & Audit` and `Build & Test`
  required status checks. As of 2026-09-17 no ruleset requires any check: the
  rulesets only forbid deletion and force-push, and `main` also requires signed,
  squash-merged PRs. So a red PR can be merged, and green CI is enforced only by
  the merge checklist in `AGENTS.md`.
- **Verify Generated Tables** is a hand-written fan-out:
  - `capture` runs the native ExifTool captures concurrently and uploads them.
    The full dump is 1.25 GB of JSON, 16 MB with zstd.
  - `source and tier 2` needs only the ExifTool tree.
  - `catalog join`, `staleness and drift` and eight `tools` unittest shards
    run in parallel on the capture.
  - A final job with the old check name fails unless every part succeeded.
- **Measured on run 35185531767 (PR #786 final head).** 18.6 min wall for the
  whole CI run. The capture takes 11 min and is now the long pole; the catalog
  join takes 7.5 min and the shards 3.5–7 min each. For comparison, the same
  job took ~38 min serially on WarpBuild 4x, and would take ~70 min serially on
  a standard runner, over its 60-min cap.
- **Sharding.** `tools/ci/unittest_shard.py` assigns every discovered test id
  to exactly one shard, balanced by `tools/ci/unittest_weights.json`: per-test
  seconds taken from one CI run's logs and committed by hand.

## Problem

Almost every PR during the generated-table work changes what Verify Generated
Tables must check, and each such change must also be made in the workflow
YAML:

1. **Commands are spelled in YAML.** New join inputs (#785), a new published
   receipt (#788) and a new fixture exclusion (#789) each edited `ci.yml`. Three
   merged PRs changing the same YAML block while #786 was open produced merge
   conflicts that had to be re-applied by hand into the new job layout.
2. **Parallelism is hand-tuned.** The capture set, the job graph, the shard count
   (8) and the weights are all decided in YAML or committed files. When the
   suite grows, or one test becomes slow, shards drift out of balance until
   someone re-measures. That already happened twice in #786: one 476 s test,
   then one 548 s test, each pinned a shard.
3. **Contributors must think about CI topology.** Adding a generator means
   knowing which job needs which capture, how the capture is compressed and
   restored, and where exclusions go.
4. **Duplicate recipes.** The same sequences exist in `regen.sh`,
   `just check-staleness` and `ci.yml`, and can disagree. `ci.yml`'s copy is the
   only one CI proves.
5. **Loose ends.**
   - No check is required by branch rules, so CI failures do not block merges.
   - `.github/actionlint.yaml` still lists `warp-*` runner labels.
   - GitHub sometimes does not fire `pull_request` for a push; the manual
     `workflow_dispatch` is the workaround.
   - `Build & Test` is ~6.5 min warm on 4 vCPU versus ~3 min on WarpBuild 8x.
   - `Deploy Documentation`, `Release` and `Bump ExifTool` have not yet run on
     the new runners: they need `main`, a tag, or a default-branch workflow.

## Goal

A PR that adds or changes a generated family touches only its generator, its
tests and a manifest entry. It never touches `ci.yml`, shard counts or weights,
and CI still runs every check exactly once, in parallel.

## Candidate solutions

### A. One repo entry point per stage (removes problems 1, 3 and 4)

Move every command into `tools/ci/verify_tables.py <stage>` (`capture`,
`source`, `catalog`, `drift`, `tools --shard k/N`, `aggregate`). Workflow steps
become one line each and never name a table.

- Derive the join's inputs, the receipt list and the drift exclusions from
  committed data: extend `tools/exiftool-tables/artifacts.py`'s manifest
  (66 artifacts with producers) with `check` and `inputs` fields.
- Have `regen.sh` and `just check-staleness` call the same stages, so local and
  CI recipes cannot diverge.
- A unit test fails when a committed ledger, receipt or fixture is not wired to
  a check, so a forgotten input is caught locally before CI.

Cost: one medium PR moving the current behavior. Prove equivalence by
comparing the set of executed test ids and checks between the old and new jobs.

### B. Planned, self-sizing matrix (removes problem 2)

Add a fast `plan` job. It discovers every test id plus every manifest check,
weighs them, chooses the shard count from a per-shard time target (~6 min),
and emits JSON. The workflow consumes it with
`strategy.matrix: ${{ fromJSON(needs.plan.outputs.matrix) }}`.

- **Timings refresh automatically.** Each shard uploads per-test durations; a
  run on `refactor/tag-machinery` saves the merged timings to the Actions
  cache, and `plan` restores the latest. Nothing is committed; the committed
  weights file becomes only a fallback.
- **Correctness is independent of timings.** Every id is assigned exactly once,
  and the aggregate job fails unless the union of executed ids equals the
  planned set. A stale weight can only unbalance shards.
- **Guard long tests.** Report any single test over a threshold (e.g. 180 s) in
  the job summary so it can be split into per-case tests.

### C. Shrink the capture long pole

- **Cache the capture.** Key it by the pinned ExifTool version plus the hashes
  of the capture scripts. Most PRs change neither, so they would restore the
  dump in seconds instead of spending ~11 min. Tradeoff: a cache hit is no
  longer a fresh native capture on that runner. Keep one fresh capture on
  pushes to `refactor/tag-machinery` and whenever the key inputs change.
- **Capture per module group, then merge.** This makes the dump parallel, but
  it changes the dump's bytes, which expression and IFD ledgers bind by digest.
  It only works if those bindings move to a canonical form first.

### D. Other options

- **Merge queue** on `refactor/tag-machinery`: tests the merge result and
  removes manual rebase/re-run cycles when the tip moves.
- **`Build & Test`**: split `cargo nextest` with `--partition` over an archive
  built once (`cargo nextest archive`), keeping a final job named `Build & Test`
  for the required check.
- **Require checks on `refactor/tag-machinery`** (maintainer decision): add a
  ruleset `required_status_checks` rule for `Lint & Audit`, `Build & Test` and
  `Verify Generated Tables`. The aggregate job keeps that last name stable
  however the fan-out changes.
- **Housekeeping**: remove `warp-*` labels from `.github/actionlint.yaml`.
  Verify `Deploy Documentation` and `Release` on their first real runs after
  `main` adopts the runner change.

## Suggested order

1. **A** first: removes YAML edits from table PRs, with no behavior change.
2. **B**: removes shard tuning.
3. **C (cache)**, if the ~11 min capture still dominates.
4. Housekeeping items as they come up.
