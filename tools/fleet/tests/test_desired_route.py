#!/usr/bin/env python3
"""`GET|PUT /v1/desired`, the listen backlog, and `ServerHub`'s
CAS-witness guard -- Stage 2 review findings F2, F4 and F5.

WHAT EACH SECTION PINS, AND HOW IT WAS CHECKED TO GO RED.

F2 -- `GET|PUT /v1/desired` (SPEC SS5.1, PLAN Stage 2 deliverables).
`server.py`'s `build_router()` registered no such route; the CAS facade
(`PUT /v1/refs/{ref}`) would have carried the document, but not the ONE
invariant `refs/fleet/desired` has that no other ref does -- `generation`
is a monotonic counter of edits, and a facade PUT leaves every writer to
compute it for itself. `TestDesiredRoute` pins the route's contract
(If-Match required, 412/400 on a missing/malformed witness, 409 on a lost
CAS, generation++ computed from the PRE-IMAGE at the witnessed version and
never from the request body) and `TestConcurrentDesiredPuts` pins the
property the counter exists for: eight writers racing on one witness
produce exactly one landed edit and exactly one increment.
  Checked red: the whole file 404s against `staging/agent-server@00d63c87`
  (no route). Against the implementation, three separate reversions each
  turn a SPECIFIC test red rather than all of them --
  (a) taking `doc["generation"] = _next_generation(cur_payload)` out of
  `handle_desired_put` so the body's own generation survives:
  `test_generation_is_computed_server_side_and_the_body_cannot_set_it`
  fails `99 != 5`;
  (b) dropping the `_normalize_witness` call and defaulting the witness to
  whatever is current (`witness = if_match or cur_sha`):
  `test_put_without_a_witness_is_refused` fails `200 != 412` and five
  more, with the clobber landing;
  (c) making `_next_generation` return `int(raw)` instead of
  `int(raw) + 1`: `test_sequential_edits_advance_the_generation_by_one_each`
  fails `1 != 2` and the eight-way race fails on its increment.

F4 -- the listen backlog. `KeelHTTPServer` inherited `socketserver`'s
`request_queue_size = 5` while `DEFAULT_MAX_CONNECTIONS` is 64 and every
`ServerHub` call opens a FRESH connection (`serverhub.py`'s docstring:
one connection per request buys phase certainty at `connect()`). Past the
backlog the kernel drops or RSTs the SYN, the client sees a connect-time
failure, and `FallbackHub` classifies that as BEFORE-SEND (SPEC SS4.3 r2)
and silently routes around a server that is healthy and reachable.
`test_serverhub.py` had already had to subclass the server to raise the
backlog for its eight-process race (its `_RaceReadyServer`, "7 red runs
in 8"); that was evidence about production.
  Checked red: with the `request_queue_size` class attribute and the
  `server_activate` override deleted (back to `socketserver`'s 5), all
  four tests in `TestListenBacklog` fail -- the two direct ones with
  "5 not greater than or equal to 64", and
  `test_a_burst_of_simultaneous_connects_all_establish` with 26 of 40
  connects raising `ConnectionResetError(54)` (macOS 27, Python 3.14).

F5 -- `ServerHub._require_expect_sha`. The real bug fixed in `ce19d70b`
had no regression test. Without the guard, an `expect_sha=None` reaches
`http.client.putheader` as an `If-Match: None` value, whose `TypeError`
is raised inside phase 2 of `ServerHub._request` and so comes back as
`PrimaryFailure(request_sent=True)` -- an AMBIGUOUS write. `FallbackHub`
then correctly refuses to re-issue it and raises `AmbiguousWriteError`,
`claim._note_renew_failure` tolerates it as a blip, and a caller bug
(reading a stale/absent sha) is laundered into "the network might have
eaten it". `TestRequireExpectSha` pins that a `None`/empty witness raises
`ValueError` before any connection object is even constructed, and
carries its own NEGATIVE CONTROL: with the guard patched out, the same
call is shown producing the ambiguous masquerade instead.
  Checked red: replacing the guard's condition with `if False:` fails 11
  of this class's checks, and
  `test_a_none_witness_raises_before_any_connection_is_opened` fails with
  the `_ExplodingConnection` message -- i.e. the transport really was
  reached, for all three of `update`, `delete` and `put_desired`.

Run with:
    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_desired_route -v
"""

from __future__ import annotations

import hashlib
import http.client
import json
import logging
import socket
import sys
import threading
import unittest
import uuid
from pathlib import Path
from typing import Dict, List, Optional, Tuple
from unittest import mock

FLEET_DIR = Path(__file__).resolve().parents[1]
KEEL_DIR = FLEET_DIR / "keel"
for _p in (FLEET_DIR, KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

# Top-level, exactly as `test_serverhub.py` imports them, so `server`'s
# `except StoreUnreachableError` clause matches the class our doubles
# raise (one module object, not two).
import server as keel_server  # noqa: E402
import store_api  # noqa: E402
from _env import HermeticCase  # noqa: E402
from keel.fallbackhub import AMBIGUOUS, PrimaryFailure, classify_primary_failure  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402

DESIRED_REF = "refs/fleet/desired"

OPERATOR_TOKEN = "operator-token-" + uuid.uuid4().hex
RUNNER_TOKEN = "runner-token-" + uuid.uuid4().hex


def _sha256_hex(raw: str) -> str:
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def _token_store() -> keel_server.TokenStore:
    return keel_server.TokenStore(
        [
            {"id": "operator-1", "role": "operator", "sha256": _sha256_hex(OPERATOR_TOKEN)},
            {"id": "runner-1", "role": "runner", "sha256": _sha256_hex(RUNNER_TOKEN)},
        ]
    )


class _ServerCase(HermeticCase):
    """A real `KeelHTTPServer` on `127.0.0.1:0` (loopback only) over an
    `InMemoryStore`, with an operator and a runner token."""

    def setUp(self) -> None:
        super().setUp()
        logging.disable(logging.CRITICAL)
        self.addCleanup(logging.disable, logging.NOTSET)
        self.store = store_api.InMemoryStore()
        self.base_url = self.start_server(self.store)

    def start_server(self, store, *, max_connections: Optional[int] = None) -> str:
        kw = {} if max_connections is None else {"max_connections": max_connections}
        config = keel_server.ServerConfig(
            bind_host="127.0.0.1",
            port=0,
            watchdog_timeout=100.0,
            watchdog_check_interval=100.0,
            **kw,
        )
        events = keel_server.EventLog(":memory:")
        self.addCleanup(events.close)
        server = keel_server.build_server(config, store=store, tokens=_token_store(), events=events)
        server.start()
        self.addCleanup(server.stop)
        self.server = server
        return f"http://127.0.0.1:{server.server_address[1]}"

    # -- raw HTTP, bypassing ServerHub, so the SERVER's contract is what
    #    is under test rather than the client's decoration of it --------- #

    def request(
        self,
        method: str,
        path: str,
        *,
        body: Optional[object] = None,
        headers: Optional[Dict[str, str]] = None,
        token: str = OPERATOR_TOKEN,
        raw_body: Optional[bytes] = None,
        base_url: Optional[str] = None,
    ) -> Tuple[int, dict, Dict[str, str]]:
        url = base_url or self.base_url
        port = int(url.rsplit(":", 1)[1])
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=15.0)
        try:
            hdrs = {"Connection": "close"}
            if token:
                hdrs["Authorization"] = f"Bearer {token}"
            if headers:
                hdrs.update(headers)
            data = raw_body
            if data is None and body is not None:
                data = json.dumps(body).encode("utf-8")
                hdrs["Content-Type"] = "application/json"
            conn.request(method, path, body=data, headers=hdrs)
            resp = conn.getresponse()
            payload = resp.read()
            parsed = json.loads(payload) if payload else {}
            return resp.status, parsed, {k.lower(): v for k, v in resp.getheaders()}
        finally:
            conn.close()

    # -- fixture helpers -------------------------------------------------- #

    def seed_desired(self, doc: dict) -> str:
        sha = self.store.create(DESIRED_REF, doc)
        assert sha is not False, "seed_desired: refs/fleet/desired already existed"
        return sha


# --------------------------------------------------------------------- #
# F2: the route's contract
# --------------------------------------------------------------------- #


class TestDesiredRoute(_ServerCase):
    def test_get_on_an_absent_desired_is_404(self) -> None:
        status, body, _hdrs = self.request("GET", "/v1/desired")
        self.assertEqual(status, 404)
        self.assertEqual(body["error"], "not-found")

    def test_get_answers_the_stored_document_its_sha_and_an_etag(self) -> None:
        sha = self.seed_desired({"generation": 7, "hosts": {"m5": {"gates": 2}}})
        status, body, hdrs = self.request("GET", "/v1/desired")
        self.assertEqual(status, 200)
        self.assertEqual(body["sha"], sha)
        self.assertEqual(body["ref"], DESIRED_REF)
        self.assertEqual(body["payload"], {"generation": 7, "hosts": {"m5": {"gates": 2}}})
        # The ETag is the witness a client echoes back as `If-Match`.
        self.assertEqual(hdrs["etag"], f'"{sha}"')

    def test_create_with_if_none_match_star_starts_at_generation_1(self) -> None:
        status, body, hdrs = self.request(
            "PUT", "/v1/desired",
            body={"hosts": {"m5": {"gates": 1}}},
            headers={"If-None-Match": "*"},
        )
        self.assertEqual(status, 201)
        self.assertEqual(body["payload"]["generation"], 1)
        self.assertEqual(hdrs["etag"], f'"{body["sha"]}"')
        # The ref on the STORE is what the server wrote -- not a copy the
        # server assembled for the answer and never persisted.
        self.assertEqual(self.store.read(DESIRED_REF), body["payload"])
        self.assertEqual(self.store.sha(DESIRED_REF), body["sha"])

    def test_create_against_an_existing_desired_is_409(self) -> None:
        self.seed_desired({"generation": 3, "hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {}}, headers={"If-None-Match": "*"},
        )
        self.assertEqual(status, 409)
        self.assertEqual(body["error"], "conflict")
        self.assertEqual(self.store.read(DESIRED_REF)["generation"], 3)

    def test_generation_is_computed_server_side_and_the_body_cannot_set_it(self) -> None:
        sha = self.seed_desired({"generation": 4, "hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired",
            body={"generation": 99, "hosts": {"i7": {"gates": 3}}},
            headers={"If-Match": sha},
        )
        self.assertEqual(status, 200)
        self.assertEqual(
            body["payload"]["generation"], 5,
            "generation must come from the PRE-IMAGE at the witnessed version "
            "(4) + 1, never from the request body (99)",
        )
        self.assertEqual(self.store.read(DESIRED_REF), body["payload"])
        self.assertEqual(self.store.read(DESIRED_REF)["hosts"], {"i7": {"gates": 3}})

    def test_a_quoted_etag_is_accepted_as_the_witness(self) -> None:
        sha = self.seed_desired({"generation": 1, "hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {}}, headers={"If-Match": f'"{sha}"'},
        )
        self.assertEqual(status, 200)
        self.assertEqual(body["payload"]["generation"], 2)

    def test_generation_survives_a_document_that_never_carried_one(self) -> None:
        sha = self.seed_desired({"hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {"m5": {}}}, headers={"If-Match": sha},
        )
        self.assertEqual(status, 200)
        self.assertEqual(body["payload"]["generation"], 1)

    def test_put_without_a_witness_is_refused(self) -> None:
        sha = self.seed_desired({"generation": 2, "hosts": {}})
        status, body, _hdrs = self.request("PUT", "/v1/desired", body={"hosts": {"m5": {"gates": 9}}})
        self.assertEqual(status, 412, "a PUT with no witness is the lost update this route refuses")
        self.assertEqual(body["error"], "precondition-required")
        # Refused means REFUSED: nothing moved.
        self.assertEqual(self.store.sha(DESIRED_REF), sha)
        self.assertEqual(self.store.read(DESIRED_REF), {"generation": 2, "hosts": {}})

    def test_a_wildcard_witness_is_a_malformed_witness_not_a_clobber(self) -> None:
        sha = self.seed_desired({"generation": 2, "hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {}}, headers={"If-Match": "*"},
        )
        self.assertEqual(status, 400)
        self.assertEqual(body["error"], "wildcard-witness")
        self.assertEqual(self.store.sha(DESIRED_REF), sha)

    def test_a_weak_validator_is_refused(self) -> None:
        sha = self.seed_desired({"generation": 2, "hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {}}, headers={"If-Match": f'W/"{sha}"'},
        )
        self.assertEqual(status, 400)
        self.assertEqual(body["error"], "weak-witness")
        self.assertEqual(self.store.sha(DESIRED_REF), sha)

    def test_an_empty_witness_is_refused(self) -> None:
        self.seed_desired({"generation": 2, "hosts": {}})
        for value in ('""', "  "):
            with self.subTest(if_match=value):
                status, body, _hdrs = self.request(
                    "PUT", "/v1/desired", body={"hosts": {}}, headers={"If-Match": value},
                )
                self.assertEqual(status, 400)
                self.assertEqual(body["error"], "malformed-witness")

    def test_both_preconditions_at_once_is_refused(self) -> None:
        sha = self.seed_desired({"generation": 2, "hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {}},
            headers={"If-Match": sha, "If-None-Match": "*"},
        )
        self.assertEqual(status, 400)
        self.assertEqual(body["error"], "conflicting-preconditions")
        self.assertEqual(self.store.sha(DESIRED_REF), sha)

    def test_a_stale_witness_is_409_and_leaves_the_document_alone(self) -> None:
        first = self.seed_desired({"generation": 1, "hosts": {}})
        second = self.store.update(DESIRED_REF, {"generation": 2, "hosts": {"m5": {}}}, first)
        self.assertNotEqual(first, second)
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {"clobber": {}}}, headers={"If-Match": first},
        )
        self.assertEqual(status, 409)
        self.assertEqual(body["error"], "conflict")
        self.assertEqual(body["sha"], second, "the answer names the version the client should re-read")
        self.assertEqual(self.store.read(DESIRED_REF), {"generation": 2, "hosts": {"m5": {}}})

    def test_a_witness_for_an_absent_desired_is_409(self) -> None:
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {}}, headers={"If-Match": "0" * 40},
        )
        self.assertEqual(status, 409)
        self.assertIsNone(self.store.sha(DESIRED_REF))

    def test_an_unparsable_body_is_400_and_writes_nothing(self) -> None:
        sha = self.seed_desired({"generation": 1, "hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", raw_body=b"{not json", headers={"If-Match": sha, "Content-Type": "application/json"},
        )
        self.assertEqual(status, 400)
        self.assertEqual(body["error"], "invalid-json")
        self.assertEqual(self.store.sha(DESIRED_REF), sha)

    def test_an_empty_body_is_refused_rather_than_wiping_the_document(self) -> None:
        """`_read_json_body` cannot tell "no body" from an explicit `{}`,
        and the first would otherwise wipe every host and bump the
        generation while doing it."""
        sha = self.seed_desired({"generation": 1, "hosts": {"m5": {"gates": 2}}})
        status, body, _hdrs = self.request("PUT", "/v1/desired", headers={"If-Match": sha})
        self.assertEqual(status, 400)
        self.assertEqual(body["error"], "empty-body")
        self.assertEqual(self.store.read(DESIRED_REF), {"generation": 1, "hosts": {"m5": {"gates": 2}}})
        # ... and an operator who really means "empty" still can.
        status, body, _hdrs = self.request("PUT", "/v1/desired", raw_body=b"{}", headers={"If-Match": sha})
        self.assertEqual(status, 200)
        self.assertEqual(body["payload"], {"generation": 2})

    def test_a_non_object_body_is_400(self) -> None:
        sha = self.seed_desired({"generation": 1, "hosts": {}})
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", raw_body=b"[1, 2, 3]",
            headers={"If-Match": sha, "Content-Type": "application/json"},
        )
        self.assertEqual(status, 400)
        self.assertEqual(body["error"], "desired-must-be-an-object")
        self.assertEqual(self.store.sha(DESIRED_REF), sha)

    def test_a_runner_may_read_desired_but_not_write_it(self) -> None:
        sha = self.seed_desired({"generation": 1, "hosts": {}})
        status, _body, _hdrs = self.request("GET", "/v1/desired", token=RUNNER_TOKEN)
        self.assertEqual(status, 200, "a runner needs its own targets")
        status, body, _hdrs = self.request(
            "PUT", "/v1/desired", body={"hosts": {}}, headers={"If-Match": sha}, token=RUNNER_TOKEN,
        )
        self.assertEqual(status, 403, "SPEC SS3.1 names the writers: keel up/down/drain + OPERATOR")
        self.assertEqual(body["error"], "forbidden-role")
        self.assertEqual(self.store.sha(DESIRED_REF), sha)

    def test_an_unauthenticated_request_is_401_on_both_verbs(self) -> None:
        self.seed_desired({"generation": 1, "hosts": {}})
        for method, headers in (("GET", None), ("PUT", {"If-Match": "x" * 40})):
            with self.subTest(method=method):
                status, _body, _hdrs = self.request(
                    method, "/v1/desired", body={} if method == "PUT" else None,
                    headers=headers, token="",
                )
                self.assertEqual(status, 401)

    def test_an_unsupported_method_is_405_not_404(self) -> None:
        status, body, _hdrs = self.request("POST", "/v1/desired", body={})
        self.assertEqual(status, 405)
        self.assertEqual(body["error"], "method-not-allowed")


# --------------------------------------------------------------------- #
# F2: exactly one winner, exactly one increment
# --------------------------------------------------------------------- #

RACER_COUNT = 8


class TestConcurrentDesiredPuts(_ServerCase):
    """THREADS, not a `ProcessPool`, and deliberately so. Each racer opens
    its own TCP connection to the real server, which is the whole race
    (the arbitration happens in the store behind the HTTP handler, not in
    the client), and this suite has a standing hazard around process
    pools: a `multiprocessing` start method chosen at import time by one
    module changes it for every other module in the same `unittest`
    invocation. Nothing here needs a separate address space, so nothing
    here takes that risk.
    """

    def test_eight_concurrent_puts_yield_one_winner_and_one_increment(self) -> None:
        start_sha = self.seed_desired({"generation": 10, "hosts": {}})
        barrier = threading.Barrier(RACER_COUNT, timeout=60)
        results: List[Tuple[int, int, dict]] = []
        results_lock = threading.Lock()

        def racer(i: int) -> None:
            barrier.wait()  # all eight release together: a real race
            status, body, _hdrs = self.request(
                "PUT", "/v1/desired",
                body={"hosts": {f"host-{i}": {"gates": i}}},
                headers={"If-Match": start_sha},
            )
            with results_lock:
                results.append((i, status, body))

        threads = [threading.Thread(target=racer, args=(i,), name=f"racer-{i}") for i in range(RACER_COUNT)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=60)
            self.assertFalse(t.is_alive(), f"{t.name} did not finish")

        self.assertEqual(len(results), RACER_COUNT)
        winners = [(i, body) for i, status, body in results if status == 200]
        losers = [(i, status) for i, status, _body in results if status != 200]
        self.assertEqual(
            len(winners), 1,
            f"exactly one racer may win the CAS; winners={[i for i, _ in winners]} losers={losers}",
        )
        self.assertTrue(
            all(status == 409 for _i, status in losers),
            f"every loser must see 409 (a lost race), never an error: {losers}",
        )

        winner_index, winner_body = winners[0]
        self.assertEqual(
            winner_body["payload"]["generation"], 11,
            "eight concurrent edits, one landed edit: the generation advances by exactly 1",
        )
        # The desired ref on the STORE is what the server wrote -- the
        # winner's document, whole, with no loser's payload smeared into
        # it and no last-writer-wins overwrite behind the 409s.
        self.assertEqual(self.store.sha(DESIRED_REF), winner_body["sha"])
        self.assertNotEqual(self.store.sha(DESIRED_REF), start_sha)
        self.assertEqual(
            self.store.read(DESIRED_REF),
            {"generation": 11, "hosts": {f"host-{winner_index}": {"gates": winner_index}}},
        )

    def test_sequential_edits_advance_the_generation_by_one_each(self) -> None:
        """The deterministic companion to the race: no timing at all, so a
        machine too slow to make eight threads overlap still pins the
        arithmetic."""
        sha: Optional[str] = None
        for expected_generation in (1, 2, 3, 4):
            status, body, _hdrs = self.request(
                "PUT", "/v1/desired",
                body={"hosts": {"m5": {"gates": expected_generation}}},
                headers={"If-None-Match": "*"} if sha is None else {"If-Match": sha},
            )
            self.assertEqual(status, 201 if expected_generation == 1 else 200)
            self.assertEqual(body["payload"]["generation"], expected_generation)
            sha = body["sha"]
        self.assertEqual(self.store.read(DESIRED_REF)["generation"], 4)


# --------------------------------------------------------------------- #
# F2: the ServerHub client half
# --------------------------------------------------------------------- #


class TestServerHubDesired(_ServerCase):
    def hub(self, token: str = OPERATOR_TOKEN) -> ServerHub:
        return ServerHub(self.base_url, token=token)

    def test_read_desired_on_an_absent_ref_is_none_none(self) -> None:
        self.assertEqual(self.hub().read_desired(), (None, None))

    def test_put_desired_with_a_none_witness_creates_and_read_desired_round_trips(self) -> None:
        h = self.hub()
        landed = h.put_desired({"hosts": {"m5": {"gates": 2}}}, None)
        self.assertEqual(landed["generation"], 1)
        sha, doc = h.read_desired()
        self.assertEqual(doc, landed)
        self.assertEqual(sha, self.store.sha(DESIRED_REF))

    def test_put_desired_bumps_the_generation_and_returns_the_stored_document(self) -> None:
        h = self.hub()
        h.put_desired({"hosts": {}}, None)
        sha, doc = h.read_desired()
        landed = h.put_desired({"generation": 1234, "hosts": {"i7": {"agents": 1}}}, sha)
        self.assertEqual(landed["generation"], doc["generation"] + 1)
        self.assertEqual(landed["hosts"], {"i7": {"agents": 1}})
        self.assertEqual(self.store.read(DESIRED_REF), landed)

    def test_a_lost_cas_is_none_never_an_exception(self) -> None:
        h = self.hub()
        h.put_desired({"hosts": {}}, None)
        stale_sha, _doc = h.read_desired()
        h.put_desired({"hosts": {"someone-else": {}}}, stale_sha)  # moves the ref
        self.assertIsNone(
            h.put_desired({"hosts": {"us": {}}}, stale_sha),
            "a lost CAS is False/None on this contract, never a raise",
        )

    def test_a_bad_witness_is_a_caller_bug_not_a_lost_race(self) -> None:
        """`put_desired("")` is refused locally, before any connection
        (F5's guard); a witness the SERVER rejects comes back as a raise
        carrying the status, never as `None`. Conflating either with a
        lost CAS would make `edit_desired` retry a caller bug five times
        over and then report it as contention."""
        h = self.hub()
        h.put_desired({"hosts": {}}, None)
        with self.assertRaises(ValueError):
            h.put_desired({"hosts": {}}, "")
        with self.assertRaises(PrimaryFailure) as ctx:
            h.put_desired({"hosts": {}}, "*")
        self.assertEqual(ctx.exception.status, 400)
        self.assertEqual(self.store.read(DESIRED_REF)["generation"], 1)

    def test_a_runner_token_raises_rather_than_reporting_a_lost_race(self) -> None:
        h_op = self.hub()
        h_op.put_desired({"hosts": {}}, None)
        sha, _doc = h_op.read_desired()
        with self.assertRaises(PrimaryFailure) as ctx:
            self.hub(RUNNER_TOKEN).put_desired({"hosts": {"nope": {}}}, sha)
        self.assertEqual(ctx.exception.status, 403)
        # 403 is fail-closed AMBIGUOUS by `serverhub.py`'s documented rule,
        # so a FallbackHub never turns an auth rejection into a second
        # write against GitHub.
        self.assertEqual(classify_primary_failure(ctx.exception), AMBIGUOUS)


# --------------------------------------------------------------------- #
# F4: the listen backlog
# --------------------------------------------------------------------- #

BURST_CONNECTS = 40
BURST_CONNECT_TIMEOUT_S = 3.0


class TestListenBacklog(HermeticCase):
    """The backlog is `listen(2)`'s argument -- how many fully-established
    connections may sit unaccepted -- and is a different quantity from the
    connection cap, which bounds how many this process SERVES at once.
    Leaving it at `socketserver`'s 5 caps the server far below its own cap
    exactly when a burst arrives."""

    def setUp(self) -> None:
        super().setUp()
        logging.disable(logging.CRITICAL)
        self.addCleanup(logging.disable, logging.NOTSET)

    def bound_server(self, *, max_connections: int) -> keel_server.KeelHTTPServer:
        """A server that is BOUND AND LISTENING but whose accept loop is
        never started -- so every connection in the burst below stays in
        the kernel's completed-connection queue and the backlog is the
        only thing that decides whether it establishes.

        Deliberately not `server.start()` + `server.stop()`: `stop()` calls
        `BaseServer.shutdown()`, which waits on an event only
        `serve_forever()` ever sets, so it would hang forever on a server
        that never served. `server_close()` is the whole teardown here.
        """
        config = keel_server.ServerConfig(
            bind_host="127.0.0.1", port=0, max_connections=max_connections,
            watchdog_timeout=100.0, watchdog_check_interval=100.0,
        )
        events = keel_server.EventLog(":memory:")
        self.addCleanup(events.close)
        server = keel_server.build_server(
            config, store=store_api.InMemoryStore(), tokens=_token_store(), events=events,
        )
        self.addCleanup(server.server_close)
        return server

    def test_the_listen_backlog_is_at_least_the_connection_cap(self) -> None:
        server = self.bound_server(max_connections=keel_server.DEFAULT_MAX_CONNECTIONS)
        self.assertGreaterEqual(
            server.request_queue_size, keel_server.DEFAULT_MAX_CONNECTIONS,
            "a server that will SERVE 64 connections must be willing to have 64 "
            "waiting to be accepted; socketserver's default of 5 is not that",
        )

    def test_a_configured_cap_above_the_default_raises_the_backlog_too(self) -> None:
        server = self.bound_server(max_connections=keel_server.DEFAULT_MAX_CONNECTIONS + 24)
        self.assertGreaterEqual(server.request_queue_size, keel_server.DEFAULT_MAX_CONNECTIONS + 24)

    def test_the_backlog_survives_an_accept_loop_restart(self) -> None:
        """`restart_accept_loop()` re-binds and re-`server_activate()`s;
        the watchdog's cure must not quietly halve the server's capacity."""
        config = keel_server.ServerConfig(
            bind_host="127.0.0.1", port=0, watchdog_timeout=100.0, watchdog_check_interval=100.0,
        )
        events = keel_server.EventLog(":memory:")
        self.addCleanup(events.close)
        server = keel_server.build_server(
            config, store=store_api.InMemoryStore(), tokens=_token_store(), events=events,
        )
        server.start()
        self.addCleanup(server.stop)
        before = server.request_queue_size
        server.restart_accept_loop()
        self.assertEqual(server.restart_count, 1)
        self.assertEqual(server.request_queue_size, before)
        self.assertGreaterEqual(server.request_queue_size, keel_server.DEFAULT_MAX_CONNECTIONS)

    def test_a_burst_of_simultaneous_connects_all_establish(self) -> None:
        """`BURST_CONNECTS` sockets connecting at once at a server that is
        not accepting. Every one must establish, because the backlog is
        deeper than the burst. With `socketserver`'s 5, the kernel drops
        (BSD) or resets (some Linux tunings) the excess SYNs and most of
        these raise `socket.timeout`/`ConnectionRefusedError` -- which is
        a BEFORE-SEND failure to `FallbackHub`, i.e. a healthy server
        silently routed around (SPEC SS4.3 r2).
        """
        server = self.bound_server(max_connections=keel_server.DEFAULT_MAX_CONNECTIONS)
        port = server.server_address[1]
        outcomes: List[Optional[BaseException]] = [None] * BURST_CONNECTS
        socks: List[Optional[socket.socket]] = [None] * BURST_CONNECTS
        barrier = threading.Barrier(BURST_CONNECTS, timeout=60)

        def connector(i: int) -> None:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(BURST_CONNECT_TIMEOUT_S)
            socks[i] = s
            try:
                barrier.wait()
                s.connect(("127.0.0.1", port))
            except BaseException as exc:  # noqa: BLE001 -- the outcome IS the assertion
                outcomes[i] = exc

        threads = [threading.Thread(target=connector, args=(i,), daemon=True) for i in range(BURST_CONNECTS)]
        try:
            for t in threads:
                t.start()
            for t in threads:
                t.join(timeout=60)
        finally:
            for s in socks:
                if s is not None:
                    s.close()

        failures = [(i, repr(exc)) for i, exc in enumerate(outcomes) if exc is not None]
        self.assertEqual(
            failures, [],
            f"{len(failures)} of {BURST_CONNECTS} simultaneous connects failed; the listen "
            f"backlog ({server.request_queue_size}) is not absorbing the burst",
        )


# --------------------------------------------------------------------- #
# F5: `ServerHub._require_expect_sha`, with its negative control
# --------------------------------------------------------------------- #


class _ExplodingConnection:
    """Stands in for `http.client.HTTPConnection`. Constructing one at all
    is the failure this class exists to report: the guard under test must
    refuse a bad witness BEFORE any connection object exists."""

    def __init__(self, *args, **kwargs):
        raise AssertionError(
            "a connection was opened for a write with no witness -- the "
            "guard did not run before the transport (F5)"
        )


class TestRequireExpectSha(_ServerCase):
    """`ce19d70b` fixed this in `serverhub.py` and shipped no test."""

    def setUp(self) -> None:
        super().setUp()
        self.seeded_sha = self.seed_desired({"generation": 1, "hosts": {}})
        self.h = ServerHub(self.base_url, token=OPERATOR_TOKEN)

    def test_a_none_witness_raises_before_any_connection_is_opened(self) -> None:
        for op, call in (
            ("update", lambda: self.h.update("refs/fleet/claims/gate/x", {"a": 1}, None)),
            ("delete", lambda: self.h.delete("refs/fleet/claims/gate/x", None)),
            ("put_desired", lambda: self.h.put_desired({"hosts": {}}, "")),
        ):
            with self.subTest(op=op):
                with mock.patch("http.client.HTTPConnection", _ExplodingConnection), \
                     mock.patch("http.client.HTTPSConnection", _ExplodingConnection):
                    with self.assertRaises(ValueError) as ctx:
                        call()
                self.assertIn("no request was sent", str(ctx.exception))

    def test_an_empty_whitespace_or_non_string_witness_is_refused_too(self) -> None:
        # `"  "` and `"a b"` matter beyond tidiness: a witness with
        # whitespace in it is not a sha, and putting one in a header value
        # is how a CR/LF would get there.
        for witness in ("", "   ", "abc def", "abc\r\nX-Evil: 1", 0, b"a" * 40, 1.5):
            with self.subTest(witness=repr(witness)):
                with self.assertRaises(ValueError):
                    self.h.update("refs/fleet/claims/gate/x", {"a": 1}, witness)

    def test_the_store_is_untouched_by_a_refused_write(self) -> None:
        with self.assertRaises(ValueError):
            self.h.update(DESIRED_REF, {"hosts": {"clobber": {}}}, None)
        self.assertEqual(self.store.sha(DESIRED_REF), self.seeded_sha)
        self.assertEqual(self.store.read(DESIRED_REF), {"generation": 1, "hosts": {}})

    def test_negative_control_without_the_guard_a_none_witness_masquerades_as_ambiguous(self) -> None:
        """Remove the guard and watch this go the other way. `None` reaches
        `http.client.putheader` as an `If-Match` value, its `TypeError` is
        raised inside `_request`'s phase 2, and the caller bug comes back
        dressed as a write that MIGHT have landed -- which
        `claim._note_renew_failure` then tolerates as a network blip. That
        is what `ce19d70b` fixed and this class pins.
        """
        with mock.patch.object(ServerHub, "_require_expect_sha", staticmethod(lambda *a, **k: None)):
            with self.assertRaises(PrimaryFailure) as ctx:
                self.h.update(DESIRED_REF, {"hosts": {"clobber": {}}}, None)
        self.assertTrue(
            ctx.exception.request_sent,
            "the unguarded failure is reported as request_sent=True -- the "
            "AMBIGUOUS masquerade the guard exists to remove",
        )
        self.assertEqual(classify_primary_failure(ctx.exception), AMBIGUOUS)
        # And the store is untouched either way, which is exactly why the
        # ambiguity was a lie: nothing was ever sent.
        self.assertEqual(self.store.sha(DESIRED_REF), self.seeded_sha)


if __name__ == "__main__":
    unittest.main()
