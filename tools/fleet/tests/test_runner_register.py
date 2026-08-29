"""Runner registration: outbound `register`, reconnect detection, and the
enriched heartbeat (Keel Stage 3R-1; SPEC §5.3 step 1).

WHY THIS MODULE BUILDS ITS OWN SERVER IN BOTH MODES. Everything here is a
property of the SERVER route: `ServerHub.register` exists only on the
primary, `keel.runner.server_client` returns `None` for any hub that is
not a `FallbackHub` around a `ServerHub`, and a hubless Stage-1 runner
therefore skips the whole path. Under `FLEET_TEST_HUB=bare`,
`_fixtures.make_hub` hands back a plain `fleetlib.Hub` with no server
behind it at all -- against which every assertion below would pass
vacuously, and the module would report green for the reason AGENTS.md
incident #1 warns about: a harness that cannot reach the code it claims
to test. So this module builds a real in-process `keel-server`
(`_fixtures._ServerHubFixture`, the same `build_server` on `127.0.0.1:0`
with the same per-process fixture token) UNCONDITIONALLY, in both modes.
It is not `@bare_only` and it is not mode-switched: the premise is the
HTTP route, and the HTTP route is present either way.

WHAT IS PINNED, and against which line of the design:

  1. `register` answers `{boot_id, settle_until, lease_expires_at}` and
     the only server-observable proof it landed is
     `/v1/status`'s `registered_runners` (`election.note_registration` is
     the sole inbound signal that averts `demote_unreachable`).
  2. `register` is NOT on the fallback write path. `FallbackHub` has no
     `register`, and neither does `fleetlib.Hub` -- so it cannot acquire
     the fail-closed ambiguous-write classifier by accident, which is
     what would turn a bounded retry into a re-issue after an ambiguous
     outcome (SPEC §4.3 r2).
  3. Registration is strictly non-fatal: a dead server yields `None` and
     the daemon still completes its reconcile. TWO different "dead"s are
     needed and only one of them used to be here. A REFUSED connection
     (`stop_server()`) can never exercise a read timeout; a listener that
     ACCEPTS and then answers nothing charges the caller its whole read
     budget. Both measured against the unfixed `server_client`:
     `register_cycle` took 0.0011 s against a closed port and 20.00 s
     against `_BlackHoleListener`, and one `run_daemon` step at
     `interval=0` took 20.43 s. `TestRegistrationLatencyBound` supplies
     the second shape and asserts a wall-clock bound; without it 18 green
     tests proved "non-fatal" while never once paying a timeout.
  4. A changed `boot_id` re-registers EXACTLY ONCE, and an unchanged one
     re-registers never. `TestReconnectNegativeControl` disables the
     comparison and requires that test to go RED -- a reconnect test that
     cannot fail proves nothing.
  5. The heartbeat carries `live_workers[]` (from the IN-MEMORY workers
     list, never a hub claim listing) and the `fallback` block. Pinned
     with a NON-EMPTY workers list: an empty one cannot tell
     `live_workers_payload(workers)` from `live_workers_payload([])`, so
     it pins the key's presence and nothing else.
  6. Registration's latency is bounded and damped. It runs on the
     reconcile loop's own thread, so an unbounded wait there is a
     scheduling outage; `server_client` gives it its own timeouts and
     `register_cycle`'s `backoff` keeps a persistently dead server from
     costing one timeout per cycle forever.
"""

from __future__ import annotations

import signal as signal_mod
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from _env import HermeticCase, scrub_env  # noqa: E402
from _fixtures import _ServerHubFixture  # noqa: E402

import claim as claim_mod  # noqa: E402
import fleetd  # noqa: E402
import keel.runner as runner  # noqa: E402
from fleetlib import Hub  # noqa: E402
from keel import election as election_mod  # noqa: E402
from keel import serverhub as serverhub_mod  # noqa: E402
from keel.fallbackhub import FallbackHub, PrimaryFailure  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402

HUB_TIP_REF = "refs/heads/refactor/tag-machinery"
REPO_ROOT = Path(__file__).resolve().parents[3]


def _seed_bare(tmp: Path) -> Path:
    """A bare state repo with one commit on the tip and one staging
    branch -- test_runner_core's fixture shape, trimmed to what the
    registration tests need."""
    assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
    bare = tmp / "hub.git"
    subprocess.run(["git", "init", "-q", "--bare", str(bare)], check=True)
    work = tmp / "seed"
    subprocess.run(["git", "init", "-q", str(work)], check=True)
    env = scrub_env()
    (work / "f.txt").write_text("tip\n")
    subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "tip"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "push", "-q", str(bare), f"HEAD:{HUB_TIP_REF}"],
                   check=True, env=env)
    (work / "g.txt").write_text("branch\n")
    subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "branch work"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "push", "-q", str(bare),
                    "HEAD:refs/heads/staging/one"], check=True, env=env)
    return bare


def _stub_gate(tmp: Path) -> Path:
    stub = tmp / "stub-gate.sh"
    stub.write_text(
        "#!/bin/bash\n"
        f"STOP={tmp}/stop-$2\n"
        'while [ ! -f "$STOP" ]; do sleep 0.2; done\n'
        "exit 0\n"
    )
    stub.chmod(0o755)
    return stub


class ServerFixture(HermeticCase):
    """A real `keel-server` on 127.0.0.1:0 in front of a real bare repo,
    plus the production `FallbackHub(ServerHub, Hub)` shape around it."""

    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.bare = _seed_bare(self.tmp)
        self.fixture = _ServerHubFixture(str(self.bare), self.tmp / "hubcache", None)
        self.addCleanup(self.fixture.close)
        self.addCleanup(self.tmpdir.cleanup)
        self.hub = self.fixture.hub
        self.host = "testhost"
        self.client = runner.server_client(self.hub)
        self.assertIsInstance(
            self.client, ServerHub,
            "the fixture must present the production FallbackHub(ServerHub, Hub) "
            "shape; without it every assertion in this module is vacuous",
        )

    # -- helpers ------------------------------------------------------- #

    @property
    def server(self):
        """The `KeelHTTPServer` behind the fixture. Private on the fixture
        by underscore; reached here (and only here) because two of these
        tests need to simulate a server RESTART, which from a client's
        point of view is exactly a new `boot_id` on the same address."""
        return self.fixture._server

    def attach_election(self):
        """A real `ElectionManager` with no lease taken: `note_registration`
        and `status_fields` are all the register route touches, and neither
        needs a claim (`ElectionManager.status_fields` tolerates
        `self.claim is None`). Nothing is started, so nothing needs
        stopping."""
        manager = election_mod.ElectionManager(
            Hub(url=str(self.bare), workdir=str(self.tmp / "election-cache")),
            host=self.host,
            advertise_urls=[self.fixture.server_url],
        )
        self.server.attach_election(manager)
        return manager

    def payload(self, workers=()):
        return runner.registration_payload(
            self.host, list(workers), REPO_ROOT, runner.fleet_scope_token(self.hub.url)
        )

    def _worker(self, tag="t1", kind="gate", pgid=4242):
        """A hand-built live `Worker`, claim and all -- the same shape
        `adopt_workers` leaves in `run_daemon`'s in-memory list. On
        `ServerFixture` rather than one test class because BOTH layers
        that consume it need one: the registration payload and the
        heartbeat."""
        c = claim_mod.Claim(self.hub, "gate", "staging-one", holder_host=self.host)
        c.ref  # touch: the ref is computed in __init__
        c._sha = "0" * 40
        c._started_at = claim_mod.datetime.now(claim_mod.timezone.utc)
        return runner.Worker(branch="staging/one", tag=tag, pgid=pgid, claim=c, kind=kind)

    def counting_client(self):
        """Wrap `register` on the real client so calls are counted while
        still going out over real HTTP to the real server."""
        calls = []
        real = self.client.register

        def counted(runner_id, body):
            calls.append((runner_id, body))
            return real(runner_id, body)

        self.client.register = counted
        return calls


# --------------------------------------------------------------------- #
# 1. The call itself
# --------------------------------------------------------------------- #


class TestServerHubRegister(ServerFixture):
    def test_register_returns_boot_id_settle_until_and_lease_expires_at(self):
        reply = self.client.register(self.host, self.payload())
        self.assertIsInstance(reply, dict)
        for key in ("boot_id", "settle_until", "lease_expires_at"):
            self.assertIn(key, reply, f"register must answer {key} (server.py's reply dict)")
        self.assertEqual(reply["boot_id"], self.server.boot_id)

    def test_registration_is_visible_in_status_registered_runners(self):
        """The ONE server-observable proof the call landed: nothing else
        in this tree stores a registration."""
        self.attach_election()
        status = self.client._request("GET", "/v1/status")[1]
        self.assertNotIn(self.host, status["server"]["registered_runners"],
                         "precondition: nobody has registered yet")
        self.client.register(self.host, self.payload())
        status = self.client._request("GET", "/v1/status")[1]
        self.assertIn(self.host, status["server"]["registered_runners"])

    def test_register_is_not_reachable_through_the_fallback_write_path(self):
        """SPEC §4.3 r2, as a fence. The moment `register` is reachable
        through `FallbackHub`, it inherits the fail-closed ambiguous
        classifier and any bounded retry around it becomes a re-issue
        after an ambiguous outcome. It must live on the primary only."""
        self.assertTrue(hasattr(self.client, "register"))
        self.assertFalse(hasattr(FallbackHub, "register"),
                         "FallbackHub must not carry register: it would drag an "
                         "idempotent announcement onto the CAS write path")
        self.assertFalse(hasattr(Hub, "register"),
                         "fleetlib.Hub must not carry register: there is no "
                         "GitHub-side registration to fall back to")
        client = runner.server_client(self.hub)
        self.assertIsInstance(client, ServerHub)
        self.assertEqual(client.base_url, self.hub.primary.base_url)
        self.assertIsNot(
            client, self.hub.primary,
            "the registration client must be a CLONE, not the FallbackHub's own "
            "primary: re-pointing that object's timeouts would move the CAS write "
            "path's budget underneath the renewer threads",
        )
        self.assertEqual(client.connect_timeout_s, runner.REGISTER_CONNECT_TIMEOUT_S)
        self.assertEqual(client.read_timeout_s, runner.REGISTER_READ_TIMEOUT_S)
        self.assertLess(
            client.read_timeout_s, serverhub_mod.DEFAULT_READ_TIMEOUT_S,
            "an announcement's budget must be smaller than a CAS write's: the "
            "write path buys 20 s to make an ambiguous outcome rarer, and this "
            "call has no ambiguous outcome to buy anything for",
        )
        self.assertIsNone(runner.server_client(self.hub.github),
                          "a plain Hub is a hubless Stage-1 runner: no client")

    def test_a_dead_server_raises_from_the_client_and_is_swallowed_by_register_once(self):
        self.fixture.stop_server()
        with self.assertRaises(PrimaryFailure):
            self.client.register(self.host, self.payload())
        logged = []
        self.assertIsNone(
            runner.register_once(self.client, self.host, self.payload(), logged.append)
        )
        self.assertTrue(logged and "REGISTER failed" in logged[0], logged)


# --------------------------------------------------------------------- #
# 2. The reconnect decision
# --------------------------------------------------------------------- #


class TestRegisterCycle(ServerFixture):
    def test_first_cycle_registers_and_keeps_the_session(self):
        calls = self.counting_client()
        session: dict = {}
        reason = runner.register_cycle(
            self.client, self.host, session, self.payload, lambda _m: None
        )
        self.assertEqual(reason, "first")
        self.assertEqual(len(calls), 1)
        self.assertEqual(session["boot_id"], self.server.boot_id)

    def test_an_unchanged_boot_id_never_re_registers(self):
        calls = self.counting_client()
        session: dict = {}
        for _ in range(4):
            runner.register_cycle(self.client, self.host, session, self.payload, lambda _m: None)
        self.assertEqual(len(calls), 1,
                         "a server that never restarted must be registered with once")

    def test_a_changed_boot_id_re_registers_exactly_once(self):
        calls = self.counting_client()
        session: dict = {}
        self.assertEqual(
            runner.register_cycle(self.client, self.host, session, self.payload, lambda _m: None),
            "first",
        )
        first_boot = session["boot_id"]
        # A server restart, as a client can see one: same address, new
        # boot_id. `handle_health` and `handle_runner_register` both read
        # `server.boot_id` live, so rebinding it is the whole simulation.
        self.server.boot_id = "b" * 32
        self.assertEqual(
            runner.register_cycle(self.client, self.host, session, self.payload, lambda _m: None),
            "reconnect",
        )
        self.assertEqual(len(calls), 2)
        self.assertNotEqual(session["boot_id"], first_boot)
        self.assertEqual(session["boot_id"], "b" * 32)
        # ...and settles: no re-register storm on the cycles after.
        for _ in range(3):
            self.assertIsNone(
                runner.register_cycle(self.client, self.host, session, self.payload, lambda _m: None)
            )
        self.assertEqual(len(calls), 2)

    def test_a_failed_first_register_is_retried_on_the_next_cycle(self):
        """An empty session means "we are not registered", so the next
        cycle tries again -- the retry policy is this loop's cadence and
        nothing else."""
        self.fixture.stop_server()
        session: dict = {}
        self.assertIsNone(
            runner.register_cycle(self.client, self.host, session, self.payload, lambda _m: None)
        )
        self.assertEqual(session, {})

    def test_a_health_probe_failure_keeps_the_existing_registration(self):
        session: dict = {}
        runner.register_cycle(self.client, self.host, session, self.payload, lambda _m: None)
        before = dict(session)
        self.fixture.stop_server()
        logged = []
        self.assertIsNone(
            runner.register_cycle(self.client, self.host, session, self.payload, logged.append)
        )
        self.assertEqual(session, before,
                         "an unreachable server is not evidence that it restarted")
        self.assertTrue(any("health probe failed" in m for m in logged), logged)


class TestReconnectNegativeControl(ServerFixture):
    """The control for `test_a_changed_boot_id_re_registers_exactly_once`.

    Disable the boot_id comparison -- exactly the one predicate that
    detects a reconnect -- and the reconnect assertion must FAIL. Modelled
    on `test_fallbackhub.test_negative_control_index_served_claim_sha_...`:
    assert the RIGHT failure by matching the assertion message, then prove
    the comparison restores itself.
    """

    def test_disabling_the_boot_id_comparison_makes_the_reconnect_test_red(self):
        original = runner._boot_id_changed
        self.addCleanup(setattr, runner, "_boot_id_changed", original)
        # The bug-present shape: a runner that never notices the server it
        # registered with is gone.
        runner._boot_id_changed = lambda health, session: False

        calls = self.counting_client()
        session: dict = {}
        runner.register_cycle(self.client, self.host, session, self.payload, lambda _m: None)
        self.assertEqual(len(calls), 1)
        self.server.boot_id = "b" * 32

        with self.assertRaises(AssertionError) as caught:
            reason = runner.register_cycle(
                self.client, self.host, session, self.payload, lambda _m: None
            )
            self.assertEqual(reason, "reconnect")
        self.assertIn("reconnect", str(caught.exception),
                      "the control must fail ON the reconnect assertion, not "
                      "incidentally somewhere else")
        self.assertEqual(len(calls), 1, "with the comparison disabled, nothing re-registers")

        # Restored (by addCleanup, but proven here rather than assumed):
        # the same stimulus against the real predicate is a reconnect.
        runner._boot_id_changed = original
        self.assertEqual(
            runner.register_cycle(self.client, self.host, session, self.payload, lambda _m: None),
            "reconnect",
        )
        self.assertEqual(len(calls), 2)


# --------------------------------------------------------------------- #
# 3. The payload
# --------------------------------------------------------------------- #


class TestRegistrationPayload(ServerFixture):
    def test_capabilities_and_live_workers_shape(self):
        w = self._worker()
        p = self.payload([w])
        self.assertEqual(p["id"], self.host)
        caps = p["capabilities"]
        for key in ("owning_user", "platform_id", "rustc_id", "cores",
                    "free_disk_gb", "free_mem_gb", "oracle_ok", "gate_version",
                    "scope_token"):
            self.assertIn(key, caps)
        self.assertEqual(caps["scope_token"], runner.fleet_scope_token(self.hub.url))
        self.assertEqual(len(p["live_workers"]), 1)
        lw = p["live_workers"][0]
        self.assertEqual(
            sorted(lw), ["claim_ref", "claim_sha", "kind", "pgid", "started_at", "tag"]
        )
        self.assertEqual(lw["claim_ref"], w.claim.ref)
        self.assertEqual(lw["claim_sha"], "0" * 40)
        self.assertEqual(lw["pgid"], 4242)
        self.assertEqual(lw["kind"], "gate")
        self.assertTrue(lw["started_at"].endswith("Z") or "+" in lw["started_at"],
                        f"started_at must be claim._iso's spelling: {lw['started_at']!r}")

    def test_live_workers_comes_from_the_in_memory_list_not_a_hub_listing(self):
        """SPEC's fresh-claims invariant, one layer up: `CachedHub.list()`
        over the claims namespace is index-served with no freshness test,
        and a stale listing feeding a liveness join has no CAS behind it to
        catch the error. So a worker with NO claim ref on the hub at all
        must still appear, and a claim ref on the hub with no live worker
        must NOT."""
        w = self._worker()
        self.assertIsNone(self.hub.sha(w.claim.ref), "nothing was pushed for this claim")
        p = self.payload([w])
        self.assertEqual([e["claim_ref"] for e in p["live_workers"]], [w.claim.ref])
        # ...and the empty case is empty, not "whatever the hub lists".
        self.assertEqual(self.payload([])["live_workers"], [])

    def test_an_unknowable_free_mem_is_null_not_minus_one(self):
        real = runner.free_mem_gb
        runner.free_mem_gb = lambda: -1.0
        self.addCleanup(setattr, runner, "free_mem_gb", real)
        self.assertIsNone(self.payload()["capabilities"]["free_mem_gb"])


# --------------------------------------------------------------------- #
# 4. The daemon: registration is non-fatal, and the loop still reconciles
# --------------------------------------------------------------------- #


class TestRunDaemonRegistration(ServerFixture):
    def setUp(self):
        super().setUp()
        self.stub = _stub_gate(self.tmp)
        self.log_dir = self.tmp / "logs"
        self._old_term = signal_mod.getsignal(signal_mod.SIGTERM)
        self._old_int = signal_mod.getsignal(signal_mod.SIGINT)
        self.addCleanup(signal_mod.signal, signal_mod.SIGTERM, self._old_term)
        self.addCleanup(signal_mod.signal, signal_mod.SIGINT, self._old_int)

    def _run_once(self):
        steps = []

        def scripted(hub, host, workers, *a, **kw):
            steps.append(1)
            return fleetd.ReconcileResult()

        rc = runner.run_daemon(
            self.hub, self.host,
            gate_command=[str(self.stub)],
            log_dir=self.log_dir,
            repo_root=REPO_ROOT,
            interval=0,
            once=True,
            reconcile=scripted,
        )
        return rc, steps

    def test_a_live_server_is_registered_from_inside_the_loop(self):
        self.attach_election()
        rc, steps = self._run_once()
        self.assertEqual(rc, 0)
        self.assertEqual(len(steps), 1)
        status = self.client._request("GET", "/v1/status")[1]
        self.assertIn(self.host, status["server"]["registered_runners"],
                      "one --once run must leave a registration behind")

    def test_a_dead_server_still_completes_a_reconcile(self):
        """Registration is strictly non-fatal. This is the whole contract:
        a runner whose registration cannot land must still gate."""
        self.fixture.stop_server()
        rc, steps = self._run_once()
        self.assertEqual(rc, 0, "a failed registration must not change the exit code")
        self.assertEqual(len(steps), 1, "the reconcile step must still have run")

    def test_a_hubless_runner_registers_nothing_and_still_runs(self):
        """`server_client` is None on a plain `fleetlib.Hub`; the whole
        path is skipped and Stage-1 behaviour is byte-identical."""
        plain = runner.build_hub(str(self.bare), workdir=self.tmp / "plain-cache")
        self.assertIsNone(runner.server_client(plain))
        steps = []
        rc = runner.run_daemon(
            plain, "otherhost",
            gate_command=[str(self.stub)],
            log_dir=self.log_dir,
            repo_root=REPO_ROOT,
            interval=0, once=True,
            reconcile=lambda *a, **kw: (steps.append(1), fleetd.ReconcileResult())[1],
        )
        self.assertEqual(rc, 0)
        self.assertEqual(len(steps), 1)


# --------------------------------------------------------------------- #
# 4b. ...and it is BOUNDED. A server that accepts and never answers.
# --------------------------------------------------------------------- #


class _BlackHoleListener:
    """A TCP listener that completes the handshake and then answers
    nothing, ever.

    THE POINT. Every other "server is down" case in this module calls
    `fixture.stop_server()`, which leaves a CLOSED port: `connect()` gets
    RST and the call fails in about a millisecond. That can never reach a
    read timeout, so it cannot exercise the condition claim (b) is about.
    A wedged accept loop, a SIGSTOPped process and a half-open tailnet
    path all look like this instead -- the connection succeeds and the
    response never comes -- and only that shape charges the caller its
    whole read budget.

    Accepted connections are HELD, never closed: closing them would send
    FIN and the client would see EOF immediately, which is the fast
    failure again with extra steps.
    """

    def __init__(self):
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(16)
        self.url = f"http://127.0.0.1:{self._sock.getsockname()[1]}"
        self._held: list = []
        self._thread = threading.Thread(target=self._accept_forever, daemon=True)
        self._thread.start()

    def _accept_forever(self):
        while True:
            try:
                conn, _addr = self._sock.accept()
            except OSError:
                return
            self._held.append(conn)

    def close(self):
        try:
            self._sock.close()
        except OSError:
            pass
        for conn in self._held:
            try:
                conn.close()
            except OSError:
                pass
        self._thread.join(timeout=5)


class TestRegistrationLatencyBound(HermeticCase):
    """Claim (b): registration must not stall the reconcile loop.

    `register_cycle` is called from `run_daemon` between the reconcile
    step and `time.sleep(interval)`, on the loop's own thread. Against a
    black hole the unfixed code paid `ServerHub`'s CAS budget there --
    5 s connect + 20 s read -- on EVERY cycle, in either steady state
    (`register_once` while unregistered, `health()` once registered).

    THE BOUND IS ASSERTED IN TERMS OF THE CONFIGURED TIMEOUTS, never a
    hard-coded number of seconds, so raising `REGISTER_READ_TIMEOUT_S`
    cannot quietly turn this test into a tautology. The constants are
    compressed here purely so the suite does not spend the production
    budget; the production values are what `server_client` reads, and
    `TestServerHubRegister.test_register_is_not_reachable_...` pins those.
    """

    # Compressed budget. Loopback + a black hole needs no headroom to
    # reach the timeout, and the assertions below scale with it.
    TEST_CONNECT_S = 0.3
    TEST_READ_S = 0.3

    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.tmp = Path(self.tmpdir.name)
        self.bare = _seed_bare(self.tmp)
        self.blackhole = _BlackHoleListener()
        self.addCleanup(self.blackhole.close)
        self.host = "testhost"
        for name, value in (("REGISTER_CONNECT_TIMEOUT_S", self.TEST_CONNECT_S),
                            ("REGISTER_READ_TIMEOUT_S", self.TEST_READ_S)):
            self.addCleanup(setattr, runner, name, getattr(runner, name))
            setattr(runner, name, value)
        # The production shape, with the black hole where the server goes.
        self.primary = ServerHub(self.blackhole.url, token="fixture-unused")
        self.github = Hub(url=str(self.bare), workdir=str(self.tmp / "cache"),
                          code_url=str(self.bare))
        self.hub = FallbackHub(self.primary, self.github)

    # -- helpers ------------------------------------------------------- #

    @property
    def budget(self) -> float:
        """One call's worst case, as the code configures it."""
        return runner.REGISTER_CONNECT_TIMEOUT_S + runner.REGISTER_READ_TIMEOUT_S

    def assert_within_budget(self, elapsed: float, calls: int, what: str,
                             slack: float = 1.0):
        """`calls` timeouts' worth, doubled, plus `slack` for whatever
        local work the caller does around them. Generous on purpose: this
        test exists to catch a 20 s stall, not to police jitter, and a
        flaky timing assertion would be removed rather than believed."""
        bound = 2 * calls * self.budget + slack
        self.assertLess(
            elapsed, bound,
            f"{what} took {elapsed:.2f}s against a black-hole server; the "
            f"configured registration budget is {self.budget:.2f}s per call "
            f"({calls} expected), so anything past {bound:.2f}s means the call "
            f"is not using it -- the reconcile loop's cadence is being charged "
            f"someone else's timeout",
        )

    def degrade_the_fallback_route(self):
        """Put `FallbackHub` in its degraded/sticky state BEFORE timing,
        so the window measures registration and not `_primary_worth_trying`'s
        own once-per-30 s primary probe (`fallbackhub.STICKY_S`).

        This mirrors the condition the finding was measured under, and it
        is the steady state of a runner whose server has been down for
        more than one cycle. The primary's own timeouts are compressed
        only for the duration of this warm-up -- restoring them is what
        keeps the timed window honest, because `server_client` is
        supposed to override them and this test must fail if it does not.
        """
        saved = (self.primary.connect_timeout_s, self.primary.read_timeout_s)
        self.primary.connect_timeout_s = 0.2
        self.primary.read_timeout_s = 0.2
        try:
            self.hub.sha("refs/heads/no-such-branch")
        finally:
            self.primary.connect_timeout_s, self.primary.read_timeout_s = saved
        self.assertNotEqual(
            self.hub.status()["route"], "primary",
            "warm-up must leave the FallbackHub degraded, or the timed window "
            "below measures the hub's probe instead of the registration",
        )
        self.assertEqual(self.primary.read_timeout_s,
                         serverhub_mod.DEFAULT_READ_TIMEOUT_S,
                         "the primary must be back on the CAS budget before timing")

    # -- the bound ----------------------------------------------------- #

    def test_a_first_registration_against_a_black_hole_is_bounded(self):
        client = runner.server_client(self.hub)
        session: dict = {}
        t0 = time.monotonic()
        self.assertIsNone(
            runner.register_cycle(client, self.host, session, lambda: {"id": self.host},
                                  lambda _m: None)
        )
        self.assert_within_budget(time.monotonic() - t0, 1, "register_cycle (unregistered)")
        self.assertEqual(session, {}, "nothing landed, so nothing is recorded")

    def test_a_health_probe_against_a_black_hole_is_bounded(self):
        """The OTHER steady state. A runner that registered before the
        server wedged takes the `health()` branch instead, and that branch
        used to pay the same 20 s."""
        client = runner.server_client(self.hub)
        session = {"boot_id": "a" * 32}
        t0 = time.monotonic()
        self.assertIsNone(
            runner.register_cycle(client, self.host, session, lambda: {"id": self.host},
                                  lambda _m: None)
        )
        self.assert_within_budget(time.monotonic() - t0, 1, "register_cycle (registered)")
        self.assertEqual(session, {"boot_id": "a" * 32},
                         "an unreachable server is not evidence that it restarted")

    def test_a_run_daemon_step_against_a_black_hole_is_bounded(self):
        """End to end, on the thread that matters. `interval=0` means the
        designed gap between steps is ~0 s, so the whole call is the
        registration's latency plus the (local, scripted) reconcile."""
        self.degrade_the_fallback_route()
        steps = []
        stub = _stub_gate(self.tmp)
        old_term = signal_mod.getsignal(signal_mod.SIGTERM)
        old_int = signal_mod.getsignal(signal_mod.SIGINT)
        self.addCleanup(signal_mod.signal, signal_mod.SIGTERM, old_term)
        self.addCleanup(signal_mod.signal, signal_mod.SIGINT, old_int)

        t0 = time.monotonic()
        rc = runner.run_daemon(
            self.hub, self.host,
            gate_command=[str(stub)],
            log_dir=self.tmp / "logs",
            repo_root=REPO_ROOT,
            interval=0,
            once=True,
            reconcile=lambda *a, **kw: (steps.append(1), fleetd.ReconcileResult())[1],
        )
        elapsed = time.monotonic() - t0
        self.assertEqual(rc, 0, "a black-hole server must not change the exit code")
        self.assertEqual(len(steps), 1, "the reconcile step must still have run")
        # 3 s of slack, not 1: a `run_daemon` step also acquires the host
        # singleton, adopts, runs the toolchain check and writes logs.
        # That baseline was 0.43 s when this was measured against the
        # unfixed code (20.43 s total), so the margin over the defect is
        # still a factor of five.
        self.assert_within_budget(elapsed, 1, "one run_daemon step", slack=3.0)

    # -- the damping --------------------------------------------------- #

    def test_consecutive_failures_back_off_instead_of_paying_every_cycle(self):
        """A bounded per-cycle cost is still a per-cycle cost. Twenty
        cycles of a dead server must not be twenty attempts.

        Driven on a fake clock at `LOOP_SECONDS`, so what is measured is
        the ladder and not this machine's scheduler.
        """
        client = runner.server_client(self.hub)
        attempts: list = []
        real_register = client.register

        def counted(runner_id, body):
            attempts.append(now[0])
            return real_register(runner_id, body)

        client.register = counted
        now = [0.0]
        backoff: dict = {}
        session: dict = {}
        cycles = 20
        for _ in range(cycles):
            runner.register_cycle(client, self.host, session, lambda: {"id": self.host},
                                  lambda _m: None, backoff, lambda: now[0])
            now[0] += runner.LOOP_SECONDS

        self.assertLess(
            len(attempts), cycles,
            "with no damping every cycle pays a timeout; that is the defect",
        )
        self.assertGreaterEqual(len(attempts), 2, "it must keep trying, just not every cycle")
        gaps = [b - a for a, b in zip(attempts, attempts[1:])]
        ceiling = runner.REGISTER_BACKOFF_MAX_S + runner.LOOP_SECONDS
        self.assertLessEqual(
            max(gaps), ceiling,
            f"the longest gap between attempts was {max(gaps)}s; it must stay under "
            f"REGISTER_BACKOFF_MAX_S + one cycle ({ceiling}s), which is what keeps a "
            f"returning server registered with inside election.DEMOTION_S",
        )
        self.assertEqual(
            runner.REGISTER_BACKOFF_MAX_S, 60.0,
            "SPEC §5.3 names the ceiling: 'backoff 1->60 s with jitter'",
        )
        self.assertLess(
            ceiling, election_mod.DEMOTION_S,
            "the backoff ceiling must leave headroom under DEMOTION_S: "
            "note_registration is the only inbound signal that averts "
            "demote_unreachable",
        )
        self.assertEqual(attempts[0], 0.0, "the first cycle must attempt immediately")
        self.assertEqual(attempts[1], float(runner.LOOP_SECONDS),
                         "one failure waits nothing -- a single blip is retried on the "
                         "very next cycle, unchanged")

    def test_a_healthy_probe_does_not_clear_the_ladder_when_register_is_the_stall(self):
        """The asymmetric case the reset rule exists for.

        A server can answer `/v1/health` in a millisecond and black-hole
        `register` -- different handlers, and only the second one takes
        the store's lock. If a successful probe cleared the ladder, this
        runner would pay a full read timeout on `register` every single
        cycle forever while `fails` was reset to 0 on every single cycle.
        So only the STEADY state (probe answered AND the boot_id still
        matches, i.e. nothing needs sending) clears it.
        """
        class _HealthyButWedged:
            def __init__(self):
                self.registers = 0

            def health(self):
                return {"boot_id": "b" * 32}

            def register(self, runner_id, body):
                self.registers += 1
                raise TimeoutError("register black-holed")

        client = _HealthyButWedged()
        # A boot_id that never matches: every cycle takes the reconnect
        # path, so every cycle would attempt a register if undamped.
        session = {"boot_id": "a" * 32}
        now = [0.0]
        backoff: dict = {}
        cycles = 20
        for _ in range(cycles):
            runner.register_cycle(client, self.host, session, lambda: {"id": self.host},
                                  lambda _m: None, backoff, lambda: now[0])
            now[0] += runner.LOOP_SECONDS
        self.assertLess(
            client.registers, cycles,
            "a fast health probe must not reset the ladder for a wedged register",
        )
        self.assertGreaterEqual(client.registers, 2)

    def test_the_ladder_resets_on_success(self):
        """A backoff that survives a success is a backoff that hides the
        server coming back."""
        backoff = {"fails": 4, "not_before": 999.0}
        runner._register_backoff_note(backoff, 100.0, True, lambda _m: None)
        self.assertEqual(backoff["fails"], 0)
        self.assertFalse(runner._register_backoff_skip(backoff, 100.0))

    def test_the_ladder_is_the_documented_one(self):
        self.assertEqual(runner._register_backoff_wait_s(0), 0.0)
        self.assertEqual(runner._register_backoff_wait_s(1), 0.0)
        self.assertEqual(runner._register_backoff_wait_s(2), runner.REGISTER_BACKOFF_BASE_S)
        self.assertEqual(runner._register_backoff_wait_s(3),
                         min(2 * runner.REGISTER_BACKOFF_BASE_S,
                             runner.REGISTER_BACKOFF_MAX_S))
        self.assertEqual(runner._register_backoff_wait_s(50), runner.REGISTER_BACKOFF_MAX_S)


# --------------------------------------------------------------------- #
# 5. The enriched heartbeat (step 8)
# --------------------------------------------------------------------- #


class TestHeartbeatEnrichment(ServerFixture):
    def _seed_desired(self, gates=0):
        doc = {
            "generation": 1,
            "hosts": {self.host: {"gates": gates, "agents": 0, "enabled": True}},
            "limits": {"min_free_gb": 14, "min_free_mem_gb": 8},
        }
        self.assertTrue(self.hub.create(fleetd.DESIRED_REF, doc))

    def _heartbeat(self, workers, pgid_probe):
        res = fleetd.reconcile_once(
            self.hub, self.host, workers, [str(_stub_gate(self.tmp))],
            self.tmp / "logs", REPO_ROOT,
            disk_probe=lambda: 100.0, mem_probe=lambda: 32.0,
            pgid_probe=pgid_probe,
        )
        self.assertTrue(res.heartbeat_written)
        return self.hub.read(runner.HOSTS_PREFIX + self.host)

    def test_heartbeat_carries_the_live_worker_and_the_fallback_block(self):
        """WITH A NON-EMPTY `workers` LIST, and that is the whole point.

        Both tests here used to run with `workers = []` and assert
        `hb["live_workers"] == []`, which pins the KEY's presence and
        nothing else: replacing `live_workers_payload(workers)` with
        `live_workers_payload([])` at fleetd.py's heartbeat left the
        module green (mutation M16), while merely DELETING the line was
        caught (M10). Claim 5 was tested at the registration_payload
        layer only, and `live_workers` appeared in no other test module.
        """
        self._seed_desired()
        w = self._worker()
        workers = [w]
        # `pgid_probe` is `reconcile_once`'s own seam. Reporting this
        # worker's pgid as live is what carries it PAST the reap loop and
        # into the heartbeat -- otherwise it is reaped as finished, the
        # list is empty by the time the heartbeat is built, and the test
        # would pass for exactly the reason it is here to rule out.
        hb = self._heartbeat(workers, lambda: {w.pgid})
        self.assertEqual(workers, [w], "the worker must have survived the reap")

        self.assertIn("live_workers", hb)
        self.assertEqual(len(hb["live_workers"]), 1,
                         "the heartbeat must carry the worker, not an empty list")
        lw = hb["live_workers"][0]
        self.assertEqual(
            sorted(lw), ["claim_ref", "claim_sha", "kind", "pgid", "started_at", "tag"]
        )
        self.assertEqual(lw["claim_ref"], w.claim.ref)
        self.assertEqual(lw["claim_sha"], "0" * 40)
        self.assertEqual(lw["pgid"], w.pgid)
        self.assertEqual(lw["tag"], w.tag)
        self.assertEqual(lw["kind"], "gate")
        self.assertTrue(lw["started_at"].endswith("Z") or "+" in lw["started_at"],
                        f"started_at must be claim._iso's spelling: {lw['started_at']!r}")

        self.assertIn("fallback", hb, "a FallbackHub-backed runner must report its route")
        for key in ("route", "degraded_since", "primary_failures", "fallback_reads",
                    "fallback_writes", "ambiguous_writes", "last_primary_error"):
            self.assertIn(key, hb["fallback"])
        self.assertEqual(hb["fallback"]["route"], "primary")

    def test_the_heartbeats_live_workers_is_the_in_memory_list_not_a_hub_listing(self):
        """Claim 5's negative half, at the HEARTBEAT layer (it was only
        ever argued at the payload layer).

        A claim ref that exists on the hub with no live worker behind it
        must NOT appear: `CachedHub.list()` over `refs/fleet/claims/` is
        index-served with no freshness test, and there is no CAS behind a
        liveness verdict to catch the staleness. So push a claim ref, run
        the step with an EMPTY worker list, and require the heartbeat to
        report the in-memory truth (none) rather than the hub's (one).
        """
        self._seed_desired()
        w = self._worker(tag="ghost")
        self.assertTrue(
            self.hub.create(w.claim.ref, {"host": self.host, "tag": "ghost"}),
            "precondition: the claim ref really is on the hub",
        )
        self.assertIsNotNone(self.hub.sha(w.claim.ref))

        hb = self._heartbeat([], lambda: set())
        self.assertEqual(
            hb["live_workers"], [],
            "a claim ref on the hub with no live worker must not appear in the "
            "heartbeat: the listing is index-served and has no CAS behind it",
        )

    def test_a_hubless_runner_reports_no_fallback_block_at_all(self):
        """Absent, not present-and-null: there is no fallback to report on,
        and a null would read as "healthy primary"."""
        plain = Hub(url=str(self.bare), workdir=str(self.tmp / "plain2"),
                    code_url=str(self.bare))
        doc = {
            "generation": 1,
            "hosts": {"plainhost": {"gates": 0, "agents": 0, "enabled": True}},
            "limits": {"min_free_gb": 14, "min_free_mem_gb": 8},
        }
        cur = plain.sha(fleetd.DESIRED_REF)
        if cur is None:
            self.assertTrue(plain.create(fleetd.DESIRED_REF, doc))
        else:
            self.assertTrue(plain.update(fleetd.DESIRED_REF, doc, cur))
        res = fleetd.reconcile_once(
            plain, "plainhost", [], [str(_stub_gate(self.tmp))],
            self.tmp / "logs", REPO_ROOT,
            disk_probe=lambda: 100.0, mem_probe=lambda: 32.0,
        )
        self.assertTrue(res.heartbeat_written)
        hb = plain.read(runner.HOSTS_PREFIX + "plainhost")
        self.assertNotIn("fallback", hb)
        # `live_workers` is pinned WITH CONTENT by the two tests above;
        # here it is only the key, because this test's subject is the
        # absent `fallback` block and its hub has no worker on it.
        self.assertIn("live_workers", hb)


if __name__ == "__main__":
    unittest.main()
