#!/usr/bin/env python3
"""Content-addressed compare-and-swap primitives over git refs.

This is the foundation of the fleet coordination design (`docs/FLEET.md`,
"§1 Shared contracts"): the hub is the only source of truth, and every write
is a single atomic git ref operation. No daemon, no lock server, no local
copy that can drift.

The primitive is `git push` of a brand-new ref: a non-forced push fails
closed if the ref already exists, which is exactly a compare-and-swap on
"does this key exist yet". `update`/`delete` are the same push, guarded by
`--force-with-lease=<ref>:<expect_sha>` so two concurrent writers racing to
mutate the same ref can never both win.

A ref points at a commit whose tree holds exactly one blob, `payload.json`.
Reading is `git cat-file`; nothing is ever checked out to a working tree.

Distinguishing "ref does not exist" from "could not reach the hub" is load
bearing: a `read()` that quietly returns `None` on a network blip looks
identical to "nobody has claimed this yet" and invites double-claiming. See
`HubUnreachableError` below -- every method that talks to the remote raises
it on a transport failure instead of returning an absence sentinel.

Only two flavours of git exit-nonzero are ever treated as "not an error":
  * `git ls-remote` returning 0 with no matching line  -> ref absent.
  * `git push` (create/update/delete) rejected for a *content* reason
    (ref already exists, stale `--force-with-lease`, non-fast-forward)
    -> the operation lost the race, so return False.
Everything else -- DNS failure, refused connection, timeout, auth failure,
a target path that isn't a git repository at all -- is a transport failure
and raises `HubUnreachableError`.

Standard library only. No dependency on this module ever leaving the local
git binary and stdlib `subprocess`/`json`.
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional


class HubError(Exception):
    """Base class for all fleetlib errors."""


class HubUnreachableError(HubError):
    """The hub could not be reached, or answered in a way that is not a
    recognized "the operation lost a race" rejection.

    Raised instead of ever silently degrading to `None`/`False` so that a
    caller cannot mistake "the network blipped" for "the ref does not
    exist" or "somebody else already holds this".
    """


# Substrings (already lower-cased) that identify a `git push` rejection as
# a genuine content-level CAS failure -- the ref already exists (create),
# or the caller's `expect_sha` is stale (update/delete) -- rather than a
# transport problem. Anything not matching one of these on a non-zero exit
# is treated as unreachable and raises.
_PUSH_REJECTION_PATTERNS = (
    "already exists",
    "stale info",
    "[rejected]",
    "failed to update ref",
    "non-fast-forward",
    "cannot lock ref",
    "fetch first",
    "not currently pointing",
)

# Substrings that identify an outright transport/access failure so read
# paths (`git ls-remote`) can raise a clear error instead of a generic one.
_TRANSPORT_HINTS = (
    "could not resolve hostname",
    "could not read from remote repository",
    "connection refused",
    "connection timed out",
    "operation timed out",
    "no route to host",
    "network is unreachable",
    "ssh: connect to host",
    "does not appear to be a git repository",
    "repository not found",
    "permission denied",
    "unable to access",
    "could not resolve proxy",
    "early eof",
    "the remote end hung up unexpectedly",
    "host key verification failed",
)


@dataclass
class _Result:
    returncode: int
    stdout: str
    stderr: str
    args: list = field(default_factory=list)

    def describe(self) -> str:
        return f"git {' '.join(self.args)} -> exit {self.returncode}: {self.stderr.strip()}"


class Hub:
    """A CAS-over-git-refs client.

    `url` is anything `git` accepts as a remote: an `ssh://` URL, a local
    path, or a `file://` URL. `workdir` is a local directory this instance
    uses as a private, disposable object-store cache (a bare repo it will
    create if absent) -- it is never the hub itself and is never checked
    out to a working tree.
    """

    def __init__(self, url: str, workdir: Path):
        self.url = str(url)
        self.workdir = Path(workdir)
        self.workdir.mkdir(parents=True, exist_ok=True)
        if not (self.workdir / "objects").is_dir():
            init = self._raw_run(["git", "init", "--quiet", "--bare", str(self.workdir)])
            if init.returncode != 0:
                raise HubError(f"could not initialize local cache repo at {self.workdir}: {init.stderr.strip()}")

    # ---------------------------------------------------------------- #
    # Public interface
    # ---------------------------------------------------------------- #

    def sha(self, ref: str) -> Optional[str]:
        """Current commit sha of `ref` on the hub, or None if it does not exist."""
        return self._remote_sha(ref)

    def read(self, ref: str) -> Optional[dict]:
        """The `payload.json` a ref points at, or None if the ref is absent.

        Raises HubUnreachableError on any transport failure -- never
        returns None for "could not reach the hub".
        """
        found_sha = self._remote_sha(ref)
        if found_sha is None:
            return None

        tmp_ref = f"refs/fleet-cache/{uuid.uuid4().hex}"
        fetch = self._run(["fetch", "--no-tags", "--quiet", self.url, f"+{ref}:{tmp_ref}"])
        if fetch.returncode != 0:
            low = fetch.stderr.lower()
            if "couldn't find remote ref" in low or "not found" in low:
                # Ref existed at sha() time and was deleted before fetch.
                # That is a legitimate absence, not an error.
                return None
            raise HubUnreachableError(f"fetch of {ref} failed: {fetch.describe()}")

        cat = self._run(["cat-file", "-p", f"{found_sha}:payload.json"])
        # Best-effort cleanup of the temporary local ref; failure here must
        # never mask the read result.
        self._run(["update-ref", "-d", tmp_ref])

        if cat.returncode != 0:
            raise HubError(f"{ref}@{found_sha} has no readable payload.json: {cat.describe()}")

        try:
            return json.loads(cat.stdout)
        except json.JSONDecodeError as exc:
            raise HubError(f"{ref}@{found_sha} payload.json is not valid JSON: {exc}") from exc

    def create(self, ref: str, payload: dict) -> bool:
        """Atomically create `ref` if, and only if, it does not exist yet.

        This is the CAS primitive everything else composes from: a
        non-forced `git push` of a brand-new ref is rejected by the remote
        if the ref is already there, with no way for two racing pushes to
        both succeed.
        """
        commit_sha = self._write_commit(self._augment(payload))
        result = self._run(["push", self.url, f"{commit_sha}:{ref}"])
        return self._interpret_push(result)

    def update(self, ref: str, payload: dict, expect_sha: str) -> bool:
        """Atomically replace `ref`'s payload, but only if it still points
        at `expect_sha`. Uses `--force-with-lease` so two racing updaters
        (e.g. two reapers) cannot both win.
        """
        commit_sha = self._write_commit(self._augment(payload))
        lease = f"--force-with-lease={ref}:{expect_sha}"
        result = self._run(["push", lease, self.url, f"{commit_sha}:{ref}"])
        return self._interpret_push(result)

    def delete(self, ref: str, expect_sha: str) -> bool:
        """Atomically delete `ref`, but only if it still points at
        `expect_sha`. Same `--force-with-lease` guard as `update`.
        """
        lease = f"--force-with-lease={ref}:{expect_sha}"
        result = self._run(["push", lease, self.url, f":{ref}"])
        return self._interpret_push(result)

    def list(self, prefix: str) -> dict:
        """{ref: sha} for every ref on the hub matching `prefix`."""
        pattern = prefix if prefix.endswith("*") else prefix.rstrip("/") + "/*"
        result = self._run(["ls-remote", self.url, pattern])
        if result.returncode != 0:
            raise HubUnreachableError(f"ls-remote {pattern} failed: {result.describe()}")
        out: dict = {}
        for line in result.stdout.splitlines():
            line = line.strip()
            if not line:
                continue
            found_sha, refname = line.split("\t", 1)
            out[refname] = found_sha
        return out

    # ---------------------------------------------------------------- #
    # Internals
    # ---------------------------------------------------------------- #

    def _remote_sha(self, ref: str) -> Optional[str]:
        result = self._run(["ls-remote", self.url, ref])
        if result.returncode != 0:
            raise HubUnreachableError(f"ls-remote {ref} failed: {result.describe()}")
        for line in result.stdout.splitlines():
            line = line.strip()
            if not line:
                continue
            found_sha, refname = line.split("\t", 1)
            if refname == ref:
                return found_sha
        return None

    def _augment(self, payload: dict) -> dict:
        base = {
            "schema_version": 1,
            "written_by": self._identity(),
            "written_at": self._now_iso(),
        }
        return {**base, **payload}

    @staticmethod
    def _identity() -> str:
        user = os.environ.get("USER") or os.environ.get("LOGNAME") or "unknown"
        return f"{user}@{socket.gethostname()}:{os.getpid()}"

    @staticmethod
    def _now_iso() -> str:
        return datetime.now(timezone.utc).isoformat()

    def _write_commit(self, payload: dict) -> str:
        """Write payload.json + tree + an orphan commit into the local
        cache repo and return the commit sha. Nothing is pushed yet.
        """
        data = json.dumps(payload, ensure_ascii=False, sort_keys=True, indent=2).encode("utf-8") + b"\n"
        blob = self._run(["hash-object", "-w", "--stdin"], input=data)
        if blob.returncode != 0:
            raise HubError(f"hash-object failed: {blob.describe()}")
        blob_sha = blob.stdout.strip()

        tree_spec = f"100644 blob {blob_sha}\tpayload.json\n"
        tree = self._run(["mktree"], input=tree_spec.encode("utf-8"))
        if tree.returncode != 0:
            raise HubError(f"mktree failed: {tree.describe()}")
        tree_sha = tree.stdout.strip()

        commit = self._run(["commit-tree", tree_sha, "-m", "fleet: payload"])
        if commit.returncode != 0:
            raise HubError(f"commit-tree failed: {commit.describe()}")
        return commit.stdout.strip()

    def _interpret_push(self, result: _Result) -> bool:
        if result.returncode == 0:
            return True
        low = result.stderr.lower()
        if any(pattern in low for pattern in _PUSH_REJECTION_PATTERNS):
            return False
        raise HubUnreachableError(f"push failed unexpectedly: {result.describe()}")

    def _run(self, args: list, input: Optional[bytes] = None, timeout: int = 30) -> _Result:
        cmd = ["git", "--git-dir", str(self.workdir)] + args
        return self._raw_run(cmd, input=input, timeout=timeout)

    @staticmethod
    def _raw_run(cmd: list, input: Optional[bytes] = None, timeout: int = 30) -> _Result:
        env = dict(os.environ)
        env.update(
            {
                "GIT_AUTHOR_NAME": "oxidex-fleet",
                "GIT_AUTHOR_EMAIL": "fleet@oxidex.local",
                "GIT_COMMITTER_NAME": "oxidex-fleet",
                "GIT_COMMITTER_EMAIL": "fleet@oxidex.local",
                "GIT_SSH_COMMAND": "ssh -o ConnectTimeout=10 -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
                "GIT_TERMINAL_PROMPT": "0",
            }
        )
        try:
            completed = subprocess.run(
                cmd,
                input=input,
                capture_output=True,
                timeout=timeout,
                env=env,
            )
        except subprocess.TimeoutExpired as exc:
            raise HubUnreachableError(f"{' '.join(cmd)} timed out after {timeout}s") from exc
        return _Result(
            returncode=completed.returncode,
            stdout=completed.stdout.decode("utf-8", "replace"),
            stderr=completed.stderr.decode("utf-8", "replace"),
            args=cmd[1:],
        )
