"""Source facts retained for the QuickTime ItemList `data` reader contract."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
DUMP = REPO_ROOT / "tools/exiftool-tables/dump_tables.pl"
PERL = os.environ.get("EXIFTOOL_PERL", "/usr/bin/perl")


class QuickTimeReaderProtocolFacts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        package = Path(self.tmp.name) / "lib/Image/ExifTool"
        package.mkdir(parents=True)
        self.lib = package.parents[1]
        (self.lib / "Image/ExifTool.pm").write_text(textwrap.dedent("""\
            package Image::ExifTool;
            our $VERSION = 'fixture';
            sub ReadValue { return $_[2]; }
            sub Decode { return $_[1]; }
            1;
        """), encoding="utf-8")
        (package / "Charset.pm").write_text(textwrap.dedent("""\
            package Image::ExifTool::Charset;
            sub Decompose { return []; }
            sub Recompose { return ''; }
            1;
        """), encoding="utf-8")
        self.quicktime = package / "QuickTime.pm"
        self.write_quicktime("return 'int8u';", "3 => 'ShiftJIS'")

    def write_quicktime(self, format_body, encoding_row):
        self.quicktime.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::QuickTime;
            use Image::ExifTool;
            our %stringEncoding = (1 => 'UTF8', 2 => 'UTF16', {encoding_row});
            sub QuickTimeFormat {{ {format_body} }}
            sub ProcessMOV {{ return QuickTimeFormat(0, 1); }}
            our %ItemList = (
                GROUPS => {{ 1 => 'ItemList' }}, FORMAT => 'string',
                PROCESS_PROC => \\&ProcessMOV,
                abcd => {{ Name => 'Example' }},
            );
            1;
        """), encoding="utf-8")

    def dump(self):
        result = subprocess.run(
            [PERL, str(DUMP), str(self.lib), "QuickTime"],
            check=True, text=True, capture_output=True,
        )
        return json.loads(result.stdout)["quicktime_itemlist_reader_protocol"]

    def test_captures_data_and_effective_helper_bodies(self):
        fact = self.dump()
        self.assertEqual(fact["kind"], "quicktime_itemlist_reader_protocol_v1")
        self.assertEqual(fact["string_encoding"]["3"], "ShiftJIS")
        self.assertTrue(fact["charset_loaded"])
        self.assertEqual(fact["dependencies"]["quicktime_format"]["__name"],
                         "Image::ExifTool::QuickTime::QuickTimeFormat")
        self.assertEqual(fact["dependencies"]["read_value"]["__name"],
                         "Image::ExifTool::ReadValue")
        self.assertEqual(fact["dependencies"]["decode"]["__name"],
                         "Image::ExifTool::Decode")

    def test_helper_or_encoding_change_changes_protocol_fact(self):
        before = self.dump()
        self.write_quicktime("return 'int16u';", "3 => 'UTF8'")
        after = self.dump()

        self.assertNotEqual(before["dependencies"]["quicktime_format"]["__deparse"],
                            after["dependencies"]["quicktime_format"]["__deparse"])
        self.assertNotEqual(before["string_encoding"], after["string_encoding"])


if __name__ == "__main__":
    unittest.main()
