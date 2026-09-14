"""Closed source-recipe tests for the shared native CheckExif helper."""
from __future__ import annotations

import json
from copy import deepcopy
import unittest

import checkexif_recipes as recipes


BODY = """($$$) {
    package Image::ExifTool::Exif;
    use strict;
    (my($et, $tagInfo, $valPtr) = @_);
    (my($format) = (($tagInfo->{'Format'} || $tagInfo->{'Writable'}) || $tagInfo->{'Table'}{'WRITABLE'}));
    if ((not($format) or ($format eq '1'))) {
        if (($tagInfo->{'Groups'}{'0'} eq 'MakerNotes')) {
            (return (undef));
        } else {
            (return 'No writable format');
        }
    }
    (return Image::ExifTool::CheckValue($valPtr, $format, $tagInfo->{'Count'}));
}"""


def fact(name: str, body: str, *, requested: str | None = None,
         source: str = "Image/ExifTool/WriteExif.pl") -> dict[str, object]:
    value: dict[str, object] = {
        "__perl": "CODE",
        "__name": name,
        "resolved": True,
        "__deparse": body,
        "source_file": source,
        "source_sha256": "a" * 64,
    }
    if requested is not None:
        value["requested_binding"] = requested
    return value


def document(body: str = BODY) -> dict[str, object]:
    result = {
        "native_write_helpers": {
            "check_value": fact(
                "Image::ExifTool::CheckValue",
                "($$) { package Image::ExifTool; return $_[0]; }",
                requested="Image::ExifTool::CheckValue",
                source="Image/ExifTool/Writer.pl",
            ),
        },
        "native_write_tables": {
            "Exif": {
                "Main": {
                    "module": "Exif",
                    "table": "Main",
                    "full_name": "Image::ExifTool::Exif::Main",
                    "effective_check_proc": {
                        "present": True,
                        "effective": fact("Image::ExifTool::Exif::CheckExif", body),
                    },
                },
            },
        },
    }

    # One fixture binding is observed at both sites; conflict tests replace one
    # observation explicitly. Real captures serialize these as separate facts.
    result["native_write_tables"]["Exif"]["Main"]["effective_check_proc"]["effective"]["dependencies"] = {
        "Image::ExifTool::CheckValue": result["native_write_helpers"]["check_value"]}
    return result


class CheckExifRecipeTests(unittest.TestCase):
    def compile(self, body: str = BODY):
        return recipes.compile_recipes(document(body))

    def test_canonical_body_emits_complete_ordered_recipe(self):
        emitted, report, omissions = self.compile()
        self.assertEqual((report.tables_seen, report.recipes_emitted, report.omitted_tables), (1, 1, 0))
        self.assertEqual(omissions, [])
        recipe = emitted[0]
        self.assertEqual(recipe.source_tables, (("Exif", "Main", "Image::ExifTool::Exif::Main"),))
        self.assertEqual(
            [(value.source, value.property) for value in recipe.format_selectors],
            [("tag", "Format"), ("tag", "Writable"), ("table", "WRITABLE")],
        )
        self.assertEqual(recipe.missing_format.equals_literal, "1")
        self.assertEqual(recipe.missing_format.group_source, recipes.Selector("tag.Groups", "0"))
        self.assertEqual(recipe.missing_format.maker_notes_literal, "MakerNotes")
        self.assertIsNone(recipe.missing_format.maker_return)
        self.assertEqual(recipe.missing_format.other_return, "No writable format")
        self.assertEqual(recipe.count_operand, recipes.Selector("tag", "Count"))
        self.assertEqual(recipe.check_value.requested_binding, "Image::ExifTool::CheckValue")
        self.assertEqual(recipe.check_value.actual_name, "Image::ExifTool::CheckValue")

    def test_final_loaded_explicit_call_and_implicit_argument_refusal(self):
        explicit = BODY.replace("return Image::ExifTool::CheckValue(",
                                "return &Image::ExifTool::CheckValue(")
        emitted, report, omissions = self.compile(explicit)
        self.assertEqual((len(emitted), report.omitted_tables, omissions), (1, 0, []))
        self.assertEqual(emitted[0].format_selectors, self.compile()[0][0].format_selectors)
        implicit = explicit.replace("($valPtr, $format, $tagInfo->{'Count'})", "")
        emitted, report, omissions = self.compile(implicit)
        self.assertEqual((emitted, report.omitted_tables), ([], 1))
        self.assertIn("closed grammar", omissions[0]["reason"])

    def test_check_value_dependency_binding_is_retained(self):
        input_doc = document()
        input_doc["native_write_helpers"]["check_value"]["dependencies"] = {
            "Image::ExifTool::IsInt": fact("Image::ExifTool::IsInt", "($) { return 1; }")
        }
        emitted, _report, _omissions = recipes.compile_recipes(input_doc)
        self.assertEqual(
            [(value.binding, value.fact.actual_name) for value in emitted[0].check_value.dependencies],
            [("Image::ExifTool::IsInt", "Image::ExifTool::IsInt")],
        )

    def test_missing_and_conflicting_callee_observations_refuse(self):
        for change in ('missing', 'body', 'source', 'nested'):
            with self.subTest(change=change):
                doc = document()
                deps = doc["native_write_tables"]["Exif"]["Main"]["effective_check_proc"]["effective"]["dependencies"]
                called = deepcopy(deps["Image::ExifTool::CheckValue"])
                deps["Image::ExifTool::CheckValue"] = called
                if change == 'missing':
                    deps.clear()
                elif change == 'body':
                    called["__deparse"] = "($$$) { die 'changed'; }"
                elif change == 'source':
                    called["source_sha256"] = "b" * 64
                else:
                    called["dependencies"] = {"Image::ExifTool::Other": fact("Image::ExifTool::Other", "{ return 1; }")}
                emitted, report, omissions = recipes.compile_recipes(doc)
                self.assertEqual((emitted, report.omitted_tables), ([], 1))
                self.assertIn("CHECK_PROC CheckValue dependency", omissions[0]["reason"])

    def test_reordered_selectors_and_changed_literals_regenerate_recipe(self):
        reordered = BODY.replace(
            "(($tagInfo->{'Format'} || $tagInfo->{'Writable'}) || $tagInfo->{'Table'}{'WRITABLE'})",
            "(($tagInfo->{'Writable'} || $tagInfo->{'Format'}) || $tagInfo->{'Table'}{'WRITABLE'})",
        )
        original = self.compile()[0][0]
        changed = self.compile(reordered)[0][0]
        self.assertNotEqual(changed.recipe_sha256, original.recipe_sha256)
        self.assertEqual(
            [(value.source, value.property) for value in changed.format_selectors],
            [("tag", "Writable"), ("tag", "Format"), ("table", "WRITABLE")],
        )

        literals = (BODY.replace("eq '1'", "eq 'fixed'")
                    .replace("eq 'MakerNotes'", "eq 'Camera'")
                    .replace("No writable format", "missing writable type")
                    .replace("{'Count'}", "{'WriteCount'}"))
        changed = self.compile(literals)[0][0]
        self.assertNotEqual(changed.recipe_sha256, original.recipe_sha256)
        self.assertEqual(changed.missing_format.equals_literal, "fixed")
        self.assertEqual(changed.missing_format.maker_notes_literal, "Camera")
        self.assertEqual(changed.missing_format.other_return, "missing writable type")
        self.assertEqual(changed.count_operand, recipes.Selector("tag", "WriteCount"))

    def test_early_return_and_extra_effect_refuse(self):
        early = BODY.replace(
            "    (my($et, $tagInfo, $valPtr) = @_);",
            "    (my($et, $tagInfo, $valPtr) = @_);\n    return undef;",
        )
        extra = BODY.replace(
            "    (return Image::ExifTool::CheckValue",
            "    $tagInfo->{'Touched'} = 1;\n    (return Image::ExifTool::CheckValue",
        )
        for source in (early, extra):
            emitted, report, omissions = self.compile(source)
            self.assertEqual(emitted, [])
            self.assertEqual((report.recipes_emitted, report.omitted_tables), (0, 1))
            self.assertIn("closed grammar", omissions[0]["reason"])

    def test_format_local_cannot_alias_any_signature_argument(self):
        for name in ("et", "tagInfo", "valPtr"):
            with self.subTest(name=name):
                emitted, report, omissions = self.compile(BODY.replace("my($format)", f"my(${name})"))
                self.assertEqual(emitted, [])
                self.assertEqual((report.recipes_emitted, report.omitted_tables), (0, 1))
                self.assertEqual(omissions[0]["reason"], "CHECK_PROC format local aliases an argument")

    def test_stale_binding_refuses_and_anonymous_actual_cv_is_preserved(self):
        stale = document()
        stale["native_write_helpers"]["check_value"]["requested_binding"] = "Image::ExifTool::OtherValue"
        emitted, report, omissions = recipes.compile_recipes(stale)
        self.assertEqual(emitted, [])
        self.assertEqual((report.recipes_emitted, report.omitted_tables), (0, 1))
        self.assertEqual(omissions[0]["reason"], "CHECK_PROC CheckValue binding is stale or unavailable")

        anonymous = document()
        anonymous["native_write_helpers"]["check_value"]["__name"] = "Image::ExifTool::__ANON__"
        emitted, report, omissions = recipes.compile_recipes(anonymous)
        self.assertEqual((len(emitted), report.omitted_tables, omissions), (1, 0, []))
        self.assertEqual(emitted[0].check_value.requested_binding, "Image::ExifTool::CheckValue")
        self.assertEqual(emitted[0].check_value.actual_name, "Image::ExifTool::__ANON__")

    def test_malformed_identity_and_missing_helper_fail_loudly(self):
        bad_identity = document()
        bad_identity["native_write_tables"]["Exif"]["Main"]["full_name"] = "Elsewhere"
        with self.assertRaisesRegex(recipes.RecipeMalformed, "identity"):
            recipes.compile_recipes(bad_identity)

        missing_helper = document()
        del missing_helper["native_write_helpers"]["check_value"]
        with self.assertRaisesRegex(recipes.RecipeMalformed, "not an object"):
            recipes.compile_recipes(missing_helper)

    def test_render_is_deterministic_and_stays_inactive(self):
        first = recipes.render(document())
        self.assertEqual(first, recipes.render(document()))
        parsed = json.loads(first)
        self.assertEqual(parsed["runtime_status"], recipes.RUNTIME_STATUS)
        self.assertEqual(len(parsed["recipes"]), 1)
        self.assertEqual(parsed["report"]["recipes_emitted"], 1)


if __name__ == "__main__":
    unittest.main()
