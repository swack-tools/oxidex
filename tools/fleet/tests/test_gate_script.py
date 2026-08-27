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

BLOCKER A adds the isolation-retry half (see gate.sh's own header comment
for the measurement that motivated it -- ~25% of whole-stage runs red on a
clean tree, on modules that pass 10/10 alone):

  4. `TestFleetTestsFailedModuleParsing` -- cheap, no subprocess:
     `_fleet_tests_failed_modules()` extracted from the real script text
     and fed literal unittest output, including the shapes that must NOT
     parse (a py_compile traceback, an import-time loader error). Wrong in
     the permissive direction, an unparseable red stage gets retried into
     a "flake"; wrong in the strict direction, there is merely no retry.
     This pins both directions.
  5. `TestFleetTestsFlakeRetry` -- expensive, same REAL-gate.sh harness as
     (3): fixture modules red together and green alone (one leaks an env
     var at import -- the measured interference shape), proving the stage
     passes and the verdict records the flake; a module red both times,
     proving FAIL and never ABORT; a mixed pair, proving a partial
     recovery is still FAIL and records NO flakes; and a clean run,
     proving `fleet_tests_flakes` is absent rather than empty.

R4 (review of staging/agent-server @ 99f06cb3) adds the loud half of
`store_verdict()`'s hub-push failure:

  6. `TestStoreVerdictLoudFailure` -- cheap, same extraction technique as
     (2)/(4): `store_verdict()` pulled verbatim from the real script text
     and run standalone against a stand-in `verdict.py` this test
     controls, so success/failure is chosen directly rather than needing a
     real hub outage. Pins that a failed store still exits the function
     successfully (the gate's own PASS/FAIL is never at stake), while
     leaving a `<tag>.verdict-store-failed` marker beside the verdict and
     the line `GATE: VERDICT STORE FAILED` in the gate's log; a successful
     store leaves neither, and clears a stale marker a PRIOR failed
     attempt under the same TAG left behind; the `TREE_SHA`-empty skip
     path touches neither the log nor the marker.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import json
import os
import re
import shlex
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
from _env import HermeticCase, scrub_env  # noqa: E402


GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}


class TestGateVersionMatchesFile(HermeticCase):
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


class TestFleetTestModulesExcludeSeams(HermeticCase):
    """`_fleet_test_modules()` (gate.sh's BLOCKER-6(iv) helper) extracted
    verbatim from the real script text and run, standalone, against a
    throwaway directory -- the cheap mechanical half of the exclusion
    proof; `TestFleetTestsStageHasTeeth.test_seams_module_is_excluded...`
    below is the expensive end-to-end half.
    """

    def setUp(self):
        super().setUp()
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


class TestFleetTestsFailedModuleParsing(HermeticCase):
    """BLOCKER A: `_fleet_tests_failed_modules()` decides whether a red
    fleet-tests stage is even a candidate for the isolation retry, so it is
    the hinge of the whole policy. Extracted verbatim from the real script
    text and fed literal unittest output -- the two measured flake shapes
    (a plain FAIL and a tearDown ERROR), both the pre-3.11 and 3.11+ header
    spellings, and the shapes that MUST yield nothing.
    """

    def _parse(self, text: str) -> list:
        source = GATE_SH.read_text(encoding="utf-8")
        func_src = _extract_shell_function(source, "_fleet_tests_failed_modules")
        tmp = Path(tempfile.mkdtemp(prefix="fleet-failed-mods-"))
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        out = tmp / "out.txt"
        out.write_text(text)
        script = f'{func_src}\n_fleet_tests_failed_modules "$1"\n'
        result = subprocess.run(
            ["bash", "-c", script, "bash", str(out)],
            capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, f"stderr: {result.stderr}")
        return [line for line in result.stdout.splitlines() if line]

    def test_the_two_measured_flake_shapes_parse_to_their_modules(self):
        # Left column verbatim from the observed runs: a plain assertion
        # failure in one module, an OSError raised out of another module's
        # tearDown (which unittest reports as ERROR, not FAIL).
        text = (
            "FAIL: test_host_singleton_stays_live_past_its_ttl_and_is_released_on_exit "
            "(test_lease_protocol.TestFleetdSingletonRenews."
            "test_host_singleton_stays_live_past_its_ttl_and_is_released_on_exit)\n"
            "ERROR: test_restart_adopts (test_adoption.TestRestartAdoption.test_restart_adopts)\n"
        )
        self.assertEqual(
            self._parse(text), ["test_adoption", "test_lease_protocol"],
            "both shapes must resolve to their MODULE, sorted and deduped",
        )

    def test_pre_311_header_shape_still_parses(self):
        """Python < 3.11 prints `(<module>.<Class>)` with no trailing test
        name. Fleet hosts are not all on the same interpreter, and a parser
        that only understood one spelling would silently stop retrying on
        the other -- i.e. it would regress to today's false FAILs on
        exactly the hosts nobody was looking at."""
        self.assertEqual(
            self._parse("FAIL: test_x (test_lease_protocol.TestFleetdSingletonRenews)\n"),
            ["test_lease_protocol"],
        )

    def test_duplicate_failures_in_one_module_collapse_to_one_name(self):
        text = (
            "FAIL: test_a (test_adoption.TestOne.test_a)\n"
            "FAIL: test_b (test_adoption.TestTwo.test_b)\n"
        )
        self.assertEqual(self._parse(text), ["test_adoption"])

    def test_unparseable_red_output_yields_no_modules(self):
        """The safety direction. A py_compile syntax error, a bare
        traceback, or a crash with no FAIL:/ERROR: header at all must
        produce an EMPTY list, which gate.sh treats as 'not retryable' and
        fails on -- silence must never be read as a flake."""
        text = (
            'File "tools/fleet/claim.py", line 3\n'
            "    def broken(\n"
            "SyntaxError: unexpected EOF while parsing\n"
            "Traceback (most recent call last):\n"
            "Segmentation fault\n"
        )
        self.assertEqual(self._parse(text), [])

    def test_import_time_loader_error_does_not_name_a_real_module(self):
        """An import-time failure is reported against
        `unittest.loader._FailedTest`, so the parse yields the literal
        name `unittest` -- which is NOT in `_fleet_test_modules()`'s list,
        and gate.sh's known-module guard refuses to retry it. Pinned here
        because it is the one case where the parser produces a name that
        looks plausible and must still not be retried."""
        text = (
            "ERROR: test_zz_fixture (unittest.loader._FailedTest.test_zz_fixture)\n"
        )
        self.assertEqual(self._parse(text), ["unittest"])
        self.assertNotIn(
            "unittest", _real_fleet_test_modules(),
            "the known-module guard is what makes this safe; it only works "
            "because 'unittest' can never be one of the stage's own modules",
        )


def _real_fleet_test_modules() -> list:
    """`_fleet_test_modules()` from the real gate.sh, run against the real
    repo root -- the list the known-module guard checks against."""
    source = GATE_SH.read_text(encoding="utf-8")
    func_src = _extract_shell_function(source, "_fleet_test_modules")
    result = subprocess.run(
        ["bash", "-c", f"{func_src}\n_fleet_test_modules\n"],
        cwd=REPO_ROOT, capture_output=True, text=True, timeout=10,
    )
    return [line for line in result.stdout.splitlines() if line]


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
    env = scrub_env(**GIT_ENV)
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


class TestFleetCodeUrlDefaultsToHubUrl(HermeticCase):
    """B4 (Stage 1 integration review): `FLEET_CODE_URL` must default to
    `FLEET_HUB_URL` when unset, so a single-repo stand-in (one host, one
    repo playing both the verdict-cache-hub and code-repo role -- the i7
    workflow's `FLEET_HUB_URL=<local repo>` with no `FLEET_CODE_URL` at
    all) keeps working exactly as it did before the two-repo split
    introduced a second variable. Before this fix that stand-in was 100%
    `ABORT config: FLEET_CODE_URL not set`, on every single loop.

    Runs the REAL `tools/fleet/gate.sh` as a subprocess (same convention as
    `_RealGateHarness`), but does NOT require the real pinned ExifTool
    oracle: `EXIFTOOL_CACHE_DIR` is pointed at a fresh, guaranteed-empty
    directory, so the oracle-precondition check fails deterministically
    REGARDLESS of what is or isn't installed on the machine running this
    test. That failure (`ABORT oracle-precondition`) is itself the proof
    the fix works -- reaching it means gate.sh got all the way through
    cloning the fixture hub under the defaulted `CODE_URL`, past both
    config-abort checks, using nothing but `FLEET_HUB_URL`.
    """

    def setUp(self):
        super().setUp()
        self.tmp = Path(tempfile.mkdtemp(prefix="gate-code-url-default-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)

    def _run(self, branch: str, hub: Path, tag: str, extra_env: dict) -> dict:
        home = self.tmp / f"home-{tag}"
        home.mkdir()
        # A directory that cannot possibly hold a real exiftool-pinned.sh --
        # guarantees the oracle-precondition check fails fast, on any host,
        # without ever touching cargo/clippy/build.
        no_oracle = self.tmp / f"no-oracle-{tag}"
        env = {
            "HOME": str(home),
            "USER": os.environ.get("USER", "fleet-test"),
            "PATH": "/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin",
            "EXIFTOOL_CACHE_DIR": str(no_oracle),
        }
        env.update(extra_env)
        result = subprocess.run(
            ["bash", str(GATE_SH), branch, tag],
            env=env, capture_output=True, text=True, timeout=60,
        )
        verdict_path = home / "gatelogs" / f"gate-{tag}.verdict"
        json_path = home / "gatelogs" / f"gate-{tag}.json"
        return {
            "rc": result.returncode,
            "verdict": verdict_path.read_text() if verdict_path.exists() else None,
            "json": json.loads(json_path.read_text()) if json_path.exists() else None,
        }

    def test_hub_url_alone_reaches_the_oracle_stage_not_a_config_abort(self):
        branch = "staging/code-url-default-fixture"
        hub = _build_fixture_hub(self.tmp, {}, branch)

        res = self._run(branch, hub, "huburl-only", {"FLEET_HUB_URL": str(hub)})

        self.assertIsNotNone(res["verdict"], "gate.sh must have written a verdict at all")
        self.assertNotIn("ABORT config", res["verdict"],
                          f"FLEET_CODE_URL must default to FLEET_HUB_URL, not abort: {res}")
        self.assertNotIn("FLEET_CODE_URL not set", res["verdict"])
        self.assertIsNotNone(res["json"])
        self.assertEqual(res["json"]["result"], "ABORT")
        self.assertEqual(
            res["json"]["stage"], "oracle-precondition",
            f"expected to clone under the defaulted CODE_URL and reach the oracle "
            f"check, got stage={res['json']['stage']!r}: {res}",
        )

    def test_neither_url_set_is_still_a_loud_config_abort(self):
        """The one case the loud ABORT must still cover: nothing at all
        named a repo. `FLEET_HUB_URL`'s own check (independent of the
        default added here) fires first."""
        branch = "staging/code-url-default-fixture-neither"
        _build_fixture_hub(self.tmp, {}, branch)  # unused; no URL is ever passed

        res = self._run(branch, self.tmp / "unused.git", "neither-url", {})

        self.assertEqual(res["verdict"], "ABORT config: FLEET_HUB_URL not set\n")
        self.assertEqual(res["json"]["result"], "ABORT")
        self.assertEqual(res["json"]["stage"], "config")

    def test_code_url_alone_without_hub_url_still_aborts(self):
        """The default is one-directional: FLEET_CODE_URL defaults to
        FLEET_HUB_URL, never the reverse. FLEET_HUB_URL (the verdict-cache
        hub) stays independently required."""
        branch = "staging/code-url-default-fixture-codeonly"
        hub = _build_fixture_hub(self.tmp, {}, branch)

        res = self._run(branch, hub, "codeurl-only", {"FLEET_CODE_URL": str(hub)})

        self.assertEqual(res["verdict"], "ABORT config: FLEET_HUB_URL not set\n")
        self.assertEqual(res["json"]["stage"], "config")


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


class _RealGateHarness:
    """The shared machinery for running the REAL `tools/fleet/gate.sh` as a
    subprocess against a fixture hub. A mixin rather than a base TestCase
    on purpose: subclassing a TestCase would make every subclass re-run the
    parent's own (expensive, real-gate) test methods a second time, and
    each of those is a full git clone + merge + oracle probe.
    """

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        probe = ledger.probe_capability()
        if not probe.ok:
            raise unittest.SkipTest(
                f"real pinned ExifTool oracle unavailable ({probe.detail}) -- gate.sh "
                f"hardcodes /tmp/oxidex-exiftool-cache/exiftool-pinned.sh and this test "
                f"runs the REAL script, so it cannot fake this precondition away"
            )

    def setUp(self):
        super().setUp()
        self.tmp = Path(tempfile.mkdtemp(prefix="gate-script-teeth-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.fakebin = _build_fake_bin(self.tmp)

    def _run_gate(self, branch: str, hub: Path, tag: str, extra_env: dict = None,
                  timeout: float = 90.0):
        """Invoke the REAL tools/fleet/gate.sh, unmodified, as a
        subprocess. `HOME` is a fresh per-test tempdir (gate.sh writes
        gatelogs/, tgt/, and its verdict cache under `$HOME`, and its
        `$HOME/git/oxidex.git` mirror-probe must find nothing so it falls
        straight through to `FLEET_CODE_URL`, the fixture). PLAN Stage 1
        task 4 split the one hub gate.sh used to read into two required
        variables -- FLEET_HUB_URL (verdict cache) and FLEET_CODE_URL
        (the repo holding the branch to clone) -- so both point at the
        same single fixture `hub`, exactly as the one fixture repo served
        both roles before the split; `extra_env` may override either."""
        home = self.tmp / f"home-{tag}"
        home.mkdir()
        env = {
            "HOME": str(home),
            "USER": os.environ.get("USER", "fleet-test"),
            "PATH": f"{self.fakebin}:/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin",
            "FLEET_HUB_URL": str(hub),
            "FLEET_CODE_URL": str(hub),
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


class TestFleetTestsStageHasTeeth(_RealGateHarness, HermeticCase):
    """Runs the real `tools/fleet/gate.sh` against fixture hubs, proving
    BLOCKER 6 (ii) and (iii): the fleet-tests stage exists in the actual
    script (not merely described in a comment), a red suite there yields
    FAIL and never ABORT, and a hang there is bounded by
    FLEET_TESTS_TIMEOUT_S and also yields FAIL (stage
    "fleet-tests-timeout"), never a wedge.
    """

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
        eventually and happened to fail".

        KNOWN FLAKY UNDER LOAD -- PRE-EXISTING, MECHANISM NOT FOUND.
        Recorded here so the next person to see it red does not re-derive
        what has already been ruled out.

        Observed rate (instrument: this single test, `python3 -m unittest
        <this id>` from tools/fleet/tests, on a machine simultaneously
        running whole fleet-tests stages): roughly 1 in 5 to 1 in 10 while
        loaded, 0/30 idle. It is NOT caused by BLOCKER A's retry: an A/B
        alternating gate.sh between 9efc39b2's version (GATE_VERSION 6, no
        retry) and the current one, 5 runs each interleaved, went red on a
        v6 run and never on a v7 run.

        The failing shape is always the same: verdict `FAIL fleet-tests`
        instead of `FAIL fleet-tests-timeout`, the whole gate.sh run
        finishing in ~1.3s, the stage's captured output EMPTY, and the
        watchdog's timeout flag never written -- i.e. the stage returned
        non-zero long before the 3s budget, without the fixture's
        `time.sleep(30)` ever running and without printing anything.

        Ruled out: `wait`'s status fidelity under gate.sh's
        `set -m; ( cmd ) &; set +m` shape -- a probe replaying exactly that
        shape reported a wrong status 0/200 times. Not ruled out: a fork or
        exec failure inside the backgrounded subshell under load, which
        would produce precisely this signature (fast, empty, non-zero) and
        which the current code cannot distinguish from a genuine red stage.

        Deliberately NOT "fixed" by widening the budget or retrying here:
        the failure is not the budget being too tight (the run ends in
        1.3s, nowhere near 3s), so a wider budget would only make the flake
        rarer and the diagnosis harder. BLOCKER A's stage-level isolation
        retry already absorbs it in the gate -- test_gate_script re-run
        alone passes -- which is the argument for that policy being
        systemic rather than a per-test patch."""
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


# --------------------------------------------------------------------- #
# BLOCKER A: the isolation retry
# --------------------------------------------------------------------- #

# The interference is real, not simulated with a random number: one fixture
# module writes an env var AT IMPORT TIME and never cleans it up, and the
# other asserts that var is absent. Run together in one python process the
# second is red; run alone in a fresh process it is green -- which is
# precisely the "env leakage between modules" shape the clean-tree
# measurement turned up, reduced to two files. Nothing here is timing- or
# load-dependent, so these tests are deterministic in both directions.
_FIXTURE_LEAKER = (
    "import os, unittest\n"
    "os.environ['FLEET_GATE_FIXTURE_LEAK'] = '1'  # at IMPORT, never cleaned up\n"
    "class TestFixtureLeaker(unittest.TestCase):\n"
    "    def test_ok(self):\n"
    "        self.assertTrue(True)\n"
)

_FIXTURE_FLAKY = (
    "import os, unittest\n"
    "class TestFixtureFlaky(unittest.TestCase):\n"
    "    def test_needs_a_clean_env(self):\n"
    "        if os.environ.get('FLEET_GATE_FIXTURE_LEAK'):\n"
    "            self.fail('fixture: another module leaked FLEET_GATE_FIXTURE_LEAK')\n"
)

_FIXTURE_ALWAYS_RED = (
    "import unittest\n"
    "class TestFixtureAlwaysRed(unittest.TestCase):\n"
    "    def test_always_fails(self):\n"
    "        self.fail('fixture: fails alone too')\n"
)

_FIXTURE_ALWAYS_GREEN = (
    "import unittest\n"
    "class TestFixtureAlwaysGreen(unittest.TestCase):\n"
    "    def test_ok(self):\n"
    "        self.assertTrue(True)\n"
)


class TestFleetTestsFlakeRetry(_RealGateHarness, HermeticCase):
    """BLOCKER A, end-to-end against the REAL gate.sh (shares the fixture
    hub / stubbed-cargo harness above, including its oracle skip).

    Measurement this exists to serve: on a clean tree the whole fleet-tests
    stage went red on 2 of 8 runs, on two different modules, both of which
    passed 10/10 alone (instrument: scratchpad/flakeloop.sh). That false
    FAIL is published to the SHARED verdict cache and condemns the branch
    fleet-wide, so the retry is a correctness fix, not a convenience.

    Every case below reads the FINAL stage from the verdict JSON. Because
    `cargo` is stubbed no oxidex binary is ever produced, so a run that
    gets PAST fleet-tests necessarily dies later at "no-binary" -- that
    stage name is the proof the fleet-tests stage passed, exactly as in
    `test_seams_module_is_excluded_from_the_stage` above.
    """

    def test_module_red_in_the_stage_but_green_alone_passes_and_is_recorded(self):
        """(i) The whole point: interference between modules must not
        condemn a branch, and the flake must be RECORDED so burn-in can
        measure the real rate off the shared verdict cache instead of us
        guessing at it."""
        branch = "staging/gate-fixture-flake"
        hub = _build_fixture_hub(
            self.tmp,
            {
                "tools/fleet/tests/test_zz_fixture_leaker.py": _FIXTURE_LEAKER,
                "tools/fleet/tests/test_zz_fixture_flaky.py": _FIXTURE_FLAKY,
            },
            branch,
        )

        res = self._run_gate(branch, hub, "flake")

        self.assertIsNotNone(res["json"], f"gate never wrote a verdict; log:\n{res['log'][-3000:]}")
        # The stage really did go red first -- otherwise this test proves
        # nothing about the retry, only that two green modules stay green.
        self.assertIn(
            "fixture: another module leaked FLEET_GATE_FIXTURE_LEAK", res["log"],
            "the fixture flake never actually fired; the retry was never exercised",
        )
        self.assertNotEqual(
            res["json"]["stage"], "fleet-tests",
            f"a module that passes ALONE must not fail the stage. log tail:\n{res['log'][-3000:]}",
        )
        self.assertNotEqual(res["json"]["stage"], "fleet-tests-timeout")
        self.assertEqual(res["json"]["stage"], "no-binary",
                         "the run must proceed past fleet-tests and die at the stubbed build")

        flakes = res["json"].get("fleet_tests_flakes")
        self.assertIsNotNone(
            flakes,
            f"the verdict must record the flake; json was {res['json']}",
        )
        self.assertEqual([f["module"] for f in flakes], ["test_zz_fixture_flaky"])
        self.assertIn("FAIL", flakes[0]["failure"])
        self.assertIn("test_zz_fixture_flaky", flakes[0]["failure"])
        # And the retry is visible in the log as one round, not a loop.
        self.assertIn("PASSED alone -- recorded as a flake", res["log"])
        self.assertEqual(
            res["log"].count("--- fleet-tests isolation re-run: test_zz_fixture_flaky"), 1,
            "exactly ONE isolation round is allowed",
        )

    def test_module_red_alone_too_is_a_genuine_fail_not_an_abort(self):
        """(ii) The retry must not become a way to launder a real failure.
        A module that fails in isolation as well is the same FAIL it was
        before BLOCKER A -- and still never an ABORT, which would schedule
        a non-damning retry for genuinely broken coordination code."""
        branch = "staging/gate-fixture-hardred"
        hub = _build_fixture_hub(
            self.tmp,
            {"tools/fleet/tests/test_zz_fixture_hard.py": _FIXTURE_ALWAYS_RED},
            branch,
        )

        res = self._run_gate(branch, hub, "hardred")

        self.assertEqual(res["rc"], 1)
        self.assertEqual(res["verdict"], "FAIL fleet-tests\n", f"log tail:\n{res['log'][-3000:]}")
        self.assertEqual(res["json"]["result"], "FAIL")
        self.assertEqual(res["json"]["stage"], "fleet-tests")
        self.assertNotEqual(res["json"]["result"], "ABORT")
        self.assertNotIn("fleet_tests_flakes", res["json"],
                         "a genuine failure must publish no flake telemetry")
        # The retry was attempted and reported -- proving the FAIL came out
        # of the new path, not out of a code path that skipped it.
        self.assertIn("FAILED alone too -- genuine failure", res["log"])

    def test_one_module_recovers_and_one_does_not_is_still_a_fail(self):
        """(iii) Partial recovery is a FAIL. A run that is red for a real
        reason must also publish NO flakes: mixing genuine failures into
        the flake record would dilute exactly the rate this field exists to
        measure."""
        branch = "staging/gate-fixture-mixed"
        hub = _build_fixture_hub(
            self.tmp,
            {
                "tools/fleet/tests/test_zz_fixture_leaker.py": _FIXTURE_LEAKER,
                "tools/fleet/tests/test_zz_fixture_flaky.py": _FIXTURE_FLAKY,
                "tools/fleet/tests/test_zz_fixture_hard.py": _FIXTURE_ALWAYS_RED,
            },
            branch,
        )

        res = self._run_gate(branch, hub, "mixed")

        self.assertEqual(res["verdict"], "FAIL fleet-tests\n", f"log tail:\n{res['log'][-3000:]}")
        self.assertEqual(res["json"]["result"], "FAIL")
        self.assertEqual(res["json"]["stage"], "fleet-tests")
        self.assertNotIn("fleet_tests_flakes", res["json"])
        # Both modules were named as candidates, the recovering one was
        # re-run first (sorted), and the stubborn one still sank the stage.
        self.assertIn("test_zz_fixture_flaky", res["log"])
        self.assertIn("test_zz_fixture_hard", res["log"])
        self.assertIn("FAILED alone too -- genuine failure", res["log"])

    def test_clean_stage_records_no_flake_field_at_all(self):
        """(iv) Absent, not empty. `write_json` runs on every path
        (low-disk, clone, merge-conflict, ...) long before fleet-tests, so
        an always-present `[]` would be indistinguishable from 'this gate
        version does not record flakes' -- and burn-in's denominator would
        quietly include runs that never reached the stage."""
        branch = "staging/gate-fixture-clean"
        hub = _build_fixture_hub(
            self.tmp,
            {"tools/fleet/tests/test_zz_fixture_green.py": _FIXTURE_ALWAYS_GREEN},
            branch,
        )

        res = self._run_gate(branch, hub, "clean")

        self.assertIsNotNone(res["json"], f"gate never wrote a verdict; log:\n{res['log'][-3000:]}")
        self.assertEqual(res["json"]["stage"], "no-binary",
                         f"fleet-tests must be clean here. log tail:\n{res['log'][-3000:]}")
        self.assertNotIn("fleet_tests_flakes", res["json"])
        self.assertNotIn("isolation re-run", res["log"],
                         "no retry may run when nothing went red")


class TestStoreVerdictLoudFailure(HermeticCase):
    """R4: `store_verdict()` extracted verbatim from gate.sh's own text
    (same brace-matched technique as `TestFleetTestModulesExcludeSeams`/
    `TestFleetTestsFailedModuleParsing` above), run standalone against a
    tiny stand-in `$SELF_DIR/verdict.py` this test controls -- no real
    hub, no real `tools/fleet/verdict.py`, so success/failure is chosen
    directly instead of needing an actual hub outage. Proves gate.sh's own
    TEXT has the loud-failure behaviour, not a description of it.
    """

    def setUp(self):
        super().setUp()
        self.tmp = Path(tempfile.mkdtemp(prefix="fleet-store-verdict-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.self_dir = self.tmp / "self_dir"
        self.self_dir.mkdir()
        self.L = self.tmp / "gate-x.log"
        self.J = self.tmp / "gate-x.json"
        self.SV = self.tmp / "gate-x.verdict-store-failed"
        self.J.write_text("{}")
        self.L.write_text("")

    def _stub_verdict_py(self, exit_code: int) -> None:
        """Stands in for `python3 tools/fleet/verdict.py store ...` --
        the real script's argv (`store --hub-url ... --workdir ...
        --json-file ...`) is accepted and ignored; only the exit code
        matters to `store_verdict()`."""
        (self.self_dir / "verdict.py").write_text(
            "import sys\n"
            "sys.stderr.write('stub verdict.py store called\\n')\n"
            f"sys.exit({exit_code})\n"
        )

    def _run_store_verdict(self, *, tree_sha: str) -> subprocess.CompletedProcess:
        source = GATE_SH.read_text(encoding="utf-8")
        func_src = _extract_shell_function(source, "store_verdict")
        # Every free variable `store_verdict()` reads is set here to a
        # fixture value, exactly the shape gate.sh itself sets them to
        # before ever calling the function -- so this exercises the real
        # function body against real files, not a rewritten stand-in.
        script = (
            f"TREE_SHA={shlex.quote(tree_sha)}\n"
            f"SELF_DIR={shlex.quote(str(self.self_dir))}\n"
            "HUB_URL='https://example.invalid/state.git'\n"
            f"VERDICT_WORKDIR={shlex.quote(str(self.tmp / 'workdir'))}\n"
            f"J={shlex.quote(str(self.J))}\n"
            f"L={shlex.quote(str(self.L))}\n"
            f"SV={shlex.quote(str(self.SV))}\n"
            f"{func_src}\n"
            "store_verdict\n"
        )
        return subprocess.run(
            ["bash", "-c", script], capture_output=True, text=True, timeout=15,
        )

    def test_successful_store_writes_no_marker_and_no_loud_line(self):
        self._stub_verdict_py(0)
        result = self._run_store_verdict(tree_sha="a" * 40)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.SV.exists(), "a successful store must not leave a marker")
        self.assertNotIn("GATE: VERDICT STORE FAILED", self.L.read_text())

    def test_failed_store_is_loud_but_never_fails_the_caller(self):
        self._stub_verdict_py(1)
        result = self._run_store_verdict(tree_sha="a" * 40)
        # store_verdict()'s own exit status stays 0 on a hub-push failure
        # -- that is the non-fatality R4 explicitly keeps -- while the
        # failure becomes loud everywhere else: the marker, and the log.
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.SV.exists(), "a failed store must leave the sibling marker")
        self.assertIn("GATE: VERDICT STORE FAILED", self.L.read_text())
        # The underlying verdict.py failure is still visible in the log
        # for a human debugging this gate, not just the marker's existence.
        self.assertIn("stub verdict.py store called", self.L.read_text())

    def test_empty_tree_sha_skips_entirely(self):
        self._stub_verdict_py(1)  # would be loud if ever invoked
        result = self._run_store_verdict(tree_sha="")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.SV.exists())
        self.assertEqual(self.L.read_text(), "",
                         "TREE_SHA never resolving must not touch the log at all")

    def test_a_stale_marker_from_a_prior_failed_attempt_is_cleared_by_a_later_success(self):
        self.SV.write_text("")  # a previous run under this same TAG failed
        self._stub_verdict_py(0)
        result = self._run_store_verdict(tree_sha="a" * 40)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(
            self.SV.exists(),
            "a later successful store must clear a stale failure marker, "
            "not report a failure that has since resolved",
        )


if __name__ == "__main__":
    unittest.main()
