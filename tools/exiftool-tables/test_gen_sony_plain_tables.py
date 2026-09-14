"""Finite Sony producer controls independent of the registry and generated Rust."""
import copy
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

import gen_sony_plain_tables as generator


def expr(text):
    return {"kind": "expr", "expr": text}


def enum(mapping, directives=None):
    return {"kind": "enum_partial" if directives else "enum",
            "map": mapping, "directives": directives}


def fixture():
    # Independent loaded-table shapes: deliberately small data, not a dump of
    # the producer's registries and not a read of the Rust output.
    tables = {}
    for name in ("CameraSettings", "CameraSettings2", "CameraSettings3",
                 "FaceInfo1", "FaceInfo2", "ShotInfo"):
        meta = {"PROCESS_PROC": {"__perl": "CODE", "__opaque": 1,
                                 "__name": "Image::ExifTool::ProcessBinaryData",
                                 "__deparse": "processor body is engine-oracle scope"},
                "FIRST_ENTRY": "0",
                "GROUPS": {"0": "MakerNotes", "2": "Camera" if name.startswith("CameraSettings") else "Image"}}
        if name.startswith("CameraSettings"):
            meta.update(FORMAT="int8u" if name == "CameraSettings3" else "int16u", PRIORITY="0")
        if name == "CameraSettings3":
            meta["DATAMEMBER"] = ["153"]
        if name == "ShotInfo":
            meta.update(DATAMEMBER=["2", "48", "50", "52"], IS_SUBDIR=["72", "94"])
        tables[name] = {"full_name": "Image::ExifTool::Sony::" + name,
                        "meta": meta, "tag_count": 1,
                        "tags": {"1": {"Name": "Sentinel", "PrintConv": enum({"2": "Two", "10": "Ten", "1": "One"})}}}
    pin = (Path(__file__).resolve().parents[2] / ".exiftool-version").read_text().strip()
    return {"exiftool_version": pin, "modules": {"Sony": {"tables": tables}}}


def table(data, name="CameraSettings"):
    return data["modules"]["Sony"]["tables"][name]


def add(data, key, row, name="CameraSettings"):
    t = table(data, name)
    t["tags"][key] = row
    t["tag_count"] = len(t["tags"])


def collision_fixture():
    data = fixture()
    cond = "$$self{Model} !~ /^DSLR-(A450|A500|A550)$/"
    add(data, "276", {"Name": "FolderNumber", "Condition": cond, "Format": "int32u", "Mask": "16760832",
                      "PrintConv": expr('sprintf("%.3d",$val)')}, "CameraSettings3")
    add(data, "276.1", {"Name": "ImageNumber", "Condition": cond, "Format": "int32u", "Mask": "16383",
                        "PrintConv": expr('sprintf("%.4d",$val)')}, "CameraSettings3")
    return data


def raw_ids(text):
    body = text.split("pub static RAW_TAG_IDS: &[&[&str]] = &[\n", 1)[1].split("];", 1)[0]
    return {match[2]: json.loads(match[1]) for match in
            re.finditer(r"^    &(\[.*\]), // (\w+)$", body, re.MULTILINE)}


class SonyPlainGeneratorTests(unittest.TestCase):
    def test_complete_six_table_output_deduplicates_maps_and_orders_keys(self):
        text, counts = generator.render(fixture())
        self.assertEqual((counts["tables"], counts["rows"], counts["maps"]), (6, 6, 1))
        self.assertIn('[("1", "One"), ("10", "Ten"), ("2", "Two")]', text)
        self.assertIn("pub const SHOTINFO: usize = 5;", text)
        self.assertIn("//! Sony plain (unenciphered)", text)
        self.assertTrue(text.endswith("}\n"))
        self.assertEqual(list(raw_ids(text)), ["CameraSettings", "CameraSettings2", "CameraSettings3",
                                               "FaceInfo1", "FaceInfo2", "ShotInfo"])
        self.assertTrue(all(ids == ["1"] for ids in raw_ids(text).values()))

    def test_source_data_changes_reach_output(self):
        data = fixture()
        row = table(data)["tags"]["1"]
        row["Name"] = "Renamed"
        row["PrintConv"]["map"]["2"] = "Different native text"
        add(data, "20", {"Name": "NewTag", "ValueConv": expr("$val * 100")})
        text, counts = generator.render(data)
        self.assertIn('name: "Renamed"', text)
        self.assertIn('("2", "Different native text")', text)
        self.assertIn('index: 20, name: "NewTag"', text)
        self.assertIn("vc: Vc::Mul(100.0_f64)", text)
        self.assertEqual(counts["rows"], 7)

    def test_numeric_tag_order_and_ordered_variants(self):
        data = fixture()
        add(data, "10", {"Name": "Ten"})
        add(data, "2", {"_variants": [
            {"Name": "First", "Condition": "($$self{Model} =~ /^NEX-/)"},
            {"Name": "Second"}]})
        text, _ = generator.render(data)
        self.assertLess(text.index('name: "First"'), text.index('name: "Second"'))
        self.assertLess(text.index('index: 2,'), text.index('index: 10,'))
        self.assertEqual(raw_ids(text)["CameraSettings"], ["1", "2", "2", "10"])

    def test_changed_expression_semantics_and_literals_refuse(self):
        mutations = [
            ("Condition", "$$self{Model} =~ /^NEX- /"),
            ("Condition", "($$self{Model} =~ /^NEX-/i)"),
            ("Condition", "($$self{Model} =~ /^NEX-/); unlink $file"),
            ("RawConv", expr("$$self{FacesDetected} = $val + 1")),
            ("ValueConv", expr("$val * 101")),
            ("PrintConv", expr('"$val  K"')),
            ("PrintConv", expr('sprintf("%.5d",$val)')),
        ]
        for field, value in mutations:
            with self.subTest(field=field, value=value):
                data = fixture()
                table(data)["tags"]["1"][field] = value
                with self.assertRaises(generator.Unsupported):
                    generator.render(data)

    def test_enum_never_hides_an_unsupported_value_or_raw_stage(self):
        for field in ("RawConv", "ValueConv"):
            data = fixture()
            table(data)["tags"]["1"][field] = expr("return undef")
            with self.assertRaises(generator.Unsupported):
                generator.render(data)

    def test_unmodeled_fields_refuse(self):
        for field, value in [("Hook", "$varSize += 1"), ("Count", "2"), ("Hidden", "1"),
                             ("ByteOrder", "II"), ("Offset", "4"), ("BitsPerWord", "8"),
                             ("Binary", "1"), ("Flags", "List"), ("_extra_keys", ["NewReadField"]),
                             ("Groups", {"1": "Different"}), ("Priority", "2")]:
            with self.subTest(field=field):
                data = fixture()
                table(data)["tags"]["1"][field] = value
                with self.assertRaises(generator.Unsupported):
                    generator.render(data)

    def test_printconv_columns_is_positive_display_only_metadata(self):
        baseline, _ = generator.render(fixture())
        for value in (2, "2", "3"):
            with self.subTest(value=value):
                data = fixture()
                table(data)["tags"]["1"]["PrintConvColumns"] = value
                self.assertEqual(generator.render(data)[0], baseline)

        for value in (True, 0, -1, "0", "02", "two", {"__perl": "CODE"}):
            with self.subTest(value=value):
                data = fixture()
                table(data)["tags"]["1"]["PrintConvColumns"] = value
                with self.assertRaises(generator.Unsupported):
                    generator.render(data)

    def test_malformed_tag_ids_names_and_format_refuse(self):
        for key in ("1.", "1.0", "01", "-1", "4294967296", "4294967296.1", "1e2", "276.10",
                    "+1", "NaN", "Infinity", "1.01x", " 1", "1\n", "1.1\n"):
            data = fixture()
            add(data, key, {"Name": "Bad"})
            with self.subTest(key=key), self.assertRaises(generator.Unsupported):
                generator.render(data)
        for field, value in [("Name", ""), ("Name", "0"), ("Name", 123),
                             ("Format", "int8u[0]"), ("Format", "string"),
                             ("Format", "int16u[$val]"), ("Format", "int16u[01]"),
                             ("Format", "var_int8u"), ("Format", None), ("PrintHex", True)]:
            data = fixture()
            table(data)["tags"]["1"][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(generator.Unsupported):
                generator.render(data)

    def test_known_raw_data_member_and_fixed_array(self):
        data = fixture()
        add(data, "2", {"Name": "FacesDetected", "Format": "int16u",
                        "DataMember": "FacesDetected", "RawConv": expr("$$self{FacesDetected} = $val")}, "ShotInfo")
        add(data, "20", {"Name": "Face2Position", "Format": "int16u[4]",
                         "RawConv": expr("$$self{FacesDetected} < 2 ? undef : $val")}, "FaceInfo1")
        text, _ = generator.render(data)
        self.assertIn("raw: Raw::Store(Dm::FacesDetected)", text)
        self.assertIn("fmt: Fmt::U16, count: 4", text)
        self.assertIn("raw: Raw::DropIfDmLess(Dm::FacesDetected, 2.0_f64)", text)
        table(data, "ShotInfo")["tags"]["2"]["DataMember"] = "Other"
        with self.assertRaisesRegex(generator.Unsupported, "DataMember"):
            generator.render(data)

    def test_subdirectory_identity_and_execution_fields(self):
        data = fixture()
        row = {"Name": "FaceInfo1", "SubDirectory": {"TagTable": "Image::ExifTool::Sony::FaceInfo1"}}
        add(data, "72", row, "ShotInfo")
        text, _ = generator.render(data)
        self.assertIn("subdir: Some(3)", text)
        for change in ({"Start": "4"}, {"ProcessProc": "Other"}, {"TagTable": "Image::ExifTool::Sony::FaceInfo3"}):
            mutated = copy.deepcopy(data)
            table(mutated, "ShotInfo")["tags"]["72"]["SubDirectory"].update(change)
            with self.assertRaises(generator.Unsupported):
                generator.render(mutated)

    def test_enriched_shared_processor_provenance_accepts_only_live_binding(self):
        data = fixture()
        processor = table(data)["meta"]["PROCESS_PROC"]
        processor.update({
            "resolved": True,
            "source_file": "Image/ExifTool.pm",
            "source_sha256": "a" * 64,
            "dependencies": {"Image::ExifTool::Get16u": {
                "__perl": "CODE", "__opaque": True, "__name": "Image::ExifTool::Get16u",
                "__deparse": "sub { return 0; }", "resolved": True,
                "source_file": "Image/ExifTool.pm", "source_sha256": "b" * 64,
            }},
        })
        generator.render(data)
        for mutate in (
            lambda p: p.update(__name="Image::ExifTool::Other"),
            lambda p: p.update(resolved=False),
            lambda p: p.update(source_file="Image/ExifTool/Other.pm"),
            lambda p: p["dependencies"].pop("Image::ExifTool::Get16u"),
            lambda p: p["dependencies"]["Image::ExifTool::Get16u"].update(__name="Image::ExifTool::Get32u"),
        ):
            with self.subTest(mutate=mutate):
                changed = fixture()
                # Reconstruct the enriched shape before applying a refusal
                # mutation so every control exercises the same selector.
                p = table(changed)["meta"]["PROCESS_PROC"]
                p.update({"resolved": True, "source_file": "Image/ExifTool.pm", "source_sha256": "a" * 64,
                          "dependencies": {"Image::ExifTool::Get16u": {"__perl": "CODE", "__opaque": True,
                              "__name": "Image::ExifTool::Get16u", "__deparse": "sub { return 0; }",
                              "resolved": True, "source_file": "Image/ExifTool.pm", "source_sha256": "b" * 64}}})
                mutate(p)
                with self.assertRaises(generator.Unsupported):
                    generator.render(changed)

    @unittest.skipUnless(os.environ.get("OXIDEX_SONY_DUMP"), "set OXIDEX_SONY_DUMP to a canonical enriched dump")
    def test_canonical_enriched_dump_keeps_legacy_sony_selection_identity(self):
        data = json.loads(Path(os.environ["OXIDEX_SONY_DUMP"]).read_text(encoding="utf-8"))
        for name in generator.TABLES:
            with self.subTest(table=name):
                generator.check_meta(name, data["modules"]["Sony"]["tables"][name])

    def test_table_contracts_missing_and_empty_tables_refuse(self):
        for field, value in [("PROCESS_PROC", {"__name": "Other"}), ("FIRST_ENTRY", "1"),
                             ("GROUPS", {"0": "EXIF", "2": "Camera"}), ("FORMAT", "int32u"),
                             ("DATAMEMBER", ["9"]), ("IS_SUBDIR", ["9"]),
                             ("VARS", {"varSize": 4}), ("PRIORITY", "2")]:
            data = fixture()
            table(data)["meta"][field] = value
            with self.subTest(field=field), self.assertRaises(generator.Unsupported):
                generator.render(data)
        data = fixture()
        table(data)["tags"] = {}
        with self.assertRaises(generator.Unsupported):
            generator.render(data)
        del data["modules"]["Sony"]["tables"]["ShotInfo"]
        with self.assertRaises((generator.Unsupported, KeyError)):
            generator.render(data)

    def test_unknown_omission_retains_first_match_veto_boundary(self):
        data = fixture()
        add(data, "20", {"Name": "UnknownSetting", "Unknown": "1"})
        text, counts = generator.render(data)
        self.assertNotIn("UnknownSetting", text)
        self.assertEqual(counts["unknown_omitted"], 1)
        self.assertEqual(raw_ids(text)["CameraSettings"], ["1"])
        add(data, "20", {"_variants": [{"Name": "UnknownSetting", "Unknown": "1"}, {"Name": "Fallback"}]})
        with self.assertRaisesRegex(generator.Unsupported, "known/Unknown"):
            generator.render(data)
        add(data, "20", {"Name": "UnknownSetting", "Unknown": "1", "Condition": "do_side_effect()"})
        with self.assertRaises(generator.Unsupported):
            generator.render(data)

    def test_malformed_alternatives_refuse(self):
        for group in ({"_variants": []}, {"_variants": [{}], "Name": "Also"},
                      {"_variants": [None]}, {"_variants": {"Name": "Wrong"}}):
            data = fixture()
            add(data, "20", group)
            with self.assertRaises(generator.Unsupported):
                generator.render(data)

    def test_independent_fractional_keys_preserve_native_identity(self):
        text, counts = generator.render(collision_fixture())
        self.assertEqual(text.count("index: 276,"), 2)
        self.assertNotIn("projections", counts)
        self.assertEqual(raw_ids(text)["CameraSettings3"], ["1", "276", "276.1"])
        self.assertLess(text.index('name: "FolderNumber"'), text.index('name: "ImageNumber"'))

    def test_fractional_keys_do_not_require_a_handwired_sibling_or_name(self):
        data = collision_fixture()
        del table(data, "CameraSettings3")["tags"]["276"]
        table(data, "CameraSettings3")["tag_count"] -= 1
        row = table(data, "CameraSettings3")["tags"]["276.1"]
        row.update(Name="RenamedIndependent", Mask="255")
        text, _ = generator.render(data)
        self.assertEqual(raw_ids(text)["CameraSettings3"], ["1", "276.1"])
        self.assertIn('index: 276, name: "RenamedIndependent"', text)
        add(data, "12.01", {"Name": "NewFraction"}, "FaceInfo2")
        text, _ = generator.render(data)
        self.assertEqual(raw_ids(text)["FaceInfo2"], ["1", "12.01"])
        add(data, "1.5", {"Name": "HalfIndex"})
        text, _ = generator.render(data)
        # CameraSettings is int16u. Native int(1.5) * 2 is byte 2, not 3.
        self.assertIn('index: 1, name: "HalfIndex"', text)
        self.assertEqual(raw_ids(text)["CameraSettings"], ["1", "1.5"])

    def test_fractional_sort_uses_exact_decimal_and_keeps_integer_offset(self):
        data = fixture()
        # Reverse insertion order, including an index at the u32 boundary.
        add(data, "4294967295.2", {"Name": "Later"})
        add(data, "4294967295.1", {"Name": "Earlier"})
        add(data, "0.01", {"Name": "BeforeFirst"})
        text, _ = generator.render(data)
        self.assertEqual(raw_ids(text)["CameraSettings"],
                         ["0.01", "1", "4294967295.1", "4294967295.2"])
        self.assertLess(text.index('name: "Earlier"'), text.index('name: "Later"'))
        self.assertEqual(text.count("index: 4294967295,"), 2)
        self.assertIn('index: 0, name: "BeforeFirst"', text)

    def test_distinct_ids_that_tie_in_native_numeric_sort_refuse(self):
        for first, second in [("4294967295.10000000000000001", "4294967295.10000000000000002"),
                              ("1", "1.00000000000000000001")]:
            data = fixture()
            add(data, first, {"Name": "First"})
            add(data, second, {"Name": "Second"})
            with self.assertRaisesRegex(generator.Unsupported, "native numeric sort"):
                generator.render(data)

    def test_fraction_that_rounds_across_native_integer_boundary_refuses(self):
        for key in ("1.9999999999999999", "4294967295.99999999999999"):
            data = fixture()
            add(data, key, {"Name": "RoundedOffset"})
            with self.assertRaisesRegex(generator.Unsupported, "native integer offset"):
                generator.render(data)

    def test_fractional_variants_and_unknown_omission_keep_metadata_aligned(self):
        data = fixture()
        add(data, "1.1", {"_variants": [
            {"Name": "First", "Condition": "($$self{Model} =~ /^NEX-/)"}, {"Name": "Second"}]})
        add(data, "1.2", {"Name": "Independent"})
        add(data, "1.3", {"_variants": [
            {"Name": "UnknownFirst", "Unknown": "1"}, {"Name": "UnknownSecond", "Unknown": "1"}]})
        text, counts = generator.render(data)
        self.assertEqual(raw_ids(text)["CameraSettings"], ["1", "1.1", "1.1", "1.2"])
        self.assertEqual(counts["unknown_omitted"], 2)
        self.assertNotIn("UnknownFirst", text)
        self.assertEqual(sum(map(len, raw_ids(text).values())), counts["rows"])

    def test_fractional_key_does_not_bypass_semantic_validation(self):
        for field, value in [("Hook", "$varSize += 1"), ("RawConv", expr("$val + 1")),
                             ("Condition", "side_effect()"), ("BitShift", "2")]:
            data = fixture()
            add(data, "1.1", {"Name": "Fraction", field: value})
            with self.subTest(field=field), self.assertRaises(generator.Unsupported):
                generator.render(data)

    def test_mask_derives_native_shift_and_rejects_unrepresented_override(self):
        data = fixture()
        add(data, "20", {"Name": "Bits", "Mask": "12", "BitShift": "2"})
        text, counts = generator.render(data)
        self.assertIn("mask: 12", text)
        self.assertNotIn("projections", counts)
        table(data)["tags"]["20"]["BitShift"] = "0"
        with self.assertRaisesRegex(generator.Unsupported, "BitShift"):
            generator.render(data)

    def test_enum_and_bitmask_directives_are_not_silently_dropped(self):
        data = fixture()
        table(data)["tags"]["1"]["PrintConv"] = enum({"0": "None"}, {"BITMASK": {"2": "Tracking", "0": "Confirmed"}})
        text, _ = generator.render(data)
        self.assertIn('[(0u32, "Confirmed"), (2u32, "Tracking")]', text)
        self.assertIn("Pc::Bitmask(M0, B0, 32, Other::None)", text)
        for directives in ({"OTHER": "guess"}, {"Unexpected": 1}, {"BITMASK": {"32": "Too far"}}, {"BITMASK": {"0": {"ref": "value"}}}):
            changed = fixture()
            table(changed)["tags"]["1"]["PrintConv"] = enum({"0": "None"}, directives)
            with self.assertRaises(generator.Unsupported):
                generator.render(changed)

    def test_exact_native_other_bodies_and_adversarial_changes(self):
        bodies = [
            ("{\n    package Image::ExifTool::Sony;\n    use strict;\n    (shift());\n}", "Other::Identity"),
            ("{\n    package Image::ExifTool::Sony;\n    use strict;\n    (my($val, $inv) = @_);\n    ($inv or (return int(($val + 0.5))));\n    (return (&Image::ExifTool::IsFloat($val) ? $val : (undef)));\n}", "Other::RoundHalfUp"),
        ]
        for body, expected in bodies:
            data = fixture()
            code = {"__perl": "CODE", "__opaque": 1, "__name": "Image::ExifTool::Sony::__ANON__", "__deparse": body}
            table(data)["tags"]["1"]["PrintConv"] = enum({"0": "Zero"}, {"OTHER": code})
            self.assertIn(expected, generator.render(data)[0])
            for mutated in (body + " die;", body.replace("shift()", "shift() + 1").replace("0.5", "0.6"), body.replace("use strict;", 'use strict; return "A  B";')):
                code["__deparse"] = mutated
                with self.assertRaises(generator.Unsupported):
                    generator.render(data)

    def test_native_printint_documentation_boundary_is_exact_and_reported(self):
        data = fixture()
        add(data, "1015", {"Name": "LensType2", "_extra_keys": ["PrintInt"]}, "CameraSettings3")
        _, counts = generator.render(data)
        self.assertEqual(len(counts["dump_boundaries"]), 1)
        table(data, "CameraSettings3")["tags"]["1015"]["Name"] = "Other"
        with self.assertRaisesRegex(generator.Unsupported, "PrintInt"):
            generator.render(data)

    def test_untrusted_strings_escape_without_changing_data(self):
        data = fixture()
        table(data)["tags"]["1"]["PrintConv"]["map"]["2"] = 'quote" back\\ newline\n tab\t café'
        text, _ = generator.render(data)
        self.assertIn('quote\\" back\\\\ newline\\u{a} tab\\u{9} café', text)
        table(data)["tags"]["1"]["Name"] = "\ud800"
        with self.assertRaisesRegex(generator.Unsupported, "surrogate"):
            generator.render(data)

    def run_cli(self, text):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, output = root / "dump.json", root / "output.rs"
            source.write_text(text)
            sentinel = b"existing output\x00keep it\n"
            output.write_bytes(sentinel)
            before = output.stat()
            result = subprocess.run([sys.executable, "-B", generator.__file__, str(source), "-o", str(output)],
                                    text=True, capture_output=True, timeout=15)
            return result, output.read_bytes(), sentinel, before.st_mtime_ns, output.stat().st_mtime_ns

    def test_cli_success_emits_complete_file_and_counts(self):
        result, output, _, _, _ = self.run_cli(json.dumps(fixture()))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(b"pub static TABLES", output)
        counts = json.loads(result.stderr.split(": ", 1)[1])
        self.assertEqual(counts["rows"], 6)

    def test_cli_refusals_leave_existing_output_bytes_and_mtime_unchanged(self):
        bad_pin = fixture(); bad_pin["exiftool_version"] = "0.00"
        bad_semantic = fixture(); table(bad_semantic)["tags"]["1"]["ValueConv"] = expr("$val * 101")
        bad_unicode = fixture(); table(bad_unicode)["tags"]["1"]["Name"] = "\ud800"
        bad_enum = fixture(); table(bad_enum)["tags"]["1"]["PrintConv"] = enum({"1": {"reference": "not a label"}})
        bad_fraction = fixture(); add(bad_fraction, "276.10", {"Name": "Alias"})
        bad_order = fixture(); add(bad_order, "1.00000000000000000001", {"Name": "NumericTie"})
        bad_offset = fixture(); add(bad_offset, "1.9999999999999999", {"Name": "RoundedOffset"})
        for text in ("{malformed", '{"exiftool_version":"13.59","exiftool_version":"13.59"}',
                     json.dumps(bad_pin), json.dumps(bad_semantic), json.dumps(bad_unicode), json.dumps(bad_enum),
                     json.dumps(bad_fraction), json.dumps(bad_order), json.dumps(bad_offset)):
            with self.subTest(text=text[:80]):
                result, output, sentinel, before, after = self.run_cli(text)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, sentinel)
                self.assertEqual(before, after)


if __name__ == "__main__":
    unittest.main()
