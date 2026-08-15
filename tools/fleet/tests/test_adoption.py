#!/usr/bin/env python3
"""Worker adoption across a fleetd restart (ARCH-FIX-SPEC.md R6).

Instrument: plain `unittest` against a throwaway fixture hub under
`tempfile.gettempdir()`. The "gate" is a stub shell script that parks until
a stop-file appears and then writes a real verdict through `verdict.py`, so
the completion path an adopted worker takes is the production one and not a
test-only shortcut.

`TestRestartAdoption` spawns REAL `fleetd` processes and SIGKILLs one of
them. That is the only instrument that measures the thing R6 is about: the
defect is a daemon starting with `workers = []`, and every in-process test
of adoption starts by handing the function a list -- which is the state the
broken version could not produce. A same-process test would have passed
against the unfixed daemon.

## What "continuously live" can and cannot mean here, measured honestly

The handover is polled by a background thread (`ClaimWatcher`) asserting
the claim REF never disappears and its ownership token `(holder_host,
started_at)` never changes. Those are the properties that actually exclude
a double gate: a vanished ref means somebody could create a fresh claim, and
a changed token means somebody did.

What is NOT asserted -- because it is not true, and saying so is the point
-- is that the lease is never *expired* during the gap. fleetd B cannot
start until it can reap host A's singleton claim, and the singleton expires
on the same `LEASE_TTL` as the gate claim, so the gap between "A dies" and
"B adopts" is at minimum a full TTL, during which the gate claim is present
but past `expires_at`. Adoption bounds that window, it does not close it;
closing it needs a shorter TTL for the host singleton than for work claims,
which is T1's lease-protocol territory and is written up in the T6 report
rather than smuggled in here.
"""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import claim as claim_mod  # noqa: E402
import fleetd  # noqa: E402
from claim import Claim  # noqa: E402
from fleetlib import Hub  # noqa: E402

TIP_REF = "refs/heads/refactor/tag-machinery"
GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}
HOST = "adoptionhost"

# Compressed lease timings (claim.py's FLEET_TEST_TTL_S / FLEET_TEST_RENEW_S).
# Long enough that a renewal really happens during the handover, short
# enough that waiting out the singleton is seconds rather than minutes.
TEST_TTL = "8"
TEST_RENEW = "1"


def build_hub(tmp: Path) -> Path:
    assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
    bare = tmp / "hub.git"
    work = tmp / "seed"
    env = {**os.environ, **GIT_ENV}
    subprocess.run(["git", "init", "-q", "--bare", str(bare)], check=True)
    subprocess.run(["git", "init", "-q", str(work)], check=True)
    (work / "f.txt").write_text("tip\n")
    subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "tip"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "push", "-q", str(bare), f"HEAD:{TIP_REF}"],
                   check=True, env=env)
    (work / "g.txt").write_text("branch\n")
    subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "work"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "push", "-q", str(bare),
                    "HEAD:refs/heads/staging/one"], check=True, env=env)
    return bare


class ClaimWatcher:
    """Samples a claim ref continuously and records every discontinuity.

    Reads the payload only when the sha moves: the ownership token can only
    change if the ref was rewritten, so this covers every token change at a
    fraction of the round trips of reading on every sample.
    """

    def __init__(self, hub: Hub, ref: str, interval: float = 0.2):
        self.hub, self.ref, self.interval = hub, ref, interval
        self.samples = 0
        self.renewals = 0
        self.vanished = 0
        self.tokens: set = set()
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._loop, daemon=True)

    def _loop(self):
        last_sha = None
        while not self._stop.is_set():
            try:
                sha = self.hub.sha(self.ref)
            except Exception:
                time.sleep(self.interval)
                continue
            self.samples += 1
            if sha is None:
                self.vanished += 1
            elif sha != last_sha:
                if last_sha is not None:
                    self.renewals += 1
                try:
                    payload = self.hub.read(self.ref) or {}
                    self.tokens.add((payload.get("holder_host"), payload.get("started_at")))
                except Exception:
                    pass
            last_sha = sha
            self._stop.wait(self.interval)

    def start(self):
        self._thread.start()
        return self

    def stop(self):
        self._stop.set()
        self._thread.join(timeout=10)


# --------------------------------------------------------------------- #
# Claim.adopt -- the lease-protocol half
# --------------------------------------------------------------------- #


class TestClaimAdopt(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.bare = build_hub(self.tmp)
        self.hub = Hub(str(self.bare), workdir=self.tmp / "cache")
        self.held: list = []

    def tearDown(self):
        for c in self.held:
            try:
                c.release()
            except Exception:
                pass
        self.tmpdir.cleanup()

    def _make_claim(self, **kw) -> Claim:
        c = Claim(self.hub, "gate", "staging-one", work_kind="gate",
                  work_key="staging/one", holder_host=HOST, ttl=30,
                  renew_interval=30, **kw)
        c.acquire()
        c.stop_renewer()  # simulate the daemon dying: nothing renews it now
        self.held.append(c)
        return c

    def test_adopt_continues_the_acquisition_rather_than_taking_a_new_one(self):
        """THE R6 lease property: `started_at` comes from the REF.

        `(holder_host, started_at)` is claim.py's ownership token. A fresh
        `started_at` would make our first renewal indistinguishable, to
        every other observer, from a steal.
        """
        original = self._make_claim(pgid=4242, workdir="nc-9")
        before = self.hub.read(original.ref)
        before_sha = self.hub.sha(original.ref)

        adopted = Claim.adopt(self.hub, original.ref, expected_host=HOST)
        self.assertIsNotNone(adopted, "our own live claim must be adoptable")
        self.held.append(adopted)

        after = self.hub.read(original.ref)
        self.assertEqual(after["started_at"], before["started_at"],
                         "the ownership token must survive adoption verbatim")
        self.assertEqual(after["holder_host"], HOST)
        self.assertNotEqual(self.hub.sha(original.ref), before_sha,
                            "adoption must renew immediately (fresh expires_at)")
        self.assertGreater(after["expires_at"], before["expires_at"])
        self.assertTrue(adopted.renewer_running(), "adoption must start the renewer")
        self.assertFalse(adopted.lost)

    def test_adopt_preserves_every_work_describing_field(self):
        """`renew()` rewrites the payload from the object's attributes, so
        a field adoption failed to restore is a field the first renewal
        DESTROYS -- `pgid` above all, since it is how anything finds the
        running gate again."""
        original = self._make_claim(pgid=4242, workdir="nc-9",
                                    gate_version="v9", rustc_id="rr", platform_id="pp")
        adopted = Claim.adopt(self.hub, original.ref, expected_host=HOST)
        self.held.append(adopted)
        self.assertTrue(adopted.renew())

        after = self.hub.read(original.ref)
        self.assertEqual(after["pgid"], 4242, "renewal must not overwrite the gate's pgid")
        self.assertEqual(after["workdir"], "nc-9")
        self.assertEqual(after["work_key"], "staging/one")
        self.assertEqual(after["work_kind"], "gate")
        self.assertEqual(after["gate_version"], "v9")
        self.assertEqual(after["rustc_id"], "rr")
        self.assertEqual(after["platform_id"], "pp")

    def test_adopt_never_deletes_and_recreates_the_ref(self):
        """`acquire()` would have to (its CAS is `create`), and the gap
        between the delete and the create is a window in which another host
        sees the branch as unclaimed."""
        original = self._make_claim(pgid=4242)
        watcher = ClaimWatcher(Hub(str(self.bare), workdir=self.tmp / "watch"),
                               original.ref, interval=0.05).start()
        time.sleep(0.3)
        adopted = Claim.adopt(self.hub, original.ref, expected_host=HOST)
        self.held.append(adopted)
        time.sleep(0.3)
        watcher.stop()

        self.assertGreater(watcher.samples, 3, "watcher must actually have sampled")
        self.assertEqual(watcher.vanished, 0, "the claim ref must never disappear")
        self.assertEqual(len(watcher.tokens), 1,
                         f"the ownership token must never change: {watcher.tokens}")

    def test_adopt_refuses_another_hosts_claim(self):
        original = self._make_claim(pgid=4242)
        self.assertIsNone(
            Claim.adopt(self.hub, original.ref, expected_host="some-other-host"),
            "a claim held by another host is not ours to adopt",
        )
        # ...and refusing must not have touched it.
        self.assertIsNotNone(self.hub.sha(original.ref))
        self.assertEqual(self.hub.read(original.ref)["holder_host"], HOST)

    def test_adopt_refuses_a_missing_or_malformed_ref(self):
        self.assertIsNone(Claim.adopt(self.hub, "refs/fleet/claims/gate/ghost",
                                      expected_host=HOST))
        self.assertIsNone(Claim.adopt(self.hub, "refs/heads/not-a-claim",
                                      expected_host=HOST))

    def test_adopt_refuses_an_unreproducible_ownership_token(self):
        """If we cannot reproduce `started_at`'s exact text, `_owns` would
        reject our own renewals -- refuse rather than hold a lease we are
        structurally unable to keep."""
        ref = claim_mod.claim_ref("gate", "weird")
        self.assertTrue(self.hub.create(ref, {
            "holder_host": HOST, "pid": 1, "pgid": 4242,
            "work_kind": "gate", "work_key": "staging/one",
            "started_at": "2026-08-15 12:00:00",  # not _iso()'s output
            "expires_at": "2026-08-15T13:00:00+00:00",
        }))
        self.assertIsNone(Claim.adopt(self.hub, ref, expected_host=HOST))

    def _adopt_with_failing_renewal(self, ref, **kw):
        """Adopt while every `Hub.update` raises, so the renewal deadline
        anchor stays observable -- a successful renewal would move it to
        `now` and hide which anchor was chosen."""
        import fleetlib
        real_update = Hub.update
        Hub.update = lambda *a, **k: (_ for _ in ()).throw(
            fleetlib.HubUnreachableError("simulated blip"))
        try:
            return Claim.adopt(self.hub, ref, expected_host=HOST, **kw)
        finally:
            Hub.update = real_update

    def _set_expiry(self, ref: str, seconds_from_now: float) -> datetime:
        payload = self.hub.read(ref)
        when = datetime.now(timezone.utc) + timedelta(seconds=seconds_from_now)
        payload["expires_at"] = when.isoformat()
        self.assertTrue(self.hub.update(ref, payload, self.hub.sha(ref)))
        return when

    def test_renewal_deadline_is_anchored_on_the_hubs_expires_at(self):
        """Anchoring at adoption time would grant a full fresh TTL of grace
        the hub never agreed to, delaying the `lost` declaration past the
        moment another host may legitimately reap us."""
        original = self._make_claim(pgid=4242)
        # A 30s lease with 20s left: it has already been running 10s without
        # us, and the anchor must say so.
        soon = self._set_expiry(original.ref, 20)

        adopted = self._adopt_with_failing_renewal(original.ref, ttl=30, renew_interval=15)
        self.assertIsNotNone(adopted, "a transient blip must not fail adoption")
        self.held.append(adopted)

        expected_anchor = soon - timedelta(seconds=adopted.ttl)
        drift = abs((adopted._last_renew_ok - expected_anchor).total_seconds())
        self.assertLess(drift, 2.0,
                        f"anchor {adopted._last_renew_ok} should track the hub's "
                        f"expires_at ({soon}) minus ttl {adopted.ttl}, not 'now'")
        self.assertFalse(adopted.lost, "20s of lease left is still saveable")

    def test_adopting_an_unsaveable_lease_refuses(self):
        """The other side of the same anchor: a lease that expires sooner
        than our next renewal attempt cannot be held, and adoption must say
        so rather than return a Claim that is already dead.

        This is the exact case an anchor of `now` would have gotten wrong --
        it would have reported a comfortable full TTL of headroom on a lease
        with three seconds left, and fleetd would have gone on believing it
        owned work another host was free to take.
        """
        original = self._make_claim(pgid=4242)
        self._set_expiry(original.ref, 3)
        adopted = self._adopt_with_failing_renewal(original.ref, ttl=30, renew_interval=15)
        self.assertIsNone(
            adopted,
            "a lease expiring in 3s with a 15s renewal cadence has no renewal "
            "left to save it; adoption must refuse",
        )


# --------------------------------------------------------------------- #
# adopt_workers -- the fleetd half, in process
# --------------------------------------------------------------------- #


class TestAdoptWorkers(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.bare = build_hub(self.tmp)
        self.hub = Hub(str(self.bare), workdir=self.tmp / "cache")
        self.workers: list = []
        self.procs: list = []
        self.marker = f"adoption-stub-{os.getpid()}"
        # Short SIGTERM->SIGKILL grace: these stubs are `sleep`, and the
        # production 10s default is 10s of test runtime per sweep.
        self._grace = fleetd.KILL_GRACE_S
        fleetd.KILL_GRACE_S = 2.0

    def tearDown(self):
        fleetd.KILL_GRACE_S = self._grace
        for w in self.workers:
            try:
                w.claim.release()
            except Exception:
                pass
        for p in self.procs:
            try:
                os.killpg(p.pid, signal.SIGKILL)
            except OSError:
                pass
            p.wait(timeout=10)
        self.tmpdir.cleanup()

    def spawn_stub_worker(self) -> subprocess.Popen:
        """A parked process in its own session, whose command line carries
        `self.marker` so the orphan sweep can be pointed at it and nothing
        else on the machine."""
        script = self.tmp / f"{self.marker}.sh"
        script.write_text("#!/bin/bash\nsleep 120\n")
        script.chmod(0o755)
        p = subprocess.Popen([str(script)], start_new_session=True,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.procs.append(p)
        deadline = time.time() + 10
        while time.time() < deadline and p.pid not in fleetd.live_pgids():
            time.sleep(0.1)
        return p

    def make_claim_on_hub(self, kind: str, key: str, *, host: str, pgid: int,
                          work_key: str = "staging/one") -> str:
        ref = claim_mod.claim_ref(kind, key)
        now = datetime.now(timezone.utc)
        self.assertTrue(self.hub.create(ref, {
            "holder_host": host, "pid": pgid, "pgid": pgid,
            "work_kind": kind, "work_key": work_key,
            "started_at": now.isoformat(),
            "expires_at": (now + timedelta(seconds=300)).isoformat(),
            "gate_version": "4", "rustc_id": "r", "platform_id": "p",
        }))
        return ref

    def adopt(self, killer=None, markers=None):
        killed: list = []

        def _fake_kill(pgid, **kw):
            killed.append(pgid)
            return "fake-killed"

        res = fleetd.adopt_workers(
            self.hub, HOST, self.workers,
            killer=killer or _fake_kill,
            markers=markers if markers is not None else [self.marker],
        )
        return res, killed

    def test_live_claim_of_ours_is_adopted(self):
        p = self.spawn_stub_worker()
        ref = self.make_claim_on_hub("gate", "staging-one", host=HOST, pgid=p.pid)

        res, killed = self.adopt()

        self.assertEqual(len(self.workers), 1, f"expected an adopted worker: {res}")
        w = self.workers[0]
        self.assertEqual(w.pgid, p.pid)
        self.assertEqual(w.branch, "staging/one")
        self.assertEqual(w.kind, "gate")
        self.assertIsNone(w.popen, "an adopted worker is not our child")
        self.assertTrue(w.alive(), "the adopted worker must read as alive")
        self.assertTrue(w.claim.renewer_running(), "adoption must resume renewal")
        self.assertIsNotNone(self.hub.sha(ref), "the claim must still be held")
        self.assertEqual(killed, [], "an adopted worker must never be swept")
        self.assertEqual([(k, key) for k, key, _ in res.adopted], [("gate", "staging-one")])

    def test_dead_claim_of_ours_is_released(self):
        p = self.spawn_stub_worker()
        pgid = p.pid
        os.killpg(pgid, signal.SIGKILL)
        p.wait(timeout=10)
        deadline = time.time() + 10
        while time.time() < deadline and pgid in fleetd.live_pgids():
            time.sleep(0.1)
        ref = self.make_claim_on_hub("gate", "staging-one", host=HOST, pgid=pgid)

        res, _killed = self.adopt()

        self.assertEqual(self.workers, [], "a dead worker must not be adopted")
        self.assertIsNone(self.hub.sha(ref),
                          "a claim whose process is gone must be released, "
                          "not left to block the branch for a full TTL")
        self.assertIn(ref, [r for r, _ in res.released])

    def test_another_hosts_claim_is_left_completely_alone(self):
        """R6's third case. Not adopted, not released, and -- because the
        sweep excludes every claimed pgid regardless of holder -- its
        process is not killed either."""
        p = self.spawn_stub_worker()
        ref = self.make_claim_on_hub("gate", "staging-other", host="a-different-host",
                                     pgid=p.pid)

        res, killed = self.adopt()

        self.assertEqual(self.workers, [], "another host's work is not ours to run")
        self.assertIsNotNone(self.hub.sha(ref), "another host's claim must survive")
        self.assertEqual(self.hub.read(ref)["holder_host"], "a-different-host")
        self.assertEqual(killed, [], "another host's worker must not be killed")
        self.assertTrue(any("a-different-host" in reason for _ref, reason in res.skipped),
                        f"the skip must be recorded and explained: {res.skipped}")
        self.assertTrue(p.poll() is None, "the other host's process must still be running")

    def test_unclaimed_fleet_worker_is_killed_by_group(self):
        """A fleet worker running with no lease at all is the hazard leases
        exist to prevent: nothing stops another host starting the same
        branch beside it."""
        p = self.spawn_stub_worker()
        res, killed = self.adopt()

        self.assertEqual(killed, [p.pid], f"the orphan must be swept: {res}")
        self.assertEqual([pg for pg, _ in res.orphans_killed], [p.pid])

    def test_orphan_sweep_really_kills_the_group(self):
        """The fake killer above proves the decision; this proves the deed,
        using the real `kill_process_group`."""
        p = self.spawn_stub_worker()
        fleetd.adopt_workers(self.hub, HOST, self.workers,
                             markers=[self.marker])
        deadline = time.time() + 20
        while time.time() < deadline and p.poll() is None:
            time.sleep(0.2)
        self.assertIsNotNone(p.poll(), "the orphan process group must actually be gone")

    def test_fleetds_own_process_group_is_never_swept(self):
        """A fleetd that SIGKILLs its own group takes out every gate on the
        host. Driven with an injected probe rather than a broad `ps` marker:
        the property is exact, and a test that sweeps real process groups to
        prove it is a bad trade."""
        try:
            own = os.getpgrp()
        except (AttributeError, OSError):
            self.skipTest("no process groups on this platform")
        killed: list = []
        res = fleetd.adopt_workers(
            self.hub, HOST, self.workers,
            worker_probe=lambda _m: {own: "fleetd itself", 424242: "a real orphan"},
            killer=lambda pgid, **kw: killed.append(pgid) or "fake",
            markers=[self.marker],
        )
        self.assertNotIn(own, killed, "fleetd must never sweep its own group")
        self.assertEqual(res.orphans_killed, [(424242, "fake")],
                         "everything except our own group is still swept")

    def test_claim_without_a_usable_pgid_is_released(self):
        ref = claim_mod.claim_ref("agent", "staging-one")
        now = datetime.now(timezone.utc)
        self.assertTrue(self.hub.create(ref, {
            "holder_host": HOST, "work_kind": "agent", "work_key": "staging/one",
            "started_at": now.isoformat(),
            "expires_at": (now + timedelta(seconds=300)).isoformat(),
        }))
        res, _killed = self.adopt()
        self.assertIsNone(self.hub.sha(ref))
        self.assertIn(ref, [r for r, _ in res.released])

    def test_the_host_singleton_is_not_treated_as_a_worker(self):
        """`adopt_workers` lists only gate/agent kinds. Adopting or
        releasing `refs/fleet/claims/host/<host>` would fight the singleton
        guard that just granted us the right to run."""
        p = self.spawn_stub_worker()
        ref = self.make_claim_on_hub("host", HOST, host=HOST, pgid=p.pid,
                                     work_key=HOST)
        res, _killed = self.adopt()
        self.assertIsNotNone(self.hub.sha(ref), "the singleton must be untouched")
        self.assertEqual(self.workers, [])
        self.assertNotIn(ref, [r for r, _ in res.released])


# --------------------------------------------------------------------- #
# The real thing: kill fleetd A, start fleetd B
# --------------------------------------------------------------------- #


class TestRestartAdoption(unittest.TestCase):
    """Fixture fleetd processes, SIGKILLed without cleanup, per R6's test."""

    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.bare = build_hub(self.tmp)
        self.hub = Hub(str(self.bare), workdir=self.tmp / "cache")
        self.daemons: list = []
        # The marker must appear in the worker's COMMAND LINE, not merely
        # in its source: `fleet_worker_pgids` matches `ps -eo command=`.
        # Putting it in a comment inside the stub matched nothing and made
        # the orphan sweep a silent no-op -- so it goes in the repo-root
        # path, which `default_gate_command` puts into argv[0].
        self.marker = f"adoption-gate-{os.getpid()}"
        self.repo_root = self.tmp / self.marker
        (self.repo_root / "tools" / "fleet").mkdir(parents=True)
        (self.repo_root / "tools" / "fleet" / "gate_version.txt").write_text("4\n")
        self.stub = self.repo_root / "tools" / "fleet" / "gate.sh"
        self.stub.write_text(self._stub_source())
        self.stub.chmod(0o755)
        # The marker fleetd sweeps on is the GATE'S PATH, mirroring
        # production's `WORKER_MARKERS = ("tools/fleet/gate.sh", ...)`.
        # A looser marker (the repo-root directory alone) also matches
        # fleetd's own `--repo-root` argument, i.e. the daemon would report
        # ITSELF as a fleet worker. The own-pgid guard stops that becoming
        # a suicide, but a matcher that needs the guard to be safe is the
        # wrong matcher.
        self.gate_marker = str(self.stub)
        self.set_desired(gates=1)

    def gate_pgids(self) -> dict:
        return fleetd.fleet_worker_pgids([self.gate_marker])

    def _stub_source(self) -> str:
        """A parked 'gate' that writes a real verdict when told to finish.

        The verdict write is the production path (`verdict.py store`), so
        the last assertion in the adoption test is measuring the real memo
        mechanism rather than a flag the stub sets for us.
        """
        return f"""#!/bin/bash
# {self.marker}
echo "$$" > {self.tmp}/gate.pid
STOP={self.tmp}/stop-gate
while [ ! -f "$STOP" ]; do sleep 0.2; done
cat > {self.tmp}/verdict.json <<'EOF'
{{"tree_sha":"0000000000000000000000000000000000000042","base_tip":"deadbeef",
 "branch":"staging/one","result":"PASS","stage":"all","gate_version":"4",
 "rustc_id":"r","platform_id":"p","host":"{HOST}","duration_s":1,"write_set":[]}}
EOF
{sys.executable} {FLEET_DIR}/verdict.py store --hub-url {self.bare} \\
    --workdir {self.tmp}/gatecache --json-file {self.tmp}/verdict.json
exit 0
"""

    def tearDown(self):
        (self.tmp / "stop-gate").write_text("")
        for p in self.daemons:
            try:
                p.send_signal(signal.SIGKILL)
            except OSError:
                pass
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                pass
        gate_pid = self.tmp / "gate.pid"
        if gate_pid.exists():
            try:
                os.killpg(int(gate_pid.read_text().strip()), signal.SIGKILL)
            except (OSError, ValueError):
                pass
        self.tmpdir.cleanup()

    def set_desired(self, gates: int):
        doc = {"generation": 1,
               "hosts": {HOST: {"gates": gates, "agents": 0, "enabled": True}},
               "limits": {}}
        cur = self.hub.sha(fleetd.DESIRED_REF)
        ok = (self.hub.create(fleetd.DESIRED_REF, doc) if cur is None
              else self.hub.update(fleetd.DESIRED_REF, doc, cur))
        self.assertTrue(ok)

    def start_fleetd(self, name: str) -> subprocess.Popen:
        log = open(self.tmp / f"fleetd-{name}.log", "wb")
        p = subprocess.Popen(
            [sys.executable, str(FLEET_DIR / "fleetd.py"),
             "--hub", str(self.bare), "--repo-root", str(self.repo_root),
             "--log-dir", str(self.tmp / "logs"), "--interval", "1"],
            stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
            start_new_session=True,
            env={**os.environ,
                 "FLEET_HOST": HOST,
                 "FLEET_TEST_TTL_S": TEST_TTL,
                 "FLEET_TEST_RENEW_S": TEST_RENEW,
                 "FLEET_WORKER_MARKERS": self.gate_marker,
                 "FLEET_KILL_GRACE_S": "2"},
        )
        self.daemons.append(p)
        return p

    def log_of(self, name: str) -> str:
        path = self.tmp / f"fleetd-{name}.log"
        return path.read_text(errors="replace") if path.exists() else ""

    def wait_for(self, predicate, timeout: float, what: str):
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                if predicate():
                    return True
            except Exception:
                pass
            time.sleep(0.25)
        self.fail(f"timed out after {timeout}s waiting for {what}")

    def gate_claim_ref(self) -> str:
        claims = self.hub.list("refs/fleet/claims/gate/")
        return next(iter(claims), None)

    def singleton_expired(self) -> bool:
        payload = self.hub.read(claim_mod.claim_ref("host", HOST))
        return payload is None or claim_mod.is_expired(payload)

    # -- the tests ---------------------------------------------------- #

    def test_b_adopts_the_gate_a_left_running(self):
        a = self.start_fleetd("a")
        self.wait_for(lambda: self.gate_claim_ref() is not None, 60,
                      f"fleetd A to claim a gate\n{self.log_of('a')}")
        ref = self.gate_claim_ref()
        self.wait_for(lambda: (self.tmp / "gate.pid").exists(), 30, "the stub gate to start")
        gate_pgid = int((self.tmp / "gate.pid").read_text().strip())

        watcher = ClaimWatcher(Hub(str(self.bare), workdir=self.tmp / "watch"), ref).start()

        a.send_signal(signal.SIGKILL)  # no cleanup, no release, no drain
        a.wait(timeout=10)
        self.assertIn(gate_pgid, fleetd.live_pgids(),
                      "the gate must outlive the daemon that started it")

        # B cannot start until A's host singleton is reapable -- see this
        # module's docstring on what that costs.
        self.wait_for(self.singleton_expired, 60, "A's host singleton to expire")
        self.start_fleetd("b")
        self.wait_for(lambda: "adoption:" in self.log_of("b"), 60,
                      f"fleetd B to report adoption\n{self.log_of('b')}")

        log_b = self.log_of("b")
        self.assertIn(f"gate/staging-one#{gate_pgid}", log_b,
                      f"B must adopt A's gate by pgid:\n{log_b}")

        # The lease is live again and being renewed by B.
        self.wait_for(lambda: not claim_mod.is_expired(self.hub.read(ref) or {}), 30,
                      "B to refresh the adopted lease")
        first = self.hub.sha(ref)
        self.wait_for(lambda: self.hub.sha(ref) != first, 30, "B to renew the adopted lease")

        # No second gate: the whole point.
        self.assertEqual(len(self.hub.list("refs/fleet/claims/gate/")), 1,
                         "adoption must not leave room for a duplicate gate")
        self.assertEqual(len(self.gate_pgids()), 1,
                         "exactly one gate process may exist for this branch")

        # The handover is over and the work is still running, so the
        # continuity window closes HERE -- before the completion phase,
        # whose whole purpose is to make the claim disappear legitimately.
        watcher.stop()
        self.assertGreater(watcher.samples, 20, "the watcher must have polled throughout")
        self.assertEqual(watcher.vanished, 0,
                         "the claim ref must never have disappeared mid-handover")
        self.assertEqual(len(watcher.tokens), 1,
                         f"the ownership token must be constant across the handover: "
                         f"{watcher.tokens}")
        self.assertGreater(watcher.renewals, 0, "the lease must have been renewed")

        # Let the adopted gate finish; B must reap it and release the claim,
        # and the verdict the gate writes on its way out must land.
        (self.tmp / "stop-gate").write_text("")
        self.wait_for(lambda: self.hub.sha(ref) is None, 60,
                      f"B to release the adopted claim on completion\n{self.log_of('b')}")
        self.wait_for(lambda: bool(self.hub.list("refs/fleet/verdicts/")), 30,
                      "the adopted gate's verdict to reach the hub")
        self.assertEqual(self.gate_pgids(), {},
                         "the finished gate's process group must be gone")

    def test_b_does_not_wait_the_full_ttl_when_as_process_group_is_dead(self):
        """ARCH-FIX FIX 2, at process level (seam 4 in test_seams.py is the
        same property under the REAL supervisor, over the full handover).

        `start_fleetd` runs fleetd.py directly with its own new session
        (`start_new_session=True`), so it is its OWN process group's
        leader -- unlike under `fleetd-wrapper.sh`, nothing else shares
        that pgid, and a SIGKILL empties the group immediately. B must be
        able to get past the singleton well under `TEST_TTL` seconds, not
        wait it out the way `test_b_adopts_the_gate_a_left_running` above
        deliberately still does (that test synchronizes on natural expiry
        on purpose, per this module's docstring, to describe the bound
        adoption alone provides; this test is the proof FIX 2 tightens
        that bound for the same-host case instead of leaving it at a full
        TTL).
        """
        a = self.start_fleetd("a")
        self.wait_for(lambda: self.gate_claim_ref() is not None, 60,
                      f"fleetd A to claim a gate\n{self.log_of('a')}")
        self.wait_for(lambda: (self.tmp / "gate.pid").exists(), 30, "the stub gate to start")

        a.send_signal(signal.SIGKILL)  # no cleanup, no release, no drain
        a.wait(timeout=10)

        started_b_at = time.time()
        self.start_fleetd("b")
        self.wait_for(lambda: "adoption:" in self.log_of("b"), 60,
                      f"fleetd B to report adoption\n{self.log_of('b')}")
        elapsed = time.time() - started_b_at

        self.assertLess(
            elapsed, float(TEST_TTL),
            f"B took {elapsed:.1f}s to get past the host singleton -- that is at "
            f"least the full LEASE_TTL ({TEST_TTL}s), i.e. FIX 2's reap-before-expiry "
            f"path did not fire and B fell back to waiting out the lease:\n"
            f"{self.log_of('b')}",
        )
        self.assertNotIn(
            f"another instance holds refs/fleet/claims/host/{HOST}", self.log_of("b"),
            f"B logged a startup refusal before it went on to adopt:\n{self.log_of('b')}",
        )

    def test_b_releases_the_claim_when_the_worker_died_too(self):
        """A killed, its gate killed too: B must free the slot rather than
        leave the branch blocked for a full lease."""
        a = self.start_fleetd("a")
        self.wait_for(lambda: self.gate_claim_ref() is not None, 60,
                      f"fleetd A to claim a gate\n{self.log_of('a')}")
        ref = self.gate_claim_ref()
        self.wait_for(lambda: (self.tmp / "gate.pid").exists(), 30, "the stub gate to start")
        gate_pgid = int((self.tmp / "gate.pid").read_text().strip())

        a.send_signal(signal.SIGKILL)
        a.wait(timeout=10)
        os.killpg(gate_pgid, signal.SIGKILL)
        self.wait_for(lambda: gate_pgid not in fleetd.live_pgids(), 30, "the gate to die")

        self.wait_for(self.singleton_expired, 60, "A's host singleton to expire")
        self.start_fleetd("b")
        self.wait_for(lambda: "adoption:" in self.log_of("b"), 60,
                      f"fleetd B to report adoption\n{self.log_of('b')}")

        log_b = self.log_of("b")
        self.assertIn("RELEASED orphaned claim", log_b, log_b)
        self.assertNotIn("adopted=[]", log_b.split("adoption:")[1].split("\n")[0].replace(
            "adopted=[]", "ADOPTED-NONE"), "sanity: parsing the summary line")

        # The slot is free: B claims the branch afresh and runs its own gate.
        self.wait_for(lambda: self.gate_claim_ref() is not None
                      and self.hub.read(self.gate_claim_ref()).get("pgid") != gate_pgid,
                      60, f"B to start its own gate on the freed slot\n{self.log_of('b')}")
        self.assertEqual(len(self.hub.list("refs/fleet/claims/gate/")), 1)

    def test_b_does_not_adopt_another_hosts_claim(self):
        """Third R6 case, at process level: a claim for a DIFFERENT host on
        the same fixture hub is invisible to this host's adoption."""
        other_ref = claim_mod.claim_ref("gate", "staging-elsewhere")
        now = datetime.now(timezone.utc)
        self.assertTrue(self.hub.create(other_ref, {
            "holder_host": "some-other-host", "pid": os.getpid(), "pgid": os.getpgrp(),
            "work_kind": "gate", "work_key": "staging/elsewhere",
            "started_at": now.isoformat(),
            "expires_at": (now + timedelta(seconds=600)).isoformat(),
        }))

        self.start_fleetd("b")
        self.wait_for(lambda: "adoption:" in self.log_of("b"), 60,
                      f"fleetd B to report adoption\n{self.log_of('b')}")

        log_b = self.log_of("b")
        self.assertIn("adopted=[]", log_b, f"nothing of ours to adopt:\n{log_b}")
        self.assertIsNotNone(self.hub.sha(other_ref),
                             "another host's claim must not be released")
        self.assertEqual(self.hub.read(other_ref)["holder_host"], "some-other-host")
        self.assertNotIn("staging-elsewhere", log_b.split("adoption:")[1].split("\n")[0])


if __name__ == "__main__":
    unittest.main()
