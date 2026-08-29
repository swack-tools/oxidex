"""keel-runner core tests (PLAN Stage 3 task 1; SPEC §2 C7, §9).

Three properties of the fleetd -> keel/runner.py split, each pinned
against the code that MOVED rather than against fleetd's re-exports:

1. OFFLINE PARITY. A runner with no server configured behaves exactly
   as fleetd does today: `build_hub` with no `server_url` returns the
   plain `fleetlib.Hub` (never a `FallbackHub` around a phantom
   primary), and `runner.run_daemon` -- the moved singleton + adoption
   + bounded-failure loop -- drives a full claim / spawn-stub-gate /
   reap / release cycle through the REAL `reconcile_once` against a
   fixture hub. The daemon shell is exercised in-process the way
   `test_fleetd.TestMainLoopSurvivesHubErrors` drives `fleetd.main`,
   but with the real step function and a real parked gate process.

2. RECONCILE ORDER (SPEC I5). Through the runner's own entry point
   (`runner.reconcile_once`), a lost-lease kill happens BEFORE any hub
   read. The instrument: a recording hub proxy whose reads sample the
   victim's liveness at the moment of the read and then raise
   `HubUnreachableError` (the hub is down -- the very condition that
   loses leases). Green = every read the step attempted saw the victim
   already dead. A negative control runs a deliberately read-first step
   against the same proxy and asserts the instrument DOES record an
   alive-at-read event for it -- so a regression to the historical
   ordering cannot pass by the instrument simply being blind.

3. DAEMON MARKERS. Both host-scheduler entry points hold the same
   `refs/fleet/claims/host/<host>` singleton during the migration, so
   `fleetd_marker_in_group`'s default must recognize a live
   `keel/runner.py` process as well as a live `fleetd.py` one --
   otherwise a successor fast-reaps a LIVE runner's singleton between
   renewals. The negative control (`marker=FLEETD_MARKER`) is the
   bug-present shape and must stay red.

The gate is a stub shell script that parks until told to finish (the
same shape as test_fleetd's); nothing here builds Rust.
"""

from __future__ import annotations

import os
import signal as signal_mod
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from _env import HermeticCase, scrub_env  # noqa: E402
from _fixtures import make_hub  # noqa: E402

import claim as claim_mod  # noqa: E402
import fleetd  # noqa: E402
import keel.runner as runner  # noqa: E402
from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402
from keel.fallbackhub import FallbackHub  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402

HUB_TIP_REF = "refs/heads/refactor/tag-machinery"
REPO_ROOT = Path(__file__).resolve().parents[3]


def make_fixture_hub(tmp: Path) -> tuple:
    """A bare hub with one commit on the tip and one staging branch --
    test_fleetd's fixture shape."""
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
    return bare, work


def make_stub_gate(tmp: Path) -> Path:
    """A gate that parks until its stop-file appears."""
    stub = tmp / "stub-gate.sh"
    stub.write_text(
        "#!/bin/bash\n"
        f"STOP={tmp}/stop-$2\n"
        'while [ ! -f "$STOP" ]; do sleep 0.2; done\n'
        "exit 0\n"
    )
    stub.chmod(0o755)
    return stub


class RunnerFixture(HermeticCase):
    """Fixture hub + stub gate + worker bookkeeping, shared by the
    daemon-shell and order tests below."""

    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.bare, self.seed = make_fixture_hub(self.tmp)
        self.hub = make_hub(self, str(self.bare), workdir=self.tmp / "hubcache")
        self.stub = make_stub_gate(self.tmp)
        self.log_dir = self.tmp / "logs"
        self.host = "testhost"
        self.workers: list = []
        # run_daemon installs SIGTERM/SIGINT handlers in-process; restore
        # the suite's own afterwards (test_fleetd's main-loop tests do the
        # same).
        self._old_term = signal_mod.getsignal(signal_mod.SIGTERM)
        self._old_int = signal_mod.getsignal(signal_mod.SIGINT)
        self.addCleanup(signal_mod.signal, signal_mod.SIGTERM, self._old_term)
        self.addCleanup(signal_mod.signal, signal_mod.SIGINT, self._old_int)

    def tearDown(self):
        for w in self.workers:
            (self.tmp / f"stop-{w.tag}").write_text("")
        deadline = time.time() + 10
        while time.time() < deadline and any(w.alive() for w in self.workers):
            time.sleep(0.2)
        for w in self.workers:
            if w.popen is not None:
                try:
                    w.popen.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    pass
        self.tmpdir.cleanup()

    def set_desired(self, gates: int, enabled: bool = True):
        doc = {
            "generation": 1,
            "hosts": {self.host: {"gates": gates, "agents": 0, "enabled": enabled}},
            "limits": {"min_free_gb": 14, "min_free_mem_gb": 8},
        }
        cur = self.hub.sha(fleetd.DESIRED_REF)
        if cur is None:
            self.assertTrue(self.hub.create(fleetd.DESIRED_REF, doc))
        else:
            self.assertTrue(self.hub.update(fleetd.DESIRED_REF, doc, cur))


# --------------------------------------------------------------------- #
# 1a. The hub wiring: no server configured => the plain state-repo Hub
# --------------------------------------------------------------------- #


class TestBuildHub(HermeticCase):
    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.addCleanup(self.tmpdir.cleanup)
        self.bare = self.tmp / "state.git"
        subprocess.run(["git", "init", "-q", "--bare", str(self.bare)], check=True)

    def test_no_server_configured_is_the_plain_hub_not_a_fallbackhub(self):
        """The offline default this stage must preserve: a runner with no
        server at all reconciles against the state repo directly, through
        the byte-identical `fleetlib.Hub` fleetd builds today."""
        hub = runner.build_hub(str(self.bare), workdir=self.tmp / "cache")
        self.assertIs(type(hub), Hub,
                      "no server_url must yield fleetlib.Hub itself, nothing wrapped")
        self.assertNotIsInstance(hub, FallbackHub)
        self.assertEqual(hub.url, str(self.bare))
        # code_url defaults to the hub URL, exactly like `fleetd --hub`
        # alone does (single-repo topology unchanged).
        self.assertEqual(hub.code_url, str(self.bare))

    def test_server_configured_wires_fallbackhub_server_first(self):
        token_file = self.tmp / "server.token"
        token_file.write_text("sekret-token\n")
        hub = runner.build_hub(
            str(self.bare),
            code_url=str(self.bare),
            server_url="http://127.0.0.1:1",  # never connected to here
            server_token_file=token_file,
            workdir=self.tmp / "cache",
        )
        self.assertIsInstance(hub, FallbackHub)
        self.assertIsInstance(hub.primary, ServerHub)
        self.assertIs(type(hub.github), Hub)
        # Identity is the GitHub half's (SPEC 4.3 rule 3): scope tokens,
        # _spawn_env and the GIT-CODE borrowers see the state repo URL.
        self.assertEqual(hub.url, str(self.bare))
        self.assertEqual(hub.github.url, str(self.bare))
        self.assertEqual(hub.primary._token, "sekret-token")

    def test_a_named_but_missing_token_file_fails_loud(self):
        """Naming a path is a statement that it is there (SPEC §8's rule
        for FLEET_GIT_TOKEN_FILE, applied to the server token): a runner
        silently running unauthenticated would 401 every write and read
        as a mysteriously degraded server."""
        with self.assertRaises(OSError):
            runner.build_hub(
                str(self.bare),
                server_url="http://127.0.0.1:1",
                server_token_file=self.tmp / "does-not-exist",
                workdir=self.tmp / "cache",
            )

    def test_an_empty_token_file_fails_loud(self):
        empty = self.tmp / "empty.token"
        empty.write_text("\n")
        with self.assertRaises(OSError):
            runner.build_hub(
                str(self.bare),
                server_url="http://127.0.0.1:1",
                server_token_file=empty,
                workdir=self.tmp / "cache",
            )

    def test_main_without_any_hub_url_exits_2(self):
        """fleetd's own contract, kept: no hub URL anywhere (flag, env
        -- scrubbed by HermeticCase -- or runner.toml) is rc 2, not a
        traceback. `--runner-toml` points into the empty tempdir so the
        operator's real ~/.keel/runner.toml cannot leak in."""
        rc = runner.main(["--runner-toml", str(self.tmp / "absent.toml")])
        self.assertEqual(rc, 2)


# --------------------------------------------------------------------- #
# 1b. Offline parity: the moved daemon shell drives claim -> spawn ->
#     reap -> release through the real reconcile step
# --------------------------------------------------------------------- #


class TestRunnerOfflineParity(RunnerFixture):
    """`runner.run_daemon` against a fixture hub with a parked stub gate:
    one daemon run, scripted through its `reconcile` seam, must claim the
    branch, spawn the gate, reap it once finished, release its claim, and
    write the heartbeat -- fleetd's observable behaviour today, produced
    by the MOVED shell (singleton, adoption, loop) and the SHARED step.

    The `reconcile` callable delegates to the real `fleetd.reconcile_once`
    (with injected disk/mem probes so a low-disk dev host cannot refuse);
    between steps it performs the test's observations and stimulus, then
    stops the daemon the way a supervisor would (SIGTERM to self)."""

    def test_claim_spawn_reap_release_through_run_daemon(self):
        self.set_desired(gates=1)
        gate_claim_ref = claim_mod.claim_ref("gate", "staging-one")
        singleton_ref = claim_mod.claim_ref("host", self.host)
        seen: dict = {}
        results: list = []

        def scripted(hub, host, workers, gate_command, log_dir, repo_root, **kw):
            res = fleetd.reconcile_once(
                hub, host, workers, gate_command, log_dir, repo_root,
                disk_probe=lambda: 100.0, mem_probe=lambda: 32.0, **kw,
            )
            results.append(res)
            step = len(results)
            if step == 1:
                # CLAIM + SPAWN happened in this step.
                self.assertEqual(len(res.started), 1,
                                 f"setup failed: refused={res.refused}")
                self.assertEqual(len(workers), 1)
                w = workers[0]
                self.workers.append(w)  # tearDown safety net
                seen["tag"] = w.tag
                seen["pgid"] = w.pgid
                self.assertTrue(w.alive(), "stub gate should be parked and alive")
                payload = hub.read(gate_claim_ref)
                self.assertIsNotNone(payload, "claim-before-launch: the gate claim "
                                              "must be on the hub while the gate runs")
                self.assertEqual(payload.get("holder_host"), host)
                self.assertEqual(payload.get("work_key"), "staging/one")
                self.assertIsNotNone(hub.sha(singleton_ref),
                                     "the host singleton must be held while the daemon runs")
                # Stimulus for step 2: drain the target, finish the gate.
                doc = hub.read(fleetd.DESIRED_REF)
                doc["hosts"][host]["gates"] = 0
                self.assertTrue(hub.update(fleetd.DESIRED_REF, doc,
                                           hub.sha(fleetd.DESIRED_REF)))
                (self.tmp / f"stop-{w.tag}").write_text("")
                w.popen.wait(timeout=30)
            elif step == 2:
                # REAP + RELEASE happened in this step.
                self.assertEqual(res.finished, [seen["tag"]])
                self.assertEqual(res.started, [], "gates=0 must drain, not start")
                self.assertEqual(workers, [], "the reaped worker must leave the list")
                self.assertIsNone(hub.sha(gate_claim_ref),
                                  "the finished gate's claim must be released, "
                                  "not left to expire")
                self.assertTrue(res.heartbeat_written)
                os.kill(os.getpid(), signal_mod.SIGTERM)  # supervisor-style stop
            return res

        rc = runner.run_daemon(
            self.hub, self.host,
            gate_command=[str(self.stub)],
            log_dir=self.log_dir,
            repo_root=REPO_ROOT,
            interval=0,
            reconcile=scripted,
        )

        self.assertEqual(rc, 0, "a drained daemon must exit cleanly")
        self.assertEqual(len(results), 2)
        # The singleton is released on the way out (fleetd's `finally`).
        self.assertIsNone(self.hub.sha(singleton_ref))
        # The heartbeat is on the hub and reflects the drained state.
        hb = self.hub.read(runner.HOSTS_PREFIX + self.host)
        self.assertIsNotNone(hb)
        self.assertEqual(hb.get("gates_running"), 0)
        # And the stub is genuinely gone, by listing.
        self.assertNotIn(seen["pgid"], runner.live_pgids())

    def test_once_flag_gives_exactly_one_step(self):
        self.set_desired(gates=0)
        calls = []

        def counting(hub, host, workers, *a, **kw):
            calls.append(1)
            return fleetd.ReconcileResult()

        rc = runner.run_daemon(
            self.hub, self.host,
            gate_command=[str(self.stub)],
            log_dir=self.log_dir,
            repo_root=REPO_ROOT,
            interval=0,
            once=True,
            reconcile=counting,
        )
        self.assertEqual(rc, 0)
        self.assertEqual(len(calls), 1)


# --------------------------------------------------------------------- #
# 2. Reconcile ORDER: the lost-lease kill precedes any hub read
# --------------------------------------------------------------------- #


class RecordingHub:
    """A hub proxy that (a) delegates everything to `inner`, (b) once
    `broken` is set, makes every COORDINATION read and write raise
    `HubUnreachableError` -- the whole spine down, the exact condition
    that loses leases -- and (c) records, for every read it refuses, the
    victim worker's liveness AT THE MOMENT OF THE READ. That sample is
    the order instrument: if the step reads before it kills, at least
    one read records the victim alive."""

    _READS = ("sha", "read", "read_with_sha", "list", "code_sha", "code_list")
    _WRITES = ("create", "update", "delete")

    def __init__(self, inner):
        self._inner = inner
        self.broken = False
        self.events: list = []  # ("read"|"write", name, ref, victim_alive)
        self.victim_alive = lambda: None  # set by the test after spawn

    def __getattr__(self, name):
        # Everything not intercepted below (url, code_url, workdir,
        # push_code_ref, ...) is the inner hub's.
        return getattr(self._inner, name)

    def _guard(self, kind, name, ref):
        if self.broken:
            self.events.append((kind, name, ref, self.victim_alive()))
            raise HubUnreachableError(f"simulated outage: {name}({ref!r})")

    def sha(self, ref):
        self._guard("read", "sha", ref)
        return self._inner.sha(ref)

    def read(self, ref):
        self._guard("read", "read", ref)
        return self._inner.read(ref)

    def read_with_sha(self, ref):
        self._guard("read", "read_with_sha", ref)
        return self._inner.read_with_sha(ref)

    def list(self, prefix):
        self._guard("read", "list", prefix)
        return self._inner.list(prefix)

    def code_sha(self, ref):
        self._guard("read", "code_sha", ref)
        return self._inner.code_sha(ref)

    def code_list(self, prefix):
        self._guard("read", "code_list", prefix)
        return self._inner.code_list(prefix)

    def create(self, ref, payload):
        self._guard("write", "create", ref)
        return self._inner.create(ref, payload)

    def update(self, ref, payload, expect_sha):
        self._guard("write", "update", ref)
        return self._inner.update(ref, payload, expect_sha)

    def delete(self, ref, expect_sha):
        self._guard("write", "delete", ref)
        return self._inner.delete(ref, expect_sha)


class TestReconcileOrder(RunnerFixture):
    """SPEC I5 through the runner's own step entry point
    (`runner.reconcile_once`): local reap + lost-lease kill BEFORE any
    hub read. The kill is a real SIGTERM to a real parked process group;
    the hub outage is total (every coordination read AND write raises);
    the order is read off liveness samples taken inside the refused
    reads themselves."""

    def start_lost_worker(self, proxy):
        w = runner.start_gate(proxy, "staging/one", "order-test",
                              [str(self.stub)], self.host, self.log_dir)
        self.assertIsNotNone(w, "setup: claim should be free")
        self.workers.append(w)
        self.assertTrue(w.alive(), "stub gate should be parked and alive")
        proxy.victim_alive = w.alive
        w.claim._mark_lost("hub no longer records us as the holder")
        proxy.broken = True
        return w

    def reconcile(self, proxy, workers):
        return runner.reconcile_once(
            proxy, self.host, workers,
            gate_command=[str(self.stub)],
            log_dir=self.log_dir,
            repo_root=REPO_ROOT,
            disk_probe=lambda: 100.0,
            mem_probe=lambda: 32.0,
        )

    def test_the_lost_lease_kill_happens_before_any_hub_read(self):
        proxy = RecordingHub(self.hub)
        w = self.start_lost_worker(proxy)
        workers = [w]

        # ONE step, hub fully down. The step still raises (a wedged hub
        # must reach a human), but the kill must already have happened.
        with self.assertRaises(HubError):
            self.reconcile(proxy, workers)

        self.assertEqual(workers, [], "killed worker must leave the worker list")
        deadline = time.time() + 15
        while time.time() < deadline and w.alive():
            time.sleep(0.2)
        self.assertFalse(w.alive(), "the lost-lease gate is still running")
        self.assertNotIn(w.pgid, runner.live_pgids(),
                         "the process GROUP must be gone (M8)")

        reads = [e for e in proxy.events if e[0] == "read"]
        self.assertTrue(reads, "the step must still have attempted its hub reads "
                               "(the reorder must not become a skip)")
        alive_at_read = [(name, ref) for _, name, ref, alive in reads if alive]
        self.assertEqual(
            alive_at_read, [],
            "a hub read observed the lost-lease worker still alive -- the kill "
            f"did not precede the reads (instrument: RecordingHub liveness "
            f"samples; reads seen: {[(n, r) for _, n, r, _ in reads]})",
        )

    def test_negative_control_the_instrument_sees_a_read_first_step(self):
        """Prove the instrument can go red: a deliberately inverted step
        -- the pre-fix shape, `hub.read(DESIRED_REF)` unguarded at the
        top -- records the victim ALIVE at that read. Without this
        control, a broken instrument (liveness sampled too late, or
        reads not recorded) would pass the test above for any order."""
        proxy = RecordingHub(self.hub)
        w = self.start_lost_worker(proxy)

        def inverted_step(hub, workers):
            hub.read(fleetd.DESIRED_REF)  # raises: the hub is down
            # (the kill loop would come after -- never reached, which is
            # exactly the historical bug's symptom)

        with self.assertRaises(HubError):
            inverted_step(proxy, [w])

        reads = [e for e in proxy.events if e[0] == "read"]
        self.assertTrue(reads)
        self.assertTrue(
            any(alive for _, _, _, alive in reads),
            "the instrument failed to observe the victim alive during a "
            "read-first step -- it could not catch an order regression",
        )
        # Clean up the still-parked gate (this control never killed it).
        (self.tmp / f"stop-{w.tag}").write_text("")


# --------------------------------------------------------------------- #
# 2b. spawn_allowed disarms the STARTS and nothing else
# --------------------------------------------------------------------- #


class TestSpawnAllowedDisarmsStartsOnly(RunnerFixture):
    """Keel 3R-2 step 4's load-bearing half, and the one nothing tested.

    `reconcile_once`'s docstring argues it at length: "`spawn_allowed=
    False` short-circuits step 3 ONLY. Steps 1 and 2 run in full, and that
    is the whole point [...] Disarming step 1 alongside step 3 would turn
    a conservative 'start nothing' into 'start nothing and stop nothing',
    which is strictly worse than the rc-5 refusal this replaced."

    That was prose. MEASURED at 578141ed in a detached worktree: inserting
    `if not spawn_allowed: return res` immediately after
    `res = ReconcileResult()` in `fleetd.reconcile_once` -- skipping the
    reap AND the lost-lease kill along with the starts -- left
    `test_journal.TestOfflineStartThroughRunDaemon` at 5 tests, OK. No
    test in the suite drove a reapable or a lease-lost worker through a
    `spawn_allowed=False` cycle, so the whole offline test class was blind
    to the strictly-worse direction while pinning the conservative one.

    This is that cycle: one worker whose process group has already exited
    (must be REAPED) and one whose lease is lost (must be KILLED), through
    a single `spawn_allowed=False` step.
    """

    def test_spawn_allowed_false_still_reaps_and_still_kills(self):
        self.set_desired(gates=2)
        finished = runner.start_gate(self.hub, "staging/one", "reap-me",
                                     [str(self.stub)], self.host, self.log_dir)
        lost = runner.start_gate(self.hub, "staging/two", "kill-me",
                                 [str(self.stub)], self.host, self.log_dir)
        self.assertIsNotNone(finished, "setup: staging/one must be claimable")
        self.assertIsNotNone(lost, "setup: staging/two must be claimable")
        self.workers.extend([finished, lost])

        # (a) a worker whose process group is already gone.
        (self.tmp / f"stop-{finished.tag}").write_text("")
        deadline = time.time() + 20
        while time.time() < deadline and finished.alive():
            time.sleep(0.1)
        self.assertFalse(finished.alive(), "setup: the finished gate must exit")
        # (b) a worker whose lease went LOST while its group still runs.
        self.assertTrue(lost.alive(), "setup: the second gate must still be parked")
        lost.claim._mark_lost("hub no longer records us as the holder")

        workers = [finished, lost]
        res = runner.reconcile_once(
            self.hub, self.host, workers,
            gate_command=[str(self.stub)], log_dir=self.log_dir,
            repo_root=REPO_ROOT, disk_probe=lambda: 100.0, mem_probe=lambda: 32.0,
            spawn_allowed=False)

        self.assertEqual(
            res.finished, [finished.tag],
            f"REAP DISARMED ALONGSIDE THE STARTS: a spawn_allowed=False cycle "
            f"stopped reaping finished work -- {res}")
        self.assertEqual(
            [t for t, _ in res.killed], [lost.tag],
            f"LOST-LEASE KILL DISARMED ALONGSIDE THE STARTS: an unleased gate "
            f"survived a spawn_allowed=False cycle, which is the duplicate-gate "
            f"hazard the kill exists to prevent -- {res}")
        self.assertEqual(workers, [], "both workers must leave the list")
        deadline = time.time() + 15
        while time.time() < deadline and lost.alive():
            time.sleep(0.2)
        self.assertFalse(lost.alive(), "the unleased gate's group must be gone")

        # And the starts ARE refused -- the conservative half, so this test
        # cannot pass by `spawn_allowed` having stopped working entirely.
        self.assertEqual(res.started, [], "spawn_allowed=False must start nothing")
        self.assertIn("offline-no-spawn", [r for r, _ in res.refused],
                      f"the refusal must be named: {res.refused}")


# --------------------------------------------------------------------- #
# 3. Daemon markers: both entry points are recognized as live schedulers
# --------------------------------------------------------------------- #


class TestDaemonMarkers(HermeticCase):
    """`fleetd_marker_in_group`'s default must match a live
    `keel/runner.py` process as well as a live `fleetd.py` one: both hold
    the SAME host-singleton ref during the migration, and a probe blind
    to one of them lets a successor fast-reap a live scheduler's claim
    between renewals (the false-reap this probe exists to prevent)."""

    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.addCleanup(self.tmpdir.cleanup)

    def spawn_marked(self, marker_arg: str):
        """A parked process whose argv carries `marker_arg`, in its own
        session/group (so the probe scans a group that is really there)."""
        p = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)", marker_arg],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            stdin=subprocess.DEVNULL, start_new_session=True,
        )
        self.addCleanup(self._reap, p)
        # Let `ps` see it.
        deadline = time.time() + 10
        while time.time() < deadline and p.pid not in runner.live_pgids():
            time.sleep(0.05)
        return p

    @staticmethod
    def _reap(p):
        if p.poll() is None:
            p.kill()
        try:
            p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            pass

    def test_a_live_runner_py_process_is_recognized_by_default(self):
        p = self.spawn_marked("tools/fleet/keel/runner.py")
        found = runner.fleetd_marker_in_group(p.pid)
        self.assertIsNotNone(
            found, "the default DAEMON_MARKERS probe must see a live "
                   "keel/runner.py-argv member of the group")
        self.assertIn("keel/runner.py", found)

    def test_negative_control_the_old_fleetd_only_marker_is_blind_to_it(self):
        """The bug-present shape, kept red on purpose: probing the same
        live runner group with fleetd.py's old single-marker default
        finds nothing -- which is what made the reap below unsafe before
        DAEMON_MARKERS existed."""
        p = self.spawn_marked("tools/fleet/keel/runner.py")
        self.assertIsNone(
            runner.fleetd_marker_in_group(p.pid, marker=runner.FLEETD_MARKER))

    def test_a_live_fleetd_process_is_still_recognized(self):
        p = self.spawn_marked("tools/fleet/fleetd.py")
        self.assertIsNotNone(runner.fleetd_marker_in_group(p.pid))

    def test_reap_refuses_a_live_runner_and_reaps_it_once_dead(self):
        """The probe wired into the actual reap: a same-host singleton
        claim naming a LIVE keel/runner.py group is refused; the same
        claim is reaped once the group is provably gone."""
        bare = self.tmp / "state.git"
        subprocess.run(["git", "init", "-q", "--bare", str(bare)], check=True)
        hub = Hub(str(bare), workdir=str(self.tmp / "cache"))
        host = "testhost"
        ref = claim_mod.claim_ref("host", host)
        p = self.spawn_marked("tools/fleet/keel/runner.py")
        self.assertTrue(hub.create(ref, {"holder_host": host, "pgid": p.pid}))

        self.assertFalse(
            runner.reap_dead_same_host_singleton(hub, host, ref),
            "a claim whose recorded group holds a live keel/runner.py "
            "member must never be reaped early")
        self.assertIsNotNone(hub.sha(ref))

        p.kill()
        p.wait(timeout=10)
        deadline = time.time() + 10
        while time.time() < deadline and p.pid in runner.live_pgids():
            time.sleep(0.05)

        self.assertTrue(
            runner.reap_dead_same_host_singleton(hub, host, ref),
            "a provably-dead same-host group's claim must be reaped "
            "without waiting out the TTL")
        self.assertIsNone(hub.sha(ref))


if __name__ == "__main__":
    unittest.main()
