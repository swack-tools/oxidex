import json
import os
import signal
import tempfile
import unittest
from unittest.mock import patch, MagicMock
from pathlib import Path

import parallel_model_fix_loop
from parallel_model_fix_loop import (
    _kill_all_active_workers,
    _kill_process_group,
    _process_group_alive,
    _register_pgid,
    _unregister_pgid,
    _wait_for_process_group_exit,
    acquire_dispatcher_lock,
    branch_name,
    commits_on_branch,
    create_worktree,
    ensure_integration_branch,
    fast_forward_local_main,
    main,
    merge_branch,
    novel_commits,
    reap_orphan_worker_pgids,
    worktree_path,
)


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


if __name__ == "__main__":
    unittest.main()
