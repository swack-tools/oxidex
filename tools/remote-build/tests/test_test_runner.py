import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("remote_test_runner", Path(__file__).resolve().parents[1] / "test_runner.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class OracleCacheTests(unittest.TestCase):
    def test_generic_recipe_inherits_only_locked_verified_oracle(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, clear=True):
            root = Path(directory)
            (root / "tools/release").mkdir(parents=True)
            lock = root / "tools/release/oracle-lock.json"
            lock.write_text("locked identity")
            (root / ".exiftool-version").write_text("13.59\n")
            cargo_home = root / "cargo"
            os.environ["CARGO_HOME"] = str(cargo_home)
            os.environ["EXIFTOOL"] = "/unverified/exiftool"
            os.environ["OXIDEX_ALLOW_EXIFTOOL_SKEW"] = "1"
            observed = []

            def provision(cache):
                observed.append((cache, dict(os.environ)))
                manifest = cache / "manifest.json"
                manifest.write_text(json.dumps({
                    "lock_sha256": runner.file_sha(lock),
                    "artifacts": {
                        "perl_executable": {"sha256": "perl"},
                        "exiftool_tree": {"sha256": "tree"},
                    },
                    "probes": {"docx": "DOCX"},
                }))
                return manifest

            with patch.object(runner, "ROOT", root), \
                 patch.object(runner, "load_oracle_bootstrap", return_value=SimpleNamespace(provision=provision)):
                manifest = runner.prepare_generic_recipe_oracle()
            cache, inherited = observed[0]
            self.assertEqual(cache, runner.oracle_cache_root(lock, cargo_home))
            self.assertEqual(manifest, cache / "manifest.json")
            self.assertEqual(inherited["OXIDEX_OPS_DIR"], str(cache))
            self.assertEqual(inherited["EXIFTOOL_CACHE_DIR"], str(cache / "cache/exiftool/13.59"))
            self.assertEqual(inherited["EXIFTOOL_PERL"], str(cache / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2"))
            self.assertEqual(inherited["OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES"], "1")
            self.assertNotIn("EXIFTOOL", inherited)
            self.assertNotIn("OXIDEX_ALLOW_EXIFTOOL_SKEW", inherited)

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
