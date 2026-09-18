@AGENTS.md

`AGENTS.md` carries the substance — build commands, repo layout, and the
instrument-truth doctrine that governs every measurement here. This file holds
only the workflow rules that sit on top of it.

## How work lands

Work does **not** reach `main` through PRs from this tree. The flow is:

1. One agent, one worktree, one `staging/<slug>` branch off the current tip.
   Run `tools/preflight.sh --upstream` first.
2. Verify with the named instrument for the change (corpus comparison for tag
   work) — see AGENTS.md. A change that touches readers must show 0 proven
   reads lost (`tools/ci/read_regression_gate.py`, which CI also runs on
   every PR).
3. Open a PR against `refactor/tag-machinery`, name the instrument beside
   every number, and let CI run: build/tests, lint, generated-table
   verification, the corpus read-regression gate and the parity ratchet.
4. Squash-merge once CI is green **and** the central claim has been
   verified independently of the agent that made it — re-run its instrument,
   don't trust its report. The maintainer may waive the CI wait explicitly.

(`tools/fleet/gate.sh` belonged to the multi-host fleet, which is stopped; it
cannot run without the fleet's verdict hub. Heavy local jobs take the shared
measurement lock instead — see `docs/AUTOGENERATION-PLAN.md`, "How the work
is run".)

**`main` is the maintainer's decision alone.** Never push to it, merge into it,
or rebase onto it. `refactor/tag-machinery` is where refactor work lives and is
currently far ahead of `main`; both carry active rulesets (`main`, `tip-guard`,
`rescued-guard`, `proof-guard`) that will reject a force-push or deletion.

`just ci-standard` (justfile) runs the same checks CI does. Locally, at minimum:
`cargo fmt --all --check && cargo clippy --release --all-features -- -D warnings
&& cargo test --workspace`.

## Parallel sessions / fast-moving tip

Before starting, and again before merging, run `git fetch origin && git rebase
origin/refactor/tag-machinery` — **the tip, not `origin/main`**. Other sessions
land competing fixes frequently. If the branch's scope shrinks to near-zero
after rebase, say so and stop rather than shipping an empty change. Re-verify
that the defect still reproduces at that HEAD before implementing a fix; if
upstream already contains it, report it superseded.

## Worktrees

This repo is worked on via git worktrees. Run `tools/preflight.sh` before the
first edit: it prints the worktree root, whether this is the main checkout or a
linked worktree, the branch, and the uncommitted-file count, and exits non-zero
on a protected branch or a dirty tree. Never edit the main checkout while
operating from a worktree, and never edit a worktree another agent owns —
several agents sharing one tree is not hypothetical here.

```
git -C <repo> worktree add -b staging/<slug> /Users/allen/git/<dir> <base>
tools/preflight.sh --upstream    # adds a base-freshness check
```

## ExifTool parity

ExifTool is pinned at **13.59** (`.exiftool-version`). Validate every tag change
against the pinned oracle across the corpus — not just unit tests — and report
before/after WRONG counts with the instrument named. Never invoke a bare
`exiftool`; assert both probes first (`-ver` → 13.59, and the `OOXML.docx`
capability probe → `DOCX`). See AGENTS.md for why a matching `-ver` alone is not
a working oracle.

## Build gotchas

`cargo test --workspace --release` **fails at base** with ~111–138 bogus
"requires panic strategy `abort` vs `unwind`" / "multiple different versions of
crate `chrono`" errors. It is an output filename collision at
`target/release/deps/liboxidex.rlib`, tripped when `cargo clippy --all-features`
and the test suite share one target dir. It reproduces on an unmodified base
commit, so it is never your change. Clear it with:

```
cargo clean --release -p chrono -p oxidex
```

Working substitutes: `cargo test --workspace` and `cargo test --lib --release`.

## Remote ops safety

Before editing a config file inside a container or service, **stop it first** —
edits to a running container are silently reverted on shutdown, and an edit that
vanished on restart looks exactly like an edit that was never made. Confirm the
change persisted after the restart.

Never apply a manifest containing literal `PLACEHOLDER` values: a template
applied verbatim fails in whichever direction is hardest to see.

*Dormant (no live target as of 2026-08-28):* the Kubernetes rules — never delete
a ServiceAccount, RBAC object, or namespace without first listing every workload
that references it and printing a dry-run plan for approval. Kept because they
are incident-derived; the cluster they applied to is gone (the pod host left the
fleet, and `kubectl config current-context` no longer parses). Re-arm these
before any future cluster work.
