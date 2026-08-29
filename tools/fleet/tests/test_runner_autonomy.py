#!/usr/bin/env python3
"""`autonomous_when_serverless` (Keel 3R-2 steps 8-10; SPEC SS12).

Instrument: `keel.runner.AutonomyGate` driven with an INJECTED monotonic
clock and an injected lease verdict for the state-machine cases, and
`runner.run_daemon` against a real fixture hub with a parked stub gate
for the dispatch cases. No case sleeps for the production sixty seconds;
every case that is about the sixty seconds drives the clock instead, so
the number under test is the one in the code rather than the one the
wall clock happened to reach.

WHAT EACH CLASS PINS, and the bug it goes red for.

  TestAutonomyTrigger -- the trigger is the SERVER LEASE REF, never
  `FallbackHub.degraded_since`. The control is `test_the_trigger_is_the
  _lease_ref_and_not_degraded_since`: a hub whose lease is live while
  `degraded_since` has been set must NOT go autonomous. Driving off
  `degraded_since` instead would make this host schedule unilaterally
  after a five-second blip, and would forget it had done so on the next
  restart (the field is in-memory and `build_hub` makes a fresh
  `FallbackHub` every time).

  TestAutonomyHysteresis -- entry needs sixty CONTINUOUS seconds, and
  exit needs a live lease across a full loop. Its negative control
  (`hysteresis_disabled`, SPEC step 12b) is MANDATORY: with the
  hysteresis off, a lease that flaps live/down/live/down toggles this
  host between two scheduling regimes every cycle, and each toggle
  changes both the dispatch set and the loop interval.

  TestAutonomousDispatch -- while autonomous the runner gates and does
  NOT dispatch agents, and it does so through `run_daemon` and the real
  `fleetd.reconcile_once`, not against a stub that agrees by
  construction.
"""

from __future__ import annotations

import contextlib
import os
import signal as signal_mod
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest import mock

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import claim as claim_mod  # noqa: E402
import fleetd  # noqa: E402
import keel.runner as runner  # noqa: E402
import workqueue  # noqa: E402
from _env import HermeticCase, scrub_env  # noqa: E402
from _fixtures import make_hub  # noqa: E402
from fleetlib import HubUnreachableError  # noqa: E402
from keel.election import SERVER_LEASE_REF  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
HOST = "autonomyhost"


def iso(dt: datetime) -> str:
    """The EXACT spelling `claim._owns` compares against, and the one
    `election`'s `ServerClaim` writes."""
    return claim_mod._iso(dt)


@contextlib.contextmanager
def hysteresis_disabled():
    """NEGATIVE CONTROL (Keel 3R-2 step 12b). Disable EXACTLY the exit
    hysteresis: require ONE live observation to leave autonomy instead of
    a full loop's worth.

    Nothing else changes -- same trigger, same entry gate, same lease
    reads. `AutonomyGate` resolves this constant in `__init__` rather than
    in its signature defaults precisely so this patch takes effect.
    """
    with mock.patch.object(runner, "AUTONOMY_EXIT_LIVE_OBSERVATIONS", 1):
        yield


class _FakeClock:
    """A monotonic clock the test advances by hand."""

    def __init__(self, start: float = 1000.0):
        self.t = float(start)

    def __call__(self) -> float:
        return self.t

    def advance(self, seconds: float) -> None:
        self.t += float(seconds)


class _LeaseHub:
    """A hub that answers exactly one ref: the server lease.

    `payload` is what `read(SERVER_LEASE_REF)` returns; `raises` makes
    every read fail, which is the third observation ("unreadable").
    `degraded_since` is here ONLY so a test can set it and prove the gate
    does not look at it.
    """

    url = "fake://state.git"
    code_url = url

    def __init__(self, payload=None, raises: bool = False):
        self.payload = payload
        self.raises = raises
        self.degraded_since = None
        self.reads: list = []

    def read(self, ref):
        self.reads.append(ref)
        if self.raises:
            raise HubUnreachableError("simulated: state repo unreachable")
        return self.payload

    def live(self, seconds_left: float = 300.0):
        self.payload = {
            "holder_host": "someserver",
            "started_at": iso(datetime.now(timezone.utc) - timedelta(seconds=60)),
            "expires_at": iso(datetime.now(timezone.utc)
                              + timedelta(seconds=seconds_left)),
        }
        self.raises = False

    def expired(self):
        self.payload = {
            "holder_host": "someserver",
            "started_at": iso(datetime.now(timezone.utc) - timedelta(seconds=900)),
            "expires_at": iso(datetime.now(timezone.utc) - timedelta(seconds=1)),
        }
        self.raises = False

    def absent(self):
        self.payload = None
        self.raises = False


# --------------------------------------------------------------------- #
# 1. The trigger
# --------------------------------------------------------------------- #


class TestAutonomyTrigger(HermeticCase):

    def gate(self, hub, clock, **kw):
        return runner.AutonomyGate(hub, enabled=True, clock=clock, **kw)

    def test_disabled_by_default_reads_nothing_and_never_engages(self):
        """SPEC SS12: config, default false, enabled on the i7 only. A
        runner that has not opted in must not even READ the lease ref --
        the feature has to cost an unconfigured host nothing at all."""
        hub = _LeaseHub()
        hub.absent()
        clock = _FakeClock()
        g = runner.AutonomyGate(hub, enabled=False, clock=clock)
        for _ in range(10):
            clock.advance(3600)
            self.assertFalse(g.observe())
        self.assertEqual(hub.reads, [],
                         "a disabled gate must not read the lease ref at all")

    def test_the_lease_ref_it_watches_is_the_one_election_writes(self):
        """A runner watching the wrong ref sees a lease that is absent
        forever and goes autonomous on a perfectly healthy fleet. The ref
        is imported from `keel.election`, and this asserts the import is
        the one that reaches the read."""
        hub = _LeaseHub()
        hub.absent()
        self.gate(hub, _FakeClock()).observe()
        self.assertEqual(hub.reads, [SERVER_LEASE_REF])
        self.assertEqual(SERVER_LEASE_REF, "refs/fleet/claims/server/singleton")

    def test_an_expired_lease_counts_as_down_and_a_live_one_does_not(self):
        clock = _FakeClock()
        hub = _LeaseHub()
        hub.live()
        g = self.gate(hub, clock)
        self.assertFalse(g.observe())
        self.assertEqual(g.last_observation, "live")
        hub.expired()
        self.assertFalse(g.observe(), "one down observation is not sixty seconds")
        self.assertEqual(g.last_observation, "down")
        clock.advance(61)
        self.assertTrue(g.observe())

    def test_an_unreadable_lease_freezes_the_gate_rather_than_moving_it(self):
        """The third observation. A state repo this runner cannot reach is
        not evidence about the SERVER, and counting it as absence would
        make a host go autonomous because its OWN git route broke -- the
        one condition under which it can least afford to schedule
        unilaterally."""
        clock = _FakeClock()
        hub = _LeaseHub()
        hub.live()
        g = self.gate(hub, clock)
        g.observe()
        hub.raises = True
        for _ in range(20):
            clock.advance(3600)
            self.assertFalse(
                g.observe(),
                "an unreadable lease must never be read as an absent one")
        self.assertEqual(g.last_observation, "unreadable")
        self.assertIsNone(g.down_since, "and it must not start the clock either")

    def test_the_trigger_is_the_lease_ref_and_not_degraded_since(self):
        """The whole point of step 9, asserted directly.

        `FallbackHub.degraded_since` is the convenient signal and the
        wrong one: in-memory, reset by every runner restart because
        `build_hub` constructs a fresh `FallbackHub`, and non-None after
        a five-second blip. Here it is set to a value an hour old while
        the lease is live -- a gate driven off it would engage; this one
        must not.
        """
        clock = _FakeClock()
        hub = _LeaseHub()
        hub.live()
        hub.degraded_since = datetime.now(timezone.utc) - timedelta(hours=1)
        g = self.gate(hub, clock)
        for _ in range(10):
            clock.advance(600)
            self.assertFalse(
                g.observe(),
                "AUTONOMY DRIVEN OFF degraded_since: the lease is live and this "
                "host went autonomous anyway")


# --------------------------------------------------------------------- #
# 2. Entry gate and exit hysteresis
# --------------------------------------------------------------------- #


class TestAutonomyHysteresis(HermeticCase):

    def setUp(self):
        super().setUp()
        self.clock = _FakeClock()
        self.hub = _LeaseHub()
        self.gate = runner.AutonomyGate(self.hub, enabled=True, clock=self.clock)

    def test_entry_needs_sixty_continuous_seconds(self):
        self.hub.absent()
        self.gate.observe()                      # clock starts here
        self.clock.advance(59)
        self.assertFalse(self.gate.observe(), "59s is not 60s")
        # One live observation RESETS the run: "continuously" is the word
        # in SPEC SS12, and a gate that accumulated across a recovery
        # would engage on a fleet whose server was only briefly away.
        self.hub.live()
        self.gate.observe()
        self.hub.absent()
        self.gate.observe()
        self.clock.advance(59)
        self.assertFalse(
            self.gate.observe(),
            "the absence clock must restart after a live observation, not resume")
        self.clock.advance(2)
        self.assertTrue(self.gate.observe())

    def test_exit_needs_a_live_lease_across_a_full_loop(self):
        self.hub.absent()
        self.gate.observe()
        self.clock.advance(61)
        self.assertTrue(self.gate.observe())
        self.hub.live()
        self.clock.advance(15)
        self.assertTrue(
            self.gate.observe(),
            "one live observation is not a loop's worth -- that is the hysteresis")
        self.clock.advance(15)
        self.assertFalse(self.gate.observe(), "two consecutive live: exit")

    def test_a_flapping_lease_never_shakes_this_host_out_of_autonomy(self):
        """THE property the hysteresis exists for. A server settling, an
        election handing over, or a five-second blip produces a lease that
        alternates live/down. Each exit-and-re-enter changes both the
        dispatch set (agents on, agents off) and the loop interval (15s,
        60s), so a host that toggles every cycle is a host whose scheduling
        regime is decided by network noise."""
        self.hub.absent()
        self.gate.observe()
        self.clock.advance(61)
        self.assertTrue(self.gate.observe(), "setup: must be autonomous first")

        toggles = 0
        for i in range(12):
            (self.hub.live if i % 2 == 0 else self.hub.absent)()
            self.clock.advance(15)
            before = self.gate.autonomous
            after = self.gate.observe()
            if before != after:
                toggles += 1
        self.assertEqual(
            toggles, 0,
            f"AUTONOMY FLAPPED: the lease alternated live/down and this host "
            f"changed scheduling regime {toggles} time(s); the exit hysteresis "
            f"is what must absorb that")
        self.assertTrue(self.gate.autonomous)

    def test_negative_control_without_the_hysteresis_a_flapping_lease_flaps(self):
        """Keel 3R-2 step 12b. One line disabled, and the failure must be
        the FLAP -- matched by its text, not merely by a non-zero exit."""
        with hysteresis_disabled():
            case = TestAutonomyHysteresis(
                "test_a_flapping_lease_never_shakes_this_host_out_of_autonomy")
            result = unittest.TestResult()
            case.run(result)
        self.assertEqual(len(result.failures), 1,
                         f"the control must go RED: {result.failures} {result.errors}")
        message = result.failures[0][1]
        self.assertIn(
            "AUTONOMY FLAPPED", message,
            f"the flap test failed, but not because it flapped -- the control "
            f"proves nothing unless the toggle is what broke. Got: {message}")


# --------------------------------------------------------------------- #
# 3. What an autonomous runner actually does: gates, never agents
# --------------------------------------------------------------------- #


class TestAutonomousDispatch(HermeticCase):
    """Through `run_daemon` and the real `fleetd.reconcile_once`."""

    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.addCleanup(self.tmpdir.cleanup)
        self.bare = self.tmp / "hub.git"
        subprocess.run(["git", "init", "-q", "--bare", str(self.bare)], check=True,
                       env=scrub_env())
        self._seed_code()
        self.hub = make_hub(self, str(self.bare), workdir=self.tmp / "cache")
        self.log_dir = self.tmp / "logs"
        self.stub = self.tmp / "stub-gate.sh"
        self.stub.write_text(
            "#!/bin/bash\n"
            f"ALL={self.tmp}/stop-all\n"
            'while [ ! -f "$ALL" ]; do sleep 0.2; done\n'
            "exit 0\n"
        )
        self.stub.chmod(0o755)
        #: Stub gates this test started, reaped by `_stop_all`.
        self.started_procs: list = []
        # Registered in setUp, so it runs AFTER every cleanup a test body
        # adds (unittest runs cleanups LIFO). A `popen.wait` registered
        # from inside the test would otherwise run BEFORE the stop file
        # exists and block for its whole timeout on a gate nothing has
        # told to finish.
        self.addCleanup(self._stop_all)
        self._old_term = signal_mod.getsignal(signal_mod.SIGTERM)
        self._old_int = signal_mod.getsignal(signal_mod.SIGINT)
        self.addCleanup(signal_mod.signal, signal_mod.SIGTERM, self._old_term)
        self.addCleanup(signal_mod.signal, signal_mod.SIGINT, self._old_int)

    def _stop_all(self):
        try:
            (self.tmp / "stop-all").write_text("")
        except OSError:
            pass
        for p in self.started_procs:
            try:
                p.wait(timeout=30)
            except Exception:
                pass

    def _seed_code(self):
        work = self.tmp / "seed"
        env = scrub_env(GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t",
                        GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t")
        subprocess.run(["git", "init", "-q", str(work)], check=True, env=env)
        (work / "f.txt").write_text("tip\n")
        subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
        subprocess.run(["git", "-C", str(work), "commit", "-qm", "tip"],
                       check=True, env=env)
        subprocess.run(["git", "-C", str(work), "push", "-q", str(self.bare),
                        f"HEAD:{workqueue.TIP_REF}"], check=True, env=env)
        (work / "g.txt").write_text("one\n")
        subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
        subprocess.run(["git", "-C", str(work), "commit", "-qm", "one"],
                       check=True, env=env)
        subprocess.run(["git", "-C", str(work), "push", "-q", str(self.bare),
                        "HEAD:refs/heads/staging/one"], check=True, env=env)

    def _desired(self, *, gates: int, agents: int):
        doc = {"hosts": {HOST: {"enabled": True, "gates": gates, "agents": agents}}}
        sha = self.hub.sha(fleetd.DESIRED_REF)
        if sha is None:
            self.assertTrue(self.hub.create(fleetd.DESIRED_REF, doc))
        else:
            self.assertTrue(self.hub.update(fleetd.DESIRED_REF, doc, sha))

    def test_an_autonomous_runner_gates_and_never_dispatches_an_agent(self):
        """SPEC SS12, both halves in one run: "never less capable than
        today's hubless Stage 1" (so it MUST still gate) and gates only
        (so it must NOT dispatch, however many agent slots `desired`
        offers). `dispatch_agents` is watched rather than inferred from a
        count -- an agent run is a PAID claude/codex invocation, and
        "no agent worker appeared" is a weaker statement than "the
        dispatcher was never called"."""
        # DRAINED for the first cycle, so nothing starts before autonomy
        # has engaged. The entry gate needs two observations (one to start
        # the absence clock, one to find it has elapsed), so cycle 1 is by
        # construction NOT autonomous -- and asserting over the whole run
        # would then be asserting about a cycle the property does not
        # cover. The target is raised between the cycles, so the gate that
        # starts is one an AUTONOMOUS runner started.
        self._desired(gates=0, agents=3)
        seen: list = []
        results: list = []
        dispatch_calls_during: list = []

        with mock.patch.object(runner, "AUTONOMY_ENTER_AFTER_S", 0.0), \
             mock.patch.object(fleetd, "dispatch_agents") as dispatcher:

            def scripted(hub, host, workers, gate_command, log_dir, repo_root, **kw):
                seen.append(dict(kw))
                before = dispatcher.call_count
                res = fleetd.reconcile_once(
                    hub, host, workers, gate_command, log_dir, repo_root,
                    disk_probe=lambda: 100.0, mem_probe=lambda: 32.0, **kw)
                results.append(res)
                dispatch_calls_during.append(dispatcher.call_count - before)
                if len(results) == 1:
                    self._desired(gates=1, agents=3)
                else:
                    self.started_procs.extend(
                        w.popen for w in workers if w.popen is not None)
                    os.kill(os.getpid(), signal_mod.SIGTERM)
                return res

            rc = runner.run_daemon(
                self.hub, HOST,
                gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT,
                interval=0, reconcile=scripted,
                autonomous_when_serverless=True,
                autonomous_interval=0,
            )

        self.assertEqual(rc, 0)
        self.assertEqual(len(results), 2, "two cycles, the second autonomous")
        # There is no server lease on this fixture store at all, so every
        # observation is DOWN and the (patched-to-zero) entry gate engages
        # on the second one.
        self.assertTrue(seen[0]["agents_allowed"],
                        "cycle 1 is not yet autonomous")
        self.assertIs(seen[1]["agents_allowed"], False,
                      f"an autonomous runner must not be allowed agents: {seen}")
        # GATES: the autonomous cycle started one.
        self.assertEqual(
            len(results[1].started), 1,
            f"AUTONOMOUS RUNNER DID NOT GATE -- SPEC SS12 requires it to stay at "
            f"least as capable as hubless Stage 1: {results[1].refused}")
        # AGENTS: and dispatched none, watched at the dispatcher itself. An
        # agent run is a PAID claude/codex invocation, so "the dispatcher
        # was never called" is the statement worth making; "no agent worker
        # appeared" would also be true on a host with no agent CLI.
        self.assertEqual(
            dispatch_calls_during[1], 0,
            f"AUTONOMOUS AGENT DISPATCH: an autonomous host called "
            f"dispatch_agents {dispatch_calls_during[1]} time(s)")
        self.assertIn("autonomous-no-agents",
                      [r for r, _ in results[1].refused],
                      f"the refusal must be named: {results[1].refused}")

    def test_a_live_server_lease_leaves_agent_dispatch_alone(self):
        """The in-suite control for the case above: same fixture, same
        desired state, a LIVE server lease -- and the agent half is
        untouched. Without this, "no agent was dispatched" would be
        evidence of nothing, because this fixture host has no agent CLI
        either."""
        self._desired(gates=1, agents=3)
        self.assertTrue(self.hub.create(SERVER_LEASE_REF, {
            "holder_host": "someserver",
            "started_at": iso(datetime.now(timezone.utc) - timedelta(seconds=60)),
            "expires_at": iso(datetime.now(timezone.utc) + timedelta(seconds=600)),
        }))
        seen: list = []

        def scripted(hub, host, workers, gate_command, log_dir, repo_root, **kw):
            seen.append(dict(kw))
            os.kill(os.getpid(), signal_mod.SIGTERM)
            return fleetd.ReconcileResult()

        with mock.patch.object(runner, "AUTONOMY_ENTER_AFTER_S", 0.0):
            rc = runner.run_daemon(
                self.hub, HOST, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT,
                interval=0, reconcile=scripted,
                autonomous_when_serverless=True, autonomous_interval=0)

        self.assertEqual(rc, 0)
        self.assertTrue(seen[0]["agents_allowed"],
                        "a live lease must leave agent dispatch armed")


if __name__ == "__main__":
    unittest.main()
