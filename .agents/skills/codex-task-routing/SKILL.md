---
name: codex-task-routing
description: Choose models for OxiDex Codex tasks and run a local review-and-repair cycle before pushing a candidate or update to GitHub.
---

# Codex task routing and local review

Read this skill before delegating Codex work or pushing a candidate. Keep the
root session's user-selected model unchanged. Set a worker's model and effort
explicitly; do not inherit Astra/max for routine work.

| Task | Model | Effort |
| --- | --- | --- |
| Read-only inventory or result summary | gpt-6-luna | low |
| Bounded implementation or scoped review | gpt-6-sol | medium |
| Complex parser or metadata-write repair | gpt-6-sol | high |
| Architecture, security, composed write-core acceptance, release judgment | gpt-6-astra | high |

Escalate with a concrete reason, using max only when the difficulty warrants it.
Do not use fast mode. Prefer scripts for status polling and data collection.
Provide a small brief with the contract, relevant paths, tests, and evidence;
avoid full-history forks. Native subagents need explicit `model`,
`reasoning_effort`, and a limited context fork. Respect existing delegation
permission and give each implementer its own worktree and branch. Use native
desktop agents first (up to three workers); use CLI workers for additional
independent work. Both routes require the explicit model and effort above.
Execute workers and builds directly on this laptop, without SSH, fleet or hub.
Do not delegate unless the user or applicable instructions authorize it.

## Before the first push

1. Fetch the intended base and reproduce the defect there. Implement and test
   the candidate, then commit it locally. Preserve the finding ledger.
2. Run local Codex review against the full candidate diff from the intended
   base. Use Sol/medium for bounded work; use a final Astra review for composed
   write-core changes after focused findings are clear.
3. Read the review, validate each actionable finding, repair it locally, and
   add a reproducing regression where appropriate. Run affected checks.
4. Commit the repairs and review the changed portion again. Repeat until the
   candidate has no unresolved actionable findings. After two repair rounds,
   diagnose a common cause or split the scope before starting another round.
5. Push only the reviewed candidate. Local review helps anticipate GitHub
   Codex feedback. It may satisfy the review gate only when the maintainer
   explicitly authorizes a fallback for unavailable GitHub review, the
   exact-head local result has been inspected, and all actionable findings
   are disposed. Fresh CI and the complete, zero-actionable GitHub review
   thread gate still apply. Preserve GitHub's actual review decision.

For later pushes, review and repair the delta from the last reviewed commit
before pushing. Review the full candidate again when its base or a shared
safety boundary changes. A new head invalidates the prior head's acceptance.
Record both the full review and subsequent delta reviews in the finding ledger.

## Launcher

From the candidate checkout:

```bash
python3 tools/codex_task.py review --base origin/refactor/tag-machinery
python3 tools/codex_task.py review --base <last-reviewed-commit> --brief <focus-file>
python3 tools/codex_task.py acceptance --base origin/refactor/tag-machinery
python3 tools/codex_task.py implementation --brief <task-file>
python3 tools/codex_task.py inventory --brief <task-file> --dry-run
```

`parser` selects Sol/high. `--model` or `--effort` requires `--reason`.
`--dry-run` prints the command without making a model call. Runs default to a
30-minute limit and write a receipt, result, events, and errors below
`$OXIDEX_OPS_DIR/evidence/codex` (default `~/oxidex-ops`). `--output-dir` must be
absolute, new, outside the checkout, and beneath `OXIDEX_OPS_DIR` on durable
storage. Temporary paths and paths through symlinks are rejected. Write roles
require a clean linked worktree on a `staging/` branch. The caller must confirm
that no other worker owns that checkout.

The helper invokes `codex exec review` for review roles with a read-only
sandbox, explicit `model` and `review_model`, and an immutable base SHA. It
ignores user configuration and disables plugins and hooks for that child
invocation to avoid unrelated startup work; authentication still uses Codex's
normal credential store. Read-only review and inventory launches explicitly use
`--ask-for-approval never` and `--sandbox read-only`. User-authorized
implementation and parser launches use `codex --yolo exec`, with no sandbox
option: they need external operations evidence, isolated targets and sccache.
This bypasses sandbox and approval prompts for that local worker; authorize a
bounded brief and exclusive checkout before launching it. Project instructions
still apply. The helper does
not modify the user's configuration or change the parent session.

A receipt with `completed` means the process finished. Its approval remains
`not_assessed`: inspect `result.md` and disposition findings in the ledger.
Failed, stale, interrupted, or missing-result runs cannot support a push.
Never infer acceptance from exit code zero alone. Review receipts do not waive
repository CI, unresolved-thread, or independent behavior-verification gates.

Keep one active published candidate per dependency cluster. Local CPU concurrency
may be high: choose configurable worker counts and `CARGO_BUILD_JOBS` from the
available cores and memory, and lower them when memory pressure appears. Use a
separate `CARGO_TARGET_DIR` for each worktree. Reserve the laptop exclusively for
corpus timing measurements; finish or pause competing builds first.
Update HANDOFF.md and record review rounds, routing reasons, and available token
usage. Missing or all-zero CLI usage is recorded as unknown, not free work.
The launcher stops its child process group on timeout, `SIGTERM`, `SIGHUP`,
or `SIGINT` and records the interrupted run.
A SubagentStart hook cannot choose a model before that subagent starts;
use explicit launch arguments rather than relying on an advisory hook.
