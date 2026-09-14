"""Tests for the non-promoting native write-definition catalog."""

from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import write_coverage_catalog as catalog


def fact(value):
    return {"present": True, "value": value}


def sample_dump():
    row = lambda name, writable, group, **extra: {
        "entry_kind": "HASH",
        "effective_properties": {"Name": fact(name), "Writable": fact(writable), "WriteGroup": fact(group)},
        "write_controls": {"Writable": fact(writable)},
        "unknown_properties": extra,
        "effective_resolution": "native_get_tag_info",
        "effective_table_binding": {"kind": "containing_table"},
    }
    return {
        "exiftool_version": "13.59",
        "modules": {"Exif": {"table_count": 1}},
        "modules_ok": 1,
        "modules_failed": 1,
        "load_errors": [{"module": "Broken", "reason": "fixture"}],
        "native_write_capture_context": {"resolved": True, "modules": [{"file": "Image/ExifTool/Writer.pl"}]},
        "native_write_format_registry": {"state": "resolved", "source": {"library_relative_path": "Image/ExifTool/Exif.pm"}},
        "native_write_tables": {
            "Exif": {"Main": {
                "full_name": "Image::ExifTool::Exif::Main",
                "table_properties": {"GROUPS": fact({"0": "EXIF", "1": "IFD0"}), "WRITABLE": fact(1)},
                "write_controls": {"WRITABLE": fact(1), "WRITE_GROUP": fact("IFD0")},
                "unknown_table_properties": {"FutureTableControl": fact({"opaque": True})},
                "effective_write_proc": {"effective": {"source_sha256": "a" * 64}},
                "effective_check_proc": {"effective": {"source_sha256": "b" * 64}},
                "rows": {
                    "316": row("HostComputer", "string", "IFD0", FutureControl=fact({"kept": True})),
                    "320": {"entry_kind": "ARRAY", "alternatives": [
                        row("First", 0, "IFD0"), row("Second", "string", {"__ref": "SCALAR", "value": "IFD1"}),
                    ]},
                    "321": {"entry_kind": "ARRAY", "alternatives": []},
                    "Alias": {"entry_kind": "SCALAR", "value": "not-a-row"},
                },
            }},
        },
    }


class WriteCoverageCatalogTests(unittest.TestCase):
    def test_retains_variants_unknowns_and_null_coverage(self):
        document = sample_dump()
        result = catalog.build_catalog(document, dump_sha256="c" * 64)
        self.assertEqual(result["schema"], catalog.SCHEMA)
        self.assertEqual(result["source"]["capture_completeness"], {
            "native_write_tables": "present",
            "global_module_capture": "partial_modules_failed",
            "native_writer_context": "resolved",
            "native_tiff_type_registry": "resolved",
        })
        self.assertEqual(result["metrics"]["native_definition_tables"], 1)
        self.assertEqual(result["metrics"]["native_definition_raw_rows"], 4)
        self.assertEqual(result["metrics"]["native_definition_variant_records"], 5)
        self.assertIsNone(result["metrics"]["coverage_percentage"])
        self.assertEqual(result["metrics"]["validated_operation_records"], 0)
        self.assertEqual(result["metrics"]["effective_writable_states"], {
            "native_declared_false": 1,
            "native_declared_true": 2,
            "unknown_no_effective_properties": 2,
        })
        second = next(item for item in result["records"] if item["identity"]["tag_name_hint"] == "Second")
        self.assertEqual(second["identity"]["variant_path"], [1])
        self.assertEqual(second["native_definition"]["effective_write_directory"]["state"], "unresolved_nonliteral")
        first = next(item for item in result["records"] if item["identity"]["tag_name_hint"] == "HostComputer")
        self.assertEqual(first["native_definition"]["row_unknown_controls"]["FutureControl"]["value"], {"kept": True})
        self.assertEqual(first["operation_coverage"]["carrier_route"], "not_inferred_from_definition_inventory")
        context = result["table_contexts"][first["identity"]["table_context_ref"]]
        self.assertEqual(context["table_properties"]["WRITABLE"], fact(1))
        self.assertEqual(catalog.resolve_source_variant(result, first),
                         result["source_entries"][first["identity"]["source_entry_ref"]]["entry"])
        empty = next(item for item in result["records"] if item["identity"]["raw_id"] == "321")
        self.assertEqual(empty["identity"]["source_variant_state"], "unresolved_empty_alternatives")
        self.assertEqual(catalog.resolve_source_variant(result, empty)["alternatives"], [])
        self.assertEqual(result["source"]["root_module_facts"]["modules_failed"], {"present": True, "value": 1})

    def test_cli_emits_stable_json_and_raw_dump_hash(self):
        document = sample_dump()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dump = root / "dump.json"
            output_a, output_b = root / "a.json", root / "b.json"
            raw = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
            dump.write_bytes(raw)
            self.assertEqual(catalog.main(["--dump", str(dump), "--output", str(output_a)]), 0)
            self.assertEqual(catalog.main(["--dump", str(dump), "--output", str(output_b)]), 0)
            self.assertEqual(output_a.read_bytes(), output_b.read_bytes())
            result = json.loads(output_a.read_text())
            self.assertEqual(result["source"]["dump_sha256"], hashlib.sha256(raw).hexdigest())

    def test_missing_native_write_sidecar_is_refused(self):
        with self.assertRaisesRegex(catalog.CatalogError, "native_write_tables"):
            catalog.build_catalog({"exiftool_version": "13.59"}, dump_sha256="d" * 64)

    def test_no_module_failures_does_not_claim_global_completeness(self):
        document = sample_dump()
        document["modules_failed"] = 0
        document["load_errors"] = []
        result = catalog.build_catalog(document, dump_sha256="f" * 64)
        self.assertEqual(result["source"]["capture_completeness"]["global_module_capture"],
                         "no_reported_module_failures_scope_unknown")

    def test_shared_context_and_source_entries_are_not_repeated_per_variant(self):
        document = sample_dump()
        body = "large-procedure-body-" * 10_000
        document["native_write_tables"]["Exif"]["Main"]["effective_write_proc"] = {"body": body}
        result = catalog.build_catalog(document, dump_sha256="e" * 64)
        rendered = json.dumps(result, sort_keys=True, separators=(",", ":"))
        self.assertEqual(rendered.count(body), 1)
        self.assertNotIn("source_entry", result["records"][0])
        self.assertNotIn("source_variant", result["records"][0])
        self.assertLess(len(rendered), len(body) + 25_000)


if __name__ == "__main__":
    unittest.main()
