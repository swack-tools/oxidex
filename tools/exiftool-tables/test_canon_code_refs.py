"""CanonCustom CODE-ref recognition, collection, and oracle admission controls.

Bodies captured from ExifTool 13.59 CanonCustom.pm:2624-2628 via B::Deparse
under Perl 5.34.1 and 5.38.2. The small census fixture preserves PersonalFuncs'
29 tag IDs; fresh full-dump/oracle runs separately verify the real population.
"""
import json
from pathlib import Path
import tempfile
import unittest

import codegen
import exprs
import verify_exprs


KEY = "Image::ExifTool::CanonCustom::ConvertPfn($val)"
BODY_534 = '''($) {
    package Image::ExifTool::CanonCustom;
    use strict;
    (my $val = (shift()));
    (return ($val ? (($val == 1) ? 'On' : ("On ($val)")) : 'Off'));
}'''
BODY_538 = '''($) {
    package Image::ExifTool::CanonCustom;
    use strict;
    (my($val) = (shift()));
    (return ($val ? (($val == 1) ? 'On' : ("On ($val)")) : 'Off'));
}'''
PERSONAL_FUNC_IDS = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 14, 15, 16, 17,
                     18, 19, 20, 21, 22, 24, 25, 26, 27, 28, 29, 30, 31, 32)


def tag(body):
    return {"Name": "PersonalFunc", "Format": "int16u",
            "PrintConv": {"kind": "code", "deparse": body}}


class CanonCodeRefRecognition(unittest.TestCase):
    def test_both_captured_bodies_resolve_to_the_named_translation(self):
        for body in (BODY_534, BODY_538):
            with self.subTest(body=body):
                self.assertEqual(exprs.code_ref_expr(body), KEY)

    def test_only_outside_literal_whitespace_is_flexible(self):
        for body in (BODY_534, BODY_538):
            wrapped = "\n\t" + body.replace(";\n", ";\n\t").replace(" = ", "\t=\n") + "\n"
            self.assertEqual(exprs.code_ref_expr(wrapped), KEY)
            for literal in ('"On  ($val)"', '"On\t($val)"', '"On\n($val)"'):
                with self.subTest(literal=literal):
                    self.assertIsNone(exprs.code_ref_expr(body.replace('"On ($val)"', literal)))

    def test_semantic_changes_are_not_audited_bodies(self):
        for body in (BODY_534, BODY_538):
            for old, new in (("== 1", "== 2"), ("== 1", "!= 1"),
                             ("$val ?", "!$val ?"), ("shift()", "shift(@_)"),
                             ("'On'", "'Enabled'"), ("'Off'", "'OFF'"),
                             ('"On ($val)"', "'On ($val)'"),
                             ("use strict;", "use strict; $val += 1;")):
                with self.subTest(old=old, new=new):
                    self.assertIsNone(exprs.code_ref_expr(body.replace(old, new)))
        self.assertIsNone(exprs.code_ref_expr(None))
        self.assertIsNone(exprs.code_ref_expr(KEY))

    def test_existing_audited_code_refs_keep_resolving(self):
        for body, key in exprs.CODE_REFS.items():
            self.assertEqual(exprs.code_ref_expr(body), key)

    def test_collector_preserves_all_29_uses_for_each_captured_body(self):
        for body in (BODY_534, BODY_538):
            doc = {"exiftool_version": "13.59", "modules": {"CanonCustom": {"tables": {
                "PersonalFuncs": {"tags": {str(i): tag(body) for i in PERSONAL_FUNC_IDS}}
            }}}}
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "tables.json"
                path.write_text(json.dumps(doc))
                version, collected, _counts = verify_exprs.census(path)
            self.assertEqual(version, "13.59")
            self.assertEqual(collected, {KEY: 29})


class CanonCodeRefAdmission(unittest.TestCase):
    def test_missing_unrelated_or_body_text_verification_refuses(self):
        for body in (BODY_534, BODY_538):
            for verified in (None, set(), {"$val / 10"}, {body}):
                stats = codegen.new_ifd_stats()
                with self.subTest(body=body, verified=verified):
                    self.assertEqual(codegen.conv_for(tag(body), stats, "num", verified),
                                     ("PrintConv::None", True))
                    self.assertEqual(stats["expr_refused_oracle"], 1)
                    self.assertEqual(stats["expr_translated_code_ref"], 0)

    def test_verified_name_preserves_the_numeric_expression(self):
        for body in (BODY_534, BODY_538):
            stats = codegen.new_ifd_stats()
            self.assertEqual(codegen.conv_for(tag(body), stats, "num", {KEY}),
                             (f"PrintConv::Expr(ExprId::{codegen.expr_ident(KEY)})", False))
            self.assertEqual(stats["expr_translated_code_ref"], 1)

    def test_verified_name_does_not_authorize_a_different_input_domain(self):
        for body in (BODY_534, BODY_538):
            for domain in (None, "str", "bytes", "list"):
                stats = codegen.new_ifd_stats()
                self.assertEqual(codegen.conv_for(tag(body), stats, domain, {KEY}),
                                 ("PrintConv::None", True))
                self.assertEqual(stats["expr_refused_input_domain"], 1)

    def test_unrecognized_body_still_refuses_even_with_verified_name(self):
        stats = codegen.new_ifd_stats()
        self.assertEqual(codegen.conv_for(tag(BODY_538.replace("== 1", "== 2")),
                                         stats, "num", {KEY}), ("PrintConv::None", True))
        self.assertEqual(stats["conv_dropped"], 1)

    def test_ifd_emission_marks_missing_verification_as_omitted(self):
        stats = codegen.new_ifd_stats()
        context = codegen.IfdGenContext(ifd_tables=set(), binary_tables=set())
        source, _reason = codegen.gen_ifd_tag_literal(tag(BODY_538), 1, stats, set(),
                                                     context, {})
        self.assertIn("omitted: Omitted {", source)
        self.assertIn("print_conv: true", source.split("omitted: Omitted {", 1)[1].split("}", 1)[0])
        self.assertNotIn("PrintConv::Expr", source)


if __name__ == "__main__":
    unittest.main()
