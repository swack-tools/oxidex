"""Lightweight controls for the independently approved qualification artifact."""
import base64
import hashlib
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

from tools.release import approved_linux_perl as approved
from tools.release import bootstrap_oracle as oracle


class ApprovedLinuxPerlTests(unittest.TestCase):
    def foreign_owner(self, target):
        original = Path.lstat
        def lstat(path):
            info = original(path)
            if Path(path) == target:
                return SimpleNamespace(st_mode=info.st_mode, st_uid=os.geteuid() + 1)
            return info
        return mock.patch.object(Path, "lstat", lstat)

    def oversized_metadata(self, target, size=8 * 1024**3):
        original = Path.lstat
        def lstat(path):
            info = original(path)
            if Path(path) == target:
                return SimpleNamespace(st_mode=info.st_mode, st_uid=info.st_uid,
                                       st_size=size)
            return info
        return mock.patch.object(Path, "lstat", lstat)

    def setUp(self):
        evidence = Path.home() / "oxidex-ops/evidence/recovery/linux-perl-approved-identity-tests"
        evidence.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=evidence)
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.prefix = oracle.perl_prefix(self.root)
        self.perl = self.prefix / "bin/perl5.38.2"
        self.zip_module = self.prefix / "lib/Archive/Zip.pm"
        self.core_module = self.prefix / "lib/Config.pm"
        for path, data in ((self.perl, b"known executable"),
                           (self.zip_module, b"known zip library"),
                           (self.core_module, b"known core library")):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.perl.chmod(0o755)
        self.descriptor = self.root / "approval.json"
        self.envelope = self.root / "envelope.json"
        self.archive = b"bounded frozen archive"
        self.approval = {
            "schema": 1, "kind": "oxidex_approved_linux_perl_installation",
            "platform": "linux-x86_64", "prefix": str(self.prefix),
            "lock_sha256": approved.sha(oracle.LOCK_PATH),
            "perl_source_sha256": oracle.LOCK["archives"]["perl"]["sha256"],
            "archive_zip_source_sha256": oracle.LOCK["archives"]["archive_zip"]["sha256"],
            "archive_sha256": hashlib.sha256(self.archive).hexdigest(),
            "archive_bytes": len(self.archive),
            "tree_sha256": oracle.sha256_tree(self.prefix),
            "exe_sha256": approved.sha(self.perl),
            "zip_sha256": approved.sha(self.zip_module),
            "zip_relative_path": "lib/Archive/Zip.pm",
            "producer": {"source_head": "a" * 40, "receipt_sha256": "b" * 64,
                         "image": "reviewed image", "compiler": "reviewed compiler"},
        }
        self.descriptor.write_text(json.dumps(self.approval))
        self.envelope.write_text(json.dumps({
            "schema": 1, "kind": "oxidex_approved_linux_perl_archive",
            "archive_base64": base64.b64encode(self.archive).decode(),
        }))
        for target, value in ((approved, "QUALIFICATION_ROOT"),):
            patcher = mock.patch.object(target, value, self.root)
            patcher.start()
            self.addCleanup(patcher.stop)
        patcher = mock.patch.object(approved, "PREFIX", self.prefix)
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = mock.patch.object(oracle, "DURABLE_ROOT", self.root)
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = mock.patch.object(approved.platform, "system", return_value="Linux")
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = mock.patch.object(approved.platform, "machine", return_value="x86_64")
        patcher.start()
        self.addCleanup(patcher.stop)

    def load(self):
        return approved.load(oracle.LOCK_PATH, descriptor=self.descriptor,
                             envelope=self.envelope)

    def test_prefix_metadata_refuses_writable_or_foreign_owner_before_tree_hash(self):
        self.prefix.chmod(0o777)
        with mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree read")):
            with self.assertRaisesRegex(approved.Refused, "prefix.*mode"):
                approved.check_tree(self.approval, self.prefix, oracle.sha256_tree)
        for mode in (0o700, 0o755):
            self.prefix.chmod(mode)
            approved.check_tree(self.approval, self.prefix, oracle.sha256_tree)
        self.root.chmod(0o777)
        with mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree read")):
            with self.assertRaisesRegex(approved.Refused, "prefix.*mode"):
                approved.check_tree(self.approval, self.prefix, oracle.sha256_tree)
        self.root.chmod(0o700)
        with mock.patch.object(approved.os, "geteuid", return_value=os.geteuid() + 1):
            with mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree read")):
                with self.assertRaisesRegex(approved.Refused, "owner"):
                    approved.check_tree(self.approval, self.prefix, oracle.sha256_tree)

    def test_foreign_owned_descendants_refuse_before_tree_hash_or_probe(self):
        symlink = self.prefix / "lib/outside-link"
        symlink.symlink_to(self.root)
        for target in (self.perl, self.core_module, self.zip_module, symlink):
            with self.subTest(target=target), self.foreign_owner(target), \
                 mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")), \
                 mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")):
                with self.assertRaisesRegex(approved.UnverifiableMetadata, "foreign owner"):
                    approved.check_tree(self.approval, self.prefix, oracle.sha256_tree)

    def test_descendant_walk_is_bounded_before_hash(self):
        with mock.patch.object(approved, "MAX_TREE_ENTRIES", 1), \
             mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")):
            with self.assertRaisesRegex(approved.UnverifiableMetadata, "entry bound"):
                approved.check_tree(self.approval, self.prefix, oracle.sha256_tree)

    def test_oversized_retained_file_refuses_before_tree_hash_or_probe(self):
        with self.oversized_metadata(self.core_module), \
             mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")), \
             mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")):
            with self.assertRaisesRegex(approved.UnverifiableMetadata, "byte bound"):
                approved.check_tree(self.approval, self.prefix, oracle.sha256_tree)

    def test_aggregate_regular_bytes_refuse_before_hash(self):
        total = sum(path.stat().st_size for path in (self.perl, self.core_module, self.zip_module))
        with mock.patch.object(approved, "MAX_TREE_BYTES", total - 1), \
             mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")):
            with self.assertRaisesRegex(approved.UnverifiableMetadata, "byte bound"):
                approved.check_tree(self.approval, self.prefix, oracle.sha256_tree)

    def test_oversized_verified_staging_preserves_recovery_evidence(self):
        staging = self.prefix.with_name(".prefix.staging-999999999-oversized")
        self.prefix.rename(staging)
        sidecar = oracle.write_staging_intent(
            staging, self.prefix, 999999999, "verified", self.approval["tree_sha256"])
        before = sidecar.read_bytes()
        with self.oversized_metadata(staging / "lib/Config.pm"), \
             mock.patch.object(oracle, "_process_is_live", return_value=False), \
             mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")), \
             mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")), \
             mock.patch.object(oracle.shutil, "rmtree", side_effect=AssertionError("delete ran")), \
             mock.patch.object(oracle, "_append_staging_journal", side_effect=AssertionError("journal mutated")):
            with self.assertRaisesRegex(oracle.UnverifiableMetadata, "byte bound"):
                oracle._materialize_perl(self.root, None, None, (self.approval, self.archive))
        self.assertTrue(staging.is_dir())
        self.assertFalse(self.prefix.exists())
        self.assertEqual(sidecar.read_bytes(), before)

    def test_oversized_new_staging_refuses_caught_error_cleanup(self):
        import shutil
        shutil.rmtree(self.prefix)
        original = Path.lstat
        def oversized_staging_lstat(path):
            info = original(path)
            if Path(path).name == "Config.pm":
                return SimpleNamespace(st_mode=info.st_mode, st_uid=info.st_uid,
                                       st_size=8 * 1024**3)
            return info
        def populate(staging):
            module = staging / "lib/Config.pm"
            module.parent.mkdir(parents=True)
            module.write_bytes(b"tiny synthetic module")
            raise oracle.Refused("injected populate failure")
        with mock.patch.object(Path, "lstat", oversized_staging_lstat), \
             mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")), \
             mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")), \
             mock.patch.object(oracle.shutil, "rmtree", side_effect=AssertionError("delete ran")), \
             mock.patch.object(oracle, "_append_staging_journal", side_effect=AssertionError("journal mutated")):
            with self.assertRaisesRegex(oracle.UnverifiableMetadata, "byte bound"):
                oracle.install_immutable_tree(
                    self.root, self.prefix, populate, lambda _candidate: None,
                    preflight_candidate=oracle._preflight_perl_metadata)
        staged = [path for path in self.prefix.parent.glob(".prefix.staging-*-*") if path.is_dir()]
        self.assertEqual(len(staged), 1)
        sidecar = oracle.staging_intent_path(staged[0])
        self.assertEqual(json.loads(sidecar.read_text())["lifecycle"], "materializing")
        self.assertFalse(self.prefix.exists())

    def test_foreign_owned_staging_descendant_preserves_verified_recovery(self):
        staging = self.prefix.with_name(".prefix.staging-999999999-foreign")
        self.prefix.rename(staging)
        sidecar = oracle.write_staging_intent(
            staging, self.prefix, 999999999, "verified", self.approval["tree_sha256"])
        before = sidecar.read_bytes()
        foreign = staging / "bin/perl5.38.2"
        with self.foreign_owner(foreign), \
             mock.patch.object(oracle, "_process_is_live", return_value=False), \
             mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")), \
             mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")), \
             mock.patch.object(oracle.shutil, "rmtree", side_effect=AssertionError("delete ran")), \
             mock.patch.object(oracle, "_append_staging_journal", side_effect=AssertionError("journal mutated")):
            with self.assertRaisesRegex(oracle.UnverifiableMetadata, "foreign owner"):
                oracle._materialize_perl(self.root, None, None, (self.approval, self.archive))
        self.assertTrue(staging.is_dir())
        self.assertFalse(self.prefix.exists())
        self.assertEqual(sidecar.read_bytes(), before)

    def test_unsafe_verified_staging_refuses_recovery_without_touching_evidence(self):
        staging = self.prefix.with_name(".prefix.staging-999999999-proof")
        self.prefix.rename(staging)
        sidecar = oracle.write_staging_intent(
            staging, self.prefix, 999999999, "verified", self.approval["tree_sha256"])
        before = sidecar.read_bytes()
        staging.chmod(0o777)
        with mock.patch.object(oracle, "_process_is_live", return_value=False), \
             mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")), \
             mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")), \
             mock.patch.object(oracle.shutil, "rmtree", side_effect=AssertionError("delete ran")), \
             mock.patch.object(oracle, "_append_staging_journal", side_effect=AssertionError("journal mutated")):
            with self.assertRaisesRegex(oracle.UnverifiableMetadata, "unsafe mode"):
                oracle._materialize_perl(self.root, None, None, (self.approval, self.archive))
        self.assertTrue(staging.is_dir())
        self.assertFalse(self.prefix.exists())
        self.assertEqual(sidecar.read_bytes(), before)

    def test_unsafe_new_staging_refuses_caught_error_cleanup_without_hash(self):
        # The normal fixture prefix is absent for a cold installation.
        import shutil
        shutil.rmtree(self.prefix)
        def populate(staging):
            staging.chmod(0o777)
            raise oracle.Refused("injected populate failure")
        def verify(candidate):
            oracle._verify_perl_tree(self.root, candidate, (self.approval, self.archive))
        with mock.patch.object(oracle, "sha256_tree", side_effect=AssertionError("tree hash ran")), \
             mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")), \
             mock.patch.object(oracle.shutil, "rmtree", side_effect=AssertionError("delete ran")), \
             mock.patch.object(oracle, "_append_staging_journal", side_effect=AssertionError("journal mutated")):
            with self.assertRaisesRegex(oracle.UnverifiableMetadata, "unsafe mode"):
                oracle.install_immutable_tree(
                    self.root, self.prefix, populate, verify,
                    preflight_candidate=oracle._preflight_perl_metadata)
        staged = list(self.prefix.parent.glob(".prefix.staging-*-*"))
        staged = [path for path in staged if path.is_dir()]
        self.assertEqual(len(staged), 1)
        sidecar = oracle.staging_intent_path(staged[0])
        self.assertEqual(json.loads(sidecar.read_text())["lifecycle"], "materializing")
        self.assertFalse(self.prefix.exists())

    def test_safe_metadata_with_corrupt_content_still_cleans_dead_staging(self):
        staging = self.prefix.with_name(".prefix.staging-999999999-corrupt")
        self.prefix.rename(staging)
        oracle.write_staging_intent(staging, self.prefix, 999999999, "materializing")
        (staging / "lib/Config.pm").write_bytes(b"corrupt contents")
        def verify(candidate):
            oracle._verify_perl_tree(self.root, candidate, (self.approval, self.archive))
        with mock.patch.object(oracle, "_process_is_live", return_value=False), \
             mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")):
            self.assertIsNone(oracle._recover_abandoned_staging(self.root, self.prefix, verify))
        self.assertFalse(staging.exists())
        self.assertFalse(oracle.staging_intent_path(staging).exists())

    def test_baseline_and_joint_mutations_refuse_before_any_probe(self):
        identity = self.load()
        approved.check_tree(identity[0], self.prefix, oracle.sha256_tree)
        for path in (self.perl, self.core_module, self.zip_module):
            with self.subTest(path=path):
                original = path.read_bytes()
                path.write_bytes(original + b" altered")
                # A mutable storage manifest could be updated to match these
                # bytes; neither bootstrap entry point may reach a probe.
                manifest = oracle.manifest_path(self.root)
                manifest.parent.mkdir(parents=True, exist_ok=True)
                manifest.write_text(json.dumps({"artifacts": {
                    "perl_tree": {"sha256": oracle.sha256_tree(self.prefix)},
                    "perl_executable": {"sha256": approved.sha(self.perl)}}}))
                with mock.patch.object(oracle, "run", side_effect=AssertionError("probe ran")), \
                     mock.patch.object(oracle, "materialize", side_effect=AssertionError("materialize ran")):
                    with self.assertRaisesRegex(oracle.Refused, "whole-tree"):
                        oracle.provision(self.root, approved_perl=identity)
                    with self.assertRaisesRegex(oracle.Refused, "whole-tree"):
                        oracle.verify(self.root, oracle.VERSION, None, approved_perl=identity)
                    with self.assertRaisesRegex(oracle.Refused, "whole-tree"):
                        oracle._verify_perl_tree(self.root, self.prefix, identity)
                self.assertEqual(path.read_bytes(), original + b" altered")
                path.write_bytes(original)

    def test_cold_install_replays_exact_frozen_tree_before_probe(self):
        from tools.release import freeze_linux_perl
        archive_path = self.root / "frozen.tar.gz"
        freeze_linux_perl.freeze_tree(self.prefix, archive_path)
        identity = ({**self.approval, "prefix": str(self.root / "cold/toolchains/perl-5.38.2/prefix")},
                    archive_path.read_bytes())
        destination = self.root / "cold"
        destination.mkdir()
        expected = oracle.perl_prefix(destination)
        with mock.patch.object(oracle, "DURABLE_ROOT", destination), \
             mock.patch.object(approved, "QUALIFICATION_ROOT", destination), \
             mock.patch.object(approved, "PREFIX", expected):
            def fake_run(argv, **_kwargs):
                if "-MConfig" in argv:
                    return str(expected)
                if "-MArchive::Zip" in argv:
                    return "1.68"
                raise AssertionError("unexpected command")
            with mock.patch.object(oracle, "run", side_effect=fake_run) as probe:
                self.assertEqual(oracle._materialize_perl(
                    destination, Path("unused"), Path("unused"), identity), "installed")
                self.assertEqual(oracle._materialize_perl(
                    destination, Path("unused"), Path("unused"), identity), "reused")
                self.assertEqual(oracle.sha256_tree(expected), self.approval["tree_sha256"])
                self.assertEqual(probe.call_count, 6)

    def test_missing_approval_wrong_prefix_lock_and_envelope_refuse(self):
        with self.assertRaisesRegex(approved.Refused, "missing"):
            approved.load(oracle.LOCK_PATH, descriptor=self.root / "missing.json",
                          envelope=self.envelope)
        for field, value in (("prefix", "/cargo/other/prefix"),
                             ("lock_sha256", "0" * 64)):
            original = self.approval[field]
            self.approval[field] = value
            self.descriptor.write_text(json.dumps(self.approval))
            with self.subTest(field=field), self.assertRaisesRegex(approved.Refused, "prefix or lock"):
                self.load()
            self.approval[field] = original
        self.descriptor.write_text(json.dumps(self.approval))
        self.envelope.write_text(json.dumps({
            "schema": 1, "kind": "oxidex_approved_linux_perl_archive",
            "archive_base64": base64.b64encode(b"wrong archive").decode(),
        }))
        with self.assertRaisesRegex(approved.Refused, "size differs"):
            self.load()

    def test_unknown_root_refuses_even_with_valid_approval(self):
        identity = self.load()
        with self.assertRaisesRegex(oracle.Refused, "durable root must be"):
            oracle.assert_approved_perl(self.root / "different", identity, allow_absent=True)


if __name__ == "__main__":
    unittest.main()
