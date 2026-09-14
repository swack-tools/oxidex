"""Generated CheckExif operands retain source selectors and helper identity."""
import unittest
import json
import os
import subprocess
import tempfile
from dataclasses import replace
from pathlib import Path
from unittest.mock import patch

import checkexif_rust_codegen as codegen
from checkexif_recipes import RecipeMalformed, RecipeRefused
from test_checkexif_recipes import BODY, document

SCALAR_BODY = (Path(__file__).parent / "testdata/checkvalue_scalar_body.txt").read_text()


def ready_document(body=BODY):
    value = document(body)
    value["native_write_helpers"]["check_value"]["__deparse"] = SCALAR_BODY
    return value


class CheckExifRustCodegenTests(unittest.TestCase):
    def test_emits_ordered_selectors_and_canonical_dependency(self):
        source, report = codegen.generate(ready_document())
        self.assertEqual(report["report"]["recipes_emitted"], 1)
        self.assertIn('property: "Format"', source)
        self.assertLess(source.index('property: "Format"'), source.index('property: "Writable"'))
        self.assertLess(source.index('property: "Writable"'), source.index('property: "WRITABLE"'))
        self.assertIn('SelectorSource::TagGroup', source)
        self.assertIn('check_value: ScalarCheckRecipe', source)
        self.assertNotIn('check_value_binding', source)
        self.assertNotIn('check_value_provenance', source)

    def test_source_changes_regenerate_operands_without_tag_exceptions(self):
        changed = ready_document(BODY.replace("{'Count'}", "{'WriteCount'}").replace("No writable format", "changed error"))
        first, _ = codegen.generate(ready_document())
        second, report = codegen.generate(changed)
        self.assertNotEqual(first, second)
        self.assertIn('property: "WriteCount"', second)
        self.assertIn('other_error: "changed error"', second)
        self.assertEqual(report["recipes"][0]["source_tables"][0][2], "Image::ExifTool::Exif::Main")

    def test_unsupported_is_reported_and_malformed_is_not_downgraded(self):
        unsupported = ready_document(BODY.replace("return Image::ExifTool::CheckValue", "return Image::ExifTool::OtherValue"))
        source, report = codegen.generate(unsupported)
        self.assertIn('CHECK_EXIF_RECIPES: &[CheckExifRecipe] = &[\n];', source)
        self.assertEqual(report["report"]["omitted_tables"], 1)
        malformed = ready_document()
        malformed["native_write_tables"]["Exif"]["Main"]["full_name"] = "bad"
        with self.assertRaises(RecipeMalformed):
            codegen.generate(malformed)

    def test_unavailable_or_different_checkvalue_omits_checkexif_recipe(self):
        unsupported_helper = ready_document()
        unsupported_helper["native_write_helpers"]["check_value"]["__deparse"] = "($$) { die 'changed'; }"
        source, report = codegen.generate(unsupported_helper)
        self.assertIn('CHECK_EXIF_RECIPES: &[CheckExifRecipe] = &[\n];', source)
        self.assertEqual(report["report"]["recipes_emitted"], 0)
        self.assertEqual(report["report"]["omitted_tables"], 1)
        self.assertIn("CheckValue scalar dependency", report["omitted_tables"][0]["reason"])

        recipes, source_report, source_omissions = codegen.compile_recipes(ready_document())
        wrong_provenance = replace(recipes[0].check_value, source_sha256="b" * 64)
        mismatched_recipe = replace(recipes[0], check_value=wrong_provenance)
        with patch.object(
            codegen,
            "compile_recipes",
            return_value=([mismatched_recipe], source_report, source_omissions),
        ):
            _source, report = codegen.generate(ready_document())
        self.assertEqual(report["report"]["recipes_emitted"], 0)
        self.assertEqual(
            report["omitted_tables"][0]["reason"],
            "CheckValue scalar dependency provenance differs",
        )

    def test_unrepresentable_scalar_operand_is_a_named_omission(self):
        source = ready_document()
        fact = source["native_write_helpers"]["check_value"]
        fact["__deparse"] = fact["__deparse"].replace("$count > 0", "$count > " + str(1 << 63))
        rendered, report = codegen.generate(source)
        self.assertIn('CHECK_EXIF_RECIPES: &[CheckExifRecipe] = &[\n];', rendered)
        self.assertEqual(report["report"]["omitted_tables"], 1)
        self.assertIn("i64", report["omitted_tables"][0]["reason"])


@unittest.skipUnless(os.environ.get('OXIDEX_TABLES_JSON'),
                     'requires fresh captured facts from the pinned release')
class NativeCheckExifArtifactFreshnessTests(unittest.TestCase):
    def test_committed_rules_and_ledger_equal_fresh_native_generation(self):
        root = Path(__file__).resolve().parents[2]
        source, report = codegen.generate(json.loads(Path(os.environ['OXIDEX_TABLES_JSON']).read_text()))
        with tempfile.TemporaryDirectory(prefix='oxidex-checkexif-freshness-') as directory:
            output = Path(directory) / 'rules.rs'
            output.write_text(source)
            subprocess.run(['rustfmt', '--edition', '2024', '--config-path', str(root / 'rustfmt.toml'),
                            str(output)], check=True, capture_output=True, timeout=30)
            self.assertEqual(output.read_text(), (root / 'src/writers/generated_checkexif_rules.rs').read_text(),
                             'committed validation rules are stale; run official regeneration')
        self.assertEqual(json.dumps(report, sort_keys=True, indent=2) + '\n',
                         (Path(__file__).parent / 'checkexif_ledger.json').read_text(),
                         'committed validation ledger is stale; run official regeneration')


if __name__ == "__main__":
    unittest.main()
