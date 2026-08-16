#!/usr/bin/env python3
"""BLOCKER 6 ("R7 gets teeth") -- tests that exercise gate.sh's TEXT, not a
stand-in for it.

R7 (ARCH-FIX-SPEC.md) landed the fleet-tests stage, but nothing forced it
to actually behave: a hang in it had no wall clock, and its module list
silently included test_seams.py, which is exactly the suite this fleet's
own commit history documents as flaky under per-gate contention (see the
POLICY comment above `GATE_VERSION` in gate.sh). This file gives R7 teeth:

  1. `TestGateVersionMatchesFile` -- cheap, no subprocess: gate.sh and
     gate_version.txt must never drift apart (tools/fleet/gate.sh's own
     header comment states this contract; this test enforces it).
  2. `TestFleetTestModulesExcludeSeams` -- cheap: `_fleet_test_modules()`
     (the shell function gate.sh uses to build its module list) is
     extracted from the real script text and run, by itself, against a
     throwaway directory of dummy test files, proving the exclusion
     mechanically rather than by re-describing it in prose.
  3. `TestFleetTestsStageHasTeeth` -- NOT cheap: runs the REAL
     `tools/fleet/gate.sh`, unmodified, as a subprocess against a fixture
     git hub under `tempfile.gettempdir()`. Only `cargo` and `just` are
     stubbed, via a PATH directory placed ahead of the real ones -- gate.sh
     itself is never rewritten or copied (contrast
     `test_adoption.py:540`'s `gate.sh`-as-a-parked-stub, which replaces
     the whole script; the point here is the opposite: prove the actual
     script text has the behaviour, not a stand-in for it). Everything
     else in the pipeline runs for real: `git clone`/`merge`, the pinned
     ExifTool oracle-precondition probe, `verdict.py`'s hub-backed cache,
     and the fleet-tests stage's own `python3 -m py_compile` +
     `python3 -m unittest` run. SKIPPED when the real pinned oracle this
     machine's gate.sh hardcodes (`/tmp/oxidex-exiftool-cache/...`) is not
     present -- same convention as test_intent.py/test_ledger.py's own
     oracle-gated tests, and for the same reason: faking the oracle here
     would test nothing about whether R7's stage actually runs.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]  # tools/fleet
REPO_ROOT = FLEET_DIR.parents[1]
GATE_SH = FLEET_DIR / "gate.sh"
GATE_VERSION_TXT = FLEET_DIR / "gate_version.txt"

sys.path.insert(0, str(FLEET_DIR))
import ledger  # noqa: E402


GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}


class TestGateVersionMatchesFile(unittest.TestCase):
    """gate.sh's own header comment states the contract: 'GATE_VERSION and
    tools/fleet/gate_version.txt must always hold the same value; bump
    both together whenever gate BEHAVIOUR changes.' Parsing the script
    text is cheap and real -- no subprocess, no fixture, and it reads the
    ACTUAL assignment gate.sh runs with, not a value copy-pasted into this
    test that could itself drift.
    """

    def test_gate_version_variable_matches_gate_version_txt(self):
        text = GATE_SH.read_text(encoding="utf-8")
        # Anchored at the START of a line: gate.sh's header comments
        # mention "GATE_VERSION" freely in prose (e.g. "GATE_VERSION
        # bumped 5 -> 6"), and none of those lines begin with the bare
        # assignment shape `GATE_VERSION="..."` a comment line (starting
        # with `#`) can't match here.
        match = re.search(r'^GATE_VERSION="([^"]+)"', text, re.MULTILINE)
        self.assertIsNotNone(match, "gate.sh must set GATE_VERSION=\"...\" at column 0")
        script_version = match.group(1)

        file_version = GATE_VERSION_TXT.read_text(encoding="utf-8").strip()

        self.assertEqual(
            script_version, file_version,
            f"gate.sh GATE_VERSION={script_version!r} != "
            f"gate_version.txt={file_version!r} -- these must be bumped together "
            f"whenever gate behaviour changes (gate.sh's own header comment)",
        )
        # Sanity: the regex itself must not be silently matching nothing
        # useful (e.g. an empty string) -- a real gate version is never "".
        self.assertTrue(script_version, "parsed GATE_VERSION must be non-empty")


def _extract_shell_function(source: str, name: str) -> str:
    """The literal `name() { ... }` block from `source`, brace-matched (not
    a naive regex up to the first `}`, which would truncate at the first
    nested `if ... fi`/`while ... done` block's own closing brace-lookalike
    -- there are none in these two functions today, but a naive cut would
    fail silently and quietly test a truncated function instead of the
    real one, which is worse than an explicit error).
    """
    start_pat = re.compile(rf"(?m)^{re.escape(name)}\(\)\s*\{{")
    m = start_pat.search(source)
    if not m:
        raise AssertionError(f"could not find `{name}() {{` in gate.sh")
    depth = 0
    i = m.end() - 1  # position of the opening brace
    end = None
    for j in range(i, len(source)):
        c = source[j]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                end = j
                break
    if end is None:
        raise AssertionError(f"unbalanced braces extracting `{name}` from gate.sh")
    return source[m.start():end + 1]


class TestFleetTestModulesExcludeSeams(unittest.TestCase):
    """`_fleet_test_modules()` (gate.sh's BLOCKER-6(iv) helper) extracted
    verbatim from the real script text and run, standalone, against a
    throwaway directory -- the cheap mechanical half of the exclusion
    proof; `TestFleetTestsStageHasTeeth.test_seams_module_is_excluded...`
    below is the expensive end-to-end half.
    """

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="fleet-test-modules-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        tests_dir = self.tmp / "tools" / "fleet" / "tests"
        tests_dir.mkdir(parents=True)
        for name in ("test_alpha.py", "test_seams.py", "test_zzz_last.py"):
            (tests_dir / name).write_text("# fixture\n")
        # A non-test_*.py file must never appear in the module list.
        (tests_dir / "helpers.py").write_text("# fixture\n")

    def _run_function(self) -> list:
        source = GATE_SH.read_text(encoding="utf-8")
        func_src = _extract_shell_function(source, "_fleet_test_modules")
        script = f"{func_src}\n_fleet_test_modules\n"
        result = subprocess.run(
            ["bash", "-c", script], cwd=self.tmp, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, f"stderr: {result.stderr}")
        return [line for line in result.stdout.splitlines() if line]

    def test_seams_excluded_others_included_and_sorted(self):
        modules = self._run_function()
        self.assertNotIn("test_seams", modules, "test_seams.py must be excluded (POLICY)")
        self.assertEqual(
            modules, ["test_alpha", "test_zzz_last"],
            "every OTHER test_*.py must still be listed, sorted, minus the .py suffix",
        )


def _build_fixture_hub(tmp: Path, staging_files: dict, branch: str) -> Path:
    """A bare git hub under `tmp` (which must itself live under
    tempfile.gettempdir()) with:
      * `refs/heads/refactor/tag-machinery` -- the tip gate.sh hardcodes
        checking out. One trivial commit.
      * `refs/heads/<branch>` -- one commit stacked on the tip, adding
        `staging_files` (path -> content). A random nonce file is always
        included so every call produces a DISTINCT tree_sha -- gate.sh's
        own verdict cache (T1.2) is keyed on tree_sha+GATE_VERSION+
        platform_id, and without this two fixtures built with otherwise
        identical content would serve each other's cached verdict instead
        of each actually running the checks under test.
    """
    assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
    bare = tmp / "hub.git"
    work = tmp / "seed"
    env = {**os.environ, **GIT_ENV}
    subprocess.run(["git", "init", "-q", "--bare", str(bare)], check=True)
    subprocess.run(["git", "init", "-q", str(work)], check=True)
    (work / "README.md").write_text("tip\n")
    subprocess.run(["git", "-C", str(work), "add", "-A"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-q", "-m", "tip"], check=True, env=env)
    subprocess.run(
        ["git", "-C", str(work), "push", "-q", str(bare), "HEAD:refs/heads/refactor/tag-machinery"],
        check=True, env=env,
    )

    nonce_path = work / "tools" / "fleet" / "_fixture_nonce.py"
    nonce_path.parent.mkdir(parents=True, exist_ok=True)
    nonce_path.write_text(f"# {uuid.uuid4().hex}\n")
    for rel, content in staging_files.items():
        p = work / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content)
    subprocess.run(["git", "-C", str(work), "add", "-A"], check=True, env=env)
    subprocess.run(["git", "-C", str(work), "commit", "-q", "-m", "staging"], check=True, env=env)
    subprocess.run(
        ["git", "-C", str(work), "push", "-q", str(bare), f"HEAD:refs/heads/{branch}"],
        check=True, env=env,
    )
    return bare


_STUB_TOOL = "#!/bin/sh\nexit 0\n"


def _build_fake_bin(tmp: Path) -> Path:
    """A directory holding no-op `cargo` and `just` stand-ins, meant to sit
    AHEAD of the real ones on PATH. This is the only interception
    `TestFleetTestsStageHasTeeth` performs -- git, python3, rustc and the
    real pinned ExifTool oracle are all the genuine tools, found via the
    rest of PATH appended after this directory.
    """
    fakebin = tmp / "fakebin"
    fakebin.mkdir()
    for name in ("cargo", "just"):
        p = fakebin / name
        p.write_text(_STUB_TOOL)
        p.chmod(0o755)
    return fakebin


class TestFleetTestsStageHasTeeth(unittest.TestCase):
    """Runs the real `tools/fleet/gate.sh` against fixture hubs, proving
    BLOCKER 6 (ii) and (iii): the fleet-tests stage exists in the actual
    script (not merely described in a comment), a red suite there yields
    FAIL and never ABORT, and a hang there is bounded by
    FLEET_TESTS_TIMEOUT_S and also yields FAIL (stage
    "fleet-tests-timeout"), never a wedge.
    """

    @classmethod
    def setUpClass(cls):
        probe = ledger.probe_capability()
        if not probe.ok:
            raise unittest.SkipTest(
                f"real pinned ExifTool oracle unavailable ({probe.detail}) -- gate.sh "
                f"hardcodes /tmp/oxidex-exiftool-cache/exiftool-pinned.sh and this test "
                f"runs the REAL script, so it cannot fake this precondition away"
            )

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="gate-script-teeth-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.fakebin = _build_fake_bin(self.tmp)

    def _run_gate(self, branch: str, hub: Path, tag: str, extra_env: dict = None,
                  timeout: float = 90.0):
        """Invoke the REAL tools/fleet/gate.sh, unmodified, as a
        subprocess. `HOME` is a fresh per-test tempdir (gate.sh writes
        gatelogs/, tgt/, and its verdict cache under `$HOME`, and its
        `$HOME/git/oxidex.git` mirror-probe must find nothing so it falls
        straight through to `FLEET_HUB_URL`, the fixture)."""
        home = self.tmp / f"home-{tag}"
        home.mkdir()
        env = {
            "HOME": str(home),
            "USER": os.environ.get("USER", "fleet-test"),
            "PATH": f"{self.fakebin}:/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin",
            "FLEET_HUB_URL": str(hub),
        }
        if extra_env:
            env.update(extra_env)
        result = subprocess.run(
            ["bash", str(GATE_SH), branch, tag],
            env=env, capture_output=True, text=True, timeout=timeout,
        )
        verdict_path = home / "gatelogs" / f"gate-{tag}.verdict"
        json_path = home / "gatelogs" / f"gate-{tag}.json"
        log_path = home / "gatelogs" / f"gate-{tag}.log"
        return {
            "rc": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
            "verdict": verdict_path.read_text() if verdict_path.exists() else None,
            "json": json.loads(json_path.read_text()) if json_path.exists() else None,
            "log": log_path.read_text(errors="replace") if log_path.exists() else "",
        }

    def test_a_red_fleet_tests_suite_yields_fail_not_abort(self):
        """BLOCKER 6 (ii): a fixture branch whose only fleet-test asserts
        False must make the REAL gate.sh's fleet-tests stage FAIL --
        proving the stage exists and runs in the actual script text, and
        that a genuine red suite is never misclassified as the non-damning
        ABORT (which `classify_failure`'s OOM-signature grep could
        otherwise mistake it for, absent the stage=="fleet-tests" force)."""
        branch = "staging/gate-fixture-red"
        hub = _build_fixture_hub(
            self.tmp,
            {
                "tools/fleet/tests/test_zz_fixture_red.py": (
                    "import unittest\n"
                    "class TestFixtureRed(unittest.TestCase):\n"
                    "    def test_fails(self):\n"
                    "        self.fail('fixture: intentional failure')\n"
                ),
            },
            branch,
        )

        res = self._run_gate(branch, hub, "red")

        self.assertEqual(res["rc"], 1, f"gate.sh must exit nonzero on a failed stage: {res}")
        self.assertEqual(res["verdict"], "FAIL fleet-tests\n", f"log tail:\n{res['log'][-2000:]}")
        self.assertIsNotNone(res["json"])
        self.assertEqual(res["json"]["result"], "FAIL")
        self.assertEqual(res["json"]["stage"], "fleet-tests")
        self.assertNotEqual(res["json"]["result"], "ABORT", "a red suite must never be an ABORT")
        # And the failure recorded is the REAL one, not an import/path bug
        # in the harness (see gate.sh's `_run_fleet_tests_stage` comment on
        # why the module must be run from tools/fleet/tests, not the repo
        # root -- a regression there fails for the WRONG reason and this
        # assertion is what catches that).
        self.assertIn("fixture: intentional failure", res["log"])
        self.assertNotIn("ModuleNotFoundError", res["log"])

    def test_fleet_tests_stage_is_killed_on_timeout_and_fails(self):
        """BLOCKER 6 (iii): FLEET_TESTS_TIMEOUT_S bounds the fleet-tests
        stage. A fixture test that sleeps far longer than the configured
        budget must be killed and recorded as stage "fleet-tests-timeout",
        result FAIL -- and the whole gate.sh run must finish in well under
        the fixture test's own sleep duration, which is the only way to
        tell "the timeout fired" apart from "the test finished on its own
        eventually and happened to fail"."""
        branch = "staging/gate-fixture-slow"
        sleep_s = 30
        hub = _build_fixture_hub(
            self.tmp,
            {
                "tools/fleet/tests/test_zz_fixture_slow.py": (
                    "import time, unittest\n"
                    "class TestFixtureSlow(unittest.TestCase):\n"
                    "    def test_slow(self):\n"
                    f"        time.sleep({sleep_s})\n"
                ),
            },
            branch,
        )

        budget_s = 3
        start = time.monotonic()
        res = self._run_gate(
            branch, hub, "slow",
            extra_env={"FLEET_TESTS_TIMEOUT_S": str(budget_s)},
            timeout=60,
        )
        elapsed = time.monotonic() - start

        self.assertEqual(res["rc"], 1, f"gate.sh must exit nonzero on a timed-out stage: {res}")
        self.assertEqual(res["verdict"], "FAIL fleet-tests-timeout\n", f"log tail:\n{res['log'][-2000:]}")
        self.assertIsNotNone(res["json"])
        self.assertEqual(res["json"]["result"], "FAIL")
        self.assertEqual(res["json"]["stage"], "fleet-tests-timeout")
        self.assertLess(
            elapsed, sleep_s,
            f"gate.sh took {elapsed:.1f}s, >= the fixture's own {sleep_s}s sleep -- "
            f"the wall-clock timeout did not actually fire in time to be the reason "
            f"this run finished",
        )

    def test_seams_module_is_excluded_from_the_stage(self):
        """BLOCKER 6 (iv), end-to-end: a fixture branch whose test_seams.py
        would fail if it ran, alongside a normal test that passes, must
        NOT fail the fleet-tests stage on account of test_seams.py -- the
        real gate.sh, run for real, must get PAST fleet-tests. `cargo`
        being stubbed means the build never produces a real oxidex binary,
        so this run is expected to fail LATER, at "no-binary" -- which is
        exactly the point: the failure must not be, and must never again
        become, "fleet-tests"."""
        branch = "staging/gate-fixture-seams"
        hub = _build_fixture_hub(
            self.tmp,
            {
                "tools/fleet/tests/test_zz_fixture_pass.py": (
                    "import unittest\n"
                    "class TestFixturePass(unittest.TestCase):\n"
                    "    def test_ok(self):\n"
                    "        self.assertTrue(True)\n"
                ),
                "tools/fleet/tests/test_seams.py": (
                    "import unittest\n"
                    "class TestSeamsMustNeverRunUnderGate(unittest.TestCase):\n"
                    "    def test_would_fail_if_run(self):\n"
                    "        self.fail('test_seams.py must be excluded from gate.sh (POLICY)')\n"
                ),
            },
            branch,
        )

        res = self._run_gate(branch, hub, "seams-excl")

        self.assertIsNotNone(res["json"], f"gate never wrote a verdict; log:\n{res['log'][-2000:]}")
        self.assertNotEqual(
            res["json"]["stage"], "fleet-tests",
            f"fleet-tests failed -- test_seams.py's fixture assertion ran when it must "
            f"have been excluded. log tail:\n{res['log'][-2000:]}",
        )
        self.assertNotEqual(res["json"]["stage"], "fleet-tests-timeout")
        self.assertNotIn(
            "test_seams.py must be excluded from gate.sh", res["log"],
            "the excluded module's own failure text must never appear in the log at all",
        )
        # The real, expected reason this fixture run fails: cargo is
        # stubbed, so no oxidex binary was ever produced.
        self.assertEqual(res["json"]["stage"], "no-binary")


if __name__ == "__main__":
    unittest.main()
