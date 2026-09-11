"""Guard upgrade refusals, variant order and whole-file output preservation."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import gen_nikon_settings_tables as generator


def expr(text):
    return {"kind": "expr", "expr": text}


def fixture():
    # Concrete pinned shapes; no inputs or expectations come from the registry.
    return {"exiftool_version": (Path(__file__).resolve().parents[2] / ".exiftool-version").read_text().strip(),
            "modules": {"NikonSettings": {"tables": {"Main": {
                "meta": {"GROUPS": {"0": "MakerNotes", "2": "Camera"},
                         "PROCESS_PROC": {"__name": "Image::ExifTool::NikonSettings::ProcessNikonSettings"}},
                "tags": {"1": {"_variants": [
                    {"Name": "ISOAutoHiLimit", "Condition": r"$$self{Model} =~ /^NIKON D6\b/i",
                     "PrintConv": {"kind": "enum", "directives": None,
                                   "map": {"33": "ISO 25600", "1": "ISO 200"}}},
                    {"Name": "ISOAutoHiLimit", "Condition": r"$$self{Model} =~ /^NIKON Z (7|7_2)\b/i",
                     "PrintConv": {"kind": "enum", "directives": None,
                                   "map": {"33": "ISO 25600", "1": "ISO 100"}}},
                ]}, "287": {"Name": "CHModeShootingSpeed", "ValueConv": expr("15 - $val"),
                            "PrintConv": expr('"$val fps"')},
                         "88": {"Name": "CmdDialsReverseRotExposureComp", "Unknown": "1",
                                "RawConv": expr("$$self{CmdDialsReverseRotExposureComp} = $val")}},
            }}}}}


def tags(data):
    return data["modules"]["NikonSettings"]["tables"]["Main"]["tags"]


class NikonSettingsGeneratorTests(unittest.TestCase):
    def test_ordered_alternatives_maps_and_unknown_state_omission(self):
        text, counts = generator.render(fixture())
        self.assertEqual(counts, {"rows": 3, "maps": 2, "unknown_omitted": 1,
                                  "state_projections": [], "mask_residuals": []})
        self.assertLess(text.index("cond: Cond::ModelD6"), text.index("cond: Cond::ModelZ7"))
        self.assertIn('[(1, "ISO 200"), (33, "ISO 25600")]', text)
        self.assertIn("conv: Conv::Fps(15)", text)
        self.assertNotIn("CmdDialsReverseRotExposureComp", text)

    def test_changed_enum_names_and_new_rows_are_generated(self):
        data = fixture()
        row = tags(data)["1"]["_variants"][0]
        row["Name"] = "RenamedISO"
        row["PrintConv"]["map"]["33"] = "Changed native label"
        tags(data)["65000"] = {"Name": "NewRawSetting"}
        text, _ = generator.render(data)
        self.assertIn('name: "RenamedISO"', text)
        self.assertIn('(33, "Changed native label")', text)
        self.assertIn('id: 0xfde8, name: "NewRawSetting"', text)

    def test_every_changed_executable_shape_refuses(self):
        mutations = [
            ("Condition", r"$$self{Model} =~ /^NIKON  D6\b/i"),
            ("Condition", r"$$self{Model} =~ /^NIKON D6\b/"),
            ("RawConv", expr("$$self{HDMIBitDepth} = $val + 1")),
            ("ValueConv", expr("$val + 1")),
            ("Hook", "return 1"), ("_extra_keys", ["NewExecutableField"]),
            ("BitShift", "1"), ("Format", "int16u"),
            ("Mask", "15"),
            ("Name", ""),
            ("PrintConv", {"kind": "enum", "map": {"1": "One"},
                           "directives": {"OTHER": "fallback"}}),
        ]
        for key, value in mutations:
            with self.subTest(field=key):
                data = fixture()
                tags(data)["1"]["_variants"][0][key] = value
                with self.assertRaises(generator.Unsupported):
                    generator.render(data)

    def test_printconv_never_hides_a_changed_valueconv(self):
        for old, new in (("15 - $val", "16 - $val"), ('"$val fps"', '"$val  fps"')):
            data = fixture()
            field = "ValueConv" if old.startswith("15") else "PrintConv"
            tags(data)["287"][field] = expr(new)
            with self.assertRaises(generator.Unsupported):
                generator.render(data)

    def test_state_projection_is_exact_and_reported(self):
        data = fixture()
        row = {"Name": "AFAreaMode", "RawConv": expr("$$self{AFAreaMode} = $val")}
        tags(data)["366"] = row
        text, counts = generator.render(data)
        self.assertEqual(len(counts["state_projections"]), 1)
        self.assertIn('id: 0x016e, name: "AFAreaMode", cond: Cond::Always, mask: 0x0, conv: Conv::Raw, dm: Dm::None', text)
        for replacement in ({"Name": "Other"}, {"RawConv": expr("$$self{AFAreaMode} = 1")}):
            changed = copy.deepcopy(data)
            tags(changed)["366"].update(replacement)
            with self.assertRaises(generator.Unsupported):
                    generator.render(changed)

    def test_existing_mask_is_reported_but_new_interpretations_refuse(self):
        data = fixture()
        row = {"Name": "BracketProgram", "Condition": "$$self{BracketSet} and $$self{BracketSet} == 5",
               "Mask": "15"}
        tags(data)["266"] = row
        _, counts = generator.render(data)
        self.assertEqual(len(counts["mask_residuals"]), 1)
        row["Mask"] = "31"
        with self.assertRaisesRegex(generator.Unsupported, "new mask"):
            generator.render(data)

    def test_layout_and_numeric_aliases_refuse(self):
        for key in ("276.1", "01", "65536", "-1"):
            data = fixture()
            tags(data)[key] = {"Name": "InvalidID"}
            with self.assertRaises(generator.Unsupported):
                generator.render(data)
        data = fixture()
        data["modules"]["NikonSettings"]["tables"]["Main"]["meta"]["FORMAT"] = "int16u"
        with self.assertRaises(generator.Unsupported):
            generator.render(data)

    def test_unknown_alternative_must_not_expose_later_fallback(self):
        data = fixture()
        variants = tags(data)["1"]["_variants"]
        variants[0]["Unknown"] = "1"
        variants[1].pop("Condition")
        with self.assertRaisesRegex(generator.Unsupported, "mixed known/Unknown"):
            generator.render(data)

    def test_unknown_condition_cannot_hide_a_state_change(self):
        data = fixture()
        tags(data)["88"]["Condition"] = "$$self{HDMIBitDepth} = 2"
        with self.assertRaisesRegex(generator.Unsupported, "Condition"):
            generator.render(data)

    def test_cli_refusal_preserves_existing_output(self):
        for failure in ("version", "conversion", "unicode", "unknown-fallback"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                data = fixture()
                if failure == "version":
                    data["exiftool_version"] = "0.00"
                elif failure == "conversion":
                    tags(data)["287"]["ValueConv"] = expr("$val / 0")
                elif failure == "unicode":
                    tags(data)["287"]["Name"] = "bad\ud800"
                else:
                    tags(data)["1"]["_variants"][0]["Unknown"] = "1"
                source, output = Path(directory) / "source.json", Path(directory) / "out.rs"
                source.write_text(json.dumps(data))
                output.write_text("preserve this output\n")
                result = subprocess.run([sys.executable, generator.__file__, str(source), "-o", str(output)],
                                        capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output.read_text(), "preserve this output\n")


if __name__ == "__main__":
    unittest.main()
