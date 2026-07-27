"""Hermetic tests for overlord_sweep.py (spec M4).

Real `git` is exercised against throwaway tempdir repos (matching
test_squad_merge_loop.py's own style): the merge/revert/bisection
mechanics are exactly what needs a real git, everything else (a real
cargo build/test, `gh`, the network) is injected. No test touches the
real ~/.oxidex.
"""
import json
import os
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import overlord_sweep
import squad_merge_loop

GIT_ENV_OVERRIDES = {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}


def git(repo, *args, input_text=None, check=True):
    return subprocess.run(
        ["git", *args], cwd=repo, input=input_text, capture_output=True, text=True, check=check,
    )


def git_out(repo, *args, input_text=None):
    return git(repo, *args, input_text=input_text).stdout


class GitRepoTestCase(unittest.TestCase):
    def setUp(self):
        patcher = patch.dict(os.environ, GIT_ENV_OVERRIDES)
        patcher.start()
        self.addCleanup(patcher.stop)
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)

    def make_repo(self, name="repo"):
        repo = self.tmp / name
        repo.mkdir()
        git(repo, "init", "-q", "-b", "main")
        git(repo, "config", "user.email", "fleet@example.com")
        git(repo, "config", "user.name", "Fleet Test")
        git(repo, "config", "commit.gpgsign", "false")
        (repo / "README.md").write_text("base\n")
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", "base commit")
        return repo

    def commit_file(self, repo, rel_path, content, message, trailers=None):
        path = repo / rel_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        git(repo, "add", "-A")
        args = ["commit", "-q", "-m", message]
        if trailers:
            args += ["-m", "\n".join(f"{k}: {v}" for k, v in trailers)]
        git(repo, *args)
        return git_out(repo, "rev-parse", "HEAD").strip()

    def new_file_commit(self, repo, rel_path, content, message):
        """A commit whose diff is a genuine NEW file add (for the
        judgment-queue new-file classification), as opposed to
        commit_file's first call on a not-yet-tracked path -- kept as
        its own helper for readability at call sites."""
        return self.commit_file(repo, rel_path, content, message)


# ---------------------------------------------------------------------------
# preflight
# ---------------------------------------------------------------------------

class PreflightTests(unittest.TestCase):
    def _write_lock(self, path, pid=111, sha="s", heartbeat_ts=1000.0):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"pid": pid, "script_git_sha": sha, "heartbeat_ts": heartbeat_ts}))

    def test_missing_locks_are_healthy_not_stale(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            result = overlord_sweep.preflight(
                home, ["canon", "nikon"], dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                now_fn=lambda: 2000.0,
            )
        self.assertTrue(result["ok"])
        self.assertEqual(result["stale"], [])

    def test_fresh_heartbeat_is_healthy(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            self._write_lock(squad_merge_loop.merger_lock_path(home, "canon"), heartbeat_ts=1000.0)
            result = overlord_sweep.preflight(
                home, ["canon"], dispatcher_lock_path=home / "logs" / "dispatcher.lock", now_fn=lambda: 1005.0,
            )
        self.assertTrue(result["ok"])

    def test_stale_merger_heartbeat_is_reported_not_fatal(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            self._write_lock(squad_merge_loop.merger_lock_path(home, "canon"), heartbeat_ts=1000.0)
            result = overlord_sweep.preflight(
                home, ["canon"], dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                now_fn=lambda: 1000.0 + 10_000, stale_seconds=600,
            )
            self.assertFalse(result["ok"])
            self.assertIn("canon", result["stale"])
            # never actually tries to acquire anything -- the lock file
            # is left exactly as it was
            self.assertTrue(squad_merge_loop.merger_lock_path(home, "canon").exists())

    def test_stale_dispatcher_lock_is_reported(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            lock_path = home / "logs" / "dispatcher.lock"
            self._write_lock(lock_path, heartbeat_ts=1000.0)
            result = overlord_sweep.preflight(
                home, [], dispatcher_lock_path=lock_path, now_fn=lambda: 1000.0 + 10_000, stale_seconds=600,
            )
        self.assertFalse(result["ok"])
        self.assertIn("dispatcher", result["stale"])

    def test_corrupt_lock_file_is_treated_as_stale(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            lock_path = squad_merge_loop.merger_lock_path(home, "canon")
            lock_path.parent.mkdir(parents=True)
            lock_path.write_text("{not json")
            result = overlord_sweep.preflight(
                home, ["canon"], dispatcher_lock_path=home / "logs" / "dispatcher.lock", now_fn=lambda: 0,
            )
        self.assertFalse(result["ok"])
        self.assertIn("canon", result["stale"])

    def test_real_dispatcher_lock_bare_pid_format_alive_is_healthy(self):
        # acquire_dispatcher_lock writes a BARE "<pid>\n" (no JSON
        # object, no heartbeat_ts at all) -- json.loads still parses
        # that as a plain int, which must be handled as its own shape,
        # not treated as "not a dict -> stale" (which would report a
        # genuinely healthy, running dispatcher as stale every time).
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            lock_path = home / "logs" / "dispatcher.lock"
            lock_path.parent.mkdir(parents=True)
            lock_path.write_text(f"{os.getpid()}\n")  # our own pid: guaranteed alive
            result = overlord_sweep.preflight(
                home, [], dispatcher_lock_path=lock_path, now_fn=lambda: 0,
            )
        self.assertTrue(result["ok"])
        self.assertNotIn("dispatcher", result["stale"])

    def test_bare_pid_format_dead_pid_is_reported_stale(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            lock_path = home / "logs" / "dispatcher.lock"
            lock_path.parent.mkdir(parents=True)
            lock_path.write_text("999999\n")
            result = overlord_sweep.preflight(
                home, [], dispatcher_lock_path=lock_path, now_fn=lambda: 0,
                dispatcher_alive_fn=lambda pid: False,
            )
        self.assertFalse(result["ok"])
        self.assertIn("dispatcher", result["stale"])

    def test_bare_pid_format_alive_pid_via_injected_alive_fn_is_healthy(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            lock_path = home / "logs" / "dispatcher.lock"
            lock_path.parent.mkdir(parents=True)
            lock_path.write_text("12345\n")
            result = overlord_sweep.preflight(
                home, [], dispatcher_lock_path=lock_path, now_fn=lambda: 0,
                dispatcher_alive_fn=lambda pid: True,
            )
        self.assertTrue(result["ok"])


# ---------------------------------------------------------------------------
# sweep-state cursor + green-stamp collection
# ---------------------------------------------------------------------------

class SweepStateTests(unittest.TestCase):
    def test_missing_file_is_no_news(self):
        self.assertEqual(overlord_sweep.load_sweep_state(Path("/does/not/exist.json")), {"squads": {}})

    def test_corrupt_file_is_no_news_not_raise(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "state.json"
            path.write_text("{not json")
            self.assertEqual(overlord_sweep.load_sweep_state(path), {"squads": {}})

    def test_round_trips_through_atomic_write(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "state.json"
            data = {"squads": {"canon": {"last_ts": "x", "last_squad_sha": "abc"}}}
            overlord_sweep.save_sweep_state(path, data)
            self.assertEqual(overlord_sweep.load_sweep_state(path), data)


class CollectGreenStampsTests(unittest.TestCase):
    def test_collects_newest_consumed_entry_per_squad(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            path = squad_merge_loop.squad_status_file(home, "canon")
            squad_merge_loop.record_head(path, "sha1", status="consumed", patch_id="p1", format_name="JPEG",
                                         squad_sha="squadsha1", now_fn=lambda: 100)
            squad_merge_loop.record_head(path, "sha2", status="consumed", patch_id="p2", format_name="CR2",
                                         squad_sha="squadsha2", now_fn=lambda: 200)
            stamps, new_cursor = overlord_sweep.collect_green_stamps(home, ["canon"], {"squads": {}})
        self.assertEqual(stamps["canon"]["squad_sha"], "squadsha2")
        self.assertEqual(stamps["canon"]["formats"], ["CR2", "JPEG"])
        self.assertEqual(new_cursor["squads"]["canon"]["last_squad_sha"], "squadsha2")

    def test_no_news_when_nothing_newer_than_cursor(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            path = squad_merge_loop.squad_status_file(home, "canon")
            squad_merge_loop.record_head(path, "sha1", status="consumed", patch_id="p1", format_name="JPEG",
                                         squad_sha="squadsha1", now_fn=lambda: 100)
            status = json.loads(path.read_text())
            ts = status["heads"]["sha1"]["ts"]
            cursor = {"squads": {"canon": {"last_ts": ts, "last_squad_sha": "squadsha1"}}}
            stamps, _new_cursor = overlord_sweep.collect_green_stamps(home, ["canon"], cursor)
        self.assertEqual(stamps, {})

    def test_missing_squad_status_file_is_no_news(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            stamps, _cursor = overlord_sweep.collect_green_stamps(home, ["canon"], {"squads": {}})
        self.assertEqual(stamps, {})

    def test_corrupt_squad_status_file_is_no_news_not_raise(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            path = squad_merge_loop.squad_status_file(home, "canon")
            path.parent.mkdir(parents=True)
            path.write_text("{not json")
            stamps, _cursor = overlord_sweep.collect_green_stamps(home, ["canon"], {"squads": {}})
        self.assertEqual(stamps, {})

    def test_quarantined_only_entries_are_not_green_stamps(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            path = squad_merge_loop.squad_status_file(home, "canon")
            squad_merge_loop.record_head(path, "sha1", status="quarantined", patch_id="p1", format_name="JPEG",
                                         now_fn=lambda: 100)
            stamps, _cursor = overlord_sweep.collect_green_stamps(home, ["canon"], {"squads": {}})
        self.assertEqual(stamps, {})


# ---------------------------------------------------------------------------
# DST fall-back: the stamp ordering key must be an INSTANT, not a naive
# local-time string
# ---------------------------------------------------------------------------

class DstFallBackStampOrderingTests(unittest.TestCase):
    """record_head stamps `ts` with offset-free LOCAL time and
    collect_green_stamps both filtered and sorted that string
    lexicographically. Inside the DST fall-back's repeated hour a LATER
    instant therefore sorts EARLIER, so the newest consumed head is
    passed over and the round reports the older squad_sha (or, on the
    next round, no news at all).

    Measured with TZ=America/Los_Angeles (both epochs verified against
    time.localtime):
      1793521800 -> 2026-11-01T01:30:00 PDT-0700   (earlier instant)
      1793524500 -> 2026-11-01T01:15:00 PST-0800   (2700s LATER instant)
    "01:15:00" < "01:30:00" as a string, so the later head loses.
    """

    EARLIER_PDT = 1793521800.0
    LATER_PST = 1793524500.0

    def setUp(self):
        patcher = patch.dict(os.environ, {"TZ": "America/Los_Angeles"})
        patcher.start()
        self.addCleanup(patcher.stop)
        time.tzset()
        # tzset() reads the RESTORED environment, so re-running it during
        # cleanup is what actually puts this process's clock back.
        self.addCleanup(time.tzset)
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.home = Path(self._tmp.name)

    def _record(self, sha, squad_sha, epoch):
        squad_merge_loop.record_head(
            squad_merge_loop.squad_status_file(self.home, "canon"), sha, status="consumed",
            patch_id=f"p-{sha}", format_name="JPEG", squad_sha=squad_sha, now_fn=lambda: epoch,
        )

    def test_the_later_instant_in_the_repeated_hour_wins(self):
        self._record("headA", "aaaa", self.EARLIER_PDT)
        self._record("headB", "bbbb", self.LATER_PST)
        stamps, new_cursor = overlord_sweep.collect_green_stamps(self.home, ["canon"], {"squads": {}})
        self.assertEqual(stamps["canon"]["squad_sha"], "bbbb")
        self.assertEqual(new_cursor["squads"]["canon"]["last_squad_sha"], "bbbb")

    def test_a_stamp_recorded_after_the_cursor_is_still_news(self):
        # The consequence that actually stalls a squad: round 1 consumes
        # the 01:30 PDT stamp and writes it as the cursor; round 2's 01:15
        # PST stamp is a LATER instant but a smaller string, so the
        # string filter reports "no news" and the round does nothing.
        self._record("headA", "aaaa", self.EARLIER_PDT)
        _stamps, cursor = overlord_sweep.collect_green_stamps(self.home, ["canon"], {"squads": {}})
        self._record("headB", "bbbb", self.LATER_PST)
        stamps, _new_cursor = overlord_sweep.collect_green_stamps(self.home, ["canon"], cursor)
        self.assertEqual(stamps.get("canon", {}).get("squad_sha"), "bbbb")

    def test_a_legacy_cursor_with_no_epoch_still_sees_new_epoch_stamps(self):
        """The migration case a naive UTC switch breaks. On-disk cursors
        written before this fix hold a naive LOCAL string; a stamp
        written after it must still register as news against that
        cursor, or every squad stalls for the length of the UTC offset.
        """
        self._record("headA", "aaaa", self.EARLIER_PDT)
        legacy_cursor = {"squads": {"canon": {"last_ts": "2026-11-01T00:00:00",
                                              "last_squad_sha": "older"}}}
        stamps, _new_cursor = overlord_sweep.collect_green_stamps(
            self.home, ["canon"], legacy_cursor,
        )
        self.assertEqual(stamps["canon"]["squad_sha"], "aaaa")

    def test_a_legacy_stamp_with_no_epoch_is_still_collected(self):
        # The other half of the migration: squad-status files already on
        # disk carry no ts_epoch at all, and must keep working.
        path = squad_merge_loop.squad_status_file(self.home, "canon")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"heads": {"legacyhead": {
            "status": "consumed", "patch_id": "p", "format": "JPEG", "work_done": True,
            "ts": "2026-11-01T01:30:00", "squad_sha": "legacysha",
        }}}))
        stamps, new_cursor = overlord_sweep.collect_green_stamps(self.home, ["canon"], {"squads": {}})
        self.assertEqual(stamps["canon"]["squad_sha"], "legacysha")
        self.assertEqual(new_cursor["squads"]["canon"]["last_ts"], "2026-11-01T01:30:00")


# ---------------------------------------------------------------------------
# Fresh sweep branch naming
# ---------------------------------------------------------------------------

class NextSweepBranchNameTests(GitRepoTestCase):
    def test_no_existing_branches_starts_at_1(self):
        repo = self.make_repo()
        name = overlord_sweep.next_sweep_branch_name(repo, overlord_sweep.default_run_git, date_str="2026-07-24")
        self.assertEqual(name, "sweep/tags-2026-07-24-1")

    def test_increments_past_existing_local_branches(self):
        repo = self.make_repo()
        git(repo, "branch", "sweep/tags-2026-07-24-1", "main")
        git(repo, "branch", "sweep/tags-2026-07-24-3", "main")
        name = overlord_sweep.next_sweep_branch_name(repo, overlord_sweep.default_run_git, date_str="2026-07-24")
        self.assertEqual(name, "sweep/tags-2026-07-24-4")

    def test_different_date_is_independent(self):
        repo = self.make_repo()
        git(repo, "branch", "sweep/tags-2026-07-24-5", "main")
        name = overlord_sweep.next_sweep_branch_name(repo, overlord_sweep.default_run_git, date_str="2026-07-25")
        self.assertEqual(name, "sweep/tags-2026-07-25-1")

    def test_considers_remote_tracking_refs_too(self):
        repo = self.make_repo()
        git(repo, "update-ref", "refs/remotes/origin/sweep/tags-2026-07-24-7", "main")
        name = overlord_sweep.next_sweep_branch_name(repo, overlord_sweep.default_run_git, date_str="2026-07-24")
        self.assertEqual(name, "sweep/tags-2026-07-24-8")


class CutFreshSweepBranchTests(GitRepoTestCase):
    def test_creates_and_checks_out_branch_from_origin_ref(self):
        repo = self.make_repo()
        ok, _message = overlord_sweep.cut_fresh_sweep_branch(
            repo, "sweep/tags-x-1", overlord_sweep.default_run_git, origin_ref="main",
        )
        self.assertTrue(ok)
        current = git_out(repo, "rev-parse", "--abbrev-ref", "HEAD").strip()
        self.assertEqual(current, "sweep/tags-x-1")


# ---------------------------------------------------------------------------
# merge_squad_into_sweep
# ---------------------------------------------------------------------------

class MergeSquadIntoSweepTests(GitRepoTestCase):
    def test_ff_only_merge_for_the_first_squad_on_a_fresh_branch(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(repo, "canon.txt", "1", "canon fix")
        git(repo, "checkout", "-q", "main")
        overlord_sweep.cut_fresh_sweep_branch(repo, "sweep/tags-x-1", overlord_sweep.default_run_git, origin_ref="main")

        info = overlord_sweep.merge_squad_into_sweep(repo, "canon", canon_sha, overlord_sweep.default_run_git)

        self.assertTrue(info["ok"])
        self.assertEqual(info["mode"], "ff")
        self.assertIsNone(info["merge_sha"])
        self.assertEqual(git_out(repo, "rev-parse", "HEAD").strip(), canon_sha)

    def test_second_squad_falls_through_to_a_controlled_merge(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(repo, "canon.txt", "1", "canon fix")
        git(repo, "checkout", "-q", "main")
        git(repo, "branch", "squad/nikon", "main")
        git(repo, "checkout", "-q", "squad/nikon")
        nikon_sha = self.commit_file(repo, "nikon.txt", "1", "nikon fix")
        git(repo, "checkout", "-q", "main")

        overlord_sweep.cut_fresh_sweep_branch(repo, "sweep/tags-x-1", overlord_sweep.default_run_git, origin_ref="main")
        info1 = overlord_sweep.merge_squad_into_sweep(repo, "canon", canon_sha, overlord_sweep.default_run_git)
        info2 = overlord_sweep.merge_squad_into_sweep(repo, "nikon", nikon_sha, overlord_sweep.default_run_git)

        self.assertEqual(info1["mode"], "ff")
        self.assertEqual(info2["mode"], "merge")
        self.assertIsNotNone(info2["merge_sha"])
        self.assertTrue((repo / "canon.txt").exists())
        self.assertTrue((repo / "nikon.txt").exists())

    def test_conflict_is_a_hard_error_isolated_to_that_squad(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(repo, "shared.txt", "canon-version", "canon change")
        git(repo, "checkout", "-q", "main")
        # Diverge main so a plain ff is impossible AND the same path
        # conflicts (add/add, different content on both sides).
        self.commit_file(repo, "shared.txt", "main-version", "main change")

        overlord_sweep.cut_fresh_sweep_branch(repo, "sweep/tags-x-1", overlord_sweep.default_run_git, origin_ref="main")
        info = overlord_sweep.merge_squad_into_sweep(repo, "canon", canon_sha, overlord_sweep.default_run_git)

        self.assertFalse(info["ok"])
        self.assertIn("conflict", info["message"])
        # merge was cleanly aborted -- no leftover conflict state
        self.assertEqual(git_out(repo, "status", "--porcelain").strip(), "")


class RevertSquadContributionRoundTripTests(GitRepoTestCase):
    """undo_last_revert only ever reverts the single most recent commit
    on HEAD -- revert_squad_contribution's ff-mode path must therefore
    always produce exactly ONE revert commit for a squad's WHOLE
    contribution, however many commits it has, or a restore after a
    failed isolation probe silently drops everything but the last one."""

    def test_ff_mode_multi_commit_contribution_is_one_revert_commit(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        self.commit_file(repo, "a.txt", "1", "add a")
        canon_tip = self.commit_file(repo, "b.txt", "1", "add b")
        git(repo, "checkout", "-q", "main")

        overlord_sweep.cut_fresh_sweep_branch(repo, "sweep/x-1", overlord_sweep.default_run_git, origin_ref="main")
        info = overlord_sweep.merge_squad_into_sweep(repo, "canon", canon_tip, overlord_sweep.default_run_git)
        self.assertEqual(info["mode"], "ff")

        head_before_revert = git_out(repo, "rev-parse", "HEAD").strip()
        ok, _message = overlord_sweep.revert_squad_contribution(repo, info, overlord_sweep.default_run_git)
        self.assertTrue(ok)
        # Exactly one new commit on top of the pre-revert tip -- not one
        # per original commit.
        revert_commits = overlord_sweep.commits_in_range(
            repo, head_before_revert, git_out(repo, "rev-parse", "HEAD").strip(), overlord_sweep.default_run_git,
        )
        self.assertEqual(len(revert_commits), 1)
        self.assertFalse((repo / "a.txt").exists())
        self.assertFalse((repo / "b.txt").exists())

    def test_undo_after_ff_mode_multi_commit_revert_restores_everything(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        self.commit_file(repo, "a.txt", "1", "add a")
        canon_tip = self.commit_file(repo, "b.txt", "1", "add b")
        git(repo, "checkout", "-q", "main")

        overlord_sweep.cut_fresh_sweep_branch(repo, "sweep/x-1", overlord_sweep.default_run_git, origin_ref="main")
        info = overlord_sweep.merge_squad_into_sweep(repo, "canon", canon_tip, overlord_sweep.default_run_git)
        overlord_sweep.revert_squad_contribution(repo, info, overlord_sweep.default_run_git)

        ok, _message = overlord_sweep.undo_last_revert(repo, overlord_sweep.default_run_git)
        self.assertTrue(ok)
        # BOTH of canon's files must come back -- not just the one from
        # the most-recently-authored original commit.
        self.assertTrue((repo / "a.txt").exists())
        self.assertTrue((repo / "b.txt").exists())


class CommitsContributedTests(GitRepoTestCase):
    def test_ff_mode_returns_the_plain_range(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        sha1 = self.commit_file(repo, "a.txt", "1", "a")
        sha2 = self.commit_file(repo, "b.txt", "1", "b")
        git(repo, "checkout", "-q", "main")
        overlord_sweep.cut_fresh_sweep_branch(repo, "sweep/x-1", overlord_sweep.default_run_git, origin_ref="main")
        info = overlord_sweep.merge_squad_into_sweep(repo, "canon", sha2, overlord_sweep.default_run_git)
        commits = overlord_sweep.commits_contributed(repo, info, overlord_sweep.default_run_git)
        self.assertEqual(commits, [sha1, sha2])

    def test_merge_mode_excludes_the_wrapper_commit(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(repo, "canon.txt", "1", "canon fix")
        git(repo, "checkout", "-q", "main")
        git(repo, "branch", "squad/nikon", "main")
        git(repo, "checkout", "-q", "squad/nikon")
        nikon_sha = self.commit_file(repo, "nikon.txt", "1", "nikon fix")
        git(repo, "checkout", "-q", "main")

        overlord_sweep.cut_fresh_sweep_branch(repo, "sweep/x-1", overlord_sweep.default_run_git, origin_ref="main")
        overlord_sweep.merge_squad_into_sweep(repo, "canon", canon_sha, overlord_sweep.default_run_git)
        info2 = overlord_sweep.merge_squad_into_sweep(repo, "nikon", nikon_sha, overlord_sweep.default_run_git)

        commits = overlord_sweep.commits_contributed(repo, info2, overlord_sweep.default_run_git)
        self.assertEqual(commits, [nikon_sha])
        self.assertNotIn(info2["merge_sha"], commits)


# ---------------------------------------------------------------------------
# Verified-trailer delta parsing
# ---------------------------------------------------------------------------

class VerifiedDeltaTests(GitRepoTestCase):
    def test_parse_verified_delta(self):
        self.assertEqual(overlord_sweep.parse_verified_delta("recheck-pass gaps=3->1"), 2)
        self.assertIsNone(overlord_sweep.parse_verified_delta("garbage"))
        self.assertIsNone(overlord_sweep.parse_verified_delta(None))
        self.assertIsNone(overlord_sweep.parse_verified_delta(""))

    def test_sum_verified_deltas_across_commits(self):
        repo = self.make_repo()
        sha1 = self.commit_file(repo, "a.txt", "1", "fix a", trailers=[("Verified", "recheck-pass gaps=3->1")])
        sha2 = self.commit_file(repo, "b.txt", "1", "fix b", trailers=[("Verified", "recheck-pass gaps=2->0")])
        total = overlord_sweep.sum_verified_deltas(repo, [sha1, sha2], overlord_sweep.default_run_git)
        self.assertEqual(total, 4)

    def test_missing_verified_trailer_contributes_zero(self):
        repo = self.make_repo()
        sha = self.commit_file(repo, "a.txt", "1", "fix a, no trailer")
        total = overlord_sweep.sum_verified_deltas(repo, [sha], overlord_sweep.default_run_git)
        self.assertEqual(total, 0)


# ---------------------------------------------------------------------------
# evaluate_post_merge (pure)
# ---------------------------------------------------------------------------

class EvaluatePostMergeTests(unittest.TestCase):
    def test_delta_meets_claim_passes(self):
        pre = {"JPEG": {"gap_count": 10}}
        post = {"JPEG": {"gap_count": 8}}
        ok, delta, problems = overlord_sweep.evaluate_post_merge(pre, post, 2)
        self.assertTrue(ok)
        self.assertEqual(delta, 2)
        self.assertEqual(problems, [])

    def test_over_delivery_passes_not_a_failure(self):
        pre = {"JPEG": {"gap_count": 10}, "NEF": {"gap_count": 5}}
        post = {"JPEG": {"gap_count": 5}, "NEF": {"gap_count": 5}}
        ok, delta, _problems = overlord_sweep.evaluate_post_merge(pre, post, 3)
        self.assertTrue(ok)
        self.assertEqual(delta, 5)

    def test_negative_component_fails(self):
        pre = {"JPEG": {"gap_count": 10}}
        post = {"JPEG": {"gap_count": 10}}
        ok, delta, problems = overlord_sweep.evaluate_post_merge(pre, post, 2)
        self.assertFalse(ok)
        self.assertEqual(delta, 0)
        self.assertTrue(any("sum(Verified)" in p for p in problems))

    def test_duplicate_emission_fails_even_with_a_good_delta(self):
        pre = {"JPEG": {"gap_count": 10, "extra_in_oxidex": []}}
        post = {"JPEG": {"gap_count": 5, "duplicate_emissions": ["JPEG:Foo"], "extra_in_oxidex": []}}
        ok, _delta, problems = overlord_sweep.evaluate_post_merge(pre, post, 2)
        self.assertFalse(ok)
        self.assertTrue(any("duplicate_emissions" in p for p in problems))

    def test_new_oxidex_only_fails(self):
        pre = {"JPEG": {"gap_count": 10, "extra_in_oxidex": []}}
        post = {"JPEG": {"gap_count": 5, "extra_in_oxidex": [{"family": "F", "name": "N"}]}}
        ok, _delta, problems = overlord_sweep.evaluate_post_merge(pre, post, 2)
        self.assertFalse(ok)
        self.assertTrue(any("new_oxidex_only" in p for p in problems))


# ---------------------------------------------------------------------------
# Mechanical bisection
# ---------------------------------------------------------------------------

class BisectSweepFailureTests(GitRepoTestCase):
    @staticmethod
    def _fake_comparison():
        def comparison_fn(repo_root, cache_dir, fmt, suffix):
            repo_root = Path(repo_root)
            canon_fixed = (repo_root / "canon_fixed.marker").exists()
            nikon_fixed = (repo_root / "nikon_fixed.marker").exists()
            bad = (repo_root / "duplicate.marker").exists()
            gap_count = 10
            if canon_fixed:
                gap_count -= 3
            if nikon_fixed:
                gap_count -= 2
            return {
                "gap_count": gap_count,
                "duplicate_emissions": ["JPEG:Dup"] if bad else [],
                "extra_in_oxidex": [],
            }
        return comparison_fn

    @staticmethod
    def _checkout_fn(repo_root, ref):
        git(repo_root, "checkout", "--detach", ref)

    def test_isolates_the_offending_squad_and_keeps_the_good_one(self):
        repo = self.make_repo()

        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(repo, "canon_fixed.marker", "1", "canon fix",
                                     trailers=[("Verified", "recheck-pass gaps=13->10")])
        git(repo, "checkout", "-q", "main")

        git(repo, "branch", "squad/nikon", "main")
        git(repo, "checkout", "-q", "squad/nikon")
        self.commit_file(repo, "nikon_fixed.marker", "1", "nikon fix (looks fine)",
                         trailers=[("Verified", "recheck-pass gaps=10->8")])
        nikon_sha = self.commit_file(repo, "duplicate.marker", "1", "nikon dup side effect")
        git(repo, "checkout", "-q", "main")

        overlord_sweep.cut_fresh_sweep_branch(repo, "sweep/tags-x-1", overlord_sweep.default_run_git, origin_ref="main")
        info_canon = overlord_sweep.merge_squad_into_sweep(repo, "canon", canon_sha, overlord_sweep.default_run_git)
        info_nikon = overlord_sweep.merge_squad_into_sweep(repo, "nikon", nikon_sha, overlord_sweep.default_run_git)
        self.assertTrue(info_canon["ok"] and info_nikon["ok"])

        merge_infos = {"canon": info_canon, "nikon": info_nikon}
        verified_deltas = {"canon": 3, "nikon": 2}
        quarantine_path = self.tmp / "quarantine.jsonl"
        logged = []

        result = overlord_sweep.bisect_sweep_failure(
            repo_root=repo, merge_infos=merge_infos, formats=["JPEG"], cache_dir="/unused",
            comparison_fn=self._fake_comparison(), checkout_fn=self._checkout_fn, base_ref="main",
            verified_deltas=verified_deltas, quarantine_path=quarantine_path,
            run_git=overlord_sweep.default_run_git, log_fn=logged.append,
        )

        self.assertEqual(result["offenders"], ["nikon"])
        self.assertEqual(result["surviving_squads"], ["canon"])
        self.assertTrue((repo / "canon_fixed.marker").exists())
        self.assertFalse((repo / "nikon_fixed.marker").exists())
        self.assertFalse((repo / "duplicate.marker").exists())

        quarantine_entries = squad_merge_loop.load_quarantine(quarantine_path)
        self.assertEqual(len(quarantine_entries), 2)  # nikon's two commits
        for entry in quarantine_entries.values():
            self.assertEqual(entry["squad"], "nikon")


# ---------------------------------------------------------------------------
# Judgment-queue classification
# ---------------------------------------------------------------------------

class ClassifyForJudgmentQueueTests(GitRepoTestCase):
    def test_clean_commit_ships_mechanically(self):
        repo = self.make_repo()
        self.commit_file(repo, "src/parsers/jpeg/x.rs", "fn foo() {}\n", "base file")
        sha = self.commit_file(repo, "src/parsers/jpeg/x.rs", "fn foo() { 1 }\n", "boring change")
        reasons = overlord_sweep.classify_for_judgment_queue(sha, repo, overlord_sweep.default_run_git)
        self.assertEqual(reasons, [])

    def test_value_map_change_is_flagged(self):
        repo = self.make_repo()
        self.commit_file(repo, "src/parsers/jpeg/x.rs", "// base\n", "base file")
        sha = self.commit_file(
            repo, "src/parsers/jpeg/x.rs",
            '// base\nconst TABLE: &[(u16, &str)] = &[\n    (1, "Economy"),\n];\n',
            "add printconv table",
        )
        reasons = overlord_sweep.classify_for_judgment_queue(sha, repo, overlord_sweep.default_run_git)
        self.assertTrue(any("value-map" in r for r in reasons))

    def test_new_file_is_flagged(self):
        repo = self.make_repo()
        sha = self.new_file_commit(repo, "src/parsers/jpeg/new_module.rs", "// new\n", "add new module")
        reasons = overlord_sweep.classify_for_judgment_queue(sha, repo, overlord_sweep.default_run_git)
        self.assertTrue(any("new file" in r for r in reasons))

    def test_new_top_level_parse_fn_is_flagged(self):
        repo = self.make_repo()
        self.commit_file(repo, "src/parsers/jpeg/x.rs", "// base\n", "base file")
        sha = self.commit_file(
            repo, "src/parsers/jpeg/x.rs", "// base\npub fn parse_new_thing() {}\n", "add parse fn",
        )
        reasons = overlord_sweep.classify_for_judgment_queue(sha, repo, overlord_sweep.default_run_git)
        self.assertTrue(any("parse_" in r for r in reasons))

    def test_tests_directory_touch_is_flagged(self):
        repo = self.make_repo()
        sha = self.commit_file(repo, "tests/jpeg_test.rs", "#[test]\nfn t() {}\n", "add test")
        reasons = overlord_sweep.classify_for_judgment_queue(sha, repo, overlord_sweep.default_run_git)
        self.assertTrue(any("tests/fixtures" in r for r in reasons))

    def test_review_unverifiable_trailer_is_flagged(self):
        repo = self.make_repo()
        sha = self.commit_file(
            repo, "src/parsers/jpeg/x.rs", "fn foo() {}\n", "fix",
            trailers=[("Review-Unverifiable", "C1")],
        )
        reasons = overlord_sweep.classify_for_judgment_queue(sha, repo, overlord_sweep.default_run_git)
        self.assertTrue(any("UNVERIFIABLE" in r for r in reasons))

    def test_commons_file_touch_is_flagged(self):
        repo = self.make_repo()
        sha = self.commit_file(repo, "src/core/format_dispatch.rs", "fn dispatch() {}\n", "touch commons")
        reasons = overlord_sweep.classify_for_judgment_queue(sha, repo, overlord_sweep.default_run_git)
        self.assertTrue(any("commons" in r for r in reasons))

    def test_commons_prefix_touch_is_flagged(self):
        repo = self.make_repo()
        sha = self.commit_file(
            repo, "src/parsers/tiff/makernotes/shared/util.rs", "fn util() {}\n", "touch shared makernotes util",
        )
        reasons = overlord_sweep.classify_for_judgment_queue(sha, repo, overlord_sweep.default_run_git)
        self.assertTrue(any("commons" in r for r in reasons))


# ---------------------------------------------------------------------------
# PR evidence table
# ---------------------------------------------------------------------------

class BuildEvidenceRowsTests(GitRepoTestCase):
    def test_builds_one_row_per_tag_with_sample_count(self):
        repo = self.make_repo()
        sha1 = self.commit_file(
            repo, "a.txt", "1", "fix a",
            trailers=[
                ("Format", "JPEG"), ("Tag", "MakerNotes:Foo"), ("Sample", "s1.jpg"),
                ("Exiftool-Value", "5"), ("Oxidex-Value", "5"),
            ],
        )
        sha2 = self.commit_file(
            repo, "b.txt", "1", "fix a again on another sample",
            trailers=[
                ("Format", "JPEG"), ("Tag", "MakerNotes:Foo"), ("Sample", "s2.jpg"),
            ],
        )
        rows = overlord_sweep.build_evidence_rows([sha1, sha2], repo, overlord_sweep.default_run_git)
        self.assertEqual(rows["MakerNotes:Foo"]["exiftool_value"], "5")
        self.assertEqual(rows["MakerNotes:Foo"]["oxidex_value"], "5")
        self.assertEqual(rows["MakerNotes:Foo"]["sample_count"], 2)

    def test_commit_with_no_tag_trailer_contributes_no_row(self):
        repo = self.make_repo()
        sha = self.commit_file(repo, "a.txt", "1", "no trailers here")
        rows = overlord_sweep.build_evidence_rows([sha], repo, overlord_sweep.default_run_git)
        self.assertEqual(rows, {})

    def test_render_evidence_table_is_a_markdown_table(self):
        rows = {"JPEG:Foo": {"exiftool_value": "5", "oxidex_value": "6", "sample_count": 2}}
        table = overlord_sweep.render_evidence_table(rows)
        self.assertIn("| Tag | Exiftool-Value | Oxidex-Value | Sample count |", table)
        self.assertIn("| JPEG:Foo | 5 | 6 | 2 |", table)

    def test_render_judgment_queue_section_empty_case(self):
        text = overlord_sweep.render_judgment_queue_section([])
        self.assertIn("ships mechanically", text)

    def test_render_judgment_queue_section_lists_reasons(self):
        text = overlord_sweep.render_judgment_queue_section([("abc123def456", ["touches a commons file"])])
        self.assertIn("abc123def456"[:12], text)
        self.assertIn("touches a commons file", text)

    def test_the_section_does_not_claim_a_review_gate_that_does_not_exist(self):
        """It said "flagged for human judgment-queue review before
        merge". Nothing enforces that: judgment_entries is interpolated
        into this prose and echoed in run_sweep's return dict, and the
        string "judgment" does not appear anywhere in
        parallel_model_fix_loop -- auto_publish_round branches only on
        run_sweep's status and on pr_checks_state, so a flagged commit
        squash-merges on green like any other. Measured 2026-07-26: BOTH
        sweep PRs that have ever landed (#124 -> 4f3eb99 and #130 ->
        a2aa0df) carry "touches a value-map/PrintConv-like table", i.e.
        turning this into a hard merge gate today would have published
        NOTHING. Until the classifier's precision is fixed, the prose has
        to describe what actually happens.
        """
        text = overlord_sweep.render_judgment_queue_section(
            [("abc123def456", ["touches a value-map/PrintConv-like table"])],
        )
        self.assertNotIn("before merge", text)
        self.assertIn("advisory", text.lower())


# ---------------------------------------------------------------------------
# Step 7b: cargo fmt before push (the measured PR #124 CI failure)
# ---------------------------------------------------------------------------

class ReattachSweepBranchTests(GitRepoTestCase):
    """The re-attach is what makes every commit after step 5 land on the
    branch instead of on an orphaned detached HEAD. Its FAILURE path had
    no coverage at all, so "a terminal reattach_failed status that
    refuses to push" rested on assertion; these pin it, including the
    no-discard invariant that matters most -- it must refuse rather than
    force-move a ref."""

    def test_happy_path_attaches_head_to_the_branch(self):
        repo = self.make_repo()
        git(repo, "checkout", "-q", "-b", "sweep/tags-2026-07-26-1")
        self.commit_file(repo, "src/a.rs", "fn a() {}\n", "fix")
        tip = git_out(repo, "rev-parse", "HEAD").strip()
        git(repo, "checkout", "-q", "--detach", tip)

        ok, message = overlord_sweep.reattach_sweep_branch(
            repo, "sweep/tags-2026-07-26-1", overlord_sweep.default_run_git,
        )
        self.assertTrue(ok, message)
        # Symbolically attached, not merely pointing at the same sha.
        self.assertEqual(
            git_out(repo, "rev-parse", "--abbrev-ref", "HEAD").strip(),
            "sweep/tags-2026-07-26-1",
        )

    def test_a_commit_made_while_detached_is_carried_onto_the_branch(self):
        repo = self.make_repo()
        git(repo, "checkout", "-q", "-b", "sweep/tags-2026-07-26-1")
        self.commit_file(repo, "src/a.rs", "fn a() {}\n", "fix")
        branch_before = git_out(repo, "rev-parse", "sweep/tags-2026-07-26-1").strip()
        git(repo, "checkout", "-q", "--detach", branch_before)
        # This is the orphan the real bug produced: bisection's revert, or
        # step 7b's cargo-fmt commit.
        self.commit_file(repo, "src/a.rs", "fn a() { }\n", "style: cargo fmt")
        orphan = git_out(repo, "rev-parse", "HEAD").strip()
        self.assertNotEqual(branch_before, orphan)

        ok, _message = overlord_sweep.reattach_sweep_branch(
            repo, "sweep/tags-2026-07-26-1", overlord_sweep.default_run_git,
        )
        self.assertTrue(ok)
        self.assertEqual(
            git_out(repo, "rev-parse", "sweep/tags-2026-07-26-1").strip(), orphan
        )

    def test_a_missing_branch_ref_fails_instead_of_creating_one(self):
        repo = self.make_repo()
        ok, message = overlord_sweep.reattach_sweep_branch(
            repo, "sweep/tags-2026-07-26-9", overlord_sweep.default_run_git,
        )
        self.assertFalse(ok)
        self.assertIn("no longer exists", message)
        rc, _out, _err = overlord_sweep.default_run_git(
            ["rev-parse", "--verify", "--quiet", "refs/heads/sweep/tags-2026-07-26-9"], repo,
        )
        self.assertNotEqual(rc, 0, "must not have created the branch it could not find")

    def test_a_branch_that_is_not_an_ancestor_is_refused_not_force_moved(self):
        # The no-discard invariant. If HEAD somehow diverged, moving the
        # ref would silently drop whatever the branch carried.
        repo = self.make_repo()
        git(repo, "checkout", "-q", "-b", "sweep/tags-2026-07-26-1")
        self.commit_file(repo, "src/a.rs", "fn a() {}\n", "branch-only work")
        branch_tip = git_out(repo, "rev-parse", "sweep/tags-2026-07-26-1").strip()
        git(repo, "checkout", "-q", "--detach", "main")
        self.commit_file(repo, "src/b.rs", "fn b() {}\n", "divergent work")

        ok, message = overlord_sweep.reattach_sweep_branch(
            repo, "sweep/tags-2026-07-26-1", overlord_sweep.default_run_git,
        )
        self.assertFalse(ok)
        self.assertIn("not an ancestor", message)
        self.assertEqual(
            git_out(repo, "rev-parse", "sweep/tags-2026-07-26-1").strip(),
            branch_tip,
            "the branch ref must be exactly where it was",
        )


class FormatSweepBranchTests(GitRepoTestCase):
    """cargo fmt itself is injected (a tempdir repo has no Cargo.toml and
    hermetic tests never shell out to a real cargo); what is exercised
    for real is the git half -- what gets staged, whether a commit is
    created at all, and that the commit is a separate, labelled one."""

    def test_commits_when_fmt_changed_a_rust_file(self):
        repo = self.make_repo()
        self.commit_file(repo, "src/a.rs", "fn a( ) {}\n", "add a.rs")
        before = git_out(repo, "rev-parse", "HEAD").strip()

        def fake_fmt(repo_root):
            (Path(repo_root) / "src" / "a.rs").write_text("fn a() {}\n")
            return True, ""

        result = overlord_sweep.format_sweep_branch(
            repo, overlord_sweep.default_run_git, fmt_fn=fake_fmt, log_fn=lambda *a: None,
        )
        self.assertTrue(result["ok"])
        self.assertTrue(result["committed"])
        head = git_out(repo, "rev-parse", "HEAD").strip()
        self.assertNotEqual(head, before)
        self.assertIn("style: cargo fmt --all", git_out(repo, "log", "-1", "--format=%s"))
        # Exactly one new commit, and it carries only the reformatted file.
        self.assertEqual(git_out(repo, "log", f"{before}..HEAD", "--format=%H").split(), [head])
        self.assertEqual(git_out(repo, "show", "--name-only", "--format=", "HEAD").split(), ["src/a.rs"])

    def test_no_commit_when_the_branch_is_already_fmt_clean(self):
        repo = self.make_repo()
        self.commit_file(repo, "src/a.rs", "fn a() {}\n", "add a.rs")
        before = git_out(repo, "rev-parse", "HEAD").strip()

        result = overlord_sweep.format_sweep_branch(
            repo, overlord_sweep.default_run_git, fmt_fn=lambda repo_root: (True, ""),
            log_fn=lambda *a: None,
        )
        self.assertTrue(result["ok"])
        self.assertFalse(result["committed"])
        self.assertEqual(git_out(repo, "rev-parse", "HEAD").strip(), before)

    def test_untracked_junk_in_the_worktree_never_rides_along(self):
        # `git add -u -- '*.rs'`: a comparison report or editor dropping
        # sitting in the sweep worktree must never end up inside a commit
        # labelled "cargo fmt".
        repo = self.make_repo()
        self.commit_file(repo, "src/a.rs", "fn a( ) {}\n", "add a.rs")

        def fake_fmt(repo_root):
            (Path(repo_root) / "src" / "a.rs").write_text("fn a() {}\n")
            (Path(repo_root) / "tagcmp-JPEG-sweep-post.json").write_text("{}")
            return True, ""

        overlord_sweep.format_sweep_branch(
            repo, overlord_sweep.default_run_git, fmt_fn=fake_fmt, log_fn=lambda *a: None,
        )
        self.assertEqual(git_out(repo, "show", "--name-only", "--format=", "HEAD").split(), ["src/a.rs"])
        self.assertIn("tagcmp-JPEG-sweep-post.json", git_out(repo, "status", "--porcelain"))

    def test_a_failing_cargo_fmt_is_reported_but_commits_nothing(self):
        repo = self.make_repo()
        before = git_out(repo, "rev-parse", "HEAD").strip()
        logged = []
        result = overlord_sweep.format_sweep_branch(
            repo, overlord_sweep.default_run_git,
            fmt_fn=lambda repo_root: (False, "error: no rustfmt component"), log_fn=logged.append,
        )
        self.assertFalse(result["ok"])
        self.assertFalse(result["committed"])
        self.assertEqual(git_out(repo, "rev-parse", "HEAD").strip(), before)
        self.assertTrue(any("cargo fmt --all FAILED" in line for line in logged))


# ---------------------------------------------------------------------------
# run_sweep -- end to end with everything side-effectful injected
# ---------------------------------------------------------------------------

class RunSweepIntegrationTests(GitRepoTestCase):
    def _squads_toml(self, tmp, squads):
        path = tmp / "squads.toml"
        body = []
        for squad in squads:
            body.append(f'[squads.{squad}]\nmodules = []\nformats = ["JPEG"]\nownership_globs = []\n')
        path.write_text("\n".join(body))
        return path

    @staticmethod
    def _passing_comparison_fn(repo_root, cache_dir, fmt, suffix):
        # "sweep-pre" (origin/main baseline) vs "sweep-post" (merged
        # sweep tip): a gap_count drop of 1, matching the fixture
        # commits' own "Verified: recheck-pass gaps=3->2" trailer, so
        # the post-merge recheck's delta inequality actually passes.
        gap_count = 5 if suffix == "sweep-pre" else 4
        return {"gap_count": gap_count, "duplicate_emissions": [], "extra_in_oxidex": []}

    @staticmethod
    def _checkout_fn(repo_root, ref):
        git(repo_root, "checkout", "--detach", ref)

    @staticmethod
    def _reformatting_fmt_fn(repo_root):
        """An fmt_fn that behaves like the real `cargo fmt --all`: it
        REWRITES a tracked .rs file.

        `lambda repo_root: (True, "")` -- what these integration tests
        injected until 2026-07-26 -- changes no file, so
        format_sweep_branch short-circuits at "already cargo-fmt clean"
        and never reaches its `git commit`. That made the orphaned-fmt-
        commit defect (the fmt commit landing on the recheck's DETACHED
        HEAD instead of on the sweep branch) completely invisible here.
        """
        for path in sorted(Path(repo_root).rglob("*.rs")):
            text = path.read_text()
            if "( )" in text:
                path.write_text(text.replace("( )", "()"))
        return True, ""

    def test_no_news_short_circuits_before_cutting_a_branch(self):
        repo = self.make_repo()
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=self._passing_comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=home / "sweep-state.json", origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
            )
        self.assertEqual(result["status"], "no_news")

    def test_full_pass_creates_a_pr_with_an_evidence_table_and_calls_out_judgment_entries(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "src/parsers/jpeg/x.rs", "fn fixed( ) {}\n", "fix JPEG:Foo",
            trailers=[
                ("Format", "JPEG"), ("Tag", "MakerNotes:Foo"), ("Sample", "s1.jpg"),
                ("Exiftool-Value", "5"), ("Oxidex-Value", "5"), ("Verified", "recheck-pass gaps=3->2"),
                ("Worker", "canon-1"),
            ],
        )
        git(repo, "checkout", "-q", "main")

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            status_path = squad_merge_loop.squad_status_file(home, "canon")
            squad_merge_loop.record_head(
                status_path, "workerheadsha", status="consumed", patch_id="p1", format_name="JPEG",
                squad_sha=canon_sha, now_fn=lambda: 100,
            )

            pr_calls = []
            pushed = []

            def fake_create_pr(title, body, branch, base):
                pr_calls.append({"title": title, "body": body, "branch": branch, "base": base})
                return {"ok": True, "url": "https://example/pr/1"}

            def fake_push(repo_root, branch):
                # What origin would actually receive: the BRANCH ref, not
                # whatever local HEAD happens to be. Recorded at push time
                # so the assertions below can compare against the exact
                # commit `gh pr create --head <branch>` would open a PR on.
                pushed.append(git_out(repo_root, "rev-parse", branch).strip())
                return True, "pushed"

            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=self._passing_comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=home / "sweep-state.json", origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                create_pr_fn=fake_create_pr, push_branch_fn=fake_push,
                # A tempdir repo has no Cargo.toml -- inject the fmt step
                # (step 7b, which sits between the workspace test and the
                # push) so this stays hermetic instead of shelling out to
                # a real cargo. It must REWRITE a tracked .rs file, exactly
                # as rustfmt does: a no-op hook produces no commit and so
                # cannot show whether that commit lands on the branch.
                fmt_fn=self._reformatting_fmt_fn,
                now_fn=lambda: 12345,
            )

            # Cursor advances for a fully durable ("ok") squad -- a later
            # sweep with no further news from canon reports no_news, not
            # a re-processing of the same stamp.
            cursor = overlord_sweep.load_sweep_state(home / "sweep-state.json")

        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["merged_squads"], ["canon"])
        self.assertEqual(len(pr_calls), 1)
        body = pr_calls[0]["body"]
        self.assertIn("MakerNotes:Foo", body)
        self.assertIn("Judgment queue", body)
        self.assertEqual(pr_calls[0]["branch"], result["branch"])
        self.assertIn("canon", cursor["squads"])

        # The fmt commit must be ON THE BRANCH at push time. The pre/post
        # recheck ends by checking out sweep_tip DETACHED, so without an
        # explicit re-attach every commit made after it (the fmt commit,
        # and any bisection revert) lands on a detached HEAD and the
        # branch ref -- the only thing `git push origin <branch>` and
        # `gh pr create --head <branch>` ever see -- stays behind.
        branch = result["branch"]
        self.assertTrue(result["fmt"]["committed"])
        self.assertEqual(git_out(repo, "rev-parse", "HEAD").strip(),
                         git_out(repo, "rev-parse", branch).strip())
        self.assertIn("style: cargo fmt --all (sweep publish)",
                      git_out(repo, "log", branch, "--format=%s").splitlines())
        self.assertEqual(git_out(repo, "show", f"{branch}:src/parsers/jpeg/x.rs"), "fn fixed() {}\n")
        self.assertEqual(pushed, [git_out(repo, "rev-parse", branch).strip()])

    def test_a_bisected_sweep_pushes_the_branch_with_the_offender_actually_reverted(self):
        """The same detached-HEAD defect as the fmt commit, one step
        earlier: bisect_sweep_failure's `git revert` commits also run
        AFTER the pre/post recheck has detached HEAD. Without a re-attach
        the branch ref still points at the pre-bisection merge tip, so
        the PR ships the quarantined squad's regression -- the exact
        thing bisection exists to keep out."""
        repo = self.make_repo()

        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "canon_fixed.marker", "1", "canon fix",
            trailers=[("Format", "JPEG"), ("Tag", "JPEG:Good"), ("Verified", "recheck-pass gaps=13->10")],
        )
        git(repo, "checkout", "-q", "main")

        git(repo, "branch", "squad/nikon", "main")
        git(repo, "checkout", "-q", "squad/nikon")
        self.commit_file(
            repo, "nikon_fixed.marker", "1", "nikon fix (looks fine)",
            trailers=[("Format", "JPEG"), ("Tag", "JPEG:Bad"), ("Verified", "recheck-pass gaps=10->8")],
        )
        nikon_sha = self.commit_file(repo, "duplicate.marker", "1", "nikon dup side effect")
        git(repo, "checkout", "-q", "main")

        def comparison_fn(repo_root, cache_dir, fmt, suffix):
            repo_root = Path(repo_root)
            gap_count = 10 - 3 * (repo_root / "canon_fixed.marker").exists() \
                - 2 * (repo_root / "nikon_fixed.marker").exists()
            return {
                "gap_count": gap_count,
                "duplicate_emissions": ["JPEG:Dup"] if (repo_root / "duplicate.marker").exists() else [],
                "extra_in_oxidex": [],
            }

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon", "nikon"])
            for squad, sha in (("canon", canon_sha), ("nikon", nikon_sha)):
                squad_merge_loop.record_head(
                    squad_merge_loop.squad_status_file(home, squad), f"{squad}head", status="consumed",
                    patch_id=f"p-{squad}", format_name="JPEG", squad_sha=sha, now_fn=lambda: 100,
                )
            pushed = []

            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=home / "sweep-state.json", origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                quarantine_path=home / "quarantine.jsonl",
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                create_pr_fn=lambda *a, **kw: {"ok": True, "url": "https://example/pr/2"},
                push_branch_fn=lambda repo_root, branch: (
                    pushed.append(git_out(repo_root, "rev-parse", branch).strip()) or (True, "pushed")
                ),
                fmt_fn=self._reformatting_fmt_fn, log_fn=lambda *a: None,
            )

        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["bisection"]["offenders"], ["nikon"])
        branch = result["branch"]
        # What origin receives must be the post-bisection tree.
        listing = git_out(repo, "ls-tree", "-r", "--name-only", branch).split()
        self.assertIn("canon_fixed.marker", listing)
        self.assertNotIn("nikon_fixed.marker", listing)
        self.assertNotIn("duplicate.marker", listing)
        self.assertEqual(pushed, [git_out(repo, "rev-parse", branch).strip()])

    def test_workspace_test_failure_blocks_pr_creation(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "x.txt", "1", "fix",
            trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:Foo"), ("Verified", "recheck-pass gaps=3->2")],
        )
        git(repo, "checkout", "-q", "main")

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            status_path = squad_merge_loop.squad_status_file(home, "canon")
            squad_merge_loop.record_head(
                status_path, "workerheadsha", status="consumed", patch_id="p1", format_name="JPEG",
                squad_sha=canon_sha, now_fn=lambda: 100,
            )
            pr_calls = []
            sweep_state_path = home / "sweep-state.json"

            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=self._passing_comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=sweep_state_path, origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                cargo_test_workspace_fn=lambda repo_root: (False, "boom"),
                create_pr_fn=lambda *a, **kw: pr_calls.append(1),
            )

            self.assertEqual(result["status"], "workspace_tests_failed")
            self.assertEqual(pr_calls, [])

            # The cursor must NOT advance past canon's stamp here: its
            # commits sit on an abandoned local sweep branch, never a PR
            # -- a later sweep must still find and retry the same stamp
            # (spec M4's "never silently skip"), not report "no_news".
            self.assertEqual(overlord_sweep.load_sweep_state(sweep_state_path), {"squads": {}})
            squads = overlord_sweep.squads_from_toml(squads_toml)
            cursor = overlord_sweep.load_sweep_state(sweep_state_path)
            stamps, _new_cursor = overlord_sweep.collect_green_stamps(home, squads, cursor)
            self.assertIn("canon", stamps)

    def test_branch_cut_failure_leaves_the_cursor_untouched_for_retry(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "x.txt", "1", "fix",
            trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:Foo"), ("Verified", "recheck-pass gaps=3->2")],
        )
        git(repo, "checkout", "-q", "main")

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            status_path = squad_merge_loop.squad_status_file(home, "canon")
            squad_merge_loop.record_head(
                status_path, "workerheadsha", status="consumed", patch_id="p1", format_name="JPEG",
                squad_sha=canon_sha, now_fn=lambda: 100,
            )
            sweep_state_path = home / "sweep-state.json"

            def failing_run_git(args, repo_root, input_text=None):
                if args[:1] == ["branch"]:
                    return 1, "", "simulated same-day branch-name race"
                return overlord_sweep.default_run_git(args, repo_root, input_text)

            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=self._passing_comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=sweep_state_path, origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock", run_git=failing_run_git,
            )

            self.assertEqual(result["status"], "branch_cut_failed")
            # Nothing was persisted at all -- collect_green_stamps must
            # find canon's stamp exactly as before, ready for a clean retry.
            squads = overlord_sweep.squads_from_toml(squads_toml)
            cursor = overlord_sweep.load_sweep_state(sweep_state_path)
            self.assertEqual(cursor, {"squads": {}})
            stamps, _new_cursor = overlord_sweep.collect_green_stamps(home, squads, cursor)
            self.assertIn("canon", stamps)

    def test_nothing_merged_leaves_the_cursor_untouched_for_retry(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "shared.txt", "canon-version", "canon change",
            trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:Foo"), ("Verified", "recheck-pass gaps=3->2")],
        )
        git(repo, "checkout", "-q", "main")
        # Diverge main so canon's merge hard-conflicts (add/add, same
        # path, different content on both sides) -- merge_squad_into_sweep
        # reports a hard error for canon, and with no other squad to
        # merge, the round is "nothing_merged".
        self.commit_file(repo, "shared.txt", "main-version", "main change")

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            status_path = squad_merge_loop.squad_status_file(home, "canon")
            squad_merge_loop.record_head(
                status_path, "workerheadsha", status="consumed", patch_id="p1", format_name="JPEG",
                squad_sha=canon_sha, now_fn=lambda: 100,
            )
            sweep_state_path = home / "sweep-state.json"

            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=self._passing_comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=sweep_state_path, origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
            )

            self.assertEqual(result["status"], "nothing_merged")
            squads = overlord_sweep.squads_from_toml(squads_toml)
            cursor = overlord_sweep.load_sweep_state(sweep_state_path)
            self.assertEqual(cursor, {"squads": {}})
            stamps, _new_cursor = overlord_sweep.collect_green_stamps(home, squads, cursor)
            self.assertIn("canon", stamps)

    def _one_squad_fixture(self, repo, tmpdir):
        """One green-stamped squad on `repo`, its home under `tmpdir`.
        Returns (home, squads_toml, sweep_state_path)."""
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "src/a.rs", "fn a( ) {}\n", "fix JPEG:Foo",
            trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:Foo"), ("Verified", "recheck-pass gaps=3->2")],
        )
        git(repo, "checkout", "-q", "main")
        home = Path(tmpdir) / "home"
        squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
        squad_merge_loop.record_head(
            squad_merge_loop.squad_status_file(home, "canon"), "workerheadsha", status="consumed",
            patch_id="p1", format_name="JPEG", squad_sha=canon_sha, now_fn=lambda: 100,
        )
        return home, squads_toml, home / "sweep-state.json"

    def test_a_failed_gh_pr_create_is_its_own_status_not_ok(self):
        """`gh pr create` fails for entirely routine reasons -- expired
        auth, a secondary rate limit, an org rule that forbids the PR,
        no network. Reporting that round as "ok" made the caller poll
        `gh pr checks <branch>` against a branch with no PR (three
        "unknown" answers, two 30s sleeps) and left an orphan branch on
        origin with the sweep cursor already advanced past it."""
        repo = self.make_repo()
        logged = []
        with tempfile.TemporaryDirectory() as tmpdir:
            home, squads_toml, sweep_state_path = self._one_squad_fixture(repo, tmpdir)
            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=self._passing_comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=sweep_state_path, origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                push_branch_fn=lambda repo_root, branch: (True, "pushed"),
                fmt_fn=self._reformatting_fmt_fn, log_fn=logged.append,
                create_pr_fn=lambda *a, **kw: {
                    "ok": False, "stdout": "",
                    "stderr": "gh: To get started with GitHub CLI, please run: gh auth login",
                },
            )
        self.assertEqual(result["status"], "pr_create_failed")
        self.assertIn("pr", result)
        self.assertTrue(any("gh pr create FAILED" in line for line in logged))

    def test_main_exits_non_zero_when_pr_creation_fails(self):
        # The CLI must surface it too: `uv run overlord_sweep.py` exiting
        # 0 on a round that opened no PR is indistinguishable from a
        # healthy one in any wrapper script or cron log.
        printed = []
        with patch.object(overlord_sweep, "run_sweep",
                          return_value={"status": "pr_create_failed", "branch": "sweep/x",
                                        "pr": {"ok": False, "stderr": "gh auth login"}}):
            with patch("builtins.print", side_effect=lambda *a, **kw: printed.append(" ".join(map(str, a)))):
                rc = overlord_sweep.main(["--repo", str(self.tmp), "--home", str(self.tmp / "home")])
        self.assertEqual(rc, 1)
        # Loud, not a dict buried in the JSON blob: an operator scanning
        # a cron log has to be able to see that no PR exists.
        self.assertTrue(any("PR CREATION FAILED" in line for line in printed), printed)

    def test_push_failure_leaves_the_cursor_untouched_for_retry(self):
        # Until 2026-07-26 the cursor advanced here (this test was named
        # ..._but_cursor_still_advances and asserted the opposite). The
        # rationale for the eager advance -- "re-sweeping already-pushed
        # content would open a SECOND PR racing the first" -- does not
        # apply to a FAILED push: origin has nothing, so a retry cannot
        # duplicate anything, and advancing is pure downside. Measured
        # consequence of the old behaviour: round 1 push_failed consumed
        # the stamp, round 2 with a healthy push reported 'no_news' and
        # the validated fix shipped only if that squad happened to stamp
        # again.
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "x.txt", "1", "fix",
            trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:Foo"), ("Verified", "recheck-pass gaps=3->2")],
        )
        git(repo, "checkout", "-q", "main")

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            status_path = squad_merge_loop.squad_status_file(home, "canon")
            squad_merge_loop.record_head(
                status_path, "workerheadsha", status="consumed", patch_id="p1", format_name="JPEG",
                squad_sha=canon_sha, now_fn=lambda: 100,
            )
            pr_calls = []
            sweep_state_path = home / "sweep-state.json"

            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=self._passing_comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=sweep_state_path, origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                create_pr_fn=lambda *a, **kw: pr_calls.append(1),
                push_branch_fn=lambda repo_root, branch: (False, "no configured push destination"),
                fmt_fn=lambda repo_root: (True, ""),
            )

            self.assertEqual(result["status"], "push_failed")
            self.assertEqual(pr_calls, [])
            cursor = overlord_sweep.load_sweep_state(sweep_state_path)
            self.assertEqual(cursor, {"squads": {}})
            squads = overlord_sweep.squads_from_toml(squads_toml)
            stamps, _new_cursor = overlord_sweep.collect_green_stamps(home, squads, cursor)
            self.assertIn("canon", stamps)

    def test_a_push_failure_is_retried_whole_by_the_next_round(self):
        """The end-to-end consequence, driven twice against one repo: a
        round whose push fails must leave the next round able to sweep
        the identical stamp and actually land it."""
        repo = self.make_repo()
        with tempfile.TemporaryDirectory() as tmpdir:
            home, squads_toml, sweep_state_path = self._one_squad_fixture(repo, tmpdir)
            common = dict(
                repo_root=repo, home=home, cache_dir="/unused",
                comparison_fn=self._passing_comparison_fn, checkout_fn=self._checkout_fn,
                squads_toml_path=squads_toml, sweep_state_path=sweep_state_path, origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                fmt_fn=self._reformatting_fmt_fn, log_fn=lambda *a: None,
            )
            first = overlord_sweep.run_sweep(
                push_branch_fn=lambda repo_root, branch: (False, "fatal: could not read from remote"),
                create_pr_fn=lambda *a, **kw: self.fail("no PR may be created after a failed push"),
                **common,
            )
            pushed = []
            second = overlord_sweep.run_sweep(
                push_branch_fn=lambda repo_root, branch: (
                    pushed.append(git_out(repo_root, "ls-tree", "-r", "--name-only", branch).split())
                    or (True, "pushed")
                ),
                create_pr_fn=lambda *a, **kw: {"ok": True, "url": "https://example/pr/3"},
                **common,
            )
        self.assertEqual(first["status"], "push_failed")
        self.assertEqual(second["status"], "ok")
        # The retry really carries the fix, not just a fresh empty branch.
        self.assertEqual(len(pushed), 1)
        self.assertIn("src/a.rs", pushed[0])

    def test_a_zero_delta_sweep_never_runs_the_workspace_suite_pushes_or_opens_a_pr(self):
        """DURABLE repo rule: compare the TREE, not the SHA. A
        cherry-pick/squash gives identical content a fresh sha, so a
        stamp whose whole contribution is already on origin/main still
        produces a branch that is N commits "ahead" while being
        tree-identical. Until 2026-07-26 that branch paid a full
        `cargo test --workspace`, was pushed, and had a PR opened on it
        (which auto_publish_round then squash-merged on green) -- a
        no-op commit on main plus a wasted CI cycle every time.
        """
        repo = self.make_repo()
        # origin/main already carries the fix's CONTENT under its own sha
        # (what a squash-merge leaves behind).
        self.commit_file(repo, "src/a.rs", "fn a() {}\n", "sweep: the same fix, squashed onto main")
        git(repo, "branch", "squad/canon", "main~1")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "src/a.rs", "fn a() {}\n", "fix JPEG:Foo",
            trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:Foo")],
        )
        git(repo, "checkout", "-q", "main")

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            squad_merge_loop.record_head(
                squad_merge_loop.squad_status_file(home, "canon"), "workerhead", status="consumed",
                patch_id="p1", format_name="JPEG", squad_sha=canon_sha, now_fn=lambda: 100,
            )
            sweep_state_path = home / "sweep-state.json"
            tested, pushed, prs = [], [], []

            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused",
                comparison_fn=self._passing_comparison_fn, checkout_fn=self._checkout_fn,
                squads_toml_path=squads_toml, sweep_state_path=sweep_state_path, origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                cargo_test_workspace_fn=lambda repo_root: tested.append(1) or (True, "ok"),
                push_branch_fn=lambda repo_root, branch: pushed.append(branch) or (True, "pushed"),
                create_pr_fn=lambda *a, **kw: prs.append(a) or {"ok": True, "url": "u"},
                fmt_fn=self._reformatting_fmt_fn, log_fn=lambda *a: None,
            )
            cursor = overlord_sweep.load_sweep_state(sweep_state_path)

        self.assertEqual(result["status"], "zero_delta")
        self.assertEqual(tested, [])
        self.assertEqual(pushed, [])
        self.assertEqual(prs, [])
        # The tree really was identical -- the branch was "ahead" by a
        # commit, which is exactly why a sha comparison cannot catch it.
        branch = result["branch"]
        self.assertEqual(git_out(repo, "rev-parse", f"{branch}^{{tree}}").strip(),
                         git_out(repo, "rev-parse", "main^{tree}").strip())
        self.assertNotEqual(git_out(repo, "rev-parse", branch).strip(),
                            git_out(repo, "rev-parse", "main").strip())
        # Advancing here is correct and required: the content IS on main,
        # so re-collecting the stamp forever would spin every round.
        self.assertIn("canon", cursor["squads"])

    def test_a_stamp_already_an_ancestor_of_origin_main_is_zero_delta_too(self):
        # The variant real `gh pr create` rejects outright ("No commits
        # between main and ..."), after the round has already burned a
        # full workspace suite and two comparison runs.
        repo = self.make_repo()
        landed = self.commit_file(repo, "src/a.rs", "fn a() {}\n", "fix already on main")
        git(repo, "branch", "squad/canon", landed)

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            squad_merge_loop.record_head(
                squad_merge_loop.squad_status_file(home, "canon"), "workerhead", status="consumed",
                patch_id="p1", format_name="JPEG", squad_sha=landed, now_fn=lambda: 100,
            )
            tested, pushed = [], []
            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused",
                comparison_fn=self._passing_comparison_fn, checkout_fn=self._checkout_fn,
                squads_toml_path=squads_toml, sweep_state_path=home / "sweep-state.json",
                origin_ref="main", dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                cargo_test_workspace_fn=lambda repo_root: tested.append(1) or (True, "ok"),
                push_branch_fn=lambda repo_root, branch: pushed.append(branch) or (True, "pushed"),
                create_pr_fn=lambda *a, **kw: self.fail("no PR for a zero-delta sweep"),
                fmt_fn=self._reformatting_fmt_fn, log_fn=lambda *a: None,
            )
        self.assertEqual(result["status"], "zero_delta")
        self.assertEqual(tested, [])
        self.assertEqual(pushed, [])


class BisectionMustNotShipWhatItRejectedTests(GitRepoTestCase):
    """DEFECT 1: bisect_sweep_failure INFERRED "the offender is no longer
    on the branch" from `offenders`/`surviving_squads` instead of
    establishing it mechanically. Both of its loops treat a FAILED
    `git revert` as "carry on" -- the isolation loop `continue`s and the
    full-abort loop just logs -- so the squad stays in `surviving`,
    run_sweep's `if not merge_infos:` abort gate never fires, and the
    round returns status 'ok' with the rejected content still on the
    branch it pushes. auto_publish_round then squash-merges it on green.

    Both revert-failure triggers were verified as raw git behaviour by
    the refuter: an empty-diff revert (`git revert --no-edit -m 1 <merge>`
    exits 1 with EMPTY stderr when the squad's content is already on
    origin/main under another sha) and a genuine revert CONFLICT (two
    squads touching overlapping regions of a shared emitter file merge
    cleanly but do not revert cleanly -- the likelier trigger, since the
    full-abort loop is only reached on a real multi-squad interaction).
    Both are stood in for here by failing exactly the `revert` calls and
    leaving every other git operation real.
    """

    @staticmethod
    def _checkout_fn(repo_root, ref):
        git(repo_root, "checkout", "--detach", ref)

    def _squads_toml(self, tmp, squads):
        path = tmp / "squads.toml"
        path.write_text("\n".join(
            f'[squads.{s}]\nmodules = []\nformats = ["JPEG"]\nownership_globs = []\n' for s in squads
        ))
        return path

    def _failing_comparison_fn(self, repo_root, cache_dir, fmt, suffix):
        """A duplicate emission the sweep INTRODUCES and can never clear,
        because the only way to clear it is to revert -- and reverting fails.

        It must be absent from the PRE report. The gate diffs pre against
        post (a sweep is answerable for what it introduces, not for what
        origin/main already carries), so a duplicate present on BOTH sides is
        inherited and correctly does not block. This fixture used to return
        ["JPEG:Dup"] for both, which described a pre-existing duplicate while
        the docstring claimed an introduced one -- the test passed only
        because the gate read POST alone."""
        pre = suffix == "sweep-pre"
        return {"gap_count": 5 if pre else 4,
                "duplicate_emissions": [] if pre else ["JPEG:Dup"],
                "extra_in_oxidex": []}

    def _inherited_dup_comparison_fn(self, repo_root, cache_dir, fmt, suffix):
        """The SAME duplicate on both sides: origin/main already had it."""
        return {"gap_count": 5 if suffix == "sweep-pre" else 4,
                "duplicate_emissions": ["JPEG:Inherited"], "extra_in_oxidex": []}

    def test_a_PRE_EXISTING_duplicate_does_not_veto_the_sweep(self):
        """This is the LAST gate before a sweep PR is opened.

        Reading duplicate_emissions off POST alone let any duplicate already
        on origin/main veto publication outright -- NEF carries nine. That is
        one reason no sweep PR had ever opened. A sweep is answerable for the
        duplicates it INTRODUCES.
        """
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "src/a.rs", "fn a() {}\n", "fix JPEG:Foo",
            trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:Foo"),
                      ("Verified", "recheck-pass gaps=3->2")],
        )
        git(repo, "checkout", "-q", "main")
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            squad_merge_loop.record_head(
                squad_merge_loop.squad_status_file(home, "canon"), "workerhead",
                status="consumed", patch_id="p1", format_name="JPEG",
                squad_sha=canon_sha, now_fn=lambda: 100,
            )
            pushed, prs = [], []
            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused",
                comparison_fn=self._inherited_dup_comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=home / "sweep-state.json", origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                quarantine_path=home / "quarantine.jsonl",
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                push_branch_fn=lambda repo_root, branch: pushed.append(branch) or (True, "pushed"),
                create_pr_fn=lambda *a, **kw: prs.append(a) or {"ok": True, "url": "u"},
                fmt_fn=lambda repo_root: (True, ""), log_fn=lambda *a: None,
            )
        # It must NOT abort on a duplicate it inherited.
        self.assertNotEqual(result["status"], "sweep_aborted")
        self.assertEqual(pushed, ["sweep/tags-2026-07-27-1"][:len(pushed)] or [])
        self.assertTrue(pushed, "an inherited duplicate must not stop the sweep from pushing")

    def test_an_unrevertable_offender_refuses_to_push_instead_of_shipping(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        git(repo, "checkout", "-q", "squad/canon")
        canon_sha = self.commit_file(
            repo, "src/a.rs", "fn a() {}\n", "fix JPEG:Foo",
            trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:Foo"),
                      ("Verified", "recheck-pass gaps=3->2")],
        )
        git(repo, "checkout", "-q", "main")

        def revert_hostile_run_git(args, repo_root, input_text=None):
            if args[:1] == ["revert"]:
                # Exactly what the measured empty-diff revert does: rc 1,
                # nothing on stderr (the message goes to stdout).
                return 1, "nothing to commit, working tree clean\n", ""
            return overlord_sweep.default_run_git(args, repo_root, input_text)

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon"])
            squad_merge_loop.record_head(
                squad_merge_loop.squad_status_file(home, "canon"), "workerhead", status="consumed",
                patch_id="p1", format_name="JPEG", squad_sha=canon_sha, now_fn=lambda: 100,
            )
            sweep_state_path = home / "sweep-state.json"
            pushed, prs, tested = [], [], []

            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused",
                comparison_fn=self._failing_comparison_fn, checkout_fn=self._checkout_fn,
                squads_toml_path=squads_toml, sweep_state_path=sweep_state_path, origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                quarantine_path=home / "quarantine.jsonl",
                cargo_test_workspace_fn=lambda repo_root: tested.append(1) or (True, "ok"),
                push_branch_fn=lambda repo_root, branch: pushed.append(branch) or (True, "pushed"),
                create_pr_fn=lambda *a, **kw: prs.append(a) or {"ok": True, "url": "u"},
                fmt_fn=lambda repo_root: (True, ""), run_git=revert_hostile_run_git,
                log_fn=lambda *a: None,
            )
            cursor = overlord_sweep.load_sweep_state(sweep_state_path)

        self.assertNotEqual(result["status"], "ok")
        self.assertEqual(result["status"], "sweep_aborted")
        self.assertEqual(result["bisection"]["unrevertable"], ["canon"])
        # Nothing that bisection could not remove may reach origin.
        self.assertEqual(pushed, [])
        self.assertEqual(prs, [])
        self.assertEqual(tested, [])
        # And the squad is neither quarantined nor consumed -- a later
        # sweep must retry it once the revert can succeed.
        self.assertEqual(cursor, {"squads": {}})

    def test_a_bisection_that_left_the_recheck_failing_refuses_to_push(self):
        """The other half of the same inference: the isolation loop can
        reach `found = True` on a path where the LAST recheck it ran
        FAILED -- squad A's revert succeeds, the recheck still fails
        (A was innocent), and then `undo_last_revert` fails, so A is
        quarantined as "the offender" while the real offender B stays on
        the branch. `offenders`/`surviving` look tidy; the branch is
        known-bad. It must not be pushed."""
        repo = self.make_repo()
        for squad, path, content in (("canon", "src/a.rs", "fn a() {}\n"),
                                     ("nikon", "src/b.rs", "fn b() {}\n")):
            git(repo, "branch", f"squad/{squad}", "main")
            git(repo, "checkout", "-q", f"squad/{squad}")
            sha = self.commit_file(
                repo, path, content, f"{squad} fix",
                trailers=[("Format", "JPEG"), ("Tag", f"JPEG:{squad}"),
                          ("Verified", "recheck-pass gaps=3->2")],
            )
            git(repo, "checkout", "-q", "main")
            setattr(self, f"{squad}_sha", sha)

        def comparison_fn(repo_root, cache_dir, fmt, suffix):
            # nikon's file is the regression, and it is never reverted
            # (the only revert that happens is canon's).
            bad = (Path(repo_root) / "src" / "b.rs").exists()
            return {"gap_count": 5 if suffix == "sweep-pre" else 4,
                    "duplicate_emissions": ["JPEG:Dup"] if bad else [], "extra_in_oxidex": []}

        def restore_hostile_run_git(args, repo_root, input_text=None):
            # `undo_last_revert`'s exact argv -- `revert --no-edit <sha>`
            # with no -m -- and nothing else.
            if args[:2] == ["revert", "--no-edit"] and "-m" not in args:
                return 1, "", "error: could not revert the revert"
            return overlord_sweep.default_run_git(args, repo_root, input_text)

        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            squads_toml = self._squads_toml(Path(tmpdir), ["canon", "nikon"])
            for squad in ("canon", "nikon"):
                squad_merge_loop.record_head(
                    squad_merge_loop.squad_status_file(home, squad), f"{squad}head",
                    status="consumed", patch_id=f"p-{squad}", format_name="JPEG",
                    squad_sha=getattr(self, f"{squad}_sha"), now_fn=lambda: 100,
                )
            pushed, prs = [], []
            result = overlord_sweep.run_sweep(
                repo_root=repo, home=home, cache_dir="/unused", comparison_fn=comparison_fn,
                checkout_fn=self._checkout_fn, squads_toml_path=squads_toml,
                sweep_state_path=home / "sweep-state.json", origin_ref="main",
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                quarantine_path=home / "quarantine.jsonl",
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                push_branch_fn=lambda repo_root, branch: pushed.append(branch) or (True, "pushed"),
                create_pr_fn=lambda *a, **kw: prs.append(a) or {"ok": True, "url": "u"},
                fmt_fn=lambda repo_root: (True, ""), run_git=restore_hostile_run_git,
                log_fn=lambda *a: None,
            )

        self.assertNotEqual(result["status"], "ok")
        self.assertFalse(result["bisection"]["recheck_passed"])
        self.assertEqual(pushed, [])
        self.assertEqual(prs, [])


if __name__ == "__main__":
    unittest.main()
