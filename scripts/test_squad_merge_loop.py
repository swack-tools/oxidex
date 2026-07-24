"""Hermetic tests for squad_merge_loop.py.

Real `git` is exercised against throwaway tempdir repos (matching
test_validate_fix_commit.py's style: the merge/cherry-pick mechanics are
exactly what needs a real git, everything else -- validate_fix_commit,
cargo_test_targeted, the tag-comparison recheck -- is injected). No test
runs a real cargo build/test, touches the network, or reads/writes the
real ~/.oxidex.

This host's global git config sets commit.gpgsign=true and a custom
core.hooksPath (confirmed via `git config --global --list`), either of
which would make a hermetic cherry-pick/commit hang or run unexpected
hooks -- GitRepoTestCase masks GIT_CONFIG_GLOBAL/GIT_CONFIG_SYSTEM for
the whole test via env patching, which also covers squad_merge_loop.py's
own internal git calls (its `_git` helper doesn't take an env override,
so it inherits process os.environ at call time).
"""
import json
import os
import signal
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import squad_merge_loop as sml

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

    def commit_file(self, repo, rel_path, content, message):
        path = repo / rel_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", message)
        return git_out(repo, "rev-parse", "HEAD").strip()


# ---------------------------------------------------------------------------
# Singleton lock (mirrors distill_lessons LockTests -- reused helpers)
# ---------------------------------------------------------------------------

class RunLockedTests(GitRepoTestCase):
    def _write_lock(self, home, squad, pid=4242, sha="test-sha", heartbeat_ts=0.0):
        path = sml.merger_lock_path(home, squad)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"pid": pid, "script_git_sha": sha, "heartbeat_ts": heartbeat_ts}))
        return path

    def test_fresh_same_sha_holder_exits_without_calling_fn(self):
        home = self.tmp / "home"
        self._write_lock(home, "nikon", heartbeat_ts=1000.0)
        calls = []
        result = sml.run_locked(
            home, "nikon", lambda hb: calls.append(1), now_fn=lambda: 1010.0,
            kill_fn=lambda *a: self.fail("must not kill a fresh same-sha holder"),
            script_sha="test-sha", pid=9999,
        )
        self.assertEqual(result["status"], "already_running")
        self.assertEqual(calls, [])

    def test_stale_heartbeat_takes_over_and_runs_fn(self):
        home = self.tmp / "home"
        lock_path = self._write_lock(home, "nikon", pid=4242, heartbeat_ts=0.0)
        killed = []
        result = sml.run_locked(
            home, "nikon", lambda hb: "ran", now_fn=lambda: 10_000.0,
            kill_fn=lambda pid, sig: killed.append((pid, sig)),
            script_sha="test-sha", pid=9999,
        )
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["result"], "ran")
        self.assertEqual(killed, [(4242, signal.SIGTERM)])
        self.assertFalse(lock_path.exists())  # released cleanly on exit

    def test_sha_mismatch_takes_over_even_when_heartbeat_is_fresh(self):
        home = self.tmp / "home"
        self._write_lock(home, "nikon", pid=4242, sha="outdated-sha", heartbeat_ts=990.0)
        killed = []
        result = sml.run_locked(
            home, "nikon", lambda hb: "ran", now_fn=lambda: 1000.0,
            kill_fn=lambda pid, sig: killed.append((pid, sig)),
            script_sha="current-sha", pid=9999,
        )
        self.assertEqual(result["status"], "ok")
        self.assertEqual(killed, [(4242, signal.SIGTERM)])

    def test_dead_holder_pid_is_tolerated(self):
        home = self.tmp / "home"
        self._write_lock(home, "nikon", pid=4242, heartbeat_ts=0.0)

        def kill_dead(pid, sig):
            raise ProcessLookupError(pid)

        result = sml.run_locked(home, "nikon", lambda hb: "ran", now_fn=lambda: 10_000.0,
                                kill_fn=kill_dead, script_sha="test-sha", pid=9999)
        self.assertEqual(result["status"], "ok")

    def test_heartbeat_callback_refreshes_the_lock_file(self):
        home = self.tmp / "home"

        def fn(heartbeat):
            heartbeat()
            data = json.loads(sml.merger_lock_path(home, "nikon").read_text())
            self.assertEqual(data["pid"], 9999)
            self.assertEqual(data["heartbeat_ts"], 500.0)
            return "ok"

        result = sml.run_locked(home, "nikon", fn, now_fn=lambda: 500.0,
                                script_sha="sha-x", pid=9999)
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["result"], "ok")


# ---------------------------------------------------------------------------
# squads.toml + candidate branch discovery
# ---------------------------------------------------------------------------

class CandidateDiscoveryTests(GitRepoTestCase):
    def _squads_toml(self):
        path = self.tmp / "squads.toml"
        path.write_text(
            '[squads.nikon]\nformats = ["NEF", "JPEG"]\n'
            '[squads.canon]\nformats = ["CR2"]\n'
        )
        return path

    def test_squad_formats_reads_the_advisory_list(self):
        self.assertEqual(sml.squad_formats(self._squads_toml(), "nikon"), ["NEF", "JPEG"])

    def test_unknown_squad_raises(self):
        with self.assertRaises(ValueError):
            sml.squad_formats(self._squads_toml(), "nonexistent")

    def test_candidate_branches_keeps_only_existing_ones(self):
        repo = self.make_repo()
        git(repo, "branch", "model-fix-parallel-nef")
        # model-fix-parallel-jpeg deliberately never created
        branches = sml.candidate_worker_branches(repo, self._squads_toml(), "nikon")
        self.assertEqual(branches, [("NEF", "model-fix-parallel-nef")])

    def test_no_candidate_branches_when_none_exist(self):
        repo = self.make_repo()
        self.assertEqual(sml.candidate_worker_branches(repo, self._squads_toml(), "nikon"), [])


class SquadSlotBranchDiscoveryTests(GitRepoTestCase):
    """Squad-mode dispatch (spec S2) creates model-fix-parallel-<squad>-<n>
    branches -- a DIFFERENT naming scheme from candidate_worker_branches's
    legacy per-format branches. Without squad_slot_branches, a squad's
    merger would never discover these at all (the Phase 3 squad pipeline
    disconnected end-to-end from its own claimed consumer)."""

    def test_finds_slot_branches_for_this_squad_only(self):
        repo = self.make_repo()
        git(repo, "branch", "model-fix-parallel-canon-1")
        git(repo, "branch", "model-fix-parallel-canon-2")
        git(repo, "branch", "model-fix-parallel-nikon-1")
        branches = sml.squad_slot_branches(repo, "canon")
        self.assertEqual(
            sorted(branches), ["model-fix-parallel-canon-1", "model-fix-parallel-canon-2"],
        )

    def test_no_slot_branches_when_none_exist(self):
        repo = self.make_repo()
        self.assertEqual(sml.squad_slot_branches(repo, "canon"), [])

    def test_does_not_match_a_legacy_per_format_branch(self):
        repo = self.make_repo()
        git(repo, "branch", "model-fix-parallel-canon")  # legacy: no "-<n>" slot suffix
        self.assertEqual(sml.squad_slot_branches(repo, "canon"), [])


class CommitFormatTrailerTests(GitRepoTestCase):
    def test_reads_the_format_trailer(self):
        repo = self.make_repo()
        sha = self.commit_file(repo, "a.rs", "1\n", "fix a\n\nFormat: JPEG\nTag: MakerNotes:Foo\n")
        self.assertEqual(sml.commit_format_trailer(repo, sha), "JPEG")

    def test_missing_trailer_is_none(self):
        repo = self.make_repo()
        sha = self.commit_file(repo, "a.rs", "1\n", "fix a, no trailers here")
        self.assertIsNone(sml.commit_format_trailer(repo, sha))


class EnsureSquadBranchTests(GitRepoTestCase):
    def test_creates_branch_from_origin_ref_when_missing(self):
        repo = self.make_repo()
        branch = sml.ensure_squad_branch(repo, "canon", origin_ref="main", log_fn=lambda *a: None)
        self.assertEqual(branch, "squad/canon")
        self.assertTrue(sml.branch_exists(repo, "squad/canon"))

    def test_does_not_create_a_worktree(self):
        repo = self.make_repo()
        sml.ensure_squad_branch(repo, "canon", origin_ref="main", log_fn=lambda *a: None)
        result = git(repo, "worktree", "list", "--porcelain")
        # Only the main checkout's own worktree entry should exist --
        # ensure_squad_branch must never call `git worktree add`.
        self.assertEqual(result.stdout.count("worktree "), 1)

    def test_does_not_recreate_an_existing_branch(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon", "main")
        sha_before = git_out(repo, "rev-parse", "squad/canon").strip()
        self.commit_file(repo, "new.txt", "1", "advance main")
        sml.ensure_squad_branch(repo, "canon", origin_ref="main", log_fn=lambda *a: None)
        self.assertEqual(git_out(repo, "rev-parse", "squad/canon").strip(), sha_before)


# ---------------------------------------------------------------------------
# Patch-id novelty + candidate commit filtering
# ---------------------------------------------------------------------------

class NoveltyAndCandidateCommitsTests(GitRepoTestCase):
    def test_candidate_commits_oldest_first_and_filters_recorded_heads(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/nikon")
        git(repo, "checkout", "-q", "-b", "model-fix-parallel-nef")
        c1 = self.commit_file(repo, "a.rs", "1\n", "fix a")
        c2 = self.commit_file(repo, "b.rs", "1\n", "fix b")

        shas = sml.new_commits_since(repo, "squad/nikon", "model-fix-parallel-nef")
        self.assertEqual(shas, [c1, c2])

        status = {"heads": {c1: {"status": "consumed"}}}
        self.assertEqual(
            sml.candidate_commits(repo, "model-fix-parallel-nef", "squad/nikon", status), [c2],
        )

    def test_union_novel_shas_excludes_a_patch_already_applied_elsewhere_by_id(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/nikon")
        git(repo, "checkout", "-q", "-b", "model-fix-parallel-nef")
        c1 = self.commit_file(repo, "a.rs", "one\n", "fix a")
        c2 = self.commit_file(repo, "b.rs", "one\n", "fix b")

        # c1's PATCH already landed on squad/nikon under a different sha
        # (cherry-picked directly) -- patch-id novelty must catch this
        # even though c1 itself was never merged by that sha.
        git(repo, "checkout", "-q", "squad/nikon")
        git(repo, "cherry-pick", c1)
        git(repo, "checkout", "-q", "model-fix-parallel-nef")

        novel = sml.union_novel_shas(repo, "model-fix-parallel-nef", "main", "squad/nikon")
        self.assertEqual(novel, {c2})

    def test_union_novel_shas_excludes_a_patch_already_on_origin_main(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/nikon")
        git(repo, "checkout", "-q", "-b", "model-fix-parallel-nef")
        c1 = self.commit_file(repo, "a.rs", "one\n", "fix a")

        git(repo, "checkout", "-q", "main")
        git(repo, "cherry-pick", c1)  # lands on "origin/main" stand-in directly
        git(repo, "checkout", "-q", "model-fix-parallel-nef")

        novel = sml.union_novel_shas(repo, "model-fix-parallel-nef", "main", "squad/nikon")
        self.assertEqual(novel, set())

    def test_is_patch_novel_against_single_commit(self):
        repo = self.make_repo()
        c1 = self.commit_file(repo, "a.rs", "one\n", "fix a")
        git(repo, "checkout", "-q", "-b", "other", "main~1")
        self.assertTrue(sml.is_patch_novel_against(repo, "other", c1))
        git(repo, "checkout", "-q", "main")
        self.assertFalse(sml.is_patch_novel_against(repo, "main", c1))


# ---------------------------------------------------------------------------
# Quarantine ledger
# ---------------------------------------------------------------------------

class QuarantineLedgerTests(GitRepoTestCase):
    def test_append_and_load_roundtrip(self):
        path = self.tmp / "quarantine.jsonl"
        entry = sml.append_quarantine(
            path, patch_id="p1", sha="s1", format_name="NEF", squad="nikon",
            reason="bad", flags=["x"], now_fn=lambda: 1000,
        )
        self.assertEqual(entry["attempt"], 1)
        loaded = sml.load_quarantine(path)
        self.assertIn("p1", loaded)
        self.assertEqual(loaded["p1"]["attempt"], 1)
        self.assertEqual(loaded["p1"]["flags"], ["x"])

    def test_attempt_counter_increments_and_backoff_grows(self):
        path = self.tmp / "quarantine.jsonl"
        e1 = sml.append_quarantine(path, patch_id="p1", sha="s1", format_name="NEF",
                                   squad="nikon", reason="bad", flags=[], now_fn=lambda: 1000)
        entries = sml.load_quarantine(path)
        e2 = sml.append_quarantine(path, patch_id="p1", sha="s2", format_name="NEF", squad="nikon",
                                   reason="bad again", flags=[], quarantine_entries=entries,
                                   now_fn=lambda: 2000)
        self.assertEqual(e2["attempt"], 2)
        self.assertGreater(e2["backoff_seconds"], e1["backoff_seconds"])

    def test_malformed_lines_are_skipped_never_raised(self):
        path = self.tmp / "quarantine.jsonl"
        path.write_text("not json at all\n" + json.dumps({"patch_id": "p1", "attempt": 1}) + "\n")
        entries = sml.load_quarantine(path)
        self.assertEqual(set(entries), {"p1"})

    def test_missing_file_is_empty_not_an_error(self):
        self.assertEqual(sml.load_quarantine(self.tmp / "nope.jsonl"), {})

    def test_dedup_keeps_latest_attempt_per_patch_id(self):
        path = self.tmp / "quarantine.jsonl"
        path.write_text(
            json.dumps({"patch_id": "p1", "attempt": 1, "reason": "first"}) + "\n"
            + json.dumps({"patch_id": "p1", "attempt": 2, "reason": "second"}) + "\n"
        )
        entries = sml.load_quarantine(path)
        self.assertEqual(len(entries), 1)
        self.assertEqual(entries["p1"]["reason"], "second")


# ---------------------------------------------------------------------------
# Squad-status ledger
# ---------------------------------------------------------------------------

class SquadStatusTests(GitRepoTestCase):
    def test_record_and_load_roundtrip(self):
        path = self.tmp / "status" / "nikon.json"
        sml.record_head(path, "sha1", status="consumed", patch_id="p1", format_name="NEF",
                        squad_sha="sha1prime", now_fn=lambda: 1000)
        data = sml.load_squad_status(path)
        self.assertEqual(data["heads"]["sha1"]["status"], "consumed")
        self.assertEqual(data["heads"]["sha1"]["squad_sha"], "sha1prime")
        self.assertTrue(data["heads"]["sha1"]["work_done"])

    def test_missing_or_corrupt_file_reads_as_no_news(self):
        self.assertEqual(sml.load_squad_status(self.tmp / "nope.json"), {"heads": {}})
        corrupt = self.tmp / "corrupt.json"
        corrupt.write_text("{ broken json")
        self.assertEqual(sml.load_squad_status(corrupt), {"heads": {}})

    def test_invalid_status_value_raises(self):
        with self.assertRaises(ValueError):
            sml.record_head(self.tmp / "x.json", "sha", status="bogus", patch_id="p", format_name="NEF")

    def test_writes_never_leave_a_torn_tempfile_behind(self):
        path = self.tmp / "status" / "nikon.json"
        for i in range(5):
            sml.record_head(path, f"sha{i}", status="consumed", patch_id=f"p{i}",
                            format_name="NEF", now_fn=lambda: 1000)
        leftovers = [p.name for p in path.parent.iterdir() if p.name != path.name]
        self.assertEqual(leftovers, [])
        data = sml.load_squad_status(path)
        self.assertEqual(len(data["heads"]), 5)


# ---------------------------------------------------------------------------
# ensure_staging_worktree
# ---------------------------------------------------------------------------

class EnsureStagingWorktreeTests(GitRepoTestCase):
    def test_creates_branch_and_worktree_when_absent(self):
        repo = self.make_repo()
        staging = self.tmp / "staging-nikon"
        branch = sml.ensure_staging_worktree(repo, staging, "nikon", origin_ref="main", log_fn=lambda *a: None)
        self.assertEqual(branch, "squad/nikon")
        self.assertTrue(staging.is_dir())
        current = git_out(staging, "rev-parse", "--abbrev-ref", "HEAD").strip()
        self.assertEqual(current, "squad/nikon")

    def test_reuses_existing_worktree_in_place_and_cleans_it(self):
        repo = self.make_repo()
        staging = self.tmp / "staging-nikon"
        sml.ensure_staging_worktree(repo, staging, "nikon", origin_ref="main", log_fn=lambda *a: None)
        (staging / "stray.txt").write_text("uncommitted junk")

        branch = sml.ensure_staging_worktree(repo, staging, "nikon", origin_ref="main", log_fn=lambda *a: None)

        self.assertEqual(branch, "squad/nikon")
        self.assertFalse((staging / "stray.txt").exists())

    def test_does_not_recreate_the_branch_if_it_already_exists(self):
        repo = self.make_repo()
        git(repo, "checkout", "-q", "-b", "squad/nikon")
        extra = self.commit_file(repo, "extra.txt", "x\n", "extra commit on squad branch")
        git(repo, "checkout", "-q", "main")
        staging = self.tmp / "staging-nikon"
        sml.ensure_staging_worktree(repo, staging, "nikon", origin_ref="main", log_fn=lambda *a: None)
        self.assertEqual(git_out(repo, "rev-parse", "squad/nikon").strip(), extra)

    def test_recovers_from_a_stuck_mid_cherry_pick_unmerged_index(self):
        # Simulates a SIGTERM landing between a failed `git cherry-pick`
        # and its own `--abort` call (e.g. the lock-takeover path or
        # stop_parallel_fix.py's merger-pgid reaping killing the daemon
        # mid-subprocess): the staging worktree is left with an UNMERGED
        # index and CHERRY_PICK_HEAD still set. A plain `checkout -- .`
        # cannot clean that up -- only `reset --hard` can -- so the next
        # poll must recover on its own, not crash forever.
        repo = self.make_repo()
        self.commit_file(repo, "f.txt", "base\n", "seed f.txt")
        git(repo, "branch", "squad/nikon")
        staging = self.tmp / "staging-nikon"
        git(repo, "worktree", "add", str(staging), "squad/nikon")
        self.commit_file(staging, "f.txt", "squad change\n", "squad change")

        # A conflicting commit from an unrelated lineage off the same base.
        src = self.tmp / "src"
        git(repo, "worktree", "add", str(src), "-b", "worker", "squad/nikon~1")
        conflict_sha = self.commit_file(src, "f.txt", "conflicting change\n", "conflicting change")

        conflict = git(staging, "cherry-pick", conflict_sha, check=False)
        self.assertNotEqual(conflict.returncode, 0)  # conflict, deliberately left un-aborted
        dirty = git(staging, "status", "--porcelain", check=False).stdout
        self.assertIn("UU", dirty)  # unmerged index, confirming the simulated crash state

        branch = sml.ensure_staging_worktree(repo, staging, "nikon", origin_ref="main", log_fn=lambda *a: None)

        self.assertEqual(branch, "squad/nikon")
        clean = git(staging, "status", "--porcelain", check=False)
        self.assertEqual(clean.stdout.strip(), "")
        current = git_out(staging, "rev-parse", "--abbrev-ref", "HEAD").strip()
        self.assertEqual(current, "squad/nikon")


# ---------------------------------------------------------------------------
# process_commit: the full per-commit pipeline
# ---------------------------------------------------------------------------

class ProcessCommitTests(GitRepoTestCase):
    def _setup_squad(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/nikon")
        git(repo, "checkout", "-q", "-b", "model-fix-parallel-nef")
        message = (
            "fix(nef): wire tag\n\n"
            "Format: NEF\n"
            "Tag: EXIF:LensModel\n"
            "Worker: nikon-1\n"
        )
        sha = self.commit_file(repo, "src/nef.rs", "fixed\n", message)
        git(repo, "checkout", "-q", "main")
        staging = self.tmp / "staging-nikon"
        git(repo, "worktree", "add", str(staging), "squad/nikon")
        home = self.tmp / "home"
        return repo, staging, home, sha

    def _process(self, repo, staging, home, sha, **overrides):
        kwargs = dict(
            repo_root=repo, staging_path=staging, squad="nikon", squad_branch="squad/nikon",
            sha=sha, fmt="NEF", is_novel=True, quarantine_entries={}, cache_dir="/unused",
            home=home,
            validate_fn=lambda sha, repo, **kw: {"ok": True, "flags": [], "patch_id": "p1"},
            cargo_test_targeted_fn=lambda *a: (True, ""),
            comparison_fn=lambda *a: {"duplicate_emissions": [], "extra_in_oxidex": []},
            now_fn=lambda: 1000, log_fn=lambda *a: None,
        )
        kwargs.update(overrides)
        return sml.process_commit(**kwargs)

    def test_not_novel_marks_consumed_without_work_and_skips_validate(self):
        repo, staging, home, sha = self._setup_squad()
        called = []
        result = self._process(
            repo, staging, home, sha, is_novel=False,
            validate_fn=lambda *a, **k: called.append(1),
            cargo_test_targeted_fn=lambda *a: self.fail("must not run tests"),
            comparison_fn=lambda *a: self.fail("must not compare"),
        )
        self.assertEqual(result["outcome"], "consumed_no_work")
        self.assertEqual(called, [])
        status = sml.load_squad_status(sml.squad_status_file(home, "nikon"))
        self.assertEqual(status["heads"][sha]["status"], "consumed")
        self.assertFalse(status["heads"][sha]["work_done"])
        self.assertEqual(
            git_out(repo, "rev-parse", "squad/nikon").strip(),
            git_out(repo, "rev-parse", "main").strip(),
        )

    def test_already_quarantined_is_skipped_without_retry_or_reappend(self):
        repo, staging, home, sha = self._setup_squad()
        patch_id = sml.compute_patch_id_for_sha(repo, sha)
        qpath = sml.quarantine_ledger_path(home)
        sml.append_quarantine(qpath, patch_id=patch_id, sha="other-sha", format_name="NEF",
                              squad="nikon", reason="prior rejection", flags=["x"], now_fn=lambda: 1)
        entries = sml.load_quarantine(qpath)
        called = []

        result = self._process(
            repo, staging, home, sha, quarantine_entries=entries,
            validate_fn=lambda *a, **k: called.append(1),
            cargo_test_targeted_fn=lambda *a: self.fail("must not run tests"),
            comparison_fn=lambda *a: self.fail("must not compare"),
        )

        self.assertEqual(result["outcome"], "skipped_quarantined")
        self.assertEqual(called, [])
        self.assertEqual(len(qpath.read_text().splitlines()), 1)

    def test_validate_flags_route_to_quarantine_never_silently_dropped(self):
        repo, staging, home, sha = self._setup_squad()
        before_head = git_out(staging, "rev-parse", "HEAD").strip()
        # process_commit computes its own canonical patch-id up front
        # (needed for the quarantine-lookup-before-validate ordering) --
        # validate_fn's own "patch_id" field is informational only and is
        # not what gets used as the quarantine ledger's key.
        expected_patch_id = sml.compute_patch_id_for_sha(repo, sha)

        result = self._process(
            repo, staging, home, sha,
            validate_fn=lambda *a, **k: {"ok": False, "flags": ["missing-trailer:Verified"], "patch_id": "irrelevant"},
            cargo_test_targeted_fn=lambda *a: self.fail("must not run tests"),
            comparison_fn=lambda *a: self.fail("must not compare"),
        )

        self.assertEqual(result["outcome"], "quarantined")
        self.assertEqual(result["patch_id"], expected_patch_id)
        self.assertIn("missing-trailer:Verified", result["reason"])
        status = sml.load_squad_status(sml.squad_status_file(home, "nikon"))
        self.assertEqual(status["heads"][sha]["status"], "quarantined")
        entries = sml.load_quarantine(sml.quarantine_ledger_path(home))
        self.assertIn(expected_patch_id, entries)
        # staging worktree was never touched -- cherry-pick was never attempted
        self.assertEqual(git_out(staging, "rev-parse", "HEAD").strip(), before_head)

    def test_full_success_publishes_via_detached_head_then_fast_forward_only(self):
        repo, staging, home, sha = self._setup_squad()
        pre_tip = git_out(repo, "rev-parse", "squad/nikon").strip()
        sweep_log = home / "logs" / "sweep-review-history.jsonl"

        result = self._process(repo, staging, home, sha, sweep_review_log_path=sweep_log)

        self.assertEqual(result["outcome"], "consumed")
        new_tip = git_out(repo, "rev-parse", "squad/nikon").strip()
        self.assertNotEqual(new_tip, pre_tip)
        self.assertEqual(new_tip, result["squad_sha"])
        # genuine fast-forward: pre_tip is an ancestor of the new tip
        is_ff = git(repo, "merge-base", "--is-ancestor", pre_tip, new_tip, check=False)
        self.assertEqual(is_ff.returncode, 0)
        # staging worktree is left re-attached to the branch, not detached
        symref = git(staging, "symbolic-ref", "-q", "HEAD", check=False)
        self.assertEqual(symref.returncode, 0)
        self.assertIn("squad/nikon", symref.stdout)
        status = sml.load_squad_status(sml.squad_status_file(home, "nikon"))
        self.assertEqual(status["heads"][sha]["status"], "consumed")
        self.assertEqual(status["heads"][sha]["squad_sha"], new_tip)
        entries = [json.loads(line) for line in sweep_log.read_text().splitlines()]
        self.assertTrue(any(e["verdict_class"] == "machine_accepted" for e in entries))

    def test_targeted_test_failure_never_moves_squad_branch(self):
        repo, staging, home, sha = self._setup_squad()
        pre_tip = git_out(repo, "rev-parse", "squad/nikon").strip()
        comparison_calls = []

        def comparison_fn(*a):
            comparison_calls.append(a)
            return {"duplicate_emissions": [], "extra_in_oxidex": []}

        result = self._process(
            repo, staging, home, sha,
            cargo_test_targeted_fn=lambda *a: (False, "boom"),
            comparison_fn=comparison_fn,
        )

        self.assertEqual(result["outcome"], "quarantined")
        self.assertEqual(len(comparison_calls), 1)  # only the pre-cherry-pick snapshot
        self.assertEqual(git_out(repo, "rev-parse", "squad/nikon").strip(), pre_tip)
        symref = git(staging, "symbolic-ref", "-q", "HEAD", check=False)
        self.assertIn("squad/nikon", symref.stdout)

    def test_duplicate_emission_recheck_quarantines_and_leaves_branch_unchanged(self):
        repo, staging, home, sha = self._setup_squad()
        pre_tip = git_out(repo, "rev-parse", "squad/nikon").strip()
        calls = {"n": 0}

        def comparison_fn(staging_path, cache_dir, fmt, suffix):
            calls["n"] += 1
            if calls["n"] == 1:
                return {"duplicate_emissions": [], "extra_in_oxidex": []}
            return {"duplicate_emissions": ["NEF:Foo"], "extra_in_oxidex": []}

        result = self._process(repo, staging, home, sha, comparison_fn=comparison_fn)

        self.assertEqual(result["outcome"], "quarantined")
        self.assertIn("duplicate_emissions", result["reason"])
        self.assertEqual(git_out(repo, "rev-parse", "squad/nikon").strip(), pre_tip)

    def test_new_oxidex_only_key_recheck_quarantines(self):
        repo, staging, home, sha = self._setup_squad()
        pre_tip = git_out(repo, "rev-parse", "squad/nikon").strip()
        calls = {"n": 0}

        def comparison_fn(staging_path, cache_dir, fmt, suffix):
            calls["n"] += 1
            if calls["n"] == 1:
                return {"duplicate_emissions": [], "extra_in_oxidex": []}
            return {"duplicate_emissions": [], "extra_in_oxidex": [{"family": "NEF", "name": "Bonus"}]}

        result = self._process(repo, staging, home, sha, comparison_fn=comparison_fn)

        self.assertEqual(result["outcome"], "quarantined")
        self.assertIn("new_oxidex_only", result["reason"])
        self.assertEqual(git_out(repo, "rev-parse", "squad/nikon").strip(), pre_tip)

    def test_cherry_pick_conflict_quarantines_and_leaves_worktree_clean(self):
        repo = self.make_repo()
        self.commit_file(repo, "src/nef.rs", "base\n", "seed nef.rs")
        git(repo, "branch", "squad/nikon")
        git(repo, "checkout", "-q", "-b", "model-fix-parallel-nef")
        sha = self.commit_file(repo, "src/nef.rs", "worker change\n", "fix(nef): worker change")
        git(repo, "checkout", "-q", "squad/nikon")
        self.commit_file(repo, "src/nef.rs", "conflicting change\n", "conflicting change on squad branch")
        git(repo, "checkout", "-q", "main")
        staging = self.tmp / "staging-nikon"
        git(repo, "worktree", "add", str(staging), "squad/nikon")
        home = self.tmp / "home"

        result = self._process(
            repo, staging, home, sha,
            cargo_test_targeted_fn=lambda *a: self.fail("must not run tests after a conflict"),
        )

        self.assertEqual(result["outcome"], "quarantined")
        self.assertIn("cherry-pick", result["reason"])
        status = git(staging, "status", "--porcelain", check=False)
        self.assertEqual(status.stdout.strip(), "")


# ---------------------------------------------------------------------------
# Batch full-corpus check
# ---------------------------------------------------------------------------

class BatchCheckCadenceTests(unittest.TestCase):
    def test_due_by_commit_count(self):
        state = {"commits_since": 10, "last_batch_ts": 0}
        self.assertTrue(sml.batch_check_due(state, batch_commits=10, batch_seconds=900, now_fn=lambda: 0))

    def test_due_by_elapsed_seconds(self):
        state = {"commits_since": 0, "last_batch_ts": 0}
        self.assertTrue(sml.batch_check_due(state, batch_commits=10, batch_seconds=900, now_fn=lambda: 901))

    def test_not_due(self):
        state = {"commits_since": 3, "last_batch_ts": 100}
        self.assertFalse(sml.batch_check_due(state, batch_commits=10, batch_seconds=900, now_fn=lambda: 200))


class RunBatchCheckTests(unittest.TestCase):
    def test_passes_and_returns_fresh_baselines(self):
        def comparison_fn(staging, cache, fmt, suffix):
            return {"duplicate_emissions": [], "extra_in_oxidex": [{"family": "NEF", "name": "X"}]}

        ok, problems, baselines = sml.run_batch_check(
            staging_path="/unused", squad="nikon", formats=["NEF"], cache_dir="/unused",
            comparison_fn=comparison_fn, baselines={}, log_fn=lambda *a: None,
        )
        self.assertTrue(ok)
        self.assertEqual(problems, [])
        self.assertIn("NEF", baselines)

    def test_duplicate_emissions_fail_loudly_without_raising(self):
        logged = []

        def comparison_fn(staging, cache, fmt, suffix):
            return {"duplicate_emissions": ["NEF:Foo"], "extra_in_oxidex": []}

        ok, problems, baselines = sml.run_batch_check(
            staging_path="/unused", squad="nikon", formats=["NEF"], cache_dir="/unused",
            comparison_fn=comparison_fn, baselines={}, log_fn=logged.append,
        )
        self.assertFalse(ok)
        self.assertTrue(problems)
        self.assertTrue(any("ERROR" in line for line in logged))

    def test_unexplained_new_oxidex_only_fails(self):
        prior = {"extra_in_oxidex": []}

        def comparison_fn(staging, cache, fmt, suffix):
            return {"duplicate_emissions": [], "extra_in_oxidex": [{"family": "NEF", "name": "New"}]}

        ok, problems, baselines = sml.run_batch_check(
            staging_path="/unused", squad="nikon", formats=["NEF"], cache_dir="/unused",
            comparison_fn=comparison_fn, baselines={"NEF": prior}, log_fn=lambda *a: None,
        )
        self.assertFalse(ok)

    def test_a_format_the_comparison_fn_cannot_find_is_skipped_not_fatal(self):
        ok, problems, baselines = sml.run_batch_check(
            staging_path="/unused", squad="nikon", formats=["NEF"], cache_dir="/unused",
            comparison_fn=lambda *a: None, baselines={}, log_fn=lambda *a: None,
        )
        self.assertTrue(ok)
        self.assertEqual(baselines, {"NEF": None})


class PollOnceBatchIntegrationTests(GitRepoTestCase):
    def _squads_toml(self):
        path = self.tmp / "squads.toml"
        path.write_text('[squads.nikon]\nformats = ["NEF"]\n')
        return path

    def _make_candidate(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/nikon")
        git(repo, "checkout", "-q", "-b", "model-fix-parallel-nef")
        sha = self.commit_file(repo, "src/nef.rs", "fixed\n", "fix(nef): wire tag")
        git(repo, "checkout", "-q", "main")
        return repo, sha

    def test_blocked_publication_skips_processing_until_due(self):
        repo, sha = self._make_candidate()
        home = self.tmp / "home"
        sml.save_batch_state(
            sml.batch_state_path(home, "nikon"),
            {"blocked": True, "commits_since": 0, "last_batch_ts": 1000, "baselines": {}},
        )
        result = sml.poll_once(
            repo_root=repo, squad="nikon", home=home, staging_dir=self.tmp / "staging-nikon",
            squads_toml_path=self._squads_toml(), cache_dir="/unused", origin_ref="main",
            batch_commits=10, batch_seconds=900, now_fn=lambda: 1001,  # not yet due
            check_recut=False, log_fn=lambda *a: None,
        )
        self.assertEqual(result["processed"], [])
        self.assertTrue(result.get("blocked"))
        # the candidate commit is still sitting there, unconsumed
        status = sml.load_squad_status(sml.squad_status_file(home, "nikon"))
        self.assertNotIn(sha, status["heads"])

    def test_blocked_publication_retries_and_clears_when_due_and_now_passing(self):
        repo, sha = self._make_candidate()
        home = self.tmp / "home"
        sml.save_batch_state(
            sml.batch_state_path(home, "nikon"),
            {"blocked": True, "commits_since": 0, "last_batch_ts": 0, "baselines": {}},
        )
        result = sml.poll_once(
            repo_root=repo, squad="nikon", home=home, staging_dir=self.tmp / "staging-nikon",
            squads_toml_path=self._squads_toml(), cache_dir="/unused", origin_ref="main",
            batch_commits=10, batch_seconds=900, now_fn=lambda: 10_000,  # well past due
            validate_fn=lambda sha, repo, **kw: {"ok": True, "flags": [], "patch_id": "p1"},
            cargo_test_targeted_fn=lambda *a: (True, ""),
            comparison_fn=lambda *a: {"duplicate_emissions": [], "extra_in_oxidex": []},
            check_recut=False, log_fn=lambda *a: None,
        )
        self.assertTrue(result["batch_check"]["ok"])
        self.assertEqual(len(result["processed"]), 1)
        self.assertEqual(result["processed"][0]["outcome"], "consumed")
        state = sml.load_batch_state(sml.batch_state_path(home, "nikon"))
        self.assertFalse(state["blocked"])

    def test_batch_seconds_trigger_fires_even_when_this_poll_consumes_nothing(self):
        # spec M2: "every merger_batch_commits commits OR
        # merger_batch_seconds seconds ... whichever first". A poll that
        # consumes zero commits (no candidate worker branches, or every
        # candidate got quarantined) must still run the periodic
        # full-corpus check once batch_seconds has elapsed since the
        # last one -- the seconds arm must not be starved by a quiet
        # squad.
        repo = self.make_repo()  # no worker branches at all -> zero candidates
        home = self.tmp / "home"
        sml.save_batch_state(
            sml.batch_state_path(home, "nikon"),
            {"blocked": False, "commits_since": 0, "last_batch_ts": 0, "baselines": {}},
        )
        batch_check_calls = []

        def comparison_fn(staging, cache, fmt, suffix):
            batch_check_calls.append(fmt)
            return {"duplicate_emissions": [], "extra_in_oxidex": []}

        result = sml.poll_once(
            repo_root=repo, squad="nikon", home=home, staging_dir=self.tmp / "staging-nikon",
            squads_toml_path=self._squads_toml(), cache_dir="/unused", origin_ref="main",
            batch_commits=10, batch_seconds=900, now_fn=lambda: 10_000,  # well past due
            comparison_fn=comparison_fn, check_recut=False, log_fn=lambda *a: None,
        )

        self.assertEqual(result["processed"], [])
        self.assertIsNotNone(result["batch_check"])
        self.assertTrue(result["batch_check"]["ran"])
        self.assertTrue(result["batch_check"]["ok"])
        state = sml.load_batch_state(sml.batch_state_path(home, "nikon"))
        self.assertEqual(state["last_batch_ts"], 10_000)

    def test_batch_check_not_due_yet_with_zero_commits_does_not_run(self):
        repo = self.make_repo()
        home = self.tmp / "home"
        sml.save_batch_state(
            sml.batch_state_path(home, "nikon"),
            {"blocked": False, "commits_since": 0, "last_batch_ts": 9_500, "baselines": {}},
        )
        result = sml.poll_once(
            repo_root=repo, squad="nikon", home=home, staging_dir=self.tmp / "staging-nikon",
            squads_toml_path=self._squads_toml(), cache_dir="/unused", origin_ref="main",
            batch_commits=10, batch_seconds=900, now_fn=lambda: 10_000,  # not yet due
            check_recut=False, log_fn=lambda *a: None,
        )
        self.assertIsNone(result["batch_check"])
        state = sml.load_batch_state(sml.batch_state_path(home, "nikon"))
        self.assertEqual(state["last_batch_ts"], 9_500)  # untouched


class PollOnceSquadSlotBranchIntegrationTests(GitRepoTestCase):
    """Squad-mode dispatch (spec S2) creates model-fix-parallel-<squad>-<n>
    branches, not the legacy model-fix-parallel-<fmt> naming
    candidate_worker_branches looks for -- poll_once must ALSO discover
    and consume these (squad_slot_branches), deriving each candidate
    commit's format from its own Format: trailer since a slot has no
    single fixed format the way a legacy branch does."""

    def _squads_toml(self):
        path = self.tmp / "squads.toml"
        path.write_text('[squads.canon]\nformats = ["JPEG", "CR2"]\n')
        return path

    def test_consumes_a_squad_slot_branch_commit_using_its_format_trailer(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon")
        git(repo, "checkout", "-q", "-b", "model-fix-parallel-canon-1")
        sha = self.commit_file(
            repo, "src/canon.rs", "fixed\n",
            "fix(canon): wire tag\n\nFormat: JPEG\nTag: MakerNotes:Foo\n",
        )
        git(repo, "checkout", "-q", "main")
        home = self.tmp / "home"

        result = sml.poll_once(
            repo_root=repo, squad="canon", home=home, staging_dir=self.tmp / "staging-canon",
            squads_toml_path=self._squads_toml(), cache_dir="/unused", origin_ref="main",
            batch_commits=10, batch_seconds=900, now_fn=lambda: 1,
            validate_fn=lambda sha, repo, **kw: {"ok": True, "flags": [], "patch_id": "p1"},
            cargo_test_targeted_fn=lambda *a: (True, ""),
            comparison_fn=lambda *a: {"duplicate_emissions": [], "extra_in_oxidex": []},
            check_recut=False, log_fn=lambda *a: None,
        )

        self.assertEqual(len(result["processed"]), 1)
        self.assertEqual(result["processed"][0]["outcome"], "consumed")
        status = sml.load_squad_status(sml.squad_status_file(home, "canon"))
        self.assertEqual(status["heads"][sha]["format"], "JPEG")

    def test_legacy_and_slot_branches_are_both_consumed_in_one_poll(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/canon")

        git(repo, "checkout", "-q", "-b", "model-fix-parallel-cr2")
        legacy_sha = self.commit_file(repo, "src/cr2.rs", "fixed\n", "fix(cr2): wire tag")
        git(repo, "checkout", "-q", "main")

        git(repo, "checkout", "-q", "-b", "model-fix-parallel-canon-1")
        slot_sha = self.commit_file(
            repo, "src/canon.rs", "fixed\n", "fix(canon): wire tag\n\nFormat: JPEG\nTag: MakerNotes:Foo\n",
        )
        git(repo, "checkout", "-q", "main")
        home = self.tmp / "home"

        result = sml.poll_once(
            repo_root=repo, squad="canon", home=home, staging_dir=self.tmp / "staging-canon",
            squads_toml_path=self._squads_toml(), cache_dir="/unused", origin_ref="main",
            batch_commits=10, batch_seconds=900, now_fn=lambda: 1,
            validate_fn=lambda sha, repo, **kw: {"ok": True, "flags": [], "patch_id": "p1"},
            cargo_test_targeted_fn=lambda *a: (True, ""),
            comparison_fn=lambda *a: {"duplicate_emissions": [], "extra_in_oxidex": []},
            check_recut=False, log_fn=lambda *a: None,
        )

        consumed_shas = {r["sha"] for r in result["processed"] if r["outcome"] == "consumed"}
        self.assertEqual(consumed_shas, {legacy_sha, slot_sha})


# ---------------------------------------------------------------------------
# Squad branch re-cut
# ---------------------------------------------------------------------------

class ShouldRecutTests(GitRepoTestCase):
    def test_false_when_branch_does_not_exist(self):
        repo = self.make_repo()
        self.assertFalse(sml.should_recut(repo, "squad/nikon", origin_ref="main", now_fn=lambda: 0))

    def test_true_once_older_than_the_staleness_threshold(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/nikon")
        base_sha = git_out(repo, "rev-parse", "main").strip()
        commit_ts = int(git_out(repo, "log", "-1", "--format=%ct", base_sha).strip())
        self.assertTrue(sml.should_recut(
            repo, "squad/nikon", origin_ref="main", staleness_seconds=10,
            now_fn=lambda: commit_ts + 20,
        ))

    def test_false_within_the_staleness_window(self):
        repo = self.make_repo()
        git(repo, "branch", "squad/nikon")
        base_sha = git_out(repo, "rev-parse", "main").strip()
        commit_ts = int(git_out(repo, "log", "-1", "--format=%ct", base_sha).strip())
        self.assertFalse(sml.should_recut(
            repo, "squad/nikon", origin_ref="main", staleness_seconds=1000,
            now_fn=lambda: commit_ts + 20,
        ))


class RecutSquadBranchTests(GitRepoTestCase):
    def test_recut_re_picks_only_still_open_novel_commits(self):
        repo = self.make_repo()
        origin_before = git_out(repo, "rev-parse", "main").strip()

        # A commit whose patch lands on "origin/main" (simulated: a
        # sweep already cherry-picked it there) by the time of the recut.
        git(repo, "checkout", "-q", "-b", "tmp-source")
        landed_orig = self.commit_file(repo, "a.rs", "one\n", "fix: a")
        git(repo, "checkout", "-q", "main")
        git(repo, "cherry-pick", landed_orig)

        # A second commit that never lands on main -- still open.
        git(repo, "checkout", "-q", "tmp-source")
        open_orig = self.commit_file(repo, "b.rs", "two\n", "fix: b")
        git(repo, "checkout", "-q", "main")

        # squad/nikon carries cherry-picked copies of BOTH, from BEFORE
        # main advanced (simulating the merger's own prior work).
        git(repo, "branch", "squad/nikon", origin_before)
        git(repo, "checkout", "-q", "squad/nikon")
        git(repo, "cherry-pick", landed_orig)
        squad_sha_landed = git_out(repo, "rev-parse", "HEAD").strip()
        git(repo, "cherry-pick", open_orig)
        squad_sha_open = git_out(repo, "rev-parse", "HEAD").strip()
        git(repo, "checkout", "-q", "main")

        home = self.tmp / "home"
        status_path = sml.squad_status_file(home, "nikon")
        sml.record_head(status_path, "worker-sha-1", status="consumed", patch_id="p1",
                        format_name="NEF", squad_sha=squad_sha_landed, now_fn=lambda: 1)
        sml.record_head(status_path, "worker-sha-2", status="consumed", patch_id="p2",
                        format_name="NEF", squad_sha=squad_sha_open, now_fn=lambda: 2)

        staging = self.tmp / "staging-nikon"
        git(repo, "worktree", "add", str(staging), "squad/nikon")

        result = sml.recut_squad_branch(
            repo_root=repo, staging_path=staging, squad="nikon", squad_branch="squad/nikon",
            home=home, origin_ref="main", log_fn=lambda *a: None,
        )

        self.assertEqual(result["kept"], ["worker-sha-2"])
        self.assertEqual(result["dropped"], ["worker-sha-1"])
        # exactly one commit ahead of the (now-advanced) main: only the
        # still-open one was re-picked; the landed one comes back "for
        # free" via the fresh base.
        ahead = git_out(repo, "rev-list", "--count", "main..squad/nikon").strip()
        self.assertEqual(ahead, "1")
        merge_base = git_out(repo, "merge-base", "squad/nikon", "main").strip()
        self.assertEqual(merge_base, git_out(repo, "rev-parse", "main").strip())
        # staging worktree left re-attached to the (rebuilt) branch
        symref = git(staging, "symbolic-ref", "-q", "HEAD", check=False)
        self.assertIn("squad/nikon", symref.stdout)

    def test_recut_with_no_recorded_heads_but_untracked_commit_aborts_no_discard(self):
        # spec M5's explicit no-discard invariant ("no ref reset may
        # discard commits not contained in origin/main, an open sweep
        # PR, or a squad staging branch") applies even to a commit that
        # landed on squad/<squad> OUTSIDE this merger's own pipeline
        # (bootstrap/manual cherry-pick) and so was never recorded in
        # squad-status at all -- recut must not silently reset the
        # branch out from under it.
        repo = self.make_repo()
        git(repo, "branch", "squad/nikon")
        # Commit on a throwaway side branch (never on main) so the patch
        # is genuinely absent from origin_ref -- committing straight
        # onto main here would make it trivially "come back for free"
        # and defeat the point of the test.
        git(repo, "checkout", "-q", "-b", "tmp-source")
        extra = self.commit_file(repo, "extra.rs", "x\n", "untracked commit, never recorded in squad-status")
        git(repo, "checkout", "-q", "squad/nikon")
        git(repo, "cherry-pick", extra)
        pre_tip = git_out(repo, "rev-parse", "squad/nikon").strip()
        git(repo, "checkout", "-q", "main")
        staging = self.tmp / "staging-nikon"
        git(repo, "worktree", "add", str(staging), "squad/nikon")
        home = self.tmp / "home"  # no squad-status file at all

        result = sml.recut_squad_branch(
            repo_root=repo, staging_path=staging, squad="nikon", squad_branch="squad/nikon",
            home=home, origin_ref="main", log_fn=lambda *a: None,
        )

        self.assertTrue(result["aborted"])
        self.assertEqual(len(result["lost"]), 1)
        # squad/nikon is left EXACTLY where it was -- the untracked
        # commit is still there, nothing was discarded.
        self.assertEqual(git_out(repo, "rev-parse", "squad/nikon").strip(), pre_tip)
        symref = git(staging, "symbolic-ref", "-q", "HEAD", check=False)
        self.assertIn("squad/nikon", symref.stdout)

    def test_recut_aborts_rather_than_drop_a_consumed_head_on_genuine_conflict(self):
        # A previously green-stamped (consumed) head that genuinely
        # conflicts against the fresh origin_ref must NOT be silently
        # dropped -- that would both violate the no-discard invariant
        # and leave squad-status claiming "consumed" for a commit no
        # longer reachable from squad/<squad> anywhere (which would also
        # mislead the create_worktree consume handshake into treating
        # the original worker-branch head as safe to discard).
        repo = self.make_repo()
        self.commit_file(repo, "f.txt", "base\n", "seed f.txt")
        git(repo, "branch", "squad/nikon")
        git(repo, "checkout", "-q", "squad/nikon")
        squad_sha = self.commit_file(repo, "f.txt", "squad change\n", "worker fix")
        git(repo, "checkout", "-q", "main")
        # origin/main advances with a conflicting edit to the same line.
        self.commit_file(repo, "f.txt", "origin advanced\n", "conflicting origin advance")

        home = self.tmp / "home"
        status_path = sml.squad_status_file(home, "nikon")
        sml.record_head(status_path, "worker-sha-1", status="consumed", patch_id="p1",
                        format_name="NEF", squad_sha=squad_sha, now_fn=lambda: 1)

        staging = self.tmp / "staging-nikon"
        git(repo, "worktree", "add", str(staging), "squad/nikon")

        result = sml.recut_squad_branch(
            repo_root=repo, staging_path=staging, squad="nikon", squad_branch="squad/nikon",
            home=home, origin_ref="main", log_fn=lambda *a: None,
        )

        self.assertTrue(result["aborted"])
        self.assertEqual(result["lost"], [squad_sha])
        # squad/nikon still carries the consumed commit -- untouched.
        self.assertEqual(git_out(repo, "rev-parse", "squad/nikon").strip(), squad_sha)
        ahead = git_out(repo, "log", "main..squad/nikon", "--oneline").strip()
        self.assertNotEqual(ahead, "")
        # squad-status is unchanged; it still (correctly) points at a
        # sha that is still actually present on squad/nikon.
        status = sml.load_squad_status(status_path)
        self.assertEqual(status["heads"]["worker-sha-1"]["squad_sha"], squad_sha)


if __name__ == "__main__":
    unittest.main()
