import json
import os
import signal
import subprocess
import tempfile
import threading
import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch, MagicMock
from pathlib import Path

from model_fix_loop import attempt_foundation_job, git_commit, load_tag_state, save_tag_state

import parallel_model_fix_loop
from parallel_model_fix_loop import (
    _kill_all_active_workers,
    _kill_process_group,
    _process_group_alive,
    _register_pgid,
    _unregister_pgid,
    _wait_for_process_group_exit,
    acquire_dispatcher_lock,
    allocate_squad_slots,
    branch_name,
    clear_held_by_foundation,
    commits_on_branch,
    create_worktree,
    discover_worktree_candidates,
    ensure_integration_branch,
    ensure_squad_staging_branch,
    fast_forward_local_main,
    is_worktree_stale_and_resolved,
    janitor_reset_stale_worktrees,
    main,
    merge_branch,
    novel_commits,
    process_format,
    process_squad_worker,
    prune_model_fix_requests,
    reap_orphan_worker_pgids,
    reset_stale_worktree,
    rotate_dashboard_log,
    run_janitor,
    run_round,
    run_squad_round,
    squad_branch_name,
    squad_from_branch,
    squad_open_gaps_from_attribution,
    squad_worker_formats,
    squad_worktree_path,
    staging_branch_or_origin,
    worktree_path,
)

GIT_ENV_OVERRIDES = {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}


def git(repo, *args, input_text=None, check=True):
    return subprocess.run(
        ["git", *args], cwd=repo, input=input_text, capture_output=True, text=True, check=check,
    )


def git_out(repo, *args, input_text=None):
    return git(repo, *args, input_text=input_text).stdout


class GitRepoTestCase(unittest.TestCase):
    """Real throwaway tempdir git repos -- matches
    test_squad_merge_loop.py's own GitRepoTestCase exactly (masking
    GIT_CONFIG_GLOBAL/GIT_CONFIG_SYSTEM so a host with commit.gpgsign or
    a custom core.hooksPath configured globally can't hang/misbehave a
    hermetic commit)."""

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

    def commit_file(self, repo, rel_path, content, message, when=None):
        """Commit rel_path=content, optionally backdating both author
        and committer dates (when: epoch seconds) -- lets
        is_worktree_stale_and_resolved's staleness check be exercised
        deterministically without a real 3-day wait."""
        path = repo / rel_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        git(repo, "add", "-A")
        env = dict(os.environ)
        if when is not None:
            stamp = f"@{int(when)} +0000"
            env["GIT_AUTHOR_DATE"] = stamp
            env["GIT_COMMITTER_DATE"] = stamp
        subprocess.run(
            ["git", "commit", "-q", "-m", message], cwd=repo, env=env, check=True,
        )
        return git_out(repo, "rev-parse", "HEAD").strip()


class MainInfiniteLoopTests(unittest.TestCase):
    def setUp(self):
        # main() takes the dispatcher singleton flock and reads/clears
        # the persisted-pgids file -- point both at a tempdir so tests
        # never touch (or contend on) the real ~/.oxidex/logs state, and
        # reset the module-level persist path main() sets.
        self._tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmpdir.cleanup)
        self.lock_path = Path(self._tmpdir.name) / "dispatcher.lock"
        self.pgids_path = Path(self._tmpdir.name) / "dispatcher-pgids.json"
        self.addCleanup(parallel_model_fix_loop._set_pgids_persist_path, None)

    def _main(self, argv, **kwargs):
        kwargs.setdefault("lock_path", self.lock_path)
        kwargs.setdefault("pgids_path", self.pgids_path)
        return main(argv, **kwargs)

    def _config_path(self, tmpdir):
        config_path = Path(tmpdir) / "config.toml"
        config_path.write_text('[worker]\nmodels = ["m"]\n')
        return config_path

    def test_runs_a_single_round_by_default(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            calls = []
            exit_code = self._main(
                ["--config", str(config_path)],
                run_round_fn=lambda args, cfg: calls.append(1) or True,
            )
            self.assertEqual(calls, [1])
            self.assertEqual(exit_code, 0)

    def test_single_round_returns_1_when_round_reports_failure(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            exit_code = self._main(
                ["--config", str(config_path)],
                run_round_fn=lambda args, cfg: False,
            )
            self.assertEqual(exit_code, 1)

    def test_infinite_keeps_calling_run_round_fn_until_it_raises(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            calls = []

            def fake_run_round(args, cfg):
                calls.append(1)
                if len(calls) == 3:
                    raise RuntimeError("stop the test loop")
                return True

            with self.assertRaises(RuntimeError):
                self._main(
                    ["--config", str(config_path), "--infinite"],
                    run_round_fn=fake_run_round,
                )
            self.assertEqual(len(calls), 3)

    def test_infinite_sleeps_between_rounds_using_injected_sleep_fn(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            round_calls = []
            sleep_calls = []

            def fake_run_round(args, cfg):
                round_calls.append(1)
                if len(round_calls) == 2:
                    raise RuntimeError("stop the test loop")
                return True

            with self.assertRaises(RuntimeError):
                self._main(
                    ["--config", str(config_path), "--infinite", "--round-delay", "5"],
                    run_round_fn=fake_run_round,
                    sleep_fn=sleep_calls.append,
                )
            # Round 1 succeeds and sleeps; round 2 raises before reaching
            # its own sleep call.
            self.assertEqual(sleep_calls, [5.0])

    def test_infinite_does_not_sleep_when_round_delay_is_zero(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            round_calls = []

            def fake_run_round(args, cfg):
                round_calls.append(1)
                if len(round_calls) == 2:
                    raise RuntimeError("stop the test loop")
                return True

            with self.assertRaises(RuntimeError):
                self._main(
                    ["--config", str(config_path), "--infinite"],
                    run_round_fn=fake_run_round,
                    sleep_fn=lambda s: self.fail("should not sleep when round-delay is 0"),
                )

    def test_missing_config_returns_1_without_running_a_round(self):
        exit_code = self._main(
            ["--config", "/nonexistent/path/config.toml"],
            run_round_fn=lambda args, cfg: self.fail("should not run a round"),
        )
        self.assertEqual(exit_code, 1)

    def test_second_dispatcher_is_refused_while_the_first_holds_the_lock(self):
        # The singleton flock: with the lock file already held (as by a
        # running dispatcher), a second main() must fail fast with exit
        # code 1 and never run a round or touch the pgid file.
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            first_lock = acquire_dispatcher_lock(self.lock_path)
            self.assertIsNotNone(first_lock)
            self.addCleanup(first_lock.close)
            exit_code = self._main(
                ["--config", str(config_path)],
                run_round_fn=lambda args, cfg: self.fail("second dispatcher must not run a round"),
                reap_fn=lambda path: self.fail("second dispatcher must not reap"),
            )
            self.assertEqual(exit_code, 1)

    def test_reaps_recorded_orphans_before_the_first_round(self):
        # Startup order matters: leftovers from a dead dispatcher must be
        # reaped before this dispatcher spawns anything of its own.
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            events = []
            self._main(
                ["--config", str(config_path)],
                run_round_fn=lambda args, cfg: events.append("round") or True,
                reap_fn=lambda path: events.append(("reap", Path(path))),
            )
            self.assertEqual(events, [("reap", self.pgids_path), "round"])


class CreateWorktreeTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.subprocess.run")
    def test_copies_config_toml_into_the_new_worktree(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            config_path = tmp / "config.toml"
            config_path.write_text('[worker]\nmodels = ["m"]\n')
            worktree = tmp / "worktree"
            worktree.mkdir()

            create_worktree(tmp, worktree, "model-fix-parallel-nef", "main", config_path=config_path)

            self.assertEqual((worktree / "config.toml").read_text(), config_path.read_text())

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_missing_config_is_not_an_error(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worktree = tmp / "worktree"
            worktree.mkdir()

            create_worktree(  # must not raise
                tmp, worktree, "model-fix-parallel-nef", "main",
                config_path=tmp / "nonexistent-config.toml",
            )
            self.assertFalse((worktree / "config.toml").exists())

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_uses_git_worktree_add_when_path_does_not_exist(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worktree = tmp / "worktree"  # deliberately not created

            create_worktree(tmp, worktree, "model-fix-parallel-nef", "main", config_path=tmp / "no-config.toml")

            argvs = [c.args[0] for c in mock_run.call_args_list]
            self.assertIn(["git", "worktree", "add", "-b", "model-fix-parallel-nef", str(worktree), "main"], argvs)
            self.assertFalse(any(argv[:2] == ["git", "checkout"] for argv in argvs))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_reuses_an_existing_worktree_in_place_instead_of_recreating_it(self, mock_run):
        def fake_run(argv, **kwargs):
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=1, stdout="", stderr="")  # branch ref gone
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worktree = tmp / "worktree"
            worktree.mkdir()  # simulates a worktree left behind by a prior failed attempt

            create_worktree(tmp, worktree, "model-fix-parallel-nef", "main", config_path=tmp / "no-config.toml")

            argvs = [c.args[0] for c in mock_run.call_args_list]
            # never torn down and recreated -- that would blow away the
            # worktree's own target/ build cache
            self.assertNotIn(
                ["git", "worktree", "add", "-b", "model-fix-parallel-nef", str(worktree), "main"], argvs,
            )
            self.assertIn(["git", "checkout", "--", "."], argvs)
            self.assertIn(["git", "clean", "-fd"], argvs)
            self.assertIn(["git", "checkout", "-B", "model-fix-parallel-nef", "main"], argvs)
            # the clean+reset happened inside the worktree itself, not repo_root
            checkout_dash_b_call = next(c for c in mock_run.call_args_list if c.args[0][:3] == ["git", "checkout", "-B"])
            self.assertEqual(checkout_dash_b_call.kwargs["cwd"], worktree)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_refuses_to_reset_a_reused_branch_that_still_carries_unmerged_commits(self, mock_run):
        # No-discard invariant (M5): a previous round's failed merge left
        # 2 commits on the worker branch that are on neither base_ref nor
        # origin/main -- the reuse path must keep the branch as-is (plain
        # checkout), never `checkout -B` it back onto base_ref.
        def fake_run(argv, **kwargs):
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0, stdout="abc123\n", stderr="")
            if argv[:3] == ["git", "rev-list", "--count"]:
                return MagicMock(returncode=0, stdout="2\n", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worktree = tmp / "worktree"
            worktree.mkdir()

            create_worktree(tmp, worktree, "model-fix-parallel-nef", "main", config_path=tmp / "no-config.toml")

            argvs = [c.args[0] for c in mock_run.call_args_list]
            self.assertNotIn(["git", "checkout", "-B", "model-fix-parallel-nef", "main"], argvs)
            self.assertIn(["git", "checkout", "model-fix-parallel-nef"], argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_resets_a_reused_branch_whose_commits_are_all_contained(self, mock_run):
        # Zero commits outside base_ref/origin/main -> the re-anchor is
        # provably non-destructive, so the normal `checkout -B` runs.
        def fake_run(argv, **kwargs):
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0, stdout="abc123\n", stderr="")
            if argv[:3] == ["git", "rev-list", "--count"]:
                return MagicMock(returncode=0, stdout="0\n", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worktree = tmp / "worktree"
            worktree.mkdir()

            create_worktree(tmp, worktree, "model-fix-parallel-nef", "main", config_path=tmp / "no-config.toml")

            argvs = [c.args[0] for c in mock_run.call_args_list]
            self.assertIn(["git", "checkout", "-B", "model-fix-parallel-nef", "main"], argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_discards_an_orphaned_branch_whose_worktree_directory_is_already_gone(self, mock_run):
        # Simulates /tmp being wiped on reboot: the worktree directory is
        # gone, but the branch ref survives in the repo's own object
        # database -- `git worktree add -b` would otherwise fail outright
        # with "a branch named ... already exists" even though nothing is
        # using it.
        def fake_run(argv, **kwargs):
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0)  # branch exists
            return MagicMock(returncode=0)

        mock_run.side_effect = fake_run
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worktree = tmp / "worktree"  # deliberately not created -- directory is gone

            create_worktree(tmp, worktree, "model-fix-parallel-nef", "main", config_path=tmp / "no-config.toml")

            argvs = [c.args[0] for c in mock_run.call_args_list]
            self.assertIn(["git", "branch", "-D", "model-fix-parallel-nef"], argvs)
            self.assertIn(["git", "worktree", "add", "-b", "model-fix-parallel-nef", str(worktree), "main"], argvs)
            # the branch delete must happen before the worktree add, not after
            delete_index = argvs.index(["git", "branch", "-D", "model-fix-parallel-nef"])
            add_index = argvs.index(["git", "worktree", "add", "-b", "model-fix-parallel-nef", str(worktree), "main"])
            self.assertLess(delete_index, add_index)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_does_not_delete_a_branch_that_does_not_exist(self, mock_run):
        def fake_run(argv, **kwargs):
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=1)  # no such branch
            return MagicMock(returncode=0)

        mock_run.side_effect = fake_run
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worktree = tmp / "worktree"

            create_worktree(tmp, worktree, "model-fix-parallel-nef", "main", config_path=tmp / "no-config.toml")

            argvs = [c.args[0] for c in mock_run.call_args_list]
            self.assertNotIn(["git", "branch", "-D", "model-fix-parallel-nef"], argvs)
            self.assertIn(["git", "worktree", "add", "-b", "model-fix-parallel-nef", str(worktree), "main"], argvs)


class ConsumeHandshakeTests(unittest.TestCase):
    """Spec M2/M5: create_worktree's optional squad_status_path guard on
    the reuse-in-place `checkout -B` call. Every scenario here reaches
    the SAME branch-exists/commits-fully-contained state the pre-existing
    test_resets_a_reused_branch_whose_commits_are_all_contained fixture
    uses (M5's own no-discard check passes, so WITHOUT the new guard a
    plain `checkout -B` would run) -- only squad_status_path and the
    branch's recorded head sha vary.
    """
    BRANCH = "model-fix-parallel-nef"

    def _fake_run(self, head_sha):
        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--verify", "--quiet", f"refs/heads/{self.BRANCH}"]:
                return MagicMock(returncode=0, stdout="abc123\n", stderr="")
            if argv[:3] == ["git", "rev-list", "--count"]:
                return MagicMock(returncode=0, stdout="0\n", stderr="")  # nothing undiscardable
            if argv == ["git", "rev-parse", "--verify", "--quiet", self.BRANCH]:
                return MagicMock(returncode=0, stdout=f"{head_sha}\n", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")
        return fake_run

    def _create(self, tmp, mock_run, head_sha, squad_status_path=None):
        mock_run.side_effect = self._fake_run(head_sha)
        worktree = tmp / "worktree"
        worktree.mkdir()
        create_worktree(
            tmp, worktree, self.BRANCH, "main", config_path=tmp / "no-config.toml",
            squad_status_path=squad_status_path,
        )
        return [c.args[0] for c in mock_run.call_args_list]

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_blocks_reset_when_head_sha_not_recorded(self, mock_run):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            status_path = tmp / "squad-status" / "nikon.json"
            status_path.parent.mkdir()
            status_path.write_text(json.dumps({"heads": {"some-other-sha": {"status": "consumed"}}}))

            argvs = self._create(tmp, mock_run, "unresolved-sha", squad_status_path=status_path)

            self.assertNotIn(["git", "checkout", "-B", self.BRANCH, "main"], argvs)
            self.assertIn(["git", "checkout", self.BRANCH], argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_allows_reset_when_head_recorded_consumed(self, mock_run):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            status_path = tmp / "squad-status" / "nikon.json"
            status_path.parent.mkdir()
            status_path.write_text(json.dumps({"heads": {"resolved-sha": {"status": "consumed"}}}))

            argvs = self._create(tmp, mock_run, "resolved-sha", squad_status_path=status_path)

            self.assertIn(["git", "checkout", "-B", self.BRANCH, "main"], argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_allows_reset_when_head_recorded_quarantined(self, mock_run):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            status_path = tmp / "squad-status" / "nikon.json"
            status_path.parent.mkdir()
            status_path.write_text(json.dumps({"heads": {"resolved-sha": {"status": "quarantined"}}}))

            argvs = self._create(tmp, mock_run, "resolved-sha", squad_status_path=status_path)

            self.assertIn(["git", "checkout", "-B", self.BRANCH, "main"], argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_none_squad_status_path_is_unguarded_backward_compat(self, mock_run):
        # squad_status_path defaults to None -- exactly today's behavior,
        # unaffected even though this same head sha would be blocked if
        # tracking were active for this format (spec: un-piloted formats
        # are completely unaffected).
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            argvs = self._create(tmp, mock_run, "whatever-unresolved-sha")
            self.assertIn(["git", "checkout", "-B", self.BRANCH, "main"], argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_missing_squad_status_file_blocks_reset_fail_closed(self, mock_run):
        # squad-status tracking IS active for this format (a path was
        # given) but the merger hasn't written its first status file yet
        # -- get-with-default reads this as "nothing resolved", not
        # "nothing to protect", so the reset stays blocked rather than
        # racing the merger's very first poll.
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            status_path = tmp / "squad-status" / "nikon.json"  # never created
            argvs = self._create(tmp, mock_run, "whatever-sha", squad_status_path=status_path)
            self.assertNotIn(["git", "checkout", "-B", self.BRANCH, "main"], argvs)
            self.assertIn(["git", "checkout", self.BRANCH], argvs)


# /tmp/base is an inert fixture path -- no real filesystem I/O happens
# here, this only exercises string/Path construction.
class WorktreePathTests(unittest.TestCase):
    def test_lowercases_format_into_a_stable_path(self):
        self.assertEqual(
            worktree_path(Path("/tmp/base"), "NEF"),  # nosec B108
            Path("/tmp/base/model-fix-nef"),  # nosec B108
        )


class BranchNameTests(unittest.TestCase):
    def test_lowercases_format_into_a_stable_branch_name(self):
        self.assertEqual(branch_name("NEF"), "model-fix-parallel-nef")


class CommitsOnBranchTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.subprocess.run")
    def test_returns_commit_subjects_oldest_first(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="first\nsecond\n")
        commits = commits_on_branch(Path("/fake/repo"), "main", "model-fix-parallel-nef")
        self.assertEqual(commits, ["first", "second"])
        args, kwargs = mock_run.call_args
        self.assertEqual(
            args[0],
            ["git", "log", "main..model-fix-parallel-nef", "--format=%s", "--reverse"],
        )
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_empty_when_no_commits(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="")
        commits = commits_on_branch(Path("/fake/repo"), "main", "model-fix-parallel-nef")
        self.assertEqual(commits, [])


class NovelCommitsTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.subprocess.run")
    def test_invokes_git_cherry_against_base(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="")
        novel_commits(Path("/fake/repo"), "main", "model-fix-parallel-nef")
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], ["git", "cherry", "main", "model-fix-parallel-nef"])
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_keeps_only_plus_marked_shas(self, mock_run):
        # "+" == no patch-equivalent upstream (novel); "-" == already in base
        # by patch-id (a dirty dup). Only the "+" shas should survive.
        mock_run.return_value = MagicMock(
            returncode=0, stdout="+ aaa111\n- bbb222\n+ ccc333\n"
        )
        novel = novel_commits(Path("/fake/repo"), "main", "model-fix-parallel-nef")
        self.assertEqual(novel, ["aaa111", "ccc333"])

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_empty_when_every_commit_is_a_patch_dup(self, mock_run):
        # A worker that only re-derived already-swept fixes: git cherry marks
        # every commit "-", so nothing is novel and the merge gate drops it.
        mock_run.return_value = MagicMock(returncode=0, stdout="- bbb222\n- ddd444\n")
        self.assertEqual(novel_commits(Path("/fake/repo"), "main", "b"), [])

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_empty_when_no_commits_at_all(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="")
        self.assertEqual(novel_commits(Path("/fake/repo"), "main", "b"), [])


class MergeBranchTests(unittest.TestCase):
    def _shas(self, pre_merge="base123", first_parent="base123"):
        """A subprocess.run side_effect: rev-parse HEAD reports pre_merge
        (recorded before the merge), rev-parse HEAD^1 reports
        first_parent (checked before any rollback), everything else
        succeeds quietly."""
        def fake_run(argv, **kwargs):
            if argv[:3] == ["git", "rev-parse", "HEAD"]:
                return MagicMock(returncode=0, stdout=f"{pre_merge}\n", stderr="")
            if argv[:3] == ["git", "rev-parse", "HEAD^1"]:
                return MagicMock(returncode=0, stdout=f"{first_parent}\n", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")
        return fake_run

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_merges_and_passes_when_tests_pass(self, mock_run):
        mock_run.side_effect = self._shas()
        merged, message = merge_branch(Path("/fake/repo"), "model-fix-parallel-nef", cargo_test_fn=lambda: True)
        self.assertTrue(merged)
        self.assertEqual(message, "merged")
        all_argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertIn(
            ["git", "merge", "--no-ff", "model-fix-parallel-nef", "-m", "merge: model-fix-parallel-nef"],
            all_argvs,
        )
        # no abort, no reset --hard
        self.assertNotIn(["git", "merge", "--abort"], all_argvs)
        self.assertFalse(any(argv[:3] == ["git", "reset", "--hard"] for argv in all_argvs))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_aborts_on_merge_conflict_without_running_tests(self, mock_run):
        cargo_test_calls = []
        shas = self._shas()

        def merge_conflicts(argv, **kwargs):
            if argv[:2] == ["git", "merge"] and "--abort" not in argv:
                return MagicMock(returncode=1, stdout="", stderr="CONFLICT (content): x.rs")
            return shas(argv, **kwargs)

        mock_run.side_effect = merge_conflicts

        merged, message = merge_branch(
            Path("/fake/repo"), "model-fix-parallel-nef",
            cargo_test_fn=lambda: cargo_test_calls.append(1) or True,
        )

        self.assertFalse(merged)
        self.assertIn("merge conflict", message)
        self.assertEqual(cargo_test_calls, [])  # never reached -- merge failed first
        all_argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertIn(["git", "merge", "--abort"], all_argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_rolls_back_to_the_recorded_pre_merge_head_when_tests_regress(self, mock_run):
        mock_run.side_effect = self._shas(pre_merge="base123", first_parent="base123")

        merged, message = merge_branch(Path("/fake/repo"), "model-fix-parallel-nef", cargo_test_fn=lambda: False)

        self.assertFalse(merged)
        self.assertIn("regressed", message)
        all_argvs = [c.args[0] for c in mock_run.call_args_list]
        # reset targets the recorded pre-merge sha, never a relative
        # HEAD~1 (which could land somewhere else if HEAD moved)
        self.assertIn(["git", "reset", "--hard", "base123"], all_argvs)
        # the merge itself was NOT aborted (it happened; only the resulting
        # commit is rolled back afterward) -- no "git merge --abort" call
        self.assertNotIn(["git", "merge", "--abort"], all_argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_refuses_to_reset_when_head_is_not_the_merge_just_created(self, mock_run):
        # No-discard invariant (M5): HEAD's first parent isn't the
        # recorded pre-merge sha (something else moved the ref), so a
        # reset could discard commits merge_branch never created --
        # refuse it and say so.
        mock_run.side_effect = self._shas(pre_merge="base123", first_parent="somethingelse")

        merged, message = merge_branch(Path("/fake/repo"), "model-fix-parallel-nef", cargo_test_fn=lambda: False)

        self.assertFalse(merged)
        self.assertIn("refusing to reset", message)
        all_argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertFalse(any(argv[:3] == ["git", "reset", "--hard"] for argv in all_argvs))


class ProcessGroupAliveTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.os.killpg")
    def test_true_when_signal_succeeds(self, mock_killpg):
        mock_killpg.return_value = None
        self.assertTrue(_process_group_alive(123))

    @patch("parallel_model_fix_loop.os.killpg")
    def test_false_when_process_lookup_error(self, mock_killpg):
        mock_killpg.side_effect = ProcessLookupError()
        self.assertFalse(_process_group_alive(123))


class KillProcessGroupTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.os.killpg")
    def test_sends_sigkill_by_default(self, mock_killpg):
        _kill_process_group(123)
        mock_killpg.assert_called_once_with(123, signal.SIGKILL)

    @patch("parallel_model_fix_loop.os.killpg")
    def test_ignores_already_dead_group(self, mock_killpg):
        mock_killpg.side_effect = ProcessLookupError()
        _kill_process_group(123)  # must not raise


class WaitForProcessGroupExitTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.os.killpg")
    def test_returns_immediately_if_already_dead(self, mock_killpg):
        mock_killpg.side_effect = ProcessLookupError()
        sleeps = []
        _wait_for_process_group_exit(123, sleep_fn=sleeps.append)
        self.assertEqual(sleeps, [])

    @patch("parallel_model_fix_loop.os.killpg")
    def test_polls_until_group_exits(self, mock_killpg):
        calls = []

        def fake_killpg(pgid, sig):
            calls.append(sig)
            if len(calls) < 3:
                return None  # still alive
            raise ProcessLookupError()

        mock_killpg.side_effect = fake_killpg
        sleeps = []
        _wait_for_process_group_exit(123, poll_interval=1, sleep_fn=sleeps.append)
        self.assertEqual(len(sleeps), 2)  # two "still alive" polls before exit confirmed

    @patch("parallel_model_fix_loop.os.killpg")
    def test_force_kills_after_timeout(self, mock_killpg):
        # Always reports alive via signal-0 checks; a plain SIGKILL call
        # should eventually fire once force_after is reached.
        mock_killpg.return_value = None
        sleeps = []
        _wait_for_process_group_exit(123, poll_interval=1, force_after=2, sleep_fn=sleeps.append)
        kill_calls = [c for c in mock_killpg.call_args_list if c.args[1] == signal.SIGKILL]
        self.assertEqual(len(kill_calls), 1)


class ActiveWorkerRegistryTests(unittest.TestCase):
    def tearDown(self):
        with parallel_model_fix_loop._active_pgids_lock:
            parallel_model_fix_loop._active_pgids.clear()

    @patch("parallel_model_fix_loop.os.killpg")
    def test_kill_all_active_workers_kills_every_registered_pgid(self, mock_killpg):
        _register_pgid(111)
        _register_pgid(222)
        _kill_all_active_workers()
        killed = {c.args[0] for c in mock_killpg.call_args_list}
        self.assertEqual(killed, {111, 222})

    def test_unregister_removes_pgid(self):
        _register_pgid(333)
        _unregister_pgid(333)
        with parallel_model_fix_loop._active_pgids_lock:
            self.assertNotIn(333, parallel_model_fix_loop._active_pgids)


class PgidPersistenceTests(unittest.TestCase):
    """_register_pgid/_unregister_pgid mirror the active set to the
    dispatcher pgid file (once main() points _pgids_persist_path at it),
    so a dispatcher killed without cleanup leaves an accurate record for
    the next startup's orphan reaper."""

    def setUp(self):
        self._tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmpdir.cleanup)
        self.pgids_path = Path(self._tmpdir.name) / "dispatcher-pgids.json"
        parallel_model_fix_loop._set_pgids_persist_path(self.pgids_path)
        self.addCleanup(parallel_model_fix_loop._set_pgids_persist_path, None)
        self.addCleanup(self._clear_registry)

    def _clear_registry(self):
        with parallel_model_fix_loop._active_pgids_lock:
            parallel_model_fix_loop._active_pgids.clear()

    def _recorded(self):
        return json.loads(self.pgids_path.read_text())["pgids"]

    def test_register_writes_the_full_snapshot(self):
        _register_pgid(111)
        _register_pgid(222)
        self.assertEqual(self._recorded(), [111, 222])

    def test_unregister_removes_the_pgid_from_the_file(self):
        _register_pgid(111)
        _register_pgid(222)
        _unregister_pgid(111)
        self.assertEqual(self._recorded(), [222])

    def test_no_persist_path_means_no_file_io(self):
        parallel_model_fix_loop._set_pgids_persist_path(None)
        _register_pgid(333)
        self.assertFalse(self.pgids_path.exists())

    def test_pgids_file_never_left_torn(self):
        # tempfile+os.replace: after any number of writes, the directory
        # holds exactly the final file, no orphaned .tmp siblings.
        for pgid in (1, 2, 3):
            _register_pgid(pgid)
        leftovers = [p.name for p in self.pgids_path.parent.iterdir() if p.name != self.pgids_path.name]
        self.assertEqual(leftovers, [])
        self.assertEqual(self._recorded(), [1, 2, 3])

    def test_stale_snapshot_can_never_overwrite_a_newer_one(self):
        # Register/unregister run concurrently on up to max_parallel
        # worker threads. The losing interleaving this pins down: T1
        # unregisters the last pgid, snapshots [], and stalls before its
        # write; T2 registers a new pgid and writes [new]; T1's stale []
        # then lands over it -- a LIVE worker vanishes from the file, and
        # a SIGKILLed dispatcher's successor reaps nothing while that
        # worker's cargo/rustc tree runs unsupervised forever. The fix
        # takes snapshot AND write under one persist lock, so the last
        # write always reflects the then-current set. Deterministic
        # both ways: pre-fix, T2 finishes its write during the join
        # below and T1's gated stale write clobbers it; post-fix, lock
        # ordering forces T2's snapshot+write entirely after T1's write.
        _register_pgid(100)

        real_write = parallel_model_fix_loop._write_pgids_file
        gate = threading.Event()
        empty_write_entered = threading.Event()

        def gated_write(path, pgids):
            # Hold exactly the unregister's empty-set write open,
            # simulating T1 preempted mid-persist while T2 races it.
            if list(pgids) == []:
                empty_write_entered.set()
                gate.wait(timeout=10)
            real_write(path, pgids)

        with patch("parallel_model_fix_loop._write_pgids_file", gated_write):
            t1 = threading.Thread(target=_unregister_pgid, args=(100,))
            t1.start()
            self.assertTrue(empty_write_entered.wait(timeout=10))
            t2 = threading.Thread(target=_register_pgid, args=(300,))
            t2.start()
            # Post-fix this join times out (T2 is correctly queued behind
            # the persist lock T1 holds); pre-fix it completes, having
            # already written [300] for T1 to clobber.
            t2.join(timeout=0.5)
            gate.set()
            t1.join(timeout=10)
            t2.join(timeout=10)
        self.assertFalse(t1.is_alive())
        self.assertFalse(t2.is_alive())
        self.assertEqual(self._recorded(), [300])


class AcquireDispatcherLockTests(unittest.TestCase):
    def setUp(self):
        self._tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmpdir.cleanup)
        self.lock_path = Path(self._tmpdir.name) / "dispatcher.lock"

    def test_first_acquire_succeeds_and_records_pid(self):
        lock = acquire_dispatcher_lock(self.lock_path)
        self.assertIsNotNone(lock)
        self.addCleanup(lock.close)
        self.assertEqual(self.lock_path.read_text().strip(), str(os.getpid()))

    def test_second_acquire_is_refused_while_first_is_held(self):
        # flock is per-open-file-description, so even a second acquire
        # from the same process is correctly refused -- exactly what a
        # second dispatcher on this host would hit.
        first = acquire_dispatcher_lock(self.lock_path)
        self.assertIsNotNone(first)
        self.addCleanup(first.close)
        self.assertIsNone(acquire_dispatcher_lock(self.lock_path))

    def test_lock_is_reacquirable_after_release(self):
        first = acquire_dispatcher_lock(self.lock_path)
        first.close()  # what any process exit does implicitly
        second = acquire_dispatcher_lock(self.lock_path)
        self.assertIsNotNone(second)
        self.addCleanup(second.close)


class ReapOrphanPgidsTests(unittest.TestCase):
    """Orphan reaping with a fake pgid file and injected kill/alive/sleep
    fns -- no real killpg anywhere."""

    def setUp(self):
        self._tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmpdir.cleanup)
        self.pgids_path = Path(self._tmpdir.name) / "dispatcher-pgids.json"

    def _write(self, pgids):
        self.pgids_path.write_text(json.dumps({"pgids": pgids}))

    def test_missing_file_reaps_nothing(self):
        signaled = reap_orphan_worker_pgids(
            self.pgids_path, kill_fn=lambda *a: self.fail("nothing to kill"),
            alive_fn=lambda p: self.fail("nothing to check"), log_fn=lambda m: None,
        )
        self.assertEqual(signaled, [])

    def test_term_then_kill_after_grace_for_a_stubborn_group(self):
        self._write([111, 222, 333])
        killed = []
        # 111 ignores SIGTERM and stays alive through the grace period;
        # 222 exits on SIGTERM; 333 was already dead.
        alive = {111: True, 222: True, 333: False}

        def kill_fn(pgid, sig):
            killed.append((pgid, sig))
            if pgid == 222 and sig == signal.SIGTERM:
                alive[222] = False

        signaled = reap_orphan_worker_pgids(
            self.pgids_path, kill_fn=kill_fn, alive_fn=lambda p: alive[p],
            sleep_fn=lambda s: None, grace_seconds=1.0, log_fn=lambda m: None,
        )
        self.assertEqual(signaled, [111, 222])
        self.assertIn((111, signal.SIGTERM), killed)
        self.assertIn((222, signal.SIGTERM), killed)
        self.assertIn((111, signal.SIGKILL), killed)
        self.assertNotIn((222, signal.SIGKILL), killed)
        self.assertFalse(any(pgid == 333 for pgid, _ in killed))

    def test_clears_the_file_after_reaping(self):
        self._write([111])
        reap_orphan_worker_pgids(
            self.pgids_path, kill_fn=lambda *a: None, alive_fn=lambda p: False,
            sleep_fn=lambda s: None, log_fn=lambda m: None,
        )
        self.assertEqual(json.loads(self.pgids_path.read_text()), {"pgids": []})

    def test_own_process_group_is_never_signaled(self):
        self._write([os.getpgrp()])
        signaled = reap_orphan_worker_pgids(
            self.pgids_path, kill_fn=lambda *a: self.fail("must not signal own group"),
            alive_fn=lambda p: True, sleep_fn=lambda s: None, log_fn=lambda m: None,
        )
        self.assertEqual(signaled, [])

    def test_corrupt_file_is_cleared_not_fatal(self):
        self.pgids_path.write_text("{ torn json")
        signaled = reap_orphan_worker_pgids(
            self.pgids_path, kill_fn=lambda *a: self.fail("nothing to kill"),
            alive_fn=lambda p: True, sleep_fn=lambda s: None, log_fn=lambda m: None,
        )
        self.assertEqual(signaled, [])
        self.assertEqual(json.loads(self.pgids_path.read_text()), {"pgids": []})


class FastForwardLocalMainTests(unittest.TestCase):
    """M5 round-start rule, exercised entirely through a scripted
    subprocess.run: local main only ever moves by pure fast-forward, and
    divergence is loudly skipped -- never reset."""

    def _script(self, main_sha="aaa", origin_sha="bbb", is_ancestor=True,
                current_branch="main", fetch_ok=True):
        calls = []

        def fake_run(argv, **kwargs):
            calls.append(argv)
            if argv[:3] == ["git", "fetch", "origin"]:
                return MagicMock(returncode=0 if fetch_ok else 1, stdout="", stderr="offline")
            if argv == ["git", "rev-parse", "--verify", "--quiet", "refs/heads/main"]:
                return MagicMock(returncode=0, stdout=f"{main_sha}\n", stderr="")
            if argv == ["git", "rev-parse", "--verify", "--quiet", "refs/remotes/origin/main"]:
                return MagicMock(returncode=0, stdout=f"{origin_sha}\n", stderr="")
            if argv[:3] == ["git", "merge-base", "--is-ancestor"]:
                return MagicMock(returncode=0 if is_ancestor else 1, stdout="", stderr="")
            if argv == ["git", "rev-parse", "--abbrev-ref", "HEAD"]:
                return MagicMock(returncode=0, stdout=f"{current_branch}\n", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")

        return calls, fake_run

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_ff_merges_when_main_is_checked_out_and_behind(self, mock_run):
        calls, fake = self._script(current_branch="main")
        mock_run.side_effect = fake
        updated, _ = fast_forward_local_main(Path("/fake/repo"), log_fn=lambda m: None)
        self.assertTrue(updated)
        self.assertIn(["git", "merge", "--ff-only", "refs/remotes/origin/main"], calls)
        self.assertFalse(any(argv[:2] == ["git", "branch"] for argv in calls))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_branch_f_updates_main_when_not_checked_out(self, mock_run):
        calls, fake = self._script(current_branch="model-fix-sweep-local")
        mock_run.side_effect = fake
        updated, _ = fast_forward_local_main(Path("/fake/repo"), log_fn=lambda m: None)
        self.assertTrue(updated)
        self.assertIn(["git", "branch", "-f", "main", "refs/remotes/origin/main"], calls)
        self.assertFalse(any(argv[:2] == ["git", "merge"] for argv in calls))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_diverged_main_is_loudly_skipped_never_reset(self, mock_run):
        calls, fake = self._script(is_ancestor=False)
        mock_run.side_effect = fake
        logged = []
        updated, message = fast_forward_local_main(Path("/fake/repo"), log_fn=logged.append)
        self.assertFalse(updated)
        self.assertIn("refusing to touch it", message)
        self.assertTrue(any("WARNING" in line for line in logged))
        # no merge, no branch -f, and certainly no reset of any kind
        self.assertFalse(any(argv[:2] == ["git", "merge"] and "--ff-only" in argv for argv in calls))
        self.assertFalse(any(argv[:2] == ["git", "branch"] for argv in calls))
        self.assertFalse(any(argv[:2] == ["git", "reset"] for argv in calls))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_already_up_to_date_touches_nothing(self, mock_run):
        calls, fake = self._script(main_sha="same", origin_sha="same")
        mock_run.side_effect = fake
        updated, message = fast_forward_local_main(Path("/fake/repo"), log_fn=lambda m: None)
        self.assertTrue(updated)
        self.assertIn("already matches", message)
        self.assertFalse(any(argv[:2] == ["git", "merge"] for argv in calls))
        self.assertFalse(any(argv[:2] == ["git", "branch"] for argv in calls))


class EnsureIntegrationBranchTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.subprocess.run")
    def test_on_main_retargets_to_the_sweep_local_branch(self, mock_run):
        # Checked out on main + no existing sweep-local branch: cut it
        # from main and check it out -- main itself is never merged into.
        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--abbrev-ref", "HEAD"]:
                return MagicMock(returncode=0, stdout="main\n", stderr="")
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=1, stdout="", stderr="")  # branch missing
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        branch = ensure_integration_branch(Path("/fake/repo"), log_fn=lambda m: None)
        self.assertEqual(branch, "model-fix-sweep-local")
        argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertIn(["git", "branch", "model-fix-sweep-local", "main"], argvs)
        self.assertIn(["git", "checkout", "model-fix-sweep-local"], argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_existing_sweep_local_branch_is_reused_never_reset(self, mock_run):
        # The branch already exists (carrying prior rounds' unswept
        # merges): plain checkout only -- no `branch -f`, no `checkout
        # -B`, nothing that could move its tip.
        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--abbrev-ref", "HEAD"]:
                return MagicMock(returncode=0, stdout="main\n", stderr="")
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0, stdout="abc\n", stderr="")  # branch exists
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        branch = ensure_integration_branch(Path("/fake/repo"), log_fn=lambda m: None)
        self.assertEqual(branch, "model-fix-sweep-local")
        argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertNotIn(["git", "branch", "model-fix-sweep-local", "main"], argvs)
        self.assertIn(["git", "checkout", "model-fix-sweep-local"], argvs)
        self.assertFalse(any(argv[:3] == ["git", "checkout", "-B"] for argv in argvs))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_non_main_branch_is_kept_as_the_integration_target(self, mock_run):
        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--abbrev-ref", "HEAD"]:
                return MagicMock(returncode=0, stdout="feature-x\n", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        branch = ensure_integration_branch(Path("/fake/repo"), log_fn=lambda m: None)
        self.assertEqual(branch, "feature-x")
        # only the branch lookup ran -- nothing was created or checked out
        argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertEqual(argvs, [["git", "rev-parse", "--abbrev-ref", "HEAD"]])

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_checkout_failure_returns_none_and_logs_instead_of_raising(self, mock_run):
        # Once sweep-local has diverged from main, a dirty dispatcher
        # checkout makes `git checkout` refuse (correctly). That must
        # come back as None + a loud warning carrying git's own stderr --
        # never a CalledProcessError, which would propagate out of
        # run_round and kill an --infinite dispatcher outright.
        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--abbrev-ref", "HEAD"]:
                return MagicMock(returncode=0, stdout="main\n", stderr="")
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0, stdout="abc\n", stderr="")  # branch exists
            if argv[:2] == ["git", "checkout"]:
                return MagicMock(
                    returncode=1, stdout="",
                    stderr="error: Your local changes to the following files would be overwritten",
                )
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        logged = []
        branch = ensure_integration_branch(Path("/fake/repo"), log_fn=logged.append)
        self.assertIsNone(branch)
        self.assertTrue(any("WARNING" in line for line in logged))
        self.assertTrue(any("would be overwritten" in line for line in logged))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_branch_creation_failure_returns_none_and_logs_instead_of_raising(self, mock_run):
        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--abbrev-ref", "HEAD"]:
                return MagicMock(returncode=0, stdout="main\n", stderr="")
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=1, stdout="", stderr="")  # branch missing
            if argv[:2] == ["git", "branch"]:
                return MagicMock(returncode=1, stdout="", stderr="fatal: cannot lock ref")
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        logged = []
        branch = ensure_integration_branch(Path("/fake/repo"), log_fn=logged.append)
        self.assertIsNone(branch)
        self.assertTrue(any("WARNING" in line for line in logged))
        # and it never went on to check anything out
        argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertFalse(any(argv[:2] == ["git", "checkout"] for argv in argvs))


class RunRoundIntegrationTargetTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.discover_formats")
    @patch("parallel_model_fix_loop.process_format")
    @patch("parallel_model_fix_loop.ensure_integration_branch", return_value=None)
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_no_integration_target_skips_the_round_without_dispatching(
        self, mock_ff, mock_ensure, mock_process, mock_discover,
    ):
        # ensure_integration_branch returning None (un-checkoutable
        # sweep-local, see its docstring) means the round has nowhere
        # M5-legal to merge: run_round must report failure WITHOUT
        # raising and WITHOUT dispatching a single worker -- the
        # --infinite loop then simply retries next round.
        args = SimpleNamespace(
            formats="NEF", cache_dir="/nonexistent", max_parallel=1,
            worktree_dir="/nonexistent", log_dir="/nonexistent", timeout=None,
        )
        ok = run_round(args, Path("/fake/config.toml"))
        self.assertFalse(ok)
        mock_discover.assert_not_called()
        mock_process.assert_not_called()


class RunRoundJanitorGatingTests(unittest.TestCase):
    """run_round's own --squad-mode help text promises "Default: off
    (run_round, today's per-format behavior, unaffected)" -- the
    currently-running per-format fleet uses exactly this legacy path, so
    the M5 janitor must NOT run here unless an operator explicitly opts
    in via --enable-janitor (run_squad_round, a new entrypoint, always
    runs it -- see RunSquadRoundTests's mock_janitor.assert_called_once
    assertions elsewhere in this file)."""

    def _args(self, tmp, **overrides):
        base = dict(
            formats="JPEG", cache_dir="/unused", max_parallel=1,
            worktree_dir=str(tmp / "wt"), log_dir=str(tmp / "log"), timeout=None,
        )
        base.update(overrides)
        return SimpleNamespace(**base)

    @patch("parallel_model_fix_loop._run_janitor_safely")
    @patch("parallel_model_fix_loop.process_format")
    @patch("parallel_model_fix_loop.ensure_integration_branch", return_value="model-fix-sweep-local")
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_janitor_not_called_by_default(self, mock_ff, mock_ensure, mock_process, mock_janitor):
        mock_process.return_value = ("JPEG", {"status": "timeout"})
        with tempfile.TemporaryDirectory() as tmpdir:
            args = self._args(Path(tmpdir))
            run_round(args, Path("/fake/config.toml"))
        mock_janitor.assert_not_called()

    @patch("parallel_model_fix_loop._run_janitor_safely")
    @patch("parallel_model_fix_loop.process_format")
    @patch("parallel_model_fix_loop.ensure_integration_branch", return_value="model-fix-sweep-local")
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_janitor_called_when_explicitly_enabled(self, mock_ff, mock_ensure, mock_process, mock_janitor):
        mock_process.return_value = ("JPEG", {"status": "timeout"})
        with tempfile.TemporaryDirectory() as tmpdir:
            args = self._args(Path(tmpdir), enable_janitor=True)
            run_round(args, Path("/fake/config.toml"))
        mock_janitor.assert_called_once()


# ---------------------------------------------------------------------------
# allocate_squad_slots (spec S2 slot-allocation formula)
# ---------------------------------------------------------------------------

class AllocateSquadSlotsTests(unittest.TestCase):
    # The S2 census snapshot, verbatim from the spec table.
    CENSUS = {
        "canon": 917, "nikon": 613, "sony-minolta": 518, "xmp": 382,
        "exif-core": 284, "olympus": 231, "pentax-samsung": 215,
        "panasonic-leica": 183, "mobile": 185, "thermal": 158,
        "sigma-c2pa": 167, "ps-docs": 138, "standards-appn": 135, "tail": 221,
    }

    def test_spec_worked_example_total_slots_20(self):
        # spec S2's own worked example: xmp's raw rounding gives it 2
        # (Sigma=21), but it has the lowest gaps-per-slot of the
        # multi-slot squads and yields its extra slot back.
        result = allocate_squad_slots(self.CENSUS, 20)
        self.assertEqual(
            result,
            {
                "canon": 4, "nikon": 3, "sony-minolta": 2, "xmp": 1,
                "exif-core": 1, "olympus": 1, "pentax-samsung": 1,
                "panasonic-leica": 1, "mobile": 1, "thermal": 1,
                "sigma-c2pa": 1, "ps-docs": 1, "standards-appn": 1, "tail": 1,
            },
        )
        self.assertEqual(sum(result.values()), 20)

    def test_spec_worked_example_lanes_50(self):
        result = allocate_squad_slots(self.CENSUS, 50)
        self.assertEqual(
            result,
            {
                "canon": 11, "nikon": 7, "sony-minolta": 6, "xmp": 4,
                "exif-core": 3, "olympus": 3, "tail": 3,
                "pentax-samsung": 2, "panasonic-leica": 2, "mobile": 2,
                "thermal": 2, "sigma-c2pa": 2, "ps-docs": 2, "standards-appn": 1,
            },
        )
        self.assertEqual(sum(result.values()), 50)

    def test_sum_equals_total_slots_whenever_slots_cover_every_active_squad(self):
        for total in (14, 15, 20, 27, 50, 73, 100):
            result = allocate_squad_slots(self.CENSUS, total)
            self.assertEqual(sum(result.values()), total, f"total_slots={total}")

    def test_every_squad_with_open_gaps_gets_at_least_one_slot(self):
        result = allocate_squad_slots(self.CENSUS, 20)
        for squad in self.CENSUS:
            self.assertGreaterEqual(result[squad], 1)

    def test_zero_open_gaps_squad_is_excluded_entirely_not_given_a_floor(self):
        gaps = dict(self.CENSUS)
        gaps["empty-squad"] = 0
        result = allocate_squad_slots(gaps, 20)
        self.assertNotIn("empty-squad", result)

    def test_negative_open_gaps_squad_is_also_excluded(self):
        gaps = {"a": 100, "b": -5}
        result = allocate_squad_slots(gaps, 10)
        self.assertNotIn("b", result)
        self.assertEqual(result, {"a": 10})

    def test_no_active_squads_returns_empty(self):
        self.assertEqual(allocate_squad_slots({}, 20), {})
        self.assertEqual(allocate_squad_slots({"a": 0, "b": 0}, 20), {})

    def test_non_positive_total_slots_returns_empty(self):
        self.assertEqual(allocate_squad_slots(self.CENSUS, 0), {})
        self.assertEqual(allocate_squad_slots(self.CENSUS, -1), {})

    def test_fairness_higher_open_gaps_never_gets_fewer_slots(self):
        # Property: after reconciliation, a squad with strictly more open
        # gaps than another never ends up with fewer slots than it.
        distributions = [
            self.CENSUS,
            {"a": 1000, "b": 500, "c": 10, "d": 1},
            {"a": 50, "b": 49, "c": 1},
            {"a": 7, "b": 7, "c": 7, "d": 7, "e": 7},
        ]
        for gaps in distributions:
            for total in (len(gaps), len(gaps) + 3, 20, 50):
                result = allocate_squad_slots(gaps, total)
                for s1, g1 in gaps.items():
                    if g1 <= 0:
                        continue
                    for s2, g2 in gaps.items():
                        if g2 <= 0:
                            continue
                        if g1 > g2:
                            self.assertGreaterEqual(
                                result[s1], result[s2],
                                f"total={total} gaps={gaps}: {s1}({g1}) should not get "
                                f"fewer slots than {s2}({g2})",
                            )

    def test_fewer_slots_than_active_squads_leaves_floor_overshoot_rather_than_zeroing_one_out(self):
        # 3 squads, only 2 total_slots: max(1,.) floor can't be reconciled
        # away without giving a squad with open gaps zero slots, which
        # spec S2 forbids -- every squad keeps its floor of 1, so the sum
        # legitimately exceeds total_slots here.
        result = allocate_squad_slots({"a": 100, "b": 50, "c": 1}, 2)
        self.assertEqual(result, {"a": 1, "b": 1, "c": 1})


# ---------------------------------------------------------------------------
# Squad naming helpers
# ---------------------------------------------------------------------------

class SquadNamingTests(unittest.TestCase):
    def test_squad_worktree_path(self):
        self.assertEqual(
            squad_worktree_path(Path("/base"), "canon", 2), Path("/base/model-fix-canon-2"),
        )

    def test_squad_branch_name(self):
        self.assertEqual(squad_branch_name("sony-minolta", 3), "model-fix-parallel-sony-minolta-3")

    def test_squad_from_branch_extracts_squad_and_slot(self):
        self.assertEqual(squad_from_branch("model-fix-parallel-canon-2"), "canon")
        self.assertEqual(squad_from_branch("model-fix-parallel-sony-minolta-11"), "sony-minolta")

    def test_squad_from_branch_none_for_legacy_per_format_branch(self):
        self.assertIsNone(squad_from_branch("model-fix-parallel-jpeg"))
        self.assertIsNone(squad_from_branch("model-fix-parallel-cr2"))

    def test_staging_branch_or_origin(self):
        self.assertEqual(staging_branch_or_origin("model-fix-parallel-canon-1"), "squad/canon")
        self.assertEqual(staging_branch_or_origin("model-fix-parallel-jpeg", "origin/main"), "origin/main")


# ---------------------------------------------------------------------------
# squad_open_gaps_from_attribution / squad_worker_formats
# ---------------------------------------------------------------------------

class SquadOpenGapsFromAttributionTests(unittest.TestCase):
    def test_extracts_open_gaps_per_squad(self):
        attribution = {"squads": {"canon": {"open_gaps": 10}, "nikon": {"open_gaps": 0}}}
        self.assertEqual(squad_open_gaps_from_attribution(attribution), {"canon": 10, "nikon": 0})

    def test_none_attribution_is_empty(self):
        self.assertEqual(squad_open_gaps_from_attribution(None), {})

    def test_missing_squads_key_is_empty(self):
        self.assertEqual(squad_open_gaps_from_attribution({}), {})


class SquadWorkerFormatsTests(unittest.TestCase):
    def _squads_toml(self, tmp):
        path = tmp / "squads.toml"
        path.write_text(
            '[squads.canon]\nmodules = ["Canon"]\nformats = ["JPEG", "CR2"]\nownership_globs = []\n'
        )
        return path

    def test_prefers_live_attribution_formats(self):
        attribution = {"squads": {"canon": {"formats": ["JPEG", "DNG"]}}}
        with tempfile.TemporaryDirectory() as tmpdir:
            formats = squad_worker_formats("canon", attribution, self._squads_toml(Path(tmpdir)))
        self.assertEqual(formats, ["JPEG", "DNG"])

    def test_falls_back_to_squads_toml_when_attribution_has_nothing(self):
        attribution = {"squads": {"canon": {"formats": []}}}
        with tempfile.TemporaryDirectory() as tmpdir:
            formats = squad_worker_formats("canon", attribution, self._squads_toml(Path(tmpdir)))
        self.assertEqual(formats, ["JPEG", "CR2"])

    def test_none_attribution_falls_back_to_squads_toml(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            formats = squad_worker_formats("canon", None, self._squads_toml(Path(tmpdir)))
        self.assertEqual(formats, ["JPEG", "CR2"])

    def test_unknown_squad_in_squads_toml_returns_empty_not_raise(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            formats = squad_worker_formats("nonexistent-squad", None, self._squads_toml(Path(tmpdir)))
        self.assertEqual(formats, [])


# ---------------------------------------------------------------------------
# process_squad_worker / process_format consume-handshake wiring
# ---------------------------------------------------------------------------

class ConsumeHandshakeWiringTests(unittest.TestCase):
    """Spec item 3: squad mode passes squad_status_path through to
    create_worktree; legacy per-format mode continues to pass None
    (unaffected -- create_worktree's default parameter)."""

    @patch("parallel_model_fix_loop.run_worker", return_value=0)
    @patch("parallel_model_fix_loop.create_worktree")
    def test_legacy_process_format_passes_no_squad_status_path(self, mock_create, mock_run_worker):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            fmt, result = process_format(
                "JPEG", tmp, "main", tmp / "wt", tmp / "log", "/cache", None, config_path=tmp / "no-config",
            )
        self.assertEqual(fmt, "JPEG")
        self.assertEqual(result["status"], "worker_done")
        _args, kwargs = mock_create.call_args
        self.assertIsNone(kwargs.get("squad_status_path"))
        # legacy worker identity is still the format itself
        mock_run_worker.assert_called_once()
        self.assertNotIn("worker_id", mock_run_worker.call_args.kwargs)

    @patch("parallel_model_fix_loop.run_worker", return_value=0)
    @patch("parallel_model_fix_loop.create_worktree")
    def test_squad_mode_process_squad_worker_passes_squad_status_path(self, mock_create, mock_run_worker):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            status_path = tmp / "squad-status" / "canon.json"
            worker_id, result = process_squad_worker(
                "canon", 2, "JPEG", tmp, "squad/canon", tmp / "wt", tmp / "log", "/cache", None,
                config_path=tmp / "no-config", squad_status_path=status_path,
            )
        self.assertEqual(worker_id, "canon-2")
        self.assertEqual(result["status"], "worker_done")
        self.assertEqual(result["squad"], "canon")
        self.assertEqual(result["format"], "JPEG")
        _args, kwargs = mock_create.call_args
        self.assertEqual(kwargs.get("squad_status_path"), status_path)
        # worker identity is the SLOT, not the format it's cycling through
        mock_run_worker.assert_called_once()
        self.assertEqual(mock_run_worker.call_args.kwargs.get("worker_id"), "canon-2")
        self.assertEqual(mock_run_worker.call_args.args[0], "JPEG")

    @patch("parallel_model_fix_loop.create_worktree", side_effect=subprocess.CalledProcessError(1, ["git"], stderr="boom"))
    def test_worktree_failure_is_reported_not_raised(self, mock_create):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worker_id, result = process_squad_worker(
                "canon", 1, "JPEG", tmp, "squad/canon", tmp / "wt", tmp / "log", "/cache", None,
            )
        self.assertEqual(worker_id, "canon-1")
        self.assertEqual(result["status"], "worktree_failed")


# ---------------------------------------------------------------------------
# run_squad_round dispatch
# ---------------------------------------------------------------------------

class RunSquadRoundTests(unittest.TestCase):
    def _args(self, tmp, **overrides):
        base = dict(
            max_parallel=3, cache_dir="/unused",
            worktree_dir=str(tmp / "wt"), log_dir=str(tmp / "log"), timeout=None,
            squads_toml=str(parallel_model_fix_loop.DEFAULT_SQUADS_TOML), home=str(tmp / "home"),
            gap_attribution_path=str(tmp / "gap-attribution.json"),
        )
        base.update(overrides)
        return SimpleNamespace(**base)

    @patch("parallel_model_fix_loop._run_janitor_safely")
    @patch("parallel_model_fix_loop.process_squad_worker")
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_allocates_slots_and_dispatches_with_round_robin_formats(
        self, mock_ff, mock_process, mock_janitor,
    ):
        mock_process.side_effect = lambda squad, n, fmt, *a, **kw: (
            f"{squad}-{n}",
            {"status": "worker_done", "returncode": 0, "worktree": Path("/w"),
             "branch": "b", "log": Path("/l"), "squad": squad, "format": fmt},
        )
        attribution = {
            "squads": {
                "canon": {"open_gaps": 10, "formats": ["JPEG", "CR2"]},
                "nikon": {"open_gaps": 5, "formats": ["NEF"]},
            }
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            args = self._args(Path(tmpdir))
            ok = run_squad_round(
                args, Path("/fake/config.toml"),
                build_attribution_fn=lambda cache_dir: attribution,
                ensure_staging_branch_fn=lambda repo_root, squad, home, log_fn: f"squad/{squad}",
            )
        self.assertTrue(ok)
        calls = {(c.args[0], c.args[1]): c.args[2] for c in mock_process.call_args_list}
        # canon: round(3*10/15)=2 slots, nikon: round(3*5/15)=1 slot
        self.assertEqual(sorted(calls), [("canon", 1), ("canon", 2), ("nikon", 1)])
        self.assertEqual(calls[("canon", 1)], "JPEG")
        self.assertEqual(calls[("canon", 2)], "CR2")
        self.assertEqual(calls[("nikon", 1)], "NEF")
        mock_janitor.assert_called_once()

    @patch("parallel_model_fix_loop._run_janitor_safely")
    @patch("parallel_model_fix_loop.process_squad_worker")
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_base_ref_comes_from_ensure_staging_branch_fn(self, mock_ff, mock_process, mock_janitor):
        mock_process.side_effect = lambda squad, n, fmt, *a, **kw: (
            f"{squad}-{n}",
            {"status": "worker_done", "returncode": 0, "worktree": Path("/w"),
             "branch": "b", "log": Path("/l"), "squad": squad, "format": fmt},
        )
        attribution = {"squads": {"canon": {"open_gaps": 1, "formats": ["JPEG"]}}}
        with tempfile.TemporaryDirectory() as tmpdir:
            args = self._args(Path(tmpdir), max_parallel=1)
            run_squad_round(
                args, Path("/fake/config.toml"),
                build_attribution_fn=lambda cache_dir: attribution,
                ensure_staging_branch_fn=lambda repo_root, squad, home, log_fn: "squad/canon-STAGED",
            )
        base_ref_used = mock_process.call_args.args[4]
        self.assertEqual(base_ref_used, "squad/canon-STAGED")

    @patch("parallel_model_fix_loop._run_janitor_safely")
    @patch("parallel_model_fix_loop.process_squad_worker")
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_squad_status_path_wired_per_squad(self, mock_ff, mock_process, mock_janitor):
        mock_process.side_effect = lambda squad, n, fmt, *a, **kw: (
            f"{squad}-{n}",
            {"status": "worker_done", "returncode": 0, "worktree": Path("/w"),
             "branch": "b", "log": Path("/l"), "squad": squad, "format": fmt},
        )
        attribution = {"squads": {"canon": {"open_gaps": 1, "formats": ["JPEG"]}}}
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir) / "home"
            args = self._args(Path(tmpdir), max_parallel=1, home=str(home))
            run_squad_round(
                args, Path("/fake/config.toml"),
                build_attribution_fn=lambda cache_dir: attribution,
                ensure_staging_branch_fn=lambda repo_root, squad, home, log_fn: "squad/canon",
            )
        squad_status_path = mock_process.call_args.kwargs["squad_status_path"]
        self.assertEqual(squad_status_path, home / "logs" / "squad-status" / "canon.json")

    @patch("parallel_model_fix_loop._run_janitor_safely")
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_no_open_gaps_anywhere_is_success_with_no_dispatch(self, mock_ff, mock_janitor):
        with tempfile.TemporaryDirectory() as tmpdir:
            args = self._args(Path(tmpdir))
            ok = run_squad_round(
                args, Path("/fake/config.toml"),
                build_attribution_fn=lambda cache_dir: {"squads": {}},
                ensure_staging_branch_fn=lambda *a: "squad/unused",
            )
        self.assertTrue(ok)

    @patch("parallel_model_fix_loop._run_janitor_safely")
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_attribution_regeneration_failure_is_reported_as_failure(self, mock_ff, mock_janitor):
        with tempfile.TemporaryDirectory() as tmpdir:
            args = self._args(Path(tmpdir))
            ok = run_squad_round(
                args, Path("/fake/config.toml"),
                build_attribution_fn=lambda cache_dir: None,
                ensure_staging_branch_fn=lambda *a: "squad/unused",
            )
        self.assertFalse(ok)

    @patch("parallel_model_fix_loop._run_janitor_safely")
    @patch("parallel_model_fix_loop.process_squad_worker")
    @patch("parallel_model_fix_loop.fast_forward_local_main", return_value=(True, "ok"))
    def test_worktree_failed_worker_makes_the_round_report_failure(self, mock_ff, mock_process, mock_janitor):
        mock_process.return_value = ("canon-1", {"status": "worktree_failed", "error": "boom"})
        attribution = {"squads": {"canon": {"open_gaps": 1, "formats": ["JPEG"]}}}
        with tempfile.TemporaryDirectory() as tmpdir:
            args = self._args(Path(tmpdir), max_parallel=1)
            ok = run_squad_round(
                args, Path("/fake/config.toml"),
                build_attribution_fn=lambda cache_dir: attribution,
                ensure_staging_branch_fn=lambda *a: "squad/canon",
            )
        self.assertFalse(ok)


# ---------------------------------------------------------------------------
# ensure_squad_staging_branch (real git; reuses squad_merge_loop machinery)
# ---------------------------------------------------------------------------

class EnsureSquadStagingBranchTests(GitRepoTestCase):
    def test_creates_squad_branch_from_origin_ref_when_missing(self):
        repo = self.make_repo()
        home = self.tmp / "home"
        branch = ensure_squad_staging_branch(repo, "canon", home, log_fn=lambda *a: None, origin_ref="main")
        self.assertEqual(branch, "squad/canon")
        result = git(repo, "rev-parse", "--verify", "--quiet", "refs/heads/squad/canon", check=False)
        self.assertEqual(result.returncode, 0)

    def test_does_not_touch_an_existing_staging_worktree(self):
        # ensure_squad_staging_branch must NEVER reset/clean the squad's
        # own merger staging worktree -- that worktree is
        # squad_merge_loop.py's private working area, checked out into
        # and cherry-picked in on its own ~120s poll cadence with no
        # lock coordination with the dispatcher at all. Simulate a
        # merger mid-poll (an uncommitted, untracked marker file sitting
        # in the staging worktree) and confirm a dispatcher round
        # calling this function leaves it completely alone.
        import squad_merge_loop as sml
        repo = self.make_repo()
        home = self.tmp / "home"
        staging = sml.default_staging_dir(home, "canon")
        sml.ensure_staging_worktree(repo, staging, "canon", origin_ref="main", log_fn=lambda *a: None)
        marker = staging / "IN_PROGRESS_MARKER"
        marker.write_text("merger mid-poll\n")

        branch = ensure_squad_staging_branch(repo, "canon", home, log_fn=lambda *a: None, origin_ref="main")

        self.assertEqual(branch, "squad/canon")
        self.assertTrue(marker.exists())
        self.assertEqual(marker.read_text(), "merger mid-poll\n")


# ---------------------------------------------------------------------------
# Janitor (spec M5)
# ---------------------------------------------------------------------------

class DiscoverWorktreeCandidatesTests(GitRepoTestCase):
    def test_finds_worktrees_under_base_paired_with_their_branch(self):
        repo = self.make_repo()
        base = self.tmp / "parallel-fix"
        base.mkdir()
        wt = base / "model-fix-canon-1"
        git(repo, "worktree", "add", "-b", "model-fix-parallel-canon-1", str(wt), "main")

        candidates = discover_worktree_candidates(base, repo)

        self.assertEqual(len(candidates), 1)
        self.assertEqual(candidates[0]["branch"], "model-fix-parallel-canon-1")
        self.assertEqual(Path(candidates[0]["path"]).resolve(), wt.resolve())

    def test_excludes_worktrees_outside_base(self):
        repo = self.make_repo()
        base = self.tmp / "parallel-fix"
        base.mkdir()
        outside = self.tmp / "elsewhere" / "model-fix-canon-1"
        git(repo, "worktree", "add", "-b", "model-fix-parallel-canon-1", str(outside), "main")

        candidates = discover_worktree_candidates(base, repo)

        self.assertEqual(candidates, [])

    def test_empty_base_returns_empty(self):
        repo = self.make_repo()
        base = self.tmp / "parallel-fix"
        base.mkdir()
        self.assertEqual(discover_worktree_candidates(base, repo), [])


class IsWorktreeStaleAndResolvedTests(GitRepoTestCase):
    OLD = 1_000_000  # epoch seconds, far in the past -- always ">3 days ago"

    def _branch_from(self, repo, branch, base="main"):
        git(repo, "branch", branch, base)

    def _old_shared_base(self, repo):
        """Backdate the commit both `main` and the worker branch will
        share as their merge-base -- staleness is measured from the
        MERGE-BASE's commit date, not from whichever per-branch commit
        happens to carry a `when=`, so the shared ancestor itself must
        be old for any of these fixtures to actually exercise the
        ">3 days" arm instead of always seeing a merge-base created at
        real "now" by make_repo()."""
        self.commit_file(repo, "shared.txt", "shared", "old shared base", when=self.OLD)

    def test_false_when_too_young(self):
        repo = self.make_repo()
        self._branch_from(repo, "model-fix-parallel-canon-1")
        git(repo, "checkout", "-q", "model-fix-parallel-canon-1")
        self.commit_file(repo, "x.txt", "1", "unresolved")
        git(repo, "checkout", "-q", "main")

        self.assertFalse(is_worktree_stale_and_resolved(
            repo_root=repo, branch="model-fix-parallel-canon-1", origin_ref="main",
            now_fn=lambda: time.time(),
        ))

    def test_false_when_stale_but_carries_an_unresolved_commit(self):
        repo = self.make_repo()
        self._old_shared_base(repo)
        self._branch_from(repo, "model-fix-parallel-canon-1")
        git(repo, "checkout", "-q", "model-fix-parallel-canon-1")
        self.commit_file(repo, "x.txt", "1", "unresolved")
        git(repo, "checkout", "-q", "main")

        self.assertFalse(is_worktree_stale_and_resolved(
            repo_root=repo, branch="model-fix-parallel-canon-1", origin_ref="main",
            staleness_seconds=10, now_fn=time.time,
        ))

    def test_true_when_stale_and_commit_already_landed_on_origin_by_patch_id(self):
        repo = self.make_repo()
        self._old_shared_base(repo)
        self._branch_from(repo, "model-fix-parallel-canon-1")
        git(repo, "checkout", "-q", "model-fix-parallel-canon-1")
        self.commit_file(repo, "x.txt", "1", "same change")
        git(repo, "checkout", "-q", "main")
        # The identical change lands on "origin/main" (main, in this
        # test's stand-in) via its own independent commit -- same
        # patch-id, different sha.
        self.commit_file(repo, "x.txt", "1", "same change landed on main")

        self.assertTrue(is_worktree_stale_and_resolved(
            repo_root=repo, branch="model-fix-parallel-canon-1", origin_ref="main",
            staleness_seconds=10, now_fn=time.time,
        ))

    def test_true_when_stale_and_commit_recorded_in_squad_status(self):
        repo = self.make_repo()
        self._old_shared_base(repo)
        self._branch_from(repo, "model-fix-parallel-canon-1")
        git(repo, "checkout", "-q", "model-fix-parallel-canon-1")
        sha = self.commit_file(repo, "x.txt", "1", "quarantined elsewhere")
        git(repo, "checkout", "-q", "main")

        self.assertTrue(is_worktree_stale_and_resolved(
            repo_root=repo, branch="model-fix-parallel-canon-1", origin_ref="main",
            squad_status={"heads": {sha: {"status": "quarantined"}}},
            staleness_seconds=10, now_fn=time.time,
        ))

    def test_true_when_stale_and_commit_patch_id_in_quarantine_ledger(self):
        repo = self.make_repo()
        self._old_shared_base(repo)
        self._branch_from(repo, "model-fix-parallel-canon-1")
        git(repo, "checkout", "-q", "model-fix-parallel-canon-1")
        sha = self.commit_file(repo, "x.txt", "1", "quarantined change")
        git(repo, "checkout", "-q", "main")
        import squad_merge_loop as sml
        patch_id = sml.compute_patch_id_for_sha(repo, sha)

        self.assertTrue(is_worktree_stale_and_resolved(
            repo_root=repo, branch="model-fix-parallel-canon-1", origin_ref="main",
            quarantine_entries={patch_id: {"reason": "bad"}},
            staleness_seconds=10, now_fn=time.time,
        ))

    def test_no_commits_at_all_is_trivially_resolved(self):
        repo = self.make_repo()
        self._branch_from(repo, "model-fix-parallel-canon-1")
        self.assertTrue(is_worktree_stale_and_resolved(
            repo_root=repo, branch="model-fix-parallel-canon-1", origin_ref="main",
            staleness_seconds=10, now_fn=lambda: time.time() + 10_000_000,
        ))

    def test_nonexistent_branch_is_false(self):
        repo = self.make_repo()
        self.assertFalse(is_worktree_stale_and_resolved(
            repo_root=repo, branch="does-not-exist", origin_ref="main",
        ))


class ResetStaleWorktreeAndJanitorResetTests(GitRepoTestCase):
    def test_reset_stale_worktree_checks_out_base_ref(self):
        repo = self.make_repo()
        git(repo, "branch", "model-fix-parallel-canon-1", "main")
        wt = self.tmp / "wt"
        git(repo, "worktree", "add", str(wt), "model-fix-parallel-canon-1")
        (wt / "untracked.txt").write_text("junk")

        logged = []
        reset_stale_worktree(repo, wt, "model-fix-parallel-canon-1", "main", log_fn=logged.append)

        self.assertFalse((wt / "untracked.txt").exists())
        self.assertTrue(any("reset" in line for line in logged))

    def test_janitor_reset_stale_worktrees_only_touches_eligible_entries(self):
        repo = self.make_repo()
        home = self.tmp / "home"
        # Backdate the commit `main` and both worker branches share as
        # their merge-base -- see IsWorktreeStaleAndResolvedTests's own
        # _old_shared_base for why this (not the per-branch commits)
        # is what staleness is actually measured from.
        self.commit_file(repo, "shared.txt", "shared", "old shared base", when=1_000_000)

        # Squad worktree: stale, fully resolved (recorded consumed).
        git(repo, "branch", "model-fix-parallel-canon-1", "main")
        git(repo, "checkout", "-q", "model-fix-parallel-canon-1")
        resolved_sha = self.commit_file(repo, "canon.txt", "1", "resolved fix")
        git(repo, "checkout", "-q", "main")
        status_path = home / "logs" / "squad-status" / "canon.json"
        status_path.parent.mkdir(parents=True)
        status_path.write_text(json.dumps({"heads": {resolved_sha: {"status": "consumed"}}}))
        git(repo, "branch", "squad/canon", "main")

        # Squad worktree: stale but carries an unresolved commit -- must
        # never be touched.
        git(repo, "branch", "model-fix-parallel-nikon-1", "main")
        git(repo, "checkout", "-q", "model-fix-parallel-nikon-1")
        self.commit_file(repo, "nikon.txt", "1", "unresolved fix")
        git(repo, "checkout", "-q", "main")

        candidates = [
            {"path": repo, "branch": "model-fix-parallel-canon-1"},
            {"path": repo, "branch": "model-fix-parallel-nikon-1"},
        ]
        reset_calls = []

        def fake_reset(repo_root, path, branch, base_ref, log_fn=print):
            reset_calls.append((branch, base_ref))

        result = janitor_reset_stale_worktrees(
            repo_root=repo, worktree_candidates=candidates, home=home, origin_ref="main",
            staleness_seconds=10, now_fn=time.time, reset_fn=fake_reset,
        )

        self.assertEqual(reset_calls, [("model-fix-parallel-canon-1", "squad/canon")])
        self.assertEqual(len(result), 1)


class ClearHeldByFoundationTests(GitRepoTestCase):
    def test_clears_when_foundation_sha_is_on_origin_ref(self):
        repo = self.make_repo()
        landed_sha = self.commit_file(repo, "f.txt", "1", "foundation landed")
        state_path = self.tmp / "tag-state.json"
        state_path.write_text(json.dumps({
            "JPEG:Foo": {"held_by_foundation": {"job": "cr3-quicktime", "sha": landed_sha}},
        }))

        cleared = clear_held_by_foundation(state_path, repo, origin_ref="main")

        self.assertEqual(cleared, ["JPEG:Foo"])
        state = json.loads(state_path.read_text())
        self.assertNotIn("held_by_foundation", state["JPEG:Foo"])

    def test_leaves_flag_when_foundation_sha_is_not_on_origin_ref(self):
        repo = self.make_repo()
        git(repo, "branch", "other", "main")
        git(repo, "checkout", "-q", "other")
        pending_sha = self.commit_file(repo, "g.txt", "1", "foundation not yet landed")
        git(repo, "checkout", "-q", "main")
        state_path = self.tmp / "tag-state.json"
        state_path.write_text(json.dumps({
            "JPEG:Bar": {"held_by_foundation": {"job": "flir-fff", "sha": pending_sha}},
        }))

        cleared = clear_held_by_foundation(state_path, repo, origin_ref="main")

        self.assertEqual(cleared, [])
        state = json.loads(state_path.read_text())
        self.assertEqual(state["JPEG:Bar"]["held_by_foundation"]["sha"], pending_sha)

    def test_no_op_when_nothing_is_held(self):
        repo = self.make_repo()
        state_path = self.tmp / "tag-state.json"
        state_path.write_text(json.dumps({"JPEG:Baz": {"fails": 0, "blacklisted": False}}))

        cleared = clear_held_by_foundation(state_path, repo, origin_ref="main")

        self.assertEqual(cleared, [])


CONFIG = {
    "base_url": "u", "api_key": "k", "models": [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}],
    "max_tokens": 4096, "reasoning_effort": "max",
}


class AttemptFoundationJobHeldByFoundationIntegrationTests(GitRepoTestCase):
    """Spec S3 item 2 / M5 janitor interop: a foundation-job-set
    held_by_foundation flag must get cleared once its commit sha reaches
    origin/main -- attempt_foundation_job (model_fix_loop.py, Phase 4/5)
    writes the flag; clear_held_by_foundation (parallel_model_fix_loop.py,
    Phase 3, already landed) clears it. End to end, against a real git
    tempdir repo (no mocked git plumbing) -- proves the two Phase 3/4/5
    pieces actually interoperate, not just that each unit-tests green in
    isolation."""

    def test_foundation_job_set_flag_is_cleared_once_its_commit_lands_on_origin_main(self):
        repo = self.make_repo()
        state_path = self.tmp / "tag-state.json"
        save_tag_state(state_path, {
            "JPEG:FLIR:Temp": {"fails": 0, "blacklisted": False, "canonical_module": "FLIR"},
            "JPEG:Canon:Other": {"fails": 0, "blacklisted": False, "canonical_module": "Canon"},
        })

        def fake_attempt_build(messages, *, git_apply_fn, repo_root, **kwargs):
            # A real file change, staged for the REAL git_commit_fn below
            # to actually commit -- proves a real sha, not a fake string.
            (Path(repo_root) / "flir_fff.rs").write_text("// FFF record walker\n")
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        job = {
            "name": "flir-fff-record-parser",
            "description": "Port the FLIR FFF record walker.",
            "target_formats": ["JPEG"],
            "target_module": "FLIR",
            "estimated_gaps": 90,
            "status": "pending",
        }

        result = attempt_foundation_job(
            job, repo, CONFIG,
            attempt_build_fn=fake_attempt_build,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            review_fn=lambda g, diff, config, **kw: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=git_commit,  # the REAL git_commit -- an actual commit lands
            log_fn=lambda *a: None,
            state_path=state_path,
        )

        self.assertEqual(result["status"], "fixed")
        commit_sha = result["commit_sha"]
        self.assertTrue(commit_sha)
        self.assertEqual(result["held_tags"], ["JPEG:FLIR:Temp"])

        state = load_tag_state(state_path)
        self.assertEqual(
            state["JPEG:FLIR:Temp"]["held_by_foundation"], {"job": job["name"], "sha": commit_sha},
        )
        self.assertNotIn("held_by_foundation", state["JPEG:Canon:Other"])

        # The commit already sits on "main" in this tempdir repo (git_commit
        # committed directly onto whatever branch was checked out) --
        # standing in for "reached origin/main". The Phase 3 janitor's own
        # clear_held_by_foundation must now clear the flag.
        cleared = clear_held_by_foundation(state_path, repo, origin_ref="main")
        self.assertEqual(cleared, ["JPEG:FLIR:Temp"])
        state = load_tag_state(state_path)
        self.assertNotIn("held_by_foundation", state["JPEG:FLIR:Temp"])
        # Unrelated entries are untouched throughout.
        self.assertNotIn("held_by_foundation", state["JPEG:Canon:Other"])


class RotateDashboardLogTests(unittest.TestCase):
    def test_rotates_when_over_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "dashboard.log"
            path.write_bytes(b"x" * 100)
            rotated = rotate_dashboard_log(path, max_bytes=50)
            self.assertTrue(rotated)
            # copytruncate, NOT rename: the live path must still exist
            # (now empty) so a long-running writer's already-open fd
            # keeps appending to IT, not to a renamed file it can never
            # see again -- see the docstring.
            self.assertTrue(path.exists())
            self.assertEqual(path.stat().st_size, 0)
            self.assertEqual(path.with_name("dashboard.log.1").read_bytes(), b"x" * 100)

    def test_writer_fd_stays_valid_across_rotation(self):
        # The whole point of copytruncate over rename: a process holding
        # dashboard.log open (redirected stdout, one fd for its entire
        # life, no reopen hook) must keep writing to the LIVE path after
        # rotation, never to the renamed .1 file.
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "dashboard.log"
            path.write_bytes(b"x" * 100)
            writer = open(path, "ab")
            try:
                rotate_dashboard_log(path, max_bytes=50)
                writer.write(b"written-after-rotation\n")
                writer.flush()
            finally:
                writer.close()
            self.assertIn(b"written-after-rotation", path.read_bytes())
            self.assertNotIn(b"written-after-rotation", path.with_name("dashboard.log.1").read_bytes())

    def test_no_op_under_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "dashboard.log"
            path.write_bytes(b"x" * 10)
            rotated = rotate_dashboard_log(path, max_bytes=50)
            self.assertFalse(rotated)
            self.assertTrue(path.exists())

    def test_no_op_when_missing(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "dashboard.log"
            self.assertFalse(rotate_dashboard_log(path, max_bytes=50))


class PruneModelFixRequestsTests(unittest.TestCase):
    def test_prunes_only_entries_older_than_max_age(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = Path(tmpdir)
            old = d / "old-request.json"
            new = d / "new-request.json"
            old.write_text("{}")
            new.write_text("{}")
            old_time = time.time() - (20 * 24 * 3600)
            os.utime(old, (old_time, old_time))

            pruned = prune_model_fix_requests(d, max_age_seconds=14 * 24 * 3600)

            self.assertEqual(pruned, [old])
            self.assertFalse(old.exists())
            self.assertTrue(new.exists())

    def test_never_prunes_keep_names(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = Path(tmpdir)
            manifest = d / "manifest.log"
            manifest.write_text("x")
            old_time = time.time() - (30 * 24 * 3600)
            os.utime(manifest, (old_time, old_time))

            pruned = prune_model_fix_requests(d, max_age_seconds=14 * 24 * 3600)

            self.assertEqual(pruned, [])
            self.assertTrue(manifest.exists())

    def test_missing_directory_is_a_no_op(self):
        self.assertEqual(prune_model_fix_requests("/does/not/exist"), [])


class RunJanitorTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.prune_model_fix_requests")
    @patch("parallel_model_fix_loop.rotate_dashboard_log")
    @patch("parallel_model_fix_loop.clear_held_by_foundation")
    @patch("parallel_model_fix_loop.janitor_reset_stale_worktrees")
    @patch("parallel_model_fix_loop.discover_worktree_candidates")
    def test_wires_every_sub_action_and_reports_a_summary(
        self, mock_discover, mock_reset, mock_clear, mock_rotate, mock_prune,
    ):
        mock_discover.return_value = [{"path": Path("/wt"), "branch": "b"}]
        mock_reset.return_value = [(Path("/wt"), "b")]
        mock_clear.return_value = ["JPEG:Foo"]
        mock_rotate.return_value = True
        mock_prune.return_value = [Path("/old.json")]

        with tempfile.TemporaryDirectory() as tmpdir:
            result = run_janitor(
                repo_root=Path(tmpdir), home=Path(tmpdir) / "home",
                worktree_base=Path(tmpdir) / "wt-base",
            )

        self.assertEqual(result, {
            "worktrees_reset": [(Path("/wt"), "b")],
            "held_by_foundation_cleared": ["JPEG:Foo"],
            "dashboard_rotated": True,
            "requests_pruned": [Path("/old.json")],
        })
        mock_discover.assert_called_once()
        mock_reset.assert_called_once()
        mock_clear.assert_called_once()
        mock_rotate.assert_called_once()
        mock_prune.assert_called_once()

    def test_every_sub_action_no_ops_safely_when_nothing_qualifies(self):
        # No injected fakes at all: real (but harmless, since everything
        # is empty/missing) sub-actions against a fresh tempdir.
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            result = run_janitor(
                repo_root=tmp, home=tmp / "home", worktree_base=tmp / "wt-base",
                tag_state_path=tmp / "tag-state.json", dashboard_log_path=tmp / "dashboard.log",
                requests_dir=tmp / "model-fix-requests",
            )
        self.assertEqual(result["worktrees_reset"], [])
        self.assertEqual(result["held_by_foundation_cleared"], [])
        self.assertFalse(result["dashboard_rotated"])
        self.assertEqual(result["requests_pruned"], [])


class RunJanitorSafelyTests(unittest.TestCase):
    def test_swallows_and_logs_a_raising_janitor_fn(self):
        logged = []

        def boom(**kwargs):
            raise RuntimeError("kaboom")

        parallel_model_fix_loop._run_janitor_safely(janitor_fn=boom, janitor_kwargs={}, log_fn=logged.append)

        self.assertTrue(any("kaboom" in line for line in logged))

    def test_calls_through_with_kwargs_on_success(self):
        calls = []
        parallel_model_fix_loop._run_janitor_safely(
            janitor_fn=lambda **kw: calls.append(kw), janitor_kwargs={"a": 1}, log_fn=lambda *a: None,
        )
        self.assertEqual(calls, [{"a": 1}])


if __name__ == "__main__":
    unittest.main()
