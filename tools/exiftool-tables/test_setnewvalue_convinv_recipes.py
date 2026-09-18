"""Closed source admission tests for SetNewValue's ConvInv result caller."""
import json
from pathlib import Path
import unittest

from checkexif_recipes import RecipeRefused
import native_reader_facts
import setnewvalue_convinv_recipes
from setnewvalue_convinv_recipes import compile_setnewvalue_convinv
from setnewvalue_convinv_rust_codegen import generate


TEMPLATE = json.loads(Path(__file__).with_name("setnewvalue_convinv_full_template.json").read_text())
HISTORICAL = {
    release: json.loads(Path(__file__).with_name(
        f"setnewvalue_convinv_full_template_{release.replace('.', '_')}.json").read_text())
    for release in ("11.78", "12.64")
}


def conv_inv_fact():
    return {
        "__perl": "CODE", "resolved": True,
        "__name": "Image::ExifTool::ConvInv",
        "source_file": "Image/ExifTool/Writer.pl", "source_sha256": "b" * 64,
        "__deparse": "return;", "dependencies": {},
    }


def fact(tokens=TEMPLATE):
    return {
        "__perl": "CODE", "resolved": True,
        "requested_binding": "Image::ExifTool::SetNewValue",
        "__name": "Image::ExifTool::SetNewValue",
        "source_file": "Image/ExifTool/Writer.pl", "source_sha256": "a" * 64,
        "__deparse": " ".join(tokens),
        "dependencies": {"Image::ExifTool::ConvInv": conv_inv_fact()},
    }


class SetNewValueConvInvRecipeTests(unittest.TestCase):
    def test_complete_authenticated_caller_emits_three_way_operand(self):
        source = fact()
        self.assertEqual(native_reader_facts.body_tokens(source["__deparse"]), TEMPLATE)
        recipe = compile_setnewvalue_convinv(source)
        self.assertEqual((recipe.undefined_error, recipe.defined_false_error, recipe.truthy_error),
                         ("continue", "skip_tag", "refuse"))
        rust, report = generate({"native_write_helpers": {"set_new_value": source}})
        self.assertTrue(report["emitted"])
        self.assertIn("defined_false_error: ConvInvErrorResult::SkipTag", rust)
        self.assertIn("truthy_error: ConvInvErrorResult::Refuse", rust)
        self.assertEqual(report["runtime_status"],
                         "inactive_source_operand_no_public_setnewvalue_admission")
        json.dumps(report)

    def test_changed_defined_false_control_is_refused(self):
        tokens = list(TEMPLATE)
        conv_inv = tokens.index("ConvInv")
        at = next(index for index in range(conv_inv, len(tokens) - 6)
                  if tokens[index:index + 6] == ["if", "(", "defined", "(", "$", "e"])
        tokens[at + 2] = "not"
        with self.assertRaisesRegex(RecipeRefused, "caller control flow"):
            compile_setnewvalue_convinv(fact(tokens))

    def test_inserted_statement_around_convinv_call_is_refused(self):
        tokens = list(TEMPLATE)
        at = tokens.index("ConvInv")
        tokens[at:at] = ["warn", "'changed'", ";"]
        with self.assertRaisesRegex(RecipeRefused, "caller control flow"):
            compile_setnewvalue_convinv(fact(tokens))

    def test_rebound_or_missing_source_emits_named_omission(self):
        source = fact()
        source["requested_binding"] = "Image::ExifTool::Other"
        rust, report = generate({"native_write_helpers": {"set_new_value": source}})
        self.assertFalse(report["emitted"])
        self.assertIn("binding", report["reason"])
        self.assertIn("= None", rust)

    def test_rebound_actual_callable_is_refused_even_with_canonical_body(self):
        source = fact()
        source["__name"] = "Image::ExifTool::Other"
        with self.assertRaisesRegex(RecipeRefused, "unresolved or rebound"):
            compile_setnewvalue_convinv(source)

    def test_missing_direct_convinv_binding_is_refused(self):
        source = fact()
        source["dependencies"] = {}
        with self.assertRaisesRegex(RecipeRefused, "direct ConvInv"):
            compile_setnewvalue_convinv(source)


if __name__ == "__main__":
    unittest.main()


class HistoricalReleaseTests(unittest.TestCase):
    """The selected 11.78/12.64 rehearsal pair, captured from immutable sources.

    Both bodies differ from 13.59 in many places (11.78: 169 differing regions),
    but the ConvInv error handling this recipe models is byte-identical in all
    three -- checked on raw B::Deparse, since the tokenizer folds that region
    into a single token on 12.64.
    """

    def test_each_selected_release_body_admits_the_same_three_way_recipe(self):
        for release, tokens in HISTORICAL.items():
            with self.subTest(release):
                recipe = compile_setnewvalue_convinv(fact(tokens))
                self.assertEqual(
                    (recipe.undefined_error, recipe.defined_false_error, recipe.truthy_error),
                    ("continue", "skip_tag", "refuse"))

    def test_historical_bodies_really_differ_from_the_pin(self):
        # Guards the premise: if these ever became copies of 13.59, the
        # historical templates would be admitting nothing new.
        for release, tokens in HISTORICAL.items():
            with self.subTest(release):
                self.assertNotEqual(tokens, TEMPLATE)

    def test_a_historical_body_with_an_inserted_statement_is_still_refused(self):
        tokens = list(HISTORICAL["11.78"])
        at = tokens.index("ConvInv")
        tokens[at:at] = ["warn", "'changed'", ";"]
        with self.assertRaisesRegex(RecipeRefused, "caller control flow"):
            compile_setnewvalue_convinv(fact(tokens))


class ConvInvOperandTests(unittest.TestCase):
    """A template match alone must not admit a body.

    The operand is what stops a newly captured release template from silently
    extending the three-way recipe to ConvInv handling that has changed.
    """

    def with_templates(self, *templates):
        original = setnewvalue_convinv_recipes._templates
        setnewvalue_convinv_recipes._templates = lambda: templates
        self.addCleanup(setattr, setnewvalue_convinv_recipes, "_templates", original)

    def test_a_template_match_without_the_modeled_region_is_refused(self):
        tokens = list(TEMPLATE)
        conv_inv = tokens.index("ConvInv")
        at = next(index for index in range(conv_inv, len(tokens) - 6)
                  if tokens[index:index + 6] == ["if", "(", "defined", "(", "$", "e"])
        tokens[at + 2] = "not"          # defined($e) -> not($e): different control flow
        self.with_templates(tokens)     # ...yet pretend a release shipped exactly this
        with self.assertRaisesRegex(RecipeRefused, "not the modeled operand"):
            compile_setnewvalue_convinv(fact(tokens))

    def test_a_duplicated_convinv_caller_is_refused(self):
        tokens = list(TEMPLATE)
        start = tokens.index("ConvInv") - 10
        region = tokens[start:start + 60]
        doubled = tokens[:start] + region + tokens[start:]
        self.with_templates(doubled)
        with self.assertRaisesRegex(RecipeRefused, "not the modeled operand"):
            compile_setnewvalue_convinv(fact(doubled))

    def test_the_operand_is_present_once_in_every_committed_template(self):
        import re
        for name, tokens in [("13.59", TEMPLATE), *HISTORICAL.items()]:
            with self.subTest(name):
                self.assertEqual(
                    re.sub(r"\s+", "", " ".join(tokens)).count(
                        setnewvalue_convinv_recipes._CONVINV_OPERAND), 1)

