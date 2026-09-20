# Generated Runtime Release-Functional Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> `superpowers:subagent-driven-development` for reviewed tasks and
> `superpowers:dispatching-parallel-agents` only for tasks whose file leases and
> dependencies are disjoint. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the generated metadata runtime, retire only proven-replaced
manual code, and prove repeatable ExifTool version transitions before
`v2.0.0-beta.1` promotion work begins.

**Architecture:** One controller preserves each task in a local named worktree
and a remote draft PR, then squash-merges reviewed, green PRs into its
single-writer `staging/beta1-functional-integration` branch and fast-forwards
the integration mirror. A final requalified whole-branch PR lands into
`refactor/tag-machinery`. Up to three Desktop subagents and six
`codex --yolo exec` workers operate concurrently when their file leases do not
overlap. Contract and engine work is serialized; vendor and container adapters
fan out after those interfaces are frozen.

**Tech Stack:** Rust, Python 3, Perl 5.38.2, ExifTool 13.59 and fixed rehearsal
releases 11.78/12.64, Cargo, `uv`, `just`, Git worktrees, Codex Desktop
subagents, Codex CLI fast mode.

**Spec:**
[`docs/superpowers/specs/2026-09-19-generated-runtime-release-functional-design.md`](../specs/2026-09-19-generated-runtime-release-functional-design.md)

## Global Constraints

- Never edit `main` or `refactor/tag-machinery` directly.
- One task owns one branch, one named worktree, one absolute
  `CARGO_TARGET_DIR`, one file lease, and one root `HANDOFF.md`.
- A file lease is a literal path or an explicitly bounded directory glob in
  the task's Files section. Words such as `relevant`, `related`, `selected`,
  `minimum`, `only when required`, `family files`, `registries`, `dispatch`,
  and `source-required` do not grant an edit lease. Resolve them to literal
  paths in the materialized PRD before dispatch or do not launch the task.
- Use
  `/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2` and
  `/Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool`; never invoke bare
  `exiftool`.
- Never place worktrees, Cargo targets, source caches, corpora, toolchains,
  receipts, logs, handoffs, or recovery state under an ephemeral temporary
  directory. Release paths must be descendants of `/Users/allen/git` or
  `/Users/allen/oxidex-ops`.
- Before any oracle measurement, require `-ver == 13.59` and
  `OOXML.docx -> FileType: DOCX`.
- Never approximate a conversion. Derive it from the selected ExifTool source
  or refuse it with a machine-readable reason.
- Use
  `python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared`
  for Cargo builds, tests, and Clippy. Use the same wrapper without `--shared`
  for corpus, read/write, transition, and timing gates.
- Every agent and CLI worker uses fast mode. When two models are adequate,
  choose the cheaper model.
- CLI launches use `codex --yolo exec`; the task PRD still prohibits worker
  pushes, destructive actions, shared-branch edits, and edits outside its
  lease. Only the controller performs authenticated remote operations.
- Workers update their worktree `HANDOFF.md` after every state transition and
  at least every 15 minutes. The controller updates the integration handoff and
  fleet ledger after every dispatch, result, review, fix, merge, gate, or
  blocker.
- Workers create frequent local checkpoint commits but never merge or push.
  The controller pushes checkpoint branches, maintains draft PRs, and
  squash-merges only reviewed, green tasks.
- Generated outputs, central registries, `.exiftool-version`, workspace
  manifests, lockfiles, and shared ledgers have one owner at a time.
- Generated declarations, generated accepted/refused rows, runtime-reachable
  routes, observed reads, writes, and conformance remain separate metrics.

## Review Focus

1. A generated `Decline` after session writes must not cause the residual path
   to apply the writes twice; Task 9 adds the rollback/commit test.
2. Non-UTF-8 and binary values must survive stored, typed, and printed channels;
   Tasks 2 and 7 add byte-exact tests.
3. Duplicate order, groups, requested-tag behavior, suppression, and instance
   identity must survive consumer and engine migration; Tasks 3, 9, and 11 pin
   these behaviors.
4. Same-pin and older-release runs must not reuse stale 13.59 artifacts or hand
   behavior; Tasks 5, 18, and 19 prove fresh native expectations.
5. Generated-on/off attribution must bind raw outputs, binary hashes, process
   results, corpus identity, and occurrence deltas; Task 8 makes those controls
   mandatory before Task 17 deletes code.

## Controller Workspace and Recovery Contract

This plan/specification branch must first be pushed, reviewed, and
squash-merged into `refactor/tag-machinery`. No implementation task starts
from an unmerged local-only copy of the plan.

After that PR lands, create the controller worktree from the remote target:

```bash
git fetch origin refactor/tag-machinery main
git worktree add -b staging/beta1-functional-integration \
  /Users/allen/git/oxidex-beta1-functional-integration \
  origin/refactor/tag-machinery
cd /Users/allen/git/oxidex-beta1-functional-integration
tools/preflight.sh
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/refactor/tag-machinery)"
GIT_SSH_COMMAND="ssh -o IdentityAgent=none -o IdentitiesOnly=yes -i /Users/allen/.ssh/id_es25519_swackhamer" \
  git push -u origin staging/beta1-functional-integration
test "$(git rev-parse HEAD)" = \
  "$(git ls-remote origin refs/heads/staging/beta1-functional-integration | cut -f1)"
```

Run those commands from the new integration worktree. Do not mask any
`preflight.sh` exit. The explicit SHA equality records the intentional
`origin/main` divergence without weakening protected-branch, dirty-tree,
fetch, or target-freshness checks. The remote
`staging/beta1-functional-integration` branch is controller-owned: task PRs
target it, only the controller merges into it, and no worker may push it.

In the integration worktree, initialize the Superpowers workspace and controller
handoff:

```bash
bash /Users/allen/.codex/plugins/cache/openai-curated-remote/superpowers/6.4.1/skills/subagent-driven-development/scripts/sdd-workspace \
  docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md
tools/preflight.sh
```

The controller records the resolved plan workspace printed by
`sdd-workspace`. Its short-lived `progress.md` begins with the plan path,
integration branch/worktree, and the literal output of `git rev-parse HEAD`.
Do not copy a symbolic ref where the resolved SHA belongs.

The durable controller root is:

```text
/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/
```

It contains `fleet-state.json`, append-only `fleet-events.jsonl`,
`receipt-index.json`, `prds/`, `reports/`, `reviews/`, and `processes/`. The
ignored Superpowers workspace is a working cache, never the sole recovery
record. Before dispatch and after every worker message, process exit, signal,
checkpoint, push, PR/CI transition, merge, ruling, or blocker, the controller
atomically advances the durable snapshot and appends an event. It then updates
the integration `HANDOFF.md`. Remote branches and PR comments are the second
recovery layer.

The controller then creates each task branch/worktree from the current remote
integration HEAD and writes that resolved SHA into the task PRD and
`HANDOFF.md`.

For every dispatch, the controller copies into the task PRD: this plan's
Global Constraints, the complete task section, its resolved base SHA, the
absolute worktree/target/evidence paths, prerequisite commit/receipt IDs, the
file lease, the handoff cadence, and the exact launch command. The worker must
run these literal freshness gates before its first edit:

```bash
tools/preflight.sh
git fetch origin staging/beta1-functional-integration main refactor/tag-machinery
test "$(git rev-parse HEAD)" = "$task_base"
test "$(git rev-parse origin/staging/beta1-functional-integration)" = "$task_base"
git rev-list --count HEAD..origin/main
```

No nonzero preflight result is tolerated. The final command records the known
`origin/main` divergence but does not use it as a gate. On resume, the worker
requires `git merge-base --is-ancestor "$task_base" HEAD`, while the controller
reconciles any later integration movement. A PRD is not a pointer to this
plan: it is the self-contained execution contract that lets a fresh CLI
process resume without conversation history.

The controller creates each task worktree with the branch, worktree, and base
recorded in the task section and ledger:

```bash
task_base=$(git -C /Users/allen/git/oxidex-beta1-functional-integration rev-parse HEAD)
git -C /Users/allen/git/oxidex-beta1-functional-integration worktree add \
  -b "$task_branch" "$task_worktree" "$task_base"
```

`task_branch` and `task_worktree` are set to the literal values in the task
section before this command; the controller records the resulting HEAD before
launching the worker.

Every task uses a child directory named exactly after its task slug under:

```text
/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/
```

The canonical task PRDs live under:

```text
/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/
```

The controller copies the immutable PRD into the plan-specific Superpowers
workspace before Desktop dispatch. The ledger stores both paths and the PRD
SHA-256. A dispatch is refused when either copy is missing or hashes differ.

For example, Task 5 launches through the durable controller as:

```bash
python3 tools/release/fleet_controller.py launch \
  --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller \
  --repo /Users/allen/git/oxidex-beta1-functional-integration --task 5
```

The controller launches Desktop implementers with an isolated
`collaboration.spawn_agent` request: `fork_turns: "none"`, the task's literal
model and reasoning effort, a stable `task_name` of `beta1_task_NN_SLUG`, and
a message containing only the task context sentence, canonical PRD path,
worktree, report path, and no-subagents rule. It records the returned agent ID
before considering the task running. Reviewers use a distinct
`beta1_review_NN_SLUG_rR` name and receive the PRD, report, review package,
and verbatim Global Constraints.

CLI launches use `fleet_controller.py launch`. It validates dependencies and
hashes, generates a unique launch token, then starts `codex --yolo exec
--enable fast_mode --model MODEL --json -o FINAL -C WORKTREE "Process token
TOKEN. Execute the canonical PRD at PRD_PATH"` with
`subprocess.Popen(..., start_new_session=True)` and append-only JSONL stdout
and stderr. The token and absolute PRD path are therefore visible in the exact
process argv without recording the process environment. The child survives
controller-shell death. The controller records the exact argv, PID, kernel
process start time, token, Codex thread ID from the `thread.started` JSON
event, log segment, final message, and exit status under
`controller/processes/TASK_NUMBER/`.

`monitor --task N --follow` tails new JSON events and advances a heartbeat;
`status --task N` is nonblocking; `stop --task N --signal TERM` verifies PID,
start time, executable, and task before signaling. `resume --task N` is
allowed only after the recorded process is dead, the worktree/remote state is
reconciled, and no unowned path changed. It starts this exact shape in the
task worktree, writes a new JSONL segment, and increments `launch_count`:

```bash
codex --yolo exec resume --enable fast_mode --model MODEL --json \
  -o FINAL SESSION_ID "Process token TOKEN. Continue from RECOVERY_PROMPT_PATH"
```

The resume prompt is a durable controller-generated file containing the PRD
hash, last accepted checkpoint, current `git status`, failed/incomplete step,
and exact next action. Recovery never uses `--last` and never infers liveness
from a reused PID alone.

## Red-zone Ownership Transition Matrix

The materialized PRD may narrow a lease but may never widen this matrix.

| Path or glob | Sequential task owners | Release condition |
|---|---:|---|
| `justfile` | 0, then 1, then 18 | each prior owner remotely merged; no concurrent edits |
| `tools/release/fleet_controller.py`, `tools/release/fleet-schema.json`, `tools/release/test_fleet_controller.py` | 0 | Task 0 only |
| `tools/exiftool-tables/runtime_ownership.py`, `runtime_ownership.json`, `test_runtime_ownership.py` | 1 | Task 1 only; vendors own distinct fragment files |
| `src/core/tag_occurrence.rs`, `src/core/tag_sink.rs`, `src/core/metadata_map.rs` | 2 | Task 2 remote merge before consumer/runtime work |
| `src/cli/tag_resolution.rs`, `src/cli/output_formatter.rs`, `src/cli/batch_processor.rs`, `src/ffi/**`, `src/composite/mod.rs` | 3 | Task 3 only; Task 14 may not edit `src/composite/mod.rs` |
| `tools/exiftool-tables/conv_codegen.py`, `conv_oracle.py`, conversion generated outputs and manifest | 4, then 10 | Task 4 remotely merges before Task 10 acquires the lease |
| `tools/exiftool-tables/upgrade_transaction.py`, `test_upgrade_transaction.py`, version-rehearsal executor/adapter tests | 5 | Task 5 only; Task 3 may not edit them |
| `tools/exiftool-tables/conformance.py`, `test_conformance.py` | 6 | Task 6 only; Task 8 explicitly denies these paths |
| `src/exiftool_tables/session.rs`, `cond.rs` | 7 | Task 7 only |
| `tools/exiftool-tables/genshare/**` | 8 | Task 8 only |
| `src/exiftool_tables/conv/mod.rs` | 4, then 9 | Task 4 remotely merges before Task 9 acquires the lease |
| `src/exiftool_tables/ifd_engine.rs` | 7, then 8, then 9, then 17, then 18 | every prior owner remotely merges before the next acquires the lease |
| `src/core/exif_dir_engine.rs`, `src/core/tiff_helpers.rs`, `src/core/jpeg_helpers.rs` | 7, then 9, then 10, then 18 | every prior owner remotely merges before the next acquires the lease |
| `src/exiftool_tables/enabled_ifd.rs` | 9 | Task 9 only |
| `src/exiftool_tables/engine.rs`, `keyed_engine.rs`, `serial_engine.rs` | 8, then 17, then 18 | every prior owner remotely merges before the next acquires the lease |
| `src/exiftool_tables/mod.rs` | 8, then 17 | Task 8 remotely merges before Task 17 acquires the lease |
| `tools/exiftool-tables/conv_exif_main_ledger.json` and its exact refusal-closure inputs/outputs | 10 | Task 10 only |
| `src/parsers/tiff/makernotes/olympus.rs`, `src/parsers/tiff/makernotes/olympus/**`, `runtime_ownership.d/olympus.json` | 11 | Task 11 only |
| each Task 12-16 parser/test lease and distinct `runtime_ownership.d/TASK_SLUG.json` | 12-16 respectively | no shared dispatcher, engine, composite, or generated output edits |
| `src/exiftool_tables/pipeline.rs`, `docs/reference/generated-runtime-walker-inventory.json`, `tests/generated_runtime_walker_contract.rs` | 17 | all Task 12-16 remotely merge first |
| compatibility/deletion ledger paths created by Task 18 | 18 | all runtime and vendor migrations remotely merge first |
| version-transition implementation and receipt files | 19 | Tasks 5, 6, and 18 merged first |
| `docs/UPGRADE-NEXT-STEPS.md`, `docs/reference/upgrade-rehearsal-11.78-12.64.md` | 19, then 20 | Task 19 remotely merges before Task 20 acquires the documentation lease |
| `.agents/skills/exiftool-parity/SKILL.md` | 0, then 20 | Task 0 remotely merges before Task 20 acquires the skill lease |
| release TODO, public measurements, and autogeneration docs | 20 | candidate frozen; no runtime writer active |

## Dependency and Dispatch Map

| Task | Slug | Worker/model | Depends on | May run with |
|---:|---|---|---|---|
| 0 | `durable-controller-oracle-bootstrap` | CLI / Terra | controller setup | none |
| 1 | `ownership-inventory` | Desktop / Terra | 0 | 2, 4, 5 |
| 2 | `typed-occurrence-core` | Desktop / Sol | 0 | 1, 4, 5 |
| 3 | `typed-consumers` | CLI / Terra | 2 | 4, 5, 6 |
| 4 | `conv-registry` | Desktop / Sol | 0 | 1, 2, 5 |
| 5 | `upgrade-transaction` | CLI / Terra | 0 | 1, 2, 4 |
| 6 | `conformance-receipts` | CLI / Luna | 0 | any non-conformance task |
| 7 | `file-session` | Desktop / Sol | 2, 4 | 3, 5, 6 |
| 8 | `generated-attribution` | CLI / Terra | 1, 3, 4, 7 | 5, 6 |
| 9 | `exif-shared-pipeline` | Desktop / Sol | 2, 4, 7, 8 | 5, 6 |
| 10 | `refusal-closure` | Desktop / Sol | 1, 4, 9 | 5, 6 |
| 11 | `olympus-pilot` | Desktop / Sol | 3, 8, 10 | 5, 6 |
| 12 | `nikon-port` | CLI / Terra | 11 | 13-16 |
| 13 | `pentax-panasonic-port` | CLI / Terra | 11 | 12, 14-16 |
| 14 | `dji-composite-xmp-port` | CLI / Terra | 11, 13 | 12, 15, 16 |
| 15 | `legacy-camera-tail` | CLI / Terra | 11 | 12-14, 16 |
| 16 | `trailer-tail` | CLI / Terra | 11 | 12-15 |
| 17 | `walker-engine-consolidation` | Desktop / Sol | 9, 11, 12-16 | none; vendors are remotely merged first |
| 18 | `proven-deletion` | Desktop / Sol | 8, 10-17 | documentation-only work |
| 19 | `version-transition-qualification` | CLI / Sol | 5, 6, 18 | documentation-only work |
| 20 | `frozen-candidate-evidence` | controller + Astra review | all prior tasks | none; writers frozen |

Initial dispatch:

- Dispatch and remotely merge Task 0 by itself.
- After Task 0's durable storage verification passes and its PR is merged,
  Desktop slots take Tasks 1, 2, and 4 while the CLI pool takes Tasks 5 and 6.
- Task 3 launches as soon as Task 2 integrates.
- Task 7 launches after Tasks 2 and 4 integrate.
- Unused CLI capacity remains idle until a dependency-ready task exists; do
  not manufacture speculative work to fill it.

## Local Checkpoint, Remote PR, and Integration Procedure

Every task is preserved twice: signed commits and `HANDOFF.md` in its local
worktree, plus a pushed task branch and draft PR on GitHub. Workers never use
GitHub credentials; the controller alone performs the remote steps with:

```bash
export GIT_SSH_COMMAND="ssh -o IdentityAgent=none -o IdentitiesOnly=yes -i /Users/allen/.ssh/id_es25519_swackhamer"
```

At each meaningful clean milestone, the worker creates a signed local
checkpoint commit and updates `HANDOFF.md`. The controller verifies the commit,
pushes the task branch, and opens a draft PR if none exists:

```bash
test -z "$(git -C "$task_worktree" status --short)"
git -C "$task_worktree" cat-file -p HEAD | rg '^gpgsig '
GIT_SSH_COMMAND="$GIT_SSH_COMMAND" git -C "$task_worktree" push -u origin "$task_branch"
gh pr create --repo swack-tools/oxidex --draft \
  --base staging/beta1-functional-integration --head "$task_branch" \
  --title "$task_pr_title" --body-file "$task_pr_body"
```

The controller stores the returned PR number/URL. Later milestone commits are
pushed normally and followed by one concise PR comment containing the pushed
SHA, completed plan step, exact tests, receipt hashes, blocker state, and next
step. No raw secrets, host credentials, or machine-only binary artifacts are
posted.

For final integration, load `task_worktree`, `task_base`, `task_head`,
`task_branch`, and `task_pr` from the controller ledger:

1. Read the task `HANDOFF.md`, local commits, draft PR, and current checks.
2. Verify `git -C "$task_worktree" status --short` is empty and local HEAD
   equals the latest pushed branch SHA.
3. Create a review package from `$task_base..$task_head` using:

```bash
bash /Users/allen/.codex/plugins/cache/openai-curated-remote/superpowers/6.4.1/skills/subagent-driven-development/scripts/review-package \
  docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md \
  "$task_base" "$task_head"
```
4. Dispatch a fresh reviewer at the task's stated reviewer model.
5. Complete the Superpowers fix loop and push every signed fix checkpoint.
6. Fetch `origin/staging/beta1-functional-integration` and compare paths:

```bash
git -C "$task_worktree" diff --name-only "$task_base..$task_head" | sort -u
git -C /Users/allen/git/oxidex-beta1-functional-integration \
  diff --name-only "$task_base..origin/staging/beta1-functional-integration" | sort -u
```

7. If the target advanced or path sets overlap, record the fetched target SHA
   as `rebase_target`, rebase the task in its own worktree onto that exact SHA,
   set `task_base=$rebase_target` and `task_head=$(git rev-parse HEAD)`, and
   write both to the ledger and task `HANDOFF.md`. Rerun covering tests, create
   the review package strictly from the new `$task_base..$task_head`, obtain a
   fresh review, and push with `--force-with-lease` against the recorded prior
   task-branch SHA. Never rewrite the protected target.
8. Update the PR body with final receipts, mark it ready, and wait for every
   required check. Fetch the target again after checks complete. If its SHA
   differs from the reviewed `task_base`, return to step 7. Otherwise record
   the exact pre-merge target SHA and squash-merge only the reviewed head:

```bash
gh pr ready "$task_pr" --repo swack-tools/oxidex
gh pr checks "$task_pr" --repo swack-tools/oxidex --watch --fail-fast
git -C "$task_worktree" fetch origin staging/beta1-functional-integration
test "$(git -C "$task_worktree" rev-parse origin/staging/beta1-functional-integration)" = "$task_base"
gh pr merge "$task_pr" --repo swack-tools/oxidex --squash \
  --match-head-commit "$task_head"
```

9. Fetch the target, read the PR merge SHA, and fast-forward the controller
   mirror:

```bash
git -C /Users/allen/git/oxidex-beta1-functional-integration fetch origin staging/beta1-functional-integration
git -C /Users/allen/git/oxidex-beta1-functional-integration merge --ff-only origin/staging/beta1-functional-integration
task_merge=$(gh pr view "$task_pr" --repo swack-tools/oxidex \
  --json mergeCommit --jq .mergeCommit.oid)
git -C /Users/allen/git/oxidex-beta1-functional-integration rev-parse "$task_merge^1"
test "$(git -C /Users/allen/git/oxidex-beta1-functional-integration rev-parse "$task_merge^1")" = "$task_base"
gh pr view "$task_pr" --repo swack-tools/oxidex --json state,mergeCommit,headRefOid,url
```

The dedicated integration target has one writer: this controller. It holds an
exclusive durable merge lease from the final pre-merge fetch through the
post-merge fetch and verifies that the returned squash commit's first parent
equals `task_base`. Any unexpected target movement is a lease violation: stop
all dispatch, preserve evidence, and do not classify the task merged. This
single-writer target removes the check-then-merge race for Tasks 0-20.

10. Record task base/head, latest pushed SHA, review verdict, CI checks, PR URL,
    merge SHA, post-merge target SHA, and receipts in the local controller
    ledger and integration `HANDOFF.md`. Release dependencies only after the
    remote merge is verified. Keep the local worktree and remote task branch
    until its wave and post-merge gates are accepted.

## Standard Candidate Acceptance Commands

The controller copies this section into every parser/runtime PRD and resolves
the three variables to the task section's literal paths. A worker may narrow
the red/green loop, but its final candidate uses the commands below. The
authenticated instruments refuse dirty trees, so formatting, tests, and
Clippy run first; the worker then commits the candidate; only that clean commit
may be measured. If a later gate fails, fix the code, create a new commit, and
rerun every receipt against the new HEAD. Never cite a receipt from an older
commit.

```bash
task_slug=nikon-port
task_target=/Users/allen/git/oxidex-beta1-targets/nikon-port
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/nikon-port
export EXIFTOOL_CACHE_DIR=/Users/allen/oxidex-ops/cache/exiftool/13.59
export EXIFTOOL_PERL=/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2
mkdir -p "$task_target" "$task_evidence"

cargo fmt --check
CARGO_TARGET_DIR="$task_target" \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$task_evidence/cargo-test.log" -- \
  cargo test --workspace --all-features
CARGO_TARGET_DIR="$task_target" \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$task_evidence/clippy.log" -- \
  cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_TARGET_DIR="$task_target" \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$task_evidence/release-build.log" -- \
  cargo build --release --bin oxidex
```

After committing the candidate and proving `git status --short` is empty, run
the combined-corpus receipt under the exclusive lock:

```bash
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  "$task_evidence/conformance.log" -- \
  python3 tools/exiftool-tables/conformance.py \
    /Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples \
    --recursive --min-files 4000 --min-tags 400000 \
    --exiftool-dir /Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool \
    --oxidex "$task_target/release/oxidex" \
    --json-out "$task_evidence/conformance.json"
```

Run the authenticated `t/images` read receipt and ratchet. `build` uses the
shared Cargo lock; `observe`, `verify`, and the regression gate use the
exclusive measurement lock:

```bash
CARGO_TARGET_DIR="$task_target" \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$task_evidence/read-build.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py build \
    --output "$task_evidence/read-build"
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  "$task_evidence/read-observe.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py observe \
    --build-proof "$task_evidence/read-build/build-proof.json" \
    --perl /Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2 \
    --exiftool-dir /Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool \
    --corpus /Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool/t/images \
    --output "$task_evidence/read-observe"
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  "$task_evidence/read-verify.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py verify \
    --receipt "$task_evidence/read-observe/receipt.json"
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  "$task_evidence/read-gate.log" -- \
  python3 tools/ci/read_regression_gate.py \
    --receipt "$task_evidence/read-observe/receipt.json"
```

Every task substitutes its own literal slug, target, and evidence path from
its task section. The full conformance JSON must reconcile, and parser tasks
require zero previously matched occurrences lost, zero new VALUE rows, and
read-regression `lost 0`. Targeted gains are asserted by the task's named
carrier tests, not by grepping formatted conformance output.

## Required Final Checkpoint in Every Materialized PRD

Task 0's `materialize` command appends this as the final action of every task,
with all variables replaced by literals from that task section:

```bash
git diff --check
git diff --name-only | sort -u
git diff --cached --name-only | sort -u
git ls-files --others --exclude-standard | sort -u
# Controller verifies every changed path is inside the exact lease.
git add -- "$TASK_FILE_1" "$TASK_FILE_2"
git commit -S -m "$TASK_COMMIT_MESSAGE"
git cat-file -p HEAD | rg '^gpgsig '
test -z "$(git status --short)"
```

The worker then writes `HANDOFF.md` with task/base/head, completed steps,
exact commands and results, receipt paths/hashes, blockers/rulings, and
`RETURN_TO_CONTROLLER` as the exact next action. It writes the report at the
canonical controller report path and returns only status, commit SHA, one-line
test summary, and concerns. The controller records the event before pushing
the task branch or opening/updating its draft PR.

---

### Task 0: Durable Controller, Oracle, and Corpus Bootstrap

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/00-durable-controller-oracle-bootstrap.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/durable-controller-oracle-bootstrap`
**Worktree:** `/Users/allen/git/oxidex-beta1-durable-controller-oracle-bootstrap`
**Target:** `/Users/allen/git/oxidex-beta1-targets/durable-controller-oracle-bootstrap`
**Commit:** `build: make release execution state durable`

**Bootstrap PRD:** Task 0 is the sole manual bootstrap. Its canonical PRD is a
byte-for-byte copy of the reviewed, merged plan, so creating it does not depend
on the not-yet-implemented materializer:

```bash
task_base=$(git -C /Users/allen/git/oxidex-beta1-functional-integration rev-parse HEAD)
git -C /Users/allen/git/oxidex-beta1-functional-integration worktree add \
  -b staging/beta1/durable-controller-oracle-bootstrap \
  /Users/allen/git/oxidex-beta1-durable-controller-oracle-bootstrap \
  "$task_base"
cd /Users/allen/git/oxidex-beta1-durable-controller-oracle-bootstrap
tools/preflight.sh
mkdir -p /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds
cp docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/00-durable-controller-oracle-bootstrap.md
cmp docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/00-durable-controller-oracle-bootstrap.md
shasum -a 256 \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/00-durable-controller-oracle-bootstrap.md
```

Record the merged plan commit and printed hash in `HANDOFF.md` before launch.
The Task 0 worker is told to execute Task 0 only.

**Launch:** Start Task 0 detached so it survives the supervising shell. Record
`worker_pid` and `ps -p "$worker_pid" -o lstart= -o command=` immediately in
`controller/processes/00/bootstrap-process-1.txt`, then watch the JSONL file:

```bash
mkdir -p /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/processes/00
bootstrap_token=$(uuidgen)
bootstrap_prompt="Bootstrap process token ${bootstrap_token}. Execute Task 0 only from the canonical PRD at /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/00-durable-controller-oracle-bootstrap.md. Obey its Global Constraints, update HANDOFF.md at every milestone, do not execute Task 1 or later, and finish with RETURN_TO_CONTROLLER."
nohup codex --yolo exec --enable fast_mode --model gpt-5.6-terra --json \
  -o /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/processes/00/final-1.md \
  -C /Users/allen/git/oxidex-beta1-durable-controller-oracle-bootstrap \
  "$bootstrap_prompt" \
  > /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/processes/00/events-1.jsonl \
  2>&1 < /dev/null &
worker_pid=$!
worker_start=$(ps -p "$worker_pid" -o lstart=)
worker_command=$(ps -ww -p "$worker_pid" -o command=)
printf 'pid=%s\nstart_time=%s\ntoken=%s\ncommand=%s\n' \
  "$worker_pid" "$worker_start" "$bootstrap_token" "$worker_command" \
  > /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/processes/00/bootstrap-process-1.txt
tail -F /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/processes/00/events-1.jsonl &
tail_pid=$!
if wait "$worker_pid"; then
  worker_status=0
else
  worker_status=$?
fi
if kill "$tail_pid" 2>/dev/null; then :; fi
printf 'exit_status=%s\n' "$worker_status" \
  >> /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/processes/00/bootstrap-process-1.txt
```

If the supervisor dies, extract the exact session ID with
`jq -r 'select(.type == "thread.started") | .thread_id'` from the persisted
`thread.started` event. A replacement first verifies PID, start time, and
executable. A matching live process is watched, never resumed; an identity
mismatch is a blocker and is never signaled. Only a confirmed-dead,
incomplete process may resume:

```bash
set -euo pipefail
cd /Users/allen/git/oxidex-beta1-durable-controller-oracle-bootstrap
process_root=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/processes/00
process_file=$(printf '%s\n' "$process_root"/bootstrap-process-*.txt | sort -V | tail -n 1)
segment=$(basename "$process_file" .txt | sed 's/^bootstrap-process-//')
recorded_pid=$(sed -n 's/^pid=//p' "$process_file")
recorded_start=$(sed -n 's/^start_time=//p' "$process_file")
recorded_token=$(sed -n 's/^token=//p' "$process_file")
recorded_command=$(sed -n 's/^command=//p' "$process_file")
if kill -0 "$recorded_pid" 2>/dev/null; then
  current_start=$(ps -p "$recorded_pid" -o lstart=)
  current_command=$(ps -ww -p "$recorded_pid" -o command=)
  test "$current_start" = "$recorded_start"
  test "$current_command" = "$recorded_command"
  printf '%s\n' "$current_command" | rg -F "$recorded_token"
  printf '%s\n' "$current_command" | rg -F '/Users/allen/git/oxidex-beta1-durable-controller-oracle-bootstrap'
  printf '%s\n' "$current_command" | rg -F 'gpt-5.6-terra'
  printf '%s\n' "$current_command" | rg -F "controller/processes/00/final-${segment}.md"
  tail -F "$process_root/events-${segment}.jsonl"
  exit 0
fi
git status --short
git log -1 --oneline
if rg -q 'RETURN_TO_CONTROLLER' HANDOFF.md "$process_root"/final-*.md; then
  exit 0
fi
session_id=$(jq -r 'select(.type == "thread.started") | .thread_id' \
  "$process_root"/events-*.jsonl | head -n 1)
test -n "$session_id"
next_segment=$((segment + 1))
resume_token=$(uuidgen)
resume_prompt="Bootstrap process token ${resume_token}. Continue Task 0 from its canonical PRD and HANDOFF.md. Reconcile the current worktree first; do not repeat completed external actions."
nohup codex --yolo exec resume --enable fast_mode --model gpt-5.6-terra --json \
  -o "$process_root/final-${next_segment}.md" \
  "$session_id" \
  "$resume_prompt" \
  > "$process_root/events-${next_segment}.jsonl" \
  2>&1 < /dev/null &
worker_pid=$!
worker_start=$(ps -p "$worker_pid" -o lstart=)
worker_command=$(ps -ww -p "$worker_pid" -o command=)
printf 'pid=%s\nstart_time=%s\ntoken=%s\ncommand=%s\n' \
  "$worker_pid" "$worker_start" "$resume_token" "$worker_command" \
  > "$process_root/bootstrap-process-${next_segment}.txt"
tail -F "$process_root/events-${next_segment}.jsonl" &
tail_pid=$!
if wait "$worker_pid"; then
  worker_status=0
else
  worker_status=$?
fi
if kill "$tail_pid" 2>/dev/null; then :; fi
printf 'exit_status=%s\n' "$worker_status" \
  >> "$process_root/bootstrap-process-${next_segment}.txt"
```

Every replacement uses the next numbered process, JSONL, and final-message
segment with a fresh token and the same liveness protocol. Recovery always
selects the newest process record and refuses `--last`.

**Files:**

- Create: `tools/release/bootstrap_oracle.py`
- Create: `tools/release/oracle-lock.json`
- Create: `tools/release/test_bootstrap_oracle.py`
- Create: `tools/release/fleet_controller.py`
- Create: `tools/release/fleet-schema.json`
- Create: `tools/release/test_fleet_controller.py`
- Create: `docs/reference/durable-release-storage.md`
- Create: `docs/reference/beta-fleet-controller.md`
- Modify: `scripts/exiftool_oracle.py`
- Modify: `justfile` recipes `docs-coverage`, `duplicate-loss-scan`,
  `compare-exiftool-full`, and `compare-exiftool-full-update`
- Modify: `.agents/skills/exiftool-parity/SKILL.md`
- Do not commit downloaded sources, corpora, Perl installations, or secrets

**Interfaces:**

- `bootstrap_oracle.py provision --root /Users/allen/oxidex-ops` installs only
  beneath the durable root and writes an authenticated storage manifest.
- `bootstrap_oracle.py verify --root /Users/allen/oxidex-ops --pin 13.59`
  refuses symlinks or resolved paths outside `/Users/allen/oxidex-ops`, checks
  locked source identities, and performs the version and DOCX probes.
- `oracle-lock.json` pins Perl 5.38.2 source SHA-256
  `a0a31534451eb7b83c7d6594a497543a54d488bc90ca00f5e34762577f40655e`,
  Archive-Zip 1.68 SHA-256
  `984e185d785baf6129c6e75f8eb44411745ac00bf6122fb1c8e822a3861ec650`
  (the digest published in the CPAN author
  [`CHECKSUMS`](https://cpan.metacpan.org/authors/id/P/PH/PHRED/CHECKSUMS)
  file for the 163,490-byte `Archive-Zip-1.68.tar.gz` artifact),
  and ExifTool tag object `2200871d9cef988051d2a99d67df3bda6cbb30a8`.
- `fleet_controller.py init|materialize|event|checkpoint|reconcile|recover|
  launch|monitor|status|heartbeat|resume|stop` validates the task DAG and
  paths, writes atomic snapshots plus append-only events, materializes
  self-contained PRDs, supervises detached CLI workers, records Desktop agent
  identities/events supplied by the primary session, records local/remote
  checkpoints, and reconstructs safe next actions after worker or controller
  death.
- `fleet-schema.json` requires schema version, plan/spec hashes, target ref/SHA,
  task number/slug, state history, dependencies, file lease, worker
  kind/model/effort/identity, PID/start-time/unique launch token/exact argv and
  session ID when applicable, launch count, PRD/report/review hashes,
  base/head/pushed/merge/target SHAs,
  worktree/target/evidence paths, heartbeat, PR/CI state, receipt hashes,
  ruling/blocker, exclusive merge-lease identity, expected merge parent, and
  exact next command.

- [ ] **Step 1: Write failing durable-path, controller, and identity tests**

In `test_bootstrap_oracle.py`, test that the resolver rejects the resolved
system temporary-directory root, `$TMPDIR`, a symlink escaping the
durable root, missing hashes, wrong Perl/Archive-Zip versions, the wrong
ExifTool tag object, a corpus below 4,000 files, and a DOCX probe other than
`DOCX`. Assert the exact durable paths used by the remainder of this plan.

In `test_fleet_controller.py`, use a durable test root beneath
`/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller-tests`
and assert: invalid/outside paths are refused; state writes are atomic;
events are append-only; dependencies block dispatch until remote merge;
materialized PRD hashes reconcile; PID reuse does not imply liveness; repeated
remote reconciliation is idempotent; detached CLI launch survives controller
death; `monitor` parses the thread ID and heartbeat; `resume` uses the recorded
ID rather than `--last`; only one controller can acquire the integration merge
lease; PID reuse with a different token-bearing exact argv is rejected;
Task 20 round paths never collide or overwrite prior receipts; an unexpected
squash parent blocks dependency release; and recovery after simulated worker
and controller death produces one next action without duplicate dispatch.

- [ ] **Step 2: Run the focused tests red**

```bash
uv run python -m unittest tools/release/test_bootstrap_oracle.py -v
uv run python -m unittest tools/release/test_fleet_controller.py -v
```

Expected: import failures because `bootstrap_oracle.py` and
`fleet_controller.py` do not exist.

- [ ] **Step 3: Implement idempotent durable provisioning**

Download archives into `/Users/allen/oxidex-ops/cache/downloads`, verify the
locked hashes before extraction, build Perl into
`/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix`, install the locked
Archive-Zip distribution into that prefix, materialize ExifTool and the
combined corpus under `/Users/allen/oxidex-ops/cache/exiftool/13.59`, and use
atomic rename within the same durable filesystem. An interrupted run leaves
the last verified installation intact and a journal naming the failed stage.

- [ ] **Step 4: Implement the durable fleet controller and path fence**

Implement all controller subcommands and the JSON schema above. Every command
resolves paths before use and accepts only descendants of `/Users/allen/git`
or `/Users/allen/oxidex-ops`. `materialize` combines the plan's Global
Constraints, complete task section, resolved base SHA, literal paths,
dependencies, exact file lease, handoff template, report contract, and launch
command into the canonical PRD. `recover` reconciles the snapshot, append-only
events, worktree commits, ignored `HANDOFF.md`, worker identity, pushed branch,
PR, CI, and merge SHA without mutating a protected branch.

`launch` uses `subprocess.Popen(start_new_session=True)` with a unique token
and canonical PRD path in the prompt argv plus durable JSONL/final-message
files. `monitor`,
`heartbeat`, `status`, `stop`, and `resume` implement the exact process and
session rules in the Controller Workspace contract. Tests use a fake Codex
executable and kill the controller parent to prove the worker remains alive.

- [ ] **Step 5: Change repository defaults and add refusal fences**

Set `scripts/exiftool_oracle.py` and the four named `justfile` recipes to the
durable cache/evidence roots. Explicit environment overrides remain supported
only when their resolved paths are under `/Users/allen/oxidex-ops`. Add a
repository test that fails if release tooling introduces a system
temporary-directory default.

- [ ] **Step 6: Provision and verify the durable installation and controller**

```bash
python3 tools/release/bootstrap_oracle.py provision \
  --root /Users/allen/oxidex-ops
python3 tools/release/bootstrap_oracle.py verify \
  --root /Users/allen/oxidex-ops --pin 13.59 \
  --manifest /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/storage-manifest.json
python3 tools/release/fleet_controller.py init \
  --plan docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md \
  --spec docs/superpowers/specs/2026-09-19-generated-runtime-release-functional-design.md \
  --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller \
  --target-ref origin/staging/beta1-functional-integration
python3 tools/release/fleet_controller.py recover \
  --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller \
  --repo /Users/allen/git/oxidex-beta1-functional-integration
```

Require Perl `v5.38.2`, Archive::Zip `1.68`, ExifTool `13.59`, DOCX detection,
at least 4,000 combined-corpus files, and recorded hashes for every archive,
source tree, corpus manifest, and executable. The controller must report Task 0
as the only launchable task and an idempotent recovery action.

- [ ] **Step 7: Run tests, documentation checks, and commit locally**

```bash
uv run python -m unittest tools/release/test_bootstrap_oracle.py -v
uv run python -m unittest tools/release/test_fleet_controller.py -v
EXIFTOOL_CACHE_DIR=/Users/allen/oxidex-ops/cache/exiftool/13.59 \
EXIFTOOL_PERL=/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2 \
  python3 scripts/exiftool_oracle.py
typos tools/release scripts/exiftool_oracle.py \
  docs/reference/durable-release-storage.md docs/reference/beta-fleet-controller.md \
  .agents/skills/exiftool-parity/SKILL.md
git add tools/release scripts/exiftool_oracle.py justfile \
  docs/reference/durable-release-storage.md docs/reference/beta-fleet-controller.md \
  .agents/skills/exiftool-parity/SKILL.md
git commit -S -m "build: make release execution state durable"
```

- [ ] **Step 8: Rehearse total process loss and recovery**

Run the controller's fixture mode with one completed task, one clean committed
but unpushed task, one pushed draft PR task, one dead CLI PID, one missing
Desktop agent, and one dependency-blocked task. Terminate the fixture
controller, invoke `recover` in a fresh process, and require the same task
states, no duplicate launch, the correct remote reconciliation actions, and
one exact next command per nonterminal task. Preserve the JSONL and recovery
receipt below the controller test root. Also terminate two successive Task 0
bootstrap supervisors while their detached workers remain live; each recovery
must select the newest numbered record, watch the matching token-bearing argv,
and create no duplicate resume.

- [ ] **Step 9: Update handoff and return for controller PR integration**

The controller pushes the checkpoint, opens the draft PR, runs review/CI, and
squash-merges Task 0 before creating any other task worktree.

---

### Task 1: Ownership Inventory and Duplicate-Owner Verifier

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/01-ownership-inventory.md`

**Worker:** Desktop subagent, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/ownership-inventory`
**Worktree:** `/Users/allen/git/oxidex-beta1-ownership-inventory`
**Target:** `/Users/allen/git/oxidex-beta1-targets/ownership-inventory`
**Commit:** `feat: verify generated and residual field ownership`

**Files:**

- Create: `tools/exiftool-tables/runtime_ownership.py`
- Create: `tools/exiftool-tables/runtime_ownership.json`
- Create: `tools/exiftool-tables/runtime_ownership.d/` source fragments
- Create: `tools/exiftool-tables/test_runtime_ownership.py`
- Modify: `justfile`
- Do not modify: `src/exiftool_tables/conv/**`, engine files, generated tables

**Interfaces:**

- Produces `runtime_ownership.json` schema 1 with source release/hash, stable
  module/table/field identity, owner category, symbol, refusal, and fixture.
  Hand-maintained ownership is split into deterministic family fragments under
  `runtime_ownership.d/` so later vendor tasks do not edit one shared file.
- Produces `runtime_ownership.py verify --root .`; exit 0 means every
  enabled field has exactly one owner and every residual symbol still exists.
- Consumes existing conversion ledgers, enabled-table registries, and explicit
  hand-residual arrays without assigning observation credit.
- Defines `StableFieldId(module: str, table: str, kind: Literal["numeric",
  "name", "index"], value: str)`, `load_rows(root: Path) -> list[dict]`, and
  `verify_rows(rows: Sequence[dict]) -> Verification`; `Refused` messages
  include the stable identity and both competing/missing owners.
- `runtime_ownership.json` has `schema: 1`, sorted `rows`, source release and
  source-tree hash, category totals, and a SHA-256 over all deterministic
  fragment inputs. Each row requires exactly one of `generated`,
  `walker-owned`, `residual`, `refused`, or `not-applicable`.

- [ ] **Step 1: Write failing schema and duplicate-owner tests**

Add fixtures in `test_runtime_ownership.py` that assert:

```python
self.assertEqual(row["owner"], "generated")
with self.assertRaisesRegex(ownership.Refused, "duplicate owner"):
    ownership.verify_rows([generated_row, residual_row])
with self.assertRaisesRegex(ownership.Refused, "unowned enabled field"):
    ownership.verify_rows([enabled_without_owner])
```

- [ ] **Step 2: Prove the tests fail before implementation**

Run:

```bash
uv run python -m unittest tools/exiftool-tables/test_runtime_ownership.py -v
```

Expected: import/file failure because `runtime_ownership.py` does not exist.

- [ ] **Step 3: Implement the inventory and verifier**

Use stable identity shaped as:

```python
{
    "module": "Exif",
    "table": "Main",
    "field": {"kind": "numeric", "value": "0x829a"},
    "owner": "generated",
    "symbol": "src/exiftool_tables/conv/exif_main.rs::decode",
    "source_release": "13.59",
    "source_sha256": hashlib.sha256(source_row_bytes).hexdigest(),
    "refusal": None,
    "fixture": str(carrier.relative_to(repository_root)),
}
```

Support `numeric`, `name`, and `index` field kinds. Infer generated rows from
committed ledgers; require explicit JSON entries for walker/residual/refused
ownership. Do not infer runtime coverage from declarations.

- [ ] **Step 4: Seed current Exif::Main and directory residual ownership**

Account for all 551 generated, 17 refused, and 29 non-conversion ledger fields,
plus `IFD0_HAND_KEPT`, `EXIF_IFD_HAND_KEPT`, IFD1 residuals, and structural
edge ownership. Every refusal records its current reason rather than being
silently categorized as residual.

- [ ] **Step 5: Add the verifier recipe and run focused tests**

Add `just verify-runtime-ownership`, then run the test and recipe. Expected:
all synthetic controls pass and the repository inventory reconciles.

- [ ] **Step 6: Commit, update handoff, and report**

```bash
git add tools/exiftool-tables/runtime_ownership.py \
  tools/exiftool-tables/runtime_ownership.json \
  tools/exiftool-tables/runtime_ownership.d \
  tools/exiftool-tables/test_runtime_ownership.py justfile
git commit -S -m "feat: verify generated and residual field ownership"
```

Record counts by owner category, every intentionally unresolved refusal,
commit SHA, exact tests, and `RETURN_TO_CONTROLLER` as the next action in
`HANDOFF.md`.

---

### Task 2: Canonical Typed-Occurrence Core

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/02-typed-occurrence-core.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/typed-occurrence-core`
**Worktree:** `/Users/allen/git/oxidex-beta1-typed-occurrence-core`
**Target:** `/Users/allen/git/oxidex-beta1-targets/typed-occurrence-core`
**Commit:** `refactor: define canonical metadata value channels`

**Files:**

- Modify: `src/core/tag_occurrence.rs`
- Modify: `src/core/tag_sink.rs`
- Modify: `src/core/metadata_map.rs`
- Modify focused tests in those files
- Do not modify CLI, FFI, composites, writers, sessions, or engines

**Interfaces:**

- Produces `ValueChannel::{Stored, ValueConv, PrintConv}`.
- Produces `TagOccurrence::project(&self, channel: ValueChannel) -> &TagValue`
  with explicit fallback rules and keeps existing `value_conv()` as the
  compatibility wrapper during this task.
- Produces `TagSink::winner_projected(&self, key: &str, channel:
  ValueChannel) -> Option<&TagValue>` and
  `MetadataMap::project_occurrences(&self, channel: ValueChannel) ->
  impl Iterator<Item = (&str, &TagOccurrence, &TagValue)>` without collapsing
  duplicate instances.

- [ ] **Step 1: Write failing channel tests**

Pin all combinations:

```rust
assert_eq!(occ.project(ValueChannel::Stored), stored);
assert_eq!(occ.project(ValueChannel::ValueConv), typed);
assert_eq!(occ.project(ValueChannel::PrintConv), printed);
```

Name the inline tests
`tag_occurrence::tests::value_channel_projection_matrix`,
`tag_sink::tests::winner_projection_preserves_instances`, and
`metadata_map::tests::project_occurrences_keeps_duplicate_order`. Cover
fallbacks, `undef`, byte strings, binary display, signed zero, lists, duplicate
winners, tombstones, and instance-specific winners.

- [ ] **Step 2: Run the focused library tests and observe failure**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/typed-occurrence-core \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-occurrence-core/red.log -- \
cargo test --lib projection
```

Expected: compile failure because `ValueChannel` and `project` do not exist.

- [ ] **Step 3: Implement the channel contract**

Use these semantics:

```rust
pub enum ValueChannel { Stored, ValueConv, PrintConv }

// Stored: stored.as_ref().unwrap_or(&raw)
// ValueConv: explicit value, then exact existing compatibility ValueConv, then raw
// PrintConv: explicit print, then ValueConv projection
```

Keep `raw` as decoded/pre-conversion input. Do not parse display strings to
manufacture typed values.

- [ ] **Step 4: Add sink/map projection APIs without migrating consumers**

Preserve compatibility for existing `get()` call sites in this task. Add
explicit projection methods used by Task 3 and occurrence enumeration that
does not collapse duplicates.

- [ ] **Step 5: Run focused tests, formatting, and Clippy**

Use the shared lock for Cargo. Require all affected core tests green and zero
Clippy warnings:

```bash
cargo fmt --check
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/typed-occurrence-core \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-occurrence-core/test.log -- \
  cargo test --lib core::
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/typed-occurrence-core \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-occurrence-core/clippy.log -- \
  cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- [ ] **Step 6: Commit, update handoff, and report**

```bash
git add src/core/tag_occurrence.rs src/core/tag_sink.rs src/core/metadata_map.rs
git commit -S -m "refactor: define canonical metadata value channels"
```

The report and `HANDOFF.md` list every fallback rule, any compatibility
conversion still living in `exiftool_compat.rs`, commit SHA, exact tests, and
`RETURN_TO_CONTROLLER` as the next action.

---

### Task 3: Migrate Typed-Occurrence Consumers

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/03-typed-consumers.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/typed-consumers`
**Worktree:** `/Users/allen/git/oxidex-beta1-typed-consumers`
**Target:** `/Users/allen/git/oxidex-beta1-targets/typed-consumers`
**Commit:** `refactor: project typed metadata values consistently`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 3`

**Files:**

- Modify: `src/cli/tag_resolution.rs`
- Modify: `src/cli/output_formatter.rs`
- Modify: `src/cli/batch_processor.rs`
- Modify: `src/ffi/read_tags.rs`
- Modify: `src/ffi/write_tags.rs`
- Modify: `src/ffi/context.rs`
- Modify: `src/ffi/lifecycle.rs`
- Modify: `src/ffi/mod.rs`
- Modify: `src/composite/compute.rs`
- Modify: `src/composite/mod.rs`
- Modify: `src/core/operations.rs`
- Modify: `include/oxidex.h`
- Modify: `bindings/python/oxidex.py`
- Add: `tests/ffi/c_integration_test.c`
- Add: `tests/typed_value_projection_tests.rs`
- Add: `tools/exiftool-tables/fixtures/typed_value_projection.json`
- Do not modify Task 2 core files or generated/engine files

**Interfaces:** Consumes Task 2's `ValueChannel`,
`TagOccurrence::project`, `TagSink::winner_projected`, and
`MetadataMap::project_occurrences`. Keeps the exported C ABI layout unchanged;
new FFI channel selection is an explicit enum/entry point, while existing
entry points retain their documented default. `copy_metadata` selects
`Stored`; `composite::compute` receives `ValueConv`; CLI
`resolved_display_value(occurrence, no_print_conv)` delegates to the canonical
projection rather than reimplementing fallback.

- [ ] **Step 1: Add a normal/`-n` characterization matrix**

Cover integer, float, rational, enum, date/time, bytes, binary placeholder,
list, undefined/suppressed, duplicate/grouped occurrences, units, and negative
zero. Assert normal output uses PrintConv and `-n` uses ValueConv from the same
occurrence.

- [ ] **Step 2: Prove the remaining consumer seams fail before migration**

Run the focused integration test under the shared lock:

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/typed-consumers \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-consumers/red.log -- \
  cargo test --test typed_value_projection_tests -- --nocapture
```

PR #867 already fixed generated-IFD production of explicit PrintConv forms.
Do not recreate or simulate that superseded producer failure. Instead, add
honest red tests for the remaining consumer defects identified by the
independent review at
`/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/reviews/03-typed-consumers-supersession.md`:

1. Construct one valid occurrence whose raw, ValueConv, and explicit
   PrintConv forms differ. Normal CLI resolution must select the explicit
   PrintConv form and `--no-print-conv` must select ValueConv from that same
   occurrence. The current normal path incorrectly recomputes from raw.
2. Add an additive FFI value-channel enum and channel-selecting entry point,
   with a red compile/use test before implementation. Existing entry points
   must remain present and retain their documented default and ABI layout.
3. Add a source-cited allowlist and verifier for intentional composite
   display stringify/reparse sites. The verifier must fail for an unlisted
   site; comments alone are insufficient.
4. Add the fixture-backed consumer projection matrix from Step 1. Fixture
   absence is a failure, not a skip or behavior receipt.

- [ ] **Step 3: Migrate consumers by intent**

- CLI normal/JSON default: `PrintConv`.
- CLI `-n`: `ValueConv`.
- file-copy/rebuild: `Stored`.
- composite inputs: `ValueConv`, except source-proven stringify/reparse cases.
- FFI/library: expose explicit channel selection without silently changing ABI
  layout.

- [ ] **Step 4: Inventory remaining display reparsing**

Add a test-backed allowlist naming the ExifTool source location for each
intentional stringify/reparse. Unlisted parsing of printed units or fractions
fails the focused verifier.

- [ ] **Step 5: Run focused tests, commit clean candidate, and measure**

Use only the pinned Perl/ExifTool paths. Save paired raw outputs and hashes in
the task evidence directory. Rerun the exact focused command from Step 2 and
then the Standard Candidate Acceptance Commands with:

```bash
task_slug=typed-consumers
task_target=/Users/allen/git/oxidex-beta1-targets/typed-consumers
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-consumers
```

Run formatting and Clippy before the signed task commit. Run the conformance
and read-receipt blocks only after that commit leaves the worktree clean.

- [ ] **Step 6: Commit, update handoff, and report**

```bash
git add src/cli/tag_resolution.rs src/cli/output_formatter.rs \
  src/cli/batch_processor.rs src/ffi src/composite/compute.rs \
  src/composite/mod.rs src/core/operations.rs \
  include/oxidex.h bindings/python/oxidex.py \
  tests/ffi/c_integration_test.c \
  tests/typed_value_projection_tests.rs \
  tools/exiftool-tables/fixtures/typed_value_projection.json
git commit -S -m "refactor: project typed metadata values consistently"
```

Write the commit SHA, tests, receipt hashes, remaining source-proven reparsing
allowlist, and `RETURN_TO_CONTROLLER` next action to `HANDOFF.md`.

---

### Task 4: Generalize the Generated Conversion Registry

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/04-conv-registry.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/conv-registry`
**Worktree:** `/Users/allen/git/oxidex-beta1-conv-registry`
**Target:** `/Users/allen/git/oxidex-beta1-targets/conv-registry`
**Commit:** `feat: generate conversion registry for enabled tables`

**Files:**

- Modify: `tools/exiftool-tables/conv_codegen.py`
- Modify: `tools/exiftool-tables/conv_oracle.py`
- Modify: `tools/exiftool-tables/test_conv_codegen.py`
- Modify: `tools/exiftool-tables/artifacts.py`
- Modify: `src/exiftool_tables/conv/mod.rs`
- Create generated registry/modules under `src/exiftool_tables/conv/`
- Modify: `src/exiftool_tables/conv/tests.rs`
- Do not modify IFD/session/core occurrence files

**Interfaces:** Preserves `Decode = fn(&mut Session, u16, &MemberVal) -> Arm`.
Produces `Entry { module: &'static str, table: &'static str, decode: Decode,
claims: fn(u16) -> bool }`, with `decoder(table: &IfdTable) -> Option<Decode>`
and `claims(table: &IfdTable, tag: &IfdTag) -> bool` both resolved through the
same generated table identity. Keeps `RawConv`, `ValueConv`, and `PrintConv`
distinct. Extends `conv_oracle.py` with `--all`, which checks every emitted
registry entry in deterministic identity order. Task 4 explicitly denies
`justfile`.

- [ ] **Step 1: Add failing multi-table registry tests**

Use a synthetic second table and assert `decoder()` and `claims()` select its
own functions rather than `exif_main::claims`. Add missing/orphan/stale and
duplicate table identity controls.

- [ ] **Step 2: Run conversion codegen and Rust tests red**

```bash
uv run python -m unittest tools/exiftool-tables/test_conv_codegen.py -v
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/conv-registry \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/conv-registry/red.log -- \
  cargo test --lib exiftool_tables::conv::tests
```

Expected: second-table dispatch fails because the registry is hard-coded to
`Exif::Main`.

- [ ] **Step 3: Generate the registry**

Emit a deterministic sorted registry such as:

```rust
pub struct Entry {
    pub module: &'static str,
    pub table: &'static str,
    pub decode: Decode,
    pub claims: fn(u16) -> bool,
}
```

Do not hand-maintain table names. Preserve per-field mixed mode and ledger
refusals.

- [ ] **Step 4: Generate per-table ledgers and stable identities**

Unrelated source row movement must not renumber write/test identities. Refuse
non-IFD key shapes until a typed key interface is implemented; do not coerce a
name/index into `u16`.

- [ ] **Step 5: Run codegen tests, pinned conversion oracle, and verifier**

Run generation twice and require `git diff --exit-code` for owned generated
paths after the second run. Use:

```bash
uv run python -m unittest tools/exiftool-tables/test_conv_codegen.py -v
uv run python tools/exiftool-tables/conv_oracle.py --check --all \
  --perl /Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2 \
  --exiftool-dir /Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/conv-registry \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/conv-registry/regen-1.log -- \
  tools/exiftool-tables/regen-all.sh
git add src/exiftool_tables/conv tools/exiftool-tables
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/conv-registry \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/conv-registry/regen-2.log -- \
  tools/exiftool-tables/regen-all.sh
git diff --exit-code -- src/exiftool_tables/conv tools/exiftool-tables
```

- [ ] **Step 6: Run Rust tests, fmt, Clippy, commit, handoff, and report**

```bash
git add tools/exiftool-tables/conv_codegen.py \
  tools/exiftool-tables/conv_oracle.py \
  tools/exiftool-tables/test_conv_codegen.py \
  tools/exiftool-tables/artifacts.py src/exiftool_tables/conv
git commit -S -m "feat: generate conversion registry for enabled tables"
```

Record generated modules/ledgers, source hashes, test and oracle receipts,
commit SHA, and `RETURN_TO_CONTROLLER` in `HANDOFF.md`.

---

### Task 5: Fresh BEFORE/AFTER Upgrade Transaction

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/05-upgrade-transaction.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/upgrade-transaction`
**Worktree:** `/Users/allen/git/oxidex-beta1-upgrade-transaction`
**Target:** `/Users/allen/git/oxidex-beta1-targets/upgrade-transaction`
**Commit:** `fix: regenerate both sides of ExifTool upgrades`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 5`

**Files:**

- Modify: `tools/exiftool-tables/upgrade_transaction.py`
- Modify: `tools/exiftool-tables/test_upgrade_transaction.py`
- Do not modify version-rehearsal executor/adapter files; Task 19 owns them
- Do not modify `.exiftool-version` or generated artifacts

**Interfaces:** Preserves `Transaction.sources`, `identities`, `grade`,
`promote`, `execute`, module `recover`, `recovery_state`, and
`validate_conformance`. Changes `Transaction.variant(label, version,
regenerate)` so both `before` and `after` variants always regenerate from their
selected immutable source tree, even when the version equals the committed
pin. Variant receipts include source hash, generator hash, output hashes, and
clean-tree proof.

- [ ] **Step 1: Add the same-pin stale-artifact regression test**

Assert the BEFORE variant invokes regeneration even when
`old == committed_pin`, records its source hash, and detects a stale committed
artifact.

- [ ] **Step 2: Run the focused test red**

```bash
uv run python -m unittest tools/exiftool-tables/test_upgrade_transaction.py -v
```

Expected: the new assertion fails at
`variant("before", self.old, self.old != self.pin)`.

- [ ] **Step 3: Remove the skip and preserve transaction recovery**

Generate BEFORE and AFTER in separate clean variant trees and target
directories. Do not mutate the caller until both variants, comparison, and
gates pass.

- [ ] **Step 4: Add failure controls**

Cover interrupted BEFORE generation, added/removed modules, changed helper
source, undeclared output, and recovery that restores the original pin and
artifacts.

- [ ] **Step 5: Run all transaction/rehearsal unit tests**

Save command/output in the task evidence directory:

```bash
mkdir -p /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/upgrade-transaction
set -o pipefail
uv run python -m unittest tools/exiftool-tables/test_upgrade_transaction.py -v \
  2>&1 | tee /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/upgrade-transaction/tests.log
```

- [ ] **Step 6: Commit, update handoff, and report**

```bash
git add tools/exiftool-tables/upgrade_transaction.py \
  tools/exiftool-tables/test_upgrade_transaction.py
git commit -S -m "fix: regenerate both sides of ExifTool upgrades"
```

Record before/after source and output hashes, recovery controls, commit SHA,
and `RETURN_TO_CONTROLLER` in `HANDOFF.md`.

---

### Task 6: Complete Occurrence-Aware Conformance Receipts

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/06-conformance-receipts.md`

**Worker:** Codex CLI, `gpt-5.6-luna`, fast mode
**Reviewer:** `gpt-5.6-terra`, fast mode
**Branch:** `staging/beta1/conformance-receipts`
**Worktree:** `/Users/allen/git/oxidex-beta1-conformance-receipts`
**Target:** `/Users/allen/git/oxidex-beta1-targets/conformance-receipts`
**Commit:** `test: authenticate conformance occurrence totals`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 6`

**Files:**

- Modify: `tools/exiftool-tables/conformance.py`
- Modify: `tools/exiftool-tables/test_conformance.py`
- Do not modify parsers, runtime, or generated files

**Interfaces:** Extends schema 1 JSON with `oracle_occurrences`,
`candidate_occurrences`, `matched_occurrences`, and complete instrument
identity. Requires
`oracle_occurrences = matched_occurrences + missing_occurrences +
value_occurrences + rename_source_occurrences` and
`candidate_occurrences = matched_occurrences + extra_occurrences +
value_occurrences + rename_target_occurrences`; duplicate instances remain
separate. Preserves `run_exiftool`, `run_oxidex`, `compare`, and matching
behavior.

- [ ] **Step 1: Add a failing JSON receipt test**

Require top-level raw oracle occurrence total, candidate occurrence total,
matched/value/missing/extra/rename reconciliation, instrument provenance,
binary hash, corpus roots/file count, oracle source/runtime, and floors.

- [ ] **Step 2: Run the focused Python test red**

```bash
uv run python -m unittest tools/exiftool-tables/test_conformance.py -v
```

Expected: the raw oracle occurrence field is absent.

- [ ] **Step 3: Add fields without changing matching behavior**

Keep formatted output stable except for an explicit instrument line. Maintain
byte-identical JSON under different hash seeds.

- [ ] **Step 4: Add vacuity and tampering controls**

Reject missing floors, changed binary after measurement, and totals that do not
reconcile.

- [ ] **Step 5: Run tests, commit, update handoff, and report**

```bash
uv run python -m unittest tools/exiftool-tables/test_conformance.py -v
typos tools/exiftool-tables/conformance.py tools/exiftool-tables/test_conformance.py
git add tools/exiftool-tables/conformance.py \
  tools/exiftool-tables/test_conformance.py
git commit -S -m "test: authenticate conformance occurrence totals"
```

Record schema/reconciliation totals, test output, commit SHA, and
`RETURN_TO_CONTROLLER` in `HANDOFF.md`.

---

### Task 7: Make Session File-Scoped

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/07-file-session.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/file-session`
**Worktree:** `/Users/allen/git/oxidex-beta1-file-session`
**Target:** `/Users/allen/git/oxidex-beta1-targets/file-session`
**Commit:** `refactor: retain ExifTool session state for each file`

**Files:**

- Modify: `src/exiftool_tables/session.rs`
- Modify: `src/exiftool_tables/ifd_engine.rs`
- Modify: `src/exiftool_tables/cond.rs` only for required session access
- Modify: `src/core/exif_dir_engine.rs`
- Modify: `src/core/tiff_helpers.rs` and `src/core/jpeg_helpers.rs` only at
  session-construction/call boundaries
- Add: `tests/generated_file_session.rs`
- Add focused Rust unit tests in those modules
- Do not change conversion registry generation or occurrence consumer APIs

**Interfaces:**

- One `Session` is created by the file-level caller and passed through every
  directory walk.
- Directory scope reset/restore covers `DIR_NAME`, `Compression`,
  `SubfileType`, byte order, count, and format while preserving file members,
  values, options, warnings, and processed state.
- Reconciles the existing cross-walk `cond::Ctx` and conversion `Session`:
  `process_exif` and `process_exif_decoded` accept `&mut Session`; the
  file-entry bridge owns one Session, and directory walks enter a
  `DirectoryScope` guard whose `Drop` restores directory-local fields on every
  return path. Existing `Session::{member,set_member,remove_member}` and
  warnings/options remain file-scoped.

- [ ] **Step 1: Write failing cross-directory state tests**

Build a synthetic parent/child/next-sibling walk in which:

```rust
// Parent sets a DataMember.
// Child reads it and temporarily changes DIR_NAME and Compression.
// Sibling still reads the parent DataMember but not the child's directory state.
```

Add byte-string, `undef`, negative-zero, evaluation-order, recursion guard, and
duplicate-occurrence controls.

- [ ] **Step 2: Run the focused Session and IFD tests red**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/file-session \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/file-session/red.log -- \
  cargo test --test generated_file_session -- --nocapture
```

Expected: the cross-directory read fails because `ifd_engine` constructs a new
session per directory.

- [ ] **Step 3: Add explicit directory scope state**

Implement a snapshot/restore API whose types make directory-local and
file-global fields explicit. Restoration must occur on Report, Suppress,
Decline, and ordinary error return.

- [ ] **Step 4: Thread one mutable Session from file entry points**

Remove directory-local `Session::new()`/`for_ifd()` construction from the walk.
Callers seed only state they actually know; absent data remains unsupplied and
causes a generated decline rather than a guessed value.

- [ ] **Step 5: Run focused tests and the full Rust library suite**

Use the shared lock. Require no ordering, group, or warning regression:

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/file-session \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/file-session/test.log -- \
  cargo test --lib
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/file-session \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/file-session/integration.log -- \
  cargo test --test generated_file_session
```

- [ ] **Step 6: Run fmt and Clippy, commit, update handoff, and report**

```bash
git add src/exiftool_tables/session.rs src/exiftool_tables/ifd_engine.rs \
  src/exiftool_tables/cond.rs src/core/exif_dir_engine.rs \
  src/core/tiff_helpers.rs src/core/jpeg_helpers.rs \
  tests/generated_file_session.rs
git commit -S -m "refactor: retain ExifTool session state for each file"
```

Record scope semantics, call-site inventory, tests, commit SHA, and
`RETURN_TO_CONTROLLER` in `HANDOFF.md`.

---

### Task 8: Validate Generated-On/Generated-Off Attribution

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/08-generated-attribution.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/generated-attribution`
**Worktree:** `/Users/allen/git/oxidex-beta1-generated-attribution`
**Target:** `/Users/allen/git/oxidex-beta1-targets/generated-attribution`
**Commit:** `test: authenticate generated route attribution`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 8`

**Files:**

- Modify: `tools/exiftool-tables/genshare/attribute.py`
- Modify: `tools/exiftool-tables/genshare/census.sh`
- Replace or update: `tools/exiftool-tables/genshare/probe.patch`
- Modify: `tools/exiftool-tables/genshare/README.md`
- Add: `tools/exiftool-tables/genshare/test_attribute.py`
- Add: `tools/exiftool-tables/genshare/test_census.py`
- Add: `tools/exiftool-tables/genshare/testdata/bounded-corpus.txt`
- Create: `src/exiftool_tables/attribution.rs`
- Modify: `src/exiftool_tables/mod.rs`
- Modify: `src/exiftool_tables/engine.rs`
- Modify: `src/exiftool_tables/ifd_engine.rs`
- Modify: `src/exiftool_tables/keyed_engine.rs`
- Modify: `src/exiftool_tables/serial_engine.rs`
- Modify: `src/exiftool_tables/runtime.rs`
- Do not change tag conversion semantics
- Do not modify: `tools/exiftool-tables/conformance.py`,
  `tools/exiftool-tables/test_conformance.py`, any vendor parser, or generated
  table output

**Interfaces:** Produces an authenticated paired control/probe receipt consumed
by Tasks 11 and 18. Replaces positional worktree arguments with this maintained
interface:

```bash
tools/exiftool-tables/genshare/census.sh \
  --repository /Users/allen/git/oxidex-beta1-generated-attribution \
  --target-dir /Users/allen/git/oxidex-beta1-targets/generated-attribution \
  --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/generated-attribution/census \
  --corpus /Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples \
  --perl /Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2 \
  --exiftool-dir /Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool \
  --tokens engine,legacy-l1,legacy-l2,producers,serial,keyed
```

The script builds one maintained binary whose unset/empty
`OXIDEX_GENSHARE_SILENCE` hook is the control,
then runs each explicit token as a probe. It writes `receipt.json` and exits
nonzero on a refused token, process failure, floor miss, inertness difference,
hash mismatch, or attribution reconciliation failure.

`attribution::Token::{Engine,LegacyL1,LegacyL2,Producers,Serial,Keyed}` parses
the comma-separated environment once. `silenced(token) -> bool` is false when
the variable is absent/empty. Engine guards drop only the outward emission
after stateful work succeeds; disabled-hook behavior must remain byte-identical
on the bounded corpus. Unknown tokens and the unsafe `conv` token exit 2 before
corpus traversal.

- [ ] **Step 1: Add failing receipt-authentication tests**

Require control/probe binary SHA-256, source SHA/dirty state, exact process
return codes, raw stdout/stderr hashes, oracle/corpus identity, inertness result,
per-occurrence deltas, and token set. Refuse historical `attr-72ea`-style data
missing paired outputs.

- [ ] **Step 2: Run the focused tests red**

Create and run `tools/exiftool-tables/genshare/test_attribute.py` and
`tools/exiftool-tables/genshare/test_census.py`:

```bash
uv run python -m unittest \
  tools/exiftool-tables/genshare/test_attribute.py \
  tools/exiftool-tables/genshare/test_census.py -v
```

Expected: the current summarized inputs lack required authentication fields.

- [ ] **Step 3: Convert the temporary probe into a maintained test seam**

The seam may silence outward reporting but must retain session writes,
descents, cipher state, and other internal effects. Unknown tokens exit nonzero.
Unset/empty control output must be byte-identical to the ordinary binary for
all deterministic fields.

- [ ] **Step 4: Harden `census.sh`**

Use explicit `CARGO_TARGET_DIR` paths, pinned Perl/source probes, candidate
hashes, corpus floors, `pipefail`, and durable per-stage status. Never infer a
successful command from `tail` or another pipeline consumer.

- [ ] **Step 5: Run bounded controls, commit, and run a small real census**

Use the exclusive lock. Prove one known generated fixture loses its generated
occurrence, one hand-only fixture does not, and the inert control matches.
The test invokes `census.sh` with an explicit three-file manifest committed at
`tools/exiftool-tables/genshare/testdata/bounded-corpus.txt` and writes its
authenticated receipt under
`/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/generated-attribution/`.
Run the Python/shell tests, Rust tests for the maintained seam, formatting, and
Clippy first. Commit the signed task candidate and require a clean worktree.
Then run the full command above through the exclusive lock.

- [ ] **Step 6: Commit, update handoff, and report**

```bash
git add tools/exiftool-tables/genshare src/exiftool_tables/attribution.rs \
  src/exiftool_tables/mod.rs src/exiftool_tables/engine.rs \
  src/exiftool_tables/ifd_engine.rs src/exiftool_tables/keyed_engine.rs \
  src/exiftool_tables/serial_engine.rs src/exiftool_tables/runtime.rs
git commit -S -m "test: authenticate generated route attribution"
```

Record the control/probe receipt hashes, token reconciliation, commit SHA, and
`RETURN_TO_CONTROLLER` in `HANDOFF.md`.

---

### Task 9: Exact-Once Exif Shared Pipeline

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/09-exif-shared-pipeline.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/exif-shared-pipeline`
**Worktree:** `/Users/allen/git/oxidex-beta1-exif-shared-pipeline`
**Target:** `/Users/allen/git/oxidex-beta1-targets/exif-shared-pipeline`
**Commit:** `refactor: route Exif directories through one tag pipeline`

**Files:**

- Modify: `src/exiftool_tables/conv/mod.rs`
- Modify: `src/exiftool_tables/ifd_engine.rs`
- Modify: `src/core/exif_dir_engine.rs`
- Modify: `src/core/tiff_helpers.rs`
- Modify: `src/core/jpeg_helpers.rs`
- Modify: `src/exiftool_tables/enabled_ifd.rs`
- Add focused tests in these modules
- Add: `tests/exif_shared_pipeline.rs`
- Do not edit vendor parser directories or delete compatibility branches

**Interfaces:** Consumes Tasks 2, 4, 7, and 8. Preserves
`conv::Arm::{Report(Report), Suppress, Decline(&'static str)}` and introduces a
`StagedEffects` guard around the mutable `Session`. The guard exposes
`commit(self, session: &mut Session)` and `discard(self)`; residual dispatch
occurs only after discard. Produces one exact-once route for IFD0, IFD1,
ExifIFD, and InteropIFD through `exif_dir_engine::walk`, with one named
`Owner::{Engine, Hand, Silent}` decision and at most one residual invocation.

- [ ] **Step 1: Add failing side-effect and exact-once tests**

Pin:

```rust
// Report commits generated writes exactly once.
// Suppress commits source-required writes, emits nothing, and never falls back.
// Decline discards staged writes before invoking exactly one residual.
```

Also test duplicate order, group 1, requested edge tags, nested directories,
and a non-UTF-8 reported scalar that currently declines at the text boundary.

- [ ] **Step 2: Run focused engine tests red**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/exif-shared-pipeline \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/exif-shared-pipeline/red.log -- \
  cargo test --test exif_shared_pipeline -- --nocapture
```

Expected: at least the decline/write and cross-directory route controls fail.

- [ ] **Step 3: Stage generated effects until outcome resolution**

Do not mutate the live session before Report/Suppress/Decline ownership is
known. Commit effects in source order only for outcomes whose ExifTool source
requires them.

- [ ] **Step 4: Route all four standard Exif directories once**

Replace replay/drain ownership ambiguity with one generated-first route and at
most one named residual. Preserve structural walker ownership for offset,
SubIFD, MakerNote, IPTC, GeoTIFF, and PrintIM edges.

- [ ] **Step 5: Make request-aware edge behavior explicit**

An explicitly requested edge tag is not discarded merely because its normal
directory behavior is silent. Add the paired default/requested test.

- [ ] **Step 6: Run tests, fmt, Clippy, commit, and measure**

Run `cargo test --test exif_shared_pipeline`, `cargo test --lib`, formatting,
and Clippy through the Standard Candidate Acceptance Commands with:

```bash
task_slug=exif-shared-pipeline
task_target=/Users/allen/git/oxidex-beta1-targets/exif-shared-pipeline
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/exif-shared-pipeline
```

Commit the signed task candidate after tests/formatting/Clippy and before the
conformance/read-receipt portions of the standard commands.

- [ ] **Step 7: Commit, update handoff, and report**

```bash
git add src/exiftool_tables/conv/mod.rs src/exiftool_tables/ifd_engine.rs \
  src/exiftool_tables/enabled_ifd.rs src/core/exif_dir_engine.rs \
  src/core/tiff_helpers.rs src/core/jpeg_helpers.rs \
  tests/exif_shared_pipeline.rs
git commit -S -m "refactor: route Exif directories through one tag pipeline"
```

The report and `HANDOFF.md` list every remaining replay/drain/yield/residual
path and its owner, test and receipt hashes, commit SHA, and
`RETURN_TO_CONTROLLER`.

---

### Task 10: Close or Classify the 17 Exif::Main Refusals

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/10-refusal-closure.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/refusal-closure`
**Worktree:** `/Users/allen/git/oxidex-beta1-refusal-closure`
**Target:** `/Users/allen/git/oxidex-beta1-targets/refusal-closure`
**Commit:** `feat: close Exif conversion refusal ownership`

**Files:**

- Create: `tools/exiftool-tables/exif_main_refusal_worklist.json`
- Add: `tools/exiftool-tables/test_exif_main_refusal_closure.py`
- Add: `tests/exif_main_refusal_closure.rs`
- Modify: `tools/exiftool-tables/conv_codegen.py`
- Modify: `tools/exiftool-tables/test_conv_codegen.py`
- Modify: `tools/exiftool-tables/conv_oracle.py`
- Modify: `tools/exiftool-tables/helper_oracle.py`
- Modify: `tools/exiftool-tables/conv_exif_main_ledger.json`
- Modify generated: `src/exiftool_tables/conv/exif_main.rs`
- Modify: `src/exiftool_tables/conv/rt.rs`
- Modify: `src/core/tag_conversion.rs`
- Modify: `src/core/formatters/composite_image_exposure_times.rs`
- Modify: `src/core/tiff_helpers.rs`
- Modify: `src/core/jpeg_helpers.rs`
- Modify: `tests/learning_opt_out_in.rs`
- Create/modify: `tools/exiftool-tables/runtime_ownership.d/exif-main-refusals.json`
- Do not perform broad compatibility deletion

**Interfaces:** Consumes Task 1 stable ownership IDs, Task 4 generated
conversion registry, and Task 9 structural-edge ownership. The checked
worklist enumerates the 17 current `refused` rows from
`conv_exif_main_ledger.json` with source body/hash, current reason, probes,
target owner, implementation file, and named test. Its verifier requires each
row to remain refused or move to exactly one generated, walker, or residual
owner; count drift without an explicit worklist update fails.

- [ ] **Step 1: Generate a checked refusal worklist**

The worklist contains the exact source expression/body/hash, probes, current
reason, required helper/grammar/walker behavior, and target owner for all 17.

- [ ] **Step 2: Split conversion work from traversal work**

Keep `SubDirectory`, offsets, SubIFD, MakerNote, and DNG private-data edges
walker-owned. Do not model traversal as a scalar conversion.

- [ ] **Step 3: Implement exact source-supported conversion slices**

Cover, where source proof permits:

- `SetPriorityDir` and `IdentifyRawFile` behavior;
- `LearningOptOutIn` shift semantics;
- declarations/loops/rationals for `CompositeImageExposureTimes`;
- `ConvertBinary`/`PrintOpcode` for OpcodeList1/2/3; and
- declarations/loops/`sprintf` for `TimeCodes`.

Each new helper is selected by source body/hash across supported releases.

- [ ] **Step 4: Resolve residual ownership hazards**

Pin `Copyright` without IFD1 double insertion, AmbientTemperature signed zero,
XP strings through `Decode`, and every retained structural residual.

- [ ] **Step 5: Run conversion oracle, ownership verifier, and affected Rust tests**

No refusal disappears unless generated output or an explicit residual owner
replaces it. Run:

```bash
uv run python -m unittest tools/exiftool-tables/test_exif_main_refusal_closure.py -v
uv run python tools/exiftool-tables/conv_oracle.py --check --all \
  --perl /Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2 \
  --exiftool-dir /Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool
uv run python tools/exiftool-tables/runtime_ownership.py verify --root .
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/refusal-closure \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/refusal-closure/test.log -- \
  cargo test --test exif_main_refusal_closure
```

- [ ] **Step 6: Regenerate twice and verify clean**

Run `tools/exiftool-tables/regen-all.sh`, stage intended generated changes,
run it again, and require `git diff --exit-code` to show no unstaged drift.

- [ ] **Step 7: Run fmt and Clippy, commit, update handoff, and report**

```bash
git add tools/exiftool-tables/exif_main_refusal_worklist.json \
  tools/exiftool-tables/test_exif_main_refusal_closure.py \
  tools/exiftool-tables/conv_codegen.py tools/exiftool-tables/test_conv_codegen.py \
  tools/exiftool-tables/conv_oracle.py tools/exiftool-tables/helper_oracle.py \
  tools/exiftool-tables/conv_exif_main_ledger.json \
  tools/exiftool-tables/runtime_ownership.d/exif-main-refusals.json \
  src/exiftool_tables/conv/exif_main.rs src/exiftool_tables/conv/rt.rs \
  src/core/tag_conversion.rs \
  src/core/formatters/composite_image_exposure_times.rs \
  src/core/tiff_helpers.rs src/core/jpeg_helpers.rs \
  tests/learning_opt_out_in.rs tests/exif_main_refusal_closure.rs
git commit -S -m "feat: close Exif conversion refusal ownership"
```

Record the before/after 17-row classification, oracle receipts, ownership
verification, commit SHA, and `RETURN_TO_CONTROLLER` in `HANDOFF.md`.

---

### Task 11: Olympus End-to-End Pilot

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/11-olympus-pilot.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/olympus-pilot`
**Worktree:** `/Users/allen/git/oxidex-beta1-olympus-pilot`
**Target:** `/Users/allen/git/oxidex-beta1-targets/olympus-pilot`
**Commit:** `feat: complete Olympus generated runtime migration`

**Files:**

- Modify: `src/parsers/tiff/makernotes/olympus.rs`
- Modify: `src/parsers/tiff/makernotes/olympus/tables.rs`
- Modify: `src/parsers/tiff/makernotes/olympus/lookups.rs`
- Modify: `src/parsers/tiff/makernotes/olympus/text_info.rs`
- Regenerate only: `src/exiftool_tables/ifd/olympus.rs`
- Regenerate only: `src/exiftool_tables/binary/olympus.rs`
- Modify: `tests/olympus_main_ifd_table.rs`
- Modify: `tests/olympus_main_info_camera_type.rs`
- Modify: `tests/olympus_sub_tables_ifd.rs`
- Modify: `tests/integration/olympus_makernotes_tests.rs`
- Add: `tests/olympus_main_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/olympus.json`
- Create: `tools/exiftool-tables/runtime_ownership.d/olympus-deletion-candidates.json`
- Do not modify unrelated vendor parsers

**Interfaces:** Consumes the frozen Task 9 exact-once pipeline and Task 8
attribution receipt. Preserves Olympus `walk_main_through_engine`,
`main_info_directory`, `parse_camera_type_and_quality`, and subtable entry
points while moving source-expressible rows to the generated path. The
deletion-candidates fragment records old symbol, new owner, fixture, and
attribution receipt but authorizes no deletion; Task 18 alone owns the final
deletion ledger.

- [ ] **Step 1: Reproduce the remaining Olympus gaps**

Using the pinned combined corpus, capture `StackedImage`, `CameraParameters`,
`Quality`, `ZoomedPreviewImage`, and `CameraType` normal/`-n`, group, and
duplicate behavior. Record carrier files and exact oracle occurrences.

- [ ] **Step 2: Write failing generated-route tests**

Require the generated route to produce the target rows with the expected typed
and printed forms. A test that passes through the old hand path is invalid;
include generated-on/off attribution.

- [ ] **Step 3: Re-express missing source facts on the generated path**

Use source-derived tables, conditions, and conversions. Retain only the three
documented structural/post-pass cases that the generated schema cannot express,
each with an ownership record.

- [ ] **Step 4: Record Olympus-local deletion candidates without deleting**

Require an ownership candidate row, passing attribution, and occurrence-aware
oracle comparison for each symbol. Do not delete central or Olympus-local
compatibility code in this task; Task 18 performs deletion after the full
vendor wave.

- [ ] **Step 5: Test, format, verify generation, and commit the candidate**

Require target MISSING rows to become matched, zero matched-to-lost rows, zero
new VALUE rows, and read-regression `lost 0`. Run the focused carrier suite:

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/olympus-pilot \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/olympus-pilot/focused.log -- \
  cargo test --test olympus_main_forward_port -- --include-ignored --nocapture
```

Run generator verification, formatting, and Clippy, then create the signed
task commit and require a clean worktree:

```bash
git add src/parsers/tiff/makernotes/olympus.rs \
  src/parsers/tiff/makernotes/olympus \
  src/exiftool_tables/ifd/olympus.rs \
  src/exiftool_tables/binary/olympus.rs \
  tests/olympus_main_ifd_table.rs tests/olympus_main_info_camera_type.rs \
  tests/olympus_sub_tables_ifd.rs \
  tests/integration/olympus_makernotes_tests.rs \
  tests/olympus_main_forward_port.rs \
  tools/exiftool-tables/runtime_ownership.d/olympus.json \
  tools/exiftool-tables/runtime_ownership.d/olympus-deletion-candidates.json
git commit -S -m "feat: complete Olympus generated runtime migration"
```

- [ ] **Step 6: Run exclusive measurements, update handoff, and report**

Run the Standard Candidate Acceptance measurement blocks with:

```bash
task_slug=olympus-pilot
task_target=/Users/allen/git/oxidex-beta1-targets/olympus-pilot
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/olympus-pilot
```

The controller freezes the shared adapter interfaces after this task integrates.

---

### Task 12: Nikon Forward-Port on the Frozen Pipeline

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/12-nikon-port.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/nikon-port`
**Worktree:** `/Users/allen/git/oxidex-beta1-nikon-port`
**Target:** `/Users/allen/git/oxidex-beta1-targets/nikon-port`
**Commit:** `feat: forward-port remaining Nikon metadata parity`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 12`

**Files:**

- Modify: `src/parsers/tiff/makernotes/nikon.rs`
- Modify only beneath: `src/parsers/tiff/makernotes/nikon/`
- Modify: `tests/integration/nikon_makernotes_tests.rs`
- Modify: `tests/integration/makernote_integration.rs` only for Nikon cases
- Add: `tests/nikon_main_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/nikon.json`
- Do not modify shared engines, helpers, central registries, or generated output

**Interfaces:** Consumes the frozen Task 11 vendor adapter/pipeline contract.
Preserves `NikonParser` entry points and Nikon encrypted state; all new output
is emitted as canonical occurrences through the shared pipeline. The ownership
fragment enumerates each touched stable field identity and its concrete parser
symbol.

- [ ] **Step 1: Re-measure Nikon gaps at the task base**

Use the current divergence report only as a hypothesis. Confirm
`FirmwareVersion56`, `PixelShiftActive`, `Converter`, `Focus`,
`WBBracketingSteps`, AFInfo2, and remaining rows against the pinned oracle.

- [ ] **Step 2: Add failing Nikon carrier tests**

Tests must fail for the named row/value/group, not silently return when a
fixture is absent. Corpus tests are explicitly ignored with a reason and are
run using `--include-ignored` in this task.

- [ ] **Step 3: Port exact source behavior using frozen interfaces**

If a missing shared helper is required, stop that slice and request a serialized
spine task. Do not copy a vendor-local approximation.

- [ ] **Step 4: Run focused tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/nikon-port \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/nikon-port/focused.log -- \
  cargo test --test nikon_main_forward_port -- --include-ignored --nocapture
```

Run the non-measurement portion of the Standard Candidate Acceptance Commands,
create the signed task commit, and require a clean worktree.

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run both authenticated measurement blocks with the literal Nikon task values.
Require target gains, zero lost matched rows, zero new VALUE rows, and read
gate `lost 0`.

---

### Task 13: Pentax and Panasonic Forward-Port

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/13-pentax-panasonic-port.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/pentax-panasonic-port`
**Worktree:** `/Users/allen/git/oxidex-beta1-pentax-panasonic-port`
**Target:** `/Users/allen/git/oxidex-beta1-targets/pentax-panasonic-port`
**Commit:** `feat: forward-port Pentax and Panasonic metadata parity`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 13`

**Files:**

- Modify: `src/parsers/tiff/makernotes/pentax.rs`
- Modify only beneath: `src/parsers/tiff/makernotes/pentax/`
- Modify: `src/parsers/tiff/makernotes/pentax_supplement.rs`
- Modify: `src/parsers/tiff/makernotes/pentax_lens_database.rs`
- Modify: `src/parsers/tiff/makernotes/panasonic.rs`
- Modify only beneath: `src/parsers/tiff/makernotes/panasonic/`
- Modify only the Panasonic `LensType` composite arm in
  `src/composite/compute.rs`
- Modify: `tests/integration/pentax_makernotes_tests.rs`
- Modify: `tests/integration/panasonic_makernotes_tests.rs`
- Modify: `tools/exiftool-tables/fixtures/pentax_iso.json`
- Modify: `tools/exiftool-tables/fixtures/pentax_flash_mode.json`
- Modify: `tools/exiftool-tables/fixtures/pentax_af_point_selected.json`
- Add: `tests/pentax_panasonic_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/pentax-panasonic.json`
- Do not modify shared engines or registries

**Interfaces:** Consumes the Task 3 typed composite input contract and frozen
Task 11 adapter contract. Pentax and Panasonic parser entry points remain
unchanged. Only the Panasonic `LensType` match arm in
`src/composite/compute.rs` may change; Task 14 waits for this PR to merge and
may not edit that file.

- [ ] **Step 1: Re-measure family gaps**

Confirm Pentax continuous-autofocus selections/focus, external flash guide
number, city codes, ISO, Panasonic MakerNoteType/Gain, and the Panasonic
LensType composite.

- [ ] **Step 2: Add failing carrier and composite-cascade tests**

Prove LensType consumes a typed MakerNote input rather than parsing display
text. Include normal/`-n` output.

- [ ] **Step 3: Port exact source behavior**

Keep family edits within the lease. Escalate shared helper needs.

- [ ] **Step 4: Run focused tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/pentax-panasonic-port \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/pentax-panasonic-port/focused.log -- \
  cargo test --test pentax_panasonic_forward_port -- --include-ignored --nocapture
```

Run the non-measurement portion of the Standard Candidate Acceptance Commands,
create the signed task commit, and require a clean worktree.

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run both authenticated measurement blocks with the literal Pentax/Panasonic
task values. Require zero lost reads and zero new VALUE rows.

---

### Task 14: Remaining DJI, Composite, and XMP Forward-Port

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/14-dji-composite-xmp-port.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/dji-composite-xmp-port`
**Worktree:** `/Users/allen/git/oxidex-beta1-dji-composite-xmp-port`
**Target:** `/Users/allen/git/oxidex-beta1-targets/dji-composite-xmp-port`
**Commit:** `feat: complete DJI and dependent metadata parity`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 14`

**Files:**

- Modify: `src/parsers/tiff/makernotes/dji.rs`
- Modify: `src/parsers/tiff/makernotes/registries/dji.rs`
- Modify: `src/parsers/jpeg/app_segments/dji_dbg.rs`
- Modify only beneath: `src/parsers/xmp/`
- Modify: `src/parsers/jpeg/xmp_parser.rs`
- Modify: `src/parsers/pdf/xmp_extractor.rs`
- Modify: `src/composite/generated_compute.rs`
- Modify: `tests/integration/dji_app4_tests.rs`
- Modify: `tests/dji_app7_sensor_id.rs`
- Modify: `tests/dji_main_float_fields.rs`
- Add: `tests/dji_main_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/dji-composite-xmp.json`
- Do not re-port already landed DJI float rows
- Do not modify: `src/core/format_dispatch.rs`, `src/parsers/mod.rs`,
  `src/parsers/tiff/makernotes/mod.rs`, `src/composite/mod.rs`, or
  `src/composite/compute.rs`

**Interfaces:** Consumes the Task 3 typed projection contract, Task 11 frozen
adapter contract, and Task 13 merged composite ownership. Preserves existing
XMP parser entry points and the family-1 group model. Generated composite arms
receive typed inputs and return the existing `Computed` shape.

- [ ] **Step 1: Re-measure remaining DJI/XMP/composite gaps**

Separate already landed floats from `FlightSpeed`, debug tags, embedded
`XMP-drone-dji`, and true composite dependencies.

- [ ] **Step 2: Add failing group-aware tests**

Pin family-1 XMP groups, typed numeric values, and composite cascade behavior.

- [ ] **Step 3: Port source behavior on current XMP machinery**

Do not resurrect the deleted namespace mapping implementation merely to apply
an old commit.

- [ ] **Step 4: Run focused tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/dji-composite-xmp-port \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/dji-composite-xmp-port/focused.log -- \
  cargo test --test dji_main_forward_port -- --include-ignored --nocapture
```

Run the non-measurement portion of the Standard Candidate Acceptance Commands,
create the signed task commit, and require a clean worktree.

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run both authenticated measurement blocks with the literal DJI task values.
Require zero lost reads and zero new VALUE rows.

---

### Task 15: Legacy Camera Long Tail

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/15-legacy-camera-tail.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/legacy-camera-tail`
**Worktree:** `/Users/allen/git/oxidex-beta1-legacy-camera-tail`
**Target:** `/Users/allen/git/oxidex-beta1-targets/legacy-camera-tail`
**Commit:** `feat: forward-port legacy camera metadata parity`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 15`

**Files:**

- Modify: `src/parsers/tiff/makernotes/kodak.rs`
- Modify: `src/parsers/tiff/makernotes/casio.rs`
- Modify: `src/parsers/tiff/makernotes/hp.rs`
- Modify: `src/parsers/tiff/makernotes/ricoh.rs`
- Modify: `src/parsers/tiff/makernotes/jvc.rs`
- Modify: `src/parsers/tiff/makernotes/registries/kodak.rs`
- Modify: `src/parsers/tiff/makernotes/registries/casio.rs`
- Modify: `src/parsers/tiff/makernotes/registries/hp.rs`
- Modify: `src/parsers/tiff/makernotes/registries/ricoh.rs`
- Modify: `src/parsers/tiff/makernotes/registries/jvc.rs`
- Add: `tests/legacy_camera_tail_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/legacy-camera-tail.json`
- Do not modify shared engines or unrelated vendor files

**Interfaces:** Consumes the frozen Task 11 adapter contract. Each family
retains its existing MakerNote parser/registry entry point and emits canonical
occurrences. No central dispatcher or registry module is in this lease.

- [ ] **Step 1: Re-measure exact remaining rows**

Cover KodakMaker/DateTimeStamp/TimeCreated, Casio BestShotMode/ArtMode/Quality,
HP CameraDateTime/ISO, Ricoh make/model, and JVC CPUVersions/Quality.

- [ ] **Step 2: Add one real-carrier failing test per family**

In `tests/legacy_camera_tail_forward_port.rs`, add tests named
`kodak_remaining_rows`, `casio_remaining_rows`, `hp_remaining_rows`,
`ricoh_remaining_rows`, and `jvc_remaining_rows`. Each test requires its named
durable carrier and pins tag, group, raw/typed/print forms, and duplicate count
from the pinned oracle.

- [ ] **Step 3: Port source-derived values and routing**

Implement only source expressions and registry rows pinned by Step 2. If a
shared helper or central dispatch edit is required, record that slice blocked
instead of widening the lease.

- [ ] **Step 4: Run family tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/legacy-camera-tail \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/legacy-camera-tail/focused.log -- \
  cargo test --test legacy_camera_tail_forward_port -- --include-ignored --nocapture
```

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run the non-measurement portion of the Standard Candidate Acceptance Commands
before the signed commit. After the worktree is clean, run both authenticated
measurement blocks with the literal legacy-camera task values.

---

### Task 16: Samsung, MediaJukebox, and Vivo Trailers

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/16-trailer-tail.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/trailer-tail`
**Worktree:** `/Users/allen/git/oxidex-beta1-trailer-tail`
**Target:** `/Users/allen/git/oxidex-beta1-targets/trailer-tail`
**Commit:** `feat: parse remaining metadata trailers`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 16`

**Files:**

- Modify: `src/parsers/tiff/makernotes/samsung.rs`
- Modify only beneath: `src/parsers/tiff/makernotes/samsung/`
- Modify: `src/parsers/trailer.rs`
- Modify: `src/parsers/audio/ape.rs`
- Modify: `tests/integration/samsung_makernotes_tests.rs`
- Modify: `tests/integration/samsung_app5_tests.rs`
- Modify: `tests/integration/exif_makernotes_tests.rs`
- Add: `tests/trailer_tail_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/trailer-tail.json`
- Do not modify shared engines, tag comparison harnesses, or unrelated parsers

**Interfaces:** Preserves `SamsungParser`, the existing trailer scan in
`src/parsers/trailer.rs`, and the APE item parser. Adds bounded MediaJukebox
and Vivo decoding inside those owners; no central format dispatcher change is
allowed. Truncated/invalid input emits no trailer occurrence and does not
consume bytes belonging to the next trailer.

- [ ] **Step 1: Re-measure trailer detection and rows**

Confirm Samsung embedded audio name/data, MediaJukebox fields, and Vivo JSONInfo
on exact carriers.

- [ ] **Step 2: Write failing detection, bounds, and malformed-trailer tests**

Include truncated length, invalid UTF-8, absent terminator, and false-positive
controls.

- [ ] **Step 3: Implement bounded source-faithful parsing and dispatch**

Preserve occurrence order and groups. Do not accept a parser that only emits
identity tags.

- [ ] **Step 4: Run family tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/trailer-tail \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/trailer-tail/focused.log -- \
  cargo test --test trailer_tail_forward_port -- --include-ignored --nocapture
```

Run the non-measurement portion of the Standard Candidate Acceptance Commands,
create the signed task commit, and require a clean worktree.

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run both authenticated measurement blocks with the literal trailer task
values. Require zero lost reads and zero new VALUE rows.

---

### Task 17: Consolidate Walker Conversion Stages

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/17-walker-engine-consolidation.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/walker-engine-consolidation`
**Worktree:** `/Users/allen/git/oxidex-beta1-walker-engine-consolidation`
**Target:** `/Users/allen/git/oxidex-beta1-targets/walker-engine-consolidation`
**Commit:** `refactor: share metadata conversion across walkers`

**Files:**

- Modify: `src/exiftool_tables/engine.rs`
- Modify: `src/exiftool_tables/ifd_engine.rs`
- Modify: `src/exiftool_tables/keyed_engine.rs`
- Modify: `src/exiftool_tables/serial_engine.rs`
- Create: `src/exiftool_tables/pipeline.rs`
- Modify: `src/exiftool_tables/mod.rs`
- Create: `docs/reference/generated-runtime-walker-inventory.json`
- Add: `tests/generated_runtime_walker_contract.rs`
- Do not delete compatibility code in this task

**Interfaces:** Acquisition adapters produce
`PipelineInput { identity: StableFieldIdentity, stored: MemberVal, groups:
Groups, provenance: Provenance }` plus `&mut Session`; `pipeline::execute`
owns condition, RawConv, ValueConv, and PrintConv stages and returns the
existing engine-specific emitted shape through an adapter callback. Numeric,
keyed-name, and serial-index identities remain distinct. This task may not
edit vendor adapter files; all Tasks 12-16 are remotely merged before it
starts, and only the four engine modules call the new shared pipeline.

- [ ] **Step 1: Inventory walkers, duplicated stages, and adapter contracts**

Record every enabled hand walker with its ExifTool source callback, acquisition
kind, supported releases, current conversion path, target shared entry point,
real carrier fixture, and detected-versus-parsed status. Then write adapter
contract tests.

Cover IFD numeric key, keyed name, serial index, binary bytes, list, group,
unknown, request filtering, and an encrypted walker. Each fixture must produce
the same occurrence shape through the shared contract.

- [ ] **Step 2: Run focused tests red against a shared entry point**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/walker-engine-consolidation \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/walker-engine-consolidation/red.log -- \
  cargo test --test generated_runtime_walker_contract -- --nocapture
```

Expected: shared entry point is absent and engines still duplicate stages.

- [ ] **Step 3: Extract the smallest shared pipeline**

Do not force unlike byte acquisition into one decoder. Preserve keyed/serial
identity without coercing it to an IFD ID.

- [ ] **Step 4: Convert engines one at a time**

After each engine adapter, run its focused tests. Keep each intermediate commit
buildable for review even though integration will squash the task. If an
engine requires a vendor-local edit, record the missing adapter and return that
slice to the controller rather than widening this lease.

- [ ] **Step 5: Add route-level detected-versus-parsed controls**

For JPEG/QuickTime, RIFF/audio, PDF/OLE/ZIP, TIFF/MakerNotes, and one keyed and
serial carrier, require real metadata beyond FileType identity tags.

- [ ] **Step 6: Run tests, fmt, Clippy, commit, and measure**

Run the focused contract suite and the Standard Candidate Acceptance Commands
with:

```bash
task_slug=walker-engine-consolidation
task_target=/Users/allen/git/oxidex-beta1-targets/walker-engine-consolidation
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/walker-engine-consolidation
```

Create the signed task commit before any authenticated measurement block and
require a clean worktree.

- [ ] **Step 7: Update handoff and report**

Report the remaining walker-specific conversion code and why it cannot yet use
the shared stage.

---

### Task 18: Delete Proven-Replaced Compatibility Code

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/18-proven-deletion.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/proven-deletion`
**Worktree:** `/Users/allen/git/oxidex-beta1-proven-deletion`
**Target:** `/Users/allen/git/oxidex-beta1-targets/proven-deletion`
**Commit:** `refactor: remove metadata paths replaced by generated runtime`

**Files:**

- Modify: `src/core/exiftool_compat.rs`
- Modify: `src/core/exif_dir_engine.rs`
- Modify: `src/core/tiff_helpers.rs`
- Modify: `src/core/jpeg_helpers.rs`
- Modify: `src/exiftool_tables/engine.rs`
- Modify: `src/exiftool_tables/ifd_engine.rs`
- Modify: `src/exiftool_tables/keyed_engine.rs`
- Modify: `src/exiftool_tables/serial_engine.rs`
- Create: `docs/reference/generated-runtime-deletion-ledger.json`
- Create: `tools/exiftool-tables/runtime_deletion_ledger.py`
- Create: `tools/exiftool-tables/test_runtime_deletion_ledger.py`
- Add: `tests/generated_runtime_deletion_controls.rs`
- Modify: `justfile` to add `verify-runtime-deletions`

**Interfaces:** Consumes Task 1 ownership rows, Task 8 generated-on/off
receipts, Task 10 refusal closure, Task 11 deletion candidates, Tasks 12-16
vendor ownership fragments, and Task 17 shared pipeline. The deletion verifier
accepts only literal `path::symbol` entries whose new owner exists and whose
authenticated candidate/source/binary hashes match the current clean commit.
It produces `verify-runtime-deletions` and refuses structural traversal owners.

- [ ] **Step 1: Generate the deletion candidate ledger**

Each entry contains:

```json
{
  "old_symbol": "path::symbol",
  "source_fields": ["Exif::Main:0x9400"],
  "new_owner": "generated or named residual symbol",
  "oracle_receipt": "/absolute/durable/receipt.json",
  "attribution_receipt": "/absolute/durable/receipt.json",
  "generated_on": "matched",
  "generated_off": "missing-or-residual",
  "deletion_commit": null
}
```

- [ ] **Step 2: Write failing verifier and generated-off tests**

Reject deletion entries without both receipts, stale binary hashes, mismatched
source fields, or a still-live duplicate owner. Run:

```bash
uv run python -m unittest tools/exiftool-tables/test_runtime_deletion_ledger.py -v
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/proven-deletion \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/proven-deletion/red.log -- \
  cargo test --test generated_runtime_deletion_controls -- --nocapture
```

- [ ] **Step 3: Delete branches in small reviewed groups**

Order:

1. Exif::Main hand conversion arms;
2. XP/AmbientTemperature/Copyright paths now proven generated or residual;
3. `exiftool_compat.rs` post-hoc formatters;
4. replay/drain/double-insert/yield shims;
5. duplicate maps and engine conversion stages.

After each group, run its focused tests and attribution control. Never delete
structural traversal required for pointers, MakerNotes, IPTC, GeoTIFF, PrintIM,
encryption, or container walking.

- [ ] **Step 4: Add the no-new-manual-tag-knowledge gate**

The gate rejects a new hand owner for a source construct supported by the
generator and rejects residual entries whose source identity is now generated.

- [ ] **Step 5: Test, verify, commit, and run attribution**

Use shared locks for tests and exclusive lock for attribution. Run:

```bash
uv run python tools/exiftool-tables/runtime_ownership.py verify --root .
uv run python tools/exiftool-tables/runtime_deletion_ledger.py verify --root .
```

Then run `cargo test --test generated_runtime_deletion_controls` and the
Standard Candidate Acceptance Commands with:

```bash
task_slug=proven-deletion
task_target=/Users/allen/git/oxidex-beta1-targets/proven-deletion
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/proven-deletion
```

Run the authenticated generated-attribution control/probe from Task 8 against
the committed clean candidate under the exclusive lock. Create that signed
commit after all ordinary tests/formatting/Clippy and before attribution. Every
deletion ledger receipt must name this candidate's source and binary hashes.

- [ ] **Step 6: Update handoff and report**

Report deleted manual rules separately from source-line changes and list every
retained compatibility symbol with its reason.

---

### Task 19: Qualify ExifTool Version Transitions

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/19-version-transition-qualification.md`

**Worker:** Codex CLI, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/version-transition-qualification`
**Worktree:** `/Users/allen/git/oxidex-beta1-version-transition-qualification`
**Target:** `/Users/allen/git/oxidex-beta1-targets/version-transition-qualification`
**Commit:** `test: prove reversible ExifTool version regeneration`

**Launch:**
`python3 tools/release/fleet_controller.py launch --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller --repo /Users/allen/git/oxidex-beta1-functional-integration --task 19`

**Files:**

- Create: `tools/exiftool-tables/version_transition_qualification.py`
- Create: `tools/exiftool-tables/test_version_transition_qualification.py`
- Create: `tools/exiftool-tables/version_transition_matrix.json`
- Modify: `tools/exiftool-tables/version_rehearsal.py`
- Modify: `tools/exiftool-tables/version_rehearsal_executor.py`
- Modify: `tools/exiftool-tables/version_rehearsal_stage_adapter.py`
- Modify: `tools/exiftool-tables/version_rehearsal_native_oracle.py`
- Modify: `tools/exiftool-tables/version_rehearsal_catalog.py`
- Modify: `tools/exiftool-tables/test_version_rehearsal.py`
- Modify: `tools/exiftool-tables/test_version_rehearsal_executor.py`
- Modify: `tools/exiftool-tables/test_version_rehearsal_stage_adapter.py`
- Modify: `tools/exiftool-tables/test_version_rehearsal_native_oracle.py`
- Modify: `tools/exiftool-tables/test_version_rehearsal_catalog.py`
- Modify: `docs/reference/upgrade-rehearsal-11.78-12.64.md`
- Modify: `docs/UPGRADE-NEXT-STEPS.md`
- Do not manually edit generated artifacts or change the release pin permanently

**Interfaces:** Wraps the existing rehearsal planner, executor, stage adapter,
native oracle, and catalog modules without duplicating their stage logic. The
matrix schema requires `id`, `before_version`, `after_version`, immutable
source identities, read fixture manifest, mandatory write fixture/readback
manifest, artifact manifest, target directory, and durable output directory.
Every result records restoration of `.exiftool-version`, generated artifacts,
and caller cleanliness.

- [ ] **Step 1: Write failing matrix and restoration tests**

In `test_version_transition_qualification.py`, require all three matrix rows,
fresh generation on both sides, mandatory native write/readback, distinct
targets, immutable source hashes, interrupted-stage recovery, original pin
restoration, and an empty tracked diff. Use fakes for unit tests; no network or
full corpus run belongs in the red/green loop.

- [ ] **Step 2: Run the qualification tests red**

```bash
uv run python -m unittest \
  tools/exiftool-tables/test_version_transition_qualification.py -v
```

Expected: import failure because the qualification module and matrix do not
exist.

- [ ] **Step 3: Add concrete rehearsal configurations**

Create checked configurations for:

- same-pin 13.59 -> 13.59;
- 11.78 -> 12.64; and
- 12.64 -> 11.78.

Every stage requires native source hash, version/DOCX probes, explicit
read fixtures, mandatory write fixtures/readback, artifact manifest, separate
target directory, and durable result path.

`version_transition_qualification.py` is the single non-promoting entry point:

```bash
python3 tools/exiftool-tables/version_transition_qualification.py \
  --matrix tools/exiftool-tables/version_transition_matrix.json \
  --repository /Users/allen/git/oxidex-beta1-version-transition-qualification \
  --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification
```

It calls the existing planner/executor/stage adapter, refuses an unclean
caller, runs all three matrix rows, and restores/verifies the caller without a
promotion action.

- [ ] **Step 4: Make tests release-aware**

Replace hard-coded 13.59 facts responsible for the historical 113 failures on
11.78 and 79 failures on 12.64 with pinned per-release facts. Do not weaken a
generic assertion to accommodate drift.

- [ ] **Step 5: Add hand-behavior retention controls**

An older-release run fails if newer hand behavior silently retains tags absent
from that release. Generated refusal is acceptable only when explicit and
counted.

- [ ] **Step 6: Commit a clean candidate and run same-pin exclusively**

Require both variants freshly generated, second regeneration clean, identical
pin/artifact manifest, and no tracked caller diff. Invoke the qualification
entry point through the exclusive lock:

```bash
git add tools/exiftool-tables/version_transition_qualification.py \
  tools/exiftool-tables/test_version_transition_qualification.py \
  tools/exiftool-tables/version_transition_matrix.json \
  tools/exiftool-tables/version_rehearsal*.py \
  tools/exiftool-tables/test_version_rehearsal*.py \
  docs/reference/upgrade-rehearsal-11.78-12.64.md
git commit -S -m "test: prove reversible ExifTool version regeneration"
test -z "$(git status --short)"
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification/same-pin.lock.log -- \
  python3 tools/exiftool-tables/version_transition_qualification.py \
    --matrix tools/exiftool-tables/version_transition_matrix.json \
    --repository /Users/allen/git/oxidex-beta1-version-transition-qualification \
    --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification \
    --only same-pin-13.59
```

If the worktree is not clean after the signed commit, do not start the
rehearsal.

- [ ] **Step 7: Run 11.78 -> 12.64 without intervention**

Require generation, verification, native read conformance, write/readback,
manifest delta, and recovery controls. No code or fixture edit is allowed after
the run starts. Invoke the same entry point with `--only 11.78-to-12.64`.

```bash
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification/forward.lock.log -- \
  python3 tools/exiftool-tables/version_transition_qualification.py \
    --matrix tools/exiftool-tables/version_transition_matrix.json \
    --repository /Users/allen/git/oxidex-beta1-version-transition-qualification \
    --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification \
    --only 11.78-to-12.64
```

- [ ] **Step 8: Run 12.64 -> 11.78 without intervention**

Apply the same gates and prove removed artifacts are handled only by manifest
delta. Invoke the same entry point with `--only 12.64-to-11.78`.

```bash
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification/reverse.lock.log -- \
  python3 tools/exiftool-tables/version_transition_qualification.py \
    --matrix tools/exiftool-tables/version_transition_matrix.json \
    --repository /Users/allen/git/oxidex-beta1-version-transition-qualification \
    --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification \
    --only 12.64-to-11.78
```

- [ ] **Step 9: Restore/verify the 13.59 caller and run all rehearsal tests**

```bash
uv run python -m unittest \
  tools/exiftool-tables/test_upgrade_transaction.py \
  tools/exiftool-tables/test_version_rehearsal.py \
  tools/exiftool-tables/test_version_rehearsal_catalog.py \
  tools/exiftool-tables/test_version_rehearsal_executor.py \
  tools/exiftool-tables/test_version_rehearsal_native_oracle.py \
  tools/exiftool-tables/test_version_rehearsal_stage_adapter.py \
  tools/exiftool-tables/test_version_transition_qualification.py -v
git diff --exit-code
test "$(tr -d '\n' < .exiftool-version)" = "13.59"
```

- [ ] **Step 10: Update handoff, commit receipts/documentation, and report**

The source tree must end at its original pin and clean state.

---

### Task 20: Freeze and Qualify the Functional Candidate

**PRD:** `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/20-frozen-candidate-evidence-r1.md`
**Worker:** Controller-owned; no implementation worker
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/frozen-candidate-evidence-r1`
**Worktree:** `/Users/allen/git/oxidex-beta1-frozen-candidate-evidence-r1`
**Target:** `/Users/allen/git/oxidex-beta1-targets/final-r1`
**Commit:** `docs: record beta functional qualification evidence`

**Files:**

- Modify only after gates pass: `TODO_RELEASE_BETA.md`
- Modify: `docs/AUTOGENERATION-PLAN.md`
- Modify: `docs/AUTOGENERATION-V2-DESIGN.md`
- Modify: `docs/UPGRADE-NEXT-STEPS.md`
- Modify: `docs/reference/upgrade-rehearsal-11.78-12.64.md`
- Modify: `docs/reference/main-divergence-2026-09-18.md`
- Modify: `docs/reference/tag-coverage-analysis.md`
- Modify: `docs/reference/parity-rollup-review-20260914.md`
- Modify: `docs/contributing/measuring-coverage.md`
- Modify: `docs/guide/exiftool-parity.md`
- Modify: `.agents/skills/exiftool-parity/SKILL.md`
- Modify only authenticated outputs beneath: `docs/public/measurements/`
- No runtime writer may change the frozen candidate during this task

**Interfaces:** Consumes the controller `receipt-index.json`, Tasks 1/18
ownership/deletion verifiers, Task 6 conformance schema, Task 8 attribution
receipt, Task 19 transition receipts, and the exact frozen candidate SHA. It
produces factual documentation only; public measurement JSON must retain the
instrument/source/binary/corpus hashes from authenticated receipts and may not
be recomputed from prose or formatted output.

- [ ] **Step 1: Freeze and record the candidate**

After Task 19 is remotely merged, stop all implementation workers and acquire
the integration merge lease. Fetch both
`origin/staging/beta1-functional-integration` and
`origin/refactor/tag-machinery`. Rebase the clean controller-owned integration
branch onto the exact current `origin/refactor/tag-machinery` SHA, rerun its
fast structural smoke tests, and update only that controller-owned remote with
`--force-with-lease` against the previously recorded integration SHA. Record
the target SHA as `qualified_target_sha`; do not create Task 20 until the
updated local and remote integration SHAs agree.

Create the named Task 20 worktree and branch from that exact synchronized
integration SHA. Record the full SHA, `qualified_target_sha`, clean state,
qualification round, branch, binary path/hash, pin, pinned source hash, Perl
hash/version, corpus roots/counts, and lock status. Do not run these gates in
the controller integration mirror.

Round 1 uses the literal paths in this task header and commands. For every
later positive integer `N`, the controller materializes all of these before
launch and refuses a collision:

```text
branch:    staging/beta1/frozen-candidate-evidence-rN
worktree:  /Users/allen/git/oxidex-beta1-frozen-candidate-evidence-rN
target:    /Users/allen/git/oxidex-beta1-targets/final-rN
evidence:  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-rN/
PRD:       /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/prds/20-frozen-candidate-evidence-rN.md
report:    /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/reports/20-frozen-candidate-evidence-rN.md
review:    /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/reviews/20-frozen-candidate-evidence-rN/
process:   /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/processes/20-rN/
receipts:  receipt-index.json key task-20/round-N
```

The materializer replaces `N` with the ledger's next integer in every command,
including `task_slug=final-rN`. Prior-round worktrees, targets, logs, PRDs,
reports, reviews, and receipts become read-only `superseded` evidence; they are
never deleted, reused, or overwritten before the release is complete.

- [ ] **Step 2: Run clean full regeneration twice**

```bash
mkdir -p /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1
test -z "$(git status --short)"
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final-r1 \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1/regen-1.log -- \
  tools/exiftool-tables/regen-all.sh
python3 tools/exiftool-tables/artifacts.py diff --tier all
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final-r1 \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1/regen-2.log -- \
  tools/exiftool-tables/regen-all.sh
python3 tools/exiftool-tables/artifacts.py diff --tier all
test -z "$(git status --short)"
```

Both diffs and the final status must be empty.

- [ ] **Step 3: Run formatting, lint, and complete tests**

Run the repository's CI recipe, the complete ignored suite, and doctests under
the shared lock. Store separate logs and exit statuses:

```bash
cargo fmt --check
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final-r1 \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1/ci.log -- \
  just ci
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final-r1 \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1/ignored.log -- \
  cargo test --release --workspace --all-features -- --include-ignored
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final-r1 \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1/doc.log -- \
  cargo test --doc --workspace --all-features
```

- [ ] **Step 4: Run typed-value and ownership gates**

Run the normal/`-n` oracle matrix, runtime ownership verifier, no-new-manual
knowledge gate, and deletion ledger verifier:

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final-r1 \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1/typed-values.log -- \
  cargo test --test typed_value_projection_tests
uv run python tools/exiftool-tables/runtime_ownership.py verify --root .
uv run python tools/exiftool-tables/runtime_deletion_ledger.py verify --root .
```

The Task 18 `just` recipe for no-new-manual knowledge also runs here; its exact
recipe name is `verify-runtime-deletions`.

- [ ] **Step 5: Run combined occurrence conformance exclusively**

Run the Standard Candidate Acceptance conformance block with:

```bash
task_slug=final-r1
task_target=/Users/allen/git/oxidex-beta1-targets/final-r1
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1
```

Record matched, MISSING, VALUE, EXTRA, RENAME, ceiling, files, and raw oracle
occurrences. Require at least 4,000 files and 400,000 oracle tags, plus the raw
occurrence floor recorded by Task 6.

- [ ] **Step 6: Run read regression and generated attribution exclusively**

Run the Standard Candidate Acceptance read-receipt block using the final values
above, then:

```bash
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1/genshare.log -- \
  tools/exiftool-tables/genshare/census.sh \
    --repository /Users/allen/git/oxidex-beta1-frozen-candidate-evidence-r1 \
    --target-dir /Users/allen/git/oxidex-beta1-targets/final-r1 \
    --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final-r1/genshare \
    --corpus /Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples \
    --perl /Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2 \
    --exiftool-dir /Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool \
    --tokens engine,legacy-l1,legacy-l2,producers,serial,keyed
```

Require read-regression `lost 0`, no public failures, authenticated generated
control/probe receipts, and reconciled per-occurrence deltas.

- [ ] **Step 7: Re-verify all version-transition receipts**

Validate same-pin, forward, reverse, native read, and native write/readback
receipts against the frozen code/tool hashes. If `TODO_RELEASE_BETA.md` names
an intended ExifTool pin after 13.59, run that current-to-next matrix row with
the same Task 19 entry point; otherwise record `not selected` rather than
inventing or silently skipping a version.

- [ ] **Step 8: Refresh main divergence**

Fetch without editing protected branches. Classify every remaining case that
`main` matches and the candidate does not as ported, superseded, intentionally
removed, or unresolved. Any unresolved user-visible regression blocks the
functional program.

- [ ] **Step 9: Update release ledger and documentation**

Record exact receipts and results in `TODO_RELEASE_BETA.md`. Update
`docs/AUTOGENERATION-PLAN.md`, `docs/AUTOGENERATION-V2-DESIGN.md`, rehearsal
documentation, and `.agents/skills/exiftool-parity/SKILL.md` so commands and
claims match reality.

- [ ] **Step 10: Commit the evidence-only update**

```bash
git add TODO_RELEASE_BETA.md docs .agents/skills/exiftool-parity/SKILL.md
git commit -S -m "docs: record beta functional qualification evidence"
```

- [ ] **Step 11: Dispatch final whole-branch review**

Build a review package from the Task 20 base through the committed candidate
HEAD. Give the Astra reviewer the spec, plan, controller ledger,
deferred minors, parked rulings, receipts index, documentation updates, and
full diff. One consolidated fix worker and one scoped re-review are allowed if
findings remain. Any runtime fix invalidates Steps 1-8 and requires a complete
rerun; a documentation-only fix reruns formatting, typos, links, and the
scoped review.

```bash
bash /Users/allen/.codex/plugins/cache/openai-curated-remote/superpowers/6.4.1/skills/subagent-driven-development/scripts/review-package \
  docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md \
  "$task_base" "$(git rev-parse HEAD)"
```

- [ ] **Step 12: Land the frozen-candidate evidence through its remote PR**

Apply the Local Checkpoint, Remote PR, and Integration Procedure to
`staging/beta1/frozen-candidate-evidence-r1`. Push the signed checkpoint, open the
draft PR against `staging/beta1-functional-integration`, attach the authenticated receipt
index, resolve the final Astra review, wait for every required CI check, and
squash-merge. Fetch the merge and fast-forward the controller integration
mirror. Preserve the task worktree and remote branch until the post-merge
evidence links and target SHA are recorded.

- [ ] **Step 13: Land the reviewed integration branch into the protected target**

Open one final PR from `staging/beta1-functional-integration` to
`refactor/tag-machinery`, attach the complete receipt index, obtain a fresh
whole-branch Astra review, and wait for every required check. Before marking it
ready, require an atomic GitHub up-to-date guarantee: either strict required
status checks on `refactor/tag-machinery` or an applicable merge-queue rule.
The controller verifies one of them live:

```bash
set -o pipefail
atomic_guard=0
if gh api repos/swack-tools/oxidex/branches/refactor%2Ftag-machinery/protection/required_status_checks \
  --jq .strict | rg -x true; then atomic_guard=1; fi
if gh api repos/swack-tools/oxidex/rules/branches/refactor%2Ftag-machinery \
  --jq '[.[] | select(.type == "merge_queue")] | length' | rg -v '^0$'; then atomic_guard=1; fi
test "$atomic_guard" -eq 1
```

If neither rule exists, record a blocker and do not merge; never substitute a
check-then-merge shell sequence.

Immediately before the final review package and again after required checks,
fetch `origin/refactor/tag-machinery` and compare it to
`qualified_target_sha`. If it changed, do not merge: increment the
qualification round, rebase the controller-owned integration branch onto that
exact target with a recorded `--force-with-lease`, create
`staging/beta1/frozen-candidate-evidence-rN`, and repeat Task 20 Steps 1-12 in
full. Prior corpus, transition, attribution, read-regression, documentation,
and review receipts are stale for the combined tree and may not be reused.

Only when the twice-checked target equals `qualified_target_sha` and the atomic
guard is live may the controller mark the final PR ready and use GitHub's
protected merge/queue path with `--match-head-commit`. Fetch the resulting
target SHA, verify the PR and merge commit, and only then record the functional
program complete.

The result is ready for the separate `main` reconciliation, packaging, CI/CD,
signing, notarization, and tagging portions of `TODO_RELEASE_BETA.md`. This plan
does not authorize those actions.

## Execution Handoff

Use the controller model described in the spec:

1. Push this plan branch, review it, and squash-merge its PR into
   `refactor/tag-machinery`.
2. Create the integration worktree and controller ledger from the verified
   remote target.
3. Manually bootstrap Task 0, then execute, supervise, review, push, and
   remotely merge it; require its durable controller/oracle verification and
   total-process-loss rehearsal before creating any other task worktree.
4. Dispatch the initial five implementation tasks exactly as listed, keeping
   at most three Desktop and six CLI workers active.
5. Preserve every meaningful clean checkpoint locally, then have the
   controller push it and update the task's draft PR.
6. Rebase and re-review overlapping returns; require fresh review and all CI
   before each squash merge into `staging/beta1-functional-integration`.
7. Fetch and fast-forward the controller mirror after every verified remote
   merge, then release dependent tasks.
8. Persist every local and remote state transition in task handoffs, the
   durable controller ledger/event stream, remote branches/PRs, and receipt
   index. Watch CLI PID/start-time/session/log/heartbeat state and resume or
   reconcile any dead/quota-exhausted worker before releasing dependencies.
9. Do not merge to `main`, publish, or tag until the separate release phase is
   authorized and all remaining `TODO_RELEASE_BETA.md` gates are satisfied.

The chosen execution method is the user-requested hybrid: Superpowers-reviewed
Desktop subagents plus parallel `codex --yolo exec` workers, with the main
session acting as integration controller.
