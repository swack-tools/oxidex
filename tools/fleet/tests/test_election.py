#!/usr/bin/env python3
"""Tests for tools/fleet/keel/election.py -- the server lease, rank
backoff, settle flag, unreachable-leader demotion and `rehost`
(SPEC SS3.3 rules 5/8, SS3.4; PLAN Stage 2 task 5) -- and the hooks
`server.py` exposes for them (`/v1/status`, `/v1/health` rows,
`POST /v1/runners/{id}/register`).

Fixture: a throwaway `git init --bare` state repo under the system temp
dir (asserted), one `fleetlib.Hub` client per candidate (its own
workdir -- two candidates must never share an object cache), compressed
timescales passed as EXPLICIT `ElectionConfig` values (deterministic)
except where a test pins the `FLEET_TEST_*` env route itself. The
re-host drill runs `election.py rehost` as real subprocesses against the
same bare repo -- the exact invocation the runbook names -- and diffs
`GET /v1/status` across a SIGKILL + re-host.

What is pinned, and the bug that makes each test fail (verified by
injecting each bug into a scratch copy of election.py/server.py and
watching the named test go red -- the modules themselves are never
edited):
  * the lease payload carries `advertise_urls`/`boot_id`/`keel_version`
    at acquire AND after every renewal (bug: patch the payload only at
    acquire instead of overriding `_payload`);
  * `ElectionConfig` defaults are the SPEC's 120/30/180/180/60 and the
    `FLEET_TEST_*` knobs compress them (bug: read env at import);
  * exactly one of N concurrent candidates wins the CAS (bug: treat
    `ClaimHeldError` as a win);
  * rank backoff: rank r may not contest before `expires_at + r*backoff`,
    an absent lease is contestable immediately, an unreadable deadline is
    never contestable (bug: drop the backoff comparison);
  * settle: `settling()` is True for `settle_s` after (re)election and
    `/v1/status`/`/v1/health` report it (bug: report False always);
  * demotion: fires iff zero registrations AND `demotion_s` has fully
    passed since settle END (not since election), releases the lease,
    emits `server.unreachable-demoted`, and the CLI exits 4 (bug: anchor
    the deadline at election, or ignore registrations);
  * `rehost` refuses a live lease (exit 3) and proceeds on an expired or
    absent one (bug: skip the liveness check);
  * the re-host drill: kill server A mid-test, rehost as B on another
    port, `status --json` minus `ts`/`server` identical -- which also
    pins that the server's own claim is excluded from `claims` (bug:
    include `refs/fleet/claims/server/*` in the status claims map).

Run with:
    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_election -v
"""

from __future__ import annotations

import hashlib
import http.client
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid
from datetime import timedelta
from pathlib import Path
from typing import Dict, List, Optional, Tuple

FLEET_DIR = Path(__file__).resolve().parents[1]
KEEL_DIR = FLEET_DIR / "keel"
for _p in (FLEET_DIR, KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import claim as claimlib  # noqa: E402
import election  # noqa: E402
import server as keel_server  # noqa: E402
import store_api  # noqa: E402
from _env import HermeticCase  # noqa: E402
from election import (  # noqa: E402
    SERVER_LEASE_REF,
    ElectionConfig,
    ElectionManager,
    RehostRefusedError,
    ServerClaim,
)
from fleetlib import Hub  # noqa: E402

ELECTION_PY = KEEL_DIR / "election.py"

OPERATOR_TOKEN = "operator-" + uuid.uuid4().hex
RUNNER_TOKEN = "runner-" + uuid.uuid4().hex


def _bare_repo(root: Path) -> str:
    path = root / "state.git"
    init = subprocess.run(["git", "init", "--quiet", "--bare", str(path)], capture_output=True)
    assert init.returncode == 0, init.stderr.decode()
    resolved = str(path.resolve())
    system_tmp = str(Path(tempfile.gettempdir()).resolve())
    assert resolved.startswith(system_tmp), f"fixture {resolved!r} is not under {system_tmp!r}"
    return str(path)


def _wait_until(pred, timeout: float, interval: float = 0.02, what: str = "condition") -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if pred():
            return
        time.sleep(interval)
    raise AssertionError(f"timed out after {timeout}s waiting for {what}")


def _tokens_json() -> Dict[str, object]:
    return {
        "tokens": [
            {"id": "op", "role": "operator", "sha256": hashlib.sha256(OPERATOR_TOKEN.encode()).hexdigest()},
            {"id": "r1", "role": "runner", "sha256": hashlib.sha256(RUNNER_TOKEN.encode()).hexdigest()},
        ]
    }


class ElectionCase(HermeticCase):
    """Bare state repo + per-candidate hubs + tracked managers."""

    def setUp(self):
        super().setUp()
        self._root = Path(tempfile.mkdtemp(prefix="election-test-"))
        self.addCleanup(shutil.rmtree, self._root, ignore_errors=True)
        self.state_url = _bare_repo(self._root)
        self._hub_count = 0
        self._managers: List[ElectionManager] = []
        self.addCleanup(self._release_managers)

    def _release_managers(self):
        for manager in self._managers:
            try:
                manager.stop_watch()
                manager.release()
            except Exception:
                pass

    def hub(self, name: Optional[str] = None) -> Hub:
        self._hub_count += 1
        return Hub(url=self.state_url, workdir=self._root / f"cache-{name or self._hub_count}")

    def manager(self, host: str, *, rank: int = 1, config: Optional[ElectionConfig] = None, **kwargs) -> ElectionManager:
        m = ElectionManager(
            self.hub(host),
            host=host,
            rank=rank,
            config=config or ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=0.2, demotion_s=60.0, rank_backoff_s=60.0, poll_s=0.2),
            **kwargs,
        )
        self._managers.append(m)
        return m

    def plant_expired_lease(self, host: str = "dead-host") -> dict:
        """A server lease whose `expires_at` is already in the past: a
        `ttl=0` acquisition (claim.py's own fixture idiom -- the renewer
        deliberately does not start for ttl<=0)."""
        stale = ServerClaim(
            self.hub(f"planter-{host}"),
            advertise_urls=["http://198.51.100.1:8470"],
            boot_id="dead-boot",
            ttl=0,
            holder_host=host,
        )
        stale.acquire()
        payload = stale.hub.read(SERVER_LEASE_REF)
        assert payload is not None and claimlib.is_expired(payload)
        return payload


# --------------------------------------------------------------------- #
# Lease payload + config
# --------------------------------------------------------------------- #


class TestServerClaimPayload(ElectionCase):
    def test_acquire_writes_the_server_payload_extras(self):
        m = self.manager("hostA")
        m.advertise_urls = ["http://100.64.0.7:8470", "http://192.168.1.7:8470"]
        self.assertTrue(m.try_elect())
        payload = self.hub("reader").read(SERVER_LEASE_REF)
        self.assertIsNotNone(payload)
        self.assertEqual(payload["advertise_urls"], ["http://100.64.0.7:8470", "http://192.168.1.7:8470"])
        self.assertEqual(payload["boot_id"], m.boot_id)
        self.assertEqual(payload["keel_version"], election.KEEL_VERSION)
        # And the base claim payload is claim.py's, unchanged.
        self.assertEqual(payload["holder_host"], "hostA")
        self.assertEqual(payload["work_kind"], "server")
        self.assertEqual(payload["work_key"], "singleton")
        self.assertIn("started_at", payload)
        self.assertIn("expires_at", payload)

    def test_every_renewal_recarries_the_extras(self):
        m = self.manager("hostA")
        self.assertTrue(m.try_elect())
        reader = self.hub("reader")
        first = reader.read(SERVER_LEASE_REF)
        self.assertTrue(m.claim.renew(), "explicit renewal should succeed")
        renewed = reader.read(SERVER_LEASE_REF)
        self.assertGreater(renewed["expires_at"], first["expires_at"], "renewal must advance expires_at")
        self.assertEqual(renewed["boot_id"], m.boot_id)
        self.assertEqual(renewed["keel_version"], election.KEEL_VERSION)
        self.assertEqual(renewed["advertise_urls"], [])

    def test_server_claim_is_claim_py_unchanged_underneath(self):
        # The renewal/lost/adopt machinery is inherited, not re-derived:
        # the ONE override is `_payload`.
        self.assertTrue(issubclass(ServerClaim, claimlib.Claim))
        lease_semantics = {
            "acquire", "acquire_or_reap", "renew", "release", "adopt",
            "_owns", "_mark_lost", "_note_renew_failure", "start_renewer", "stop_renewer",
        }
        overridden = set(ServerClaim.__dict__) & lease_semantics
        self.assertEqual(overridden, set(), "ServerClaim must not re-derive lease semantics")
        self.assertIn("_payload", ServerClaim.__dict__, "the extras ride on the ONE override")

    def test_config_defaults_are_the_spec_numbers(self):
        # HermeticCase scrubbed FLEET_TEST_* from the invoker, so these are
        # the production values: TTL 120 / renew 30 (SS3.3 rule 5), settle
        # 180 (rule 8), demotion 180 (SS3.4 step 7), backoff 60, poll 30.
        cfg = ElectionConfig.from_env()
        self.assertEqual(
            (cfg.ttl, cfg.renew_interval, cfg.settle_s, cfg.demotion_s, cfg.rank_backoff_s, cfg.poll_s),
            (120.0, 30.0, 180.0, 180.0, 60.0, 30.0),
        )

    def test_fleet_test_env_knobs_compress_the_config(self):
        os.environ[claimlib.TTL_ENV] = "2"
        os.environ[claimlib.RENEW_ENV] = "0.5"
        os.environ[election.SETTLE_ENV] = "0.3"
        os.environ[election.DEMOTION_ENV] = "0.4"
        os.environ[election.RANK_BACKOFF_ENV] = "1.5"
        os.environ[election.POLL_ENV] = "0.25"
        cfg = ElectionConfig.from_env()
        self.assertEqual(
            (cfg.ttl, cfg.renew_interval, cfg.settle_s, cfg.demotion_s, cfg.rank_backoff_s, cfg.poll_s),
            (2.0, 0.5, 0.3, 0.4, 1.5, 0.25),
        )
        # Explicit overrides still beat the env (claim.py's precedence).
        self.assertEqual(ElectionConfig.from_env(settle_s=9.0).settle_s, 9.0)


# --------------------------------------------------------------------- #
# Exactly one winner
# --------------------------------------------------------------------- #


class TestExactlyOneWinner(ElectionCase):
    N = 6

    def test_exactly_one_of_n_concurrent_candidates_wins(self):
        managers = [self.manager(f"cand-{i}") for i in range(self.N)]
        barrier = threading.Barrier(self.N)
        results: Dict[str, bool] = {}
        errors: List[BaseException] = []

        def contend(m: ElectionManager):
            try:
                barrier.wait(timeout=10)
                results[m.host] = m.try_elect()
            except BaseException as exc:  # noqa: BLE001 -- surfaced below
                errors.append(exc)

        threads = [threading.Thread(target=contend, args=(m,)) for m in managers]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=30)
        self.assertEqual(errors, [])
        winners = [host for host, won in results.items() if won]
        self.assertEqual(len(results), self.N)
        self.assertEqual(len(winners), 1, f"exactly one winner expected, got {winners}")
        payload = self.hub("reader").read(SERVER_LEASE_REF)
        self.assertEqual(payload["holder_host"], winners[0], "the ref names the winner")
        # Everyone who lost stays lost while the lease is live.
        for m in managers:
            if m.host != winners[0]:
                self.assertFalse(m.try_elect(), f"{m.host} contested a live lease")


# --------------------------------------------------------------------- #
# Rank backoff
# --------------------------------------------------------------------- #


class TestRankBackoff(ElectionCase):
    def cfg(self, backoff: float = 100.0) -> ElectionConfig:
        return ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=0.1, demotion_s=60.0, rank_backoff_s=backoff, poll_s=0.2)

    def test_eligible_at_is_expiry_plus_rank_times_backoff(self):
        payload = self.plant_expired_lease()
        expires = claimlib._parse_iso(payload["expires_at"])
        m = self.manager("ranked", rank=2, config=self.cfg(backoff=100.0))
        self.assertEqual(m.eligible_at(payload), expires + timedelta(seconds=200.0))

    def test_rank_waits_out_its_backoff_past_expiry(self):
        payload = self.plant_expired_lease()
        expires = claimlib._parse_iso(payload["expires_at"])
        m = self.manager("ranked", rank=2, config=self.cfg(backoff=100.0))
        # Just past expiry: rank 2 must NOT contest yet...
        self.assertFalse(m.try_elect(now=expires + timedelta(seconds=1)))
        self.assertIsNone(m.claim, "a refused attempt must not have acquired")
        # ...one second before its slot: still no...
        self.assertFalse(m.try_elect(now=expires + timedelta(seconds=199)))
        # ...and once `expires_at + rank*backoff` passes, it wins.
        self.assertTrue(m.try_elect(now=expires + timedelta(seconds=201)))
        self.assertEqual(self.hub("reader").read(SERVER_LEASE_REF)["holder_host"], "ranked")

    def test_rank_zero_contests_immediately_after_expiry(self):
        self.plant_expired_lease()
        m = self.manager("rank0", rank=0, config=self.cfg(backoff=100.0))
        self.assertTrue(m.try_elect())

    def test_absent_lease_is_contestable_immediately_at_any_rank(self):
        m = self.manager("high-rank", rank=7, config=self.cfg(backoff=1000.0))
        self.assertTrue(m.try_elect())

    def test_live_lease_is_never_contested(self):
        holder = self.manager("holder")
        self.assertTrue(holder.try_elect())
        m = self.manager("rank0", rank=0, config=self.cfg(backoff=0.0))
        self.assertFalse(m.try_elect())

    def test_unreadable_deadline_is_never_contested(self):
        # Fail safe: a lease whose expires_at we cannot parse is treated as
        # live, whatever the rank.
        self.hub("writer").create(SERVER_LEASE_REF, {"holder_host": "x", "expires_at": "not-a-date"})
        m = self.manager("rank0", rank=0, config=self.cfg(backoff=0.0))
        self.assertFalse(m.try_elect(now=claimlib._utcnow() + timedelta(days=365)))


# --------------------------------------------------------------------- #
# Settle flag
# --------------------------------------------------------------------- #


class TestSettleFlag(ElectionCase):
    def test_settling_for_settle_s_after_election_then_clear(self):
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=1.0, demotion_s=60.0, rank_backoff_s=60.0, poll_s=0.2)
        m = self.manager("hostA", config=cfg)
        before = time.time()
        self.assertTrue(m.try_elect())
        self.assertTrue(m.settling(), "fresh election must be settling")
        health = m.health_fields()
        self.assertTrue(health["settling"])
        self.assertGreaterEqual(health["settle_until"], before)
        self.assertLessEqual(health["settle_until"], time.time() + 1.0 + 0.5)
        self.assertIsNotNone(health["lease_expires_at"])
        _wait_until(lambda: not m.settling(), timeout=5, what="settle window to end")
        self.assertFalse(m.health_fields()["settling"])

    def test_reelection_settles_again(self):
        # SS3.3 rule 8 says settle after (RE)election: a fresh term after a
        # lease blip starts a fresh window.
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=1.0, demotion_s=60.0, rank_backoff_s=0.0, poll_s=0.05)
        m = self.manager("hostA", config=cfg)
        self.assertTrue(m.try_elect())
        _wait_until(lambda: not m.settling(), timeout=5, what="first settle to end")
        # Simulate the lease blip: the ref vanishes; the renewer (or an
        # explicit renew) marks lost.
        reader = self.hub("reader")
        sha = reader.sha(SERVER_LEASE_REF)
        self.assertTrue(reader.delete(SERVER_LEASE_REF, expect_sha=sha))
        m.claim.renew()
        self.assertTrue(m.claim.lost)
        self.assertTrue(m.try_elect(), "re-election over the absent lease")
        self.assertTrue(m.settling(), "a new term must settle again")


# --------------------------------------------------------------------- #
# Unreachable-leader demotion
# --------------------------------------------------------------------- #


class TestUnreachableDemotion(ElectionCase):
    def test_demotion_decision_boundary_is_settle_end_plus_demotion_s(self):
        clock_val = [100.0]
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=10.0, demotion_s=20.0, rank_backoff_s=60.0, poll_s=1.0)
        m = self.manager("hostA", config=cfg, clock=lambda: clock_val[0])
        self.assertTrue(m.try_elect())
        # settle ends at 110, demotion due at 130 -- never before.
        self.assertFalse(m.demotion_due(now_mono=110.0), "at settle end")
        self.assertFalse(m.demotion_due(now_mono=129.9), "just before the deadline")
        self.assertTrue(m.demotion_due(now_mono=130.0), "at the deadline")
        # `demotion_s` after ELECTION (100+20=120) is NOT the deadline --
        # the window anchors on settle END (SS3.4 step 7: "180 s after
        # settle").
        self.assertFalse(m.demotion_due(now_mono=120.0))

    def test_a_registration_averts_demotion(self):
        clock_val = [100.0]
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=10.0, demotion_s=20.0, rank_backoff_s=60.0, poll_s=1.0)
        m = self.manager("hostA", config=cfg, clock=lambda: clock_val[0])
        self.assertTrue(m.try_elect())
        m.note_registration("runner-1")
        self.assertFalse(m.demotion_due(now_mono=1e9), "a registered runner means reachable")
        self.assertEqual(m.registered_runner_ids(), ["runner-1"])

    def test_a_lost_lease_is_not_the_unreachable_path(self):
        clock_val = [100.0]
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=10.0, demotion_s=20.0, rank_backoff_s=60.0, poll_s=1.0)
        m = self.manager("hostA", config=cfg, clock=lambda: clock_val[0])
        self.assertTrue(m.try_elect())
        m.claim._mark_lost("injected for the test")
        self.assertFalse(m.demotion_due(now_mono=1e9))

    def test_demotion_fires_releases_the_lease_and_reports(self):
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=0.15, demotion_s=0.2, rank_backoff_s=60.0, poll_s=60.0)
        events = keel_server.EventLog(":memory:")
        self.addCleanup(events.close)
        demoted_calls: List[float] = []
        m = self.manager("hostA", config=cfg, events=events, on_demote=lambda: demoted_calls.append(time.time()))
        self.assertTrue(m.try_elect())
        reader = self.hub("reader")
        self.assertIsNotNone(reader.sha(SERVER_LEASE_REF))
        m.start_watch()
        self.assertTrue(m.demoted_event.wait(timeout=10), "demotion never fired")
        self.assertTrue(m.demoted)
        self.assertEqual(len(demoted_calls), 1)
        _wait_until(lambda: reader.sha(SERVER_LEASE_REF) is None, timeout=5, what="lease release on demotion")
        kinds = [e.kind for e in events.since(0)]
        self.assertIn("server.elected", kinds)
        self.assertIn("server.settle_end", kinds)
        self.assertIn("server.unreachable-demoted", kinds)
        # Idempotent: a second call must not double-fire the callback.
        m.demote_unreachable()
        self.assertEqual(len(demoted_calls), 1)

    def test_watch_never_demotes_a_server_a_runner_registered_with(self):
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=0.1, demotion_s=0.2, rank_backoff_s=60.0, poll_s=60.0)
        m = self.manager("hostA", config=cfg)
        self.assertTrue(m.try_elect())
        m.note_registration("runner-1")
        m.start_watch()
        self.assertFalse(m.demoted_event.wait(timeout=1.0), "demoted despite a registration")
        self.assertIsNotNone(self.hub("reader").sha(SERVER_LEASE_REF))


# --------------------------------------------------------------------- #
# rehost
# --------------------------------------------------------------------- #


class TestRehost(ElectionCase):
    def test_rehost_refuses_a_live_lease(self):
        holder = self.manager("holder")
        self.assertTrue(holder.try_elect())
        m = self.manager("newcomer")
        with self.assertRaises(RehostRefusedError) as ctx:
            m.rehost()
        self.assertEqual(ctx.exception.holder_host, "holder")
        self.assertIsNone(m.claim, "a refused rehost must not hold anything")
        # The living holder's lease is untouched.
        self.assertEqual(self.hub("reader").read(SERVER_LEASE_REF)["holder_host"], "holder")

    def test_rehost_proceeds_on_an_expired_lease(self):
        self.plant_expired_lease(host="dead-host")
        # settle_s is generous on purpose: the settling assertion below
        # runs after a fresh reader Hub is constructed (a git subprocess),
        # and a loaded host once burned >0.2 s there -- the window's
        # LENGTH is not what this test measures.
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=30.0, demotion_s=600.0, rank_backoff_s=60.0, poll_s=1.0)
        m = self.manager("newcomer", rank=9, config=cfg)  # rank is irrelevant to the manual path
        m.rehost()
        payload = self.hub("reader").read(SERVER_LEASE_REF)
        self.assertEqual(payload["holder_host"], "newcomer")
        self.assertFalse(claimlib.is_expired(payload))
        self.assertTrue(m.settling(), "a re-hosted server settles like any elected one")

    def test_rehost_proceeds_on_an_absent_lease(self):
        m = self.manager("first")
        m.rehost()
        self.assertEqual(self.hub("reader").read(SERVER_LEASE_REF)["holder_host"], "first")

    def test_losing_the_reap_race_is_a_refusal_not_a_crash(self):
        self.plant_expired_lease()
        m = self.manager("racer")
        real_acquire_or_reap = ServerClaim.acquire_or_reap
        state_url, root = self.state_url, self._root
        stolen = {"done": False}

        def steal_then_acquire(claim_self):
            # Between our liveness read and our CAS, another candidate
            # reaps and wins (once -- the rival's own acquisition goes
            # straight through). `rehost` must report refusal (their lease
            # is live now), never a traceback and never a second holder.
            if not stolen["done"]:
                stolen["done"] = True
                rival = ElectionManager(
                    Hub(url=state_url, workdir=root / "rival-cache"),
                    host="rival",
                    config=ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=0.1, demotion_s=60.0, rank_backoff_s=0.0, poll_s=0.2),
                )
                try:
                    assert rival.try_elect() is True
                finally:
                    rival.stop_watch()
                    if rival.claim is not None:
                        rival.claim.stop_renewer()  # leave the ref: the race we are simulating
            return real_acquire_or_reap(claim_self)

        try:
            ServerClaim.acquire_or_reap = steal_then_acquire
            with self.assertRaises(RehostRefusedError) as ctx:
                m.rehost()
        finally:
            # `del`, not re-assignment: the patch ADDED a class-dict entry
            # shadowing the inherited method, and the "one override only"
            # test reads `ServerClaim.__dict__` directly.
            del ServerClaim.acquire_or_reap
        self.assertIs(ServerClaim.acquire_or_reap, real_acquire_or_reap)
        self.assertEqual(ctx.exception.holder_host, "rival")


# --------------------------------------------------------------------- #
# The server.py hooks over HTTP
# --------------------------------------------------------------------- #


class HttpHookCase(ElectionCase):
    """A real `KeelHTTPServer` on 127.0.0.1:0 with an `InMemoryStore` (the
    hooks' store reads are `Store`-shaped, nothing more) and an attached
    `ElectionManager` over the bare-repo lease."""

    def setUp(self):
        super().setUp()
        self.store = store_api.InMemoryStore()
        self.events = keel_server.EventLog(":memory:")
        self.addCleanup(self.events.close)
        tokens = keel_server.TokenStore(_tokens_json()["tokens"])
        self.http_server = keel_server.KeelHTTPServer(
            ("127.0.0.1", 0),
            keel_server.ServerConfig(bind_host="127.0.0.1", port=0),
            self.store,
            tokens,
            self.events,
        )
        self.addCleanup(self.http_server.stop)
        self.http_server.start()
        self.port = self.http_server.server_address[1]

    def request(self, method: str, path: str, token: Optional[str] = OPERATOR_TOKEN, body: Optional[bytes] = None) -> Tuple[int, dict]:
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        try:
            headers = {}
            if token is not None:
                headers["Authorization"] = f"Bearer {token}"
            if body is not None:
                headers["Content-Type"] = "application/json"
            conn.request(method, path, body=body, headers=headers)
            resp = conn.getresponse()
            raw = resp.read()
            return resp.status, (json.loads(raw) if raw else {})
        finally:
            conn.close()


class TestStatusAndHealthHooks(HttpHookCase):
    def test_status_without_an_election_is_not_settling_and_has_no_lease(self):
        status, body = self.request("GET", "/v1/status")
        self.assertEqual(status, 200)
        self.assertEqual(body["server"]["settling"], False)
        self.assertIsNone(body["server"]["lease"])
        self.assertEqual(body["server"]["boot_id"], self.http_server.boot_id)

    def test_status_reports_settling_and_the_lease_while_elected(self):
        # settle_s covers an HTTP round trip between election and the
        # assertion; 2 s is margin for a loaded host, and the wait below
        # still bounds the test.
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=2.0, demotion_s=60.0, rank_backoff_s=60.0, poll_s=0.2)
        m = self.manager("hostA", config=cfg)
        self.assertTrue(m.try_elect())
        self.http_server.attach_election(m)
        status, body = self.request("GET", "/v1/status")
        self.assertEqual(status, 200)
        self.assertTrue(body["server"]["settling"], "/v1/status must report settling (SS3.3 rule 8)")
        self.assertEqual(body["server"]["lease"]["holder_host"], "hostA")
        self.assertEqual(body["server"]["lease"]["ref"], SERVER_LEASE_REF)
        _wait_until(lambda: not m.settling(), timeout=10, what="settle to end")
        status, body = self.request("GET", "/v1/status")
        self.assertFalse(body["server"]["settling"])

    def test_status_excludes_the_server_claim_and_serves_fleet_state(self):
        self.store.create("refs/fleet/desired", {"generation": 3})
        self.store.create("refs/fleet/claims/gate/tree123", {"holder_host": "i7"})
        self.store.create("refs/fleet/claims/server/singleton", {"holder_host": "hostA"})
        status, body = self.request("GET", "/v1/status")
        self.assertEqual(status, 200)
        self.assertEqual(body["desired"], {"generation": 3})
        self.assertEqual(list(body["claims"]), ["refs/fleet/claims/gate/tree123"])

    def test_status_requires_auth(self):
        status, body = self.request("GET", "/v1/status", token=None)
        self.assertEqual(status, 401)

    def test_health_carries_lease_expires_at_and_settle_until(self):
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=2.0, demotion_s=60.0, rank_backoff_s=60.0, poll_s=0.2)
        m = self.manager("hostA", config=cfg)
        self.assertTrue(m.try_elect())
        self.http_server.attach_election(m)
        status, body = self.request("GET", "/v1/health", token=None)
        self.assertEqual(status, 200)
        self.assertIsNotNone(body["lease_expires_at"])
        self.assertIsNotNone(body["settle_until"])
        self.assertTrue(body["settling"])


class TestRegisterRoute(HttpHookCase):
    def test_register_counts_toward_demotion_and_answers_the_spec_fields(self):
        cfg = ElectionConfig(ttl=5.0, renew_interval=1.0, settle_s=0.1, demotion_s=0.2, rank_backoff_s=60.0, poll_s=0.2)
        m = self.manager("hostA", config=cfg)
        self.assertTrue(m.try_elect())
        self.http_server.attach_election(m)
        status, body = self.request("POST", "/v1/runners/r1/register", token=RUNNER_TOKEN, body=b"{}")
        self.assertEqual(status, 200)
        self.assertEqual(body["boot_id"], self.http_server.boot_id)
        self.assertIn("settle_until", body)
        self.assertIn("lease_expires_at", body)
        self.assertEqual(m.registered_runner_ids(), ["r1"])
        self.assertFalse(m.demotion_due(now_mono=1e9), "the registration must avert demotion")

    def test_register_requires_auth(self):
        status, _ = self.request("POST", "/v1/runners/r1/register", token=None, body=b"{}")
        self.assertEqual(status, 401)


# --------------------------------------------------------------------- #
# The re-host drill (SS3.4 acceptance; PLAN Stage 2)
# --------------------------------------------------------------------- #


class TestRehostDrill(ElectionCase):
    """Kill server A mid-test, rehost as B on another port; `GET
    /v1/status` minus `ts`/`server` must be identical -- as real
    `election.py rehost` subprocesses, compressed through the same
    `FLEET_TEST_*` knobs a human would use."""

    TTL_S = 2.0

    def drill_env(self, **extra: str) -> Dict[str, str]:
        return self.hermetic_env(
            FLEET_TEST_TTL_S=str(self.TTL_S),
            FLEET_TEST_RENEW_S="0.5",
            FLEET_TEST_SETTLE_S="0.3",
            FLEET_TEST_DEMOTION_S="600",  # never mid-drill
            **extra,
        )

    def spawn_rehost(self, name: str, register_runner: bool = False) -> Tuple[subprocess.Popen, Path]:
        port_file = self._root / f"{name}.port"
        out = open(self._root / f"{name}.log", "wb")
        self.addCleanup(out.close)
        proc = subprocess.Popen(
            [
                sys.executable, str(ELECTION_PY), "rehost",
                "--state-url", self.state_url,
                "--lease-workdir", str(self._root / f"lease-{name}"),
                "--state-workdir", str(self._root / f"store-{name}"),
                "--tokens-file", str(self._root / "tokens.json"),
                "--events-db", str(self._root / f"events-{name}.db"),
                "--port", "0",
                "--port-file", str(port_file),
                "--sweep-interval", "0",
                "--host", f"host-{name}",
            ],
            env=self.drill_env(),
            stdout=out,
            stderr=subprocess.STDOUT,
        )
        self.addCleanup(self._reap, proc)
        return proc, port_file

    def _reap(self, proc: subprocess.Popen) -> None:
        if proc.poll() is None:
            proc.kill()
        proc.wait(timeout=10)

    def _status(self, port_file: Path) -> dict:
        host, port = port_file.read_text().strip().rsplit(":", 1)
        conn = http.client.HTTPConnection(host, int(port), timeout=10)
        try:
            conn.request("GET", "/v1/status", headers={"Authorization": f"Bearer {OPERATOR_TOKEN}"})
            resp = conn.getresponse()
            body = json.loads(resp.read())
            assert resp.status == 200, body
            return body
        finally:
            conn.close()

    def test_kill_a_rehost_b_status_minus_ts_and_server_is_identical(self):
        (self._root / "tokens.json").write_text(json.dumps(_tokens_json()))
        # Seed real fleet state for the two servers to agree about.
        seeder = self.hub("seeder")
        seeder.create("refs/fleet/desired", {"generation": 7, "hosts": {"i7": {"gates": 1, "agents": 0}}})
        seeder.create("refs/fleet/claims/gate/tree-abc", {"holder_host": "i7", "expires_at": "2999-01-01T00:00:00+00:00"})
        seeder.create("refs/fleet/verdicts/deadbeef/8/plat1", {"verdict": "PASS"})

        proc_a, port_a = self.spawn_rehost("A")
        _wait_until(port_a.exists, timeout=30, what="server A to bind")
        status_a = self._status(port_a)
        self.assertEqual(status_a["server"]["host"], "host-A")
        self.assertEqual(status_a["desired"]["generation"], 7)
        self.assertIn("refs/fleet/claims/gate/tree-abc", status_a["claims"])
        self.assertNotIn(
            SERVER_LEASE_REF, status_a["claims"],
            "the server's own claim must live under `server`, not `claims`, "
            "or the acceptance diff can never be empty",
        )

        # While A lives: a second rehost refuses, exit 3, and A is unharmed.
        refused = subprocess.run(
            [
                sys.executable, str(ELECTION_PY), "rehost",
                "--state-url", self.state_url,
                "--lease-workdir", str(self._root / "lease-refused"),
                "--state-workdir", str(self._root / "store-refused"),
                "--tokens-file", str(self._root / "tokens.json"),
                "--events-db", str(self._root / "events-refused.db"),
                "--port", "0",
                "--sweep-interval", "0",
                "--host", "host-refused",
            ],
            env=self.drill_env(),
            capture_output=True,
            timeout=60,
        )
        self.assertEqual(refused.returncode, election.EXIT_REHOST_REFUSED, refused.stderr.decode())
        self.assertIn("refusing to re-host", refused.stderr.decode())
        self.assertEqual(self.hub("reader").read(SERVER_LEASE_REF)["holder_host"], "host-A")

        # Kill A mid-test -- SIGKILL, so nothing releases the lease.
        proc_a.send_signal(signal.SIGKILL)
        proc_a.wait(timeout=10)
        reader = self.hub("reader")
        payload = reader.read(SERVER_LEASE_REF)
        self.assertIsNotNone(payload, "a SIGKILL must leave the lease to expire, not release it")

        # Wait out the dead lease, then rehost as B on another port.
        expires = claimlib._parse_iso(payload["expires_at"])
        _wait_until(
            lambda: claimlib._utcnow() > expires + timedelta(seconds=0.3),
            timeout=self.TTL_S + 30,
            interval=0.1,
            what="A's lease to expire",
        )
        proc_b, port_b = self.spawn_rehost("B")
        _wait_until(port_b.exists, timeout=30, what="server B to bind")
        self.assertNotEqual(port_a.read_text(), port_b.read_text(), "B is on another port")
        status_b = self._status(port_b)
        self.assertEqual(status_b["server"]["host"], "host-B")
        self.assertNotEqual(status_a["server"]["boot_id"], status_b["server"]["boot_id"])

        # THE instrument: del(.ts, .server) -> identical.
        a = {k: v for k, v in status_a.items() if k not in ("ts", "server")}
        b = {k: v for k, v in status_b.items() if k not in ("ts", "server")}
        self.assertEqual(a, b)
        self.assertTrue(a["claims"], "the diff must be over real state, not two empty views")

        # B stops cleanly: exit 0, lease released.
        proc_b.send_signal(signal.SIGTERM)
        self.assertEqual(proc_b.wait(timeout=15), election.EXIT_OK)
        self.assertIsNone(reader.sha(SERVER_LEASE_REF))

    def test_unreachable_demotion_exits_4_and_releases_the_lease(self):
        (self._root / "tokens.json").write_text(json.dumps(_tokens_json()))
        proc = subprocess.run(
            [
                sys.executable, str(ELECTION_PY), "rehost",
                "--state-url", self.state_url,
                "--lease-workdir", str(self._root / "lease-D"),
                "--state-workdir", str(self._root / "store-D"),
                "--tokens-file", str(self._root / "tokens.json"),
                "--events-db", str(self._root / "events-D.db"),
                "--port", "0",
                "--sweep-interval", "0",
                "--host", "host-D",
            ],
            env=self.hermetic_env(
                FLEET_TEST_TTL_S="2",
                FLEET_TEST_RENEW_S="0.5",
                FLEET_TEST_SETTLE_S="0.2",
                FLEET_TEST_DEMOTION_S="0.3",
            ),
            capture_output=True,
            timeout=60,
        )
        self.assertEqual(proc.returncode, election.EXIT_UNREACHABLE_DEMOTED, proc.stderr.decode())
        self.assertIn("server.unreachable-demoted", proc.stderr.decode())
        self.assertIsNone(self.hub("reader").sha(SERVER_LEASE_REF), "demotion must release the lease")


if __name__ == "__main__":
    unittest.main()
