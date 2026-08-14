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

A background thread renews the lease every `RENEW` seconds so a
long-running holder never has its claim expire out from under it. If the
holder process crashes (killed, segfaults, `os._exit`) before `__exit__`
runs, nothing releases the claim -- that is intentional: the claim simply
expires at `expires_at` and becomes reapable by `reap_expired`, which is
the whole point of a lease instead of a lock. A crash can leak a claim for
at most `LEASE_TTL` seconds; it can never leak one silently forever.

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

LEASE_TTL = 600  # seconds a claim is valid for before it is reapable
RENEW = 120  # seconds between background lease renewals

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

    Use as a context manager. `__enter__` acquires (reaping a stale expired
    claim first if one is in the way) and starts a background renewer
    thread; `__exit__` stops the renewer and releases the claim. Because
    release only happens in `__exit__`, a process that crashes before then
    leaves the claim to expire and be reaped -- it cannot leak forever.
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
        ttl: int = LEASE_TTL,
        renew_interval: int = RENEW,
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
        self.ttl = ttl
        self.renew_interval = renew_interval

        self._sha: Optional[str] = None
        self._started_at: Optional[datetime] = None
        self._lock = threading.Lock()
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

    # -- lifecycle -------------------------------------------------------#

    def acquire(self) -> "Claim":
        """Acquire the lease. Raises `ClaimHeldError` if another live
        (unexpired) holder already has it. Does NOT reap a stale claim
        itself -- see `acquire_or_reap` for that behaviour, which is what
        the context manager uses.
        """
        now = _utcnow()
        payload = self._payload(now)
        with self._lock:
            if self.hub.create(self.ref, payload):
                self._sha = self.hub.sha(self.ref)
                self._started_at = now
                return self
        raise ClaimHeldError(f"{self.ref} is already held")

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

    def renew(self) -> bool:
        """Push a fresh `expires_at` `renew_interval` seconds out. Returns
        False (never raises) if the lease was lost from under us -- e.g.
        a reaper decided we were expired and deleted the ref. Callers that
        care should treat a False renew as "the claim is gone, stop work."
        """
        with self._lock:
            if self._sha is None:
                return False
            now = _utcnow()
            payload = self._payload(now)
            try:
                ok = self.hub.update(self.ref, payload, expect_sha=self._sha)
            except HubUnreachableError:
                # Transient network blip: don't tear down the lease over
                # one failed renewal attempt, but don't advance our
                # tracked sha either -- next renewal will retry against
                # the same expect_sha.
                return False
            if ok:
                self._sha = self.hub.sha(self.ref)
            return ok

    def release(self) -> bool:
        """Delete the claim if we still hold it. Idempotent: calling this
        twice, or on a claim that was never acquired, is a harmless no-op.
        """
        with self._lock:
            if self._sha is None:
                return True
            try:
                ok = self.hub.delete(self.ref, expect_sha=self._sha)
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
            self.renew()

    def start_renewer(self) -> None:
        if self._thread is not None:
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

    # -- context manager ---------------------------------------------- #

    def __enter__(self) -> "Claim":
        self.acquire_or_reap()
        self.start_renewer()
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        self.stop_renewer()
        self.release()
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
