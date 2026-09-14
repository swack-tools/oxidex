import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


PATH = Path(__file__).with_name("catalog_observed_snapshot.py")
spec = importlib.util.spec_from_file_location("catalog_observed_snapshot", PATH)
publication = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(publication)


def entry(raw_key, observed_read="not_observed_yet", observed_write="not_observed_yet"):
    return {
        "identity": {"table": "Image::ExifTool::Exif::Main", "raw_key": raw_key, "variant_index": 0},
        "catalog": {"name": "Artist", "groups": {"1": "IFD0"}},
        "source": {"state": "joined", "row_sha256": "a" * 64},
        "source_layout_status": "source_row_joined",
        "source_derived_implementation": "ifd_schema_declaration_eligible_unobserved",
        "reader_implementation": "ifd_schema_declaration_eligible_unobserved",
        "writer_implementation": "generated_writer_declaration_unobserved",
        "implementation_refusal_reasons": None,
        "observed_read": observed_read,
        "observed_write": observed_write,
    }


def join(rows):
    return {
        "schema": publication.JOIN_SCHEMA,
        "inputs": {"exiftool_version": "13.59"},
        "counts": {"joined_records": len(rows), "observed_read": {"not_observed_yet": len(rows)}},
        "entries": rows,
    }


class CatalogObservedSnapshotTests(unittest.TestCase):
    def test_snapshot_preserves_observations_and_source_binding(self):
        source = join([entry("315")])
        observed = copy.deepcopy(source)
        observed["entries"][0]["observed_read"] = "observed_matched_read"
        observed["entries"][0]["observed_write"] = "observed_matched_write"
        observed["counts"] = {"joined_records": 1, "observed_read": {"observed_matched_read": 1},
                              "observed_write": {"observed_matched_write": 1}}
        source_bytes = b'{"source":"immutable"}'
        snapshot = publication.make_snapshot(source, observed, publication.hashlib.sha256(source_bytes).hexdigest(),
                                             "1" * 40, "2" * 40, "native verifier")
        publication.validate_snapshot(source, source_bytes, snapshot)
        self.assertEqual(snapshot["observed_join"]["entries"][0]["observed_write"], "observed_matched_write")
        self.assertEqual(snapshot["source_table_observations"]["Image::ExifTool::Exif::Main"]["observed_read"],
                         {"observed_matched_read": 1})

    def test_refuses_observation_that_changes_source_or_coordinate_accounting(self):
        source = join([entry("315")])
        observed = copy.deepcopy(source)
        observed["entries"][0]["catalog"]["name"] = "Forged"
        with self.assertRaisesRegex(ValueError, "source/declaration"):
            publication.validate_pair(source, observed)
        observed = copy.deepcopy(source)
        observed["entries"][0]["identity"]["raw_key"] = "316"
        with self.assertRaisesRegex(ValueError, "coordinates"):
            publication.validate_pair(source, observed)

    def test_refuses_stale_or_unattributed_snapshot(self):
        source = join([entry("315")])
        bytes_a = b"first source join"
        snapshot = publication.make_snapshot(source, source, publication.hashlib.sha256(bytes_a).hexdigest(),
                                             "1" * 40, "2" * 40, "native verifier")
        with self.assertRaisesRegex(ValueError, "different source join"):
            publication.validate_snapshot(source, b"later source join", snapshot)
        snapshot["runtime"]["instrument"] = ""
        with self.assertRaisesRegex(ValueError, "runtime commit or instrument"):
            publication.validate_snapshot(source, bytes_a, snapshot)

    def test_refuses_forged_observation_summary(self):
        source = join([entry("315")])
        observed = copy.deepcopy(source)
        observed["counts"]["observed_read"] = {"observed_matched_read": 1}
        with self.assertRaisesRegex(ValueError, "read counts"):
            publication.validate_pair(source, observed)

    def test_cli_replaces_and_then_verifies_a_bound_receipt(self):
        source = join([entry("315")])
        observed = copy.deepcopy(source)
        observed["entries"][0]["observed_read"] = "observed_matched_read"
        observed["counts"]["observed_read"] = {"observed_matched_read": 1}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source_path, observed_path = root / "source.json", root / "observed.json"
            snapshot_path, report_path = root / "snapshot.json", root / "report.md"
            source_path.write_text(json.dumps(source)); observed_path.write_text(json.dumps(observed))
            snapshot_path.write_text("old"); report_path.write_text("old")
            command = [sys.executable, str(PATH), "--source-join", str(source_path),
                       "--observed-join", str(observed_path), "--source-commit", "1" * 40,
                       "--runtime-commit", "2" * 40, "--instrument", "native verifier",
                       "--snapshot", str(snapshot_path), "--report", str(report_path), "--replace"]
            subprocess.run(command, check=True, capture_output=True, text=True)
            subprocess.run([sys.executable, str(PATH), "--source-join", str(source_path),
                            "--snapshot", str(snapshot_path), "--verify"],
                           check=True, capture_output=True, text=True)


if __name__ == "__main__":
    unittest.main()
