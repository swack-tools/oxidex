"""Probe-shaping tests for verify_exprs.py: the oracle proves a translation
only over the probes this module builds, so the probe set is part of the
instrument. Each test pins that a form's distinguishing inputs are actually
generated -- a first-element form on lists whose first element differs from
the rest, a trailing-whitespace form on inputs with and without the
whitespace -- and that the literal escaping on both harness sides survives
the control characters those probes carry. Values are never asserted here;
that is the oracle's job. Run with
`python3 -m unittest discover -s tools/exiftool-tables -p 'test*.py'`.
"""
import json
import tempfile
import unittest
from pathlib import Path

import verify_exprs


class ElementCount(unittest.TestCase):
    def test_sized_binary_format(self):
        self.assertEqual(verify_exprs.element_count({"Format": "int16s[4]"}, "int16u"), 4)
        # `string[16]` / `undef[N]` are one scalar of N bytes
        self.assertEqual(verify_exprs.element_count({"Format": "string[16]"}, None), 1)
        self.assertEqual(verify_exprs.element_count({"Format": "undef[8]"}, None), 1)
        self.assertEqual(verify_exprs.element_count({}, "int16u"), 1)

    def test_ifd_count_beside_a_numeric_writable(self):
        # Olympus::Main RedBalance / Pentax::Main ExposureCompensation
        self.assertEqual(verify_exprs.element_count({"Writable": "int16u", "Count": "2"}, None), 2)
        self.assertEqual(verify_exprs.element_count({"Writable": "int16u", "Count": 2}, None), 2)
        # a string with a Count is still one scalar (Olympus::Equipment SerialNumber)
        self.assertEqual(verify_exprs.element_count({"Writable": "string", "Count": "32"}, None), 1)
        # no Count, a variable count, a count of one: no declared list
        self.assertEqual(verify_exprs.element_count({"Writable": "rational64u"}, None), 1)
        self.assertEqual(verify_exprs.element_count({"Writable": "int32u", "Count": "-1"}, None), 1)
        self.assertEqual(verify_exprs.element_count({"Writable": "int32u", "Count": "1"}, None), 1)
        self.assertEqual(verify_exprs.element_count({"Format": "int32u", "Writable": "0"}, "string"), 1)

    def test_census_carries_the_ifd_count(self):
        dump = {
            "exiftool_version": "13.59",
            "modules": {"Olympus": {"tables": {"Main": {"meta": {}, "tags": {
                "4119": {"Name": "RedBalance", "Writable": "int16u", "Count": "2",
                         "ValueConv": {"kind": "expr", "expr": "$val=~s/ .*//; $val / 256"}},
                "519": {"Name": "CameraType", "Writable": "string",
                        "ValueConv": {"kind": "expr", "expr": "$val =~ s/\\s+$//; $val"}},
            }}}}},
        }
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "t.json"
            path.write_text(json.dumps(dump), encoding="utf-8")
            version, counter, counts = verify_exprs.census(str(path))
        self.assertEqual(version, "13.59")
        self.assertEqual(counter["$val=~s/ .*//; $val / 256"], 1)
        self.assertEqual(counts["$val=~s/ .*//; $val / 256"], {2})
        self.assertEqual(counts["$val =~ s/\\s+$//; $val"], {1})


class FirstElementProbes(unittest.TestCase):
    def test_multi_element_lists_with_distinct_first_elements(self):
        probes = verify_exprs.list_probes_for("$val=~s/ .*//; $val / 256", {2})
        sizes = {len(p) for p in probes}
        self.assertTrue({1, 2, 3} <= sizes, sizes)
        # both orders of a pair, so "first" is told apart from last/min/max
        self.assertIn([7.0, 3.0], probes)
        self.assertIn([3.0, 7.0], probes)
        self.assertIn([65535.0, 0.0], probes)
        self.assertIn([0.0, 65535.0], probes)
        self.assertIn([3.0, 7.0, 11.0], probes)
        # the mined literal still broadcasts, and the short/long lists remain
        self.assertIn([256.0, 256.0], probes)
        self.assertIn([0.0], probes)
        # no duplicates
        self.assertEqual(len(probes), len({tuple(p) for p in probes}))

    def test_no_declared_count_still_probes_lists(self):
        # Samsung.pm:458 declares no Count: the multi-element shape must not
        # depend on the carrying field saying so.
        probes = verify_exprs.list_probes_for("$val=~s/ .*//; $val", None)
        self.assertTrue({1, 2, 3} <= {len(p) for p in probes})
        self.assertIn([12345.0, 99.0], probes)

    def test_helper_boundaries_ride_in_the_first_element(self):
        probes = verify_exprs.list_probes_for("$val =~ s/ .*//; ConvertUnixTime($val)", None)
        for v in verify_exprs._HELPER_BOUNDARY_PROBES["ConvertUnixTime"]:
            self.assertIn([v, 1.0], probes, v)

    def test_other_list_forms_are_unchanged(self):
        probes = verify_exprs.list_probes_for('sprintf("%d %d", split(" ",$val))', {2})
        self.assertEqual({len(p) for p in probes}, {1, 2, 3})  # count, short, long
        self.assertNotIn([7.0, 3.0], probes)


class TrimProbes(unittest.TestCase):
    def test_trim_form_gets_its_own_battery(self):
        probes = verify_exprs.probes_for("str", "$val =~ s/\\s+$//; $val")
        self.assertIs(probes, verify_exprs.STR_PROBES_TRIM)
        # with and without trailing whitespace, tabs, a bare word, empty
        for want in ("abc ", "abc\t", "abc", "a", "", "SX151 ", "abc \t\n\r\x0b\x0c", " abc"):
            self.assertIn(want, probes, repr(want))
        # every probe is ASCII: non-ASCII would test the harness's output
        # encoding, not the conversion (see BYTES_PROBES_RAW)
        self.assertTrue(all(ord(c) < 0x80 for p in probes for c in p))

    def test_other_str_forms_keep_their_batteries(self):
        self.assertIs(verify_exprs.probes_for("str", "$self->ConvertDateTime($val)"),
                      verify_exprs.STR_PROBES_CDT)
        self.assertIs(verify_exprs.probes_for("str", "$val =~ tr/-/:/; $val"),
                      verify_exprs.STR_PROBES_TR)


class LiteralEscaping(unittest.TestCase):
    def test_perl_double_quoted_body(self):
        self.assertEqual(verify_exprs.perl_escape('a$b@c"d\\e'), 'a\\$b\\@c\\"d\\\\e')
        # braced hex for every control character: `\0` then a digit would be octal
        self.assertEqual(verify_exprs.perl_escape("\x00" + "1"), "\\x{00}1")
        self.assertEqual(verify_exprs.perl_escape("abc \t\n\r\x0b\x0c"),
                         "abc \\x{09}\\x{0a}\\x{0d}\\x{0b}\\x{0c}")
        self.assertEqual(verify_exprs.perl_escape("2024-01-02 10:20:30"), "2024-01-02 10:20:30")

    def test_rust_str_literal(self):
        # a raw CR is a compile error inside a Rust string literal
        self.assertEqual(verify_exprs.rust_str_literal('a"b\\c\x00\t\r\n\x0b'),
                         '"a\\"b\\\\c\\u{0}\\t\\r\\n\\u{b}"')

    def test_perl_side_sets_the_probe_verbatim(self):
        script = verify_exprs.build_perl_script([(0, "str", "$val =~ s/\\s+$//; $val", "abc \r\n")], "/lib")
        self.assertIn('my $val = "abc \\x{0d}\\x{0a}";', script)
        # a CR in a RESULT is escaped like an LF, so one result stays one line
        self.assertIn("$s =~ s/\\r/\\\\r/g;", script)
        joined = verify_exprs.build_perl_script([(1, "list", "$val=~s/ .*//; $val / 256", [7.0, 3.0])], "/lib")
        self.assertIn('my $val = "7 3";', joined)


if __name__ == "__main__":
    unittest.main()
