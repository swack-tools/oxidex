#!/usr/bin/env python3
"""Tests for `tools/fleet/keel/runner_toml.py` (PLAN Stage 3 task 7):
the `~/.keel/runner.toml` loader with env overrides for hub/code/server/
token paths and gates/agents caps.

Every test passes an explicit `path`/`env` -- never `DEFAULT_PATH`, never
the real `os.environ` -- so this suite is hermetic regardless of what
runner.toml (if any) exists on the machine actually running it.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import shutil
import sys
import tempfile
import unittest
from pathlib import Path

_FLEET_DIR = Path(__file__).resolve().parents[1]
_KEEL_DIR = _FLEET_DIR / "keel"
for _p in (_FLEET_DIR, _KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

from keel import runner_toml  # noqa: E402
from keel.runner_toml import RunnerConfig, RunnerTomlError  # noqa: E402
from _env import HermeticCase  # noqa: E402


class RunnerTomlTestCase(HermeticCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.mkdtemp(prefix="runner-toml-")
        self.addCleanup(shutil.rmtree, self._tmp, ignore_errors=True)

    def _write(self, text: str) -> Path:
        p = Path(self._tmp) / "runner.toml"
        p.write_text(text)
        return p


# --------------------------------------------------------------------- #
# load(): pure parse, no env involved
# --------------------------------------------------------------------- #


class TestLoad(RunnerTomlTestCase):
    def test_missing_file_is_every_field_none_not_an_error(self):
        cfg = runner_toml.load(Path(self._tmp) / "does-not-exist.toml")
        self.assertEqual(cfg, RunnerConfig())

    def test_empty_file_is_every_field_none(self):
        p = self._write("")
        self.assertEqual(runner_toml.load(p), RunnerConfig())

    def test_full_file_populates_every_field(self):
        p = self._write(
            """
            [hub]
            url = "https://github.com/example/state.git"

            [code]
            url = "https://github.com/example/code.git"

            [server]
            url = "http://100.64.1.2:8470"

            [token]
            git_file = "/home/allen/.keel/secrets/git-token"
            server_file = "/home/allen/.keel/secrets/server-token"

            [limits]
            max_gates = 2
            max_agents = 1
            """
        )
        cfg = runner_toml.load(p)
        self.assertEqual(cfg.hub_url, "https://github.com/example/state.git")
        self.assertEqual(cfg.code_url, "https://github.com/example/code.git")
        self.assertEqual(cfg.server_url, "http://100.64.1.2:8470")
        self.assertEqual(cfg.git_token_file, "/home/allen/.keel/secrets/git-token")
        self.assertEqual(cfg.server_token_file, "/home/allen/.keel/secrets/server-token")
        self.assertEqual(cfg.max_gates, 2)
        self.assertEqual(cfg.max_agents, 1)

    def test_partial_file_leaves_the_rest_none(self):
        p = self._write('[hub]\nurl = "https://example/state.git"\n')
        cfg = runner_toml.load(p)
        self.assertEqual(cfg.hub_url, "https://example/state.git")
        self.assertIsNone(cfg.code_url)
        self.assertIsNone(cfg.server_url)
        self.assertIsNone(cfg.max_gates)

    def test_token_paths_are_tilde_expanded(self):
        p = self._write('[token]\ngit_file = "~/.keel/secrets/git-token"\n')
        cfg = runner_toml.load(p)
        self.assertNotIn("~", cfg.git_token_file)
        self.assertTrue(cfg.git_token_file.endswith("/.keel/secrets/git-token"))

    def test_max_gates_zero_is_a_real_cap_not_none(self):
        """`0` (cap this host at zero gates) must survive as `0`, never
        collapse to `None` ('no cap') -- the module docstring's own
        distinction."""
        p = self._write("[limits]\nmax_gates = 0\n")
        cfg = runner_toml.load(p)
        self.assertEqual(cfg.max_gates, 0)
        self.assertIsNotNone(cfg.max_gates)

    def test_malformed_toml_raises(self):
        p = self._write("this is not [ valid toml")
        with self.assertRaises(RunnerTomlError):
            runner_toml.load(p)

    def test_non_string_url_raises(self):
        p = self._write("[hub]\nurl = 5\n")
        with self.assertRaises(RunnerTomlError):
            runner_toml.load(p)

    def test_non_integer_cap_raises(self):
        p = self._write('[limits]\nmax_gates = "two"\n')
        with self.assertRaises(RunnerTomlError):
            runner_toml.load(p)

    def test_boolean_cap_raises(self):
        """`bool` is an `int` subclass in Python -- `true`/`false` must
        not silently become `1`/`0`."""
        p = self._write("[limits]\nmax_gates = true\n")
        with self.assertRaises(RunnerTomlError):
            runner_toml.load(p)

    def test_negative_cap_raises(self):
        p = self._write("[limits]\nmax_gates = -1\n")
        with self.assertRaises(RunnerTomlError):
            runner_toml.load(p)

    def test_wrong_shape_table_raises(self):
        """`[hub]` as a bare scalar rather than a table -- a human typo
        (`hub = "..."` instead of `[hub]\\nurl = "..."`) must be reported,
        not silently treated as an empty table."""
        p = self._write('hub = "https://example/state.git"\n')
        with self.assertRaises(RunnerTomlError):
            runner_toml.load(p)

    def test_blank_string_url_is_none_not_empty_string(self):
        """A key present but blank (`url = \"\"` or all-whitespace) reads
        as unconfigured, matching every other optional-string convention
        in this tree (`_read_token_file`'s `text or None`)."""
        p = self._write('[hub]\nurl = "   "\n')
        cfg = runner_toml.load(p)
        self.assertIsNone(cfg.hub_url)


# --------------------------------------------------------------------- #
# resolve(): env overrides win
# --------------------------------------------------------------------- #


class TestResolveEnvOverrides(RunnerTomlTestCase):
    def _file(self):
        return self._write(
            """
            [hub]
            url = "https://file/state.git"

            [server]
            url = "http://file:8470"

            [limits]
            max_gates = 1
            max_agents = 1
            """
        )

    def test_no_env_returns_the_file_verbatim(self):
        p = self._file()
        cfg = runner_toml.resolve(p, env={})
        self.assertEqual(cfg, runner_toml.load(p))

    def test_env_var_overrides_the_files_value(self):
        p = self._file()
        cfg = runner_toml.resolve(p, env={"FLEET_HUB_URL": "https://env/state.git"})
        self.assertEqual(cfg.hub_url, "https://env/state.git")
        # everything else the file set is untouched
        self.assertEqual(cfg.server_url, "http://file:8470")

    def test_env_var_can_set_a_field_the_file_never_mentioned(self):
        p = self._write("")  # nothing configured
        cfg = runner_toml.resolve(p, env={"KEEL_SERVER_URL": "http://env:8470"})
        self.assertEqual(cfg.server_url, "http://env:8470")

    def test_empty_string_env_var_does_not_override(self):
        """A unit template that always sets the variable, sometimes to
        nothing, must not clobber a real file value with an empty one --
        SPEC's own env-knob convention (`claim.py`'s `_env_seconds`:
        unset/unparseable means the production default, never a crash;
        here it means 'not overridden')."""
        p = self._file()
        cfg = runner_toml.resolve(p, env={"FLEET_HUB_URL": ""})
        self.assertEqual(cfg.hub_url, "https://file/state.git")

    def test_missing_file_plus_env_is_env_only(self):
        cfg = runner_toml.resolve(
            Path(self._tmp) / "does-not-exist.toml",
            env={"FLEET_HUB_URL": "https://env/state.git", "KEEL_MAX_GATES": "3"},
        )
        self.assertEqual(cfg.hub_url, "https://env/state.git")
        self.assertEqual(cfg.max_gates, 3)
        self.assertIsNone(cfg.code_url)

    def test_all_seven_env_vars_are_wired(self):
        """One assertion per `ENV_VARS` entry -- a field added to
        `RunnerConfig` without a matching env override would silently
        never be overridable, and this is the fence against that."""
        env = {
            "FLEET_HUB_URL": "https://env/hub.git",
            "FLEET_CODE_URL": "https://env/code.git",
            "KEEL_SERVER_URL": "http://env-server:8470",
            "FLEET_GIT_TOKEN_FILE": "/env/git-token",
            "KEEL_TOKEN_FILE": "/env/server-token",
            "KEEL_MAX_GATES": "7",
            "KEEL_MAX_AGENTS": "9",
        }
        self.assertEqual(set(env), set(runner_toml.ENV_VARS.values()))
        cfg = runner_toml.resolve(Path(self._tmp) / "does-not-exist.toml", env=env)
        self.assertEqual(cfg.hub_url, "https://env/hub.git")
        self.assertEqual(cfg.code_url, "https://env/code.git")
        self.assertEqual(cfg.server_url, "http://env-server:8470")
        self.assertEqual(cfg.git_token_file, "/env/git-token")
        self.assertEqual(cfg.server_token_file, "/env/server-token")
        self.assertEqual(cfg.max_gates, 7)
        self.assertEqual(cfg.max_agents, 9)

    def test_env_token_file_is_also_tilde_expanded(self):
        cfg = runner_toml.resolve(
            Path(self._tmp) / "does-not-exist.toml",
            env={"FLEET_GIT_TOKEN_FILE": "~/.keel/secrets/git-token"},
        )
        self.assertNotIn("~", cfg.git_token_file)

    def test_env_var_max_gates_not_an_integer_raises(self):
        with self.assertRaises(RunnerTomlError):
            runner_toml.resolve(Path(self._tmp) / "does-not-exist.toml", env={"KEEL_MAX_GATES": "many"})

    def test_env_var_max_gates_negative_raises(self):
        with self.assertRaises(RunnerTomlError):
            runner_toml.resolve(Path(self._tmp) / "does-not-exist.toml", env={"KEEL_MAX_GATES": "-3"})

    def test_env_var_max_gates_zero_overrides_to_a_real_zero(self):
        p = self._file()  # file says max_gates = 1
        cfg = runner_toml.resolve(p, env={"KEEL_MAX_GATES": "0"})
        self.assertEqual(cfg.max_gates, 0)

    def test_default_env_is_os_environ(self):
        """`resolve()` with no `env=` argument reads the REAL
        `os.environ` -- proven by patching it, not by inspecting source."""
        import os
        from unittest import mock

        p = self._file()
        with mock.patch.dict(os.environ, {"FLEET_HUB_URL": "https://real-environ/x.git"}):
            cfg = runner_toml.resolve(p)
        self.assertEqual(cfg.hub_url, "https://real-environ/x.git")


class TestFieldNames(RunnerTomlTestCase):
    def test_field_names_matches_env_vars_keys(self):
        self.assertEqual(set(runner_toml.field_names()), set(runner_toml.ENV_VARS))


if __name__ == "__main__":
    unittest.main()
