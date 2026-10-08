#!/usr/bin/env python3
"""Small refusal controls for the read-only Task19 receipt adapter.

These do not claim that synthetic results pass the production replay verifier.
"""
from __future__ import annotations

from contextlib import redirect_stderr
import io
from pathlib import Path
import os
import subprocess
import shutil
import sys
from unittest.mock import patch
from tempfile import TemporaryDirectory
import unittest

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import verify_task19_results as adapter


class Task19AdapterControls(unittest.TestCase):
    def test_all_tool_hashes_must_be_explicit_and_unique(self) -> None:
        items = [f"{name}={'a' * 64}" for name in adapter.TOOL_FILES]
        self.assertEqual(len(adapter.tool_expectations(items)), len(adapter.TOOL_FILES))
        for invalid in (items[:-1], items + items[:1], ["unknown.py=" + "a" * 64] + items):
            with self.subTest(invalid=invalid), self.assertRaises(adapter.qualification.Refused):
                adapter.tool_expectations(invalid)

    def test_six_side_binary_identities_must_be_explicit(self) -> None:
        items = [f"{row}:{side}:/target/{row}/{side}/oxidex:{'b' * 64}"
                 for row in adapter.ROWS for side in adapter.qualification.SIDES]
        self.assertEqual(len(adapter.binary_expectations(items)), 6)
        for invalid in (items[:-1], items + items[:1], [items[0].replace('/target/', 'relative/')]+items[1:]):
            with self.subTest(invalid=invalid), self.assertRaises(adapter.qualification.Refused):
                adapter.binary_expectations(invalid)

    def test_next_pin_markdown_selection_and_ambiguity(self) -> None:
        labels = ("Next pin", "Intended next pin", "Intended ExifTool pin",
                  "ExifTool pin after current", "Next ExifTool release",
                  "Intended ExifTool release")
        forms = ("{label}: 13.60", "- [ ] {label}: 13.60",
                 "- **{label}:** 13.60", "- [x] **{label}:** 13.60")
        for label in labels:
            for form in forms:
                text = form.format(label=label)
                with self.subTest(text=text):
                    self.assertEqual(adapter.next_pin_selection(text), "13.60")
        for text in ("- [ ] Run one current-pin -> next-pin rehearsal",
                     "**Next pin:** not selected", "- [ ] Intended next pin: not selected",
                     "For ExifTool release 13.59, confirm archive fixtures",
                     "For current pin 13.59, confirm archive fixtures",
                     "The current pin is 13.59",
                     "- [ ] Verify ExifTool version 13.59 capability",
                     "Next release: v2.0.0-beta.1",
                     "Current ExifTool release: 13.59\nNext pin: not selected",
                     "- [ ] **Current ExifTool version:** 13.59\nNext pin: not selected",
                     "Current pin: 13.59"):
            with self.subTest(text=text):
                self.assertEqual(adapter.next_pin_selection(text), "not selected")
        for text in ("- [ ] Next pin maybe 13.60", "Next pin: 13.60\nNext pin: 13.61",
                     "- [ ] Upcoming pin: 13.60", "The intended next pin is 13.60",
                     "Potential pin: 13.60", "Upcoming ExifTool release: 13.60",
                     "The intended ExifTool release is 13.60", "Next version: 13.60", "ExifTool version: 13.60",
                     "Set pin to 13.60", "Set current pin to 13.60",
                     "Current ExifTool release: 13.60",
                     "Current ExifTool version: 13.60"):
            with self.subTest(text=text), self.assertRaises(adapter.qualification.Refused):
                adapter.next_pin_selection(text)

    def test_final_marker_identity_binds_loaded_bytes_and_rejects_swap(self) -> None:
        import json
        with TemporaryDirectory() as directory:
            marker = Path(directory) / "qualification-result.json"
            original = {"schema": 1, "run_id": "original"}
            marker.write_text(json.dumps(original))
            binding, original_bytes = adapter.marker_snapshot(marker)
            self.assertEqual(json.loads(original_bytes), original)
            adapter.require_marker_unchanged(marker, binding)
            replacement = Path(directory) / "replacement.json"
            replacement.write_text(json.dumps({"schema": 1, "run_id": "replacement"}))
            replacement.replace(marker)
            with self.assertRaisesRegex(adapter.qualification.Refused, "changed"):
                adapter.require_marker_unchanged(marker, binding)
            # Replacing with identical bytes is still a different file identity.
            same = Path(directory) / "same.json"
            same.write_bytes(original_bytes)
            same.replace(marker)
            with self.assertRaisesRegex(adapter.qualification.Refused, "changed"):
                adapter.require_marker_unchanged(marker, binding)

    def test_fifo_final_marker_refuses_without_waiting_for_writer(self) -> None:
        with TemporaryDirectory() as directory:
            fifo = Path(directory) / "qualification-result.json"
            os.mkfifo(fifo)
            with self.assertRaisesRegex(adapter.qualification.Refused, "regular file"):
                adapter.marker_snapshot(fifo)

    def test_final_marker_swap_during_loader_cannot_bind_replacement(self) -> None:
        import json
        with TemporaryDirectory() as directory:
            paths = []
            for name in ("same", "forward", "reverse"):
                parent = Path(directory) / name
                parent.mkdir()
                marker = parent / "qualification-result.json"
                marker.write_text(json.dumps({"schema": 1}))
                paths.append(marker)
            expected_binaries = {
                (row, side): {"path": f"/target/{row}/{side}", "sha256": "b" * 64}
                for row in adapter.ROWS for side in adapter.qualification.SIDES}
            expected_tools = {name: adapter.sha(adapter.ROOT / name) for name in adapter.TOOL_FILES}
            def swap(_path):
                replacement = paths[0].with_name("replacement.json")
                replacement.write_text(json.dumps({"schema": 1, "replacement": True}))
                replacement.replace(paths[0])
                return {"schema": 1}
            with patch.object(adapter.qualification, "_evidence_location", side_effect=lambda path, _label: path), \
                 patch.object(adapter.qualification, "load_committed_result", side_effect=swap):
                with self.assertRaisesRegex(adapter.qualification.Refused, "changed"):
                    adapter.verify_results(
                        paths=tuple(paths), expected_head="a" * 40, expected_tree="c" * 40,
                        expected_matrix_sha256=adapter.sha(adapter.qualification.CANONICAL_MATRIX),
                        expected_policy_sha256=adapter.POLICY_SHA256,
                        expected_tools=expected_tools, expected_binaries=expected_binaries)

    def test_output_cannot_physically_overlap_owned_source(self) -> None:
        with TemporaryDirectory() as directory:
            source = Path(directory) / "source"
            source.mkdir()
            alias = Path(directory) / "alias"
            alias.symlink_to(source, target_is_directory=True)
            with patch.object(adapter, "ROOT", source):
                for output in (source / "receipt.json", alias / "receipt.json"):
                    with self.subTest(output=output), self.assertRaisesRegex(
                            adapter.qualification.Refused, "overlaps"):
                        adapter.refuse_source_output_overlap(output)
                adapter.refuse_source_output_overlap(Path(directory) / "evidence" / "receipt.json")
                # The real case alias exists only on case-insensitive volumes.
                case_alias = source.with_name(source.name.upper())
                if case_alias.exists() and os.path.samefile(case_alias, source):
                    with self.assertRaisesRegex(adapter.qualification.Refused, "overlaps"):
                        adapter.refuse_source_output_overlap(case_alias / "receipt.json")
                # A synthetic inode alias exercises the identity check on Linux.
                bind_alias = Path(directory) / "bind-alias"
                bind_alias.mkdir()
                original_samefile = os.path.samefile
                with patch.object(adapter.os.path, "samefile", side_effect=lambda left, right:
                                  left == bind_alias or original_samefile(left, right)):
                    with self.assertRaisesRegex(adapter.qualification.Refused, "overlaps"):
                        adapter.refuse_source_output_overlap(bind_alias / "receipt.json")

    def test_cli_rejects_source_output_before_replay_or_write(self) -> None:
        with TemporaryDirectory() as directory:
            repo = Path(directory)
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "test"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            (repo / ".exiftool-version").write_text("13.59\n")
            (repo / "TODO_RELEASE_BETA.md").write_text("Next pin: not selected\n")
            subprocess.run(["git", "-C", str(repo), "add", ".exiftool-version", "TODO_RELEASE_BETA.md"], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            head = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            tree = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD^{tree}"], text=True).strip()
            output = repo / "receipt.json"
            argv = ["--same-pin-result", "/missing/same/qualification-result.json",
                    "--forward-result", "/missing/forward/qualification-result.json",
                    "--reverse-result", "/missing/reverse/qualification-result.json",
                    "--expected-head", head, "--expected-tree", tree,
                    "--expected-matrix-sha256", "a" * 64,
                    "--expected-read-policy-sha256", adapter.POLICY_SHA256,
                    "--next-pin", "not-selected", "--output", str(output)]
            for name in adapter.TOOL_FILES:
                argv.extend(["--expected-tool", f"{name}={'b' * 64}"])
            for row in adapter.ROWS:
                for side in adapter.qualification.SIDES:
                    argv.extend(["--expected-binary", f"{row}:{side}:/target/oxidex:{'c' * 64}"])
            with patch.object(adapter, "ROOT", repo), \
                 patch.object(adapter, "require_imported_source_paths"), \
                 patch.object(adapter.qualification, "_evidence_location", side_effect=lambda path, _label: path), \
                 patch.object(adapter, "verify_results") as replay, redirect_stderr(io.StringIO()) as error:
                self.assertEqual(adapter.main(argv), 2)
                self.assertIn("overlaps", error.getvalue())
                replay.assert_not_called()
            self.assertFalse(output.exists())

    def test_durable_result_path_refuses_temporary_evidence(self) -> None:
        with TemporaryDirectory() as directory:
            marker = Path(directory) / "qualification-result.json"
            marker.write_text("{}")
            with self.assertRaisesRegex(adapter.qualification.Refused, "durable"):
                adapter.qualification._evidence_location(marker, "Task19 committed result")

    def test_subordinate_inventory_detects_earlier_run_mutation(self) -> None:
        with TemporaryDirectory() as directory:
            roots = []
            for label in ("same", "forward", "reverse"):
                run = Path(directory) / label
                run.mkdir()
                marker = run / "qualification-result.json"
                marker.write_text('{}')
                roots.append(marker)
            earlier = roots[0].parent / "execution-status.json"
            earlier.write_text('{"state":"complete"}')
            policy = Path(directory) / "policy.json"
            policy.write_text('{}')
            value = {"rows": {}, "read_policy_input": {"path": str(policy)}}
            with patch.object(adapter.qualification, "_evidence_location", side_effect=lambda path, _label: path):
                before = adapter.subordinate_snapshot(tuple(roots), value)
                self.assertEqual(adapter.subordinate_snapshot(tuple(roots), value), before)
                earlier.write_text('{"state":"tampered"}')
                self.assertNotEqual(adapter.subordinate_snapshot(tuple(roots), value), before)

    def test_no_replace_publication_and_failed_durability_retract(self) -> None:
        with TemporaryDirectory() as directory:
            output = Path(directory) / "receipt.json"
            output.write_text('{"owner":"foreign"}\n')
            with self.assertRaisesRegex(adapter.qualification.Refused, "already exists"):
                adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertEqual(output.read_text(), '{"owner":"foreign"}\n')
            output.unlink()
            original_link = os.link
            def publish_foreign(source, destination, **kwargs):
                Path(destination).write_text('{"owner":"racer"}\n')
                return original_link(source, destination, **kwargs)
            with patch.object(adapter.os, "link", side_effect=publish_foreign):
                with self.assertRaisesRegex(adapter.qualification.Refused, "already exists"):
                    adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertEqual(output.read_text(), '{"owner":"racer"}\n')
            output.unlink()
            with patch.object(adapter.qualification, "_fsync_directory", side_effect=[OSError("disk"), None]):
                with self.assertRaises(OSError):
                    adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertFalse(output.exists())
            adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertIn('verified_read_only', output.read_text())

    def test_actual_source_refuses_dirty_pin_and_todo(self) -> None:
        with TemporaryDirectory() as directory:
            repo = Path(directory)
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "test"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            pin, todo = repo / ".exiftool-version", repo / "TODO_RELEASE_BETA.md"
            pin.write_text("13.59\n")
            todo.write_text("Next pin: not selected\n")
            subprocess.run(["git", "-C", str(repo), "add", ".exiftool-version", "TODO_RELEASE_BETA.md"], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            head = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            tree = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD^{tree}"], text=True).strip()
            with patch.object(adapter, "ROOT", repo), \
                 patch.object(adapter, "require_imported_source_paths"):
                clean = adapter.source_snapshot(head, tree)
                self.assertEqual(clean["head"], head)
                pin.write_text("13.60\n")
                with self.assertRaisesRegex(adapter.qualification.Refused, "changes"):
                    adapter.source_snapshot(head, tree)
                pin.write_text("13.59\n")
                todo.write_text("Next pin: 13.60\n")
                with self.assertRaisesRegex(adapter.qualification.Refused, "changes"):
                    adapter.source_snapshot(head, tree)
                todo.write_text("Next pin: not selected\n")
                self.assertEqual(adapter.source_snapshot(head, tree), clean)
                subprocess.run(["git", "-C", str(repo), "update-index", "--skip-worktree", ".exiftool-version"], check=True)
                with self.assertRaisesRegex(adapter.qualification.Refused, "hidden index flags"):
                    adapter.source_snapshot(head, tree)
                subprocess.run(["git", "-C", str(repo), "update-index", "--no-skip-worktree", ".exiftool-version"], check=True)

    def test_committed_write_matrix_replay_from_synthetic_stage_fixture(self) -> None:
        # The stage fixture runs a real source-derived 1,530-row matrix parser.
        # Only the costly executor orchestration/stage-result seam is mocked.
        import test_version_rehearsal_stage_adapter as fixture_module
        fixture = fixture_module.AdapterTests(methodName="runTest")
        fixture.setUp()
        try:
            run_dir = fixture.root / "run" / "same-pin-13.59" / "before"
            checkout = run_dir / "checkouts" / "release-11-78"
            checkout.parent.mkdir(parents=True)
            shutil.move(str(fixture.checkout), str(checkout))
            fixture.checkout = checkout
            fixture.reports = run_dir / "stage-results" / "release-11-78"
            fixture.reports.mkdir(parents=True)
            fixture.write_manifest = fixture.write_manifest.resolve()
            stage = fixture_module.adapter
            with patch.dict(os.environ, {"CARGO_HOME": str(fixture.root / "empty-cargo-home")}):
                stage.generate(fixture.args("generate"), run=fixture.fake_run)
                built = stage.build(fixture.args("build"), run=fixture.fake_run)
                write = stage.write(fixture.args("write"), run=fixture.fake_run)
            digester = adapter.qualification.rehearsal.sha256_json
            journal = {"releases": {"11.78": {"reports": {
                name: {"path": f"stage-results/release-11-78/{name}.json", "sha256": digester(report)}
                for name, report in (("build", built), ("write", write))}}}}
            (run_dir / "execution-status.json").write_text(__import__("json").dumps(journal))
            config_path = run_dir / "inputs" / "config.json"
            config_path.parent.mkdir()
            config_path.write_text(__import__("json").dumps({
                "target_directories": {"11.78": str(fixture.target)},
                "write_fixture_bindings": {"11.78": adapter.executor._write_fixture_binding(
                    str(fixture.write_manifest))}}))
            self.assertEqual(write["fixtures"]["manifest"], adapter.executor._write_fixture_binding(str(fixture.write_manifest))["path"])
            self.assertEqual(write["fixtures"]["manifest_sha256"], adapter.executor._write_fixture_binding(str(fixture.write_manifest))["sha256"])
            self.assertEqual(adapter.executor._require_fixture_proof(write), adapter.executor._write_fixture_binding(str(fixture.write_manifest))["fixtures"])
            self.assertEqual(write["matrix_reports"][0]["path"], str(fixture.reports / "raw" / "write-matrix" / "0000" / "report.json"))
            row = {"id": "same-pin-13.59", "before": {
                "release": "11.78", "write_report_sha256": digester(write),
                "instrument": {"native_identity": write["native_identity"],
                               "native_probe_sha256": write["native_probe_sha256"]}}}
            with patch.object(adapter.executor, "_stage_result", return_value=write):
                proof = adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                       fixture_module.COMMIT)
                self.assertEqual(proof["matrix_reports"][0]["passed"], 1530)
                self.assertEqual(proof["writer_binary"], built["writer_binary"])
                row["before"]["write_report_sha256"] = "0" * 64
                with self.assertRaisesRegex(adapter.qualification.Refused, "write digest"):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
                row["before"]["write_report_sha256"] = digester(write)
                matrix = Path(write["matrix_reports"][0]["path"])
                original_matrix = matrix.read_bytes()
                matrix.unlink()
                with self.assertRaises(adapter.qualification.Refused):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
                matrix.write_bytes(original_matrix)
                altered = __import__("json").loads(original_matrix)
                altered["passed"] -= 1
                matrix.write_text(__import__("json").dumps(altered))
                with self.assertRaises(adapter.qualification.Refused):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
                matrix.write_bytes(original_matrix)
            raw_path = Path(write["raw_report"]["path"])
            raw_original = raw_path.read_bytes()
            malformed = __import__("json").loads(raw_original)
            malformed["commands"] = [1]
            raw_path.write_text(__import__("json").dumps(malformed))
            import copy, hashlib
            malformed_write = copy.deepcopy(write)
            malformed_write["raw_report"]["sha256"] = hashlib.sha256(raw_path.read_bytes()).hexdigest()
            row["before"]["write_report_sha256"] = digester(malformed_write)
            original_report_for = adapter.qualification._report_for
            def report_for(path, status, selected_release, stage_name):
                return malformed_write if stage_name == "write" else original_report_for(
                    path, status, selected_release, stage_name)
            with patch.object(adapter.qualification, "_report_for", side_effect=report_for), \
                 patch.object(adapter.executor, "_stage_result", return_value=malformed_write):
                with self.assertRaisesRegex(adapter.qualification.Refused, "commands must be a list of objects"):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
            row["before"]["write_report_sha256"] = digester(write)
            raw_path.write_bytes(raw_original)
            wrong_writer = dict(write, writer_binary=dict(write["writer_binary"], sha256="0" * 64))
            with patch.object(adapter.executor, "_stage_result", return_value=wrong_writer):
                with self.assertRaisesRegex(adapter.qualification.Refused, "writer"):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
        finally:
            fixture.doCleanups()

    def test_stale_policy_and_matrix_refuse_before_receipt_replay(self) -> None:
        with TemporaryDirectory() as directory:
            absent = Path(directory) / 'absent' / 'qualification-result.json'
            binaries = {(row, side): {"path": f"/target/{row}/{side}", "sha256": 'b' * 64}
                        for row in adapter.ROWS for side in adapter.qualification.SIDES}
            tools = {name: adapter.sha(adapter.ROOT / name) for name in adapter.TOOL_FILES}
            common = dict(paths=(absent, absent, absent), expected_head='a' * 40,
                          expected_tree='c' * 40, expected_tools=tools,
                          expected_binaries=binaries)
            with self.assertRaisesRegex(adapter.qualification.Refused, 'accepted read-policy'):
                adapter.verify_results(expected_matrix_sha256='d' * 64,
                                       expected_policy_sha256='e' * 64, **common)
            with self.assertRaisesRegex(adapter.qualification.Refused, 'local canonical matrix'):
                adapter.verify_results(expected_matrix_sha256='d' * 64,
                                       expected_policy_sha256=adapter.POLICY_SHA256, **common)

    def test_missing_committed_final_cannot_pass(self) -> None:
        with TemporaryDirectory() as directory:
            paths = tuple(Path(directory) / name / 'qualification-result.json'
                          for name in ('same', 'forward', 'reverse'))
            binaries = {(row, side): {"path": f"/target/{row}/{side}", "sha256": 'b' * 64}
                        for row in adapter.ROWS for side in adapter.qualification.SIDES}
            tools = {name: adapter.sha(adapter.ROOT / name) for name in adapter.TOOL_FILES}
            with self.assertRaises(adapter.qualification.Refused):
                adapter.verify_results(
                    paths=paths, expected_head='a' * 40, expected_tree='c' * 40,
                    expected_matrix_sha256=adapter.sha(adapter.qualification.CANONICAL_MATRIX),
                    expected_policy_sha256=adapter.POLICY_SHA256,
                    expected_tools=tools, expected_binaries=binaries)


if __name__ == '__main__':
    unittest.main()
