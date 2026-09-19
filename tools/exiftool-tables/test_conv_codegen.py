#!/usr/bin/env python3
"""Unit tests for the Autogeneration v2 backend (conv_codegen.py) and the
consistency of its committed outputs (the generated Rust arms, the ledger and
the pinned-Perl capture conv_oracle.py wrote)."""
import hashlib
import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))
import conv_codegen as C  # noqa: E402
import artifacts as A  # noqa: E402
import conv_oracle as O  # noqa: E402


class RegexTranslation(unittest.TestCase):
    def test_dollar_becomes_end_of_text_and_is_reported(self):
        self.assertEqual(C.translate_regex(r"\s+$", ""), (r"(?-u)\s+\z", True))
        self.assertEqual(C.translate_regex(r"^(inf|undef)$", ""), (r"(?-u)^(inf|undef)\z", True))

    def test_nul_escape_and_flags(self):
        self.assertEqual(C.translate_regex(r"\0+$", "")[0], r"(?-u)\x00+\z")
        self.assertEqual(C.translate_regex("^(off|on)$", "i")[0], r"(?i-u)^(off|on)\z")
        self.assertEqual(C.translate_regex("^.*: ", "")[0], "(?-u)^.*: ")
        self.assertFalse(C.translate_regex("^.*: ", "")[1])

    def test_class_metacharacters_rust_would_read_as_operators_are_escaped(self):
        self.assertEqual(C.translate_regex(r"[&~[]", "")[0], r"(?-u)[\&\~\[]")

    def test_refusals(self):
        for pat, flags in [(r"(a)\1", ""), (r"(?=x)", ""), (r"$val", ""), ("a", "x"),
                           ("a", "m"), (r"[[:alpha:]]", ""), (r"\Qx", ""), ("\u00e9", "")]:
            with self.assertRaises(C.Refuse, msg=pat):
                C.translate_regex(pat, flags)

    def test_pattern_final_dollar_is_exact_where_allowed(self):
        # Perl's `$` is end-or-before-a-final-newline; at the very end of the
        # pattern it becomes the `eol` capture (rt keeps it out of an s///
        # span), elsewhere `\z` with the run-time decline flag.
        self.assertEqual(C.translate_regex(r"\s+$", "", eol_ok=True),
                         (r"(?-u)\s+(?P<eol>\n?)\z", False))
        self.assertEqual(C.translate_regex(r"(a$|b)", "", eol_ok=True),
                         (r"(?-u)(a\z|b)", True))


class Formats(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(C.parse_format("%.1f mm"),
                         [("spec", False, False, None, 1, "f"), ("lit", " mm")])
        self.assertEqual(C.parse_format("%.2x:%%"),
                         [("spec", False, False, None, 2, "x"), ("lit", ":"), ("lit", "%")])

    def test_refusals(self):
        for f in ["%e", "%*d", "%05s", "%v02x", "100%"]:
            with self.assertRaises(C.Refuse, msg=f):
                C.parse_format(f)


class Literals(unittest.TestCase):
    def test_rust_str(self):
        self.assertEqual(C.rust_str('a"\\\n\x03é'), '"a\\"\\\\\\n\\x03\\u{e9}"')

    def test_numbers(self):
        self.assertEqual(C.perl_num_literal("0xff"), "rt::int(255)")
        self.assertEqual(C.perl_num_literal("1_000"), "rt::int(1000)")
        self.assertEqual(C.perl_num_literal("0.5"), "rt::float(0.5_f64)")
        with self.assertRaises(C.Refuse):
            C.perl_num_literal("99999999999999999999")

    def test_interpolation_pieces(self):
        self.assertEqual(C.interp_pieces("${val} m"), [("var", "val"), ("lit", " m")])
        self.assertEqual(C.interp_pieces("$val C"), [("var", "val"), ("lit", " C")])
        # `$vals[1]` is an element of a declared array (the emitter checks
        # the array exists); a hash element or method call is refused.
        self.assertEqual(C.interp_pieces("-$vals[1]"), [("lit", "-"), ("elem", ("vals", 1))])
        for body in ["@a", "$val{x}", "$x->y", "$v[$i]"]:
            with self.assertRaises(C.Refuse, msg=body):
                C.interp_pieces(body)


def compile_one(tag):
    mod = C.Module()
    return C.compile_field(mod, 0x1234, tag), mod


class Fields(unittest.TestCase):
    def test_unported_helper_refuses_the_whole_field(self):
        tag = {"Name": "X", "RawConv": {"kind": "expr", "expr": "$self->SetPriorityDir(); $val"},
               "PrintConv": {"kind": "expr", "expr": '"$val m"'}}
        with self.assertRaises(C.Refuse) as cm:
            compile_one(tag)
        self.assertIn("SetPriorityDir", cm.exception.reason)

    def test_session_mutating_helpers_take_the_session_after_their_arguments(self):
        tag = {"Name": "X", "ValueConv": {"kind": "expr", "expr": '$self->Decode($val,"UCS2","II")'}}
        (src, slots, _), mod = compile_one(tag)
        body = "\n".join(mod.fns)
        self.assertIn("fn vc_1234(s: &mut Session", body)
        self.assertRegex(body, r"let t1 = val.clone\(\);.*h\(helpers::decode\(s, &t1, &t2, &t3, &t4, &t5\)\)")
        tag = {"Name": "X", "RawConv": {"kind": "expr",
                                        "expr": "Image::ExifTool::Exif::ConvertExifText($self,$val,1,$tag)"}}
        (_src, _slots, _), mod = compile_one(tag)
        self.assertIn('rt::string("X")', "\n".join(mod.fns))  # FoundTag's $tag
        with self.assertRaises(C.Refuse):  # $self first, or nothing
            compile_one({"Name": "X", "RawConv": {"kind": "expr", "expr":
                         "Image::ExifTool::Exif::ConvertExifText($val,1)"}})

    def test_options_reads_only_known_option_names(self):
        ok = {"Name": "X", "ValueConv": {"kind": "expr", "expr": "$self->Options('CharsetEXIF')"}}
        (_src, _slots, _), mod = compile_one(ok)
        self.assertIn('s.option("CharsetEXIF")', "\n".join(mod.fns))
        for name in ("charsetexif", "Verbose", "Bogus"):
            with self.assertRaises(C.Refuse, msg=name):
                compile_one({"Name": "X", "ValueConv": {"kind": "expr",
                             "expr": f"$self->Options('{name}')"}})

    def test_substitution_used_for_its_value(self):
        tag = {"Name": "X", "ValueConv": {"kind": "expr",
                                          "expr": "$val =~ s/^ab//i and $val = hex($val); $val"}}
        (_src, _slots, _), mod = compile_one(tag)
        body = "\n".join(mod.fns)
        self.assertIn("rt::subst_count(", body)
        self.assertIn("rt::hex(", body)
        with self.assertRaises(C.Refuse):
            compile_one({"Name": "X", "ValueConv": {"kind": "expr", "expr": "($val =~ s/a//g) + 1"}})

    def test_non_ascii_literal_refuses(self):
        with self.assertRaises(C.Refuse):
            compile_one({"Name": "X", "PrintConv": {"kind": "expr", "expr": '"$val \u00b5s"'}})
        with self.assertRaises(C.Refuse):
            compile_one({"Name": "X", "PrintConv": {"kind": "enum", "map": {"1": "\u00b5"}}})

    def test_eval_site_lexicals_refuse(self):
        tag = {"Name": "X", "RawConv": {"kind": "expr", "expr": "Foo($tag)"}}
        with self.assertRaises(C.Refuse):
            compile_one(tag)
        tag = {"Name": "X", "ValueConv": {"kind": "expr", "expr": "$val . $tag"}}
        with self.assertRaises(C.Refuse):
            compile_one(tag)

    def test_refusal_keys_recorded_only_by_name_still_refuse(self):
        tag = {"Name": "X", "Binary": "1", "_extra_keys": ["ConvertBinary", "WriteGroup"],
               "PrintConv": {"kind": "enum", "map": {"1": "a"}}}
        with self.assertRaises(C.Refuse) as cm:
            compile_one(tag)
        self.assertIn("ConvertBinary", cm.exception.reason)

    def test_member_write_is_rawconv_only(self):
        ok = {"Name": "X", "RawConv": {"kind": "expr", "expr": "$$self{X} = $val"}}
        (src, _slots, _binary), _mod = compile_one(ok)
        self.assertIn("arm_1234", src)
        bad = {"Name": "X", "ValueConv": {"kind": "expr", "expr": "$$self{X} = $val"}}
        with self.assertRaises(C.Refuse):
            compile_one(bad)

    def test_identity_and_binary_fields(self):
        (src, slots, binary), _ = compile_one({"Name": "X"})
        self.assertEqual((slots, binary), ({}, False))
        self.assertIn("value: None", src)
        (src, _slots, binary), _ = compile_one({"Name": "X", "Binary": "1"})
        self.assertTrue(binary)
        self.assertIn("Out::Binary", src)

    def test_hash_directive_other_than_bitmask_or_other_refuses(self):
        tag = {"Name": "X", "PrintConv": {"kind": "enum_partial", "map": {"1": "a"},
                                          "directives": {"PrintHex": 1}}}
        with self.assertRaises(C.Refuse):
            compile_one(tag)

    def test_non_ifd_key_shapes_are_refused_not_coerced(self):
        for key in ("01", "0x1", "name", "65536", "-1"):
            with self.subTest(key=key), tempfile.TemporaryDirectory() as td:
                dump = Path(td) / "dump.json"
                dump.write_text(json.dumps({
                    "exiftool_version": "13.59",
                    "modules": {"Synthetic": {"tables": {"Second": {
                        "tags": {key: {"Name": "X", "ValueConv": {
                            "kind": "expr", "expr": "$val + 1"}}}
                    }}}},
                }))
                with self.assertRaisesRegex(C.Refuse, "non-IFD tag key"):
                    C.generate(dump, "Synthetic::Second")


class Registry(unittest.TestCase):
    def entry(self, module, table, stem=None):
        return C.RegistryEntry(module, table, stem or C.table_stem(module, table))

    def test_multi_table_registry_is_sorted_and_routes_each_own_functions(self):
        text = C.render_registry([
            self.entry("Synthetic", "Second"),
            self.entry("Exif", "Main"),
        ])
        self.assertLess(text.index('module: "Exif"'), text.index('module: "Synthetic"'))
        self.assertIn("decode: synthetic_second::decode", text)
        self.assertIn("claims: synthetic_second::claims", text)
        self.assertNotIn("claims: exif_main::claims,\n        },\n        Entry {\n            module: \"Synthetic\"", text)

    def test_registry_rejects_duplicate_table_identity_and_stem(self):
        with self.assertRaisesRegex(ValueError, "duplicate conversion table identity"):
            C.render_registry([self.entry("Exif", "Main"), self.entry("Exif", "Main")])
        with self.assertRaisesRegex(ValueError, "duplicate conversion module stem"):
            C.render_registry([
                self.entry("One", "Main", "same"),
                self.entry("Two", "Main", "same"),
            ])
        with self.assertRaisesRegex(ValueError, "duplicate conversion module stem"):
            C.registry_with_candidate(
                [self.entry("One", "Main", "one_main")],
                self.entry("Different", "Table", "one_main"),
            )

    def test_registry_discovery_rejects_missing_orphan_and_stale_outputs(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            conv = root / "src/exiftool_tables/conv"
            tools = root / "tools/exiftool-tables"
            conv.mkdir(parents=True)
            tools.mkdir(parents=True)
            ledger = {
                "schema": C.LEDGER_SCHEMA,
                "table": "Exif::Main",
            }
            (tools / "conv_exif_main_ledger.json").write_text(json.dumps(ledger))
            with self.assertRaisesRegex(ValueError, "missing conversion module"):
                C.discover_registry(root)
            (conv / "exif_main.rs").write_text("// generated")
            (conv / "orphan.rs").write_text("// generated")
            with self.assertRaisesRegex(ValueError, "orphan conversion module"):
                C.discover_registry(root)
            (conv / "orphan.rs").unlink()
            entries = C.discover_registry(root)
            stale = C.replace_registry("prefix\n// BEGIN GENERATED CONVERSION REGISTRY\nstale\n"
                                       "// END GENERATED CONVERSION REGISTRY\nsuffix\n", entries)
            self.assertNotIn("stale", stale)
            self.assertEqual(stale, C.replace_registry(stale, entries))

    def test_table_identity_not_source_row_position_drives_stem(self):
        before = self.entry("Synthetic", "Second")
        after = self.entry("Synthetic", "Second")
        self.assertEqual(before.stem, after.stem)
        self.assertEqual(before.identity, "Synthetic::Second")

    def test_artifact_and_oracle_discovery_share_registry_identities(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            conv = root / "src/exiftool_tables/conv"
            tools = root / "tools/exiftool-tables"
            conv.mkdir(parents=True)
            (tools / "testdata").mkdir(parents=True)
            (conv / "mod.rs").write_text(
                f"{C.REGISTRY_BEGIN}\nold\n{C.REGISTRY_END}\n")
            for module, table in (("Exif", "Main"), ("Synthetic", "Second")):
                stem = C.table_stem(module, table)
                (conv / f"{stem}.rs").write_text("// generated")
                (tools / f"conv_{stem}_ledger.json").write_text(json.dumps({
                    "schema": C.LEDGER_SCHEMA,
                    "table": f"{module}::{table}",
                }))
            self.assertEqual(
                [a.key for a in A.conversion_artifacts(root)],
                ["conv-registry", "conv-synthetic_second", "conv-synthetic_second-ledger",
                 "conv-synthetic_second-oracle"],
            )
            self.assertEqual(list(O.discover_tables(root)), ["Exif::Main", "Synthetic::Second"])

    def test_oracle_check_ignores_only_interpreter_install_path(self):
        want = {"capture": {"perl": "/old/perl", "exiftool_version": "13.59"}, "fields": {}}
        got = {"capture": {"perl": "/new/perl", "exiftool_version": "13.59"}, "fields": {}}
        self.assertTrue(O.capture_matches(want, got))
        got["capture"]["exiftool_version"] = "13.60"
        self.assertFalse(O.capture_matches(want, got))


class CommittedOutputs(unittest.TestCase):
    """The generated file, its ledger and the oracle capture describe one
    generation."""

    rust = REPO / "src/exiftool_tables/conv/exif_main.rs"
    ledger_path = HERE / "conv_exif_main_ledger.json"
    capture_path = HERE / "testdata" / "conv_exif_main_outputs.json"

    def setUp(self):
        self.ledger = json.loads(self.ledger_path.read_text(encoding="utf-8"))

    def test_ledger_hashes_the_committed_rust(self):
        digest = hashlib.sha256(self.rust.read_bytes()).hexdigest()
        self.assertEqual(self.ledger["rust_sha256"], digest)

    def test_claimed_ids_are_the_ledgers_generated_fields(self):
        text = self.rust.read_text(encoding="utf-8")
        claimed = re.search(r"pub static CLAIMED: &\[u16\] = &\[(.*?)\];", text, re.S).group(1)
        ids = [int(x, 16) for x in re.findall(r"0x[0-9a-f]+", claimed)]
        self.assertEqual(ids, [int(g["id"], 16) for g in self.ledger["generated"]])
        self.assertEqual(len(ids), self.ledger["counts"]["generated"])

    def test_every_field_is_generated_refused_or_not_a_conversion_once(self):
        seen = [r["id"] for k in ("generated", "refused", "not_conversion_fields")
                for r in self.ledger[k]]
        self.assertEqual(len(seen), len(set(seen)))
        for r in self.ledger["refused"]:
            self.assertTrue(r["reason"])
            self.assertTrue(r.get("note"), r["name"])

    def test_capture_covers_exactly_this_generation(self):
        cap = json.loads(self.capture_path.read_text(encoding="utf-8"))
        self.assertEqual(cap["capture"]["ledger_rust_sha256"], self.ledger["rust_sha256"])
        self.assertEqual(cap["capture"]["exiftool_version"], self.ledger["exiftool_version"])
        self.assertEqual(sorted(cap["fields"]), sorted(g["id"] for g in self.ledger["generated"]))


if __name__ == "__main__":
    unittest.main()
