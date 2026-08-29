"""fleetd + cli tests against a throwaway fixture hub.

The gate is a stub shell script that sleeps until told to finish -- unit
tests never build Rust (FLEET_PLAN.md: "mock the gate"). Probes (disk,
mem, pgids) are injected so nothing here depends on the host machine.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from _env import HermeticCase, scrub_env  # noqa: E402
from _fixtures import break_hub, make_hub, within_sweep  # noqa: E402

import cli
import claim as claim_mod
import fleetd
# The module `live_pgids` and `ProcessListingUnavailable` actually LIVE in.
# `fleetd` re-exports both, but `TestLivePgidsRefusesToGuess` patches
# `subprocess.run` in the defining module's namespace, which is the only
# one the probe itself resolves.
import keel.runner as runner_mod
from fleetlib import Hub

HUB_TIP_REF = "refs/heads/refactor/tag-machinery"


def make_fixture_hub(tmp: Path) -> tuple:
    """A bare hub with one commit on the tip and one staging branch."""
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
    # one branch with a real change so the queue sees a non-ancestor
    (work / "g.txt").write_text("branch\n")
    subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "branch work"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "push", "-q", str(bare),
                    "HEAD:refs/heads/staging/one"], check=True, env=env)
    return bare, work


# --------------------------------------------------------------------- #
# Waiting for real processes
# --------------------------------------------------------------------- #
#
# Every wait in this file waits on a REAL child -- a stub gate noticing its
# stop-file, a spawned process writing a dump, a killed process group going
# away. How long that takes is a property of the HOST, not of the behaviour
# under test: the same stub that exits in 0.1s on an idle laptop can take
# tens of seconds on a gate host simultaneously building Rust.
#
# `deadline = time.time() + 10` encoded a machine speed into a behavioural
# assertion, and -- worse -- missing the window fell through SILENTLY into
# the next assertion. Observed on the i7 under gate keel3 (gate_version 8,
# duration 2013s), recorded as `fleet_tests_flakes[0]`:
#
#     FAIL: test_the_reaped_gate_refusal_is_still_one_shot_and_the_warning_is_not
#     AssertionError: 'testhost-one-73575' not found in []
#
# which reads as "fleetd stopped reaping gates" and was in fact "the gate
# had not exited yet". A test that is green alone and red under load is a
# defect in the test (AGENTS.md, "Name the instrument"): it corrupts the
# gate, which is the instrument every other measurement here is graded by.
#
# So: a budget generous enough for a loaded host, and a timeout that is an
# explicit self-describing failure instead of a fall-through. The budget
# bounds nothing any test asserts -- each wait returns the instant its
# condition holds, so on a healthy host raising it costs zero wall time and
# buys a red that says which of "still running", "killed instead of reaped"
# and "reaped but not reported" actually happened. It is NOT a retry: the
# assertions after it are unchanged and still run exactly once.
WAIT_BUDGET_S = float(os.environ.get("FLEET_TESTS_WAIT_BUDGET_S", "180"))


def poll_until(predicate, budget: float = None, interval: float = 0.05) -> tuple:
    """Poll `predicate` until true or `budget` expires.

    Returns `(ok, elapsed)`. The caller says what a timeout means -- this
    never decides on its own that a timeout is harmless.
    """
    budget = WAIT_BUDGET_S if budget is None else budget
    t0 = time.time()
    while True:
        if predicate():
            return True, time.time() - t0
        elapsed = time.time() - t0
        if elapsed >= budget:
            return False, elapsed
        time.sleep(interval)


class WaitsForProcesses:
    """`self.await_true(...)`: poll for a real-process condition, and fail
    LOUDLY and specifically if the budget runs out."""

    def await_true(self, predicate, describe, budget: float = None,
                   interval: float = 0.05) -> float:
        """`describe` is the thing being waited FOR. Pass a callable to have
        the current state rendered at failure time rather than at call
        time -- the whole value of the message is that it reports what was
        true when the wait gave up."""
        budget = WAIT_BUDGET_S if budget is None else budget
        ok, elapsed = poll_until(predicate, budget=budget, interval=interval)
        if not ok:
            what = describe() if callable(describe) else describe
            self.fail(f"timed out after {elapsed:.1f}s (budget {budget:.0f}s, raise "
                      f"FLEET_TESTS_WAIT_BUDGET_S if this host is genuinely that "
                      f"slow) waiting for {what}")
        return elapsed


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


class FleetdBase(WaitsForProcesses, HermeticCase):
    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.bare, self.seed = make_fixture_hub(self.tmp)
        self.hub = make_hub(self, str(self.bare), workdir=self.tmp / "hubcache")
        self.stub = make_stub_gate(self.tmp)
        self.workers = []
        self.host = "testhost"
        # T3: the durable warning store `main` owns for the daemon's whole
        # lifetime. Held on the fixture (rather than created per call
        # inside `reconcile`) so that a test making several reconciles sees
        # the same persistence a running fleetd does -- which is the entire
        # property `warnings` exists for.
        self.host_warnings = fleetd.HostWarnings()
        os.environ["FLEET_HOST"] = self.host

    def tearDown(self):
        # finish any parked stub gates so nothing outlives the test
        for w in self.workers:
            (self.tmp / f"stop-{w.tag}").write_text("")
        poll_until(lambda: not any(w.alive() for w in self.workers))
        for w in self.workers:
            if w.popen is not None:
                w.popen.wait(timeout=WAIT_BUDGET_S)
        self.tmpdir.cleanup()
        os.environ.pop("FLEET_HOST", None)

    # -- process-state reporting, for failure messages ------------------ #

    def worker_states(self) -> list:
        """What `self.workers` actually is right now. `alive()` is polled
        here (not cached) because "was the process still running?" is the
        first question any reap failure raises and the answer is worthless
        if it is stale."""
        out = []
        for w in self.workers:
            rc = w.popen.returncode if w.popen is not None else "no-popen"
            out.append(f"{w.tag}(kind={w.kind} alive={w.alive()} rc={rc})")
        return out

    def reap_report(self, tag: str, res) -> str:
        """Everything a reap assertion needs to be diagnosable: whether the
        worker is still alive, and all four channels a reaped worker can
        land in -- `finished` (the reap), `killed` (a lost lease took it
        instead), `refused` (this loop's one-shot reasons) and `warnings`
        (the durable ones). Without `killed` here, a lease-loss and a slow
        exit are indistinguishable in the failure text, and they need
        opposite fixes."""
        return (f"tag={tag} workers={self.worker_states()} "
                f"finished={res.finished} killed={res.killed} "
                f"refused={res.refused} warnings={res.warnings}")

    def finish_worker(self, tag: str) -> float:
        """Tell the stub with this tag to stop, then block until it is
        GENUINELY gone -- `Worker.alive()` false, which for a worker we
        spawned means `Popen.poll()` has returned and reaped the child.

        `reconcile_once` moves a worker to `finished` on the first pass
        that sees `alive()` false, so asserting on `finished` before this
        returns is asserting on how fast this host schedules a parked bash
        loop. Returns the elapsed seconds so a caller can report them."""
        (self.tmp / f"stop-{tag}").write_text("")
        return self.await_true(
            lambda: not any(w.tag == tag and w.alive() for w in self.workers),
            lambda: (f"stub worker {tag} to exit after its stop-file was written "
                     f"(it never did, so nothing below is evidence about fleetd's "
                     f"reaping); workers={self.worker_states()}"),
        )

    def reconcile(self, disk=100.0, mem=32.0, pgid_probe=None):
        """`pgid_probe` is forwarded ONLY when given, so every existing
        caller still exercises the production `live_pgids`. The tests that
        pass one are about what this step does when the `ps` listing is
        unavailable or lying -- see `TestUnavailableProcessListing`."""
        return fleetd.reconcile_once(
            self.hub, self.host, self.workers,
            gate_command=[str(self.stub)],
            log_dir=self.tmp / "logs",
            repo_root=Path(__file__).resolve().parents[3],
            disk_probe=lambda: disk,
            mem_probe=lambda: mem,
            warnings=self.host_warnings,
            **({"pgid_probe": pgid_probe} if pgid_probe is not None else {}),
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
        self.workers[0].popen.wait(timeout=WAIT_BUDGET_S)
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

    def test_heartbeat_carries_refused_from_the_reconcile_result(self):
        """PLAN Stage 1 task 5 / SPEC L121: `write_heartbeat`'s payload
        must carry this loop's `ReconcileResult.refused` verbatim -- a
        (reason, detail) pair per refusal, JSON round-tripped as a
        2-element array -- so `fleet status --why` (and later `/v1/why`)
        can answer from the ref alone."""
        self.set_desired(gates=3, enabled=False, reason="quarantine test")
        res = self.reconcile()
        self.assertIn("disabled", [r[0] for r in res.refused])
        self.assertTrue(res.heartbeat_written)

        hb = self.hub.read(fleetd.HOSTS_PREFIX + self.host)
        self.assertIn("refused", hb)
        # JSON has no tuples -- each pair round-trips as a 2-element list.
        self.assertEqual(hb["refused"], [list(r) for r in res.refused])
        reasons = [r[0] for r in hb["refused"]]
        self.assertIn("disabled", reasons)
        detail = dict(hb["refused"])["disabled"]
        self.assertEqual(detail, "quarantine test")

    def test_heartbeat_refused_is_empty_when_a_gate_starts_cleanly(self):
        """The genuine "nothing to report" case: a target the host can
        actually meet, and it meets it. `set_desired(gates=0)` is NOT this
        case any more -- see `TestRefusedReasons.test_target_zero_...`
        below, which is the S1 fix for exactly that (previously silent)
        state."""
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")
        self.assertEqual(res.refused, [], f"a clean start must refuse nothing: {res.refused}")
        hb = self.hub.read(fleetd.HOSTS_PREFIX + self.host)
        self.assertEqual(hb.get("refused"), [])


class TestRefusedReasons(FleetdBase):
    """S1 (Stage 1 integration review): `reconcile_once` must record a
    `refused` reason for every idle condition, not just `disabled` and
    `limits`. Before this, an enabled host with both targets at zero, or
    with targets it could not currently fill, recorded nothing -- `fleet
    status --why` printed '(no refused reasons on file)' for a host idling
    entirely by design, indistinguishable from one that was silently
    broken."""

    def test_target_zero_when_enabled_with_both_targets_at_zero(self):
        """The exact silent case the review named: enabled, gates=0,
        agents=0. Previously `res.refused == []` here -- see the now-
        renamed `test_heartbeat_refused_is_empty_when_a_gate_starts_cleanly`
        above, which used to assert this very state was silent."""
        self.set_desired(gates=0)
        res = self.reconcile()
        self.assertEqual(res.started, [])
        self.assertIn("target-zero", [r[0] for r in res.refused], f"refused={res.refused}")
        detail = dict(res.refused)["target-zero"]
        self.assertIn("gates 0", detail)
        self.assertIn("agents 0", detail)

    def test_no_target_zero_when_gates_alone_is_positive(self):
        """Only-BOTH-zero is the trigger; a host with a live gate target
        (even one it cannot fill this loop) is not idle by design and must
        not be mislabeled `target-zero`."""
        self.set_desired(gates=1, limits={"min_free_gb": 999, "min_free_mem_gb": 8})
        res = self.reconcile()
        self.assertNotIn("target-zero", [r[0] for r in res.refused], f"refused={res.refused}")
        self.assertIn("limits", [r[0] for r in res.refused])

    def test_queue_empty_when_targets_exceed_what_is_currently_claimable(self):
        """`staging/one` is the fixture's only branch. Once it is already
        running here, a second gate slot has nothing left to claim -- the
        queue itself is not empty (the branch is still IN it), but nothing
        in it is claimable, which is the distinction `queue-empty` names."""
        self.set_desired(gates=1)
        res1 = self.reconcile()
        self.assertEqual(len(res1.started), 1, f"refused={res1.refused}")

        self.set_desired(gates=2)
        res2 = self.reconcile()
        self.assertEqual(res2.started, [])
        self.assertIn("queue-empty", [r[0] for r in res2.refused], f"refused={res2.refused}")

    def test_queue_error_is_recorded_not_raised(self):
        """B1's twin on the `reconcile_once` side: `workqueue.Queue.compute()`
        raises `QueueError` (not `HubError`) when the tip ref itself is
        unreadable on the hub -- unguarded, this propagated straight out of
        `reconcile_once` and crashed the daemon before its first heartbeat.
        The reason name is `workqueue.compute_or_refusal`'s own
        (`queue-unavailable`): fleetd records what the queue says, it does
        not invent a second vocabulary for the same fact.
        Deleting the tip ref directly from the bare hub reproduces exactly
        that precondition without needing a second, differently-broken
        fixture repo."""
        self.set_desired(gates=1)
        subprocess.run(
            ["git", "-C", str(self.bare), "update-ref", "-d", HUB_TIP_REF],
            check=True,
        )
        res = self.reconcile()
        self.assertEqual(res.started, [])
        self.assertIn("queue-unavailable", [r[0] for r in res.refused], f"refused={res.refused}")
        self.assertTrue(dict(res.refused)["queue-unavailable"], "must carry the underlying detail")
        # And the daemon really did keep going: a second reconcile against
        # the still-broken hub must fail the SAME way, not raise.
        res2 = self.reconcile()
        self.assertIn("queue-unavailable", [r[0] for r in res2.refused])

    def test_why_flag_renders_target_zero_end_to_end(self):
        """`fleet status --why` (cli.py) reads `refused` straight off the
        durable heartbeat -- this is the human-facing end of S1, not just
        `reconcile_once`'s return value. `_refused_list`/the `--why`
        renderer are already reason-agnostic (PLAN Stage 1 task 5), so one
        end-to-end reason is enough to prove the wiring; the reason values
        themselves are pinned above."""
        self.set_desired(gates=0)
        res = self.reconcile()
        self.assertTrue(res.heartbeat_written)

        import contextlib
        import io
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status", "--why"])
        out = buf.getvalue()
        self.assertIn("refused: target-zero (gates 0 / agents 0)", out, out)


class TestGateVerdictStoreFailureSurfaced(FleetdBase):
    """R4 (review of staging/agent-server @ 99f06cb3): gate.sh's own
    `store_verdict()` (see test_gate_script.py's
    `TestStoreVerdictLoudFailure` for that half) leaves a sibling
    `gate-<tag>.verdict-store-failed` marker when it could not push this
    gate's verdict to the hub cache. This is the fleetd half: the SAME
    reap step that already turns a lost lease into a kill (`res.killed`)
    and an exited worker into `res.finished` is where that marker gets
    read, so a finished gate that left one behind becomes a `refused`
    reason on that exact loop -- reusing `ReconcileResult.refused` and
    `write_heartbeat`'s existing carry-through (PLAN Stage 1 task 5)
    rather than inventing a second, parallel channel for the same fact.
    """

    def test_marker_left_by_gate_sh_becomes_a_refused_reason_on_reap(self):
        self.set_desired(gates=1)
        res1 = self.reconcile()
        self.assertEqual(len(res1.started), 1, f"refused={res1.refused}")
        tag = res1.started[0]

        # What gate.sh's store_verdict() leaves behind on a hub-push
        # failure -- written directly here since this test is about
        # fleetd's reap-time reading of it, not gate.sh's own writing of
        # it (that half is test_gate_script.py's job).
        marker = self.tmp / "logs" / f"gate-{tag}.verdict-store-failed"
        self.assertTrue(marker.parent.is_dir(), "start_gate must have created the log dir")
        marker.write_text("")

        self.finish_worker(tag)
        res2 = self.reconcile()
        self.assertIn(tag, res2.finished,
                      f"gate exited but was not reaped: {self.reap_report(tag, res2)}")
        self.assertIn("verdict-store-failed", [r[0] for r in res2.refused],
                      f"refused={res2.refused}")
        detail = dict(res2.refused)["verdict-store-failed"]
        self.assertIn(tag, detail, detail)

        # And it survives into the durable heartbeat `fleet status --why`
        # reads -- not just this call's return value.
        self.assertTrue(res2.heartbeat_written)
        hb = self.hub.read(fleetd.HOSTS_PREFIX + self.host)
        self.assertIn("verdict-store-failed", [r[0] for r in hb["refused"]], hb)

    def test_no_marker_means_no_false_refusal_on_reap(self):
        """The negative case: a gate that finishes cleanly, with no marker,
        must not manufacture a `verdict-store-failed` reason -- proving
        the check above is real (it truly reads the marker) rather than
        firing on every reap regardless."""
        self.set_desired(gates=1)
        res1 = self.reconcile()
        self.assertEqual(len(res1.started), 1, f"refused={res1.refused}")
        tag = res1.started[0]

        self.finish_worker(tag)
        res2 = self.reconcile()
        self.assertIn(tag, res2.finished,
                      f"gate exited but was not reaped: {self.reap_report(tag, res2)}")
        self.assertNotIn("verdict-store-failed", [r[0] for r in res2.refused],
                         f"refused={res2.refused}")


class TestVerdictStoreFailureIsDurableAndOwnerless(FleetdBase):
    """T3 (review of `staging/agent-server` @ 6bf59f2b). Two gaps in the R4
    plumbing that `TestGateVerdictStoreFailureSurfaced` above pins the
    working half of.

    1. IT LASTED ONE LOOP. `ReconcileResult.refused` is this loop's
       scheduling answer; the reap step appends `verdict-store-failed`
       exactly once, on the pass that reaps the gate, and the very next
       reconcile -- 15 seconds later -- overwrites the heartbeat with a
       `refused[]` that no longer mentions it. An operator who ran
       `fleet status --why` sixteen seconds after the failure saw a healthy
       host. The condition, meanwhile, had not changed at all: the marker
       file was still sitting there.

    2. IT ONLY SAW ITS OWN GATES. The reap-time check runs inside the loop
       over `workers`, so it can only fire for a gate `fleetd` spawned and
       is holding. A marker written by `train.real_gate` (a subprocess of
       the train's own process, no worker, no claim) or by a human running
       `gate.sh` by hand was read by nothing, on any host, ever -- and the
       hosts that run trains are exactly the hosts whose verdict pushes
       matter most.

    `HostWarnings` fixes both by sweeping the LOG DIRECTORY rather than the
    worker list: provenance stops mattering, and an entry lives exactly as
    long as its file does.
    """

    def _marker(self, tag: str):
        logs = self.tmp / "logs"
        logs.mkdir(parents=True, exist_ok=True)
        m = fleetd._verdict_store_failed_marker(logs, tag)
        m.write_text("")
        return m

    def _reasons(self, entries):
        return [r[0] for r in entries]

    def test_a_marker_from_a_gate_fleetd_never_spawned_becomes_a_warning(self):
        """No worker, no claim, no reap -- the shape `train.real_gate` and
        a hand-run `gate.sh` leave behind, and the shape the reap-time
        check is structurally unable to see."""
        self.set_desired(gates=0)
        self._marker("train-staging-alpha-991")

        res = self.reconcile()
        self.assertEqual(res.started, [])
        self.assertEqual(res.finished, [], "there was no worker to reap")
        self.assertIn("verdict-store-failed", self._reasons(res.warnings),
                      f"warnings={res.warnings}")
        detail = dict(res.warnings)["verdict-store-failed"]
        self.assertIn("train-staging-alpha-991", detail)
        # ...and it is NOT a refusal: nothing was refused, the host is
        # simply at its desired target of zero.
        self.assertNotIn("verdict-store-failed", self._reasons(res.refused))

    def test_the_warning_is_in_every_heartbeat_not_just_the_first(self):
        """The durability property, measured the way an operator would: by
        re-reading the durable heartbeat ref after each loop."""
        self.set_desired(gates=0)
        self._marker("m5-hand-run-1")

        for loop in range(3):
            res = self.reconcile()
            self.assertIn("verdict-store-failed", self._reasons(res.warnings),
                          f"loop {loop}: warnings={res.warnings}")
            self.assertTrue(res.heartbeat_written)
            hb = self.hub.read(fleetd.HOSTS_PREFIX + self.host)
            self.assertIn("verdict-store-failed",
                          [w[0] for w in (hb.get("warnings") or [])], hb)

    def test_removing_the_marker_clears_the_warning(self):
        """The other half of durability, and the reason nothing here
        expires on a timer: the entry tracks the FILE, and
        `store_verdict()`'s own `rm -f "$SV"` on a later successful store
        is what removes it. A warning that outlived its cause would train
        operators to ignore the field."""
        self.set_desired(gates=0)
        marker = self._marker("m5-clears")
        self.assertIn("verdict-store-failed", self._reasons(self.reconcile().warnings))

        marker.unlink()
        res = self.reconcile()
        self.assertEqual(res.warnings, [], f"warnings={res.warnings}")
        hb = self.hub.read(fleetd.HOSTS_PREFIX + self.host)
        self.assertEqual(hb.get("warnings"), [])

    def test_a_clean_host_reports_no_warnings_at_all(self):
        """The negative control. Without it, a `scan` that appended
        unconditionally would satisfy every test above."""
        self.set_desired(gates=0)
        res = self.reconcile()
        self.assertEqual(res.warnings, [])

    def test_two_markers_are_two_warnings_and_neither_is_duplicated(self):
        """Keyed by PATH, so re-sweeping the same directory on every loop
        cannot accumulate copies of the same fact -- which is what an
        append-only `refused`-style list would have done here, given that
        this sweep runs every 15 seconds forever."""
        self.set_desired(gates=0)
        self._marker("tag-a")
        self._marker("tag-b")
        self.reconcile()
        res = self.reconcile()
        self.assertEqual(len(res.warnings), 2, res.warnings)
        details = " ".join(d for _, d in res.warnings)
        self.assertIn("tag-a", details)
        self.assertIn("tag-b", details)

    def test_fleet_status_why_renders_the_warning(self):
        """`fleet status --why` is the command the human runs when nothing
        is happening; a durable field it does not print is a durable field
        nobody reads."""
        import contextlib
        import io

        self.set_desired(gates=0)
        self._marker("m5-why-render")
        self.assertTrue(self.reconcile().heartbeat_written)

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status", "--why"])
        out = buf.getvalue()
        self.assertIn("warning: verdict-store-failed", out, out)
        self.assertIn("m5-why-render", out, out)

    def test_the_reaped_gate_refusal_is_still_one_shot_and_the_warning_is_not(self):
        """The two channels side by side, which is the finding in one test.

        A gate fleetd DID spawn produces both: a `refused` entry on the
        single loop that reaps it (unchanged -- `fleet status --why`'s
        existing rendering and `TestGateVerdictStoreFailureSurfaced` both
        depend on it) and a warning that is still there on the next loop,
        and the one after that.
        """
        self.set_desired(gates=1)
        res1 = self.reconcile()
        self.assertEqual(len(res1.started), 1, f"refused={res1.refused}")
        tag = res1.started[0]
        self._marker(tag)

        # Wait for the gate to be GENUINELY gone before reconciling. The
        # reap is what puts `tag` in `finished`, and the reap only happens
        # once `alive()` is false -- so a fixed sleep here would be
        # measuring this host's scheduler, not fleetd (see WAIT_BUDGET_S:
        # that is the flake this line replaces).
        self.finish_worker(tag)

        reaped = self.reconcile()
        self.assertIn(tag, reaped.finished,
                      f"the gate exited but the reap did not report it: "
                      f"{self.reap_report(tag, reaped)}")
        self.assertIn("verdict-store-failed", self._reasons(reaped.refused),
                      f"the reap loop did not raise the one-shot refusal: "
                      f"{self.reap_report(tag, reaped)}")
        self.assertIn("verdict-store-failed", self._reasons(reaped.warnings),
                      f"the marker sweep did not raise the durable warning: "
                      f"{self.reap_report(tag, reaped)}")

        after = self.reconcile()
        self.assertNotIn("verdict-store-failed", self._reasons(after.refused),
                         f"the per-loop refusal is (still) one-shot by design: "
                         f"{self.reap_report(tag, after)}")
        self.assertIn("verdict-store-failed", self._reasons(after.warnings),
                      f"the durable warning vanished with the refusal -- this is "
                      f"exactly the 15-second visibility window T3 exists to close: "
                      f"{self.reap_report(tag, after)}")


class TestHostWarningsScanOnAMissingLogDir(HermeticCase):
    """`HostWarnings.scan` promises that "we could not look" never reads
    as "the condition cleared". `Path.glob` on a directory that does not
    exist yields nothing and raises nothing, so the OSError branch alone
    did not keep that promise: an absent `~/gatelogs` (a fresh host, a
    moved `--log-dir`, a purge) silently emptied every warning on the next
    loop. The guard is `is_dir()`; this pins it without a reconcile."""

    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    @staticmethod
    def _reasons(entries):
        return [r for r, _ in entries]

    def test_an_absent_log_dir_keeps_what_was_seen(self):
        logs = self.tmp / "gatelogs"
        logs.mkdir()
        fleetd._verdict_store_failed_marker(logs, "m5-missing-dir").write_text("")
        hw = fleetd.HostWarnings()
        self.assertEqual(self._reasons(hw.scan(logs)), ["verdict-store-failed"])

        shutil.rmtree(logs)
        self.assertEqual(self._reasons(hw.scan(logs)), ["verdict-store-failed"],
                         "an absent directory cleared the warning -- 'could not "
                         "look' read as 'looked and found nothing'")
        self.assertEqual(self._reasons(hw.scan(self.tmp / "never-existed")),
                         ["verdict-store-failed"])

        # The real all-clear is a directory that IS there and holds no marker.
        logs.mkdir()
        self.assertEqual(hw.scan(logs), [])

    def test_a_file_where_the_directory_should_be_is_not_an_all_clear_either(self):
        logs = self.tmp / "gatelogs"
        logs.mkdir()
        fleetd._verdict_store_failed_marker(logs, "m5-not-a-dir").write_text("")
        hw = fleetd.HostWarnings()
        self.assertEqual(len(hw.scan(logs)), 1)
        shutil.rmtree(logs)
        logs.write_text("")  # a FILE at the log-dir path
        self.assertEqual(len(hw.scan(logs)), 1)


class TestSpawnEnvHelper(HermeticCase):
    """`fleetd._spawn_env` in isolation, no subprocess: it must set
    FLEET_HUB_URL/FLEET_CODE_URL from the `Hub` object (overriding any
    stale value already in os.environ) while leaving everything else --
    including the three vars B4 names explicitly -- untouched."""

    def test_hub_and_code_url_come_from_the_hub_object_not_the_environment(self):
        import unittest.mock as mock
        tmp = tempfile.mkdtemp(prefix="spawn-env-helper-")
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        hub = Hub("https://example.invalid/state.git", workdir=Path(tmp) / "hc",
                  code_url="https://example.invalid/code.git")
        with mock.patch.dict(os.environ, {
            "FLEET_GIT_TOKEN_FILE": "/tmp/tok",
            "EXIFTOOL_CACHE_DIR": "/tmp/oracle-cache",
            "FLEET_TRAIN_DEPLOY_KEY": "/tmp/deploy-key",
            "FLEET_HUB_URL": "stale-value-must-be-overwritten",
            "FLEET_CODE_URL": "stale-value-must-be-overwritten-too",
            "UNRELATED_VAR": "still-here",
        }):
            env = fleetd._spawn_env(hub)
        self.assertEqual(env["FLEET_HUB_URL"], hub.url)
        self.assertEqual(env["FLEET_CODE_URL"], hub.code_url)
        self.assertEqual(env["FLEET_GIT_TOKEN_FILE"], "/tmp/tok")
        self.assertEqual(env["EXIFTOOL_CACHE_DIR"], "/tmp/oracle-cache")
        self.assertEqual(env["FLEET_TRAIN_DEPLOY_KEY"], "/tmp/deploy-key")
        self.assertEqual(env["UNRELATED_VAR"], "still-here")

    def test_hub_and_code_url_are_set_even_when_absent_from_the_environment(self):
        """The argv-only case: neither var was ever in os.environ at all."""
        import unittest.mock as mock
        tmp = tempfile.mkdtemp(prefix="spawn-env-helper-absent-")
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        hub = Hub("https://example.invalid/state2.git", workdir=Path(tmp) / "hc")
        env_without = {k: v for k, v in os.environ.items()
                       if k not in ("FLEET_HUB_URL", "FLEET_CODE_URL")}
        with mock.patch.dict(os.environ, env_without, clear=True):
            env = fleetd._spawn_env(hub)
        self.assertEqual(env["FLEET_HUB_URL"], hub.url)
        self.assertEqual(env["FLEET_CODE_URL"], hub.code_url)  # defaults to hub.url


class TestSpawnEnv(FleetdBase):
    """B4 (Stage 1 integration review): fleetd must build the spawned
    gate/agent's environment EXPLICITLY from the `Hub` it is actually
    using, not rely on `subprocess.Popen`'s default full inheritance of
    fleetd's own `os.environ` -- because a fleetd started `--hub X --code
    Y` on argv alone never writes FLEET_HUB_URL/FLEET_CODE_URL into its
    own environment in the first place, so plain inheritance hands the
    child neither."""

    def make_env_dump_gate(self):
        """A stub 'gate' that records the two vars of interest to a file
        before parking, so the test can read back exactly what the
        subprocess saw -- not what this test process happens to have set."""
        stub = self.tmp / "stub-env-gate.sh"
        stub.write_text(
            "#!/bin/bash\n"
            f'env | grep -E "^FLEET_(HUB|CODE)_URL=" > "{self.tmp}/envdump-$2.txt"\n'
            f'STOP="{self.tmp}/stop-$2"\n'
            'while [ ! -f "$STOP" ]; do sleep 0.2; done\n'
            "exit 0\n"
        )
        stub.chmod(0o755)
        return stub

    def _read_dump(self, tag: str) -> str:
        return self._await_file(self.tmp / f"envdump-{tag}.txt",
                                f"the env dump for {tag}").read_text()

    def _await_file(self, path: Path, what: str) -> Path:
        """The spawned process writes this file as its first act, so its
        absence means the spawn is slow or the spawn failed -- two very
        different findings, and neither is "the env was wrong", which is
        what the assertions below would otherwise report."""
        self.await_true(
            path.exists,
            lambda: (f"the spawned process to write {what} at {path}; "
                     f"log dir contents={sorted(p.name for p in (self.tmp / 'logs').glob('*'))}"),
        )
        return path

    def test_start_gate_env_carries_hub_and_code_url_given_only_argv_config(self):
        """Simulates `fleetd --hub <state> --code <code>`: a `Hub` with a
        `code_url` distinct from `url` (exactly what `main` builds from
        `args.hub`/`args.code`), and neither FLEET_HUB_URL nor
        FLEET_CODE_URL present anywhere in this process's OWN environment
        -- proving the child's config came from the `Hub` object, not from
        ambient inheritance."""
        distinct_code_url = str(self.tmp / "distinct-code.git")
        hub = Hub(str(self.bare), workdir=self.tmp / "hubcache-spawnenv",
                  code_url=distinct_code_url)
        saved = {k: os.environ.pop(k, None) for k in ("FLEET_HUB_URL", "FLEET_CODE_URL")}
        self.addCleanup(lambda: [os.environ.__setitem__(k, v) for k, v in saved.items()
                                  if v is not None])
        self.assertNotIn("FLEET_HUB_URL", os.environ)
        self.assertNotIn("FLEET_CODE_URL", os.environ)

        gate = self.make_env_dump_gate()
        w = fleetd.start_gate(hub, "staging/one", "envtag1", [str(gate)],
                               self.host, self.tmp / "logs")
        self.assertIsNotNone(w, "claim must succeed against the fixture hub")
        self.workers.append(w)

        content = self._read_dump("envtag1")
        self.assertIn(f"FLEET_HUB_URL={hub.url}\n", content)
        self.assertIn(f"FLEET_CODE_URL={distinct_code_url}\n", content)

    def test_start_agent_env_carries_hub_and_code_url_given_only_argv_config(self):
        """`start_agent` builds its own argv (`sys.executable`,
        `agentworker.py`, ...) internally, so this proves the Popen call's
        environment the same way `test_start_gate_...` does -- by making
        the subprocess itself dump the two vars -- rather than by actually
        running agentworker.py (covered elsewhere, e.g. `TestAgentSlots`):
        `sys.executable` is swapped for a wrapper that dumps env and exits,
        never reaching the real interpreter or agentworker.py's own logic.
        """
        distinct_code_url = str(self.tmp / "distinct-code-agent.git")
        hub = Hub(str(self.bare), workdir=self.tmp / "hubcache-spawnenv-agent",
                  code_url=distinct_code_url)
        saved = {k: os.environ.pop(k, None) for k in ("FLEET_HUB_URL", "FLEET_CODE_URL")}
        self.addCleanup(lambda: [os.environ.__setitem__(k, v) for k, v in saved.items()
                                  if v is not None])

        wrapper = self.tmp / "sys-executable-env-dump.sh"
        wrapper.write_text(
            "#!/bin/bash\n"
            f'env | grep -E "^FLEET_(HUB|CODE)_URL=" > "{self.tmp}/envdump-agent1.txt"\n'
            f'printf "%s\\n" "$@" > "{self.tmp}/argvdump-agent1.txt"\n'
            "exit 0\n"
        )
        wrapper.chmod(0o755)

        import unittest.mock as mock
        with mock.patch.object(fleetd.sys, "executable", str(wrapper)):
            w = fleetd.start_agent(hub, "staging/one", "agent1", self.host,
                                    self.tmp / "logs", Path(__file__).resolve().parents[3])
        self.assertIsNotNone(w, "claim must succeed against the fixture hub")
        self.workers.append(w)

        content = self._read_dump("agent1")
        self.assertIn(f"FLEET_HUB_URL={hub.url}\n", content)
        self.assertIn(f"FLEET_CODE_URL={distinct_code_url}\n", content)

        # S2 (Stage 1 integration review): the worker is told the CODE
        # repo on argv too, not left to infer it. `--hub` stays the STATE
        # repo; `--code` is what it clones and probes `refs/heads/*` on.
        argv_dump = self._await_file(self.tmp / "argvdump-agent1.txt",
                                     "the agent's argv dump")
        argv = argv_dump.read_text().splitlines()
        self.assertIn("--code", argv, argv)
        self.assertEqual(argv[argv.index("--code") + 1], distinct_code_url, argv)
        self.assertEqual(argv[argv.index("--hub") + 1], hub.url, argv)


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
            # once, so A's first CAS write hits a stale expect_sha. The
            # sneak goes through `self.hub` by BOUND calls (not the
            # unbound `orig_read`): under FLEET_TEST_HUB=server the
            # fixture hub is a FallbackHub, not a plain Hub, and the
            # `sneak["done"]` guard set above already prevents recursion
            # into this patch either way.
            if ref == cli.DESIRED_REF and not sneak["done"]:
                sneak["done"] = True
                cur = self.hub.sha(cli.DESIRED_REF)
                d2 = self.hub.read(cli.DESIRED_REF)
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
        # `cli.main` writes through its OWN plain Hub, out-of-band to the
        # fixture server; the fixture hub's view converges at the next
        # sweep, so poll for the composed doc rather than asserting the
        # cache was never behind.
        final = within_sweep(
            lambda: self.hub.read(cli.DESIRED_REF),
            lambda d: bool(d)
            and d.get("hosts", {}).get(self.host, {}).get("gates") == 4
            and "otherhost" in d.get("hosts", {}),
        )
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
        env = scrub_env()
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
            self.workers[0].popen.wait(timeout=WAIT_BUDGET_S)
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


class TestMainLoopSurvivesHubErrors(FleetdBase):
    """`fleetd.main`'s loop guard: a hub failure costs an ITERATION, not
    the daemon -- and only up to a point.

    WHAT WAS WRONG. `reconcile_once` reads five or six hub refs and
    `workqueue.Queue.compute()` reads every claim payload, and not one of
    those calls had a `try` around it in `reconcile_once` OR in `main`. A
    single `HubError` -- `fleetlib.Hub.read`'s ls-remote/fetch race, which
    renewing leases made reachable on every loop -- did not degrade a
    queue, it EXITED THE DAEMON, on a host with live gates running. That
    race is fixed in `fleetlib` (see `test_fleetlib.py`), but the class of
    failure it belonged to is not: a dropped ssh signature, a hub mid-`gc`,
    a full disk under the object cache all raise the same exception.

    WHAT MUST NOT BE TRUE INSTEAD. A daemon that swallows hub errors
    forever looks alive, logs cheerfully, starts nothing and reports a
    heartbeat only because that write lives inside the step that is
    failing. Both directions are tested here; neither alone is the fix.

    NAME THE INSTRUMENT. `fleetd.main` is driven IN-PROCESS against the
    fixture bare hub, with only `reconcile_once` replaced -- the singleton
    claim, `adopt_workers`, the signal handlers, the exit codes and the
    `finally: singleton.release()` are all the real ones. `HOME` is
    redirected into the test tempdir because `main` puts its object cache
    under `Path.home()`. Against the unguarded `main`, the first two of
    these tests do not fail, they ERROR with the raised `HubError` -- which
    is precisely the production symptom.
    """

    def setUp(self):
        super().setUp()
        import signal as _signal
        self._old_term = _signal.getsignal(_signal.SIGTERM)
        self._old_int = _signal.getsignal(_signal.SIGINT)
        self.addCleanup(_signal.signal, _signal.SIGTERM, self._old_term)
        self.addCleanup(_signal.signal, _signal.SIGINT, self._old_int)

    def run_main(self, fake_reconcile, extra_argv=()):
        import unittest.mock as mock
        argv = [
            "--hub", str(self.bare),
            "--repo-root", str(Path(__file__).resolve().parents[3]),
            "--log-dir", str(self.tmp / "logs"),
            "--interval", "0",
            *extra_argv,
        ]
        with mock.patch.dict(os.environ, {"HOME": str(self.tmp)}), \
                mock.patch.object(fleetd, "reconcile_once", fake_reconcile):
            return fleetd.main(argv)

    def test_a_transient_hub_error_costs_one_iteration_not_the_daemon(self):
        import signal as _signal
        from fleetlib import HubError

        calls = []

        def flaky(*_a, **_kw):
            calls.append(len(calls) + 1)
            if len(calls) <= 2:
                raise HubError(
                    "refs/fleet/claims/gate/x@abc has no readable payload.json: simulated"
                )
            # Third step succeeds. Ask the daemon to stop the way a
            # supervisor would, so the loop exits through its normal path
            # rather than through an exception.
            os.kill(os.getpid(), _signal.SIGTERM)
            return fleetd.ReconcileResult()

        rc = self.run_main(flaky)

        self.assertEqual(len(calls), 3, "the daemon did not survive two failed reconcile steps")
        self.assertEqual(rc, 0, "a recovered daemon must exit cleanly")

    def test_a_persistently_unreachable_hub_still_surfaces(self):
        from fleetlib import HubError

        calls = []

        def always_dead(*_a, **_kw):
            calls.append(len(calls) + 1)
            raise HubError("ls-remote refs/fleet/desired failed: exit 128: could not read from remote")

        rc = self.run_main(always_dead)

        self.assertEqual(
            len(calls), fleetd.RECONCILE_HUB_FAILURE_LIMIT,
            "the daemon must stop retrying after RECONCILE_HUB_FAILURE_LIMIT consecutive failures",
        )
        self.assertEqual(rc, 6, "a wedged daemon must exit NONZERO so its supervisor reacts")

    def test_the_failure_counter_resets_on_a_good_step(self):
        """Bounded means bounded CONSECUTIVELY. A hub that blips once an
        hour must never accumulate its way to an exit over a day of
        otherwise healthy reconciles.
        """
        import signal as _signal
        from fleetlib import HubError

        limit = fleetd.RECONCILE_HUB_FAILURE_LIMIT
        calls = []
        # fail (limit-1), succeed, fail (limit-1), succeed, then stop.
        script = ([False] * (limit - 1)) + [True] + ([False] * (limit - 1)) + [True]

        def scripted(*_a, **_kw):
            idx = len(calls)
            calls.append(idx)
            if idx >= len(script):
                os.kill(os.getpid(), _signal.SIGTERM)
                return fleetd.ReconcileResult()
            if not script[idx]:
                raise HubError("simulated blip")
            return fleetd.ReconcileResult()

        rc = self.run_main(scripted)

        self.assertEqual(
            len(calls), len(script) + 1,
            f"the daemon exited early: {2 * (limit - 1)} failures spread across two runs of "
            f"{limit - 1} is never {limit} CONSECUTIVE failures",
        )
        self.assertEqual(rc, 0)

    def test_once_mode_reports_a_failed_step_instead_of_exiting_zero(self):
        """`--once` is the cron backstop. A single step that never ran must
        not report success: exit 0 there tells the supervisor the host
        reconciled when nothing happened at all.
        """
        from fleetlib import HubError

        calls = []

        def dead(*_a, **_kw):
            calls.append(1)
            raise HubError("simulated blip")

        rc = self.run_main(dead, extra_argv=("--once",))

        self.assertEqual(len(calls), 1, "--once must still be exactly one step")
        self.assertEqual(rc, 6)

    def test_a_non_hub_exception_is_never_swallowed(self):
        """The guard catches `HubError` and nothing else. A bug in this
        file, an OOM or a KeyboardInterrupt must take the process down
        loudly rather than be retried every fifteen seconds forever.
        """
        def buggy(*_a, **_kw):
            raise ValueError("a bug in reconcile_once, not a hub problem")

        with self.assertRaises(ValueError):
            self.run_main(buggy)


class TestLostLeaseIsKilledWhileTheHubIsUnreachable(FleetdBase):
    """R2's "a lost lease is stop-work", enforced during the ONE condition
    that used to suspend it: the hub being unreadable.

    WHAT WAS WRONG. `reconcile_once` opened with two unguarded reads --

        desired_doc = hub.read(DESIRED_REF) or {}
        tip_sig     = hub.read(TIP_SIGNAL_REF) or {}

    -- and the reap/lost-lease-kill loop came AFTER them. So a hub that
    could not be read raised out of the step before the kill loop ran.

    That is not an unlucky ordering, it is the inverse of the one that is
    safe. A lease goes LOST because its renewal push failed, and renewals
    fail for the same reason the reads do: the hub. The single condition
    under which stop-work matters most was the single condition under
    which stop-work did not happen.

    THE COST WAS BOUNDED AND STILL TOO LONG. `main` tolerates
    `RECONCILE_HUB_FAILURE_LIMIT` (5) consecutive failed steps before
    exiting nonzero -- ~75s at the 15s interval -- and for all of it an
    unleased gate kept running while another host, seeing the claim
    expire, was free to reap it and start the same branch. Two gates on
    one branch is the duplicate-merge hazard the KILL comment in
    `reconcile_once` argues about, and it is not detectable afterwards.

    THE FIX IS AN ORDERING, not a new mechanism: the reap/kill loop is
    purely local (an in-memory `lost` flag the renewer thread set, plus a
    `ps` listing), so it runs FIRST, and the hub reads follow it
    individually guarded.

    NAME THE INSTRUMENT. The gate is a real parked stub process in its own
    process group and the kill is a real SIGTERM to that group -- `killed`
    here is a `ps`-verified fact, not a recorded call on a stub. The hub
    is made unreachable the way production does it, by pointing the Hub at
    a path that is not a repository, so the failure is raised by real
    `git` through real `fleetlib` classification (see
    `test_fleetlib.py`'s `TestFetchFailureClassification` -- that
    classification is what makes this arrive as a RAISE rather than as a
    None that would read as "no desired doc").

    Against the pre-fix ordering the first test does not fail, it ERRORs
    with the raised `HubUnreachableError` and a still-running gate --
    which is exactly the production symptom.
    """

    def break_the_hub(self):
        """Make the hub unreachable in whichever shape `make_hub` built.
        Bare: repoint the Hub at a path that is not a git repository, so
        every subsequent read fails at the transport, through real git.
        Server: `_fixtures.break_hub` stops the fixture keel-server
        (connection refused on the primary) AND repoints the fallback Hub
        at the dead path -- hub-unreachability THROUGH the production
        FallbackHub shape, which is a real outage path, not a skip."""
        dead = self.tmp / "hub-is-gone.git"
        break_hub(self.hub, str(dead))
        self.assertFalse(dead.exists())

    def start_one_gate(self):
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"setup failed: refused={res.refused}")
        w = self.workers[0]
        self.assertTrue(w.alive(), "stub gate should be parked and alive")
        return w

    def test_a_lost_lease_is_killed_in_one_reconcile_with_the_hub_down(self):
        from fleetlib import HubError

        w = self.start_one_gate()
        pgid = w.pgid
        w.claim._mark_lost("hub no longer records us as the holder")
        self.break_the_hub()

        # ONE step. Not five, not "eventually": the whole point is that
        # stop-work does not wait on the hub coming back.
        with self.assertRaises(HubError):
            self.reconcile()

        self.assertNotIn(w, self.workers, "killed worker must leave the worker list")
        # SIGKILL to a process group is asynchronous: the kernel has to
        # schedule each member to die. Waiting for that is not waiting for
        # fleetd's behaviour, which already happened above.
        self.await_true(
            lambda: not w.alive(),
            lambda: (f"the lost-lease gate {w.tag} (pgid {w.pgid}) to die after "
                     f"kill_worker; rc={w.popen.returncode if w.popen else 'no-popen'} "
                     f"live_pgids_contains_it={w.pgid in fleetd.live_pgids()}"),
        )
        self.assertNotIn(
            pgid, fleetd.live_pgids(),
            "the process GROUP must be gone (M8) -- children go with the leader",
        )

    def test_the_step_still_raises_so_a_wedged_hub_reaches_a_human(self):
        """The reorder must not become a swallow. `main` counts these and
        exits nonzero at RECONCILE_HUB_FAILURE_LIMIT; a step that killed
        what it had to and then reported success would leave a daemon up,
        cheerful and reaching nothing.
        """
        from fleetlib import HubUnreachableError

        self.start_one_gate()
        self.break_the_hub()
        with self.assertRaises(HubUnreachableError) as ctx:
            self.reconcile()
        self.assertIn(fleetd.DESIRED_REF, str(ctx.exception))

    def test_an_unreadable_desired_is_never_recorded_as_disabled(self):
        """"The operator stood this host down" and "we could not ask" are
        different facts. Collapsing the second into the first would have
        `fleet status` report a deliberate quarantine during an outage.
        """
        from fleetlib import HubError

        self.start_one_gate()
        self.break_the_hub()
        try:
            res = self.reconcile()
        except HubError:
            res = None
        # The raise carries the step's result away, so assert on the
        # observable instead: nothing started, and the run did not claim
        # a heartbeat it never wrote.
        self.assertIsNone(res)

    def test_one_failing_read_degrades_only_its_own_concern(self):
        """`TIP_SIGNAL_REF` feeds a heartbeat field. Its failure must not
        cost the gate starts, which is the difference between "guarded"
        and "guarded independently".
        """
        from fleetlib import HubError

        self.set_desired(gates=1)
        real_read = self.hub.read

        def only_tip_signal_fails(ref):
            if ref == fleetd.TIP_SIGNAL_REF:
                raise HubError("simulated: tip signal ref unreadable")
            return real_read(ref)

        self.hub.read = only_tip_signal_fails
        self.addCleanup(lambda: setattr(self.hub, "read", real_read))

        with self.assertRaises(HubError):
            self.reconcile()

        self.assertEqual(len(self.workers), 1,
                         "an unreadable tip signal must not stop a healthy host starting work")
        self.assertTrue(self.workers[0].alive())

    def test_a_finished_workers_release_failure_cannot_block_anothers_kill(self):
        """Two workers, one finished and one with a lost lease, and a hub
        that fails every write. The finished one's claim release cannot
        complete -- and must not take the lost one's kill with it. The
        reorder puts both in the same local loop, so an exception escaping
        the first iteration would strand the second.
        """
        from fleetlib import HubError

        # The base fixture carries one staging branch; this test needs two
        # workers, so give the queue a second candidate.
        env = scrub_env()
        (self.seed / "h.txt").write_text("second branch\n")
        subprocess.run(["git", "-C", str(self.seed), "add", "."], check=True, env=env)
        subprocess.run(["git", "-C", str(self.seed), "commit", "-qm", "more work"],
                       check=True, env=env)
        subprocess.run(["git", "-C", str(self.seed), "push", "-q", str(self.bare),
                        "HEAD:refs/heads/staging/two"], check=True, env=env)

        self.set_desired(gates=2)
        res = self.reconcile()
        self.assertEqual(len(res.started), 2, f"setup failed: refused={res.refused}")
        finished, lost = self.workers[0], self.workers[1]

        # Finish the first for real, so the reap path runs for it.
        (self.tmp / f"stop-{finished.tag}").write_text("")
        finished.popen.wait(timeout=WAIT_BUDGET_S)
        lost.claim._mark_lost("hub no longer records us as the holder")
        self.break_the_hub()

        with self.assertRaises(HubError):
            self.reconcile()

        self.assertEqual(self.workers, [], "both workers must have left the list")
        self.await_true(
            lambda: not lost.alive(),
            lambda: (f"the lost-lease worker {lost.tag} (pgid {lost.pgid}) to die -- it "
                     f"was stranded by the finished worker's failed release; "
                     f"rc={lost.popen.returncode if lost.popen else 'no-popen'} "
                     f"live_pgids_contains_it={lost.pgid in fleetd.live_pgids()}"),
        )


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
        # Patch the sha read on the CLASS OF THE FIXTURE HUB: plain Hub in
        # bare mode (exactly the old `fleetd.Hub` patch), FallbackHub under
        # FLEET_TEST_HUB=server -- where patching `fleetd.Hub` would miss
        # the server route entirely, the reaper would read the RENEWED sha
        # and its CAS would legitimately succeed, testing nothing. This way
        # the reaper is fed the stale witness in both modes and the delete
        # must be refused by the hub's own CAS (through the server, that is
        # the server's CAS).
        with mock.patch.object(type(self.hub), "sha", return_value=stale_sha):
            reaped = fleetd.reap_dead_same_host_singleton(
                self.hub, self.host, ref, own_pid=os.getpid(),
                marker_probe=lambda pgid, exclude_pid: None,
            )
        self.assertFalse(reaped, "a CAS against a stale sha must fail, not clobber the renewal")
        self.assertIsNotNone(self.hub.sha(ref), "the renewed claim must survive")


class TestFleetdMarkerInGroup(WaitsForProcesses, HermeticCase):
    """`fleetd_marker_in_group` against REAL processes -- the whole point of
    this function is that fleetd shares its wrapper's process group and so
    is never that group's LEADER (see the function's docstring), so a fake
    that only exercises the leader case would not test the fix at all."""

    def setUp(self):
        super().setUp()
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
        # Without this wait being an assertion, a slow fork turned into a
        # FileNotFoundError from `read_text()` below -- an error about a
        # missing temp file, in a test about process-group scanning.
        self.await_true(
            self.pidfile.exists,
            lambda: (f"the leader script to record its child's pid at {self.pidfile}; "
                     f"bash alive={p.poll() is None} rc={p.returncode}"),
        )
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


class TestSingletonTTL(HermeticCase):
    """`fleetd.singleton_ttl_s`: the integration reconciliation's one-line
    fix -- the host-singleton lease gets its OWN, shorter TTL
    (`FLEET_SINGLETON_TTL_S`, default 120s) instead of inheriting the
    600s `LEASE_TTL` gate/agent claims use. Combined with FIX 2's
    `reap_dead_same_host_singleton`, this bounds a hard-killed fleetd's
    scheduler handover to seconds in the common case and at most 120s in
    the pathological one, instead of the full 600s either way.
    """

    def setUp(self):
        super().setUp()
        self._saved = {}
        for name in ("FLEET_SINGLETON_TTL_S", claim_mod.TTL_ENV):
            self._saved[name] = os.environ.pop(name, None)

    def tearDown(self):
        for name, value in self._saved.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value

    def test_default_is_120_seconds(self):
        self.assertEqual(fleetd.singleton_ttl_s(), 120.0)

    def test_honors_its_own_env_override(self):
        os.environ["FLEET_SINGLETON_TTL_S"] = "45"
        self.assertEqual(fleetd.singleton_ttl_s(), 45.0)

    def test_a_malformed_override_falls_back_to_120_not_a_crash(self):
        # claim_mod._env_seconds never raises (see its own docstring) --
        # a typo in the env must leave the daemon starting, not dead.
        os.environ["FLEET_SINGLETON_TTL_S"] = "not-a-number"
        self.assertEqual(fleetd.singleton_ttl_s(), 120.0)

    def test_the_hermetic_test_ttl_wins_over_the_singleton_override(self):
        """`FLEET_TEST_TTL_S` compresses EVERY claim's TTL uniformly for
        fixture-hub tests (test_adoption.py, test_seams.py). Passing an
        explicit `ttl=` to `Claim()` bypasses `Claim`'s own env lookup
        (see `Claim.__init__`), so if `singleton_ttl_s` did not check
        `TTL_ENV` itself FIRST, the singleton would silently stop
        compressing in test mode while gate/agent claims kept
        compressing -- reintroducing exactly the kind of
        seam-whose-halves-each-have-green-tests this effort exists to
        close.
        """
        os.environ[claim_mod.TTL_ENV] = "4"
        os.environ["FLEET_SINGLETON_TTL_S"] = "999"
        self.assertEqual(fleetd.singleton_ttl_s(), 4.0)

    def test_no_env_at_all_still_returns_120(self):
        self.assertNotIn("FLEET_SINGLETON_TTL_S", os.environ)
        self.assertNotIn(claim_mod.TTL_ENV, os.environ)
        self.assertEqual(fleetd.singleton_ttl_s(), 120.0)


class TestSingletonTTLWiredIntoMain(FleetdBase):
    """`singleton_ttl_s` computing the right number in isolation (above)
    is not evidence `fleetd.main` actually threads it into the `Claim` it
    constructs -- this drives the real `main()` against the fixture hub
    and reads the live singleton claim's own payload back off the hub."""

    def run_main(self, fake_reconcile):
        import unittest.mock as mock
        argv = [
            "--hub", str(self.bare),
            "--repo-root", str(Path(__file__).resolve().parents[3]),
            "--log-dir", str(self.tmp / "logs"),
            "--interval", "0",
        ]
        with mock.patch.dict(os.environ, {"HOME": str(self.tmp)}), \
                mock.patch.object(fleetd, "reconcile_once", fake_reconcile):
            return fleetd.main(argv)

    def test_the_live_singleton_claim_carries_the_configured_ttl(self):
        import signal as _signal

        os.environ["FLEET_SINGLETON_TTL_S"] = "37"
        # Production path, not test-compressed: TTL_ENV must be absent so
        # `FLEET_SINGLETON_TTL_S` is the value actually exercised here.
        self.assertNotIn(claim_mod.TTL_ENV, os.environ)
        observed = {}

        def peek_then_stop(hub, host, *_rest, **_kw):
            ref = claim_mod.claim_ref("host", host)
            sha = hub.sha(ref)
            self.assertIsNotNone(sha, "main() must have acquired the singleton by now")
            payload = hub.read(ref)
            started = claim_mod._parse_iso(payload["started_at"])
            expires = claim_mod._parse_iso(payload["expires_at"])
            observed["ttl"] = (expires - started).total_seconds()
            os.kill(os.getpid(), _signal.SIGTERM)
            return fleetd.ReconcileResult()

        try:
            rc = self.run_main(peek_then_stop)
        finally:
            os.environ.pop("FLEET_SINGLETON_TTL_S", None)

        self.assertEqual(rc, 0)
        self.assertAlmostEqual(
            observed["ttl"], 37.0, delta=0.5,
            msg="main() did not construct the singleton Claim with singleton_ttl_s()",
        )


# --------------------------------------------------------------------- #
# An UNAVAILABLE process listing is not an EMPTY one
# --------------------------------------------------------------------- #


class TestLivePgidsRefusesToGuess(HermeticCase):
    """`live_pgids` must never hand a caller a listing it cannot vouch for.

    It used to answer a failed `ps` with `set()`:

        except (OSError, subprocess.TimeoutExpired):
            return set()

    and every consumer reads absence from that set as death -- the reap in
    `reconcile_once`, the release in `journal.adopt_from_journal`, the
    orphan verdict in `adopt_workers`. `TestUnavailableProcessListing`
    below pins what that cost; these four pin the probe itself.
    """

    def _ps_raises(self, exc):
        """Make only the `ps` child fail, leaving every other subprocess
        this module runs alone."""
        real = subprocess.run

        def fake(argv, *a, **kw):
            if isinstance(argv, (list, tuple)) and argv and argv[0] == "ps":
                raise exc
            return real(argv, *a, **kw)

        return mock.patch.object(runner_mod.subprocess, "run", fake)

    def test_a_working_ps_lists_this_runners_own_group(self):
        """The positive control for the three refusals below: without it,
        a `live_pgids` that raised unconditionally would pass them all."""
        pgids = runner_mod.live_pgids()
        self.assertIn(os.getpgrp(), pgids,
                      "a truthful listing always contains the caller's own group")

    def test_an_unspawnable_ps_raises_instead_of_reporting_an_empty_host(self):
        with self._ps_raises(OSError(35, "Resource temporarily unavailable")):
            with self.assertRaises(runner_mod.ProcessListingUnavailable):
                runner_mod.live_pgids()

    def test_a_timed_out_ps_raises(self):
        with self._ps_raises(subprocess.TimeoutExpired(cmd="ps", timeout=10)):
            with self.assertRaises(runner_mod.ProcessListingUnavailable):
                runner_mod.live_pgids()

    def test_a_nonzero_ps_raises_rather_than_parsing_its_empty_stdout(self):
        """`subprocess.run` without `check=True` reports a `ps` killed by a
        signal only in `returncode`, so this case used to parse as an
        empty -- i.e. universally fatal -- listing."""
        real = subprocess.run

        def fake(argv, *a, **kw):
            if isinstance(argv, (list, tuple)) and argv and argv[0] == "ps":
                return subprocess.CompletedProcess(argv, 1, "", "ps: fatal")
            return real(argv, *a, **kw)

        with mock.patch.object(runner_mod.subprocess, "run", fake):
            with self.assertRaises(runner_mod.ProcessListingUnavailable):
                runner_mod.live_pgids()


class TestUnavailableProcessListing(FleetdBase):
    """One failed `ps` must not reap a live gate and start a second one.

    THE DEFECT, measured at 2c7716a7 before the fix by injecting a single
    empty listing into the second `reconcile_once` of
    `test_journal.TestOfflineStartThroughRunDaemon`: the adopted worker for
    `staging/one` was reaped (`finished=['journal-gate-gate-staging-one']`)
    while its process group was still alive in a real `ps`, its CAS'd lease
    was released, and -- the branch now unclaimed and the deficit grown by
    exactly the worker just discarded -- the SAME step started
    `journalhost-one-...` beside the gate that was still running. Two gates
    on one branch, reached without a single lease ever going lost.

    `test_the_bug_present_shape_...` is the in-suite control: it restores
    the old `set()` answer and requires the duplicate, so the two tests
    below are evidence that the guard is what prevents it rather than a
    fixture that could never have produced it.
    """

    def unavailable(self):
        def probe():
            raise fleetd.ProcessListingUnavailable("ps could not be run: injected")
        return probe

    def start_one_gate(self):
        """A live gate in the shape ADOPTION produces: real process group,
        real CAS'd claim, and `popen=None`.

        The shape is load-bearing, not incidental. `Worker.alive` answers
        from `popen.poll()` whenever this daemon forked the worker itself
        and only falls through to the pgid listing when it did not -- so
        the listing can only destroy workers that were ADOPTED, which is
        exactly the population a restarted or offline runner has, and
        exactly the one in the reported failure (a journal-adopted gate).
        Asserting any of this against a `popen`-backed worker would pass
        with the guard removed: the listing would never be consulted.
        """
        self.set_desired(gates=2)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"fixture must start a gate: {res.refused}")
        spawned = self.workers[0]
        self.assertTrue(spawned.alive(), "the fixture gate must be parked and alive")
        adopted = fleetd.Worker(
            branch=spawned.branch, tag=spawned.tag, pgid=spawned.pgid,
            claim=spawned.claim, popen=None, kind=spawned.kind,
            job_key=spawned.job_key,
        )
        self.workers[0] = adopted
        # `tearDown` stops the stub by tag and waits on `alive()`, which for
        # the adopted shape is the real `ps`; this only reaps the Popen so
        # the finished child cannot linger as a zombie.
        self.addCleanup(self._wait_quietly, spawned.popen)
        self.assertTrue(adopted.alive(),
                        "the adopted-shape worker must read alive through the ps "
                        "listing, or nothing below is evidence")
        return adopted

    @staticmethod
    def _wait_quietly(popen):
        try:
            popen.wait(timeout=WAIT_BUDGET_S)
        except Exception:  # noqa: BLE001 -- cleanup must never mask a result
            pass

    def test_a_live_worker_is_not_reaped_when_ps_cannot_answer(self):
        w = self.start_one_gate()
        res = self.reconcile(pgid_probe=self.unavailable())
        self.assertEqual(res.finished, [],
                         f"an unreadable `ps` is not evidence of death: "
                         f"{self.reap_report(w.tag, res)}")
        self.assertIn(w, self.workers,
                      "the worker must keep its slot, its claim and its renewer")
        self.assertTrue(w.alive(), "and must not have been killed")
        self.assertEqual(len(self.hub.list("refs/fleet/claims/gate/")), 1,
                         "the live worker's CAS'd lease must not be released")

    def test_the_refusal_is_named_rather_than_a_silently_idle_step(self):
        self.start_one_gate()
        res = self.reconcile(pgid_probe=self.unavailable())
        self.assertIn("process-listing-unavailable", [r for r, _ in res.refused],
                      f"`fleet status --why` must be able to say why: {res.refused}")

    def test_no_second_gate_is_started_on_the_branch_the_live_one_holds(self):
        w = self.start_one_gate()
        res = self.reconcile(pgid_probe=self.unavailable())
        self.assertNotIn(w.branch, [x.branch for x in self.workers if x is not w],
                         f"DUPLICATE GATE: a second worker took {w.branch} while the "
                         f"first was still running; started={res.started}")
        self.assertEqual(
            len([x for x in self.workers if x.branch == w.branch]), 1,
            f"exactly one worker may hold {w.branch}; workers={self.worker_states()}")

    def test_end_to_end_a_failing_ps_neither_reaps_nor_duplicates(self):
        """THE WHOLE DEFECT, through the production probe.

        No `pgid_probe` is injected: `reconcile_once` calls the real
        `live_pgids`, and only the `ps` CHILD is made to fail -- the shape
        a loaded host produces (EAGAIN under process pressure, EMFILE, a
        timeout). Deliberately free of any reference to
        `ProcessListingUnavailable`, so it runs unchanged against
        2c7716a7 and fails there on BEHAVIOUR rather than on a missing
        symbol. Measured at that commit: `finished=['testhost-one-...']`
        for a worker still alive, and a second gate started on its branch.
        """
        w = self.start_one_gate()
        real = subprocess.run

        def ps_fails(argv, *a, **kw):
            if isinstance(argv, (list, tuple)) and argv and argv[0] == "ps":
                raise OSError(35, "Resource temporarily unavailable")
            return real(argv, *a, **kw)

        with mock.patch.object(runner_mod.subprocess, "run", ps_fails):
            res = self.reconcile()

        self.assertTrue(w.alive(), "the fixture worker must still be running, or "
                                   "this test is measuring a genuine exit")
        self.assertEqual(res.finished, [],
                         f"a `ps` that could not run is not evidence that a worker "
                         f"died: {self.reap_report(w.tag, res)}")
        self.assertIn(w, self.workers, "the live worker must keep its slot")
        self.assertEqual(
            len([x for x in self.workers if x.branch == w.branch]), 1,
            f"DUPLICATE GATE: {w.branch} is held by a live gate and was started "
            f"again; started={res.started} workers={self.worker_states()}")
        self.assertEqual(len(self.hub.list("refs/fleet/claims/gate/")), 1,
                         "the live worker's CAS'd lease must still be on the hub")

    def test_the_bug_present_shape_reaps_the_live_worker_and_duplicates_it(self):
        """NEGATIVE CONTROL. `pgid_probe` returning `set()` is precisely what
        the old `live_pgids` handed this step when `ps` failed. The reap
        must fire, the lease must be released, and the branch must be taken
        a second time -- if any of that stops being true, the three tests
        above stop being evidence about anything."""
        w = self.start_one_gate()
        res = self.reconcile(pgid_probe=lambda: set())
        self.assertIn(w.tag, res.finished,
                      f"the control must reproduce the reap: "
                      f"{self.reap_report(w.tag, res)}")
        self.assertTrue(w.alive(),
                        "and the reaped worker must still be RUNNING -- that is what "
                        "makes the duplicate below a duplicate rather than a restart")
        self.assertIn(w.branch, [x.branch for x in self.workers],
                      f"the control must reproduce the duplicate gate on {w.branch}: "
                      f"started={res.started} workers={self.worker_states()}")
        # The reap dropped `w` from `self.workers`, so `tearDown` no longer
        # knows to stop it. Put it back, or this control leaves a parked
        # stub behind for as long as the machine is up.
        self.workers.append(w)
