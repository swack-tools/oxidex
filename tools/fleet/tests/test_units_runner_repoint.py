#!/usr/bin/env python3
"""Tests for PLAN Stage 3 task 7's unit re-point: every `units/*`
supervisor template must launch `keel/runner.py` (SPEC SS2 C7), not
`fleetd.py`, and must log under `~/.keel/log/`, never `/tmp` (macOS
purges it) or the old `~/gatelogs/fleetd-wrapper*` location.

Text-level assertions against the real template files, the same
technique `test_units_secrets.py` uses for its own B5 fence -- these
files are installed by a human onto a real host, so there is no
subprocess to run; the content itself is the contract.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import re
import subprocess
import unittest
from pathlib import Path

from _env import HermeticCase  # noqa: E402

UNITS_DIR = Path(__file__).resolve().parents[1] / "units"


class TestEntryPointRepointed(HermeticCase):
    def test_systemd_unit_execs_runner_py_not_fleetd_py(self):
        text = (UNITS_DIR / "fleetd.service").read_text()
        self.assertIn("keel/runner.py", text)
        self.assertNotIn("tools/fleet/fleetd.py", text)

    def test_launchd_plist_execs_runner_py_not_fleetd_py(self):
        text = (UNITS_DIR / "com.oxidex.fleetd.plist").read_text()
        self.assertIn("keel/runner.py", text)
        self.assertNotIn("tools/fleet/fleetd.py", text)

    def test_fleetd_wrapper_launches_runner_py_not_fleetd_py(self):
        text = (UNITS_DIR / "fleetd-wrapper.sh").read_text()
        self.assertIn("keel/runner.py", text)
        # The invocation line itself, not merely a comment mentioning the
        # new path: RUNNER_SCRIPT must actually be what gets exec'd.
        self.assertRegex(text, r'"\$PYTHON"\s+"\$RUNNER_SCRIPT"')
        self.assertNotRegex(text, r'"\$PYTHON"\s+"\$FLEET_DIR/fleetd\.py"')


class TestLogsUnderDotKeelLog(HermeticCase):
    def test_systemd_unit_logs_under_dot_keel_log(self):
        text = (UNITS_DIR / "fleetd.service").read_text()
        self.assertIn("StandardOutput=append:%h/.keel/log/runner.log", text)
        self.assertIn("StandardError=append:%h/.keel/log/runner.log", text)

    def test_launchd_plist_logs_under_dot_keel_log(self):
        text = (UNITS_DIR / "com.oxidex.fleetd.plist").read_text()
        self.assertIn("<string>/Users/allen/.keel/log/runner.log</string>", text)

    def test_launchd_plist_no_longer_logs_to_tmp(self):
        """The specific regression this pass fixes (SPEC SS2 C6's own
        row: "the plist currently logs to /tmp/fleetd.log") -- macOS
        purges /tmp, taking every crash's last words with it. Checked
        against the actual StandardOutPath/StandardErrorPath VALUES, not
        the whole file text -- the file's own history comment names the
        old path deliberately, exactly like gate.sh's header keeps
        `work2.oxidex.net` as history (test_no_hardcoded_hosts.py's own
        documented exemption for comments)."""
        text = (UNITS_DIR / "com.oxidex.fleetd.plist").read_text()
        paths = re.findall(r"<key>Standard(?:Out|Error)Path</key><string>([^<]*)</string>", text)
        self.assertTrue(paths, "expected at least one StandardOutPath/StandardErrorPath in the plist")
        for value in paths:
            self.assertNotEqual(value, "/tmp/fleetd.log")

    def test_fleetd_wrapper_default_log_is_under_dot_keel_log(self):
        text = (UNITS_DIR / "fleetd-wrapper.sh").read_text()
        self.assertNotIn("gatelogs/fleetd-wrapper.log", text)

    def test_cron_backstop_logs_under_dot_keel_log(self):
        text = (UNITS_DIR / "cron-backstop.txt").read_text()
        self.assertIn(".keel/log/runner-wrapper-cron.log", text)
        self.assertNotIn("gatelogs/fleetd-wrapper-cron.log", text)

    def test_cron_backstop_creates_the_log_directory(self):
        """`~/.keel/log` may not exist yet on a host whose first-ever
        supervisor start is via this cron line (unlike `~/gatelogs`,
        which install_secrets.sh/gate.sh runs typically create earlier)
        -- the line must `mkdir -p` it before redirecting output there,
        or the very first backstop-triggered start silently loses its
        log to a shell redirection error."""
        text = (UNITS_DIR / "cron-backstop.txt").read_text()
        cron_line = next(
            line for line in text.splitlines() if line.strip().startswith("*/5")
        )
        self.assertIn("mkdir -p", cron_line)
        self.assertIn(".keel/log", cron_line)


class TestFleetEnvShIsTheOneSharedDefault(HermeticCase):
    """`config.py`'s own governing rule, applied to this pass's new
    shared literal: `FLEETD_WRAPPER_LOG`'s default must live in exactly
    ONE place (`fleet-env.sh`), sourced by both consumers -- never
    hand-kept twice, which is the exact class of drift
    `test_verdict_marker_seam.py` exists to catch for a different
    literal.
    """

    def test_fleet_env_sh_sets_the_default(self):
        text = (UNITS_DIR / "fleet-env.sh").read_text()
        self.assertIn(
            ': "${FLEETD_WRAPPER_LOG:=$HOME/.keel/log/runner-wrapper.log}"', text
        )
        self.assertIn("export FLEETD_WRAPPER_LOG", text)

    def test_fleetd_wrapper_sources_fleet_env_sh(self):
        text = (UNITS_DIR / "fleetd-wrapper.sh").read_text()
        self.assertIn('. "$SELF_DIR/fleet-env.sh"', text)
        # And does NOT re-spell the literal default itself.
        self.assertNotIn(':-$HOME/.keel/log/runner-wrapper.log}', text)

    def test_restart_fleetd_sources_fleet_env_sh(self):
        text = (UNITS_DIR / "restart-fleetd.sh").read_text()
        self.assertIn('. "$SELF_DIR/fleet-env.sh"', text)
        self.assertNotIn(':-$HOME/.keel/log/runner-wrapper.log}', text)

    def test_fleetd_wrapper_and_restart_fleetd_agree_on_the_default_by_execution(self):
        """The seam itself, not just "both source the same file": run
        each script's own `LOG=` resolution (with FLEETD_WRAPPER_LOG
        unset) in a real subshell and compare the resulting value,
        exactly like `test_verdict_marker_seam.
        TestGateShAndFleetdAgreeOnTheMarkerPath` cross-checks its own
        literal by evaluating shell text rather than by inspection."""

        def resolved_log(script_name: str) -> str:
            path = UNITS_DIR / script_name
            source = path.read_text()
            # Extract just the two lines this test cares about: the
            # SELF_DIR assignment and the source-plus-LOG lines, run in a
            # clean subshell so PIDFILE/other unrelated lines never run.
            self_dir_line = next(
                line for line in source.splitlines() if line.strip().startswith("SELF_DIR=")
            )
            script = (
                f"{self_dir_line}\n"
                f'. "$SELF_DIR/fleet-env.sh"\n'
                'LOG="$FLEETD_WRAPPER_LOG"\n'
                'printf "%s" "$LOG"\n'
            )
            result = subprocess.run(
                ["bash", "-c", script], cwd=str(UNITS_DIR), capture_output=True, text=True,
                env={"HOME": "/fake/home", "PATH": "/usr/bin:/bin"}, timeout=10,
            )
            self.assertEqual(result.returncode, 0, msg=result.stderr)
            return result.stdout

        wrapper_log = resolved_log("fleetd-wrapper.sh")
        restart_log = resolved_log("restart-fleetd.sh")
        self.assertEqual(wrapper_log, restart_log)
        self.assertEqual(wrapper_log, "/fake/home/.keel/log/runner-wrapper.log")


if __name__ == "__main__":
    unittest.main()
