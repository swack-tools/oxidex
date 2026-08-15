#!/usr/bin/env python3
"""Tip signal (T1.3 / M3 / P5).

`bump_tip_signal` advances `refs/fleet/signals/tip` to `{sha, generation,
ts}` -- a cheap, poll-friendly signal the hub's `post-receive` hook bumps on
every update to `refs/heads/refactor/tag-machinery`, which `fleetd` on every
host polls every 15 s instead of diffing the full branch ref. This is the
*entire* implementation the hook calls into (see `hooks/post-receive`); it
runs locally against the hub's own bare repo (`git --git-dir`), never over
ssh, because a hook already is the hub.

NOTE (ARCH-FIX R9): this module used to also carry a drift budget (`check`)
and a forced-rebase routine (`converge`) -- refusing a stale branch a gate
claim and getting it current again without hand-merging a generated file.
Both were deleted 2026-08-15: zero production caller ever invoked either
(no `gate.sh` stage, no `fleetd`/`workqueue` code path shelled out to
`drift.py check` or `drift.py converge`), so the protection they described
never actually ran. See `docs/FLEET.md`'s M3 note and the deletion commit
for the superseding design (tree-keyed verdict caching + LLM convergence
agents); the tip signal here is unaffected and remains load-bearing.

Two things this module deliberately does NOT own:
  * `refs/fleet/claims/*` -- claim/lease mechanics are `claim.py` (T1.1).
  * `fleetlib.Hub` -- imported as-is from `staging/fleet-t02` (T0.2),
    never reimplemented. Only its public surface (`sha`, `read`, `create`,
    `update`, `delete`, `list`, `.url`, `.workdir`) is used here.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fleetlib import Hub  # noqa: E402

# --------------------------------------------------------------------- #
# Constants (must match `docs/FLEET.md` "Shared contracts")
# --------------------------------------------------------------------- #

TIP_SIGNAL_REF = "refs/fleet/signals/tip"


class DriftError(Exception):
    """Base class for drift.py failures that are not a normal outcome."""


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
    """A read/write git invocation against a bare (or `.git`) dir -- no
    working tree required. Used for the hook's tip-signal bump.
    """
    return _run(["git", "--git-dir", git_dir] + args, **kw)


def _describe(result: subprocess.CompletedProcess) -> str:
    out = result.stdout.decode("utf-8", "replace") if isinstance(result.stdout, bytes) else result.stdout
    err = result.stderr.decode("utf-8", "replace") if isinstance(result.stderr, bytes) else result.stderr
    return f"$ {' '.join(result.args)}\n(exit {result.returncode})\nstdout: {out.strip()}\nstderr: {err.strip()}"


# --------------------------------------------------------------------- #
# The tip signal (what the hook calls)
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
# CLI -- the hook shells out to `bump-tip-signal`.
# --------------------------------------------------------------------- #


def _cli_bump_tip_signal(args) -> int:
    payload = bump_tip_signal(args.git_dir, args.new_sha, ref=args.ref)
    print(json.dumps(payload, indent=2))
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="drift.py", description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_bump = sub.add_parser("bump-tip-signal", help="Advance refs/fleet/signals/tip (called by the post-receive hook)")
    p_bump.add_argument("new_sha")
    p_bump.add_argument("--git-dir", required=True, dest="git_dir")
    p_bump.add_argument("--ref", default=TIP_SIGNAL_REF)
    p_bump.set_defaults(func=_cli_bump_tip_signal)

    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except DriftError as exc:
        print(f"drift.py: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
