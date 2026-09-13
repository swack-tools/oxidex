"""Shared ProcessBinaryData string-layout schema tests.

These exercise ExifTool's table grammar, not Canon routing: the same default
stride, fixed string, and bare-remainder forms are emitted from source data.
"""

import collections
import unittest

import codegen


def native_table(tags):
    return {
        "meta": {
            "PROCESS_PROC": {"__name": "Image::ExifTool::ProcessBinaryData"},
            "FORMAT": "string",
        },
        "tags": tags,
    }


class RemainderStringSchema(unittest.TestCase):
    def test_default_string_stride_fixed_string_and_remainder_are_distinct(self):
        output = codegen.gen_table(
            "Example",
            "MakeModel",
            native_table({
                "0": {"Name": "DefaultStride"},
                "1": {"Name": "Make", "Format": "string[6]"},
                "7": {"Name": "Model", "Format": "string"},
            }),
            collections.Counter(),
            set(),
            [],
        )
        self.assertIsNotNone(output)
        self.assertIn("default_format: Fmt::Str(1)", output)
        self.assertIn('name: "DefaultStride", format: None, count: 1', output)
        self.assertIn('name: "Make", format: Some(Fmt::Str(6)), count: 1', output)
        self.assertIn(
            'name: "Model", format: Some(Fmt::RemainderString), count: 1', output
        )
        self.assertNotIn("Fmt::Str(0)", output)

    def test_bare_string_does_not_depend_on_a_string_table_default(self):
        source, _, reason = codegen.gen_field_literal(
            {"Name": "Tail", "Format": "string"},
            12,
            None,
            collections.Counter(),
            None,
            "int8u",
            set(),
        )
        self.assertEqual(reason, None)
        self.assertIn("format: Some(Fmt::RemainderString)", source)
        self.assertNotIn("Fmt::Str(0)", source)

    def test_binary_format_and_count_conditions_remain_omitted(self):
        # ProcessBinaryData's GetTagInfo retry supplies raw valPt only; the
        # native fixture's `$format` and `$count` are undef at this point.
        output = codegen.gen_table(
            "Example",
            "Context",
            native_table({
                "0": {"Name": "FormatDependent", "Condition": '$format eq "string"'},
                "1": {"Name": "CountDependent", "Condition": "$count == 1"},
            }),
            collections.Counter(),
            set(),
            [],
        )
        self.assertIsNotNone(output)
        self.assertNotIn("condition: Some(Cond::Format", output)
        self.assertNotIn("condition: Some(Cond::Count", output)
        self.assertEqual(output.count("condition: None"), 2)
        self.assertEqual(output.count("condition: true"), 2)

    def test_native_exe_debug_rsds_shape_uses_the_same_remainder_format(self):
        # EXE.pm:332-343 is a non-Canon ProcessBinaryData carrier: the
        # explicit `string` field starts at 24 while the table default is the
        # ordinary int8u fallback. No EXE-specific branch belongs in codegen.
        output = codegen.gen_table(
            "EXE",
            "DebugRSDS",
            {
                "meta": {
                    "PROCESS_PROC": {"__name": "Image::ExifTool::ProcessBinaryData"},
                },
                "tags": {
                    "20": {"Name": "PDBAge", "Format": "int32u"},
                    "24": {"Name": "PDBFileName", "Format": "string"},
                },
            },
            collections.Counter(),
            set(),
            [],
        )
        self.assertIsNotNone(output)
        self.assertIn("default_format: Fmt::Int8u", output)
        self.assertIn('index: 24, sub: None, name: "PDBFileName"', output)
        self.assertIn("format: Some(Fmt::RemainderString)", output)


if __name__ == "__main__":
    unittest.main()
