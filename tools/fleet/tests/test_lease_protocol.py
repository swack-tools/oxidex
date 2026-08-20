#!/usr/bin/env python3
"""Property tests for the lease protocol: claim.py's renewer + fleetd's
stop-work wiring (ARCH-FIX-SPEC R2).

WHY THIS FILE EXISTS. `test_claim.py` proves every half of the lease
primitive in isolation -- acquire wins once, renew moves `expires_at`,
release deletes, an expired claim is reapable -- and all of it passed,
every day, while the guarantee those halves compose into did not hold at
all. `fleetd` acquired claims with `acquire_or_reap()` and never started
the renewer, so every claim held longer than `LEASE_TTL` (600s -- i.e.
every real gate) expired mid-work and became reapable by any host. The
host singleton expired ten minutes after each daemon start. Nothing
failed, nothing logged, no unit test was red.

The distinguishing feature of that defect is TIME: it is invisible at
unit-test timescale and certain at production timescale. So the tests
here are written the only way that catches it -- hold a claim across a
worker that OUTLIVES the TTL, and poll the hub the whole way through --
with TTL and renewal cadence compressed by `FLEET_TEST_TTL_S` /
`FLEET_TEST_RENEW_S` so ten minutes of production becomes four seconds of
test. The compression is the mechanism under test, not a shortcut around
it: `tools/fleet/tests/test_seams.py` runs the same property at real
timescale in burn-in.

NAME THE INSTRUMENT. Liveness is observed from a SECOND `Hub` instance
with its own object cache -- what another host would see -- never from
the holder's own in-memory state, which is exactly the thing that was
wrong. A killed process group is verified with `fleetd.live_pgids()`
(`ps -eo pgid=`, a listing), never `pgrep`, which matches the invoking
command line and over-reported all day on 2026-08-14.

Every test runs against a throwaway `git init --bare` fixture under the
system temp dir; `setUp` asserts that before any test body runs. Plain
unittest, standard library only.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import claim as claim_mod  # noqa: E402
import fleetd  # noqa: E402
from claim import (  # noqa: E402
    Claim,
    claim_ref,
    is_claim_live,
    reap_expired,
)
from fleetlib import Hub  # noqa: E402

# Compressed timescale. TTL=4s/renew=1s stands in for the production
# 600s/120s (the same 4:1 headroom), and a 10s stub worker stands in for a
# gate: it outlives its TTL 2.5x over, which is the whole point.
TTL_S = 4.0
RENEW_S = 1.0
WORKER_S = 10.0

HUB_TIP_REF = "refs/heads/refactor/tag-machinery"


# --------------------------------------------------------------------- #
# Fixtures
# --------------------------------------------------------------------- #


class LeaseFixture(unittest.TestCase):
    """A throwaway bare hub, plus the guard that it is never the real one."""

    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="lease-proto-")
        self.tmp = Path(self._tmp_root)
        self.hub_path = str(self.tmp / "hub.git")
        init = subprocess.run(
            ["git", "init", "--quiet", "--bare", self.hub_path], capture_output=True
        )
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)

        self.hub = Hub(url=self.hub_path, workdir=str(self.tmp / "cache"))
        self._spawned: list = []
        self._claims: list = []

    def tearDown(self):
        # Release tracked claims FIRST. `addCleanup` runs after tearDown,
        # so a claim released there would still have a renewer thread
        # pushing at an object cache this method had already deleted.
        for c in self._claims:
            try:
                c.release()
            except Exception:  # noqa: BLE001 -- teardown must not mask a failure
                pass
        for popen in self._spawned:
            if popen.poll() is None:
                try:
                    os.killpg(popen.pid, signal.SIGKILL)
                except OSError:
                    popen.kill()
                try:
                    popen.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    pass
        shutil.rmtree(self._tmp_root, ignore_errors=True)

    def track(self, claim: Claim) -> Claim:
        """Release this claim (and stop its renewer) before the fixture
        tears its object cache down."""
        self._claims.append(claim)
        return claim

    def observer(self) -> Hub:
        """A second Hub with its own cache: another host's view of the hub."""
        other = tempfile.mkdtemp(prefix="lease-proto-observer-")
        self.addCleanup(shutil.rmtree, other, ignore_errors=True)
        return Hub(url=self.hub_path, workdir=other)

    def stub_worker(self, seconds: float) -> subprocess.Popen:
        """A process in its OWN session (pgid == pid), standing in for a
        gate: it sleeps, it can be killed by group, and it outlives the
        lease TTL."""
        script = self.tmp / f"worker-{len(self._spawned)}.sh"
        script.write_text(f"#!/bin/bash\nsleep {seconds}\n")
        script.chmod(0o755)
        popen = subprocess.Popen(
            ["/bin/bash", str(script)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
        )
        self._spawned.append(popen)
        return popen

    def wait_until(self, predicate, timeout: float, what: str, poll: float = 0.2):
        """Poll `predicate` until true; fail with `what` if it never is.
        Returns the elapsed seconds, so callers can assert on the LATENCY
        of a state change and not merely that it eventually happened."""
        start = time.monotonic()
        deadline = start + timeout
        while time.monotonic() < deadline:
            if predicate():
                return time.monotonic() - start
            time.sleep(poll)
        self.fail(f"{what} did not happen within {timeout:g}s")


def renewer_threads(claim: Claim) -> list:
    """Live threads this claim's renewer would have created, found by
    name -- so 'exactly one renewer' is checked against the interpreter's
    thread table, not against the object's own bookkeeping."""
    want = f"claim-renew-{claim.kind}-{claim.key}"
    return [t for t in threading.enumerate() if t.name == want and t.is_alive()]


# --------------------------------------------------------------------- #
# The renewer's lifecycle is owned by acquire/release, not by the caller
# --------------------------------------------------------------------- #


class TestRenewerLifecycle(LeaseFixture):
    def test_acquire_starts_the_renewer(self):
        c = Claim(self.hub, "gate", "acq", work_key="staging/acq", ttl=TTL_S,
                  renew_interval=RENEW_S)
        self.track(c).acquire()
        self.assertTrue(
            c.renewer_running(),
            "acquire() must start the renewer -- fleetd only ever called "
            "acquire()/acquire_or_reap(), and that is how every claim expired",
        )
        self.assertEqual(len(renewer_threads(c)), 1)

    def test_acquire_or_reap_starts_the_renewer(self):
        c = Claim(self.hub, "gate", "aor", work_key="staging/aor", ttl=TTL_S,
                  renew_interval=RENEW_S)
        self.track(c).acquire_or_reap()
        self.assertTrue(c.renewer_running())
        self.assertEqual(len(renewer_threads(c)), 1)

    def test_acquire_or_reap_after_reaping_a_stale_claim_still_renews(self):
        """The reap branch is a different code path to the plain-win one,
        and it is the branch fleetd's start_gate actually takes."""
        dead = Claim(self.hub, "gate", "stale", work_key="staging/stale", ttl=0)
        dead.acquire()  # already expired; no renewer (nothing to keep alive)
        self.assertFalse(dead.renewer_running())

        successor = Claim(self.observer(), "gate", "stale", work_key="staging/stale",
                          ttl=TTL_S, renew_interval=RENEW_S)
        self.track(successor).acquire_or_reap()
        self.assertTrue(successor.renewer_running())

    def test_release_stops_the_renewer(self):
        c = Claim(self.hub, "gate", "rel", work_key="staging/rel", ttl=TTL_S,
                  renew_interval=RENEW_S)
        c.acquire()
        self.assertTrue(c.renewer_running())
        c.release()
        self.assertFalse(c.renewer_running())
        self.assertEqual(renewer_threads(c), [])

    def test_context_manager_does_not_double_start_the_renewer(self):
        with Claim(self.hub, "gate", "cm", work_key="staging/cm", ttl=TTL_S,
                   renew_interval=RENEW_S) as c:
            self.assertTrue(c.renewer_running())
            self.assertEqual(
                len(renewer_threads(c)), 1,
                "the context manager must not start a second renewer on top "
                "of the one acquire() already started",
            )
        self.assertFalse(c.renewer_running())
        self.assertEqual(renewer_threads(c), [])
        self.assertIsNone(self.hub.sha(c.ref), "__exit__ must still release")

    def test_zero_ttl_claim_starts_no_renewer(self):
        """A ttl<=0 claim is a deliberately-expired test fixture. Starting
        a renewer on one would spin at a clamped-to-zero interval."""
        c = Claim(self.hub, "gate", "zero", work_key="staging/zero", ttl=0)
        self.track(c).acquire()
        self.assertFalse(c.renewer_running())


class TestTimescaleOverrides(LeaseFixture):
    def test_env_overrides_ttl_and_renew_interval(self):
        prior = {k: os.environ.get(k) for k in (claim_mod.TTL_ENV, claim_mod.RENEW_ENV)}
        os.environ[claim_mod.TTL_ENV] = "8"
        os.environ[claim_mod.RENEW_ENV] = "2"
        try:
            c = Claim(self.hub, "gate", "env", work_key="staging/env")
            self.assertEqual(c.ttl, 8.0)
            self.assertEqual(c.renew_interval, 2.0)
        finally:
            for k, v in prior.items():
                if v is None:
                    os.environ.pop(k, None)
                else:
                    os.environ[k] = v

    def test_explicit_argument_beats_the_env_override(self):
        prior = os.environ.get(claim_mod.TTL_ENV)
        os.environ[claim_mod.TTL_ENV] = "8"
        try:
            c = Claim(self.hub, "gate", "env2", work_key="staging/env2", ttl=30,
                      renew_interval=5)
            self.assertEqual(c.ttl, 30.0)
            self.assertEqual(c.renew_interval, 5.0)
        finally:
            if prior is None:
                os.environ.pop(claim_mod.TTL_ENV, None)
            else:
                os.environ[claim_mod.TTL_ENV] = prior

    def test_unparseable_env_falls_back_to_the_production_default(self):
        prior = os.environ.get(claim_mod.TTL_ENV)
        os.environ[claim_mod.TTL_ENV] = "not-a-number"
        try:
            c = Claim(self.hub, "gate", "env3", work_key="staging/env3")
            self.assertEqual(c.ttl, float(claim_mod.LEASE_TTL))
        finally:
            if prior is None:
                os.environ.pop(claim_mod.TTL_ENV, None)
            else:
                os.environ[claim_mod.TTL_ENV] = prior

    def test_renew_interval_at_or_past_the_ttl_is_clamped(self):
        """A cadence >= the TTL is a lease that expires between renewals.
        Clamping can only renew MORE often than asked, so it can never
        manufacture a lost lease."""
        c = Claim(self.hub, "gate", "clamp", work_key="staging/clamp", ttl=10,
                  renew_interval=3600)
        self.assertLessEqual(c.renew_interval, c.ttl / 2.0)


# --------------------------------------------------------------------- #
# R2 property (a): a claim held through work that outlives its TTL is
# CONTINUOUSLY live on the hub, and reap_expired takes nothing.
# --------------------------------------------------------------------- #


class TestClaimOutlivesItsTTL(LeaseFixture):
    def test_claim_is_continuously_live_while_a_worker_outlives_the_ttl(self):
        # The stub outlasts the observation window by a wide margin. The
        # property under test is the lease, so the worker must never be
        # the thing that ends first -- a race between `sleep 10` and a
        # 10s poll loop would fail this test for a reason that has
        # nothing to do with leases.
        worker = self.stub_worker(WORKER_S * 3)
        c = Claim(
            self.hub, "gate", "long", work_key="staging/long",
            ttl=TTL_S, renew_interval=RENEW_S, pid=worker.pid, pgid=worker.pid,
        )
        self.track(c).acquire()

        watcher = self.observer()  # what another host sees
        expiries = []
        polls = 0
        start = time.monotonic()
        # Poll for well past 2x TTL. If the renewer stops, `is_claim_live`
        # goes false within TTL_S and the next poll catches it.
        while time.monotonic() - start < WORKER_S:
            elapsed = time.monotonic() - start
            self.assertTrue(
                is_claim_live(watcher, "gate", "long"),
                f"claim went dead on the hub after {elapsed:.1f}s "
                f"(ttl={TTL_S}s) while its worker was still running",
            )
            self.assertEqual(
                reap_expired(watcher), [],
                f"another host's reaper collected a live claim after {elapsed:.1f}s",
            )
            payload = watcher.read(c.ref)
            self.assertIsNotNone(payload)
            expiries.append(payload["expires_at"])
            self.assertIsNone(
                worker.poll(), "the stub worker must still be running while we poll"
            )
            polls += 1
            time.sleep(RENEW_S)
        held = time.monotonic() - start

        self.assertGreater(held, TTL_S * 2, "the hold must outlive the TTL, twice over")
        self.assertGreater(polls, 4, "too few observations to call this continuous")
        self.assertGreater(
            max(expiries), min(expiries),
            "expires_at never advanced: the claim survived the window because "
            "the TTL was long, not because anything renewed it",
        )
        self.assertFalse(c.lost, f"claim reported lost: {c.lost_reason}")
        self.assertIsNone(
            worker.poll(),
            "the worker was still running for the whole window, so every "
            "observation above was of a claim that was still doing work",
        )

        self.assertTrue(c.release())
        self.assertIsNone(self.hub.sha(c.ref), "release must delete the claim ref")


# --------------------------------------------------------------------- #
# R2 property (b): stop renewing, let the TTL pass -> reapable, once.
# --------------------------------------------------------------------- #


class TestStoppedRenewerExpires(LeaseFixture):
    def test_stopping_the_renewer_makes_the_claim_reapable_exactly_once(self):
        c = Claim(self.hub, "gate", "stops", work_key="staging/stops", ttl=TTL_S,
                  renew_interval=RENEW_S)
        c.acquire()
        watcher = self.observer()
        self.assertTrue(is_claim_live(watcher, "gate", "stops"))

        c.stop_renewer()  # simulate the pre-fix world: hold, never renew
        self.assertFalse(c.renewer_running())

        self.wait_until(
            lambda: not is_claim_live(watcher, "gate", "stops"),
            timeout=TTL_S + 5,
            what="an unrenewed claim expiring",
        )

        first = reap_expired(watcher)
        self.assertEqual(
            first, [claim_ref("gate", "stops")],
            "an expired claim must be reapable -- that is what makes a lease "
            "a lease and not a lock",
        )
        second = reap_expired(self.observer())
        self.assertEqual(second, [], "a reaped claim must not be reapable twice")
        self.assertIsNone(self.hub.sha(claim_ref("gate", "stops")))


# --------------------------------------------------------------------- #
# R2 property (c1): a claim reaped from under its renewer sets `lost`
# --------------------------------------------------------------------- #


class TestLostLeaseDetection(LeaseFixture):
    def test_reaped_claim_sets_lost_within_one_renew_interval(self):
        c = Claim(self.hub, "gate", "reaped", work_key="staging/reaped", ttl=TTL_S,
                  renew_interval=RENEW_S)
        self.track(c).acquire()
        self.assertFalse(c.lost)

        # Another host reaps us (CAS'd on the sha it observed), exactly as
        # claim.reap_expired would.
        other = self.observer()
        sha = other.sha(c.ref)
        self.assertIsNotNone(sha)
        self.assertTrue(other.delete(c.ref, expect_sha=sha))

        # One renewal interval to notice, plus slack for git round trips.
        latency = self.wait_until(
            lambda: c.lost, timeout=RENEW_S * 6 + 5,
            what="the renewer noticing its claim was reaped", poll=0.1,
        )
        self.assertLess(
            latency, RENEW_S * 2 + 2.0,
            f"loss detected {latency:.1f}s after the reap (renew interval "
            f"{RENEW_S}s): stop-work must follow the loss within about one "
            "renewal, because every second of lag is a second in which two "
            "hosts may be running the same work",
        )
        self.assertIn(c.ref, c.lost_reason)
        self.assertTrue(c.lost_reason.strip(), "a lost lease must carry a reason")

        # The renewer stops pushing at a hub that already gave its answer.
        self.wait_until(
            lambda: not c.renewer_running(), timeout=RENEW_S * 3 + 5,
            what="the renewer thread exiting after the lease was lost",
        )
        # Sticky: it does not flicker back on a later call.
        self.assertFalse(c.renew())
        self.assertTrue(c.lost)

    def test_claim_taken_over_by_another_holder_sets_lost(self):
        """Reaped AND re-claimed between two renewals: the ref exists, so
        the "does it still exist" check is not enough -- the payload has to
        prove it is still OUR acquisition."""
        c = Claim(self.hub, "gate", "stolen", work_key="staging/stolen", ttl=TTL_S,
                  renew_interval=RENEW_S)
        self.track(c).acquire()

        other = self.observer()
        sha = other.sha(c.ref)
        self.assertTrue(other.delete(c.ref, expect_sha=sha))
        thief = Claim(other, "gate", "stolen", work_key="staging/stolen",
                      holder_host="some-other-host", ttl=TTL_S, renew_interval=RENEW_S)
        self.track(thief).acquire()

        self.wait_until(
            lambda: c.lost, timeout=RENEW_S * 3 + 5,
            what="the renewer noticing another host now holds its claim", poll=0.1,
        )
        self.assertIn("some-other-host", c.lost_reason)
        # And the thief is untouched: our loser must not delete their claim.
        self.assertIsNotNone(self.hub.sha(thief.ref))
        self.assertFalse(thief.lost)

    def test_an_exception_in_renewal_marks_lost_instead_of_killing_the_renewer(self):
        """A renewer thread that dies on an unexpected exception leaves the
        lease unrenewed, `lost` False and nothing logged -- the original
        defect, restored by a stray traceback. Anything that stops renewal
        must end as `lost` once the lease can no longer be saved.

        Short TTL here on purpose: the property is about the DEADLINE, and
        a shorter one proves it just as well in a quarter of the time.
        """
        ttl, renew = 2.0, 0.5
        hub = self.observer()  # own Hub, so the monkeypatch stays local
        c = Claim(hub, "gate", "boom", work_key="staging/boom", ttl=ttl,
                  renew_interval=renew)
        self.track(c).acquire()

        def boom(*_a, **_k):
            raise ValueError("simulated non-Hub failure inside renewal")

        hub.update = boom
        self.wait_until(
            lambda: c.lost, timeout=ttl + renew * 4 + 5, poll=0.1,
            what="an exception-throwing renewal being reported as a lost lease",
        )
        self.assertIn("ValueError", c.lost_reason)
        self.assertIn("no successful renewal", c.lost_reason)

        # And it fires EARLY, by design: at the moment we declare the
        # lease lost, the hub-side claim is still (just) live, with no
        # more than one renewal interval left on it. That margin is the
        # whole point -- stop-work has to beat the reaper that will make
        # this branch claimable by somebody else, not follow it.
        payload = self.observer().read(c.ref)
        self.assertIsNotNone(payload)
        remaining = (
            datetime.fromisoformat(payload["expires_at"]) - datetime.now(timezone.utc)
        ).total_seconds()
        self.assertLessEqual(
            remaining, renew + 1.0,
            f"declared lost with {remaining:.1f}s still on the lease: that is "
            "not 'no renewal left to save it', it is giving up early",
        )
        self.assertGreater(
            remaining, -1.0,
            f"declared lost {-remaining:.1f}s AFTER the lease had already "
            "expired: another host could have claimed this branch first",
        )

    def test_a_broken_local_object_cache_marks_the_lease_lost(self):
        """`rm -rf ~/.fleetd` (or a full disk) breaks renewal locally, with
        the hub perfectly reachable. The hub-side deadline does not care
        why we failed, so neither does `lost`."""
        ttl, renew = 2.0, 0.5
        hub = self.observer()
        c = Claim(hub, "gate", "nocache", work_key="staging/nocache", ttl=ttl,
                  renew_interval=renew)
        self.track(c).acquire()
        shutil.rmtree(hub.workdir)
        self.wait_until(
            lambda: c.lost, timeout=ttl + renew * 4 + 5, poll=0.1,
            what="a locally-broken renewal being reported as a lost lease",
        )
        self.assertIn("no successful renewal", c.lost_reason)

    def test_release_after_loss_never_deletes_the_new_holders_claim(self):
        c = Claim(self.hub, "gate", "polite", work_key="staging/polite", ttl=TTL_S,
                  renew_interval=RENEW_S)
        c.acquire()
        other = self.observer()
        self.assertTrue(other.delete(c.ref, expect_sha=other.sha(c.ref)))
        thief = Claim(other, "gate", "polite", work_key="staging/polite",
                      holder_host="some-other-host", ttl=TTL_S, renew_interval=RENEW_S)
        self.track(thief).acquire()
        self.wait_until(lambda: c.lost, timeout=RENEW_S * 3 + 5,
                        what="loss detection", poll=0.1)

        c.release()  # the loser cleaning up
        self.assertIsNotNone(
            self.hub.sha(thief.ref),
            "a claim lost to another holder must never be deleted by the loser",
        )


# --------------------------------------------------------------------- #
# R2 property (c2): fleetd treats a lost lease as stop-work and KILLS
# --------------------------------------------------------------------- #


def make_fixture_hub(tmp: Path) -> Path:
    """A bare hub with a tip and one staging branch, so workqueue offers
    exactly one candidate."""
    assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
    bare = tmp / "fleethub.git"
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
    (work / "g.txt").write_text("branch\n")
    subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "branch work"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "push", "-q", str(bare),
                    "HEAD:refs/heads/staging/one"], check=True, env=env)
    return bare


class TestFleetdStopsWorkOnLostLease(unittest.TestCase):
    """The consumer half of the contract. `claim.lost` is inert unless
    somebody acts on it, and prose in a docstring is not somebody."""

    def setUp(self):
        self._tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmpdir.name)
        self.bare = make_fixture_hub(self.tmp)
        self.assertTrue(str(self.bare).startswith(tempfile.gettempdir()))
        self.hub = Hub(str(self.bare), workdir=self.tmp / "hubcache")
        self.host = "leasehost"
        self.workers: list = []

        # Compress time for the claims fleetd constructs internally: it
        # passes no ttl/renew_interval, so the env is the only lever.
        self._prior_env = {
            k: os.environ.get(k)
            for k in (claim_mod.TTL_ENV, claim_mod.RENEW_ENV, "FLEET_HOST")
        }
        os.environ[claim_mod.TTL_ENV] = str(TTL_S)
        os.environ[claim_mod.RENEW_ENV] = str(RENEW_S)
        os.environ["FLEET_HOST"] = self.host

        self.stub = self.tmp / "stub-gate.sh"
        self.stub.write_text(
            "#!/bin/bash\n"
            f"STOP={self.tmp}/stop-$2\n"
            'while [ ! -f "$STOP" ]; do sleep 0.2; done\n'
            "exit 0\n"
        )
        self.stub.chmod(0o755)

        self.set_desired(gates=1)

    def set_desired(self, gates: int):
        doc = {"generation": 1,
               "hosts": {self.host: {"gates": gates, "agents": 0, "enabled": True}},
               "limits": {}}
        cur = self.hub.sha(fleetd.DESIRED_REF)
        ok = (self.hub.create(fleetd.DESIRED_REF, doc) if cur is None
              else self.hub.update(fleetd.DESIRED_REF, doc, cur))
        self.assertTrue(ok)

    def tearDown(self):
        for w in self.workers:
            (self.tmp / f"stop-{w.tag}").write_text("")
            if w.popen is not None and w.popen.poll() is None:
                try:
                    os.killpg(w.pgid, signal.SIGKILL)
                except OSError:
                    pass
                try:
                    w.popen.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    pass
        for k, v in self._prior_env.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
        self._tmpdir.cleanup()

    def reconcile(self):
        return fleetd.reconcile_once(
            self.hub, self.host, self.workers,
            gate_command=[str(self.stub)],
            log_dir=self.tmp / "logs",
            repo_root=Path(__file__).resolve().parents[3],
            disk_probe=lambda: 100.0,
            mem_probe=lambda: 32.0,
        )

    def _wait(self, predicate, timeout, what):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.1)
        self.fail(f"{what} did not happen within {timeout:g}s")

    def test_worker_with_a_lost_lease_is_killed_by_process_group(self):
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")
        worker = self.workers[0]
        pgid = worker.pgid
        self.assertTrue(worker.alive())
        self.assertTrue(
            worker.claim.renewer_running(),
            "fleetd's gate claim must renew -- it never did, and that is R2",
        )
        self.assertIn(pgid, fleetd.live_pgids(), "instrument: ps -eo pgid=, not pgrep")

        # Another host reaps the claim out from under the running gate.
        other = Hub(str(self.bare), workdir=self.tmp / "othercache")
        sha = other.sha(worker.claim.ref)
        self.assertIsNotNone(sha)
        self.assertTrue(other.delete(worker.claim.ref, expect_sha=sha))

        self._wait(lambda: worker.claim.lost, RENEW_S * 3 + 5,
                   "the gate claim's renewer noticing the reap")

        # Drain to zero first, so this reconcile does exactly one thing.
        # (Starting a replacement afterwards would be legitimate -- the
        # branch is unclaimed again -- but it would muddy the assertion
        # about what the kill itself did.)
        self.set_desired(gates=0)
        res2 = self.reconcile()

        self.assertEqual(
            [t for t, _r in res2.killed], [worker.tag],
            "a worker whose lease is lost must be killed, not drained: another "
            "host may already be gating the same branch",
        )
        self.assertIn(worker.claim.ref, res2.killed[0][1])
        self.assertEqual(self.workers, [], "the killed worker must leave the roster")
        self.assertEqual(res2.finished, [], "a kill is not a normal finish")
        self.assertIsNotNone(
            worker.popen.poll(), "the gate process must be dead, and reaped"
        )
        self.assertNotIn(
            pgid, fleetd.live_pgids(),
            "the whole process GROUP must be gone -- a pid-only kill leaves "
            "cargo/rustc children orphaned (M8)",
        )

    def test_worker_with_a_healthy_lease_is_never_killed(self):
        """The control. A kill switch that fires on a healthy lease would
        be strictly worse than the bug it replaces."""
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")
        worker = self.workers[0]

        # Let several renewal intervals pass -- past the TTL -- with the
        # renewer doing its job.
        time.sleep(TTL_S + RENEW_S)

        res2 = self.reconcile()
        self.assertEqual(res2.killed, [], f"healthy lease was killed: {res2.killed}")
        self.assertEqual(self.workers, [worker])
        self.assertTrue(worker.alive())
        self.assertFalse(worker.claim.lost, worker.claim.lost_reason)
        self.assertTrue(
            is_claim_live(self.hub, "gate", "staging-one"),
            "the gate's claim must still be live on the hub past its TTL",
        )

    def test_kill_process_group_refuses_to_kill_fleetds_own_group(self):
        """The belt to start_new_session's braces: a fleetd that SIGKILLs
        its own process group takes out every gate on the host."""
        outcome = fleetd.kill_process_group(os.getpgrp(), grace=0.1)
        self.assertIn("refused", outcome)
        self.assertIn("own process group", outcome)


# --------------------------------------------------------------------- #
# R2 property: the fleetd HOST SINGLETON renews (it never did)
# --------------------------------------------------------------------- #


class TestFleetdSingletonRenews(LeaseFixture):
    def test_host_singleton_stays_live_past_its_ttl_and_is_released_on_exit(self):
        host = "singleton-test-host"
        home = self.tmp / "home"
        (home / ".fleetd").mkdir(parents=True)
        env = {
            **os.environ,
            "HOME": str(home),  # keep the daemon's hub cache out of the real ~
            "FLEET_HOST": host,
            claim_mod.TTL_ENV: str(TTL_S),
            claim_mod.RENEW_ENV: str(RENEW_S),
        }
        # Log to a file, not a pipe nobody reads: a full pipe buffer would
        # block the daemon and the test would blame the lease for it.
        daemon_log = open(self.tmp / "fleetd.log", "wb")
        self.addCleanup(daemon_log.close)
        daemon = subprocess.Popen(
            [sys.executable, str(Path(__file__).resolve().parents[1] / "fleetd.py"),
             "--hub", self.hub_path, "--interval", "1",
             "--log-dir", str(self.tmp / "gatelogs")],
            stdout=daemon_log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
            env=env, start_new_session=True,
        )
        self._spawned.append(daemon)

        watcher = self.observer()
        ref = claim_ref("host", host)
        self.wait_until(lambda: watcher.sha(ref) is not None, timeout=30,
                        what="fleetd taking its host singleton claim")

        expiries = []
        start = time.monotonic()
        while time.monotonic() - start < TTL_S * 2 + RENEW_S:
            elapsed = time.monotonic() - start
            self.assertTrue(
                is_claim_live(watcher, "host", host),
                f"the fleetd host singleton expired after {elapsed:.1f}s "
                f"(ttl={TTL_S}s) while the daemon was still running -- before "
                "this fix it expired 10 minutes after every daemon start",
            )
            self.assertIsNone(daemon.poll(), "the daemon must still be running")
            expiries.append(watcher.read(ref)["expires_at"])
            time.sleep(RENEW_S)

        self.assertGreater(
            max(expiries), min(expiries),
            "the singleton's expires_at never advanced: nothing renewed it",
        )

        daemon.send_signal(signal.SIGTERM)
        daemon.wait(timeout=30)
        self.assertIsNone(
            watcher.sha(ref),
            "a cleanly stopped fleetd must release its host singleton",
        )


class TestFixtureDaemonCannotSweepUnscopedWorkers(LeaseFixture):
    """END-TO-END PIN of the 2026-08-20 incident. This very class of
    fixture -- a REAL `fleetd.py` on a throwaway hub with production
    WORKER_MARKERS -- once ran its startup orphan sweep against the whole
    host: every real gate was marker-matched, claim-less on the EMPTY
    fixture hub, and therefore killed. A live, manually-launched gate died
    mid-run on the i7; the fleet-tests stage that ran this suite was
    itself the murder weapon.

    The scope token is the fix: the daemon may only kill worker-shaped
    groups stamped with ITS OWN hub's token. Here both kinds run side by
    side while a real daemon starts, adopts, and sweeps -- the unscoped
    decoy (argv carries the production marker but no token, exactly like a
    hand-launched gate) must survive the daemon's whole life, and the
    scoped decoy (stamped with THIS fixture hub's token, like a worker a
    crashed predecessor left behind) must die, proving the sweep is armed
    and selective rather than disarmed."""

    def test_startup_sweep_spares_unscoped_and_kills_scoped(self):
        import fleetd as fleetd_mod

        # Both decoys are leaders of their own sessions and match the
        # PRODUCTION marker via their script path -- the exact shape of a
        # real gate in `ps -eo command=`.
        marker_dir = self.tmp / "tools" / "fleet"
        marker_dir.mkdir(parents=True)
        script = marker_dir / "gate.sh"
        script.write_text("#!/bin/bash\nsleep 120\n")
        script.chmod(0o755)

        unscoped = subprocess.Popen(
            [str(script), "staging/decoy", "hand-launched"],
            start_new_session=True,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self._spawned.append(unscoped)
        scoped = subprocess.Popen(
            [str(script), "staging/decoy2", "crashed-predecessor",
             fleetd_mod.fleet_scope_token(self.hub_path)],
            start_new_session=True,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self._spawned.append(scoped)

        host = "sweep-scope-test-host"
        home = self.tmp / "home"
        (home / ".fleetd").mkdir(parents=True)
        env = {
            **os.environ,
            "HOME": str(home),
            "FLEET_HOST": host,
            claim_mod.TTL_ENV: str(TTL_S),
            claim_mod.RENEW_ENV: str(RENEW_S),
        }
        # Production markers MUST be in force -- confinement by marker is
        # exactly what this test refuses to rely on.
        env.pop("FLEET_WORKER_MARKERS", None)
        daemon_log = open(self.tmp / "fleetd-sweep.log", "wb")
        self.addCleanup(daemon_log.close)
        daemon = subprocess.Popen(
            [sys.executable, str(Path(__file__).resolve().parents[1] / "fleetd.py"),
             "--hub", self.hub_path, "--interval", "1",
             "--log-dir", str(self.tmp / "gatelogs")],
            stdout=daemon_log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
            env=env, start_new_session=True,
        )
        self._spawned.append(daemon)

        # The startup sweep runs right after the singleton is held; the
        # scoped decoy's death is the observable that it has happened.
        self.wait_until(lambda: scoped.poll() is not None, timeout=60,
                        what="the scoped, claim-less decoy being swept")
        # Two more reconcile intervals of daemon life, then the assertion
        # that matters: gate-shaped without our token means UNTOUCHABLE.
        time.sleep(2)
        self.assertIsNone(daemon.poll(), "the daemon must still be running")
        self.assertIsNone(
            unscoped.poll(),
            "an unscoped worker-shaped process was killed by a fixture "
            "daemon's sweep -- the 2026-08-20 incident has regressed",
        )
        daemon.send_signal(signal.SIGTERM)
        daemon.wait(timeout=30)
        self.assertIsNone(unscoped.poll(),
                          "the unscoped decoy must outlive the daemon entirely")


if __name__ == "__main__":
    unittest.main()
