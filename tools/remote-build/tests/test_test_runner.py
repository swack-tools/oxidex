import importlib.util
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import Mock, patch

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

    def test_real_bootstrap_and_oracle_consumer_agree_on_durable_root(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT) as directory, patch.dict(runner.os.environ):
            cache = Path(directory) / "cache"
            runner.configure_oracle_environment(cache, "13.59")
            bootstrap = runner.load_oracle_bootstrap(cache)
            self.assertEqual(bootstrap.resolve_durable_root(cache), cache.resolve())
            spec = importlib.util.spec_from_file_location("scripts.test_real_python_oracle", runner.ROOT / "scripts/exiftool_oracle.py")
            oracle = importlib.util.module_from_spec(spec)
            import sys
            with patch.dict(sys.modules, {spec.name: oracle}):
                spec.loader.exec_module(oracle)
            self.assertEqual(oracle.cache_dir(), cache / "cache/exiftool/13.59")
            self.assertEqual(oracle.choose_perl(), str(cache / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2"))
            with self.assertRaises(bootstrap.Refused):
                bootstrap.resolve_durable_root(Path(directory) / "other")
            with patch.dict(runner.os.environ, {"EXIFTOOL_CACHE_DIR": str(Path(directory) / "foreign")}):
                with self.assertRaises(oracle.OracleError):
                    oracle.cache_dir()
