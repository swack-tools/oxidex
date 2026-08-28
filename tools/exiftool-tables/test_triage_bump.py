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


class GeneratorLessDerivationTests(unittest.TestCase):
    """Pin the standing-HAND list against the filesystem it describes.

    The defect these replace: `GENERATOR_LESS_FILES` was a literal listing
    all six bespoke-DSL files, and stayed that way after `regen-all.sh`
    tier 2d started generating two of them, so every bump report over-stated
    standing HAND work by 2.
    """

    REPO_ROOT = MODULE_PATH.resolve().parents[2]

    WIRED = {
        "src/parsers/tiff/makernotes/sony/main_extra_tables.rs",
        "src/parsers/tiff/makernotes/minolta_a100_tables.rs",
    }
    STILL_GENERATOR_LESS = {
        "src/parsers/tiff/makernotes/sony/enciphered_tables.rs",
        "src/parsers/tiff/makernotes/sony/plain_tables.rs",
        "src/parsers/tiff/makernotes/nikon/encrypted_tables.rs",
        "src/parsers/tiff/makernotes/nikon/settings_tables.rs",
    }

    def test_every_candidate_file_exists_on_disk(self):
        # The candidate half IS a literal (it encodes "hand-translated
        # through a per-file DSL", which nothing on disk records), so it
        # gets the check a literal needs: a renamed or deleted file must
        # fail here rather than silently drop out of every bump report.
        for _module, path, _source in triage_bump.BESPOKE_DSL_FILES:
            with self.subTest(path=path):
                self.assertTrue((self.REPO_ROOT / path).is_file(),
                                f"{path} named by BESPOKE_DSL_FILES does not exist")

    def test_derived_list_is_exactly_the_four_without_a_generator(self):
        derived = {path for _m, path, _s in triage_bump.generator_less_files()}
        self.assertEqual(derived, self.STILL_GENERATOR_LESS)

    def test_files_a_regen_script_generates_are_excluded(self):
        derived = {path for _m, path, _s in triage_bump.generator_less_files()}
        for path in self.WIRED:
            with self.subTest(path=path):
                self.assertNotIn(path, derived)
                # ...and for the right reason: a regen script really does
                # name it outside a comment.
                named = any(
                    path in triage_bump._strip_shell_comments(
                        (self.REPO_ROOT / rel).read_text(encoding="utf-8"))
                    for rel in triage_bump.REGEN_SCRIPTS
                )
                self.assertTrue(named, f"{path} is excluded but no regen script names it")

    def test_a_comment_naming_a_file_does_not_count_as_a_generator(self):
        # regen-all.sh's tier-2d banner names the four unreconstructed files
        # in prose. Counting that sentence is the `ricoh.rs:215` failure
        # (reachability.py's docstring; docs/reference/corpus-synthesis.md),
        # so the derivation strips comments first. Negative control: a
        # script whose ONLY mention of a path is inside a comment must leave
        # that path on the list.
        script = (
            '#!/usr/bin/env bash\n'
            '# note: src/parsers/tiff/makernotes/sony/main_extra_tables.rs '
            'is deliberately not generated here\n'
            'echo "unrelated # not a comment"\n'
        )
        stripped = triage_bump._strip_shell_comments(script)
        self.assertNotIn("main_extra_tables.rs", stripped)
        self.assertIn('echo "unrelated # not a comment"', stripped)

    def test_unreadable_regen_script_is_a_loud_refusal_not_an_empty_list(self):
        with self.assertRaises(SystemExit) as cm:
            triage_bump.generator_less_files(Path("/nonexistent-oxidex-root"))
        self.assertIn("refusing to guess", str(cm.exception))


if __name__ == "__main__":
    unittest.main()
