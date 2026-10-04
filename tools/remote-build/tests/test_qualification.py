"""Safety boundaries for the Spot qualification transport."""
import io
import json
from pathlib import Path
import subprocess
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

    def test_result_archive_refuses_path_escape_and_partial_publication(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "results.tar.gz"
            with tarfile.open(archive, "w:gz") as stream:
                payload = b"bad"
                member = tarfile.TarInfo("../escape")
                member.size = len(payload)
                stream.addfile(member, io.BytesIO(payload))
            with self.assertRaisesRegex(ValueError, "unsafe member"):
                qualification.safe_extract(archive, root / "staged")
            with tarfile.open(archive, "w:gz") as stream:
                for _ in range(2):
                    member = tarfile.TarInfo("summary.json")
                    member.size = 2
                    stream.addfile(member, io.BytesIO(b"{}"))
            with self.assertRaises(FileExistsError):
                qualification.safe_extract(archive, root / "staged")
            self.assertFalse((root / "staged").exists())

    def test_result_archive_extracts_without_newer_tar_filter_api(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "results.tar.gz"
            with tarfile.open(archive, "w:gz") as stream:
                member = tarfile.TarInfo("results/summary.json")
                member.size = 2
                stream.addfile(member, io.BytesIO(b"{}"))
            with patch.object(tarfile.TarFile, "extractall", side_effect=AssertionError("newer filter API")):
                qualification.safe_extract(archive, root / "staged")
            self.assertEqual((root / "staged/results/summary.json").read_bytes(), b"{}")
            with self.assertRaisesRegex(ValueError, "already exists"):
                qualification.safe_extract(archive, root / "staged")

    def test_results_publish_only_after_all_markers_verify(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output"
            output.mkdir()
            staged = root / "staged/output"
            staged.mkdir(parents=True)
            head = "a" * 40
            pin = "13.59"
            corpus = staged / "corpus-read/observations/receipt.json"
            corpus.parent.mkdir(parents=True)
            corpus.write_text("{}")
            rows = {}
            for index, row in enumerate(qualification.ROWS):
                marker = staged / f"spot-{head[:12]}-{index}/qualification-result.json"
                marker.parent.mkdir()
                marker.write_text("{}")
                rows[row.format(pin=pin)] = {"marker_sha256": qualification.sha(marker)}
            summary = {"status": "PASS", "head": head, "pin": pin, "corpus_gate": "PASS",
                       "corpus_receipt_sha256": qualification.sha(corpus),
                       "selected_file_floor": 1, "rows": rows}
            (staged / "remote-qualification.json").write_text(json.dumps(summary))
            archive_sha = "b" * 64
            replay = {"schema": 1, "kind": "oxidex_remote_corpus_archive_replay",
                      "status": "PASS", "head": head, "pin": pin,
                      "archive_sha256": archive_sha,
                      "corpus_receipt_sha256": qualification.sha(corpus), "corpus_files": 1}
            with self.assertRaisesRegex(RuntimeError, "not a replayed PASS"):
                qualification.publish_results(staged, output, head, pin, replay, archive_sha)
            self.assertFalse((output / "remote-results").exists())
            corpus.write_text(json.dumps({
                "schema": "oxidex_corpus_read_receipt_v2", "instrument": "corpus_read_receipt.py",
                "producer": {"source_commit": head, "source_dirty": False},
                "build_proof": {"snapshot": {"source_commit": head}},
                "native": {"exiftool_version": pin},
                "corpus": {"files": {"fixture.jpg": "0" * 64}},
                "metric_c": {"corpus_files": 1}, "observations": [{}, {}],
                "sources": {"fixture.jpg": {}}}))
            summary["corpus_receipt_sha256"] = qualification.sha(corpus)
            replay["corpus_receipt_sha256"] = qualification.sha(corpus)
            (staged / "remote-qualification.json").write_text(json.dumps(summary))
            missing = staged / f"spot-{head[:12]}-0/qualification-result.json"
            missing.unlink()
            with self.assertRaisesRegex(RuntimeError, "marker differs"):
                qualification.publish_results(staged, output, head, pin, replay, archive_sha)
            self.assertFalse((output / "remote-results").exists())
            missing.write_text("{}")
            with self.assertRaisesRegex(RuntimeError, "not committed evidence"):
                qualification.publish_results(staged, output, head, pin, replay, archive_sha)
            self.assertFalse((output / "remote-results").exists())
            with patch.object(qualification, "validate_downloaded_marker") as validate:
                published = qualification.publish_results(staged, output, head, pin, replay, archive_sha)
            self.assertEqual(validate.call_count, 3)
            self.assertEqual(published, output / "remote-results")
            self.assertFalse(staged.exists())
            self.assertTrue((published / "remote-qualification.json").is_file())

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

    def test_invalid_timeout_refuses_before_any_remote_launch(self):
        for invalid in ("not-a-number", "0", "-1"):
            with self.subTest(invalid=invalid), \
                 patch.dict("os.environ", {"OXIDEX_REMOTE_QUALIFICATION_TIMEOUT_SECONDS": invalid}), \
                 patch.object(qualification, "exact_inputs") as inputs, \
                 patch.object(qualification, "select_worker") as select:
                with self.assertRaisesRegex(ValueError, "positive integer"):
                    qualification.controller(Path("/missing"), "project")
                inputs.assert_not_called()
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
            original_addfile = tarfile.TarFile.addfile
            def mutate_after_addfile(archive, member, *args, **kwargs):
                result = original_addfile(archive, member, *args, **kwargs)
                if member.name.endswith("fixture.bin"):
                    fixture.write_bytes(b"changed")
                return result
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "exact_inputs", return_value=("a" * 40, "13.59", [policy, provisioned])), \
                 patch.object(qualification, "git", return_value=str(signer)), \
                 patch.object(tarfile.TarFile, "addfile", new=mutate_after_addfile):
                with self.assertRaisesRegex(ValueError, "changed while creating"):
                    qualification.prepare_archive(output, destination, bundle)
            self.assertFalse(destination.exists())

    def test_archive_uses_frozen_input_even_if_live_file_changes_during_add(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            provisioned = output / "provisioned"
            provisioned.mkdir(parents=True)
            policy = output / "read-policy-input.json"
            policy.write_text("original")
            bundle = root / "repository.bundle"
            bundle.write_bytes(b"bundle")
            signer = root / "allowed_signers"
            signer.write_text("swackhamer key\n")
            destination = root / "input.tar.gz"
            original_addfile = tarfile.TarFile.addfile
            def change_live_during_add(archive, member, *args, **kwargs):
                if member.name.endswith("read-policy-input.json"):
                    policy.write_text("transient")
                    try:
                        return original_addfile(archive, member, *args, **kwargs)
                    finally:
                        policy.write_text("original")
                return original_addfile(archive, member, *args, **kwargs)
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "exact_inputs", return_value=("a" * 40, "13.59", [policy, provisioned])), \
                 patch.object(qualification, "git", return_value=str(signer)), \
                 patch.object(tarfile.TarFile, "addfile", new=change_live_during_add):
                qualification.prepare_archive(output, destination, bundle)
            with tarfile.open(destination) as stream:
                self.assertEqual(stream.extractfile("ops/output/read-policy-input.json").read(), b"original")

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

    def test_worker_inventory_command_failure_records_blocked_transport(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            output.mkdir()
            def package(_output, destination, _bundle):
                destination.write_bytes(b"package")
                return "a" * 40, "13.59"
            def git(*args):
                return "" if args[0] == "status" else "a" * 40
            failure = subprocess.CalledProcessError(1, ["gcloud", "compute", "instances", "list"])
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "exact_inputs", return_value=("a" * 40, "13.59", [])), \
                 patch.object(qualification, "git", side_effect=git), \
                 patch.object(qualification, "prepare_archive", side_effect=package), \
                 patch.object(qualification, "resolve_project", return_value="project"), \
                 patch.object(qualification, "select_worker", side_effect=failure), \
                 patch.object(qualification.subprocess, "run"):
                self.assertEqual(qualification.controller(output, "project"), 2)
            transports = list((root / "evidence/remote-qualification").glob("*/transport.json"))
            self.assertEqual(len(transports), 1)
            receipt = json.loads(transports[0].read_text())
            self.assertEqual(receipt["status"], "BLOCKED_WORKER_SELECTION")
            self.assertIn("gcloud", receipt["error"])

    def test_task19_unknown_and_held_lease_exits_retain_lineage(self):
        for code in (4, 5):
            with self.subTest(code=code):
                result = {"status": "pending"}
                receipt = Path("/owned/spot-head-0")
                self.assertEqual(qualification.task19_row_exit(code, "same-pin-13.59", receipt, result), code)
                self.assertEqual(result["status"], "RUNNING_RETAINED")
                self.assertEqual(result["unconfirmed_row"]["receipt_root"], str(receipt))
                self.assertEqual(result["unconfirmed_row"]["exit_code"], code)
                transport = {"status": "running", "remote_exit_code": code}
                with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
                    qualification.require_remote_success(transport)
                self.assertEqual(transport["status"], "RUNNING_RETAINED")
                self.assertEqual(transport["unconfirmed_task19_exit"], code)
        with self.assertRaisesRegex(ValueError, "exit 2"):
            qualification.task19_row_exit(2, "same-pin-13.59", Path("/owned/spot-head-0"), {})

    def test_detached_exit_status_is_retained_unless_complete(self):
        for status in ("", "partial", "1x", "256", "-1"):
            with self.subTest(status=status):
                receipt = {"status": "running"}
                with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
                    qualification.confirmed_exit_status(status, receipt)
                self.assertEqual(receipt["status"], "RUNNING_RETAINED")
        self.assertEqual(qualification.confirmed_exit_status("0", {}), 0)
        self.assertEqual(qualification.confirmed_exit_status("5", {}), 5)

    def test_spot_archive_replay_rejects_malformed_corpus_bytes(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            receipt = output / "corpus-read/observations/receipt.json"
            receipt.parent.mkdir(parents=True)
            receipt.write_text("{}")
            archive = root / "results.tar.gz"
            with tarfile.open(archive, "w:gz") as stream:
                stream.add(receipt, arcname="output/corpus-read/observations/receipt.json")
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "git", side_effect=["a" * 40, ""]):
                with self.assertRaisesRegex(ValueError, "measures another source"):
                    qualification.replay_archived_corpus(archive, output, "a" * 40)

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
