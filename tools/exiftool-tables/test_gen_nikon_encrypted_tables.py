"""Controls for the native Nikon encrypted-table projection."""
import copy
import hashlib
import json
from pathlib import Path
import re
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


def encrypted_callback(data, decrypt_body="canonical decrypt body"):
    """Attach a captured-style callback fact to the encrypted ShotInfo root."""
    callback_body = "canonical ProcessNikonEncrypted body"
    callback_pair = ("Image::ExifTool::Nikon::ProcessNikonEncrypted",
                     generator.hashlib.sha256(callback_body.encode()).hexdigest())
    decrypt_pair = ("Image::ExifTool::Nikon::Decrypt",
                    generator.hashlib.sha256(decrypt_body.encode()).hexdigest())
    xlat_bytes = bytes(range(256)) + bytes(reversed(range(256)))
    callback = {
        "__perl": "CODE", "__opaque": True, "__name": callback_pair[0],
        "__deparse": callback_body, "resolved": True,
        "source_file": "Image/ExifTool/Nikon.pm", "source_sha256": "1" * 64,
        "dependencies": {
            decrypt_pair[0]: {
                "__perl": "CODE", "__opaque": True, "__name": decrypt_pair[0],
                "__deparse": decrypt_body, "resolved": True,
                "source_file": "Image/ExifTool/Nikon.pm", "source_sha256": "1" * 64,
                "lexical_arrays": {
                    "resolved": True,
                    "rows": [list(range(256)), list(reversed(range(256)))],
                    "sha256": hashlib.sha256(xlat_bytes).hexdigest(),
                },
            },
        },
    }
    data["modules"]["Nikon"]["tables"]["Main"]["tags"]["145"]["SubDirectory"]["ProcessProc"] = callback
    return callback_pair, decrypt_pair, callback


class NikonEncryptedGeneratorTests(unittest.TestCase):
    def test_full_hydration_callback_contract_pins_complete_closure(self):
        """Canonical Perl 5.38.2 B::Deparse form remains a closed contract."""
        callback = (
            "Image::ExifTool::Nikon::ProcessNikonEncrypted",
            "4814b522c2940b28240fc0fbcaa6d22e43619d3e604c3d7a65de4b61513c467e",
        )
        self.assertIn(callback, generator.CODE)
        contract = generator.NIKON_ENCRYPTED_CALLBACKS[callback]
        self.assertEqual(contract["source_file"], "Image/ExifTool/Nikon.pm")
        self.assertEqual(contract["dependencies"], {
            "Image::ExifTool::Nikon::Decrypt": {
                "body_sha256": "5ecb54a37173daf492800e65c341309ce78d56ed7483f6c9efed7bdc4d7e949b",
                "source_file": "Image/ExifTool/Nikon.pm",
            },
            "Image::ExifTool::Nikon::InitEncryptedSubdir": {
                "body_sha256": "56a3cc34bff49394ce5d9e531d762bba5594d6df741150a253f849d8e6415c5e",
                "source_file": "Image/ExifTool/Nikon.pm",
            },
            "Image::ExifTool::Nikon::PrepareNikonOffsets": {
                "body_sha256": "0451602b9206b6f48dfce8f69640edba38ef1b51322d6b4fe54b2c7761f79153",
                "source_file": "Image/ExifTool/Nikon.pm",
            },
            "Image::ExifTool::Nikon::SetByteOrder": {
                "name": "Image::ExifTool::SetByteOrder",
                "body_sha256": "b09a10c46f0800e2a2d1bc8cde1269fa205300e62f0d5bf7358d6a57ac2c8d4d",
                "source_file": "Image/ExifTool.pm",
            },
        })

    def test_map_insertion_order_does_not_change_generated_output(self):
        first = fixture()
        second = fixture()
        values = second["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]["PrintConv"]["map"]
        second["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]["PrintConv"]["map"] = {
            key: values[key] for key in reversed(list(values))
        }
        self.assertEqual(generator.render(first), generator.render(second))

    def test_rebound_decrypt_helper_refuses_unchanged_callback(self):
        data = fixture()
        callback_pair, decrypt_pair, callback = encrypted_callback(data)
        generator.CODE.add(callback_pair)
        self.addCleanup(generator.CODE.remove, callback_pair)
        generator.CODE.add(decrypt_pair)
        self.addCleanup(generator.CODE.remove, decrypt_pair)
        previous = getattr(generator, "NIKON_ENCRYPTED_CALLBACKS", None)
        generator.NIKON_ENCRYPTED_CALLBACKS = {
            callback_pair: {
                "source_file": "Image/ExifTool/Nikon.pm", "source_sha256": "1" * 64,
                "dependencies": {
                    decrypt_pair[0]: {
                        "body_sha256": decrypt_pair[1],
                        "source_file": "Image/ExifTool/Nikon.pm", "source_sha256": "1" * 64,
                    },
                },
            },
        }
        self.addCleanup(
            lambda: (delattr(generator, "NIKON_ENCRYPTED_CALLBACKS")
                     if previous is None else setattr(generator, "NIKON_ENCRYPTED_CALLBACKS", previous))
        )
        canonical_text, _ = generator.render(data)
        self.assertIn("pub static XLAT0: [u8; 256]", canonical_text)

        changed = copy.deepcopy(data)
        helper = changed["modules"]["Nikon"]["tables"]["Main"]["tags"]["145"]["SubDirectory"]["ProcessProc"]["dependencies"][decrypt_pair[0]]
        helper["__deparse"] = "changed native decrypt body"
        helper["source_sha256"] = "2" * 64
        self.assertEqual(
            changed["modules"]["Nikon"]["tables"]["Main"]["tags"]["145"]["SubDirectory"]["ProcessProc"]["__deparse"],
            callback["__deparse"],
        )
        with self.assertRaises(generator.Unsupported):
            generator.render(changed)

        rebound = copy.deepcopy(data)
        helper = rebound["modules"]["Nikon"]["tables"]["Main"]["tags"]["145"]["SubDirectory"]["ProcessProc"]["dependencies"][decrypt_pair[0]]
        helper["source_sha256"] = "3" * 64
        with self.assertRaises(generator.Unsupported):
            generator.render(rebound)

        changed_lookup = copy.deepcopy(data)
        lookup = changed_lookup["modules"]["Nikon"]["tables"]["Main"]["tags"]["145"]["SubDirectory"]["ProcessProc"]["dependencies"][decrypt_pair[0]]["lexical_arrays"]
        lookup["rows"][0][17] ^= 1
        lookup["sha256"] = hashlib.sha256(bytes(lookup["rows"][0]) + bytes(lookup["rows"][1])).hexdigest()
        # Root and Decrypt bodies, plus their provenance, remain bound.  A
        # changed closed-over lookup byte must still reject stale Rust crypto.
        changed_text, _ = generator.render(changed_lookup)
        self.assertNotEqual(changed_text, canonical_text)
        self.assertIn("0x10, 0x11, 0x12", canonical_text)
        self.assertIn("0x10, 0x10, 0x12", changed_text)

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

    def test_dumped_extra_keys_must_be_empty(self):
        data = fixture()
        row = data["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]
        row["_extra_keys"] = []
        generator.render(data)
        row["_extra_keys"] = ["NewExecutableField"]
        with self.assertRaisesRegex(generator.Unsupported, "unrecognized dumped fields"):
            generator.render(data)

    def test_printconv_columns_is_validated_presentation_only_metadata(self):
        data = fixture()
        row = data["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]
        baseline, _ = generator.render(data)
        row["PrintConvColumns"] = 2
        rendered, _ = generator.render(data)
        self.assertEqual(rendered, baseline)
        row["PrintConvColumns"] = "2"
        self.assertEqual(generator.render(data)[0], baseline)
        for invalid in (0, -1, True, "02"):
            with self.subTest(invalid=invalid):
                changed = copy.deepcopy(data)
                changed["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]["PrintConvColumns"] = invalid
                with self.assertRaisesRegex(generator.Unsupported, "invalid PrintConvColumns"):
                    generator.render(changed)

    def test_reader_ignored_native_properties_are_closed_by_shape_and_location(self):
        row = fixture()["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]
        identity = ("Nikon", "Test")
        generator.reader_ignored_properties(identity, "1", {"DelValue": 0})
        generator.reader_ignored_properties(identity, "1", {"AlwaysDecrypt": "1"})
        for property, value in (("DelValue", -1), ("AlwaysDecrypt", 0)):
            with self.subTest(property=property), self.assertRaises(generator.Unsupported):
                generator.reader_ignored_properties(identity, "1", {property: value})

        typo = {"0": "None", "7": "Dynamic AF Area"}
        generator.reader_ignored_properties(("NikonCustom", "AnyTable"), "32.1", {"Prinonv": typo})
        generator.reader_ignored_properties(("Nikon", "AnyOtherTable"), "99", {"Prinonv": typo})
        typo["not-a-number"] = "changed"
        with self.assertRaises(generator.Unsupported):
            generator.reader_ignored_properties(("NikonCustom", "AnyTable"), "32.1", {"Prinonv": typo})
        with self.assertRaises(generator.Unsupported):
            generator.reader_ignored_properties(("NikonCustom", "AnyTable"), "32.1", {"Prinonv": {}})

    def test_rust_string_escaping_preserves_literal_backslash_sequences(self):
        self.assertEqual(generator.rs(r"a\nb\tc\rd"), r'"a\\nb\\tc\\rd"')
        self.assertEqual(generator.rs("a\nb\tc\rd\x01"), r'"a\u{a}b\u{9}c\u{d}d\u{1}"')

    def test_perl_only_regexes_refuse_before_emission(self):
        self.assertEqual(generator.rust_regex(r"^NIKON (?:D[0-9])?\b"), r"^NIKON (?:D[0-9])?\b")
        for pattern in (r"(?=D5)", r"(D5)\1", r"(?P<camera>D5)", r"D5\K",
                        r"\X", r"\R", r"\o{123}", r"\j", r"D{2,}", r"[a[b]]", r"[a&&b]"):
            with self.subTest(pattern=pattern), self.assertRaises(generator.Unsupported):
                generator.rust_regex(pattern)
        data = fixture()
        data["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]["Condition"] = r"$$self{Model} =~ /(?=D5)/"
        with self.assertRaises(generator.Unsupported):
            generator.render(data)

    def test_graph_identity_keeps_nikon_and_nikoncustom_name_collisions_distinct(self):
        data = fixture()
        nikon = data["modules"]["Nikon"]["tables"]
        custom = data["modules"]["NikonCustom"]["tables"]
        nikon["Test"]["tags"].update({
            "2": {"Name": "NikonChild", "SubDirectory": {"TagTable": "Image::ExifTool::Nikon::Shared"}},
            "3": {"Name": "CustomChild", "SubDirectory": {"TagTable": "Image::ExifTool::NikonCustom::Shared"}},
        })
        nikon["Shared"] = {"meta": {"FORMAT": "int8u"}, "tags": {"1": {"Name": "NikonOnly"}}}
        custom["Shared"] = {"meta": {"FORMAT": "int8u"}, "tags": {"1": {"Name": "CustomOnly"}}}
        text, counts = generator.render(data)
        self.assertEqual(counts["tables"], 3)
        self.assertIn("static TAGS_NIKON_SHARED", text)
        self.assertIn("static TAGS_NIKONCUSTOM_SHARED", text)
        self.assertIn('name: "NikonOnly"', text)
        self.assertIn('name: "CustomOnly"', text)
        test_rows = text.split("static TAGS_TEST", 1)[1].split("];", 1)[0]
        self.assertIn("subdir: Some(SubDir { table: 0", test_rows)
        self.assertIn("subdir: Some(SubDir { table: 2", test_rows)

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


FORMS = json.loads((Path(__file__).with_name("testdata") / "nikon_encrypted_forms.json").read_text())
PBD = "Image::ExifTool::ProcessBinaryData"
PNO = "Image::ExifTool::Nikon::PrepareNikonOffsets"


def real_closure_fixture(callback_release, pbd_release=None, label="13.59"):
    """The unit fixture, driven by a captured release's real callback closure.

    Every encrypted root dispatches through that closure and the table uses
    that release's (or `pbd_release`'s) real ProcessBinaryData body.
    """
    data = fixture()
    data["exiftool_version"] = label
    for ident in ("145", "151", "152"):
        data["modules"]["Nikon"]["tables"]["Main"]["tags"][ident]["SubDirectory"]["ProcessProc"] = \
            copy.deepcopy(FORMS[callback_release]["callback"])
    data["modules"]["Nikon"]["tables"]["Test"]["meta"]["PROCESS_PROC"] = \
        copy.deepcopy(FORMS[pbd_release or callback_release]["process_binary_data"])
    return data


def without_header_label(text):
    return re.sub(r"in-process \(ExifTool [0-9.]+\)", "in-process (ExifTool <label>)", text)


class ContentGateTests(unittest.TestCase):
    """The version label is provenance text; only modeled content admits or refuses."""

    def test_byte_identical_relabelled_source_is_admitted(self):
        baseline, counts = generator.render(real_closure_fixture("13.59"))
        self.assertIn("in-process (ExifTool 13.59)", baseline)
        for label in ("99.99", "12.64", "11.78", "13.60"):
            with self.subTest(label=label):
                text, relabelled_counts = generator.render(real_closure_fixture("13.59", label=label))
                self.assertIn(f"in-process (ExifTool {label})", text)
                self.assertEqual(without_header_label(text), without_header_label(baseline))
                self.assertEqual(relabelled_counts, counts)

    def test_13_59_label_with_changed_modeled_content_is_refused(self):
        """The label the old gate required is present; the content is not the approved form."""
        generator.render(real_closure_fixture("13.59"))
        mutations = {
            "callback body": lambda d: d["modules"]["Nikon"]["tables"]["Main"]["tags"]["152"]["SubDirectory"]["ProcessProc"],
            "PrepareNikonOffsets helper": lambda d: d["modules"]["Nikon"]["tables"]["Main"]["tags"]["152"]["SubDirectory"]["ProcessProc"]["dependencies"][PNO],
            "Decrypt helper": lambda d: d["modules"]["Nikon"]["tables"]["Main"]["tags"]["152"]["SubDirectory"]["ProcessProc"]["dependencies"]["Image::ExifTool::Nikon::Decrypt"],
            "ProcessBinaryData": lambda d: d["modules"]["Nikon"]["tables"]["Test"]["meta"]["PROCESS_PROC"],
        }
        for what, locate in mutations.items():
            with self.subTest(what=what):
                data = real_closure_fixture("13.59")
                self.assertEqual(data["exiftool_version"], "13.59")
                fact = locate(data)
                fact["__deparse"] = fact["__deparse"].replace("{", "{ 1;", 1)
                with self.assertRaises(generator.Unsupported):
                    generator.render(data)

    def test_malformed_label_refuses_only_for_header_provenance(self):
        for label in (None, "", "latest", 13.59):
            with self.subTest(label=label):
                data = real_closure_fixture("13.59")
                data["exiftool_version"] = label
                with self.assertRaisesRegex(generator.Unsupported, "malformed ExifTool release label"):
                    generator.render(data)

    def test_12_64_captured_forms_are_admitted_and_rewrite_to_approved(self):
        approved, approved_counts = generator.render(real_closure_fixture("13.59"))
        captured, captured_counts = generator.render(real_closure_fixture("12.64", label="12.64"))
        # 12.64's PrepareNikonOffsets and ProcessBinaryData are captured forms;
        # every other modeled body is byte-identical, so the artifact is too.
        self.assertEqual(without_header_label(captured), without_header_label(approved))
        self.assertEqual(captured_counts, approved_counts)
        for fact in (FORMS["12.64"]["callback"]["dependencies"][PNO], FORMS["12.64"]["process_binary_data"]):
            with self.subTest(name=fact["__name"]):
                form = generator.CAPTURED_FORMS[(fact["__name"], hashlib.sha256(fact["__deparse"].encode()).hexdigest())]
                approved_fact = (FORMS["13.59"]["callback"]["dependencies"][PNO] if fact["__name"] == PNO
                                 else FORMS["13.59"]["process_binary_data"])
                self.assertEqual(hashlib.sha256(approved_fact["__deparse"].encode()).hexdigest(), form["approved"])
                self.assertEqual(hashlib.sha256(generator._compact(approved_fact["__deparse"]).encode()).hexdigest(),
                                 form["approved_compact_sha256"])
                self.assertTrue(generator.captured_form(fact["__name"], fact["__deparse"]))

    def test_captured_form_requires_its_complete_recorded_delta(self):
        fact = FORMS["12.64"]["process_binary_data"]
        key = (fact["__name"], hashlib.sha256(fact["__deparse"].encode()).hexdigest())
        original = generator.CAPTURED_FORMS[key]
        self.addCleanup(generator.CAPTURED_FORMS.__setitem__, key, original)
        # Dropping a region leaves an unexplained difference from the approved form.
        generator.CAPTURED_FORMS[key] = dict(original, delta=original["delta"][:-1])
        with self.assertRaisesRegex(generator.Unsupported, "does not rewrite to approved form"):
            generator.captured_form(*key[:1], fact["__deparse"])
        # A region that is not present exactly once is not the captured body.
        generator.CAPTURED_FORMS[key] = dict(original, delta=original["delta"] + (("}", "}"),))
        with self.assertRaisesRegex(generator.Unsupported, "lacks its recorded region exactly once"):
            generator.captured_form(*key[:1], fact["__deparse"])

    def test_captured_form_is_never_admitted_without_its_effect_enforced(self):
        with self.assertRaisesRegex(generator.Unsupported, "outside an enforcing context"):
            generator.code(copy.deepcopy(FORMS["12.64"]["process_binary_data"]))
        effects = set()
        generator.code(copy.deepcopy(FORMS["12.64"]["process_binary_data"]), effects)
        self.assertEqual(effects, {"no_notdup"})

    def test_pre_always_decrypt_prepare_offsets_refuses_always_decrypt_rows(self):
        for release, admitted in (("13.59", True), ("12.64", False)):
            with self.subTest(release=release):
                data = real_closure_fixture(release)
                data["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]["AlwaysDecrypt"] = 1
                if admitted:
                    generator.render(data)
                else:
                    with self.assertRaisesRegex(generator.Unsupported, "pre-AlwaysDecrypt PrepareNikonOffsets"):
                        generator.render(data)

    def test_pre_notdup_binary_data_refuses_reachable_duplicate_addresses(self):
        def with_child(release, start=None, allow_reprocess=False, extra=None):
            data = real_closure_fixture("13.59", pbd_release=release)
            tables = data["modules"]["Nikon"]["tables"]
            sub = {"TagTable": "Image::ExifTool::Nikon::Child"}
            if start is not None:
                sub["Start"] = start
            tables["Test"]["tags"]["0"] = {"Name": "ChildDir", "SubDirectory": sub}
            if allow_reprocess:
                tables["Test"]["meta"]["VARS"] = {"ALLOW_REPROCESS": 1}
            tables["Child"] = {"meta": {"FORMAT": "int8u"}, "tags": {"1": {"Name": "ChildTag"}}}
            if extra:
                extra(tables)
            return data

        # 13.59 passes NotDup, so the fixed-Start child at its parent's address is processed.
        generator.render(with_child("13.59"))
        # 12.64 does not: the same child is a duplicate address unless the parent allows it.
        with self.assertRaisesRegex(generator.Unsupported, "without ALLOW_REPROCESS"):
            generator.render(with_child("12.64"))
        generator.render(with_child("12.64", allow_reprocess=True))
        # A fixed-Start child at a nonzero offset cannot be its parent's address.
        def at_two(tables):
            tables["Test"]["tags"]["2"] = tables["Test"]["tags"].pop("0")
        generator.render(with_child("12.64", extra=at_two))
        # A data-chosen `$val` target beside a fixed one could coincide with it.
        def mixed(tables):
            tables["Test"]["tags"]["3"] = {"Name": "Other", "Format": "int32u", "SubDirectory": {
                "TagTable": "Image::ExifTool::Nikon::Child", "Start": "$val"}}
        with self.assertRaisesRegex(generator.Unsupported, "mixes fixed-Start and \\$val-Start"):
            generator.render(with_child("12.64", allow_reprocess=True, extra=mixed))
        # A fixed-Start child with its own subdirectories could collide with a later sibling.
        def nested(tables):
            tables["Child"]["tags"]["2"] = {"Name": "Grandchild", "SubDirectory": {
                "TagTable": "Image::ExifTool::Nikon::Leaf"}}
            tables["Leaf"] = {"meta": {"FORMAT": "int8u"}, "tags": {}}
        with self.assertRaisesRegex(generator.Unsupported, "has subdirectories"):
            generator.render(with_child("12.64", allow_reprocess=True, extra=nested))

    def test_11_78_callback_is_an_explicit_pinned_omission(self):
        text, counts = generator.render(real_closure_fixture("11.78", pbd_release="13.59", label="11.78"))
        self.assertEqual(counts, {"tables": 0, "rows": 0, "maps": 0, "omitted_roots": 3})
        self.assertIn("pub static TABLES: &[BinTable] = &[\n];", text)
        self.assertEqual(text.count("encrypted: None }"), 3)
        self.assertEqual(text.count("// omitted: DecryptLen/DecryptMore partial decryption"), 3)
        # The Rust runtime still links against the release's own @xlat bytes.
        self.assertIn("pub static XLAT0: [u8; 256]", text)
        # The omission pins the whole closure: a changed 11.78 helper refuses.
        data = real_closure_fixture("11.78", pbd_release="13.59")
        helper = data["modules"]["Nikon"]["tables"]["Main"]["tags"]["152"]["SubDirectory"]["ProcessProc"]["dependencies"]["Image::ExifTool::Nikon::Decrypt"]
        helper["__deparse"] += " "
        with self.assertRaisesRegex(generator.Unsupported, "unregistered helper body"):
            generator.render(data)

    def test_modeled_encrypted_root_keys_are_closed(self):
        data = real_closure_fixture("13.59")
        data["modules"]["Nikon"]["tables"]["Main"]["tags"]["152"]["SubDirectory"]["DecryptLen"] = "100"
        with self.assertRaisesRegex(generator.Unsupported, "unmodeled encrypted SubDirectory keys \\['DecryptLen'\\]"):
            generator.render(data)

    def test_decryptstart_only_root_binds_to_a_verified_closure(self):
        data = real_closure_fixture("13.59")
        tables = data["modules"]["Nikon"]["tables"]
        root = tables["Main"]["tags"]["145"]["SubDirectory"]
        root.pop("ProcessProc")
        tables["Shot"] = {"meta": {"FORMAT": "int8u"}, "tags": {"1": {"Name": "ShotTag"}}}
        root["TagTable"] = "Image::ExifTool::Nikon::Shot"
        proc = copy.deepcopy(FORMS["13.59"]["callback"])
        proc.pop("dependencies")
        tables["Shot"]["meta"]["PROCESS_PROC"] = proc
        text, _ = generator.render(data)
        self.assertIn('name: "ShotTag"', text)
        proc["source_sha256"] = "0" * 64
        with self.assertRaisesRegex(generator.Unsupported, "not bound to a verified callback closure"):
            generator.render(data)


class OmittedTableLedgerTests(unittest.TestCase):
    def setUp(self):
        # Cleanups run last-in first-out: empty the ledger, then restore it.
        self.addCleanup(generator.OMITTED_TABLES.update, dict(generator.OMITTED_TABLES))
        self.addCleanup(generator.OMITTED_TABLES.clear)
        generator.OMITTED_TABLES.clear()

    def unmodeled(self):
        data = fixture()
        tables = data["modules"]["Nikon"]["tables"]
        tables["Parent"] = {"meta": {"FORMAT": "int8u"}, "tags": {
            "4": {"Name": "Kid", "Condition": "$$self{FirmwareVersion}", "Hook": '$varSize += 4 if $$self{FirmwareVersion} and $$self{FirmwareVersion} ge "1.10"',
                  "SubDirectory": {"TagTable": "Image::ExifTool::Nikon::Test"}},
            "9": {"Name": "After"}}}
        tables["Main"]["tags"]["145"]["SubDirectory"]["TagTable"] = "Image::ExifTool::Nikon::Parent"
        tables["Test"]["tags"]["1"]["PrintConv"] = {"kind": "expr", "expr": "unmodeled($val)"}
        return data

    def record(self, data, name="Test", refusal=None):
        table = data["modules"]["Nikon"]["tables"][name]
        if refusal is None:
            try:
                generator.render(data)
            except generator.Unsupported as error:
                refusal = str(error)
        generator.OMITTED_TABLES[("Nikon", name, generator.table_digest(table))] = {
            "refusal_sha256": hashlib.sha256(refusal.encode()).hexdigest(), "note": "test omission"}

    def test_unrecorded_unmodeled_table_is_a_hard_error(self):
        with self.assertRaisesRegex(generator.Unsupported, "unregistered PrintConv"):
            generator.render(self.unmodeled())

    def test_recorded_exact_table_is_omitted_and_parent_row_stays_selected(self):
        data = self.unmodeled()
        self.record(data)
        text, counts = generator.render(data)
        self.assertEqual(counts["omitted_tables"], 1)
        self.assertNotIn("TAGS_TEST", text)
        self.assertIn("// Nikon::Test omitted: test omission", text)
        kid = next(line for line in text.splitlines() if 'name: "Kid"' in line)
        # Condition and Hook still apply; the row extracts nothing.
        self.assertIn("cond: Cond::Truthy(Dm::FirmwareVersion)", kid)
        self.assertIn('hook: Hook::AddIfFirmwareGe(4, "1.10")', kid)
        self.assertIn("unknown: true", kid)
        self.assertIn("subdir: None", kid)
        self.assertIn('name: "After"', text)
        # Pointed at directly by a root, the omission leaves that root inert.
        direct = fixture()
        direct["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]["PrintConv"] = {"kind": "expr", "expr": "unmodeled($val)"}
        generator.OMITTED_TABLES.clear()
        self.record(direct)
        text, counts = generator.render(direct)
        self.assertEqual(counts["omitted_roots"], 3)
        self.assertIn("// omitted: Nikon::Test: test omission", text)

    def test_changed_content_is_not_covered_by_the_recorded_omission(self):
        data = self.unmodeled()
        self.record(data)
        data["modules"]["Nikon"]["tables"]["Test"]["tags"]["1"]["Name"] = "Renamed"
        with self.assertRaisesRegex(generator.Unsupported, "unregistered PrintConv"):
            generator.render(data)

    def test_stale_or_mismatched_omission_is_refused(self):
        data = fixture()
        self.record(data, refusal="whatever it used to be")
        with self.assertRaisesRegex(generator.Unsupported, "recorded omission is stale"):
            generator.render(data)
        generator.OMITTED_TABLES.clear()
        data = self.unmodeled()
        self.record(data, refusal="a different refusal")
        with self.assertRaisesRegex(generator.Unsupported, "no longer matches"):
            generator.render(data)


if __name__ == "__main__":
    unittest.main()
