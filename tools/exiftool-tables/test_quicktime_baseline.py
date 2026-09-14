import hashlib
import json
from pathlib import Path
import unittest
import subprocess
import tempfile

import quicktime_atom_tables as selector
import quicktime_baseline as baseline
import capture_quicktime_baseline as capture

HERE = Path(__file__).resolve().parent


class BaselineTests(unittest.TestCase):
    def test_source_fingerprint_detects_already_dirty_tracked_and_untracked_edits(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            tracked = root / "source.py"
            tracked.write_text("committed")
            subprocess.run(["git", "-C", str(root), "add", "."], check=True)
            subprocess.run(["git", "-C", str(root), "-c", "user.name=Fixture",
                            "-c", "user.email=fixture@example.invalid", "commit", "-qm", "base"], check=True)
            tracked.write_text("first edit")
            first = baseline.source_fingerprint(root)
            tracked.write_text("second edit")
            self.assertNotEqual(first, baseline.source_fingerprint(root))
            other = root / "new.py"
            other.write_text("one")
            first = baseline.source_fingerprint(root)
            other.write_text("two")
            self.assertNotEqual(first, baseline.source_fingerprint(root))

    def test_same_version_oracle_source_edits_are_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / "lib/Image/ExifTool/QuickTime.pm"
            source.parent.mkdir(parents=True)
            source.write_bytes(b"original processor")
            (root / "exiftool").write_bytes(b"version 13.59")
            manifest = {"schema": "oxidex_pinned_oracle_sources_v1", "version": "13.59",
                        "files": {name: hashlib.sha256((root / name).read_bytes()).hexdigest()
                                  for name in ("exiftool", "lib/Image/ExifTool/QuickTime.pm")}}
            baseline.verify_oracle_sources(root, manifest)
            source.write_bytes(b"modified processor")
            with self.assertRaisesRegex(ValueError, "differs from pinned release"):
                baseline.verify_oracle_sources(root, manifest)
            source.write_bytes(b"original processor")
            (source.parent / "Injected.pm").write_bytes(b"unexpected source")
            with self.assertRaisesRegex(ValueError, "file universe"):
                baseline.verify_oracle_sources(root, manifest)

    def test_committed_fixture_bytes_and_observation_hashes(self):
        baseline.verify_fixtures()
        report = json.loads((baseline.ROOT / "docs/reference/quicktime-reading-baseline.json").read_text())
        self.assertEqual(report["provenance_status"], "replay_verified")
        self.assertRegex(report["binary_sha256"], r"^[a-f0-9]{64}$")
        self.assertRegex(report["source_commit"], r"^[a-f0-9]{40}$")
        self.assertIsInstance(report["source_dirty"], bool)
        self.assertIsInstance(report["source_dirty_files"], list)
        self.assertEqual(report["instrument_sha256"],
                         hashlib.sha256(Path(baseline.__file__).read_bytes()).hexdigest())
        self.assertEqual(report["reading_matched_fixture_projections"],
                         sum(row["expected"] == row["actual"] for row in report["fixtures"]))
        recorded = {row["fixture"]: row["fixture_sha256"] for row in report["fixtures"]}
        self.assertEqual(recorded, {name: hashlib.sha256(data).hexdigest()
                                    for name, data in baseline.cases().items()})

    def test_replay_refuses_in_tree_output_before_build(self):
        with self.assertRaisesRegex(ValueError, "outside the worktree"):
            baseline.compare(Path("nonexistent-oracle"), baseline.ROOT / "must-not-be-created")
        self.assertFalse((baseline.ROOT / "must-not-be-created").exists())

    def test_committed_source_ledger_and_summary_are_current(self):
        report = selector.report((HERE / "fixtures/quicktime_source_13_59.json").read_bytes())
        self.assertEqual(report["source"]["capture_scope"]["source_module_table_count"], 87)
        self.assertEqual(selector.serialized(report), (HERE / "quicktime_source_capabilities.json").read_text())
        self.assertEqual(selector.serialized(selector.summary(report)),
                         (baseline.ROOT / "docs/reference/quicktime-source-baseline.json").read_text())

    def test_projection_preserves_group_and_value_types(self):
        self.assertEqual(baseline.projection([{"ItemList:AlbumID": 4294967297,
                                              "UserData:AlbumID": "different"}]),
                         {"ItemList:AlbumID": 4294967297})
        with self.assertRaises(ValueError):
            baseline.projection([{}, {}])

    def test_reading_check_detects_changed_values_and_pin(self):
        before = {"pin": "13.59", "fixtures": [{"actual": {"ItemList:AlbumID": 1}}]}
        baseline.check_reading_baseline(before, before)
        with self.assertRaises(ValueError):
            baseline.check_reading_baseline({**before, "pin": "13.60"}, before)
        with self.assertRaises(ValueError):
            baseline.check_reading_baseline({**before, "fixtures": []}, before)

    def test_capture_preserves_selected_tables_and_records_parent_scope(self):
        doc = json.loads((HERE / "fixtures/quicktime_source_13_59.json").read_text())
        full = {**doc, "modules_failed": 0}
        module = full["modules"]["QuickTime"]
        module["tables"]["Other"] = {"sentinel": True}
        module["table_count"] = len(module["tables"])
        result = capture.extract(full, full_hash="0" * 64, source_commit="1" * 40,
                                 perl_version="5.38.2", tool_hash="2" * 64)
        self.assertEqual(result["capture_scope"]["source_module_table_count"], 4)
        self.assertEqual(result["modules"]["QuickTime"]["table_count"], 3)
        self.assertNotIn("Other", result["modules"]["QuickTime"]["tables"])
        for name in selector.TABLES:
            self.assertEqual(result["modules"]["QuickTime"]["tables"][name], module["tables"][name])

    def test_capture_provenance_cannot_be_removed_or_reassigned(self):
        document = json.loads((HERE / "fixtures/quicktime_source_13_59.json").read_text())
        for key in ("kind", "tables", "source_module_table_count", "source_commit", "full_dump_sha256", "dump_tool_sha256", "perl_version"):
            changed = json.loads(json.dumps(document))
            del changed["capture_scope"][key]
            with self.assertRaises(ValueError):
                selector.report(json.dumps(changed).encode())
        document["capture_scope"]["dump_tool_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "tool hash"):
            selector.report(json.dumps(document).encode())

    def test_scope_and_candidate_counts_conserve_all_source_alternatives(self):
        report = selector.report((HERE / "fixtures/quicktime_source_13_59.json").read_bytes())
        for family in report["families"]:
            self.assertEqual(family["variant_records"], family["declarative_candidates"] + family["refused_records"])
            for record in family["records"]:
                self.assertFalse(record["runtime_connected"])
                self.assertIsNone(record["observed_write"])
                self.assertIsNone(record["observed_read"])


if __name__ == "__main__":
    unittest.main()
