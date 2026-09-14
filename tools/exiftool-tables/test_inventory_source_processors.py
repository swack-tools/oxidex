import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import inventory_source_processors as inventory


def table(tags=None, *, tag_count=0):
    return {
        "full_name": "Image::ExifTool::Any::Main",
        "meta": {},
        "tags": {} if tags is None else tags,
        "tag_count": tag_count,
    }


def document(*, modules=None, modules_ok=1, modules_failed=0):
    return {
        "modules": {"Any": {"table_count": 1, "tables": {"Main": table()}}} if modules is None else modules,
        "modules_ok": modules_ok,
        "modules_failed": modules_failed,
        "exiftool_version": "13.59",
    }


class StructuralInputTests(unittest.TestCase):
    def test_keyed_word_candidate_uses_processor_shape_not_table_name(self):
        from test_word_directory import fixture_processor
        matched, unrelated = inventory.run_selectors(HERE, [
            {"PROCESS_PROC": fixture_processor()},
            {"PROCESS_PROC": {"__perl": "CODE", "__name": "Image::ExifTool::Any::Custom",
                              "__deparse": "{ return 1; }"}},
        ])
        self.assertTrue(matched["keyed_word_candidate"])
        self.assertFalse(matched["keyed_profile"])
        self.assertFalse(unrelated["keyed_word_candidate"])

    def test_empty_or_failed_module_sets_cannot_define_a_denominator(self):
        with self.assertRaisesRegex(ValueError, "non-empty"):
            inventory.validate_document(document(modules={}, modules_ok=0))
        with self.assertRaisesRegex(ValueError, "failed modules"):
            inventory.validate_document(document(modules_failed=1))

    def test_every_module_and_table_count_is_checked(self):
        bad_module = {"Any": {"table_count": 2, "tables": {"Main": table()}}}
        with self.assertRaisesRegex(ValueError, "table_count 2 != table objects 1"):
            inventory.validate_document(document(modules=bad_module))
        bad_table = table(tag_count=1)
        with self.assertRaisesRegex(ValueError, "tag_count 1 != raw keys 0"):
            inventory.validate_document(document(modules={"Any": {"table_count": 1, "tables": {"Main": bad_table}}}))

    def test_malformed_variant_containers_are_rejected_before_reporting(self):
        tags = {"1": {"_variants": {"Name": "NotAnAlternative"}}}
        bad = table(tags, tag_count=1)
        with self.assertRaisesRegex(ValueError, "variants must be a list"):
            inventory.validate_document(document(modules={"Any": {"table_count": 1, "tables": {"Main": bad}}}))

    def test_non_list_variant_dict_is_not_counted_as_named(self):
        counts = inventory.count_rows({"1": {"_variants": {"Name": "NotAnAlternative"}}})
        self.assertEqual(counts["alternative_slots"], 1)
        self.assertEqual(counts["malformed_variant_container_slots"], 1)
        self.assertEqual(counts["named_alternatives"], 0)

    def test_identity_expectations_are_required_by_the_cli(self):
        with tempfile.TemporaryDirectory() as directory:
            dump = Path(directory) / "dump.json"
            dump.write_text(json.dumps(document()))
            proc = subprocess.run(
                [sys.executable, str(HERE / "inventory_source_processors.py"), "--dump", str(dump)],
                text=True,
                capture_output=True,
                check=False,
            )
        self.assertEqual(proc.returncode, 2)
        self.assertIn("--expected-dump-sha", proc.stderr)
        self.assertIn("--expected-selector-commit", proc.stderr)


if __name__ == "__main__":
    unittest.main()
