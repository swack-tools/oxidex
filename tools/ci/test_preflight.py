"""Integration tests for the portable diagnostics emitted by preflight.sh."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO = Path(__file__).resolve().parents[2]
PREFLIGHT = REPO / "tools/preflight.sh"


class PreflightDiagnosticsTests(unittest.TestCase):
    def setUp(self) -> None:
        configured_tmpdir = os.environ.get("TMPDIR")
        if configured_tmpdir:
            scratch_parent = Path(configured_tmpdir)
        else:
            ops_dir = Path(os.environ.get("OXIDEX_OPS_DIR", Path.home() / "oxidex-ops"))
            scratch_parent = ops_dir / "tmp" / "preflight-portability-tests"
        scratch_parent.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(prefix="preflight-portability-", dir=scratch_parent)
        self.root = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)

    def run_git(self, repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", *args], cwd=repo, text=True, capture_output=True,
            env={**os.environ, "GIT_CONFIG_NOSYSTEM": "1"}, check=True,
        )

    def make_repo(self, name: str, branch: str) -> Path:
        repo = self.root / name
        repo.mkdir()
        self.run_git(repo, "init", "-b", branch)
        self.run_git(repo, "config", "user.name", "Preflight Test")
        self.run_git(repo, "config", "user.email", "preflight@example.invalid")
        self.run_git(repo, "config", "commit.gpgsign", "false")
        (repo / "tracked").write_text("tracked\n", encoding="utf-8")
        self.run_git(repo, "add", "tracked")
        self.run_git(repo, "-c", "commit.gpgsign=false", "commit", "-m", "initial")
        return repo

    def run_preflight(self, repo: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["bash", str(PREFLIGHT)], cwd=repo, text=True,
                              capture_output=True, env={**os.environ, "GIT_CONFIG_NOSYSTEM": "1"})

    def test_protected_branches_keep_exit_two(self):
        for branch in ("main", "refactor/tag-machinery"):
            with self.subTest(branch=branch):
                result = self.run_preflight(self.make_repo(branch.replace("/", "-"), branch))
                self.assertEqual(result.returncode, 2, result.stderr)

    def test_clean_staging_branch_is_successful(self):
        result = self.run_preflight(self.make_repo("clean", "staging/preflight"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("preflight: OK", result.stdout)

    def test_dirty_staging_branch_keeps_exit_three(self):
        repo = self.make_repo("dirty", "staging/preflight")
        (repo / "untracked").write_text("dirty\n", encoding="utf-8")
        result = self.run_preflight(repo)
        self.assertEqual(result.returncode, 3, result.stderr)

    def test_protected_diagnostic_quotes_spaces_and_uses_portable_destination(self):
        repo = self.make_repo("repo with spaces", "main")
        result = self.run_preflight(repo)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("git -C ", result.stderr)
        self.assertIn("repo\\ with\\ spaces", result.stderr)
        self.assertIn('"${OXIDEX_WORKTREE_ROOT:-$HOME/git}/<dir>"', result.stderr)


if __name__ == "__main__":
    unittest.main()
