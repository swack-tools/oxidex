#!/usr/bin/env python3
"""Tests for B5 (review finding): every unit/plist/cron template that
launches fleetd against the private (HTTPS) state repo must also set
`FLEET_GIT_TOKEN_FILE`, or fleetd's very first git op against that repo
raises an uncaught `HubUnreachableError` before the singleton claim, let
alone a heartbeat.

Each host-supervisor mechanism spells the SAME path differently, and this
suite pins each spelling separately rather than one shared regex:
  - systemd (`fleetd.service`): `%h` is systemd's own specifier for the
    unit's HOME, expanded by systemd itself before exec -- never a shell.
  - launchd (`com.oxidex.fleetd.plist`): `EnvironmentVariables` values are
    literal XML strings. launchd does not expand `$HOME` or any other
    shell variable in them (confirmed by the file's own preexisting
    `ProgramArguments`, which already hardcodes `/Users/allen/...` for the
    exact same reason) -- so the correct value here is a real absolute
    path, not `$HOME`.
  - cron (`cron-backstop.txt`): the whole line is handed to `sh -c` with
    `HOME` already in cron's own environment (crontab(5)), so `$HOME` in
    the command text is a real, working shell expansion -- exactly like
    the pidfile/log paths already on that same line.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import unittest
from pathlib import Path
from _env import HermeticCase  # noqa: E402

UNITS_DIR = Path(__file__).resolve().parents[1] / "units"


class TestUnitsSetGitTokenFile(HermeticCase):
    def test_systemd_unit_sets_token_file_via_specifier(self):
        text = (UNITS_DIR / "fleetd.service").read_text()
        self.assertIn(
            "Environment=FLEET_GIT_TOKEN_FILE=%h/.keel/secrets/git-token",
            text,
            "fleetd.service must set FLEET_GIT_TOKEN_FILE using systemd's "
            "%h specifier (expanded by systemd, not a shell)",
        )

    def test_systemd_unit_sets_code_url(self):
        """Not new (Stage 1 task 4), but a precondition B5's fix assumes:
        the token is useless if the remotes it authenticates aren't both
        configured."""
        text = (UNITS_DIR / "fleetd.service").read_text()
        self.assertIn("Environment=FLEET_HUB_URL=", text)
        self.assertIn("Environment=FLEET_CODE_URL=", text)

    def test_launchd_plist_sets_token_file_as_a_literal_path(self):
        text = (UNITS_DIR / "com.oxidex.fleetd.plist").read_text()
        self.assertIn("<key>FLEET_GIT_TOKEN_FILE</key>", text)
        self.assertIn("<string>/Users/allen/.keel/secrets/git-token</string>", text)
        # The negative half of the class docstring's claim: launchd would
        # NOT expand this, so it must never be spelled with a $HOME/%h
        # that only looks like it will work.
        self.assertNotIn("$HOME/.keel/secrets/git-token", text)
        self.assertNotIn("%h/.keel/secrets/git-token", text)

    def test_launchd_plist_sets_code_url(self):
        text = (UNITS_DIR / "com.oxidex.fleetd.plist").read_text()
        self.assertIn("<key>FLEET_HUB_URL</key>", text)
        self.assertIn("<key>FLEET_CODE_URL</key>", text)

    def test_cron_backstop_sets_token_file_via_shell_expansion(self):
        text = (UNITS_DIR / "cron-backstop.txt").read_text()
        self.assertIn("FLEET_GIT_TOKEN_FILE=$HOME/.keel/secrets/git-token", text)

    def test_cron_backstop_sets_code_url(self):
        text = (UNITS_DIR / "cron-backstop.txt").read_text()
        self.assertIn("FLEET_HUB_URL=", text)
        self.assertIn("FLEET_CODE_URL=", text)

    def test_all_three_templates_exist(self):
        """Guards the test itself against a rename silently turning every
        assertion above into a FileNotFoundError instead of a clear
        failure -- same class of trap as test_no_hardcoded_hosts.py's own
        `test_candidate_files_exist`."""
        for name in ("fleetd.service", "com.oxidex.fleetd.plist", "cron-backstop.txt"):
            path = UNITS_DIR / name
            self.assertTrue(path.is_file(), f"expected unit template missing: {path}")


if __name__ == "__main__":
    unittest.main()
