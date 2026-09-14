import copy
import json
from pathlib import Path
import unittest

import quicktime_generated_specs as specs


HERE = Path(__file__).resolve().parent


def snapshot():
    return json.loads((HERE / "fixtures/quicktime_source_13_59.json").read_text())


class GeneratedItemListSpecsTests(unittest.TestCase):
    def test_new_supported_source_row_generates_rust_without_name_mapping(self):
        document = snapshot()
        table = document["modules"]["QuickTime"]["tables"]["ItemList"]
        table["tags"]["z9!?"] = {"Name": "FutureCounter", "Format": "int64u"}
        table["tag_count"] += 1

        result = specs.compile_document(document)

        generated = {row["name"]: row for row in result["specs"]}
        self.assertEqual(generated["FutureCounter"]["raw_fourcc"], "7a39213f")
        self.assertEqual(generated["FutureCounter"]["source_format"], {"kind": "unsigned", "width": 64})
        self.assertIn('name: "FutureCounter"', specs.render_rust(result))
        self.assertEqual(result["identity_counts"], {"source_records": 397, "generated": 92, "omitted": 305})

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

    def test_processor_source_identity_change_refuses_without_renaming_it(self):
        document = snapshot()
        document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"]["source_sha256"] = "0" * 64

        result = specs.compile_document(document)

        self.assertEqual(result["specs"], [])
        self.assertEqual(result["protocol"]["reason"], "missing_or_changed_processor_contract:PROCESS_PROC")

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

    def test_committed_artifact_keeps_the_source_snapshot_digest(self):
        raw = (HERE / "fixtures/quicktime_source_13_59.json").read_bytes()
        result = specs.compile_document(json.loads(raw), raw=raw)
        source_ledger = json.loads((HERE / "quicktime_source_capabilities.json").read_text())
        self.assertEqual(result["source"]["dump_sha256"], source_ledger["source"]["dump_sha256"])


if __name__ == "__main__":
    unittest.main()
