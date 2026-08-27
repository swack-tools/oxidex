#!/usr/bin/env python3
"""Tests for `tools/fleet/keel/serverhub.py` -- the client side of
keel-server's `/v1/refs` CAS façade (SPEC §2 C4, §4.2, §4.3; PLAN Stage 2
task 3) -- and for the two properties of `server.py`'s refs handlers that
belong to the same task: the `POST /v1/refs/{ref}` create route and the
server-side half of r1 (any read under `refs/fleet/claims/` is served
live, whatever the query string says).

Every test runs against a real `KeelHTTPServer` bound to `127.0.0.1:0`
(an OS-assigned ephemeral port; fixture servers bind loopback ONLY):

  * the 409 -> `False`, 404 -> `None`, 503 -> before-send mapping, with
    `keel.fallbackhub.classify_primary_failure` as the arbiter of what
    each raise means to a `FallbackHub` (SPEC §4.2/§4.3 r2);
  * a `ProcessPool` of 8 racers creating the same ref THROUGH the server:
    exactly one winner (the acceptance instrument for this task);
  * a store double with a corruptible index: a corrupted index entry for
    a claim still yields the true sha via the server (r1), with a
    negative control proving the double really would have served the
    stale sha for any other ref.

Run with:
    python3 -m unittest tools.fleet.tests.test_serverhub -v
or, from this directory:
    python3 -m unittest test_serverhub -v
"""

from __future__ import annotations

import concurrent.futures
import hashlib
import http.client
import json
import logging
import multiprocessing
import sys
import threading
import time
import unittest
import uuid
from pathlib import Path
from typing import Dict, List, Optional, Tuple, Union

FLEET_DIR = Path(__file__).resolve().parents[1]
KEEL_DIR = FLEET_DIR / "keel"
for _p in (FLEET_DIR, KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

# `store_api` must be the SAME module object `server.py` imports (both
# top-level, KEEL_DIR on sys.path) or the dispatcher's
# `except StoreUnreachableError` clause would not match the class our
# doubles raise and every 503 test would see a 500 instead.
import server as keel_server  # noqa: E402
import store_api  # noqa: E402
from _env import HermeticCase  # noqa: E402
from keel.fallbackhub import (  # noqa: E402
    AMBIGUOUS,
    BEFORE_SEND,
    AmbiguousWriteError,
    FallbackHub,
    PrimaryFailure,
    classify_primary_failure,
)
from keel.serverhub import FRESH_PREFIX, HubUnreachableError, ServerHub  # noqa: E402

RUNNER_TOKEN = "runner-token-" + uuid.uuid4().hex
OPERATOR_TOKEN = "operator-token-" + uuid.uuid4().hex


def _sha256_hex(raw: str) -> str:
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def _token_store() -> keel_server.TokenStore:
    return keel_server.TokenStore(
        [
            {"id": "runner-1", "role": "runner", "sha256": _sha256_hex(RUNNER_TOKEN)},
            {"id": "operator-1", "role": "operator", "sha256": _sha256_hex(OPERATOR_TOKEN)},
        ]
    )


# --------------------------------------------------------------------- #
# Store doubles
# --------------------------------------------------------------------- #


class StaleIndexStore:
    """A `Store` with a real cache/truth split, for pinning r1.

    `truth` is the store; `index` is a cache the reads are served from
    unless `fresh=True`. Writes go through to both (write-through), which
    is the honest shape: staleness in this double comes only from a test
    reaching in and corrupting `index` -- the stand-in for "a runner
    moved the ref directly while the server's index had not swept yet".
    `InMemoryStore` cannot host these tests because it HAS no index:
    every read there is fresh by construction, so a missing fresh-forcing
    bug would pass against it.
    """

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self.truth: Dict[str, Tuple[str, dict]] = {}
        self.index: Dict[str, Tuple[str, dict]] = {}
        self._counter = 0
        self.fresh_calls: List[Tuple[str, bool]] = []  # (ref, fresh) per read

    def _next_sha(self) -> str:
        self._counter += 1
        return f"idx{self._counter:037d}"

    def corrupt_index(self, ref: str, bogus_sha: str) -> None:
        with self._lock:
            _sha, payload = self.index[ref]
            self.index[ref] = (bogus_sha, dict(payload))

    # -- reads --

    def sha(self, ref: str, *, fresh: bool = False) -> Optional[str]:
        return self.read_with_sha(ref, fresh=fresh)[0]

    def read(self, ref: str, *, fresh: bool = False) -> Optional[dict]:
        return self.read_with_sha(ref, fresh=fresh)[1]

    def read_with_sha(self, ref: str, *, fresh: bool = False) -> Tuple[Optional[str], Optional[dict]]:
        with self._lock:
            self.fresh_calls.append((ref, fresh))
            entry = (self.truth if fresh else self.index).get(ref)
            if entry is None:
                return (None, None)
            return (entry[0], dict(entry[1]))

    def list(self, prefix: str) -> Dict[str, store_api.RefListing]:
        now = time.time()
        norm = prefix if prefix.endswith("/") else prefix + "/"
        with self._lock:
            return {
                ref: store_api.RefListing(sha=sha, observed_at=now)
                for ref, (sha, _payload) in self.truth.items()
                if ref == prefix or ref.startswith(norm)
            }

    def fetch_namespace(self, prefix: str) -> Dict[str, str]:
        norm = prefix if prefix.endswith("/") else prefix + "/"
        with self._lock:
            return {
                ref: sha
                for ref, (sha, _payload) in self.truth.items()
                if ref == prefix or ref.startswith(norm)
            }

    # -- writes (write-through: truth and index together) --

    def create(self, ref: str, payload: dict) -> Union[str, bool]:
        with self._lock:
            if ref in self.truth:
                return False
            new_sha = self._next_sha()
            self.truth[ref] = (new_sha, dict(payload))
            self.index[ref] = (new_sha, dict(payload))
            return new_sha

    def update(self, ref: str, payload: dict, expect_sha: str) -> Union[str, bool]:
        with self._lock:
            entry = self.truth.get(ref)
            if entry is None or entry[0] != expect_sha:
                return False
            new_sha = self._next_sha()
            self.truth[ref] = (new_sha, dict(payload))
            self.index[ref] = (new_sha, dict(payload))
            return new_sha

    def delete(self, ref: str, expect_sha: str) -> bool:
        with self._lock:
            entry = self.truth.get(ref)
            if entry is None or entry[0] != expect_sha:
                return False
            del self.truth[ref]
            self.index.pop(ref, None)
            return True


class BrokenStore:
    """Every method raises `StoreUnreachableError` -- the server answers
    503 for all of it (the `not-ready` shape a `FallbackHub` may treat as
    before-send)."""

    def _boom(self, *a, **kw):
        raise store_api.StoreUnreachableError("spine away (test)")

    sha = read = read_with_sha = list = fetch_namespace = _boom
    create = update = delete = _boom


class CrashingStore:
    """Reads work (empty), writes crash AFTER the server has the request
    -- the dispatcher answers 500 (`internal-error`), the shape that is
    ambiguous for a write (`hubstore.WriteOutcomeUnknownError` reaches
    clients the same way)."""

    def sha(self, ref, *, fresh=False):
        return None

    def read(self, ref, *, fresh=False):
        return None

    def read_with_sha(self, ref, *, fresh=False):
        return (None, None)

    def list(self, prefix):
        return {}

    def fetch_namespace(self, prefix):
        return {}

    def _boom(self, *a, **kw):
        raise RuntimeError("cas exploded after the request arrived (test)")

    create = update = delete = _boom


class SlowCreateStore:
    """Delegates to an `InMemoryStore`, but `create` sleeps first: the
    client's read timeout fires AFTER the request was sent and the write
    then LANDS anyway -- the canonical ambiguous failure (SPEC §4.3 r2,
    seam 11's shape)."""

    def __init__(self, inner: store_api.InMemoryStore, delay_s: float):
        self.inner = inner
        self.delay_s = delay_s

    def __getattr__(self, name):
        return getattr(self.inner, name)

    def create(self, ref: str, payload: dict):
        time.sleep(self.delay_s)
        return self.inner.create(ref, payload)


class StubGitHubHalf:
    """The `github` half a `FallbackHub` falls back to: records every
    write so a test can assert exactly when the fallback route was (and
    was not) written."""

    def __init__(self) -> None:
        self.creates: List[Tuple[str, dict]] = []
        self.updates: List[Tuple[str, dict, str]] = []
        self.deletes: List[Tuple[str, str]] = []

    def sha(self, ref: str) -> Optional[str]:
        return None

    def read(self, ref: str) -> Optional[dict]:
        return None

    def read_with_sha(self, ref: str) -> Tuple[Optional[str], Optional[dict]]:
        return (None, None)

    def list(self, prefix: str) -> Dict[str, str]:
        return {}

    def fetch_namespace(self, prefix: str) -> Dict[str, str]:
        return {}

    def create(self, ref: str, payload: dict) -> bool:
        self.creates.append((ref, dict(payload)))
        return True

    def update(self, ref: str, payload: dict, expect_sha: str) -> bool:
        self.updates.append((ref, dict(payload), expect_sha))
        return True

    def delete(self, ref: str, expect_sha: str) -> bool:
        self.deletes.append((ref, expect_sha))
        return True


# --------------------------------------------------------------------- #
# Fixture
# --------------------------------------------------------------------- #


# WAS `_RaceReadyServer`, a `KeelHTTPServer` subclass this file kept only
# to raise `socketserver`'s default `request_queue_size` of 5 to 64: the
# eight-process racer test's simultaneous SYNs overflowed the listen
# backlog on loopback and the kernel RST'd the excess (connect `EINVAL`/
# `ECONNRESET`, or a broken pipe on send -- 7 red runs in 8 before the
# override, 0 in 20 after; macOS 27, Python 3.14). That override was
# evidence about PRODUCTION, not about this test: the same burst arrives
# at a real server every time a settle ends or a wave of renewals comes
# due, and `ServerHub` opens a fresh connection per call. `KeelHTTPServer`
# now carries the backlog itself (`request_queue_size = 64`, raised
# further by `server_activate` when `max_connections` is), so the fixture
# uses the real class and `tests/test_desired_route.py::TestListenBacklog`
# pins the property where it now lives.
_RaceReadyServer = keel_server.KeelHTTPServer


class ServerHubTestCase(HermeticCase):
    """A fresh fixture server per test (127.0.0.1:0 -- loopback only),
    watchdog effectively disabled, server logging quieted (the 500-path
    tests exercise `logging.exception` on purpose)."""

    def setUp(self) -> None:
        super().setUp()
        logging.disable(logging.CRITICAL)
        self.addCleanup(logging.disable, logging.NOTSET)

    def start_server(self, store, router: Optional[keel_server.Router] = None) -> str:
        config = keel_server.ServerConfig(
            bind_host="127.0.0.1",
            port=0,
            watchdog_timeout=100.0,
            watchdog_check_interval=100.0,
        )
        events = keel_server.EventLog(":memory:")
        self.addCleanup(events.close)
        server = _RaceReadyServer(
            ("127.0.0.1", 0), config, store, _token_store(), events, router=router
        )
        server.start()
        self.addCleanup(server.stop)
        self.server = server
        return f"http://127.0.0.1:{server.server_address[1]}"

    def hub(self, base_url: str, token: str = RUNNER_TOKEN, **kw) -> ServerHub:
        return ServerHub(base_url, token=token, **kw)

    def raw_get(self, base_url: str, path: str, token: str = RUNNER_TOKEN) -> Tuple[int, dict]:
        """A GET that bypasses `ServerHub` entirely -- for pinning what the
        SERVER does with a request the client never decorated."""
        port = int(base_url.rsplit(":", 1)[1])
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5.0)
        self.addCleanup(conn.close)
        conn.request("GET", path, headers={"Authorization": f"Bearer {token}"})
        resp = conn.getresponse()
        body = resp.read()
        return resp.status, (json.loads(body) if body else {})


# --------------------------------------------------------------------- #
# The 409 / 404 mapping (CAS semantics through the wire)
# --------------------------------------------------------------------- #


class TestStatusMapping(ServerHubTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.store = store_api.InMemoryStore()
        self.base_url = self.start_server(self.store)
        self.h = self.hub(self.base_url)

    def test_absent_ref_maps_404_to_none(self) -> None:
        self.assertIsNone(self.h.sha("refs/fleet/test/absent"))
        self.assertIsNone(self.h.read("refs/fleet/test/absent"))
        self.assertEqual(self.h.read_with_sha("refs/fleet/test/absent"), (None, None))

    def test_create_then_read_round_trips(self) -> None:
        ref = "refs/fleet/test/a"
        self.assertIs(self.h.create(ref, {"v": 1}), True)
        sha, payload = self.h.read_with_sha(ref)
        self.assertIsNotNone(sha)
        self.assertEqual(payload, {"v": 1})
        self.assertEqual(self.h.sha(ref), sha)

    def test_create_of_existing_ref_maps_409_to_false(self) -> None:
        ref = "refs/fleet/test/b"
        self.assertIs(self.h.create(ref, {"v": 1}), True)
        self.assertIs(self.h.create(ref, {"v": 2}), False)
        # The loser's payload never landed.
        self.assertEqual(self.h.read(ref), {"v": 1})

    def test_update_with_live_witness_wins_and_stale_witness_maps_409_to_false(self) -> None:
        ref = "refs/fleet/test/c"
        self.h.create(ref, {"v": 1})
        sha1 = self.h.sha(ref)
        self.assertIs(self.h.update(ref, {"v": 2}, expect_sha=sha1), True)
        # sha1 is now stale: the CAS must refuse, as False, never a raise.
        self.assertIs(self.h.update(ref, {"v": 3}, expect_sha=sha1), False)
        self.assertEqual(self.h.read(ref), {"v": 2})

    def test_delete_with_stale_witness_maps_409_to_false(self) -> None:
        ref = "refs/fleet/test/d"
        self.h.create(ref, {"v": 1})
        sha1 = self.h.sha(ref)
        self.h.update(ref, {"v": 2}, expect_sha=sha1)
        self.assertIs(self.h.delete(ref, expect_sha=sha1), False)  # moved
        sha2 = self.h.sha(ref)
        self.assertIs(self.h.delete(ref, expect_sha=sha2), True)
        self.assertIsNone(self.h.sha(ref))
        self.assertIs(self.h.delete(ref, expect_sha=sha2), False)  # already gone

    def test_list_is_hub_shaped_and_excludes_the_leaf_at_the_prefix(self) -> None:
        # `fleetlib.Hub.list` is `{ref: sha}` strictly UNDER the prefix
        # (its ls-remote pattern is `<prefix>/*`); `fetch_namespace` also
        # sees a leaf AT the prefix. `ServerHub` must present both shapes
        # or a FallbackHub route flip would change a caller's answer.
        self.h.create("refs/fleet/test/leaf", {"kind": "leaf"})
        self.h.create("refs/fleet/test/leaf/under", {"kind": "under"})
        listed = self.h.list("refs/fleet/test/leaf")
        self.assertEqual(list(listed), ["refs/fleet/test/leaf/under"])
        self.assertIsInstance(listed["refs/fleet/test/leaf/under"], str)
        ns = self.h.fetch_namespace("refs/fleet/test/leaf")
        self.assertEqual(sorted(ns), ["refs/fleet/test/leaf", "refs/fleet/test/leaf/under"])

    def test_auth_failure_raises_hub_unreachable_never_none_or_false(self) -> None:
        # SPEC §4.2: 401/403 -> HubUnreachableError, "never 404/409 on
        # transport" -- an auth problem must not read as "absent" or
        # "lost the race".
        bad = self.hub(self.base_url, token="not-a-real-token")
        with self.assertRaises(HubUnreachableError):
            bad.sha("refs/fleet/test/absent")
        with self.assertRaises(HubUnreachableError):
            bad.create("refs/fleet/test/auth", {"v": 1})
        # And it is NOT promoted into the fallback-safe bucket (the spec's
        # before-send list is exhaustive): a write raising it is ambiguous.
        try:
            bad.create("refs/fleet/test/auth", {"v": 1})
        except HubUnreachableError as exc:
            self.assertEqual(classify_primary_failure(exc), AMBIGUOUS)

    def test_push_ref_and_push_options_are_refused(self) -> None:
        with self.assertRaises(NotImplementedError):
            self.h.push_ref("abc:refs/heads/staging/x")
        with self.assertRaises(NotImplementedError):
            self.h.create("refs/fleet/test/po", {"v": 1}, push_options=["train-token=x"])
        # The refusal happened before any HTTP call: nothing was created.
        self.assertIsNone(self.h.sha("refs/fleet/test/po"))


# --------------------------------------------------------------------- #
# Transport phases: 503 / connection refused / 500 / timeout-after-send
# (what each raise means to a FallbackHub, SPEC §4.3 r2)
# --------------------------------------------------------------------- #


class TestTransportPhases(ServerHubTestCase):
    def test_503_is_before_send_and_fallbackhub_reissues_the_write(self) -> None:
        base_url = self.start_server(BrokenStore())
        h = self.hub(base_url)
        # Reads: raise, carrying the 503.
        with self.assertRaises(PrimaryFailure) as ctx:
            h.sha("refs/fleet/test/x")
        self.assertEqual(ctx.exception.status, 503)
        self.assertEqual(classify_primary_failure(ctx.exception), BEFORE_SEND)
        # Writes: the raise says before-send, so a FallbackHub re-issues
        # against its github half -- the one case r2 allows.
        with self.assertRaises(PrimaryFailure) as ctx:
            h.create("refs/fleet/test/x", {"v": 1})
        self.assertIs(ctx.exception.request_sent, False)
        self.assertEqual(classify_primary_failure(ctx.exception), BEFORE_SEND)
        github = StubGitHubHalf()
        fb = FallbackHub(h, github)
        self.assertIs(fb.create("refs/fleet/test/x", {"v": 1}), True)
        self.assertEqual(github.creates, [("refs/fleet/test/x", {"v": 1})])

    def test_connection_refused_is_before_send(self) -> None:
        # An ephemeral port with nothing listening: bind, learn the port,
        # close -- connecting to it is refused before any byte leaves.
        import socket

        probe = socket.socket()
        probe.bind(("127.0.0.1", 0))
        dead_port = probe.getsockname()[1]
        probe.close()
        h = ServerHub(f"http://127.0.0.1:{dead_port}", token=RUNNER_TOKEN, connect_timeout_s=2.0)
        with self.assertRaises(PrimaryFailure) as ctx:
            h.create("refs/fleet/test/x", {"v": 1})
        self.assertIs(ctx.exception.request_sent, False)
        self.assertEqual(classify_primary_failure(ctx.exception), BEFORE_SEND)

    def test_500_write_is_ambiguous_and_fallbackhub_never_reissues(self) -> None:
        # `hubstore.WriteOutcomeUnknownError` -> 500 is exactly this wire
        # shape: the server had the request and cannot say what the CAS
        # did (keel/hubstore.py point 4).
        base_url = self.start_server(CrashingStore())
        h = self.hub(base_url)
        with self.assertRaises(PrimaryFailure) as ctx:
            h.create("refs/fleet/test/x", {"v": 1})
        self.assertEqual(ctx.exception.status, 500)
        self.assertIs(ctx.exception.request_sent, True)
        self.assertEqual(classify_primary_failure(ctx.exception), AMBIGUOUS)
        github = StubGitHubHalf()
        fb = FallbackHub(h, github)
        with self.assertRaises(AmbiguousWriteError):
            fb.update("refs/fleet/test/x", {"v": 2}, expect_sha="a" * 40)
        self.assertEqual(github.updates, [])  # r2: never a second write

    def test_timeout_after_send_is_ambiguous_and_the_write_lands_anyway(self) -> None:
        # The canonical r2 case (seam 11's shape): the request was sent,
        # the client gave up, the server executed the CAS regardless. The
        # client must RAISE (never False, never a silent fallback write),
        # and the landed write is then what the next renewal's re-read
        # adopts (claim.py L661-663).
        inner = store_api.InMemoryStore()
        base_url = self.start_server(SlowCreateStore(inner, delay_s=1.2))
        h = self.hub(base_url, read_timeout_s=0.3)
        ref = "refs/fleet/test/slow"
        with self.assertRaises(PrimaryFailure) as ctx:
            h.create(ref, {"v": 1})
        self.assertIs(ctx.exception.request_sent, True)
        self.assertEqual(classify_primary_failure(ctx.exception), AMBIGUOUS)
        # The write the client could not see LANDS on the store.
        deadline = time.monotonic() + 5.0
        while inner.sha(ref) is None and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertIsNotNone(inner.sha(ref), "the ambiguous write should have landed")
        # Through a FallbackHub the same failure raises and the github
        # half is never written.
        github = StubGitHubHalf()
        fb = FallbackHub(h, github)
        with self.assertRaises(AmbiguousWriteError):
            fb.create("refs/fleet/test/slow2", {"v": 1})
        self.assertEqual(github.creates, [])


# --------------------------------------------------------------------- #
# r1: claims are read live, on both sides of the wire
# --------------------------------------------------------------------- #


class TestFreshClaims(ServerHubTestCase):
    CLAIM_REF = FRESH_PREFIX + "gate/staging-x"
    PLAIN_REF = "refs/fleet/verdicts/deadbeef/8/linux"

    def _seed(self, store: StaleIndexStore, h: ServerHub) -> Tuple[str, str]:
        """Create one claim and one non-claim ref through the server,
        then corrupt BOTH index entries. Returns their true shas."""
        self.assertIs(h.create(self.CLAIM_REF, {"holder_host": "m5"}), True)
        self.assertIs(h.create(self.PLAIN_REF, {"verdict": "PASS"}), True)
        claim_true = store.truth[self.CLAIM_REF][0]
        plain_true = store.truth[self.PLAIN_REF][0]
        store.corrupt_index(self.CLAIM_REF, "f" * 40)
        store.corrupt_index(self.PLAIN_REF, "e" * 40)
        return claim_true, plain_true

    def test_corrupted_index_entry_for_a_claim_still_returns_the_true_sha(self) -> None:
        # The acceptance test for this task (PLAN Stage 2): a stale index
        # sha on our own claim is a healthy gate killed (claim.py renew ->
        # _mark_lost), so the answer must come from truth, not the index.
        store = StaleIndexStore()
        h = self.hub(self.start_server(store))
        claim_true, _plain_true = self._seed(store, h)
        self.assertEqual(h.sha(self.CLAIM_REF), claim_true)
        sha, payload = h.read_with_sha(self.CLAIM_REF)
        self.assertEqual(sha, claim_true)
        self.assertEqual(payload, {"holder_host": "m5"})

    def test_negative_control_a_non_claim_ref_is_served_from_the_index(self) -> None:
        # The double's teeth: had the ref not been a claim, the corrupted
        # index entry WOULD have been the answer. Without this control the
        # test above could pass against a store with no cache at all.
        store = StaleIndexStore()
        h = self.hub(self.start_server(store))
        _claim_true, plain_true = self._seed(store, h)
        self.assertEqual(h.sha(self.PLAIN_REF), "e" * 40)
        self.assertNotEqual(h.sha(self.PLAIN_REF), plain_true)

    def test_server_forces_fresh_for_claims_even_without_the_query_param(self) -> None:
        # The server-side half of r1 (this task's edit to
        # `server.handle_refs_get`): a GET that never asked for `?fresh=1`
        # -- a client that forgot, or predates the rule -- still gets the
        # claim's true sha.
        store = StaleIndexStore()
        base_url = self.start_server(store)
        h = self.hub(base_url)
        claim_true, plain_true = self._seed(store, h)
        status, body = self.raw_get(base_url, "/v1/refs/" + self.CLAIM_REF)
        self.assertEqual(status, 200)
        self.assertEqual(body["sha"], claim_true)
        # ... while a non-claim GET without the param is index-served:
        # the forcing is claims-scoped, not a blanket cache bypass.
        status, body = self.raw_get(base_url, "/v1/refs/" + self.PLAIN_REF)
        self.assertEqual(status, 200)
        self.assertEqual(body["sha"], "e" * 40)
        self.assertNotEqual(body["sha"], plain_true)

    def test_client_forces_fresh_even_when_the_server_does_not(self) -> None:
        # The client-side half of r1: against a LEGACY server whose GET
        # handler honours only the query string (the pre-task handler,
        # reconstructed here), `ServerHub`'s own `?fresh=1` must be what
        # keeps the claim live.
        def legacy_refs_get(handler, params, query, principal):
            ref = keel_server._decode_ref(params["ref"])
            fresh = query.get("fresh", ["0"])[0] == "1"  # no claims forcing
            sha, payload = handler.server.store.read_with_sha(ref, fresh=fresh)
            if sha is None:
                handler._send_json(404, {"error": "not-found"})
                return
            handler._send_json(200, {"sha": sha, "payload": payload})

        router = keel_server.Router()
        router.add("GET", "/v1/refs/{ref:path}", legacy_refs_get, auth=frozenset())
        router.add(
            "POST", "/v1/refs/{ref:path}", keel_server.handle_refs_post,
            auth=frozenset({"runner", "operator"}),
        )
        store = StaleIndexStore()
        base_url = self.start_server(store, router=router)
        h = self.hub(base_url)
        claim_true, _plain = self._seed(store, h)
        # Control first: the legacy server really is legacy -- an
        # undecorated GET serves the corrupted index entry.
        status, body = self.raw_get(base_url, "/v1/refs/" + self.CLAIM_REF)
        self.assertEqual(status, 200)
        self.assertEqual(body["sha"], "f" * 40)
        # The client's forced `?fresh=1` alone gets the truth.
        self.assertEqual(h.sha(self.CLAIM_REF), claim_true)
        self.assertEqual(h.read_with_sha(self.CLAIM_REF)[0], claim_true)


# --------------------------------------------------------------------- #
# Eight racers, one winner, THROUGH the server
# --------------------------------------------------------------------- #

RACER_COUNT = 8


def _race_create(base_url: str, token: str, ref: str, racer: int, barrier) -> Tuple[int, bool]:
    """Runs in a spawned worker process: everything it needs arrives as
    arguments (module-level tokens are re-randomised on re-import under
    the spawn start method, so they must NOT be read here)."""
    from keel.serverhub import ServerHub as _ServerHub  # noqa: PLC0415 -- child import path

    hub = _ServerHub(base_url, token=token)
    barrier.wait()  # all eight release together: a real race, not a queue
    won = hub.create(ref, {"racer": racer})
    return (racer, bool(won))


class TestOneWinnerRace(ServerHubTestCase):
    def test_eight_process_racers_creating_one_ref_yield_exactly_one_winner(self) -> None:
        store = store_api.InMemoryStore()
        base_url = self.start_server(store)
        ref = "refs/fleet/claims/gate/contested"
        ctx = multiprocessing.get_context("spawn")
        with ctx.Manager() as manager:
            barrier = manager.Barrier(RACER_COUNT, timeout=60)
            with concurrent.futures.ProcessPoolExecutor(max_workers=RACER_COUNT, mp_context=ctx) as pool:
                futures = [
                    pool.submit(_race_create, base_url, RUNNER_TOKEN, ref, i, barrier)
                    for i in range(RACER_COUNT)
                ]
                results = [f.result(timeout=120) for f in futures]
        winners = sorted(racer for racer, won in results if won)
        losers = sorted(racer for racer, won in results if not won)
        self.assertEqual(len(results), RACER_COUNT)
        self.assertEqual(
            len(winners), 1,
            f"exactly one racer must win the create; winners={winners} losers={losers}",
        )
        # The landed payload is the winner's, byte for byte -- no loser's
        # payload half-applied, no last-writer-wins smear.
        payload = store.read(ref)
        self.assertEqual(payload, {"racer": winners[0]})


if __name__ == "__main__":
    unittest.main()
