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

Reads are COHERENT: `read()` fetches the ref into a private local ref and
then reads THAT, so the payload it returns and the sha it read it from are
the same commit no matter what the hub does mid-read. This is not a
refinement -- the older sequence resolved the sha, fetched, and then
cat-filed the sha it had resolved *first*, and once leases began renewing
themselves every held claim ref started moving on a timer. A read that
landed in that window raised, and the one consumer that read every claim
payload on every loop (`fleetd`) had no `try` around it. `read_with_sha()`
exposes the coherent pair for callers that need both; `sha()` remains a
single `ls-remote`, and is for existence and for CAS witnesses, never for
"which sha should I read the payload at".

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
import re
import shlex
import socket
import subprocess
import time
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional, Sequence, Tuple


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
# paths (`git ls-remote`, `git fetch`) can raise a clear error instead of
# degrading to an absence sentinel. See `_is_transport_failure`.
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
    # GitHub-specific, and the reason these three are HINTS rather than
    # push-rejection patterns. When the spine is a GitHub repo instead of
    # an ssh hub, the way it says "not now" is a rejection, not a dropped
    # connection: a primary or secondary rate limit, or the abuse-detection
    # mechanism, comes back as a refused git operation whose stderr carries
    # `remote: You have exceeded a secondary rate limit`, `remote: You have
    # triggered an abuse detection mechanism`, or an `API rate limit
    # exceeded` line. Every one of those is TRANSIENT -- retry in a minute
    # and it works -- so classifying one as a lost CAS race would be the
    # expensive direction of being wrong twice over: `create` returning
    # False means "somebody else already holds this" (so this host stands
    # down from work nobody is doing), and `update` returning False means
    # "my lease moved under me" (which `claim._mark_lost` turns into a
    # killed healthy worker, `claim.py` L677-682). Transient must raise.
    "rate limit",
    "secondary rate",
    "abuse detection",
)

# git's "the whole repository is gone" wording interpolates the URL between
# the two words -- `fatal: repository 'ssh://hub/oxidex.git' not found` --
# so the bare "repository not found" hint above never matches the message
# it was written for. It matters that this one is recognized POSITIVELY
# rather than merely failing to match the absence phrase: this is the
# single most likely way a hub disappears, and its message ends in the two
# words "not found".
_REPO_NOT_FOUND_RE = re.compile(r"repository\s+'[^']*'\s+not found")

# The ONE phrase git uses when a fetch names a ref the remote does not
# have: `fatal: couldn't find remote ref refs/fleet/claims/gate/x`. It is
# the only fetch-failure signature that means ABSENT.
#
# It used to be `"couldn't find remote ref" in low or "not found" in low`,
# and the second half of that disjunction is why this comment exists.
# "not found" is a substring of, among others, `fatal: repository
# '<url>' not found` -- the hub itself being gone, which is the most
# consequential transport failure there is. `read()` answered None for it,
# and None from `read()` means "nobody has claimed this yet", which is the
# one confusion this module's docstring calls load bearing: it invites two
# hosts onto one branch at exactly the moment the hub cannot arbitrate.
# The absence signature is therefore matched EXACTLY, and every message
# that is not it -- recognized transport hint or not -- raises.
_ABSENT_REF_HINT = "couldn't find remote ref"


# HTTPS credentials for the GitHub spine, supplied by FILE and never by
# argv, environment value, or an interactive prompt.
#
# `~/.keel/github.token` is a per-host fine-grained PAT (SPEC 8,
# "Credentials"); `FLEET_GIT_TOKEN_FILE` names it. When that variable is
# set, every git command this module runs is given a `credential.helper`
# pointing at the shell script below, which reads the token out of the file
# and hands it to git on its stdout. The token therefore never appears in a
# process argument list (`ps -eo args` shows it to every user on the host),
# never in an environment value (`_ps_env` reads those), and never in a
# `_Result` -- git does not echo a password it received from a helper, and
# the helper itself writes nothing but `username=`/`password=` on stdout.
#
# Two config entries, not one. The first is `credential.helper=` with an
# EMPTY value, which is git's documented way to reset the helper list
# (git-config(1), credential.helper: "an empty value resets the helper
# list to empty"); without it a host-level helper configured in
# ~/.gitconfig or /etc/gitconfig -- osxkeychain on the laptops, and the
# 1Password agent this fleet has already been bitten by -- runs FIRST and
# git uses whatever it answers. The second entry is ours. The pair makes
# "the token comes from FLEET_GIT_TOKEN_FILE" a fact rather than a hope.
#
# The entries are APPENDED at `GIT_CONFIG_COUNT`, not written at index 0,
# so a caller that has already staged its own GIT_CONFIG_* pairs keeps
# them.
_TOKEN_FILE_ENV = "FLEET_GIT_TOKEN_FILE"
_CREDENTIAL_HELPER = Path(__file__).resolve().parent / "keel" / "git-credential-file"


def credential_env(env: Optional[dict] = None) -> dict:
    """`env` (default `os.environ`) copied, with the fleet credential
    helper wired in when `FLEET_GIT_TOKEN_FILE` is set.

    Returns a NEW dict; the input is never mutated. When the variable is
    unset the copy is returned untouched -- so a fleet that has not opted
    into HTTPS-token auth runs the exact git invocations it ran before this
    function existed. That "unchanged when unset" property is what lets the
    whole existing test suite and the `git init --bare` fixtures stay as
    they are.

    Raises `HubError` when the variable IS set but the helper script or the
    token file is missing or unreadable. Failing loud is deliberate and is
    the same lesson as `scripts/instrument.py`'s `resolve_binary()`: a
    credential path that silently resolves to nothing does not stop
    anything, it just makes every subsequent git command fail with an
    authentication error that reads like a permissions problem on the
    remote. The message names the PATH and never the contents.
    """
    out = dict(os.environ if env is None else env)
    token_file = out.get(_TOKEN_FILE_ENV)
    if not token_file:
        return out

    if not _CREDENTIAL_HELPER.is_file():
        raise HubError(
            f"{_TOKEN_FILE_ENV} is set but the credential helper "
            f"{_CREDENTIAL_HELPER} does not exist"
        )
    if not os.access(_CREDENTIAL_HELPER, os.X_OK):
        raise HubError(
            f"{_TOKEN_FILE_ENV} is set but the credential helper "
            f"{_CREDENTIAL_HELPER} is not executable"
        )
    token_path = Path(token_file)
    if not token_path.is_file():
        raise HubError(
            f"{_TOKEN_FILE_ENV}={token_file} does not name an existing file"
        )
    if not os.access(token_path, os.R_OK):
        raise HubError(f"{_TOKEN_FILE_ENV}={token_file} is not readable")

    # `!` makes git treat the value as a command rather than a path or a
    # `git credential-<name>` shorthand; shlex.quote survives a helper
    # path containing spaces, which git would otherwise hand to `sh -c`
    # word-split (run-command.c's `need_shell` lists space as a
    # metacharacter, so a quoted path is not optional here).
    entries = (
        ("credential.helper", ""),
        ("credential.helper", "!" + shlex.quote(str(_CREDENTIAL_HELPER))),
    )
    try:
        base = int(out.get("GIT_CONFIG_COUNT", "0") or 0)
    except ValueError:
        base = 0
    if base < 0:
        base = 0
    for offset, (key, value) in enumerate(entries):
        out[f"GIT_CONFIG_KEY_{base + offset}"] = key
        out[f"GIT_CONFIG_VALUE_{base + offset}"] = value
    out["GIT_CONFIG_COUNT"] = str(base + len(entries))
    return out


def _is_transport_failure(stderr: str) -> bool:
    """True if `stderr` carries a recognized transport/access failure.

    Consulted BEFORE any absence signature, never after. A message can
    contain both (`fatal: repository '<url>' not found` contains the words
    "not found"), and when it does, transport wins -- the expensive
    direction of being wrong here is calling an unreachable hub "empty",
    not calling an empty hub "unreachable".
    """
    low = stderr.lower()
    return any(h in low for h in _TRANSPORT_HINTS) or bool(_REPO_NOT_FOUND_RE.search(low))


# How many times `read()` re-runs the WHOLE ls-remote/fetch/cat-file
# sequence when a fetch it was told succeeded leaves no resolvable commit
# behind. The fetch-first rewrite below should make this unreachable -- the
# sha being read is the one the fetch just brought, and a local ref cannot
# move underneath a local `cat-file`. It is kept as a bounded backstop for
# anything that can still race the object store (a concurrent
# `git gc --prune=now` in the same cache dir, a ref write that loses to a
# sibling process sharing the workdir), because the failure mode it guards
# against used to take fleetd down rather than degrade a single read. A
# fresh fetch is the only thing that repairs a missing object, which is why
# the retry re-runs the sequence instead of patching up the last step.
#
# Bounded, never infinite: a genuinely broken hub must still surface as an
# error. And deliberately NOT applied to the other way `cat-file` fails --
# a commit that IS in the local store and simply has no `payload.json` in
# its tree is a real error, unaffected by retrying, and gets git's same
# `fatal: path 'payload.json' does not exist in '<sha>'` wording. Telling
# the two apart by git's message would be guesswork; `rev-parse` on the
# fetched ref answers it directly.
_READ_ATTEMPTS = 3
_READ_RETRY_SLEEP_S = 0.2


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

    `code_url` is the remote that answers *code* questions -- "is this sha
    an ancestor of the tip", "do I have these objects", "fetch this staging
    branch" -- as opposed to the coordination refs under `refs/fleet/*`
    that `url` answers. Historically they were the same repository, so it
    DEFAULTS TO `url` and every existing caller, fixture and test sees no
    change whatsoever. They diverge only once coordination state moves to a
    private repo while the code stays public (SPEC 8, "Two repos, two
    exposures"): the three borrowers named in SPEC 4.4
    (`workqueue._fetch_for_ancestry`, `dispatch._have_objects`,
    `train._fetch_into_hub_cache`) read `code_url` instead of `url`.

    The default is resolved ONCE, at construction. Reassigning `hub.url`
    afterwards does not move `code_url` -- there is no live aliasing to
    reason about, which matters because `FallbackHub` (SPEC 4.3) presents
    the GitHub half's `.url`/`.workdir`/`.code_url` as its own.
    """

    def __init__(self, url: str, workdir: Path, code_url: Optional[str] = None):
        self.url = str(url)
        self.code_url = self.url if code_url is None else str(code_url)
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
        """Current commit sha of `ref` on the hub, or None if it does not exist.

        A single `ls-remote`, deliberately: this is the cheap existence
        probe (`hub.sha(ref) is not None`) and the CAS witness callers pass
        back as `expect_sha`, and both want the hub's answer right now
        rather than a fetch.

        It is NOT the way to get a sha to read a payload at. A `sha()`
        followed by a `read()` is two independent observations of a ref that
        renewing leases move on a timer -- use `read_with_sha()`, which
        returns a sha and the payload that belongs to it.
        """
        return self._remote_sha(ref)

    def read(self, ref: str) -> Optional[dict]:
        """The `payload.json` a ref points at, or None if the ref is absent.

        Raises HubUnreachableError on any transport failure -- never
        returns None for "could not reach the hub".
        """
        return self._read(ref, want_sha=False)[1]

    def read_with_sha(self, ref: str) -> Tuple[Optional[str], Optional[dict]]:
        """`(sha, payload)` for `ref`, COHERENT with each other, or
        `(None, None)` if the ref is absent.

        Coherent means: the sha returned is the one whose payload is
        returned. That was not true before, and the gap was a live
        time-of-check/time-of-use race -- the reason this method exists and
        the reason `read()` now delegates to it.

        THE OLD SEQUENCE, and why it broke:

            found_sha = self._remote_sha(ref)              # ls-remote -> S1
            self._run(["fetch", ..., f"+{ref}:{tmp_ref}"]) # brings S2
            self._run(["cat-file", "-p", f"{found_sha}:payload.json"])

        A ref that moved between the ls-remote and the fetch left the fetch
        bringing S2 while the cat-file asked for S1. Payload commits are
        orphans, so nothing else drags S1 into the local object store, and
        git answered `fatal: path 'payload.json' does not exist in '<S1>'`
        -- which this method turned into a raised `HubError`. Not a
        hypothetical: `tools/fleet/tests/test_seams.py` hit it unprompted
        during a seam-2 run, and `fleetd.reconcile_once` had no `try` around
        the `hub.read` that raised it, so a claim renewing underneath a
        queue computation could exit the daemon.

        THE WINDOW EXISTS BY DESIGN NOW. Before leases self-renewed (R2) no
        claim ref ever moved and this race was unreachable. R2 rewrites
        every held claim once per renewal interval and R4 has
        `workqueue.Queue.compute()` read every claim payload on every call,
        so the two correctness fixes together made a latent bug reachable
        on every loop.

        THE SEQUENCE NOW:

          1. `ls-remote` -- purely to tell ABSENT from UNREACHABLE. That
             distinction is this module's central promise (see the module
             docstring) and `git fetch`'s stderr does not carry it as
             reliably as `ls-remote`'s exit code does. The sha it resolves
             is deliberately DISCARDED; it is a fact about the past by the
             time the next line runs.
          2. `fetch` into a fresh uuid-named `tmp_ref`.
          3. `rev-parse tmp_ref` and `cat-file tmp_ref:payload.json` -- both
             against the LOCAL ref the fetch just wrote. A local ref no
             remote can move is the whole fix: whatever the hub does in
             between, the sha reported and the payload returned come from
             the same fetched commit.

        The ref moving between steps 1 and 2 is therefore no longer an
        error at all: the read simply returns the newer payload, which is a
        legitimate serialization of a concurrent write. The ref being
        DELETED between steps 1 and 2 still returns None, as before.

        COST. This method runs one `rev-parse` that `read()` does not --
        measured at +7ms on a ~46ms read, i.e. +15% on the single hottest
        primitive in the fleet, which every claim renewal, every queue
        computation and every reap goes through. That is why the sha is
        opt-in and `read()` still issues exactly the four git commands it
        issued before this fix (`ls-remote`, `fetch`, `cat-file`,
        `update-ref -d`): correctness here must not be bought with latency
        on a path that runs against a 5s test TTL and a 600s real one.
        """
        return self._read(ref, want_sha=True)

    def _read(self, ref: str, want_sha: bool) -> Tuple[Optional[str], Optional[dict]]:
        """Shared body of `read`/`read_with_sha`; see `read_with_sha` for
        the sequence and why it is that sequence. `want_sha` buys the extra
        `rev-parse` round trip, which the sha-less `read()` must not pay.
        """
        last: Optional[_Result] = None
        for attempt in range(1, _READ_ATTEMPTS + 1):
            # (1) Absence probe only. `_remote_sha` raises
            # HubUnreachableError on transport failure, so "the ref is not
            # there" and "we could not ask" stay distinguishable -- the one
            # thing a caller must never confuse (module docstring).
            if self._remote_sha(ref) is None:
                return None, None

            # (2) Whatever the ref points at NOW lands here.
            tmp_ref = f"refs/fleet-cache/{uuid.uuid4().hex}"
            fetch = self._run(["fetch", "--no-tags", "--quiet", self.url, f"+{ref}:{tmp_ref}"])
            if fetch.returncode != 0:
                # ORDER IS THE FIX, and the `and` below is the order:
                # transport is consulted BEFORE the absence signature, so
                # a message carrying both is transport. The `ls-remote`
                # above answered the absence question for its own instant
                # only -- the transport can die between it and here -- so
                # this branch is the only place a fetch failure is ever
                # classified. See `_is_transport_failure`/`_ABSENT_REF_HINT`.
                if (not _is_transport_failure(fetch.stderr)
                        and _ABSENT_REF_HINT in fetch.stderr.lower()):
                    # Ref existed at the ls-remote and was deleted before
                    # the fetch. A legitimate absence, not an error.
                    return None, None
                # Everything else raises: a recognized transport failure,
                # and equally a message nobody has classified yet. Fail
                # CLOSED -- returning None is a positive claim about the
                # hub's contents, and an unrecognized error is not
                # evidence for one.
                raise HubUnreachableError(f"fetch of {ref} failed: {fetch.describe()}")

            # (3) Read the commit the FETCH brought, not the one the
            # ls-remote resolved. `tmp_ref` is local and uuid-unique, so
            # nothing can move it between these commands -- which is why
            # naming the ref is as good as naming the sha, and why the
            # `rev-parse` is optional rather than load-bearing.
            def resolve():
                r = self._run(["rev-parse", "--verify", "--quiet", f"{tmp_ref}^{{commit}}"])
                return r.stdout.strip() if r.returncode == 0 else None

            payload: Optional[dict] = None
            try:
                fetched_sha = resolve() if want_sha else None
                cat = self._run(["cat-file", "-p", f"{tmp_ref}:payload.json"])
                if cat.returncode != 0:
                    # Cold path. The sha is wanted now even when the caller
                    # did not ask for one: it tells an object-store failure
                    # (retryable) from a commit that genuinely carries no
                    # payload (not), and names the right sha in the error
                    # either way. Both cold paths resolve while `tmp_ref`
                    # still exists -- after the cleanup below there is no
                    # way left to name the commit at all.
                    fetched_sha = fetched_sha or resolve()
                else:
                    try:
                        payload = json.loads(cat.stdout)
                    except json.JSONDecodeError as exc:
                        raise HubError(
                            f"{ref}@{fetched_sha or resolve() or '<unresolved>'} payload.json "
                            f"is not valid JSON: {exc}"
                        ) from exc
            finally:
                # Best-effort cleanup of the temporary local ref; failure
                # here must never mask the read result.
                self._run(["update-ref", "-d", tmp_ref])

            if cat.returncode == 0:
                return fetched_sha, payload

            last = cat
            if fetched_sha is None and attempt < _READ_ATTEMPTS:
                # The fetch claimed success and left nothing resolvable at
                # `tmp_ref`: an object-store problem, not a payload problem.
                # Re-run the whole sequence -- see `_READ_ATTEMPTS`.
                time.sleep(_READ_RETRY_SLEEP_S)
                continue

            # Either the commit is present locally and genuinely has no
            # `payload.json` (a real error, unaffected by retrying), or the
            # object store stayed broken for every attempt. Both are
            # HubError, with the wording preserved verbatim -- `test_seams.py`
            # matches on it.
            raise HubError(
                f"{ref}@{fetched_sha or '<unresolved>'} has no readable payload.json: "
                f"{cat.describe()}"
            )

        raise HubError(  # pragma: no cover -- only reachable if _READ_ATTEMPTS < 1
            f"{ref}: read gave up without attempting anything "
            f"(_READ_ATTEMPTS={_READ_ATTEMPTS}): "
            f"{last.describe() if last is not None else '<no result>'}"
        )

    def create(self, ref: str, payload: dict, push_options: Optional[Sequence[str]] = None) -> bool:
        """Atomically create `ref` if, and only if, it does not exist yet.

        This is the CAS primitive everything else composes from: a
        non-forced `git push` of a brand-new ref is rejected by the remote
        if the ref is already there, with no way for two racing pushes to
        both succeed.

        `push_options`, if given, are passed through as `git push -o <opt>`
        for each entry (see `_push_option_args` -- e.g. the train's
        `train-token=<secret>` option required by the hub's R1 update-hook
        guard on `refs/heads/main` / `refs/heads/refactor/tag-machinery`).
        Omitted (the default) reproduces the exact pre-existing behavior of
        this method for every caller that does not pass it.
        """
        commit_sha = self._write_commit(self._augment(payload))
        result = self._run(["push", *self._push_option_args(push_options), self.url, f"{commit_sha}:{ref}"])
        return self._interpret_push(result)

    def update(self, ref: str, payload: dict, expect_sha: str, push_options: Optional[Sequence[str]] = None) -> bool:
        """Atomically replace `ref`'s payload, but only if it still points
        at `expect_sha`. Uses `--force-with-lease` so two racing updaters
        (e.g. two reapers) cannot both win.

        See `create` for `push_options` semantics.
        """
        commit_sha = self._write_commit(self._augment(payload))
        lease = f"--force-with-lease={ref}:{expect_sha}"
        result = self._run(["push", *self._push_option_args(push_options), lease, self.url, f"{commit_sha}:{ref}"])
        return self._interpret_push(result)

    def delete(self, ref: str, expect_sha: str, push_options: Optional[Sequence[str]] = None) -> bool:
        """Atomically delete `ref`, but only if it still points at
        `expect_sha`. Same `--force-with-lease` guard as `update`.

        See `create` for `push_options` semantics.
        """
        lease = f"--force-with-lease={ref}:{expect_sha}"
        result = self._run(["push", *self._push_option_args(push_options), lease, self.url, f":{ref}"])
        return self._interpret_push(result)

    def push_ref(
        self,
        refspec: str,
        push_options: Optional[Sequence[str]] = None,
        force: bool = False,
    ) -> _Result:
        """A raw `git push <refspec>` against the hub -- the non-CAS push
        path (a plain branch advance, not a create/update/delete of a
        payload-commit ref). Exists so callers that push branches directly
        (e.g. the train advancing `refs/heads/refactor/tag-machinery`) can
        still get `push_options` plumbing through `fleetlib` instead of
        reimplementing `git push -o ...` themselves.

        Returns the raw `_Result` (unlike create/update/delete, there is no
        single-content-reason-vs-transport-failure distinction to collapse
        here -- a plain branch push can be rejected for reasons, such as a
        hub-side hook denial, that are neither "lost a CAS race" nor a
        transport failure, so this method leaves interpretation to the
        caller rather than raising or returning a bare bool).
        """
        args = ["push"]
        if force:
            args.append("--force")
        args.extend(self._push_option_args(push_options))
        args.extend([self.url, refspec])
        return self._run(args)

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

    def fetch_namespace(self, prefix: str) -> dict:
        """`{ref: sha}` for the ref AT `prefix` and every ref UNDER it, in
        exactly ONE `ls-remote` round trip.

        This is the whole-namespace read: one network hop answers "what is
        in `refs/fleet/claims/` right now", which is what an index build
        (SPEC 3.2, the server's `CachedHub`) and a prefix query
        (`GET /v1/refs?prefix=`) each need, and what a per-ref `sha()` loop
        turns into N hops of roughly 0.6 s apiece against GitHub.

        HOW IT DIFFERS FROM `list()`, which also returns `{ref: sha}` from
        one `ls-remote` and is deliberately left exactly as it was:
        `list()` normalises its argument to `<prefix>/*`, so a ref sitting
        AT the prefix is invisible to it -- `list("refs/fleet/signals/tip")`
        looks for `refs/fleet/signals/tip/*` and reports the tip signal, a
        ref the train CAS-bumps on every advance, as ABSENT. That is
        harmless for `list()`'s existing callers, every one of which passes
        a directory-shaped prefix, and wrong for a namespace read that must
        not silently drop a leaf.

        So `fetch_namespace` hands the single `ls-remote` BOTH patterns --
        `git ls-remote` accepts many and unions them
        (`builtin/ls-remote.c`'s `tail_match`). Only one of the two can ever
        match, and the caller is precisely who does not know which:
        `refs/fleet/signals/tip` is a leaf and `refs/fleet/claims` is a
        directory, and git's ref store refuses to let a prefix be both at
        once (a second `create` under an existing leaf comes back False,
        `cannot lock ref ...: '<prefix>' exists` -- measured, and pinned by
        `test_fleetlib.TestFetchNamespace.
        test_git_refuses_a_leaf_and_a_directory_of_the_same_name`). Asking
        both questions in the one round trip is how the answer stops
        depending on knowing the shape in advance.

        Trailing `/` and a trailing `*` are accepted and normalised away,
        so `refs/fleet/claims`, `refs/fleet/claims/` and
        `refs/fleet/claims/*` all mean the same namespace.

        Raises `HubUnreachableError` on transport failure, like every other
        remote method here: an empty dict means the namespace is empty, and
        that must never be what an unreachable spine looks like.
        """
        base = str(prefix).strip()
        while base.endswith("*"):
            base = base[:-1]
        base = base.rstrip("/")
        if not base:
            raise ValueError("fetch_namespace requires a non-empty ref prefix")
        result = self._run(["ls-remote", self.url, base, base + "/*"])
        if result.returncode != 0:
            raise HubUnreachableError(
                f"ls-remote {base} failed: {result.describe()}"
            )
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

    @staticmethod
    def _push_option_args(push_options: Optional[Sequence[str]]) -> list:
        """`["-o", opt1, "-o", opt2, ...]` for `push_options`, or `[]` if
        None/empty -- the latter is load-bearing: every pre-existing call
        site that does not pass `push_options` must see the exact same
        `git push` invocation as before this parameter existed. The hub
        must also have `receive.advertisePushOptions=true` set (see
        tools/fleet/rollout/install_hook.sh) or git rejects any push
        carrying `-o` at the transport level before this ever reaches a
        hook -- that is a hub config requirement, not something this
        method can paper over.
        """
        if not push_options:
            return []
        args: list = []
        for opt in push_options:
            args.extend(["-o", opt])
        return args

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
        """`True` = the CAS won, `False` = it lost a race, raise = we do
        not know.

        ORDER IS THE RULE, and it is the same rule `_read` states at its
        fetch-failure branch: TRANSPORT IS CONSULTED FIRST, so a message
        carrying both a transport hint and a content-rejection pattern is
        transport. It has to be, now that the spine is GitHub. A secondary
        rate limit arrives as

            remote: You have exceeded a secondary rate limit ...
             ! [remote rejected] <sha> -> refs/fleet/claims/x (...)
            error: failed to push some refs to '<url>'

        -- a REJECTION carrying `[rejected]`, which is a
        `_PUSH_REJECTION_PATTERNS` entry. Read content-first, that is
        indistinguishable from losing a CAS race, and the two lies it tells
        are both expensive: `create` -> False reads as "another host holds
        this claim" and stands a healthy host down from work nobody is
        doing, and `update` -> False reads as "my lease moved under me",
        which `claim._mark_lost` (claim.py L677-682) turns into a killed
        healthy gate. Both are silent. Raising instead is the behaviour a
        blip already gets: `claim._note_renew_failure` tolerates it and the
        next renewal re-reads and adopts our own landed write.

        Nothing else changes: a rejection with no transport hint still
        returns False (the overwhelmingly common case -- git's plain
        `(stale info)` / `(non-fast-forward)` wording carries no hint), and
        an unclassifiable failure still raises.
        """
        if result.returncode == 0:
            return True
        low = result.stderr.lower()
        if _is_transport_failure(low):
            raise HubUnreachableError(f"push failed transiently: {result.describe()}")
        if any(pattern in low for pattern in _PUSH_REJECTION_PATTERNS):
            return False
        raise HubUnreachableError(f"push failed unexpectedly: {result.describe()}")

    def _run(self, args: list, input: Optional[bytes] = None, timeout: int = 30) -> _Result:
        cmd = ["git", "--git-dir", str(self.workdir)] + args
        return self._raw_run(cmd, input=input, timeout=timeout)

    @staticmethod
    def _raw_run(cmd: list, input: Optional[bytes] = None, timeout: int = 30) -> _Result:
        # `credential_env` returns a plain copy of os.environ when
        # FLEET_GIT_TOKEN_FILE is unset, so this line is a no-op for every
        # ssh-spine caller and the git invocation below is byte-identical
        # to the one this method has always issued.
        env = credential_env()
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
