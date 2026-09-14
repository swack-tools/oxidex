import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import quicktime_generated_specs as specs


HERE = Path(__file__).resolve().parent


def snapshot():
    return json.loads((HERE / "fixtures/quicktime_source_13_59.json").read_text())


def fresh_dump():
    document = snapshot()
    document.pop("capture_scope")
    document["modules_failed"] = 0
    return document


class GeneratedItemListSpecsTests(unittest.TestCase):
    def test_new_supported_source_row_generates_rust_without_name_mapping(self):
        document = fresh_dump()
        table = document["modules"]["QuickTime"]["tables"]["ItemList"]
        table["tags"]["z9!?"] = {"Name": "FutureCounter", "Format": "int64u"}
        table["tag_count"] += 1

        result = specs.compile_document(document)

        generated = {row["name"]: row for row in result["specs"]}
        self.assertEqual(generated["FutureCounter"]["raw_fourcc"], "7a39213f")
        self.assertEqual(generated["FutureCounter"]["source_format"], {"kind": "unsigned", "width": 64})
        self.assertIn('name: "FutureCounter"', specs.render_rust(result))
        self.assertEqual(result["identity_counts"], {"source_records": 397, "generated": 92, "omitted": 305})

    def test_cli_generates_from_fresh_dump_without_baseline_capture(self):
        document = fresh_dump()
        table = document["modules"]["QuickTime"]["tables"]["ItemList"]
        table["tags"]["n3w!"] = {"Name": "FreshDumpTag", "Format": "int16u"}
        table["tag_count"] += 1
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            dump, ledger, rust = folder / "fresh.json", folder / "ledger.json", folder / "specs.rs"
            dump.write_text(json.dumps(document))
            run = subprocess.run([sys.executable, str(HERE / "quicktime_generated_specs.py"),
                                  "--dump", str(dump), "--ledger", str(ledger), "--rust", str(rust)],
                                 cwd=specs.ROOT, env={**os.environ, "OXIDEX_ALLOW_DIRTY_TREE": "1"},
                                 text=True, capture_output=True)
            self.assertEqual(run.returncode, 0, run.stderr)
            self.assertIn('name: "FreshDumpTag"', rust.read_text())
            self.assertEqual(json.loads(ledger.read_text())["identity_counts"]["generated"], 92)

    def test_callback_or_format_change_is_omitted_from_generated_specs(self):
        document = snapshot()
        row = document["modules"]["QuickTime"]["tables"]["ItemList"]["tags"]["plID"]
        row["ValueConv"] = {"kind": "perl", "expr": "$val + 1"}

        result = specs.compile_document(document)

        self.assertNotIn("AlbumID", {row["name"] for row in result["specs"]})
        entry = next(row for row in result["ledger"]
                     if row["identity"]["table"] == "ItemList" and row["identity"]["raw_key"] == "plID")
        self.assertFalse(entry["generated"])
        self.assertIn("unsupported_source_property:ValueConv", entry["reasons"])

    def test_changed_processor_contract_refuses_all_itemlist_rows(self):
        document = snapshot()
        document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"]["__name"] = "Other::Process"

        result = specs.compile_document(document)

        self.assertEqual(result["specs"], [])
        itemlist = [row for row in result["ledger"] if row["identity"]["table"] == "ItemList"]
        self.assertEqual(len(itemlist), 105)
        self.assertTrue(all("missing_or_changed_processor_contract:PROCESS_PROC" in row["reasons"]
                            for row in itemlist))

    def test_processor_source_identity_is_provenance_not_eligibility(self):
        document = snapshot()
        document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"]["source_sha256"] = "0" * 64

        result = specs.compile_document(document)

        self.assertEqual(len(result["specs"]), 91)
        self.assertIsNone(result["protocol"]["reason"])

    def test_changed_processor_body_refuses_without_renaming_it(self):
        document = snapshot()
        document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"]["__deparse"] += " # changed"

        result = specs.compile_document(document)

        self.assertEqual(result["specs"], [])
        self.assertEqual(result["protocol"]["reason"], "missing_or_changed_processor_contract:PROCESS_PROC")

    def test_changed_quicktime_format_helper_refuses_all_itemlist_rows(self):
        document = snapshot()
        document["quicktime_itemlist_reader_protocol"]["dependencies"]["quicktime_format"]["__deparse"] += " # changed"

        result = specs.compile_document(document)

        self.assertEqual(result["specs"], [])
        self.assertEqual(result["protocol"]["reason"],
                         "missing_or_changed_reader_protocol:quicktime_format")
        itemlist = [row for row in result["ledger"] if row["identity"]["table"] == "ItemList"]
        self.assertTrue(all("missing_or_changed_reader_protocol:quicktime_format" in row["reasons"]
                            for row in itemlist))

    def test_changed_string_encoding_data_refuses_all_itemlist_rows(self):
        document = snapshot()
        document["quicktime_itemlist_reader_protocol"]["string_encoding"]["3"] = "UTF8"

        result = specs.compile_document(document)

        self.assertEqual(result["specs"], [])
        self.assertEqual(result["protocol"]["reason"],
                         "missing_or_changed_reader_protocol:string_encoding")

    def test_reader_dependency_source_hash_is_provenance_not_eligibility(self):
        document = snapshot()
        document["quicktime_itemlist_reader_protocol"]["dependencies"]["read_value"]["source_sha256"] = "0" * 64

        result = specs.compile_document(document)

        self.assertEqual(len(result["specs"]), 91)
        self.assertIsNone(result["protocol"]["reason"])

    def test_rust_output_escapes_source_strings(self):
        document = snapshot()
        row = document["modules"]["QuickTime"]["tables"]["ItemList"]["tags"]["plID"]
        row["Name"] = 'Quoted"Name\\Path\nNext'
        row["PrintConv"] = {"kind": "enum", "directives": None,
                            "map": {"1": 'line\nwith "quote" and \\ slash'}}

        rendered = specs.render_rust(specs.compile_document(document))

        self.assertIn('name: "Quoted\\"Name\\\\Path\\nNext"', rendered)
        self.assertIn('rendered: "line\\nwith \\"quote\\" and \\\\ slash"', rendered)

    def test_committed_artifacts_are_current_and_preserve_every_source_identity(self):
        result = specs.compile_document(snapshot())
        self.assertEqual(specs.serialized(result),
                         (HERE / "quicktime_generated_itemlist_ledger.json").read_text())
        self.assertEqual(specs.render_rust(result),
                         (specs.ROOT / "src/parsers/quicktime/generated_itemlist_specs.rs").read_text())
        self.assertEqual(len({json.dumps(row["identity"], sort_keys=True) for row in result["ledger"]}),
                         result["identity_counts"]["source_records"])
        self.assertEqual(result["identity_counts"], {"source_records": 396, "generated": 91, "omitted": 305})
        self.assertEqual(sum(not row["generated"] for row in result["ledger"]
                             if row["identity"]["table"] == "ItemList"), 14)

    def test_committed_artifact_keeps_selected_table_identity_not_full_dump_provenance(self):
        raw = (HERE / "fixtures/quicktime_source_13_59.json").read_bytes()
        result = specs.compile_document(json.loads(raw))
        source_ledger = json.loads((HERE / "quicktime_source_capabilities.json").read_text())
        self.assertNotIn("dump_sha256", result["source"])
        self.assertEqual(result["source"]["table_sha256"],
                         {family["table"]: family["source_table_sha256"]
                          for family in source_ledger["families"]})


if __name__ == "__main__":
    unittest.main()
