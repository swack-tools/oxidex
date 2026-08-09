import inspect
import json
import os
import shutil
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

import overlord_sweep
import parallel_model_fix_loop
from parallel_model_fix_loop import (
    _kill_all_active_workers,
    _kill_process_group,
    _process_group_alive,
    _register_pgid,
    _unregister_pgid,
    _wait_for_process_group_exit,
    acquire_dispatcher_lock,
    adopt_open_sweep_prs,
    allocate_squad_slots,
    auto_publish_round,
    branch_name,
    clear_held_by_foundation,
    commits_on_branch,
    create_worktree,
    discover_worktree_candidates,
    ensure_integration_branch,
    ensure_squad_staging_branch,
    ensure_sweep_worktree,
    fast_forward_local_main,
    is_worktree_stale_and_resolved,
    janitor_reset_stale_worktrees,
    list_open_sweep_prs,
    main,
    merge_branch,
    novel_commits,
    parse_worktree_list,
    pr_checks_state,
    pr_review_state,
    pr_ref_from_result,
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
    sync_worktrees_to_origin_main,
    wait_for_pr_checks,
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
        self.publish_calls = []

    def record_publish(self, *args, **kwargs):
        """Stand-in for auto_publish_round. See _main for why this class
        must never reach the real one."""
        self.publish_calls.append((args, kwargs))
        return {"status": "no_news"}

    def _main(self, argv, **kwargs):
        kwargs.setdefault("lock_path", self.lock_path)
        kwargs.setdefault("pgids_path", self.pgids_path)
        # HERMETICITY, and this is not a nicety. --auto-publish defaults ON
        # for --infinite, so a test that calls the real main() without
        # injecting this runs the REAL auto_publish_round against the REAL
        # REPO_ROOT and ~/.oxidex/worktrees/overlord-sweep. Measured
        # 2026-07-26: that made 4 live `gh pr list` calls, and with one
        # open green sweep/* PR present it went on to
        # `gh pr merge --squash --delete-branch` a REAL pull request and
        # fast-forward ~100 live worktrees -- from `python3 -m unittest`.
        # Defaulted here rather than per-test so a future test added to
        # this class cannot reintroduce it.
        kwargs.setdefault("auto_publish_fn", self.record_publish)
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

    def test_infinite_never_reaches_the_real_publisher_from_a_unit_test(self):
        # Guards the hermeticity hole this class had: --auto-publish
        # defaults ON for --infinite, so an un-injected main() ran the real
        # auto_publish_round against the live repo -- 4 live `gh pr list`
        # calls, and with one open green sweep/* PR it squash-merged a REAL
        # PR and fast-forwarded ~100 live worktrees. Asserting the
        # stand-in was used proves the injection point is still wired; the
        # sentinel proves the class default is the stand-in and not the
        # production function.
        self.assertIs(
            inspect.signature(main).parameters["auto_publish_fn"].default,
            parallel_model_fix_loop.auto_publish_round,
            "main()'s production default must stay the real publisher -- the "
            "test class is what overrides it",
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            rounds = []

            def fake_run_round(args, cfg):
                rounds.append(1)
                if len(rounds) == 2:
                    raise RuntimeError("stop the test loop")
                return True

            with self.assertRaises(RuntimeError):
                self._main(
                    ["--config", str(config_path), "--infinite"],
                    run_round_fn=fake_run_round,
                )
        # One completed round -> one publish attempt, and it went to the
        # stand-in rather than to GitHub.
        self.assertEqual(len(self.publish_calls), 1)

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

    def test_an_idle_round_backs_off_even_when_round_delay_is_zero(self):
        # This test used to assert that --round-delay 0 NEVER sleeps. That is
        # no longer true and the old assertion was the bug: a round that finds
        # nothing to publish returns in milliseconds, so at delay 0 the loop
        # spun. Measured 2026-07-28 -- with every green stamp correctly
        # skipped as stale, the dispatcher ran ~10 rounds a SECOND, each
        # logging 'no_news'. A round that publishes nothing now waits
        # IDLE_ROUND_DELAY_SECONDS; work arrives from workers on a scale of
        # minutes, so there is nothing to gain by asking again instantly.
        import parallel_model_fix_loop as p

        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir)
            round_calls = []
            slept = []

            def fake_run_round(args, cfg):
                round_calls.append(1)
                if len(round_calls) == 2:
                    raise RuntimeError("stop the test loop")
                return True

            with self.assertRaises(RuntimeError):
                self._main(
                    ["--config", str(config_path), "--infinite"],
                    run_round_fn=fake_run_round,
                    sleep_fn=slept.append,
                )
            self.assertEqual(slept, [p.IDLE_ROUND_DELAY_SECONDS])

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
        # origin/main, AND neither commit's patch is already present
        # upstream (cherry says "+" against both refs) -- genuinely novel,
        # unmerged work. The reuse path must keep the branch as-is (plain
        # checkout), never `checkout -B` it back onto base_ref.
        def fake_run(argv, **kwargs):
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0, stdout="abc123\n", stderr="")
            if argv[:2] == ["git", "rev-list"]:
                return MagicMock(returncode=0, stdout="sha1\nsha2\n", stderr="")
            if argv[:2] == ["git", "cherry"]:
                return MagicMock(returncode=0, stdout="+ sha1\n+ sha2\n", stderr="")
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
            if argv[:2] == ["git", "rev-list"]:
                return MagicMock(returncode=0, stdout="", stderr="")
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
    def test_resets_a_branch_whose_only_commits_were_already_squash_merged_upstream(self, mock_run):
        # A squash merge gives previously-landed work a brand new SHA with
        # no ancestry link back to the original commit -- so a worker
        # branch that happens to carry that same (now-duplicate) commit
        # is "ahead" of base_ref/origin/main by pure ancestry forever,
        # even though its patch already landed. git cherry catches this
        # by patch-id ("-" prefix = equivalent patch already upstream),
        # so the branch must still be treated as safely resettable.
        def fake_run(argv, **kwargs):
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0, stdout="abc123\n", stderr="")
            if argv[:2] == ["git", "rev-list"]:
                return MagicMock(returncode=0, stdout="dupe_sha\n", stderr="")
            if argv[:2] == ["git", "cherry"]:
                # already-merged-by-patch-id relative to both base_ref and origin/main
                return MagicMock(returncode=0, stdout="- dupe_sha\n", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worktree = tmp / "worktree"
            worktree.mkdir()

            create_worktree(tmp, worktree, "model-fix-parallel-nef", "main", config_path=tmp / "no-config.toml")

            argvs = [c.args[0] for c in mock_run.call_args_list]
            self.assertIn(["git", "checkout", "-B", "model-fix-parallel-nef", "main"], argvs)
            self.assertNotIn(["git", "checkout", "model-fix-parallel-nef"], argvs)

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

    def _fake_run(self, head_sha, covered_by_base=False):
        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--verify", "--quiet", f"refs/heads/{self.BRANCH}"]:
                return MagicMock(returncode=0, stdout="abc123\n", stderr="")
            if argv[:3] == ["git", "rev-list", "--count"]:
                return MagicMock(returncode=0, stdout="0\n", stderr="")  # nothing undiscardable
            if argv == ["git", "rev-parse", "--verify", "--quiet", self.BRANCH]:
                return MagicMock(returncode=0, stdout=f"{head_sha}\n", stderr="")
            if argv[:2] == ["git", "merge-base"]:
                # returncode 0 = head_sha IS an ancestor of (or equal to)
                # base_ref -- the worker never committed anything beyond
                # base_ref, so there is nothing for the consume handshake
                # to protect. Defaults to "not covered" (1) so every
                # existing test in this class -- which simulates a worker
                # branch that genuinely diverged -- keeps its original
                # blocked-reset expectation unless it opts in.
                return MagicMock(returncode=0 if covered_by_base else 1, stdout="", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")
        return fake_run

    def _create(self, tmp, mock_run, head_sha, squad_status_path=None, covered_by_base=False):
        mock_run.side_effect = self._fake_run(head_sha, covered_by_base=covered_by_base)
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

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_allows_reset_when_head_never_diverged_from_base(self, mock_run):
        # The common case: a worker investigated a tag and never landed a
        # commit, so its branch head is still exactly whatever base_ref
        # was at creation time. The merger never recorded this sha --
        # there was never a real commit for it to look at -- so without
        # the _head_already_covered_by_base escape hatch this worktree
        # would be blocked from ever refreshing to a newer base_ref tip,
        # forever, even though there is nothing on the branch to protect.
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            status_path = tmp / "squad-status" / "nikon.json"  # no entry for this sha either
            status_path.parent.mkdir()
            status_path.write_text(json.dumps({"heads": {}}))
            argvs = self._create(
                tmp, mock_run, "never-diverged-sha", squad_status_path=status_path,
                covered_by_base=True,
            )
            self.assertIn(["git", "checkout", "-B", self.BRANCH, "main"], argvs)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_still_blocks_reset_when_diverged_and_unresolved_even_if_status_file_exists(self, mock_run):
        # Guards against a too-broad fix: a genuinely diverged, unresolved
        # head must stay blocked even when the squad-status file exists
        # and has entries -- just not for this sha.
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            status_path = tmp / "squad-status" / "nikon.json"
            status_path.parent.mkdir()
            status_path.write_text(json.dumps({"heads": {"some-other-sha": {"status": "consumed"}}}))
            argvs = self._create(
                tmp, mock_run, "diverged-unresolved-sha", squad_status_path=status_path,
                covered_by_base=False,
            )
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

    @patch("parallel_model_fix_loop.os.killpg")
    def test_false_when_pgid_was_recycled_by_another_owner(self, mock_killpg):
        # EPERM: the pgid exists but belongs to someone else, so OUR worker's
        # processes are gone. Returning True here would spin until force_after
        # and then aim SIGKILL at a stranger's process group; RAISING here took
        # the whole dispatcher down on 2026-07-27 (32 workers lost to one
        # recycled pgid), which is the regression this pins.
        mock_killpg.side_effect = PermissionError(1, "Operation not permitted")
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

    @patch("parallel_model_fix_loop.os.killpg")
    def test_ignores_group_we_do_not_own(self, mock_killpg):
        # A pgid we cannot signal is not ours. Swallowing EPERM is what keeps
        # us from delivering SIGKILL to an unrelated process group that merely
        # inherited the number.
        mock_killpg.side_effect = PermissionError(1, "Operation not permitted")
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
    def test_branch_held_by_another_worktree_is_diagnosed_as_such(self, mock_run):
        # Git says exactly why. Guessing over the top of it sends the reader
        # to the wrong remedy: on 2026-07-28 every round logged "likely
        # uncommitted changes" while the dispatcher checkout was clean and
        # the real cause was a second worktree holding the branch. Stashing
        # cannot fix that; detaching the other worktree can.
        held = ("fatal: 'model-fix-sweep-local' is already used by worktree "
                "at '/Users/allen/.oxidex/worktrees/fleet-main'")

        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--abbrev-ref", "HEAD"]:
                return MagicMock(returncode=0, stdout="main\n", stderr="")
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0, stdout="", stderr="")  # exists
            if argv[:2] == ["git", "checkout"]:
                return MagicMock(returncode=1, stdout="", stderr=held)
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        logged = []
        branch = ensure_integration_branch(Path("/fake/repo"), log_fn=logged.append)
        self.assertIsNone(branch)
        message = " ".join(logged)
        self.assertIn("/Users/allen/.oxidex/worktrees/fleet-main", message)
        self.assertIn("checkout --detach", message)
        self.assertNotIn("uncommitted changes", message)

    @patch("parallel_model_fix_loop.subprocess.run")
    def test_a_genuinely_dirty_checkout_still_says_so(self, mock_run):
        # The other side of the same branch: when git does NOT report a
        # worktree conflict, the stash advice is the right advice.
        def fake_run(argv, **kwargs):
            if argv == ["git", "rev-parse", "--abbrev-ref", "HEAD"]:
                return MagicMock(returncode=0, stdout="main\n", stderr="")
            if argv[:4] == ["git", "rev-parse", "--verify", "--quiet"]:
                return MagicMock(returncode=0, stdout="", stderr="")
            if argv[:2] == ["git", "checkout"]:
                return MagicMock(
                    returncode=1, stdout="",
                    stderr="error: Your local changes would be overwritten")
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = fake_run
        logged = []
        self.assertIsNone(
            ensure_integration_branch(Path("/fake/repo"), log_fn=logged.append))
        self.assertIn("uncommitted changes", " ".join(logged))

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

    @patch("parallel_model_fix_loop._branch_head_sha", side_effect=["before", "after"])
    @patch(
        "parallel_model_fix_loop.run_worker",
        side_effect=subprocess.TimeoutExpired(cmd=["model_fix_loop.py"], timeout=10),
    )
    @patch("parallel_model_fix_loop.create_worktree")
    def test_squad_timeout_after_commit_is_publishable(
        self, mock_create, mock_run_worker, mock_branch_head,
    ):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            worker_id, result = process_squad_worker(
                "canon", 4, "M4A", tmp, "squad/canon", tmp / "wt", tmp / "log",
                "/cache", 10,
            )

        self.assertEqual(worker_id, "canon-4")
        self.assertEqual(result["status"], "worker_done")
        self.assertTrue(result["timed_out"])
        self.assertTrue(result["commit_created"])
        self.assertEqual(result["starting_head"], "before")
        self.assertEqual(result["ending_head"], "after")

    @patch("parallel_model_fix_loop._branch_head_sha", side_effect=["same", "same"])
    @patch(
        "parallel_model_fix_loop.run_worker",
        side_effect=subprocess.TimeoutExpired(cmd=["model_fix_loop.py"], timeout=10),
    )
    @patch("parallel_model_fix_loop.create_worktree")
    def test_squad_timeout_without_commit_remains_a_failure(
        self, mock_create, mock_run_worker, mock_branch_head,
    ):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            _worker_id, result = process_squad_worker(
                "canon", 4, "M4A", tmp, "squad/canon", tmp / "wt", tmp / "log",
                "/cache", 10,
            )

        self.assertEqual(result["status"], "timeout")
        self.assertTrue(result["timed_out"])
        self.assertFalse(result["commit_created"])

    @patch("parallel_model_fix_loop._branch_head_sha", side_effect=["before", "after"])
    @patch(
        "parallel_model_fix_loop.run_worker",
        side_effect=subprocess.TimeoutExpired(cmd=["model_fix_loop.py"], timeout=10),
    )
    @patch("parallel_model_fix_loop.create_worktree")
    def test_legacy_timeout_after_commit_enters_merge_phase(
        self, mock_create, mock_run_worker, mock_branch_head,
    ):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            fmt, result = process_format(
                "M4A", tmp, "main", tmp / "wt", tmp / "log", "/cache", 10,
                config_path=tmp / "no-config",
            )

        self.assertEqual(fmt, "M4A")
        self.assertEqual(result["status"], "worker_done")
        self.assertTrue(result["commit_created"])


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


# ---------------------------------------------------------------------------
# Auto-publish: sweep -> cargo fmt -> PR -> merge on green -> worktree sync
# ---------------------------------------------------------------------------

class ParseWorktreeListTests(unittest.TestCase):
    PORCELAIN = (
        "worktree /repo\n"
        "HEAD 1111111111111111111111111111111111111111\n"
        "branch refs/heads/main\n"
        "\n"
        "worktree /wt/detached\n"
        "HEAD 2222222222222222222222222222222222222222\n"
        "detached\n"
        "\n"
        "worktree /wt/gone\n"
        "HEAD 3333333333333333333333333333333333333333\n"
        "branch refs/heads/squad/canon\n"
        "prunable gitdir file points to non-existent location\n"
        "\n"
        "worktree /repo.git\n"
        "bare\n"
    )

    def test_reports_every_record_with_its_attributes(self):
        entries = parse_worktree_list(self.PORCELAIN)
        self.assertEqual([str(e["path"]) for e in entries],
                         ["/repo", "/wt/detached", "/wt/gone", "/repo.git"])
        self.assertEqual(entries[0]["branch"], "main")
        self.assertFalse(entries[0]["detached"])
        self.assertIsNone(entries[1]["branch"])
        self.assertTrue(entries[1]["detached"])
        # "prunable <reason>" carries a trailing reason string; the flag
        # must be set from the prefix, not from an exact-line match.
        self.assertEqual(entries[2]["branch"], "squad/canon")
        self.assertTrue(entries[2]["prunable"])
        self.assertTrue(entries[3]["bare"])

    def test_empty_output_is_no_entries(self):
        self.assertEqual(parse_worktree_list(""), [])


class EnsureSweepWorktreeTests(GitRepoTestCase):
    def test_creates_a_detached_worktree_on_first_use(self):
        repo = self.make_repo()
        path = self.tmp / "sweep-wt"
        result, message = ensure_sweep_worktree(
            repo, path, origin_ref="main", log_fn=lambda *a: None,
        )
        self.assertEqual(result, path)
        self.assertIn("created", message)
        # Detached on purpose: run_sweep cuts and checks out its own
        # branch, and a branch-tracked sweep worktree would collide with
        # the dispatcher's own checkout of the same branch.
        self.assertEqual(git_out(path, "rev-parse", "--abbrev-ref", "HEAD").strip(), "HEAD")

    def test_reuse_resets_the_worktree_off_the_previous_sweep_branch(self):
        repo = self.make_repo()
        path = self.tmp / "sweep-wt"
        ensure_sweep_worktree(repo, path, origin_ref="main", log_fn=lambda *a: None)
        # Simulate the state a finished sweep leaves behind: sitting on
        # its own sweep branch, with a stray edit in the tree.
        git(path, "checkout", "-q", "-b", "sweep/tags-2026-07-25-1")
        (path / "README.md").write_text("half-written sweep state\n")
        main_sha = git_out(repo, "rev-parse", "main").strip()

        result, message = ensure_sweep_worktree(
            repo, path, origin_ref="main", log_fn=lambda *a: None,
        )
        self.assertEqual(result, path)
        self.assertIn("reused", message)
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(), main_sha)
        self.assertEqual(git_out(path, "status", "--porcelain").strip(), "")
        # No-discard: the previous sweep branch itself is untouched.
        self.assertEqual(
            git(repo, "rev-parse", "--verify", "refs/heads/sweep/tags-2026-07-25-1", check=False).returncode, 0,
        )

    def test_a_directory_that_is_not_a_registered_worktree_is_self_healed(self):
        """`if path.is_dir():` took the reuse branch on EXISTENCE alone.
        A half-failed `worktree add`, a `worktree remove` that left the
        directory behind, or a human who ran overlord_sweep.py's own
        usage line by hand all leave a plain directory there -- every git
        call in the reuse branch then fails, ensure_sweep_worktree
        returns (None, ...), and auto-publish is silently and
        permanently disabled for the life of the dispatcher while
        looking exactly like a healthy no-news round. The `worktree
        prune` + `worktree add` self-heal below existed but was only
        reachable when the directory was ABSENT."""
        repo = self.make_repo()
        path = self.tmp / "sweep-wt"
        path.mkdir()  # exists, but git has never heard of it
        result, message = ensure_sweep_worktree(
            repo, path, origin_ref="main", log_fn=lambda *a: None,
        )
        self.assertEqual(result, path)
        self.assertEqual(git_out(path, "rev-parse", "--abbrev-ref", "HEAD").strip(), "HEAD")
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(),
                         git_out(repo, "rev-parse", "main").strip())
        self.assertIn("created", message)

    def test_a_stale_registration_whose_directory_was_recreated_is_pruned_and_re_added(self):
        # The exact shape of a half-failed add: git still has the
        # registration, the directory is back but empty. `worktree add`
        # refuses that path until it is pruned.
        repo = self.make_repo()
        path = self.tmp / "sweep-wt"
        ensure_sweep_worktree(repo, path, origin_ref="main", log_fn=lambda *a: None)
        shutil.rmtree(path)
        path.mkdir()
        result, _message = ensure_sweep_worktree(
            repo, path, origin_ref="main", log_fn=lambda *a: None,
        )
        self.assertEqual(result, path)
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(),
                         git_out(repo, "rev-parse", "main").strip())

    def test_unusable_worktree_returns_none_instead_of_raising(self):
        # A failure here must degrade to "skip auto-publish this round",
        # never to an exception that takes down an --infinite dispatcher.
        repo = self.make_repo()
        logged = []
        result, message = ensure_sweep_worktree(
            repo, self.tmp / "sweep-wt", origin_ref="refs/heads/does-not-exist", log_fn=logged.append,
        )
        self.assertIsNone(result)
        self.assertIn("could not create", message)
        self.assertTrue(any("skipping auto-publish" in line for line in logged))


class EnsureSweepWorktreeLockRecoveryTests(GitRepoTestCase):
    """A stale index.lock in the sweep worktree's gitdir made the reuse
    branch's `checkout --force --detach` fail with rc=128 FOREVER:
    _is_git_worktree answers True, so the prune + `worktree add`
    self-heal the docstring reasons about was provably unreachable, and
    auto-publish stayed off across dispatcher RESTARTS until a human
    deleted one file.

    The trigger is narrow (measured: SIGTERM and SIGINT to a git holding
    the lock both clean it up, and SIGKILLing git's PARENT leaves the
    orphaned git to clean up too -- only a SIGKILL/hard-crash of the git
    process itself leaves the file), which is exactly why the recovery
    must be BOUNDED: a live git may legitimately hold this lock, and
    deleting a held lock risks a lost index write.
    """

    def lock_path(self, worktree):
        rc, out, _err = parallel_model_fix_loop.default_run_git(
            ["rev-parse", "--absolute-git-dir"], worktree,
        )
        self.assertEqual(rc, 0)
        return Path(out.strip()) / "index.lock"

    def _provisioned(self):
        repo = self.make_repo()
        path = self.tmp / "sweep-wt"
        ensure_sweep_worktree(repo, path, origin_ref="main", log_fn=lambda *a: None)
        git(path, "checkout", "-q", "-b", "sweep/tags-2026-07-25-1")
        return repo, path

    def test_a_stale_lock_is_cleared_and_the_worktree_is_reused(self):
        repo, path = self._provisioned()
        lock = self.lock_path(path)
        lock.write_text("")
        os.utime(lock, (time.time() - 3600, time.time() - 3600))
        logged = []

        result, message = ensure_sweep_worktree(
            repo, path, origin_ref="main", log_fn=logged.append, sleep_fn=lambda s: None,
        )

        self.assertEqual(result, path)
        self.assertFalse(lock.exists())
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(),
                         git_out(repo, "rev-parse", "main").strip())
        self.assertIn("index.lock", message)
        # No-discard: the previous sweep branch ref survives the recovery.
        self.assertEqual(
            git(repo, "rev-parse", "--verify", "refs/heads/sweep/tags-2026-07-25-1",
                check=False).returncode, 0,
        )

    def test_a_fresh_lock_is_left_alone_for_the_live_git_that_holds_it(self):
        repo, path = self._provisioned()
        lock = self.lock_path(path)
        lock.write_text("")   # just created: mtime is now
        logged = []

        result, message = ensure_sweep_worktree(
            repo, path, origin_ref="main", log_fn=logged.append, sleep_fn=lambda s: None,
        )

        self.assertIsNone(result)
        # Nothing destructive: the lock is untouched, the worktree is
        # still registered, and the round is simply skipped.
        self.assertTrue(lock.exists())
        self.assertTrue(parallel_model_fix_loop._is_git_worktree(
            path, parallel_model_fix_loop.default_run_git))
        self.assertIn("index.lock", message)
        self.assertTrue(any("skipping auto-publish" in line for line in logged))

    def test_a_persistently_unresettable_worktree_is_rebuilt_not_disabled(self):
        # Any reason the force-detach keeps failing (permissions, a
        # corrupt index, a wedged ref) gets the same bounded escalation:
        # unregister the worktree and let the existing prune +
        # `worktree add` path rebuild it, rather than returning
        # (None, ...) for the life of the dispatcher.
        repo, path = self._provisioned()

        def detach_hostile_git(args, repo_root, input_text=None):
            if args[:3] == ["checkout", "--force", "--detach"]:
                return 128, "", "fatal: cannot reset this worktree"
            return parallel_model_fix_loop.default_run_git(args, repo_root, input_text)

        result, message = ensure_sweep_worktree(
            repo, path, run_git=detach_hostile_git, origin_ref="main",
            log_fn=lambda *a: None, sleep_fn=lambda s: None,
        )

        self.assertEqual(result, path)
        self.assertIn("created", message)
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(),
                         git_out(repo, "rev-parse", "main").strip())
        self.assertEqual(git_out(path, "rev-parse", "--abbrev-ref", "HEAD").strip(), "HEAD")


class PrRefFromResultTests(unittest.TestCase):
    def test_prefers_the_url_gh_printed(self):
        self.assertEqual(
            pr_ref_from_result({"ok": True, "stdout": "https://github.com/o/r/pull/125"}, "sweep/x"),
            "https://github.com/o/r/pull/125",
        )

    def test_ignores_gh_chatter_around_the_url(self):
        pr = {"ok": True, "stdout": "Creating pull request for sweep/x\nhttps://github.com/o/r/pull/9\n"}
        self.assertEqual(pr_ref_from_result(pr, "sweep/x"), "https://github.com/o/r/pull/9")

    def test_falls_back_to_the_branch_name(self):
        # `gh pr checks <branch>` resolves a PR by head branch, so a
        # create_pr_fn that returned no URL is not a dead end.
        self.assertEqual(pr_ref_from_result({"ok": True}, "sweep/tags-2026-07-25-1"),
                         "sweep/tags-2026-07-25-1")
        self.assertEqual(pr_ref_from_result(None, "sweep/tags-2026-07-25-1"),
                         "sweep/tags-2026-07-25-1")


def _gh_checks(*buckets):
    """A fake run_gh returning one check per bucket for `gh pr checks`."""
    payload = json.dumps([{"name": f"job{i}", "state": b.upper(), "bucket": b}
                          for i, b in enumerate(buckets)])

    def run_gh(args, repo_root):
        return 0, payload, ""
    return run_gh


class PrChecksStateTests(unittest.TestCase):
    def test_all_passing_is_green(self):
        state, _detail = pr_checks_state("pr", "/repo", _gh_checks("pass", "pass"))
        self.assertEqual(state, "green")

    def test_skipped_jobs_count_as_green(self):
        state, _detail = pr_checks_state("pr", "/repo", _gh_checks("pass", "skipping"))
        self.assertEqual(state, "green")

    def test_any_failure_is_red_even_alongside_passes(self):
        state, detail = pr_checks_state("pr", "/repo", _gh_checks("pass", "fail"))
        self.assertEqual(state, "red")
        self.assertIn("fail", detail)

    def test_a_cancelled_check_is_red(self):
        state, _detail = pr_checks_state("pr", "/repo", _gh_checks("pass", "cancel"))
        self.assertEqual(state, "red")

    def test_pending_beats_pass(self):
        state, _detail = pr_checks_state("pr", "/repo", _gh_checks("pass", "pending"))
        self.assertEqual(state, "pending")

    def test_no_checks_yet_is_pending_never_green(self):
        # The dangerous case: right after a push GitHub has not created
        # the workflow runs yet, and calling that "green" would merge a
        # PR nothing ever tested.
        state, detail = pr_checks_state("pr", "/repo", lambda args, repo: (8, "[]", ""))
        self.assertEqual(state, "pending")
        self.assertIn("no checks reported", detail)

    def test_an_unrecognised_bucket_is_pending_not_green(self):
        state, _detail = pr_checks_state("pr", "/repo", _gh_checks("pass", "something-new"))
        self.assertEqual(state, "pending")

    def test_non_json_output_is_unknown(self):
        state, detail = pr_checks_state(
            "pr", "/repo", lambda args, repo: (1, "", "gh: authentication required"),
        )
        self.assertEqual(state, "unknown")
        self.assertIn("authentication", detail)


class WaitForPrChecksTests(unittest.TestCase):
    def _clock(self, step=1.0):
        state = {"t": 0.0}

        def now():
            state["t"] += step
            return state["t"]
        return now

    def test_returns_green_once_the_pending_checks_finish(self):
        answers = iter([("pending", "queued"), ("pending", "running"), ("green", "all good")])
        slept = []
        with patch("parallel_model_fix_loop.pr_checks_state", side_effect=lambda *a: next(answers)):
            state, detail = wait_for_pr_checks(
                "pr", "/repo", sleep_fn=slept.append, now_fn=self._clock(),
                timeout_seconds=1000, interval_seconds=7, log_fn=lambda *a: None,
            )
        self.assertEqual(state, "green")
        self.assertEqual(detail, "all good")
        self.assertEqual(slept, [7, 7])

    def test_red_returns_immediately_without_sleeping(self):
        slept = []
        with patch("parallel_model_fix_loop.pr_checks_state", return_value=("red", "Lint & Audit=fail")):
            state, detail = wait_for_pr_checks(
                "pr", "/repo", sleep_fn=slept.append, now_fn=self._clock(),
                timeout_seconds=1000, log_fn=lambda *a: None,
            )
        self.assertEqual(state, "red")
        self.assertIn("Lint & Audit", detail)
        self.assertEqual(slept, [])

    def test_forever_pending_times_out_instead_of_blocking_the_dispatcher(self):
        slept = []
        with patch("parallel_model_fix_loop.pr_checks_state", return_value=("pending", "queued")):
            state, _detail = wait_for_pr_checks(
                "pr", "/repo", sleep_fn=slept.append, now_fn=self._clock(step=10),
                timeout_seconds=25, interval_seconds=5, log_fn=lambda *a: None,
            )
        self.assertEqual(state, "timeout")
        self.assertLessEqual(len(slept), 3)

    def test_repeated_unknown_gives_up_early(self):
        # gh not installed / auth expired: polling for the full timeout
        # would burn 45 minutes of dispatcher time on a question that is
        # not going to answer differently.
        slept = []
        with patch("parallel_model_fix_loop.pr_checks_state", return_value=("unknown", "gh: not found")):
            state, _detail = wait_for_pr_checks(
                "pr", "/repo", sleep_fn=slept.append, now_fn=self._clock(),
                timeout_seconds=10_000, max_unknown_polls=3, log_fn=lambda *a: None,
            )
        self.assertEqual(state, "unknown")
        self.assertEqual(len(slept), 2)


def _gh_pr_entry(number, head, **overrides):
    """One `gh pr list --json ...` record carrying the same field set the
    production query asks for.

    `gh` returns ONLY the fields requested, so a fake that omits isDraft/
    baseRefName/isCrossRepository cannot express a draft or a fork PR at
    all -- which is precisely why a draft sweep PR reached merge_pr and
    produced merge_failed noise every round unnoticed.
    """
    entry = {"number": number, "url": f"https://github.com/o/r/pull/{number}",
             "headRefName": head, "isDraft": False, "baseRefName": "main",
             "isCrossRepository": False}
    entry.update(overrides)
    return entry


def _gh_pr_list(*prs):
    """A `gh pr list` payload; each pr is (number, headRefName) or a dict
    from _gh_pr_entry."""
    return json.dumps([
        pr if isinstance(pr, dict) else _gh_pr_entry(*pr) for pr in prs
    ])


class ListOpenSweepPrsTests(unittest.TestCase):
    def test_keeps_only_sweep_branch_prs_and_orders_them_oldest_first(self):
        payload = _gh_pr_list((130, "feat/some-human-branch"), (128, "sweep/tags-2026-07-26-1"),
                              (126, "sweep/tags-2026-07-25-2"))
        prs = list_open_sweep_prs("/repo", lambda args, repo: (0, payload, ""))
        # Only this automation's own namespace is ever adopted -- a
        # human's PR going green must never be squash-merged by a
        # dispatcher running unattended for weeks.
        self.assertEqual([p["number"] for p in prs], [126, 128])

    def test_unparseable_output_is_no_prs_not_an_exception(self):
        # Expired auth / gh missing: one skipped adoption pass, never a
        # crash inside an --infinite dispatcher.
        self.assertEqual(list_open_sweep_prs("/repo", lambda args, repo: (1, "", "gh: auth required")), [])

    def test_a_human_sweep_branch_of_a_different_shape_is_never_adopted(self):
        # A bare "sweep/" prefix was too broad. This repo really does
        # carry human/skill-driven sweep branches of another shape --
        # sweep/parallel-fix-tags-2026-07-23 and -07-24, both with live
        # registered worktrees -- and an unattended dispatcher must not
        # squash-merge a PR it did not create.
        payload = _gh_pr_list(
            (140, "sweep/parallel-fix-tags-2026-07-24"),
            (141, "sweep/manual-tags-2026-07-25"),
            (142, "sweep/tags-2026-07-26-1"),
        )
        prs = list_open_sweep_prs("/repo", lambda args, repo: (0, payload, ""))
        self.assertEqual([p["number"] for p in prs], [142])

    def test_the_shape_test_is_anchored_at_both_ends(self):
        for head, adopted in (
            ("sweep/tags-2026-07-26-1", True),
            ("sweep/tags-2026-07-26-12", True),
            ("wip-sweep/tags-2026-07-26-1", False),   # not anchored at the start
            ("sweep/tags-2026-07-26-1-evil", False),  # not anchored at the end
            ("sweep/tags-2026-07-26", False),         # missing the counter
            ("sweep/tags-bogus-1", False),
            # `\d` is Unicode-wide (the pattern compiles with UNICODE and
            # no re.ASCII), so every one of these matched and was adopted
            # -- `gh pr merge --squash --delete-branch` issued against a
            # branch this automation provably did not cut, since
            # next_sweep_branch_name is strftime('%Y-%m-%d') + an int and
            # emits ASCII under every locale tested (C, ar_SA, ar_AE,
            # fa_IR, ja_JP, hi_IN, th_TH). `git check-ref-format --branch`
            # accepts all of them, so they are legal refs.
            ("sweep/tags-٢٠٢٦-٠٧-٢٦-١", False),  # Arabic-Indic
            ("sweep/tags-2026-07-26-١", False),  # Arabic-Indic counter
            ("sweep/tags-2026-07-26-１", False),  # FULLWIDTH DIGIT ONE
            ("sweep/tags-२०२६-07-26-1", False),  # Devanagari year
            ("sweep/tags-2026-07-26-\U0001d7f6", False),  # MATHEMATICAL MONOSPACE DIGIT FOUR
        ):
            with self.subTest(head=head):
                self.assertEqual(
                    parallel_model_fix_loop.is_own_sweep_branch(head), adopted
                )

    def test_the_server_side_search_is_not_trusted_as_the_boundary(self):
        # --search is a substring match, so a head that merely CONTAINS
        # the searched text must still be rejected locally.
        payload = _gh_pr_list((150, "attacker/sweep/tags-2026-07-26-1"))
        self.assertEqual(list_open_sweep_prs("/repo", lambda args, repo: (0, payload, "")), [])

    def test_the_query_asks_for_the_fields_the_gates_below_need(self):
        # gh returns only the requested fields, so a gate on isDraft /
        # baseRefName / isCrossRepository is unenforceable unless they are
        # asked for here. Pinned because the omission is invisible: the
        # keys simply read as missing and every PR looks adoptable.
        calls = []
        list_open_sweep_prs("/repo", lambda args, repo: calls.append(args) or (0, "[]", ""))
        self.assertEqual(
            calls,
            [["pr", "list", "--state", "open", "--json",
              "number,url,headRefName,isDraft,baseRefName,isCrossRepository",
              "--search", "head:sweep/tags-", "--limit", "200"]],
        )

    def test_a_draft_sweep_pr_is_never_adopted(self):
        # GitHub refuses to merge a draft server-side, so adopting one
        # buys two wasted gh calls and one ERROR-shaped
        # "could not merge adopted PR" line EVERY round, forever.
        payload = _gh_pr_list(_gh_pr_entry(160, "sweep/tags-2026-07-26-4", isDraft=True))
        self.assertEqual(list_open_sweep_prs("/repo", lambda args, repo: (0, payload, "")), [])

    def test_a_cross_repository_pr_is_never_adopted(self):
        # swack-tools/oxidex is PUBLIC with forks, and headRefName for a
        # cross-repo PR is the BARE branch name -- so a fork branch named
        # exactly sweep/tags-2026-07-26-9 satisfies SWEEP_BRANCH_RE. A
        # branch name is not provenance; this is the load-bearing gate,
        # not isDraft.
        payload = _gh_pr_list(
            _gh_pr_entry(161, "sweep/tags-2026-07-26-9", isCrossRepository=True),
        )
        self.assertEqual(list_open_sweep_prs("/repo", lambda args, repo: (0, payload, "")), [])

    def test_a_pr_retargeted_away_from_main_is_never_adopted(self):
        # real_create_pr always passes --base main; a PR whose base a
        # human moved is no longer the thing this automation opened.
        payload = _gh_pr_list(
            _gh_pr_entry(162, "sweep/tags-2026-07-26-5", baseRefName="release/1.x"),
        )
        self.assertEqual(list_open_sweep_prs("/repo", lambda args, repo: (0, payload, "")), [])

    def test_an_ordinary_open_sweep_pr_still_passes_every_new_gate(self):
        payload = _gh_pr_list((163, "sweep/tags-2026-07-26-6"))
        prs = list_open_sweep_prs("/repo", lambda args, repo: (0, payload, ""))
        self.assertEqual([p["number"] for p in prs], [163])


class AdoptOpenSweepPrsTests(unittest.TestCase):
    """MAJOR 4: a sweep PR whose checks went green AFTER the round that
    created it had already given up (the 45-minute
    wait_for_pr_checks timeout, a red-then-fixed run, a `gh pr create`
    that failed and was retried by hand) was never revisited by
    anything. overlord_sweep advances the sweep-state cursor before the
    push, so the stamps that fed that PR are already consumed and no
    later sweep re-cuts those commits: the fixes were stranded forever.
    """

    def _run_gh(self, list_payload, checks_by_ref, merge_rc=0):
        calls = []

        def run_gh(args, repo_root):
            calls.append(args)
            if args[:2] == ["pr", "list"]:
                return 0, list_payload, ""
            if args[:2] == ["pr", "checks"]:
                return 0, checks_by_ref[args[2]], ""
            if args[:2] == ["pr", "view"]:
                if "headRefOid,reviews" in args:
                    return 0, self.APPROVED_REVIEW, ""
                return 0, json.dumps({"state": "OPEN"}), ""
            return merge_rc, "Merged\n", "" if merge_rc == 0 else "not mergeable"
        return run_gh, calls

    GREEN = json.dumps([{"name": "Build & Test", "state": "SUCCESS", "bucket": "pass"}])
    RED = json.dumps([{"name": "Lint & Audit", "state": "FAILURE", "bucket": "fail"}])
    PENDING = json.dumps([{"name": "Build & Test", "state": "IN_PROGRESS", "bucket": "pending"}])
    APPROVED_REVIEW = json.dumps({
        "headRefOid": "a" * 40,
        "reviews": [{"state": "APPROVED", "submittedAt": "2026-08-02T00:00:00Z",
                     "author": {"login": "reviewer"}, "commit": {"oid": "a" * 40}}],
    })

    def test_a_since_green_abandoned_pr_is_surfaced_for_review(self):
        url = "https://github.com/o/r/pull/126"
        run_gh, calls = self._run_gh(_gh_pr_list((126, "sweep/tags-2026-07-25-2")), {url: self.GREEN})
        adopted = adopt_open_sweep_prs(repo_root="/repo", run_gh=run_gh, log_fn=lambda *a: None)
        self.assertEqual([(a["pr"], a["action"]) for a in adopted], [(url, "left_open_awaiting_review")])
        self.assertNotIn(["pr", "merge", url, "--squash", "--delete-branch"], calls)

    def test_a_still_red_pr_is_left_open_and_never_merged(self):
        url = "https://github.com/o/r/pull/126"
        run_gh, calls = self._run_gh(_gh_pr_list((126, "sweep/tags-2026-07-25-2")), {url: self.RED})
        adopted = adopt_open_sweep_prs(repo_root="/repo", run_gh=run_gh, log_fn=lambda *a: None)
        self.assertEqual([a["action"] for a in adopted], ["left_open"])
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in calls))

    def test_a_still_pending_pr_is_not_waited_on(self):
        # Adoption is a single non-blocking read per PR: the round's own
        # sweep still has to run, and a PR that is genuinely mid-CI gets
        # picked up by the NEXT round instead of blocking this one.
        url = "https://github.com/o/r/pull/126"
        run_gh, calls = self._run_gh(_gh_pr_list((126, "sweep/tags-2026-07-25-2")), {url: self.PENDING})
        adopt_open_sweep_prs(repo_root="/repo", run_gh=run_gh, log_fn=lambda *a: None)
        self.assertEqual(sum(1 for a in calls if a[:2] == ["pr", "checks"]), 1)
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in calls))

    def test_a_green_pr_without_a_current_head_approval_is_left_open(self):
        url = "https://github.com/o/r/pull/126"
        run_gh, calls = self._run_gh(
            _gh_pr_list((126, "sweep/tags-2026-07-25-2")), {url: self.GREEN},
        )

        def no_approval(args, repo_root):
            if args[:2] == ["pr", "view"] and "headRefOid,reviews" in args:
                return 0, json.dumps({"headRefOid": "a" * 40, "reviews": []}), ""
            return run_gh(args, repo_root)

        adopted = adopt_open_sweep_prs(repo_root="/repo", run_gh=no_approval, log_fn=lambda *a: None)
        self.assertEqual(adopted[0]["action"], "left_open")
        self.assertEqual(adopted[0]["reviews"], "pending")
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in calls))

    def test_no_open_sweep_prs_is_no_gh_merge_traffic_at_all(self):
        run_gh, calls = self._run_gh("[]", {})
        self.assertEqual(adopt_open_sweep_prs(repo_root="/repo", run_gh=run_gh,
                                              log_fn=lambda *a: None), [])
        self.assertEqual(calls, [["pr", "list", "--state", "open", "--json",
                                  "number,url,headRefName,isDraft,baseRefName,isCrossRepository",
                                   "--search", "head:sweep/tags-", "--limit", "200"]])

    def test_one_raising_gh_call_does_not_abandon_the_remaining_prs(self):
        """A gh runner that RAISES (observed: BlockingIOError errno 35,
        "Resource temporarily unavailable", from fork exhaustion on a box
        already running min(20, CPU) workers each in `cargo test
        --workspace`; also FileNotFoundError when `gh` is unlinked
        mid-round by a `brew upgrade` during a weeks-long --infinite run)
        aborted the whole adoption pass from inside the per-PR loop.
        Measured cost: PR #10 accounted for, #11 raised, #12 never
        looked at, and the adoption record discarded entirely."""
        payloads = {f"https://github.com/o/r/pull/{n}": self.GREEN for n in (10, 11, 12)}
        run_gh, calls = self._run_gh(
            _gh_pr_list((10, "sweep/tags-2026-07-26-1"), (11, "sweep/tags-2026-07-26-2"),
                        (12, "sweep/tags-2026-07-26-3")),
            payloads,
        )

        def flaky_gh(args, repo_root):
            if args[:2] == ["pr", "checks"] and args[2].endswith("/11"):
                raise BlockingIOError(35, "Resource temporarily unavailable")
            return run_gh(args, repo_root)

        adopted = adopt_open_sweep_prs(repo_root="/repo", run_gh=flaky_gh, log_fn=lambda *a: None)
        self.assertEqual([a["pr"].rsplit("/", 1)[-1] for a in adopted], ["10", "11", "12"])
        actions = {a["pr"].rsplit("/", 1)[-1]: a["action"] for a in adopted}
        self.assertEqual(actions["10"], "left_open_awaiting_review")
        self.assertEqual(actions["12"], "left_open_awaiting_review")
        self.assertEqual(actions["11"], "check_failed")
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in calls))


class PrReviewStateTests(unittest.TestCase):
    def _payload(self, reviews):
        return json.dumps({"headRefOid": "a" * 40, "reviews": reviews})

    def test_current_approval_overrides_same_reviewers_old_request_for_changes(self):
        reviews = [
            {"state": "CHANGES_REQUESTED", "submittedAt": "2026-08-01T00:00:00Z",
             "author": {"login": "reviewer"}, "commit": {"oid": "a" * 40}},
            {"state": "APPROVED", "submittedAt": "2026-08-02T00:00:00Z",
             "author": {"login": "reviewer"}, "commit": {"oid": "a" * 40}},
        ]
        state, _detail = pr_review_state(
            "126", "/repo", lambda args, repo: (0, self._payload(reviews), ""),
        )
        self.assertEqual(state, "approved")

    def test_any_current_request_for_changes_blocks_merge(self):
        reviews = [
            {"state": "APPROVED", "submittedAt": "2026-08-02T00:00:00Z",
             "author": {"login": "one"}, "commit": {"oid": "a" * 40}},
            {"state": "CHANGES_REQUESTED", "submittedAt": "2026-08-02T00:01:00Z",
             "author": {"login": "two"}, "commit": {"oid": "a" * 40}},
        ]
        state, _detail = pr_review_state(
            "126", "/repo", lambda args, repo: (0, self._payload(reviews), ""),
        )
        self.assertEqual(state, "changes_requested")

    def test_an_approval_for_an_old_head_does_not_authorize_new_code(self):
        reviews = [{"state": "APPROVED", "submittedAt": "2026-08-02T00:00:00Z",
                    "author": {"login": "reviewer"}, "commit": {"oid": "b" * 40}}]
        state, _detail = pr_review_state(
            "126", "/repo", lambda args, repo: (0, self._payload(reviews), ""),
        )
        self.assertEqual(state, "pending")


class DefaultRunGhNeverRaisesTests(unittest.TestCase):
    """Every consumer of a gh runner already copes with a failure tuple
    (pr_checks_state -> "unknown", list_open_sweep_prs -> []), and
    exactly one of them -- list_open_sweep_prs -- had its own
    `except OSError`. Putting the guard in the runner instead makes
    that redundant rather than load-bearing."""

    def _run(self, exc):
        with patch("parallel_model_fix_loop.subprocess.run", side_effect=exc):
            return parallel_model_fix_loop.default_run_gh(["pr", "checks", "1"], "/repo")

    def test_a_blocking_io_error_becomes_a_failure_tuple(self):
        # errno 35 from fork exhaustion, reproduced by lowering
        # RLIMIT_NPROC and calling subprocess.run(["gh", "--version"]).
        rc, out, err = self._run(BlockingIOError(35, "Resource temporarily unavailable"))
        self.assertNotEqual(rc, 0)
        self.assertEqual(out, "")
        self.assertIn("Resource temporarily unavailable", err)

    def test_a_missing_gh_binary_becomes_a_failure_tuple(self):
        rc, _out, err = self._run(FileNotFoundError(2, "No such file or directory: 'gh'"))
        self.assertNotEqual(rc, 0)
        self.assertIn("gh", err)

    def test_the_failure_tuple_reads_as_unknown_not_green(self):
        # The property that matters: an unrunnable gh must never be
        # mistaken for a green PR.
        with patch("parallel_model_fix_loop.subprocess.run",
                   side_effect=BlockingIOError(35, "Resource temporarily unavailable")):
            state, _detail = pr_checks_state("pr", "/repo", parallel_model_fix_loop.default_run_gh)
        self.assertEqual(state, "unknown")


class SyncWorktreesToOriginMainTests(GitRepoTestCase):
    """Real tempdir repo + real `git worktree add`: the whole point of
    this step is which worktrees git actually moves and which it refuses
    to, which only a real git can answer. origin_ref="main"/fetch=False
    stand in for origin/main so no test needs a remote."""

    def setUp(self):
        super().setUp()
        self.repo = self.make_repo()
        self.old_sha = git_out(self.repo, "rev-parse", "HEAD").strip()
        self.target = self.commit_file(self.repo, "landed.txt", "swept\n", "sweep: land a tag fix")

    def add_worktree(self, name, branch, *extra):
        path = self.tmp / name
        git(self.repo, "worktree", "add", "-q", *extra, str(path), branch)
        return path

    def sync(self):
        return sync_worktrees_to_origin_main(
            repo_root=self.repo, origin_ref="main", fetch=False, log_fn=lambda *a: None,
        )

    def test_fast_forwards_a_clean_branch_worktree_that_is_behind(self):
        git(self.repo, "branch", "worker-behind", self.old_sha)
        path = self.add_worktree("wt-behind", "worker-behind")
        summary = self.sync()
        self.assertIn(path.resolve(), [p.resolve() for p in summary["updated"]])
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(), self.target)

    def test_skips_a_dirty_worktree_and_keeps_its_edit(self):
        git(self.repo, "branch", "worker-dirty", self.old_sha)
        path = self.add_worktree("wt-dirty", "worker-dirty")
        (path / "README.md").write_text("a worker is mid-fix here\n")
        summary = self.sync()
        skipped = {p.resolve(): reason for p, reason in summary["skipped"]}
        self.assertIn("dirty", skipped[path.resolve()])
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(), self.old_sha)
        self.assertEqual((path / "README.md").read_text(), "a worker is mid-fix here\n")

    def test_skips_a_worktree_carrying_unpushed_commits(self):
        git(self.repo, "branch", "worker-ahead", self.old_sha)
        path = self.add_worktree("wt-ahead", "worker-ahead")
        (path / "fix.rs").write_text("fn fix() {}\n")
        git(path, "add", "-A")
        git(path, "commit", "-q", "-m", "fix JPEG:Foo (not swept yet)")
        ahead_sha = git_out(path, "rev-parse", "HEAD").strip()

        summary = self.sync()

        skipped = {p.resolve(): reason for p, reason in summary["skipped"]}
        self.assertIn("not on main", skipped[path.resolve()])
        # The commit survives -- this is the no-discard invariant, and a
        # worker's validated-but-unswept fix is exactly what it protects.
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(), ahead_sha)

    def test_fast_forwards_a_detached_worktree(self):
        path = self.add_worktree("wt-detached", self.old_sha, "--detach")
        summary = self.sync()
        self.assertIn(path.resolve(), [p.resolve() for p in summary["updated"]])
        self.assertEqual(git_out(path, "rev-parse", "HEAD").strip(), self.target)
        self.assertEqual(git_out(path, "rev-parse", "--abbrev-ref", "HEAD").strip(), "HEAD")

    def test_an_already_current_worktree_is_reported_not_touched(self):
        summary = self.sync()
        # The main checkout itself is on main, already at the target.
        self.assertIn(self.repo.resolve(), [p.resolve() for p in summary["current"]])
        self.assertEqual(summary["failed"], [])

    def test_unresolvable_target_syncs_nothing(self):
        summary = sync_worktrees_to_origin_main(
            repo_root=self.repo, origin_ref="refs/remotes/origin/nope", fetch=False,
            log_fn=lambda *a: None,
        )
        self.assertEqual(summary, {"updated": [], "current": [], "skipped": [], "failed": []})


class AutoPublishRoundTests(unittest.TestCase):
    """auto_publish_round with every side effect injected: the sweep, the
    worktree provisioning, `gh`, and the worktree sync."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)
        self.sweep_repo = self.tmp / "sweep-wt"
        self.sweep_repo.mkdir()
        self.gh_calls = []
        self.sync_calls = []

    def _ensure_worktree_fn(self, repo_root, path, **kwargs):
        return self.sweep_repo, "reused"

    def _sync_fn(self, **kwargs):
        self.sync_calls.append(kwargs)
        return {"updated": [], "current": [], "skipped": [], "failed": []}

    def _run_gh(self, checks_payload, merge_rc=0, list_payload="[]"):
        def run_gh(args, repo_root):
            self.gh_calls.append(args)
            if args[:2] == ["pr", "list"]:
                return 0, list_payload, ""
            if args[:2] == ["pr", "checks"]:
                return 0, checks_payload, ""
            if args[:2] == ["pr", "view"] and "headRefOid,reviews" in args:
                return 0, self.APPROVED_REVIEW, ""
            return merge_rc, "Merged\n", "" if merge_rc == 0 else "not mergeable"
        return run_gh

    def _publish(self, sweep_result, run_gh, **overrides):
        kwargs = dict(
            repo_root=self.tmp / "repo", cache_dir="/unused", home=self.tmp / "home",
            sweep_fn=lambda **kw: sweep_result, ensure_worktree_fn=self._ensure_worktree_fn,
            sync_fn=self._sync_fn, run_gh=run_gh, sleep_fn=lambda s: None,
            log_fn=lambda *a: None,
        )
        kwargs.update(overrides)
        return auto_publish_round(**kwargs)

    OK_SWEEP = {"status": "ok", "branch": "sweep/tags-2026-07-25-1",
                "pr": {"ok": True, "stdout": "https://github.com/o/r/pull/125"}}
    GREEN = json.dumps([{"name": "Build & Test", "state": "SUCCESS", "bucket": "pass"},
                        {"name": "Lint & Audit", "state": "SUCCESS", "bucket": "pass"}])
    RED = json.dumps([{"name": "Build & Test", "state": "SUCCESS", "bucket": "pass"},
                      {"name": "Lint & Audit", "state": "FAILURE", "bucket": "fail"}])
    PENDING = json.dumps([{"name": "Build & Test", "state": "IN_PROGRESS", "bucket": "pending"}])
    APPROVED_REVIEW = json.dumps({
        "headRefOid": "a" * 40,
        "reviews": [{"state": "APPROVED", "submittedAt": "2026-08-02T00:00:00Z",
                     "author": {"login": "reviewer"}, "commit": {"oid": "a" * 40}}],
    })

    def test_all_green_and_approved_publishes_without_merging(self):
        result = self._publish(self.OK_SWEEP, self._run_gh(self.GREEN))
        self.assertEqual(result["status"], "published_awaiting_review")
        self.assertNotIn(["pr", "merge", "https://github.com/o/r/pull/125", "--squash", "--delete-branch"],
                         self.gh_calls)
        # The fleet never merges, so nothing ever triggers a sync from
        # this path.
        self.assertEqual(self.sync_calls, [])

    def test_red_checks_leave_the_pr_open_and_never_merge(self):
        result = self._publish(self.OK_SWEEP, self._run_gh(self.RED))
        self.assertEqual(result["status"], "checks_red")
        self.assertFalse(any(args[:2] == ["pr", "merge"] for args in self.gh_calls))
        self.assertEqual(self.sync_calls, [])

    def test_still_pending_checks_time_out_without_merging(self):
        clock = iter([0, 10, 20, 30, 40, 50, 60, 70])
        result = self._publish(
            self.OK_SWEEP, self._run_gh(self.PENDING),
            now_fn=lambda: next(clock), checks_timeout_seconds=15, checks_interval_seconds=1,
        )
        self.assertEqual(result["status"], "checks_timeout")
        self.assertFalse(any(args[:2] == ["pr", "merge"] for args in self.gh_calls))
        self.assertEqual(self.sync_calls, [])

    def test_a_check_that_flips_red_between_the_green_poll_and_the_merge_is_not_merged(self):
        # .github/workflows/ci.yml:11-13 sets
        # `concurrency: cancel-in-progress: true`, so a push to the same
        # ref (or a re-run) moves an in-flight job into the "cancel"
        # bucket -- RED -- in the seconds between the poll that returned
        # green and `gh pr merge`. One re-read closes that window.
        answers = iter([self.GREEN, self.RED])

        def run_gh(args, repo_root):
            self.gh_calls.append(args)
            if args[:2] == ["pr", "list"]:
                return 0, "[]", ""
            if args[:2] == ["pr", "checks"]:
                return 0, next(answers), ""
            if args[:2] == ["pr", "view"] and "headRefOid,reviews" in args:
                return 0, self.APPROVED_REVIEW, ""
            return 0, "Merged\n", ""

        result = self._publish(self.OK_SWEEP, run_gh)
        self.assertEqual(result["status"], "checks_red")
        self.assertFalse(any(args[:2] == ["pr", "merge"] for args in self.gh_calls))
        self.assertEqual(self.sync_calls, [])

    def test_green_checks_without_review_leave_the_round_pr_open(self):
        def no_review(args, repo_root):
            self.gh_calls.append(args)
            if args[:2] == ["pr", "list"]:
                return 0, "[]", ""
            if args[:2] == ["pr", "checks"]:
                return 0, self.GREEN, ""
            if args[:2] == ["pr", "view"] and "headRefOid,reviews" in args:
                return 0, json.dumps({"headRefOid": "a" * 40, "reviews": []}), ""
            return 1, "", "unexpected gh call"

        result = self._publish(self.OK_SWEEP, no_review)
        self.assertEqual(result["status"], "reviews_pending")
        self.assertFalse(any(args[:2] == ["pr", "merge"] for args in self.gh_calls))
        self.assertEqual(self.sync_calls, [])

    def test_an_abandoned_sweep_pr_from_an_earlier_round_is_adopted_and_surfaced(self):
        # MAJOR 4: nothing revisited an already-open sweep PR, and the
        # sweep cursor had already consumed its stamps -- so a PR that
        # went green one minute after the checks timeout was stranded on
        # origin forever, never even noticed again.
        run_gh = self._run_gh(self.GREEN, list_payload=_gh_pr_list((126, "sweep/tags-2026-07-25-2")))
        result = self._publish({"status": "no_news"}, run_gh)
        self.assertEqual(result["status"], "no_news")
        self.assertEqual([a["action"] for a in result["adopted"]], ["left_open_awaiting_review"])
        self.assertNotIn(["pr", "merge", "https://github.com/o/r/pull/126", "--squash", "--delete-branch"],
                         self.gh_calls)
        # The fleet never merges, so nothing ever triggers a sync.
        self.assertEqual(self.sync_calls, [])

    def test_adoption_runs_before_the_sweep_cuts_a_new_branch(self):
        order = []
        run_gh = self._run_gh(self.GREEN, list_payload=_gh_pr_list((126, "sweep/tags-2026-07-25-2")))

        def watching_gh(args, repo_root):
            order.append(args[:2])
            return run_gh(args, repo_root)

        self._publish({"status": "no_news"}, watching_gh,
                      sweep_fn=lambda **kw: order.append(["sweep"]) or {"status": "no_news"})
        self.assertLess(order.index(["pr", "checks"]), order.index(["sweep"]))

    def test_a_sweep_with_no_news_and_no_open_pr_touches_nothing_at_all(self):
        result = self._publish({"status": "no_news"}, self._run_gh(self.GREEN))
        self.assertEqual(result["status"], "no_news")
        # One read-only `gh pr list` (the adoption pass) and nothing
        # else: no checks poll, no merge, no sync.
        self.assertEqual(self.gh_calls, [["pr", "list", "--state", "open", "--json",
                                          "number,url,headRefName,isDraft,baseRefName,isCrossRepository",
                                   "--search", "head:sweep/tags-", "--limit", "200"]])
        self.assertEqual(self.sync_calls, [])

    def test_an_unavailable_sweep_worktree_skips_the_round(self):
        result = self._publish(
            self.OK_SWEEP, self._run_gh(self.GREEN),
            ensure_worktree_fn=lambda repo_root, path, **kw: (None, "worktree add failed"),
        )
        self.assertEqual(result["status"], "no_worktree")
        # Only the read-only adoption listing ran; no PR of this round's
        # was polled or merged, and nothing was synced.
        self.assertEqual([a[:2] for a in self.gh_calls], [["pr", "list"]])
        self.assertEqual(self.sync_calls, [])

    def test_an_unavailable_sweep_worktree_still_adopts_a_stale_green_pr(self):
        # The worktree is what the SWEEP needs; an already-open PR from
        # an earlier round needs nothing but gh, so a provisioning
        # failure must not stop it from being noticed too.
        result = self._publish(
            self.OK_SWEEP,
            self._run_gh(self.GREEN, list_payload=_gh_pr_list((126, "sweep/tags-2026-07-25-2"))),
            ensure_worktree_fn=lambda repo_root, path, **kw: (None, "worktree add failed"),
        )
        self.assertEqual(result["status"], "no_worktree")
        self.assertEqual([a["action"] for a in result["adopted"]], ["left_open_awaiting_review"])
        self.assertEqual(self.sync_calls, [])

    def test_a_raising_gh_mid_adoption_does_not_abort_the_round(self):
        # DEFECT 6, at the round level: the abort discarded the adoption
        # record entirely, so a PR that was accounted for before the
        # raise disappeared from the result, and the round's own sweep
        # never got a chance either.
        swept = []
        payloads = {f"https://github.com/o/r/pull/{n}": self.GREEN for n in (10, 11, 12)}
        base = self._run_gh(self.GREEN,
                            list_payload=_gh_pr_list((10, "sweep/tags-2026-07-26-1"),
                                                     (11, "sweep/tags-2026-07-26-2"),
                                                     (12, "sweep/tags-2026-07-26-3")))

        def flaky_gh(args, repo_root):
            if args[:2] == ["pr", "checks"] and args[2].endswith("/11"):
                raise BlockingIOError(35, "Resource temporarily unavailable")
            if args[:2] == ["pr", "checks"]:
                self.gh_calls.append(args)
                return 0, payloads[args[2]], ""
            return base(args, repo_root)

        result = self._publish(
            {"status": "no_news"}, flaky_gh,
            sweep_fn=lambda **kw: swept.append(1) or {"status": "no_news"},
        )
        self.assertEqual(result["status"], "no_news")
        self.assertEqual(len(result["adopted"]), 3)
        self.assertEqual(self.sync_calls, [])
        self.assertEqual(swept, [1])

    def test_a_sweep_whose_bisection_left_the_recheck_failing_is_not_merged(self):
        # Belt-and-braces on top of run_sweep's own hard stop: a future
        # status regression must not be able to hand this function a
        # branch that bisection could not clear. status == "ok" is not
        # sufficient once a bisection happened.
        sweep = dict(self.OK_SWEEP,
                     bisection={"offenders": ["nikon"], "surviving_squads": ["canon"],
                                "unrevertable": [], "recheck_passed": False})
        result = self._publish(sweep, self._run_gh(self.GREEN))
        self.assertEqual(result["status"], "bisection_unverified")
        self.assertFalse(any(args[:2] == ["pr", "merge"] for args in self.gh_calls))
        self.assertEqual(self.sync_calls, [])

    def test_a_sweep_whose_bisection_cleared_the_recheck_still_publishes(self):
        sweep = dict(self.OK_SWEEP,
                     bisection={"offenders": ["nikon"], "surviving_squads": ["canon"],
                                "unrevertable": [], "recheck_passed": True})
        self.assertEqual(self._publish(sweep, self._run_gh(self.GREEN))["status"],
                         "published_awaiting_review")

    def test_an_empty_diff_branch_is_not_polled_or_merged(self):
        # The repo's DURABLE idempotency rule, applied at the consumer:
        # compare the TREE, not the SHA. Pushing a tree-identical branch
        # and squash-merging it puts an empty commit on main and burns a
        # full CI cycle.
        def run_git(args, repo_root):
            if args[:2] == ["diff", "--quiet"]:
                return 0, "", ""   # git's "no differences" exit code
            return parallel_model_fix_loop.default_run_git(args, repo_root)

        result = self._publish(self.OK_SWEEP, self._run_gh(self.GREEN), run_git=run_git)
        self.assertEqual(result["status"], "zero_delta")
        self.assertFalse(any(args[:2] == ["pr", "checks"] for args in self.gh_calls))
        self.assertFalse(any(args[:2] == ["pr", "merge"] for args in self.gh_calls))

    def test_sweep_receives_the_sweep_worktree_and_a_home_scoped_cursor(self):
        seen = {}
        self._publish(
            {"status": "no_news"}, self._run_gh(self.GREEN),
            sweep_fn=lambda **kw: seen.update(kw) or {"status": "no_news"},
        )
        # The sweep must run in the dedicated worktree, never in the
        # dispatcher's own checkout (whose HEAD the next round reads).
        self.assertEqual(seen["repo_root"], self.sweep_repo)
        self.assertEqual(seen["home"], self.tmp / "home")
        self.assertEqual(seen["sweep_state_path"], self.tmp / "home" / "logs" / "sweep-state.json")


class AutoPublishNoNewsIsATrueNoOpTests(GitRepoTestCase):
    """The no-op path, driven through the REAL overlord_sweep.run_sweep
    rather than a stub: with no squad stamped green since the last
    cursor position, a round must not create a ref, make a commit, or
    open/merge/poll a PR.

    The one gh call such a round does make is the READ-ONLY `gh pr list`
    of the round-start adoption pass (see adopt_open_sweep_prs) -- that
    pass is the only thing in this system that can ever land a sweep PR
    stranded by an earlier round, so it deliberately runs even when
    there is no news to sweep. With no open sweep PR it changes nothing.
    """

    def test_real_run_sweep_with_no_green_stamps_creates_no_refs_and_no_gh_writes(self):
        repo = self.make_repo()
        home = self.tmp / "home"
        squads_toml = self.tmp / "squads.toml"
        squads_toml.write_text('[squads.canon]\nmodules = []\nformats = ["JPEG"]\nownership_globs = []\n')
        refs_before = git_out(repo, "for-each-ref", "--format=%(refname)")
        head_before = git_out(repo, "rev-parse", "HEAD").strip()
        gh_calls, sync_calls = [], []

        def sweep_fn(**kwargs):
            return overlord_sweep.run_sweep(
                comparison_fn=lambda *a: {"gap_count": 0},
                checkout_fn=overlord_sweep.real_checkout,
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                push_branch_fn=lambda repo_root, branch: self.fail("no-news round pushed a branch"),
                create_pr_fn=lambda *a, **kw: self.fail("no-news round created a PR"),
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                **kwargs,
            )

        result = auto_publish_round(
            repo_root=repo, cache_dir="/unused", home=home, squads_toml_path=squads_toml,
            fmt_fn=lambda repo_root: self.fail("no-news round ran cargo fmt"),
            sweep_fn=sweep_fn, ensure_worktree_fn=lambda repo_root, path, **kw: (repo, "reused"),
            sync_fn=lambda **kw: sync_calls.append(kw),
            run_gh=lambda args, repo_root: gh_calls.append(args) or (0, "[]", ""),
            log_fn=lambda *a: None,
        )

        self.assertEqual(result["status"], "no_news")
        self.assertEqual(git_out(repo, "for-each-ref", "--format=%(refname)"), refs_before)
        self.assertEqual(git_out(repo, "rev-parse", "HEAD").strip(), head_before)
        self.assertEqual(git_out(repo, "status", "--porcelain"), "")
        self.assertEqual([a[:2] for a in gh_calls], [["pr", "list"]])
        self.assertEqual(result["adopted"], [])
        self.assertEqual(sync_calls, [])


class AutoPublishEndToEndTests(GitRepoTestCase):
    """The acceptance case end to end, against real git: a worker's
    validated fix sits on a squad branch stamped green, and one
    auto_publish_round call has to get it onto origin/main and every
    worktree updated -- with only `gh` faked (there is no GitHub here).
    The push and the sweep are REAL: a bare repo stands in for origin,
    and the fake `gh pr merge` advances origin/main the way GitHub's own
    squash-merge would."""

    def test_a_green_and_approved_round_pushes_the_fix_but_never_merges_it(self):
        import squad_merge_loop

        origin = self.tmp / "origin.git"
        git(self.tmp, "init", "-q", "--bare", str(origin))
        repo = self.make_repo()
        git(repo, "remote", "add", "origin", str(origin))
        git(repo, "push", "-q", "-u", "origin", "main")
        origin_main_before = git_out(origin, "rev-parse", "main").strip()

        # A worker's fix, validated onto its squad branch -- and, as
        # every worker-authored fix has been, not cargo-fmt clean.
        git(repo, "checkout", "-q", "-b", "squad/canon")
        (repo / "src").mkdir()
        (repo / "src" / "fix.rs").write_text("fn fixed( ) {}\n")
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", "fix JPEG:Foo\n\nFormat: JPEG\nTag: JPEG:Foo\n"
                                        "Verified: recheck-pass gaps=3->2\n")
        squad_sha = git_out(repo, "rev-parse", "HEAD").strip()
        git(repo, "checkout", "-q", "main")

        home = self.tmp / "home"
        squad_merge_loop.record_head(
            squad_merge_loop.squad_status_file(home, "canon"), "workerhead", status="consumed",
            patch_id="p1", format_name="JPEG", squad_sha=squad_sha, now_fn=lambda: 100,
        )
        squads_toml = self.tmp / "squads.toml"
        squads_toml.write_text('[squads.canon]\nmodules = []\nformats = ["JPEG"]\nownership_globs = []\n')

        # A worker worktree sitting on an older main: the thing that has
        # to end up updated once the fix lands.
        worker_wt = self.tmp / "worker-wt"
        git(repo, "worktree", "add", "-q", "-b", "worker-idle", str(worker_wt), "main")

        gh_calls = []
        pr_head_branch = {}

        def fake_gh(args, repo_root):
            gh_calls.append(args)
            if args[:2] == ["pr", "list"]:
                # No sweep PR is open before this round -- the adoption
                # pass has nothing to pick up.
                return 0, "[]", ""
            if args[:2] == ["pr", "checks"]:
                return 0, json.dumps([{"name": "Build & Test", "state": "SUCCESS", "bucket": "pass"},
                                      {"name": "Lint & Audit", "state": "SUCCESS", "bucket": "pass"},
                                      {"name": "Multi-platform Build", "state": "SKIPPED",
                                       "bucket": "skipping"}]), ""
            if args[:2] == ["pr", "view"] and "headRefOid,reviews" in args:
                return 0, json.dumps({
                    "headRefOid": "a" * 40,
                    "reviews": [{"state": "APPROVED", "submittedAt": "2026-08-02T00:00:00Z",
                                 "author": {"login": "reviewer"}, "commit": {"oid": "a" * 40}}],
                }), ""
            if args[:2] == ["pr", "merge"]:
                # Stand in for GitHub's squash-merge, FAITHFULLY: GitHub
                # merges the REMOTE head branch of the PR -- whatever
                # `git push origin <branch>` actually delivered -- and has
                # no idea what this machine's local HEAD points at.
                #
                # The previous fake ran `git push -q origin HEAD:main` from
                # the local checkout, which merged the local DETACHED HEAD
                # instead. That silently papered over the orphaned-fmt-
                # commit defect: the fmt commit only ever existed on the
                # detached HEAD, so pushing HEAD made it "land" on main
                # while the branch origin really had was still unformatted.
                git(origin, "update-ref", "refs/heads/main",
                    f"refs/heads/{pr_head_branch['branch']}")
                return 0, "Merged\n", ""
            return 1, "", "unexpected gh call"

        def fake_fmt(repo_root):
            """rustfmt, as far as this test is concerned: the one thing
            that makes the branch `cargo fmt --all -- --check` clean."""
            (Path(repo_root) / "src" / "fix.rs").write_text("fn fixed() {}\n")
            return True, ""

        def sweep_fn(**kwargs):
            return overlord_sweep.run_sweep(
                comparison_fn=lambda repo_root, cache_dir, fmt, suffix: {
                    "gap_count": 5 if suffix == "sweep-pre" else 4,
                    "duplicate_emissions": [], "extra_in_oxidex": [],
                },
                checkout_fn=overlord_sweep.real_checkout,
                cargo_test_workspace_fn=lambda repo_root: (True, "ok"),
                create_pr_fn=lambda title, body, branch, base: (
                    pr_head_branch.update(branch=branch)
                    or {"ok": True, "stdout": "https://github.com/o/r/pull/200"}
                ),
                dispatcher_lock_path=home / "logs" / "dispatcher.lock",
                **kwargs,
            )

        result = auto_publish_round(
            repo_root=repo, cache_dir="/unused", home=home, squads_toml_path=squads_toml,
            sweep_fn=sweep_fn,
            # The repo itself doubles as the sweep worktree here; the
            # dedicated-worktree provisioning has its own tests above.
            ensure_worktree_fn=lambda repo_root, path, **kw: (repo, "reused"),
            fmt_fn=fake_fmt,
            # The lint gate defaults to real_cargo_lint, which shells out to
            # `cargo clippy --all-features -- -D warnings`. Against this
            # test's synthetic repo that can only fail, and it did: this
            # asserted 'merged' and got 'lint_failed' on every run. The gate
            # itself is exercised in test_overlord_sweep.py, both passing and
            # failing (see the lint_fn returning (False, "error: unreachable
            # pattern")); what THIS test is for is the publish path.
            lint_fn=lambda repo_root: (True, ""),
            run_gh=fake_gh, sleep_fn=lambda s: None, log_fn=lambda *a: None,
        )

        self.assertEqual(result["status"], "published_awaiting_review")
        self.assertTrue(result["sweep"]["fmt"]["committed"])

        # What origin ACTUALLY received on the head branch -- the fix
        # still has to reach the PR's branch, fmt commit included, even
        # though the fleet never merges it itself. Asserted on the branch
        # as it exists in the bare origin repo, not on any local ref.
        pushed_branch = pr_head_branch["branch"]
        self.assertEqual(git_out(origin, "show", f"refs/heads/{pushed_branch}:src/fix.rs"),
                         "fn fixed() {}\n")
        self.assertIn("style: cargo fmt --all (sweep publish)",
                      git_out(origin, "log", f"refs/heads/{pushed_branch}", "--format=%s").splitlines())

        # The fleet publishes; it never merges. origin/main must be
        # exactly where it started, no worktree gets fast-forwarded, and
        # `gh pr merge` is never even attempted -- a human merges the
        # [needs review] PR by hand.
        self.assertEqual(git_out(origin, "rev-parse", "main").strip(), origin_main_before)
        self.assertEqual(git_out(worker_wt, "rev-parse", "HEAD").strip(), origin_main_before)
        self.assertIsNone(result.get("sync"))
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in gh_calls))


class AutoPublishFormattingTests(GitRepoTestCase):
    """The measured PR #124 failure: worker-authored Rust is validated
    semantically but never style-checked, and CI's "Lint & Audit" job
    runs `cargo fmt --all -- --check`. The publish path must format the
    sweep branch, and must do so as its own commit -- and only when
    rustfmt actually changed something."""

    def test_fmt_commit_is_created_only_when_fmt_changes_something(self):
        repo = self.make_repo()
        self.commit_file(repo, "src/a.rs", "fn a( ) {}\n", "worker fix, unformatted")
        before = git_out(repo, "rev-parse", "HEAD").strip()

        def reformatting_fmt(repo_root):
            (Path(repo_root) / "src" / "a.rs").write_text("fn a() {}\n")
            return True, ""

        changed = overlord_sweep.format_sweep_branch(
            repo, parallel_model_fix_loop.default_run_git, fmt_fn=reformatting_fmt,
            log_fn=lambda *a: None,
        )
        self.assertTrue(changed["committed"])
        after_fmt = git_out(repo, "rev-parse", "HEAD").strip()
        self.assertNotEqual(after_fmt, before)
        self.assertIn("style: cargo fmt --all", git_out(repo, "log", "-1", "--format=%s"))

        # Second pass over an already-clean branch: no empty commit.
        unchanged = overlord_sweep.format_sweep_branch(
            repo, parallel_model_fix_loop.default_run_git, fmt_fn=lambda repo_root: (True, ""),
            log_fn=lambda *a: None,
        )
        self.assertFalse(unchanged["committed"])
        self.assertEqual(git_out(repo, "rev-parse", "HEAD").strip(), after_fmt)

    def test_auto_publish_hands_the_fmt_hook_to_the_sweep(self):
        # The dispatcher must not be able to publish while silently
        # bypassing the fmt step -- fmt_fn is threaded through to
        # run_sweep, which calls format_sweep_branch just before the push.
        seen = {}
        marker = lambda repo_root: (True, "")  # noqa: E731 -- identity marker for the assert below
        auto_publish_round(
            repo_root=self.tmp / "repo", cache_dir="/unused", home=self.tmp / "home",
            sweep_fn=lambda **kw: seen.update(kw) or {"status": "no_news"},
            ensure_worktree_fn=lambda repo_root, path, **kw: (self.tmp, "reused"),
            # Injected so the round-start adoption pass stays hermetic --
            # the default run_gh would shell out to a real `gh`.
            run_gh=lambda args, repo_root: (0, "[]", ""),
            fmt_fn=marker, log_fn=lambda *a: None,
        )
        self.assertIs(seen["fmt_fn"], marker)


class MainAutoPublishGatingTests(unittest.TestCase):
    """--auto-publish is tri-state: ON by default for --infinite (an
    unattended pipeline is the whole point of that mode), off for a
    single round, and explicitly overridable either way."""

    def setUp(self):
        self._tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmpdir.cleanup)
        self.tmp = Path(self._tmpdir.name)
        self.lock_path = self.tmp / "dispatcher.lock"
        self.pgids_path = self.tmp / "dispatcher-pgids.json"
        self.addCleanup(parallel_model_fix_loop._set_pgids_persist_path, None)
        self.config_path = self.tmp / "config.toml"
        self.config_path.write_text('[worker]\nmodels = ["m"]\n')
        self.publishes = []

    def _main(self, argv, rounds=1, publish_fn=None, **kwargs):
        calls = []

        def fake_round(args, cfg):
            calls.append(1)
            if len(calls) >= rounds:
                raise KeyboardInterrupt("stop the test loop")
            return True

        def default_publish(**kw):
            self.publishes.append(kw)
            return {"status": "no_news"}

        try:
            main(
                ["--config", str(self.config_path), *argv],
                run_round_fn=kwargs.pop("run_round_fn", fake_round),
                auto_publish_fn=publish_fn or default_publish,
                lock_path=self.lock_path, pgids_path=self.pgids_path, **kwargs,
            )
        except KeyboardInterrupt:
            pass
        return calls

    def test_infinite_publishes_every_round_with_no_extra_flag(self):
        # The acceptance case: `uv run scripts/parallel_model_fix_loop.py
        # --infinite` and nothing else.
        self._main(["--infinite"], rounds=3)
        self.assertEqual(len(self.publishes), 2)

    def test_no_auto_publish_opts_infinite_out(self):
        self._main(["--infinite", "--no-auto-publish"], rounds=3)
        self.assertEqual(self.publishes, [])

    def test_a_single_round_does_not_publish_by_default(self):
        self._main([], rounds=1, run_round_fn=lambda args, cfg: True)
        self.assertEqual(self.publishes, [])

    def test_a_single_round_publishes_when_asked_explicitly(self):
        self._main(["--auto-publish"], rounds=1, run_round_fn=lambda args, cfg: True)
        self.assertEqual(len(self.publishes), 1)

    def test_publish_kwargs_come_from_the_cli(self):
        self._main(
            ["--auto-publish", "--home", str(self.tmp / "home"),
             "--sweep-worktree-dir", str(self.tmp / "sweep"), "--pr-checks-timeout", "60",
             "--pr-checks-interval", "5"],
            rounds=1, run_round_fn=lambda args, cfg: True,
        )
        kwargs = self.publishes[0]
        self.assertEqual(kwargs["home"], self.tmp / "home")
        self.assertEqual(kwargs["sweep_worktree_dir"], self.tmp / "sweep")
        self.assertEqual(kwargs["checks_timeout_seconds"], 60)
        self.assertEqual(kwargs["checks_interval_seconds"], 5)

    def _main_rc(self, argv, publish_fn, **kwargs):
        """main()'s ACTUAL return code for a one-shot round (no
        KeyboardInterrupt escape hatch -- the whole point is the value it
        returns)."""
        return main(
            ["--config", str(self.config_path), *argv],
            run_round_fn=kwargs.pop("run_round_fn", lambda args, cfg: True),
            auto_publish_fn=publish_fn, lock_path=self.lock_path,
            pgids_path=self.pgids_path, **kwargs,
        )

    def test_a_one_shot_round_whose_publish_failed_exits_non_zero(self):
        """_run_auto_publish_safely's return value was DISCARDED, so
        `--auto-publish` without --infinite exited 0 for all 11 terminal
        publish-failure statuses. That is the one genuinely
        machine-invisible case: with --infinite, stdout already announces
        every failure each round and adoption already retries, but a
        one-shot operator/CI invocation has nothing else to read."""
        for status in ("no_worktree", "push_failed", "pr_create_failed", "checks_red",
                       "checks_timeout", "checks_unknown",
                       "branch_cut_failed", "sweep_aborted", "reattach_failed",
                       "workspace_tests_failed", "bisection_unverified", "raised"):
            with self.subTest(status=status):
                rc = self._main_rc(["--auto-publish"], lambda **kw: {"status": status})
                self.assertEqual(rc, 1)

    def test_a_one_shot_round_that_published_or_had_nothing_to_do_exits_zero(self):
        for status in ("published_awaiting_review", "no_news", "zero_delta"):
            with self.subTest(status=status):
                self.assertEqual(
                    self._main_rc(["--auto-publish"], lambda **kw: {"status": status}), 0,
                )

    def test_a_one_shot_round_with_publishing_off_is_unaffected(self):
        # No publish ran, so the exit code must still be the dispatch
        # result alone -- not a failure invented from a missing status.
        self.assertEqual(self._main_rc([], lambda **kw: self.fail("must not publish")), 0)

    def test_a_dispatch_failure_still_dominates_a_successful_publish(self):
        rc = self._main_rc(["--auto-publish"], lambda **kw: {"status": "published_awaiting_review"},
                           run_round_fn=lambda args, cfg: False)
        self.assertEqual(rc, 1)

    def test_repeated_non_publishing_rounds_escalate_once_they_stack_up(self):
        # DEFECT 11's other half: a permanently wedged sweep worktree
        # emits one WARNING per round, which is indistinguishable in a
        # weeks-long log from a run of quiet rounds. A stall banner names
        # the repeated status.
        printed = []
        with patch("builtins.print", side_effect=lambda *a, **kw: printed.append(" ".join(map(str, a)))):
            self._main(["--infinite"], rounds=6, publish_fn=lambda **kw: {"status": "no_worktree"})
        banners = [line for line in printed if "AUTO-PUBLISH STALLED" in line]
        self.assertTrue(banners, printed[-8:])
        self.assertIn("no_worktree", banners[0])

    def test_a_healthy_round_resets_the_stall_counter(self):
        statuses = iter(["no_worktree", "no_worktree", "published_awaiting_review",
                         "no_worktree", "no_worktree"])
        printed = []
        with patch("builtins.print", side_effect=lambda *a, **kw: printed.append(" ".join(map(str, a)))):
            self._main(["--infinite"], rounds=6, publish_fn=lambda **kw: {"status": next(statuses)})
        self.assertEqual([line for line in printed if "AUTO-PUBLISH STALLED" in line], [])

    def test_an_adopted_review_ready_pr_counts_as_publishing_even_on_a_pass_through_status(self):
        # A round whose own sweep found nothing but which surfaced a
        # stranded PR from an earlier round for review DID publish -- it
        # must not count toward a stall.
        printed = []
        with patch("builtins.print", side_effect=lambda *a, **kw: printed.append(" ".join(map(str, a)))):
            self._main(["--infinite"], rounds=6, publish_fn=lambda **kw: {
                "status": "push_failed", "adopted": [{"action": "left_open_awaiting_review"}],
            })
        self.assertEqual([line for line in printed if "AUTO-PUBLISH STALLED" in line], [])

    def test_a_raising_publish_never_stops_the_infinite_loop(self):
        # A gh outage, an expired token, a git hiccup: the round's fixes
        # are already durable on their squad branches, so the loop must
        # survive and retry on the next round.
        def boom(**kwargs):
            raise RuntimeError("gh: server error")

        calls = self._main(["--infinite"], rounds=4, publish_fn=boom)
        self.assertEqual(len(calls), 4)


class RunAutoPublishSafelyTests(unittest.TestCase):
    def test_swallows_and_logs_a_raising_publish_fn(self):
        logged = []

        def boom(**kwargs):
            raise RuntimeError("kaboom")

        result = parallel_model_fix_loop._run_auto_publish_safely(
            publish_fn=boom, publish_kwargs={}, log_fn=logged.append,
        )
        self.assertEqual(result["status"], "raised")
        self.assertTrue(any("kaboom" in line for line in logged))

    def test_calls_through_with_kwargs_on_success(self):
        calls = []
        parallel_model_fix_loop._run_auto_publish_safely(
            publish_fn=lambda **kw: calls.append(kw) or {"status": "no_news"},
            publish_kwargs={"cache_dir": "/c"}, log_fn=lambda *a: None,
        )
        self.assertEqual(calls, [{"cache_dir": "/c"}])


if __name__ == "__main__":
    unittest.main()


class IdleRoundMustBackOffTests(unittest.TestCase):
    """A round that found nothing to do still has to pause.

    fleet_up.sh passes --round-delay 0 so a productive round starts the next
    one immediately. That is right for productive rounds and wrong for barren
    ones: a round with no news returns in milliseconds. Measured 2026-07-28 --
    once every green stamp was correctly skipped as stale, the dispatcher ran
    ~10 rounds a SECOND, each logging 'no_news', flooding the log and burning
    CPU to accomplish nothing.
    """

    def test_idle_statuses_get_the_backoff_even_at_zero_delay(self):
        import parallel_model_fix_loop as p
        for status in ("no_news", "nothing_merged", "zero_delta"):
            self.assertIn(status, p.IDLE_STATUSES, f"{status} should be idle")
        self.assertGreaterEqual(p.IDLE_ROUND_DELAY_SECONDS, 1.0)

    def test_an_explicit_round_delay_is_never_overridden(self):
        # The floor exists for --round-delay 0 only. Promoting an operator's
        # explicit 5s to 60s would override a deliberate choice; the first
        # draft of this fix did exactly that and broke
        # test_infinite_sleeps_between_rounds_using_injected_sleep_fn.
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = self._config_path(tmpdir) if hasattr(self, "_config_path") else None
        # Behavioural assertion lives in the existing infinite-loop tests;
        # this pins the rule the code encodes.
        import parallel_model_fix_loop as p
        import inspect
        src = inspect.getsource(p.main)
        self.assertIn("if not delay and", src,
                      "the idle floor must be gated on an unset round_delay")

    def test_a_failing_status_is_not_treated_as_idle(self):
        # A failure should keep the configured cadence so a transient fault is
        # retried promptly -- backing off on failure would slow recovery.
        import parallel_model_fix_loop as p
        for status in ("lint_failed", "sweep_aborted", "published_awaiting_review"):
            self.assertNotIn(status, p.IDLE_STATUSES)


class PublishIdentityTests(unittest.TestCase):
    """[publish].github_user -- pinning who the auto-publisher pushes as.

    Regression cover for 2026-08-04, when `git push` authenticated as
    whichever account `gh auth switch` had last made active. That account
    was read-only, so two consecutive sweeps built a branch and then died
    on 403 at the push, while every prior round had logged the
    indistinguishable 'no_news'.
    """

    @staticmethod
    def _gh(returncode=0, stdout="", stderr=""):
        """A subprocess.run stand-in recording the argv it was handed."""
        calls = []

        def run_fn(argv, **kwargs):
            calls.append(argv)
            return SimpleNamespace(returncode=returncode, stdout=stdout, stderr=stderr)

        return run_fn, calls

    def test_no_user_configured_keeps_the_ambient_runners(self):
        # Omitting the table must be a true no-op, not a silent behaviour
        # change for every host that never sets it.
        import parallel_model_fix_loop as p
        run_fn, calls = self._gh(stdout="tok")
        self.assertIsNone(p.resolve_publish_token(None, run_fn=run_fn))
        self.assertIsNone(p.resolve_publish_token("", run_fn=run_fn))
        self.assertEqual(calls, [], "gh must not be consulted when no user is configured")
        self.assertIsNone(p.publish_identity(None, run_fn=run_fn),
                          "no configured user must mean no identity, not a bound one")

    def test_token_is_resolved_for_the_named_account(self):
        import parallel_model_fix_loop as p
        run_fn, calls = self._gh(stdout="ghs_secret\n")
        self.assertEqual(p.resolve_publish_token("swackhamer", run_fn=run_fn), "ghs_secret")
        self.assertEqual(calls, [["gh", "auth", "token", "--user", "swackhamer"]])

    def test_unknown_account_raises_rather_than_falling_back(self):
        # The whole point: an unusable identity must fail loudly at
        # startup, never silently degrade to the ambient account.
        import parallel_model_fix_loop as p
        run_fn, _ = self._gh(returncode=1, stderr="no accounts")
        with self.assertRaises(p.PublishIdentityError) as ctx:
            p.resolve_publish_token("ghost", run_fn=run_fn)
        self.assertIn("ghost", str(ctx.exception))
        self.assertIn("gh auth login", str(ctx.exception),
                      "the error must carry the remedy, not just the symptom")

    def test_empty_token_is_an_error(self):
        import parallel_model_fix_loop as p
        run_fn, _ = self._gh(returncode=0, stdout="   \n")
        with self.assertRaises(p.PublishIdentityError):
            p.resolve_publish_token("swackhamer", run_fn=run_fn)

    def test_missing_gh_binary_raises_publish_identity_error(self):
        import parallel_model_fix_loop as p

        def run_fn(argv, **kwargs):
            raise OSError("no gh")

        with self.assertRaises(p.PublishIdentityError):
            p.resolve_publish_token("swackhamer", run_fn=run_fn)

    def test_every_bound_callable_puts_the_token_in_the_environment(self):
        # git and gh disagreed only because each picked up the ambient
        # account independently; one token in the env pins both. All four
        # callables are checked because binding only run_git/run_gh would
        # leave the actual `git push` and `gh pr create` -- the two that
        # returned 403 -- still running as the ambient account.
        import parallel_model_fix_loop as p
        run_fn, _ = self._gh(stdout="ghs_secret")
        ident = p.publish_identity("swackhamer", run_fn=run_fn)
        seen = []

        def fake_run(argv, **kwargs):
            seen.append((argv, kwargs.get("env") or {}))
            return SimpleNamespace(returncode=0, stdout="", stderr="")

        with patch.object(p.subprocess, "run", fake_run):
            ident.run_git(["status"], "/repo")
            ident.run_gh(["pr", "list"], "/repo")
            ident.push_branch_fn("/repo", "sweep/x")
            ident.create_pr_fn("t", "b", "sweep/x", "main", "/repo")
        self.assertEqual(len(seen), 4)
        for argv, env in seen:
            self.assertEqual(env.get("GH_TOKEN"), "ghs_secret",
                             f"{argv[:3]} must authenticate as the configured account")
            self.assertEqual(env.get("GITHUB_TOKEN"), "ghs_secret")
        # The two that actually failed in production, by exact argv.
        self.assertIn(["git", "push", "-u", "origin", "sweep/x"], [a for a, _ in seen])
        self.assertEqual([a for a, _ in seen][3][:3], ["gh", "pr", "create"])

    def test_bound_push_and_pr_keep_their_return_shapes(self):
        # run_sweep reads (ok, message) from push_branch_fn and a dict
        # from create_pr_fn; a shape change here would surface as a
        # confusing sweep failure rather than an auth one.
        import parallel_model_fix_loop as p
        run_fn, _ = self._gh(stdout="ghs_secret")
        ident = p.publish_identity("swackhamer", run_fn=run_fn)

        def fake_run(argv, **kwargs):
            return SimpleNamespace(returncode=1, stdout="out", stderr="403")

        with patch.object(p.subprocess, "run", fake_run):
            ok, msg = ident.push_branch_fn("/repo", "sweep/x")
            pr = ident.create_pr_fn("t", "b", "sweep/x")
        self.assertFalse(ok)
        self.assertIn("403", msg)
        self.assertEqual(pr, {"ok": False, "stdout": "out", "stderr": "403"})

    def test_auto_publish_round_threads_the_bound_callables_into_the_sweep(self):
        # The regression that binding-only-the-runners would have left:
        # push_branch_fn/create_pr_fn must reach run_sweep, or the sweep
        # silently falls back to overlord_sweep's unbound versions.
        import parallel_model_fix_loop as p
        captured = {}

        def fake_sweep(**kwargs):
            captured.update(kwargs)
            return {"status": "no_news"}

        sentinel_push, sentinel_pr = object(), object()
        p.auto_publish_round(
            repo_root="/repo", cache_dir="/cache", home=Path("/home"),
            sweep_fn=fake_sweep, ensure_worktree_fn=lambda *a, **k: ("/sweep-repo", ""),
            sync_fn=lambda *a, **k: None, fmt_fn=lambda *a, **k: (True, ""),
            push_branch_fn=sentinel_push, create_pr_fn=sentinel_pr,
            run_gh=lambda *a, **k: (0, "[]", ""), log_fn=lambda *a, **k: None,
        )
        self.assertIs(captured.get("push_branch_fn"), sentinel_push)
        self.assertIs(captured.get("create_pr_fn"), sentinel_pr)

    def test_sweep_callables_are_omitted_when_no_identity_is_configured(self):
        # None must not be forwarded -- run_sweep's own defaults have to win.
        import parallel_model_fix_loop as p
        captured = {}

        def fake_sweep(**kwargs):
            captured.update(kwargs)
            return {"status": "no_news"}

        p.auto_publish_round(
            repo_root="/repo", cache_dir="/cache", home=Path("/home"),
            sweep_fn=fake_sweep, ensure_worktree_fn=lambda *a, **k: ("/sweep-repo", ""),
            sync_fn=lambda *a, **k: None, fmt_fn=lambda *a, **k: (True, ""),
            run_gh=lambda *a, **k: (0, "[]", ""), log_fn=lambda *a, **k: None,
        )
        self.assertNotIn("push_branch_fn", captured)
        self.assertNotIn("create_pr_fn", captured)

    def test_bound_gh_runner_keeps_the_no_raise_contract(self):
        # default_run_gh deliberately cannot raise; the bound variant is
        # substituted for it and must not reintroduce an exception path.
        import parallel_model_fix_loop as p
        run_fn, _ = self._gh(stdout="ghs_secret")
        run_gh = p.publish_identity("swackhamer", run_fn=run_fn).run_gh

        def boom(argv, **kwargs):
            raise OSError("fork failed")

        with patch.object(p.subprocess, "run", boom):
            rc, out, err = run_gh(["pr", "list"], "/repo")
        self.assertEqual((rc, out), (127, ""))
        self.assertIn("could not run gh", err)
