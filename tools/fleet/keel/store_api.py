#!/usr/bin/env python3
"""The contract `keel/server.py`'s HTTP routes are written against.

`server.py` owns transport only: HTTP routing, bearer auth, chunked SSE, the
`/v1/wait` long-poll, the connection cap, and the listener watchdog
(docs/AGENT-SERVER-SPEC.md SS2 C6). It has NO opinion on how a ref's sha and
payload get computed, cached, or written through to the state repo -- that
is the `CachedHub`'s job (SS3.2, a separate file/task: `keel/cachedhub.py`).
This module exists so the two can be built, reviewed, and tested
independently: `server.py` imports nothing from `cachedhub.py`, only the
shapes declared here, and `cachedhub.py`'s `CachedHub` is written to satisfy
this `Store` protocol structurally (no inheritance required -- anything with
the right eight methods IS a `Store`).

Method semantics mirror `fleetlib.Hub` (SS4.1/SS4.2 of the spec) so a sha
obtained through either the direct GitHub hub or an HTTP round trip through
a `Store` is interchangeable:

  * `True`/a sha string on success; `False` means the CAS lost a race --
    NEVER an exception. `create`/`update` return the new sha on success
    (the server needs it to answer the HTTP call with the sha GitHub
    produced, SS4.2: "a sha obtained via either route is valid on the
    other"); `delete` returns a bare bool.
  * `None` means the ref is absent (`sha`/`read`/`read_with_sha` only).
  * a raised `StoreUnreachableError` means the TRANSPORT itself failed.
    `server.py` maps this, and only this, to HTTP 503. It must never be
    conflated with a CAS conflict (`False`) or an absent ref (`None`) --
    that conflation is exactly the "stale index -> false `lost` -> healthy
    gate killed" defect SS4.3 exists to close.

`fresh`, where a method accepts it, means "do not answer from any local
cache/index -- read live." `server.py` sets it from the caller's `?fresh=1`
query parameter for every `/v1/refs` read it forwards, and unconditionally
for `/v1/wait` (a long-poll that answered from a stale cache could never
observe the change it exists to detect). SS4.3 rule 1 additionally requires
every read under `refs/fleet/claims/` to be fresh regardless of the query
parameter -- that is a `CachedHub`-side POLICY (it is the one place that
knows a ref is a claim), not something `server.py` enforces itself; `fresh`
here is only the plumbing that lets a caller ask for it.

Standard library only.
"""

from __future__ import annotations

import threading
import time
from dataclasses import dataclass
from typing import Dict, Optional, Protocol, Tuple, Union, runtime_checkable

Payload = Dict[str, object]


class StoreUnreachableError(Exception):
    """The store's transport failed (network, disk, subprocess, whatever it
    is backed by) -- distinct from a CAS conflict (`False`) and an absent
    ref (`None`). `server.py`'s dispatcher maps this, and only this,
    exception to HTTP 503; anything else that escapes a handler is a bug
    and becomes a 500.
    """


@dataclass(frozen=True)
class RefListing:
    """One row of a `GET /v1/refs?prefix=` answer (SPEC SS5.1:
    `{ref:{sha,observed_at}}`). `observed_at` is a `time.time()`-style
    float -- when the entry now being served was last confirmed live: a
    `CachedHub` index hit reports when the sweep or write-through observed
    it, and any implementation answering straight from a live read (never
    caching, as `InMemoryStore` below does) is free to report "now".
    """

    sha: str
    observed_at: float


@runtime_checkable
class Store(Protocol):
    """Eight methods. `server.py`'s routes call exactly these and nothing
    else on whatever object is handed to `KeelHTTPServer(store=...)` --
    that object can be an `InMemoryStore` (tests, standalone smoke runs) or
    a `CachedHub` (production) interchangeably.
    """

    def sha(self, ref: str, *, fresh: bool = False) -> Optional[str]:
        """The current sha of `ref`, or `None` if it does not exist. The
        cheap existence probe and the CAS witness callers read back as
        `expect_sha` -- see `fleetlib.Hub.sha`'s docstring for why this is
        NOT the way to get a sha to read a payload at."""

    def read(self, ref: str, *, fresh: bool = False) -> Optional[Payload]:
        """The payload `ref` points at, or `None` if absent."""

    def read_with_sha(self, ref: str, *, fresh: bool = False) -> Tuple[Optional[str], Optional[Payload]]:
        """`(sha, payload)` for `ref`, COHERENT with each other (the sha
        returned is the one the payload belongs to), or `(None, None)` if
        absent. See `fleetlib.Hub.read_with_sha` for why `sha()` then
        `read()` as two separate calls is the wrong shape."""

    def list(self, prefix: str) -> Dict[str, RefListing]:
        """`{ref: RefListing}` for every ref matching `prefix`, for
        `GET /v1/refs?prefix=`. Lightweight by design -- shas and
        observed-at only, never payloads (SPEC SS5.1)."""

    def create(self, ref: str, payload: Payload) -> Union[str, bool]:
        """Atomically create `ref` iff it does not exist yet. Returns the
        new sha on success, `False` if the ref already exists (lost the
        race) -- never raises for that; only a transport failure raises."""

    def update(self, ref: str, payload: Payload, expect_sha: str) -> Union[str, bool]:
        """Atomically replace `ref`'s payload iff it still points at
        `expect_sha`. Returns the new sha on success, `False` if `ref` had
        moved (lost the race)."""

    def delete(self, ref: str, expect_sha: str) -> bool:
        """Atomically delete `ref` iff it still points at `expect_sha`.
        `True` = deleted, `False` = lost the race (moved or already
        gone)."""

    def fetch_namespace(self, prefix: str) -> Dict[str, str]:
        """`{ref: sha}` for the ref AT `prefix` and every ref UNDER it, in
        one round trip -- see `fleetlib.Hub.fetch_namespace` (already
        implemented, Stage 1) for the leaf-vs-directory reasoning this
        signature exists to satisfy. `server.py` never calls this itself
        today (no route exposes a raw namespace fetch); it is here because
        a `CachedHub` needs it to satisfy `Store` structurally with the
        exact method `fleetlib.Hub` already provides, and because a future
        route (a bulk index rebuild trigger) will want it without a second
        contract to agree on."""


# --------------------------------------------------------------------- #
# Reference implementation -- tests and standalone runs ONLY.
# --------------------------------------------------------------------- #


class InMemoryStore:
    """A `Store` that lives entirely in one process's memory: no git, no
    index, no write-through, no persistence across a restart.

    This exists for `test_server_transport.py` (which tests `server.py`'s
    TRANSPORT -- routing, auth, SSE, long-poll, the connection cap, the
    watchdog -- and must not need a real git remote or a finished
    `CachedHub` to do it) and for running `server.py` standalone as a smoke
    test before `cachedhub.py` lands. It is deliberately NOT a fake
    `CachedHub`: it has none of SPEC SS3.2/SS4.3's guarantees (the
    monotonic-sweep rule, the fresh-claims rule, write-through to a real
    spine) because it has no sweep, no index, and no spine to write through
    to -- every call is already "fresh" because there is nothing else to be
    stale against. A test that wants to pin index-staleness behaviour
    belongs against the real `CachedHub`, not here.

    Thread-safe (one lock around the whole dict) -- adequate for a test
    double serving a handful of concurrent connections, not a production
    concurrency target.
    """

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._refs: Dict[str, Tuple[str, Payload]] = {}
        self._counter = 0

    def _next_sha(self) -> str:
        # Not a real git sha -- a monotonic, unique, sha-shaped (40 hex-ish
        # chars) token, so tests can tell two writes apart without
        # shelling out to git. The counter goes right after the "mem"
        # prefix (not padded out to the far end) so it lands inside the
        # 40-char result instead of being sliced off by it.
        self._counter += 1
        return f"mem{self._counter:037d}"

    def sha(self, ref: str, *, fresh: bool = False) -> Optional[str]:
        del fresh
        with self._lock:
            entry = self._refs.get(ref)
            return entry[0] if entry else None

    def read(self, ref: str, *, fresh: bool = False) -> Optional[Payload]:
        del fresh
        with self._lock:
            entry = self._refs.get(ref)
            return dict(entry[1]) if entry else None

    def read_with_sha(self, ref: str, *, fresh: bool = False) -> Tuple[Optional[str], Optional[Payload]]:
        del fresh
        with self._lock:
            entry = self._refs.get(ref)
            if entry is None:
                return (None, None)
            return (entry[0], dict(entry[1]))

    def list(self, prefix: str) -> Dict[str, RefListing]:
        now = time.time()
        norm = prefix if prefix.endswith("/") else prefix + "/"
        with self._lock:
            out = {}
            for ref, (sha, _payload) in self._refs.items():
                if ref == prefix or ref.startswith(norm):
                    out[ref] = RefListing(sha=sha, observed_at=now)
            return out

    def create(self, ref: str, payload: Payload) -> Union[str, bool]:
        with self._lock:
            if ref in self._refs:
                return False
            new_sha = self._next_sha()
            self._refs[ref] = (new_sha, dict(payload))
            return new_sha

    def update(self, ref: str, payload: Payload, expect_sha: str) -> Union[str, bool]:
        with self._lock:
            entry = self._refs.get(ref)
            current_sha = entry[0] if entry else None
            if current_sha != expect_sha:
                return False
            new_sha = self._next_sha()
            self._refs[ref] = (new_sha, dict(payload))
            return new_sha

    def delete(self, ref: str, expect_sha: str) -> bool:
        with self._lock:
            entry = self._refs.get(ref)
            current_sha = entry[0] if entry else None
            if current_sha != expect_sha:
                return False
            del self._refs[ref]
            return True

    def fetch_namespace(self, prefix: str) -> Dict[str, str]:
        norm = prefix if prefix.endswith("/") else prefix + "/"
        with self._lock:
            return {
                ref: sha
                for ref, (sha, _payload) in self._refs.items()
                if ref == prefix or ref.startswith(norm)
            }
