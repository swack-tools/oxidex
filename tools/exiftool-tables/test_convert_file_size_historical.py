#!/usr/bin/env python3
"""Source-backed Palm::MOBI ConvertFileSize code-ref admission."""

import json
import unittest
from collections import defaultdict
from pathlib import Path

import codegen
import exprs

HERE = Path(__file__).resolve().parent
FACT = json.loads((HERE / "testdata" / "convert_file_size_historical.json")
                  .read_text(encoding="utf-8"))
PINS = json.loads((HERE / "testdata" / "helper_source_pins.json")
                  .read_text(encoding="utf-8"))
KEY = "Image::ExifTool::ConvertFileSize($val)"


class ConvertFileSizeCodeRefs(unittest.TestCase):
    def test_each_native_body_admits_its_existing_oracle_translation(self):
        self.assertEqual(set(FACT["releases"]), {"11.78", "12.64", "13.59"})
        for version, fact in FACT["releases"].items():
            with self.subTest(version=version):
                self.assertEqual(fact["helper_source_sha256"],
                                 PINS[version]["helpers"]["Image::ExifTool::ConvertFileSize"])
                self.assertEqual(fact["palm"]["numeric"], "171966")
                self.assertEqual(fact["palm"]["printed"],
                                 "168 kB" if version == "11.78" else "172 kB")
                self.assertEqual(exprs.code_ref_expr(fact["deparse"]), KEY)
                tag = {"PrintConv": {"kind": "code", "deparse": fact["deparse"]}}
                emitted, refused = codegen.conv_for(tag, defaultdict(int), "num", {KEY})
                self.assertFalse(refused)
                self.assertEqual(emitted,
                                 f"PrintConv::Expr(ExprId::{codegen.expr_ident(KEY)})")
                self.assertEqual(codegen.conv_for(tag, defaultdict(int), "num", set()),
                                 ("PrintConv::None", True))
        self.assertEqual(exprs.translate(KEY)[0], "String")

    def test_an_unreviewed_body_is_refused(self):
        body = FACT["releases"]["12.64"]["deparse"]
        self.assertIsNone(exprs.code_ref_expr(body.replace("2000000", "2000001")))
        self.assertIsNone(exprs.code_ref_expr(None))


if __name__ == "__main__":
    unittest.main()
