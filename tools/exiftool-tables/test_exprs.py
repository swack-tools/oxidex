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


class Concatenation(unittest.TestCase):
    """Perl's `.` at additive precedence; each side stringified as Perl does."""

    def test_icc_profile_version(self):
        r = exprs.translate_or_compile_any('($val >> 8).".".(($val & 0xf0)>>4).".".($val & 0x0f)')
        self.assertEqual((r[0], r[1]), ("num", "String"))
        # bitwise results print as unsigned digits straight off the u64
        self.assertIn(".to_string()", r[2])

    def test_ternaries_joined(self):
        r = exprs.translate_or_compile_any(
            '($val & 0x01 ? "Embedded, " : "Not Embedded, ") . ($val & 0x02 ? "Not Independent" : "Independent")'
        )
        self.assertEqual((r[0], r[1]), ("num", "String"))

    def test_helper_result_joined_with_literal(self):
        r = exprs.translate_or_compile_any('ConvertDuration($val) . " (approx)"')
        self.assertEqual(r[2], 'format!("{}{}", crate::exiftool_tables::exprs::convert_duration({v}), " (approx)".to_string())')

    def test_numeric_side_uses_perl_stringification(self):
        r = exprs.translate_or_compile_any('"x" . $val')
        self.assertIn("perl_num({v})", r[2])
        r = exprs.translate_or_compile_any('"x" . int($val)')
        self.assertIn("perl_int(", r[2])

    def test_bool_or_undef_operand_is_refused(self):
        self.assertIsNone(exprs.translate_or_compile_any('$val . undef'))
        self.assertIsNone(exprs.translate_or_compile_any('IsInt($val) . "x"'))

    def test_decimal_literals_still_lex_as_numbers(self):
        self.assertEqual(exprs.translate_or_compile_any("$val * .5")[2], "(({v}) * (0.5_f64))")


class ListDomain(unittest.TestCase):
    """Fixed-count fields: `{v}` is `&[f64]`; every pinned shape compiles,
    the unbounded ones stay refused."""

    def compile(self, text):
        r = exprs.translate_or_compile_any(text)
        self.assertIsNotNone(r, text)
        return r

    def test_sprintf_fed_from_split(self):
        r = self.compile('sprintf("%4d %4d %4d (%dK)", split(" ",$val))')
        self.assertEqual((r[0], r[1]), ("list", "String"))
        self.assertEqual(r[2].count("list_get({v}, "), 4)
        # the repetition operator folds at compile time: 3 + 3*10 conversions
        r = self.compile('sprintf("%3d %4d %6d" . " %3d %4d %6d" x 10, split(" ",$val))')
        self.assertEqual(r[2].count("list_get({v}, "), 33)
        r = self.compile('sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d",split(" ",$val));')
        self.assertEqual(r[2].count("list_get({v}, "), 6)

    def test_bound_array_statements_and_interpolation(self):
        r = self.compile('my @v=split(" ",$val); $_*=15 foreach @v; "$v[1] $v[0] $v[3] $v[2]"')
        self.assertEqual((r[0], r[1]), ("list", "String"))
        self.assertIn("let mut __l", r[2])
        self.assertIn("*__x = *__x * (15.0_f64)", r[2])
        self.assertIn("list_elem_str(&__l, 1)", r[2])
        r = self.compile('my @a=split " ",$val;$a[0]*=2;$a[3]*=2;"@a"')
        self.assertIn("list_set(&mut __l, 0, __v)", r[2])
        self.assertIn("list_join(&__l)", r[2])
        r = self.compile('my @a = split " ",$val; $_ /= 0x4000 foreach @a; "@a"')
        self.assertIn("/ (16384.0_f64)", r[2])

    def test_element_map_to_strings(self):
        r = self.compile("my @a = split ' ', $val; $_ = sprintf('%.6f', $_) foreach @a; \"@a\";")
        self.assertIn('format!("{:.6}", x)', r[2])
        self.assertIn('__s.join(" ")', r[2])
        self.assertIn("let __l", r[2])
        self.assertNotIn("let mut __l", r[2])

    def test_element_assign_and_bitmask(self):
        r = self.compile("my @v = split(' ',$val); $v[0] &= 0x0f; $v[1] = $v[2] * 256 + $v[3]; return \"$v[0] $v[1]\";")
        self.assertIn("as u64) & (", r[2])
        self.assertIn("list_set(&mut __l, 1, __v)", r[2])

    def test_icc_device_attributes_and_hex_id(self):
        r = self.compile("my @v = split ' ', $val; ($v[1] & 0x01 ? \"Transparency, \" : \"Reflective, \") . ($v[1] & 0x08 ? \"B&W\" : \"Color\");")
        self.assertEqual((r[0], r[1]), ("list", "String"))
        self.assertEqual(
            self.compile("Image::ExifTool::ICC_Profile::HexID($val)"),
            ("list", "String", "crate::exiftool_tables::exprs::icc_hex_id({v})"),
        )

    def test_list_hex_and_bytes_hex_spellings(self):
        self.assertEqual(self.compile('unpack "H*", pack "C*", split " ", $val')[2],
                         "crate::exiftool_tables::exprs::list_hex({v})")
        self.assertEqual(self.compile('join " ", unpack "H2H2", $val')[0], "bytes")
        self.assertEqual(self.compile('uc unpack "H*", $val')[2],
                         "crate::exiftool_tables::exprs::unpack_hex({v}).to_uppercase()")
        self.assertEqual(self.compile('"0x" . unpack("H*",$val)')[0], "bytes")

    def test_sprintf_literal_percent_is_not_a_conversion(self):
        # `%%` is a literal percent; the first version of the conversion
        # guard refused every `sprintf("%.0f%%", ...)` PrintConv in the
        # tables (36 generated sites went PrintConv::None on a local regen).
        self.assertEqual(
            exprs.translate_or_compile_any('sprintf("%.0f%%",$val)'),
            ("num", "String", 'format!("{:.0}%", {v})'),
        )
        self.assertEqual(
            exprs.translate_or_compile_any('sprintf("%.0f%%",$val*100)')[2],
            'format!("{:.0}%", (({v}) * (100.0_f64)))',
        )
        self.assertIsNone(exprs.translate_or_compile_any('sprintf("%d%c", $val)'))
        self.assertIsNone(exprs.translate_or_compile_any('sprintf("%s", $val)'))

    def test_refusals(self):
        for text in (
            'my @v=reverse split(" ",$val);"@v"',                       # reverse: not modelled
            "my @v = split(' ',$val); $v[0] &= 0x0f; # comment return \"$v[0]\";",  # comment extent unknowable
            'my @a = split " ",$val; sprintf("%d.%d%c",@a)',           # %c not modelled
            'sprintf("%d %s", split(" ",$val))',                        # %s not modelled
            'my @v=split(" ",$val); "$w[0]"',                           # unbound array
            'my @v=split(",",$val); "@v"',                              # not the space split
            'Image::ExifTool::ICC_Profile::HexID($val + 1)',            # helper takes the list itself
        ):
            self.assertIsNone(exprs.translate_or_compile_any(text), text)


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
