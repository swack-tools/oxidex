"""Integration tests for the portable diagnostics emitted by preflight.sh."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO = Path(__file__).resolve().parents[2]
PREFLIGHT = REPO / "tools/preflight.sh"


class PreflightHarness(unittest.TestCase):
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

    def make_repo(self, name: str, branch: str, *, toolchain: str | None = None) -> Path:
        repo = self.root / name
        repo.mkdir()
        self.run_git(repo, "init", "-b", branch)
        self.run_git(repo, "config", "user.name", "Preflight Test")
        self.run_git(repo, "config", "user.email", "preflight@example.invalid")
        self.run_git(repo, "config", "commit.gpgsign", "false")
        (repo / "tracked").write_text("tracked\n", encoding="utf-8")
        if toolchain is not None:
            (repo / "rust-toolchain.toml").write_text(
                f'[toolchain]\nchannel = "{toolchain}"\ncomponents = ["rustfmt", "clippy"]\n', encoding="utf-8")
        self.run_git(repo, "add", ".")
        self.run_git(repo, "-c", "commit.gpgsign=false", "commit", "-m", "initial")
        return repo

    def run_preflight(self, repo: Path, env: dict | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["bash", str(PREFLIGHT)], cwd=repo, text=True,
                              capture_output=True, env={**os.environ, "GIT_CONFIG_NOSYSTEM": "1", **(env or {})})


class PreflightDiagnosticsTests(PreflightHarness):
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


class PreflightToolchainTests(PreflightHarness):
    """A compiler resolved implicitly from PATH instead of the pin is exit 6."""

    def fake_tools(self, name: str, rustc: str | None, cargo: str | None) -> Path:
        """A bin directory whose `rustc`/`cargo` report the given releases.

        None installs a tool that cannot run (exit 127), so whatever real
        toolchain the host has is always shadowed, never consulted.
        """
        directory = self.root / name
        directory.mkdir()
        broken = "echo 'not runnable' >&2\nexit 127\n"
        tools = {"rustc": broken, "cargo": broken}
        if rustc is not None:
            tools["rustc"] = (
                'case "$1" in\n'
                f'  -vV) printf "rustc {rustc} (fake 2026-01-01)\\nbinary: rustc\\n'
                f'commit-hash: {"f" * 40}\\nrelease: {rustc}\\n" ;;\n'
                f'  --print) echo "/fake/toolchains/{rustc}" ;;\n'
                "esac\n")
        if cargo is not None:
            tools["cargo"] = f'echo "cargo {cargo} (fake 2026-01-01)"\n'
        for tool, body in tools.items():
            script = directory / tool
            script.write_text("#!/bin/sh\n" + body, encoding="utf-8")
            script.chmod(0o755)
        return directory

    def env_for(self, *first: Path, **extra: str) -> dict:
        path = os.pathsep.join([*(str(entry) for entry in first), os.environ.get("PATH", os.defpath)])
        env = {"PATH": path, "RUSTC": "", "OXIDEX_ALLOW_TOOLCHAIN_SKEW": ""}
        env.update(extra)
        return env

    def test_pinned_rustc_and_cargo_pass(self):
        repo = self.make_repo("pinned", "staging/pin", toolchain="1.97.1")
        result = self.run_preflight(repo, self.env_for(self.fake_tools("bin", "1.97.1", "1.97.1")))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("toolchain: pin 1.97.1 (rust-toolchain.toml)", result.stdout)
        self.assertIn("-> rustc 1.97.1 (fake 2026-01-01)", result.stdout)
        self.assertIn("sysroot  : /fake/toolchains/1.97.1", result.stdout)
        self.assertIn("preflight: OK", result.stdout)

    def test_path_resolved_foreign_rustc_is_exit_six(self):
        repo = self.make_repo("skewed", "staging/pin", toolchain="1.97.1")
        result = self.run_preflight(repo, self.env_for(self.fake_tools("brew", "1.98.1", "1.98.1")))
        self.assertEqual(result.returncode, 6, result.stdout + result.stderr)
        self.assertIn("TOOLCHAIN MISMATCH: rustc is 1.98.1, pin is 1.97.1", result.stderr)
        self.assertIn("TOOLCHAIN MISMATCH: cargo is 1.98.1, pin is 1.97.1", result.stderr)
        self.assertIn('export PATH="$HOME/.cargo/bin:$PATH"', result.stderr)
        self.assertNotIn("preflight: OK", result.stdout)

    def test_cargo_alone_off_pin_still_fails(self):
        repo = self.make_repo("cargo-skew", "staging/pin", toolchain="1.97.1")
        result = self.run_preflight(repo, self.env_for(self.fake_tools("mixed", "1.97.1", "1.98.1")))
        self.assertEqual(result.returncode, 6, result.stderr)
        self.assertIn("cargo is 1.98.1", result.stderr)
        self.assertNotIn("rustc is", result.stderr)

    def test_rustc_env_is_what_cargo_uses_so_it_is_what_is_checked(self):
        repo = self.make_repo("rustc-env", "staging/pin", toolchain="1.97.1")
        brew = self.fake_tools("brew", "1.98.1", "1.97.1")
        pinned = self.fake_tools("pinned", "1.97.1", "1.97.1")
        ok = self.run_preflight(repo, self.env_for(brew, RUSTC=str(pinned / "rustc")))
        self.assertEqual(ok.returncode, 0, ok.stderr)
        self.assertIn("[$RUSTC]", ok.stdout)
        skew = self.run_preflight(repo, self.env_for(pinned, RUSTC=str(brew / "rustc")))
        self.assertEqual(skew.returncode, 6, skew.stderr)

    def test_missing_rustc_is_unverified_not_ok(self):
        repo = self.make_repo("no-rustc", "staging/pin", toolchain="1.97.1")
        result = self.run_preflight(repo, self.env_for(self.fake_tools("absent", None, "1.97.1")))
        self.assertEqual(result.returncode, 6, result.stdout + result.stderr)
        self.assertIn("rustc is UNRESOLVABLE", result.stderr)

    def test_override_downgrades_to_a_recorded_warning(self):
        repo = self.make_repo("override", "staging/pin", toolchain="1.97.1")
        for index, value in enumerate(("1", "true", "TRUE")):
            with self.subTest(value=value):
                result = self.run_preflight(repo, self.env_for(self.fake_tools(f"brew-{index}", "1.98.1", "1.98.1"),
                                                               OXIDEX_ALLOW_TOOLCHAIN_SKEW=value))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("TOOLCHAIN MISMATCH: rustc is 1.98.1", result.stderr)
                self.assertIn("MISMATCH OVERRIDDEN (OXIDEX_ALLOW_TOOLCHAIN_SKEW=1)", result.stdout)
        result = self.run_preflight(repo, self.env_for(self.fake_tools("brew-no", "1.98.1", "1.98.1"),
                                                       OXIDEX_ALLOW_TOOLCHAIN_SKEW="0"))
        self.assertEqual(result.returncode, 6, result.stderr)

    def test_dirty_tree_keeps_precedence_over_toolchain(self):
        repo = self.make_repo("dirty-skew", "staging/pin", toolchain="1.97.1")
        (repo / "untracked").write_text("dirty\n", encoding="utf-8")
        result = self.run_preflight(repo, self.env_for(self.fake_tools("brew", "1.98.1", "1.98.1")))
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("TOOLCHAIN MISMATCH", result.stderr)

    def test_minor_and_symbolic_channels(self):
        repo = self.make_repo("minor", "staging/pin", toolchain="1.97")
        ok = self.run_preflight(repo, self.env_for(self.fake_tools("patch", "1.97.3", "1.97.3")))
        self.assertEqual(ok.returncode, 0, ok.stderr)
        repo = self.make_repo("symbolic", "staging/pin", toolchain="stable")
        sym = self.run_preflight(repo, self.env_for(self.fake_tools("any", "1.98.1", "1.98.1")))
        self.assertEqual(sym.returncode, 0, sym.stderr)
        self.assertIn("channel 'stable' is symbolic", sym.stdout)

    def test_checkout_without_a_pin_skips_the_check(self):
        repo = self.make_repo("unpinned", "staging/pin")
        result = self.run_preflight(repo, self.env_for(self.fake_tools("brew", "1.98.1", "1.98.1")))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("toolchain: no rust-toolchain.toml (skipped)", result.stdout)


if __name__ == "__main__":
    unittest.main()
