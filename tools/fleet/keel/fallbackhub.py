#!/usr/bin/env python3
"""`FallbackHub` -- one `Hub` surface over two routes to the same state
repo, with the two rules that make two routes safe (SPEC 4.3).

THE SHAPE. A runner, the CLI, `verdict.py` and `agentrun.py` all talk to
the state spine through the eight-method `fleetlib.Hub` contract
(SPEC 4.1). In Keel there are two implementations of that contract that
reach the same refs: `ServerHub` (HTTP to `keel-server`, which executes
the identical `Hub.create/update/delete` against the state repo and
returns the sha GitHub produced) and `GitHubHub` (`fleetlib.Hub` itself,
pushing at the repo directly). `FallbackHub(primary, github)` presents
both as one: the primary when it answers, the GitHub half when it does
not. Because the server is provably a cache and both routes execute the
same CAS with the same `--force-with-lease` witness, a sha obtained on
either route is valid on the other -- which is what lets a lease renewed
via the server and then directly be ONE lease.

THE TWO RULES, and why each exists:

r1 FRESH CLAIMS -- belongs to the primary, asserted here by test.
   `claim.renew()` (claim.py L644-690) re-reads `hub.sha(ref)`; if it
   differs from the sha it last observed it reads the payload, and if the
   ownership token says the claim is ours it ADOPTS that sha and updates
   against it. A rejected update calls `_mark_lost` unconditionally, and a
   lost worker is killed by process group (fleetd.py L1793-1832). So a
   primary that answers a claim's sha from a stale index -- one the runner
   moved past with a direct renewal while the server was away -- makes
   the next renewal adopt a sha the store no longer carries, the CAS
   rejects it, and a healthy gate dies. The rule is therefore that any
   read under `refs/fleet/claims/` is served LIVE by the primary (one
   `ls-remote` against the store, never the index). This module cannot
   enforce that inside the primary, but `tests/test_fallbackhub.py`
   drives claim.py's real `renew()` through a FallbackHub across a
   route flip and asserts `lost` stays False, with a negative control
   whose primary serves the claim sha from its index and goes red.

r2 NO WRITE RETRY AFTER AN AMBIGUOUS FAILURE -- enforced here.
   A write that failed on the primary is re-issued against GitHub ONLY
   when the primary failed BEFORE the request was sent: connection
   refused, DNS failure, TLS handshake, or the server's `503 not-ready`
   (it answered, and said it did nothing). A timeout after the request
   went out, a connection dropped mid-exchange, or any other 5xx after
   the server may already have executed the CAS is AMBIGUOUS: the write
   may have landed. Re-issuing it would be a second write -- for a
   `create` it comes back False and the caller stands down from work it
   already holds; for a renewal `update` the witness is stale so it
   comes back False and `_mark_lost` kills the gate. So an ambiguous
   failure RAISES `HubUnreachableError` (`AmbiguousWriteError`, a
   subclass) instead: exactly today's behaviour on a network blip, which
   `claim._note_renew_failure` tolerates (L694-715), and the next
   renewal's top-of-loop re-read plus `_owns` adopts the write if it did
   land (L651-663). Reads (`sha/read/read_with_sha/list/fetch_namespace`)
   always fall back -- a read has no side effect to duplicate.

   Classification is deliberately FAIL-CLOSED: anything this module
   cannot positively place in the before-send bucket is ambiguous. A
   primary that raises a bare `HubUnreachableError` with no phase
   information gets no fallback for its writes -- the conservative
   direction is a raised error the lease machinery already absorbs,
   never a silent double write. `classify_primary_failure` documents the
   vocabulary a primary can use to be classified precisely.

r3 STICKINESS. After any primary failure the GitHub route is used for
   every operation for `sticky_s` (30 s) without contacting the primary
   at all -- a write issued direct in that window was never sent to the
   primary, so r2 is not engaged. After the window a write first PROBES
   the primary with a read (`sha(ref)`, or the injected `probe`) and
   routes to it only if the probe answers; reads simply try the primary
   again. A probe that fails re-arms the window. The probe is a read and
   not the write itself because the renew cadence (120 s) is longer than
   the window: with a server that hangs after accepting the request,
   every renewal would otherwise re-probe by writing, hit the ambiguous
   case, raise, and the lease would run out of tolerated failures against
   a route that was never going to answer.

WHAT ROUTES WHERE. Only the coordination surface is two-route. Code reads
and writes (`code_sha`, `code_list`, `push_ref`, `push_code_ref`,
`push_tip_ref`, `delete_code_ref`) go to the GitHub half always -- branch
pushes never go through the server (SPEC 4.2: `ServerHub.push_ref`
raises `NotImplementedError`). `.url`, `.workdir`, `.code_url`,
`.code_push_url` and `.tip_push_url` are the GitHub half's, so the
GIT-CODE borrowers (`workqueue._git`, `dispatch._git`, the train's local
fetch into `hub.workdir`, SPEC 9) work unchanged against a FallbackHub.

Stdlib only, like everything under `tools/fleet/keel/`.
"""

from __future__ import annotations

import errno
import http.client
import socket
import ssl
import sys
import threading
import time
import urllib.error
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Optional, Sequence, Tuple

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # tools/fleet

from fleetlib import HubUnreachableError  # noqa: E402

__all__ = [
    "AMBIGUOUS",
    "BEFORE_SEND",
    "STICKY_S",
    "AmbiguousWriteError",
    "FallbackHub",
    "PrimaryFailure",
    "classify_primary_failure",
]

# The two phases a primary failure can be in, as `classify_primary_failure`
# reports them. Strings rather than an enum so they read unchanged in a
# heartbeat, a log line and a test assertion.
BEFORE_SEND = "before-send"
AMBIGUOUS = "ambiguous"

# How long the GitHub route stays in use after a primary failure before the
# primary is tried again (SPEC 4.3 rule 3).
STICKY_S = 30.0

# Claim refs are the ones whose freshness r1 is about. Spelled here rather
# than imported from claim.py so this module depends on fleetlib alone.
CLAIMS_PREFIX = "refs/fleet/claims/"

# What "the primary failed" looks like. `HubUnreachableError` is the
# contract (SPEC 4.2: every transport failure raises it); the other two
# are what leaks from a primary that forgot to wrap -- `urllib.error.
# URLError` is an `OSError`, `http.client.RemoteDisconnected` is an
# `HTTPException`. Catching them keeps a sloppy primary from taking a
# runner down with a traceback instead of a fallback. A plain `HubError`
# is NOT in this tuple: that is a content problem ("payload.json is not
# valid JSON"), a fact about the store rather than the route, and
# switching routes would not change it.
_PRIMARY_FAILURE_TYPES = (HubUnreachableError, OSError, http.client.HTTPException)

# errno values that mean the connection was never established, so no
# request can have been sent.
_BEFORE_SEND_ERRNOS = frozenset(
    e for e in (
        getattr(errno, "ECONNREFUSED", None),
        getattr(errno, "EHOSTUNREACH", None),
        getattr(errno, "ENETUNREACH", None),
        getattr(errno, "EHOSTDOWN", None),
        getattr(errno, "ENETDOWN", None),
        getattr(errno, "EADDRNOTAVAIL", None),
    )
    if e is not None
)


class PrimaryFailure(HubUnreachableError):
    """A `HubUnreachableError` that states which phase it failed in.

    The vocabulary a primary (`ServerHub`) is expected to use:
    `raise PrimaryFailure("...", request_sent=False)` for a failure it
    KNOWS happened before the request left (connect refused, DNS, TLS,
    `503 not-ready`), `request_sent=True` for anything after. A primary
    that cannot say leaves it `None` and the exception's cause chain is
    consulted instead (`classify_primary_failure`); if that does not
    settle it either, the failure is ambiguous.

    `status` carries the HTTP status when the server answered at all, so
    a `503` can be recognised without a `urllib.error.HTTPError` in the
    chain.
    """

    def __init__(
        self,
        message: str,
        *,
        request_sent: Optional[bool] = None,
        status: Optional[int] = None,
    ):
        super().__init__(message)
        self.request_sent = request_sent
        self.status = status


class AmbiguousWriteError(HubUnreachableError):
    """A write the primary may or may not have executed (r2).

    Raised in place of a second write. `op` is `create`/`update`/`delete`,
    `ref` the target, `cause` the primary's original failure. It is a
    `HubUnreachableError` so every existing consumer treats it as the
    blip it is: `claim._note_renew_failure` tolerates it, `verdict.store`'s
    CLI reports "hub unreachable, verdict not cached", `fleetd` counts it
    as a degraded step.
    """

    def __init__(self, op: str, ref: str, cause: BaseException):
        super().__init__(
            f"{op} of {ref} failed on the primary after the request may have been "
            f"sent; the write is AMBIGUOUS and is NOT re-issued against GitHub "
            f"(SPEC 4.3 r2): {type(cause).__name__}: {cause}"
        )
        self.op = op
        self.ref = ref
        self.cause = cause


def _exception_chain(exc: BaseException, limit: int = 8):
    """`exc`, then its `__cause__`/`__context__` chain, cycle-safe and bounded."""
    seen = set()
    cur: Optional[BaseException] = exc
    while cur is not None and len(seen) < limit and id(cur) not in seen:
        seen.add(id(cur))
        yield cur
        cur = cur.__cause__ if cur.__cause__ is not None else cur.__context__


def _http_status(exc: BaseException) -> Optional[int]:
    """An HTTP status carried by `exc`, if it has one (`urllib.error.
    HTTPError.code`, or a `status`/`code` int attribute on anything)."""
    for attr in ("status", "code"):
        value = getattr(exc, attr, None)
        if isinstance(value, bool):
            continue
        if isinstance(value, int):
            return value
    return None


def classify_primary_failure(exc: BaseException) -> str:
    """`BEFORE_SEND` if the primary can be shown to have failed before the
    request was sent; `AMBIGUOUS` otherwise.

    The order is the rule. An explicit statement by the primary
    (`request_sent` on any exception in the chain) wins. Failing that,
    the chain is searched for a POSITIVE before-send signature:

      * `ConnectionRefusedError`, or an `OSError` whose errno says the
        connection was never established (`ECONNREFUSED`, `EHOSTUNREACH`,
        `ENETUNREACH`, ...);
      * `socket.gaierror`/`socket.herror` -- DNS;
      * `ssl.SSLCertVerificationError`, or an `ssl.SSLError` whose reason
        names the handshake -- TLS;
      * an HTTP `503` -- the server answered "not ready" and executed
        nothing (SPEC 4.3 r2 names it; SPEC 7 "Server <-> GitHub
        partition": "server 503s writes").

    Everything else -- `socket.timeout`/`TimeoutError` (urllib cannot tell
    a connect timeout from a read timeout, so both are treated as after
    send), `RemoteDisconnected`, `ConnectionResetError`, `BrokenPipeError`,
    `IncompleteRead`, `BadStatusLine`, a `500`/`502`/`504`, and a bare
    `HubUnreachableError` with no information at all -- is `AMBIGUOUS`.
    Fail-closed: the cost of calling a before-send failure ambiguous is
    one tolerated blip; the cost of the reverse is a double write.
    """
    chain = list(_exception_chain(exc))

    for e in chain:
        stated = getattr(e, "request_sent", None)
        if isinstance(stated, bool):
            return AMBIGUOUS if stated else BEFORE_SEND

    for e in chain:
        # urllib wraps the socket-level failure in URLError(reason=<exc>);
        # the reason is the thing to classify.
        reason = getattr(e, "reason", None) if isinstance(e, urllib.error.URLError) else None
        candidates = [e] + ([reason] if isinstance(reason, BaseException) else [])
        for c in candidates:
            if isinstance(c, (socket.gaierror, socket.herror)):
                return BEFORE_SEND
            if isinstance(c, ConnectionRefusedError):
                return BEFORE_SEND
            if isinstance(c, ssl.SSLCertVerificationError):
                return BEFORE_SEND
            if isinstance(c, ssl.SSLError):
                text = f"{getattr(c, 'reason', '')} {c}".upper()
                if "HANDSHAKE" in text or "CERTIFICATE" in text:
                    return BEFORE_SEND
                continue
            if isinstance(c, OSError) and not isinstance(c, urllib.error.URLError):
                if getattr(c, "errno", None) in _BEFORE_SEND_ERRNOS:
                    return BEFORE_SEND
            if _http_status(c) == 503:
                return BEFORE_SEND

    return AMBIGUOUS


class FallbackHub:
    """The `Hub` surface over `primary` (a `ServerHub`, or anything with
    the same method surface) and `github` (a `fleetlib.Hub` on the state
    repo), applying SPEC 4.3.

    `sticky_s`, `clock` and `probe` exist for tests: `clock` is a
    monotonic-seconds callable (default `time.monotonic`), `probe` a
    zero-argument callable used to re-probe the primary after the sticky
    window (default: `primary.sha(<the ref being written>)`).
    """

    def __init__(
        self,
        primary: Any,
        github: Any,
        *,
        sticky_s: float = STICKY_S,
        clock: Callable[[], float] = time.monotonic,
        probe: Optional[Callable[[], Any]] = None,
    ):
        if primary is None or github is None:
            raise ValueError("FallbackHub needs both a primary and a github half")
        self.primary = primary
        self.github = github
        self.sticky_s = float(sticky_s)
        self._clock = clock
        self._probe_fn = probe
        self._lock = threading.Lock()
        self._degraded_since: Optional[datetime] = None
        self._sticky_until: Optional[float] = None
        self._last_primary_error: Optional[BaseException] = None
        self._primary_failures = 0
        self._fallback_reads = 0
        self._fallback_writes = 0
        self._ambiguous_writes = 0

    # ---------------------------------------------------------------- #
    # Identity: the GitHub half's, so GIT-CODE borrowers work unchanged
    # ---------------------------------------------------------------- #

    @property
    def fallback(self) -> Any:
        """SPEC 4.3 names the halves `primary`/`fallback`; `github` is the
        same object under the name the task uses."""
        return self.github

    @property
    def url(self) -> str:
        return self.github.url

    @property
    def workdir(self) -> Path:
        return self.github.workdir

    @property
    def code_url(self) -> str:
        return self.github.code_url

    @property
    def code_push_url(self) -> str:
        return self.github.code_push_url

    @property
    def tip_push_url(self) -> str:
        return self.github.tip_push_url

    # ---------------------------------------------------------------- #
    # Degradation state (reported in the runner heartbeat)
    # ---------------------------------------------------------------- #

    @property
    def degraded(self) -> bool:
        with self._lock:
            return self._degraded_since is not None

    @property
    def degraded_since(self) -> Optional[datetime]:
        """UTC time of the first primary failure of the current degraded
        stretch, or None while the primary is in use. Stays put across
        re-armed sticky windows; cleared the moment the primary answers."""
        with self._lock:
            return self._degraded_since

    @property
    def last_primary_error(self) -> Optional[BaseException]:
        with self._lock:
            return self._last_primary_error

    def status(self) -> dict:
        """A heartbeat-shaped summary: route, `degraded_since` (ISO or
        None), counters."""
        with self._lock:
            since = self._degraded_since
            return {
                "route": "github" if since is not None else "primary",
                "degraded_since": since.isoformat() if since is not None else None,
                "primary_failures": self._primary_failures,
                "fallback_reads": self._fallback_reads,
                "fallback_writes": self._fallback_writes,
                "ambiguous_writes": self._ambiguous_writes,
                "last_primary_error": (
                    f"{type(self._last_primary_error).__name__}: {self._last_primary_error}"
                    if self._last_primary_error is not None else None
                ),
            }

    # ---------------------------------------------------------------- #
    # The coordination surface: reads fall back always
    # ---------------------------------------------------------------- #

    def sha(self, ref: str) -> Optional[str]:
        return self._read("sha", lambda: self.primary.sha(ref), lambda: self.github.sha(ref))

    def read(self, ref: str) -> Optional[dict]:
        return self._read("read", lambda: self.primary.read(ref), lambda: self.github.read(ref))

    def read_with_sha(self, ref: str) -> Tuple[Optional[str], Optional[dict]]:
        return self._read(
            "read_with_sha",
            lambda: self.primary.read_with_sha(ref),
            lambda: self.github.read_with_sha(ref),
        )

    def list(self, prefix: str) -> dict:
        return self._read("list", lambda: self.primary.list(prefix), lambda: self.github.list(prefix))

    def fetch_namespace(self, prefix: str) -> dict:
        primary_fetch = getattr(self.primary, "fetch_namespace", None)
        if not callable(primary_fetch):
            # A primary without the bulk read is not a failed primary.
            return self.github.fetch_namespace(prefix)
        return self._read(
            "fetch_namespace",
            lambda: primary_fetch(prefix),
            lambda: self.github.fetch_namespace(prefix),
        )

    # ---------------------------------------------------------------- #
    # The coordination surface: writes obey r2
    # ---------------------------------------------------------------- #

    def create(self, ref: str, payload: dict, push_options: Optional[Sequence[str]] = None) -> bool:
        kw = self._write_kwargs(push_options)
        return self._write(
            "create", ref,
            lambda: self.primary.create(ref, payload, **kw),
            lambda: self.github.create(ref, payload, **kw),
        )

    def update(
        self, ref: str, payload: dict, expect_sha: str, push_options: Optional[Sequence[str]] = None,
    ) -> bool:
        kw = self._write_kwargs(push_options)
        return self._write(
            "update", ref,
            lambda: self.primary.update(ref, payload, expect_sha, **kw),
            lambda: self.github.update(ref, payload, expect_sha, **kw),
        )

    def delete(self, ref: str, expect_sha: str, push_options: Optional[Sequence[str]] = None) -> bool:
        kw = self._write_kwargs(push_options)
        return self._write(
            "delete", ref,
            lambda: self.primary.delete(ref, expect_sha, **kw),
            lambda: self.github.delete(ref, expect_sha, **kw),
        )

    @staticmethod
    def _write_kwargs(push_options: Optional[Sequence[str]]) -> dict:
        # Forwarded only when given, so a primary whose signature has no
        # `push_options` (they are a hub-era hook option with no meaning
        # on GitHub) still works for every caller that does not pass one.
        return {} if not push_options else {"push_options": push_options}

    # ---------------------------------------------------------------- #
    # Code reads and writes: GitHub half only, never the server
    # ---------------------------------------------------------------- #

    def code_sha(self, ref: str) -> Optional[str]:
        return self.github.code_sha(ref)

    def code_list(self, prefix: str) -> dict:
        return self.github.code_list(prefix)

    def push_ref(self, refspec: str, push_options: Optional[Sequence[str]] = None, force: bool = False):
        return self.github.push_ref(refspec, push_options=push_options, force=force)

    def push_code_ref(self, refspec: str, *args, **kwargs):
        return self.github.push_code_ref(refspec, *args, **kwargs)

    def push_tip_ref(self, refspec: str, *args, **kwargs):
        return self.github.push_tip_ref(refspec, *args, **kwargs)

    def delete_code_ref(self, ref: str, expect_sha: str) -> bool:
        return self.github.delete_code_ref(ref, expect_sha)

    # ---------------------------------------------------------------- #
    # Routing
    # ---------------------------------------------------------------- #

    def _read(self, op: str, via_primary: Callable[[], Any], via_github: Callable[[], Any]) -> Any:
        if self._primary_worth_trying():
            try:
                result = via_primary()
            except _PRIMARY_FAILURE_TYPES as exc:
                self._note_primary_failure(exc)
            else:
                self._note_primary_ok()
                return result
        with self._lock:
            self._fallback_reads += 1
        return via_github()

    def _write(self, op: str, ref: str, via_primary: Callable[[], Any], via_github: Callable[[], Any]) -> Any:
        if not self._primary_usable_for_write(ref):
            # Never sent to the primary: a direct write, not a retry.
            with self._lock:
                self._fallback_writes += 1
            return via_github()
        try:
            result = via_primary()
        except _PRIMARY_FAILURE_TYPES as exc:
            self._note_primary_failure(exc)
            if classify_primary_failure(exc) == BEFORE_SEND:
                with self._lock:
                    self._fallback_writes += 1
                return via_github()
            with self._lock:
                self._ambiguous_writes += 1
            raise AmbiguousWriteError(op, ref, exc) from exc
        self._note_primary_ok()
        return result

    def _primary_worth_trying(self) -> bool:
        """Reads: the primary unless inside a sticky window."""
        with self._lock:
            if self._degraded_since is None:
                return True
            return self._clock() >= (self._sticky_until or 0.0)

    def _primary_usable_for_write(self, ref: str) -> bool:
        """Writes: the primary when healthy; after the sticky window only
        once a READ probe answers (module docstring, r3)."""
        with self._lock:
            if self._degraded_since is None:
                return True
            if self._clock() < (self._sticky_until or 0.0):
                return False
        probe = self._probe_fn if self._probe_fn is not None else (lambda: self.primary.sha(ref))
        try:
            probe()
        except _PRIMARY_FAILURE_TYPES as exc:
            self._note_primary_failure(exc)
            return False
        self._note_primary_ok()
        return True

    def _note_primary_failure(self, exc: BaseException) -> None:
        with self._lock:
            if self._degraded_since is None:
                self._degraded_since = datetime.now(timezone.utc)
            self._sticky_until = self._clock() + self.sticky_s
            self._last_primary_error = exc
            self._primary_failures += 1

    def _note_primary_ok(self) -> None:
        with self._lock:
            self._degraded_since = None
            self._sticky_until = None
