#!/usr/bin/env python3
"""Unit tests for the Autogeneration v2 backend (conv_codegen.py) and the
consistency of its committed outputs (the generated Rust arms, the ledger and
the pinned-Perl capture conv_oracle.py wrote)."""
import hashlib
import json
import re
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))
import conv_codegen as C  # noqa: E402


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
                           ("a", "m"), (r"[[:alpha:]]", ""), (r"\Qx", "")]:
            with self.assertRaises(C.Refuse, msg=pat):
                C.translate_regex(pat, flags)


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
        for body in ["@a", "$val[1]", "$val{x}", "$x->y"]:
            with self.assertRaises(C.Refuse, msg=body):
                C.interp_pieces(body)


def compile_one(tag):
    mod = C.Module()
    return C.compile_field(mod, 0x1234, tag), mod


class Fields(unittest.TestCase):
    def test_unported_helper_refuses_the_whole_field(self):
        tag = {"Name": "X", "ValueConv": {"kind": "expr", "expr": '$self->Decode($val,"UTF8")'},
               "PrintConv": {"kind": "expr", "expr": '"$val m"'}}
        with self.assertRaises(C.Refuse) as cm:
            compile_one(tag)
        self.assertIn("Decode", cm.exception.reason)

    def test_eval_site_lexicals_refuse(self):
        tag = {"Name": "X", "RawConv": {"kind": "expr", "expr": "Foo($tag)"}}
        with self.assertRaises(C.Refuse):
            compile_one(tag)
        tag = {"Name": "X", "ValueConv": {"kind": "expr", "expr": "$val . $tag"}}
        with self.assertRaises(C.Refuse):
            compile_one(tag)

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

    def test_capture_covers_exactly_this_generation(self):
        cap = json.loads(self.capture_path.read_text(encoding="utf-8"))
        self.assertEqual(cap["capture"]["ledger_rust_sha256"], self.ledger["rust_sha256"])
        self.assertEqual(cap["capture"]["exiftool_version"], self.ledger["exiftool_version"])
        self.assertEqual(sorted(cap["fields"]), sorted(g["id"] for g in self.ledger["generated"]))


if __name__ == "__main__":
    unittest.main()
