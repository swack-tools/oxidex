#!/usr/bin/env python3
"""Leases over `refs/fleet/claims/<kind>/<key>`, built on `fleetlib.Hub`.

This is T1.1 (`docs/FLEET.md` P1, M1 "Claims and leases"). A claim is a ref
whose *existence* is the claim: `Hub.create` is the compare-and-swap that
lets exactly one caller win a given `(kind, key)`. Renewal and release are
`Hub.update`/`Hub.delete` guarded by `--force-with-lease`, so no two
processes can ever race to mutate the same claim.

Use `Claim` as a context manager:

    with Claim(hub, "gate", tree_sha, work_kind="gate", work_key=branch_slug,
               gate_version=GATE_VERSION, workdir="nc-3") as claim:
        run_the_gate()
    # claim released here, even on an exception

or acquire it directly -- both paths behave identically, which is the
point of this module's central rule:

    RENEWAL IS NOT OPTIONAL AND IS NOT THE CALLER'S JOB.

`acquire()`, `acquire_or_reap()` and `adopt()` start the background renewer
themselves; `release()` and `__exit__` stop it. There is no way to hold a
lease without renewing it, because that is exactly what happened: from
this system's first day until 2026-08-15, `fleetd` acquired every gate,
agent and host-singleton claim with `acquire_or_reap()` and never called
`start_renewer()`. Only the `with`-statement path renewed. Every claim
held past `LEASE_TTL` -- i.e. every real gate, all of which run longer
than ten minutes -- silently expired mid-work and became reapable by any
host, so the mutual exclusion the leases exist to provide lapsed at
minute ten of every gate the fleet ever ran. Nothing failed, nothing
logged; three green unit-test suites covered the two halves of the seam
and none covered the composition. Starting the renewer inside `acquire`
is what makes that class of defect unreachable rather than merely fixed.

A background thread renews the lease every `renew_interval` seconds so a
long-running holder never has its claim expire out from under it. If the
holder process crashes (killed, segfaults, `os._exit`) before `__exit__`
runs, nothing releases the claim -- that is intentional: the claim simply
expires at `expires_at` and becomes reapable by `reap_expired`, which is
the whole point of a lease instead of a lock. A crash can leak a claim for
at most `ttl` seconds; it can never leak one silently forever.

## The lost lease

`Claim.lost` is the other half of the contract. It goes True when the
renewer establishes that the lease is no longer ours:

  * the claim ref vanished from the hub (a reaper decided we were
    expired, or an operator deleted it), or
  * the ref exists but carries somebody else's acquisition token, or
  * the CAS renewal was rejected, or
  * renewal has been failing for long enough that the hub-side
    `expires_at` is about to pass with no attempt left to save it -- an
    unreachable hub, but equally a corrupted local object cache or a full
    disk, because the deadline does not care why we failed. At that point
    another host may legitimately reap and re-claim, whatever we believe
    locally.

`lost` is thread-safe to read (its own tiny lock, deliberately NOT the
lock the renewer holds across a 30-second `git push`, so a consumer
polling it can never block behind a network round trip), it is sticky
until the next `acquire()`, and it carries `lost_reason` for logging.

**Consumers MUST treat `lost` as stop-work.** A lost lease means another
host may already be running the same work; the whole point of the lease
is that this never happens. `fleetd` therefore kills a lost worker's
process group rather than draining it -- see the comment at
`fleetd.reconcile_once` for why that inverts the usual rule.

## Compressing time in tests

`FLEET_TEST_TTL_S` and `FLEET_TEST_RENEW_S` override the TTL and renewal
cadence for any `Claim` constructed without explicit values. A property
that only manifests after ten minutes at production timescale will not be
tested at production timescale; it will simply not be tested, which is
how this defect survived. Precedence is: explicit constructor argument >
environment variable (read at construction) > module constant.

`rustc_id` vs `platform_id`: these answer different questions and must
never be collapsed into one value (see `docs/FLEET.md`, "Two tasks computed
`toolchain_id` differently, and both were right"):

  * `rustc_id`     = sha256(`rustc -vV`) with the `host:` line STRIPPED.
                      "Is this host on the canonical compiler?" -- every
                      host on the same rustc release shares one value.
  * `platform_id`  = sha256(`rustc -vV`) UNSTRIPPED.
                      "Is this verdict/claim transferable to that host?"
                      `ffi_c_integration` passes on Linux and fails on
                      macOS at the identical rustc release, so a Linux
                      artifact must never satisfy a macOS slot. Collapsing
                      the two would reintroduce exactly that skew.

Standard library only.
"""

from __future__ import annotations

import hashlib
import os
import socket
import subprocess
import threading
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Optional

from fleetlib import Hub, HubError, HubUnreachableError

# ---------------------------------------------------------------------- #
# Constants (docs/FLEET.md "Shared contracts")
# ---------------------------------------------------------------------- #

# Env overrides so tests can compress time (see module docstring). Read at
# import for the module constants and again at every `Claim` construction,
# so a test that sets the variable after import still gets the compressed
# value -- an in-process test driving `fleetd.reconcile_once` cannot
# re-import claim.py.
TTL_ENV = "FLEET_TEST_TTL_S"
RENEW_ENV = "FLEET_TEST_RENEW_S"


def _env_seconds(name: str, default: float) -> float:
    """`os.environ[name]` as seconds, or `default` if unset/unparseable/
    negative. Never raises: a typo in an env var must not take a daemon
    down, it must leave the production default in place.
    """
    raw = os.environ.get(name)
    if raw is None or not raw.strip():
        return default
    try:
        value = float(raw)
    except (TypeError, ValueError):
        return default
    return value if value >= 0 else default


LEASE_TTL = _env_seconds(TTL_ENV, 600)  # seconds a claim is valid before reapable
RENEW = _env_seconds(RENEW_ENV, 120)  # seconds between background renewals

CLAIMS_PREFIX = "refs/fleet/claims"


class ClaimError(HubError):
    """Base class for claim.py-specific errors."""


class ClaimHeldError(ClaimError):
    """Raised by `Claim.acquire()` when the (kind, key) is already held by
    an unexpired lease -- i.e. the CAS lost, and the loser is not the one
    who should proceed.
    """


# ---------------------------------------------------------------------- #
# Ref naming
# ---------------------------------------------------------------------- #


def claim_ref(kind: str, key: str) -> str:
    """The ref path for a claim of `kind` on `key`."""
    if "/" in kind:
        raise ValueError(f"claim kind must not contain '/': {kind!r}")
    return f"{CLAIMS_PREFIX}/{kind}/{key}"


def parse_claim_ref(ref: str) -> Optional[tuple]:
    """(kind, key) for a `refs/fleet/claims/<kind>/<key>` ref, or None if
    `ref` doesn't match that shape.
    """
    prefix = CLAIMS_PREFIX + "/"
    if not ref.startswith(prefix):
        return None
    rest = ref[len(prefix):]
    if "/" not in rest:
        return None
    kind, key = rest.split("/", 1)
    return kind, key


# ---------------------------------------------------------------------- #
# Time helpers
# ---------------------------------------------------------------------- #


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


def _iso(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).isoformat()


def _parse_iso(value: str) -> datetime:
    dt = datetime.fromisoformat(value)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt


def is_expired(payload: dict, now: Optional[datetime] = None) -> bool:
    """True if `payload["expires_at"]` is at or before `now`.

    A payload with no (or unparseable) `expires_at` is treated as *not*
    expired -- absence of a deadline is not itself a deadline. Callers that
    require a well-formed claim payload should validate that separately.
    """
    now = now or _utcnow()
    raw = payload.get("expires_at")
    if not raw:
        return False
    try:
        return _parse_iso(raw) <= now
    except (ValueError, TypeError):
        return False


# ---------------------------------------------------------------------- #
# Toolchain identity (rustc_id / platform_id)
# ---------------------------------------------------------------------- #


def _rustc_vv(timeout: int = 10) -> str:
    """Raw `rustc -vV` output, replicating the gate's PATH resolution
    (`$HOME/.cargo/bin` before anything a login shell would otherwise pick
    up) -- see docs/FLEET.md, "Toolchain must be measured the way the gate
    resolves it." Returns "" if rustc cannot be found or run; callers hash
    whatever they get, so an empty toolchain is still a stable (if useless)
    id rather than a crash.
    """
    env = dict(os.environ)
    cargo_bin = str(Path.home() / ".cargo" / "bin")
    env["PATH"] = cargo_bin + os.pathsep + env.get("PATH", "")
    try:
        result = subprocess.run(
            ["rustc", "-vV"], capture_output=True, timeout=timeout, env=env
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    if result.returncode != 0:
        return ""
    return result.stdout.decode("utf-8", "replace")


def compute_platform_id(rustc_vv: Optional[str] = None) -> str:
    """sha256(`rustc -vV`), UNSTRIPPED. Part of a verdict/claim's transfer
    identity -- see module docstring.
    """
    text = _rustc_vv() if rustc_vv is None else rustc_vv
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def compute_rustc_id(rustc_vv: Optional[str] = None) -> str:
    """sha256(`rustc -vV`) with the `host:` line stripped. Canonical
    compiler identity -- see module docstring.
    """
    text = _rustc_vv() if rustc_vv is None else rustc_vv
    kept = [
        line for line in text.splitlines() if not line.strip().startswith("host:")
    ]
    return hashlib.sha256("\n".join(kept).encode("utf-8")).hexdigest()


# ---------------------------------------------------------------------- #
# The lease
# ---------------------------------------------------------------------- #


@dataclass
class ClaimHandle:
    """A lightweight, picklable-ish summary of a held claim -- not the live
    `Claim` object itself (which owns a thread and isn't safe to hand
    across processes), just its identifying fields for logging/tests.
    """

    ref: str
    kind: str
    key: str
    sha: str
    started_at: str
    expires_at: str


class Claim:
    """A lease over `refs/fleet/claims/<kind>/<key>`.

    `acquire()`/`acquire_or_reap()` take the lease AND start the renewer;
    `release()`/`__exit__` stop the renewer and drop the lease. Holding
    without renewing is not expressible (module docstring explains why
    that matters more than it sounds). Because release only happens on the
    way out, a process that crashes first leaves the claim to expire and
    be reaped -- it cannot leak forever.

    Read `lost` (and `lost_reason`) to find out whether the lease is still
    ours; a True there is a stop-work order, not a warning.
    """

    def __init__(
        self,
        hub: Hub,
        kind: str,
        key: str,
        *,
        work_kind: Optional[str] = None,
        work_key: Optional[str] = None,
        gate_version: str = "",
        rustc_id: Optional[str] = None,
        platform_id: Optional[str] = None,
        workdir: Optional[str] = None,
        holder_host: Optional[str] = None,
        pid: Optional[int] = None,
        pgid: Optional[int] = None,
        ttl: Optional[float] = None,
        renew_interval: Optional[float] = None,
    ):
        self.hub = hub
        self.kind = kind
        self.key = key
        self.ref = claim_ref(kind, key)
        self.work_kind = work_kind or kind
        self.work_key = work_key if work_key is not None else key
        self.gate_version = gate_version
        self._rustc_id = rustc_id
        self._platform_id = platform_id
        self.workdir = workdir
        self.holder_host = holder_host or socket.gethostname()
        self.pid = pid if pid is not None else os.getpid()
        try:
            self.pgid = pgid if pgid is not None else os.getpgrp()
        except (AttributeError, OSError):
            self.pgid = self.pid
        # Precedence: explicit argument > env var > module constant.
        self.ttl = _env_seconds(TTL_ENV, LEASE_TTL) if ttl is None else float(ttl)
        requested_renew = (
            _env_seconds(RENEW_ENV, RENEW) if renew_interval is None else float(renew_interval)
        )
        # A renewal cadence at or past the TTL is a lease that expires
        # between renewals -- structurally unrenewable, which is the exact
        # failure this module exists to make unreachable. Clamp to at most
        # ttl/2 (two chances per lease) rather than trusting the caller
        # got the ratio right. Clamping can only ever renew MORE often
        # than asked, so it cannot cause a false `lost`.
        self.renew_interval = requested_renew
        if self.ttl > 0:
            self.renew_interval = max(0.05, min(requested_renew, self.ttl / 2.0))

        self._sha: Optional[str] = None
        self._started_at: Optional[datetime] = None
        self._last_renew_ok: Optional[datetime] = None
        self._released = False
        self._lock = threading.Lock()
        # `lost` gets its OWN lock. The renewer holds `self._lock` across a
        # `git push` that can block for 30s; a consumer polling `lost` in a
        # reconcile loop must never stall behind that.
        self._state_lock = threading.Lock()
        self._lost = False
        self._lost_reason = ""
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None

    # -- payload -------------------------------------------------------- #

    def _resolved_rustc_id(self) -> str:
        if self._rustc_id is None:
            self._rustc_id = compute_rustc_id()
        return self._rustc_id

    def _resolved_platform_id(self) -> str:
        if self._platform_id is None:
            self._platform_id = compute_platform_id()
        return self._platform_id

    def _payload(self, now: datetime) -> dict:
        started = self._started_at or now
        payload = {
            "holder_host": self.holder_host,
            "pid": self.pid,
            "pgid": self.pgid,
            "work_kind": self.work_kind,
            "work_key": self.work_key,
            "started_at": _iso(started),
            "expires_at": _iso(now + timedelta(seconds=self.ttl)),
            "gate_version": self.gate_version,
            "rustc_id": self._resolved_rustc_id(),
            "platform_id": self._resolved_platform_id(),
        }
        if self.workdir is not None:
            payload["workdir"] = self.workdir
        return payload

    # -- lost state ----------------------------------------------------- #

    @property
    def lost(self) -> bool:
        """True once the renewer established the lease is no longer ours.

        STOP WORK when this is True -- another host may already be running
        the same work. Sticky until the next `acquire()`.
        """
        with self._state_lock:
            return self._lost

    @property
    def lost_reason(self) -> str:
        """Why the lease was lost, for logging. "" while `lost` is False."""
        with self._state_lock:
            return self._lost_reason

    def _mark_lost(self, reason: str) -> None:
        with self._state_lock:
            # A renewal still in flight when `release()` deleted the ref
            # will fail its CAS. That is us tidying up after ourselves,
            # not a lease stolen out from under running work, and it must
            # not be reported as one.
            if self._lost or self._released:
                return
            self._lost = True
            self._lost_reason = reason

    def _clear_lost(self) -> None:
        with self._state_lock:
            self._lost = False
            self._lost_reason = ""

    def _owns(self, payload: Optional[dict]) -> bool:
        """Is `payload` the claim WE acquired?

        The token is (holder_host, started_at): `started_at` is fixed at
        acquire and rewritten verbatim by every renewal, so it identifies
        this acquisition even across a reap-and-recreate by another
        process on the same host. It deliberately does NOT include pid --
        `fleetd.start_gate` rewrites pid/pgid into the payload after the
        gate is spawned, and an ownership test that changed underneath
        that rewrite would declare a healthy claim lost.
        """
        if payload is None or self._started_at is None:
            return False
        return (
            payload.get("holder_host") == self.holder_host
            and payload.get("started_at") == _iso(self._started_at)
        )

    # -- lifecycle -------------------------------------------------------#

    def acquire(self) -> "Claim":
        """Acquire the lease AND start the background renewer. Raises
        `ClaimHeldError` if another live (unexpired) holder already has
        it. Does NOT reap a stale claim itself -- see `acquire_or_reap`
        for that behaviour, which is what the context manager uses.
        """
        now = _utcnow()
        with self._lock:
            # Re-acquiring a released/lost Claim object must not carry the
            # previous acquisition's identity into the new payload.
            self._started_at = None
            self._released = False
            payload = self._payload(now)
            won = self.hub.create(self.ref, payload)
            if won:
                self._started_at = now
                self._last_renew_ok = now
                self._sha = self.hub.sha(self.ref)
        if not won:
            raise ClaimHeldError(f"{self.ref} is already held")
        self._clear_lost()
        # Renewal is not the caller's job (module docstring). Every path
        # that takes a lease starts the renewer here; the context manager
        # adds nothing, so `with Claim(...)` cannot double-start it.
        self.start_renewer()
        return self

    def acquire_or_reap(self) -> "Claim":
        """Acquire the lease; if it's held but the holder's lease has
        expired, reap it (CAS'd against the exact sha observed) and retry
        once. If a second acquirer wins the reap-then-create race, this
        raises `ClaimHeldError` rather than looping forever.
        """
        try:
            return self.acquire()
        except ClaimHeldError:
            pass

        existing_sha = self.hub.sha(self.ref)
        if existing_sha is not None:
            existing_payload = self.hub.read(self.ref)
            if existing_payload is not None and is_expired(existing_payload):
                # Best-effort: if this loses the CAS race to another
                # reaper, that's fine -- we just retry acquire() below,
                # which will raise ClaimHeldError if someone else's fresh
                # claim beat us to it.
                self.hub.delete(self.ref, expect_sha=existing_sha)

        return self.acquire()

    @classmethod
    def adopt(
        cls,
        hub: Hub,
        ref: str,
        *,
        expected_host: Optional[str] = None,
        ttl: Optional[float] = None,
        renew_interval: Optional[float] = None,
    ) -> Optional["Claim"]:
        """Rebuild a live `Claim` around a claim ref THIS HOST already holds,
        continuing the existing acquisition instead of starting a new one
        (ARCH-FIX-SPEC.md R6). Returns None if the ref is not ours to adopt.

        `fleetd` calls this on start for every claim whose `holder_host` is
        this host and whose recorded process group is still alive: the
        daemon died, the gate it launched did not, and the lease must go on
        being renewed by whoever is now supervising that process.

        ADOPTION IS NOT RE-ACQUISITION, and the difference is the whole
        contract:

          * `started_at` is read from the REF, never recomputed as `now`.
            `(holder_host, started_at)` is this module's ownership token
            (`_owns`), so minting a fresh `started_at` would make our very
            first renewal look, to every other observer, exactly like
            another process having stolen the claim -- including to a
            predecessor that turned out to be alive after all, which would
            then correctly declare its own lease lost and kill a healthy
            gate.
          * The ref is never deleted and re-created. `acquire()` would have
            to (its CAS is `create`), and the gap between the delete and the
            create is a window in which another host sees the branch as
            unclaimed. Adoption is a plain CAS `update` from the sha
            already on the hub, so the claim's existence is continuous
            across the handover.
          * Every payload field that describes the WORK -- `pid`, `pgid`,
            `work_kind`, `work_key`, `workdir`, `gate_version`, the two
            toolchain ids -- is restored from the ref, because `renew()`
            rewrites the payload from this object's attributes. Rebuilding
            with fresh defaults would overwrite the running gate's `pgid`
            with the daemon's own on the first renewal, and `pgid` is how
            anything ever finds that gate again.

        The lease is renewed once, synchronously, before this returns: an
        adopted lease is usually close to expiry (its renewer died with the
        old daemon), and the renewal doubles as the proof that the claim is
        still ours. If that renewal establishes the lease is gone, adoption
        FAILS rather than half-succeeding -- the caller then treats the
        process as an orphan, which is right, because whoever took the claim
        may already be running the same work.
        """
        parsed = parse_claim_ref(ref)
        if parsed is None:
            return None
        kind, key = parsed

        payload = hub.read(ref)
        if payload is None:
            return None

        host = expected_host if expected_host is not None else socket.gethostname()
        if payload.get("holder_host") != host:
            # Another host's lease. Not ours to renew, release or reason
            # about -- the single most important thing this method does NOT
            # do.
            return None

        raw_started = payload.get("started_at")
        if not raw_started:
            return None
        try:
            started = _parse_iso(raw_started)
        except (ValueError, TypeError):
            return None
        if _iso(started) != raw_started:
            # We could not reproduce the token's exact text, so `_owns`
            # would reject our own renewals. Refuse loudly-by-None rather
            # than adopt a claim we are structurally unable to hold.
            return None

        sha = hub.sha(ref)
        if sha is None:
            return None

        c = cls(
            hub,
            kind,
            key,
            work_kind=payload.get("work_kind") or kind,
            work_key=payload.get("work_key") if payload.get("work_key") is not None else key,
            gate_version=payload.get("gate_version") or "",
            rustc_id=payload.get("rustc_id"),
            platform_id=payload.get("platform_id"),
            workdir=payload.get("workdir"),
            holder_host=host,
            pid=payload.get("pid"),
            pgid=payload.get("pgid"),
            ttl=ttl,
            renew_interval=renew_interval,
        )
        c._started_at = started
        c._sha = sha
        c._released = False
        # Anchor the renewal deadline on the HUB's `expires_at`, not on now.
        # `_note_renew_failure` computes "when does this lease die" as
        # `_last_renew_ok + ttl`; anchoring at adoption time would claim a
        # full fresh TTL of grace that the hub never agreed to, and would
        # delay the `lost` declaration past the moment another host may
        # legitimately reap us -- exactly the race the flag exists to beat.
        anchor = started
        raw_expires = payload.get("expires_at")
        if raw_expires:
            try:
                anchor = _parse_iso(raw_expires) - timedelta(seconds=c.ttl)
            except (ValueError, TypeError):
                anchor = started
        c._last_renew_ok = anchor
        c._clear_lost()

        # Renew now: refresh a lease whose renewer died, and prove ownership
        # before reporting success. A transient failure that does NOT cost
        # the lease is fine -- the background renewer will retry.
        c.renew()
        if c.lost:
            return None
        c.start_renewer()
        return c

    def renew(self) -> bool:
        """Push a fresh `expires_at` one `ttl` out. Returns False and never
        raises. A False that means "the lease is gone" also sets `lost`;
        a False that means "one attempt failed, the lease is still ours"
        does not.

        The current sha is re-read from the hub each time rather than
        trusted from the last write: a renewal whose CAS succeeded but
        whose follow-up `sha()` failed would otherwise leave us holding a
        stale `expect_sha` and declare a perfectly healthy lease lost on
        the next cycle.
        """
        with self._lock:
            if self._started_at is None or self._released:
                return False
            if self.lost:
                return False  # sticky: stop hammering the hub
            now = _utcnow()
            try:
                current = self.hub.sha(self.ref)
                if current is None:
                    self._mark_lost(
                        f"{self.ref} no longer exists on the hub "
                        "(reaped as expired, or deleted)"
                    )
                    return False
                if current != self._sha:
                    payload = self.hub.read(self.ref)
                    if not self._owns(payload):
                        holder = (payload or {}).get("holder_host", "?")
                        started = (payload or {}).get("started_at", "?")
                        self._mark_lost(
                            f"{self.ref} is now held by {holder} (started_at "
                            f"{started}); our acquisition was superseded"
                        )
                        return False
                    # Still ours -- our own earlier write, whose sha we
                    # never got to observe. Adopt it and carry on.
                    self._sha = current
                ok = self.hub.update(self.ref, self._payload(now), expect_sha=self._sha)
            except HubError as exc:
                # HubError, not just HubUnreachableError. A renewal can
                # also fail for entirely local reasons -- the object cache
                # under ~/.fleetd deleted or corrupted, a full disk, an
                # unreadable payload -- and the hub-side `expires_at` goes
                # on ticking through every one of them. Catching only the
                # network case left those failures raising out of the
                # renewer thread, which then died with a traceback nobody
                # reads, `lost` still False, and the lease quietly
                # unrenewed: the exact original defect, reintroduced by a
                # narrower `except`.
                self._note_renew_failure(now, exc)
                return False
            if not ok:
                self._mark_lost(
                    f"CAS renewal of {self.ref} was rejected against "
                    f"{self._sha}; another writer moved it"
                )
                return False
            self._last_renew_ok = now
            try:
                self._sha = self.hub.sha(self.ref)
            except HubError:
                # The renewal LANDED; only our readback failed. Leave the
                # stale sha in place -- the re-read at the top of the next
                # renewal repairs it via the ownership token.
                pass
            return True

    def _note_renew_failure(self, now: datetime, exc: Exception) -> None:
        """Decide whether repeated renewal failure has cost us the lease.

        One blip must not stop a gate. But "we could not write the
        renewal" is not the same as "the lease is safe": the hub-side
        `expires_at` is ticking regardless of what we believe or why we
        failed, and once it passes, any host may legitimately reap the
        claim and start the same work. So we declare the lease lost one
        renewal interval BEFORE that deadline -- the last moment at which
        no chance to save it remains -- rather than after, so the killer
        beats the duplicate.
        """
        anchor = self._last_renew_ok or self._started_at
        if anchor is None:
            return
        expires_at = anchor + timedelta(seconds=self.ttl)
        if now + timedelta(seconds=self.renew_interval) >= expires_at:
            self._mark_lost(
                f"no successful renewal of {self.ref} since {_iso(anchor)}; it "
                f"expires at {_iso(expires_at)} with no renewal left to save "
                f"it ({type(exc).__name__}: {exc})"
            )

    def release(self) -> bool:
        """Stop the renewer and delete the claim if we still hold it.
        Idempotent: calling this twice, or on a claim that was never
        acquired, is a harmless no-op.
        """
        # Stop the renewer BEFORE taking `self._lock`: `stop_renewer`
        # joins the renewer thread, which may itself be holding that lock
        # inside a `git push`. Taking it first would deadlock.
        self.stop_renewer()
        with self._lock:
            self._released = True
            if self._sha is None:
                return True
            try:
                ok = self.hub.delete(self.ref, expect_sha=self._sha)
                if not ok:
                    # Stale sha (a readback we never got) or somebody
                    # else's claim now. Only the former is ours to delete,
                    # and the ownership token tells them apart -- leaving
                    # a released claim to sit out its whole TTL blocks the
                    # branch for no reason.
                    current = self.hub.sha(self.ref)
                    if current is not None and self._owns(self.hub.read(self.ref)):
                        ok = self.hub.delete(self.ref, expect_sha=current)
            except HubUnreachableError:
                return False
            self._sha = None
            return ok

    def handle(self) -> Optional[ClaimHandle]:
        if self._sha is None or self._started_at is None:
            return None
        now = _utcnow()
        return ClaimHandle(
            ref=self.ref,
            kind=self.kind,
            key=self.key,
            sha=self._sha,
            started_at=_iso(self._started_at),
            expires_at=_iso(now + timedelta(seconds=self.ttl)),
        )

    # -- background renewer ---------------------------------------------#

    def _renew_loop(self) -> None:
        while not self._stop.wait(self.renew_interval):
            try:
                self.renew()
            except Exception as exc:  # noqa: BLE001 -- see below
                # A renewer thread that dies on an unexpected exception
                # leaves the lease unrenewed, `lost` unset and nothing
                # logged -- silently restoring the very defect this
                # module exists to prevent. Nothing is allowed out of
                # here; anything unexpected is treated as a renewal
                # failure and becomes `lost` once the lease can no longer
                # be saved.
                self._note_renew_failure(_utcnow(), exc)
            if self.lost:
                # Nothing left to renew. The flag stays set for whoever
                # polls it; exiting stops us pushing at a hub that has
                # already told us the answer.
                return

    def start_renewer(self) -> None:
        """Idempotent -- a second call while the renewer runs is a no-op.

        That is what lets `acquire()` own renewal unconditionally without
        the context manager double-starting a thread.
        """
        if self._thread is not None:
            return
        if self.ttl <= 0:
            # A zero/negative TTL claim is deliberately already expired
            # (test fixtures simulating a crashed holder). There is
            # nothing to keep alive, and a renewer at the clamped
            # interval would spin.
            return
        self._stop.clear()
        self._thread = threading.Thread(
            target=self._renew_loop, name=f"claim-renew-{self.kind}-{self.key}", daemon=True
        )
        self._thread.start()

    def stop_renewer(self, timeout: float = 5.0) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout)
            self._thread = None

    def renewer_running(self) -> bool:
        """Is the background renewer thread alive? For tests and doctors."""
        return self._thread is not None and self._thread.is_alive()

    # -- context manager ---------------------------------------------- #

    def __enter__(self) -> "Claim":
        # No `start_renewer()` here on purpose: `acquire_or_reap` ->
        # `acquire` already started it, and renewal must not depend on
        # which entry point a caller happened to use.
        self.acquire_or_reap()
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        self.release()  # stops the renewer first, then drops the lease
        return False


# ---------------------------------------------------------------------- #
# Reaping
# ---------------------------------------------------------------------- #


def list_claims(hub: Hub, kind: Optional[str] = None) -> dict:
    """{ref: sha} for every claim on the hub, optionally restricted to one
    `kind`.
    """
    prefix = f"{CLAIMS_PREFIX}/{kind}" if kind else CLAIMS_PREFIX
    return hub.list(prefix)


def reap_expired(hub: Hub, kind: Optional[str] = None, now: Optional[datetime] = None) -> list:
    """Delete every expired claim (optionally scoped to `kind`), each via a
    CAS'd delete against the exact sha this reaper observed.

    Two reapers racing on the same expired claim: both list it and both
    read the same `expect_sha`, but only one `Hub.delete` wins -- the
    other's `--force-with-lease` is stale and `Hub.delete` returns False,
    which this function reports as "not reaped by us" (it is silently
    excluded from the returned list) rather than raising. That is the
    direct fix for incident 12 (`rm -rf ~/tgt/*` racing a live gate): a
    second reaper can never delete a claim it didn't observe as still
    being the current one.
    """
    now = now or _utcnow()
    reaped = []
    for ref, sha in list_claims(hub, kind=kind).items():
        payload = hub.read(ref)
        if payload is None:
            # Deleted between list() and read() -- already gone, nothing
            # for us to do.
            continue
        if not is_expired(payload, now=now):
            continue
        if hub.delete(ref, expect_sha=sha):
            reaped.append(ref)
    return reaped


def is_claim_live(hub: Hub, kind: str, key: str, now: Optional[datetime] = None) -> bool:
    """True iff `refs/fleet/claims/<kind>/<key>` exists and its lease has
    not yet expired. An existing-but-expired claim is NOT live -- it is
    reap-eligible, and treating it as live would let a dead lease block
    forever.
    """
    ref = claim_ref(kind, key)
    payload = hub.read(ref)
    if payload is None:
        return False
    return not is_expired(payload, now=now)


def is_workdir_claimed(hub: Hub, workdir: str, now: Optional[datetime] = None) -> bool:
    """Is target directory `workdir` (e.g. `nc-3` for `~/tgt/nc-3`) named by
    any live claim, of any kind?

    This is the interlock against incident 12: `rm -rf ~/tgt/*` once killed
    7 running gates because cleanup had no way to ask "is anything actually
    using this directory right now?" A cleanup script must call this (or
    equivalently check the returned claim) before removing a `~/tgt/nc-*`
    directory, and must never delete one for which this returns True.
    """
    now = now or _utcnow()
    for ref in list_claims(hub):
        payload = hub.read(ref)
        if payload is None:
            continue
        if payload.get("workdir") != workdir:
            continue
        if is_expired(payload, now=now):
            continue
        return True
    return False
