"""Pinned 13.59 Minolta OTHER bodies across B::Deparse formatting versions.

PERL_534 and the other two registry inputs come from dump_tables.pl using
macOS /usr/bin/perl 5.34.1. PERL_538 is the actual Ubuntu CI failure body from
run 34519240103, job 103012303989. Expectations name the existing Rust forms;
this test does not derive its inputs or expectations from the registry.
"""
import unittest

import gen_minolta_a100_tables as generator

PERL_534 = r'''{
    package Image::ExifTool::Minolta;
    use strict;
    (my($val, $inv) = @_);
    ($inv and (return (undef)));
    (my $id = ($val & 65280));
    (my $mb = $Image::ExifTool::Minolta::metabonesID{$id});
    if ($mb) {
        (ref($mb) or (($id = $mb), ($mb = $Image::ExifTool::Minolta::metabonesID{$id})));
        (require Image::ExifTool::Canon);
        (my $lens = $Image::ExifTool::Canon::canonLensTypes{$val - $id});
        ($lens and (return ("$lens + $$mb")));
    } elsif (($val >= 18688)) {
        (require Image::ExifTool::Sigma);
        (my $lens = $Image::ExifTool::Sigma::sigmaLensTypes{$val - 18688});
        ($lens and (return ("$lens + MC-11 SA-E")));
    }
    (return (undef));
}'''

PERL_538 = r'''{ package Image::ExifTool::Minolta; use strict; (my($val, $inv) = @_); ($inv and (return (undef))); (my($id) = ($val & 65280)); (my($mb) = $Image::ExifTool::Minolta::metabonesID{$id}); if ($mb) { (ref($mb) or (($id = $mb), ($mb = $Image::ExifTool::Minolta::metabonesID{$id}))); (require Image::ExifTool::Canon); (my($lens) = $Image::ExifTool::Canon::canonLensTypes{$val - $id}); ($lens and (return ("$lens + $$mb"))); } elsif (($val >= 18688)) { (require Image::ExifTool::Sigma); (my($lens) = $Image::ExifTool::Sigma::sigmaLensTypes{$val - 18688}); ($lens and (return ("$lens + MC-11 SA-E"))); } (return (undef)); }'''

FOCUS_BODY = r'''{
    package Image::ExifTool::Minolta;
    use strict;
    (my($val, $inv) = @_);
    ($inv and (($val =~ /([-+]?\d+)/), (return $1)));
    (return (($val < 0) ? ("Front Focus ($val)") : ("Back Focus (+$val)")));
}'''

PARAMETER_BODY = r'''($$$) {
    package Image::ExifTool::Exif;
    use strict;
    (my($val, $inv, $conv) = @_);
    ($inv and (return $val));
    if (($val > 0)) {
        if (($val > 65520)) {
            ($val = ($val - 65536));
        } else {
            ($val = "+$val");
        }
    }
    (return $val);
}'''


class MinoltaDeparseTests(unittest.TestCase):
    def translate(self, body):
        pools = generator.Pools()
        tag = {"PrintConv": {
            "kind": "enum_partial", "map": {"1": "Known lens"},
            "directives": {"OTHER": {"__perl": "CODE", "__deparse": body}},
        }}
        result = generator.translate_pc("WBInfoA100", "18877", "LensType", tag, pools)
        self.assertEqual(pools.maps, [(("1", "Known lens"),)])
        return result

    def test_registered_perl_534_spelling_is_preserved(self):
        self.assertEqual(self.translate(PERL_534), "Pc::Map(M0, Other::MinoltaLens)")

    def test_observed_perl_538_spelling_is_accepted(self):
        self.assertEqual(self.translate(PERL_538), "Pc::Map(M0, Other::MinoltaLens)")

    def test_outside_string_line_wrapping_is_allowed(self):
        wrapped = PERL_538.replace("; ", ";\n\t")
        self.assertEqual(self.translate(wrapped), "Pc::Map(M0, Other::MinoltaLens)")

    def test_other_registered_bodies_still_translate(self):
        for body, expected in ((FOCUS_BODY, "MinoltaFocus"),
                               (PARAMETER_BODY, "ExifParameter")):
            with self.subTest(expected=expected):
                self.assertEqual(self.translate(body), f"Pc::Map(M0, Other::{expected})")
                wrapped = body.replace(";\n", ";\n\t\n")
                self.assertNotEqual(wrapped, body)
                self.assertEqual(self.translate(wrapped), f"Pc::Map(M0, Other::{expected})")

    def test_changed_semantics_are_still_rejected(self):
        for original in (PERL_534, PERL_538):
            for old, new in (("$val & 65280", "$val | 65280"),
                             ("$val >= 18688", "$val >= 18689"),
                             ("MC-11 SA-E", "MC-11 SA-X")):
                with self.subTest(original=original[:60], change=(old, new)):
                    changed = original.replace(old, new)
                    self.assertNotEqual(changed, original)
                    with self.assertRaises(generator.Unsupported):
                        self.translate(changed)

    def test_string_whitespace_is_not_normalized_away(self):
        for original in (PERL_534, PERL_538):
            for altered in ("MC-11  SA-E", "MC-11\tSA-E", "MC-11\nSA-E"):
                with self.subTest(altered=repr(altered)):
                    with self.assertRaises(generator.Unsupported):
                        self.translate(original.replace("MC-11 SA-E", altered))

    def test_other_registry_string_whitespace_is_also_preserved(self):
        with self.assertRaises(generator.Unsupported):
            self.translate(FOCUS_BODY.replace("Back Focus", "Back  Focus"))


if __name__ == "__main__":
    unittest.main()
