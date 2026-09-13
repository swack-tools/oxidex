"""Deferred source facts for table PROCESS_PROC code references.

The dump table owns the processor CV, while a bare helper call resolves through
its package glob when invoked.  These tests use only copied fixture Perl to
keep the two facts independently observable: a later module replaces the
package-local reader after the table module loaded.
"""

import hashlib
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


class ProcessProcessorFacts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = Path(self.tmp.name) / "lib"
        package = self.lib / "Image/ExifTool"
        package.mkdir(parents=True)
        (self.lib / "Image/ExifTool.pm").write_text(
            "package Image::ExifTool; our $VERSION = 'fixture'; 1;\n",
            encoding="utf-8",
        )
        self.fixture = package / "Fixture.pm"
        self.reader = package / "Reader.pm"
        self.later = package / "ZZLater.pm"
        self.write_fixture()
        self.write_reader("return 0x0010;")
        self.write_later("*Image::ExifTool::Fixture::Get16u = \\&Image::ExifTool::Reader::Get32u;")

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

    def write_reader(self, body):
        self.reader.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::Reader;
            sub Get16u {{ return 0x0010; }}
            sub Get32u {{ {body} }}
            1;
        """), encoding="utf-8")

    def write_later(self, statement):
        self.later.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::ZZLater;
            {statement}
            1;
        """), encoding="utf-8")

    def dump(self):
        result = subprocess.run(
            [PERL, str(DUMP), str(self.lib), "Fixture", "Reader", "ZZLater"],
            check=True, text=True, capture_output=True,
        )
        return json.loads(result.stdout)

    def processor(self, doc=None):
        doc = doc or self.dump()
        return doc["modules"]["Fixture"]["tables"]["Main"]["meta"]["PROCESS_PROC"]

    def test_table_meta_uses_actual_processor_and_final_package_binding(self):
        fact = self.processor()
        self.assertEqual(fact["__perl"], "CODE")
        self.assertTrue(fact["__opaque"])
        self.assertTrue(fact["resolved"])
        self.assertEqual(fact["__name"], "Image::ExifTool::Fixture::ProcessWords")
        self.assertEqual(fact["source_file"], "Image/ExifTool/Fixture.pm")
        self.assertEqual(fact["source_sha256"], hashlib.sha256(self.fixture.read_bytes()).hexdigest())
        # ZZLater is loaded last.  The table still owns ProcessWords, but its
        # bare Get16u call dispatches through the final Fixture package glob.
        reader = fact["dependencies"]["Image::ExifTool::Fixture::Get16u"]
        self.assertTrue(reader["resolved"])
        self.assertEqual(reader["__name"], "Image::ExifTool::Reader::Get32u")
        self.assertEqual(reader["source_file"], "Image/ExifTool/Reader.pm")
        self.assertEqual(reader["source_sha256"], hashlib.sha256(self.reader.read_bytes()).hexdigest())

    def test_processor_edit_changes_fact_without_changing_named_row_data(self):
        before = self.dump()
        text = self.fixture.read_text(encoding="utf-8")
        self.fixture.write_text(text.replace("return $word;", "return $word + 1;", 1), encoding="utf-8")
        after = self.dump()
        first, second = self.processor(before), self.processor(after)
        self.assertNotEqual(first["__deparse"], second["__deparse"])
        self.assertNotEqual(first["source_sha256"], second["source_sha256"])
        self.assertEqual(before["modules"]["Fixture"]["tables"]["Main"]["tags"],
                         after["modules"]["Fixture"]["tables"]["Main"]["tags"])

    def test_missing_bare_reader_is_explicit_unresolved_dependency(self):
        self.write_later("")
        text = self.fixture.read_text(encoding="utf-8")
        self.fixture.write_text(text.replace("sub Get16u { return 0x0010; }\n", "", 1), encoding="utf-8")
        fact = self.processor()
        reader = fact["dependencies"]["Image::ExifTool::Fixture::Get16u"]
        self.assertFalse(reader["resolved"])
        self.assertEqual(reader["reason"], "code_ref_unavailable")
        self.assertIsNone(reader["__deparse"])
        self.assertIsNone(reader["source_file"])
        self.assertIsNone(reader["source_sha256"])

    def test_processor_outside_selected_library_is_unresolved(self):
        outside = Path(self.tmp.name) / "outside"
        external = outside / "Image/ExifTool"
        external.mkdir(parents=True)
        (external / "External.pm").write_text(textwrap.dedent("""\
            package Image::ExifTool::External;
            sub ProcessWords { return 1; }
            1;
        """), encoding="utf-8")
        self.fixture.write_text(textwrap.dedent("""\
            package Image::ExifTool::Fixture;
            use lib '""" + str(outside) + """';
            use Image::ExifTool::External;
            our %Main = ( PROCESS_PROC => \\&Image::ExifTool::External::ProcessWords, 1 => { Name => 'Word' } );
            1;
        """), encoding="utf-8")
        fact = self.processor()
        self.assertFalse(fact["resolved"])
        self.assertEqual(fact["reason"], "source_outside_selected_lib")
        self.assertIsNone(fact["source_file"])
        self.assertIsNone(fact["source_sha256"])


PINNED = Path(os.environ.get("OXIDEX_PINNED_EXIFTOOL", REPO_ROOT / "target/exiftool-src" / ("exiftool-" + (REPO_ROOT / ".exiftool-version").read_text().strip())))
ORACLE = REPO_ROOT / "tools/exiftool-tables/oracle.pl"


@unittest.skipUnless((PINNED / "lib/Image/ExifTool/CanonCustom.pm").is_file(),
                     "pinned ExifTool 13.59 source unavailable")
class PinnedCanonCustom(unittest.TestCase):
    """Copied pinned source proves local glob rebinding reaches both paths."""

    def dump(self, lib):
        result = subprocess.run(
            [PERL, str(DUMP), str(lib), "CanonCustom"],
            check=True, text=True, capture_output=True,
        )
        return json.loads(result.stdout)["modules"]["CanonCustom"]["tables"]["Functions5D"]["meta"]["PROCESS_PROC"]

    def oracle(self, lib):
        result = subprocess.run(
            [PERL, str(ORACLE), str(lib)],
            check=True, text=True, capture_output=True,
        )
        for line in result.stdout.splitlines():
            p = line.split("\t", 3)
            if p[:3] == ["NATIVE_PROCESSOR", "CanonCustom", "Functions5D"] and len(p) == 4:
                return json.loads(p[3])
        self.fail("missing CanonCustom Functions5D native processor record")

    def test_local_get16u_rebind_changes_dump_and_independent_oracle_facts(self):
        original = self.dump(PINNED / "lib")
        self.assertEqual(original["__name"], "Image::ExifTool::CanonCustom::ProcessCanonCustom")
        self.assertEqual(original["dependencies"]["Image::ExifTool::CanonCustom::Get16u"]["__name"],
                         "Image::ExifTool::Get16u")

        copied = tempfile.TemporaryDirectory()
        self.addCleanup(copied.cleanup)
        copied_lib = Path(copied.name) / "lib"
        import shutil
        shutil.copytree(PINNED / "lib", copied_lib)
        source = copied_lib / "Image/ExifTool/CanonCustom.pm"
        text = source.read_text(encoding="utf-8")
        marker = "\n\n1;  # end\n"
        self.assertIn(marker, text)
        source.write_text(text.replace(marker,
            "\n\n*Image::ExifTool::CanonCustom::Get16u = \\&Image::ExifTool::Get32u;\n\n1;  # end\n", 1),
            encoding="utf-8")

        rebound = self.dump(copied_lib)
        self.assertEqual(rebound["__deparse"], original["__deparse"])
        reader = rebound["dependencies"]["Image::ExifTool::CanonCustom::Get16u"]
        self.assertTrue(reader["resolved"])
        self.assertEqual(reader["__name"], "Image::ExifTool::Get32u")
        self.assertNotEqual(rebound["source_sha256"], original["source_sha256"])

        independent = self.oracle(copied_lib)
        self.assertEqual(independent["__name"], "Image::ExifTool::CanonCustom::ProcessCanonCustom")
        self.assertEqual(independent["__deparse"], rebound["__deparse"])
        self.assertEqual(independent["source_sha256"], rebound["source_sha256"])
        oracle_reader = independent["dependencies"]["Image::ExifTool::CanonCustom::Get16u"]
        self.assertTrue(oracle_reader["resolved"])
        self.assertEqual(oracle_reader["__name"], "Image::ExifTool::Get32u")


if __name__ == "__main__":
    unittest.main()
