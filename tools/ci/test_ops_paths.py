"""All release entry points agree on a portable, durable operational root."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from tools.release import fleet_controller


REPO = Path(__file__).resolve().parents[2]


class OperationalRootTests(unittest.TestCase):
    def roots(self, **overrides):
        environment = {key: value for key, value in os.environ.items()
                       if key not in ("OXIDEX_OPS_DIR", "OXIDEX_WORKTREE_ROOT", "OXIDEX_TARGET_ROOT",
                                      "EXIFTOOL_CACHE_DIR", "EXIFTOOL_PERL")}
        environment.update(overrides)
        return subprocess.run(
            [sys.executable, "-c", "import json; from tools.release import bootstrap_oracle as b; "
             "from scripts import exiftool_oracle as e; from tools.ci import release_oracle as r; "
             "from tools.release import fleet_controller as f; "
             "print(json.dumps([str(b.DURABLE_ROOT), str(e.DURABLE_ROOT), str(r.DEFAULT_CACHE_ROOT), "
             "str(f.GIT_ROOT), str(f.TARGET_BASE)]))"],
            cwd=REPO, env=environment, text=True, capture_output=True,
        )

    def test_default_uses_current_users_home(self):
        result = self.roots(HOME="/opt/portable-developer")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [
            "/opt/portable-developer/oxidex-ops",
            "/opt/portable-developer/oxidex-ops",
            "/opt/portable-developer/oxidex-ops/cache/exiftool",
            "/opt/portable-developer/git",
            "/opt/portable-developer/git/oxidex-beta1-targets",
        ])

    def test_explicit_durable_override_is_shared(self):
        result = self.roots(OXIDEX_OPS_DIR="/srv/oxidex operations",
                            OXIDEX_WORKTREE_ROOT="/srv/oxidex worktrees",
                            OXIDEX_TARGET_ROOT="/srv/oxidex targets")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [
            "/srv/oxidex operations", "/srv/oxidex operations",
            "/srv/oxidex operations/cache/exiftool",
            "/srv/oxidex worktrees", "/srv/oxidex targets",
        ])

    def test_ephemeral_override_is_rejected(self):
        for variable in ("OXIDEX_OPS_DIR", "OXIDEX_WORKTREE_ROOT", "OXIDEX_TARGET_ROOT"):
            for path in ("/tmp/oxidex-ops", "/private/tmp/oxidex-ops", "relative-ops"):
                with self.subTest(variable=variable, path=path):
                    result = self.roots(**{variable: path})
                    self.assertNotEqual(result.returncode, 0)

    def test_cache_version_follows_the_repository_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            (repo / "scripts").mkdir()
            helper = repo / "scripts/ops_paths.py"
            helper.write_bytes((REPO / "scripts/ops_paths.py").read_bytes())
            (repo / ".exiftool-version").write_text("99.42\n")
            result = subprocess.run(
                [sys.executable, str(helper), "--cache"], text=True, capture_output=True,
                env={**os.environ, "OXIDEX_OPS_DIR": "/srv/portable-ops"}, check=True,
            )
            self.assertEqual(result.stdout.strip(), "/srv/portable-ops/cache/exiftool/99.42")

    def test_launch_instructions_keep_spaced_roots_as_single_arguments(self):
        with mock.patch.object(fleet_controller, "CONTROLLER_BASE", Path("/srv/operations root/controller")), \
                mock.patch.object(fleet_controller, "GIT_ROOT", Path("/srv/worktree root")):
            _language, command = fleet_controller._launch_instruction({"worker": {"kind": "CLI"}, "number": 5})
        argv = shlex.split(command)
        self.assertEqual(argv[argv.index("--root") + 1], "/srv/operations root/controller")
        self.assertEqual(argv[argv.index("--repo") + 1], "/srv/worktree root/oxidex-beta1-functional-integration")


if __name__ == "__main__":
    unittest.main()
