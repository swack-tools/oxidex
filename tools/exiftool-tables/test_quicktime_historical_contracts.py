"""Historical native QuickTime contracts for the generated scalar readers.

The bounded fixtures retain the selected library's effective tables and full
processor/helper bodies. Their capture_scope names the full dump and dump tool.
The 12.64 acceptance is limited to ItemList `data`, direct Keys lookup, and
movie-level UserData explicit Format rows; other ProcessMOV routes stay outside
these generators. 11.78 remains blocked because its reader branches differ.
"""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import quicktime_generated_specs as itemlist
import quicktime_keys_specs as keys
import quicktime_userdata_specs as userdata


HERE = Path(__file__).resolve().parent
COMPILERS = (("ItemList", itemlist), ("Keys", keys), ("UserData", userdata))


def source(version):
    return json.loads((HERE / "fixtures" / f"quicktime_source_{version.replace('.', '_')}.json").read_text())


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

    def test_1178_is_explicitly_blocked_instead_of_emitting_empty_readers(self):
        results = self.compile_all("11.78", source("11.78"))
        self.assertEqual({name: result["identity_counts"]["source_records"] for name, result in results.items()},
                         {"ItemList": 320, "Keys": 55, "UserData": 182})
        for name, result in results.items():
            self.assertEqual(result["identity_counts"]["generated"], 0)
            with self.assertRaisesRegex(ValueError, "source rows but no generated specs"):
                itemlist.require_nonempty_supported(result, name)

    def test_1264_protocol_mutations_fail_closed_and_trigger_empty_guard(self):
        document = source("12.64")
        document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"]["__deparse"] += "\n# changed"
        results = self.compile_all("12.64", document)
        self.assertEqual(results["ItemList"]["identity_counts"]["generated"], 0)
        self.assertEqual(results["Keys"]["identity_counts"]["generated"], 0)
        for name in ("ItemList", "Keys"):
            with self.assertRaises(ValueError):
                itemlist.require_nonempty_supported(results[name], name)

        document = source("12.64")
        document["quicktime_userdata_reader_protocol"]["dependencies"]["Image::ExifTool::FoundTag"]["__deparse"] += "\n# changed"
        result = self.compile_all("12.64", document)["UserData"]
        self.assertEqual(result["identity_counts"]["generated"], 0)
        with self.assertRaises(ValueError):
            itemlist.require_nonempty_supported(result, "UserData")

    def test_current_1359_generation_is_byte_identical(self):
        document = json.loads((HERE / "fixtures/quicktime_source_13_59.json").read_text())
        results = self.compile_all("13.59", document)
        for name, compiler in COMPILERS:
            self.assertGreater(results[name]["identity_counts"]["generated"], 0)
            self.assertEqual(compiler.render_rust(results[name]), compiler.RUST.read_text())


if __name__ == "__main__":
    unittest.main()
