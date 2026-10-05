import importlib.util
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import Mock

spec = importlib.util.spec_from_file_location("remote_test_runner", Path(__file__).resolve().parents[1] / "test_runner.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class OracleCacheTests(unittest.TestCase):
    def test_same_lock_reuses_location_but_revalidates(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identity = root / "oracle-lock.json"
            identity.write_text("locked identity")
            bootstrap = SimpleNamespace(provision=Mock(side_effect=lambda cache: cache / "manifest.json"))
            first, _ = runner.provision_cached_oracle(bootstrap, identity, root / "cargo")
            second, _ = runner.provision_cached_oracle(bootstrap, identity, root / "cargo")
            self.assertEqual(first, second)
            self.assertEqual(bootstrap.provision.call_count, 2)
            identity.write_text("different identity")
            changed, _ = runner.provision_cached_oracle(bootstrap, identity, root / "cargo")
            self.assertNotEqual(changed, first)

    def test_verification_failure_refuses_and_releases_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identity = root / "lock"
            identity.write_text("identity")
            bootstrap = SimpleNamespace(provision=Mock(side_effect=RuntimeError("corrupt oracle")))
            with self.assertRaisesRegex(RuntimeError, "corrupt oracle"):
                runner.provision_cached_oracle(bootstrap, identity, root / "cargo")
            bootstrap.provision.side_effect = lambda cache: cache / "manifest.json"
            runner.provision_cached_oracle(bootstrap, identity, root / "cargo")
