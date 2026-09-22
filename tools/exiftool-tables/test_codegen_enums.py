"""codegen.py's plain-hash PrintConv emission (4b-i).

ExifTool renders a hash miss as `"Unknown ($val)"`, or `sprintf('Unknown
(0x%x)', $val)` when the TAG carries `PrintHex` and `IsInt($val)`
(ExifTool.pm:3624-3631). The runtime does the rendering; the one schema fact
it needs is the tag-level PrintHex, which `conv_for` routes as below. Run with
`python3 -m unittest discover -s tools/exiftool-tables -p 'test*.py'`.
"""
import collections
import unittest

import codegen
import others


def _stats():
    stats = collections.Counter()
    stats["pc_directives_dropped"] = collections.Counter()
    stats["other_unregistered_bodies"] = collections.Counter()
    return stats


def _enum_tag(mapping, print_hex=False):
    tag = {
        "Name": "T",
        "Format": "int16u",
        "PrintConv": {"kind": "enum", "map": mapping, "directives": None},
    }
    if print_hex:
        tag["PrintHex"] = 1
    return tag


class PlainHashPrintConv(unittest.TestCase):
    def test_int_hash_without_printhex_is_an_int_enum(self):
        stats = _stats()
        src, refused = codegen.conv_for(_enum_tag({"1": "One", "2": "Two"}), stats, "num", set())
        self.assertEqual(src, 'PrintConv::IntEnum(&[(1, "One"), (2, "Two")])')
        self.assertFalse(refused)
        self.assertEqual(stats["enum_int"], 1)
        self.assertEqual(stats["enum_int_printhex"], 0)

    def test_int_hash_with_printhex_carries_print_hex_through_partial_enum_int(self):
        stats = _stats()
        src, refused = codegen.conv_for(_enum_tag({"1": "One"}, print_hex=True), stats, "num", set())
        self.assertEqual(
            src, 'PrintConv::PartialEnumInt { exact: &[(1, "One")], other: None, print_hex: true }'
        )
        self.assertFalse(refused)
        self.assertEqual(stats["enum_int_printhex"], 1)
        self.assertEqual(stats["enum_int"], 0)

    def test_string_hash_without_printhex_is_a_str_enum(self):
        stats = _stats()
        src, refused = codegen.conv_for(_enum_tag({"APPL": "Apple", "": ""}), stats, "str", set())
        self.assertEqual(src, 'PrintConv::StrEnum(&[("", ""), ("APPL", "Apple")])')
        self.assertFalse(refused)
        self.assertEqual(stats["enum_str"], 1)

    def test_string_hash_with_printhex_is_refused_and_withheld(self):
        # `IsInt($val)` on a string-keyed hash is a property of the runtime
        # value; the schema cannot promise the hex form, so omit and count.
        stats = _stats()
        src, refused = codegen.conv_for(_enum_tag({"1": "One", "x": "Ex"}, print_hex=True), stats, "str", set())
        self.assertEqual(src, "PrintConv::None")
        self.assertTrue(refused)
        self.assertEqual(stats["enum_str_printhex_refused"], 1)
        self.assertEqual(stats["enum_str"], 0)


class FixedArrayPatternPrintConv(unittest.TestCase):
    def stacked_tag(self):
        return {
            "Name": "StackedImage",
            "Writable": "int32u",
            "Count": 2,
            "PrintConv": {
                "kind": "enum_partial",
                "map": {
                    "0 0": "No",
                    "1 *": "Live Composite (* images)",
                    "3 2": "ND2 (1EV)",
                    "9 *": "Focus-stacked (* images)",
                },
                "directives": {
                    "OTHER": {
                        "__perl": "CODE",
                        "__deparse": others.OLYMPUS_STACKED_IMAGE_OTHER,
                    },
                },
            },
        }

    def test_exact_pinned_olympus_identity_emits_source_map_pattern(self):
        stats = _stats()
        src, refused = codegen.conv_for(
            self.stacked_tag(),
            stats,
            "list",
            set(),
            source_identity=("Olympus", "CameraSettings", 0x0804),
        )
        self.assertEqual(
            src,
            'PrintConv::StrEnum(&[("\\u{1f}oxidex-fixed-array-pattern-v1", ""), '
            '("0 0", "No"), ("1 *", "Live Composite (* images)"), '
            '("3 2", "ND2 (1EV)"), ("9 *", "Focus-stacked (* images)")])',
        )
        self.assertFalse(refused)
        self.assertEqual(stats["other_fixed_array_pattern"], 1)

    def test_changed_source_identity_or_shape_is_refused(self):
        mutations = (
            ("table", ("Olympus", "Main", 0x0804)),
            ("tag_id", ("Olympus", "CameraSettings", 0x0805)),
            ("name", ("Olympus", "CameraSettings", 0x0804)),
            ("writable", ("Olympus", "CameraSettings", 0x0804)),
            ("count", ("Olympus", "CameraSettings", 0x0804)),
            ("deparse", ("Olympus", "CameraSettings", 0x0804)),
            ("literal_star", ("Olympus", "CameraSettings", 0x0804)),
        )
        for mutation, identity in mutations:
            with self.subTest(mutation=mutation):
                tag = self.stacked_tag()
                if mutation == "name":
                    tag["Name"] = "NotStackedImage"
                elif mutation == "writable":
                    tag["Writable"] = "int16u"
                elif mutation == "count":
                    tag["Count"] = 3
                elif mutation == "deparse":
                    tag["PrintConv"]["directives"]["OTHER"]["__deparse"] += " "
                elif mutation == "literal_star":
                    tag["PrintConv"]["map"]["1 *"] = "Literal star"
                stats = _stats()
                src, _refused = codegen.conv_for(
                    tag,
                    stats,
                    "list",
                    set(),
                    source_identity=identity,
                )
                self.assertEqual(src, "PrintConv::None")
                self.assertEqual(stats["other_fixed_array_pattern"], 0)


if __name__ == "__main__":
    unittest.main()
