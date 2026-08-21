#!/usr/bin/env python3
"""Tests for `tools/fleet/rollout/install_secrets.sh` (B5, review finding).

Hermetic: nothing here talks to the real GitHub state repo. The one branch
that needs a network path (the `git ls-remote` probe) is pointed at a host
under the `.invalid` TLD, which RFC 2606 reserves to NEVER resolve -- so
`git`'s failure is fast and deterministic whether or not this sandbox has
network access at all, without needing a real credential or a live
opt-in flag the way `tests/live/test_tip_ruleset.py` needs
`FLEET_LIVE_GITHUB=1` for an actual push.

Everything else (argument validation, the secrets-dir mkdir/chmod, the
token-file mode/empty checks) needs no network whatsoever.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "rollout" / "install_secrets.sh"
UNRESOLVABLE_URL = "https://state-repo.example.invalid/oxidex-fleet-state.git"
CANARY_TOKEN = "ghp_CANARY_MUST_NEVER_APPEAR_IN_OUTPUT_0000000000"


class InstallSecretsTestCase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.mkdtemp(prefix="install-secrets-")
        self.addCleanup(self._rmtree)
        self.home = Path(self._tmp) / "home"
        self.home.mkdir()
        self.token_file = self.home / ".keel" / "secrets" / "git-token"

    def _rmtree(self):
        import shutil

        shutil.rmtree(self._tmp, ignore_errors=True)

    def _run(self, args, env_extra=None, home=None):
        env = {**os.environ, "HOME": str(home or self.home)}
        if env_extra:
            env.update(env_extra)
        return subprocess.run(
            [str(SCRIPT), *args],
            capture_output=True,
            text=True,
            env=env,
        )

    def _write_token(self, content=CANARY_TOKEN + "\n", mode=0o600, path=None):
        path = path or self.token_file
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        os.chmod(path, mode)
        return path

    # -- usage / argument validation --------------------------------

    def test_the_script_ships_and_is_executable(self):
        self.assertTrue(SCRIPT.is_file(), msg=f"{SCRIPT} is missing")
        self.assertTrue(os.access(SCRIPT, os.X_OK), msg=f"{SCRIPT} is not executable")

    def test_no_state_url_is_a_usage_error(self):
        r = self._run([])
        self.assertEqual(r.returncode, 1)
        self.assertIn("FLEET_HUB_URL", r.stderr)

    def test_non_https_state_url_is_refused(self):
        r = self._run(["--state-url", "ssh://git@github.com/x/y.git"])
        self.assertEqual(r.returncode, 1)
        self.assertIn("https://", r.stderr)

    def test_flag_and_env_var_are_equivalent(self):
        """--state-url and FLEET_HUB_URL must reach the same validation
        path (matches seed_desired.py's own `--hub`/`FLEET_HUB_URL`
        precedent)."""
        r_flag = self._run(["--state-url", "not-a-url"])
        r_env = self._run([], env_extra={"FLEET_HUB_URL": "not-a-url"})
        self.assertEqual(r_flag.returncode, r_env.returncode)
        self.assertEqual(r_flag.returncode, 1)

    def test_unknown_argument_is_a_usage_error(self):
        r = self._run(["--bogus-flag"])
        self.assertEqual(r.returncode, 1)

    # -- secrets directory: create + fix mode ------------------------

    def test_creates_the_secrets_directory_at_0700(self):
        self.assertFalse(self.token_file.parent.exists())
        r = self._run(["--state-url", UNRESOLVABLE_URL])
        self.assertTrue(self.token_file.parent.is_dir())
        mode = stat.S_IMODE(self.token_file.parent.stat().st_mode)
        self.assertEqual(mode, 0o700)
        # No token yet -> still fails overall, just not on the directory.
        self.assertEqual(r.returncode, 3)

    def test_fixes_an_existing_directory_with_looser_permissions(self):
        self.token_file.parent.mkdir(parents=True)
        os.chmod(self.token_file.parent, 0o755)
        self._run(["--state-url", UNRESOLVABLE_URL])
        mode = stat.S_IMODE(self.token_file.parent.stat().st_mode)
        self.assertEqual(mode, 0o700)

    # -- token file: existence / mode / emptiness --------------------

    def test_missing_token_file_exits_3_with_instructions(self):
        r = self._run(["--state-url", UNRESOLVABLE_URL])
        self.assertEqual(r.returncode, 3)
        self.assertIn(str(self.token_file), r.stderr)
        self.assertIn("chmod 0600", r.stderr)
        self.assertNotIn(
            "ghp_", r.stderr, "must never invent or suggest an actual-looking token"
        )

    def test_wrong_mode_exits_4(self):
        self._write_token(mode=0o644)
        r = self._run(["--state-url", UNRESOLVABLE_URL])
        self.assertEqual(r.returncode, 4)
        self.assertIn("644", r.stderr)
        self.assertIn("600", r.stderr)

    def test_empty_file_exits_5(self):
        self._write_token(content="", mode=0o600)
        r = self._run(["--state-url", UNRESOLVABLE_URL])
        self.assertEqual(r.returncode, 5)
        self.assertIn("empty", r.stderr)

    def test_token_file_flag_overrides_the_default_path(self):
        alt = Path(self._tmp) / "elsewhere" / "token"
        self._write_token(mode=0o600, path=alt)
        r = self._run(["--state-url", UNRESOLVABLE_URL, "--token-file", str(alt)])
        # Reaches the network probe (rc 6, unresolvable host) rather than
        # failing on file-existence (rc 3) -- proves the flag path was
        # actually used, not the default.
        self.assertEqual(r.returncode, 6)

    # -- the probe: unreachable host is a clean rc 6, never a hang -----

    def test_unresolvable_host_exits_6_and_shows_gits_own_stderr(self):
        self._write_token(mode=0o600)
        r = self._run(["--state-url", UNRESOLVABLE_URL])
        self.assertEqual(r.returncode, 6)
        self.assertIn(UNRESOLVABLE_URL, r.stderr)
        self.assertIn("fatal", r.stderr.lower())

    def test_the_probe_completes_quickly(self):
        """`.invalid` never resolves (RFC 2606) -- the probe must fail
        fast, not hang waiting on a connection that will never complete."""
        import time

        self._write_token(mode=0o600)
        start = time.monotonic()
        self._run(["--state-url", UNRESOLVABLE_URL])
        elapsed = time.monotonic() - start
        self.assertLess(elapsed, 30, "probe took too long against a guaranteed-unresolvable host")

    # -- the token never appears anywhere in the script's own output --

    def test_the_token_never_appears_in_stdout_or_stderr(self):
        self._write_token(content=CANARY_TOKEN + "\n", mode=0o600)
        r = self._run(["--state-url", UNRESOLVABLE_URL])
        self.assertNotIn(CANARY_TOKEN, r.stdout)
        self.assertNotIn(CANARY_TOKEN, r.stderr)

    def test_the_token_never_appears_even_on_the_missing_file_message(self):
        r = self._run(["--state-url", UNRESOLVABLE_URL])
        self.assertNotIn(CANARY_TOKEN, r.stdout)
        self.assertNotIn(CANARY_TOKEN, r.stderr)


if __name__ == "__main__":
    unittest.main()
