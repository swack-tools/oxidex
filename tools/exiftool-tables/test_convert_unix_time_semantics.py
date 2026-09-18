#!/usr/bin/env python3
"""ConvertUnixTime port selection (exprs.CONVERT_UNIX_TIME_SEMANTICS).

ExifTool 11.78/12.64 and 13.59 carry different `ConvertUnixTime` subs: the
old one truncates toward zero after a 1e-6 nudge, the new one floors and
rounds half-to-even. The 11.78 upgrade rehearsal's verify_exprs.py run failed
10 expressions (77 probes) because every translation compiled to the 13.59
port. These tests pin that the port is chosen from the pinned tree's SOURCE,
that an unrecognised body refuses rather than guesses, and that codegen
compiles with the port the oracle ledger was proven with.
"""

import hashlib
import json
import re
import tempfile
import unittest
from pathlib import Path

import codegen
import exprs

REPO = Path(__file__).resolve().parents[2]

# ExifTool 11.78's lib/Image/ExifTool.pm, verbatim (12.64 is byte-identical).
SUB_11_78 = 'sub ConvertUnixTime($;$$)\n{\n    my ($time, $toLocal, $dec) = @_;\n    return \'0000:00:00 00:00:00\' if $time == 0;\n    my (@tm, $tz);\n    if ($dec) {\n        my $frac = $time - int($time);\n        $time = int($time);\n        $frac < 0 and $frac += 1, $time -= 1;\n        $dec = sprintf(\'%.*f\', $dec, $frac);\n        # remove number before decimal and increment integer time if it was rounded up\n        $dec =~ s/^(\\d)// and $1 eq \'1\' and $time += 1;\n    } else {\n        $time = int($time + 1e-6) if $time != int($time);  # avoid round-off errors\n        $dec = \'\';\n    }\n    if ($toLocal) {\n        @tm = localtime($time);\n        $tz = TimeZoneString(\\@tm, $time);\n    } else {\n        @tm = gmtime($time);\n        $tz = \'\';\n    }\n    my $str = sprintf("%4d:%.2d:%.2d %.2d:%.2d:%.2d$dec%s",\n                      $tm[5]+1900, $tm[4]+1, $tm[3], $tm[2], $tm[1], $tm[0], $tz);\n    return $str;\n}\n'
# The pinned ExifTool 13.59's lib/Image/ExifTool.pm, verbatim.
SUB_13_59 = 'sub ConvertUnixTime($;$$)\n{\n    my ($time, $toLocal, $dec) = @_;\n    return \'0000:00:00 00:00:00\' if $time == 0;\n    my (@tm, $tz, $trim);\n    $dec = $static_vars{SystemTimeRes} || 0 unless defined $dec;\n    $dec < 0 and $dec = -$dec, $trim = 1;\n    my $itime = int($time);\n    my $frac = $time - $itime;\n    $frac < 0 and $frac += 1, $itime -= 1;\n    $dec = sprintf(\'%.*f\', $dec, $frac);\n    # remove number before decimal and increment integer time if necessary\n    $dec =~ s/^(\\d)// and $1 eq \'1\' and $itime += 1;\n    $dec =~ s/\\.?0+$// if $trim;    # trim trailing zeros if specified\n    if (not $toLocal) {\n        @tm = gmtime($itime);\n        $tz = \'\';\n    } elsif ($static_vars{KeepUTCTime}) {\n        @tm = gmtime($itime);\n        $tz = \'Z\';\n    } else {\n        @tm = localtime($itime);\n        $tz = TimeZoneString(\\@tm, $itime);\n    }\n    my $str = sprintf("%4d:%.2d:%.2d %.2d:%.2d:%.2d$dec%s",\n                      $tm[5]+1900, $tm[4]+1, $tm[3], $tm[2], $tm[1], $tm[0], $tz);\n    return $str;\n}\n'

FILETIME = "$val=$val/1e7-11644473600; ConvertUnixTime($val,1)"
MAC_EPOCH = "ConvertUnixTime($val - ((66 * 365 + 17) * 24 * 3600))"


def write_tree(root, sub_text):
    pm = Path(root) / "Image" / "ExifTool.pm"
    pm.parent.mkdir(parents=True)
    pm.write_text("package Image::ExifTool;\n# preamble\n" + sub_text
                  + "\n#-----\nsub Other { 1 }\n1;\n", encoding="latin-1")
    return root


class Selection(unittest.TestCase):
    def setUp(self):
        self.addCleanup(exprs.set_convert_unix_time_semantics,
                        exprs.DEFAULT_CONVERT_UNIX_TIME_SEMANTICS)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)

    def detect(self, sub_text, name):
        return exprs.detect_convert_unix_time_semantics(write_tree(self.tmp / name, sub_text))[0]

    def test_each_release_body_selects_its_own_port(self):
        self.assertEqual(self.detect(SUB_11_78, "old"), "trunc_epsilon")
        self.assertEqual(self.detect(SUB_13_59, "new"), "floor_round")
        # Comment and whitespace edits are not semantic.
        reflowed = SUB_11_78.replace("    ", "\t").replace(
            "# avoid round-off errors", "# reworded comment")
        self.assertEqual(self.detect(reflowed, "reflowed"), "trunc_epsilon")

    def test_any_code_change_selects_nothing(self):
        edited = SUB_11_78.replace("1e-6", "1e-7")
        self.assertIsNone(self.detect(edited, "edited"))
        self.assertIsNone(self.detect("sub Other { 1 }", "absent"))
        self.assertEqual(exprs.detect_convert_unix_time_semantics(self.tmp / "no-tree"),
                         (None, None))

    def test_translations_follow_the_selected_port(self):
        for name, fn in (("trunc_epsilon", "convert_unix_time_trunc_epsilon"),
                         ("floor_round", "convert_unix_time")):
            exprs.set_convert_unix_time_semantics(name)
            for expr in (FILETIME, MAC_EPOCH):
                code = exprs.translate_or_compile_any(expr)[2]
                self.assertRegex(code, rf"crate::exiftool_tables::exprs::{fn}\(")
                other = {"convert_unix_time", "convert_unix_time_trunc_epsilon"} - {fn}
                self.assertNotRegex(code, rf"::{other.pop()}\(")

    def test_unproven_source_refuses_every_convert_unix_time(self):
        exprs.set_convert_unix_time_semantics(None)
        self.assertIsNone(exprs.translate_or_compile_any(FILETIME))
        self.assertIsNone(exprs.translate_or_compile_any(MAC_EPOCH))
        self.assertIsNone(exprs.translate_or_compile_any("Image::ExifTool::ConvertUnixTime($val)"))
        # Nothing unrelated is refused with it.
        self.assertIsNotNone(exprs.translate_or_compile_any("$val / 10"))

    def test_every_port_exists_in_the_rust_helpers(self):
        rust = (REPO / "src" / "exiftool_tables" / "exprs.rs").read_text(encoding="utf-8")
        for fn, _body in exprs.CONVERT_UNIX_TIME_SEMANTICS.values():
            self.assertRegex(rust, rf"pub fn {fn}\(time: f64, to_local: bool\) -> String")

    @unittest.skipUnless(Path("/tmp/oxidex-exiftool-cache/exiftool/lib").is_dir(),
                         "pinned 13.59 tree not cached on this host")
    def test_pinned_tree_is_the_default_port(self):
        name, _ = exprs.detect_convert_unix_time_semantics("/tmp/oxidex-exiftool-cache/exiftool/lib")
        self.assertEqual(name, exprs.DEFAULT_CONVERT_UNIX_TIME_SEMANTICS)


class LedgerCarriesThePort(unittest.TestCase):
    def setUp(self):
        self.addCleanup(exprs.set_convert_unix_time_semantics,
                        exprs.DEFAULT_CONVERT_UNIX_TIME_SEMANTICS)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tables = Path(tmp.name) / "tables.json"
        self.tables.write_text(json.dumps({"exiftool_version": "11.78", "modules": {}}))
        self.ledger = Path(tmp.name) / "ledger.json"

    def load(self, **extra):
        ledger = {
            "schema": codegen.LEDGER_SCHEMA, "exiftool_version": "11.78",
            "perl_version": "v5.38.2",
            "tables_sha256": hashlib.sha256(self.tables.read_bytes()).hexdigest(),
            "probe_counts": {"pass": 1, "fail": 0, "skip": 0},
            "verified_expressions": [MAC_EPOCH], **extra,
        }
        self.ledger.write_text(json.dumps(ledger))
        return codegen.load_oracle_ledger(str(self.ledger), str(self.tables), "11.78")

    def test_recorded_port_is_applied(self):
        self.load(helper_semantics={"ConvertUnixTime": "trunc_epsilon"})
        self.assertIn("convert_unix_time_trunc_epsilon(",
                      exprs.translate_or_compile_any(MAC_EPOCH)[2])

    def test_absent_field_means_the_default_port(self):
        exprs.set_convert_unix_time_semantics("trunc_epsilon")
        self.load()
        self.assertRegex(exprs.translate_or_compile_any(MAC_EPOCH)[2],
                         r"::convert_unix_time\(")

    def test_recorded_refusal_is_applied(self):
        self.load(helper_semantics={"ConvertUnixTime": None})
        self.assertIsNone(exprs.translate_or_compile_any(MAC_EPOCH))

    def test_unknown_port_is_refused_loudly(self):
        for bad in ({"ConvertUnixTime": "round_up"}, {"PrintFraction": "x"}, ["trunc_epsilon"]):
            with self.assertRaises(SystemExit) as ctx:
                self.load(helper_semantics=bad)
            self.assertIn("helper_semantics", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
