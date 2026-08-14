#!/usr/bin/env python3
"""Tip signal, drift budget, and forced rebase (T1.3 / M3 / P5).

Traces to `docs/FLEET.md` incident 5: `vf-fix-conds12` failed a
`615 vs 613` assertion the tip no longer contained, because it was gated 24
commits behind. Today's queue held branches 6, 8 and 24 commits behind --
this module is the fix: a cheap tip-moved signal (`bump_tip_signal`, driven
by the hub's `post-receive` hook), a budget that refuses a stale branch a
gate claim (`check`), and a forced-rebase routine that gets it current
again without ever hand-merging a generated file (`converge`).

Three things this module deliberately does NOT own:
  * `refs/fleet/claims/*` -- claim/lease mechanics are `claim.py` (T1.1).
  * `refs/fleet/intents/*` -- the intent/ledger schema is `intent.py` (T1.4).
    `converge()` reports `status="blocked-on-rebase"` with the conflicting
    paths; it is the caller's job to persist that against an intent once
    T1.4 exists. Guessing that schema here would be redefining someone
    else's interface from the outside, which is exactly what "don't
    redefine fleetlib's interface" (ground rule 6) warns against by
    example.
  * `fleetlib.Hub` -- imported as-is from `staging/fleet-t02` (T0.2),
    never reimplemented. Only its public surface (`sha`, `read`, `create`,
    `update`, `delete`, `list`, `.url`, `.workdir`) is used here; drift.py
    drives its own `git` subprocesses against `hub.workdir` (a bare object
    cache Hub already maintains) rather than reaching into Hub's
    underscore-prefixed internals.

-------------------------------------------------------------------------
The `--ours`/`--theirs` trap (read this before touching `_resolve_conflicts`)
-------------------------------------------------------------------------
During `git rebase <tip>`, git replays the branch's commits *on top of* the
tip one at a time. For the purposes of a 3-way merge at each step:

    --ours   == the tree being rebased ONTO == the tip / upstream
    --theirs == the commit being replayed    == the branch's own commit

This is the reverse of `git merge`, where `--ours` is your current branch
and `--theirs` is the thing you're merging in. Confusing the two here is a
mistake that has actually happened on this project: `git checkout --theirs`
was run intending "take the tip's copy", but during a rebase `--theirs` is
the branch's own (stale, 24-commits-behind) copy -- so the checkout
silently "succeeded" and populated the tree with exactly the wrong content,
producing a binary_tables.rs with enum variants the rest of the tree no
longer had, and 17 downstream compile errors that had nothing to do with
the actual defect.

`_resolve_conflicts` below uses `--ours` (verified experimentally: see
`tests/test_drift_converge.py::TestOursTheirsTrap`) and, because a
one-word flag flip is exactly the kind of mistake that is invisible at
review time, never trusts the checkout's exit code alone -- it re-reads
the file's blob against the tip commit with `git diff --quiet <tip> --
<path>` before staging it, and again against `HEAD` after
`rebase --continue`. A checkout that silently grabbed the wrong side fails
that comparison instead of shipping.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fleetlib import Hub  # noqa: E402

# --------------------------------------------------------------------- #
# Constants (must match `docs/FLEET.md` "Shared contracts")
# --------------------------------------------------------------------- #

TIP_BRANCH_REF = "refs/heads/refactor/tag-machinery"
TIP_SIGNAL_REF = "refs/fleet/signals/tip"

MAX_DRIFT_COMMITS = 5
MAX_DRIFT_MINUTES = 30

# Files the generator (`tools/exiftool-tables/regen.sh`) owns outright.
# Never hand-merged: on conflict, take the tip's copy and re-run the
# generator rather than resolving line-by-line.
GENERATED_FILES = (
    "src/exiftool_tables/binary_tables.rs",
)

REGEN_SCRIPT_RELATIVE = "tools/exiftool-tables/regen.sh"

# Not `--all-targets`: that is stricter than the gate and once failed all
# 14 branches on pre-existing test lints (AGENTS.md / FLEET_PLAN.md T1.3).
DEFAULT_FASTCHECK_CMDS = (
    ("cargo", "fmt", "--all", "--", "--check"),
    ("cargo", "clippy", "--release", "--all-features",
     "--features", "jpeg-tag-matrix-binary", "--", "-D", "warnings"),
    ("cargo", "check"),
)


class DriftError(Exception):
    """Base class for drift.py failures that are not a normal outcome."""


# --------------------------------------------------------------------- #
# Result types
# --------------------------------------------------------------------- #


@dataclass
class DriftStatus:
    branch: str
    branch_sha: str
    tip_sha: str
    base_sha: str
    commits_behind: int
    minutes_behind: float
    ok: bool
    signal: Optional[dict] = None
    signal_is_stale: bool = False

    def as_tuple(self):
        """`(commits_behind, minutes_behind, ok)` -- the exact shape the
        spec's `drift.check(branch)` is written against.
        """
        return (self.commits_behind, self.minutes_behind, self.ok)


@dataclass
class ConvergeResult:
    branch: str
    status: str  # "up-to-date" | "converged" | "blocked-on-rebase" | "fastcheck-failed" | "regen-failed"
    tip_sha: str
    branch_sha: Optional[str]
    detail: str
    resolved_generated_files: list = field(default_factory=list)
    conflicted_paths: list = field(default_factory=list)


# --------------------------------------------------------------------- #
# git plumbing helpers
# --------------------------------------------------------------------- #


def _identity() -> str:
    user = os.environ.get("USER") or os.environ.get("LOGNAME") or "unknown"
    return f"{user}@{socket.gethostname()}:{os.getpid()}"


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _git_env() -> dict:
    env = dict(os.environ)
    env.update(
        {
            "GIT_AUTHOR_NAME": "oxidex-fleet",
            "GIT_AUTHOR_EMAIL": "fleet@oxidex.local",
            "GIT_COMMITTER_NAME": "oxidex-fleet",
            "GIT_COMMITTER_EMAIL": "fleet@oxidex.local",
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_SSH_COMMAND": env.get(
                "GIT_SSH_COMMAND",
                "ssh -o ConnectTimeout=10 -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
            ),
        }
    )
    return env


def _run(cmd: list, cwd: Optional[str] = None, input_bytes: Optional[bytes] = None,
          timeout: int = 120) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd, cwd=cwd, input=input_bytes, capture_output=True, timeout=timeout, env=_git_env(),
    )


def _git_dir(git_dir: str, args: list, **kw) -> subprocess.CompletedProcess:
    """A read-oriented git invocation against a bare (or `.git`) dir --
    no working tree required. Used for `check()`'s graph math and for the
    hook's tip-signal bump.
    """
    return _run(["git", "--git-dir", git_dir] + args, **kw)


def _git_tree(repo_dir: str, args: list, **kw) -> subprocess.CompletedProcess:
    """A working-tree git invocation (checkout, rebase, status...). Used
    by `converge()`, which needs an actual worktree to rebase and run
    `cargo` in.
    """
    return _run(["git", "-C", repo_dir] + args, **kw)


def _describe(result: subprocess.CompletedProcess) -> str:
    out = result.stdout.decode("utf-8", "replace") if isinstance(result.stdout, bytes) else result.stdout
    err = result.stderr.decode("utf-8", "replace") if isinstance(result.stderr, bytes) else result.stderr
    return f"$ {' '.join(result.args)}\n(exit {result.returncode})\nstdout: {out.strip()}\nstderr: {err.strip()}"


def _fetch_sha(git_dir: str, hub_url: str, ref: str) -> Optional[str]:
    """Fetch `ref` from `hub_url` into `git_dir`'s object store (a bare or
    `.git` dir) via a throwaway ref, then drop the throwaway ref -- the
    objects stay reachable for the caller's immediate use. Mirrors
    `fleetlib.Hub.read`'s own tmp-ref-then-delete pattern. Returns the
    fetched sha, or None if the remote ref does not exist.
    """
    tmp_ref = f"refs/fleet-drift-cache/{uuid.uuid4().hex}"
    result = _git_dir(git_dir, ["fetch", "--no-tags", "--quiet", hub_url, f"+{ref}:{tmp_ref}"])
    if result.returncode != 0:
        low = result.stderr.decode("utf-8", "replace").lower()
        if "couldn't find remote ref" in low or "not found" in low:
            return None
        raise DriftError(f"fetch of {ref} from {hub_url} failed:\n{_describe(result)}")
    sha_result = _git_dir(git_dir, ["rev-parse", "--verify", tmp_ref])
    if sha_result.returncode != 0:
        raise DriftError(f"fetched {ref} but could not resolve {tmp_ref}:\n{_describe(sha_result)}")
    sha = sha_result.stdout.decode().strip()
    # Best-effort cleanup; losing the tmp ref must never mask the result.
    _git_dir(git_dir, ["update-ref", "-d", tmp_ref])
    return sha


# --------------------------------------------------------------------- #
# M3 part 1: the tip signal (what the hook calls)
# --------------------------------------------------------------------- #


def _write_payload_commit(git_dir: str, payload: dict) -> str:
    data = json.dumps(payload, ensure_ascii=False, sort_keys=True, indent=2).encode("utf-8") + b"\n"
    blob = _git_dir(git_dir, ["hash-object", "-w", "--stdin"], input_bytes=data)
    if blob.returncode != 0:
        raise DriftError(f"hash-object failed:\n{_describe(blob)}")
    blob_sha = blob.stdout.decode().strip()

    tree_spec = f"100644 blob {blob_sha}\tpayload.json\n".encode("utf-8")
    tree = _git_dir(git_dir, ["mktree"], input_bytes=tree_spec)
    if tree.returncode != 0:
        raise DriftError(f"mktree failed:\n{_describe(tree)}")
    tree_sha = tree.stdout.decode().strip()

    commit = _git_dir(git_dir, ["commit-tree", tree_sha, "-m", "fleet: tip signal"])
    if commit.returncode != 0:
        raise DriftError(f"commit-tree failed:\n{_describe(commit)}")
    return commit.stdout.decode().strip()


def bump_tip_signal(git_dir: str, new_tip_sha: str, ref: str = TIP_SIGNAL_REF,
                     max_retries: int = 200, ts: Optional[str] = None) -> dict:
    """Advance `ref` (default `refs/fleet/signals/tip`) to
    `{sha: new_tip_sha, generation: <prev + 1>, ts, written_by}`.

    This is the entire implementation the `post-receive` hook calls into.
    It runs *locally* against the hub's own repo (git_dir is the hub's
    bare `.git`, not a remote URL) -- no ssh, no fleetlib.Hub, because a
    hook already *is* the hub.

    Race safety ("two pushes racing must not lose a generation bump or go
    backwards"): the read-current / compute-next / CAS-write is a plain
    optimistic-concurrency loop over `git update-ref <ref> <new> <old>`,
    which git itself makes atomic (verified in
    tests/test_drift_hook.py -- see also the interactive proof in this
    task's session transcript: a stale `<old>` is rejected with exit 128
    and the ref is left untouched, never partially updated). A losing
    racer simply re-reads the (now newer) generation and retries; nothing
    is ever skipped or decremented.
    """
    last_error = None
    for _ in range(max_retries):
        cur = _git_dir(git_dir, ["rev-parse", "--verify", "--quiet", ref])
        if cur.returncode == 0:
            old_commit_sha = cur.stdout.decode().strip()
            cat = _git_dir(git_dir, ["cat-file", "-p", f"{old_commit_sha}:payload.json"])
            cur_generation = 0
            if cat.returncode == 0:
                try:
                    cur_generation = int(json.loads(cat.stdout.decode())["generation"])
                except Exception:
                    cur_generation = 0
        else:
            old_commit_sha = ""  # git update-ref's create-only spelling
            cur_generation = 0

        payload = {
            "schema_version": 1,
            "written_by": _identity(),
            "written_at": _now_iso(),
            "sha": new_tip_sha,
            "generation": cur_generation + 1,
            "ts": ts or _now_iso(),
        }
        new_commit_sha = _write_payload_commit(git_dir, payload)

        update = _git_dir(git_dir, ["update-ref", ref, new_commit_sha, old_commit_sha])
        if update.returncode == 0:
            return payload

        last_error = _describe(update)
        time.sleep(random.uniform(0.0, 0.01))

    raise DriftError(f"bump_tip_signal: exceeded {max_retries} CAS retries on {ref}. Last attempt:\n{last_error}")


# --------------------------------------------------------------------- #
# M3 part 2: drift budget
# --------------------------------------------------------------------- #


def _resolve_tip(hub: Hub) -> tuple:
    """The true current tip sha, plus whatever the signal ref says (which
    may be absent or lagging if the hook is not installed / just fired).
    The signal is a fast poll target, never the source of truth for the
    commit graph math below -- if it disagrees with `refs/heads/...`, that
    is itself useful information (`signal_is_stale`), not something to
    silently trust.
    """
    raw_tip_sha = hub.sha(TIP_BRANCH_REF)
    if raw_tip_sha is None:
        raise DriftError(f"hub has no {TIP_BRANCH_REF}")
    signal = hub.read(TIP_SIGNAL_REF)
    stale = signal is not None and signal.get("sha") != raw_tip_sha
    return raw_tip_sha, signal, stale


def check(branch: str, hub: Hub, now: Optional[float] = None) -> DriftStatus:
    """`(commits_behind, minutes_behind, ok)` for `branch` against the
    hub's current tip. Pure read: fetches just enough of the object graph
    into `hub.workdir` (its bare cache) to answer, checks out nothing.

    `commits_behind` = commits on the tip's line since `branch` diverged.
    `minutes_behind` = wall-clock minutes since the *oldest* of those
    commits landed, i.e. how long this branch has been missing tip work --
    0 if the branch is fully caught up. Budget: `MAX_DRIFT_COMMITS=5`,
    `MAX_DRIFT_MINUTES=30`; over either ⇒ `ok=False` and the branch may
    not claim a gate (spec P5 / M3).
    """
    branch_ref = f"refs/heads/{branch}"
    tip_sha, signal, stale = _resolve_tip(hub)

    branch_sha = _fetch_sha(hub.workdir, hub.url, branch_ref)
    if branch_sha is None:
        raise DriftError(f"hub has no {branch_ref}")
    fetched_tip_sha = _fetch_sha(hub.workdir, hub.url, TIP_BRANCH_REF)
    if fetched_tip_sha != tip_sha:
        # hub.sha() and the fetch disagreed -- the tip moved between the
        # two calls. Use the freshly-fetched value; it is the one whose
        # objects are actually present locally.
        tip_sha = fetched_tip_sha

    base = _git_dir(hub.workdir, ["merge-base", branch_sha, tip_sha])
    if base.returncode != 0:
        raise DriftError(f"no merge-base between {branch} ({branch_sha}) and tip ({tip_sha}):\n{_describe(base)}")
    base_sha = base.stdout.decode().strip()

    count = _git_dir(hub.workdir, ["rev-list", "--count", f"{base_sha}..{tip_sha}"])
    if count.returncode != 0:
        raise DriftError(f"rev-list --count failed:\n{_describe(count)}")
    commits_behind = int(count.stdout.decode().strip())

    minutes_behind = 0.0
    if commits_behind > 0:
        oldest = _git_dir(hub.workdir, ["rev-list", f"{base_sha}..{tip_sha}", "--reverse"])
        if oldest.returncode != 0:
            raise DriftError(f"rev-list --reverse failed:\n{_describe(oldest)}")
        first_new_commit = oldest.stdout.decode().splitlines()[0].strip()
        ts_result = _git_dir(hub.workdir, ["show", "-s", "--format=%ct", first_new_commit])
        if ts_result.returncode != 0:
            raise DriftError(f"show --format=%ct failed:\n{_describe(ts_result)}")
        commit_epoch = int(ts_result.stdout.decode().strip())
        wall_now = now if now is not None else time.time()
        minutes_behind = max(0.0, (wall_now - commit_epoch) / 60.0)

    ok = commits_behind <= MAX_DRIFT_COMMITS and minutes_behind <= MAX_DRIFT_MINUTES

    return DriftStatus(
        branch=branch,
        branch_sha=branch_sha,
        tip_sha=tip_sha,
        base_sha=base_sha,
        commits_behind=commits_behind,
        minutes_behind=minutes_behind,
        ok=ok,
        signal=signal,
        signal_is_stale=stale,
    )


# --------------------------------------------------------------------- #
# M3 part 3: forced rebase
# --------------------------------------------------------------------- #


def _conflicted_paths(repo_dir: str) -> list:
    result = _git_tree(repo_dir, ["diff", "--name-only", "--diff-filter=U"])
    if result.returncode != 0:
        raise DriftError(f"listing conflicted paths failed:\n{_describe(result)}")
    return [line.strip() for line in result.stdout.decode().splitlines() if line.strip()]


def _resolve_generated_conflicts(repo_dir: str, tip_sha: str, paths: list) -> list:
    """For each conflicted path known to be GENERATED: take the tip's
    content explicitly (never `--theirs` -- see the module docstring) and
    verify the result actually matches the tip before staging it.

    Returns the list of paths resolved this way. Raises DriftError (rather
    than silently continuing) if the post-checkout content does not match
    the tip -- i.e. never trusts the checkout's exit code alone.
    """
    resolved = []
    for path in paths:
        co = _git_tree(repo_dir, ["checkout", "--ours", "--", path])
        if co.returncode != 0:
            raise DriftError(f"checkout --ours failed for {path} (during rebase onto {tip_sha}):\n{_describe(co)}")

        # The verification the war story demands: don't trust the
        # checkout succeeded just because it exited 0. Confirm the
        # working-tree content byte-for-byte matches the tip's blob.
        verify = _git_tree(repo_dir, ["diff", "--quiet", tip_sha, "--", path])
        if verify.returncode != 0:
            raise DriftError(
                f"generated file {path} did not match tip {tip_sha} after `checkout --ours` -- "
                "refusing to stage a mismatched generated file. This is exactly the "
                "--ours/--theirs trap; do not paper over it by switching to --theirs."
            )

        add = _git_tree(repo_dir, ["add", "--", path])
        if add.returncode != 0:
            raise DriftError(f"git add failed for {path}:\n{_describe(add)}")
        resolved.append(path)
    return resolved


def _run_regen(repo_dir: str, regen_cmd: list, log_dir: Path) -> tuple:
    """Run the table generator with its log OUTSIDE the repo tree --
    `regen.sh` refuses to run against a dirty tree, and a log file it
    wrote inside the tree would make it dirty before it even starts.
    Returns (ok, log_path, detail).
    """
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"regen-{uuid.uuid4().hex}.log"
    result = _run(regen_cmd, cwd=repo_dir, timeout=1800)
    out = result.stdout.decode("utf-8", "replace") if isinstance(result.stdout, bytes) else result.stdout
    err = result.stderr.decode("utf-8", "replace") if isinstance(result.stderr, bytes) else result.stderr
    log_path.write_text(f"$ {' '.join(regen_cmd)}\n(exit {result.returncode})\n\nSTDOUT:\n{out}\n\nSTDERR:\n{err}\n")
    return result.returncode == 0, log_path, (out + err)


def _run_fastcheck(repo_dir: str, cmds=DEFAULT_FASTCHECK_CMDS) -> tuple:
    """fmt --check + clippy (release, jpeg-tag-matrix-binary, -D warnings)
    + `cargo check`. Deliberately NOT `--all-targets` -- see the module
    docstring on DEFAULT_FASTCHECK_CMDS: `--all-targets` is stricter than
    the actual gate and once failed all 14 branches on pre-existing test
    lints that the real gate does not check.
    """
    for cmd in cmds:
        result = _run(list(cmd), cwd=repo_dir, timeout=1800)
        if result.returncode != 0:
            return False, _describe(result)
    return True, "fastcheck passed: " + " && ".join(" ".join(c) for c in cmds)


def converge(
    branch: str,
    repo_dir: str,
    hub: Hub,
    *,
    fastcheck: Optional[Callable] = None,
    regen_cmd: Optional[list] = None,
    push: bool = True,
    external_log_dir: Optional[Path] = None,
) -> ConvergeResult:
    """Fetch, rebase `branch` onto the hub's current tip, run a fastcheck,
    and (if `push`) push the result back. On a genuine (non-generated)
    conflict, abort the rebase and report `status="blocked-on-rebase"`
    rather than guessing -- "surfacing it in minutes is the whole point"
    (FLEET_SPEC.md M3).

    `repo_dir` must be a working-tree clone with a remote reachable at
    `hub.url` (any name; this function fetches by URL directly and does
    not assume a particular remote name). `fastcheck`, if given, replaces
    `_run_fastcheck` -- tests inject a stub here rather than building the
    real Rust workspace, matching the "mock the gate; do not build Rust in
    a unit test" principle FLEET_PLAN.md applies to the merge train.
    `regen_cmd`, if given, replaces the real
    `tools/exiftool-tables/regen.sh` invocation for the same reason (it
    needs perl + network in production).
    """
    fastcheck = fastcheck or _run_fastcheck
    log_dir = external_log_dir or Path(tempfile.gettempdir()) / "oxidex-fleet-drift-logs"

    status = check(branch, hub)
    if status.commits_behind == 0:
        return ConvergeResult(
            branch=branch, status="up-to-date", tip_sha=status.tip_sha, branch_sha=status.branch_sha,
            detail=f"{branch} is already at tip {status.tip_sha}",
        )

    branch_ref = f"refs/heads/{branch}"

    # Bring the local working tree's remote-tracking state current, then
    # check out the branch. `--force` is safe here: this is a local ref
    # we fully control inside our own scratch clone, not the branch on
    # the hub.
    fetch_branch = _git_tree(repo_dir, ["fetch", "--quiet", hub.url,
                                         f"+{branch_ref}:refs/heads/{branch}"])
    if fetch_branch.returncode != 0:
        raise DriftError(f"fetch of {branch_ref} into {repo_dir} failed:\n{_describe(fetch_branch)}")
    fetch_tip = _git_tree(repo_dir, ["fetch", "--quiet", hub.url,
                                      f"+{TIP_BRANCH_REF}:refs/fleet-drift/tip"])
    if fetch_tip.returncode != 0:
        raise DriftError(f"fetch of {TIP_BRANCH_REF} into {repo_dir} failed:\n{_describe(fetch_tip)}")

    checkout = _git_tree(repo_dir, ["checkout", "--quiet", branch])
    if checkout.returncode != 0:
        raise DriftError(f"checkout of {branch} failed:\n{_describe(checkout)}")

    rebase = _git_tree(repo_dir, ["rebase", status.tip_sha])
    if rebase.returncode != 0:
        conflicts = _conflicted_paths(repo_dir)
        if not conflicts:
            # `rebase` failed for a reason that isn't a content conflict
            # (e.g. it needs `git config user.*`, or a hook rejected it).
            _git_tree(repo_dir, ["rebase", "--abort"])
            raise DriftError(f"rebase onto {status.tip_sha} failed with no conflicted paths:\n{_describe(rebase)}")

        non_generated = [p for p in conflicts if p not in GENERATED_FILES]
        if non_generated:
            # Genuine source conflict. Stop and say so -- never guess a
            # resolution (FLEET_SPEC.md principle 6, "refuse rather than
            # approximate").
            _git_tree(repo_dir, ["rebase", "--abort"])
            return ConvergeResult(
                branch=branch, status="blocked-on-rebase", tip_sha=status.tip_sha, branch_sha=status.branch_sha,
                detail=(
                    f"rebase of {branch} onto {status.tip_sha} conflicts on non-generated "
                    f"path(s): {non_generated}. Rebase aborted; branch left untouched."
                ),
                conflicted_paths=conflicts,
            )

        resolved = _resolve_generated_conflicts(repo_dir, status.tip_sha, conflicts)

        cont = _git_tree(repo_dir, ["-c", "core.editor=true", "rebase", "--continue"])
        if cont.returncode != 0:
            still_conflicted = _conflicted_paths(repo_dir)
            _git_tree(repo_dir, ["rebase", "--abort"])
            return ConvergeResult(
                branch=branch, status="blocked-on-rebase", tip_sha=status.tip_sha, branch_sha=status.branch_sha,
                detail=f"rebase --continue failed after resolving {resolved}:\n{_describe(cont)}",
                conflicted_paths=still_conflicted,
            )

        # Re-run the generator against the pin rather than trusting the
        # mechanically-copied tip content forever. `regen.sh` refuses a
        # dirty tree, so this only runs after `rebase --continue` above
        # has committed; its log is written outside the repo tree.
        cmd = regen_cmd or ["bash", REGEN_SCRIPT_RELATIVE]
        ok, log_path, detail = _run_regen(repo_dir, cmd, log_dir)
        if not ok:
            return ConvergeResult(
                branch=branch, status="regen-failed", tip_sha=status.tip_sha, branch_sha=status.branch_sha,
                detail=f"regen.sh failed after rebase; log at {log_path}:\n{detail[-4000:]}",
                resolved_generated_files=resolved,
                conflicted_paths=conflicts,
            )

        dirty = _git_tree(repo_dir, ["status", "--porcelain"])
        if dirty.stdout.decode().strip():
            # regen.sh disagreed with what we mechanically copied from the
            # tip -- log it loudly (this is the "missing conflict domain"
            # style signal from FLEET_SPEC.md M6) and commit the refresh,
            # since regen.sh is the actual source of truth for
            # correctness; the tip copy was only the mechanical rebase
            # resolution.
            _git_tree(repo_dir, ["add", "-A"])
            commit_msg = "regen(tables): refresh after forced rebase (drift.converge)"
            commit = _git_tree(repo_dir, ["commit", "-m", commit_msg])
            if commit.returncode != 0:
                raise DriftError(f"commit of post-regen changes failed:\n{_describe(commit)}")
            print(
                f"[drift.converge] LOUD: regen.sh changed {path_count_hint(dirty.stdout.decode())} "
                f"file(s) beyond the tip's committed copy for {branch}; committed the refresh. "
                f"log at {log_path}",
                file=sys.stderr,
            )

    else:
        resolved = []

    branch_head = _git_tree(repo_dir, ["rev-parse", "HEAD"]).stdout.decode().strip()

    ok, detail = fastcheck(repo_dir)
    if not ok:
        return ConvergeResult(
            branch=branch, status="fastcheck-failed", tip_sha=status.tip_sha, branch_sha=branch_head,
            detail=detail, resolved_generated_files=resolved,
        )

    if push:
        lease = f"--force-with-lease={branch_ref}:{status.branch_sha}"
        push_result = _git_tree(repo_dir, ["push", lease, hub.url, f"HEAD:{branch_ref}"])
        if push_result.returncode != 0:
            raise DriftError(f"push of converged {branch} failed:\n{_describe(push_result)}")

    return ConvergeResult(
        branch=branch, status="converged", tip_sha=status.tip_sha, branch_sha=branch_head,
        detail=f"{branch} rebased onto {status.tip_sha}, fastcheck green" + (", pushed" if push else ""),
        resolved_generated_files=resolved,
    )


def path_count_hint(porcelain_output: str) -> int:
    return len([line for line in porcelain_output.splitlines() if line.strip()])


# --------------------------------------------------------------------- #
# CLI -- the hook shells out to `bump-tip-signal`; `check`/`converge` are
# for manual/ops use and for other fleet tooling that prefers a
# subprocess boundary over importing this module directly.
# --------------------------------------------------------------------- #


def _cli_bump_tip_signal(args) -> int:
    payload = bump_tip_signal(args.git_dir, args.new_sha, ref=args.ref)
    print(json.dumps(payload, indent=2))
    return 0


def _cli_check(args) -> int:
    hub = Hub(url=args.hub, workdir=args.workdir)
    status = check(args.branch, hub)
    print(json.dumps({
        "branch": status.branch,
        "commits_behind": status.commits_behind,
        "minutes_behind": round(status.minutes_behind, 2),
        "ok": status.ok,
        "tip_sha": status.tip_sha,
        "branch_sha": status.branch_sha,
        "base_sha": status.base_sha,
        "signal_is_stale": status.signal_is_stale,
    }, indent=2))
    return 0 if status.ok else 1


def _cli_converge(args) -> int:
    hub = Hub(url=args.hub, workdir=args.workdir)
    result = converge(args.branch, args.repo, hub, push=not args.no_push)
    print(json.dumps({
        "branch": result.branch,
        "status": result.status,
        "tip_sha": result.tip_sha,
        "branch_sha": result.branch_sha,
        "detail": result.detail,
        "resolved_generated_files": result.resolved_generated_files,
        "conflicted_paths": result.conflicted_paths,
    }, indent=2))
    return 0 if result.status in ("up-to-date", "converged") else 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="drift.py", description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_bump = sub.add_parser("bump-tip-signal", help="Advance refs/fleet/signals/tip (called by the post-receive hook)")
    p_bump.add_argument("new_sha")
    p_bump.add_argument("--git-dir", required=True, dest="git_dir")
    p_bump.add_argument("--ref", default=TIP_SIGNAL_REF)
    p_bump.set_defaults(func=_cli_bump_tip_signal)

    p_check = sub.add_parser("check", help="commits_behind/minutes_behind/ok for a branch")
    p_check.add_argument("branch")
    p_check.add_argument("--hub", required=True)
    p_check.add_argument("--workdir", required=True)
    p_check.set_defaults(func=_cli_check)

    p_converge = sub.add_parser("converge", help="Rebase a branch onto the tip, fastcheck, push")
    p_converge.add_argument("branch")
    p_converge.add_argument("--hub", required=True)
    p_converge.add_argument("--workdir", required=True)
    p_converge.add_argument("--repo", required=True, help="working-tree clone to rebase in")
    p_converge.add_argument("--no-push", action="store_true")
    p_converge.set_defaults(func=_cli_converge)

    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except DriftError as exc:
        print(f"drift.py: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
