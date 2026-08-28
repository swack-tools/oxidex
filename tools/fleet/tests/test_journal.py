#!/usr/bin/env python3
"""The runner-local job journal (`keel/journal.py`; PLAN Stage 3 task 2).

Instrument: plain `unittest` against a throwaway bare state repo under
`tempfile.gettempdir()`, built through `_fixtures.make_hub` so every case
runs under `FLEET_TEST_HUB=bare` AND `=server`. Wherever a case is about a
process group rather than a store, the "worker" is a real parked
subprocess in its own session whose argv carries this fixture's marker and
(when the case wants it to be adoptable) the hub's real
`fleetd.fleet_scope_token`, so the identity check adoption performs is the
production one -- `ps` output through `fleetd._scoped_worker_in_group` --
and not a stub that agrees with the code under test by construction.

WHAT EACH CLASS PINS, and the bug it goes red for. Seventeen mutations of
`keel/journal.py` were applied one at a time to a scratch copy of this
tree and this module was re-run against each; every one goes red, and the
mutation is named beside the test it kills so a reader can re-run the
control rather than trust this sentence. Three of them went GREEN on the
first pass and the tests were strengthened until they did not:

  * the fsync count was taken on a job's FIRST record, where the
    directory fsync masked a missing record fsync -- it is taken on the
    second record now, and a separate case covers the directory;
  * `adopt_from_journal`'s `holder_host` gate was invisible behind
    `rebuild_claim`'s own, because the only foreign-host case used a LIVE
    process, which never reaches the release path --
    `test_another_hosts_dead_entry_is_refused_rather_than_released` is
    the case that exposes it;
  * "an absent journal is readable" was asserted against a mutation that
    was a no-op (`Path.glob` on a missing directory raises nothing), and
    the control was rewritten to make absence unreadable for real.

That is the point of running the controls rather than reasoning about
them: all three tests looked like they were pinning something.
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import claim as claim_mod  # noqa: E402
import fleetd  # noqa: E402
from fleetlib import HubError  # noqa: E402
from keel import journal as jr  # noqa: E402
from _env import HermeticCase, scrub_env  # noqa: E402
from _fixtures import break_hub, make_hub  # noqa: E402

GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}
HOST = "journalhost"
OTHER_HOST = "some-other-host"

# Long enough that no renewer declares a lease lost in the middle of a
# case that is not about that; the two cases that ARE about it set their
# own ttl explicitly.
TTL = 600.0
RENEW = 300.0


def build_bare(tmp: Path) -> Path:
    assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
    bare = tmp / "hub.git"
    subprocess.run(["git", "init", "-q", "--bare", str(bare)], check=True,
                   env=scrub_env(**GIT_ENV))
    return bare


def iso(dt: datetime) -> str:
    """The EXACT spelling `claim._owns` compares against."""
    return claim_mod._iso(dt)


class JournalCase(HermeticCase):
    """Tempdir, journal root, bare state repo, and process bookkeeping."""

    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.root = self.tmp / "keel" / "journal"
        self.j = jr.Journal(self.root)
        self.procs: list = []
        self.claims: list = []
        self.marker = f"journal-stub-{os.getpid()}"

    def tearDown(self):
        for c in self.claims:
            try:
                c.stop_renewer(timeout=2)
            except Exception:
                pass
        for p in self.procs:
            try:
                os.killpg(p.pid, signal.SIGKILL)
            except OSError:
                pass
            try:
                p.wait(timeout=10)
            except Exception:
                pass
        self.tmpdir.cleanup()
        super().tearDown()

    def make_hub_for(self):
        self.bare = build_bare(self.tmp)
        hub = make_hub(self, str(self.bare), workdir=self.tmp / "cache")
        return hub

    def spawn_stub(self, *, scoped: bool = True, marked: bool = True,
                   scope_token: str = None):
        """A parked process in its own session carrying this fixture's
        marker, and (by default) the hub's real scope token -- the exact
        two-part evidence `fleetd._scoped_worker_in_group` demands.

        `marked=False` drops the marker as well, which is what a pgid
        RECYCLED onto an unrelated same-uid process looks like."""
        name = f"{self.marker}.sh" if marked else "unrelated-bystander.sh"
        script = self.tmp / name
        script.write_text("#!/bin/bash\nsleep 120\n")
        script.chmod(0o755)
        argv = [str(script)]
        if scoped:
            argv.append(scope_token or self.token)
        p = subprocess.Popen(argv, start_new_session=True,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.procs.append(p)
        deadline = time.time() + 10
        while time.time() < deadline and p.pid not in fleetd.live_pgids():
            time.sleep(0.05)
        return p

    def kill_stub(self, p):
        os.killpg(p.pid, signal.SIGKILL)
        p.wait(timeout=10)
        deadline = time.time() + 10
        while time.time() < deadline and p.pid in fleetd.live_pgids():
            time.sleep(0.05)

    def journal_job(self, job_key, *, pgid=None, holder_host=HOST,
                    claim_ref=None, claim_sha="0" * 40, started_at=None,
                    expires_at=None, work_key=None, kind="gate",
                    scope_token=None, closed=False):
        """Write a complete offer/claim/spawn history for one job."""
        started = started_at or iso(datetime.now(timezone.utc) - timedelta(seconds=30))
        ref = claim_ref or claim_mod.claim_ref(kind, job_key)
        work_key = work_key or job_key.replace("-", "/", 1)
        self.j.offer(job_key=job_key, kind=kind, work_key=work_key, tag=job_key)
        self.j.claim(job_key=job_key, claim_ref=ref, claim_sha=claim_sha,
                     holder_host=holder_host, started_at=started,
                     expires_at=expires_at or iso(datetime.now(timezone.utc)
                                                  + timedelta(seconds=TTL)),
                     gate_version="8", rustc_id="rustc-from-the-gates-path",
                     platform_id="platform-from-the-gates-path")
        if pgid is not None:
            self.j.spawn(job_key=job_key, pid=pgid, pgid=pgid,
                         scope_token=scope_token or getattr(self, "token", None))
        if closed:
            self.j.exit(job_key=job_key, rc=0, outcome="PASS")
        return ref

    def seed_claim_on_hub(self, hub, ref, *, host, pgid, started_at,
                          expires_in=TTL):
        now = datetime.now(timezone.utc)
        self.assertTrue(hub.create(ref, {
            "holder_host": host, "pid": pgid, "pgid": pgid,
            "work_kind": "gate", "work_key": "staging/one",
            "started_at": started_at,
            "expires_at": iso(now + timedelta(seconds=expires_in)),
            "gate_version": "8", "rustc_id": "r", "platform_id": "p",
        }))
        return hub.sha(ref)


# --------------------------------------------------------------------- #
# Writing
# --------------------------------------------------------------------- #


class TestJournalWrites(JournalCase):
    """Append-only, fsynced, one file per job, unusable key never fatal.

    Controls run (each turns the named test red): dropping
    `os.fsync(fh.fileno())` after the record write; dropping the
    `_fsync_dir()` call on a new file.
    """

    def test_records_append_one_json_object_per_line(self):
        self.j.offer(job_key="k", kind="gate", work_key="staging/k", tag="k")
        self.j.spawn(job_key="k", pid=7, pgid=7)
        raw = (self.root / "k.jsonl").read_text()
        lines = raw.splitlines()
        self.assertEqual(len(lines), 2)
        self.assertTrue(raw.endswith("\n"), "every record must terminate its line")
        self.assertEqual([json.loads(x)["event"] for x in lines], ["offer", "spawn"])

    def test_a_later_record_never_overwrites_the_file(self):
        """Append-only is the property; `os.replace` of a whole file would
        lose the earlier state at exactly the moment a crash consults it."""
        self.j.offer(job_key="k", kind="gate", work_key="staging/k", tag="k")
        first = (self.root / "k.jsonl").read_text()
        self.j.exit(job_key="k", rc=0)
        self.assertTrue((self.root / "k.jsonl").read_text().startswith(first))

    def test_every_record_is_fsynced(self):
        """A record that is only `flush()`ed is in the kernel's page cache,
        not on the disk, and the crash the journal exists to survive is
        exactly the one that loses it. There is no way to observe a
        missing fsync after the fact, so this counts the calls.

        The FIRST append also fsyncs the directory (a new file's name has
        to be durable too), which would mask a missing record-fsync -- so
        the count is taken on the SECOND append, where the record's own
        fsync is the only one left to make.
        """
        self.j.offer(job_key="k", kind="gate", work_key="staging/k", tag="k")
        seen = []
        real = os.fsync

        def counting(fd):
            seen.append(fd)
            return real(fd)

        os.fsync = counting
        try:
            self.j.spawn(job_key="k", pid=7, pgid=7)
        finally:
            os.fsync = real
        self.assertEqual(len(seen), 1,
                         "a journal record that is not fsynced is not evidence")

    def test_a_new_files_directory_entry_is_fsynced_too(self):
        """Bytes on disk under a name that is not is a journal that has
        forgotten the job it most recently learned about."""
        seen = []
        real = os.fsync

        def counting(fd):
            seen.append(fd)
            return real(fd)

        os.fsync = counting
        try:
            self.j.offer(job_key="brand-new", kind="gate", work_key="s/n", tag="n")
        finally:
            os.fsync = real
        self.assertEqual(len(seen), 2, "record fsync + directory fsync")

    def test_none_valued_fields_are_dropped_so_a_later_record_cannot_unset(self):
        self.j.claim(job_key="k", claim_ref="refs/fleet/claims/gate/k",
                     claim_sha="abc", holder_host=HOST, started_at=iso(
                         datetime.now(timezone.utc)))
        self.j.spawn(job_key="k", pid=5, pgid=5)  # carries no claim_ref
        job = self.j.read_job("k")
        self.assertEqual(job.claim_ref, "refs/fleet/claims/gate/k")
        self.assertEqual(job.pgid, 5)

    def test_an_unusable_job_key_is_slugged_not_rejected(self):
        """Refusing to journal is refusing to spawn (`JournalWriteError`'s
        docstring), so an exotic branch name must not cost the fleet a
        gate. The record keeps the true key."""
        key = "intent:some/weird key"
        self.j.offer(job_key=key, kind="agent", work_key=key, tag="t")
        job = self.j.read_job(key)
        self.assertIsNotNone(job)
        self.assertEqual(job.job_key, key)
        self.assertNotIn("/", Path(job.path).name)

    def test_distinct_keys_never_share_a_file(self):
        a, b = "a/b", "a-b"
        self.j.offer(job_key=a, kind="gate", work_key=a, tag="a")
        self.j.offer(job_key=b, kind="gate", work_key=b, tag="b")
        self.assertNotEqual(self.j.path_for(a), self.j.path_for(b))
        self.assertEqual({j.job_key for j in self.j.scan().jobs}, {a, b})

    def test_a_slugged_stem_can_never_collide_with_a_verbatim_one(self):
        """The two namespaces are made disjoint by construction (`_`
        leads a slug and is forbidden as a verbatim stem's first
        character), rather than by trusting twelve hex digits."""
        slugged = jr.file_stem("a/b")
        self.assertTrue(slugged.startswith("_"))
        # No key that takes the verbatim path can produce this string...
        self.assertIsNone(jr._SAFE_STEM.fullmatch(slugged))
        # ...and feeding it back in takes the slug path too, so the
        # namespaces stay separate rather than folding into each other.
        self.assertNotEqual(jr.file_stem(slugged), slugged)

    def test_a_write_that_cannot_land_raises_rather_than_silently_succeeding(self):
        """The caller's contract is "do not spawn". A swallowed write error
        is a process nothing will ever be able to adopt."""
        blocked = self.tmp / "blocked"
        blocked.write_text("not a directory\n")
        j = jr.Journal(blocked / "journal")
        with self.assertRaises(jr.JournalWriteError):
            j.offer(job_key="k", kind="gate", work_key="staging/k", tag="k")

    def test_unknown_event_names_are_refused_at_the_write(self):
        with self.assertRaises(ValueError):
            self.j.append("renewed", job_key="k")


# --------------------------------------------------------------------- #
# Reading, and the fail-closed rules
# --------------------------------------------------------------------- #


class TestJournalScan(JournalCase):
    """Folding, and the three states of a journal read.

    Controls run: making `_parse_records` `continue` past a bad line
    instead of raising turns `test_a_corrupt_line_makes_the_whole_journal_
    unreadable` red (`readable` stays True); making `scan` record a torn
    tail as `unreadable` turns `test_a_truncated_final_record_is_
    tolerated_but_disarms_the_sweep` red; making an absent root report
    itself `unreadable` turns `test_an_absent_journal_is_readable_and_
    armed` red.
    """

    def test_the_fold_is_last_record_wins_per_field(self):
        self.journal_job("staging-one", pgid=1234, scope_token="fleet-scope=aaaa")
        self.j.verdict(job_key="staging-one", outcome="PASS", tree="t" * 40)
        job = self.j.scan().job("staging-one")
        self.assertEqual(job.pgid, 1234)
        self.assertEqual(job.work_key, "staging/one")
        self.assertEqual(job.outcome, "PASS")
        self.assertTrue(job.open)
        self.assertTrue(job.spawned)

    def test_an_exit_record_closes_the_job(self):
        self.journal_job("staging-one", pgid=1234, closed=True)
        scan = self.j.scan()
        self.assertEqual(scan.open_jobs, ())
        self.assertTrue(scan.job("staging-one").closed)

    def test_an_absent_journal_is_readable_and_armed(self):
        """A host that has never run a job has no journal. Treating that
        as corruption would disarm every fresh host's first sweep, for
        ever -- absent is not unreadable (SPEC §10 I6)."""
        scan = jr.Journal(self.tmp / "never-existed").scan()
        self.assertEqual(scan.jobs, ())
        self.assertTrue(scan.readable)
        self.assertTrue(scan.sweep_armed)

    def test_a_corrupt_line_makes_the_whole_journal_unreadable(self):
        self.journal_job("staging-one", pgid=1234)
        path = self.j.path_for("staging-one")
        raw = path.read_text().splitlines(keepends=True)
        raw.insert(1, "{ this is not json\n")
        path.write_text("".join(raw))

        scan = self.j.scan()

        self.assertFalse(scan.readable, "a corrupt record must fail closed")
        self.assertFalse(scan.sweep_armed)
        self.assertEqual([p for p, _ in scan.unreadable], [str(path)])
        self.assertIn("not valid JSON", scan.why_not_readable())

    def test_a_truncated_final_record_is_tolerated_but_disarms_the_sweep(self):
        """The dropped record is by construction the MOST RECENT one, so a
        torn tail is exactly where the `spawn` that names a pgid goes
        missing. The earlier records still stand (they were fsynced);
        the sweep does not."""
        self.journal_job("staging-one", pgid=1234)
        path = self.j.path_for("staging-one")
        with open(path, "a", encoding="utf-8") as fh:
            fh.write('{"v":1,"event":"exit","job_key":"staging-one","ts":"2026')

        scan = self.j.scan()

        self.assertTrue(scan.readable, "a torn tail is not corruption")
        self.assertFalse(scan.sweep_armed, "but it must disarm the sweep")
        job = scan.job("staging-one")
        self.assertTrue(job.torn)
        self.assertTrue(job.open, "the dropped record must not be guessed at")
        self.assertEqual(job.pgid, 1234, "the fsynced records still stand")

    def test_a_file_whose_only_record_is_torn_describes_no_job(self):
        path = self.root
        path.mkdir(parents=True, exist_ok=True)
        (path / "half.jsonl").write_text('{"v":1,"event":"offe')
        scan = self.j.scan()
        self.assertEqual(scan.jobs, ())
        self.assertTrue(scan.readable)
        self.assertFalse(scan.sweep_armed)

    def test_a_record_from_a_future_schema_fails_closed(self):
        """Version skew on this very host: a newer runner journaled here
        and an older one is now reading. "Skip what I do not recognize"
        is the wrong answer -- the skipped record may be the `exit` that
        closed a job."""
        self.journal_job("staging-one", pgid=1234)
        path = self.j.path_for("staging-one")
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps({"v": jr.SCHEMA_VERSION + 1, "event": "exit",
                                 "job_key": "staging-one", "ts": iso(
                                     datetime.now(timezone.utc))}) + "\n")
        scan = self.j.scan()
        self.assertFalse(scan.readable)
        self.assertIn("schema version", scan.why_not_readable())

    def test_an_unknown_event_fails_closed(self):
        self.journal_job("staging-one", pgid=1234)
        path = self.j.path_for("staging-one")
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps({"v": 1, "event": "reaped", "job_key": "staging-one",
                                 "ts": iso(datetime.now(timezone.utc))}) + "\n")
        scan = self.j.scan()
        self.assertFalse(scan.readable)
        self.assertIn("unknown event", scan.why_not_readable())

    def test_one_unreadable_file_disarms_the_whole_scan_not_just_itself(self):
        """Same argument as `adopt_workers`' unreadable-claim disarm: a
        knowably incomplete picture has no basis for calling anything an
        orphan."""
        self.journal_job("staging-one", pgid=1234)
        self.journal_job("staging-two", pgid=1235)
        (self.j.path_for("staging-two")).write_text("garbage\n")
        scan = self.j.scan()
        self.assertFalse(scan.readable)
        self.assertFalse(scan.sweep_armed)

    def test_read_job_raises_loudly_for_a_corrupt_single_job(self):
        self.journal_job("staging-one", pgid=1)
        self.j.path_for("staging-one").write_text("nope\n")
        with self.assertRaises(jr.JournalError):
            self.j.read_job("staging-one")

    def test_prune_removes_closed_jobs_and_never_open_ones(self):
        self.journal_job("old-closed", pgid=1, closed=True)
        self.journal_job("still-open", pgid=2)
        later = datetime.now(timezone.utc) + timedelta(days=30)
        removed = self.j.prune(retention_s=3600, now=later)
        self.assertEqual(removed, ["old-closed"])
        self.assertEqual({j.job_key for j in self.j.scan().jobs}, {"still-open"})


# --------------------------------------------------------------------- #
# Rebuilding a Claim from journal fields alone
# --------------------------------------------------------------------- #


class TestRebuildClaim(JournalCase):
    """The offline substitute for `Claim.adopt`.

    Controls run: dropping the `_iso(started) != job.started_at` guard
    turns `test_a_started_at_that_does_not_round_trip_is_refused` red;
    dropping the `holder_host != host` guard turns
    `test_another_hosts_journal_entry_is_never_rebuilt` red; anchoring
    unconditionally on the journaled `expires_at` (dropping the
    `expires > now` condition) turns
    `test_a_stale_journaled_expiry_falls_back_to_now` red.
    """

    def setUp(self):
        super().setUp()
        self.hub = self.make_hub_for()
        self.token = fleetd.fleet_scope_token(self.hub.url)

    def rebuild(self, job, **kw):
        out = jr.rebuild_claim(job, host=HOST, hub=self.hub, ttl=TTL,
                               renew_interval=RENEW, **kw)
        if out.claim is not None:
            self.claims.append(out.claim)
        return out

    def test_the_rebuilt_claim_carries_the_journals_ownership_token(self):
        started = iso(datetime.now(timezone.utc) - timedelta(seconds=45))
        self.journal_job("staging-one", pgid=4321, started_at=started)
        job = self.j.read_job("staging-one")

        out = self.rebuild(job)

        self.assertIsNotNone(out.claim, out.why)
        self.assertEqual(out.claim.ref, "refs/fleet/claims/gate/staging-one")
        self.assertEqual(out.claim.pgid, 4321)
        self.assertEqual(out.claim.work_key, "staging/one")
        self.assertEqual(out.claim.holder_host, HOST)
        # `_owns` is the property that matters: the rebuilt claim must
        # recognize a payload written by the acquisition it inherited.
        self.assertTrue(out.claim._owns({"holder_host": HOST, "started_at": started}))
        self.assertTrue(out.claim.renewer_running(),
                        "an adopted lease with no renewer is an unheld lease")

    def test_the_rebuilt_claims_pid_is_the_workers_never_this_runners(self):
        self.journal_job("staging-one", pgid=4321)
        out = self.rebuild(self.j.read_job("staging-one"))
        self.assertEqual(out.claim.pid, 4321)
        self.assertNotEqual(out.claim.pid, os.getpid())

    def test_the_toolchain_ids_are_restored_never_re_measured(self):
        """`Claim.adopt` restores `rustc_id`/`platform_id` off the ref
        because they were measured under the GATE's PATH, not the
        runner's (invariant I15) -- and because recomputing them shells
        out to rustc, per adopted job, on the one code path whose premise
        is that things are already going wrong."""
        self.journal_job("staging-one", pgid=4321)
        out = self.rebuild(self.j.read_job("staging-one"))
        self.assertEqual(out.claim._rustc_id, "rustc-from-the-gates-path")
        self.assertEqual(out.claim._platform_id, "platform-from-the-gates-path")
        self.assertEqual(out.claim.gate_version, "8")

    def test_another_hosts_journal_entry_is_never_rebuilt(self):
        self.journal_job("staging-one", pgid=4321, holder_host=OTHER_HOST)
        out = self.rebuild(self.j.read_job("staging-one"))
        self.assertIsNone(out.claim)
        self.assertIn(OTHER_HOST, out.why)

    def test_a_started_at_that_does_not_round_trip_is_refused(self):
        """`Claim.adopt` L568-573's guard, reproduced: a token we cannot
        reproduce byte-for-byte makes every renewal we send look, to us,
        like somebody else's claim."""
        self.journal_job("staging-one", pgid=4321,
                         started_at="2026-08-28T00:00:00Z")  # not claim._iso's spelling
        out = self.rebuild(self.j.read_job("staging-one"))
        self.assertIsNone(out.claim)
        self.assertIn("round-trip", out.why)

    def test_a_journal_entry_with_no_claim_ref_is_refused(self):
        self.j.offer(job_key="staging-one", kind="gate", work_key="staging/one", tag="t")
        self.j.spawn(job_key="staging-one", pid=99, pgid=99)
        out = self.rebuild(self.j.read_job("staging-one"))
        self.assertIsNone(out.claim)
        self.assertIn("claim ref", out.why)

    def test_a_live_journaled_expiry_anchors_the_renewer_on_it(self):
        now = datetime.now(timezone.utc)
        self.journal_job("staging-one", pgid=4321,
                         expires_at=iso(now + timedelta(seconds=TTL / 2)))
        out = self.rebuild(self.j.read_job("staging-one"), now=now)
        self.assertFalse(out.anchored_on_now)
        # `Claim.adopt`'s rule: expires_at - ttl, never a fresh TTL.
        self.assertLess(out.claim._last_renew_ok, now)

    def test_a_stale_journaled_expiry_falls_back_to_now(self):
        """Renewals are not journaled, so for any job older than one lease
        the journaled expiry is guaranteed stale. Anchoring on it makes
        `_note_renew_failure` declare a healthy, correctly-leased gate
        lost on the renewer's FIRST tick, and `lost` is sticky -- a store
        that returns four seconds later still costs the fleet the gate."""
        now = datetime.now(timezone.utc)
        self.journal_job("staging-one", pgid=4321,
                         expires_at=iso(now - timedelta(seconds=3600)))
        out = self.rebuild(self.j.read_job("staging-one"), now=now)
        self.assertTrue(out.anchored_on_now)
        self.assertFalse(out.claim.lost)
        # The proof the anchor is what makes the difference: with the
        # stale value the lease would already be unsalvageable.
        self.assertGreaterEqual(out.claim._last_renew_ok, now - timedelta(seconds=1))

    def test_a_stale_journaled_sha_is_repaired_by_the_ownership_token(self):
        """The journal records `claim_sha` at CLAIM time and never
        updates it, so it is expected to be stale by the time it is used.
        Correctness rests on `started_at`, not on the sha -- which is the
        same live-read rule SPEC §4.3 r1 states for claims."""
        started = iso(datetime.now(timezone.utc) - timedelta(seconds=30))
        ref = claim_mod.claim_ref("gate", "staging-one")
        self.seed_claim_on_hub(self.hub, ref, host=HOST, pgid=4321,
                               started_at=started)
        self.journal_job("staging-one", pgid=4321, started_at=started,
                         claim_sha="dead" * 10)  # never the real sha

        out = self.rebuild(self.j.read_job("staging-one"))

        self.assertIsNotNone(out.claim, out.why)
        self.assertTrue(out.claim.renew(), "the stale sha must self-heal")
        self.assertFalse(out.claim.lost)
        self.assertEqual(self.hub.read(ref)["started_at"], started,
                         "renewal must preserve the inherited acquisition")


# --------------------------------------------------------------------- #
# Offline adoption
# --------------------------------------------------------------------- #


class TestAdoptFromJournal(JournalCase):
    """The four dispositions, with real process groups.

    Controls run: replacing the `identity_probe` call with an
    unconditional "assumed ours" turns `test_a_recycled_pgid_is_never_
    adopted` red (the bystander is adopted); dropping the
    `scan.readable` gate turns `test_an_unreadable_journal_adopts_
    nothing` red; dropping the `holder_host` gate turns
    `test_another_hosts_dead_entry_is_refused_rather_than_released` red
    (and NOT the live-process case beside it -- see this module's
    docstring).
    """

    def setUp(self):
        super().setUp()
        self.hub = self.make_hub_for()
        self.token = fleetd.fleet_scope_token(self.hub.url)
        self.workers: list = []

    def adopt(self, **kw):
        kw.setdefault("markers", [self.marker])
        kw.setdefault("scope_token", self.token)
        res = jr.adopt_from_journal(
            self.j, HOST, self.workers, hub=self.hub,
            ttl=TTL, renew_interval=RENEW, **kw)
        for w in self.workers:
            if w.claim not in self.claims:
                self.claims.append(w.claim)
        return res

    def test_a_live_identity_verified_group_is_adopted(self):
        p = self.spawn_stub()
        self.journal_job("staging-one", pgid=p.pid)

        res = self.adopt()

        self.assertEqual([k for k, _ in res.adopted], ["staging-one"], res.summary())
        self.assertEqual(len(self.workers), 1)
        w = self.workers[0]
        self.assertEqual(w.pgid, p.pid)
        self.assertEqual(w.branch, "staging/one")
        self.assertIsNone(w.popen, "an adopted worker is not our child")
        self.assertTrue(w.alive())
        self.assertTrue(w.claim.renewer_running(),
                        "SPEC §5.3: adoption starts the renewer")
        self.assertEqual(res.to_release, [])
        self.assertIsNone(res.refused_wholesale)

    def test_a_journal_entry_whose_pgid_is_gone_is_released_never_adopted(self):
        p = self.spawn_stub()
        pgid = p.pid
        self.kill_stub(p)
        ref = self.journal_job("staging-one", pgid=pgid)

        res = self.adopt()

        self.assertEqual(res.adopted, [], "dead work must never be adopted")
        self.assertEqual(self.workers, [])
        self.assertEqual([(o.job_key, o.claim_ref) for o in res.to_release],
                         [("staging-one", ref)])
        self.assertIn("is gone", res.to_release[0].reason)

    def test_a_recycled_pgid_is_never_adopted(self):
        """A pgid is a name that gets recycled. The journal says which
        number to look at; `ps` says whether it is ours -- an alive,
        same-uid process that carries no worker marker and no scope token
        is a bystander, and adopting it would hand it to the lost-lease
        kill with no further checks."""
        bystander = self.spawn_stub(scoped=False, marked=False)
        ref = self.journal_job("staging-one", pgid=bystander.pid,
                               scope_token=self.token)

        res = self.adopt()

        self.assertEqual(res.adopted, [], "a recycled pgid must not be adopted")
        self.assertEqual(self.workers, [])
        self.assertEqual([(o.job_key, o.claim_ref) for o in res.to_release],
                         [("staging-one", ref)])
        self.assertIn("recycled", res.to_release[0].reason)
        self.assertIsNone(bystander.poll(),
                          "and it must certainly not be killed")

    def test_a_journal_entry_from_another_hubs_scope_is_refused(self):
        """The scope token is derived from the HUB URL, so a
        disagreement means this entry was written by a runner pointed at
        a different fleet. Adopting it would renew, on THIS hub, a lease
        taken on another -- the `fleet_scope_token` incident (a fixture
        daemon acting on the real fleet's processes) one level out."""
        p = self.spawn_stub()
        self.journal_job("staging-one", pgid=p.pid,
                         scope_token="fleet-scope=000000000000")

        res = self.adopt()

        self.assertEqual(res.adopted, [])
        self.assertEqual(res.to_release, [], "not ours to release either")
        self.assertIn("hub scope", res.refused[0][1])
        self.assertIsNone(p.poll())

    def test_another_hosts_entry_is_refused_not_released(self):
        """Deleting another host's live claim is the one thing
        `Claim.adopt` calls the most important thing it does NOT do."""
        p = self.spawn_stub()
        ref = self.journal_job("staging-one", pgid=p.pid, holder_host=OTHER_HOST)

        res = self.adopt()

        self.assertEqual(res.adopted, [])
        self.assertEqual(res.to_release, [],
                         "another host's claim is never ours to release")
        self.assertEqual([k for k, _ in res.refused], ["staging-one"])
        self.assertIn(OTHER_HOST, res.refused[0][1])
        self.assertEqual(ref, claim_mod.claim_ref("gate", "staging-one"))

    def test_another_hosts_dead_entry_is_refused_rather_than_released(self):
        """The case the `holder_host` gate exists for, and the one the
        live-process case above cannot expose. Reach the pgid checks with
        a foreign `holder_host` and the entry lands in `to_release` --
        which `release_pending` would then aim a CAS delete at. The gate
        has to come FIRST, before liveness is even considered."""
        p = self.spawn_stub()
        pgid = p.pid
        self.kill_stub(p)
        self.journal_job("staging-one", pgid=pgid, holder_host=OTHER_HOST)

        res = self.adopt()

        self.assertEqual(res.to_release, [],
                         "a dead process does not make another host's claim ours")
        self.assertEqual([k for k, _ in res.refused], ["staging-one"])

    def test_a_job_that_never_reached_spawn_is_released(self):
        """The runner died between taking the lease and `Popen`. No
        process exists; the claim is owed a release, the same disposition
        `fleetd.adopt_workers` gives a claim with no usable pgid."""
        ref = self.journal_job("staging-one", pgid=None)
        res = self.adopt()
        self.assertEqual(res.adopted, [])
        self.assertEqual([(o.job_key, o.claim_ref) for o in res.to_release],
                         [("staging-one", ref)])
        self.assertIn("never spawned", res.to_release[0].reason)

    def test_a_closed_job_is_never_looked_at_again(self):
        p = self.spawn_stub()
        self.journal_job("staging-one", pgid=p.pid, closed=True)
        res = self.adopt()
        self.assertEqual(res.adopted, [])
        self.assertEqual(res.to_release, [])
        self.assertEqual(res.refused, [])

    def test_an_unreadable_journal_adopts_nothing(self):
        """The task statement's fail-closed rule: adopt nothing, sweep
        nothing. Note the live, adoptable worker that is deliberately NOT
        adopted -- that is the cost, and it is the recoverable direction."""
        p = self.spawn_stub()
        self.journal_job("staging-one", pgid=p.pid)
        self.j.path_for("staging-one").write_text("{ truncated and wrong\n")

        res = self.adopt()

        self.assertEqual(res.adopted, [])
        self.assertEqual(self.workers, [])
        self.assertEqual(res.to_release, [])
        self.assertIsNotNone(res.refused_wholesale)
        self.assertIn("adopting nothing and sweeping nothing", res.refused_wholesale)
        self.assertIsNone(p.poll(), "and nothing is killed")

    def test_journal_adoption_never_kills_anything(self):
        """SPEC §5.3: the offline runner "sweeps nothing". An unleased,
        scope-token-carrying group -- the exact shape
        `fleetd.adopt_workers` SIGKILLs -- must survive this path,
        because the claim listing that entitles that kill is precisely
        what is unavailable."""
        orphan = self.spawn_stub()  # journaled by nobody, claimed by nobody
        res = self.adopt()
        self.assertEqual(res.adopted, [])
        self.assertIsNone(orphan.poll())
        self.assertEqual(res.sweep_skipped, "journal adoption never sweeps")

    def test_our_own_process_group_is_never_adopted(self):
        self.journal_job("staging-one", pgid=os.getpgrp())
        res = self.adopt()
        self.assertEqual(res.adopted, [])
        self.assertIn("own group", res.refused[0][1])


# --------------------------------------------------------------------- #
# Paying the owed releases once a route returns
# --------------------------------------------------------------------- #


class TestReleasePending(JournalCase):
    """The deferred half of "released, never adopted".

    Controls run: deleting without the `holder_host` re-check turns
    `test_a_claim_another_host_now_holds_is_left_alone` red (the other
    host's claim is deleted); deleting without the `started_at` re-check
    turns `test_a_claim_this_host_re_acquired_is_left_alone` red.
    """

    def setUp(self):
        super().setUp()
        self.hub = self.make_hub_for()
        self.token = fleetd.fleet_scope_token(self.hub.url)
        self.workers: list = []

    def owed(self, *, host_on_hub=HOST):
        started = iso(datetime.now(timezone.utc) - timedelta(seconds=30))
        ref = claim_mod.claim_ref("gate", "staging-one")
        self.seed_claim_on_hub(self.hub, ref, host=host_on_hub, pgid=999999,
                               started_at=started)
        self.journal_job("staging-one", pgid=999999, started_at=started)
        res = jr.adopt_from_journal(self.j, HOST, self.workers, hub=self.hub,
                                    markers=[self.marker], scope_token=self.token,
                                    ttl=TTL, renew_interval=RENEW)
        self.assertEqual(len(res.to_release), 1, res.summary())
        return ref, res

    def test_an_owed_release_is_cas_deleted_once_the_store_answers(self):
        ref, res = self.owed()
        out = jr.release_pending(self.hub, HOST, res, journal=self.j)
        self.assertEqual(out, [(ref, "released")])
        self.assertIsNone(self.hub.sha(ref), "the branch must return to the queue")
        self.assertEqual(res.to_release, [], "the debt is paid")
        self.assertTrue(self.j.read_job("staging-one").closed,
                        "and the journal entry is closed")

    def test_a_claim_another_host_now_holds_is_left_alone(self):
        """Between the crash and now the claim may have been legitimately
        reaped and re-taken. The journal is evidence; the store is
        authority."""
        ref, res = self.owed(host_on_hub=OTHER_HOST)
        out = jr.release_pending(self.hub, HOST, res, journal=self.j)
        self.assertIsNotNone(self.hub.sha(ref), "another host's claim must survive")
        self.assertEqual(self.hub.read(ref)["holder_host"], OTHER_HOST)
        self.assertIn("left alone", out[0][1])

    def test_a_claim_this_host_re_acquired_is_left_alone(self):
        """`holder_host` alone is half a token. This host may itself have
        taken the branch again in the interval -- our own next runner
        reaping it, or an `autonomous_when_serverless` gate -- which
        produces a claim with OUR `holder_host` and a different
        acquisition. Deleting that one drops a live gate's lease."""
        ref, res = self.owed()
        sha = self.hub.sha(ref)
        fresh = iso(datetime.now(timezone.utc))
        payload = self.hub.read(ref)
        payload["started_at"] = fresh
        self.assertTrue(self.hub.update(ref, payload, expect_sha=sha))

        out = jr.release_pending(self.hub, HOST, res, journal=self.j)

        self.assertIsNotNone(self.hub.sha(ref), "a live re-acquisition must survive")
        self.assertIn("re-acquired", out[0][1])

    def test_a_store_still_away_leaves_the_debt_owed(self):
        ref, res = self.owed()
        break_hub(self.hub, str(self.tmp / "not-a-repo"))
        out = jr.release_pending(self.hub, HOST, res, journal=self.j)
        self.assertIn("unreachable", out[0][1])
        self.assertEqual([o.claim_ref for o in res.to_release], [ref],
                         "an unreachable store must not discharge the debt")

    def test_a_claim_already_gone_is_simply_closed(self):
        ref, res = self.owed()
        self.assertTrue(self.hub.delete(ref, expect_sha=self.hub.sha(ref)))
        out = jr.release_pending(self.hub, HOST, res, journal=self.j)
        self.assertEqual(out, [(ref, "already gone")])
        self.assertEqual(res.to_release, [])


# --------------------------------------------------------------------- #
# The startup call itself: the rc 5 path is gone
# --------------------------------------------------------------------- #


class TestAdoptAtStartup(JournalCase):
    """`adopt_at_startup` -- what `keel/runner.py` calls in place of
    `fleetd.main`'s `adopt_workers(...) / except HubError: return 5`.

    Controls run: re-raising `HubError` instead of falling back to the
    journal turns `test_offline_start_adopts_from_the_journal_instead_of_
    refusing` and `test_an_offline_runner_is_not_allowed_to_spawn` red;
    never taking the `not scan.sweep_armed` branch turns both disarm
    cases red. `test_the_sweep_is_armed_with_a_readable_journal` is the
    in-suite control for those two: same orphan, same path, journal fine
    -- and the kill happens, so "the orphan survived" is evidence rather
    than a fixture that never kills anything.
    """

    def setUp(self):
        super().setUp()
        self.hub = self.make_hub_for()
        self.token = fleetd.fleet_scope_token(self.hub.url)
        self.workers: list = []
        self.logged: list = []
        self._grace = fleetd.KILL_GRACE_S
        fleetd.KILL_GRACE_S = 2.0

    def tearDown(self):
        fleetd.KILL_GRACE_S = self._grace
        super().tearDown()

    def startup(self, **kw):
        res = jr.adopt_at_startup(
            self.hub, HOST, self.workers, journal=self.j,
            markers=[self.marker], scope_token=self.token,
            ttl=TTL, renew_interval=RENEW,
            log=self.logged.append, **kw)
        for w in self.workers:
            if w.claim not in self.claims:
                self.claims.append(w.claim)
        return res

    # -- the store answers ------------------------------------------- #

    def test_with_the_store_up_the_hub_claim_is_truth_exactly_as_today(self):
        """SPEC §5.3. The journal contributes nothing to this decision --
        note that the journal here names a DIFFERENT (dead) pgid, and the
        adopted worker's pgid is the store's."""
        p = self.spawn_stub()
        started = iso(datetime.now(timezone.utc) - timedelta(seconds=30))
        ref = claim_mod.claim_ref("gate", "staging-one")
        self.seed_claim_on_hub(self.hub, ref, host=HOST, pgid=p.pid,
                               started_at=started)
        self.journal_job("staging-one", pgid=424242, started_at=started)

        res = self.startup()

        self.assertEqual(res.mode, "store")
        self.assertTrue(res.spawn_allowed)
        self.assertIsNone(res.journal_result)
        self.assertEqual([w.pgid for w in self.workers], [p.pid])

    def test_an_unreadable_journal_disarms_the_hub_driven_sweep(self):
        """SPEC §10 I6's journal-evidence case. The orphan below is
        exactly what `adopt_workers` kills; an unreadable journal
        suppresses that, and the suppression has to be decided BEFORE the
        hub pass runs, because a disarm that arrives after the SIGKILL
        disarms nothing."""
        orphan = self.spawn_stub()
        self.root.mkdir(parents=True, exist_ok=True)
        (self.root / "staging-one.jsonl").write_text("{ not json at all\n")

        res = self.startup()

        self.assertEqual(res.mode, "store")
        self.assertIsNotNone(res.sweep_skipped)
        self.assertEqual(res.suppressed_kills, [orphan.pid])
        self.assertIsNone(orphan.poll(), "the orphan must be alive")
        self.assertTrue(any("DISARMED" in line for line in self.logged),
                        f"a suppressed sweep must never be silent: {self.logged}")

    def test_the_sweep_is_armed_with_a_readable_journal(self):
        """The negative control for the case above: same orphan, same
        code path, journal fine -- and the kill happens. Without this,
        "the orphan survived" would prove nothing."""
        orphan = self.spawn_stub()
        self.journal_job("some-other-job", pgid=None)

        res = self.startup()

        self.assertIsNone(res.sweep_skipped)
        self.assertEqual(res.suppressed_kills, [])
        self.assertEqual([pg for pg, _ in res.hub_result.orphans_killed], [orphan.pid])

    def test_a_torn_final_record_also_disarms_the_sweep(self):
        orphan = self.spawn_stub()
        self.journal_job("staging-one", pgid=None)
        with open(self.j.path_for("staging-one"), "a", encoding="utf-8") as fh:
            fh.write('{"v":1,"event":"spawn"')

        res = self.startup()

        self.assertEqual(res.suppressed_kills, [orphan.pid])
        self.assertIsNone(orphan.poll())

    # -- neither route answers --------------------------------------- #

    def test_offline_start_adopts_from_the_journal_instead_of_refusing(self):
        """The rc 5 path (`fleetd.main` L2387-2394) is gone. With BOTH
        routes down the runner starts, adopts what it can prove is its
        own, and keeps the lease alive."""
        p = self.spawn_stub()
        self.journal_job("staging-one", pgid=p.pid)
        break_hub(self.hub, str(self.tmp / "not-a-repo"))

        res = self.startup()

        self.assertEqual(res.mode, "journal", res.summary())
        self.assertTrue(res.offline)
        self.assertIsInstance(res.hub_error, HubError)
        self.assertEqual([k for k, _ in res.journal_result.adopted], ["staging-one"])
        self.assertEqual([w.pgid for w in self.workers], [p.pid])
        self.assertTrue(self.workers[0].claim.renewer_running())
        self.assertTrue(any("adopting from the local job journal" in line
                            for line in self.logged), self.logged)

    def test_an_offline_runner_is_not_allowed_to_spawn(self):
        """SPEC §5.3: "sweeps nothing, spawns nothing, and retries the
        store every 30 s". Work whose claim cannot be CAS-arbitrated is
        the duplicate-gate hazard leases exist for."""
        break_hub(self.hub, str(self.tmp / "not-a-repo"))
        res = self.startup()
        self.assertFalse(res.spawn_allowed)
        self.assertEqual(jr.OFFLINE_RETRY_S, 30)

    def test_offline_start_with_no_journal_at_all_still_starts(self):
        """An empty journal is not an error -- it is a host with nothing
        running. What must NOT happen is a refusal to start."""
        break_hub(self.hub, str(self.tmp / "not-a-repo"))
        res = self.startup()
        self.assertEqual(res.mode, "journal")
        self.assertEqual(self.workers, [])
        self.assertEqual(res.journal_result.adopted, [])
        self.assertIsNone(res.journal_result.refused_wholesale)

    def test_offline_start_with_an_unreadable_journal_adopts_nothing(self):
        p = self.spawn_stub()
        self.journal_job("staging-one", pgid=p.pid)
        self.j.path_for("staging-one").write_text("}\n")
        break_hub(self.hub, str(self.tmp / "not-a-repo"))

        res = self.startup()

        self.assertEqual(res.mode, "journal")
        self.assertEqual(self.workers, [], "fail closed: adopt nothing")
        self.assertIsNotNone(res.journal_result.refused_wholesale)
        self.assertIsNone(p.poll(), "and sweep nothing")

    def test_offline_start_never_kills_and_never_adopts_a_dead_group(self):
        p = self.spawn_stub()
        pgid = p.pid
        self.kill_stub(p)
        orphan = self.spawn_stub()
        self.journal_job("staging-one", pgid=pgid)
        break_hub(self.hub, str(self.tmp / "not-a-repo"))

        res = self.startup()

        self.assertEqual(res.journal_result.adopted, [])
        self.assertEqual(len(res.journal_result.to_release), 1)
        self.assertIsNone(orphan.poll(), "the offline runner sweeps nothing")


if __name__ == "__main__":
    unittest.main()
