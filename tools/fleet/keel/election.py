#!/usr/bin/env python3
"""The server lease, settle, unreachable-demotion and re-host machinery
(docs/AGENT-SERVER-SPEC.md SS3.3 rule 5/8, SS3.4; PLAN Stage 2 task 5).

WHAT THIS OWNS. `keel-server`'s own singleton is a lease exactly like
every worker's: `refs/fleet/claims/server/singleton`, written through
`claim.py` UNCHANGED -- acquire is `Hub.create` (the CAS that elects
exactly one of N candidates), renewal is the `Claim` renewer thread, and
losing the CAS is `ClaimHeldError`, never an error. This module adds only
what the SPEC says the server kind adds:

  * payload extras `advertise_urls[]`, `boot_id`, `keel_version` (SS3.1
    "lease" row) -- `ServerClaim` subclasses `Claim` and widens `_payload`,
    so every renewal re-carries the extras; claim.py itself is not edited.
  * TTL 120 s / renew 30 s (SS3.3 rule 5), overridable by the SAME env
    knobs every other claim compresses under (`FLEET_TEST_TTL_S` /
    `FLEET_TEST_RENEW_S`, read at `ElectionConfig.from_env()` time).
  * rank backoff (SS3.4 trigger): a candidate of rank r may attempt the
    lease only once `now > expires_at + r * 60 s`; an ABSENT lease has no
    `expires_at` to back off from and is contestable immediately -- the
    CAS still elects exactly one.
  * the settle window (SS3.3 rule 8): for 180 s after (re)election the
    server "registers runners, serves reads, writes nothing but its own
    lease, offers nothing". This stage has no scheduler or train to hold
    back yet; what exists of the rule here is the FLAG -- `settling` in
    `/v1/status` and `settle_until` in `/v1/health` -- which the Stage 4
    scheduler and train thread will consult before offering anything.
  * unreachable-leader demotion (SS3.4 step 7): if no runner has
    registered by 180 s AFTER settle ends, the server releases its lease,
    logs `server.unreachable-demoted`, and exits 4, so the next-ranked
    candidate proceeds. (Judge 3's "healthy lease, nobody can reach it".)
  * `rehost`: the manual path (SS3.4 "Manual"). Refuses on a LIVE lease
    (exit 3); proceeds on an expired or absent one, skipping rank backoff
    -- a human asked, so the human's host is the candidate.

WHAT THIS DOES NOT OWN. The transport (`server.py`) only exposes the
flags this module computes: `KeelHTTPServer.election` is duck-typed and
`server.py` never imports this module (no cycle; the transport tests keep
needing neither git nor a lease). Scheduling holds, the runner-side
candidate loop, and `keel server move` live in later stages. The durable
`server.unreachable-demoted` note in the host's heartbeat ref (SS3.4
step 7) lands with the heartbeat writer (Stage 4); here the demotion is
observable in the event ring, the log, exit code 4, and the released
lease itself.

WHY DEMOTION ANCHORS ON REGISTRATIONS-SINCE-ELECTION. The demotion timer
asks "can anything reach the advertised URLs", and a runner's `register`
is the only inbound proof this stage has. Registrations are counted from
this manager's election onward and are NOT reset when the incumbent
re-acquires after a lease blip (`boot_id` never changes within a process,
so runners have no reason to re-register; resetting the count would make
a healthy, fully-connected server demote itself after a GitHub hiccup).

Exit codes (`main`): 0 clean stop (signal; lease released), 2 usage or
startup failure, 3 rehost refused (live lease), 4 unreachable-demoted.

Standard library only (everything under `tools/fleet/keel/` is).
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import signal
import socket
import sys
import threading
import time
import uuid
from dataclasses import dataclass
from datetime import datetime, timedelta
from pathlib import Path
from typing import Callable, Dict, List, Optional, Sequence

_KEEL_DIR = Path(__file__).resolve().parent
_FLEET_DIR = _KEEL_DIR.parent
for _p in (_FLEET_DIR, _KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import claim as claimlib  # noqa: E402
from claim import Claim, ClaimHeldError, claim_ref, is_expired  # noqa: E402
from fleetlib import Hub, HubError  # noqa: E402

KEEL_VERSION = "0.1"

SERVER_KIND = "server"
SERVER_KEY = "singleton"
SERVER_LEASE_REF = claim_ref(SERVER_KIND, SERVER_KEY)  # refs/fleet/claims/server/singleton

# Production numbers (SPEC SS3.3 rule 5, SS3.4). Each is compressible for
# tests through the env knob named beside it, read at
# `ElectionConfig.from_env()` time -- construction, not import, so a test
# that sets the variable after importing this module still gets the
# compressed value (same rule as claim.py's).
SERVER_TTL_S = 120.0  # FLEET_TEST_TTL_S (claim.py's own knob)
SERVER_RENEW_S = 30.0  # FLEET_TEST_RENEW_S (claim.py's own knob)
SETTLE_S = 180.0  # FLEET_TEST_SETTLE_S
DEMOTION_S = 180.0  # FLEET_TEST_DEMOTION_S
RANK_BACKOFF_S = 60.0  # FLEET_TEST_RANK_BACKOFF_S
POLL_S = 30.0  # FLEET_TEST_ELECTION_POLL_S

SETTLE_ENV = "FLEET_TEST_SETTLE_S"
DEMOTION_ENV = "FLEET_TEST_DEMOTION_S"
RANK_BACKOFF_ENV = "FLEET_TEST_RANK_BACKOFF_S"
POLL_ENV = "FLEET_TEST_ELECTION_POLL_S"

EXIT_OK = 0
EXIT_USAGE = 2
EXIT_REHOST_REFUSED = 3
EXIT_UNREACHABLE_DEMOTED = 4


def _env_seconds(name: str, default: float) -> float:
    # claim.py's parser, on purpose: same "unset/unparseable/negative means
    # the production default, never a crash" contract for every knob here.
    return claimlib._env_seconds(name, default)


@dataclass(frozen=True)
class ElectionConfig:
    """The five timers plus the poll cadence, resolved once.

    `from_env()` is the constructor everything in this module uses:
    explicit dataclass arguments beat env vars beat the production
    constants -- claim.py's precedence, applied to the server's numbers.
    """

    ttl: float = SERVER_TTL_S
    renew_interval: float = SERVER_RENEW_S
    settle_s: float = SETTLE_S
    demotion_s: float = DEMOTION_S
    rank_backoff_s: float = RANK_BACKOFF_S
    poll_s: float = POLL_S

    @classmethod
    def from_env(cls, **overrides: float) -> "ElectionConfig":
        values = dict(
            ttl=_env_seconds(claimlib.TTL_ENV, SERVER_TTL_S),
            renew_interval=_env_seconds(claimlib.RENEW_ENV, SERVER_RENEW_S),
            settle_s=_env_seconds(SETTLE_ENV, SETTLE_S),
            demotion_s=_env_seconds(DEMOTION_ENV, DEMOTION_S),
            rank_backoff_s=_env_seconds(RANK_BACKOFF_ENV, RANK_BACKOFF_S),
            poll_s=_env_seconds(POLL_ENV, POLL_S),
        )
        values.update({k: float(v) for k, v in overrides.items()})
        return cls(**values)


class ServerClaim(Claim):
    """`Claim(kind="server", key="singleton")` plus the three payload
    extras SS3.1's lease row names for the server kind. `_payload` is the
    ONE method widened: it is what `acquire` and every `renew` serialize,
    so the extras survive each renewal instead of appearing only at
    acquisition. Everything else -- CAS, renewer, ownership token, `lost`,
    adopt, reap -- is claim.py, unchanged and untouched.
    """

    def __init__(
        self,
        hub: Hub,
        *,
        advertise_urls: Sequence[str],
        boot_id: str,
        keel_version: str = KEEL_VERSION,
        ttl: Optional[float] = None,
        renew_interval: Optional[float] = None,
        holder_host: Optional[str] = None,
    ):
        super().__init__(
            hub,
            SERVER_KIND,
            SERVER_KEY,
            work_kind=SERVER_KIND,
            work_key=SERVER_KEY,
            holder_host=holder_host,
            ttl=ttl,
            renew_interval=renew_interval,
        )
        self.advertise_urls = list(advertise_urls)
        self.boot_id = boot_id
        self.keel_version = keel_version

    def _payload(self, now: datetime) -> dict:
        payload = super()._payload(now)
        payload["advertise_urls"] = list(self.advertise_urls)
        payload["boot_id"] = self.boot_id
        payload["keel_version"] = self.keel_version
        return payload


class RehostRefusedError(Exception):
    """`rehost` against a LIVE (unexpired) lease. Carries who holds it and
    until when, because the operator's next question is always "whose"."""

    def __init__(self, holder_host: str, expires_at: str):
        self.holder_host = holder_host
        self.expires_at = expires_at
        super().__init__(
            f"{SERVER_LEASE_REF} is live: held by {holder_host!r} until "
            f"{expires_at} -- refusing to re-host over a living server "
            "(SPEC SS3.4: a live lease makes it refuse, exit 3)"
        )


class ElectionManager:
    """One candidate's (and, once elected, one leader's) view of the
    server lease: eligibility with rank backoff, the settle window, the
    registration count, and the unreachable-demotion decision.

    Thread model: `try_elect`/`rehost`/`release` are called by whoever
    drives the process (CLI main loop, Stage-3 runner, a test);
    `note_registration`/`settling`/`health_fields`/`status_fields` are
    called from HTTP handler threads; `_watch_loop` runs on its own
    thread. Shared state sits behind `self._lock`; the `Claim` object has
    its own locking and its reads (`lost`) never block behind a push.

    The demotion DECISION is `demotion_due(now_mono)`, a pure function of
    recorded state and a passed-in clock reading, so tests pin the
    boundary exactly instead of racing a background thread; the watch
    thread is just "call it on a tick".
    """

    def __init__(
        self,
        hub: Hub,
        *,
        host: Optional[str] = None,
        rank: int = 1,
        advertise_urls: Sequence[str] = (),
        boot_id: Optional[str] = None,
        keel_version: str = KEEL_VERSION,
        config: Optional[ElectionConfig] = None,
        events=None,  # duck-typed EventLog (server.py); .append(kind, payload)
        on_demote: Optional[Callable[[], None]] = None,
        clock: Callable[[], float] = time.monotonic,
    ):
        self.hub = hub
        self.host = host or socket.gethostname()
        self.rank = int(rank)
        self.advertise_urls = list(advertise_urls)
        self.boot_id = boot_id or uuid.uuid4().hex
        self.keel_version = keel_version
        self.config = config or ElectionConfig.from_env()
        self.events = events
        self.on_demote = on_demote
        self._clock = clock

        self._lock = threading.Lock()
        self.claim: Optional[ServerClaim] = None
        self._settle_until_mono: Optional[float] = None
        self._settle_until_wall: Optional[float] = None
        self._settle_end_emitted = False
        self._registrations: Dict[str, float] = {}
        self._demoted = False
        self.demoted_event = threading.Event()
        self._stop = threading.Event()
        self._watch_thread: Optional[threading.Thread] = None
        self._last_reacquire_mono = float("-inf")

    # -- events --------------------------------------------------------- #

    def _emit(self, kind: str, **payload) -> None:
        logging.info("keel-election: %s %s", kind, payload)
        if self.events is not None:
            try:
                self.events.append(kind, payload)
            except Exception:
                logging.exception("keel-election: event append failed for %s", kind)

    # -- eligibility / election ----------------------------------------- #

    def eligible_at(self, payload: Optional[dict]) -> Optional[datetime]:
        """The wall-clock instant from which THIS candidate may contest the
        lease, given the current lease payload. `None` = contestable now
        (the lease is absent -- no `expires_at` exists to back off from).
        A payload with an unparseable/missing `expires_at` never becomes
        eligible through this method: `is_expired` treats it as not
        expired, and `try_elect` refuses accordingly (fail safe: never
        contest a lease we cannot read a deadline off).
        """
        if payload is None:
            return None
        raw = payload.get("expires_at")
        if not raw:
            return None
        try:
            expires = claimlib._parse_iso(raw)
        except (ValueError, TypeError):
            return None
        return expires + timedelta(seconds=self.rank * self.config.rank_backoff_s)

    def try_elect(self, now: Optional[datetime] = None) -> bool:
        """One election attempt (SS3.4 steps 1). Returns True iff WE now
        hold the lease (settle begun); False when the lease is live, when
        rank backoff says "not yet", or when another candidate's CAS won.
        Transport failures raise `HubError` -- a candidate that cannot
        read the store has nothing safe to say about the lease.
        """
        now = now or claimlib._utcnow()
        payload = self.hub.read(SERVER_LEASE_REF)
        if payload is not None:
            if not is_expired(payload, now=now):
                return False  # a living leader; nothing to contest
            eligible = self.eligible_at(payload)
            if eligible is None and payload.get("expires_at"):
                return False  # unreadable deadline: fail safe, do not contest
            if eligible is not None and now < eligible:
                return False  # rank backoff: not our turn yet
        try:
            claim = self._new_claim()
            claim.acquire_or_reap()
        except ClaimHeldError:
            return False  # lost the CAS -- exactly one winner, not us
        self._begin_term(claim)
        return True

    def rehost(self, now: Optional[datetime] = None) -> None:
        """The manual path (SS3.4 "Manual"): proceed iff the lease is
        expired or absent, SKIPPING rank backoff; raise
        `RehostRefusedError` on a live lease (the CLI maps it to exit 3).
        Losing the reap-and-create race to another candidate is also a
        refusal -- by the time we lost, someone else's lease is live.
        """
        now = now or claimlib._utcnow()
        payload = self.hub.read(SERVER_LEASE_REF)
        if payload is not None and not is_expired(payload, now=now):
            raise RehostRefusedError(
                str(payload.get("holder_host", "?")), str(payload.get("expires_at", "?"))
            )
        try:
            claim = self._new_claim()
            claim.acquire_or_reap()
        except ClaimHeldError as exc:
            live = self.hub.read(SERVER_LEASE_REF) or {}
            raise RehostRefusedError(
                str(live.get("holder_host", "?")), str(live.get("expires_at", "?"))
            ) from exc
        self._begin_term(claim)

    def _new_claim(self) -> ServerClaim:
        return ServerClaim(
            self.hub,
            advertise_urls=self.advertise_urls,
            boot_id=self.boot_id,
            keel_version=self.keel_version,
            ttl=self.config.ttl,
            renew_interval=self.config.renew_interval,
            holder_host=self.host,
        )

    def _begin_term(self, claim: ServerClaim) -> None:
        with self._lock:
            self.claim = claim
            now_mono = self._clock()
            self._settle_until_mono = now_mono + self.config.settle_s
            self._settle_until_wall = time.time() + self.config.settle_s
            self._settle_end_emitted = False
            self._demoted = False
            # Registrations are NOT cleared: see the module docstring.
        self._emit(
            "server.elected",
            host=self.host,
            boot_id=self.boot_id,
            rank=self.rank,
            advertise_urls=list(self.advertise_urls),
            settle_until=self._settle_until_wall,
        )

    # -- settle --------------------------------------------------------- #

    def settling(self) -> bool:
        with self._lock:
            if self.claim is None or self._settle_until_mono is None:
                return False
            return self._clock() < self._settle_until_mono

    # -- registrations / demotion --------------------------------------- #

    def note_registration(self, runner_id: str) -> None:
        """A runner reached us. The one inbound proof the demotion timer
        accepts; the register route calls this."""
        with self._lock:
            self._registrations[str(runner_id)] = time.time()

    def registered_runner_ids(self) -> List[str]:
        with self._lock:
            return sorted(self._registrations)

    def demotion_due(self, now_mono: Optional[float] = None) -> bool:
        """SS3.4 step 7, as a decision: True iff we hold an un-lost lease,
        nobody has ever registered, and `demotion_s` has fully passed since
        settle ended."""
        with self._lock:
            if self._demoted or self.claim is None or self._settle_until_mono is None:
                return False
            if self.claim.lost:
                return False  # the lost-lease path, not the unreachable path
            if self._registrations:
                return False
            now_mono = self._clock() if now_mono is None else now_mono
            return now_mono >= self._settle_until_mono + self.config.demotion_s

    def demote_unreachable(self) -> None:
        """Release the lease so the next-ranked candidate proceeds without
        waiting out our TTL, mark ourselves demoted, and tell whoever runs
        us (exit 4 in `main`). Idempotent."""
        with self._lock:
            if self._demoted:
                return
            self._demoted = True
            claim = self.claim
        self._emit(
            "server.unreachable-demoted",
            host=self.host,
            boot_id=self.boot_id,
            settle_until=self._settle_until_wall,
            demotion_s=self.config.demotion_s,
        )
        logging.error(
            "keel-election: server.unreachable-demoted -- no runner registered "
            "within %.1fs after settle; releasing %s and exiting 4",
            self.config.demotion_s,
            SERVER_LEASE_REF,
        )
        if claim is not None:
            try:
                claim.release()
            except HubError:
                logging.exception("keel-election: releasing the lease on demotion failed")
        self.demoted_event.set()
        if self.on_demote is not None:
            try:
                self.on_demote()
            except Exception:
                logging.exception("keel-election: on_demote callback raised")

    @property
    def demoted(self) -> bool:
        with self._lock:
            return self._demoted

    # -- the watch thread ----------------------------------------------- #

    def _tick(self) -> bool:
        """One watch decision. Returns False when the watch should stop
        (we demoted). Split from the loop for direct testing."""
        # Emit settle_end exactly once per term.
        with self._lock:
            emit_settle_end = (
                self.claim is not None
                and not self._settle_end_emitted
                and self._settle_until_mono is not None
                and self._clock() >= self._settle_until_mono
            )
            if emit_settle_end:
                self._settle_end_emitted = True
        if emit_settle_end:
            self._emit("server.settle_end", host=self.host, boot_id=self.boot_id)
        if self.demotion_due():
            self.demote_unreachable()
            return False
        # SS7 "Server <-> GitHub partition": lease renew fails -> degraded,
        # re-acquire every poll_s. The renewer marked `lost` and exited;
        # a fresh term (fresh settle, SS3.3 rule 8) starts if we win.
        claim = self.claim
        if claim is not None and claim.lost:
            now_mono = self._clock()
            if now_mono - self._last_reacquire_mono >= self.config.poll_s:
                self._last_reacquire_mono = now_mono
                try:
                    if self.try_elect():
                        self._emit("server.reacquired", host=self.host, boot_id=self.boot_id)
                except HubError as exc:
                    logging.warning("keel-election: re-acquire attempt failed: %s", exc)
        return True

    def _watch_loop(self, interval: float) -> None:
        while not self._stop.wait(interval):
            try:
                if not self._tick():
                    return
            except Exception:
                logging.exception("keel-election: watch tick raised")

    def start_watch(self) -> None:
        """Idempotent, same shape as `Claim.start_renewer`."""
        if self._watch_thread is not None and self._watch_thread.is_alive():
            return
        self._stop.clear()
        # Tick often enough to hit compressed windows promptly and rarely
        # enough to be free at production scale (~1 Hz vs 180 s windows).
        interval = max(0.02, min(1.0, self.config.settle_s / 4.0, self.config.demotion_s / 4.0))
        self._watch_thread = threading.Thread(
            target=self._watch_loop, args=(interval,), name="keel-election-watch", daemon=True
        )
        self._watch_thread.start()

    def stop_watch(self, timeout: float = 5.0) -> None:
        self._stop.set()
        if self._watch_thread is not None:
            self._watch_thread.join(timeout=timeout)
            self._watch_thread = None

    # -- release -------------------------------------------------------- #

    def release(self) -> None:
        """Deliberate stop: stop watching, drop the lease if held."""
        self.stop_watch()
        with self._lock:
            claim = self.claim
        if claim is not None:
            claim.release()

    # -- reporting (server.py reads these, duck-typed) ------------------- #

    def _lease_expires_at_iso(self) -> Optional[str]:
        claim = self.claim
        if claim is None or claim.lost:
            return None
        anchor = claim._last_renew_ok or claim._started_at
        if anchor is None:
            return None
        return claimlib._iso(anchor + timedelta(seconds=claim.ttl))

    def health_fields(self) -> Dict[str, object]:
        """The election-owned rows of `GET /v1/health` (SPEC SS5.1):
        `lease_expires_at`, `settle_until`, plus the flags."""
        with self._lock:
            settle_until = self._settle_until_wall
            demoted = self._demoted
        claim = self.claim
        return {
            "lease_expires_at": self._lease_expires_at_iso(),
            "settle_until": settle_until,
            "settling": self.settling(),
            "lease_lost": bool(claim is not None and claim.lost),
            "demoted": demoted,
        }

    def status_fields(self) -> Dict[str, object]:
        """The `server` block of `GET /v1/status`. Everything about THIS
        server process lives here and nowhere else in the status body, so
        the SS3.4 acceptance diff -- `del(.ts, .server)` before a kill vs
        after a re-host -- is empty exactly when the two servers agree
        about the fleet."""
        claim = self.claim
        lease: Optional[Dict[str, object]] = None
        if claim is not None and claim._started_at is not None and not claim.lost:
            lease = {
                "ref": SERVER_LEASE_REF,
                "holder_host": claim.holder_host,
                "started_at": claimlib._iso(claim._started_at),
                "expires_at": self._lease_expires_at_iso(),
                "boot_id": claim.boot_id,
                "advertise_urls": list(claim.advertise_urls),
                "keel_version": claim.keel_version,
            }
        with self._lock:
            settle_until = self._settle_until_wall
            demoted = self._demoted
        return {
            "host": self.host,
            "rank": self.rank,
            "boot_id": self.boot_id,
            "keel_version": self.keel_version,
            "advertise_urls": list(self.advertise_urls),
            "settling": self.settling(),
            "settle_until": settle_until,
            "demoted": demoted,
            "lease": lease,
            "registered_runners": self.registered_runner_ids(),
        }


# --------------------------------------------------------------------- #
# CLI -- `keel server rehost` delegates here (the keel CLI is another
# Stage 2 task; this entrypoint is complete on its own so the drill and
# the runbook need nothing else).
# --------------------------------------------------------------------- #


def _keel_home() -> Path:
    override = os.environ.get("KEEL_HOME")
    if override:
        return Path(override)
    return Path.home() / ".keel"


def _write_port_file(path: Path, host: str, port: int) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(f"{host}:{port}\n")
    tmp.replace(path)  # atomic: a reader never sees a half-written line


def cmd_status(args) -> int:
    """Print the server lease as JSON. Exit 0 = live, 5 = absent,
    6 = expired (distinct from argparse's 2 and rehost's 3/4)."""
    hub = Hub(url=args.state_url, workdir=Path(args.lease_workdir))
    payload = hub.read(SERVER_LEASE_REF)
    body = {
        "ref": SERVER_LEASE_REF,
        "present": payload is not None,
        "expired": bool(payload is not None and is_expired(payload)),
        "payload": payload,
    }
    print(json.dumps(body, indent=2, sort_keys=True))
    if payload is None:
        return 5
    return 6 if body["expired"] else 0


def cmd_rehost(args) -> int:
    """SS3.4 steps 1-6 for the manual path: refuse a live lease (exit 3),
    else take it, build the CachedHub-backed server, serve until a signal
    (exit 0, lease released) or unreachable-demotion (exit 4)."""
    # Imported here, not at module top: candidates and tests that only
    # need the lease machinery must not pay for (or depend on) the
    # transport and the CachedHub.
    import hubstore  # noqa: E402
    import server as keel_server  # noqa: E402

    config = ElectionConfig.from_env()
    events = keel_server.EventLog(Path(args.events_db))
    lease_hub = Hub(url=args.state_url, workdir=Path(args.lease_workdir))
    manager = ElectionManager(
        lease_hub,
        host=args.host,
        rank=args.rank,
        advertise_urls=args.advertise_url or [],
        config=config,
        events=events,
    )

    try:
        manager.rehost()
    except RehostRefusedError as exc:
        print(f"keel-election rehost: {exc}", file=sys.stderr)
        events.close()
        return EXIT_REHOST_REFUSED
    except HubError as exc:
        print(f"keel-election rehost: state repo unreachable: {exc}", file=sys.stderr)
        events.close()
        return EXIT_USAGE

    server = None
    store = None
    try:
        store = hubstore.build_store(
            args.state_url,
            Path(args.state_workdir),
            sweep_interval=args.sweep_interval,
        )
        tokens_path = Path(args.tokens_file)
        if tokens_path.exists():
            tokens = keel_server.TokenStore.from_file(tokens_path)
        else:
            tokens = keel_server.TokenStore.empty()
            logging.warning("keel-election: no tokens file at %s", tokens_path)
        server_config = keel_server.ServerConfig(
            bind_host=args.bind_host, port=args.port, allow_any_bind=args.allow_any_bind
        )
        server = keel_server.build_server(
            server_config,
            store=store,
            tokens=tokens,
            events=events,
            health_provider=store.health,
        )
        # One identity: the lease payload's boot_id IS the server's. The
        # lease was written before the server existed, so the transport
        # adopts the manager's id rather than the other way around.
        server.boot_id = manager.boot_id
        server.attach_election(manager)
        server.start()
        actual_host, actual_port = server.server_address[0], server.server_address[1]
        if args.port_file:
            _write_port_file(Path(args.port_file), actual_host, actual_port)
        manager.start_watch()
        logging.info(
            "keel-election: serving on %s:%s (boot_id=%s, settling until %s)",
            actual_host, actual_port, manager.boot_id, manager._settle_until_wall,
        )

        stop = threading.Event()

        def _on_signal(signum, frame):  # noqa: ARG001
            stop.set()

        signal.signal(signal.SIGTERM, _on_signal)
        signal.signal(signal.SIGINT, _on_signal)
        while not stop.is_set() and not manager.demoted_event.is_set():
            # Two events, one waiter: poll at a human-invisible cadence.
            manager.demoted_event.wait(0.2)
            if stop.is_set() or manager.demoted_event.is_set():
                break
        if manager.demoted_event.is_set():
            return EXIT_UNREACHABLE_DEMOTED
        manager.release()
        return EXIT_OK
    except OSError as exc:
        # Bind/startup failure after we took the lease: give it back so
        # the next candidate is not stuck behind our TTL.
        print(f"keel-election rehost: startup failed: {exc}", file=sys.stderr)
        try:
            manager.release()
        except HubError:
            logging.exception("keel-election: lease release after startup failure failed")
        return EXIT_USAGE
    finally:
        manager.stop_watch()
        if server is not None:
            server.stop()
        if store is not None:
            store.close()
        events.close()


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        prog="keel-election",
        description="Server lease election / settle / demotion / re-host (SPEC SS3.4).",
    )
    parser.add_argument("--log-level", default="INFO")
    sub = parser.add_subparsers(dest="cmd", required=True)

    def add_common(p) -> None:
        p.add_argument(
            "--state-url",
            default=os.environ.get("KEEL_STATE_URL") or os.environ.get("FLEET_HUB_URL"),
            help="state repo remote answering refs/fleet/* (or KEEL_STATE_URL / FLEET_HUB_URL)",
        )
        p.add_argument(
            "--lease-workdir",
            default=str(_keel_home() / "election-cache"),
            help="disposable local object cache for the LEASE hub (its own dir: "
            "the renewer thread and the store index must never share one)",
        )

    p_status = sub.add_parser("status", help="print the server lease; exit 0 live / 5 absent / 6 expired")
    add_common(p_status)
    p_status.set_defaults(fn=cmd_status)

    p_rehost = sub.add_parser(
        "rehost",
        help="refuse on a live lease (exit 3); else take the lease and serve "
        "(exit 0 on signal, 4 on unreachable-demotion)",
    )
    add_common(p_rehost)
    p_rehost.add_argument("--host", default=None, help="holder_host override (default: this hostname)")
    p_rehost.add_argument("--rank", type=int, default=1)
    p_rehost.add_argument("--advertise-url", action="append", default=None)
    p_rehost.add_argument("--bind-host", default=os.environ.get("KEEL_BIND", "127.0.0.1"))
    p_rehost.add_argument("--port", type=int, default=int(os.environ.get("KEEL_PORT", "8470")))
    p_rehost.add_argument("--port-file", default=None, help="write '<host>:<port>' here once bound (port 0 friendly)")
    p_rehost.add_argument("--allow-any-bind", action="store_true", default=False)
    p_rehost.add_argument("--state-workdir", default=str(_keel_home() / "state.git"))
    p_rehost.add_argument("--tokens-file", default=str(_keel_home() / "auth.json"))
    p_rehost.add_argument("--events-db", default=str(_keel_home() / "events.db"))
    p_rehost.add_argument("--sweep-interval", type=float, default=float(os.environ.get("KEEL_SWEEP_INTERVAL", "30")))
    p_rehost.set_defaults(fn=cmd_rehost)

    args = parser.parse_args(argv)
    logging.basicConfig(
        level=getattr(logging, args.log_level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(message)s",
    )
    if not args.state_url:
        print("keel-election: no state repo URL (--state-url / KEEL_STATE_URL / FLEET_HUB_URL)", file=sys.stderr)
        return EXIT_USAGE
    return int(args.fn(args))


if __name__ == "__main__":
    sys.exit(main())
