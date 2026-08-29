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
     the daemon still completes its reconcile.
  4. A changed `boot_id` re-registers EXACTLY ONCE, and an unchanged one
     re-registers never. `TestReconnectNegativeControl` disables the
     comparison and requires that test to go RED -- a reconnect test that
     cannot fail proves nothing.
  5. The heartbeat carries `live_workers[]` (from the IN-MEMORY workers
     list, never a hub claim listing) and the `fallback` block.
"""

from __future__ import annotations

import signal as signal_mod
import subprocess
import sys
import tempfile
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
        self.assertIs(runner.server_client(self.hub), self.hub.primary)
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
    def _worker(self, tag="t1", kind="gate"):
        c = claim_mod.Claim(self.hub, "gate", "staging-one", holder_host=self.host)
        c.ref  # touch: the ref is computed in __init__
        c._sha = "0" * 40
        c._started_at = claim_mod.datetime.now(claim_mod.timezone.utc)
        return runner.Worker(branch="staging/one", tag=tag, pgid=4242, claim=c, kind=kind)

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
# 5. The enriched heartbeat (step 8)
# --------------------------------------------------------------------- #


class TestHeartbeatEnrichment(ServerFixture):
    def test_heartbeat_carries_live_workers_and_the_fallback_block(self):
        doc = {
            "generation": 1,
            "hosts": {self.host: {"gates": 0, "agents": 0, "enabled": True}},
            "limits": {"min_free_gb": 14, "min_free_mem_gb": 8},
        }
        self.assertTrue(self.hub.create(fleetd.DESIRED_REF, doc))
        workers: list = []
        res = fleetd.reconcile_once(
            self.hub, self.host, workers, [str(_stub_gate(self.tmp))],
            self.tmp / "logs", REPO_ROOT,
            disk_probe=lambda: 100.0, mem_probe=lambda: 32.0,
        )
        self.assertTrue(res.heartbeat_written)
        hb = self.hub.read(runner.HOSTS_PREFIX + self.host)
        self.assertIn("live_workers", hb)
        self.assertEqual(hb["live_workers"], [])
        self.assertIn("fallback", hb, "a FallbackHub-backed runner must report its route")
        for key in ("route", "degraded_since", "primary_failures", "fallback_reads",
                    "fallback_writes", "ambiguous_writes", "last_primary_error"):
            self.assertIn(key, hb["fallback"])
        self.assertEqual(hb["fallback"]["route"], "primary")

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
        self.assertIn("live_workers", hb)


if __name__ == "__main__":
    unittest.main()
