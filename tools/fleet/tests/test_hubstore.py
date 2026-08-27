#!/usr/bin/env python3
"""Tests for tools/fleet/keel/hubstore.py -- `CachedHubStore`, the one
adapter between `server.py`'s `Store` contract and `CachedHub`, and the
wiring of the two through a real `KeelHTTPServer`.

Fixture: a throwaway `git init --bare` state repo under the system temp
dir (asserted), two independent `fleetlib.Hub` clients on it -- `store`,
the one the `CachedHub` under test wraps, and `direct`, a runner writing
on the fallback route whose changes the index cannot see until a sweep --
and, for the HTTP tests, a `KeelHTTPServer` on 127.0.0.1:0 fronting the
adapter. Never a real remote, never the production hub.

What is pinned, and the bug that makes each test fail (checked by
monkeypatching the module from a scratch runner; the module itself is
never edited):
  * the adapter is a `store_api.Store` and keeps the False/None/raise
    contract; `create`/`update` return the sha the STORE holds, not a bool
    (bug: return the bool through);
  * r1 through the adapter AND through `GET /v1/refs/<claim>`: a corrupt
    index entry for a claim never reaches the wire; a non-claim is the
    negative control and `?fresh=1` bypasses its index entry (bug: serve
    reads from the index / ignore `fresh`);
  * `/v1/wait` observes a DIRECT writer's change (same bug);
  * `list()` includes the leaf AT the prefix and reports `observed_at`
    (bug: `Hub.list` shape);
  * a READ transport failure is 503; a WRITE transport failure is a 5xx
    that is NOT 503 (bug: map both to `StoreUnreachableError` -- the
    client would re-issue the write, SPEC §4.3 rule 2);
  * a landed write whose readback failed still answers the live sha;
    when even that fails the adapter says "unknown", never a stale sha
    (bug: return the pre-CAS entry's sha).

Run with:
    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_hubstore -v
"""

from __future__ import annotations

import hashlib
import http.client
import json
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid
from pathlib import Path
from typing import Optional, Tuple

FLEET_DIR = Path(__file__).resolve().parents[1]
KEEL_DIR = FLEET_DIR / "keel"
for _p in (FLEET_DIR, KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import hubstore  # noqa: E402
import server as keel_server  # noqa: E402
import store_api  # noqa: E402
from _env import HermeticCase  # noqa: E402
from fleetlib import Hub, HubUnreachableError  # noqa: E402
from keel.cachedhub import SOURCE_SWEEP, CachedHub, RefEntry  # noqa: E402

CLAIMS = "refs/fleet/claims/"
RUNNER_TOKEN = "runner-token-" + uuid.uuid4().hex


def _bare_repo(root: Path) -> str:
    path = root / "state.git"
    init = subprocess.run(["git", "init", "--quiet", "--bare", str(path)], capture_output=True)
    assert init.returncode == 0, init.stderr.decode()
    resolved = str(path.resolve())
    system_tmp = str(Path(tempfile.gettempdir()).resolve())
    assert resolved.startswith(system_tmp), f"fixture {resolved!r} is not under {system_tmp!r}"
    return str(path)


class HubStoreTestCase(HermeticCase):
    def setUp(self):
        super().setUp()
        self._root = Path(tempfile.mkdtemp(prefix="hubstore-test-"))
        self.hub_url = _bare_repo(self._root)
        self.store = Hub(url=self.hub_url, workdir=self._root / "store-cache")
        self.direct = Hub(url=self.hub_url, workdir=self._root / "direct-cache")
        self.ns = f"refs/fleet/test/{uuid.uuid4().hex[:12]}/"
        self.claims_ns = f"{CLAIMS}gate/{uuid.uuid4().hex[:12]}-"
        self.cached = CachedHub(self.store)
        self.adapter = hubstore.CachedHubStore(self.cached)

    def tearDown(self):
        self.adapter.close()
        shutil.rmtree(self._root, ignore_errors=True)

    def ref(self, name: str) -> str:
        return self.ns + name

    def claim_ref(self, name: str) -> str:
        return self.claims_ns + name

    def corrupt(self, ref: str, sha: str = "d" * 40, payload=None) -> None:
        """Plant a wrong index entry far in the future on the tick clock so
        no observation in this test can repair it by the monotonic rule."""
        self.cached.index._entries[ref] = RefEntry(sha, payload, time.monotonic() + 1e6, time.time(), SOURCE_SWEEP)

    def fail_store(self, method: str, times: Optional[int] = None, after_calls: int = 0):
        """Make `self.store.<method>` raise `HubUnreachableError` -- for
        `times` calls (None = forever) once `after_calls` real calls have
        gone through. Returns the list of raised-call args."""
        real = getattr(self.store, method)
        raised: list = []
        state = {"seen": 0}

        def broken(*args, **kwargs):
            state["seen"] += 1
            if state["seen"] > after_calls and (times is None or len(raised) < times):
                raised.append(args)
                raise HubUnreachableError(f"injected: {method} unreachable")
            return real(*args, **kwargs)

        setattr(self.store, method, broken)
        return raised


# --------------------------------------------------------------------- #
# Contract
# --------------------------------------------------------------------- #


class TestContract(HubStoreTestCase):
    def test_adapter_is_a_store(self):
        self.assertIsInstance(self.adapter, store_api.Store)
        self.assertIsInstance(store_api.InMemoryStore(), store_api.Store)
        self.assertIs(self.adapter.cached, self.cached)
        self.assertIs(self.adapter.hub, self.store)

    def test_contract_values_and_landed_shas(self):
        ref = self.ref("a")
        self.assertIsNone(self.adapter.sha(ref))
        self.assertEqual(self.adapter.read_with_sha(ref), (None, None))
        created = self.adapter.create(ref, {"v": 1})
        self.assertIsInstance(created, str)
        self.assertEqual(len(created), 40)
        self.assertEqual(created, self.direct.sha(ref), "create answers the sha the store holds")
        self.assertIs(self.adapter.create(ref, {"v": 2}), False)
        self.assertIs(self.adapter.update(ref, {"v": 3}, expect_sha="0" * 40), False)
        updated = self.adapter.update(ref, {"v": 3}, expect_sha=created)
        self.assertIsInstance(updated, str)
        self.assertNotEqual(updated, created)
        self.assertEqual(updated, self.direct.sha(ref), "update answers the sha the store holds")
        self.assertEqual(self.adapter.read(ref)["v"], 3)
        sha, payload = self.adapter.read_with_sha(ref)
        self.assertEqual((sha, payload["v"]), (updated, 3))
        self.assertIs(self.adapter.delete(ref, expect_sha="0" * 40), False)
        self.assertIs(self.adapter.delete(ref, expect_sha=updated), True)
        self.assertIsNone(self.adapter.sha(ref))
        self.assertEqual(self.adapter.read_with_sha(ref), (None, None))
        self.assertIsNone(self.direct.sha(ref))

    def test_landed_sha_is_the_stores_even_for_a_claim(self):
        ref = self.claim_ref("landed")
        created = self.adapter.create(ref, {"holder_host": "h", "started_at": "t"})
        self.assertEqual(created, self.direct.sha(ref))
        updated = self.adapter.update(ref, {"holder_host": "h", "started_at": "t", "n": 2}, expect_sha=created)
        self.assertEqual(updated, self.direct.sha(ref))

    def test_list_includes_the_leaf_at_the_prefix_and_reports_observed_at(self):
        leaf = self.ref("leafdir")
        before = time.time()
        self.assertIsInstance(self.adapter.create(leaf, {"k": "leaf"}), str)
        self.assertIsInstance(self.adapter.create(self.ref("leafdir-sibling"), {"k": "s"}), str)
        other = self.ref("other/child")
        self.assertTrue(self.direct.create(other, {"k": "child"}))
        listing = self.adapter.list(leaf)
        self.assertEqual(set(listing), {leaf}, "the ref AT the prefix is listed, siblings with a shared stem are not")
        entry = listing[leaf]
        self.assertIsInstance(entry, store_api.RefListing)
        self.assertEqual(entry.sha, self.direct.sha(leaf))
        self.assertGreaterEqual(entry.observed_at, before)
        self.assertLessEqual(entry.observed_at, time.time())
        self.cached.sweep()  # the direct writer's ref is invisible to the index until a sweep
        whole = self.adapter.list(self.ns)
        self.assertEqual({r: e.sha for r, e in whole.items()}, self.direct.fetch_namespace(self.ns))
        self.assertEqual(self.adapter.fetch_namespace(self.ns), self.direct.fetch_namespace(self.ns))

    def test_fresh_read_bypasses_the_index_for_a_non_claim(self):
        ref = self.ref("plain")
        created = self.adapter.create(ref, {"v": 1})
        self.assertTrue(self.direct.update(ref, {"v": 2}, expect_sha=created))
        truth = self.direct.sha(ref)
        self.assertEqual(self.adapter.sha(ref), created, "a direct write is invisible to the index until a sweep")
        self.assertEqual(self.adapter.read(ref)["v"], 1)
        self.assertEqual(self.adapter.sha(ref, fresh=True), truth)
        self.assertEqual(self.adapter.read_with_sha(ref, fresh=True), (truth, self.direct.read(ref)))
        self.assertEqual(self.adapter.read(ref, fresh=True)["v"], 2)


# --------------------------------------------------------------------- #
# r1 through the adapter
# --------------------------------------------------------------------- #


class TestFreshClaims(HubStoreTestCase):
    def test_corrupt_index_entry_for_a_claim_never_reaches_the_caller(self):
        ref = self.claim_ref("corrupt")
        self.assertIsInstance(self.adapter.create(ref, {"holder_host": "h", "started_at": "t"}), str)
        truth = self.direct.sha(ref)
        self.corrupt(ref, payload={"holder_host": "imposter", "started_at": "never"})
        self.assertEqual(self.adapter.sha(ref), truth)
        self.assertEqual(self.adapter.sha(ref, fresh=True), truth)
        sha, payload = self.adapter.read_with_sha(ref)
        self.assertEqual((sha, payload["holder_host"]), (truth, "h"))
        self.assertEqual(self.adapter.read(ref)["holder_host"], "h")

    def test_negative_control_non_claim_is_index_served_unless_fresh(self):
        ref = self.ref("noncl")
        self.assertIsInstance(self.adapter.create(ref, {"v": 1}), str)
        truth = self.direct.sha(ref)
        self.corrupt(ref, payload={"v": "planted"})
        self.assertEqual(self.adapter.sha(ref), "d" * 40)
        self.assertEqual(self.adapter.read(ref)["v"], "planted")
        self.assertEqual(self.adapter.sha(ref, fresh=True), truth)
        self.assertEqual(self.adapter.read(ref, fresh=True)["v"], 1)

    def test_direct_renewal_of_a_claim_is_visible_without_a_sweep(self):
        ref = self.claim_ref("renewed")
        created = self.adapter.create(ref, {"holder_host": "h", "started_at": "t"})
        self.assertTrue(self.direct.update(ref, {"holder_host": "h", "started_at": "t", "n": 2}, expect_sha=created))
        self.assertEqual(self.adapter.sha(ref), self.direct.sha(ref))
        self.assertEqual(self.adapter.read(ref)["n"], 2)


# --------------------------------------------------------------------- #
# Failure mapping (point 4 of the module docstring)
# --------------------------------------------------------------------- #


class TestFailureMapping(HubStoreTestCase):
    def test_read_transport_failure_is_store_unreachable(self):
        ref = self.claim_ref("down")
        self.fail_store("sha")
        self.fail_store("read_with_sha")
        self.fail_store("fetch_namespace")
        with self.assertRaises(store_api.StoreUnreachableError):
            self.adapter.sha(ref)
        with self.assertRaises(store_api.StoreUnreachableError):
            self.adapter.read_with_sha(ref)
        with self.assertRaises(store_api.StoreUnreachableError):
            self.adapter.sha(self.ref("x"), fresh=True)
        # A prefix INSIDE the indexed namespace is index-served by design and
        # never touches the store; one outside it is the live-read probe.
        self.assertEqual(self.adapter.list(self.ref("dir")), {})
        with self.assertRaises(store_api.StoreUnreachableError):
            self.adapter.list("refs/elsewhere/dir")

    def test_write_transport_failure_is_never_store_unreachable(self):
        ref = self.ref("w")
        created = self.adapter.create(ref, {"v": 1})
        self.fail_store("create")
        self.fail_store("update")
        self.fail_store("delete")
        for op in (
            lambda: self.adapter.create(self.ref("new"), {"v": 1}),
            lambda: self.adapter.update(ref, {"v": 2}, expect_sha=created),
            lambda: self.adapter.delete(ref, expect_sha=created),
        ):
            with self.assertRaises(hubstore.WriteOutcomeUnknownError) as cm:
                op()
            self.assertNotIsInstance(cm.exception, store_api.StoreUnreachableError)
            self.assertFalse(cm.exception.landed)
            self.assertIsInstance(cm.exception.cause, HubUnreachableError)
        self.assertEqual(self.direct.sha(ref), created, "nothing was written")

    def test_landed_write_with_failed_readback_answers_the_live_sha(self):
        ref = self.ref("rb")
        created = self.adapter.create(ref, {"v": 1})
        raised = self.fail_store("read_with_sha", times=1)
        updated = self.adapter.update(ref, {"v": 2}, expect_sha=created)
        self.assertEqual(len(raised), 1, "the readback after the CAS failed")
        self.assertEqual(updated, self.direct.sha(ref))
        self.assertNotEqual(updated, created)
        self.assertEqual(self.adapter.read(ref)["v"], 2)

    def test_landed_write_whose_sha_is_unobservable_says_unknown(self):
        ref = self.ref("unk")
        created = self.adapter.create(ref, {"v": 1})
        self.fail_store("read_with_sha")
        self.fail_store("sha")
        with self.assertRaises(hubstore.WriteOutcomeUnknownError) as cm:
            self.adapter.update(ref, {"v": 2}, expect_sha=created)
        self.assertTrue(cm.exception.landed)
        self.assertNotIsInstance(cm.exception, store_api.StoreUnreachableError)
        self.assertNotEqual(self.direct.sha(ref), created, "the CAS did land")
        self.assertEqual(self.direct.read(ref)["v"], 2)


# --------------------------------------------------------------------- #
# Through a real KeelHTTPServer
# --------------------------------------------------------------------- #


class TestThroughServer(HubStoreTestCase):
    def setUp(self):
        super().setUp()
        self.events = keel_server.EventLog(self._root / "events.db")
        tokens = keel_server.TokenStore(
            [{"id": "runner-1", "role": "runner", "sha256": hashlib.sha256(RUNNER_TOKEN.encode()).hexdigest()}]
        )
        config = keel_server.ServerConfig(
            bind_host="127.0.0.1", port=0, long_poll_timeout=10.0, long_poll_interval=0.1,
            watchdog_timeout=60.0, watchdog_check_interval=60.0,
        )
        self.server = keel_server.build_server(
            config, store=self.adapter, tokens=tokens, events=self.events, health_provider=self.adapter.health
        )
        self.server.start()
        self.port = self.server.server_address[1]

    def tearDown(self):
        self.server.stop()
        self.events.close()
        super().tearDown()

    def request(self, method: str, path: str, body=None, headers=None, timeout: float = 15.0) -> Tuple[int, dict]:
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout)
        hdrs = {"Authorization": f"Bearer {RUNNER_TOKEN}"}
        if headers:
            hdrs.update(headers)
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            hdrs["Content-Type"] = "application/json"
        try:
            conn.request(method, path, body=data, headers=hdrs)
            resp = conn.getresponse()
            raw = resp.read()
            parsed = json.loads(raw.decode()) if raw else {}
            return resp.status, parsed
        finally:
            conn.close()

    def test_cas_round_trip_answers_the_stores_shas(self):
        ref = self.ref("http")
        status, body = self.request("GET", f"/v1/refs/{ref}")
        self.assertEqual(status, 404)
        status, body = self.request("PUT", f"/v1/refs/{ref}", {"v": 1}, {"If-None-Match": "*"})
        self.assertEqual(status, 201, body)
        self.assertEqual(body["sha"], self.direct.sha(ref))
        status, _ = self.request("PUT", f"/v1/refs/{ref}", {"v": 9}, {"If-None-Match": "*"})
        self.assertEqual(status, 409)
        status, body2 = self.request("PUT", f"/v1/refs/{ref}", {"v": 2}, {"If-Match": body["sha"]})
        self.assertEqual(status, 200, body2)
        self.assertEqual(body2["sha"], self.direct.sha(ref))
        status, _ = self.request("PUT", f"/v1/refs/{ref}", {"v": 3}, {"If-Match": body["sha"]})
        self.assertEqual(status, 409, "a stale witness is a lost race")
        status, got = self.request("GET", f"/v1/refs/{ref}")
        self.assertEqual((status, got["sha"], got["payload"]["v"]), (200, body2["sha"], 2))
        status, listing = self.request("GET", f"/v1/refs?prefix={self.ns}")
        self.assertEqual(status, 200)
        self.assertEqual(listing[ref]["sha"], body2["sha"])
        self.assertIn("observed_at", listing[ref])
        status, _ = self.request("DELETE", f"/v1/refs/{ref}", headers={"If-Match": body["sha"]})
        self.assertEqual(status, 409)
        status, _ = self.request("DELETE", f"/v1/refs/{ref}", headers={"If-Match": body2["sha"]})
        self.assertEqual(status, 204)
        self.assertIsNone(self.direct.sha(ref))
        status, _ = self.request("GET", f"/v1/refs/{ref}")
        self.assertEqual(status, 404)

    def test_r1_a_corrupt_index_entry_for_a_claim_never_reaches_the_wire(self):
        ref = self.claim_ref("wire")
        self.assertIsInstance(self.adapter.create(ref, {"holder_host": "h", "started_at": "t"}), str)
        truth = self.direct.sha(ref)
        self.corrupt(ref, payload={"holder_host": "imposter", "started_at": "never"})
        status, got = self.request("GET", f"/v1/refs/{ref}")
        self.assertEqual(status, 200)
        self.assertEqual((got["sha"], got["payload"]["holder_host"]), (truth, "h"))
        # Negative control: the non-claim is index-served, and ?fresh=1 is the way past it.
        plain = self.ref("plain")
        self.assertIsInstance(self.adapter.create(plain, {"v": 1}), str)
        self.corrupt(plain, payload={"v": "planted"})
        status, got = self.request("GET", f"/v1/refs/{plain}")
        self.assertEqual((status, got["sha"], got["payload"]["v"]), (200, "d" * 40, "planted"))
        status, got = self.request("GET", f"/v1/refs/{plain}?fresh=1")
        self.assertEqual((status, got["sha"], got["payload"]["v"]), (200, self.direct.sha(plain), 1))

    def test_wait_observes_a_direct_writers_change(self):
        ref = self.ref("waited")
        created = self.adapter.create(ref, {"v": 1})

        def later():
            time.sleep(0.4)
            self.direct.update(ref, {"v": 2}, expect_sha=created)

        t = threading.Thread(target=later, daemon=True)
        t.start()
        started = time.monotonic()
        status, got = self.request("GET", f"/v1/wait?ref={ref}&since={created}")
        t.join()
        self.assertEqual(status, 200, got)
        self.assertEqual(got["sha"], self.direct.sha(ref))
        self.assertNotEqual(got["sha"], created)
        self.assertLess(time.monotonic() - started, 8.0, "woke on the change, not on the timeout")

    def test_read_failure_is_503_write_failure_is_not(self):
        ref = self.ref("f")
        created = self.adapter.create(ref, {"v": 1})
        self.fail_store("update")
        status, body = self.request("PUT", f"/v1/refs/{ref}", {"v": 2}, {"If-Match": created})
        self.assertGreaterEqual(status, 500)
        self.assertNotEqual(status, 503, "a write that raised in transport must never look like before-send (r2)")
        self.assertEqual(self.direct.sha(ref), created)
        self.fail_store("read_with_sha")
        status, body = self.request("GET", f"/v1/refs/{ref}?fresh=1")
        self.assertEqual((status, body.get("error")), (503, "store-unreachable"))

    def test_health_carries_the_index_fields(self):
        status, body = self.request("GET", "/v1/health")
        self.assertEqual(status, 200)
        self.assertTrue(body["github_ok"])
        self.assertEqual(body["sweeps"], 1)
        self.assertEqual(body["state_url"], self.hub_url)
        self.assertIn("index_observed_at", body)


class TestBuildStore(HermeticCase):
    def test_build_store_sweeps_and_close_stops_the_sweeper(self):
        root = Path(tempfile.mkdtemp(prefix="hubstore-build-"))
        try:
            url = _bare_repo(root)
            direct = Hub(url=url, workdir=root / "direct")
            ref = f"refs/fleet/test/{uuid.uuid4().hex[:8]}/seed"
            self.assertTrue(direct.create(ref, {"v": 1}))
            store = hubstore.build_store(url, root / "server-cache", sweep_interval=0.2)
            try:
                self.assertIsInstance(store, store_api.Store)
                self.assertTrue(store.cached.sweeper_running())
                self.assertEqual(store.sha(ref), direct.sha(ref))
                self.assertEqual(store.hub.url, url)
                deadline = time.monotonic() + 5
                while store.cached.sweeps < 2 and time.monotonic() < deadline:
                    time.sleep(0.05)
                self.assertGreaterEqual(store.cached.sweeps, 2, "the sweeper thread ran")
                health = store.health()
                self.assertTrue(health["github_ok"])
                self.assertTrue(health["sweeper_running"])
            finally:
                store.close()
            self.assertFalse(store.cached.sweeper_running())
            with self.assertRaises(HubUnreachableError):
                hubstore.build_store(str(root / "does-not-exist.git"), root / "nope-cache", sweep_interval=0)
        finally:
            shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
