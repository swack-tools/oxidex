"""Controls for the native Nikon encrypted-table projection."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import gen_nikon_encrypted_tables as generator


def enum(mapping):
    return {"kind": "enum", "map": mapping, "directives": None}


def fixture():
    roots = {}
    for ident, name in (("145", "ShotInfo"), ("151", "ColorBalance"),
                        ("152", "LensData")):
        roots[ident] = {"Name": name, "Condition": "$$valPt =~ /^0100/",
                        "SubDirectory": {"TagTable": "Image::ExifTool::Nikon::Test",
                                         "DecryptStart": "0"}}
    return {
        "exiftool_version": "13.59",
        "modules": {
            "Nikon": {"tables": {
                "Main": {"meta": {}, "tags": roots},
                "Test": {"meta": {"FORMAT": "int8u"}, "tags": {
                    "1": {"Name": "Mode", "PrintConv": enum({"2": "Two", "10": "Ten", "1": "One"})},
                }},
            }},
            "NikonCustom": {"tables": {"Unused": {"meta": {"FORMAT": "int8u"}, "tags": {}}}},
        },
    }


class NikonEncryptedGeneratorTests(unittest.TestCase):
    def test_native_rows_maps_and_numeric_order_project(self):
        text, counts = generator.render(fixture())
        self.assertEqual(counts, {"tables": 1, "rows": 1, "maps": 1})
        self.assertIn('[("1", "One"), ("10", "Ten"), ("2", "Two"),]', text)
        self.assertIn('name: "Mode"', text)
        self.assertIn('pub static SHOT_INFO_ROOTS', text)

    def test_supported_native_name_enum_and_row_updates_regenerate(self):
        data = fixture()
        row = data["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]
        row["Name"] = "RenamedMode"
        row["PrintConv"]["map"]["2"] = "Changed native label"
        data["modules"]["Nikon"]["tables"]["Test"]["tags"]["20"] = {"Name": "NewNativeRow"}
        text, counts = generator.render(data)
        self.assertIn('name: "RenamedMode"', text)
        self.assertIn('("2", "Changed native label")', text)
        self.assertIn('index: 20, frac: 0, name: "NewNativeRow"', text)
        self.assertEqual(counts["rows"], 2)

    def test_changed_code_body_refuses_before_destination_overwrite(self):
        data = fixture()
        root = data["modules"]["Nikon"]["tables"]["Main"]["tags"]["145"]
        root["SubDirectory"]["ProcessProc"] = {
            "__perl": "CODE", "__opaque": 1,
            "__name": "Image::ExifTool::Nikon::ProcessNikonEncrypted",
            "__deparse": "altered native body",
        }
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.json"
            output = Path(directory) / "out.rs"
            source.write_text(json.dumps(data))
            output.write_text("preserve this output\n")
            result = subprocess.run([sys.executable, generator.__file__, str(source), "-o", str(output)],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unregistered native CODE body", result.stderr)
            self.assertEqual(output.read_text(), "preserve this output\n")

    def test_unknown_executable_and_schema_changes_refuse(self):
        data = fixture()
        row = data["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]
        for field, value in [
            ("ValueConv", {"kind": "expr", "expr": "$val * 17"}),
            ("PrintConv", {"kind": "expr", "expr": 'system("bad")'}),
            ("Offset", "2"),
        ]:
            with self.subTest(field=field):
                changed = copy.deepcopy(data)
                changed["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"][field] = value
                with self.assertRaises(generator.Unsupported):
                    generator.render(changed)


if __name__ == "__main__":
    unittest.main()
