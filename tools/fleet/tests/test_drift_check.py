#!/usr/bin/env python3
"""Tests for `drift.check()` -- the drift budget.

Everything here runs against a throwaway `git init --bare` repo created
under the system temp dir -- never the production hub. `DriftCheckTestCase`
asserts the fixture URL is a temp path before any test body runs.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import drift  # noqa: E402
from fleetlib import Hub  # noqa: E402

SECONDS_PER_MINUTE = 60


def _run_git(args, cwd=None, env_extra=None):
    env = dict(os.environ)
    env.update(
        {
            "GIT_AUTHOR_NAME": "t",
            "GIT_AUTHOR_EMAIL": "t@t",
            "GIT_COMMITTER_NAME": "t",
            "GIT_COMMITTER_EMAIL": "t@t",
            "GIT_TERMINAL_PROMPT": "0",
        }
    )
    if env_extra:
        env.update(env_extra)
    result = subprocess.run(args, cwd=cwd, capture_output=True, env=env)
    assert result.returncode == 0, f"{' '.join(args)} failed: {result.stderr.decode()}"
    return result.stdout.decode().strip()


def _commit_at(src, message, epoch_seconds, filename="f.txt", content=None):
    """Create a commit whose author+committer date is `epoch_seconds`, so
    tests can construct branches that are behind by a controlled number of
    *minutes* as well as commits.
    """
    (Path(src) / filename).write_text(content if content is not None else message)
    _run_git(["git", "add", filename], cwd=src)
    date = f"{int(epoch_seconds)} +0000"
    _run_git(
        ["git", "commit", "--quiet", "-m", message],
        cwd=src,
        env_extra={"GIT_AUTHOR_DATE": date, "GIT_COMMITTER_DATE": date},
    )
    return _run_git(["git", "rev-parse", "HEAD"], cwd=src)


class DriftCheckTestCase(unittest.TestCase):
    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="drift-check-test-")
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.hub_path = str(Path(self._tmp_root) / "hub.git")
        _run_git(["git", "init", "--quiet", "--bare", self.hub_path])

        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(resolved.startswith(system_tmp))
        self.assertNotIn("work2.oxidex.net", resolved)

        self.src = tempfile.mkdtemp(prefix="drift-check-src-")
        self.addCleanup(shutil.rmtree, self.src, ignore_errors=True)
        _run_git(["git", "clone", "--quiet", self.hub_path, self.src])

        self.hub_workdir = tempfile.mkdtemp(prefix="drift-check-cache-")
        self.addCleanup(shutil.rmtree, self.hub_workdir, ignore_errors=True)
        self.hub = Hub(url=self.hub_path, workdir=self.hub_workdir)

    def _push(self, refspec):
        _run_git(["git", "push", "--quiet", self.hub_path, refspec], cwd=self.src)


class TestCommitsBehind(DriftCheckTestCase):
    def test_branch_at_tip_is_zero_behind_and_ok(self):
        now = time.time()
        _commit_at(self.src, "base", now - 5 * SECONDS_PER_MINUTE)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=self.src)
        self._push("refactor/tag-machinery")
        _run_git(["git", "checkout", "--quiet", "-b", "staging/at-tip"], cwd=self.src)
        self._push("staging/at-tip")

        status = drift.check("staging/at-tip", self.hub, now=now)
        self.assertEqual(status.commits_behind, 0)
        self.assertEqual(status.minutes_behind, 0.0)
        self.assertTrue(status.ok)
        self.assertEqual(status.as_tuple(), (0, 0.0, True))

    def test_branch_within_budget_is_accepted(self):
        now = time.time()
        _commit_at(self.src, "base", now - 60 * SECONDS_PER_MINUTE)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=self.src)
        self._push("refactor/tag-machinery")

        _run_git(["git", "checkout", "--quiet", "-b", "staging/three-behind"], cwd=self.src)
        self._push("staging/three-behind")

        # Tip advances by 3 commits, most recent one 2 minutes ago --
        # comfortably inside MAX_DRIFT_COMMITS=5 and MAX_DRIFT_MINUTES=30.
        _run_git(["git", "checkout", "--quiet", "refactor/tag-machinery"], cwd=self.src)
        for i in range(3):
            _commit_at(self.src, f"tip advance {i}", now - (3 - i) * SECONDS_PER_MINUTE)
        self._push("refactor/tag-machinery")

        status = drift.check("staging/three-behind", self.hub, now=now)
        self.assertEqual(status.commits_behind, 3)
        self.assertLess(status.minutes_behind, 30)
        self.assertTrue(status.ok, msg=f"expected ok=True at {status.commits_behind} behind, "
                                        f"{status.minutes_behind:.1f}m -- {status}")

    def test_branch_six_commits_behind_is_refused(self):
        """The exact scenario named in FLEET_PLAN.md T1.3: 'Today's queue
        held branches 6, 8 and 24 commits behind.' 6 > MAX_DRIFT_COMMITS=5,
        so this must be refused even though it is well inside the time
        budget.
        """
        now = time.time()
        _commit_at(self.src, "base", now - 10 * SECONDS_PER_MINUTE)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=self.src)
        self._push("refactor/tag-machinery")

        _run_git(["git", "checkout", "--quiet", "-b", "staging/six-behind"], cwd=self.src)
        self._push("staging/six-behind")

        _run_git(["git", "checkout", "--quiet", "refactor/tag-machinery"], cwd=self.src)
        for i in range(6):
            _commit_at(self.src, f"tip advance {i}", now - (6 - i) * SECONDS_PER_MINUTE)
        self._push("refactor/tag-machinery")

        status = drift.check("staging/six-behind", self.hub, now=now)
        self.assertEqual(status.commits_behind, 6)
        self.assertFalse(status.ok, "6 commits behind must exceed MAX_DRIFT_COMMITS=5 and be refused")

    def test_branch_over_time_budget_but_under_commit_budget_is_refused(self):
        """Only 2 commits behind (well under 5), but the oldest of them
        landed 45 minutes ago -- over MAX_DRIFT_MINUTES=30. Must still be
        refused: the two budgets are independent, either one tripping is
        enough.
        """
        now = time.time()
        _commit_at(self.src, "base", now - 120 * SECONDS_PER_MINUTE)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=self.src)
        self._push("refactor/tag-machinery")

        _run_git(["git", "checkout", "--quiet", "-b", "staging/stale-but-shallow"], cwd=self.src)
        self._push("staging/stale-but-shallow")

        _run_git(["git", "checkout", "--quiet", "refactor/tag-machinery"], cwd=self.src)
        _commit_at(self.src, "tip advance old", now - 45 * SECONDS_PER_MINUTE)
        _commit_at(self.src, "tip advance recent", now - 1 * SECONDS_PER_MINUTE)
        self._push("refactor/tag-machinery")

        status = drift.check("staging/stale-but-shallow", self.hub, now=now)
        self.assertEqual(status.commits_behind, 2)
        self.assertGreater(status.minutes_behind, 30)
        self.assertFalse(status.ok)


class TestCheckThenConvergeThenAccepted(DriftCheckTestCase):
    def test_six_behind_refused_then_converge_then_accepted(self):
        """The acceptance scenario from FLEET_PLAN.md T1.3's own test
        list: 'against the fixture hub -- a branch 6 behind is refused,
        converges, is then accepted.'
        """
        now = time.time()
        _commit_at(self.src, "base", now - 10 * SECONDS_PER_MINUTE)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=self.src)
        self._push("refactor/tag-machinery")

        _run_git(["git", "checkout", "--quiet", "-b", "staging/needs-converge"], cwd=self.src)
        _commit_at(self.src, "branch work", now - 5 * SECONDS_PER_MINUTE, filename="branch-only.txt")
        self._push("staging/needs-converge")

        _run_git(["git", "checkout", "--quiet", "refactor/tag-machinery"], cwd=self.src)
        for i in range(6):
            _commit_at(self.src, f"tip advance {i}", now - (6 - i) * SECONDS_PER_MINUTE,
                       filename=f"tip-{i}.txt")
        self._push("refactor/tag-machinery")

        before = drift.check("staging/needs-converge", self.hub, now=now)
        self.assertEqual(before.commits_behind, 6)
        self.assertFalse(before.ok, "must be refused a gate claim before converging")

        repo_dir = tempfile.mkdtemp(prefix="drift-check-converge-repo-")
        self.addCleanup(shutil.rmtree, repo_dir, ignore_errors=True)
        _run_git(["git", "clone", "--quiet", self.hub_path, repo_dir])

        def _stub_fastcheck(_repo_dir):
            return True, "stubbed fastcheck: ok (no Rust workspace in this synthetic fixture)"

        result = drift.converge("staging/needs-converge", repo_dir, self.hub, fastcheck=_stub_fastcheck)
        self.assertEqual(result.status, "converged", msg=result.detail)

        after = drift.check("staging/needs-converge", self.hub, now=now)
        self.assertEqual(after.commits_behind, 0, msg=f"still behind after converge: {after}")
        self.assertTrue(after.ok, "must be accepted for a gate claim after converging")


class TestSignalConsumption(DriftCheckTestCase):
    def test_check_uses_signal_sha_when_fresh(self):
        now = time.time()
        _commit_at(self.src, "base", now - 5 * SECONDS_PER_MINUTE)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=self.src)
        self._push("refactor/tag-machinery")
        tip_sha = _run_git(["git", "rev-parse", "refactor/tag-machinery"], cwd=self.src)

        _run_git(["git", "checkout", "--quiet", "-b", "staging/watched"], cwd=self.src)
        self._push("staging/watched")

        ok = self.hub.create("refs/fleet/signals/tip", {"sha": tip_sha, "generation": 1, "ts": "now"})
        self.assertTrue(ok)

        status = drift.check("staging/watched", self.hub, now=now)
        self.assertFalse(status.signal_is_stale)
        self.assertEqual(status.tip_sha, tip_sha)

    def test_check_detects_stale_signal_but_still_uses_real_tip(self):
        now = time.time()
        _commit_at(self.src, "base", now - 5 * SECONDS_PER_MINUTE)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=self.src)
        self._push("refactor/tag-machinery")

        _run_git(["git", "checkout", "--quiet", "-b", "staging/watched2"], cwd=self.src)
        self._push("staging/watched2")

        # A signal pointing at a sha that is NOT the real tip (hook never
        # fired, or fired late) must never be trusted blindly.
        self.hub.create("refs/fleet/signals/tip", {"sha": "f" * 40, "generation": 1, "ts": "now"})

        real_tip_sha = _run_git(["git", "rev-parse", "refactor/tag-machinery"], cwd=self.src)
        status = drift.check("staging/watched2", self.hub, now=now)
        self.assertTrue(status.signal_is_stale)
        self.assertEqual(status.tip_sha, real_tip_sha,
                          "check() must use the real refs/heads tip, never trust a stale signal")


if __name__ == "__main__":
    unittest.main()
