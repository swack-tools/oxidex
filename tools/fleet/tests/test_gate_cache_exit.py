"""gate.sh's cache-hit branch: the exit status must equal the cached verdict.

THE DEFECT THIS PINS. The cache-hit branch ended `exit 0` unconditionally, so a
cached FAIL wrote `FAIL cache-hit tree=<sha>` into the verdict file and then
reported SUCCESS to its caller. The verdict file (which fleetd reads) and the
exit status (which a human, a supervisor, or a shell `&&` reads) disagreed, and
a re-gate of a known-bad tree passed silently. Fresh failures exit 1 via
`fail()`; only a cached PASS may exit 0.

WHY THESE TESTS DRIVE THE REAL SCRIPT TEXT. An earlier seam in this repo
(the verdict-store marker) was pinned by two tests that each spelled the
constant themselves, so renaming it on one side left both green. These tests
extract and execute gate.sh's OWN cache-hit block rather than restating its
logic, so a change to that block is a change to what is tested.

Run: cd tools/fleet/tests && python3 -m unittest test_gate_cache_exit
"""

from __future__ import annotations

import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

FLEET = Path(__file__).resolve().parents[1]
GATE_SH = FLEET / "gate.sh"
VERSION_TXT = FLEET / "gate_version.txt"


def _cache_block() -> str:
    """gate.sh's cache-hit branch, extracted verbatim.

    Anchored on the two lines that bracket it in the file. If the block moves
    or is renamed this raises, which is the correct failure: a test that
    silently matched nothing would be a test that pins nothing.
    """
    text = GATE_SH.read_text()
    start = text.index('  if [ -n "$CACHED_RESULT" ]; then')
    end = text.index("  fi\nfi", start)
    return text[start:end] + "  fi\n"


class TestCacheHitExitStatus(unittest.TestCase):
    """The four verdicts a cache lookup can produce, plus a malformed payload."""

    def _run(self, cached_json: str) -> tuple[int, str]:
        """Execute the real block with a stubbed environment; return (rc, verdict)."""
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            v, j, log = tmp / "v", tmp / "j", tmp / "log"
            (tmp / "ct").mkdir()
            (tmp / "d").mkdir()
            script = f"""
set -u
CACHED_JSON={json.dumps(cached_json)}
V={v!s}; J={j!s}; L={log!s}
TAG=t; TREE_SHA=deadbeef
CARGO_TARGET_DIR={tmp / "ct"!s}; D={tmp / "d"!s}
CACHED_RESULT=$(printf '%s' "$CACHED_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("result",""))' 2>/dev/null)
{_cache_block()}
exit 42  # sentinel: reached only when the block did NOT exit
"""
            p = subprocess.run(["bash", "-c", script], capture_output=True, text=True)
            return p.returncode, (v.read_text().strip() if v.exists() else "")

    def test_cached_pass_exits_zero(self):
        rc, verdict = self._run('{"result": "PASS"}')
        self.assertEqual(rc, 0, f"a cached PASS must exit 0; verdict file said {verdict!r}")
        self.assertTrue(verdict.startswith("PASS"), verdict)

    def test_cached_fail_exits_nonzero(self):
        """THE REGRESSION. Before the fix this returned 0 while writing FAIL."""
        rc, verdict = self._run('{"result": "FAIL"}')
        self.assertTrue(verdict.startswith("FAIL"), f"verdict file: {verdict!r}")
        self.assertNotEqual(
            rc, 0,
            "a cached FAIL exited 0 -- the verdict file says FAIL while the exit "
            "status says success, so a re-gate of a known-bad tree passes silently",
        )

    def test_cached_abort_exits_nonzero(self):
        """verdict.py's lookup() is documented never to serve an ABORT, so this
        should be unreachable in production. It is pinned anyway: 'unreachable'
        is a property of today's lookup(), not of this block, and the safe
        direction for an unexpected verdict is non-zero."""
        rc, verdict = self._run('{"result": "ABORT"}')
        self.assertTrue(verdict.startswith("ABORT"), verdict)
        self.assertNotEqual(rc, 0, "an unexpected cached verdict must not report success")

    def test_malformed_payload_falls_through_to_a_real_run(self):
        """A payload with no parseable result must NOT be treated as a verdict.
        It has to fall through to the real gate (sentinel 42), because a cache
        entry we cannot read is not evidence of anything."""
        for payload in ('{"result": ""}', "not json at all", "{}"):
            with self.subTest(payload=payload):
                rc, _ = self._run(payload)
                self.assertEqual(
                    rc, 42,
                    f"payload {payload!r} short-circuited the gate instead of "
                    "falling through to a real run",
                )


class TestGateVersionIsSpelledOnceEffectively(unittest.TestCase):
    """gate.sh and gate_version.txt must agree.

    Two spellings of one constant, with nothing pinning them together, is how
    the verdict-store marker defect happened: renaming one side left every test
    green while the two halves stopped meeting. Python consumers (verdict.py,
    claim.py, fleetd.py) read the .txt; the gate itself reads its own literal.
    """

    def _sh_version(self) -> str:
        m = re.search(r'^GATE_VERSION="([^"]+)"', GATE_SH.read_text(), re.M)
        self.assertIsNotNone(m, "GATE_VERSION assignment not found in gate.sh")
        return m.group(1)

    def test_shell_and_txt_agree(self):
        self.assertEqual(
            self._sh_version(), VERSION_TXT.read_text().strip(),
            "gate.sh's GATE_VERSION and tools/fleet/gate_version.txt disagree -- "
            "the gate would write verdicts under one key and the Python "
            "consumers would look them up under another",
        )

    def test_version_is_not_one_reused_by_the_other_line(self):
        """This file exists on two branches that share one verdict cache. The
        Keel line consumed 5-8 for unrelated gate semantics, so this line must
        never mint those numbers: a collision serves a verdict produced under
        different rules."""
        self.assertNotIn(
            self._sh_version(), {"5", "6", "7", "8"},
            "GATE_VERSION collides with a value the staging/agent-server line "
            "already used for different gate semantics",
        )


if __name__ == "__main__":
    unittest.main()
