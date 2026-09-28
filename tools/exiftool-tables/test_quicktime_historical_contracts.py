"""Historical native QuickTime contracts for the generated scalar readers.

The bounded fixtures retain the selected library's effective tables and full
processor/helper bodies. Their capture_scope names the full dump and dump tool.
Acceptance is limited to ItemList `data`, direct Keys lookup, and movie-level
UserData explicit Format rows; other ProcessMOV routes stay outside these
generators. Reader differences are emitted as source-selected capabilities.
"""

import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import quicktime_generated_specs as itemlist
import quicktime_keys_specs as keys
import quicktime_userdata_specs as userdata
import quicktime_protocol_caps as caps
from quicktime_test_sources import selected_document
from quicktime_test_sources import source_document


COMPILERS = (("ItemList", itemlist), ("Keys", keys), ("UserData", userdata))


def source(version):
    return source_document(version)


class HistoricalContracts(unittest.TestCase):
    def compile_all(self, version, document):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".exiftool-version").write_text(version + "\n")
            with patch.object(itemlist, "ROOT", root), patch.object(keys, "ROOT", root), patch.object(userdata, "ROOT", root):
                return {name: compiler.compile_document(document) for name, compiler in COMPILERS}

    def test_1264_emits_native_source_rows_for_all_three_readers(self):
        document = source("12.64")
        results = self.compile_all("12.64", document)
        self.assertEqual({name: result["identity_counts"] for name, result in results.items()}, {
            "ItemList": {"source_records": 352, "generated": 89, "omitted": 263},
            "Keys": {"source_records": 60, "generated": 53, "omitted": 7},
            "UserData": {"source_records": 192, "generated": 17, "omitted": 175},
        })
        self.assertTrue({"Compilation", "Title", "Artist", "Copyright"} <=
                        {row["name"] for row in results["ItemList"]["specs"]})
        self.assertIn("Artist", {row["name"] for row in results["Keys"]["specs"]})
        self.assertIn("CompressorVersion", {row["name"] for row in results["UserData"]["specs"]})
        for name, compiler in COMPILERS:
            self.assertIsNone(results[name]["protocol"]["reason"])
            itemlist.require_nonempty_supported(results[name], name)
        for name, table in (("ItemList", "ItemList"), ("Keys", "Keys")):
            recorded = results[name]["protocol"]["processor_contract" if name == "ItemList" else "processor"]["__deparse_sha256"]
            body = document["modules"]["QuickTime"]["tables"][table]["meta"]["PROCESS_PROC"]["__deparse"]
            self.assertEqual(recorded, hashlib.sha256(body.encode()).hexdigest())

    def test_absent_itemlist_does_not_trigger_other_families_guard(self):
        document = source("12.64")
        table = document["modules"]["QuickTime"]["tables"]["ItemList"]
        table["tags"] = {}
        table["tag_count"] = 0
        result = self.compile_all("12.64", document)["ItemList"]
        self.assertEqual(result["identity_counts"]["source_records"], 252)
        self.assertEqual(result["identity_counts"]["generated"], 0)
        itemlist.require_nonempty_supported(result, "ItemList")

    def test_1178_emits_reviewed_native_subset(self):
        document = source("11.78")
        results = self.compile_all("11.78", document)
        self.assertEqual({name: result["identity_counts"]["source_records"] for name, result in results.items()},
                         {"ItemList": 320, "Keys": 55, "UserData": 182})
        self.assertEqual({name: result["identity_counts"]["generated"] for name, result in results.items()},
                         {"ItemList": 73, "Keys": 48, "UserData": 17})
        self.assertIn("PlayListID", {row["name"] for row in results["ItemList"]["specs"]})
        for name, result in results.items():
            self.assertIsNone(result["protocol"]["reason"], name)
            itemlist.require_nonempty_supported(result, name)
        for name, table in (("ItemList", "ItemList"), ("Keys", "Keys")):
            recorded = results[name]["protocol"]["processor_contract" if name == "ItemList" else "processor"]["__deparse_sha256"]
            body = document["modules"]["QuickTime"]["tables"][table]["meta"]["PROCESS_PROC"]["__deparse"]
            self.assertEqual(recorded, hashlib.sha256(body.encode()).hexdigest())

    def test_1264_protocol_mutations_fail_closed_and_trigger_empty_guard(self):
        document = source("12.64")
        document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"]["__deparse"] += "\n# changed"
        results = self.compile_all("12.64", document)
        self.assertEqual(results["ItemList"]["identity_counts"]["generated"], 0)
        self.assertEqual(results["Keys"]["identity_counts"]["generated"], 0)
        for name in ("ItemList", "Keys"):
            with self.assertRaisesRegex(ValueError, "100 source rows" if name == "ItemList" else "60 source rows"):
                itemlist.require_nonempty_supported(results[name], name)

        document = source("12.64")
        document["quicktime_userdata_reader_protocol"]["dependencies"]["Image::ExifTool::FoundTag"]["__deparse"] += "\n# changed"
        result = self.compile_all("12.64", document)["UserData"]
        self.assertEqual(result["identity_counts"]["generated"], 0)
        with self.assertRaises(ValueError):
            itemlist.require_nonempty_supported(result, "UserData")

    def test_selected_generation_is_byte_identical(self):
        document = selected_document()
        version = document["exiftool_version"]
        results = self.compile_all(version, document)
        for name, compiler in COMPILERS:
            self.assertGreater(results[name]["identity_counts"]["generated"], 0)
            self.assertEqual(compiler.render_rust(results[name]), compiler.RUST.read_text())
        for name in ("ItemList", "Keys"):
            recorded = results[name]["protocol"]["processor_contract" if name == "ItemList" else "processor"]["__deparse_sha256"]
            body = document["modules"]["QuickTime"]["tables"][name]["meta"]["PROCESS_PROC"]["__deparse"]
            self.assertEqual(recorded, hashlib.sha256(body.encode()).hexdigest())


if __name__ == "__main__":
    unittest.main()
