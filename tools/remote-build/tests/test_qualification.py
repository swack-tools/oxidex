"""Safety boundaries for the Spot qualification transport."""
import io
import json
from pathlib import Path
import tarfile
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import qualification


class QualificationTransportTests(unittest.TestCase):
    def test_input_closure_includes_manifest_fixture_and_rejects_symlink(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            provisioned = root / "provisioned"
            provisioned.mkdir()
            fixture = root / "cache" / "fixture.jpg"
            fixture.parent.mkdir()
            fixture.write_bytes(b"fixture")
            (provisioned / "read.json").write_text(json.dumps({"path": str(fixture)}))
            paths = qualification.input_paths([provisioned], root)
            self.assertEqual(set(paths), {fixture, provisioned / "read.json"})
            alias = root / "alias"
            alias.symlink_to(fixture)
            with self.assertRaisesRegex(ValueError, "symlink"):
                qualification.input_paths([alias], root)
            external = root.parent / "external-fixture.jpg"
            (provisioned / "external.json").write_text(json.dumps({"fixture": str(external)}))
            with self.assertRaisesRegex(ValueError, "outside OXIDEX_OPS_DIR"):
                qualification.input_paths([provisioned], root)

    def test_signer_symlink_is_archived_as_readable_file(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            provisioned = output / "provisioned"
            provisioned.mkdir(parents=True)
            policy = output / "read-policy-input.json"
            policy.write_text("{}")
            bundle = root / "repository.bundle"
            bundle.write_bytes(b"bundle")
            signer = root / "signer-real"
            signer.write_text("swackhamer key\n")
            signer.chmod(0o600)
            link = root / "signer-link"
            link.symlink_to(signer)
            destination = root / "input.tar.gz"
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "exact_inputs", return_value=("a" * 40, "13.59", [policy, provisioned])), \
                 patch.object(qualification, "git", return_value=str(link)):
                qualification.prepare_archive(output, destination, bundle)
            with tarfile.open(destination) as stream:
                member = stream.getmember("maintainer.allowed_signers")
                self.assertTrue(member.isfile())
                self.assertEqual(member.mode, 0o644)
                self.assertEqual(stream.extractfile(member).read(), b"swackhamer key\n")

    def test_result_archive_refuses_path_escape_and_overwrite(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "results.tar.gz"
            with tarfile.open(archive, "w:gz") as stream:
                payload = b"bad"
                member = tarfile.TarInfo("../escape")
                member.size = len(payload)
                stream.addfile(member, io.BytesIO(payload))
            with self.assertRaisesRegex(ValueError, "unsafe member"):
                qualification.safe_extract(archive, root)
            with tarfile.open(archive, "w:gz") as stream:
                member = tarfile.TarInfo("existing.json")
                member.size = 2
                stream.addfile(member, io.BytesIO(b"{}"))
            (root / "existing.json").write_text("original")
            with self.assertRaisesRegex(ValueError, "replace existing evidence"):
                qualification.safe_extract(archive, root)
            self.assertEqual((root / "existing.json").read_text(), "original")

    def test_project_environment_is_used_before_gcloud(self):
        with patch.dict("os.environ", {"OXIDEX_REMOTE_PROJECT": "spot-project"}), \
             patch.object(qualification.subprocess, "check_output") as default_project:
            self.assertEqual(qualification.resolve_project(None), "spot-project")
            self.assertEqual(qualification.resolve_project("explicit-project"), "explicit-project")
            default_project.assert_not_called()

    def test_stale_inputs_refuse_before_worker_selection(self):
        with patch.object(qualification, "exact_inputs", side_effect=ValueError("stale plan")), \
             patch.object(qualification, "select_worker") as select:
            with self.assertRaisesRegex(ValueError, "stale plan"):
                qualification.controller(Path("/missing"), "project")
            select.assert_not_called()

    def test_archive_refuses_input_changed_after_validation(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            provisioned = output / "provisioned"
            provisioned.mkdir(parents=True)
            policy = output / "read-policy-input.json"
            policy.write_text("{}")
            fixture = provisioned / "fixture.bin"
            fixture.write_bytes(b"original")
            bundle = root / "repository.bundle"
            bundle.write_bytes(b"bundle")
            signer = root / "allowed_signers"
            signer.write_text("swackhamer key\n")
            destination = root / "input.tar.gz"
            original_add = tarfile.TarFile.add
            def mutate_after_add(archive, name, *args, **kwargs):
                result = original_add(archive, name, *args, **kwargs)
                if Path(name) == fixture:
                    fixture.write_bytes(b"changed")
                return result
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "exact_inputs", return_value=("a" * 40, "13.59", [policy, provisioned])), \
                 patch.object(qualification, "git", return_value=str(signer)), \
                 patch.object(tarfile.TarFile, "add", new=mutate_after_add):
                with self.assertRaisesRegex(ValueError, "changed while creating"):
                    qualification.prepare_archive(output, destination, bundle)
            self.assertFalse(destination.exists())

    def test_unavailable_worker_records_blocked_transport(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            output.mkdir()
            def package(_output, destination, _bundle):
                destination.write_bytes(b"package")
                return "a" * 40, "13.59"
            def git(*args):
                return "" if args[0] == "status" else "a" * 40
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "exact_inputs", return_value=("a" * 40, "13.59", [])), \
                 patch.object(qualification, "git", side_effect=git), \
                 patch.object(qualification, "prepare_archive", side_effect=package), \
                 patch.object(qualification, "resolve_project", return_value="project"), \
                 patch.object(qualification, "select_worker", side_effect=RuntimeError("no eligible Spot worker")), \
                 patch.object(qualification.subprocess, "run"):
                self.assertEqual(qualification.controller(output, "project"), 2)
            transports = list((root / "evidence/remote-qualification").glob("*/transport.json"))
            self.assertEqual(len(transports), 1)
            receipt = json.loads(transports[0].read_text())
            self.assertEqual(receipt["status"], "BLOCKED_NO_WORKER")
            self.assertIn("no eligible Spot worker", receipt["error"])
            self.assertEqual(receipt["head"], "a" * 40)

class CorpusCommandContractTests(unittest.TestCase):
    def test_nested_proof_and_receipt_paths_follow_instrument_outputs(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            commands = qualification.corpus_commands(root, Path("instrument.py"), Path("perl"),
                                                     Path("exiftool"), Path("samples"), 193)
            self.assertEqual(commands[0][-1], str(root / "build"))
            self.assertEqual(commands[1][commands[1].index("--build-proof") + 1],
                             str(root / "build" / "build-proof.json"))
            self.assertEqual(commands[1][commands[1].index("--output") + 1],
                             str(root / "observations"))
            for command in commands[2:]:
                self.assertEqual(command[command.index("--receipt") + 1],
                                 str(root / "observations" / "receipt.json"))


if __name__ == "__main__":
    unittest.main()
