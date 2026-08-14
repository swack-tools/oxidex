#!/usr/bin/env python3
"""Focused regression tests for triage_bump.py's `_variants` classification.

Step 23 landed `conds.py` (a closed Condition grammar) and
`codegen.py`'s `compile_variant_group` (which compiles a `_variants` array's
alternatives through that grammar, all-or-nothing per array). Before this
change, `triage_bump.py` classified every added/changed `_variants` array as
COND unconditionally, which stopped being true the moment Step 23 landed.
These tests pin the new behaviour: a `_variants` array is AUTO exactly when
`conds.compile_cond()` accepts every alternative's Condition (mirroring
`compile_variant_group`'s all-or-nothing rule), and COND -- naming the
refusing construct -- the moment one does not.
"""

import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("triage_bump.py")
SPEC = importlib.util.spec_from_file_location("triage_bump", MODULE_PATH)
triage_bump = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(triage_bump)


class ClassifyVariantsTests(unittest.TestCase):
    def test_all_conditions_compile_is_auto(self):
        variants = [
            {"Condition": "$$self{Model} =~ /^ILCE-7/", "Name": "A", "Format": "int16u"},
            {"Name": "B", "Format": "int16u"},  # no Condition -> Cond::Always, always compiles
        ]
        delta = triage_bump.classify_variants("Sony", "Tag9400c", "42", variants, "changed")

        self.assertEqual(delta.bucket, triage_bump.AUTO)
        self.assertIn("compile_variant_group", delta.note)

    def test_one_refused_condition_is_cond_and_names_the_construct(self):
        # A three-way `or` chain is explicitly outside conds.py's grammar
        # (see conds.py's module docstring) -- this is the exact shape that
        # defeated the real Sony::Main tag 36944 / 37888 deltas in the
        # 13.58 -> 13.59 bump.
        variants = [
            {"Condition": "$$self{Model} =~ /^ILCE-7/", "Name": "A", "Format": "int16u"},
            {"Condition": "$$self{Model} =~ /^A/ or $$self{Model} =~ /^B/",
             "Name": "B", "Format": "int16u"},
        ]
        delta = triage_bump.classify_variants("Sony", "Main", "36944", variants, "changed")

        self.assertEqual(delta.bucket, triage_bump.COND)
        self.assertIn("alternative 1", delta.note)
        self.assertIn("'B'", delta.note)  # names the alternative by its Name
        self.assertIn("outside conds.py's closed grammar", delta.note)

    def test_lt_ge_string_compare_is_refused_and_named(self):
        # `lt`/`ge` string comparisons are one of the named grammar gaps
        # (Step 23's landing note: 11 of 12 refusals in the pinned corpus
        # are OR chains / lt-ge compares / \d-class regexes).
        variants = [{"Condition": '$$self{Model} lt "Z"', "Name": "Weird", "Format": "int16u"}]
        delta = triage_bump.classify_variants("Test", "Tbl", "1", variants, "added")

        self.assertEqual(delta.bucket, triage_bump.COND)
        self.assertIn("unrecognised condition atom", delta.note)

    def test_nested_variants_alternative_is_refused_outright(self):
        variants = [{"_variants": [{"Name": "Inner"}]}]
        delta = triage_bump.classify_variants("Test", "Tbl", "1", variants, "added")

        self.assertEqual(delta.bucket, triage_bump.COND)
        self.assertIn("nested _variants", delta.note)

    def test_non_dict_alternative_is_refused_outright(self):
        variants = ["not-a-dict"]
        delta = triage_bump.classify_variants("Test", "Tbl", "1", variants, "added")

        self.assertEqual(delta.bucket, triage_bump.COND)
        self.assertIn("non-dict shape", delta.note)


class DiffTagVariantsIntegrationTests(unittest.TestCase):
    """Exercise the real diff_tag() added/changed paths, not just the
    classify_variants() helper in isolation -- these are the two call sites
    the task named (added-tag path, changed-tag path)."""

    def test_added_tag_with_all_compiling_variants_is_auto(self):
        new_tag = {"_variants": [
            {"Condition": "$$self{Model} =~ /^ILCE-7/", "Name": "A", "Format": "int16u"},
            {"Name": "B", "Format": "int16u"},
        ]}
        deltas = list(triage_bump.diff_tag("Sony", "Tag9400c", "42", None, new_tag, True))

        self.assertEqual(len(deltas), 1)
        self.assertEqual(deltas[0].bucket, triage_bump.AUTO)
        self.assertEqual(deltas[0].kind, "added")

    def test_changed_tag_with_refused_variant_is_cond(self):
        old_tag = {"_variants": [{"Condition": "$$self{Model} =~ /^A/", "Name": "A", "Format": "int16u"}]}
        new_tag = {"_variants": [
            {"Condition": "$$self{Model} =~ /^A/", "Name": "A", "Format": "int16u"},
            {"Condition": "$$self{Model} =~ /^B/ or $$self{Model} =~ /^C/",
             "Name": "B", "Format": "int16u"},
        ]}
        deltas = list(triage_bump.diff_tag("Sony", "Main", "36944", old_tag, new_tag, True))

        self.assertEqual(len(deltas), 1)
        self.assertEqual(deltas[0].bucket, triage_bump.COND)

    def test_variants_array_replaced_by_plain_tag_is_hand_not_auto_or_cond(self):
        # The new shape has no _variants array left for conds.py to accept
        # or refuse at all -- not itself an AUTO/COND question.
        old_tag = {"_variants": [{"Condition": "$$self{Model} =~ /^A/", "Name": "A", "Format": "int16u"}]}
        new_tag = {"Name": "Plain", "Format": "int16u"}
        deltas = list(triage_bump.diff_tag("Sony", "Main", "1", old_tag, new_tag, True))

        self.assertEqual(len(deltas), 1)
        self.assertEqual(deltas[0].bucket, triage_bump.HAND)

    def test_standalone_condition_field_is_still_cond_not_auto(self):
        # A bare Condition on a non-variant (single-entry) tag is untouched
        # by Step 23 -- conds.py only ever sees Conditions inside a
        # _variants array's alternatives.
        old_tag = {"Name": "X", "Format": "int16u", "Condition": "$$self{Model} =~ /^A/"}
        new_tag = {"Name": "X", "Format": "int16u", "Condition": "$$self{Model} =~ /^B/"}
        deltas = list(triage_bump.diff_tag("Sony", "Tag9050d", "10", old_tag, new_tag, True))

        self.assertEqual(len(deltas), 1)
        self.assertEqual(deltas[0].bucket, triage_bump.COND)
        self.assertIn("standalone Condition", deltas[0].note)


if __name__ == "__main__":
    unittest.main()
