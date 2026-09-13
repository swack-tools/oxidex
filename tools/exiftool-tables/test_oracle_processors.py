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
            "package Image::ExifTool; our $VERSION = 'fixture'; our %specialTags; 1;\n",
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
                1 => { Name => 'Word' },
            );
            1;
        """), encoding="utf-8")

    def facts(self):
        result = subprocess.run(
            ["/usr/bin/perl", str(ORACLE), str(self.lib)],
            check=True, text=True, capture_output=True,
        )
        records = {}
        for line in result.stdout.splitlines():
            pieces = line.split("\t", 3)
            if len(pieces) == 4 and pieces[0] == "NATIVE_PROCESSOR":
                records[(pieces[1], pieces[2])] = json.loads(pieces[3])
        return records

    def test_oracle_records_every_processor_after_later_rebinding(self):
        fact = self.facts()[("Fixture", "Main")]
        self.assertEqual(fact["__perl"], "CODE")
        self.assertTrue(fact["resolved"])
        self.assertEqual(fact["__name"], "Image::ExifTool::Fixture::ProcessWords")
        self.assertEqual(fact["source_file"], "Image/ExifTool/Fixture.pm")
        self.assertEqual(fact["source_sha256"], hashlib.sha256(self.fixture.read_bytes()).hexdigest())
        binding = fact["dependencies"]["Image::ExifTool::Fixture::Get16u"]
        self.assertTrue(binding["resolved"])
        self.assertEqual(binding["__name"], "Image::ExifTool::Reader::Get32u")
        self.assertEqual(binding["source_file"], "Image/ExifTool/Reader.pm")

    def test_oracle_preserves_unresolved_bare_reader(self):
        self.later.write_text("package Image::ExifTool::ZZLater; 1;\n", encoding="utf-8")
        text = self.fixture.read_text(encoding="utf-8")
        self.fixture.write_text(text.replace("sub Get16u { return 0x0010; }\n", "", 1), encoding="utf-8")
        fact = self.facts()[("Fixture", "Main")]
        binding = fact["dependencies"]["Image::ExifTool::Fixture::Get16u"]
        self.assertFalse(binding["resolved"])
        self.assertEqual(binding["reason"], "code_ref_unavailable")


if __name__ == "__main__":
    unittest.main()
