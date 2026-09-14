import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

PATH = Path(__file__).with_name("join_catalog_hydrated.py")
spec = importlib.util.spec_from_file_location("join_catalog_hydrated", PATH)
join = importlib.util.module_from_spec(spec)
spec.loader.exec_module(join)

TABLE = "Image::ExifTool::QuickTime::ItemList"
SOURCE = {"Image/ExifTool.pm": {"library_relative_path": "Image/ExifTool.pm", "sha256": "a"}}


def entry(name="Title", raw="titl", variant=0):
    return {"table": TABLE, "raw_key": raw, "variant_index": variant, "name": name,
            "normalized_name": name.lower(), "groups": {"0": "QuickTime", "1": "ItemList", "2": "Audio"},
            "no_lookup": False, "unknown": False}


def catalog(entries):
    names = sorted({item["normalized_name"] for item in entries})
    return {"schema": join.CATALOG_SCHEMA, "exiftool_version": "13.59", "entries": entries,
            "unique_names": names, "producer": {"sources": copy.deepcopy(SOURCE)},
            "counts": {"catalog_total_tag_entries": len(entries),
                       "distinct_case_insensitive_entry_names": len(names), "catalog_unique_tag_names": len(names)}}


def hydrated(tags, sources=SOURCE, total=1):
    return {"exiftool_version": "13.59", "hydrated_layouts": {
        "catalog_counts": {"total_tag_entries": total}, "source_provenance": {"sources": copy.deepcopy(sources)},
        "tables": {TABLE: {"full_name": TABLE, "tags": tags}}}}


class CatalogHydratedJoinTests(unittest.TestCase):
    def quicktime_facts(self, source, *, generated, reasons):
        identity = {"module": "QuickTime", "table": "ItemList", "raw_key": "titl",
                    "source_sha256": join.canonical_hash(source), "variant_path": []}
        return ({"schema": "quicktime_generated_itemlist_specs_v1",
                 "ledger": [{"identity": identity, "generated": generated, "reasons": reasons}]},
                {"families": [{"records": [{"identity": identity, "reasons": reasons}]}]})

    def test_exact_coordinate_variant_name_and_hash_join(self):
        result = join.build(catalog([entry("Title", "titl", 0), entry("Alternate", "titl", 1)]),
                            hydrated({"titl": {"_variants": [{"Name": "Title"}, {"Name": "Alternate"}]}}, total=2), "catalog", "hydrated")
        self.assertEqual(result["counts"]["status"], {"source_row_joined": 2})
        self.assertEqual(result["entries"][0]["source"]["state"], "joined")
        self.assertRegex(result["entries"][0]["source"]["row_sha256"], r"^[0-9a-f]{64}$")

    def test_name_conflict_and_absence_are_truthful(self):
        result = join.build(catalog([entry("Title"), entry("Missing", "miss")]),
                            hydrated({"titl": {"Name": "Different"}}, total=2), "c", "h")
        self.assertEqual(result["counts"]["status"], {"source_row_absent": 1, "source_row_name_conflict": 1})
        self.assertEqual({row["catalog"]["name"]: row["source"]["state"] for row in result["entries"]},
                         {"Title": "conflict", "Missing": "absent"})

    def test_quicktime_exact_identity_joins_generated_and_refused_selector_facts(self):
        source = {"Name": "Title"}
        ledger, capabilities = self.quicktime_facts(source, generated=True, reasons=[])
        result = join.build(catalog([entry()]), hydrated({"titl": source}), "c", "h", ledger, capabilities)
        self.assertEqual(result["entries"][0]["source_derived_implementation"],
                         "generated_reader_declaration_unobserved")
        ledger, capabilities = self.quicktime_facts(source, generated=False, reasons=["unsupported:conversion"])
        result = join.build(catalog([entry()]), hydrated({"titl": source}), "c", "h", ledger, capabilities)
        self.assertEqual(result["entries"][0]["source_derived_implementation"], "blocked_generated_reader_refusal")
        self.assertEqual(result["entries"][0]["implementation_refusal_reasons"], ["unsupported:conversion"])

    def test_quicktime_name_or_hash_collision_cannot_consume_a_source_row(self):
        source = {"Name": "Title"}
        ledger, capabilities = self.quicktime_facts({"Name": "Other"}, generated=True, reasons=[])
        result = join.build(catalog([entry()]), hydrated({"titl": source}), "c", "h", ledger, capabilities)
        self.assertEqual(result["entries"][0]["source_derived_implementation"], "source_row_not_yet_consumed")
        self.assertEqual(result["entries"][0]["observed_read"], "not_observed_yet")

    def test_rejects_malformed_native_denominators_and_names(self):
        bad = catalog([entry()])
        bad["counts"]["catalog_total_tag_entries"] = 2
        with self.assertRaisesRegex(ValueError, "catalog_total_tag_entries"):
            join.build(bad, hydrated({"titl": {"Name": "Title"}}), "c", "h")
        bad = catalog([entry()])
        bad["unique_names"] = []
        with self.assertRaisesRegex(ValueError, "unique_names conservation"):
            join.build(bad, hydrated({"titl": {"Name": "Title"}}), "c", "h")

    def test_rejects_missing_or_mismatched_source_manifest(self):
        with self.assertRaisesRegex(ValueError, "absent from hydrated manifest"):
            join.build(catalog([entry()]), hydrated({"titl": {"Name": "Title"}}, {}), "c", "h")
        with self.assertRaisesRegex(ValueError, "source provenance mismatch"):
            join.build(catalog([entry()]), hydrated({"titl": {"Name": "Title"}},
                                                     {"Image/ExifTool.pm": {"sha256": "wrong"}}), "c", "h")

    def test_cli_check_compares_existing_outputs_and_never_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            catalog_path, hydrated_path = root / "catalog.json", root / "hydrated.json"
            output, report = root / "join.json", root / "join.md"
            catalog_path.write_text(json.dumps(catalog([entry()])))
            hydrated_path.write_text(json.dumps(hydrated({"titl": {"Name": "Title"}})))
            command = ["python3", str(PATH), "--catalog", str(catalog_path), "--hydrated", str(hydrated_path),
                       "--output", str(output), "--report", str(report)]
            self.assertEqual(subprocess.run(command).returncode, 0)
            self.assertEqual(subprocess.run(command + ["--check"]).returncode, 0)
            output.write_text("stale\n")
            self.assertNotEqual(subprocess.run(command + ["--check"]).returncode, 0)
            self.assertEqual(output.read_text(), "stale\n")

    def test_cli_rejects_output_report_and_hardlink_aliases(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            catalog_path, hydrated_path = root / "catalog.json", root / "hydrated.json"
            catalog_path.write_text(json.dumps(catalog([entry()])))
            hydrated_path.write_text(json.dumps(hydrated({"titl": {"Name": "Title"}})))
            command = ["python3", str(PATH), "--catalog", str(catalog_path), "--hydrated", str(hydrated_path),
                       "--output", str(root / "same"), "--report", str(root / "same")]
            self.assertNotEqual(subprocess.run(command).returncode, 0)
            hardlink = root / "catalog-link.json"
            os.link(catalog_path, hardlink)
            command[-3] = str(hardlink)
            command[-1] = str(root / "report.md")
            self.assertNotEqual(subprocess.run(command).returncode, 0)


if __name__ == "__main__":
    unittest.main()
