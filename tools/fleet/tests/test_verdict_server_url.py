#!/usr/bin/env python3
"""Tests for `tools/fleet/verdict.py`'s `--server-url` (PLAN Stage 3 task 7).

"a gate can store its verdict through the server, falling back to
direct" (PLAN Stage 3 task 7). `verdict.build_hub` is the seam: no
`--server-url`/`KEEL_SERVER_URL` configured must behave EXACTLY as
before this flag existed (a plain `fleetlib.Hub`, no `keel` import even
touched at call time); configured, it must build
`FallbackHub(ServerHub(...), Hub(...))` -- the identical shape every
other coordination write in this fleet goes through (SPEC SS4.3) -- and
`store()`/`lookup()` must actually round-trip THROUGH a real fixture
`keel-server`, and keep working when that server disappears mid-run.

Fixture servers bind `127.0.0.1` only (hard rule; also `keel/server.py`'s
own default and every other fixture in this directory).

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

_FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(_FLEET_DIR))

from fleetlib import Hub  # noqa: E402
import verdict  # noqa: E402
from _env import HermeticCase  # noqa: E402
from _fixtures import _ServerHubFixture  # noqa: E402
from keel.fallbackhub import FallbackHub  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402


def _good_payload(**overrides) -> dict:
    payload = {
        "tree_sha": "a" * 40,
        "base_tip": "b" * 40,
        "branch": "staging/example",
        "result": "PASS",
        "stage": "complete",
        "gate_version": "2",
        "rustc_id": "r" * 64,
        "platform_id": "p" * 64,
        "host": "server",
        "duration_s": 120,
        "write_set": ["src/foo.rs"],
    }
    payload.update(overrides)
    return payload


def _ns(**kwargs) -> argparse.Namespace:
    base = dict(hub_url="unused", workdir="unused", server_url=None, token_file=None)
    base.update(kwargs)
    return argparse.Namespace(**base)


# --------------------------------------------------------------------- #
# build_hub: unit-level routing, no network
# --------------------------------------------------------------------- #


class TestBuildHubRouting(HermeticCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.mkdtemp(prefix="verdict-server-url-")
        self.addCleanup(shutil.rmtree, self._tmp, ignore_errors=True)
        self.hub_path = str(Path(self._tmp) / "hub.git")
        init = subprocess.run(["git", "init", "--quiet", "--bare", self.hub_path], capture_output=True)
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())
        self.workdir = str(Path(self._tmp) / "cache")

    def test_no_server_url_anywhere_returns_a_plain_hub(self):
        """The default, and every existing gate.sh invocation: no
        `--server-url`, no `KEEL_SERVER_URL` -- must be a bare
        `fleetlib.Hub`, not a `FallbackHub` wrapping one, so behaviour is
        byte-for-byte what it was before this flag existed."""
        args = _ns(hub_url=self.hub_path, workdir=self.workdir)
        hub = verdict.build_hub(args)
        self.assertIsInstance(hub, Hub)
        self.assertNotIsInstance(hub, FallbackHub)

    def test_server_url_flag_builds_a_fallback_hub(self):
        args = _ns(hub_url=self.hub_path, workdir=self.workdir, server_url="http://127.0.0.1:1")
        hub = verdict.build_hub(args)
        self.assertIsInstance(hub, FallbackHub)
        self.assertIsInstance(hub.primary, ServerHub)
        self.assertIsInstance(hub.github, Hub)
        self.assertEqual(hub.primary.base_url, "http://127.0.0.1:1")

    def test_server_url_env_var_used_when_flag_absent(self):
        with self._env({"KEEL_SERVER_URL": "http://127.0.0.1:2"}):
            args = _ns(hub_url=self.hub_path, workdir=self.workdir)
            hub = verdict.build_hub(args)
        self.assertIsInstance(hub, FallbackHub)
        self.assertEqual(hub.primary.base_url, "http://127.0.0.1:2")

    def test_explicit_flag_wins_over_env_var(self):
        with self._env({"KEEL_SERVER_URL": "http://127.0.0.1:2"}):
            args = _ns(hub_url=self.hub_path, workdir=self.workdir, server_url="http://127.0.0.1:3")
            hub = verdict.build_hub(args)
        self.assertEqual(hub.primary.base_url, "http://127.0.0.1:3")

    def test_no_server_url_never_reads_a_token_file(self):
        """A token file that does not exist must not even be attempted
        when there is no server -- `_read_token_file` is called only on
        the FallbackHub-building branch."""
        args = _ns(
            hub_url=self.hub_path, workdir=self.workdir,
            token_file=str(Path(self._tmp) / "does-not-exist"),
        )
        hub = verdict.build_hub(args)  # must not raise / must not warn
        self.assertIsInstance(hub, Hub)

    def test_token_file_contents_reach_the_serverhub(self):
        token_path = Path(self._tmp) / "token"
        token_path.write_text("  fixture-token-value  \n")
        args = _ns(hub_url=self.hub_path, workdir=self.workdir, server_url="http://127.0.0.1:1", token_file=str(token_path))
        hub = verdict.build_hub(args)
        # ServerHub never exposes the token via repr/attribute name that
        # reads as public API, but it does hold it as `_token` for the
        # Authorization header -- this is the one place a test may peek,
        # to prove the file's contents (stripped) made it through, not
        # the literal file path or an unstripped copy.
        self.assertEqual(hub.primary._token, "fixture-token-value")

    def test_missing_token_file_is_reported_not_raised(self):
        args = _ns(
            hub_url=self.hub_path, workdir=self.workdir, server_url="http://127.0.0.1:1",
            token_file=str(Path(self._tmp) / "does-not-exist"),
        )
        hub = verdict.build_hub(args)  # must not raise
        self.assertIsInstance(hub, FallbackHub)
        self.assertIsNone(hub.primary._token)

    def _env(self, extra: dict):
        import os
        import unittest.mock as mock

        return mock.patch.dict(os.environ, extra)


# --------------------------------------------------------------------- #
# Round trip through a REAL fixture keel-server, and the fallback off it
# --------------------------------------------------------------------- #


class TestStoreAndLookupThroughServer(HermeticCase):
    """`_ServerHubFixture` starts a real `KeelHTTPServer` (SPEC C6) bound
    to `127.0.0.1:0`, backed by a `CachedHub` fronting a throwaway bare
    repo -- the same stack `keel/cli.py` and a runner talk to, not a
    stub. These tests drive `verdict.build_hub`/`store`/`lookup` through
    it, so the seam under test is the exact one `gate.sh` will call.
    """

    def setUp(self):
        super().setUp()
        self._tmp = tempfile.mkdtemp(prefix="verdict-server-url-e2e-")
        self.addCleanup(shutil.rmtree, self._tmp, ignore_errors=True)
        self.hub_path = str(Path(self._tmp) / "hub.git")
        init = subprocess.run(["git", "init", "--quiet", "--bare", self.hub_path], capture_output=True)
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())
        self.workdir = Path(self._tmp) / "cache"
        self.fixture = _ServerHubFixture(self.hub_path, self.workdir, code_url=None)
        self.addCleanup(self.fixture.close)

    def _args(self, **overrides):
        token_path = Path(self._tmp) / "server-token"
        if not token_path.exists():
            token_path.write_text(self.fixture.token)
        base = dict(
            hub_url=self.hub_path,
            workdir=str(self.workdir),
            server_url=self.fixture.server_url,
            token_file=str(token_path),
        )
        base.update(overrides)
        return _ns(**base)

    def test_store_through_the_server_lands_on_the_real_repo(self):
        args = self._args()
        hub = verdict.build_hub(args)
        self.assertIsInstance(hub, FallbackHub)

        payload = _good_payload()
        outcome = verdict.store(hub, payload)
        self.assertEqual(outcome, "created")

        # Proof the write actually reached the bare repo (not just the
        # server's in-memory index): a fresh, direct `fleetlib.Hub`
        # reading the SAME hub_path, never touching the server, must see
        # it too -- "a sha obtained via either route is valid on the
        # other" (SPEC SS4.2).
        direct = Hub(url=self.hub_path, workdir=str(Path(self._tmp) / "direct-cache"))
        found = verdict.lookup(direct, payload["tree_sha"], payload["gate_version"], payload["platform_id"])
        self.assertIsNotNone(found)
        self.assertEqual(found["result"], "PASS")

    def test_lookup_through_the_server_sees_what_store_through_the_server_wrote(self):
        args = self._args()
        hub = verdict.build_hub(args)
        payload = _good_payload(tree_sha="c" * 40)
        self.assertEqual(verdict.store(hub, payload), "created")

        # A second FallbackHub instance (a second gate run) through the
        # SAME fixture server.
        hub2 = verdict.build_hub(self._args())
        found = verdict.lookup(hub2, payload["tree_sha"], payload["gate_version"], payload["platform_id"])
        self.assertIsNotNone(found)
        self.assertEqual(found["branch"], "staging/example")

    def test_falls_back_to_direct_when_the_server_is_unreachable(self):
        """The whole point of the flag: a dead server must not stop a
        gate from caching its verdict -- `store()` through the
        `FallbackHub` must still land on the real repo directly (SPEC
        SS4.3: connection-refused is a BEFORE-SEND failure, always safe
        to retry against GitHub)."""
        self.fixture.stop_server()  # connection refused from here on

        args = self._args()
        hub = verdict.build_hub(args)
        payload = _good_payload(tree_sha="d" * 40)
        outcome = verdict.store(hub, payload)
        self.assertEqual(outcome, "created")
        self.assertTrue(hub.degraded, "FallbackHub must report degraded once its primary is down")

        found = verdict.lookup(hub, payload["tree_sha"], payload["gate_version"], payload["platform_id"])
        self.assertIsNotNone(found, "the fallen-back-to write must be readable back, still via the fallback route")
        self.assertEqual(found["result"], "PASS")

    def test_cli_store_and_lookup_wire_the_server_url_flag_through_argparse(self):
        """End-to-end through `verdict.main`, the way `gate.sh` actually
        invokes this file -- not just the Python-level `build_hub`."""
        json_file = Path(self._tmp) / "verdict.json"
        payload = _good_payload(tree_sha="e" * 40)
        import json

        json_file.write_text(json.dumps(payload))

        token_path = Path(self._tmp) / "cli-token"
        token_path.write_text(self.fixture.token)

        store_argv = [
            "store",
            "--hub-url", self.hub_path,
            "--workdir", str(self.workdir),
            "--json-file", str(json_file),
            "--server-url", self.fixture.server_url,
            "--token-file", str(token_path),
        ]
        self.assertEqual(verdict.main(store_argv), 0)

        lookup_argv = [
            "lookup",
            "--hub-url", self.hub_path,
            "--workdir", str(self.workdir),
            "--tree-sha", payload["tree_sha"],
            "--gate-version", payload["gate_version"],
            "--platform-id", payload["platform_id"],
            "--server-url", self.fixture.server_url,
            "--token-file", str(token_path),
        ]
        self.assertEqual(verdict.main(lookup_argv), 0)


if __name__ == "__main__":
    unittest.main()
