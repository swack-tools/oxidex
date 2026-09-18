import unittest
from convinv_recipes import scalar_fallthrough, ConvInvRecipe
from checkexif_recipes import RecipeRefused

class ScalarFallthrough(unittest.TestCase):
 def test_scalar_states_and_presence_refusal(self):
  r=ConvInvRecipe(None)
  for value in (b'a\0', 'é\0', None, ''): self.assertEqual(scalar_fallthrough(r,value,{}),(value,None))
  for key in ('PrintConv','PrintConvInv','ValueConv','ValueConvInv'):
   with self.subTest(key=key):
    with self.assertRaises(RecipeRefused): scalar_fallthrough(r,'x',{key:None})
 def test_gates_refuse(self):
  r=ConvInvRecipe(None)
  for row in ({'List':1},{'RawJoin':1},{'WriteCheck':1},{'RawConvInv':1},{'Table':{'CHECK_PROC':1}}):
   with self.assertRaises(RecipeRefused): scalar_fallthrough(r,'x',row)


import json
import re
from dataclasses import replace
from pathlib import Path

import convinv_recipes
from convinv_recipes import compile_convinv, compile_convinv_profile

TESTDATA = Path(__file__).with_name("testdata")
FACT_SHA = {  # Writer.pl of each immutable selected source tree
    "11.78": "61a793c72c00c8860b8c2bc6c45d3ae251c22f5b412c18fcc8c4a8518b116d91",
    "12.64": "51ce8d57d9456594934f4161a9088f5145f1669973089b41e78241d7250ed8ad",
    "13.59": "cfe916df77f7b37a4fc62e22f0c03de93ca727f0fa22a155ad0eecf95c050d44",
}


def captured(release):
    return json.loads((TESTDATA / f"convinv_{release.replace('.', '_')}_fact.json").read_text())


class PerReleaseProfiles(unittest.TestCase):
    """Real B::Deparse ConvInv bodies captured from each release's own tree."""

    def test_each_release_admits_only_its_own_profile_and_the_same_recipe(self):
        for release in FACT_SHA:
            with self.subTest(release):
                fact = captured(release)
                self.assertEqual(fact["source_sha256"], FACT_SHA[release])
                recipe, profile = compile_convinv_profile(fact)
                self.assertEqual(profile.captured_from, release)
                self.assertEqual((recipe.default_type, recipe.error_separator), ("PrintConv", " for "))
                self.assertEqual(bool(profile.equivalences), release != "13.59")

    def test_templates_are_faithful_captures_of_the_bodies(self):
        # Filling the two placeholders must reproduce body_tokens exactly.
        import native_reader_facts
        for profile in convinv_recipes._PROFILES:
            with self.subTest(profile.captured_from):
                tokens = native_reader_facts.body_tokens(captured(profile.captured_from)["__deparse"])
                filled = ["'PrintConv'" if t == "<DEFAULT_TYPE>" else
                          '"$err2 for ${wgrp1}:$tag"' if t == "<ERROR_LITERAL>" else t
                          for t in convinv_recipes._template(profile)]
                self.assertEqual(tokens, filled)

    def test_historical_bodies_really_differ_from_the_pin(self):
        pin = captured("13.59")["__deparse"]
        for release in ("11.78", "12.64"):
            with self.subTest(release):
                self.assertNotEqual(captured(release)["__deparse"], pin)

    def test_version_label_never_admits(self):
        # A 13.59 body relabeled as 11.78 still matches only the 13.59 profile,
        # and an 11.78 body relabeled as 13.59 only the 11.78 one.
        for body_from, label in (("13.59", "11.78"), ("11.78", "13.59")):
            with self.subTest(body_from):
                fact = captured(body_from)
                fact["source_sha256"] = FACT_SHA[label]
                self.assertEqual(compile_convinv_profile(fact)[1].captured_from, body_from)

    def test_inserted_statement_in_a_historical_body_is_refused(self):
        fact = captured("11.78")
        fact["__deparse"] = fact["__deparse"].replace(
            "my($err, $type);", "my($err, $type); ($val = 'corrupt');", 1)
        with self.assertRaisesRegex(RecipeRefused, "unsupported statements|operand/order"):
            compile_convinv(fact)

    def test_hybrid_bodies_are_refused(self):
        # 11.78's three-argument CHECK_PROC call inside the 12.64 body, and the
        # 13.59 WriteCheck gate inside the 11.78 body, belong to no release.
        hybrids = (
            ("12.64", "(\\$val), $convType));", "(\\$val)));"),
            ("11.78", "unless ($err2) {", "unless (defined($err2)) {"),
        )
        for release, before, after in hybrids:
            with self.subTest(release):
                fact = captured(release)
                self.assertIn(before, fact["__deparse"])
                fact["__deparse"] = fact["__deparse"].replace(before, after, 1)
                with self.assertRaises(RecipeRefused):
                    compile_convinv(fact)

    def test_error_literal_outside_the_grammar_is_refused_in_a_historical_body(self):
        fact = captured("12.64")
        fact["__deparse"] = fact["__deparse"].replace(
            '"$err2 for ${wgrp1}:$tag"', '"$err2 \\n${wgrp1}:$tag"', 1)
        with self.assertRaises(RecipeRefused):
            compile_convinv(fact)


class ProfileOperands(unittest.TestCase):
    """A whole-body match alone must not admit: the modeled regions must hold."""

    def with_profiles(self, *profiles):
        original = convinv_recipes._PROFILES
        convinv_recipes._PROFILES = profiles
        self.addCleanup(setattr, convinv_recipes, "_PROFILES", original)

    def test_every_operand_occurs_exactly_once_in_its_own_release(self):
        for profile in convinv_recipes._PROFILES:
            compact = re.sub(r"\s+", "", captured(profile.captured_from)["__deparse"])
            for name, operand in profile.operands:
                operand = operand.replace("<ERROR_LITERAL>", '"$err2for${wgrp1}:$tag"').replace(
                    "<DEFAULT_TYPE>", "'PrintConv'")
                with self.subTest(profile.captured_from, operand=name):
                    self.assertEqual(compact.count(operand), 1)

    def test_a_template_match_without_the_modeled_region_is_refused(self):
        # Pretend 11.78 had shipped with 13.59's operands: the whole-body
        # template still matches, but its CHECK_PROC call is not 13.59's.
        wrong = replace(convinv_recipes._PROFILES[2], operands=convinv_recipes._PROFILES[0].operands)
        self.with_profiles(convinv_recipes._PROFILES[0], wrong)
        with self.assertRaisesRegex(RecipeRefused, "not the modeled operand"):
            compile_convinv(captured("11.78"))

    def test_a_duplicated_modeled_region_is_refused(self):
        import native_reader_facts
        fact = captured("11.78")
        call = "($err2 = &$checkProc($self, $tagInfo, (\\$val)));"
        self.assertEqual(fact["__deparse"].count(call), 1)
        fact["__deparse"] = fact["__deparse"].replace(call, call + "\n" + call, 1)
        # Admit the doubled body by an exact template so only the operand can refuse.
        doubled = native_reader_facts.body_tokens(fact["__deparse"])
        doubled[doubled.index('"$err2 for ${wgrp1}:$tag"')] = "<ERROR_LITERAL>"
        default = next(i for i in range(2, len(doubled))
                       if doubled[i] == "'PrintConv'" and doubled[i - 2:i] == ["|", "|"])
        doubled[default] = "<DEFAULT_TYPE>"
        original = convinv_recipes._template
        convinv_recipes._template = lambda profile: doubled
        self.addCleanup(setattr, convinv_recipes, "_template", original)
        self.with_profiles(convinv_recipes._PROFILES[2])
        with self.assertRaisesRegex(RecipeRefused, "CHECK_PROC scalar call is not the modeled operand"):
            compile_convinv(fact)

    def test_historical_writecheck_delta_is_unreachable_in_the_scalar_route(self):
        # The only 12.64 difference: its gate is unreachable because the
        # scalar route refuses every WriteCheck row before CHECK_PROC.
        recipe = compile_convinv(captured("12.64"))
        with self.assertRaisesRegex(RecipeRefused, "WriteCheck"):
            scalar_fallthrough(recipe, "x", {"WriteCheck": "1"})


if __name__ == '__main__':
    unittest.main()
