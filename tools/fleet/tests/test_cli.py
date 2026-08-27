#!/usr/bin/env python3
"""cli.py tests: R1 (review of staging/agent-server @ 99f06cb3;
docs/AGENT-SERVER-SPEC.md Sec4.4(c); docs/AGENT-SERVER-PLAN.md Stage 1
task 5). `fleet status`'s QUEUE line asks `workqueue.Queue` for the live
queue, and `Queue.compute()` reads the tip via `hub.code_sha(TIP_REF)` --
a CODE-repo question. `cli._hub()` built its `Hub` with no way to name that
repo at all, so on a split spine `code_url` silently defaulted to `.url`
(the STATE repo, which carries no `refs/heads/*`), and every `fleet
status` -- `--why` included -- printed a permanent
`error: tip ref ... does not exist on the code repo '<state url>'` on the
QUEUE line. SPEC Sec4.4(c) listed cli.py as "coordination-only"; this file
is what proves that was wrong for exactly one line of it.

The fix is `--code`/`FLEET_CODE_URL`, mirroring `fleetd.py`'s own pair
(same names, same precedence, same "unset means same repo as --hub"
default) so a single-repo fleet's existing invocations are unaffected --
see `TestStatusCodeUrlPlumbing.test_missing_code_url_reports_the_queue_error`
for that unaffected (still-broken-if-unconfigured) case, kept on purpose
as the fixture that proves the other tests are exercising something real.

Reuses `test_code_url_split._RepoPair`: a real STATE repo (no
`refs/heads/*` at all) and a real CODE repo (a real tip commit plus two
staging branches with real commit history, one already merged onto the
tip, one genuinely queued) -- the same fixture the rest of this suite
already uses to pin the code/state routing table, so this file adds no
second notion of what "two real bare repos" means.

Run with:
    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_cli -v
"""

from __future__ import annotations

import contextlib
import io
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import cli  # noqa: E402

from test_code_url_split import _RepoPair  # noqa: E402
from _env import HermeticCase  # noqa: E402

_QUEUE_LINE_RE = re.compile(r"^QUEUE\s+(.*?)\s{2,}CLAIMS\s+(\S+)\s{2,}DESIRED gen (\S+)\s*$")


class _CliCase(HermeticCase):
    def setUp(self):
        super().setUp()
        self._tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmpdir.name)
        self.repos = _RepoPair(self.tmp)
        # cli._hub() caches git objects under Path.home()/".fleetd"/"clicache" --
        # HOME is redirected into the fixture for every `cli.main()` call
        # below (via `_status()`) so nothing here ever touches the real
        # machine's cache, matching test_bringup_split.py's own rule.
        self._home_patch = mock.patch.dict(os.environ, {"HOME": str(self.tmp)})
        self._home_patch.start()
        self.addCleanup(self._home_patch.stop)

    def tearDown(self):
        self._tmpdir.cleanup()

    def _status(self, argv: list) -> str:
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = cli.main(argv)
        self.assertEqual(rc, 0, buf.getvalue())
        return buf.getvalue()

    def _queue_line(self, out: str) -> re.Match:
        for line in out.splitlines():
            m = _QUEUE_LINE_RE.match(line)
            if m:
                return m
        raise AssertionError(f"no QUEUE line found in output:\n{out}")


class TestStatusCodeUrlPlumbing(_CliCase):
    def test_code_flag_resolves_the_real_queue(self):
        out = self._status([
            "--hub", str(self.repos.state), "--code", str(self.repos.code), "status",
        ])
        m = self._queue_line(out)
        # staging/alpha is real drift off the tip and belongs in the
        # queue; staging/merged is an ancestor of the tip and is filtered
        # out -- see _RepoPair's own docstring for why both exist.
        self.assertEqual(m.group(1), "1", out)

    def test_code_env_var_resolves_the_real_queue(self):
        """FLEET_CODE_URL, not just --code, must reach `_hub()` -- the
        same fallback order `--hub`/FLEET_HUB_URL already has."""
        with mock.patch.dict(os.environ, {"FLEET_CODE_URL": str(self.repos.code)}):
            out = self._status(["--hub", str(self.repos.state), "status"])
        m = self._queue_line(out)
        self.assertEqual(m.group(1), "1", out)

    def test_code_flag_takes_precedence_over_env_var(self):
        """An empty code repo (no tip at all) named by FLEET_CODE_URL must
        lose to a --code flag naming the real one -- proves precedence is
        wired, not merely that one route or the other happens to work."""
        empty_code = self.tmp / "empty-code.git"
        subprocess.run(["git", "init", "-q", "--bare", str(empty_code)], check=True)
        with mock.patch.dict(os.environ, {"FLEET_CODE_URL": str(empty_code)}):
            out = self._status([
                "--hub", str(self.repos.state), "--code", str(self.repos.code), "status",
            ])
        m = self._queue_line(out)
        self.assertEqual(m.group(1), "1", out)

    def test_missing_code_url_reports_the_queue_error_instead_of_crashing(self):
        """Before R1 this was the UNCONDITIONAL behaviour of every `fleet
        status` against a split spine -- there was no flag or env var that
        could have changed it. After R1 it is what happens only when an
        operator truly omits both --code and FLEET_CODE_URL:
        `cmd_status`'s pre-existing broad `except Exception` (unchanged by
        this fix) still keeps the table rendering instead of crashing, but
        the error names the STATE repo it wrongly fell back to -- not a
        placeholder -- which is what proves `code_url` really did default
        to `.url` here rather than to something else."""
        out = self._status(["--hub", str(self.repos.state), "status"])
        m = self._queue_line(out)
        self.assertIn("error:", m.group(1), m.group(1))
        self.assertIn(str(self.repos.state), m.group(1), m.group(1))

    def test_why_flag_also_gets_a_real_queue_count(self):
        """--why adds a second section below the QUEUE line; it must not
        change how the QUEUE line itself is computed."""
        out = self._status([
            "--hub", str(self.repos.state), "--code", str(self.repos.code),
            "status", "--why",
        ])
        m = self._queue_line(out)
        self.assertEqual(m.group(1), "1", out)
        self.assertIn("WHY (last reconcile's refused[]", out)


if __name__ == "__main__":
    unittest.main()
