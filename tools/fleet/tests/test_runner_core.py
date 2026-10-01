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

import fcntl
import hashlib
import os
import signal as signal_mod
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from _env import HermeticCase, scrub_env  # noqa: E402
from _fixtures import make_hub  # noqa: E402

import claim as claim_mod  # noqa: E402
import fleetd  # noqa: E402
import keel.runner as runner  # noqa: E402
from keel import journal as journal_mod  # noqa: E402
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
        self.addCleanup(self.tmpdir.cleanup)
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
            try:
                w.claim.release()
            except HubError:
                pass

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


class TestJournalWiring(RunnerFixture):
    def test_open_run_refuses_a_second_offer_without_erasing_first(self):
        j = journal_mod.Journal(self.tmp / "journal")
        w = runner.start_gate(self.hub, "staging/one", "first", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.workers.append(w)
        before = j.read_job(w.job_key)
        with self.assertRaisesRegex(journal_mod.JournalError, "previous journal run"):
            runner.start_gate(self.hub, "staging/one", "second", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.assertEqual(j.read_job(w.job_key), before)
        self.assertIsNotNone(self.hub.sha(w.claim.ref))

    def test_torn_first_record_refuses_new_offer(self):
        j = journal_mod.Journal(self.tmp / "journal")
        path = j.path_for("gate-staging-one")
        path.parent.mkdir(parents=True)
        path.write_bytes(b'{"v":2,"event":"offer"')
        with self.assertRaisesRegex(journal_mod.JournalError, "torn journal"):
            runner.start_gate(self.hub, "staging/one", "blocked", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.assertIsNone(self.hub.sha(claim_mod.claim_ref("gate", "staging-one")))
        self.assertEqual(path.read_bytes(), b'{"v":2,"event":"offer"')

    def test_default_journal_stays_inside_fixture_home(self):
        j = journal_mod.Journal()
        self.assertEqual(j.root, Path(os.environ["KEEL_HOME"]) / "journal")
        self.assertEqual(scrub_env()["KEEL_HOME"], os.environ["KEEL_HOME"])

    def test_gate_records_offer_claim_spawn_and_exit(self):
        j = journal_mod.Journal(self.tmp / "journal")
        w = runner.start_gate(self.hub, "staging/one", "journal-gate", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.assertIsNotNone(w)
        self.workers.append(w)
        self.assertEqual(w.job_key, "gate-staging-one")
        state = j.read_job(w.job_key)
        self.assertEqual(state.events, ("offer", "claim", "spawn"))
        self.assertEqual(state.pgid, w.pgid)
        self.assertEqual(state.started_at, self.hub.read(w.claim.ref)["started_at"])
        (self.tmp / f"stop-{w.tag}").write_text("")
        w.popen.wait(timeout=10)
        self.set_desired(gates=0)
        workers = [w]
        fleetd.reconcile_once(self.hub, self.host, workers, [str(self.stub)],
                              self.log_dir, REPO_ROOT, disk_probe=lambda: 100,
                              mem_probe=lambda: 100, journal=j)
        self.assertEqual(workers, [])
        self.assertTrue(j.read_job(w.job_key).closed)

    def test_failed_journal_offer_never_acquires_or_spawns(self):
        j = journal_mod.Journal(self.tmp / "journal")
        with mock.patch.object(j, "offer", side_effect=journal_mod.JournalWriteError("full")):
            with self.assertRaises(journal_mod.JournalWriteError):
                runner.start_gate(self.hub, "staging/one", "failed-offer", [str(self.stub)],
                                  self.host, self.log_dir, journal=j)
        self.assertIsNone(self.hub.sha(claim_mod.claim_ref("gate", "staging-one")))

    def test_offline_daemon_adopts_only_the_open_verified_run(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        j = journal_mod.Journal()
        w = runner.start_gate(self.hub, "staging/one", "first", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.workers.append(w)
        w.claim.stop_renewer(timeout=2)
        captured = []

        def scripted(_hub, _host, workers, *_args, **kw):
            captured.extend(workers)
            self.assertFalse(kw["spawn_allowed"])
            self.assertEqual([item.pgid for item in workers], [w.pgid])
            self.assertEqual(workers[0].job_key, w.job_key)
            return fleetd.ReconcileResult()

        with mock.patch.object(fleetd, "adopt_workers",
                               side_effect=HubUnreachableError("offline")):
            rc = runner.run_daemon(self.hub, self.host, gate_command=[str(self.stub)],
                                   log_dir=self.log_dir, repo_root=REPO_ROOT,
                                   once=True, reconcile=scripted)
        self.assertEqual(rc, 0)
        self.assertEqual(len(captured), 1)
        captured[0].claim.stop_renewer(timeout=2)
        self.assertTrue(j.read_job(w.job_key).open)

    def test_offline_dead_run_replays_owed_release_after_store_answers(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        j = journal_mod.Journal()
        w = runner.start_gate(self.hub, "staging/one", "dead", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.workers.append(w)
        w.claim.stop_renewer(timeout=2)
        os.killpg(w.pgid, signal_mod.SIGKILL)
        w.popen.wait(timeout=10)
        self.assertIsNotNone(self.hub.sha(w.claim.ref))

        def scripted(_hub, _host, workers, *_args, **kw):
            self.assertEqual(workers, [])
            self.assertFalse(kw["spawn_allowed"])
            return fleetd.ReconcileResult()

        with mock.patch.object(fleetd, "adopt_workers",
                               side_effect=HubUnreachableError("offline")):
            rc = runner.run_daemon(self.hub, self.host, gate_command=[str(self.stub)],
                                   log_dir=self.log_dir, repo_root=REPO_ROOT,
                                   once=True, reconcile=scripted)
        self.assertEqual(rc, 0)
        self.assertIsNone(self.hub.sha(w.claim.ref))
        self.assertTrue(j.read_job(w.job_key).closed)

    def test_old_journal_token_cannot_release_same_hosts_new_claim(self):
        j = journal_mod.Journal()
        w = runner.start_gate(self.hub, "staging/one", "old", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.workers.append(w)
        os.killpg(w.pgid, signal_mod.SIGKILL)
        w.popen.wait(timeout=10)
        old_token = j.read_job(w.job_key).started_at
        w.claim.release()
        replacement = claim_mod.Claim(self.hub, kind="gate", key="staging-one",
                                      work_kind="gate", work_key="staging/one",
                                      holder_host=self.host)
        replacement.acquire()
        try:
            self.assertNotEqual(self.hub.read(replacement.ref)["started_at"], old_token)

            def scripted(_hub, _host, workers, *_args, **kw):
                self.assertFalse(kw["spawn_allowed"])
                self.assertEqual(workers, [])
                return fleetd.ReconcileResult()

            with mock.patch.object(fleetd, "adopt_workers",
                                   side_effect=HubUnreachableError("offline")):
                self.assertEqual(runner.run_daemon(
                    self.hub, self.host, gate_command=[str(self.stub)], log_dir=self.log_dir,
                    repo_root=REPO_ROOT, once=True, reconcile=scripted), 0)
            self.assertIsNotNone(self.hub.sha(replacement.ref))
            self.assertEqual(self.hub.read(replacement.ref)["started_at"],
                             claim_mod._iso(replacement._started_at))
        finally:
            replacement.release()

    def test_reconcile_closes_superseded_run_without_deleting_new_claim(self):
        journal = journal_mod.Journal()
        worker = runner.start_gate(self.hub, "staging/one", "old", [str(self.stub)],
                                   self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        (self.tmp / f"stop-{worker.tag}").write_text("")
        worker.popen.wait(timeout=10)
        worker.claim.release()
        replacement = claim_mod.Claim(
            self.hub, kind="gate", key="staging-one", work_kind="gate",
            work_key="staging/one", holder_host=self.host)
        replacement.acquire()
        try:
            new_token = self.hub.read(replacement.ref)["started_at"]
            self.assertNotEqual(new_token, journal.read_job(worker.job_key).started_at)
            runner.reconcile_journal_runs(journal, self.hub, self.host, [])
            self.assertTrue(journal.read_job(worker.job_key).closed)
            self.assertEqual(self.hub.read(replacement.ref)["started_at"], new_token)
        finally:
            replacement.release()

    def test_recycled_pgid_does_not_keep_dead_journal_run_open(self):
        journal = journal_mod.Journal()
        worker = runner.start_gate(self.hub, "staging/one", "old", [str(self.stub)],
                                   self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        (self.tmp / f"stop-{worker.tag}").write_text("")
        worker.popen.wait(timeout=10)
        worker.claim.release()
        with mock.patch.object(runner.os, "killpg", return_value=None), \
                mock.patch.object(runner, "_journal_group_identity", return_value="other"):
            runner.reconcile_journal_runs(journal, self.hub, self.host, [])
        self.assertTrue(journal.read_job(worker.job_key).closed)

    def test_expired_exact_claim_allows_positive_recycled_identity(self):
        journal = journal_mod.Journal()
        worker = runner.start_gate(self.hub, "staging/one", "old", [str(self.stub)],
                                   self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        worker.claim.stop_renewer(timeout=2)
        (self.tmp / f"stop-{worker.tag}").write_text("")
        worker.popen.wait(timeout=10)
        payload = self.hub.read(worker.claim.ref)
        payload["expires_at"] = "2020-01-01T00:00:00+00:00"
        self.assertTrue(self.hub.update(worker.claim.ref, payload,
                                        expect_sha=self.hub.sha(worker.claim.ref)))
        with mock.patch.object(runner.os, "killpg", return_value=None), \
                mock.patch.object(runner, "_journal_group_identity", return_value="other") as identity:
            runner.reconcile_journal_runs(journal, self.hub, self.host, [])
        identity.assert_called_once()
        self.assertTrue(journal.read_job(worker.job_key).closed)
        self.assertIsNone(self.hub.sha(worker.claim.ref))

    def test_missing_identity_rows_keep_kernel_live_old_run_open(self):
        journal = journal_mod.Journal()
        worker = runner.start_gate(self.hub, "staging/one", "old", [str(self.stub)],
                                   self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        (self.tmp / f"stop-{worker.tag}").write_text("")
        worker.popen.wait(timeout=10)
        worker.claim.release()
        with mock.patch.object(runner.os, "killpg", return_value=None), \
                mock.patch.object(runner, "_journal_group_identity", return_value="missing"):
            runner.reconcile_journal_runs(journal, self.hub, self.host, [])
        self.assertTrue(journal.read_job(worker.job_key).open)
        with mock.patch.object(runner.os, "killpg", return_value=None), \
                mock.patch.object(runner, "_journal_group_identity", return_value="other"):
            runner.reconcile_journal_runs(journal, self.hub, self.host, [])
        self.assertTrue(journal.read_job(worker.job_key).closed)

    def test_ambiguous_create_preserves_exact_attempted_token(self):
        claim = claim_mod.Claim(
            self.hub, kind="host", key=self.host, work_kind="fleetd",
            work_key=self.host, holder_host=self.host)
        real_create = self.hub.create

        def accepted_without_reply(ref, payload):
            self.assertTrue(real_create(ref, payload))
            raise HubUnreachableError("response lost")

        with mock.patch.object(self.hub, "create", side_effect=accepted_without_reply):
            with self.assertRaises(HubUnreachableError):
                claim.acquire()
        payload = self.hub.read(claim.ref)
        self.assertEqual(payload["started_at"], claim_mod._iso(claim._started_at))
        self.hub.delete(claim.ref, expect_sha=self.hub.sha(claim.ref))

    def test_ambiguous_host_recovery_rejects_pid_time_lookalike(self):
        actual = claim_mod.Claim(
            self.hub, kind="host", key=self.host, work_kind="fleetd",
            work_key=self.host, holder_host=self.host)
        actual.acquire()
        self.addCleanup(actual.release)
        lookalike = claim_mod.Claim(
            self.hub, kind="host", key=self.host, work_kind="fleetd",
            work_key=self.host, holder_host=self.host)
        lookalike._started_at = claim_mod._utcnow()
        with mock.patch.object(self.hub, "update", wraps=self.hub.update) as update:
            matched, adopted = runner._recover_ambiguous_host_claim(
                self.hub, self.host, lookalike)
        self.assertFalse(matched)
        self.assertIsNone(adopted)
        update.assert_not_called()

    def test_ambiguous_host_claim_rejects_replaced_token_before_renewal(self):
        host_claim = claim_mod.Claim(
            self.hub, kind="host", key=self.host, work_kind="fleetd",
            work_key=self.host, holder_host=self.host)
        host_claim.acquire()
        self.addCleanup(host_claim.release)
        first = self.hub.read(host_claim.ref)
        second = dict(first, started_at="2020-01-01T00:00:00+00:00")
        with mock.patch.object(self.hub, "read", side_effect=[first, second]) as reads, \
                mock.patch.object(self.hub, "update", wraps=self.hub.update) as update:
            matched, adopted = runner._recover_ambiguous_host_claim(
                self.hub, self.host, host_claim)
        self.assertTrue(matched)
        self.assertIsNone(adopted)
        self.assertEqual(reads.call_count, 2)
        update.assert_not_called()

    def test_ambiguous_host_claim_rejects_replacement_after_sha_read(self):
        host_claim = claim_mod.Claim(
            self.hub, kind="host", key=self.host, work_kind="fleetd",
            work_key=self.host, holder_host=self.host)
        host_claim.acquire()
        self.addCleanup(host_claim.release)
        first = self.hub.read(host_claim.ref)
        replacement = dict(first, started_at="2020-01-01T00:00:00+00:00")
        with mock.patch.object(self.hub, "read",
                               side_effect=[first, first, replacement]) as reads, \
                mock.patch.object(self.hub, "sha", return_value="f" * 40), \
                mock.patch.object(self.hub, "update", wraps=self.hub.update) as update:
            matched, adopted = runner._recover_ambiguous_host_claim(
                self.hub, self.host, host_claim)
        self.assertTrue(matched)
        self.assertIsNone(adopted)
        self.assertEqual(reads.call_count, 3)
        update.assert_not_called()

    def test_false_offline_listing_does_not_queue_live_claim_release(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        journal = journal_mod.Journal()
        worker = runner.start_gate(self.hub, "staging/one", "live", [str(self.stub)],
                                   self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        worker.claim.stop_renewer(timeout=2)
        rebuilt = []
        result = journal_mod.adopt_from_journal(
            journal, self.host, rebuilt, hub=self.hub,
            pgid_probe=lambda: set())
        self.assertEqual(result.to_release, [])
        self.assertEqual([(w.job_key, w.pgid) for w in rebuilt],
                         [(worker.job_key, worker.pgid)])
        self.assertEqual(journal_mod.release_pending(
            self.hub, self.host, result, journal=journal), [])
        self.assertIsNotNone(self.hub.sha(worker.claim.ref))
        for adopted in rebuilt:
            adopted.claim.stop_renewer(timeout=2)

    def test_false_process_listing_and_identity_listing_cannot_release_live_group(self):
        journal = journal_mod.Journal()
        worker = runner.start_gate(self.hub, "staging/one", "live", [str(self.stub)],
                                   self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        with mock.patch.object(runner, "_journal_group_identity", return_value="missing"):
            result = journal_mod.adopt_from_journal(
                journal, self.host, [], hub=self.hub, pgid_probe=lambda: set(),
                identity_probe=lambda *_args: None)
        self.assertEqual(result.to_release, [])
        self.assertIsNotNone(self.hub.sha(worker.claim.ref))

    def test_failed_local_signal_retains_lost_worker_until_retry(self):
        journal = journal_mod.Journal()
        worker = runner.start_gate(self.hub, "staging/one", "live", [str(self.stub)],
                                   self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        worker.claim._mark_lost("lease gone")
        real_killpg = os.killpg

        def signal_denied(pgid, sig):
            if sig == 0:
                return real_killpg(pgid, sig)
            raise PermissionError("injected signal refusal")

        with mock.patch.object(runner.os, "killpg", side_effect=signal_denied):
            self.assertEqual(runner.stop_lost_workers(self.workers, journal, self.host), [])
        self.assertIn(worker, self.workers)
        self.assertTrue(journal.read_job(worker.job_key).open)
        self.assertIsNotNone(self.hub.sha(worker.claim.ref))
        self.assertIsNone(worker.popen.poll())
        stopped = runner.stop_lost_workers(self.workers, journal, self.host)
        self.assertEqual([tag for tag, _reason in stopped], [worker.tag])
        self.assertTrue(journal.read_job(worker.job_key).closed)

    def test_log_open_failure_releases_claim_and_closes_offer(self):
        journal = journal_mod.Journal()
        real_open = open

        def fail_log(path, *args, **kwargs):
            if str(path).endswith(".launch.log"):
                raise PermissionError("log directory not writable")
            return real_open(path, *args, **kwargs)

        with mock.patch("builtins.open", side_effect=fail_log):
            with self.assertRaises(PermissionError):
                runner.start_gate(self.hub, "staging/one", "no-log",
                                  [str(self.stub)], self.host, self.log_dir,
                                  journal=journal)
        self.assertIsNone(self.hub.sha(claim_mod.claim_ref("gate", "staging-one")))
        self.assertTrue(journal.read_job("gate-staging-one").closed)

    def test_local_lock_refuses_a_second_runner_before_store_access(self):
        lock_dir = journal_mod.Journal().root.parent / "runner-locks"
        lock_dir.mkdir(parents=True, exist_ok=True)
        path = lock_dir / (hashlib.sha256(self.host.encode()).hexdigest() + ".lock")
        fd = os.open(path, os.O_CREAT | os.O_RDWR, 0o600)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with mock.patch.object(claim_mod.Claim, "acquire_or_reap",
                                   side_effect=AssertionError("store touched")):
                rc = runner.run_daemon(
                    self.hub, self.host, gate_command=[str(self.stub)],
                    log_dir=self.log_dir, repo_root=REPO_ROOT, once=True,
                    reconcile=lambda *_a, **_kw: self.fail("second runner reconciled"))
            self.assertEqual(rc, 3)
        finally:
            os.close(fd)

    def test_store_recovery_takes_host_singleton_before_enabling_starts(self):
        real_acquire = claim_mod.Claim.acquire_or_reap
        real_adopt = fleetd.adopt_workers
        calls = {"singleton": 0, "adopt": 0}
        allowed = []

        def acquire(claim):
            if claim.kind == "host":
                calls["singleton"] += 1
                if calls["singleton"] == 1:
                    raise HubUnreachableError("both routes down")
            return real_acquire(claim)

        def adopt(*args, **kwargs):
            self.assertIsNotNone(self.hub.sha(claim_mod.claim_ref("host", self.host)))
            calls["adopt"] += 1
            if calls["adopt"] == 1:
                raise HubUnreachableError("both routes down")
            return real_adopt(*args, **kwargs)

        def step(hub, host, _workers, *_args, **kw):
            allowed.append(kw["spawn_allowed"])
            if kw["spawn_allowed"]:
                self.assertIsNotNone(hub.sha(claim_mod.claim_ref("host", host)))
                os.kill(os.getpid(), signal_mod.SIGTERM)
            return fleetd.ReconcileResult()

        with mock.patch.object(claim_mod.Claim, "acquire_or_reap", acquire), \
                mock.patch.object(fleetd, "adopt_workers", adopt), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            rc = runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT, interval=0, reconcile=step)
        self.assertEqual(rc, 0)
        self.assertEqual(allowed, [False, False, True])
        self.assertEqual(calls["singleton"], 2)

    def test_offline_host_claim_never_renews_worker_before_ownership(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        journal = journal_mod.Journal()
        worker = runner.start_gate(
            self.hub, "staging/one", "live", [str(self.stub)],
            self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        worker.claim.stop_renewer(timeout=2)
        worker_sha = self.hub.sha(worker.claim.ref)
        foreign = claim_mod.Claim(
            self.hub, kind="host", key=self.host, work_kind="fleetd",
            work_key=self.host, holder_host="different-machine")
        foreign.acquire()
        self.addCleanup(foreign.release)
        real_acquire = claim_mod.Claim.acquire_or_reap
        calls = {"host": 0}

        def offline_then_held(claim):
            if claim.kind == "host":
                calls["host"] += 1
                if calls["host"] == 1:
                    raise HubUnreachableError("response lost")
            return real_acquire(claim)

        with mock.patch.object(claim_mod.Claim, "acquire_or_reap", offline_then_held), \
                mock.patch.object(self.hub, "update", wraps=self.hub.update) as update, \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            rc = runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT, interval=0,
                reconcile=lambda *_a, **_kw: fleetd.ReconcileResult())
        self.assertEqual(rc, 3)
        self.assertEqual(self.hub.sha(worker.claim.ref), worker_sha)
        self.assertFalse(any(call.args[0] == worker.claim.ref
                             for call in update.call_args_list))
        self.assertTrue(worker.alive())
        self.assertTrue(journal.read_job(worker.job_key).open)

    def test_host_retry_precedes_a_failing_reconcile_read(self):
        real_acquire = claim_mod.Claim.acquire_or_reap
        calls = {"host": 0, "reconcile": 0}

        def acquire(claim):
            if claim.kind == "host":
                calls["host"] += 1
                if calls["host"] == 1:
                    raise HubUnreachableError("first route down")
            return real_acquire(claim)

        def failing_step(*_args, **_kwargs):
            calls["reconcile"] += 1
            self.assertEqual(calls["host"], 2)
            raise HubUnreachableError("unrelated desired read failed")

        with mock.patch.object(claim_mod.Claim, "acquire_or_reap", acquire), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            rc = runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT,
                once=True, reconcile=failing_step)
        self.assertEqual(rc, 6)
        self.assertEqual(calls, {"host": 2, "reconcile": 1})

    def test_lost_worker_signal_precedes_host_retry_store_call(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        journal = journal_mod.Journal()
        worker = runner.start_gate(
            self.hub, "staging/one", "live", [str(self.stub)],
            self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        worker.claim.stop_renewer(timeout=2)
        real_acquire = claim_mod.Claim.acquire_or_reap
        real_adopt = journal_mod.adopt_at_startup
        real_kill = runner.kill_process_group
        events = []
        host_attempts = 0

        def acquire(claim):
            nonlocal host_attempts
            if claim.kind == "host":
                host_attempts += 1
                if host_attempts == 1:
                    raise HubUnreachableError("offline")
                self.assertIn("signal", events)
                events.append("host-retry")
            return real_acquire(claim)

        def adopt(*args, **kwargs):
            result = real_adopt(*args, **kwargs)
            for adopted in args[2]:
                adopted.claim._mark_lost("test lost lease")
            return result

        def kill(pgid, **kwargs):
            events.append("signal")
            kwargs["grace"] = 0
            return real_kill(pgid, **kwargs)

        with mock.patch.object(claim_mod.Claim, "acquire_or_reap", acquire), \
                mock.patch.object(journal_mod, "adopt_at_startup", adopt), \
                mock.patch.object(runner, "kill_process_group", kill), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            rc = runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT, once=True,
                reconcile=lambda *_a, **_kw: fleetd.ReconcileResult())
        self.assertEqual(rc, 0)
        self.assertEqual(events[:2], ["signal", "host-retry"])
        self.assertFalse(worker.alive())

    def test_lost_worker_during_host_retry_is_stopped_before_held_exit(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        journal = journal_mod.Journal()
        worker = runner.start_gate(
            self.hub, "staging/one", "live", [str(self.stub)],
            self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        worker.claim.stop_renewer(timeout=2)
        foreign = claim_mod.Claim(
            self.hub, kind="host", key=self.host, work_kind="fleetd",
            work_key=self.host, holder_host="different-machine")
        foreign.acquire()
        self.addCleanup(foreign.release)
        adopted = []
        real_acquire = claim_mod.Claim.acquire_or_reap
        real_adopt = journal_mod.adopt_at_startup
        host_attempts = 0

        def acquire(claim):
            nonlocal host_attempts
            if claim.kind == "host":
                host_attempts += 1
                if host_attempts == 1:
                    raise HubUnreachableError("offline")
                for item in adopted:
                    item.claim._mark_lost("lost during host retry")
            return real_acquire(claim)

        def adopt(*args, **kwargs):
            result = real_adopt(*args, **kwargs)
            adopted.extend(args[2])
            return result

        with mock.patch.object(claim_mod.Claim, "acquire_or_reap", acquire), \
                mock.patch.object(journal_mod, "adopt_at_startup", adopt), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            rc = runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT, interval=0,
                reconcile=lambda *_a, **_kw: fleetd.ReconcileResult())
        self.assertEqual(rc, 3)
        self.assertEqual(host_attempts, 2)
        self.assertFalse(worker.alive())
        # SIGTERM happened, but an adopted group may still exist until its
        # former parent reaps it. Keep the durable run open in that interval.
        self.assertTrue(journal.read_job(worker.job_key).open)

    def test_remote_journal_failure_follows_local_reconcile(self):
        calls = []

        def step(*_args, **_kwargs):
            calls.append("local-reconcile")
            return fleetd.ReconcileResult()

        def fail_after_start(*_args, **_kwargs):
            calls.append("journal-cleanup")
            if len(calls) > 1:
                raise HubUnreachableError("journal claim read failed")

        with mock.patch.object(runner, "reconcile_journal_runs", side_effect=fail_after_start), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            rc = runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT,
                once=True, reconcile=step)
        self.assertEqual(rc, 0)
        self.assertEqual(calls, ["journal-cleanup", "local-reconcile", "journal-cleanup"])

    def test_ambiguous_host_create_recovers_its_exact_claim(self):
        real_acquire = claim_mod.Claim.acquire_or_reap
        real_adopt = fleetd.adopt_workers
        calls = {"singleton": 0, "adopt": 0}
        allowed = []

        def ambiguous_once(claim):
            if claim.kind == "host":
                calls["singleton"] += 1
                if calls["singleton"] == 1:
                    real_acquire(claim)
                    raise HubUnreachableError("response lost after create")
            return real_acquire(claim)

        def offline_then_store(*args, **kwargs):
            calls["adopt"] += 1
            if calls["adopt"] == 1:
                raise HubUnreachableError("both routes down")
            return real_adopt(*args, **kwargs)

        def step(hub, host, _workers, *_args, **kw):
            allowed.append(kw["spawn_allowed"])
            if kw["spawn_allowed"]:
                self.assertIsNotNone(hub.sha(claim_mod.claim_ref("host", host)))
                os.kill(os.getpid(), signal_mod.SIGTERM)
            return fleetd.ReconcileResult()

        with mock.patch.object(claim_mod.Claim, "acquire_or_reap", ambiguous_once), \
                mock.patch.object(fleetd, "adopt_workers", offline_then_store), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            rc = runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT, interval=0, reconcile=step)
        self.assertEqual(rc, 0)
        self.assertEqual(allowed, [False, True])

    def test_later_ambiguous_host_retry_recovers_new_exact_token(self):
        real_acquire = claim_mod.Claim.acquire_or_reap
        attempts = 0
        allowed = []

        def retry_then_lose_reply(claim):
            nonlocal attempts
            if claim.kind == "host":
                attempts += 1
                if attempts == 1:
                    raise HubUnreachableError("initial route down")
                if attempts == 2:
                    real_acquire(claim)
                    raise HubUnreachableError("retry create response lost")
            return real_acquire(claim)

        def step(_hub, _host, _workers, *_args, **kwargs):
            allowed.append(kwargs["spawn_allowed"])
            if kwargs["spawn_allowed"] or len(allowed) >= 3:
                os.kill(os.getpid(), signal_mod.SIGTERM)
            return fleetd.ReconcileResult()

        with mock.patch.object(claim_mod.Claim, "acquire_or_reap",
                               retry_then_lose_reply), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            rc = runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT, interval=0,
                reconcile=step)
        self.assertEqual(rc, 0)
        self.assertEqual(attempts, 2)
        self.assertEqual(allowed, [False, False, True])

    def test_false_startup_listing_keeps_live_journal_run_open(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        journal = journal_mod.Journal()
        worker = runner.start_gate(
            self.hub, "staging/one", "live", [str(self.stub)],
            self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        worker.claim.stop_renewer(timeout=2)
        real_adopt = fleetd.adopt_workers

        def false_listing(*args, **kwargs):
            return real_adopt(*args, pgid_probe=lambda: set(), **kwargs)

        observed = {}

        def inspect(_hub, _host, workers, *_args, **_kw):
            observed["pgids"] = [w.pgid for w in workers]
            observed["claims"] = [w.claim for w in workers]
            return fleetd.ReconcileResult()

        with mock.patch.object(fleetd, "adopt_workers", false_listing), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            self.assertEqual(runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT, once=True,
                reconcile=inspect), 0)
        self.assertTrue(worker.alive())
        self.assertEqual(observed["pgids"], [worker.pgid])
        for claim in observed["claims"]:
            claim.stop_renewer(timeout=2)
        self.assertIsNotNone(self.hub.sha(worker.claim.ref))
        self.assertFalse(journal.read_job(worker.job_key).closed)
        with self.assertRaisesRegex(journal_mod.JournalError, "previous journal run"):
            runner.start_gate(self.hub, "staging/one", "duplicate", [str(self.stub)],
                              self.host, self.log_dir, journal=journal)

    def test_unavailable_process_listing_cannot_turn_open_run_into_release(self):
        j = journal_mod.Journal()
        w = runner.start_gate(self.hub, "staging/one", "live", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.workers.append(w)
        w.claim.stop_renewer(timeout=2)

        def scripted(_hub, _host, workers, *_args, **kw):
            self.assertFalse(kw["spawn_allowed"])
            self.assertEqual(workers, [])
            return fleetd.ReconcileResult()

        with mock.patch.object(fleetd, "adopt_workers",
                               side_effect=HubUnreachableError("offline")), \
             mock.patch.object(fleetd, "live_pgids",
                               side_effect=runner.ProcessListingUnavailable("ps failed")):
            self.assertEqual(runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)], log_dir=self.log_dir,
                repo_root=REPO_ROOT, once=True, reconcile=scripted), 0)
        self.assertIsNotNone(self.hub.sha(w.claim.ref))
        self.assertTrue(j.read_job(w.job_key).open)
        self.assertIsNone(w.popen.poll())

    def test_store_return_rebuilds_workers_before_reenabling_starts(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        j = journal_mod.Journal()
        w = runner.start_gate(self.hub, "staging/one", "live", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.workers.append(w)
        w.claim.stop_renewer(timeout=2)
        real_adopt = fleetd.adopt_workers
        calls = []
        refreshed = []

        def adopt(*args, **kwargs):
            calls.append(1)
            if len(calls) == 1:
                raise HubUnreachableError("initial listing down")
            result = real_adopt(*args, **kwargs)
            refreshed.extend(args[2])
            return result

        def scripted(_hub, _host, workers, *_args, **kw):
            self.assertEqual(workers, [])
            self.assertFalse(kw["spawn_allowed"])
            return fleetd.ReconcileResult()

        with mock.patch.object(fleetd, "adopt_workers", side_effect=adopt), \
             mock.patch.object(fleetd, "live_pgids",
                               side_effect=runner.ProcessListingUnavailable("initial ps down")):
            self.assertEqual(runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)], log_dir=self.log_dir,
                repo_root=REPO_ROOT, once=True, reconcile=scripted), 0)
        self.assertEqual(len(calls), 2)
        self.assertEqual([item.pgid for item in refreshed], [w.pgid])
        refreshed[0].claim.stop_renewer(timeout=2)
        self.assertTrue(j.read_job(w.job_key).open)

    def test_store_recheck_quiesces_old_renewer_before_claim_adoption(self):
        os.environ["FLEET_WORKER_MARKERS"] = str(self.stub)
        journal = journal_mod.Journal()
        worker = runner.start_gate(self.hub, "staging/one", "live", [str(self.stub)],
                                   self.host, self.log_dir, journal=journal)
        self.workers.append(worker)
        worker.claim.stop_renewer(timeout=2)
        real_adopt = fleetd.adopt_workers
        adopted_offline = []
        calls = 0

        def adopt(hub, host, workers, **kwargs):
            nonlocal calls
            calls += 1
            if calls == 1:
                raise HubUnreachableError("first listing down")
            self.assertEqual(len(adopted_offline), 1)
            self.assertFalse(adopted_offline[0].claim.renewer_running())
            return real_adopt(hub, host, workers, **kwargs)

        def step(_hub, _host, workers, *_args, **kwargs):
            self.assertEqual(len(workers), 1)
            adopted_offline.extend(workers)
            self.assertFalse(kwargs["spawn_allowed"])
            return fleetd.ReconcileResult()

        with mock.patch.object(fleetd, "adopt_workers", side_effect=adopt), \
                mock.patch.object(runner, "check_toolchain_agreement",
                                  return_value=(True, None)):
            self.assertEqual(runner.run_daemon(
                self.hub, self.host, gate_command=[str(self.stub)],
                log_dir=self.log_dir, repo_root=REPO_ROOT, once=True,
                reconcile=step), 0)
        self.assertEqual(calls, 2)
        adopted_offline[0].claim.stop_renewer(timeout=2)

    def test_hub_startup_closes_a_run_after_releasing_its_dead_claim(self):
        j = journal_mod.Journal()
        w = runner.start_gate(self.hub, "staging/one", "dead", [str(self.stub)],
                              self.host, self.log_dir, journal=j)
        self.workers.append(w)
        w.claim.stop_renewer(timeout=2)
        os.killpg(w.pgid, signal_mod.SIGKILL)
        w.popen.wait(timeout=10)

        def scripted(_hub, _host, workers, *_args, **kw):
            self.assertTrue(kw["spawn_allowed"])
            self.assertEqual(workers, [])
            return fleetd.ReconcileResult()

        self.assertEqual(runner.run_daemon(
            self.hub, self.host, gate_command=[str(self.stub)], log_dir=self.log_dir,
            repo_root=REPO_ROOT, once=True, reconcile=scripted), 0)
        self.assertIsNone(self.hub.sha(w.claim.ref))
        self.assertTrue(j.read_job(w.job_key).closed)

    def test_foreign_cas_claim_refuses_and_closes_only_this_offer(self):
        j = journal_mod.Journal(self.tmp / "journal")
        foreign = claim_mod.Claim(self.hub, kind="gate", key="staging-one",
                                  work_kind="gate", work_key="staging/one",
                                  holder_host="another-host")
        foreign.acquire()
        self.addCleanup(foreign.release)
        self.assertIsNone(runner.start_gate(
            self.hub, "staging/one", "refused", [str(self.stub)],
            self.host, self.log_dir, journal=j))
        state = j.read_job("gate-staging-one")
        self.assertEqual(state.events, ("offer", "exit"))
        self.assertTrue(state.closed)
        self.assertEqual(self.hub.read(foreign.ref)["holder_host"], "another-host")

    def test_failed_spawn_record_kills_group_and_releases_claim(self):
        j = journal_mod.Journal(self.tmp / "journal")
        spawned = []
        real_spawn = subprocess.Popen

        def record_spawn(*args, **kwargs):
            p = real_spawn(*args, **kwargs)
            if args and args[0] and args[0][0] == str(self.stub):
                spawned.append(p)
            return p

        with mock.patch.object(j, "spawn", side_effect=journal_mod.JournalWriteError("full")), \
             mock.patch.object(runner.subprocess, "Popen", side_effect=record_spawn):
            with self.assertRaises(journal_mod.JournalWriteError):
                runner.start_gate(self.hub, "staging/one", "failed-spawn", [str(self.stub)],
                                  self.host, self.log_dir, journal=j)
        self.assertEqual(len(spawned), 1)
        spawned[0].wait(timeout=10)
        self.assertIsNone(self.hub.sha(claim_mod.claim_ref("gate", "staging-one")))


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
