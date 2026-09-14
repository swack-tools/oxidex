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


def add_menu_settings_z8v2(data, hook=generator.MENU_SETTINGS_Z8V2_HOOK):
    tables = data["modules"]["Nikon"]["tables"]
    tables["Test"]["tags"]["2"] = {
        "Name": "NestedMenuSettings",
        "SubDirectory": {"TagTable": "Image::ExifTool::Nikon::MenuSettingsZ8v2"},
    }
    tables["MenuSettingsZ8v2"] = {
        "meta": {"FORMAT": "int8u"},
        "tags": {
            "0": {
                "Name": "MenuSettingsZ8",
                "Hook": hook,
                "SubDirectory": {"TagTable": "Image::ExifTool::Nikon::MenuSettingsZ8"},
            },
        },
    }
    tables["MenuSettingsZ8"] = {"meta": {"FORMAT": "int8u"}, "tags": {}}


class NikonEncryptedGeneratorTests(unittest.TestCase):
    def test_current_dump_code_provenance_is_accepted_but_unknown_fields_refuse(self):
        body = "registered test body"
        pair = ("Image::ExifTool::Test", generator.hashlib.sha256(body.encode()).hexdigest())
        generator.CODE.add(pair)
        self.addCleanup(generator.CODE.remove, pair)
        code = {
            "__perl": "CODE", "__opaque": True,
            "__name": pair[0], "__deparse": body,
            "resolved": True, "source_file": "Image/ExifTool.pm",
            "source_sha256": "0" * 64,
            "dependencies": {"Image::ExifTool::Get16u": {"resolved": True}},
        }
        self.assertEqual(generator.code(code), pair[0])
        code["unrecognized"] = True
        with self.assertRaises(generator.Unsupported):
            generator.code(code)

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

    def test_fixed_divisor_print_conversions_refuse_source_mutations(self):
        def pc(expression):
            return generator.pc({"PrintConv": {"kind": "expr", "expr": expression}}, {})

        self.assertEqual(pc('sprintf("f/%.1f",$val/100)'), "Pc::FNumberDiv100")
        self.assertEqual(pc('sprintf("%.1fmm",$val/10)'), "Pc::MmDiv(10.0)")
        self.assertEqual(pc('sprintf("%.1f mm",$val)'), 'Pc::FixedSuffix(1, " mm")')
        self.assertEqual(pc('sprintf("%.1f m", $val/10)'), "Pc::MetersDiv10")
        for expression in ('sprintf("f/%.1f",$val/200)',
                           'sprintf("%.1f m", $val/20)',
                           'sprintf("f/%x1f",$val/100)',
                           'sprintf("%x1fmm",$val/10)',
                           'sprintf("%x1f mm",$val)',
                           'sprintf("%x1f m", $val/10)'):
            with self.subTest(expression=expression):
                with self.assertRaises(generator.Unsupported):
                    pc(expression)

    def test_root_count_guard_requires_complete_native_grammar(self):
        data = fixture()
        root = data["modules"]["Nikon"]["tables"]["Main"]["tags"]["145"]
        root["Condition"] = "$$valPt =~ /^0210/ and $count == 5399"
        text, _ = generator.render(data)
        self.assertIn("counts: &[5399]", text)

        root["Condition"] = "$$valPt =~ /^0210/ and ($count == 5408 or $count == 5412)"
        text, _ = generator.render(data)
        self.assertIn("counts: &[5408, 5412]", text)

        root["Condition"] = "$$valPt =~ /^0210/ and $count == 5399 and 0"
        with self.assertRaises(generator.Unsupported):
            generator.render(data)

    def test_menu_settings_z8v2_hook_requires_exact_pinned_body_and_location(self):
        data = fixture()
        add_menu_settings_z8v2(data)
        text, _ = generator.render(data)
        self.assertIn("hook: Hook::MenuSettingsZ8v2", text)

        changed = copy.deepcopy(data)
        changed["modules"]["Nikon"]["tables"]["MenuSettingsZ8v2"]["tags"]["0"]["Hook"] += "\n$varSize += 99"
        with self.assertRaises(generator.Unsupported):
            generator.render(changed)

        changed = fixture()
        changed["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]["Hook"] = generator.MENU_SETTINGS_Z8V2_HOOK
        with self.assertRaises(generator.Unsupported):
            generator.render(changed)

        changed = copy.deepcopy(data)
        z8v2 = changed["modules"]["Nikon"]["tables"]["MenuSettingsZ8v2"]["tags"]
        z8v2["1"] = z8v2.pop("0")
        with self.assertRaises(generator.Unsupported):
            generator.render(changed)


if __name__ == "__main__":
    unittest.main()
