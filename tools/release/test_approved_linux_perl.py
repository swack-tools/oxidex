"""Lightweight controls for the independently approved qualification artifact."""
import base64
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from tools.release import approved_linux_perl as approved
from tools.release import bootstrap_oracle as oracle


class ApprovedLinuxPerlTests(unittest.TestCase):
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
                oracle._materialize_perl(destination, Path("unused"), Path("unused"), identity)
                self.assertEqual(oracle.sha256_tree(expected), self.approval["tree_sha256"])
                self.assertEqual(probe.call_count, 4)

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
