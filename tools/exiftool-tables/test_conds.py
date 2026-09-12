#!/usr/bin/env python3
"""`conds.py`'s closed `Condition` grammar, pinned shape by shape.

Two things are tested here, and they are different claims:

  1. STABILITY -- every shape the grammar accepted before slice I-3's
     tokenizer/precedence parser still compiles to byte-identical Rust
     text (`not $$self{X}` is a `negate: true` flag, an `and` chain is
     right-nested `Cond::And`). The committed tables are generated from
     this text; any churn here is churn there.
  2. THE CLOSED SET -- exactly the connective shapes the docstring names
     compile, at Perl's precedence, and everything else stays refused with
     a reason. `verify_cond.py` then proves the ACCEPTED shapes against the
     pinned Perl; a refusal needs no oracle, only a test that it stays one.

The parser's job is grouping, so several tests compare whole Rust trees:
a wrong precedence is a wrong tree, and a wrong tree is a wrong first-match
decision under a real tag name.
"""
import unittest

import conds

MODEL_RE = 'Cond::MemberRegex { member: "Model", pattern: "^E-M", ignore_case: false, negate: false }'
A = 'Cond::MemberTruthy { member: "A", negate: false }'
NOT_A = 'Cond::MemberTruthy { member: "A", negate: true }'
B = 'Cond::MemberTruthy { member: "B", negate: false }'
C = 'Cond::MemberTruthy { member: "C", negate: false }'
COUNT_NE_1 = "Cond::CountCmp { op: CmpOp::Ne, value: 1 }"
DEFINED_X = 'Cond::MemberDefined { member: "X", negate: false }'
NOT_DEFINED_X = 'Cond::MemberDefined { member: "X", negate: true }'


def and_(left, right):
    return f"Cond::And(&{left}, &{right})"


def or_(left, right):
    return f"Cond::Or(&{left}, &{right})"


class StabilityTests(unittest.TestCase):
    """Shapes accepted before slice I-3, and the exact text they produced."""

    def test_bare_member_and_its_negation_stay_flags(self):
        self.assertEqual(conds.compile_cond("$$self{A}"), A)
        self.assertEqual(conds.compile_cond("not $$self{A}"), NOT_A)

    def test_and_chain_is_right_nested(self):
        # `A and B and C` -> And(A, And(B, C)), the pre-I-3 text verbatim.
        self.assertEqual(
            conds.compile_cond("$$self{A} and $$self{B} and $$self{C}"),
            and_(A, and_(B, C)),
        )

    def test_set_member_idiom_with_a_trailing_and_chain(self):
        got = conds.compile_cond("($$self{N} = $count) and $$self{A} and $$self{B}")
        self.assertEqual(
            got,
            'Cond::SetMember { member: "N", source: EffectSource::Count, then: Some(&'
            + and_(A, B) + ") }",
        )
        self.assertEqual(
            conds.compile_cond("$$self{Flag} = 1"),
            'Cond::SetMember { member: "Flag", source: EffectSource::Const(1), then: None }',
        )

    def test_regex_body_is_never_split_on_a_connective(self):
        # ` or ` / `||` / `&&` inside the pattern are pattern characters.
        got = conds.compile_cond("$$self{Model} =~ /a or b\\|\\|c&&d/")
        self.assertEqual(
            got,
            'Cond::MemberRegex { member: "Model", pattern: "a or b\\\\|\\\\|c&&d", '
            "ignore_case: false, negate: false }",
        )
        got = conds.compile_cond('$$self{Model} eq "x and y"')
        self.assertEqual(
            got, 'Cond::MemberStrEq { member: "Model", value: "x and y", negate: false }'
        )

    def test_no_condition_is_always(self):
        self.assertEqual(conds.compile_cond(None), "Cond::Always")
        self.assertEqual(conds.compile_cond("   "), "Cond::Always")


class DisjunctionTests(unittest.TestCase):
    def test_olympus_focusinfo_0x1500(self):
        # Olympus.pm:3583.
        got = conds.compile_cond("$$self{Model} =~ /E-(1|M5)\\b/ || $count != 1")
        self.assertEqual(
            got,
            or_(
                'Cond::MemberRegex { member: "Model", pattern: "E-(1|M5)\\\\b", '
                "ignore_case: false, negate: false }",
                COUNT_NE_1,
            ),
        )

    def test_or_chain_is_right_nested_like_and(self):
        self.assertEqual(
            conds.compile_cond("$$self{A} or $$self{B} or $$self{C}"), or_(A, or_(B, C))
        )
        self.assertEqual(
            conds.compile_cond("$count == 48 or $count == 64"),
            or_(
                "Cond::CountCmp { op: CmpOp::Eq, value: 48 }",
                "Cond::CountCmp { op: CmpOp::Eq, value: 64 }",
            ),
        )

    def test_double_ampersand_is_and(self):
        self.assertEqual(conds.compile_cond("$$self{A} && $$self{B}"), and_(A, B))
        self.assertEqual(conds.compile_cond("$$self{A}&&$$self{B}"), and_(A, B))
        self.assertEqual(conds.compile_cond("$$self{A}||$$self{B}"), or_(A, B))


class PrecedenceTests(unittest.TestCase):
    """perlop: `or` < `and` < `not` < `||` < `&&` < `!`."""

    def test_double_pipe_binds_tighter_than_and(self):
        # `A || B and C` is `(A || B) and C`.
        self.assertEqual(
            conds.compile_cond("$$self{A} || $$self{B} and $$self{C}"), and_(or_(A, B), C)
        )

    def test_and_binds_tighter_than_or(self):
        # `A or B and C` is `A or (B and C)`.
        self.assertEqual(
            conds.compile_cond("$$self{A} or $$self{B} and $$self{C}"), or_(A, and_(B, C))
        )
        self.assertEqual(
            conds.compile_cond("$$self{A} and $$self{B} or $$self{C}"), or_(and_(A, B), C)
        )

    def test_double_pipe_groups_under_and(self):
        # `A and B || C` is `A and (B || C)`.
        self.assertEqual(
            conds.compile_cond("$$self{A} and $$self{B} || $$self{C}"), and_(A, or_(B, C))
        )

    def test_double_ampersand_binds_tighter_than_double_pipe(self):
        # `A && B || C` is `(A && B) || C`.
        self.assertEqual(
            conds.compile_cond("$$self{A} && $$self{B} || $$self{C}"), or_(and_(A, B), C)
        )

    def test_not_binds_tighter_than_and_and_or(self):
        # `not A and B` is `(not A) and B`; `not A or B` is `(not A) or B`
        # (Sony.pm's `not $$self{MetaVersion} or $$self{MetaVersion} ne ...`).
        self.assertEqual(conds.compile_cond("not $$self{A} and $$self{B}"), and_(NOT_A, B))
        self.assertEqual(conds.compile_cond("not $$self{A} or $$self{B}"), or_(NOT_A, B))

    def test_bang_binds_tighter_than_everything(self):
        self.assertEqual(conds.compile_cond("!$$self{A} || $$self{B}"), or_(NOT_A, B))
        self.assertEqual(conds.compile_cond("!$$self{A} && $$self{B}"), and_(NOT_A, B))
        self.assertEqual(conds.compile_cond("! $$self{A} and $$self{B}"), and_(NOT_A, B))


class DefinedTests(unittest.TestCase):
    def test_olympus_focusinfo_0x1600(self):
        # Olympus.pm:3621.
        self.assertEqual(
            conds.compile_cond("not defined $$self{ImageStabilization}"),
            'Cond::MemberDefined { member: "ImageStabilization", negate: true }',
        )

    def test_every_spelling(self):
        self.assertEqual(conds.compile_cond("defined $$self{X}"), DEFINED_X)
        self.assertEqual(conds.compile_cond("defined($$self{X})"), DEFINED_X)
        self.assertEqual(conds.compile_cond("defined ($$self{X})"), DEFINED_X)
        self.assertEqual(conds.compile_cond("defined $self->{X}"), DEFINED_X)
        self.assertEqual(conds.compile_cond("!defined $$self{X}"), NOT_DEFINED_X)
        self.assertEqual(conds.compile_cond("!defined($$self{X})"), NOT_DEFINED_X)
        self.assertEqual(conds.compile_cond("not defined($$self{X})"), NOT_DEFINED_X)

    def test_defined_combines_at_named_unary_precedence(self):
        # Sony.pm's `... and defined $$self{AFAreaILCA} and $$self{AFAreaILCA} != 8`.
        got = conds.compile_cond("$$self{A} and defined $$self{X} and $$self{X} != 8")
        self.assertEqual(
            got,
            and_(A, and_(DEFINED_X, 'Cond::MemberCmp { member: "X", op: CmpOp::Ne, value: 8 }')),
        )
        self.assertEqual(
            conds.compile_cond("!defined($$self{X}) || $count != 1"), or_(NOT_DEFINED_X, COUNT_NE_1)
        )

    def test_nested_hash_member_is_refused(self):
        # Exif.pm's Composite `not defined $$self{VALUE}{DateTimeOriginal}`
        # reads a nested hash the schema does not model.
        self.assertIsNone(conds.compile_cond("not defined $$self{VALUE}{DateTimeOriginal}"))


class RegexSpellingTests(unittest.TestCase):
    def test_m_slash_is_the_match_operator(self):
        # Olympus.pm:3467 (FocusInfo 0x31b), trailing space included.
        self.assertEqual(conds.compile_cond("$$self{Model} =~ m/^E-M|^OM-/ "),
                         'Cond::MemberRegex { member: "Model", pattern: "^E-M|^OM-", '
                         "ignore_case: false, negate: false }")
        self.assertEqual(conds.compile_cond("$$self{Model} !~ m/^E-M/i"),
                         'Cond::MemberRegex { member: "Model", pattern: "^E-M", '
                         "ignore_case: true, negate: true }")

    def test_other_delimiters_are_refused(self):
        self.assertIsNone(conds.compile_cond("$$self{Model} =~ m{^E-M}"))
        self.assertIsNone(conds.compile_cond("$$self{Model} =~ m(^E-M)"))
        self.assertIsNone(conds.compile_cond("$$valPt =~ m{^http://ns.adobe.com/xmp/extension/\\0}"))


class RefusalTests(unittest.TestCase):
    """Everything outside the closed set stays refused -- and says why."""

    def assertRefused(self, condition, reason_fragment):  # noqa: N802 - unittest style
        self.assertIsNone(conds.compile_cond(condition), condition)
        reason = conds.refusal_reason(condition)
        self.assertIsNotNone(reason, condition)
        self.assertIn(reason_fragment, reason, condition)

    def test_parenthesised_grouping_compiles(self):
        # Slice I-4. Grouping restarts the precedence ladder, so
        # `(A or B) and C` is `And(Or(A, B), C)` -- not the `Or(A, And(B, C))`
        # the bare chain gives. Canon.pm's `CanonCameraInfoPowerShot` is the
        # real carrier; verify_cond.py probes both against the Perl.
        src = conds.compile_cond('$format eq "int32u" and ($count == 138 or $count == 148)')
        self.assertIsNotNone(src)
        self.assertTrue(src.startswith("Cond::And(&Cond::FormatEq"), src)
        self.assertIn("Cond::Or(&Cond::CountCmp", src)
        grouped = conds.compile_cond("($$self{A} or $$self{B}) and $$self{C}")
        self.assertIsNotNone(grouped)
        self.assertTrue(grouped.startswith("Cond::And(&Cond::Or("), grouped)
        # An atom may contain its own parentheses; the scanner tracks depth
        # and does not cut the atom at an inner `)`.
        self.assertIsNotNone(conds.compile_cond("defined($$self{X}) and $count == 2"))

    def test_a_side_effect_inside_a_group_still_refuses(self):
        # The safety property grouping must not break: Sony.pm's Tag9400a
        # assigns a data member inside the group. Compiling the syntax while
        # dropping the assignment would be a silent wrong answer, so the
        # assignment atom is refused and the whole group with it.
        self.assertRefused(
            "$$valPt =~ /^[\\x07]/ or ($$valPt =~ /^[\\x5e]/ and $$self{DoubleCipher} = 1)",
            "unrecognised condition atom",
        )
        self.assertRefused(
            "$count <= 25 and $count != 21 and $$self{AEInfoSize} = $count",
            "unrecognised condition atom",
        )

    def test_not_over_a_parenthesised_group(self):
        # Negation is a per-atom flag in this schema, not a node, so a `not`
        # over a group is refused rather than mis-grouped as `(not A) or B`.
        self.assertRefused("not ($$self{A} or $$self{B})", "outside the grammar")

    def test_not_over_a_double_pipe_chain(self):
        # Perl: `not A || B` is `not (A || B)`; `(not A) || B` would be wrong.
        self.assertRefused("not $$self{A} || $$self{B}", "`not` over a `||`/`&&` chain")
        self.assertRefused("not $$self{A} && $$self{B}", "`not` over a `||`/`&&` chain")

    def test_negation_over_a_comparison_or_regex(self):
        # `!$$self{X} == 1` is `(!$$self{X}) == 1` in Perl -- not the
        # negation of the comparison, and not something the schema carries.
        self.assertRefused("!$$self{X} == 1", "only a bare member or a `defined` test negates")
        self.assertRefused("not $$self{Model} =~ /E-M/", "only a bare member or a `defined` test negates")
        self.assertRefused("not $count == 1", "only a bare member or a `defined` test negates")

    def test_set_member_tail_with_a_top_level_or(self):
        # `(X = 1) and A or B` is `((X = 1) and A) or B` in Perl.
        self.assertRefused("($$self{X} = 1) and $$self{A} or $$self{B}", "top-level `or`")

    def test_other_perl(self):
        # `xor` is not a connective the tokenizer knows, so the whole text is
        # one (unrecognisable) atom -- refused at the atom, as `_tokenize`'s
        # docstring says a parenthesised group is.
        self.assertRefused("$$self{A} xor $$self{B}", "unrecognised condition atom")
        self.assertRefused('GetByteOrder() eq "MM"', "unrecognised condition atom")
        self.assertRefused('$$self{FirmwareVersion} lt "02.00"', "unrecognised condition atom")
        self.assertRefused("$$self{Model} =~ /EOS R\\d/", "outside the vetted subset")
        # `and`/`or` are connectives only with a space on both sides, so a
        # trailing one is part of an (unrecognisable) atom; `||`/`&&` need
        # no spaces, so a trailing one dangles.
        self.assertRefused("$$self{A} and", "unrecognised condition atom")
        self.assertRefused("$$self{A} ||", "dangling connective")
        self.assertRefused("$$self{A} &&", "dangling connective")
        self.assertRefused("or $$self{A}", "unrecognised condition atom")

    def test_refusal_reason_is_none_for_a_compiling_condition(self):
        self.assertIsNone(conds.refusal_reason("$$self{A} || $$self{B}"))
        self.assertIsNone(conds.refusal_reason(None))


if __name__ == "__main__":
    unittest.main()
