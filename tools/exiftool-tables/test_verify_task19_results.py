#!/usr/bin/env python3
"""Small refusal controls for the read-only Task19 receipt adapter.

These do not claim that synthetic results pass the production replay verifier.
"""
from __future__ import annotations

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
        for text in ("Next pin: 13.60", "- [ ] Next pin: 13.60",
                     "- **Next pin:** 13.60", "- [x] **Next pin:** 13.60"):
            with self.subTest(text=text):
                self.assertEqual(adapter.next_pin_selection(text), "13.60")
        self.assertEqual(adapter.next_pin_selection("- [ ] Run one current-pin -> next-pin rehearsal"),
                         "not selected")
        self.assertEqual(adapter.next_pin_selection("**Next pin:** not selected"), "not selected")
        for text in ("- [ ] Next pin maybe 13.60", "Next pin: 13.60\nNext pin: 13.61"):
            with self.subTest(text=text), self.assertRaises(adapter.qualification.Refused):
                adapter.next_pin_selection(text)

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
            with patch.object(adapter, "ROOT", repo):
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
