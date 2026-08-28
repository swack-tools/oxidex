#!/usr/bin/env python3
"""THE FENCE: a gate environment can never again turn hermetic tests red.

WHAT HAPPENED (gate `keel1`, i7, `staging/agent-server@90cb01e4`). The fleet
suite was 642/642 on the m5 and red on the i7 in four modules --
`test_code_url_split`, `test_install_secrets`, `test_train_three_repos`,
`test_verdict_marker_seam` -- and every failure was the INVOKING
environment, not the code: `gate.sh` exports `FLEET_HUB_URL` and sources
`units/fleet-env.sh` (which exports `EXIFTOOL_CACHE_DIR` and
`FLEET_VERDICT_STORE_FAILED_SUFFIX`), then runs this suite as a child;
fixtures that built subprocess envs as `{**os.environ, ...}` or ran entry
points in-process inherited the gate's real hub, its real marker suffix
and (on a host without a domain in its hostname) git's inability to
auto-detect an identity under a redirected HOME. See `_env.py` for the
full account and the one helper every fixture now goes through.

WHAT THIS FILE PROVES, in three layers, cheapest first:

  1. `scrub_env` / `HermeticEnvMixin` do what they say: the fleet-shaped
     variables go, the test-run knobs stay, `extra` wins, the git identity
     is pinned, and `os.environ` is restored afterwards byte for byte.
  2. The instrument has teeth: a throwaway test module that reads
     `FLEET_HUB_URL` straight from `os.environ` FAILS under the poisoned
     environment below, and the same read through `scrub_env` PASSES. A
     fence that cannot be shown to discriminate is decoration.
  3. The four modules that went red are run, as real subprocesses, with
     `FLEET_HUB_URL=/nonexistent/hub.git FLEET_CODE_URL=/nonexistent/code.git
     EXIFTOOL_CACHE_DIR=/nonexistent` (and the other leaks found alongside
     them) exported -- a strictly NASTIER environment than any gate's,
     because a fixture that so much as touches one of these paths fails
     loudly instead of quietly reading a real repo -- and must be green.

Instrument: `python3 -m unittest <module>` in a subprocess, cwd
`tools/fleet/tests`, exactly the way `gate.sh`'s `_fleet_tests_unittest_run`
invokes it. The four run CONCURRENTLY (they are independent, tempdir-only
fixtures) so the fence costs the wall time of the slowest, not the sum.

Run with:
    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_env_hermetic -v
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest import mock

import _env
from _env import GIT_IDENTITY, HermeticCase, HermeticEnvMixin, scrub_env

TESTS_DIR = Path(__file__).resolve().parent

# The modules gate keel1 turned red. Kept as a literal list rather than a
# glob: the point is that THESE, which are known to spawn the train, the
# shell scripts and bash, survive a hostile invoker.
RED_MODULES = (
    "test_code_url_split",
    "test_install_secrets",
    "test_train_three_repos",
    "test_verdict_marker_seam",
)

# Every variable a gate (or an operator's shell) has been seen to export
# around the suite, each pointing somewhere that cannot possibly work, so
# that "the fixture inherited it" is a loud failure rather than a quiet
# read of a real repository.
POISON = {
    "FLEET_HUB_URL": "/nonexistent/hub.git",
    "FLEET_CODE_URL": "/nonexistent/code.git",
    "FLEET_CODE_PUSH_URL": "/nonexistent/code-push.git",
    "FLEET_TIP_PUSH_URL": "/nonexistent/tip.git",
    "EXIFTOOL_CACHE_DIR": "/nonexistent",
    "EXIFTOOL": "/nonexistent/exiftool-pinned.sh",
    "FLEET_VERDICT_STORE_FAILED_SUFFIX": ".poisoned-by-the-invoker",
    "FLEET_GIT_TOKEN_FILE": "/nonexistent/git-token",
    "FLEET_TRAIN_DEPLOY_KEY": "/nonexistent/deploy-key",
    "FLEET_TRAIN_TOKEN": "/nonexistent/train.token",
    "FLEET_AGENT_CLI_OVERRIDE": "/nonexistent/agent-cli",
    "GIT_SSH_COMMAND": "/nonexistent/ssh",
    "KEEL_POISON": "1",
}

# Default mirrors gate.sh's FLEET_TESTS_TIMEOUT_S default (1800 since
# GATE_VERSION 8); a single module gets the whole-stage budget at worst.
PER_MODULE_TIMEOUT_S = float(os.environ.get("FLEET_TESTS_TIMEOUT_S", "1800"))


def _poisoned_env(**extra: str) -> dict:
    env = dict(os.environ)
    env.update(POISON)
    env["FLEET_TESTS_HERMETIC"] = "1"
    env.update(extra)
    return env


def _run_module(module: str, cwd: Path = TESTS_DIR, timeout: float = PER_MODULE_TIMEOUT_S):
    """One `python3 -m unittest <module>` under the poisoned environment --
    the invocation shape `gate.sh` uses, with a worse environment."""
    return subprocess.run(
        [sys.executable, "-m", "unittest", module],
        cwd=str(cwd), env=_poisoned_env(), capture_output=True, text=True, timeout=timeout,
    )


# --------------------------------------------------------------------- #
# 1. the helper itself
# --------------------------------------------------------------------- #


class TestScrubEnv(HermeticCase):
    def test_every_poison_variable_is_scrubbed(self):
        for name in POISON:
            with self.subTest(name=name):
                self.assertTrue(_env.is_scrubbed(name), f"{name} would survive scrub_env")

    def test_scrubbed_copy_has_none_of_them_and_os_environ_is_untouched(self):
        with mock.patch.dict(os.environ, POISON):
            out = scrub_env()
            for name in POISON:
                self.assertNotIn(name, out)
            # The copy is a copy: the process environment still carries them.
            self.assertEqual(os.environ["FLEET_HUB_URL"], POISON["FLEET_HUB_URL"])

    def test_the_test_run_knobs_survive(self):
        keep = {"FLEET_TESTS_HERMETIC": "1", "FLEET_TESTS_TIMEOUT_S": "5",
                "FLEET_SEAMS_SLOW": "1", "FLEET_LIVE_GITHUB": "1",
                "FLEET_TEST_HUB_URL": "/x.git"}
        with mock.patch.dict(os.environ, {**POISON, **keep}):
            out = scrub_env()
        for name, value in keep.items():
            with self.subTest(name=name):
                self.assertEqual(out.get(name), value)

    def test_extra_wins_and_is_stringified(self):
        out = scrub_env(FLEET_HOST="h", FLEET_TEST_TTL_S=12.0, HOME=Path("/tmp/x"))
        self.assertEqual(out["FLEET_HOST"], "h")
        self.assertEqual(out["FLEET_TEST_TTL_S"], "12.0")
        self.assertEqual(out["HOME"], "/tmp/x")

    def test_git_identity_is_pinned(self):
        """The i7 half of the incident: under a redirected HOME, git on a
        host whose hostname has no domain cannot auto-detect an email and
        every merge/commit the code under test makes dies with 'Committer
        identity unknown'. The scrubbed env carries a fixed identity."""
        out = scrub_env()
        for name, value in GIT_IDENTITY.items():
            self.assertEqual(out[name], value)
        r = subprocess.run(["git", "var", "GIT_COMMITTER_IDENT"],
                           env=scrub_env(HOME=tempfile.gettempdir()),
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(r.stdout.startswith("t <t@t>"), r.stdout)

    def test_unrelated_variables_pass_through(self):
        with mock.patch.dict(os.environ, {"UNRELATED_VAR": "kept"}):
            self.assertEqual(scrub_env()["UNRELATED_VAR"], "kept")


class TestHermeticEnvMixin(HermeticCase):
    """The in-process route, observed from outside: a nested TestCase is run
    through a real `unittest.TestResult` so that `setUp`, the body, and the
    cleanup all happen, and the environment is compared before and after."""

    def test_scrubs_for_the_body_and_restores_afterwards(self):
        seen: dict = {}

        class Probe(HermeticEnvMixin, unittest.TestCase):
            def setUp(self):
                super().setUp()
                seen["in_setup"] = dict(os.environ)

            def test_body(self):
                seen["in_body"] = dict(os.environ)
                os.environ["FLEET_HOST"] = "set-by-the-test"  # must be undone too
                self.assertEnvHermetic(allow=["FLEET_HOST"])
                with self.assertRaises(AssertionError):
                    self.assertEnvHermetic()

        with mock.patch.dict(os.environ, POISON):
            before = dict(os.environ)
            result = unittest.TestResult()
            Probe("test_body").run(result)
            after = dict(os.environ)

        self.assertEqual(result.failures, [], result.failures)
        self.assertEqual(result.errors, [], result.errors)
        for name in POISON:
            self.assertNotIn(name, seen["in_setup"])
            self.assertNotIn(name, seen["in_body"])
        for name, value in GIT_IDENTITY.items():
            self.assertEqual(seen["in_body"][name], value)
        self.assertEqual(after, before, "os.environ was not restored byte for byte")

    def test_hermetic_case_is_the_mixin_plus_testcase(self):
        self.assertTrue(issubclass(HermeticCase, HermeticEnvMixin))
        self.assertTrue(issubclass(HermeticCase, unittest.TestCase))


# --------------------------------------------------------------------- #
# 1b. every fixture in this directory goes through the helper
# --------------------------------------------------------------------- #


class TestEveryFixtureIsHermetic(HermeticCase):
    """Structural fence, read from the ASTs: no top-level test class in
    `tools/fleet/tests/test_*.py` inherits `unittest.TestCase` directly
    (it must be `HermeticCase`, or a class that is), and every `setUp` /
    `setUpClass` defined on one calls its `super()` so the scrub actually
    runs. The poisoned subprocess runs below cover four modules; this
    covers all of them, for the cost of a parse. Classes nested inside
    functions (probes a test builds and runs itself) are not scanned."""

    BARE_BASES = {"unittest.TestCase", "TestCase"}

    def _top_level_classes(self):
        import ast
        for path in sorted(TESTS_DIR.glob("test_*.py")):
            tree = ast.parse(path.read_text(), filename=str(path))
            for node in tree.body:
                if isinstance(node, ast.ClassDef):
                    yield path.name, node

    def test_no_fixture_inherits_unittest_testcase_directly(self):
        import ast
        bare = [f"{name}:{cls.lineno} {cls.name}({', '.join(ast.unparse(b) for b in cls.bases)})"
                for name, cls in self._top_level_classes()
                if any(ast.unparse(b) in self.BARE_BASES for b in cls.bases)]
        self.assertEqual(bare, [], "these fixtures bypass _env.HermeticCase:\n" + "\n".join(bare))

    def test_every_setup_calls_super_so_the_scrub_runs(self):
        import ast
        missing = []
        for name, cls in self._top_level_classes():
            for item in cls.body:
                if isinstance(item, ast.FunctionDef) and item.name in ("setUp", "setUpClass"):
                    if not any(f"super().{item.name}()" in ast.unparse(s) for s in item.body):
                        missing.append(f"{name}:{item.lineno} {cls.name}.{item.name}")
        self.assertEqual(missing, [], "these setUp/setUpClass never call super(), so "
                         "HermeticEnvMixin never scrubs for them:\n" + "\n".join(missing))

    def test_the_ast_rule_would_catch_a_bare_testcase(self):
        """Negative control: the rule fires on the pre-fix shape."""
        import ast
        tree = ast.parse("import unittest\nclass T(unittest.TestCase):\n    def setUp(self):\n        pass\n")
        cls = tree.body[1]
        self.assertTrue(any(ast.unparse(b) in self.BARE_BASES for b in cls.bases))
        setup = cls.body[0]
        self.assertFalse(any("super().setUp()" in ast.unparse(s) for s in setup.body))


# --------------------------------------------------------------------- #
# 2. the fence has teeth
# --------------------------------------------------------------------- #


class TestThePoisonReachesATestProcess(HermeticCase):
    """Negative control for layer 3. If the poison did not reach a
    subprocess test module, the four green runs below would prove nothing."""

    def setUp(self):
        super().setUp()
        self.tmp = Path(tempfile.mkdtemp(prefix="env-fence-"))
        self.addCleanup(__import__("shutil").rmtree, self.tmp, ignore_errors=True)
        # `_env` must be importable from the throwaway module's directory the
        # same way it is from tools/fleet/tests.
        (self.tmp / "_env.py").write_text((TESTS_DIR / "_env.py").read_text())

    def _write(self, name: str, body: str) -> None:
        (self.tmp / f"{name}.py").write_text(textwrap.dedent(body))

    def test_a_fixture_that_copies_os_environ_goes_red_and_one_that_scrubs_stays_green(self):
        self._write("test_leaky", """
            import os, unittest
            class T(unittest.TestCase):
                def test_hub_is_not_set(self):
                    env = {**os.environ}   # the pre-fix idiom
                    self.assertNotIn("FLEET_HUB_URL", env)
        """)
        self._write("test_sealed", """
            import os, unittest
            from _env import HermeticCase, scrub_env
            class T(HermeticCase):
                def test_hub_is_not_set(self):
                    self.assertNotIn("FLEET_HUB_URL", scrub_env())
                    self.assertNotIn("FLEET_HUB_URL", os.environ)
                    self.assertEqual(os.environ.get("FLEET_TESTS_HERMETIC"), "1")
        """)
        leaky = _run_module("test_leaky", cwd=self.tmp, timeout=60)
        sealed = _run_module("test_sealed", cwd=self.tmp, timeout=60)
        self.assertNotEqual(leaky.returncode, 0,
                            "the poison never reached the test process; the fence is vacuous\n"
                            + leaky.stderr)
        self.assertIn("FLEET_HUB_URL", leaky.stderr)
        self.assertEqual(sealed.returncode, 0, sealed.stderr)


# --------------------------------------------------------------------- #
# 3. the four red modules, under a worse environment than any gate's
# --------------------------------------------------------------------- #


class TestRedModulesAreGreenUnderAPoisonedEnvironment(HermeticCase):
    def test_each_module_passes_with_every_leak_exported(self):
        with ThreadPoolExecutor(max_workers=len(RED_MODULES)) as pool:
            results = dict(zip(RED_MODULES, pool.map(_run_module, RED_MODULES)))
        failed = []
        for module, r in results.items():
            summary = [l for l in r.stderr.splitlines() if l.startswith(("Ran ", "OK", "FAILED"))]
            if r.returncode != 0 or not any(l.startswith("OK") for l in summary):
                tail = "\n".join(r.stderr.splitlines()[-60:])
                failed.append(f"--- {module}: rc={r.returncode} {summary}\n{tail}")
        self.assertEqual(failed, [], "\n".join(
            ["hermetic modules went red under the poisoned environment "
             f"{sorted(POISON)}:"] + failed))


if __name__ == "__main__":
    unittest.main()
