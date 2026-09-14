"""ConvInv artifact contract; native execution is tested separately."""
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from checkexif_recipes import RecipeMalformed
import convinv_rust_codegen as generator
from test_checkexif_recipes import fact

ROOT = Path(__file__).resolve().parents[2]


def document():
    tokens = json.loads((Path(__file__).with_name("convinv_full_template.json")).read_text())
    tokens = ["'PrintConv'" if token == "<DEFAULT_TYPE>" else
              '\"$err2 for ${wgrp1}:$tag\"' if token == "<ERROR_LITERAL>" else token for token in tokens]
    helper = fact("Image::ExifTool::ConvInv", " ".join(tokens),
                  requested="Image::ExifTool::ConvInv", source="Image/ExifTool/Writer.pl")
    return {"exiftool_version": "synthetic", "native_write_helpers": {"conv_inv": helper}}


class ConvInvCodegen(unittest.TestCase):
    def test_supported_shape_emits_optional_recipe_and_source_operand(self):
        source, report = generator.generate(document())
        self.assertTrue(report["emitted"])
        self.assertIn("Some(ConvInvRecipe", source)
        self.assertEqual(report["recipe"]["error_separator"], " for ")

    def test_missing_or_changed_source_emits_explicit_omission(self):
        changed = deepcopy(document())
        changed["native_write_helpers"]["conv_inv"]["__deparse"] += " unexpected_statement;"
        for value in ({}, changed):
            source, report = generator.generate(value)
            self.assertFalse(report["emitted"])
            self.assertIn("= None;", source)
            self.assertTrue(report["reason"])

    def test_malformed_envelope_does_not_become_supported_omission(self):
        for value in (None, [], {"native_write_helpers": []}):
            with self.assertRaises(RecipeMalformed):
                generator.generate(value)


@unittest.skipUnless(os.environ.get("OXIDEX_TABLES_JSON"), "requires fresh official native capture")
class ConvInvFreshness(unittest.TestCase):
    def test_registered_artifacts_match_fresh_generation(self):
        source, report = generator.generate(json.loads(Path(os.environ["OXIDEX_TABLES_JSON"]).read_text()))
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "rules.rs"
            path.write_text(source)
            subprocess.run(["rustfmt", "--edition", "2024", "--config-path", str(ROOT / "rustfmt.toml"), str(path)],
                           check=True, capture_output=True, timeout=30)
            self.assertEqual(path.read_text(), (ROOT / "src/writers/generated_convinv_rules.rs").read_text())
        self.assertEqual(json.dumps(report, sort_keys=True, indent=2) + "\n",
                         (ROOT / "tools/exiftool-tables/convinv_ledger.json").read_text())
