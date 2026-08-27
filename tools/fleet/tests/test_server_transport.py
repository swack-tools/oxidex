#!/usr/bin/env python3
"""Tests for `tools/fleet/keel/server.py` -- the keel-server TRANSPORT
layer: HTTP/1.1 keep-alive, chunked SSE replay for `/v1/events`, the
`/v1/wait` long-poll, bearer auth (401 vs 403), the connection cap, and the
listener watchdog. See that module's docstring and
`docs/AGENT-SERVER-SPEC.md` SS2 C6 / SS5 / SS8 for the contract.

Every test here runs against a real `KeelHTTPServer` bound to
`127.0.0.1:0` (an OS-assigned ephemeral port), fronting a
`store_api.InMemoryStore` -- never a real git remote, never the eventual
`CachedHub`. That is deliberate: this file tests the wire protocol and the
transport mechanisms, not ref-CAS semantics (which belong to
`test_serverhub.py`/`CachedHub`'s own tests once those land) -- see
`server.py`'s module docstring for the ownership split.

Run with:
    python3 -m unittest tools.fleet.tests.test_server_transport -v
or, from this directory:
    python3 -m unittest test_server_transport -v
"""

from __future__ import annotations

import dataclasses
import hashlib
import http.client
import json
import logging
import os
import socket
import sys
import tempfile
import threading
import time
import unittest
import uuid
from pathlib import Path
from typing import Dict, List, Optional, Tuple

FLEET_DIR = Path(__file__).resolve().parents[1]
KEEL_DIR = FLEET_DIR / "keel"
for _p in (FLEET_DIR, KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import server as keel_server  # noqa: E402
import store_api  # noqa: E402
from _env import HermeticCase  # noqa: E402


# --------------------------------------------------------------------- #
# Shared fixtures / helpers
# --------------------------------------------------------------------- #

RUNNER_TOKEN = "runner-token-" + uuid.uuid4().hex
OPERATOR_TOKEN = "operator-token-" + uuid.uuid4().hex
AGENT_TOKEN = "agent-token-" + uuid.uuid4().hex


def _sha256_hex(raw: str) -> str:
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def _default_token_store() -> keel_server.TokenStore:
    return keel_server.TokenStore(
        [
            {"id": "runner-1", "role": "runner", "sha256": _sha256_hex(RUNNER_TOKEN)},
            {"id": "operator-1", "role": "operator", "sha256": _sha256_hex(OPERATOR_TOKEN)},
            {"id": "agent-1", "role": "agent", "sha256": _sha256_hex(AGENT_TOKEN)},
        ]
    )


class _RawHTTPClient:
    """A hand-rolled HTTP/1.1 client for the one case `http.client` cannot
    do cleanly: reading a `Transfer-Encoding: chunked` response that never
    ends (SSE) chunk by chunk as they arrive, without waiting for EOF."""

    def __init__(self, host: str, port: int, timeout: float = 5.0):
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.rfile = self.sock.makefile("rb")

    def send_request(self, method: str, path: str, headers: Optional[Dict[str, str]] = None) -> None:
        lines = [f"{method} {path} HTTP/1.1", "Host: 127.0.0.1"]
        for key, value in (headers or {}).items():
            lines.append(f"{key}: {value}")
        lines.append("")
        lines.append("")
        self.sock.sendall("\r\n".join(lines).encode("ascii"))

    def read_status_and_headers(self) -> Tuple[int, Dict[str, str]]:
        status_line = self.rfile.readline().decode("iso-8859-1").strip()
        status = int(status_line.split(" ")[1])
        headers: Dict[str, str] = {}
        while True:
            line = self.rfile.readline().decode("iso-8859-1").strip()
            if line == "":
                break
            key, _, value = line.partition(":")
            headers[key.strip().lower()] = value.strip()
        return status, headers

    def read_chunk(self) -> Optional[bytes]:
        """One `Transfer-Encoding: chunked` chunk's data, or `b""` for the
        terminal zero-length chunk. Blocks (up to the socket timeout) until
        a full chunk is available -- exactly one call per server-side
        `_write_chunk()`, since `server.py` flushes every chunk on its own."""
        size_line = self.rfile.readline().decode("ascii").strip()
        if not size_line:
            return None
        size = int(size_line.split(";")[0], 16)
        data = self.rfile.read(size) if size else b""
        self.rfile.readline()  # trailing CRLF after the chunk data
        if size == 0:
            return b""
        return data

    def close(self) -> None:
        try:
            self.rfile.close()
        finally:
            self.sock.close()


class ServerTestCase(HermeticCase):
    """Base class: a fresh `KeelHTTPServer` per test, small/fast timeouts
    throughout so the whole suite stays well inside a gate's per-module
    budget, torn down in `tearDown` regardless of how the test ended."""

    max_connections = 64
    watchdog_timeout = 100.0  # effectively disabled unless a test overrides it
    watchdog_check_interval = 100.0
    long_poll_timeout = 1.0
    long_poll_interval = 0.02
    sse_keepalive_interval = 15.0

    def setUp(self) -> None:
        super().setUp()
        self.store = store_api.InMemoryStore()
        self.tokens = _default_token_store()
        self.events = keel_server.EventLog(":memory:")
        self.addCleanup(self.events.close)
        config = keel_server.ServerConfig(
            bind_host="127.0.0.1",
            port=0,
            max_connections=self.max_connections,
            long_poll_timeout=self.long_poll_timeout,
            long_poll_interval=self.long_poll_interval,
            sse_keepalive_interval=self.sse_keepalive_interval,
            watchdog_timeout=self.watchdog_timeout,
            watchdog_check_interval=self.watchdog_check_interval,
        )
        self.server = keel_server.build_server(config, store=self.store, tokens=self.tokens, events=self.events)
        self.server.start()
        self.host, self.port = self.server.server_address[0], self.server.server_address[1]
        self.addCleanup(self.server.stop)

    def http_connection(self, timeout: float = 5.0) -> http.client.HTTPConnection:
        conn = http.client.HTTPConnection(self.host, self.port, timeout=timeout)
        self.addCleanup(conn.close)
        return conn

    def raw_client(self, timeout: float = 5.0) -> _RawHTTPClient:
        client = _RawHTTPClient(self.host, self.port, timeout=timeout)
        self.addCleanup(client.close)
        return client

    def get(self, path: str, token: Optional[str] = None) -> http.client.HTTPResponse:
        conn = self.http_connection()
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        conn.request("GET", path, headers=headers)
        return conn.getresponse()

    def put(self, path: str, body: dict, token: Optional[str], extra_headers: Optional[Dict[str, str]] = None) -> http.client.HTTPResponse:
        conn = self.http_connection()
        headers = dict(extra_headers or {})
        if token:
            headers["Authorization"] = f"Bearer {token}"
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
        conn.request("PUT", path, body=data, headers=headers)
        return conn.getresponse()

    def delete(self, path: str, token: Optional[str], extra_headers: Optional[Dict[str, str]] = None) -> http.client.HTTPResponse:
        conn = self.http_connection()
        headers = dict(extra_headers or {})
        if token:
            headers["Authorization"] = f"Bearer {token}"
        conn.request("DELETE", path, headers=headers)
        return conn.getresponse()


# --------------------------------------------------------------------- #
# Bind validation (SPEC SS8)
# --------------------------------------------------------------------- #


class TestBindValidation(HermeticCase):
    def test_loopback_allowed(self) -> None:
        keel_server.validate_bind_host("127.0.0.1", allow_any_bind=False)
        keel_server.validate_bind_host("localhost", allow_any_bind=False)

    def test_tailscale_cgnat_allowed(self) -> None:
        keel_server.validate_bind_host("100.64.0.1", allow_any_bind=False)
        keel_server.validate_bind_host("100.127.255.254", allow_any_bind=False)

    def test_arbitrary_public_address_refused(self) -> None:
        with self.assertRaises(ValueError):
            keel_server.validate_bind_host("8.8.8.8", allow_any_bind=False)

    def test_wildcard_bind_refused(self) -> None:
        with self.assertRaises(ValueError):
            keel_server.validate_bind_host("0.0.0.0", allow_any_bind=False)

    def test_non_ip_hostname_refused(self) -> None:
        with self.assertRaises(ValueError):
            keel_server.validate_bind_host("example.com", allow_any_bind=False)

    def test_allow_any_bind_overrides(self) -> None:
        keel_server.validate_bind_host("8.8.8.8", allow_any_bind=True)

    def test_env_var_override(self) -> None:
        previous = os.environ.get("KEEL_ALLOW_PUBLIC_BIND")
        os.environ["KEEL_ALLOW_PUBLIC_BIND"] = "1"
        try:
            keel_server.validate_bind_host("8.8.8.8", allow_any_bind=False)
        finally:
            if previous is None:
                os.environ.pop("KEEL_ALLOW_PUBLIC_BIND", None)
            else:
                os.environ["KEEL_ALLOW_PUBLIC_BIND"] = previous

    def test_server_construction_refuses_bad_bind(self) -> None:
        config = keel_server.ServerConfig(bind_host="8.8.8.8", port=0)
        # events=EventLog(":memory:") explicitly: a rejected bind must
        # never fall through to build_server's real-file default
        # (default_events_db_path(), typically ~/.keel/events.db) as a
        # side effect -- this test asserts that ordering below, and stays
        # hermetic itself regardless.
        events = keel_server.EventLog(":memory:")
        self.addCleanup(events.close)
        with self.assertRaises(ValueError):
            keel_server.build_server(config, store=store_api.InMemoryStore(), events=events)

    def test_rejected_bind_never_touches_the_real_default_events_db(self) -> None:
        # Redirect KEEL_HOME to a guaranteed-empty temp directory rather
        # than asserting on the real ~/.keel/events.db: that file may
        # already exist from a previous (or a prior buggy) run, and
        # EventLog's `CREATE TABLE IF NOT EXISTS` + a no-op commit against
        # an already-initialized file does not reliably bump its mtime --
        # a weak signal that stayed green even with the bug still present.
        # An empty temp dir makes "was the file created at all" the
        # question, which is unambiguous either way.
        previous = os.environ.get("KEEL_HOME")
        with tempfile.TemporaryDirectory() as tmp:
            os.environ["KEEL_HOME"] = tmp
            try:
                target = keel_server.default_events_db_path()
                self.assertFalse(target.exists())
                config = keel_server.ServerConfig(bind_host="8.8.8.8", port=0)
                with self.assertRaises(ValueError):
                    keel_server.build_server(config, store=store_api.InMemoryStore())
                self.assertFalse(target.exists(), f"build_server created {target} for a rejected bind")
            finally:
                if previous is None:
                    os.environ.pop("KEEL_HOME", None)
                else:
                    os.environ["KEEL_HOME"] = previous


# --------------------------------------------------------------------- #
# TokenStore
# --------------------------------------------------------------------- #


class TestTokenStore(HermeticCase):
    def test_known_token_authenticates(self) -> None:
        store = _default_token_store()
        principal = store.authenticate(RUNNER_TOKEN)
        self.assertIsNotNone(principal)
        self.assertEqual(principal.role, "runner")
        self.assertEqual(principal.id, "runner-1")

    def test_unknown_token_rejected(self) -> None:
        store = _default_token_store()
        self.assertIsNone(store.authenticate("not-a-real-token"))

    def test_empty_store_rejects_everything(self) -> None:
        store = keel_server.TokenStore.empty()
        self.assertIsNone(store.authenticate(RUNNER_TOKEN))

    def test_from_file_round_trips(self) -> None:

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "auth.json"
            path.write_text(json.dumps({"tokens": [{"id": "x", "role": "operator", "sha256": _sha256_hex("secret")}]}))
            store = keel_server.TokenStore.from_file(path)
            principal = store.authenticate("secret")
            self.assertIsNotNone(principal)
            self.assertEqual(principal.role, "operator")


# --------------------------------------------------------------------- #
# Router
# --------------------------------------------------------------------- #


class TestRouter(HermeticCase):
    def test_path_param_matches_single_segment(self) -> None:
        pattern = keel_server.compile_path("/v1/jobs/{id}")
        m = pattern.match("/v1/jobs/abc123")
        self.assertIsNotNone(m)
        self.assertEqual(m.group("id"), "abc123")
        self.assertIsNone(pattern.match("/v1/jobs/abc/def"))

    def test_path_param_greedy_matches_slashes(self) -> None:
        pattern = keel_server.compile_path("/v1/refs/{ref:path}")
        m = pattern.match("/v1/refs/refs/fleet/claims/gate/abc")
        self.assertIsNotNone(m)
        self.assertEqual(m.group("ref"), "refs/fleet/claims/gate/abc")

    def test_match_distinguishes_404_from_405(self) -> None:
        router = keel_server.Router()
        router.add("GET", "/v1/thing", lambda *a: None, auth=None)
        route, params, path_exists = router.match("GET", "/v1/other")
        self.assertIsNone(route)
        self.assertFalse(path_exists)
        route, params, path_exists = router.match("POST", "/v1/thing")
        self.assertIsNone(route)
        self.assertTrue(path_exists)
        route, params, path_exists = router.match("GET", "/v1/thing")
        self.assertIsNotNone(route)
        self.assertTrue(path_exists)


# --------------------------------------------------------------------- #
# EventLog
# --------------------------------------------------------------------- #


class TestEventLog(HermeticCase):
    def test_since_returns_in_order_after_seq(self) -> None:
        log = keel_server.EventLog(":memory:")
        self.addCleanup(log.close)
        seqs = [log.append("kind.a", {"n": i}) for i in range(5)]
        self.assertEqual(seqs, sorted(seqs))
        events = log.since(seqs[1])
        self.assertEqual([e.seq for e in events], seqs[2:])
        self.assertEqual([e.payload["n"] for e in events], [2, 3, 4])

    def test_since_zero_returns_everything(self) -> None:
        log = keel_server.EventLog(":memory:")
        self.addCleanup(log.close)
        log.append("a", {})
        log.append("b", {})
        self.assertEqual(len(log.since(0)), 2)

    def test_ring_capacity_prunes_oldest(self) -> None:
        log = keel_server.EventLog(":memory:", ring_capacity=3)
        self.addCleanup(log.close)
        for i in range(10):
            log.append("kind", {"n": i})
        remaining = log.since(0)
        self.assertEqual(len(remaining), 3)
        self.assertEqual([e.payload["n"] for e in remaining], [7, 8, 9])

    def test_wait_for_new_wakes_on_append(self) -> None:
        log = keel_server.EventLog(":memory:")
        self.addCleanup(log.close)
        baseline = log.latest_seq()
        result: Dict[str, object] = {}

        def waiter() -> None:
            result["woke"] = log.wait_for_new(baseline, timeout=5.0)

        t = threading.Thread(target=waiter)
        t.start()
        time.sleep(0.1)
        log.append("kind", {})
        t.join(timeout=5)
        self.assertFalse(t.is_alive())
        self.assertTrue(result.get("woke"))

    def test_wait_for_new_times_out_with_nothing_new(self) -> None:
        log = keel_server.EventLog(":memory:")
        self.addCleanup(log.close)
        start = time.monotonic()
        woke = log.wait_for_new(log.latest_seq(), timeout=0.2)
        elapsed = time.monotonic() - start
        self.assertFalse(woke)
        self.assertGreaterEqual(elapsed, 0.2)


# --------------------------------------------------------------------- #
# /v1/health
# --------------------------------------------------------------------- #


class TestHealth(ServerTestCase):
    def test_health_is_public_and_well_formed(self) -> None:
        resp = self.get("/v1/health")
        self.assertEqual(resp.status, 200)
        body = json.loads(resp.read())
        self.assertIn("boot_id", body)
        self.assertIn("connections", body)
        self.assertIn("watchdog", body)

    def test_health_reflects_injected_health_provider(self) -> None:
        self.server.health_provider = lambda: {"github_ok": True, "settle_until": 123.0}
        resp = self.get("/v1/health")
        body = json.loads(resp.read())
        self.assertEqual(body["github_ok"], True)
        self.assertEqual(body["settle_until"], 123.0)


# --------------------------------------------------------------------- #
# Keep-alive reuse
# --------------------------------------------------------------------- #


class TestKeepAliveReuse(ServerTestCase):
    def test_two_requests_share_one_tcp_connection(self) -> None:
        conn = self.http_connection()
        conn.request("GET", "/v1/health")
        r1 = conn.getresponse()
        self.assertEqual(r1.status, 200)
        r1.read()
        self.assertNotEqual((r1.getheader("Connection") or "").lower(), "close")

        # If the server had closed the socket after r1, this second request
        # on the SAME http.client.HTTPConnection object would raise
        # (BrokenPipeError / ConnectionResetError / "Connection aborted")
        # instead of completing.
        conn.request("GET", "/v1/health")
        r2 = conn.getresponse()
        self.assertEqual(r2.status, 200)
        r2.read()

        # And the server's own accounting agrees: one TCP connection
        # accepted total, even though two requests were served.
        self.assertEqual(self.server.total_connections_accepted, 1)

    def test_connection_close_header_is_honoured(self) -> None:
        conn = self.http_connection()
        conn.request("GET", "/v1/health", headers={"Connection": "close"})
        resp = conn.getresponse()
        self.assertEqual(resp.status, 200)
        resp.read()
        # A second request on the same (now server-closed) connection must fail.
        with self.assertRaises((BrokenPipeError, ConnectionResetError, http.client.BadStatusLine, OSError)):
            conn.request("GET", "/v1/health")
            conn.getresponse().read()


# --------------------------------------------------------------------- #
# Connection cap -> 503
# --------------------------------------------------------------------- #


class TestConnectionCap(ServerTestCase):
    max_connections = 1

    def test_second_connection_gets_503_then_recovers(self) -> None:
        # Connection #1: just open it (no request yet) -- setup() has
        # already claimed the one slot the moment the TCP handshake lands.
        holder = socket.create_connection((self.host, self.port), timeout=5)
        self.addCleanup(holder.close)
        deadline = time.monotonic() + 2.0
        while self.server.active_connection_count() < 1 and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(self.server.active_connection_count(), 1)

        # Connection #2: over the cap -- must get a real, well-formed 503.
        rejected = self.raw_client()
        rejected.send_request("GET", "/v1/health")
        status, headers = rejected.read_status_and_headers()
        self.assertEqual(status, 503)
        self.assertEqual(headers.get("connection"), "close")

        # Release the held slot; a fresh connection now succeeds.
        holder.close()
        deadline = time.monotonic() + 2.0
        while self.server.active_connection_count() > 0 and time.monotonic() < deadline:
            time.sleep(0.01)
        resp = self.get("/v1/health")
        self.assertEqual(resp.status, 200)


# --------------------------------------------------------------------- #
# Auth matrix (401 vs 403)
# --------------------------------------------------------------------- #


class TestAuthMatrix(ServerTestCase):
    def test_health_needs_no_token(self) -> None:
        self.assertEqual(self.get("/v1/health").status, 200)

    def test_missing_authorization_header_is_401(self) -> None:
        resp = self.get("/v1/refs/refs/fleet/test/x")
        self.assertEqual(resp.status, 401)
        self.assertEqual(resp.getheader("WWW-Authenticate"), "Bearer")

    def test_malformed_scheme_is_401(self) -> None:
        conn = self.http_connection()
        conn.request("GET", "/v1/refs/refs/fleet/test/x", headers={"Authorization": f"Basic {RUNNER_TOKEN}"})
        resp = conn.getresponse()
        self.assertEqual(resp.status, 401)

    def test_unknown_token_is_401(self) -> None:
        resp = self.get("/v1/refs/refs/fleet/test/x", token="totally-unknown-token")
        self.assertEqual(resp.status, 401)

    def test_any_known_role_may_read_refs(self) -> None:
        for token in (RUNNER_TOKEN, OPERATOR_TOKEN, AGENT_TOKEN):
            with self.subTest(token=token[:6]):
                resp = self.get("/v1/refs/refs/fleet/test/x", token=token)
                self.assertEqual(resp.status, 404)  # authenticated, just absent

    def test_agent_role_forbidden_from_writing_refs(self) -> None:
        resp = self.put("/v1/refs/refs/fleet/test/x", {"a": 1}, token=AGENT_TOKEN, extra_headers={"If-None-Match": "*"})
        self.assertEqual(resp.status, 403)
        resp.read()

    def test_runner_and_operator_may_write_refs(self) -> None:
        for token in (RUNNER_TOKEN, OPERATOR_TOKEN):
            with self.subTest(token=token[:6]):
                resp = self.put(f"/v1/refs/refs/fleet/test/{token[:6]}", {"a": 1}, token=token, extra_headers={"If-None-Match": "*"})
                self.assertEqual(resp.status, 201)
                resp.read()

    def test_tokens_never_appear_in_logs(self) -> None:
        secret = "canary-token-" + uuid.uuid4().hex
        logger = logging.getLogger()
        captured: List[str] = []

        class _Capture(logging.Handler):
            def emit(self, record: logging.LogRecord) -> None:
                captured.append(record.getMessage())

        handler = _Capture()
        logger.addHandler(handler)
        previous_level = logger.level
        logger.setLevel(logging.DEBUG)
        try:
            # A request with an Authorization header at all (valid or not)
            # must never put that header's value into a log record.
            conn = self.http_connection()
            conn.request("GET", "/v1/refs/refs/fleet/test/x", headers={"Authorization": f"Bearer {secret}"})
            conn.getresponse().read()
        finally:
            logger.removeHandler(handler)
            logger.setLevel(previous_level)
        for message in captured:
            self.assertNotIn(secret, message)


# --------------------------------------------------------------------- #
# /v1/refs routing (the transport-layer glue over Store; CAS SEMANTICS
# themselves are InMemoryStore's/CachedHub's job, not tested here beyond
# "does the route call the right Store method and map the result")
# --------------------------------------------------------------------- #


class TestRefsRouting(ServerTestCase):
    def test_create_read_conflict_update_delete_round_trip(self) -> None:
        ref = "refs/fleet/test/roundtrip"

        # absent -> 404
        resp = self.get(f"/v1/refs/{ref}", token=OPERATOR_TOKEN)
        self.assertEqual(resp.status, 404)
        resp.read()

        # create
        resp = self.put(f"/v1/refs/{ref}", {"v": 1}, token=OPERATOR_TOKEN, extra_headers={"If-None-Match": "*"})
        self.assertEqual(resp.status, 201)
        sha1 = json.loads(resp.read())["sha"]

        # create again -> conflict (already exists)
        resp = self.put(f"/v1/refs/{ref}", {"v": 1}, token=OPERATOR_TOKEN, extra_headers={"If-None-Match": "*"})
        self.assertEqual(resp.status, 409)
        resp.read()

        # read
        resp = self.get(f"/v1/refs/{ref}", token=RUNNER_TOKEN)
        self.assertEqual(resp.status, 200)
        body = json.loads(resp.read())
        self.assertEqual(body["sha"], sha1)
        self.assertEqual(body["payload"], {"v": 1})

        # update with stale sha -> conflict
        resp = self.put(f"/v1/refs/{ref}", {"v": 2}, token=OPERATOR_TOKEN, extra_headers={"If-Match": "not-the-real-sha"})
        self.assertEqual(resp.status, 409)
        resp.read()

        # update with correct sha -> 200, new sha
        resp = self.put(f"/v1/refs/{ref}", {"v": 2}, token=OPERATOR_TOKEN, extra_headers={"If-Match": sha1})
        self.assertEqual(resp.status, 200)
        sha2 = json.loads(resp.read())["sha"]
        self.assertNotEqual(sha1, sha2)

        # list by prefix
        resp = self.get("/v1/refs?prefix=refs/fleet/test", token=RUNNER_TOKEN)
        self.assertEqual(resp.status, 200)
        listing = json.loads(resp.read())
        self.assertIn(ref, listing)
        self.assertEqual(listing[ref]["sha"], sha2)

        # delete with stale sha -> conflict
        resp = self.delete(f"/v1/refs/{ref}", token=OPERATOR_TOKEN, extra_headers={"If-Match": sha1})
        self.assertEqual(resp.status, 409)
        resp.read()

        # delete with correct sha -> 204
        resp = self.delete(f"/v1/refs/{ref}", token=OPERATOR_TOKEN, extra_headers={"If-Match": sha2})
        self.assertEqual(resp.status, 204)

        # gone
        resp = self.get(f"/v1/refs/{ref}", token=RUNNER_TOKEN)
        self.assertEqual(resp.status, 404)
        resp.read()

    def test_put_without_precondition_header_is_400(self) -> None:
        resp = self.put("/v1/refs/refs/fleet/test/no-precondition", {"v": 1}, token=OPERATOR_TOKEN)
        self.assertEqual(resp.status, 400)

    def test_unknown_route_is_404(self) -> None:
        resp = self.get("/v1/not-a-real-route")
        self.assertEqual(resp.status, 404)

    def test_wrong_method_on_known_path_is_405(self) -> None:
        conn = self.http_connection()
        conn.request("POST", "/v1/health")
        resp = conn.getresponse()
        self.assertEqual(resp.status, 405)
        resp.read()


# --------------------------------------------------------------------- #
# SSE replay ordering
# --------------------------------------------------------------------- #


class TestSSEReplayOrdering(ServerTestCase):
    sse_keepalive_interval = 0.3

    def _read_events(self, client: _RawHTTPClient, count: int) -> List[Tuple[int, str, dict]]:
        collected: List[Tuple[int, str, dict]] = []
        while len(collected) < count:
            chunk = client.read_chunk()
            self.assertIsNotNone(chunk)
            if chunk == b"" or chunk.startswith(b":"):
                continue  # keepalive comment or terminal chunk; keep waiting
            text = chunk.decode("utf-8")
            seq = None
            kind = None
            data = None
            for line in text.split("\n"):
                if line.startswith("id: "):
                    seq = int(line[len("id: ") :])
                elif line.startswith("event: "):
                    kind = line[len("event: ") :]
                elif line.startswith("data: "):
                    data = json.loads(line[len("data: ") :])
            collected.append((seq, kind, data))
        return collected

    def _auth_headers(self, extra: Optional[Dict[str, str]] = None) -> Dict[str, str]:
        headers = {"Authorization": f"Bearer {RUNNER_TOKEN}"}
        headers.update(extra or {})
        return headers

    def test_replay_from_since_is_in_order(self) -> None:
        seqs = [self.events.append("kind.pre", {"n": i}) for i in range(4)]
        client = self.raw_client()
        client.send_request("GET", f"/v1/events?since={seqs[1]}", headers=self._auth_headers())
        status, headers = client.read_status_and_headers()
        self.assertEqual(status, 200)
        self.assertEqual(headers.get("content-type"), "text/event-stream")
        self.assertEqual(headers.get("transfer-encoding"), "chunked")

        replayed = self._read_events(client, 2)
        self.assertEqual([e[0] for e in replayed], seqs[2:])
        self.assertEqual([e[2]["n"] for e in replayed], [2, 3])

    def test_live_event_after_connect_is_streamed(self) -> None:
        client = self.raw_client()
        client.send_request("GET", "/v1/events?since=0", headers=self._auth_headers())
        client.read_status_and_headers()

        def publish_later() -> None:
            time.sleep(0.1)
            self.events.append("kind.live", {"marker": "hello"})

        threading.Thread(target=publish_later).start()
        events = self._read_events(client, 1)
        self.assertEqual(events[0][1], "kind.live")
        self.assertEqual(events[0][2]["marker"], "hello")

    def test_last_event_id_header_resumes_replay(self) -> None:
        seqs = [self.events.append("kind", {"n": i}) for i in range(3)]
        client = self.raw_client()
        client.send_request("GET", "/v1/events", headers=self._auth_headers({"Last-Event-ID": str(seqs[0])}))
        client.read_status_and_headers()
        replayed = self._read_events(client, 2)
        self.assertEqual([e[0] for e in replayed], seqs[1:])

    def test_idle_stream_keepalive_raw_bytes(self) -> None:
        client = self.raw_client()
        client.send_request("GET", "/v1/events?since=0", headers=self._auth_headers())
        client.read_status_and_headers()
        start = time.monotonic()
        chunk = client.read_chunk()
        elapsed = time.monotonic() - start
        self.assertEqual(chunk, keel_server._SSE_KEEPALIVE_LINE)
        self.assertGreaterEqual(elapsed, self.sse_keepalive_interval * 0.5)


# --------------------------------------------------------------------- #
# /v1/wait long-poll
# --------------------------------------------------------------------- #


class TestLongPollWake(ServerTestCase):
    long_poll_timeout = 5.0
    long_poll_interval = 0.02

    def test_wakes_promptly_on_change(self) -> None:
        ref = "refs/fleet/test/wait-wake"

        def flip_later() -> None:
            time.sleep(0.2)
            self.store.create(ref, {"v": 1})

        threading.Thread(target=flip_later).start()
        start = time.monotonic()
        resp = self.get(f"/v1/wait?ref={ref}&since=", token=RUNNER_TOKEN)
        elapsed = time.monotonic() - start
        self.assertEqual(resp.status, 200)
        body = json.loads(resp.read())
        self.assertEqual(body["ref"], ref)
        self.assertIsNotNone(body["sha"])
        # Woke well before the 5s deadline -- proves it was notified by the
        # change, not merely timed out and happened to see the new value.
        self.assertLess(elapsed, 2.0)

    def test_times_out_with_204_when_nothing_changes(self) -> None:
        ref = "refs/fleet/test/wait-timeout"
        config_timeout = 0.3
        self.server.config = dataclasses.replace(self.server.config, long_poll_timeout=config_timeout)
        start = time.monotonic()
        resp = self.get(f"/v1/wait?ref={ref}&since=", token=RUNNER_TOKEN)
        elapsed = time.monotonic() - start
        self.assertEqual(resp.status, 204)
        self.assertGreaterEqual(elapsed, config_timeout * 0.8)

    def test_missing_ref_param_is_400(self) -> None:
        resp = self.get("/v1/wait", token=RUNNER_TOKEN)
        self.assertEqual(resp.status, 400)


# --------------------------------------------------------------------- #
# Shutdown ordering: stop() must not return while an SSE handler thread
# is still running (it may still be touching self.events).
# --------------------------------------------------------------------- #


class TestStreamShutdownOrdering(ServerTestCase):
    def test_stop_waits_for_sse_handler_thread_to_exit(self) -> None:
        client = self.raw_client()
        client.send_request("GET", "/v1/events?since=0", headers={"Authorization": f"Bearer {RUNNER_TOKEN}"})
        client.read_status_and_headers()

        # Give the handler thread a moment to actually start its loop and
        # register itself, then stop the server. If `stop()` returns
        # before the thread has exited, `_stream_threads` is still
        # non-empty right after the call -- and a caller that then closes
        # a resource the thread is still using (e.g. `self.events`, as
        # `ServerTestCase.tearDown` does next) would race it.
        deadline = time.monotonic() + 2.0
        while not self.server._stream_threads and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertTrue(self.server._stream_threads, "SSE handler never registered itself")

        self.server.stop()

        self.assertEqual(
            self.server._stream_threads,
            set(),
            "stop() returned while an SSE handler thread was still registered/running",
        )
        client.close()


# --------------------------------------------------------------------- #
# Listener watchdog
# --------------------------------------------------------------------- #


class TestWatchdog(ServerTestCase):
    def test_tick_restarts_after_sustained_probe_failure(self) -> None:
        self.server.config = dataclasses.replace(self.server.config, watchdog_timeout=0.2)
        restarts = []
        self.server.restart_accept_loop = lambda: restarts.append(1)

        # Healthy probes: no restart, and the healthy timestamp keeps
        # advancing.
        self.server._watchdog_tick(alive=True)
        self.assertEqual(restarts, [])

        # Force the "last healthy" timestamp into the past so the very
        # next failed-probe tick is already over watchdog_timeout --
        # deterministic, no real sleeping required.
        self.server.last_healthy_probe_at = time.monotonic() - 10.0
        self.server._watchdog_tick(alive=False)
        self.assertEqual(restarts, [1])

    def test_tick_does_not_restart_before_timeout_elapses(self) -> None:
        self.server.config = dataclasses.replace(self.server.config, watchdog_timeout=100.0)
        restarts = []
        self.server.restart_accept_loop = lambda: restarts.append(1)
        self.server._watchdog_tick(alive=False)
        self.assertEqual(restarts, [])

    def test_restart_accept_loop_keeps_the_server_serving(self) -> None:
        resp = self.get("/v1/health")
        self.assertEqual(resp.status, 200)
        old_restart_count = self.server.restart_count

        self.server.restart_accept_loop()

        self.assertEqual(self.server.restart_count, old_restart_count + 1)
        # Same port, real requests still succeed post-restart.
        deadline = time.monotonic() + 3.0
        last_error = None
        while time.monotonic() < deadline:
            try:
                resp = self.get("/v1/health")
                if resp.status == 200:
                    resp.read()
                    return
            except OSError as exc:  # pragma: no cover -- only on a slow restart
                last_error = exc
                time.sleep(0.05)
        self.fail(f"server did not resume serving after restart_accept_loop: {last_error}")


if __name__ == "__main__":
    unittest.main(verbosity=2)
