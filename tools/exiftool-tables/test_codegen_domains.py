"""codegen.py's input-domain rules for a tag's ValueConv/PrintConv pair.

A PrintConv reads the ValueConv's OUTPUT (GetValue queues ValueConv ahead of
PrintConv, ExifTool.pm:3524-3525), so its domain follows the compiled
ValueConv's Rust type, not the field's Format. Run with
`python3 -m unittest discover -s tools/exiftool-tables -p 'test*.py'`.
"""
import unittest

import codegen


def _tag(fmt, value_conv=None, print_conv=None):
    tag = {"Name": "T", "Format": fmt}
    if value_conv is not None:
        tag["ValueConv"] = {"kind": "expr", "expr": value_conv}
    if print_conv is not None:
        tag["PrintConv"] = {"kind": "expr", "expr": print_conv}
    return tag


class PrintConvInputDomain(unittest.TestCase):
    def test_string_valued_value_conv_makes_the_print_conv_str_domain(self):
        # int32u field, ConvertUnixTime -> String, then ConvertDateTime (str).
        tag = _tag("int32u", "ConvertUnixTime($val)", "$self->ConvertDateTime($val)")
        self.assertEqual(codegen.print_conv_input_domain(tag, "num", True), "str")

    def test_numeric_value_conv_keeps_the_print_conv_numeric(self):
        tag = _tag("int16u", "$val / 10", 'sprintf("%.1f",$val)')
        self.assertEqual(codegen.print_conv_input_domain(tag, "num", True), "num")
        # A bytes-domain field whose ValueConv hands on a number reads as a
        # number in the PrintConv too.
        self.assertEqual(codegen.print_conv_input_domain(tag, "bytes", True), "num")

    def test_option_valued_value_conv_is_numeric(self):
        tag = _tag("int16u", "$val ? $val : undef", "$val")
        self.assertEqual(codegen.print_conv_input_domain(tag, "num", True), "num")

    def test_unmodeled_or_absent_value_conv_keeps_the_field_domain(self):
        self.assertEqual(codegen.print_conv_input_domain(_tag("undef"), "bytes", False), "bytes")
        # Modeled flag false -> the ValueConv text is not even consulted.
        tag = _tag("int32u", "ConvertUnixTime($val)", "$self->ConvertDateTime($val)")
        self.assertEqual(codegen.print_conv_input_domain(tag, "num", False), "num")
        # A code-ref ValueConv (no expr text) with the flag true, defensively.
        tag = {"Name": "T", "Format": "string", "ValueConv": {"kind": "code", "deparse": "..."}}
        self.assertEqual(codegen.print_conv_input_domain(tag, "str", True), "str")

    def test_bytes_value_conv_returning_a_string(self):
        tag = _tag("undef[16]", "Image::ExifTool::ASF::GetGUID($val)", "$val")
        self.assertEqual(codegen.print_conv_input_domain(tag, "bytes", True), "str")

    def test_list_value_conv_returning_a_string_feeds_a_str_print_conv(self):
        # ICC_Profile::Header ProfileDateTime: int16u[6] -> sprintf over the
        # split -> String -> `$self->ConvertDateTime($val)` (str identity).
        tag = _tag("int16u[6]", 'sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d",split(" ",$val));',
                   "$self->ConvertDateTime($val)")
        self.assertEqual(codegen.print_conv_input_domain(tag, "list", True), "str")


class ValueDomain(unittest.TestCase):
    def test_scalar_formats(self):
        self.assertEqual(codegen.value_domain("int16u", 1), "num")
        self.assertEqual(codegen.value_domain("string[4]", 1), "str")
        self.assertEqual(codegen.value_domain("undef[16]", 1), "bytes")

    def test_fixed_count_numeric_is_the_list_domain(self):
        for fmt in ("int16u[4]", "int32u[33]", "int8u[16]", "int16s[5]", "fixed32s[3]", "float[2]"):
            base = fmt.split("[")[0]
            want = "list" if base in codegen.SCALAR_FORMATS else None
            self.assertEqual(codegen.value_domain(fmt, 4), want, fmt)
        # codegen passes the base with the count separately as well
        self.assertEqual(codegen.value_domain("int16u", 4), "list")

    def test_unknown_base_with_a_count_stays_refused(self):
        self.assertIsNone(codegen.value_domain("var_string[3]", 3))


if __name__ == "__main__":
    unittest.main()
