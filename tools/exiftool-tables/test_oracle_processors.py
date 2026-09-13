"""Independent `NATIVE_PROCESSOR` oracle records for PROCESS_PROC tables."""

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
ORACLE = REPO_ROOT / "tools/exiftool-tables/oracle.pl"


class OracleProcessorFacts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = Path(self.tmp.name) / "lib"
        package = self.lib / "Image/ExifTool"
        package.mkdir(parents=True)
        (self.lib / "Image/ExifTool.pm").write_text(
            "package Image::ExifTool; our $VERSION = 'fixture'; our %specialTags = map { $_ => 1 } qw(PROCESS_PROC GROUPS FORMAT FIRST_ENTRY); 1;\n",
            encoding="utf-8",
        )
        self.fixture = package / "Fixture.pm"
        self.reader = package / "Reader.pm"
        self.later = package / "ZZLater.pm"
        self.write_fixture()
        self.reader.write_text(textwrap.dedent("""\
            package Image::ExifTool::Reader;
            sub Get32u { return 0x00100000; }
            1;
        """), encoding="utf-8")
        # Oracle module order is lexical, so this rebind happens after Reader.
        self.later.write_text(textwrap.dedent("""\
            package Image::ExifTool::ZZLater;
            *Image::ExifTool::Fixture::Get16u = \\&Image::ExifTool::Reader::Get32u;
            1;
        """), encoding="utf-8")

    def write_fixture(self):
        self.fixture.write_text(textwrap.dedent("""\
            package Image::ExifTool::Fixture;
            sub Get16u { return 0x0010; }
            sub ProcessWords {
                my ($et, $dir, $table) = @_;
                my $word = Get16u($dir->{DataPt}, $dir->{DirStart});
                return $word;
            }
            our %Main = (
                PROCESS_PROC => \\&ProcessWords,
                GROUPS => { 0 => 'EXIF', 1 => 'Fixture', 2 => 'Image' },
                FORMAT => 'int8u',
                1 => {
                    Name => 'Word', Format => 'int8u', Count => 1,
                    Condition => '$val == 1', RawConv => '$val',
                    ValueConv => '$val', PrintConv => { 1 => 'One' },
                    Flags => { Unknown => 1, Priority => 3 },
                    SubDirectory => { TagTable => 'Fixture::Child', Start => 2 },
                },
                2 => [
                    { Name => 'Variant', Format => 'int16u' },
                    { Format => 'int8u', Count => undef },
                ],
            );
            our %Empty = ( PROCESS_PROC => \\&ProcessWords, GROUPS => { 0 => 'EXIF' } );
            1;
        """), encoding="utf-8")

    def facts(self):
        result = subprocess.run(
            ["/usr/bin/perl", str(ORACLE), str(self.lib)],
            check=True, text=True, capture_output=True,
        )
        processors, tables, rows = {}, {}, {}
        for line in result.stdout.splitlines():
            pieces = line.split("\t")
            if len(pieces) == 4 and pieces[0] == "NATIVE_PROCESSOR":
                processors[(pieces[1], pieces[2])] = json.loads(pieces[3])
            elif len(pieces) == 4 and pieces[0] == "NATIVE_PROCESSOR_TABLE":
                tables[(pieces[1], pieces[2])] = json.loads(pieces[3])
            elif len(pieces) == 6 and pieces[0] == "NATIVE_PROCESSOR_ROW":
                rows[(pieces[1], pieces[2], pieces[3], pieces[4])] = json.loads(pieces[5])
        return processors, tables, rows

    def test_oracle_records_every_processor_after_later_rebinding(self):
        processors, _tables, _rows = self.facts()
        fact = processors[("Fixture", "Main")]
        self.assertEqual(fact["__perl"], "CODE")
        self.assertTrue(fact["resolved"])
        self.assertEqual(fact["__name"], "Image::ExifTool::Fixture::ProcessWords")
        self.assertEqual(fact["source_file"], "Image/ExifTool/Fixture.pm")
        self.assertEqual(fact["source_sha256"], hashlib.sha256(self.fixture.read_bytes()).hexdigest())
        binding = fact["dependencies"]["Image::ExifTool::Fixture::Get16u"]
        self.assertTrue(binding["resolved"])
        self.assertEqual(binding["__name"], "Image::ExifTool::Reader::Get32u")
        self.assertEqual(binding["source_file"], "Image/ExifTool/Reader.pm")


    def test_table_and_row_streams_preserve_variants_properties_and_zero_rows(self):
        _processors, tables, rows = self.facts()
        main = tables[("Fixture", "Main")]
        self.assertEqual(main["row_record_count"], 3)
        self.assertEqual(main["named_row_count"], 2)
        self.assertTrue(main["groups"]["present"])
        self.assertEqual(main["format"]["value"], {"kind": "scalar", "value": "int8u"})
        self.assertEqual(tables[("Fixture", "Empty")]["row_record_count"], 0)
        self.assertEqual(tables[("Fixture", "Empty")]["named_row_count"], 0)

        plain = rows[("Fixture", "Main", "1", "-")]
        self.assertEqual(plain["name"], "Word")
        self.assertEqual(plain["properties"]["Count"],
                         {"present": True, "value": {"kind": "scalar", "value": "1"}})
        self.assertEqual(plain["properties"]["RawConv"]["value"]["value"], "$val")
        self.assertEqual(plain["effective_flags"]["Unknown"], {"kind": "scalar", "value": "1"})
        self.assertEqual(plain["expanded_properties"]["Priority"], {"kind": "scalar", "value": "3"})
        self.assertEqual(plain["properties"]["SubDirectory"]["value"]["map"]["Start"],
                         {"kind": "scalar", "value": "2"})
        self.assertEqual(rows[("Fixture", "Main", "2", "0")]["name"], "Variant")
        unnamed = rows[("Fixture", "Main", "2", "1")]
        self.assertIsNone(unnamed["name"])
        self.assertTrue(unnamed["properties"]["Count"]["present"])
        self.assertEqual(unnamed["properties"]["Count"]["value"], {"kind": "undef"})

    def test_oracle_preserves_unresolved_bare_reader(self):
        self.later.write_text("package Image::ExifTool::ZZLater; 1;\n", encoding="utf-8")
        text = self.fixture.read_text(encoding="utf-8")
        self.fixture.write_text(text.replace("sub Get16u { return 0x0010; }\n", "", 1), encoding="utf-8")
        processors, _tables, _rows = self.facts()
        fact = processors[("Fixture", "Main")]
        binding = fact["dependencies"]["Image::ExifTool::Fixture::Get16u"]
        self.assertFalse(binding["resolved"])
        self.assertEqual(binding["reason"], "code_ref_unavailable")


if __name__ == "__main__":
    unittest.main()
