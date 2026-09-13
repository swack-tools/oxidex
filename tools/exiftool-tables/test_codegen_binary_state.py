"""Source-driven schema tests for shared binary state and Conditions."""

import collections
import unittest

import codegen


def stats():
    got = collections.Counter()
    for key in (
        "unsupported_exprs",
        "pc_directives_dropped",
        "other_unregistered_bodies",
        "value_conv_refused_expressions",
        "dropped_code_refs",
        "hook_refusal_reasons",
    ):
        got[key] = collections.Counter()
    return got


class BinaryStateSchema(unittest.TestCase):
    def test_set_member_and_standalone_condition_are_data_but_legacy_omits_them(self):
        source, _, reason = codegen.gen_field_literal(
            {
                "Name": "Locations",
                "Format": "int8u",
                "RawConv": {"kind": "expr", "expr": "$$self{Locations} = $val"},
                "Condition": "$$self{Prior} >= 2",
            },
            1,
            None,
            stats(),
            None,
            "int8u",
            set(),
        )
        self.assertIn(
            'condition: Some(Cond::MemberCmp { member: "Prior", op: CmpOp::Ge, value: 2 })',
            source,
        )
        self.assertIn(
            'raw_conv: Some(RawConvEffect::SetMember { member: "Locations" })',
            source,
        )
        self.assertIn("raw_conv: true, condition: true", source)
        self.assertIsNone(reason)

    def test_uncompiled_standalone_condition_remains_an_explicit_refusal(self):
        source, _, reason = codegen.gen_field_literal(
            {"Name": "T", "Condition": "unsupported($val)"},
            0,
            None,
            stats(),
            None,
            "int8u",
            set(),
        )
        self.assertIn("condition: None", source)
        self.assertIn("condition: true", source)
        self.assertIsNone(reason)

    def test_sidecar_reason_is_the_actual_unknown_refusal_not_a_reclassification(self):
        source, _, reason = codegen.gen_field_literal(
            {"Name": "Reserved", "Unknown": 1, "Format": "undef[120]"},
            0, None, stats(), None, "int8u", set(),
        )
        self.assertIsNone(source)
        self.assertEqual(reason, "unknown")
        self.assertEqual(codegen.omitted_native_reasons(reason), ("unknown",))

    def test_variant_refusal_carries_its_atomic_condition_cause(self):
        source, _, reason = codegen.compile_variant_group(
            {"_variants": [
                {"Name": "First", "Condition": "unsupported($val)"},
                {"Name": "Second", "Condition": "$$val == 2"},
            ]},
            1, None, stats(), None, "int8u", set(),
        )
        self.assertIsNone(source)
        self.assertEqual(reason, "condition")

    def test_literal_prefix_value_local_raw_conv_is_data_but_stays_withheld(self):
        source, _, _ = codegen.gen_field_literal(
            {
                "Name": "Track",
                "RawConv": {
                    "kind": "expr",
                    "expr": "($val =~ s/^0 // and $val) ? $val : undef",
                },
            },
            125,
            None,
            stats(),
            None,
            "int8u",
            set(),
        )
        self.assertIn("raw_conv: Some(RawConvEffect::ValueLocal)", source)
        self.assertIn("raw_conv: true", source)

    def test_another_plain_literal_prefix_is_value_local_without_producer_rules(self):
        source, _, _ = codegen.gen_field_literal(
            {
                "Name": "Other",
                "RawConv": {
                    "kind": "expr",
                    "expr": "($val =~ s/ACME-42_ // and $val) ? $val : undef",
                },
            },
            126,
            None,
            stats(),
            None,
            "int8u",
            set(),
        )
        self.assertIn("raw_conv: Some(RawConvEffect::ValueLocal)", source)
        self.assertIn("raw_conv: true", source)

    def test_only_complete_value_local_expression_avoids_the_opaque_barrier(self):
        for expression in (
            "$val =~ s/^0 //",
            "helper($val)",
            "$$self{Unsafe} = helper($val)",
            "($val =~ s/^0 // and helper($val)) ? $val : undef",
            "($val =~ s/^$prefix// and $val) ? $val : undef",
            "($val =~ s/^A.+// and $val) ? $val : undef",
            "($val =~ s/^0 //e and $val) ? $val : undef",
            "($val =~ s/^0 (?{helper()})// and $val) ? $val : undef",
            "($val =~ s/^0 // and $val) ? $val : undef; helper()",
        ):
            with self.subTest(expression=expression):
                source, _, _ = codegen.gen_field_literal(
                    {"Name": "T", "RawConv": {"kind": "expr", "expr": expression}},
                    0,
                    None,
                    stats(),
                    None,
                    "int8u",
                    set(),
                )
                self.assertIn("raw_conv: None", source)
                self.assertIn("raw_conv: true", source)


class IfdStateSchema(unittest.TestCase):
    def test_compiled_direct_ifd_condition_is_available_to_parent_routing(self):
        source, reason = codegen.gen_ifd_tag_literal(
            {"Name": "Child", "Condition": r"$$valPt =~ /^\x01/"},
            0x202A,
            stats(),
            set(),
            {},
            {},
        )
        self.assertIsNone(reason)
        self.assertIn(r'condition: Some(Cond::ValPtRegex { pattern: "^\\x01", negate: false })', source)
        self.assertIn("omitted: Omitted::NONE", source)


if __name__ == "__main__":
    unittest.main()
