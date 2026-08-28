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


class TestFullScriptCacheChain(unittest.TestCase):
    """The whole chain, not just the branch: real gate.sh, real verdict lookup.

    WHY THIS EXISTS. `TestCacheHitExitStatus` above extracts the INNER cache
    branch and recreates `CACHED_RESULT` itself, so its malformed-payload cases
    prove that *the test's* re-spelling of the parse falls through — not that
    gate.sh's own `lookup -> CACHE_STATUS guard -> -n guard -> python parse`
    chain does. Those are seam tests; this is the integration test that closes
    the gap. (The irony is recorded rather than hidden: the module docstring
    above preaches against restating logic, and the first version of these
    tests restated the parse line.)

    Method: run the UNMODIFIED gate.sh with `verdict.py` shadowed by a stub on
    a copied tools/fleet, against a fixture bare repo. Assert on the log, which
    is the script's own account of where it got to.
    """

    def _run_gate(self, stub_lookup_stdout: str, stub_rc: int = 0):
        """Returns (returncode, log_text, verdict_text)."""
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            home, hub, work = tmp / "home", tmp / "hub.git", tmp / "work"
            (home / "gatelogs").mkdir(parents=True)
            env = {
                **__import__("os").environ,
                "HOME": str(home),
                "FLEET_HUB_URL": str(hub),
                "FLEET_CODE_URL": str(hub),
            }

            def git(*a, cwd=None):
                subprocess.run(["git", *a], cwd=cwd, check=True,
                               capture_output=True, env={**env,
                               "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
                               "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"})

            git("init", "-q", "--bare", str(hub))
            work.mkdir()
            git("init", "-q", str(work))
            (work / "f.txt").write_text("base\n")
            git("add", ".", cwd=work)
            git("commit", "-qm", "tip", cwd=work)
            git("push", "-q", str(hub), "HEAD:refs/heads/refactor/tag-machinery", cwd=work)
            git("checkout", "-q", "-b", "b", cwd=work)
            (work / "g.txt").write_text("branch\n")
            git("add", ".", cwd=work)
            git("commit", "-qm", "work", cwd=work)
            git("push", "-q", str(hub), "HEAD:refs/heads/staging/probe", cwd=work)

            # Copy tools/fleet and shadow verdict.py, so the REAL gate.sh runs
            # its own lookup/guard/parse against a payload we control.
            fleet = tmp / "fleet"
            subprocess.run(["cp", "-R", str(FLEET), str(fleet)], check=True)
            (fleet / "verdict.py").write_text(
                "import sys\n"
                f"sys.stdout.write({stub_lookup_stdout!r})\n"
                f"sys.exit({stub_rc})\n"
            )
            p = subprocess.run(
                ["bash", str(fleet / "gate.sh"), "staging/probe", "probe"],
                capture_output=True, text=True, env=env, timeout=300,
            )
            log = home / "gatelogs" / "gate-probe.log"
            v = home / "gatelogs" / "gate-probe.verdict"
            return (p.returncode,
                    log.read_text() if log.exists() else "",
                    v.read_text().strip() if v.exists() else "")

    def test_real_script_serves_a_cached_fail_as_nonzero(self):
        rc, log, verdict = self._run_gate('{"result": "FAIL", "tree_sha": "x"}')
        self.assertIn("GATE CACHE HIT", log, f"expected a cache hit; log:\n{log[:400]}")
        self.assertTrue(verdict.startswith("FAIL"), f"verdict: {verdict!r}")
        self.assertNotEqual(rc, 0, "the real gate.sh reported success on a cached FAIL")

    def test_real_script_falls_through_on_a_malformed_payload(self):
        """THE GAP THE SEAM TESTS LEFT. gate.sh's own parse must yield no
        result, so the script proceeds past the cache to a real run. Proven by
        the absence of a cache hit AND the presence of the post-cache oracle
        header, which is the first thing written after the cache block."""
        for payload, rc_stub in (("not json at all", 0), ('{"result": ""}', 0), ("", 0)):
            with self.subTest(payload=payload):
                rc, log, verdict = self._run_gate(payload, rc_stub)
                self.assertNotIn(
                    "GATE CACHE HIT", log,
                    f"payload {payload!r} was treated as a verdict by the real script",
                )
                self.assertIn(
                    "merged onto", log,
                    "the script did not reach the post-cache stage, so this test "
                    f"proves nothing about fall-through; log:\n{log[:400]}",
                )
                self.assertFalse(
                    verdict.startswith(("PASS cache-hit", "FAIL cache-hit")),
                    f"a cache verdict was written for {payload!r}: {verdict!r}",
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

    def test_version_is_namespaced_so_cross_line_collision_is_impossible(self):
        """A bare integer only avoids collision HISTORICALLY.

        An earlier version of this test asserted the value was not one of
        {5,6,7,8} -- the numbers the staging/agent-server line had already
        consumed. That is a check against the past: it cannot stop the other
        line choosing the same number tomorrow, and both lines write into one
        verdict cache. `verdict.py::verdict_ref` validates gate_version only as
        a non-empty string without "/" or "."/"..", so a per-line prefix makes
        the collision structurally impossible instead of merely unobserved.
        """
        v = self._sh_version()
        self.assertRegex(
            v, r"^[a-z][a-z0-9]*-",
            "GATE_VERSION must carry a per-line namespace prefix (e.g. 'tip-5'); "
            f"a bare value like {v!r} can collide with the other branch's "
            "sequence in the shared verdict cache",
        )
        # and it must remain a legal ref segment for verdict_ref()
        self.assertNotIn("/", v)
        self.assertNotIn(v, (".", ".."))
        self.assertTrue(v)


if __name__ == "__main__":
    unittest.main()
