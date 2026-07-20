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
    branch_name,
    commits_on_branch,
    create_worktree,
    main,
    merge_branch,
    worktree_path,
)


class MainInfiniteLoopTests(unittest.TestCase):
    def _config_path(self, tmpdir):
        config_path = Path(tmpdir) / "config.toml"
        config_path.write_text('[worker]\nmodels = ["m"]\n')
        return config_path

    def test_runs_a_single_round_by_default(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            calls = []
            exit_code = main(
                ["--config", str(config_path)],
                run_round_fn=lambda args, cfg: calls.append(1) or True,
            )
            self.assertEqual(calls, [1])
            self.assertEqual(exit_code, 0)

    def test_single_round_returns_1_when_round_reports_failure(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            exit_code = main(
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
                main(
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
                main(
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
                main(
                    ["--config", str(config_path), "--infinite"],
                    run_round_fn=fake_run_round,
                    sleep_fn=lambda s: self.fail("should not sleep when round-delay is 0"),
                )

    def test_missing_config_returns_1_without_running_a_round(self):
        exit_code = main(
            ["--config", "/nonexistent/path/config.toml"],
            run_round_fn=lambda args, cfg: self.fail("should not run a round"),
        )
        self.assertEqual(exit_code, 1)


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
        mock_run.return_value = MagicMock(returncode=0)
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


class MergeBranchTests(unittest.TestCase):
    @patch("parallel_model_fix_loop.subprocess.run")
    def test_merges_and_passes_when_tests_pass(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        merged, message = merge_branch(Path("/fake/repo"), "model-fix-parallel-nef", cargo_test_fn=lambda: True)
        self.assertTrue(merged)
        self.assertEqual(message, "merged")
        merge_call = mock_run.call_args_list[0]
        self.assertEqual(
            merge_call.args[0],
            ["git", "merge", "--no-ff", "model-fix-parallel-nef", "-m", "merge: model-fix-parallel-nef"],
        )
        # only the merge itself ran -- no abort, no reset --hard
        all_argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertNotIn(["git", "merge", "--abort"], all_argvs)
        self.assertFalse(any(argv[:3] == ["git", "reset", "--hard"] for argv in all_argvs))

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_aborts_on_merge_conflict_without_running_tests(self, mock_run):
        cargo_test_calls = []

        def merge_conflicts(argv, **kwargs):
            if argv[:2] == ["git", "merge"] and "--abort" not in argv:
                return MagicMock(returncode=1, stdout="", stderr="CONFLICT (content): x.rs")
            return MagicMock(returncode=0, stdout="", stderr="")

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
    def test_rolls_back_merge_when_tests_regress(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")

        merged, message = merge_branch(Path("/fake/repo"), "model-fix-parallel-nef", cargo_test_fn=lambda: False)

        self.assertFalse(merged)
        self.assertIn("regressed", message)
        all_argvs = [c.args[0] for c in mock_run.call_args_list]
        self.assertIn(["git", "reset", "--hard", "HEAD~1"], all_argvs)
        # the merge itself was NOT aborted (it happened; only the resulting
        # commit is rolled back afterward) -- no "git merge --abort" call
        self.assertNotIn(["git", "merge", "--abort"], all_argvs)


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


if __name__ == "__main__":
    unittest.main()
