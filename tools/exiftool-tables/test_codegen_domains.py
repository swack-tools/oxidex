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


if __name__ == "__main__":
    unittest.main()
