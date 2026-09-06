"""Grammar-level tests for exprs.py: which conversion texts compile, to what,
and -- just as important -- which are refused.

These pin the SHAPE of the translation (domain, Rust type, emitted call);
the VALUES are the differential oracle's job (verify_exprs.py against the
pinned Perl), never a unit test's. Run with
`python3 -m unittest discover -s tools/exiftool-tables -p 'test*.py'`.
"""
import unittest

import exprs


class ConvertUnixTimeGrammar(unittest.TestCase):
    """ExifTool.pm:6784-6810 in both spellings; `$toLocal` only as the
    literal 1; option-reading second arguments refused."""

    def test_fully_qualified_with_epoch_shift_and_local(self):
        r = exprs.translate_or_compile_any("Image::ExifTool::ConvertUnixTime($val + 631065600, 1)")
        self.assertEqual(r[0], "num")
        self.assertEqual(r[1], "String")
        self.assertIn("exprs::convert_unix_time(", r[2])
        self.assertTrue(r[2].endswith(", true)"), r[2])
        self.assertIn("631065600.0_f64", r[2])

    def test_fully_qualified_gmtime(self):
        r = exprs.translate_or_compile_any("Image::ExifTool::ConvertUnixTime($val + 631065600)")
        self.assertTrue(r[2].endswith(", false)"), r[2])

    def test_bare_name_both_forms(self):
        self.assertEqual(
            exprs.translate_or_compile_any("ConvertUnixTime($val)"),
            ("num", "String", "crate::exiftool_tables::exprs::convert_unix_time({v}, false)"),
        )
        self.assertEqual(
            exprs.translate_or_compile_any("ConvertUnixTime($val,1)"),
            ("num", "String", "crate::exiftool_tables::exprs::convert_unix_time({v}, true)"),
        )

    def test_filetime_idiom_is_a_translations_entry(self):
        r = exprs.translate_or_compile_any("$val=$val/1e7-11644473600; ConvertUnixTime($val,1)")
        self.assertEqual(r[0], "num")
        self.assertEqual(
            r[2],
            "crate::exiftool_tables::exprs::convert_unix_time({v} / 1e7 - 11644473600.0, true)",
        )

    def test_option_reading_second_argument_is_refused(self):
        # QuickTime.pm's `%timeInfo` block reads option state the generated
        # table does not have; it must stay refused, not default to UTC.
        self.assertIsNone(exprs.translate_or_compile_any(
            'ConvertUnixTime($val, $self->Options("QuickTimeUTC") || $$self{FileType} eq "CR3")'
        ))

    def test_unpinned_literal_arguments_are_refused(self):
        self.assertIsNone(exprs.translate_or_compile_any("ConvertUnixTime($val, 0)"))
        self.assertIsNone(exprs.translate_or_compile_any("ConvertUnixTime($val, 1, 3)"))
        self.assertIsNone(exprs.translate_or_compile_any("ConvertUnixTime($val, 2)"))


class BytesDomainShapes(unittest.TestCase):
    def test_unpack_hex_both_spacings(self):
        for text in ('unpack("H*",$val)', 'unpack("H*", $val)'):
            self.assertEqual(
                exprs.translate_or_compile_any(text),
                ("bytes", "String", "crate::exiftool_tables::exprs::unpack_hex({v})"),
                text,
            )

    def test_unpack_other_templates_are_refused(self):
        self.assertIsNone(exprs.translate_or_compile_any('unpack("H2",$val)'))
        self.assertIsNone(exprs.translate_or_compile_any('unpack("H*", $val) . "x"'))

    def test_get_guid_both_spellings(self):
        want = ("bytes", "String", "crate::exiftool_tables::exprs::asf_get_guid({v})")
        self.assertEqual(exprs.translate_or_compile_any("Image::ExifTool::ASF::GetGUID($val)"), want)
        self.assertEqual(
            exprs.translate_or_compile_any("require Image::ExifTool::ASF; Image::ExifTool::ASF::GetGUID($val)"),
            want,
        )


class ParenlessSprintfG(unittest.TestCase):
    def test_bare_percent_g_is_six_significant_digits(self):
        self.assertEqual(
            exprs.translate_or_compile_any('sprintf "%g", $val'),
            ("num", "String", "crate::exiftool_tables::exprs::perl_g_spec({v}, 6, false)"),
        )

    def test_other_paren_less_forms_stay_refused(self):
        # Only the one census spelling has an entry; the grammar itself still
        # requires the parenthesised call.
        self.assertIsNone(exprs.translate_or_compile_any('sprintf "%.2f", $val'))


if __name__ == "__main__":
    unittest.main()
