"""Shared git-repo test fixture for `test_verdict*.py`.

Builds small, real git repositories in a throwaway temp directory so
`GitRepo.is_ancestor` / `commits_between` / `files_touched` run against
actual git plumbing rather than a mock of it. Every repo built here lives
under the system temp directory -- asserted by `require_temp_path`, the
same guard `test_fleetlib.py` uses for the hub fixture -- and is expected
to be cleaned up by the caller's `unittest.TestCase.addCleanup`.

This file intentionally does not match `test_*.py` so `unittest discover`
does not try to run it as a test module by itself.
"""

from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path
from typing import Dict


def require_temp_path(path) -> None:
    resolved = str(Path(path).resolve())
    system_tmp = str(Path(tempfile.gettempdir()).resolve())
    assert resolved.startswith(system_tmp), f"{resolved!r} is not under the system temp dir {system_tmp!r}"


class RepoBuilder:
    """A disposable, non-bare git repo for building commit graphs by hand."""

    def __init__(self, path):
        self.path = Path(path)
        self.path.mkdir(parents=True, exist_ok=True)
        require_temp_path(self.path)
        self._run("init", "--quiet", "-b", "main")
        self._run("config", "user.email", "fleet-test@oxidex.local")
        self._run("config", "user.name", "fleet-test")
        self._run("config", "commit.gpgsign", "false")

    def _run(self, *args: str) -> str:
        result = subprocess.run(["git", "-C", str(self.path), *args], capture_output=True, text=True)
        if result.returncode != 0:
            raise RuntimeError(f"git {' '.join(args)} failed: {result.stderr}")
        return result.stdout.strip()

    def commit(self, files: Dict[str, str], message: str) -> str:
        """Write/overwrite `files` (relative path -> content) and commit.

        Returns the new commit sha.
        """
        for rel, content in files.items():
            full = self.path / rel
            full.parent.mkdir(parents=True, exist_ok=True)
            full.write_text(content)
            self._run("add", rel)
        self._run("commit", "--quiet", "-m", message)
        return self._run("rev-parse", "HEAD")

    def sha(self, ref: str = "HEAD") -> str:
        return self._run("rev-parse", ref)
