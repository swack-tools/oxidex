"""Safety boundaries for the Spot qualification transport."""
import hashlib
from contextlib import ExitStack
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import qualification
sys.path.insert(0, str(qualification.ROOT))


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
                 patch.object(qualification, "git", return_value=str(link)), \
                 patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}):
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
            staged = root / "staged/output"
            staged.mkdir(parents=True)
            head = "a" * 40
            pin = "13.59"
            policy = staged / "read-policy-input.json"
            policy.write_text("{}")
            preparation = staged / "preparation.json"
            preparation.write_text(json.dumps({"head": head, "pin": pin,
                "policy_sha256": qualification.sha(policy),
                "policy_reference_sha256": "d" * 64}))
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
                       "selected_file_floor": 1, "rows": rows,
                       "input_archive_sha256": "c" * 64, "input_file_count": 1,
                       "preparation_sha256": qualification.sha(preparation),
                       "reference_policy_sha256": "d" * 64}
            (staged / "remote-qualification.json").write_text(json.dumps(summary))
            archive_sha = "b" * 64
            replay = {"schema": 1, "kind": "oxidex_remote_corpus_archive_replay",
                      "status": "PASS", "head": head, "pin": pin,
                      "archive_sha256": archive_sha,
                      "corpus_receipt_sha256": qualification.sha(corpus), "corpus_files": 1,
                      "task19_markers": {row: item["marker_sha256"] for row, item in rows.items()}}
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
            with patch.object(qualification, "validate_downloaded_marker"):
                with self.assertRaisesRegex(RuntimeError, "loader replay binding"):
                    qualification.publish_results(staged, output, head, pin, replay, archive_sha)
            summary["committed_loader_replay"] = {
                "schema": 1, "kind": "oxidex_spot_task19_loader_replay", "status": "PASS",
                "head": head, "pin": pin,
                "matrix_sha256": qualification.sha(qualification.qualification_module().CANONICAL_MATRIX),
                "target_root": "/target", "lease_path": str(output / "transition.host.lock"),
                "rows": {row: item["marker_sha256"] for row, item in rows.items()}}
            (staged / "remote-qualification.json").write_text(json.dumps(summary))
            with patch.object(qualification, "validate_downloaded_marker") as validate:
                published = qualification.publish_results(staged, output, head, pin, replay, archive_sha)
            self.assertEqual(validate.call_count, 3)
            self.assertEqual(published, output / "remote-results")
            self.assertFalse(staged.exists())
            self.assertTrue((published / "remote-qualification.json").is_file())

    def test_published_receipt_verifier_binds_archive_and_retained_spot_paths(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            (output / "remote-results").mkdir(parents=True)
            evidence = root / "evidence" / "remote-qualification" / "run"
            evidence.mkdir(parents=True)
            archive = evidence / "results.tar.gz"
            names = ["remote-qualification.json", "source-identity.json", "corpus-read/observations/receipt.json"]
            names += [f"spot-{'a' * 12}-{index}/qualification-result.json" for index in range(3)]
            names += ["spot-" + "a" * 12 + "-0/same-pin-13.59/before/execution-status.json"]
            for name in names:
                path = output / "remote-results" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                if name == "remote-qualification.json":
                    path.write_text(json.dumps({"input_archive_sha256": "c" * 64,
                                                "source_identity": {"mode": "maintainer-ssh"},
                                                "input_file_count": 6}))
                elif name == "source-identity.json":
                    path.write_text(json.dumps({"mode": "maintainer-ssh"}))
                else:
                    path.write_text(name)
            with tarfile.open(archive, "w:gz") as source:
                for name in names:
                    source.add(output / "remote-results" / name, arcname="evidence/remote-qualification/qualification-aaaaaaaaaaaa-" + "b" * 32 + "/" + name)
            signer = evidence / "maintainer.allowed_signers"
            signer.write_text("swackhamer key")
            bundle = evidence / "repository.bundle"
            bundle.write_bytes(b"source bundle")
            head, stage = "a" * 40, "/target/ops/evidence/remote-qualification/qualification-aaaaaaaaaaaa-" + "b" * 32
            record = {"status": "PASS", "head": head, "pin": "13.59",
                      "source_identity": {"mode": "maintainer-ssh"},
                      "run_id": "qualification-aaaaaaaaaaaa-" + "b" * 32, "remote_output": stage,
                      "instance": "spot", "instance_id": "123", "zone": "zone-a",
                      "results_dir": str(output / "remote-results"),
                      "results_sha256": qualification.sha(archive),
                      "source_bundle_sha256": qualification.sha(bundle),
                      "signers_sha256": qualification.sha(signer),
                      "input_archive_sha256": "c" * 64, "input_file_count": 6,
                      "corpus_archive_replay": {"status": "PASS"},
                      "retained_remote_recheck": {
                          "status": "RETAINED_AT_PUBLICATION", "instance": "spot",
                          "instance_id": "123", "zone": "zone-a",
                          "source_path": "/mnt/runner-data/remote-build/sources/qualification-aaaaaaaaaaaa-" + "b" * 32,
                          "target_path": "/mnt/runner-data/remote-build/targets/qualification-aaaaaaaaaaaa-" + "b" * 32,
                          "output_path": stage,
                          "lease_path": stage + "/transition.host.lock",
                          "input_archive_path": stage + "/spot-input.tar.gz",
                          "input_archive_sha256": "c" * 64,
                          "portable_loader_replay": False}}
            transport = evidence / "transport.json"
            transport.write_text(json.dumps(record))
            with (
                patch.object(qualification, "ops_root", return_value=root),
                patch.object(qualification, "git", side_effect=[head, ""]),
                patch.object(qualification, "verify_result_tree") as verify,
                patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}),
            ):
                result = qualification.verify_published(output, transport)
            self.assertEqual(result["status"], "PASS")
            verify.assert_called_once()
            record["retained_remote_recheck"]["source_path"] = "/foreign/source"
            transport.write_text(json.dumps(record))
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "git", side_effect=[head, ""]), \
                 patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}):
                with self.assertRaisesRegex(RuntimeError, "retained source"):
                    qualification.verify_published(output, transport)
            record["retained_remote_recheck"]["source_path"] = "/mnt/runner-data/remote-build/sources/qualification-aaaaaaaaaaaa-" + "b" * 32
            transport.write_text(json.dumps(record))
            summary = output / "remote-results/spot-aaaaaaaaaaaa-0/same-pin-13.59/before/execution-status.json"
            summary.write_text("changed")
            with (
                patch.object(qualification, "ops_root", return_value=root),
                patch.object(qualification, "git", side_effect=[head, ""]),
                patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}),
            ):
                with self.assertRaisesRegex(RuntimeError, "published evidence differs"):
                    qualification.verify_published(output, transport)
            summary.write_text(names[-1])
            archive.write_bytes(b"tampered")
            with (
                patch.object(qualification, "ops_root", return_value=root),
                patch.object(qualification, "git", side_effect=[head, ""]),
            ):
                with self.assertRaisesRegex(RuntimeError, "digest differs"):
                    qualification.verify_published(output, transport)

    def test_project_environment_is_used_before_gcloud(self):
        with patch.dict("os.environ", {"OXIDEX_REMOTE_PROJECT": "spot-project"}), \
             patch.object(qualification.subprocess, "check_output") as default_project:
            self.assertEqual(qualification.resolve_project(None), "spot-project")
            self.assertEqual(qualification.resolve_project("explicit-project"), "explicit-project")
            default_project.assert_not_called()

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
                 patch.object(tarfile.TarFile, "addfile", new=mutate_after_addfile), \
                 patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}):
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
                 patch.object(tarfile.TarFile, "addfile", new=change_live_during_add), \
                 patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}):
                qualification.prepare_archive(output, destination, bundle)
            with tarfile.open(destination) as stream:
                self.assertEqual(stream.extractfile("ops/output/read-policy-input.json").read(), b"original")

    def test_task19_unknown_and_held_lease_exits_retain_lineage(self):
        for code, transport_code in ((4, 4), (5, 5), (130, 5), (137, 5), (-9, 5), (1, 5)):
            with self.subTest(code=code):
                result = {"status": "pending"}
                receipt = Path("/owned/spot-head-0")
                self.assertEqual(qualification.task19_row_exit(code, "same-pin-13.59", receipt, result),
                                 transport_code)
                self.assertEqual(result["status"], "RUNNING_RETAINED")
                self.assertEqual(result["unconfirmed_row"]["receipt_root"], str(receipt))
                self.assertEqual(result["unconfirmed_row"]["exit_code"], code)
                transport = {"status": "running", "remote_exit_code": transport_code}
                with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
                    qualification.require_remote_success(transport)
                self.assertEqual(transport["status"], "RUNNING_RETAINED")
                self.assertEqual(transport["unconfirmed_task19_exit"], transport_code)
        with self.assertRaisesRegex(ValueError, "exit 2"):
            qualification.task19_row_exit(2, "same-pin-13.59", Path("/owned/spot-head-0"), {})

    def test_container_kill_retains_unconfirmed_task19_state(self):
        for code in (1, 125, 137, 143):
            with self.subTest(code=code):
                transport = {"status": "running", "remote_exit_code": code}
                with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
                    qualification.require_remote_success(transport)
                self.assertEqual(transport["status"], "RUNNING_RETAINED")
                self.assertEqual(transport["unconfirmed_task19_exit"], code)

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
                 patch.object(qualification, "git", side_effect=["a" * 40, ""]), \
                 patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}), \
                 patch.object(qualification, "source_signers", return_value=Path("/unused")), \
                 patch.object(qualification, "verify_source_result_binding"), \
                 patch.object(qualification, "verify_archived_source_members"):
                with self.assertRaisesRegex(ValueError, "measures another source"):
                    qualification.replay_archived_corpus(archive, output, "a" * 40)

    def test_spot_archive_gate_reads_frozen_bytes_if_live_receipt_changes(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            receipt = output / "corpus-read/observations/receipt.json"
            receipt.parent.mkdir(parents=True)
            head = "a" * 40
            original = {"producer": {"source_commit": head}}
            receipt.write_text(json.dumps(original))
            archive = root / "results.tar.gz"
            marker_rows = {}
            for index, template in enumerate(qualification.ROWS):
                row = template.format(pin="13.59")
                marker = output / f"spot-{head[:12]}-{index}/qualification-result.json"
                marker.parent.mkdir(parents=True)
                marker.write_text(json.dumps({"row": row}))
                marker_rows[row] = marker
            with tarfile.open(archive, "w:gz") as stream:
                stream.add(receipt, arcname="output/corpus-read/observations/receipt.json")
                for marker in marker_rows.values():
                    stream.add(marker, arcname=str(marker.relative_to(root)))
            def measure(frozen, *_args):
                receipt.write_text("{}")
                self.assertEqual(json.loads(frozen.read_text()), original)
                return SimpleNamespace(corpus_files=1), SimpleNamespace(status="PASS"), original
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "git", side_effect=[head, ""]), \
                 patch.dict(sys.modules, {"read_regression_gate": SimpleNamespace(measure=measure)}), \
                 patch.object(qualification, "source_signers", return_value=Path("/unused")), \
                 patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}), \
                 patch.object(qualification, "verify_source_result_binding"), \
                 patch.object(qualification, "verify_archived_source_members"), \
                 patch.object(qualification, "qualification_module", return_value=SimpleNamespace(
                     load_committed_result=lambda marker: {"caller": {"head": head},
                         "rows": [{"id": json.loads(marker.read_text())["row"]}]})):
                replay = qualification.replay_archived_corpus(archive, output, head)
            self.assertEqual(replay["status"], "PASS")
            self.assertEqual(replay["corpus_receipt_sha256"],
                             hashlib.sha256(json.dumps(original).encode()).hexdigest())

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

class RestrictedPreparationTests(unittest.TestCase):
    def test_committed_canonical_write_carrier_is_copied_into_new_output(self):
        import qualification_prepare as prepare
        with TemporaryDirectory() as directory:
            output = Path(directory)
            row = prepare.canonical_write_carrier(output)
            self.assertEqual(row, {"path": str(output / "write-cohort/tag_matrix_base.jpg"),
                                   "sha256": prepare.WRITE_COHORT_SHA256, "bytes": 771})
            self.assertEqual(prepare.sha(Path(row["path"])), prepare.WRITE_COHORT_SHA256)
            with self.assertRaisesRegex(ValueError, "already exists"):
                prepare.canonical_write_carrier(output)

    def test_write_rebinding_uses_canonical_copy_and_checks_approved_source(self):
        import qualification_prepare as prepare
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / "source"
            images = source / "t/images"
            images.mkdir(parents=True)
            old = images / "ExifTool.jpg"
            old.write_bytes(b"\xff\xd8source")
            reference = root / "old-write.json"
            row = {"path": "/old/t/images/ExifTool.jpg", "sha256": prepare.sha(old),
                   "bytes": old.stat().st_size}
            reference.write_text(json.dumps({"schema": 1, "kind": prepare.WRITE_KIND,
                                             "fixtures": [row]}))
            canonical = prepare.canonical_write_carrier(root / "output")
            destination = root / "output/write-fixtures.json"
            proof = prepare.rebind_write_manifest(reference, destination, source, canonical)
            self.assertEqual(proof["count"], 1)
            self.assertEqual(json.loads(destination.read_text())["fixtures"], [canonical])
            self.assertIn(Path(canonical["path"]), qualification.input_paths([destination], root))
            self.assertNotEqual(canonical["sha256"], row["sha256"])
            old.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "differs"):
                prepare.rebind_write_manifest(reference, root / "output/other-write.json", source, canonical)
            self.assertFalse((root / "output/other-write.json").exists())
            reference.write_text(json.dumps({"schema": 1, "kind": prepare.WRITE_KIND,
                                             "fixtures": [row, row]}))
            with self.assertRaisesRegex(ValueError, "exactly one carrier"):
                prepare.rebind_write_manifest(reference, root / "output/extra-write.json", source, canonical)

    def test_ordered_selection_rejects_same_count_different_content(self):
        import qualification_prepare as prepare
        rows = [{"path": "/old/t/images/a.jpg", "sha256": "a" * 64, "bytes": 12},
                {"path": "/old/t/images/b.jpg", "sha256": "b" * 64, "bytes": 13}]
        document = {"schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest", "fixtures": rows}
        expected = prepare.logical_selection(document)
        self.assertEqual([name for name, _, _ in expected], ["a.jpg", "b.jpg"])
        changed = {**document, "fixtures": [rows[0], {**rows[1], "sha256": "c" * 64}]}
        self.assertNotEqual(prepare.logical_selection(changed), expected)
        changed = {**document, "fixtures": list(reversed(rows))}
        self.assertNotEqual(prepare.logical_selection(changed), expected)

    def test_rebinding_rejects_escape_and_content_change(self):
        import qualification_prepare as prepare
        with TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            images = source / "t/images"
            images.mkdir(parents=True)
            carrier = images / "a.jpg"
            carrier.write_bytes(b"approved")
            row = {"path": "/old/t/images/a.jpg", "sha256": prepare.sha(carrier), "bytes": 8}
            self.assertEqual(prepare.rebind_carrier(row, source)["path"], str(carrier.resolve()))
            with self.assertRaisesRegex(ValueError, "escapes"):
                prepare.rebind_carrier({**row, "path": "/old/t/images/../outside.jpg"}, source)
            with self.assertRaisesRegex(ValueError, "differs"):
                prepare.rebind_carrier({**row, "sha256": "0" * 64}, source)

    def test_missing_source_approval_refuses_before_preparation_output(self):
        import qualification_prepare as prepare
        from tools.release import approved_linux_perl
        with TemporaryDirectory() as directory, \
             patch.object(approved_linux_perl.platform, "system", return_value="Linux"), \
             patch.object(approved_linux_perl.platform, "machine", return_value="x86_64"):
            output = Path(directory) / "unused-output"
            with self.assertRaisesRegex(approved_linux_perl.Refused, "missing"):
                prepare.prepare(Path("/src/reference"), output, "a" * 40)
            self.assertFalse(output.exists())

    def test_stale_head_refuses_before_preparation(self):
        import qualification_prepare as prepare
        from tools.release import approved_linux_perl, bootstrap_oracle
        with patch.object(approved_linux_perl, "load", return_value=({}, b"")), \
             patch.object(bootstrap_oracle, "assert_approved_perl"), \
             patch.object(prepare.subprocess, "check_output", return_value="b" * 40):
            with self.assertRaisesRegex(ValueError, "exact clean candidate HEAD"):
                prepare.prepare(Path("/src/reference"), Path("/target/ops/new"), "a" * 40)

    def test_direct_launcher_rejects_project_and_argument_escape(self):
        import qualification_transport as transport
        with self.assertRaisesRegex(ValueError, "invalid qualification launcher project"):
            transport.command("../other", "prepare")
        with self.assertRaisesRegex(ValueError, "invalid qualification launcher argument"):
            transport.command("owned", "python3", "evil\x00arg")
        command = transport.command("owned", "python3", "/target/checkout/run.py")
        self.assertIn("oxidex-remote-build owned python3", command)
        self.assertNotIn("docker", command)

    def test_download_hash_mismatch_never_publishes(self):
        import qualification_transport as transport
        with TemporaryDirectory() as directory:
            local = Path(directory) / "proof.json"
            class FakeTransport:
                def scp(self, destination, remote, download=False):
                    self.assertions = (remote, download)
                    return ["fake-scp", str(destination)]
            def fake_run(argv, **_kwargs):
                Path(argv[1]).write_bytes(b"bad")
            with patch.object(transport.subprocess, "run", side_effect=fake_run):
                with self.assertRaisesRegex(ValueError, "checksum differs"):
                    transport.download(FakeTransport(), "/target/proof.json", local, "0" * 64)
            self.assertFalse(local.exists())
            self.assertEqual(list(local.parent.iterdir()), [])

class ArchiveReplayCompletenessTests(unittest.TestCase):
    def test_success_requires_all_three_archived_markers_and_full_loader(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / "output"
            receipt = output / "corpus-read/observations/receipt.json"
            receipt.parent.mkdir(parents=True)
            head = "a" * 40
            receipt.write_text(json.dumps({"producer": {"source_commit": head}}))
            markers = []
            for index, template in enumerate(qualification.ROWS):
                marker = output / f"spot-{head[:12]}-{index}/qualification-result.json"
                marker.parent.mkdir(parents=True)
                marker.write_text(json.dumps({"row": template.format(pin="13.59")}))
                markers.append(marker)
            archive = root / "results.tar.gz"
            def pack(count):
                with tarfile.open(archive, "w:gz") as stream:
                    stream.add(receipt, arcname=str(receipt.relative_to(root)))
                    for marker in markers[:count]:
                        stream.add(marker, arcname=str(marker.relative_to(root)))
            loaded = []
            def load(marker):
                loaded.append(marker)
                return {"caller": {"head": head}, "rows": [{"id": json.loads(marker.read_text())["row"]}]}
            gate = SimpleNamespace(measure=lambda *_args: (
                SimpleNamespace(corpus_files=1), SimpleNamespace(status="PASS"), {}))
            module = SimpleNamespace(load_committed_result=load)
            with patch.object(qualification, "ops_root", return_value=root), \
                 patch.object(qualification, "git", side_effect=lambda *args: head if args == ("rev-parse", "HEAD") else ""), \
                 patch.object(qualification, "qualification_module", return_value=module), \
                 patch.dict(sys.modules, {"read_regression_gate": gate}), \
                 patch.object(qualification, "verify_source", return_value={"mode": "maintainer-ssh"}), \
                 patch.object(qualification, "source_signers", return_value=Path("/unused")), \
                 patch.object(qualification, "verify_source_result_binding"), \
                 patch.object(qualification, "verify_archived_source_members"):
                pack(2)
                with self.assertRaisesRegex(ValueError, "lacks one committed Task19 marker"):
                    qualification.replay_archived_corpus(archive, output, head)
                loaded.clear()
                pack(3)
                result = qualification.replay_archived_corpus(archive, output, head)
                self.assertEqual(result["status"], "PASS")
                self.assertEqual(len(loaded), 3)
                self.assertEqual(set(result["task19_markers"]),
                                 {name.format(pin="13.59") for name in qualification.ROWS})

class LauncherConventionRegressionTests(unittest.TestCase):
    def test_staged_archive_has_launcher_toolchain_pin(self):
        import qualification_transport as transport
        with TemporaryDirectory() as directory:
            base = Path(directory)
            root, approved, old = base / "candidate", base / "approved", base / "old"
            bootstrap = root / "tools/remote-build/qualification_bootstrap.py"
            bootstrap.parent.mkdir(parents=True)
            bootstrap.write_text("# bootstrap\n")
            (root / "tools/remote-build/qualification_source.py").write_text("# source verifier\n")
            pin = root / "rust-toolchain.toml"
            pin.write_text('[toolchain]\nchannel = "1.99.0"\n')
            (approved / "read-policy-input.json").parent.mkdir(parents=True)
            (approved / "read-policy-input.json").write_text("{}")
            for pair, names in transport.REFERENCE_FILES.items():
                for name in names:
                    path = (approved / "downloaded/inputs/provisioned" / pair / name
                            if name.startswith("read-fixtures") else old / "provisioned" / pair / name)
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(name)
            bundle, signer, archive = base / "bundle", base / "signers", base / "source.tar.gz"
            bundle.write_bytes(b"signed")
            signer.write_text("maintainer ssh-ed25519 AAAA\n")
            (approved / "approved-linux-perl.json").write_text('{"schema":1}')
            with patch.object(transport.approved_linux_perl, "load"):
                transport.source_archive(root, approved, old, bundle, signer, archive)
            with tarfile.open(archive) as stream:
                self.assertEqual(stream.extractfile("rust-toolchain.toml").read(), pin.read_bytes())
                self.assertEqual(stream.extractfile("repository.bundle").read(), b"signed")
            attestation = base / "source-attestation"
            attestation.mkdir()
            for name in ("document.json", "document.sig", "github-commit.json",
                         "pgp-proof.txt", "integration-ref.json"):
                (attestation / name).write_text(name)
            with patch.object(transport.approved_linux_perl, "load"):
                transport.source_archive(root, approved, old, bundle, signer, archive, attestation)
            with tarfile.open(archive) as stream:
                self.assertEqual(stream.extractfile("source-attestation/document.sig").read(),
                                 b"document.sig")

    def test_signing_config_targets_cloned_checkout(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            signer = root / "signers"
            signer.write_text("maintainer ssh-ed25519 AAAA\n")
            calls = []
            def fake_run(argv, **_kwargs):
                calls.append(argv)
                if argv[0] == "ssh-keygen":
                    Path(argv[-1] + ".pub").write_text("ssh-ed25519 BBBB\n")
            with patch.object(qualification, "ROOT", root / "checkout"), \
                 patch.object(qualification.subprocess, "run", side_effect=fake_run):
                qualification.configure_local_signing(root / "target", signer)
            configs = [argv for argv in calls if argv[0] == "git"]
            self.assertEqual(len(configs), 5)
            self.assertTrue(all(argv[:5] == ["git", "-C", str(root / "checkout"),
                                                    "config", "--local"] for argv in configs))

    def test_machine_result_is_downloaded_not_parsed_from_launcher_stdout(self):
        import qualification_transport as transport
        with TemporaryDirectory() as directory:
            root = Path(directory)
            body = {"schema": 1, "archive_sha256": "a" * 64}
            encoded = (json.dumps(body) + "\n").encode()
            digest = hashlib.sha256(encoded).hexdigest()
            def ssh(command):
                self.assertTrue(command.startswith("sha256sum "))
                return SimpleNamespace(stdout=digest + "  /target/pack-result.json\n")
            class Direct:
                def scp(self, local, remote, download=False):
                    self.remote = remote
                    return ["fake-scp", str(local)]
            direct = Direct()
            with patch.object(transport.subprocess, "run",
                              side_effect=lambda argv, **kwargs: Path(argv[-1]).write_bytes(encoded)):
                result = transport.downloaded_result(ssh, direct, "/host/target", root,
                                                     "pack-result.json")
            self.assertEqual(result, body)
            self.assertEqual(direct.remote, "/host/target/pack-result.json")
            self.assertEqual(json.loads((root / "pack-result.json").read_text()), body)

    def test_marker_floors_are_read_from_downloaded_tree(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            staged = root / "downloaded"
            row, run_id, head, pin = "same-pin-13.59", "spot-aaaaaaaaaaaa-0", "a" * 40, "13.59"
            marker = staged / run_id / "qualification-result.json"
            marker.parent.mkdir(parents=True)
            policy_file = staged / "read-policy-input.json"
            policy_file.write_text(json.dumps({"rows": {row: {"before": 123, "after": 456}}}))
            remote = Path("/target/ops/evidence/remote-qualification/foreign")
            policy = {"path": str(remote / "read-policy-input.json"), "sha256": qualification.sha(policy_file)}
            matrix = root / "version_transition_matrix.json"
            matrix.write_bytes(b"matrix")
            marker.write_text(json.dumps({"schema": 1, "kind": "kind", "run_id": run_id,
                "status": "tooling-executed-nonpromoting", "promotion": "forbidden",
                "caller_restored": True, "caller": {"head": head, "pin_version": pin, "status": "clean"},
                "rows": [{"id": row, "qualification_outcome": "pending", "promotion": "forbidden",
                          "caller_restored": True, "read_policy_input": policy,
                          "read_payload_floors": {"before": 0, "after": 0}}],
                "matrix": {"path": str(matrix), "sha256": qualification.sha(matrix)},
                "read_policy_input": policy, "receipt_manifest": {}}))
            module = SimpleNamespace(SCHEMA=1, RESULT_KIND="kind", CANONICAL_MATRIX=matrix)
            with patch.object(qualification, "qualification_module", return_value=module):
                with self.assertRaisesRegex(RuntimeError, "differs from frozen read floors"):
                    qualification.validate_downloaded_marker(marker, remote, head, row, run_id, pin)


class ExplicitQualificationWorkerTests(unittest.TestCase):
    def test_explicit_identity_uses_native_admission_without_auto_selection(self):
        import qualification_transport as transport
        from types import SimpleNamespace
        from unittest.mock import Mock, patch
        direct = SimpleNamespace(instance_id="438", ssh=Mock())
        env = {"OXIDEX_REMOTE_INSTANCE": "builder-test", "OXIDEX_REMOTE_ZONE": "zone-a", "OXIDEX_REMOTE_INSTANCE_ID": "438"}
        with patch.dict(transport.os.environ, env, clear=True), patch.object(transport, "select_worker") as select, patch.object(transport, "DirectTransport", return_value=direct), patch.object(transport, "verify_builder_admission", return_value={"resource_probe": {"cpu": .1, "memory": .2}}) as admission:
            vm, sample, observed = transport.qualification_worker("project")
            select.assert_not_called()
            admission.assert_called_once_with("builder-test", "zone-a", "project", "438", direct.ssh)
            self.assertEqual((vm.instance_id, sample, observed), ("438", (.1, .2), direct))

    def test_partial_identity_or_runner_name_refuses_before_selection(self):
        import qualification_transport as transport
        from unittest.mock import patch
        for env in ({"OXIDEX_REMOTE_INSTANCE": "builder-test"}, {"OXIDEX_REMOTE_INSTANCE": "runner-test", "OXIDEX_REMOTE_ZONE": "z", "OXIDEX_REMOTE_INSTANCE_ID": "438"}, {"OXIDEX_REMOTE_INSTANCE": "builder-test", "OXIDEX_REMOTE_ZONE": "z", "OXIDEX_REMOTE_INSTANCE_ID": "bad"}):
            with patch.dict(transport.os.environ, env, clear=True), patch.object(transport, "select_worker") as select, patch.object(transport, "DirectTransport") as direct:
                with self.assertRaisesRegex(ValueError, "numeric pinned ID"):
                    transport.qualification_worker("project")
                select.assert_not_called()
                direct.assert_not_called()

    def test_automatic_selection_keeps_window_gate_and_rechecks_admission(self):
        import qualification_transport as transport
        from types import SimpleNamespace
        from unittest.mock import Mock, patch
        vm = SimpleNamespace(name="builder-auto", zone="z", instance_id="438")
        direct = SimpleNamespace(instance_id="438", ssh=Mock())
        with patch.dict(transport.os.environ, {}, clear=True), patch.object(transport, "select_worker", return_value=(vm, (.2, .3))) as select, patch.object(transport, "DirectTransport", return_value=direct), patch.object(transport, "verify_builder_admission") as admission:
            self.assertEqual(transport.qualification_worker("project"), (vm, (.2, .3), direct))
            select.assert_called_once_with("project")
            admission.assert_called_once()


class QualificationLifecycleRunTests(unittest.TestCase):
    """Exercise the controller boundary with a fake SSH peer and durable files."""

    def run_case(self, scenario, remote_exit="0"):
        import qualification_transport as transport
        head = "a" * 40
        run_id = "qualification-aaaaaaaaaaaa-" + "b" * 32
        archive_digest = "c" * 64
        input_digest = "d" * 64
        source_identity = {"mode": "maintainer-ssh", "head": head}
        events = []
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            candidate = root / "candidate"
            candidate.mkdir()
            (candidate / ".exiftool-version").write_text("13.59\n")
            signer = root / "allowed_signers"
            signer.write_text("maintainer public key\n")
            reference, provisioning = root / "reference", root / "provisioning"
            reference.mkdir()
            provisioning.mkdir()
            (reference / "read-policy-input.json").write_text("approved policy\n")
            output = root / "published"
            transport_path = root / "evidence/remote-qualification" / run_id / "transport.json"
            vm = SimpleNamespace(name="builder-test", zone="test-zone", instance_id="123")

            class FakeDirect:
                def ssh(self, shell):
                    return ("ssh", shell)

                def scp(self, source, destination, download=False):
                    return ("scp", str(source), str(destination), download)

            def fake_git(*args):
                if args == ("rev-parse", "HEAD"):
                    return head
                if args == ("status", "--porcelain"):
                    return ""
                if args == ("config", "--path", "--get", "gpg.ssh.allowedSignersFile"):
                    return str(signer)
                raise AssertionError(args)

            def fake_archive(_root, _reference, _provisioning, _bundle, _signer,
                             destination, _attestation):
                destination.write_bytes(b"source archive")
                return hashlib.sha256(destination.read_bytes()).hexdigest()

            def fake_process(argv, **kwargs):
                if argv[0] == "git":
                    if "create" in argv:
                        Path(argv[-2]).write_bytes(b"bundle")
                    return SimpleNamespace(stdout="", returncode=0)
                if argv[0] == "scp":
                    events.append("upload")
                    return SimpleNamespace(stdout="", returncode=0)
                self.assertEqual(argv[0], "ssh")
                shell = argv[1]
                if "nohup setsid" in shell:
                    # Inspect the on-disk receipt before the fake remote peer
                    # can acknowledge or execute the detached command.
                    durable = json.loads(transport_path.read_text())
                    self.assertEqual(durable["status"], "RUNNING_RETAINED")
                    self.assertIs(durable["launch_acknowledged"], False)
                    job = durable["remote_job"]
                    source = transport.SOURCE_HOST + "/" + run_id
                    self.assertEqual(job, {
                        "run_script": source + "/run.sh",
                        "launch_pid_file": source + "/launch.pid",
                        "exit_status_file": source + "/exit.status",
                        "exit_status_tmp_file": source + "/exit.status.tmp",
                        "log": source + "/run.log"})
                    events.append("launch")
                    if scenario == "ack_loss":
                        raise subprocess.CalledProcessError(255, argv)
                    return SimpleNamespace(stdout="", returncode=0)
                if shell.startswith("if test -f "):
                    events.append("poll")
                    return SimpleNamespace(stdout=remote_exit + "\n", returncode=0)
                if "--pack-results" in shell:
                    events.append("pack")
                    if scenario == "pack_failure":
                        raise subprocess.CalledProcessError(5, argv)
                    return SimpleNamespace(stdout="", returncode=0)
                if "--verify-results-tar" in shell:
                    events.append("replay")
                    return SimpleNamespace(stdout="", returncode=0)
                if "spot-input.tar.gz" in shell:
                    events.append("retained_check")
                    return SimpleNamespace(stdout=input_digest + "  input\n", returncode=0)
                if "/src/qualification_bootstrap.py" in shell:
                    events.append("bootstrap")
                    if scenario == "prelaunch_refusal":
                        raise subprocess.CalledProcessError(2, argv)
                return SimpleNamespace(stdout="", returncode=0)

            def fake_result(_ssh, _direct, _target, _evidence, name):
                events.append("collect_" + name)
                if scenario == "collection_failure":
                    raise ValueError("injected collection failure")
                if name == "pack-result.json":
                    return {"source_identity": source_identity,
                            "archive_sha256": archive_digest}
                return {"archive_sha256": archive_digest}

            def fake_extract(_archive, staged):
                staged_output = staged / "evidence/remote-qualification" / run_id
                staged_output.mkdir(parents=True)
                (staged_output / "remote-qualification.json").write_text(
                    json.dumps({"input_archive_sha256": input_digest}))
                policy = qualification.sha(reference / "read-policy-input.json")
                (staged_output / "preparation.json").write_text(json.dumps({
                    "head": head, "pin": "13.59", "policy_reference_sha256": policy,
                    "policy_sha256": policy}))

            with ExitStack() as patches:
                patches.enter_context(patch.object(qualification, "ROOT", candidate))
                patches.enter_context(patch.object(qualification, "ops_root", return_value=root))
                patches.enter_context(patch.object(qualification, "git", side_effect=fake_git))
                patches.enter_context(patch.object(qualification, "source_attestation", return_value=None))
                patches.enter_context(patch.object(qualification, "resolve_project", return_value="test-project"))
                patches.enter_context(patch.object(transport, "identity", return_value=("uploader", "/key")))
                patches.enter_context(patch.object(transport, "unique_run_id", return_value=run_id))
                patches.enter_context(patch.object(transport, "verify_source", return_value=source_identity))
                patches.enter_context(patch.object(transport, "source_archive", side_effect=fake_archive))
                patches.enter_context(patch.object(transport, "qualification_worker", return_value=(
                    vm, (.1, .2), FakeDirect())))
                patches.enter_context(patch.object(transport.subprocess, "run", side_effect=fake_process))
                patches.enter_context(patch.object(transport, "downloaded_result", side_effect=fake_result))
                patches.enter_context(patch.object(transport, "download", side_effect=lambda _direct, _remote, local, _digest: local.write_bytes(b"archive")))
                patches.enter_context(patch.object(qualification, "safe_extract", side_effect=fake_extract))
                patches.enter_context(patch.object(qualification, "verify_source_result_binding"))
                patches.enter_context(patch.object(qualification, "verify_archived_receipts"))
                patches.enter_context(patch.object(qualification, "publish_results", return_value=root / "published-result"))
                patches.enter_context(patch.object(transport, "DirectTransport", return_value=FakeDirect()))
                if scenario == "timeout":
                    patches.enter_context(patch.dict(transport.os.environ,
                                                     {"OXIDEX_REMOTE_QUALIFICATION_TIMEOUT_SECONDS": "1"}))
                    patches.enter_context(patch.object(transport.time, "monotonic", side_effect=[0, 1]))
                result = transport.run(output, reference, provisioning, None)
            receipt = json.loads(transport_path.read_text())
            return result, receipt, events

    def test_lost_launch_ack_keeps_durable_unknown_job(self):
        result, receipt, events = self.run_case("ack_loss")
        self.assertEqual(result, 2)
        self.assertEqual(receipt["status"], "RUNNING_RETAINED")
        self.assertIs(receipt["launch_acknowledged"], False)
        self.assertIn("remote_job", receipt)
        self.assertEqual(events.count("launch"), 1)
        self.assertNotIn("pack", events)

    def test_timeout_and_malformed_poll_keep_unknown_job(self):
        for scenario, status in (("timeout", "0"), ("malformed", "partial")):
            with self.subTest(scenario=scenario):
                result, receipt, events = self.run_case(scenario, status)
                self.assertEqual(result, 2)
                self.assertEqual(receipt["status"], "RUNNING_RETAINED")
                self.assertIn("remote_job", receipt)
                self.assertNotIn("pack", events)

    def test_unconfirmed_exit_never_starts_pack_or_collection(self):
        for status in ("4", "5", "137", "143"):
            with self.subTest(status=status):
                result, receipt, events = self.run_case("unknown", status)
                self.assertEqual(result, 2)
                self.assertEqual(receipt["status"], "RUNNING_RETAINED")
                self.assertEqual(receipt["remote_exit_code"], int(status))
                self.assertEqual(receipt["unconfirmed_task19_exit"], int(status))
                self.assertNotIn("pack", events)
                self.assertFalse(any(event.startswith("collect_") for event in events))

    def test_confirmed_collection_failure_is_refusal_and_unknown_never_collects(self):
        for status in ("0", "2"):
            with self.subTest(status=status):
                result, receipt, events = self.run_case("collection_failure", status)
                self.assertEqual(result, 2)
                self.assertEqual(receipt["status"], "REFUSED")
                self.assertIs(receipt["remote_terminal_confirmed"], True)
                self.assertIn("pack", events)
        result, receipt, events = self.run_case("unknown", "5")
        self.assertEqual(receipt["status"], "RUNNING_RETAINED")
        self.assertNotIn("pack", events)

    def test_confirmed_success_and_prelaunch_refusal_remain_distinct(self):
        result, receipt, events = self.run_case("success")
        self.assertEqual(result, 0)
        self.assertEqual(receipt["status"], "PASS")
        self.assertEqual(receipt["remote_exit_code"], 0)
        self.assertLess(events.index("launch"), events.index("poll"))
        self.assertLess(events.index("poll"), events.index("pack"))
        self.assertIn("replay", events)
        result, receipt, events = self.run_case("prelaunch_refusal")
        self.assertEqual(result, 2)
        self.assertEqual(receipt["status"], "REFUSED")
        self.assertNotIn("remote_job", receipt)
        self.assertNotIn("launch", events)
