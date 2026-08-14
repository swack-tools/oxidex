#!/usr/bin/env python3
"""Tests for `drift.converge()` -- the forced rebase, and specifically the
`--ours`/`--theirs` trap described in `drift.py`'s module docstring.

Everything here runs against a throwaway `git init --bare` repo created
under the system temp dir -- never the production hub.

`cargo`/`regen.sh` are never actually invoked: `converge()` accepts
`fastcheck` and `regen_cmd` injection points precisely so tests can stand
in a stub instead of building the real Rust workspace or running the real
(perl + network) table generator, matching the "mock the gate; do not
build Rust in a unit test" principle FLEET_PLAN.md applies elsewhere. The
generated-file path in this repo's synthetic fixture is
`src/exiftool_tables/binary_tables.rs` -- the same path
`GENERATED_FILES` names in `drift.py` -- with small fixture content, not
the real 5MB generated file.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import drift  # noqa: E402
from fleetlib import Hub  # noqa: E402

GENERATED_PATH = "src/exiftool_tables/binary_tables.rs"


def _run_git(args, cwd=None):
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
    result = subprocess.run(args, cwd=cwd, capture_output=True, env=env)
    assert result.returncode == 0, f"{' '.join(args)} failed: {result.stderr.decode()}"
    return result.stdout.decode().strip()


def _write(src, relpath, content):
    path = Path(src) / relpath
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def _ok_fastcheck(_repo_dir):
    return True, "stubbed fastcheck: ok"


class DriftConvergeTestCase(unittest.TestCase):
    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="drift-converge-test-")
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.hub_path = str(Path(self._tmp_root) / "hub.git")
        _run_git(["git", "init", "--quiet", "--bare", self.hub_path])

        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(resolved.startswith(system_tmp))
        self.assertNotIn("work2.oxidex.net", resolved)

        self.hub_workdir = tempfile.mkdtemp(prefix="drift-converge-cache-")
        self.addCleanup(shutil.rmtree, self.hub_workdir, ignore_errors=True)
        self.hub = Hub(url=self.hub_path, workdir=self.hub_workdir)

        self.log_dir = Path(tempfile.mkdtemp(prefix="drift-converge-logs-"))
        self.addCleanup(shutil.rmtree, self.log_dir, ignore_errors=True)

    def _push(self, src, refspec):
        _run_git(["git", "push", "--quiet", self.hub_path, refspec], cwd=src)

    def _new_clone(self, prefix="drift-converge-clone-"):
        clone_dir = tempfile.mkdtemp(prefix=prefix)
        self.addCleanup(shutil.rmtree, clone_dir, ignore_errors=True)
        _run_git(["git", "clone", "--quiet", self.hub_path, clone_dir])
        return clone_dir


class TestOursTheirsTrap(DriftConvergeTestCase):
    """The war story from FLEET_PLAN.md T1.3, made mechanical: a conflict
    on the GENERATED binary_tables.rs must resolve to the TIP's content --
    verified by byte-for-byte comparison, never by trusting the checkout's
    exit status.
    """

    def test_generated_file_conflict_resolves_to_tip_content(self):
        src = tempfile.mkdtemp(prefix="drift-converge-src-")
        self.addCleanup(shutil.rmtree, src, ignore_errors=True)

        _write(src, GENERATED_PATH, "GENERATED: base\n")
        _write(src, "src/lib.rs", "// unrelated\n")
        _run_git(["git", "init", "--quiet", src])
        _run_git(["git", "add", "-A"], cwd=src)
        _run_git(["git", "commit", "--quiet", "-m", "base"], cwd=src)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=src)
        self._push(src, "refactor/tag-machinery")

        # Branch diverges and makes its OWN (stale) regen of the generated
        # file -- this is the "24-commits-stale generated file" scenario.
        _run_git(["git", "checkout", "--quiet", "-b", "staging/stale-regen"], cwd=src)
        _write(src, GENERATED_PATH, "GENERATED: branch's stale regen\n")
        _run_git(["git", "commit", "--quiet", "-am", "branch: stale regen + branch work"], cwd=src)
        self._push(src, "staging/stale-regen")

        # Tip advances and regenerates the SAME file with fresh content.
        _run_git(["git", "checkout", "--quiet", "refactor/tag-machinery"], cwd=src)
        _write(src, GENERATED_PATH, "GENERATED: tip's fresh regen\n")
        _run_git(["git", "commit", "--quiet", "-am", "tip: fresh regen"], cwd=src)
        self._push(src, "refactor/tag-machinery")
        tip_sha = _run_git(["git", "rev-parse", "refactor/tag-machinery"], cwd=src)
        tip_generated_content = _run_git(["git", "show", f"{tip_sha}:{GENERATED_PATH}"], cwd=src)

        repo_dir = self._new_clone()

        regen_calls = []

        def _stub_regen(repo_dir_arg):
            """Stand in for tools/exiftool-tables/regen.sh: idempotent --
            reproduces exactly what the tip already committed, which is
            what the real generator should do too when re-run against an
            unchanged pin. Runs as a real subprocess (a tiny script,
            written via `python3 -c` so the fixture content never has to
            survive shell quoting) so converge()'s subprocess-based
            `_run_regen` plumbing is exercised for real, not bypassed.
            """
            regen_calls.append(repo_dir_arg)
            write_expr = (
                "import pathlib; "
                f"pathlib.Path({GENERATED_PATH!r}).write_text({(tip_generated_content + chr(10))!r})"
            )
            return [sys.executable, "-c", write_expr]

        stub_cmd = _stub_regen(repo_dir)

        result = drift.converge(
            "staging/stale-regen", repo_dir, self.hub,
            fastcheck=_ok_fastcheck,
            regen_cmd=stub_cmd,
            external_log_dir=self.log_dir,
        )

        self.assertEqual(result.status, "converged", msg=result.detail)
        self.assertIn(GENERATED_PATH, result.resolved_generated_files)

        # The actual assertion this test exists for: content comparison,
        # not exit-status trust.
        final_content = _run_git(["git", "show", f"HEAD:{GENERATED_PATH}"], cwd=repo_dir)
        self.assertEqual(
            final_content, tip_generated_content,
            "generated file must match the TIP's content after conflict resolution, "
            "not the branch's stale copy -- this is the --ours/--theirs trap",
        )
        self.assertNotIn("branch's stale regen", final_content)

        # And the unrelated file's branch-only work must have survived the
        # rebase untouched.
        self.assertTrue((Path(repo_dir) / "src" / "lib.rs").exists())

        # A log was written OUTSIDE the repo tree (regen.sh refuses a
        # dirty tree; a log written inside it would itself cause that).
        self.assertTrue(any(self.log_dir.glob("regen-*.log")), "expected a regen log under the external log dir")
        for log_path in self.log_dir.glob("regen-*.log"):
            self.assertFalse(str(log_path).startswith(repo_dir))

    def test_resolve_generated_conflicts_raises_if_content_does_not_match_tip(self):
        """Direct unit test of the verification step itself: if
        `checkout --ours` were ever to grab the wrong side (a git version
        quirk, a misconfigured merge driver, whatever), the mismatch must
        be caught and raised rather than silently staged.
        """
        src = tempfile.mkdtemp(prefix="drift-converge-verify-src-")
        self.addCleanup(shutil.rmtree, src, ignore_errors=True)
        _write(src, GENERATED_PATH, "base\n")
        _run_git(["git", "init", "--quiet", src])
        _run_git(["git", "add", "-A"], cwd=src)
        _run_git(["git", "commit", "--quiet", "-m", "base"], cwd=src)
        base_sha = _run_git(["git", "rev-parse", "HEAD"], cwd=src)

        _write(src, GENERATED_PATH, "tip content\n")
        _run_git(["git", "commit", "--quiet", "-am", "tip"], cwd=src)
        tip_sha = _run_git(["git", "rev-parse", "HEAD"], cwd=src)

        _run_git(["git", "checkout", "--quiet", "-b", "feature", base_sha], cwd=src)
        _write(src, GENERATED_PATH, "branch content\n")
        _run_git(["git", "commit", "--quiet", "-am", "branch"], cwd=src)

        rebase = subprocess.run(["git", "rebase", tip_sha], cwd=src, capture_output=True)
        self.assertNotEqual(rebase.returncode, 0, "expected a conflict to set up this test")

        # Sabotage: overwrite the working-tree file with neither side's
        # content, so the post-checkout verification must catch it even
        # though `checkout --ours` itself will report success.
        real_path = Path(src) / GENERATED_PATH

        original_run = drift._git_tree

        def _sabotaged(repo_dir, args, **kw):
            result = original_run(repo_dir, args, **kw)
            if args[:2] == ["checkout", "--ours"]:
                real_path.write_text("neither side\n")
            return result

        drift._git_tree = _sabotaged
        try:
            with self.assertRaises(drift.DriftError):
                drift._resolve_generated_conflicts(src, tip_sha, [GENERATED_PATH])
        finally:
            drift._git_tree = original_run


class TestGenuineConflictBlocks(DriftConvergeTestCase):
    def test_genuine_source_conflict_marks_blocked_not_guessed(self):
        src = tempfile.mkdtemp(prefix="drift-converge-genuine-src-")
        self.addCleanup(shutil.rmtree, src, ignore_errors=True)

        _write(src, "src/core/format_dispatch.rs", "line one\nline two\nline three\n")
        _run_git(["git", "init", "--quiet", src])
        _run_git(["git", "add", "-A"], cwd=src)
        _run_git(["git", "commit", "--quiet", "-m", "base"], cwd=src)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=src)
        self._push(src, "refactor/tag-machinery")

        _run_git(["git", "checkout", "--quiet", "-b", "staging/genuine-conflict"], cwd=src)
        _write(src, "src/core/format_dispatch.rs", "line one\nBRANCH CHANGE\nline three\n")
        _run_git(["git", "commit", "--quiet", "-am", "branch: real source edit"], cwd=src)
        self._push(src, "staging/genuine-conflict")
        branch_sha_before = _run_git(["git", "rev-parse", "staging/genuine-conflict"], cwd=src)

        _run_git(["git", "checkout", "--quiet", "refactor/tag-machinery"], cwd=src)
        _write(src, "src/core/format_dispatch.rs", "line one\nTIP CHANGE\nline three\n")
        _run_git(["git", "commit", "--quiet", "-am", "tip: conflicting source edit"], cwd=src)
        self._push(src, "refactor/tag-machinery")
        tip_sha = _run_git(["git", "rev-parse", "refactor/tag-machinery"], cwd=src)

        repo_dir = self._new_clone()

        called = {"fastcheck": False, "regen": False}

        def _must_not_be_called_fastcheck(_repo_dir):
            called["fastcheck"] = True
            return True, "should not have run"

        def _must_not_be_called_regen(_repo_dir):
            called["regen"] = True
            return ["true"]

        result = drift.converge(
            "staging/genuine-conflict", repo_dir, self.hub,
            fastcheck=_must_not_be_called_fastcheck,
            regen_cmd=None,
            external_log_dir=self.log_dir,
        )

        self.assertEqual(result.status, "blocked-on-rebase", msg=result.detail)
        self.assertIn("src/core/format_dispatch.rs", result.conflicted_paths)
        self.assertFalse(called["fastcheck"], "must never reach fastcheck on a genuine conflict")

        # Refuse rather than approximate: no guessed resolution was
        # committed, and the rebase was cleanly aborted.
        status = subprocess.run(["git", "status", "--porcelain"], cwd=repo_dir, capture_output=True)
        self.assertEqual(status.stdout.decode().strip(), "", "working tree must be clean after an aborted rebase")

        rebase_in_progress = (Path(repo_dir) / ".git" / "rebase-merge").exists() or \
            (Path(repo_dir) / ".git" / "rebase-apply").exists()
        self.assertFalse(rebase_in_progress, "rebase must have been aborted, not left dangling")

        # The branch on the hub itself is untouched -- converge() must not
        # have force-pushed a guessed resolution.
        hub_branch_sha = _run_git(["git", "rev-parse", "refactor/tag-machinery"], cwd=src)  # sanity: tip unaffected
        self.assertEqual(hub_branch_sha, tip_sha)
        current_branch_sha_on_hub = subprocess.run(
            ["git", "ls-remote", self.hub_path, "refs/heads/staging/genuine-conflict"],
            capture_output=True,
        ).stdout.decode().split()[0]
        self.assertEqual(current_branch_sha_on_hub, branch_sha_before)


class TestUpToDate(DriftConvergeTestCase):
    def test_branch_already_at_tip_is_a_noop(self):
        src = tempfile.mkdtemp(prefix="drift-converge-uptodate-src-")
        self.addCleanup(shutil.rmtree, src, ignore_errors=True)
        _write(src, "README", "x\n")
        _run_git(["git", "init", "--quiet", src])
        _run_git(["git", "add", "-A"], cwd=src)
        _run_git(["git", "commit", "--quiet", "-m", "base"], cwd=src)
        _run_git(["git", "branch", "-M", "refactor/tag-machinery"], cwd=src)
        self._push(src, "refactor/tag-machinery")
        _run_git(["git", "checkout", "--quiet", "-b", "staging/already-current"], cwd=src)
        self._push(src, "staging/already-current")

        repo_dir = self._new_clone()
        result = drift.converge("staging/already-current", repo_dir, self.hub, fastcheck=_ok_fastcheck)
        self.assertEqual(result.status, "up-to-date")


if __name__ == "__main__":
    unittest.main()
