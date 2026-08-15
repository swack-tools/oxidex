"""fleetd + cli tests against a throwaway fixture hub.

The gate is a stub shell script that sleeps until told to finish -- unit
tests never build Rust (FLEET_PLAN.md: "mock the gate"). Probes (disk,
mem, pgids) are injected so nothing here depends on the host machine.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import cli
import claim as claim_mod
import fleetd
from fleetlib import Hub

HUB_TIP_REF = "refs/heads/refactor/tag-machinery"


def make_fixture_hub(tmp: Path) -> tuple:
    """A bare hub with one commit on the tip and one staging branch."""
    assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
    bare = tmp / "hub.git"
    subprocess.run(["git", "init", "-q", "--bare", str(bare)], check=True)
    work = tmp / "seed"
    subprocess.run(["git", "init", "-q", str(work)], check=True)
    env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
           "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
    (work / "f.txt").write_text("tip\n")
    subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "tip"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "push", "-q", str(bare), f"HEAD:{HUB_TIP_REF}"],
                   check=True, env=env)
    # one branch with a real change so the queue sees a non-ancestor
    (work / "g.txt").write_text("branch\n")
    subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "branch work"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "push", "-q", str(bare),
                    "HEAD:refs/heads/staging/one"], check=True, env=env)
    return bare, work


def make_stub_gate(tmp: Path) -> Path:
    """A gate that parks until its stop-file appears, so tests control
    exactly when a 'gate' finishes."""
    stub = tmp / "stub-gate.sh"
    stub.write_text(
        "#!/bin/bash\n"
        f"STOP={tmp}/stop-$2\n"
        'while [ ! -f "$STOP" ]; do sleep 0.2; done\n'
        "exit 0\n"
    )
    stub.chmod(0o755)
    return stub


class FleetdBase(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.bare, self.seed = make_fixture_hub(self.tmp)
        self.hub = Hub(str(self.bare), workdir=self.tmp / "hubcache")
        self.stub = make_stub_gate(self.tmp)
        self.workers = []
        self.host = "testhost"
        os.environ["FLEET_HOST"] = self.host

    def tearDown(self):
        # finish any parked stub gates so nothing outlives the test
        for w in self.workers:
            (self.tmp / f"stop-{w.tag}").write_text("")
        deadline = time.time() + 10
        while time.time() < deadline and any(w.alive() for w in self.workers):
            time.sleep(0.2)
        for w in self.workers:
            if w.popen is not None:
                w.popen.wait(timeout=10)
        self.tmpdir.cleanup()
        os.environ.pop("FLEET_HOST", None)

    def reconcile(self, disk=100.0, mem=32.0):
        return fleetd.reconcile_once(
            self.hub, self.host, self.workers,
            gate_command=[str(self.stub)],
            log_dir=self.tmp / "logs",
            repo_root=Path(__file__).resolve().parents[3],
            disk_probe=lambda: disk,
            mem_probe=lambda: mem,
        )

    def set_desired(self, gates, enabled=True, reason=None, limits=None):
        doc = {
            "generation": 1,
            "hosts": {self.host: {"gates": gates, "agents": 0, "enabled": enabled,
                                  **({"reason": reason} if reason else {})}},
            "limits": limits or {"min_free_gb": 14, "min_free_mem_gb": 8},
        }
        cur = self.hub.sha(fleetd.DESIRED_REF)
        if cur is None:
            self.assertTrue(self.hub.create(fleetd.DESIRED_REF, doc))
        else:
            self.assertTrue(self.hub.update(fleetd.DESIRED_REF, doc, cur))


class TestConvergence(FleetdBase):
    def test_starts_gate_toward_desired_and_drains_without_killing(self):
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")
        self.assertEqual(len(self.workers), 1)
        self.assertTrue(self.workers[0].alive(), "stub gate should be parked and alive")
        # gate claim exists on the hub
        claims = self.hub.list("refs/fleet/claims/gate/")
        self.assertEqual(len(claims), 1)

        # Drain to 0: the running stub must NOT be killed.
        self.set_desired(gates=0)
        res2 = self.reconcile()
        self.assertEqual(res2.started, [])
        self.assertTrue(self.workers[0].alive(), "drain must never kill live work")

        # Let it finish; next reconcile reaps it and releases the claim.
        (self.tmp / f"stop-{self.workers[0].tag}").write_text("")
        self.workers[0].popen.wait(timeout=15)
        res3 = self.reconcile()
        self.assertEqual(len(res3.finished), 1)
        self.assertEqual(self.workers, [])
        self.assertEqual(len(self.hub.list("refs/fleet/claims/gate/")), 0,
                         "finished worker's claim must be released")

    def test_disabled_starts_nothing(self):
        self.set_desired(gates=3, enabled=False, reason="quarantine test")
        res = self.reconcile()
        self.assertEqual(res.started, [])
        self.assertIn("disabled", [r[0] for r in res.refused])

    def test_low_disk_refuses_to_start(self):
        self.set_desired(gates=1)
        res = self.reconcile(disk=5.0)
        self.assertEqual(res.started, [])
        self.assertIn("limits", [r[0] for r in res.refused])

    def test_unknown_mem_probe_does_not_block(self):
        self.set_desired(gates=1)
        res = self.reconcile(mem=-1.0)
        self.assertEqual(len(res.started), 1,
                         "an unanswerable probe must not idle a healthy host")

    def test_heartbeat_written_and_down_rendering(self):
        self.set_desired(gates=0)
        res = self.reconcile()
        self.assertTrue(res.heartbeat_written)
        hb = self.hub.read(fleetd.HOSTS_PREFIX + self.host)
        self.assertEqual(hb["gates_running"], 0)
        self.assertEqual(hb["owning_user"], fleetd.owning_user())

        # Backdate the heartbeat past HEARTBEAT_STALE -> cli renders DOWN.
        hb["ts"] = "2020-01-01T00:00:00Z"
        cur = self.hub.sha(fleetd.HOSTS_PREFIX + self.host)
        self.assertTrue(self.hub.update(fleetd.HOSTS_PREFIX + self.host, hb, cur))
        import contextlib, io
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status"])
        out = buf.getvalue()
        self.assertIn("DOWN", out)


class TestDesiredCAS(FleetdBase):
    def test_concurrent_edits_compose(self):
        """Two operators race `fleet up` for different hosts: both edits
        must land (the loser re-reads and reapplies). Sequential here but
        exercising the real conflict path by pre-moving the ref between
        one caller's read and write via the mutate hook."""
        self.set_desired(gates=1)

        sneak = {"done": False}
        orig_read = cli.Hub.read

        def racing_read(hub_self, ref):
            doc = orig_read(hub_self, ref)
            # After caller A reads, sneak in operator B's committed edit
            # once, so A's first CAS write hits a stale expect_sha.
            if ref == cli.DESIRED_REF and not sneak["done"]:
                sneak["done"] = True
                cur = self.hub.sha(cli.DESIRED_REF)
                d2 = orig_read(self.hub, cli.DESIRED_REF)
                d2["hosts"]["otherhost"] = {"gates": 2, "enabled": True}
                d2["generation"] += 1
                assert self.hub.update(cli.DESIRED_REF, d2, cur)
            return doc

        cli.Hub.read = racing_read
        try:
            rc = cli.main(["--hub", str(self.bare), "up", self.host, "--gates", "4"])
        finally:
            cli.Hub.read = orig_read
        self.assertEqual(rc, 0)
        final = self.hub.read(cli.DESIRED_REF)
        self.assertEqual(final["hosts"][self.host]["gates"], 4, "our edit landed")
        self.assertEqual(final["hosts"]["otherhost"]["gates"], 2, "racer's edit survived")


if __name__ == "__main__":
    unittest.main()


class TestAgentSlots(FleetdBase):
    def make_branch_stale(self):
        """Advance the tip past `staging/one` so the branch has real DRIFT.

        `make_fixture_hub` builds staging/one as a commit ON TOP of the tip,
        so the branch already contains the tip -- correct for the gate tests
        that share this fixture, but the exact condition ARCH-FIX R5's
        dispatch preflight refuses to buy an agent for (`no-drift`: there is
        nothing to converge). An agent test needs a branch the tip has moved
        past, so this makes one.
        """
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
               "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        base = subprocess.run(
            ["git", "-C", str(self.seed), "rev-parse", "HEAD~1"],
            capture_output=True, text=True, check=True).stdout.strip()
        subprocess.run(["git", "-C", str(self.seed), "checkout", "-q", "-B", "tipwork", base],
                       check=True, env=env)
        (self.seed / "tipmoved.txt").write_text("tip moved\n")
        subprocess.run(["git", "-C", str(self.seed), "add", "."], check=True, env=env)
        subprocess.run(["git", "-C", str(self.seed), "commit", "-qm", "tip moves on"],
                       check=True, env=env)
        subprocess.run(["git", "-C", str(self.seed), "push", "-qf", str(self.bare),
                        f"HEAD:{HUB_TIP_REF}"], check=True, env=env)

    def test_agent_spawns_with_stub_cli_and_reaps(self):
        self.make_branch_stale()
        stub = self.tmp / "stub-cli.sh"
        stub.write_text("#!/bin/bash\necho stub agent ran\nexit 0\n")
        stub.chmod(0o755)
        os.environ["FLEET_AGENT_CLI_OVERRIDE"] = str(stub)
        try:
            doc = {"generation": 1,
                   "hosts": {self.host: {"gates": 0, "agents": 1, "enabled": True}},
                   "limits": {}}
            cur = self.hub.sha(fleetd.DESIRED_REF)
            (self.hub.create(fleetd.DESIRED_REF, doc) if cur is None
             else self.hub.update(fleetd.DESIRED_REF, doc, cur))
            res = self.reconcile()
            self.assertEqual(len(res.started), 1, f"agent should start: {res.refused}")
            self.assertEqual(self.workers[0].kind, "agent")
            self.assertEqual(len(self.hub.list("refs/fleet/claims/agent/")), 1)
            # worker exits (stub pushes nothing -> exit 7); next reconcile reaps
            self.workers[0].popen.wait(timeout=30)
            res2 = self.reconcile()
            self.assertEqual(len(res2.finished), 1)
            # cooldown: the no-progress branch is NOT respawned this loop
            # (each spawn is a paid CLI run), so its claim stays released
            self.assertEqual(res2.started, [])
            self.assertEqual(len(self.hub.list("refs/fleet/claims/agent/")), 0)
        finally:
            os.environ.pop("FLEET_AGENT_CLI_OVERRIDE", None)

    def test_no_cli_refuses_without_spawning(self):
        os.environ["FLEET_AGENT_CLI_OVERRIDE"] = ""  # falsy -> real which() on PATH
        doc = {"generation": 1,
               "hosts": {self.host: {"gates": 0, "agents": 1, "enabled": True}},
               "limits": {}}
        cur = self.hub.sha(fleetd.DESIRED_REF)
        (self.hub.create(fleetd.DESIRED_REF, doc) if cur is None
         else self.hub.update(fleetd.DESIRED_REF, doc, cur))
        import unittest.mock as mock
        with mock.patch("agentworker.available_clis", return_value=[]):
            res = self.reconcile()
        self.assertEqual(res.started, [])
        self.assertIn("no-agent-cli", [r[0] for r in res.refused])
        os.environ.pop("FLEET_AGENT_CLI_OVERRIDE", None)


class TestReapDeadSameHostSingleton(FleetdBase):
    """ARCH-FIX-SPEC.md FIX 2 (seam 4's red half), at the function level --
    `reap_dead_same_host_singleton` and `fleetd_marker_in_group` in
    isolation, with the process-liveness question answered by an injected
    `marker_probe` rather than a real `ps` listing. The heavy end of this
    (a real SIGKILL, a real supervisor) is `test_seams.py`'s seam 4 and
    `test_adoption.py`'s `TestRestartAdoption`; this file is the fast,
    deterministic half."""

    def singleton_ref(self) -> str:
        return claim_mod.claim_ref("host", self.host)

    def write_singleton(self, holder_host, pgid, expired=False) -> str:
        now = datetime.now(timezone.utc)
        expires = now - timedelta(seconds=5) if expired else now + timedelta(seconds=600)
        payload = {
            "holder_host": holder_host,
            "pid": 424242,
            "pgid": pgid,
            "work_kind": "fleetd",
            "work_key": holder_host,
            "started_at": now.isoformat(),
            "expires_at": expires.isoformat(),
            "gate_version": "",
            "rustc_id": "r",
            "platform_id": "p",
        }
        self.assertTrue(self.hub.create(self.singleton_ref(), payload))
        return self.singleton_ref()

    def test_reaps_when_the_process_is_provably_dead(self):
        ref = self.write_singleton(self.host, pgid=999999)
        reaped = fleetd.reap_dead_same_host_singleton(
            self.hub, self.host, ref, own_pid=os.getpid(),
            marker_probe=lambda pgid, exclude_pid: None,
        )
        self.assertTrue(reaped)
        self.assertIsNone(self.hub.sha(ref), "the stale claim must be gone")

    def test_refuses_when_a_marker_is_still_alive(self):
        ref = self.write_singleton(self.host, pgid=999999)
        reaped = fleetd.reap_dead_same_host_singleton(
            self.hub, self.host, ref, own_pid=os.getpid(),
            marker_probe=lambda pgid, exclude_pid: "python3 fleetd.py --hub ...",
        )
        self.assertFalse(reaped)
        self.assertIsNotNone(self.hub.sha(ref), "a live claim must not be touched")

    def test_refuses_a_different_hosts_claim_even_if_dead(self):
        """Not ours to reason about, same rule as `Claim.adopt`."""
        ref = self.write_singleton("some-other-host", pgid=999999)
        called = []

        def probe(pgid, exclude_pid):
            called.append(pgid)
            return None
        reaped = fleetd.reap_dead_same_host_singleton(
            self.hub, self.host, ref, own_pid=os.getpid(), marker_probe=probe,
        )
        self.assertFalse(reaped)
        self.assertIsNotNone(self.hub.sha(ref))
        self.assertEqual(called, [], "another host's claim must never even reach the ps probe")

    def test_refuses_without_a_usable_pgid(self):
        ref = self.write_singleton(self.host, pgid=None)
        called = []

        def probe(pgid, exclude_pid):
            called.append(pgid)
            return None
        reaped = fleetd.reap_dead_same_host_singleton(
            self.hub, self.host, ref, own_pid=os.getpid(), marker_probe=probe,
        )
        self.assertFalse(reaped, "no pgid is evidence of nothing -- refuse, don't guess")
        self.assertEqual(called, [])

    def test_refuses_when_the_ref_is_already_gone(self):
        ref = self.singleton_ref()
        self.assertIsNone(self.hub.sha(ref))
        reaped = fleetd.reap_dead_same_host_singleton(
            self.hub, self.host, ref, own_pid=os.getpid(),
            marker_probe=lambda pgid, exclude_pid: None,
        )
        self.assertFalse(reaped)

    def test_reap_is_cas_and_loses_to_a_concurrent_renewal(self):
        """Even when the probe says 'dead', a stale-sha CAS must not clobber
        a payload that changed underneath it (e.g. a renewal that landed
        between the read and the delete)."""
        ref = self.write_singleton(self.host, pgid=999999)
        stale_sha = self.hub.sha(ref)
        # Simulate a concurrent renewal: rewrite the payload, moving the sha.
        now = datetime.now(timezone.utc)
        renewed_payload = {
            "holder_host": self.host, "pid": 424242, "pgid": 999999,
            "work_kind": "fleetd", "work_key": self.host,
            "started_at": now.isoformat(),
            "expires_at": (now + timedelta(seconds=600)).isoformat(),
            "gate_version": "", "rustc_id": "r", "platform_id": "p",
        }
        self.assertTrue(self.hub.update(ref, renewed_payload, stale_sha))

        import unittest.mock as mock
        with mock.patch.object(fleetd.Hub, "sha", return_value=stale_sha):
            reaped = fleetd.reap_dead_same_host_singleton(
                self.hub, self.host, ref, own_pid=os.getpid(),
                marker_probe=lambda pgid, exclude_pid: None,
            )
        self.assertFalse(reaped, "a CAS against a stale sha must fail, not clobber the renewal")
        self.assertIsNotNone(self.hub.sha(ref), "the renewed claim must survive")


class TestFleetdMarkerInGroup(unittest.TestCase):
    """`fleetd_marker_in_group` against REAL processes -- the whole point of
    this function is that fleetd shares its wrapper's process group and so
    is never that group's LEADER (see the function's docstring), so a fake
    that only exercises the leader case would not test the fix at all."""

    def setUp(self):
        self.procs: list = []

    def tearDown(self):
        for p in self.procs:
            try:
                p.terminate()
                p.wait(timeout=5)
            except Exception:
                pass

    def spawn_group_with_nonleader_marker(self) -> tuple:
        """A bash leader (never matching "fleetd.py") that backgrounds a
        python child WITHOUT setsid -- exactly `fleetd-wrapper.sh`'s shape
        -- so the child shares the leader's pgid but is not it.

        The leader script is a FILE, invoked by path (`bash script.sh`),
        not `bash -c "<script text>"`: `ps` reports a process's argv, and
        `-c`'s script text IS argv for that process, so a marker string
        embedded in an inline `-c` script would spuriously match the
        LEADER too and this test would not be exercising the non-leader
        case it exists to check. A script file's own contents are never
        part of its invoker's command line, matching how
        `fleetd-wrapper.sh` actually runs (`bash /path/to/wrapper.sh`).
        """
        script = self.pidfile.parent / "leader.sh"
        script.write_text(
            "#!/bin/bash\n"
            f'{sys.executable} -c "import time; time.sleep(30)" fleetd.py-marker & '
            f'echo $! > "{self.pidfile}"\n'
            "wait\n"
        )
        script.chmod(0o755)
        p = subprocess.Popen(["bash", str(script)], start_new_session=True)
        self.procs.append(p)
        deadline = time.time() + 10
        while time.time() < deadline and not self.pidfile.exists():
            time.sleep(0.1)
        child_pid = int(self.pidfile.read_text().strip())
        pgid = os.getpgid(p.pid)
        self.assertEqual(pgid, p.pid, "sanity: bash must be the group leader here")
        self.assertEqual(os.getpgid(child_pid), pgid,
                         "sanity: the backgrounded child must share bash's pgid, unset-sid")
        return pgid, child_pid

    def test_finds_a_matching_process_that_is_not_the_group_leader(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.pidfile = Path(self.tmpdir.name) / "child.pid"
        pgid, child_pid = self.spawn_group_with_nonleader_marker()
        try:
            found = fleetd.fleetd_marker_in_group(pgid)
            self.assertIsNotNone(
                found, "the marker lives on a non-leader member; a leader-only "
                       "scan (fleet_worker_pgids's filter) would miss it entirely")
            self.assertIn("fleetd.py", found)
        finally:
            self.tmpdir.cleanup()

    def test_excluding_the_only_matching_pid_reports_no_match(self):
        """The self-match guard: if the only 'fleetd.py'-looking member IS
        the caller (`exclude_pid`), that is not evidence of a live
        predecessor -- it is the successor looking at itself."""
        self.tmpdir = tempfile.TemporaryDirectory()
        self.pidfile = Path(self.tmpdir.name) / "child.pid"
        pgid, child_pid = self.spawn_group_with_nonleader_marker()
        try:
            found = fleetd.fleetd_marker_in_group(pgid, exclude_pid=child_pid)
            self.assertIsNone(found)
        finally:
            self.tmpdir.cleanup()

    def test_a_pgid_with_no_members_at_all_is_reported_as_no_match(self):
        # A pgid astronomically unlikely to be live on any real host.
        found = fleetd.fleetd_marker_in_group(999_999_9)
        self.assertIsNone(found)
