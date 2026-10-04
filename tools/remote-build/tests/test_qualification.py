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
