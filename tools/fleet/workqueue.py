#!/usr/bin/env python3
"""Compute the fleet queue purely from hub refs, on every read.

This is T1.1's second half (`docs/FLEET.md` P1, "M4 -- Queue derived from
refs, never copied"):

    queue = {staging/* on hub}
          - {branches already ancestors of the tip}
          - {branches with a live claim}
          - {branches whose intent is withdrawn}

`~/.train-queue` is being deleted on every host in wave 3. Nothing in this
module reads or writes a local queue file, and `Queue.compute()` re-derives
the answer from scratch on every call -- there is no cache to go stale.
Two real incidents motivate this (`docs/FLEET.md` incidents 3 and 10): a
host gated a branch an hour after it was retired from the hub, and another
kept 31 stale staging refs because `git fetch --prune` does not prune when
fetching into local branches. Both become unrepresentable once there is no
local copy that can drift from the hub.

The only local state this module touches is `fleetlib.Hub.workdir`, the
disposable bare-repo object cache `Hub` itself already owns -- reused here
purely to hold the *real* commit history of the tip and staging branches
(as opposed to the orphan payload commits `Hub` writes) so that
`git merge-base --is-ancestor` can run against it. Git's object store is
content-addressed, so sharing it with `Hub`'s own commits is harmless;
nothing here is a second source of truth, since the cache refs used for the
ancestry check are rebuilt by fetching the hub fresh on every `compute()`
call and are torn down again afterward.

A branch's "intent" is looked up by convention: `refs/heads/staging/<slug>`
corresponds to `refs/fleet/intents/<slug>` (see `docs/FLEET.md` M5 -- intent
slugs and staging branch names share a namespace throughout the spec's own
examples, e.g. `staging/rb-s26` / `route-legacy-formats`). A branch whose
intent payload has `status == "withdrawn"` is excluded from the queue even
though the ref itself is still sitting on the hub.

Standard library only.

Named `workqueue.py`, not `queue.py`: the plan (`docs/FLEET.md`) originally
assigned this file the name `queue.py`, but that collides with the stdlib
`queue` module (`Queue`, `Empty`, `Full`, `SimpleQueue`) that `threading`
and `concurrent.futures` rely on internally. With `tools/fleet` on
`sys.path` -- the convention `fleetlib`'s own tests use -- a file literally
named `queue.py` there shadows the real module for any spawned subprocess
that inherits that `sys.path`, which broke `concurrent.futures`'s own
`ProcessPoolExecutor` in an unrelated test purely by existing on disk, no
import of it required. Confirmed by removing the file and watching the
unrelated test pass again. Renamed instead of worked around: a module that
breaks any consumer who puts its directory on `sys.path` is a defect in
the module, not in the consumers. Please don't rename this back to
`queue.py`.
"""

from __future__ import annotations

import subprocess
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Dict, Optional, Set

from claim import CLAIMS_PREFIX, is_expired
from fleetlib import Hub, HubUnreachableError

TIP_REF = "refs/heads/refactor/tag-machinery"
STAGING_PREFIX = "refs/heads/staging"
INTENTS_PREFIX = "refs/fleet/intents"

_QUEUE_CACHE_NS = "refs/fleet-queue-cache"


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


class QueueError(Exception):
    """Base class for workqueue.py-specific errors."""


@dataclass
class QueueEntry:
    """One admissible branch: `slug` is the part of the ref after
    `staging/` (e.g. `"foo"` for `refs/heads/staging/foo`).
    """

    slug: str
    ref: str
    sha: str


class Queue:
    """Computes the fleet queue fresh from hub refs on every call.

    Holds no state across `compute()` calls beyond the disposable git
    object cache `fleetlib.Hub` already owns -- this is deliberately not a
    class you construct once and poll; construct it, or reuse one, and call
    `compute()` every time you need the current answer.
    """

    def __init__(
        self,
        hub: Hub,
        tip_ref: str = TIP_REF,
        staging_prefix: str = STAGING_PREFIX,
        intents_prefix: str = INTENTS_PREFIX,
    ):
        self.hub = hub
        self.tip_ref = tip_ref
        self.staging_prefix = staging_prefix.rstrip("/")
        self.intents_prefix = intents_prefix.rstrip("/")

    # ------------------------------------------------------------------ #
    # Public
    # ------------------------------------------------------------------ #

    def compute(self, now: Optional[datetime] = None) -> Dict[str, QueueEntry]:
        """{slug: QueueEntry} for every branch currently admissible to the
        queue. Recomputed from hub refs every call -- never cached.
        """
        now = now or _utcnow()

        # `code_sha`, not `sha`: the tip is a CODE ref. Against a split
        # spine, `hub.sha(TIP_REF)` asks the STATE repo -- which carries
        # only `refs/fleet/*` -- and gets a perfectly successful `None`.
        # That `None` becomes the QueueError below on every single call,
        # which is how `fleetd --hub <state> --code <code>` died in a
        # traceback before it wrote its first heartbeat: the daemon's loop
        # catches HubError, and QueueError is not one.
        tip_sha = self.hub.code_sha(self.tip_ref)
        if tip_sha is None:
            raise QueueError(
                f"tip ref {self.tip_ref!r} does not exist on the code repo "
                f"{self.hub.code_url!r}"
            )

        staging = self._list_staging()
        if not staging:
            return {}

        cache_ns = self._fetch_for_ancestry(tip_sha, staging)
        try:
            live_claim_keys = self._live_claim_work_keys(now)
            withdrawn_slugs = self._withdrawn_intent_slugs()

            out: Dict[str, QueueEntry] = {}
            for slug, (ref, sha) in staging.items():
                if self._is_ancestor(cache_ns, sha, tip_sha):
                    continue  # already merged -- nothing left to do
                branch = ref.removeprefix("refs/heads/")
                if slug in live_claim_keys or ref in live_claim_keys or branch in live_claim_keys:
                    continue  # somebody is already working this key
                if slug in withdrawn_slugs:
                    continue  # intent was withdrawn -- ref is stale intent
                out[slug] = QueueEntry(slug=slug, ref=ref, sha=sha)
            return out
        finally:
            self._cleanup_cache(cache_ns, staging)

    def compute_or_refusal(self, now: Optional[datetime] = None) -> tuple:
        """`(queue, None)` on success, or `({}, (reason, detail))` when the
        queue cannot be computed for a reason that is ABOUT THE QUEUE
        rather than about the hub.

        This exists because `compute()` raises `QueueError`, which is not a
        `HubError`, and `fleetd`'s reconcile loop catches exactly
        `HubError` -- on purpose ("a bug in this file, a KeyboardInterrupt
        or a MemoryError must still take the process down loudly"). A
        missing tip is neither a bug nor a transport failure: it is a
        configuration fact the operator needs stated, and the whole point
        of `fleet status --why` is that a host which starts nothing says
        why. So the scheduler asks through this method and gets a
        `refused` reason it can put in the heartbeat, and the daemon keeps
        running, keeps heartbeating, and keeps reaping.

        `HubUnreachableError` deliberately still propagates: that IS a hub
        failure, the loop's existing degrade-this-step path handles it,
        and turning it into a refusal reason would report a network outage
        as a permanent configuration verdict.
        """
        try:
            return self.compute(now=now), None
        except QueueError as exc:
            return {}, ("queue-unavailable", str(exc))

    def slugs(self, now: Optional[datetime] = None) -> list:
        """Sorted list of queued slugs -- convenience for callers that only
        want names, e.g. `fleet status`.
        """
        return sorted(self.compute(now=now).keys())

    # ------------------------------------------------------------------ #
    # Internals
    # ------------------------------------------------------------------ #

    def _list_staging(self) -> Dict[str, tuple]:
        # `code_list`, not `list`: `refs/heads/staging/*` lives on the CODE
        # repo. Aimed at the state repo this returns `{}` -- no error, no
        # log line -- and `compute()` reads that as "the queue is empty",
        # which is a fleet that idles while reporting itself healthy.
        raw = self.hub.code_list(self.staging_prefix)  # {refname: sha}
        prefix = self.staging_prefix + "/"
        out: Dict[str, tuple] = {}
        for ref, sha in raw.items():
            if not ref.startswith(prefix):
                continue
            slug = ref[len(prefix):]
            out[slug] = (ref, sha)
        return out

    def _live_claim_work_keys(self, now: datetime) -> Set[str]:
        """`work_key` of every live (unexpired) claim on the hub.

        ARCH-FIX R4: `fleetd.start_gate`/`start_agent` set `work_key=branch`,
        e.g. `"staging/foo"` -- the ref with `refs/heads/` stripped, never
        the bare slug (`"foo"`) and never the full ref
        (`"refs/heads/staging/foo"`). `compute()` below matches against all
        three forms for exactly this reason: comparing only slug/ref left a
        real fleetd-held gate claim invisible to this set, so a second host
        computing the queue would offer the same branch as gate work while
        another host was already gating it -- the double-gate leases exist
        to prevent, reintroduced at the queue layer instead of the claim
        layer. `is_expired` here relies on the holder actually renewing
        (claim.py's `acquire`-owns-renewal contract, R2); before that fix
        this filter silently dropped every gate past ten minutes, which is
        every real gate, and was a second, independent path to the same
        double-gate outcome.
        """
        keys: Set[str] = set()
        for ref in self.hub.list(CLAIMS_PREFIX):
            payload = self.hub.read(ref)
            if payload is None:
                continue
            if is_expired(payload, now=now):
                continue
            work_key = payload.get("work_key")
            if work_key:
                keys.add(work_key)
        return keys

    def _withdrawn_intent_slugs(self) -> Set[str]:
        out: Set[str] = set()
        prefix = self.intents_prefix + "/"
        for ref in self.hub.list(self.intents_prefix):
            if not ref.startswith(prefix):
                continue
            slug = ref[len(prefix):]
            payload = self.hub.read(ref)
            if payload is not None and payload.get("status") == "withdrawn":
                out.add(slug)
        return out

    def _fetch_for_ancestry(self, tip_sha: str, staging: Dict[str, tuple]) -> str:
        """Pull the *real* commit history of the tip and every staging
        branch into the local cache, under a disposable ref namespace, so
        `merge-base --is-ancestor` can run against it. Returns the
        namespace used, for later cleanup.

        The tip and every staging branch are CODE refs, so this fetches
        from `hub.code_url` -- not `hub.url` (the state hub once code and
        state are split across two repos). `code_url` defaults to `url`
        (`fleetlib.Hub`), so a fixture with a single combined repo behaves
        exactly as before.
        """
        cache_ns = f"{_QUEUE_CACHE_NS}/{uuid.uuid4().hex}"
        refspecs = [f"+{self.tip_ref}:{cache_ns}/tip"]
        for slug, (ref, _sha) in staging.items():
            safe = slug.replace("/", "__")
            refspecs.append(f"+{ref}:{cache_ns}/staging/{safe}")

        result = self._git(["fetch", "--no-tags", "--quiet", self.hub.code_url, *refspecs])
        if result.returncode != 0:
            raise HubUnreachableError(
                f"fetch for ancestry check failed: {result.stderr.decode('utf-8', 'replace').strip()}"
            )
        return cache_ns

    def _is_ancestor(self, cache_ns: str, candidate_sha: str, tip_sha: str) -> bool:
        result = self._git(["merge-base", "--is-ancestor", candidate_sha, tip_sha])
        if result.returncode not in (0, 1):
            stderr = result.stderr.decode("utf-8", "replace").strip()
            raise QueueError(f"merge-base --is-ancestor failed unexpectedly: {stderr}")
        return result.returncode == 0

    def _cleanup_cache(self, cache_ns: str, staging: Dict[str, tuple]) -> None:
        # Best-effort: local ref cleanup failing must never mask a queue
        # result the caller already has.
        refs = [f"{cache_ns}/tip"]
        for slug in staging:
            safe = slug.replace("/", "__")
            refs.append(f"{cache_ns}/staging/{safe}")
        for ref in refs:
            self._git(["update-ref", "-d", ref])

    def _git(self, args, timeout: int = 30):
        cmd = ["git", "--git-dir", str(self.hub.workdir)] + args
        try:
            return subprocess.run(cmd, capture_output=True, timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            raise HubUnreachableError(f"{' '.join(cmd)} timed out after {timeout}s") from exc
