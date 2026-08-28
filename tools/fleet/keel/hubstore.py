#!/usr/bin/env python3
"""`CachedHubStore` -- the ONE adapter between `keel/server.py`'s `Store`
contract (`keel/store_api.py`) and `keel/cachedhub.py`'s `CachedHub`.

The two were built in parallel against the same spec and their surfaces
differ in four places. Neither is widened here; this file is where the
differences are reconciled, and nowhere else:

1. `fresh`. `Store` reads take `fresh=` (set from `?fresh=1`, and
   unconditionally by `/v1/wait`). `CachedHub` has no such flag: it
   decides freshness by ref (`refs/fleet/claims/*` is ALWAYS live, SPEC
   §4.3 rule 1 -- that policy stays inside `CachedHub`, untouched). A
   `fresh=True` read therefore goes to the wrapped `fleetlib.Hub` directly
   (`CachedHub.hub`), which is the store. The index is not updated by such
   a read; that leaves it at worst BEHIND the store, which is what a cache
   is allowed to be.

2. `list()` shape. `Store.list(prefix)` returns `{ref: RefListing(sha,
   observed_at)}` for the ref AT the prefix and every ref UNDER it;
   `CachedHub.list` is `Hub.list`'s `{ref: sha}` strictly under the prefix.
   The adapter asks `CachedHub.fetch_namespace` (AT + UNDER, index-served
   inside the namespace) and reads `observed_at` off the public index
   entry when it holds that very sha, else reports "now" (the value was
   just read live).

3. The landed sha. `Store.create/update` return the new sha (SPEC §4.2:
   the server answers a write with the sha GitHub produced, so a sha
   obtained via either route is valid on the other); `CachedHub` returns
   `Hub`'s bare bool and fills its index only from a coherent readback of
   the store AFTER the CAS. The adapter takes the sha from that readback's
   index entry -- the store's own answer to a read that started after our
   CAS, never the payload we sent -- and when the readback failed (entry
   marked stale) it asks the store again, live.

4. Errors. A READ that fails in transport maps `HubUnreachableError` to
   `StoreUnreachableError`, which `server.py` answers with 503; reads
   always fall back (SPEC §4.3 rule 2), so that is safe. A WRITE that
   fails in transport must NOT: `FallbackHub` treats a 503 as "the server
   answered and did nothing" and re-issues the write against GitHub, and a
   `git push` that raised may very well have been executed by GitHub
   before the answer was lost. Re-issuing an `update` whose witness has
   since moved comes back `False`, `claim.renew` calls `_mark_lost`, and a
   healthy gate is killed -- the exact r2 violation. So a write-path
   failure raises `WriteOutcomeUnknownError`, which is deliberately NOT a
   `StoreUnreachableError`; `server.py` answers it 500, which the client
   classifies as ambiguous and RAISES (today's blip behaviour that
   `claim._note_renew_failure` tolerates and the next renewal's `_owns`
   re-read adopts). The same exception covers a CAS that landed but whose
   sha could not be read back: the write is real, the answer is unknown,
   and the only honest reply is "unknown".

Standard library only (everything under `tools/fleet/keel/` is).
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Callable, Dict, Optional, Tuple, Union

_KEEL_DIR = Path(__file__).resolve().parent
_FLEET_DIR = _KEEL_DIR.parent
for _p in (_FLEET_DIR, _KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import store_api  # noqa: E402  -- the SAME module object server.py imports, so its except-clause matches
from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402
from keel.cachedhub import DEFAULT_NAMESPACE, CachedHub  # noqa: E402
from store_api import Payload, RefListing, StoreUnreachableError  # noqa: E402

DEFAULT_SWEEP_INTERVAL_S = 30.0


class WriteOutcomeUnknownError(Exception):
    """A write-through raised in transport (sent, maybe executed) or
    landed but its sha could not be read back. NOT a
    `StoreUnreachableError` on purpose -- see point 4 in the module
    docstring: a 503 would invite the client to re-issue the write.

    `landed` is True when the CAS is known to have succeeded and only the
    readback failed; False when the CAS itself raised (outcome unknown).
    """

    def __init__(self, op: str, ref: str, cause: Optional[BaseException], landed: bool = False):
        self.op = op
        self.ref = ref
        self.cause = cause
        self.landed = landed
        state = "landed but its sha could not be read back" if landed else "raised in transport; outcome unknown"
        detail = f": {type(cause).__name__}: {cause}" if cause is not None else ""
        super().__init__(f"{op} {ref} {state}{detail}")


class CachedHubStore:
    """`store_api.Store` over a `CachedHub`. Holds nothing of its own:
    every read and write goes to the `CachedHub`, which holds the index
    and applies the three rules that keep it a cache."""

    def __init__(self, cached: CachedHub):
        self._cached = cached

    @property
    def cached(self) -> CachedHub:
        return self._cached

    @property
    def hub(self) -> Hub:
        """The wrapped store client (the GitHub half)."""
        return self._cached.hub

    # -- reads (transport failure -> StoreUnreachableError -> 503) ---------

    def sha(self, ref: str, *, fresh: bool = False) -> Optional[str]:
        return self._read(lambda: self._cached.hub.sha(ref) if fresh else self._cached.sha(ref))

    def read(self, ref: str, *, fresh: bool = False) -> Optional[Payload]:
        return self.read_with_sha(ref, fresh=fresh)[1]

    def read_with_sha(self, ref: str, *, fresh: bool = False) -> Tuple[Optional[str], Optional[Payload]]:
        return self._read(lambda: self._cached.hub.read_with_sha(ref) if fresh else self._cached.read_with_sha(ref))

    def fetch_namespace(self, prefix: str) -> Dict[str, str]:
        return self._read(lambda: dict(self._cached.fetch_namespace(prefix)))

    def list(self, prefix: str) -> Dict[str, RefListing]:
        shas = self.fetch_namespace(prefix)
        now = time.time()
        out: Dict[str, RefListing] = {}
        for ref, sha in shas.items():
            entry = self._cached.index.get(ref)
            observed = entry.observed_at if entry is not None and entry.sha == sha else now
            out[ref] = RefListing(sha=sha, observed_at=observed)
        return out

    @staticmethod
    def _read(fn: Callable[[], object]):
        try:
            return fn()
        except HubUnreachableError as exc:
            raise StoreUnreachableError(str(exc)) from exc

    # -- writes (transport failure -> WriteOutcomeUnknownError, never 503) --

    def create(self, ref: str, payload: Payload) -> Union[str, bool]:
        return self._write("create", ref, lambda: self._cached.create(ref, payload))

    def update(self, ref: str, payload: Payload, expect_sha: str) -> Union[str, bool]:
        return self._write("update", ref, lambda: self._cached.update(ref, payload, expect_sha))

    def delete(self, ref: str, expect_sha: str) -> bool:
        try:
            return bool(self._cached.delete(ref, expect_sha))
        except HubError as exc:
            raise WriteOutcomeUnknownError("delete", ref, exc) from exc

    def _write(self, op: str, ref: str, cas: Callable[[], bool]) -> Union[str, bool]:
        before = self._cached.index.get(ref)
        try:
            ok = cas()
        except HubError as exc:
            raise WriteOutcomeUnknownError(op, ref, exc) from exc
        if not ok:
            return False
        return self._landed(op, ref, before)

    def _landed(self, op: str, ref: str, before) -> str:
        """The sha the store holds now that our CAS has succeeded.

        `CachedHub._write` read the ref back from the store after the CAS
        and recorded the result in the index (or marked the entry stale
        when that readback failed). An entry that is not the one we saw
        before the CAS, is not stale and is not a tombstone is therefore a
        store observation that STARTED after our write landed -- the sha
        GitHub produced, or a newer one if another writer has already
        moved the ref, which is still what the store reports. Anything
        else (readback failed, ref outside the indexed namespace) is asked
        of the store live; through `CachedHub.sha` a claim is live by
        rule anyway and a stale entry is never served.
        """
        after = self._cached.index.get(ref)
        if after is not None and after is not before and not after.stale and after.sha is not None:
            return after.sha
        try:
            sha = self._cached.sha(ref)
        except HubError as exc:
            raise WriteOutcomeUnknownError(op, ref, exc, landed=True) from exc
        if sha is None:
            raise WriteOutcomeUnknownError(op, ref, None, landed=True)
        return sha

    # -- lifecycle / health ------------------------------------------------

    def health(self) -> Dict[str, object]:
        """Extra fields for `GET /v1/health` (SPEC §5.1): `index_observed_at`
        (wall clock of the newest index entry) and `github_ok` (the last
        sweep succeeded)."""
        cached = self._cached
        entries = cached.index.snapshot()
        newest = max((e.observed_at for e in entries.values()), default=None)
        report = cached.last_sweep
        return {
            "index_observed_at": newest,
            "index_refs": len(entries),
            "github_ok": cached.sweeps > 0 and cached.last_sweep_error is None,
            "sweeps": cached.sweeps,
            "sweep_failures": cached.sweep_failures,
            "last_sweep_error": cached.last_sweep_error,
            "last_sweep_listed": report.listed if report is not None else None,
            "sweeper_running": cached.sweeper_running(),
            "state_url": cached.url,
        }

    def close(self) -> None:
        self._cached.stop_sweeper()


def build_store(
    url: str,
    workdir: Path,
    *,
    code_url: Optional[str] = None,
    sweep_interval: float = DEFAULT_SWEEP_INTERVAL_S,
    namespace: str = DEFAULT_NAMESPACE,
) -> CachedHubStore:
    """`fleetlib.Hub(url, workdir)` -> `CachedHub` (first sweep in the
    constructor; raises `HubUnreachableError` if the state repo cannot be
    listed -- an unreachable spine is not an empty index) -> adapter, with
    the periodic sweeper started when `sweep_interval > 0`. `close()` stops
    it."""
    hub = Hub(url=url, workdir=Path(workdir), code_url=code_url)
    cached = CachedHub(hub, namespace=namespace)
    if sweep_interval > 0:
        cached.start_sweeper(sweep_interval)
    return CachedHubStore(cached)
