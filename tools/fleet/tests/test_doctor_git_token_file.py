#!/usr/bin/env python3
"""Tests for `doctor.check_git_token_file` (B5, review finding).

`FLEET_HUB_URL` pointed at a private GitHub repo over HTTPS needs a
credential or fleetd's first git op -- the singleton claim, before any
heartbeat -- raises an uncaught `HubUnreachableError` on a tokenless host.
This check is doctor.py's fitness gate for that precondition; it never
opens the token file (existence + mode only), matching the docstring's own
claim that a health check must not risk putting a secret in its own
output.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import doctor  # noqa: E402
from _env import HermeticCase  # noqa: E402


class TestCheckGitTokenFile(HermeticCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.mkdtemp(prefix="doctor-token-")
        self.addCleanup(self._rmtree)
        # R2/R7: check_git_token_file() now falls back to
        # config.default_git_token_file(), which reads $HOME -- redirect
        # it into this test's own tempdir so every test here is
        # deterministic regardless of whether the REAL machine running
        # the suite happens to have ~/.keel/secrets/git-token (same
        # hermeticity concern R7 raises for HOME-dependent defaults
        # elsewhere in this tree). Tests that want the default path to
        # exist create it explicitly under self._home.
        self._home = Path(self._tmp) / "home"
        self._home.mkdir()
        self._home_patch = mock.patch.dict(os.environ, {"HOME": str(self._home)})
        self._home_patch.start()
        self.addCleanup(self._home_patch.stop)

    def _rmtree(self):
        import shutil

        shutil.rmtree(self._tmp, ignore_errors=True)

    def _token_file(self, mode=0o600, content="ghp_x\n"):
        path = Path(self._tmp) / "git-token"
        path.write_text(content)
        os.chmod(path, mode)
        return str(path)

    def _default_token_file(self, mode=0o600, content="ghp_default\n"):
        """A token file at the REDIRECTED default path
        (`$HOME/.keel/secrets/git-token`), for the R2 fallback tests."""
        path = self._home / ".keel" / "secrets" / "git-token"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        os.chmod(path, mode)
        return path

    def test_non_https_hub_url_is_informational_not_a_failure(self):
        """A local `git init --bare` fixture or an ssh hub needs no token
        file -- must not fail doctor.py on hosts still on the old ssh hub
        or in a test fixture."""
        with mock.patch.dict(os.environ, {"FLEET_HUB_URL": "ssh://git@example/hub.git"}, clear=False):
            os.environ.pop("FLEET_GIT_TOKEN_FILE", None)
            c = doctor.check_git_token_file()
        self.assertIsNone(c.ok, "non-https hub_url must be informational (ok=None), not PASS/FAIL")

    def test_missing_hub_url_is_informational_not_a_failure(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("FLEET_HUB_URL", None)
            os.environ.pop("FLEET_GIT_TOKEN_FILE", None)
            c = doctor.check_git_token_file()
        self.assertIsNone(c.ok)

    def test_https_hub_url_with_no_token_file_var_fails_loud(self):
        """No env var AND no file at the (redirected) default path --
        still a loud failure, not a silent skip."""
        with mock.patch.dict(
            os.environ,
            {"FLEET_HUB_URL": "https://github.com/swack-tools/oxidex-fleet-state.git"},
            clear=False,
        ):
            os.environ.pop("FLEET_GIT_TOKEN_FILE", None)
            c = doctor.check_git_token_file()
        self.assertFalse(c.ok)
        self.assertIn("FLEET_GIT_TOKEN_FILE is unset", c.detail)
        self.assertIn("install_secrets.sh", c.detail, "must name the fix, not just the symptom")

    def test_default_token_file_accepted_when_env_var_unset(self):
        """R2: a correctly-permissioned token file sitting at the default
        path (`~/.keel/secrets/git-token`, same default
        `install_secrets.sh` and every `units/*` template use) must PASS
        even though `FLEET_GIT_TOKEN_FILE` was never exported -- that is
        exactly the hand-run-step gap R2 names."""
        default_path = self._default_token_file(mode=0o600)
        with mock.patch.dict(
            os.environ,
            {"FLEET_HUB_URL": "https://github.com/swack-tools/oxidex-fleet-state.git"},
            clear=False,
        ):
            os.environ.pop("FLEET_GIT_TOKEN_FILE", None)
            c = doctor.check_git_token_file()
        self.assertTrue(c.ok, msg=c.detail)
        self.assertIn(str(default_path), c.detail)
        self.assertIn("default", c.detail, "must say it fell back to the default, not pretend the var was set")

    def test_default_token_file_with_wrong_mode_still_fails_loud(self):
        """The R2 fallback does not relax the 0600 requirement -- a
        loosely-permissioned file at the default path is still a FAIL,
        not a silent PASS."""
        default_path = self._default_token_file(mode=0o644)
        with mock.patch.dict(
            os.environ,
            {"FLEET_HUB_URL": "https://github.com/swack-tools/oxidex-fleet-state.git"},
            clear=False,
        ):
            os.environ.pop("FLEET_GIT_TOKEN_FILE", None)
            c = doctor.check_git_token_file()
        self.assertFalse(c.ok)
        self.assertIn("0o644", c.detail)
        self.assertIn("0600", c.detail)
        self.assertIn(str(default_path), c.detail)

    def test_explicit_env_var_wins_over_default_path(self):
        """An explicitly-set `FLEET_GIT_TOKEN_FILE` is used as-is and the
        default path is never even consulted -- the R2 fallback only
        fires when the var is unset, never as a second candidate on top
        of an explicit (and in this case deliberately wrong) one."""
        self._default_token_file(mode=0o600)  # a valid default, ignored
        explicit = self._token_file(mode=0o600)
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_HUB_URL": "https://github.com/swack-tools/oxidex-fleet-state.git",
                "FLEET_GIT_TOKEN_FILE": explicit,
            },
        ):
            c = doctor.check_git_token_file()
        self.assertTrue(c.ok, msg=c.detail)
        self.assertIn(explicit, c.detail)
        self.assertNotIn("default", c.detail.lower())

    def test_https_hub_url_with_missing_file_fails_loud(self):
        missing = str(Path(self._tmp) / "does-not-exist")
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_HUB_URL": "https://github.com/swack-tools/oxidex-fleet-state.git",
                "FLEET_GIT_TOKEN_FILE": missing,
            },
        ):
            c = doctor.check_git_token_file()
        self.assertFalse(c.ok)
        self.assertIn(missing, c.detail)
        self.assertIn("install_secrets.sh", c.detail)

    def test_wrong_mode_fails_loud(self):
        token_file = self._token_file(mode=0o644)
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_HUB_URL": "https://github.com/swack-tools/oxidex-fleet-state.git",
                "FLEET_GIT_TOKEN_FILE": token_file,
            },
        ):
            c = doctor.check_git_token_file()
        self.assertFalse(c.ok)
        self.assertIn("0o644", c.detail)
        self.assertIn("0600", c.detail)
        self.assertIn("install_secrets.sh", c.detail)

    def test_correct_mode_passes(self):
        token_file = self._token_file(mode=0o600)
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_HUB_URL": "https://github.com/swack-tools/oxidex-fleet-state.git",
                "FLEET_GIT_TOKEN_FILE": token_file,
            },
        ):
            c = doctor.check_git_token_file()
        self.assertTrue(c.ok, msg=c.detail)

    def test_the_token_contents_are_never_read_into_the_report(self):
        """A distinctive, obviously-fake secret must never appear in the
        check's own output -- the whole reason this check stats rather
        than opens the file."""
        secret = "ghp_UNMISTAKABLE_CANARY_VALUE_0000000000"
        token_file = self._token_file(mode=0o600, content=secret + "\n")
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_HUB_URL": "https://github.com/swack-tools/oxidex-fleet-state.git",
                "FLEET_GIT_TOKEN_FILE": token_file,
            },
        ):
            c = doctor.check_git_token_file()
        self.assertNotIn(secret, c.detail)
        self.assertNotIn(secret, c.line())

    def test_check_is_included_in_main_checks(self):
        """A check that exists but is never run is not a check -- guard
        against the function being added but forgotten in `main()`'s
        list."""
        source = (FLEET_DIR / "doctor.py").read_text()
        self.assertIn("check_git_token_file()", source)
        # Specifically inside the checks list, not just defined once as
        # the `def` line itself.
        self.assertGreaterEqual(source.count("check_git_token_file()"), 2)


if __name__ == "__main__":
    unittest.main()
