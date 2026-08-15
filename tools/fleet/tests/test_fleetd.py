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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import cli
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
